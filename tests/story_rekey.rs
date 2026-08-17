//! **S23 — a long-lived connection rekeys itself without the user
//! noticing.**
//!
//! > **Accepts:** the per-direction epoch ratchet advances every 65 536
//! > messages with no handshake, no round trip and no application-visible
//! > event. There is no DH re-handshake; a new handshake from a live static
//! > means replacement (S3b), not rekey.
//! > **Anchor:** §7.7. **Paused clock:** yes.
//!
//! # Why this file is separate from `spec_rekey.rs` (rulings 82, 251)
//!
//! Ruling 82: *"a configurable epoch pins the boundary **behaviour** and a
//! separate constant test pins the **value** — independently, which is the
//! stronger arrangement, since a single test crossing a real boundary would
//! pass just as well against a wrong constant."* Ruling 251 turned that
//! into Appendix B's *"pinned at **both triggers**"*, and this file is the
//! second trigger: the ratified `REKEY_EPOCH_MSGS`, crossed once, by an
//! endpoint on `Config::new()` — **no knob**. `spec_rekey.rs` carries the
//! epoch-distance separators at ruling 82's knob, and the `Rekey()` vector.
//!
//! The two are complementary and neither subsumes the other. A build whose
//! constant was 1 024 instead of 65 536 would satisfy every test in
//! `spec_rekey.rs` — they all configure their own epoch — and fails
//! [`rekey`](s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing)
//! here, whose whole content is *the production endpoint's boundary is
//! where §7.7 says it is*.
//!
//! # Working rule 9 — what this test does and does not separate
//!
//! It **catches**: a wrong `REKEY_EPOCH_MSGS`; a boundary that costs a
//! handshake, a DH, a lost byte or a stalled stream; a counter that resets
//! with the epoch. It does **not** catch the never-rekey build — crossing a
//! boundary invisibly is satisfied for free by a build in which nothing
//! happens (ruling 251), and the assertion that fails that build is
//! `spec_rekey.rs`'s `rk4_a_packet_two_epochs_back_does_not_open`. Splitting
//! them is deliberate: the separating assertion needs a packet held two
//! epochs back, which at the production constant would cost 131 072 seals
//! to construct.
//!
//! # Cost, and why it is gated into the release run
//!
//! Crossing a real boundary means sealing 65 666 packets and 75 MiB of
//! stream data, which is the point — §7.7's constant is only exercised by
//! paying for it. Measured on the base commit (`5806f12`, Apple silicon):
//!
//! | profile | this test | the rest of `--all-features` |
//! |---|---|---|
//! | debug | **13.4 s** | ≈ 18 s |
//! | release | **1.0 s** | — |
//!
//! 13.4 s nearly doubles the debug suite, and Appendix B sanctions the
//! alternative in as many words: *"the production constant […] crossing
//! once — **in the release run if debug-slow**"* (ruling 251). So it is
//! `#[cfg_attr(debug_assertions, ignore)]`, and the release gate
//! (`cargo test --release --all-features`, which every slice ends on) is
//! where it runs. To run it in debug anyway:
//!
//! ```text
//! cargo test --all-features --test story_rekey -- --ignored
//! ```
//!
//! It holds no large buffer: the payload is generated and verified in
//! 64 KiB rounds, and `Tap::drain` keeps the fabric's log from growing to
//! the size of the transfer.
//!
//! Paused clock throughout (`#[tokio::test(start_paused = true)]`), and no
//! sleep: virtual time advances only where the ACK and flow-control timers
//! ask it to.

use std::cell::Cell;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Duration;

use slither::constants::{PKT_DATA, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP, REKEY_EPOCH_MSGS};
use slither::testutil::{Pair, Spied, Tap, TestConnection, local, settle};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// How far past the boundary the transfer runs before it stops.
///
/// Enough that the packets **after** the boundary are a stretch of traffic
/// rather than a single packet that might have been a fluke, and small
/// enough not to pay for a second epoch.
const OVERSHOOT: u64 = 32;

/// Bytes verified per round. Under `INITIAL_MAX_STREAM_DATA` (262 144) so
/// the writer is never blocked for a whole round on stream credit.
const ROUND: usize = 64 * 1024;

/// The stream payload: period 251, which is prime and coprime with every
/// packet size in play, so a transfer that repeated, dropped or reordered
/// any run of bytes differs from this at almost every byte rather than at
/// none.
fn pattern(offset: usize, len: usize) -> Vec<u8> {
    (offset..offset + len).map(|i| (i % 251) as u8).collect()
}

/// What the tap is watched for, accumulated across drains.
///
/// The fabric's log is drained as the transfer runs — 65 537 sealed packets
/// retained in full would cost more memory than the transfer itself — so
/// every property this test asserts about the wire is folded in here as the
/// packets go past, and nothing is asked of the log afterwards.
#[derive(Debug, Default)]
struct Watch {
    /// A's first and last Data counters, and the count.
    first: Option<u64>,
    last: Option<u64>,
    packets: u64,
    /// Counters that did not exceed their predecessor: §7.7's *"the counter
    /// is never reset by the ratchet"*, violated.
    non_monotonic: u64,
    /// Handshake packets from **either** peer: S23's *"no handshake"*.
    handshakes: u64,
}

impl Watch {
    fn ingest(&mut self, spied: &[Spied], a: SocketAddr) {
        for s in spied {
            match s.bytes.first() {
                Some(&PKT_HANDSHAKE_INIT) | Some(&PKT_HANDSHAKE_RESP) => self.handshakes += 1,
                Some(&PKT_DATA) if s.src == a => {
                    let counter =
                        u64::from_le_bytes(s.bytes[6..14].try_into().expect("eight bytes"));
                    if let Some(prev) = self.last
                        && counter <= prev
                    {
                        self.non_monotonic += 1;
                    }
                    self.first.get_or_insert(counter);
                    self.last = Some(counter);
                    self.packets += 1;
                }
                _ => {}
            }
        }
    }

    /// A's highest counter so far, or 0 before it has sealed anything.
    fn highest(&self) -> u64 {
        self.last.unwrap_or(0)
    }
}

/// Send one datagram and read it back at the far end, failing loudly rather
/// than hanging.
async fn round_trip(from: &TestConnection, to: &TestConnection, payload: &[u8], what: &str) {
    from.send_datagram(payload).expect("send_datagram");
    settle().await;
    match tokio::time::timeout(Duration::from_secs(5), to.recv_datagram()).await {
        Ok(Ok(got)) => assert!(
            got == payload,
            "{what}: the datagram arrived with a foreign body"
        ),
        Ok(Err(e)) => panic!("{what}: connection lost: {e:?}"),
        Err(_) => panic!(
            "{what}: nothing arrived in 5 s of virtual time — a packet sealed \
             under the new epoch's key is not opening"
        ),
    }
}

// ══════════════════════════════════════════════════════════════════════
// S23
// ══════════════════════════════════════════════════════════════════════

/// **S23, at `REKEY_EPOCH_MSGS`.**
///
/// One `Config::new()` pair — the production epoch size, reached the only
/// way an application reaches it: by sending 65 536 messages. A single
/// unidirectional stream carries the traffic across the boundary and its
/// bytes are verified as they arrive, so *"no application-visible event"* is
/// asserted the way an application would notice one: the stream would stall,
/// lose a byte, or die.
///
/// The four obligations, each with the build it catches:
///
/// * **The boundary is at 65 536.** A's counters are observed to run from
///   below `REKEY_EPOCH_MSGS` to above it, and the transfer keeps working
///   across it. BROKEN BUILD: a constant of 1 024 or 2¹⁵ — invisible to
///   every test in `spec_rekey.rs`, which configures its own epoch.
/// * **No handshake, no DH.** Not one handshake packet on the fabric after
///   establishment, and neither peer's cumulative DH count moves. BROKEN
///   BUILD: WireGuard's periodic re-handshake, which §5.4 and §7.7 override
///   — *"post-compromise healing within a connection does not exist"*.
/// * **No application-visible event.** 65 537 packets of stream data arrive
///   byte-exact and the stream ends cleanly. BROKEN BUILD: one that drops
///   the packets sealed either side of the boundary, or stalls waiting for
///   a key that no round trip is coming to deliver.
/// * **The counter is never reset.** Every counter exceeds its predecessor
///   across the boundary. BROKEN BUILD: one that restarts the counter with
///   the epoch — §7.2's replay window would then swallow the whole of epoch
///   1, which is the failure this assertion names before it happens.
///
/// NOT SEPARATED: the never-rekey build passes this test, by design. See
/// the module docs, and `spec_rekey.rs`'s `rk4`.
#[tokio::test(start_paused = true)]
#[cfg_attr(
    debug_assertions,
    ignore = "13.4 s in debug against 1.0 s in release: Appendix B's \"in the \
              release run if debug-slow\" (ruling 251). `cargo test --release \
              --all-features` runs it; in debug, `-- --ignored` does."
)]
async fn s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing() {
    local(async {
        let pair = Pair::seeded(0x5EED_0023);
        let (ca, cb) = pair.establish().await;
        settle().await;

        let tap: Tap = pair.net.tap();
        let a_addr = pair.a.addr();
        // Establishment's own handshake packets are not S23's subject: the
        // log is emptied here so every packet counted below arrived after
        // the connection was up.
        let _ = tap.drain();
        let (dhs_a, dhs_b) = (pair.a.dhs.get(), pair.b.dhs.get());

        let watch = Rc::new(std::cell::RefCell::new(Watch::default()));
        let highest = Rc::new(Cell::new(0u64));
        let target = REKEY_EPOCH_MSGS + OVERSHOOT;

        let mut send = ca.open_uni().await.expect("open_uni");
        let written = Rc::new(Cell::new(0usize));

        let writer = {
            let highest = Rc::clone(&highest);
            let written = Rc::clone(&written);
            async move {
                let mut offset = 0usize;
                while highest.get() < target {
                    let chunk = pattern(offset, ROUND);
                    let mut done = 0usize;
                    while done < chunk.len() {
                        done += send.write(&chunk[done..]).await.expect("write");
                    }
                    offset += chunk.len();
                }
                written.set(offset);
                send.finish().await.expect("finish");
            }
        };

        let reader = {
            let watch = Rc::clone(&watch);
            let highest = Rc::clone(&highest);
            let cb = &cb;
            async move {
                let mut recv = cb.accept_uni().await.expect("accept_uni");
                let mut offset = 0usize;
                let mut buf = vec![0u8; ROUND];
                loop {
                    let read = tokio::time::timeout(Duration::from_secs(120), recv.read(&mut buf))
                        .await
                        .unwrap_or_else(|_| {
                            panic!(
                                "the stream stalled after {offset} bytes with 120 s of \
                             virtual time spent — a receiver that stopped opening \
                             packets at the epoch boundary looks exactly like this"
                            )
                        });
                    // Fold the wire evidence in and free the log before the
                    // next round: 65 537 retained packets cost more than the
                    // transfer does.
                    {
                        let drained = tap.drain();
                        let mut w = watch.borrow_mut();
                        w.ingest(&drained, a_addr);
                        highest.set(w.highest());
                    }
                    match read {
                        Ok(Some(n)) => {
                            assert!(
                                buf[..n] == pattern(offset, n)[..],
                                "S23: the stream is not byte-exact across the \
                                 epoch boundary — the first bad byte is inside \
                                 the {n}-byte read at offset {offset}"
                            );
                            offset += n;
                        }
                        Ok(None) => break,
                        Err(e) => panic!("read failed at offset {offset}: {e:?}"),
                    }
                }
                offset
            }
        };

        let (_, got) = tokio::join!(writer, reader);

        // One last drain: the writer's tail and the FIN.
        {
            let mut w = watch.borrow_mut();
            w.ingest(&pair.net.tap().drain(), a_addr);
        }
        // Taken by value and the borrow released here: everything below is a
        // statement about the transfer that just ended, and the two round
        // trips at the end of the test are `await`s that must not be made
        // while a `RefCell` reference is alive.
        let w = std::mem::take(&mut *watch.borrow_mut());

        assert_eq!(
            got,
            written.get(),
            "S23: every byte written crossed the boundary and arrived — {} of \
             {} bytes did",
            got,
            written.get()
        );

        // ── The boundary is where §7.7 says it is, and it was crossed.
        let first = w.first.expect("A sealed Data packets");
        let last = w.last.expect("A sealed Data packets");
        assert!(
            first < REKEY_EPOCH_MSGS,
            "the transfer started at counter {first}, already past \
             `REKEY_EPOCH_MSGS` — nothing below observes a crossing"
        );
        assert!(
            last > REKEY_EPOCH_MSGS,
            "A's counter reached only {last}: the transfer never crossed \
             `REKEY_EPOCH_MSGS` = {REKEY_EPOCH_MSGS}, so this test proved \
             nothing about the production epoch size. If this is red and the \
             suite is otherwise green, the constant moved."
        );
        assert_eq!(
            last / REKEY_EPOCH_MSGS,
            1,
            "§7.7: epoch = `counter / REKEY_EPOCH_MSGS`. A's last counter is \
             {last}, which is epoch {} — the transfer was sized for exactly one \
             boundary and the arithmetic must agree",
            last / REKEY_EPOCH_MSGS
        );
        assert!(
            w.packets >= REKEY_EPOCH_MSGS,
            "{} packets is fewer than the {REKEY_EPOCH_MSGS} a boundary costs: \
             the counter advanced without packets being sealed, and §7.7's \
             *messages per epoch* is not what this fixture measured",
            w.packets
        );

        // ── The counter is never reset by the ratchet.
        assert_eq!(
            w.non_monotonic, 0,
            "§7.7: *the counter is never reset by the ratchet* — {} of A's \
             {} packets carried a counter no higher than its predecessor",
            w.non_monotonic, w.packets
        );

        // ── No handshake, no DH.
        assert_eq!(
            w.handshakes, 0,
            "S23: *no handshake* — {} handshake packets crossed the fabric \
             after establishment. A new handshake from a live static is \
             replacement (S3b), never rekey.",
            w.handshakes
        );
        assert_eq!(
            (pair.a.dhs.get(), pair.b.dhs.get()),
            (dhs_a, dhs_b),
            "S23: *there is no DH re-handshake* — the ratchet is symmetric-key \
             only, so a single charged DH across 65 537 messages is the \
             WireGuard design §7.7 overrides"
        );

        // ── And the connection is ordinary afterwards, in both directions.
        // A's next seal is in epoch 1; B's is still in epoch 0, and each
        // opens at the other end.
        round_trip(&ca, &cb, b"after the boundary, A to B", "A→B post-boundary").await;
        round_trip(&cb, &ca, b"after the boundary, B to A", "B→A post-boundary").await;
    })
    .await;
}

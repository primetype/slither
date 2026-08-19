//! **Ruling 270 — the reassembly hole-ceiling becomes credit-derived.**
//!
//! Written from the ratified behaviour and from round 42's committed
//! evidence (`.spec-v2-clean-slate/round42-C-reassembly-kill.md`) by an
//! author who never read the fix — CLAUDE.md working rule 6 — in a worktree
//! cut at `405796e`, verified as this file's first act.
//!
//! # The property, not the formula
//!
//! Ruling 270 states the ceiling as
//! `max(REASSEMBLY_CHUNKS_MAX = 1024, f(advertised stream window))` and does
//! **not** publish `f`. Nothing here computes `f`. Every test states one
//! half of the property instead:
//!
//! * **a sender of full-size frames that never exceeds its advertised
//!   credit cannot be killed by any loss pattern** — this file; and
//! * a tiny-fragment flood no such sender could produce still dies with
//!   `PROTOCOL_VIOLATION` — `src/core/connection/tests_reassembly_credit.rs`,
//!   which needs hand-built frames the public API cannot emit.
//!
//! Both halves are needed. A "fix" that deletes the ceiling passes this
//! file and fails the other one.
//!
//! # The loss shape, and why it is `drop_at` and not `lossy`
//!
//! round42-C's separation table is the derivation, and it is the reason a
//! naive loss test proves nothing:
//!
//! | forward-path loss shape | result at base |
//! |---|---|
//! | none | completes |
//! | uniform 1-in-50 | completes — `cwnd_max` 13 317 B, **eleven packets** |
//! | bounded queue, tail-drop at 500 | completes |
//! | **one transient mass-loss burst** | **ceiling hit at 1 025 chunks, dies** |
//!
//! A *steady* loss rate cannot reach the ceiling: NewReno drives the window
//! below the hazard before enough holes can coexist. The hazard needs zero
//! steady loss — so the window grows to its flow-control limit — and then
//! one transient burst, which is what a saturated receive socket buffer
//! looks like from the protocol's side. So the fabric here is **lossless
//! until the burst**, and the burst is `FlakyPolicy::drop_at` over an index
//! set, never an RNG draw.
//!
//! # No sleep, no kernel
//!
//! Two endpoints over `testutil`'s `FlakyWire` on tokio's paused clock; one
//! seed, carried over from the evidence's reproducer (`0x4200_0001`). Every
//! wire delay and every timer resolves in virtual time.
//!
//! # For the integrator (working rule 15)
//!
//! This target has **no `[[test]]` stanza** in `Cargo.toml` — adding one is
//! the integrator's job, not a blind author's. The inner
//! `#![cfg(feature = "test-util")]` below is what keeps a plain `cargo test`
//! green in the meantime: without it, cargo's auto-discovery compiles this
//! file with `testutil` gated out and the whole test gate fails to build.
//! The stanza to add is
//! `[[test]] name = "story_reassembly" required-features = ["test-util"]`,
//! after which the `cfg` may be dropped.

#![cfg(feature = "test-util")]

use std::net::SocketAddr;
use std::time::Duration;

use slither::config::Config;
use slither::constants::{INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA};
use slither::error::{ConnectionLost, ReadError, WriteError};
use slither::testutil::{FlakyPolicy, Pair, Tap, TestConnection, addr_a, local};
// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// The evidence's seed. Carried verbatim so that a failure here and the
/// throwaway reproducer in `round42-C` are the same experiment.
const SEED: u64 = 0x4200_0001;

/// One-way fabric delay, both directions: the evidence's 5 ms, i.e. a
/// 10 ms RTT. Large enough that a flight's worth of holes coexists before
/// any retransmission can arrive, which is the whole hazard.
const ONE_WAY: Duration = Duration::from_millis(5);

/// The A-wire send index at which the burst begins.
///
/// The evidence armed on `cwnd > 2 600 000` — one flight able to carry more
/// than 2 x 1 024 packets, so more than 1 024 holes can coexist before any
/// retransmission arrives — and recorded that the trigger fired at **send
/// index 5502**. `cwnd` is not on the public API, so the index is used
/// directly: on a lossless path the two are the same instant, because slow
/// start's trajectory is a function of the bytes already sent.
///
/// The burst is installed **before the handshake**, not armed by observing
/// the run. Two attempts at observation are worth recording, because both
/// silently produced a test that proved nothing:
///
/// * **armed from the writer's loop** — fires at exactly 5 500 while the
///   writer is flow-control-parked, and **never fires at all** once the
///   window exceeds the payload (16 MiB / 12 MiB), because such a writer
///   buffers everything and parks nowhere;
/// * **armed from the reader's loop** — always fires, but the reader is
///   woken once per *driver turn*, and a driver turn at this cwnd carries a
///   whole flight. The measured first observation past 5 500 was **9 212**,
///   by which point the transfer had too little left to run and the same
///   burst was survivable at base.
///
/// A `drop_at` index set is a pure function of the wire's own send counter,
/// so installing it up front needs no observation and no RNG, and the
/// fabric is lossless up to exactly this index.
const BURST_FROM: usize = 5_500;

/// How many consecutive A-wire sends the burst covers. Every **even**
/// index inside it dies: the alternating pattern a tail-dropping socket
/// buffer produces as it fills and drains, and the pattern that maximises
/// the discontiguous-range count.
const BURST: usize = 6_000;

/// Bytes the writer pushes. Enough that the sender must traverse the whole
/// burst window (≈ 11 500 sends, of which 3 000 are discarded) and still
/// have data left, so the transfer's completion is a real event.
const TOTAL: usize = 12 * 1024 * 1024;

/// Write size. The evidence's 64 KiB chunks.
const CHUNK: usize = 64 * 1024;

/// Virtual-time budget. Far past anything here, and only a runaway hits it.
const PATIENCE: Duration = Duration::from_secs(600);

/// Byte `i` of the stream, so the reader can check content without holding
/// a 12 MiB copy.
fn byte_at(i: usize) -> u8 {
    (i % 251) as u8
}

/// What one run of the burst scenario observed.
#[derive(Debug)]
struct Outcome {
    /// Bytes the reader took off the stream before it ended.
    read: usize,
    /// How the reader's stream ended: `Ok(())` for a clean FIN.
    reader: Result<(), ReadError>,
    /// How the writer ended.
    writer: Result<(), WriteError>,
    /// A-wire sends at the end of the run.
    a_sends: usize,
}

impl Outcome {
    /// The burst window was fully crossed — 3 000 datagrams really were
    /// discarded. Without this a build that stalls before
    /// `BURST_FROM + BURST` passes every survival assertion by never being
    /// tested.
    fn traversed(&self) -> bool {
        self.a_sends >= BURST_FROM + BURST
    }

    /// The §10.6 kill, exactly: the receiver tears down with
    /// `PROTOCOL_VIOLATION` and the sender learns of it as `PeerClosed`.
    fn killed_by_the_ceiling(&self) -> bool {
        matches!(
            self.reader,
            Err(ReadError::ConnectionLost(
                ConnectionLost::ProtocolViolation { code: 1 }
            ))
        )
    }

    /// A one-line description for a failure message.
    fn summary(&self) -> String {
        format!(
            "read {} of {TOTAL} bytes, {} A-sends; reader={:?} writer={:?}",
            self.read, self.a_sends, self.reader, self.writer
        )
    }
}

/// Counts A's sends off the shared tap, so a run can prove the burst
/// window was really crossed.
///
/// `FlakyWire`'s own send index counts every `send_to` **before** any
/// policy decision, and the tap records every send that was neither
/// refused nor blackholed — neither happens here — so the two agree.
/// Dropped datagrams are tapped as well (the tap is written before the
/// loss decision), which is what keeps the count exact *inside* the burst.
struct ASends {
    tap: Tap,
    a: SocketAddr,
    n: usize,
}

impl ASends {
    fn new(tap: Tap) -> Self {
        Self {
            tap,
            a: addr_a(),
            n: 0,
        }
    }

    fn count(&mut self) -> usize {
        for spied in self.tap.drain() {
            if spied.src == self.a {
                self.n += 1;
            }
        }
        self.n
    }
}

/// Drive one saturating bulk transfer over a lossless 10 ms path with one
/// transient mass-loss burst, at the given advertised windows.
///
/// Nothing adversarial is expressible here: the writer is
/// `SendStream::write`, so every frame it emits is as large as the path
/// allows, and it can never exceed the credit the peer advertised because
/// the API parks it instead.
async fn burst_run(stream_window: u64, connection_window: u64) -> Outcome {
    let config = Config::new()
        .with_flow_windows(stream_window, connection_window)
        .expect("windows inside the varint bound");
    let pair = Pair::seeded_with(SEED, config);

    // Lossless 10 ms path in both directions, plus — on the forward path
    // only — one transient mass-loss burst: every **even** A-wire send index
    // in `[BURST_FROM, BURST_FROM + BURST)`. Nothing before it is lost and
    // nothing after it is, which is the shape round42-C measured and the
    // only shape that reaches the ceiling.
    let mut forward = FlakyPolicy::drop_at((BURST_FROM..BURST_FROM + BURST).step_by(2));
    forward.base_delay = ONE_WAY;
    pair.a.wire.set_policy(forward);
    pair.b
        .wire
        .set_policy(FlakyPolicy::perfect().with_delay(ONE_WAY, Duration::ZERO));

    let (ca, cb): (TestConnection, TestConnection) = pair.establish().await;

    let tap = pair.net.tap();

    // Both connection handles stay owned by *this* frame. An `async move`
    // block that swallowed `ca` would drop it the moment the writer
    // finished — §16.3 closes the connection on the last handle's drop, so
    // the reader would see `PeerClosed { code: 0 }` mid-transfer and every
    // survival assertion would fail for a reason that has nothing to do
    // with §10.6. (It did, on the first run of this file.)
    let mut s = ca.open_uni().await.expect("open_uni");

    // Read **after** the join: a writer stops long before the wire does.
    let mut sends = ASends::new(tap);

    let writer = async {
        let mut sent = 0usize;
        while sent < TOTAL {
            let want = CHUNK.min(TOTAL - sent);
            // The payload is offset-dependent, so each chunk is written
            // from the right phase of the ramp.
            let buf: Vec<u8> = (0..want).map(|i| byte_at(sent + i)).collect();
            match s.write(&buf).await {
                Ok(n) => {
                    assert!(n >= 1, "a blocked write is Pending, never Ok(0)");
                    sent += n;
                }
                Err(e) => return (sent, Err(e)),
            }
        }
        (sent, s.finish().await)
    };

    let reader = async {
        let mut r = cb.accept_uni().await.expect("accept_uni");
        let mut buf = vec![0u8; CHUNK];
        let mut read = 0usize;
        loop {
            match r.read(&mut buf).await {
                Ok(Some(n)) => {
                    for (k, got) in buf[..n].iter().enumerate() {
                        assert_eq!(
                            *got,
                            byte_at(read + k),
                            "§9.5: byte {} came back wrong",
                            read + k
                        );
                    }
                    read += n;
                }
                Ok(None) => return (read, Ok(())),
                Err(e) => return (read, Err(e)),
            }
        }
    };

    let ((_sent, writer), (read, reader)) = tokio::join!(writer, reader);
    let a_sends = sends.count();
    Outcome {
        read,
        reader,
        writer,
        a_sends,
    }
}

/// [`burst_run`] under a virtual-time deadline, so a stall fails loudly
/// rather than hanging the suite.
async fn burst(stream_window: u64, connection_window: u64) -> Outcome {
    match tokio::time::timeout(PATIENCE, burst_run(stream_window, connection_window)).await {
        Ok(o) => o,
        Err(_) => panic!("the transfer was still running after {PATIENCE:?} of virtual time"),
    }
}

/// Assert the ratified property at one window pair.
fn assert_survives(o: &Outcome, windows: &str) {
    assert!(
        !o.killed_by_the_ceiling(),
        "RULING 270 at {windows}: a conforming full-frame sender inside its \
         advertised credit was killed with CLOSE(PROTOCOL_VIOLATION) by a \
         loss pattern. {}",
        o.summary()
    );
    assert!(
        o.reader.is_ok() && o.writer.is_ok(),
        "at {windows}: the connection did not survive the burst. {}",
        o.summary()
    );
    assert_eq!(
        o.read,
        TOTAL,
        "at {windows}: the transfer did not complete. {}",
        o.summary()
    );
    assert!(
        o.traversed(),
        "at {windows}: the burst window was never fully crossed, so the \
         3 000 discards did not all happen and nothing was tested — {} \
         sends, needed {}",
        o.a_sends,
        BURST_FROM + BURST
    );
}

// ══════════════════════════════════════════════════════════════════════
// 1. The survival story
// ══════════════════════════════════════════════════════════════════════

/// **A conforming sender cannot be killed by a loss pattern** — at the
/// window pair where, at base, it is.
///
/// This is round42-C's kill reconstructed in virtual time: 8 MiB / 16 MiB
/// on both ends, a saturating writer of full-size frames, a lossless 10 ms
/// path so the window reaches its flow-control limit, and then one
/// transient burst that discards every second datagram for 6 000 sends.
/// Nothing in it is adversarial — the sender is `SendStream::write` and
/// parks when its credit runs out — and the only unusual thing on the path
/// is that some datagrams do not arrive.
///
/// # What separates the builds
///
/// * **At base** the receive half stores more than 1 024 discontiguous
///   ranges — the evidence measured 1 025 — and answers by tearing the
///   connection down with `PROTOCOL_VIOLATION`, blaming the peer for the
///   network's behaviour. Red, and [`Outcome::killed_by_the_ceiling`]
///   makes the red *exact*: any other failure reports a different message.
/// * **A build whose ceiling scales with the advertised credit** carries
///   the same holes and completes.
/// * **A build that simply raises the constant** passes here and fails
///   [`the_property_holds_at_a_second_raised_window`] at 16 MiB, and a
///   build that deletes the ceiling passes both and fails the abuse half
///   in `tests_reassembly_credit.rs`.
///
/// The burst is an index set on the forward wire, and
/// [`Outcome::traversed`] asserts the whole 6 000-send window was crossed
/// — 3 000 datagrams really discarded. Working rule 9: a survival
/// assertion over a burst that never happened is satisfied by every build,
/// and two earlier arming schemes here produced exactly that (see
/// [`BURST_FROM`]).
#[tokio::test(start_paused = true)]
async fn a_conforming_sender_survives_a_transient_mass_loss_burst_at_eight_mib() {
    local(async {
        let o = burst(8 * 1024 * 1024, 16 * 1024 * 1024).await;
        assert_survives(&o, "8 MiB / 16 MiB");
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 2. The property at more than one point
// ══════════════════════════════════════════════════════════════════════

/// **The same property below the base build's crossing** — 2 MiB / 8 MiB,
/// which round 42 measured as the 100 ms optimum and ruling 269(ii)'s
/// sizing advice steers operators toward.
///
/// Green at base, by the measured curve: 4 MiB reaches 946 stored ranges
/// against a ceiling of 1 024, and 2 MiB is half of that again. It must
/// **stay** green — this is the point ruling 269's own advice lands on, so
/// a fix that fixed 8 MiB by moving the hazard down here would be worse
/// than no fix.
///
/// # What separates the builds
///
/// A ceiling derived from the credit **without** ruling 270's floor —
/// `f(2 MiB)` alone with no `max(1024, ·)` — is not necessarily red here,
/// which is why the floor gets its own test at the default windows in
/// `tests_reassembly_credit.rs`. What this one catches is a fix that
/// *lowers* the effective tolerance anywhere in the raised-window range.
#[tokio::test(start_paused = true)]
async fn the_property_holds_below_the_base_builds_crossing() {
    local(async {
        let o = burst(2 * 1024 * 1024, 8 * 1024 * 1024).await;
        assert_survives(&o, "2 MiB / 8 MiB");
    })
    .await;
}

/// **The property at a window twice past the failing one** — 16 MiB /
/// 32 MiB.
///
/// Red at base for the same reason as the 8 MiB story, and it is the test
/// that separates *credit-derived* from *a bigger constant*: a build that
/// answers ruling 270 by raising `REASSEMBLY_CHUNKS_MAX` to some new fixed
/// number passes the 8 MiB story and dies here as soon as the window
/// exceeds that number's reach. Only a ceiling that moves **with** the
/// advertised credit passes both.
#[tokio::test(start_paused = true)]
async fn the_property_holds_at_a_second_raised_window() {
    local(async {
        let o = burst(16 * 1024 * 1024, 32 * 1024 * 1024).await;
        assert_survives(&o, "16 MiB / 32 MiB");
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 4. The default is untouched
// ══════════════════════════════════════════════════════════════════════

/// **The ratified default regime does not move.** The identical burst that
/// kills at 8 MiB completes at `INITIAL_MAX_STREAM_DATA` /
/// `INITIAL_MAX_DATA` today, and must still complete after.
///
/// Green at base, and the reason it is green is the 12× margin the
/// evidence measured: a 256 KiB window admits ~218 packets in flight, so
/// at most ~83 discontiguous ranges can coexist against a ceiling of
/// 1 024. This is the regime every other test in the suite runs in, and a
/// fix that changed behaviour here would be changing behaviour for
/// everyone who configured nothing.
///
/// # What separates the builds
///
/// A ceiling computed as `f(window)` **without** ruling 270's
/// `max(1024, ·)` floor lands under 1 024 at the default window; if `f` is
/// aggressive enough it lands under the ~83 ranges this burst produces,
/// and this test is red. It is the cheapest guard against a regression
/// that would only ever be seen by default-window users.
#[tokio::test(start_paused = true)]
async fn the_ratified_default_completes_the_same_burst() {
    local(async {
        let o = burst(INITIAL_MAX_STREAM_DATA, INITIAL_MAX_DATA).await;
        assert_survives(&o, "the ratified defaults");
    })
    .await;
}

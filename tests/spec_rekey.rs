//! **The epoch ratchet — §7.7, Appendix B (ruling 251).**
//!
//! > Transport keys ratchet forward on a counter-derived schedule. A
//! > message sealed at `counter` belongs to epoch
//! > `counter / REKEY_EPOCH_MSGS`; each direction ratchets independently;
//! > the counter is **never reset** by the ratchet. […] The receiver
//! > retains the current and immediately preceding epoch keys (straggler
//! > tolerance: one epoch back); anything older is refused, its key
//! > ratcheted away.
//!
//! # Why this file exists (ruling 251)
//!
//! The ratchet worked and **nothing observed it**: hard-wiring the epoch
//! schedule to never-rekey left all 1 041 tests green. `with_epoch_size`
//! (ruling 82's knob, `config.rs:183`) was exercised by one setter unit
//! test, and no test configured a small epoch and crossed a boundary
//! *through an endpoint*. This file is the config-knob half of ruling 82's
//! *"a configurable epoch pins the boundary behaviour and a separate
//! constant test pins the value — independently"*; `story_rekey.rs` is the
//! same behaviour at the production constant, and the value is pinned
//! [here](rk1_the_rekey_of_zeros_vector) by computation rather than by
//! round trip.
//!
//! # Working rule 9 — which build each test separates
//!
//! Every test below carries a `BROKEN BUILD:` block. Two mutations matter
//! and they are **not** separated by the same assertions:
//!
//! * **never-rekey** — the schedule hard-wired so no epoch is ever left
//!   (`epoch_size` forced to `u64::MAX`, or the config knob ignored).
//!   Only [`rk4`](rk4_a_packet_two_epochs_back_does_not_open) fails it:
//!   *the boundary crossing being invisible is satisfied for free by the
//!   build in which nothing ever happens*, so [`rk2`](rk2_crossing_several_epoch_boundaries_is_invisible)
//!   is a conformance statement and says so rather than pretending to be a
//!   pin.
//! * **current-epoch-only** — a receiver that keeps one key instead of
//!   two. [`rk5`](rk5_a_packet_one_epoch_back_still_opens) fails it, and
//!   [`rk4`](rk4_a_packet_two_epochs_back_does_not_open) passes it, which
//!   is exactly why both exist.
//!
//! # The instrument: hold a packet, then post it late
//!
//! §7.7's refusal is *"a generic decryption failure at the hiss surface"* —
//! silent, with no event and no error. An integration test therefore cannot
//! observe the refusal directly; it can only observe **what a packet
//! carried, or failed to carry, to the application**. The carrier is an
//! unreliable datagram (§11.1: *"no delivery promise … no
//! retransmission"*), because it is the one payload the stack will not
//! re-send behind the test's back — a stream frame held back and injected
//! late would have been retransmitted at its own counter long before, and
//! "the application never saw it" would be true of a conforming build and a
//! broken one alike.
//!
//! The hold is `FlakyPolicy::drop_at`, which is **index-based and takes no
//! RNG draw**, and the tap sits *above* the loss decision (`testutil`
//! `send_to` steps 4→5) — so the held packet's exact sealed bytes are
//! recoverable even though the fabric destroyed it. `Network::inject` then
//! posts those bytes at a later epoch: no policy applies, delivery is
//! immediate, and nothing is counted or tapped.
//!
//! **The held packet has never been delivered**, which is the whole reason
//! the hold exists: replaying a packet the peer already opened would be
//! refused by §7.2's replay window in *every* build, and the assertion
//! would separate nothing (working rule 9). `REPLAY_WINDOW` is 2 048 bits
//! and every counter distance used here is under 60, so the window admits
//! all of them and the epoch is the only thing that can refuse.
//!
//! # Paused clock, and one thing deliberately avoided
//!
//! Every test is `#[tokio::test(start_paused = true)]`, and there is no
//! sleep. The observation instrument is [`claim_ready`] — poll each
//! `recv_datagram` exactly once and stop at the first `Pending` — which
//! advances **no virtual time at all**, so an absence proved here is an
//! absence at one instant and not a race with a timer.
//!
//! [`claim_ready`]: fn.claim_ready.html

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::pin::pin;
use std::task::Poll;

use cryptoxide::chacha20poly1305::ChaCha20Poly1305;
use slither::Config;
use slither::constants::{
    AEAD_TAG_LEN, DATA_HEADER_LEN, MAX_DATAGRAM, MAX_DATAGRAM_PAYLOAD, MAX_EPOCH_JUMP, PKT_DATA,
    PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP,
};
use slither::testutil::{FlakyPolicy, Pair, Tap, TestConnection, TestRecvStream, local, settle};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// The epoch size these tests configure (ruling 82's knob).
///
/// Not 1, and not 2. §7.2's ACK machinery and §14's congestion control both
/// react to packet counts, and an epoch of 1 or 2 makes every packet a
/// boundary — ruling 251 measured 14 unrelated tests breaking under
/// `epoch_size = 1`. 16 is large enough that a settle-step's two or three
/// packets cannot skip an epoch (which is what makes
/// [`pump_until_epoch`]'s exactness assertions sound) and small enough that
/// crossing two boundaries costs about forty small datagrams.
const EPOCH: u64 = 16;

/// The config both endpoints share. §7.7: *"both ends must pass the
/// identical epoch size"* (Appendix B) — `Pair::seeded_with` hands one
/// `Config` to both, which is the only arrangement that satisfies it.
fn small_epoch_config() -> Config {
    Config::new().with_epoch_size(NonZeroU64::new(EPOCH).expect("EPOCH is nonzero"))
}

/// A packet as the tap saw it: its §3.4 counter, and its sealed length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sealed {
    counter: u64,
    len: usize,
}

impl Sealed {
    /// §7.7: *"a message sealed at `counter` belongs to epoch
    /// `counter / epoch_size`"*.
    fn epoch(self) -> u64 {
        self.counter / EPOCH
    }
}

/// Every Data packet `who` has handed to the fabric, in send order.
///
/// The counter is read out of the header rather than out of a core: §3.4
/// fixes the Data header as
/// `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`, little-endian,
/// in clear (`DATA_HEADER_LEN` = 14) — the header is the AEAD's associated
/// data, so these eight bytes are exactly the counter the packet was sealed
/// at. Handshake packets carry no counter and are filtered out by their
/// type byte.
fn sealed_by(tap: &Tap, who: SocketAddr) -> Vec<Sealed> {
    tap.snapshot()
        .iter()
        .filter(|s| s.src == who && s.bytes.first() == Some(&PKT_DATA))
        .map(|s| {
            assert!(
                s.bytes.len() >= DATA_HEADER_LEN + AEAD_TAG_LEN,
                "a Data packet shorter than its own header plus tag: {} bytes",
                s.bytes.len()
            );
            Sealed {
                counter: u64::from_le_bytes(s.bytes[6..14].try_into().expect("eight header bytes")),
                len: s.bytes.len(),
            }
        })
        .collect()
}

/// The sealed bytes of `who`'s Data packets, in send order.
fn sealed_bytes(tap: &Tap, who: SocketAddr) -> Vec<Vec<u8>> {
    tap.snapshot()
        .iter()
        .filter(|s| s.src == who && s.bytes.first() == Some(&PKT_DATA))
        .map(|s| s.bytes.clone())
        .collect()
}

/// This wire's 0-based send index — the number `drop_at` counts in.
///
/// The tap sits below the send-failure and blackhole checks and above the
/// loss draw, so with neither active on `who` this is exactly that index.
/// Carried from `story_datagram.rs:259`.
fn sent_from(tap: &Tap, who: SocketAddr) -> usize {
    tap.snapshot().iter().filter(|s| s.src == who).count()
}

/// A datagram that names itself: a big-endian tag, then a body that is a
/// function of the tag. Adapted from `story_datagram.rs`.
fn tagged(tag: u16, len: usize) -> Vec<u8> {
    assert!(len >= 2, "a self-naming datagram needs its two tag bytes");
    let mut v = vec![0u8; len];
    v[..2].copy_from_slice(&tag.to_be_bytes());
    for (i, b) in v[2..].iter_mut().enumerate() {
        *b = ((i + usize::from(tag)) % 251) as u8;
    }
    v
}

/// Read a payload's tag back **and check its body against the tag**, so a
/// test cannot mistake one datagram for another.
fn tag_of(d: &[u8]) -> u16 {
    assert!(d.len() >= 2, "a datagram shorter than its own tag: {d:?}");
    let tag = u16::from_be_bytes([d[0], d[1]]);
    let want = tagged(tag, d.len());
    assert!(
        d == want.as_slice(),
        "a datagram tagged {tag} has a foreign body — the payload was corrupted, \
         truncated or misattributed"
    );
    tag
}

/// Poll `fut` exactly once.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// Claim every datagram already queued on `c`, **without advancing the
/// clock and without parking**.
///
/// This is the whole observation instrument of this file. It advances no
/// virtual time, so "nothing arrived" is a statement about one instant
/// after the driver has had its turns — never a race with a retransmission
/// timer, and never a silence that a slower build would have broken.
async fn claim_ready(c: &TestConnection, what: &str) -> Vec<u16> {
    let mut out = Vec::new();
    loop {
        let mut fut = pin!(c.recv_datagram());
        match poll_once(fut.as_mut()).await {
            Poll::Ready(Ok(d)) => out.push(tag_of(&d)),
            Poll::Ready(Err(e)) => panic!("{what}: connection lost mid-claim: {e:?}"),
            Poll::Pending => break,
        }
    }
    out
}

/// Send one small datagram and give both drivers their turns.
///
/// Small on purpose. §14.5's admission gate holds the sender to
/// `INITIAL_WINDOW` (12 000 bytes) of un-acknowledged data, and these tests
/// deliberately never advance the clock, so no ACK is ever delayed into
/// existence and nothing in flight is ever released. Forty 30-byte packets
/// fit inside that window with room to spare; forty 1 200-byte ones would
/// stall the pump half way and the test would hang rather than fail.
async fn nudge(c: &TestConnection, tag: u16) {
    c.send_datagram(&tagged(tag, 8)).expect("send_datagram");
    settle().await;
}

/// Drive `who`'s counter forward until its last sealed packet is in
/// `target` — asserting on arrival that it landed in `target` **exactly**,
/// not past it.
///
/// The exactness is what makes the straggler distances in [`rk4`] and
/// [`rk5`] facts rather than hopes: a pump that overshot by an epoch would
/// turn [`rk5`]'s *"one epoch back"* into *"two epochs back"* and the test
/// would then be asserting the opposite of what it says.
///
/// [`rk4`]: rk4_a_packet_two_epochs_back_does_not_open
/// [`rk5`]: rk5_a_packet_one_epoch_back_still_opens
async fn pump_until_epoch(
    pair: &Pair,
    ca: &TestConnection,
    cb: &TestConnection,
    tap: &Tap,
    target: u64,
    first_tag: u16,
) -> u16 {
    let mut tag = first_tag;
    let cap = (target + 2) * EPOCH + 16;
    while sealed_by(tap, pair.a.addr())
        .last()
        .expect("A has sealed at least one Data packet")
        .epoch()
        < target
    {
        nudge(ca, tag).await;
        // Drain B so nothing is lost to §11.3's bounded receive side; the
        // tags are not the subject here, only the counters are.
        let _ = claim_ready(cb, "pump drain").await;
        tag += 1;
        assert!(
            u64::from(tag - first_tag) < cap,
            "the pump sent {} datagrams without reaching epoch {target}: A's \
             counter is not advancing one per packet, which every distance in \
             this file assumes",
            tag - first_tag
        );
    }
    let last = *sealed_by(tap, pair.a.addr())
        .last()
        .expect("A has sealed at least one Data packet");
    assert_eq!(
        last.epoch(),
        target,
        "the pump overshot: it stopped at counter {} (epoch {}) rather than \
         inside epoch {target}, so every epoch distance asserted after it would \
         be wrong",
        last.counter,
        last.epoch()
    );
    tag
}

/// Hold one full-size datagram: seal it, let the fabric destroy it, and
/// return its sealed bytes together with the packet's counter.
///
/// The returned packet is one **the peer has never seen**, which is what
/// makes a later [`Network::inject`] of it a test of the epoch and not of
/// §7.2's replay window.
///
/// [`Network::inject`]: slither::testutil::Network::inject
async fn hold_one(pair: &Pair, ca: &TestConnection, tap: &Tap, tag: u16) -> (Vec<u8>, Sealed) {
    /// Wide enough that a coalesced ACK or a stray packet sharing the
    /// window cannot let the datagram escape, narrow enough that the
    /// escape assertion below is meaningful.
    const WINDOW: usize = 4;

    let base = sent_from(tap, pair.a.addr());
    let before = sealed_by(tap, pair.a.addr()).len();
    pair.a
        .wire
        .set_policy(FlakyPolicy::drop_at(base..base + WINDOW));
    ca.send_datagram(&tagged(tag, MAX_DATAGRAM_PAYLOAD))
        .expect("send_datagram");
    settle().await;
    let after = sent_from(tap, pair.a.addr());
    assert!(
        after > base,
        "the drop window was never reached: A made no send at index {base}, so \
         nothing was held and everything below would prove nothing"
    );
    assert!(
        after <= base + WINDOW,
        "A made {} sends while a {WINDOW}-index window was armed, so one escaped \
         it and the held packet may have been delivered after all",
        after - base
    );
    pair.a.wire.set_policy(FlakyPolicy::perfect());

    let packets = sealed_by(tap, pair.a.addr());
    let bytes = sealed_bytes(tap, pair.a.addr());
    let held: Vec<usize> = (before..packets.len())
        .filter(|&i| packets[i].len == MAX_DATAGRAM)
        .collect();
    assert_eq!(
        held.len(),
        1,
        "expected exactly one {MAX_DATAGRAM}-byte packet inside the drop window, \
         found {} among {:?} — a `MAX_DATAGRAM_PAYLOAD` datagram occupies exactly \
         `DATA_HEADER_LEN + 1 + {MAX_DATAGRAM_PAYLOAD} + AEAD_TAG_LEN` = \
         {MAX_DATAGRAM} bytes and nothing else in these tests is that size",
        held.len(),
        &packets[before..]
    );
    let i = held[0];
    (bytes[i].clone(), packets[i])
}

/// The handshake packets on the wire, both types, from either peer.
fn handshakes(tap: &Tap) -> usize {
    tap.snapshot()
        .iter()
        .filter(|s| {
            matches!(
                s.bytes.first(),
                Some(&PKT_HANDSHAKE_INIT) | Some(&PKT_HANDSHAKE_RESP)
            )
        })
        .count()
}

/// Read exactly `n` bytes, failing loudly rather than hanging the suite.
async fn read_exactly(r: &mut TestRecvStream, n: usize, what: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let mut buf = vec![0u8; 16 * 1024];
    while out.len() < n {
        match tokio::time::timeout(std::time::Duration::from_secs(30), r.read(&mut buf)).await {
            Ok(Ok(Some(k))) => out.extend_from_slice(&buf[..k]),
            Ok(Ok(None)) => panic!(
                "{what}: end of stream after {} of {n} bytes — the ratchet lost data",
                out.len()
            ),
            Ok(Err(e)) => panic!("{what}: read failed after {} bytes: {e:?}", out.len()),
            Err(_) => panic!(
                "{what}: stalled after {} of {n} bytes with 30 s of virtual time \
                 spent — a receiver that stopped opening packets at a boundary \
                 looks exactly like this",
                out.len()
            ),
        }
    }
    out
}

// ══════════════════════════════════════════════════════════════════════
// 1. The `Rekey()` vector — §7.7's constant, pinned by computation
// ══════════════════════════════════════════════════════════════════════

/// §7.7: *"Epoch `e`'s key is Noise §11.3 `Rekey()` applied `e` times:
/// `Rekey(k) = ENCRYPT(k, 2⁶⁴ − 1, empty, zeros[32])[0..32]`. The
/// ChaCha20-Poly1305 vector […]:
/// `REKEY(0³²) = 25ce…4c58`"*.
///
/// **Ruling 251 decided this file is that vector's home**, and why it
/// cannot be a round trip: *"a both-sides-hiss boundary test cannot pin it,
/// because a wrong `Rekey()` agrees with itself"*. Every other test here
/// drives both ends of one implementation; if hiss ratcheted with, say, the
/// nonce `0` instead of `2⁶⁴ − 1`, or kept the tag instead of the
/// ciphertext, both ends would do it identically and every behavioural test
/// in this file and in `story_rekey.rs` would still be green. This test is
/// the only thing standing between that build and a release.
///
/// The construction is spelled out from the Noise spec rather than borrowed
/// from hiss, which is the point — an independent computation:
///
/// * the AEAD is ChaCha20-Poly1305 — §2.2's reference suite is
///   `P256 / ChaChaPoly / Blake2b`;
/// * §3.4: *"Noise builds the ChaChaPoly nonce as
///   `32 zero bits ‖ LE64(counter)`"*, so `n = 2⁶⁴ − 1` is four zero bytes
///   then eight `0xFF`;
/// * the associated data is empty and the plaintext is `zeros[32]`;
/// * `Rekey` keeps `[0..32]` — the ciphertext, **not** the 16-byte tag.
///
/// BROKEN BUILD: any `Rekey()` that differs — wrong nonce, wrong AEAD, tag
/// instead of ciphertext, key material carried in instead of zeros. None of
/// them is visible to a test that opens what it sealed.
///
/// The `cryptoxide` call is a **test-only** computation over fixed
/// constants — the golden-wire philosophy, not session cryptography
/// (ruling 251). Nothing in `src/` gains a primitive: every session
/// Noise/curve/AEAD operation still flows through hiss.
#[test]
fn rk1_the_rekey_of_zeros_vector() {
    /// `Rekey(k)` — Noise §11.3, as §7.7 writes it.
    fn rekey(key: &[u8; 32]) -> [u8; 32] {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut aead = ChaCha20Poly1305::new(key, &nonce, &[]);
        let mut out = [0u8; 32];
        let mut tag = [0u8; AEAD_TAG_LEN];
        aead.encrypt(&[0u8; 32], &mut out, &mut tag);
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    const REKEY_OF_ZEROS: &str = "25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58";

    let once = rekey(&[0u8; 32]);
    assert_eq!(
        hex(&once),
        REKEY_OF_ZEROS,
        "§7.7's `REKEY(0³²)` vector. If this is the only red in the file, the \
         spec's constant and this construction disagree — that is a ruling, not \
         an expectation to update (CLAUDE.md's wire-pin rule)."
    );

    // The transform is not the identity and not a fixed point: epoch 2's
    // key differs from epoch 1's. A `Rekey` that returned its input would
    // satisfy every behavioural test in this file — both ends would agree
    // on a key that never changed — and it would satisfy a vector pin
    // written only against `0³²` if that vector had been computed from the
    // same broken code. It cannot satisfy both this line and the one above.
    let twice = rekey(&once);
    assert_ne!(
        hex(&twice),
        hex(&once),
        "§7.7: epoch `e`'s key is `Rekey()` applied `e` times, so applying it \
         again must move"
    );
    assert_ne!(
        hex(&twice),
        hex(&[0u8; 32]),
        "and must not return to the key it started from"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 2. Crossing boundaries is invisible (ruling 82's knob)
// ══════════════════════════════════════════════════════════════════════

/// **S23 at the configurable epoch.** §7.7 / Appendix B: *"seals cross the
/// epoch boundary invisibly — a stream is byte-exact across it, with no
/// handshake, no round trip and no application-visible event; each
/// direction ratchets independently; the counter is never reset."*
///
/// A 96 KiB stream over an epoch of [`EPOCH`] messages crosses several
/// boundaries — five on the base commit, and *at least three* is asserted
/// from the wire rather than assumed — and the bytes come out identical.
///
/// **This test separates nothing, and says so.** A never-rekey build passes
/// it in full: *boundary-invisibility alone is satisfied for free by the
/// build in which nothing ever happens* (ruling 251). It is here because
/// S23's acceptance is a **negative** obligation — the ratchet must produce
/// no handshake, no extra round trip, no event, no lost byte — and a
/// negative obligation is still worth executing even when its degenerate
/// build satisfies it. [`rk4`](rk4_a_packet_two_epochs_back_does_not_open)
/// is the test that fails the degenerate build.
///
/// BROKEN BUILD (what it *does* catch): one that re-handshakes at the
/// boundary (`handshakes` grows, or a DH is charged), one that resets the
/// counter with the epoch (the counters stop being strictly increasing —
/// and §7.2's replay window would then eat the whole second epoch), and one
/// that drops or reorders the bytes sealed either side of a boundary.
#[tokio::test(start_paused = true)]
async fn rk2_crossing_several_epoch_boundaries_is_invisible() {
    local(async {
        let pair = Pair::seeded_with(0x5EED_0002, small_epoch_config());
        let (ca, cb) = pair.establish().await;
        settle().await;

        let tap = pair.net.tap();
        let handshakes_after_establish = handshakes(&tap);
        let (dhs_a, dhs_b) = (pair.a.dhs.get(), pair.b.dhs.get());

        const LEN: usize = 96 * 1024;
        let want: Vec<u8> = (0..LEN).map(|i| (i % 251) as u8).collect();

        let mut send = ca.open_uni().await.expect("open_uni");
        let writer = async {
            let mut done = 0usize;
            while done < want.len() {
                done += send.write(&want[done..]).await.expect("write");
            }
            send.finish().await.expect("finish");
        };
        let reader = async {
            let mut recv = cb.accept_uni().await.expect("accept_uni");
            read_exactly(&mut recv, LEN, "the stream across the boundaries").await
        };
        let (_, got) = tokio::join!(writer, reader);

        assert!(
            got == want,
            "§7.7: the stream is byte-exact across every epoch boundary it \
             crossed"
        );

        // ── The boundaries were really crossed, and the counter never reset.
        let sealed = sealed_by(&tap, pair.a.addr());
        let first = sealed.first().expect("A sealed Data packets").counter;
        let last = sealed.last().expect("A sealed Data packets").counter;
        assert!(
            sealed.windows(2).all(|w| w[1].counter > w[0].counter),
            "§7.7: *the counter is never reset by the ratchet* — A's counters \
             must be strictly increasing across every boundary, got {:?}",
            sealed.iter().map(|s| s.counter).collect::<Vec<_>>()
        );
        let crossed = last / EPOCH - first / EPOCH;
        assert!(
            crossed >= 3,
            "the fixture never crossed enough boundaries to prove anything: A's \
             counters ran {first}..={last} at an epoch of {EPOCH}, {crossed} \
             boundaries. This is the assertion that keeps this test from being \
             vacuous."
        );

        // ── No handshake, no DH, no application-visible event.
        assert_eq!(
            handshakes(&tap),
            handshakes_after_establish,
            "S23: *there is no DH re-handshake* — not one handshake packet may \
             appear after establishment"
        );
        assert_eq!(
            (pair.a.dhs.get(), pair.b.dhs.get()),
            (dhs_a, dhs_b),
            "S23: the ratchet is symmetric-key only; a single charged DH means a \
             handshake happened"
        );
        assert!(
            claim_ready(&cb, "after the crossings").await.is_empty(),
            "no datagram was sent, so nothing may be readable: the ratchet \
             surfaces nothing to the application"
        );

        // Still usable afterwards, in both directions: the connection did not
        // quietly half-die at a boundary.
        nudge(&ca, 1).await;
        nudge(&cb, 2).await;
        assert_eq!(
            claim_ready(&cb, "A→B after the crossings").await,
            vec![1],
            "A→B still carries datagrams after the boundaries"
        );
        assert_eq!(
            claim_ready(&ca, "B→A after the crossings").await,
            vec![2],
            "B→A still carries datagrams after the boundaries"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 3. Each direction ratchets independently
// ══════════════════════════════════════════════════════════════════════

/// §7.7: *"each direction ratchets independently"*.
///
/// A sends far enough to leave two epochs behind while B, sending nothing
/// but ACKs, stays in its own epoch 0 — proved from the wire on both sides.
/// B's next datagram then opens at A.
///
/// BROKEN BUILD: one epoch counter per **connection** rather than per
/// direction — a sender that ratcheted its seal key on the counter it last
/// *received*, or a receiver that ratcheted the key it opens with on the
/// counter it last *sent*. B's packet would then be sealed under epoch 2's
/// key and opened under epoch 0's (or the reverse) and would not open at
/// all. Nothing else in the suite separates that build: a connection that
/// only ever sends symmetric traffic keeps both directions' counters in
/// step, and the two epochs then coincide by accident.
#[tokio::test(start_paused = true)]
async fn rk3_the_two_directions_ratchet_independently() {
    local(async {
        let pair = Pair::seeded_with(0x5EED_0003, small_epoch_config());
        let (ca, cb) = pair.establish().await;
        settle().await;
        let tap = pair.net.tap();

        // A alone talks, far enough to leave two epochs behind.
        nudge(&ca, 0).await;
        let _ = claim_ready(&cb, "priming").await;
        pump_until_epoch(&pair, &ca, &cb, &tap, MAX_EPOCH_JUMP, 1).await;

        let a_epoch = sealed_by(&tap, pair.a.addr())
            .last()
            .expect("A sealed packets")
            .epoch();
        let b_epoch = sealed_by(&tap, pair.b.addr())
            .last()
            .map_or(0, |s| s.epoch());
        assert!(
            a_epoch > b_epoch,
            "the fixture never separated the two directions: A is in epoch \
             {a_epoch} and B in epoch {b_epoch}, so a build with one shared \
             epoch would be indistinguishable here"
        );

        // B answers from its own, lower, epoch. Under a shared epoch this
        // packet is sealed with the wrong key and never opens.
        nudge(&cb, 500).await;
        assert_eq!(
            claim_ready(&ca, "B's reply from its own epoch").await,
            vec![500],
            "§7.7: each direction ratchets independently — B seals in epoch \
             {b_epoch} while A is in epoch {a_epoch}, and A must open it"
        );

        // And A's direction still works from its own, higher, epoch.
        nudge(&ca, 501).await;
        assert_eq!(
            claim_ready(&cb, "A's reply from its own epoch").await,
            vec![501],
            "§7.7: and the higher-epoch direction is unaffected by the lower one"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 4. THE SEPARATOR — two epochs back does not open
// ══════════════════════════════════════════════════════════════════════

/// **§7.7's refusal, and the one assertion in this file a never-rekey build
/// fails.**
///
/// > The receiver retains the current and immediately preceding epoch keys
/// > […]; anything older is refused, its key ratcheted away.
///
/// A datagram is sealed in epoch *e*, held off the wire, and posted only
/// after B has committed to epoch *e + 2* — a distance proved from the tap,
/// not assumed. It must not reach the application.
///
/// **The oracle is inside the test.** An absence proves nothing on its own:
/// a fixture where `inject` never worked, or where the connection had died,
/// would produce the same silence. So a *second* packet is held in the
/// current epoch and injected the same way immediately afterwards, and it
/// must arrive. The order is deliberate — the refusal is asserted first, by
/// absence, and the arrival that follows proves the instrument was working
/// when that absence was measured (working rule 12: a true lemma about the
/// wrong state proves nothing).
///
/// BROKEN BUILD: **the never-rekey build** — `epoch_size` hard-wired to
/// `u64::MAX`, or `Config::with_epoch_size` ignored, or any schedule under
/// which no epoch is ever left. Every key stays epoch 0's, the held packet
/// opens on arrival, tag 1 reaches the application and this test is red.
/// Also red on a receiver that keeps *three* epochs instead of two, or that
/// re-derives an old key on demand — §7.7 forbids both: *"anything older is
/// refused, its key ratcheted away […] Implementations must not chase
/// epochs."*
///
/// NOT SEPARATED, deliberately: §7.2's replay window cannot produce this
/// silence. The held packet was destroyed by the fabric before delivery, so
/// B has never seen its counter, and the distance from B's highest opened
/// counter is under 60 — far inside `REPLAY_WINDOW`'s 2 048.
#[tokio::test(start_paused = true)]
async fn rk4_a_packet_two_epochs_back_does_not_open() {
    local(async {
        let pair = Pair::seeded_with(0x5EED_0004, small_epoch_config());
        let (ca, cb) = pair.establish().await;
        settle().await;
        let tap = pair.net.tap();

        // The fixture delivers, before anything is held.
        nudge(&ca, 0).await;
        assert_eq!(
            claim_ready(&cb, "the warm-up datagram").await,
            vec![0],
            "the fixture must deliver over a perfect path before an absence \
             below can mean anything"
        );

        // Hold tag 1 in whatever epoch A is in now.
        let (stale, stale_at) = hold_one(&pair, &ca, &tap, 1).await;
        let stale_epoch = stale_at.epoch();

        // Drive B's commit two epochs past it. B commits on packets that
        // **open**, so the arrival of the pump's datagrams is what moves it.
        let next_tag =
            pump_until_epoch(&pair, &ca, &cb, &tap, stale_epoch + MAX_EPOCH_JUMP, 2).await;
        let marker = next_tag;
        nudge(&ca, marker).await;
        let landed = *sealed_by(&tap, pair.a.addr())
            .last()
            .expect("A sealed packets");
        assert_eq!(
            claim_ready(&cb, "the marker that moves B's commit").await,
            vec![marker],
            "B must open a packet in epoch {} for its commit to be there — this \
             arrival is what makes the held packet two epochs old rather than \
             merely old-looking",
            landed.epoch()
        );
        assert!(
            landed.epoch() >= stale_epoch + MAX_EPOCH_JUMP,
            "B committed only to epoch {} while the held packet is in epoch \
             {stale_epoch}: that is one epoch back, which §7.7 says must open, \
             and this test would then be asserting the opposite of the spec",
            landed.epoch()
        );

        // The refusal, asserted by absence.
        pair.net.inject(pair.a.addr(), pair.b.addr(), &stale);
        settle().await;
        let arrived = claim_ready(&cb, "after the stale packet was posted").await;
        assert!(
            arrived.is_empty(),
            "§7.7: a packet from epoch {stale_epoch}, {} epochs behind B's \
             commit at epoch {}, must be refused *without key derivation* — it \
             reached the application instead, as {arrived:?}. This is the \
             never-rekey build's signature.",
            landed.epoch() - stale_epoch,
            landed.epoch()
        );

        // …and the refusal is silent: a generic decryption failure, not a
        // protocol violation. The connection is untouched.
        nudge(&ca, 900).await;
        assert_eq!(
            claim_ready(&cb, "after the refusal").await,
            vec![900],
            "§7.7: the refusal is *a generic decryption failure at the hiss \
             surface* — it may not kill or disturb the connection"
        );

        // THE ORACLE. A packet held the same way in the current epoch, and
        // posted the same way, must arrive — so the silence above was the
        // epoch and not the instrument.
        let (fresh, fresh_at) = hold_one(&pair, &ca, &tap, 3).await;
        assert!(
            fresh_at.epoch() >= landed.epoch(),
            "the oracle packet must be in B's committed epoch or later, else it \
             proves nothing about the instrument"
        );
        pair.net.inject(pair.a.addr(), pair.b.addr(), &fresh);
        settle().await;
        assert_eq!(
            claim_ready(&cb, "the oracle").await,
            vec![3],
            "a held-and-injected packet in the current epoch must arrive: if it \
             does not, `inject` or the hold is broken and the absence asserted \
             above proves nothing"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 5. Straggler tolerance — one epoch back still opens
// ══════════════════════════════════════════════════════════════════════

/// §7.7: *"The receiver retains the current and immediately preceding epoch
/// keys (straggler tolerance: one epoch back)"*.
///
/// The mirror of [`rk4`](rk4_a_packet_two_epochs_back_does_not_open), and
/// the reason that test cannot stand alone. A datagram sealed in epoch *e*
/// is held, B is driven to commit epoch *e + 1* — **exactly** one, asserted
/// from the tap — and the straggler is then posted. It must open.
///
/// BROKEN BUILD: **the current-epoch-only receiver**, which retains one key
/// and ratchets the previous one away as soon as it commits. It passes
/// [`rk4`](rk4_a_packet_two_epochs_back_does_not_open) — it refuses
/// *everything* old — and fails here. A real network reorders, and a
/// straggler from the last epoch is the ordinary case, not the exotic one:
/// this test is the difference between rotation and data loss.
///
/// # Why this lives at the knob and cannot be repeated at 65 536
///
/// §7.2's replay window binds first. Measured on this fixture at the
/// production epoch, with the held packet and the receiver's commit inside
/// **one** epoch throughout — so the epoch cannot be what refuses:
///
/// | counters back | same epoch | opens |
/// |---|---|---|
/// | 1 000 | yes | yes |
/// | 3 000 | yes | **no** |
/// | 5 000 | yes | **no** |
///
/// `REPLAY_WINDOW` is 2 048 and one production epoch is 65 536, so *"one
/// epoch back"* is never reachable at the ratified constant: the retained
/// previous-epoch key is used only for packets within `REPLAY_WINDOW`
/// counters of a boundary. That is exactly what it is for, and exactly what
/// this test constructs — at an epoch of [`EPOCH`], where the two limits do
/// not overlap. `story_rekey.rs` records the same measurement at the point
/// where a reader would otherwise ask why the straggler is not asserted
/// there.
#[tokio::test(start_paused = true)]
async fn rk5_a_packet_one_epoch_back_still_opens() {
    local(async {
        let pair = Pair::seeded_with(0x5EED_0005, small_epoch_config());
        let (ca, cb) = pair.establish().await;
        settle().await;
        let tap = pair.net.tap();

        nudge(&ca, 0).await;
        assert_eq!(
            claim_ready(&cb, "the warm-up datagram").await,
            vec![0],
            "the fixture must deliver over a perfect path first"
        );

        let (straggler, straggler_at) = hold_one(&pair, &ca, &tap, 1).await;
        let straggler_epoch = straggler_at.epoch();

        // Exactly one epoch forward — `pump_until_epoch` asserts it did not
        // overshoot, which is what keeps this from silently becoming rk4.
        let next_tag = pump_until_epoch(&pair, &ca, &cb, &tap, straggler_epoch + 1, 2).await;
        let marker = next_tag;
        nudge(&ca, marker).await;
        let landed = *sealed_by(&tap, pair.a.addr())
            .last()
            .expect("A sealed packets");
        assert_eq!(
            claim_ready(&cb, "the marker that moves B's commit").await,
            vec![marker],
            "B must open a packet in the new epoch for its commit to move there"
        );
        assert_eq!(
            landed.epoch(),
            straggler_epoch + 1,
            "B must be **exactly** one epoch ahead of the straggler: at two it \
             would be §7.7's refusal case and this test would assert the \
             opposite of the spec"
        );

        pair.net.inject(pair.a.addr(), pair.b.addr(), &straggler);
        settle().await;
        assert_eq!(
            claim_ready(&cb, "the straggler from the previous epoch").await,
            vec![1],
            "§7.7: the receiver retains the immediately preceding epoch's key — \
             a packet sealed in epoch {straggler_epoch} must still open after \
             the commit to epoch {}",
            landed.epoch()
        );
    })
    .await;
}

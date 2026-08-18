//! Slice 5a acceptance tests for §12 (ACK).
//!
//! Written by the blind §12 test author **from `SPEC.md` §12 (with §7.2,
//! §8.3/§8.4 and §16.5), `CONTRACT-5a.md` and `PLAN-5.md` §7 alone**, in
//! parallel with the implementation and with the §13/§14 author, and
//! without reading either (CLAUDE.md working rule 6).
//!
//! # Two layers, and the choice between them is not cosmetic
//!
//! - **`ack::AckState` and `ack::derive` directly.** §12.2's derivation is
//!   a pure function of the replay window and §12.4's policy is a pure
//!   function of four scalars, so the arithmetic is asserted where every
//!   input is chosen: no packet, no clock, no seal. `CONTRACT-5a.md` §2.1
//!   says in terms that `derive` "does not consult `AckState`", which is
//!   what makes this layer possible at all.
//! - **[`Solo`] — one core plus a raw hiss half**, for everything that is
//!   a statement about *packets*: that a pure ACK is `seal_quiet` and not
//!   ack-eliciting, that a replayed datagram advances nothing, that a
//!   structurally-bad ACK takes its whole packet down with it.
//!
//! # This file decodes the wire itself
//!
//! [`testfix::parse_frames`] **panics** on frame type `0x02` — deliberately,
//! because a slice-4 core emitting an ACK had crossed its slice boundary.
//! Slice 5 makes that emission correct, so this file carries its own
//! [`AckFields`] decode and its own §12.1 range arithmetic, and hands only
//! the *rest* of each plaintext to `parse_frames`. Two consequences worth
//! stating:
//!
//! - the ACK's meaning is computed here from §12.1's text, never through
//!   `frame::Ack::ranges_desc`, so a codec that agrees with itself about a
//!   private representation and writes the wrong bytes fails here;
//! - a build that packs the ACK anywhere but **first** (§8.5) reaches
//!   `parse_frames` with a `0x02` and panics with that message. That panic
//!   is an assertion, not an accident.
//!
//! # Working rule 9 is the organising principle
//!
//! Every test carries a `Mutation caught:` line naming what a broken build
//! does and which assertion separates it. §12 is unusually full of bounds a
//! collapsed build satisfies for free — "`ack_delay` is small" is true of a
//! build that reports 0 forever, and "at most 64 ranges" is true of a build
//! that emits one. Where the spec admits more than one conforming output
//! (which ranges survive truncation, how many pairs a tight `room` keeps)
//! the assertion is deliberately weakened to the property §12 actually
//! states: an assertion a conforming build can fail is a flake, not a pin.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here reads
//! a clock. Plain `#[test]`, no `sleep`. The paused-clock requirement lands
//! on 5b's `tests/story_reliability.rs`.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::time::{Duration, Instant};

use super::*;

// ═══════════════════════════════════════════════════════════════════════
// The integration seam
// ═══════════════════════════════════════════════════════════════════════
//
// `CONTRACT-5a.md` §2.1 pins `ack.rs`'s surface but not the module path of
// the types it borrows. **If integration has to touch anything in this
// file, expect it to be these lines.**
use super::ack::{self, AckAction, AckState};
use super::frame;
use super::session::ReplayWindow;
use super::stream_id::Dir;
use super::streams::StreamRef;
use super::testfix::*;
use super::timers::TimerKind;

use crate::constants::{
    FRAME_ACK, FRAME_PADDING, FRAME_PING, INITIAL_WINDOW, MAX_ACK_DELAY, MAX_ACK_RANGES,
    MAX_PLAINTEXT, PROTOCOL_VIOLATION, REPLAY_WINDOW,
};

// ═══════════════════════════════════════════════════════════════════════
// §12.1, decoded by hand
// ═══════════════════════════════════════════════════════════════════════

/// §8.4's ACK, as its wire fields, in this file's own vocabulary.
///
/// Deliberately **not** `super::frame::Ack`: the derivation under test
/// produces that type, and a test that reads it back through the same
/// type's own `ranges_desc` would be asking the implementation whether it
/// agrees with itself.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AckFields {
    largest: u64,
    ack_delay: u64,
    first_range: u64,
    ranges: Vec<(u64, u64)>,
}

impl AckFields {
    /// §12.1's range semantics, written out from the spec text:
    ///
    /// > the first block covers `largest − first_range ..= largest`; for
    /// > each subsequent `(gap, range)` pair, with `prev_smallest` the
    /// > smallest counter of the preceding block: the block's largest is
    /// > `prev_smallest − gap − 2`, and the block covers
    /// > `block_largest − range ..= block_largest`.
    ///
    /// Checked subtraction throughout: §12.1's third bullet makes a block
    /// descending below counter zero a structural failure, so an underflow
    /// here is a test-side bug and must panic rather than saturate into a
    /// plausible-looking range.
    fn blocks(&self) -> Vec<std::ops::RangeInclusive<u64>> {
        let mut out = Vec::with_capacity(1 + self.ranges.len());
        let mut smallest = self
            .largest
            .checked_sub(self.first_range)
            .expect("§12.1: the first block must not descend below counter zero");
        out.push(smallest..=self.largest);
        for (gap, range) in &self.ranges {
            let block_largest = smallest
                .checked_sub(*gap)
                .and_then(|v| v.checked_sub(2))
                .expect("§12.1: a block must not descend below counter zero");
            smallest = block_largest
                .checked_sub(*range)
                .expect("§12.1: a block must not descend below counter zero");
            out.push(smallest..=block_largest);
        }
        out
    }

    /// Every counter the ACK acknowledges, flattened. Only for ACKs whose
    /// span is small — the whole point of §12.5 is that the *implementation*
    /// never does this.
    fn counters(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for b in self.blocks() {
            assert!(
                b.end() - b.start() < 4096,
                "this helper is for small spans only"
            );
            // Descending **within** a block as well as across them, which is
            // the order every expected literal in this file is written in.
            // `extend(b)` over a `RangeInclusive` walks ascending, which
            // contradicted those literals in four tests; the literals were
            // right (integration, ruling 142).
            out.extend(b.rev());
        }
        out
    }
}

/// Decode a plaintext's **leading** ACK, returning it and the rest.
///
/// §8.5 packs the ACK first, so a plaintext that begins with `0x02` has its
/// ACK here and anything later is another frame's business.
fn split_ack(pt: &[u8]) -> (Option<AckFields>, Vec<u8>) {
    if pt.first() != Some(&(FRAME_ACK as u8)) {
        return (None, pt.to_vec());
    }
    let mut at = 0usize;
    let ty = take_varint(pt, &mut at);
    assert_eq!(ty, FRAME_ACK);
    let largest = take_varint(pt, &mut at);
    let ack_delay = take_varint(pt, &mut at);
    let range_count = take_varint(pt, &mut at);
    let first_range = take_varint(pt, &mut at);
    let mut ranges = Vec::with_capacity(range_count as usize);
    for _ in 0..range_count {
        let gap = take_varint(pt, &mut at);
        let range = take_varint(pt, &mut at);
        ranges.push((gap, range));
    }
    (
        Some(AckFields {
            largest,
            ack_delay,
            first_range,
            ranges,
        }),
        pt[at..].to_vec(),
    )
}

/// One packet a drain produced: its leading ACK, and everything else.
type Packet = (Option<AckFields>, Vec<Wire>);

/// Every datagram a drain produced, decoded.
///
/// The `parse_frames` call is what asserts §8.5's order: a second ACK, or a
/// first one that is not the packet's first frame, panics there.
fn packets(s: &mut Solo, d: &Drained) -> Vec<Packet> {
    d.transmits()
        .iter()
        .map(|t| {
            let pt = s.peer.open_dgram(&t.data);
            let (ack, rest) = split_ack(&pt);
            (ack, parse_frames(&rest))
        })
        .collect()
}

/// Just the ACKs, in generation order.
fn acks(s: &mut Solo, d: &Drained) -> Vec<AckFields> {
    packets(s, d).into_iter().filter_map(|(a, _)| a).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// Frames the core has no verb to produce
// ═══════════════════════════════════════════════════════════════════════

fn ping() -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_PING);
    f
}

fn padding(n: usize) -> Vec<u8> {
    let mut f = Vec::new();
    for _ in 0..n {
        put(&mut f, FRAME_PADDING);
    }
    f
}

/// An ACK frame, encoded exactly as §8.4 lays it out.
fn ack_frame(largest: u64, ack_delay: u64, first_range: u64, pairs: &[(u64, u64)]) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_ACK);
    put(&mut f, largest);
    put(&mut f, ack_delay);
    put(&mut f, pairs.len() as u64);
    put(&mut f, first_range);
    for (gap, range) in pairs {
        put(&mut f, *gap);
        put(&mut f, *range);
    }
    f
}

/// §3.4's counter, read from the cleartext header.
fn counter_of(dgram: &[u8]) -> u64 {
    u64::from_le_bytes(dgram[6..14].try_into().expect("§3.4 data header"))
}

/// Seal `pt` as the peer so that it lands on exactly `counter`, burning and
/// discarding the packets in between.
///
/// hiss owns the counter and only moves it forward (§7.1), so a test that
/// needs a packet at a chosen counter seals it *when* that counter comes up.
/// Discarded packets are never delivered, so the core never sees them —
/// which is precisely how a gap is opened in its replay window.
fn seal_at(peer: &mut RawPeer, counter: u64, pt: &[u8]) -> Vec<u8> {
    for _ in 0..4096 {
        let d = peer.seal(pt);
        let c = counter_of(&d);
        assert!(c <= counter, "§7.1: the counter only goes forward");
        if c == counter {
            return d;
        }
    }
    panic!("seal_at({counter}) never reached its counter");
}

/// `handle_datagram` from the peer's address, then drain — without sealing,
/// so a datagram can be delivered twice (§7.2's replay).
fn feed(s: &mut Solo, now: Instant, dgram: &[u8]) -> Drained {
    s.conn.handle_datagram(now, a_addr(), dgram);
    drain(&mut s.conn)
}

/// **The end of the driver's receive drain, at the core seam.**
///
/// **[ruling 271]** §12.4's every-2nd trigger arms `AckDelay` at `now`
/// itself — a deadline *already due* — and the ACK stays `pending` until
/// something fires it. `shell::driver`'s `biased` `select!` is what fires
/// it: `recv_from` sits above `sleep_until`, so an already-due deadline
/// loses to every datagram still on the socket and wins the instant the
/// socket empties. The core has no notion of that boundary, so a core-level
/// test that wants the coalesced ACK **on the wire** supplies it here.
///
/// `handle_timeout` runs only deadlines that are due (§16.5), so this is a
/// no-op while the ACK is still riding `now + MAX_ACK_DELAY` — which is
/// exactly what makes it usable as an unconditional "the socket is empty
/// now" step, and what lets the tests below keep asserting *which* of the
/// two arms §12.4 took.
fn drain_boundary(s: &mut Solo, now: Instant) -> Drained {
    s.conn.handle_timeout(now);
    drain(&mut s.conn)
}

/// A window with `counters` marked, in the order given.
fn window_of(counters: &[u64]) -> ReplayWindow {
    let mut w = ReplayWindow::new();
    for c in counters {
        assert!(w.check_and_mark(*c), "the fixture must mark {c} fresh");
    }
    w
}

/// `derive`'s output in this file's vocabulary.
fn derived(w: &ReplayWindow, delay_us: u64, room: usize) -> Option<AckFields> {
    let a = ack::derive(w, delay_us, room)?;
    let mut bytes = Vec::new();
    frame::Frame::Ack(a).encode(&mut bytes);
    let (fields, rest) = split_ack(&bytes);
    assert!(rest.is_empty(), "the ACK must encode to exactly its bytes");
    fields
}

/// How many plaintext bytes `derive`'s output occupies, including the type
/// byte — the quantity `room` bounds.
fn derived_len(w: &ReplayWindow, delay_us: u64, room: usize) -> usize {
    let a = ack::derive(w, delay_us, room).expect("an ACK was expected here");
    frame::Frame::Ack(a).encoded_len()
}

// ═══════════════════════════════════════════════════════════════════════
// §12.1 — range semantics
// ═══════════════════════════════════════════════════════════════════════

mod range_semantics {
    use super::*;

    /// §12.1's `(gap, range)` arithmetic, against literals computed by hand.
    ///
    /// Window: 98..=100, 94..=95, 86..=88, so
    /// `largest = 100`, `first_range = 2`; then
    /// `gap = 98 − 95 − 2 = 1`, `range = 95 − 94 = 1`; then
    /// `gap = 94 − 88 − 2 = 4`, `range = 88 − 86 = 2`.
    ///
    /// Mutation caught: the off-by-one that makes `gap` the *count of
    /// missing counters* (`prev_smallest − block_largest − 1`) rather than
    /// §12.1's `− 2`. It emits `[(2, 1), (5, 2)]`, which a conforming
    /// decoder reads as `94..=97` and `85..=87` — every block shifted by
    /// one, and each subsequent block shifted again. Asserting the decoded
    /// *blocks* alone would catch it too; asserting the raw pairs as well
    /// names which side of the encoding is wrong.
    #[test]
    fn pairs_encode_the_spec_arithmetic_not_the_missing_counter_count() {
        let w = window_of(&[100, 99, 98, 95, 94, 88, 87, 86]);
        let a = derived(&w, 0, MAX_PLAINTEXT).expect("the window has a greatest");

        assert_eq!(a.largest, 100, "§12.1: the first block's top");
        assert_eq!(a.first_range, 2, "§12.1: 100 − 98");
        assert_eq!(
            a.ranges,
            vec![(1, 1), (4, 2)],
            "§12.1: block_largest = prev_smallest − gap − 2"
        );
        assert_eq!(
            a.blocks(),
            vec![98..=100, 94..=95, 86..=88],
            "and they decode back to the window's ranges"
        );
    }

    /// The derived ACK means exactly what the window holds — no block
    /// invented, none dropped, when nothing binds.
    ///
    /// Mutation caught: a build that walks the bitmap from the *oldest* end
    /// and reverses at the end gets the blocks right but the `largest`
    /// wrong; a build that emits `range` where `gap` belongs produces a
    /// wire-legal frame acknowledging counters the peer never sent, which
    /// this equality rejects. A "the ACK has three blocks" assertion would
    /// pass both.
    #[test]
    fn derived_ranges_equal_the_windows_ranges_when_nothing_truncates() {
        let w = window_of(&[40, 39, 33, 32, 31, 20, 7, 6, 5, 4, 0]);
        let a = derived(&w, 0, MAX_PLAINTEXT).expect("the window has a greatest");

        let want: Vec<_> = w.ranges_desc().collect();
        assert_eq!(a.blocks(), want, "§12.2: derived from the window, verbatim");
        assert_eq!(
            a.counters(),
            vec![40, 39, 33, 32, 31, 20, 7, 6, 5, 4, 0],
            "and the flattened counters are exactly what was marked"
        );
    }

    /// A single received counter is one block and zero pairs.
    ///
    /// Mutation caught: a build that always emits at least one `(gap,
    /// range)` pair — the shape you get from a loop written as
    /// `for i in 0..=n`. Its pair would have to descend below the greatest
    /// and, at counter 0, below zero.
    #[test]
    fn one_counter_is_one_block_and_no_pairs() {
        let a = derived(&window_of(&[0]), 0, MAX_PLAINTEXT).expect("greatest is 0");
        assert_eq!(a.largest, 0);
        assert_eq!(a.first_range, 0);
        assert!(a.ranges.is_empty(), "§12.1: no subsequent blocks exist");
        assert_eq!(a.blocks(), vec![0..=0]);
    }

    /// §12.1's third bullet, **both sides**: a first block reaching exactly
    /// counter zero is legal; one counter further is structural.
    ///
    /// This is slice 1's one-sided-boundary defect, applied to the descent
    /// rule. `tests.rs` already pins the illegal side at the codec; the
    /// *legal* side is what a build with an off-by-one in its underflow
    /// check gets wrong, and it is the side no test held.
    ///
    /// Mutation caught: `checked_sub(first_range).filter(|v| *v > 0)` —
    /// i.e. treating "reaches zero" as "descends below zero". It kills a
    /// connection on a wire-legal ACK, which is unrecoverable and silent
    /// until a peer's counter space is young.
    #[test]
    fn a_block_reaching_counter_zero_is_legal_and_one_below_is_structural() {
        let t = t0();

        // Legal: 5 − 5 = 0.
        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(5, 0, 5, &[]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"legal", false));
        let d = s.deliver(t, &pt);
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            1,
            "§12.1: a block reaching exactly zero is not a failure, so the \
             packet's other frames apply"
        );

        // Structural: 5 − 6 underflows.
        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(5, 0, 6, &[]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"illegal", false));
        let d = s.deliver(t, &pt);
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, PROTOCOL_VIOLATION);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "§8.2: nothing from the packet is applied"
        );
    }

    /// The same boundary on a `(gap, range)` pair rather than the first
    /// block, because the two descents are computed by different code.
    ///
    /// `largest = 5`, `first_range = 0`, `gap = 3` ⇒ `5 − 3 − 2 = 0`: legal,
    /// and exactly zero. `gap = 4` ⇒ −1: structural.
    ///
    /// Mutation caught: an underflow check applied only to `first_range`
    /// (the obvious one to write, since it is the only subtraction spelled
    /// out in §12.1's first bullet) and not to the `− gap − 2` descent.
    #[test]
    fn a_pair_descending_to_exactly_zero_is_legal_and_one_below_is_structural() {
        let t = t0();

        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(5, 0, 0, &[(3, 0)]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"legal", false));
        let d = s.deliver(t, &pt);
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            1
        );

        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(5, 0, 0, &[(4, 0)]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"illegal", false));
        let d = s.deliver(t, &pt);
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, PROTOCOL_VIOLATION);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0
        );
    }

    /// §12.1: *"`ack_delay` is raw microseconds as a varint — no exponent
    /// scaling."*
    ///
    /// Mutation caught: the QUIC habit of an `ack_delay_exponent` (RFC 9000
    /// defaults it to 3, i.e. units of 8 µs) or of reporting milliseconds.
    /// `12 345` was chosen because it survives neither: a ÷8 build writes
    /// `1543`, a ÷1000 build writes `12`, and a build that rounds to the
    /// millisecond writes `12000`. A value like `8000` would pass the ÷8
    /// build's round trip if the reader scaled back, and a "delay is
    /// plausible" bound would pass all of them.
    #[test]
    fn ack_delay_is_carried_as_raw_microseconds() {
        let a = derived(&window_of(&[9]), 12_345, MAX_PLAINTEXT).expect("greatest is 9");
        assert_eq!(a.ack_delay, 12_345, "§12.1: raw µs, no exponent scaling");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12.2 — derivation, fused to the replay window
// ═══════════════════════════════════════════════════════════════════════

mod derivation {
    use super::*;

    /// An empty window derives nothing. §2.1: *"Returns `None` iff the
    /// window has no greatest … in which case no ACK can be owed either."*
    ///
    /// Mutation caught: a build defaulting `greatest` to 0 and emitting an
    /// ACK for counter 0 before anything has been received — which
    /// acknowledges a packet the peer did send, and retires it from the
    /// peer's loss recovery without it ever having arrived.
    #[test]
    fn an_empty_window_derives_no_ack() {
        assert!(ack::derive(&ReplayWindow::new(), 0, MAX_PLAINTEXT).is_none());
    }

    /// **A-4.** The 2048-bit alternating worst case §12.2 names: it no
    /// longer fits one packet, so construction truncates at
    /// `MAX_ACK_RANGES` **pairs**, keeping the newest.
    ///
    /// Three separate assertions, because they fail to three different
    /// bugs:
    ///
    /// 1. `ranges.len() == MAX_ACK_RANGES` — exactly 64 pairs, 65 blocks.
    /// 2. `largest == window.greatest()` — the *newest* end survived.
    /// 3. the surviving blocks are a **prefix** of the window's own
    ///    newest-first walk — nothing was reordered or skipped.
    ///
    /// Mutation caught: a build that truncates from the newest end (the
    /// natural shape if you collect the window's ranges into a `Vec` and
    /// then `truncate` after reversing) still emits 64 pairs and still
    /// satisfies any `<= 64` bound — assertion 2 is what fails it, because
    /// its `largest` would be ~2 000 counters stale, and a stale `largest`
    /// is how a sender concludes its newest flight was lost. Assertion 1
    /// alone is passed by a build that emits 64 pairs of the *oldest*
    /// ranges; assertion 2 alone is passed by a build with no cap at all,
    /// which then overflows the packet.
    ///
    /// Note the assertion this test does **not** make: it does not require
    /// any *particular* 65 blocks beyond their being the newest 65. §12.2
    /// says "truncating at `MAX_ACK_RANGES` pairs **or** at packet
    /// capacity, whichever binds", and with `room` this large only the pair
    /// cap binds — but a build that also stopped early for a reason of its
    /// own would be conforming and is not this test's business.
    #[test]
    fn the_alternating_worst_case_truncates_at_sixty_four_pairs_newest_first() {
        // 0, 2, 4, … 2046: the alternating window, marked ascending so the
        // greatest is the newest.
        let counters: Vec<u64> = (0..REPLAY_WINDOW as u64).step_by(2).collect();
        let w = window_of(&counters);
        let greatest = w.greatest().expect("something was marked");
        assert_eq!(greatest, REPLAY_WINDOW as u64 - 2);

        let available: Vec<_> = w.ranges_desc().collect();
        assert!(
            available.len() > MAX_ACK_RANGES + 1,
            "the fixture must offer more blocks than the cap admits, or the \
             cap is not exercised: {} available",
            available.len()
        );

        let a = derived(&w, 0, MAX_PLAINTEXT).expect("the window has a greatest");

        assert_eq!(
            a.ranges.len(),
            MAX_ACK_RANGES,
            "§12.2: the cap is on the (gap, range) pairs"
        );
        assert_eq!(
            a.blocks().len(),
            MAX_ACK_RANGES + 1,
            "§12.2: at most 65 blocks including the first"
        );
        assert_eq!(
            a.largest, greatest,
            "§12.2: newest-first — the greatest survives truncation"
        );
        assert_eq!(
            a.blocks(),
            available[..=MAX_ACK_RANGES].to_vec(),
            "§12.2: the surviving blocks are the newest ones, in order"
        );
    }

    /// The complement: a window with **fewer** than 65 blocks is not
    /// padded up to the cap.
    ///
    /// Mutation caught: a build that emits `MAX_ACK_RANGES` pairs
    /// unconditionally, filling with zeros. Wire-legal, and it acknowledges
    /// 128 counters below the window that were never received.
    #[test]
    fn a_small_window_is_not_padded_to_the_cap() {
        let w = window_of(&[10, 8, 6]);
        let a = derived(&w, 0, MAX_PLAINTEXT).expect("greatest is 10");
        assert_eq!(a.ranges.len(), 2, "three blocks means two pairs");
        assert_eq!(a.blocks(), vec![10..=10, 8..=8, 6..=6]);
    }

    /// **A-4, capacity half.** `room` binds before the pair cap does, and
    /// the frame that comes back fits.
    ///
    /// The boundary is taken from **both sides** at a value computed from
    /// the build's own untruncated output, so the test states a
    /// relationship rather than a magic number: at `room == len` nothing
    /// binds and the full frame comes back; at `room == len − 1` something
    /// must have been dropped, and whatever comes back still fits.
    ///
    /// Mutation caught: a build that ignores `room` entirely returns the
    /// same 64-pair frame at `len − 1`, which then overruns the plaintext
    /// it was packed into — §8.6's bound is checked at seal time, so the
    /// symptom is a failed seal, and §7.9 makes a failed seal terminal.
    ///
    /// Deliberately **not** asserted: *how many* pairs survive at
    /// `len − 1`. §12.2 fixes only that the truncation is newest-first and
    /// that the result fits; a build dropping one pair and a build dropping
    /// five are both conforming.
    #[test]
    fn room_truncates_from_the_old_end_and_the_result_always_fits() {
        let counters: Vec<u64> = (0..REPLAY_WINDOW as u64).step_by(2).collect();
        let w = window_of(&counters);
        let greatest = w.greatest().expect("something was marked");

        let full_len = derived_len(&w, 0, MAX_PLAINTEXT);
        let full = derived(&w, 0, MAX_PLAINTEXT).expect("greatest exists");

        // Exactly enough room: nothing binds but the pair cap.
        assert_eq!(
            derived(&w, 0, full_len),
            Some(full.clone()),
            "§12.2: at exactly its own length, capacity does not bind"
        );

        // One byte less: capacity binds.
        let tight = derived(&w, 0, full_len - 1).expect("the first block still fits");
        assert!(
            tight.ranges.len() < full.ranges.len(),
            "§12.2: one byte less must cost at least one pair"
        );
        assert!(
            derived_len(&w, 0, full_len - 1) < full_len,
            "§2.1: the returned frame always encodes to <= room bytes"
        );
        assert_eq!(
            tight.largest, greatest,
            "§12.2: capacity truncation is newest-first too"
        );
    }

    /// `room` too small for even the first block: `None`, and §2.1 says the
    /// ACK then stays owed for the next packet.
    ///
    /// Both sides of the smallest viable `room`, computed rather than
    /// hardcoded: at `first_only_len` the single-block ACK comes back; one
    /// byte below it, nothing does.
    ///
    /// Mutation caught: a build that checks `room` only inside the pair
    /// loop and emits the first block unconditionally. It returns a frame
    /// larger than `room`, and because the first block is the *whole point*
    /// of the ACK the overrun is exactly where it is least expected.
    #[test]
    fn no_ack_is_derived_when_even_the_first_block_does_not_fit() {
        let w = window_of(&[1000, 999, 990, 980]);

        // The single-block frame's length: derive against a `room` that
        // admits the first block and nothing else is not knowable up front,
        // so take the length of the frame the build produces for a
        // single-block window with the same `largest`.
        let single = window_of(&[1000, 999]);
        let first_only_len = derived_len(&single, 0, MAX_PLAINTEXT);

        assert!(
            derived(&w, 0, first_only_len).is_some(),
            "§2.1: room for the first block is room for an ACK"
        );
        assert!(
            ack::derive(&w, 0, first_only_len - 1).is_none(),
            "§2.1: if even the first block does not fit, this returns None"
        );
        assert!(
            ack::derive(&w, 0, 0).is_none(),
            "and zero room is the degenerate case of the same rule"
        );
    }

    /// §2.1: *"`derive` reads only the window. It does **not** consult
    /// `AckState`; the delay is passed in."*
    ///
    /// Mutation caught: a build that clamps `ack_delay` to `MAX_ACK_DELAY`
    /// at derivation. §12.3 puts that cap on the **estimator's**
    /// subtraction (§13.1), not on the reported value: capping at emission
    /// makes a genuinely late ACK indistinguishable from a punctual one and
    /// silently inflates the peer's RTT. `40_000` µs is above the 25 ms cap
    /// precisely so a clamping build reports `25_000`.
    #[test]
    fn the_reported_delay_is_not_capped_at_max_ack_delay() {
        let over = u64::try_from(MAX_ACK_DELAY.as_micros()).expect("25 ms fits") + 15_000;
        let a = derived(&window_of(&[3]), over, MAX_PLAINTEXT).expect("greatest is 3");
        assert_eq!(
            a.ack_delay, over,
            "§12.3: the cap is the estimator's, applied on receipt (§13.1)"
        );
    }

    /// §7.2's fusion, from the other side: the window **is** the record, so
    /// a counter the window has forgotten is not acknowledged.
    ///
    /// A counter more than `REPLAY_WINDOW` behind the greatest is dropped
    /// by §7.2 and cannot appear in an ACK. The window's own walk stops at
    /// the edge; this asserts the derivation does not reach past it.
    ///
    /// Mutation caught: a build keeping its own side-table of received
    /// counters — the "second tracker" §12.2 forbids — would still hold
    /// counter 0 here and acknowledge it. That is not a wire error; it is
    /// the fused design quietly not being the design.
    #[test]
    fn the_derivation_never_reaches_past_the_windows_edge() {
        let mut w = ReplayWindow::new();
        assert!(w.check_and_mark(0));
        let far = REPLAY_WINDOW as u64 + 1;
        assert!(w.check_and_mark(far), "far ahead is always fresh");
        assert!(
            !w.would_accept(0),
            "§7.2: 0 is now {} behind and outside the window",
            far
        );

        let a = derived(&w, 0, MAX_PLAINTEXT).expect("greatest is far");
        assert_eq!(a.largest, far);
        assert!(
            !a.counters().contains(&0),
            "§12.2: the window is the single record, and it has forgotten 0"
        );
        assert_eq!(a.blocks(), vec![far..=far]);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12.3 — ack_delay
// ═══════════════════════════════════════════════════════════════════════

mod ack_delay {
    use super::*;

    /// **A-3.** The delay is measured from the arrival of the packet
    /// bearing `largest` — asserted by **equality** against an injected
    /// delta, not by a bound.
    ///
    /// Mutation caught: a build reporting `0` always. Every "the delay is
    /// small" or "the delay is at most `MAX_ACK_DELAY`" bound passes it for
    /// free — this is slice 2a's defect exactly, and §12.3 is where it
    /// would cost the peer's RTT estimate.
    #[test]
    fn the_delay_is_the_measured_microseconds_since_the_largest_arrived() {
        let t = t0();
        let mut a = AckState::new();
        assert_eq!(a.on_recv(t, 0, None, true, true), AckAction::Now);
        assert_eq!(a.ack_delay_us(t + Duration::from_micros(7_300)), 7_300);
    }

    /// Measured from the packet bearing the **current** greatest, not from
    /// the first unacknowledged packet.
    ///
    /// Mutation caught: `largest_at` stamped on the packet that armed
    /// `AckDelay` — i.e. the *first* of the batch. It is the same field,
    /// written at the wrong moment, and it over-reports the delay by the
    /// whole batch's span. Here it would report 15 000 µs instead of 5 000,
    /// and the peer would subtract 15 ms of RTT it never spent.
    #[test]
    fn the_delay_tracks_the_newest_packet_not_the_first_of_the_batch() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 5, None, true, true);
        a.on_recv(t + Duration::from_millis(10), 6, Some(5), true, true);
        assert_eq!(
            a.ack_delay_us(t + Duration::from_millis(15)),
            5_000,
            "§12.3: from the arrival of the packet bearing `largest`"
        );
    }

    /// An older counter filling a gap does **not** move the anchor: it is
    /// not the window's greatest.
    ///
    /// Mutation caught: `largest_at = now` on every receive, unconditional
    /// — the one-liner. It reports 5 000 µs here where the true answer is
    /// 15 000, understating the delay whenever a straggler arrives late,
    /// which is exactly when the peer's RTT sample is most sensitive.
    ///
    /// This is the assertion the previous test cannot make: there, a
    /// stamp-every-receive build and a stamp-on-new-greatest build agree.
    #[test]
    fn a_gap_filling_arrival_does_not_move_the_delay_anchor() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 9, None, true, true);
        a.on_recv(t + Duration::from_millis(10), 7, Some(9), true, true);
        assert_eq!(
            a.ack_delay_us(t + Duration::from_millis(15)),
            15_000,
            "§12.3: 7 is not the window's greatest, so it is not the anchor"
        );
    }

    /// §12.3: *"When the window's largest counter was not frame-seen (a
    /// keepalive's counter, §7.5), `ack_delay = 0`."*
    ///
    /// **Both sides in one test**, because that is the whole difficulty:
    /// "0 when not frame-seen" is satisfied for free by a build that
    /// reports 0 unconditionally, and the frame-seen case is what separates
    /// them. Same instants, same counters, one flag flipped.
    ///
    /// Mutation caught: a build that ignores `frame_seen` in either
    /// direction — reporting 0 always (fails the second half), or reporting
    /// the elapsed time always (fails the first).
    #[test]
    fn a_keepalive_bearing_the_largest_yields_zero_and_a_frame_does_not() {
        let t = t0();
        let later = t + Duration::from_millis(9);

        let mut quiet = AckState::new();
        quiet.on_recv(t, 4, None, false, false);
        assert_eq!(
            quiet.ack_delay_us(later),
            0,
            "§12.3: the largest was a keepalive"
        );

        let mut seen = AckState::new();
        seen.on_recv(t, 4, None, false, true);
        assert_eq!(
            seen.ack_delay_us(later),
            9_000,
            "§12.3: a frame-bearing largest reports the real delay"
        );
    }

    /// A frame-bearing packet arriving *after* a keepalive took the
    /// greatest restores a real delay, and the reverse buries it.
    ///
    /// §7.5's keepalive is not rare, and the flag is per-*largest*, not
    /// sticky. Mutation caught: `largest_frame_seen` latched once (`|=`
    /// rather than `=`), which reports a real delay for a keepalive-topped
    /// window forever after the first frame; and its mirror, a flag never
    /// cleared back to true.
    #[test]
    fn the_frame_seen_flag_follows_the_greatest_rather_than_latching() {
        let t = t0();
        let mut a = AckState::new();

        // A frame-bearing packet, then a keepalive that becomes the
        // greatest: the answer must go back to 0.
        a.on_recv(t, 1, None, true, true);
        a.on_recv(t + Duration::from_millis(2), 2, Some(1), false, false);
        assert_eq!(
            a.ack_delay_us(t + Duration::from_millis(6)),
            0,
            "§12.3: the greatest is now a keepalive's counter"
        );

        // …and a later frame-bearing greatest restores it.
        a.on_recv(t + Duration::from_millis(8), 3, Some(2), true, true);
        assert_eq!(
            a.ack_delay_us(t + Duration::from_millis(11)),
            3_000,
            "§12.3: the newest frame-bearing greatest is the anchor again"
        );
    }

    /// §2.1: *"`0` … before any receive."*
    ///
    /// Mutation caught: `largest_at.unwrap()` on a fresh connection. §12.4
    /// says an ACK cannot be owed before a receive, so this is unreachable
    /// through the core — which is exactly why it is worth pinning here:
    /// nothing else would.
    #[test]
    fn the_delay_is_zero_before_anything_has_been_received() {
        assert_eq!(AckState::new().ack_delay_us(t0()), 0);
    }

    /// §2.1: *"in **microseconds**, saturating."*
    ///
    /// A `now` earlier than the anchor is not reachable through the core —
    /// `Instant` is monotonic and the core is handed instants in order —
    /// but the contract states saturation, and the natural spelling
    /// (`now - largest_at`) **panics** on it in debug. Cheap to pin, and it
    /// is the difference between a bug and a transport that aborts.
    #[test]
    fn a_backwards_instant_saturates_rather_than_panicking() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t + Duration::from_secs(10), 0, None, true, true);
        assert_eq!(a.ack_delay_us(t), 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12.4 — the delayed-ACK policy, as a state machine
// ═══════════════════════════════════════════════════════════════════════

mod policy {
    use super::*;

    /// §12.4: *"The first ack-eliciting packet of a session has no previous
    /// greatest, so the rule applies vacuously and yields an immediate
    /// ACK."*
    ///
    /// Mutation caught: `prev_greatest.map_or(false, |p| counter != p + 1)`
    /// — the `None ⇒ in-order` reading. It is the *more* natural spelling
    /// and it delays the session's very first ACK by 25 ms, which §12.4
    /// says in terms is the thing that "seeds the peer's RTT estimate
    /// early".
    #[test]
    fn the_first_ack_eliciting_packet_of_a_session_is_acked_immediately() {
        let t = t0();
        let mut a = AckState::new();
        assert_eq!(a.on_recv(t, 0, None, true, true), AckAction::Now);
        assert!(a.is_owed());
    }

    /// **A-1, both halves.** After one in-order ack-eliciting packet the
    /// ACK is armed at `MAX_ACK_DELAY`; after the second it is armed at
    /// **`now` itself** and is due.
    ///
    /// **[ruling 271]** The second half read *"after the second it is
    /// owed"*, and asserted `AckAction::Now` with `is_owed()`. §12.4's
    /// every-2nd trigger no longer emits — it **arms at `now`**, a deadline
    /// already due, which `shell::driver`'s `biased` `select!` fires the
    /// instant the socket is empty. The property this test always held is
    /// untouched: *one is not two, and two is not one*. Only the second
    /// trigger's output moved, from an emission to a due deadline.
    ///
    /// The session is primed first (its first packet forces an immediate
    /// ACK by the rule above) and `on_ack_packed()` clears it, so what this
    /// measures is the steady state and not the session opening.
    ///
    /// Mutation caught: **an immediate-ACK-per-packet build** — the
    /// pre-ratification policy §12.4 replaced — fails the first half; a
    /// **delay-only build** with no counter fails the second, because it
    /// returns `Arm(t + MAX_ACK_DELAY)` where the ratified cadence returns
    /// `Arm(t)` and leaves nothing due. Either half alone is passed by one
    /// of the two wrong builds, which is why they are one test.
    ///
    /// The last two assertions are ruling 271's own separation, and are why
    /// `is_ready()` and `is_owed()` are **both** read: a build that
    /// collapsed `pending` into `owed` — the one thing `ack.rs` says
    /// restores the pre-271 cadence *"exactly"* — satisfies `is_ready()`
    /// and fails `!is_owed()`, and no assertion on the returned
    /// `AckAction` alone can see it.
    #[test]
    fn an_ack_is_owed_after_every_second_ack_eliciting_packet() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();
        assert!(!a.is_owed(), "packing clears the debt");

        let first = a.on_recv(t, 1, Some(0), true, true);
        assert_eq!(
            first,
            AckAction::Arm(t + MAX_ACK_DELAY),
            "§12.4: the 1st unacknowledged ack-eliciting packet arms"
        );
        assert!(!a.is_ready(), "§12.4: one is not two — nothing is due yet");

        let second = a.on_recv(t, 2, Some(1), true, true);
        assert_eq!(
            second,
            AckAction::Arm(t),
            "§12.4 (ruling 271): the 2nd arms at `now` itself, already due"
        );
        assert!(a.is_ready(), "…and the ACK rides the next packet built");
        assert!(
            !a.is_owed(),
            "ruling 271: …but it is not, by itself, a reason to build one"
        );
    }

    /// The arming instant is `now + MAX_ACK_DELAY` exactly.
    ///
    /// Mutation caught: `MAX_ACK_DELAY / 2` (QUIC implementations often use
    /// a fraction of the advertised max), or `K_GRANULARITY`, or arming
    /// from the *session's* start rather than from this packet. Equality on
    /// the `Instant` is what separates them; "an `AckDelay` deadline
    /// exists" separates none.
    #[test]
    fn the_delay_timer_is_armed_at_exactly_twenty_five_milliseconds() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();

        let later = t + Duration::from_millis(400);
        assert_eq!(
            a.on_recv(later, 1, Some(0), true, true),
            AckAction::Arm(later + Duration::from_millis(25)),
            "§12.4: MAX_ACK_DELAY is 25 ms, measured from this packet"
        );
    }

    /// **A-2.** §12.4's third trigger: an ack-eliciting packet whose
    /// counter is not exactly one greater than the previous greatest is
    /// ACKed immediately — as the **1st** since the last ACK, where the
    /// "every 2nd" rule would have armed instead.
    ///
    /// All three shapes §12.4 names, plus the negative that makes them
    /// mean something.
    ///
    /// Mutation caught: a build with only the counter and the timer passes
    /// **every other §12.4 test** — it is a legal-looking delayed-ACK
    /// implementation that simply never reacts to reordering, so the peer's
    /// fast retransmit is delayed by 25 ms per gap. The negative case is
    /// what stops the mirror-image bug: a build returning `Now`
    /// unconditionally satisfies all three positives.
    #[test]
    fn out_of_order_arrival_forces_an_immediate_ack_and_in_order_does_not() {
        let t = t0();

        // Opens a gap: 7 where 6 was expected.
        let mut a = AckState::new();
        a.on_recv(t, 5, None, true, true);
        a.on_ack_packed();
        assert_eq!(
            a.on_recv(t, 7, Some(5), true, true),
            AckAction::Now,
            "§12.4: it opens a gap"
        );

        // Sits inside / fills a gap: below the greatest.
        let mut a = AckState::new();
        a.on_recv(t, 7, None, true, true);
        a.on_ack_packed();
        assert_eq!(
            a.on_recv(t, 6, Some(7), true, true),
            AckAction::Now,
            "§12.4: it fills a gap"
        );

        // A far-behind straggler, still inside the replay window.
        let mut a = AckState::new();
        a.on_recv(t, 100, None, true, true);
        a.on_ack_packed();
        assert_eq!(
            a.on_recv(t, 40, Some(100), true, true),
            AckAction::Now,
            "§12.4: it sits inside a gap"
        );

        // **The negative.** Exactly one greater: no immediate ACK.
        let mut a = AckState::new();
        a.on_recv(t, 5, None, true, true);
        a.on_ack_packed();
        assert_eq!(
            a.on_recv(t, 6, Some(5), true, true),
            AckAction::Arm(t + MAX_ACK_DELAY),
            "§12.4: exactly one greater is in order, so the 1st only arms"
        );
    }

    /// §12.4's immediate rule is scoped to **ack-eliciting** packets.
    ///
    /// §2.1: *"A packet that is window-fresh but **not** ack-eliciting
    /// neither advances `since_ack` nor arms `AckDelay`, and does not make
    /// an ACK owed."*
    ///
    /// Mutation caught: the out-of-order test applied to every window-fresh
    /// packet rather than to ack-eliciting ones. §7.5's keepalives and the
    /// peer's own pure ACKs are non-eliciting, and under loss they arrive
    /// out of order constantly — so the wrong scope turns every reordered
    /// keepalive into a reverse-path ACK. That is free amplification, and
    /// no completion test sees it.
    #[test]
    fn a_non_eliciting_out_of_order_packet_owes_nothing() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 5, None, true, true);
        a.on_ack_packed();

        assert_eq!(
            a.on_recv(t, 9, Some(5), false, true),
            AckAction::None,
            "§12.4: the immediate rule names an ack-eliciting packet"
        );
        assert!(!a.is_owed());

        // …and it did not advance the counter either: the next single
        // ack-eliciting packet must still be only the *first*.
        assert_eq!(
            a.on_recv(t, 10, Some(9), true, true),
            AckAction::Arm(t + MAX_ACK_DELAY),
            "§2.1: a non-eliciting packet does not advance `since_ack`"
        );
    }

    /// Two non-eliciting packets do not add up to an ACK.
    ///
    /// Mutation caught: `since_ack += 1` written before the `ack_eliciting`
    /// test rather than after it. The build then ACKs every second
    /// keepalive — a 1:1 reverse-path cost on an idle connection, which is
    /// the traffic §12.4's ratification note exists to remove.
    #[test]
    fn non_eliciting_packets_never_reach_the_every_second_trigger() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();

        for c in 1..=6u64 {
            assert_eq!(
                a.on_recv(t, c, Some(c - 1), false, true),
                AckAction::None,
                "counter {c}: §12.4 counts ack-eliciting packets"
            );
            assert!(!a.is_owed(), "counter {c}");
        }
    }

    /// §12.4's timer half: `AckDelay` firing makes the ACK owed.
    ///
    /// Mutation caught: `on_delay_expired` implemented as a no-op because
    /// the caller "will pack an ACK anyway" — true only while something
    /// else is owed, and false in the case the timer exists for.
    #[test]
    fn the_delay_timer_firing_makes_the_ack_owed() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();
        a.on_recv(t, 1, Some(0), true, true);
        assert!(!a.is_owed());

        a.on_delay_expired();
        assert!(a.is_owed(), "§12.4: whichever comes first");
    }

    /// Packing an ACK clears the debt **and** resets the "every 2nd"
    /// counter — §12.4 arms on the first *unacknowledged* ack-eliciting
    /// packet, so packing makes everything received acknowledged.
    ///
    /// The reset is asserted through its consequence, because
    /// `since_ack` is private: the very next in-order ack-eliciting packet
    /// must **arm** rather than being counted as a second.
    ///
    /// Mutation caught: `on_ack_packed` clearing `owed` and forgetting
    /// `since_ack`. The counter then hits every parity boundary one packet
    /// early forever after, so half the ACKs are immediate and the delayed
    /// policy silently degrades toward the per-packet one it replaced —
    /// while every "an ACK eventually arrives" test stays green.
    ///
    /// **[ruling 271]** The debt is now **two** flags, not one, and this
    /// test is where that shows: the 2nd packet leaves the ACK `pending`
    /// rather than `owed`, so a build clearing only `owed` here leaves
    /// `is_ready()` true for ever — every packet built for any reason
    /// carries a redundant ACK, and `ACK_COALESCE_MAX` is then measured
    /// from the wrong origin. `ack.rs` states that consequence in terms
    /// (*"all three, not two"*); `!a.is_ready()` is the assertion that
    /// makes it a pin rather than a comment. The counter reset is asserted
    /// exactly as before, through its consequence, because `since_ack` is
    /// private — only the consequence's *shape* changed, from `Now` on the
    /// 2nd to `Arm(t)` on the 2nd.
    #[test]
    fn packing_an_ack_resets_the_every_second_counter_not_only_the_debt() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();

        a.on_recv(t, 1, Some(0), true, true); // 1st → Arm at +25 ms
        assert_eq!(a.on_recv(t, 2, Some(1), true, true), AckAction::Arm(t)); // 2nd
        a.on_ack_packed();
        assert!(!a.is_owed());
        assert!(
            !a.is_ready(),
            "ruling 271: packing clears `pending` too, not only `owed`"
        );

        assert_eq!(
            a.on_recv(t, 3, Some(2), true, true),
            AckAction::Arm(t + MAX_ACK_DELAY),
            "§12.4: after packing, this is the 1st unacknowledged again"
        );
    }

    /// An ACK due and not yet packed **stays** due across further arrivals.
    ///
    /// Deliberately weak, and the weakness is the point: §12.4 does not say
    /// what `on_recv` returns for a third ack-eliciting packet while the
    /// debt from the second is still outstanding, and `AckAction::Now` and
    /// `AckAction::None` are both defensible there (reported in
    /// `TESTS-5a-ack.md` as an unstated scope). What §12.4 *does* fix is
    /// that the debt does not evaporate.
    ///
    /// **[ruling 271]** The debt the second packet leaves is now `pending`
    /// rather than `owed`, so the survival is read through `is_ready()`.
    /// The unstated scope above narrowed but did not close: the ratified
    /// text says the burst *"folds into the same ACK"*, which fixes that
    /// the debt survives — this test's subject — and still leaves the
    /// returned `AckAction` unstated, so it is still not asserted here.
    ///
    /// The loop deliberately stays **well below** `ACK_COALESCE_MAX` (32).
    /// That is the coalescing window, where the debt is held; at the valve
    /// the debt is discharged by design, and a test that walked into it
    /// would be asserting the flush rather than the survival.
    ///
    /// Mutation caught: `pending` recomputed from `since_ack % 2 == 0` on
    /// each receive rather than latched. The third packet clears a debt
    /// nobody paid, and the ACK is simply never sent — which under loss is
    /// an indefinite stall, since the peer is waiting on it. Post-271 that
    /// mutation is **easier** to write and no easier to see: the coalescing
    /// branch is reached by every packet of the burst, so a `= (…)` where a
    /// `|= (…)` was meant is one character.
    #[test]
    fn an_unpacked_debt_survives_further_arrivals() {
        let t = t0();
        let mut a = AckState::new();
        a.on_recv(t, 0, None, true, true);
        a.on_ack_packed();
        a.on_recv(t, 1, Some(0), true, true);
        a.on_recv(t, 2, Some(1), true, true);
        assert!(a.is_ready(), "§12.4: the 2nd leaves an ACK due");
        assert!(!a.is_owed(), "ruling 271: due, and it builds no packet");

        for c in 3..=5u64 {
            a.on_recv(t, c, Some(c - 1), true, true);
            assert!(a.is_ready(), "counter {c}: the debt is not paid by arrival");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12.4 — the policy, through the core
// ═══════════════════════════════════════════════════════════════════════

mod policy_on_the_wire {
    use super::*;

    /// **A-1 through the core**, with the `AckDelay` timer observed
    /// directly.
    ///
    /// **[ruling 271]** This test was named
    /// `one_packet_arms_the_timer_and_the_second_emits_the_ack`, and the
    /// second half of that name stopped being true: the 2nd ack-eliciting
    /// packet **re-arms** the timer at `now` itself and emits nothing. The
    /// emission is one step further on, at the drain boundary
    /// [`drain_boundary`] supplies.
    ///
    /// Mutation caught: everything `an_ack_is_owed_after_every_second…`
    /// catches, plus the wiring — a core that computes the right
    /// `AckAction` and never arms the timer, or arms a different one.
    /// `conn.timer(TimerKind::AckDelay)` is what separates "the policy is
    /// right" from "the policy is connected", and post-271 it carries more
    /// of the test than it used to: `Some(t1 + MAX_ACK_DELAY)` after the
    /// 1st and `Some(t1)` after the 2nd are the **only** observable
    /// difference between the two arrivals, since neither transmits.
    ///
    /// Two further separations, both new and both load-bearing:
    ///
    /// * `d.transmits().is_empty()` after the **2nd** fails a build that
    ///   kept the pre-271 emission point — the plain restatement of what
    ///   ruling 271 changed.
    /// * the final `None` fails a build that emits the coalesced ACK and
    ///   leaves `AckDelay` armed. That build ACKs correctly here and then
    ///   emits a standalone ACK at every subsequent `handle_timeout` for
    ///   the life of the connection, which is the reverse-path flood
    ///   `a_repeated_timeout_at_the_same_instant_emits_no_second_ack`
    ///   describes — reachable post-271 from an *ordinary* burst rather
    ///   than only from the 25 ms timer.
    #[test]
    fn one_packet_arms_the_timer_and_the_second_makes_it_due_now() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Prime: the session's first ack-eliciting packet is immediate.
        let d = s.deliver(t, &ping());
        assert_eq!(acks(&mut s, &d).len(), 1, "§12.4: the first is immediate");
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            None,
            "§12.4: packing the ACK disarms the timer"
        );

        // 1st unacknowledged: armed at MAX_ACK_DELAY, nothing sent.
        let t1 = t + Duration::from_millis(100);
        let d = s.deliver(t1, &ping());
        assert!(
            d.transmits().is_empty(),
            "§12.4: one ack-eliciting packet is not two"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t1 + MAX_ACK_DELAY),
            "§12.4: armed at MAX_ACK_DELAY on the first unacknowledged packet"
        );

        // 2nd: still nothing sent — the deadline moves *back* to `now`.
        let d = s.deliver(t1, &ping());
        assert!(
            d.transmits().is_empty(),
            "§12.4 (ruling 271): the 2nd arms, it does not emit"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t1),
            "§12.4 (ruling 271): re-armed at `now` itself — already due"
        );

        // The drain boundary: the ACK goes out, covering both.
        let d = drain_boundary(&mut s, t1);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1, "§12.4: one ACK for the whole batch");
        assert_eq!(got[0].largest, 2, "it acknowledges the newest counter");
        assert_eq!(got[0].counters(), vec![2, 1, 0], "and everything received");
        assert_eq!(s.conn.timer(TimerKind::AckDelay), None);
    }

    /// The timer path, **both sides of 25 ms**.
    ///
    /// §16.5 and `Timers::due` fire a deadline at `D` itself, so `D` is the
    /// firing instant and `D − 1 ms` is not.
    ///
    /// Mutation caught: a build arming at a fraction of `MAX_ACK_DELAY`
    /// fires early and fails the first half; a build that never arms fires
    /// never and fails the second. This is slice 1's one-sided-boundary
    /// defect in its §12 costume: asserting only that an ACK eventually
    /// arrives at `+25 ms` is satisfied by a build that fires at `+1 ms`.
    #[test]
    fn the_delayed_ack_fires_at_twenty_five_milliseconds_and_not_before() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let d = s.deliver(t, &ping());
        assert_eq!(acks(&mut s, &d).len(), 1);

        let t1 = t + Duration::from_millis(100);
        let d = s.deliver(t1, &ping());
        assert!(d.transmits().is_empty());

        s.conn.handle_timeout(t1 + Duration::from_millis(24));
        let early = drain(&mut s.conn);
        assert!(
            early.transmits().is_empty(),
            "§12.4: MAX_ACK_DELAY is 25 ms, and 24 is not 25"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t1 + MAX_ACK_DELAY),
            "and the timer is still armed"
        );

        s.conn.handle_timeout(t1 + MAX_ACK_DELAY);
        let due = drain(&mut s.conn);
        let got = acks(&mut s, &due);
        assert_eq!(got.len(), 1, "§12.4: the AckDelay timer fires");
        assert_eq!(got[0].largest, 1);
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            None,
            "§12.4: and the debt is paid, so the timer is disarmed"
        );
    }

    /// §16.5: *"`handle_timeout` is idempotent — each due timer is stopped
    /// before its logic runs."*
    ///
    /// Mutation caught: a build that leaves `AckDelay` armed after firing.
    /// It re-owes an ACK at every subsequent `handle_timeout`, producing a
    /// standalone ACK packet per shell tick for the life of the connection
    /// — a reverse-path flood that no completion test notices.
    #[test]
    fn a_repeated_timeout_at_the_same_instant_emits_no_second_ack() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let d = s.deliver(t, &ping());
        assert_eq!(acks(&mut s, &d).len(), 1);

        let t1 = t + Duration::from_millis(100);
        s.deliver(t1, &ping());

        let due = t1 + MAX_ACK_DELAY;
        s.conn.handle_timeout(due);
        let first = drain(&mut s.conn);
        assert_eq!(acks(&mut s, &first).len(), 1);

        s.conn.handle_timeout(due);
        let again = drain(&mut s.conn);
        assert!(
            again.transmits().is_empty(),
            "§16.5: a repeated call at one instant finds an empty due set"
        );
    }

    /// **A-2 through the core.** A gap in the counter space is ACKed in the
    /// same drain, and the ACK reports the gap.
    ///
    /// Mutation caught: a build with only the "every 2nd" and timer
    /// triggers emits **nothing** here, so the peer learns of the loss
    /// 25 ms late — per gap. The second assertion (the ranges) catches the
    /// different bug of a build that reacts to the gap but reports a
    /// contiguous `0..=3`, acknowledging two packets that never arrived.
    #[test]
    fn a_gap_in_the_counter_space_is_acked_in_the_same_drain() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &ping());
        assert_eq!(acks(&mut s, &d).len(), 1, "counter 0, immediate");

        // Counters 1 and 2 are sealed and discarded: the core never sees
        // them, so counter 3 opens a gap.
        let dgram = seal_at(&mut s.peer, 3, &ping());
        let d = feed(&mut s, t, &dgram);

        let got = acks(&mut s, &d);
        assert_eq!(
            got.len(),
            1,
            "§12.4: out-of-order arrival is ACKed immediately, as the 1st"
        );
        assert_eq!(got[0].largest, 3);
        assert_eq!(
            got[0].blocks(),
            vec![3..=3, 0..=0],
            "§12.1: the gap at 1..=2 is reported as a gap"
        );
    }

    /// **A-9.** A replayed datagram advances nothing.
    ///
    /// §7.2: *"No replayed packet ever moves the endpoint or refreshes
    /// liveness"*, and §2.1 scopes `on_recv` to window-fresh packets for
    /// exactly this reason.
    ///
    /// The replay is delivered where a **second** ack-eliciting packet
    /// would tip the "every 2nd" rule, so a build that folds replays in has
    /// no way to hide.
    ///
    /// **[ruling 271]** The way it used to have no way to hide was that it
    /// *"emits an ACK the correct build does not"*, and that observable is
    /// gone: post-271 the 2nd packet emits nothing either, so a replay
    /// folded into the counter is invisible on the wire. What replaces it
    /// is **the armed deadline** — `t + MAX_ACK_DELAY` if the replay was
    /// correctly dropped, `t` if it was counted — and that assertion was
    /// already in this test, one line below the one that used to do the
    /// work. It is now the separator, and the emission it used to be is
    /// asserted at the drain boundary instead.
    ///
    /// This is exactly the amplification the pre-271 rationale in
    /// `fold_ack_policy` warns about, and ruling 271 records that
    /// coalescing makes it *cheaper*: a burst of genuine packets now buys
    /// one ACK, so a replay that advanced the counter would be worth
    /// proportionally more.
    ///
    /// Mutation caught: `on_recv` called before the window's
    /// check-and-mark, or on its `false` branch. An attacker replaying one
    /// captured datagram then gets one ACK per copy — free reverse-path
    /// amplification off a packet it cannot even read — and every other
    /// §12 test passes.
    #[test]
    fn a_replayed_packet_does_not_advance_the_every_second_counter() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &ping());
        assert_eq!(acks(&mut s, &d).len(), 1, "counter 0, immediate");

        let dgram = s.peer.seal(&ping());
        let d = feed(&mut s, t, &dgram);
        assert!(d.transmits().is_empty(), "counter 1 is the 1st: armed only");
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t + MAX_ACK_DELAY),
            "…and the 1st arms at MAX_ACK_DELAY, not at `now`"
        );

        let d = feed(&mut s, t, &dgram);
        assert!(
            d.transmits().is_empty(),
            "§7.2: the replay is dropped after decryption, without delivery, \
             so it is not the 2nd anything"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t + MAX_ACK_DELAY),
            "and the armed deadline is untouched — a build that counted the \
             replay would have moved it to `now`"
        );
        assert!(
            drain_boundary(&mut s, t).transmits().is_empty(),
            "…so the drain boundary has nothing to flush: the replay bought \
             the attacker no ACK at all"
        );

        // A genuinely fresh second packet still works.
        let d = s.deliver(t, &ping());
        assert!(
            d.transmits().is_empty(),
            "the 2nd arms rather than emitting"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t),
            "§12.4: the 2nd *fresh* ack-eliciting packet makes the ACK due"
        );
        let d = drain_boundary(&mut s, t);
        assert_eq!(
            acks(&mut s, &d).len(),
            1,
            "…and one ACK leaves at the drain boundary"
        );
    }

    /// **A-7.** A pure-ACK packet carries only an ACK, is sealed
    /// `seal_quiet` (§7.4), is not ack-eliciting, and is not tracked for
    /// loss.
    ///
    /// All four properties §12.4's last bullet states, each with its own
    /// observable:
    ///
    /// | property | observable | broken build |
    /// |---|---|---|
    /// | only an ACK | the decoded frame list | packs a PING to "make it useful" |
    /// | `seal_quiet` | `liveness.last_send()` unchanged | `seal` moves the §7.5 marking clock, and the connection stops sending keepalives it owes |
    /// | not ack-eliciting | `liveness.is_armed()` still false | arming re-enables §7.4's death deadline off a packet that carries no obligation |
    /// | never tracked | `bytes_in_flight()` still 0 | the window fills with ACKs and the sender gates itself to a halt |
    ///
    /// **The delivery is at `t + 5 ms`, not at `t`.** At `t` the
    /// `last_send` assertion would pass against a build using plain `seal`,
    /// because the install pinned `last_send` to `t` already — the
    /// degenerate build would satisfy it for free, which is working rule 9
    /// exactly.
    #[test]
    fn a_pure_ack_packet_is_quiet_unelicited_and_untracked() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let installed_last_send = s.conn.liveness().expect("installed").last_send();
        assert_eq!(installed_last_send, t);

        let later = t + Duration::from_millis(5);
        let d = s.deliver(later, &ping());

        let decoded = packets(&mut s, &d);
        assert_eq!(decoded.len(), 1, "one packet out");
        let (ack, rest) = &decoded[0];
        assert!(ack.is_some(), "§12.4: a standalone ACK packet");
        assert!(
            rest.is_empty(),
            "§12.4: a pure ACK carries nothing else — got {rest:?}"
        );

        let live = s.conn.liveness().expect("still installed");
        assert_eq!(
            live.last_send(),
            installed_last_send,
            "§12.4/§7.4: pure ACKs are sealed `seal_quiet`, so the marking \
             clock does not move"
        );
        assert!(
            !live.is_armed(),
            "§12.4: a pure ACK is not ack-eliciting, so it does not arm \
             §7.4's death deadline"
        );
        assert_eq!(
            s.conn.bytes_in_flight(),
            0,
            "§12.4: pure ACKs are never tracked for loss"
        );
    }

    /// The same, sustained: a hundred pure ACKs leave nothing in flight.
    ///
    /// Mutation caught: a build that inserts pure ACKs into the sent map.
    /// One such packet is ~30 bytes and easy to overlook; a hundred is
    /// 3 KB of a 12 000-byte initial window, and the failure mode §12.4's
    /// bullet exists to prevent — the sender gating itself shut — only
    /// appears after minutes of real traffic. Here it appears at once.
    #[test]
    fn a_hundred_pure_acks_leave_nothing_in_flight() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        for _ in 0..100 {
            s.deliver(t, &ping());
        }
        assert_eq!(
            s.conn.bytes_in_flight(),
            0,
            "§12.4: never tracked for loss, and they bypass the window"
        );
    }

    /// §12.4: *"An owed ACK rides the next outgoing packet (packing order
    /// §8.5); if none is pending, a standalone ACK packet is generated."*
    ///
    /// The "if none is pending" branch is what every other test in this
    /// module exercises. This one exercises the **first** clause, and it is
    /// the only §12 test that needs §14: under ruling 134 the congestion
    /// window is the sole reason data stays pending across a pump, so a
    /// packet the ACK can ride only exists once the window defers a seal.
    /// (Recorded in `TESTS-5a-ack.md`: §12.4's riding rule is unobservable
    /// without §14's admission gate, which is PLAN-5 §1's "one feedback
    /// loop through one call site" seen from the test side.)
    ///
    /// The stimulus is one packet carrying `ACK ‖ PING`: the ACK frees the
    /// window so data can seal, and the PING makes an ACK owed in the same
    /// application pass.
    ///
    /// Mutation caught: a build that always generates a standalone pure-ACK
    /// packet. It is *correct* on the wire and costs a whole datagram per
    /// ACK on a saturated path — 30 bytes of header and tag per ~1 200
    /// bytes of data, plus a packet the peer must open. Asserting only "an
    /// ACK was sent" passes it.
    #[test]
    fn an_owed_ack_rides_a_pending_data_packet_rather_than_going_alone() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let r: StreamRef = s.conn.open(Dir::Uni).expect("the first uni stream fits");

        // More than the initial congestion window, well under the initial
        // stream credit (262 144), so §14's gate is what leaves data
        // pending and §10's ledger is not. Checked at compile time: if
        // either constant moves, this test stops testing what it says.
        const BYTES: usize = 20_000;
        const _: () = assert!(BYTES as u64 > INITIAL_WINDOW);
        const _: () = assert!((BYTES as u64) < crate::constants::INITIAL_MAX_STREAM_DATA);

        write_all(&mut s.conn, t, r, &ramp(0, BYTES));
        let d = drain(&mut s.conn);

        let sent: Vec<u64> = d.transmits().iter().map(|x| counter_of(&x.data)).collect();
        assert!(!sent.is_empty(), "some data must have gone out");
        let flight = s.conn.bytes_in_flight();
        assert!(
            flight > 0 && flight <= INITIAL_WINDOW,
            "§14.5: the initial window bounds the first flight, got {flight}"
        );

        // Acknowledge the whole first flight and elicit an ACK, in one
        // packet.
        let largest = *sent.last().expect("non-empty");
        let mut pt = ack_frame(largest, 0, largest - sent[0], &[]);
        pt.extend_from_slice(&ping());
        let d = s.deliver(t, &pt);

        let decoded = packets(&mut s, &d);
        let carrying: Vec<&Packet> = decoded.iter().filter(|(a, _)| a.is_some()).collect();
        assert_eq!(
            carrying.len(),
            1,
            "§12.4: one owed ACK, so exactly one ACK on the wire"
        );
        assert!(
            carrying[0]
                .1
                .iter()
                .any(|f| matches!(f, Wire::Stream { .. })),
            "§12.4: the owed ACK rides the next outgoing packet — it did \
             not, and went alone: {decoded:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12.5 — processing a received ACK
// ═══════════════════════════════════════════════════════════════════════

mod processing {
    use super::*;

    /// Write `bytes` on a fresh uni stream and return the counters the core
    /// sealed for it, with the drain that produced them.
    fn write_and_collect(s: &mut Solo, now: Instant, bytes: usize) -> (StreamRef, Vec<u64>) {
        let r: StreamRef = s.conn.open(Dir::Uni).expect("the first uni stream fits");
        write_all(&mut s.conn, now, r, &ramp(0, bytes));
        let d = drain(&mut s.conn);
        let counters = d
            .transmits()
            .iter()
            .map(|x| counter_of(&x.data))
            .collect::<Vec<_>>();
        assert!(!counters.is_empty(), "the write must have sealed something");
        (r, counters)
    }

    /// **A-6.** §12.5: an ACK whose `largest` exceeds the highest counter
    /// this session has sealed is *"ignored whole — the frame applies as a
    /// no-op with a trace; the packet's other frames still apply"*.
    ///
    /// The forged ACK's ranges deliberately **cover the real in-flight
    /// counters** (`0 ..= 1 000 000`). That is what makes the test separate
    /// the two wrong builds:
    ///
    /// - a build treating it as RFC 9000 §13.1's `PROTOCOL_VIOLATION` kills
    ///   the connection — `assert_alive` fails;
    /// - a build that applies it acknowledges the entire flight —
    ///   `bytes_in_flight` drops to 0 instead of staying put;
    /// - a build that discards the whole *packet* loses the STREAM frame —
    ///   no `StreamOpened`, nothing readable.
    ///
    /// A forged ACK with a range covering nothing in flight (the obvious
    /// `first_range = 0` version) would separate none of them, because an
    /// applying build would also have nothing to acknowledge.
    #[test]
    fn an_ack_above_the_highest_sealed_counter_is_ignored_whole() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let (_r, _sent) = write_and_collect(&mut s, t, 4096);
        let flight_before = s.conn.bytes_in_flight();
        assert!(flight_before > 0);

        let highest = s.conn.next_counter().expect("installed") - 1;
        let forged = 1_000_000;
        assert!(forged > highest, "the point of the test");

        let mut pt = ack_frame(forged, 0, forged, &[]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"hello", true));
        let d = s.deliver(t, &pt);

        assert_alive(&d);
        assert_eq!(
            s.conn.bytes_in_flight(),
            flight_before,
            "§12.5: ignored whole — the frame applies as a no-op"
        );
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            1,
            "§12.5: the packet's other frames still apply (§8.2's split)"
        );

        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), 1);
        let (got, eos) = read_available(&mut s.conn, t, claimed[0].1);
        assert_eq!(got, b"hello".to_vec());
        assert!(eos, "the FIN applied too");
    }

    /// The boundary of the same rule, **both sides**: `largest` exactly
    /// equal to the highest sealed counter is processed; one above is not.
    ///
    /// §12.5 says "**exceeds**", so equality is inside. This is slice 1's
    /// one-sided-boundary defect again: a build written with `>=` passes
    /// every ignore-whole test and silently discards the ACK for the newest
    /// packet in flight, forever — which is the packet the RTT sample and
    /// §13.2's `largest_acked` both depend on.
    ///
    /// Mutation caught: `ack.largest >= highest_sealed ⇒ ignore`.
    #[test]
    fn largest_equal_to_the_highest_sealed_counter_is_processed() {
        let t = t0();

        // Equal: processed.
        let mut s = Solo::installed_at(t);
        let (_r, sent) = write_and_collect(&mut s, t, 4096);
        let highest = s.conn.next_counter().expect("installed") - 1;
        assert_eq!(
            highest,
            *sent.last().expect("non-empty"),
            "the last data packet is the highest sealed"
        );
        assert!(s.conn.bytes_in_flight() > 0);
        s.deliver(t, &ack_frame(highest, 0, highest - sent[0], &[]));
        assert_eq!(
            s.conn.bytes_in_flight(),
            0,
            "§12.5: `exceeds` excludes equality, so this ACK applies"
        );

        // One above: ignored whole.
        let mut s = Solo::installed_at(t);
        let (_r, sent) = write_and_collect(&mut s, t, 4096);
        let highest = s.conn.next_counter().expect("installed") - 1;
        let flight = s.conn.bytes_in_flight();
        let d = s.deliver(t, &ack_frame(highest + 1, 0, highest + 1 - sent[0], &[]));
        assert_alive(&d);
        assert_eq!(
            s.conn.bytes_in_flight(),
            flight,
            "§12.5: one above the highest sealed is ignored whole"
        );
    }

    /// §12.5: *"Duplicate acknowledgment of a counter is a no-op."*
    ///
    /// Mutation caught: a build subtracting `size` from `bytes_in_flight`
    /// per acknowledged counter rather than per removed entry. The second
    /// ACK underflows a `u64` — a debug panic, or in release a
    /// `bytes_in_flight` near `u64::MAX` that closes the congestion window
    /// permanently. Duplicate ACKs are not exotic: §12.2's ranges
    /// re-acknowledge everything still in the window on every ACK, so the
    /// *normal* case is that most of an ACK is duplicate.
    #[test]
    fn a_duplicate_acknowledgment_changes_nothing() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let (_r, sent) = write_and_collect(&mut s, t, 4096);
        assert!(s.conn.bytes_in_flight() > 0);

        let largest = *sent.last().expect("non-empty");
        let pt = ack_frame(largest, 0, largest - sent[0], &[]);

        s.deliver(t, &pt);
        assert_eq!(s.conn.bytes_in_flight(), 0, "the flight is acknowledged");

        for round in 0..3 {
            let d = s.deliver(t, &pt);
            assert_alive(&d);
            assert_eq!(
                s.conn.bytes_in_flight(),
                0,
                "§12.5: duplicate acknowledgment is a no-op (round {round})"
            );
        }
    }

    /// **A-5, both sides.** `range_count = MAX_ACK_RANGES` is fine;
    /// `MAX_ACK_RANGES + 1` is §8.2's structural class — *"nothing from the
    /// packet is applied"* — and CLOSEs with `PROTOCOL_VIOLATION`.
    ///
    /// The two packets differ in **exactly one pair**, and both carry the
    /// same STREAM frame, so the STREAM's fate is attributable to the count
    /// and to nothing else. `largest` is identical in both (and above the
    /// highest sealed, so the ACK itself is a §12.5 no-op either way) —
    /// which is deliberate: the variable under test is the count.
    ///
    /// Mutation caught: `>=` in the count check rejects 64 and kills a
    /// conforming peer's connection; a missing check accepts 65 and, worse,
    /// applies the packet — §8.2's "nothing is applied" is the half a
    /// parse-then-continue build gets wrong even when it does reject.
    #[test]
    fn sixty_four_ranges_apply_and_sixty_five_take_the_whole_packet_down() {
        let t = t0();
        let pairs = vec![(0u64, 0u64); MAX_ACK_RANGES + 1];

        // 64: legal.
        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(1_000_000, 0, 0, &pairs[..MAX_ACK_RANGES]);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"sixty-four", false));
        let d = s.deliver(t, &pt);
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            1,
            "§12.2: 64 pairs is the cap, not one past it"
        );

        // 65: structural.
        let mut s = Solo::installed_at(t);
        let mut pt = ack_frame(1_000_000, 0, 0, &pairs);
        pt.extend_from_slice(&stream_frame(Solo::peer_uni(0), 0, b"sixty-five", false));
        let d = s.deliver(t, &pt);
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, PROTOCOL_VIOLATION);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "§8.2: nothing from the packet is applied — no ACK scheduling, \
             no state"
        );
    }

    /// **A-8, as far as the ignore-whole rule permits.** A wire-legal ACK
    /// carrying the full 64 pairs, whose blocks name counters the session
    /// really sealed, acknowledges exactly the in-flight set and nothing
    /// else.
    ///
    /// The session's counter space is pushed past 130 first — 400 delivered
    /// PINGs provoke ~200 pure ACKs, one sealed counter each, and they
    /// bypass the congestion window — because 64 pairs need at least 129
    /// counters of descent and §12.5's ignore-whole rule caps `largest` at
    /// the highest counter *sealed*. Those 64 pairs
    /// therefore name pure-ACK counters, which §12.4 says are never
    /// tracked: acknowledging them must be a no-op, while the first block
    /// retires the real flight.
    ///
    /// Mutation caught: a build that intersects the ACK against the sent
    /// map by *iterating the ACK's counters* rather than the map's entries
    /// still gets the right answer here — see `TESTS-5a-ack.md`, where
    /// PLAN-5's A-8 is reported as asserting a property that does not
    /// separate the builds it names. What this **does** catch is a build
    /// whose per-block walk mis-associates ranges with entries: the
    /// `bytes_in_flight == 0` assertion requires every one of the flight's
    /// counters to be found under the *first* block while 64 further blocks
    /// march past unmatched entries, which is where an off-by-one in a
    /// merge walk lands.
    #[test]
    fn a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Burn counters cheaply: each ack-eliciting packet the peer sends
        // is answered by a pure ACK, which is one sealed counter and is
        // never tracked.
        //
        // **[ruling 271]** The `drain_boundary` call is what keeps this
        // *cheap*, and it is a fixture repair rather than a change of
        // subject: this test's property is §12.5's per-block walk, which
        // has nothing to do with the ACK cadence, but the counters it
        // needs were being bought at the cadence's exchange rate. Under
        // coalescing, 400 undrained pings buy **13** counters — the first
        // immediate ACK plus one per `ACK_COALESCE_MAX` — and the
        // precondition below fails on the fixture, not on the behaviour.
        // Firing the already-due deadline after each ping is the driver's
        // own drain boundary and restores one ACK per two pings.
        for _ in 0..400 {
            s.deliver(t, &ping());
            drain_boundary(&mut s, t);
        }
        let floor = s.conn.next_counter().expect("installed");
        assert!(
            floor > 2 * MAX_ACK_RANGES as u64 + 2,
            "the counter space must be deep enough for 64 descending pairs, \
             got {floor}"
        );
        assert_eq!(s.conn.bytes_in_flight(), 0, "pure ACKs are not tracked");

        let (_r, sent) = write_and_collect(&mut s, t, 4096);
        let flight = s.conn.bytes_in_flight();
        assert!(flight > 0);

        // First block: the whole flight. Then 64 pairs marching down
        // through the pure-ACK counters, two counters each.
        let largest = *sent.last().expect("non-empty");
        let first_range = largest - sent[0];
        let pairs = vec![(0u64, 0u64); MAX_ACK_RANGES];
        let pt = ack_frame(largest, 0, first_range, &pairs);

        let d = s.deliver(t, &pt);
        assert_alive(&d);
        assert_eq!(
            s.conn.bytes_in_flight(),
            0,
            "§12.5: the first block retires the flight"
        );
    }

    /// §12.5's intersection is against the **in-flight set**: an ACK naming
    /// only counters that were never tracked changes nothing.
    ///
    /// Mutation caught: a build that removes map entries by *range
    /// position* rather than by key — for instance popping the map's first
    /// `n` entries where `n` is the ACK's block count. It retires real
    /// packets on an ACK that names none of them, and the sender then never
    /// retransmits data the peer never got.
    #[test]
    fn an_ack_naming_only_untracked_counters_retires_nothing() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Pure-ACK counters 0..=9, never tracked.
        for _ in 0..10 {
            s.deliver(t, &ping());
        }
        let untracked_top = s.conn.next_counter().expect("installed") - 1;

        let (_r, _sent) = write_and_collect(&mut s, t, 4096);
        let flight = s.conn.bytes_in_flight();
        assert!(flight > 0);

        let d = s.deliver(t, &ack_frame(untracked_top, 0, untracked_top, &[]));
        assert_alive(&d);
        assert_eq!(
            s.conn.bytes_in_flight(),
            flight,
            "§12.5: intersected with the in-flight set, which holds none of \
             these counters"
        );
    }

    /// An ACK is not ack-eliciting (§8.3), so receiving one owes no ACK
    /// back — §12.4's *"no ACK-of-ACK loops"*, from the receiving side.
    ///
    /// Mutation caught: `is_ack_eliciting` consulted per *packet* as "the
    /// packet carried frames" rather than per §8.3's table. Two peers each
    /// answering the other's ACK is a loop that saturates a link and
    /// terminates only when one of them dies; it is invisible to any test
    /// that asserts an ACK *arrives*.
    #[test]
    fn a_received_ack_elicits_no_ack_in_reply() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let (_r, sent) = write_and_collect(&mut s, t, 4096);
        let largest = *sent.last().expect("non-empty");

        for round in 0..4 {
            let d = s.deliver(t, &ack_frame(largest, 0, largest - sent[0], &[]));
            assert!(
                acks(&mut s, &d).is_empty(),
                "§8.3/§12.4: an ACK is not ack-eliciting (round {round})"
            );
            assert_eq!(
                s.conn.timer(TimerKind::AckDelay),
                None,
                "and it arms nothing (round {round})"
            );
        }
    }

    /// A packet carrying only PADDING elicits no ACK either — §8.3 marks
    /// PADDING not ack-eliciting, and it is the only frame that is
    /// *frame-seen* but not eliciting.
    ///
    /// Mutation caught: `ack_eliciting` derived from `!plaintext.is_empty()`
    /// — i.e. "it was not a keepalive, so it counts". The distinction
    /// matters because §12.3 already keys `frame_seen` off emptiness, so
    /// the two flags are easy to conflate into one. A build that conflates
    /// them ACKs padding, and §8.4 lets a peer send any number of PADDING
    /// bytes anywhere.
    ///
    /// **[ruling 271]** The middle assertion had to be **strengthened**, not
    /// merely translated. It read *"this is the 1st and not the 2nd"* off
    /// `d.transmits().is_empty()` — and post-271 the 2nd transmits nothing
    /// either, so that observable stopped separating the two and the test
    /// would have passed a build that counted the PADDING packet. The
    /// deadline separates them: the 1st arms at `t + MAX_ACK_DELAY`, the
    /// 2nd re-arms at `t`. A build that let PADDING advance `since_ack`
    /// puts a due deadline where a 25 ms one belongs, and *nothing on the
    /// wire says so* until the drain boundary.
    #[test]
    fn a_padding_only_packet_elicits_no_ack() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &padding(8));
        assert!(
            d.transmits().is_empty(),
            "§8.3: PADDING is not ack-eliciting, first packet or not"
        );
        assert_eq!(s.conn.timer(TimerKind::AckDelay), None);

        // The next packet is ack-eliciting and *in order* (counter 1
        // follows the greatest 0 the PADDING packet installed), so it is
        // only the 1st: proof that the PADDING packet did not advance
        // §12.4's counter either.
        let d = s.deliver(t, &ping());
        assert!(d.transmits().is_empty(), "§8.3: nothing is owed yet");
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t + MAX_ACK_DELAY),
            "§2.1: a non-eliciting packet does not advance `since_ack`, so \
             this is the 1st — armed at MAX_ACK_DELAY, not due at `now`"
        );

        // …and the ACK the 2nd provokes covers the PADDING packet's
        // counter all the same.
        s.deliver(t, &ping());
        let d = drain_boundary(&mut s, t);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].counters(),
            vec![2, 1, 0],
            "§12.2: the window records every authenticated fresh counter, \
             ack-eliciting or not"
        );
    }

    /// A keepalive (§3.4's empty plaintext) elicits no ACK, and its counter
    /// is still acknowledged by the next ACK.
    ///
    /// Mutation caught: a build treating the empty plaintext as zero frames
    /// and therefore as an ordinary non-eliciting packet would pass the
    /// first half — but §12.3's `frame_seen` flag has to come from
    /// somewhere, and the second half (the counter appearing in the ACK)
    /// fails a build that skips the window mark for keepalives entirely,
    /// which is the shortcut that makes the peer retransmit them forever.
    ///
    /// **[ruling 271]** Same strengthening as
    /// [`a_padding_only_packet_elicits_no_ack`]: *"counter 1 is only the
    /// 1st"* was read off an empty transmit list, which the 2nd now
    /// satisfies too. The armed deadline is what still tells the two
    /// apart — `t + MAX_ACK_DELAY` for the 1st, `t` for the 2nd — and the
    /// ACK itself is collected at the drain boundary.
    #[test]
    fn a_keepalive_elicits_no_ack_but_is_still_acknowledged() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &[]);
        assert!(
            d.transmits().is_empty(),
            "§3.4/§12.4: a keepalive carries no ack-eliciting frame"
        );
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            None,
            "§12.4: a keepalive arms no timer either"
        );

        // Counter 1 is in order behind the keepalive's 0, so it is the 1st
        // and arms at MAX_ACK_DELAY; the 2nd makes the ACK due.
        let d = s.deliver(t, &ping());
        assert!(d.transmits().is_empty());
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t + MAX_ACK_DELAY),
            "§12.4: the keepalive did not advance `since_ack`, so this is \
             the 1st"
        );

        s.deliver(t, &ping());
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t),
            "§12.4 (ruling 271): …and this is the 2nd"
        );
        let d = drain_boundary(&mut s, t);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].counters(),
            vec![2, 1, 0],
            "§7.2: the keepalive's counter was authenticated and marked"
        );
    }

    /// §12.3 through the core: an ACK provoked by a keepalive-topped window
    /// reports `ack_delay = 0`, and one provoked by a frame-bearing
    /// greatest does not.
    ///
    /// Both halves, same instants. Mutation caught: a core that passes
    /// `frame_seen = true` unconditionally into `on_recv` — the flag is
    /// derived from `Received::Keepalive` versus `Received::Frames`, and a
    /// core that folds the two before calling `on_recv` loses it silently.
    /// The unit tests in `ack_delay` cannot see this: they call `on_recv`
    /// with the flag the test chose.
    ///
    /// **[ruling 271]** Only the **first** stanza moved, and only in how the
    /// ACK is collected: the 2nd ack-eliciting packet arms `AckDelay` at
    /// its own `now` instead of emitting, so the ACK is taken from the
    /// drain boundary at that same instant. The instant is what the stanza
    /// is about — `ack_delay = 0` because emission and the largest's
    /// arrival coincide — and it is unchanged, which is why the assertion
    /// is unchanged. The two `MAX_ACK_DELAY` stanzas below were green
    /// through ruling 271 and are untouched: §12.4's *first* unacknowledged
    /// packet still arms at 25 ms, and a keepalive still arms nothing.
    #[test]
    fn the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest() {
        let t = t0();
        let gap = Duration::from_millis(7);

        // Frame-bearing greatest: the delay is real. The ACK is provoked by
        // a *second* ack-eliciting packet so that the emission instant is
        // `gap` after the largest's arrival.
        let mut s = Solo::installed_at(t);
        s.deliver(t, &ping()); // counter 0, immediate ACK
        s.deliver(t, &ping()); // counter 1, armed at t + 25 ms
        s.deliver(t + gap, &ping()); // counter 2, the 2nd → due at t + gap
        assert_eq!(
            s.conn.timer(TimerKind::AckDelay),
            Some(t + gap),
            "§12.4 (ruling 271): the 2nd re-arms at this packet's `now`, \
             which is earlier than the 1st's `t + 25 ms`"
        );
        let d = drain_boundary(&mut s, t + gap);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].largest, 2);
        assert_eq!(
            got[0].ack_delay, 0,
            "§12.3: measured from the arrival of the packet bearing \
             `largest`, which is this very packet"
        );

        // Now a delay that is genuinely non-zero: the largest arrives, and
        // the ACK is emitted `gap` later off the AckDelay timer.
        let mut s = Solo::installed_at(t);
        s.deliver(t, &ping()); // counter 0, immediate ACK
        s.deliver(t, &ping()); // counter 1, armed at t + 25 ms
        s.conn.handle_timeout(t + MAX_ACK_DELAY);
        let d = drain(&mut s.conn);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].ack_delay,
            u64::try_from(MAX_ACK_DELAY.as_micros()).expect("25 ms fits"),
            "§12.3: the full delayed-ACK interval, in raw µs"
        );

        // And the keepalive case: the greatest was not frame-seen.
        let mut s = Solo::installed_at(t);
        s.deliver(t, &ping()); // counter 0, immediate ACK
        s.deliver(t, &ping()); // counter 1, armed at t + 25 ms
        s.deliver(t, &[]); // counter 2, a keepalive: now the greatest
        s.conn.handle_timeout(t + MAX_ACK_DELAY);
        let d = drain(&mut s.conn);
        let got = acks(&mut s, &d);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].largest, 2, "the keepalive's counter is the largest");
        assert_eq!(
            got[0].ack_delay, 0,
            "§12.3: the window's largest was not frame-seen"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Two cores, end to end
// ═══════════════════════════════════════════════════════════════════════

mod two_cores {
    use super::*;

    /// §12 closed loop: A's data is acknowledged by B, and A's flight
    /// empties. This is the smallest statement that both halves of §12 are
    /// connected to each other rather than merely correct in isolation.
    ///
    /// Mutation caught: a core that derives ACKs correctly and never
    /// *processes* one (or the reverse). Each of `bytes_in_flight`'s two
    /// assertions fails a different half — the pre-condition fails a build
    /// that tracks nothing, the post-condition a build that never retires.
    /// `Pair::pump` runs until the wire is quiet, so a build that answers
    /// every ACK with an ACK never terminates and fails as a named panic.
    ///
    /// The **second** pump, at `t + MAX_ACK_DELAY`, is not decoration: the
    /// flight is an odd or even number of packets depending on how §8.6
    /// packs 4 KiB, and if it is odd the last packet is B's 1st
    /// unacknowledged and is owed only when `AckDelay` fires. Asserting
    /// `bytes_in_flight == 0` after a single same-instant pump would be a
    /// test that passes or fails on the frame-header size — which is why
    /// this test does **not** assert how many ACKs B sent.
    #[test]
    fn two_cores_acknowledge_each_others_data_and_the_flight_empties() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        let r = p.a.open(Dir::Uni).expect("the first uni stream fits");
        write_all(&mut p.a, t, r, &ramp(0, 4096));
        let _ = p.drain_a();
        assert!(
            p.a.bytes_in_flight() > 0,
            "§13.5/§14.5: the data is tracked while in flight"
        );

        let _ = p.pump(t);
        let _ = p.pump(t + MAX_ACK_DELAY);
        assert_eq!(
            p.a.bytes_in_flight(),
            0,
            "§12.5: B's ACKs retired A's whole flight"
        );
        assert_eq!(
            p.b.bytes_in_flight(),
            0,
            "and B's own ACKs were never tracked (§12.4)"
        );
    }
}

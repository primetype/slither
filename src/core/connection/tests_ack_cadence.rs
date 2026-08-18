//! Ruling 271's ACK cadence, pinned by a blind author.
//!
//! Written from `SPEC.md` §12.4/§12.1/§12.2, §13.1/§13.2/§13.3, §7.4/§7.5's
//! liveness accounting and ruling 33, the fixtures in
//! [`testfix`](super::testfix), and round 42's committed measurements
//! (`.spec-v2-clean-slate/round42-G-datapath-attribution.md` "ACK ratio",
//! `round42-H-residual-profile.md` "The true ACK-lever value"). The author
//! never read `src/shell/driver.rs`, `ack.rs`, `recv.rs` or `send.rs`, and
//! wrote in parallel with the implementer (working rule 6).
//!
//! # What is being changed, and what is not
//!
//! §12.4's **first** bullet — *"An ACK is owed after every **2nd**
//! ack-eliciting packet"* — becomes per-drain coalescing: **a burst of N
//! ack-eliciting datagrams processed in one receive-drain produces at most
//! one ACK emission.** Everything else in §12.4 is untouched:
//!
//! * the immediate ACK on out-of-order arrival (bullet two);
//! * `MAX_ACK_DELAY` = 25 ms as the **outer bound** — an ack-eliciting
//!   packet is acknowledged within it even when no further drain ever
//!   happens (the idle-after-burst tail);
//! * ruling 33's liveness-neutrality of ACKs, and §7.4's `seal_quiet`;
//! * every wire format, `MAX_ACK_RANGES`, and §12.1's range arithmetic.
//!
//! # The recovery half is the point
//!
//! Our ACKs are the peer's only feedback: §13.1 takes RTT samples from
//! them, §13.2 declares loss from them, §14.2 opens the window from them.
//! A cadence fix that starves them buys throughput by blinding the sender,
//! and every such fix must go **red** in this file. Five of the tests
//! below exist only for that:
//! [`a_lone_ack_eliciting_packet_is_acked_within_max_ack_delay`],
//! [`the_tail_of_an_idle_burst_is_acked_within_max_ack_delay`],
//! [`a_coalesced_ack_reports_every_range_a_per_packet_ack_would`],
//! [`a_single_lost_packet_is_recovered_without_advancing_the_clock`] and
//! [`the_estimator_still_samples_across_a_coalesced_exchange`].
//!
//! # What "one drain" is read as, at the core seam
//!
//! §16.4 is explicit that *"**every mutating call** (`handle_datagram`, …)
//! **is followed by draining `poll_output()` to the terminal
//! `Timeout(Option<Instant>)`**"*, and the fixtures obey it: `Solo::deliver`
//! (`testfix.rs:1170`) and `Pair::flush_a_to_b_from` (`testfix.rs:701`)
//! both drain **after each datagram**. So "one drain over N datagrams" is
//! not a state the driver contract permits and not a state any fixture
//! helper expresses. This file therefore reads ruling 271's unit as **one
//! receive batch**: the N datagrams a driver turn takes off the socket, all
//! bearing the same `now` (§16.5's once-per-turn instant), each drained per
//! §16.4. That reading is red at base, honours §16.4, and reproduces the
//! shape round 42-G measured on the real socket.
//!
//! The **literal** reading — N `handle_datagram` calls before a single
//! `poll_output` loop — is written too, as
//! [`twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack`],
//! and reported in `AUTHOR-ACK-REPORT.md` §C1 as an unresolved conflict.
//! The author does **not** pick between them (working rule 3).

use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

use super::stream_id::Dir;
use super::streams::StreamRef;
use super::testfix::*;
use crate::constants::{FRAME_PING, K_INITIAL_RTT, MAX_ACK_DELAY};

/// The reference suite, named locally: `testfix`'s own alias is private to
/// that module and this file must not reach into it.
type Suite = crate::packet::ReferenceSuite;

/// The burst size. Twenty is the brief's number and is comfortably above
/// §12.4's every-2nd trigger, so a base core emits ten ACKs where the
/// ratified cadence emits one — a factor of ten, not a boundary.
const BURST: usize = 20;

// ═══════════════════════════════════════════════════════════════════════
// §12.1, decoded in this file's own vocabulary
// ═══════════════════════════════════════════════════════════════════════

/// §8.4's ACK as its wire fields.
///
/// Deliberately reconstructed from [`Wire::Ack`] rather than read back
/// through `frame::Ack`: a derivation asked whether it agrees with itself
/// always says yes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AckFields {
    largest: u64,
    first_range: u64,
    ranges: Vec<(u64, u64)>,
}

impl AckFields {
    /// §12.1's range semantics, transcribed from the spec text:
    ///
    /// > the first block covers `largest − first_range ..= largest`; for
    /// > each subsequent `(gap, range)` pair, with `prev_smallest` the
    /// > smallest counter of the preceding block: the block's largest is
    /// > `prev_smallest − gap − 2`, and the block covers
    /// > `block_largest − range ..= block_largest`.
    ///
    /// Checked subtraction throughout — §12.1 makes a block descending
    /// below counter zero a structural failure, so an underflow here is a
    /// bug in this file and must panic rather than saturate into a
    /// plausible-looking range.
    fn blocks(&self) -> Vec<RangeInclusive<u64>> {
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

    /// Whether this ACK acknowledges `counter`.
    fn covers(&self, counter: u64) -> bool {
        self.blocks().iter().any(|b| b.contains(&counter))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Counting ACK emissions
// ═══════════════════════════════════════════════════════════════════════

/// Every ACK frame a drain produced, in generation order.
fn acks(s: &mut Solo, d: &Drained) -> Vec<AckFields> {
    s.packets(d)
        .into_iter()
        .flatten()
        .filter_map(|f| match f {
            Wire::Ack {
                largest,
                ranges,
                first_range,
                ..
            } => Some(AckFields {
                largest,
                first_range,
                ranges,
            }),
            _ => None,
        })
        .collect()
}

/// How many **packets** in a drain carried an ACK.
///
/// This, and not the ACK-frame count, is what ruling 271 bounds: §12.4
/// lets an owed ACK ride an outgoing data packet, so a build that
/// piggybacks perfectly has emitted nothing extra even though an ACK frame
/// went out. Counting frames would make a correct piggyback look like a
/// cadence violation; counting *standalone* ACK packets only would let a
/// build hide ten ACKs inside ten credit packets.
fn ack_bearing_packets(s: &mut Solo, d: &Drained) -> usize {
    s.packets(d)
        .into_iter()
        .filter(|p| p.iter().any(|f| matches!(f, Wire::Ack { .. })))
        .count()
}

// ═══════════════════════════════════════════════════════════════════════
// Frames and delivery
// ═══════════════════════════════════════════════════════════════════════

/// A PING — §8.3's ack-eliciting frame the core has no verb to produce.
///
/// PING rather than STREAM on purpose: a STREAM burst also moves flow
/// credit, so a build could answer it with `MAX_STREAM_DATA` packets and
/// the ACK count would depend on §10.3's advance threshold rather than on
/// §12.4. PING isolates the cadence.
fn ping() -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_PING);
    f
}

/// §3.4's counter, read from the cleartext header.
fn counter_of(dgram: &[u8]) -> u64 {
    u64::from_le_bytes(dgram[6..14].try_into().expect("§3.4 data header"))
}

/// Seal `pt` as the peer so it lands on exactly `counter`, burning and
/// discarding the packets in between — the only way to open a gap, since
/// hiss owns the counter and only moves it forward (§7.1).
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

/// `handle_datagram` then drain, without re-sealing.
fn feed(s: &mut Solo, now: Instant, dgram: &[u8]) -> Drained {
    s.conn.handle_datagram(now, a_addr(), dgram);
    drain(&mut s.conn)
}

/// Deliver `n` in-order ack-eliciting datagrams **at one instant**, each
/// drained per §16.4, and return how many packets carrying an ACK left.
///
/// This is the "one receive batch" of the module header: the shape a
/// driver turn produces when the socket had `n` datagrams queued.
fn burst(s: &mut Solo, now: Instant, n: usize) -> usize {
    let mut emitted = 0;
    for _ in 0..n {
        let d = s.deliver(now, &ping());
        emitted += ack_bearing_packets(s, &d);
    }
    emitted
}

/// Advance only the core's own deadlines — **no further input** — until an
/// ACK covering `counter` leaves, or `horizon` passes. Returns the instant
/// the ACK left and every ACK that left with it.
///
/// A pure timer walk: this is the idle-after-burst tail, so the peer is
/// silent by construction and nothing but §12.4's `AckDelay` can produce
/// the ACK.
fn idle_until_acked(
    s: &mut Solo,
    mut deadline: Option<Instant>,
    counter: u64,
    horizon: Instant,
) -> Option<(Instant, Vec<AckFields>)> {
    for _ in 0..64 {
        let at = deadline?;
        if at > horizon {
            return None;
        }
        s.conn.handle_timeout(at);
        let d = drain(&mut s.conn);
        let got = acks(s, &d);
        if got.iter().any(|a| a.covers(counter)) {
            return Some((at, got));
        }
        let next = d.deadline;
        assert_ne!(
            next,
            Some(at),
            "§16.5: a timer that fires and re-arms at the same instant is \
             ruling 131's spin, not a cadence"
        );
        deadline = next;
    }
    panic!("the timer walk did not terminate in 64 steps");
}

// ═══════════════════════════════════════════════════════════════════════
// 1 — the cadence itself
// ═══════════════════════════════════════════════════════════════════════

/// **Ruling 271, the whole of it.** A burst of [`BURST`] ack-eliciting
/// datagrams taken off the socket in one receive batch produces **at most
/// one** ACK emission.
///
/// §12.4 as amended: a burst of N ack-eliciting datagrams processed in one
/// receive-drain produces at most one ACK emission. The warm-up packet is
/// excluded because §12.4's surviving second bullet ACKs the session's
/// first ack-eliciting packet immediately — *"the rule applies vacuously
/// and yields an immediate ACK"* — and that ACK is ratified.
///
/// **Expected RED at base**: §12.4's pre-271 first bullet owes an ACK
/// *"after every **2nd** ack-eliciting packet"*, so twenty in-order
/// packets emit ten. That is round 42-G's measured 1 ACK per 2 data
/// datagrams (33.6 % of all datagrams ACK-only, against quinn's 1.68 %),
/// reproduced at the core seam.
///
/// Separating mutations (working rule 9). `<= 1` fails a build that keeps
/// the every-2nd trigger (10), one that ACKs each packet (20), and one
/// that ACKs every 4th (5) — a "fewer than before" bound would pass the
/// last two. The **second** assertion is what stops the collapsed fix: a
/// build that simply never ACKs satisfies `<= 1` for free, and is caught
/// here rather than only in the tail tests, because a bound the degenerate
/// implementation meets asserts nothing.
#[test]
fn a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let d = s.deliver(t, &ping());
    assert_eq!(
        ack_bearing_packets(&mut s, &d),
        1,
        "§12.4: the session's first ack-eliciting packet is ACKed immediately"
    );

    let emitted = burst(&mut s, t, BURST);
    assert!(
        emitted <= 1,
        "§12.4 (ruling 271): {BURST} ack-eliciting datagrams in one receive \
         batch owe at most one ACK emission, not {emitted}"
    );

    // And the burst is still *acknowledged* — coalescing, not starving.
    let largest = BURST as u64;
    let d = drain(&mut s.conn);
    let acked = acks(&mut s, &d).iter().any(|a| a.covers(largest))
        || idle_until_acked(&mut s, d.deadline, largest, t + MAX_ACK_DELAY).is_some();
    assert!(
        acked,
        "§12.4: coalescing bounds the ACK *rate*; counter {largest} must \
         still be acknowledged within MAX_ACK_DELAY"
    );
}

/// The **literal** reading of "one receive-drain": [`BURST`] datagrams fed
/// to the core before a single `poll_output()` loop runs.
///
/// This sequence is the one §16.4 forbids a driver to perform — *"every
/// mutating call … is followed by draining `poll_output()`"* — so it is
/// unreachable from any conforming shell, and no fixture helper expresses
/// it. It is written because ruling 271's wording is "one receive-drain"
/// and someone has to state what that is; the conflict is reported in
/// `AUTHOR-ACK-REPORT.md` §C1 rather than resolved here (working rule 3).
///
/// Separating mutation: none that
/// [`a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack`] does
/// not already catch — a debt latch consumed once per drain satisfies this
/// for free. This doc once continued *"if this passes at base it pins
/// nothing … not as a gate"*; the recorded matrices settled it the other
/// way (`AUTHOR-ACK-REPORT.md`, `MUTATION-271-REPORT.md`): red at base
/// before the fix, red under the `ACK_COALESCE_MAX` → 2 revert after it.
/// It **is** a working gate for the drain-scoped reading, kept as one —
/// author conflict C1, closed in ruling 271's record.
#[test]
fn twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let d = s.deliver(t, &ping());
    assert_eq!(ack_bearing_packets(&mut s, &d), 1, "the vacuous first ACK");

    for _ in 0..BURST {
        let dgram = s.peer.seal(&ping());
        s.conn.handle_datagram(t, a_addr(), &dgram);
    }
    let d = drain(&mut s.conn);
    let emitted = ack_bearing_packets(&mut s, &d);
    assert!(
        emitted <= 1,
        "§12.4 (ruling 271), literal reading: one `poll_output` loop over \
         {BURST} datagrams owes at most one ACK emission, not {emitted}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2 — MAX_ACK_DELAY is the outer bound
// ═══════════════════════════════════════════════════════════════════════

/// One ack-eliciting datagram, then silence: the ACK leaves within
/// `MAX_ACK_DELAY`.
///
/// §12.4: the `AckDelay` timer is *"armed at `MAX_ACK_DELAY` on receipt of
/// the first unacknowledged ack-eliciting packet"*, and firing it owes the
/// ACK. Nothing in ruling 271 touches that clause, and it is the clause
/// that makes coalescing safe: the peer's §13.1 estimator and §13.2 loss
/// detection never wait longer than 25 ms for feedback, however quiet the
/// receiver's own drain schedule becomes.
///
/// A warm-up packet precedes it so the packet under test is not the
/// session's first — §12.4's vacuous gap rule would ACK that one
/// immediately and the timer would never be exercised.
///
/// **Expected GREEN at base, and must stay green.** Separating mutations:
/// a coalescing fix that emits an ACK *only when a subsequent drain
/// happens* never emits here at all — there is no subsequent drain — and
/// `idle_until_acked` returns `None`, failing the `expect`. A fix that
/// arms the timer at `2 · MAX_ACK_DELAY`, or at `KEEPALIVE_TIMEOUT`, fails
/// the bound instead; the two are asserted separately so the failure
/// message says which. The bound is an inequality against the *deadline*
/// `at + MAX_ACK_DELAY`, not "some ACK eventually arrives" — the latter is
/// satisfied by a build that waits a PTO.
#[test]
fn a_lone_ack_eliciting_packet_is_acked_within_max_ack_delay() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let _ = s.deliver(t, &ping()); // counter 0 — the vacuous immediate ACK

    let at = t + Duration::from_millis(1);
    let d = s.deliver(at, &ping()); // counter 1
    let when = if acks(&mut s, &d).iter().any(|a| a.covers(1)) {
        Some(at)
    } else {
        idle_until_acked(&mut s, d.deadline, 1, at + MAX_ACK_DELAY).map(|(w, _)| w)
    };

    let when = when.expect(
        "§12.4: an ack-eliciting packet is acknowledged when the `AckDelay` \
         timer fires, even if no further drain ever happens",
    );
    assert!(
        when <= at + MAX_ACK_DELAY,
        "§12.4: MAX_ACK_DELAY (25 ms) is the outer bound; the ACK left {:?} \
         after arrival",
        when - at
    );
}

/// The **tail of a burst**: after [`BURST`] + 1 datagrams arrive at one
/// instant and the peer goes quiet, the largest counter is still
/// acknowledged within `MAX_ACK_DELAY`.
///
/// An odd count is deliberate. At base §12.4's every-2nd trigger fires on
/// the even arrivals, so the final packet is *"the first unacknowledged
/// ack-eliciting packet"* and only the timer can discharge it — the base
/// core walks the same timer path the ratified cadence will walk for the
/// whole burst. Under ruling 271 the single coalesced ACK may already
/// cover the largest, which is why the check is a disjunction rather than
/// an assertion that the timer fired.
///
/// **Expected GREEN at base, and must stay green.** Separating mutation: a
/// fix that coalesces by dropping the `AckDelay` arming leaves the tail of
/// every burst unacknowledged until the next inbound packet — which, at
/// the end of a transfer, is never. On a bulk send that is the last window
/// of data, and the sender recovers it by PTO
/// (`srtt + max(4·rttvar, K_GRANULARITY) + MAX_ACK_DELAY`, §13.3) instead
/// of by ACK. This test is that regression.
#[test]
fn the_tail_of_an_idle_burst_is_acked_within_max_ack_delay() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let _ = s.deliver(t, &ping()); // counter 0
    let _ = burst(&mut s, t, BURST + 1); // counters 1..=21
    let largest = BURST as u64 + 1;

    let d = drain(&mut s.conn);
    let when = if acks(&mut s, &d).iter().any(|a| a.covers(largest)) {
        Some(t)
    } else {
        idle_until_acked(&mut s, d.deadline, largest, t + MAX_ACK_DELAY).map(|(w, _)| w)
    };

    let when = when.expect(
        "§12.4: the last packet of a burst is acknowledged by the `AckDelay` \
         timer when the peer falls silent",
    );
    assert!(
        when <= t + MAX_ACK_DELAY,
        "§12.4: the burst tail waited {:?}, past MAX_ACK_DELAY",
        when - t
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3 — immediate on gap, mid-batch
// ═══════════════════════════════════════════════════════════════════════

/// A gap opening in the middle of a receive batch is ACKed **in that
/// batch**, not deferred to its end.
///
/// §12.4's second bullet, unchanged by ruling 271: *"An ACK is owed
/// **immediately** on out-of-order arrival: an ack-eliciting packet whose
/// counter is not exactly one greater than the window's previous greatest
/// (it opens, fills, or sits inside a gap)."* This is the peer's fast path
/// to §13.2 — the gap **is** the loss signal, and the packet threshold
/// (`K_PACKET_THRESHOLD` = 3) can only fire on an ACK that reports it.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutations: a fix that folds the gap into the end-of-batch
/// ACK emits nothing at this delivery and fails the `unwrap_or_else`. A
/// fix that reacts to the gap but reports a contiguous `0..=8` — the
/// tempting simplification once ranges are being coalesced — passes that
/// and fails the block assertion, having told the sender that two packets
/// it must retransmit arrived.
///
/// **This test deliberately does not bound how many ACKs the batch
/// emitted.** Under ruling 271 the gap rule is the only thing that can
/// produce a second emission from one batch, so bounding the count would
/// be this author deciding whether "at most one per drain" admits the gap
/// exception — reported as `AUTHOR-ACK-REPORT.md` §C2 instead (working
/// rule 3).
#[test]
fn a_gap_mid_batch_is_acked_in_the_same_batch() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let _ = s.deliver(t, &ping()); // counter 0
    let _ = burst(&mut s, t, 5); // counters 1..=5, in order

    // Counters 6 and 7 are sealed and discarded, so 8 opens a gap.
    let dgram = seal_at(&mut s.peer, 8, &ping());
    let d = feed(&mut s, t, &dgram);

    let got = acks(&mut s, &d);
    let reporting = got
        .iter()
        .find(|a| a.largest == 8)
        .unwrap_or_else(|| panic!("§12.4: out-of-order arrival is ACKed immediately; got {got:?}"));
    assert_eq!(
        reporting.blocks(),
        vec![8..=8, 0..=5],
        "§12.1: the gap at 6..=7 is reported as a gap, newest-first"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4 — loss detection survives the cadence
// ═══════════════════════════════════════════════════════════════════════

/// A coalesced ACK carries **every** range a per-packet ACK would.
///
/// The content half of the recovery obligation. §12.2 derives the ACK from
/// the replay window's snapshot — *"the single received-packet record;
/// there is no second tracker"* — so coalescing may change *when* an ACK
/// leaves and must not change *what it says*. §12.5's sender intersects
/// the ranges with its in-flight set, so a range the receiver drops is a
/// packet the sender keeps believing lost, or believes arrived when it did
/// not.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutations, and the first is the one a naive coalescing patch
/// actually commits: collapsing the batch to `largest..=largest` — "we saw
/// everything up to here" — reports `[9..=9]` and silently claims counters
/// 0..=3 were never received, so the sender retransmits four packets that
/// arrived. Second: reporting the contiguous `0..=9`, which hides the real
/// loss at 4..=5 and starves §13.2 of the very gap it needs. Only the
/// exact block list separates both; "the ACK mentions 9" separates
/// neither.
#[test]
fn a_coalesced_ack_reports_every_range_a_per_packet_ack_would() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    for c in [0u64, 1, 2, 3] {
        let dgram = seal_at(&mut s.peer, c, &ping());
        let _ = feed(&mut s, t, &dgram);
    }
    // 4 and 5 are lost on the way. 6..=9 arrive as one batch.
    let mut last = Drained::default();
    for c in [6u64, 7, 8, 9] {
        let dgram = seal_at(&mut s.peer, c, &ping());
        last = feed(&mut s, t, &dgram);
    }

    let mut got = acks(&mut s, &last);
    if !got.iter().any(|a| a.covers(9)) {
        got = idle_until_acked(&mut s, last.deadline, 9, t + MAX_ACK_DELAY)
            .expect("§12.4: counter 9 is acknowledged within MAX_ACK_DELAY")
            .1;
    }

    let newest = got
        .iter()
        .rfind(|a| a.covers(9))
        .expect("an ACK covering counter 9");
    assert_eq!(
        newest.blocks(),
        vec![6..=9, 0..=3],
        "§12.2/§12.1: the coalesced ACK reports both blocks and the gap at \
         4..=5, exactly as a per-packet ACK would"
    );
}

/// A single lost packet is recovered **without the clock advancing**.
///
/// The latency half of the recovery obligation, stated so that no timer
/// value has to be reproduced here. §13.2's packet threshold declares a
/// packet lost as soon as *"a later packet in its space has been
/// acknowledged"* and it is `K_PACKET_THRESHOLD` = 3 or more counters
/// below the largest acknowledged — no waiting. The loss opens a gap in
/// the receiver's window, §12.4's second bullet ACKs that gap immediately,
/// and the sender retransmits in the same instant. So on a paused clock
/// the whole repair completes at `t`, and a build that needs virtual time
/// to advance is a build whose feedback arrived late.
///
/// `Pair::pump` moves datagrams both ways at one instant until the wire is
/// quiet, so the transfer either completes at `t` or does not complete.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutations: a fix that defers the gap ACK to the `AckDelay`
/// timer leaves B's report of the hole 25 ms away, `pump(t)` never carries
/// it, and A recovers only by PTO — `read_all` returns short and this
/// fails. A fix that coalesces the *ranges* into a contiguous block hides
/// the hole entirely, A never retransmits, and it fails the same way. A
/// fix that starves ACKs enough to stall §14.5's admission gate fails at
/// the same assertion. The test is stated as a completion, not as a
/// retransmission count, precisely so that it cannot be satisfied by a
/// build that retransmits the wrong packet.
#[test]
fn a_single_lost_packet_is_recovered_without_advancing_the_clock() {
    let t = t0();
    let mut p = Pair::installed_at(t);

    let r = p.a.open(Dir::Uni).expect("the first uni stream fits");
    let payload = ramp(0, 8192);
    write_all(&mut p, t, r, &payload);
    let _ = p.drain_a();
    assert!(
        p.a_to_b.len() >= 4,
        "the fixture must produce a multi-packet flight for §13.2's packet \
         threshold to be reachable; got {}",
        p.a_to_b.len()
    );
    assert!(
        p.a.bytes_in_flight() > 0,
        "§13.5/§14.5: the data is tracked while in flight"
    );

    // The first data packet dies on the wire. Everything behind it opens a
    // gap in B's window (§7.2), which is §12.4's immediate-ACK trigger.
    let lost = p.a_to_b.remove(0);
    assert!(!lost.is_empty());

    let _ = p.pump(t);

    let streams = accept_all(&mut p.b, Dir::Uni);
    assert_eq!(streams.len(), 1, "§9.2: one peer-opened uni stream");
    let got = read_all(&mut p.b, t, streams[0].1);
    assert_eq!(
        got.len(),
        payload.len(),
        "§13.2: the lost packet is declared by the packet threshold and \
         retransmitted in the same instant — no virtual time passed"
    );
    assert_eq!(got, payload, "§9.5: the stream reassembles in order");
}

// ═══════════════════════════════════════════════════════════════════════
// 5 — RTT sampling survives
// ═══════════════════════════════════════════════════════════════════════

/// The estimator still gets a sample across a coalesced exchange, and the
/// sample is bounded by the real RTT plus `MAX_ACK_DELAY`.
///
/// §13.1: *"An ACK yields an RTT sample when its `largest` is newly
/// acknowledged"*, and *"Before any sample the estimator seeds from
/// `K_INITIAL_RTT`"* = 333 ms. Coalescing reduces how many ACKs arrive; it
/// must not reduce them to zero, and the delay it adds is exactly what
/// §12.3's `ack_delay` and §13.1's capped subtraction exist to absorb.
///
/// The exchange injects a 20 ms one-way delay by handing each direction's
/// datagrams over at a later instant — the sans-io equivalent of
/// `FlakyPolicy::with_delay`, and cheaper.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutations. `has_sample()` fails a fix that emits no ACK at
/// all in this window, even after its own timer — the collapsed cadence.
/// The **lower** bound `min_rtt >= RTT` fails a build that samples but
/// computes zero (a `now`-minus-`now` bug, which every "an RTT exists"
/// assertion passes). The **upper** bound `min_rtt <= RTT + MAX_ACK_DELAY`
/// is the cadence statement proper: it fails a fix whose ACK waited for a
/// PTO or for a second burst rather than for §12.4's own timer. And
/// `smoothed_rtt != K_INITIAL_RTT` fails a build that takes the sample and
/// never folds it, which the two `min_rtt` bounds alone would not see.
#[test]
fn the_estimator_still_samples_across_a_coalesced_exchange() {
    const RTT: Duration = Duration::from_millis(20);
    let t = t0();
    let mut p = Pair::installed_at(t);

    let r = p.a.open(Dir::Uni).expect("the first uni stream fits");
    write_all(&mut p, t, r, &ramp(0, 8192));
    let _ = p.drain_a();

    // One way out, one way back: B sees the flight at t + 10 ms and A sees
    // whatever B answered at t + 20 ms.
    let _ = p.flush_a_to_b(t + RTT / 2);
    let _ = p.flush_b_to_a(t + RTT);

    // If B coalesced without emitting, its ACK is owed to `AckDelay`; walk
    // that timer and deliver what it produces. This is the ratified outer
    // bound being *used*, not worked around.
    if !p.a.recovery().rtt().has_sample() {
        let at = t + RTT / 2 + MAX_ACK_DELAY;
        p.b.handle_timeout(at);
        let _ = p.drain_b();
        let _ = p.flush_b_to_a(at + RTT / 2);
    }

    assert!(
        p.a.recovery().rtt().has_sample(),
        "§13.1: a coalesced exchange must still yield an RTT sample within \
         MAX_ACK_DELAY of the data arriving"
    );
    let min_rtt = p.a.recovery().rtt().min_rtt().expect("a sample exists");
    assert!(
        min_rtt >= RTT,
        "§13.1: min_rtt is the raw latest_rtt, never below the real round \
         trip; got {min_rtt:?}"
    );
    assert!(
        min_rtt <= RTT + MAX_ACK_DELAY,
        "§13.1/§12.4: the sample may be delayed by at most MAX_ACK_DELAY; \
         got {min_rtt:?}"
    );
    assert_ne!(
        p.a.smoothed_rtt(),
        K_INITIAL_RTT,
        "§13.1: the first sample replaces the K_INITIAL_RTT seed"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6 — liveness neutrality (ruling 33)
// ═══════════════════════════════════════════════════════════════════════

/// Emitting a coalesced ACK moves no liveness clock.
///
/// Ruling 33 / §12.4: pure-ACK packets are *"sealed `seal_quiet` (§7.4),
/// are not ack-eliciting …, are never tracked for loss"*. §7.5's death
/// clock is the thing this protects: a send that marks defers the peer's
/// keepalive obligation, and a send that is ack-eliciting arms the death
/// deadline. An ACK does neither, or an ACK-only exchange keeps a
/// half-open session alive forever — SECV5-2's immortal session.
///
/// The emission here is driven by the **timer**, with no inbound packet at
/// that instant, which is the only way to observe the send side alone:
/// §7.4's death deadline is derived from `last_authenticated_recv`, so any
/// test that delivers a packet in order to provoke an ACK moves the very
/// clock it is checking.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutations: a fix that seals the coalesced ACK with `seal`
/// rather than `seal_quiet` — an easy slip when the ACK starts riding
/// packet-building paths it did not before — moves `last_send()` and fails
/// the first assertion. A fix that makes the coalesced ACK ack-eliciting
/// (say by packing a PING beside it to force a prompt response) arms the
/// death deadline and fails the second and third. The three are asserted
/// separately because they fail for different reasons and §14.5's
/// exemption depends on the third.
#[test]
fn a_coalesced_ack_moves_no_liveness_clock() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    let before = *s.conn.liveness().expect("installed");
    let _ = s.deliver(t, &ping());
    let _ = burst(&mut s, t, BURST);

    // Discharge whatever is owed, from the timer, with the peer silent.
    let mut deadline = drain(&mut s.conn).deadline;
    for _ in 0..8 {
        let Some(at) = deadline.filter(|at| *at <= t + MAX_ACK_DELAY) else {
            break;
        };
        s.conn.handle_timeout(at);
        let next = drain(&mut s.conn).deadline;
        if next == Some(at) {
            break;
        }
        deadline = next;
    }

    let after = *s.conn.liveness().expect("still installed");
    assert_eq!(
        after.last_send(),
        before.last_send(),
        "§7.4/ruling 33: an ACK is `seal_quiet` and does not move §7.5's \
         marking clock"
    );
    assert!(
        !after.is_armed(),
        "§7.4: an ACK is not ack-eliciting, so it does not arm the death \
         deadline"
    );
    assert_eq!(
        s.conn.bytes_in_flight(),
        0,
        "§12.4/§13.5: pure-ACK packets are never tracked for loss, and \
         §14.5 exempts them from the window"
    );
}

/// An ACK-only emission cannot push out a death deadline that real traffic
/// armed.
///
/// The second half of ruling 33's neutrality, and the one that is a
/// security property rather than bookkeeping: §7.4's *"The send clock never
/// defers death, it only enables it"*. Once the death deadline is armed,
/// the **receive** clock is what moves it; our own outbound ACKs must not,
/// or a connection whose peer has vanished stays alive as long as it keeps
/// ACKing.
///
/// **Expected GREEN at base, and must stay green.**
///
/// Separating mutation: any fix that arms or re-derives liveness on the
/// coalesced-ACK emission path — including the plausible one of treating
/// "we built a packet" as "we sent, so re-anchor". Equality of
/// `deadline()` and of `last_authenticated_recv()` across the emission is
/// the assertion; "the connection is still alive" would pass every one of
/// those builds.
#[test]
fn an_ack_only_emission_cannot_defer_death() {
    let t = t0();
    let mut s = Solo::installed_at(t);

    // Real traffic first, so the death deadline is armed and has a value
    // to compare: our own ack-eliciting send is §7.4's arming trigger.
    let r = s.conn.open(Dir::Uni).expect("our own uni space");
    let _ = s
        .conn
        .write(t, r, &ramp(0, 512))
        .expect("§16.9: an installed core writes");
    let _ = drain(&mut s.conn);

    let _ = s.deliver(t, &ping());
    let _ = burst(&mut s, t, BURST);
    let armed = *s.conn.liveness().expect("installed");

    let d = drain(&mut s.conn);
    if let Some(at) = d.deadline.filter(|at| *at <= t + MAX_ACK_DELAY) {
        s.conn.handle_timeout(at);
        let _ = drain(&mut s.conn);
    }

    let after = *s.conn.liveness().expect("still installed");
    assert_eq!(
        after.deadline(),
        armed.deadline(),
        "§7.4/ruling 33: an ACK-only emission does not defer the death \
         deadline"
    );
    assert_eq!(
        after.last_authenticated_recv(),
        armed.last_authenticated_recv(),
        "§7.4: and it does not touch the receive anchor the deadline hangs \
         from"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Local fixture helpers
// ═══════════════════════════════════════════════════════════════════════

/// Write every byte of `data` on A's stream `r`.
///
/// No pumping inside the loop, deliberately: the caller of
/// [`a_single_lost_packet_is_recovered_without_advancing_the_clock`] needs
/// A's whole flight sitting in `a_to_b` so it can drop one datagram, and a
/// helper that pumped would have delivered it first.
fn write_all(p: &mut super::testfix::Pair, now: Instant, r: StreamRef, data: &[u8]) {
    let mut at = 0;
    for _ in 0..4096 {
        if at == data.len() {
            return;
        }
        let n =
            p.a.write(now, r, &data[at..])
                .expect("write must not error");
        assert!(
            n > 0,
            "§10.1: the initial credit must admit {} bytes",
            data.len()
        );
        at += n;
    }
    panic!("write_all stalled at {at} of {} bytes", data.len());
}

/// Read everything currently readable on `r`, stopping at FIN or at the
/// first empty read.
///
/// Unlike [`read_exactly`] this never panics on short data — the test
/// asserts the length itself, so a starved build reports how far it got
/// instead of dying inside the helper with a message that names the
/// fixture rather than the defect.
fn read_all(conn: &mut super::Connection<Suite>, now: Instant, r: StreamRef) -> Vec<u8> {
    let mut got = Vec::new();
    let mut buf = vec![0u8; 4096];
    for _ in 0..4096 {
        match conn.read(now, r, &mut buf).expect("read must not error") {
            None | Some(0) => return got,
            Some(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    panic!("read_all did not terminate");
}

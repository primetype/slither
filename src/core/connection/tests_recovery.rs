//! Slice 5a acceptance tests for §13 (loss detection, RTT, PTO) and §14
//! (NewReno congestion control).
//!
//! Written by 5a-test-recovery **from `SPEC.md` §13/§14/§8.7,
//! `.slices/05-reliability/CONTRACT-5a.md` and Round 23's rulings alone**,
//! in parallel with the implementation and without reading it (CLAUDE.md
//! working rule 6). The companion report is
//! `.slices/05-reliability/TESTS-5a-recovery.md`.
//!
//! # Two levels, and the choice is not cosmetic
//!
//! - **Unit level** — `RttEstimator`, `Recovery` and `NewReno` are driven
//!   directly. §13/§14 are arithmetic, and arithmetic is pinned by exact
//!   equality on a scripted input, never by an inequality a collapsed
//!   build satisfies for free. Every value asserted here is computed by
//!   hand in the test's own comment from the spec's formula, so a build
//!   that agrees with itself and disagrees with §13 fails.
//! - **Core level** — `Solo` from [`testfix`](super::testfix), for the
//!   three properties that are *not* arithmetic: what a packet's `size`
//!   counts (ruling 136), what the admission gate admits (§14.5), and
//!   whether a probe is exempt from the gate but not from the accounting
//!   (§13.4, ruling 43).
//!
//! # Working rule 9 is the organising principle
//!
//! Every test carries a `Mutation caught:` line naming what the broken
//! build does and which assertion separates it. A congestion controller is
//! the most prone construct in the project — *"the window grew"* passes
//! against almost anything, and *"the window shrank on loss"* passes a
//! controller that halves on every ACK. Where an assertion would also fail
//! a **conforming** build it is not written: an assertion a conforming
//! build can fail is a flake, not a pin.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here
//! reads a clock. Plain `#[test]`, no `sleep`, no runtime. The
//! paused-clock obligation lands on 5b's `tests/story_reliability.rs`.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::time::{Duration, Instant};

use super::*;

// ═══════════════════════════════════════════════════════════════════════
// The integration seam
// ═══════════════════════════════════════════════════════════════════════
//
// `CONTRACT-5a.md` §2.2/§2.3 pin these type names and their signatures but
// not the modules they live in beyond the file names in §3's ownership
// table. **If integration has to touch anything in this file, expect it to
// be these four lines and the `sent()` helper below.**
use super::congestion::{Controller, NewReno};
use super::frame::{Ack, Frame};
use super::recovery::{Recovery, RttEstimator, SentFrame, SentPacket};
use super::stream_id::Dir;
use super::streams::StreamRef;
use super::testfix::*;

use crate::constants::{
    AEAD_TAG_LEN, DATA_HEADER_LEN, INITIAL_WINDOW, K_GRANULARITY, K_INITIAL_RTT, MAX_DATAGRAM,
    MINIMUM_WINDOW, STREAMS_CREDIT_BATCH,
};
use crate::packet::ReferenceSuite;

type Suite = ReferenceSuite;

// ═══════════════════════════════════════════════════════════════════════
// Arithmetic helpers
// ═══════════════════════════════════════════════════════════════════════

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn us(n: u64) -> Duration {
    Duration::from_micros(n)
}

/// §13.3's base interval before any RTT sample:
/// `K_INITIAL_RTT + max(4 · K_INITIAL_RTT/2, K_GRANULARITY) + MAX_ACK_DELAY`
/// `= 333 + 666 + 25 = 1024 ms`.
///
/// Written out rather than imported so a constant that moves fails here
/// instead of moving the expectation with it.
const INITIAL_PTO: Duration = Duration::from_millis(1024);

// ═══════════════════════════════════════════════════════════════════════
// Sent-packet fixtures
// ═══════════════════════════════════════════════════════════════════════

/// A `StreamRef` to tag sent-packet entries with.
///
/// The unit tests need *identities* they can assert on by value, and
/// `SentFrame::Stream { r, range, .. }` is the only variant with enough
/// room to make each entry distinct. §16.9 makes `open()` legal before
/// install, so one throwaway core hands out the ref without a session.
fn tag_ref() -> StreamRef {
    let mut c: Connection<Suite> = Connection::connecting([0x11u8; 32]);
    c.open(Dir::Uni)
        .expect("§16.9: open() before install is legal")
}

/// A per-counter frame identity: distinct for every counter, so
/// `AckOutcome::acked` and `::lost` can be asserted by equality.
fn tag(r: StreamRef, counter: u64) -> SentFrame {
    SentFrame::Stream {
        r,
        range: counter * 100..counter * 100 + 10,
        fin: false,
    }
}

/// **The one struct literal in this file.**
///
/// Ruling 137 (`CONTRACT-5a.md` §0) says *"`SentPacket` carries a `u32`
/// path generation from slice 5, held at 0"*, and §2.2's struct listing
/// **omits the field**. That contradiction is reported in
/// `TESTS-5a-recovery.md` §4; the ruling is binding and §2.2's list is the
/// one working rule 8 calls a stated construction with a contradicted
/// scope, so the field is written here. Every `SentPacket` in the file is
/// built through this function, so if the implementer named the field
/// something other than `path_gen` the fix is one token in one place.
fn sent(counter: u64, time_sent: Instant, size: u64, r: StreamRef) -> SentPacket {
    SentPacket {
        counter,
        time_sent,
        size,
        app_limited: false,
        path_gen: 0,
        frames: vec![tag(r, counter)],
    }
}

/// §12.1's descending encoding of an arbitrary acknowledged set, built
/// here rather than through the implementation.
///
/// Deliberately **not** `ack::derive`: a derivation that agrees with the
/// recovery walk about a wrong encoding must fail here. The construction
/// is §12.1's own — each additional pair starts `gap + 2` below the
/// previous range's smallest counter.
fn ack_of(counters: &[u64], ack_delay_us: u64) -> Ack {
    let mut c: Vec<u64> = counters.to_vec();
    c.sort_unstable();
    c.dedup();
    assert!(!c.is_empty(), "an ACK acknowledges at least one counter");

    let mut runs: Vec<(u64, u64)> = Vec::new();
    for &n in &c {
        match runs.last_mut() {
            Some((_, hi)) if *hi + 1 == n => *hi = n,
            _ => runs.push((n, n)),
        }
    }
    runs.reverse(); // §12.2's order: newest first, descending.

    let (lo0, hi0) = runs[0];
    let mut ranges = Vec::new();
    let mut prev_lo = lo0;
    for &(lo, hi) in &runs[1..] {
        ranges.push((prev_lo - hi - 2, hi - lo));
        prev_lo = lo;
    }
    Ack {
        largest: hi0,
        ack_delay: ack_delay_us,
        first_range: hi0 - lo0,
        ranges,
    }
}

/// The same ACK as a sealable frame stream, for the core-level tests.
fn ack_bytes(ack: &Ack) -> Vec<u8> {
    let mut out = Vec::new();
    Frame::Ack(ack.clone()).encode(&mut out);
    out
}

/// §3.4's counter, read off the datagram header rather than through the
/// session — so a test can acknowledge exactly what the core sealed
/// without decrypting it.
fn counter_of(dgram: &[u8]) -> u64 {
    u64::from_le_bytes(
        dgram[6..14]
            .try_into()
            .expect("§3.4: the data header is 14 bytes"),
    )
}

fn counters_of(d: &Drained) -> Vec<u64> {
    d.transmits().iter().map(|t| counter_of(&t.data)).collect()
}

fn total_bytes(d: &Drained) -> u64 {
    d.transmits().iter().map(|t| t.data.len() as u64).sum()
}

// ═══════════════════════════════════════════════════════════════════════
// §13.1 — RTT estimation
// ═══════════════════════════════════════════════════════════════════════

mod rtt {
    use super::*;

    /// §13.1: *"Before any sample the estimator seeds from `K_INITIAL_RTT`
    /// with `rttvar = K_INITIAL_RTT / 2`."*
    ///
    /// Mutation caught: a build seeding `rttvar = 0` — the seed every
    /// RFC-9002 transcription that forgets §5.3's initial state produces.
    /// It gives a first PTO of 358 ms instead of 1024 ms, which is still
    /// "plausible" and which no completion test on a healthy path can
    /// see. The PTO equality is what separates it; `rttvar()` alone would
    /// too, but both are asserted because the interval is the observable
    /// the rest of §13 is built on.
    #[test]
    fn before_any_sample_the_estimator_reads_the_initial_seed() {
        let e = RttEstimator::new();

        assert!(!e.has_sample(), "§14.4's precondition starts false");
        assert_eq!(e.smoothed_rtt(), K_INITIAL_RTT, "§13.1: 333 ms");
        assert_eq!(e.rttvar(), K_INITIAL_RTT / 2, "§13.1: 166.5 ms");
        assert_eq!(
            e.pto_interval(),
            INITIAL_PTO,
            "§13.3: 333 + max(4 · 166.5, 1) + 25 = 1024 ms"
        );
    }

    /// §13.1's first-sample rule: `smoothed = latest`, `rttvar = latest/2`,
    /// `min_rtt = latest`.
    ///
    /// Mutation caught: a build that runs the ⅞/⅛ recurrence from the
    /// seed on the *first* sample. From 333 ms with a 100 ms sample it
    /// produces 303.875 ms — within a factor of three of the truth, so
    /// every "the RTT estimate is roughly right" bound passes it, and the
    /// connection then runs a PTO nearly a second too long for the whole
    /// first flight. Exact equality is the only assertion that separates
    /// them.
    #[test]
    fn the_first_sample_replaces_the_seed_outright() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);

        assert!(e.has_sample());
        assert_eq!(e.smoothed_rtt(), ms(100), "§13.1: smoothed = latest");
        assert_eq!(e.rttvar(), ms(50), "§13.1: rttvar = latest / 2");
    }

    /// §13.1's later-sample recurrence, to the nanosecond.
    ///
    /// Hand-computed from the spec: after a 100 ms first sample,
    /// `smoothed = 100`, `rttvar = 50`. A second sample of 200 ms with no
    /// reported delay gives
    /// `rttvar = ¾·50 + ¼·|100 − 200| = 37.5 + 25 = 62.5 ms` and
    /// `smoothed = ⅞·100 + ⅛·200 = 87.5 + 25 = 112.5 ms`.
    /// Both are exact in nanoseconds, so integer arithmetic reproduces
    /// them without rounding and the assertion is not a tolerance.
    ///
    /// Mutation caught: `smoothed = latest` on every sample — the
    /// single commonest collapse of this estimator, and one that passes
    /// any "the estimate tracks the true RTT" bound because on a stable
    /// path it *is* the true RTT. It gives 200 ms here. A build that
    /// updates `smoothed` before `rttvar` (using the new smoothed in the
    /// deviation term) gives `rttvar = 37.5 + ¼·|112.5 − 200| = 59.375`,
    /// which the `rttvar` assertion separates and the `smoothed` one does
    /// not — which is why both are asserted.
    #[test]
    fn a_later_sample_follows_the_seven_eighths_recurrence_exactly() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        e.sample(ms(200), Duration::ZERO);

        assert_eq!(e.rttvar(), us(62_500), "¾ · 50 + ¼ · |100 − 200|");
        assert_eq!(e.smoothed_rtt(), us(112_500), "⅞ · 100 + ⅛ · 200");
    }

    /// §13.1: the peer's `ack_delay` *is* subtracted when the result stays
    /// at or above `min_rtt`.
    ///
    /// 100 ms first sample fixes `min_rtt = 100`. A 200 ms sample with a
    /// 20 ms reported delay: `200 ≥ 100 + 20`, so `adjusted = 180`, giving
    /// `rttvar = 37.5 + ¼·|100 − 180| = 57.5` and
    /// `smoothed = 87.5 + 22.5 = 110 ms`.
    ///
    /// Mutation caught: a build that ignores `ack_delay` entirely — the
    /// tempting simplification, since the field is the peer's word and
    /// slither could just not trust it. It gives 112.5 ms. The failure it
    /// causes is a PTO permanently longer than the path needs, which
    /// slows recovery and breaks nothing, so no functional test sees it.
    #[test]
    fn the_peer_ack_delay_is_subtracted_when_the_result_stays_above_min_rtt() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        e.sample(ms(200), ms(20));

        assert_eq!(e.rttvar(), us(57_500));
        assert_eq!(e.smoothed_rtt(), ms(110));
    }

    /// §13.1: *"subtracted **only when** doing so does not push the sample
    /// below `min_rtt`"*.
    ///
    /// `min_rtt = 100` from the first sample. A 110 ms sample with a 25 ms
    /// delay would give 85 ms — below `min_rtt` — so the delay is **not**
    /// applied and `adjusted = 110`:
    /// `rttvar = 37.5 + ¼·|100 − 110| = 40 ms`,
    /// `smoothed = 87.5 + 13.75 = 101.25 ms`.
    ///
    /// Mutation caught: a build that always subtracts. It gives
    /// `adjusted = 85`, `smoothed = 98.125 ms` — **below `min_rtt`**, an
    /// estimate of a round trip shorter than the shortest round trip ever
    /// observed. That shortens every PTO for the connection's life and
    /// manufactures spurious probes; §13.1 wrote the guard for exactly
    /// this, and a hostile peer that inflates `ack_delay` is the reason it
    /// is not merely a nicety. The exact value separates them; so does
    /// `smoothed_rtt() >= ms(100)`, asserted as well because it is the
    /// property, and the equality is only its witness.
    #[test]
    fn the_peer_ack_delay_is_refused_when_it_would_push_the_sample_below_min_rtt() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        e.sample(ms(110), ms(25));

        assert_eq!(e.rttvar(), ms(40));
        assert_eq!(e.smoothed_rtt(), us(101_250));
        assert!(
            e.smoothed_rtt() >= ms(100),
            "§13.1: the estimate never falls below min_rtt through ack_delay"
        );
    }

    /// §13.1: the peer's report is *"capped at `MAX_ACK_DELAY`"* before the
    /// `min_rtt` guard is consulted.
    ///
    /// A 200 ms sample with a **100 ms** reported delay: the cap makes it
    /// 25 ms, `200 ≥ 100 + 25` holds, `adjusted = 175`, so
    /// `smoothed = 87.5 + 21.875 = 109.375 ms`.
    ///
    /// Mutation caught: a build that applies the guard but not the cap.
    /// It subtracts the full 100 ms, `adjusted = 100`, `smoothed = 100 ms`
    /// — and because the guard still holds (`200 ≥ 100 + 100`), the
    /// `min_rtt` assertion above does **not** catch it. This is the
    /// hostile-peer direction: an inflated `ack_delay` shrinks the
    /// estimate, shrinks the PTO, and makes us probe on the peer's
    /// schedule. Only the cap bounds it, and only this test asserts it.
    #[test]
    fn the_peer_ack_delay_is_capped_before_the_min_rtt_guard_is_applied() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        e.sample(ms(200), ms(100));

        assert_eq!(e.smoothed_rtt(), us(109_375), "the cap binds at 25 ms");
    }

    /// §13.2: `loss_delay = max(9/8 · max(smoothed, latest), K_GRANULARITY)`
    /// — the **`max` with `latest`**, which is the clause a build drops.
    ///
    /// After samples of 100 ms then 300 ms: `smoothed = ⅞·100 + ⅛·300 =
    /// 125 ms`, `latest = 300 ms`. The rule gives `9/8 · 300 = 337.5 ms`.
    ///
    /// Mutation caught: `9/8 · smoothed`, dropping the `max`. It gives
    /// 140.625 ms. On a stable path the two agree exactly — which is why
    /// this is tested on a **rising** one, and why a build carrying this
    /// defect passes every steady-state loss test and then declares half
    /// the flight lost the moment the path's RTT climbs, converting a
    /// latency increase into a congestion collapse.
    #[test]
    fn the_loss_delay_takes_the_larger_of_smoothed_and_latest() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        assert_eq!(
            e.loss_delay(),
            us(112_500),
            "with smoothed == latest == 100, 9/8 · 100"
        );

        e.sample(ms(300), Duration::ZERO);
        assert_eq!(e.smoothed_rtt(), ms(125), "⅞ · 100 + ⅛ · 300");
        assert_eq!(e.loss_delay(), us(337_500), "9/8 · max(125, 300)");
    }

    /// §13.2's floor: `max(…, K_GRANULARITY)`.
    ///
    /// On a 100 µs path `9/8 · 100 µs = 112.5 µs`, below the 1 ms
    /// granularity, so the floor binds.
    ///
    /// Mutation caught: a build omitting the floor. It declares packets
    /// lost 112 µs after they were sent — on a loopback or a datacentre
    /// path, faster than the peer can plausibly answer, so every
    /// reordering becomes a loss and the window never opens. A one-sided
    /// pin: the value above the floor is covered by the test before this
    /// one, so both sides of the `max` are asserted.
    #[test]
    fn the_loss_delay_never_falls_below_the_granularity_floor() {
        let mut e = RttEstimator::new();
        e.sample(us(100), Duration::ZERO);

        assert_eq!(e.loss_delay(), K_GRANULARITY, "§13.2's 1 ms floor");
    }

    /// §13.3's interval from a sampled estimator, and §14.4's use of the
    /// same function with `pto_count = 0`.
    ///
    /// After a single 100 ms sample: `smoothed = 100`, `rttvar = 50`, so
    /// `PTO = 100 + max(4 · 50, 1) + 25 = 325 ms`.
    ///
    /// Mutation caught: a build omitting `MAX_ACK_DELAY` from the sum
    /// (300 ms), and a build using `rttvar` rather than `4 · rttvar`
    /// (175 ms). Both are within a small factor of the truth and both
    /// probe early forever — a bandwidth cost, never a correctness
    /// failure, so nothing else in the suite would notice.
    #[test]
    fn the_pto_interval_is_smoothed_plus_four_rttvar_plus_max_ack_delay() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);

        assert_eq!(e.pto_interval(), ms(325), "100 + 200 + 25");
    }

    /// §13.1's roam clause: *"`min_rtt` is re-seeded from the first
    /// post-roam sample — it MUST be allowed to rise"*.
    ///
    /// Estimator-level only. §13.6's four fences are slice 7's and are
    /// deliberately not tested here (see `TESTS-5a-recovery.md` §3), but
    /// `reseed_min_rtt` is a pure function on a unit-testable struct and
    /// its arithmetic is assertable today: after a 100 ms sample fixes
    /// `min_rtt = 100`, a re-seed makes the *next* sample the new floor,
    /// so a 200 ms sample is no longer eligible for `ack_delay`
    /// subtraction (`200 ≥ 200 + 25` is false) and `smoothed` becomes
    /// `⅞·100 + ⅛·200 = 112.5 ms`.
    ///
    /// Mutation caught: a `reseed_min_rtt` that does nothing — the
    /// no-op is invisible in slice 5 because nothing calls it, and in
    /// slice 7 it pins the PTO floor of a new long path under an old
    /// short one's `min_rtt` for the connection's remaining life, which
    /// §13.1 names as the failure. Without the re-seed the same script
    /// gives `adjusted = 175` and `smoothed = 109.375 ms`.
    #[test]
    fn reseeding_lets_min_rtt_rise() {
        let mut e = RttEstimator::new();
        e.sample(ms(100), Duration::ZERO);
        e.reseed_min_rtt();
        e.sample(ms(200), ms(25));

        assert_eq!(
            e.smoothed_rtt(),
            us(112_500),
            "§13.1: the re-seeded min_rtt is 200, so the delay is refused"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §13.2 — ack-based loss detection
// ═══════════════════════════════════════════════════════════════════════

mod loss {
    use super::*;

    /// Four packets in one burst, the fourth acknowledged alone.
    ///
    /// The ACK arrives 100 ms after the burst, so the first sample is
    /// 100 ms and `loss_delay = 9/8 · 100 = 112.5 ms`. Every packet's age
    /// is 100 ms — inside the time threshold — so **only** the packet
    /// threshold can declare anything, which is what makes this a test of
    /// the threshold and not of the clock.
    fn burst_of_four() -> (Recovery, StreamRef, Instant) {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        for (c, size) in [(0u64, 1000u64), (1, 1100), (2, 1200), (3, 1300)] {
            rec.on_sent(sent(c, t, size, r));
        }
        (rec, r, t)
    }

    /// §13.2: *"`K_PACKET_THRESHOLD` = 3 or more counters below the largest
    /// acknowledged"*.
    ///
    /// Counter 0 is exactly 3 below the acknowledged 3 and is lost;
    /// counters 1 and 2 are 2 and 1 below and survive.
    ///
    /// Mutation caught: a threshold of 2 also loses counter 1, and a
    /// threshold of 4 loses nothing. Both are caught by asserting the lost
    /// set **by value** rather than by count — `lost.len() == 1` would pass
    /// a build that lost the wrong packet. The survivors are asserted
    /// through `bytes_in_flight`, which is the quantity the admission gate
    /// reads, so a build that removes an entry without crediting its size
    /// back fails here too.
    #[test]
    fn the_packet_threshold_declares_exactly_three_counters_below_the_largest() {
        let (mut rec, r, t) = burst_of_four();

        let out = rec.on_ack(t + ms(100), &ack_of(&[3], 0), 3);

        assert_eq!(out.acked, vec![tag(r, 3)], "one packet newly acknowledged");
        assert_eq!(
            out.lost,
            vec![tag(r, 0)],
            "§13.2: 3 below the largest acked, and nothing else"
        );
        assert_eq!(
            rec.bytes_in_flight(),
            1100 + 1200,
            "counters 1 and 2 survive with their sizes intact"
        );
        assert_eq!(
            rec.loss_deadline(),
            Some(t + us(112_500)),
            "§13.2: the survivors arm the timer at time_sent + loss_delay"
        );
    }

    /// The other side of the same boundary (slice 1's lesson: `LEN` and
    /// `LEN-1` were tested and `LEN+1` was not).
    ///
    /// With counter 2 acknowledged, the deepest survivor is 2 below the
    /// largest, and **nothing** may be declared lost.
    ///
    /// Mutation caught: `K_PACKET_THRESHOLD` read as 2 — a build that
    /// retransmits one packet too eagerly on every reorder, wasting
    /// bandwidth and cutting the window for a loss that did not happen.
    /// The test above alone does not catch it; this one does.
    #[test]
    fn two_counters_below_the_largest_is_not_yet_lost() {
        let (mut rec, _r, t) = burst_of_four();

        let out = rec.on_ack(t + ms(100), &ack_of(&[2], 0), 3);

        assert!(
            out.lost.is_empty(),
            "§13.2: nothing is 3 or more below the largest acked"
        );
        assert!(
            out.congestion.is_none(),
            "§14.3: no loss, no congestion event"
        );
        assert_eq!(rec.bytes_in_flight(), 1000 + 1100 + 1300);
    }

    /// A transfer in progress: three old packets, then a fourth, then two
    /// more still outstanding when the ACK for the fourth arrives.
    ///
    /// Counters 0–2 are sent at `t`, counter 3 at `t + 400 ms`, counters 4
    /// and 5 at `t + 401 ms` — counters ascend with send time, as a real
    /// core's do. The ACK covers 1, 2 and 3 and arrives at `t + 500 ms`,
    /// so the sample is 100 ms and `loss_delay = 112.5 ms`. Counter 0 is
    /// 3 below the largest and is lost. **Nothing below `largest_acked`
    /// survives**, so §13.2's minimum is taken over an empty set.
    fn transfer_with_newer_packets_outstanding() -> (Recovery, StreamRef, Instant) {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        for c in 0u64..3 {
            rec.on_sent(sent(c, t, 1000 + c * 100, r));
        }
        rec.on_sent(sent(3, t + ms(400), 1300, r));
        rec.on_sent(sent(4, t + ms(401), 1400, r));
        rec.on_sent(sent(5, t + ms(401), 1500, r));
        (rec, r, t)
    }

    /// **[AMENDED 2026/08/15 — ruling 131]** §13.2's `Loss` timer arms from
    /// the minimum across *the survivors below `largest_acked`*, never
    /// across the whole sent map.
    ///
    /// Mutation caught: *"minimum across in-flight packets"* — the
    /// parenthetical as it read before ruling 131, and the one an
    /// RFC-9002 transcription reaches for because the whole map is the
    /// thing in hand. Here every entry below the largest acked is either
    /// acknowledged or lost, so the correct walk arms **nothing**, while
    /// the wide one arms at counter 4's `time_sent + loss_delay` —
    /// ruling 131's *"a timer that fires and declares nothing"*, verbatim.
    /// `loss_deadline() == None` is the assertion that separates them, and
    /// it separates them **only** in this state: with any survivor below
    /// the largest acked the two readings agree, which is why the fixture
    /// is built to leave none. Counters 4 and 5 are asserted still in
    /// flight so the `None` cannot be read as "the map is empty".
    #[test]
    fn packets_above_the_largest_acked_never_arm_the_loss_timer() {
        let (mut rec, r, t) = transfer_with_newer_packets_outstanding();

        let out = rec.on_ack(t + ms(500), &ack_of(&[1, 2, 3], 0), 5);

        assert_eq!(
            out.acked,
            vec![tag(r, 1), tag(r, 2), tag(r, 3)],
            "ascending counter order"
        );
        assert_eq!(out.lost, vec![tag(r, 0)]);
        assert_eq!(
            rec.bytes_in_flight(),
            1400 + 1500,
            "counters 4 and 5 are untouched — the walk does not judge them"
        );
        assert_eq!(
            rec.loss_deadline(),
            None,
            "ruling 131: no survivor below largest_acked, so nothing to arm"
        );
    }

    /// The same rule from the other direction: `on_loss_timeout` runs *the
    /// same walk*, so it must not judge packets above `largest_acked`
    /// either — however long it has been.
    ///
    /// Mutation caught: ruling 131's second failure mode — an
    /// implementation that ranges over the whole map *and acts on it*
    /// **declares recent packets lost**. Five seconds after the ACK,
    /// counters 4 and 5 are far older than any `loss_delay`, so a wide
    /// walk retransmits two packets that were never in doubt and cuts the
    /// window for them. The narrow walk returns an empty outcome. This
    /// state is reachable in the ordinary way — a `Loss` timer armed by an
    /// earlier walk, firing after the map has moved on.
    #[test]
    fn the_loss_timeout_walk_never_judges_packets_above_the_largest_acked() {
        let (mut rec, _r, t) = transfer_with_newer_packets_outstanding();
        let _ = rec.on_ack(t + ms(500), &ack_of(&[1, 2, 3], 0), 5);

        let out = rec.on_loss_timeout(t + ms(5_500));

        assert!(
            out.lost.is_empty(),
            "ruling 131: entries above largest_acked are not judged at all"
        );
        assert!(out.congestion.is_none());
        assert_eq!(rec.bytes_in_flight(), 1400 + 1500);
    }

    /// §13.2: the timer arms at the **minimum** over the survivors.
    ///
    /// Counter 0 goes at `t` and is lost by the packet threshold;
    /// counters 1 and 2 go at `t + 90 ms` and `t + 95 ms`, the largest at
    /// `t + 100 ms`, and the ACK lands at `t + 200 ms` — a 100 ms sample,
    /// `loss_delay = 112.5 ms`, so both survivors are inside the threshold
    /// (ages 110 ms and 105 ms) and their deadlines differ by 5 ms.
    ///
    /// Mutation caught: a build taking the **maximum**, or simply the last
    /// survivor the iteration visited — `t + 207.5 ms` instead of
    /// `t + 202.5 ms`. Either declares the earlier packet's loss 5 ms late
    /// on this path and proportionally later on a slower one; nothing
    /// fails, everything is slower, and no completion test can see it.
    /// Only exact equality separates them — `loss_deadline().is_some()`
    /// passes all three builds.
    #[test]
    fn the_loss_timer_arms_at_the_earliest_surviving_send_time() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1000, r));
        rec.on_sent(sent(1, t + ms(90), 1100, r));
        rec.on_sent(sent(2, t + ms(95), 1200, r));
        rec.on_sent(sent(3, t + ms(100), 1300, r));

        let out = rec.on_ack(t + ms(200), &ack_of(&[3], 0), 3);

        assert_eq!(out.lost, vec![tag(r, 0)], "3 below the largest acked");
        assert_eq!(
            rec.loss_deadline(),
            Some(t + ms(90) + us(112_500)),
            "§13.2: the minimum over survivors 1 and 2, not the maximum"
        );
    }

    /// The `>=` boundary, built so that the packet under test is exactly
    /// `loss_delay` old when the ACK arrives.
    ///
    /// Counter 1 is sent 12.5 ms after counter 0 and acknowledged 100 ms
    /// later, so the sample is 100 ms and `loss_delay = 112.5 ms` — and
    /// counter 0's age at that instant is exactly 112.5 ms. `younger_by`
    /// shifts **counter 0 alone**, leaving the sample and therefore the
    /// threshold untouched: the two sides of the boundary differ by 1 ns
    /// of *age* and by nothing else.
    ///
    /// It shifts counter 0 rather than the ACK's arrival for a reason
    /// worth keeping. Moving the ACK moves `latest_rtt`, which moves
    /// `loss_delay` by 9/8 of the same amount — so an under-side built by
    /// delivering the ACK 1 ns early is *still* over the threshold, and
    /// the test passes a build with either comparison. That version was
    /// written first and failed for exactly this reason.
    fn at_the_time_threshold(younger_by: Duration) -> (Recovery, StreamRef, Instant) {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t + younger_by, 1000, r));
        rec.on_sent(sent(1, t + us(12_500), 1100, r));
        (rec, r, t)
    }

    /// **[ruling 141, reversing ruling 139(f)]** §13.2's time threshold is
    /// **`>=`**: a packet exactly `loss_delay` old **is** lost.
    ///
    /// This test was written the other way round, against ruling 139(f)'s
    /// strict `>`, and its author flagged it as "the first to revisit if
    /// the ruling moves". It moved. §13.2 arms the `Loss` timer at
    /// `time_sent + loss_delay`, so at the firing instant the age *equals*
    /// `loss_delay` — under `>` the walk the firing triggers declares
    /// nothing and re-arms at the same instant, and this core's
    /// `sync_recovery_timers` then announces a deadline at or before
    /// `now`, so the shell schedules an immediate wake and **the pair
    /// spins**. Not a wasted timer: a livelock, on every firing.
    ///
    /// Mutation caught: the strict `>`. It is the form ruling 139(f)
    /// mandated for one morning, and the form a careless reading of "more
    /// than `loss_delay`" produces.
    #[test]
    fn a_packet_exactly_loss_delay_old_is_lost() {
        let (mut rec, r, t) = at_the_time_threshold(Duration::ZERO);

        let out = rec.on_ack(t + us(112_500), &ack_of(&[1], 0), 1);

        assert_eq!(
            out.lost,
            vec![tag(r, 0)],
            "ruling 141: at exactly `loss_delay` the packet is lost, or the \
             `Loss` timer fires on nothing"
        );
        assert_eq!(rec.bytes_in_flight(), 0);
    }

    /// The other side of the boundary, one nanosecond **under**.
    ///
    /// Under ruling 141 the "at exactly the threshold" case is now the
    /// *lost* side, so the pair only pins the boundary if this one sits a
    /// tick below it — slice 1's lesson, where `LEN` and `LEN-1` were
    /// tested and `LEN+1` was not.
    ///
    /// Mutation caught: two builds at once. A build that never applies the
    /// time threshold at all fails the test above (every straggler then
    /// waits for the packet threshold or the PTO, so a tail of fewer than
    /// three packets is recovered a full PTO late). A build that applies
    /// it with an off-by-one slack fails this one.
    #[test]
    fn a_packet_one_nanosecond_younger_than_loss_delay_is_not_yet_lost() {
        let (mut rec, _r, t) = at_the_time_threshold(Duration::from_nanos(1));

        let out = rec.on_ack(t + us(112_500), &ack_of(&[1], 0), 1);

        assert!(
            out.lost.is_empty(),
            "ruling 141: `>=`, so one tick under the threshold is not lost"
        );
        assert_eq!(rec.bytes_in_flight(), 1000, "counter 0 survives");
    }

    /// §13.2's `loss_time` is *"recomputed from scratch on every walk"*
    /// (`CONTRACT-5a.md` §4.2).
    ///
    /// The first ACK arms the timer for counters 1 and 2; the second
    /// acknowledges both, leaving nothing below the largest acked.
    ///
    /// Mutation caught: a build that folds the new minimum into the old
    /// with `loss_time = min(loss_time, …)` and never clears it. The stale
    /// deadline outlives the packet that justified it, fires, declares
    /// nothing, and — because the walk that follows re-arms from the same
    /// stale value — keeps firing. `loss_deadline() == None` separates it;
    /// `<= ` or `>=` against the old value does not.
    #[test]
    fn the_loss_deadline_is_recomputed_rather_than_accumulated() {
        let (mut rec, r, t) = burst_of_four();
        let _ = rec.on_ack(t + ms(100), &ack_of(&[3], 0), 3);
        assert!(
            rec.loss_deadline().is_some(),
            "precondition: counters 1 and 2 armed it"
        );

        let out = rec.on_ack(t + ms(120), &ack_of(&[1, 2, 3], 0), 3);

        assert_eq!(out.acked, vec![tag(r, 1), tag(r, 2)], "1 and 2 are new");
        assert_eq!(
            rec.loss_deadline(),
            None,
            "§13.2: nothing below largest_acked survives, so nothing is armed"
        );
        assert!(rec.is_empty(), "the map is empty");
        assert_eq!(rec.bytes_in_flight(), 0);
    }

    /// §13.2's timer firing declares the survivors the ACK walk left.
    ///
    /// Fired one nanosecond past the armed deadline, because ruling 139's
    /// `>` is strict at the deadline itself — see
    /// `the_loss_timer_firing_at_its_own_deadline_declares_nothing`.
    ///
    /// Mutation caught: an `on_loss_timeout` that returns
    /// `AckOutcome::default()` — the stub an implementer leaves when the
    /// ACK path is wired first. Every transfer still completes, because
    /// the PTO eventually retransmits, so only a test that asserts the
    /// **frames** come back at the loss timer sees it. The congestion
    /// event is asserted too: a loss declared by the timer is a congestion
    /// event exactly as one declared by an ACK is (§14.3 says *"any loss
    /// of an ack-eliciting packet"*).
    #[test]
    fn the_loss_timeout_declares_the_survivors_the_ack_walk_left() {
        let (mut rec, r, t) = burst_of_four();
        let _ = rec.on_ack(t + ms(100), &ack_of(&[3], 0), 3);
        let deadline = rec.loss_deadline().expect("armed by counters 1 and 2");

        let out = rec.on_loss_timeout(deadline + Duration::from_nanos(1));

        assert_eq!(out.lost, vec![tag(r, 1), tag(r, 2)], "ascending order");
        assert_eq!(rec.bytes_in_flight(), 0);
        assert_eq!(rec.loss_deadline(), None, "nothing left to arm");
        let ev = out.congestion.expect("§14.3: a loss episode");
        assert_eq!(ev.lost_bytes, 1100 + 1200, "both packets, summed");
        assert_eq!(ev.sent_time, t, "the earliest lost packet's send time");
        assert!(!ev.is_persistent);
    }

    /// The consequence of ruling 139's strict `>` meeting §13.2's arming
    /// rule, asserted rather than assumed.
    ///
    /// **The livelock guard — [ruling 141].**
    ///
    /// The timer is armed at `time_sent + loss_delay`, so at exactly that
    /// instant the packet's age *equals* `loss_delay`. The firing must
    /// therefore **declare something**, or the walk it triggers finds
    /// nothing, re-arms at the same deadline, and this core's
    /// `sync_recovery_timers` announces a deadline at or before `now` —
    /// the shell schedules an immediate wake and **the pair spins**.
    ///
    /// This test was written asserting the opposite, under ruling 139(f)'s
    /// strict `>`, and its author reported the consequence as a finding
    /// rather than quietly living with it: *"ruling 131's 'arms a timer
    /// that fires and declares nothing' reached by a different route"*.
    /// It is the same defect ruling 131 had just fixed three paragraphs
    /// earlier in §13.2, arriving by a second route the maintainer did not
    /// see. Ruling 141 reversed the comparison; this is now the assertion
    /// that keeps it reversed.
    ///
    /// Mutation caught: the strict `>`. Under it this firing returns
    /// nothing and the connection livelocks its driver on any path with
    /// loss — a failure that no *functional* test sees, because every byte
    /// still arrives, and that presents as a busy loop rather than as a
    /// wrong answer.
    #[test]
    fn the_loss_timer_firing_at_its_own_deadline_declares_the_packet() {
        let (mut rec, _r, t) = burst_of_four();
        let _ = rec.on_ack(t + ms(100), &ack_of(&[3], 0), 3);
        let deadline = rec.loss_deadline().expect("armed");

        let out = rec.on_loss_timeout(deadline);

        assert!(
            !out.lost.is_empty(),
            "ruling 141: a `Loss` firing that declares nothing re-arms at its \
             own deadline and spins the driver"
        );
        assert_eq!(
            rec.loss_deadline(),
            None,
            "and nothing is left to arm for: a deadline surviving its own \
             firing is the livelock in its other form"
        );
    }

    /// §12.5, via `CONTRACT-5a.md` §2.2: an ACK that acknowledges nothing
    /// new is *"a total no-op"*.
    ///
    /// Mutation caught: a build that re-reports already-acknowledged
    /// packets. Its `acked` frames go back to the stream layer a second
    /// time, `ack_events` grow the congestion window for bytes that were
    /// counted once already, and a peer that simply re-sends its last ACK
    /// — which §12.4 permits and a lossy path guarantees — inflates the
    /// window without delivering anything. Asserting the outcome is empty
    /// **and** that the window-facing `ack_events` are empty is what
    /// separates it; `acked.is_empty()` alone would not.
    #[test]
    fn an_ack_that_acknowledges_nothing_new_is_a_total_no_op() {
        let (mut rec, _r, t) = burst_of_four();
        let first = rec.on_ack(t + ms(100), &ack_of(&[3], 0), 3);
        assert_eq!(first.acked.len(), 1, "precondition");
        let flight = rec.bytes_in_flight();

        let out = rec.on_ack(t + ms(150), &ack_of(&[3], 0), 3);

        assert!(out.acked.is_empty());
        assert!(out.lost.is_empty());
        assert!(out.ack_events.is_empty(), "§14: nothing to grow the window");
        assert!(out.congestion.is_none());
        assert_eq!(rec.bytes_in_flight(), flight, "the map did not move");
    }

    /// §12.5's *"ignore whole"*: an ACK whose `largest` exceeds the
    /// highest counter we have sealed.
    ///
    /// Mutation caught: a build that trusts the peer's `largest`. It sets
    /// `largest_acked` to a counter that does not exist, and **every**
    /// packet in the map is then at least 3 below it — so the next walk
    /// declares the entire flight lost, retransmits all of it and cuts the
    /// window, on one forged or corrupted frame. Asserting
    /// `bytes_in_flight` is unchanged is what catches the version that
    /// silently swallows the frame but still records `largest_acked`;
    /// asserting the outcome is empty alone would not.
    #[test]
    fn an_ack_above_the_highest_sealed_counter_is_ignored_whole() {
        let (mut rec, _r, t) = burst_of_four();

        let out = rec.on_ack(t + ms(100), &ack_of(&[9], 0), 3);

        assert!(out.acked.is_empty());
        assert!(out.lost.is_empty());
        assert!(out.congestion.is_none());
        assert_eq!(
            rec.bytes_in_flight(),
            1000 + 1100 + 1200 + 1300,
            "§12.5: the whole frame is ignored, including its largest"
        );
        assert_eq!(rec.loss_deadline(), None);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §13.1 — which ACK yields a sample (ruling 138)
// ═══════════════════════════════════════════════════════════════════════

mod sampling {
    use super::*;

    /// **[ruling 138]** *"'newly acknowledged' is load-bearing: if this
    /// ACK's `largest` was already acknowledged by an earlier ACK, there is
    /// **no** sample, even when the frame newly acknowledges other
    /// packets."*
    ///
    /// Three packets in one burst. The first ACK covers counter 2 alone
    /// and lands 100 ms later — a 100 ms sample. The second ACK, 500 ms
    /// after the burst, covers counters 1 **and** 2: counter 1 is newly
    /// acknowledged, counter 2 is not, and counter 2 is the `largest`. So
    /// the frame does real work and yields no sample.
    ///
    /// Mutation caught: a build that samples whenever *anything* is newly
    /// acknowledged, or that samples from `largest` unconditionally. It
    /// takes a 500 ms sample from a packet whose acknowledgement it
    /// already saw at 100 ms, giving `smoothed = ⅞·100 + ⅛·500 = 150 ms`.
    /// The estimate inflates on every duplicate-largest ACK, and a lossy
    /// path produces those constantly — the PTO grows, recovery slows, and
    /// nothing fails. `smoothed_rtt() == 100 ms` separates them exactly.
    /// The `acked` assertion is the other half of the pin: it proves the
    /// ACK was **not** simply ignored, which is the trivial way to pass
    /// the first assertion.
    #[test]
    fn an_ack_whose_largest_was_already_acknowledged_yields_no_sample() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        for c in 0u64..3 {
            rec.on_sent(sent(c, t, 1200, r));
        }

        let _ = rec.on_ack(t + ms(100), &ack_of(&[2], 0), 2);
        assert_eq!(
            rec.rtt().smoothed_rtt(),
            ms(100),
            "precondition: the first sample"
        );

        let out = rec.on_ack(t + ms(500), &ack_of(&[1, 2], 0), 2);

        assert_eq!(
            out.acked,
            vec![tag(r, 1)],
            "counter 1 *is* newly acknowledged — the frame is not ignored"
        );
        assert_eq!(
            rec.rtt().smoothed_rtt(),
            ms(100),
            "ruling 138: largest was already acknowledged, so no sample"
        );
    }

    /// §13.1's `latest_rtt`, which §13 never defines (PLAN-5 §8-H3): the
    /// arrival instant minus the send time **of the largest newly
    /// acknowledged packet**.
    ///
    /// Counter 0 goes at `t`, counter 1 at `t + 50 ms`, and one ACK at
    /// `t + 150 ms` covers both. The sample is 100 ms — measured from
    /// counter 1 — not 150 ms.
    ///
    /// Mutation caught: a build measuring from the *oldest* newly
    /// acknowledged packet, or from the first entry the walk happens to
    /// visit. Under a coalescing peer, or after any reordering, the
    /// oldest is arbitrarily older than the largest, so the estimate is
    /// inflated by the peer's ACK cadence rather than by the path. It
    /// gives 150 ms here.
    #[test]
    fn the_sample_measures_from_the_largest_newly_acknowledged_packet() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));
        rec.on_sent(sent(1, t + ms(50), 1200, r));

        let _ = rec.on_ack(t + ms(150), &ack_of(&[0, 1], 0), 1);

        assert_eq!(
            rec.rtt().smoothed_rtt(),
            ms(100),
            "§13.1 with RFC 9002 §5.1: measured from counter 1"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §13.3 — probe timeout
// ═══════════════════════════════════════════════════════════════════════

mod pto {
    use super::*;

    /// §13.3's interval before any sample, anchored at the send.
    ///
    /// Mutation caught: the `rttvar = 0` seed again, from the timer's side
    /// — `t + 358 ms` instead of `t + 1024 ms`. A first flight that is
    /// merely slow then probes three times before the genuine ACK
    /// arrives, and each probe is ack-eliciting, so the peer ACKs it and
    /// everything still works. Only the exact deadline shows it.
    #[test]
    fn the_first_pto_deadline_is_the_send_plus_one_thousand_and_twenty_four_ms() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));

        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO));
    }

    /// §13.3: *"The `Pto` timer is armed **only while** at least one
    /// ack-eliciting packet is in the sent map"*.
    ///
    /// Mutation caught: a build that arms the PTO from the anchor alone.
    /// §13.3 states the failure itself: *"an idle connection self-sustains
    /// a probe train — PTO fires, the bare PING is ack-eliciting, the peer
    /// ACKs, `pto_count` resets, the timer re-arms — at ~20 packets/s
    /// against the 10 s keepalive cadence"*. Nothing breaks: the
    /// connection stays up, every transfer completes, and the endpoint
    /// emits twenty packets a second forever. Asserting `None` **after**
    /// the map drains is the only thing that sees it; the `Some` before it
    /// is asserted so the `None` cannot be read as "never armed at all".
    #[test]
    fn the_pto_is_disarmed_when_the_sent_map_empties() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        assert_eq!(rec.pto_deadline(), None, "nothing sent, nothing armed");

        rec.on_sent(sent(0, t, 1200, r));
        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO), "armed");

        let _ = rec.on_ack(t + ms(100), &ack_of(&[0], 0), 0);

        assert!(rec.is_empty(), "precondition: the map drained");
        assert_eq!(
            rec.pto_deadline(),
            None,
            "§13.3: disarmed when the map empties"
        );
    }

    /// §13.3's anchor is *"the last ack-eliciting send"* — PLAN-5 §8-H6,
    /// which the spec leaves unstated and `CONTRACT-5a.md` §4.3 fixes as
    /// `deadline = last_ack_eliciting + …`.
    ///
    /// Mutation caught: anchoring at the **oldest unacknowledged** packet,
    /// which is the other natural reading of "the packet we are waiting
    /// for". It fires at `t + 1024 ms` here instead of `t + 1224 ms`, and
    /// on a busy connection it fires while packets sent microseconds ago
    /// are still in flight — a probe for a flight that was never idle.
    /// The two readings converge whenever only one packet is outstanding,
    /// which is why this test sends two.
    #[test]
    fn the_pto_anchors_at_the_last_ack_eliciting_send() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));
        rec.on_sent(sent(1, t + ms(200), 1200, r));

        assert_eq!(rec.pto_deadline(), Some(t + ms(200) + INITIAL_PTO));
    }

    /// §13.3: *"doubled per consecutive unanswered probe (`2^pto_count`)"*,
    /// with ruling 139's increment moment — **at the timer's firing**.
    ///
    /// **[Re-derived 2026/08/17 — ruling 254.]** The ladder is unchanged;
    /// what changed is where the cap sits relative to it. At
    /// `PTO_BACKOFF_CAP` = 2⁶ the third sample (8×) had five rungs of
    /// headroom, so it could only be explained by *"still doubling"*. At 2³
    /// that same sample **is** the cap, and a build that stopped doubling
    /// after 4× is indistinguishable from one that stopped because the cap
    /// bound — the assertion stops separating the two hypotheses while
    /// still passing, which is working rule 9's shape exactly.
    ///
    /// So the rungs are asserted as **two groups with different content**:
    /// the ones strictly below the cap (2×, 4×), which only doubling
    /// explains, and the pinned one (8×, and the firing after it still 8×),
    /// which only the cap explains. Three samples in a row would have
    /// asserted the first property twice and the second not at all.
    ///
    /// Mutation caught: a build that increments when the probe is *sent*
    /// rather than when the timer fires. The two differ by exactly one
    /// interval, so the train runs `1×, 1×, 2×, 4×` and probes one extra
    /// time at the shortest interval — invisible on a healthy path,
    /// visible as a doubled probe count on a black-holed one.
    ///
    /// Mutation caught: a build that adds rather than doubles
    /// (`1×, 2×, 3×`). It agrees at the first firing and diverges at the
    /// second, which is why the 4× rung is asserted and not just the 2×.
    ///
    /// Mutation caught: **no cap at all** — 16× at the fourth firing, which
    /// only the last assertion here separates from the conforming build.
    #[test]
    fn each_pto_firing_doubles_the_interval() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));

        // Strictly below the cap: doubling is the only explanation.
        rec.on_pto_timeout();
        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO * 2), "2×");
        rec.on_pto_timeout();
        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO * 4), "4×");

        // The cap's own rung, and then the rung that is not there: a build
        // still doubling reads 16× on the second of these.
        rec.on_pto_timeout();
        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO * 8), "8× — 2³");
        rec.on_pto_timeout();
        assert_eq!(
            rec.pto_deadline(),
            Some(t + INITIAL_PTO * 8),
            "§13.3: the fourth firing is pinned, not 16×"
        );
    }

    /// §13.3's `PTO_BACKOFF_CAP` = 2³, **stored as the multiplier 8**
    /// (**[RATIFIED 2026/08/17 — ruling 254]**; 2⁶ before it).
    ///
    /// `CONTRACT-5a.md` §4.3 names this *"the single most likely
    /// mechanical error in the whole slice"*: v0.1 writes
    /// `1u32 << self.pto_count.min(PTO_BACKOFF_CAP)` with its own
    /// `PTO_BACKOFF_CAP = 6`, and the same idiom with slither's constant
    /// shifts by the **multiplier** instead of the exponent.
    ///
    /// # The hazard sharpened when the cap fell, and this test carries it
    ///
    /// At 2⁶ the bad idiom was `1u32 << 64`: a panic in debug, undefined in
    /// release, and on `u64` an `Instant` addition that overflows. It could
    /// not be missed, and this test's old body was really asserting that
    /// *something detonated*.
    ///
    /// At 2³ it is `1u32 << 8` = **256** — a legal `u32`, no panic, no
    /// overflow, and a probe train running at 32× the ratified cadence with
    /// every packet still going out. Ruling 254 says so in terms: *"the
    /// compile-time pins are now load-bearing, not belt-and-braces."* So
    /// this test asserts the **value** rather than the survival, and does it
    /// past the eighth firing, where the two readings first differ by
    /// something an equality can see: 8× against 256×.
    ///
    /// Mutation caught (exponent read as multiplier): `INITIAL_PTO * 256`
    /// at the last assertion — 4.4 minutes between probes on a 25 s death
    /// clock, i.e. no probe train at all.
    /// Mutation caught (no cap): 16× at the fourth firing.
    /// Mutation caught (cap at the wrong rung, 2² or 2⁴): 4× or 16× at the
    /// third firing, which is asserted separately from the pinned rungs.
    #[test]
    fn the_pto_backoff_multiplier_caps_at_eight() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));

        // §13.3's `2^pto_count`: three firings reach 2³, the cap.
        for _ in 0..3 {
            rec.on_pto_timeout();
        }
        assert_eq!(rec.pto_deadline(), Some(t + INITIAL_PTO * 8), "2³");

        rec.on_pto_timeout();
        assert_eq!(
            rec.pto_deadline(),
            Some(t + INITIAL_PTO * 8),
            "§13.3: the cap holds at the fourth firing"
        );

        // Well past the cap — and, deliberately, past **eight** firings,
        // because that is where a `pto_count` saturated at the multiplier
        // rather than the exponent yields `1u32 << 8` = 256.
        for _ in 0..60 {
            rec.on_pto_timeout();
        }
        assert_eq!(
            rec.pto_deadline(),
            Some(t + INITIAL_PTO * 8),
            "64 firings: pinned at 8×, and specifically not the 256× that \
             `1u32 << pto_count` gives — legal, silent, and 32× too slow"
        );
    }

    /// §13.3: *"`pto_count` resets to 0 whenever **any** packet is newly
    /// acknowledged."*
    ///
    /// Two packets at `t`; three PTO firings take the multiplier to 8×;
    /// then counter 1 is acknowledged 100 ms after the send. That ACK is
    /// also the first RTT sample, so the base becomes
    /// `100 + max(4 · 50, 1) + 25 = 325 ms`, and counter 0 is still in
    /// flight so the timer stays armed — at `t + 325 ms`.
    ///
    /// Mutation caught: a build that resets only when *the probe's own*
    /// packet is acknowledged — the reading "consecutive unanswered
    /// probe" invites. After a recovery the connection keeps the 8×
    /// backoff, so the next real loss waits 2.6 s instead of 325 ms, and
    /// it decays only through further successful probes. Every transfer
    /// still completes. `t + 2600 ms` is what it gives here.
    #[test]
    fn pto_count_resets_when_any_packet_is_newly_acknowledged() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));
        rec.on_sent(sent(1, t, 1200, r));
        for _ in 0..3 {
            rec.on_pto_timeout();
        }
        assert_eq!(
            rec.pto_deadline(),
            Some(t + INITIAL_PTO * 8),
            "precondition: backed off"
        );

        let _ = rec.on_ack(t + ms(100), &ack_of(&[1], 0), 1);

        assert_eq!(
            rec.pto_deadline(),
            Some(t + ms(325)),
            "§13.3: pto_count back to 0, on the freshly sampled base"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §14.4 — persistent congestion, computed inside §13.2's walk
// ═══════════════════════════════════════════════════════════════════════

mod persistent_congestion {
    use super::*;

    /// A recovery with one 100 ms sample already taken, so
    /// `pto_interval()` is `100 + 200 + 25 = 325 ms` and §14.4's period is
    /// `3 × 325 = 975 ms` at the moment of the first ACK. The second ACK
    /// re-samples at the same 100 ms, which leaves `smoothed` at 100 ms
    /// and shrinks `rttvar` to 37.5 ms, so the period at walk time is
    /// `3 × 275 = 825 ms`. Every gap used below is 1000 ms, which exceeds
    /// both — the test does not depend on which instant the period is
    /// evaluated at.
    fn sampled() -> (Recovery, StreamRef, Instant) {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1200, r));
        let _ = rec.on_ack(t + ms(100), &ack_of(&[0], 0), 0);
        (rec, r, t + ms(100))
    }

    /// §14.4: two ack-eliciting packets more than `PTO × 3` apart, both
    /// lost, with no packet acknowledged between them.
    ///
    /// Counters 1 and 2 are 1000 ms apart and both fall to the time
    /// threshold when counter 3's ACK arrives; nothing lies between them.
    ///
    /// Mutation caught: a build with no §14.4 at all. Under a total
    /// blackhole it halves the window per episode instead of collapsing
    /// it, so the sender keeps a multi-packet window pointed at a path
    /// that is delivering nothing — the exact condition §14.4 exists to
    /// detect. It reports `is_persistent: false` here. The event's
    /// `sent_time` and `lost_bytes` are asserted in the same breath
    /// because a build that reports the *last* lost packet's send time
    /// escapes its own recovery fence on the next episode.
    #[test]
    fn two_far_apart_losses_with_nothing_acked_between_are_persistent() {
        let (mut rec, r, t1) = sampled();
        rec.on_sent(sent(1, t1, 1100, r));
        rec.on_sent(sent(2, t1 + ms(1000), 1200, r));
        rec.on_sent(sent(3, t1 + ms(2000), 1300, r));

        let out = rec.on_ack(t1 + ms(2100), &ack_of(&[3], 0), 3);

        assert_eq!(out.lost, vec![tag(r, 1), tag(r, 2)]);
        let ev = out.congestion.expect("§14.3: a loss episode");
        assert!(
            ev.is_persistent,
            "§14.4: 1000 ms apart, nothing acked between, and a prior sample exists"
        );
        assert_eq!(ev.sent_time, t1, "the earliest lost packet's send time");
        assert_eq!(ev.lost_bytes, 1100 + 1200);
    }

    /// §14.4's *"with **no packet acknowledged between them**"*.
    ///
    /// The same 1000 ms spread, but counter 2 — sent between the two lost
    /// packets — is acknowledged by the same ACK. The run is broken, so
    /// this is an ordinary loss episode.
    ///
    /// Mutation caught: a build that computes the spread over the lost set
    /// and forgets the acknowledgement test. That build collapses the
    /// window to 2 400 B on **any** pattern where two losses straddle a
    /// second of wall time with delivery in between — a connection that is
    /// working, on a path that is merely long or moderately lossy. It is
    /// the most damaging false positive in §14 and it is invisible to
    /// every completion test, because the transfer still finishes.
    /// `is_persistent == false` **with a congestion event still present**
    /// is the assertion: dropping the event entirely would also make the
    /// first assertion pass.
    #[test]
    fn a_run_broken_by_an_acknowledged_packet_is_not_persistent() {
        let (mut rec, r, t1) = sampled();
        rec.on_sent(sent(1, t1, 1100, r));
        rec.on_sent(sent(2, t1 + ms(500), 1200, r));
        rec.on_sent(sent(3, t1 + ms(1000), 1300, r));
        rec.on_sent(sent(4, t1 + ms(2000), 1400, r));

        let out = rec.on_ack(t1 + ms(2100), &ack_of(&[2, 4], 0), 4);

        assert_eq!(out.lost, vec![tag(r, 1), tag(r, 3)]);
        assert_eq!(out.acked, vec![tag(r, 2), tag(r, 4)]);
        let ev = out.congestion.expect("§14.3: still a loss episode");
        assert!(
            !ev.is_persistent,
            "§14.4: counter 2 was acknowledged between the two losses"
        );
        assert_eq!(ev.lost_bytes, 1100 + 1300);
    }

    /// §14.4: *"`persistent_period` evaluates the §13.3 PTO formula **with
    /// `pto_count = 0`**"*.
    ///
    /// Identical to the positive case above, except that three PTO firings
    /// have taken `pto_count` to 3 before the ACK arrives — which is the
    /// ordinary state during a blackhole, since the probes are what keep
    /// firing while nothing is acknowledged.
    ///
    /// Mutation caught: a build using the backed-off interval. §14.4 says
    /// what happens in terms: *"with the backoff included, the threshold
    /// would run up to 2³× too long and persistent congestion would never
    /// trigger under exactly the sustained loss it exists to detect"*
    /// (**[ruling 254]** — "2⁶×" until `PTO_BACKOFF_CAP` fell to 2³; the
    /// conclusion survives and the factor does not). At `pto_count = 3`,
    /// which is now the cap itself and so the **worst** case rather than a
    /// sample from the middle of the ladder, the period becomes
    /// `8 × 275 = 2200 ms`, the 1000 ms spread no longer clears it, and the
    /// collapse never happens. Note
    /// this test cannot fail a build that resets `pto_count` before
    /// computing the period, because such a build computes the correct
    /// period — an assertion a conforming build cannot fail.
    #[test]
    fn the_persistent_period_ignores_the_pto_backoff() {
        let (mut rec, r, t1) = sampled();
        rec.on_sent(sent(1, t1, 1100, r));
        rec.on_sent(sent(2, t1 + ms(1000), 1200, r));
        rec.on_sent(sent(3, t1 + ms(2000), 1300, r));
        for _ in 0..3 {
            rec.on_pto_timeout();
        }

        let out = rec.on_ack(t1 + ms(2100), &ack_of(&[3], 0), 3);

        let ev = out.congestion.expect("§14.3: a loss episode");
        assert!(
            ev.is_persistent,
            "§14.4: the period is a property of the path, not of the probe count"
        );
    }

    /// Losses close together are **not** persistent, however many there
    /// are.
    ///
    /// Three packets 10 ms apart, all lost to the time threshold when the
    /// fourth is acknowledged 500 ms later. The spread is 20 ms, far below
    /// the period.
    ///
    /// Mutation caught: a build that reads §14.4 as "several packets lost
    /// at once" and collapses on burst loss — the ordinary case a single
    /// congestion event is *for*. Also caught: a build that reports the
    /// congestion event per lost packet rather than once per episode, via
    /// `lost_bytes` — the sum of all three, not the last one's size. The
    /// `sent_time` assertion pins the earliest, which is what §14.3's
    /// recovery fence needs to be correct.
    #[test]
    fn a_burst_of_close_together_losses_is_one_ordinary_episode() {
        let t = t0();
        let r = tag_ref();
        let mut rec = Recovery::new();
        rec.on_sent(sent(0, t, 1000, r));
        rec.on_sent(sent(1, t + ms(10), 1100, r));
        rec.on_sent(sent(2, t + ms(20), 1200, r));
        rec.on_sent(sent(3, t + ms(500), 1300, r));

        let out = rec.on_ack(t + ms(600), &ack_of(&[3], 0), 3);

        assert_eq!(out.lost, vec![tag(r, 0), tag(r, 1), tag(r, 2)]);
        let ev = out.congestion.expect("§14.3: one event for the episode");
        assert!(
            !ev.is_persistent,
            "20 ms apart is not persistent congestion"
        );
        assert_eq!(
            ev.lost_bytes,
            1000 + 1100 + 1200,
            "§14.3: once per episode, over the whole lost set"
        );
        assert_eq!(ev.sent_time, t, "the earliest, not the latest");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §14.2 / §14.3 — NewReno
// ═══════════════════════════════════════════════════════════════════════

mod newreno {
    use super::*;

    /// §14.2's initial state.
    ///
    /// Mutation caught: `ssthresh` initialised to `INITIAL_WINDOW` rather
    /// than `u64::MAX`. Such a build starts in **congestion avoidance**,
    /// so the first flight grows by one MTU per RTT instead of doubling —
    /// a connection that is correct, complete, and an order of magnitude
    /// slower to reach the path's capacity. `window()` alone cannot see
    /// it; `ssthresh()` is `#[cfg(test)]` in §2.3 precisely so it can.
    #[test]
    fn a_new_controller_starts_at_the_initial_window_in_slow_start() {
        let c = NewReno::new();

        assert_eq!(c.window(), INITIAL_WINDOW, "§14.2: 12 000 B");
        assert_eq!(c.ssthresh(), u64::MAX, "§14.2: ssthresh starts at u64::MAX");
        assert_eq!(c.recovery_start(), None, "§14.3: nothing is fenced yet");
    }

    /// §14.2: *"**Slow start** (cwnd < ssthresh): cwnd grows by the bytes
    /// newly acknowledged."*
    ///
    /// The three acknowledgements are deliberately **not** MTU-sized.
    ///
    /// Mutation caught: a build that adds one `MAX_DATAGRAM` per
    /// acknowledgement instead of the bytes acknowledged. With 1200-byte
    /// acks the two are identical — which is what a test written against
    /// full packets would have missed — so the sizes here are 500, 700 and
    /// 1200, giving 14 400 for the correct build and 15 600 for the
    /// per-packet one. A build growing by half the bytes gives 13 200.
    #[test]
    fn slow_start_grows_by_exactly_the_bytes_acknowledged() {
        let t = t0();
        let mut c = NewReno::new();

        for (i, bytes) in [500u64, 700, 1200].into_iter().enumerate() {
            let at = t + ms(i as u64 + 1);
            c.on_sent(at, bytes);
            c.on_ack(at + ms(50), at, bytes, false);
        }

        assert_eq!(
            c.window(),
            INITIAL_WINDOW + 2400,
            "12 000 + 500 + 700 + 1200"
        );
    }

    /// A controller already cut once, so `cwnd == ssthresh == 6 000` and
    /// every subsequent acknowledgement runs §14.2's integer
    /// appropriate-byte-counting. `recovery_start` is `t`, so the
    /// acknowledgements below are all stamped strictly after it.
    fn in_congestion_avoidance(t: Instant) -> NewReno {
        let mut c = NewReno::new();
        c.on_congestion_event(t, t, false, 1200);
        assert_eq!(c.window(), 6_000, "precondition");
        assert_eq!(c.ssthresh(), 6_000, "precondition");
        c
    }

    /// §14.2's congestion avoidance, scripted so that **both** of its
    /// failure modes diverge from the correct answer at a named step.
    ///
    /// Hand-computed against `CONTRACT-5a.md` §4.5, `MAX_DATAGRAM` = 1200:
    ///
    /// | ack | accumulator | cwnd |
    /// |---|---|---|
    /// | 5 000 | 5 000 | 6 000 |
    /// | 5 000 | 10 000 → 4 000 | 7 200 |
    /// | 3 200 | 7 200 → 0 | 8 400 |
    /// | 20 000 | 20 000 → 11 600 → 2 000 | 10 800 |
    ///
    /// Mutation caught (remainder): a build that zeroes the accumulator
    /// after a crossing instead of subtracting `cwnd`. It diverges at step
    /// three — 7 200 instead of 8 400 — because the 3 200 it discarded was
    /// exactly what carried the accumulator over. It under-grows forever,
    /// by an amount that depends on the peer's ACK granularity, so the
    /// same connection is slower against a peer that ACKs more often.
    ///
    /// Mutation caught (`if` for `while`): diverges at step four — 9 600
    /// instead of 10 800 — because a 20 000-byte ack crosses the
    /// accumulator twice. `CONTRACT-5a.md` §4.5 names this one.
    ///
    /// Mutation caught (slow start never exits): step one alone gives
    /// 11 000. Asserting each step rather than only the total is what
    /// makes the failure say *which* rule broke.
    #[test]
    fn congestion_avoidance_adds_one_datagram_per_crossing_and_carries_the_remainder() {
        let t = t0();
        let mut c = in_congestion_avoidance(t);
        let sent_at = t + ms(1);

        c.on_ack(t + ms(10), sent_at, 5_000, false);
        assert_eq!(c.window(), 6_000, "accumulator 5 000, no crossing");

        c.on_ack(t + ms(20), sent_at, 5_000, false);
        assert_eq!(c.window(), 7_200, "crossed once; 4 000 carried");

        c.on_ack(t + ms(30), sent_at, 3_200, false);
        assert_eq!(c.window(), 8_400, "the carried 4 000 is what crosses here");

        c.on_ack(t + ms(40), sent_at, 20_000, false);
        assert_eq!(c.window(), 10_800, "one ack, two crossings");
    }

    /// §14.3's cut: `cwnd = max(cwnd × 0.5, MINIMUM_WINDOW)`,
    /// `ssthresh = cwnd`, recovery starts **at the event**.
    ///
    /// Mutation caught: `ssthresh` left at `u64::MAX` after the cut. That
    /// build re-enters slow start at half the window and doubles straight
    /// back into the loss it just took — the classic omission, and one
    /// that looks like fast recovery on a graph. `recovery_start` is
    /// asserted as `Some(now)` and not `Some(sent_time)`: §14.3 says the
    /// period starts at the event, and a build anchoring it at the lost
    /// packet's send time fences nothing, because every packet in flight
    /// was sent after it.
    #[test]
    fn a_congestion_event_halves_the_window_and_sets_ssthresh_to_it() {
        let t = t0();
        let mut c = NewReno::new();

        c.on_congestion_event(t + ms(100), t, false, 1200);

        assert_eq!(c.window(), 6_000);
        assert_eq!(c.ssthresh(), 6_000, "§14.3: ssthresh = the new cwnd");
        assert_eq!(
            c.recovery_start(),
            Some(t + ms(100)),
            "§14.3: the period starts at the event, not at the send"
        );
    }

    /// §14.3: *"Subsequent congestion events for packets **sent before**
    /// the recovery period started are ignored — one loss burst produces
    /// exactly one window cut."*
    ///
    /// Mutation caught: a build that cuts per congestion event. One burst
    /// of four lost packets takes 12 000 → 6 000 → 3 000 → 2 400 → 2 400
    /// in a single round trip, and the connection then crawls at the
    /// minimum window while the path is merely reordering. It still
    /// completes every transfer, which is why `assert_eq!(window(),
    /// 6_000)` — the exact value — is the pin and "the window shrank" is
    /// not. The boundary is `sent_time <= recovery_start`, so the second
    /// event is stamped **exactly at** the marker: the equal case is the
    /// one a `<` build gets wrong.
    #[test]
    fn a_second_event_for_a_packet_sent_before_the_recovery_period_does_not_cut_again() {
        let t = t0();
        let mut c = NewReno::new();
        c.on_congestion_event(t + ms(100), t, false, 1200);

        c.on_congestion_event(t + ms(110), t - ms(10), false, 1200);
        assert_eq!(c.window(), 6_000, "sent strictly before the marker");

        c.on_congestion_event(t + ms(120), t + ms(100), false, 1200);
        assert_eq!(
            c.window(),
            6_000,
            "§14.3's test is `sent_time <= recovery_start`: the equal case is fenced"
        );
    }

    /// The positive control for the rule above, and it is not optional: a
    /// build that ignores **every** congestion event after the first
    /// passes the previous test perfectly.
    ///
    /// Mutation caught: exactly that build — one cut per connection
    /// instead of one cut per episode. It never responds to a second,
    /// genuinely new loss episode, so it is the most aggressive possible
    /// sender after its first loss. Only an event for a packet sent
    /// **after** the marker separates it, and it must cut again to 3 000.
    #[test]
    fn a_new_episode_after_the_recovery_period_cuts_again() {
        let t = t0();
        let mut c = NewReno::new();
        c.on_congestion_event(t + ms(100), t, false, 1200);

        c.on_congestion_event(t + ms(300), t + ms(200), false, 1200);

        assert_eq!(c.window(), 3_000, "§14.3: a fresh episode, a fresh cut");
        assert_eq!(c.ssthresh(), 3_000);
        assert_eq!(c.recovery_start(), Some(t + ms(300)));
    }

    /// §14.2's `MINIMUM_WINDOW` floor, approached from above.
    ///
    /// Four episodes, each for a packet sent after the previous marker:
    /// 12 000 → 6 000 → 3 000 → 2 400 (not 1 500) → 2 400.
    ///
    /// Mutation caught: a build without the `max`. It halves to 1 500,
    /// then 750, then 375 — below one datagram, so the admission gate
    /// admits **nothing** and the connection can only make progress
    /// through the PTO probe's cwnd exemption, one packet per backed-off
    /// timeout. That is a live-lock that looks like a very slow network.
    #[test]
    fn the_window_never_falls_below_the_minimum() {
        let t = t0();
        let mut c = NewReno::new();

        let mut at = t;
        for expected in [6_000u64, 3_000, MINIMUM_WINDOW, MINIMUM_WINDOW] {
            c.on_congestion_event(at + ms(10), at + ms(5), false, 1200);
            assert_eq!(c.window(), expected);
            at += ms(20);
        }
    }

    /// §14.3's symmetric half: *"acknowledgments of packets sent before the
    /// recovery period started do not grow the window"* — *"the same
    /// `sent_time ≤ recovery_start` test gates both"*.
    ///
    /// Mutation caught: a build implementing only the event half — the
    /// overwhelmingly common omission, because RFC 9002 states the growth
    /// fence in §7.3.2 and the event fence in §7.3.1 and an implementer
    /// reading only the loss path finds one of them. §14.3 names the
    /// consequence: *"the pre-cut flight's ACKs keep inflating cwnd
    /// through the recovery they triggered"* — half a window of
    /// in-flight bytes, all sent before the cut, all arriving just after
    /// it, undoing the cut entirely. Asserting `window()` **unchanged**
    /// across both fenced acks is the pin; the third ack is the positive
    /// control, without which a controller that never grows also passes.
    #[test]
    fn acknowledgements_of_packets_sent_before_the_recovery_period_do_not_grow_it() {
        let t = t0();
        let mut c = in_congestion_avoidance(t);

        c.on_ack(t + ms(10), t - ms(50), 6_000, false);
        assert_eq!(c.window(), 6_000, "sent strictly before the marker");

        c.on_ack(t + ms(20), t, 6_000, false);
        assert_eq!(
            c.window(),
            6_000,
            "sent exactly at the marker: still fenced"
        );

        c.on_ack(t + ms(30), t + Duration::from_nanos(1), 6_000, false);
        assert_eq!(
            c.window(),
            7_200,
            "one nanosecond after the marker, the ack grows the window"
        );
    }

    /// §14.5's `app_limited`: *"when it is set the controller does not grow
    /// the window on that acknowledgment — idle connections earn no
    /// phantom window."*
    ///
    /// Mutation caught: a build that never consults the flag. §14.5 states
    /// the hazard: *"the dangerous direction is a window that grows while
    /// the application is idle and later discharges as a burst."* A
    /// request/response connection sending 100 B a second would otherwise
    /// accumulate an unbounded window against a path it has never
    /// measured, and spend it all at once the moment the application has
    /// something to say. The second acknowledgement is the positive
    /// control: without it, a controller that never grows at all passes.
    #[test]
    fn an_app_limited_acknowledgement_does_not_grow_the_window() {
        let t = t0();
        let mut c = NewReno::new();

        c.on_ack(t + ms(10), t, 1200, true);
        assert_eq!(c.window(), INITIAL_WINDOW, "§14.5: app-limited, no growth");

        c.on_ack(t + ms(20), t, 1200, false);
        assert_eq!(c.window(), INITIAL_WINDOW + 1200, "not app-limited, growth");
    }

    /// §14.4's collapse: *"the controller collapses: `cwnd =
    /// MINIMUM_WINDOW`"* — not the halving §14.3 would otherwise apply.
    ///
    /// Mutation caught: a build that passes `is_persistent` through and
    /// never reads it. From 12 000 it halves to 6 000, so a path that has
    /// delivered nothing for several PTOs is still probed with five
    /// datagrams at a time. `ssthresh` is asserted at 6 000 rather than
    /// 2 400 because §14.4 collapses the *window* and says slow start
    /// *"effectively restarts"* — with `ssthresh` at the halved value the
    /// controller does exactly that, and a build that also drags `ssthresh`
    /// to the minimum leaves the connection in congestion avoidance
    /// forever, growing one MTU per RTT from 2 400 B.
    #[test]
    fn a_persistent_congestion_event_collapses_the_window_to_the_minimum() {
        let t = t0();
        let mut c = NewReno::new();

        c.on_congestion_event(t + ms(100), t, true, 2400);

        assert_eq!(c.window(), MINIMUM_WINDOW, "§14.4: collapsed, not halved");
        assert_eq!(c.ssthresh(), 6_000, "§14.3's cut still sets ssthresh");
    }

    /// **[ruling 139]** *"Persistent congestion does **not** clear
    /// `recovery_start`."*
    ///
    /// Mutation caught: RFC 9002 §7.6.2's behaviour, which additionally
    /// clears the recovery marker — the reading PLAN-5 §8-H7 raised and
    /// ruling 139 closed the other way. A build that clears it un-fences
    /// the entire pre-collapse flight, so the acknowledgements still
    /// arriving from the blackholed path grow the window straight back out
    /// of the collapse that was just declared. Both halves are asserted:
    /// the marker survives, **and** an acknowledgement of a packet sent
    /// before it does not grow the window — the second is the observable
    /// the first exists for, and a build could keep the marker and forget
    /// to consult it.
    #[test]
    fn persistent_congestion_does_not_clear_the_recovery_period() {
        let t = t0();
        let mut c = NewReno::new();
        c.on_congestion_event(t + ms(100), t, true, 2400);

        assert_eq!(
            c.recovery_start(),
            Some(t + ms(100)),
            "ruling 139: the marker is not cleared"
        );

        c.on_ack(t + ms(110), t + ms(50), 6_000, false);
        assert_eq!(
            c.window(),
            MINIMUM_WINDOW,
            "the pre-collapse flight is still fenced from growing it"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §14.5 — the admission gate, the packet's size, and the probe
// ═══════════════════════════════════════════════════════════════════════
//
// These are the three properties that are *not* arithmetic, so they are
// driven through a real core. `Solo`'s core has received nothing in most
// of them, which means it owes no ACK and every packet it emits is
// ack-eliciting — the state in which `bytes_in_flight` and the sum of the
// emitted datagram lengths are the same number, and can therefore be
// asserted equal.

mod admission {
    use super::*;

    fn solo_with_uni(t: Instant) -> (Solo, StreamRef) {
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Uni).expect("§9.1: our own uni space");
        (s, r)
    }

    /// §14.5's gate is `bytes_in_flight + candidate_size <= cwnd`, and
    /// **ruling 136**'s `size` is the full datagram.
    ///
    /// With `INITIAL_WINDOW` = 12 000 and 1 200-byte datagrams, exactly ten
    /// fit and the eleventh does not: `10 800 + 1 200 = 12 000` is admitted
    /// and `12 000 + 1 200` is not. 24 000 bytes are written, so the
    /// eleventh packet is queued and waiting rather than absent.
    ///
    /// Mutation caught (`<` for `<=`): nine packets, not ten. §14.5 writes
    /// `≤` and a window sized as an exact multiple of the MTU makes the
    /// difference a whole packet — 10 % of the initial window, every
    /// connection, forever.
    ///
    /// Mutation caught (ruling 136, plaintext accounting): the packet
    /// **count** is still ten, because 1 170-byte units divide 12 000 the
    /// same way — which is exactly why the count alone is not the test.
    /// `bytes_in_flight() == 12 000` is: a build counting plaintext reads
    /// 11 700 and believes it has 300 B of room it does not have. The
    /// overshoot is ~30 B per packet, it never fails, and no functional
    /// test in the suite can see it. Ruling 136 says so in terms: *"a
    /// window derived in datagram units is spent in datagram units."*
    #[test]
    fn the_initial_window_admits_exactly_ten_full_datagrams() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 24_000));

        let d = drain(&mut s.conn);
        let ts = d.transmits();

        assert_eq!(ts.len(), 10, "§14.5: 10 × 1200 fills 12 000 exactly");
        for x in &ts {
            assert_eq!(x.data.len(), MAX_DATAGRAM, "§8.6: the packets are full");
        }
        assert_eq!(total_bytes(&d), INITIAL_WINDOW);
        assert_eq!(
            s.conn.bytes_in_flight(),
            INITIAL_WINDOW,
            "ruling 136: the datagram, not the plaintext"
        );
        assert_eq!(s.conn.congestion_window(), INITIAL_WINDOW);
    }

    /// The same rule on a single small packet, where the three candidate
    /// readings of *"size in bytes"* (PLAN-5 §8-H8) are furthest apart.
    ///
    /// Mutation caught: `size` recorded as the plaintext length, or as the
    /// ciphertext without the header. The equality against the datagram
    /// the core actually handed out is exact for all three readings' worth
    /// of difference; the second assertion states the magnitude — the
    /// 14-byte header and 16-byte tag — so a failure names the defect
    /// rather than a number.
    #[test]
    fn a_tracked_packets_size_is_the_whole_datagram() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 500));

        let d = drain(&mut s.conn);
        let ts = d.transmits();

        assert_eq!(ts.len(), 1, "500 B is one packet");
        assert_eq!(
            s.conn.bytes_in_flight(),
            ts[0].data.len() as u64,
            "ruling 136: DATA_HEADER_LEN + ciphertext + AEAD_TAG_LEN"
        );
        assert!(
            s.conn.bytes_in_flight() >= 500 + (DATA_HEADER_LEN + AEAD_TAG_LEN) as u64,
            "the header and tag are counted, not just the payload"
        );
    }

    /// §14.5 and §12.5: an acknowledged packet's bytes leave the flight.
    ///
    /// The RTT assertion rides along because it pins the whole path from a
    /// sealed ACK frame to §13.1's estimator through the core, which no
    /// unit test can reach: the packet went out at `t` and the ACK arrives
    /// at `t + 50 ms`.
    ///
    /// Mutation caught: a build that removes the map entry without
    /// crediting `bytes_in_flight` back, or that maintains the sum only on
    /// insertion. It gates itself shut after a few thousand packets and
    /// then makes progress only through the PTO probe's exemption — a
    /// connection that starts fast and degrades to one packet per timeout,
    /// which reads as a network problem. `== 0` is the assertion; `<
    /// cwnd` would pass the leaking build for a long time.
    #[test]
    fn acknowledging_the_flight_returns_bytes_in_flight_to_zero() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 500));
        let d = drain(&mut s.conn);
        let cs = counters_of(&d);
        assert_eq!(cs.len(), 1, "precondition");
        assert!(s.conn.bytes_in_flight() > 0, "precondition");

        let _ = s.deliver(t + ms(50), &ack_bytes(&ack_of(&cs, 0)));

        assert_eq!(s.conn.bytes_in_flight(), 0, "the flight is empty");
        assert_eq!(
            s.conn.smoothed_rtt(),
            ms(50),
            "§13.1: the first sample, measured through the core"
        );
    }

    /// The gate holds a backlog, and the backlog moves when the window
    /// reopens.
    ///
    /// Mutation caught: a build with no gate at all — caught by the
    /// ten-packet count above, but caught here too, from the other side: a
    /// build that gates the *first* pass and then forgets on the next
    /// mutating call emits the remaining eleven packets at `t + 1 ms`. And
    /// a build whose `bytes_in_flight` never falls (the leak above) never
    /// sends the backlog at all — `!after.transmits().is_empty()` is what
    /// separates a stalled connection from a working one.
    #[test]
    fn the_gate_holds_the_backlog_until_the_flight_is_acknowledged() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 24_000));
        let d = drain(&mut s.conn);
        assert_eq!(d.transmits().len(), 10, "precondition: the window is full");

        s.conn.handle_timeout(t + ms(1));
        let idle = drain(&mut s.conn);
        assert!(
            idle.transmits().is_empty(),
            "§14.5: no room, so nothing more is sealed"
        );

        let after = s.deliver(t + ms(50), &ack_bytes(&ack_of(&counters_of(&d), 0)));

        assert!(!after.transmits().is_empty(), "the window reopened");
        assert!(
            s.conn.bytes_in_flight() <= s.conn.congestion_window(),
            "§14.5's gate still holds after the reopen"
        );
    }

    /// §13.4: a PTO probe is *"exempt from the congestion admission gate"*
    /// — and ruling 43 / §17.5: it is **not** exempt from the accounting.
    ///
    /// The window is full to the byte, so nothing else can be sealed. The
    /// deadline is asserted first, which pins §13.3's arming through the
    /// core at the same time (a build seeding `rttvar = 0` arms at
    /// `t + 358 ms`).
    ///
    /// Mutation caught (the exemption missing): the probe is refused, the
    /// map never drains, the PTO re-arms with a doubled interval and
    /// refuses again — a black-holed path with a full window is
    /// **permanently** stuck, and `DEAD_TIMEOUT` eventually kills a
    /// connection that could have recovered. §14.5 states the reason for
    /// the exemption in exactly these terms. No completion test on a
    /// *healing* path detects it, because a healing path drains the window
    /// by acknowledgement.
    ///
    /// Mutation caught (exempt from accounting too): `bytes_in_flight`
    /// unchanged across the probe. §17.5: *"had the exempt probes been left
    /// untracked, the sent map would have been bounded by cwnd only in the
    /// sense that it did not contain the packets it was missing."* Such a
    /// probe is invisible to loss recovery, so its own loss is never
    /// detected and the frames it carried are never retransmitted.
    ///
    /// The count is asserted as **one**: §13.4's *"one ack-eliciting
    /// packet"* is a deliberate simplification of RFC 9002 §6.2.4's two,
    /// and a build that flushes the whole backlog on a PTO firing sends
    /// eleven — un-gating the connection completely, since a PTO always
    /// eventually fires.
    #[test]
    fn a_pto_probe_is_sent_with_the_window_full_and_is_counted_in_flight() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 24_000));
        let d = drain(&mut s.conn);
        assert_eq!(d.transmits().len(), 10, "precondition");
        let full = s.conn.bytes_in_flight();
        assert_eq!(full, INITIAL_WINDOW, "precondition: not one byte of room");
        assert_eq!(
            d.deadline,
            Some(t + INITIAL_PTO),
            "§13.3 through the core: the PTO is armed at the send"
        );

        s.conn.handle_timeout(t + INITIAL_PTO);
        let probe = drain(&mut s.conn);
        let ts = probe.transmits();

        assert_eq!(ts.len(), 1, "§13.4: one ack-eliciting packet per firing");
        assert_eq!(
            s.conn.bytes_in_flight(),
            full + ts[0].data.len() as u64,
            "ruling 43: exempt from admission, never from accounting"
        );
    }

    /// §14.5's `app_limited`, with ruling 139's moment: *"stamped on the
    /// packet that emptied the queue with headroom left."*
    ///
    /// One 500-byte write is one packet, and that packet is the one that
    /// emptied the queue while 11 400 B of window remained. Its
    /// acknowledgement must not grow the window.
    ///
    /// Mutation caught: a build that never sets the flag. §14.5 names the
    /// hazard — *"idle connections earn no phantom window"* — and this is
    /// its smallest instance: a connection that sends 500 B a second would
    /// otherwise grow its window by 500 B a second against a path it has
    /// not measured, and discharge the accumulation as a burst the moment
    /// the application has something to say. The window here must be
    /// **exactly** unchanged; `<=` would pass a build that grows by any
    /// amount less than the whole packet.
    #[test]
    fn a_packet_that_emptied_the_queue_earns_no_window_growth() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 500));
        let d = drain(&mut s.conn);
        assert_eq!(d.transmits().len(), 1, "precondition");

        let _ = s.deliver(t + ms(50), &ack_bytes(&ack_of(&counters_of(&d), 0)));

        assert_eq!(
            s.conn.congestion_window(),
            INITIAL_WINDOW,
            "§14.5: the sender ran out of data, not of window"
        );
    }

    /// The positive control for the test above, and it is what makes the
    /// pair a pin rather than a pair of one-sided claims.
    ///
    /// Ten packets go out under a window that is full to the byte: the
    /// sender stopped because the **window** closed, not because it ran
    /// out of data — 12 000 B are still queued — so not one of them is
    /// app-limited and slow start adds every acknowledged byte.
    ///
    /// Mutation caught: a build that stamps `app_limited` unconditionally,
    /// or that derives it from "the send queue is empty *now*" at ACK time
    /// rather than from the flag recorded at send time. §14.5 is explicit
    /// that the flag is *"recorded at send time by us"* precisely so ACK
    /// timing cannot move it — a peer that delays its ACK until our queue
    /// happens to be empty could otherwise freeze our window at 12 000 B
    /// for the life of the connection. Such a build reads 12 000 here; the
    /// correct one reads 24 000.
    #[test]
    fn a_window_limited_flight_grows_the_window_by_every_byte_it_carried() {
        let t = t0();
        let (mut s, r) = solo_with_uni(t);
        write_all(&mut s.conn, t, r, &ramp(0, 24_000));
        let d = drain(&mut s.conn);
        assert_eq!(d.transmits().len(), 10, "precondition");

        let _ = s.deliver(t + ms(50), &ack_bytes(&ack_of(&counters_of(&d), 0)));

        assert_eq!(
            s.conn.congestion_window(),
            INITIAL_WINDOW * 2,
            "§14.2 slow start: cwnd grows by the 12 000 B acknowledged"
        );
    }

    /// §13.5: *"Non-ack-eliciting packets are never inserted"*, and §14.5:
    /// pure ACKs *"are never tracked in flight and never gated"*.
    ///
    /// Two ack-eliciting packets arrive and nothing is read, so the core
    /// owes an ACK by §12.4's every-second rule and has no data of its own
    /// to send. Whatever it emits is therefore a pure-ACK packet.
    ///
    /// Mutation caught: a build that records **every** sealed packet in the
    /// sent map — the natural shape if the insertion is written at the
    /// seal rather than gated on `packet_is_ack_eliciting`. The pure ACK
    /// then sits in the map forever: the peer will never acknowledge it,
    /// because it is not ack-eliciting, so it is eventually declared lost,
    /// cuts the congestion window for a loss that did not happen, and —
    /// since the map is never empty — the `Pto` timer is never disarmed
    /// and §13.3's self-sustaining probe train starts. An idle,
    /// receive-only connection is where this bites, and nothing else in
    /// the suite looks there.
    #[test]
    fn a_pure_ack_packet_is_never_tracked_in_flight() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver_stream_bytes(t, Solo::peer_uni(0), 0, 2048, false);

        assert!(
            !d.transmits().is_empty(),
            "§12.4: the second ack-eliciting packet owes an ACK now"
        );
        assert_eq!(
            s.conn.bytes_in_flight(),
            0,
            "§13.5: only ack-eliciting packets enter the map"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 113 — the FIN is carried, never inferred
// ═══════════════════════════════════════════════════════════════════════
//
// PLAN-5 §6.2's pair, from both directions. The degenerate build is the
// one `mod.rs`'s deleted `frame_carried_fin` implemented:
// `streams.final_size(r) == Some(range.end)`. It is exact for every frame
// this implementation emits *today* — the FIN rides the frame that ends
// the stream — and wrong in general, because §8.7 lets a retransmission
// split, merge or coalesce ranges freely. **Inference passes B and fails
// A**, in both the acknowledgement and the loss direction.

mod fin {
    use super::*;

    /// 500 bytes written and finished on a local uni stream, the FIN
    /// already sealed. `final_size` is 500, so a range of `0..500` "ends at
    /// the final size" and the inference cannot tell the two cases apart.
    fn finished_stream(t: Instant) -> (Solo, StreamRef) {
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Uni).expect("§9.1: our own uni space");
        write_all(&mut s.conn, t, r, &ramp(0, 500));
        s.conn
            .finish(t, r)
            .expect("§9.5: finish on a live send half");
        let _ = drain(&mut s.conn);
        (s, r)
    }

    fn finished_events(d: &Drained, r: StreamRef) -> usize {
        d.count_events(|e| matches!(e, ConnEvent::StreamFinished { r: q } if *q == r))
    }

    /// **T-FIN-A.** A retransmission re-framed to end at the final size,
    /// **without** the FIN, is acknowledged. `fin_acked` must stay false.
    ///
    /// Mutation caught: the inference. It sets `fin_acked`, `is_terminal()`
    /// becomes true, the send half is freed and §9.7's `DataRecvd` is
    /// announced — while the FIN is still unacknowledged and may still be
    /// in flight or lost. The application is told its stream was delivered
    /// when the peer has not been told the stream ended, so the peer's
    /// reader never sees EOF. `mod.rs:517–525` records exactly this: *"the
    /// inference would have silently set `fin_acked` on a re-framed
    /// retransmission and driven the send half to `DataRecvd` early."*
    /// This is the highest-value test in the file and the only one the
    /// degenerate build fails.
    #[test]
    fn a_range_ending_at_the_final_size_without_the_fin_does_not_finish_the_stream() {
        let t = t0();
        let (mut s, r) = finished_stream(t);

        s.conn.on_ack_range(t + ms(10), r, 0..500, false);
        let d = drain(&mut s.conn);

        assert_eq!(
            finished_events(&d, r),
            0,
            "ruling 113: the FIN is carried, and this frame did not carry it"
        );
        assert!(
            s.conn.stream_id(r).is_some(),
            "§9.7: not fully closed, so the entry survives"
        );
    }

    /// **T-FIN-B.** The packet that *did* carry the FIN is acknowledged.
    ///
    /// Mutation caught: a build that ignores the `fin` argument
    /// altogether and never sets `fin_acked`. The stream never reaches
    /// `DataRecvd`, `SendStream::acked()` never completes, and — because
    /// §9.2's watermark advances only on full closure — the index is never
    /// freed, so a long-lived connection leaks one table entry per stream.
    /// Without this test T-FIN-A alone is passed by that build.
    #[test]
    fn the_range_that_carried_the_fin_finishes_the_stream() {
        let t = t0();
        let (mut s, r) = finished_stream(t);

        s.conn.on_ack_range(t + ms(10), r, 0..500, true);
        let d = drain(&mut s.conn);

        assert_eq!(finished_events(&d, r), 1, "§9.7: DataRecvd, once");
        assert_eq!(
            s.conn.stream_id(r),
            None,
            "§9.2: fully closed, freed, and the watermark advanced"
        );
    }

    /// The same defect from the **loss** side, where it is visible on the
    /// wire rather than in an event.
    ///
    /// `CONTRACT-5a.md` §5: *"`SendHalf::on_lost_range(range, fin)` clears
    /// `fin_sent` when `fin` is true."*
    ///
    /// Mutation caught: the inference again. A lost range that merely ends
    /// at the final size clears `fin_sent`, so the FIN is re-sent on the
    /// retransmission — and, because every retransmission of the tail also
    /// ends at the final size, it is re-sent **every** time that tail is
    /// lost. `CONTRACT-5a.md` §5 calls it *"the FIN is re-sent forever"*.
    /// Asserting the *absence* of a FIN-bearing frame is what separates it;
    /// asserting that the range came back does not.
    #[test]
    fn a_lost_range_without_the_fin_does_not_resend_the_fin() {
        let t = t0();
        let (mut s, r) = finished_stream(t);

        s.conn.on_lost_range(t + ms(10), r, 0..500, false);
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);

        assert!(
            frames
                .iter()
                .any(|f| matches!(f, Wire::Stream { fin: false, .. })),
            "§8.7 `ranges`: the lost range returns to the pending set"
        );
        assert!(
            !frames
                .iter()
                .any(|f| matches!(f, Wire::Stream { fin: true, .. })),
            "ruling 113: this frame did not carry the FIN, so nothing re-sends it"
        );
    }

    /// The loss side's positive control.
    ///
    /// Mutation caught: a build that never clears `fin_sent` — the
    /// opposite error, and the worse one. `CONTRACT-5a.md` §5: *"the packet
    /// that really carried it does not clear the flag and the FIN is never
    /// re-sent, hanging the peer's reader at EOF."* The connection stays
    /// up, the data all arrives, and the peer's `read()` never returns
    /// `None`. Only re-sending the FIN-bearing range shows it.
    #[test]
    fn a_lost_range_that_carried_the_fin_resends_it() {
        let t = t0();
        let (mut s, r) = finished_stream(t);

        s.conn.on_lost_range(t + ms(10), r, 0..500, true);
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);

        assert!(
            frames
                .iter()
                .any(|f| matches!(f, Wire::Stream { fin: true, .. })),
            "§8.7: the FIN rides the retransmission of the range that carried it"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// PLAN-5 §6.4 — the watermarks acknowledgement made reachable
// ═══════════════════════════════════════════════════════════════════════
//
// Slice 4a could not close a **locally-opened** stream at all: §9.7 frees
// the send half only when the FIN is acknowledged, and nothing
// acknowledged anything until now.

mod watermark {
    use super::*;

    /// §10.4's scope rule, from RFC 9000 §4.6: *"closing streams we opened
    /// must not inflate the peer's allowance."*
    ///
    /// Eight streams — `STREAMS_CREDIT_BATCH`, so a build that grants on
    /// every closure has enough grants to cross the batching threshold and
    /// actually emit the frame — are opened, written, finished and
    /// acknowledged. All eight fully close, and the peer earns nothing.
    ///
    /// Mutation caught: a build without `streams.rs`'s `if !local` test,
    /// which grants MAX_STREAMS on **any** full closure. That inflates the
    /// peer's stream allowance by one for every stream *we* opened, so a
    /// connection that opens ten thousand outbound streams hands the peer
    /// ten thousand extra inbound slots — the exact resource inversion
    /// §10.4 exists to prevent, and one that no test can see until a local
    /// stream can be closed at all. The `StreamFinished` count is asserted
    /// too: without it, a build that never closes anything passes by
    /// emitting nothing.
    #[test]
    fn acknowledging_our_own_streams_closure_grants_the_peer_nothing() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let mut refs = Vec::new();
        for _ in 0..STREAMS_CREDIT_BATCH {
            let r = s.conn.open(Dir::Uni).expect("§9.1: our own uni space");
            write_all(&mut s.conn, t, r, &ramp(0, 100));
            s.conn.finish(t, r).expect("§9.5");
            refs.push(r);
        }
        let _ = drain(&mut s.conn);

        let mut finished = 0usize;
        for (i, r) in refs.iter().enumerate() {
            s.conn.on_ack_range(t + ms(10 + i as u64), *r, 0..100, true);
            let d = drain(&mut s.conn);
            finished += d.count_events(|e| matches!(e, ConnEvent::StreamFinished { .. }));
            let frames = s.drain_frames(&d);
            assert!(
                !frames.iter().any(|f| matches!(f, Wire::MaxStreamsUni(_))),
                "§10.4: our own stream's closure grants the peer nothing"
            );
        }

        assert_eq!(
            finished, STREAMS_CREDIT_BATCH as usize,
            "all eight fully closed — otherwise the assertion above is vacuous"
        );
    }

    /// The positive control: a **peer-opened** stream's closure *does* earn
    /// the peer credit, so the test above is a statement about `local` and
    /// not about MAX_STREAMS never being emitted.
    ///
    /// Mutation caught: a build that grants for neither — it passes the
    /// previous test perfectly and starves the peer of stream slots
    /// instead, which stalls any peer that opens streams in a loop once
    /// `INITIAL_MAX_STREAMS_UNI` is exhausted.
    ///
    /// **Note for the integrator:** this is one of the two tests in this
    /// file that decodes packets from a core that has *received* something,
    /// so it depends on `testfix::parse_frames` gaining an ACK arm — see
    /// `TESTS-5a-recovery.md` §2, conflict C3.
    #[test]
    fn retiring_peer_opened_streams_does_grant_the_peer_credit() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        for i in 0..STREAMS_CREDIT_BATCH {
            let f = stream_frame(Solo::peer_uni(i), 0, &ramp(0, 10), true);
            let _ = s.deliver(t, &f);
        }
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(
            claimed.len(),
            STREAMS_CREDIT_BATCH as usize,
            "precondition: eight peer-opened streams"
        );

        for (_, r) in &claimed {
            abandon_recv(&mut s.conn, t + ms(1), *r);
        }
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);

        assert!(
            frames.iter().any(|f| matches!(f, Wire::MaxStreamsUni(_))),
            "§10.4: a peer-opened stream's closure does earn the peer credit"
        );
    }

    /// §9.2 and §8.4: an index we opened and have since **fully closed** is
    /// *"processed as acknowledged, never re-opened"*.
    ///
    /// A local bidi stream needs both halves gone: the send half is freed
    /// by the acknowledged FIN, the receive half by abandonment. Only then
    /// does §9.2's watermark cover the index — which is PLAN-5 §6.4's item
    /// 3, *"a locally-opened watermark advancing at all"*, and it is
    /// asserted from the side that separates it rather than by reading the
    /// watermark, which is private.
    ///
    /// Mutation caught: a build that treats the index as unknown and
    /// **re-opens** it on the peer's frame. `accept(Dir::Bi)` then hands
    /// the application a stream in *our own* space — a stream we opened,
    /// closed, and are now told is incoming. §8.4's *"never re-opened"* is
    /// the rule, and a claimable stream is the observable that separates
    /// the two builds; `assert_alive` alone does not, because both builds
    /// may leave the connection up.
    #[test]
    fn a_stream_frame_naming_a_local_index_we_have_closed_is_a_no_op() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Bi).expect("§9.1: our own bidi space");
        write_all(&mut s.conn, t, r, &ramp(0, 100));
        s.conn.finish(t, r).expect("§9.5");
        let _ = drain(&mut s.conn);

        s.conn.on_ack_range(t + ms(10), r, 0..100, true);
        abandon_recv(&mut s.conn, t + ms(11), r);
        let _ = drain(&mut s.conn);
        assert_eq!(
            s.conn.stream_id(r),
            None,
            "precondition: both halves gone, so the index is fully closed"
        );

        let id = raw_id(0, Dir::Bi, false);
        let d = s.deliver(t + ms(20), &stream_frame(id, 0, &ramp(0, 8), false));

        assert_alive(&d);
        assert_eq!(s.conn.accept(Dir::Bi), None, "§8.4: ACKed, never re-opened");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 137 — the path generation
// ═══════════════════════════════════════════════════════════════════════

mod path_generation {
    use super::*;

    /// **[ruling 137]** *"`SentPacket` therefore carries a `u32` path
    /// generation from slice 5 onward, held at 0 until roaming exists"*
    /// (`SPEC.md`:3934–3936).
    ///
    /// **What this pins and what it does not.** It pins the field's
    /// existence and its type — the binding is `u32`, so a `bool` or a
    /// `u64` fails here — and it records the constant slice 7 will make
    /// move. It does **not** pin what the core stamps, because nothing in
    /// §2.4's accessor list exposes the sent map and there is therefore no
    /// reachable assertion about a `SentPacket` the core built. That gap is
    /// recorded in `TESTS-5a-recovery.md` §3.
    ///
    /// Mutation caught: the one ruling 137 exists to prevent — reusing
    /// `recovery_start` as the roam fence instead of carrying a stamp. Such
    /// a build has no field to read here at all. §14.6 spells out why that
    /// build is wrong and why it fails silently: *"`recovery_start` is also
    /// set by every ordinary congestion event, so an implementation reusing
    /// it would suppress RTT sampling after every normal loss episode —
    /// silently, and permanently on a lossy path."*
    #[test]
    fn a_sent_packet_carries_a_u32_path_generation_held_at_zero() {
        let p = sent(0, t0(), 1200, tag_ref());

        let generation: u32 = p.path_gen;
        assert_eq!(
            generation, 0,
            "ruling 137: held at 0 until slice 7 moves it"
        );
    }
}

//! Round 40's core-level coverage for the **PTO announce-gate** (ruling
//! 249) and the **2³ backoff cap** (ruling 254).
//!
//! Written from `SPEC.md` §13.3/§13.4/§13.6, Appendix B's two new
//! obligations and rulings 249/254 by an author who never read the fix
//! (CLAUDE.md working rule 6), in a worktree cut at `c21869f` — the
//! commit that carries the ratified rulings and the amended spec and
//! **not** the implementation. Every test below that pins the new
//! behaviour is expected to be red on that base; the report says which.
//!
//! # The defect (ruling 249, measured)
//!
//! `Recovery::pto_deadline()` is `None` **iff** the sent map is empty, and
//! `sync_recovery_timers` announces whatever it returns. §13.3 has always
//! had a second precondition and the code has only ever had the first:
//!
//! > **The `Pto` timer is armed only while at least one ack-eliciting
//! > packet is in the sent map** *and while §7.3's amplification budget
//! > admits a probe datagram*.
//!
//! Past `pto_count`'s saturation the deadline stops moving (the anchor
//! moves only in `on_sent`, and a firing the budget refuses seals
//! nothing), so a saturated train at a closed budget announces the **same
//! past instant forever**: `sleep_until` returns at once, `handle_timeout`
//! re-fires, and §16.3's single `!Send` driver — shared by every
//! connection on the endpoint — spins at 100 % of a core until
//! `DEAD_TIMEOUT` reaps the session. Reachable from any roam, because a
//! roam zeroes the budget (§13.6).
//!
//! # Why "the deadline is correct" is not a test
//!
//! Same shape as `tests_livelock.rs`, which pins the keepalive half of
//! this (slice 7b's F1): **a livelock is not a wrong value, it is the same
//! correct value forever.** `assert_eq!(timer(Pto), anchor + 8 × base)` is
//! precisely what the spinning build does. So every test here asserts from
//! a side that separates them:
//!
//! | shape | the pre-249 build | the fixed build |
//! |---|---|---|
//! | **announcement** — what the connection's terminal `Timeout` is, at a `now` past the saturated deadline | that past `Pto` instant | the `Liveness` deadline, in the future |
//! | **termination** — a driver loop that sleeps to each announced deadline | never advances past the refusal instant | dies at `DEAD_TIMEOUT` in one step |
//! | **freeze** — the multiplier the first post-refund deadline is drawn at | climbed to the cap during the blockade | the one it entered the blockade with |
//! | **cadence** (254) — the interval the ladder settles on | 64 × base | 8 × base |
//!
//! The companion half `tests_livelock.rs` established and this file
//! inherits: a core that answers `Timeout(None)` to everything satisfies
//! non-retrospection trivially and is an **immortal** connection, which
//! ruling 182's beacon proof calls the worse collapse. So every
//! suppression assertion here is paired with one that the death clock is
//! still armed, and [`the_starved_connection_still_dies_at_the_dead_timeout`]
//! drives it all the way to the `Closed` event.
//!
//! # What is deliberately **not** here
//!
//! * The keepalive announce-gate. `tests_livelock.rs` owns it (ruling
//!   249's scope guard: *"the keepalive was checked and is already
//!   safe"*), and duplicating its pins would only make two files fail for
//!   one cause. This file asserts `TimerKind::Keepalive` is `None` in the
//!   starved state as a **fixture precondition**, because the identity of
//!   the next armed timer depends on it — not as a pin of F1.
//! * `PTO_BACKOFF_CAP` itself. A test that imports the constant and
//!   asserts against it asserts nothing about its value;
//!   `tests/spec_constants.rs` pins the number from outside the crate.
//!   [`the_announced_probe_cadence_caps_at_eight_times_the_base`] writes
//!   the multiplier **8** as a literal, derives the base from the core,
//!   and pins the *shape* the constant produces.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here
//! reads a clock. Plain `#[test]`, no `sleep`. The paused-clock obligation
//! for E5a/E5b lands on `tests/story_reliability.rs`.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::testfix::*;
use super::timers::TimerKind;

use crate::constants::{
    AEAD_TAG_LEN, AMPLIFICATION_FACTOR, DATA_HEADER_LEN, DEAD_TIMEOUT, FRAME_ACK, FRAME_PADDING,
};
use crate::error::ConnectionLost;

// ═══════════════════════════════════════════════════════════════════════
// 1. Fixture-level scaffolding
// ═══════════════════════════════════════════════════════════════════════

/// §3.4's cleartext header plus the AEAD tag — what a Data packet costs
/// before a single frame byte.
const PKT_OVERHEAD: u64 = (DATA_HEADER_LEN + AEAD_TAG_LEN) as u64;

/// §8.3's `PATH_CHALLENGE` on the wire: one type byte plus eight opaque
/// (ruling 208). An unvalidated address offers it on every packet the
/// budget admits (§8.7's standing obligation), so the shaping packet in
/// [`roam_and_starve`] must leave room for it or the calibration is wrong.
const CHALLENGE_COST: u64 = 1 + 8;

/// The RTT this file warms the estimator to. Small, so the whole backoff
/// ladder fits far inside `DEAD_TIMEOUT` **on either cap** — which is what
/// lets the same fixture construct the saturated state on the pre-254
/// build (64 ×) and on the ratified one (8 ×). Ruling 249's threat model
/// is exactly this: *"an authenticated peer warms the RTT estimator,
/// roams, goes silent"*.
const WARM_RTT: Duration = Duration::from_millis(20);

/// A third address, for the peer to move to.
fn c_addr() -> SocketAddr {
    v4(9, 41_000)
}

/// §8.4's ACK, as bytes. One block, one counter — the RTT sample is all
/// this file wants from it.
fn ack_frame(largest: u64) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, FRAME_ACK);
    put(&mut out, largest);
    put(&mut out, 0); // ack_delay
    put(&mut out, 0); // range_count
    put(&mut out, 0); // first_range
    out
}

/// How many more datagram bytes §7.3 will admit right now, or `None` on a
/// validated address, where it will admit anything.
fn room(s: &Solo) -> Option<u64> {
    s.conn
        .amplification_budget()
        .map(|(sent, recv)| (AMPLIFICATION_FACTOR * recv).saturating_sub(sent))
}

/// Warm the RTT estimator to [`WARM_RTT`] and leave **one** ack-eliciting
/// packet in flight.
///
/// Returns the base PTO interval **as the core computes it** and the
/// instant of the in-flight send (§13.3's anchor).
///
/// The interval is read off the core rather than written down: §13.3's
/// `PTO = srtt + max(4·rttvar, K_GRANULARITY) + MAX_ACK_DELAY` is already
/// pinned by exact equality in `tests_recovery.rs`, and re-deriving it
/// here would make every test in this file fail for that arithmetic rather
/// than for the property it is about. What this file pins is the
/// **multiplier** the interval is drawn at, which is a ratio.
fn warm(s: &mut Solo, start: Instant) -> (Duration, Instant) {
    let r = s.conn.open(crate::core::Dir::Uni).expect("§16.4: open()");
    s.conn.write(start, r, &[7u8; 200]).expect("write");
    s.conn.flush(start);
    let d = drain(&mut s.conn);
    assert_eq!(
        d.transmits().len(),
        1,
        "fixture: one packet, so the ACK below samples a known round trip"
    );

    // The ACK is the estimator's only sample, and it empties the sent map.
    let ack_at = start + WARM_RTT;
    let _ = s.deliver_from(ack_at, a_addr(), &ack_frame(0));
    assert_eq!(
        s.conn.smoothed_rtt(),
        WARM_RTT,
        "fixture: the first sample *is* the smoothed RTT (§13.1)"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Pto),
        None,
        "§13.3's first precondition: the ACK emptied the sent map, so no \
         `Pto` is armed — an idle connection must not self-sustain a probe train"
    );

    // A second ack-eliciting packet, never acknowledged: the train's root.
    let anchor = ack_at + Duration::from_millis(1);
    s.conn.write(anchor, r, &[7u8; 200]).expect("write");
    s.conn.flush(anchor);
    let _ = drain(&mut s.conn);
    let base = s
        .conn
        .timer(TimerKind::Pto)
        .expect("§13.3: a non-empty sent map on a validated address arms `Pto`")
        - anchor;
    assert!(
        base > Duration::ZERO,
        "fixture: the base interval must be positive"
    );
    (base, anchor)
}

/// Fire the `Pto` at the instant the core announced it, on a **validated**
/// address, where the probe leaves and re-anchors the train. Returns that
/// instant.
fn fire_pto(s: &mut Solo, what: &str) -> Instant {
    let at = s
        .conn
        .timer(TimerKind::Pto)
        .unwrap_or_else(|| panic!("fixture: no `Pto` armed while {what}"));
    s.conn.handle_timeout(at);
    let d = drain(&mut s.conn);
    assert_eq!(
        d.transmits().len(),
        1,
        "§13.4: a firing PTO sends **one** ack-eliciting packet ({what})"
    );
    at
}

/// Roam the connection to [`c_addr`] and spend the fresh budget down to
/// **exactly zero**, at `at`.
///
/// Two levers in one helper because they are one state: §13.6's roam
/// zeroes the budget (the address is unvalidated again) and §7.3's
/// arithmetic then admits `3 × 30 = 90` bytes, which is more than a probe
/// datagram. Zero is the unambiguous closed budget — every predicate an
/// implementer might write for *"admits a probe datagram"* refuses at
/// zero, so this fixture does not smuggle a size guess into the pin.
///
/// The roaming packet is a §7.5 keepalive (empty plaintext): authenticated
/// and window-fresh, so §7.2/§7.3 accept it as a roam (ruling 180), and
/// **not** ack-eliciting, so it acknowledges nothing and leaves
/// `pto_count` alone (§13.3 resets it only on a newly acknowledged
/// packet). The shaping packet is a §11 datagram, sealed inside the call
/// (§16.7) and **fully sent**, so the send queue is empty afterwards: a
/// leftover would be flushed by the refund in
/// [`the_refund_re_announces_the_probe_deadline_and_fires_a_probe`] and
/// would move §13.3's anchor, which is the quantity that test is about.
fn roam_and_starve(s: &mut Solo, at: Instant) {
    roam_and_starve_at(s, at, at);
}

/// The roam **receive** lands at `recv_at`; the budget-spending send at
/// `send_at`. Separated instants are what let a test pin §7.4's anchor on
/// the receive clock — a helper doing both at one instant makes
/// receive-anchored and send-anchored the same number (working rule 9).
fn roam_and_starve_at(s: &mut Solo, recv_at: Instant, send_at: Instant) {
    let before = s.conn.remote_address();
    let d = s.deliver_from(recv_at, c_addr(), &[]);
    assert_eq!(
        s.conn.remote_address(),
        Some(c_addr()),
        "fixture: the peer moved (§7.3); it was at {before:?}"
    );
    assert!(
        d.transmits().is_empty(),
        "fixture: the roam itself emits nothing, so the whole 90-byte \
         budget is this helper's to spend"
    );

    let space = room(s).expect("§13.6: a roam zeroes the budget — the address is unvalidated");
    // One type byte, one length varint, and the standing challenge offer.
    let payload = space - PKT_OVERHEAD - 1 - 1 - CHALLENGE_COST;
    assert!(
        payload < 64,
        "fixture: the length varint above is one byte only below 64; got {payload}"
    );
    s.conn
        .send_datagram(send_at, &ramp(0, payload as usize))
        .expect("§11: a sub-maximum datagram is accepted");
    let d = drain(&mut s.conn);
    assert_eq!(
        d.transmits().len(),
        1,
        "fixture: the shaping datagram is sized to fit, so it must leave whole"
    );
    assert_eq!(
        d.transmits()[0].data.len() as u64,
        space,
        "fixture: the shaping packet must be *exactly* the room to be spent, \
         or every assertion calibrated against a closed budget is calibrated \
         against the wrong number"
    );
    assert_eq!(
        room(s),
        Some(0),
        "fixture: the point of the helper — §7.3 now admits nothing at all"
    );
}

/// What a bounded emulation of `Driver::run` did.
#[derive(Debug)]
enum Drive {
    /// The connection reported `Closed` at this instant.
    Died(Instant, ConnectionLost),
    /// The next announced deadline is past the horizon, or there is none.
    Quiet { steps: usize, next: Option<Instant> },
}

/// The driver loop, in the small: **sleep to each announced deadline, hand
/// it back, repeat.**
///
/// This is §16.3's actor reduced to the one connection under test, and it
/// is the shape that makes ruling 249's finding a *livelock* rather than a
/// wrong number. `sleep_until(d)` for a `d` at or before the current
/// instant returns immediately, so a core that re-announces the same past
/// deadline is a hot loop; the emulation reproduces that by never letting
/// `now` go backwards.
///
/// `MAX_STEPS` is the pin. A conforming core leaves the horizon (or dies)
/// in a handful of steps; the pre-249 core cannot leave it at all, and the
/// panic below is what that looks like from inside a test.
fn drive(s: &mut Solo, from: Instant, horizon: Instant) -> Drive {
    const MAX_STEPS: usize = 64;
    let mut now = from;
    for steps in 0..MAX_STEPS {
        let d = drain(&mut s.conn);
        if let Some(lost) = d.closed() {
            return Drive::Died(now, lost);
        }
        match d.deadline {
            None => return Drive::Quiet { steps, next: None },
            Some(next) if next > horizon => {
                return Drive::Quiet {
                    steps,
                    next: Some(next),
                };
            }
            Some(next) => {
                // A deadline in the past fires **now**: `sleep_until`
                // returns at once and the clock does not go backwards.
                now = now.max(next);
                s.conn.handle_timeout(now);
            }
        }
    }
    panic!(
        "the driver loop did not leave {horizon:?} in {MAX_STEPS} steps, and \
         is at {now:?}: the core is re-announcing a deadline the loop has \
         already consumed. This is ruling 249's livelock — §16.3's one \
         `!Send` actor, shared by every connection on the endpoint, spins \
         at 100 % of a core here."
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Ruling 249 — the announcement gate
// ═══════════════════════════════════════════════════════════════════════

/// **The livelock separation.** A saturated probe train at a closed budget
/// announces the **`Liveness`** deadline, not a `Pto` at or before `now`.
///
/// The state, built exactly as ruling 249's threat model describes it: an
/// authenticated peer warms the RTT estimator, the train saturates on the
/// validated address, the peer roams (§13.6 zeroes the budget), and the
/// budget is spent to nothing. §13.3's *first* precondition still holds —
/// the sent map is **not** empty, asserted below — so nothing here is the
/// trivial disarm.
///
/// Mutation caught: the shipped `pto_deadline()`, whose doc says `None`
/// **iff** the sent map is empty. It announces `anchor + interval × 2^cap`,
/// an instant already 12 s in the past at the assertion point, and the
/// assertion `deadline > now` fails on it. Also caught, by the companion
/// assertions: a "fix" that disarms `Liveness` alongside `Pto`, which
/// satisfies non-retrospection trivially and makes the connection immortal
/// (ruling 182's collapse).
///
/// Not asserted, deliberately: `timer(Pto) == Some(x)` for any `x`. That
/// equality is what the broken build satisfies.
#[test]
fn a_starved_probe_train_announces_liveness_not_a_deadline_in_the_past() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let (_base, _anchor) = warm(&mut s, start);

    // Saturate. Eight firings saturate `pto_count` at **either** cap — 2³
    // needs three, 2⁶ needs six — so the construction reaches the state
    // this test is about on the pre-254 build too, and the red it reports
    // is 249's and not 254's.
    let mut last = start;
    for i in 0..8 {
        last = fire_pto(&mut s, &format!("saturating the train, firing {i}"));
    }

    let roam_at = last + Duration::from_millis(1);
    roam_and_starve(&mut s, roam_at);

    // Preconditions, all four, or the pin below is vacuous.
    assert!(
        s.conn.bytes_in_flight() > 0,
        "fixture: §13.3's *first* precondition holds — the sent map is not \
         empty, so a disarmed `Pto` here is the budget's doing and not the \
         empty-map rule's"
    );
    assert_eq!(room(&s), Some(0), "fixture: the budget admits nothing");
    assert_eq!(
        s.conn.timer(TimerKind::Keepalive),
        None,
        "fixture: slice 7b's F1 gate already holds the keepalive at zero \
         room (`tests_livelock.rs`), which is why `Liveness` is the next \
         armed timer below rather than the beacon"
    );

    // Well past any rung of either ladder (2⁶ × 85 ms is 5.4 s), and well
    // short of the death deadline: on the pre-249 build the announced `Pto`
    // is 12 s stale here.
    let t = roam_at + DEAD_TIMEOUT / 2;
    s.conn.handle_timeout(t);
    let d = drain(&mut s.conn);

    assert!(
        d.transmits().is_empty(),
        "fixture: §7.3 admits nothing at room 0, so nothing left — if a \
         probe had gone out, this test would prove nothing about the gate"
    );

    let announced = d.deadline.expect(
        "§13.3: the `Timeout` falls to the next armed timer — `Liveness` at \
         the latest. `None` here is the immortal-connection collapse",
    );
    assert!(
        announced > t,
        "ruling 249: the core announced {announced:?}, at or before the \
         instant it was just handed ({t:?}). `sleep_until` completes \
         immediately, `handle_timeout` re-fires, and §16.3's shared driver \
         spins until `DEAD_TIMEOUT`"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Pto),
        None,
        "§13.3, amended: *the `Pto` timer is armed only while … §7.3's \
         amplification budget admits a probe datagram*"
    );
    assert_eq!(
        Some(announced),
        s.conn.timer(TimerKind::Liveness),
        "§13.3: the announced `Timeout` is the next armed timer, which here \
         is §7.4's death clock"
    );
    assert_eq!(
        announced,
        roam_at + DEAD_TIMEOUT,
        "§7.4: and the death clock is anchored on the last authenticated \
         **receive**, which was the roam"
    );
}

/// **Death is undisturbed.** Under a total blockade the connection still
/// dies at `DEAD_TIMEOUT`, receive-anchored — and a driver loop reaches it
/// in a bounded number of steps.
///
/// §13.4 has always stated the intended outcome — *"the session still dies
/// at `DEAD_TIMEOUT` as §7.3 intends"* — and this is the assertion that it
/// happens rather than being deferred forever by a timer that fires and
/// emits nothing.
///
/// The receive anchor is separated from the send anchor on purpose: the
/// roam is at `roam_at` and the shaping send is 7 ms later, so a death
/// clock run off the last *send* lands 7 ms late and this fails.
///
/// Mutation caught: the pre-249 core, on which `drive` panics — the loop
/// consumes the same past `Pto` 64 times without the clock advancing past
/// `roam_at + 2⁶ × base`. That panic **is** the livelock, and it is why
/// this file emulates the driver rather than asserting on one drain.
#[test]
fn the_starved_connection_still_dies_at_the_dead_timeout() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let (_, _) = warm(&mut s, start);
    let last = fire_pto(&mut s, "arming a backed-off train");

    let roam_at = last + Duration::from_millis(1);
    // §7.4's anchor is the receive; the budget-spending send is
    // deliberately 7 ms later, so a send-anchored build dies 7 ms late
    // and goes red here. (The blind draft moved *both* instants 7 ms —
    // the helper then took one timestamp — which anchored the receive at
    // +7 ms too and read as an implementation defect; §7.4 and
    // `Liveness::deadline()` were opened at integration and the code was
    // right: receive-anchored. Corrected geometry, same pin.)
    roam_and_starve_at(&mut s, roam_at, roam_at + Duration::from_millis(7));
    assert!(
        s.conn.bytes_in_flight() > 0,
        "fixture: the sent map is not empty"
    );

    match drive(
        &mut s,
        roam_at,
        roam_at + DEAD_TIMEOUT + Duration::from_secs(1),
    ) {
        Drive::Died(at, lost) => {
            assert!(
                matches!(lost, ConnectionLost::TimedOut),
                "§7.4: a blockaded connection dies of the death clock, not of \
                 anything else; got {lost:?}"
            );
            assert_eq!(
                at,
                roam_at + DEAD_TIMEOUT,
                "§7.4: `DEAD_TIMEOUT` after the last authenticated **receive** \
                 — not after the last send, which was 7 ms later"
            );
        }
        other => panic!(
            "§13.4: *the session still dies at `DEAD_TIMEOUT` as §7.3 \
             intends* — the loop instead reported {other:?}"
        ),
    }
}

/// **Re-arm on refund.** The authenticated, window-fresh receive that
/// refunds the budget re-announces the deadline, and the next firing emits
/// a probe.
///
/// §13.3, amended: *"the deadline is announced again at the authenticated,
/// window-fresh receive that refunds the budget (… every receive
/// recomputes the `Timeout`, so no dedicated re-arm machinery exists)."*
///
/// The refund carries PADDING only: not ack-eliciting (§8.3), so it
/// acknowledges nothing and cannot reset `pto_count` — a refund built from
/// an ACK would re-announce a deadline for a reason this test is not about.
///
/// Mutation caught, two of them. A gate that disarms `Pto` **permanently**
/// once the budget closes — the cheapest way to make the previous test
/// pass — never re-announces and fails the first assertion. A gate applied
/// to the timer *state* rather than the announcement resets the anchor or
/// the count, and the exact-equality assertion fails: the deadline must
/// come back **where it was left**, which is §13.3's *"sent map,
/// `pto_count` and anchor are untouched while the budget is closed"*.
///
/// The probe's content is §13.4's third case, ruling 221: on an
/// unvalidated address the probe carries `PATH_CHALLENGE`, and no PING is
/// owed beside it because the challenge is itself ack-eliciting.
#[test]
fn the_refund_re_announces_the_probe_deadline_and_fires_a_probe() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let (base, _) = warm(&mut s, start);
    let last = fire_pto(&mut s, "arming a backed-off train");

    let roam_at = last + Duration::from_millis(1);
    roam_and_starve(&mut s, roam_at);
    // The shaping datagram is the newest ack-eliciting send, so it is
    // §13.3's anchor, and one firing has happened: `2 × base`.
    let frozen = roam_at + base * 2;

    let t = roam_at + DEAD_TIMEOUT / 2;
    s.conn.handle_timeout(t);
    let _ = drain(&mut s.conn);
    let in_flight = s.conn.bytes_in_flight();

    // §7.2/§7.3's refund: authenticated, window-fresh, from the anchor.
    let d = s.deliver_from(t, c_addr(), &[FRAME_PADDING as u8; 600]);
    assert!(
        d.closed().is_none(),
        "fixture: the connection is alive at the refund"
    );
    let refunded = room(&s).expect("still unvalidated: no challenge has been answered");
    assert!(
        refunded > PKT_OVERHEAD + CHALLENGE_COST,
        "fixture: the refund must admit a probe datagram; room is {refunded}"
    );

    assert_eq!(
        s.conn.timer(TimerKind::Pto),
        Some(frozen),
        "§13.3: the deadline is announced again at the receive that refunds \
         the budget — and **where it was left**, because the sent map, \
         `pto_count` and the anchor were untouched throughout"
    );

    // The firing. The deadline is legitimately overdue (ruling 249(ii): a
    // deadline computed while the budget was closed fires once), so the
    // instant is the later of the two.
    let fire_at = frozen.max(t);
    s.conn.handle_timeout(fire_at);
    let d = drain(&mut s.conn);
    assert!(
        !d.transmits().is_empty(),
        "§13.4: the re-armed firing sends one ack-eliciting packet"
    );
    let probe = s
        .packets(&d)
        .into_iter()
        .find(|frames| frames.iter().any(|f| matches!(f, Wire::PathChallenge(_))));
    assert!(
        probe.is_some(),
        "§13.4/ruling 221: on an unvalidated address the probe carries \
         `PATH_CHALLENGE` — *the only mechanism by which a lost challenge is \
         asked again*. Got {:?}",
        s.packets(&d)
    );
    assert!(
        s.conn.bytes_in_flight() > in_flight,
        "§13.5: and the probe is in the sent map — a probe that leaves \
         untracked is not a probe"
    );
}

/// **The freeze.** `pto_count` does not climb across a blockade: the first
/// deadline announced after the refund is drawn at the multiplier the
/// train entered the blockade with.
///
/// Ruling 249 states the consequence in as many words — *"the backoff no
/// longer climbs through a blockade; `pto_count` freezes and resumes at the
/// first admitted firing"* — and re-founds ruling 139(a) on RFC 9002's
/// ordering point alone, because the budget-suppressed firing its
/// slither-specific rationale described no longer happens.
///
/// One firing before the roam, so the multiplier under test is **2**, not
/// the cap. That is working rule 9: at a saturated count "frozen" and
/// "climbed" are the same number, and the assertion would separate nothing.
///
/// Mutation caught: the pre-249 build, on which `drive` panics after 64
/// same-instant firings — every one of which incremented `pto_count`, so
/// even a hypothetical build that escaped the spin would come out at the
/// cap (`64 × base` there, `8 × base` post-254) rather than `2 × base`.
/// Also caught: a gate that re-announces by *recomputing from scratch* —
/// resetting `pto_count` to 0 when the budget refunds gives `1 × base` and
/// fails, and it is the plausible implementation of "re-arm" that ruling
/// 249's *"no dedicated re-arm machinery exists"* rules out.
#[test]
fn the_backoff_does_not_climb_through_a_closed_budget() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let (base, _) = warm(&mut s, start);
    let last = fire_pto(&mut s, "taking `pto_count` to exactly 1");

    let roam_at = last + Duration::from_millis(1);
    roam_and_starve(&mut s, roam_at);

    let horizon = roam_at + DEAD_TIMEOUT / 2;
    match drive(&mut s, roam_at, horizon) {
        Drive::Quiet { steps, next } => assert_eq!(
            next,
            Some(roam_at + DEAD_TIMEOUT),
            "§13.3: with the budget closed the only thing left to wait for is \
             §7.4's death clock; the loop consumed {steps} deadline(s) first"
        ),
        other => panic!("the connection must survive the blockade here: {other:?}"),
    }

    let _ = s.deliver_from(horizon, c_addr(), &[FRAME_PADDING as u8; 600]);
    assert_eq!(
        s.conn.timer(TimerKind::Pto),
        Some(roam_at + base * 2),
        "ruling 249: `pto_count` was 1 when the budget closed and is 1 when \
         it opens — {:?} of blockade added no rungs to the ladder",
        horizon - roam_at
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Ruling 254 — the cap, seen from the announced deadline
// ═══════════════════════════════════════════════════════════════════════

/// **The cadence.** The announced probe deadlines step `1×, 2×, 4×` and
/// then settle at exactly **8 ×** the base PTO, and stay there.
///
/// `PTO_BACKOFF_CAP` = 2³ (ruling 254). This is the *connection*-level
/// view — the spacing of `TimerKind::Pto` as the shell would see it —
/// which is the quantity §13.3's survival envelope is about: *"under
/// sustained random loss the probe cadence never thins beyond 8 × PTO"*.
/// `tests_recovery.rs` drives `Recovery` directly for the same arithmetic;
/// neither reaches the other's failure.
///
/// Both sides of working rule 9, because a one-sided bound here asserts
/// nothing:
///
/// * *not all equal* — an always-at-base build (no backoff at all) gives
///   `1,1,1,1,…` and fails the doubling assertions;
/// * *the cap is reached and holds* — the pre-254 build gives
///   `1,2,4,8,16,32,64,64` and fails at the fifth rung; an uncapped build
///   gives `…,16,32,64,128` and fails there too.
///
/// The multiplier **8** is written as a literal on purpose: importing
/// `PTO_BACKOFF_CAP` and asserting against it is a tautology, and the
/// hazard sharpened at the new value — `1u32 << 8` is a legal 256 where
/// `1u32 << 64` was loud (ruling 254). The base is read off the core, so
/// this test moves with `K_INITIAL_RTT` and the PTO formula and not with
/// the cap.
#[test]
fn the_announced_probe_cadence_caps_at_eight_times_the_base() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let (base, anchor) = warm(&mut s, start);

    let mut prev = anchor;
    let mut rungs = Vec::new();
    for i in 0..8 {
        let at = fire_pto(&mut s, &format!("walking the ladder, firing {i}"));
        rungs.push(at - prev);
        prev = at;
    }

    assert_eq!(
        rungs,
        vec![
            base,
            base * 2,
            base * 4,
            base * 8,
            base * 8,
            base * 8,
            base * 8,
            base * 8,
        ],
        "§13.3: `2^pto_count` capped at `PTO_BACKOFF_CAP` = 2³ — the ladder \
         doubles three times and then holds. Base is {base:?}"
    );

    // Both halves of rule 9, stated as assertions rather than left to the
    // vector: a build with no backoff at all, and a build with no cap.
    assert!(
        rungs.iter().any(|r| *r != rungs[0]),
        "a build that never backs off announces the same interval forever"
    );
    assert_eq!(
        *rungs.iter().max().expect("eight rungs"),
        base * 8,
        "§13.3's envelope: *the probe cadence never thins beyond 8 × PTO*"
    );
    assert_eq!(
        rungs[rungs.len() - 1],
        rungs[rungs.len() - 2],
        "the cap is **reached**: the last two rungs are the same, so this is \
         a ladder that arrived at its ceiling and not one still climbing"
    );
}

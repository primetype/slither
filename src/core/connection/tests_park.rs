//! **Ruling 265 — the vetoed keepalive arms the death clock.**
//!
//! `tests_livelock.rs` §5 (`the_passive_form`) reports that the `armed ==
//! false` variant of F1 is **not constructible**, and rests that report on
//! one inequality: every receive that sets §7.5's passive debt credits
//! §7.3's budget by at least `3 × 30 = 90` bytes, so the room can never be
//! below the 30 the keepalive asks for at the instant the debt is set.
//!
//! **The first half of that argument is exact. The second half is false, and
//! this file is the measurement.** The room does not grow monotonically: a
//! pure ACK is *sized to the room* — [`Connection::packing`] clamps the
//! plaintext to `room − 30` and [`ack::derive`](super::ack::derive)
//! truncates newest-first at that room — so on a sufficiently fragmented
//! replay window **one** non-marking, non-arming packet takes all of it.
//! Measured here at **105 bytes against a 105-byte room, 33 ACK range
//! pairs**.
//!
//! # The state that reaches
//!
//! A connection that owes the passive keepalive, cannot send it, and has
//! **no timer armed at all**:
//!
//! * §7.4 clears `armed` on every authenticated, window-fresh receive, and
//!   the *same* call sets the passive debt (`Liveness::
//!   on_authenticated_fresh_recv`) — so `Liveness::deadline()` is `None`
//!   exactly where §7.5 says a keepalive is owed;
//! * [`keepalive_can_leave`](super::Connection::keepalive_can_leave)
//!   suppresses both keepalive announcements while the budget is starved;
//! * `Pto` is budget-gated too (ruling 249), `AckDelay` is disarmed by the
//!   ACK that starved the budget, and `Loss` drains.
//!
//! `Timeout(None)`, forever: the shell sleeps and nothing ever wakes it.
//! That is the **immortal half-open session** ruling 182's beacon proof
//! exists to forbid, reached through a door that proof does not enumerate —
//! it quantifies over *sends*, and this is a keepalive that is **owed and
//! never sent**. Ruling 265's backstop announces §7.4's own deadline in its
//! place.
//!
//! # Working rule 9 — what each assertion separates
//!
//! A test that only asserted *"the connection dies"* would pass a build that
//! never reached the state at all, and one that only asserted
//! *"`Timeout` is not `None`"* would pass the pre-F1 **spin**. So the
//! assertions are kept apart, and each names its degenerate build:
//!
//! | assertion | the build it fails |
//! |---|---|
//! | **premise**, read at the instant the starving ACK lands | a construction that silently missed the state — §14.5's gate left open, so the coalesced ACK + `PATH_CHALLENGE` leaves and *arms* the clock |
//! | **liveness** — died **or** sent, within `DEAD_TIMEOUT` + slack | the shipped build before ruling 265: announces `Timeout(None)` and neither dies nor sends |
//! | **non-retrospection** — bounded consecutive non-advancing turns | a "fix" that drops the gate from the announcement but keeps `transmit_keepalive`'s guard: F1's spin, re-armed in the past for ever |
//! | **the recovering peer** ([`the_backstop_does_not_reap_a_connection_whose_peer_comes_back`]) | a "fix" that announces `Liveness` unconditionally — it passes all three above and reaps a connection whose peer is merely starving it briefly |
//!
//! The premise is read **at the ACK instant, not at the end**: on a build
//! that then kills the connection, `liveness()` is already `None` by the
//! time the driver stops, and reading it there turns the premise into a
//! panic about the fixture.
//!
//! # No clock, so no runtime
//!
//! Sans-io: `now: Instant` is an argument and nothing here reads a clock.
//! Plain `#[test]`, no `sleep`. Every mutating call is followed by draining
//! `poll_output()` to the terminal `Timeout` (§16.4), which is what
//! [`testfix::drain`] does. The shell half of this ruling is
//! `tests/story_park.rs`.

#![allow(clippy::items_after_statements)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::stream_id::Dir;
use super::testfix::{Solo, Wire, drain, max_data_frame, parse_frames, ramp, t0, v4, write_all};
use super::timers::TimerKind;
use crate::constants::{AMPLIFICATION_FACTOR, DEAD_TIMEOUT};

// ═══════════════════════════════════════════════════════════════════════
// 1. The construction
// ═══════════════════════════════════════════════════════════════════════

/// A third address, for the peer to roam to and then vanish from.
fn c_addr() -> SocketAddr {
    v4(9, 41_000)
}

/// How many more datagram bytes §7.3 will admit right now, or `None` on a
/// validated address.
fn room(s: &Solo) -> Option<u64> {
    s.conn
        .amplification_budget()
        .map(|(sent, recv)| (AMPLIFICATION_FACTOR * recv).saturating_sub(sent))
}

/// Gaps to leave in the replay window.
///
/// §12.2's ACK needs one range pair per gap, and 33 pairs is what fills the
/// 105-byte room the roam below funds. 48 is comfortably above that, so the
/// truncation `ack::derive` performs is the *room*, never the supply — which
/// is the property under test.
const GAPS: u64 = 48;

/// Unacked stream bytes to put in flight before topping up to the window.
const BULK: usize = 9_000;

/// Steps 1–3, built at `start`, returning the core and the deadline it
/// announced at the roam instant.
///
/// Afterwards: `armed == false`, the passive debt set, the budget armed at
/// `(0, L)` for the roam packet's `L`, and §14.5's window **exactly full**
/// so the coalesced ACK + `PATH_CHALLENGE` packet cannot leave.
///
/// # Why the window must be full to the byte
///
/// §14.5's gate is `bytes_in_flight + size <= cwnd`, so what the
/// construction needs is `cwnd − bytes_in_flight < room` — *"`bytes_in_flight`
/// non-trivial"* is not enough. Left with 1 086 B of headroom the coalesced
/// packet leaves, `PATH_CHALLENGE` is ack-eliciting, the death clock arms,
/// and the connection dies at `DEAD_TIMEOUT` — a green test asserting
/// nothing. The `assert_eq!` below is what keeps that from happening
/// silently.
fn roamed_with_a_starved_budget(start: Instant) -> (Solo, Option<Instant>) {
    let mut s = Solo::installed_at(start);

    // 1a — fragment the peer-side replay window. Sealing a datagram and
    // dropping it burns a counter, which is a gap: §7.2 marks only what
    // arrives.
    let mut credit = 1_000_000u64;
    for _ in 0..GAPS {
        let _burned = s.peer.seal(&[]);
        credit += 100_000;
        let _ = s.deliver(start, &max_data_frame(credit));
    }

    // 1b — put unacked data in flight. **Bounded**: a stream left with data
    // queued makes the pump plan a full-size packet that §14.5 refuses, and
    // the top-up below never gets a turn.
    let r = s.conn.open(Dir::Uni).expect("§9.1: our own uni space");
    let blocked = write_all(&mut s.conn, start, r, &ramp(0, BULK));
    assert_eq!(blocked, 0, "fixture: {BULK} B fits the initial credit");
    let _ = drain(&mut s.conn);

    // 1c — top the flight up to exactly the congestion window.
    for _ in 0..64 {
        // §8.5 packs an owed ACK ahead of the datagram fill, which would
        // make the shaping packet larger than the size computed here.
        if let Some(at) = s.conn.timer(TimerKind::AckDelay) {
            s.conn.handle_timeout(at);
            let _ = drain(&mut s.conn);
        }
        let headroom = s.conn.congestion_window() - s.conn.bytes_in_flight();
        if headroom < 34 {
            break;
        }
        // packet = 30 (§3.4 header + tag) + 1 (DATAGRAM_LEN) + varint + len
        let want = headroom.min(1_000);
        let payload = want - 30 - 1 - if want - 32 < 64 { 1 } else { 2 };
        let before = s.conn.bytes_in_flight();
        s.conn
            .send_datagram(start, &ramp(0, payload as usize))
            .expect("§11: a sub-maximum datagram is accepted");
        let _ = drain(&mut s.conn);
        if s.conn.bytes_in_flight() == before {
            break;
        }
    }
    assert_eq!(
        s.conn.congestion_window() - s.conn.bytes_in_flight(),
        0,
        "fixture: §14.5's gate must be shut, or the post-roam packet leaves \
         and arms the death clock — see this function's header"
    );

    // 2/3 — the roam. One small ack-eliciting authenticated packet from a
    // new source: `commit_roam` re-arms the budget at `(0, L)`, and
    // `on_authenticated_fresh_recv` clears `armed` and sets the passive debt
    // in the same call.
    let d = s.deliver_from(start, c_addr(), &max_data_frame(credit + 1));
    (s, d.deadline)
}

// ═══════════════════════════════════════════════════════════════════════
// 2. The driver, run at the core
// ═══════════════════════════════════════════════════════════════════════

/// What one `Driver::run` simulation observed.
struct Run {
    /// Where the loop stopped.
    at: Instant,
    /// Datagrams emitted over the whole run.
    sent: usize,
    /// Whether the connection died.
    died: bool,
    /// `(owes_passive_keepalive, is_armed, room)` **after the first step** —
    /// the instant the starving pure ACK lands, and the only place the
    /// premise is still readable on a build that then kills the connection.
    premise: Option<(bool, bool, Option<u64>)>,
    /// `(datagram bytes, ACK range pairs)` of the first step's packet, when
    /// that packet is a lone ACK. This is `the_passive_form`'s corrected
    /// arithmetic, pinned rather than asserted in prose.
    ack: Option<(usize, usize)>,
}

/// Follow the announced deadlines exactly as `Driver::run` does: sleep to
/// whatever was announced, hand it back, repeat.
///
/// A deadline **at or before** the `now` just handled is a *catch-up* turn —
/// `sleep_until` completes at once, the timer fires, and the work it names
/// moves `last_send`. That is legitimate exactly once, and is ordinary right
/// after a receive whose `last_send + KEEPALIVE_TIMEOUT` has already passed.
/// F1's spin is the same value re-announced **for ever**, so the bound here
/// is on *consecutive* non-advancing turns, not on one of them.
fn run_driver(s: &mut Solo, start: Instant, mut next: Option<Instant>, horizon: Instant) -> Run {
    /// Consecutive turns that may fail to advance before it is a spin.
    const STALL_CAP: u32 = 4;

    let mut at = start;
    let mut sent = 0usize;
    let mut premise = None;
    let mut ack = None;
    let mut stalled = 0u32;
    for step in 0..4096 {
        let (Some(when), true) = (next, at <= horizon) else {
            return Run {
                at,
                sent,
                died: false,
                premise,
                ack,
            };
        };
        if when <= at {
            stalled += 1;
            assert!(
                stalled <= STALL_CAP,
                "spin: {stalled} consecutive turns without advancing, at step {step}"
            );
        } else {
            stalled = 0;
        }
        let when = when.max(at);
        if when > horizon {
            return Run {
                at,
                sent,
                died: false,
                premise,
                ack,
            };
        }
        at = when;
        s.conn.handle_timeout(at);
        let d = drain(&mut s.conn);
        sent += d.transmits().len();
        if step == 0 {
            ack = match d.transmits().as_slice() {
                [t] => match parse_frames(&s.peer.open_dgram(&t.data)).as_slice() {
                    [Wire::Ack { ranges, .. }] => Some((t.data.len(), ranges.len())),
                    _ => None,
                },
                _ => None,
            };
            premise = s
                .conn
                .liveness()
                .map(|l| (l.owes_passive_keepalive(), l.is_armed(), room(s)));
        }
        if d.closed().is_some() {
            return Run {
                at,
                sent,
                died: true,
                premise,
                ack,
            };
        }
        next = d.deadline;
    }
    panic!("the driver never reached a fixed point");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. The assertions
// ═══════════════════════════════════════════════════════════════════════

/// **Ruling 265's pin.** A connection owing a keepalive §7.3 will not let
/// leave must still be reaped at `DEAD_TIMEOUT`.
///
/// See the module header for the four builds this separates and why the
/// three assertions are kept apart. The liveness assertion is deliberately
/// *died **or** sent*: it does not prescribe which fix landed.
#[test]
fn a_starved_passive_keepalive_must_not_park_the_connection_forever() {
    let start = t0();
    let (mut s, deadline) = roamed_with_a_starved_budget(start);

    let horizon = start + DEAD_TIMEOUT + Duration::from_secs(5);
    let run = run_driver(&mut s, start, deadline, horizon);

    // ── premise ──────────────────────────────────────────────────────
    let (debt, armed, room_left) = run
        .premise
        .expect("fixture: the session is installed at the first driver step");
    assert!(
        debt,
        "premise: §7.5's passive keepalive must be owed in the parked state"
    );
    assert!(
        !armed,
        "premise: §7.4's death clock must be disarmed — that is the whole \
         subject. `armed` is cleared by the same receive that set the debt."
    );
    assert!(
        room_left.is_some_and(|r| r < 30),
        "premise: §7.3's budget must refuse the 30-byte keepalive, or \
         `keepalive_can_leave()` is true and nothing is suppressed; \
         room was {room_left:?}"
    );

    // ── the arithmetic `the_passive_form` had wrong ──────────────────
    let (bytes, ranges) = run
        .ack
        .expect("the one post-roam packet must be a lone ACK (§12.4's standalone)");
    assert!(
        ranges >= 20,
        "§12.2: the ACK must be filling the room with range pairs, not \
         costing ~35 bytes; got {ranges} pairs in {bytes} B"
    );
    assert_eq!(
        bytes, 105,
        "§7.3 + ruling 203: a pure ACK is sized to the room — the roam \
         funded 3 × 35 = 105 datagram bytes and the ACK took all of them"
    );

    // ── liveness ─────────────────────────────────────────────────────
    assert!(
        run.died || run.sent > 1,
        "§7.4/§7.5: a connection owing a keepalive it cannot send must still \
         be reaped at DEAD_TIMEOUT (or recover and send). It did neither: \
         parked at {:?} after start with {} packet(s) sent since the roam, \
         and the shell is asleep on Timeout(None) for ever.",
        run.at - start,
        run.sent
    );

    // ── and the reaper is §7.4's own deadline, not something later ───
    if run.died {
        assert_eq!(
            run.at - start,
            DEAD_TIMEOUT,
            "§7.4: the deadline is `last_authenticated_recv + DEAD_TIMEOUT`"
        );
    }
}

/// **The other half of ruling 265's soundness.**
///
/// A backstop that reaped a connection whose peer is only *briefly* starving
/// it would be a cure worse than the disease — and it would pass every
/// assertion in the test above. Both disjuncts of `keepalive_can_leave()`
/// lift only on an authenticated, window-fresh receive, and that receive
/// also moves `last_authenticated_recv`, so the backstop deadline must slide
/// with it.
///
/// Park the connection, then let the peer speak again **inside**
/// `DEAD_TIMEOUT`: the budget is re-funded, the real `Keepalive`
/// announcement returns in the backstop's place, keepalives leave, and death
/// lands `DEAD_TIMEOUT` after the *new* receive rather than the old one.
#[test]
fn the_backstop_does_not_reap_a_connection_whose_peer_comes_back() {
    let start = t0();
    let (mut s, deadline) = roamed_with_a_starved_budget(start);

    // Reach the held state, well inside DEAD_TIMEOUT.
    let run = run_driver(&mut s, start, deadline, start + Duration::from_secs(1));
    assert!(
        !run.died,
        "fixture: the hold is reached long before DEAD_TIMEOUT"
    );

    // The peer speaks again from the address it roamed to. This is the one
    // event that can lift either disjunct of the hold.
    let back = start + Duration::from_secs(10);
    let d = s.deliver_from(back, c_addr(), &max_data_frame(9_999_999));
    assert!(
        d.closed().is_none(),
        "§7.2: an authenticated, window-fresh receive must not kill the \
         connection it just refreshed"
    );
    assert!(
        room(&s).is_some_and(|r| r >= 30),
        "§7.3: the receive re-funds the budget, so the keepalive can leave"
    );
    assert!(
        s.conn.timer(TimerKind::Keepalive).is_some(),
        "§7.5: with the hold lifted the passive keepalive is announced again"
    );

    // And the driver now runs to a keepalive, not straight to a death.
    let run = run_driver(
        &mut s,
        back,
        d.deadline,
        back + DEAD_TIMEOUT + Duration::from_secs(5),
    );
    assert!(
        run.sent >= 1,
        "§7.5: the dance resumes — a keepalive must leave once the budget \
         admits it"
    );
    assert_eq!(
        run.died.then(|| run.at - start),
        Some(Duration::from_secs(10) + DEAD_TIMEOUT),
        "§7.4: the clock is anchored to the **latest** authenticated receive, \
         so the session outlives the original anchor by exactly the 10 s the \
         peer's return bought it"
    );
}

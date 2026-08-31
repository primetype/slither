//! Acceptance tests for checked monotonic-deadline arithmetic (ruling 284).
//!
//! The tests use the actual edge of the platform's monotonic clock rather
//! than a guessed large duration.  That makes every `None` below evidence
//! of a final `Instant + Duration` overflow, while `Duration::MAX` exercises
//! the recovery formula's intermediate arithmetic independently.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::ack::{AckAction, AckState};
use super::close::Lifecycle;
use super::frame::Ack;
use super::mobility::Contested;
use super::recovery::{Recovery, RttEstimator, SentPacket};
use super::testfix::{Solo, drain};
use super::timers::TimerKind;
use super::*;

use crate::constants;
use crate::core::Deadline;

fn duration_from_nanos(nanos: u128) -> Duration {
    let secs = u64::try_from(nanos / 1_000_000_000).expect("within Duration::MAX");
    let subsec = u32::try_from(nanos % 1_000_000_000).expect("less than one second");
    Duration::new(secs, subsec)
}

fn clock_horizon() -> Instant {
    static HORIZON: OnceLock<Instant> = OnceLock::new();
    *HORIZON.get_or_init(|| {
        let origin = Instant::now();
        assert!(
            origin.checked_add(Duration::MAX).is_none(),
            "this test needs a finite Instant horizon"
        );

        let mut representable = 0u128;
        let mut unreachable = Duration::MAX.as_nanos();
        while unreachable
            .checked_sub(representable)
            .expect("binary-search bounds stay ordered")
            > 1
        {
            let width = unreachable
                .checked_sub(representable)
                .expect("binary-search bounds stay ordered");
            let middle = representable
                .checked_add(width / 2)
                .expect("the midpoint stays inside Duration::MAX");
            if origin.checked_add(duration_from_nanos(middle)).is_some() {
                representable = middle;
            } else {
                unreachable = middle;
            }
        }

        let horizon = origin
            .checked_add(duration_from_nanos(representable))
            .expect("the lower binary-search bound is representable");
        assert!(horizon.checked_add(Duration::from_nanos(1)).is_none());
        horizon
    })
}

fn sent(counter: u64, at: Instant, size: u64) -> SentPacket {
    SentPacket {
        counter,
        time_sent: at,
        size,
        app_limited: false,
        path_gen: 0,
        frames: Vec::new(),
    }
}

fn one_counter_ack(counter: u64) -> Ack {
    Ack {
        largest: counter,
        ack_delay: 0,
        first_range: 0,
        ranges: Vec::new(),
    }
}

fn checked_after(anchor: Instant, delay: Duration) -> Instant {
    anchor
        .checked_add(delay)
        .expect("the ordinary control deadline is representable")
}

fn initial_pto() -> Duration {
    let variance = (constants::K_INITIAL_RTT / 2)
        .checked_mul(4)
        .expect("the frozen initial variance is representable")
        .max(constants::K_GRANULARITY);
    constants::K_INITIAL_RTT
        .checked_add(variance)
        .and_then(|base| base.checked_add(constants::MAX_ACK_DELAY))
        .expect("the frozen initial PTO is representable")
}

#[test]
fn liveness_remains_armed_when_its_deadline_is_beyond_the_clock_horizon() {
    let horizon = clock_horizon();
    let mut solo = Solo::installed_at(horizon);

    let liveness = solo.conn.liveness().expect("the session is installed");
    assert!(liveness.is_armed(), "the handshake still arms liveness");
    assert_eq!(liveness.last_authenticated_recv(), horizon);
    assert_eq!(
        solo.conn.timer(TimerKind::Liveness),
        None,
        "the core must not replace the unreachable death with `now`"
    );

    solo.conn.handle_timeout(horizon);
    let output = drain(&mut solo.conn);
    assert!(output.outs.is_empty(), "an unreachable timer does not fire");
    assert!(solo.conn.is_established(), "the session state is retained");
    assert!(
        solo.conn.liveness().is_some_and(|state| state.is_armed()),
        "unannounced is distinct from disabled"
    );
}

#[test]
fn passive_and_persistent_keepalives_remain_owed_without_a_fake_deadline() {
    let horizon = clock_horizon();
    let mut solo = Solo::installed_at(horizon);

    let output = solo.deliver(horizon, &[constants::FRAME_PING as u8]);
    assert!(
        solo.conn
            .liveness()
            .is_some_and(|state| state.owes_passive_keepalive()),
        "the authenticated receive records the passive debt"
    );
    assert_eq!(solo.conn.timer(TimerKind::Keepalive), None);
    assert_eq!(
        output.deadline, None,
        "the debt may not be announced at an invented earlier instant"
    );

    let interval = constants::PERSISTENT_KEEPALIVE_MIN;
    solo.conn
        .set_persistent_keepalive(horizon, Some(interval))
        .expect("the minimum interval is valid");
    assert_eq!(solo.conn.persistent_keepalive(), Some(interval));
    assert_eq!(solo.conn.timer(TimerKind::PersistentKeepalive), None);

    solo.conn.handle_timeout(horizon);
    let output = drain(&mut solo.conn);
    assert!(
        output.transmits().is_empty(),
        "neither unreachable keepalive may fire at its anchor"
    );
    assert_eq!(solo.conn.persistent_keepalive(), Some(interval));
    assert!(
        solo.conn
            .liveness()
            .is_some_and(|state| state.owes_passive_keepalive()),
        "both logical obligations survive the unannounced timeout"
    );
}

#[test]
fn a_delayed_ack_keeps_its_cadence_state_when_the_first_deadline_is_unreachable() {
    let horizon = clock_horizon();
    let mut ack = AckState::new();

    let first = ack.on_recv(horizon, 7, Some(6), true, true);
    assert_eq!(
        first,
        AckAction::Arm(Deadline::Unreachable),
        "the first in-order packet owes a delay beyond the horizon"
    );
    assert!(!ack.is_ready(), "one packet still waits for its delay");

    let second = ack.on_recv(horizon, 8, Some(7), true, true);
    assert_eq!(
        second,
        AckAction::Arm(Deadline::at(horizon)),
        "the second packet makes the retained ACK due at the drain boundary"
    );
    assert!(
        ack.is_ready(),
        "the first packet's cadence state was not lost"
    );

    let ordinary = Instant::now();
    let mut control = AckState::new();
    assert_eq!(
        control.on_recv(ordinary, 1, Some(0), true, true),
        AckAction::Arm(Deadline::at(checked_after(
            ordinary,
            constants::MAX_ACK_DELAY
        )))
    );
}

#[test]
fn a_contested_verdict_stays_armed_but_unannounced_at_the_horizon() {
    let horizon = clock_horizon();
    let mut solo = Solo::installed_at(horizon);

    solo.conn.mark_contested(horizon);
    let output = drain(&mut solo.conn);
    assert_eq!(output.transmits().len(), 1, "the probe itself still leaves");
    match solo.conn.contested() {
        Contested::Armed {
            armed_at, deadline, ..
        } => {
            assert_eq!(armed_at, horizon);
            assert_eq!(deadline, Deadline::Unreachable);
        }
        other => panic!("the transmitted probe must be retained as Armed, got {other:?}"),
    }
    assert_eq!(solo.conn.timer(TimerKind::Contested), None);

    solo.conn.handle_timeout(horizon);
    let output = drain(&mut solo.conn);
    assert!(output.closed().is_none(), "the verdict did not fire early");
    assert!(matches!(solo.conn.contested(), Contested::Armed { .. }));

    let ordinary = Instant::now();
    let mut control = Solo::installed_at(ordinary);
    control.conn.mark_contested(ordinary);
    let _ = drain(&mut control.conn);
    assert_eq!(
        control.conn.timer(TimerKind::Contested),
        Some(checked_after(
            ordinary,
            control.conn.timing_profile().passive_keepalive()
        ))
    );
}

#[test]
fn close_linger_retains_post_mortem_state_when_its_expiry_is_unreachable() {
    let horizon = clock_horizon();
    let mut solo = Solo::installed_at(horizon);

    solo.conn.close(horizon, constants::NO_ERROR, b"done");
    let output = drain(&mut solo.conn);
    assert_eq!(
        solo.conn.lifecycle.linger_deadline(),
        Some(Deadline::Unreachable)
    );
    assert_eq!(solo.conn.timer(TimerKind::CloseLinger), None);
    assert!(
        output
            .outs
            .iter()
            .any(|item| matches!(item, ConnOutput::Event(ConnEvent::Closed(_)))),
        "entering closing still surfaces its result"
    );

    solo.conn.handle_timeout(horizon);
    let output = drain(&mut solo.conn);
    assert!(
        output
            .outs
            .iter()
            .all(|item| !matches!(item, ConnOutput::ToEndpoint(_))),
        "unreachable linger must not retire state at an invented instant"
    );
    assert!(matches!(solo.conn.lifecycle, Lifecycle::Closing(_)));

    let ordinary = Instant::now();
    let mut control = Solo::installed_at(ordinary);
    control.conn.close(ordinary, constants::NO_ERROR, b"done");
    let _ = drain(&mut control.conn);
    assert_eq!(
        control.conn.timer(TimerKind::CloseLinger),
        Some(checked_after(ordinary, constants::CLOSE_LINGER))
    );
}

#[test]
fn recovery_duration_intermediates_become_unreachable_and_can_recover_later() {
    let mut rtt = RttEstimator::new();
    rtt.sample(Duration::MAX, Duration::ZERO);
    assert_eq!(
        rtt.loss_delay(),
        None,
        "9/8 of Duration::MAX must not wrap or saturate"
    );
    assert_eq!(
        rtt.pto_interval(),
        None,
        "four times the enormous variance is not representable"
    );

    for _ in 0..128 {
        rtt.sample(Duration::from_millis(1), Duration::ZERO);
    }
    assert!(
        rtt.loss_delay().is_some() && rtt.pto_interval().is_some(),
        "later ordinary samples recompute rather than permanently disabling recovery"
    );
}

#[test]
fn pto_and_loss_state_survive_unrepresentable_final_instant_additions() {
    let horizon = clock_horizon();
    let mut pto = Recovery::new();
    pto.on_sent(sent(0, horizon, 31));
    assert_eq!(pto.pto_deadline(), None);
    assert_eq!(pto.bytes_in_flight(), 31, "the sent packet is retained");
    assert!(!pto.is_empty());
    pto.on_pto_timeout();
    assert_eq!(pto.pto_deadline(), None, "backoff cannot fabricate a time");
    assert_eq!(pto.bytes_in_flight(), 31);

    let mut loss = Recovery::new();
    loss.on_sent(sent(0, horizon, 10));
    loss.on_sent(sent(2, horizon, 20));
    let outcome = loss.on_ack(horizon, &one_counter_ack(2), 2);
    assert!(outcome.lost.is_empty(), "the older packet is not yet due");
    assert_eq!(outcome.ack_events.len(), 1);
    assert_eq!(loss.bytes_in_flight(), 10, "counter 0 remains in flight");
    assert_eq!(
        loss.loss_deadline(),
        None,
        "its time-threshold deadline is beyond the clock horizon"
    );

    let ordinary = Instant::now();
    let mut pto_control = Recovery::new();
    pto_control.on_sent(sent(0, ordinary, 1));
    assert_eq!(
        pto_control.pto_deadline(),
        Some(checked_after(ordinary, initial_pto()))
    );

    let mut loss_control = Recovery::new();
    loss_control.on_sent(sent(0, ordinary, 10));
    loss_control.on_sent(sent(2, ordinary, 20));
    let outcome = loss_control.on_ack(ordinary, &one_counter_ack(2), 2);
    assert!(outcome.lost.is_empty());
    assert_eq!(
        loss_control.loss_deadline(),
        Some(checked_after(ordinary, constants::K_GRANULARITY)),
        "representable loss time stays exact"
    );
}

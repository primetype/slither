//! Acceptance tests for checked monotonic-deadline arithmetic (ruling 284).
//!
//! These tests deliberately manufacture the last future [`Instant`] this
//! platform can represent.  They never sleep: the clock horizon is found
//! with `checked_add` and binary search, then each timer family is exercised
//! on both sides of that boundary.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::guard::TimestampGuard;
use super::intro_queue::{Arrival, IntroQueue};
use super::*;

use crate::config::Config;
use crate::constants;
use crate::core::{Deadline, EndpointOutput, Timestamp};
use crate::error::ConnectError;
use crate::identity::Identity;
use crate::packet::ReferenceSuite;
use crate::testutil::CountingIdentity;

type Id = CountingIdentity<ReferenceSuite>;

fn duration_from_nanos(nanos: u128) -> Duration {
    let secs = u64::try_from(nanos / 1_000_000_000).expect("within Duration::MAX");
    let subsec = u32::try_from(nanos % 1_000_000_000).expect("less than one second");
    Duration::new(secs, subsec)
}

/// The final future instant representable from one ordinary origin.
///
/// `Instant` exposes no `MAX`, so the test finds it using only the checked
/// operation whose failure is under test.  Representability is monotone for
/// every platform implementation of `Instant`: once an offset is outside
/// the underlying clock domain, a larger one is outside too.
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
        assert!(
            horizon.checked_add(Duration::from_nanos(1)).is_none(),
            "the search must reach the one-nanosecond boundary"
        );
        horizon
    })
}

fn addr(last: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, last)), port)
}

fn checked_after(anchor: Instant, delay: Duration) -> Instant {
    anchor
        .checked_add(delay)
        .expect("the ordinary control deadline is representable")
}

fn endpoint(now: Instant, local_seed: u8) -> Endpoint<Id> {
    Endpoint::new(
        now,
        Config::default(),
        CountingIdentity::seeded([local_seed; 32]),
        [local_seed.wrapping_add(1); 32],
    )
}

fn drain(ep: &mut Endpoint<Id>) -> (Vec<EndpointOutput<ReferenceSuite>>, Option<Instant>) {
    let mut outputs = Vec::new();
    for _ in 0..100_000 {
        match ep.poll_output() {
            EndpointOutput::Timeout(deadline) => return (outputs, deadline),
            output => outputs.push(output),
        }
    }
    panic!("the endpoint drain never reached its terminal Timeout");
}

#[test]
fn an_unreachable_handshake_train_stays_pending_without_an_earlier_timeout() {
    let horizon = clock_horizon();
    let peer: Id = CountingIdentity::seeded([0x42; 32]);
    let peer_static = *peer.public_static();
    let remote = addr(42, 42_042);
    let mut ep = endpoint(horizon, 0x11);

    let (conn, _handle) = ep
        .mint_pending(horizon, remote, peer_static, ())
        .expect("a new peer can be dialled");
    assert_eq!(
        ep.next_deadline(),
        Some(horizon),
        "the first attempt is due immediately, without arithmetic"
    );

    ep.handle_timeout(horizon);
    let (outputs, announced) = drain(&mut ep);
    assert_eq!(
        outputs
            .iter()
            .filter(|output| matches!(output, EndpointOutput::Transmit(_)))
            .count(),
        1,
        "the first initiation still leaves"
    );
    assert!(
        outputs
            .iter()
            .all(|output| !matches!(output, EndpointOutput::HandshakeFailed(_, _))),
        "an arithmetic horizon is not a handshake failure"
    );
    assert_eq!(
        announced, None,
        "neither retransmit nor give-up may be clamped to an earlier instant"
    );
    assert!(
        matches!(
            ep.mint_pending(horizon, remote, peer_static, ()),
            Err(ConnectError::AlreadyConnected)
        ),
        "the unannounced train is still logically pending"
    );

    ep.handle_timeout(horizon);
    let (outputs, announced) = drain(&mut ep);
    assert!(outputs.is_empty(), "an unreachable deadline never fires");
    assert_eq!(announced, None);

    // A new anchor re-derives the interval.  The synthetic earlier anchor is
    // intentional: there is no representable instant after `horizon`; this
    // calls the same state transition with an ordinary anchor to prove the
    // unreachable value was not stored as permanent timer disablement.
    let ordinary = Instant::now();
    ep.start_attempt(ordinary, conn);
    let (_, announced) = drain(&mut ep);
    let deadline = announced.expect("the retransmit is reachable from a new anchor");
    let minimum = checked_after(ordinary, constants::RETRANSMIT_BASE);
    let maximum_delay = constants::RETRANSMIT_BASE
        .checked_add(constants::RETRANSMIT_JITTER_MAX)
        .expect("the frozen retransmit interval is representable");
    let maximum = checked_after(ordinary, maximum_delay);
    assert!(deadline >= minimum);
    assert!(deadline <= maximum);
}

#[test]
fn an_intro_beyond_the_horizon_is_retained_and_a_refresh_recomputes_its_expiry() {
    let horizon = clock_horizon();
    let source = addr(7, 7_007);
    let mut queue = IntroQueue::<Id>::new(4, 4);

    let id = match queue.arrive(horizon, source, 7, b"msg1").arrival {
        Arrival::Parked(id) => id,
        other => panic!("the first arrival must park, got {other:?}"),
    };
    assert_eq!(
        queue.get(id).map(|entry| entry.deadline()),
        Some(Deadline::Unreachable)
    );
    assert_eq!(
        queue.next_deadline(),
        None,
        "the queue must not invent an earlier expiry"
    );
    assert!(queue.expire(horizon).is_empty(), "unreachable is not due");
    assert!(
        queue.get(id).is_some(),
        "the logical expiry retains the entry"
    );

    let ordinary = Instant::now();
    assert_eq!(
        queue.arrive(ordinary, source, 8, b"newest").arrival,
        Arrival::Refreshed(id)
    );
    let expected = checked_after(ordinary, constants::INTRO_TTL);
    assert_eq!(queue.next_deadline(), Some(expected));
    assert_eq!(
        queue.expire(expected).len(),
        1,
        "the exact boundary still fires"
    );
    assert!(queue.get(id).is_none());
}

#[test]
fn guard_aging_and_exemption_keep_unreachable_state_without_sweeping_it() {
    let horizon = clock_horizon();
    let peer = b"peer-at-the-clock-horizon";
    let stamp = Timestamp::new(17, 23);
    let mut guard = TimestampGuard::default();

    let _undo = guard.record(peer, stamp, horizon);
    assert_eq!(guard.greatest(peer), Some(stamp));
    assert_eq!(
        guard.next_orphan_deadline(),
        None,
        "orphan TTL overflow is unannounced, not clamped"
    );
    guard.age_orphans(horizon);
    assert_eq!(
        guard.greatest(peer),
        Some(stamp),
        "an unreachable orphan deadline must not sweep the record"
    );

    let ordinary = Instant::now();
    let protected = b"peer-with-unreachable-exemption";
    let protected_stamp = Timestamp::new(18, 24);
    let _undo = guard.record(protected, protected_stamp, ordinary);
    guard.extend_exemption(
        protected,
        Deadline::after(horizon, constants::HANDSHAKE_GIVEUP),
    );
    assert_eq!(
        guard.next_orphan_deadline(),
        None,
        "an unreachable exemption outranks the ordinary orphan TTL"
    );
    let after_ordinary_delay = constants::TS_GUARD_ORPHAN_TTL
        .checked_add(constants::HANDSHAKE_GIVEUP)
        .expect("the frozen guard windows are representable");
    let after_ordinary_ttl = checked_after(ordinary, after_ordinary_delay);
    guard.age_orphans(after_ordinary_ttl);
    assert_eq!(
        guard.greatest(protected),
        Some(protected_stamp),
        "enabled-but-unreachable exemption state is not the same as absent"
    );
}

#[test]
fn representable_endpoint_deadlines_keep_their_exact_boundaries() {
    let ordinary = Instant::now();
    let mut queue = IntroQueue::<Id>::new(2, 2);
    let source = addr(9, 9_009);
    let id = match queue.arrive(ordinary, source, 9, b"msg1").arrival {
        Arrival::Parked(id) => id,
        other => panic!("the first arrival must park, got {other:?}"),
    };
    let expiry = checked_after(ordinary, constants::INTRO_TTL);
    assert_eq!(queue.next_deadline(), Some(expiry));
    let just_before = expiry
        .checked_sub(Duration::from_nanos(1))
        .expect("the ordinary expiry is after the clock origin");
    assert!(queue.expire(just_before).is_empty());
    assert_eq!(queue.expire(expiry).len(), 1);
    assert!(queue.get(id).is_none());
}

//! Acceptance tests for the application-configurable connection timing profile.
//!
//! These tests exercise the public timing-profile API ratified by ruling 282
//! and its validation strengthened by ruling 283.
//!
//! **Provenance (corrected by ruling 286):** this file first claimed it was
//! "authored independently from the implementation". It was not — tests and
//! implementation arrived together, one external author, one commit (PR #1,
//! `fcc182a`), so `required_dead_margin` below re-derives ruling 283's
//! relation with the same checked chain as `src/config.rs` and is **not** an
//! outside check on it (working rule 6's mutually-consistent shape). The
//! independent re-derivation of the boundary values, done from the ruling
//! text alone, is recorded in ruling 286.

use std::fmt::Debug;
use std::time::Duration;

use slither::config::{Config, TimingProfile, TimingProfileError};
use slither::constants::{
    DEAD_TIMEOUT, K_INITIAL_RTT, KEEPALIVE_TIMEOUT, PERSISTENT_KEEPALIVE_MIN, SHELL_LATENESS_BOUND,
};

const NS: Duration = Duration::from_nanos(1);

fn require_value_traits<T: Copy + Eq + Debug>() {}

fn required_dead_margin(passive: Duration) -> Duration {
    passive
        .checked_mul(2)
        .and_then(|twice| twice.checked_add(K_INITIAL_RTT))
        .and_then(|with_rtt| with_rtt.checked_add(SHELL_LATENESS_BOUND))
        .and_then(|with_first_shell| with_first_shell.checked_add(SHELL_LATENESS_BOUND))
        .expect("the test's small passive interval cannot overflow")
}

/// Ruling 282 keeps v1 as the value-object default. This assertion is against
/// the public constants, rather than duplicate literals, so a value drift is
/// still caught by the existing constant pins.
#[test]
fn the_default_profile_is_exactly_v1_and_has_value_semantics() {
    require_value_traits::<TimingProfile>();

    let profile = TimingProfile::default();
    let copied = profile;

    assert_eq!(copied, profile);
    assert_eq!(profile.passive_keepalive(), KEEPALIVE_TIMEOUT);
    assert_eq!(profile.dead_timeout(), DEAD_TIMEOUT);
    assert!(
        format!("{profile:?}").contains("TimingProfile"),
        "Debug should identify the public value object"
    );
}

/// Ruling 283 keeps the profile valid by construction. Every side of both
/// inequalities is pinned, including the strict instant at which the dead
/// timer would race an answer to the second passive keepalive after both
/// shells have consumed their permitted delivery lateness.
#[test]
fn construction_enforces_the_flood_floor_and_strict_liveness_margin() {
    let passive = PERSISTENT_KEEPALIVE_MIN;
    let former_margin = passive
        .checked_mul(2)
        .and_then(|twice| twice.checked_add(K_INITIAL_RTT))
        .and_then(|with_rtt| with_rtt.checked_add(SHELL_LATENESS_BOUND))
        .expect("the former one-shell margin is representable");
    let exact_margin = required_dead_margin(passive);

    assert_eq!(
        exact_margin,
        former_margin + SHELL_LATENESS_BOUND,
        "the strengthened relation budgets shell lateness on both sides"
    );

    assert!(matches!(
        TimingProfile::try_new(passive - NS, Duration::from_secs(10)),
        Err(TimingProfileError::KeepaliveTooShort)
    ));

    assert!(matches!(
        TimingProfile::try_new(passive, exact_margin - NS),
        Err(TimingProfileError::DeadTimeoutTooShort)
    ));
    assert!(matches!(
        TimingProfile::try_new(passive, former_margin + NS),
        Err(TimingProfileError::DeadTimeoutTooShort)
    ));
    assert!(matches!(
        TimingProfile::try_new(passive, exact_margin),
        Err(TimingProfileError::DeadTimeoutTooShort)
    ));

    let profile = TimingProfile::try_new(passive, exact_margin + NS)
        .expect("one nanosecond beyond the strict margin is admissible");
    assert_eq!(profile.passive_keepalive(), passive);
    assert_eq!(profile.dead_timeout(), exact_margin + NS);
}

/// **[ruling 286]** Ruling 283 names two short application profiles as
/// remaining valid beside the default, and until this pin only 2 s / 5 s
/// was asserted anywhere. Both clear the strengthened margin by exactly
/// 167 ms — less than one `SHELL_LATENESS_BOUND` — so they are the named
/// values most sensitive to any future strengthening of the relation, and
/// the independent re-derivation flagged them for precisely that reason.
#[test]
fn the_named_short_application_profiles_are_admitted() {
    for (keepalive, dead) in [(1, 3), (2, 5)] {
        let profile =
            TimingProfile::try_new(Duration::from_secs(keepalive), Duration::from_secs(dead))
                .expect("ruling 283 names this profile as remaining valid");
        assert_eq!(profile.passive_keepalive(), Duration::from_secs(keepalive));
        assert_eq!(profile.dead_timeout(), Duration::from_secs(dead));
    }
}

/// Checked arithmetic is observable API behaviour, not merely an
/// implementation precaution. These cases cover doubling, the fixed
/// allowances (including the newly required second shell allowance), and
/// construction of the eventual monotonic deadline.
#[test]
fn construction_reports_arithmetic_overflow() {
    assert!(matches!(
        TimingProfile::try_new(Duration::MAX, Duration::MAX),
        Err(TimingProfileError::ArithmeticOverflow)
    ));
    assert!(matches!(
        TimingProfile::try_new(Duration::MAX / 2, Duration::MAX),
        Err(TimingProfileError::ArithmeticOverflow)
    ));

    let room_before_second_shell = Duration::MAX
        .checked_sub(K_INITIAL_RTT)
        .and_then(|room| room.checked_sub(SHELL_LATENESS_BOUND))
        .expect("the fixed allowances fit within Duration::MAX");
    let overflows_only_on_second_shell = room_before_second_shell
        .checked_div(2)
        .expect("division by two is defined");
    let through_first_shell = overflows_only_on_second_shell
        .checked_mul(2)
        .and_then(|twice| twice.checked_add(K_INITIAL_RTT))
        .and_then(|with_rtt| with_rtt.checked_add(SHELL_LATENESS_BOUND));
    assert!(
        through_first_shell.is_some(),
        "the regression value must survive the former one-shell relation"
    );
    assert!(
        through_first_shell
            .and_then(|margin| margin.checked_add(SHELL_LATENESS_BOUND))
            .is_none(),
        "the regression value must overflow specifically at the second shell allowance"
    );
    assert!(matches!(
        TimingProfile::try_new(overflows_only_on_second_shell, Duration::MAX),
        Err(TimingProfileError::ArithmeticOverflow)
    ));

    assert!(matches!(
        TimingProfile::try_new(PERSISTENT_KEEPALIVE_MIN, Duration::MAX),
        Err(TimingProfileError::ArithmeticOverflow)
    ));
}

#[test]
fn config_owns_one_endpoint_wide_profile() {
    let default = TimingProfile::default();
    assert_eq!(Config::new().timing_profile(), default);

    let custom = TimingProfile::try_new(Duration::from_secs(2), Duration::from_secs(5))
        .expect("5 s satisfies ruling 283's strengthened two-shell margin");
    let config = Config::new().with_timing_profile(custom);

    assert_eq!(config.timing_profile(), custom);
}

// The remaining tests need the public kernel-free network. Keeping only this
// module behind `test-util` lets ordinary `cargo test` still compile and run
// the public value-object tests; the full acceptance command is:
//
//     cargo test --features test-util --test story_timing_profile
#[cfg(feature = "test-util")]
mod flow {
    use std::future::Future;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::pin::pin;
    use std::task::Poll;

    use slither::constants::{HANDSHAKE_GIVEUP, INTRO_TTL, MAX_ACK_DELAY};
    use slither::error::{AcceptError, ConfigError, ConnectError, ConnectionLost, IntroError};
    use slither::identity::{Identity, PublicKeyOf};
    use slither::shell::Notification;
    use slither::testutil::{
        CountingIdentity, FlakyPolicy, Network, Pair, Spied, Tap, TestSendStream, addr_c, settle,
    };
    use tokio::time::Instant;

    use super::*;

    type Suite = slither::packet::ReferenceSuite;
    type Id = CountingIdentity<Suite>;
    type Pk = PublicKeyOf<Id>;
    type Endpoint = slither::shell::Endpoint<Id>;
    type Connection = slither::shell::Connection<Suite>;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    struct Node {
        endpoint: Endpoint,
        public_static: Pk,
        address: SocketAddr,
    }

    impl Node {
        fn spawn(net: &Network, key_seed: u8, port: u16, config: Config) -> Self {
            let address = addr(port);
            let identity: Id = CountingIdentity::seeded([key_seed; 32]);
            let public_static = *identity.public_static();
            let endpoint = Endpoint::builder()
                .identity(identity)
                .wire(net.wire(address))
                .config(config)
                .build();
            Self {
                endpoint,
                public_static,
                address,
            }
        }
    }

    async fn establish(dialler: &Node, listener: &Node) -> (Connection, Connection) {
        let dial = dialler
            .endpoint
            .connect(listener.address, listener.public_static)
            .expect("the remote static is not already connected");
        let accept = async {
            let intro = listener
                .endpoint
                .accept()
                .await
                .expect("the endpoint remains live");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");
            proven.accept().await.expect("accept")
        };

        let (dialled, accepted) = tokio::join!(dial, accept);
        (
            dialled.expect("the dial completes over the perfect fixture"),
            accepted,
        )
    }

    async fn staged_accept(node: &Node) -> Result<Connection, AcceptError> {
        let intro = node
            .endpoint
            .accept()
            .await
            .expect("the endpoint remains live");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await
    }

    fn sent_from(tap: &Tap, from: SocketAddr) -> usize {
        tap.datagrams()
            .iter()
            .filter(|(source, _destination, _bytes)| *source == from)
            .count()
    }

    async fn advance_to(instant: Instant) {
        let now = Instant::now();
        assert!(
            instant >= now,
            "the test never moves virtual time backwards"
        );
        tokio::time::advance(instant - now).await;
        settle().await;
    }

    async fn poll_once<F: Future>(mut future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
        std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
    }

    async fn assert_still_open(connection: &Connection, why: &str) {
        let mut closed = pin!(connection.closed());
        assert!(
            poll_once(closed.as_mut()).await.is_pending(),
            "the connection closed early: {why}"
        );
    }

    async fn assert_timed_out(connection: &Connection, why: &str) {
        let mut closed = pin!(connection.closed());
        assert_eq!(
            poll_once(closed.as_mut()).await,
            Poll::Ready(ConnectionLost::TimedOut),
            "{why}"
        );
    }

    async fn notified_within(
        duration: Duration,
        connection: &Connection,
        why: &str,
    ) -> Notification {
        tokio::time::timeout(duration, connection.notified())
            .await
            .unwrap_or_else(|_| panic!("no notification within {duration:?}: {why}"))
            .unwrap_or_else(|lost| panic!("the connection ended with {lost:?}: {why}"))
    }

    fn last_datagram(snapshot: &[Spied], from: SocketAddr, to: SocketAddr) -> Vec<u8> {
        snapshot
            .iter()
            .rev()
            .find(|datagram| datagram.src == from && datagram.dst == to)
            .map(|datagram| datagram.bytes.clone())
            .unwrap_or_default()
    }

    async fn write_stream_all(stream: &mut TestSendStream, bytes: &[u8], why: &str) {
        let mut written = 0usize;
        while written < bytes.len() {
            let count =
                tokio::time::timeout(Duration::from_secs(5), stream.write(&bytes[written..]))
                    .await
                    .unwrap_or_else(|_| panic!("the stream write remained pending: {why}"))
                    .unwrap_or_else(|error| {
                        panic!("the stream write failed with {error:?}: {why}")
                    });
            assert!(count > 0, "a blocked write must be Pending: {why}");
            written += count;
        }
    }

    fn profile(passive_secs: u64, dead_secs: u64) -> TimingProfile {
        TimingProfile::try_new(
            Duration::from_secs(passive_secs),
            Duration::from_secs(dead_secs),
        )
        .expect("the fixture profile satisfies ruling 283's strict margin")
    }

    /// Different local profiles still establish one v1 connection. The
    /// dialled and accepted halves then reach their own receive-anchored
    /// verdicts, proving both propagation paths and the absence of timing
    /// negotiation on the wire.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn asymmetric_profiles_are_wire_compatible_and_make_local_death_decisions() {
        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let fast = profile(1, 3);
                let slow = profile(2, 5);
                let a = Node::spawn(&net, 1, 7601, Config::new().with_timing_profile(fast));
                let b = Node::spawn(&net, 2, 7602, Config::new().with_timing_profile(slow));
                let (dialled, accepted) = establish(&a, &b).await;
                let installed = Instant::now();
                settle().await;

                advance_to(installed + fast.dead_timeout() - Duration::from_millis(1)).await;
                assert_still_open(&dialled, "the dialled half must survive to its deadline").await;
                assert_still_open(&accepted, "the accepted half has the slower local profile")
                    .await;

                advance_to(installed + fast.dead_timeout() + SHELL_LATENESS_BOUND).await;
                assert_timed_out(
                    &dialled,
                    "the dialled connection must inherit its endpoint's 3 s dead timeout",
                )
                .await;
                assert_still_open(
                    &accepted,
                    "no wire negotiation may replace the acceptor's 5 s local timeout",
                )
                .await;

                advance_to(installed + slow.dead_timeout() + SHELL_LATENESS_BOUND).await;
                assert_timed_out(
                    &accepted,
                    "the accepted connection must inherit its endpoint's 5 s dead timeout",
                )
                .await;
            })
            .await;
    }

    /// A `None`-basis refusal arms the contested verdict for the effective
    /// passive interval, not v1's fixed 10 seconds. Ordinary liveness is
    /// deliberately later, so a missing/customisation bug cannot pass by
    /// dying for the wrong reason.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_custom_profile_sets_the_contested_verdict_deadline() {
        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(2, 5);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 11, 7651, config.clone());
                let b = Node::spawn(&net, 12, 7652, config.clone());
                let (ca, cb) = establish(&a, &b).await;

                // Model a crash, not a graceful close: the endpoint handle
                // goes first and the coincident final connection drop emits
                // nothing. A fresh endpoint then presents the same static.
                drop(b.endpoint);
                drop(cb);
                settle().await;
                let restarted = Node::spawn(&net, 12, 7653, config);

                let refused = {
                    let mut dial = pin!(
                        restarted
                            .endpoint
                            .connect(a.address, a.public_static)
                            .expect("the restarted endpoint has no live connection")
                    );
                    tokio::select! {
                        result = dial.as_mut() => {
                            panic!("the live None-basis connection must refuse this dial: {result:?}")
                        }
                        result = staged_accept(&a) => result,
                    }
                };
                assert_eq!(refused.err(), Some(AcceptError::Stale));

                let marked = notified_within(
                    SHELL_LATENESS_BOUND * 4,
                    &ca,
                    "the admitted None-basis refusal must mark the live connection",
                )
                .await;
                assert_eq!(marked, Notification::Contested);
                let armed_at = Instant::now();

                advance_to(
                    armed_at + custom.passive_keepalive() - Duration::from_millis(1),
                )
                .await;
                assert_still_open(
                    &ca,
                    "the contested verdict may not fire before the effective passive interval",
                )
                .await;

                advance_to(
                    armed_at + custom.passive_keepalive() + SHELL_LATENESS_BOUND,
                )
                .await;
                assert_timed_out(
                    &ca,
                    "the custom 2 s contested deadline must beat both 5 s liveness and v1's 10 s",
                )
                .await;
            })
            .await;
    }

    async fn one_exchange(sender: &Connection, receiver: &Connection, payload: &[u8]) {
        sender.send_message(payload).await.expect("send_message");
        assert_eq!(
            receiver.recv_message().await.expect("recv_message"),
            payload
        );
        sender
            .acked()
            .await
            .expect("the perfect fixture acknowledges");
        settle().await;
    }

    /// Pin the custom passive cadence on the connection returned to the
    /// dialler. Advancing to 500 ms first drains delayed ACK/control work so
    /// the assertion cannot mistake that traffic for a keepalive.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn the_dialled_connection_uses_the_configured_passive_deadline() {
        let net = Network::new();
        let tap = net.tap();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 4);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 3, 7611, config.clone());
                let b = Node::spawn(&net, 4, 7612, config);
                let (dialled, accepted) = establish(&a, &b).await;
                let installed = Instant::now();

                one_exchange(&accepted, &dialled, b"toward dialler").await;
                advance_to(installed + Duration::from_millis(500)).await;
                let before = sent_from(&tap, a.address);

                advance_to(installed + custom.passive_keepalive() - Duration::from_millis(1)).await;
                assert_eq!(
                    sent_from(&tap, a.address),
                    before,
                    "the dialled half must not keepalive before its configured deadline"
                );

                advance_to(installed + custom.passive_keepalive() + SHELL_LATENESS_BOUND).await;
                assert!(
                    sent_from(&tap, a.address) > before,
                    "the dialled half owes a passive keepalive at the configured deadline"
                );
            })
            .await;
    }

    /// The mirror path: a profile must be stamped when a connection is born
    /// from staged acceptance, not only when it is minted by `connect()`.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn the_accepted_connection_uses_the_configured_passive_deadline() {
        let net = Network::new();
        let tap = net.tap();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 4);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 5, 7621, config.clone());
                let b = Node::spawn(&net, 6, 7622, config);
                let (dialled, accepted) = establish(&a, &b).await;
                let installed = Instant::now();

                one_exchange(&dialled, &accepted, b"toward acceptor").await;
                advance_to(installed + Duration::from_millis(500)).await;
                let before = sent_from(&tap, b.address);

                advance_to(installed + custom.passive_keepalive() - Duration::from_millis(1)).await;
                assert_eq!(
                    sent_from(&tap, b.address),
                    before,
                    "the accepted half must not keepalive before its configured deadline"
                );

                advance_to(installed + custom.passive_keepalive() + SHELL_LATENESS_BOUND).await;
                assert!(
                    sent_from(&tap, b.address) > before,
                    "the accepted half owes a passive keepalive at the configured deadline"
                );
            })
            .await;
    }

    /// Ruling 282 changes established-session liveness only. An unanswered
    /// handshake therefore outlives a deliberately short profile and still
    /// gives up at the fixed 90-second protocol deadline.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_short_profile_does_not_shorten_handshake_give_up() {
        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 3);
                let a = Node::spawn(&net, 13, 7661, Config::new().with_timing_profile(custom));
                let absent: Id = CountingIdentity::seeded([14; 32]);
                let absent_static = *absent.public_static();
                let started = Instant::now();
                let mut dial = pin!(
                    a.endpoint
                        .connect(addr(7662), absent_static)
                        .expect("the absent static is not already connected")
                );

                assert!(poll_once(dial.as_mut()).await.is_pending());
                advance_to(started + custom.dead_timeout() + SHELL_LATENESS_BOUND).await;
                assert!(
                    poll_once(dial.as_mut()).await.is_pending(),
                    "D_eff is not application dial patience"
                );

                advance_to(started + HANDSHAKE_GIVEUP - Duration::from_millis(1)).await;
                assert!(
                    poll_once(dial.as_mut()).await.is_pending(),
                    "the fixed handshake train must survive until HANDSHAKE_GIVEUP"
                );

                advance_to(started + HANDSHAKE_GIVEUP + SHELL_LATENESS_BOUND).await;
                assert!(
                    matches!(
                        poll_once(dial.as_mut()).await,
                        Poll::Ready(Err(ConnectError::TimedOut))
                    ),
                    "the unchanged handshake deadline still resolves TimedOut"
                );
            })
            .await;
    }

    /// A held introduction is endpoint state, not an established session.
    /// Two chains parked together pin both sides of the unchanged 15-second
    /// TTL while the listener uses a 1 s / 3 s liveness profile.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_short_profile_does_not_shorten_intro_ttl() {
        const EDGE: Duration = Duration::from_millis(100);

        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 3);
                let config = Config::new().with_timing_profile(custom);
                let listener = Node::spawn(&net, 15, 7671, config.clone());
                let first = Node::spawn(&net, 16, 7672, config.clone());
                let second = Node::spawn(&net, 17, 7673, config);

                let first_dial = first
                    .endpoint
                    .connect(listener.address, listener.public_static)
                    .expect("first dial");
                let first_intro = listener
                    .endpoint
                    .accept()
                    .await
                    .expect("first parked introduction");
                drop(first_dial);

                let second_dial = second
                    .endpoint
                    .connect(listener.address, listener.public_static)
                    .expect("second dial");
                let second_intro = listener
                    .endpoint
                    .accept()
                    .await
                    .expect("second parked introduction");
                drop(second_dial);
                let parked_at = Instant::now();

                advance_to(parked_at + INTRO_TTL - EDGE).await;
                let still_valid =
                    tokio::time::timeout(SHELL_LATENESS_BOUND * 4, first_intro.read_identity())
                        .await
                        .expect("read_identity answered before the TTL");
                assert!(
                    still_valid.is_ok(),
                    "an intro must remain takeable until the fixed INTRO_TTL"
                );

                advance_to(parked_at + INTRO_TTL + EDGE).await;
                let expired =
                    tokio::time::timeout(SHELL_LATENESS_BOUND * 4, second_intro.read_identity())
                        .await
                        .expect("read_identity answered after the TTL");
                assert!(matches!(expired, Err(IntroError::Expired)));
            })
            .await;
    }

    /// One ack-eliciting datagram arms the fixed delayed-ACK timer. The
    /// receiver uses a much longer 1-second passive interval, so output at
    /// 25 ms distinguishes recovery policy from liveness policy.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_short_profile_does_not_change_the_ack_delay() {
        let net = Network::new();
        let tap = net.tap();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 3);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 18, 7681, config.clone());
                let b = Node::spawn(&net, 19, 7682, config);
                let (dialled, accepted) = establish(&a, &b).await;
                settle().await;

                dialled.send_datagram(b"ack me").expect("send_datagram");
                let received = accepted.recv_datagram().await.expect("recv_datagram");
                assert_eq!(received, b"ack me");
                settle().await;
                let received_at = Instant::now();
                let before_ack = sent_from(&tap, b.address);

                advance_to(received_at + MAX_ACK_DELAY - Duration::from_millis(1)).await;
                assert_eq!(
                    sent_from(&tap, b.address),
                    before_ack,
                    "the first ack-eliciting packet is delayed until MAX_ACK_DELAY"
                );

                advance_to(received_at + MAX_ACK_DELAY + SHELL_LATENESS_BOUND).await;
                assert!(
                    sent_from(&tap, b.address) > before_ack,
                    "the ACK must leave on the fixed recovery timer, not K_eff or D_eff"
                );
            })
            .await;
    }

    /// The beacon keeps its old inclusive floor, while its exclusive ceiling
    /// follows the effective profile on both connection birth paths. A
    /// rejected update must leave the last accepted value untouched.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn persistent_keepalive_uses_each_connections_effective_dead_timeout() {
        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(1, 3);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 7, 7631, config.clone());
                let b = Node::spawn(&net, 8, 7632, config);
                let (dialled, accepted) = establish(&a, &b).await;
                let last_inside = custom.dead_timeout() - NS;

                for (connection, birth) in [(&dialled, "dialled"), (&accepted, "accepted")] {
                    assert_eq!(
                        connection.set_persistent_keepalive(Some(PERSISTENT_KEEPALIVE_MIN)),
                        Ok(()),
                        "the 1 s floor is inclusive on the {birth} connection"
                    );
                    assert_eq!(
                        connection.set_persistent_keepalive(Some(last_inside)),
                        Ok(()),
                        "one nanosecond below the effective dead timeout is admissible"
                    );
                    assert_eq!(connection.persistent_keepalive(), Some(last_inside));

                    assert_eq!(
                        connection.set_persistent_keepalive(Some(custom.dead_timeout())),
                        Err(ConfigError::KeepaliveTooLong),
                        "the effective dead timeout itself is excluded on the {birth} connection"
                    );
                    assert_eq!(
                        connection.persistent_keepalive(),
                        Some(last_inside),
                        "a rejected ceiling value must leave the prior setting unchanged"
                    );
                    assert_eq!(
                        connection.set_persistent_keepalive(Some(PERSISTENT_KEEPALIVE_MIN - NS)),
                        Err(ConfigError::KeepaliveTooShort),
                        "the existing 1 s flood floor remains exclusive below"
                    );
                }
            })
            .await;
    }

    /// The public-shell transpose of ruling 265's starved-budget fixture.
    /// The peer fragments the replay window, fills the sender's flight, then
    /// roams onto an address that never answers. The resulting pure ACK
    /// spends the amplification room, so the passive keepalive is owed but
    /// cannot leave. The fallback death must use this profile's 12 seconds,
    /// not v1's fixed 25 seconds.
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_vetoed_keepalive_uses_the_effective_dead_timeout_backstop() {
        const FRAGMENTS: u32 = 160;
        const PTO_TRAIN: Duration = Duration::from_secs(8);

        let custom = profile(5, 12);
        let config = Config::new().with_timing_profile(custom);
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let pair = Pair::seeded_with(0x41_0282, config);
                let tap = pair.net.tap();
                let (ca, cb) = pair.establish().await;
                let a_address = pair.a.addr();
                let b_address = pair.b.addr();

                // Warm one bidi stream in both directions.
                let bi_a = tokio::time::timeout(Duration::from_secs(5), ca.open_bi())
                    .await
                    .expect("open_bi resolved")
                    .expect("open_bi");
                let (mut send_a, mut _recv_a) = bi_a.split();
                write_stream_all(&mut send_a, b"hello", "warm a to b").await;
                let bi_b = tokio::time::timeout(Duration::from_secs(5), cb.accept_bi())
                    .await
                    .expect("accept_bi resolved")
                    .expect("accept_bi");
                let (mut send_b, mut recv_b) = bi_b.split();
                let mut warm = [0u8; 16];
                let _ = tokio::time::timeout(Duration::from_secs(5), recv_b.read(&mut warm))
                    .await
                    .expect("warm read resolved")
                    .expect("warm read");
                settle().await;

                // Lost small packets leave enough replay-window gaps for a
                // pure ACK to consume all room funded by the later roam.
                pair.b.wire.set_policy(FlakyPolicy::lossy(0.5));
                for fragment in 0..FRAGMENTS {
                    write_stream_all(
                        &mut send_b,
                        &[fragment as u8; 4],
                        "fragment the replay window",
                    )
                    .await;
                    settle().await;
                }

                pair.b.wire.set_policy(FlakyPolicy::lossy(1.0));
                settle().await;

                // Fill A's congestion flight. Its PTO probes are exempt
                // from cwnd and finish shutting the gate during this fixed
                // eight-second virtual-time train.
                let bulk = vec![0xabu8; 200 * 1024];
                let mut written = 0usize;
                while written < bulk.len() {
                    match tokio::time::timeout(
                        Duration::from_millis(50),
                        send_a.write(&bulk[written..]),
                    )
                    .await
                    {
                        Ok(Ok(count)) => written += count,
                        _ => break,
                    }
                }
                assert!(written > 0, "the fixture must put data in flight");
                let _ = tokio::time::timeout(PTO_TRAIN, std::future::pending::<()>()).await;

                // Capture a fresh B→A packet after the wire has tapped and
                // dropped it, then inject the same authenticated bytes from
                // a dead address to drive receive-based roaming.
                let before = tap.snapshot().len();
                write_stream_all(&mut send_b, b"roam", "mint the roam trigger").await;
                settle().await;
                let fresh = last_datagram(&tap.snapshot()[before..], b_address, a_address);
                assert!(!fresh.is_empty(), "the tap must see a fresh roam trigger");
                pair.net.inject(addr_c(), a_address, &fresh);
                settle().await;
                assert_eq!(
                    ca.remote_address(),
                    addr_c(),
                    "the authenticated packet must re-home A onto the dead address"
                );

                let roam_at = Instant::now();
                let sent_at_roam = tap.snapshot().len();

                let early = tokio::time::timeout(
                    custom.dead_timeout() - Duration::from_millis(1),
                    ca.closed(),
                )
                .await;
                assert!(
                    early.is_err(),
                    "the backstop is receive-anchored and may not fire before D_eff"
                );

                let lost = tokio::time::timeout(
                    Duration::from_millis(1) + SHELL_LATENESS_BOUND,
                    ca.closed(),
                )
                .await
                .expect("the vetoed keepalive must retain a liveness deadline");
                assert_eq!(lost, ConnectionLost::TimedOut);
                assert!(
                    Instant::now() - roam_at <= custom.dead_timeout() + SHELL_LATENESS_BOUND,
                    "the backstop used v1's 25 s deadline instead of D_eff"
                );

                let snapshot = tap.snapshot();
                let later_output: Vec<_> = snapshot[sent_at_roam..]
                    .iter()
                    .filter(|datagram| datagram.src == a_address)
                    .cloned()
                    .collect();
                assert!(
                    later_output.is_empty(),
                    "the premise is a keepalive held behind the gates; A unexpectedly sent \
                     {later_output:?} after the roam"
                );
            })
            .await;
    }

    /// `Service::call` on the owned connection mints an internal accounted
    /// handle. That must remain possible after linger has released the core:
    /// the public connection handle still carries everything cloning needs,
    /// and a call reports the latched connection error instead of panicking
    /// while trying to recover configuration from absent core state.
    #[cfg(feature = "tower")]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn tower_call_after_linger_returns_the_latched_error_without_panicking() {
        use slither::constants::{CLOSE_LINGER, NO_ERROR};
        use tower::Service;

        let net = Network::new();
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let custom = profile(2, 6);
                let config = Config::new().with_timing_profile(custom);
                let a = Node::spawn(&net, 9, 7641, config.clone());
                let b = Node::spawn(&net, 10, 7642, config);
                let (mut dialled, _accepted) = establish(&a, &b).await;

                let close_at = Instant::now();
                tokio::time::timeout(
                    SHELL_LATENESS_BOUND,
                    dialled.close(NO_ERROR, b"tower regression"),
                )
                .await
                .expect("close resolves at the seal, within the fixed shell-lateness bound");
                assert_eq!(dialled.closed().await, ConnectionLost::LocallyClosed);

                // `Closed` is latched at the start of close, while the core
                // remains through §15.2's linger. Crossing this deadline is
                // what makes the regression meaningful: `release_dead`
                // removes the core, but the public handle remains callable.
                advance_to(close_at + CLOSE_LINGER - Duration::from_millis(1)).await;
                assert!(
                    dialled.is_established(),
                    "a short liveness profile must not shorten CLOSE_LINGER"
                );
                advance_to(close_at + CLOSE_LINGER + SHELL_LATENESS_BOUND).await;
                assert!(
                    !dialled.is_established(),
                    "the core must be released after the unchanged CLOSE_LINGER"
                );

                for attempt in 1..=2 {
                    let opened = <Connection as Service<()>>::call(&mut dialled, ()).await;
                    assert_eq!(
                        opened.err(),
                        Some(ConnectionLost::LocallyClosed),
                        "tower call {attempt} after core release must return the latched error"
                    );
                }
            })
            .await;
    }
}

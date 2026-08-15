//! The I/O shell — `SPEC.md` §16.
//!
//! The cores are sans-io state machines that never read a clock; the shell
//! is the single `!Send` tokio actor that gives them a socket and a timer,
//! and the handles the application holds.
//!
//! ```text
//! Endpoint ──connect()──▶ Connecting ──▶ Connection
//!    └──────accept()───▶ Intro ─▶ Claimed ─▶ Proven ─▶ Connection
//!            (0 DH)      (1 DH)    (2 DH)     (4 DH)
//! ```
//!
//! # A `LocalSet` is required
//!
//! [`Endpoint::builder`]'s `build()` calls `tokio::task::spawn_local`, so
//! the endpoint must be built inside a `tokio::task::LocalSet` on a
//! current-thread runtime. **No `Send` bound may be added to this path.**
//! §16.3 is explicit about why: a DH provider is not required to be `Send`
//! — an iOS Secure Enclave key is the story that forbids it — and neither
//! is a [`Wire`](wire::Wire). The shell's types hold `Rc`s, so the
//! invariant is enforced by the type system rather than by review.
//!
//! # The seam, in one paragraph (§16.3, ruling 53)
//!
//! Endpoint verbs — `accept()` and §6.2's three staged verbs — are a
//! command channel plus a oneshot reply, because §6.2 requires the DH costs
//! to land on the driver task. `connect()` is the exception (ruling 87): it
//! is not `async`, performs no DH, and answers §16.1's NONE/PENDING/LIVE
//! test from a **synchronous read of the shared cell**. The connection data
//! path and the four accessors share an `Rc<RefCell<_>>` with the driver;
//! each data-path verb is written once as `poll_*`, and the `async fn` is
//! `poll_fn` over it.
//!
//! # What is here, and what is not
//!
//! Slice 3 builds `connect`/`accept`, the staged ladder, `close()`,
//! `closed()` and the four accessors. §16.2's stream, message, datagram,
//! `acked()`, `notified()` and keepalive verbs arrive with the slices that
//! define them and are **absent rather than stubbed**: in this crate an
//! unimplemented verb is a claim about the protocol.

pub mod wire;

mod connection;
mod driver;
mod endpoint;
mod shared;
mod staged;

pub use self::connection::Connection;
pub use self::endpoint::{Connecting, Endpoint, EndpointBuilder};
pub use self::staged::{Claimed, Intro, Proven};

#[cfg(test)]
mod tests {
    //! Implementation smoke tests — **not** slice 3b's story tests.
    //!
    //! `tests/story_lifecycle.rs`, `tests/story_dial.rs` and
    //! `tests/spec_shell.rs` are the acceptance criteria and were written
    //! independently (working rule 6). What is here exercises the *seam*
    //! from inside: the borrow discipline, the drop rules, the latch, and
    //! the cancel-then-redial ordering — the five things round 7 found
    //! defects in.

    use std::time::Duration;

    use crate::error::{ConnectError, ConnectionLost};
    use crate::testutil::{Pair, local, settle};

    /// The whole ladder, end to end: a dial and a staged accept complete,
    /// both sides get a `Connection`, and the initiator paid §6.1's 4 DH.
    #[tokio::test(start_paused = true)]
    async fn a_dial_and_a_staged_accept_meet() {
        local(async {
            let pair = Pair::seeded(0x5117E5);
            let (a, b) = pair.establish().await;

            assert!(a.is_established());
            assert!(b.is_established());
            assert_eq!(a.remote_address(), pair.b.addr);
            assert_eq!(a.remote_static().as_ref(), pair.b.public_static.as_ref());
            assert_eq!(b.remote_static().as_ref(), pair.a.public_static.as_ref());
            // Ruling 89: both peers of one session produce the same value.
            assert_eq!(a.session_id(), b.session_id());
            assert_eq!(pair.a.dhs.get(), 4, "§6.1's initiator ladder is 4 DH");
        })
        .await;
    }

    /// `close()` resolves at the seal (§16.2), and the peer surfaces
    /// `PeerClosed` with the same code and reason.
    #[tokio::test(start_paused = true)]
    async fn a_close_reaches_the_peer_as_peer_closed() {
        local(async {
            let pair = Pair::seeded(1);
            let (a, b) = pair.establish().await;

            a.close(0x2a, b"so long").await;
            assert_eq!(a.closed().await, ConnectionLost::LocallyClosed);
            assert_eq!(
                b.closed().await,
                ConnectionLost::PeerClosed {
                    code: 0x2a,
                    reason: b"so long".to_vec(),
                }
            );
        })
        .await;
    }

    /// Ruling 46's latch, all four properties in one place: concurrent
    /// awaiters all resolve, a future first awaited **after** the death
    /// resolves too, and a dropped one leaves nothing behind.
    #[tokio::test(start_paused = true)]
    async fn closed_is_latched_concurrent_and_cancel_safe() {
        local(async {
            let pair = Pair::seeded(2);
            let (a, b) = pair.establish().await;

            // Cancel-safety: a dropped `closed()` must leave the waker map
            // as it found it. Poll it once so it really parks.
            {
                let mut parked = Box::pin(b.closed());
                assert!(
                    futures_lite_poll_once(&mut parked).is_none(),
                    "a healthy connection resolved `closed()`",
                );
            }

            let one = b.closed();
            let two = b.closed();
            let three = b.closed();
            a.close(7, b"").await;
            let (one, two, three) = tokio::join!(one, two, three);
            assert_eq!(one, two);
            assert_eq!(two, three);

            // Latched: first awaited after the death, and again.
            assert_eq!(b.closed().await, one);
            assert_eq!(b.closed().await, one);
        })
        .await;
    }

    /// Ruling 87 + ruling 50, structurally: drop the `Connecting` and
    /// redial **on the very next line**, with no advance of the clock
    /// between them.
    #[tokio::test(start_paused = true)]
    async fn a_cancelled_dial_frees_the_static_with_no_clock_advance() {
        local(async {
            let pair = Pair::seeded(3);
            // `b` never accepts, so the dial stays in flight.
            let dialling = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("the static is NONE");

            // A second dial while the first is in flight is refused.
            assert_eq!(
                pair.a
                    .endpoint
                    .connect(pair.b.addr, pair.b.public_static)
                    .err(),
                Some(ConnectError::AlreadyConnected),
            );

            drop(dialling);
            // No `.await`, no `advance`, nothing between these two lines.
            let redial = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("the cancellation is ordered ahead of this verb");
            drop(redial);
        })
        .await;
    }

    /// §16.3: the driver lives while any handle lives. Dropping the
    /// `Endpoint` while a `Connection` lives leaves the connection usable;
    /// dropping the connection then stops the driver.
    #[tokio::test(start_paused = true)]
    async fn the_endpoint_is_not_the_last_handle() {
        local(async {
            let pair = Pair::seeded(4);
            let (a, b) = pair.establish().await;
            let crate::testutil::Pair {
                net,
                a: peer_a,
                b: peer_b,
            } = pair;

            drop(peer_a.endpoint);
            settle().await;
            assert!(a.is_established(), "the driver stopped with the endpoint");

            // Still alive enough to close, and the peer still hears it.
            let before = net.sends();
            a.close(0, b"").await;
            settle().await;
            assert!(net.sends() > before, "the CLOSE never left");
            assert!(matches!(
                b.closed().await,
                ConnectionLost::PeerClosed { .. }
            ));
            drop(peer_b);
        })
        .await;
    }

    /// Ruling 88: dropping the last `Connection` when it is **also** the
    /// last handle in the process transmits nothing.
    ///
    /// The count is **A's sends alone**, taken from the tap by source.
    /// `Network::sends()` would also count B's, and B is deliberately left
    /// whole here so that nothing on its side can close A's connection
    /// first — a connection already closing seals nothing on drop, and the
    /// assertion would then pass for a reason that has nothing to do with
    /// ruling 88.
    #[tokio::test(start_paused = true)]
    async fn the_coincident_last_handle_drop_transmits_nothing() {
        local(async {
            let pair = Pair::seeded(5);
            let (a, b) = pair.establish().await;
            let crate::testutil::Pair {
                net,
                a: peer_a,
                b: peer_b,
            } = pair;
            let tap = net.tap();
            let from_a = |tap: &crate::testutil::Tap| {
                tap.snapshot()
                    .iter()
                    .filter(|spied| spied.src == peer_a.addr)
                    .count()
            };

            // A's only remaining handle is the connection itself.
            drop(peer_a.endpoint);
            settle().await;

            let before = from_a(&tap);
            drop(a);
            settle().await;
            assert_eq!(
                from_a(&tap),
                before,
                "§15.4's endpoint-dropped row transmitted something",
            );

            // And the control: B is untouched, so it did not close A first.
            assert!(b.is_established());
            drop(b);
            drop(peer_b.endpoint);
        })
        .await;
    }

    /// The other half of ruling 88, and the reason the test above is a pin
    /// rather than a tautology: the **non**-coincident drop *does* seal a
    /// CLOSE, so the two rules really are different.
    #[tokio::test(start_paused = true)]
    async fn a_non_coincident_last_connection_drop_does_transmit() {
        local(async {
            let pair = Pair::seeded(9);
            let (a, b) = pair.establish().await;
            let tap = pair.net.tap();
            let addr_a = pair.a.addr;
            let from_a = || {
                tap.snapshot()
                    .iter()
                    .filter(|spied| spied.src == addr_a)
                    .count()
            };

            let before = from_a();
            // The `Endpoint` is still held, so this is *not* the last
            // handle in the process.
            drop(a);
            settle().await;
            assert!(from_a() > before, "the graceful CLOSE was never sealed");
            assert!(matches!(
                b.closed().await,
                ConnectionLost::PeerClosed { code: 0, .. },
            ));
        })
        .await;
    }

    /// Dropping an `Intro` is §6.2's silent reject: nothing is sent, and
    /// the peer simply retransmits on §5.5's schedule.
    #[tokio::test(start_paused = true)]
    async fn dropping_an_intro_is_silent() {
        local(async {
            let pair = Pair::seeded(6);
            let dialling = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("connect");

            let intro = pair.b.endpoint.accept().await.expect("an introduction");
            assert_eq!(intro.source(), pair.a.addr);
            assert_ne!(intro.sender_index(), 0, "§17.2 mints nonzero indices");
            let before = pair.b.dhs.get();
            drop(intro);
            settle().await;
            assert_eq!(pair.b.dhs.get(), before, "a rejected intro cost DH");

            // The dial is still running: the next retransmit surfaces a
            // fresh introduction.
            tokio::time::advance(Duration::from_secs(6)).await;
            let again = pair.b.endpoint.accept().await.expect("a retransmission");
            drop(again);
            drop(dialling);
        })
        .await;
    }

    /// §5.5's give-up reaches the shell: the `Connecting` resolves
    /// `TimedOut`, and **not before** `HANDSHAKE_GIVEUP`.
    ///
    /// The lower bound is the half that separates a working timer from one
    /// that gave up immediately; the upper bound separates it from one that
    /// never fires at all.
    #[tokio::test(start_paused = true)]
    async fn a_dial_nobody_answers_gives_up_at_the_giveup_and_not_before() {
        local(async {
            let pair = Pair::seeded(7);
            pair.net.partition(pair.b.addr);

            let started = tokio::time::Instant::now();
            let outcome = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("connect")
                .await;
            let elapsed = tokio::time::Instant::now() - started;

            assert_eq!(outcome.err(), Some(ConnectError::TimedOut));
            assert!(
                elapsed >= crate::constants::HANDSHAKE_GIVEUP,
                "gave up early, at {elapsed:?}",
            );
            assert!(
                elapsed < crate::constants::HANDSHAKE_GIVEUP + Duration::from_secs(6),
                "gave up late, at {elapsed:?}",
            );

            // And the static is free again.
            drop(
                pair.a
                    .endpoint
                    .connect(pair.b.addr, pair.b.public_static)
                    .expect("a redial after the give-up"),
            );
        })
        .await;
    }

    /// Documentation obligation #3, reached through the shell: a connection
    /// with nothing to say dies at `DEAD_TIMEOUT`, and `closed()` says so
    /// **with no verb in flight** (§7.4, §15.4's first row).
    #[tokio::test(start_paused = true)]
    async fn an_idle_connection_dies_at_dead_timeout() {
        local(async {
            let pair = Pair::seeded(8);
            let (a, b) = pair.establish().await;

            let started = tokio::time::Instant::now();
            assert_eq!(a.closed().await, ConnectionLost::TimedOut);
            let elapsed = tokio::time::Instant::now() - started;
            assert!(
                elapsed >= crate::constants::DEAD_TIMEOUT,
                "died early, at {elapsed:?}",
            );
            assert_eq!(b.closed().await, ConnectionLost::TimedOut);

            // §15.4's liveness row transmits nothing, and the state is gone.
            assert!(!a.is_established());
            assert!(!b.is_established());
        })
        .await;
    }

    /// Rulings 81/84's ordering, from the outside: after §15.2's linger
    /// expires the core drops its state and emits `Retired`, and the shell
    /// hands that to the endpoint core **before** releasing its own
    /// bookkeeping. The observable consequence is that the static is free
    /// again — a redial to it succeeds — and `is_established()` has gone
    /// false on a handle the application still holds.
    ///
    /// The broken version — releasing the shell record first, so `Retired`
    /// never reaches the endpoint core — leaves the static LIVE for the
    /// endpoint's life, and the redial below returns `AlreadyConnected`.
    #[tokio::test(start_paused = true)]
    async fn a_closed_connection_frees_its_static_when_the_linger_expires() {
        local(async {
            let pair = Pair::seeded(10);
            let (a, b) = pair.establish().await;

            a.close(0, b"").await;
            settle().await;

            // Still lingering: the session is alive, and the static is not
            // free — §15.2 keeps the state for `CLOSE_LINGER` so it can
            // reply to authenticated inbound.
            assert!(a.is_established(), "the linger dropped state early");
            assert_eq!(
                pair.a
                    .endpoint
                    .connect(pair.b.addr, pair.b.public_static)
                    .err(),
                Some(ConnectError::AlreadyConnected),
                "the static was freed before the linger expired",
            );

            tokio::time::advance(crate::constants::CLOSE_LINGER + Duration::from_millis(1)).await;
            settle().await;

            assert!(!a.is_established(), "the linger never expired");
            let redial = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("`Retired` never reached the endpoint core: the route leaked");
            drop(redial);
            drop(b);
        })
        .await;
    }

    /// §10.6, in the one place slice 3 can reach it: the driver's
    /// ready-introduction queue must not accumulate ids for entries the
    /// core has already expired.
    ///
    /// The broken version hands the stale id out first, and
    /// `read_identity()` answers `IntroError::Expired` while a perfectly
    /// good initiation waits behind it — so the assertion is on the *live*
    /// introduction succeeding, not on a queue length no consumer can see.
    #[tokio::test(start_paused = true)]
    async fn an_expired_introduction_is_not_handed_to_a_later_accept() {
        local(async {
            let pair = Pair::seeded(11);

            // Surface an introduction at `b` and never accept it.
            let dialling = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("connect");
            settle().await;

            // Stop the retransmit train, so nothing refreshes the entry,
            // and let it age out of §6.3's queue.
            drop(dialling);
            tokio::time::advance(crate::constants::INTRO_TTL + Duration::from_secs(1)).await;
            settle().await;

            // A fresh dial. `accept()` must hand over *this* one.
            let dialling = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("the static is NONE again");
            let intro = pair.b.endpoint.accept().await.expect("an introduction");
            let claimed = intro
                .read_identity()
                .await
                .expect("accept() handed over an introduction that had already expired");
            assert_eq!(
                claimed.claimed_static().as_ref(),
                pair.a.public_static.as_ref(),
            );
            drop(claimed);
            drop(dialling);
        })
        .await;
    }

    /// Poll a future once without a runtime turn.
    fn futures_lite_poll_once<F: std::future::Future>(
        future: &mut std::pin::Pin<Box<F>>,
    ) -> Option<F::Output> {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(value) => Some(value),
            std::task::Poll::Pending => None,
        }
    }
}

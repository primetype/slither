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
//! to land on the driver task. `connect()` is the exception (rulings 87 and
//! 90): it is not `async` because its first half, `mint_pending`, costs **0
//! DH**, so the handle calls it on the shared cell and answers §16.1's
//! NONE/PENDING/LIVE test out of the endpoint core's **own** static map.
//! Its second half, `start_attempt`, carries §6.1's two initiator DH and
//! rides the command channel like every other DH-bearing verb. The
//! connection data path and the four accessors share an `Rc<RefCell<_>>`
//! with the driver; each data-path verb is written once as `poll_*`, and
//! the `async fn` is `poll_fn` over it.
//!
//! # What is here, and what is not
//!
//! Slice 3 builds `connect`/`accept`, the staged ladder, `close()`,
//! `closed()` and the four accessors. Slice 4 adds §16.2's stream surface:
//! `open_bi`/`open_uni`/`accept_bi`/`accept_uni` plus [`SendStream`],
//! [`RecvStream`] and [`BiStream`]. §16.2's message, datagram, `acked()`,
//! `notified()` and
//! keepalive verbs arrive with the slices that define them and are **absent
//! rather than stubbed**: in this crate an unimplemented verb is a claim
//! about the protocol. `SendStream::acked()` is slice 5's (ruling 122b) —
//! it needs `ConnEvent::StreamFinished`, which needs §12's ACK processing —
//! and the `AsyncRead`/`AsyncWrite` impls are slice 8's (ruling 96).

pub mod wire;

mod connection;
mod driver;
mod endpoint;
mod shared;
mod staged;
mod stream;

pub use self::connection::Connection;
pub use self::endpoint::{Connecting, Endpoint, EndpointBuilder};
pub use self::staged::{Claimed, Intro, Proven};
pub use self::stream::{BiStream, RecvStream, SendStream};

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

    /// Rulings 81/84's ordering, from the outside: `Closed(_)` fires at
    /// the death, `ToEndpoint::Retired` only at `CLOSE_LINGER` expiry, and
    /// the shell hands that `Retired` to the endpoint core **before**
    /// releasing its own bookkeeping (§16.4's MUST) — "else the index
    /// route and the guard-entry pin leak for the endpoint's life".
    ///
    /// # The two degenerate builds this separates, and how
    ///
    /// 1. **Released at the death.** Weaken `Driver::release_dead`'s gate
    ///    from `closed.is_some() && !is_established()` to `closed.is_some()`
    ///    and the shell tears the record down the moment `close()` seals,
    ///    dropping the connection core *before* it ever emits `Retired`.
    ///    The **first half** below is what sees that: during the linger
    ///    §15.2 still holds the session — that is what makes the linger
    ///    able to receive, and its reply rule is CLOSE's only reliability
    ///    mechanism — so `is_established()` must still answer `true` and
    ///    the static must still be occupied.
    ///
    /// 2. **`Retired` dropped on the floor.** Delete the
    ///    `handle_connection_event` call in `Driver::serve_connection`'s
    ///    `ToEndpoint` arm. The **second half** is what sees that. Before
    ///    ruling 90 it was the half that used to assert nothing: *neither*
    ///    `is_established()` *nor* `Endpoint::connect`'s `Ok` could observe
    ///    the mutation, because `release_dead` freed the shell's **mirror**
    ///    of the static map whether or not the `Retired` had been
    ///    delivered, and `connect()` was answered from that mirror — so
    ///    only the **resolved `Connecting`** reported the core's real
    ///    answer, and a dropped one threw it away. Ruling 90 deleted the
    ///    mirror: the redial's admission test is now `mint_pending` reading
    ///    the core's own map, so under the mutation `connect()` itself
    ///    answers `AlreadyConnected` and the `expect` below is the first
    ///    thing that fires. The wire reading and the completed handshake
    ///    are kept anyway — they were the assertions that separated the two
    ///    builds when nothing cheaper could, and a test does not get weaker
    ///    because the seam got stronger.
    ///
    /// The third build — a *core* that emits `Retired` at the death
    /// instead of at the expiry — is a mutation of frozen `src/core/**`,
    /// pinned there (`src/core/connection/tests.rs`, §15.2's linger
    /// tests). At this seam it is indistinguishable from build 1.
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
            drop(a);
            drop(b);

            // Take a wire reading first — under the mutation the endpoint
            // core refuses before anything is sealed, so **no msg1 leaves
            // at all**, and this separates without B's cooperation. Before
            // ruling 90 this was the *only* thing that separated them,
            // because `connect()`'s `Ok` was the shell mirror's answer
            // rather than the core's; it is now both.
            let tap = pair.net.tap();
            let from_a = || {
                tap.snapshot()
                    .iter()
                    .filter(|spied| spied.src == pair.a.addr)
                    .count()
            };
            let before = from_a();

            let redial = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect(
                    "`mint_pending` still found the static occupied after the linger: \
                     `Retired` never reached the endpoint core (ruling 90)",
                );
            settle().await;
            assert!(
                from_a() > before,
                "no msg1 left the wire: `core::Endpoint::connect` refused the redial, \
                 so `Retired` never reached the endpoint core and the static leaked",
            );

            // And the endpoint core's own answer, end to end. B's draining
            // linger started at the same instant as A's closing one (the
            // fabric is delay-free), so its static is free too and the
            // whole 4-DH ladder must climb again.
            let accept = async {
                let intro = pair.b.endpoint.accept().await.expect("an introduction");
                let claimed = intro.read_identity().await.expect("read_identity");
                let proven = claimed.authenticate().await.expect("authenticate");
                proven.accept().await.expect("accept")
            };
            let (again_a, again_b) = tokio::join!(
                async {
                    redial.await.expect(
                        "the endpoint core still held the static: `Retired` was not delivered",
                    )
                },
                accept,
            );
            assert!(again_a.is_established());
            assert!(again_b.is_established());
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

    /// A `Waker` is **application-supplied**, and this crate does not get
    /// to assume what `wake()` does with it. An executor that polls a
    /// ready task *inline* — rather than pushing it onto a run queue, as
    /// tokio does — re-enters `Connection::poll_closed` synchronously, and
    /// `poll_closed` takes `ConnCell`'s `borrow_mut`.
    ///
    /// The broken version is what `Driver::latch` used to be: `borrow_mut`
    /// held across `closed_wakers.wake_all()`. The inline poll below then
    /// hits `already mutably borrowed` **inside the driver task**, mid-drain
    /// — which `spawn_local` swallows, so the only evidence left is the two
    /// assertions here: the inline poll never completed, and `Driver::drop`
    /// ran `stop()` so the endpoint now answers `Local` instead of
    /// `AlreadyConnected`.
    ///
    /// Restoring the borrow is the mutation; both assertions separate it.
    #[tokio::test(start_paused = true)]
    async fn a_waker_that_polls_inline_does_not_re_enter_a_live_borrow() {
        use std::cell::Cell;
        use std::future::Future;
        use std::pin::Pin;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};

        local(async {
            let pair = Pair::seeded(12);
            let (a, b) = pair.establish().await;
            let a = Rc::new(a);

            let waker = inline_waker();
            let outcome: Rc<Cell<Option<ConnectionLost>>> = Rc::new(Cell::new(None));

            // Park under our waker. One poll is what registers in
            // `ConnCell::closed_wakers`; the future stays alive for the
            // rest of the test so its `WakerSlot` guard does not unpark it.
            let mut parked: Pin<Box<dyn Future<Output = ConnectionLost>>> = Box::pin(a.closed());
            assert!(
                parked
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending(),
                "a healthy connection resolved `closed()`",
            );

            ON_WAKE.with(|slot| {
                // The closure owns its own handle, so nothing here borrows
                // a local and the whole action is `'static` — which is what
                // a real executor's waker looks like.
                let handle = Rc::clone(&a);
                let outcome = Rc::clone(&outcome);
                *slot.borrow_mut() = Some(Box::new(move || {
                    // Poll straight back into the same cell, which is what
                    // an inline executor does with a ready task. A fresh
                    // future rather than the parked one, so this cannot
                    // recurse: after the death it resolves on its first
                    // poll.
                    let mut inline = Box::pin(handle.closed());
                    let polled = inline
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()));
                    if let Poll::Ready(lost) = polled {
                        outcome.set(Some(lost));
                    }
                }));
            });

            // The peer closes. A's driver latches and wakes inside
            // `serve_connection`'s drain — the borrow under review.
            b.close(9, b"inline").await;
            settle().await;

            assert_eq!(
                outcome.take(),
                Some(ConnectionLost::PeerClosed {
                    code: 9,
                    reason: b"inline".to_vec(),
                }),
                "the inline re-poll never completed: the waker ran under \
                 `ConnCell`'s borrow and the driver task panicked",
            );

            // And the driver is alive rather than merely quiet. `connect()`
            // tests `driver_stopped` *before* it consults the endpoint
            // core's static map, so a stopped driver answers `Local` here
            // where a live one, with A still draining, answers
            // `AlreadyConnected`.
            assert_eq!(
                pair.a
                    .endpoint
                    .connect(pair.b.addr, pair.b.public_static)
                    .err(),
                Some(ConnectError::AlreadyConnected),
                "the driver stopped: `Driver::drop` ran `stop()` on an unwind",
            );

            // The thread-local outlives the test on this thread.
            ON_WAKE.with(|slot| *slot.borrow_mut() = None);
            drop(parked);
        })
        .await;
    }

    /// The second half of the same finding, on the other waker site:
    /// `PendingSlot::resolve` used to call `Waker::wake` while the
    /// caller's `slot.borrow_mut()` was live, and `Connecting::poll`
    /// borrows that same slot.
    ///
    /// Driven by §5.5's give-up rather than by a death, because that is
    /// the path that resolves a `Connecting` from the driver:
    /// `fail_pending` → `resolve_slot`.
    ///
    /// The broken version: wake inside the borrow in
    /// [`resolve_slot`](super::shared::resolve_slot). The inline re-poll
    /// then finds the slot already borrowed.
    #[tokio::test(start_paused = true)]
    async fn an_inline_waker_may_re_poll_a_connecting_from_wake() {
        use std::cell::{Cell, RefCell};
        use std::future::Future;
        use std::pin::Pin;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};

        local(async {
            let pair = Pair::seeded(13);
            // Nobody answers, so the attempt runs to §5.5's give-up.
            pair.net.partition(pair.b.addr);

            let dialling = pair
                .a
                .endpoint
                .connect(pair.b.addr, pair.b.public_static)
                .expect("the static is NONE");
            // The driver must *start* the attempt before the clock jumps,
            // or §5.5's 90 s give-up is measured from the far side of the
            // advance and nothing fires.
            settle().await;
            type Dial = Pin<Box<crate::shell::Connecting<crate::testutil::TestIdentity>>>;
            let dialling: Rc<RefCell<Dial>> = Rc::new(RefCell::new(Box::pin(dialling)));
            let outcome: Rc<Cell<Option<ConnectError>>> = Rc::new(Cell::new(None));

            let waker = inline_waker();
            assert!(
                dialling
                    .borrow_mut()
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending(),
                "the dial resolved before the give-up",
            );

            ON_WAKE.with(|action| {
                let dialling = Rc::clone(&dialling);
                let outcome = Rc::clone(&outcome);
                *action.borrow_mut() = Some(Box::new(move || {
                    // Poll straight back in, which is what an inline
                    // executor does. `Connecting::poll` takes the very
                    // borrow `resolve_slot` must have released by now.
                    let polled = dialling
                        .borrow_mut()
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop()));
                    if let Poll::Ready(Err(error)) = polled {
                        outcome.set(Some(error));
                    }
                }));
            });

            tokio::time::advance(crate::constants::HANDSHAKE_GIVEUP + Duration::from_secs(1)).await;
            settle().await;

            assert_eq!(
                outcome.take(),
                Some(ConnectError::TimedOut),
                "the inline re-poll never completed: the waker ran under \
                 `PendingSlot`'s borrow and the driver task panicked",
            );

            ON_WAKE.with(|action| *action.borrow_mut() = None);
        })
        .await;
    }

    // ═══════════════════════════════════════════════════════════════════
    // An executor whose `wake()` runs application code inline
    //
    // `Waker` is supplied by the consumer, so nothing in this crate gets
    // to assume `wake()` merely pushes to a run queue — tokio's does, and
    // that is why no ordinary test here can reach the two re-entrancy
    // sites F10 names. `std::task::Wake` requires `Send + Sync` and every
    // cell being re-entered is `!Send`, so the action travels through a
    // thread-local; the driver cannot tell the difference, because the
    // re-entry is a synchronous call out of `wake()` either way.
    // ═══════════════════════════════════════════════════════════════════

    thread_local! {
        /// What [`InlineWaker`] runs, synchronously, on the thread that
        /// woke it — in both tests above, the driver's own.
        static ON_WAKE: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
            const { std::cell::RefCell::new(None) };
    }

    struct InlineWaker;

    impl std::task::Wake for InlineWaker {
        fn wake(self: std::sync::Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            // Taken out and put back, so the action is free to park and be
            // woken again without a nested borrow of its own.
            let action = ON_WAKE.with(|slot| slot.borrow_mut().take());
            if let Some(mut action) = action {
                action();
                ON_WAKE.with(|slot| *slot.borrow_mut() = Some(action));
            }
        }
    }

    fn inline_waker() -> std::task::Waker {
        std::task::Waker::from(std::sync::Arc::new(InlineWaker))
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

    // ═══════════════════════════════════════════════════════════════════
    // Slice 4b: end-to-end smoke
    //
    // **Not** the slice's acceptance tests — `tests/story_streams.rs` and
    // `tests/spec_streams.rs` are, and they were written independently
    // (working rule 6). These two exist because the implementer needs to
    // know the seam works at all, and because the first of them is the
    // shape that catches the slice's named worst defect.
    // ═══════════════════════════════════════════════════════════════════

    /// A uni stream carries its bytes, and **dropping the `SendStream`
    /// after `finish()` leaves the peer's clean EOF intact**.
    ///
    /// The drop is the point. `SendHalf::reset` early-returns only when a
    /// reset already exists — a set FIN does **not** stop it — so a `Drop`
    /// that resets unconditionally destroys the unsent buffer and turns
    /// this `Ok(None)` into `Err(ReadError::Reset(0))`. Any test that reads
    /// before the sender drops passes over that defect.
    #[tokio::test(start_paused = true)]
    async fn a_finished_uni_stream_survives_its_senders_drop() {
        local(async {
            let pair = Pair::seeded(0x4B_0010);
            let (a, b) = pair.establish().await;

            let mut send = a.open_uni().await.expect("open_uni");
            let payload = b"the spec is the authority";
            let mut written = 0;
            while written < payload.len() {
                written += send.write(&payload[written..]).await.expect("write");
            }
            send.finish().await.expect("finish");
            let id = send.id();
            assert!(
                id.is_some(),
                "ruling 116: a live handle's id is always Some"
            );
            drop(send);
            settle().await;

            let mut recv = b.accept_uni().await.expect("accept_uni");
            assert_eq!(recv.id(), id, "both ends name the same stream");

            let mut got = Vec::new();
            let mut buf = [0u8; 8];
            while let Some(n) = recv.read(&mut buf).await.expect("read") {
                got.extend_from_slice(&buf[..n]);
            }
            assert_eq!(got, payload);

            // Sticky, and the id keeps answering after the stream is gone.
            assert_eq!(recv.read(&mut buf).await, Ok(None));
            assert_eq!(recv.id(), id, "ruling 116: `id()` keeps answering");
        })
        .await;
    }

    /// **Ruling 124: a handle's own terminal state outranks the
    /// connection's death latch.**
    ///
    /// The receive half. A reader that reached EOF *completed its
    /// transfer*; the connection dying afterwards does not un-complete it,
    /// and answering `ConnectionLost` to the next `read` reports a failure
    /// about a success — ruling 121's misreport with its sign flipped.
    /// Slice 8 makes it load-bearing rather than tidy: `AsyncRead` requires
    /// a sticky EOF, so `read_to_end` over a connection that dies after the
    /// FIN would surface a spurious `io::Error`.
    ///
    /// **What the broken build does:** with the death latch checked first —
    /// which is what `CONTRACT-4b.md` §8 said, and what the implementer
    /// built and correctly flagged as C4 — the final `read` is
    /// `Err(ConnectionLost)` and this test fails on its last assertion.
    /// Every other stream test in this file passes against both orders,
    /// because none of them reads again after the connection is gone.
    #[tokio::test(start_paused = true)]
    async fn a_finished_streams_eof_outlives_its_connection() {
        local(async {
            let pair = Pair::seeded(0x4B_0020);
            let (a, b) = pair.establish().await;

            let mut send = a.open_uni().await.expect("open_uni");
            send.write(b"done").await.expect("write");
            send.finish().await.expect("finish");
            settle().await;

            let mut recv = b.accept_uni().await.expect("accept_uni");
            let mut buf = [0u8; 8];
            let n = recv.read(&mut buf).await.expect("read").expect("bytes");
            assert_eq!(&buf[..n], b"done");
            assert_eq!(recv.read(&mut buf).await, Ok(None), "EOF");

            // Now kill the connection under the finished reader.
            b.close(0, b"").await;
            settle().await;

            assert_eq!(
                recv.read(&mut buf).await,
                Ok(None),
                "ruling 124: the stream ended before the connection did, and \
                 the handle reports the fate of its own stream"
            );
        })
        .await;
    }

    /// **Ruling 124, the send half** — and the reason `local_end` is an
    /// enum rather than the `bool` the contract specified.
    ///
    /// The two terminal answers differ: `finish()` is idempotent, so a
    /// second one is `Ok(())`, while `write()` after it is
    /// `Err(Finished)`. A `bool` cannot distinguish `Finished` from
    /// `Reset`, which forces `poll_finish` to fall through to the core —
    /// where a dead connection answers `ConnectionLost` and the ordering
    /// silently reverts.
    ///
    /// **What the broken build does:** with the death latch first, the
    /// second `finish()` is `Err(ConnectionLost)` and the `write` is too,
    /// so both assertions fail.
    #[tokio::test(start_paused = true)]
    async fn a_finished_send_half_answers_from_its_own_state_after_death() {
        local(async {
            use crate::error::WriteError;

            let pair = Pair::seeded(0x4B_0021);
            let (a, _b) = pair.establish().await;

            let mut send = a.open_uni().await.expect("open_uni");
            send.write(b"x").await.expect("write");
            send.finish().await.expect("finish");

            a.close(0, b"").await;
            settle().await;

            assert_eq!(
                send.finish().await,
                Ok(()),
                "ruling 124: `finish` stays idempotent across the death"
            );
            assert_eq!(
                send.write(b"more").await,
                Err(WriteError::Finished),
                "ruling 124: the half's own terminal state, not the \
                 connection's"
            );
        })
        .await;
    }

    /// **Ruling 115's consequence, and the one thing in 4b the contract
    /// left unstated — see `IMPLEMENTATION-4b.md` D1.**
    ///
    /// §16.2:4392 is unconditional: *dropping the last handle to a
    /// `Connection` performs `close(NO_ERROR, "")`*. Ruling 115 has just
    /// made a stream handle a handle, so the last one can now be a
    /// `SendStream` — which is the *ordinary* shape ruling 115's own
    /// rationale names, a task that owns a stream and has let the connection
    /// handle go.
    ///
    /// Two things are pinned here. Dropping the `Connection` while a stream
    /// lives must **not** close (the stream is still a handle), and dropping
    /// that stream afterwards **must**. Without the second, this connection
    /// emits no CLOSE at all and the peer pays `DEAD_TIMEOUT`.
    #[tokio::test(start_paused = true)]
    async fn the_last_handle_to_a_connection_may_be_a_stream() {
        local(async {
            let pair = Pair::seeded(0x4B_0012);
            let (a, b) = pair.establish().await;

            let mut send = a.open_uni().await.expect("open_uni");
            send.finish().await.expect("finish");

            drop(a);
            settle().await;
            assert!(
                b.is_established(),
                "ruling 115: a live `SendStream` is a handle, so this was \
                 not the last one and no CLOSE is owed yet",
            );

            drop(send);
            settle().await;
            assert_eq!(
                b.closed().await,
                ConnectionLost::PeerClosed {
                    code: crate::constants::NO_ERROR,
                    reason: Vec::new(),
                },
                "§16.2: the last handle to a connection performs \
                 `close(NO_ERROR, \"\")`, whatever kind of handle it is",
            );
        })
        .await;
    }

    /// `reset()` reaches the peer as `Err(ReadError::Reset)`, and **ruling
    /// 121's latch makes it stick**: without it the retry reads `Ok(None)`
    /// out of the core and §9.6's abandoned data is reported as a complete
    /// transfer.
    #[tokio::test(start_paused = true)]
    async fn a_reset_is_sticky_at_the_reader() {
        local(async {
            let pair = Pair::seeded(0x4B_0011);
            let (a, b) = pair.establish().await;

            let mut send = a.open_uni().await.expect("open_uni");
            send.write(b"partial").await.expect("write");
            settle().await;

            let mut recv = b.accept_uni().await.expect("accept_uni");
            let mut buf = [0u8; 32];
            assert_eq!(recv.read(&mut buf).await, Ok(Some(7)));

            send.reset(0x2a);
            settle().await;

            assert_eq!(
                recv.read(&mut buf).await,
                Err(crate::error::ReadError::Reset(0x2a))
            );
            assert_eq!(
                recv.read(&mut buf).await,
                Err(crate::error::ReadError::Reset(0x2a)),
                "ruling 121: the core is not sticky, so the handle must be",
            );

            // The connection and its siblings are untouched.
            assert!(a.is_established() && b.is_established());
        })
        .await;
    }

    // ═══════════════════════════════════════════════════════════════════
    // Slice 4b: the three faults `FlakyWire` cannot express
    //
    // Working rule 13. The fixture models a **network** — it loses,
    // delays, duplicates, reorders and fails sends — and none of that
    // reaches a stopped driver, an inline-polling waker or a drop with no
    // runtime entered. All three are stream-handle faults, so all three
    // live here rather than in an integration test that cannot construct
    // them.
    // ═══════════════════════════════════════════════════════════════════

    /// §16.8's maps are keyed by a value the **application** controls, so
    /// what bounds them has to be asserted — and asserted from the side that
    /// separates a build which removes its entries from one that does not.
    ///
    /// A `<= N` assertion would pass a build that never inserts at all
    /// (working rule 9). This one parks a reader and a writer on every
    /// stream, checks the maps actually hold them, drops the handles and
    /// requires **zero** entries: the broken build leaves eight.
    #[tokio::test(start_paused = true)]
    async fn dropping_stream_handles_empties_the_waker_maps() {
        use std::task::Context;

        use super::BiStream;

        local(async {
            let pair = Pair::seeded(0x4B_0001);
            let (a, _b) = pair.establish().await;
            assert_eq!(
                a.stream_waker_entries(),
                (0, 0),
                "a connection with no streams holds no waker-map entries",
            );

            const N: usize = 8;
            let mut handles = Vec::new();
            for _ in 0..N {
                handles.push(a.open_bi().await.expect("open_bi"));
            }

            // Park a real waiter on each, so the maps hold wakers and not
            // merely empty slots: nothing has been written to these streams,
            // so every read is pending.
            let waker = std::task::Waker::noop();
            let mut cx = Context::from_waker(waker);
            let mut buf = [0u8; 8];
            let mut halves: Vec<_> = handles.into_iter().map(BiStream::split).collect();
            for (_, recv) in &mut halves {
                assert!(
                    recv.poll_read(&mut cx, &mut buf).is_pending(),
                    "a stream nobody has written to should park its reader",
                );
            }

            assert_eq!(
                a.stream_waker_entries(),
                (N, N),
                "every live half owns exactly one per-`StreamRef` entry",
            );

            drop(halves);
            assert_eq!(
                a.stream_waker_entries(),
                (0, 0),
                "a dropped handle must take its map entry with it (§16.8)",
            );
        })
        .await;
    }

    /// **R2 and R3 in one shape.** A reader and a writer are parked; the
    /// driver then goes away *without* the handle count reaching zero — the
    /// only way it can, since a parked waiter is holding a handle — and both
    /// must resolve rather than park for ever.
    ///
    /// The wake is delivered to an [`InlineWaker`], so this is also §16.8's
    /// finding F10 for the two stream maps: `Driver::latch` takes the wakers
    /// under `ConnCell`'s borrow and wakes them after it, and an executor
    /// that re-polls inline lands in `poll_read`/`poll_write`, which take
    /// that same borrow. With the wake inside the borrow this test panics in
    /// the driver's own drop.
    ///
    /// Nothing in `testutil` can produce either fault: a fabric that loses
    /// and reorders datagrams cannot stop a driver, and tokio's wakers only
    /// push to a run queue.
    #[tokio::test(start_paused = true)]
    async fn a_parked_stream_waiter_resolves_when_the_driver_stops() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::task::{Context, Poll, Waker};

        use crate::error::{ReadError, WriteError};

        type Halves = (
            crate::testutil::TestSendStream,
            crate::testutil::TestRecvStream,
        );

        // Which half resolved, and with what. `latch` hands the sweep one
        // waker per map, so this `InlineWaker` is woken once for the reader
        // and once for the writer and the action runs twice; what is being
        // pinned is that **both** halves resolve, not how many times the
        // executor was poked.
        let outcomes: Rc<RefCell<Vec<(&'static str, ConnectionLost)>>> =
            Rc::new(RefCell::new(Vec::new()));

        let set = tokio::task::LocalSet::new();
        #[expect(
            unused_variables,
            reason = "held past `drop(set)` so the driver's death is not a handle-count effect"
        )]
        let (pair, a, b, kept): (_, _, _, Rc<RefCell<Option<Halves>>>) = set
            .run_until(async {
                let pair = Pair::seeded(0x4B_0002);
                let (a, b) = pair.establish().await;

                let (send, recv) = a.open_bi().await.expect("open_bi").split();
                let kept = Rc::new(RefCell::new(Some((send, recv))));

                let waker = inline_waker();
                let mut cx = Context::from_waker(&waker);
                let mut buf = [0u8; 8];

                {
                    let mut borrow = kept.borrow_mut();
                    let (send, recv) = borrow.as_mut().expect("both halves");

                    // The reader parks at once: nothing has been written.
                    assert!(recv.poll_read(&mut cx, &mut buf).is_pending());

                    // The writer parks on §10.1's stream window. The core
                    // call is what arms the wakeup — `StreamWritable` fires
                    // only for a half whose `blocked` flag `SendHalf::write`
                    // set — so this fills by writing, never by consulting a
                    // credit accessor.
                    let chunk = vec![0x5Au8; 8 * 1024];
                    let mut parked = false;
                    for _ in 0..64 {
                        if send.poll_write(&mut cx, &chunk).is_pending() {
                            parked = true;
                            break;
                        }
                    }
                    assert!(parked, "the writer never reached §10.1's stream window");
                }

                // Re-poll from inside `wake()`, which is what an executor
                // that runs a ready task inline does.
                ON_WAKE.with(|action| {
                    let kept = Rc::clone(&kept);
                    let outcomes = Rc::clone(&outcomes);
                    *action.borrow_mut() = Some(Box::new(move || {
                        let mut borrow = kept.borrow_mut();
                        let Some((send, recv)) = borrow.as_mut() else {
                            return;
                        };
                        let mut cx = Context::from_waker(Waker::noop());
                        let mut buf = [0u8; 8];
                        if let Poll::Ready(Err(ReadError::ConnectionLost(lost))) =
                            recv.poll_read(&mut cx, &mut buf)
                        {
                            outcomes.borrow_mut().push(("read", lost));
                        }
                        if let Poll::Ready(Err(WriteError::ConnectionLost(lost))) =
                            send.poll_write(&mut cx, b"x")
                        {
                            outcomes.borrow_mut().push(("write", lost));
                        }
                    }));
                });

                // The endpoints and both connections travel out with the
                // parked halves: the driver must die from the `LocalSet`
                // going away, not from the handle count reaching zero.
                (pair, a, b, kept)
            })
            .await;

        // Dropping the `LocalSet` drops the driver task, and `Drop for
        // Driver` runs `stop()` — the same path a panicking driver takes.
        drop(set);

        let recorded = outcomes.borrow().clone();
        for half in ["read", "write"] {
            assert!(
                recorded.contains(&(half, ConnectionLost::EndpointDropped)),
                "the parked {half} was never swept by `Driver::latch`; \
                 parking for ever is F1's shape — recorded: {recorded:?}",
            );
        }

        ON_WAKE.with(|action| *action.borrow_mut() = None);
        drop(kept.borrow_mut().take());
    }

    /// **R4.** A stream handle dropped with **no runtime entered at all**.
    ///
    /// Both `Drop` impls call `shared::now()`, which is
    /// `tokio::time::Instant::now()`, and a panic inside a `Drop` during
    /// unwinding aborts the process. `FlakyWire` cannot express a process,
    /// so this is a plain `#[test]`: it builds the handles on a runtime,
    /// tears the runtime down, and only then drops them.
    #[test]
    fn a_stream_handle_may_be_dropped_with_no_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .expect("a current-thread runtime");
        let set = tokio::task::LocalSet::new();

        let (pair, a, b, bi) = runtime.block_on(set.run_until(async {
            let pair = Pair::seeded(0x4B_0003);
            let (a, b) = pair.establish().await;
            let bi = a.open_bi().await.expect("open_bi");
            (pair, a, b, bi)
        }));

        drop(set);
        drop(runtime);

        // No runtime, no `LocalSet`, no driver. `tokio::time::Instant::now`
        // falls back to `std::time::Instant::now` outside a runtime rather
        // than panicking — pinned here because the two `Drop` impls depend
        // on it and nothing else in the suite reaches this state.
        drop(bi);
        drop(a);
        drop(b);
        drop(pair);
    }

    // ── slice 6's seam, from inside ─────────────────────────────────────
    //
    // S15, S16 and S30 are two independent authors' (working rule 6) and
    // live in `tests/story_datagram.rs` and `tests/story_message.rs`. What
    // is here is the *seam*: the three new waker sets, whose occupancy is
    // the one thing `LocalSet::run_until` can hide — it re-polls its body
    // on any local-task wake, so a build that never wired `MessageReadable`
    // to `message_readers` can still pass an `.await`-based test, woken
    // incidentally by the driver (`PLAN-6.md` §9).

    /// A parked `recv_message()` and a parked `recv_datagram()` each hold
    /// exactly one slot, and dropping the future gives it back.
    ///
    /// Asserted from **both** sides: nonzero while parked separates this
    /// from a build that never parks at all, and zero after the drop
    /// separates it from one that never releases (working rule 9).
    #[tokio::test(start_paused = true)]
    async fn the_claim_verbs_park_in_their_own_sets_and_release_on_drop() {
        local(async {
            let pair = Pair::seeded(0x6_0001);
            let (_a, b) = pair.establish().await;

            assert_eq!(b.sugar_waker_entries(), (0, 0, 0), "nothing parked yet");
            {
                let mut message = Box::pin(b.recv_message());
                let mut datagram = Box::pin(b.recv_datagram());
                assert!(futures_lite_poll_once(&mut message).is_none());
                assert!(futures_lite_poll_once(&mut datagram).is_none());
                assert_eq!(
                    b.sugar_waker_entries(),
                    (1, 1, 0),
                    "each parked future holds exactly its own slot"
                );
            }
            assert_eq!(
                b.sugar_waker_entries(),
                (0, 0, 0),
                "a dropped future leaves the sets as it found them"
            );
        })
        .await;
    }

    /// One datagram out, one datagram in, and the claim wakes a future that
    /// parked before it arrived.
    #[tokio::test(start_paused = true)]
    async fn a_datagram_crosses_and_wakes_a_parked_claim() {
        local(async {
            let pair = Pair::seeded(0x6_0002);
            let (a, b) = pair.establish().await;

            let mut waiting = Box::pin(b.recv_datagram());
            assert!(futures_lite_poll_once(&mut waiting).is_none());

            a.send_datagram(b"unreliable").expect("send_datagram");
            settle().await;

            assert_eq!(waiting.await.expect("the datagram arrived"), b"unreliable");
        })
        .await;
    }

    /// §11.4's bound is checked **at the handle, before any queue** — so an
    /// oversize send is an error and the next claim still sees only the
    /// datagram that was legal.
    #[tokio::test(start_paused = true)]
    async fn an_oversize_datagram_is_rejected_and_queues_nothing() {
        local(async {
            let pair = Pair::seeded(0x6_0003);
            let (a, b) = pair.establish().await;

            a.send_datagram(b"first").expect("send_datagram");
            let over = vec![0xAB; crate::constants::MAX_DATAGRAM_PAYLOAD + 1];
            assert!(matches!(
                a.send_datagram(&over),
                Err(crate::error::DatagramError::TooLarge)
            ));
            settle().await;

            assert_eq!(b.recv_datagram().await.expect("the legal one"), b"first");
        })
        .await;
    }

    /// §16.2's own idiom, end to end: `send_message`, `acked`, `close` —
    /// and the receiver still drains the message **after** the latch
    /// (ruling 152).
    #[tokio::test(start_paused = true)]
    async fn a_message_survives_the_senders_close() {
        local(async {
            let pair = Pair::seeded(0x6_0004);
            let (a, b) = pair.establish().await;

            a.send_message(b"one whole message").await.expect("send");
            a.acked().await.expect("the peer acknowledged it");
            a.close(0, b"").await;
            settle().await;

            assert_eq!(
                b.recv_message().await.expect("drained after the latch"),
                b"one whole message"
            );
            assert!(
                b.recv_message().await.is_err(),
                "with nothing left to claim the death is reported"
            );
        })
        .await;
    }

    /// A payload above `MESSAGE_RECV_MAX` never reaches the core.
    #[tokio::test(start_paused = true)]
    async fn an_oversize_message_is_rejected_at_the_handle() {
        local(async {
            let pair = Pair::seeded(0x6_0005);
            let (a, _b) = pair.establish().await;

            let over = vec![0u8; crate::constants::MESSAGE_RECV_MAX as usize + 1];
            assert!(matches!(
                a.send_message(&over).await,
                Err(crate::error::MessageError::TooLarge)
            ));
        })
        .await;
    }
}

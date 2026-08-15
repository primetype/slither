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
    ///    `ToEndpoint` arm. The **second half** is what sees that, and it
    ///    is the half that used to assert nothing: *neither*
    ///    `is_established()` *nor* `Endpoint::connect`'s `Ok` can observe
    ///    it. `release_dead` nulls `ConnCell::core` and frees the
    ///    **mirror** — `ShellState::statics`, §16.1's synchronous
    ///    admission test — whether or not the `Retired` was delivered, and
    ///    `connect()` is answered from that mirror. Only `core::Endpoint`'s
    ///    own map still holds the static, and the sole thing that reports
    ///    its answer back is the **resolved `Connecting`**: a dropped one
    ///    throws it away. So the redial is driven to a completed handshake
    ///    here rather than dropped on the next line. Under the mutation
    ///    the core answers `AlreadyConnected`, no msg1 is ever
    ///    transmitted, and `redial.await` resolves `Err`.
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

            // `connect()`'s `Ok` proves nothing: it is the mirror's
            // answer. Take a wire reading first — under the mutation the
            // endpoint core refuses before anything is sealed, so **no
            // msg1 leaves at all**, and this separates without B's
            // cooperation.
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
                .expect("the shell mirror still said LIVE after the linger");
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
            // tests `driver_stopped` *before* the static mirror, so a
            // stopped driver answers `Local` here where a live one, with A
            // still draining, answers `AlreadyConnected`.
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
}

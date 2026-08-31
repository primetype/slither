//! §16.2's `Connection` handle.
//!
//! The verb inventory and where the work happens are in [the shell module
//! docs](super); the composability layer that wraps these verbs is
//! [`crate::compat`]. `[corrected 2026/08/18 — ruling 264]`
//!
//! What is here is the shared-cell data path: every verb is written
//! **once**, as `poll_*(&self, cx, …) -> Poll<_>`, and the `async fn` §16.2
//! declares is `poll_fn` over it.

use std::cell::RefCell;
use std::future::poll_fn;
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::config::TimingProfile;
use crate::constants;
use crate::core::connection::{AckSnapshot, SendMessage, validate_persistent_keepalive};
use crate::core::{Connection as CoreConnection, ConnectionId, Dir, StreamRef};
use crate::error::{ConfigError, ConnectionLost, DatagramError, MessageError};
use crate::packet::{Channel, Handshake};

use super::shared::{ConnCell, Notification, ShellLink, WakerSlot, close_now, now};
use super::stream::{BiStream, RecvStream, SendStream};

/// The static public key type of a suite — §2.4's canonical octets.
type PublicKeyFor<S> = <<S as Channel>::Curve as hiss::curve::Curve>::PublicKey;

/// A live slither connection — one Noise session and its frame layer
/// (§16.1).
///
/// # Lifetime, and the two drop rules that are not the same rule
///
/// Dropping the **last handle to this connection** performs
/// `close(NO_ERROR, "")`: the graceful teardown of §15.2, with a CLOSE on
/// the wire.
///
/// Dropping the last handle **in the process** stops the driver and every
/// connection dies silently — **nothing is transmitted** (§15.4's
/// endpoint-dropped row).
///
/// Where the two coincide — this is the last `Connection` *and* the last
/// handle of any kind — **[RATIFIED 2026/08/15 — ruling 88]** the
/// endpoint-dropped row governs and **no CLOSE is sealed**. A synchronous
/// `Drop` cannot await the driver, and the driver is already stopping; the
/// peer's ordinary cost is bounded at this connection's representable
/// effective-dead-timeout deadline (the v1 default is `DEAD_TIMEOUT`, 25 s),
/// which that row already accepts. A deadline beyond the platform clock
/// horizon remains logically enabled and is never replaced by an earlier one.
///
/// This is documentation obligation #4 and it is drop-order sensitive: it
/// is the opposite of the obvious guess, and which of the two rules fires
/// depends on the order your values fall out of scope. Keeping an
/// [`Endpoint`](super::Endpoint) alive across the drop is what makes the
/// CLOSE happen.
///
/// # A second handle: wrap it in an `Rc`
///
/// **[RATIFIED 2026/08/18 — ruling 259(iv)]** `Connection` is deliberately
/// not `Clone` (§16.2), and the supported way to hold it from two places is
/// the one that needs nothing from this crate: `let conn = Rc::new(conn);`
/// and clone the `Rc`. Sharing works because every data-path verb takes
/// `&self` — §16.3's shared cell is already inside — so a `&Connection`
/// reached through an `Rc` can do everything an owned one can.
///
/// **Both drop rules above are unchanged by it**, and that is the point of
/// preferring an `Rc` to a `clone()`: the two rules turn on the *last*
/// handle, and an `Rc` keeps exactly one `Connection` in existence however
/// many holders it has. `close(NO_ERROR, "")` therefore fires when the last
/// `Rc` goes, not when the first one does. The driver is `!Send` and the
/// handle is `!Send` with it, so `Rc` — not `Arc` — is the right pointer.
pub struct Connection<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    id: ConnectionId,
    /// Fixed for the connection's life — §6.1 proved it before the
    /// connection existed, and nothing changes it — so it is held here
    /// rather than in the cell and keeps answering after §15.2's linger has
    /// dropped the session.
    remote_static: PublicKeyFor<S>,
    /// Fixed for the same reason, by §7.8: one session per connection, and
    /// no transport state ever crosses a handshake.
    session_id: hiss::noise::SessionId,
    /// Ruling 282's immutable profile, retained after the core is released so
    /// configuration validation has the same answer on a dead connection.
    timing_profile: TimingProfile,
}

impl<S: Handshake> Connection<S> {
    /// Total, deliberately (ruling 286): the profile is the caller's to
    /// supply, because the driver reads it off the live core it already
    /// holds — a constructor that borrowed the cell to fetch it would
    /// panic on a released core, and handles can outlive the core (that is
    /// why [`clone_handle`](Self::clone_handle) exists). The one shared
    /// `!Send` driver task must have no panic path here.
    pub(crate) fn new(
        shell: Rc<dyn ShellLink>,
        cell: Rc<RefCell<ConnCell<S>>>,
        id: ConnectionId,
        remote_static: PublicKeyFor<S>,
        session_id: hiss::noise::SessionId,
        timing_profile: TimingProfile,
    ) -> Self {
        shell.acquire();
        cell.borrow_mut().handles += 1;
        Self {
            shell,
            cell,
            id,
            remote_static,
            session_id,
            timing_profile,
        }
    }

    /// **[Integrator, ruling 239]** A second handle to this connection, with
    /// the same accounting `new` performs.
    ///
    /// **Not `Clone`, and deliberately not public.** §16.2 ratified
    /// `Connection` without a `Clone` impl and the last-handle drop rule
    /// (`close(NO_ERROR, "")`) is load-bearing; publishing this would change
    /// that surface, which slice 8 may not do.
    ///
    /// It exists so `compat::tower`'s owned `Service` impl can hand its
    /// future a handle rather than a borrow — `Service::call` takes
    /// `&mut self` and gives an anonymous lifetime that `type Future` cannot
    /// name (`Service` has no GAT), so the *only* way to build an owned
    /// future is for it to own something. This is that something, and it is
    /// **accounted**: `shell.acquire()` plus `cell.handles += 1`, exactly
    /// mirroring `Drop`, so the count is balanced and a future outliving its
    /// caller's handle cannot make `handles` reach zero early or late.
    ///
    /// # Why the `cfg`
    ///
    /// `compat::tower` is its only caller and that module is
    /// `#[cfg(feature = "tower")]`, so **without** the feature this is dead
    /// code and `-D warnings` rejects it. The gate table hid that: the two
    /// lint-bearing gates run `--all-features`, and `cargo test` runs
    /// default features without `-D warnings`, so no gate builds a
    /// combination in which this warns. The narrow fix is the `cfg`; the
    /// general one is a feature-matrix job, recorded separately.
    #[cfg(feature = "tower")]
    pub(crate) fn clone_handle(&self) -> Self {
        Self::new(
            Rc::clone(&self.shell),
            Rc::clone(&self.cell),
            self.id,
            self.remote_static.clone(),
            self.session_id.clone(),
            self.timing_profile,
        )
    }

    // No `id()` accessor. §16.2's `Connection` surface is a list, and in
    // this project a list is read as exhaustive whether or not it says so
    // (CLAUDE.md working rule 8) — `ConnectionId` appears in §16.4's *core*
    // API and nowhere on the handle. It would be useful for correlating
    // traces and it may well be worth a ruling, but adding a public
    // accessor is additive later and removing one is breaking, so the
    // absence is the reversible choice.

    /// Close the connection (§15.2).
    ///
    /// Resolves once the CLOSE frame is **sealed** and the closing state is
    /// entered — **not** once it has left the wire, and not once the peer
    /// has it. §15.2 then lingers for `CLOSE_LINGER` (5 s), replying at
    /// most once a second to authenticated, window-fresh inbound.
    ///
    /// `reason` is truncated at [`CLOSE_REASON_MAX`] (§8.4).
    ///
    /// A second `close()`, or one on a connection that has already died, is
    /// a no-op: a connection dies once, and §16.4 emits its `Closed` once.
    ///
    /// [`CLOSE_REASON_MAX`]: crate::constants::CLOSE_REASON_MAX
    pub async fn close(&self, code: u64, reason: &[u8]) {
        poll_fn(|cx| self.poll_close(cx, code, reason)).await
    }

    /// The one implementation of [`close`](Self::close) (§16.3, ruling 53).
    ///
    /// Always `Ready` on the first poll, and that is the specification
    /// rather than a shortcut: §16.7 makes sealing synchronous **inside the
    /// mutating call that triggers it**, and §16.2 resolves `close()` at
    /// the seal. The `cx` is unused for exactly that reason — the verb is
    /// written in the poll form because §16.3 requires every data-path verb
    /// to have one, and because slice 4's `AsyncWrite` shutdown path is the
    /// same function with its error mapped.
    fn poll_close(&self, _cx: &mut Context<'_>, code: u64, reason: &[u8]) -> Poll<()> {
        self.close_now(code, reason);
        Poll::Ready(())
    }

    /// Seal the CLOSE and mark the cell dirty. Shared by
    /// [`poll_close`](Self::poll_close) and the last-handle drop.
    ///
    /// The body lives in [`shared::close_now`] because ruling 115 gave the
    /// stream handles the same last-handle obligation, and two copies would
    /// be two places to get §16.7's seal-then-signal order wrong.
    ///
    /// [`shared::close_now`]: super::shared::close_now
    fn close_now(&self, code: u64, reason: &[u8]) {
        close_now(&self.shell, &self.cell, self.id, code, reason);
    }

    /// Resolve when this connection ends, with the reason it ended
    /// (**ruling 46**).
    ///
    /// Every row of §15.4's teardown matrix resolves it. It is a **latched**
    /// signal, not a queue:
    ///
    /// * **cancel-safe** — a dropped future has consumed nothing and leaves
    ///   the waker map exactly as it found it;
    /// * **concurrent** — any number of tasks may await it and all of them
    ///   resolve;
    /// * **permanent** — after the death it resolves immediately, with the
    ///   same value, for ever, including for a future first awaited *after*
    ///   the death.
    ///
    /// On a healthy connection it never resolves, which is what makes it
    /// the `select!` arm of a long-running loop.
    ///
    /// A `closed()` future is **not a handle** (§16.3, ruling 62): holding
    /// one while dropping every [`Connection`] still stops the driver and
    /// still kills the session silently.
    pub async fn closed(&self) -> ConnectionLost {
        // The key is minted once, before the first poll, and released by
        // the guard's `Drop` — including on cancellation. This is the whole
        // of the verb's cancel-safety.
        let cell = Rc::clone(&self.cell);
        let key = cell.borrow_mut().closed_wakers.key();
        let slot = WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().closed_wakers.unpark(key)
        });
        poll_fn(|cx| self.poll_closed(cx, slot.key())).await
    }

    /// The one implementation of [`closed`](Self::closed) (§16.3, ruling
    /// 53).
    fn poll_closed(&self, cx: &mut Context<'_>, key: u64) -> Poll<ConnectionLost> {
        let mut cell = self.cell.borrow_mut();
        match cell.closed.clone() {
            Some(lost) => Poll::Ready(lost),
            None => {
                cell.closed_wakers.park(key, cx);
                Poll::Pending
            }
        }
    }

    /// Claim **one** [`Notification`] — §16.2's narrow per-connection event
    /// stream (ruling 46).
    ///
    /// The same pull model as [`accept_bi`](Self::accept_bi),
    /// [`recv_message`](Self::recv_message) and
    /// [`recv_datagram`](Self::recv_datagram): the connection **retains**
    /// what has not been claimed, and this hands over exactly one, so a
    /// notification is never dropped on the floor between an application's
    /// two visits. Retention is **one slot per kind**, never a queue — two
    /// unclaimed roams merge into the net move rather than accumulating.
    ///
    /// Resolves `Err(ConnectionLost)` once the connection has ended **and**
    /// its unclaimed notifications have been drained: a notification
    /// generated before the death is not lost to the death (ruling 152).
    ///
    /// **Cancel-safe**: a dropped future has claimed nothing, and the next
    /// call yields the same notification.
    ///
    /// ```no_run
    /// use slither::prelude::*;
    ///
    /// # async fn example<S: slither::packet::Handshake>(conn: &Connection<S>) {
    ///
    /// while let Ok(notification) = conn.notified().await {
    ///     match notification {
    ///         Notification::AddressMoved { from, to } => {
    ///             tracing::info!(%from, %to, "the peer moved");
    ///         }
    ///         Notification::Contested => {
    ///             tracing::warn!("someone else claims this peer's identity");
    ///         }
    ///         Notification::ContestCleared => {
    ///             tracing::info!("the peer answered; the connection stands");
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # }
    /// ```
    pub async fn notified(&self) -> Result<Notification, ConnectionLost> {
        let slot = self.notification_slot();
        poll_fn(|cx| self.poll_notified(cx, slot.key())).await
    }

    /// The one implementation of [`notified`](Self::notified).
    ///
    /// Precedence is ruling 128's inversion, on
    /// [`poll_recv_datagram`](Self::poll_recv_datagram)'s terms exactly:
    ///
    /// 1. a slot has something → `Ready(Ok(notification))`;
    /// 2. the death latch → `Ready(Err(lost))`, **only** once every slot is
    ///    drained;
    /// 3. no core and no latch → `debug_assert!` and `EndpointDropped`;
    /// 4. otherwise park in `notification_wakers`.
    ///
    /// **Parking is never permitted on a dead connection** (ruling 128):
    /// nothing further can arrive, so step 4 is unreachable past the latch.
    ///
    /// **[ruling 58]** At most one item, and only inside the poll. An
    /// adapter that claimed ahead of its consumer would rebuild the
    /// unbounded shell queue §10.6 forbids while looking like a
    /// convenience.
    pub(crate) fn poll_notified(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<Notification, ConnectionLost>> {
        let mut cell = self.cell.borrow_mut();
        if let Some(notification) = cell.notifications.take_oldest() {
            return Poll::Ready(Ok(notification));
        }
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(lost));
        }
        if cell.core.is_none() {
            debug_assert!(
                false,
                "a connection cell held neither a core nor a close reason (§16.3)"
            );
            return Poll::Ready(Err(ConnectionLost::EndpointDropped));
        }
        cell.notification_wakers.park(key, cx);
        Poll::Pending
    }

    /// Mint this future's slot in §16.2's notification-waiter set.
    fn notification_slot(&self) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().notification_wakers.key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().notification_wakers.unpark(key)
        })
    }

    /// §7.5's persistent keepalive — the beacon. `None` disables it, which
    /// is the default.
    ///
    /// **This is not what keeps an ordinary connection alive.** §7.5's
    /// passive dance is automatic for any connection that has carried
    /// traffic and needs no opt-in: one message in one direction is enough
    /// to make a pair self-sustaining indefinitely. The beacon is for a link
    /// that is **mutually idle** — one that must stay open through a NAT
    /// whose binding would otherwise lapse, or through a firewall that reaps
    /// idle flows.
    ///
    /// # The admissible band
    ///
    /// `[1 s, effective dead timeout)` — **1 s inclusive and the ceiling
    /// exclusive**. With [`TimingProfile::default`] that is `[1 s, 25 s)`.
    /// Outside it the call returns [`ConfigError`] and **leaves the current
    /// interval unchanged**: it never panics (which would be undefined
    /// behaviour across an FFI boundary) and it never silently clamps
    /// (which would report success while giving a beacon that does not do
    /// what was asked).
    ///
    /// Below 1 s the beacon is a flood; at or above the effective dead
    /// timeout it cannot keep a connection alive at all, because a beacon at
    /// exactly that interval arrives, at best, simultaneously with the
    /// verdict.
    ///
    /// # A beacon does not defer death
    ///
    /// Both keepalives are **marking** sends, so they *arm* §7.4's death
    /// clock rather than postponing it. A connection whose entire output is
    /// beacons and which never hears back still ends at its effective dead
    /// timeout after the last authenticated packet it received. That is the
    /// point: the beacon keeps a *path* open, and the peer's answers are what
    /// keep the *connection* alive.
    ///
    /// Synchronous — a shared-cell write like the accessors, not a command
    /// round trip. On a connection that has already ended, `None` still
    /// succeeds and a `Some(_)` is still validated.
    pub fn set_persistent_keepalive(&self, interval: Option<Duration>) -> Result<(), ConfigError> {
        // Validated before the cell is touched, so a dead connection gives
        // the same verdict a live one would: the band is a property of the
        // value, not of the connection's state.
        validate_persistent_keepalive(interval, self.timing_profile.dead_timeout())?;
        let applied = {
            let mut cell = self.cell.borrow_mut();
            match cell.core.as_mut() {
                Some(core) => {
                    let result = core.set_persistent_keepalive(now(), interval);
                    debug_assert!(result.is_ok(), "the band was checked immediately above");
                    cell.dirty = true;
                    true
                }
                None => false,
            }
        };
        if applied {
            self.shell.mark_dirty(self.id);
        }
        Ok(())
    }

    /// The configured persistent-keepalive interval, or `None` when the
    /// beacon is off (§7.5).
    ///
    /// **[RATIFIED 2026/08/16 — ruling 189]** This exists because ruling 44
    /// makes *"a rejected call leaves the interval **unchanged**"* an
    /// acceptance criterion that nothing in §16.2's surface could observe.
    /// Without it the obligation is testable only by inferring the interval
    /// from beacon cadence across a long timing window, bracketed from both
    /// sides so that neither an upward nor a downward clamp survives — which
    /// a blind test author did, and should not have had to.
    /// A configuration setter whose effect cannot be read back is the
    /// defect; this is the fix.
    ///
    /// A shared-cell read like the other accessors (§16.8), never a command
    /// round trip.
    ///
    /// On a connection whose core is gone this reports `None`, and that is
    /// consistent rather than lossy: `set_persistent_keepalive` on a dead
    /// connection validates the band and then stores nothing, so there is
    /// no configured beacon to report. The shell keeps **no mirror** of the
    /// interval — a second copy would be a second source of truth for a
    /// value the core already owns.
    pub fn persistent_keepalive(&self) -> Option<Duration> {
        let cell = self.cell.borrow();
        cell.core
            .as_ref()
            .and_then(|core| core.persistent_keepalive())
    }

    /// Resolve once everything handed to this connection **so far** has
    /// been acknowledged by the peer's transport (**rulings 47 and 54**).
    ///
    /// This is the verb behind `send(msg).await; acked().await;
    /// close(NO_ERROR, "").await` — §15.2 lets `close()` drop stream,
    /// recovery and congestion state immediately, so without it a
    /// write-then-close loses its tail at the path's loss rate, silently.
    ///
    /// # It is a snapshot, taken at the call
    ///
    /// Every byte handed to the connection at this instant, across **every**
    /// stream — including the message streams §9.8 never surfaces a handle
    /// for. Bytes written *after* the call do not extend it, which is what
    /// makes it terminate on a live connection even while a bulk stream is
    /// still being written. A byte **abandoned by a reset** counts as
    /// settled (§9.6: an abandoned byte is never acknowledged, and waiting
    /// on one would never terminate).
    ///
    /// **The FIN is not part of it.** A snapshot that waited for FINs would
    /// never terminate on a stream the application intends to keep open;
    /// [`SendStream::acked`] is the per-stream verb that includes the FIN.
    ///
    /// # After the connection dies
    ///
    /// A snapshot that is **settled** resolves `Ok(())` even once the
    /// connection has died — **ruling 135**. The peer's ACK and the peer's
    /// CLOSE can arrive in one driver pass, and the application is woken
    /// after the latch is set, so the alternative reports `ConnectionLost`
    /// over a transfer that was fully delivered and fully acknowledged, in
    /// a race the sender cannot win. An **unsettled** snapshot reports
    /// `Err(ConnectionLost)`: nothing further can be acknowledged, so it
    /// never parks (ruling 128).
    ///
    /// Once the driver has released the core there is no snapshot left to
    /// test and this reports `Err(ConnectionLost)` unconditionally.
    ///
    /// # Cancel-safety
    ///
    /// **Cancel-safe: a dropped future has claimed nothing.** It observes;
    /// it consumes no bytes and mutates no core state, and its waker slot
    /// is released by the guard's `Drop`.
    ///
    /// [`SendStream::acked`]: super::SendStream::acked
    pub async fn acked(&self) -> Result<(), ConnectionLost> {
        // **Taken here, in the body — not inside `poll_acked`** (rulings
        // 47/54). §16.2 fixes the snapshot at the instant of the call;
        // re-reading the send offsets on every poll is the implementation
        // that never terminates under a writer loop, which is the case
        // §16.2 spells out. There is no `await` between this and the first
        // poll, so "at the call" and "at the first poll" are the same
        // instant on this runtime.
        let snapshot = self
            .cell
            .borrow()
            .core
            .as_ref()
            .map(CoreConnection::ack_snapshot);
        let slot = self.settled_slot();
        poll_fn(|cx| self.poll_acked(cx, snapshot.as_ref(), slot.key())).await
    }

    /// The one implementation of [`acked`](Self::acked) (§16.3, ruling 53).
    fn poll_acked(
        &self,
        cx: &mut Context<'_>,
        snapshot: Option<&AckSnapshot>,
        key: u64,
    ) -> Poll<Result<(), ConnectionLost>> {
        let mut cell = self.cell.borrow_mut();
        // **[RATIFIED 2026/08/16 — ruling 135]** The settled snapshot
        // outranks the death latch, exactly as ruling 124's terminal state
        // does on a stream handle: *a stream whose bytes were acknowledged
        // completed, and the connection dying afterwards does not
        // un-complete it.* An empty snapshot — nothing was ever written —
        // is settled vacuously and resolves here on the first poll, live or
        // dead.
        if let Some(snap) = snapshot
            && cell
                .core
                .as_ref()
                .is_some_and(|core| core.snapshot_settled(snap))
        {
            return Poll::Ready(Ok(()));
        }
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(lost));
        }
        if cell.core.is_none() {
            debug_assert!(
                false,
                "a connection cell held neither a core nor a close reason (§16.3)"
            );
            return Poll::Ready(Err(ConnectionLost::EndpointDropped));
        }
        cell.settled_wakers.park(key, cx);
        Poll::Pending
    }

    /// Mint this future's slot in §16.8's settled-waiter set, released on
    /// drop — [`closed`](Self::closed)'s shape, for the same reason: any
    /// number of `acked()` futures can coexist on one `&self`.
    fn settled_slot(&self) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().settled_wakers.key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().settled_wakers.unpark(key)
        })
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.2's stream verbs
    // ═══════════════════════════════════════════════════════════════════

    /// Open a bidirectional stream (§9.1).
    ///
    /// When §10.4's cumulative limit is exhausted this waits for a
    /// MAX_STREAMS allowance rather than failing: `StreamsExhausted` is a
    /// core-internal condition and no public verb can return it (ruling
    /// 101).
    ///
    /// # A freed bidi index returns the peer's allowance
    ///
    /// A bidi index is returned to the peer's allowance only when **both**
    /// halves are freed, and the send half is freed on acknowledgement
    /// (§12's ACK processing, `Streams::on_ack_range`). `open_uni` has no
    /// such dependency: a peer-opened uni stream read to end-of-stream is
    /// fully closed at once and grants its MAX_STREAMS_UNI.
    /// `[corrected 2026/08/18 — ruling 264]`
    ///
    /// # Cancel-safety
    ///
    /// **Cancel-safe: a dropped future has claimed nothing.** The stream
    /// index and the handle are taken in one synchronous step with no
    /// fallible operation between them, so there is no state in which an
    /// index has been spent on a stream no handle names.
    pub async fn open_bi(&self) -> Result<BiStream<S>, ConnectionLost> {
        let slot = self.opener_slot(Dir::Bi);
        poll_fn(|cx| self.poll_open_bi(cx, slot.key())).await
    }

    /// Open a unidirectional stream — this end sends, the peer receives
    /// (§9.1).
    ///
    /// Waits for a MAX_STREAMS allowance when §10.4's cumulative limit is
    /// exhausted, and unlike [`open_bi`](Self::open_bi) that wait is
    /// satisfiable in this slice: the peer's uni streams close as soon as
    /// their receive half is retired.
    ///
    /// # Do not mix with [`send_message`](Self::send_message) — S30
    ///
    /// §9.8's messages are **sugar over auto-managed unidirectional
    /// streams**: they open, fill and finish uni streams of their own. An
    /// application that uses both the message verb and the raw uni verbs on
    /// **one connection** cannot tell which uni streams are its own, and
    /// slither will not guess — this is a **programming error with a
    /// defined loud failure**, not a silent interleaving.
    ///
    /// Pick one per connection. The two shapes look identical at the call
    /// site, which is exactly why this warning is on all three of
    /// `send_message`, `open_uni` and `accept_uni` rather than in one place.
    ///
    /// Cancel-safe, for [`open_bi`](Self::open_bi)'s reason.
    pub async fn open_uni(&self) -> Result<SendStream<S>, ConnectionLost> {
        let slot = self.opener_slot(Dir::Uni);
        poll_fn(|cx| self.poll_open_uni(cx, slot.key())).await
    }

    /// Claim the next peer-opened bidirectional stream (§9.1).
    ///
    /// **FIFO, in open order** (ruling 112). §9.2's implicit opening can
    /// open several streams from one frame; each becomes claimable
    /// separately and each `accept_bi` claims exactly one.
    ///
    /// # After the connection ends — **ruling 128**
    ///
    /// Streams the peer opened **before** the death are still handed over,
    /// and the handle they come back on is usable: ruling 128's drain
    /// applies to [`RecvStream::read`] too. Ruling 118 said the opposite,
    /// and was reasoning about the **closing** endpoint, where §15.2 really
    /// does free stream state; a *draining* endpoint — one that received an
    /// authenticated CLOSE — keeps it for `CLOSE_LINGER` precisely so this
    /// can happen. The case that forces it is the ordinary one: a sender
    /// that writes, finishes and drops its handles closes implicitly, the
    /// peer's driver processes the data and the CLOSE in one pass, and the
    /// peer's application is woken **after** the latch is set. It is not a
    /// race the receiver can win.
    ///
    /// When nothing is left to claim this reports `Err(ConnectionLost)` on
    /// the first poll and **never parks**: nothing further can arrive.
    ///
    /// The drain window is the core's own: `CLOSE_LINGER` (5 s) after a
    /// peer CLOSE, and **zero** on the deaths that have no linger —
    /// liveness timeout, nonce exhaustion, `Replaced`, endpoint dropped
    /// (ruling 133). A receiver killed by its effective dead timeout
    /// mid-transfer cannot drain, which is honest: a path that produced no
    /// CLOSE produced no finished sender either. After a **local** `close()`
    /// the window is the linger as well — see [`RecvStream::read`] for the
    /// one row of it ruling 133 expects to change.
    ///
    /// [`RecvStream::read`]: super::RecvStream::read
    ///
    /// Cancel-safe: a dropped future has claimed no stream. The claim and
    /// the handle are one step, which matters more here than for `open_*` —
    /// a popped stream with no handle would be unclaimable for ever while
    /// the peer's bytes went on charging the receive ledger.
    pub async fn accept_bi(&self) -> Result<BiStream<S>, ConnectionLost> {
        let slot = self.acceptor_slot(Dir::Bi);
        poll_fn(|cx| self.poll_accept_bi(cx, slot.key())).await
    }

    /// Claim the next peer-opened unidirectional stream (§9.1). FIFO, and
    /// cancel-safe, exactly as [`accept_bi`](Self::accept_bi).
    ///
    /// # Do not mix with [`send_message`](Self::send_message) — S30
    ///
    /// §9.8's messages are **sugar over auto-managed unidirectional
    /// streams**: they open, fill and finish uni streams of their own. An
    /// application that uses both the message verb and the raw uni verbs on
    /// **one connection** cannot tell which uni streams are its own, and
    /// slither will not guess — this is a **programming error with a
    /// defined loud failure**, not a silent interleaving.
    ///
    /// Pick one per connection. The two shapes look identical at the call
    /// site, which is exactly why this warning is on all three of
    /// `send_message`, `open_uni` and `accept_uni` rather than in one place.
    ///
    pub async fn accept_uni(&self) -> Result<RecvStream<S>, ConnectionLost> {
        let slot = self.acceptor_slot(Dir::Uni);
        poll_fn(|cx| self.poll_accept_uni(cx, slot.key())).await
    }

    /// The one implementation of [`open_bi`](Self::open_bi) (ruling 122a).
    pub(crate) fn poll_open_bi(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<BiStream<S>, ConnectionLost>> {
        self.poll_open_with(cx, key, Dir::Bi, install_bi)
    }

    /// The one implementation of [`open_uni`](Self::open_uni).
    pub(crate) fn poll_open_uni(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<SendStream<S>, ConnectionLost>> {
        self.poll_open_with(cx, key, Dir::Uni, SendStream::install)
    }

    /// The one implementation of [`accept_bi`](Self::accept_bi).
    pub(crate) fn poll_accept_bi(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<BiStream<S>, ConnectionLost>> {
        self.poll_accept_with(cx, key, Dir::Bi, install_bi)
    }

    /// The one implementation of [`accept_uni`](Self::accept_uni).
    pub(crate) fn poll_accept_uni(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<RecvStream<S>, ConnectionLost>> {
        self.poll_accept_with(cx, key, Dir::Uni, RecvStream::install)
    }

    // ═══════════════════════════════════════════════════════════════════
    // §9.8's messages and §11's datagrams
    // ═══════════════════════════════════════════════════════════════════

    /// Send one reliable-unordered message (§9.8).
    ///
    /// Sugar over an auto-managed unidirectional stream: the next outbound
    /// uni stream is allocated, the whole payload written, the FIN set, and
    /// the stream garbage-collected once the FIN'd range is acknowledged.
    /// **No stream handle surfaces**, so [`acked`](Self::acked) is how an
    /// application awaits that acknowledgement — the idiom is
    /// `send_message(m).await; acked().await; close().await`.
    ///
    /// # Example
    ///
    /// Two endpoints on the loopback. The answerer climbs §6.2's staged
    /// ladder and claims the message with
    /// [`recv_message`](Self::recv_message); the dialler sends it, waits for
    /// the acknowledgement, and closes.
    ///
    /// ```no_run
    /// # use slither::prelude::*;
    /// # use rand_chacha::ChaCha20Rng;
    /// # use rand_chacha::rand_core::SeedableRng;
    /// # slither::channel! { pub MySuite<P256, ChaChaPoly, Blake2b>; }
    /// # fn seeded() -> Result<ChaCha20Rng, Box<dyn std::error::Error>> {
    /// #     let mut seed = [0u8; 32];
    /// #     getrandom::fill(&mut seed)?;
    /// #     Ok(ChaCha20Rng::from_seed(seed))
    /// # }
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let dialler: SoftwareIdentity<MySuite> = SoftwareIdentity::generate(seeded()?)?;
    /// # let answerer: SoftwareIdentity<MySuite> = SoftwareIdentity::generate(seeded()?)?;
    /// block_on(async move {
    /// #     let answerer_key = *answerer.public_static();
    /// #     let dial_sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    /// #     let ans_sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    /// #     let answerer_addr = ans_sock.local_addr()?;
    ///     let dial_ep = Endpoint::builder()
    ///         .identity(dialler)
    ///         .wire(dial_sock)
    ///         .config(Config::new())
    ///         .build();
    ///     let answer_ep = Endpoint::builder()
    ///         .identity(answerer)
    ///         .wire(ans_sock)
    ///         .config(Config::new())
    ///         .build();
    ///
    ///     // The answerer: one introduction up the ladder, one DH at a
    ///     // time, then the message. A real server loops on `accept()`.
    ///     let answering = tokio::task::spawn_local(async move {
    ///         let intro = answer_ep.accept().await.expect("an introduction");
    ///         let claimed = intro.read_identity().await.expect("+1 DH: es");
    ///         let proven = claimed.authenticate().await.expect("+1 DH: ss");
    ///         let conn = proven.accept().await.expect("+2 DH: ee, se");
    ///
    ///         let msg = conn.recv_message().await.expect("the message");
    ///         assert_eq!(&msg[..], b"hello");
    ///
    ///         // Hold the session open until the dialler closes it. Dropping
    ///         // the connection here would send a CLOSE that can beat the
    ///         // acknowledgement the dialler is waiting on — and `acked()`
    ///         // would then resolve `PeerClosed` instead of `Ok`.
    ///         let _ = conn.closed().await;
    ///     });
    ///
    ///     // The dialler: Appendix B's obligation, in three lines.
    ///     let conn = dial_ep.connect(answerer_addr, answerer_key)?.await?;
    ///     conn.send_message(b"hello").await?;
    ///     conn.acked().await?;
    ///     conn.close(slither::constants::NO_ERROR, b"done").await;
    ///
    ///     answering.await?;
    ///     Ok(())
    /// })
    /// # }
    /// ```
    ///
    /// Payloads above `MESSAGE_RECV_MAX` (262 144 B) are rejected here with
    /// [`MessageError::TooLarge`], **at the handle** and before the core is
    /// consulted: a larger sugar send could stall for ever against a
    /// sugar-consuming receiver, which never extends credit.
    ///
    /// It resolves *"as soon as the payload entered send state"* (§9.8) —
    /// not on delivery, and not on acknowledgement.
    ///
    /// # Waiting, and what it waits for
    ///
    /// It waits for §10.4's stream allowance or for enough §10.3
    /// connection credit to admit the **whole** payload. §10.6 makes credit
    /// the buffer commitment, so partial admission is not available:
    /// accepting beyond the peer's credit would buffer up to 32 MiB per
    /// connection, a term §17.5's ceiling table does not contain (ruling
    /// 150).
    ///
    /// # Cancel-safety — **load-bearing here**
    ///
    /// **Cancel-safe: a dropped future has sent nothing.** No stream is
    /// opened, no index spent and no byte buffered until the whole payload
    /// is admitted, which happens in one synchronous core call.
    ///
    /// This is worth stating because the natural decomposition — open,
    /// write in a loop, finish — is **not** cancel-safe in a way that
    /// matters: a dropped future would leave a **FIN-less half-written uni
    /// stream** on the wire, which against a message-mode receiver is
    /// exactly §9.8's overflow case. An application's own
    /// `timeout(d, send_message(..))` would then manufacture the failure
    /// this protocol's diagnostics exist to attribute to a mixing error.
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError> {
        let slot = self.message_sender_slot();
        poll_fn(|cx| self.poll_send_message(cx, msg, slot.key())).await
    }

    /// Claim the oldest complete unclaimed message (§9.8).
    ///
    /// Each incoming unidirectional stream is one message; the payload
    /// surfaces only when reassembly is complete — **FIN and every byte** —
    /// and the stream is freed by the claim. *"Oldest"* is **open order**
    /// (ruling 112): a complete stream sitting behind an incomplete one is
    /// surfaced, because messages are reliable-**unordered**.
    ///
    /// `Ok(payload)` with an empty `Vec` is a delivered **empty message**,
    /// not an absence.
    ///
    /// The worked pair — an answerer claiming what a dialler sent — is the
    /// example on [`send_message`](Self::send_message).
    ///
    /// # Mixing this with `accept_uni()` is a programming error
    ///
    /// Both verbs draw from the same incoming-uni supply and the wire
    /// carries **no discriminator** between the two modes, so the receiving
    /// application's verb choice alone decides how a stream is interpreted
    /// and no implementation can repair a mixture. Calling this verb puts
    /// the connection in message mode: from the first call, an unclaimed uni
    /// stream that fills its initial window without pinning a final size is
    /// **reset** with `MESSAGE_OVERFLOW`, and its sender sees
    /// `WriteError::Reset(0x06)`. Use bidi streams alongside messages, or
    /// tag in band. §9.8 states both safe patterns.
    ///
    /// # After the connection ends — **ruling 152**
    ///
    /// Messages that arrived complete **before** the death are still
    /// handed over, and the death is reported only once none is left.
    /// Ruling 128's enumeration named `read` and `accept_*` and predates
    /// this verb; the obligation it serves is Appendix B's
    /// `send_message` → `acked` → `close`, where the receiver's driver
    /// processes the data and the CLOSE in one pass and cannot win the race
    /// by being prompt.
    ///
    /// Cancel-safe: the claim and the return are one expression, so a
    /// dropped future has claimed nothing.
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost> {
        let slot = self.message_reader_slot();
        poll_fn(|cx| self.poll_recv_message(cx, slot.key())).await
    }

    /// Queue one unreliable datagram (§11).
    ///
    /// **Not `async`, and it never waits.** §11.3's send queue is bounded at
    /// 64 with a drop-oldest discipline, so pressure evicts the *oldest*
    /// queued datagram rather than blocking the caller or rejecting the new
    /// one — and `Ok(())` therefore promises only that the datagram entered
    /// the queue. §11.1 promises nothing beyond that: no delivery, no
    /// ordering, no retransmission, no sequence identity at all.
    ///
    /// A payload above `MAX_DATAGRAM_PAYLOAD` (1169 B) is
    /// [`DatagramError::TooLarge`], rejected **before any queue** (§11.4):
    /// nothing is queued and nothing is evicted.
    ///
    /// Its cancel-safety is vacuous — there is no future to drop.
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError> {
        // §11.4's bound is checked at the handle. Doing it before the borrow
        // keeps the oversize path from marking the cell dirty for a call
        // that changed nothing.
        if data.len() > constants::MAX_DATAGRAM_PAYLOAD {
            return Err(DatagramError::TooLarge);
        }
        let sent = {
            let mut cell = self.cell.borrow_mut();
            if let Some(lost) = cell.closed.clone() {
                return Err(DatagramError::ConnectionLost(lost));
            }
            let Some(core) = cell.core.as_mut() else {
                debug_assert!(
                    false,
                    "a connection cell held neither a core nor a close reason (§16.3)"
                );
                return Err(DatagramError::ConnectionLost(
                    ConnectionLost::EndpointDropped,
                ));
            };
            let sent = core.send_datagram(now(), data);
            if sent.is_ok() {
                cell.dirty = true;
            }
            sent
        };
        if sent.is_ok() {
            self.shell.mark_dirty(self.id);
        }
        sent
    }

    /// Claim the oldest queued datagram (§11), waiting for one to arrive.
    ///
    /// FIFO over §11.3's receive queue, which is bounded at 64 with the same
    /// drop-oldest discipline: a datagram this end never claimed can be
    /// evicted by a newer arrival, and nothing reports that to the
    /// application — §11.1 promises no delivery, and §11.5's counters are an
    /// operator signal on `slither::frames`, not an error.
    ///
    /// Drains after the connection's death on
    /// [`recv_message`](Self::recv_message)'s terms (ruling 152), and is
    /// cancel-safe for the same reason.
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost> {
        let slot = self.datagram_reader_slot();
        poll_fn(|cx| self.poll_recv_datagram(cx, slot.key())).await
    }

    /// The one implementation of [`send_message`](Self::send_message)
    /// (ruling 122a).
    ///
    /// Precedence, in order — the house order of ruling 124, with §9.8's
    /// handle-side bound ahead of it:
    ///
    /// 1. `msg.len() > MESSAGE_RECV_MAX` → `TooLarge`. §9.8 puts the check
    ///    *"at the handle"*, and the core re-checks only because its own
    ///    unit tests call it directly. Ruling 110's `buf.is_empty()` check
    ///    is the house precedent for a shell-side guard;
    /// 2. the death latch → `ConnectionLost`. **Latch first**, unlike the
    ///    claim verbs below: there is nothing buffered for a *send* to
    ///    drain, so ruling 128's inversion has nothing to protect here;
    /// 3. no core → `debug_assert!` and `EndpointDropped`;
    /// 4. the core admits the payload → `Ok(())`;
    /// 5. the core says *not now* → park in `message_senders`.
    pub(crate) fn poll_send_message(
        &self,
        cx: &mut Context<'_>,
        msg: &[u8],
        key: u64,
    ) -> Poll<Result<(), MessageError>> {
        if msg.len() as u64 > constants::MESSAGE_RECV_MAX {
            return Poll::Ready(Err(MessageError::TooLarge));
        }
        let admitted = {
            let mut cell = self.cell.borrow_mut();
            if let Some(lost) = cell.closed.clone() {
                return Poll::Ready(Err(MessageError::ConnectionLost(lost)));
            }
            let Some(core) = cell.core.as_mut() else {
                debug_assert!(
                    false,
                    "a connection cell held neither a core nor a close reason (§16.3)"
                );
                return Poll::Ready(Err(MessageError::ConnectionLost(
                    ConnectionLost::EndpointDropped,
                )));
            };
            match core.send_message(now(), msg) {
                Err(error) => return Poll::Ready(Err(error)),
                Ok(SendMessage::Blocked) => {
                    // Ruling 150: nothing happened, so parking is the whole
                    // of the retry. Woken by `StreamsAvailable { Uni }` or
                    // `SendCreditAvailable`, and by the death latch.
                    cell.message_senders.park(key, cx);
                    false
                }
                Ok(SendMessage::Sent) => {
                    cell.dirty = true;
                    true
                }
            }
        };
        if !admitted {
            return Poll::Pending;
        }
        self.shell.mark_dirty(self.id);
        Poll::Ready(Ok(()))
    }

    /// The one implementation of [`recv_message`](Self::recv_message).
    ///
    /// Precedence, in order — **the core first**, which is ruling 128's
    /// inversion extended to this verb by ruling 152:
    ///
    /// 1. `core.recv_message(now())` → `Ready(Ok(payload))` on `Some`;
    /// 2. the death latch → `Ready(Err(lost))`, **only** if the core had
    ///    nothing;
    /// 3. no core → `debug_assert!` and `EndpointDropped`;
    /// 4. otherwise park in `message_readers`.
    ///
    /// **Parking is never permitted on a dead connection** (ruling 128):
    /// nothing further can arrive, so step 4 is unreachable past the latch.
    pub(crate) fn poll_recv_message(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<Vec<u8>, ConnectionLost>> {
        let (outcome, called) = {
            let mut cell = self.cell.borrow_mut();
            // Every call — `Some` or `None` — can have run §9.8's overflow
            // scan and emitted a RESET_STREAM, and a claim retires a half
            // that owes MAX_DATA and MAX_STREAMS. So the cell is dirtied on
            // the strength of the *call*, not of the answer — and left
            // alone when there was no core to call.
            let claimed = cell.core.as_mut().map(|core| core.recv_message(now()));
            let called = claimed.is_some();
            if called {
                cell.dirty = true;
            }
            let outcome = match claimed {
                Some(Some(payload)) => Some(Ok(payload)),
                _ => match cell.closed.clone() {
                    Some(lost) => Some(Err(lost)),
                    None if cell.core.is_none() => {
                        debug_assert!(
                            false,
                            "a connection cell held neither a core nor a close reason (§16.3)"
                        );
                        Some(Err(ConnectionLost::EndpointDropped))
                    }
                    None => {
                        cell.message_readers.park(key, cx);
                        None
                    }
                },
            };
            (outcome, called)
        };
        // Outside the borrow (finding F10): driving re-enters the cell.
        if called {
            self.shell.mark_dirty(self.id);
        }
        match outcome {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }

    /// The one implementation of [`recv_datagram`](Self::recv_datagram).
    ///
    /// [`poll_recv_message`](Self::poll_recv_message)'s precedence exactly,
    /// with one difference: **the core call takes no `now` and dirties
    /// nothing** (ruling 151). Claiming a datagram emits no frame —
    /// datagrams are flow-control exempt (§10.7), so there is no credit
    /// true-up and nothing to seal — and marking the cell dirty for it would
    /// wake the driver to discover that nothing is owed.
    pub(crate) fn poll_recv_datagram(
        &self,
        cx: &mut Context<'_>,
        key: u64,
    ) -> Poll<Result<Vec<u8>, ConnectionLost>> {
        let mut cell = self.cell.borrow_mut();
        if let Some(payload) = cell.core.as_mut().and_then(CoreConnection::recv_datagram) {
            return Poll::Ready(Ok(payload));
        }
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(lost));
        }
        if cell.core.is_none() {
            debug_assert!(
                false,
                "a connection cell held neither a core nor a close reason (§16.3)"
            );
            return Poll::Ready(Err(ConnectionLost::EndpointDropped));
        }
        cell.datagram_readers.park(key, cx);
        Poll::Pending
    }

    /// Mint this future's slot in §9.8's message-reader set.
    fn message_reader_slot(&self) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().message_readers.key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().message_readers.unpark(key)
        })
    }

    /// Mint this future's slot in §11's datagram-reader set.
    fn datagram_reader_slot(&self) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().datagram_readers.key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().datagram_readers.unpark(key)
        })
    }

    /// Mint this future's slot in §9.8's message-sender set.
    fn message_sender_slot(&self) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().message_senders.key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().message_senders.unpark(key)
        })
    }

    /// Mint this future's slot in §16.8's opener map, released on drop.
    ///
    /// `open_*`/`accept_*` keep the per-future [`WakerSlot`] shape that
    /// `closed()` uses — several `open_bi()` futures can coexist on one
    /// `&self` — whereas the stream handles hold their key in a field,
    /// because slice 8's `AsyncWrite::poll_write` has no argument to carry
    /// one (§16.3, ruling 53).
    fn opener_slot(&self, dir: Dir) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().stream_openers[dir.slot()].key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().stream_openers[dir.slot()].unpark(key)
        })
    }

    /// Mint this future's slot in §16.8's acceptor map.
    fn acceptor_slot(&self, dir: Dir) -> WakerSlot<impl FnMut(u64)> {
        let key = self.cell.borrow_mut().stream_acceptors[dir.slot()].key();
        WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().stream_acceptors[dir.slot()].unpark(key)
        })
    }

    /// `open_bi`/`open_uni`, differing only in what they build.
    ///
    /// # `build` runs under the same borrow as `core.open`, deliberately
    ///
    /// `core.open` increments `ever_opened` and spends an index against
    /// §10.4's cumulative limit. A future dropped **after** the index was
    /// spent and **before** the handle existed would leak it for the
    /// connection's life, with a send half nothing can ever finish and a
    /// contribution pinned in the ledger. That state is unreachable if and
    /// only if the core call and the construction happen in one synchronous
    /// body with no `?`, no early return and nothing fallible between them —
    /// so `build` takes the live `&mut ConnCell` rather than the cell, and
    /// the window is not merely unlikely but unrepresentable.
    fn poll_open_with<T>(
        &self,
        cx: &mut Context<'_>,
        key: u64,
        dir: Dir,
        build: impl FnOnce(
            Rc<dyn ShellLink>,
            Rc<RefCell<ConnCell<S>>>,
            &mut ConnCell<S>,
            ConnectionId,
            StreamRef,
        ) -> T,
    ) -> Poll<Result<T, ConnectionLost>> {
        let mut cell = self.cell.borrow_mut();
        // Ruling 118: the latch answers first, and nothing is drained.
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(lost));
        }
        let Some(core) = cell.core.as_mut() else {
            debug_assert!(
                false,
                "a connection cell held neither a core nor a close reason (§16.3)"
            );
            return Poll::Ready(Err(ConnectionLost::EndpointDropped));
        };
        match core.open(dir) {
            Ok(r) => {
                let handle = build(
                    Rc::clone(&self.shell),
                    Rc::clone(&self.cell),
                    &mut cell,
                    self.id,
                    r,
                );
                Poll::Ready(Ok(handle))
            }
            // Ruling 101: the shell converts exhaustion into a park, which
            // is why `StreamsExhausted` is `pub(crate)` and never reaches an
            // application.
            Err(_) => {
                cell.stream_openers[dir.slot()].park(key, cx);
                Poll::Pending
            }
        }
    }

    /// `accept_bi`/`accept_uni`, on the same terms as
    /// [`poll_open_with`](Self::poll_open_with) — **except for the death
    /// latch, which is where the two verbs part company.**
    ///
    /// # The core is asked first — ruling 128, and this verb only
    ///
    /// Ruling 124's precedence (terminal state → death latch → the core)
    /// governs every other verb on this handle and is unchanged. Here the
    /// order is inverted: `accept(dir)` first, the latch only if it handed
    /// back `None`. A stream the peer opened before the death is *in the
    /// core*, and answering `ConnectionLost` over it loses data that
    /// arrived in full — ruling 47's problem seen from the receiving end.
    /// The core needs no change to allow it: `core::Connection::accept` has
    /// no `lost` guard and never had one.
    ///
    /// **Nothing here may return `Pending` once the latch is set.** No
    /// further stream can ever be opened on a dead connection, so a park
    /// would be permanent — *"parking is never permitted on a dead
    /// connection"* (ruling 128).
    ///
    /// The claim is still one synchronous step with the handle's
    /// construction, so a `poll_accept` that popped can never be dropped
    /// before the handle exists — the orphan the old latch-first order was
    /// wrongly credited with preventing.
    ///
    /// The park is woken by `ConnEvent::StreamOpened`, of which ruling 99
    /// emits **one per stream** — so a wake is not a promise of a stream,
    /// and every waiter re-polls and claims at most one.
    fn poll_accept_with<T>(
        &self,
        cx: &mut Context<'_>,
        key: u64,
        dir: Dir,
        build: impl FnOnce(
            Rc<dyn ShellLink>,
            Rc<RefCell<ConnCell<S>>>,
            &mut ConnCell<S>,
            ConnectionId,
            StreamRef,
        ) -> T,
    ) -> Poll<Result<T, ConnectionLost>> {
        let mut cell = self.cell.borrow_mut();
        // The core first (ruling 128). `None` covers both "nothing
        // unclaimed" and "the driver has released the core", and the latch
        // below answers each of them.
        let claimed = cell.core.as_mut().and_then(|core| core.accept(dir));
        if let Some(r) = claimed {
            let handle = build(
                Rc::clone(&self.shell),
                Rc::clone(&self.cell),
                &mut cell,
                self.id,
                r,
            );
            return Poll::Ready(Ok(handle));
        }
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(lost));
        }
        if cell.core.is_none() {
            debug_assert!(
                false,
                "a connection cell held neither a core nor a close reason (§16.3)"
            );
            return Poll::Ready(Err(ConnectionLost::EndpointDropped));
        }
        cell.stream_acceptors[dir.slot()].park(key, cx);
        Poll::Pending
    }

    /// The peer's static public key — **proven**, not claimed (§6.1).
    ///
    /// A synchronous read of the shared cell (§16.8), never a driver
    /// round-trip. It keeps answering after the connection has died, so a
    /// post-mortem handle can still say who it was talking to.
    pub fn remote_static(&self) -> PublicKeyFor<S> {
        self.remote_static.clone()
    }

    /// The address this connection's datagrams go to — §5.6's anchor.
    ///
    /// **Total**: it never panics and is never `Option`. Before the install
    /// it is the address `connect()` was given; §7.3's roaming is what makes
    /// it change afterwards, and it keeps answering after the connection has
    /// died.
    ///
    /// It moves only where **the peer** moved. Our own interface change or
    /// NAT rebind does not alter where we send, so it is invisible here —
    /// the peer observes that one, on its side, as its own roam.
    pub fn remote_address(&self) -> SocketAddr {
        self.cell.borrow().remote_address
    }

    /// hiss's channel binding for this session (**ruling 89**).
    ///
    /// This is [`hiss::noise::SessionId`], re-exported — not a slither
    /// type. hiss derives it from the handshake hash, both peers of a
    /// session produce the same value, and it is a *public*
    /// channel-binding value meant for out-of-band comparison: logging it,
    /// or a short-authentication-string check.
    ///
    /// # Its `Eq` is not constant-time
    ///
    /// By hiss's deliberate choice. It carries no secret, and it **must not
    /// be used to compare one** — reaching for it as a token or a session
    /// key comparison is the mistake this paragraph exists to prevent.
    pub fn session_id(&self) -> hiss::noise::SessionId {
        self.session_id.clone()
    }

    /// How many per-`StreamRef` waker-map entries this connection holds, in
    /// `(readers, writers)` order — the pin for §16.8's bound.
    ///
    /// Crate-internal and test-only: it is not a protocol fact, it is the
    /// only way to assert from the side that **separates** a build which
    /// removes its map entries from one that does not. An upper-bound
    /// assertion would pass a build that never inserts (working rule 9).
    #[cfg(test)]
    pub(crate) fn stream_waker_entries(&self) -> (usize, usize) {
        self.cell.borrow().stream_waker_entries()
    }

    /// How many futures are parked in slice 6's three sets, in
    /// `(message readers, datagram readers, message senders)` order.
    ///
    /// [`stream_waker_entries`](Self::stream_waker_entries)' reason, for the
    /// sets §9.8 and §11 add. It is **additive rather than an extension of
    /// that accessor**, whose arity tests this slice does not own already
    /// name.
    #[cfg(test)]
    pub(crate) fn sugar_waker_entries(&self) -> (usize, usize, usize) {
        self.cell.borrow().sugar_waker_entries()
    }

    /// Whether a session is installed.
    ///
    /// `true` for the whole of a live connection's life, and `false` again
    /// once §15.2's linger has expired and the state is dropped — which is
    /// a real transition an application can observe on a handle it still
    /// holds, not a placeholder.
    pub fn is_established(&self) -> bool {
        self.cell.borrow().is_established()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The type-erased mirror of the seven minters above — **ruling 228**
// ═══════════════════════════════════════════════════════════════════════
//
// Ruling 122(a) kept the `poll_*` verbs `pub(crate)` on the ground that
// *"slice 8's `compat/` is in-crate and reaches them"*. That is true of the
// verbs and **not true of what they require**: every one takes a `key: u64`
// minted by one of the seven `fn`s above, all of them private to this
// module, so `crate::compat` cannot obtain one — and without a slot an
// adapter has no key that is stable for its life and released on drop,
// which is the whole of the verbs' cancel-safety.
//
// Worse, `WakerSlot<impl FnMut(u64)>` is **unnameable**, so it cannot be an
// adapter struct's field at all, and a §16.11 `Stream` adapter must hold its
// slot for the adapter's whole life rather than for one poll. Boxing the
// release closure is what makes the type nameable.
//
// Additive, in-crate, and no public signature moves, so ruling 204 is
// untouched. The private minters above are left exactly as they were:
// §16.2's `async fn` forms pay no allocation for this.
//
// **Why `S: 'static`, and why it restricts nothing.** The bound is the
// `Box<dyn FnMut(u64)>`'s: the release closure owns an
// `Rc<RefCell<ConnCell<S>>>`, so erasing it behind a `'static` trait object
// needs `S: 'static`. The only way to obtain a `Connection<S>` is through an
// `Endpoint<I>`, and `EndpointBuilder::build` already requires `I: 'static`
// — which gives `I::Suite: 'static`. It is that same bound, and for the same
// kind of reason: the spawned task's, **not** a `Send` requirement (S21).
//
// **Why the `allow` and not a `cfg` matrix.** Ruling 228 fixes the set at
// **seven** — the complete mirror — while which members are live depends on
// the enabled features: `sink` reaches five, `tower` reaches
// `opener_slot_boxed`, and `settled_slot_boxed` has no adapter in slice 8 at
// all (§16.2's `acked()` has no `Stream` or `Sink` face). A `cfg` matrix
// would have to be revised every time an adapter moved between features, and
// would make the set something other than the seven the ruling names.
impl<S: Handshake + 'static> Connection<S> {
    /// [`notification_slot`](Self::notification_slot), type-erased for
    /// `compat::Notifications` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn notification_slot_boxed(&self) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().notification_wakers.key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().notification_wakers.unpark(key)
            }),
        )
    }

    /// [`settled_slot`](Self::settled_slot), type-erased (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn settled_slot_boxed(&self) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().settled_wakers.key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().settled_wakers.unpark(key)
            }),
        )
    }

    /// [`message_reader_slot`](Self::message_reader_slot), type-erased for
    /// `compat::Messages` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn message_reader_slot_boxed(&self) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().message_readers.key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().message_readers.unpark(key)
            }),
        )
    }

    /// [`datagram_reader_slot`](Self::datagram_reader_slot), type-erased for
    /// `compat::Datagrams` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn datagram_reader_slot_boxed(&self) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().datagram_readers.key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().datagram_readers.unpark(key)
            }),
        )
    }

    /// [`message_sender_slot`](Self::message_sender_slot), type-erased for
    /// `compat::MessageSink` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn message_sender_slot_boxed(&self) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().message_senders.key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().message_senders.unpark(key)
            }),
        )
    }

    /// [`opener_slot`](Self::opener_slot), type-erased for `compat::tower`'s
    /// `OpenBi` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn opener_slot_boxed(&self, dir: Dir) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().stream_openers[dir.slot()].key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().stream_openers[dir.slot()].unpark(key)
            }),
        )
    }

    /// [`acceptor_slot`](Self::acceptor_slot), type-erased for
    /// `compat::IncomingBi` and `compat::IncomingUni` (ruling 228).
    #[allow(dead_code)]
    pub(crate) fn acceptor_slot_boxed(&self, dir: Dir) -> WakerSlot<Box<dyn FnMut(u64)>> {
        let key = self.cell.borrow_mut().stream_acceptors[dir.slot()].key();
        WakerSlot::new(
            key,
            Box::new({
                let cell = Rc::clone(&self.cell);
                move |key| cell.borrow_mut().stream_acceptors[dir.slot()].unpark(key)
            }),
        )
    }
}

/// Build both halves of one bidirectional stream under a **single** cell
/// borrow — the `build` argument [`Connection::poll_open_with`] and
/// [`Connection::poll_accept_with`] take for `Dir::Bi`.
fn install_bi<S: Handshake>(
    shell: Rc<dyn ShellLink>,
    cell_rc: Rc<RefCell<ConnCell<S>>>,
    cell: &mut ConnCell<S>,
    conn: ConnectionId,
    r: StreamRef,
) -> BiStream<S> {
    let send = SendStream::install(Rc::clone(&shell), Rc::clone(&cell_rc), cell, conn, r);
    let recv = RecvStream::install(shell, cell_rc, cell, conn, r);
    BiStream::new(send, recv)
}

impl<S: Handshake> std::fmt::Debug for Connection<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written: the curve's `PublicKey` is not required to be
        // `Debug`, and a peer key is not something to print by default.
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("remote_address", &self.remote_address())
            .field("established", &self.is_established())
            .finish_non_exhaustive()
    }
}

impl<S: Handshake> Drop for Connection<S> {
    fn drop(&mut self) {
        let last_for_connection = {
            let mut cell = self.cell.borrow_mut();
            cell.handles -= 1;
            cell.handles == 0
        };

        // `release` decrements the process-wide handle count and, at zero,
        // tells the driver to stop. It must run **before** the decision
        // below, because that decision is exactly "was this also the last
        // handle in the process?" (ruling 88).
        let last_in_process = self.shell.release();

        if last_for_connection && !last_in_process {
            self.close_now(constants::NO_ERROR, b"");
        }
    }
}

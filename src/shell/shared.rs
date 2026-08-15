//! The shell's shared state — §16.3's `Rc<RefCell<_>>` cell, the command
//! channel, and the waker map.
//!
//! Nothing here decides anything. It is the plumbing [`driver`] and the
//! three handle modules both need, kept in one place so a handle file never
//! reaches into the driver for a type the driver does not own.
//!
//! # The one rule
//!
//! **No borrow of [`ShellState`] or [`ConnCell`] is ever held across an
//! `await`.** The driver is a single `!Send` task and every handle runs on
//! the same thread, so a borrow that spans a yield point is the only way
//! this design can panic. Every borrow in the shell is taken inside a
//! non-`async` helper, or inside a `{ }` block that ends before the next
//! `.await`.
//!
//! [`driver`]: super::driver

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Waker};

use tokio::sync::{mpsc, oneshot};

use crate::core::{Connection as CoreConnection, ConnectionId, IntroId, Timestamp};
use crate::error::{AcceptError, AuthError, ConnectError, ConnectionLost, IntroError};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::Handshake;

/// The instant every mutating core call is given.
///
/// **`tokio::time::Instant`, converted — never `std::time::Instant::now()`.**
/// §16.10 requires the whole protocol to be drivable on tokio's paused
/// clock, and a core fed a real `Instant` while the driver sleeps on a
/// virtual one has two clocks. The cores compare instants only against each
/// other, so one source is all that is needed — and this is it, for the
/// driver and for every handle-side mutating verb alike.
pub(crate) fn now() -> std::time::Instant {
    tokio::time::Instant::now().into_std()
}

/// A set of parked wakers, keyed so a repeatedly polled future replaces its
/// own entry instead of accumulating one per poll, and so a **dropped**
/// future removes exactly its own.
///
/// This is §16.8's "quinn pattern" in its smallest useful form: slice 3
/// needs it only for `closed()` and `accept()`, and slice 4's per-stream
/// blocked-readers / blocked-writers maps are the same type keyed by
/// `StreamId`.
#[derive(Debug, Default)]
pub(crate) struct Wakers {
    next: u64,
    parked: BTreeMap<u64, Waker>,
}

impl Wakers {
    /// Mint a key. Monotone and never reused, so a late `unpark` from a
    /// dropped future can never evict a live one's waker.
    pub(crate) fn key(&mut self) -> u64 {
        let key = self.next;
        self.next += 1;
        key
    }

    /// Park (or refresh) `key`'s waker.
    pub(crate) fn park(&mut self, key: u64, cx: &Context<'_>) {
        match self.parked.get_mut(&key) {
            Some(existing) if existing.will_wake(cx.waker()) => {}
            slot => {
                let waker = cx.waker().clone();
                match slot {
                    Some(existing) => *existing = waker,
                    None => {
                        self.parked.insert(key, waker);
                    }
                }
            }
        }
    }

    /// Remove `key`'s waker. Idempotent — a future that completed without
    /// ever parking still calls this from its guard's `Drop`.
    pub(crate) fn unpark(&mut self, key: u64) {
        self.parked.remove(&key);
    }

    /// Take everyone parked here, clearing the map.
    ///
    /// Clearing is not an optimisation: a woken task re-polls and parks
    /// again if it still needs to, and leaving stale wakers behind would
    /// wake tasks whose futures have since been dropped.
    ///
    /// # Why this hands the wakers back instead of waking them
    ///
    /// This used to be a `wake_all(&mut self)` that called `Waker::wake`
    /// on the spot. Its only caller — `Driver::latch` — reaches it through
    /// a live `RefCell::borrow_mut` of the [`ConnCell`], and **a `Waker` is
    /// application-supplied**: `wake()` runs whatever the consumer's
    /// executor does, and an executor that polls a ready task inline
    /// rather than queueing it re-enters `Connection::poll_closed`, which
    /// takes that same borrow. `already mutably borrowed` inside the
    /// driver task, from a consumer doing nothing wrong. Returning the
    /// wakers makes the borrow-then-wake order the only order the type
    /// permits, rather than a rule a caller has to remember.
    ///
    /// tokio's own wakers only push to a run queue, so nothing in this
    /// crate's tests could reach it — which is precisely why it had to be
    /// closed by construction and not by argument.
    #[must_use = "the wakers must be woken after the cell borrow ends (§16.8)"]
    pub(crate) fn take_all(&mut self) -> Vec<Waker> {
        std::mem::take(&mut self.parked).into_values().collect()
    }
}

/// A waker-map slot owned by one future, released on drop.
///
/// The `async fn` forms in §16.2 are `poll_fn` over a `poll_*`, which has
/// nowhere to put cleanup; this guard is where it goes. Dropping a
/// `closed()` future therefore leaves the map exactly as it found it, which
/// is the whole of that verb's cancel-safety.
pub(crate) struct WakerSlot<F: FnMut(u64)> {
    key: u64,
    release: F,
}

impl<F: FnMut(u64)> WakerSlot<F> {
    pub(crate) fn new(key: u64, release: F) -> Self {
        Self { key, release }
    }

    pub(crate) fn key(&self) -> u64 {
        self.key
    }
}

impl<F: FnMut(u64)> Drop for WakerSlot<F> {
    fn drop(&mut self) {
        (self.release)(self.key);
    }
}

/// §5.4's three-valued static state, as the **shell** needs to answer it
/// synchronously (§16.1, ruling 87). NONE is the absence of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticState {
    /// An outbound `connect()` whose `Connecting` is still alive.
    Pending,
    /// A live connection — dialled or accepted.
    Live,
}

/// One entry of ruling 87's synchronous static map.
///
/// The `attempt` is what makes cancel-then-redial safe. On a paused clock
/// the sequence `drop(connecting); ep.connect(same_static)` runs to
/// completion **before the driver is scheduled at all**, so by the time the
/// driver processes the first attempt's cancellation — or its
/// `HANDSHAKE_GIVEUP` — the map already holds the *second* attempt's entry.
/// A driver that removed by key alone would delete the live redial. Every
/// writer therefore checks the stamp it is entitled to remove, and a stale
/// writer finds a newer one and leaves it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StaticSlot {
    pub(crate) attempt: u64,
    pub(crate) state: StaticState,
}

/// One connection's shell-side cell: §16.3's `Rc<RefCell<_>>` data path.
///
/// It holds the connection **core** as well as the shell bookkeeping,
/// because §16.3 puts `close()` on the handle side of the seam: the handle
/// borrows this, calls `core.close(now, …)` — which seals synchronously
/// (§16.7) — marks the cell dirty and wakes the driver, which drains
/// `poll_output()` to `Timeout` and performs the I/O.
pub(crate) struct ConnCell<S: Handshake> {
    /// The sans-io core. `None` once the driver has released the
    /// connection, which happens **after** §16.4's `Retired` (ruling 81).
    pub(crate) core: Option<CoreConnection<S>>,
    /// §5.6's anchor — what `remote_address()` reads.
    ///
    /// In the cell rather than in the handle because slice 7's roaming
    /// (§7.3) moves it; `remote_static` and `session_id` do not move for a
    /// connection's life (§7.8 — one session per connection, no rekey
    /// swap), so those live in the handle and keep answering after §15.2's
    /// linger has dropped the session.
    pub(crate) remote_address: SocketAddr,
    /// Ruling 46's latch. `Some` once §16.4's `Closed(_)` has been drained.
    pub(crate) closed: Option<ConnectionLost>,
    /// Everyone awaiting the latch.
    pub(crate) closed_wakers: Wakers,
    /// §16.2's notification slots — declared now, filled in slice 7.
    ///
    /// Empty and unread in this slice, and deliberately present: `closed()`
    /// must not be built as the shell's only event path, or slice 7's
    /// `notified()` is a rewrite rather than an addition (§16.2's
    /// "retention is one slot per kind").
    #[allow(dead_code, reason = "slice 7 fills it; the place is the point")]
    pub(crate) notifications: NotificationSlots,
    /// Set by any handle-side mutating borrow; cleared by the driver when
    /// it drains.
    pub(crate) dirty: bool,
    /// How many `Connection` handles point at this cell. The last one out
    /// performs §16.2's `close(NO_ERROR, "")` — unless it is also the last
    /// handle in the process (ruling 88).
    pub(crate) handles: usize,
}

impl<S: Handshake> ConnCell<S> {
    pub(crate) fn is_established(&self) -> bool {
        self.core
            .as_ref()
            .is_some_and(CoreConnection::is_established)
    }
}

/// What a [`Connection`](super::connection::Connection) handle needs from
/// the shell, with the identity type erased.
///
/// §16.2 writes a bare `Connection`, and §16.4's core is parameterised by
/// the **suite** (`core::Connection<C: Handshake>`) — not by the identity,
/// which is a statement about where *our own* private key lives and has
/// nothing to do with the session. So the shell's handle is
/// `Connection<S: Handshake>` too, and the two things it still needs from
/// the identity-parameterised [`Shell`] travel through this trait instead
/// of through a type parameter.
pub(crate) trait ShellLink {
    /// Release one handle; `true` if it was the last in the process
    /// (§16.3, ruling 88).
    fn release(&self) -> bool;
    /// §16.3: every mutating borrow ends by marking the connection dirty
    /// and waking the driver.
    fn mark_dirty(&self, id: ConnectionId);
    /// Count one more handle.
    fn acquire(&self);
}

impl<I: Identity> ShellLink for Shell<I> {
    fn release(&self) -> bool {
        Shell::release(self)
    }

    fn mark_dirty(&self, id: ConnectionId) {
        self.send(Command::Dirty(id));
    }

    fn acquire(&self) {
        Shell::acquire(self);
    }
}

/// §16.2's per-kind notification retention. Slice 7 fills it.
///
/// A unit struct rather than an empty enum: it is a **place**, not a claim
/// about which kinds exist.
#[derive(Debug, Default)]
pub(crate) struct NotificationSlots;

/// A `Connecting`'s resolution slot (§16.2).
pub(crate) enum PendingOutcome<I: Identity> {
    /// §5.5's train is running.
    Waiting,
    /// Established: the `Connection` handle the driver built, waiting to be
    /// handed over.
    Ready(super::connection::Connection<I::Suite>),
    /// §5.5's give-up, or a local failure (ruling 72).
    Failed(ConnectError),
}

/// The slot a `Connecting` and the driver share.
///
/// It is an `Rc<RefCell<_>>` of its own rather than an entry in
/// [`ShellState`] because the handle creates it **before** the driver has
/// minted a [`ConnectionId`] — on a paused clock the driver may not run
/// between `connect()` and the drop that cancels it, so there is no instant
/// at which an id-keyed map would be safe to consult.
pub(crate) struct PendingSlot<I: Identity> {
    pub(crate) outcome: PendingOutcome<I>,
    pub(crate) waker: Option<Waker>,
    /// Written by the driver once `core::Endpoint::connect` has minted it.
    pub(crate) id: Option<ConnectionId>,
}

impl<I: Identity> PendingSlot<I> {
    pub(crate) fn new() -> Self {
        Self {
            outcome: PendingOutcome::Waiting,
            waker: None,
            id: None,
        }
    }

    /// Install `outcome`, handing back what the caller must dispose of
    /// **outside** the borrow: the previous outcome and the parked waker.
    ///
    /// Use [`resolve_slot`] rather than calling this directly; it is the
    /// one place that gets the ordering right.
    #[must_use = "the previous outcome and the waker must be disposed of outside the borrow"]
    fn resolve(&mut self, outcome: PendingOutcome<I>) -> (PendingOutcome<I>, Option<Waker>) {
        let previous = std::mem::replace(&mut self.outcome, outcome);
        (previous, self.waker.take())
    }
}

/// Resolve a `Connecting`'s slot, then wake it — **in that order, with the
/// borrow released in between**.
///
/// Two things here run consumer code, and neither may run under
/// `slot.borrow_mut()`:
///
/// * `Waker::wake` is the consumer's executor. An executor that polls a
///   ready task inline re-enters `Connecting::poll`, which borrows this
///   same cell. See [`Wakers::take_all`] for the same hazard on the
///   connection side.
/// * **dropping the previous outcome.** A `PendingOutcome::Ready` owns a
///   [`Connection`](super::connection::Connection), whose `Drop` takes
///   `ConnCell`'s borrow *and* may stop the driver (ruling 88). Today
///   every driver-side path takes `record.slot` out before resolving, so
///   the previous outcome is always `Waiting` and the drop is free — a
///   property of the current call sites, not of this function, which is
///   the sort of thing that quietly stops being true.
pub(crate) fn resolve_slot<I: Identity>(
    slot: &Rc<RefCell<PendingSlot<I>>>,
    outcome: PendingOutcome<I>,
) {
    let (previous, waker) = slot.borrow_mut().resolve(outcome);
    drop(previous);
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// The shell state every handle reads synchronously (§16.8) and the driver
/// writes.
pub(crate) struct ShellState<I: Identity> {
    /// §16.4's endpoint core.
    ///
    /// It lives in the shared cell — not privately in the driver — so that
    /// §6.2's `Intro::source()` and `Intro::sender_index()` are **live
    /// reads** of the parked entry. Ruling 71 makes that load-bearing: §5.5
    /// mints a new random index on every retransmit, and §6.3's dedup
    /// replaces a same-source entry **without re-surfacing it**, so a value
    /// cached when the introduction was handed over is stale the moment the
    /// peer retransmits. Nothing else reads it from a handle: every
    /// **mutating** endpoint verb is a driver round-trip, because §6.2
    /// requires the DH to land on the driver task (§16.3, ruling 53).
    pub(crate) endpoint: crate::core::Endpoint<I>,
    /// Ruling 87's cell: §16.1's NONE / PENDING / LIVE, answerable with no
    /// driver round-trip, keyed by the static's canonical §2.4 octets.
    ///
    /// This is a **second record** of state the endpoint core also keeps,
    /// and the duplication is forced rather than chosen. Ruling 87 says
    /// `connect()` "only mints the pending" and performs no DH, so the
    /// natural implementation would write the core's own static map
    /// synchronously. Slice 3a's frozen `core::Endpoint::connect()` mints
    /// the pending **and** builds msg1 in one call — 2 DH — so the shell
    /// cannot reach the core's map without paying on the caller's task.
    /// See `IMPLEMENTATION-3b.md` §5 conflict C-B1.
    ///
    /// Every transition is enumerated in [`driver`](super::driver)'s module
    /// docs, and the driver `debug_assert!`s if the core ever disagrees.
    pub(crate) statics: BTreeMap<Vec<u8>, StaticSlot>,
    /// How many handles — `Endpoint`, `Connecting`, `Connection` — exist.
    ///
    /// Staged objects are deliberately **not** counted: §16.3 is explicit
    /// that "a staged object's verb is a round-trip to a driver it does not
    /// keep alive", which is why `IntroError`/`AuthError`/`AcceptError` all
    /// carry `EndpointDropped` and `ConnectError` does not (ruling 62). Nor
    /// is a `closed()` future: it "owns nothing and merely observes".
    pub(crate) handles: usize,
    /// Set once the driver has returned. Every verb answers from it rather
    /// than hanging.
    pub(crate) driver_stopped: bool,
    /// Mints [`StaticSlot::attempt`]. Shell-side and monotone; unrelated to
    /// the core's `ConnectionId`, which does not exist yet at the instant
    /// `connect()` has to stamp its entry.
    pub(crate) next_attempt: u64,
}

impl<I: Identity> ShellState<I> {
    /// Claim `key` for a new outbound attempt, or report the state that
    /// already holds it. §16.1's NONE/PENDING/LIVE test, at the instant of
    /// the call.
    pub(crate) fn claim_static(&mut self, key: Vec<u8>) -> Result<u64, ConnectError> {
        if self.statics.contains_key(&key) {
            return Err(ConnectError::AlreadyConnected);
        }
        let attempt = self.next_attempt;
        self.next_attempt += 1;
        self.statics.insert(
            key,
            StaticSlot {
                attempt,
                state: StaticState::Pending,
            },
        );
        Ok(attempt)
    }

    /// Release `key`, but only if it still holds `attempt`.
    ///
    /// Returns whether anything was removed. See [`StaticSlot`] for why the
    /// stamp check is not optional.
    pub(crate) fn release_static(&mut self, key: &[u8], attempt: u64) -> bool {
        match self.statics.get(key) {
            Some(slot) if slot.attempt == attempt => {
                self.statics.remove(key);
                true
            }
            _ => false,
        }
    }
}

/// The handle side of the seam: the shared cell plus the command channel.
///
/// Cloned into every handle. It is `Rc`-based and therefore `!Send`, which
/// is the architecture invariant expressed as a type rather than as a
/// comment.
pub(crate) struct Shell<I: Identity> {
    pub(crate) state: Rc<RefCell<ShellState<I>>>,
    pub(crate) commands: mpsc::UnboundedSender<Command<I>>,
}

impl<I: Identity> Clone for Shell<I> {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
            commands: self.commands.clone(),
        }
    }
}

impl<I: Identity> Shell<I> {
    /// Build the pair: the handle side and the driver's receiver.
    pub(crate) fn new(
        endpoint: crate::core::Endpoint<I>,
    ) -> (Self, mpsc::UnboundedReceiver<Command<I>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                state: Rc::new(RefCell::new(ShellState {
                    endpoint,
                    statics: BTreeMap::new(),
                    handles: 0,
                    driver_stopped: false,
                    next_attempt: 0,
                })),
                commands: tx,
            },
            rx,
        )
    }

    /// Push a command at the driver.
    ///
    /// Never blocks and never fails observably: a closed channel means the
    /// driver has already stopped, and every verb has a defined answer for
    /// that which does not depend on the command arriving.
    pub(crate) fn send(&self, command: Command<I>) {
        let _ = self.commands.send(command);
    }

    /// Count one more handle (§16.3's driver lifetime).
    pub(crate) fn acquire(&self) {
        self.state.borrow_mut().handles += 1;
    }

    /// Release one handle, and tell the driver when it was the last.
    ///
    /// Returns whether the process has now let go of everything — the test
    /// ruling 88 turns on.
    pub(crate) fn release(&self) -> bool {
        let last = {
            let mut state = self.state.borrow_mut();
            // Saturating, with the assertion beside it: an over-release
            // would wrap in a release build and give the driver a handle
            // count it can never reach zero from — a hang rather than a
            // panic, and the worst shape a defect can take here. Every
            // handle acquires exactly once in its constructor and releases
            // exactly once in its `Drop`.
            debug_assert!(state.handles > 0, "a shell handle was released twice");
            state.handles = state.handles.saturating_sub(1);
            state.handles == 0
        };
        if last {
            self.send(Command::HandlesGone);
        }
        last
    }

    pub(crate) fn driver_stopped(&self) -> bool {
        self.state.borrow().driver_stopped
    }
}

/// A verb, on its way to the driver task.
///
/// Every variant is either a §6.2 round-trip (a `oneshot` reply, because
/// §16.3 requires the DH to land on the driver) or a one-way signal.
pub(crate) enum Command<I: Identity> {
    /// §16.2's `connect()`. **One-way** — ruling 87: the verb is not
    /// `async`, so there is no reply to await, and the NONE/PENDING/LIVE
    /// answer was already read out of [`ShellState::statics`].
    Connect {
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
        static_key: Vec<u8>,
        attempt: u64,
        slot: Rc<RefCell<PendingSlot<I>>>,
    },
    /// Ruling 50: a `Connecting` was dropped. Carries the slot rather than
    /// a [`ConnectionId`] because on a paused clock the driver may not have
    /// run since the `Connect` that would have minted one — so there is no
    /// id yet to name, and the slot is the only thing both sides can hold.
    Cancel(Rc<RefCell<PendingSlot<I>>>),
    /// §16.2's `accept()`. The reply carries a fully built [`Intro`], so a
    /// cancelled `accept()` drops one — which is §6.2's silent reject, the
    /// documented meaning of dropping a staged object, rather than a leak.
    ///
    /// [`Intro`]: super::staged::Intro
    Accept(oneshot::Sender<super::staged::Intro<I>>),
    /// §6.2's stage 1.
    ReadIdentity(IntroId, oneshot::Sender<Result<PublicKeyOf<I>, IntroError>>),
    /// §6.2's stage 2.
    Authenticate(
        IntroId,
        oneshot::Sender<Result<(PublicKeyOf<I>, Timestamp), AuthError>>,
    ),
    /// §6.2's stage 3. Carries the proven static from the `Proven` handle:
    /// §16.4's `accept()` hands back `(ConnectionId, Connection)` and the
    /// endpoint core exposes no per-connection static accessor, so the
    /// value travels with the verb that proved it.
    AcceptChain(
        IntroId,
        PublicKeyOf<I>,
        oneshot::Sender<Result<super::connection::Connection<I::Suite>, AcceptError>>,
    ),
    /// Dropping a staged object at any stage (§6.2, ruling 48).
    Reject(IntroId),
    /// §16.3: a handle mutated a connection cell. The driver drains it to
    /// `Timeout` and performs the I/O.
    Dirty(ConnectionId),
    /// The last handle went away (§16.3, §15.4's endpoint-dropped row).
    HandlesGone,
}

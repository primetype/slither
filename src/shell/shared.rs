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
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Waker};

use tokio::sync::{mpsc, oneshot};

use crate::core::{Connection as CoreConnection, ConnectionId, IntroId, StreamRef, Timestamp};
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
/// [`StreamRef`] — **not** `StreamId`, which a tie-break can renumber at
/// install (§16.8, ruling 117).
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

    /// Whether anyone is parked here.
    ///
    /// Read by [`release_waker_slot`] to drop a per-`StreamRef` entry once
    /// its last waiter is gone: the map is keyed by a value the application
    /// controls, so an entry that outlives its handle is unbounded growth.
    pub(crate) fn is_empty(&self) -> bool {
        self.parked.is_empty()
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
    /// How many handles point at this cell — `Connection`, and since
    /// **ruling 115** `SendStream`, `RecvStream` and `BiStream` too. The
    /// last one out performs §16.2's `close(NO_ERROR, "")` — unless it is
    /// also the last handle in the process (ruling 88).
    pub(crate) handles: usize,
    /// §16.8's blocked-readers map: one entry per live
    /// [`RecvStream`](super::RecvStream), created by that handle and removed
    /// by its `Drop`.
    ///
    /// **Keyed by [`StreamRef`], never `StreamId`** (ruling 117): a
    /// `StreamId` is fixed by an opener parity that §6.7's tie-break can
    /// invert, so a map keyed by it would need rekeying at every install and
    /// a park that straddled one would look up a key that no longer exists.
    pub(crate) blocked_readers: BTreeMap<StreamRef, Wakers>,
    /// §16.8's blocked-writers map, on the same terms.
    pub(crate) blocked_writers: BTreeMap<StreamRef, Wakers>,
    /// §16.2's `SendStream::acked()` waiters — **ruling 47**, one entry per
    /// live [`SendStream`](super::SendStream), on
    /// [`blocked_writers`](Self::blocked_writers)' terms exactly: created by
    /// that handle's constructor, removed by its `Drop`, keyed by
    /// [`StreamRef`] and never by `StreamId` (ruling 117).
    ///
    /// Woken by `ConnEvent::StreamFinished` and `ConnEvent::StreamReset`,
    /// and by the death latch.
    pub(crate) blocked_ackers: BTreeMap<StreamRef, Wakers>,
    /// Which send halves have reached §9.7's `DataRecvd` — the latch
    /// `SendStream::acked()` answers from.
    ///
    /// # Why a latch and not the event alone
    ///
    /// `ConnEvent::StreamFinished` is a **wakeup**, and an `acked()` that
    /// only ever resolved on the wake would hang on the ordinary sequence:
    /// the peer's ACK arrives, the driver publishes the event with nobody
    /// parked, and the application calls `acked()` afterwards. That is not
    /// an unlikely interleaving — it is `write; finish; acked()` whenever
    /// the ACK beats the application to the call, which on a paused clock
    /// is *most* of the time. Ruling 135 needs it too: the ACK and the
    /// peer's CLOSE can arrive in one driver pass, and the fact that the
    /// transfer completed has to outlive the latch that says the
    /// connection did not.
    ///
    /// **Bounded by the number of live `SendStream` handles**, not by the
    /// number of streams the connection has ever finished: the driver
    /// records a stream here only while
    /// [`blocked_ackers`](Self::blocked_ackers) holds that stream's slot —
    /// i.e. only while a handle that could ask exists — and the handle's
    /// `Drop` removes both. Without that gate this map would grow once per
    /// finished stream for the connection's life, which for §9.8's message
    /// streams is once per message.
    pub(crate) finished_senders: BTreeSet<StreamRef>,
    /// §16.2's `Connection::acked()` waiters — rulings 47 and 54.
    ///
    /// `closed_wakers`' sibling and not a per-`StreamRef` map: the verb is
    /// a **connection-level snapshot**, so every waiter re-polls on any
    /// change to any stream's settledness.
    pub(crate) settled_wakers: Wakers,
    /// `open_bi`/`open_uni` futures parked on §10.4's cumulative limit,
    /// indexed by `Dir::slot()`.
    ///
    /// An array and not a map: `Dir` has exactly two values, so this is
    /// bounded by construction rather than by anyone's discipline.
    pub(crate) stream_openers: [Wakers; 2],
    /// `accept_bi`/`accept_uni` futures parked on an empty unclaimed queue.
    /// Same shape, same reason.
    pub(crate) stream_acceptors: [Wakers; 2],
}

impl<S: Handshake> ConnCell<S> {
    /// A fresh cell around an installed core.
    ///
    /// `dirty` starts `true`: the core has been mutated by whatever built
    /// it, and §16.4's drain contract is discharged by the driver's next
    /// pass.
    ///
    /// A constructor rather than two struct literals in [`driver`], because
    /// §16.8's waker maps grow a field per slice and a literal per call site
    /// is a place to forget one.
    ///
    /// [`driver`]: super::driver
    pub(crate) fn new(core: CoreConnection<S>, remote_address: SocketAddr) -> Self {
        Self {
            core: Some(core),
            remote_address,
            closed: None,
            closed_wakers: Wakers::default(),
            notifications: NotificationSlots,
            dirty: true,
            handles: 0,
            blocked_readers: BTreeMap::new(),
            blocked_writers: BTreeMap::new(),
            blocked_ackers: BTreeMap::new(),
            finished_senders: BTreeSet::new(),
            settled_wakers: Wakers::default(),
            stream_openers: Default::default(),
            stream_acceptors: Default::default(),
        }
    }

    /// Record §9.7's `DataRecvd` for `r`, and hand back the waiters to be
    /// woken **outside** the borrow (finding F10).
    ///
    /// The latch is written only when [`blocked_ackers`] holds `r`'s
    /// slot — see [`finished_senders`] for why that gate is what bounds
    /// the map.
    ///
    /// [`blocked_ackers`]: Self::blocked_ackers
    /// [`finished_senders`]: Self::finished_senders
    #[must_use = "the wakers must be woken after the cell borrow ends (§16.8)"]
    pub(crate) fn note_send_finished(&mut self, r: StreamRef) -> Vec<Waker> {
        let Some(wakers) = self.blocked_ackers.get_mut(&r) else {
            return Vec::new();
        };
        let woken = wakers.take_all();
        self.finished_senders.insert(r);
        woken
    }

    pub(crate) fn is_established(&self) -> bool {
        self.core
            .as_ref()
            .is_some_and(CoreConnection::is_established)
    }

    /// Sweep every stream waker parked in this cell, handing them back to be
    /// woken **after** the borrow ends (§16.8, finding F10).
    ///
    /// `closed_wakers` is deliberately **not** in here: its caller
    /// ([`Driver::latch`](super::driver::Driver)) takes it under the same
    /// borrow, and folding it in would hide which sweep set the latch.
    #[must_use = "the wakers must be woken after the cell borrow ends (§16.8)"]
    pub(crate) fn take_all_stream_wakers(&mut self) -> Vec<Waker> {
        let mut woken = Vec::new();
        for wakers in self.blocked_readers.values_mut() {
            woken.extend(wakers.take_all());
        }
        for wakers in self.blocked_writers.values_mut() {
            woken.extend(wakers.take_all());
        }
        // Ruling 128's *"parking is never permitted on a dead
        // connection"* reaches both `acked()` verbs: neither can resolve
        // from anything but the latch once the connection is gone, so
        // both must be woken to see it.
        for wakers in self.blocked_ackers.values_mut() {
            woken.extend(wakers.take_all());
        }
        woken.extend(self.settled_wakers.take_all());
        for wakers in &mut self.stream_openers {
            woken.extend(wakers.take_all());
        }
        for wakers in &mut self.stream_acceptors {
            woken.extend(wakers.take_all());
        }
        woken
    }

    /// How many per-`StreamRef` waker-map entries exist, in
    /// `(readers, writers)` order.
    ///
    /// The pin for §16.8's bound (working rule 9): these two maps are keyed
    /// by a value the **application** controls, so "bounded" is not
    /// assertable from the outside and a build that never inserts satisfies
    /// any upper bound for free. A test parks, asserts non-zero, drops the
    /// handles and asserts zero — which separates the two.
    #[cfg(test)]
    pub(crate) fn stream_waker_entries(&self) -> (usize, usize) {
        (self.blocked_readers.len(), self.blocked_writers.len())
    }
}

/// Remove one handle's waker from a per-`StreamRef` map, and remove the
/// entry itself once it is empty.
///
/// §16.8's map is keyed by a value the application controls, so what bounds
/// it has to be stated. Three things do, and the argument needs all three:
///
/// 1. **per-waker** — `Wakers::unpark` on the handle's own key, so a
///    cancelled poll leaves nothing behind;
/// 2. **per-entry** — this function, called from the handle's `Drop`.
///    **This is the bound that matters**, and it is structural rather than
///    disciplinary: neither [`SendStream`](super::SendStream) nor
///    [`RecvStream`](super::RecvStream) is `Clone`, so there is exactly one
///    owner of each entry and its lifetime is exactly that handle's. The
///    maps are therefore bounded by the number of **live stream handles** —
///    §10.4's cumulative limit — not by the number of streams the
///    connection has ever opened;
/// 3. **per-wake** — `Wakers::take_all` clears the inner set, so a
///    woken-and-never-re-parked stream leaves an empty `Wakers` rather than
///    a stale one.
///
/// *What state does (2) assume (working rule 12)?* That the handles are not
/// `Clone` and that `BiStream::split` yields one of each, never two. If
/// either is ever relaxed, (2) fails and these maps need reference counting.
pub(crate) fn release_waker_slot(map: &mut BTreeMap<StreamRef, Wakers>, r: StreamRef, key: u64) {
    let std::collections::btree_map::Entry::Occupied(mut entry) = map.entry(r) else {
        return;
    };
    entry.get_mut().unpark(key);
    if entry.get().is_empty() {
        entry.remove();
    }
}

/// Re-poll every parked [`Connection::acked`] on this connection.
///
/// [`Connection::acked`]: super::Connection::acked
///
/// # Why this needs a wake source `CONTRACT-5b.md` does not name
///
/// The contract wakes `settled_wakers` on `ConnEvent::StreamFinished`,
/// `ConnEvent::StreamReset` and the death latch. Those three do not cover
/// the case §16.2 states in terms — *"`acked()` terminates on a live
/// connection even while a bulk stream is still being written"*. A stream
/// with no FIN never reaches §9.7's `DataRecvd`, so it emits **no**
/// `StreamFinished` however much of it is acknowledged, and the snapshot
/// the verb took can become settled with no event of any kind behind it.
///
/// Three things settle a snapshot entry, and this is the enumeration a
/// later change has to re-check rather than a claim about one call site:
///
/// 1. bytes acknowledged (`SendHalf::on_ack_range`) — reachable only from
///    an **inbound datagram**, which is one of the two callers;
/// 2. the send half freed at `DataRecvd`/`ResetRecvd`, or its whole entry
///    removed — also inbound, and covered by the `StreamFinished` arm too;
/// 3. a **local** reset, which `SendHalf::settled_to` counts as settled
///    (§16.2: *"acknowledged **or abandoned by a reset**"*) —
///    [`SendStream::reset`](super::SendStream::reset) and that handle's
///    `Drop`, the other caller.
///
/// Waking is always safe: a woken `poll_acked` re-reads the core and parks
/// again if nothing changed. Missing one is not — it is a verb that never
/// resolves, which is the failure `acked()` exists to prevent.
pub(crate) fn wake_settled<S: Handshake>(cell: &RefCell<ConnCell<S>>) {
    // Taken inside the borrow, woken outside it (§16.8, finding F10).
    let woken = cell.borrow_mut().settled_wakers.take_all();
    for waker in woken {
        waker.wake();
    }
}

/// Seal `close(code, reason)` on a connection cell and mark it dirty
/// (§15.2, §16.7).
///
/// Shared by [`Connection`](super::Connection)'s `close()` and its
/// last-handle `Drop` **and** by the stream handles' `Drop`, which since
/// ruling 115 can be the last handle to a connection. One implementation,
/// because two would be two places to get §16.7's seal-then-signal order
/// wrong.
pub(crate) fn close_now<S: Handshake>(
    shell: &Rc<dyn ShellLink>,
    cell: &RefCell<ConnCell<S>>,
    id: ConnectionId,
    code: u64,
    reason: &[u8],
) {
    let mutated = {
        let mut cell = cell.borrow_mut();
        match cell.core.as_mut() {
            Some(core) => {
                core.close(now(), code, reason);
                cell.dirty = true;
                true
            }
            // The driver has already released the core — §15.2's linger
            // expired, or the connection was never installed. There is
            // nothing left to seal with.
            None => false,
        }
    };
    if mutated {
        shell.mark_dirty(id);
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
/// [`ShellState`] because §16.8's resolution has to be readable by a future
/// that may be polled at any time, and the driver writes it from the other
/// side of the seam. It no longer carries the [`ConnectionId`]: ruling 90
/// mints that synchronously in `connect()`, so the `Connecting` owns it
/// outright.
pub(crate) struct PendingSlot<I: Identity> {
    pub(crate) outcome: PendingOutcome<I>,
    pub(crate) waker: Option<Waker>,
}

impl<I: Identity> PendingSlot<I> {
    pub(crate) fn new() -> Self {
        Self {
            outcome: PendingOutcome::Waiting,
            waker: None,
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
}

/// A guard against a core that re-emits for ever, matching the driver's own.
/// See [`ShellState::drain_endpoint`].
const HANDLE_DRAIN_BOUND: usize = 100_000;

impl<I: Identity> ShellState<I> {
    /// §16.4's drain contract, discharged on the **handle** side.
    ///
    /// "Every mutating call is followed by draining `poll_output()` to the
    /// terminal `Timeout`" (§16.4), and ruling 90 puts two mutating endpoint
    /// calls on a handle:
    ///
    /// * [`Endpoint::connect`](super::Endpoint::connect) →
    ///   `core::Endpoint::mint_pending`, and
    /// * `Connecting::drop` → `core::Endpoint::handle_connection_event(..,
    ///   Retired)`, ruling 50's cancel.
    ///
    /// **Both are 0 DH and both emit nothing**, so this loop terminates on
    /// its first pop and the deadline it returns is the one the driver would
    /// read. The `debug_assert` is the pin for "emit nothing", and it is not
    /// decoration: were one of them ever to emit, popping here would
    /// *destroy* the output — the silent-CLOSE-loss shape of finding 4 in
    /// `.slices/03-skeleton/FIXES-3b.md`. The assert turns that into a named
    /// failure in every debug build, which is every `cargo test`.
    ///
    /// The driver is woken by the command each of those verbs sends
    /// afterwards, so it re-serves and re-reads the deadline regardless;
    /// nothing here is load-bearing for liveness.
    pub(crate) fn drain_endpoint(&mut self) {
        for _ in 0..HANDLE_DRAIN_BOUND {
            match self.endpoint.poll_output() {
                crate::core::EndpointOutput::Timeout(_) => return,
                other => {
                    debug_assert!(
                        false,
                        "a handle-side endpoint verb queued an output (§16.4, ruling 90): \
                         {}",
                        match other {
                            crate::core::EndpointOutput::Transmit(_) => "Transmit",
                            crate::core::EndpointOutput::IntroReady(..) => "IntroReady",
                            crate::core::EndpointOutput::ToConnection(..) => "ToConnection",
                            crate::core::EndpointOutput::HandshakeFailed(..) => "HandshakeFailed",
                            crate::core::EndpointOutput::Timeout(_) => unreachable!(),
                        }
                    );
                }
            }
        }
        panic!(
            "core::Endpoint::poll_output did not reach Timeout in {HANDLE_DRAIN_BOUND} outputs (§16.4)"
        );
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
                    handles: 0,
                    driver_stopped: false,
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
    /// §16.2's `connect()`. **One-way** — rulings 87 and 90: the verb is not
    /// `async`, so there is no reply to await; `mint_pending` already
    /// answered §16.1's NONE/PENDING/LIVE test out of the endpoint core's
    /// own static map, and already minted `id` and `core`. What is left for
    /// the driver is §6.1's two initiator DH — `start_attempt` — plus the
    /// shell-side bookkeeping that goes with them.
    ///
    /// The core is **boxed**. A `core::Connection` is ~680 bytes and every
    /// other variant here is a handful, so carrying it inline would make the
    /// channel's element size — paid by `Dirty`, `Cancel` and `HandlesGone`
    /// alike — the size of the largest verb. One allocation per dial buys it
    /// back.
    Connect {
        id: ConnectionId,
        core: Box<CoreConnection<I::Suite>>,
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
        slot: Rc<RefCell<PendingSlot<I>>>,
    },
    /// Ruling 50: a `Connecting` was dropped.
    ///
    /// Carries the [`ConnectionId`] because ruling 90's `mint_pending`
    /// minted it **synchronously**, inside the `connect()` that returned the
    /// `Connecting` — so by the time anything can drop one, the id exists.
    /// Before ruling 90 it could not: the id was minted on the driver, and
    /// on a paused clock the driver may not have run since, so this variant
    /// had to carry the slot instead.
    ///
    /// The core-side retirement is **not** this command's job — it already
    /// happened, in `Connecting::drop` itself. This releases the driver's
    /// own record.
    Cancel(ConnectionId),
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

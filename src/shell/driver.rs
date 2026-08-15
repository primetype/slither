//! §16.3's driver — the single `!Send` actor.
//!
//! One task owns the [`Wire`], both sans-io cores, and every connection's
//! shell-side cell. It is spawned with `tokio::task::spawn_local`, so a
//! `LocalSet` is required, and **no `Send` bound may be added anywhere on
//! this path**: a DH provider is not required to be `Send` (an iOS Secure
//! Enclave key is the story that forbids it, S21), and neither is a `Wire`.
//!
//! # The loop
//!
//! Each iteration is: **serve** — drain both cores to their terminal
//! `Timeout` and perform the I/O they asked for — then **wait** in a
//! `select!` over three arms: a command from a handle, an inbound datagram,
//! and the earliest deadline either core announced. There is no fourth arm.
//! Every handle-side wake, including a mutating borrow of a connection cell
//! and the last-handle drop, arrives as a [`Command`], so there is exactly
//! one path by which this task is woken and exactly one place to get it
//! wrong.
//!
//! The command arm is `biased` first, and that is load-bearing rather than
//! a micro-optimisation: ruling 50 requires a `Connecting`'s cancellation to
//! be ordered ahead of any endpoint verb the application issues after the
//! drop returns. Both travel on the one channel, so FIFO gives the ordering
//! — provided no datagram or timer can interleave a core mutation between
//! them.
//!
//! # The borrow rule
//!
//! **No `RefCell` borrow is ever held across an `await`.** Every core call
//! below happens inside a non-`async` helper, and those helpers return
//! owned values — the datagrams to send — which the `async` code then acts
//! on with all borrows released. This is the one invariant that makes an
//! `Rc<RefCell<_>>` seam sound in a task that yields, and it is the first
//! thing to check when reviewing this file.
//!
//! # `Retired` ordering (rulings 81 / 84, §16.4)
//!
//! `ToEndpoint::Retired` is a **MUST**: the shell delivers it to
//! `core::Endpoint::handle_connection_event` **before** releasing the
//! connection's shell-side bookkeeping, or the index route and the §17.1
//! guard pin leak for the endpoint's life. [`Driver::serve_connection`]
//! does that literally — `Retired` is handled inside the drain pass that
//! produced it — and [`Driver::release_dead`], the shell-side release, runs
//! only after every drain in the pass has finished. There is no ordering in
//! which the route outlives the bookkeeping that frees it.
//!
//! Which deaths carry a `Retired` is the connection core's to decide and it
//! already does (rulings 81/84): `Closed(_)` at the death; `Retired` at
//! `CloseLinger` expiry on the closing and draining paths; `Retired` in the
//! same drain on the no-linger paths; and **no `Retired` at all** when no
//! session was ever installed — which is why [`Driver::release_dead`] keys
//! on the *core's* state rather than on having seen a `Retired`, and why
//! [`Driver::command_cancel`] synthesises one for a pending that has no
//! session index to name (see there for why `0` is the safe value).
//!
//! # The static map's transitions
//!
//! `ShellState::statics` is ruling 87's synchronous NONE/PENDING/LIVE cell.
//! Its complete set of writers:
//!
//! | Transition | Written by | When |
//! |---|---|---|
//! | NONE → PENDING | the handle | `Endpoint::connect`, synchronously |
//! | PENDING → NONE | the handle | `Connecting::drop`, synchronously (ruling 50) |
//! | PENDING → NONE | the driver | `EndpointOutput::HandshakeFailed` — §5.5's give-up |
//! | PENDING → LIVE | the driver | `ConnEvent::Established` |
//! | NONE → LIVE | the driver | a staged `accept()` that returned a connection |
//! | LIVE → NONE | the driver | the connection's state is released |
//!
//! Every driver-side removal is stamp-checked, because a cancel-and-redial
//! installs a **newer** attempt under the same key before the driver has
//! processed the older one's death.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::rc::Rc;

use tokio::sync::{mpsc, oneshot};

use crate::constants;
use crate::core::{
    ConnEvent, ConnOutput, Connection as CoreConnection, ConnectionId, Disposition, EndpointOutput,
    IntroId, ToEndpoint, Transmit,
};
use crate::error::{AcceptError, ConnectError, ConnectionLost};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::Handshake;

use super::connection::Connection;
use super::shared::{
    Command, ConnCell, NotificationSlots, PendingOutcome, PendingSlot, Shell, ShellLink,
    StaticState, Wakers, now,
};
use super::staged::Intro;
use super::wire::Wire;

/// A guard against a core that re-emits for ever. §16.4's drain terminates
/// in `Timeout`; a core that does not is a bug, and this turns it into a
/// named panic rather than a CI hang.
const DRAIN_BOUND: usize = 100_000;

/// The driver's record of one connection.
struct ConnRecord<I: Identity> {
    /// Shared with every [`Connection`] handle for this connection.
    cell: Rc<RefCell<ConnCell<I::Suite>>>,
    /// The peer's canonical §2.4 static octets — the key into the shell's
    /// static map — and the key itself, kept for the accessors.
    static_key: Vec<u8>,
    remote_static: PublicKeyOf<I>,
    /// The stamp this connection's static entry carries.
    attempt: u64,
    /// The `Connecting` awaiting establishment, while one exists.
    slot: Option<Rc<RefCell<PendingSlot<I>>>>,
}

/// An introduction §6.3's queue surfaced and no `accept()` has taken.
#[derive(Debug, Clone, Copy)]
struct ReadyIntro {
    id: IntroId,
    source: SocketAddr,
}

/// What woke the loop.
enum Event<I: Identity> {
    Command(Option<Command<I>>),
    Received(std::io::Result<(usize, SocketAddr)>),
    Timeout,
}

/// §16.3's actor.
pub(crate) struct Driver<I: Identity, W: Wire> {
    wire: W,
    shell: Shell<I>,
    commands: mpsc::UnboundedReceiver<Command<I>>,
    conns: BTreeMap<ConnectionId, ConnRecord<I>>,
    /// Introductions waiting for an `accept()`, oldest first.
    ///
    /// This holds `IntroId`s for entries the **core** has parked, and the
    /// core enforces §6.3's `INTRO_QUEUE_CAP` on them — so it is bounded
    /// *at any instant*, and [`prune_ready`](Self::prune_ready) is what
    /// keeps that true over time. Nothing reliable lives here (§10.6): an
    /// introduction is stage-0 bytes the core owns, not payload.
    ready: VecDeque<ReadyIntro>,
    /// `accept()` callers waiting for an introduction, oldest first.
    waiting: VecDeque<oneshot::Sender<Intro<I>>>,
    /// The identity-erased handle a `Connection` keeps (see [`ShellLink`]).
    /// Built once: every connection handle clones this `Rc`.
    link: Rc<dyn ShellLink>,
}

impl<I: Identity + 'static, W: Wire> Driver<I, W> {
    pub(crate) fn new(
        wire: W,
        shell: Shell<I>,
        commands: mpsc::UnboundedReceiver<Command<I>>,
    ) -> Self {
        Self {
            wire,
            link: Rc::new(shell.clone()),
            shell,
            commands,
            conns: BTreeMap::new(),
            ready: VecDeque::new(),
            waiting: VecDeque::new(),
        }
    }

    /// Run until every handle is gone (§16.3).
    pub(crate) async fn run(mut self) {
        let mut buf = [0u8; constants::MAX_DATAGRAM];

        loop {
            // 1. Everything the cores want done, and the I/O for it. This
            //    is where §16.4's drain contract is discharged.
            let outgoing = self.serve();
            self.transmit(outgoing).await;

            // 2. §16.3: "dropping every handle stops it, and every session
            //    dies silently with it." Checked **after** the I/O above,
            //    so a CLOSE sealed by a legitimate last-handle-to-*this*-
            //    connection drop still reaches the wire. Ruling 88's
            //    coincident case seals nothing in the first place, so
            //    nothing here can resurrect it.
            //    The borrow is bound to a `let` rather than left as a
            //    temporary in the `if` condition: a condition temporary's
            //    scope reaches the end of the whole `if`, and nothing in
            //    this file may hold a cell borrow across a block it does
            //    not control.
            let handles = self.shell.state.borrow().handles;
            if handles == 0 {
                break;
            }

            // 3. Nothing left to do: wait for the next thing that could
            //    change that.
            let deadline = self.deadline();
            let event = {
                let Self { wire, commands, .. } = &mut self;
                tokio::select! {
                    biased;
                    command = commands.recv() => Event::Command(command),
                    received = wire.recv_from(&mut buf) => Event::Received(received),
                    () = sleep_until(deadline), if deadline.is_some() => Event::Timeout,
                }
            };

            match event {
                Event::Command(Some(command)) => self.handle_command(command),
                // Unreachable while a handle lives, and step 2 broke the
                // loop if none does.
                Event::Command(None) => break,
                Event::Received(Ok((len, src))) => {
                    let len = len.min(buf.len());
                    self.handle_datagram(src, &buf[..len]);
                }
                // A receive error is not a protocol event. §7.4 makes
                // liveness receive-driven and ruling 49 makes a failing
                // *send* a trace and nothing more; the same reasoning
                // applies to the other half of the seam.
                Event::Received(Err(error)) => {
                    tracing::warn!(
                        target: "slither::io",
                        verb = "recv_from",
                        %error,
                        "Wire::recv_from failed; the endpoint is untouched",
                    );
                }
                Event::Timeout => self.handle_timeout(),
            }
        }

        self.shutdown();
    }

    // ═══════════════════════════════════════════════════════════════════
    // Draining — synchronous by construction, so no borrow can span a yield
    // ═══════════════════════════════════════════════════════════════════

    /// Drain both cores to their terminal `Timeout` and collect the I/O.
    fn serve(&mut self) -> Vec<Outgoing> {
        let mut out = Vec::new();

        for _ in 0..DRAIN_BOUND {
            self.serve_endpoint(&mut out);

            // A connection is dirty because a handle mutated it
            // (`close()`, a last-handle drop), because a datagram or a
            // timeout reached it, or because the endpoint core just
            // installed its session. Draining one can retire it, which
            // mutates the endpoint core, which can queue more — hence the
            // outer loop.
            let dirty: Vec<ConnectionId> = self
                .conns
                .iter()
                .filter(|(_, record)| record.cell.borrow().dirty)
                .map(|(id, _)| *id)
                .collect();
            if dirty.is_empty() {
                break;
            }
            for id in dirty {
                self.serve_connection(id, &mut out);
            }
        }

        self.release_dead();
        self.prune_ready();
        self.dispatch_intros();
        out
    }

    /// Drop `IntroId`s the endpoint core no longer holds.
    ///
    /// **This is what keeps `ready` bounded, and §10.6 requires it.** The
    /// queue holds ids for entries the core has parked under §6.3's
    /// `INTRO_QUEUE_CAP`, so it is bounded *at any instant* — but an
    /// application that never calls `accept()` would otherwise accumulate
    /// one dead id per expiry for the endpoint's life, which is exactly the
    /// unbounded intermediate queue §10.6 forbids.
    ///
    /// It is also the difference between an `accept()` that hands out a
    /// live introduction and one that hands out a corpse: a stale id ahead
    /// of a live one in the queue would resolve `IntroError::Expired` at
    /// `read_identity()` while a perfectly good initiation waited behind
    /// it.
    ///
    /// `intro_source` is the presence oracle (ruling 71: it reads through
    /// to live state, so it cannot answer from a cache).
    fn prune_ready(&mut self) {
        if self.ready.is_empty() {
            return;
        }
        let state = self.shell.state.borrow();
        self.ready
            .retain(|ready| state.endpoint.intro_source(ready.id).is_some());
    }

    /// Drain the endpoint core (§16.4).
    fn serve_endpoint(&mut self, out: &mut Vec<Outgoing>) {
        for _ in 0..DRAIN_BOUND {
            let output = self.shell.state.borrow_mut().endpoint.poll_output();
            match output {
                EndpointOutput::Timeout(_) => return,
                EndpointOutput::Transmit(transmit) => out.push(Outgoing {
                    conn: None,
                    transmit,
                }),
                EndpointOutput::IntroReady(id, source) => {
                    self.ready.push_back(ReadyIntro { id, source });
                }
                EndpointOutput::ToConnection(id, install) => {
                    if let Some(record) = self.conns.get(&id) {
                        let mut cell = record.cell.borrow_mut();
                        if let Some(core) = cell.core.as_mut() {
                            core.handle_endpoint_event(now(), install);
                        }
                        cell.dirty = true;
                    }
                }
                EndpointOutput::HandshakeFailed(id, error) => self.fail_pending(id, error),
            }
        }
        panic!(
            "core::Endpoint::poll_output did not reach Timeout in {DRAIN_BOUND} outputs (§16.4)"
        );
    }

    /// Drain one connection core (§16.4), publishing its events.
    ///
    /// **`Retired` is delivered to the endpoint core inside this loop** —
    /// see the module docs.
    fn serve_connection(&mut self, id: ConnectionId, out: &mut Vec<Outgoing>) {
        let Some(record) = self.conns.get(&id) else {
            return;
        };
        let cell = Rc::clone(&record.cell);

        for _ in 0..DRAIN_BOUND {
            let output = {
                let mut borrow = cell.borrow_mut();
                borrow.dirty = false;
                match borrow.core.as_mut() {
                    Some(core) => core.poll_output(),
                    None => return,
                }
            };

            match output {
                ConnOutput::Timeout(_) => return,
                ConnOutput::Transmit(transmit) => out.push(Outgoing {
                    conn: Some(id),
                    transmit,
                }),
                ConnOutput::ToEndpoint(event) => {
                    debug_assert!(
                        matches!(event, ToEndpoint::Retired { .. }),
                        "§16.4 defines exactly one connection→endpoint event",
                    );
                    // The MUST, discharged here and not later.
                    self.shell
                        .state
                        .borrow_mut()
                        .endpoint
                        .handle_connection_event(now(), id, event);
                }
                ConnOutput::Event(event) => self.publish(id, &cell, event),
            }
        }
        panic!(
            "core::Connection::poll_output did not reach Timeout in {DRAIN_BOUND} outputs (§16.4)"
        );
    }

    /// Turn a `ConnEvent` into the shell surface it serves (§16.2).
    fn publish(
        &mut self,
        id: ConnectionId,
        cell: &Rc<RefCell<ConnCell<I::Suite>>>,
        event: ConnEvent,
    ) {
        match event {
            ConnEvent::Established => self.establish(id, cell),
            ConnEvent::Closed(lost) => Self::latch(cell, lost),
        }
    }

    /// Ruling 46's latch: set once, read for ever.
    ///
    /// §16.4 emits `Closed` exactly once per connection and the core
    /// `debug_assert!`s it, so the `is_none()` guard here is defence for the
    /// one place a second value would be observable — [`shutdown`], which
    /// latches `EndpointDropped` over connections that may already have
    /// died.
    ///
    /// [`shutdown`]: Self::shutdown
    fn latch(cell: &Rc<RefCell<ConnCell<I::Suite>>>, lost: ConnectionLost) {
        let mut borrow = cell.borrow_mut();
        if borrow.closed.is_none() {
            borrow.closed = Some(lost);
            borrow.closed_wakers.wake_all();
        }
    }

    /// A session installed: fill in what the accessors read, mark the
    /// static LIVE, and resolve the `Connecting`.
    fn establish(&mut self, id: ConnectionId, cell: &Rc<RefCell<ConnCell<I::Suite>>>) {
        let Some(record) = self.conns.get_mut(&id) else {
            return;
        };

        let anchor = {
            let borrow = cell.borrow();
            match borrow.core.as_ref().and_then(CoreConnection::session) {
                Some(session) => session.anchor,
                None => {
                    debug_assert!(false, "ConnEvent::Established without an installed session");
                    return;
                }
            }
        };
        cell.borrow_mut().remote_address = anchor;

        // PENDING → LIVE, stamp-checked.
        if let Some(slot) = self
            .shell
            .state
            .borrow_mut()
            .statics
            .get_mut(&record.static_key)
            && slot.attempt == record.attempt
        {
            slot.state = StaticState::Live;
        }

        let Some(slot) = record.slot.take() else {
            return;
        };
        let session_id = match session_id_of(cell) {
            Some(session_id) => session_id,
            None => return,
        };
        let handle = Connection::new(
            Rc::clone(&self.link),
            Rc::clone(cell),
            id,
            record.remote_static.clone(),
            session_id,
        );
        slot.borrow_mut().resolve(PendingOutcome::Ready(handle));
    }

    /// §5.5's give-up, or ruling 72's local failure: resolve the
    /// `Connecting` and forget the connection that never was.
    ///
    /// No `Retired` is synthesised. §16.4's `HandshakeFailed` is the
    /// endpoint core telling us it has *already* dropped the pending, and
    /// no session was ever installed, so there is no index route and no
    /// guard pin left to release (rulings 81/84's "no `Retired` at all"
    /// case).
    fn fail_pending(&mut self, id: ConnectionId, error: ConnectError) {
        let Some(record) = self.conns.remove(&id) else {
            return;
        };
        self.shell
            .state
            .borrow_mut()
            .release_static(&record.static_key, record.attempt);
        if let Some(slot) = record.slot {
            slot.borrow_mut().resolve(PendingOutcome::Failed(error));
        }
    }

    /// Release the shell-side bookkeeping of every connection whose core
    /// has finished — **after** its `Retired` reached the endpoint core.
    ///
    /// "Finished" is `Closed` latched **and** the session gone. A merely
    /// *closing* connection still holds its session and still answers its
    /// accessors, which is what §15.2's 5 s linger is for; it is released
    /// when `CloseLinger` expires and the core drops its state.
    fn release_dead(&mut self) {
        let done: Vec<ConnectionId> = self
            .conns
            .iter()
            .filter(|(_, record)| {
                let cell = record.cell.borrow();
                cell.closed.is_some() && !cell.is_established()
            })
            .map(|(id, _)| *id)
            .collect();

        for id in done {
            let Some(record) = self.conns.remove(&id) else {
                continue;
            };
            self.shell
                .state
                .borrow_mut()
                .release_static(&record.static_key, record.attempt);
            // The core is released here, and only here. A handle that
            // calls `close()` afterwards finds `None` and does nothing —
            // the same answer the core itself would have given.
            record.cell.borrow_mut().core = None;
            if let Some(slot) = record.slot {
                slot.borrow_mut()
                    .resolve(PendingOutcome::Failed(ConnectError::Local));
            }
        }
    }

    /// Hand parked introductions to waiting `accept()` callers.
    fn dispatch_intros(&mut self) {
        while !self.ready.is_empty() {
            // Prune callers whose future was dropped. On a single-threaded
            // runtime this check and the `send` below are atomic with
            // respect to the application: the driver is running, so no
            // other task on this thread can drop a receiver between them.
            while self.waiting.front().is_some_and(oneshot::Sender::is_closed) {
                self.waiting.pop_front();
            }
            let Some(reply) = self.waiting.pop_front() else {
                return;
            };
            let ready = self
                .ready
                .pop_front()
                .expect("the loop condition checked it");
            let sender_index = self
                .shell
                .state
                .borrow()
                .endpoint
                .intro_sender_index(ready.id)
                .unwrap_or_default();
            let intro = Intro::new(self.shell.clone(), ready.id, ready.source, sender_index);
            // On the unreachable `Err`, the returned `Intro`'s own `Drop`
            // is §6.2's silent reject — the documented meaning of dropping
            // a staged object, not a leak.
            drop(reply.send(intro));
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // I/O
    // ═══════════════════════════════════════════════════════════════════

    /// Send everything the drain produced.
    ///
    /// **Ruling 49:** a failing `send_to` is traced against the connection
    /// whose datagram it was and **nothing else** — no teardown, no verb
    /// resolved with an error, no notification. §18.1 gains no I/O variant.
    async fn transmit(&mut self, outgoing: Vec<Outgoing>) {
        for Outgoing { conn, transmit } in outgoing {
            let Transmit { to, data } = transmit;
            if let Err(error) = self.wire.send_to(&data, to).await {
                tracing::warn!(
                    target: "slither::io",
                    verb = "send_to",
                    conn = ?conn,
                    to = %to,
                    %error,
                    "Wire::send_to failed; the connection is untouched (§16.3, ruling 49)",
                );
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Inputs
    // ═══════════════════════════════════════════════════════════════════

    fn handle_datagram(&mut self, src: SocketAddr, datagram: &[u8]) {
        let now = now();
        let disposition = self
            .shell
            .state
            .borrow_mut()
            .endpoint
            .handle_datagram(now, src, datagram);

        if let Disposition::ForConnection(id) = disposition
            && let Some(record) = self.conns.get(&id)
        {
            let mut cell = record.cell.borrow_mut();
            if let Some(core) = cell.core.as_mut() {
                core.handle_datagram(now, src, datagram);
            }
            cell.dirty = true;
        }
    }

    fn handle_timeout(&mut self) {
        let now = now();
        self.shell.state.borrow_mut().endpoint.handle_timeout(now);
        for record in self.conns.values() {
            let mut cell = record.cell.borrow_mut();
            if let Some(core) = cell.core.as_mut() {
                core.handle_timeout(now);
            }
            cell.dirty = true;
        }
    }

    /// The earliest deadline either core announced (§16.5).
    ///
    /// Read out of `poll_output()`'s terminal `Timeout`, which §16.4 makes
    /// "simultaneously the drain sentinel and the next-deadline
    /// announcement". Both queues are empty here — [`serve`](Self::serve)
    /// ran to `Timeout` and every mutating call since sent a command rather
    /// than queuing an output — so this pops nothing.
    fn deadline(&self) -> Option<std::time::Instant> {
        let endpoint = match self.shell.state.borrow_mut().endpoint.poll_output() {
            EndpointOutput::Timeout(deadline) => deadline,
            _ => {
                debug_assert!(false, "the endpoint core queued an output outside a drain");
                None
            }
        };

        self.conns
            .values()
            .filter_map(|record| {
                let mut cell = record.cell.borrow_mut();
                match cell.core.as_mut()?.poll_output() {
                    ConnOutput::Timeout(deadline) => deadline,
                    _ => {
                        debug_assert!(false, "a connection core queued an output outside a drain");
                        None
                    }
                }
            })
            .chain(endpoint)
            .min()
    }

    // ═══════════════════════════════════════════════════════════════════
    // Commands
    // ═══════════════════════════════════════════════════════════════════

    fn handle_command(&mut self, command: Command<I>) {
        match command {
            Command::Connect {
                remote,
                remote_static,
                static_key,
                attempt,
                slot,
            } => self.command_connect(remote, remote_static, static_key, attempt, slot),
            Command::Cancel(slot) => self.command_cancel(&slot),
            Command::Accept(reply) => {
                self.waiting.push_back(reply);
                self.dispatch_intros();
            }
            Command::ReadIdentity(id, reply) => {
                let result = self.shell.state.borrow_mut().endpoint.read_identity(id);
                drop(reply.send(result));
            }
            Command::Authenticate(id, reply) => {
                let result = self
                    .shell
                    .state
                    .borrow_mut()
                    .endpoint
                    .authenticate(now(), id);
                drop(reply.send(result));
            }
            Command::AcceptChain(id, remote_static, reply) => {
                self.command_accept_chain(id, remote_static, reply);
            }
            Command::Reject(id) => self.shell.state.borrow_mut().endpoint.reject(now(), id),
            Command::Dirty(id) => {
                if let Some(record) = self.conns.get(&id) {
                    record.cell.borrow_mut().dirty = true;
                }
            }
            // The loop's own liveness check reads the count; this variant
            // exists only to wake the `select!`.
            Command::HandlesGone => {}
        }
    }

    /// §16.2's `connect()`, on the driver task where §6.1's two initiator
    /// DH may be spent (ruling 87).
    fn command_connect(
        &mut self,
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
        static_key: Vec<u8>,
        attempt: u64,
        slot: Rc<RefCell<PendingSlot<I>>>,
    ) {
        // The `Connecting` may already have been dropped — on a paused
        // clock nothing here has run since the verb returned. The `Cancel`
        // is behind us in the queue and will retire this pending; short-
        // circuiting would leave the core without the pending the cancel is
        // about to release, and the retransmit train running.
        let result =
            self.shell
                .state
                .borrow_mut()
                .endpoint
                .connect(now(), remote, remote_static.clone());

        match result {
            Ok((id, core)) => {
                slot.borrow_mut().id = Some(id);
                let cell = Rc::new(RefCell::new(ConnCell {
                    core: Some(core),
                    remote_address: remote,
                    closed: None,
                    closed_wakers: Wakers::default(),
                    notifications: NotificationSlots,
                    dirty: true,
                    handles: 0,
                }));
                self.conns.insert(
                    id,
                    ConnRecord {
                        cell,
                        static_key,
                        remote_static,
                        attempt,
                        slot: Some(slot),
                    },
                );
            }
            Err(error) => {
                // The shell's map said NONE and the core says otherwise:
                // the two have diverged, which is the one failure mode the
                // duplication in `ShellState::statics` can have.
                debug_assert!(
                    false,
                    "the shell's static map admitted a connect the core refused: {error:?}",
                );
                self.shell
                    .state
                    .borrow_mut()
                    .release_static(&static_key, attempt);
                slot.borrow_mut().resolve(PendingOutcome::Failed(error));
            }
        }
    }

    /// Ruling 50: a `Connecting` was dropped.
    ///
    /// §16.4's API list has no cancel verb, and `ToEndpoint::Retired` is
    /// the core-side effect S29 needs — the endpoint core's own docs name
    /// this as "S29's cancellation path": it stops §5.5's retransmit train,
    /// frees the pending index (§17.3), takes the dialled address out of
    /// §6.5's hint set (§17.4), releases the §17.1 pin, and frees the
    /// static so the very next `connect()` succeeds. It emits nothing — the
    /// `Connecting` is already resolved by its own drop, and a
    /// `HandshakeFailed` would be a second resolution.
    ///
    /// # Why `our_index: 0`
    ///
    /// A `connect()`-created connection that never installed a session has
    /// no session index to name. `handle_connection_event` passes the value
    /// to `remove_session` and `remove_pending` before doing the work that
    /// actually matters here (`drop_pending`, which removes the *current*
    /// attempt's index from the pending's own record). **`0` cannot collide
    /// with anything**: `IndexTables::mint` draws a random **nonzero**
    /// `u32` absent from both tables, so no live route is ever keyed on it.
    /// Any other placeholder would have a 2⁻³² chance per live session of
    /// evicting somebody else's route.
    fn command_cancel(&mut self, slot: &Rc<RefCell<PendingSlot<I>>>) {
        let Some(id) = slot.borrow().id else {
            // The `Connect` ahead of us in the queue failed, so there is no
            // pending to cancel and the static was already released.
            return;
        };
        let Some(_record) = self.conns.remove(&id) else {
            return;
        };
        self.shell
            .state
            .borrow_mut()
            .endpoint
            .handle_connection_event(now(), id, ToEndpoint::Retired { our_index: 0 });
        // The shell-side static was released synchronously by
        // `Connecting::drop` — before this command was even queued, which
        // is what makes the immediate redial work with no clock advance.
    }

    /// §6.2's stage 3, on the driver task where its two DH belong.
    fn command_accept_chain(
        &mut self,
        id: IntroId,
        remote_static: PublicKeyOf<I>,
        reply: oneshot::Sender<Result<Connection<I::Suite>, AcceptError>>,
    ) {
        let accepted = self.shell.state.borrow_mut().endpoint.accept(now(), id);
        let (conn_id, core) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                drop(reply.send(Err(error)));
                return;
            }
        };

        let established = core.session().map(|session| {
            (
                session.anchor,
                <I::Suite as Handshake>::session_id(&session.seal).clone(),
            )
        });
        let Some((anchor, session_id)) = established else {
            debug_assert!(false, "§16.4: `accept()` returns an established connection");
            drop(reply.send(Err(AcceptError::Stale)));
            return;
        };
        // The proven static comes from the `Proven` handle rather than from
        // the core: §16.4's `accept()` returns `(ConnectionId, Connection)`
        // and the core exposes no per-connection static accessor. The
        // handle's value *is* the proven one — `Proven` is only reachable
        // through `authenticate()`, which returned it.
        let static_key = remote_static.as_ref().to_vec();

        let cell = Rc::new(RefCell::new(ConnCell {
            core: Some(core),
            remote_address: anchor,
            closed: None,
            closed_wakers: Wakers::default(),
            notifications: NotificationSlots,
            dirty: true,
            handles: 0,
        }));

        // NONE → LIVE. `claim_static` is not used: the shell's map is the
        // *outbound* admission test, and an inbound replacement is §5.4's
        // business, decided in the core, which has already decided it.
        let attempt = {
            let mut state = self.shell.state.borrow_mut();
            let attempt = state.next_attempt;
            state.next_attempt += 1;
            state.statics.insert(
                static_key.clone(),
                super::shared::StaticSlot {
                    attempt,
                    state: StaticState::Live,
                },
            );
            attempt
        };

        let handle = Connection::new(
            Rc::clone(&self.link),
            Rc::clone(&cell),
            conn_id,
            remote_static.clone(),
            session_id,
        );
        self.conns.insert(
            conn_id,
            ConnRecord {
                cell,
                static_key,
                remote_static,
                attempt,
                slot: None,
            },
        );

        // If the `accept()` future was cancelled, this `Connection` is
        // dropped here — the last handle to it — and §16.2's
        // `close(NO_ERROR, "")` follows, which is the right answer for a
        // connection nobody claimed.
        drop(reply.send(Ok(handle)));
    }

    /// The driver has stopped (§16.3): every session dies silently, and
    /// **nothing is transmitted** (§15.4's endpoint-dropped row).
    fn shutdown(&mut self) {
        self.shell.state.borrow_mut().driver_stopped = true;
        for record in self.conns.values() {
            Self::latch(&record.cell, ConnectionLost::EndpointDropped);
            record.cell.borrow_mut().core = None;
        }
        // Dropping the senders is what makes a parked `accept()` resolve
        // `None` — §16.2's "None = endpoint closed".
        self.waiting.clear();
        self.conns.clear();
    }
}

/// One datagram to put on the wire, and the connection it belongs to
/// (ruling 49's attribution).
///
/// `None` is an endpoint-core transmit — a msg1, a retransmit, or a msg2.
/// §16.4's `EndpointOutput::Transmit` carries no connection id, so slice 3
/// cannot attribute those; see `IMPLEMENTATION-3b.md` §5, finding U7.
struct Outgoing {
    conn: Option<ConnectionId>,
    transmit: Transmit,
}

/// hiss's channel binding for an installed session (ruling 89).
fn session_id_of<S: Handshake>(cell: &Rc<RefCell<ConnCell<S>>>) -> Option<hiss::noise::SessionId> {
    let borrow = cell.borrow();
    let session = borrow.core.as_ref()?.session()?;
    Some(<S as Handshake>::session_id(&session.seal).clone())
}

/// `tokio::time::sleep_until`, or a future that never completes.
///
/// The `select!` arm is guarded by `deadline.is_some()`, so the `None` case
/// is never polled; it exists to give the arm one type.
async fn sleep_until(deadline: Option<std::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}

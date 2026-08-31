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
//! ruling 50's cancel **synthesises** one for a pending that has no session
//! index to name. After ruling 90 that synthesis happens in
//! `Connecting::drop` rather than here, because the redial that follows it
//! reads the endpoint core's own map; see [`Driver::command_cancel`], which
//! is what is left of the cancel on this side.
//!
//! # The static map — there is exactly one, and the driver does not own it
//!
//! **[RULING 90]** §5.4's NONE/PENDING/LIVE state lives in the endpoint
//! core's `StaticMap` and **nowhere else**. Every transition is the core's:
//!
//! | Transition | Made by | When |
//! |---|---|---|
//! | NONE → PENDING | `mint_pending` | `Endpoint::connect`, synchronously on the handle (0 DH) |
//! | PENDING → NONE | `handle_connection_event(Retired)` | `Connecting::drop`, synchronously on the handle (ruling 50) |
//! | PENDING → NONE | `drop_pending` | §5.5's give-up, inside `handle_timeout` |
//! | PENDING → LIVE | `StaticMap::promote` | `complete_initiation` — the msg2 that installs |
//! | NONE → LIVE | `accept` | §6.2's stage 3 |
//! | LIVE → NONE | `handle_connection_event(Retired)` | teardown |
//!
//! Until ruling 90 the shell kept a **stamped mirror** of that map, because
//! ruling 87 made §16.2's `connect()` synchronous while slice 3a's
//! `core::Endpoint::connect()` mints the pending *and* builds msg1 in one
//! 2-DH call — so the handle could not reach the core's map without paying
//! on the caller's task. The mirror was a second record of a security
//! invariant, it needed a monotone `attempt` stamp on every entry to survive
//! cancel-and-redial, and it was the root of the seam review's worst finding
//! (C-B1). Splitting the core verb deleted all of it: the map, the stamps,
//! `claim_static`, `release_static` and `next_attempt`.
//!
//! What that buys, beyond one fewer copy: the two maps can no longer
//! disagree **because there is only one**, so `[AcceptChain(K), Connect(K)]`
//! in the command queue is decided by §6.4's PENDING branch in the core
//! rather than by which command the driver reached first.
//!
//! # Stopping
//!
//! [`Driver::stop`] runs from [`Driver`]'s `Drop`, so it runs on an unwind
//! as well as on the ordinary exit. That is not defensive decoration: this
//! task is spawned with `spawn_local` and its `JoinHandle` is dropped, so a
//! panic here is **silent**, and without the guard it also left every
//! `closed()` future and every in-flight `Connecting` parked for ever
//! behind accessors that kept answering from a cell nobody would write
//! again. Read `stop`'s docs before adding a new kind of waiter: the rule
//! is that anything parked on the *cell* side of §16.3's seam must be
//! resolved there explicitly, because only the `oneshot`-backed verbs
//! unblock themselves.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::rc::Rc;

use tokio::sync::{mpsc, oneshot};

use crate::constants;
use crate::core::{
    ConnEvent, ConnOutput, Connection as CoreConnection, ConnectionId, Dir, Disposition,
    EndpointOutput, IntroId, ToEndpoint, Transmit,
};
use crate::error::{AcceptError, ConnectError, ConnectionLost};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::Handshake;

use super::connection::Connection;
use super::shared::{
    Command, ConnCell, NotificationSlots, PendingOutcome, PendingSlot, Shell, ShellLink, Wakers,
    now, resolve_slot, wake_settled,
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
    /// The peer's static, kept for the accessors a [`Connection`] handle
    /// carries. **Not** a static-map key: ruling 90 leaves that map in the
    /// core, and nothing here writes it.
    remote_static: PublicKeyOf<I>,
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
    /// The instant [`handle_timeout`](Self::handle_timeout) last ran, if the
    /// loop has not seen another kind of event since.
    ///
    /// **F1's spin detector, and nothing else reads it.** A deadline that is
    /// merely *overdue* is ordinary; one still due at the instant the
    /// timeout pass just ran is a timer its own firing re-armed in the past.
    /// See [`deadline`](Self::deadline) for why the difference is the whole
    /// of ruling 141's class.
    last_timeout: Option<std::time::Instant>,
    /// **[ruling 255]** Consecutive turns whose previous event was a timer
    /// firing and whose announced min deadline was at or before that
    /// firing's instant. One such turn is ordinary (a firing can shrink a
    /// *different* timer's deadline into the past — a lost datagram
    /// retransmits nothing while an ACK reset shrinks the PTO interval);
    /// a genuine spin re-fires immediately forever. `Cell` because
    /// [`deadline`](Self::deadline) takes `&self`.
    overdue_streak: std::cell::Cell<u32>,
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
            last_timeout: None,
            overdue_streak: std::cell::Cell::new(0),
            conns: BTreeMap::new(),
            ready: VecDeque::new(),
            waiting: VecDeque::new(),
        }
    }

    /// Run until every handle is gone (§16.3).
    pub(crate) async fn run(mut self) {
        let mut buf = [0u8; constants::MAX_DATAGRAM];

        loop {
            // 1. Everything the cores want done. This is where §16.4's
            //    drain contract is discharged.
            let outgoing = self.serve();

            // 2. The next deadline, read **here** — after the drain and
            //    before the first yield of the iteration.
            //
            //    **[ruling 262]** This is a *pure read*: `deadline` collects
            //    through `next_deadline`, not `poll_output`, so it consumes
            //    nothing and the position is no longer load-bearing for
            //    correctness. It used to be: `poll_output` **pops**, so a
            //    `close()` landing during `transmit()`'s yield — §16.3
            //    (ruling 53) puts `close()` on the *handle* side of the seam,
            //    and `Wire::send_to` is an application-supplied `async fn`
            //    that a real socket leaves `Pending` on a full send buffer —
            //    queued a `Transmit` on an already-drained core, which a
            //    deadline read after the yield then destroyed: a silently
            //    lost CLOSE and one effective dead-timeout window for the
            //    peer. Ruling 262 removed the destruction rather than the discipline; see
            //    `deadline` for why the discipline alone was never enough.
            //
            //    It stays here because the value is freshest here, and
            //    reading it here cannot go stale in a way that matters:
            //    **every** handle-side core mutation ends by sending a
            //    command (`Connection::close_now` → `Command::Dirty`,
            //    `Connecting::drop` → `Cancel`, `Endpoint::connect` →
            //    `Connect`), and the command arm is `biased` first, so a
            //    mutation during the yield wins the `select!` immediately
            //    and the loop recomputes rather than sleeping on the old
            //    value. `sleep_until` is absolute, so a long send does not
            //    shift it either.
            let deadline = self.deadline();

            self.transmit(outgoing).await;

            // 3. §16.3: "dropping every handle stops it, and every session
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

            // 4. Nothing left to do: wait for the next thing that could
            //    change that.
            //
            //    **[RATIFIED 2026/08/18 — ruling 271] `biased`, and the
            //    order of the last two arms, is now protocol.** §12.4's
            //    coalesced ACK is armed at `now` — already due — and the
            //    core has no notion of a "receive drain": this `select!` is
            //    where that notion lives. `recv_from` sits **above**
            //    `sleep_until`, so an already-due deadline loses to every
            //    datagram still on the socket and wins the instant the
            //    socket empties. That is exactly *"the end of the receive
            //    drain"*, and it is why per-drain coalescing needed no new
            //    core API.
            //
            //    Swapping these two arms, or dropping `biased`, does not
            //    break anything visibly: it silently returns the ACK cadence
            //    to one per received datagram — **worse** than the every-2nd
            //    policy 271 replaced — with every test still green and
            //    nothing on the wire to say so but a datagram census.
            //    Measured at the cadence this ordering does produce:
            //    ACK-only datagrams fell from 33.6 % of wire traffic to
            //    3.4 %, and `bulk` throughput rose 85 → 111 MiB/s.
            let event = {
                let Self { wire, commands, .. } = &mut self;
                tokio::select! {
                    biased;
                    command = commands.recv() => Event::Command(command),
                    received = wire.recv_from(&mut buf) => Event::Received(received),
                    () = sleep_until(deadline), if deadline.is_some() => Event::Timeout,
                }
            };

            if !matches!(event, Event::Timeout) {
                self.last_timeout = None;
                self.overdue_streak.set(0);
            }

            match event {
                Event::Command(Some(command)) => self.handle_command(command),
                // Unreachable while a handle lives, and step 3 broke the
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

        // No `self.stop()` here, deliberately: [`Driver`]'s `Drop` is the
        // **one** stop path, and `self` is dropped on the next line. That
        // is what makes the stop run on an unwind as well — see the `Drop`
        // impl at the bottom of this file.
    }

    // ═══════════════════════════════════════════════════════════════════
    // Draining — synchronous by construction, so no borrow can span a yield
    // ═══════════════════════════════════════════════════════════════════

    /// Drain both cores to their terminal `Timeout` and collect the I/O.
    ///
    /// # Exhausting the pass bound is a panic, like both inner loops
    ///
    /// [`serve_endpoint`](Self::serve_endpoint) and
    /// [`serve_connection`](Self::serve_connection) both end their
    /// `DRAIN_BOUND` loop in a named `panic!`; this one used to fall
    /// through silently, which is the one asymmetry of the three. It is
    /// not benign: falling through means proceeding to `release_dead`,
    /// `prune_ready` and then [`deadline`](Self::deadline) with connection
    /// cores still dirty and the endpoint core undrained, so whatever they
    /// queued is **not collected by this pass** and `transmit` never sees
    /// it. Since ruling 262 `deadline` no longer *destroys* it — it reads
    /// rather than pops — but nothing here posts a `Command::Dirty` either,
    /// so an undrained `Transmit` is held until some unrelated event wakes
    /// the driver. A silent exhaustion therefore still degrades to a CLOSE
    /// the peer may wait one effective dead-timeout window for; the loss
    /// became a stall, which is not an improvement worth being quiet about.
    ///
    /// Panicking is now also *cheaper* than it was when this loop was
    /// written: [`Driver`]'s `Drop` runs [`stop`](Self::stop) on an unwind,
    /// so a driver panic degrades to `ConnectionLost::EndpointDropped` and
    /// `ConnectError::Local` rather than to a frozen endpoint. Loud beats
    /// silent in both directions.
    fn serve(&mut self) -> Vec<Outgoing> {
        let mut out = Vec::new();
        let mut settled = false;

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
                settled = true;
                break;
            }
            for id in dirty {
                self.serve_connection(id, &mut out);
            }
        }
        assert!(
            settled,
            "the shell's drain did not settle in {DRAIN_BOUND} passes (§16.4)"
        );

        self.release_dead();
        self.prune_ready();
        self.prune_waiting();
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

    /// Drop `accept()` callers whose future was dropped.
    ///
    /// [`prune_ready`](Self::prune_ready)'s **dual**, and it runs on the
    /// same terms — unconditionally, once per drain — for the same §10.6
    /// reason. `waiting` is bounded by the number of *live* `accept()`
    /// futures, which is application concurrency; a **cancelled** one leaves
    /// a dead `oneshot::Sender` behind, and a dead sender is exactly the
    /// unbounded intermediate queue §10.6 forbids.
    ///
    /// The canonical cancellation idiom is what produces them —
    ///
    /// ```text
    /// loop {
    ///     tokio::select! {
    ///         intro = endpoint.accept() => { … }
    ///         _ = &mut shutdown => break,
    ///     }
    /// }
    /// ```
    ///
    /// — or `timeout(d, ep.accept())`, and every losing iteration leaves
    /// one. [`dispatch_intros`](Self::dispatch_intros) prunes too, but only
    /// from the front and only while an introduction is available to hand
    /// out, so an endpoint that is never dialled never prunes there at all.
    /// That was the gap: the pruning was written on the side that has an
    /// introduction to deliver rather than on the side that accumulates.
    ///
    /// `is_closed()` is monotone — a receiver that is gone stays gone — so
    /// this can never discard a caller that is still waiting.
    fn prune_waiting(&mut self) {
        self.waiting.retain(|reply| !reply.is_closed());
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
                // §6.4's LIVE branch reaching the connection it names. Both
                // arms are one call into the core plus the dirty flag: the
                // effects — `Closed(Replaced)` and `Retired`, or the PING
                // and `Contested` — come out of that core's own drain, in
                // this same `serve()` pass, because `serve` loops while
                // anything is dirty.
                EndpointOutput::Replaced(id) => {
                    self.deliver_to_core(id, CoreConnection::replaced);
                }
                EndpointOutput::Contested(id) => {
                    self.deliver_to_core(id, CoreConnection::mark_contested);
                }
            }
        }
        panic!(
            "core::Endpoint::poll_output did not reach Timeout in {DRAIN_BOUND} outputs (§16.4)"
        );
    }

    /// Run one mutating verb against a named connection core, and mark it
    /// dirty so this pass drains whatever it produced.
    ///
    /// An unknown id is a no-op: the endpoint core learns of a connection's
    /// death through `Retired`, and the shell releases its record in the
    /// same pass, so a stale id is reachable and is not an error.
    fn deliver_to_core(
        &mut self,
        id: ConnectionId,
        verb: impl FnOnce(&mut CoreConnection<I::Suite>, std::time::Instant),
    ) {
        let Some(record) = self.conns.get(&id) else {
            return;
        };
        let mut cell = record.cell.borrow_mut();
        if let Some(core) = cell.core.as_mut() {
            verb(core, now());
        }
        cell.dirty = true;
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

            // §16.8's four waker maps. Every arm takes the wakers **inside**
            // the borrow and wakes them **outside** it — see `wake_stream`.
            ConnEvent::StreamReadable { r } => {
                Self::wake_stream(cell, |cell| cell.blocked_readers.get_mut(&r));
            }
            ConnEvent::StreamWritable { r } => {
                Self::wake_stream(cell, |cell| cell.blocked_writers.get_mut(&r));
            }
            // The **reader**, not the writer: `poll_read` re-polls and
            // surfaces `Err(ReadError::Reset)`. In slice 4 a peer cannot
            // reset our send half — §9.9's STOP_SENDING is deferred — so
            // there is no blocked writer this could concern.
            //
            // `acked()`'s two maps are woken as well, because
            // `CONTRACT-5b.md` §2.5 names this event beside
            // `StreamFinished` for both. It is a **wake and not a
            // verdict**: on a bidi stream this is §9.6's peer-emitted reset
            // of our *receive* half, our send half is untouched, and
            // `poll_acked` must go on waiting.
            //
            // **The blocked writer is woken too, and slice 6 is why.**
            // Slice 5b recorded that the one reset of ours a peer can cause
            // — §9.8's receiver-emitted overflow reset — was *"not
            // representable in this build"* (`IMPLEMENTATION-5b.md` §4-C1).
            // §9.8 makes it representable, and its whole point is that the
            // sender is **stalled at the stream window** when it arrives:
            // that writer is parked in `blocked_writers`, and without this
            // line it is woken by nothing and stalls for ever anyway —
            // §9.8's loud failure delivered as the silent one it replaces.
            ConnEvent::StreamReset { r, error_code } => {
                // **Ruling 165: latch before waking.** The next ACK frees a
                // peer-reset half and emits `StreamFinished`; without the
                // latch, the writer this wake unparks would then be told
                // `Finished` and `acked()` would answer `Ok(())` — success
                // reported over data the peer discarded.
                //
                // **[RATIFIED 2026/08/16 — ruling 241]** …but only on a
                // **unidirectional** stream. `peer_resets` is read by the
                // *send* half alone (`SendStream::poll_write`,
                // `poll_finish`, `poll_acked`), and ruling 165's premise —
                // *"the bytes were discarded"* — is §9.8's
                // **receiver-emitted** overflow reset, which exists only on
                // the uni message path.
                //
                // On a **bidirectional** stream the same frame means the
                // peer abandoned **its own** send direction, which says
                // nothing about our bytes. §9.9 is explicit while
                // STOP_SENDING is deferred: *"an uninterested receiver drops
                // its handle and discards arrivals, and the sender runs to
                // FIN or resets."* Latching there made the canonical client
                // shape — read a `BiStream` to EOF, drop it — fail the
                // peer's `shutdown()` with `Reset(0)` **after the payload
                // had arrived whole**. Measured; it is how S31 first failed.
                //
                // The recv half is untouched: it learns of the reset through
                // the core, exactly as before.
                let gates_our_send_half = cell
                    .borrow()
                    .core
                    .as_ref()
                    .and_then(|core| core.stream_id(r))
                    .is_some_and(|id| id.dir() == Dir::Uni);
                if gates_our_send_half {
                    cell.borrow_mut().peer_resets.insert(r, error_code);
                }
                Self::wake_stream(cell, |cell| cell.blocked_readers.get_mut(&r));
                Self::wake_stream(cell, |cell| cell.blocked_writers.get_mut(&r));
                Self::wake_stream(cell, |cell| cell.blocked_ackers.get_mut(&r));
                wake_settled(cell);
            }
            // **All of them, not one.** Ruling 99 emits one event per
            // newly-opened stream, and §9.2's implicit open of index 5 opens
            // six streams; but a wake is not a promise of a stream either
            // way, so every parked `accept_*` re-polls and claims at most
            // one. Waking a single waiter per event would be an assumption
            // about a correspondence the core does not guarantee.
            ConnEvent::StreamOpened { dir } => {
                Self::wake_stream(cell, |cell| Some(&mut cell.stream_acceptors[dir.slot()]));
            }
            // **Both sets, for `Dir::Uni`.** §10.4's allowance is what
            // `open_uni()` waits on *and* what a `send_message()` refused
            // before its own `open` waits on — §9.8's stream is allocated
            // from the same cumulative limit and surfaces no handle, so
            // nothing else would ever wake it.
            ConnEvent::StreamsAvailable { dir } => {
                Self::wake_stream(cell, |cell| Some(&mut cell.stream_openers[dir.slot()]));
                if dir == Dir::Uni {
                    Self::wake_stream(cell, |cell| Some(&mut cell.message_senders));
                }
            }
            // **[ruling 150]** §10.3's connection credit, for the one verb
            // that can be refused for it while holding no stream.
            // `StreamWritable` covers every *half* with a blocked writer;
            // this covers the message that has not opened one yet.
            ConnEvent::SendCreditAvailable => {
                Self::wake_stream(cell, |cell| Some(&mut cell.message_senders));
            }
            // §9.8 and §11's claim verbs. One event per claimable item
            // (§2.4's emission scope), and a wake is still not a promise:
            // several parked readers all re-poll and at most one claims.
            ConnEvent::MessageReadable => {
                Self::wake_stream(cell, |cell| Some(&mut cell.message_readers));
            }
            ConnEvent::DatagramReadable => {
                Self::wake_stream(cell, |cell| Some(&mut cell.datagram_readers));
            }
            // **Ruling 47's `acked()`.** The event is §9.7's `DataRecvd`:
            // every byte of this send half **and its FIN** acknowledged.
            // It is latched as well as woken — see
            // [`ConnCell::finished_senders`] for why a wake alone hangs the
            // ordinary sequence — and the connection-level snapshot is
            // re-polled beside it, since a stream reaching `DataRecvd` is
            // one of the two things that can settle one.
            ConnEvent::StreamFinished { r } => {
                let woken = {
                    let mut borrow = cell.borrow_mut();
                    let mut woken = borrow.note_send_finished(r);
                    woken.extend(borrow.settled_wakers.take_all());
                    woken
                };
                for waker in woken {
                    waker.wake();
                }
            }

            // §16.2's three notifications (ruling 46). Each fills its slot
            // and wakes the one set; the slot is what makes the fact
            // **retained** rather than dropped on the floor between the
            // application's two visits, and the wake is what releases a
            // `notified()` that is parked right now.
            //
            // `AddressMoved` also moves the accessor: `remote_address()`
            // reads this field, and §7.3's roam is the only thing that ever
            // changes it after the install.
            ConnEvent::AddressMoved { from, to } => {
                Self::notify(cell, |slots| slots.address_moved(from, to), Some(to));
            }
            ConnEvent::Contested => {
                Self::notify(cell, NotificationSlots::contested, None);
            }
            ConnEvent::ContestCleared => {
                Self::notify(cell, NotificationSlots::contest_cleared, None);
            }
        }
    }

    /// Fill one notification slot, optionally move the anchor mirror, and
    /// wake the waiters **outside** the borrow (finding F10).
    ///
    /// [`wake_stream`](Self::wake_stream)'s sibling: that one only selects a
    /// set, and these three arms have to write before they wake.
    fn notify(
        cell: &Rc<RefCell<ConnCell<I::Suite>>>,
        fill: impl FnOnce(&mut NotificationSlots),
        anchor: Option<SocketAddr>,
    ) {
        let woken = {
            let mut borrow = cell.borrow_mut();
            if let Some(anchor) = anchor {
                borrow.remote_address = anchor;
            }
            fill(&mut borrow.notifications);
            borrow.notification_wakers.take_all()
        };
        for waker in woken {
            waker.wake();
        }
    }

    /// Wake one waker set, with the cell borrow released first.
    ///
    /// The selector runs under the borrow and hands back the set; the wakes
    /// happen after it ends. That order is the whole point: a `Waker` is
    /// **application-supplied**, and an executor that polls a ready task
    /// inline rather than queueing it re-enters `poll_read`/`poll_write`,
    /// which take this same `borrow_mut`. `already mutably borrowed` inside
    /// the driver task, from a consumer doing nothing wrong.
    /// [`Wakers::take_all`] is `#[must_use]` so that this is the only
    /// spelling the type permits, and this helper is where the six arms
    /// share it rather than each restating it.
    fn wake_stream(
        cell: &Rc<RefCell<ConnCell<I::Suite>>>,
        select: impl FnOnce(&mut ConnCell<I::Suite>) -> Option<&mut Wakers>,
    ) {
        let woken = {
            let mut borrow = cell.borrow_mut();
            select(&mut borrow)
                .map(Wakers::take_all)
                .unwrap_or_default()
        };
        for waker in woken {
            waker.wake();
        }
    }

    /// A session installed: fill in what the accessors read, mark the
    /// static LIVE, and resolve the `Connecting`.
    fn establish(&mut self, id: ConnectionId, cell: &Rc<RefCell<ConnCell<I::Suite>>>) {
        let Some(record) = self.conns.get_mut(&id) else {
            return;
        };

        // The profile is read here, off the live core, because
        // `Connection::new` is total and takes it as an argument — the
        // constructor must not borrow a possibly-released core (ruling 286).
        let (anchor, timing_profile) = {
            let borrow = cell.borrow();
            let live = borrow.core.as_ref().and_then(|core| {
                core.session()
                    .map(|session| (session.anchor, core.timing_profile()))
            });
            match live {
                Some(pair) => pair,
                None => {
                    debug_assert!(false, "ConnEvent::Established without an installed session");
                    return;
                }
            }
        };
        cell.borrow_mut().remote_address = anchor;

        // §5.4's PENDING → LIVE is not written here: ruling 90 leaves the
        // static map in the core, and `complete_initiation` promoted the
        // entry in the same call that emitted the `Install` this event
        // followed.

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
            timing_profile,
        );
        resolve_slot(&slot, PendingOutcome::Ready(handle));
    }

    /// §5.5's give-up, or ruling 72's local failure: resolve the
    /// `Connecting` and forget the connection that never was.
    ///
    /// No `Retired` is synthesised. §16.4's `HandshakeFailed` is the
    /// endpoint core telling us it has *already* dropped the pending, and
    /// no session was ever installed, so there is no index route and no
    /// guard pin left to release (rulings 81/84's "no `Retired` at all"
    /// case). `drop_pending` released the static in the same call, which is
    /// the whole of §5.4's PENDING → NONE after ruling 90.
    fn fail_pending(&mut self, id: ConnectionId, error: ConnectError) {
        let Some(record) = self.conns.remove(&id) else {
            return;
        };
        if let Some(slot) = record.slot {
            resolve_slot(&slot, PendingOutcome::Failed(error));
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
            // §5.4's LIVE → NONE is the core's, on the `Retired` this pass
            // has already delivered (see the module docs' ordering note);
            // ruling 90 leaves no shell-side copy to release here.
            //
            // The core is released here, and only here. A handle that
            // calls `close()` afterwards finds `None` and does nothing —
            // the same answer the core itself would have given.
            record.cell.borrow_mut().core = None;
            if let Some(slot) = record.slot {
                resolve_slot(&slot, PendingOutcome::Failed(ConnectError::Local));
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
            let cell = Rc::clone(&record.cell);
            {
                let mut borrow = cell.borrow_mut();
                if let Some(core) = borrow.core.as_mut() {
                    core.handle_datagram(now, src, datagram);
                }
                borrow.dirty = true;
            }
            // §12's ACK was applied inside that call. A `Connection::acked()`
            // snapshot can be settled by it with **no** `ConnEvent` behind
            // it — see [`wake_settled`], which is where the enumeration
            // lives.
            wake_settled(&cell);
        }
    }

    fn handle_timeout(&mut self) {
        let now = now();
        // **F1's detector.** Recorded so `deadline` can tell an *overdue*
        // timer — legitimate, and the ordinary case after the clock advances
        // while this task is parked — from one the firing itself re-armed in
        // the past, which is ruling 141's spin and §16.5's idempotence
        // failing at once. Cleared by every other event below.
        self.last_timeout = Some(now);
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
    /// announcement".
    ///
    /// # The spin detector, and why it is not `deadline >= now` (**F1**)
    ///
    /// A core that announces a deadline **already passed** makes step 4's
    /// `sleep_until` complete immediately, `handle_timeout` re-fire the same
    /// timer, and the actor spin at 100 % of one core. **Clamping to `now`
    /// here would change nothing**: `sleep_until(max(d, now))` completes
    /// immediately for exactly the same set of deadlines, so the loop runs
    /// at the same rate — it would look like a second-line defence and be a
    /// no-op. An assertion is the thing that pays. A spin is invisible on
    /// the wire (ruling 141) and unreachable from `FlakyWire`, which models
    /// a network and not a CPU (working rule 13), so the one function every
    /// drain passes through is where the whole class becomes a loud failure
    /// — including for the instances nobody has found yet.
    ///
    /// # It is an `assert!`, not a `debug_assert!` (**[RATIFIED 2026/08/17
    /// — ruling 249(ii)]**)
    ///
    /// It shipped as a `debug_assert!`, which left the class **silent in
    /// release** — precisely where it costs: this driver is §16.3's one
    /// `!Send` actor serving *every* connection on the endpoint, so one
    /// connection announcing a past deadline burns 100 % of a core for all
    /// of them, with nothing red anywhere and nothing on the wire.
    ///
    /// The promotion is ordered **after** ruling 249(i), and the order is
    /// load-bearing. 249(i) gates the `Pto` announcement on §7.3's budget;
    /// before it landed, the one known-reachable member of this class was a
    /// budget-suppressed saturated PTO, and promoting first would have
    /// panicked release drivers in a state the protocol itself produced.
    /// With (i) in, that state no longer announces, and its re-arm cannot
    /// trip this either: the budget grows only on a *receive*, so the
    /// re-armed deadline is always announced on a pass where `last_timeout`
    /// is `None`.
    ///
    /// What remains trippable is an **unknown** member of ruling 141's spin
    /// class, and there the alternative to a clean panic is the shared
    /// driver spinning silently forever.
    ///
    /// **`CONTRACT-7b.md` §4.2 specifies that assertion as
    /// `debug_assert!(deadline >= now)`, and that predicate is too strong.**
    /// It fires on a **correct** state and does so on an existing test:
    /// `sd6_a_lost_datagram_is_never_retransmitted_and_never_blocks`
    /// announces a `Pto` 1.974 s in the past — verified identical at this
    /// slice's base commit, so it predates every change here. That deadline
    /// is `last_ack_eliciting + pto`, and when the clock advances while the
    /// driver is parked it is **already past the first time it is computed**.
    /// The driver's very next act is to fire it; a probe leaves, the deadline
    /// moves forward, and nothing spins. *Overdue is not spinning.* A
    /// spinning core does announce a past deadline, but the converse does not
    /// hold, and asserting the converse is working rule 12's *true lemma
    /// about the wrong state*.
    ///
    /// What ruling 141's class actually is: a timer **the firing itself
    /// re-arms in the past**. §16.5 already forbids exactly that, from the
    /// other side — `handle_timeout` is idempotent, *"every due deadline is
    /// stopped before its logic runs, so a repeated call at one instant finds
    /// an empty due set"* — and a deadline still due at the instant the
    /// timeout pass just ran is that empty set being non-empty. So the
    /// detector below keys on [`last_timeout`](Self::last_timeout): it fires
    /// only when the previous loop event was a timer firing at `t` and a core
    /// is still announcing a deadline at or before `t`. No threshold, no
    /// tolerance, and no false positive from an overdue timer.
    ///
    /// # It reads, and does not pop (**[RATIFIED 2026/08/18 — ruling 262]**)
    ///
    /// This used to collect deadlines with `poll_output()`, whose terminal
    /// `Timeout` is §16.4's announcement — but `poll_output()` **pops**, so
    /// the two `_` arms it needed did not merely mis-report a deadline, they
    /// *destroyed* whatever the core had queued, which for a connection core
    /// is a datagram. They were `debug_assert!(false)` plus `None`: silent in
    /// release, and in debug a panic on the `spawn_local` task whose
    /// `JoinHandle` the shell drops — so **neither profile went red**, and
    /// the arms were a data-loss site rather than the guard they read as.
    ///
    /// The precondition was documented as *"no yield since the drain"*, and
    /// that is not the invariant. [`serve`](Self::serve)'s **post-drain
    /// tail** — `release_dead`, `prune_ready`, `prune_waiting`,
    /// `dispatch_intros` — runs after the last `dirty` scan and calls into
    /// **consumer** code with no yield at all: `dispatch_intros` sends an
    /// `Intro` down a `oneshot`, `release_dead` resolves a `Connecting` slot
    /// and drops a `PublicKeyOf<I>`. A consumer whose executor polls inline
    /// rather than queueing — the case [`latch`](Self::latch),
    /// [`resolve_slot`](super::shared::resolve_slot) and `poll_recv_message`
    /// each name, and each release their borrow for, which is exactly what
    /// lets the reentrant call *succeed* — re-enters the data path there and
    /// queues output that nothing drains before this runs. The regression
    /// test is `reentrant_consumer_write_in_the_post_drain_tail_is_not_destroyed`
    /// in this file's test module; it was red in **both** profiles.
    ///
    /// So the collection is a **pure read**: `next_deadline` announces the
    /// same value without consuming anything, and there is no `_` arm left
    /// to assert about. A core holding an undrained output announces its
    /// correct next deadline and *keeps* the output, which the
    /// `Command::Dirty` its mutator sent brings the driver back for on the
    /// very next turn — the `biased` command arm of step 4's `select!`.
    ///
    /// The call **stays** at [`run`](Self::run) step 2 even though it no
    /// longer has to: before the send is still the freshest value, and the
    /// discipline now costs nothing. `spec_shell.rs`'s
    /// `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer`
    /// no longer depends on the position.
    fn deadline(&self) -> Option<std::time::Instant> {
        let endpoint = self.shell.state.borrow().endpoint.next_deadline();

        self.conns
            .values()
            .filter_map(|record| record.cell.borrow().core.as_ref()?.next_deadline())
            .chain(endpoint)
            .min()
            .inspect(|announced| {
                // **[ruling 255, amending 249(ii)]** Still an `assert!` — a
                // release driver spinning silently is worse than a panic
                // that names the connection's core — but it trips on the
                // **third consecutive** overdue announce, not the first.
                // Measured (slice R40-C's finding, reproduced at
                // integration): a `Loss` firing that marks a lost DATAGRAM
                // retransmits nothing, while the ACK that revealed the gap
                // has already reset `pto_count` and shrunk the `Pto`
                // deadline ~4 ms into the past — the next firing sends the
                // probe and the deadline advances. One overdue step with
                // progress behind it is the "overdue is not spinning" case
                // the doc above always named; 249(ii) asserted the
                // converse one level up. A genuine spin re-fires
                // immediately and forever, so the streak reaches 3 in
                // virtual-zero time; every benign chain measured or
                // constructed is length ≤ 2. If a legitimate 3-chain ever
                // appears, the threshold moves, not the mechanism.
                if let Some(fired_at) = self.last_timeout {
                    if *announced <= fired_at {
                        let streak = self.overdue_streak.get() + 1;
                        self.overdue_streak.set(streak);
                        assert!(streak < 3, "{PAST_DEADLINE}");
                    } else {
                        self.overdue_streak.set(0);
                    }
                }
            })
    }

    // ═══════════════════════════════════════════════════════════════════
    // Commands
    // ═══════════════════════════════════════════════════════════════════

    fn handle_command(&mut self, command: Command<I>) {
        match command {
            Command::Connect {
                id,
                core,
                remote,
                remote_static,
                slot,
            } => self.command_connect(id, *core, remote, remote_static, slot),
            Command::Cancel(id) => self.command_cancel(id),
            Command::Accept(reply) => {
                self.waiting.push_back(reply);
                self.dispatch_intros();
            }
            Command::ReadIdentity(id, reply) => {
                let result = self
                    .shell
                    .state
                    .borrow_mut()
                    .endpoint
                    .read_identity(now(), id);
                drop(reply.send(result));
            }
            Command::Authenticate(id, psk, reply) => {
                let result = self
                    .shell
                    .state
                    .borrow_mut()
                    .endpoint
                    .authenticate(now(), id, &psk);
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

    /// §16.2's `connect()`'s **second half** — §6.1's two initiator DH, on
    /// the driver task where §6.2 requires them (rulings 87 and 90).
    ///
    /// Everything cheap already happened, synchronously, in the verb itself:
    /// `mint_pending` answered §16.1's NONE/PENDING/LIVE test out of the
    /// endpoint core's own map, minted `id` and built the connection core.
    /// There is no `Err` arm here because there is no second admission test
    /// to fail — that is the whole of what deleting the mirror bought.
    ///
    /// # A cancel that overtook this command spends nothing
    ///
    /// On a paused clock `connect(); drop(connecting)` runs to completion
    /// before the driver is scheduled, leaving `[Connect(id), Cancel(id)]`
    /// in the queue. Ruling 50's cancel already retired the pending in the
    /// core — synchronously, in `Connecting::drop` — so `start_attempt`
    /// finds nothing under `id`, returns, and **no msg1 reaches the wire
    /// and no DH is spent**. Before ruling 90 this arm called
    /// `core::Endpoint::connect`, which built and sent msg1 unconditionally
    /// and left the `Cancel` behind it to undo an attempt the peer had
    /// already seen.
    ///
    /// The record is still inserted, and deliberately: the `Cancel` behind
    /// us removes it, and an early return here would leave the two arms
    /// disagreeing about whether it exists.
    fn command_connect(
        &mut self,
        id: ConnectionId,
        core: CoreConnection<I::Suite>,
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
        slot: Rc<RefCell<PendingSlot<I>>>,
    ) {
        let cell = Rc::new(RefCell::new(ConnCell::new(core, remote)));
        self.conns.insert(
            id,
            ConnRecord {
                cell,
                remote_static,
                slot: Some(slot),
            },
        );

        // §6.1's `es` + `ss`, and §5.5's first initiation on the wire.
        self.shell
            .state
            .borrow_mut()
            .endpoint
            .start_attempt(now(), id);
    }

    /// Ruling 50: a `Connecting` was dropped — the driver's half of it.
    ///
    /// # The core-side retirement is **not** here, and after ruling 90 it
    /// cannot be
    ///
    /// §16.4's API list has no cancel verb, and `ToEndpoint::Retired` is the
    /// core-side effect S29 needs — the endpoint core's own docs name it
    /// "S29's cancellation path": it stops §5.5's retransmit train, frees
    /// the pending index (§17.3), takes the dialled address out of §6.5's
    /// hint set (§17.4), releases the §17.1 pin, and frees the static so the
    /// very next `connect()` succeeds. `Connecting::drop` delivers it
    /// **synchronously**, in the instant the handle dies.
    ///
    /// It has to. Ruling 50's MUST is that the cancellation is ordered ahead
    /// of any endpoint verb issued after the drop returns, *with no advance
    /// of the clock between them* — and after ruling 90 the redial's
    /// admission test is `mint_pending` reading the core's own static map.
    /// A `Retired` delivered here, one command later, would be a redial
    /// answered `AlreadyConnected` by a map still holding the corpse. The
    /// mirror used to absorb that; there is no mirror.
    ///
    /// So what is left for this command is the driver's own bookkeeping —
    /// the `ConnRecord` — plus the wake that makes the loop recompute its
    /// deadlines with the pending gone.
    ///
    /// # §16.4's MUST still holds, and more strongly than before
    ///
    /// §16.4: "the shell delivers `Retired` to `handle_connection_event`
    /// **before** releasing the connection's shell-side bookkeeping (else
    /// the index route and the guard-entry pin leak for the endpoint's
    /// life)." The two are now in different tasks' turns rather than two
    /// statements, and the order between them is guaranteed by the channel:
    /// the drop delivers `Retired` and *then* sends this command.
    fn command_cancel(&mut self, id: ConnectionId) {
        self.conns.remove(&id);
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
        // Read before the core moves into the cell: `Connection::new` is
        // total and must not borrow the cell for this (ruling 286).
        let timing_profile = core.timing_profile();
        let cell = Rc::new(RefCell::new(ConnCell::new(core, anchor)));

        // §5.4's NONE → LIVE was written by `core::Endpoint::accept` itself,
        // in the call above. Ruling 90 leaves that map in the core, so there
        // is nothing to mirror here — and no second stamp to keep honest.

        let handle = Connection::new(
            Rc::clone(&self.link),
            Rc::clone(&cell),
            conn_id,
            remote_static.clone(),
            session_id,
            timing_profile,
        );
        self.conns.insert(
            conn_id,
            ConnRecord {
                cell,
                remote_static,
                slot: None,
            },
        );

        // If the `accept()` future was cancelled, this `Connection` is
        // dropped here — the last handle to it — and §16.2's
        // `close(NO_ERROR, "")` follows, which is the right answer for a
        // connection nobody claimed.
        drop(reply.send(Ok(handle)));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Stopping
//
// A separate impl block because [`Drop`] may not carry bounds the struct
// does not, and the block above needs `I: 'static` for the erased
// `Rc<dyn ShellLink>`. Nothing here does.
// ═══════════════════════════════════════════════════════════════════════

impl<I: Identity, W: Wire> Driver<I, W> {
    /// Ruling 46's latch: set once, read for ever.
    ///
    /// §16.4 emits `Closed` exactly once per connection and the core
    /// `debug_assert!`s it, so the `is_none()` guard here is defence for the
    /// one place a second value would be observable — [`stop`], which
    /// latches `EndpointDropped` over connections that may already have
    /// died.
    ///
    /// # The wakers are woken with the borrow released
    ///
    /// `Waker::wake` runs the **consumer's** executor, and an executor that
    /// polls a ready task inline rather than queueing it re-enters
    /// `Connection::poll_closed`, which takes this same `borrow_mut`. That
    /// is `already mutably borrowed` inside the driver task — from a
    /// consumer doing nothing wrong, on a `Waker` this crate does not
    /// supply. So the borrow ends first and
    /// [`Wakers::take_all`](super::shared::Wakers::take_all), which is
    /// `#[must_use]`, is what makes that the only spelling available.
    ///
    /// [`stop`]: Self::stop
    fn latch(cell: &Rc<RefCell<ConnCell<I::Suite>>>, lost: ConnectionLost) {
        let woken = {
            let mut borrow = cell.borrow_mut();
            if borrow.closed.is_some() {
                return;
            }
            borrow.closed = Some(lost);
            let mut woken = borrow.closed_wakers.take_all();
            // `notification_wakers` is taken here, beside `closed_wakers`
            // and for the same reason: a `notified()` parked with every slot
            // empty can only ever resolve from the latch, so nothing else
            // would release it. It is **not** in
            // `take_all_stream_wakers` — a notification survives the death
            // and the slots outlive it, so the sweep that frees the stream
            // verbs is the wrong place to say so.
            woken.extend(borrow.notification_wakers.take_all());
            // §16.8's four stream maps sweep here too, and this is the only
            // thing that wakes a cell-parked stream waiter on **driver**
            // death: [`stop`] calls this, and `Drop for Driver` calls
            // `stop` on an unwind. Without it a writer parked on
            // flow-control credit parks for ever when the driver panics —
            // which no network fixture can produce, because a fixture that
            // loses, delays, duplicates and reorders cannot express "this
            // driver stopped".
            woken.extend(borrow.take_all_stream_wakers());
            woken
        };
        for waker in woken {
            waker.wake();
        }
    }

    /// The driver has stopped (§16.3): every session dies silently, and
    /// **nothing is transmitted** (§15.4's endpoint-dropped row).
    ///
    /// # Every waiter resolves here, and that is the whole point
    ///
    /// This runs on an **unwind** as well as on the ordinary exit (see the
    /// [`Drop`] impl below), so it is the only thing standing between a
    /// panicking driver and an endpoint that is frozen but still reports
    /// itself healthy. Two seams have to be closed, not one, because §16.3
    /// splits the handle surface by cost (ruling 53):
    ///
    /// * The `oneshot`-backed verbs — `accept()` and the three staged verbs
    ///   — close themselves: their senders die with the [`Driver`], `rx.await`
    ///   errors, and each verb has a defined answer for that
    ///   (`EndpointDropped`, or `None` for `accept()`). Nothing extra is
    ///   needed and nothing here may get in their way.
    /// * The **cell-backed** waiters do not. `Connection::closed()` parks in
    ///   `closed_wakers` and a `Connecting` parks in its [`PendingSlot`];
    ///   both are woken only by driver-side code, so without the two sweeps
    ///   below they park for ever while `is_established()` keeps answering
    ///   `true` from a cell nobody will write again.
    ///
    /// A `Connecting` can be parked in either of two places, and both are
    /// swept:
    ///
    /// 1. its `Command::Connect` was processed, so the slot is in a
    ///    [`ConnRecord`] — resolved with the records;
    /// 2. its `Command::Connect` is **still in the channel** — the driver
    ///    never saw it, so no record names it and only the queue does. That
    ///    is why the channel is drained rather than merely dropped: dropping
    ///    the receiver frees the slot's `Rc` but leaves the `Connecting`'s
    ///    own copy parked with nobody to wake it.
    ///
    /// `ConnectError::Local` is the answer in both cases, for ruling 62's
    /// reason: `ConnectError` deliberately has no `EndpointDropped`, and a
    /// failure with no DH spent on the caller's behalf is what `Local`
    /// names — the same value `Endpoint::connect` already returns when it
    /// finds `driver_stopped` set.
    ///
    /// Idempotent: `latch` is guarded, `driver_stopped` is a set-once flag,
    /// and both collections are cleared.
    fn stop(&mut self) {
        self.shell.state.borrow_mut().driver_stopped = true;

        // Nothing further can be queued; anything already queued is
        // answered below rather than silently dropped.
        self.commands.close();
        while let Ok(command) = self.commands.try_recv() {
            if let Command::Connect { slot, .. } = command {
                resolve_slot(&slot, PendingOutcome::Failed(ConnectError::Local));
            }
        }

        for record in self.conns.values_mut() {
            Self::latch(&record.cell, ConnectionLost::EndpointDropped);
            record.cell.borrow_mut().core = None;
            if let Some(slot) = record.slot.take() {
                resolve_slot(&slot, PendingOutcome::Failed(ConnectError::Local));
            }
        }

        // Dropping the senders is what makes a parked `accept()` resolve
        // `None` — §16.2's "None = endpoint closed".
        self.waiting.clear();
        self.conns.clear();
    }
}

/// **The stop path, on the ordinary exit and on an unwind alike.**
///
/// [`Driver::run`] does not call [`stop`](Driver::stop) itself; this does,
/// on the one line where `self` goes away, so there is exactly one stop
/// path and no `break` or `?` added later can skip it.
///
/// It is a `Drop` rather than a `catch_unwind` because `catch_unwind` would
/// need `AssertUnwindSafe` over a `!Send`, `!UnwindSafe` actor holding an
/// `Rc<RefCell<_>>` — an assertion this code is in no position to make —
/// whereas `Drop` runs during unwinding by construction. **The panic is not
/// swallowed**: it keeps propagating out of the task exactly as before.
///
/// What changes is what it leaves behind. Before this impl, a panic
/// anywhere in the driver skipped the stop entirely: `driver_stopped` stayed
/// `false`, so `Endpoint::connect` kept handing out `Connecting`s that could
/// never resolve; every `closed()` future and every in-flight `Connecting`
/// parked for ever; `is_established()` went on answering `true` from a cell
/// nobody would write again; and `tokio::task::spawn_local` stored the panic
/// in a `JoinHandle` the shell drops, so **nothing** surfaced — the test
/// harness prints `ok` over a frozen endpoint. Now the same panic degrades
/// to `ConnectionLost::EndpointDropped` and `ConnectError::Local`, which are
/// answers an application can act on.
///
/// It also runs when the `Driver` future is dropped without ever being
/// polled — a `LocalSet` dropped out from under it — which is the same
/// state by a different route and deserves the same answer.
impl<I: Identity, W: Wire> Drop for Driver<I, W> {
    fn drop(&mut self) {
        self.stop();
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

/// What [`Driver::deadline`]'s detector says when it fires.
///
/// A `const` so the string is written once and so a test that provokes the
/// class can name what it expects to see.
///
/// **[ruling 249(ii), threshold by ruling 255]** The detector is an
/// `assert!` tripping on the third consecutive overdue announce, so this is
/// a *release* panic message as well as a debug one — it is what an
/// operator sees, not only what a test matches on.
const PAST_DEADLINE: &str = "a core announced a deadline in the past — ruling 141's spin class: `sleep_until` \
     completes at once, `handle_timeout` re-fires, and the actor spins";

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

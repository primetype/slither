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
    Command, ConnCell, PendingOutcome, PendingSlot, Shell, ShellLink, Wakers, now, resolve_slot,
    wake_settled,
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
            // 1. Everything the cores want done. This is where §16.4's
            //    drain contract is discharged.
            let outgoing = self.serve();

            // 2. The next deadline, read **here** — after the drain and
            //    before the first yield of the iteration.
            //
            //    §16.4 makes `poll_output()`'s terminal `Timeout`
            //    "simultaneously the drain sentinel and the next-deadline
            //    announcement", and `poll_output` **pops**. So reading a
            //    deadline is a pure read only while the queue is provably
            //    empty, and the only thing that establishes that is a drain
            //    with no yield after it. `transmit()` below is a yield —
            //    `Wire::send_to` is an application-supplied `async fn`, and
            //    a real socket returns `Pending` whenever its send buffer
            //    is full — and §16.3 (ruling 53) puts `close()` on the
            //    *handle* side of the seam, so a `close()` landing during
            //    that yield queues a `Transmit` on a core the driver has
            //    already drained. Reading the deadline after the yield
            //    popped that datagram and discarded it: a silently lost
            //    CLOSE and 25 s of `DEAD_TIMEOUT` for the peer.
            //
            //    Reading it here cannot go stale in a way that matters:
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
    /// cores still dirty and the endpoint core undrained — and `deadline`
    /// **pops**, so an undrained `Transmit` sitting there is destroyed
    /// rather than sent. A silent exhaustion therefore degrades to a lost
    /// CLOSE and 25 s of `DEAD_TIMEOUT` for the peer, which is exactly the
    /// failure the `deadline` docs describe.
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
            ConnEvent::StreamReset { r, .. } => {
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
    /// # It must be called with no yield since the drain
    ///
    /// `poll_output()` **pops**, so the `_` arms below do not merely
    /// mis-report a deadline — they *destroy* whatever the core queued,
    /// which for a connection core is a datagram. This function has no way
    /// to establish that the queues are empty; only its **caller's
    /// position** does, and [`run`](Self::run) step 2 is that position and
    /// says why. Moving this call after `transmit().await` is what the
    /// regression test
    /// `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer`
    /// (`tests/spec_shell.rs`) exists to catch.
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

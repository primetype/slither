//! §16.2's `Connection` handle — the subset slice 3 builds.
//!
//! `close()`, ruling 46's `closed()`, the four accessors, slice 4's stream
//! verbs, slice 5's `acked()` and slice 6's four sugar verbs. The §16.2
//! verbs still absent — `notified`, `set_persistent_keepalive` — belong to
//! slice 7 and are **absent rather than stubbed**: in this module tree an
//! unimplemented verb is a claim about the protocol, and an
//! `unimplemented!()` on a public surface is a worse claim than an absence.
//!
//! # Where the work happens
//!
//! §16.3 (ruling 53) puts the connection data path on the **shared-cell**
//! side of the seam: the handle borrows the connection's
//! `Rc<RefCell<ConnCell>>`, calls the sans-io core directly — which seals
//! synchronously (§16.7) — marks the cell dirty and wakes the driver, which
//! drains `poll_output()` to `Timeout` and performs the I/O. Each verb is
//! written **once**, as `poll_*(&self, cx, …) -> Poll<_>`; the `async fn`
//! §16.2 declares is `poll_fn` over it.

use std::cell::RefCell;
use std::future::poll_fn;
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::constants;
use crate::core::connection::{AckSnapshot, SendMessage};
use crate::core::{Connection as CoreConnection, ConnectionId, Dir, StreamRef};
use crate::error::{ConnectionLost, DatagramError, MessageError};
use crate::packet::{Channel, Handshake};

use super::shared::{ConnCell, ShellLink, WakerSlot, close_now, now};
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
/// peer's cost is bounded at `DEAD_TIMEOUT` (25 s), which that row already
/// accepts.
///
/// This is documentation obligation #4 and it is drop-order sensitive: it
/// is the opposite of the obvious guess, and which of the two rules fires
/// depends on the order your values fall out of scope. Keeping an
/// [`Endpoint`](super::Endpoint) alive across the drop is what makes the
/// CLOSE happen.
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
}

impl<S: Handshake> Connection<S> {
    pub(crate) fn new(
        shell: Rc<dyn ShellLink>,
        cell: Rc<RefCell<ConnCell<S>>>,
        id: ConnectionId,
        remote_static: PublicKeyFor<S>,
        session_id: hiss::noise::SessionId,
    ) -> Self {
        shell.acquire();
        cell.borrow_mut().handles += 1;
        Self {
            shell,
            cell,
            id,
            remote_static,
            session_id,
        }
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
    /// # In slice 4 an exhausted bidi space parks for ever
    ///
    /// A bidi index is returned to the peer's allowance only when **both**
    /// halves are freed, and the send half is freed on acknowledgement —
    /// which needs §12's ACK processing, which this slice does not have. So
    /// once the bidi limit is reached, `open_bi` never resumes. `open_uni`
    /// has no such gap: a peer-opened uni stream read to end-of-stream is
    /// fully closed at once and grants its MAX_STREAMS_UNI.
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
    /// (ruling 133). A receiver killed by `DEAD_TIMEOUT` mid-transfer
    /// cannot drain, which is honest: a path that produced no CLOSE
    /// produced no finished sender either. After a **local** `close()` the
    /// window is the linger as well — see [`RecvStream::read`] for the one
    /// row of it ruling 133 expects to change.
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
                return Err(DatagramError::ConnectionLost(ConnectionLost::EndpointDropped));
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
        let outcome = {
            let mut cell = self.cell.borrow_mut();
            // Every call — `Some` or `None` — can have run §9.8's overflow
            // scan and emitted a RESET_STREAM, and a claim retires a half
            // that owes MAX_DATA and MAX_STREAMS. So the cell is dirtied on
            // the strength of the *call*, not of the answer.
            let claimed = cell.core.as_mut().map(|core| core.recv_message(now()));
            if claimed.is_some() {
                cell.dirty = true;
            }
            match claimed {
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
            }
        };
        // Outside the borrow (finding F10): waking and driving both re-enter
        // the cell.
        self.shell.mark_dirty(self.id);
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
    /// Slice 7's roaming (§7.3) is what makes this change; in this slice it
    /// is fixed for the connection's life.
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

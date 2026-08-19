//! §16.2's three stream handles — `SendStream`, `RecvStream`, `BiStream`.
//!
//! Where these sit in the shell, and what they cost, are in [the shell
//! module docs](super); the [`AsyncRead`]/[`AsyncWrite`] impls over them are
//! [`crate::compat::io`].
//!
//! What is here is the shared-cell data path. Each verb is written
//! **once**, as `poll_*(&mut self, cx, …) -> Poll<_>`; the `async fn` §16.2
//! declares is `poll_fn` over it, and `AsyncWrite` is the same function with
//! its error mapped. That is why the waker key is a **field** rather than a
//! per-future `WakerSlot`: `AsyncWrite::poll_write(self: Pin<&mut Self>, cx,
//! buf)` has no argument to carry one.
//!
//! [`SendStream::acked`] (ruling 122b) resolves on
//! `ConnEvent::StreamFinished`, which is §9.7's `DataRecvd` — it needs §12's
//! ACK processing, and nothing below that layer can reach it.
//!
//! [`AsyncRead`]: https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html
//! [`AsyncWrite`]: https://docs.rs/tokio/latest/tokio/io/trait.AsyncWrite.html

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::constants;
use crate::core::{ConnectionId, StreamId, StreamRef};
use crate::error::{ConnectionLost, ReadError, WriteError};
use crate::packet::Handshake;

use super::shared::{ConnCell, ShellLink, close_now, now, release_waker_slot, wake_settled};

/// The terminal outcome a [`RecvStream`] latches — **ruling 121**.
///
/// The core is honest per call and **not** sticky: `Streams::read` retires
/// the receive half *and then* returns `Err(Reset(code))`, so the next
/// `read` on that `StreamRef` finds no half and answers `Ok(None)` — which
/// §16.2 documents as "FIN reached, all data delivered". That is false of a
/// stream whose data §9.6 abandoned, and an application that logs the reset
/// and retries its loop would read a clean end-of-stream: **data loss
/// presented as success**. The handle latches instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// The FIN's final size was reached and every byte was delivered.
    Eof,
    /// The peer sent RESET_STREAM with this code (§9.6).
    Reset(u64),
}

/// Read the cached wire id, filling the cache from the core on first sight
/// (**ruling 116**).
///
/// The cache is not an optimisation. `core::Connection::stream_id` is **not
/// monotone**: `Streams::after_half_freed` removes the entry, so an uncached
/// `id()` answers `None` again once a stream fully closes — reachable in
/// slice 4 for a peer-opened uni stream read to EOF. That would be the
/// opposite of [`Connection::remote_static`]'s keeps-answering property, for
/// the same reason.
///
/// [`Connection::remote_static`]: super::Connection::remote_static
fn cached_id<S: Handshake>(
    cell: &RefCell<ConnCell<S>>,
    r: StreamRef,
    cache: &Cell<Option<StreamId>>,
) -> Option<StreamId> {
    if let Some(id) = cache.get() {
        return Some(id);
    }
    let id = cell.borrow().core.as_ref()?.stream_id(r)?;
    cache.set(Some(id));
    Some(id)
}

// ═══════════════════════════════════════════════════════════════════════
// SendStream
// ═══════════════════════════════════════════════════════════════════════

/// The send half of a stream (§16.2).
///
/// **Not `Clone`, and the uniqueness is load-bearing.** Exactly one
/// `SendStream` exists per stream, which is what makes this handle the sole
/// owner of its §16.8 waker-map entry and the sole author of its half's
/// retirement.
///
/// # Dropping it resets the stream
///
/// §16.2: *dropping a `SendStream` **without `finish()`** resets it with
/// error code 0*. A handle on which [`finish`](Self::finish) or
/// [`reset`](Self::reset) has already been called drops silently — see the
/// `Drop` impl, where the distinction is the difference between a clean EOF
/// at the peer and `ReadError::Reset(0)` over destroyed data.
///
/// # It is a handle for the driver's lifetime — **ruling 115**
///
/// Holding a `SendStream` keeps the driver alive exactly as holding a
/// [`Connection`](super::Connection) does, and dropping the last handle of
/// *any* kind to a connection performs §16.2's `close(NO_ERROR, "")`.
/// Ruling 62's test decides it: a value that changes protocol state when
/// dropped is a handle, and this one puts RESET_STREAM on the wire. The
/// consequence that makes it necessary is that **a `Drop` which must emit a
/// frame needs a driver to emit it**: under the alternative, dropping the
/// last `Connection` would fire `close(NO_ERROR, "")` underneath a live
/// `SendStream` and that stream's RESET_STREAM would be silently lost — in
/// the ordinary shape of a task that owns a stream and has let the
/// connection handle go.
///
/// Ruling 88 still governs the genuinely-last drop in the process: nothing
/// is transmitted.
pub struct SendStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    /// This handle's slot in `ConnCell::blocked_writers[r]`.
    key: u64,
    /// This handle's slot in `ConnCell::blocked_ackers[r]` — ruling 47's
    /// [`acked`](Self::acked).
    ///
    /// A second key rather than a shared one: the two maps are woken by
    /// different events (`StreamWritable` versus `StreamFinished`), and one
    /// key in two maps would make a cancelled `write()` evict a live
    /// `acked()`'s waker.
    ack_key: u64,
    /// This half's terminal state — **ruling 124 step 1**, which answers
    /// ahead of the connection's death latch.
    ///
    /// It distinguishes `Finished` from `Reset` because the two have
    /// *different* terminal answers for `finish()`: a second `finish()` is
    /// idempotent `Ok(())`, while a `finish()` after `reset()` is
    /// `Err(Finished)`. A `bool` conflates them and forces `poll_finish`
    /// to fall through to the core, which then answers `ConnectionLost`
    /// once the connection has died — the very inconsistency ruling 124
    /// closes.
    local_end: LocalEnd,
    id: Cell<Option<StreamId>>,
}

/// A send half's own terminal state, which outranks the connection's
/// (ruling 124).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LocalEnd {
    /// Neither `finish()` nor `reset()` has been called on this handle.
    Live,
    /// `finish()` succeeded: the FIN is in send state.
    Finished,
    /// `reset()` was called, before or after a `finish()`, with the code it
    /// carried.
    ///
    /// The code is held because [`SendStream::acked`] reports it
    /// (§16.2:4425 — *"it returns `Reset(code)` if the stream was reset
    /// before its data was acknowledged"*), and the core cannot supply it
    /// afterwards: `SendHalf::take_reset` hands the code to the frame
    /// layer once, and the half is freed outright at `ResetRecvd`.
    Reset(u64),
}

impl<S: Handshake> SendStream<S> {
    /// Build a handle over an already-borrowed cell.
    ///
    /// It takes `&mut ConnCell<S>` rather than re-borrowing `cell` because
    /// §4.4's rule — *never call `core.open()` or `core.accept()` without
    /// constructing the handle in the same expression* — requires the core
    /// call and this constructor to run under **one** borrow. A constructor
    /// that took the `RefCell` would panic on the second `borrow_mut`, and
    /// splitting the two would open the window in which an index is spent on
    /// a stream no handle names.
    pub(crate) fn install(
        shell: Rc<dyn ShellLink>,
        cell_rc: Rc<RefCell<ConnCell<S>>>,
        cell: &mut ConnCell<S>,
        conn: ConnectionId,
        r: StreamRef,
    ) -> Self {
        shell.acquire();
        cell.handles += 1;
        let key = cell.blocked_writers.entry(r).or_default().key();
        // Created here and not on first park, exactly like the writers'
        // slot: the entry's presence is what tells the driver a handle
        // exists that could ask about this stream, which is what bounds
        // `ConnCell::finished_senders`.
        let ack_key = cell.blocked_ackers.entry(r).or_default().key();
        Self {
            shell,
            cell: cell_rc,
            conn,
            r,
            key,
            ack_key,
            local_end: LocalEnd::Live,
            // **Filled eagerly, not lazily — ruling 143.** Ruling 116 said
            // "cached the first time the core answers `Some`", which is too
            // late: §12's ACK can fully close a locally-opened stream, and
            // `Streams::after_half_freed` then removes the entry, so a
            // handle whose *first* `id()` call happens after that answers
            // `None` for ever. Every handle is constructed while its stream
            // exists, so there is exactly one instant at which the answer is
            // guaranteed available, and this is it.
            id: Cell::new(cell.core.as_ref().and_then(|c| c.stream_id(r))),
        }
    }

    /// This stream's wire id (§9.1), or `None` before one exists.
    ///
    /// **Cached, and it keeps answering** — after the stream closes and
    /// after the connection dies, like
    /// [`Connection::remote_static`](super::Connection::remote_static).
    ///
    /// In wire v1 an application-held handle's id is **always `Some`**: the
    /// shell publishes no pre-establishment `Connection` (ruling 116), so
    /// there is no route to a handle whose parity is unfixed. The `Option`
    /// is retained deliberately — removing it is a breaking change to a
    /// ratified surface to save a `match`, and re-adding it when a later
    /// line publishes a pre-establishment route would be breaking again.
    pub fn id(&self) -> Option<StreamId> {
        cached_id(&self.cell, self.r, &self.id)
    }

    /// Write as much of `buf` as flow control allows (§10.1).
    ///
    /// **This is not `write_all`.** A short write is normal and the caller
    /// must loop:
    ///
    /// * `Ok(n)` with `n >= 1` — `n` bytes were accepted into send state and
    ///   **already sealed** (§16.7, ruling 114: a mutating call flushes
    ///   everything the ledger admits). `n <= buf.len()`. Those `n` bytes are
    ///   committed; a caller that treats a short return as a failure and
    ///   retries the whole buffer **duplicates data**.
    /// * `Ok(0)` — **exactly one meaning: `buf` was empty.** Never
    ///   "blocked", never end-of-stream. The check happens before the core is
    ///   touched (ruling 110), because the core's own `Ok(0)` is overloaded
    ///   and a shell that forwarded an empty write would park for ever on a
    ///   buffer of its own making.
    /// * pending — `buf` was non-empty and no credit is available. The
    ///   waker is parked in §16.8's blocked-writers map and woken by
    ///   MAX_STREAM_DATA or MAX_DATA.
    /// * `Err(WriteError::Finished)` — [`finish`](Self::finish) or
    ///   [`reset`](Self::reset) has been called on this handle, or the core
    ///   no longer holds the stream.
    /// * `Err(WriteError::ConnectionLost)` — the connection ended.
    ///
    /// # Cancel-safety
    ///
    /// **Cancel-safe: a dropped future has claimed nothing.** The poll body
    /// is synchronous, so a future can only be dropped between polls — that
    /// is, while pending, which is reached only after the core buffered
    /// **zero** bytes. No partially consumed buffer is ever stored across a
    /// poll, which is the hazard ruling 53 named for this verb.
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError> {
        poll_fn(|cx| self.poll_write(cx, buf)).await
    }

    /// The one implementation of [`write`](Self::write) (§16.3, ruling 53).
    pub(crate) fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, WriteError>> {
        let (outcome, dirty) = {
            let mut cell = self.cell.borrow_mut();

            // **[RATIFIED 2026/08/16 — ruling 165]** The **peer's** reset
            // outranks even our own terminal state: §9.8's overflow reset
            // means the bytes were discarded, and `Finished` would report
            // that as an orderly close. The core cannot answer this for us —
            // the next ACK frees a peer-reset half, after which it reports
            // `Finished` like any absent half.
            if let Some(code) = cell.peer_resets.get(&self.r).copied() {
                return Poll::Ready(Err(WriteError::Reset(code)));
            }
            // **[RATIFIED 2026/08/15 — ruling 124]** Terminal state first,
            // on this half as on the receive half: a handle reports the fate
            // of *its own stream*, and the connection's fate is `closed()`'s
            // to report. A half that has already finished or reset has no
            // further interaction with the connection left to fail.
            if self.local_end != LocalEnd::Live {
                return Poll::Ready(Err(WriteError::Finished));
            }
            // Then the connection's death (ruling 124 step 2).
            if let Some(lost) = cell.closed.clone() {
                return Poll::Ready(Err(WriteError::ConnectionLost(lost)));
            }
            // Then ruling 110's empty-buffer short-circuit — below the death
            // latch for the reason given in `poll_read`.
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            let Some(core) = cell.core.as_mut() else {
                return Poll::Ready(Err(WriteError::ConnectionLost(no_core())));
            };

            // **The core call is what arms the wakeup.**
            // `ConnEvent::StreamWritable` fires only for a send half whose
            // `blocked` flag is set, and `SendHalf::write` is the only thing
            // that sets it. A shell that read a credit accessor, decided it
            // was blocked and parked without calling the core would never be
            // woken: the core would not know a writer was waiting.
            match core.write(now(), self.r, buf) {
                Ok(0) => {
                    cell.blocked_writers
                        .entry(self.r)
                        .or_default()
                        .park(self.key, cx);
                    cell.dirty = true;
                    (Poll::Pending, true)
                }
                Ok(n) => {
                    cell.dirty = true;
                    (Poll::Ready(Ok(n)), true)
                }
                Err(e) => (Poll::Ready(Err(e)), false),
            }
        };
        if dirty {
            self.shell.mark_dirty(self.conn);
        }
        outcome
    }

    /// No more data: the FIN pins the final size (§9.3).
    ///
    /// **It resolves on the first poll and never parks.** `finish()` does
    /// *not* wait for the peer to acknowledge anything — ruling 47's
    /// `acked()`, which slice 5 builds, is the verb that would.
    ///
    /// Idempotent: a second `finish()` is `Ok(())`. After
    /// [`reset`](Self::reset) it is `Err(WriteError::Finished)`, and after
    /// it every [`write`](Self::write) is `Err(WriteError::Finished)`.
    ///
    /// Finishing also disarms the reset this handle's `Drop` would otherwise
    /// perform, which is the whole of §16.2's "without `finish()`".
    pub async fn finish(&mut self) -> Result<(), WriteError> {
        poll_fn(|cx| self.poll_finish(cx)).await
    }

    /// The one implementation of [`finish`](Self::finish).
    ///
    /// Always `Ready` on the first poll, and that is the specification
    /// rather than a shortcut: §16.7 makes sealing synchronous inside the
    /// mutating call that triggers it. `cx` is unused for exactly that
    /// reason — the verb is written in the poll form because §16.3 requires
    /// every data-path verb to have one, and because slice 8's
    /// `AsyncWrite::poll_shutdown` is this function with its error mapped.
    pub(crate) fn poll_finish(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), WriteError>> {
        let (outcome, dirty) = {
            let mut cell = self.cell.borrow_mut();
            // **Ruling 124 step 1** — this half's terminal state first. The
            // two terminal answers differ, which is why `local_end` is an
            // enum: `finish()` is idempotent, so a second one is `Ok(())`,
            // while a `finish()` after `reset()` is `Err(Finished)`. Both
            // outrank the connection's death: the FIN was accepted into send
            // state when it was accepted, and the connection dying later
            // does not un-accept it.
            // Ruling 165, ahead of our own terminal state — see `poll_write`.
            if let Some(code) = cell.peer_resets.get(&self.r).copied() {
                return Poll::Ready(Err(WriteError::Reset(code)));
            }
            match self.local_end {
                LocalEnd::Finished => return Poll::Ready(Ok(())),
                LocalEnd::Reset(_) => return Poll::Ready(Err(WriteError::Finished)),
                LocalEnd::Live => {}
            }
            // Then the connection's death (ruling 124 step 2).
            if let Some(lost) = cell.closed.clone() {
                return Poll::Ready(Err(WriteError::ConnectionLost(lost)));
            }
            let Some(core) = cell.core.as_mut() else {
                return Poll::Ready(Err(WriteError::ConnectionLost(no_core())));
            };
            match core.finish(now(), self.r) {
                Ok(()) => {
                    cell.dirty = true;
                    (Ok(()), true)
                }
                Err(e) => (Err(e), false),
            }
        };
        if outcome.is_ok() {
            self.local_end = LocalEnd::Finished;
        }
        if dirty {
            self.shell.mark_dirty(self.conn);
        }
        Poll::Ready(outcome)
    }

    /// Resolve once every byte written to **this** stream *and its FIN* are
    /// acknowledged by the peer's transport (**ruling 47**).
    ///
    /// It is legal and expected **after** [`finish`](Self::finish), and
    /// never returns [`WriteError::Finished`] for that reason (§16.2).
    ///
    /// # Before `finish()` it parks and never resolves
    ///
    /// **[RATIFIED 2026/08/15 — ruling 139(e)]** §16.2 requires every byte
    /// *and its FIN*, and a FIN that was never queued cannot be
    /// acknowledged. So this —
    ///
    /// ```text
    /// stream.write(&data).await?;   // no finish()
    /// stream.acked().await?;        // hangs for ever
    /// ```
    ///
    /// — is a permanent park on a healthy connection, and it looks exactly
    /// like a transport hang. It is written down because it is the one
    /// shape a caller reaches for by accident. Call
    /// [`finish`](Self::finish) first; or, if the stream is meant to stay
    /// open, use [`Connection::acked`], whose snapshot is FIN-free.
    ///
    /// # Every outcome
    ///
    /// * `Ok(())` — §9.7's `DataRecvd`: all bytes and the FIN acknowledged.
    ///   **This outranks the connection's death latch** (ruling 135): a
    ///   stream whose bytes were acknowledged completed, and the connection
    ///   dying afterwards does not un-complete it.
    /// * `Err(WriteError::Reset(code))` — [`reset`](Self::reset) was called
    ///   on this handle. Outranks everything, including a `DataRecvd` that
    ///   preceded it: the handle reports what the application last told it
    ///   to do with the stream.
    /// * `Err(WriteError::ConnectionLost(..))` — the connection ended with
    ///   this half unacknowledged. It **never parks** on a dead connection:
    ///   nothing further can be acknowledged (ruling 128).
    ///
    /// # Cancel-safety
    ///
    /// **Cancel-safe: a dropped future has claimed nothing.** It observes;
    /// nothing is consumed, and the waker slot is this handle's for its
    /// whole life rather than the future's.
    ///
    /// [`Connection::acked`]: super::Connection::acked
    pub async fn acked(&mut self) -> Result<(), WriteError> {
        poll_fn(|cx| self.poll_acked(cx)).await
    }

    /// The one implementation of [`acked`](Self::acked) (§16.3, ruling 53).
    ///
    /// A pure read: it mutates no core state, so unlike every other verb in
    /// this file it neither marks the cell dirty nor wakes the driver.
    pub(crate) fn poll_acked(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), WriteError>> {
        let mut cell = self.cell.borrow_mut();
        // **Ruling 124 step 1** — this half's own terminal state first.
        // Only `Reset` is terminal for *this* verb: `Finished` means the
        // FIN is in send state, which is the beginning of what `acked()`
        // waits for, and answering `WriteError::Finished` for it is what
        // §16.2:4372 forbids in terms.
        if let LocalEnd::Reset(code) = self.local_end {
            return Poll::Ready(Err(WriteError::Reset(code)));
        }
        // **[RATIFIED 2026/08/16 — ruling 135]** Then the acknowledgement,
        // **above** the death latch. The peer's ACK and the peer's CLOSE
        // can arrive in one driver pass and the application is woken after
        // the latch is set, so a latch-first order reports
        // `ConnectionLost` over a transfer that was fully delivered and
        // fully acknowledged — in a race the sender cannot win. This is
        // ruling 128's defect on the sender's side.
        // **Ruling 165.** §16.2: `acked()` returns `Reset(code)` if the
        // stream was reset before its data was acknowledged — "a local
        // reset, or the peer's §9.8 overflow reset". This must precede the
        // `finished_senders` latch, because the ACK that frees a peer-reset
        // half emits `StreamFinished`.
        if let Some(code) = cell.peer_resets.get(&self.r).copied() {
            return Poll::Ready(Err(WriteError::Reset(code)));
        }
        if cell.finished_senders.contains(&self.r) {
            return Poll::Ready(Ok(()));
        }
        if let Some(lost) = cell.closed.clone() {
            return Poll::Ready(Err(WriteError::ConnectionLost(lost)));
        }
        if cell.core.is_none() {
            return Poll::Ready(Err(WriteError::ConnectionLost(no_core())));
        }
        cell.blocked_ackers
            .entry(self.r)
            .or_default()
            .park(self.ack_key, cx);
        Poll::Pending
    }

    /// Abandon the stream abruptly with `error_code` (§9.6).
    ///
    /// Synchronous, infallible and **idempotent — the first code wins**.
    /// After it, [`write`](Self::write) and [`finish`](Self::finish) both
    /// answer `Err(WriteError::Finished)`.
    ///
    /// Legal **after `finish()`**: the core does not check the FIN, so a
    /// reset-after-finish pins `final_size` at the highest byte actually
    /// transmitted (ruling 111) and discards everything still buffered. That
    /// is a deliberate capability, not an oversight — but it is also exactly
    /// why this handle's `Drop` must not reset a stream that was finished.
    ///
    /// On a connection that has already ended it is a silent no-op.
    ///
    /// After it, [`acked`](Self::acked) reports
    /// `Err(WriteError::Reset(error_code))` — the code recorded here, not
    /// the peer's.
    pub fn reset(&mut self, error_code: u64) {
        let dirty = {
            let mut cell = self.cell.borrow_mut();
            match (cell.closed.is_some(), cell.core.as_mut()) {
                (false, Some(core)) => {
                    core.reset(now(), self.r, error_code);
                    cell.dirty = true;
                    true
                }
                _ => false,
            }
        };
        // Set even when the connection is dead: this handle has been closed
        // locally either way, and `Drop` must not go on to reset it.
        self.local_end = LocalEnd::Reset(error_code);
        if dirty {
            self.shell.mark_dirty(self.conn);
            // §16.2 settles an abandoned byte as well as an acknowledged
            // one, so this can complete a `Connection::acked()` snapshot
            // with no `ConnEvent` behind it — the third row of
            // [`wake_settled`]'s enumeration.
            wake_settled(&self.cell);
        }
    }
}

impl<S: Handshake> std::fmt::Debug for SendStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendStream")
            .field("id", &self.id())
            .field("local_end", &self.local_end)
            .finish_non_exhaustive()
    }
}

/// §16.2: *dropping a `SendStream` **without `finish()`** resets it with
/// error code 0*.
///
/// # The `closed_locally` gate is not an optimisation
///
/// `SendHalf::reset` early-returns **only when a reset already exists** — a
/// set FIN does *not* stop it. So "reset unconditionally on drop, the core
/// is idempotent" is wrong: it resets every stream the application
/// **finished**, pinning `final_size` at the highest byte transmitted,
/// discarding everything still buffered, and turning the peer's clean EOF
/// into `ReadError::Reset(0)`. Every byte not yet on the wire is destroyed,
/// and the defect is invisible to any test that reads before the sender
/// drops.
///
/// [`reset`](SendStream::reset) sets the same flag, so a handle already
/// reset is not reset a second time with a different code.
impl<S: Handshake> Drop for SendStream<S> {
    fn drop(&mut self) {
        let dirty = {
            let mut cell = self.cell.borrow_mut();
            release_waker_slot(&mut cell.blocked_writers, self.r, self.key);
            // Ruling 165's latch is bounded by live handles.
            cell.peer_resets.remove(&self.r);
            // Ruling 47's two slots go with it. This handle is the sole
            // owner of both — `SendStream` is not `Clone` — so removing
            // the latch here is what bounds `finished_senders` by the
            // number of live handles rather than by the number of streams
            // the connection has ever finished.
            release_waker_slot(&mut cell.blocked_ackers, self.r, self.ack_key);
            cell.finished_senders.remove(&self.r);
            match (self.local_end, cell.core.as_mut()) {
                (LocalEnd::Live, Some(core)) => {
                    core.reset(now(), self.r, constants::NO_ERROR);
                    cell.dirty = true;
                    true
                }
                // Either this handle is already closed locally, or the
                // driver has released the core (§15.4's endpoint-dropped
                // row). Both are ordinary teardown, not a defect.
                _ => false,
            }
        };
        if dirty {
            self.shell.mark_dirty(self.conn);
            // The reset above abandons every unacknowledged byte, which
            // §16.2 counts as settled — [`wake_settled`]'s third row again.
            wake_settled(&self.cell);
        }
        release_handle(&self.shell, &self.cell, self.conn);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// RecvStream
// ═══════════════════════════════════════════════════════════════════════

/// The receive half of a stream (§16.2).
///
/// **Not `Clone`**, for the same reason as [`SendStream`]: uniqueness is
/// what bounds §16.8's waker map and what makes this handle the sole author
/// of its half's retirement.
///
/// # Dropping it abandons the receive half
///
/// Arrivals are discarded and stream-level credit is never again advanced,
/// and the half is **retired at once** — freed, tombstoned and trued up to
/// the connection-level ledger in the same step (ruling 93). An abandoned
/// stream never wedges the connection window and never starves the peer's
/// cumulative stream allowance.
///
/// It is a handle for the driver's lifetime — see [`SendStream`]'s note on
/// ruling 115.
pub struct RecvStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    /// This handle's slot in `ConnCell::blocked_readers[r]`.
    key: u64,
    /// Ruling 121's latch: the terminal outcome, re-reported for ever.
    ended: Option<Ended>,
    id: Cell<Option<StreamId>>,
}

impl<S: Handshake> RecvStream<S> {
    /// Build a handle over an already-borrowed cell. See
    /// [`SendStream::install`] for why it takes the borrow rather than the
    /// cell.
    pub(crate) fn install(
        shell: Rc<dyn ShellLink>,
        cell_rc: Rc<RefCell<ConnCell<S>>>,
        cell: &mut ConnCell<S>,
        conn: ConnectionId,
        r: StreamRef,
    ) -> Self {
        shell.acquire();
        cell.handles += 1;
        let key = cell.blocked_readers.entry(r).or_default().key();
        Self {
            shell,
            cell: cell_rc,
            conn,
            r,
            key,
            ended: None,
            // **Filled eagerly, not lazily — ruling 143.** Ruling 116 said
            // "cached the first time the core answers `Some`", which is too
            // late: §12's ACK can fully close a locally-opened stream, and
            // `Streams::after_half_freed` then removes the entry, so a
            // handle whose *first* `id()` call happens after that answers
            // `None` for ever. Every handle is constructed while its stream
            // exists, so there is exactly one instant at which the answer is
            // guaranteed available, and this is it.
            id: Cell::new(cell.core.as_ref().and_then(|c| c.stream_id(r))),
        }
    }

    /// This stream's wire id (§9.1). Cached and keeps answering — see
    /// [`SendStream::id`].
    pub fn id(&self) -> Option<StreamId> {
        cached_id(&self.cell, self.r, &self.id)
    }

    /// Read from the contiguous prefix into `buf`.
    ///
    /// * `Ok(Some(n))` with `n >= 1` — `n` bytes were drained into
    ///   `buf[..n]`. `n <= buf.len()`; a short read is normal.
    /// * `Ok(Some(0))` — **exactly one meaning: `buf` was empty.** It never
    ///   means "no data"; that is pending. The check happens before the core
    ///   is touched (ruling 119), mirroring [`SendStream::write`]'s.
    /// * pending — `buf` was non-empty and nothing is available yet. The
    ///   waker is parked in §16.8's blocked-readers map.
    /// * `Ok(None)` — **end of stream**: the FIN's final size was reached
    ///   *and* every byte was delivered. **Sticky**: every later read is
    ///   `Ok(None)`.
    /// * `Err(ReadError::Reset(code))` — the peer sent RESET_STREAM (§9.6).
    ///   **Sticky**, latched here (ruling 121). Data the peer abandoned was
    ///   *not* delivered, which is precisely what distinguishes this from
    ///   `Ok(None)`.
    /// * `Err(ReadError::ConnectionLost)` — the connection ended.
    ///
    /// `Ok(Some(0))` versus `Ok(None)` is the distinction that hangs a
    /// reader for ever if it is inverted. At this surface the ambiguity is
    /// gone: `Ok(Some(0))` cannot mean "wait", because waiting is spelt
    /// pending.
    ///
    /// # After the connection dies you can still drain — **ruling 128**
    ///
    /// Bytes that arrived **before** the death are still delivered, and the
    /// FIN behind them still surfaces as `Ok(None)`. Only when nothing is
    /// left does this report `ConnectionLost`; it never parks on a dead
    /// connection, because nothing further can arrive. The case it exists
    /// for is the ordinary one — a sender that writes, finishes and drops
    /// its handles closes implicitly, and the receiver's driver processes
    /// the data and the CLOSE in the same pass — where the alternative
    /// loses a stream that arrived in full, in a race the receiver cannot
    /// win.
    ///
    /// **The window is not unbounded**, and it is exactly as long as the
    /// core keeps the stream state:
    ///
    /// * a peer CLOSE — `CLOSE_LINGER` (5 s), the drain §15.2 holds open
    ///   for this;
    /// * liveness timeout, nonce exhaustion, `Replaced`, endpoint dropped —
    ///   **no drain at all**. Those deaths drop the session at once and the
    ///   driver releases the core, so the first read after one of them
    ///   reports `ConnectionLost` however many bytes had arrived. Ruling
    ///   133 states that consequence rather than leaving it to be found,
    ///   and it is honest rather than unfortunate: a path that produced no
    ///   CLOSE produced no finished sender either;
    /// * a **local** `close()` — the drain currently lasts the linger here
    ///   too. Ruling 133 says a closing endpoint may free stream state at
    ///   once, because calling `close()` while a receive half holds unread
    ///   bytes *is* a decision to discard them; the core does not yet do
    ///   it, and until it does, a local closer can still drain what had
    ///   already arrived. Do not build on this: it is the one row of the
    ///   window that is expected to change.
    ///
    /// # Cancel-safety
    ///
    /// **Cancel-safe: a dropped future has claimed nothing.** Pending is
    /// reached only after the core reported zero bytes consumed, which
    /// advances no credit and retires no half. Bytes are read straight into
    /// the caller's `buf` — there is no shell-side scratch buffer (§10.6,
    /// ruling 56), so there is nowhere for a dropped future to strand data.
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError> {
        poll_fn(|cx| self.poll_read(cx, buf)).await
    }

    /// The one implementation of [`read`](Self::read) (§16.3, ruling 53).
    pub(crate) fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<Option<usize>, ReadError>> {
        let (outcome, dirty) = {
            let mut cell = self.cell.borrow_mut();

            // **[RATIFIED 2026/08/15 — ruling 124]** This handle's own
            // terminal state answers first — ahead of the connection's death
            // latch, not behind it. A stream that reached EOF *completed*;
            // reporting `ConnectionLost` for it afterwards tells a reader
            // that a finished transfer failed, which is ruling 121's
            // misreport with its sign flipped. Slice 8 makes it concrete:
            // `AsyncRead` requires a sticky EOF or `read_to_end` breaks.
            // The core is not sticky and would answer `Ok(None)` to a
            // re-read after a reset, which is why the latch lives here.
            match self.ended {
                Some(Ended::Eof) => return Poll::Ready(Ok(None)),
                Some(Ended::Reset(code)) => return Poll::Ready(Err(ReadError::Reset(code))),
                None => {}
            }
            // Then ruling 119's empty-buffer short-circuit, before the core
            // is touched.
            //
            // **It is now above the death latch, not below it — ruling
            // 128.** It was below, on the argument that "a dead connection
            // does not park, so `Ok(Some(0))` would report success on a
            // corpse". That argument assumed the latch answered next; it no
            // longer does, and the check has to stay above the core call
            // either way, because ruling 119's whole point is that an empty
            // buffer never reaches the core. What it now says on a dead
            // connection is the same thing it says on a live one: *you
            // asked for no bytes and got none*, which claims nothing about
            // the connection.
            if buf.is_empty() {
                return Poll::Ready(Ok(Some(0)));
            }

            // **[RATIFIED 2026/08/15 — ruling 128]** Then the **core**, and
            // the death latch only after it. This inverts ruling 124 step 2
            // for this verb and `accept_*`, and for no other: received,
            // unclaimed stream state survives the connection's death, so a
            // reader drains the bytes that already arrived and then reaches
            // the FIN's `Ok(None)`. Answering from the latch first loses
            // data that arrived in full, in a race the receiver cannot win
            // — the sender's driver delivers the data and the CLOSE in one
            // pass and wakes the application afterwards.
            //
            // The drain window is the core's: `CLOSE_LINGER` after a peer
            // CLOSE, and **zero** on the deaths with no linger (ruling
            // 133), where the core is released and the `None` arm below
            // answers.
            let Some(core) = cell.core.as_mut() else {
                return Poll::Ready(match cell.closed.clone() {
                    Some(lost) => Err(ReadError::ConnectionLost(lost)),
                    None => Err(ReadError::ConnectionLost(no_core())),
                });
            };

            match core.read(now(), self.r, buf) {
                // "No data available" — the one answer that would park.
                //
                // **The latch is re-checked here, and it has to be**
                // (ruling 128: *"parking is never permitted on a dead
                // connection"*). The core's own `lost` is not the same
                // fact as this cell's `closed`: a peer CLOSE latches
                // `cell.closed` at once (`publish`'s `ConnEvent::Closed`
                // arm) but the core is released only later, by
                // `release_dead`, after the `CLOSE_LINGER` drain — so for
                // that window `cell.core` is still `Some` and still
                // answers `Ok(Some(0))` over a connection `cell.closed`
                // already names as dead. (`Driver::stop` is not this case:
                // it nulls `cell.core` in the same call that latches, so a
                // `poll_read` reaching this line afterwards is already
                // impossible — see the `cell.core.as_mut()` check above.)
                // Trusting the core's guard alone parks a reader that
                // nothing will ever wake, which is the failure this whole
                // rule exists to prevent.
                // `[corrected 2026/08/18 — ruling 264]`
                Ok(Some(0)) => match cell.closed.clone() {
                    Some(lost) => (Poll::Ready(Err(ReadError::ConnectionLost(lost))), false),
                    None => {
                        cell.blocked_readers
                            .entry(self.r)
                            .or_default()
                            .park(self.key, cx);
                        cell.dirty = true;
                        (Poll::Pending, true)
                    }
                },
                Ok(Some(n)) => {
                    cell.dirty = true;
                    (Poll::Ready(Ok(Some(n))), true)
                }
                Ok(None) => {
                    self.ended = Some(Ended::Eof);
                    cell.dirty = true;
                    (Poll::Ready(Ok(None)), true)
                }
                Err(ReadError::Reset(code)) => {
                    self.ended = Some(Ended::Reset(code));
                    cell.dirty = true;
                    (Poll::Ready(Err(ReadError::Reset(code))), true)
                }
                Err(e) => (Poll::Ready(Err(e)), false),
            }
        };
        if dirty {
            self.shell.mark_dirty(self.conn);
        }
        outcome
    }
}

impl<S: Handshake> std::fmt::Debug for RecvStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecvStream")
            .field("id", &self.id())
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

/// §16.2 and ruling 93: dropping a `RecvStream` abandons the receive half.
///
/// **Unconditional, and that is checked rather than assumed.**
/// `Streams::retire_recv` starts with `entries.get_mut(&r)?` and
/// `stream.recv.take()?`, so a second retirement — after a `read` that
/// already returned `Ok(None)` or `Err(Reset)`, both of which retire — takes
/// the early return. There is no double true-up, which matters: running
/// `recv_window().consume(delta)` twice would grant the peer more window
/// than it is owed, and that is §10.6's memory bound failing open.
///
/// Ruling 93's **two** tombstone mechanisms — the per-half discard and
/// §9.2's watermark — are entirely the core's. This calls one verb and
/// mirrors none of it.
impl<S: Handshake> Drop for RecvStream<S> {
    fn drop(&mut self) {
        let dirty = {
            let mut cell = self.cell.borrow_mut();
            release_waker_slot(&mut cell.blocked_readers, self.r, self.key);
            match cell.core.as_mut() {
                Some(core) => {
                    core.abandon_recv(now(), self.r);
                    cell.dirty = true;
                    true
                }
                None => false,
            }
        };
        if dirty {
            self.shell.mark_dirty(self.conn);
        }
        release_handle(&self.shell, &self.cell, self.conn);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// BiStream
// ═══════════════════════════════════════════════════════════════════════

/// Both halves of one bidirectional stream (§9.1).
///
/// It carries no verbs of its own: §16.2's whole comment on it is
/// *`.split()` → the pair*, and `AsyncRead`/`AsyncWrite` are slice 8
/// (ruling 96). [`split`](Self::split) it and use the halves.
///
/// # Drop
///
/// `BiStream` has **no `Drop` impl of its own** — its two fields' do the
/// work, in declaration order: the send half first (RESET_STREAM, unless it
/// was finished), then the receive half (`abandon_recv`). Both frames leave
/// in one pump, because the second `Dirty` finds the first still undrained.
/// Nothing in the spec fixes that order; it is recorded here because a build
/// that implemented `Drop` on `BiStream` itself and forgot one half would
/// pass any test that checked only the other.
pub struct BiStream<S: Handshake> {
    send: SendStream<S>,
    recv: RecvStream<S>,
}

impl<S: Handshake> BiStream<S> {
    pub(crate) fn new(send: SendStream<S>, recv: RecvStream<S>) -> Self {
        Self { send, recv }
    }

    /// This stream's wire id (§9.1). Both halves name the same stream, so
    /// this is [`SendStream::id`].
    pub fn id(&self) -> Option<StreamId> {
        self.send.id()
    }

    /// Take the two halves apart.
    ///
    /// The halves are independent from here: each carries its own drop rule,
    /// and dropping one does nothing to the other.
    pub fn split(self) -> (SendStream<S>, RecvStream<S>) {
        (self.send, self.recv)
    }

    /// The send half, for `compat::io`'s `AsyncWrite` delegation.
    ///
    /// **Borrowing, deliberately, rather than `split`ing**: §16.11's
    /// `AsyncWrite for BiStream` writes through the send half while the
    /// receive half stays attached, because *"a half-closed `BiStream` is
    /// the shape `copy_bidirectional` and every request/response protocol
    /// relies on"*. Taking the halves apart would drop one of them.
    pub(crate) fn send_mut(&mut self) -> &mut SendStream<S> {
        &mut self.send
    }

    /// The receive half, for `compat::io`'s `AsyncRead` delegation.
    ///
    /// [`send_mut`](Self::send_mut)'s reason, from the other side.
    pub(crate) fn recv_mut(&mut self) -> &mut RecvStream<S> {
        &mut self.recv
    }

    /// Put two halves back together — **ruling 120**.
    ///
    /// Both must name the **same stream on the same connection**. On a
    /// mismatch the pair is handed back unchanged, in the order it was
    /// given.
    ///
    /// It is a `Result` and not a `debug_assert` because ruling 44's
    /// precedent governs — *rejection is a `Result`, never a panic*. Under
    /// the assertion a release build would hold a `BiStream` whose halves
    /// are different streams, whose [`id`](Self::id) is a lie, and whose
    /// `Drop` resets a stream the caller never named. No new error type, so
    /// §18.1's taxonomy stays closed (ruling 61).
    ///
    /// # The cell identity is checked as well as the `ConnectionId`
    ///
    /// Ruling 120 words the test as *the same `StreamRef` on the same
    /// `ConnectionId`*. A `ConnectionId` is monotone and never reused
    /// **within one endpoint** — but two endpoints in one process mint the
    /// same ids, so that pair alone would accept two halves of two
    /// different connections' streams. The extra `Rc::ptr_eq` conjunct
    /// cannot reject a legitimate pair, because both halves of a stream are
    /// always built from one `Rc`.
    ///
    /// The `Err` variant is deliberately large — it *is* the two handles,
    /// handed back rather than dropped. Boxing them, which is what
    /// `clippy::result_large_err` asks for, would change a ratified
    /// signature (ruling 120) to save a move of two handle structs on a path
    /// taken only by a caller who has already made a mistake.
    #[expect(
        clippy::result_large_err,
        reason = "ruling 120 fixes this signature; the `Err` is the returned pair itself"
    )]
    pub fn join(
        send: SendStream<S>,
        recv: RecvStream<S>,
    ) -> Result<Self, (SendStream<S>, RecvStream<S>)> {
        if Rc::ptr_eq(&send.cell, &recv.cell) && send.conn == recv.conn && send.r == recv.r {
            Ok(Self::new(send, recv))
        } else {
            Err((send, recv))
        }
    }
}

impl<S: Handshake> std::fmt::Debug for BiStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BiStream")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Shared by both halves
// ═══════════════════════════════════════════════════════════════════════

/// The answer when the cell holds neither a core nor a close reason.
///
/// Unreachable by construction: `Driver::release_dead` only clears a core
/// whose `closed` is already set, and `Driver::stop` latches before it
/// clears. The `debug_assert` is the pin for that, and
/// `ConnectionLost::EndpointDropped` is the honest degraded answer if the
/// invariant is ever broken — hanging would be worse.
fn no_core() -> ConnectionLost {
    debug_assert!(
        false,
        "a connection cell held neither a core nor a close reason (§16.3)"
    );
    ConnectionLost::EndpointDropped
}

/// Release one stream handle: the per-connection count, the process-wide
/// count, and §16.2's last-handle `close(NO_ERROR, "")`.
///
/// **This is the same rule `Connection::drop` applies** (§16.2: *dropping
/// the last handle to a `Connection` performs `close(NO_ERROR, "")`*), and
/// ruling 115 is what brings stream handles inside it. The ordinary shape it
/// exists for: a task owns a stream, has let the `Connection` go, and
/// finishes — that drop is the last handle to the connection, and without
/// this the peer would learn nothing until `DEAD_TIMEOUT`.
///
/// Ruling 88's exception applies unchanged: when this is also the last
/// handle **in the process**, the driver is already stopping and §15.4's
/// endpoint-dropped row governs — nothing is transmitted.
fn release_handle<S: Handshake>(
    shell: &Rc<dyn ShellLink>,
    cell: &Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
) {
    let last_for_connection = {
        let mut borrow = cell.borrow_mut();
        debug_assert!(borrow.handles > 0, "a stream handle was released twice");
        borrow.handles = borrow.handles.saturating_sub(1);
        borrow.handles == 0
    };
    // Before the decision below, because that decision is exactly "was this
    // also the last handle in the process?" (ruling 88).
    let last_in_process = shell.release();
    if last_for_connection && !last_in_process {
        close_now(shell, cell, conn, constants::NO_ERROR, b"");
    }
}

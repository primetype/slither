//! §16.11's byte-stream face: the two `io::Error` conversions and the four
//! `AsyncRead`/`AsyncWrite` impls.
//!
//! Ungated — `tokio` is already a hard dependency and the two traits
//! themselves need no tokio feature.
//!
//! Nothing here adds a field, a `Drop` or any state. Every impl is one of
//! the handles' existing `pub(crate) poll_*` verbs *with its error mapped*,
//! which is §16.3's *"written once"* rule (ruling 53) spelled out for this
//! surface: *"§16.11's `AsyncRead`/`AsyncWrite` is the **same function**
//! with its error mapped."*

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::error::{ConnectionLost, ReadError, WriteError};
use crate::packet::Handshake;
use crate::shell::{BiStream, RecvStream, SendStream};

// ═══════════════════════════════════════════════════════════════════════
// §16.11.1 — the `io::ErrorKind` mapping
// ═══════════════════════════════════════════════════════════════════════

/// §16.11.1's read column.
///
/// **[RATIFIED 2026/08/16 — ruling 227.]** Until that ruling §16.11 read
/// `Reset → ConnectionReset` and `ConnectionLost → NotConnected /
/// BrokenPipe` — a slash between two kinds with no rule for choosing, over a
/// [`ConnectionLost`] with seven variants.
fn read_kind(err: &ReadError) -> io::ErrorKind {
    match err {
        ReadError::Reset(_) => io::ErrorKind::ConnectionReset,
        ReadError::ConnectionLost(lost) => match lost {
            ConnectionLost::TimedOut => io::ErrorKind::TimedOut,
            ConnectionLost::NonceExhausted => io::ErrorKind::ConnectionAborted,
            ConnectionLost::LocallyClosed => io::ErrorKind::NotConnected,
            ConnectionLost::PeerClosed { .. } => io::ErrorKind::ConnectionAborted,
            ConnectionLost::ProtocolViolation { .. } => io::ErrorKind::ConnectionAborted,
            ConnectionLost::Replaced => io::ErrorKind::ConnectionAborted,
            ConnectionLost::EndpointDropped => io::ErrorKind::NotConnected,
        },
    }
}

/// §16.11.1's write column.
///
/// The rule, so it can be judged rather than memorised: **on the write side,
/// a peer or transport that went away *under a writer* is `BrokenPipe`; a
/// connection *this side* never had or gave up is `NotConnected`.** That
/// reads §16.11's original slash as a **variant** split rather than a
/// read/write direction split — which is how it was written, the comment
/// having sat on the `WriteError` line alone.
///
/// `TimedOut` is lifted out of both columns because
/// [`io::ErrorKind::TimedOut`] exists and a `DEAD_TIMEOUT` death is exactly
/// what it names; collapsing it would make every death look alike to a
/// consumer whose only view is an [`io::Error`].
//
// **[RATIFIED 2026/08/16 — ruling 238.]** This match is **exhaustive** and
// carries no `_ =>` arm. It read the other way until that ruling: ruling 227
// required a fallback mapping to `Other`, reasoning that `WriteError` is
// `#[non_exhaustive]` (ruling 61 reserves `Stopped`) ***so*** the conversion
// needs one.
//
// The *"so"* is false. **`#[non_exhaustive]` is inert inside the defining
// crate**, and this conversion can only live here — the orphan rule puts
// `impl From<WriteError> for io::Error` in slither or nowhere — so the
// attribute never bites and rustc reports the arm as unreachable.
// `error.rs`'s `write_error_is_exhaustive_in_crate` already proves it.
//
// Ruling 238 moved the **conclusion** as well as the reasoning, which is why
// the arm is gone rather than merely `#[allow]`ed. A `_ =>` arm does not
// future-proof this conversion, it **hides** the future: it would silently
// map `Stopped` to `Other` on the day that variant lands. An exhaustive match
// makes that day a compile error at the one site that must be updated — which
// is what ruling 227's own *"must not be `unreachable!()`"* clause was
// reaching for. **A variant that cannot compile cannot panic.**
fn write_kind(err: &WriteError) -> io::ErrorKind {
    match err {
        WriteError::Reset(_) => io::ErrorKind::ConnectionReset,
        WriteError::Finished => io::ErrorKind::BrokenPipe,
        WriteError::ConnectionLost(lost) => match lost {
            ConnectionLost::TimedOut => io::ErrorKind::TimedOut,
            ConnectionLost::NonceExhausted => io::ErrorKind::BrokenPipe,
            ConnectionLost::LocallyClosed => io::ErrorKind::NotConnected,
            ConnectionLost::PeerClosed { .. } => io::ErrorKind::BrokenPipe,
            ConnectionLost::ProtocolViolation { .. } => io::ErrorKind::BrokenPipe,
            ConnectionLost::Replaced => io::ErrorKind::BrokenPipe,
            ConnectionLost::EndpointDropped => io::ErrorKind::NotConnected,
        },
    }
}

/// §16.11's `impl From<ReadError> for std::io::Error`, with §16.11.1's kinds.
///
/// **The original error is preserved as the [`io::Error`]'s inner value.**
/// Every arm is `io::Error::new(kind, err)`, never `io::Error::from(kind)`,
/// so a caller recovers what the [`io::ErrorKind`] projection drops —
/// including a reset's `u64` code, which is data only this conversion's
/// consumer can reach:
///
/// ```
/// use slither::error::ReadError;
///
/// let e: std::io::Error = ReadError::Reset(7).into();
/// assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
/// let inner = e.into_inner().expect("the slither error is preserved");
/// assert_eq!(*inner.downcast::<ReadError>().unwrap(), ReadError::Reset(7));
/// ```
impl From<ReadError> for io::Error {
    fn from(err: ReadError) -> Self {
        io::Error::new(read_kind(&err), err)
    }
}

/// §16.11's `impl From<WriteError> for std::io::Error`, with §16.11.1's
/// kinds.
///
/// The original error is preserved as the inner value, exactly as for
/// [`ReadError`].
impl From<WriteError> for io::Error {
    fn from(err: WriteError) -> Self {
        io::Error::new(write_kind(&err), err)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The three shared bodies
// ═══════════════════════════════════════════════════════════════════════
//
// `SendStream`/`RecvStream` and the two halves of a `BiStream` run the
// *same* code: the delegating impls below are two lines each, so there is
// no second implementation of anything (ruling 53, invariant 2).

/// [`AsyncWrite::poll_write`] over [`SendStream`]'s verb.
///
/// **No `WriteZero` guard, and none is wanted.** `SendStream::poll_write`
/// returns `Ok(0)` **only** on its `buf.is_empty()` short-circuit (ruling
/// 110); a non-empty write blocked by flow-control credit parks in
/// `blocked_writers` and returns `Poll::Pending`. So this can only answer
/// `Ok(0)` to a caller that passed an empty buffer, which is what
/// `AsyncWrite` asks for.
fn poll_write_half<S: Handshake>(
    send: &mut SendStream<S>,
    cx: &mut Context<'_>,
    buf: &[u8],
) -> Poll<io::Result<usize>> {
    send.poll_write(cx, buf).map(|r| r.map_err(io::Error::from))
}

/// [`AsyncWrite::poll_shutdown`] over [`SendStream`]'s verbs — **ruling 57**.
///
/// `finish()` **and then** `acked()`. The weaker reading — `finish()` alone
/// — is prior art elsewhere and is rejected here for ruling 47's reason: the
/// natural last act of a transfer is `copy(..).await; shutdown().await`, and
/// under the weak reading it loses its tail at the path's loss rate,
/// silently. That is S28's bug reachable a second time, through the
/// `AsyncWrite` surface, by an application that never touches `close()`.
///
/// # No `shutdown_started` flag, and that is a fact rather than a shortcut
///
/// [`SendStream::poll_finish`] is *"always `Ready` on the first poll"* —
/// §16.7 makes sealing synchronous inside the mutating call that triggers it
/// — and `finish()` is idempotent, *"a second `finish()` is `Ok(())`"*. So
/// re-entering after a `Pending` from `poll_acked` calls `poll_finish` again
/// harmlessly, and there is no state to store.
///
/// It cannot hang past the connection's own death: a dying connection
/// resolves `poll_acked` in error.
fn poll_shutdown_half<S: Handshake>(
    send: &mut SendStream<S>,
    cx: &mut Context<'_>,
) -> Poll<io::Result<()>> {
    ready!(send.poll_finish(cx)).map_err(io::Error::from)?;
    send.poll_acked(cx).map(|r| r.map_err(io::Error::from))
}

/// [`AsyncRead::poll_read`] over [`RecvStream`]'s verb.
///
/// # The empty-buffer short-circuit is load-bearing
///
/// An empty `buf` is answered here, **without touching the handle**. Ruling
/// 119 makes `RecvStream::poll_read` answer `Ok(Some(0))` to an empty slice,
/// whose documented meaning is *park* — and once mapped into a [`ReadBuf`]
/// that filled nothing it is **indistinguishable from EOF**. `Ok(Some(0))` =
/// park and `Ok(None)` = end of stream is the distinction ruling 119 exists
/// to protect, and the `AsyncRead` surface is where the two collide, so the
/// collision is removed *before* the call rather than after it.
///
/// # No scratch buffer
///
/// The tail of the caller's `ReadBuf` is filled directly. `RecvStream::read`
/// pins the reason: *"there is no shell-side scratch buffer (§10.6), so
/// there is nowhere for a dropped future to strand data"* — an impl that
/// read into a `Vec` and copied out would build one (§10.6, invariant 7).
/// The crate forbids `unsafe`, so the uninitialised tail is reached through
/// [`ReadBuf::initialize_unfilled_to`], which is safe.
///
/// # EOF is sticky, and it is already sticky
///
/// Rulings 121 and 124 latch `Ended::Eof` / `Ended::Reset(code)` **in the
/// handle**, ahead of the connection's death latch, and §16.11 is why that
/// matters: *"`AsyncRead` requires a sticky end-of-file, so a connection
/// that dies after the FIN would otherwise surface a spurious `io::Error` to
/// `read_to_end`."* This function must not re-implement it and must not
/// defeat it.
fn poll_read_half<S: Handshake>(
    recv: &mut RecvStream<S>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
) -> Poll<io::Result<()>> {
    let remaining = buf.remaining();
    if remaining == 0 {
        return Poll::Ready(Ok(()));
    }
    let filled = {
        let dst = buf.initialize_unfilled_to(remaining);
        match ready!(recv.poll_read(cx, dst)) {
            Ok(Some(n)) => n,
            // End of stream. `AsyncRead` spells EOF *"returned `Ok` without
            // filling anything"*, so this and the `Ok(Some(0))` that the
            // short-circuit above makes unreachable land on the same line.
            // **Not `unreachable!()`** for the second: a panic reachable
            // only through a future refactor is worse than a defensive arm
            // that costs nothing.
            Ok(None) => 0,
            Err(e) => return Poll::Ready(Err(e.into())),
        }
    };
    buf.advance(filled);
    Poll::Ready(Ok(()))
}

// ═══════════════════════════════════════════════════════════════════════
// The four impls
// ═══════════════════════════════════════════════════════════════════════
//
// All four handles are plain structs of `Rc`s and `Copy` fields, so each is
// `Unpin` and every impl begins `self.get_mut()`. None of them projects a
// pin, and none adds a field, a `Drop` or any state.

impl<S: Handshake> AsyncWrite for SendStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        poll_write_half(self.get_mut(), cx, buf)
    }

    /// **A no-op that returns `Ready`, and it is not delivery confirmation**
    /// — ruling 56, which requires this paragraph as well as this behaviour.
    ///
    /// It touches nothing: not the cell, not the core, not the driver.
    ///
    /// **Why it can be a no-op.** Bytes accepted by `poll_write` are already
    /// in send state, and `poll_write` accepts only what flow-control credit
    /// admits (§10.1), so there is no shell buffer for a flush to push.
    ///
    /// **What to use instead.** [`SendStream::acked`] is the verb that means
    /// *acknowledged*. Promising acknowledgement here would make `flush` a
    /// second, weaker `acked()` and put the two in competition — ruling 54
    /// renamed `Connection::flush()` to `acked()` precisely so the two
    /// cannot be confused.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// `finish()` **and then** `acked()` — ruling 57. See
    /// [`SendStream::acked`] for what the second half waits for.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        poll_shutdown_half(self.get_mut(), cx)
    }
}

impl<S: Handshake> AsyncRead for RecvStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        poll_read_half(self.get_mut(), cx, buf)
    }
}

/// Delegates to the **receive** half.
///
/// [`BiStream`] gains **no `Drop`** from this impl. Its existing rustdoc
/// records why: it *"has no `Drop` impl of its own — its two fields' do the
/// work, in declaration order"*, and a build that implemented `Drop` on
/// `BiStream` and forgot one half *"would pass any test that checked only
/// the other."*
impl<S: Handshake> AsyncRead for BiStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        poll_read_half(self.get_mut().recv_mut(), cx, buf)
    }
}

/// Delegates to the **send** half.
///
/// [`poll_shutdown`](AsyncWrite::poll_shutdown) shuts down the send half
/// **only**: it does not touch, abandon or reset the receive half. A
/// half-closed `BiStream` is the shape `copy_bidirectional` and every
/// request/response protocol relies on — the request ends with a FIN and the
/// response is still to come.
impl<S: Handshake> AsyncWrite for BiStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        poll_write_half(self.get_mut().send_mut(), cx, buf)
    }

    /// A no-op returning `Ready`, for `AsyncWrite for SendStream`'s reason
    /// (ruling 56). **Not delivery confirmation** — see
    /// [`SendStream::acked`].
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// The **send half's** `finish()` then `acked()` (ruling 57). The
    /// receive half is left untouched and readable.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        poll_shutdown_half(self.get_mut().send_mut(), cx)
    }
}

//! §16.11's `Stream`/`Sink` faces — `feature = "sink"`.
//!
//! Six `Stream`s and two `Sink`s over §16.2's verbs, using
//! [`futures_core::Stream`] and [`futures_sink::Sink`].
//!
//! # The one invariant that governs all of them — **ruling 58, normative**
//!
//! *"A `Stream` adapter over `recv_message`, `recv_datagram`,
//! `accept_bi`/`accept_uni` or `notified()` claims **at most one item, and
//! only from inside `poll_next`**. No prefetch, no read-ahead task, no
//! intermediate queue."*
//!
//! §16.4's pull model is precisely what keeps reliable data in the core
//! until the application takes it (§16.8), and an adapter that claimed ahead
//! of its consumer would rebuild the unbounded shell queue §10.6 forbids —
//! **while looking like an ordinary ergonomic convenience**. Concretely,
//! every `poll_next` below calls its underlying `poll_*` exactly once and
//! returns what it got: no struct here holds a `VecDeque`, a `Vec<Item>` or
//! an `Option<Item>` receive slot, and none spawns a task. The one buffer in
//! the whole of `compat/` is [`MessageSink`]'s single pending slot, and it
//! exists because `Sink`'s own protocol requires it on the **send** side.
//!
//! # They never end — **ruling 226**
//!
//! Every `Result`-carrying face yields `Some(Err(ConnectionLost))` for as
//! long as it is polled after the connection dies, and **never `None`**.
//! That is faithful to the verbs they wrap — the underlying `poll_*`
//! re-report the latched death indefinitely, and [`ConnectionLost`] is
//! `Clone` for exactly that reason — and it keeps the *reason* recoverable,
//! which a `None` destroys. **A bare `while let Some(_) = s.next().await`
//! therefore spins**; each type's rustdoc repeats that with the terminating
//! shape.
//!
//! [`Incoming`] is the exception, and not by a different rule: its item is
//! not a `Result`, and `Endpoint::accept()` already answers `None` for *the
//! endpoint is closed — the driver has stopped*.
//!
//! # They borrow — **ruling 231**
//!
//! Neither [`Connection`] nor [`Endpoint`] is `Clone`, and `Connection`'s
//! last-handle-drop rule (`close(NO_ERROR, "")`) is load-bearing, so every
//! adapter borrows its handle and carries a lifetime. The consequence a
//! consumer meets is that **a borrowed adapter cannot be moved into
//! `spawn_local`** — the handle moves in and the adapter is built inside the
//! task. Each type's rustdoc shows that shape.

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use futures_core::Stream;
use futures_sink::Sink;

use crate::core::Dir;
use crate::error::{ConnectionLost, DatagramError, MessageError};
use crate::identity::Identity;
use crate::packet::Handshake;
use crate::shell::{BiStream, Connection, Endpoint, Intro, Notification, RecvStream, WakerSlot};

// ═══════════════════════════════════════════════════════════════════════
// The constructors
// ═══════════════════════════════════════════════════════════════════════
//
// Inherent `impl` blocks on crate-local types, written in this module:
// Rust permits that in any module of the defining crate, so
// `src/shell/connection.rs` and `src/shell/endpoint.rs` need no edit for
// the faces themselves.

// `S: 'static` is ruling 228's bound, not a new one of this module's: the
// type-erased slot the adapters hold is a `Box<dyn FnMut(u64)>`, whose
// release closure owns an `Rc` into the connection cell. It restricts nothing
// that can exist — `EndpointBuilder::build` already requires `I: 'static`,
// and a `Connection<S>` is only reachable through an `Endpoint<I>`.
impl<S: Handshake + 'static> Connection<S> {
    /// A [`Stream`] over §9.8's messages — [`recv_message`] as a face.
    ///
    /// # Messages and streams do not mix — **S30, ruling 51**
    ///
    /// `messages()` and [`incoming_uni()`](Self::incoming_uni) **draw from
    /// the same supply** and must not both be used on one connection. §9.8's
    /// sugar rides auto-managed unidirectional streams and **the wire
    /// carries no discriminator**, so nothing can tell a "message" from an
    /// application's own uni stream. Mixing them is **normatively a
    /// programming error**, not a race you can win by being careful.
    ///
    /// The safe and unsafe shapes look alike at the call site, which is why
    /// this is written down: if you want both objects and streams on one
    /// connection, use `messages()` alongside
    /// [`open_bi`](Self::open_bi)/[`incoming_bi()`](Self::incoming_bi),
    /// which are a separate supply.
    ///
    /// # It never ends — ruling 226
    ///
    /// After the connection dies this yields `Some(Err(..))` for as long as
    /// it is polled, so **`while let Some(_) = msgs.next().await` spins**:
    ///
    /// ```text
    /// let mut msgs = conn.messages().take_while(|r| ready(r.is_ok()));
    /// while let Some(Ok(msg)) = msgs.next().await { /* … */ }
    /// ```
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {   // the `Connection` moves in
    ///     let mut msgs = conn.messages();     // the adapter is built here
    ///     while let Some(Ok(msg)) = msgs.next().await { /* … */ }
    /// });
    /// ```
    ///
    /// [`recv_message`]: Self::recv_message
    pub fn messages(&self) -> Messages<'_, S> {
        Messages {
            conn: self,
            slot: self.message_reader_slot_boxed(),
        }
    }

    /// A [`Stream`] over §11's unreliable datagrams —
    /// [`recv_datagram`](Self::recv_datagram) as a face.
    ///
    /// Datagrams are a separate supply from messages and from streams, so
    /// this composes with any of the others.
    ///
    /// # It never ends — ruling 226
    ///
    /// ```text
    /// let mut dgrams = conn.datagrams().take_while(|r| ready(r.is_ok()));
    /// while let Some(Ok(d)) = dgrams.next().await { /* … */ }
    /// ```
    ///
    /// A bare `while let Some(_) = dgrams.next().await` **spins** once the
    /// connection is dead.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {   // the `Connection` moves in
    ///     let mut dgrams = conn.datagrams();  // the adapter is built here
    ///     while let Some(Ok(d)) = dgrams.next().await { /* … */ }
    /// });
    /// ```
    pub fn datagrams(&self) -> Datagrams<'_, S> {
        Datagrams {
            conn: self,
            slot: self.datagram_reader_slot_boxed(),
        }
    }

    /// A [`Stream`] over peer-opened bidirectional streams —
    /// [`accept_bi`](Self::accept_bi) as a face. FIFO, in open order.
    ///
    /// This is the safe companion to [`messages()`](Self::messages): bidi
    /// streams are a different supply from §9.8's sugar, so the two compose.
    ///
    /// # It never ends — ruling 226
    ///
    /// ```text
    /// let mut streams = conn.incoming_bi().take_while(|r| ready(r.is_ok()));
    /// while let Some(Ok(s)) = streams.next().await { /* … */ }
    /// ```
    ///
    /// A bare `while let Some(_) = streams.next().await` **spins** once the
    /// connection is dead.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {      // the `Connection` moves in
    ///     let mut streams = conn.incoming_bi();  // the adapter is built here
    ///     while let Some(Ok(s)) = streams.next().await { /* … */ }
    /// });
    /// ```
    pub fn incoming_bi(&self) -> IncomingBi<'_, S> {
        IncomingBi {
            conn: self,
            slot: self.acceptor_slot_boxed(Dir::Bi),
        }
    }

    /// A [`Stream`] over peer-opened unidirectional streams —
    /// [`accept_uni`](Self::accept_uni) as a face. FIFO, in open order.
    ///
    /// # Messages and streams do not mix — **S30, ruling 51**
    ///
    /// `incoming_uni()` and [`messages()`](Self::messages) **draw from the
    /// same supply** and must not both be used on one connection. §9.8's
    /// messages *are* auto-managed unidirectional streams and **the wire
    /// carries no discriminator**, so a peer's message and a peer's uni
    /// stream are the same bytes: whichever face claims one first gets it.
    /// Mixing them is **normatively a programming error**.
    ///
    /// The safe and unsafe shapes look alike at the call site. If you want
    /// both, take objects from `messages()` and streams from
    /// [`incoming_bi()`](Self::incoming_bi).
    ///
    /// # It never ends — ruling 226
    ///
    /// ```text
    /// let mut streams = conn.incoming_uni().take_while(|r| ready(r.is_ok()));
    /// while let Some(Ok(s)) = streams.next().await { /* … */ }
    /// ```
    ///
    /// A bare `while let Some(_) = streams.next().await` **spins** once the
    /// connection is dead.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {       // the `Connection` moves in
    ///     let mut streams = conn.incoming_uni();  // the adapter is built here
    ///     while let Some(Ok(s)) = streams.next().await { /* … */ }
    /// });
    /// ```
    pub fn incoming_uni(&self) -> IncomingUni<'_, S> {
        IncomingUni {
            conn: self,
            slot: self.acceptor_slot_boxed(Dir::Uni),
        }
    }

    /// A [`Stream`] over §16.2's [`Notification`]s —
    /// [`notified`](Self::notified) as a face.
    ///
    /// # It never ends — ruling 226
    ///
    /// ```text
    /// let mut events = conn.notifications().take_while(|r| ready(r.is_ok()));
    /// while let Some(Ok(ev)) = events.next().await { /* … */ }
    /// ```
    ///
    /// A bare `while let Some(_) = events.next().await` **spins** once the
    /// connection is dead.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {        // the `Connection` moves in
    ///     let mut events = conn.notifications();   // the adapter is built here
    ///     while let Some(Ok(ev)) = events.next().await { /* … */ }
    /// });
    /// ```
    pub fn notifications(&self) -> Notifications<'_, S> {
        Notifications {
            conn: self,
            slot: self.notification_slot_boxed(),
        }
    }

    /// A [`Sink`] over §9.8's messages —
    /// [`send_message`](Self::send_message) as a face.
    ///
    /// It holds **at most one** pending `Vec<u8>`, because `Sink`'s protocol
    /// requires it: `start_send` may not block and `send_message` is
    /// `async`. That single slot is the only buffer in the whole of
    /// `compat/`, and it is on the **send** side, so ruling 58 — which is
    /// about claiming ahead of a *consumer* — is untouched.
    ///
    /// [`poll_close`](Sink::poll_close) **does not close the connection**;
    /// see its own note.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {   // the `Connection` moves in
    ///     let mut out = conn.message_sink();  // the adapter is built here
    ///     out.send(payload).await?;
    /// });
    /// ```
    pub fn message_sink(&self) -> MessageSink<'_, S> {
        MessageSink {
            conn: self,
            slot: self.message_sender_slot_boxed(),
            pending: None,
        }
    }

    /// A [`Sink`] over §11's unreliable datagrams —
    /// [`send_datagram`](Self::send_datagram) as a face.
    ///
    /// It buffers **nothing**: `send_datagram` is synchronous and never
    /// waits (§11.3's send queue is bounded at 64 with a drop-oldest
    /// discipline, so pressure evicts rather than blocking), so
    /// `poll_ready` is always `Ready` and `start_send` does the whole of the
    /// work. There is no waker slot here because there is nothing to park
    /// on.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {    // the `Connection` moves in
    ///     let mut out = conn.datagram_sink();  // the adapter is built here
    ///     out.send(payload).await?;
    /// });
    /// ```
    pub fn datagram_sink(&self) -> DatagramSink<'_, S> {
        DatagramSink { conn: self }
    }
}

impl<I: Identity> Endpoint<I> {
    /// A [`Stream`] over §6.2's introductions —
    /// [`accept`](Self::accept) as a face.
    ///
    /// **[ruling 229]** §16.11's list of `Stream`/`Sink` faces named seven,
    /// none of them the endpoint's, and this is the eighth — added rather
    /// than the plan's face being struck, because ruling 58 governs it
    /// identically and nothing in §16.11 wanted it excluded.
    ///
    /// # This one *does* end
    ///
    /// Its item is an [`Intro`], not a `Result`, and `accept()` already
    /// answers `None` for *"the endpoint is closed — the driver has
    /// stopped"*. So `while let Some(intro) = incoming.next().await` is the
    /// correct shape here, and does not spin.
    ///
    /// # Dropping it with a request outstanding is §6.2's silent reject
    ///
    /// The adapter holds one outstanding request between polls. Dropping it
    /// in the instant the driver handed an introduction over drops that
    /// `Intro` — which is *"the documented meaning of dropping a staged
    /// object, not a loss"*, exactly as for a dropped `accept()` future. An
    /// initiator that got no answer retransmits.
    ///
    /// Costs **0 DH** per item: an `Intro` is msg1's parked bytes and its
    /// source, nothing more.
    ///
    /// # It borrows — ruling 231
    ///
    /// ```text
    /// tokio::task::spawn_local(async move {     // the `Endpoint` moves in
    ///     let mut incoming = endpoint.incoming();
    ///     while let Some(intro) = incoming.next().await { /* … */ }
    /// });
    /// ```
    pub fn incoming(&self) -> Incoming<'_, I> {
        Incoming {
            endpoint: self,
            pending: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The five `Result`-carrying streams
// ═══════════════════════════════════════════════════════════════════════

/// The [`Stream`] returned by [`Connection::messages`].
///
/// **Never ends** (ruling 226): after the connection dies it yields
/// `Some(Err(ConnectionLost))` for as long as it is polled, so a bare
/// `while let Some(_) = s.next().await` **spins**. Compose the terminating
/// shape:
///
/// ```text
/// let mut msgs = conn.messages().take_while(|r| ready(r.is_ok()));
/// ```
///
/// **Borrows** its [`Connection`] (ruling 231), so it cannot be moved into
/// `spawn_local`; move the `Connection` in and build it inside the task.
pub struct Messages<'a, S: Handshake> {
    conn: &'a Connection<S>,
    /// Held for the adapter's whole life and released on drop — the whole of
    /// the underlying verb's cancel-safety, and the reason ruling 228 boxes
    /// the release closure so this field can be named.
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Stream for Messages<'_, S> {
    type Item = Result<Vec<u8>, ConnectionLost>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Ruling 58: one call, one item, and nothing kept.
        let this = self.get_mut();
        this.conn.poll_recv_message(cx, this.slot.key()).map(Some)
    }
}

/// The [`Stream`] returned by [`Connection::datagrams`].
///
/// **Never ends** (ruling 226) — a bare `while let Some(_)` **spins**; use
/// `.take_while(|r| ready(r.is_ok()))`. **Borrows** its [`Connection`]
/// (ruling 231): move the `Connection` into `spawn_local` and build this
/// inside the task.
pub struct Datagrams<'a, S: Handshake> {
    conn: &'a Connection<S>,
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Stream for Datagrams<'_, S> {
    type Item = Result<Vec<u8>, ConnectionLost>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.conn.poll_recv_datagram(cx, this.slot.key()).map(Some)
    }
}

/// The [`Stream`] returned by [`Connection::incoming_bi`].
///
/// **Never ends** (ruling 226) — a bare `while let Some(_)` **spins**; use
/// `.take_while(|r| ready(r.is_ok()))`. **Borrows** its [`Connection`]
/// (ruling 231): move the `Connection` into `spawn_local` and build this
/// inside the task.
pub struct IncomingBi<'a, S: Handshake> {
    conn: &'a Connection<S>,
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Stream for IncomingBi<'_, S> {
    type Item = Result<BiStream<S>, ConnectionLost>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.conn.poll_accept_bi(cx, this.slot.key()).map(Some)
    }
}

/// The [`Stream`] returned by [`Connection::incoming_uni`].
///
/// **Never ends** (ruling 226) — a bare `while let Some(_)` **spins**; use
/// `.take_while(|r| ready(r.is_ok()))`. **Borrows** its [`Connection`]
/// (ruling 231): move the `Connection` into `spawn_local` and build this
/// inside the task.
///
/// **S30 / ruling 51:** this and [`Connection::messages`] draw from the same
/// supply and must not both be used on one connection.
pub struct IncomingUni<'a, S: Handshake> {
    conn: &'a Connection<S>,
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Stream for IncomingUni<'_, S> {
    type Item = Result<RecvStream<S>, ConnectionLost>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.conn.poll_accept_uni(cx, this.slot.key()).map(Some)
    }
}

/// The [`Stream`] returned by [`Connection::notifications`].
///
/// **Never ends** (ruling 226) — a bare `while let Some(_)` **spins**; use
/// `.take_while(|r| ready(r.is_ok()))`. **Borrows** its [`Connection`]
/// (ruling 231): move the `Connection` into `spawn_local` and build this
/// inside the task.
pub struct Notifications<'a, S: Handshake> {
    conn: &'a Connection<S>,
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Stream for Notifications<'_, S> {
    type Item = Result<Notification, ConnectionLost>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.conn.poll_notified(cx, this.slot.key()).map(Some)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The endpoint's face — the one that ends
// ═══════════════════════════════════════════════════════════════════════

/// The [`Stream`] returned by [`Endpoint::incoming`].
///
/// **This one ends.** Its item is an [`Intro`] rather than a `Result`, and
/// `None` means the endpoint is closed — the driver has stopped — so
/// `while let Some(intro) = incoming.next().await` is correct here and does
/// not spin.
///
/// It holds a `oneshot::Receiver` rather than a boxed in-flight future
/// (**ruling 229**), which is what keeps it `Unpin` and keeps the adapter
/// clear of the cost §16.3 names as the thing ruling 53 exists to avoid.
///
/// **Dropping it with a request outstanding may drop one `Intro`** — §6.2's
/// silent reject, *"the documented meaning of dropping a staged object, not
/// a loss"*.
///
/// **Borrows** its [`Endpoint`] (ruling 231).
pub struct Incoming<'a, I: Identity> {
    endpoint: &'a Endpoint<I>,
    /// The outstanding request, between polls. `Unpin`, so this adapter is.
    pending: Option<tokio::sync::oneshot::Receiver<Intro<I>>>,
}

impl<I: Identity> Stream for Incoming<'_, I> {
    type Item = Intro<I>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Ruling 58 holds here too: at most one introduction is claimed, and
        // only from inside this call.
        let this = self.get_mut();
        this.endpoint.poll_accept(cx, &mut this.pending)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The two sinks
// ═══════════════════════════════════════════════════════════════════════

/// The [`Sink`] returned by [`Connection::message_sink`].
///
/// Holds **at most one** pending payload — `Sink`'s protocol requires it,
/// because `start_send` may not block while
/// [`Connection::send_message`] is `async`. That slot is the only buffer in
/// `compat/`, and it is on the send side.
///
/// **Borrows** its [`Connection`] (ruling 231).
pub struct MessageSink<'a, S: Handshake> {
    conn: &'a Connection<S>,
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
    /// `Sink`'s single required slot. **Not a queue** — `start_send` is only
    /// permitted after a `poll_ready` that emptied it.
    pending: Option<Vec<u8>>,
}

impl<S: Handshake> MessageSink<'_, S> {
    /// Drive the pending payload, if any, to a decision.
    ///
    /// `Ready(Ok(()))` exactly when the slot is empty. The slot is cleared
    /// on an error as well as on success: a `Sink` that reported an error
    /// and then re-drove the same doomed payload on the next `poll_ready`
    /// would never make progress.
    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), MessageError>> {
        let Some(msg) = self.pending.as_deref() else {
            return Poll::Ready(Ok(()));
        };
        let outcome = ready!(self.conn.poll_send_message(cx, msg, self.slot.key()));
        self.pending = None;
        Poll::Ready(outcome)
    }
}

impl<S: Handshake> Sink<Vec<u8>> for MessageSink<'_, S> {
    type Error = MessageError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().poll_drain(cx)
    }

    /// Stores the payload. **Never blocks and never touches the core** —
    /// the work happens in `poll_ready`/`poll_flush`.
    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        let this = self.get_mut();
        debug_assert!(
            this.pending.is_none(),
            "Sink::start_send without a preceding Ready poll_ready overwrites the slot"
        );
        this.pending = Some(item);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().poll_drain(cx)
    }

    /// Flushes, and **does not close the connection**.
    ///
    /// A `Sink` adapter that tore down a multiplexer because one of its
    /// faces was dropped would end every stream, every other message reader
    /// and the datagram path with it. [`Connection::close`] is the verb that
    /// means close.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().poll_drain(cx)
    }
}

/// The [`Sink`] returned by [`Connection::datagram_sink`].
///
/// Buffers **nothing**: [`Connection::send_datagram`] is synchronous, so
/// `start_send` does the whole of the work and `poll_ready` is always
/// `Ready`. §11.1 promises nothing about a datagram beyond its entering the
/// queue — no delivery, no ordering, no retransmission — and this face
/// promises exactly as much.
///
/// **Borrows** its [`Connection`] (ruling 231).
pub struct DatagramSink<'a, S: Handshake> {
    conn: &'a Connection<S>,
}

impl<S: Handshake> Sink<Vec<u8>> for DatagramSink<'_, S> {
    type Error = DatagramError;

    /// Always `Ready`: there is nothing to wait for.
    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        self.get_mut().conn.send_datagram(&item)
    }

    /// Nothing is buffered, so there is nothing to flush.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// **Does not close the connection** — [`MessageSink`]'s reason.
    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// `Debug`, for all eight
// ═══════════════════════════════════════════════════════════════════════
//
// **[RATIFIED 2026/08/18 — ruling 259(v)]** Rust API guideline C-DEBUG:
// every public type is `Debug`. All eight are written by hand rather than
// derived, for two reasons, both of which apply to every one of them.
//
// 1. Most hold a [`WakerSlot`], whose payload here is a
//    `Box<dyn FnMut(u64)>`. A closure is never `Debug`, so a derive does
//    not compile.
// 2. A derive on a generic type emits `impl<S: Handshake + Debug>`, so the
//    impl would silently vanish for any suite whose type is not itself
//    `Debug` — and §16.2's surface must be `Debug` for *every* suite.
//
// The output is the shape the rest of the crate already uses (`Endpoint`,
// `SendStream`, `Connection`): the type's name, whatever is cheap and
// non-secret, and `finish_non_exhaustive`. **None of these holds key
// material** — the peer static and the session id live behind the
// `Connection` they borrow, whose own hand-written `Debug` prints neither.

impl<S: Handshake> fmt::Debug for Messages<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Messages").finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for Datagrams<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Datagrams").finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for IncomingBi<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncomingBi").finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for IncomingUni<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncomingUni").finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for Notifications<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notifications").finish_non_exhaustive()
    }
}

impl<I: Identity> fmt::Debug for Incoming<'_, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Whether a request is outstanding is the one thing worth seeing
        // here: it is what decides whether dropping this adapter may drop
        // an `Intro` — §6.2's silent reject.
        f.debug_struct("Incoming")
            .field("request_outstanding", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for MessageSink<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The slot's **occupancy**, never its bytes: a pending payload is
        // application plaintext.
        f.debug_struct("MessageSink")
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl<S: Handshake> fmt::Debug for DatagramSink<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatagramSink").finish_non_exhaustive()
    }
}

//! §16.11's composability surface: slither's verbs, in the shapes the async
//! ecosystem consumes.
//!
//! §16.2's verbs are the whole of the protocol surface. **This module adds
//! no verb and no state** — it fixes the shape in which those verbs meet
//! `tokio::io`, `futures`, `tokio_util::codec` and `tower`. Everything here
//! is shell-layer: no core type, no wire byte, no timer.
//!
//! ```text
//! core::Endpoint / core::Connection    pure state machines  (§16.4)
//!         ↑
//! shell: one !Send driver task, and the handles                (§16.3)
//!         ↑
//! compat: AsyncRead/Write · Stream/Sink · Framed · Service    (§16.11)
//! ```
//!
//! # A `Connection` is a multiplexer, so the byte-oriented object is a stream
//!
//! That substitution is the whole of the mapping and the rest follows: the
//! `AsyncRead`/`AsyncWrite` impls are on [`SendStream`](crate::SendStream),
//! [`RecvStream`](crate::RecvStream) and [`BiStream`](crate::BiStream), never
//! on a `Connection`.
//!
//! # What is here
//!
//! | Surface | Gate | What it adds |
//! |---|---|---|
//! | [`io`] | none | `From<ReadError>`/`From<WriteError>` for [`std::io::Error`], and `AsyncRead`/`AsyncWrite` on the three stream handles |
//! | [`rt`] | none | [`block_on`] — a current-thread runtime inside a `LocalSet` |
//! | `stream` | `sink` | six `Stream`s and two `Sink`s over §16.2's verbs |
//! | `codec` | `codec` | `Framed` constructors over a bidirectional stream |
//! | `tower` | `tower` | the two `Service` shapes and `serve` |
//!
//! The adapter types are named from `slither::compat::*`; they are not
//! re-exported at the crate root. [`block_on`] is the exception, because it
//! is the first thing a consumer needs.
//!
//! # The three rules that bound every adapter here
//!
//! 1. **An adapter never claims ahead of its consumer** (ruling 58,
//!    *normative*). At most one item, and only from inside `poll_next`. No
//!    prefetch, no read-ahead task, no intermediate queue — anything else
//!    rebuilds the unbounded shell queue §10.6 forbids **while looking like
//!    an ordinary ergonomic convenience**. The only buffer in this module is
//!    `MessageSink`'s single slot, which `Sink`'s own protocol requires on
//!    the *send* side.
//! 2. **No verb is implemented twice** (ruling 53). Every adapter calls an
//!    existing `poll_*`; §16.11's `AsyncRead`/`AsyncWrite` is *"the same
//!    function with its error mapped"*.
//! 3. **No `Send` bound, anywhere** (S21). The driver is `!Send` by
//!    requirement, and `spawn_local` is the only spawn used.
//!
//! # The `Result`-carrying faces never end — **ruling 226**
//!
//! Every `Stream` here whose item is a `Result` yields
//! `Some(Err(ConnectionLost))` for as long as it is polled after the
//! connection dies, and **never `None`**: the underlying `poll_*` re-report
//! the latched death indefinitely, and that keeps the *reason* recoverable,
//! which a `None` destroys. **A bare `while let Some(_) = s.next().await`
//! spins.** `Incoming` is the exception — its item is not a `Result` and
//! its `None` means *the endpoint is closed*.
//!
//! # Every adapter borrows — **ruling 231**
//!
//! Neither [`Connection`](crate::Connection) nor [`Endpoint`](crate::Endpoint)
//! is `Clone`, and a `Connection`'s last-handle drop performs
//! `close(NO_ERROR, "")`, so an owning adapter would change when a
//! connection ends. Every adapter therefore carries a lifetime, and the
//! consequence a consumer meets is that a **borrowed adapter cannot be moved
//! into `spawn_local`**: move the handle into the task and build the adapter
//! inside it.

pub mod io;
pub mod rt;

pub use self::rt::block_on;

#[cfg(feature = "sink")]
pub mod stream;

#[cfg(feature = "sink")]
pub use self::stream::{
    DatagramSink, Datagrams, Incoming, IncomingBi, IncomingUni, MessageSink, Messages,
    Notifications,
};

#[cfg(feature = "codec")]
pub mod codec;

#[cfg(feature = "tower")]
pub mod tower;

#[cfg(feature = "tower")]
pub use self::tower::{Connect, OpenBi, serve};

// Slice 8. The integrator's to uncomment (working rules 6 and 15): the file
// is the spec-test author's alone, and `mod tests;` naming a missing file is
// a compile error on which no gate can run.
// #[cfg(test)]
// mod tests;

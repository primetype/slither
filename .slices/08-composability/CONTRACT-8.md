# CONTRACT-8 — binding API contract for slice 8 (`compat/`)

**Base commit:** `5ed5bb4`.

**This file is binding on every slice-8 agent.** A blind test author must be
able to write compiling tests from this file alone (rule 14: slice 4a's author
guessed an API for ten minutes because its contract was uncommitted, and two of
its guesses were semantic).

**Nothing in slice 8 changes a handle signature** — ruling 204 ratified the
public handle API as it stands; the adapters go on top of a frozen surface.

---

## 0. Module layout and feature gates

```
src/compat/
    mod.rs      — the module, its rustdoc, and the re-exports        (no gate)
    io.rs       — From<ReadError>/From<WriteError>, AsyncRead/AsyncWrite (no gate)
    rt.rs       — block_on                                            (no gate)
    stream.rs   — Stream/Sink adapters              #[cfg(feature = "sink")]
    codec.rs    — Framed constructors               #[cfg(feature = "codec")]
    tower.rs    — Service shapes                    #[cfg(feature = "tower")]
```

`src/lib.rs` gains `pub mod compat;` and re-exports `block_on` at the crate
root. The adapter **types** are named from `slither::compat::*`; they are not
re-exported at the crate root.

**The three features already exist in `Cargo.toml` at `5ed5bb4`** with their
optional dependencies. No agent adds a feature or a dependency:

```toml
sink  = ["dep:futures-core", "dep:futures-sink"]
codec = ["sink", "dep:tokio-util"]     # implies sink
tower = ["dep:tower-service"]          # does NOT imply sink or codec
```

Crate-wide constraints that apply to every line of `compat/`:

- `#![forbid(unsafe_code)]` — no `unsafe`, including for `ReadBuf`'s
  uninitialised tail. Use `ReadBuf::initialize_unfilled_to(n)` and
  `ReadBuf::advance(n)`.
- `#![warn(missing_docs)]` plus `RUSTDOCFLAGS=-D warnings` — every public item
  needs a doc comment, and rulings 56 and 57 require **specific** text at
  their impls.
- MSRV **1.96**. `std::task::Waker::noop()` is available and is the intended
  instrument for the ruling-58 pin.
- **No `Send` bound anywhere.** Not on a type, not on a where-clause, not on
  a `Box<dyn …>`. S21.

### Generic parameters — the thing `PLAN.md` §3 omits

Every handle is generic and `PLAN.md` §3's sketch drops the parameter. The
code is the authority (ruling 204):

| Handle | Real declaration |
|---|---|
| `Endpoint` | `Endpoint<I: Identity>` |
| `Connecting` | `Connecting<I: Identity>`, `Future<Output = Result<Connection<I::Suite>, ConnectError>>` |
| `Intro` | `Intro<I: Identity>` |
| `Connection` | `Connection<S: Handshake>` |
| `SendStream` / `RecvStream` / `BiStream` | `…<S: Handshake>` |

`I::Suite` is the `Handshake` associated with an `Identity`
(`identity.rs:75`, `CurveOf<I> = <<I as Identity>::Suite as Channel>::Curve`).
**In `compat/tower.rs` the handshake parameter is named `H`, not `S`**, because
`S` is taken by the service type in `PLAN.md` §3.4's sketch.

**Neither `Connection<S>` nor `Endpoint<I>` implements `Clone`** (only the
internal `Shell<I>` does), and `Connection`'s last-handle drop performs
`close(NO_ERROR, "")`. **Every `Stream`/`Sink` adapter therefore borrows its
handle** and carries a lifetime. See PLAN-8 Open question 10.

## 1. Existing handle surface (frozen — quoted, not changed)

**Every stream handle is generic over `S: Handshake`.** `PLAN.md` §3 and
§16.11 both sketch these types *without* their generic parameter. The code is
the authority on the parameter (ruling 204 ratified these signatures), and
every adapter in slice 8 must carry `<S: Handshake>` through. **This is the
single most likely thing for a blind test author to get wrong from
`PLAN.md` §3 alone.**

### 1.1 `src/shell/stream.rs`

```rust
pub struct SendStream<S: Handshake> { /* … */ }

impl<S: Handshake> SendStream<S> {
    pub fn id(&self) -> Option<StreamId>;
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError>;
    pub async fn finish(&mut self) -> Result<(), WriteError>;
    pub async fn acked(&mut self) -> Result<(), WriteError>;
    pub fn reset(&mut self, error_code: u64);

    // pub(crate) — ruling 122(a); `compat/` is in-crate and reaches them.
    pub(crate) fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8])
        -> Poll<Result<usize, WriteError>>;
    pub(crate) fn poll_finish(&mut self, _cx: &mut Context<'_>)
        -> Poll<Result<(), WriteError>>;
    pub(crate) fn poll_acked(&mut self, cx: &mut Context<'_>)
        -> Poll<Result<(), WriteError>>;
}

pub struct RecvStream<S: Handshake> { /* … */ }

impl<S: Handshake> RecvStream<S> {
    pub fn id(&self) -> Option<StreamId>;
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;

    pub(crate) fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<Result<Option<usize>, ReadError>>;
}

pub struct BiStream<S: Handshake> { /* send + recv, in that field order */ }

impl<S: Handshake> BiStream<S> {
    pub(crate) fn new(send: SendStream<S>, recv: RecvStream<S>) -> Self;
    pub fn id(&self) -> Option<StreamId>;
    pub fn split(self) -> (SendStream<S>, RecvStream<S>);
    pub fn join(send: SendStream<S>, recv: RecvStream<S>)
        -> Result<Self, (SendStream<S>, RecvStream<S>)>;   // ruling 120
}
```

**Facts from the existing rustdoc that slice 8 depends on:**

- `poll_finish` is **always `Ready` on the first poll**, and its own rustdoc
  names the reason and names slice 8: *"§16.7 makes sealing synchronous
  inside the mutating call that triggers it … slice 8's
  `AsyncWrite::poll_shutdown` is this function with its error mapped."*
- `poll_read`'s rustdoc names slice 8 too: *"`AsyncRead` requires a sticky
  EOF or `read_to_end` breaks. The core is not sticky and would answer
  `Ok(None)` to a re-read after a reset, which is why the latch lives here."*
  The handle latches `Ended::Eof` / `Ended::Reset(code)` (rulings 121, 124).
- `poll_acked` is **a pure read**: it mutates no core state, marks nothing
  dirty, wakes no driver.
- `BiStream` has **no `Drop` of its own**; its two fields drop in declaration
  order — send half first (RESET_STREAM unless finished), then receive half.
  **An adapter must not add a `Drop` to a wrapper around `BiStream`** without
  reproducing both halves' rules; the existing rustdoc records that a build
  which implemented `Drop` on `BiStream` and forgot one half *"would pass any
  test that checked only the other."*
- Both `write`/`read` futures are documented **cancel-safe**: a dropped
  future has claimed nothing, and there is **no shell-side scratch buffer**
  (§10.6). Slice 8 must not introduce one.

## 2. `compat::io` — error conversions

```rust
impl From<ReadError>  for std::io::Error { fn from(e: ReadError)  -> Self; }
impl From<WriteError> for std::io::Error { fn from(e: WriteError) -> Self; }
```

Ungated. §16.11 declares both. **No third conversion** — `ConnectionLost`,
`MessageError` and `DatagramError` get **no** `From<…> for io::Error`: they
are not part of §16.11's two-line block, and working rule 8 reads that list as
exhaustive.

**The original error is preserved as the `io::Error`'s inner value**, so a
caller recovers it:

```rust
let e: std::io::Error = ReadError::Reset(7).into();
assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
let inner = e.into_inner().expect("slither error preserved");
assert_eq!(*inner.downcast::<ReadError>().unwrap(), ReadError::Reset(7));
```

This is **binding and testable**: build each `io::Error` with
`io::Error::new(kind, err)`, never `io::Error::from(kind)`. Without it the
code that only `AsyncRead` can reach — a stream reset code — is unrecoverable,
and S31's *"a reset surfaces as `ConnectionReset` rather than a silent
truncation"* is satisfied in kind but not in detail.

The exact kind for every variant is **§8's table**, which is the normative
part of this section.

## 3. `compat::io` — `AsyncRead`/`AsyncWrite` impls

Ungated — tokio is already a hard dependency and the traits themselves need no
tokio feature.

```rust
impl<S: Handshake> tokio::io::AsyncWrite for SendStream<S> { … }
impl<S: Handshake> tokio::io::AsyncRead  for RecvStream<S> { … }
impl<S: Handshake> tokio::io::AsyncRead  for BiStream<S>   { … }
impl<S: Handshake> tokio::io::AsyncWrite for BiStream<S>   { … }
```

All four handles are `Unpin` (plain structs of `Rc`s and `Copy` fields), so
each impl begins `let this = self.get_mut();` and never projects a pin.
**The impls add no field, no `Drop` and no state.**

### 3.1 `AsyncWrite for SendStream<S>` — exact mapping

| Call | Behaviour |
|---|---|
| `poll_write(cx, buf)` | `self.poll_write(cx, buf)` with `Err(e) → Err(io::Error::from(e))`. `Ok(n) → Ok(n)` unchanged. |
| `poll_flush(_cx)` | **`Poll::Ready(Ok(()))`, unconditionally.** Touches nothing — not the cell, not the core, not the driver. |
| `poll_shutdown(cx)` | `ready!(self.poll_finish(cx))?` **then** `self.poll_acked(cx)`, both errors mapped. |
| `poll_write_vectored` | **not implemented** (default). `is_write_vectored()` stays `false`. |

**Ruling 56 is a rustdoc requirement as well as a behaviour.** §16.11: *"`AsyncWrite::flush` is **not** delivery confirmation — promising otherwise
would make it a second, weaker `acked()` and put the two in competition.
**Rustdoc must say so at the impl.**"* The doc comment on `poll_flush` must
state (a) that it is a no-op, (b) *why* — `poll_write` accepts only what
flow-control credit admits, so accepted bytes are already in send state and
there is no shell buffer to push — and (c) that
[`SendStream::acked`] is the verb that means acknowledged.

**Ruling 57's shutdown needs no stored state**, and that is a fact about the
existing code rather than a shortcut: `poll_finish` is *"always `Ready` on the
first poll"* (§16.7 seals synchronously) and `finish()` is idempotent — *"a
second `finish()` is `Ok(())`"* — so re-entering `poll_shutdown` after a
`Pending` from `poll_acked` calls `poll_finish` again harmlessly. **Do not add
a `shutdown_started: bool`.**

Shutdown cannot hang past the connection's death: a dying connection resolves
`poll_acked` in error (ruling 57, *"Bounded by connection death"*).

### 3.2 `AsyncRead for RecvStream<S>` — exact mapping

```rust
fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>)
    -> Poll<std::io::Result<()>>
```

| Core result | `AsyncRead` result |
|---|---|
| `Poll::Pending` | `Poll::Pending` |
| `Ready(Ok(None))` — end of stream | `Ready(Ok(()))`, **nothing filled** (this *is* `AsyncRead`'s EOF) |
| `Ready(Ok(Some(n)))`, `n > 0` | `buf.advance(n)`; `Ready(Ok(()))` |
| `Ready(Err(e))` | `Ready(Err(e.into()))` per §8 |

**Two hazards, both binding:**

1. **The adapter short-circuits `buf.remaining() == 0` itself, returning
   `Ready(Ok(()))` without touching the handle.** It must never hand
   `RecvStream::poll_read` an empty slice. Ruling 119 makes an empty `buf`
   return `Ok(Some(0))`, whose documented meaning is *park* — and once mapped
   into a `ReadBuf` that filled nothing, it is **indistinguishable from EOF**.
   `Ok(Some(0))` = park and `Ok(None)` = end of stream is the distinction
   ruling 119 exists to protect, and *"a blind test author has already
   guessed wrong once"* on it (round 18, and again in slice 4a). The
   `AsyncRead` surface is where the two collide, so the collision is removed
   before the call, not after it.
2. **No scratch buffer.** Fill `ReadBuf`'s tail directly via
   `initialize_unfilled_to(buf.remaining())` (safe; the crate forbids
   `unsafe`) and pass that slice to `poll_read`. `RecvStream::read`'s existing
   rustdoc pins the reason: *"there is no shell-side scratch buffer (§10.6),
   so there is nowhere for a dropped future to strand data."*

**Verified against the bodies at `5ed5bb4`, so the table above is sound
rather than plausible** (working rule 11 — open the file the rationale
describes):

- `RecvStream::poll_read` returns `Ready(Ok(Some(0)))` **only** on the
  `buf.is_empty()` short-circuit. A core answer of *"no data available"*
  becomes `Poll::Pending` (or a `ConnectionLost` error if the cell is
  latched), never `Ok(Some(0))`. So once the adapter short-circuits
  `remaining() == 0`, that row is unreachable from `compat/` — **map it to
  "fill nothing, `Ready(Ok(()))`" anyway; do not write `unreachable!()`.**
  A panic reachable only through a future refactor is worse than a
  defensive arm that costs nothing.
- `SendStream::poll_write` returns `Ready(Ok(0))` **only** on the
  `buf.is_empty()` short-circuit (ruling 110). A non-empty write blocked by
  flow-control credit parks in `blocked_writers` and returns `Poll::Pending`.
  So `AsyncWrite::poll_write` never returns `Ok(0)` for a non-empty buffer,
  and no `WriteZero` guard is needed or wanted.

**Sticky terminal state is already in `RecvStream` and must not be
re-implemented.** Rulings 121 and 124 latch `Ended::Eof` / `Ended::Reset(code)`
in the handle, ahead of the connection's death latch. SPEC.md:4805 states why
this is slice 8's concern: *"`AsyncRead` requires a sticky end-of-file, so a
connection that dies after the FIN would otherwise surface a spurious
`io::Error` to `read_to_end`."*

### 3.3 `BiStream<S>` — both traits, delegating

- `AsyncRead` delegates to the **receive** half.
- `AsyncWrite` delegates to the **send** half.
- **`poll_shutdown` shuts down the send half only.** It does not touch,
  abandon or reset the receive half — a half-closed `BiStream` is the shape
  `copy_bidirectional` and every request/response protocol relies on.
- **No `Drop` is added.** `BiStream`'s existing rustdoc: it *"has **no `Drop`
  impl of its own** — its two fields' do the work, in declaration order"*, and
  records that a build which implemented `Drop` on `BiStream` and forgot one
  half *"would pass any test that checked only the other."*

Delegation needs a `pub(crate)` accessor pair on `BiStream`
(`fn send_mut(&mut self) -> &mut SendStream<S>`, `fn recv_mut(&mut self) ->
&mut RecvStream<S>`) unless `compat/io.rs`'s impls are written so they can
reach the private fields. **Adding those two `pub(crate)` accessors in
`src/shell/stream.rs` is Agent A's**, and is the only change that file needs.

## 4. `compat::stream` — `Stream`/`Sink` adapters (`feature = "sink"`)

Traits are `futures_core::Stream` and `futures_sink::Sink`. The constructors
are **inherent methods written in `src/compat/stream.rs`** — Rust permits an
inherent `impl` block for a crate-local type in any module of that crate, so
`src/shell/connection.rs` and `src/shell/endpoint.rs` need **no edit for
this**.

Every adapter type: `#[cfg(feature = "sink")]`, borrows its handle, is `Unpin`
(no `!Unpin` field), holds **one** `WakerSlot` for its whole life, and holds
**no item buffer** on the receive side.

```rust
#[cfg(feature = "sink")]
impl<S: Handshake> Connection<S> {
    pub fn messages(&self)      -> Messages<'_, S>;
    pub fn datagrams(&self)     -> Datagrams<'_, S>;
    pub fn incoming_bi(&self)   -> IncomingBi<'_, S>;
    pub fn incoming_uni(&self)  -> IncomingUni<'_, S>;
    pub fn notifications(&self) -> Notifications<'_, S>;
    pub fn message_sink(&self)  -> MessageSink<'_, S>;
    pub fn datagram_sink(&self) -> DatagramSink<'_, S>;
}

#[cfg(feature = "sink")]
impl<I: Identity> Endpoint<I> {
    pub fn incoming(&self) -> Incoming<'_, I>;
}
```

### 4.1 The eight item types

| Type | Trait | Item / parameter | Backed by |
|---|---|---|---|
| `Messages<'a, S>` | `Stream` | `Result<Vec<u8>, ConnectionLost>` | `Connection::poll_recv_message` |
| `Datagrams<'a, S>` | `Stream` | `Result<Vec<u8>, ConnectionLost>` | `Connection::poll_recv_datagram` |
| `IncomingBi<'a, S>` | `Stream` | `Result<BiStream<S>, ConnectionLost>` | `Connection::poll_accept_bi` |
| `IncomingUni<'a, S>` | `Stream` | `Result<RecvStream<S>, ConnectionLost>` | `Connection::poll_accept_uni` |
| `Notifications<'a, S>` | `Stream` | `Result<Notification, ConnectionLost>` | `Connection::poll_notified` |
| `Incoming<'a, I>` | `Stream` | `Intro<I>` — **not** a `Result` | `Endpoint::accept` (see 4.3) |
| `MessageSink<'a, S>` | `Sink<Vec<u8>>` | `Error = MessageError` | `Connection::poll_send_message` |
| `DatagramSink<'a, S>` | `Sink<Vec<u8>>` | `Error = DatagramError` | `Connection::send_datagram` (sync) |

`Vec<u8>` is the concrete "Bytes" of `PLAN.md` §3.2 — slither has no `bytes`
dependency and is not acquiring one.

### 4.2 Stream termination — binding, and easy to get wrong

**Subject to PLAN-8 Open question 9.** Recommendation: the five
`Result`-carrying streams **do not end** — `poll_next` yields
`Some(Err(ConnectionLost))` for ever once the connection dies and never yields
`None`. Two reasons, both checked against the code:

- `ConnectionLost` is `Clone` *because* *"a connection dies once and the same
  value is handed to N awaiting futures and to every later verb call"*
  (`src/error.rs:193`). The underlying `poll_*` re-report it indefinitely.
- Ending the stream instead would discard the death **reason**, which is the
  only thing a consumer draining `messages()` ever learns about why it
  stopped.

The alternative — yield `Some(Err(lost))` **once**, then `None` — costs one
`bool` per adapter (not a queue, so ruling 58 is untouched), gives
`while let Some(item) = s.next().await` a terminating shape, and matches what
`Framed` does. It is the better ergonomics and the worse fidelity to the
underlying verb. **Q9 decides; a blind test author must not be left to guess
which, because it is exactly the `Ok(Some(0))`/`Ok(None)` class of mistake
ruling 119 was written about.**

`Incoming<'a, I>` is the exception and **does** end under either answer:
`Endpoint::accept()`
returns `Option<Intro<I>>` where *"`None` means the endpoint is closed — the
driver has stopped"*, so `poll_next` yields `None` there and is terminated
from then on.

### 4.3 `Incoming` — the one adapter with no `poll_*` behind it

**Subject to PLAN-8 Open question 3; this section is the recommended shape.**
`Endpoint::accept()` is `oneshot`-backed (ruling 53's channel side — §6.2
requires the DH on the driver task), so there is no `poll_accept`. Agent A
adds one:

```rust
// src/shell/endpoint.rs — pub(crate), and accept() becomes poll_fn over it.
pub(crate) fn poll_accept(
    &self,
    cx: &mut Context<'_>,
    pending: &mut Option<tokio::sync::oneshot::Receiver<Intro<I>>>,
) -> Poll<Option<Intro<I>>>;
```

`Incoming<'a, I>` then holds `&'a Endpoint<I>` plus
`Option<oneshot::Receiver<Intro<I>>>` — `Unpin`, no boxed future. If the
maintainer declines, `Incoming` instead holds
`Option<Pin<Box<dyn Future<Output = Option<Intro<I>>> + 'a>>>` and everything
else in this section is unchanged.

**Rustdoc obligation:** dropping an `Incoming` with a request outstanding may
drop one `Intro`, which is §6.2's **silent reject** — `accept()`'s own rustdoc
already calls that *"the documented meaning of dropping a staged object, not a
loss"*. Say so on `Incoming`.

### 4.4 The sinks

**`MessageSink`** holds **at most one** pending `Vec<u8>`, because `Sink`'s
protocol requires it (`start_send` may not block) and `send_message` is
async. That single slot is the only buffer permitted anywhere in `compat/`.

| Call | Behaviour |
|---|---|
| `poll_ready(cx)` | If a message is buffered, drive `poll_send_message` on it; `Ready(Ok(()))` when the slot is empty, `Pending` while it is not. |
| `start_send(item)` | Store `item`. Never blocks, never touches the core. |
| `poll_flush(cx)` | Drive the buffered message to completion; `Ready(Ok(()))` when the slot is empty. |
| `poll_close(cx)` | `poll_flush`, then `Ready(Ok(()))`. **Does not close the connection.** |

`poll_close` closing the connection would be a `Sink` adapter tearing down a
multiplexer because one of its faces was dropped. It must not.

**`DatagramSink`** buffers **nothing** — `send_datagram` is synchronous
(ruling 204 item 5: *"`send_datagram` stays sync, `send_message` stays async …
the asymmetry is the type system saying what it can"*):

| Call | Behaviour |
|---|---|
| `poll_ready(_)` | `Ready(Ok(()))`, always. |
| `start_send(item)` | `self.conn.send_datagram(&item)` — returns `DatagramError` directly. |
| `poll_flush(_)` / `poll_close(_)` | `Ready(Ok(()))`. |

`DatagramSink` needs **no `WakerSlot`**.

### 4.5 Ruling 58, restated as the acceptance criterion for this module

*"A `Stream` adapter … claims **at most one item, and only from inside
`poll_next`**. No prefetch, no read-ahead task, no intermediate queue."*

Concretely, for every one of the six streams:

- `poll_next` calls the underlying `poll_*` **exactly once** per invocation.
- On `Ready`, it returns that item and **stops**. It does not loop to fill
  anything.
- The struct has **no `VecDeque`, no `Vec`, no `Option<Item>` receive slot,
  and spawns no task.** `src/shell/shared.rs`'s own rustdoc says it from the
  other end: *"An implementer who reaches for a `VecDeque` has rebuilt the
  unbounded intermediate queue §10.6 forbids."*

### 4.6 S30's rustdoc obligation (ruling 51)

The doc comment on **`messages()`** and on **`incoming_uni()`** must each say
that the two draw from the same supply and must not both be used on one
connection, that mixing them is **normatively a programming error** because
the wire carries no discriminator, and that `open_bi()`/`incoming_bi()`
alongside messages is the safe alternative. This is documentation obligation
#4's shape, and S30 records that *"the safe and unsafe shapes look alike at
the call site."*

## 5. `compat::codec` (`feature = "codec"`)

`PLAN.md` §3.3: the constructors *"exist only to remove a `use`"*, and the
result is *"identical to `Framed::new(conn.open_bi().await?, codec)`"*.

```rust
// src/compat/codec.rs — an inherent impl block on Connection, in this module.
#[cfg(feature = "codec")]
impl<S: Handshake> Connection<S> {
    /// `Framed::new(self.open_bi().await?, codec)`.
    pub async fn framed_bi<C>(&self, codec: C)
        -> Result<tokio_util::codec::Framed<BiStream<S>, C>, ConnectionLost>;

    /// `Framed::new(self.accept_bi().await?, codec)`.
    pub async fn accept_framed_bi<C>(&self, codec: C)
        -> Result<tokio_util::codec::Framed<BiStream<S>, C>, ConnectionLost>;
}
```

`C` carries **no bound** on these constructors — `Framed::new` requires none;
the `Encoder`/`Decoder` bounds appear on `Framed`'s own `Sink`/`Stream` impls
where the user meets them.

**Exactly these two constructors are in scope.** There is deliberately **no**
`framed_uni` / `accept_framed_uni` (`FramedWrite`/`FramedRead` over the two
half-handles): they are not in `PLAN.md` §3.3, and a consumer writes
`FramedWrite::new(conn.open_uni().await?, codec)` in one line. **Do not write
tests against a `framed_uni` — it does not exist in this slice.**

`Framed<BiStream<S>, C>` works only because §3's `AsyncRead + AsyncWrite`
impls exist on `BiStream<S>`; that is the whole of the dependency, and it is
why the `codec` feature implies `sink` but the *type* needs nothing from
`compat/stream.rs`.

## 6. `compat::tower` (`feature = "tower"`)

**Subject to PLAN-8 Open questions 4 and 5.** The trait is
`tower_service::Service`.

### 6.1 The dialer — `Endpoint<I>` itself

```rust
#[cfg(feature = "tower")]
impl<I: Identity> tower_service::Service<(SocketAddr, PublicKeyOf<I>)> for Endpoint<I> {
    type Response = Connection<I::Suite>;
    type Error    = ConnectError;
    type Future   = Connect<I>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;  // always Ready(Ok(()))
    fn call(&mut self, req: (SocketAddr, PublicKeyOf<I>)) -> Self::Future;
}

/// The future `Endpoint`'s `Service` returns.
#[cfg(feature = "tower")]
pub struct Connect<I: Identity>(/* Connecting<I> | Option<ConnectError> */);

impl<I: Identity> Future for Connect<I> {
    type Output = Result<Connection<I::Suite>, ConnectError>;
}
```

This impl works **on the owned type** because `Endpoint::connect` is *not*
`async` and the `Connecting<I>` it returns owns everything it needs
(`Shell<I>` clone plus an `Rc` slot) — it borrows nothing from `&self`.
`Connect<I>` exists only to fold `connect`'s **synchronous** `Err` (e.g.
`ConnectError::AlreadyConnected`, which ruling 87/90 make arrive *before any
await*) into a future, since `Service::call` cannot return a `Result`.

`Connect<I>` **must own a `Connecting<I>` on the success path**, not a copy of
its parts: dropping a `Connecting` is ruling 50's cancellation, and a
`Service` future that is dropped must cancel exactly as `drop(connecting)`
does.

### 6.2 The stream-opener — `&Connection<S>`, not `Connection<S>`

```rust
#[cfg(feature = "tower")]
impl<'a, S: Handshake> tower_service::Service<()> for &'a Connection<S> {
    type Response = BiStream<S>;
    type Error    = ConnectionLost;
    type Future   = OpenBi<'a, S>;
}
```

**`PLAN.md` §3.4 writes `impl Service<()> for Connection`, and that cannot be
written.** `open_bi(&self)` borrows, so the returned future borrows; but
`Service::call(&mut self, …)` gives an anonymous lifetime that `type Future`
cannot name, and `Service` has no GAT. Implementing on `&'a Connection<S>`
names the lifetime in `Self` and the future can carry it. `&'a Connection<S>`
is `Copy`, so `call(&mut self)` moves it into `OpenBi<'a, S>` freely.

This is a **finding against `PLAN.md` §3.4**, not a choice: the sketched form
does not compile. It is flagged in PLAN-8 Open question 5. A consumer writes
`(&conn).oneshot(())` or `let mut svc = &conn;`.

### 6.3 `serve` — the accepting side

```rust
#[cfg(feature = "tower")]
pub async fn serve<H, Svc>(conn: &Connection<H>, svc: Svc) -> ConnectionLost
where
    H: Handshake,
    Svc: tower_service::Service<BiStream<H>> + Clone + 'static,
    Svc::Future: 'static;
```

- **Returns `ConnectionLost`, not `Result<(), ConnectionLost>`.** `PLAN.md`
  §3.4 writes the `Result`, but the loop has no success exit — it runs until
  `accept_bi()` fails, and the only way it can fail is the connection dying.
  A `Result` whose `Ok` is unreachable is a shape, not information. **Flagged
  as a deviation from `PLAN.md` §3.4; the maintainer may prefer the `Result`
  for symmetry, in which case it is `Err(lost)` always.**
- `accept_bi()` in a loop; per accepted stream, `svc.clone()` and
  `tokio::task::spawn_local(async move { let _ = svc.call(stream).await; })`.
- **`spawn_local`, never `tokio::spawn`** — S21, and the whole actor path.
  Hence `Svc: 'static` and `Svc::Future: 'static`, which the `BiStream<H>` (an
  owned handle) satisfies. **No `Send` bound.**
- **`Svc::Response` and `Svc::Error` are unconstrained and both are
  discarded.** `serve` does not trace the service's error: §18.2's trace
  targets are a **closed list of exactly five** and *"adding one is a protocol
  revision"*. Error handling is the service's, and the rustdoc says so.

### 6.4 `Rpc` — gated on `tower` **and** `codec`

```rust
#[cfg(all(feature = "tower", feature = "codec"))]
pub struct Rpc<'a, H: Handshake, C> { /* &'a Connection<H> + C */ }
```

`PLAN.md` §3.4's `Rpc<C: Codec>` is `Service<C::Item, Response = C::Item>`,
where the codec is `tokio_util::codec`'s — so it **cannot compile under
`tower` alone**, and `tower` does not imply `codec` in the manifest. The
`cfg(all(…))` gate is the recommendation in PLAN-8 Open question 4; the
alternative is `tower = ["codec", "dep:tower-service"]`.

`call` is *"open_bi, send, finish, read, EOF"* — one bi stream per call, the
stream being the correlation, because *"slither has no request/response
correlation on the wire."*

**`Rpc` is the designated cut** if the slice runs past its ≈1.5k-line budget;
S33's acceptance is satisfiable without it (PLAN-8 Open question 5).

### 6.5 The `!Send` caveat is rustdoc, not folklore (S33)

The module rustdoc states, from §16.11 and `PLAN.md` §3.4: `tower::Service`
itself, `tokio_util::codec`, `futures`' combinators and `tokio::io::copy` all
carry **no `Send` bound**, so this surface composes. What does **not** compose
is anything that spawns onto a work-stealing executor — `tower::buffer::Buffer`,
`spawn_ready`, `BoxService` (**use `UnsyncBoxService`**), hyper, and plain
`tokio::spawn` on any slither handle. A `Send` façade is **deliberately out of
v0.2 scope** and the handle shapes stay compatible with adding one later
without a breaking change.

## 7. `block_on` LocalSet helper

```rust
// src/compat/rt.rs — no feature gate. Re-exported as `slither::block_on`.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output;
```

- Builds a **current-thread** `tokio` runtime with `enable_all()`, creates a
  `tokio::task::LocalSet`, and runs `future` inside it. Every slither handle
  and driver is `!Send`; this is the one line that makes the crate usable
  without a paragraph of prose (`PLAN.md` §3.5).
- **No `Send` bound on `F` or `F::Output`.** That is the entire point.
- **Panics** if the runtime cannot be built, and **panics** if called from
  inside an existing tokio runtime. Both are documented on the function; the
  second is the mistake a consumer will actually make.
- The rustdoc carries the copy-pasteable example `PLAN.md` §3.5 asks for
  (*"Ship a helper and a copy-pasteable example rather than a paragraph of
  prose"*), showing the `Endpoint` built, a connection dialled and a
  `BiStream` written, all inside one `block_on`.

**`block_on` is not the test harness.** Slice 8's tests use the existing
paused-clock fixture (§10); `block_on` builds an unpaused runtime and must
never appear in a test that asserts a timer.

## 8. Error-mapping tables

**⚠ This section is the one part of the contract that is not fully derivable
from a ratified source.** §16.11 gives exactly two mappings for two enums
totalling nine variants:

```rust
impl From<ReadError>  for std::io::Error {}   // Reset → ConnectionReset
impl From<WriteError> for std::io::Error {}   // ConnectionLost → NotConnected / BrokenPipe
```

`Reset → ConnectionReset` is unambiguous. **`ConnectionLost → NotConnected /
BrokenPipe` is a slash between two kinds with no rule for choosing**, over a
`ConnectionLost` that has **seven** variants, and it says nothing at all about
`WriteError::Finished`. This is working rule 8's defect class — *a stated
construction with an unstated scope* — and it is raised as **PLAN-8 Open
question 8**. The tables below are the **recommendation**, written so the
blind agents have something exact to build and test against; they are marked
where they exceed the spec.

### 8.1 `From<ReadError> for io::Error`

| `ReadError` | `io::ErrorKind` | Source |
|---|---|---|
| `Reset(code)` | `ConnectionReset` | **§16.11, ratified** |
| `ConnectionLost(TimedOut)` | `TimedOut` | recommended |
| `ConnectionLost(NonceExhausted)` | `ConnectionAborted` | recommended |
| `ConnectionLost(LocallyClosed)` | `NotConnected` | recommended |
| `ConnectionLost(PeerClosed { .. })` | `ConnectionAborted` | recommended |
| `ConnectionLost(ProtocolViolation { .. })` | `ConnectionAborted` | recommended |
| `ConnectionLost(Replaced)` | `ConnectionAborted` | recommended |
| `ConnectionLost(EndpointDropped)` | `NotConnected` | recommended |

### 8.2 `From<WriteError> for io::Error`

| `WriteError` | `io::ErrorKind` | Source |
|---|---|---|
| `Reset(code)` | `ConnectionReset` | recommended, by symmetry with §16.11's read row |
| `Finished` | `BrokenPipe` | recommended — write-after-finish is the archetypal broken pipe |
| `ConnectionLost(TimedOut)` | `TimedOut` | recommended |
| `ConnectionLost(NonceExhausted)` | `BrokenPipe` | recommended |
| `ConnectionLost(LocallyClosed)` | `NotConnected` | **§16.11's `NotConnected` half** |
| `ConnectionLost(PeerClosed { .. })` | `BrokenPipe` | **§16.11's `BrokenPipe` half** |
| `ConnectionLost(ProtocolViolation { .. })` | `BrokenPipe` | recommended |
| `ConnectionLost(Replaced)` | `BrokenPipe` | recommended |
| `ConnectionLost(EndpointDropped)` | `NotConnected` | recommended |

**The rule behind the recommendation, stated so it can be judged rather than
memorised:** on the write side, a peer or transport that went away under a
writer is `BrokenPipe`; a connection *this side* never had or gave up is
`NotConnected`. That reading makes §16.11's slash a **variant** split rather
than a direction split, which is how it is written (the comment sits on the
`WriteError` line alone). `TimedOut` is lifted out of both because
`io::ErrorKind::TimedOut` exists and a 25 s `DEAD_TIMEOUT` is exactly what it
names — losing it would make every death look alike to a consumer whose only
view is `io::Error`.

**`WriteError` is `#[non_exhaustive]`** (ruling 61, reserving `Stopped`), so
the `match` needs a `_ =>` arm. It maps to `io::ErrorKind::Other`, and it must
**not** be `unreachable!()` — that is a panic on a variant a future minor
version adds.

### 8.3 The inner error is preserved

Every arm constructs `io::Error::new(kind, err)` so that
`e.into_inner().downcast::<ReadError>()` / `::<WriteError>()` recovers the
original, including a reset's `u64` code. Binding, and directly testable.

### 8.4 What is *not* converted

No `From<ConnectionLost>`, `From<MessageError>`, `From<DatagramError>`,
`From<ConnectError>`, `From<AcceptError>`, `From<AuthError>`,
`From<IntroError>` or `From<ConfigError>` for `io::Error`. §16.11 names two
conversions; working rule 8 reads that list as exhaustive. §18.1's taxonomy
gains nothing and loses nothing in this slice.

## 9. What the adapters must NOT do

A checklist, because each line is a way this slice has a known path to being
built wrong.

1. **No prefetch, no read-ahead task, no intermediate queue** (ruling 58,
   normative). No `VecDeque`, no `Vec<Item>`, no `spawn`-ing drainer, no
   "claim while we're here". The single-item `MessageSink` slot in §4.4 is the
   **only** buffer in `compat/`, and it exists because `Sink`'s own protocol
   requires it on the **send** side.
2. **No second implementation of a verb** (ruling 53). Every adapter calls an
   existing `pub(crate) poll_*`. If a needed `poll_*` does not exist, that is
   a finding to report — not a reason to write the logic twice.
3. **No boxed, stored in-flight future** on any data-path adapter. SPEC.md:5177
   names this as the cost ruling 53 exists to avoid, including that *"a stored
   future has already copied a buffer that the next `poll_write` may not pass
   again."* (`Incoming` is the single exception under discussion — §4.3.)
4. **`poll_flush` must not touch the core, the cell or the driver** (ruling
   56), and must not be described as delivery confirmation.
5. **`poll_shutdown` must not stop at `finish()`** (ruling 57). The weak
   reading makes S28's silent tail loss reachable through `AsyncWrite` by an
   application that never touches `close()`.
6. **No `Send` bound**, anywhere, on anything (S21). No `tokio::spawn`; only
   `spawn_local`.
7. **No new error type, and no new `From` conversion** beyond §16.11's two
   (§18.1 closed; ruling 204 item 2).
8. **No new trace target.** §18.2 is a closed list of five and adding one is a
   protocol revision.
9. **No change to any public handle signature** (ruling 204). Slice 8 is
   additive: new modules, new types, new inherent methods behind features,
   plus in-crate `pub(crate)` promotions.
10. **No `Drop` impl on anything wrapping a stream handle.** The halves' own
    `Drop`s are the protocol behaviour; a wrapper's `Drop` either duplicates
    or silently omits one of them.
11. **No shell-side scratch buffer on the read path** (§10.6).
12. **`Sink::poll_close` must not close the connection.**
13. **No `unsafe`** — the crate forbids it, `ReadBuf` has a safe path.
14. **No wire byte, no core type, no timer.** §16.11: *"Everything here is
    shell-layer."* If a change in `compat/` moves a golden-wire test, stop:
    that is a ruling, not an expectation to update.

---

## 10. Test fixture — the names both test authors build against

Taken from `src/testutil/mod.rs` at `5ed5bb4`, so neither blind author has to
guess. All are behind `feature = "test-util"`.

| Name | What it is |
|---|---|
| `slither::testutil::Pair` | the two-endpoint fixture; `pair.establish().await -> (TestConnection, TestConnection)` |
| `slither::testutil::local` | address helper used by the existing suites |
| `slither::testutil::settle` | `async fn settle()` — gives both drivers a turn **without advancing virtual time** |
| `slither::testutil::{Network, FlakyWire, FlakyPolicy}` | the in-memory fabric; `FlakyPolicy::{perfect, lossy, drop_first, drop_at, with_delay, with_duplication}` |
| `slither::testutil::TestIdentity` | `CountingIdentity<ReferenceSuite>` |
| `slither::testutil::TestEndpoint` | `Endpoint<TestIdentity>` |
| `slither::testutil::TestConnection` | `Connection<ReferenceSuite>` |
| `slither::testutil::TestSendStream` / `TestRecvStream` / `TestBiStream` | the three handles at `ReferenceSuite` |
| `slither::testutil::TestIntro` / `TestConnecting` / `TestPublicKey` | staged handle, dial future, key type |

**`Network`, `FlakyWire` and `FlakyPolicy` are attested surface under ruling
60** — renaming one is a protocol revision (see ruling 206 for what happened
when a field became a method without checking). Slice 8 renames nothing here.

**Paused clock, never a sleep** (§16.10). `tests/story_streams.rs` is the model
to copy: `tokio::time::timeout` is the observation instrument in both
directions — a `within` helper asserts *it resolved*, an `is_pending` helper
asserts *it had not resolved by then*, and `settle()` gives both drivers a
turn without advancing time. **There is no `sleep`.**

### Dev-dependencies the test authors may rely on

These are **integrator-added** (PLAN-8 §7) and are guaranteed present by the
time the suites are compiled:

- `tokio` with `io-util` — `tokio::io::copy`, `BufWriter`, `AsyncWriteExt`,
  `AsyncReadExt`.
- `futures-util` — `StreamExt::next`, `SinkExt::send`.
- `tower` with `features = ["util"]` — `UnsyncBoxService`, `ServiceExt::oneshot`.

**For the ruling-58 pin, use `std::task::Waker::noop()`** (stable within the
1.96 MSRV) and `Pin::new(&mut stream).poll_next(&mut cx)` directly, rather
than a combinator. It is exact about how many times the adapter is polled,
which is the whole assertion.

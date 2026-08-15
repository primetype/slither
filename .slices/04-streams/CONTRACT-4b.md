# CONTRACT-4b — the binding API contract for slice 4b

**Status: BINDING.** Both 4b agents compile against this and neither may
change it. If you believe something here is wrong, **say so in your
report and implement it as written anyway** (working rule 5 asks you to
flag, not to deviate silently).

Derived from `.slices/04-streams/PLAN-4b.md` §1, with every open question
resolved by **Round 20 (rulings 115–123)** in
`.spec-v2-clean-slate/rulings.md`. Where the plan says "OQ-n", this file
states the ruled answer.

---

## 0. What 4b builds

`SendStream` / `RecvStream` / `BiStream`, `Connection::{open_bi, open_uni,
accept_bi, accept_uni}`, §16.8's per-`StreamRef` waker maps, §16.2's drop
semantics for both half-handles, and the four story tests closing **S12
(loss-free), S13, S14, S17**.

**Not in 4b:** `AsyncRead`/`AsyncWrite` (slice 8, ruling 96);
`SendStream::acked()` (slice 5, ruling 122b — **absent, not stubbed**);
`send_message`/`recv_message` (slice 6); `notified()` (slice 7).

---

## 1. Types and the generic parameter

The shipped shell handle is **`Connection<S: Handshake>`**, not §16.2's
bare `Connection` (`src/shell/connection.rs:61`): §16.4's core is
parameterised by the **suite**, not the identity, and the shell erases
the identity through `Rc<dyn ShellLink>`. The three stream handles are
parameterised the same way and for the same reason — each holds an
`Rc<RefCell<ConnCell<S>>>`, and `ConnCell<S>` holds `core::Connection<S>`.

```rust
pub struct SendStream<S: Handshake> { /* private */ }
pub struct RecvStream<S: Handshake> { /* private */ }
pub struct BiStream<S: Handshake>  { /* private */ }
```

**None of the three is `Clone`.** Uniqueness is load-bearing: exactly one
`SendStream` and one `RecvStream` may exist per `StreamRef`, so a
handle's `Drop` is the sole owner of its waker-map entry and of its
half's retirement.

**Field shape** (normative in shape so both agents reason about the same
object; names may differ):

```rust
pub struct SendStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    key: u64,               // this handle's slot in ConnCell::blocked_writers[r]
    closed_locally: bool,   // set by finish() AND by reset(); Drop resets only when false
    id: Cell<Option<StreamId>>,   // cached; see §4
}

pub struct RecvStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    key: u64,               // slot in blocked_readers[r]
    ended: Option<Ended>,   // Ended::Eof | Ended::Reset(u64) — ruling 121's latch
    id: Cell<Option<StreamId>>,
}

pub struct BiStream<S: Handshake> { send: SendStream<S>, recv: RecvStream<S> }
```

**The waker key is a field, not a per-future `WakerSlot`.**
`Connection::closed()` mints its key inside the `async fn` and passes it
to `poll_closed`; that shape is unavailable here, because ruling 53
requires slice 8's `AsyncWrite` to be *the same function with its error
mapped*, and `AsyncWrite::poll_write(self: Pin<&mut Self>, cx, buf)` has
no argument to carry a key. `open_*`/`accept_*` **do** keep the
per-future `WakerSlot` shape: they have no `AsyncWrite` form, and several
`open_bi()` futures can coexist on one `&self` `Connection`.

**Test-facing aliases** — 4b-impl adds these beside `TestConnection`
(`src/testutil/mod.rs:947`):

```rust
pub type TestSendStream = crate::shell::SendStream<crate::packet::ReferenceSuite>;
pub type TestRecvStream = crate::shell::RecvStream<crate::packet::ReferenceSuite>;
pub type TestBiStream   = crate::shell::BiStream<crate::packet::ReferenceSuite>;
```

---

## 2. `Connection`'s four new verbs

```rust
impl<S: Handshake> Connection<S> {
    pub async fn open_bi(&self)    -> Result<BiStream<S>,   ConnectionLost>;
    pub async fn open_uni(&self)   -> Result<SendStream<S>, ConnectionLost>;
    pub async fn accept_bi(&self)  -> Result<BiStream<S>,   ConnectionLost>;
    pub async fn accept_uni(&self) -> Result<RecvStream<S>, ConnectionLost>;

    // Ruling 122a: pub(crate). Slice 8's compat/ is in-crate and reaches
    // these; promotion later is additive, publishing `key: u64` is not.
    pub(crate) fn poll_open_bi(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<BiStream<S>, ConnectionLost>>;
    pub(crate) fn poll_open_uni(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<SendStream<S>, ConnectionLost>>;
    pub(crate) fn poll_accept_bi(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<BiStream<S>, ConnectionLost>>;
    pub(crate) fn poll_accept_uni(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<RecvStream<S>, ConnectionLost>>;
}
```

| Situation | `open_bi` / `open_uni` | `accept_bi` / `accept_uni` |
|---|---|---|
| success | `Ok(handle)` | `Ok(handle)` |
| core says no | core returns `Err(StreamsExhausted)` → **park** in `stream_openers[dir]`, woken by `ConnEvent::StreamsAvailable { dir }` | core returns `None` → **park** in `stream_acceptors[dir]`, woken by `ConnEvent::StreamOpened { dir }` |
| connection already dead | `Err(ConnectionLost)` **immediately** (ruling 118) | `Err(ConnectionLost)` **immediately, nothing drained first** (ruling 118) |
| `cell.core == None` and `cell.closed == None` | `debug_assert!(false)`, then `Err(ConnectionLost::EndpointDropped)` | same |

**Ruling 118 in one line:** the drain-then-report rule is `notified()`'s
alone. §15.2 drops stream state at close, so there is nothing to drain;
draining would hand back a handle on which every `read` fails.

**`StreamsExhausted` never reaches the caller** — ruling 101 makes the
shell convert it into a park, which is why the type stays `pub(crate)`.

**FIFO** (ruling 112): `accept_*` yields peer-opened streams in open
order. An implicit open of six streams (ruling 99) emits six
`StreamOpened { dir }` events; the shell must **not** assume one event
equals one stream. On each wake it re-polls, and one `accept_*` future
claims exactly one stream.

**`open_bi` on an exhausted bidi space parks for ever in slice 4.** Bidi
MAX_STREAMS replenishment needs both halves freed, and the send half
frees only on acknowledgement, which slice 4 has no ACKs for
(`CONTRACT-4a.md` §5). **This must be in the rustdoc, and no test may
await it.** The `open_uni` equivalent *is* reachable — a peer-opened uni
stream read to EOF fully closes and grants MAX_STREAMS_UNI — and is worth
a test.

---

## 3. `SendStream`

```rust
impl<S: Handshake> SendStream<S> {
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError>;
    pub async fn finish(&mut self) -> Result<(), WriteError>;
    pub fn reset(&mut self, error_code: u64);
    pub fn id(&self) -> Option<StreamId>;

    pub(crate) fn poll_write(&mut self, cx: &mut Context<'_>, buf: &[u8])
        -> Poll<Result<usize, WriteError>>;
    pub(crate) fn poll_finish(&mut self, cx: &mut Context<'_>)
        -> Poll<Result<(), WriteError>>;
}
```

### `write` — every outcome

| Value | Means |
|---|---|
| `Ok(n)`, `n ≥ 1` | `n` bytes accepted into send state **and already sealed** (§16.7 + ruling 114: a mutating call flushes everything the ledger admits). `n ≤ buf.len()`; **a partial write is normal and the caller must loop.** `write` is not `write_all`. |
| `Ok(0)` | **Exactly one meaning: `buf` was empty.** Never "blocked", never EOF. The shell checks `buf.is_empty()` **before** calling the core (ruling 110). |
| `Poll::Pending` | `buf` was non-empty and the core returned `Ok(0)` — blocked by stream **or** connection credit (§10.1). Parked in `blocked_writers[r]`. |
| `Err(WriteError::Finished)` | `finish()` or `reset()` was called on this handle, **or** the core no longer holds this stream. |
| `Err(WriteError::ConnectionLost(l))` | the connection ended. |
| `Err(WriteError::Reset(_))` | **unreachable in slice 4.** `SendHalf::write` reports `Finished`, not `Reset`, for a locally reset half, and §9.9's STOP_SENDING is deferred so a peer cannot reset our send half. Do not test for it; do not fabricate a path to it. |

### `finish`

- `Ok(())` — the FIN is accepted into send state (§16.2). **It resolves
  on the first poll and never parks.** A test author may expect
  `finish()` to wait for the peer: it does not, and ruling 47's `acked()`
  — slice 5 — is what would.
- **Idempotent**: a second `finish()` is `Ok(())`.
- After `reset()` → `Err(Finished)`.
- Connection dead → `Err(ConnectionLost)`.
- After `finish()`, `write()` → `Err(Finished)`.

### `reset(error_code)`

- Synchronous, no return value, cannot fail.
- **Idempotent, first code wins.**
- Legal **after `finish()`**: the core does not check `fin`, so a
  reset-after-finish pins `final_size` = highest byte transmitted (ruling
  111) and discards the rest.
- On a dead connection: a silent no-op that still marks the cell dirty.
- After `reset()`, `write()` and `finish()` → `Err(Finished)`.
- Sets `closed_locally = true` — **as does `finish()`**. See §6.

---

## 4. `id()` — ruling 116

```rust
pub fn id(&self) -> Option<StreamId>;   // on all three handles
```

**The handle caches**: `None` until the core's first `Some`, then
`Some(id)` for ever, including after the stream closes and after the
connection dies. Caching is not an optimisation — the core's
`stream_id(r)` is **not monotone**, because `Streams::after_half_freed`
removes the entry, so an uncached `id()` answers `None` again once a
stream fully closes. That would contradict `remote_static()`'s
keeps-answering property.

**In wire v1 an application-held handle's `id()` is therefore always
`Some`.** Ruling 116 removed the documented `None` cause by ruling that
no pre-establishment `Connection` is published. The `Option` is retained
deliberately: removing it is a breaking change to save a `match`, and
re-adding it when a later line publishes a pre-establishment route would
be breaking again. **Do not write a test asserting `None` is reachable.**

---

## 5. `RecvStream`

```rust
impl<S: Handshake> RecvStream<S> {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
    pub fn id(&self) -> Option<StreamId>;

    pub(crate) fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<Result<Option<usize>, ReadError>>;
}
```

**This is the table slice 4a's test author guessed wrong. It is written
down rather than inferred.**

| Value | Means |
|---|---|
| `Ok(Some(n))`, `n ≥ 1` | `n` bytes drained from the contiguous prefix into `buf[..n]`. `n ≤ buf.len()`. A short read is normal. |
| `Ok(Some(0))` | **Exactly one meaning: `buf` was empty.** Ruling 119: the shell short-circuits an empty `buf` before touching the core, mirroring `write`. It never means "no data" at the handle surface — that is `Pending`. |
| `Poll::Pending` | `buf` was non-empty and the core returned `Ok(Some(0))` — no data available. Parked in `blocked_readers[r]`. |
| `Ok(None)` | **End of stream**: the FIN's final size was reached *and* every byte was delivered. **Sticky** — every later `read` is `Ok(None)`. |
| `Err(ReadError::Reset(code))` | the peer sent RESET_STREAM with `code` (§9.6). **Sticky, latched by the shell** (ruling 121) — the core is *not* sticky, and returns `Ok(None)` on the retry. |
| `Err(ReadError::ConnectionLost(l))` | the connection ended. |

`Ok(Some(0))` versus `Ok(None)` is the distinction that hangs a reader
for ever if inverted. At the handle surface the ambiguity is gone
entirely: `Ok(Some(0))` cannot mean "park", because the handle parks by
returning `Pending`.

**Ruling 121 is a correctness requirement, not a nicety.** Without the
latch, an application that logs the reset and retries reads a clean
end-of-stream, and §9.6's abandoned data is reported as a complete
transfer.

---

## 6. `BiStream` — ruling 120

```rust
impl<S: Handshake> BiStream<S> {
    pub fn split(self) -> (SendStream<S>, RecvStream<S>);
    /// Both halves must name the same `StreamRef` on the same connection.
    /// On mismatch the halves are handed back unchanged.
    pub fn join(send: SendStream<S>, recv: RecvStream<S>)
        -> Result<Self, (SendStream<S>, RecvStream<S>)>;
    pub fn id(&self) -> Option<StreamId>;
}
```

`join` validates same-`StreamRef`-same-`ConnectionId` and returns the
pair unchanged on mismatch. No new error type, so §18.1 stays closed
(ruling 61). The rejected alternative was a `debug_assert`, under which a
release build holds a `BiStream` whose halves are different streams and
whose `Drop` resets a stream the caller never named; ruling 44's
precedent governs — *rejection is a `Result`, never a panic*.

`BiStream` has **no `Drop` impl of its own**: its two fields' `Drop`s do
the work, in declaration order (`send`, then `recv`). It has no `write`
or `read` of its own either — §16.2's comment is `.split() → the pair`,
and ruling 96 puts `AsyncRead`/`AsyncWrite` in slice 8.

---

## 7. Handle lifetime — ruling 115

**All three stream handles are handles for §16.3's driver lifetime and
for ruling 88.** Each calls `shell.acquire()` and bumps `cell.handles` on
construction, and releases both on `Drop`, exactly as `Connection` does.

The authority is ruling 62's test: *a future or handle that changes
protocol state when dropped is a handle; one that does not, is not.*
Dropping a `SendStream` puts RESET_STREAM on the wire; dropping a
`RecvStream` retires the receive half, sets ruling 93's tombstone and
trues up the ledger. Both qualify.

The consequence that decides it: **a `Drop` that must emit a frame needs
a driver to emit it.** Under the alternative, dropping the last
`Connection` fires `close(NO_ERROR, "")` underneath a live `SendStream`
and that stream's RESET_STREAM is silently lost — in the *ordinary* shape
of a task that owns a stream and has let the connection handle go.

Ruling 88 still governs the genuinely-last drop in the process: nothing
is transmitted.

**Do not read §16.3:4409's four-item list as the set of driver-keeping
handles** — staged objects are in it and are excluded. See ruling 115.

---

## 8. After the connection dies — one rule, all verbs

`ConnCell::closed` is set once by `Driver::latch` and read for ever.
**Every 4b verb answers from it before anything else.**

| Verb | Answer once `closed == Some(l)` |
|---|---|
| `write`, `finish` | `Err(WriteError::ConnectionLost(l))` |
| `read` | `Err(ReadError::ConnectionLost(l))` |
| `reset` | silent no-op that still marks dirty |
| `id` | the cached value — **keeps answering**, like `remote_static()` |
| `open_*`, `accept_*` | `Err(l)` immediately, nothing drained (ruling 118) |
| `Drop` of either half | release the handle count; skip the core call if the core is gone |

`ConnectionLost` is `Clone` (ruling 46), so every waiter gets the value.

---

## 9. Drop semantics — and the single most likely 4b bug

### `Drop for SendStream`

**`SendHalf::reset` early-returns only when a reset already exists — a
set `fin` does NOT stop it.** So the obvious implementation, "reset
unconditionally on drop, the core is idempotent", **resets every finished
stream**, discards the unsent buffer, and turns the peer's clean EOF into
`ReadError::Reset(0)`.

The rule: **`Drop` calls `reset(0)` only when `closed_locally == false`**
— that is, only when neither `finish()` nor `reset()` has been called on
this handle. §16.2's phrase is "dropping a `SendStream` without
`finish()`", and ruling 122 confirms `reset()` sets the same flag (a
handle already reset must not reset again with a different code).

This defect passes any test that reads before the sender drops. **A test
that drops the `SendStream` after `finish()` and still expects the
reader's `Ok(None)` is the only thing that catches it, and 4b-test must
write one.**

### `Drop for RecvStream`

Calls `abandon_recv(now, r)` unless the connection is gone. Ruling 93
requires **both** tombstone mechanisms — the per-half tombstone *and* the
watermark — because either alone is wrong: the watermark alone
resurrects an abandoned bidi half, and the tombstone alone never returns
the peer's MAX_STREAMS credit. That is 4a's code; 4b's job is to call it
exactly once, at the moment of abandonment (§9.7).

### Both

- **`Drop` cannot await.** The frame is sealed synchronously inside the
  core call (§16.7, ruling 114); `Drop` then marks the cell dirty and
  wakes the driver, which performs the I/O. Nothing is awaited.
- **Remove this handle's waker-map entry**, and remove the per-`StreamRef`
  map entry itself when it becomes empty — the map is keyed by a value
  the application controls and must not grow without bound.
- **If the driver is already gone** (`cell.core == None`), skip the core
  call and release only. No panic, no debug-assert: this is reachable by
  ordinary teardown ordering.

---

## 10. The waker maps — ruling 117 and 122c

Added to `ConnCell<S>`:

```rust
blocked_writers:  BTreeMap<StreamRef, Wakers>,
blocked_readers:  BTreeMap<StreamRef, Wakers>,
stream_openers:   [Wakers; 2],   // indexed by Dir
stream_acceptors: [Wakers; 2],   // indexed by Dir
```

**Keyed by `StreamRef`, never `StreamId`** (ruling 117): a `StreamId` is
fixed by an opener parity that §6.7's tie-break can invert, so a map
keyed by it would need rekeying at every install.

**`Wakers`, not bare `Waker`** (ruling 122c): `Wakers::take_all` is
`#[must_use]`, which makes finding F10's borrow-then-wake ordering the
only spelling the type permits.

**Finding F10 is a hard requirement**: take the wakers out **inside** the
`RefCell` borrow, then drop the borrow, **then** wake. A `Waker` is
application-supplied, and an executor that polls inline re-enters the
cell. `Wakers::take_all`'s doc comment records why this was closed by
construction rather than by argument.

---

## 11. What is NOT settled by this contract

Report these; do not resolve them.

- **Ruling 113's `on_ack_range` signature is known-wrong** (the FIN flag
  is missing). Slice 5 fixes it. It is not 4b's.
- The two frame-level assertions `PLAN.md` §11.2/§11.4 assign to 4b are
  unreachable from `tests/` — `Tap` yields sealed datagrams. **Ruling 123
  moves them to 4a's in-crate file and the maintainer adds them at
  integration.** 4b-test uses the behavioural substitutes in `PLAN-4b.md`
  §6.

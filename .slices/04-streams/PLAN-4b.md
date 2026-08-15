# PLAN-4b — slice 4b: the stream shell

**Status:** DRAFT (planner in flight)
**Branch:** `main` · **HEAD at planning start:** `b67f871`
**Scope:** ruling 107's 4b — `SendStream` / `RecvStream` / `BiStream`,
`Connection::{open_bi, open_uni, accept_bi, accept_uni}`, ruling 53's
`poll_*`-once data path, §16.8 waker maps, §16.2 drop semantics, four
story tests (S12 loss-free half, S13, S14, S17).

---

## §0. Preconditions, checked

`PLAN.md` §6.3 names one: *"4b must not start until ruling 90's split has
landed and been committed."* **Satisfied at `b67f871`** —
`src/core/endpoint/mod.rs:405` is `mint_pending`, `:472` is
`start_attempt`, and `ShellState::drain_endpoint`
(`src/shell/shared.rs:361`) is the handle-side drain those two require.
`src/shell/shared.rs` and `src/shell/driver.rs` are therefore free for
4b-impl to own outright.

---

## §1. The binding API contract for 4b

**This section becomes `CONTRACT-4b.md`. Both 4b agents compile against
it and neither may change it.** If you believe something here is wrong,
say so in your report and implement it as written anyway.

### 1.1 The generic parameter

The shipped shell handle is **`Connection<S: Handshake>`**, not §16.2's
bare `Connection` (`src/shell/connection.rs:61`). Its own doc explains
why (`src/shell/shared.rs:192–201`): §16.4's core is parameterised by the
**suite**, not by the identity — the identity is a statement about where
*our own* private key lives and has nothing to do with the session — so
the shell erases the identity through `Rc<dyn ShellLink>` and keeps only
the suite.

The three stream handles are parameterised the **same way and for the
same reason**: each holds an `Rc<RefCell<ConnCell<S>>>`, and `ConnCell<S>`
holds `core::Connection<S>`.

```rust
pub struct SendStream<S: Handshake> { /* private */ }
pub struct RecvStream<S: Handshake> { /* private */ }
pub struct BiStream<S: Handshake>  { /* private */ }
```

**None of the three is `Clone`.** `Connection<S>` is not `Clone` today
either. Uniqueness is load-bearing for §3's waker maps and for §5's drop
semantics: exactly one `SendStream` and one `RecvStream` may exist per
`StreamRef`, so the handle's `Drop` is the sole owner of its map entry.

**Type aliases for the test author** — 4b-impl adds these to
`src/testutil/mod.rs` beside the existing `TestConnection`
(`src/testutil/mod.rs:947`), and 4b-test names them rather than spelling
the suite:

```rust
pub type TestSendStream = crate::shell::SendStream<crate::packet::ReferenceSuite>;
pub type TestRecvStream = crate::shell::RecvStream<crate::packet::ReferenceSuite>;
pub type TestBiStream   = crate::shell::BiStream<crate::packet::ReferenceSuite>;
```

### 1.2 Private fields (normative in shape, so both agents reason about
the same object)

```rust
pub struct SendStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    /// This handle's slot in `ConnCell::blocked_writers[r]`. Minted once,
    /// released in `Drop`.
    key: u64,
    /// Set by `finish()` and by `reset()`. **`Drop` resets only when this
    /// is `false`** — see §5.1, where getting it wrong destroys the FIN.
    closed_locally: bool,
    /// §16.9's id, cached the first time the core answers `Some`.
    /// See §7 U3: the core's answer is not monotone.
    id: Cell<Option<StreamId>>,
}

pub struct RecvStream<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    conn: ConnectionId,
    r: StreamRef,
    key: u64,                       // slot in `blocked_readers[r]`
    /// Latched terminal outcome, so `read` is idempotent after it ends.
    /// See §7 U-C-D — the core is NOT sticky on `Reset`.
    ended: Option<Ended>,           // Ended::Eof | Ended::Reset(u64)
    id: Cell<Option<StreamId>>,
}

pub struct BiStream<S: Handshake> { send: SendStream<S>, recv: RecvStream<S> }
```

**Why the key lives in the handle and not in a per-future `WakerSlot`.**
`Connection::closed()` (`src/shell/connection.rs:179–186`) mints its key
in the `async fn` and passes it into `poll_closed`. That shape is
unavailable here: ruling 53 requires §16.11's `AsyncWrite` to be *the
same function with its error mapped*, and `AsyncWrite::poll_write(self:
Pin<&mut Self>, cx, buf)` has **no argument to carry a key**. So the key
must be a field. `open_*`/`accept_*` keep `closed()`'s per-future
`WakerSlot` shape, because they have no `AsyncRead`/`AsyncWrite` form and
because several `open_bi()` futures can coexist on one `&self`
`Connection`.

### 1.3 `Connection`'s four new verbs

```rust
impl<S: Handshake> Connection<S> {
    pub async fn open_bi(&self)    -> Result<BiStream<S>,   ConnectionLost>;
    pub async fn open_uni(&self)   -> Result<SendStream<S>, ConnectionLost>;
    pub async fn accept_bi(&self)  -> Result<BiStream<S>,   ConnectionLost>;
    pub async fn accept_uni(&self) -> Result<RecvStream<S>, ConnectionLost>;

    // The single implementations (ruling 53). `pub(crate)`, so slice 8's
    // `compat/` can reach them; not `pub`, because promotion is additive
    // and demotion is breaking.
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

Semantics, exhaustively:

| Situation | `open_bi` / `open_uni` | `accept_bi` / `accept_uni` |
|---|---|---|
| success | `Ok(handle)` | `Ok(handle)` |
| core says no | `Err(StreamsExhausted)` → **park** in `stream_openers[dir]`, woken by `ConnEvent::StreamsAvailable { dir }` (§10.4, §16.2:4168) | `None` → **park** in `stream_acceptors[dir]`, woken by `ConnEvent::StreamOpened { dir }` |
| connection already dead | `Err(ConnectionLost)` | `Err(ConnectionLost)` — **but see OQ-3**, the pull-model drain question |
| `cell.core == None` and `cell.closed == None` | `debug_assert!(false)`, then `Err(ConnectionLost::EndpointDropped)` | same |

`StreamsExhausted` never reaches the caller — ruling 101 is explicit that
the shell converts it into a park, which is why the type stays
`pub(crate)`.

**FIFO** (ruling 112): `accept_*` yields peer-opened streams in open
order. An implicit open of six streams (ruling 99) emits six
`StreamOpened { dir }` events and six `accept()` calls drain them; the
shell must **not** assume one event equals one stream — on each wake it
re-polls, and a single `accept_*` future claims exactly one.

**`open_bi` on an exhausted bidi space parks for ever in slice 4.** See
§7 U8 — bidi MAX_STREAMS replenishment is unreachable without ACKs
(`CONTRACT-4a.md` §5). This is a slice boundary, must be in the rustdoc,
and **no test may await it**.

### 1.4 `SendStream`

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

**`acked()` is NOT built in 4b.** See §7 U1.

**`write` — every outcome, stated:**

| Value | Means |
|---|---|
| `Ok(n)`, `n ≥ 1` | `n` bytes **accepted into send state** and already sealed (§16.7 + ruling 114: a mutating call flushes everything the ledger admits). `n ≤ buf.len()`; **a partial write is normal and the caller must loop.** `write` is not `write_all`. |
| `Ok(0)` | **Exactly one meaning: `buf` was empty.** Never "blocked", never EOF. The shell checks `buf.is_empty()` **before** calling the core and returns `Ok(0)` at once (ruling 110 — parking on an empty write waits for credit that would not help). |
| `Poll::Pending` | `buf` was non-empty and the core returned `Ok(0)` — blocked by stream **or** connection credit (§10.1). Parked in `blocked_writers[r]`. |
| `Err(WriteError::Finished)` | `finish()` or `reset()` was called on this handle, **or** the core no longer holds this stream. |
| `Err(WriteError::ConnectionLost(l))` | the connection ended. |
| `Err(WriteError::Reset(_))` | **unreachable in slice 4.** `SendHalf::write` (`src/core/connection/send.rs:216–219`) reports `Finished`, not `Reset`, for a locally reset half, and §9.9's STOP_SENDING is deferred so a peer cannot reset our send half. Do not write a test that expects it; do not add a code path that fabricates it. |

**`finish`:**
- `Ok(())` — the FIN is accepted into send state (§16.2:4174). **It
  resolves on the first poll and never parks.** Say this out loud: a test
  author may expect `finish()` to wait for the peer; it does not, and
  ruling 47's `acked()` is what would.
- **Idempotent** — a second `finish()` is `Ok(())`
  (`send.rs:245–251`).
- After `reset()` → `Err(Finished)`.
- Connection dead → `Err(ConnectionLost)`.
- After `finish()`, `write()` → `Err(Finished)`.

**`reset(error_code)`:**
- Synchronous, no return, cannot fail.
- **Idempotent, first code wins** (`send.rs:266–269`).
- Legal **after `finish()`**: `SendHalf::reset` does not check `fin`, so
  a reset-after-finish pins `final_size = highest byte transmitted`
  (ruling 111) and discards the rest.
- On a dead connection: the core has **no** lost check
  (`src/core/connection/mod.rs:443`), so the call mutates and pumps and
  emits nothing. Harmless; the shell still marks dirty.
- After `reset()`, `write()` and `finish()` → `Err(Finished)`.

**`id()`** — see §7 U3. **The shell caches**: `None` until the first
core answer is `Some`, and `Some(id)` for ever after. The core's
`stream_id(r)` is *not* monotone — `Streams::after_half_freed`
(`src/core/connection/streams.rs:919`) does `self.entries.remove(&r)`, so
a fully closed stream answers `None` again.

### 1.5 `RecvStream`

```rust
impl<S: Handshake> RecvStream<S> {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
    pub fn id(&self) -> Option<StreamId>;

    pub(crate) fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8])
        -> Poll<Result<Option<usize>, ReadError>>;
}
```

**`read` — every outcome, stated. This is the table slice 4a's test
author guessed wrong; it is written down rather than inferred.**

| Value | Means |
|---|---|
| `Ok(Some(n))`, `n ≥ 1` | `n` bytes drained from the contiguous prefix into `buf[..n]`. `n ≤ buf.len()`. A short read is normal. |
| `Ok(Some(0))` | **Exactly one meaning: `buf` was empty.** The shell short-circuits an empty `buf` before touching the core, mirroring `write`. It never means "no data" at the handle surface — that is `Pending`. (**OQ-4**: this mirror of ruling 110 has not been ruled for `read`.) |
| `Poll::Pending` | `buf` was non-empty and the core returned `Ok(Some(0))` — no data available. Parked in `blocked_readers[r]`. |
| `Ok(None)` | **End of stream**: the FIN's final size was reached *and* every byte was delivered. **Sticky** — every later `read` is `Ok(None)`. |
| `Err(ReadError::Reset(code))` | the peer sent RESET_STREAM with `code` (§9.6). **Latched by the shell** so it is sticky too — see §7 C-D; the core is not. |
| `Err(ReadError::ConnectionLost(l))` | the connection ended. |

`Ok(Some(0))` versus `Ok(None)` is the distinction that hangs a reader
for ever if inverted. At the **handle** surface the ambiguity is removed
entirely: `Ok(Some(0))` cannot mean "park", because the handle parks by
returning `Pending`.

**`id()`** — same caching rule as `SendStream`.

### 1.6 `BiStream`

```rust
impl<S: Handshake> BiStream<S> {
    pub fn split(self) -> (SendStream<S>, RecvStream<S>);
    /// The two halves must name the same stream on the same connection.
    /// On mismatch the halves are handed back unchanged.
    pub fn join(send: SendStream<S>, recv: RecvStream<S>)
        -> Result<Self, (SendStream<S>, RecvStream<S>)>;
    pub fn id(&self) -> Option<StreamId>;
}
```

`BiStream` has **no `Drop` impl of its own** — its two fields' `Drop`s
do the work, in declaration order (`send` then `recv`). It has no `write`
or `read` of its own either: §16.2's comment is `.split() → the pair`,
and ruling 96 puts the `AsyncRead`/`AsyncWrite` impls in slice 8.

`join`'s fallible signature is a **recommendation, not spec** — §16.2
never mentions `join`; ruling 96 introduces the name and says nothing
about its inputs. See §7 U6 and **OQ-5**. No new error type is minted, so
§18.1 stays closed (ruling 61).

### 1.7 After the connection dies — one rule, all verbs

`ConnCell::closed` (`src/shell/shared.rs:164`) is set once by
`Driver::latch` and read for ever. **Every 4b verb answers from it before
anything else**, except where OQ-3 rules otherwise:

| Verb | Answer once `closed == Some(l)` |
|---|---|
| `write`, `finish` | `Err(WriteError::ConnectionLost(l))` |
| `read` | `Err(ReadError::ConnectionLost(l))` |
| `reset` | silent no-op that still marks dirty |
| `id` | the cached value — **keeps answering**, like `remote_static()` |
| `open_*`, `accept_*` | `Err(l)` (OQ-3) |
| `Drop` of either half | see §5.4 |

`ConnectionLost` is `Clone` (ruling 46), so every waiter gets the value.

---

## §2. Module map and file ownership

**Working rule 6 is absolute: no two concurrent agents may name the same
path.** `PLAN.md` §6.2's first cut is below with **four corrections**,
each verified against the tree at `b67f871`.

### 2.1 The partition

| Path | New? | Owner | Contents |
|---|---|---|---|
| `src/shell/stream.rs` | new | **4b-impl** | `SendStream`, `RecvStream`, `BiStream`; the `poll_*` forms; `id()`; both `Drop` impls |
| `src/shell/connection.rs` | edit | **4b-impl** | the four verbs as `poll_fn` over `poll_*` |
| `src/shell/shared.rs` | edit | **4b-impl** | the four waker maps in `ConnCell`; the `Wakers` doc correction (C-A) |
| `src/shell/driver.rs` | edit | **4b-impl** | routing the six `ConnEvent`s (`publish`, `driver.rs:493–515`); extending `latch` to sweep the new maps |
| `src/shell/mod.rs` | edit | **4b-impl** | `mod stream;`, the three `pub use`s, and the module-doc line that currently says the stream verbs are absent (`mod.rs:40–43`) |
| `src/lib.rs` | edit | **4b-impl** | add the three types to the `pub use shell::{…}` line (`lib.rs:139`) |
| **`src/testutil/mod.rs`** | edit | **4b-impl** | **[correction 1]** the three type aliases of §1.1 |
| **`Cargo.toml`** | edit | **4b-impl** | **[correction 2]** two `[[test]]` stanzas with `required-features = ["test-util"]` |
| `tests/story_streams.rs` | new | **4b-test** | S12, S13, S14, S17 |
| `tests/spec_streams.rs` | new | **4b-test** | the §16.2 obligations reachable from the public API |

**Correction 1 — `src/testutil/mod.rs` is missing from `PLAN.md` §6.2.**
`testutil` already carries `TestConnection`
(`src/testutil/mod.rs:947`), and without the three stream aliases the
blind test author must spell
`slither::SendStream<slither::packet::ReferenceSuite>` — a guess at two
paths. It is an **impl-owned** file; the test author must not touch it.

**Correction 2 — `Cargo.toml` is missing.** `testutil` is behind
`#[cfg(any(test, feature = "test-util"))]` (`src/lib.rs:129`), and every
existing integration target is declared with
`required-features = ["test-util"]` so that a feature-less `cargo test`
skips it rather than compiling to "running 0 tests". Two new targets need
two new stanzas. Impl-owned, or the integrator's.

**Correction 3 — `src/shell/mod.rs` is not only a module file.** It is
33 KB and most of it is an **inline** `#[cfg(test)] mod tests { … }`
(`src/shell/mod.rs:57` onward) holding slice 3b's seam smoke tests. It is
listed as 4b-impl's, and that is right — but it means **4b-impl also
owns the shell's internal tests** and 4b-test must never open it. State
that in both briefs.

**Correction 4 — `src/error.rs` really is on nobody's path**, and
`PLAN.md` is right. Confirmed: `WriteError` and `ReadError`
(`src/error.rs:239–260`) already carry every variant 4b needs, and
`WriteError::Reset` is a variant 4b must leave unreachable rather than
find a use for.

### 2.2 What the integration tests can actually reach

`tests/*.rs` see only slither's public API. Verified at `b67f871`:

**Reachable today:** `Endpoint`, `EndpointBuilder`, `Connecting`,
`Connection`, `Intro`/`Claimed`/`Proven`, `ConnectionLost`, `ReadError`,
`WriteError`, `ConfigError`, `StreamId`, `Dir`, `ConnectionId`,
`Timestamp`, `SessionId`, and the whole of `testutil` — `Network`,
`FlakyWire`, `FlakyPolicy` (`perfect`, `drop_first`, `drop_at`, `lossy`,
`with_delay`, `with_duplication`, `fail_sends`, `failing_sends_until`),
`Tap`, `TestIdentity`, `TestEndpoint`, `TestConnection`.

**4b must add to `pub use`:** `SendStream`, `RecvStream`, `BiStream` (and
the three testutil aliases).

**NOT reachable, and three planned assertions depend on it — flag:**

1. **`StreamRef` is `pub(crate)`** (`src/core/mod.rs:59`) and stays that
   way. No integration test may name it. Every story assertion must be
   phrased in `StreamId`s, byte counts, or handle behaviour.
2. **`Tap` yields sealed datagrams.** `Tap::datagrams()`
   (`src/testutil/mod.rs:136`) returns `(from, to, Vec<u8>)` of the
   *wire* datagram, which after establishment is AEAD ciphertext. **No
   integration test can count frames.** This kills two assertions
   `PLAN.md` §11 asks 4b to make — see §6.5 and risk R6.
3. **`Connection` has no `role()` accessor** and §16.2 does not give it
   one. S13's parity claim must be phrased through
   `StreamId::initiated_by_connection_initiator()`
   (`src/core/connection/stream_id.rs:77`, public) rather than by asking
   the connection what it is.

---

## §3. The waker-map design

### 3.1 What is added to `ConnCell<S>`

```rust
pub(crate) struct ConnCell<S: Handshake> {
    // … existing seven fields …

    /// §16.8's blocked-readers map. One entry per **live `RecvStream`
    /// that has parked at least once**; created lazily, removed by that
    /// handle's `Drop`.
    pub(crate) blocked_readers: BTreeMap<StreamRef, Wakers>,
    /// §16.8's blocked-writers map, on the same terms.
    pub(crate) blocked_writers: BTreeMap<StreamRef, Wakers>,
    /// `open_bi`/`open_uni` futures parked on §10.4's cumulative limit.
    /// Indexed by `Dir::slot()`, **not** a map: `Dir` has exactly two
    /// values, so the array is bounded by construction.
    pub(crate) stream_openers: [Wakers; 2],
    /// `accept_bi`/`accept_uni` futures parked on an empty unclaimed
    /// queue. Same shape, same reason.
    pub(crate) stream_acceptors: [Wakers; 2],
}
```

**Keyed by `StreamRef`, not `StreamId`** — ruling 95's amendment. §16.8's
own sentence says `StreamId` and is stale; so does `Wakers`' doc comment
at `src/shell/shared.rs:48–51`. Both are conflict C-A and both need
correcting in 4b. A `StreamId`-keyed map cannot be built at all:
`stream_id()` is `None` before establishment and the four stream
`ConnEvent`s carry `StreamRef`.

**Why `Wakers` and not a bare `Option<Waker>`.** At most one future can
park per handle per direction — the handles are not `Clone` and the verbs
take `&mut self` — so a single waker would suffice, and a
`BTreeMap<StreamRef, Waker>` would be simpler. It is nonetheless the
wrong choice: `Wakers::take_all` is
`#[must_use = "the wakers must be woken after the cell borrow ends
(§16.8)"]` (`src/shell/shared.rs:111`), which makes F10's borrow-then-wake
ordering **the only spelling the type permits**. With a bare `Waker` an
implementer can write
`cell.borrow_mut().blocked_readers.remove(&r).map(Waker::wake)` and
reintroduce F10 in the slice whose brief names F10. Uniformity with the
reviewed type is worth more than the saved allocation.

### 3.2 Who inserts

| Map | Inserted by | Key |
|---|---|---|
| `blocked_readers[r]` | `RecvStream::poll_read`, when the core returned `Ok(Some(0))` on a non-empty buf | the handle's own `self.key` |
| `blocked_writers[r]` | `SendStream::poll_write`, when the core returned `Ok(0)` on a non-empty buf | the handle's own `self.key` |
| `stream_openers[dir]` | `Connection::poll_open_*`, on `Err(StreamsExhausted)` | a per-future `WakerSlot` key |
| `stream_acceptors[dir]` | `Connection::poll_accept_*`, on `None` | a per-future `WakerSlot` key |

**The ordering constraint nobody has written down.** `ConnEvent::
StreamWritable { r }` fires only for a send half whose `blocked` flag is
set, and `blocked` is set **only inside `SendHalf::write` when
`room == 0`** (`src/core/connection/send.rs:228–231`;
`on_max_stream_data` at `:378–388` and `unblock` at `:391–395` both
return `false` when it was not set). Therefore:

> **`poll_write` must never park without having just called
> `core.write()` and received `Ok(0)`.** A shell that reads a credit
> accessor, decides it is blocked, and parks without calling the core
> will never be woken — the core does not know a writer is waiting.

The same shape does **not** apply to `poll_read`: `StreamReadable` fires
from `recv.is_readable()` on frame arrival
(`src/core/connection/streams.rs:492–494`) regardless of whether anyone
asked. But parking without a preceding `read` still risks a lost wakeup
if the data arrived before the park, so **park only after a core call
that said "nothing"**, in both directions. That is also what makes the
verbs cancel-safe (§4).

### 3.3 Who wakes, and where the F10 boundary is

`Driver::publish` (`src/shell/driver.rs:493–515`). **It already runs with
no cell borrow held** — `serve_connection` (`:456–464`) scopes
`cell.borrow_mut()` inside the `let output = { … }` block and matches on
the result outside it. That is the boundary, and it exists; 4b does not
have to create it, only avoid destroying it.

The shape every arm takes:

```rust
ConnEvent::StreamReadable { r } => {
    let woken = {
        let mut b = cell.borrow_mut();
        b.blocked_readers.get_mut(&r).map(Wakers::take_all).unwrap_or_default()
    };                                   // ← borrow ends here
    for w in woken { w.wake(); }         // ← consumer code runs here
}
```

Full routing table:

| `ConnEvent` | Wakes |
|---|---|
| `StreamReadable { r }` | `blocked_readers[r]` |
| `StreamWritable { r }` | `blocked_writers[r]` |
| `StreamReset { r, .. }` | `blocked_readers[r]` — so `poll_read` re-polls and surfaces `Err(Reset)`. **Not** the writer: in slice 4 a peer cannot reset our send half (§9.9 deferred). |
| `StreamOpened { dir }` | `stream_acceptors[dir]` — **all of them**, because ruling 99 emits one event per stream and a single event may not correspond to a single waiting future |
| `StreamsAvailable { dir }` | `stream_openers[dir]` |
| `StreamFinished { r }` | nothing in 4b (`acked()` is not built; the event never fires in slice 4) |
| `Closed(lost)` → `latch` | **all four maps**, plus `closed_wakers` |

**`Driver::latch` (`driver.rs:992–1004`) must be extended.** It is the
only thing that wakes cell-parked waiters on connection death *and* on
driver death, because `Driver::stop` (`:1047–1071`) calls it and
`Drop for Driver` (`:1100–1104`) calls `stop` on an unwind. Today it
takes `closed_wakers.take_all()` only. If 4b does not add the four new
maps to it, **a writer parked on flow-control credit hangs for ever when
the driver panics** — F1's exact shape, in the slice that was cut out
because F1's shape lives here.

### 3.4 Who removes — and what bounds a map the application controls

Three independent bounds, and the argument needs all three:

1. **Per-waker.** `Wakers::unpark(key)` on the handle's key, called from
   `Drop` (for `SendStream`/`RecvStream`) or from `WakerSlot::drop` (for
   the `open_*`/`accept_*` futures). Idempotent
   (`src/shell/shared.rs:85–87`). This is what stops a cancelled future
   leaving a waker behind — FIXES-3b finding 3 was `Driver::waiting`
   growing 132.3 bytes per cancelled `accept()` for the endpoint's life.
2. **Per-entry.** `SendStream::drop` does
   `cell.blocked_writers.remove(&self.r)`; `RecvStream::drop` does
   `cell.blocked_readers.remove(&self.r)`. **This is the bound that
   matters**, and it is structural rather than disciplinary: because
   neither handle is `Clone`, there is exactly one owner of each entry
   and its lifetime is exactly that handle's lifetime. A map keyed by a
   value the application controls is bounded by *the number of live
   stream handles*, which is bounded by §10.4's cumulative limit — 32
   bidi and 128 uni today — **not** by the number of streams the
   connection has ever opened.
3. **Per-wake.** `take_all` clears the inner `Wakers`
   (`shared.rs:112–113`), so a woken-and-never-re-parked stream leaves an
   empty `Wakers` behind rather than a stale one. 4b may additionally
   remove an entry that is empty after `take_all`; it is not required,
   because (2) already bounds it.

*What state does this argument assume (working rule 12)?* It assumes the
handles are not `Clone` and that `BiStream::split` produces one
`SendStream` and one `RecvStream`, never two of either. Both are §1's
choices, and if either is relaxed later, bound (2) fails and the maps
need reference counting. Say so in the rustdoc.

**The test that pins it** (working rule 9): open and drop N stream
handles in a loop and assert the two maps are **empty** afterwards — not
"bounded". A degenerate build that never inserts also passes "bounded";
it does not pass a test that first parks a reader, asserts the map is
non-empty, then drops and asserts empty. Assert from the side that
separates them. This needs a `pub(crate)` accessor on `ConnCell`, so it
is a **`src/shell/mod.rs` inline test, owned by 4b-impl**, not an
integration test.

---

## §4. Cancel-safety, verb by verb

§16.2's standard, set for `notified()`: *"It is cancel-safe: a dropped
future has claimed nothing."* Every 4b verb is held to it.

### 4.1 `SendStream::write` — cancel-safe

`poll_write` is synchronous: it takes the borrow, calls `core.write`,
releases the borrow, and returns. There is no yield point inside it, so a
future can only be dropped **between** polls — i.e. while `Pending`,
which is reached only after `core.write` returned `Ok(0)` and buffered
**nothing**. Mutations at that point: `SendHalf::blocked = true`
(`send.rs:229`). That is not observable to the application and is cleared
by the next credit arrival either way. The waker is removed by `Drop`, or
overwritten by the next `poll_write` on the same key.

**Where it would fail, and does not:** if `poll_write` stored a partially
consumed buffer across polls — ruling 53's named hazard, *"a stored
future has already copied a buffer that the next `poll_write` may not
pass again"*. It does not store; that is why the verb is written as a
`poll_*` in the first place.

**The residual sharp edge, which is not a cancel-safety failure but reads
like one:** `write` returning `Ok(n)` with `n < buf.len()` has already
committed `n` bytes. A caller that treats a short return as a failure and
retries the whole buffer duplicates data. That is `io::Write` semantics
and is correct, but it must be in the rustdoc.

### 4.2 `RecvStream::read` — cancel-safe

Same argument. `Pending` is reached only after the core returned
`Ok(Some(0))`, which consumes nothing: `RecvHalf::read` reports
`consumed == 0` and `Streams::read` (`streams.rs:321`) skips the credit
advance entirely. A dropped `read` future has taken no byte and advanced
no credit.

**Where it would fail:** if `poll_read` drained into a shell-side scratch
buffer and copied out on the next poll. It must not — §10.6 and ruling
56 forbid a shell buffer, and a dropped future would strand the drained
bytes where no later `read` could reach them. **Read straight into the
caller's `buf`.**

### 4.3 `SendStream::finish` — trivially cancel-safe, and worth saying why

`poll_finish` is `Ready` on the first poll, so there is no cancellation
window at all. It is written in the poll form only because ruling 53
requires every data-path verb to have one and because slice 8's
`AsyncWrite::poll_shutdown` is this function error-mapped — exactly the
reasoning already recorded for `poll_close`
(`src/shell/connection.rs:119–131`).

A `finish()` future that is **created and never awaited** does nothing:
the FIN is not pinned, and dropping the handle then resets it. `Future`
is `#[must_use]`, so this is a warning, not a silent bug.

### 4.4 `open_bi` / `open_uni` — cancel-safe **only if built correctly**

This is the one with a real failure mode. `core.open(dir)`
(`streams.rs:211–223`) **increments `ever_opened` and consumes an index
against §10.4's cumulative limit**. If the future is dropped after
`open()` succeeded but before the handle is constructed, the stream is
leaked: an index spent for ever, a send half live for ever, and its
contribution pinned in the connection ledger.

That state is **unreachable if and only if the core call and the handle
construction happen in the same synchronous `poll_*` body with no `?`,
no early return, and no fallible step between them.** Make it a stated
rule in the contract, not an emergent property:

> **Never call `core.open()` or `core.accept()` without constructing the
> handle in the same expression.**

A dropped future that parked on `StreamsExhausted` has claimed nothing.
A future that returned `Ready(Ok(handle))` has delivered it. Both ends
are safe; only the middle is not, and the middle can be made
unrepresentable.

### 4.5 `accept_bi` / `accept_uni` — same, and strictly worse

`core.accept(dir)` **pops** from `unclaimed`
(`streams.rs:239–241`). A pop that is not followed by a handle is not
merely a leak — the stream is unclaimable for ever, while the peer's
bytes keep arriving and keep charging the receive ledger. The same
one-expression rule closes it.

There is a second, subtler failure: `poll_accept_*` that pops and *then*
notices `cell.closed.is_some()` and returns `Err(ConnectionLost)` orphans
the popped stream. **Check the latch before popping** — which is also
what OQ-3 is about.

### 4.6 `reset()` — not async, but note it anyway

`reset` is `&mut self` and synchronous, so there is no cancellation
question. It is listed here because a reader scanning for "which verbs
can I cancel" must find the answer for all five, not four.

---

## §5. Drop semantics

### 5.1 `Drop for SendStream` — and the single most likely 4b bug

§16.2:4345: *"Dropping a `SendStream` without `finish()` resets it with
error code 0."*

```rust
impl<S: Handshake> Drop for SendStream<S> {
    fn drop(&mut self) {
        let mutated = {
            let mut cell = self.cell.borrow_mut();
            cell.blocked_writers.remove(&self.r);          // §3.4 bound (2)
            match (self.closed_locally, cell.core.as_mut()) {
                (false, Some(core)) => { core.reset(now(), self.r, 0);
                                         cell.dirty = true; true }
                _ => false,
            }
        };                                                  // ← borrow ends
        if mutated { self.shell.mark_dirty(self.conn); }
    }
}
```

**The bug to name explicitly in both briefs.** It is tempting to drop the
`closed_locally` flag and reset unconditionally, reasoning that
`SendHalf::reset` is idempotent. **It is not idempotent with respect to
`finish()`**: `send.rs:266–269` early-returns only when
`self.reset.is_some()`, and `fin` alone does not stop it. So an
unconditional reset-on-drop **resets every stream the application
finished**, pinning `final_size` at the highest byte transmitted, wiping
`fresh`/`retransmit`/`unacked`/`buf` (`send.rs:277–281`), and turning the
peer's clean EOF into `ReadError::Reset(0)`. Every byte not yet on the
wire is destroyed.

This build passes any test that reads the data *before* the sender drops,
and fails only the one that drops the `SendStream` after `finish()` and
then reads. **S12 must be written to drop it** — see §6.1.

The symmetric question, U7: does a `reset()`ed-then-dropped stream count
as "without `finish()`"? Literally yes, and an unconditional reset there
is harmless because the core keeps the first code. Setting
`closed_locally` on `reset()` too is the cleaner statement of intent and
costs nothing.

### 5.2 `Drop for RecvStream`

§16.2:4346–4353 and ruling 93 with its amendment. One call:

```rust
cell.blocked_readers.remove(&self.r);
if let Some(core) = cell.core.as_mut() { core.abandon_recv(now(), self.r); cell.dirty = true; }
```

**Unconditional is correct here**, and I checked rather than assumed
(working rule 11): `Connection::abandon_recv`
(`src/core/connection/mod.rs:488`) → `Streams::abandon_recv`
(`streams.rs:360`) → `retire_recv` (`streams.rs:877`), whose first two
statements are `entries.get_mut(&r)?` and `stream.recv.take()?`. A second
retirement — after a `read` that already returned `Ok(None)` or
`Err(Reset)`, both of which call `retire_recv` themselves
(`streams.rs:336`, `:340`) — takes the early return. **No double
true-up**, so no over-release of connection credit. That was the bug
worth checking for: `flow.recv_window().consume(delta)` running twice
would grant the peer more window than it is owed, which is §10.6's memory
bound failing open.

**Ruling 93's two mechanisms are entirely the core's** — `retire_recv`
sets `stream.recv_tomb` (`streams.rs:898`) and `after_half_freed`
(`:903`) advances the watermark only when `is_fully_closed()`. 4b calls
one verb; it must **not** try to mirror the tombstone logic in the shell.

### 5.3 How the frame gets out — `Drop` cannot await

The chain, and every link exists today:

1. `core.reset(now, r, 0)` (`mod.rs:443`) queues the RESET_STREAM and
   calls `self.pump(now)`, which seals synchronously (§16.7, ruling 114 —
   a mutating call flushes everything the ledger admits). RESET_STREAM
   goes out on `seal_quiet` (ruling 98).
2. `cell.dirty = true`.
3. **Outside the borrow**, `self.shell.mark_dirty(self.conn)` →
   `Command::Dirty(id)` on an `mpsc::UnboundedSender`
   (`src/shell/shared.rs:218–220, 430–432`), which never blocks and never
   fails observably.
4. The driver wakes, `serve_connection` drains `poll_output()` to
   `Timeout`, and `Wire::send_to` puts it on the wire.

**If the driver is already gone**, `Shell::send` swallows the closed
channel (`shared.rs:431`) and `cell.core` is `None`
(`driver.rs:605, :1061`), so step 1 is skipped and the drop is a silent
no-op — the same answer `Connection::close_now` gives
(`connection.rs:146–148`). §15.4's endpoint-dropped row: nothing
transmitted.

**Drop outside the `LocalSet`.** `shared::now()` is
`tokio::time::Instant::now().into_std()` (`shared.rs:40–42`). The
precedent exists — `Drop for Connection` already calls `close_now`, which
already calls `now()` (`connection.rs:140`) — but I could not find a test
that drops a handle **outside** a runtime context, and `FlakyWire` cannot
create one. **4b-impl must verify** whether `tokio::time::Instant::now()`
panics with no runtime entered, and if it does, the two `Drop` impls need
a guard. This is risk R4 and it is exactly working rule 13's shape: the
fixture models a network, not a process.

### 5.4 The interaction §16.2 does not cover: stream handles and ruling 88

`ShellState::handles` counts *"`Endpoint`, `Connecting`, `Connection`"*
(`shared.rs:321–327`) — a list, and 4b adds three types to a world it
already enumerates. Whether a `SendStream` is a "handle" decides:

- whether holding a `SendStream` keeps the driver alive after the
  application drops the `Connection`; and
- whether dropping the last `Connection` while a `SendStream` lives fires
  `close(NO_ERROR, "")` **underneath** that stream, making every
  subsequent `write` return `ConnectionLost`.

This is **OQ-1**, the highest-cost question in the slice. Nothing in
§16.2, §16.3 or ruling 88 answers it, and both answers are implementable.

### 5.5 Drop order inside `BiStream`

`BiStream { send, recv }` drops `send` first: RESET_STREAM (if
unfinished), then `abandon_recv`. Both frames leave in one pump, since
step 3 above is one `Dirty`. Nothing in the spec fixes this order; it is
worth a comment, and worth a test that asserts **both** effects are
observed by the peer from a single `BiStream` drop — a build that
implements `Drop` on `BiStream` itself and forgets one half passes any
test that checks only the other.

---

## §6. Story-to-test mapping

Every test is `#[tokio::test(start_paused = true)]` over
`testutil::Network` + `FlakyWire`, **no `sleep`** (§16.10). Per working
rule 9, each entry says **what the broken build does** — a build that
passes the test while being wrong.

### 6.1 S12 — `tests/story_streams.rs`

Name: `s12_loss_free_a_user_can_stream_over_a_reordering_duplicating_path`.
`PLAN.md` §7.1 is right that the name must carry `loss_free`: slice 4 has
no retransmission, slice 5 owns the loss half, and a name that is not a
pin lets slice 5 ship without it.

Policy: `FlakyPolicy::perfect().with_delay(base, jitter).with_duplication(rate)`
— **reorder and duplicate on, loss off**. This is materially stronger
than "lossless" suggests: it is what exercises §9.5's overlap rule and
§7.2's replay window.

| Assertion | The broken build it separates |
|---|---|
| the received `Vec<u8>` **equals** the sent one, payload `b[i] = (i % 251) as u8` | drops the overlapping tail of a duplicated frame (an off-by-N shift is invisible under a repeated-byte payload) |
| total bytes read **==** total bytes written, asserted separately | re-delivers duplicated bytes; fails differently from truncation |
| payload **≥ 64 KiB** | a single-packet test exercises nothing in §9.5 — `MAX_PLAINTEXT` is 1170 (`src/constants.rs:141`) |
| `read()` returns `Ok(None)`, and a **second** `read()` returns `Ok(None)` again | a build where EOF is not sticky, or where the second read hangs |
| **`finish().await`, then `drop(send)`, then read to EOF** | **§5.1's unconditional-reset-on-drop build.** Without the drop, that build ships. This assertion is not in `PLAN.md` §11.1 and is the single most valuable line in the file. |
| `write` returns `Ok(n)` with `n < buf.len()` at least once over 64 KiB, and the loop still delivers every byte | a `write` that silently claims the whole buffer |

### 6.2 S13 — two tests

**`s13_a_stalled_stream_does_not_block_a_concurrent_one`.** Slice 4 has
no loss recovery, so a lost datagram is never retransmitted and the
stalled stream stalls **for ever** — which is the point. Construction:
write a multi-packet payload to stream A, drop a *middle* datagram of
A's run with `FlakyPolicy::drop_at([k])`, then write to stream B.

| Assertion | Separates |
|---|---|
| B's bytes arrive **complete and equal** to what was written, **while** an `A.read()` future is still `Pending` | a single reassembly buffer keyed by offset rather than by stream; a fill loop that drains A before touching B |
| A **does** deliver its prefix before the gap | a build that discards a stream on the first gap |

**Coupling to flag:** per-stream withholding by datagram index only works
because ruling 114 makes two sequential `write()` calls never contend —
each flushes fully, so A's bytes and B's bytes are in different packets.
If ruling 114's reading is ever revisited, this test's construction goes
with it. Say so in the test's doc comment.

**`s13_stream_ids_carry_establishment_parity`.** Assert the dialler's
ids satisfy `initiated_by_connection_initiator() == true` and
`as_u64() & 0x01 == 0`, and the acceptor's the opposite, **on the same
connection in the same test**.

| Assertion | Separates |
|---|---|
| both sides asserted, opposite values | a build allocating from one shared counter — "the two sides' ids differ" passes that build |
| `dir()` correct for a uni and a bidi stream on each side (`0x02` bit) | a build that ignores the direction bit |

### 6.3 S14 — `s14_a_reset_stream_leaves_the_connection_and_its_siblings_alive`

| Assertion | Separates |
|---|---|
| `Err(ReadError::Reset(0x2a))` with the **exact** code | a build surfacing "some reset"; and code `0` cannot distinguish the peer's code from §16.2's drop default |
| a **sibling** stream completes a full write/read round trip *after* the reset | a reset that tears down the connection |
| `conn.closed()` is still `Pending` | the same |
| a **second** `read()` after the reset still returns `Err(Reset(0x2a))` | **the core's non-stickiness (C-D)**: an unlatched shell returns `Ok(None)` — a clean EOF for a stream that was violently abandoned |
| a *dropped, unfinished* `SendStream` produces exactly `Reset(0)` on the peer | conflates the drop default with an explicit reset; also the positive half of §5.1 |

`PLAN.md` §11.3's third assertion — "the connection-level credit released
by the reset equals `final_size`, observed by a subsequent write of
exactly that many bytes fitting" — is sound but expensive (it needs the
connection window bound first, i.e. ~1 MiB in flight). Keep it; note the
cost.

### 6.4 S17 — two tests

**`s17_a_slow_reader_stalls_its_own_stream_only`.**

| Assertion | Separates |
|---|---|
| with the reader not reading, `write` first returns **`Pending`** after exactly `INITIAL_MAX_STREAM_DATA` (262 144) bytes accepted — assert **the byte count**, not merely that it blocks | build B (grants unconditionally) never blocks; asserting only "it blocks" passes build A (grants nothing) |
| `write` returns `Pending`, **not** `Ok(0)` | a shell that reports blocked as a zero-length success — and, under §1.4, `Ok(0)` now means something else entirely |
| a **second** stream still accepts data while the first is stalled | **build C**: one shared ledger collapsing the two levels |
| then stall enough streams that the **connection** window (1 048 576) binds, and assert a *fresh* stream with a full stream window of its own now blocks | the same build C, from the other side |

**`s17_the_sender_resumes_when_the_reader_drains`.** After the reader
drains, the sender accepts more.

**The assertion `PLAN.md` §11.4 asks for and 4b cannot make.** It asks
that draining `WINDOW/2 − 1` emit **no** MAX_STREAM_DATA and draining
`WINDOW/2` emit exactly one. That is a frame-level count, and
`Tap::datagrams()` returns AEAD ciphertext. **It is a 4a test, not a 4b
test** — and 4a already owns §10.3's re-grant formula
(`PLAN.md` §7.3). 4b's reachable substitute:

| Assertion | Separates |
|---|---|
| after draining `WINDOW/2 − 1` bytes, the sender's `write` is **still `Pending`** | build B (grants on every read) |
| after draining one more byte (to `WINDOW/2`), the sender's `write` **completes** | build A (grants nothing) |
| the number of bytes it then accepts is bounded by the new window, not unlimited | a build that drops the ceiling on resume |

Both directions asserted is what makes this a pin rather than a name.

### 6.5 `tests/spec_streams.rs` — the §16.2 obligations reachable from
outside

`PLAN.md` §7.3 gives 4b exactly one row — *"§16.2 drop semantics for both
half-handles"* — plus the handle-drop half of the discard-credit row.
Expanded:

| Test | What the broken build does |
|---|---|
| dropping an unfinished `SendStream` → peer reads `Err(Reset(0))` | a `Drop` that does nothing; a `Drop` that resets with a different code |
| dropping a **finished** `SendStream` → peer reads `Ok(None)` | §5.1's unconditional reset |
| dropping a `SendStream` whose `write` future is **parked on credit** → peer still reads `Err(Reset(0))` | a `Drop` that skips the core call when a waker is parked; also the F10 re-entry hazard |
| dropping a `RecvStream` mid-stream, then writing more from the peer → the peer's `write` **stalls at the stream window** and the *connection* keeps working; a second stream on the same connection still writes freely | ruling 93's whole point — a build that only tombstones and never trues up wedges the connection window |
| dropping the `RecvStream` half of a `BiStream` while the `SendStream` half lives → the send half still writes, the peer still reads it | two independent lifetimes over one core stream; a `Drop` written on `BiStream` rather than on the halves |
| cancel-safety: drop a `read()` future mid-park, re-issue → **no byte lost, none duplicated** | a shell-side scratch buffer (§4.2) |
| the same for `write()`, `open_bi()`, `accept_uni()` | §4.4/§4.5's pop-without-construct leak |
| `id()` is `None` before … — **not writable, see §7 U-C-B** | — |
| `id()` keeps answering `Some` after the stream is fully closed | the uncached build (§7 U3) |

**Reachability note for the fifth row and for §5.4**: whether a
`SendStream` can outlive its `Connection` at all depends on OQ-1. Written
before OQ-1 is ruled, that test encodes a guess.

---

## §7. The unstated-scope hunt (working rule 8)

*A stated construction with an unstated or contradicted scope.*

**U1 — `impl SendStream` is five lines and 4b builds four of them.**
`acked()` (§16.2:4157) needs `ConnEvent::StreamFinished`, which
`CONTRACT-4a.md` §5 says **never fires in slice 4**. Ruling 107's list of
4b's contents does not name it. There is a stated answer, in the code
rather than the spec: `src/shell/mod.rs:40–43` — *"§16.2's stream,
message, datagram, `acked()`, `notified()` and keepalive verbs arrive
with the slices that define them and are **absent rather than
stubbed**"*. `acked()`'s defining slice is 5. **Recommendation: omit it,
and say in the module doc that it is slice 5's.** Low cost, high
confidence; listed because a list of five that a brief silently reads as
four is precisely this defect class.

**U2 — `impl RecvStream` is three lines.** It omits any way to observe a
peer's FIN without reading, and any receiver-side reset. Both are
correct: §9.9's STOP_SENDING is deferred (`PLAN.md` §12.2) and
`WriteError::Stopped` must not be added (ruling 61). Recorded as
*checked and correct*, not as a finding.

**U3 — `id()`'s `None` has one documented cause and two real ones.**
§16.2:4159 annotates `None` as "before establishment (§16.9)". But
`Streams::after_half_freed` removes the entry (`streams.rs:919`), so
`Connection::stream_id(r)` answers `None` again once the stream is fully
closed — **reachable in slice 4** for a peer-opened uni stream the
application reads to EOF. A stated construction (`Option`), a documented
scope (`before establishment`), and a second cause the text does not
reach. Recommendation in §1.4: cache. Cost of the uncached build: `id()`
becomes non-monotone, so logging a stream id after it finishes returns
`None` — the opposite of `remote_static()`'s documented "keeps answering
after the connection has died".

**U4 — `ShellState::handles` enumerates three handle kinds and 4b adds
three more.** → OQ-1. This is the highest-cost instance in the slice.

**U5 — §16.8 names `StreamId` as the waker-map key.** → conflict C-A.

**U6 — `join` is a name with no signature.** Ruling 96 says `BiStream`
lands "with `split()` and `join()`"; §16.2 never mentions `join`. What
bounds its two arguments — same stream? same connection? — is unstated.
→ OQ-5.

**U7 — "without `finish()`" does not say where `reset()` falls.**
§16.2:4345. Low cost (the core keeps the first code), but the flag must
be set by both verbs or the intent is only in the implementer's head.

**U8 — §16.2:4168 states the `open_*` park mechanism without stating its
reachability.** *"`open_bi`/`open_uni` wait for MAX_STREAMS allowance
when the cumulative limit is exhausted (§10.4), woken by
`StreamsAvailable`."* True for **uni**: a peer-opened uni stream fully
closes when its reader drains to EOF, which grants MAX_STREAMS_UNI. False
for **bidi** in slice 4: a bidi stream fully closes only when *both*
halves free, and the send half frees only on acknowledgement
(`CONTRACT-4a.md` §5, ruling 113). So **an exhausted bidi space never
replenishes in slice 4 and `open_bi` parks for ever.** Must be rustdoc'd;
no test may await it. The `open_uni` version *is* reachable and is worth
a test.

**U9 — §16.2's claim verbs and the pull model's post-death rule.**
§16.2:4225 puts `accept_bi` in "the same pull model" as `notified()`, and
§16.2:4230 says `notified()` reports death only after its retained items
are drained. Whether that carries to `accept_*` is unstated. → OQ-3.

**U10 — §16.9's early sends have no reachable handle.** → conflict C-B,
and the largest finding in the file.

---

## §8. Conflicts found — reported, NOT resolved (working rule 3)

**C-A — §16.8 keys the waker maps by `StreamId`; ruling 95 keys the
events by `StreamRef`; §16.9 says there is no `StreamId` yet.**

- Side A (`SPEC.md:4960–4963`): *"a stream verb that would wait parks its
  waker **under its `StreamId`** and is woken by the matching
  `ConnEvent`."* Repeated in code at `src/shell/shared.rs:48–51`:
  *"slice 4's per-stream blocked-readers / blocked-writers maps are the
  same type keyed by `StreamId`."* And again in ruling 107 itself
  (`rulings.md:2463`): *"per-`StreamId` waker maps under `RefCell`"*.
- Side B (ruling 95's amendment, and `CONTRACT-4a.md` §3): the four
  stream-naming `ConnEvent`s carry `StreamRef`, *"if the events keyed by
  `StreamId` … the shell could not match a wakeup to its waker before
  establishment"*. `SPEC.md:4985`: `id()` is `None` until established.

Side A is unimplementable as written. This is almost certainly a stale
token that ruling 95 did not sweep — working rule 4's exact shape, a
value changed while the prose still argues the old position. **Reported,
not resolved**; §3 assumes B and flags the assumption. Three texts need
the edit, not one.

**C-B — §16.9 says pre-establishment stream work is ordinary and
reachable; the shell hands out no `Connection` until establishment.**

- Side A (`SPEC.md:4973–4978`): *"Queued work before establishment is
  **ordinary work**: the connection core exists from `connect()`, and
  early stream opens, writes, messages, and datagrams land in ordinary
  stream/queue state, pumping when a session installs."* And
  `SPEC.md:4985`: *"`id()` returns `None` until the connection is
  established"* — a statement that presupposes an application holding a
  handle whose `id()` it can call.
- Side B (the code, checked): `Endpoint::connect` returns `Connecting`,
  and `Driver::establish` (`src/shell/driver.rs:519–555`) constructs the
  `Connection` handle **only** on `ConnEvent::Established`, resolving
  `PendingOutcome::Ready` there. The accept side reaches a `Connection`
  through `Command::AcceptChain`, by which point §6.2 stage 3 has
  completed. **There is no route by which an application holds a
  `Connection` before establishment.**

Consequences if B stands: §16.9's early opens and early writes are
unreachable from the public API; `id()` can never be `None` for the
documented reason; ruling 95's "stable across install" property —
`StreamRef`'s entire justification — is unexercised at the shell level
and testable only in 4a; and `SendStream::id()`'s `Option` is a shape
with no reachable `None` (until U3's second cause, which is a different
`None`).

Consequences if A stands: §16.2's `Connection` surface needs a route to
a pre-establishment handle — `Connecting` exposing one, or `connect()`
returning a pair — which is a **public API change**, not a 4b
implementation detail.

I am not resolving this. It is the one finding here most likely to change
what gets built.

**C-C — `accept_*` after death: pull-model drain, or immediate error?**
Both sides are in §16.2, one paragraph apart. `SPEC.md:4225–4232` puts
`accept_bi` in the same pull model as `notified()` and says a retained
item *"is never dropped on the floor between an application's two
visits"* and that death is reported only after the drain. Against it:
`core::Connection::read` (`src/core/connection/mod.rs:467`) returns
`Err(ConnectionLost)` unconditionally after death, so a handle drained
out of a dead connection can never be read — the drain would hand back a
handle with no reachable use. → OQ-3.

**C-D — `ReadError::Reset` is not sticky in the core, and `Ok(None)` is
documented as a promise the reset falsifies.** `Streams::read`
(`streams.rs:339–342`) calls `retire_recv` **and then** returns
`Err(Reset(code))`; the next `read` on that `StreamRef` finds no recv
half and returns `Ok(None)` (`streams.rs:316–318`). §16.2:4163 documents
`Ok(None)` as *"FIN reached, all data delivered"* — false of a stream the
peer reset, where data was abandoned (§9.6). An application looping
`while let Some(n) = read()?` sees the reset once and reads a clean EOF
on the retry. §1.5 latches it in the shell; **reported rather than
resolved**, because the alternative reading is that the core is right and
§16.2's annotation is merely incomplete.

**C-E — `PLAN.md` §11.4 and §11.2 ask 4b for frame-level assertions the
public API cannot make.** Not a spec conflict; a plan/fixture conflict.
`Tap` yields sealed datagrams (§2.2). Both assertions belong to 4a, which
already owns the §10.3 re-grant formula and ruling 114's round-robin
test. §6.4 gives the reachable substitutes. Recorded so the boundary
moves once, deliberately, rather than being discovered by a blind test
author at compile time.

---

## §9. Open questions, ranked by cost of a wrong answer

**OQ-1 — Do stream handles count as handles for §16.3's driver lifetime
and ruling 88's coincident-drop rule?**
*Cost: highest in the slice.* Getting it wrong is a public behavioural
defect either way, and it is not fixable later without a breaking change.

- **(a) They count.** `SendStream::new` calls `shell.acquire()` and
  `cell.handles += 1`; `Drop` releases. Holding a `SendStream` keeps the
  driver alive and defers `close(NO_ERROR, "")` until the stream goes
  too. *Cost if wrong:* a process that drops its `Endpoint` and
  `Connection` but leaks one `SendStream` keeps the driver task and the
  socket alive for ever — §16.3's "dropping every handle stops it"
  silently fails.
- **(b) They do not count.** Dropping the last `Connection` fires
  `close(NO_ERROR, "")` underneath any live `SendStream`, whose next
  `write` returns `ConnectionLost`. *Cost if wrong:* the natural pattern
  `let s = conn.open_uni().await?; drop(conn); s.write(..).await` fails,
  and it fails **silently at runtime** rather than at compile time.
- **(c) They keep the *cell* alive but not the *process* count**: bump
  `cell.handles` (deferring the connection's CLOSE) but not
  `ShellState::handles` (so the driver still stops when the last
  `Endpoint`/`Connection`/`Connecting` goes).

**Recommendation: (a), full counting.** Three reasons. §16.3:4409
enumerates *"`Endpoint`, staged objects, `Connection`, and stream
handles"* as the thin clients over the driver's state — stream handles
are named in that list, and the very next sentence is *"The driver lives
while any handle lives"*. Ruling 62's exclusions are argued
individually (a staged object *"does not keep the driver alive"*, a
`closed()` future *"owns nothing and merely observes"*); neither argument
reaches a `SendStream`, which owns a send half and can emit a wire frame
from its `Drop`. And a stream handle whose `Drop` must put RESET_STREAM
on the wire (§5.1) needs the driver to still be there to put it there —
under (b) that frame is silently lost whenever the `Connection` was
dropped first, which is the ordinary shape of a task that owns a stream.
Under (a), ruling 88 then governs the genuinely-last drop and nothing is
transmitted, which is already the ratified answer.

**OQ-2 — Is `poll_*` `pub(crate)` or `pub`?**
*Cost: low but irreversible in one direction.*
- `pub(crate)`: slice 8's `compat/` reaches it (same crate). Promotion
  later is additive.
- `pub`: consumers can build their own adapters without waiting for
  slice 8; but the signatures are frozen under semver from 4b onward,
  including the `key: u64` parameter on `poll_open_*`, which is an
  implementation detail.

**Recommendation: `pub(crate)`.** It matches `Connection::poll_close` and
`poll_closed`, which are private today (`src/shell/connection.rs:128,
190`), and §16.2's surface is a list — publishing verbs it does not name
is the additive-vs-breaking asymmetry `Connection::id()`'s absence was
already decided on (`connection.rs:94–100`).

**OQ-3 — After the connection dies, does `accept_bi`/`accept_uni` drain
the unclaimed queue before reporting `ConnectionLost`?** (Conflict C-C.)
*Cost: medium.* It changes one branch, but it changes what a
correctly-written application observes at teardown.
- **(a) Report death immediately.** Simple; matches the ordering §4.5
  needs (check the latch before popping).
- **(b) Drain first, then report** — the `notified()` rule, applied to
  the verb §16.2 says shares its model.

**Recommendation: (a), with the reason recorded rather than assumed.**
Under (b) the handle handed back is inert: `core::Connection::read`
refuses after death (`mod.rs:467`), so the application receives a
`RecvStream` on which every `read` is `Err(ConnectionLost)` — the pull
model's promise ("never dropped on the floor") is satisfied in letter and
void in substance. (b) also costs a `RecvStream` whose `Drop` calls
`abandon_recv` on a dying core. If the maintainer prefers (b), it must
come with a rule for what `read` does on such a handle, and that rule is
a core change, not a shell one.

**OQ-4 — Does an empty `buf` short-circuit `read`, mirroring ruling
110's rule for `write`?**
*Cost: medium — it is ruling 110's exact failure mode on the other side.*
Ruling 110 ruled the write case: a zero-length write is `Ok(0)` and the
shell must check its own buffer length "or it parks forever on an empty
write of its own making". `read` has the same trap and no ruling: the
core's `Ok(Some(0))` means "no data", so a shell that parks on it after
being handed an empty `buf` parks for ever waiting for data it could not
copy anywhere.

**Recommendation: yes — short-circuit `buf.is_empty()` to
`Ok(Some(0))` before touching the core**, exactly as `write`
short-circuits to `Ok(0)`. It makes both handle-surface conventions
identical and unambiguous (§1.4, §1.5), and it keeps `Pending` as the
single meaning of "wait". The alternative — call the core and translate —
means `Ok(Some(0))` at the handle carries two meanings again, which is
the distinction that hung the reader in 4a.

**OQ-5 — What is `BiStream::join`'s signature and what bounds its
inputs?** (U6.)
*Cost: low-medium; it is public API and cannot be narrowed later.*
- **(a) `join(send, recv) -> Result<Self, (SendStream, RecvStream)>`** —
  checks both halves name the same `StreamRef` on the same
  `ConnectionId`, hands them back unchanged on mismatch.
- (b) `join(send, recv) -> Self` with a `debug_assert` — a release build
  then holds a `BiStream` whose two halves are different streams, whose
  `id()` is a lie and whose `Drop` resets a stream the caller did not
  name.
- (c) No `join` at all — but ruling 96 names it, and `split` without
  `join` makes the pair non-reconstructible after a `select!`.

**Recommendation: (a).** No new error type, so §18.1 stays closed
(ruling 61); the mismatch is a programmer error but §16.2's own ruling 44
precedent is that *"rejection is a `Result`, never a panic"* for
anything reachable across FFI, and a returned pair costs nothing.

**OQ-6 — Does `SendStream::acked()` exist in 4b?** (U1.)
*Cost: low.* **Recommendation: no** — absent rather than stubbed, per
`src/shell/mod.rs:40–43`, with a module-doc line naming slice 5. Listed
because ruling 107's contents list omits it and an omission is invisible
until someone builds against it (ruling 71).

**OQ-7 — Are the waker maps `BTreeMap<StreamRef, Wakers>` or
`BTreeMap<StreamRef, Waker>`?**
*Cost: low.* **Recommendation: `Wakers`** — §3.1's argument: `take_all`'s
`#[must_use]` makes F10's ordering the only spelling the type permits.

---

## §10. Risk register

**R1 — `Drop for SendStream` resets a finished stream.** (§5.1.)
*Most likely defect in the slice.* The core's `reset` is not idempotent
with respect to `fin`, and the mistake is invisible unless a test drops
the handle after `finish()`. **Caught by:** S12's mandatory
`drop(send)` step and `spec_streams.rs`'s "dropping a finished
`SendStream` → peer reads `Ok(None)`".

**R2 — a parked writer or reader is never woken on driver death.**
`Driver::latch` is the only sweep and today it takes `closed_wakers`
only (§3.3). **Unreachable from `FlakyWire` by construction** — the
fixture models a network, not a process, and cannot express "this driver
panics" (F1's exact shape). **Caught by:** an inline test in
`src/shell/mod.rs` (4b-impl's file) that parks a `write` on credit,
forces the driver to stop, and asserts the future resolves
`Err(ConnectionLost::EndpointDropped)` rather than hanging. Must be
written deliberately; nothing will produce it by accident.

**R3 — F10 reintroduced in a new arm.** Six `ConnEvent` arms, each
needing borrow-then-wake. `take_all`'s `#[must_use]` guards it, but only
if the map holds `Wakers` (OQ-7). **Caught by:** the existing
`a_waker_that_polls_inline_does_not_re_enter_a_live_borrow` pattern from
FIXES-3b-round2, extended to a parked reader and a parked writer —
`InlineWaker` re-polling from inside `wake()`. **Not reachable with
tokio's own wakers**, which only push to a run queue; this is why it must
be closed by construction and tested with a hostile waker.

**R4 — `Drop` outside the `LocalSet`.** (§5.3.) `shared::now()` calls
`tokio::time::Instant::now()`; whether that panics with no runtime
entered is **unverified**, and a panic in a `Drop` during unwinding
aborts the process. `FlakyWire` cannot express it. **Caught by:** a
plain `#[test]` (no `#[tokio::test]`) that builds a handle inside a
runtime, moves it out, and drops it — or, if that is not constructible, a
`std::thread` drop. 4b-impl must check `tokio`'s behaviour directly.

**R5 — the waker maps grow without bound.** FIXES-3b finding 3 was
`Driver::waiting` growing 132.3 bytes per cancelled `accept()` for the
endpoint's life; a map keyed by a value the *application* controls is the
same shape with a larger multiplier (ruling 107 names it). **Caught by:**
§3.4's park-then-assert-non-empty-then-drop-then-assert-empty test.
A bound-only assertion ("the map has ≤ N entries") passes a build that
never inserts — working rule 9.

**R6 — a blind test author writes frame-level assertions.** (§C-E.)
`PLAN.md` §11.2 and §11.4 ask for "some packet carries frames for both
streams" and "draining `WINDOW/2 − 1` emits no MAX_STREAM_DATA". Neither
is reachable from `tests/`, because `Tap` yields ciphertext. **Caught
by:** stating it in the test author's brief, not at integration. This is
the slice-4a `CONTRACT-4a.md` accident in a new costume — the brief's
inputs must say what the fixture cannot see.

**R7 — the fixture cannot express a socket.** Working rule 13, restated
for 4b. `FlakyWire` loses, delays, duplicates, reorders and now fails
sends. It cannot express: a driver panic (R2), a drop outside a runtime
(R4), an inline-polling waker (R3), or a `RefCell` re-entry. **Three of
4b's seven risks are in that set.** Every one of them must be reached by
an in-crate test with a hand-built hostile waker or a deliberately
stopped driver — not by a flow test. Budget for them explicitly, because
no amount of story-testing will produce them.

**R8 — `open_bi` parked on an exhausted bidi space hangs a test for
ever.** (U8.) A test author writing "exhaust the limit, then assert
`open_bi` resumes" has written a hang, and on a paused clock a hang is
not a timeout — it is a test that never returns. **Caught by:** the
brief. Use `open_uni` for the exhaustion-and-resume test; `open_bi`'s
park may be asserted only as "still `Pending`".

**R9 — the two blind agents disagree about whether a stream handle keeps
the connection alive.** (OQ-1.) Unresolved, this splits the two agents'
mental models of every drop test in §6.5. **It must be ruled before
dispatch**, not at integration — this is the ruling that `CONTRACT-4a.md`
being uncommitted at cut time cost slice 4a ten minutes and two semantic
guesses (working rule 14).

---

## §11. Notes back to the maintainer

**Sequencing.** Working rule 14: `CONTRACT-4b.md` must be **committed
before either agent's worktree is cut**, and the brief must name the
commit. Slice 4a's test author was cut at `fdf5972` while its binding
contract landed at `74fa5f2`, and it reconstructed the API for ten
minutes.

**What must be ruled before dispatch:** OQ-1 (it changes every drop
test), OQ-3 and OQ-4 (they change return conventions the blind test
author writes assertions against), and OQ-5 (public API shape). OQ-2,
OQ-6 and OQ-7 can be taken as the recommendations above and revisited at
integration.

**What must be reported and not silently carried:** C-A (three texts to
sweep, including one in `src/shell/shared.rs`), C-B (the largest one),
C-C, C-D, C-E.

**One thing in the brief I want to flag rather than do silently
(working rule 5).** The brief says §3 should describe "§16.8's
per-`StreamRef` blocked-reader/blocked-writer waker maps", and ruling
107's own text says "per-`StreamId` waker maps". Those are two different
keys and only one can be built. I have written §3 against `StreamRef`,
because §16.9 makes `StreamId` unavailable at park time and ruling 95
already converted the events, but I have **not** treated the brief's
phrasing as authority to edit §16.8, ruling 107, or the `Wakers` doc
comment. Those three edits are the maintainer's, and C-A is where I have
put them.

---

---

## Scratch: raw notes as gathered

*(appended as I read; promoted into sections above)*

### Sources read (with line anchors)

- `CONTRACT-4a.md` whole (270 lines).
- `SPEC.md` §16.2 = 4094–4401; §16.3 = 4403–4470; §16.8 = 4955–4969;
  §16.9 = 4971–4988; §16.10 = 4990–5026; §16.11 begins 5028.
- Section index: §9.1 2874 · §9.2 2900 · §9.3 2925 · §9.4 2940 · §9.5 2953
  · §9.6 2990 · §9.7 3041 · §9.8 3064 · §9.9 3210 · §10.1 3219 ·
  §10.3 3273 · §10.4 3345 · §10.5 3372 · §10.6 3402 · §16.1 4043 ·
  §16.4 4620 · §16.5 4860 · §16.7 4940.

### §16.2 verbatim surface for 4b (SPEC.md:4105–4165)

```rust
impl Connection {
    pub async fn open_bi(&self)  -> Result<BiStream, ConnectionLost>;   // .split() → the pair
    pub async fn open_uni(&self) -> Result<SendStream, ConnectionLost>;
    pub async fn accept_bi(&self)  -> Result<BiStream, ConnectionLost>; // .split() → the pair
    pub async fn accept_uni(&self) -> Result<RecvStream, ConnectionLost>;
}
impl SendStream {
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError>;
    pub async fn finish(&mut self) -> Result<(), WriteError>;
    pub async fn acked(&mut self) -> Result<(), WriteError>;   // ruling 47
    pub fn reset(&mut self, error_code: u64);
    pub fn id(&self) -> Option<StreamId>;   // None before establishment (§16.9)
}
impl RecvStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
        // Ok(None) = FIN reached, all data delivered
    pub fn id(&self) -> Option<StreamId>;   // None before establishment (§16.9)
}
```

Prose obligations attached (SPEC.md:4168–4184, 4337–4401):
- `open_bi`/`open_uni` **wait** for MAX_STREAMS allowance when the
  cumulative limit is exhausted (§10.4), woken by `StreamsAvailable`.
- `write` waits for stream **and** connection credit (§10.1).
- `finish()` resolves when the FIN is accepted into the stream's send
  state; errors surface as `WriteError`.
- Dropping a `SendStream` **without `finish()`** resets it with error
  code 0.
- Dropping a `RecvStream` abandons the receive half — ruling 93 + its
  amendment (both tombstone mechanisms).
- Dropping the last `Connection` handle performs `close(NO_ERROR, "")`;
  dropping *every* handle stops the driver and nothing is transmitted
  (ruling 88).
- "The types are normative in shape; an implementation may rename."
  (SPEC.md:4335)

### §16.3 (ruling 53) seam table (SPEC.md:4423–4428)

| Surface | Mechanism |
|---|---|
| Endpoint verbs (`accept`, staged) | command channel + oneshot |
| `connect` | command send only + synchronous cell read |
| **Connection data path** — `write`, `read`, `open_*`, `accept_*`, `send_message`, `recv_*`, `close`, `acked`, `notified` | **shared cell `Rc<RefCell<_>>` + §16.8 blocked-readers/blocked-writers waker maps** |
| Accessors | reads of the same cell |

SPEC.md:4453 — "**Every mutating borrow ends by marking the connection
dirty and waking the driver**, which drains `poll_output()` to `Timeout`
and performs the I/O."

SPEC.md:4458–4466 — each data-path verb is written **once** as
`poll_*(&mut self, cx) -> Poll<_>`; `async fn` is `poll_fn` over it;
§16.11's `AsyncRead`/`AsyncWrite` is the same function error-mapped.

### §16.8 verbatim (SPEC.md:4955–4969)

> Per-stream wakers key the shell's blocked-readers/blocked-writers maps
> (the quinn pattern): a stream verb that would wait parks its waker
> **under its `StreamId`** and is woken by the matching `ConnEvent`.

**Flag (candidate conflict C-A):** `StreamId` here vs ruling 95 /
`CONTRACT-4a.md` §3, where every stream-naming `ConnEvent` is keyed by
`StreamRef`, and §16.9 (SPEC.md:4985) says `id()` is `None` until
establishment. Keying a waker map by `StreamId` is *impossible*
pre-establishment, which §16.9 says is exactly when work is in flight.

### §16.9 verbatim points (SPEC.md:4971–4988)

- Early opens/writes are **ordinary work**; no special mechanism.
- Stream IDs assigned **at establishment**; pre-establishment handles
  hold core-internal indices; `id()` returns `None` until established.
- "no frame is emitted before install".

### The code as shipped — verified reads

**`src/core/connection/mod.rs:386–553`** — the verb surface matches
`CONTRACT-4a.md` §2 exactly, **plus two entry points the contract does
not list**:

```rust
pub(crate) fn flush(&mut self, now: Instant);                       // mod.rs:505
pub(crate) fn on_reset_acked(&mut self, now: Instant, r: StreamRef); // mod.rs:550
```

Arity check against the contract (all confirmed as-shipped):
`open(dir)`, `stream_id(r)`, `accept(dir)`, `write(now, r, data)`,
`finish(now, r)`, `reset(now, r, code)`, `read(now, r, buf)`,
`abandon_recv(now, r)`, `reassembly_capacity()`, `role()`,
`on_ack_range(now, r, range)`, `on_lost_range(now, r, range)`.

**Which verbs check "connection lost" and which do not** (mod.rs):
| verb | dead-connection behaviour |
|---|---|
| `write` | `self.lost()?` → `Err(WriteError::ConnectionLost(_))` (mod.rs:423) |
| `finish` | `self.lost()?` → `Err(WriteError::ConnectionLost(_))` (mod.rs:436) |
| `read` | → `Err(ReadError::ConnectionLost(_))` (mod.rs:467) |
| `reset` | **no check** — mutates and pumps regardless (mod.rs:443) |
| `abandon_recv` | **no check** — mutates and pumps regardless (mod.rs:488) |
| `open` / `accept` / `stream_id` | **no check** — pure stream-table ops |

**Error-variant reachability in slice 4** (`src/error.rs:239–260`):

```rust
pub enum WriteError { Reset(u64), ConnectionLost(ConnectionLost), Finished }
pub enum ReadError  { Reset(u64), ConnectionLost(ConnectionLost) }
```

- `SendHalf::write` (send.rs:216–219) returns **`Finished`** when
  `self.fin || self.reset.is_some()`. So **`WriteError::Reset` is
  unreachable from `write`** — a locally reset send half reports
  `Finished`, not `Reset`. In slice 4 `WriteError::Reset` is reachable
  from **no core verb at all** (its only §16.2 consumer is
  `SendStream::acked()`, ruling 47, which needs `StreamFinished`, which
  CONTRACT-4a §5 says never fires in slice 4).
- `SendHalf::finish` (send.rs:245–251) is **idempotent**: a second
  `finish()` after `finish()` is `Ok(())`; after `reset()` it is
  `Err(Finished)`.
- `SendHalf::reset` (send.rs:266–269) is idempotent — a second `reset`
  is a silent no-op, keeping the **first** error code.
- `Streams::write`/`finish` map a **missing entry or missing send half**
  to `WriteError::Finished` (streams.rs:261–262, 276–277). So "this
  stream does not exist" and "you finished it" are the same value.

**`Streams::read` (streams.rs:307–344) — the sticky/non-sticky
asymmetry, and it is not symmetric:**
- missing entry, or `stream.recv == None` → **`Ok(None)`** (EOF).
- `ReadOutcome::End` → `retire_recv(r)` then `Ok(None)`. EOF is therefore
  **sticky** — the half is gone, and every later read is `Ok(None)`.
- `ReadOutcome::Reset(code)` → `retire_recv(r)` then
  `Err(ReadError::Reset(code))`. **`Reset` is NOT sticky**: the half is
  retired by the same call, so a *second* `read` on that `StreamRef`
  returns **`Ok(None)`**, not `Err(Reset)` again. Whether the shell
  latches the reset is unspecified — see OQ.

**`src/shell/shared.rs`** (read whole, 533 lines):
- `Wakers { next: u64, parked: BTreeMap<u64, Waker> }` with `key()`
  (monotone, never reused), `park(key, cx)` (`will_wake` refresh),
  `unpark(key)` (idempotent remove), and
  `#[must_use] take_all() -> Vec<Waker>` (shared.rs:111–114).
- `take_all`'s doc (shared.rs:95–110) **is finding F10's fix already
  landed for `closed()`**: it hands the wakers back instead of waking
  them, because `Waker::wake` runs consumer executor code that may
  re-enter `poll_closed` and re-take the same `RefCell` borrow. "tokio's
  own wakers only push to a run queue, so nothing in this crate's tests
  could reach it — which is precisely why it had to be closed by
  construction and not by argument."
- `WakerSlot<F: FnMut(u64)>` (shared.rs:123–142) — key + release
  closure, released on `Drop`. This is the cancel-safety mechanism for
  `poll_fn`-shaped verbs.
- **`Wakers`' own doc, shared.rs:48–51, says slice 4's maps are "the
  same type keyed by `StreamId`".** Same stale key as §16.8 — see
  conflict C-A. This comment must change in 4b.
- `ConnCell<S: Handshake>` fields: `core: Option<CoreConnection<S>>`,
  `remote_address`, `closed: Option<ConnectionLost>`, `closed_wakers:
  Wakers`, `notifications: NotificationSlots`, `dirty: bool`,
  `handles: usize`.
- `ShellLink` trait: `release() -> bool` (true = last handle **in the
  process**), `mark_dirty(id)`, `acquire()`.
- `ShellState::handles` doc (shared.rs:321–327): *"How many handles —
  `Endpoint`, `Connecting`, `Connection` — exist. Staged objects are
  deliberately **not** counted … Nor is a `closed()` future."* **A list.
  4b adds three more handle types and the list does not say what bounds
  it.** See OQ-1 / working-rule-8 hunt.
- `resolve_slot` (shared.rs:295–304) — the "mutate under borrow, then
  drop + wake outside it" template, with the doc explicitly noting the
  ordering hazard is a property of current call sites.

**`src/shell/connection.rs`** (read whole, 276 lines):
- `pub struct Connection<S: Handshake>` holding
  `shell: Rc<dyn ShellLink>`, `cell: Rc<RefCell<ConnCell<S>>>`,
  `id: ConnectionId`, `remote_static`, `session_id`.
- **`Connection` is NOT `Clone`** — there is no `Clone` impl and no
  `try_clone`. `cell.handles` is therefore always exactly 1 today.
- `close_now` (connection.rs:135–153) is the template every 4b mutating
  verb copies: `{ borrow_mut; core.as_mut(); mutate; cell.dirty = true }`
  then **outside the borrow** `self.shell.mark_dirty(self.id)`.
  `core == None` → do nothing at all.
- No `Connection::id()` accessor, deliberately (connection.rs:94–100) —
  and the reason given is working rule 8: "§16.2's `Connection` surface
  is a list, and in this project a list is read as exhaustive".
- `closed()` (connection.rs:175–199) is the exact `WakerSlot` + `poll_fn`
  cancel-safety pattern 4b's parking verbs must copy.
- `Drop for Connection` (connection.rs:258–276): decrement `cell.handles`
  → `self.shell.release()` → `if last_for_connection && !last_in_process
  { close_now(NO_ERROR, b"") }`.

**`src/shell/driver.rs`**:
- `serve_connection` (440–490) takes `cell.borrow_mut()` **only inside
  the `let output = { … }` block**, then matches outside it. So
  `publish()` (493–515) runs with **no cell borrow held** — that is the
  F10 boundary, and it already exists.
- `publish`'s stream arm (508–513) drops all six stream events on the
  floor today, with a comment saying 4b replaces exactly that arm.
- `latch` (992–1004) — set `closed`, `take_all()`, drop borrow, wake.
- `stop` (1047–1071) — sets `driver_stopped`, drains the command queue,
  then per connection `latch(EndpointDropped)` + `core = None` +
  resolve the `Connecting`. **`latch` is the only thing that wakes
  cell-parked waiters on driver death.** 4b's new maps must be swept
  here or a parked writer hangs for ever on a driver panic.
- `Drop for Driver` (1100–1104) calls `stop()`, so the sweep runs on an
  unwind too.
- `core = None` is set in exactly two places: driver.rs:605 and
  driver.rs:1061 (`stop`).

**Public surface today** (`src/lib.rs`):
- `pub use shell::{Claimed, Connecting, Connection, Endpoint,
  EndpointBuilder, Intro, Proven};` (lib.rs:139)
- `pub use crate::core::{ConnectionId, Dir, IntroId, StreamId,
  Timestamp};` (lib.rs:166) — **`StreamId` and `Dir` are already
  public.**
- `src/shell/mod.rs:53–55` re-exports `Connection`, `Connecting`,
  `Endpoint`, `EndpointBuilder`, `Claimed`, `Intro`, `Proven`.
- `StreamRef` and `StreamsExhausted` are `pub(crate)`
  (`src/core/mod.rs:59`).


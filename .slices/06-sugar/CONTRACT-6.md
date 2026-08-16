# CONTRACT-6 — the binding API contract for slice 6 (messages and datagrams)

**Status: BINDING.** All three slice-6 agents compile against this and none
may change it. If you believe something here is wrong, **say so in your
report and implement it as written anyway** (working rule 5).

Extracted from `.slices/06-sugar/PLAN-6.md` §2, with every open question
resolved by **Round 26 (rulings 150–158)**.

---

## 0. The rulings that govern slice 6 — these override anything below

| # | Decision |
|---|---|
| **150** | **`send_message` admits the whole payload or nothing.** §10.6 makes credit the buffer commitment, so accepting beyond the peer's credit would put an unbudgeted 32 MiB into §17.5. Not in tension with ruling 134: 134 lets `write()` outrun the **congestion** window, which is not a buffer bound; flow control still gates admission. A refusal for **connection credit** needs a wake no event provides today — slice 6 mints one `pub(crate)` `ConnEvent` variant for it (internal, additive, nothing on the wire moves). |
| **151** | **`core::recv_message` takes `now: Instant`.** It retires a receive half (owing MAX_DATA) and can emit RESET_STREAM, and §16.7 seals inside the mutating call. **`recv_datagram` takes no `now`** — datagrams are flow-control exempt (§10.7), so claiming one emits nothing. State that asymmetry in the rustdoc so nobody "fixes" it. |
| **152** | **The post-death drain covers `recv_message` and `recv_datagram`**, not just ruling 128's two named verbs. Appendix B ratifies an obligation the short list cannot satisfy. Same precedence table; **parking is still never permitted on a dead connection**. |
| **153** | The overflow predicate is the **highest received offset** with no pinned final size — **and `send_message` MUST carry the FIN on its final data frame**, never a separate empty FIN. Without that, a conforming 262 144-byte message whose FIN is in flight is reset with `MESSAGE_OVERFLOW`, producing ruling 59's "transfers die at 256 KiB" post-mortem pointing at the wrong cause. |
| **154** | §10.3's fifth retirement trigger ("final size reached with no reader") is **struck** — §9.7 never contained it. **Retention until claimed is the rule.** |
| **155** | **One datagram per packet, packed before the stream fill.** Datagrams-after-the-fill is the smallest diff and the worst outcome: a saturated stream starves them entirely and the bounded queue evicts continuously — silent loss with no distinguishing counter. Also: `MAX_DATAGRAM_PAYLOAD` (1169) + type + length exceeds `MAX_PLAINTEXT` (1170), so **the `0x30` extends-to-end form is mandatory**, not an optimisation — without it the ratified maximum datagram cannot be sent at all. |
| **156** | The pending-claim flag is cleared by a `recv_message()` returning `Some`. **Two** drop counters (send and recv). `earns_stream_credit` stays `true` everywhere. |
| **163** | `core::send_message` returns **`Result<SendMessage, MessageError>`** with `enum SendMessage { Sent, Blocked }`. "Not now" was never an error — it is `write`'s `Ok(0)` and `accept`'s `None`, in the success type. §18.1 stays closed and untouched. `Blocked` carries no reason: one `message_senders` map, three wake sources. |
| **164** | The overflow predicate's second clause (**highest received offset, no final size pinned**) is now stated in §15.3's registry and §9.8's prose too. §9.8's *"which it cannot be, having no FIN"* argued as fact the thing ruling 153 had to add — do not take it as the argument. |

---

## §2. The binding API contract (becomes `CONTRACT-6.md`)

*House style of `CONTRACT-5a/5b`: a **BINDING** banner, §0's rulings table,
signature blocks, then the algorithms, then the appendix of prohibitions. Style
rule carried from `CONTRACT-5a.md:35-40`: **every return value is stated for
every state, including the ones that look obvious.** Three items below were marked
⚠ **RULING REQUIRED**; Round 26 ruled all three (150, 151, 155) and §0's table
is the answer. Round 27 adds 159–162.*

### 2.1 New module: `src/core/connection/datagram.rs` (§11)

```rust
/// §11.3's two bounded queues and §11.5's counters. Core state, not shell
/// state — SPEC §11.3 says so in terms.
pub(crate) struct Datagrams {
    send: VecDeque<Vec<u8>>,   // bounded by DATAGRAM_SEND_QUEUE  = 64
    recv: VecDeque<Vec<u8>>,   // bounded by DATAGRAM_RECV_QUEUE  = 64
    drops: DatagramDrops,
}

/// §11.5's counters. **Two**, not one — §11.3 and §18.2 both say "counters".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DatagramDrops {
    pub(crate) send: u64,
    pub(crate) recv: u64,
}

impl Datagrams {
    /// §11.3 drop-oldest, newest always accepted. Returns `true` iff an
    /// eviction occurred (the caller traces and counts).
    fn push_send(&mut self, data: Vec<u8>) -> bool;
    fn push_recv(&mut self, data: Vec<u8>) -> bool;
    fn pop_send(&mut self) -> Option<Vec<u8>>;   // used by the fill pass
    fn peek_send(&self) -> Option<&[u8]>;        // to size the frame before committing
    fn pop_recv(&mut self) -> Option<Vec<u8>>;   // recv_datagram()
    fn has_send(&self) -> bool;                  // feeds Connection::has_output()
}
```

**`push_send` / `push_recv` — every outcome**

| Queue state before | Effect | Return | Counter |
|---|---|---|---|
| `len < 64` | the new datagram is appended | `false` | unchanged |
| `len == 64` | **the front is dropped**, then the new datagram is appended — *"the arriving or newly-sent datagram always enters"* (§11.3) | `true` | the matching counter `+= 1` |

**Not representable:** rejecting the newest. A build that returned "queue full,
try later" for `send_datagram` contradicts §16.2's *"`send_datagram` never
waits"* and S15's *"drops oldest under pressure rather than blocking"*.

### 2.2 `Frame::Datagram` — `src/core/connection/frame.rs` (edit)

```rust
pub(crate) enum Frame {
    // ... the ten existing variants, unchanged ...
    /// `0x30`/`0x31` — §11's unreliable payload. §8.4.
    Datagram(Datagram),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Datagram {
    pub(crate) data: Vec<u8>,
    /// `false` ⇒ type `0x30`, no length field, **extends to the end of the
    /// plaintext and must be the packet's final frame**.
    /// `true`  ⇒ type `0x31`, explicit varint length.
    pub(crate) len_present: bool,
}
```

Five dispatch points gain an arm — `type_code`, `encoded_len`, **`extends_to_end`**,
`encode`, and `parse` (`frame.rs:91-152`, `:600-649`). `is_ack_eliciting`
(`frame.rs:687`) and `retransmission` (`frame.rs:726`) **already answer for both
codes and MUST NOT be touched** (C4).

`Frame::extends_to_end` becomes:
```rust
pub(crate) fn extends_to_end(&self) -> bool {
    match self {
        Frame::Stream(s) => !s.len_present,
        Frame::Datagram(d) => !d.len_present,
        _ => false,
    }
}
```

**`Datagram::parse_body` — every outcome**

| Input | Result |
|---|---|
| type `0x30`, `n` bytes remain in the plaintext | `Ok((Datagram { data: n bytes, len_present: false }, n))` |
| type `0x31`, `length` varint then `length` bytes present | `Ok((Datagram { data, len_present: true }, consumed))` |
| type `0x31`, `length` overruns the remaining plaintext | `Err(Structural::…)` — §8.4's *"`length` overrunning the plaintext"* |
| type `0x30` **and it is not the final frame** | `Err(Structural::…)` — §8.4's second structural error |

**[CORRECTED 2026/08/16 — ruling 159]** *The check exists.* `frame.rs:705`
returns `Structural::TrailingFrame` for exactly this, in exactly the loop this
paragraph proposes, and its comment records that it is deliberately
dead-by-construction. The ¬LEN STREAM case is **not** accepted silently. So the
DATAGRAM case is covered as soon as `Frame::Datagram` answers `extends_to_end()`
truthfully — **implement that and add nothing else.** The paragraph below is kept
for its reasoning, which is sound, and its conclusion, which is wrong:

> ⚠ *(superseded)* **The `0x30`-not-final check has nowhere to live today.** `parse_body` receives a `&[u8]` and returns bytes consumed; a `0x30`
body by definition consumes the rest, so "not final" is **unrepresentable inside
`parse_body`** — after it returns, the cursor is at the end and the loop
terminates. The check belongs in `Frame::parse`'s **loop**. This is the same
shape as the ¬LEN STREAM case, which `parse` today accepts **silently**
(`frame.rs:298-306` consumes `pt.len() - at` with no positional check). Stated
here because a blind implementer will otherwise write the arm and believe the
error case is covered by construction. `Structural` (`frame.rs:552-580`) needs
one variant or a reused one, and it must be **nameable in the trace** —
`Structural`'s own doc says its variants exist for the trace, not for per-case
behaviour.

### 2.3 Changes to `core::Connection` — `src/core/connection/mod.rs`

```rust
// ── §11 datagrams ────────────────────────────────────────────────────────
/// §16.4's `send_datagram`. Never blocks; §11.3's drop-oldest absorbs pressure.
pub(crate) fn send_datagram(&mut self, now: Instant, data: &[u8])
    -> Result<(), DatagramError>;

/// §16.4's `recv_datagram`: claim the oldest queued datagram.
///
/// **Takes no `now`, and that is not an oversight**: claiming a datagram
/// emits nothing. Datagrams are flow-control-exempt (§10.7), so there is no
/// credit true-up and nothing to seal. Contrast `recv_message` below.
pub(crate) fn recv_datagram(&mut self) -> Option<Vec<u8>>;

// ── §9.8 messages ────────────────────────────────────────────────────────
/// §16.4's `send_message`. **Ruled: ruling 150** — whole-payload atomic
/// admission; the "not now" is a non-`MessageError` outcome, and a refusal
/// for connection credit is woken by the new `pub(crate)` `ConnEvent`.
pub(crate) fn send_message(&mut self, now: Instant, msg: &[u8])
    -> Result<SendMessage, MessageError>;

/// §16.4's `recv_message`: claim the oldest **complete unclaimed** uni stream
/// as one payload, then free the stream.
///
/// **Ruled: ruling 151** — `now` is added to §16.4's signature.
/// `recv_datagram` stays `now`-free (§10.7 exempts datagrams).
pub(crate) fn recv_message(&mut self, now: Instant) -> Option<Vec<u8>>;

// ── observability ────────────────────────────────────────────────────────
/// §11.5's counters. `#[cfg(test)]` — §16.2's accessor list is exhaustive and
/// does not contain this, so it is NOT public API. House pattern:
/// `mod.rs:679-701`.
#[cfg(test)]
pub(crate) fn datagram_drops(&self) -> DatagramDrops;
```

**`send_datagram` — every outcome**

| Condition | Result | Notes |
|---|---|---|
| connection already lost | `Err(DatagramError::ConnectionLost(l))` | checked **first**, mirroring `write` (`mod.rs:472`, `self.lost()?`) |
| `data.len() > MAX_DATAGRAM_PAYLOAD` (1169) | `Err(DatagramError::TooLarge)` | §11.4: *"at the handle, **before any queue**"* — so **nothing is queued and no eviction happens**. A build that evicted the oldest and *then* rejected is wrong. |
| `data.is_empty()` | `Ok(())` — a zero-length datagram is **queued and sent** | §8.4 admits `0x31` with `length = 0`; §11 states no minimum. Stated so nobody invents one. |
| otherwise, queue `len < 64` | `Ok(())` | |
| otherwise, queue `len == 64` | `Ok(())`, oldest evicted, `drops.send += 1`, **traced** | the caller still gets `Ok(())` — §11.1 promises nothing about delivery |

`send_datagram` **ends with `self.pump(now)`** — it is a mutating call that may
put a frame on the wire, and §16.7 seals inside the mutating call.

**`recv_datagram` — every outcome**

| Condition | Result |
|---|---|
| the recv queue is non-empty | `Some(payload)`, front removed (FIFO — §16.4's *"claim the oldest queued datagram"*) |
| the recv queue is empty | `None` |
| **the connection is dead but the queue is non-empty** | `Some(payload)` — the core does **not** consult `self.lost`. The death check is the shell's, *after* the core returns `None`. **Ruled: ruling 152** — the drain covers `recv_message` and `recv_datagram`. |

**`recv_message` — every outcome**

| Condition | Result |
|---|---|
| some unclaimed peer-opened **uni** stream is complete (FIN pinned **and** every byte to the final size received) | `Some(payload)` of the **oldest such stream in open order** (ruling 112's FIFO, filtered to complete). The stream leaves `unclaimed`, its receive half is **retired**, its bytes count as consumed for connection credit (§10.3), and MAX_STREAMS_UNI credit is owed to the peer (§10.4). |
| no unclaimed uni stream is complete | `None`, **and the pending-claim flag is set** (§4.2) |
| a complete unclaimed stream carries a **zero-byte** payload (FIN at offset 0) | `Some(Vec::new())` — an empty message is a message. **Not** `None`; `None` must mean "nothing to claim" or the shell parks on a delivered message for ever. |
| the connection is dead but a complete unclaimed stream remains | `Some(payload)` — the core does not consult `self.lost`. **Ruled: ruling 152.** Appendix B's message-then-close obligation requires it, and ruling 152 now says so — the sentence that read *"no ruling says so"* predated Round 26. |
| a **bidi** stream completes | never surfaced here. `recv_message` draws from `unclaimed[Dir::Uni]` **only** — §9.8 is uni sugar, and §9.8:3113-3116 makes bidi the *safe* alternative precisely because it never collides. |

**Every call to `recv_message` — including one that returns `Some` — first runs
§4.3's overflow scan.** §9.8:3177-3179: *"while a `recv_message()` claim is
pending, **and at the instant such a claim is made**"*.

### 2.4 Two new `ConnEvent` variants — `src/core/connection/mod.rs`

```rust
pub(crate) enum ConnEvent {
    // ... the eight existing variants, unchanged ...
    /// A complete message is claimable through `recv_message()`. §16.4.
    MessageReadable,
    /// A datagram is claimable through `recv_datagram()`. §16.4.
    DatagramReadable,
}
```
Both are **field-less signals**, exactly as §16.4:4867-4868 writes them, and
carry no payload — §16.4's *"signals, not payload carriers"*.

**Emission scope, stated — because working rule 8's defect is an unstated one:**
- `MessageReadable` fires **once per uni stream that becomes complete while
  unclaimed** — the one-per-item discipline ruling 99 fixed for `StreamOpened`.
  Not once per STREAM frame; not for a stream already claimed by `accept_uni()`.
- `DatagramReadable` fires **once per DATAGRAM frame admitted to the recv
  queue**, *including* one that evicted an older datagram (the queue went full →
  full, but a **new** item is claimable). It does **not** fire for the evicted
  datagram.
- Neither fires for a locally-*sent* item.
- `publish`'s match (`driver.rs:493-562`) is exhaustive with no `_` arm, so
  omitting either is a compile error, not a silent gap.

### 2.5 Shell — `src/shell/connection.rs`, `shared.rs`, `driver.rs`

```rust
impl<S: Handshake> Connection<S> {
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>;   // NOT async
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;

    pub(crate) fn poll_send_message(&self, cx: &mut Context<'_>, msg: &[u8], key: u64)
        -> Poll<Result<(), MessageError>>;
    pub(crate) fn poll_recv_message(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<Vec<u8>, ConnectionLost>>;
    pub(crate) fn poll_recv_datagram(&self, cx: &mut Context<'_>, key: u64)
        -> Poll<Result<Vec<u8>, ConnectionLost>>;
}
```
`pub(crate)` on the three `poll_*` forms follows ruling 122a — slice 8's in-crate
compat reaches them. §16.11's `messages`/`datagrams` `Stream` faces are **slice
8's**, not slice 6's (`PLAN.md:295`). `send_datagram` has no `poll_` form because
it cannot be pending. Each `async fn` is `poll_fn` over its `poll_*` with a
per-future `WakerSlot`, exactly `accept_uni`'s shape (`connection.rs:392-395`,
`:449-455`).

**`Connection::send_datagram` — every outcome**

| Condition | Result |
|---|---|
| core present, payload ≤ 1169 | `Ok(())`; sets `cell.dirty = true` and calls `shell.mark_dirty(self.conn)` — the shape of `SendStream::reset` (`stream.rs:486-510`) |
| payload > 1169 | `Err(DatagramError::TooLarge)`; **does not mark dirty** |
| death latch set | `Err(DatagramError::ConnectionLost(l))` |
| `cell.core.is_none()` | `debug_assert!(false, …)` then `Err(DatagramError::ConnectionLost(EndpointDropped))` — the house shape (`connection.rs:565-571`) |

It takes `&self`, is **not** `async`, and its cancel-safety is vacuous.

**`poll_recv_message` / `poll_recv_datagram` — precedence, in order.**
This is `CONTRACT-5b.md:104-108`'s table extended. A blind author gets it wrong
by reading §16.2's post-death paragraph, which names only `read` and `accept_*`:

1. **the core call** (`core.recv_message(now())` / `core.recv_datagram()`) →
   `Poll::Ready(Ok(payload))` if it yields `Some`;
2. **death latch** (`cell.closed`) → `Poll::Ready(Err(lost))`, **only if the core
   had nothing**;
3. `cell.core.is_none()` → `debug_assert!(false)` + `Err(EndpointDropped)`;
4. otherwise park (`message_readers` / `datagram_readers`) → `Poll::Pending`.

**Parking is never permitted on a dead connection** (ruling 128). Both verbs are
**cancel-safe**: a dropped future has claimed nothing, because the claim and the
return are the same expression.

**`poll_send_message` — precedence, in order.**

1. `msg.len() > MESSAGE_RECV_MAX` → `Ready(Err(MessageError::TooLarge))`,
   **checked in the shell before the core call** (§9.8:3088 *"rejected at the
   handle"*; house precedent is ruling 110's `buf.is_empty()` check);
2. death latch → `Ready(Err(MessageError::ConnectionLost(l)))`;
3. `core.is_none()` → `debug_assert!(false)` + `ConnectionLost`;
4. the core call → `Ready(Ok(()))` on acceptance;
5. the core says *not now* → park → `Pending`. **Ruled by 150: "not now" is
   and which waker catches it.**

**`send_message`'s cancel-safety is load-bearing and the contract must state it
in one sentence.** Under Q1's recommended answer (whole-payload atomic
admission), a dropped future has sent **nothing**: no stream opened, no byte
admitted. Under the obvious alternative (the shell decomposing into
open → write-loop → finish), a dropped future leaves a **FIN-less half-written
uni stream on the wire**, which against a message-mode receiver is exactly
§9.8's overflow case — the application's own `select!` timeout would manufacture
the failure S30 exists to diagnose. Say which, or a blind author builds the
second because it is the natural decomposition.

**New waker slots on `ConnCell` (`src/shell/shared.rs`) — the complete list.**
Slice 5b's contract was short by one here; this list is stated as **exhaustive**
and every row names every wake source:

| field | type | woken by |
|---|---|---|
| `message_readers` | `Wakers` | `ConnEvent::MessageReadable` (new `publish` arm); **the death latch** via `take_all_stream_wakers` (`shared.rs:318-341`) |
| `datagram_readers` | `Wakers` | `ConnEvent::DatagramReadable` (new `publish` arm); **the death latch** via `take_all_stream_wakers` |
| `message_senders` | `Wakers` | `ConnEvent::StreamsAvailable { dir: Dir::Uni }` (extend the existing arm at `driver.rs:540-542`); **the death latch**; **and ruling 150's new `pub(crate)` `ConnEvent` for the credit wake** |

All three **must** be added to `ConnCell::new` (`shared.rs:267-284`) **and** to
`take_all_stream_wakers` (`shared.rs:318-341`). Omitting the sweep is ruling
147's defect verbatim — *"a verb parked there when the connection dies is woken
by nothing"*.

**`wake_settled` (`shared.rs:390-425`) — does slice 6 add a source?** Check it,
do not assume it. A **message stream's** bytes are in `Connection::acked()`'s
snapshot (§16.2:4441-4447, *"including the message streams §9.8 never surfaces a
handle for"*), and the core already emits `ConnEvent::StreamFinished { r }` for
any fully-acknowledged send half — including a handle-less one.
`driver.rs:550-560` calls `note_send_finished(r)`, which inserts into
`finished_senders` **and** sweeps `settled_wakers`. So `Connection::acked()` over
messages should work with **no new wake source** — *provided* `StreamFinished` is
in fact emitted for handle-less streams. But `finished_senders` is bounded by the
rule *"written only while `blocked_ackers` holds that stream's slot"*
(`shared.rs:237`), and a handle-less message stream has **no** `blocked_ackers`
entry and no `Drop for SendStream` to remove it. **Ruled: ruling 160** — the map must
not grow for handle-less streams. Either exclude them or remove the entry when
the message's send half retires; say which in the implementation report.

### 2.6 Errors and constants — **already landed; `src/error.rs` is on nobody's path**

Verified at `d64b290`, so no agent opens either file for these:

- `src/error.rs:264` `pub enum MessageError { TooLarge, ConnectionLost(..) }`,
  `:275` `pub enum DatagramError { TooLarge, ConnectionLost(..) }` — both already
  exist, both **exhaustive** (ruling 61: only `WriteError` is
  `#[non_exhaustive]`), already `Clone + Send + Sync`, already pinned by
  `error.rs:357,381,422-425,462-483` and `tests/spec_errors.rs`.
- `src/constants.rs`: `MESSAGE_RECV_MAX = 262_144` (`:297`),
  `MAX_DATAGRAM_PAYLOAD = 1169` (`:315`), `DATAGRAM_SEND_QUEUE = 64` (`:318`),
  `DATAGRAM_RECV_QUEUE = 64` (`:321`), `MESSAGE_OVERFLOW = 0x06` (`:497`), plus
  the const-asserts at `:566`, `:571-572`, `:581`, `:585`.

**Consequence, and it is the constraint Q1 runs into:** `MessageError` is closed,
already shipped, and already pinned by a passing test. Adding a variant to it is
a breaking change *and* reds `spec_errors`. **Q1's "not now" must not be a
`MessageError`.** This mirrors slice 4b's correction 4 — *"`src/error.rs` really
is on nobody's path"* (`PLAN-4b.md:331-336`).

### 2.7 What slice 6 must NOT build

- **No `SentFrame::Datagram` and no datagram identity** — §8.7's `never` class;
  `CONTRACT-5b.md:116-129`, verified true at C6. A datagram contributes only the
  packet's `size`.
- **No receiver oversize rule** — §11.4, unrepresentable by construction.
- **No `Notification`, no `notified()`** — slice 7 (`PLAN.md:294`).
- **No `messages`/`datagrams`/`incoming_*` `Stream` or `Sink` faces, no `codec`,
  no `tower`** — §16.11's adapters are **slice 8's** (`PLAN.md:295`), and
  ruling 58's no-prefetch pin is slice 8's test.
- **No new trace target.** §18.2 is a closed list of five; everything slice 6
  traces goes to `slither::frames`.
- **No public accessor for the drop counters.** §16.2's accessor list is
  exhaustive (working rule 8) and §11.5 asks only for the **trace**.
- **No `STOP_SENDING`** (§9.9, deferred).
- **No re-deciding `is_ack_eliciting` or `retransmission` for `0x30`/`0x31`** —
  slice 3a answered and slice 3a's tests pin it.
- **No `#[non_exhaustive]`** on either new error enum.

---

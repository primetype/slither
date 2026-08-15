# Slice 4a — the API contract

**Both 4a agents compile against this file and neither may change it.**
It exists because the implementer and the test author are blind to each
other's code: this is the only thing that makes an independently written
test compile against an independently written implementation. In the
routing track that produced 35 tests compiling with **zero** errors
against code neither author had seen.

If you believe something here is wrong, **say so in your report and
implement it as written anyway** — a contract that one agent silently
improves is a contract that no longer binds. (Working rule 5 is about
briefs that look wrong; this is the same rule applied to the seam.)

Rulings 93–107 are in `.spec-v2-clean-slate/rulings.md` (Round 17). Read
that round — it is the decision record for every ambiguity the planner
found, and it overturns two of the plan's own provisionals.

---

## 1. Types

### `src/core/connection/stream_id.rs` — new, owned by 4a-impl

```rust
/// §9.1's wire stream identifier. Public (ruling 101).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct StreamId(u64);

impl StreamId {
    pub fn index(self) -> u64;                              // id >> 2
    pub fn dir(self) -> Dir;
    pub fn initiated_by_connection_initiator(self) -> bool;
    pub fn as_u64(self) -> u64;
    pub(crate) fn from_u64(v: u64) -> Self;                 // total: 62-bit varint ⇒ 60-bit index
    pub(crate) fn new(index: u64, dir: Dir, opener: Opener) -> Self;
}

impl core::fmt::Display for StreamId { /* the u64, decimal */ }

/// §9.1's direction. Public (ruling 101).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dir { Bi, Uni }

/// Which end opened it. Crate-internal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Opener { Initiator, Responder }

/// One of §9.1's four spaces. Crate-internal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Space { pub dir: Dir, pub opener: Opener }
```

**No public constructor on `StreamId`.** An application must not be able
to mint an id for a stream it does not own (ruling 101).

`StreamId` and `Dir` are re-exported from `src/lib.rs` on the existing
`pub use crate::core::{ConnectionId, IntroId, Timestamp};` line, whose
comment at `lib.rs:161–166` already states why: a public signature naming
an unreachable type is a rustdoc break, not merely a lint.

### The opaque handle key — ruling 95

```rust
/// The core's stream handle key: stable across install, unlike a wire
/// `StreamId`, which §16.9 cannot assign until establishment.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct StreamRef(/* private */);
```

`StreamRef` is **stable across install**: a handle opened before
establishment keeps the same `StreamRef` afterwards. The wire id is read
through `stream_id()`, which returns `None` until established.

**The trap ruling 95 closes:** a core that returns an internal index
*typed as* `StreamId` and remaps at install leaves every live handle
holding a stale key. That build passes a pre-establishment test and a
post-establishment test and fails only "open early, write late".

### `StreamsExhausted` — crate-internal (ruling 101)

```rust
/// §16.4's `open()` error. `pub(crate)`: §18.1's taxonomy is closed
/// (ruling 61) and §16.2 specifies the shell **parks** instead of
/// surfacing it, so no public verb can return it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StreamsExhausted;
```

### `Role` — already landed, do not redefine

`src/core/mod.rs` carries `pub enum Role { Initiator, Responder }` and
`Install { session, role }` as of commit `fdf5972` (ruling 106).
`Connection` holds `role: Option<Role>`, set at install. **4a-impl adds
the accessor it needs**; the field exists.

---

## 2. `core::Connection`'s stream surface

```rust
impl<C: Handshake> Connection<C> {
    pub(crate) fn open(&mut self, dir: Dir) -> Result<StreamRef, StreamsExhausted>;
    pub(crate) fn write(&mut self, now: Instant, r: StreamRef, data: &[u8])
        -> Result<usize, WriteError>;
    pub(crate) fn finish(&mut self, now: Instant, r: StreamRef) -> Result<(), WriteError>;
    pub(crate) fn reset(&mut self, now: Instant, r: StreamRef, error_code: u64);
    pub(crate) fn read(&mut self, now: Instant, r: StreamRef, buf: &mut [u8])
        -> Result<Option<usize>, ReadError>;
    /// **Ruling 109.** §16.2's `RecvStream::drop` calls this; ruling 93
    /// and its amendment are its whole semantics.
    pub(crate) fn abandon_recv(&mut self, now: Instant, r: StreamRef);
    pub(crate) fn accept(&mut self, dir: Dir) -> Option<StreamRef>;   // FIFO, ruling 112
    pub(crate) fn stream_id(&self, r: StreamRef) -> Option<StreamId>;

    /// **Ruling 113.** Uncalled from the wire until slice 5 wires §12 to
    /// them; `pub(crate)` on `Connection` so 4a's tests can reach the GC
    /// and watermark logic at all.
    pub(crate) fn on_ack_range(&mut self, now: Instant, r: StreamRef, range: Range<u64>);
    pub(crate) fn on_lost_range(&mut self, now: Instant, r: StreamRef, range: Range<u64>);

    /// **Ruling 94.** Total bytes of reassembly **capacity** currently
    /// allocated across every receive half. Test-visible on purpose:
    /// §10.6's memory bound is otherwise an untestable MUST, which is
    /// what it became last time.
    pub(crate) fn reassembly_capacity(&self) -> u64;

    /// The role fixed at install (ruling 106). `None` before it.
    pub(crate) fn role(&self) -> Option<Role>;
}
```

**Return-value conventions — pinned here because both agents need the
same ones and §16.2 specifies only the `async` surface:**

| Call | Value | Means |
|---|---|---|
| `write` | `Ok(n)`, `n > 0` | `n` bytes buffered |
| `write` | `Ok(0)` | **blocked** by stream or connection credit; the shell parks — **but only for a non-empty input** (ruling 110: a zero-length write is a no-op that also returns `Ok(0)`, and parking on it waits for credit that would not help) |
| `write` | `Err(WriteError::Finished)` | `finish()`/`reset()` already called |
| `write` | `Err(WriteError::ConnectionLost(_))` | connection is gone |
| `read` | `Ok(Some(n))`, `n > 0` | `n` bytes drained from the contiguous prefix |
| `read` | `Ok(Some(0))` | **no data available**; the shell parks |
| `read` | `Ok(None)` | end of stream: final size reached **and** drained |
| `read` | `Err(ReadError::Reset(code))` | peer reset it (§9.6) |
| `accept` | `None` | nothing claimable in that direction |

`Ok(Some(0))` versus `Ok(None)` is the distinction the shell turns into
"park" versus "EOF". Getting it backwards hangs a reader forever on a
finished stream, which is why it is written down rather than inferred.

**Rulings 108–113 amend this section after dispatch.** `read`, `finish`
and `abandon_recv` carry `now` because §10.3 makes consumption advance
credit and §16.7 makes sealing synchronous *inside the mutating call* —
`poll_output()` has no instant and ruling 80 forbids the core inventing
one. `final_size` on `reset()` is the end offset of the highest byte
**actually transmitted** (ruling 111), not the accepted-but-unsealed
total.

**`open()` before establishment is legal** (§16.9) and returns a usable
`StreamRef`. `write()` on it is legal. `stream_id()` on it is `None`.

---

## 3. `ConnEvent` — the six new variants

```rust
pub(crate) enum ConnEvent {
    Established,                                  // exists
    StreamOpened { dir: Dir },                    // new — ONE PER STREAM, ruling 99
    StreamsAvailable { dir: Dir },                // new
    StreamReadable { r: StreamRef },              // new
    StreamWritable { r: StreamRef },              // new
    StreamFinished { r: StreamRef },              // new — never fires in slice 4 (no ACKs)
    StreamReset { r: StreamRef, error_code: u64 },// new
    Closed(ConnectionLost),                       // exists
}
```

Keyed by `StreamRef`, not `StreamId` — ruling 95's amendment. If the
events keyed by wire id, the shell could not match a wakeup to its waker
before establishment, which is exactly when §16.9 says work is in flight.

`ConnEvent` is not `Copy` (it already is not — `Closed` carries a
`Vec<u8>` reason).

---

## 4. The rulings that change behaviour, in one list

Full reasoning in Round 17. **These are decided; do not re-litigate them,
and do report if you find one unimplementable.**

| # | Rule |
|---|---|
| **93** | Dropping a receive half **retires it at once** — freed, tombstoned, connection-credit trued up in the same step. The true-up value is the **highest stream-level limit ever advertised** for that half, *not* the highest received offset. **Amended (see below): two tombstone mechanisms, not one.** |
| **94** | Reassembly: coalesce-on-insert, `REASSEMBLY_CHUNKS_MAX` = 1024, **allocate lazily**. Total capacity across all streams stays inside the connection window. |
| **95** | `StreamRef` keys eleven sites: the five verbs, `accept`, `stream_id`, and the four stream-naming `ConnEvent`s. |
| **97** | Receive-path order: **legality → watermark → limit → final size → flow control.** The limit check runs *before* the implicit opens it would authorise. |
| **98** | Seal paths, from §7.4 (`SPEC.md:1953–1958`), **not** from the plan's table, which is wrong: STREAM **first transmission** → `seal` (marking); STREAM **retransmission** → `seal_quiet`; **RESET_STREAM → `seal_quiet`**; all four credit frames → `seal_quiet`. All ack-eliciting. Marking belongs to the *seal*, so a packet mixing a fresh STREAM frame with credit frames is marking. |
| **99** | **One `StreamOpened` per newly-opened stream.** An implicit open of index 5 emits six. A frame above the cumulative limit emits **zero** and kills the connection. |
| **100** | An empty, FIN-less STREAM frame **opens** its stream. Its "no-op" is about the data, not the open. |
| **101** | `StreamId` and `Dir` public; `StreamsExhausted` `pub(crate)`. |
| **102** | §10.4's both triggers use `STREAMS_CREDIT_BATCH`. No second literal `8`. |
| **103** | `STREAMS_CREDIT_BATCH` is invisible receiver policy; `REASSEMBLY_CHUNKS_MAX` is observable receiver policy; the four windows are wire constants. Mark which is which in `src/constants.rs`. |
| **104** | §10 has **three** violations. `REASSEMBLY_CHUNKS_MAX` overflow ⇒ `PROTOCOL_VIOLATION` is the third. |
| **105** | The tombstone obligation is discharged by an **injected duplicate**; the loss-driven variant is owed to slice 7. |
| **106** | `Install` carries the role. **Stream-ID parity comes from `Connection::role()`, never from "I was created by `connect()`."** |

---

### 4a. Ruling 93's amendment — abandonment needs **two** tombstone mechanisms

Added after dispatch. Full text in Round 17 under ruling 93, and in
`SPEC.md` §16.2.

| Space | On abandoning the receive half | Later STREAM frames for it |
|---|---|---|
| **peer-opened uni** | it is the only half this endpoint holds ⇒ the stream is **fully closed** (§9.7): the watermark advances and the peer earns a MAX_STREAMS grant (§10.4) | inert by §9.2's **watermark** rule |
| **bidi** (either opener) | our **send half is still live** ⇒ **not** fully closed: the watermark does **not** advance and the index stays in the open set | **not** watermark no-ops and **not** implicit opens — discarded by §16.2's own "arrivals for it are discarded" |

In both cases the frames are ACKed, delivered nowhere, and consume no
further credit, because §10.3's true-up is absolute and that stream's
contribution already sits at its maximum. **The stream-level
`FLOW_CONTROL_ERROR` check still runs** against the frozen advertised
limit — that is what stops an abandoned half becoming an unbounded sink.

**Watermark alone resurrects an abandoned bidi receive half on the next
frame**, re-charging the cumulative limit against freed state.
**Per-half tombstone alone never advances the uni watermark**, so the
peer never gets its MAX_STREAMS credit back. Both are needed.

## 5. What slice 4a must NOT build

`.slices/04-streams/PLAN.md` §12 is the full list with the seam each
exclusion must leave. In brief: **no** §9.8 messages, **no** STOP_SENDING
(`0x05` stays a structural unknown-type error), **no** §11 datagrams,
**no** §12 ACK generation or application, **no** §13 loss recovery.

**The consequence to plan around, not discover:** with no ACK processing,
a send half can never reach `DataRecvd`. So `ConnEvent::StreamFinished`
**never fires in slice 4**, the watermark advances only on the receive
side, and MAX_STREAMS replenishment is reachable for peer-opened **uni**
streams and not for peer-opened **bidi** ones. That is the slice
boundary, not a defect.

`on_ack_range(r, range)` and `on_lost_range(r, range)` are defined on the
send half **in slice 4** and left uncalled from the wire; slice 5 wires
§12 to them. This is what makes GC and watermark logic testable now. The
un-ACKed retention set therefore never drains in slice 4 — that is
expected, and **freeing on send instead is a collapsed implementation
that would make slice 5's tests pass for free**.

---

## 6. Test fixture — there is no two-core fixture yet

`src/core/connection/tests.rs` has a **one-core + hand-rolled `Peer`**
harness (`handshake()` at line 345 returns `(EstablishedSession, Peer)`).
Its helpers are private to that module and **that file is on nobody's
path during 4a**.

A two-core harness is ~20 lines on top of a copy of `handshake()`'s body:
both halves of the hiss split are already produced there, so build two
`EstablishedSession`s and install each into its own
`Connection::connecting(...)` via `handle_endpoint_event`. Give side A
`Role::Initiator` and side B `Role::Responder` — which also makes §9.1's
parity directly assertable at core level.

Whoever needs it writes it **inside their own file**.

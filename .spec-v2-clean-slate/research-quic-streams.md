# Research: QUIC's stream and flow-control model, distilled for slither

Sources: RFC 9000 (QUIC transport, esp. §§2–4, 16, 19), RFC 9002 (loss detection —
confirmed orthogonal to flow control), RFC 9221 (QUIC DATAGRAM extension); local
`quinn-proto-0.11.16` source at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/`
as a real Rust implementation reference; slither's own `SPEC.md` (§9, Leg 2) and
`TODO.md` as the point of comparison.

---

## 1. Stream model

### Stream ID encoding (RFC 9000 §2.1)

A stream ID is a **62-bit varint**. The two least-significant bits are a type tag,
not part of an ordinal counter:

| Bit | Meaning |
|---|---|
| `0x01` | initiator: `0` = client-initiated, `1` = server-initiated |
| `0x02` | direction: `0` = bidirectional, `1` = unidirectional |

The remaining 60 bits are `index` — a per-(initiator, direction) monotonically
allocated counter starting at 0. This yields exactly **four independent stream-ID
spaces**: client-bidi, server-bidi, client-uni, server-uni. Each space has its
own next-ID counter and its own concurrency limit (see §3 below).

quinn-proto mirrors this precisely
(`~/.cargo/…/quinn-proto-0.11.16/src/lib.rs:255-276`):

```rust
pub fn new(initiator: Side, dir: Dir, index: u64) -> Self {
    Self((index << 2) | ((dir as u64) << 1) | initiator as u64)
}
pub fn initiator(self) -> Side { if self.0 & 0x1 == 0 { Client } else { Server } }
pub fn dir(self) -> Dir { if self.0 & 0x2 == 0 { Bi } else { Uni } }
pub fn index(self) -> u64 { self.0 >> 2 }
```

`StreamId` is-a `VarInt` on the wire (`From<StreamId> for VarInt`) — it costs 1–4
bytes for any realistic stream count (index < 2^28 fits in 4 bytes; the first
2^14 streams of a type fit in 2 bytes, §4 below).

### Implicit opening (RFC 9000 §2.1)

> "A stream ID that is used out of order results in all streams of that type
> with lower-numbered stream IDs also being opened."

I.e. sending/receiving on stream ID `N` of a given (initiator, dir) implicitly
opens IDs `0..N` of that same space too (subject to the concurrency limit,
§3 below — opening past the limit is a protocol error). There is no explicit
OPEN_STREAM frame; the first STREAM/RESET_STREAM/STREAM_DATA_BLOCKED frame *is*
the open. quinn-proto tracks this via `next_remote`/`opened` fields in
`StreamsState` (`connection/streams/state.rs:90-96`) rather than per-open
messages.

### Stream lifecycle — two independent half-close state machines

Each stream has a **send half** and a **recv half**, each with its own state
machine (RFC 9000 §3, Figures 2 and 3). A unidirectional stream only has the
half that matches its creator's role; a bidirectional stream has both, tracked
independently — closing one direction doesn't touch the other.

**Send-stream states (§3.1, Figure 2):**

```
   o
   | Create Stream (Sending) / Peer Creates Bidirectional Stream
   v
+-------+  Send RESET_STREAM
| Ready |------------------------.
+-------+                        |
   | Send STREAM / STREAM_DATA_BLOCKED
   v                             |
+-------+  Send RESET_STREAM     |
| Send  |----------------------->|
+-------+                        |
   | Send STREAM + FIN           |
   v                             v
+-------+  Send RESET_STREAM  +-------+
| Data  |-------------------->| Reset |
| Sent  |                     | Sent  |
+-------+                     +-------+
   | Recv All ACKs               | Recv ACK
   v                             v
+-------+                     +-------+
| Data  |                     | Reset |
| Recvd |                     | Recvd |
+-------+                     +-------+
```

`Ready` → `Send` on the first byte written; → `DataSent` on sending FIN;
→ `DataRecvd` once every byte (including FIN) is ACKed. `ResetStream` can fire
from `Ready`/`Send`/`DataSent`, jumping to `ResetSent` → `ResetRecvd` once the
RESET itself is ACKed. `DataRecvd`/`ResetRecvd` are terminal — the sender can
free the stream's send-side state.

quinn-proto's actual internal enum collapses this to three cases because ACK
bookkeeping is tracked elsewhere (`connection/streams/send.rs:280-288`):

```rust
pub(super) enum SendState {
    Ready,
    DataSent { finish_acked: bool },
    ResetSent,
}
```
— `DataRecvd`/`ResetRecvd` are represented as `DataSent{finish_acked:true}` /
stream removal, not extra enum variants. **Lesson for slither: the RFC's 6-state
diagram is a spec fiction for exposition; a real implementation needs far fewer
states plus a couple of booleans/counters.**

**Receive-stream states (§3.2, Figure 3):**

```
   o
   | Recv STREAM / STREAM_DATA_BLOCKED / RESET_STREAM
   | Create Bidi Stream (Sending) / Recv MAX_STREAM_DATA / STOP_SENDING (Bidi)
   | Create Higher-Numbered Stream
   v
+-------+  Recv RESET_STREAM
| Recv  |------------------------.
+-------+                        |
   | Recv STREAM + FIN           |
   v                             |
+-------+  Recv RESET_STREAM     |
| Size  |------------------------>|
| Known |                        |
+-------+                        |
   | Recv All Data                |
   v                              v
+-------+ Recv RESET_STREAM  +-------+
| Data  |---(optional)------>| Reset |
| Recvd |<--(optional)-------| Recvd |
+-------+   Recv All Data    +-------+
   | App Read All Data            | App Read Reset
   v                              v
+-------+                     +-------+
| Data  |                     | Reset |
| Read  |                     | Read  |
+-------+                     +-------+
```

`Recv` accepts out-of-order STREAM frames (buffered — see §5); `SizeKnown` once
a FIN pins the final length; `DataRecvd` once every byte up to that length has
arrived (still buffered, not yet delivered to the app); `DataRead` once the
application has drained it all. quinn-proto again collapses this
(`connection/streams/recv.rs:432-436`):

```rust
enum RecvState {
    Recv { size: Option<u64> },
    ResetRecvd { size: u64, error_code: VarInt },
}
```
— `DataRecvd`/`DataRead` fold into "is the assembler's contiguous prefix equal
to `size`, and has the app drained it" rather than distinct states.

### What a minimal transport needs vs. what QUIC does for HTTP/3's sake

QUIC's 4-way stream-ID space split (client/server × bidi/uni) and the whole
concurrency-limit apparatus (§3, MAX_STREAMS/STREAMS_BLOCKED) exist mainly
because **HTTP/3 opens large numbers of short-lived, peer-creatable streams**
(one per request/response, plus control/QPACK unidirectional streams) and needs
a way to bound how many the peer may open unilaterally, plus cheap client/server
disambiguation without a handshake round-trip. A small Noise-based transport
between two already-mutually-authenticated peers (slither's IK model — no
anonymous peer, no "am I client or server for this stream" ambiguity beyond
who's the connection initiator) needs much less:

- It probably does **not** need four ID spaces. A single flat, connection-wide
  stream-ID space (with, at most, an initiator-parity bit if both sides may
  open streams unprompted) is sufficient — slither has exactly one peer per
  connection and no third-party multiplexed protocol running over it.
- It likely still wants the **bidi/uni distinction** (a control channel vs. a
  bulk one-way transfer are different shapes) but doesn't need it baked into
  the ID's low bits if streams are rare enough that a small header field or
  even a separate "open" signal is cheap.
- **MAX_STREAMS concurrency control matters less** at small peer counts and
  disappears entirely if slither caps streams at some small fixed number (e.g.
  "N concurrent reliable streams") rather than modelling unbounded
  HTTP/3-style stream churn.

## 2. Frame taxonomy

All fields below are RFC 9000 §16 varints unless noted. Frame type byte is
itself always a varint too (single byte for all types below except DATAGRAM's
`0x30/0x31`, still one byte).

| Frame | Type | Fields | Purpose | v1 for slither? |
|---|---|---|---|---|
| **STREAM** | `0x08-0x0f` (§19.8) | `Stream ID`, `[Offset]` if OFF, `[Length]` if LEN, `Stream Data` | Carries a byte range of a stream; FIN/LEN/OFF are bits 0/1/2 of the type byte itself (not separate fields) — `0x08 \| FIN\|LEN\|OFF` | **Yes — this is the whole point of the redesign** |
| **RESET_STREAM** | `0x04` (§19.4) | `Stream ID`, `Error Code`, `Final Size` | Abrupt sender-side stream abort; final size lets the receiver true-up flow-control accounting even though the tail bytes never arrive | Yes, if streams exist at all — otherwise a stream abort has no clean signal |
| **STOP_SENDING** | `0x05` (§19.5) | `Stream ID`, `Error Code` | Ask the peer to RESET_STREAM a stream you don't want to keep receiving (doesn't itself close anything) | Nice-to-have; can defer — an application can just ignore/drain, or slither could fold this into an app-level "cancel" message initially and add the wire frame if it earns its keep |
| **MAX_DATA** | `0x10` (§19.9) | `Maximum Data` (absolute) | Connection-level receive-credit grant | Yes, if connection-level flow control is in v1 (recommended — see §3) |
| **MAX_STREAM_DATA** | `0x11` (§19.10) | `Stream ID`, `Maximum Stream Data` (absolute) | Per-stream receive-credit grant | Yes — needed the moment streams exist and can outrun the receiver's buffer |
| **MAX_STREAMS** | `0x12`/`0x13` (§19.11, bidi/uni) | `Max Streams` (absolute count) | Raise the peer's stream-open concurrency ceiling | Only if slither adopts a hard concurrent-stream cap that needs runtime adjustment; a fixed compile-time cap (simpler) can skip the frame and just refuse opens past it silently/with a local error, no wire signal needed |
| **DATA_BLOCKED** | `0x14` (§19.12) | `Maximum Data` (the limit hit) | Diagnostic: "I'd send more but connection flow control stopped me" | Optional — pure diagnostics; a minimal design can omit it and simply resume sending once credit arrives (silence carries the same information with less code, at the cost of debuggability) |
| **STREAM_DATA_BLOCKED** | `0x15` (§19.13) | `Stream ID`, `Maximum Stream Data` | Same, per-stream | Optional, same reasoning |
| **STREAMS_BLOCKED** | `0x16`/`0x17` (§19.14) | `Max Streams` | Same, for concurrency limits | Optional/skip if MAX_STREAMS itself is skipped |
| **ACK** | `0x02`/`0x03` (§19.3) | `Largest Acked`, `ACK Delay`, `Range Count`, `First Range`, `[Gap, Range]…`, `[ECN counts]` | Selective ack of packet numbers | **Slither already has this** — Leg 2 `FRAME_ACK` (SPEC §9.2) is a fixed-width variant of the same idea; keep it, optionally switch to varints |
| **PING** | `0x01` (§19.2) | none | Elicit an ACK / keep-alive | Slither already has `FRAME_PING` (SPEC §9.1) — keep |
| **CONNECTION_CLOSE** | `0x1c`/`0x1d` (§19.19) | `Error Code`, `[Frame Type]` (transport variant only), `Reason Length`, `Reason` | Graceful/explicit teardown with a reason | Slither has **no explicit close today** (SPEC §2: `0x04` "close" is reserved, never emitted; teardown is timer-driven only). Worth adding in v1 — an explicit close frame is cheap and avoids waiting out `DEAD_TIMEOUT` on a deliberate disconnect |
| **DATAGRAM** (RFC 9221) | `0x30`/`0x31` | `[Length]` if LEN bit set, `Data` | Unreliable, unordered, flow-control-exempt payload multiplexed with reliable frames in the same encrypted packet | **Maps directly onto slither's existing Leg 1 unreliable-datagram notion** (see below) |

### Mapping slither's existing unreliable datagram onto RFC 9221

Slither already has an "honest unreliable datagram" concept — Leg 1's
`Connection::seal`/`open` (`src/session.rs:207`, wrapping `hiss`'s
`DatagramSend`/`DatagramRecv`) is *itself* an unreliable, unordered,
per-packet payload, and `TODO.md` §2 explicitly names it: *"Base `Connection`
exposes Leg 1's honest unreliable-datagram semantics."* Today that notion is
**the entire packet payload** — there's no frame-level multiplexing at Leg 1;
Leg 2's reliable DATA frames live one layer up, inside the sealed plaintext.

RFC 9221's DATAGRAM frame generalizes this: rather than "this whole packet is
either raw datagram bytes (Leg 1) or a concatenation of reliable frames
(Leg 2)," a QUIC-shaped design puts a `FRAME_DATAGRAM` type **inside** the same
frame stream as STREAM/ACK/PING, so one encrypted packet can coalesce an ACK, a
STREAM-frame fragment, *and* an unreliable datagram together. For slither this
means: **fold the unreliable-datagram notion into the unified frame layer as a
`DATAGRAM` frame type**, dropping the Leg 1/Leg 2 split in favour of "every
sealed packet's plaintext is a frame stream; DATAGRAM is just one more frame
type, alongside STREAM, ACK, PING, RESET_STREAM, etc." That's a strict
simplification over today's two-tier design and is exactly RFC 9221 §5.2–5.3's
model: no retransmission, no ordering, and (per RFC 9221 §5.3, quoted above)
**no contribution to connection- or stream-level flow control** — datagrams
should not need MAX_DATA credit, matching slither's current unrestricted-size
(up to `MAX_PLAINTEXT`) fire-and-forget semantics.

## 3. Flow control

RFC 9000 §4 is short and the mechanism is simple, symmetric at both levels:

### Two levels

1. **Stream-level** (§4.1): each stream has a receive limit, an **absolute
   byte offset**, not a window size — `MAX_STREAM_DATA` says "you may send up
   to offset X on this stream," not "you may send X more bytes." The receiver
   advertises it per-stream; initial values come from transport parameters
   (`initial_max_stream_data_bidi_local`, `…_bidi_remote`, `…_uni` — asymmetric
   by design, since a locally-opened bidi stream's receive limit and a
   remotely-opened bidi stream's receive limit can differ).
2. **Connection-level** (§4.1): `MAX_DATA` bounds the **sum of bytes received
   across all streams** (not including retransmissions of the same bytes, and
   — per RFC 9221 — not including DATAGRAM frames at all). Initial value:
   `initial_max_data`. A sender must respect **both** limits — whichever is
   tighter binds.

quinn-proto's `StreamsState` tracks both symmetrically
(`connection/streams/state.rs:110-131`): `max_data` (peer's grant to us,
outgoing budget), `local_max_data`/`sent_max_data` (our grant to the peer,
incoming budget, lazily re-sent), plus per-stream equivalents inside each
`Send`/`Recv`. Note the receive side keeps *two* numbers — `local_max_data`
(what we've decided the limit now is) and `sent_max_data` (what we last told
the peer) — because re-announcing an unchanged limit is wasted bytes; a new
MAX_DATA frame is only queued once the gap between "already told" and "current
decision" crosses a threshold (§4.2).

### How limits advance — consumption drives credit

The receiver's **application-level consumption**, not merely arrival, is what
should drive new credit in a well-behaved implementation (§4.1: *"the receiver
identifies when it is ready to advertise a larger limit"*). §4.2 leaves the
exact policy to the implementation but is explicit that the receiver should
autotune based on the observed rate of consumption vs. RTT (the same shape as
TCP receive-window autotuning) rather than issuing a fixed increment forever —
issuing credit too eagerly wastes memory commitments, issuing it too late
stalls the sender once an RTT of data is in flight. **A minimal implementation
can start simpler**: re-grant credit once the used fraction of the current
window crosses some ratio (e.g. half-consumed), sized to at least a
bandwidth-delay product's worth of buffer, and skip full autotuning for v1 —
this is exactly quinn's default: `stream_receive_window` config value used to
seed `initial_max_stream_data_*`, static per connection unless the app calls
`Connection::set_receive_window` at runtime.

### Blocked signals

`DATA_BLOCKED`/`STREAM_DATA_BLOCKED`/`STREAMS_BLOCKED` (§4.1, §4.6, §19.12–14)
exist purely as a **diagnostic/liveness aid**: a sender that has been
window-limited for a while sends one so the receiver's monitoring can tell
"peer is stalled because I haven't granted credit" apart from "peer has
nothing to send" or "peer is congestion-limited." They carry no semantics the
receiver must act on beyond optionally hurrying up its next MAX_DATA/
MAX_STREAM_DATA. **Genuinely optional for a minimal transport** — the same
information is recoverable from timing/telemetry, and omitting the frames
saves both wire bytes and state-machine cases (in quinn-proto, "blocked" state
tracking — `connection_blocked: Vec<StreamId>` in `state.rs:110` — exists
purely to remember which streams to unblock/notify once credit returns; a
polling or event-driven local API doesn't need the wire signal to work, only
the local bookkeeping).

### Interaction with reliable delivery and buffering

Flow control and reliable delivery are cleanly separable concerns but share
one number: a receiver can only safely advertise `MAX_STREAM_DATA`/`MAX_DATA`
increases for bytes it is willing to **buffer until read**, because reliable
delivery means bytes already inside the advertised window must be held (and
gap-filled if reordered) even if the application hasn't drained them yet.
Concretely: the credit ceiling **is** the buffer-size commitment. This is why
initial limits are transport parameters exchanged at connection setup
(equivalent, in slither's world, to a value baked into the handshake or a
fixed protocol constant) — the sender must never be told to trust more credit
than the receiver has memory backing it.

### Minimal viable flow-control design for slither

- Keep **both levels** (stream + connection) — dropping connection-level FC
  while keeping per-stream FC lets a peer open many streams and aggregate
  past any sane buffer budget; dropping per-stream FC while keeping
  connection-level FC lets one greedy stream starve others' credit.
  Both are cheap (one MAX_DATA/MAX_STREAM_DATA frame each, occasionally).
- Use **absolute offsets**, not deltas — simpler idempotency (duplicate/
  reordered credit frames are naturally monotonic-max, no double-counting
  risk) and matches the STREAM frame's own offset field for a consistent
  mental model.
- Skip `DATA_BLOCKED`/`STREAM_DATA_BLOCKED`/`STREAMS_BLOCKED` in v1; add only
  if operational experience shows the diagnostics are needed.
- Skip `MAX_STREAMS`/`STREAMS_BLOCKED` if slither adopts a small fixed
  concurrent-stream cap (simpler); add them only if the cap needs to be
  dynamic/negotiated.
- Fixed, generous initial windows (protocol constants, no transport-parameter
  negotiation needed given slither has no version/parameter negotiation at
  all) are enough for v1; autotuning is a later refinement.

## 4. Varint encoding (RFC 9000 §16)

The scheme (confirmed against `quinn-proto`'s `varint.rs`, byte-for-byte):

| Prefix (top 2 bits of first byte) | Total length | Usable value bits | Max value |
|---|---|---|---|
| `00` | 1 byte | 6 | 63 |
| `01` | 2 bytes | 14 | 16 383 |
| `10` | 4 bytes | 30 | 1 073 741 823 |
| `11` | 8 bytes | 62 | 4 611 686 018 427 387 903 (2⁶²−1) |

Decode: read the first byte, its top two bits select the total length, mask
them off, then big-endian-interpret the remaining bits across however many
bytes the prefix demands (`~/.cargo/…/quinn-proto-0.11.16/src/varint.rs:142-193`
— `decode`/`encode`, verified against the RFC text). All QUIC integer fields —
stream IDs, offsets, lengths, error codes, ACK ranges — are varints; there is
no fixed-width integer anywhere in a QUIC frame body except raw payload bytes
and specific fixed-size fields like connection IDs.

**Why it matters for compact framing:** small, common values (most stream
IDs, most per-frame lengths, most ACK ranges) cost one byte instead of four or
eight, while values that need the full range still fit. It also means
frame headers self-describe their own length without a side-channel, so a
parser never needs an out-of-band schema to know how many bytes a given field
consumes — it reads the first byte and knows.

**Recommendation: adopt QUIC's varint wholesale, verbatim.** Slither's current
Leg 2 layer uses fixed-width fields throughout (`seq(8)`, `length(2)`,
`largest(8)`, `gap(2)`, `length(2)` in AckRangePair — SPEC §9.2–9.3) — simple,
but wasteful for the common case (a `length(2)` field burns 2 bytes even for a
4-byte message; a `seq(8)` burns 8 bytes when the connection has sent 200
messages). Given the redesign is adding STREAM-frame offsets (potentially
64-bit-range) and stream IDs to the wire, a varint scheme earns its complexity
now — it's ~50 lines of code (see quinn-proto's `varint.rs` above, which
slither could copy near-verbatim) and every frame in the new design (STREAM
offset/length, stream ID, MAX_DATA/MAX_STREAM_DATA limits, ACK ranges) benefits
uniformly. There is no compatibility reason not to (backward compat is
explicitly dropped), so there's no cost to switching now versus retrofitting
later.

## 5. Ordering and fragmentation

### How STREAM frames give ordered delivery

A STREAM frame (§19.8) carries `(Stream ID, Offset, Length, Data)` — i.e. each
frame is **a labelled byte range** within a per-stream logical byte sequence,
not a self-contained message. The receiver's job (RFC 9000 doesn't mandate an
implementation but describes the effect in §2.2) is to buffer arriving ranges
and deliver the **contiguous prefix** to the application as it becomes
available — exactly what quinn-proto's `Assembler`
(`connection/streams/assembler.rs:11-25`) does: a `BinaryHeap<Buffer>` of
out-of-order chunks plus a `bytes_read` cursor marking how far the contiguous,
delivered prefix extends. Frames can arrive in any order (retransmission,
reordering, racing packets) — the offset field alone is what reconstructs
order, decoupled entirely from packet arrival order or packet numbers.

### Fragmentation falls out of streams for free

Because a stream is a byte sequence sliced into arbitrarily-sized STREAM
frames, a message larger than one packet's payload is **not a special case** —
it's just written as consecutive offset ranges, each carried in as many
packets as needed, reassembled by the same Offset-driven logic that handles
ordering. There is no separate "fragmentation protocol": splitting and
reassembly are one mechanism, and it has no message-size ceiling beyond the
stream's own byte-offset range (a 62-bit offset space, RFC 9000 §2.2's flow
control MUST still apply — the effective cap is whatever flow-control credit
and the application choose to grant, not a wire-format limit).

### Replacing slither's 1159-byte message cap

Slither's current `MAX_MESSAGE = 1159` (SPEC §9.6) exists precisely because
Leg 2's DATA frame has **no fragmentation**: "multi-packet messages
(fragmentation) are OUT, reserved for the STREAM work" (SPEC §9.3). Adopting
STREAM frames removes this cap by construction — a stream's total length is
unbounded by the wire format; any single-packet ceiling becomes purely a
per-*frame* payload limit (how much of a stream's byte range one STREAM frame
carries, bounded by `MAX_DATAGRAM` minus headers), not a per-*message* limit.
This is the direct mechanism by which "fragmentation falls out of streams" —
exactly the TODO.md §7 framing: *"Fragmentation (today's ~1.1 KB message cap)
falls out of streams."*

### Should slither keep an unordered-reliable-message notion alongside streams?

Two shapes are on the table:

1. **Keep Leg 2's shape as a "reliable datagram/message" type**, distinct from
   streams: at-least-once-on-wire, exactly-once-surfaced, unordered,
   independent messages, no head-of-line blocking across messages — coexisting
   with new ordered streams for bulk/large transfers.
2. **Fold everything into streams + (unreliable) datagrams**: a "reliable
   unordered message" becomes a short-lived stream that immediately sends the
   whole payload + FIN and is torn down after ACK — no separate wire concept.

Arguments for keeping a **distinct reliable-message primitive (option 1)**:
- It's a materially different delivery *contract*: no head-of-line blocking is
  a real, valuable property (SPEC §9.3 states this explicitly as the design
  intent) that a stream — even a short-lived one — doesn't give you if
  multiple concurrent "messages" share one stream (a lost early message stalls
  later ones on that stream); using one stream *per* message avoids HOL
  blocking but then needs a cheap way to open/tear down many short streams,
  which re-introduces most of the complexity streams were meant to amortize.
- Small control/signalling messages (the actual current use of `FRAME_DATA` —
  e.g. any app-level RPC-shaped traffic) map awkwardly onto "open a stream,
  send one frame, FIN, wait for ACK, garbage-collect the stream state" — that
  is strictly more state than "send a DATA frame with a seq number," for
  equivalent guarantees on a per-message basis.
- QUIC itself doesn't have this primitive because HTTP/3 never needed it — every
  reliable QUIC transfer in the deployed ecosystem is stream-shaped. Slither
  isn't HTTP/3; the existing Leg 2 use case (small reliable RPC-ish messages
  between a *known pair of trusted peers*, not many short-lived
  request/response exchanges across possibly-adversarial concurrent clients)
  is a better fit for a lightweight message primitive than for QUIC's
  stream-open machinery.

Arguments for **folding into streams + datagrams only (option 2)**:
- One reliable-delivery code path to build, test, and reason about
  (STREAM-frame retransmission + reassembly) instead of two (DATA-frame retransmission
  + STREAM-frame retransmission+reassembly) — meaningfully less surface area,
  which matters for a "small Noise-based transport" that wants to stay small.
  Given slither v0.1 already ships RFC-9002-shaped loss detection generically
  over "frames," extending that machinery to retransmit STREAM-frame ranges
  instead of whole DATA frames is not a large marginal step once it exists.
- A short-lived, low-numbered stream per logical message, opened implicitly
  by first use and closed on FIN+ACK, is close to as cheap as a DATA frame if
  the stream bookkeeping is lean (a `HashMap<StreamId, StreamState>` entry,
  freed on completion) — quinn-proto's own free-list reuse pattern
  (`StreamRecv::Free`/`Open`, `state.rs:23-65`) exists exactly to make
  stream-open/close cheap and GC-friendly for high-churn stream usage.

**Recommendation**: this is a real design fork, not a clear-cut call — flag it
explicitly for the SPEC discussion rather than pre-deciding. A pragmatic
middle path worth considering: keep the wire format unified (frames are
STREAM/RESET_STREAM/ACK/PING/DATAGRAM/…, no separate DATA-frame type), but
give the **API** a "send one reliable message, no explicit stream handle"
convenience method that internally allocates a short stream ID out of a
dedicated ID sub-range and manages its lifecycle transparently — this gets
option 2's implementation simplicity (one retransmission/reassembly path)
while approximating option 1's ergonomics for the common small-message case.
The HOL-blocking argument (multiple such implicit streams, not one shared
stream) still holds if each message gets its own stream ID.

## 6. What to borrow vs. drop for slither

Slither's constraints that materially change the calculus versus general
QUIC: **no version negotiation** (SPEC has none; "unknown version ⇒ silent
drop" is the whole story), **no connection migration beyond WireGuard-style
endpoint roaming** (SPEC §6 "Roaming" — single active path, moved wholesale on
a fresh authenticated packet, no multi-path, no explicit migration frames), **no
0-RTT/1-RTT packet-type zoo** (Noise IK is a fixed 2-message handshake — msg1/
msg2/data are the only three "packet types" that will ever exist, versus
QUIC's Initial/0-RTT/Handshake/1-RTT/Retry space built for TLS 1.3's
negotiation surface), and **one path, one connection ID equivalent** (the
`sender_index`/`receiver_index` u32 pair already serves CID's demux role,
without CID rotation/privacy machinery — WireGuard-style indices are not
meant to be unlinkable across rotations the way QUIC CIDs are for multi-path
privacy).

### Adopt wholesale

- **Varint encoding (§16)** — small, self-contained, no reason not to (§4
  above). Copy quinn-proto's ~50-line implementation nearly verbatim.
- **STREAM frame shape**: `(Stream ID, [Offset], [Length], Data)` with
  OFF/LEN/FIN as type-byte bits — this is the entire fragmentation +
  ordering mechanism in one frame, exactly what the redesign wants (§5).
- **The stream-ID low-bit tagging idea** (initiator/direction in the ID
  itself) — cheap, avoids a side-channel "who opened this and which way does
  data flow" lookup. Slither likely only needs 1 tag bit (bidi vs. uni), not
  2, since — per §1 — a single flat ID space per connection is enough absent
  HTTP/3-style massive stream churn from both sides.
- **Implicit stream opening by first-use ID** (§2.1) — no explicit
  open-stream frame needed, consistent with how slither's Leg 1 sessions
  already come up implicitly on first Data packet with a new index.
  Skipping the "consistent ordering" nuance is fine at connection-scale
  concurrency (few streams).
- **Absolute-offset flow control (MAX_DATA/MAX_STREAM_DATA)** at both levels
  — cheap, prevents unbounded buffering, composable with reliable delivery
  (§3). This is new work relative to today's Leg 2 (which has none), but it's
  small and directly required once streams can outrun a receiver's buffer.
  Skip the BLOCKED-family diagnostic frames initially.
- **RFC 9002-shaped loss detection** — slither already has this
  (SPEC §9.4, ratified) and it is explicitly congestion-control-independent
  (confirmed above) and stream-agnostic: it operates on "which frames were in
  which packet," so extending it from whole-DATA-frame retransmission to
  STREAM-frame-range retransmission is a natural generalization, not a new
  subsystem.
- **RFC 9221's DATAGRAM-as-a-frame-type model** — fold slither's existing
  unreliable-datagram notion into the unified frame layer as one more frame
  type (no flow control, no retransmission, no ordering guarantee — §2 above)
  instead of keeping a separate Leg 1/Leg 2 tier split. Strictly simplifies
  the layering.

### Adapt (take the idea, shrink the mechanism)

- **Stream-ID namespaces**: collapse QUIC's 4 spaces (client/server × bidi/
  uni) to 1–2 (see §1) — slither's IK handshake already knows unambiguously
  who's the initiator, and there's no third-party protocol multiplexed on top
  needing HTTP/3's degree of stream-churn isolation.
- **Concurrency limits (MAX_STREAMS/STREAMS_BLOCKED)**: adapt to a simple
  fixed cap (a protocol constant, like slither's existing `MAX_ACK_RANGES` or
  `MAX_MESSAGE`) rather than a peer-negotiated, runtime-adjustable limit —
  cuts two frame types and their state tracking for a property slither likely
  doesn't need to tune per-connection.
- **CONNECTION_CLOSE**: slither has none today (the `0x04` reserved slot is
  never emitted — SPEC §2); QUIC's version (error code + optional frame-type
  + reason string) is worth adapting in simplified form — an explicit close
  frame (error code + short reason) is cheap and gives slither's peers a
  graceful-teardown signal it currently lacks entirely (today: only
  timer-driven death via `DEAD_TIMEOUT`/`REJECT_AGE`).
- **Stream state machine**: adopt the RFC's *conceptual* states (open →
  sending/receiving → closed, plus a reset side-path) but implement it
  quinn-proto's way — a 2–3-variant enum plus a couple of counters/booleans
  (`finish_acked`, buffered/read offsets), not 6 literal enum states. The RFC
  diagram is documentation, not an implementation mandate (confirmed by
  reading quinn-proto's actual `SendState`/`RecvState` against the RFC
  figures, §1 above).
- **The reliable-unordered-message vs. streams-only question (§5)** — genuinely
  open; recommend deciding via the SPEC discussion rather than defaulting to
  either QUIC's answer (streams only) or Leg 2's answer (messages only)
  without weighing slither's actual traffic shape (small trusted-peer RPC
  vs. bulk transfer).

### Drop entirely

- **Version negotiation** (QUIC §6 packet, VN mechanism) — slither has a
  1-byte version already, "unknown ⇒ silent drop"; no negotiation dance
  needed absent a desire to support multiple protocol versions live.
- **Connection migration frames** (`NEW_CONNECTION_ID`/`RETIRE_CONNECTION_ID`/
  `PATH_CHALLENGE`/`PATH_RESPONSE`, §19.15-19.18, and CID privacy rotation) —
  slither's roaming (SPEC §6, "an authenticated fresh Data packet whose source
  differs… moves the endpoint") already solves the single-path-follows-the-peer
  case WireGuard-style, with no need for QUIC's explicit multi-path/CID-pool
  machinery, which exists for privacy-preserving CID rotation and genuine
  multi-path racing that slither doesn't do.
- **0-RTT / packet-number-space zoo** (Initial/Handshake/0-RTT/1-RTT
  encryption levels, §17) — an artifact of layering TLS 1.3 (with its own
  0-RTT early-data concept) under QUIC. Noise IK via `hiss` is a fixed
  2-message handshake with its own transport-key derivation; there's no
  equivalent "send data before the handshake completes" mode to protect
  against replay for, and no multiple encryption-level packet number spaces
  to reconcile.
- **ACK_FREQUENCY / IMMEDIATE_ACK** (quinn-proto extension frames, not core
  RFC 9000) — slither's ACK policy is already "immediate, every ack-eliciting
  packet" (SPEC §9.2); there's no tunable cadence to negotiate.
- **CRYPTO frames and NEW_TOKEN** (§19.6-19.7) — TLS-handshake-fragment and
  address-validation-token concepts respectively; `hiss`'s Noise handshake is
  fixed-size and single-message-pair, needs neither.
- **Transport parameter negotiation as a wire concept** — slither has no
  version/parameter negotiation surface at all today; initial flow-control
  limits etc. can simply be fixed protocol constants (as slither already does
  throughout SPEC.md, e.g. `MAX_DATAGRAM`, `REKEY_EPOCH_MSGS`) rather than
  peer-exchanged parameters, unless a genuine need for asymmetric/tunable
  limits emerges later.

---

## Reference index

- RFC 9000 §2 (Streams), §2.1 (Stream IDs), §2.2 (Sending/receiving data),
  §3 (Stream States, Figures 2–3), §4 (Flow Control), §4.1–4.2 (data limits),
  §4.6 (stream limits), §16 (Variable-Length Integer Encoding), §19.2 (PING),
  §19.3 (ACK), §19.4 (RESET_STREAM), §19.5 (STOP_SENDING), §19.6 (CRYPTO),
  §19.7 (NEW_TOKEN), §19.8 (STREAM), §19.9 (MAX_DATA), §19.10
  (MAX_STREAM_DATA), §19.11 (MAX_STREAMS), §19.12 (DATA_BLOCKED), §19.13
  (STREAM_DATA_BLOCKED), §19.14 (STREAMS_BLOCKED), §19.19 (CONNECTION_CLOSE).
- RFC 9002 (loss detection, confirmed congestion-control- and flow-control-
  orthogonal).
- RFC 9221 §2 (rationale), §3 (`max_datagram_frame_size` transport parameter),
  §4 (DATAGRAM frame format), §5.2–5.3 (no reliability/ordering/flow control).
- `quinn-proto-0.11.16` (local, `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/`):
  - `varint.rs` — VarInt encode/decode (matches RFC 9000 §16 exactly).
  - `lib.rs:230-303` — `StreamId` encoding/decoding, `Dir`.
  - `frame.rs:108-160` — `FrameType` constants (`STREAM_TYS = 0x08..=0x0f`,
    `DATAGRAM_TYS = 0x30..=0x31`, all other frame type bytes), `Frame` enum.
  - `frame.rs:454-520` — `Stream`/`StreamMeta` struct + wire encode
    (OFF/LEN/FIN bit packing).
  - `frame.rs:827-853` — `ResetStream`, `StopSending` struct layouts.
  - `frame.rs:893-898` — `Datagram` struct (RFC 9221).
  - `connection/streams/state.rs:23-140` — `StreamsState`: per-connection
    flow-control fields (`max_data`, `local_max_data`, `sent_max_data`,
    `data_sent`, `data_recvd`, `unacked_data`, `send_window`,
    `stream_receive_window`, initial-limit fields from transport parameters).
  - `connection/streams/send.rs:280-288` — `SendState` (collapsed 3-variant
    implementation of the RFC's 6-state diagram).
  - `connection/streams/recv.rs:432-436` — `RecvState` (collapsed 2-variant
    implementation).
  - `connection/streams/assembler.rs:11-58` — `Assembler`: out-of-order
    STREAM-frame byte-range reassembly (binary heap of buffered ranges +
    contiguous-prefix cursor).
  - `connection/datagrams.rs:1-70` — `Datagrams` API (send/max_size), no
    flow-control interaction, RFC 9221-shaped.
  - `transport_parameters.rs:40-60` — default initial flow-control/stream
    limits (`initial_max_data`, `initial_max_stream_data_*`,
    `initial_max_streams_*`, all default 0 unless configured).
- `/Users/nicolasdiprima/work/primetype/slither/SPEC.md` §§1-10 (current
  ratified wire — Leg 1 packet layer, Leg 2 reliable frame layer, `MAX_MESSAGE
  = 1159`, ACK/PTO/loss-detection design already RFC-9002-shaped).
- `/Users/nicolasdiprima/work/primetype/slither/TODO.md` (v0.2 plan; item 7:
  "Streams + congestion control: later milestone… Fragmentation… falls out of
  streams"; item 2: object-model sketch naming Leg 1's unreliable-datagram
  surface and a reserved `0x04` frame space for future STREAM work).
- `/Users/nicolasdiprima/work/primetype/slither/src/session.rs:207,277`
  (`Connection::seal`/`open` — today's unreliable-datagram primitive, the
  thing to fold into a DATAGRAM frame type per RFC 9221).

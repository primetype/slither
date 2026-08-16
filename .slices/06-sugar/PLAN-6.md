# PLAN-6 — Slice 6: Sugar (§9.8 messages, §11 datagrams)

Status: **COMPLETE.** Planner agent, HEAD `d64b290`, branch `main`.
Skeleton written before the first `Read` (working rule 2); §0 appended as read.

Closes stories **S15**, **S16**, **S30**.

**Headline.** Recommended cut: **none** — one slice, three concurrent agents
(one implementer, two blind test authors split by behaviour, not by layer).
**8 open questions**, **6 conflicts**, **9 unstated-scope findings**.

**Two corrections to my brief, per working rule 5.**
1. The DATAGRAM/`SentFrame` paragraph the brief attributes to
   `.slices/05-reliability/CONTRACT-5a.md` §2.7 is in **`CONTRACT-5b.md`**
   `:116-129`. `CONTRACT-5a.md` has no §2.7 — its §2 stops at 2.4. The claim
   itself is **verified true** against the code (§0 C6).
2. The brief's *"801 tests"* is the `--all-features` figure. Bare `cargo test` at
   `d64b290` runs **726** (603 lib + 103 + 11 + 4 integration + 5 doc); six
   integration targets sit behind `required-features = ["test-util"]`. Both are
   release gates, so both numbers matter — but a blind author told "801" and
   running `cargo test` will think it has broken something.

---

## §0. Evidence log (append-only, written as read)

<!-- every fact I establish lands here first, with file:line -->

### §9.8 (SPEC.md:3064–3208) — messages

- `send_message(bytes)`: allocates **the next outbound uni stream**, writes the
  whole payload, sets FIN, GCs the stream when the FIN'd range is fully acked.
  **No stream handle surfaces**; `Connection::acked()` is how the app awaits the
  ack; the send verb resolves **as soon as the payload entered send state**
  (3082–3087).
- Payloads above `MESSAGE_RECV_MAX` are rejected **at the handle** with
  `MessageError::TooLarge` (3088–3090).
- `recv_message()`: each incoming uni stream is one message; surfaces the payload
  **only when reassembly is complete (FIN and all bytes)**, then **frees the
  stream** (3091–3093).
- Shared supply (3094–3099): `accept_uni()`/`open_uni()` remain. A stream claimed
  by `accept_uni()` **leaves message consideration**; `recv_message()` surfaces
  **the oldest fully-reassembled unclaimed uni stream**. Mode is the receiver's
  choice, **invisible on the wire**.
- Ruling 51 (3101–3121): mixing the two receive modes on one connection is a
  **programming error**, normative. Failure is **defined and loud** — the
  overflow policy — not silent. Bidi is safe alongside messages.
- `MESSAGE_RECV_MAX` = `INITIAL_MAX_STREAM_DATA` = 262 144 B (3123–3129). The
  bound is **structural**: a sugar-consumed stream never earns more per-stream
  credit.
- **Where the bound is *not* enforced** (3139–3143, verbatim): *"`MESSAGE_RECV_MAX`
  is checked on the **send** side of `send_message()` only, so `open_uni()`
  bypasses that check entirely."* → **the receive side runs no size check at
  all.** What it does instead is the overflow policy below.
- Overflow rule (3145–3159): an **unclaimed** uni stream — neither claimed by
  `accept_uni()` nor surfaceable by `recv_message()` (it cannot be: no FIN) —
  that **reaches `MESSAGE_RECV_MAX`** is **reset by the receiver**. The receive
  half **retires** (its bytes **count as consumed at the connection level**,
  §10.3) and the receiver emits **RESET_STREAM (`0x04`)** with
  `error_code = MESSAGE_OVERFLOW = 0x06` — *"the one receiver-emitted reset
  (§9.6), retained and regenerated until acknowledged despite the retired half"*.
  Sender observes `WriteError::Reset(MESSAGE_OVERFLOW)`.
- **MUST trace what it emitted**, under §18.2 `slither::frames` (ruling 59,
  3156–3159). This is a **MUST**, i.e. an obligation slice 6 owes.
- Ruling 52 (3161–3175): the code is minted from §15.3's transport-reserved
  range; §15.3's *"never sent"* gains **this single named exception**. Not a
  layout change.
- **The guard** (3177–3188, the load-bearing paragraph): the receiver runs the
  check **while a `recv_message()` claim is pending, and at the instant such a
  claim is made**. It applies to **every** unclaimed window-full stream, **not
  merely the oldest**. Streams claimed by `accept_uni()` are **untouched**.

### §10.7 (SPEC.md:3482–3488) — exemptions

- DATAGRAM frames are **flow-control-exempt** (congestion-controlled instead).
- Retransmissions of the same stream bytes consume no new credit.

### §11 (SPEC.md:3490–3551) — datagrams

- §11.1: no delivery/ordering promise, no retransmission, **no sequence identity
  at all**. **ack-eliciting** (the packet is tracked; an ACK confirms the *packet*
  arrived, not that the datagram reached the application);
  **congestion-controlled** (count in flight, admission gate §14.5 applies, loss
  never retransmits); **flow-control-exempt**.
- §11.2: `MAX_DATAGRAM_PAYLOAD` = **1169 B** = `MAX_PLAINTEXT` − 1 — *"a type-`0x30`
  frame's one type byte, data to the end of the plaintext"*. **A datagram never
  spans packets.**
- §11.3: `DATAGRAM_SEND_QUEUE` = `DATAGRAM_RECV_QUEUE` = **64 datagrams**, bounded
  **by count**, discipline **drop-oldest with the newest always accepted**.
  *"Both queues, their eviction discipline, and the drop counters live **in the
  connection core**, not the shell (§16.4)."*
- §11.4: send oversize → `DatagramError::TooLarge` **at the handle, before any
  queue**. Receive oversize is **impossible by construction** — no receiver
  oversize rule exists.
- §11.5: *"Every queue-overflow drop — send-side eviction and receive-side
  eviction — increments **a counter** surfaced on the `slither::frames` trace
  target (§18.2); the counters are core state."* Note singular/plural drift
  ("a counter" / "the counters") — see §6.

### §8.3/§8.4/§8.5/§8.7 — the DATAGRAM frame

- §8.3:2651 — `0x30`/`0x31` DATAGRAM, fields `[length (0x31 only)], data`,
  **ack-eliciting = yes**, **retransmission = never**.
- §8.4:2813–2823 — layout, verbatim:
  `type(0x30) ‖ data(to the end of the plaintext)  — must be final` /
  `type(0x31) ‖ length(varint) ‖ data(length B)`.
  Structural errors: **`length` overrunning the plaintext**; **a `0x30` frame
  that is not the packet's final frame**. Receiver-side oversize is
  unrepresentable.
- §8.5:2831–2832 — packing order: ACK → control (credit, RESET_STREAM, CLOSE) →
  **STREAM and DATAGRAM fill** → PING last. *"At most one extends-to-end frame
  (¬LEN STREAM, or `0x30` DATAGRAM) per packet, in final position."*
- §8.7:2862–2867 — the RESET_STREAM carve-out, verbatim: *"with one carve-out:
  the receiver-emitted overflow reset of §9.6/§9.8 retires its receive half at
  the moment of emission, so its **identity is retained at the connection
  level** and re-emitted until acknowledged; **the discard termination never
  applies to it**."*
- §8.7:2868 — `never` class = PADDING, PING, ACK, **DATAGRAM**, CLOSE.
  → confirms CONTRACT-5a §2.7: **no `SentFrame` identity for DATAGRAM.**

### §9.6 (SPEC.md:3019–3039) — the receiver-emitted reset, in full

- `final_size` carries **the receiver's highest received offset**, and is
  **informational**.
- The stream's **sender**, on receipt: stops (re)transmitting, **frees its send
  half** (un-ACKed ranges dropped; the freed half counts toward full closure,
  §9.7), and surfaces `WriteError::Reset(error_code)` **to a blocked or
  subsequent writer**.
- **No flow-control true-up runs in this direction** (3026–3027) — the frame
  releases the *sender's* obligation, not the receiver's credit. (This does
  **not** contradict §9.8's "its bytes count as consumed at the connection
  level": that is the **emitting receiver's** own retirement true-up, a
  different end of the wire. Two ends, two ledgers — spell this out in the
  contract so nobody "fixes" one into the other.)
- Retention (3028–3039): the identity `{ stream_id, MESSAGE_OVERFLOW, final_size }`
  is **retained in the connection's regenerate set** and re-emitted on loss
  **until acknowledged**; §8.7's "stream state is discarded" termination does
  not apply. Rationale given: without it, PTO retransmissions are no-op'd and
  ACKed below the closed-stream watermark, liveness never fires, the send half
  wedges.

### §9.7 (SPEC.md:3041–3062) — GC

- A **receive half** frees on exactly **three** triggers as written here:
  `DataRead` (read to final size), `ResetRead` (observed the reset), or
  **abandonment of a handle** (ruling 93). **It does *not* list "final size
  reached with no reader."** See §7 (conflicts).
- A stream is **fully closed** when its halves are freed; full closure earns the
  peer a MAX_STREAMS credit (§10.4).

### §10.2/§10.3/§10.4 — credit, and why the message bound is structural

- §10.2:3247–3252 — `INITIAL_MAX_DATA` 1 MiB, `INITIAL_MAX_STREAM_DATA` 256 KiB,
  `INITIAL_MAX_STREAMS_BIDI` 32, `INITIAL_MAX_STREAMS_UNI` **128 (cumulative —
  higher for message traffic, §9.8)**.
- §10.3:3275–3283 — re-grant: prospective limit = `bytes_read + WINDOW`; emit
  when `prospective − last_advertised ≥ WINDOW/2`. **"Consumption, not arrival,
  drives credit: an unread buffer earns nothing."**
- §10.3:3289–3290 — *"**Sugar-consumed streams never earn stream-level credit**
  (§9.8); their reads still earn connection-level credit."*
  → **KEY: this needs no new suppression mechanism.** An unclaimed peer-opened
  uni stream has `bytes_read = 0`, so its prospective limit never moves off
  `INITIAL_MAX_STREAM_DATA` and no MAX_STREAM_DATA is ever emitted for it. The
  262 144-byte ceiling on an unclaimed uni stream is therefore *already* what
  slices 4–5 implement, provided credit advance is read-driven. **Verify in
  code** (see §8, Q4).
- §10.3:3291–3303 — **retirement advances connection credit**, five triggers
  (see §7 for the fifth), true-up is **absolute, monotone bring-to-final**,
  idempotent with bytes already counted by reads, and runs **after** the §8.4
  `FLOW_CONTROL_ERROR` check.
- §10.4:3355–3360 — the peer grants **+1 MAX_STREAMS when it fully closes a
  peer-opened stream**, batched at `STREAMS_CREDIT_BATCH` = 8 (or when the
  peer's remaining allowance ≤ 8). → **`recv_message()` is what pays for the
  next message's stream allowance.** A message-mode receiver that stops calling
  `recv_message()` stalls the sender's `send_message()` at stream allowance,
  which is the intended backpressure.

### §10.6 (SPEC.md:3477–3481) — consumption, defined

*"Consumption is the application taking bytes out of the connection core — a
`read()` draining the contiguous prefix, **a message or datagram claimed by its
verb** (§16.4), or a retirement true-up (§10.3). **No unbounded intermediate
queue may exist between core and handle**: message and datagram payloads stay
accounted inside the core (or its flow-control ledger) **until the handle takes
them**."*
→ A completed-but-unclaimed message keeps holding connection credit. The
retirement true-up happens **at the claim**, not at FIN.

### §15.3 (SPEC.md:4058) — `MESSAGE_OVERFLOW`

`0x06`, *"Carried in RESET_STREAM's `error_code`, **never in CLOSE**."*
`0x07`–`0x0f` reserved, never sent.

### §18.1 (SPEC.md:5652–5664)

- `WriteError::{Reset(u64), ConnectionLost(ConnectionLost), Finished}`,
  `#[non_exhaustive]` — the **only** non-exhaustive error type (ruling 61).
- `MessageError::{TooLarge, ConnectionLost(ConnectionLost)}` — **exhaustive**.
- `DatagramError::{TooLarge, ConnectionLost(ConnectionLost)}` — **exhaustive**.
  → **There is no `WouldBlock`/`Blocked` variant available to either, and
  adding one is a major version bump plus a ruling.** This bites `send_message`
  in the core — see §8, Q1.

### §18.2 (SPEC.md:5694, ruling 59 at 5719–5731)

`slither::frames` carries: the frame layer's violation CLOSEs, **the datagram
queue-overflow drop counters (§11.5)**, and **the message-mode overflow reset we
emit — the stream, its final size, and the mode conflict that caused it**.
Ruling 59 is a **MUST** and repeats the three fields. Note §18.2 says
"counter**s**" (plural) where §11.5 says "a counter" — see §6.

### §16.2 (SPEC.md:4152–4185, 4281–4285, 4416–4451)

- Shell signatures, verbatim:
  ```rust
  pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;
  pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;
  pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>;
  pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;
  ```
  **`send_datagram` is `fn`, not `async fn`.**
- 4283–4285: *"`send_message` **waits for stream allowance**, then behaves per
  §9.8. `send_datagram` **never waits** (drop-oldest queue, §11.3)."*
- 4416–4419 (ruling 47): *"`send_message()` and `finish()` resolve when the
  payload and the FIN are **accepted into send state**, not when the peer has
  them."*
- 4441–4447 (ruling 47): `Connection::acked()` takes a **snapshot** *"across
  every stream, **including the message streams §9.8 never surfaces a handle
  for**"*, and resolves when each byte is acknowledged **or abandoned by a
  reset**.
- 4434–4439: `SendStream::acked()` *"returns `Reset(code)` if the stream was
  reset before its data was acknowledged (a local reset, **or the peer's §9.8
  overflow reset**)"*.
- 4335–4343: the pull model — `notified()` claims **one**, *"in the same pull
  model as `accept_bi`, **`recv_message` and `recv_datagram`**"*.

### §16.4 (SPEC.md:4830–4850, 4864–4869) — the core API, verbatim

```rust
fn send_message(&mut self, now: Instant, msg: &[u8]) -> Result<(), MessageError>;
fn send_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), DatagramError>;
// claim verbs — the pull model (§10.6, §11.3, §9.8):
fn recv_message(&mut self) -> Option<Vec<u8>>;   // claim the oldest complete unclaimed message
fn recv_datagram(&mut self) -> Option<Vec<u8>>;  // claim the oldest queued datagram
```
`ConnEvent::MessageReadable` and `ConnEvent::DatagramReadable` already appear in
§16.4's enum (4867–4868) and in §16.2's "already served" list (4389–4396).
**Note: neither claim verb takes `now`** — but `recv_message()` is the trigger
for the overflow check, which emits a frame. See §8, Q2.

### §16.11 (SPEC.md:5264–5273, ruling 58)

*"**Normative.** A `Stream` adapter over `recv_message`, `recv_datagram`,
`accept_bi`/`accept_uni` or `notified()` claims **at most one item, and only
from inside `poll_next`**."* The named faces (`messages`, `datagrams`,
`incoming_bi`, `incoming_uni`, `notifications`, and **the two sinks**) are
*"ordinary adapters ... otherwise unconstrained by this document"*.

### Appendix B (SPEC.md:6018–6039) — the obligations slice 6 owes

Verbatim, the two bullets that name slice 6's behaviour:
- *"Message sugar: **exactly-once surfacing**, the 256 KiB bound at the handle,
  **GC after full ACK** (§9.8); the overflow reset (a FIN-less window-filling
  uni stream under a pending `recv_message()` is reset, the sender surfaces
  `WriteError::Reset`, **connection credit trues up**); **the lost-reset case**
  (the receiver-emitted RESET_STREAM is regenerated until acknowledged —
  dropping its first transmission still frees the sender, §9.6, §8.7)."*
- *"**The unclaimed uni stream fails loudly** (§9.8, ruling 51) ... assert on the
  paused clock that A's write resolves `Err(WriteError::Reset(MESSAGE_OVERFLOW))`
  ... that B's connection credit trues up, and that **keepalives kept both sides
  alive throughout**. **Assert the not-oldest case too**: a second such stream
  behind a slower unclaimed one is reset on its own account. And assert the
  **negative**, which is what the guard buys: a receiver that uses
  `accept_uni()` only, and is slow to call it, is **not** reset — its
  window-full stream waits under §16.4's backpressure-by-retention and
  **resumes when the accept lands**."*
- *"DATAGRAM: no retransmission on loss; queue-overflow drop-oldest with newest
  accepted; the drop counter emitted on `slither::frames` (§11)."*
- 6238–6241: the ruling-47 message-then-close obligation (`send_message(m).await;
  acked().await; close(NO_ERROR,"").await; drop handles` ⇒ B receives `m` in
  full) — **already exercised in slice 5b? verify; if it used a stream instead
  of `send_message`, slice 6 owes the message form.**

### CODE SURVEY — core (`src/core/`), verified at `d64b290`

**C1. `poll_output` seals nothing, and there is no `now` cache.**
`Connection` has **no `Instant` field** (`src/core/connection/mod.rs:102-137`).
`poll_output` (`mod.rs:422-426`) is a pure pop of `self.outputs`, falling through
to `ConnOutput::Timeout(self.timers.next())`. Its own doc: *"Takes no `now` and
seals nothing: §16.7 puts sealing inside the mutating call that triggers it."*
Sealing is `pump(now)` → `pump_packets(now, probe)` (`mod.rs:1163-1319`) using the
`now` **argument**. No `Instant::now()` exists in non-test `src/core/`.
→ **A verb that must put a frame on the wire needs `now`.** `abandon_recv` takes
`now` (`mod.rs:574`) for exactly this reason: retiring a half owes MAX_DATA.
`accept(dir)` (`mod.rs:461`) does **not** take `now` — and correctly, because it
emits nothing. `recv_message()` retires a half **and** may emit RESET_STREAM.
**See §7 conflict C-1 and §8 Q1.**

**C2. The `unclaimed` queue already exists and already names slice 6.**
`streams.rs:256-262`: `unclaimed: [VecDeque<StreamRef>; 2]`, doc: *"Slice 6's
message seam (SPEC §9.8): a **queue of unclaimed halves**, not 'the newest one' —
§9.8 adds a second claim verb drawing from the same supply."* `Streams::accept`
(`streams.rs:326-335`) is `pop_front` — ruling 112's FIFO. **Claimed/unclaimed is
membership in that deque and nothing else — there is no per-`Stream` flag**, and
the pop is irreversible. Three touch points: `streams.rs:334`, `:1087`, `:1137`.
Peer-opened streams are created **eagerly** on the first STREAM *or* RESET_STREAM
frame via `locate` (`streams.rs:1078-1091`), live fully in `entries`, and receive,
charge credit and reassemble while unclaimed.

**C3. `earns_stream_credit` already exists — slice 4 built slice 6's hook.**
`RecvHalf.earns_stream_credit: bool` (`recv.rs:~72`), doc: *"§12.1's seam. §10.3:
'Sugar-consumed streams never earn stream-level credit'. In slice 4 the answer is
always `true` — a **field**, not a constant, so slice 6 does not thread a mode
flag through the ledger."* The suppression point is `RecvHalf::take_grant`
(`recv.rs:264-273`), which returns `None` when the flag is false, so
`owed.max_stream_data` is never populated and the connection-level MAX_DATA path
(`streams.rs:422-424`) is untouched — **exactly what §10.3 asks for**. See §8 Q4:
the flag may be *vacuous*, because credit is read-driven and a message stream is
never `read()`.

**C4. The DATAGRAM seam is pre-cut, deliberately.**
- `FRAME_DATAGRAM = 0x30` / `FRAME_DATAGRAM_LEN = 0x31` (`constants.rs:205,208`)
  with `const _: () = assert!(FRAME_DATAGRAM_LEN == FRAME_DATAGRAM | 0x01);`
  (`constants.rs:578`).
- `Frame` has **no** DATAGRAM variant (`frame.rs:65-87`); `frame.rs:63-64` says
  *"§11's DATAGRAM arrives with slice 6."*
- **The classifiers already answer for both type codes**: `is_ack_eliciting`
  (`frame.rs:687`) → `true`; `retransmission` (`frame.rs:726`) →
  `Some(Retransmission::Never)`, both already under test (`frame.rs:1206`,
  `:1231`). Slice 6 must **not** re-decide these.
- `parse` has no arm for `0x30`/`0x31` — they hit
  `other => return Err(Structural::UnknownType(other))` (`frame.rs:648`) with the
  comment *"Slice 6 adds arms above; it does not widen this one."*
- `Packing.extends_to_end` (`frame.rs:742-755`) already enforces §8.5, and its doc
  says: *"the rule is a property of the **stage**, not of STREAM. Slice 6 adds
  DATAGRAM as a second contributor to the same stage; a check living in the stream
  fill loop would be duplicated there, and the duplicate would be the one that
  drifts."* Enforcement is in `Packing::push` (`frame.rs:843-865`).
- Stages: `Ack < Control < Fill < Ping` (`frame.rs:757-795`); `fill()` is *"Stage 3
  — the STREAM and DATAGRAM fill (§8.5)"*. `pump_packets` calls them in order
  (`mod.rs:1188-1210`). Budget = `MAX_PLAINTEXT` = 1170.

**C5. slither has never emitted an extends-to-end frame.**
`Stream::new` hard-sets `len_present: true` (`frame.rs:225-234`), so
`Frame::extends_to_end()` (`frame.rs:126-130`) is **always false** for anything
this build produces. The parser accepts the ¬LEN form but nothing emits it.
→ **Consequence, and it is not optional:** a `MAX_DATAGRAM_PAYLOAD`-sized
datagram (1169 B) does not fit the `0x31` form — `1 + varint(1169)=2 + 1169 =
1172 > 1170`. **Slice 6 must emit `0x30` for it, which makes `extends_to_end` live
for the first time.** See §8 Q3.

**C6. `SentPacket`/`SentFrame` — CONTRACT-5a §2.7 verified true.**
`recovery.rs:34-74`: `SentPacket { counter, time_sent, size, app_limited,
path_gen, frames: Vec<SentFrame> }`; `size` is computed from the plaintext length
**before any `SentFrame` is consulted** (`mod.rs:1215-1221`) and `bytes_in_flight`
is maintained from `size` alone (`recovery.rs:202`); `frames` is documented
*"May be empty"*. `SentFrame` (`recovery.rs:76-112`) has five variants — `Stream`,
`ResetStream`, `MaxData`, `MaxStreamData`, `MaxStreams` — **no DATAGRAM, and none
is needed.** `Retransmission::Never` means `apply_ack_outcome` (`mod.rs:763-812`)
has nothing to do on either the acked or the lost path.

**C7. `MESSAGE_OVERFLOW` exists and is unused by production code.**
`constants.rs:497-500`, `pub const MESSAGE_OVERFLOW: u64 = 0x06;`. Referenced only
by `constants.rs:722`'s name table and `tests/spec_constants.rs:667,672,801`.
`Violation::code()` (`flow.rs:66-74`) maps to five codes and not this one.

**C8. Receiver-emitted reset machinery does NOT exist; the sender-emitted kind
does, and the two are deliberately separate.**
`send.rs:59-64`, verbatim: *"Slice 6's message seam (SPEC §9.8): the
**receiver**-emitted reset of §9.8 is retained in a connection-level regenerate
set that outlives the stream state. **These are deliberately not one structure.**"*
The existing connection-level regenerate set (`streams.rs:147-152`) holds only
`max_data: bool`, `max_streams: [bool; 2]`, `max_stream_data: BTreeSet<StreamRef>`
— **no reset member**. The sender-side reset lives in `ResetState { error_code,
final_size, pending, acked }` (`send.rs:65-72`) with: set-pending in
`SendHalf::reset` (`:276-292`) and `on_reset_lost` (`:482-486`, guarded by
`filter(|r| !r.acked)`); cleared at pack time by `take_reset` (`:377-384`);
re-owed on a full packet in `Streams::pack_control` (`streams.rs:764-796`);
terminated by `on_reset_acked` (`:474-479`) **or** by state discard via
`is_terminal()` (`:200-208`). `recv.rs` has **no** reset-emission code at all.
→ Slice 6 must build a **connection-level** retained identity whose termination is
acknowledgement **only** (§8.7:2862-2867). Reusing `ResetState` would inherit the
discard termination the spec says must never apply.

**C9. `ConnEvent` has neither `MessageReadable` nor `DatagramReadable`**
(`mod.rs:1452-1513`); grep finds neither identifier anywhere in `src/`. The doc
at `mod.rs:1435-1438` states the policy: *"The variants §16.4 lists that this
slice cannot yet construct … are absent rather than stubbed: an uninhabited
variant is a claim about the protocol."* Events stage in
`Connection.events: Vec<ConnEvent>` (`mod.rs:123-125`) and move to `outputs` via
`drain_events()` (`mod.rs:971-976`), called at `mod.rs:379`, `:619`, `:939`, `:967`.

**C10. `testfix.rs::parse_frames` panics on an unknown type**
(`testfix.rs:139-226`), with the message *"…this decoder has aged out and the arm
belongs here (working rule 15), not in the caller"*. It decodes into a separate
`Wire` vocabulary (`testfix.rs:80-124`). **A DATAGRAM emitted by the core panics
every fixture test that decodes a packet, and the panic accuses the wrong party.**
Precedent: the `Wire::Ack` arm was added at slice 5's integration for exactly this.

**C11. There is no counter/stats struct anywhere in `src/`.** The house pattern
for core introspection is `#[cfg(test)] pub(crate) fn` on `Connection`
(`mod.rs:679-701`: `bytes_in_flight`, `congestion_window`, `smoothed_rtt`,
`recovery`). `slither::frames` has three live sites: `mod.rs:854-859` (structural,
WARN), `mod.rs:955-960` (semantic violation, WARN), `recovery.rs:235-240`
(forged-future ACK, DEBUG). `Cargo.toml` records §18.2 as **a closed list of
exactly five targets**.

**C12. Test baseline.** `cargo test` (bare): **726 passing** — 603 lib + 103
`spec_constants` + 11 `spec_errors` + 4 `spec_packet` + 5 doc-tests, 0 failed,
0 ignored. Six integration targets are gated behind
`required-features = ["test-util"]` and run only under `--all-features`
(`story_lifecycle`, `story_dial`, `spec_shell`, `story_streams`, `spec_streams`,
`story_reliability`) — that is where the brief's 801 comes from.

**C13. `write`/`read`/`open` conventions as implemented** (needed verbatim in the
contract, because slice 4a's author had to guess this class of thing):
- `open(dir, flow)` (`streams.rs:304-317`) → `Err(StreamsExhausted)` when
  `index >= flow.remote_max_streams(dir)`.
- `write(now, r, data) -> Result<usize, WriteError>`; **`Ok(0)` means blocked, not
  EOF**, and `write(now, r, &[])` is a no-op that *also* returns `Ok(0)`
  (`mod.rs:465-482`, `send.rs:226-248`). Partial writes are normal.
- `read(now, r, buf) -> Result<Option<usize>, ReadError>`: `Ok(Some(n>0))` = data;
  **`Ok(Some(0))` = no data, park**; **`Ok(None)` = end of stream** (and retires
  the half); `Ok(None)` also for an unknown ref or a half-less stream;
  `Err(ReadError::ConnectionLost)` only when dead *and* the read was `Ok(Some(0))`
  (ruling 128).
- `finish` is **idempotent**; `reset` returns `()` and no-ops on a missing half.

### Stories (STORIES.md)

- **S15:263–268** — *"`send_datagram` never waits — it drops oldest under
  pressure rather than blocking — and delivery is unordered and unreliable by
  contract. Oversize input is `DatagramError::TooLarge`, not a silent
  truncation."* Anchor §11.3, §18.1. **Paused clock: yes.**
- **S16:271–276** — *"a message up to `MESSAGE_RECV_MAX` (262 144) in one call;
  larger is `MessageError::TooLarge`. (bubble-engine chunks above this and cares
  about the exact bound.) A datagram's own bound is separate:
  `MAX_DATAGRAM_PAYLOAD` = 1169."* Anchor **§9.8**, §18.1. **No paused-clock
  marker on S16** — the only one of the three without it.
- **S30:503–527** — four `Accepts` bullets: (1) mixing is a programming error by
  ruling, `open_bi()` is the documented alternative, tag-in-band the other;
  (2) the bounded named failure — `WriteError::Reset(MESSAGE_OVERFLOW)`, code
  `0x06`, *"distinguishable from a peer's `reset(0)` and from a dropped
  `SendStream`"*, instead of stalling for ever with keepalives flowing;
  (3) **the guard is load-bearing** — a stream-mode receiver merely slow to
  `accept_uni()` is **not** reset; (4) **ruling 59** — the receiver has no error
  and no notification, so it **MUST** trace under `slither::frames`, *"naming
  the stream, its final size, and the mode conflict."*

---

## §1. Cut recommendation

**Recommendation: do NOT cut slice 6 into core/shell. One slice, three
concurrent agents on disjoint paths — one implementer (core *and* shell) and
two blind test authors, split by *behaviour* rather than by layer: author A
owns datagrams (§11, S15), author B owns messages and the overflow reset
(§9.8, S16, S30).**

The argument is about where the review boundary buys something.

**1. A 6a/6b cut would put the boundary where there is nothing to review.**
Slice 6's shell half is four verbs. Three of them (`recv_message`,
`recv_datagram`, and `send_message`'s wait-for-allowance) are *literal* mirrors
of shapes already shipped in 4b and 5b: a `poll_*` over the shared cell, a
waker slot, a `poll_fn`, a `ConnEvent` arm in the driver's dispatch. The fourth,
`send_datagram`, is the only novel shape and it is novel by being **simpler** —
§16.2 declares it `pub fn`, not `async fn`, so it has no waker, no parking and
no cancel-safety surface at all. There is no design question in that half. An
integration round spent on it is an integration round not spent on the guard.

**2. Every open question in §8 below is a *core* question, and every one of them
is decided by code the same agent writes on both sides of the seam.** The
guard's trigger is "a `recv_message()` claim is pending" — a core flag whose
only producer is the shell's parked future and whose only consumer is the core's
reset scan. Ruling 113's defect is shipping state in one half whose consumers
live in the other. Here the two are one mechanism read from two sides, and the
cut would run it through a contract seam rather than through a compiler.
*(I state this weaker than slice 5's version of the same argument, deliberately:
unlike slice 5's sent-packet map, this one **is** exercisable in 6a — a core
test can call `core.recv_message()` directly and observe `None`. What the cut
would cost is not testability but the chance to get the flag's clearing rule
wrong on each side independently.)*

**3. S30's ratified acceptance shape is an integration test, so a 6a would close
no story.** Appendix B (SPEC.md:6025–6039) writes S30 as: A calls `open_uni()`
and writes past 262 144 while B **only ever** calls `recv_message()`; assert on
the paused clock that **A's write resolves `Err(WriteError::Reset(MESSAGE_OVERFLOW))`**,
that B's connection credit trues up, and that keepalives kept both sides alive
throughout. Every noun in that sentence is a shell handle. Under a 6a/6b cut,
6a closes none of S15/S16/S30 and CLAUDE.md's bar — *"a slice is done when its
stories are paused-clock tests that pass"* — is met only at 6b. That is
survivable precedent (4a and 5a did it), but it is a poor trade at slice 6's
size: `PLAN.md:302` puts slice 6 at **≈ 1.5k lines, half of slice 4 or 5**, and
the cut's fixed cost — a contract, a dispatch, an integration round — does not
halve with it.

**Why the split that *is* worth making is datagrams-vs-messages, and why it is
a test-author split rather than a slice split.** The two bodies of work share
exactly three things: the packing pass in `poll_output`, two new `ConnEvent`
variants, and the `slither::frames` trace target. Everything else is disjoint —
different frame types, different state, different stories, and, decisively,
**different kinds of evidence**. Datagrams are shallow and broad: a frame
layout with a positional constraint (§8.5's one-extends-to-end-frame-in-final-
position), two bounded queues, an eviction discipline, counters, and three
cross-cutting classifications to pin (ack-eliciting §8.3, congestion-controlled
§14.5, liveness-**marking** §7.4). Messages are narrow and deep: one lifecycle,
one guard, one retained frame identity that outlives the state it names.
Slice 5 used two blind authors for exactly this reason and recorded it
(rulings.md:3550–3553). Slice 6's case is at least as strong.

**They must not be two concurrent *implementers*.** Both halves touch
`src/core/connection/mod.rs` (the verb surface, the event queue, the
`poll_output` packing pass) and `src/shell/connection.rs` (the four verbs) and
`src/shell/shared.rs` (two waker slots). Working rule 6 admits no shared path
between concurrent agents, and slice 2a's lesson is that the finish order
decides who wins. One implementer, two test authors, three disjoint path sets —
see §3.

**Sequencing.** Implementer and both test authors dispatch **together** from a
commit that already contains `CONTRACT-6.md` (working rule 14 — slice 4a's
author reconstructed an API for ten minutes because `CONTRACT-4a.md` landed one
commit late). The integrator then wires `Cargo.toml`'s `[[test]]` stanzas and
`testfix.rs`'s frame parser, which are the two files that are only valid once
both sides exist (§3).

---

## §2. The binding API contract (becomes `CONTRACT-6.md`)

*House style of `CONTRACT-5a/5b`: a **BINDING** banner, §0's rulings table,
signature blocks, then the algorithms, then the appendix of prohibitions. Style
rule carried from `CONTRACT-5a.md:35-40`: **every return value is stated for
every state, including the ones that look obvious.** Three items below are marked
⚠ **RULING REQUIRED** — they are not the planner's to decide, and they are §8's
Q1–Q3.*

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

⚠ **The `0x30`-not-final check has nowhere to live today, and this must be said
out loud.** `parse_body` receives a `&[u8]` and returns bytes consumed; a `0x30`
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
/// §16.4's `send_message`.  ⚠ RULING REQUIRED on the return type — §8 Q1.
pub(crate) fn send_message(&mut self, now: Instant, msg: &[u8])
    -> Result<SendMessage, MessageError>;

/// §16.4's `recv_message`: claim the oldest **complete unclaimed** uni stream
/// as one payload, then free the stream.
///
/// ⚠ RULING REQUIRED: **`now` is added to §16.4's signature** — §8 Q2 / §7 C-1.
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
| **the connection is dead but the queue is non-empty** | `Some(payload)` — the core does **not** consult `self.lost`. The death check is the shell's, *after* the core returns `None`. ⚠ see §8 Q5. |

**`recv_message` — every outcome**

| Condition | Result |
|---|---|
| some unclaimed peer-opened **uni** stream is complete (FIN pinned **and** every byte to the final size received) | `Some(payload)` of the **oldest such stream in open order** (ruling 112's FIFO, filtered to complete). The stream leaves `unclaimed`, its receive half is **retired**, its bytes count as consumed for connection credit (§10.3), and MAX_STREAMS_UNI credit is owed to the peer (§10.4). |
| no unclaimed uni stream is complete | `None`, **and the pending-claim flag is set** (§4.2) |
| a complete unclaimed stream carries a **zero-byte** payload (FIN at offset 0) | `Some(Vec::new())` — an empty message is a message. **Not** `None`; `None` must mean "nothing to claim" or the shell parks on a delivered message for ever. |
| the connection is dead but a complete unclaimed stream remains | `Some(payload)` — the core does not consult `self.lost`. ⚠ **§8 Q5 / §7 C-2: Appendix B's message-then-close obligation requires this and no ruling says so.** |
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
5. the core says *not now* → park → `Pending`. ⚠ **Q1 decides what "not now" is
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
| `message_senders` | `Wakers` | `ConnEvent::StreamsAvailable { dir: Dir::Uni }` (extend the existing arm at `driver.rs:540-542`); **the death latch**; ⚠ **and whatever Q1 rules for the credit wake** |

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
entry and no `Drop for SendStream` to remove it. ⚠ **Unbounded growth — §9 R4.**

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

## §3. Module map and file ownership

Working rule 6 is absolute: **no two concurrent agents may name the same path.**
Three agents run concurrently — `6-impl`, `6-test-dgram`, `6-test-msg`.

### 3.1 The table

| Path | New? | Owner | Contents |
|---|---|---|---|
| `src/core/connection/datagram.rs` | new | **6-impl** | `Datagrams`, `DatagramDrops`, §11.3's discipline |
| `src/core/connection/frame.rs` | edit | **6-impl** | `Frame::Datagram`, the five dispatch arms, the `0x30`-not-final check in `parse`'s loop |
| `src/core/connection/mod.rs` | edit | **6-impl** | the four verbs, the two `ConnEvent` variants, the datagram fill in `pump_packets`, the liveness-marking OR, the `#[cfg(test)]` counter accessor, **and the `#[cfg(test)] mod tests_datagram; mod tests_message;` declarations — declared here, files created by nobody but the test authors** |
| `src/core/connection/streams.rs` | edit | **6-impl** | the overflow scan, the connection-level retained reset, `recv_message`'s claim + retire, the pending-claim flag |
| `src/core/connection/recv.rs` | edit | **6-impl** | whatever the completeness predicate and `earns_stream_credit` need |
| `src/core/connection/send.rs` | edit | **6-impl** | `send_message`'s stream (if anything is needed beyond `write`/`finish`) |
| `src/core/connection/recovery.rs` | edit | **6-impl** | **only** if the retained reset needs a `SentFrame` routing change — it must **not** gain a DATAGRAM variant (§2.7) |
| `src/shell/connection.rs` | edit | **6-impl** | the four verbs + three `poll_*` |
| `src/shell/shared.rs` | edit | **6-impl** | the three waker slots, `ConnCell::new`, `take_all_stream_wakers` |
| `src/shell/driver.rs` | edit | **6-impl** | two new `publish` arms; extend the `StreamsAvailable` arm |
| `src/shell/mod.rs` | edit | **6-impl** | the module-doc line listing absent verbs (`mod.rs:43`) — **and it holds slice 3b's inline `#[cfg(test)] mod tests`, so 6-impl owns the shell's internal tests and no test author opens it** (slice 4b's correction 3) |
| `src/lib.rs` | edit | **6-impl** | re-exports, if any are missing |
| `src/core/connection/tests_datagram.rs` | new | **6-test-dgram** | **this agent's alone.** 6-impl creates nothing at this path. |
| `tests/story_datagram.rs` | new | **6-test-dgram** | S15, end to end |
| `src/core/connection/tests_message.rs` | new | **6-test-msg** | **this agent's alone.** |
| `tests/story_message.rs` | new | **6-test-msg** | S16 and S30, end to end |

**On nobody's path:** `src/error.rs` and `src/constants.rs` (§2.6 — everything
slice 6 needs already exists). `src/testutil/mod.rs` is **6-impl's** by slice
4b's correction 1 if it is touched at all; the test authors must not open it, and
should **report** a missing fixture capability rather than add one
(`CONTRACT-5b.md:149-152`).

### 3.2 The integrator's files (working rule 15)

These are only valid once both sides exist, or are somebody else's file already.
**Not yours, either of you:**

- **`src/core/connection/testfix.rs` — and slice 6 is the case working rule 15
  was written for.** `parse_frames` (`testfix.rs:139-226`) **panics** on any
  frame type it does not know, with a message accusing the core of crossing a
  slice boundary. **Slice 6 adds a frame type.** The moment `6-impl` emits a
  `0x30`/`0x31`, every fixture test in `tests_streams.rs`, `tests_ack.rs`,
  `tests_recovery.rs` and `tests.rs` that decodes a packet containing one dies —
  **and the tree still compiles**, so it fails only when tests run, and the
  message blames the implementer for a decoder that has aged out. Precedent:
  the `Wire::Ack` arm was added at slice 5's integration for exactly this.
  **The `Wire::Datagram { data, had_len }` arm must be added by the integrator
  and committed BEFORE the worktrees are cut** (working rule 14): `6-test-dgram`
  cannot write a single assertion about a datagram frame without it, and slice
  4a's author lost ten minutes to precisely this. The arm's `0x30` branch is the
  ¬LEN STREAM branch's shape — `len = pt.len() - at`.
- **`Cargo.toml`'s two `[[test]]` stanzas** for `story_datagram` and
  `story_message`, both `required-features = ["test-util"]`. Cargo **refuses to
  parse the manifest** when a `[[test]]` names a missing file, which reds every
  gate at once (ruling 126). `6-impl` may commit them **commented out** under an
  integration header; the integrator uncomments them.
- **`src/core/connection/tests.rs`, `tests_streams.rs`, `tests_ack.rs`,
  `tests_recovery.rs`** — existing test authors' files. `ConnEvent` gains two
  variants; any exhaustive `match` in them that stops compiling is an
  **integration** edit made after both agents finish, never concurrently
  (`PLAN.md:748-752`).
- **`tests/spec_shell.rs:18`** — the debt table's row for the sugar verbs, if one
  exists. One line, integrator's, at the end.

### 3.3 The one sequencing hazard

`6-impl` touches `src/shell/shared.rs` and `src/shell/driver.rs`, which is where
slice 4b's plan recorded its collision (`PLAN.md:774-782`). Nothing else is in
flight now, so there is no conflict — but the **testfix arm must land first**,
and the commit that carries it is the commit all three worktrees are cut from,
**named in all three briefs** (working rule 14).

---

## §4. The algorithms and state

### 4.1 §11.3's queues and §11.5's counters — `Datagrams`, in `datagram.rs`

**The rule.** Both queues bounded by **count** at 64. Discipline: **drop-oldest,
newest always accepted.** On eviction the matching counter increments and the
drop is traced on `slither::frames`.

**The state.** `Datagrams { send: VecDeque<Vec<u8>>, recv: VecDeque<Vec<u8>>,
drops: DatagramDrops { send: u64, recv: u64 } }`, owned by `core::Connection`.
§11.3 is explicit that this is **core** state — *"not the shell (§16.4) — the
bound is protocol state, not a delivery detail."*

**Where each half is driven.**
- **Send push**: `Connection::send_datagram` (`mod.rs`), after the `lost()` check
  and the size check, before `pump(now)`.
- **Send pop**: the stage-3 fill inside `pump_packets` (`mod.rs:1188-1210`).
- **Recv push**: the DATAGRAM arm of frame application (alongside
  `on_stream_frame` in `streams.rs`'s caller), emitting `DatagramReadable`.
- **Recv pop**: `Connection::recv_datagram`.

**What the counters are not.** They are **not** a delivery-loss counter. A
datagram lost on the wire increments nothing — §11.1 promises no delivery and
§8.7 puts DATAGRAM in the `never` class. Only **queue-overflow evictions** count.
State this in the contract: a test author who assumes otherwise writes an
assertion no conforming build can satisfy.

### 4.2 The datagram frame, and the two type codes

**Encoding rule (a sender policy — §8.4 admits both forms; §8.5 constrains
position).** For the datagram at the front of the send queue, with
`room = packing.room()`:
1. if `1 + varint_len(n) + n <= room` → emit **`0x31`** (length-prefixed);
2. else if `1 + n <= room` → emit **`0x30`** — `Packing::push` then latches
   `extends_to_end` and nothing further can be packed into this packet
   (`frame.rs:843-865`, already enforced);
3. else leave it queued for the next packet.

Step 2 is **not optional.** `MAX_DATAGRAM_PAYLOAD` = 1169 = `MAX_PLAINTEXT` − 1
is defined *by* the `0x30` form (§11.2), and a 1169-byte datagram does not fit
`0x31` in an empty packet: `1 + 2 + 1169 = 1172 > 1170`. **A build that only ever
emits `0x31` cannot send a maximum-size datagram at all** — and the failure is
silent, because the datagram simply never fits and is eventually evicted. This
also makes `Frame::extends_to_end` live for the first time: slither has never
emitted an extends-to-end frame (C5).

⚠ **How many datagrams per packet, and in which order relative to the STREAM
fill, is unstated — §6 U-1, §8 Q3.** Recommendation there: **at most one
datagram per packet, packed before the stream fill.**

### 4.3 §9.8's auto-managed send — `send_message`

**The rule.** Allocate the next outbound uni stream, write the whole payload, set
FIN, GC the stream when the FIN'd range is fully acknowledged. No handle
surfaces.

**The steps, in the core:**
1. `msg.len() as u64 > MESSAGE_RECV_MAX` → `Err(MessageError::TooLarge)`. (The
   shell checks first; the core re-checks because its own unit tests call it
   directly.)
2. `self.lost()?` → `Err(MessageError::ConnectionLost(_))`.
3. `self.streams.open(Dir::Uni, &self.flow)` → on `StreamsExhausted`, **nothing
   has happened**; report *not now*. ⚠ Q1.
4. Admit the whole payload. ⚠ Q1 decides whether this can partially fail.
5. `finish(r)` — pins the final size.
6. `pump(now)`.

**GC.** §9.7's `DataRecvd` already frees a send half when every byte and the FIN
are acknowledged; `streams.rs:481` emits `ConnEvent::StreamFinished { r }` there.
Nothing new is required — **but the stream has no handle**, which is where §9 R4
lives.

**The one thing that must be true and is easy to get wrong:** the message
stream's bytes are inside `Connection::acked()`'s snapshot
(`AckSnapshot(streams.send_offsets())`, `mod.rs:657-659`) automatically, because
the snapshot iterates the stream table and not a handle list. That is what makes
`send_message(m).await; acked().await; close().await` — §16.2's own idiom and
Appendix B's obligation at SPEC.md:6238-6241 — work. **Verify it; do not assume
it.**

### 4.4 §9.8's claim — `recv_message`

**The rule.** Each incoming uni stream is one message; surface the payload only
when reassembly is complete (**FIN and all bytes**), then free the stream.
Surface **the oldest fully-reassembled unclaimed uni stream**.

**"Oldest" is settled: ruling 112.** *"FIFO in open order matches
`recv_message`'s 'oldest complete unclaimed'"* (rulings.md:2597-2604). So: scan
`unclaimed[Dir::Uni]` **in queue order** and take the **first complete** one —
not the first-completed one. Two messages where the second completes first
therefore surface **second-completed-first only if it is earlier in open order**;
a complete stream behind an incomplete one **is** surfaced. Messages are
unordered by contract, and this is what "reliable-**unordered**" means.

**The steps:**
1. Run §4.5's overflow scan (**every call**, `Some` or `None`).
2. Walk `unclaimed[Dir::Uni]` in order; take the first `r` whose receive half has
   a pinned final size and has received every byte to it.
3. Drain its payload, remove `r` from `unclaimed`, **retire the receive half**
   (`Streams::retire_recv`, `streams.rs:1094-1117`) — which trues up connection
   credit (§10.3's *"surfaced as a message (§9.8)"* trigger, SPEC.md:3294) and,
   via full closure, owes the peer +1 MAX_STREAMS_UNI (§10.4).
4. Clear the pending-claim flag; `pump(now)` — **this is why `now` is needed**
   (§7 C-1, §8 Q2).
5. If step 2 found nothing: **set** the pending-claim flag, return `None`.

**`earns_stream_credit` (C3) is very likely vacuous, and the contract should say
so rather than leave an implementer to invent a mode flag.** Credit advance is
read-driven (§10.3: *"Consumption, not arrival, drives credit: an unread buffer
earns nothing"*) and reaches `take_grant()` only through `Streams::read`
(`streams.rs:414-425`). A message stream is **never `read()`** — `recv_message`
drains it wholesale and retires it — and an unclaimed stream has no reader at
all. So no unclaimed or message-consumed uni stream can earn stream-level credit
**whatever the flag says**, and §9.8's *"the bound is structural"* holds by
construction. See §8 Q4 for whether to set it anyway.

### 4.5 The overflow scan and its guard — the heart of S30

**The guard (§9.8:3177-3188).** The check runs **while a `recv_message()` claim
is pending, and at the instant such a claim is made.** It applies to **every**
unclaimed window-full stream, **not merely the oldest**. Streams claimed by
`accept_uni()` are **untouched**.

**The state:** one `bool` — `message_claim_pending` — on `Connection` (or
`Streams`). Recommended lifecycle, and it is a **decision, not a reading**:
- set `true` by a `recv_message()` that returns `None`;
- set `false` by a `recv_message()` that returns `Some`;
- **never cleared by a dropped future** — the core cannot observe that, and does
  not need to: once an application has demonstrated message mode, ruling 51 says
  a concurrent `accept_uni()` user is committing the programming error.
The maintainer's own mechanism (rulings.md:801-803) is *"a receiver looping on
`recv_message()` always has a claim pending"* — that is satisfied by this
lifecycle, and working rule 11 says to check the mechanism exists: it does, as
the shell's parked `poll_recv_message`. ⚠ The **clearing** rule is unstated —
§6 U-4, §8 Q6.

**The two trigger points:**
1. inside `recv_message()`, before the claim walk;
2. on inbound stream data, **only if the flag is set** — the natural place is
   `Streams::on_stream_frame` (`streams.rs:552`) after the receive half updates.

**The predicate — ⚠ this is the highest-risk line in the slice (§6 U-2, §8 Q7).**
The candidate is: `r ∈ unclaimed[Uni]` **and** the receive half has no pinned
final size **and** its highest received offset has reached `MESSAGE_RECV_MAX`.
The hazard: a **legitimate 262 144-byte message whose FIN is in a later packet**
satisfies that predicate for one RTT and is reset. See §8 Q7.

**The emission, and why it cannot reuse `ResetState`:**
1. compute the wire `StreamId` **before** freeing anything — `stream_id(r)` reads
   `entries` (`streams.rs:319-324`) and the entry is about to go;
2. **retire the receive half at once** (§9.8:3150) — bytes count as consumed for
   connection-level credit (§10.3);
3. insert `{ id, error_code: MESSAGE_OVERFLOW, final_size: highest received
   offset (§9.6:3021 — informational), pending: true, acked: false }` into a
   **connection-level** retained set, new state beside `Regenerate`
   (`streams.rs:147-152`);
4. **trace** — `slither::frames`, WARN, naming **the stream, its final size, and
   the mode conflict** (ruling 59 is a **MUST**; S30's fourth `Accepts` bullet
   repeats all three fields);
5. pack it in **stage 2 (control)**, alongside the existing RESET_STREAM path
   (`streams.rs:764-796`), re-owing on a full packet exactly as that path does;
6. terminate on **acknowledgement only**. §8.7:2862-2867: *"the discard
   termination never applies to it."*

**Why not `ResetState` (`send.rs:65-72`):** it lives on a `SendHalf`, it is
terminated by `is_terminal()` → `after_half_freed` → entry removal
(`send.rs:200-208`, `streams.rs:511-514`), and `send.rs:59-64` already records the
decision verbatim — *"These are deliberately not one structure."* A peer-opened
uni stream **has no send half at all** (`Stream::new(dir, i, false)`), so there is
no `ResetState` to put it in even if one wanted to.

**Loss/ack routing.** `SentFrame::ResetStream { r }` (`recovery.rs:76-112`) is
reusable — the `StreamRef` stays a valid key into the **retained set** after
`entries` no longer holds it. `Streams::on_reset_acked` (`streams.rs:503-515`)
and `on_reset_lost` (`streams.rs:939-944`) each need a branch: *if the stream is
gone, look in the retained set.* An implementer who only edits the send-half path
ships a reset that is never re-emitted, and Appendix B's lost-reset obligation
(SPEC.md:6022-6024) is the test that catches it.

### 4.6 Liveness marking — small, cross-cutting, easy to miss

§7.4:1953-1954: **`seal` marks `last_send` for packets carrying a
first-transmission STREAM frame *or DATAGRAM frame*.** `pump_packets` computes
`marking` from `self.streams.fill(...)` (`mod.rs:1204`); slice 6 must **OR in**
"this packet carried a datagram". Every DATAGRAM is a first transmission (its
class is `never`), so there is no retransmission case to exclude. A build that
seals datagram packets with `seal_quiet` sends keepalives it does not owe — and
that is a testable difference (§5, T-D7).

---

## §5. Story-to-test mapping

### 5.0 Three rules that bind every test in this slice

**(a) Working rule 9 — a bound is only a test if the degenerate case violates
it.** Every entry below states **what the broken build does**. Two entries below
are marked **WEAK** because the honest answer is that the degenerate build passes
them; they are kept as regression pins and labelled, not dressed up.

**(b) Delivery is assertable only over `FlakyPolicy::perfect()`.** `testutil` is
deterministic by MUST (module doc, ruling 60: seeded RNG, fixed draw order,
ordered maps, no OS-entropy path), so over a perfect wire "the datagram arrived"
is a *fact*, not a probability, and asserting it is not a flake. **Any test that
needs something *not* to arrive uses `FlakyPolicy::drop_at` / `drop_first` /
`Network::block_path`** — index-based and RNG-free. **Never `lossy()`**: ruling
148 found it invisible to every public counter (`Network::sends()` counts before
policy; `Tap` records before loss is applied), so a loss-rate test cannot prove
a datagram died. `testutil`'s own module doc says the same at `:40-43`.

**(c) An assertion a *conforming* build can fail is a flake, not a pin.** §8.5's
fill quantum is implementation-defined and coalescing is many-to-many, so **no
test asserts a packet count** where a payload assertion will do. `min_packets`
(`story_reliability.rs:258`) is the existing precedent for a one-sided bound
where a count is unavoidable.

---

### 5.1 S15 — a user can send an unreliable datagram — **6-test-dgram**

*Accepts (STORIES.md:265-268): `send_datagram` never waits — it drops oldest
under pressure rather than blocking — and delivery is unordered and unreliable by
contract. Oversize input is `DatagramError::TooLarge`, not a silent truncation.*

#### Core, `src/core/connection/tests_datagram.rs`

| # | Test | What the **broken build** does |
|---|---|---|
| D1 | **drop-oldest, from both sides.** Pre-install core (§16.9 — nothing seals without a session), `send_datagram` × 80, then install and drain. Assert the **first** payload emitted is #17 **and** the **last** is #80, and `datagram_drops().send == 16`. | An **unbounded** queue emits #1 first → fails on the first assertion. A **reject-newest** queue emits #64 last → fails on the second. *Both* degenerate directions are separated, which "≤ 64 were sent" would not do. |
| D2 | **oversize is rejected before any queue.** Fill to 64, then `send_datagram(&[0; 1170])` → `Err(TooLarge)`; assert the queue still begins at #1 and `drops.send == 0`. | A build that pushes then validates has already evicted #1 → fails. This is §11.4's *"before any queue"* made observable. |
| D3 | **the bound, from both sides.** 1168 → `Ok`, 1169 → `Ok`, 1170 → `Err(TooLarge)`. | An off-by-one either way fails. Slice 1's lesson: test `LEN+1`, not only `LEN` and `LEN-1`. |
| D4 | **a maximum-size datagram uses the `0x30` form.** Send 1169 B into an empty packet; `parse_frames` yields `Wire::Datagram { had_len: false }` and the plaintext is exactly 1170 B. | A build that only emits `0x31` **never emits the datagram at all** (1172 > 1170) — the test fails by absence, and without D4 that failure is silent for ever. This is §4.2's pin. |
| D5 | **`0x30` is final.** After packing a `0x30`, `packing.frames()` ends with it and a further `fill`/`ping` push is refused. **WEAK** | `Packing.extends_to_end` already refuses (`frame.rs:843-865`), so **any** build using `Packing` passes. Kept as a regression pin against someone bypassing `Packing`; labelled so nobody counts it as coverage. See §6 U-3 — §8.4's *receiver*-side "`0x30` not final" error is **unrepresentable**. |
| D6 | **the receive queue drops oldest and counts.** Feed 65 DATAGRAM frames without claiming; `recv_datagram()` yields #2 first; `drops.recv == 1`. | An unbounded recv queue yields #1 → fails. |
| D7 | **a datagram marks liveness (§7.4).** Send a datagram; assert the keepalive deadline was deferred (`liveness()`, `mod.rs:212`). | A build sealing datagram packets with `seal_quiet` does not defer → a keepalive fires that is not owed → fails. |
| D8 | **a lost datagram is never retransmitted (§8.7 `never`).** Send one datagram, declare its packet lost, drive a PTO; assert **no** second `Wire::Datagram` appears. | A build that minted a `SentFrame::Datagram` and re-queued it emits a second → fails. This is the pin that CONTRACT-5b §2.7's prohibition is real and not merely written down. |
| D9 | **flow-control exempt (§10.7).** N datagrams change neither `send_charged` nor `recv_charged`. | A build charging them like stream bytes fails; and would eventually stall the connection at `INITIAL_MAX_DATA` with no error — the exact silent wedge §10.7 exists to prevent. |
| D10 | **congestion-controlled (§14.5).** With the window closed, `send_datagram` **queues** instead of sealing, and the packet's `size` is in `bytes_in_flight`. | A build exempting datagrams from the gate seals beyond cwnd → `bytes_in_flight` exceeds `congestion_window()` → fails. |
| D11 | **§16.9 early sends.** 80 datagrams before install; after install, 64 pump out and 16 were evicted. | A build that discards pre-install datagrams delivers none → fails. |
| D12 | **a zero-length datagram is a datagram.** `send_datagram(&[])` → `Ok`, and the peer's `recv_datagram()` yields `Some(vec![])`. | A build treating empty as a no-op delivers nothing → fails; a build treating `None`/empty alike hangs the receiver. |

#### Integration, `tests/story_datagram.rs`

| # | Test | What the **broken build** does |
|---|---|---|
| SD1 | **`send_datagram` never waits.** Inside `local()`, with A→B blocked, 200 calls all return `Ok(())` with no `.await`. Partly a compile-time pin (the verb is `fn`, not `async fn`). | An `async` or blocking verb does not compile against the test, which is the loudest possible failure. |
| SD2 | **oversize is an error, not a truncation.** 1170 → `Err(TooLarge)`; then 1169 over a **perfect** wire and B's `recv_datagram()` yields **exactly** 1169 bytes. | A build that truncates to 1169 and sends returns `Ok` → the first assertion fails. Without the second half, a build that rejects *everything* passes. |
| SD3 | **drop-oldest, weak form.** After a burst larger than the queue, B receives a **suffix** of the burst and never a stale head. | A FIFO-reject build delivers the head and drops the tail → fails. **Deliberately weaker than D1**: the strong form needs the send queue to back up, which requires closing the congestion window, and working rule 13 says to name that rather than build a fragile fixture. The exact-eviction pin lives at D1. |
| SD4 | **unreliable and unordered by contract.** `drop_at([2])`, five datagrams; B receives four; drive past 3 × PTO and assert **no fifth arrival**. | A build retransmitting datagrams delivers all five → fails. Uses `drop_at`, **not** `lossy()` (ruling 148). |
| SD5 | **`recv_datagram` is cancel-safe.** Park it, drop the future, then send; the next call yields the datagram. | A build that claims on poll and discards on drop loses it → fails. |
| SD6 | **post-death drain.** **Ruled: ruling 152** — ships **live**, not `#[ignore]`d. `recv_datagram` drains what arrived before the death, then reports it; it never parks with the latch set. | A build applying ruling 128's short two-verb list refuses a datagram that had already arrived. |

---

### 5.2 S16 — a user can send a single-shot message — **6-test-msg**

*Accepts (STORIES.md:273-276): a message up to `MESSAGE_RECV_MAX` (262 144) in
one call; larger is `MessageError::TooLarge`. **bubble-engine chunks above this
and cares about the exact bound.** A datagram's own bound is separate: 1169.*
**S16 carries no paused-clock marker** — the only one of the three without it.
The bound tests are clock-free; SM3 needs the paused clock anyway.

#### Core, `src/core/connection/tests_message.rs`

| # | Test | What the **broken build** does |
|---|---|---|
| M1 | **a message is one uni stream with a FIN.** `send_message(now, b"hi")` produces exactly one `Wire::Stream { fin: true }` on a fresh uni id — and **no** new frame type. | A second reliability path (a DATA frame) or a FIN-less stream fails. §9.8's *"there is no DATA frame type"*. |
| M2 | **exactly-once surfacing.** Deliver the message stream's packet twice; `recv_message()` yields it once, the second call `None`. | A build without §9.2's closed-stream watermark surfaces it twice → fails. |
| M3 | **completeness is FIN *and* all bytes.** Deliver bytes without the FIN → `None`; deliver a FIN with a gap → `None`; fill the gap → `Some`. | A build surfacing on FIN alone, or on any data, returns `Some` early → fails. |
| M4 | **the bound, from both sides.** 262 143 → `Ok`, 262 144 → `Ok`, 262 145 → `Err(TooLarge)`. | Off-by-one either way fails. S16 says bubble-engine *cares about the exact bound*, so this is a consumer-visible pin. |
| M5 | **oldest complete unclaimed, in open order** (ruling 112). Open uni #1 and #2; complete #2, then complete #1, **then** call `recv_message()` twice. Assert **#1's payload first**. | A completion-order queue returns #2 first → fails. The naive version (complete #2, claim, complete #1, claim) **cannot separate them** and would be a name without a pin. |
| M6 | **retention: credit trues up at the claim, not at the FIN.** After the message is fully reassembled but **unclaimed**, connection consumed-credit is unchanged; after `recv_message()`, it advances by the payload. | A build retiring at FIN advances early → fails. This pins §10.6's *"payloads stay accounted inside the core until the handle takes them"*. |
| M7 | **claims pay for the next message.** Claim 8 messages → a `Wire::MaxStreamsUni` appears (§10.4's batch of 8). | A build granting on *our own* stream closures inflates the peer's allowance (RFC 9000 §4.6's scope error) → the frame appears at the wrong time → fails. |
| M8 | **an empty message is a message.** `send_message(b"")` → peer yields `Some(vec![])`. | A build returning `None` parks the receiver on a delivered message for ever. |
| M9 | **a sugar stream never earns stream-level credit (§10.3).** Deliver a full 262 144-byte message and claim it; assert **no `Wire::MaxStreamData`** for that id, and that `Wire::MaxData` **does** appear. **WEAK-by-construction** | Passes for free today — credit is read-driven and a message stream is never `read()` (§4.4). Kept as the regression pin against a future mode flag set the wrong way, and labelled so it is not counted as coverage of a mechanism that is not there. |
| M10 | **a handle-less message stream is inside `acked()`'s snapshot.** `ack_snapshot()` taken after `send_message` contains the message's `StreamRef`; `snapshot_settled` flips only when its bytes are acknowledged. | A snapshot built from *handles* omits it → `acked()` returns immediately → SM3's whole point evaporates, silently. |
| M11 | **`send_message` is atomic under Q1's answer.** A `send_message` the core refuses (no stream allowance) has opened **no** stream and consumed **no** payload; the next successful call uses the same next index. | A build that opens the stream and *then* discovers it cannot proceed leaks a stream index and, worse, a FIN-less stream on the wire. |

#### Integration, `tests/story_message.rs` (S16 half)

| # | Test | What the **broken build** does |
|---|---|---|
| SM1 | **262 144 in one call.** Over a perfect wire, `send_message` of 262 144 bytes; B's `recv_message()` yields all of them, byte-identical. | A build chunking at a lower bound delivers a short message or several → fails. |
| SM2 | **larger is `TooLarge`, and nothing is sent.** 262 145 → `Err(MessageError::TooLarge)`, and the `Tap` shows **no** new packet. | A truncate-and-send build returns `Ok` → fails; a build that rejects *after* opening the stream leaks a stream → the tap assertion fails. |
| SM3 | **ruling 47's message-then-close** (Appendix B:6238-6241). With deterministic drops armed, `send_message(m).await; acked().await; close(NO_ERROR, "").await`, drop every handle; B receives `m` **in full**. **Plus the negative**: the same sequence *without* `acked()` **fails** the same assertion at the same drop set. | Without the negative, a build that never loses anything passes for free — Appendix B says *"run it enough times that the pre-ruling-47 ordering fails the same assertion"*, and `drop_at` makes "enough times" exactly once. |

---

### 5.3 S30 — mixing messages with uni streams fails loudly — **6-test-msg**

Appendix B (SPEC.md:6025-6039) writes this story as an integration test and names
five assertions. All five are below; **none of them is optional**, and the third
and fourth are the ones a plausible build gets wrong.

| # | Test | What the **broken build** does |
|---|---|---|
| SM4 | **the mixing shape, end to end.** A `open_uni()` and writes past 262 144; B **only ever** calls `recv_message()`. Assert (i) A's write resolves `Err(WriteError::Reset(MESSAGE_OVERFLOW))`; (ii) the code is **`0x06`, not `0`**; (iii) B's connection credit trues up — B can still receive 262 144 further bytes on another stream afterwards; (iv) `closed()` never resolves and the clock advanced past `KEEPALIVE_TIMEOUT`. | **Pre-ruling-51**: A stalls for ever, keepalives flowing — assertion (i) hangs, which is why the test needs a virtual-time `within()` bound or it wedges the suite. **Pre-ruling-52**: the code is `0` — (ii) fails, and (ii) is the *whole* of ruling 52. **No true-up**: (iii) fails and the connection wedges at `INITIAL_MAX_DATA` with no error. **(iv)** is what stops the test from passing for the wrong reason: a build that killed the connection on liveness would also resolve A's write in error. |
| SM5 | **the not-oldest case** (Appendix B: *"Assert the not-oldest case too"*). Two `open_uni()` streams; the **older** stays small and incomplete, the **newer** fills the window. Assert the **newer** is reset on its own account. | A build written as `if let Some(oldest) = unclaimed.front()` leaves the newer stalled → its write never resolves → fails. §9.8:3181-3182 is explicit: *"every unclaimed window-full stream, not merely the oldest"*, and rulings.md:801-802 records that the ratified "oldest" restriction was **dropped** for this reason. |
| SM6 | **the negative — what the guard buys.** B uses `accept_uni()` **only** and is slow to call it. A's window-full stream is **not** reset; when the accept lands, credit extends and A's write **resumes**. | The **unguarded** build — the blanket form ruling 51's applying agent refused, and was right to (rulings.md:793-800) — resets it, so A gets `Err(Reset)` where it should get progress. Without SM6 the unguarded build passes every other test in this slice. |
| SM7 | **the lost reset** (Appendix B:6022-6024). Drop the packet carrying the first RESET_STREAM (`drop_at` on the receiver's send index); assert A's write **still** resolves `Err(Reset(MESSAGE_OVERFLOW))` after retransmission. | A build that put the reset in the send-half `ResetState` loses it with the retired half → A hangs for ever. §8.7:2862-2867 and §9.6:3028-3039 exist for exactly this, and this is the only test that reaches it. **Must be wrapped in a virtual-time bound**, or the failure mode is a hung suite rather than a red test. |
| SM8 | **the receiver traces it** (ruling 59, S30's fourth `Accepts`, a **MUST**). Capture `slither::frames`; assert one record naming **the stream, its final size, and the mode conflict**. | A build that resets silently leaves the only end that can fix the bug with no evidence — which is the entire content of ruling 59. ⚠ **No tracing-capture helper exists in `testutil`** — §9 R2. If none is provided the obligation is untested, and an untested MUST is not a thing to discover at the release gate. |
| SM9 | **`send_message` is cancel-safe.** Drop a `send_message` future before it resolves; the `Tap` shows **no** uni stream opened, and a later `send_message` succeeds. | A shell that decomposes into open → write-loop → finish leaves a **FIN-less half-written uni stream**, which against a message-mode receiver is §9.8's overflow case — the application's own `select!` timeout manufactures S30's failure. This is the test that pins §2.5's cancel-safety sentence, and it is the one a blind author will not write unless the contract says so. |

---

## §6. The unstated-scope hunt (working rule 8)

*A stated construction with an unstated or contradicted scope. ~19 instances
across six slices, never once a wrong value. Nine here; the three that decide
what gets built are U-1, U-2 and U-4.*

**U-1 — §8.5's stage 3 names two contributors and orders neither.**
*"then STREAM and DATAGRAM fill"* (SPEC.md:2830). The round-robin sentence that
follows is explicitly scoped — *"**Within the STREAM fill**, streams with pending
data are served round-robin"* — so it says nothing about DATAGRAM. **Unstated:**
(a) whether datagrams precede or follow the stream fill; (b) **how many
datagrams may share one packet**; (c) whether a datagram participates in the
round-robin. Every reading is wire-legal, and they differ by which side starves:
streams-first starves a datagram queue behind a saturated bulk stream (and the
starved datagrams are then *evicted*, so the failure is silent data loss);
drain-all-datagrams-first starves the stream. → §8 Q3.

**U-2 — §9.8 names the overflow trigger and never says which quantity reaches
it.** *"an unclaimed uni stream … that **reaches `MESSAGE_RECV_MAX`** is reset"*
(SPEC.md:3145-3147), and two lines earlier *"consumes its full initial window
without pinning a final size"* (3133-3134). **Unstated:** highest *received*
offset, contiguous *reassembled* prefix, or *advertised-credit consumed*. These
diverge under loss, and one reading resets a stream whose FIN is still in flight.
→ §8 Q7. This is the highest-cost gap in the slice.

**U-3 — §8.4 states a receiver error that no receiver can observe.** The layout
is `type(0x30) ‖ data(**to the end of the plaintext**)`; the error list two lines
below is *"Structural errors: `length` overrunning the plaintext; **a `0x30`
frame that is not the packet's final frame**"* (SPEC.md:2815-2822). A `0x30`
frame consumes the remainder **by definition**, so a following frame is
absorbed into its data and is unobservable. The stated error case is
**unreachable on receive**; it is a *sender* prohibition wearing a receiver's
clothes. Consequence for slice 6: test D5 is weak by construction, and no
receiver-side rejection can be written. → §7 C-6.

**U-4 — "while a `recv_message()` claim is pending" never says what sets or
clears "pending".** SPEC.md:3177-3179 gives a two-part trigger — *"while a claim
is pending, **and at the instant such a claim is made**"* — which implies the
state can be **not** pending, but no text says when it stops. rulings.md:801-803
supplies the mechanism informally (*"a receiver looping on `recv_message()`
always has a claim pending"*) and no more. The choice is behaviourally visible:
under *cleared-by-a-successful-claim*, a receiver that always has a message ready
never runs the check; under *latched-for-ever*, a receiver that called
`recv_message()` once and then switched to `accept_uni()` gets its streams reset.
→ §8 Q6.

**U-5 — §16.4's core API list omits a parameter, exactly as ruling 71's did.**
`fn recv_message(&mut self) -> Option<Vec<u8>>` (SPEC.md:4847) has no `now`, but
the verb retires a receive half (owing MAX_DATA, §10.3:3291) and may emit
RESET_STREAM (§9.8:3151). Ruling 71 is the precedent: §16.4's core API omitted
the stage-0 accessors and *"the omission was invisible until someone built
against it."* → §7 C-1, §8 Q2.

**U-6 — §16.2's post-death paragraph lists two verbs and slice 6 adds two more.**
Ruling 128's rule (SPEC.md:4359-4368) is written as *"`read` serves the buffered
bytes … and `accept_bi`/`accept_uni` hand over streams opened before the
death"*. `recv_message` and `recv_datagram` did not exist when it was written and
are not named. Read as exhaustive — which working rule 8 says a list is —
`recv_message()` reports `ConnectionLost` at the latch, and **Appendix B's own
message-then-close obligation fails**. → §7 C-2, §8 Q5.

**U-7 — the two new `ConnEvent`s have no emission discipline, and the one they
resemble needed a ruling to get one.** §16.4:4867-4868 lists `MessageReadable`
and `DatagramReadable` as signals and says nothing about *how many*.
`StreamOpened` had the identical hole and it took **ruling 99** to fix it at
*"one per newly-opened stream."* Unstated here: one per completed message or one
per arrival burst; whether an eviction-causing datagram still signals; whether a
signal fires for a stream already claimed by `accept_uni()`. §2.4 decides all
three by contract — recorded here so the decision is visible as a decision.

**U-8 — §11.5 says "a counter", §11.3 and §18.2 say "the counters".**
SPEC.md:3546-3548 vs 3529-3530 and 5694; Appendix B:6039 says "the drop counter".
Unstated: one shared counter or one per queue, and whether either is ever reset.
→ §7 C-4, §8 Q8.

**U-9 — nothing says what happens to a queued datagram at close or at roam.**
§11.3 bounds the queues; §15.2 lets `close()` drop state immediately; §14.6's
roam reset seam enumerates congestion and recovery state and does not mention the
datagram queues. Derivable (a queued datagram is discarded at close — §11.1
promises nothing) and roam is slice 7's, but the derivation is not written down
and a blind implementer may instead try to flush the queue into the CLOSE packet.
State it in the contract: **`close()` discards the send queue; nothing is
flushed.**

---

## §7. Conflicts — reported, NOT resolved (working rule 3)

*The prose has held the correct intent and the formal rule the bug repeatedly in
this project. I am not defaulting to the code-like rule in any of these. Both
sides, with line numbers.*

> **Bookkeeping note, working rule 4's shape.** My brief says *"**Five times** …
> most recently **ruling 131**"*; `CLAUDE.md:105-108` still says *"**Three
> times** … most recently **ruling 69**"*. The rule's own count and exemplar were
> not swept when the later instances landed. Reported, not edited — `CLAUDE.md`
> is not on this slice's path.

### C-1 — `recv_message`'s signature cannot do what `recv_message`'s job is

- **The formal rule.** SPEC.md:4847, §16.4's core API:
  `fn recv_message(&mut self) -> Option<Vec<u8>>;` — no `now`. The list is read as
  exhaustive (working rule 8), and the sibling verb `fn accept(&mut self, dir: Dir)`
  (`:4846`) is correctly `now`-free.
- **The prose.** §9.8:3150-3151 — the overflow check runs *"at the instant such a
  claim is made"* and **emits RESET_STREAM**. §10.3:3291-3295 — surfacing a
  message **retires** the receive half, which advances connection credit and owes
  MAX_DATA. §16.7 — sealing is synchronous, inside the mutating call.
- **The code.** `mod.rs:419-421`: *"`poll_output` takes no `now` and seals
  nothing: §16.7 puts sealing inside the mutating call that triggers it."*
  `Connection` has **no `Instant` field** (`mod.rs:102-137`), and no
  `Instant::now()` exists in non-test `src/core/`. `abandon_recv` takes `now`
  (`mod.rs:574`) for the *weaker* of `recv_message`'s two reasons — retirement
  alone, with no frame to emit.
- **Why it matters.** Without `now`, a `recv_message()` that triggers the
  overflow reset cannot seal it; the frame waits for the next `now`-bearing call,
  which on an otherwise idle connection is a timer — a delay of seconds on the
  one path S30 exists to make prompt. The credit true-up has the same shape.
- **Not resolved here.** Adding `now` edits a ratified signature.

### C-2 — Appendix B requires a post-death drain that ruling 128's rule does not name

- **The formal rule.** SPEC.md:4359-4368 (ruling 128): *"While the connection
  still holds a stream's received state, **`read`** serves the buffered bytes and
  then the FIN's `Ok(None)`, and **`accept_bi`/`accept_uni`** hand over streams
  opened before the death; when nothing is left, both report `ConnectionLost`."*
  Two verbs, named. rulings.md:3222-3229 repeats the same two.
- **The prose that contradicts it.** SPEC.md:6238-6241, Appendix B, ratified:
  *"on endpoint A: `send_message(m).await`, then `acked().await`, then
  `close(NO_ERROR, "").await`, **then drop every handle**. Assert endpoint B
  **receives `m` in full** from `recv_message()`."* B's driver processes the data
  and the CLOSE in one pass — ruling 128's own worked example — so B's
  `recv_message()` runs **after** the latch is set. Under the two-verb reading it
  answers `Err(PeerClosed)` and the obligation is unsatisfiable.
- **Extra weight.** Ruling 128's own reasoning is *"the most natural sender
  pattern in the protocol delivered nothing usable"*, and §16.2:4442 names
  `send_message(msg); acked(); close()` as **the** idiom the `acked()` verb
  exists for. The rule's rationale describes messages; its enumeration omits them.
- **Not resolved here.** `recv_datagram`'s membership is a separate and weaker
  question (§8 Q5).

### C-3 — §10.3 lists five retirement triggers; §9.7 lists three, and the fifth is not among them

- SPEC.md:3292-3295, §10.3: *"When a receive half is retired for any reason —
  read to its final size, reset observed, handle abandoned (§16.2), surfaced as a
  message (§9.8), or **final size reached with no reader (§9.7)** — …"*
- SPEC.md:3045-3052, §9.7, the cited section: a receive half frees on **three**
  triggers — `DataRead`, `ResetRead`, or abandonment of a handle. *"Final size
  reached with no reader"* **is not there.**
- **Why it is not cosmetic.** If §10.3's fifth trigger is real, a complete but
  **unclaimed** message stream retires — and trues up connection credit — at the
  FIN, before `recv_message()` claims it. That contradicts §10.6:3477-3481
  (*"message and datagram payloads stay accounted inside the core … **until the
  handle takes them**"*) and §16.4:4968-4973's backpressure-by-retention
  (*"a reassembled-never-claimed uni stream holds its stream state and its
  MAX_STREAMS credit until claimed"*). It also decides test M6 outright.
- **Three statements, at most two of which can hold.** Reported, not picked.

### C-4 — the datagram drop counter is singular in one place and plural in three

- §11.5:3546-3548: *"increments **a counter** … the **counters** are core state"*
  — singular and plural in one sentence.
- §11.3:3529-3530: *"the drop **counters** live in the connection core"*.
- §18.2:5694: *"the datagram queue-overflow drop **counters**"*.
- Appendix B:6038-6039: *"the drop **counter** emitted on `slither::frames`"*.
- Low cost, but it is a stated construction with a contradicted scope, and §6's
  finding is that this shape is never harmless twice running. → Q8.

### C-5 — "no true-up in this direction" and "its bytes count as consumed" read as a contradiction and are not one

- §9.6:3026-3027: *"**No flow-control true-up runs in this direction** — the
  frame releases the *sender's* obligation, not the receiver's credit."*
- §9.8:3150-3151: *"The receive half retires (**its bytes count as consumed at
  the connection level**, §10.3)"*.
- Appendix B:6021 asserts *"connection credit trues up"* **without saying whose**.
- **My reading — flagged as a reading, not a ruling:** they are two different ends
  of the wire. The *emitting receiver* retires its own half and advances its own
  MAX_DATA (§9.8); the *stream's sender*, receiving the RESET_STREAM, runs **no**
  true-up because it holds no receive half for that stream (§9.6). Both are true
  simultaneously. **Reported anyway** because a blind implementer reading only
  §9.6 will suppress the §9.8 true-up, and the resulting wedge — the receiver's
  MAX_DATA never advancing past 262 144 of stranded bytes — is silent. Appendix
  B's unqualified sentence is the one that should gain the missing word.

### C-6 — §8.4's `0x30`-not-final structural error is unreachable

- SPEC.md:2815: `type(0x30) ‖ data(**to the end of the plaintext**)  — must be final`.
- SPEC.md:2821-2822: *"Structural errors: `length` overrunning the plaintext; **a
  `0x30` frame that is not the packet's final frame**."*
- The first makes the second unobservable: a `0x30` frame consumes the remainder
  by definition, so any "following frame" is absorbed into its payload. A
  conforming receiver **cannot** detect the condition, and the ¬LEN STREAM form
  has the same property — `frame.rs:298-306` already consumes `pt.len() - at`
  with no positional check and no error.
- Reported rather than resolved: either the error is a sender-side MUST that
  §8.4 has filed under the receiver's error list, or a length-bearing check is
  intended that the layout does not permit.

---

## §8. Open questions, ranked by cost of a wrong answer

*Eight. Questions I could answer from the spec are answered in §2/§4 with the
line cited, not listed here.*

---

### Q1 — What does `core::send_message` do when it cannot proceed, and what does the shell wait on? **(highest cost)**

§16.2:4284 says only *"`send_message` **waits for stream allowance**, then behaves
per §9.8"*, and §9.8:3082-3084 says it *"writes the whole payload, sets FIN"*.
But `core::write` is **credit-gated at admission** (`send.rs:226-248`:
`room = stream_room.min(conn_room).min(len)`, `Ok(0)` = blocked), and
`MessageError` is **exhaustive and already shipped** — `TooLarge` and
`ConnectionLost`, nothing else (`error.rs:264`, pinned by `spec_errors`). **There
is no representable "not now."**

| Candidate | Cost if wrong |
|---|---|
| **(a) Atomic. `send_message` succeeds only if stream allowance *and* whole-payload credit are available; otherwise nothing happens and it reports "not now" through a return type that is not a `MessageError` (e.g. `Result<bool, MessageError>` or a small `enum SendMessage { Sent, Blocked }`).** | Needs a **wake for "connection credit arrived"** that no `ConnEvent` provides today: `StreamWritable { r }` fires only for streams with a blocked writer (`streams.rs:690`, `:703`), and a pending `send_message` has no stream yet. Without one, a `send_message` blocked on connection credit **never wakes**. |
| **(b) Atomic on stream allowance only; the payload enters send state beyond credit, held as a core-side tail that drains as credit arrives** (quinn's shape, and the most literal reading of *"writes the whole payload"*). | Sender-side buffering becomes (outstanding uncredited message streams) × 256 KiB. With `INITIAL_MAX_STREAMS_UNI` = 128 that is **32 MiB**, and §17.5's state-ceiling table contains **no such term** — so this is a memory-bound change that needs its own ratification, not an implementation choice. |
| **(c) The shell decomposes: `open_uni` → write-loop → `finish`, with no core verb at all.** | **Not cancel-safe.** A dropped `send_message` future leaves a FIN-less, half-written uni stream on the wire — which against a message-mode receiver is precisely §9.8's overflow case. An application's own `timeout(d, send_message(..))` would then *manufacture* the failure S30 exists to diagnose. It also contradicts §16.4, which puts `send_message` on the core. |

**Recommendation: (a).** It is cancel-safe by construction (the claim and the
commit are one expression); it needs no new buffer and no change to `SendHalf`'s
admission model; and it leaves §17.5's numbers exactly as ratified, because
sender-side buffering stays bounded by the connection's own send credit. Its one
cost is the missing credit wake, and that cost is **payable and small**: the
cheapest form is to have the core, when it refuses for credit, remember that a
message send is waiting and emit the **existing** `ConnEvent::StreamsAvailable
{ dir: Uni }`-shaped signal when either constraint clears. Whether that reuses an
existing variant or mints one is the part I cannot decide — it touches §16.4's
ratified enum. **(b) is the runner-up and I would take it over (c) without
hesitation**, but only behind a §17.5 amendment.

*Working rule 5 note:* the brief did not ask me to change a signature, and I am
not doing so — I am reporting that §16.4's stated return type cannot express a
state §16.2's own prose requires.

---

### Q2 — Does `core::recv_message` take `now`?

The conflict is C-1. Candidates: **(i)** add `now: Instant`; **(ii)** keep the
ratified signature and let the reset and the credit grant ride the next
`now`-bearing call.

**Cost of (ii):** on an idle connection the next `now` comes from a timer, so
S30's reset is delayed by up to a keepalive interval — on the one path whose
entire purpose is to fail *promptly* instead of stalling. It also makes
`recv_message` the only retiring verb in the core that does not seal what it owes,
diverging from `abandon_recv` (`mod.rs:574`) for no stated reason.
**Cost of (i):** a one-parameter edit to a ratified list — the same edit ruling 71
made, for the same reason.

**Recommendation: (i).** And state the asymmetry in the contract in one sentence,
so nobody "fixes" it later: **`recv_datagram` takes no `now` and that is
correct** — datagrams are flow-control-exempt (§10.7), so claiming one emits
nothing and there is nothing to seal.

---

### Q3 — Datagram packing: how many per packet, and in which order relative to the stream fill?

U-1. Candidates: **(i)** at most one datagram per packet, packed **before** the
stream fill; **(ii)** drain the whole send queue first; **(iii)** datagrams after
the stream fill.

| | Cost if wrong |
|---|---|
| (ii) | 64 queued datagrams monopolise ~64 packets; a bulk stream stalls for an unbounded time under a datagram flood. |
| (iii) | A saturated stream fills all 1170 B of every packet, datagrams **never** go out, and the send queue evicts continuously — **silent data loss with no counter distinguishable from ordinary pressure**. This is the worst of the three and it is the one an implementer writes by accident, because appending after the existing `streams.fill(...)` call is the smallest diff. |
| (i) | A max-size datagram consumes the whole packet, delaying stream bytes — but by **at most one packet per datagram**, and the queue is bounded at 64, so the delay is bounded. |

**Recommendation: (i).** It is the only option whose starvation is bounded in
both directions, and the bound is statable: a stream is delayed by at most 64
packets; a datagram waits at most one packet per queued predecessor. Also fix the
type-code rule here (§4.2): `0x31` when it fits, `0x30` when only that fits,
defer otherwise — **without the `0x30` branch a maximum-size datagram can never
be sent at all** (C5).

---

### Q4 — Is `earns_stream_credit` set `false` for peer-opened uni receive halves?

The field already exists and is seeded `true` (`recv.rs:~72`, C3), placed there by
slice 4 for slice 6. Candidates: **(i)** leave it `true` everywhere and document
why §9.8's bound holds anyway; **(ii)** set it `false` at creation for peer-opened
uni halves and flip it `true` when `accept(Uni)` claims one.

**Answerable from the spec, and I am answering it: (i).** §10.3:3277-3278 —
*"Consumption, not arrival, drives credit: an unread buffer earns nothing"* — and
`take_grant()` is reachable **only** from `Streams::read` (`streams.rs:414-425`).
An unclaimed stream has no reader; a message stream is never `read()` at all
(`recv_message` drains it wholesale and retires it). So no unclaimed or
message-consumed uni half can earn stream-level credit under either setting, and
§9.8:3127-3129's *"the bound is structural"* holds **by construction**.
(ii) adds a flip with no spec text behind it and one more state to get wrong.

**Recommendation: (i)**, with the reasoning written into the contract and test M9
kept as a labelled regression pin (§5.2). *Working rule 12 check — what state does
this argument assume?* It assumes `take_grant` has exactly one caller. If slice 6
adds a second path that grants credit without a `read`, the argument dies with it;
so the contract must say **do not add one**.

---

### Q5 — Do `recv_message` and `recv_datagram` drain after the connection's death?

The conflict is C-2. For `recv_message` the answer is forced by Appendix
B:6238-6241 and I would treat it as already ruled in substance: **yes**. For
`recv_datagram` it is genuinely open — datagrams are unreliable by contract, so
nothing is promised.

**Cost:** for `recv_message`, getting it wrong breaks a **ratified Appendix B
obligation** and the idiom §16.2 names as `acked()`'s reason for existing. For
`recv_datagram`, getting it wrong loses at most a datagram nobody was promised.

**Recommendation: yes to both**, with the same precedence table (§2.5). Symmetry
costs nothing here and the asymmetry would be a trap for exactly the reason
ruling 128 exists: a receiver woken *after* the latch cannot win the race by
being prompt.

---

### Q6 — What clears the pending-claim flag?

U-4. Candidates: **(i)** cleared by a `recv_message()` that returns `Some`;
**(ii)** latched for ever after the first call; **(iii)** cleared when the shell's
future is dropped (**not implementable** — the core cannot observe a dropped
future, and §16.4 gives it no hook).

**Cost of (ii):** an application that calls `recv_message()` once, exploratively,
and then commits to `accept_uni()` has its window-full streams reset — the
unguarded behaviour that ruling 51's applying agent refused, reintroduced through
the back door. **Cost of (i):** a receiver that always has a message ready never
runs the scan, so a concurrently-overflowing stream waits until the receiver next
blocks. That is a delay, not a stall — the moment the receiver drains, it blocks
and the scan fires.

**Recommendation: (i).** It is the literal reading of *"while a claim is pending,
and at the instant such a claim is made"*, it matches the maintainer's own stated
mechanism (rulings.md:801-803), and its failure mode is bounded delay rather than
a reset the application did not earn.

---

### Q7 — Which quantity "reaches `MESSAGE_RECV_MAX`", and does an in-flight FIN race it?

U-2, and the highest-cost *implementation* question after Q1. Candidates for the
predicate: **(i)** highest received offset (`high_water`) `>= MESSAGE_RECV_MAX`
and no pinned final size; **(ii)** the contiguous reassembled prefix reaches it;
**(iii)** advertised stream credit is fully consumed.

- **(ii)** never fires when a byte in the middle was lost — precisely the case
  where the sender is stalled at the window and needs rescuing. It reintroduces
  ruling 51's permanent stall under loss.
- **(iii)** is the same as (i) in practice (`last_advertised` is seeded to the
  window and never moves for an unclaimed stream) but reads as a credit fact
  rather than a data fact, and would break the moment Q4's answer changed.
- **(i)** fires correctly under loss — **but appears to race a legitimate
  262 144-byte message whose FIN has not yet arrived.**

**The race is closable, and closing it is a requirement on `send_message`, not on
the predicate.** If `send_message` attaches the FIN to the **last data frame**
rather than emitting a separate empty FIN frame, then `high_water == 262 144`
implies that frame arrived, which implies the final size is pinned — and the
predicate can never fire on a well-formed maximum-size message. The residual case
is `open_uni()` + `write(262 144)` + `finish()` sending a separate FIN frame to a
message-mode receiver, and **that is the mixing error by ruling 51**, whose
defined loud failure is exactly this reset.

**Recommendation: (i), plus a contract requirement that the core's `send_message`
carries the FIN on its final data frame.** The second half is the part that will
be missed if it is not written down, and its absence is a **false positive on a
conforming application** — the worst failure class in this slice.

---

### Q8 — One drop counter or two?

C-4. **Recommendation: two** (`send` and `recv`). §11.3 and §18.2 both say
"counters"; §11.5's singular is inside a sentence that says "counters" four words
later; and one counter cannot answer the operator question §11.5 exists for
(*is my application over-producing, or is my peer over-sending?*). Cost either
way is a trace field, not behaviour.

---

## §9. Risk register

*Working rule 13: **the fixture bounds the coverage.** Two of slice 3b's four
seam-review findings were unreachable from all 451 tests by construction. The
first question below is therefore not "what might break" but **"what can slice
6's fixtures not express?"***

### What slice 6's fixtures cannot express

| Boundary | What it hides in slice 6 |
|---|---|
| **`Tap` yields sealed datagrams** (`testutil/mod.rs:86-91`: *"Deliberately just bytes: `testutil` never parses a packet"*) | **No integration test can count or classify a frame.** Every assertion about `0x30` vs `0x31`, about a datagram not being retransmitted, about the packing order, or about the reset's `final_size` field must be a **core** test through `testfix::parse_frames`. §5 is written that way; a test author who tries to do it from `tests/` will fail and may weaken the assertion instead of moving it. |
| **`FlakyPolicy::lossy()` is invisible to every public counter** (ruling 148; `Network::sends()` counts *before* policy, `Tap` records *before* loss is applied) | A loss-rate test cannot prove a datagram died. All of §5's negative-delivery tests use `drop_at`/`drop_first`/`block_path`. |
| **`FlakyWire` models a network, not a socket** | There is no way to express *"the core could not seal"*. That matters here because the **send queue only backs up when the congestion window closes** — there is no fixture for "the send path stalled". SD3 is therefore deliberately the weak form and D1 carries the real pin. |
| **`LocalSet::run_until` re-polls its body on any local-task wake** | **No test that awaits from the `local()` body can detect a lost wakeup.** SM6 (*"resumes when the accept lands"*), SD5 and the three new waker slots are all exposed: a build that forgot to wire `MessageReadable` to `message_readers` may still pass, woken incidentally by the driver. **Mitigation, and it should be in the briefs:** assert waker-map occupancy directly — `stream_waker_entries()` already exists for this (`connection.rs:627-630`, `#[cfg(test)]`) and should be extended to the three new slots — and use the existing manual `poll_once` helpers rather than `.await` for the wakeup assertions. |
| **No tracing capture exists anywhere in the tree** | See R2 — this is the largest hole. |

### Ranked risks

**R1 — the receiver's trace obligation is untestable with today's fixtures.**
Ruling 59 is a **MUST**, S30's fourth `Accepts` bullet repeats it, and §11.5's
drop counters are *only* surfaced through the trace (there is no public accessor,
and §16.2's accessor list is exhaustive). `testutil` has **no** subscriber-capture
helper and `Tap` is bytes-only. Without one, SM8 and the counter obligation are
unverified, and an unverified MUST discovered at the release gate is a blocked
release. **Action: add a `testutil::TraceCapture` (a `tracing` layer collecting
records by target), owned by `6-impl`, and land it PRE-DISPATCH with the testfix
arm** — `6-test-msg` cannot write SM8 against a helper that does not exist
(working rule 14, slice 4a's ten lost minutes).

**R2 — `finished_senders` grows without bound for handle-less message streams.**
`shared.rs:237`'s stated bound is *"written only while `blocked_ackers` holds
that stream's slot"*, and removal is `Drop for SendStream` (`stream.rs:548`). A
message stream has **no handle**, so it has no `blocked_ackers` entry and no
`Drop` to clean up after it. If `note_send_finished` inserts unconditionally,
a long-lived messaging connection accumulates one `BTreeSet<StreamRef>` entry per
message **for ever** — and §17.5's per-connection ceiling table has no term for
it. This is the single most likely *implementation* defect in the slice, it is
invisible to every functional test, and it lives in a file the shell implementer
will edit for other reasons. **Action: state the invariant in the contract and
have `6-test-msg` assert set size after N messages.**

**R3 — the overflow scan is a quadratic DoS in its obvious form.** §9.8 says the
check applies to *"**every** unclaimed window-full stream"*, and the natural
implementation scans `unclaimed[Uni]` on every `recv_message()` **and on every
inbound stream frame while a claim is pending**. `unclaimed` is bounded by the
**cumulative** MAX_STREAMS_UNI, not a concurrent count (§10.4:3352), so a peer
can accumulate unclaimed streams limited only by connection credit — a
one-byte-per-stream flood inside 1 MiB is a very long deque, scanned per frame.
**Action: maintain the window-full set incrementally** (a stream enters it when
its high-water crosses the bound, leaves it when claimed or reset) rather than
scanning. Note this is the same amplification argument §10.6 and §11.3 both
already make; slice 6 should not reintroduce it.

**R4 — a false-positive reset on a conforming application.** Q7's residual case.
If `send_message` emits a separate empty FIN frame, a legitimate 262 144-byte
message is reset for one RTT window with `MESSAGE_OVERFLOW` — the protocol
accusing a correct application of a programming error. Severity is high because
the symptom (*transfers die at exactly 256 KiB*) is **the exact post-mortem
ruling 59 describes**, so the operator's log would confirm the wrong diagnosis.
**Action: Q7's contract requirement, plus a core test at exactly 262 144 via
`send_message` asserting no reset is emitted.**

**R5 — two new `ConnEvent` variants break exhaustive matches in files the
implementer does not own.** `driver.rs`'s `publish` is exhaustive with no `_` arm
(good — it fails loudly, and it is `6-impl`'s file), but
`src/core/connection/tests.rs`, `tests_streams.rs`, `tests_ack.rs` and
`tests_recovery.rs` belong to earlier test authors. Any match there is an
**integration** edit, made after both agents finish (`PLAN.md:748-752`).
Sequencing risk, not a design risk — but it is the risk that makes the tree fail
to compile for the integrator and not for anybody else.

**R6 — `parse_frames` fails late and blames the wrong party.** `testfix.rs:139-226`
panics on an unknown type; the tree still **compiles**; the message says the core
crossed a slice boundary. Mitigated only by landing the `Wire::Datagram` arm
pre-dispatch (§3.2). Left unmitigated, the first symptom is a wall of panics in
the *streams* and *recovery* suites during 6-impl's own gate run.

**R7 — a mixed-width bound comparison.** `MESSAGE_RECV_MAX: u64`
(`constants.rs:297`) versus `MAX_DATAGRAM_PAYLOAD: usize` (`:315`), against a
`msg.len(): usize` on both handles. The message check must be
`msg.len() as u64 > MESSAGE_RECV_MAX` (widening); a narrowing cast the other way
is a silent bound change on a 32-bit target, and MSRV 1.96 is checked but not
cross-compiled. One line in the contract closes it.

**R8 — S16 has no paused-clock marker and three of its tests need one.** S15 and
S30 are marked *"Paused clock: yes"*; S16 is not (STORIES.md:276). SM3 (the
`acked()` idiom) and SM7 (the lost reset) are both timing tests. Not a defect —
just a place where the story's metadata under-describes its acceptance, and a
blind author who takes the marker literally may write SM3 without a virtual-time
bound and hang the suite instead of failing it. **Every test that waits on a
retransmission must be wrapped in the existing `within()` idiom
(`story_reliability.rs:103`).**

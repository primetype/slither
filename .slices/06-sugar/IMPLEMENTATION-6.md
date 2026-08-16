# Slice 6 — implementation log

Implementer: main tree, branch `main`, base commit `f19cb99`.

Blind test authors (isolated worktrees): `tests/story_datagram.rs` (§11),
`tests/story_message.rs` (§9.8). I never see their tests; they never see this code.

## 0. Status

- [ ] Read contract + plan + rulings + spec sections
- [ ] `src/core/connection/datagram.rs`
- [ ] `send_message` / `recv_message`
- [ ] `send_datagram` / `recv_datagram`
- [ ] overflow scan + guard
- [ ] shell verbs
- [ ] gates

## 1. Reading notes

### Read so far
- `CONTRACT-6.md` (366 lines, whole).
- Rulings 150–158 (`rulings.md:3799-3975`).
- `PLAN-6.md` §3 (ownership), §4 (algorithms), §6 (unstated-scope), §9 (risks).

### Binding decisions extracted
1. `Datagrams` in new `datagram.rs`: two `VecDeque<Vec<u8>>` bounded at 64,
   drop-oldest / newest-always-accepted, two `u64` drop counters, traced on
   `slither::frames`.
2. `Frame::Datagram { data, len_present }` — `0x30` = extends-to-end (no length),
   `0x31` = varint length. Five dispatch arms; `is_ack_eliciting` and
   `retransmission` MUST NOT be touched.
3. `send_datagram(now, &[u8]) -> Result<(), DatagramError>`: `lost()?` first,
   then size check (before any queue), then push (evicting), then `pump(now)`.
4. `recv_datagram() -> Option<Vec<u8>>` — **no `now`** (ruling 151), drains after
   death (ruling 152).
5. `send_message(now, &[u8]) -> Result<SendMessage, MessageError>` — whole payload
   or nothing (ruling 150); FIN on the last data frame (ruling 153).
6. `recv_message(now) -> Option<Vec<u8>>` — oldest complete unclaimed uni stream in
   open order; runs the overflow scan on **every** call; drains after death.
7. Packing: **one datagram per packet, before the stream fill** (ruling 155);
   `0x30` mandatory for the 1169-byte maximum.
8. Overflow predicate: highest received offset, no pinned final size, stream in
   `unclaimed[Uni]`, flag pending. Reset carries `MESSAGE_OVERFLOW` in a
   **connection-level retained set**, not `ResetState`; terminated by ack only.
9. R3's guard: maintain the window-full set **incrementally**, do not scan.
10. R2: `note_send_finished` must not grow `finished_senders` unboundedly for
    handle-less message streams.
11. Liveness: `seal` (not `seal_quiet`) for a packet carrying a datagram (§7.4).
12. `close()` discards the datagram send queue; nothing flushed (U-9).

### Contract items flagged on first read (detail in §4/§5 below)
- CONTRACT §2.3 declares `send_message -> Result<SendMessage, MessageError>` with
  `SendMessage` an undefined type, marked "⚠ RULING REQUIRED — §8 Q1"; ruling 150
  answers Q1 but does not name a return type. §2.5's shell signature is
  `Result<(), MessageError>`.
- CONTRACT §2.4 is titled "Two new `ConnEvent` variants" but ruling 150 mints a
  **third** (the connection-credit wake). §2.5's waker table names it only as
  "whatever Q1 rules".
- PLAN §9 R1 asks for `testutil::TraceCapture` to land pre-dispatch, owned by
  6-impl. My brief does not list `src/testutil/` among my files.

## 2. What I built

## 3. Deviations and why

## 4. Conflicts found

### F1 — the §9.8 reset is rejected by `check_peer_may_send` and kills the connection

**Found by running it, not by reading.** With everything else in place, S30's
sequence produces `ConnectionLost::ProtocolViolation { code: 4 }`
(`STREAM_STATE_ERROR`) on the **sender**, not
`WriteError::Reset(MESSAGE_OVERFLOW)`.

`Streams::check_peer_may_send` (`streams.rs`, slice 4) reads:

> The peer may send STREAM/RESET_STREAM on any bidi stream, and on a uni
> stream only if the peer opened it.

`SPEC.md:2686-2689` states the rule **with an exception the generalisation
dropped**:

> a `stream_id` naming a stream the sender of the frame could not send on
> (their receive-only half) ⇒ `STREAM_STATE_ERROR` — **with exactly one
> exception, the message-mode overflow reset of §9.8, in which the
> *receiver* of a uni stream emits RESET_STREAM as its abandonment signal
> (§9.6)**

This is working rule 8's shape exactly: a stated construction whose stated
scope the code widened. It was invisible until this slice because the case
was not constructible — `driver.rs:519-525` and `IMPLEMENTATION-5b.md`
§4-C1 both say so in terms.

**Consequence, and why it is mine.** Without the fix S30 cannot pass:
`CONTRACT-6.md` is silent on the sender's side of the reset, but STORIES.md
S30 requires *"the sender observes `WriteError::Reset(MESSAGE_OVERFLOW)`"*
and §9.8 requires *"the sender's stream frees instead of wedging"*. The
observed behaviour is the opposite of §9.8's intent — the whole connection
dies with a violation code accusing the peer.

### F2 — the sender-side handling of the receiver-emitted reset is in nobody's contract

`CONTRACT-6.md` §2.7 lists what slice 6 must not build and does not reach
this; §2.1–§2.5 describe the receiver's side of §9.8 only. But §18.1 names
`WriteError::Reset` as *"the stream was reset — by the local application,
**or by the peer's message-mode overflow reset (§9.8, the one
receiver-emitted RESET_STREAM)**"*, and slice 5b recorded the case as
deferred to this slice. Built; see §3.

## 5. Mechanisms named that do not exist

## 6. Fixture capabilities needed and not added

## 7. Gate output

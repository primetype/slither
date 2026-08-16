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

## 5. Mechanisms named that do not exist

## 6. Fixture capabilities needed and not added

## 7. Gate output

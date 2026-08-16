# Slice 6 — implementation log

Implementer: main tree, branch `main`, base commit `f19cb99`.

Blind test authors (isolated worktrees): `tests/story_datagram.rs` (§11),
`tests/story_message.rs` (§9.8). I never see their tests; they never see this code.

## 0. Status — complete

- [x] Read contract + plan + rulings + spec sections
- [x] `src/core/connection/datagram.rs`
- [x] `send_message` / `recv_message`
- [x] `send_datagram` / `recv_datagram`
- [x] overflow scan + guard (incremental, §9 R3)
- [x] shell verbs + three waker slots + three `publish` arms
- [x] gates: all green, 806 passing / 0 failing (baseline 801 + 5 seam tests)

**One finding that changed code outside the contract's description:** §9.8's
receiver-emitted RESET_STREAM was rejected by `check_peer_may_send` and killed
the connection with `STREAM_STATE_ERROR`. S30 could not have passed. See §4-F1.

**Ruling 163** arrived mid-task and matched what was already built.

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

### Core
- **`src/core/connection/datagram.rs`** (new). `Datagrams` — two
  `VecDeque<Vec<u8>>` bounded at 64, drop-oldest / newest-always-accepted —
  and `DatagramDrops { send, recv }`. The eviction counter is incremented
  and traced **inside** `push_send`/`push_recv` rather than by the caller
  (`CONTRACT-6.md` §2.1 says *"the caller traces and counts"*), because
  `drops` is that type's own field and a caller-side count is one more place
  to forget one. Trace at **WARN** on `slither::frames` carrying the
  cumulative counter: a default subscriber sits at INFO, so a lower level
  would leave the drop silent for exactly the operator §11.5 is written for.
  `unpop_send` is additive — §14.5's refused packet must put the datagram
  back at the **front**, or a repeatedly-refused datagram ages to the head
  of the eviction order and is dropped ahead of newer ones.
- **`frame.rs`**: `Frame::Datagram { data, len_present }` and its five
  dispatch arms; `extends_to_end` now answers for both frame types;
  `Packing::datagram(&[u8]) -> bool` picks `0x31` when the length varint
  also fits and `0x30` otherwise. `is_ack_eliciting` and `retransmission`
  untouched (C4). `Structural::TrailingFrame` minted for §8.4's
  `0x30`-not-final rule, with the guard in `parse`'s loop — **dead by
  construction**, and documented as such at both sites (§6 U-3, §7 C-6).
- **`recv.rs`**: `RecvHalf::is_complete()` (FIN pinned, every byte to it,
  **and no reset**) and `take_message()`. `take_message` counts bytes exactly
  as `read` does but **never calls `take_grant`** — that omission, not
  `earns_stream_credit`, is the mechanism behind §10.3's *"sugar-consumed
  streams never earn stream-level credit"* (ruling 156c leaves the flag
  `true` everywhere).
- **`streams.rs`**: `recv_message`, the incremental `overflow_candidates`
  set, `retained_resets`, and the §9.8 reset emission. `Stream::unclaimed`
  mirrors the deque's membership so the per-frame question is O(1).
- **`send.rs`**: `SendHalf::peer_reset` and `stopped_by_peer` — see §4-F1.
- **`mod.rs`**: the four verbs, `SendMessage`, three `ConnEvent` variants,
  the datagram fill before `streams.fill`, the liveness-marking OR,
  `owes_output()`, the `close()`/`drop_state()` send-queue discard, and the
  `#[cfg(test)] datagram_drops()` accessor.

### Shell
- **`shared.rs`**: `message_readers`, `datagram_readers`, `message_senders`
  on `ConnCell`, in `ConnCell::new` **and** in `take_all_stream_wakers`;
  `Wakers::len` and `ConnCell::sugar_waker_entries` for the occupancy pin.
- **`driver.rs`**: `publish` arms for the three new events, `StreamsAvailable
  { Uni }` extended to `message_senders`, and `blocked_writers` added to the
  `StreamReset` arm (§4-F1).
- **`connection.rs`**: `send_message`, `recv_message`, `send_datagram` (not
  async), `recv_datagram`, their three `poll_*` forms and three slot
  minters, plus `sugar_waker_entries`.
- **`mod.rs`**: five implementer seam tests. **Not** the acceptance tests —
  S15/S16/S30 are the blind authors' and live in `tests/story_*.rs`.

### Verified by running, then removed (temporary probes)
Three throwaway tests confirmed the mechanisms most likely to be silently
wrong, and are **not** in the commit — the blind authors own these
assertions:
- a 1169-byte datagram crosses end to end (so the `0x30` path is live and
  ruling 155's *"cannot be sent at all"* is closed);
- a **262 144-byte `send_message` is not falsely reset** — ruling 153's
  false positive on a conforming application does not occur;
- the mixing sender observes `WriteError::Reset(MESSAGE_OVERFLOW)`. This one
  **failed**, and is §4-F1.

## 3. Deviations and why

1. **`SendMessage` defined as `{ Sent, Blocked }`.** `CONTRACT-6.md` §2.3
   named the type and defined it nowhere. Chosen before the maintainer's
   ruling 163 arrived mid-task, and **identical to it**: two variants, no
   reason carried, "not now" reported in the success type because it is
   `write`'s `Ok(0)` and `accept`'s `None`, not an error. §18.1 untouched.
   **The no-op invariant holds and is delivered by ordering, not cleanup**:
   `send_message` tests connection credit *before* `Streams::open`, and
   `Streams::open` returns `Err(StreamsExhausted)` above its first mutation
   (verified by reading it — working rule 11). On `Blocked` no index is
   spent, no half exists, no byte is buffered, no event is queued and
   `pump` is not called.
2. **§16.4's core signature for `send_message` is `Result<(), MessageError>`
   in SPEC.md; I implemented `Result<SendMessage, MessageError>`.** Ruled by
   163 while in flight. `pub(crate)`; the shell's public verb is
   `Result<(), MessageError>` exactly as §16.2 has it, so no public API and
   no wire byte moves.
3. **`send_message` calls `streams.write` / `streams.finish` directly, not
   `Connection::write` / `Connection::finish`.** Both of those `pump`, and
   an intervening pump would put the last data frame on the wire **before**
   the FIN was pinned — manufacturing exactly the separate-empty-FIN-frame
   race ruling 153 forbids. With `finish` applied first,
   `SendHalf::next_chunk`'s existing `carries_fin` puts the FIN on the chunk
   that ends at the final size, and `on_lost_range` clears `fin_sent` so a
   **retransmitted** tail re-carries it. Ruling 153 therefore needed no new
   flag — only an ordering constraint.
4. **The overflow scan is an incrementally-maintained set, not a walk.**
   §9 R3's action, implemented as `Streams::overflow_candidates`. A stream
   enters when its high-water crosses the bound with no final size, and
   leaves on a pinned final size, on `accept()`, on the §9.8 claim, or on a
   peer reset. Per-frame cost is O(log n) on a stream *at* the bound and
   zero otherwise.
5. **`Stream::unclaimed` added** as an O(1) mirror of the `unclaimed` deque's
   membership, written in the three places the deque is. Without it the
   per-frame "is this still unclaimed?" question is linear in a deque bounded
   only by §10.4's *cumulative* limit.
6. **The `#[cfg(test)] mod tests_datagram; mod tests_message;` declarations
   in `core/connection/mod.rs` are committed COMMENTED OUT**, with an
   integration header. `PLAN-6.md` §3.1 asks 6-impl to declare them and
   create nothing — but a `mod` naming a missing file fails the build, so
   every gate in my own table would be red. Same treatment, same reason and
   same header as ruling 126's `Cargo.toml` stanzas. **The integrator
   uncomments four lines** (two here, two in `Cargo.toml`).
7. **`sugar_waker_entries` is a new accessor, not an extension of
   `stream_waker_entries`.** §9's mitigation says to extend the existing one;
   its arity is already named by tests this slice does not own, and widening
   it would red them for an unrelated reason. Additive instead.
8. **`drop_state` discards the datagram **send** queue** as well as
   `close()`. U-9 states the `close()` rule; the same reasoning covers every
   other death, and the **receive** queue is deliberately kept for ruling
   152's post-death drain.

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

## 4b. Rulings 159–164, answered one by one

Rounds 27 and 28 landed while I was building. Read after the fact; each is
answered here against the code as committed.

- **159 — the `0x30`-not-final check.** The ruling says the check *"already
  exists"* at `frame.rs:705` returning `Structural::TrailingFrame`, and that
  `CONTRACT-6.md` §2.2's three claims are *"all wrong"*. **The outcome is
  right and the attribution is not.** `git show f19cb99:src/core/connection/frame.rs
  | grep -c TrailingFrame` → **0**; at HEAD → **3**. That variant and that
  loop guard are *mine, added in this slice*, and the file read at 02:16 was
  my uncommitted working tree, not the base commit. **§2.2 was correct about
  `f19cb99`.** Nothing needs changing — what I built is exactly what 159
  asks for: the guard in `parse`'s loop, dead by construction and documented
  as such, `Frame::Datagram` answering `extends_to_end()` truthfully, and
  **nothing inside `parse_body`**. Recorded because the ruling as written
  will tell a later reader the planner was careless when it was not, and
  because it is working rule 11's own shape — a rationale naming a mechanism
  whose provenance was not checked.
- **160 — `finished_senders` and handle-less message streams.** The ruling
  offers two remedies and asks the implementer to pick. **I pick neither,
  because the map already excludes them and no change was needed.**
  `ConnCell::note_send_finished` (`shared.rs`) is
  `let Some(wakers) = self.blocked_ackers.get_mut(&r) else { return Vec::new(); };`
  and the `finished_senders.insert(r)` sits **after** that early return. A
  §9.8 message stream has no `SendStream`, therefore no `blocked_ackers`
  entry, therefore no insert. Verified by opening the file, not by reading
  the contract, which predicted the opposite (§5-2).
  `Connection::acked()` over messages still works, because the
  `settled_wakers` sweep in `driver.rs`'s `StreamFinished` arm is **outside**
  that gate.
- **161 — packing never evicts.** Held: the fill uses `peek_send()`, which
  does not remove, so a head that does not fit stays queued; the only
  eviction is `send_datagram`'s on enqueue. §14.5's refused packet returns
  the datagram to the **front** via `unpop_send`. The preferential-delay
  residual is now documented at the packing site in `pump_packets`.
- **162 / Round 28's "recorded, not ruled" — the trace obligation.** Matches
  what I reported independently in §6, and is now a named slice-9
  obligation. Nothing for me to build.
- **163 — `SendMessage { Sent, Blocked }`.** Already built identically
  before the ruling arrived; see §3-1 for the no-op invariant, which holds
  by ordering.
- **164 — §15.3's registry and §9.8's parenthetical.** I did not rely on
  either. The predicate as implemented is *"in `unclaimed[Uni]`, **no final
  size pinned**, and highest received offset ≥ `MESSAGE_RECV_MAX`"*, and
  `note_message_progress` drops a stream from the candidate set the instant
  a final size is pinned — ruling 153's independent clause, not the
  parenthetical's argument. The 262 144-byte probe in §2 confirms it by
  running.

## 5. Mechanisms named that do not exist

1. **`CONTRACT-6.md` §2.4 is titled "Two new `ConnEvent` variants"** and
   ruling 150 — quoted in the same file's §0 — mints a **third** for the
   connection-credit wake. §2.5's waker table refers to it only as
   *"whatever Q1 rules"*. Built as `ConnEvent::SendCreditAvailable`, emitted
   when a MAX_DATA actually raises the limit. Three variants, not two.
2. **`CONTRACT-6.md` §2.5's `wake_settled` paragraph and §9 R4 both warn
   that `finished_senders` grows without bound for handle-less message
   streams. It does not.** `ConnCell::note_send_finished` (`shared.rs`)
   returns `Vec::new()` **before** the insert when `blocked_ackers` has no
   entry for the stream, and `blocked_ackers` holds one entry per live
   `SendStream`. A handle-less message stream therefore inserts nothing.
   Opened the file rather than reasoning from the contract (working rule
   11); no change was needed. The `settled_wakers` sweep in the
   `StreamFinished` arm is **outside** that gate, so `Connection::acked()`
   over messages works with no new wake source — as §2.5 predicted.
3. **`CONTRACT-6.md` §2.2's `Datagram::parse_body` outcome table lists a
   `0x30`-not-final error the same section then says is unrepresentable.**
   Correct, and its own ⚠ note says so. Implemented as
   `Structural::TrailingFrame` checked in `parse`'s loop, and documented at
   both sites as dead by construction so a later reader does not mistake it
   for a live check.
4. **§9 R1 asks for a `testutil::TraceCapture` "owned by 6-impl and landed
   PRE-DISPATCH".** It does not exist at `f19cb99` — see §6.

## 6. Fixture capabilities needed and not added

**`testutil::TraceCapture` was never landed.** `PLAN-6.md` §9 R1 calls it
the largest hole in the slice — ruling 59 is a **MUST**, S30's fourth
`Accepts` bullet repeats it, and §11.5's drop counters are surfaced
*only* through the trace (§16.2's accessor list is exhaustive, so there is
no public counter). `grep -rn "tracing" src/testutil/` at `f19cb99` returns
nothing.

**Reported, not added** (brief: *"If you need a further fixture capability,
report it; do not add it"*), and adding it now would not help: both blind
authors were cut from `f19cb99` and cannot see a file that lands after
them. `src/testutil/` is also absent from my brief's file list, though
`PLAN-6.md` §3.1 assigns it to 6-impl if touched.

**Consequence to weigh at integration:** the receiver's ruling-59 trace
obligation and §11.5's two counters are **unverified by any test that can
exist today**. The `#[cfg(test)] Connection::datagram_drops()` accessor I
added lets a core test pin the eviction *discipline*; it cannot pin the
trace. An unverified MUST discovered at the release gate is a blocked
release.

**Also worth knowing:** `PLAN-6.md` §3.1 gives the blind core-test authors
`src/core/connection/tests_datagram.rs` and `tests_message.rs`, whose `mod`
declarations live in **my** `mod.rs`. In their own worktrees those files
cannot compile without a declaration they are not allowed to write. Whatever
they did locally, the committed declarations are mine and are commented out
(§3-6).

## 7. Gate output

Run on the commit being reported, in the main tree at `main`.

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ cargo fmt --all --check
(no output, exit 0)

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
 Documenting slither v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s)
   Generated target/doc/slither/index.html

$ cargo test                    →  731 passed, 0 failed   (baseline 726)
$ cargo test --all-features     →  806 passed, 0 failed   (baseline 801)
$ cargo test --release --all-features
                                →  806 passed, 0 failed

$ cargo test --all-features golden
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored

$ cargo +1.96 check --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

**No existing test went red.** The +5 on both counts is the five implementer
seam tests added to `src/shell/mod.rs`'s own `mod tests`; every other number
is the baseline unchanged. The wire pins are byte-identical — slice 6 adds a
frame **type** (`0x30`/`0x31`, already in `constants.rs` and already answered
by `is_ack_eliciting`/`retransmission` in slice 3a) and moves no existing
byte.

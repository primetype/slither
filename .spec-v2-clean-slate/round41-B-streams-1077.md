# Round 41 item 7 — `streams.rs:1077` `stream_payload_room` over-promise

Investigation agent B. Worktree:
`/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a20df04ee94d38449`

## 0. Base verification

```
$ pwd && git rev-parse HEAD && git status --short && git log --oneline -3
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a20df04ee94d38449
1395ea6001533e4bdec551cd0862ecf207b6c915
1395ea6 Round 40 closes: the round-41 material, consolidated
65af633 Slice R40-E integration: ruling 253 lands — the merge is small-to-large
8ffd6c1 Slice R40-E: merge the blind test author (the work bound and its separators)
```

Base is `1395ea6` as briefed. Tree clean. No reset needed.

## 1. REPRODUCE — YES, on the first try, both orderings

Reproducer: `tests/repro_1077.rs` (worktree-only) + a `[[test]]` stanza
appended to `Cargo.toml`. Two tests, identical except for **when** the
policy is installed: `repro_after_establish` (policy after the handshake)
and `repro_before_establish` (policy from the dial). Both use the briefed
recipe verbatim: `Pair::seeded(0xE5B0_000C)`, one 2 KiB uni stream,
`FlakyPolicy::lossy(0.5).with_delay(Duration::from_millis(20), Duration::ZERO)`
on **both** wires, paused clock, no sleeps.

```
$ RUST_BACKTRACE=1 cargo test --all-features --test repro_1077
running 2 tests
test repro_after_establish ... FAILED
test repro_before_establish ... FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

Panic head + the load-bearing frames of the backtrace:

```
thread 'repro_after_establish' (4508855) panicked at src/core/connection/streams.rs:1092:25:
stream_payload_room over-promised
stack backtrace:
   2: <slither::core::connection::streams::Streams>::fill
             at ./src/core/connection/streams.rs:1092:25
   3: <slither::core::connection::Connection<..>>::pump_packets
             at ./src/core/connection/mod.rs:2320:40
   4: <slither::core::connection::Connection<..>>::pump_inner
             at ./src/core/connection/mod.rs:2094:14
   5: <slither::core::connection::Connection<..>>::pump
             at ./src/core/connection/mod.rs:2072:14
   6: <slither::core::connection::Connection<..>>::apply_live
             at ./src/core/connection/mod.rs:1800:14
   7: <slither::core::connection::Connection<..>>::handle_datagram
             at ./src/core/connection/mod.rs:646:18
   8: <slither::shell::driver::Driver<..>>::handle_datagram
             at ./src/shell/driver.rs:927:26
```

Note the line number: the assert is at **1092**, not 1077 — item 7's
`streams.rs:1077` is off by the fifteen lines the `fill` loop grew before
`1395ea6`. The `debug_assert` text is the one named. Same defect.

The panic is on the **datagram-receive** path (`handle_datagram` →
`apply_live` → `pump`), not on the write path: it is the pump triggered by
an incoming ACK, which is the clue that matters for §3.

## 2. INSTRUMENT — the exact disagreement

Instrumentation (worktree only, later reverted):

* `src/core/connection/frame.rs` — an `eprintln!` at the top of
  `stream_payload_room` printing `id`, `offset`, `fixed`, `room()`,
  `used`, `budget`, `stage`, `extends_to_end`; plus a scratch
  `Packing::dbg_state()`.
* `src/core/connection/streams.rs` — an `eprintln!` immediately before
  `packing.fill(frame)` printing the query offset, the promised `room`,
  the chunk's offset/len/fin/fresh, the frame's **actual**
  `encoded_len()`, the packet's remaining capacity, and every frame
  already packed as `(type_code, encoded_len)`.

Full trace of the failing pump (`-- --nocapture`, grepped to the two tags):

```
[ROOM] id=2 offset=0    fixed=2 room()=1170 used=0    budget=1170 stage=Ack  ete=false
[FILL] id=2 query_off=0    room=1166 chunk_off=0    chunk_len=1024 fin=false fresh=true  encoded_len=1028 pkt_room=1170 used=0    budget=1170 stage=Ack  ete=false has_data_pending=true  frames=[]
[ROOM] id=2 offset=1024 fixed=4 room()=142  used=1028 budget=1170 stage=Fill ete=false
[FILL] id=2 query_off=1024 room=136  chunk_off=1024 chunk_len=136  fin=false fresh=true  encoded_len=142  pkt_room=142  used=1028 budget=1170 stage=Fill ete=false has_data_pending=true  frames=[(10, 1028)]
[ROOM] id=2 offset=1160 fixed=4 room()=0    used=1170 budget=1170 stage=Fill ete=false
...
[ROOM] id=2 offset=0    fixed=2 room()=1170 used=0    budget=1170 stage=Ack  ete=false
[FILL] id=2 query_off=0    room=1166 chunk_off=0    chunk_len=1024 fin=false fresh=false encoded_len=1028 pkt_room=1170 used=0    budget=1170 stage=Ack  ete=false has_data_pending=true  frames=[]
[ROOM] id=2 offset=1024 fixed=4 room()=142  used=1028 budget=1170 stage=Fill ete=false
[FILL] id=2 query_off=1024 room=136  chunk_off=1024 chunk_len=136  fin=false fresh=false encoded_len=142  pkt_room=142  used=1028 budget=1170 stage=Fill ete=false has_data_pending=false frames=[(10, 1028)]
[ROOM] id=2 offset=2048 fixed=4 room()=0    used=1170 budget=1170 stage=Fill ete=false     <-- returns None
[FILL] id=2 query_off=2048 room=0    chunk_off=2048 chunk_len=0    fin=true  fresh=true  encoded_len=5    pkt_room=0    used=1170 budget=1170 stage=Fill ete=false has_data_pending=false frames=[(10, 1028), (14, 142)]
thread 'repro_after_establish' panicked at src/core/connection/streams.rs:1117:25:
stream_payload_room over-promised
```

(1117 is the instrumented line number of the same `debug_assert`.)

### The failing call, decoded

The retransmission pass has just refilled a packet to **exactly** full:
`used == budget == 1170`, `room() == 0`. Two frames are in it — a
1028-byte STREAM at offset 0 (`type_code` 10 = `LEN`) and a 142-byte
STREAM at offset 1024 (`type_code` 14 = `OFF|LEN`). The stream's only
remaining obligation is the **bare FIN** at offset 2048:
`has_data_pending() == false`, `has_pending() == true`.

* `stream_payload_room(id=2, offset=2048)`: `fixed = 1 + varint_len(2) +
  varint_len(2048) = 1 + 1 + 2 = 4`; `room() = 0`;
  `0.checked_sub(4)` is `None`, so it returns **`None`** — its documented
  meaning being *"not even an empty frame fits"*
  (`src/core/connection/frame.rs:1051-1053`).
* `src/core/connection/streams.rs:1071`:
  `packing.stream_payload_room(id, offset).unwrap_or(0)` collapses that
  `None` onto **`0`** — which is the value `stream_payload_room` also
  returns for the *opposite* fact, `Some(0)` meaning *"an empty frame
  fits exactly, with zero payload"* (reachable only at `room() == fixed +
  1`).
* `src/core/connection/streams.rs:1072`: the defer guard is
  `room == 0 && send.has_data_pending()`. `has_data_pending()` is
  **false** here — a bare FIN is not data — so the guard does not fire.
* `src/core/connection/send.rs:363` `next_chunk(0)`: both range sets are
  empty, so it takes the FIN branch, sets `fin_sent = true`, and returns
  a `Chunk { offset: 2048, data: [], fin: true }`. The `take == 0` early
  return that protects the data path is never reached.
* `src/core/connection/frame.rs:362` `Stream::body_len` for that frame:
  `varint_len(2)=1 + varint_len(2048)=2 + varint_len(0)=1 + 0` = 4, plus
  the type byte = **`encoded_len() == 5`**.
* `src/core/connection/frame.rs:1104` `Packing::push`:
  `self.used + len > self.budget` → `1170 + 5 > 1170` → returns `false`.
  Assert fires.

### The quantity the two sides computed differently

**Neither side miscomputes a varint width.** The widths agree exactly:
`stream_payload_room`'s `fixed` (4) plus the length varint (1) is
precisely `encoded_len()` (5), and the payload-width search
(`frame.rs:1060-1067`) is sound — `varint_len` is monotone, so a chunk
shorter than the promise always encodes shorter.

The disagreement is about a **capacity that was never promised at all**:

| side | file:line | quantity |
|---|---|---|
| callee | `frame.rs:1055` `stream_payload_room` | returns `None` = *no frame of any size fits* |
| caller | `streams.rs:1071` `.unwrap_or(0)` | reads `0` = *a zero-payload frame fits* |
| caller | `streams.rs:1072` guard | rescues only `has_data_pending()`, i.e. only the data case |
| callee | `frame.rs:1104` `push` | `used + 5 > budget` — refuses |

Nothing changed between the query and the fill: same `used`, same
`budget`, same `stage`, `extends_to_end` false throughout, and no other
frame was packed in between (`frames=[(10,1028),(14,142)]` at both
points). This is **not** a state-change race.

## 3. ROOT CAUSE

`Packing::stream_payload_room` answers with a three-valued fact and the
fill loop reads it as two-valued. `None` means *"not even an empty frame
fits"*; `Some(0)` means the **opposite** — *"a frame fits, with zero
payload"*, reachable only at `room() == fixed + 1`. `streams.rs:1071`
collapses both onto `0` with `.unwrap_or(0)`, and the guard on the next
line — `room == 0 && send.has_data_pending()` — was written for the
`Some(0)` reading only. That guard is documented at
`send.rs:182-185`: *"Whether the fill loop has **bytes** to take, as
opposed to a bare FIN. The distinction matters when the packet has no
room: an empty FIN frame still fits where a data frame does not."* True
of `Some(0)`, false of `None`. So when the sole remaining obligation is a
bare FIN (`has_data_pending() == false`, `has_pending() == true`) and the
packet is full, the guard abstains, `next_chunk(0)` returns the empty
FIN chunk through the one branch of `next_chunk` that ignores its
`max_len` (`send.rs:373-384` — the `take == 0` early return is below it
and never reached), and a 5-byte frame is offered to a packet with zero
bytes left.

**Classification: (a), on the caller's side of the query** — an
arithmetic/typing defect in how `streams.rs` interprets the room result,
not in `stream_payload_room`'s own arithmetic. Explicitly **not** (b):
the instrumented trace shows `used`, `budget`, `stage`,
`extends_to_end` and the packed-frame list identical at the query and at
the fill, so nothing changed in between. Explicitly **not** (c):
`push`'s `used + len > budget` refusal is exactly §8.6's per-seal bound
(*"Every slither seal is at most `MAX_PLAINTEXT` + 16"*), and accepting
the frame would overrun the plaintext.

### Which side matches the spec

**`fill` does; the caller does not.**

* **§8.1** (varint encoding, SPEC.md:3265): both sides compute widths
  from the same `varint_len`, and they agree byte for byte here —
  `fixed = 4` on the query side, `encoded_len() = 5` on the frame side,
  the difference being exactly the `varint_len(0) = 1` length prefix the
  query deliberately excludes. §8.1 is not implicated.
* **§8.5** (SPEC.md:3589): names the stage order and *"at most one
  extends-to-end frame … per packet, in final position"*. Neither is
  violated — `ete=false` throughout, and `Stream::new` never emits the
  ¬LEN form.
* **§8.6** (SPEC.md:3636): the bound `fill` enforces. Sound.
* **§9.5** (SPEC.md:3767): *"An empty STREAM frame with FIN is a valid
  end-of-stream marker"* — the frame the loop built is legal; it just
  does not fit in **this** packet. §9.5 says nothing about capacity, so
  it cannot license the over-promise.

### A latent second reader of the same query, checked and cleared

`stream_payload_room` consults only `room()` — it does **not** consult
`Packing::extends_to_end`, while `push` (`frame.rs:1100-1102`) refuses
*everything* once that flag is set, regardless of room. Rule 8's shape:
a construction whose scope is unstated. Ruling 155 packs one DATAGRAM
**before** the stream fill (`mod.rs:2308-2311`), so a `0x30` (¬LEN)
datagram can set the flag ahead of `Streams::fill` in the same packet.

It is **not independently reachable in this build**, and the arithmetic
says why: `Packing::datagram` picks the ¬LEN form only when
`varint_len(n) + n > body`, i.e. `body - n < varint_len(n) <= 2` for any
`n <= MAX_DATAGRAM` (1169), so a ¬LEN datagram always leaves **at most
one byte**. `fixed >= 2` for every stream frame, so
`stream_payload_room` returns `None` there — which funnels straight back
into the defect above rather than being a second one. The fix in §5
closes both. Worth recording for whoever adds a third extends-to-end
contributor or a ¬LEN STREAM emitter: at that point the flag would need
to be in the query.

## 4. RELEASE-BENIGN CLAIM — verified, not trusted

### (i) Release build completes with bytes intact

```
$ cargo test --release --all-features --test repro_1077
    Finished `release` profile [optimized] target(s)
     Running tests/repro_1077.rs (target/release/deps/repro_1077-cdd1b6513f75ca18)
running 2 tests
test repro_after_establish ... ok
test repro_before_establish ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Both tests assert `got == want` over the full 2 KiB (byte-for-byte, and
the payload is `i % 251` so any shift moves every subsequent byte). No
`[profile.release]` override exists in `Cargo.toml`, so
`debug_assertions` really is off.

### (ii) The defer-retry makes progress — measured, not argued

The `debug_assert` was replaced with an `eprintln!` (identical control
flow to release) and the fixture instrumented with virtual-time
timestamps. 64 consecutive seeds, same recipe:

```
$ cargo test --all-features --test repro_1077 sweep -- --nocapture --test-threads=1
[DONE] 0xe5b00000 in 84.754s
[SEED] 0xe5b00001
[DONE] 0xe5b00001 in 270ms
...
[SEED] 0xe5b0000c
[OVERPROMISE] id=2 query_off=2048 room=0 chunk_off=2048 chunk_len=0 fin=true fresh=true has_data_pending=false
[DONE] 0xe5b0000c in 1.124s
...
[SEED] 0xe5b00027
[OVERPROMISE] id=2 query_off=2048 room=0 chunk_off=2048 chunk_len=0 fin=true fresh=true has_data_pending=false
[DONE] 0xe5b00027 in 2.04s
...
[SEED] 0xe5b00035
[OVERPROMISE] id=2 query_off=2048 room=0 chunk_off=2048 chunk_len=0 fin=true fresh=true has_data_pending=false
[DONE] 0xe5b00035 in 66.132s
...
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 1.83s
```

* **Firing count: exactly one per affected transfer.** 3 of 64 seeds
  (`0x…0C`, `0x…27`, `0x…35`, ≈4.7 %) reach the arm, and **never twice**.
  No livelock, no repeated over-promise.
* **All three finish**, bytes intact, in bounded virtual time.
* Why it cannot recur: `room() == 0` means the packet is *full*, so the
  pump seals it and builds the next one from `MAX_PLAINTEXT`, where the
  5-byte FIN frame trivially fits.

### (iii) The one way it *could* have stalled, checked

`return_chunk(2048..2048, fin=true, fresh=true)` inserts an **empty**
range into `fresh`. Had `RangeSet::insert` admitted it, `fresh` would be
permanently non-empty, `has_data_pending()` permanently true, and
`next_chunk` would return `None` for ever on the `take == 0` line — the
FIN would never be sent and the stream would hang. It does not:
`src/core/connection/send.rs:610-613` early-returns on
`r.start >= r.end`. The state after the failed fill is exactly the state
before it, plus `fin_sent = false` restored. Benign confirmed.

### (iv) Unrelated fixture noise, recorded so it is not mistaken for this

3 of the 64 seeds ended `ConnectionLost(TimedOut)` (`0x…08`, `0x…25`,
`0x…2A`). None of them touched the over-promise arm. That is §7.5's
25 s dead timeout losing a race against a sustained 50 % **two-way**
loss rate — a property of the recipe, not of this defect. Flagged only
because a reader of the sweep output would otherwise attribute it here.

## 5. FIX CANDIDATES

### The fix implemented — `src/core/connection/streams.rs`, one predicate

Two lines replaced (plus a comment). `git diff -- src/core/connection/streams.rs`:

```diff
-                let room = packing.stream_payload_room(id, offset).unwrap_or(0);
-                if room == 0 && send.has_data_pending() {
+                // `None` and `Some(0)` are **opposite** facts and must not
+                // be collapsed: `None` is *"not even an empty frame fits"*,
+                // `Some(0)` is *"a frame fits, with no payload"* — reachable
+                // only at `room() == fixed + 1`, which is exactly the width
+                // of the bare-FIN frame. `has_data_pending()` is the guard
+                // for the second (`send.rs`: an empty FIN frame still fits
+                // where a data frame does not); only `fits.is_none()` is the
+                // guard for the first, because a bare FIN is not data and
+                // walks past the other one into a `fill` no packet can
+                // honour.
+                let fits = packing.stream_payload_room(id, offset);
+                let room = fits.unwrap_or(0);
+                if fits.is_none() || (room == 0 && send.has_data_pending()) {
                     defer = true;
                 } else if let Some(chunk) = send.next_chunk(room.min(STREAM_FILL_QUANTUM)) {
```

Lines changed: `streams.rs:1071-1072` become `1071-1083`. **Nothing else
in the crate is touched.** `stream_payload_room`, `push`, `next_chunk`,
`return_chunk` and every constant are untouched.

Why it is complete, including the latent `extends_to_end` reader of §3:
`extends_to_end == true` implies `room() <= 1` (a ¬LEN datagram leaves
`body - n < varint_len(n) <= 2` bytes, and slither never emits a ¬LEN
STREAM), and `fixed >= 2` for every stream frame, so
`stream_payload_room` is `None` there — which the new disjunct defers on.

### Reproducer, with the `debug_assert` restored

```
$ RUST_BACKTRACE=1 cargo test --all-features --test repro_1077 -- --test-threads=1
running 3 tests
test repro_after_establish ... ok
test repro_before_establish ... ok
test sweep ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.77s
```

### No wire byte and no observable timing moves — measured

The 64-seed sweep's virtual-time completion instants were captured
before the fix (with the assert replaced by a print, i.e. release control
flow) and after it, and diffed:

```
$ diff pre-fix-sweep.txt post-fix-sweep.txt && echo IDENTICAL
IDENTICAL: 64/64 seeds, same virtual-time completion, same outcomes
```

Every seed's completion instant is identical to the millisecond,
including the three that reach the arm and the three that time out. On a
deterministic seeded fabric that is the strongest available statement:
**the fixed build and the release-mode broken build are observationally
identical.** The defect was only ever a debug-driver panic. No wire byte,
no ratified constant and no timer is involved — this is entirely the
internal `Packing` seam (ruling 207(c)), so **no ruling is needed**.

### Full gate table, on the worktree with the fix

| Gate | Command | Result |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | `Finished dev profile ... in 0.59s` |
| Format | `cargo fmt --all --check` | `FMT: no diff` |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | `Finished dev profile ... in 3.86s`, zero warnings |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | `Generated .../target/doc/slither/index.html` |
| Tests | `cargo test` | `binaries 5 passed 880 failed 0 ignored 0` |
| Tests | `cargo test --all-features` | `passed 1071 failed 0 ignored 1` |
| Release tests | `cargo test --release --all-features` | `binaries 23 passed 1072 failed 0 ignored 0` |
| Wire pins | `spec_constants` (112) + `story_codec` (4), under `cargo test` | all pass |

(1071 = the baseline 1068 plus this worktree's three scratch reproducer
tests; the one `ignored` is the pre-existing `story_rekey` ignore, and
`--release` runs it, hence 1072.)

MSRV and `cargo deny` were **not** run — they are unaffected by a
two-line predicate change and neither was requested; do not read this
table as claiming them.

### The other plausible fix, and why it was not taken

**Make the seam three-valued instead of patching the caller.** Replace
`stream_payload_room(id, offset) -> Option<usize>` with an enum —
`NoFrameFits` / `PayloadRoom(usize)` — or add
`Packing::fits_empty_stream_frame(id, offset)`. That is the *structurally*
better fix: it removes the `unwrap_or(0)` foot-gun permanently, and it
gives the query somewhere to consult `extends_to_end`, closing the
latent hazard of §3 by construction rather than by the arithmetic
accident that a ¬LEN datagram happens to leave ≤ 1 byte.

Tradeoff, and why the minimal fix wins today: it changes a `pub(crate)`
signature on ruling 207(c)'s seam, which slice 6's DATAGRAM contributor
and slice 7's probe packing both sit on, for zero behavioural
difference. **Neither spec text prefers one** — §8.5 and §8.6 constrain
what may be *packed*, not how the packer asks. Recommend the minimal fix
now and the enum only if a third extends-to-end contributor is ever
added, at which point the arithmetic accident stops holding and the
query genuinely must see the flag.

**Rejected outright:** making `fill` accept the frame. `push`'s
`used + len > budget` is §8.6's per-seal bound; relaxing it would
overrun `MAX_PLAINTEXT` and move wire bytes.

## 6. TEST GAP

**Why nothing reaches it (rule 13's shape).** The arm needs a
**four-step geometry** that no existing test constructs: (1) a stream
whose *retransmit* set is a strict prefix of its final size, so
`next_chunk`'s `carries_fin` — which requires
`Some(end) == final_size() && fresh.is_empty() && retransmit.is_empty()`
(`send.rs:404-408`) — is false and the FIN stays a **separate bare
obligation**; (2) that retransmit prefix summing to *exactly*
`MAX_PLAINTEXT` (1024 + 136 payload bytes → 1028 + 142 = 1170); (3) the
bare FIN still owed at the moment that packet fills; (4) all three in one
pump. The existing population cannot produce it, for two
separate reasons:

* **The deliberate-loss tests drop *chosen* indices** — `drop_at`,
  `block_path`, ruling 148's counter-provable blackholes — precisely so
  the geometry stays stable and a red is unambiguous. A stable geometry
  is exactly what keeps a retransmit set off the 1170-byte boundary.
  `story_streams` S12, the one bulk-stream soak, runs
  `FlakyPolicy::perfect()` with reorder and duplication and **no loss at
  all**, so it has no retransmit set to place.
* **The two tests that do use `lossy` on a stream are `story_compat`'s
  two S31 tests (`tests/story_compat.rs:207` seed `0x5310_0001`, 64 KiB;
  `:379` seed `0x5310_0003`, 48 KiB) at `lossy(0.10)`.** They even produce the
  *bare-FIN* half of the state for free — `BufWriter::shutdown` finishes
  after the body has drained, which is step 2 of the recipe below. What
  they cannot produce is the second half on demand: at 10 % loss the
  retransmit prefix rarely reaches a full quantum, and with two seeds
  there is no draw to reroll. My 64-seed sweep at 50 % loss hits the arm
  on 3 seeds (≈5 %); at two seeds and a fifth of the loss rate, the
  suite's expected count is essentially zero.

So the harness is not missing a **fault** class — `lossy` exists and is
used. It is missing a **coincidence** class, and no amount of
test-writing against a two-seed lossy fixture closes it. The answer is to
build the coincidence deterministically rather than hunt seeds.

**What the permanent regression test should pin, and how to get there
without seed roulette** (a core-level `Solo`/`Duo` test, no `lossy`):

1. `write` 2048 bytes on a uni stream and pump — two packets, the second
   ending at 2048;
2. `finish()` **after** that pump, so `fin` is set with `write_offset`
   already drained: the FIN is now a bare obligation at offset 2048,
   never rideable on a data chunk that ends at 1160;
3. declare only the **first** packet lost, so `retransmit == 0..1160`
   while `1160..2048` stays in `unacked`;
4. pump. The retransmit repacks 0..1024 and 1024..1160 to exactly 1170
   bytes, and the bare FIN meets `room() == 0`.

**The separating assertions** — a name is not a pin, so both halves:

* **(debug, the fault itself)** the pump completes **without panicking**.
  This is the assertion the broken build fails, deterministically and
  with no seed search, under the `cargo test` gate. It has to be stated
  as the point of the test, because §4 proved the two builds are
  observationally identical in release — there is nothing else to
  observe.
* **(both profiles, against a wrong fix)** *the deferred FIN is emitted
  by the very next packet* — parse the next transmit and assert it
  carries a STREAM frame for that id with `FIN` set, `offset == 2048`,
  zero-length data — **and** that the stream reaches `DataSent`/the
  reader sees end-of-stream. A "fix" that defers by dropping the half out
  of the rotation (`set_queued(false)` instead of `push_front`), or one
  that forgets `return_chunk`'s `fin_sent = false` restoration, passes
  the no-panic half and hangs the peer for ever on this half. That is the
  degenerate build rule 9 asks for.

## 7. VERDICT

**Base.** `1395ea6`, clean tree, verified as the first command. No reset
needed.

**Reproduced?** **Yes**, on the first attempt, with the briefed recipe
verbatim, and in both orderings of the policy install. Also reproduced on
2 further seeds of a 64-seed sweep (3/64 ≈ 5 %).

**Root cause (one line).** `src/core/connection/streams.rs:1071` —
`packing.stream_payload_room(id, offset).unwrap_or(0)` collapses the
callee's `None` (*"not even an empty frame fits"*,
`src/core/connection/frame.rs:1057`) onto `Some(0)` (*"a frame fits with
zero payload"*), and the next line's guard rescues only
`has_data_pending()`, so a **bare FIN** owed against a packet already at
`used == budget == MAX_PLAINTEXT` is built into a 5-byte frame and
offered to a `push` that correctly refuses it
(`frame.rs:1104`, §8.6's per-seal bound).

**Which side matches the spec.** `Packing::fill`. §8.1's varint widths
agree on both sides; §8.6 is what `push` enforces; §9.5 makes the empty
FIN frame legal but says nothing about capacity. The caller is wrong.

**Release behaviour.** The "benign" claim is **verified and can be
strengthened**: `cargo test --release --all-features --test repro_1077`
passes with bytes intact, the arm fires **exactly once** per affected
transfer (never twice — the packet is full, so the next one starts
empty), `RangeSet::insert` rejects the empty range that would otherwise
have stranded the FIN, and a 64-seed pre/post-fix diff of virtual-time
completion instants is **byte-identical**. The broken build in release
and the fixed build are observationally indistinguishable. The defect is
exclusively a debug-driver panic.

**Fix.** One predicate at `streams.rs:1071-1072` → `1071-1083`
(`fits.is_none() || (room == 0 && send.has_data_pending())`). No wire
byte, no ratified constant, no timer. **No ruling needed** — this is
ruling 207(c)'s internal seam.

**Gates.** Reproducer green with the assert restored (3/3).
`cargo test` 880 passed / 0 failed. `cargo test --all-features` 1071
passed / 0 failed / 1 ignored. `cargo test --release --all-features`
1072 passed / 0 failed. `cargo fmt --check` no diff. `cargo clippy
--all-features --all-targets -D warnings` zero warnings. `cargo doc`
with `RUSTDOCFLAGS=-D warnings` clean. `cargo build --all-features
--all-targets` clean. Wire pins (`spec_constants` 112, `story_codec` 4)
pass. MSRV and `cargo deny` **not run** — not claimed.

**Also recorded, not fixed.** `stream_payload_room` does not consult
`Packing::extends_to_end` while `push` refuses everything once it is
set. Cleared as unreachable today by an arithmetic accident (a ¬LEN
DATAGRAM always leaves ≤ 1 byte, and `fixed >= 2`), and closed by this
fix in passing — but it is an unstated scope on a stated construction
(rule 8), and it stops being safe the moment a third extends-to-end
contributor or a ¬LEN STREAM emitter is added.

**Worktree artefacts** (nothing lands): `tests/repro_1077.rs` + its
`[[test]]` stanza; the fix in `src/core/connection/streams.rs`;
`pre-fix-sweep.txt` / `post-fix-sweep.txt` beside this report.

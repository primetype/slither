# Round 42, report D — the R42 implementer (ring + credit-derived ceiling)

*Opus implementer, worktree at `405796e`; commit `8b0788c`, cherry-picked as `ba50a33`. Verbatim report below.*

---

# Slice R42 — implementer report

Base commit (rule 14, verified as first act): `405796ef8179a45a9d6552f7979997cb62e52fe7`
(`405796e Round 42: the stall is a pre-existing reassembly kill, separated three ways`) — matches brief. Working tree clean at start.

Partition owned by this agent: `src/core/connection/send.rs`, `src/core/connection/recv.rs`,
`src/core/connection/streams.rs` (only if plumbing requires), `src/constants.rs` (ADDITIVE only),
`examples/bench_vs_tcp.rs`, this file. No `tests_*.rs` / `tests/` touched.

## Scope A — the drain fix (ruling 269(iii))

### The representation chosen: a **ring** (`VecDeque<u8>`), not an offset cursor

`SendHalf::buf` becomes `VecDeque<u8>`. The five sites report B enumerates are
the whole change, and `self.buf` still has no reader outside `send.rs`.

| site | change |
|---|---|
| `write()` | `Vec::extend_from_slice` → `VecDeque::extend(&data[..room])` (std's `Copy` specialisation is a `copy_slice`, one memcpy) |
| `reset()` / `stopped_by_peer()` | `Vec::new()` → `VecDeque::new()` — **no cursor to reset** |
| `slice()` → `copy_range()` | returns `Vec<u8>` built from `as_slices()`; at most two memcpys |
| `release()` | `Vec::drain(..drop)` (O(live) memmove) → `VecDeque::drain(..drop)`, which for a **front** drain is a head advance and moves no byte |
| `next_chunk()` | `self.slice(a,b).to_vec()` → `self.copy_range(a,b)` |

**Why the ring and not the mutant's cursor.** The brief allows either; the ring
is the one that needs no slack bound at all, because it *has* no slack:

* **Cost.** Cursor-with-slack is O(1) amortised only if the compaction
  threshold is proportional to the live size (a fixed absolute slack `S` costs
  `live/S` copies per released byte — still O(window)). At `α·live` the
  amortised cost is `1/α` byte-copies per released byte *and* the peak length
  is `(1+α)·live`: the two are traded against each other, and report B's
  caveat (d) already measures the residue (`the compaction still copies ~1 byte
  per released byte`). The ring pays **zero** copies per released byte.
* **Memory (ruling 94's discipline).** `buf.len()` stays **exactly** the live
  byte count — the same identity the shipped `Vec` had. The representation
  adds **no slack whatsoever**; peak allocation is bounded by the peak live
  count under the same geometric growth policy `Vec` already used. The
  mutant's 2× length (and up to 4× capacity after `Vec` doubling) is gone, and
  so is the need to name a slack constant.
* **Invariant surface.** Because `buf.len()` keeps its meaning, `release()`'s
  clamp `drop.min(self.buf.len())` keeps its meaning too, and the two reset
  paths need no second field zeroed. Report B's checklist collapses instead of
  being discharged item by item.

**The stated bound, as it appears in the rustdoc:** `buf.len() == write_offset
− base` at all times outside a reset — asserted by a `debug_assert!` in
`release()` (guarded on `reset.is_none() && peer_reset.is_none()`, because a
reset empties the buffer and leaves both offsets standing), so every debug
`cargo test` run checks it — and `write_offset − base` is
bounded by the stream's advertised credit, because `write()` is bounded by
`max_data − write_offset` and by `conn_room`. Slack: none. This is the same
`head`-plus-`advance()` shape `recv.rs`'s `Chunk` already has (report B §2.2's
final bullet), taken one step further: the receive half's chunk keeps a head
gap on purpose; the send half needs none.

**Report B §2.2's invariant checklist, discharged**

| invariant | status |
|---|---|
| `base` is the stream offset of the first live byte | unchanged — it is the ring's front |
| `slice(start,end)` returns exactly `[start,end)` | `copy_range` indexes from the front through `as_slices()`; out-of-range still panics |
| `release`'s clamp is against the **live** length | `buf.len()` **is** the live length (unlike the cursor) |
| `reset`/`stopped_by_peer` leave `base` and clear the buffer | `VecDeque::new()`; there is no second field to forget |
| retransmission reads ≥ `base`, < `write_offset` | unchanged; `copy_range` is the same arithmetic |
| `RangeSet`s, `write_offset`, `final_size`, `Chunk::offset` | untouched |

## Scope B — the credit-derived reassembly ceiling (ruling 270)

### The derivation

`ceiling(W) = max(REASSEMBLY_CHUNKS_MAX, W / P + 1)`, `W` = the stream window
this half advertises (`RecvHalf::with_window`, ruling 259(viii)),
`P = REASSEMBLY_MIN_CONFORMING_FRAME` (new, additive).

**Property guaranteed:** *a peer that never exceeds its advertised stream
credit and whose STREAM frames each carry at least `P` bytes cannot cross the
ceiling under any loss or reordering pattern.*

Proof sketch (post-ruling-253 gap-merge invariant):
1. Stored chunks are pairwise disjoint **and non-adjacent** — `insert` merges
   on adjacency, not merely on overlap — so a stored chunk is a maximal run of
   received bytes and the count is `holes + 1`.
2. Every stored chunk is the union of one or more received frames, so it is at
   least as large as the smallest frame that built it: ≥ `P` — **except** the
   front chunk, which `Reassembly::read` may have partially consumed
   (`Chunk::advance`). That is the `+ 1`.
3. Every stored byte lies in `[read_offset, read_offset + W)`: below is
   already delivered and `insert` drops it, above is a `FLOW_CONTROL_ERROR`
   before `insert` is reached.
4. Disjoint ranges of ≥ `P` bytes inside a `W`-byte span number at most
   `W / P`. With the partially-read front chunk: `W / P + 1` = the ceiling.

### Choosing `P` — and the trap in the brief's own suggestion

The brief suggests *"an existing ratified size … e.g. a full DATAGRAM
payload"*. `MAX_DATAGRAM_PAYLOAD` is **1169**, and choosing it would have made
the property **false of slither's own sender**: `streams.rs:1162` fills a
STREAM frame with `room.min(STREAM_FILL_QUANTUM)` and `STREAM_FILL_QUANTUM`
(`frame.rs:58`) is **1024**, so slither's own saturating sender never emits a
STREAM frame larger than 1024 bytes. `P` must be ≤ the largest frame a
conforming sender is *guaranteed* not to exceed downward, so `P = 1024`.
(Rule 11: the mechanism was checked against the code, not against the spec's
packet sizes.)

`P` is a **new constant in `constants.rs`**, not a reference to
`STREAM_FILL_QUANTUM`: the quantum is deliberately implementation-defined and
kept out of the constants table (`frame.rs:52-58`), it is the **local
sender's** choice, and a receiver policy must not be defined by it. The two
happen to agree at 1024, and the doc comment says so.

### Numbers

| stream window | `W / P + 1` | ceiling in force | note |
|---|---|---|---|
| 256 KiB (ratified default) | 257 | **1024** (floor) | unchanged — the floor binds |
| 1 MiB | 1 025 | 1 025 | |
| 4 MiB | 4 097 | 4 097 | round42-C measured 946 needed at 4 MiB |
| 8 MiB (the failing cell) | **8 193** | 8 193 | round42-C measured ≥1 025 needed; the alternating-drop bound is ~3 700 |

Separation from the flood: the adversarial bound is `W/2` ranges (1-byte
frames at even offsets). `W/1024` is **512×** below it, and the tiny-fragment
flood still dies exactly as ratified — the disposition is unchanged.

### Memory consequence (for §17.5's ceiling row)

Per-chunk metadata is `size_of::<Chunk>()` = 40 B (`offset: u64`, `data:
Vec<u8>`, `head: usize`) in a `VecDeque<Chunk>`. Worst case per stream:

* 256 KiB window: 1 024 × 40 B ≈ **40 KiB** metadata against 256 KiB credit —
  today's figure, unchanged.
* 8 MiB window: 8 193 × 40 B ≈ **320 KiB** metadata against 8 MiB credit —
  **3.8 %** of the credit already committed, i.e. the credit term still
  dominates, which is what §17.5's row asserts.

## Scope C — bench harness defects (round42-C)

`BulkState` gains `failure: Rc<RefCell<Option<String>>>` with `fail()` (first
cause wins) and `failed()`.

1. **A dead cell no longer idles.** Every loop that watched `done` now watches
   `failed()` as well — `done` is the *meter's* flag and a dead cell never
   satisfies the meter, which is the whole mechanism behind round 42's six
   minutes at 0 % CPU. `write_all` now returns `Result<(), String>` instead of
   `bool`, so the diagnosis is carried out rather than swallowed; the reader
   loop separates `Ok(None)` (the writer's clean FIN) from `Err` and records
   the latter. The TCP cell got the same treatment on both sides.
2. **`BulkState::finish()` fails the cell.** It now asserts that no failure was
   recorded **and** that the meter collected every window, naming the count and
   the cause. A short `BENCH` line is not a slow result, it is no result.

**Demonstrated, not asserted** — the separating probe below produced exactly
this, on the first cell that died:

```
BENCH scenario=sweep-rtt ... windows=2Mi/8Mi streams=1 n=5 p50=9.19 ...
thread 'main' (5855198) panicked at examples/bench_vs_tcp.rs:790:9:
the cell died after 0 of 5 windows: read failed: torn down after a protocol violation: code 1
```

Before this change that same cell was six minutes of silence; now it names the
protocol error, the window count, and the line number, and stops.

## Scope D — drafted spec text (REPORT ONLY, SPEC.md untouched)

`SPEC.md` was **not** modified — the commit contains four files, none of them
it.

### (a) The §10.6 amendment paragraph

**Insertion point.** `grep -n 'REASSEMBLY_CHUNKS_MAX' SPEC.md` puts §10.6's
clause at 4395 and its revisit hook at 4397-4399; the new paragraph goes at
**line 4400**, as a new paragraph between the *"second bound"* paragraph and
ruling 253's amendment. Surrounding lines, quoted:

```
4394  insert, and a stream whose stored discontiguous ranges would exceed
4395  `REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol
4396  violation: CLOSE with `PROTOCOL_VIOLATION` (§15.3; quinn's
4397  defragment-plus-hard-fail shape). The ceiling value ships
4398  ratified-but-revisitable, gated on the Appendix B
4399  defragmentation/throughput check.
4400  <- INSERT HERE (blank line, the paragraph below, blank line)
4401  **[AMENDED 2026/08/17 — ruling 253]** **The mandate above bounds state;
4402  this clause bounds work.** Coalesce-on-insert's total copy work per
```

> **[AMENDED 2026/08/18 — ruling 270]** **The ceiling is derived from the
> advertised credit, and `REASSEMBLY_CHUNKS_MAX` is its floor.** A receiver
> tolerates `max(REASSEMBLY_CHUNKS_MAX, W / P + 1)` stored discontiguous
> ranges per stream, where `W` is the stream window it advertises and `P` is
> `REASSEMBLY_MIN_CONFORMING_FRAME` (1024 B — receiver policy, observable, the
> same third kind as the ceiling itself). The flat value was **stricter than
> this section's own mandate**, which is already stated as *O(advertised
> credit)*, and the strictness is what killed honest peers: a stream's credit
> and its tolerated hole count were two constants that did not scale together,
> so at a raised window a sender **inside its credit**, on a path losing
> packets in the pattern a saturated receive socket produces, exceeded the
> second while obeying the first. The property the derivation buys — and the
> reason the divisor is packet-scale rather than 2 — is this: **a peer that
> never exceeds its advertised credit and whose STREAM frames each carry at
> least `P` bytes cannot cross the ceiling under any loss or reordering
> pattern.** Stored ranges are maximal runs, since they coalesce on
> *adjacency* and not merely on overlap, so each is at least one frame wide
> except the partially-read front one, and disjoint ranges of at least `P`
> bytes inside a `W`-byte span number at most `W / P`. **The disposition does
> not change.** Crossing the ceiling is still CLOSE with `PROTOCOL_VIOLATION`
> (§10.5's third violation), and this section's own worked flood — one-byte
> frames at offsets 0, 2, 4, …, some `W / 2` ranges — sits **512×** above the
> derived ceiling and still dies there. That boundary is the point of the
> change and not a casualty of it. This is the revisit the paragraph above
> reserved: the Appendix B defragmentation/throughput check was run at a
> raised window and came back **fatal rather than slow**.

### (b) The §17.5 ceiling-row adjustment (one sentence)

In the *established connections* row (SPEC.md:6941), replace

> … plus per-stream book-keeping and reassembly metadata bounded by
> `REASSEMBLY_CHUNKS_MAX` (§10.6 — the second bound is what makes the credit
> term the dominant term rather than a 25–50× underestimate) …

with

> … plus per-stream book-keeping and reassembly metadata bounded by §10.6's
> ceiling — `REASSEMBLY_CHUNKS_MAX` at the ratified window and
> `window / REASSEMBLY_MIN_CONFORMING_FRAME + 1` above it (ruling 270), so at
> ~40 B per stored range the metadata is ~40 KiB against 256 KiB of credit at
> the ratified window and ~320 KiB against 8 MiB at a raised one: under 4 %
> either way, which is what keeps the credit term dominant rather than a
> 25–50× underestimate …

### (c) The ruling-269 addendum (one sentence, plus its evidence)

> **Addendum (ruling 270).** 269(ii)'s surviving guidance — *"size ≈ 2 × RTT ×
> target rate, and not larger"* — was, when it shipped, advice that steered
> operators into a fatal region: at 100 ms any target above ~22 MiB/s
> recommends a stream window past the ~4.5 MiB crossing where §10.6's flat
> 1024-range ceiling killed a conforming connection. With 270's credit-derived
> ceiling the hazard is gone by construction — the ceiling now scales with the
> very window the advice sizes — so **the sizing advice is now safe at any
> raise**, and the measured 100 ms optimum has moved with it: 269 recorded
> 2 MiB/8 MiB at 9.01 MiB/s as this host's best, and on the same host the
> ladder now rises monotonically to **17.66 MiB/s at 8 MiB/16 MiB**.

### (d) Rule-4 sweep — every site still stating the flat ceiling

`SPEC.md` (**none touched** — all for the maintainer):

| line | what it says | needs |
|---|---|---|
| 4172 | *"`REASSEMBLY_CHUNKS_MAX` (§10.6) is a **third** kind … The values and their locations do not move"* | there are now **two** constants in that class; `REASSEMBLY_MIN_CONFORMING_FRAME` joins it |
| 4345 | §10.5's third violation, stated as *"would exceed `REASSEMBLY_CHUNKS_MAX` after coalescing"* | should read *"§10.6's ceiling"* — **and see conflict 3 about the rest of that sentence** |
| 4395 | §10.6's own clause, *"(= 1024)"* | the drafted paragraph qualifies it two lines later, but the parenthetical still reads as the whole rule |
| 4418 | §10.6's constant table row | add the derivation and the new constant |
| 6941 | §17.5's ceiling row | (b) above |
| 7424 | Appendix B: *"stays O(credit) or dies at `REASSEMBLY_CHUNKS_MAX`"* | still true of the flood; the name is now the **floor** |
| 7889 | Appendix B obligation O53b, *"before the §10.2 constants and `REASSEMBLY_CHUNKS_MAX` ratify"* | **this is the gate round 42 ran and found red** — it should record that it has now been run and what it returned |
| 7918 | the consolidated named-constants table row | add a row for `REASSEMBLY_MIN_CONFORMING_FRAME` |

`src/`, outside my partition (**not touched**):

| site | what it says | needs |
|---|---|---|
| `src/constants.rs:323` | `REASSEMBLY_CHUNKS_MAX`'s own doc: *"Distinct out-of-order chunks the reassembler will hold per stream"* | it is now the **floor**; my brief scopes `constants.rs` to *additive only*, so I put the correction in the new constant's doc directly beneath it and left this line alone |
| `src/core/connection/flow.rs:8,11` | module doc: *"reassembly ranges exceeding `REASSEMBLY_CHUNKS_MAX`"* | now the derived ceiling |
| `src/core/connection/flow.rs:90` | **the error string itself**: `#[error("protocol violation: reassembly ranges exceed REASSEMBLY_CHUNKS_MAX")]` | **user-visible and now inaccurate** — see conflict 4 |

`src/` inside my partition — swept: `recv.rs`'s module doc (17, and a new
section at 21-33) and the check site (943) both restated. `recv.rs`'s two
inline ceiling tests (1047, 1051) use `RecvHalf::new()`, i.e. the ratified
window, where the floor binds and their assertions stay exactly true.

Test files (the author's / the integrator's, **not touched**), all still
correct because they all run at the ratified window where the floor binds:
`src/core/connection/tests_reassembly.rs:144,661,675,853`,
`src/core/connection/tests_streams.rs:1436-1491`,
`tests/spec_constants.rs:444,498,501`, `tests/story_streams.rs:254`,
`tests/spec_ack_burst.rs:7`.

## Gates + bench acceptance

Every command below was run on the committed tree, and the tree was verified
byte-identical to the commit afterwards (`git status --porcelain | wc -l` → 0,
`git diff HEAD --stat | wc -l` → 0).

```
$ cargo fmt --all --check
(no output)

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/.../agent-af58e34baf6c6aed8)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.02s

$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.85s

$ cargo test --all-features 2>&1 | grep -E "^test result" | awk -F'[ ;]' '{p+=$4; f+=$6} END {print "PASSED:", p, "FAILED:", f}'
PASSED: 1095 FAILED: 0

$ cargo test 2>&1 | grep -E "^test result" | awk ...
no-features PASSED: 898 FAILED: 0

$ cargo test --release --all-features 2>&1 | grep -E "^test result" | awk ...
release PASSED: 1097 FAILED: 0
```

*(1097 in release, 1095 in debug: `tests/story_rekey.rs:218` and
`tests/spec_ack_burst.rs:414` are `#[cfg_attr(debug_assertions, ignore)]`.
Pre-existing; `grep -rn debug_assertions src/ tests/` shows those two sites
and no others.)*

**No test file was created, modified or deleted.** All 1095 are the ones that
were there at `405796e`.

### The wire pins, explicitly

```
$ cargo test --all-features --test spec_constants
test result: ok. 112 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test --all-features --test spec_packet --test spec_compat --test spec_rekey
running 24 tests
test result: ok. 24 passed; 0 failed; ...
running 4 tests
test result: ok. 4 passed; 0 failed; ...
running 5 tests
test result: ok. 5 passed; 0 failed; ...

$ cargo test --all-features golden   (golden-named tests only)
test packet::tests::golden_canonical_static ... ok
test packet::tests::golden_mac1_label ... ok
test packet::tests::golden_init_header ... ok
test packet::tests::golden_data_header ... ok
test packet::tests::golden_msg1_payload ... ok
test packet::tests::golden_resp_header ... ok
test packet::tests::golden_prologue ... ok
test packet::tests::msg1_payload_matches_the_golden_vector ... ok
test packet::tests::sizes_match_the_golden_vectors ... ok
test packet::tests::golden_mac1 ... ok
test packet::tests::mac1_tag_matches_the_golden_vectors ... ok
test packet::tests::mac1_key_matches_the_golden_vector ... ok
```

Byte-identical. Nothing in this slice reaches a frame, a header, a timer or a
wire constant: the send change is a container swap behind a private field, and
`REASSEMBLY_CHUNKS_MAX` did not move (it is now the ceiling's floor, and
`spec_constants.rs:501`'s `assert_eq!(REASSEMBLY_CHUNKS_MAX, 1024)` still
passes unmodified).

### The rest of the release table

```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated .../target/doc/slither/index.html

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated .../target/doc/slither/index.html

$ cargo +1.96 check --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.19s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

### THE BENCH

**Provenance of the before column.** I could not re-measure it: reverting the
core files needs `git checkout <base> -- src/...`, which this environment
refused. The before column is therefore round42-B's, which states it
re-measured both columns back-to-back **on this host** at `d8bb652`. My after
column is a different session on the same host, so cell-to-cell drift of a few
percent is expected and visible (B's default rung 93.30, mine 83.10).

#### `cargo run --release --example bench_vs_tcp -- sweep` — zero RTT ladder

```
BENCH scenario=sweep proto=slither rtt_ms=0 windows=default    streams=1 n=5 p50=83.10 min=78.08 max=90.14 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1562811,dg_in=1562809,ack_dg=791101,mtu=1179
BENCH scenario=sweep proto=slither rtt_ms=0 windows=512Ki/2Mi  streams=1 n=5 p50=79.69 min=78.53 max=80.21 unit=MiB/s note=amp=1.043,loss=-0.0000,dg_out=1464115,dg_in=1464117,ack_dg=737088,mtu=1188
BENCH scenario=sweep proto=slither rtt_ms=0 windows=1Mi/4Mi    streams=1 n=5 p50=96.63 min=92.80 max=97.83 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1769454,dg_in=1769430,ack_dg=888009,mtu=1186
BENCH scenario=sweep proto=slither rtt_ms=0 windows=2Mi/8Mi    streams=1 n=5 p50=96.54 min=95.44 max=97.70 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1780590,dg_in=1780547,ack_dg=892006,mtu=1188
BENCH scenario=sweep proto=slither rtt_ms=0 windows=8Mi/16Mi   streams=1 n=5 p50=95.49 min=95.44 max=96.87 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1768829,dg_in=1768829,ack_dg=884952,mtu=1188
BENCH_DONE wall_s=110.1
```

| stream/conn window | before (r42-B) | **after (this tree)** | × |
|---|---|---|---|
| 256Ki/1Mi (default) | 70.66 | **83.10** | 1.18 |
| 512Ki/2Mi | 51.44 | **79.69** | 1.55 |
| 1Mi/4Mi | 34.71 | **96.63** | 2.78 |
| 2Mi/8Mi | 20.33 | **96.54** | 4.75 |
| 8Mi/16Mi | 5.74 | **95.49** | **16.6** |

**Flat-or-rising, and the bar is met.** Top-to-bottom spread: **12.3× before,
1.21× after**, with the top four rungs at 80–97 MiB/s and the ladder rising,
not falling. The one dip (512Ki at 79.69 against default's 83.10, −4 %) is
inside the default rung's own min/max spread (78.08–90.14) and is the same
shape B's after column has (89.89 against 93.30). Amplification is 1.043–1.044
and measured loss 0.0000 in every cell, so none of it is retransmission.

#### `-- bulk`

```
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default   streams=1 n=5 p50=83.20 min=81.31 max=97.65 unit=MiB/s note=amp=1.044,loss=0.0000,...
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi  streams=1 n=5 p50=96.18 min=95.24 max=98.32 unit=MiB/s note=amp=1.044,loss=0.0000,...
BENCH scenario=bulk proto=tcp     rtt_ms=0 windows=kernel-default streams=1 n=5 p50=10983.47 min=10890.17 max=11102.37 unit=MiB/s
BENCH_DONE wall_s=66.0
```

Against B's 72.52 (default) and 90.24 (raised): **+15 %** and **+7 %**. **The
window knob's sign is flipped and stays flipped** — raising to 8Mi/16Mi was
12.7× *slower* at `405796e` and is now 1.16× *faster*.

#### `-- sweep-rtt` — the cell that was gating the slice

```
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=default   streams=1 n=5 p50=1.43  min=1.41  max=1.44  note=amp=1.039,loss=-0.0006,...
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=512Ki/2Mi streams=1 n=5 p50=2.55  min=2.54  max=2.55  note=amp=1.040,loss=0.0003,...
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=1Mi/4Mi   streams=1 n=5 p50=4.71  min=4.70  max=4.72  note=amp=1.041,loss=0.0009,...
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=2Mi/8Mi   streams=1 n=5 p50=9.09  min=9.04  max=9.17  note=amp=1.040,loss=-0.0000,...
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=8Mi/16Mi  streams=1 n=5 p50=17.66 min=16.81 max=18.36 note=amp=1.038,loss=-0.0022,dg_out=318162,dg_in=318864,ack_dg=159560,mtu=1200
BENCH_DONE wall_s=112.9
```

| window | baseline (r42-B) | mutant (r42-B) | **this tree** |
|---|---|---|---|
| 256Ki/1Mi | 1.44 | 1.45 | **1.43** |
| 512Ki/2Mi | 2.51 | 2.57 | **2.55** |
| 1Mi/4Mi | 4.59 | 4.80 | **4.71** |
| 2Mi/8Mi | 9.04 | 9.39 | **9.09** |
| 8Mi/16Mi | 5.59 *(fell)* | **STALLED** | **17.66** ✅ |

**The 8Mi/16Mi @ 100 ms cell completes — n=5, no kill, no stall — and the
100 ms column rises monotonically with the window for the first time**
(1.43 → 2.55 → 4.71 → 9.09 → 17.66). The optimum moved off 2Mi/8Mi, which is
what makes ruling 269(ii)'s sizing advice usable at a raise. The four lower
rungs are within ±2 % of baseline, exactly as B predicted: below 2 MiB at
100 ms the window binds, not the CPU.

### The separating measurement (rule 9 — what the degenerate build does)

The bench numbers above pass with **both** changes in. To show the ceiling is
load-bearing and not carried by the drain fix, I forced `ceiling_for` to
return the floor — the ring send half, the flat 1024 ceiling — rebuilt, and
re-ran `sweep-rtt`:

```
BENCH scenario=sweep-rtt ... windows=2Mi/8Mi streams=1 n=5 p50=9.19 ...
thread 'main' (5855198) panicked at examples/bench_vs_tcp.rs:790:9:
the cell died after 0 of 5 windows: read failed: torn down after a protocol violation: code 1
```

Three things at once:

* **Scope B is necessary.** With the shipped (non-mutant) send-half
  representation and the flat ceiling, the 8 MiB/100 ms cell dies with
  `PROTOCOL_VIOLATION`. The drain fix alone does **not** fix that cell — it
  rate-enables the kill, which is round42-C's verdict reproduced
  independently, on a tree that has never contained the mutant.
* **Scope B is sufficient**, together with A: restoring the derived ceiling is
  the only difference between that panic and the 17.66 MiB/s run above.
* **Scope C works.** The same failure that was six minutes of 0 % CPU in
  round 42 is now one line naming the protocol error and the window count.

The probe was reverted by hand and the tree verified identical to the commit
(0 files modified, 0 diff lines), and `cargo test --all-features` re-run:
1095 passed, 0 failed.

## Conflicts and findings (reported, not resolved — rules 3, 5, 11)

**1 — The brief's suggested `P` is refuted by the code. I did not follow it.**
The brief says *"pick P as an existing ratified size if one fits, e.g. a full
DATAGRAM payload, and name it"*. `MAX_DATAGRAM_PAYLOAD` is **1169**, and
choosing it would have made ruling 270's property **false of slither's own
sender**: `streams.rs:1162` fills a STREAM frame with
`room.min(STREAM_FILL_QUANTUM)`, and `STREAM_FILL_QUANTUM` (`frame.rs:58`) is
**1024**, so no slither sender ever emits a STREAM frame larger than 1024
bytes. A `W / 1169` ceiling would be below what slither's own saturating
sender can legitimately produce, and the property would have read as a
guarantee while guaranteeing nothing about the only sender we ship. I chose
`P = 1024` and gave it its own constant. **This is the maintainer's to
confirm** — it changes the ceiling by 14 % and it is the one number in the
derivation that a ruling should own.

**2 — The property is conditional, and its scope should be stated (rule 8).**
*"Frames ≥ P"* is not a property of every conforming sender, including ours.
`next_chunk(room.min(STREAM_FILL_QUANTUM))` emits a **sub-`P` frame whenever
the packet's residual room is smaller than a quantum** — an ACK frame sharing
the packet, or a second stream taking the tail of a multiplexed one. A stream
whose frames were *all* residues would have its run count bounded by
`W / residue`, not `W / P`, and could exceed the derived ceiling while inside
its credit. It cannot happen for the saturating single-stream case (every pass
gives the stream a full quantum), and the ratified-window floor of 1024 covers
the ordinary multiplexed case, but **the guarantee has an unstated edge and I
am naming it rather than letting the rustdoc imply it is universal.** Lowering
`P` to cover it costs proportionally: `P = 128` puts the 8 MiB ceiling at 65 k
chunks ≈ 2.6 MB of metadata (32 % of the credit) and cuts the flood separation
from 512× to 64×. My judgement is that 1024 plus the floor is the right trade,
but the trade is a ruling, not an implementation choice.

**3 — §10.5's rationale for calling the ceiling a tolerance is weakened.**
`SPEC.md:4342-4348`: *"It is a tolerance and not an exact limit — **the sender
cannot compute it**, since it depends on this receiver's coalescing and on the
arrival order the network produced."* With ruling 270 the ceiling is a
function of the window the receiver **advertises**, so the sender now knows
one of its two inputs exactly. The claim survives in the letter — `P` is
receiver policy and is never advertised — but the reason given is a step
weaker than it was, and rule 4(a) says the other clauses of that sentence
should be read when one is corrected. Flagged, not resolved.

**4 — `Violation::Reassembly`'s user-visible error string is now inaccurate.**
`src/core/connection/flow.rs:90`:
`#[error("protocol violation: reassembly ranges exceed REASSEMBLY_CHUNKS_MAX")]`.
That constant is now the ceiling's **floor**, so at a raised window the message
names a number the peer did not exceed. `flow.rs` is outside my partition and I
did not touch it; it is a one-line fix for the integrator, and the module doc
two screens above it (`flow.rs:8,11`) needs the same sweep.

**5 — `REASSEMBLY_CHUNKS_MAX`'s own doc comment is stale, and I left it that
way deliberately.** `src/constants.rs:323` still reads *"Distinct out-of-order
chunks the reassembler will hold per stream"*, which is now the floor rather
than the count. My brief scopes `constants.rs` to **additive only**, so rather
than edit a ratified constant's doc I put the correction in the new constant's
doc directly beneath it and am reporting the site. If the maintainer wants the
existing doc amended, it is one sentence.

**6 — Not a conflict, a caveat on the numbers.** The 90–97 MiB/s ceiling is
this host's and this topology's (both endpoints, both AEAD directions, acks and
recovery on one core). It is not a protocol constant. Report B says the same
thing and it is worth repeating before anyone quotes it.

**7 — What I did not do.** `streams.rs` was **not** modified: the window
already reaches `RecvHalf::with_window` (`streams.rs:130,133`), so scope B
needed no plumbing. No `tests_*.rs` or `tests/` file was created, modified or
deleted, and no commented-out `// INTEGRATION:` module declaration was needed,
because nothing I wrote requires a module that does not yet exist.


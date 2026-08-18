# Round 42, report B — the drain attribution: measurement mutant

*Opus agent, worktree at `d8bb652`; THROWAWAY mutant commits preserved
on branch `mutant/drain-attribution` — reference only, DO NOT LAND.
All 1095 tests pass under the mutant unmodified; wire pins byte-identical.
Verbatim report below.*

---

# Round 42 item 1 — drain-attribution MEASUREMENT MUTANT

**THROWAWAY. DO NOT LAND.** Base commit: `d8bb65214d5fcd69b412c64446a13ca326c714ed` (d8bb652), verified by `git rev-parse HEAD` as first act (rule 14).

## 0. Provenance / base verification

`git rev-parse HEAD` in the worktree, as the first command of the session:

```
d8bb65214d5fcd69b412c64446a13ca326c714ed
d8bb652 Round 42 item 1: the quinn cross-measurement anchors, and a second hypothesis
```

Matches the briefed base. Working tree clean at that point.

## 1. RTT probe

**Headline: perceived `srtt` at the default window is 2.27 ms and `min_rtt` is
0.25 ms — milliseconds, not tens of microseconds, so the hypothesis is not dead
on its face. But the inflated figure is an *effect* of the ack-path cost, not a
cause of the 73 MiB/s, and it explains none of the ceiling causally.**

### 1.1 Where the estimator lives

`RttEstimator`, `src/core/connection/recovery.rs:694-806` — `latest`, `smoothed`,
`rttvar`, `min_rtt`, exactly SPEC §13.1. Accessors `smoothed_rtt()` (:763),
`min_rtt()` (:773). Reachable from `core::Connection` only through the
`#[cfg(test)]` `recovery()` / `smoothed_rtt()` accessors at
`src/core/connection/mod.rs:1296-1305`, which an `examples/` target cannot see.

### 1.2 How it was exposed (throwaway)

Three edits, all marked `MEASUREMENT MUTANT — DO NOT LAND`:

1. `src/core/connection/mod.rs` — `pub(crate) fn probe_rtt(&self) -> (Duration,
   Option<Duration>, u64, u64)` returning `(smoothed_rtt, min_rtt,
   congestion_window, bytes_in_flight)`, **not** `cfg(test)` gated.
2. `src/shell/connection.rs` — `pub fn probe_rtt(&self) -> Option<...>`, a
   synchronous read of the shared cell (`self.cell.borrow().core`), the same
   shape as the existing `is_established()`.
3. `examples/bench_vs_tcp.rs` — a sampler task inside `slither_bulk` that reads
   the **sender's** handle every 200 ms from the end of `BULK_RAMP` to the last
   counted byte, and a `probe_report` that prints one `PROBE ...` line to
   **stderr** per cell (stdout's `BENCH ` lines stay machine-greppable).

### 1.3 Numbers

Baseline (unmutated core), `--release`, clean loopback, one stream, one thread
carrying both endpoints. ~100 readings per cell, one per 200 ms of the timed
section, taken on the **sender's** handle. `cwnd`/`inflight` in bytes.

| windows | srtt p50 | srtt min | srtt max | min_rtt | cwnd p50 | inflight p50 | inflight max | MiB/s |
|---|---|---|---|---|---|---|---|---|
| default 256Ki/1Mi | 2.271 ms | 0.769 ms | 2.504 ms | **0.251 ms** | 944 MB | 146 026 | 197 555 | 72.52 |
| raised 8Mi/16Mi | **974.99 ms** | 663.5 ms | 1369.6 ms | **0.110 ms** | 77 MB | 7 106 465 | 8 691 915 | 5.73 |

The congestion window is never the binding constraint at zero RTT (`cwnd` is
three orders of magnitude above `inflight` in both rows).

### 1.4 Verdict

**The perceived RTT is milliseconds, not tens of microseconds — so the
hypothesis is not dead on arrival — but the causality runs the other way. The
inflated RTT is an *effect* of the ack-path cost, not a cause of the 73 MiB/s.**

The arithmetic first. At the measured perceived `srtt` of **2.271 ms** and a
256 KiB stream window, ruling 269(ii)'s rule `W / (2 × RTT)` predicts
**55.0 MiB/s**; the looser `W / RTT` predicts **110.1 MiB/s**. The measured cell
is 72.5 MiB/s (`bulk`) / 70.7 MiB/s (`sweep`), i.e. **0.66 × W/RTT** — inside
the band, above ruling 269's observed 0.44–0.57 ×. So on its face the default
cell *is* numerically consistent with being window-limited at the perceived RTT.

Three measurements refute the causal reading:

1. **`min_rtt` is 0.078–0.251 ms.** That is what co-scheduling both endpoints
   on one thread actually costs — a couple of hundred microseconds, not
   milliseconds. `W / (2 × min_rtt)` is 500–1600 MiB/s, nowhere near binding.
2. **`srtt` is not a property of the harness — it is a function of the
   window.** Across the baseline ladder it is 2.3 / 5.2 / 15.9 / 51.8 /
   969 ms at 256Ki / 512Ki / 1Mi / 2Mi / 8Mi. A fixed co-scheduling delay
   cannot do that. What it *is* doing is tracking `bytes_in_flight ÷
   throughput` — a standing queue, Little's law with a saturated sender:
   146 026 B ÷ 72.52 MiB/s = 1.92 ms against `srtt` 2.27 ms; 7 106 465 B ÷
   5.73 MiB/s = 1.18 s against `srtt` 0.975 s.
3. **The mutant moves it.** Changing only the send buffer's *representation* —
   nothing about scheduling, threads, or the wire — drops `srtt` at the default
   window from 2.271 to 1.411 ms and at 8 MiB from 968.8 to **42.3 ms**, while
   throughput rises. An independent cause cannot be moved by a buffer layout.

So: **none** of the 73 MiB/s is explained by an independently-inflated RTT. The
numerical fit at the default window is circular — `R` is set by the rate, not
the rate by `R`. The congestion window is never binding either (`cwnd` p50 is
0.9–1.2 GB against `inflight` of 0.15–8.5 MB in every cell).

## 2. Mutant design

**Headline: the ladder is flattened. A 12.3× monotone fall before, 1.06× after;
the top rung goes 5.74 → 94.36 MiB/s. The drain attribution is confirmed, and
it accounts for the whole of the falling ladder.**

### 2.1 What changed

`src/core/connection/send.rs`, `SendHalf` only — no frame layout, no constant,
no wire byte. `buf: Vec<u8>` gains a sibling `head: usize`: the live bytes are
`buf[head..]` and `base` now lives at `buf[head]` rather than `buf[0]`.

* `release()` — was `self.buf.drain(..drop)`, an O(live) memmove on **every**
  acked STREAM frame. Now `self.head += drop`, plus one `copy_within` +
  `truncate` compaction, taken only when `head >= buffered()` — i.e. once the
  dead prefix is at least as large as the live tail. A compaction therefore
  copies no more bytes than have been released since the previous one, so the
  cost is O(1) amortised per released byte.
* `slice(start, end)` — both indices are offset by `head`.
* `write()` — untouched (`extend_from_slice` still appends).
* `reset()` / `stopped_by_peer()` — `buf = Vec::new()` gains `head = 0`.
* new private helper `buffered() = buf.len() - head`, used by `release`'s clamp
  (which was `drop.min(self.buf.len())`) and by the compaction test.

### 2.2 Invariants preserved

* `base` remains the stream offset of the first live byte; every caller-visible
  offset (`write_offset`, `final_size`, the `RangeSet`s, `Chunk::offset`) is
  untouched. The mutant changes **where in `buf` that offset is stored**, and
  nothing else.
* `slice(start, end)` must still return exactly `[start, end)` of stream data —
  hence the `+ head` on both indices, not just the low one.
* `release`'s clamp was against `buf.len()`; it must now be against the **live**
  length, or a release could advance `base` past `write_offset`.
* `reset` / `stopped_by_peer` discard the buffer while leaving `base` where it
  was; `head` must go back to 0 with the `Vec`, or `slice` would index past the
  end on the next write.
* Retransmission reads below `write_offset` but never below `base` (only the
  contiguous acked prefix is released, and `on_lost_range` subtracts `acked`
  before re-queuing), so `head + (start - base)` is always in range. This
  invariant is the shipped code's too — the mutant does not add it.
* **This is the shape the receive half already has.** `recv.rs`'s reassembly
  `Chunk` carries its own `head` and `advance()` is `self.head += n`
  (`src/core/connection/recv.rs:617-621`), with ruling 253's small-to-large
  merge on top. The send half was the outlier.

### 2.3 Readers of `buf` that had to be adjusted

`buf` is **entirely private to `SendHalf`** — `grep -rn '\.buf\b' src/core/`
returns exactly five sites in `send.rs` (plus two unrelated ones in
`frame.rs`'s own decoder). Nothing in `streams.rs`, the frame-fill path, or the
shell ever touches it, and no test names it. The five:

| site | line (base) | change |
|---|---|---|
| `write` | 280 | none — append is still append |
| `reset` | 329 | `head = 0` added |
| `stopped_by_peer` | 352 | `head = 0` added |
| `slice` | 576 | both indices `+ head` |
| `release` | 592-593 | clamp uses `buffered()`; drain → cursor + compaction |

## 3. Ladder before/after (`sweep`)

`cargo run --release --example bench_vs_tcp -- sweep` — one stream, clean
loopback, rtt 0 ms, 5 timed 4 s windows per cell after a 2 s ramp. **Both
columns were re-measured in this session on this host**, back to back, rather
than taking the briefed figures — the briefed baseline reproduces (70.65 →
70.66, 20.22 → 20.33, 5.69 → 5.74).

| stream/conn window | before (MiB/s) | after (MiB/s) | × | before `srtt` p50 | after `srtt` p50 |
|---|---|---|---|---|---|
| 256Ki/1Mi (default) | 70.66 | **93.30** | 1.32 | 2.303 ms | 1.411 ms |
| 512Ki/2Mi | 51.44 | **89.89** | 1.75 | 5.184 ms | 2.734 ms |
| 1Mi/4Mi | 34.71 | **95.21** | 2.74 | 15.889 ms | 5.318 ms |
| 2Mi/8Mi | 20.33 | **95.09** | 4.68 | 51.781 ms | 10.677 ms |
| 8Mi/16Mi | 5.74 | **94.36** | **16.44** | 968.755 ms | 42.306 ms |

Spread top-to-bottom: **12.3× before, 1.06× after.** Amplification is flat at
1.040–1.044 and measured loss is 0.0000 in every cell of both columns, so no
part of either column is retransmission.

**The ladder is flattened.** The monotone fall is gone; what is left is a
window-independent ceiling of 90–95 MiB/s, which is what `T ≈ a + b·w` called
`a` (the fit put `1/a` at 109 MiB/s; measured 93–95, so the intercept was
mildly optimistic or a few percent of window-dependent cost survives — the
compaction still copies ~1 byte per released byte).

## 4. Scenario 2 (clean bulk, default + raised windows + TCP reference)

## 5. `sweep-rtt` (100 ms ladder) before/after

`cargo run --release --example bench_vs_tcp -- sweep-rtt` — one stream through
the delaying relay at 100 ms RTT.

| stream/conn window | before (MiB/s) | after (MiB/s) | Δ |
|---|---|---|---|
| 256Ki/1Mi (default) | 1.44 | 1.45 | +0.7 % |
| 512Ki/2Mi | 2.51 | 2.57 | +2.4 % |
| 1Mi/4Mi | 4.59 | 4.80 | +4.6 % |
| 2Mi/8Mi | **9.04** (optimum) | **9.39** (optimum) | +3.9 % |
| 8Mi/16Mi | 5.59 | **STALLED — see §8** | — |

**At 100 ms the drain is not the binding constraint below 2 MiB, and the fix
buys almost nothing there.** The optimum does **not** move: it is still
2Mi/8Mi, and the best achievable at 100 ms becomes **9.39 MiB/s** against 9.04
before — a 4 % gain, well inside what a host reboot would move. That is exactly
what the model says it should be: at 100 ms the RTT-limited ceiling
`W/(2·RTT)` is 10.0 MiB/s at a 2 MiB window, and the measured 9.39 is 0.47 ×
`W/RTT`, right inside ruling 269's 0.44–0.57 band. The window, not the CPU,
is what binds these cells.

The 8 MiB rung is the exception, and it did not get faster — it stopped
finishing at all. §8(a).

The four completed rungs were measured **twice** under the mutant (the second
pass is the stall-reproduction run): 1.45 / 2.57 / 4.74 / 9.17 against 1.45 /
2.57 / 4.80 / 9.39. Cell-to-cell repeatability is ~2 %, which is the same
order as the gains — another way of saying the drain fix does nothing
measurable at 100 ms below 2 MiB.

## 6. Deltas, stated plainly

**The drain attribution is confirmed, and it is the whole of the falling
ladder.**

* **At the ratified default window, zero RTT** — the fix buys **+22 % to
  +32 %** (72.52 → 88.51 in `bulk`; 70.66 → 93.30 in `sweep`). Restated as
  per-byte cost, the drain was **18–24 %** of it. Ruling 269's estimate of
  *"~35 % of its per-byte cost"* was derived from the `a + b·w` fit and is
  **an over-estimate**; the measured figure is 18–24 %.
* **At the ladder top (8Mi/16Mi, zero RTT)** — the fix buys **15.7–16.4×**
  (5.73 → 90.24; 5.74 → 94.36). The `sweep` ladder's 12.3× monotone fall
  becomes a 1.06× flat line at 90–95 MiB/s. **The window knob's sign flips**:
  raising to 8Mi/16Mi was 12.7× slower, and is now 1.02× faster.
* **At 100 ms RTT** — the fix buys **essentially nothing** where the window
  binds: +0.7 % / +2.4 % / +4.6 % / +3.9 % on the four lower rungs. The
  optimum stays at **2Mi/8Mi** and the best achievable at 100 ms becomes
  **9.39 MiB/s** (from 9.04). The 8 MiB rung, which previously *fell* to 5.59,
  now **stalls** (§8).
* **The RTT collapse is the same finding seen from the other side.** `srtt` at
  the default window 2.30 → 1.41 ms; at 8Mi/16Mi **968.8 → 42.3 ms**, a 23×
  drop, with `min_rtt` unchanged at 0.04–0.25 ms throughout.

Nothing in the wire moved: amplification 1.040–1.044 and loss 0.0000 in every
clean-loopback cell of both columns, so the gain is CPU returned to the
protocol, not bytes stopped from being resent.

## 7. Test suite confirmation

The mutant is behaviour-preserving against the whole suite, **unmodified** — no
test was touched, added, or skipped.

```
$ cargo test --all-features 2>&1 | grep -E "^test result" | awk -F'[ ;]' '{p+=$4; f+=$6} END {print "PASSED:", p, "FAILED:", f}'
PASSED: 1095 FAILED: 0

$ cargo fmt --all --check && echo "FMT CLEAN"
FMT CLEAN

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a7e9fa682ef66aec6)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
```

Clippy needed **one** fix and it was in the throwaway probe, not the mutant: a
`collapsible_if` in the benchmark's sampler task. Zero warnings after it. The
25 test binaries include the golden-wire and size/constant pins, which are
byte-identical — as they must be, since the change is a buffer index and
touches no frame, constant, or layout.

## 8. Caveats

**(a) The 8 MiB / 100 ms cell stalls under the mutant. This is the one result
that is not an improvement, and I did not attribute it.** Under the mutant,
`sweep-rtt`'s top rung (8Mi/16Mi at 100 ms) produced no timed window at all: the
process sat at **0.0 % CPU for six minutes**, parked in `kevent` inside tokio's
timer/IO driver (`measurements/stall-sample.txt`) — every task idle on a timer,
not spinning. The baseline completes the same cell in ~22 s at 5.59 MiB/s, with
5.6 % measured loss and amplification 1.101; it was already the only lossy cell
in the whole matrix.

**Reproduced three times out of three** (`measurements/stall-probe.log`, each
run capped and killed): the original `sweep-rtt`, a repeat `sweep-rtt`, and
`bulk-rtt` at 100 ms. It is deterministic, not a flake.

**It is specific to that one cell, and the evidence narrows it sharply.** The
same 8Mi/16Mi window under the mutant, measured on purpose to bracket it:

| cell (mutant, 8Mi/16Mi) | result | loss | `cwnd` p50 | `inflight` p50 |
|---|---|---|---|---|
| 0 ms RTT (`bulk`) | 90.24 MiB/s | 0.0000 | 1 176 MB | 8 566 879 |
| 0 ms RTT (`sweep`) | 94.36 MiB/s | 0.0000 | 1 234 MB | 8 534 443 |
| 20 ms RTT (`bulk-rtt`) | 79.13 MiB/s | 0.0000 | 2 433 004 | 2 424 000 |
| 50 ms RTT (`bulk-rtt`) | 37.69 MiB/s | 0.0005 | 2 210 404 | 2 206 800 |
| **100 ms RTT** | **STALL** | — | — | — |

So the mutant's buffer arithmetic is exercised at 8 MiB windows, at speed, with
retransmission timers live, at three different RTTs, and is clean. Note also
that at 20 and 50 ms `inflight` sits **exactly** on `cwnd` (2 424 000 against
2 433 004; 2 206 800 against 2 210 404) — §14's controller is the binding
constraint there and is behaving. Only the 100 ms rung fails.

Two readings remain, and I did **not** separate them:

  * *Rate-enabled, pre-existing.* The mutant makes the sender ~16× more
    capable, so it can now genuinely fill an 8 MiB window at 100 ms; buffers
    drop hard and something in recovery or flow control does not come back. On
    this reading the stall was always there and the drain was hiding it.
  * *A mutant defect on a path only mass retransmission reaches.* The change is
    behaviour-preserving by construction and 1095 tests agree — but working
    rule 13 applies squarely: `FlakyWire` on a paused clock is not a saturated
    UDP socket, and nothing in the suite drives an 8 MiB window through
    kernel-level drop.

The baseline's own numbers lean towards the first reading — that cell already
carried 5.6 % loss and amplification 1.101 before any mutant existed, the only
cell in the matrix that did — but *lean* is the strongest word the evidence
supports. This is the first thing the real slice should reproduce, and it is
the strongest argument for the fix landing through the full blind-split process
rather than as a patch.

**(b) The mutant doubles the send buffer's peak size.** Compaction triggers at
`head >= live`, so `buf.len()` reaches up to `2 × live` and the `Vec`'s
capacity follows — ~2 MiB per stream at a 1 MiB window instead of ~1 MiB.
Ruling 94 is the standing precedent that per-stream allocation is a
first-class budget here, so the real fix probably wants a ring buffer or an
explicit slack bound rather than this threshold. The threshold was chosen for
provable amortisation, not for memory.

**(c) The 90–95 MiB/s ceiling is this host's, and it is a single-thread
figure.** Both endpoints, both AEAD directions, acks and recovery are on one
core (`topology=one-thread/one-LocalSet/both-endpoints`). It is not a protocol
constant and should not be quoted as one.

**(d) `a + b·w` under-predicted the residual.** The fit put the
window-independent ceiling at 109 MiB/s; removing the drain reaches 93–95. The
gap is either fit error at the ladder's low end or a few percent of genuinely
window-dependent cost that survives (the compaction still copies ~1 byte per
released byte, and `write()`'s `extend_from_slice` reallocation now copies the
dead prefix as well). Reported as measured; not chased.

**(e) The probe is the sender's estimator only.** `probe_rtt()` reads
`link.ca`. The receiver's estimator was not sampled, and `bytes_in_flight` is a
point sample at 200 ms granularity, so the Little's-law check in §1.4 is an
order-of-magnitude argument, not a fit.

**(f) Everything here is throwaway.** Three files carry instrumentation that
must not land — `src/core/connection/mod.rs`'s and `src/shell/connection.rs`'s
`probe_rtt`, and `examples/bench_vs_tcp.rs`'s sampler — plus the mutant itself
in `src/core/connection/send.rs`. All four are marked `MEASUREMENT MUTANT — DO
NOT LAND` in place. `run-bench.sh`, `run-stall.sh` and `measurements/` are
scaffolding.

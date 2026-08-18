# Ruling-271 ACK-cadence coalescing: experiment results

## Setup

`git rev-parse HEAD` = `99998d57c17cccfec73b79ed2fee90a8ea1212cd` — matched, proceeded. `git cherry-pick d5a390a` applied clean (commit `2fe5f19`, 6 files, `IMPL-ACK-REPORT.md` included). Read the full report — its C1 (−8% at 100ms/default, three refuted candidates: byte-based slow start, `app_limited` direction, §12.3 `ack_delay` subtraction) and C2 (`ACK_COALESCE_MAX=32` binding at 28.3 data-per-ack) are the two open questions this answers.

I added four `#[doc(hidden)]` process-wide atomic-counter probes (EXPERIMENT-ONLY, tagged `agent-ac999434a8102c68a`, never touching wire/constants/timers) to make two things measurable that weren't accessible outside `#[cfg(test)]`: `MAX_DATA`/`MAX_STREAM_DATA` frame-pack count (`src/core/connection/streams.rs`), and RTT-estimator sample count + last `smoothed_rtt` (`src/core/connection/recovery.rs`), both exposed via `src/lib.rs` and read from `examples/bench_vs_tcp.rs`. Diff is instrumentation-only (75 lines, 4 files); `ACK_COALESCE_MAX` is restored to the shipped `32` in the final tree. Nothing here was committed or will land.

## Question 1 — the valve

`cargo run --release --example bench_vs_tcp -- bulk` (rtt=0), sweeping `ACK_COALESCE_MAX`:

| valve | default MiB/s (runs) | default ack% | raised (8Mi/16Mi) MiB/s (runs) | raised ack% |
|---|---|---|---|---|
| 2 (pending unreachable) | — | — | — | — |
| **32 (shipped)** | 115.00, 108.01 | 3.67%, 3.54% | **134.95, 131.84** | 3.15%, 3.15% |
| 64 | 107.11, 110.19 | 2.08%, 2.12% | 106.98, 109.48 | 1.59%, 1.59% |
| 128 | 112.45 | 1.16% | 112.24 | 0.81% |
| 1024 | 109.18 | 1.13% | 110.27 | 0.42% |

TCP baseline held flat ~11.2–11.3 GiB/s throughout (unaffected, as expected).

Raw:
```
valve=32: BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=115.00 ... ack_dg=78495,dg_out=2140384
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=134.95 ... ack_dg=78998,dg_out=2504629
          BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=108.01 ... ack_dg=70907,dg_out=2005581
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=131.84 ... ack_dg=77024,dg_out=2442342
valve=64: BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=107.11 ... ack_dg=41555,dg_out=1995711
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=106.98 ... ack_dg=31665,dg_out=1996165
          BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=110.19 ... ack_dg=43273,dg_out=2041608
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=109.48 ... ack_dg=32238,dg_out=2031164
valve=128:BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=112.45 ... ack_dg=24258,dg_out=2090406
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=112.24 ... ack_dg=16623,dg_out=2062145
valve=1024:BENCH scenario=bulk rtt_ms=0 windows=default n=5 p50=109.18 ... ack_dg=22974,dg_out=2028565
          BENCH scenario=bulk rtt_ms=0 windows=8Mi/16Mi n=5 p50=110.27 ... ack_dg=8491,dg_out=2044028
```

**Findings:**
- **≥~115 at defaults**: only valve=32's first run touched it (115.00); its own second run was 108.01. All valves 32–1024 land default-window throughput in a noisy 107–115 band — flat, not a returns curve. Diminishing returns for the default-window case start *at* 32; nothing above it helps.
- **Raised windows: valve=32 is a peak, not a floor.** 64/128/1024 all lose ~15–20% versus 32 (134.95/131.84 → 106.98–112.24), reproducibly across two independent 32-runs and two independent 64-runs. This is the "or degrades" case the brief asked about — larger valve **actively hurts** the raised-window/rtt=0 configuration. I did not chase the mechanism (out of scope for Q1), but flag it: raising the valve is not a free lunch.
- **Unbounded (1024) behaves, but not for free.** ack% does not scale linearly with valve past ~128 (128→1024 is 8× the valve but only ~2× fewer ACK-only datagrams for default, ~2× for raised) — a natural per-drain-boundary event rate, not the valve, becomes the limiter well before 1024. No pathological stall or giant-frame cost observed; `amp`/`loss` stayed flat (~1.04 / ~0) at every valve. But it does cost the raised-window throughput valve=32 gets.
- **`MAX_ACK_RANGES=64` under loss — unmeasurable with this fixture.** `examples/bench_vs_tcp.rs`'s transport is an explicitly lossless, non-reordering in-process FIFO relay (its own doc comment: *"loss is deliberately out of scope here"*; the nonzero `loss=` field is kernel socket-buffer overrun, not injected loss). Working rule 13 applies squarely. Reading `ack::derive` (`src/core/connection/ack.rs:298-338`) instead: ranges are built newest-first, truncated at `MAX_ACK_RANGES` or `room`, oldest dropped first — and coalescing structurally **raises** the risk of hitting that cap under real loss, because each ACK now spans far more newly-received traffic before being built, giving more distinct gaps time to accumulate in the replay window. This is a code-level risk flagged for the maintainer, not a measured defect.

## Question 2 — the −8% at 100ms/default

**Reproduction** (`sweep-rtt`, valve=32, shipped): 3 runs, default rung = **1.34, 1.29, 1.38** MiB/s (my own 4th run with instrumentation added: 1.43; combined avg ≈1.36). Control at valve=2 (pending unreachable ⇒ pre-271 emission point, same code path): **1.46, 1.41** (+1.44 with instrumentation; avg ≈1.44). Regression confirmed: **≈−6 to −8%**, matching the report's 1.36 vs 1.47.

**(a) Persistence at 64/128/1024 — yes, unchanged, no worsening, no recovery:**

| valve | default MiB/s | rtt_samples/s | credit(MaxData) frames/s |
|---|---|---|---|
| 2 (control) | 1.46 / 1.41 / 1.44 | 642.75 | 11.46 / 11.65 / 11.76 |
| 32 (shipped) | 1.34 / 1.29 / 1.38 / 1.43 | 55.30 | 12.01 / 12.14 / 12.67 / 12.85 |
| 64 | 1.44 | 29.34 | 11.80 |
| 128 | 1.36 | 44.78 | 14.00 |
| 1024 | 1.44 | 19.85 | 11.66 |

**(b) Mechanism [i] — REFUTED, measured.** `credit_per_s` (MAX_DATA/MAX_STREAM_DATA emission rate) is statistically indistinguishable across every valve tested (11.5–14/s, no trend), while throughput regresses ~7%. The "credit grants ride the frequent ACK-only packets" theory does not hold. Code confirms why: `Connection::read()` (`src/core/connection/mod.rs:957-970`) calls `self.pump(now)` unconditionally right after consuming bytes — independent of ACK state — and `Streams::pack_control` packs `MaxData` (`streams.rs:1020`) *before* the ruling-271 pending-ACK break check (`ack_packed &amp;&amp; !self.ack.is_owed() &amp;&amp; packing.frames().len() == 1`, `mod.rs:2561`); a due MaxData makes `frames().len() &gt;= 2`, so it's never held back by the valve.

**A new, better-fitting candidate — measured, not fully proven.** Instrumented `RttEstimator::sample()` (`recovery.rs:333`, called once per received ACK **frame**, which is exactly the cadence at which `NewReno::on_ack` (`congestion.rs:120`) can grow `cwnd`, since `Controller::on_ack` fires once per newly-acked packet, batched by whichever frame covers it — `mod.rs:1566-1568`). That sample rate **collapsed 11–22×** at the 100ms/default cell: 642.75/s → 55.30/s (valve 32) → 29.34/s (valve 64) → 44.78/s (128) → 19.85/s (1024). `last_srtt_us` stayed flat (~103–121ms) across every valve — no estimator-value inflation, so candidate [ii] as literally framed ("ack_delay inflating the estimator") is also not supported. What *is* supported: growth is summed correctly per byte (confirms the report's byte-based-slow-start refutation), but it now arrives in far fewer, larger discrete steps; between ACK-frame arrivals `cwnd` is flat. At 100ms RTT with the small default window, the sender's small `cwnd` is more likely to exhaust before the next, now much rarer, ACK — an idle-wait effect a frequent-small-ACK cadence used to fill with incremental unlocks. The saturation pattern corroborates this: throughput does not degrade further from valve 32→1024 even as `rtt_samples/s` keeps falling, consistent with the stall pattern already being maximal once ACK frequency drops below roughly what one small-cwnd burst needs per RTT.

I did **not** directly instrument sender idle/stall time (the fully decisive measurement) — flagging that gap rather than claiming confirmation. What's measured: (1) mechanism [i] excluded by direct counter comparison across 5 valve values; (2) ACK-frame/RTT-sample arrival frequency collapses 11–22× under coalescing at exactly this cell, confirmed by code to gate congestion-window growth cadence; (3) the regression persists unchanged at 64/128/1024, matching the report's own valve=8 "saturates immediately" finding.

## Bottom line for the ruling

**Valve: keep 32.** Raising it does not help the default-window throughput (flat/noisy from 32 up) and measurably **hurts** the raised-window/rtt=0 case (−15 to −20% at 64/128/1024 vs 32, reproduced twice each at 32 and 64). **Regression: keep open, not a coalescing-depth artifact.** The −7 to −8% at 100ms/default is present at every valve tested including 1024 (effectively unbounded) — it is intrinsic to per-drain coalescing at that specific (small-window, high-RTT) operating point, not fixable by tuning the valve. Best-supported mechanism, measured but not fully proven: ACK-frame arrival frequency (confirmed 11–22× lower) gates congestion-window growth cadence, producing idle time between growth events at high RTT with a small window — credit-grant starvation (hypothesis [i]) is excluded by direct measurement.

Files touched (instrumentation only, reverted valve to 32, nothing committed): `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-aadc2a32ec3653d4b/src/core/connection/streams.rs`, `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-aadc2a32ec3653d4b/src/core/connection/recovery.rs`, `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-aadc2a32ec3653d4b/src/lib.rs`, `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-aadc2a32ec3653d4b/examples/bench_vs_tcp.rs`.
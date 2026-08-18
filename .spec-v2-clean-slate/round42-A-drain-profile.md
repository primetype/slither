# Round 42, report A — the drain attribution: profile

*Sonnet profiling agent, worktree at `d8bb652`, macOS `sample`, no
commits (throwaway `debug = true` only). Verbatim report below. Relay
thread excluded from all bucket math (verified 100 % idle in kevent).*

---

# Round 42 Item 1 — Per-datagram CPU attribution (profiling only, nothing lands)

Commit under test: `d8bb65214d5fcd69b412c64446a13ca326c714ed` (verified via `git rev-parse HEAD`).

Platform: macOS (Darwin 25.6.0), sampling-based profiling.

Workload: `examples/bench_vs_tcp.rs`, single-stream clean-loopback bulk transfer,
both endpoints on one thread. Reference numbers from the story: 73 MiB/s at
default windows, ~64k datagrams/s => ~15.6 us/datagram total. Known floors:
syscalls ~2.3 us (measured socket floor), AEAD ~2.7 us (measured cryptoxide
floor). ~10 us/datagram unattributed. Confirmed O(window) memmove in
`SendHalf::release` (send.rs:581-595, per ruling 269).

This report is measurement-only. No production code changes are made; the
only edit anywhere is a worktree-local, throwaway `debug = true` under
`[profile.release]` in `Cargo.toml`, noted below and not committed.

---

## Method

Throwaway edit to `Cargo.toml` (worktree-local, not committed):

```toml
# THROWAWAY, worktree-local only, added for round-42 item-1 sampling
# profiling of examples/bench_vs_tcp.rs -- NOT part of the ratified gate
# surface, not intended to be committed. Keeps line info in the release
# binary so `sample`/`xctrace` can symbolicate frames.
[profile.release]
debug = true
```

Build:

```
$ cargo build --release --example bench_vs_tcp
   ...
    Finished `release` profile [optimized + debuginfo] target(s) in 8.94s
```

Tool choice: macOS built-in `sample` (preference order 1) worked without
sudo/entitlement prompts on this machine — `xctrace` was not needed as a
fallback.

**Config (a), default windows** — the `bulk` scenario (`scenario_bulk`,
single stream, rtt=0, ramp 2s + 5*4s windows = 22s total):

```
$ nohup ./target/release/examples/bench_vs_tcp bulk > bulk-default.log 2>&1 &
PID=31078
$ sample 31078 15 -f sample-default.txt
Sampling process 31078 for 15 seconds with 1 millisecond of run time between samples
Sampling completed, processing symbols...
Sample analysis of process 31078 written to file sample-default.txt
```

Sample window: t=3s to t=18s of the 22s run (started 3s in, past the 2s
ramp; the run's own timed samples span t=2s-22s), well inside the 5
back-to-back 4s timed windows. The run finished cleanly:

```
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default streams=1 n=5 p50=69.79 p99=- p999=- min=68.92 max=73.40 unit=MiB/s note=amp=1.043,loss=0.0000,dg_out=1305408,dg_in=1305368,ack_dg=660795,mtu=1180
```

69.79 MiB/s median, consistent with the ~73 MiB/s reference figure (this
run's median is a touch lower, plausibly sampling overhead from `sample`
itself running concurrently).

**A methodological fact discovered while parsing the output, load-bearing
for everything below:** `bench_vs_tcp`'s topology is one thread running
*both* slither endpoints (`com.apple.main-thread`) plus a **second**
thread (`bench-relay`) that only forwards raw UDP between the two
loopback sockets. `sample` records **both** threads' stacks every tick,
including *blocked* stacks. Inspecting `bench-relay`'s call tree shows
its **entire** 12535-sample tree is one unbroken chain ending in
`kevent` (`mio::Poll::poll` waiting on the kqueue) — that thread is
essentially 100% idle-blocked, waiting for the next datagram to relay,
because forwarding is cheap and the main thread is the bottleneck (its
CPU-bound work at ~15.6 us/datagram * ~65k dg/s is ~100% of a core,
matching `ps aux`'s observed 99.6% CPU for the process). So the raw
process-wide "Sort by top of stack" leaf table is **contaminated** by
~12535 idle-wait `kevent` samples that have nothing to do with per-datagram
CPU cost. All bucket percentages below exclude that idle contribution:
denominator = (process-wide leaf total) − (bench-relay's all-kevent
samples), i.e. the main thread's own on-CPU leaf time.

## Top functions — default window

Leaf/self-weight table from `sample`'s own "Sort by top of stack, same
collapsed (when >= 5)" section (process-wide, i.e. both threads; see the
kevent caveat above — the kevent row is almost entirely the idle relay
thread). Top 15 by sample count, out of 24883 total displayed (entries
below 5 samples are not itemized individually by `sample`):

| # | Function | Samples | % of displayed total | Bucket |
|---|----------|---------|----------------------|--------|
| 1 | `kevent` (libsystem_kernel) | 12546 | 50.4% | syscall — **but 12535 of these are the idle `bench-relay` thread blocked waiting for I/O; main-thread-only kevent is ~11** |
| 2 | `__sendto` (libsystem_kernel) | 5050 | 20.3% | syscall |
| 3 | `_platform_memmove` (libsystem_platform) | 2927 | 11.8% | memmove/drain (mixed — see breakdown below) |
| 4 | `cryptoxide::chacha20::ChaCha::process` | 1295 | 5.2% | AEAD |
| 5 | `__recvfrom` (libsystem_kernel) | 1119 | 4.5% | syscall |
| 6 | `cryptoxide::poly1305::donna64::State::blocks` | 597 | 2.4% | AEAD |
| 7 | `slither::core::connection::ack::derive` | 181 | 0.7% | ack/recovery |
| 8 | `_xzm_free` (libsystem_malloc) | 87 | 0.3% | allocation |
| 9 | `mach_absolute_time` (libsystem_kernel) | 83 | 0.3% | timer/pump (clock reads) |
| 10 | `_platform_memset` (libsystem_platform) | 62 | 0.2% | other |
| 11 | `ReplayWindow::check_and_mark` | 50 | 0.2% | ack/recovery |
| 12 | `Poly1305::finalize` | 49 | 0.2% | AEAD |
| 13 | `Session::open` (packet-level unseal) | 37 | 0.1% | frame encode/decode |
| 14 | `Poly1305::update_mut` | 36 | 0.1% | AEAD |
| 15 | `<deduplicated_symbol>` (libsystem_malloc) | 32 | 0.1% | allocation |

Full 82-row leaf table (everything `sample` itemized at >=5 samples) was
used for the bucket totals below; only the top 15 are reproduced here for
brevity.

### The memmove call-stack check (the point of this whole exercise)

Per ruling 269, a confirmed O(window) memmove lives in `SendHalf::release`
(`send.rs:581-595`, specifically `self.buf.drain(..drop)` at line 593).
Grepping this default-window sample file for the literal string
`release` (case-insensitive) across the **entire** file returns **zero
hits** — `SendHalf::release` never appears as its own stack frame.

Reading the actual call-tree context around the two largest
`_platform_memmove` leaf instances (1331 and 1317 samples, 2648 of 2927
total, i.e. 90.5%) resolves this: both trace through the identical path

```
Connection::handle_datagram
  -> Connection::apply_ack_outcome
    -> Streams::on_ack_range
      -> Streams::on_ack_range (tail, offset 400 -- SendHalf::on_ack_range/release inlined here)
        -> _platform_memmove
```

Cross-checked against source: `send.rs:484` `SendHalf::on_ack_range` calls
`self.release()` at line 491 (`streams.rs:677` `Streams::on_ack_range`
calls into it per-stream). LLVM has inlined `SendHalf::on_ack_range` and
`release` into `Streams::on_ack_range`'s reported frame — that is why no
"release" symbol survives, not because the code path isn't taken. **This
confirms ruling 269's attribution**: the dominant memmove hot spot is
`SendHalf::release`'s `Vec::drain(..drop)`, reached from the **incoming
ACK processing path**, not from the send path directly.

The remaining `_platform_memmove` leaf samples were traced individually
via their call-tree context:

| Origin | Samples | % of all memmove | Note |
|---|---|---|---|
| `SendHalf::release` via `Streams::on_ack_range` (ack path) | 2648 | 90.5% | **the O(window) bug, confirmed** |
| `RecvHalf::apply_stream` (applying an incoming stream frame into recv buffer) | 85 | 2.9% | expected payload copy |
| `SendHalf::next_chunk` (chunking payload for send) | 18 | 0.6% | expected payload copy |
| `Streams::read` / `RecvHalf::read` (app-facing read copy) | 16 | 0.5% | expected payload copy |
| `Streams::write` (app-facing write copy) | 11 | 0.4% | expected payload copy |
| Inside `cryptoxide::chacha20::process` (AEAD's own internal buffer shuffle) | 24 | 0.8% | reclassified into AEAD bucket below |
| Untraced small fragments (<=3 samples each, many call sites, not individually traced) | 125 | 4.3% | residual, see caveats |

## Bucket attribution — default window

Denominator: 24883 (process-wide displayed leaf total) minus 12535
(bench-relay's all-`kevent` idle block) = **12348**, the main thread's own
on-CPU leaf time (both slither endpoints combined, since both run inline
on that one thread).

| Bucket | Samples | % | Notes |
|---|---|---|---|
| syscall (`__sendto`, `__recvfrom`, main-thread `kevent`) | 6180 | 50.05% | `__sendto` 5050 dominates; `__recvfrom` 1119 |
| memmove/drain (`SendHalf::release`, confirmed via call stack) | 2648 | 21.45% | ack-processing path, not send path |
| AEAD (cryptoxide chacha20/poly1305 + hiss cipher glue) | 2091 | 16.94% | includes the 24-sample AEAD-internal memmove |
| timer/pump/driver machinery | 368 | 2.98% | driver tick, BTreeMap connection/stream scans, waker bookkeeping, `mach_absolute_time` clock reads, tokio UdpSocket wrapper glue |
| ack/recovery processing | 305 | 2.47% | `ack::derive`, `ReplayWindow::check_and_mark`, `Recovery::on_ack`/`on_sent`, `RangeSet`, `AckState::on_recv` |
| allocation (malloc/free/realloc, Vec growth) | 257 | 2.08% | |
| frame encode/decode | 227 | 1.84% | packet classify/pack/parse + the 4 payload-copy memmove sub-buckets |
| other (memset/bzero, error-path glue, untraced memmove fragments, TLS) | 229 | 1.85% | see caveats |
| *(unaccounted residual)* | ~43 | 0.35% | rounding + entries `sample` didn't itemize individually (<5 samples each) |

### Sanity cross-check against the known floors

Using the story's ~15.6 us/datagram total and these percentages:

- AEAD: 16.94% x 15.6 us = **2.64 us**, versus the measured cryptoxide
  floor of ~2.7 us — close agreement, validates the bucket boundaries and
  the sampling methodology.
- syscall: 50.05% x 15.6 us = **7.81 us**, versus the measured socket
  floor of ~2.3 us — **substantially higher** than the isolated floor.
  This is the single biggest surprise in this profile: raw
  `sendto`/`recvfrom` leaf time, on this thread, at this workload, is
  ~3.4x the isolated socket-floor measurement. Candidate explanations
  (not verified further here — out of scope for a measurement-only
  pass): the isolated floor benchmark may measure one syscall pair where
  this workload's one thread makes 4 socket calls per "round" (both
  endpoints send + receive), the two counts are not simply additive if
  the isolated benchmark was single-direction, or there is real
  per-call overhead this workload incurs that the floor benchmark does
  not (e.g. non-uniform payload sizes, `sendto`'s 5050 samples vs
  `recvfrom`'s 1119 is a 4.5:1 imbalance worth its own follow-up).
- memmove/drain: 21.45% x 15.6 us = **3.35 us** — this is the answer to
  "where do the ~10 us go": `SendHalf::release`'s ack-triggered
  `Vec::drain` is, by itself, roughly comparable in cost to the entire
  AEAD cipher, **even at default (256 KiB/1 MiB) windows** — i.e. this is
  not only an 8 MiB-window-regime problem.

## Top functions — 8 MiB window

**Config (b), raised windows** — used the `sweep` scenario (window
ladder, 5 rungs at ~22s each on clean loopback, rtt=0), since `bulk` only
exercises default windows and `bulk-rtt`/`sweep-rtt` add RTT which isn't
wanted here. `SWEEP`'s final rung is `WINDOWS_RAISED` = 8Mi/16Mi
(`RAISED_STREAM`=8<<20, `RAISED_CONN`=16<<20), matching the brief's
"8Mi/16Mi (the sweep's top rung)".

First attempt was lost to a timing mistake worth recording: I spent time
writing up config (a) findings between launching the `sweep` run and
checking on it, and the whole 112.5s run (`BENCH_DONE wall_s=112.5`)
finished before I got back to it — no process left to attach `sample` to.
Second attempt used a background wait-script polling the log for exactly
4 `BENCH scenario=sweep` lines (i.e. the first 4 rungs done, the 5th —
raised windows — in progress), and fired `sample` the instant that
landed:

```
$ nohup ./target/release/examples/bench_vs_tcp sweep > sweep2.log 2>&1 &
PID=33489
# (background wait-script polls sweep2.log for 4 completed rungs)
# ... rung 4 (2Mi/8Mi) completes at t~89s ...
$ sample 33489 15 -f sample-raised.txt
Sampling process 33489 for 15 seconds with 1 millisecond of run time between samples
Sampling completed, processing symbols...
Sample analysis of process 33489 written to file sample-raised.txt
```

The run finished at `wall_s=111.7`; the sample window (t~89s to t~104s)
sits entirely inside the 5th rung's ~22.7s span (t~89s-111.7s), confirmed
against the log:

```
BENCH scenario=sweep proto=slither rtt_ms=0 windows=8Mi/16Mi streams=1 n=5 p50=4.90 p99=- p999=- min=4.47 max=5.96 unit=MiB/s note=amp=1.040,loss=0.0000,dg_out=91969,dg_in=91969,ack_dg=46020,mtu=1195
```

p50 4.90 MiB/s — consistent with both this run's default-window rung
(p50=74.94 MiB/s) and the throwaway first attempt's raised-window rung
(p50=5.69 MiB/s): raising the window collapses single-stream zero-RTT
throughput by ~15x, reproducibly, exactly the "no arithmetic predicts
this" result `SWEEP`'s own doc comment describes.

Top functions, full leaf table this time (only 14 rows cleared the >=5
threshold — most other buckets from config (a) simply vanished, crowded
out):

| # | Function | Samples | % of displayed total | Bucket |
|---|----------|---------|----------------------|--------|
| 1 | `kevent` (libsystem_kernel) | 12538 | 50.3% | syscall — again almost entirely the idle `bench-relay` thread (12528 of 12538) |
| 2 | `_platform_memmove` (libsystem_platform) | 11708 | **46.9%** | memmove/drain — **99.8% verified same `release()`-via-`on_ack_range` path as config (a)** |
| 3 | `__sendto` (libsystem_kernel) | 379 | 1.5% | syscall |
| 4 | `cryptoxide::chacha20::ChaCha::process` | 107 | 0.4% | AEAD |
| 5 | `__recvfrom` (libsystem_kernel) | 93 | 0.4% | syscall |
| 6 | `cryptoxide::poly1305::donna64::State::blocks` | 46 | 0.2% | AEAD |
| 7 | `slither::core::connection::ack::derive` | 18 | 0.1% | ack/recovery |
| 8 | `Session::open` (packet-level unseal) | 10 | <0.1% | frame encode/decode |
| 9 | `_platform_memset` | 9 | <0.1% | other |
| 10 | `_xzm_free` (libsystem_malloc) | 8 | <0.1% | allocation |
| 11 | `ReplayWindow::check_and_mark` | 7 | <0.1% | ack/recovery |
| 12 | `cryptoxide::cryptoutil::xor_keystream_mut` | 7 | <0.1% | AEAD |
| 13 | `BTreeMap<ConnectionId,ConnRecord>::Iter::next` | 6 | <0.1% | timer/pump |
| 14 | `Connection::pump_packets` | 5 | <0.1% | timer/pump |

Verifying the memmove call stack again (same method as config (a)):

```
Connection::handle_datagram          (11700 samples reach this frame)
  -> Connection::apply_ack_outcome   (11696)
    -> Streams::on_ack_range         (11684)
      -> Streams::on_ack_range (tail, offset 400 -- release() inlined)  (11683)
        -> _platform_memmove          (11683)
```

11683 of 11708 total memmove leaf samples (99.8%) trace through the
*identical* `apply_ack_outcome -> Streams::on_ack_range -> release()`
path as config (a) — at raised windows this is now essentially the
**only** source of memmove, the small alternate paths (recv-side
`apply_stream`, `next_chunk`, app read/write copies) that were visible
at default windows don't even clear the 5-sample threshold anymore.

`bench-relay` was re-checked and is, again, a single unbroken 12528-deep
chain ending in `kevent` — still 100% idle-blocked, nothing else on that
thread.

## Bucket attribution — 8 MiB/16 MiB window

Denominator: 24941 (displayed leaf total, both threads) minus 12528
(bench-relay's all-`kevent` idle block) = **12413**.

| Bucket | Samples | % | vs config (a) |
|---|---|---|---|
| memmove/drain (`SendHalf::release`, confirmed) | 11708 | **94.32%** | up from 21.45% — a **4.4x** share increase |
| syscall (`__sendto`, `__recvfrom`, main-thread `kevent`) | 482 | 3.88% | down from 50.05% (share, not absolute cost — the connection is processing ~14x fewer datagrams/s, 91969 dg over ~20s vs 1305408, so syscall *count* collapsed while memmove work per remaining datagram grew) |
| AEAD | 160 | 1.29% | down from 16.94% |
| ack/recovery processing | 25 | 0.20% | down from 2.47% |
| frame encode/decode | 10 | 0.08% | down from 1.84% |
| allocation | 8 | 0.06% | down from 2.08% |
| timer/pump/driver machinery | 11 | 0.09% | down from 2.98% |
| other | 9 | 0.07% | down from 1.85% |
| *(unaccounted residual)* | 0 | 0.00% | every displayed-table entry accounted for exactly |

### Sanity check: does memmove grow dramatically from (a) to (b)?

**Yes, dramatically — and the mechanism is confirmed, not just the
share.** Absolute per-15s-sample memmove leaf-sample count went from 2927
to 11708 (a 4x rise) while the *denominator* (main-thread on-CPU samples)
stayed roughly flat (12348 -> 12413, since the thread is still ~100%
CPU-bound, just spending nearly all of it in one place now) — meaning
essentially all of the "freed up" time from doing 14x fewer syscalls,
14x less AEAD work, etc. was absorbed by this one memmove. Its share of
on-CPU time rose from 21.45% to 94.32%. The call-stack verification
found the *exact same* `Connection::apply_ack_outcome -> Streams::
on_ack_range -> SendHalf::release -> Vec::drain` path in both configs,
now accounting for 99.8% of all memmove samples instead of 90.5%. This
is a clean, direct confirmation of ruling 269's attribution: **the
O(window) `Vec::drain` in `SendHalf::release` (send.rs:581-595) is the
single dominant CPU cost once the stream window is raised**, and it is
already the second-largest cost (after raw syscalls) at the ratified
default window.

## Answer: where do the ~10 us/datagram go (at default windows)?

Applying the default-window bucket percentages to the story's ~15.6
us/datagram total (cross-checked against the two independently-measured
floors, which line up well — see the AEAD row above):

| Component | Share | ~us/datagram | Status before this profile |
|---|---|---|---|
| syscalls (`sendto`/`recvfrom`) | 50.05% | ~7.81 us | "known", but the isolated floor (~2.3 us) undershoots this by 3.4x — see note below |
| **`SendHalf::release`'s `Vec::drain` (ack path)** | **21.45%** | **~3.35 us** | **unattributed — this profile's main finding** |
| AEAD (cryptoxide) | 16.94% | ~2.64 us | known, matches the ~2.7 us floor closely |
| timer/pump/driver machinery | 2.98% | ~0.46 us | unattributed, minor |
| ack/recovery processing (excluding the drain) | 2.47% | ~0.39 us | unattributed, minor |
| allocation | 2.08% | ~0.32 us | unattributed, minor |
| frame encode/decode | 1.84% | ~0.29 us | unattributed, minor |
| other | 1.85% | ~0.29 us | unattributed, minor |

Of the ~10 us the brief flagged as unattributed, **roughly a third of it
(~3.35 us) is the single `SendHalf::release` memmove** — already the
second largest cost bucket in the whole datagram budget at *default*
windows, before any window is raised. The rest is spread thin across
several small buckets (pump/driver bookkeeping, ack/recovery arithmetic,
allocation, frame packing/parsing), none individually dominant. The
other large piece of the puzzle is that **the syscall floor measured in
isolation does not match what this workload spends in `sendto`/
`recvfrom`** — that gap (~5.5 us) is bigger than the memmove finding and
is *not* explained by this profile; it is flagged as the next thing
worth measuring (see Caveats), not asserted as a specific mechanism.

At raised (8Mi/16Mi) windows the picture simplifies sharply: the same
`SendHalf::release` call site alone consumes 94.32% of on-CPU time,
confirming it as an O(window) cost that dominates once the window that
sizes it grows.

## Caveats

- `debug = true` was added to `[profile.release]` for symbolication;
  optimization level and inlining are untouched (still whatever the
  crate's default release profile uses), so this should not change
  timing/branching versus the shipped profile, only which frames survive
  as distinct symbols in `sample`'s output. It does NOT explain why
  `SendHalf::release` has no separate frame — that is ordinary inlining,
  present with or without `debug = true`; `debug = true` only lets
  `sample` attach a source line to frames that do survive as distinct
  symbols (several rows above show `file.rs:NNN`).
- Sampling is at 1ms granularity via `sample`; short-lived leaf functions
  can be under- or over-represented relative to true wall-clock share.
  The AEAD-vs-floor cross-check above is the best evidence the
  methodology's percentages are in the right ballpark.
- The process-wide "Sort by top of stack" table `sample` emits mixes
  both threads. The `bench-relay` thread is idle-blocked in `kevent`
  essentially 100% of the time in this workload and was excluded from
  the CPU-budget denominator (see Method). Every other leaf bucket was
  confirmed empty for `bench-relay` by inspecting its (very short, single
  unbroken chain) call tree directly.
- ~125 of 2927 memmove leaf samples (4.3%) were not individually traced
  to a specific caller — they are scattered across many single- to
  triple-digit call sites in the raw call tree, and tracing every one
  has sharply diminishing returns given the dominant 90.5% finding is
  unambiguous. They are folded into "other" rather than "memmove/drain"
  since the latter bucket is specifically defined (per the brief) as
  memmove reached from release/drain paths.
- Symbolication quality is good for slither/hiss/cryptoxide/tokio Rust
  frames (readable v0-mangled names resolve legibly by inspection
  without needing a demangler) and for system libraries. One `???` frame
  appears at a fixed offset inside the `bench_vs_tcp` binary in the
  driver's tokio task-harness poll path — likely a monomorphized
  closure/generator whose symbol didn't survive, but its children (the
  driver `run`/`handle_datagram` chain) resolve fine, so it costs nothing
  attribution-wise.
- **The syscall-floor gap is a real open question this profile raises but
  does not answer.** `__sendto`+`__recvfrom` leaf time is ~7.81 us/dg in
  this workload versus a ~2.3 us measured socket floor — a ~3.4x gap,
  larger in absolute terms than the `SendHalf::release` finding. This
  profile only has leaf attribution ("time was spent inside the syscall
  trampoline"), not a decomposition of *why* it costs more here than in
  the isolated floor benchmark (call count per datagram, payload-size
  effects, non-`NODELAY`-equivalent UDP path differences, contention with
  the sampler itself, etc. are all still open). Flagging rather than
  guessing, per the brief's "do not fabricate a profile."
- Single machine, single run per config, 15s samples. No repeated trials
  to characterize run-to-run variance of the bucket percentages
  themselves (the throughput numbers *are* corroborated across two
  independent `sweep` runs: raised-window p50 4.90 and 5.69 MiB/s, and
  default-window p50 69.79, 72.24 and 74.94 MiB/s across the `bulk` run
  and the two `sweep` runs — all consistent with each other and with the
  ~73 MiB/s reference).
- Config (a)'s `sample` ran concurrently with the `bulk` scenario's own
  CPU work on the same machine (the sampler itself consumes some CPU);
  this is inherent to sampling-based profiling on a live process and is
  the likely reason config (a)'s measured p50 (69.79 MiB/s) came in
  slightly under the ~73 MiB/s reference and under config (b)'s own
  default-window rungs (72.24, 74.94 MiB/s, sampled *after* rather than
  *during*).
- Scope: this was measurement only. No production code was changed —
  the sole edit anywhere is the throwaway `[profile.release]` `debug =
  true` in `Cargo.toml` (worktree-local, uncommitted; `git status`
  confirms only `Cargo.toml` modified plus this new report file, no
  commits made). Release gates were not run since none of the ratified
  code changed. Full sample files (`sample-default.txt`,
  `sample-raised.txt`, and the intermediate `bulk-default.log`/
  `sweep*.log`) are left in the scratchpad directory, not the worktree.

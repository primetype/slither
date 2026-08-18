# Round 42, report H — the residual profile at HEAD (post-ring)

*Sonnet profiler, worktree at `bc71246`, macOS `sample`, relay thread
verified idle and excluded; raw samples in the session scratchpad.
Verbatim report below.*

---

# Round 42-H: Residual profiling — attributing the ~6 µs/data-datagram gap

HEAD: `bc712462bec925fe3a551cb6293440a702557b8` (verified via `git rev-parse HEAD`,
first act; matches the expected `bc71246` — "Round 42: the datapath attribution —
retry storm refuted, two levers measured, a residual named").

Round 42-G's finding: syscalls account for a large share of the ~8.9 µs/datagram
gap between slither (~13 µs/data-dg) and quinn (4.1 µs/dg); after subtracting the
two named, measured levers (connect()-socket ≈1.41 µs, ACK-cadence floor-only
≈1.53 µs), a ≈5.96 µs residual against the 8.9 µs gap (≈6 µs, as this brief frames
it) was left **unattributed** — described as "protocol/userspace processing
invisible to syscall instrumentation." This report profiles HEAD's CPU time
directly (macOS `sample`, 15 s, default-windows `bulk`) to attribute that residual.

Measurement only. No production code changes. The only tree modification was a
throwaway `debug = true` under `[profile.release]` in `Cargo.toml` — added, used
to build `examples/bench_vs_tcp` with symbols, and **reverted** before this report
was finalized (`git status --short` at the end of this session shows only this
file, untracked).

**Headline finding, stated up front:** the "~6 µs residual" is **not sitting in
slither's userspace/protocol code as CPU time**. This profile accounts for
100% of the sampled CPU budget by construction, and the non-syscall,
non-AEAD share of it is only **≈2.19 µs/data-datagram**, not ≈6 µs. The
likely location of the rest of the gap is **inside the syscall-attributed
sampling bucket itself** — `sample`'s leaf-time attribution to
`__sendto`/`__recvfrom` runs far above the isolated per-call floor's
prediction, reproducing round 42-G's own "sendto anomaly" (there flagged,
not resolved) with a fresh instrument. See "The residual named" below.

---

## Method

Recipe (round 42-A's, reused as directed):

```toml
# THROWAWAY, worktree-local only, added for round-42-H residual profiling of
# examples/bench_vs_tcp.rs -- NOT part of the ratified gate surface, not
# intended to be committed. Keeps line info in the release binary so
# `sample` can symbolicate frames. Reverted before this report is finalized.
[profile.release]
debug = true
```

Build:

```
$ cargo build --release --example bench_vs_tcp
    Finished `release` profile [optimized + debuginfo] target(s) in 9.35s
```

Run (default `-- bulk`, which is long enough to sample without looping — it
runs default windows [ramp 2 s + 5×4 s timed ≈ 22 s], then raised windows,
then TCP, ≈66 s total per the earlier round's timing):

```
$ nohup ./target/release/examples/bench_vs_tcp bulk > bulk-head.log 2>&1 &
PID=51086
$ ps -p 51086 -o pid,pcpu,command
  PID  %CPU COMMAND
51086 100.0 .../target/release/examples/bench_vs_tcp bulk
$ sample 51086 15 -f sample-head.txt
Sampling process 51086 for 15 seconds with 1 millisecond of run time between samples
Sampling completed, processing symbols...
```

Sample launch time (from the report's own header) was ~7 s into the process's
run and ran for 15 s, i.e. inside the default-windows block's ramp tail +
timed section (block ends at ≈22 s, before the config switches to raised
windows). The default-windows `BENCH` line landed right after sampling
completed, confirming the whole 15 s sample fell inside that config:

```
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default streams=1 n=5 \
  p50=79.71 p99=- p999=- min=79.28 max=85.78 unit=MiB/s \
  note=amp=1.044,loss=0.0001,dg_out=1507039,dg_in=1506958,ack_dg=762832,mtu=1179
```

**Measured throughput for this run: 79.71 MiB/s, mtu=1179 B.** (Somewhat
below the ~88 MiB/s context anchor — consistent with round 42-A's own
observation that `sample` running concurrently depresses throughput a
little; this run's own numbers are used for every conversion below, so the
sample and the throughput correspond to the same execution.)

```
data-dg/s = 79.71 MiB/s × 1,048,576 / 1179 B  = 70,892.3 dg/s
µs/data-dg (whole budget) = 1,000,000 / 70,892.3 = 14.106 µs
ack_dg / dg_out (this config) = 762,832 / 1,507,039 = 0.5062  (≈1 ack per 2 data, matches round42-G)
```

**Thread separation, verified exactly as round 42-A's method:** `sample`'s
"Call graph" section shows two threads, `com.apple.main-thread` (11344
samples in its own tree) and `bench-relay` (also 11344). Reading
`bench-relay`'s full call tree (lines 1395-1406 of the raw file): it is one
single, unbroken 11344-sample chain, `thread_start → ... → mio::Poll::poll →
kevent` — **100% idle-blocked**, confirmed by direct inspection, not by
assumption. Excluded from every bucket below.

```
    11344 Thread_6016877: bench-relay
      11344 thread_start (in libsystem_pthread.dylib) + 8
        11344 _pthread_start ...
          11344 std::sys::pal::unix::thread::Thread::new::thread_start ...
            ... (single chain, no branching) ...
                11344 kevent (in libsystem_kernel.dylib) + 8
```

**Denominator.** `sample`'s "Sort by top of stack, same collapsed (when >= 5)"
table totals **22484** samples process-wide. Subtracting `bench-relay`'s
11344 idle-blocked `kevent` samples: **11140** is the main thread's own
on-CPU leaf-sample budget for the 15 s window — this is what every bucket
percentage below is computed against. The leaf table's `kevent` row is
11357; `11357 − 11344 = 13` main-thread-only `kevent` samples (real blocking
syscall waits on the driver thread itself), bucketed with syscalls below.

Symbolication was demangled with `rustfilt`; every slither/hiss/cryptoxide
frame resolved to a readable name (this build's debuginfo is materially
better-resolved than round 42-A's — most of the functions LLVM had inlined
away there survive as distinct frames here, letting this profile go
substantially finer).

Top of the leaf table (full 87-row table used for all bucket math below):

```
        kevent  (in libsystem_kernel.dylib)        11357
        __sendto  (in libsystem_kernel.dylib)        5759
        <cryptoxide::chacha20::ChaCha<20>>::process        1423
        __recvfrom  (in libsystem_kernel.dylib)        1256
        <cryptoxide::poly1305::donna64::State>::blocks        651
        _platform_memmove  (in libsystem_platform.dylib)        311
        slither::core::connection::ack::derive        210
        _xzm_free  (in libsystem_malloc.dylib)        114
        <session::Session<ReferenceSuite>>::open        75
        <cryptoxide::poly1305::Poly1305>::finalize        74
        mach_absolute_time  (in libsystem_kernel.dylib)        69
        <session::ReplayWindow>::check_and_mark        62
        <cryptoxide::poly1305::Poly1305>::update_mut        54
        <hiss::DatagramRecv<IK>>::decrypt_at        53
        _platform_memset  (in libsystem_platform.dylib)        39
        <tokio UdpSocket::send_to>::{closure#0}        37
        ... (73 more rows, all individually classified below)
```

---

## Bucket table — default windows (the deliverable)

Every `_platform_memmove` leaf occurrence was individually traced to its
caller in the raw call tree (51 occurrences summing to 311 samples; the
largest 9 — summing to 210 of the 311 — were traced exactly, the remaining
101 samples across ~42 small call sites were not individually traced and
are folded into "other", per round 42-A's own precedent). The accounting
below closes **exactly**: bucket samples sum to 11140, the full denominator.

| Bucket | Samples | % of hot core | µs/data-dg |
|---|---|---|---|
| **Syscalls proper** (`__sendto` 5759, `__recvfrom` 1256, main-thread `kevent` 13) | 7028 | 63.08% | **8.898** |
| **AEAD** (cryptoxide chacha/poly + hiss cipher glue; seal 55.9% / open 44.1%, see split below) | 2381 | 21.37% | **3.014** |
| **Allocation** (malloc/free/realloc, Vec growth) | 322 | 2.89% | 0.408 |
| **ACK + recovery bookkeeping** (`ack::derive`, `on_ack_range`, `RangeSet`, sent-map ops, incl. 10 memmove samples from `BTreeMap<u64,SentPacket>` node removal) | 318 | 2.85% | 0.403 |
| **Timers/pump/driver loop** | 309 | 2.77% | 0.391 |
| **Frame DECODE/parse** (`Session::open`, `ReplayWindow::check_and_mark`, `frame::parse`, `Packing::into_plaintext`, `classify`, `handle_datagram` self, incl. 28 memmove samples) | 231 | 2.07% | 0.293 |
| Other (memset/bzero/mutex/error-glue + 101 untraced memmove fragments) | 182 | 1.63% | 0.231 |
| **Reassembly/recv-path** (`apply_stream`, `on_stream_frame`, app-facing `read`, incl. 106 memmove samples) | 140 | 1.26% | 0.177 |
| Tokio/quinn-udp plumbing ABOVE the syscall (poll/readiness frames) | 90 | 0.81% | 0.114 |
| Instant::now / clock reads (`mach_absolute_time`, `Instant::add`/`checked_add`) | 80 | 0.72% | 0.101 |
| Frame ENCODE (packet build, `Packing::push`, `Streams::fill`/`pack_control`, `Vec<Frame>` drop) | 44 | 0.40% | 0.056 |
| **Ring send-half** (`SendHalf::next_chunk`, self + its memmove child) | 15 | 0.13% | 0.019 |
| **Total** | **11140** | **100%** | **14.106** |

### The ring fix confirmed — memmove is now small, and it's not the old bug

Per the brief's expectation: **confirmed small.** `_platform_memmove` is only
311 of 22484 leaf samples (1.4% process-wide, 2.8% of the main-thread
denominator) — down from round 42-A's 21.45% at default windows (and 94.32%
at raised windows). Tracing `Streams::on_ack_range → SendHalf::release`
finds **zero** memmove children anywhere in this sample — `release()`'s own
source now documents why (`src/core/connection/send.rs:657`, ruling
269(iii)):

```rust
    /// **[ruling 269(iii)]** O(1) in the bytes released and O(1) in the bytes
    /// left: a front `drain` on a `VecDeque` advances the ring's head and
    /// moves no element. The `Vec` this replaced re-`memmove`d every live
    /// byte on **every** acknowledged STREAM range — the cost round 42's
    /// profile found at 21 % of the per-datagram budget at the ratified
    /// window and 94 % at 8 MiB. See the module doc.
    fn release(&mut self) {
```

`SendHalf::on_ack_range` (`send.rs:534`) now costs allocator churn instead
of memmove — it calls `unacked.remove()`, `retransmit.remove()` (both
`RangeSet::remove`, `send.rs:718`), and `acked.insert()` (`RangeSet::insert`,
`send.rs:698`) before the now-O(1) `release()`. Traced call-tree excerpt
(`Streams::on_ack_range → RangeSet::remove → _xzm_free`/`_malloc_zone_malloc`):

```
33 Streams::on_ack_range
  19 RangeSet::remove(+340)
    9 _xzm_free, 5 _xzm_free, 3 _free, 1 DYLD-STUB$$free
  9 RangeSet::remove(+112)
    5 _xzm_xzone_malloc, 2 DYLD-STUB$$malloc, 1 dedup, 1 _malloc_zone_malloc
17 Streams::on_ack_range(+248)
  8 RangeSet::remove(+112) -> 5 _malloc_zone_malloc, 2 _xzm_xzone_malloc, 1 dedup
  7 RangeSet::remove(+340) -> 5 _xzm_free, 2 DYLD-STUB$$free
```

`RangeSet` (`send.rs:692`) is a `Vec<Range<u64>>` under the hood; slither
tracks **three** of them per stream (`unacked`, `retransmit`, `acked`), and
`insert`/`remove` on a `Vec`-backed set still grow/shrink/coalesce the
backing `Vec` — small, but real, malloc/free churn. Separately,
`Recovery::on_ack` (`recovery.rs:252`) removes from a `BTreeMap<u64,
SentPacket>` "sent-map" (`recovery.rs:118`) — a `remove_kv` on the B-tree
triggers node-internal `memmove` (10 samples traced directly:
`Recovery::on_ack → OccupiedEntry::remove_kv → Handle::... → memmove`).

**This is the answer to "should now be small — say if not": it is now
small (0.13% of the core / 0.019 µs/data-dg for the ring itself), and the
ACK-path cost that replaced the old O(window) drain is diffuse allocator
churn (RangeSet's three `Vec`s, the sent-map `BTreeMap`), not a single hot
function — it shows up split across the Allocation (0.408 µs) and ACK/
recovery (0.403 µs) buckets rather than concentrated in one place.**

### AEAD seal vs. open split

Traced by summing every occurrence of `<ChaChaPoly as Cipher>::encrypt` vs.
`::decrypt` as call-tree nodes (their **inclusive** counts capture every
AEAD-bucket descendant — `ChaCha::process`, `poly1305::*`,
`xor_keystream_mut`, `pad16` — so this both splits and cross-checks the
2381-sample AEAD bucket total):

```
$ grep -c 'ChaChaPoly as .. Cipher>::decrypt' sample-head-demangled.txt  (inclusive sum) = 1048
$ grep -c 'ChaChaPoly as .. Cipher>::encrypt' sample-head-demangled.txt  (inclusive sum) = 1331
1048 + 1331 = 2379  (AEAD bucket total: 2381 — within 2 samples, i.e. everything
in the AEAD bucket flows through exactly these two hiss entry points, as expected)
```

| Side | Samples | Share of AEAD | µs/data-dg |
|---|---|---|---|
| Seal (send) | 1331 | 55.9% | 1.686 |
| Open (recv) | 1048 | 44.1% | 1.328 |

Seal costs ~27% more than open in this measurement — not the naive
50/50 expectation for a stream-cipher-plus-MAC scheme processing the same
byte volume in each direction. Traced example seal chain:
`pump_packets → Session::seal_inner (session.rs) → DatagramSend::encrypt_next
→ ChaChaPoly::encrypt → ChaChaPoly1305::encrypt → ChaCha::process`; traced
example open chain: `handle_datagram (mod.rs:668) → Session::open
(session.rs) → DatagramRecv::decrypt_at → ChaChaPoly::decrypt →
ChaChaPoly1305::decrypt → ChaCha::process`. Both chains verified directly
in the call tree; the asymmetry itself is reported, not explained — could be
implementation (encrypt/decrypt code-path divergence in cryptoxide),
workload (A also opens tiny 37 B acks, B also seals tiny 37 B acks — should
be small per the byte-share arithmetic below, but not zero), or sampling
noise at this sample size. **Against the context anchor's ≈2.7 µs/data-dg
seal+open floor: this run measures 3.014 µs — a ≈12% excess**, materially
smaller than the AEAD discrepancies seen in the syscall bucket (see below),
i.e. AEAD's measured cost is close to (not identical to) its isolated
floor, unlike syscalls.

---

## The residual named

**Ranked, non-syscall non-AEAD buckets (1731 samples, 15.54% of the core,
≈2.194 µs/data-dg total — this is the full non-syscall-non-AEAD residual as
directly measured):**

| Rank | Bucket | µs/data-dg | % of core |
|---|---|---|---|
| 1 | Allocation | 0.408 | 2.89% |
| 2 | ACK + recovery bookkeeping | 0.403 | 2.85% |
| 3 | Timers/pump/driver loop | 0.391 | 2.77% |
| 4 | Frame DECODE/parse | 0.293 | 2.07% |
| 5 | Other/untraced | 0.231 | 1.63% |
| 6 | Reassembly/recv-path | 0.177 | 1.26% |
| 7 | Tokio/udp plumbing above syscall | 0.114 | 0.81% |
| 8 | Instant::now/clock reads | 0.101 | 0.72% |
| 9 | Frame ENCODE | 0.056 | 0.40% |
| 10 | Ring send-half | 0.019 | 0.13% |

**No single bucket dominates.** The top three (allocation, ACK/recovery,
driver loop) are within 5% of each other (0.39-0.41 µs each) and together
account for 1.202 µs — most (55%) of the 2.194 µs non-syscall-non-AEAD
total, but split three ways, not concentrated. Concrete top frames, with
file:line, for each of the top three:

- **Allocation** (`src/core/connection/send.rs:718` `RangeSet::remove`,
  `:698` `RangeSet::insert` — three `RangeSet`s per stream, `Vec`-backed,
  grown/shrunk on every ACK range) and (`src/core/connection/recovery.rs:118`
  the `sent: BTreeMap<u64, SentPacket>`, `:252` `on_ack`'s `remove_kv`).
- **ACK + recovery bookkeeping**: `src/core/connection/ack.rs` `derive`
  (210 samples, the single largest non-syscall-non-AEAD leaf function in
  the whole profile), `src/core/connection/mod.rs:1223` `on_ack_range`,
  `:1513` `apply_ack_outcome`, `src/core/connection/recovery.rs:219`
  `on_sent`, `:252` `on_ack`.
- **Timers/pump/driver loop**: `src/shell/driver.rs:345` `serve` iterating
  `:161`'s `conns: BTreeMap<ConnectionId, ConnRecord<I>>` every turn (30
  samples just for the 1-connection `Iter::next` walk), `src/core/
  connection/mod.rs:2207` `pump_packets`, plus per-turn deadline
  recomputation (`Endpoint::deadline`, `Timers::next`,
  `TimestampGuard::next_orphan_deadline`, `IntroQueue::next_deadline`,
  `Connection::sync_liveness_timer`) — all size-independent, O(1)-per-turn
  bookkeeping that runs whether or not there is a datagram to send.

**Where the ~6 µs actually is — the headline finding restated with the
arithmetic.** Directly measured, slither's own on-CPU time splits
**63.1% syscalls / 21.4% AEAD / 15.5% everything else**. The
"everything else" (userspace/protocol processing: allocation, ACK
bookkeeping, driver loop, frame encode/decode, recv-path, clock reads,
tokio plumbing) totals only **≈2.19 µs/data-dg** — nowhere near the ≈6 µs
round 42-G's cross-protocol arithmetic implied was hiding there. Meanwhile
the **syscall bucket alone (8.898 µs/data-dg) is ≈1.89× the context
anchor's async-floor prediction (≈4.70 µs/data-dg, the shared tok-oneway
floor scaled by the 1.506 real-datagrams-per-data-datagram ratio)** — a
**≈4.2 µs excess concentrated entirely inside the syscall bucket**, larger
than the whole non-syscall-non-AEAD residual this profile measured. This is
the same **direction** as round 42-G's own "sendto anomaly" (there: ~49%
measured share vs. a smaller floor-model prediction, a ~1.67 µs excess,
explicitly flagged "not resolved") — this profile reproduces it independently,
via CPU sampling rather than syscall census/floor-probe arithmetic, and
finds it **larger** here (≈4.2 µs vs. ≈1.67 µs). **Conclusion: most of
the previously-"unattributed" residual is not hidden userspace/protocol
code — it is hidden inside what `sample` calls syscall time, i.e. the gap
between the isolated per-call floor and this workload's actual in-process
`sendto`/`recvfrom` cost. This profile does not explain that gap's cause
(candidate explanations are the same ones round 42-G already listed:
route/PCB-lookup effects, non-uniform payload sizes, contention with the
sampler itself, or genuine extra per-call cost under this concurrent
workload that an isolated floor probe does not reproduce) — it corroborates
that the gap is real and, if anything, larger post-ring than round 42-G's
own numbers.**

---

## The true ACK-lever value

Round 42-G's ACK-lever bound was explicitly **floor-only** (syscall cost
only, "excluding any userspace/frame-construction/crypto savings"): 1.53
µs/data-dg, a conservative lower bound. This section extends it using the
bucket data above.

**Model:** buckets split into size-independent (roughly O(1) per datagram,
regardless of payload bytes — syscalls, tokio plumbing, frame decode,
ACK/recovery, driver loop, allocation, clock reads, other) and
size-dependent (scale with payload bytes moved — AEAD, frame encode,
ring send-half, recv-path copy):

```
Size-independent total = A(8.898) + B(0.114) + E(0.293) + F(0.403)
                        + I(0.391) + J(0.408) + K(0.101) + L(0.231)
                        = 10.839 µs/data-dg

Size-dependent total   = C(3.014) + D(0.056) + G(0.177) + H(0.019)
                        = 3.266 µs/data-dg

10.839 + 3.266 = 14.105 µs  (matches the 14.106 µs total, rounding)
```

Datagrams processed per 1 data-datagram, this run: `1 + ack_dg/dg_out`
`= 1 + 0.5062 = 1.506` (matches round 42-G's ~1-per-2 cadence exactly).

**Size-independent cost is assumed uniform per datagram** (a UDP
`sendto`/`recvfrom`, a `handle_datagram` dispatch, an ACK-frame's
`derive`/`on_ack_range`, a driver-loop turn, all cost roughly the same
whether the packet is 1179 B of data or 37 B of ACK — the same assumption
round 42-G's own floor-only bound made, extended here to more than just
syscalls):

```
Per-datagram size-independent cost = 10.839 / 1.506 = 7.198 µs/datagram
ACK-attributable (size-independent) = 7.198 × 0.5062 = 3.643 µs/data-dg
```

**Size-dependent cost scales with bytes**, and ACK-only datagrams are tiny
(37 B, per round 42-G's `out_hist`/`in_hist` census):

```
ACK byte-share of total bytes moved = (0.5062 × 37) / (1179 + 0.5062 × 37)
                                     = 18.73 / 1197.73 = 1.564%
ACK-attributable (size-dependent) = 3.266 × 0.01564 = 0.051 µs/data-dg
```

**Total true ACK-lever value ≈ 3.643 + 0.051 ≈ 3.69 µs/data-dg** — the
combined cost, on **both** sides, of B building+sealing an ACK-only
datagram (send path: `pack_control` → `seal_inner` → `sendto`) and A
receiving+opening+processing it (`recvfrom` → `Session::open` →
`apply_ack_outcome`/`on_ack_range` → `Recovery::on_ack`), at the measured
1-per-2 cadence, both sides combined.

**This is ≈2.4× round 42-G's floor-only lower bound (1.53 µs)** — the
additional ≈2.16 µs is the userspace/protocol processing (driver dispatch,
ACK bookkeeping, frame decode, allocation) that a cadence change would also
eliminate, on top of the syscalls the floor-only bound already counted.

**≈26.2% of slither's entire 14.11 µs/data-dg budget is attributable to
ACK-only-datagram processing on both sides**, at the current 1-per-2
cadence.

## vs. quinn's non-floor budget

Round 42-G measured quinn's own non-floor cost at **≈1.92 µs/data-dg**
(quinn total 4.1 µs − its own ≈2.18 µs connected-socket floor). **The true
ACK-lever value alone (3.69 µs) is ≈1.9× quinn's entire non-floor budget.**
Put differently: eliminating slither's ACK chattiness down toward quinn's
own cadence (quinn: 1 ACK per 58.6 data datagrams, ack fraction 1.68%,
vs. slither's 1-per-2, 33.6%) would, on this profile's arithmetic, recover
more processing time than quinn spends on everything it does beyond the
shared UDP floor:

```
Savings at quinn's cadence ≈ 3.693 × (1 − 0.0171/0.5062) ≈ 3.693 × 0.9662
                            ≈ 3.57 µs/data-dg
```

(0.0171 = quinn's ack_dg/data_dg ratio, derived from its 1.68% ack
fraction: `0.0168/(1-0.0168)`.) This is the number that answers "what would
§12.4's cadence ruling actually buy beyond syscalls": **not 1.53 µs, but
somewhere around 3.6-3.7 µs/data-dg — roughly a quarter of slither's whole
per-datagram budget** — assuming the size-independent-cost-per-datagram
model above holds and the levers compose additively (unverified, see
Caveats).

---

## Caveats

- **The size-independent/size-dependent split for the ACK-lever model is an
  assumption, not a measurement.** It is the same kind of assumption round
  42-G's own floor-only bound made (treating per-call cost as roughly
  payload-size-independent for syscalls); this report extends it to more
  buckets (driver loop, ACK bookkeeping, allocation, frame decode) without
  separately verifying that, e.g., `ack::derive` or `RangeSet::remove` cost
  the same whether the acknowledged range came from a full-MTU data segment
  or a padding-only ACK. A direct A/B (implement the cadence change,
  re-profile) is the only way to confirm this model rather than assume it.
- **The AEAD seal/open split (55.9%/44.1%) and the "seal costs ~27% more
  than open" finding are reported, not explained.** Possible causes
  (implementation asymmetry in cryptoxide's encrypt vs. decrypt code path,
  workload asymmetry from A also opening tiny ACKs / B also sealing tiny
  ACKs, or sampling noise) are listed but none were checked further —
  out of scope for a measurement-only pass.
- **The "sendto anomaly" — the single largest number in this report (the
  ≈4.2 µs syscall-bucket excess over the floor-model prediction) — is
  reported per rule 3/4 as a conflict/open question, not resolved.**
  Round 42-G flagged the same-direction discrepancy explicitly ("Reported
  per rule 3/4: two real, load-bearing measurements are in tension...Not
  resolved here"); this session's fresh, independent CPU-sampling
  measurement corroborates that the anomaly is real (a different
  instrument finds the same direction) but does not identify its cause,
  and finds it **larger** in this run's numbers than round 42-G's own
  figures — the two reports' magnitudes for the "excess" (≈1.67 µs vs.
  ≈4.2 µs here) are themselves in tension and are not reconciled here;
  they may reflect different runs/workloads/host states rather than a
  real change, since (per rule 3/4) neither report claims a controlled A/B.
- **Bucket classification involved judgment calls, flagged here rather
  than silently made:**
  - `ReplayWindow::check_and_mark` (62 samples) was bucketed under
    **frame DECODE/parse** (per-datagram inbound validation) in this
    report; round 42-A bucketed the same function under **ACK/recovery**.
    Neither the brief's bucket list nor SPEC.md dictates which is correct
    — it is genuinely both (anti-replay validation runs on every inbound
    datagram, ack or data). Reported per rule 3, not resolved; moving it
    to ACK/recovery would shift 62 samples (0.078 µs) from bucket E to F,
    not changing the "no single bucket dominates" conclusion.
  - `Streams::fill`, `Streams::pack_control`, and `SendHalf::next_chunk`
    were bucketed by tracing their actual callers in this run's call tree
    (`pump_packets` → `pack_control` → `fill`, all send-side) rather than
    by name alone — `fill` could plausibly be misread as a recv-path
    "take" operation from its name; the traced call-tree context (visible
    in the excerpts above) is what resolved it to frame ENCODE.
  - 101 of 311 memmove leaf samples (32%), across ~42 small call sites,
    were not individually traced (sharply diminishing returns given the
    dominant findings above are unambiguous) and are folded into "other" —
    same practice round 42-A used (there: 125/2927, 4.3%; here a larger
    fraction, 32%, because the overall memmove bucket is now so much
    smaller that the untraced tail is a bigger share of a smaller number).
- **Single machine, single 15 s sample, one run.** No repeated trials to
  characterize run-to-run variance of the bucket percentages. This run's
  throughput (79.71 MiB/s) came in below the ~88 MiB/s context anchor,
  consistent with round 42-A's own note that `sample` running concurrently
  depresses throughput; every conversion above used this run's own
  measured throughput rather than the anchor, so the sample and the
  µs/data-dg figures correspond to the same execution.
- **The "true ACK-lever" and "residual" figures are both derived from one
  profile's bucket shares, not a controlled A/B of an actual cadence or
  ring-buffer change.** Implementing either lever and re-measuring is the
  only way to confirm these numbers rather than model them — the same
  caveat round 42-G raised about its own remedy-class estimates.
- **Scope: measurement only.** No production code was changed. The sole
  edit anywhere was the throwaway `[profile.release]` `debug = true` in
  `Cargo.toml`, added, used to build the symbolicated release binary, and
  reverted before this report was finalized (`git status --short` at the
  end of this session shows only `RESIDUAL-REPORT.md`, untracked). Release
  gates were not run since no ratified code changed. Raw files
  (`sample-head.txt`, its `rustfilt`-demangled copy, `bulk-head.log`) are
  left in the scratchpad directory, not the worktree.

# N — Audit item 3.1: the split-timer measurement of the loopback p99

Base commit: `980cd131cd8dc5c3ee1635cab687573dd905ca91` (verified `git rev-parse HEAD`, tree clean).
Worktree: `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-af22337ae6301b788`
Machine: macOS (Darwin 25.6.0), 16 cores, baseline load average ≈ 3.4.
Date: 2026/08/18.

## 0. Base verification

```
$ git rev-parse HEAD
980cd131cd8dc5c3ee1635cab687573dd905ca91
$ git log --oneline -3
980cd13 Ruling 259(viii) record: the config-raisable windows enter §10.2, with their rule-4 companions
bb32603 Ruling 259(viii) integration: the window refusals fold into ConfigError, the mint verb renamed
2c93102 Ruling 259(viii): the flow-control knob, and the core half of the comment sweep
$ git status --short
(empty)
```

Base matches the brief. No reset needed.

## 1. The item and its decision rule

`round41-H-audit-triage.md` §3.1, classification **INVESTIGATE-LATER (not a
defect; premise partly unsupported)**. The anomaly under investigation is D's
phase C — 1000 sequential, unpipelined datagram round trips over real loopback
UDP, both endpoints on one OS thread, real clock:

```
PHASE_C_OK n=1000 p50_us=131 p99_us=22105 min_us=41 max_us=86094   (release)
PHASE_C_OK n=1000 p50_us=510 p99_us=63424 min_us=164 max_us=195053 (debug)
```

Round 41 refuted the *"at 0 % CPU"* leg of the premise (the `ps` samples were
all taken during the 60 s **idle** window that begins after phase C finishes)
and re-scoped the item to one named next measurement — **split the timer**:

> p99 of `t1−t0` ≈ 22 ms indicts the send path; p99 of `t2−t1` ≈ 22 ms with
> `t1−t0` in microseconds indicts wake/scheduling and closes the item as
> "not slither".

The paused-clock twin (agent C's E8) already eliminates a protocol-internal
cause: handle→wire and wire→parked-recv were both **0 virtual ns** over 760
round trips, `nonzero=0` at every percentile. So no core or driver timer is
responsible; what is open is real syscall versus real scheduler cost.

The original harness was uncommitted scratch and is gone.

## 2. The harness

**`examples/audit_udp.rs`** — new file, new `examples/` directory, committed
permanently by the maintainer's decision. `Cargo.toml` is **not** touched: the
example names only non-optional dependencies (`slither::SoftwareIdentity`,
`slither::packet::ReferenceSuite`, `slither::shell::wire::Wire`, `tokio`,
`rand_chacha`), so no `required-features` stanza is needed and cargo
auto-discovers it — the manifest has no `autoexamples = false`. **No
production code was changed.**

Topology, deliberately D's and not a better one: one process, **one
current-thread tokio runtime inside one `LocalSet` (`slither::block_on`), both
endpoints on that single OS thread**, two real `tokio::net::UdpSocket`s on
`127.0.0.1:0`, real unpaused clock, release build. Endpoint B runs an echo task
(`recv_datagram().await` → `send_datagram`); endpoint A runs the phase-C loop —
50 discarded warmup round trips (D's phase C ran warm, after phases A and B),
then 1000 timed ones, one datagram outstanding, 64-byte payload carrying the
iteration counter so every echo is checked against its request.

### 2.1 Why the split had to be three-way, not two-way

**`Connection::send_datagram` is not `async` and performs no syscall.**
`src/shell/connection.rs:799`, and its own rustdoc says so: *"Not `async`, and
it never waits."* It bound-checks the payload against §11.4, pushes into
§11.3's send queue, sets `cell.dirty`, calls `shell.mark_dirty`, and returns.
The `send_to` syscall happens later, on the driver's next turn.

So `t1 − t0` is an **enqueue by construction**, and the decision rule's first
branch — *"p99 of `t1−t0` ≈ 22 ms indicts the send path"* — cannot fire however
slow the send path is. Taken literally the two-way split is an unfalsifiable
test of the hypothesis it was written to settle: it can only ever land on the
second branch. **This is reported, not resolved** (working rule 3), and it is
rule 11's shape in the triage's own text — a rationale naming a mechanism the
code does not have. §7 states it precisely.

The harness closes the gap by wrapping endpoint A's socket in a `TimedWire`
that implements the public `slither::shell::wire::Wire` trait and times every
`send_to`. That seam is documented as application-supplied (§16.3 normative
property 1, *"The application supplies it"*), so this needs no instrumentation
inside the crate. Five series result:

| Series | Interval | A tall p99 here indicts |
|---|---|---|
| `send_call` | `t0` → `t1` | the handle call — enqueue only |
| `wire_dispatch` | `t1` → the following `send_to` **entry** | driver wake + drain: tokio/OS scheduling |
| `wire_syscall` | that `send_to`'s entry → exit | **the send path**, measured directly |
| `reply_wait` | `t1` → `t2` | everything downstream of the handle |
| `total` | `t0` → `t2` | continuity with `p50=131us / p99=22105us` |

`wire_dispatch` and `wire_syscall` are a strict prefix of `reply_wait`, so the
five decompose the tail rather than merely halving it.

### 2.2 The match is checked, not assumed

`wire_dispatch`/`wire_syscall` take the first `send_to` span at or after `t1`.
A ping-pong emits more sends than round trips, so the harness records each
send's length and prints both histograms. Every run:

```
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
```

1000 data packets of 97 B and 500 ACK-only packets of 37 B. Both add up
against the ratified constants: `DATA_HEADER_LEN` = 14 and `AEAD_TAG_LEN` = 16
(`src/constants.rs:90,119`), so 97 = 14 + **67** + 16, and 67 is one DATAGRAM
frame — type byte, a 2-byte RFC 9000 varint for the length 64, and the 64-byte
payload. 37 = 14 + **7** + 16, an ACK-only plaintext.

**All 1000 matched spans are 97 B in all seven runs**, so no ACK-only packet
was ever emitted between the handle call and the following data send: the two
wire series time the datagram's own packet, every iteration. That is evidence,
not an assumption about coalescing.

## 3. The runs, verbatim

Build: `cargo run --release --example audit_udp` (runs 1–5) and the same
release binary invoked directly (runs L1–L2, so that a concurrent cargo
invocation could not block on the target lock).

### Runs 1–5 — machine otherwise idle (baseline load avg ≈ 3.4 / 16 cores)

```
$ cargo run --release --example audit_udp
audit_udp — round 41 item 3.1, the split-timer loopback probe
  iterations=1000 warmup=50 payload=64B topology=one-thread/one-LocalSet/two-real-UDP-sockets

SERIES send_call n=1000 p50_us=1 p90_us=1 p99_us=2 min_us=0 max_us=11 p50_ns=1208 p99_ns=2583 max_ns=11042
SERIES wire_dispatch n=1000 p50_us=0 p90_us=4 p99_us=7 min_us=0 max_us=36 p50_ns=666 p99_ns=7750 max_ns=36416
SERIES wire_syscall n=1000 p50_us=4 p90_us=9 p99_us=13 min_us=3 max_us=42 p50_ns=4500 p99_ns=13208 max_ns=42959
SERIES reply_wait n=1000 p50_us=49 p90_us=93 p99_us=147 min_us=28 max_us=187 p50_ns=49291 p99_ns=147209 max_ns=187000
SERIES total n=1000 p50_us=50 p90_us=95 p99_us=149 min_us=29 max_us=198 p50_ns=50500 p99_ns=149209 max_ns=198042
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=58
```

```
$ cargo run --release --example audit_udp
SERIES send_call n=1000 p50_us=1 p90_us=1 p99_us=1 min_us=0 max_us=5 p50_ns=1000 p99_ns=1417 max_ns=5833
SERIES wire_dispatch n=1000 p50_us=0 p90_us=3 p99_us=4 min_us=0 max_us=8 p50_ns=375 p99_ns=4542 max_ns=8416
SERIES wire_syscall n=1000 p50_us=3 p90_us=8 p99_us=9 min_us=2 max_us=11 p50_ns=3625 p99_ns=9167 max_ns=11250
SERIES reply_wait n=1000 p50_us=45 p90_us=102 p99_us=124 min_us=28 max_us=129 p50_ns=45334 p99_ns=124625 max_ns=129417
SERIES total n=1000 p50_us=46 p90_us=103 p99_us=125 min_us=29 max_us=130 p50_ns=46292 p99_ns=125875 max_ns=130417
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=55
```

```
$ cargo run --release --example audit_udp
SERIES send_call n=1000 p50_us=1 p90_us=1 p99_us=1 min_us=0 max_us=7 p50_ns=1084 p99_ns=1959 max_ns=7625
SERIES wire_dispatch n=1000 p50_us=0 p90_us=3 p99_us=5 min_us=0 max_us=11 p50_ns=500 p99_ns=5375 max_ns=11041
SERIES wire_syscall n=1000 p50_us=4 p90_us=9 p99_us=11 min_us=3 max_us=20 p50_ns=4250 p99_ns=11625 max_ns=20958
SERIES reply_wait n=1000 p50_us=46 p90_us=111 p99_us=136 min_us=30 max_us=160 p50_ns=46708 p99_ns=136333 max_ns=160625
SERIES total n=1000 p50_us=47 p90_us=112 p99_us=137 min_us=31 max_us=162 p50_ns=47708 p99_ns=137875 max_ns=162417
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=58
```

```
$ cargo run --release --example audit_udp
SERIES send_call n=1000 p50_us=1 p90_us=1 p99_us=1 min_us=0 max_us=3 p50_ns=1084 p99_ns=1958 max_ns=3125
SERIES wire_dispatch n=1000 p50_us=0 p90_us=4 p99_us=6 min_us=0 max_us=7 p50_ns=500 p99_ns=6125 max_ns=7000
SERIES wire_syscall n=1000 p50_us=4 p90_us=8 p99_us=11 min_us=2 max_us=17 p50_ns=4375 p99_ns=11291 max_ns=17167
SERIES reply_wait n=1000 p50_us=45 p90_us=107 p99_us=143 min_us=28 max_us=178 p50_ns=45958 p99_ns=143292 max_ns=178667
SERIES total n=1000 p50_us=47 p90_us=109 p99_us=145 min_us=29 max_us=179 p50_ns=47167 p99_ns=145000 max_ns=179500
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=59
```

```
$ cargo run --release --example audit_udp
SERIES send_call n=1000 p50_us=1 p90_us=1 p99_us=1 min_us=0 max_us=8 p50_ns=1083 p99_ns=1917 max_ns=8750
SERIES wire_dispatch n=1000 p50_us=0 p90_us=3 p99_us=5 min_us=0 max_us=13 p50_ns=459 p99_ns=5166 max_ns=13000
SERIES wire_syscall n=1000 p50_us=3 p90_us=9 p99_us=11 min_us=2 max_us=14 p50_ns=3917 p99_ns=11583 max_ns=14708
SERIES reply_wait n=1000 p50_us=44 p90_us=108 p99_us=136 min_us=29 max_us=150 p50_ns=44625 p99_ns=136083 max_ns=150167
SERIES total n=1000 p50_us=45 p90_us=110 p99_us=137 min_us=30 max_us=152 p50_ns=45667 p99_ns=137750 max_ns=152042
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=57
```

### Runs L1–L2 — under a concurrent `cargo test --all-features`

The leading hypothesis is scheduler jitter, so the probe was also run while the
crate's own debug test suite compiled and ran alongside it. Total system CPU
during these two runs was 110 % → 163 % of 1600 % — a *moderately* busy
machine, not a saturated one (see §6).

```
$ uptime
10:03  up 1 day,  1:57, 6 users, load averages: 3.40 3.51 3.01
$ ./target/release/examples/audit_udp
SERIES send_call n=1000 p50_us=0 p90_us=1 p99_us=1 min_us=0 max_us=7 p50_ns=875 p99_ns=1250 max_ns=7709
SERIES wire_dispatch n=1000 p50_us=0 p90_us=2 p99_us=4 min_us=0 max_us=10 p50_ns=333 p99_ns=4459 max_ns=10667
SERIES wire_syscall n=1000 p50_us=3 p90_us=7 p99_us=8 min_us=2 max_us=12 p50_ns=3333 p99_ns=8958 max_ns=12209
SERIES reply_wait n=1000 p50_us=41 p90_us=90 p99_us=110 min_us=28 max_us=116 p50_ns=41750 p99_ns=110541 max_ns=116416
SERIES total n=1000 p50_us=42 p90_us=91 p99_us=111 min_us=29 max_us=117 p50_ns=42667 p99_ns=111375 max_ns=117333
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=52
```

```
$ ps -A -o %cpu | awk '{s+=$1} END {print "total_cpu_pct", s}'
total_cpu_pct 162.7
$ ./target/release/examples/audit_udp
SERIES send_call n=1000 p50_us=0 p90_us=1 p99_us=1 min_us=0 max_us=15 p50_ns=958 p99_ns=1375 max_ns=15458
SERIES wire_dispatch n=1000 p50_us=0 p90_us=3 p99_us=4 min_us=0 max_us=11 p50_ns=333 p99_ns=4417 max_ns=11541
SERIES wire_syscall n=1000 p50_us=3 p90_us=6 p99_us=8 min_us=2 max_us=27 p50_ns=3833 p99_ns=8209 max_ns=27084
SERIES reply_wait n=1000 p50_us=46 p90_us=90 p99_us=105 min_us=28 max_us=121 p50_ns=46125 p99_ns=105416 max_ns=121708
SERIES total n=1000 p50_us=47 p90_us=91 p99_us=106 min_us=29 max_us=122 p50_ns=47166 p99_ns=106416 max_ns=122750
WIRE_SENDS total=1500 per_round_trip=1.50 all_lengths=37=500,97=1000
WIRE_MATCHED n=1000 lengths=97=1000
PHASE_C_DONE iterations=1000 wall_ms=54
```

## 4. The numbers side by side

Seven runs, 7000 timed round trips, 10 500 wire sends.

| Series | p50 (µs) | p99 (µs) | worst single sample (µs) |
|---|---|---|---|
| `send_call` (`t1−t0`) | **0.9 – 1.2** | **1.3 – 2.6** | 15.5 |
| `wire_dispatch` | 0.3 – 0.7 | 4.4 – 7.8 | 36.4 |
| `wire_syscall` | 3.3 – 4.5 | 8.2 – 13.2 | **43.0** |
| `reply_wait` (`t2−t1`) | 41.8 – 49.3 | 105 – 147 | 187 |
| `total` (`t2−t0`) | **42.7 – 50.5** | **106 – 149** | **198** |
| D's original `total` | 131 | **22 105** | 86 094 |

Run-to-run spread on `total` p99 is 106–149 µs — a factor of 1.4 across seven
runs, loaded and unloaded. Nothing varies wildly; no further runs were needed.

## 5. Verdict

**The 22 ms tail did not reproduce. It is not merely smaller — it is absent by
two orders of magnitude, and neither branch of the decision rule fires.**

- `total` p99 is **106–149 µs** against D's **22 105 µs**: 150–200× lower.
- The single worst round trip in 7000 was **198 µs**, against D's max of
  **86 094 µs**: 430× lower.
- `total` p50 is **43–50 µs** against D's **131 µs**, so the *typical* round
  trip is also 2.6× faster — this is a different machine and/or a different
  system state, not the same distribution with its tail clipped.

Per the brief, this is reported plainly rather than forced into a verdict: on
this host the anomaly is **environment-dependent and not present**.

**What the decomposition does establish, independent of the tail.** The
three-way split is informative even with no tail to attribute, because it puts
a measured ceiling on the one mechanism the triage named:

1. **The `sequential send_to` hypothesis is now measured, not argued.** The
   largest single `send_to` in 10 500 calls was **43 µs**; p99 is 8–13 µs. That
   is **three orders of magnitude** short of 22 ms. §3.1 already argued this
   from first principles (*"a loop of length 1 has no serialisation to
   expose"*); it is now a number. Whatever produced D's tail, the send path
   cannot have.
2. **The handle call is ~1 µs and cannot ever be the answer** — see §2.1: it
   performs no syscall at all.
3. **~85 % of the round trip is downstream of the syscall returning.** p50
   `total` ≈ 46 µs, of which `send_call` + `wire_dispatch` + `wire_syscall` ≈
   5.5 µs. The remaining ≈ 40 µs is kernel loopback delivery, the peer driver's
   wake and turn, the peer's echo, its send, and the local driver's wake — the
   wake/scheduling half of the decision rule. A 22 ms event would have to be
   **≈ 480× the entire observed round trip**, and it would have to live in that
   remainder, because the other three stages are bounded above by 43 µs
   measured.

So the split does point the same way §3.1's leading hypothesis did — real
OS/tokio scheduler jitter on the shared single thread — but it points there by
**exclusion on this host**, not by exhibiting the stall. **It does not, on its
own evidence, close audit item 8**: closing "not slither" on the strength of a
tail that was not present is a rule-12 move (a true lemma about a state other
than the one in question). What the measurement supports is narrower and
solid: *no stage inside slither's send path exceeded 43 µs in 10 500 samples,
and the item's own named suspect is refuted quantitatively.* The maintainer
rules on the close-out.

## 6. Caveats

1. **Different machine, unknown original.** D's host is not recorded in
   `audit/D-real-socket.md` beyond the topology. This ran on macOS 15
   (Darwin 25.6.0), 16 cores. A 2.6× difference in p50 says the hosts differ
   materially; the p99 comparison inherits that.
2. **I could not saturate the machine.** The intended experiment — pin all 16
   cores with hogs and re-run, to see whether the tail is *resurrectable* —
   was blocked by the sandbox's command classifier. The substitute (a
   concurrent `cargo test --all-features`) reached only 110–163 % of 1600 %
   total CPU. **A negative result at that load is weak evidence about
   behaviour at saturation**, and the loaded runs were, if anything, marginally
   *faster*. Someone with an unrestricted shell should repeat this under real
   contention before the item is closed on "cannot reproduce".
3. **macOS scheduling is not Linux scheduling.** Mach's QoS bands and the
   loopback path both differ from Linux's. If D's numbers came from Linux (or
   from a VM, or from CI), this is not a replication.
4. **Baseline load ≈ 3.4 on a 16-core box** — an ordinary working desktop with
   an editor and agents on it, not a quiesced benchmark host. That is *closer*
   to D's conditions than an idle host would be, not further.
5. **`Instant::now()` is inside the measurement.** Each round trip takes three
   clock reads; on this host `send_call` p50 is ~1 µs, which is the enqueue
   *plus* one clock read. At the 43 µs scale that is noise; at the 22 ms scale
   it is nothing.
6. **The warmup changes comparability slightly.** 50 discarded round trips
   precede the timed 1000. D's phase C was warm for a different reason (it
   followed phases A and B), so this is closer to D's shape than a cold start
   would be — but it is not identical, and a first-iteration outlier is
   excluded here that may not have been there.
7. **The two wire series depend on the matching rule**, which is checked but
   not proven: §2.2's evidence is that all 1000 matched spans were 97 B in all
   seven runs. If the driver ever reorders an ACK-only packet ahead of the data
   packet on the same turn, those two series would time the ACK instead — and
   `WIRE_MATCHED lengths` would show `37=…`, which is exactly why it is
   printed.
8. **`n=1000` bounds what a percentile can see.** p99 over 1000 samples is the
   10th-worst; this run has 7000, so the tail is observed down to ~p99.99 at
   198 µs. Rarer stalls than 1-in-7000 are outside this instrument.

## 7. One finding to hand back, not resolved (working rules 3 and 11)

**§3.1's decision rule rests on a premise the code contradicts.** It says
*"p99 of `t1−t0` ≈ 22 ms indicts the send path"*, where `t1−t0` brackets the
handle-send call. `Connection::send_datagram` (`src/shell/connection.rs:799`)
is **not `async`** and issues no syscall — its own rustdoc says *"Not `async`,
and it never waits"* — so that branch is unreachable regardless of the send
path's cost, and a strictly two-way split would have "confirmed" the
scheduling branch by construction while proving nothing.

This is the defect class rule 11 names: a rationale whose named mechanism is
not the one in the code. It did not change the conclusion — the third series
this harness adds tests the send path directly, and the send path is exonerated
quantitatively — but the *argument* as written would have been unsound, which
is the inverse of ruling 206 and invisible to any review that only checks
conclusions.

No spec text, ratified constant, or production behaviour is affected.

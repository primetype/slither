# Ruling 260 — O53a/O53b discharge (agent J)

## 0. Base verification

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a4b06ec9ee76fec20
$ git rev-parse HEAD
ae718236b4da215ba85d4366b482c09715d0e811
$ git status --short
(clean)
```

Base matches the briefed `ae71823`. No reset needed.

## 1. The obligations (SPEC.md:7653-7663)

### 1.1 Verbatim (SPEC.md:7651-7663)

```
**Post-implementation validation obligations (gates on the flagged
rulings).**
- **The ACK-loss-burst simulation** (the §7.2/D-5 gate): FlakyWire on
  the paused clock, sustained ACK-loss bursts against the 2048-bit fused
  window under the every-2nd ACK policy, quantifying spurious-retransmit
  and false-congestion-event rates. If the numbers disappoint, the
  range-tracker ACK (§19) is the ready remedy — before ratification
  hardens the fused choice.
- **The window-constants throughput sanity check** (the §10.2 and §10.6
  gate): bulk-transfer throughput over the same FlakyWire topology MUST
  be **within 20 % of quinn under its shipped defaults**, with no stall,
  measured with the §10.6 reassembly bound active (the
  defragmentation/coalescing cost is part of the number) — before the
  §10.2 constants and `REASSEMBLY_CHUNKS_MAX` ratify.
```

Both are pre-ratification gates by their own words ("before ratification
hardens…", "before the §10.2 constants … ratify"). The wire ratified on
2026/08/14; ruling 260 re-scopes them to measured, pinned obligations.

## 2. Harness vocabulary (FlakyWire / FlakyPolicy)

`grep -n 'pub fn' src/testutil/mod.rs` — the relevant vocabulary:

- `FlakyPolicy` fields are all `pub`: `loss`, `duplicate`, `base_delay`,
  `jitter`, `drop_first`, `drop_at: BTreeSet<usize>`, `send_failure`.
- `FlakyPolicy::drop_at(indices)` — **drops exactly these 0-based send
  indices, no RNG**. This is the burst constructor: a periodic index set
  gives contiguous runs of dropped datagrams at an exact severity.
- `FlakyPolicy::lossy(rate)` — Bernoulli, seeded but brittle (the draw a
  datagram sees moves when an unrelated send is added ahead of it).
- `Network::block_path(from, to)` / `heal_path` — **per-direction**
  blackhole. Whole-path, not a rate.
- `FlakyWire::set_policy` — mid-run policy change; **the send index is
  not reset**, so `drop_at` indices count every send the wire ever made.
- `SharedWire` (`Peer::wire`) exposes `set_policy` / `policy` to a test.
- `Network::sends()` — every `send_to`, counted *before* any policy
  decision. `Network::tap()` → `Tap::datagrams()` → `(src, dst, bytes)`
  for every **accepted** send.

**Chosen construction: a per-direction burst on the ACK path.** The bulk
transfer runs A→B on one stream. B's wire therefore carries essentially
nothing but ACK-bearing packets, so a `drop_at` index set installed on
**B's** wire is an ACK-loss policy by construction. Bursts are periodic
runs (`BURST` consecutive drops every `PERIOD` sends), which is a
sustained burst pattern at an exact, RNG-free severity.

## 3. Instrumentation available (counters / accessors)

There is **no** retransmit, spurious-retransmit or congestion counter
reachable from an integration test:

```
$ grep -rn 'retransmit\|spurious\|congestion' src/core/connection/*.rs | grep -i 'pub fn\|count\|stat'
src/core/connection/close.rs:85:/// recovery and congestion state has already been dropped and there is
src/core/connection/mod.rs:1982:        // §15.2: *"all stream, flow-control, recovery and congestion state
src/core/connection/mod.rs:3098:    /// stream, flow-control, recovery, or congestion state ever crosses a
src/core/connection/timers.rs:151:    /// §15.2 drops all stream, flow-control, recovery and congestion state

$ grep -n 'pub(crate) fn \|pub fn ' src/core/connection/congestion.rs
62:    pub(crate) fn new() -> Self {
82:    pub(crate) fn reset(&mut self, now: Instant) {
100:    pub(crate) fn ssthresh(&self) -> u64 {
106:    pub(crate) fn recovery_start(&self) -> Option<Instant> {
```

`cwnd`/`ssthresh` are `pub(crate)`; `src/shell/connection.rs` exposes no
stats verb; `src/core/connection/testfix.rs` has no `pub fn` at all. So
the measurement is at the **test level**, as the brief anticipated.

## 4. PART 1 — O53a: ACK-loss-burst measurements

### 4.1 Method

`tests/spec_ack_burst.rs`, `#[tokio::test(start_paused = true)]`:

* Two endpoints on `testutil::Pair`, one **uni** stream, 2 MiB A → B —
  enough that the 256 KiB stream window and the 1 MiB connection window
  each cycle many times, so the figure is steady state and not the
  initial window.
* Connection established and the stream primed **before** the fabric is
  slowed, so the handshake's round trips are not charged to the transfer.
* Both directions then get `base_delay = 10 ms, jitter = 0` — a 20 ms
  RTT. Without an injected RTT the PTO sits at its `K_GRANULARITY` floor
  and the congestion controller never binds, so an ACK-loss experiment on
  a zero-delay fabric measures nothing.
* **The forward path (A → B) never loses a datagram.** So B holds every
  byte that leaves A the first time, and every forward datagram beyond
  the loss-free minimum carries data the receiver already held. The
  baseline run (`burst = 0`) supplies that minimum empirically.
* **The burst** is `FlakyPolicy::drop_at` on B's wire — no RNG at all —
  dropping `burst` consecutive return-path datagrams out of every
  `PERIOD = 8`, sustained for the whole transfer. B's absolute send index
  at the start of the measured phase is read from the tap (every setup
  send was accepted, so the tap count *is* the index), and the schedule
  is built from there.
* The drop count is **counter-proved**: `send_to` writes the tap at step 4
  and decides deliveries at step 5, so an index the wire reached and the
  schedule names is an index that died. `ret_dropped` is
  `|schedule ∩ [b_index, b_index + ret_dgrams)|`.

Two rates, per the obligation:

* **spurious-retransmit rate** = `(fwd_burst − fwd_baseline) / fwd_burst`,
  reported in datagrams and in wire bytes.
* **false-congestion-event rate** — there is no cwnd accessor, so this is
  **inferred**, not counted: the virtual-time inflation
  `elapsed_burst / elapsed_baseline`. Stated as an inference. See §4.2's
  caveat for what else is inside it.

### 4.2 Measured rates

```
$ cargo test --features test-util --test spec_ack_burst --release -- --nocapture --test-threads=1
baseline: Run { fwd_dgrams: 1838, fwd_bytes: 2181296, ret_dgrams: 930, ret_dropped: 0, elapsed: 310ms }
burst 2/8: ack_loss=25.1% spurious_dgrams=0.11% spurious_bytes=0.04% inflation=1.403x  Run { fwd_dgrams: 1840, fwd_bytes: 2182196, ret_dgrams: 933, ret_dropped: 234, elapsed: 435ms }
burst 4/8: ack_loss=50.1% spurious_dgrams=0.43% spurious_bytes=0.06% inflation=2.981x  Run { fwd_dgrams: 1846, fwd_bytes: 2182692, ret_dgrams: 943, ret_dropped: 472, elapsed: 924ms }
burst 6/8: ack_loss=75.1% spurious_dgrams=1.55% spurious_bytes=0.12% inflation=33.503x  Run { fwd_dgrams: 1867, fwd_bytes: 2183902, ret_dgrams: 999, ret_dropped: 750, elapsed: 10.386s }
```

**The table for ruling 260** (2 MiB, 20 ms RTT, paused clock, seed
`0x0053_A000`):

| burst | ACK loss (counter-proved) | fwd datagrams | spurious retransmits (dgrams) | spurious retransmits (bytes) | virtual time | inflation |
|---|---|---|---|---|---|---|
| 0/8 (baseline) | 0.0 % (0 / 930) | 1838 | — | — | 310 ms | 1.000× |
| 2/8 | 25.1 % (234 / 933) | 1840 | **0.11 %** | **0.04 %** | 435 ms | 1.403× |
| 4/8 | 50.1 % (472 / 943) | 1846 | **0.43 %** | **0.06 %** | 924 ms | 2.981× |
| 6/8 | 75.1 % (750 / 999) | 1867 | **1.55 %** | **0.12 %** | 10.386 s | 33.503× |

**The numbers do not disappoint, and the mechanism is legible.** The
fused window is *cumulative*: `ack::derive` reads the replay window's
`ranges_desc()`, and on a lossless forward path that window is **one
contiguous block**, so `first_range = largest − start` covers every
counter ever received. Any *single* surviving ACK therefore carries the
receiver's complete state. Losing an ACK burst costs the sender feedback
**timing**, never feedback **information** — which is why 75 % ACK loss
still buys only 1.55 % spurious retransmits. §19's range-tracker ACK, the
remedy the obligation held in reserve, is not needed.

**Caveat on the inflation column — read it as an upper bound, not as a
false-congestion count.** The return path of a one-way transfer carries
ACKs *and* flow-control credit (`MAX_DATA` / `MAX_STREAM_DATA`) in the
same packets. At 75 % loss the sender is legitimately credit-starved as
well as feedback-starved, and a static 256 KiB window cannot open. So the
33.5× at 6/8 bounds the false-congestion contribution from above; it does
not isolate it. The clean number is the spurious-retransmit rate, and at
1.55 % it says the recovery machinery is **not** falsely declaring loss —
whatever the inflation is, it is not being spent on retransmission.

### 4.3 Separation evidence (rule 9)

*A bound is only a test if the degenerate case violates it.* The broken
build the obligation is really about is **"a window that forgets ACKed
ranges"** — an ACK that does not carry the cumulative range, so that a
packet acknowledged only by a lost ACK is never acknowledged at all. One
token in `src/core/connection/ack.rs::derive` produces exactly that:

```
-        first_range: largest - *first.start(),
+        first_range: 0, // MUTATION (reverted): the ACK forgets every range below `largest`
```

Applied on a committed-clean tree, measured, then reverted with
`git checkout -- src/core/connection/ack.rs` (`git status --short`
confirms only `Cargo.toml` and the new test file remain).

```
$ cargo test --features test-util --test spec_ack_burst --release -- --nocapture --test-threads=1 o53a_measure
baseline: Run { fwd_dgrams: 3747, fwd_bytes: 4363224, ret_dgrams: 1946, ret_dropped: 0, elapsed: 38.873s }
thread 'o53a_measure' panicked at tests/spec_ack_burst.rs:210:58:
write: ConnectionLost(TimedOut)
```

| | fwd datagrams for 2 MiB | spurious vs the correct baseline | virtual time |
|---|---|---|---|
| correct build, **0 %** ACK loss | 1838 | — | 310 ms |
| forgetful ACK, **0 %** ACK loss | 3747 | **50.9 %** | 38.873 s (125×) |
| forgetful ACK, **25 %** ACK burst | — | — | **connection dies**: `ConnectionLost(TimedOut)` |

So the separation is not marginal. The broken build spends **half its
forward datagrams on data the receiver already held with no ACK loss at
all**, and it cannot survive the gentlest burst — §15's liveness timeout
kills the connection before the transfer finishes. The pinned envelope
below (2.5 % at 75 % ACK loss) is two orders of magnitude away from the
broken build's 50.9 % at 0 %.

### 4.4 The pinned envelope

`SPURIOUS_CEILING = 0.025` — the harshest burst's measured **1.55 %**,
with the ×1.5 regression headroom the brief asks for (1.55 % × 1.5 =
2.33 %, rounded up to 2.5 %). A regression bound, not a tight pin:
ruling 260's envelope, comment-marked as such in the test.

The burst schedule itself is also asserted to have bitten — `ack_loss()`
must land within a point of the nominal severity — because a schedule
that ran past the return path's send count would silently heal the wire
and make the spurious-rate assertion pass for the wrong reason.

## 5. PART 2 — O53b: throughput floor

### 5.1 `benches/throughput.rs` current numbers

`Cargo.toml`'s `[[bench]]` stanza is `harness = false`, `test = false`,
`required-features = ["test-util"]` — so it is `cargo bench`, and
`cargo test` deliberately does **not** run it.

```
$ cargo bench --features test-util --bench throughput

slither throughput — one thread carries both endpoints
────────────────────────────────────────────────────────────────────────

  loopback/stream        one bi stream, real UDP syscalls
           73.6 MiB/s    (median 72.1, worst 70.9)
          617.7 Mbit/s   over 5 samples of 4.0 MiB
      note: mean read fill 44907 B over 467 read calls; both endpoints share one thread

  inmem/stream           one bi stream, no kernel
           50.9 MiB/s    (median 47.6, worst 36.8)
          426.8 Mbit/s   over 5 samples of 4.0 MiB
      note: mean read fill 51654 B; includes two Vec allocs + copies per datagram the fixture adds

  inmem/streams×4        four bi streams, same total bytes
          143.0 MiB/s    (median 126.2, worst 120.6)
         1200.0 Mbit/s   over 5 samples of 4.0 MiB

  inmem/stream@rtt       one bi stream, 20 ms injected RTT
            9.0 MiB/s    (median 5.4, worst 4.8)
           75.3 Mbit/s   over 3 samples of 2.0 MiB
      note: window/RTT predicts 12.5 MiB/s, measured 9.0

  inmem/datagram         unreliable, 1169-byte payloads
           61.6 MiB/s    (median 58.3, worst 50.3)
          516.7 Mbit/s   over 5 samples of 4.6 MiB
      note: 20480/20480 delivered (100.0%)

  inmem/message          single-shot, 16 KiB each
          168.2 MiB/s    (median 165.7, worst 158.7)
         1410.8 Mbit/s   over 5 samples of 4.0 MiB

  inmem/handshake        endpoint setup + full 4-DH ladder
            269 conn/s   (median 246, worst 171)
          3.713 ms each  (median 4.070, worst 5.859)
```

**These are wall-clock numbers and they are machine-specific.** They
cannot be a pinned floor: the spread inside one run is already 36.8–50.9
MiB/s on `inmem/stream`. That is why O53b's floor is pinned in **virtual**
time instead, and why the bench stays a measurement rather than becoming
a gate (its own stanza comment says so).

O53b's literal bar — *"within 20 % of quinn under its shipped defaults"* —
is also not a bar a test can hold: it names a figure produced by another
crate on unstated hardware. **The half of the sentence that can be pinned
is "with no stall"**, and that is what ruling 260 keeps.

### 5.2 Determinism of virtual-time throughput

Under `#[tokio::test(start_paused = true)]` the clock **only** advances
when the runtime has nothing left to do but wait on a timer: tokio
auto-advances to the next deadline rather than sleeping. So virtual
elapsed time is a function of the timer schedule, and the timer schedule
is a function of the protocol's own deadlines plus `FlakyWire`'s injected
delays — both deterministic, and `Network::seeded` makes every draw a
function of one seed (ruling 60 forbids an OS-entropy path). Nothing in
the figure is charged to CPU speed, allocator behaviour, or scheduling.

Proof, three runs, plus the same numbers under a *different optimisation
profile* — which is the stronger claim:

```
$ cargo test --features test-util --test spec_ack_burst --release -- --nocapture --test-threads=1
O53b no-stall: 2097152 bytes in 0ns of virtual time (1838 fwd datagrams)
O53b floor: 2097152 bytes in 310ms virtual = 6.45 MiB/s (window/RTT predicts 12.50)

(run 2, identical)
O53b no-stall: 2097152 bytes in 0ns of virtual time (1838 fwd datagrams)
O53b floor: 2097152 bytes in 310ms virtual = 6.45 MiB/s (window/RTT predicts 12.50)

(run 3, identical)
O53b no-stall: 2097152 bytes in 0ns of virtual time (1838 fwd datagrams)
O53b floor: 2097152 bytes in 310ms virtual = 6.45 MiB/s (window/RTT predicts 12.50)

$ cargo test --features test-util --test spec_ack_burst -- --nocapture --test-threads=1   # DEBUG
O53b no-stall: 2097152 bytes in 0ns of virtual time (1838 fwd datagrams)
O53b floor: 2097152 bytes in 310ms virtual = 6.45 MiB/s (window/RTT predicts 12.50)
```

Identical to the nanosecond across profiles. The wall-clock `inmem/stream`
figure moved 36.8 → 50.9 MiB/s *inside a single bench run*.

### 5.3 The pinned floor

Two tests, because the literal formulation degenerates on a perfect wire:

1. **`o53b_a_bulk_transfer_over_a_perfect_wire_never_waits_on_a_timer`.**
   2 MiB over a zero-delay perfect `FlakyWire` costs **0 ns** of virtual
   time. "Half the measured value" is undefined for a zero elapsed, so the
   equivalent regression bound is stated as an elapsed ceiling at
   `K_GRANULARITY` (1 ms) — *below the smallest timer in the stack*, so
   not one timer may fire on the transfer's critical path. As a throughput
   floor that is ≥ 2048 MiB/s of virtual-time throughput. A single delayed
   ACK costs `MAX_ACK_DELAY` = 25 ms and blows it 25× over.
2. **`o53b_virtual_time_throughput_holds_its_floor`.** Over the 20 ms-RTT
   fabric the transfer is window-limited and virtual time is meaningful:
   2 MiB in **310 ms** = **6.45 MiB/s**, against the **12.5 MiB/s** that
   `INITIAL_MAX_STREAM_DATA / RTT` predicts for a static window. The floor
   is **half** the measured value: `FLOOR_MIB_S = 3.22`.

Both run in **debug and release** (0.8 s in debug together), so unlike
O53a they are in the plain `cargo test --all-features` gate too.

## 6. Gates

All nine, on the commit being reported (`95fa5b1`), pasted as run.

| Gate | Command | Result |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 5.02s` |
| Format | `cargo fmt --all --check` | no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | `Finished \`dev\` profile … in 0.09s`, zero warnings |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | `Generated …/target/doc/slither/index.html` |
| Docs (all features) | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | `Generated …/target/doc/slither/index.html` |
| Tests | `cargo test` | 881 passed, 0 failed (10 + 11 + 112 + 4 + 744) |
| Tests | `cargo test --all-features` | 23 suites all `ok`, 0 failed; `spec_ack_burst`: 2 passed, 1 ignored |
| Release tests | `cargo test --release --all-features` | all `ok`, 0 failed; `spec_ack_burst`: **3 passed** |
| Wire pins | golden-wire + size/constant tests under `cargo test` | in the 744/112 lib+spec suites above, all `ok` |
| MSRV | `cargo +1.96 check --all-features --all-targets` | `Finished \`dev\` profile … in 5.54s` |
| Supply chain | `cargo deny check` | `advisories ok, bans ok, licenses ok, sources ok` |

The `[[test]]` stanza works in both directions: the feature-less
`cargo test` **skips** the target (it is not in that run's suite list),
and `--all-features` runs it — which is exactly what ruling 194's
auto-discovery trap requires the stanza for.

## 7. Commit

`95fa5b1` — *"Ruling 260: O53a measured and pinned, O53b re-scoped to a
deterministic floor"*, no trailers. Two files, both inside the brief's
partition:

```
$ git show --stat --oneline 95fa5b1
95fa5b1 Ruling 260: O53a measured and pinned, O53b re-scoped to a deterministic floor
 Cargo.toml              |  11 +
 tests/spec_ack_burst.rs | 564 ++++++++++++++++++
```

`benches/throughput.rs` was **not** modified — it already answers O53b's
wall-clock half (ruling 247) and adding a floor to it would have turned a
measurement into a machine-specific gate, which its own stanza comment
argues against.

## 8. Findings for the maintainer

1. **`FlakyPolicy`'s rustdoc is wrong about its own construction.** It
   says *"Every field is public, so a test may build one literally"*, and
   `failing` is private — so a literal or `..base` functional update is
   `E0451` from outside the crate. It compiles *inside* `src/`, which is
   why it has survived. Working rule 8's shape: a stated construction
   whose scope is unstated and, here, false. `src/testutil/mod.rs:184`.
   Not fixed — outside this agent's file partition.
2. **Both O53a and O53b are re-scoped, not literally discharged**, and the
   ruling should say which half of each survived. O53a's letter is fully
   met. O53b's *"within 20 % of quinn under its shipped defaults"* is
   **not**, and cannot be by a test: it names another crate's figure on
   unstated hardware. What is pinned is its other clause, *"with no
   stall"*, in two forms (§5.3).
3. **The obligation's own remedy is not needed.** O53a held §19's
   range-tracker ACK in reserve *"if the numbers disappoint"*. They do
   not: 1.55 % spurious retransmits at 75 % ACK loss, against 50.9 % for a
   build whose ACK is not cumulative. The reason is structural and worth
   recording in the ruling — on a lossless forward path the replay window
   is one contiguous block, so **every** ACK carries the receiver's
   complete state and ACK loss costs timing, not information.

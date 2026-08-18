# Round 42, report C — the 8 MiB/100 ms "stall": PRE-EXISTING, and it is a kill

*Opus investigator, worktree at `8ae28a9`; throwaway commits preserved on
branch `stall/reproducer` (probes + the virtual-time reproducer,
seed 0x4200_0001, with its rule-9 survival control). Verbatim report
below.*

---

# STALL-REPORT — round 42 drain slice, phase 1

Question: is the 8 MiB/16 MiB @ 100 ms RTT stall a LATENT PRE-EXISTING
defect that the drain mutant merely rate-enables, or a defect in the
mutant itself?

Base commit (rule 14 check): 8ae28a9aeeb55845801018753dee69b459cc114e
(`Round 42 item 1: attribution complete — profile and mutant agree, and a
stall gates the slice`) — verified as first act, matches the brief.

Mutant branch `mutant/drain-attribution` head: 2bfbede (three throwaway
commits 252c232, 76b05ab, 2bfbede).

## Prior art (read targeted, per rule 1)

**Ruling 249** (`rulings.md:7630`) — *the `Pto` deadline is announced only
while §7.3's amplification budget admits a probe.* While the budget admits
none, the announced `Timeout` falls to the next armed timer and the
recovery state is untouched: **the gate is on the announcement, not the
timer state**. Re-arming needs no machinery because "the budget grows only
on an authenticated, window-fresh receive, and every receive already
recomputes the `Timeout`."

**Ruling 249's addendum (added by 265)** — 249's *"`Liveness` at the
latest"* has an unstated scope. True where 249 measured it; **false as a
general backstop**, because §7.4 disarms `Liveness` at the very receive
that sets the passive keepalive's debt. "this sentence must not be copied
into any further gate clause without it."

**Ruling 265** (`rulings.md:8555`) — the immortal park: keepalive owed +
§7.3 budget starved + `Liveness` disarmed = **every timer `None`**,
connection alive and silent at +60 s virtual. Fixed by a death-clock
backstop in `sync_liveness_timer`: while a keepalive is owed and vetoed,
announce `last_authenticated_recv + DEAD_TIMEOUT`.

**Ruling 255** (`rulings.md:8089`) — the past-deadline guard counts a
streak of three; benign overdue chains are length one.

**Hypothesis formed before measuring.** The shape to look for is 265's
class one door down: **data outstanding** (not a keepalive owed) +
§7.3 budget starved → `Pto` announcement vetoed by 249(i) + `Liveness`
disarmed + no keepalive owed → 265's backstop does not arm → every timer
`None` **with data in flight**. If that is the state, the defect is
pre-existing and the mutant only reaches it faster.

## Mutant read (source diff, `git diff d8bb652..HEAD -- src/`)

The offset-cursor change touches exactly five sites in
`src/core/connection/send.rs`, and `self.buf` has no other reader:

```
290: self.buf.extend_from_slice(&data[..room]);   // write(), unchanged
339/363: self.buf = Vec::new(); self.head = 0;    // the two resets
586-588: slice(): lo = head + (start-base), hi = head + (end-base)
593: buffered() = buf.len() - head
609-622: release(): head += drop; base += drop; compaction at head >= buffered()
```

The representation invariant `buf.len() - head == write_offset - base`
holds by construction: `write()` grows `buf` by exactly the `room` it adds
to `write_offset`, and `release()` advances `head` and `base` by the same
`drop`. Nothing else reads `buf.len()`, so **no shipped caller can observe
the dead prefix** — in particular `write()`'s `room` is computed from
`max_data - write_offset` and `conn_room` (offsets), never from `buf.len()`.
`slice()` is correct under compaction because `head` and `base` move
together. The prime suspect named in the brief (*retransmission reads via
`slice()` after compaction*) is therefore **not** a defect on inspection:
a retransmit range always has `start >= base`, and `head + (start - base)`
is the same byte before and after a `copy_within` that decrements `head`
and leaves `base` alone.

Consequence: the mutant is semantically identical to baseline modulo the
buffer's peak length (2x) and the speed. It is a **rate** change.
## Reproduction

Merged the mutant into the worktree, added a throwaway deep-state probe
(`probe_state()` on the core, on the shell handle, and per-module `probe()`
dumps in `send.rs`/`recv.rs`/`flow.rs`/`recovery.rs`/`mobility.rs`/
`streams.rs`), a `stall` scenario reaching the single 8Mi/16Mi @ 100 ms
cell, and a watchdog that dumps both cores when the wire stops moving.

```
$ SLITHER_STALL_WATCH=1 SLITHER_STALL_RTT=100 \
    target/release/examples/bench_vs_tcp stall
=== STALL DUMP #1 quiet_for=5.0s wire_a=[20315, 24320345, 7954, 507562] \
                                 wire_b=[7954, 507562, 17646, 21128046]
--- A (sender) CORE GONE
--- B (receiver) CORE GONE
    A established=false B established=false
```

**The connection is not parked. It is dead, on both sides.** `probe_state()`
returns `None` because `cell.core` is `None` — §15.2 has already dropped the
state. The six minutes of 0 % CPU are the *harness*, not the protocol.

Second run, with the two application tasks reporting why they exited:

```
$ SLITHER_STALL_WATCH=1 SLITHER_STALL_RTT=100 \
    target/release/examples/bench_vs_tcp stall
!!! READ ERROR: ConnectionLost(ProtocolViolation { code: 1 })
!!! WRITE ERROR after 21317 bytes of this chunk: \
      ConnectionLost(PeerClosed { code: 1, reason: [] })
```

`code: 1` is `PROTOCOL_VIOLATION` (`src/constants.rs:513`). **The receiver
kills the connection**; the sender learns of it as `PeerClosed`. Wire
counters at death: A sent 20 363 datagrams / 24.38 MB, B received 17 725 /
21.22 MB — **2 638 datagrams (13.0 %) lost on the forward path.**

### Why it looked like a stall (and why that is the mutant's, but harmless)

`slither_bulk`'s task set is joined with `for t in tasks { t.await }`. When
the connection dies mid-transfer both the writer and the reader return, but
`BulkState::done` is only set by the **meter**, which never fills. The
mutant's own RTT-probe task (commit `252c232`) loops `while !done.get()`
forever, so the join never completes: 0 % CPU, parked on a 200 ms sleep,
every protocol timer genuinely gone because every core is gone.

That is a **bench-harness** defect introduced by the measurement mutant, and
it is why the failure presented as a hang instead of an error. It is not the
protocol defect, and it is not why the cell fails.
## State at stall

The death is at `src/core/connection/recv.rs:882` — §10.6's reassembly
ceiling. Instrumented at both `PROTOCOL_VIOLATION` emitters (the frame
decoder's `Received::Structural` in `mod.rs`, and this one) to tell them
apart; only this one fires:

```
$ SLITHER_STALL_WATCH=1 SLITHER_STALL_RTT=100 \
    target/release/examples/bench_vs_tcp stall
!!! MUTANT PROBE: REASSEMBLY CEILING HIT chunks=1025 max=1024
!!! READ ERROR: ConnectionLost(ProtocolViolation { code: 1 })
!!! WRITE ERROR after 4007 bytes of this chunk: \
      ConnectionLost(PeerClosed { code: 1, reason: [] })
```

`REASSEMBLY_CHUNKS_MAX = 1024` (`src/constants.rs:323`), §10.6's
*"a stream whose stored discontiguous ranges would exceed
`REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol violation:
CLOSE with `PROTOCOL_VIOLATION`"* (`SPEC.md:4393-4397`).

This is **not** ruling 265's family. No timer is idle-with-work-outstanding;
there is no timer at all, because there is no connection. The all-timers-idle
sample the brief describes is the *post-mortem*: both cores dropped, both
handles `CORE GONE`, and only the harness's own tasks left sleeping.

## Separation experiments

`src/core/connection/send.rs` was reverted to `d8bb652`'s (pre-mutant)
content — `git diff d8bb652 -- src/core/connection/send.rs` shows **only**
the added throwaway `probe()`, 30 insertions and no deletions — and the
same cell re-run. Loss was then varied as the single independent variable
with a throwaway knob in the bench's relay, protocol code untouched.

| # | send.rs | forward-path loss shape | Result |
|---|---|---|---|
| 1 | mutant | real (socket overflow, ~13 %) | **ceiling hit, chunks=1025**, cell dies |
| 2 | **baseline** | real (none injected, 1.98 % measured) | completes, 5.65 MiB/s, `inflight_max=8 311 855` |
| 3 | baseline | uniform 1-in-50 (2 %) | completes, 0.07 MiB/s, `cwnd_max=13 317` |
| 4 | baseline | bounded queue, tail-drop at 500 dgrams | completes, 5.17 MiB/s, `cwnd_max=775 954` |
| 5 | **baseline** | **one-shot alternating burst** (3 000 dgrams from #4 000) | **ceiling hit, chunks=1025**, cell dies |

```
$ SLITHER_STALL_WATCH=1 SLITHER_STALL_RTT=100 \
    SLITHER_RELAY_BURST_AT=4000 SLITHER_RELAY_BURST_LEN=3000 \
    target/release/examples/bench_vs_tcp stall
!!! MUTANT PROBE: REASSEMBLY CEILING HIT chunks=1025 max=1024
!!! READ ERROR: ConnectionLost(ProtocolViolation { code: 1 })
!!! WRITE ERROR after 44155 bytes of this chunk: \
      ConnectionLost(PeerClosed { code: 1, reason: [] })
```

**Row 5 settles it: unmutated code, a fully conforming sender inside its
advertised credit, and a network that loses packets — same death.**

Rows 3 and 4 are the important negative controls, and they explain why this
was never seen before. A *steady* loss rate cannot reach the ceiling: NewReno
drives the window down to where fewer than 1024 packets are ever in flight
(row 3's `cwnd_max` is 13 KB — eleven packets). The hazard needs
**zero steady loss, so the window grows to its flow-control limit, and then a
transient mass-loss event**. That is exactly the shape of a saturated socket
buffer, and it is why the mutant reaches it: at ~90 MiB/s it overruns a
buffer the baseline at 5.6 MiB/s does not.
## LEG 3 — where the loss actually happens

Instrumented the relay's forward direction with cumulative counters and
compared them against the endpoints' own wire counters (baseline `send.rs`,
no injected loss, the real 100 ms path):

```
$ SLITHER_STALL_WATCH=1 SLITHER_STALL_RTT=100 \
    target/release/examples/bench_vs_tcp stall
CUMULATIVE wire_a(out_dg,out_B,in_dg,in_B)=[127226, 152586672, 58695, 2380761] \
           wire_b=[58695, 2380761, 117208, 140591833]
RELAY fwd: in=127226 deliberate_drops=0 out=127226
```

* A sent **127 226** datagrams.
* The relay **received all 127 226** — zero loss at the relay's receive socket.
* The relay **forwarded all 127 226** — zero loss in its (unbounded) queue.
* B received **117 208**.

**10 018 datagrams (7.9 %) die in the receiving endpoint's own UDP receive
buffer.** Not the relay, not the fabric. `net.inet.udp.recvspace` on this host
is **786 896 B ≈ 655 datagrams at a 1200 B MTU**, and the bench sets no
`SO_RCVBUF` anywhere (its own header says so, `examples/bench_vs_tcp.rs:135`).
The reverse (ACK) direction is lossless: `wire_b.out_dg == wire_a.in_dg`
exactly.

So the loss location is a **saturated receiver socket**, and the burst shape
follows directly: while B's driver is descheduled the buffer fills and the
kernel tail-drops; when it runs, it drains and datagrams land again. Alternating
runs of drops and arrivals is precisely the pattern that maximises the
discontiguous-range count.

At 5.6 MiB/s a 786 KB buffer is ~140 ms of headroom and B keeps up. At the
mutant's ~90 MiB/s it is ~8.7 ms — below one scheduling quantum. **That, and
only that, is what the drain mutant changes.**

This is not a bench artefact. Any receiver momentarily descheduled — a
scheduler quantum, a GC pause, a busy core — overruns its socket buffer, and
on a high-BDP path where the window has grown lossless, the resulting hole
pattern kills a fully conforming connection.
## LEG 2 — the same state, at baseline, in virtual time

`tests/throwaway_reassembly_ceiling.rs` (throwaway; the **seed** for phase 2's
regression test, not the test itself). Unmutated `send.rs`, `FlakyWire`, tokio
paused clock, no sleep, one seed (`0x4200_0001`), fully deterministic.

Construction, in three steps:

1. `Pair::seeded_with(0x4200_0001, Config::new().with_flow_windows(8 MiB, 16 MiB))`
   — the bench cell's own windows.
2. A lossless 5 ms one-way fabric while the writer saturates, so slow start
   takes the window to its flow-control limit. The burst is armed the first
   tick `cwnd > 2 600 000` — i.e. once one flight can carry more than
   2 × 1024 packets, so more than 1024 holes can coexist before any
   retransmission can arrive.
3. `FlakyPolicy::drop_at` on **every even send index** for the next 6 000
   sends — the alternating pattern a tail-dropping socket buffer produces as
   it fills and drains. Deterministic; no RNG.

Result (first run, before the assertions were tidied — the raw evidence):

```
ARMING burst at send index 5502, cwnd=2612439
!!! MUTANT PROBE: REASSEMBLY CEILING HIT chunks=1025 max=1024
```

and the two ends' views, asserted:

* receiver: `ReadError::ConnectionLost(ProtocolViolation { code: 1 })`
* sender:  `WriteError::ConnectionLost(PeerClosed { code: 1, .. })`

Nothing in the construction is adversarial. The sender writes 64 KiB chunks
and never exceeds the credit it was granted; the reader drains continuously;
the only unusual thing on the path is that some datagrams do not arrive.

**Working rule 9 — the control.** `without_the_burst_the_same_transfer_is_
untroubled` is the same test with the burst never armed, and asserts the
connection is alive and the window reached the same size. Without it, "the
connection died" would pass on a build that dies for any reason.

### The reachability curve

`tests/throwaway_reachability_curve.rs` runs one construction at four window
sizes and reads the receive half's high-water discontiguous-range count
(a throwaway `MAX_CHUNKS` high-water mark in `recv.rs`):

```
CURVE windows=ratified 256Ki/1Mi     stream_window=  262144 max_chunks=   83
CURVE windows=1Mi/4Mi                stream_window= 1048576 max_chunks=  267
CURVE windows=4Mi/8Mi                stream_window= 4194304 max_chunks=  946
CURVE windows=8Mi/16Mi (bench cell)  stream_window= 8388608 max_chunks= 1025  <- CEILING
```

Linear in the stream window, as it must be: the hole count is bounded by the
packets one flight can carry, and the flight is bounded by the window.
**The ratified 256 KiB default has a 12x margin. 4 MiB has 8 %. The crossing
is at roughly 4.5 MiB.**

That is the answer to *"why has this never been seen"*: at the ratified
constants the hazard is unreachable by construction, and every default-window
test in the suite is inside the safe region. Working rule 13 again — the
fixture bounded the coverage, and this time the bound was the **window
setting**, not the fabric.

## Verdict

**PRE-EXISTING.** The drain mutant contains no defect; it rate-enables one.

**Mechanism, at file:line.**
`src/core/connection/recv.rs:882` — `if self.chunks.len() >
constants::REASSEMBLY_CHUNKS_MAX { return Err(Violation::Reassembly) }` —
implementing §10.6 (`SPEC.md:4393-4397`), whose code is
`constants::PROTOCOL_VIOLATION` via `Violation::code()`
(`src/core/connection/flow.rs:102`), with the constant at
`src/constants.rs:323` (`REASSEMBLY_CHUNKS_MAX = 1024`).

A stream's advertised credit and its tolerated hole count are set by two
independent constants that do not scale together. The credit is the
configured window — 8 MiB in the failing cell, ~6 990 packets at the observed
1 200 B MTU. The hole tolerance is a flat 1 024. A sender inside its credit,
on a path that loses packets in the pattern a saturated socket buffer
produces, exceeds the second while obeying the first, **and the receiver
answers by killing the connection and blaming the peer.**

**Why the mutant is not the defect.** `SendHalf`'s offset-cursor change
touches five sites in one file; `self.buf` has no other reader; the invariant
`buf.len() - head == write_offset - base` holds by construction; `slice()` is
correct across compaction because `head` and `base` move together. The
brief's prime suspect (retransmission via `slice()` after compaction) is not a
defect on inspection, and the empirical separation is stronger than the
inspection: **reverting `send.rs` to `d8bb652` verbatim and injecting the loss
shape by hand reproduces the identical death** (`chunks=1025`), on the real
path and again in virtual time.

**What the mutant does change** is the rate — ~5.6 MiB/s to ~90 MiB/s — and
therefore whether the receiving endpoint's 786 896 B UDP socket buffer
overflows. That buffer is ~140 ms of headroom at baseline and ~8.7 ms under
the mutant, i.e. below one scheduling quantum. The mutant is a **stimulus**,
not a cause.

**Not ruling 265's family.** The brief's expected shape — data outstanding
with every timer `None` — is not what is there. There are no timers because
there is no connection: both cores are dropped and both handles report
`CORE GONE`. The observed idleness is the post-mortem.

## Slice consequences

### 1. A ruling is needed, and the spec already left the hook

§10.6, `SPEC.md:4398-4400`: *"The ceiling value ships
**ratified-but-revisitable**, gated on the Appendix B
defragmentation/throughput check."* That gate is **live and unsatisfied** —
`round41-H-audit-triage.md:125` and `round41-J-O53-discharge.md:34` record
O53b as a stated Appendix B obligation standing word-for-word at HEAD:
*"bulk-transfer throughput … **with no stall**, measured with the §10.6
reassembly bound active."*

**Phase 1 has now run that gate, and it is red.** Not slow — fatal. This is
the evidence §10.6 named in advance as the thing that would revisit the
value, so phase 2's ruling is inside a hook the spec left open, not a
re-litigation of a frozen constant. `REASSEMBLY_CHUNKS_MAX` is also, by
`src/constants.rs:315-322`'s own words, *"receiver policy, observable"* — the
**third** kind, neither a wire constant nor a §10.2 table row (ruling 103) —
so moving it moves no wire byte and turns no golden-wire test red.

### 2. The collision with ruling 269, which is why this gates the slice

Ruling 269(ii) ships the sizing guidance *"size ≈ 2 × RTT × target rate, and
not larger"*, and records the measured 100 ms optimum as **2 MiB / 8 MiB**.
Applied at 100 ms RTT that rule recommends a **stream** window of
2 × 0.1 s × target — 5 MB at 25 MiB/s, 10 MB at 50 MiB/s. Both are past the
measured 4.5 MiB crossing. **slither's own ratified sizing advice steers
operators into the hazard**, and the drain fix, whose entire purpose is to
make large windows affordable, removes the last thing discouraging them.
Landing the drain fix without this is landing a footgun and then oiling it.

### 3. Design constraints for phase 2's fix

* **The ceiling must scale with the advertised credit, because §10.6's own
  mandate is already stated that way**: *"per-stream reassembly state MUST be
  O(advertised credit) and MUST NOT scale with the number of received
  frames."* A flat 1 024 is **stricter** than the mandate, and the strictness
  is exactly what kills honest peers. A ceiling derived from the window —
  order `advertised_window / MAX_DATAGRAM`, floored at today's 1 024 so no
  default-window receiver becomes more permissive — satisfies the mandate,
  keeps metadata a small fraction of the credit already committed, and is
  unreachable by an honest sender **by construction**, since no sender can
  open more holes than it has packets in flight.
* **It must not weaken the anti-amplification argument §10.6 exists for.**
  That argument is about *1-byte frames at alternating offsets* — ~512 000
  ranges inside 1 MiB of credit. Dividing by a **packet-size** constant, not
  by 2, is what separates the honest case (holes <= window/MTU) from the
  adversarial one (ranges <= credit/2). Phase 2 must state that separation
  explicitly; it is the whole soundness of the change.
* **Ruling 253's work bound is untouched and must stay so.** The
  small-to-large coalescing discipline bounds *copy work*; this changes only
  how many ranges may be *stored*. Any candidate must be checked against
  §10.6's amended clause, not only its first paragraph — working rule 4(a).
* **Open question the maintainer should rule on, not the implementer:** even
  with a raised ceiling, crossing it is dispositioned as
  `CLOSE(PROTOCOL_VIOLATION)` — the receiver blames the peer for the
  network's behaviour. §10.6 calls the ceiling a *tolerance*; the code calls
  breaching it a *violation*. Whether an honest-conditions breach deserves a
  different disposition is a separate decision from the value, and phase 2's
  brief should say which one it is deciding.

### 4. The regression test

`tests/throwaway_reassembly_ceiling.rs` is the seed, not the deliverable. It
asserts **today's** behaviour (the connection dies); phase 2's test asserts
the **inverse** — that the same construction completes. Carry over:

* the exact seed `0x4200_0001` and the 5 ms one-way fabric;
* the arming rule (burst armed the first tick `cwnd > 2 600 000`, so more
  than 1 024 holes can coexist before any retransmission lands) — reached at
  send index **5502**, `cwnd=2612439`, tick **20**;
* `FlakyPolicy::drop_at` on every even index for 6 000 sends — deterministic,
  no RNG, and the drop is proved by the index set rather than configured;
* **the control** (`without_the_burst_…`), which is what makes it a test
  rather than an observation (working rule 9): same windows, same fabric, no
  burst, connection alive at `cwnd=91 159 239`, `in_flight=4 374 124`;
* the reachability curve as a **second** test — the ratified default must
  stay in the safe region whatever the fix does to the ceiling.

Note for the phase-2 briefs: the honest reproduction is `drop_at`, **not**
`FlakyPolicy::lossy`. Rows 3 and 4 above show why — a *steady* loss rate
drives NewReno's window below the hazard, and a test built on `lossy` would
pass on a broken build for the wrong reason. That is working rule 9's shape
in the fixture rather than in the assertion.

### 5. Two harness defects to fix before phase 2 measures anything

* **`slither_bulk` hangs instead of reporting a dead connection.** The
  mutant's RTT-probe task loops `while !done.get()`, and `done` is set only
  by the meter, so a cell whose connection dies never joins. That is what
  turned a `PROTOCOL_VIOLATION` into "six minutes of 0 % CPU". Any task in
  that join set must also exit on connection loss.
* **A died cell reports a number instead of failing.** `BulkState::finish()`
  (`examples/bench_vs_tcp.rs:815`) returns whatever samples the meter
  collected, so a cell that dies mid-run emits a `BENCH` line with `n < 5`
  and no indication that anything went wrong. Phase 2 will be reading these
  lines to decide whether the drain fix worked.

### 6. Working-rule notes

* **Rule 13, again, and with a new bound.** No test in the suite could have
  found this: `FlakyWire` can express the loss, but every test that
  configures a raised window is a *flow-control* test that never puts 2 000
  packets in flight, and every test that moves bulk data uses the ratified
  window, where the hazard is unreachable. The fixture was adequate; the
  **parameter space** was not. Worth recording as a distinct instance —
  rule 13's existing examples are all about what the fabric can express.
* **Rule 12, on this report.** The verdict rests on the claim that unmutated
  `send.rs` reaches the state. That claim is not an inspection argument; it
  is `git diff d8bb652 -- src/core/connection/send.rs` showing 30 insertions
  and no deletions, plus two independent reproductions on that tree. The
  inspection of the mutant is offered as corroboration, and it should be
  read as the weaker half.

## Addendum — why only the 100 ms cell, under the mutant

The brief's own data answers this once the mechanism is named. From
`measurements/stall-probe.log` (the mutant's run, before this investigation):

```
20 ms  8Mi/16Mi  79.13 MiB/s  inflight_p50=2 424 000  loss=-0.0001
50 ms  8Mi/16Mi  37.69 MiB/s  inflight_p50=2 206 800  loss= 0.0005
100 ms 8Mi/16Mi  STALL
```

At 20 and 50 ms the in-flight window settles at ~2.3 MB — under the ~2.5 MB
crossing this investigation measured, so **fewer than 1 024 holes can exist
at once no matter what the network does**, and the measured loss is zero.
At 100 ms the bandwidth-delay product is 2-5x larger; baseline alone reaches
`inflight_max=8 311 855` in the same cell. The mutant's rate then overruns
the receiver's socket buffer, and the window is already three times past the
crossing when it does.

So "only 100 ms fails" is not a property of 100 ms. It is the one cell in the
matrix where the in-flight window exceeds 2 x 1 024 x MTU, which is the only
place the hazard exists at all.

## Provenance

Base commit verified as first act: `8ae28a9aeeb5…` (rule 14). Mutant merged
at `ab416e8`. Throwaway commits in this worktree: `31d02df` (deep probe +
stall scenario), `d0ced36` (baseline reproduces under burst loss), `b1eddf3`
(loss attribution). Nothing outside this worktree was touched; `.claude/` was
not touched; no wire constant, header layout, frame type or timer was
modified. `SPEC.md` and `rulings.md` were read only by targeted `grep -n` plus
bounded `sed -n` ranges (rule 1).

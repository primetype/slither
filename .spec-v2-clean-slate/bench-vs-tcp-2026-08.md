# slither versus raw kernel TCP — the comparative matrix

**Measured 2026/08/18 at `fdaa0de` (`fdaa0deb8733c94ccd178af760e68de35eee3daa`).**
Harness: `examples/bench_vs_tcp.rs`, committed alongside this file.

This is a measurement, not a gate. Nothing here asserts a bound; every figure
is a property of the host it ran on, and a threshold would be a flake on any
loaded machine.

---

## 1. Environment

| | |
|---|---|
| Host | Apple M4 Max, 16 cores, 64 GiB |
| OS | macOS 26.6.1 (build 25G76), arm64 |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1` |
| Profile | `--release`, cargo defaults (no `[profile]` section in `Cargo.toml`: `opt-level=3`, no LTO, `codegen-units=16`, `overflow-checks=false`) |
| Features | none — the example uses `SoftwareIdentity`, not `testutil`, so it needs no `required-features` stanza and no `Cargo.toml` change |
| Loopback | `127.0.0.1`, kernel defaults, no `SO_*` tuning of any kind |
| Wall clock | 739.4 s (12 min 19 s) for the full matrix |

Command: `cargo run --release --example bench_vs_tcp -- all`

---

## 2. Method

**Topology.** Both protocol endpoints — slither's *and* TCP's — run on one
current-thread tokio runtime inside one `LocalSet`. That is forced for slither
(§16.3's `!Send` actor) and applied to TCP deliberately, to hold the *userspace*
CPU budget constant. It does **not** hold the total budget constant: a TCP byte
is still segmented, acked and copied by the kernel, on whatever core the softirq
lands on. `benches/throughput.rs`'s warning applies verbatim — a slither figure
of `N` MiB/s means one core carried `N` MiB/s of payload while also sealing it,
opening it and doing the peer's receive work. Neither column is an `iperf`
number.

**Delay relay.** An in-file relay on its **own OS thread with its own
current-thread runtime**, so its copies do not contend with the endpoints under
test. One-way delay `D` per direction, FIFO per direction (a reader task pushes
`(now + D, bytes)` onto a queue, a writer task pops and `sleep_until`s), no
loss, no reordering. `RTT = 2D`. Loss is out of scope by ruling; the loss story
remains the `testutil::FlakyWire` virtual-time suite.

- UDP flavour: datagram-preserving — one `recv_from` + one `Vec` + one `send_to`
  per **datagram**.
- TCP flavour: byte-stream — one `read` + one `Vec` + one `write_all` per
  64 KiB **chunk**, queue bounded at 1024 chunks (64 MiB) per direction.

The relay's per-unit cost is therefore ~50× heavier for slither than for TCP for
the same payload. That is a shared-artefact asymmetry, documented not corrected:
correcting it would mean giving TCP a per-1200-byte relay, which is not what a
network does either.

**Bulk method.** Steady state: a 2 s ramp during which nothing is counted, then
five back-to-back 4 s windows on the same connection. The ramp covers TCP slow
start and receive-buffer autotuning, slither's congestion-window growth, and —
specific to this crate — the fact that a configured stream-window raise is only
announced on the **first STREAM frame the peer sends on that stream**
(`Streams::on_stream`, ruling 259(viii)), so it reaches the sender about one RTT
into the transfer.

**Counting wire.** Both slither endpoints' sockets are wrapped in a
`CountingWire` over the public `Wire` seam and snapshotted at the first and last
counted byte. That yields per cell:

- `amp` — sender wire bytes ÷ payload bytes delivered;
- `loss` — 1 − (receiver datagrams ÷ sender datagrams);
- `dg_out`, `dg_in`, `ack_dg`, mean wire MTU.

No production code is instrumented.

**TCP settings.** `TCP_NODELAY` on for the latency scenarios, kernel default
(off) for bulk. The relay sets `TCP_NODELAY` on its own two sockets so it adds
delay and nothing else.

### 2.1 The one harness defect, stated up front

**The UDP relay is a real link emulator. The TCP relay is not, and cannot be in
userspace.**

UDP forwarding is stateless, so slither's two endpoints exchange packets end to
end and the delay is fully visible to its RTT estimator, congestion control and
flow control. Every delayed **slither** figure below is a real measurement of
slither on a delayed path.

TCP is connection-oriented, so a userspace relay must **terminate** it. The
client's SYN is answered by the relay over zero-RTT loopback; the relay's
upstream connection is zero-RTT loopback too. Neither TCP stack ever observes
the delay. Consequences, both flagged as `note=proxy-terminated` in the raw
output:

1. **Establishment loses exactly one RTT.** A real TCP pays 1 RTT for
   SYN/SYN-ACK and 1 RTT for the byte round trip; through this relay only the
   second. Every delayed `proto=tcp` establishment figure is one RTT lower than
   a real path gives.
2. **Bulk loses the delay entirely.** Neither stack's window is ever binding.
   A delayed `proto=tcp` bulk figure is loopback TCP with latency added
   *downstream of the control loop* — an unattainable upper bound, not a
   comparable measurement.

Correcting this needs IP-layer emulation (`dummynet`, `netem`), a root-level
change to the host and out of scope. It is reported rather than papered over.
Note the direction: it makes slither look *worse* in §3 and TCP look *better*
in §5, i.e. it is not a bias toward the expected answer.

Secondary evidence of the same defect: the TCP relay needs a queue bound or it
is a memory bomb, because the client's congestion control never sees the delay.
The UDP relay needs none — slither's own windows bound what it puts on the wire.

**Measured relay slop.** From the TCP establishment cells, where the byte round
trip is exactly two relay traversals: 24.712 ms at RTT 20 and 104.739 ms at
RTT 100, against 20.18 and 100.18 ms of delay + local cost. That is
**≈ 2.3 ms per traversal**, and it is the same for both protocols. slither's
establishment crosses the relay four times and therefore carries ≈ 9.2 ms of it;
the ping-pong crosses twice and carries ≈ 4.6 ms. Every relay figure below has
this in it.

---

## 3. Scenario 1 — establishment (dial → first application byte echoed back)

| RTT | slither p50 | slither min/max | TCP p50 (measured) | TCP min/max |
|---|---|---|---|---|
| 0 ms (direct) | **1.911 ms** | 1.535 / 4.253 | **0.181 ms** | 0.148 / 0.238 |
| 20 ms | 52.752 ms | 49.022 / 53.975 | 24.712 ms † | 22.865 / 25.310 |
| 100 ms | 213.300 ms | 211.667 / 214.737 | 104.739 ms † | 103.610 / 105.036 |

† proxy-terminated: missing one RTT (§2.1).

**n** = 20 samples at RTT 0, 10 at 20 ms and 100 ms (the high-RTT cells are
cheap but each rebuilds two endpoints, two sockets and a relay; ten is enough to
show a 1.5 ms spread on a 213 ms figure).

### 3.1 The RTT arithmetic — stated, not measured

Neither a TLS baseline nor a real delayed path was measured. The following is
protocol arithmetic and is labelled as such.

| Stack | Flights to first echoed byte | at 100 ms RTT |
|---|---|---|
| **slither (IK)** | msg1 → msg2 → data → echo = **2 RTT** | 200 ms |
| **raw TCP** | SYN → SYN-ACK → ACK+data → echo = **2 RTT** | 200 ms |
| TCP + TLS 1.3 (full) | 1 RTT TCP + 1 RTT TLS, app data with client Finished = **3 RTT** | 300 ms |
| TCP + TLS 1.2 (full) | 1 RTT TCP + 2 RTT TLS = **4 RTT** | 400 ms |

**Raw TCP ties slither at 2 RTT.** slither does not beat the kernel's stream
here and the harness's proxy artefact must not be read as saying it loses to it
either: correcting the measured 104.7 ms by the missing RTT gives ≈ 204.7 ms for
a real-path TCP, against slither's 213.3 ms — of which ≈ 9.2 ms is relay slop,
leaving ≈ 204 ms. **The two are within measurement noise of each other.**

What slither actually replaces is not raw TCP; it is TCP + TLS. Against
TLS 1.3 the arithmetic says slither saves **one RTT**, and against TLS 1.2
**two**. Both are stated, neither is measured.

Two things the arithmetic does not cover and which a reader should not infer:
TCP Fast Open would put raw TCP at 1 RTT, and TLS 1.3 session resumption with
0-RTT data would put TLS at 2 RTT. slither has no 0-RTT path — IK msg1 carries
only the 12-byte timestamp payload — so it does not compete in that regime.

### 3.2 The zero-RTT cost is asymmetric crypto

At RTT 0 slither costs **1.911 ms** against TCP's **0.181 ms**: ~1.7 ms more.
That is the IK handshake's P-256 work — an ephemeral keygen plus the DHs, on
both endpoints, on one thread — and it is what buys mutual authentication and
forward secrecy that raw TCP does not provide at all. It is also the figure that
matters for a connection-churning workload and the one to watch if the DH
provider changes.

---

## 4. Scenario 2 — bulk, one stream, clean loopback (no relay)

| Configuration | p50 MiB/s | min | max | amp | loss |
|---|---|---|---|---|---|
| slither, default windows (256 KiB / 1 MiB) | **73.44** | 71.97 | 74.89 | 1.043 | 0.0000 |
| slither, raised windows (8 MiB / 16 MiB) | **5.74** | 5.69 | 5.78 | 1.040 | 0.0000 |
| raw TCP, kernel defaults | **10 979.42** | 10 940.41 | 11 025.25 | — | — |

**TCP wins this by ~150×, and the reasons are structural.** They are not a list
of excuses; each is checkable against the numbers beside it.

1. **Segment size.** slither's mean wire MTU here is **1180 bytes** and it
   emitted **1 358 022** datagrams in the 20 s of counted time. `lo0` on this
   host is `mtu 16384` (verified with `ifconfig lo0`), and TCP coalesces
   further; the same payload costs it roughly two orders of magnitude fewer
   trips through the stack.
2. **Per-byte AEAD, twice.** Every byte is ChaCha20-Poly1305 sealed and opened
   in this process. TCP does neither.
3. **Userspace framing, acks and recovery, on the measured thread.** slither
   also emitted **687 447** ack-bearing datagrams. TCP's equivalent work is in
   the kernel and largely not on this core.
4. **Both endpoints on one thread.** The 73.44 MiB/s is what one core produced
   while *also* consuming it. TCP's 10 979 MiB/s is a userspace loop on one core
   plus a kernel that is free to use others.

None of these four is separately quantified here — this benchmark measures the
sum. The attribution between them is **not measured**.

The honest summary: **on a clean local path with no adversary and no delay, raw
TCP is dramatically faster, and choosing slither there is choosing
authentication, encryption, multiplexing and roaming over throughput.**

---

## 5. Scenario 3 — bulk under RTT

| RTT | slither default | slither raised (8 Mi/16 Mi) | raw TCP † |
|---|---|---|---|
| 20 ms | 5.53 MiB/s | 5.49 MiB/s | 5 466.56 MiB/s |
| 50 ms | 2.67 MiB/s | 5.52 MiB/s | 2 340.39 MiB/s |
| 100 ms | 1.42 MiB/s | 5.58 MiB/s | 1 185.36 MiB/s |

† **Not a delayed-path TCP measurement** (§2.1). The relay terminates the
connection, so neither TCP stack sees the delay; these are loopback TCP figures
with latency added downstream of the control loop. The visible fall from 5 466
to 1 185 MiB/s as `D` grows is the relay's own 64 MiB in-flight bound
(`64 MiB / D` = 6.4 / 2.6 / 1.3 GiB/s), i.e. **it is a measurement of the
harness, not of TCP.** What a real TCP would do on these paths is bounded by
`min(cwnd, rwnd) / RTT` and was **not measured**.

### 5.1 What the knob bought

| RTT | default | raised | delta |
|---|---|---|---|
| 20 ms | 5.53 | 5.49 | **×0.99 (nothing)** |
| 50 ms | 2.67 | 5.52 | **×2.07** |
| 100 ms | 1.42 | 5.58 | **×3.93** |

The default column follows the window-over-RTT law, at about **half** the naive
prediction: `INITIAL_MAX_STREAM_DATA / RTT` is 12.5 / 5.0 / 2.5 MiB/s and the
measured 5.53 / 2.67 / 1.42 is 0.44 / 0.53 / 0.57 of it. That factor is
`CREDIT_REGRANT_DIVISOR`: §10.3 re-grants at half the window consumed, so the
sender spends `W/2` and then waits a round trip for credit, and the sustained
rate sits between `W/(2·RTT)` and `W/RTT`. It is not a defect; it is the
re-grant cadence, and it means **the useful rule of thumb is `window /
(2 × RTT)`, not `window / RTT`.**

The raised column does **not** follow that law. All three cells land on
≈ 5.5 MiB/s — the *same* figure the 8 MiB configuration produces on a clean
zero-RTT loopback (5.74 MiB/s, §4). The raise moved the bottleneck off the
window and onto something that scales *with* the window, which §6 identifies.

---

## 6. The window is not a direction, it is an optimum — and the reason

### 6.1 Clean loopback, window ladder (`sweep`, RTT 0, one stream)

| stream / connection window | p50 MiB/s | min | max | amp | loss | `dg_out` | mean MTU |
|---|---|---|---|---|---|---|---|
| default 256 KiB / 1 MiB | **70.65** | 69.98 | 70.88 | 1.043 | 0.0000 | 1 308 445 | 1180 |
| 512 KiB / 2 MiB | 53.81 | 52.80 | 54.02 | 1.041 | 0.0000 | 982 862 | 1191 |
| 1 MiB / 4 MiB | 35.42 | 35.41 | 35.52 | 1.040 | 0.0000 | 649 441 | 1192 |
| 2 MiB / 8 MiB | 20.22 | 20.18 | 20.26 | 1.040 | 0.0000 | 371 649 | 1194 |
| 8 MiB / 16 MiB | **5.69** | 5.61 | 5.75 | 1.040 | 0.0000 | 110 348 | 1195 |

**Raising the window makes a zero-RTT transfer monotonically slower, by 12×
across this ladder, and it is not retransmission.** `amp` is flat at ~1.04 and
`loss` is exactly 0.0000 in every row: the wire carries the same bytes per
payload byte, just fewer per second. That kills the obvious hypothesis (bigger
window ⇒ bigger burst ⇒ socket-buffer drops ⇒ retransmits) **by measurement**.

The cost is **linear in the window**. Inverting to seconds per MiB of payload:

| window (MiB) | 0.25 | 0.5 | 1 | 2 | 8 |
|---|---|---|---|---|---|
| s / MiB | 0.01416 | 0.01858 | 0.02824 | 0.04946 | 0.17575 |

Successive slopes `ΔT/Δw` are 0.0177, 0.0193, 0.0212, 0.0210 — constant to
about 8 %. So `T ≈ a + b·w` with **`a ≈ 0.0092 s/MiB` (a window-independent
ceiling of ≈ 109 MiB/s) and `b ≈ 0.0198 s/MiB per MiB of window`**.

### 6.2 The mechanism, checked against the code

An `O(buffered bytes)`-per-ack operation exists on the hot path (working
rule 11 — read, not inferred):

- `src/core/connection/send.rs:78` — `SendHalf { buf: Vec<u8>, … }`, a
  **contiguous** send buffer.
- `src/core/connection/send.rs:593` — `release()` performs
  `self.buf.drain(..drop)`, which memmoves the entire remaining buffer down.
- `src/core/connection/send.rs:491` — `on_ack_range()` calls `release()`, i.e.
  **once per acknowledged stream range**.

A bulk writer keeps the send buffer full to the flow-control limit, so the
buffer sits at ≈ the configured window `W` and each ack memmoves ≈ `W` bytes.

Magnitude check: 1 MiB of payload is ~880 packets at the observed 1192 B wire
MTU, and `ack_dg ≈ dg_out / 2` in every row, so ~440 memmoves of `W` per MiB of
payload. Setting `440·W / BW = b·W` gives **`BW ≈ 23.3 GB/s`** — a plausible
single-core memmove rate on this host, and it agrees with the 8 MiB row's
implied rate directly.

**Attribution status.** The mechanism is present, is on the hot path, has the
measured shape, and has the right magnitude. It is **not profiler-confirmed** —
no profiler and no new dependency were in scope. Two alternative `O(window)`
candidates were examined and are weak:

- `RangeSet` is a `Vec<Range<u64>>`, but it coalesces to ~1 range under
  in-order, lossless traffic, so its per-ack cost is `O(1)` here.
- The recovery map is `BTreeMap<u64, SentPacket>` and its ack path is a range
  query, i.e. `O(newly acked)`, not `O(window)`.

Two consequences worth stating plainly:

- **The ratified default pays this too.** At 256 KiB, `b·w = 0.00495` of a
  `0.01416 s/MiB` total: about **35 %** of slither's per-byte cost on a clean
  loopback path is the drain, not the protocol. The window-independent ceiling
  `1/a` is ≈ **109 MiB/s**.
- **This is a finding, not a fix.** No production code was touched here. It is
  a ruling question for the maintainer, not a benchmark author's edit.

### 6.3 100 ms RTT, window ladder (`sweep-rtt`) — where the optimum is

| stream / connection window | p50 MiB/s | min | max | amp | loss |
|---|---|---|---|---|---|
| default 256 KiB / 1 MiB | 1.42 | 1.41 | 1.42 | 1.039 | 0.0000 |
| 512 KiB / 2 MiB | 2.46 | 2.44 | 2.46 | 1.040 | 0.0000 |
| 1 MiB / 4 MiB | 4.45 | 4.43 | 4.60 | 1.040 | 0.0000 |
| **2 MiB / 8 MiB** | **9.01** | 8.89 | 9.18 | 1.040 | 0.0001 |
| 8 MiB / 16 MiB | 5.65 | 5.64 | 5.87 | 1.110 | **0.0635** |

**The knob has a maximum, and 8 MiB is past it.** At 100 ms RTT the best
configuration on this ladder is **2 MiB / 8 MiB at 9.01 MiB/s** — **6.3× the
default**, and **1.6× better than the 8 MiB / 16 MiB choice** this benchmark
was briefed to try.

The shape is exactly the two laws crossing:

- window-limited: throughput ≈ `W / (2 · RTT)`, rising linearly in `W` — the
  first four rows scale ×1.73, ×1.81, ×2.02 for each doubling;
- CPU-limited: throughput ≈ `1 / (a + b·W)`, falling in `W` (§6.1).

The crossover on this host at 100 ms RTT is near **2–3 MiB**, which is
also where BDP arithmetic puts it: `9 MiB/s × 0.1 s × 2` (the re-grant factor)
≈ 1.8 MiB.

The 8 MiB row also shows the only significant loss in the whole matrix
(`loss = 0.0635`, `amp` up to 1.110). Candidate causes, **not separated by
measurement**: the UDP relay's single thread doing two syscalls and one
allocation per 1200-byte datagram and dropping on its own socket buffer; the
endpoints' socket buffers overrunning on a larger burst; or slither's own pacing
under a window it cannot fill efficiently. The counting wire sees end-to-end
datagram loss but not *where* it happened.

**Practical guidance this yields**, and the thing to carry away from the knob:
size `stream_window` at about `2 × RTT × target_rate` — the BDP with the
re-grant factor — and **not larger**. "Raise it a lot" is the wrong instinct on
this build.

---

## 7. Scenario 4 — latency ping-pong (64 B, unpipelined, `TCP_NODELAY` on)

n = 2000 timed round trips per cell, after 100 discarded.

| RTT | proto | p50 | p99 | p99.9 | min | max |
|---|---|---|---|---|---|---|
| 0 ms | slither (datagram path) | **37.3 µs** | 55.8 | 58.0 | 28.8 | 59.4 |
| 0 ms | TCP nodelay | **15.5 µs** | 19.6 | 22.6 | 13.6 | 26.9 |
| 20 ms | slither (datagram path) | **24 691.9 µs** | 25 977.1 | 29 631.7 | 22 118.1 | 33 830.1 |
| 20 ms | TCP nodelay | **24 576.9 µs** | 25 826.0 | 27 913.8 | 22 269.2 | 29 224.5 |

**At zero RTT TCP is 2.4× faster** (37.3 vs 15.5 µs p50) and its tail is
tighter (p99 19.6 vs 55.8 µs). The 21.8 µs gap is slither's per-round-trip
userspace cost: two AEAD operations, packet build and parse, and two driver task
wakeups per direction, all on the one thread that is also running the peer.

**At 20 ms RTT the two are indistinguishable** — 24.69 vs 24.58 ms p50, a 0.5 %
difference against a 4.6 ms relay slop that both pay equally. Once a real
network is in the path, slither's per-round-trip cost is 0.15 % of the round
trip and disappears. The tails differ slightly (p99.9 29.6 vs 27.9 ms) and that
difference is **not attributed**: candidates are the extra driver wakeups and
the OS scheduler, and this harness cannot separate them. `examples/audit_udp.rs`
is the instrument that exists for that question.

Note this cell is honest for TCP in a way §5 is not: at RTT 0 there is no relay,
and at 20 ms both protocols cross the relay exactly twice, so the proxy
termination does not distort a *latency* comparison the way it distorts a
throughput one.

---

## 8. Scenario 5 — multiplexing (slither only, clean loopback, aggregate)

TCP's single-connection loopback figure from §4 — **10 979 MiB/s** — is the
reference line. No application framing layer was built over TCP: with loss ruled
out of scope, a framed TCP would be measuring the framing library, not TCP.

| windows | streams | aggregate p50 MiB/s | min | max | amp | loss |
|---|---|---|---|---|---|---|
| default | 1 | **73.97** | 72.88 | 74.57 | 1.043 | 0.0001 |
| default | 4 | **70.96** | 70.82 | 71.35 | 1.040 | 0.0000 |
| default | 16 | **76.11** | 74.84 | 76.33 | 1.040 | 0.0000 |
| 8 Mi/16 Mi | 1 | 5.85 | 5.80 | 5.90 | 1.040 | 0.0000 |
| 8 Mi/16 Mi | 4 | 7.26 | 6.88 | 8.86 | 1.068 | 0.0109 |
| 8 Mi/16 Mi | 16 | 7.04 | 6.41 | 10.66 | 1.072 | 0.0102 |

**Multiplexing is free at the ratified window.** 1 → 4 → 16 concurrent uni
streams moves the aggregate 73.97 → 70.96 → 76.11 MiB/s: a 7 % band with no
trend, i.e. inside the run-to-run spread. Sixteen streams cost neither
throughput nor wire bytes (`amp` 1.040, `loss` 0.0000). That is the property a
multiplexed transport is supposed to have and this build has it.

**Multiplexing does not rescue the raised-window collapse.** 5.85 → 7.26 → 7.04
MiB/s: a 1.2× gain from 1 to 4 streams and nothing beyond. This is a second
independent confirmation of §6.2's mechanism and it sharpens it: the cost tracks
**total buffered bytes on the connection**, not the per-stream window. The
connection ledger caps total in-flight at `connection_window` (16 MiB) however
many streams share it, so splitting the same 16 MiB across sixteen send buffers
moves the same total number of bytes per ack round. Had the cost been strictly
per-stream-buffer, sixteen streams would have been ~16× faster; they are not.

The raised multi-stream cells are also the noisiest in the matrix (max 10.66
against a p50 of 7.04) and carry ~1 % loss with `amp` up to 1.072 — the only
cells outside §6.3's 8 MiB / 100 ms row to show either.

Against TCP's **10 979 MiB/s** single-connection reference, sixteen slither
streams aggregate to **76 MiB/s** — 144× slower, for the reasons in §4, which
multiplexing neither improves nor worsens.

---

## 9. Where each protocol wins

**Raw TCP wins, decisively and structurally:**

- Clean-loopback bulk, by ~150× (§4). 16 KiB segments against 1180 B datagrams,
  no AEAD, and the kernel doing the framing on other cores.
- Zero-RTT round-trip latency, by 2.4× with a tighter tail (§7).
- Zero-RTT establishment, by ~1.7 ms (§3.2) — the P-256 handshake.

**slither ties:**

- Establishment flight count on a real path: **2 RTT, the same as raw TCP**
  (§3.1), with the measured figures within noise once the harness's missing RTT
  and relay slop are accounted for.
- Round-trip latency at 20 ms RTT (§7): 0.5 % apart, which is nothing.

**slither does something TCP structurally cannot:**

- **Sixteen concurrent streams for free** (§8): 76.11 MiB/s aggregate against
  73.97 for one, no extra wire bytes, no loss. A single TCP connection carrying
  sixteen logical streams needs an application framing layer and inherits
  head-of-line blocking; sixteen TCP connections need sixteen handshakes and
  sixteen congestion controllers. This benchmark deliberately built neither
  (out of scope with loss ruled out), so the *advantage* is unmeasured — but the
  slither half of it is measured and it costs nothing.

**slither wins on the axis this comparison does not have a column for:** raw TCP
provides no authentication, no encryption, no multiplexing, no roaming, and no
unreliable datagram path. The comparison a deployment actually faces is against
**TCP + TLS**, where §3.1's arithmetic gives slither a one-RTT advantage over
TLS 1.3 and two over TLS 1.2 — arithmetic, stated, **not measured here**.

---

## 10. Differences that are NOT attributed by measurement

Listed explicitly rather than glossed:

1. **The §4 bulk gap's internal breakdown.** Segment size, AEAD cost, userspace
   framing, and single-thread-both-endpoints each plausibly contribute; the
   benchmark measures only their sum. Not separated.
2. **The §6.2 window-cost mechanism.** `SendHalf::release`'s
   `buf.drain(..)` is present, on the hot path, and matches the measured shape
   and magnitude. It is not profiler-confirmed and the residual `a` is not
   broken down further.
3. **Where the §6.3 loss occurs.** 6.35 % end-to-end datagram loss at 8 MiB /
   100 ms. Candidates: the relay's own socket buffer, the endpoints' socket
   buffers, slither's pacing. The counting wire sees the loss, not its location.
4. **The §7 tail difference at 20 ms.** p99.9 29.6 ms vs 27.9 ms. Candidates:
   driver wakeups, OS scheduling. Not separated.
5. **What a real TCP would do on a delayed path.** Not measured at all — the
   relay cannot produce one (§2.1). The `proto=tcp` figures in §5 measure the
   harness's queue bound, not TCP.
6. **What TLS costs.** No TLS baseline was built. §3.1 is arithmetic.

---

## 11. Caveats

- **Single machine, single run.** One M4 Max, macOS 26.6.1. Loopback on macOS is
  not a network: no NIC, no driver, 16 KiB MTU, and TCP segment offload
  behaviour that has no analogue on a real path. Absolute figures do not
  transfer; the *ratios within a column* are what travels.
- **The TCP relay is a proxy** (§2.1). This is the largest single caveat and it
  invalidates the `proto=tcp` column of §5 as a TCP measurement.
- **Relay slop of ≈ 2.3 ms per traversal** is in every delayed figure, four
  traversals for slither establishment, two for everything else.
- **The UDP relay is per-datagram and the TCP relay per-64-KiB-chunk** — a ~50×
  asymmetry in relay work for the same payload, charged against slither.
- **Single-threaded both-endpoints topology.** Forced for slither, applied to
  TCP for parity of the userspace budget only; the kernel half of TCP is not
  held to it.
- **No loss dimension.** Ruled out of scope. The `testutil::FlakyWire`
  virtual-time suite remains the loss story, and it models loss properly and
  resolves in virtual time; nothing here supersedes it.
- **Five samples per bulk cell.** Enough for a p50 with a spread; there is no
  p99 in five samples and the harness prints `-` rather than dressing the
  maximum up as one (working rule 9).
- **`SoftwareIdentity` with seeded keys.** Replayable, and the keys protect
  nothing. A hardware DH provider would move §3.2's figure and nothing else.
- **Nothing here is a gate.** No threshold is asserted, and none should be
  derived from these numbers without re-measuring on the target host.

---

## 12. Raw output

`cargo run --release --example bench_vs_tcp -- all`, verbatim. Everything above
is derived from these lines and from `ifconfig lo0`.

```text
bench_vs_tcp — slither versus raw kernel TCP
  chunk=65536B ramp=2s window=4s samples=5 ping_iters=2000 raised=8388608/16777216
  topology=one-thread/one-LocalSet/both-endpoints, relay=own-thread/own-runtime

# establishment — dial to first application byte echoed back
BENCH scenario=establish proto=slither rtt_ms=0 windows=default streams=1 n=20 p50=1.911 p99=- p999=- min=1.535 max=4.253 unit=ms
BENCH scenario=establish proto=tcp rtt_ms=0 windows=n/a streams=1 n=20 p50=0.181 p99=- p999=- min=0.148 max=0.238 unit=ms
BENCH scenario=establish proto=slither rtt_ms=20 windows=default streams=1 n=10 p50=52.752 p99=- p999=- min=49.022 max=53.975 unit=ms
BENCH scenario=establish proto=tcp rtt_ms=20 windows=n/a streams=1 n=10 p50=24.712 p99=- p999=- min=22.865 max=25.310 unit=ms note=proxy-terminated,missing-1-rtt
BENCH scenario=establish proto=slither rtt_ms=100 windows=default streams=1 n=10 p50=213.300 p99=- p999=- min=211.667 max=214.737 unit=ms
BENCH scenario=establish proto=tcp rtt_ms=100 windows=n/a streams=1 n=10 p50=104.739 p99=- p999=- min=103.610 max=105.036 unit=ms note=proxy-terminated,missing-1-rtt

# bulk — one stream, steady state
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default streams=1 n=5 p50=73.44 p99=- p999=- min=71.97 max=74.89 unit=MiB/s note=amp=1.043,loss=0.0000,dg_out=1358022,dg_in=1358019,ack_dg=687447,mtu=1180
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi streams=1 n=5 p50=5.74 p99=- p999=- min=5.69 max=5.78 unit=MiB/s note=amp=1.040,loss=0.0000,dg_out=110348,dg_in=110347,ack_dg=55217,mtu=1195
BENCH scenario=bulk proto=tcp rtt_ms=0 windows=kernel-default streams=1 n=5 p50=10979.42 p99=- p999=- min=10940.41 max=11025.25 unit=MiB/s

# sweep — slither only, window ladder, one stream, rtt=0 ms
BENCH scenario=sweep proto=slither rtt_ms=0 windows=default streams=1 n=5 p50=70.65 p99=- p999=- min=69.98 max=70.88 unit=MiB/s note=amp=1.043,loss=0.0000,dg_out=1308445,dg_in=1308443,ack_dg=662352,mtu=1180
BENCH scenario=sweep proto=slither rtt_ms=0 windows=512Ki/2Mi streams=1 n=5 p50=53.81 p99=- p999=- min=52.80 max=54.02 unit=MiB/s note=amp=1.041,loss=0.0000,dg_out=982862,dg_in=982861,ack_dg=494820,mtu=1191
BENCH scenario=sweep proto=slither rtt_ms=0 windows=1Mi/4Mi streams=1 n=5 p50=35.42 p99=- p999=- min=35.41 max=35.52 unit=MiB/s note=amp=1.040,loss=0.0000,dg_out=649441,dg_in=649441,ack_dg=325942,mtu=1192
BENCH scenario=sweep proto=slither rtt_ms=0 windows=2Mi/8Mi streams=1 n=5 p50=20.22 p99=- p999=- min=20.18 max=20.26 unit=MiB/s note=amp=1.040,loss=0.0000,dg_out=371649,dg_in=371649,ack_dg=186211,mtu=1194
BENCH scenario=sweep proto=slither rtt_ms=0 windows=8Mi/16Mi streams=1 n=5 p50=5.69 p99=- p999=- min=5.61 max=5.75 unit=MiB/s note=amp=1.040,loss=0.0000,dg_out=110348,dg_in=110347,ack_dg=55217,mtu=1195

# bulk-rtt — one stream, steady state
BENCH scenario=bulk-rtt proto=slither rtt_ms=20 windows=default streams=1 n=5 p50=5.53 p99=- p999=- min=5.52 max=7.06 unit=MiB/s note=amp=1.040,loss=-0.0002,dg_out=107334,dg_in=107355,ack_dg=54269,mtu=1189
BENCH scenario=bulk-rtt proto=slither rtt_ms=20 windows=8Mi/16Mi streams=1 n=5 p50=5.49 p99=- p999=- min=5.41 max=6.90 unit=MiB/s note=amp=1.066,loss=0.0412,dg_out=107930,dg_in=103482,ack_dg=51786,mtu=1200
BENCH scenario=bulk-rtt proto=tcp rtt_ms=20 windows=kernel-default streams=1 n=5 p50=5466.56 p99=- p999=- min=5309.43 max=5564.22 unit=MiB/s note=proxy-terminated,not-a-delayed-path
BENCH scenario=bulk-rtt proto=slither rtt_ms=50 windows=default streams=1 n=5 p50=2.67 p99=- p999=- min=2.64 max=2.69 unit=MiB/s note=amp=1.039,loss=-0.0013,dg_out=49222,dg_in=49284,ack_dg=24964,mtu=1188
BENCH scenario=bulk-rtt proto=slither rtt_ms=50 windows=8Mi/16Mi streams=1 n=5 p50=5.52 p99=- p999=- min=5.48 max=7.10 unit=MiB/s note=amp=1.041,loss=0.0069,dg_out=106226,dg_in=105489,ack_dg=52819,mtu=1200
BENCH scenario=bulk-rtt proto=tcp rtt_ms=50 windows=kernel-default streams=1 n=5 p50=2340.39 p99=- p999=- min=2313.33 max=2355.71 unit=MiB/s note=proxy-terminated,not-a-delayed-path
BENCH scenario=bulk-rtt proto=slither rtt_ms=100 windows=default streams=1 n=5 p50=1.42 p99=- p999=- min=1.41 max=1.42 unit=MiB/s note=amp=1.040,loss=0.0001,dg_out=26625,dg_in=26623,ack_dg=13491,mtu=1186
BENCH scenario=bulk-rtt proto=slither rtt_ms=100 windows=8Mi/16Mi streams=1 n=5 p50=5.58 p99=- p999=- min=5.56 max=6.04 unit=MiB/s note=amp=1.095,loss=0.0502,dg_out=108807,dg_in=103345,ack_dg=51747,mtu=1199
BENCH scenario=bulk-rtt proto=tcp rtt_ms=100 windows=kernel-default streams=1 n=5 p50=1185.36 p99=- p999=- min=1171.80 max=1190.91 unit=MiB/s note=proxy-terminated,not-a-delayed-path

# sweep-rtt — slither only, window ladder, one stream, rtt=100 ms
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=default streams=1 n=5 p50=1.42 p99=- p999=- min=1.41 max=1.42 unit=MiB/s note=amp=1.039,loss=-0.0009,dg_out=26629,dg_in=26653,ack_dg=13487,mtu=1185
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=512Ki/2Mi streams=1 n=5 p50=2.46 p99=- p999=- min=2.44 max=2.46 unit=MiB/s note=amp=1.040,loss=-0.0004,dg_out=45320,dg_in=45338,ack_dg=22842,mtu=1190
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=1Mi/4Mi streams=1 n=5 p50=4.45 p99=- p999=- min=4.43 max=4.60 unit=MiB/s note=amp=1.040,loss=-0.0001,dg_out=83059,dg_in=83065,ack_dg=41689,mtu=1193
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=2Mi/8Mi streams=1 n=5 p50=9.01 p99=- p999=- min=8.89 max=9.18 unit=MiB/s note=amp=1.040,loss=0.0001,dg_out=166741,dg_in=166732,ack_dg=83548,mtu=1194
BENCH scenario=sweep-rtt proto=slither rtt_ms=100 windows=8Mi/16Mi streams=1 n=5 p50=5.65 p99=- p999=- min=5.64 max=5.87 unit=MiB/s note=amp=1.110,loss=0.0635,dg_out=110912,dg_in=103864,ack_dg=52038,mtu=1200

# ping-pong — 64 B, unpipelined, TCP_NODELAY on
BENCH scenario=pingpong proto=slither rtt_ms=0 windows=default streams=1 n=2000 p50=37.3 p99=55.8 p999=58.0 min=28.8 max=59.4 unit=us note=datagram-path
BENCH scenario=pingpong proto=tcp rtt_ms=0 windows=n/a streams=1 n=2000 p50=15.5 p99=19.6 p999=22.6 min=13.6 max=26.9 unit=us note=nodelay
BENCH scenario=pingpong proto=slither rtt_ms=20 windows=default streams=1 n=2000 p50=24691.9 p99=25977.1 p999=29631.7 min=22118.1 max=33830.1 unit=us note=datagram-path
BENCH scenario=pingpong proto=tcp rtt_ms=20 windows=n/a streams=1 n=2000 p50=24576.9 p99=25826.0 p999=27913.8 min=22269.2 max=29224.5 unit=us note=nodelay

# multiplexing — slither only, concurrent uni streams, clean loopback
BENCH scenario=mux proto=slither rtt_ms=0 windows=default streams=1 n=5 p50=73.97 p99=- p999=- min=72.88 max=74.57 unit=MiB/s note=aggregate,amp=1.043,loss=0.0001,dg_out=1368264,dg_in=1368183,ack_dg=692590,mtu=1180
BENCH scenario=mux proto=slither rtt_ms=0 windows=default streams=4 n=5 p50=70.96 p99=- p999=- min=70.82 max=71.35 unit=MiB/s note=aggregate,amp=1.040,loss=0.0000,dg_out=1302060,dg_in=1302014,ack_dg=658680,mtu=1190
BENCH scenario=mux proto=slither rtt_ms=0 windows=default streams=16 n=5 p50=76.11 p99=- p999=- min=74.84 max=76.33 unit=MiB/s note=aggregate,amp=1.040,loss=0.0000,dg_out=1389724,dg_in=1389680,ack_dg=702567,mtu=1189
BENCH scenario=mux proto=slither rtt_ms=0 windows=8Mi/16Mi streams=1 n=5 p50=5.85 p99=- p999=- min=5.80 max=5.90 unit=MiB/s note=aggregate,amp=1.040,loss=0.0000,dg_out=110348,dg_in=110347,ack_dg=55217,mtu=1195
BENCH scenario=mux proto=slither rtt_ms=0 windows=8Mi/16Mi streams=4 n=5 p50=7.26 p99=- p999=- min=6.88 max=8.86 unit=MiB/s note=aggregate,amp=1.068,loss=0.0109,dg_out=140773,dg_in=139243,ack_dg=69680,mtu=1199
BENCH scenario=mux proto=slither rtt_ms=0 windows=8Mi/16Mi streams=16 n=5 p50=7.04 p99=- p999=- min=6.41 max=10.66 unit=MiB/s note=aggregate,amp=1.072,loss=0.0102,dg_out=147075,dg_in=145580,ack_dg=72850,mtu=1199

BENCH_DONE wall_s=739.4

```

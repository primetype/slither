# Round 42, report G — the datapath attribution (the syscall bucket)

*Opus agent (killed four times by server-side 529s mid-run) + Sonnet
continuation inheriting its worktree per rule 2 — the report file and
built instruments carried the state across both deaths. Instruments
preserved on branch `datapath/attribution` (MEASUREMENT — DO NOT LAND).
Verbatim report below.*

---

# DATAPATH-REPORT — round 42 carried item: the syscall bucket

Base commit (rule 14, verified as first act):
```
$ git rev-parse HEAD
fc4492d17279d218e9c787a8cb53ac8642636d36
$ git log --oneline -1
fc4492d Ruling 270: the reassembly ceiling derives from the credit it polices
```
Tree clean at start. All instruments in this worktree are THROWAWAY
("MEASUREMENT — DO NOT LAND").

Host: darwin 25.6.0 (macOS), Apple silicon. All numbers measured here.

## Deviations found (read this first)

1. **`spin-lockstep` does not reproduce the "~2.3 µs" historical floor.**
   Measured 8.2–9.2 µs/unit (0.29–0.43 µs/call, but `calls_per_unit` is
   21–28 because ~91–93% of its calls are `WouldBlock` spins waiting for
   the peer's send to land). The mode whose number actually lands near
   2.3 µs is `raw-send-drn conn=yes` at **2.366 µs/call** — a connected,
   blocking `sendto` with a draining reader, zero spins. Reported both;
   not resolved which one the "known ~2.3 µs" in the brief referred to.
2. **The brief's "~5.5 µs/datagram gap" does not match a direct
   recomputation from its own anchors.** `13 µs (slither) − 4.1 µs
   (quinn) = 8.9 µs`, not 5.5 µs. All percentage-of-gap figures below are
   given against **both** 8.9 µs (directly recomputed) and 5.5 µs
   (as stated), so the reader can pick the intended basis. Not resolved.
3. **The profile's `__sendto`(~40%)/`__recvfrom`(~9%) asymmetry does not
   reproduce at the raw syscall floor — and points the other way.**
   Isolated per-call cost on this host: `raw-send` 2.419 µs (unconn) /
   1.788 µs (conn); `raw-recv` **3.293 µs (unconn) / 2.357 µs (conn)** —
   recvfrom is 32–36% *pricier* per call in isolation, not cheaper. The
   census also shows send and recv call counts are roughly balanced
   (sends ≈ receives, as they must be on a lossless link), so the
   profile's ~4.4× time asymmetry is not explained by call-count
   imbalance either. See "The sendto anomaly" below — not resolved.
4. **The DRIVER-PATTERN retry-storm hypothesis is essentially refuted by
   the census.** `send_attempts / send_ok ≈ 1.0000` on every sender in
   every run (≈1.75–2M calls, 0–1 total `WouldBlock` events). See
   "Census" below.

## Established context (given, not re-measured)

- slither bulk @ default windows: ~88 MiB/s ~ 77k dg/s through the whole
  pipeline, both endpoints on one core => ~13 us/datagram.
- `sample` decomposition: `__sendto` ~40 %, `__recvfrom` ~9 %,
  kevent+waker+readiness ~0.1 %.
- quinn: 245k dg/s = 4.1 us/datagram, max_gso_segments=1.
- The old 2.3 us "floor" was a spin loop on nonblocking std sockets.

## Census

Instrument: `examples/bench_vs_tcp.rs`'s `CountingWire` (already built by
the prior agent, uncommitted — see diff below), which replaces the
production `self.inner.send_to(...).await` / `recv_from(...).await` with
explicit `try_send_to`/`try_recv_from` loops so every kernel entry is
counted, including the ones that deliver nothing. Counters are cumulative
over the **whole connection** (handshake + 2 s ramp + 20 s timed section
+ FIN tail), printed once per window-config at the end of `slither_bulk`.

Command:
```
$ cargo build --release --example bench_vs_tcp --example floor_probe
    Finished `release` profile [optimized] target(s) in 0.02s   # already built
$ ./target/release/examples/bench_vs_tcp bulk
```

Raw output (full run in `BENCH_DONE wall_s=66.1`):
```
CENSUS side=A-sender send_attempts=1753885 send_ok=1753884 send_wb=1 send_parks=1 send_att_per_ok=1.0000 send_ok_per_park=1753884.0 send_errno=none recv_attempts=912074 recv_ok=887833 recv_wb=24241 recv_parks=24241 recv_att_per_ok=1.0273 recv_ok_per_park=36.6 recv_errno=none out_hist=0:10055|128:2784|256:8345|384:3276|512:6057|640:1|768:2781|896:13102|1152:1707483 in_hist=0:887833
CENSUS side=B-receiver send_attempts=887851 send_ok=887851 send_wb=0 send_parks=0 send_att_per_ok=1.0000 send_ok_per_park=887851.0 send_errno=none recv_attempts=1778134 recv_ok=1753884 recv_wb=24250 recv_parks=24250 recv_att_per_ok=1.0138 recv_ok_per_park=72.3 recv_errno=none out_hist=0:887851 in_hist=0:10055|128:2784|256:8345|384:3276|512:6057|640:1|768:2781|896:13102|1152:1707483
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default streams=1 n=5 p50=86.23 p99=- p999=- min=85.60 max=86.66 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1596828,dg_in=1596828,ack_dg=808337,mtu=1181
CENSUS side=A-sender send_attempts=2002284 send_ok=2002283 send_wb=1 send_parks=1 send_att_per_ok=1.0000 send_ok_per_park=2002283.0 send_errno=none recv_attempts=1006159 recv_ok=1000936 recv_wb=5223 recv_parks=5223 recv_att_per_ok=1.0052 recv_ok_per_park=191.6 recv_errno=none out_hist=0:548|128:1|640:1|1152:2001733 in_hist=0:1000936
CENSUS side=B-receiver send_attempts=1000976 send_ok=1000976 send_wb=0 send_parks=0 send_att_per_ok=1.0000 send_ok_per_park=1000976.0 send_errno=none recv_attempts=2009339 recv_ok=2000851 recv_wb=8488 recv_parks=8488 recv_att_per_ok=1.0042 recv_ok_per_park=235.7 recv_errno=none out_hist=0:1000976 in_hist=0:548|128:1|640:1|1152:2000301
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi streams=1 n=5 p50=99.48 p99=- p999=- min=97.91 max=99.93 unit=MiB/s note=amp=1.044,loss=0.0000,dg_out=1813833,dg_in=1813812,ack_dg=907401,mtu=1200
BENCH scenario=bulk proto=tcp rtt_ms=0 windows=kernel-default streams=1 n=5 p50=11433.21 p99=- p999=- min=10971.10 max=11475.34 unit=MiB/s
```

**Send attempts per delivered data datagram (retry multiplier):**

| config | side | send_att/ok |
|---|---|---|
| default | A (data) | 1.0000006 |
| default | B (ack)  | 1.0000000 |
| raised  | A (data) | 1.0000005 |
| raised  | B (ack)  | 1.0000000 |

Essentially exactly 1.0 everywhere. Across ~1.75–2M send calls per side
per run, there was a total of **0 or 1** `WouldBlock`. **No retry storm
on the send side, on either side of the connection, at either window
size.** `send_errno=none` in every case — no `ENOBUFS`, no other errno.

**Recv attempts per delivered datagram:**

| config | side | recv_att/ok | recv_wb | recv_ok/park (dg per wakeup) |
|---|---|---|---|---|
| default | A (recv acks) | 1.0273 | 24241 | 36.6 |
| default | B (recv data) | 1.0138 | 24250 | 72.3 |
| raised  | A (recv acks) | 1.0052 | 5223  | 191.6 |
| raised  | B (recv data) | 1.0042 | 8488  | 235.7 |

Recv side has a small (1.4–2.7%) attempt overhead from cycling on
`WouldBlock`/`readable().await`, batching 37–236 delivered datagrams per
readable-wakeup depending on window size (larger windows → fewer, bigger
bursts). `recv_errno=none` in every case.

**Datagram-size histogram** (`out_hist`/`in_hist`, bucket = `size/128`):
A's default-config `out_hist` shows 97.4% of A's sends land in the
1152–1279 B bucket (full-MTU data, `mtu=1181` per the BENCH note) with
the remaining 2.6% spread across smaller buckets — handshake and
ramp/tail traffic, since census spans the whole connection, not just the
steady-state 20 s bracket. A's `in_hist` and B's `out_hist` are 100% in
the 0–127 B bucket (37 B ACK-only), consistent with a pure one-way bulk
test where B only ever sends ACKs.

## Floors

Instrument: `examples/floor_probe.rs` (already built by the prior agent,
uncommitted). One addition made in this session: a **`raw-recv` mode**,
mirroring `raw-send`/`raw-send-drn` on the receive side (a feeder thread
floods datagrams via blocking `send`/`send_to`, the main thread does
nothing but time `recv_from` calls) — added because the census and the
given profile disagreed sharply on sendto-vs-recvfrom cost and no
existing mode isolated recvfrom alone. See "The sendto anomaly" below.

Command and full raw output (`mode=all size=1200`, DUR=3s/mode/conn-variant):
```
$ ./target/release/examples/floor_probe all 1200
FLOOR mode=raw-send conn=no size=1200 units=1240064 secs=3.000 per_s=413340 us_per_unit=2.419 calls=1240064 calls_per_unit=1.000 us_per_call=2.419 wouldblock=0
  (raw-send ok=1240064 of 1240064)
FLOOR mode=raw-send-drn conn=no size=1200 units=915456 secs=3.000 per_s=305144 us_per_unit=3.277 calls=915456 calls_per_unit=1.000 us_per_call=3.277 wouldblock=0
  (raw-send ok=915456 of 915456)
FLOOR mode=raw-lockstep conn=no size=1200 units=416704 secs=3.000 per_s=138881 us_per_unit=7.200 calls=833408 calls_per_unit=2.000 us_per_call=3.600 wouldblock=0
FLOOR mode=spin-lockstep conn=no size=1200 units=325952 secs=3.000 per_s=108646 us_per_unit=9.204 calls=6929615 calls_per_unit=21.260 us_per_call=0.433 wouldblock=6277711
FLOOR mode=tok-lockstep conn=no size=1200 units=360320 secs=3.000 per_s=120101 us_per_unit=8.326 calls=720640 calls_per_unit=2.000 us_per_call=4.163 wouldblock=0
FLOOR mode=tok-pingpong conn=no size=1200 units=169024 secs=3.000 per_s=56338 us_per_unit=17.750 calls=676096 calls_per_unit=4.000 us_per_call=4.437 wouldblock=0
FLOOR mode=tok-oneway conn=no size=1200 units=961792 secs=3.000 per_s=320576 us_per_unit=3.119 calls=1923584 calls_per_unit=2.000 us_per_call=1.560 wouldblock=0
  (tok-oneway conn=no sent=961792 delivered=961792 loss=0.0000 sendto_per_s=320576)
FLOOR mode=raw-send conn=yes size=1200 units=1677824 secs=3.000 per_s=559255 us_per_unit=1.788 calls=1677824 calls_per_unit=1.000 us_per_call=1.788 wouldblock=0
  (raw-send ok=1677824 of 1677824)
FLOOR mode=raw-send-drn conn=yes size=1200 units=1267968 secs=3.000 per_s=422605 us_per_unit=2.366 calls=1267968 calls_per_unit=1.000 us_per_call=2.366 wouldblock=0
  (raw-send ok=1267968 of 1267968)
FLOOR mode=raw-lockstep conn=yes size=1200 units=485504 secs=3.000 per_s=161834 us_per_unit=6.179 calls=971008 calls_per_unit=2.000 us_per_call=3.090 wouldblock=0
FLOOR mode=spin-lockstep conn=yes size=1200 units=365824 secs=3.000 per_s=121934 us_per_unit=8.201 calls=10258569 calls_per_unit=28.042 us_per_call=0.292 wouldblock=9526921
FLOOR mode=tok-lockstep conn=yes size=1200 units=435520 secs=3.000 per_s=145168 us_per_unit=6.889 calls=871040 calls_per_unit=2.000 us_per_call=3.444 wouldblock=0
FLOOR mode=tok-pingpong conn=yes size=1200 units=209920 secs=3.001 per_s=69960 us_per_unit=14.294 calls=839680 calls_per_unit=4.000 us_per_call=3.573 wouldblock=0
FLOOR mode=tok-oneway conn=yes size=1200 units=1376128 secs=3.000 per_s=458696 us_per_unit=2.180 calls=2752320 calls_per_unit=2.000 us_per_call=1.090 wouldblock=0
  (tok-oneway conn=yes sent=1376192 delivered=1376128 loss=0.0000 sendto_per_s=458718)

$ ./target/release/examples/floor_probe raw-recv 1200      # added this session
FLOOR mode=raw-recv conn=no size=1200 units=911104 secs=3.001 per_s=303637 us_per_unit=3.293 calls=911104 calls_per_unit=1.000 us_per_call=3.293 wouldblock=0
  (raw-recv ok=911104 of 911104)
FLOOR mode=raw-recv conn=yes size=1200 units=1272576 secs=3.000 per_s=424191 us_per_unit=2.357 calls=1272576 calls_per_unit=1.000 us_per_call=2.357 wouldblock=0
  (raw-recv ok=1272576 of 1272576)
```

**Headline: tokio-async floor (the mode structurally closest to slither's
`Wire`, one current-thread runtime, `send_to`/`recv_from` async fns):**

| mode | conn | µs/datagram | calls/unit |
|---|---|---|---|
| `tok-oneway` (saturating one-way, sender + counting receiver tasks) | no | **3.119** | 2 |
| `tok-oneway` | yes | **2.180** | 2 |
| `tok-pingpong` (round trip, 1 task) | no | 17.750 | 4 |
| `tok-pingpong` | yes | 14.294 | 4 |
| `tok-lockstep` (send-then-recv, never parks) | no | 8.326 | 2 |
| `tok-lockstep` | yes | 6.889 | 2 |

`tok-oneway` is the honest floor for slither's actual shape: 3.119 µs/dg
unconnected (matching slither's real `send_to(buf, addr)` usage — see
"Connected vs unconnected" below for why unconnected is the relevant
column) is the bare cost of one async `sendto` + one async `recvfrom`
with real (loopback) network delivery, no crypto, no protocol, on this
host.

**Std-spin mode, for comparability with the historical "~2.3 µs" floor —
see Deviation 1.** `spin-lockstep` (nonblocking + busy-spin on
`WouldBlock`, described in its own doc comment as "the old '2.3 us'
floor") measured **9.204 µs/unit unconnected, 8.201 µs/unit connected**
— not 2.3 µs. Its `calls_per_unit` is 21–28 because 91–93% of its calls
are `WouldBlock` spins (the recv side busy-waits for the peer's packet
to actually land, which on loopback still takes real wall time). Its
*per-call* cost (0.433/0.292 µs) is likewise nowhere near 2.3 µs. The
number that **does** land close to 2.3 µs is `raw-send-drn conn=yes`
(blocking `send`, connected, a real reader draining): **2.366 µs/call**,
0 `WouldBlock`. Reported, not resolved, which mode the "known ~2.3 µs"
figure in the brief was originally measuring.

## Connected vs unconnected

Confirmed first: **slither's production `Wire` trait (`src/shell/wire.rs`)
uses unconnected `send_to(buf, addr)` / `recv_from(buf)`** — never
`connect()`. So the `conn=no` column above is what slither actually pays
today; `conn=yes` is the hypothetical after a `connect()`-once-per-peer
change.

| mode | unconn (µs) | conn (µs) | delta (µs) | reduction |
|---|---|---|---|---|
| `tok-oneway` (2 calls/dg) | 3.119 | 2.180 | 0.939 | 30.1% |
| `raw-send` (1 call) | 2.419 | 1.788 | 0.631 | 26.1% |
| `raw-recv` (1 call) | 3.293 | 2.357 | 0.936 | 28.4% |
| `tok-lockstep` (2 calls) | 8.326 | 6.889 | 1.437 | 17.3% |
| `tok-pingpong` (4 calls) | 17.750 | 14.294 | 3.456 | 19.5% |
| `spin-lockstep` | 9.204 | 8.201 | 1.003 | 10.9% |

The per-call delta is consistent across every mode and both syscall
directions: **connecting the UDP socket to its peer saves roughly
26–30% of the raw per-call cost on this host** — both for `sendto`
(26.1%, isolated) and for `recvfrom` (28.4%, isolated), and 30.1% for
`tok-oneway`'s combined send+recv floor, which is the number carried
into the remedy-class synthesis below. This is a one-line, shell-internal
change (`UdpSocket::connect()` once per peer, then `send`/`recv` instead
of `send_to`/`recv_from`) — no protocol or wire change.

## ACK ratio

**slither**, from `bench_vs_tcp bulk`'s `wire_note` (bracketed to exactly
the 5×4 s steady-state timed section, excluding ramp/handshake/tail —
sharper than the whole-connection census above):

| windows | dg_out (data) | ack_dg | ack fraction | data-per-ack |
|---|---|---|---|---|
| default | 1,596,828 | 808,337 | **33.61%** | 1.975 |
| 8Mi/16Mi | 1,813,833 | 907,401 | **33.35%** | 1.999 |

Slither sends almost exactly **1 ACK for every 2 data datagrams**,
independent of window size.

**quinn**, same shape of test (single loopback connection, one uni
stream, bulk write, `st/default/mtu1200` — single-threaded, both
endpoints on one thread, matching slither's discipline). `quinnbench`
(in the scratchpad, not the slither worktree) still built, so this was
measured rather than skipped. `quinn-proto::ConnectionStats` only
printed `udp_tx` before this session; I added `udp_rx.datagrams` (the
client's own inbound ACK-only datagrams in a uni-stream test) and
`frame_tx.acks`/`frame_rx.acks` to `quinnbench/src/main.rs`'s existing
print line — a 15-line addition, rebuilt clean:

```
$ cargo build --release   # quinnbench, 4.39s, clean
$ ./target/release/quinnbench st default mtu1200
platform: max_gso_segments=1 gro_segments=1 may_fragment=false
[st/default/mtu1200] quinn=0.11.11 quinn-proto=0.11.17 quinn-udp=0.5.15 rustls=0.23.43 (ring)
[st/default/mtu1200] runtime: current_thread (both endpoints on ONE thread)
[st/default/mtu1200] window 1: 291.7 MiB/s  (2447 Mbit/s)  cpu 1.00 cores  (291.7 MiB/s per core)
[st/default/mtu1200] window 2: 289.2 MiB/s  (2426 Mbit/s)  cpu 1.00 cores  (289.2 MiB/s per core)
[st/default/mtu1200] window 3: 287.9 MiB/s  (2415 Mbit/s)  cpu 1.00 cores  (287.9 MiB/s per core)
[st/default/mtu1200] path: current_mtu=1200 rtt=1.141039ms cwnd=656525 | over the measured 12s: udp_tx.datagrams=3133272 (261106 datagram/s, 1200 B/datagram) frames_tx.stream=3133295 lost_packets=76 congestion_events=34 | MEASUREMENT round42: udp_rx.datagrams=53444 (43 B/datagram) ack_frac=0.0168 frame_tx.acks=5894 frame_rx.acks=51670
[st/default/mtu1200] SUMMARY: mean 289.6 MiB/s  (min 287.9, max 291.7, spread 1.3%)
```

quinn: `udp_tx.datagrams=3,133,272`, `udp_rx.datagrams=53,444` → **ack
fraction 1.68%**, **data-per-ack = 58.6**. `platform: max_gso_segments=1
gro_segments=1` confirms quinn gets **no GSO/GRO on this macOS host
either** — same platform ceiling as slither (relevant to the remedy-class
verdict below).

**This is the largest single asymmetry found in this whole
investigation: slither sends ~1 ACK per 2 data datagrams; quinn sends ~1
ACK per 58.6.** Roughly a **30× difference in ACK cadence** for the same
kind of workload (single uni-directional bulk stream, no other traffic).

## The sendto anomaly

The given profile context says `__sendto` is ~40% of one core and
`__recvfrom` ~9% — roughly a **4.4× cost asymmetry** in `sample`'s
on-CPU time attribution. The census shows call counts are **not** that
lopsided: combined across both endpoints, default-window send calls
(1,753,885 + 887,851 ≈ 2.64M) and recv calls (912,074 + 1,778,134 ≈
2.69M) are within 2% of each other, as they must be on a lossless link
(every send has a matching recv). So the profile's 4.4× time asymmetry
is not a call-count asymmetry.

The natural next hypothesis is that `sendto` is intrinsically pricier
per call than `recvfrom` on this host/kernel. **The raw floor says the
opposite:**

| syscall | unconn (µs/call) | conn (µs/call) |
|---|---|---|
| `sendto`/`send` (`raw-send`) | 2.419 | 1.788 |
| `recvfrom`/`recv` (`raw-recv`, added this session) | **3.293** | **2.357** |

`recvfrom` costs **32–36% more** per call than `sendto` in isolation,
not less. This directly contradicts the direction of the profile's
asymmetry. Two things this does *not* do: it does not explain the
profile's 4.4×, and it does not resolve the conflict — it sharpens it,
because now there are two independent measurements (call-count parity,
and recvfrom being the pricier syscall in isolation) both pointing away
from "sendto is just an expensive syscall here," while the given profile
says sendto dominates on-CPU time by 4.4×.

Candidate explanations, none checked further in this session (out of
scope — this needs a fresh `sample` capture correlated against a fresh
census run, ideally the same process invocation, to settle):
- The profile may be from a different workload/config (mux streams,
  raised windows, contention) than the isolated floor or this session's
  bulk run.
- `sample`'s on-CPU attribution reflects **time spent inside the
  syscall's stack frame**, not call count — if slither's real `sendto`
  call site does more work before/around the trap (buffer prep, route
  cache effects under concurrent load from both directions on one core)
  than the isolated single-purpose floor test, the aggregate cost could
  diverge from the isolated per-call number even though the isolated
  number itself is accurate for what it measures.
- Unconnected `sendto` forces a per-call route/PCB lookup that a
  connected `send` skips (this project's own connected-vs-unconnected
  measurement shows a real, reproducible effect in this direction — see
  above) — but the isolated `raw-send`/`raw-recv` floor already used
  matching conn/unconn settings for both syscalls, so this alone doesn't
  explain a divergence between the isolated comparison and the profile.

Reported per rule 3/4: two real, load-bearing measurements are in
tension with the given profile figure. Not resolved here.

## Remedy-class verdict

**Framing note before the numbers:** the brief's stated gap is "~5.5
µs/datagram"; direct recomputation from its own anchors gives 8.9 µs
(13 − 4.1). Every share below is given against **both**. All of this
section is a **model built from measured per-call/per-datagram floor
costs, not a direct A/B measurement of any fix actually implemented** —
treat the percentages as order-of-magnitude, evidence-pointing figures,
not a budget that sums to 100% of anything. "Where the numbers allow"
per the brief: two of the four classes allow a fairly direct estimate,
one is refuted outright, and the fourth is unmeasured/mixed with
unattributed protocol cost — said plainly below rather than forced.

**(a) DRIVER-PATTERN (retry storm / spurious calls) — refuted as a
throughput cause; a *different* driver-pattern lever is real and cheap.**
The retry-storm hypothesis is dead: `send_attempts/send_ok ≈ 1.0000` on
every sender, every config (Census, above) — 0–1 total `WouldBlock` in
~1.75–2M calls. Recv-side overhead is 1.4–2.7%, nowhere near enough to
matter. **Estimated share of the gap: ~0%.**
However, the connected-vs-unconnected finding **is** a genuine, cheap,
shell-internal driver-pattern fix (not a retry fix — an addressing-mode
fix): switching slither's `Wire` from `send_to`/`recv_from` to
`connect()` once per peer + `send`/`recv` saves ~30.1% of the per-call
floor cost (measured directly, `tok-oneway`: 3.119→2.180 µs). Scaled by
the real-datagram-per-data-datagram ratio (1.506, from the ACK ratio
section — every data datagram carries ~0.506 datagrams' worth of ACK
traffic riding the same floor cost): **≈1.41 µs/data-datagram, ≈10.9% of
the 13 µs baseline, ≈15.9% of the 8.9 µs gap, ≈25.7% of the stated 5.5
µs gap.**

**(b) PLATFORM BATCHING (sendmsg_x/sendmmsg territory) — real, but does
not separate slither from quinn; separates *both* from raw TCP.**
`quinnbench` prints `max_gso_segments=1 gro_segments=1` — **quinn gets
no GSO/GRO on this macOS host either.** Both protocols pay the same
one-datagram-per-syscall floor (`tok-oneway conn=no` 3.119 µs is a fair
proxy for that floor, present in whatever either program is built on
top of tokio/std UDP on macOS). That floor is ≈4.70 µs embedded in every
current 13 µs data-datagram (≈36.1% of it) — a real cost, but one quinn
pays too, which is exactly why quinn's own 4.1 µs total is barely above
its own floor (≈4.1 − 2.18 = 1.92 µs of non-floor cost, if quinn uses
`connect()`, which is plausible for a client but not verified here).
Raw kernel TCP, by contrast, hit **11,433 MiB/s** on this loopback vs.
slither's 86–99 MiB/s and quinn's ~290 MiB/s — two orders of magnitude
higher, because the kernel is not bound by one-datagram-per-syscall UDP
semantics at all. **Verdict: PLATFORM BATCHING explains why *neither*
UDP protocol reaches TCP's ceiling; it does not explain the
slither-vs-quinn gap, since both are equally floor-limited.** Attributing
a "share of the gap" to this class would be double-counting — the floor
cost is common to both sides of the comparison.

**(c) ACK-VOLUME (reverse path chattier than quinn's) — the single
largest, most surprising finding in this report.** Slither: ~1 ACK per
**2** data datagrams (33.6% of all datagrams are ACK-only), flat across
window sizes. quinn, same workload shape: ~1 ACK per **58.6** data
datagrams (1.68%). **A ~30× difference in ACK cadence.** Floor-only
(syscall-cost-only, excluding any userspace/frame-construction/crypto
savings an actual cadence change would also capture — so this is a
**conservative lower bound**) estimate of bringing slither to quinn's
cadence: **≈1.53 µs/data-datagram, ≈11.7% of the 13 µs baseline, ≈17.1%
of the 8.9 µs gap, ≈27.7% of the stated 5.5 µs gap.** This is ratified
territory (ACK policy) per the brief, not a code bug — but it is the
best-evidenced, largest single lever found.

**(d) IRREDUCIBLE (the tokio floor accounts for most of it) — partially
true, but leaves an unattributed residual that is neither floor nor
either named lever.** The bare async-UDP floor (≈4.70 µs/data-dg, 36.1%
of 13 µs, shared with quinn per (b) above) is the single largest
component of slither's per-datagram cost, and quinn's own total (4.1 µs)
sits close enough to its floor that quinn has very little further to
give. But: combining (a)'s connect() fix (≈1.41 µs) and (c)'s ACK-cadence
fix (≈1.53 µs) — the two cheap, measured levers — accounts for **≈2.94
µs/data-datagram, ≈22.6% of the 13 µs baseline, ≈33.0% of the 8.9 µs
gap, ≈53.5% of the stated 5.5 µs gap**, leaving a residual of **≈5.96 µs
(vs. the 8.9 µs gap) or ≈2.56 µs (vs. the stated 5.5 µs gap)** that is
*not* floor (floor is already counted in both slither's and quinn's
baseline, per (b)) and *not* either named lever. A rough,
**cross-run** (different invocations, not a controlled A/B) sanity
check against the given profile: the floor accounts for ~36.1
percentage-points of a core, the profile's `__sendto`+`__recvfrom`
account for ~49 — an ~12.9-point (≈1.67 µs/data-dg) excess of measured
syscall time over the isolated floor's prediction, which is roughly
consistent with (though not proof of) the unattributed residual having
a real, unmodeled syscall-adjacent component beyond the isolated-floor
number — separate from the "sendto anomaly" direction-mismatch above,
which questions whether even that 49% figure and this session's
measurements are from comparable conditions. **The most honest reading:
most of slither's per-datagram cost beyond quinn's is protocol/userspace
processing (crypto, framing, ACK bookkeeping logic itself, not just its
syscalls) that this instrumentation was not built to isolate** — a fifth,
uncategorized bucket the four remedy classes don't cover.

**Ranked by evidence strength, not necessarily by impact:**
1. Retry-storm DRIVER-PATTERN: refuted, high confidence (direct census).
2. ACK-VOLUME: real and large (30× cadence difference, directly
   measured on both sides), but its *share of the gap* is a floor-only
   lower-bound model.
3. connect()-socket driver-pattern fix: real, directly measured,
   moderate share, cheapest to ship and verify.
4. PLATFORM BATCHING: real but not gap-differentiating (shared with
   quinn) — not a lever for closing the slither-vs-quinn gap specifically.
5. IRREDUCIBLE / unattributed: the largest single named component (the
   floor) is genuinely shared/irreducible; a real residual remains
   unexplained by any of the four classes and needs its own
   investigation (likely crypto/framing/userspace profiling, not
   syscall-level instrumentation).

## Caveats

- **All slither/quinn/floor numbers in this report were measured in
  different processes at different times**, not a single controlled
  A/B harness. Cross-comparisons (e.g., "excess syscall time beyond
  floor" against the given profile) are order-of-magnitude sanity
  checks, not controlled experiments.
- **The census instrument changes the code path it measures.** The
  `try_send_to`/`try_recv_from` loop in `CountingWire` is not the
  production `self.inner.send_to(...).await` shape — it should behave
  equivalently (tokio's `.await` on these also resolves via `try_*` +
  readiness internally) but was not verified byte-for-byte against the
  production path's own syscall count independently.
- **The ACK-ratio and connect-vs-unconnect findings are about macOS
  loopback only.** Route/PCB lookup costs, GSO/GRO availability, and
  ACK-cadence tradeoffs can all differ materially on Linux or over a
  real network path with nonzero RTT and loss.
- **The remedy-class shares are additive by assumption**, not verified
  independent — implementing both the connect() fix and an ACK-cadence
  change and re-measuring is the only way to confirm they compose as
  modeled rather than overlapping or interacting.
- **quinn's own connect()/no-connect status was not verified** — the
  ~30% connect-vs-unconnect delta used in remedy class (b)'s "1.92 µs of
  non-floor cost" arithmetic is slither's own floor measurement, not
  confirmed against quinn's actual socket-setup code, so quinn's 4.1 µs
  baseline may or may not already include this saving.
- `quinnbench`'s modification (`udp_rx`/`frame_tx.acks`/`frame_rx.acks`
  added to one `println!`) lives entirely in the scratchpad copy,
  **outside both the slither worktree and the main repo** — nothing
  here touches slither's tree beyond the two files listed below.
- Every number above is from **this host, this run, this session** —
  re-run before trusting for a decision with material cost attached.

## Files touched in this worktree (uncommitted — MEASUREMENT, DO NOT LAND)

- `examples/bench_vs_tcp.rs` — `CountingWire` syscall census
  instrumentation (prior agent's work, unchanged this session).
- `examples/floor_probe.rs` — tokio/raw/spin UDP floor probe (prior
  agent's work) **plus** a `raw-recv` mode added this session (mirrors
  `raw-send`/`raw-send-drn` on the receive side; wired into `main()`'s
  mode dispatch and the doc-comment mode table).
- `DATAPATH-REPORT.md` — this file.
- `/private/tmp/.../scratchpad/perfctx/quinnbench/src/main.rs` — **not
  in the slither worktree or main repo** — a 15-line addition to one
  `println!` exposing `udp_rx.datagrams`/`frame_tx.acks`/`frame_rx.acks`
  from `quinn_proto::ConnectionStats`, for the ACK-ratio comparison.

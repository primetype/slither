# Round 42 material — recorded at the benchmark slice (2026/08/18)

**[Item 1 DONE 2026/08/18 — the R42 slice, rulings 269(iii) + 270
implemented: implementation `ba50a33` (VecDeque ring + credit-derived
ceiling), blind acceptance `03fc192` (red-on-base with the exact kill),
integration `069d7a0`, review carry-alongs `de50def`; verification in
`round42-F` (four ceiling mutants separating; Fable-fork review GO, two
findings landed); records in rulings 270 + the 269 addendum. Bench
after: ladder 83/80/97/97/95 MiB/s (was 12.3× spread), 100 ms column
monotone to 17.66 MiB/s, the kill cell completes. Carried onward, NOT
closed: the syscall/async-datapath bucket — ATTRIBUTED 2026/08/18
(`round42-G`, branch `datapath/attribution`): retry-storm REFUTED
(attempts/ok ≈ 1.0000 over ~2M calls); platform batching NOT
gap-differentiating (quinn pays the same no-GSO macOS floor; the honest
async floor is tok-oneway 3.12 µs/dg unconnected, and the old "2.3 µs"
anchor was a mislabelled connected-drain figure); TWO measured levers —
(i) ACK cadence: slither ACKs 1-per-2 data datagrams (33.6 % of all
datagrams) vs quinn's 1-per-58.6 (1.68 %), a 30× difference, ratified
§12.4 territory, floor-only lower bound ≈1.53 µs/data-dg; (ii)
connect()-per-peer addressing: 30.1 % per-call floor saving, ≈1.41
µs/data-dg, shell-internal but multi-peer/roaming design questions.
Combined model ≈ +2.94 µs → ~88 → ~113 MiB/s. A ≈6 µs residual is
UNATTRIBUTED (protocol/userspace processing — needs userspace CPU
profiling, a fifth bucket the syscall instrumentation cannot see). Two
rule-12 flags left open: the profile's sendto/recvfrom asymmetry does
not reproduce in isolation, and the two sessions' conditions may not be
comparable. Maintainer decision pending on which lever, if any, to
pursue.]**

1. **The O(window) ack-path drain** (ruling 269(iii), deferred by
   decision). `SendHalf`'s send buffer is a contiguous `Vec<u8>`;
   `release()` (`send.rs:581–595`) is `Vec::drain(..drop)`, memmoving
   the ≈window-sized remainder once per acked STREAM frame (≈ 880–930
   per MiB at ~1192 B MTU) — linear-in-window cost, measured 12× across
   the 3c ladder at zero loss, ~35 % of per-byte cost at the ratified
   default, implied ≈ 46 GB/s single-core memmove on the M4 Max
   reference host (review-corrected constant; NOT profiler-confirmed).
   The slice: (a) first settle attribution — a profile, or a one-off
   mutant replacing `buf` with a `VecDeque`/offset-cursor and re-running
   the ladder; (b) then the real representation change — internal only,
   no wire change, no API change; (c) acceptance gate: scenario 3c's
   window ladder re-run flat-or-rising where it now falls 12×, and
   scenario 2's default-window figure not regressed; (d) full blind
   process (author/verifier split, mutation evidence for any new
   invariant). Evidence: `bench-vs-tcp-2026-08.md` §3c + analysis;
   `examples/bench_vs_tcp.rs -- sweep` reproduces the ladder in ~2 min.

   **Addendum (2026/08/18) — cross-measurement anchors for the slice,
   from a quinn-on-this-host control (scratchpad `perfctx/`, slither
   untouched):** quinn 0.11.11 in slither's exact framing — one
   current-thread runtime carrying BOTH endpoints, datagrams pinned to
   1200 B (PMTUD off), macOS so no GSO (one sendmsg/recvmsg per
   datagram, same syscall discipline) — sustains **270 MiB/s**
   (forced-ChaCha20-Poly1305 control: 205 MiB/s), against slither's
   73 MiB/s. The bare-socket floor on this host (one thread, unbatched
   1200 B loopback send/recv, no crypto, no protocol) is **~500 MiB/s
   (~440k datagrams/s)**; cryptoxide 0.6.2 ChaCha20-Poly1305 seals
   1200 B at **~845 MiB/s per core** (NEON path active), so seal+open
   of 73 MiB/s is ≈17 % of the core — the cipher is never the wall.
   slither runs ≈64k datagrams/s where quinn runs ≈245k. **Second
   hypothesis for step (a), from quinn's stats:** co-scheduling both
   endpoints on one thread inflated quinn's *perceived* RTT to 1.29 ms
   (57.8 µs multi-thread) — a 256 KiB stream window at ~3.4 ms
   perceived RTT caps at exactly 73 MiB/s. Untested against slither
   (nothing read its estimator); the ladder's *falling* shape proves
   window-cap alone cannot be the whole story, so the attribution step
   must separate drain cost from window×perceived-RTT before choosing
   the representation fix — print slither's RTT estimate inside the
   bench as the first probe. Note also quinn's defaults for context:
   stream window 1.19 MiB (4.8× slither's), connection window
   effectively unbounded, send buffer 9.5 MiB.

   **Attribution DONE (2026/08/18) — `round42-A` (profile) +
   `round42-B` (mutant), both at `d8bb652`.** Step (a) is discharged,
   both ways at once, and the results reshape the slice:
   - **The drain is confirmed twice**: 21.45 % of on-CPU time at the
     default window (the exact `apply_ack_outcome → on_ack_range →
     release` path), 94.32 % at 8 MiB. The offset-cursor mutant
     (behaviour-preserving; 1095/1095 tests green unmodified) flattens
     the ladder 12.3× → 1.06×: default 72.5 → 88.5 MiB/s (+22 %; the
     fit's "~35 %" was an over-estimate — measured 18–24 %), 8 MiB
     5.7 → 90–94 (≈16×), **and the knob's sign flips** (a raise was
     12.7× slower, becomes 1.02× faster). At 100 ms the optimum stays
     2 MiB/8 MiB; best becomes 9.39 MiB/s (+4 %).
   - **The perceived-RTT hypothesis is retired as a cause**: srtt is a
     standing-queue *effect* (Little's law — srtt ≈ inflight ÷
     throughput across the whole ladder; min_rtt is 0.08–0.25 ms; the
     buffer mutant alone moves srtt 969 → 42 ms at 8 MiB).
   - **New, gates the slice: the 8 MiB/16 MiB cell at 100 ms RTT
     STALLS under the mutant, 3/3 runs** — 0 % CPU, parked in kevent,
     every timer idle; baseline finishes the same cell (its only lossy
     cell: 5.6 % loss, amp 1.101). 20 ms and 50 ms are fine with
     inflight pinned to cwnd. Unseparated: rate-enabled *pre-existing*
     defect vs mutant-only defect on a mass-retransmission path.
     Rule 13 squarely: FlakyWire cannot express a saturated socket.
     **The slice must resolve this before any representation change
     lands.**

   **Phase 1 verdict (2026/08/18, `round42-C`): PRE-EXISTING — and not
   a stall, a KILL.** The 8 MiB/100 ms cell dies at
   `recv.rs:882`: §10.6's flat `REASSEMBLY_CHUNKS_MAX = 1024` against
   8 MiB of advertised credit (≈6 990 packets). A conforming sender
   inside its credit, under one transient mass-loss event (measured:
   all 10 018 lost datagrams die in the receiving endpoint's own UDP
   socket buffer — 786 896 B ≈ 655 datagrams, `SO_RCVBUF` never set;
   the relay lost zero), exceeds the hole ceiling while obeying the
   credit and is closed with `PROTOCOL_VIOLATION`. Separated three
   ways: identical death with `send.rs` reverted verbatim to
   `d8bb652`; deterministic virtual-time reproducer on FlakyWire +
   paused clock, seed 0x4200_0001, with a rule-9 control that
   survives; the mutant's own invariant verified clean. Steady loss
   cannot reach it (NewReno collapses cwnd first) — it needs
   zero-loss-then-burst, exactly a socket buffer overflowing.
   Reachability is linear in the window: 83 / 267 / 946 / 1025 chunks
   at 256 Ki / 1 Mi / 4 Mi / 8 Mi; crossing ≈ 4.5 MiB; the ratified
   default has 12× margin. §10.6 shipped the ceiling
   *ratified-but-revisitable* gated on the Appendix B check whose "no
   stall" clause phase 1 has now run red at 8 MiB; ruling 269's own
   sizing advice (≈ 2×RTT×rate) steers operators past the crossing
   once the drain fix lands. Ruling 270 material; two bench-harness
   defects also recorded (probe task never joins a dead cell;
   `finish()` emits `n<5` instead of failing).

   - The real fix wants a ring buffer or an explicit slack bound, not
     the mutant's compact-at-half (peak buffer doubles; ruling 94
     makes per-stream allocation a budget).
   - **The next fruit after the drain is the syscall/async-datapath
     bucket**: 50 % of on-CPU time (~7.8 µs/datagram) against the
     measured 2.3 µs raw-socket floor — cause unattributed (tokio
     readiness machinery / wakeup churn are candidates, not findings);
     quinn's whole pipeline fits in 4.1 µs on the same discipline.
     Post-drain ceiling measured 90–95 MiB/s; recovering the syscall
     gap is the path toward quinn's 270.



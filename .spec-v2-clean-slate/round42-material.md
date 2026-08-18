# Round 42 material — recorded at the benchmark slice (2026/08/18)

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



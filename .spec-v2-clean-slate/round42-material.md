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


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

# slither test coverage measurement

Base commit expected: `cd12ed7` (verify against actual `git log --oneline -1`)

## 0. Base verification

Command: `git -C /Users/nicolasdiprima/work/primetype/slither log --oneline -1`
Output: `cd12ed7 Ruling 242: SPEC.md is stamped RATIFIED, with its amendment table`

**Matches expected base `cd12ed7`. Proceeding, no mismatch.**

## 1. Raw coverage numbers

### Summary run

Command:
```
cd /Users/nicolasdiprima/work/primetype/slither && cargo llvm-cov --all-features --workspace --summary-only
```

Output (test run trailer + full per-file table, verbatim):
```
test s13_a_stalled_stream_does_not_block_a_concurrent_one ... ok
test s12_loss_free_a_user_can_stream_over_a_reordering_duplicating_path ... ok
test s17_the_sender_resumes_when_the_reader_drains ... ok
test s17_a_slow_reader_stalls_its_own_stream_only ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s

     Running tests/story_tower.rs (target/llvm-cov-target/debug/deps/story_tower-dcfdfc3ac6bbb269)

running 6 tests
test s33_endpoint_service_dials_and_folds_the_synchronous_error ... ok
test s33_unsync_box_service_composes ... ok
test s33_a_service_call_opens_exactly_one_bi_stream ... ok
test s33_connection_service_poll_ready_is_immediate_and_opens_nothing ... ok
test s33_concurrent_calls_do_not_head_of_line_block_each_other ... ok
test s33_serve_runs_until_the_connection_dies_and_survives_a_failing_service ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

Filename                                Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
compat/codec.rs                              16                 2    87.50%           4                 0   100.00%          11                 0   100.00%           0                 0         -
compat/io.rs                                128                 3    97.66%          17                 0   100.00%         106                 1    99.06%           0                 0         -
compat/rt.rs                                 13                13     0.00%           1                 1     0.00%           8                 8     0.00%           0                 0         -
compat/stream.rs                            152                21    86.18%          23                 4    82.61%         105                16    84.76%           0                 0         -
compat/tower.rs                              97                21    78.35%          13                 3    76.92%          64                15    76.56%           0                 0         -
config.rs                                    75                12    84.00%          14                 1    92.86%          61                 8    86.89%           0                 0         -
constants.rs                                 69                 0   100.00%           4                 0   100.00%          76                 0   100.00%           0                 0         -
core/connection/ack.rs                      103                 1    99.03%           9                 0   100.00%          81                 1    98.77%           0                 0         -
core/connection/close.rs                    153                 0   100.00%          14                 0   100.00%          92                 0   100.00%           0                 0         -
core/connection/congestion.rs                66                 3    95.45%          11                 1    90.91%          79                 3    96.20%           0                 0         -
core/connection/datagram.rs                  71                 9    87.32%          11                 1    90.91%          51                 7    86.27%           0                 0         -
core/connection/flow.rs                     320                 1    99.69%          31                 0   100.00%         199                 1    99.50%           0                 0         -
core/connection/frame.rs                   1242                50    95.97%          85                 0   100.00%         760                11    98.55%           0                 0         -
core/connection/mobility.rs                 224                 3    98.66%          21                 1    95.24%         146                 3    97.95%           0                 0         -
core/connection/mod.rs                     2674               108    95.96%         113                 0   100.00%        1450                64    95.59%           0                 0         -
core/connection/recovery.rs                 362                 8    97.79%          27                 1    96.30%         258                 7    97.29%           0                 0         -
core/connection/recv.rs                     695                29    95.83%          50                 3    94.00%         420                20    95.24%           0                 0         -
core/connection/send.rs                     892                11    98.77%          56                 0   100.00%         514                 6    98.83%           0                 0         -
core/connection/session.rs                  982                15    98.47%          56                 0   100.00%         543                12    97.79%           0                 0         -
core/connection/stream_id.rs                124                 0   100.00%          15                 0   100.00%          85                 0   100.00%           0                 0         -
core/connection/streams.rs                 1781               118    93.37%          90                 4    95.56%        1019                71    93.03%           0                 0         -
core/connection/testfix.rs                 1112                72    93.53%          69                 0   100.00%         651                29    95.55%           0                 0         -
core/connection/tests_ack.rs               1927                 4    99.79%          76                 0   100.00%         812                 2    99.75%           0                 0         -
core/connection/tests_contested.rs          910                13    98.57%          42                 0   100.00%         451                 6    98.67%           0                 0         -
core/connection/tests_livelock.rs           586                12    97.95%          21                 1    95.24%         315                 7    97.78%           0                 0         -
core/connection/tests_path.rs              1120                10    99.11%          52                 2    96.15%         519                 7    98.65%           0                 0         -
core/connection/tests_reassembly.rs         559                 1    99.82%          16                 0   100.00%         211                 1    99.53%           0                 0         -
core/connection/tests_recovery.rs          1919                 1    99.95%          88                 1    98.86%         822                 1    99.88%           0                 0         -
core/connection/tests_roam.rs              1075                36    96.65%          45                 2    95.56%         512                16    96.88%           0                 0         -
core/connection/tests_sizing.rs             586                 4    99.32%          26                 0   100.00%         285                 4    98.60%           0                 0         -
core/connection/tests_streams.rs           3157                13    99.59%         126                 6    95.24%        1270                12    99.06%           0                 0         -
core/connection/timers.rs                   410                 0   100.00%          31                 0   100.00%         199                 0   100.00%           0                 0         -
core/endpoint/guard.rs                      201                44    78.11%          26                 6    76.92%         157                27    82.80%           0                 0         -
core/endpoint/handshake.rs                   55                 0   100.00%           2                 0   100.00%          25                 0   100.00%           0                 0         -
core/endpoint/intro_queue.rs                197                 7    96.45%          20                 1    95.00%         144                 3    97.92%           0                 0         -
core/endpoint/mod.rs                        517                14    97.29%          29                 0   100.00%         358                12    96.65%           0                 0         -
core/endpoint/routing.rs                   1474                80    94.57%          43                 1    97.67%         670                39    94.18%           0                 0         -
core/endpoint/staged.rs                     462                77    83.33%          15                 2    86.67%         284                48    83.10%           0                 0         -
core/endpoint/tables.rs                     102                 4    96.08%          16                 0   100.00%          66                 1    98.48%           0                 0         -
core/mod.rs                                  71                44    38.03%           8                 1    87.50%          49                20    59.18%           0                 0         -
error.rs                                    153                 0   100.00%          13                 0   100.00%         120                 0   100.00%           0                 0         -
identity.rs                                  51                 8    84.31%           5                 1    80.00%          32                 4    87.50%           0                 0         -
packet/header.rs                              9                 0   100.00%           3                 0   100.00%          23                 0   100.00%           0                 0         -
packet/mac.rs                                37                 1    97.30%           3                 0   100.00%          26                 1    96.15%           0                 0         -
packet/mod.rs                                68                 4    94.12%           2                 0   100.00%          53                 1    98.11%           0                 0         -
packet/payload.rs                            39                 0   100.00%           3                 0   100.00%          17                 0   100.00%           0                 0         -
packet/suite.rs                             134                51    61.94%          15                 2    86.67%         140                35    75.00%           0                 0         -
shell/connection.rs                         796                71    91.08%          94                 5    94.68%         534                53    90.07%           0                 0         -
shell/driver.rs                             797                50    93.73%          52                 0   100.00%         536                34    93.66%           0                 0         -
shell/endpoint.rs                           216                29    86.57%          21                 4    80.95%         183                17    90.71%           0                 0         -
shell/mod.rs                               1414                12    99.15%         103                 0   100.00%        1499                 3    99.80%           0                 0         -
shell/shared.rs                             423                14    96.69%          45                 0   100.00%         319                13    95.92%           0                 0         -
shell/staged.rs                             160                34    78.75%          23                 4    82.61%         129                23    82.17%           0                 0         -
shell/stream.rs                             455                68    85.05%          37                 4    89.19%         323                39    87.93%           0                 0         -
shell/wire.rs                               118                 1    99.15%          15                 0   100.00%          52                 0   100.00%           0                 0         -
testutil/mod.rs                            1717                29    98.31%         136                 7    94.85%         952                27    97.16%           0                 0         -
varint.rs                                    303                 0   100.00%          19                 0   100.00%         180                 0   100.00%           0                 0         -
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
TOTAL                                     33569              1236    96.32%        2005                70    96.51%       19046               739    96.12%           0                 0         -
```

**Headline: 96.32% region coverage, 96.51% function coverage, 96.12% line coverage (19046 lines, 739 missed; 1236/33569 regions missed; 70/2005 functions missed).**

Note: `Branches` column is all zero/`-` — this build of `cargo-llvm-cov`/toolchain is not collecting branch coverage (region coverage is the closest proxy available here).

JSON detail generated via:
```
cargo llvm-cov --all-features --workspace --json --output-path cov.json
```
→ `/private/tmp/claude-501/-Users-nicolasdiprima-work-primetype-slither/721dbfa4-ee18-4ecb-8459-b449d16c1367/scratchpad/cov.json`

### Per-file table (sorted by uncovered lines descending, top 25; testutil/tests_* files flagged as scaffolding)

| File | Lines | Missed | Line Cover% | Region Cover% | Note |
|---|---|---|---|---|---|
| core/connection/streams.rs | 1019 | 71 | 93.03% | 93.37% | core — real |
| core/connection/mod.rs | 1450 | 64 | 95.59% | 95.96% | core — real |
| shell/connection.rs | 534 | 53 | 90.07% | 91.08% | shell — real |
| core/endpoint/staged.rs | 284 | 48 | 83.10% | 83.33% | core — real |
| core/endpoint/routing.rs | 670 | 39 | 94.18% | 94.57% | core — real |
| shell/stream.rs | 323 | 39 | 87.93% | 85.05% | shell — real |
| packet/suite.rs | 140 | 35 | 75.00% | 61.94% | low region% — real, worth check |
| shell/driver.rs | 536 | 34 | 93.66% | 93.73% | shell — real |
| core/connection/testfix.rs | 651 | 29 | 95.55% | 93.53% | test *fixture* helper code (used by tests) |
| testutil/mod.rs | 952 | 27 | 97.16% | 98.31% | scaffolding — deprioritized per brief |
| core/endpoint/guard.rs | 157 | 27 | 82.80% | 78.11% | core — real, low% |
| shell/staged.rs | 129 | 23 | 82.17% | 78.75% | shell — real |
| core/mod.rs | 49 | 20 | 59.18% | 38.03% | tiny file, very low % — investigate |
| core/connection/recv.rs | 420 | 20 | 95.24% | 95.83% | core — real |
| shell/endpoint.rs | 183 | 17 | 90.71% | 86.57% | shell — real |
| core/connection/tests_roam.rs | 512 | 16 | 96.88% | 96.65% | test file itself — investigate why |
| compat/stream.rs | 105 | 16 | 84.76% | 86.18% | `sink`/`codec`/`tower` feature interplay — investigate |
| compat/tower.rs | 64 | 15 | 76.56% | 78.35% | `tower` feature — investigate |
| core/connection/session.rs | 543 | 12 | 97.79% | 98.47% | core — real |
| core/connection/tests_streams.rs | 1270 | 12 | 99.06% | 99.59% | test file itself |
| shell/shared.rs | 319 | 13 | 95.92% | 96.69% | shell — real |
| core/connection/frame.rs | 760 | 11 | 98.55% | 95.97% | core — real |
| compat/rt.rs | 8 | 8 | 0.00% | 0.00% | **entire tiny file uncovered** — investigate |
| config.rs | 61 | 8 | 86.89% | 84.00% | investigate |
| core/connection/datagram.rs | 51 | 7 | 86.27% | 87.32% | core — real |
| core/connection/recovery.rs | 258 | 7 | 97.29% | 97.79% | core — real |
| core/connection/tests_livelock.rs | 315 | 7 | 97.78% | 97.95% | test file itself |
| core/connection/tests_path.rs | 519 | 7 | 98.65% | 99.11% | test file itself |
| identity.rs | 32 | 4 | 87.50% | 84.31% | small |
| core/connection/tests_sizing.rs | 285 | 4 | 98.60% | 99.32% | test file itself |
| packet/mod.rs | 53 | 1 | 98.11% | 94.12% | trivial |
| core/connection/mobility.rs | 146 | 3 | 97.95% | 98.66% | core — real, small |
| core/connection/congestion.rs | 79 | 3 | 96.20% | 95.45% | core — real, small |
| core/endpoint/intro_queue.rs | 144 | 3 | 97.92% | 96.45% | core — real, small |

(remaining files ≤2 missed lines or 100% — see full table above)

## 2. Triage of uncovered code

Methodology: `cargo llvm-cov report --lcov` was regenerated from the **same**
profile data as the summary run above (no retest — cargo-llvm-cov reused the
cached `.profraw`/profdata), parsed into per-file `DA:` (line) and `FNDA:`
(function) records, and cross-referenced against source. This is line-level
data; the `Branches` column in the summary table is all-zero because this
toolchain build isn't collecting branch/MC-DC coverage, so region coverage
(LLVM's finer-grained "was this span of code, e.g. a match arm, executed"
metric) is the best proxy for branch coverage available here.

**A recurring artefact worth flagging up front**: for small `const fn`s,
generic/monomorphized methods, and anything the compiler inlines
aggressively, `cargo-llvm-cov`'s **function**-level table (`Missed
Functions`) frequently shows 0 executions for a function whose **line**-level
coverage is fully green — because every call site got inlined and the
stand-alone out-of-line copy of the function body was never directly
invoked. `core/connection/mod.rs` shows 26 "zero-count functions" (accessors
like `role()`, `write()`, `read()`, …) whose actual source lines are **not**
in the uncovered-line list at all — those are this artefact, not real gaps.
Likewise `packet/suite.rs` shows dozens of zero-count monomorphizations of
`Channel` trait methods for `second_suite`/`third_suite` test fixtures, all
attributed to line 234 (the `channel!` macro's own definition line, where
Rust's coverage instrumentation attributes macro-generated code by default)
— that line **is** covered. **Line-level uncovered ranges, not the function
table, are what the triage below is based on.**

### (a) Genuinely untested logic in core/shell — real risk

1. **`core::Timestamp::succ()`'s carry branch** — `src/core/mod.rs:149-153`.
   The nanosecond-rollover arm (`nanos >= 999_999_999` → `secs += 1, nanos =
   0`) is never exercised; only the plain `nanos + 1` arm is. This is
   `§5.2`'s initiation-timestamp forcing function and the doc comment calls
   out that ordering correctness depends on it. Textbook boundary the
   existing tests never hit.

2. **The §8.7 "regenerate" path for lost flow-control credit frames is
   entirely untested end-to-end.** `Streams::owe_max_data()`,
   `owe_max_stream_data()`, `owe_max_streams()` (`streams.rs:1175-1190`) are
   each one-line setters, individually trivial — but their only callers,
   `Connection`'s loss handler's `SentFrame::MaxData | MaxStreamData |
   MaxStreams` arms (`core/connection/mod.rs:1458-1460`), are **also**
   uncovered. No test currently drives a MAX_DATA / MAX_STREAM_DATA /
   MAX_STREAMS frame to be lost and checks it gets re-armed for
   retransmission. `tests_recovery.rs` has extensive STREAM-frame loss
   coverage but nothing for credit frames. If this path were actually
   broken, a peer that lost a MAX_STREAM_DATA update could stall
   indefinitely with no automatic recovery — a liveness bug this specific
   corner exists to prevent.

3. **`RecvTombstone::check_reset`** — `src/core/connection/recv.rs:403-414`
   — is 100% uncovered, and its one call site,
   `streams.rs:799 tomb.check_reset(f.final_size)` inside
   `on_reset_stream`, is *also* in the uncovered set. `RecvTombstone`
   validates a frame arriving for a receive half that has already been
   freed/retired (§9.7); the STREAM-frame sibling path
   (`tomb.check_stream`, `streams.rs:706`) **is** covered, but the
   RESET_STREAM sibling is not. A late/duplicate RESET_STREAM for an
   already-closed stream is a normal reordering/retransmission scenario on
   a lossy network, not an edge case — this is a real gap.

4. **The bounded LRU eviction in the connection-guard is never actually
   triggered.** `core::endpoint::guard::GuardEntries::evict_if_over_cap`
   (`guard.rs:480-504`) is the §17.1 mitigation that caps the unpinned tier
   at `TS_GUARD_ORPHAN_CAP` (1024) — a DoS-mitigation mechanism. Its
   *entire body* (the victim-selection scan and the removal) is uncovered.
   `record()` (`guard.rs:263-291`, `pub(crate)`) calls it unconditionally
   on every insert, but no test — including the fairly thorough
   `core/endpoint/tests.rs`, which exercises pinning, orphan-aging and the
   tie-break — ever creates more than 1024 distinct guard entries. This is
   the one item in this list that's a genuine **security-relevant**
   mechanism with zero verification that it does what its own doc comment
   claims.

5. **`compat::tower`'s borrowed-reference `Service` impl is entirely
   untested**, while its owned-`Connection` sibling is well tested.
   `impl<'a, S> Service<()> for &'a Connection<S>` (`compat/tower.rs:166-188`,
   both `poll_ready` and `call`) and its future's `Future::poll`
   (`tower.rs:205-208`) show zero executions. `story_tower.rs`'s 6 tests
   (`s33_*`) all appear to go through the owned `Connection`/`UnsyncBoxService`
   path (`compat/tower.rs`'s doc explains *why both impls exist* — the
   owned one is what `'static` combinators need — but never explains that
   only one is tested). Since the two impls are near-identical by
   construction, the risk this misses is that they silently drift, not
   that the logic is complex.

6. **A broad, repeated shape across `core/connection/streams.rs` and
   `mod.rs`**: `let Some(x) = self.entries.get_mut(&r) else { return; }`
   guard clauses (and their `send`/`recv`-half siblings) in `reset`,
   `read`, `on_ack_range`, `on_lost_range`, `on_reset_acked`, `fill`,
   `pack_control`, `restore`, `on_reset_lost`, `reset_for_overflow`,
   `note_message_progress`, `after_half_freed` — roughly 20 of
   `streams.rs`'s 71 missed lines. These guard the case where a
   `StreamRef` an outstanding event/regeneration/rotation entry still
   names has already been fully retired and removed from `entries` (or its
   relevant half already freed) by the time the event is processed — a
   real race between e.g. an ACK arriving and the stream having already
   been reset-and-reaped, not a theoretical one. None are individually
   worth a dedicated test, but as a set they're the single largest
   contiguous block of real, reachable, currently-unexercised logic in the
   crate. Distinct from item 6 in the ranked list below in that this is
   *many small guards*, not one path.

### (b) Error paths no fixture can currently reach — harness/build-matrix limitations

1. **`packet::suite::protocol_name` / `append` are `const fn`s evaluated
   entirely at compile time** (`packet/suite.rs:133-169`). The
   `channel!` macro (`suite.rs:247`) calls them inside a `const RAW: (...)
   = protocol_name(...)` binding to build each suite's Noise protocol
   name — i.e. they run in rustc's const evaluator when the crate is
   *compiled*, never as instrumented runtime code. **No test, however
   well-designed, can move this to "covered" under `cargo llvm-cov`** — the
   tool only observes runtime execution. This is a structural blind spot
   analogous to the `FlakyWire` socket-blindness the brief calls out, just
   at the opposite end (compile-time rather than runtime). Not a real risk
   — the value these functions produce is exercised indirectly by every
   golden-wire test (a wrong protocol name breaks the handshake hash) —
   but worth recording so nobody spends effort trying to "cover" it.

2. **The feature-combination axis is untested by both this coverage run
   and CI itself**, and this is a distinct, more consequential finding
   than raw line coverage. `.github/workflows/test.yml`'s matrix has
   exactly two feature states: `""` (default, nothing on) and
   `--all-features`. `check.yml`'s build/clippy/doc gates are the same
   two. `Cargo.toml`'s feature table (`git show cd12ed7:Cargo.toml`) is:
   ```
   test-util = []
   sink = ["dep:futures-core", "dep:futures-sink"]
   codec = ["sink", "dep:tokio-util"]      # codec implies sink — verified
   tower = ["dep:tower-service"]           # tower does NOT imply sink
   ```
   `tower` alone (`cargo test --no-default-features --features tower`) is
   therefore a real, valid, buildable configuration that has — as far as I
   can find — **never once been compiled**, by CI or by this
   `--all-features` coverage run (which turns everything on
   simultaneously and so can't distinguish "works because `tower` is
   correct" from "works because `sink` happened to also be on"). Same for
   `sink` alone, and `sink+tower` without `codec`. `compat/tower.rs`'s own
   doc comments assert `tower`'s code needs nothing from `compat::stream`
   — that claim has never been build-verified. This is a gap in the
   *build/CI matrix*, not something `cargo llvm-cov --all-features` (or any
   coverage tool run the same way) could ever surface on its own.

3. **`compat::rt::block_on`** (`src/compat/rt.rs:73-80`) is 0% covered —
   the entire function. This one is explained by the project's own testing
   philosophy rather than being a fixture gap in the usual sense: the
   module doc says outright *"slither's own timers are asserted on
   tokio's paused clock … `block_on` must never appear in a test that
   asserts a timer"*, and its doc-example is marked `no_run` (so even the
   doctest doesn't execute it). Every test in the suite drives the cores
   directly or over `FlakyWire` on a paused clock, so nothing in the
   existing suite has a reason to call the one function that builds a
   **real, unpaused** runtime. Listed here rather than in (a) because the
   reason it's uncovered is structural, but unlike item (b)/1 above, this
   one *can* be tested cheaply without violating the no-sleep rule (see
   the ranked list).

4. **`config::SystemClock::now()`'s pre-epoch `Err` arm**
   (`config.rs:45-48`) can only be hit if the OS reports a wall-clock time
   before 1970, which isn't something a test can construct without
   unsafely faking `SystemTime` itself. The crate already abstracts this
   behind the `WallClock` trait specifically so tests never need
   `SystemClock` — this is a legitimate, permanent blind spot for this one
   concrete impl, not worth chasing.

### (c) Debug/Display/derive boilerplate — not worth chasing

Hand-written `Debug` impls account for a real fraction of the "missed
lines" total and are explicitly documented as hand-written *because* the
derive would either not compile (fields without `Debug`) or would print
something that shouldn't be printed (a scalar, a cipher state):
`EndpointOutput<C>::fmt` (`core/mod.rs:333-355`, ~20 lines — the largest
single boilerplate block found), `Connection<S>::fmt`
(`shell/connection.rs:1376-1385`), `SendStream::fmt`
(`shell/stream.rs:533-539`), `RecvStream::fmt` (`shell/stream.rs:839-845`),
`BiStream::fmt` (`shell/stream.rs:987-992`), `Config::fmt`
(`config.rs:65-73`), `EndpointBuilder::fmt` (`shell/endpoint.rs:507-515`),
`SoftwareIdentity::fmt` (`identity.rs:214-218`). None of these are worth a
dedicated test; the field selection is visible by inspection and a
"contains no secret" assertion on one (`SoftwareIdentity`, since it
explicitly avoids printing the scalar) would be the only borderline case,
and even that is low value.

### (d) Defensive unreachable!/debug_assert arms — correctly uncovered

A large, consistent category: `debug_assert!(false, "<invariant
description>")` fallback arms that exist to turn a broken invariant into a
loud debug-build failure rather than a silent divergence, paired with an
honest (never a panic) release-build fallback. Found in:
- `core/connection/streams.rs` (5x): `self.role == None` guards in
  `on_stream_frame`, `on_reset_stream`, `on_max_stream_data` — "frames are
  applied only after the install".
- `core/endpoint/staged.rs:373-379`, `routing.rs:356-373` (2x): chain-state
  invariants around intro/tie-break handling.
- `core/endpoint/guard.rs:371-373`: `promote_pin` — "promoting a pin that
  was never taken".
- `shell/shared.rs:871-893` (`drain_endpoint`'s `no_core()` at
  `stream.rs:1006-1012`, similarly): "a handle-side endpoint verb queued an
  output" / "a connection cell held neither a core nor a close reason".
- `shell/driver.rs:1194-1198`: "`accept()` returns an established
  connection".

All are `self` -contradicting-its-own-established-invariant arms, correctly
uncovered by a healthy test suite, and **should not be chased** — writing a
test that reaches one would mean deliberately breaking the invariant it
guards.

### (e) Test-only / feature-gated scaffolding

- `src/testutil/mod.rs` — 27 missed lines, 98.31% already. Per the brief,
  deprioritized: this is the `Network`/`FlakyWire` fabric itself, not
  product code.
- `core/connection/testfix.rs` (29 missed) is test-fixture/builder code
  (`Solo`-style harness helpers used by `tests_*.rs`), same category as
  `testutil` — low interest.
- `tests_*.rs` files showing nonzero missed lines (`tests_roam.rs` 16,
  `tests_streams.rs` 12, `tests_ack.rs` 2, `tests_recovery.rs` 1, etc.) are
  overwhelmingly `assert!`'s own `{:?}` panic-message formatting arguments
  (only evaluated if the assertion fails — e.g. `streams.rs:1764,1775`
  inside its own inline `#[cfg(test)] mod tests`) or one genuinely dead
  helper: **`tests_roam.rs:844-848`'s `_addresses_are_distinct()`** is a
  plain `#[allow(dead_code)]` function, not a `#[test]`, that no test
  calls — looks like an orphaned leftover from a refactor. Worth a
  five-second look (delete it or wire it into a `#[test]`), not a coverage
  item.
- **`shell/connection.rs:1279-1288`, `settled_slot_boxed`**: confirmed via
  `grep` to have **zero callers anywhere in the crate**. This is *not* an
  oversight — the surrounding comment (`shell/connection.rs:1255-1261`)
  says explicitly: ruling 228 fixes the type-erased waker-slot family at
  "the complete mirror" of seven, and *"`settled_slot_boxed` has no
  adapter in slice 8 at all (§16.2's `acked()` has no `Stream` or `Sink`
  face)"* — i.e. it's deliberately-provisioned, currently-unused surface,
  documented as such, with `#[allow(dead_code)]` chosen over a `cfg`
  matrix specifically to avoid per-feature churn. Its 6 siblings
  (`opener_slot_boxed`, `acceptor_slot_boxed`, `notification_slot_boxed`,
  `message_reader_slot_boxed`, `datagram_reader_slot_boxed`,
  `message_sender_slot_boxed`) **are** called (from `compat/stream.rs` and
  `compat/tower.rs`) and mostly show as covered. I initially over-read this
  as "the whole waker-slot family is dead" from the raw line ranges; on
  inspection only this one is, and it's already explained in the source.
  Flagging the correction explicitly per working rule 4.

## 3. Low-hanging fruit, ranked

Ranked by (risk covered ÷ effort). All are plain `#[test]`s against a core
type unless noted; none require a paused-clock flow test except #5 and #6
which touch the shell/driver actor.

1. **`Timestamp::succ()` nanosecond-rollover** — `src/core/mod.rs:148-160`.
   Add to `core::connection::testfix` or a small test near
   `core/connection/tests_recovery.rs`/wherever `Timestamp` already has
   coverage: `assert_eq!(Timestamp::new(5, 999_999_999).succ(), Timestamp::new(6,
   0))` plus the ordinary-increment case for contrast. **Effort: 2
   minutes, one `#[test]`, no fixture needed.** This is exactly the "bound
   only a test if the degenerate case violates it" shape the brief warns
   about — right now there is no test that would fail if the carry logic
   were deleted.

2. **Guard's bounded-LRU eviction never fires** —
   `src/core/endpoint/guard.rs:480-504`, tested via `core/endpoint/tests.rs`
   (or a new inline `#[cfg(test)] mod tests` in `guard.rs` itself — there
   isn't one yet). `record()` is `pub(crate)` and needs no crypto/IO: loop
   `record(&key_i, ts, now)` for 1025 distinct keys with a monotonically
   advancing synthetic `Instant`/timestamp, assert the entry count stays
   at 1024 and that the specific oldest (`last_admitted`-minimum) key was
   evicted. **Effort: ~20 minutes** (mostly wiring a synthetic key
   generator and an `Instant` base — `core`'s `now: Instant` argument
   convention makes this trivial, no clock pausing needed since
   `GuardEntries` never reads a clock itself). **This is the highest-risk
   item on the list** — it's a named DoS mitigation with its core
   mechanism at zero verification.

3. **`compat::rt::block_on` has never been called** —
   `src/compat/rt.rs:73-80`. A single `#[test]` (plain `#[test]`, *not*
   `#[tokio::test]` — that would panic per the function's own "Panics"
   doc) calling `slither::block_on(async { 42 })` and asserting `42` comes
   back proves the runtime builds and the future actually runs, without
   touching any slither timer. **Effort: 5 minutes.** Belongs in a small
   new test file or alongside `compat/io.rs`'s existing tests — check
   where `compat`'s other plain (non-paused-clock) tests already live.

4. **§8.7 lost-credit-frame regeneration is unverified end-to-end** —
   spans `core/connection/mod.rs:1458-1460` and
   `core/connection/streams.rs:1175-1190`. Best done as one
   `tests_recovery.rs`-style test that: makes flow-control credit owed
   (drive `recv_window` to grant), sends the resulting MAX_DATA (or
   MAX_STREAM_DATA/MAX_STREAMS) frame, declares it **lost** (the existing
   loss-injection helpers `tests_recovery.rs` already uses for STREAM
   frames), and asserts the frame reappears on the next `poll_transmit`.
   **Effort: ~30-45 minutes** — needs to follow the existing loss-test
   idiom in `tests_recovery.rs` but the mechanism itself is simple once
   the credit is owed.

5. **`RecvTombstone::check_reset` — a late RESET_STREAM for an
   already-freed receive half is unverified** —
   `core/connection/recv.rs:403-414`, exercised via `Connection`/`Streams`
   in `tests_streams.rs` or `tests_reassembly.rs` (wherever the sibling
   `tomb.check_stream` case already has a test to mirror). Freed a receive
   half (e.g. via a completed message or an `abandon_recv`), then deliver
   a RESET_STREAM for the same stream ref and assert it's validated
   against the tombstone rather than silently mis-handled. **Effort: ~15
   minutes**, mirrors an existing test shape.

6. **`compat::tower`'s `&Connection: Service<()>` is untested while the
   owned impl is well-tested** — `compat/tower.rs:166-208`. Add one test
   to `tests/story_tower.rs` alongside the existing `s33_*` tests: take
   `&conn` instead of `conn`, call `.call(())`, `.await`, assert a stream
   opens — essentially a copy of whichever `s33_*` test exercises the
   owned path with one line changed. **Effort: 10 minutes.**

7. **The stale-`StreamRef` guard clauses across `streams.rs`/`mod.rs`
   mutators** (item (a)/6 above) — pick 2-3 representative ones (e.g.
   `reset()`/`read()` on an already-fully-retired `StreamRef`,
   `on_ack_range()` after the send half is already gone) and add direct
   `Streams`-level or `Connection`-level tests to
   `core/connection/tests_streams.rs`. **Effort: ~30 minutes for a
   representative subset**; covering literally every one of the ~15
   individual guard sites is not worth it (they're structurally
   identical), but zero of them currently have a test that would fail if
   the guard were removed and the code panicked on `.unwrap()` instead.

8. **`Channel::read_msg2` / `read_msg1_intro`'s `MessageTooShort` arms** —
   `packet/suite.rs:339-341` and `368-370` (inside the `channel!` macro
   body, so exercised per-suite). These guard a `TryFrom<&[u8]>` slice
   length conversion the doc comment says "cannot fail for anything the
   core routes here" — true for traffic through `core::Connection`, but
   the trait methods are directly callable, so a focused packet-level unit
   test (in `spec_packet.rs` or wherever `Channel` trait methods are
   already unit-tested with `ReferenceSuite`) calling
   `ReferenceSuite::read_msg2` / `read_msg1_intro` with a
   deliberately-wrong-length slice and asserting `Err(MessageTooShort)`
   is cheap and pins a real (if currently core-shielded) contract.
   **Effort: 10 minutes.**

9. **`SoftwareIdentity::generate()`'s success path is ambiguous in the
   coverage data** — `identity.rs:253-262`. Only line 259 (the `break` on
   a valid scalar draw) shows uncovered while the surrounding lines show
   covered, which doesn't have an obvious innocent explanation (a P-256
   scalar is valid on very close to 100% of draws, so `break` should fire
   almost every time `generate()` runs at all). I did not chase this
   further — flagging honestly rather than asserting a cause. **A direct
   test — `SoftwareIdentity::<S>::generate(seeded_rng)` and assert `Ok`
   — is cheap (10 minutes) and would resolve the ambiguity regardless of
   what's currently causing it.**

10. **Dead test helper** — `core/connection/tests_roam.rs:844-848`,
    `_addresses_are_distinct()`. Not a coverage item so much as a
    5-minute cleanup: either delete it or call it from one of the
    surrounding address-fixture tests.

**Not recommended, despite being uncovered**: chasing `packet::suite::protocol_name`/`append`
(compile-time only, see (b)/1), any `debug_assert!(false, …)` arm (see
(d)), any hand-written `Debug` impl (see (c)), or `testutil`/`testfix`
scaffolding (see (e)).

## 4. Coverage caveats — covered != tested

- **The FNDA/DA (function-count vs. line-count) mismatch described at the
  top of §2 is the main "don't over-trust the number" finding here.**
  `core/connection/mod.rs` reports 0 `Functions` missed (**100%** function
  coverage in the summary table) while its `lcov` function-record data
  shows 26 zero-count symbols — the two numbers measure different things
  (`cargo llvm-cov`'s summary table's `Functions` column apparently
  dedupes/rolls up across monomorphizations differently than the raw
  `FNDA` records do) and neither alone tells you whether a *specific*
  accessor was ever called outside of an inlined context. Don't read
  "100% function coverage" as "every function was called and asserted
  against" — several of `Connection`'s own public accessors
  (`role()`, `open()`, `write()`, `read()`, …) are only "covered" via
  inlined call sites; whether they're covered *with meaningful
  assertions* wasn't re-verified here (see next point).
- **A covered line is not an asserted line, and I did not re-audit every
  green line's test for a real assertion** — that would be a much larger
  task than this measurement pass. The one place I can say with
  confidence there's a degenerate-case-blind test (the specific failure
  mode the brief asks about by name) is the **absence** of one: `Timestamp::succ()`
  (ranked-list item 1) has *no test at all* that exercises the boundary,
  which is a cleaner and more useful finding than a test that exists but
  doesn't pin the boundary — there's simply nothing there yet to be
  fooled by.
- **The `evict_if_over_cap` finding (ranked item 2) is the sharpest instance
  of "coverage percentage hides risk" in this pass**: `guard.rs` sits at
  78.11% region / 82.80% line coverage, which reads as "mostly fine, low
  priority" from the summary table alone — but the missing 21.9% is
  concentrated almost entirely in one security-relevant function whose
  entire *reason to exist* (bounding a resource that's attacker-influenced,
  per its own doc comment) has never been exercised. The percentage and
  the risk are not proportional here.

## 5. Harness-limitation findings (ratified-finding-shaped)

Two distinct classes found, both **structural** — no amount of additional
test-writing against the *current* harness would close them:

1. **Compile-time-only code is invisible to `cargo llvm-cov` by
   construction** (`packet::suite::protocol_name`/`append`, detailed in
   §2(b)/1). Source-based coverage instruments and observes *runtime*
   execution; a `const fn` invoked only from a `const` binding runs in
   rustc's const evaluator during compilation and leaves no runtime trace
   to instrument, regardless of test quality. This is the mirror image of
   the `FlakyWire` socket-blindness the brief names (that one hides
   *reachable-at-runtime-but-unmodeled* faults; this one hides
   *code that's never at runtime at all*).

2. **The feature-combination axis is entirely unverified, by CI and by
   this coverage run alike** (detailed in §2(b)/2). `cargo llvm-cov
   --all-features` — like every gate in `CLAUDE.md`'s release-gate table
   except the bare `cargo test`/`cargo build` (which use *no* features) —
   only ever exercises the two extremes (`""` and everything-on). A
   feature combination that compiles cleanly under `--all-features` but
   is broken under, e.g., `--features tower` alone (no `sink`) would be
   invisible to every gate currently run, coverage included, until
   someone builds that exact combination by hand. I did not attempt to
   build `--no-default-features --features tower` myself (out of scope
   for a coverage-measurement pass, and `CLAUDE.md`'s concurrency notice
   asks me not to touch `Cargo.toml`/lockfile while the integrator is
   editing it) — flagging the gap in the verification story rather than
   resolving it.

## Notes / conflicts encountered

- No conflicts between spec/rulings/code encountered — this was a
  coverage-measurement task with no wire-behaviour questions in scope.
- One self-correction during this pass, recorded per working rule 4: I
  initially read `shell/connection.rs`'s scattered uncovered-line ranges
  as "the whole family of seven type-erased `*_slot_boxed` waker
  accessors is dead code." On inspection only one of the seven
  (`settled_slot_boxed`) actually has zero callers, and that one is
  explicitly documented in the surrounding comment as deliberately
  unused pending a future adapter (ruling 228). The other six are called
  from `compat/stream.rs`/`compat/tower.rs` and are mostly covered. See
  §2(e) for the corrected version.
- I did not exhaustively read every one of the ~739 missed lines —
  given the crate's size and that this is a triage/recommendation pass,
  I sampled representative ranges per file (particularly for
  `core/endpoint/staged.rs` and `core/endpoint/routing.rs`, where I
  confirmed the dominant pattern — `debug_assert!(false, …)` invariants
  and small conditional branches inside intro/tie-break bookkeeping — but
  did not individually characterize all 48 and 39 missed lines
  respectively). The patterns found were consistent enough across files
  that I'm confident the ranked list above captures the highest-value
  items, but a maintainer wanting a complete line-by-line accounting of
  every file should treat this as a strong start, not exhaustive coverage
  of the coverage report.

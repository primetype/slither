# Round 39 worklist — the resume point

**Written 2026/08/16, paused for usage credits.** This file is the single
entry point for picking the work back up cold. Everything it references is
committed; nothing lives in a scratchpad.

## Where the repo is

| | |
|---|---|
| Commit | `2989b44` — *"Round 39: post-slice review"* |
| Slices | **0–9 complete.** `PLAN.md`'s ten vertical slices are done |
| Tests | **1041** passing under `--all-features`, 864 at default |
| Coverage | **96.32% region / 96.51% function / 96.12% line** |
| Gates | **all nine green** on `2989b44`, verified and pasted in-session |
| Spec | `SPEC.md` stamped **RATIFIED 2026/08/14**, wire version 1, amendment table current |
| Rulings | **248**, in `rulings.md` |

**Nothing below is blocking a release.** Slice 9 (ship) is done on
slither's side. The one named slice-9 item still outstanding is the
**bubble-engine cutover**, and it is not in this file because it is not
slither's tree — see "Not slither's tree" at the bottom.

## How to resume — read in this order

1. **This file** for the worklist.
2. **`rulings.md` §243–248** (search `## Round 39`) — the round's decisions
   and the eight open questions, with reasoning.
3. The three agent reports, only for the item you are actually working on:
   - `review-api-opus-r39.md` — 685 lines, full `pub` enumeration, S1–S33
     story table, 6 reported conflicts
   - `review-api-sonnet-r39.md` — 843 lines, independent, same task
   - `review-coverage-r39.md` — 599 lines, per-file table, ranked
     opportunities at §3, harness limitations at §5
4. **`CLAUDE.md` working rules** before dispatching any agent. Rule 4 gained
   clause (c) this round.

All three reports verified their base as `cd12ed7`; the review fixes landed
in `2989b44` on top of it, so a handful of their findings are **already
fixed** — see "Already done" below before acting on a report.

## Already done in `2989b44` — do not re-fix

| Ruling | What |
|---|---|
| 243 | `compat/io.rs`'s `write_kind` `_ =>` arm deleted (ruling 238 finally applied to the code) |
| 244 | `#[must_use]` on `Intro` / `Claimed` / `Proven` (S8) |
| 245 | `wire.rs`'s false `Endpoint<W: Wire>`; `compat/mod.rs`'s "every adapter borrows" |
| 246 | `clone_handle` gated `#[cfg(feature = "tower")]` |
| 247 | `benches/throughput.rs` added |

## Part A — coverage work. No decision needed, just do it

Ranked by risk ÷ effort. Full detail with file:line and test shape is
`review-coverage-r39.md` §3. **Working rule 6 applies**: for anything
story-level, the test author is not the implementer.

| # | Item | Where | Effort |
|---|---|---|---|
| **A1** | **Bounded-LRU eviction never fires in any test** — a named DoS mitigation capped at 1024 with its core mechanism at zero verification. **Highest risk on the list.** `record()` is `pub(crate)`, needs no crypto or IO: loop 1025 distinct keys with an advancing `Instant`, assert the count holds at 1024 and the `last_admitted`-minimum key is the one evicted | `core/endpoint/guard.rs:480-504` | ~20 min |
| **A2** | `Timestamp::succ()` nanosecond-rollover carry has no test — deleting the carry logic breaks nothing today. Working rule 9's shape exactly | `core/mod.rs:148-160` | 2 min |
| **A3** | `compat::rt::block_on` has never been called — the first line a consumer hits, 0% covered. Plain `#[test]`, **not** `#[tokio::test]` (that panics per its own docs) | `compat/rt.rs:73-80` | 5 min |
| **A4** | §8.7's lost-credit-frame regeneration unverified end-to-end. Make credit owed, send the MAX_DATA frame, declare it lost with `tests_recovery.rs`'s existing loss injection, assert it reappears. Liveness risk | `core/connection/mod.rs:1458-1460`, `streams.rs:1175-1190` | ~30–45 min |
| **A5** | `RecvTombstone::check_reset` — a late RESET_STREAM for an already-freed receive half. Mirrors the tested `check_stream` sibling | `core/connection/recv.rs:403-414` | ~15 min |
| **A6** | `&Connection: Service<()>` (the borrowed tower impl) is untested while the owned one is well covered | `compat/tower.rs:166-208` | ~10 min |
| **A7** | ~15–20 stale-`StreamRef` guard clauses, a recurring shape. Pick a representative subset rather than chasing all | `streams.rs`, `connection/mod.rs` | ~30 min |
| **A8** | `Channel::read_msg2` / `read_msg1_intro` `MessageTooShort` arms — core-shielded but directly reachable through the trait | `packet/suite.rs:339-341, 368-370` | 10 min |
| **A9** | Delete dead test helper `_addresses_are_distinct()` | `core/connection/tests_roam.rs:844-848` | 5 min |

**Two things the coverage number does not say**, both from
`review-coverage-r39.md` §4–5 and worth more than the percentage:

- `packet::suite::protocol_name` / `append` are `const fn`s evaluated
  entirely at compile time through the `channel!` macro — **invisible to any
  runtime coverage tool by construction**. The mirror of the `FlakyWire`
  socket-blindness finding (working rule 13).
- `guard.rs` reads as 78–83% and therefore "low priority", but the gap is
  concentrated almost entirely in A1. **Percentage and risk are not
  proportional there.**

## Part B — needs a maintainer decision. `rulings.md` §248

Do not settle these by picking the plausible-looking side; each is a call.

| # | Question | Evidence |
|---|---|---|
| **B1** | **`SendCreditAvailable` is in the code and in no spec section.** §16.4 lists 13 `ConnEvent` variants, §16.2 re-enumerates 10, the code has 14. Ruling 150 authorised it; `grep SendCreditAvailable SPEC.md` returns nothing. Working rule 8's defect class, in the section that rule was written about. **Spec amendment, not a code change** | opus F3, `rulings.md:3833-3839` |
| **B2** | **`BiStream::join` has two signatures in the spec.** §16.2 (`SPEC.md:5112`) gives `Result<Self, (SendStream, RecvStream)>` per ruling 120; §16.11 (`SPEC.md:6219`) gives `-> Self`. Code follows §16.2. Reported not resolved, per working rule 3 | opus conflict 1 |
| **B3** | **A feature-matrix CI job.** The gates build exactly two points on the lattice — `""` and `--all-features` — and ruling 246's defect lived at every point between. Eight combinations pass today; nothing keeps them passing. Changes the gate table, so it is the maintainer's call | ruling 246, coverage §5 |
| **B4** | **Window auto-tuning.** Today a single stream is capped at `INITIAL_MAX_STREAM_DATA / RTT` ≈ 2.5 MiB/s at 100 ms, and `Config` exposes no flow-control knob. Whether slither wants auto-tuning at all is a protocol decision. **No constant was touched** — they are ratified | ruling 247(a) |
| **B5** | **`ConnectionId` and `IntroId`** are re-exported at the crate root, appear in no public signature, and have no public constructor or accessor. `lib.rs:268` also says "three identifiers" and re-exports five. Dead surface about to be frozen under semver | opus F10 |
| **B6** | **No public route to a second `Connection` handle.** `clone_handle` is `pub(crate)`; `Rc<Connection>` is the answer, is used in `tests/story_lifecycle.rs:850`, and appears in no rustdoc | opus F8 |
| **B7** | **11 public types lack `Debug`** — all eight `compat::stream` adapters plus `Connect`, `OpenBi`, `OpenBiOwned`, and seven in `testutil`. Rust API guideline C-DEBUG | opus F6 |
| **B8** | **STORIES.md S18 is stale**: it has the application observing `ConnEvent::AddressMoved`, which is `pub(crate)`; ruling 46 routed it to `Notification`. Code is right, story text was never swept | opus conflict 4 |

## Part C — carried forward from earlier rounds

- **Ruling 233** — `Incoming`'s `None` is documented as "the endpoint
  closed", the one route ruling 231's borrow makes unreachable. The
  reachable route (a driver panic) is unmentioned, and per working rule 13
  no fixture here can express it.
- **`serve` returns `ConnectionLost`** where `PLAN.md` §3.4 sketched a
  `Result`. `CONTRACT-8.md` §6.3 flags it; no ruling has settled it.
- **Deferred, unscheduled:** H1 (§6.9's 2-DH vs 4-DH accounting), F2 (needs
  a harness that can express N idle connections), H3's `generate_os()`, A4,
  F4, F6, L1–L3.

## Part D — throughput follow-ups, from ruling 247

Numbers on this machine at `2989b44`, single-threaded with **both** endpoints
on one runtime (so not comparable to a two-process iperf figure):

```
loopback/stream    52 MiB/s (437 Mbit/s)   real UDP syscalls
inmem/streams×4    86 MiB/s                four streams, same total bytes
inmem/stream       36 MiB/s                one stream
inmem/stream@rtt  8.4 MiB/s                same code, 20 ms injected RTT
inmem/datagram     62 MiB/s                100% delivered
inmem/message     167 MiB/s
inmem/handshake   284 conn/s (3.5 ms)      incl. two P-256 keygens
```

- **D1** — the **stream/message gap is unexplained.** Messages reach
  167 MiB/s where four parallel streams reach 86 and one reaches 36. Two
  explanations are already dead: the read path *does* coalesce (mean fill
  ~45–56 KB against a 1169-byte wire payload), and the `FlakyWire` tap
  allocation is accounted for. Worth profiling rather than guessing.
- **D2** — **one stream leaves most of the connection unused** (36 vs 86).
  The per-stream path serialises something the connection does not. S13's
  independent-streams promise is therefore a throughput lever, not only a
  head-of-line-blocking one, and that is not documented anywhere.
- **D3** — `testutil::FlakyWire` **retains every datagram it ever sends**:
  `send_to` pushes a `Spied { bytes: buf.to_vec() }` into the tap and queues
  a second copy for delivery. Two heap allocations per datagram plus
  unbounded growth until something drains it. Fine for short tests, wrong
  for a benchmark or any long-running use. A `Network::set_tap_enabled`
  (or a tap that is opt-in) would fix both. **Not done this round** because
  the reviewers held the tree read-only and `src/` had to stay at `cd12ed7`.

## Commands

```sh
# the benchmark (release profile, ~30 s)
cargo bench --features test-util --bench throughput
cargo bench --features test-util --bench throughput -- loopback   # substring filter

# coverage
cargo llvm-cov --all-features --workspace --summary-only

# the feature matrix ruling 246 says nothing currently guards
for f in "" test-util sink codec tower sink,tower test-util,codec test-util,sink,tower; do
  if [ -z "$f" ]; then cargo clippy --all-targets -- -D warnings
  else cargo clippy --features "$f" --all-targets -- -D warnings; fi
done

# the nine release gates
cargo build --all-features --all-targets
cargo fmt --all --check
cargo clippy --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo test && cargo test --all-features
cargo test --release --all-features
cargo +1.96 check --all-features --all-targets
cargo deny check
```

## Not slither's tree

**The bubble-engine cutover.** `../bubble-reboot/bubble-engine/Cargo.toml`
already points here — `slither = { path = "../../slither" }` plus a dev entry
with `test-util` — but **that repo has uncommitted work in it**: modified
`Cargo.toml` and `bubble-engine/Cargo.toml`, and a deleted vendored
`slither/` copy. Nothing has been run or edited there. Ruling 166 is the
reason: reading another agent's uncommitted tree produced two wrong rulings
and publicly faulted an agent that had been correct.

Two things to know before that build runs: slice 8 added `compat/`, and
**ruling 241 changed when `WriteError::Reset` fires on a bidirectional
stream** — if bubble-engine relied on the old behaviour it surfaces there.

**No crates.io publish.** The maintainer does that.

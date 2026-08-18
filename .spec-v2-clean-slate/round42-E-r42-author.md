# Round 42, report E — the R42 blind test author (ceiling property tests)

*Opus blind author, worktree at `405796e`; commit `e0ff8ab`, cherry-picked as `03fc192`. Its partition refusal (an existing tests_reassembly.rs my brief wrongly named) is the slice's first rule-5/6 catch. Verbatim report below.*

---

# Round 42 blind test author — report

Base: `405796ef8179a45a9d6552f7979997cb62e52fe7` (verified by `git rev-parse HEAD`
as first act, rule 14).

Pinning ruling 270: the reassembly hole-ceiling becomes credit-derived —
`ceiling = max(REASSEMBLY_CHUNKS_MAX = 1024, f(advertised stream window))` —
with the property that a conforming full-frame sender cannot be killed by any
loss pattern, while a tiny-fragment flood still dies with
CLOSE(PROTOCOL_VIOLATION).

## Tests

### `tests/story_reassembly.rs` — the survival half (shell level, paused clock)

One fixture, four window points. `Pair::seeded_with(0x4200_0001, Config::new()
.with_flow_windows(stream, connection))`; a lossless 5 ms one-way fabric both
ways; on the forward wire **one transient mass-loss burst** —
`FlakyPolicy::drop_at` over every **even** A-wire send index in
`[5500, 11500)`, i.e. 3 000 discards, installed **before the handshake** so it
needs no observation and no RNG; a saturating writer of 12 MiB in 64 KiB
`SendStream::write` calls; a reader draining continuously and checking every
byte.

| # | test | window pair |
|---|---|---|
| 1 | `a_conforming_sender_survives_a_transient_mass_loss_burst_at_eight_mib` | 8 MiB / 16 MiB |
| 2a | `the_property_holds_below_the_base_builds_crossing` | 2 MiB / 8 MiB |
| 2b | `the_property_holds_at_a_second_raised_window` | 16 MiB / 32 MiB |
| 4 | `the_ratified_default_completes_the_same_burst` | 256 KiB / 1 MiB |

Each asserts, in this order: **not** killed with
`ConnectionLost::ProtocolViolation { code: 1 }` (so the red is the ratified
kill and nothing else); reader and writer both `Ok`; all 12 MiB read back
byte-for-byte; and `traversed()` — `a_sends >= 11500`, so all 3 000 discards
really happened (rule 9: a survival assertion over a burst that did not
happen is satisfied by every build).

2b is the test that separates **credit-derived** from **a bigger constant**: a
build that answers ruling 270 by raising `REASSEMBLY_CHUNKS_MAX` to a new
fixed number passes 1 and dies at 16 MiB.

### `src/core/connection/tests_reassembly_credit.rs` — the abuse half (core level)

Six sans-io tests. `now` is an argument, no clock, no runtime. The abuse
shape cannot be written at the public API — one-byte frames at alternating
offsets are not something any shipped verb emits — so these drive
`core::Connection` with hand-built frames through `testfix`'s `Solo`, at
raised windows built from `ConnSeed { sub_seed, windows: FlowWindows { .. } }`
(the same seed `connect()` takes; ruling 259(viii)).

| # | test | shape |
|---|---|---|
| 3a | `a_tiny_fragment_flood_still_dies_at_a_raised_window` | 65 536 one-byte fragments at odd offsets, 8 MiB window |
| 3b | `a_tiny_fragment_flood_still_dies_at_the_default_window` | 32 768 fragments, ratified default |
| 5a | `the_ceiling_floor_is_exactly_1024_at_the_default_window` | 1 024 alive, the 1 025th is a violation |
| 5b | `the_floor_holds_at_a_raised_window_too` | 1 024 alive at 8 MiB |
| 6 | `the_honest_hole_shape_survives_at_a_raised_window` | 2 000 packet-sized ranges with packet-sized holes, 8 MiB |
| 6c | `the_same_bytes_arriving_contiguously_are_untroubled` | rule-9 control for 6 |

Test 6 is **extra to the brief** and is the cheap core-level twin of story
test 1: it is the same property with congestion control, recovery and the
clock removed, so a red there separates "the ceiling" from "the sender".

#### The arithmetic every bound here is derived from

At an advertised stream credit `C`, the most discontiguous ranges each shape
can store is:

| shape | bound at C = 8 MiB |
|---|---|
| full-size frames, every second datagram lost | `C / (2 · MAX_DATAGRAM)` = **3 495** |
| one-byte frames at alternating offsets | `C / 2` = **4 194 304** |

To hold `N` holes a sender must cover `2N` packets of offset space, and its
packets are `MAX_DATAGRAM` = 1 200 bytes; a flooder's are 1. That factor of
1 200 is the whole separation, and each test states where inside it it sits:
test 6 at 2 000 (43 % under the honest bound, 95 % over the old ceiling),
test 3a at 65 536 (**18.7×** the honest bound, spending 1.6 % of the credit,
so the death cannot be §10.5's flow violation — `assert_violation` pins the
code as `PROTOCOL_VIOLATION` `0x01`, not `FLOW_CONTROL_ERROR` `0x02`).

Test 5a is a deliberate duplicate of `tests_streams.rs`'s
`exactly_1024_stored_ranges_survive_and_the_1025th_is_a_protocol_violation`.
That file is another author's under rule 6 and I may not touch it; if the
implementation edits it, 5a is the independent pin that survives the edit.
Stated in its rustdoc rather than left to be found.

## Red-green matrix at base (`405796e`)

Expected by the brief: 1 RED, 2 red/green per the measured curve, 3 GREEN,
4 GREEN, 5 GREEN. Observed: exactly that, plus the two extra tests.

| # | test | expected | observed |
|---|---|---|---|
| 1 | `a_conforming_sender_survives_…_at_eight_mib` | RED (the kill) | **RED, the kill** |
| 2a | `the_property_holds_below_the_base_builds_crossing` (2 MiB) | GREEN | GREEN |
| 2b | `the_property_holds_at_a_second_raised_window` (16 MiB) | RED | **RED, the kill** |
| 4 | `the_ratified_default_completes_the_same_burst` | GREEN | GREEN |
| 3a | `a_tiny_fragment_flood_still_dies_at_a_raised_window` | GREEN | GREEN |
| 3b | `a_tiny_fragment_flood_still_dies_at_the_default_window` | GREEN | GREEN |
| 5a | `the_ceiling_floor_is_exactly_1024_at_the_default_window` | GREEN | GREEN |
| 5b | `the_floor_holds_at_a_raised_window_too` | GREEN | GREEN |
| 6 | `the_honest_hole_shape_survives_at_a_raised_window` | (extra) RED | **RED, the kill** |
| 6c | `the_same_bytes_arriving_contiguously_are_untroubled` | (extra) GREEN | GREEN |

### `tests/story_reassembly.rs`

```
$ cargo test --all-features --test story_reassembly -- --nocapture --test-threads=1
running 4 tests
test a_conforming_sender_survives_a_transient_mass_loss_burst_at_eight_mib ...
thread '…' panicked at tests/story_reassembly.rs:311:5:
RULING 270 at 8 MiB / 16 MiB: a conforming full-frame sender inside its advertised credit was killed with CLOSE(PROTOCOL_VIOLATION) by a loss pattern. read 6342439 of 12582912 bytes, 9212 A-sends; reader=Err(ConnectionLost(ProtocolViolation { code: 1 })) writer=Ok(())
FAILED
test the_property_holds_at_a_second_raised_window ...
thread '…' panicked at tests/story_reassembly.rs:311:5:
RULING 270 at 16 MiB / 32 MiB: a conforming full-frame sender inside its advertised credit was killed with CLOSE(PROTOCOL_VIOLATION) by a loss pattern. read 6342439 of 12582912 bytes, 9210 A-sends; reader=Err(ConnectionLost(ProtocolViolation { code: 1 })) writer=Ok(())
FAILED
test the_property_holds_below_the_base_builds_crossing ... ok
test the_ratified_default_completes_the_same_burst ... ok

test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.18s
```

The red is the ratified kill and nothing else: `ProtocolViolation { code: 1 }`
is asserted **first**, before "the connection survived" and before "the
transfer completed", so a fixture artefact reports a different message. The
two greens both cleared `traversed()` — `a_sends >= 11500` — so all 3 000
discards happened in those runs too.

### `src/core/connection/tests_reassembly_credit.rs`

```
$ cargo test --lib --all-features tests_reassembly_credit
running 6 tests
test core::connection::tests_reassembly_credit::the_floor_holds_at_a_raised_window_too ... ok
test core::connection::tests_reassembly_credit::the_ceiling_floor_is_exactly_1024_at_the_default_window ... ok
test core::connection::tests_reassembly_credit::a_tiny_fragment_flood_still_dies_at_a_raised_window ... ok
test core::connection::tests_reassembly_credit::a_tiny_fragment_flood_still_dies_at_the_default_window ... ok
test core::connection::tests_reassembly_credit::the_honest_hole_shape_survives_at_a_raised_window ... FAILED
test core::connection::tests_reassembly_credit::the_same_bytes_arriving_contiguously_are_untroubled ... ok

failures:

---- core::connection::tests_reassembly_credit::the_honest_hole_shape_survives_at_a_raised_window stdout ----
thread '…' panicked at src/core/connection/tests_reassembly_credit.rs:184:9:
RULING 270: a hole pattern a conforming full-frame sender can produce inside its advertised credit was answered with CLOSE(PROTOCOL_VIOLATION)

test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 760 filtered out; finished in 0.37s
```

### Nothing else in the suite moved

```
$ cargo test --all-features --no-fail-fast
… (26 targets)
test result: FAILED. 765 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;   [--lib, test 6]
test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out;     [story_reassembly, tests 1 and 2b]
   every other target: ok
```

### Gates run on this tree (rule 7)

```
$ cargo fmt --all --check
FMT CLEAN                          (no diff; the run above shows the command's own output)

$ cargo clippy --all-features --all-targets -- -D warnings
   (no output — zero warnings)

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.33s
   Generated …/target/doc/slither/index.html

$ cargo test --no-run          # default features, no `test-util`
    Finished `test` profile …
  Executable tests/story_reassembly.rs (…)
```

The last one is the one that matters for the missing `[[test]]` stanza: the
inner `#![cfg(feature = "test-util")]` makes the auto-discovered target
compile to an empty binary without the feature, so the plain `cargo test`
gate is **not** broken by the stanza being the integrator's job.

`cargo test --release --all-features`, MSRV and `cargo deny` were **not**
run — they are the integrator's, and two of these tests are red by design at
this commit.

## How I ran them without owning the `mod` declaration

`src/core/connection/tests_reassembly_credit.rs` is a core-level module, so
it needs a declaration in `mod.rs` to compile — the file `mod.rs` belongs to
neither blind agent (rules 6, 11, 15). I added one line to `mod.rs`
temporarily, ran every gate above, and **reverted it before committing**:

```
// TEMPORARY-BLIND-AUTHOR-DECL — reverted before commit; the integrator owns it.
#[cfg(test)]
mod tests_reassembly_credit;
```

`git show --stat` on the commit below confirms `mod.rs` is not in it. The
declaration the integrator must add, next to `mod tests_reassembly;` at
`src/core/connection/mod.rs:135`, is those two lines without the comment.

`tests/story_reassembly.rs` needed no such trick: cargo auto-discovers
`tests/*.rs`, and `--all-features` supplies `test-util`. The
`[[test]] name = "story_reassembly" / required-features = ["test-util"]`
stanza is still the integrator's to add (rule 15); the inner `cfg` above
keeps the featureless gate green until it does, and may be dropped when the
stanza lands.

## Derivations used

### round42-C-reassembly-kill.md (committed evidence)

The reproducer recipe, carried over verbatim from its §4 ("The regression
test"):

* seed `0x4200_0001`, `Config::new().with_flow_windows(8 MiB, 16 MiB)` both
  ends, a 5 ms one-way lossless fabric while the writer saturates;
* burst armed the first tick `cwnd > 2_600_000` (so >1024 holes can coexist
  before any retransmission lands) — reached at send index 5502,
  `cwnd=2612439`, tick 20;
* `FlakyPolicy::drop_at` on **every even send index** for the next 6 000
  sends — deterministic, no RNG. Explicitly **not** `FlakyPolicy::lossy`:
  rows 3/4 of its separation table show a *steady* loss rate drives NewReno's
  window below the hazard, so a `lossy`-based test passes on a broken build
  for the wrong reason (rule 9 in the fixture rather than the assertion).

The measured reachability curve (its "high-water discontiguous-range count"
probe), which fixes which windows are red at base:

| windows | stream_window | max_chunks | at base |
|---|---|---|---|
| ratified default 256Ki/1Mi | 262144 | 83 | survives (12x margin) |
| 1Mi/4Mi | 1048576 | 267 | survives |
| 4Mi/8Mi | 4194304 | 946 | survives (8 % margin) |
| 8Mi/16Mi | 8388608 | **1025** | **KILLED** (ceiling 1024) |

Crossing ≈ 4.5 MiB. Deaths observed as receiver
`ReadError::ConnectionLost(ProtocolViolation { code: 1 })` / sender
`WriteError::ConnectionLost(PeerClosed { code: 1, .. })`.

## Conflicts

### C1 — the brief's partition names a file that already exists (BLOCKING, resolved by renaming)

The brief grants me `src/core/connection/tests_reassembly.rs` as a **new
file** whose `mod` declaration is the integrator's job. At base that file is
**tracked, 1 176 lines, and already declared live**:

```
$ git ls-files src/core/connection/ | grep tests_reassembly
src/core/connection/tests_reassembly.rs
$ grep -n 'mod tests_reassembly' src/core/connection/mod.rs
135:mod tests_reassembly;
```

It also already contains ceiling tests (`tests_reassembly.rs:661-700`,
`:853`) that the implementer plausibly must touch when the ceiling becomes
credit-derived. Writing into it would hand two concurrent agents one path —
exactly rule 6's slice-2a accident. Per rule 5 I did not do what the brief
said; I report it and took the safest reading of its intent: a **new,
distinctly named** file, `src/core/connection/tests_reassembly_credit.rs`,
still with **no `mod` declaration of mine** (integrator's job, see "How I ran
them").

### C2 — not a conflict, but the sites ruling 270 must sweep (working rule 4)

I found **no** contradiction between §10.6's text, ruling 253's work bound
and the measured behaviour. Ruling 253's clause bounds *copy work* at
`O(credit · log credit)`; ruling 270 changes only how many ranges may be
*stored*, and the work clause is already stated over the credit, so raising
the range ceiling does not restate it. Reported as checked rather than as
silence.

What I did find is the rule-4 shape — text that will still argue the old
position after the code changes. These are the implementer's or integrator's
to dispose of, and I am reporting them, not resolving them:

* `SPEC.md:4418` — §10.6's own constants table: `| REASSEMBLY_CHUNKS_MAX |
  1024 stored discontiguous ranges per stream |`. After ruling 270 that is a
  **floor**, not the ceiling.
* `SPEC.md:7918` — the consolidated table repeats the same row verbatim,
  with `§10.6 — receiver policy, **externally observable** (ruling 103)`.
* `SPEC.md:4345` (§10.5's third violation) — *"**It is a tolerance and not
  an exact limit** — the sender cannot compute it, since it depends on this
  receiver's coalescing and on the arrival order the network produced"*.
  Ruling 270 makes a *lower bound* on it computable by the sender, from the
  credit it was granted and its own packet size. That strengthens the clause
  rather than contradicting it, but the sentence as written says the opposite
  of what the fix's soundness argument relies on.
* `SPEC.md:6941` (§ the memory-bound table) — reassembly metadata *"bounded
  by `REASSEMBLY_CHUNKS_MAX` (§10.6 — the second bound is what makes the
  credit term the dominant term rather than a 25–50× underestimate)"*. The
  conclusion survives a credit-derived ceiling (≈ `C/(2·MTU)` ranges at
  ~48 B each is a few per cent of `C`), but the *mechanism named* is a flat
  constant, and rule 11 is about mechanisms named in rationales.
* `SPEC.md:7424` (Appendix B) — *"a one-byte-frames-at-even-offsets flood
  stays O(credit) or dies at `REASSEMBLY_CHUNKS_MAX` with
  `PROTOCOL_VIOLATION`"*. Still true, and
  `a_tiny_fragment_flood_still_dies_at_a_raised_window` is written to keep
  it true; the constant named is again the floor.
* `SPEC.md:7889` — the O53b obligation *"before the §10.2 constants and
  `REASSEMBLY_CHUNKS_MAX` ratify"*, which round42-C reports as the live gate
  this whole slice discharges.
* `src/constants.rs:315–323` — the doc comment for the constant, whose
  "receiver policy, observable / it is a **tolerance**" reasoning is
  unchanged but whose subject is now a floor.
* `src/core/connection/recv.rs:17` and `:625`, `src/core/connection/flow.rs:8–11`
  — module-level prose naming the flat ceiling.

### C3 — the disposition question round42-C left open is answered by my brief

round42-C §3 ends with *"Open question the maintainer should rule on, not
the implementer: even with a raised ceiling, crossing it is dispositioned as
`CLOSE(PROTOCOL_VIOLATION)` — the receiver blames the peer for the network's
behaviour."* My brief states the kill disposition as **ratified and
unchanged** (§10.5/§10.6), so every abuse test here asserts exactly that
code, and no test asserts a softer disposition. Recorded so that the
integrator can see the question was answered rather than dropped.

### C4 — a behaviour worth knowing, found while writing the fixture

§15.3 answers **every** packet arriving after the CLOSE with another CLOSE.
A packed batch that straddles the death therefore emits `[1, 1]`, and
`testfix::assert_violation`'s *"exactly one CLOSE"* fails on a **correct**
build. `flood_until_death` is batched to one packet for that reason, stated
in its rustdoc. Any future flood-shaped test that reuses `assert_violation`
across multi-packet deliveries will hit this.

## Two dead ends, recorded because both produced tests that proved nothing

Rule 9's shape, twice, in the fixture rather than in the assertion — and
neither was visible from the test's own result:

1. **Arming the burst from the writer's loop.** Fires at exactly send index
   5 500 while the writer is flow-control-parked, and **never fires at all**
   at 16 MiB / 12 MiB, because a writer whose payload fits inside its credit
   never parks. The 16 MiB test passed at base with the burst never
   installed.
2. **Arming it from the reader's loop.** Always fires, but the reader is
   woken once per *driver turn* and a driver turn at this cwnd carries a
   whole flight: the first observation past 5 500 was **9 212**, by which
   point the transfer had too little left to run and the same burst was
   survivable at base — the 8 MiB story went green.

The fix is that a `drop_at` index set is a pure function of the wire's own
send counter, so the burst is installed **before the handshake** and needs no
observation at all. `Outcome::traversed()` (`a_sends >= 11500`) is what
converts "the burst was configured" into "the burst happened", and it is the
assertion both dead ends would have failed.

A third, smaller one: the writer originally owned `ca` in an `async move`,
so finishing the write dropped the last connection handle and §16.3 closed
the connection — `PeerClosed { code: 0 }` mid-transfer, a red for a reason
with nothing to do with §10.6.

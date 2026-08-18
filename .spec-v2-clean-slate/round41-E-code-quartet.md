# Round 41 — E: code quartet (items 6, 8, 9, 11)

Base: 94dab200a172a5b62b4d28711962574d61a9d3c8 (verified clean at start)
Worktree: /Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-ac0b05b198c77189a

## 0. Base verification

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-ac0b05b198c77189a
$ git rev-parse HEAD
94dab200a172a5b62b4d28711962574d61a9d3c8
$ git status --short
(empty)
```

## 1. Recorded wording (round41-material.md items 6, 8, 9, 11)

* **6** — "`tests_path.rs`'s second conflict header (`CONTRACT-7b.md` `!elicits`
  vs SS8.7's broader condition) still reads 'REPORTED AND NOT RESOLVED'; the code
  follows SS8.7 and its test is green. One line."
* **8** — "The pump loop's `contested.is_pending()` offer disjunct is
  unreachable (predates ruling 250; reported in place at the R40-B merge). Its
  comment reads as a live mechanism."
* **9** — "`copy_work()` summed over live halves is not monotone across half
  retirement (documented at the `Streams` level; no test depends on it). Decide
  whether the connection-level meter should survive retirement."
* **11** — "`tests_livelock.rs`'s fixed-point bound still says `cap = 64` — much
  looser than needed post-249/254 (mechanism comment already corrected at
  `96f7ef0`)." 

## 2. Item 8 — the pump's `contested.is_pending()` disjunct

### 2.1 The site

`src/core/connection/mod.rs:2271-2276`, inside `pump_packets`'s build loop:

```rust
let offer = owe_challenge
    && (self.owes_output()
        || self.contested.is_pending()      // <- the disjunct
        || self.ack.is_owed()
        || probe);
```

The comment above it (mod.rs:2209-2231) already says, under **[ruling 250]**,
that it "is **unreachable here** — reported rather than deleted", and a second
copy of the same claim sits in `pump_contested_probe`'s doc comment
(mod.rs:3213-3222). Round 41 item 8 is the instruction to spend that report.

**Not to be confused with the two live `is_pending()` sites:**
* `mod.rs:2958` — `!self.contested.is_pending() && self.amplification.admits(size)`,
  the keepalive gate; pinned by `tests_livelock.rs` (module doc, l.10 and
  `beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past`, l.443) and by
  `tests_path.rs:1435`. **Live — untouched.**
* `mod.rs:3198` — `let Contested::Pending { floor } = self.contested else ...`,
  `pump_contested_probe`'s own entry test. **Live — untouched.**
### 2.2 Measurement (instrumented, full suite)

Instrumentation added in the worktree (scratch only, removed before the commit):

* at the `offer` site in the loop — an `assert!(!self.contested.is_pending(), ...)`,
  an `ITEM8_PENDING_IN_LOOP` line if it is ever `Pending`, an
  `ITEM8_ARMED_IN_LOOP` line if it is `Armed`, and a per-500-iteration
  `ITEM8_STATS total=..` heartbeat proving the site executes;
* in `pump_contested_probe` — one line at entry-with-a-mark, and one at **each**
  of its five exits, so the branch partition is measured, not assumed.

```
$ cargo test --all-features -- --nocapture > item8-instrumented-2.log 2>&1; echo "EXIT=$?"
EXIT=0

$ grep -o 'ITEM8_[A-Z_]*' item8-instrumented-2.log | sort | uniq -c
  55 ITEM8_ARMED_IN_LOOP
  49 ITEM8_PROBE_ENTRY_PENDING
  17 ITEM8_PROBE_EXIT_NOROOM
  32 ITEM8_PROBE_SENT_ARMED
  74 ITEM8_STATS

$ grep -c ITEM8_PENDING_IN_LOOP item8-instrumented-2.log
0

$ grep -E '^test result:' item8-instrumented-2.log | awk '{s+=$4} END {print "total passed:", s}'
total passed: 1069
```

Read off:

* **0** loop iterations observed `Pending`, over **>=37 000** iterations of that
  exact line (74 heartbeats x 500; the largest single-binary total printed is
  `total=9500`). The `assert!` never fired — the suite is green **with** it in.
* The site is **not** vacuously exercised: 55 iterations observed `Armed`, so the
  loop does run on connections with a live contested mark; it only ever sees the
  post-transmission state.
* The fixture **does** reach `Pending` — 49 pump entries found a mark (rule 13's
  question answered: this is not an untested region). Their exits partition
  exactly, 17 + 32 = 49, over only **two** of the five branches:
  `!packing.ping()` (17, budget below the 31-byte probe -> `return false` ->
  the pump returns and the loop never runs) and the transmit (32, leaving
  `Armed`). `REFUSED` / `NOSESSION` / `NONCE` were never taken, and each of
  those three is also a `return false`.

Release, same instrumentation (`assert!` is live in release too):

```
$ cargo test --release --all-features > item8-instrumented-release.log 2>&1; echo "EXIT=$?"
EXIT=0
```
### 2.3 Mechanism (why no state reaches it)

Measurement alone is rule 13's weak half. The structural argument:

1. **`Contested::Pending` has exactly one writer.** `grep -rn 'Contested::' src/`
   gives one assignment of `Pending` — `mod.rs:3116`, in `mark_contested`. Every
   other write is `Contested::No` (`mod.rs:712` on the verdict timer, `1425` and
   `1436` on ACK coverage) or `Contested::Armed` (`mod.rs:3309`, the probe's
   transmit).
2. **That writer is never called from inside the pump.** `grep -rn
   'mark_contested' src/` gives one non-test caller, `shell/driver.rs:473`, via
   `deliver_to_core`. `mark_contested` **calls** `pump`; nothing in the pump
   calls back into it. So `self.contested` cannot change from `No`/`Armed` to
   `Pending` at any point between the pump's entry and the loop's `offer`.
3. **`pump_packets` runs `pump_contested_probe` before the loop is entered**
   (`mod.rs:2191-2193`) and it is total over the three cases:
   * not `Pending` -> `return true` (the `let ... else`, `mod.rs:3198`) — the
     loop is entered with `No` or `Armed`;
   * `Pending` and something refuses the send (no room for the PING, budget
     refuses, no session, nonce exhausted) -> `return false` -> `if
     !self.pump_contested_probe(..) { return; }` **returns from the whole pump**
     — the loop is never entered at all;
   * `Pending` and the send succeeds -> `self.contested = Contested::Armed`
     (`mod.rs:3309`) then `return true` — the loop is entered with `Armed`.

   There is no fourth case: the function's only paths to `true` are the
   `let ... else` and the transmit's tail, and the transmit's tail assigns
   `Armed` unconditionally before it.

Therefore `self.contested.is_pending()` is **constantly false** at
`mod.rs:2273`, on every iteration of every pump. Deleting it is
behaviour-preserving — not a behaviour change, which is what the comment at
`mod.rs:3219-3222` worried about ("removing a disjunct is a behaviour change")
while stating the unreachability proof two lines above it.

**Ruling 250 did not create this** — the comment says so and the claim checks
out (rule 11: read the artefact the claim is about). Ruling 250 (`rulings.md`
7325+) deleted the 212(c) pre-pass and made the probe coalesce; before it, the
probe still ran *above* the loop (as rank-2 pre-pass then probe), so the loop
still could not observe `Pending`. What 250 changed is where the state is
**served**: the challenge now rides the probe's own packet
(`pump_contested_probe` -> `pack_path_frames`, `mod.rs:3232-3236`), which is the
live mechanism the disjunct's comment should have been describing.
### 2.4 Sweep (rule 4): references to the disjunct

`grep -rn 'disjunct' src/ tests/ .spec-v2-clean-slate/rulings.md SPEC.md` — 18
hits. Triaged:

* **`mod.rs:2222-2231` and `mod.rs:3210-3222`** — the only two sites that argue
  *about this disjunct*. Both rewritten (2.5).
* `mod.rs:2233` (ruling 217, `ack.is_owed()`), `2248` (ruling 221, `probe`),
  `2375` (ruling 224's struck-through boundary 2, which cites `ack.is_owed()` as
  "a disjunct of `offer` above") — **other** disjuncts of the same expression,
  all still live and all still true after the edit. Untouched.
* `tests_livelock.rs:42, 291, 429, 535, 648, 689, 881, 893` and
  `tests_path.rs:1436` — every one of these is about the **keepalive refusal**
  `contested.is_pending() || !amplification.admits(30)` (`mod.rs:2958`), a
  different predicate that is still live. Untouched.
* `rulings.md:6298` — ruling 221's own text. Not a claim about this disjunct.

**No test name and no test comment references the offer disjunct.** The nearest
candidate, `tests_path.rs::a_pending_contested_probe_does_not_block_the_challenge`,
asserts over the packet **carrying the PING** — i.e. it pins
`pump_contested_probe`'s coalescing, the live mechanism, not the loop's
predicate. It stays green (2.5).

`grep -rn 'mark_contested' src/` also swept: one non-test caller
(`shell/driver.rs:473`); the rest are tests. Nothing in the pump.
### 2.5 Fix applied

`src/core/connection/mod.rs`:

```rust
// before
let offer = owe_challenge
    && (self.owes_output()
        || self.contested.is_pending()
        || self.ack.is_owed()
        || probe);
// after (mod.rs:2289)
let offer = owe_challenge && (self.owes_output() || self.ack.is_owed() || probe);
```

Both comment sites rewritten:

* the loop's block now records the deletion, dated, with the measurement and
  the one-writer argument, and then names the live mechanism — ruling 250's
  coalescing, with `owe_challenge` threaded through it so the loop does not
  offer a second challenge on the same pass;
* `pump_contested_probe`'s block no longer says the loop "names the state
  literally"; it says the disjunct is gone and that **this** is the site where
  SS8.7's condition is discharged for a connection carrying a mark.

Full suite green after the edit — see section 6's gate table.

**One thing I did not do, and why.** A `debug_assert!(!self.contested.is_pending())`
would pin the invariant the deletion relies on. I left it out deliberately:
round 41's own item 7 is a `debug_assert!` that fires on a reachable state and
panics debug drivers, resolved only yesterday (ruling 257). Converting a proof
into a panic site buys a guard against a refactor nobody is planning, at the
price of the exact failure the round just paid for. The proof is in the comment
instead, where the next refactor will read it. **Flagged as the maintainer's
call, not mine.**

## 3. Item 9 — `copy_work()` monotonicity across half retirement

### 3.1 What it is / where

* `Reassembly::copy_work` — the field and its doc, `recv.rs:596-597, 622-634`.
  Incremented at `recv.rs:778` (the arriving frame's bytes on store) and `:819`
  (bytes re-copied by a merge). **Monotone and never reset**, explicitly
  including `discard()` (`recv.rs:643-644`).
* `RecvHalf::copy_work` — `recv.rs:149-153`, passes it through.
* `Streams::reassembly_copy_work` — `streams.rs:364-378`: `entries.values()`,
  `filter_map(|s| s.recv.as_ref())`, `map(RecvHalf::copy_work).sum()`. The doc
  already states the non-monotonicity and calls it "the same caveat" as ruling
  94's capacity accessor.
* `Connection::reassembly_copy_work` — `mod.rs:950-964`, `pub(crate)`.
* The blind author's report of the question, verbatim, is at
  `tests_reassembly.rs:205-213`: *"'never reset' and 'sum the live halves'
  cannot both be true, and the integrator picks one."*
### 3.2 When the sum drops

**Exactly one line**: `streams.rs:1501`, `let Some(recv) = stream.recv.take()`
inside `retire_recv`. The `RecvHalf` is converted to a tombstone
(`streams.rs:1519`) and dropped; its `copy_work` leaves the sum at that instant.
A grep for `recv.take()` and `entries.remove` in `streams.rs` gives only that
line and `:1539`'s `entries.remove(&r)`, which cannot hold a live `recv` (it
runs only when `is_fully_closed()`).

`retire_recv` has **five** callers, i.e. five events that drop the sum:

| site | event |
|---|---|
| `streams.rs:484` | `claim_message` — SS9.8's message claim retires the half |
| `streams.rs:576` | `read` returning `ReadOutcome::End` — FIN fully read |
| `streams.rs:580` | `read` returning `ReadOutcome::Reset` |
| `streams.rs:601` | `abandon_recv` — SS16.2's dropped `RecvStream` (ruling 93) |
| `streams.rs:1312` | SS9.8's overflow reset, receiver-emitted |

Worth noting for the decision: the **first** of those is the message API. Every
`claim_message` retires a receive half, so on a message-shaped workload — the
headline SS9.8 verb — the connection-level meter returns to (near) zero
continuously.
### 3.3 Consumers today

`grep -rn 'reassembly_copy_work' src/ tests/ benches/`: `recv.rs` (the field),
`streams.rs` (the sum), `mod.rs` (the accessor), and `tests_reassembly.rs`.
**Nothing in `tests/`, nothing in `benches/`, nothing public.** The accessor is
`pub(crate)` and carries a working-rule-15 note naming its one caller.

**Does anything assert monotonicity?** No.

* `tests_reassembly.rs` asserts `copy_work` **deltas** — `== 0` for a covered
  frame, cumulative across a 64-iteration loop (`:320-323`) — and never retires
  or resets a half while measuring. Its own doc says so (`:209-212`).
* `recv.rs:1053`'s `a_reset_returns_the_capacity_and_not_the_copy_work` pins
  monotonicity **at the half**, which is where the ruling states it. Unaffected
  either way.

**What the ruling and the spec actually bound** (rule 11 — both opened):

* Ruling 253(i), `rulings.md:7525-7527`: *"total copy work **per stream** is
  O(credit x log credit)"*.
* SS10.6 as amended, `SPEC.md:4287-4292`: *"Coalesce-on-insert's total copy work
  **per stream** MUST be O(that stream's advertised credit x log credit)"*.

So the normative bound is **per stream**, and the per-half meter — monotone,
never reset — is the instrument for it. The connection-level sum is an
aggregate convenience, and SS10.6 carries a ratified warning (ruling 94's note,
`SPEC.md:4306-4308`) about exactly this hazard: *"the level this mandate is
stated at is not the level its own worked example computes"*.
### 3.4 Options (a) / (b)

**(a) Keep as-is, documented.** Zero code. The half-level meter keeps the
"never reset" promise where SS10.6 and ruling 253 state the bound; the
connection accessor is documented as a live-halves sum, exactly like
`reassembly_capacity`. The contradiction the blind author reported dissolves
by **scoping** rather than by changing anything: "never reset" is a claim about
`Reassembly`, "sum the live halves" is a claim about `Streams`, and both are
true at their own level. Cost: the wording at `streams.rs:364-370` and
`tests_reassembly.rs:205-213` should say *why* it is scoped that way, or the
next reader re-opens the same question.

**(b) Accumulate retired halves into a connection-level counter.** Sketch:

```rust
// streams.rs, in `Streams` — one field, zero-initialised in the constructor
    /// Ruling 253's copy work from halves that have since retired. Kept so
    /// the connection-level meter is monotone: a **work** total a peer has
    /// already spent is not handed back by the retirement it can trigger.
    retired_copy_work: u64,

// streams.rs, in `retire_recv`, immediately after `stream.recv.take()`
    self.retired_copy_work += recv.copy_work();

// streams.rs, in `reassembly_copy_work`
    self.retired_copy_work
        + self.entries.values()
            .filter_map(|s| s.recv.as_ref())
            .map(RecvHalf::copy_work)
            .sum::<u64>()
```

Borrow-check note: `stream` borrows `self.entries` only, so the `+=` on a
sibling field is fine; `recv.copy_work()` must be read before `recv` is moved
into `recv.tombstone()` at `:1519`. Diff: **~6 lines of code, ~10 of comment**,
in one file, plus a doc line at `mod.rs:950` and a rewrite of the caveat
paragraph at `streams.rs:364-370` and the reported question at
`tests_reassembly.rs:205-213`.

The test that would pin it (and it needs one — rule 9): deliver into a peer uni
stream until the meter is non-zero, record it, retire the half
(`abandon_recv`, or a `claim_message`), assert the meter is **unchanged**. It
separates: today's build returns 0 there. It belongs in `tests_reassembly.rs`
beside the `work()` helper whose doc raised the question.
### 3.5 Recommendation

**(b), the connection-level accumulation** — with the reason stated as a scope
argument rather than a tidiness one.

1. **The failure direction is the unsafe one.** Retirement makes the meter
   *under*-report work. An Appendix B work-bound test that happens to retire a
   half mid-workload reads a **better** ratio than the truth and passes on a
   build that has the defect — rule 9's trap, arriving through the fixture
   rather than through the assertion.
2. **The likeliest future workload retires on every unit of work.**
   `claim_message` (`streams.rs:484`) retires the receive half, so a work-bound
   test written over SS9.8 messages — slither's headline API, and S34's shape —
   measures approximately nothing. That is not a hypothetical class of test;
   ruling 253 amended Appendix B with a work-bound obligation, and the next
   author of one has no reason to suspect the meter.
3. **SS10.6 itself asks the two accessors to differ.** *"The mandate above
   bounds state; this clause bounds work"* (`SPEC.md:4287-4288`). Freed memory
   really is gone, so summing live halves is right for **capacity**; spent work
   is not given back, so the same aggregation is wrong for **work**. The code
   currently applies the state shape to the work meter, which is the one
   asymmetry the amended section explicitly draws.
4. **Cost and blast radius are near zero**: ~6 lines, `pub(crate)`, test-only
   consumers, no wire byte, no constant, no timer, no behaviour — and no
   existing test asserts the sum drops (3.3).

Against it, honestly: the normative bound is **per stream** (3.3), and a
monotone connection total is not the quantity ruled — a reader could take the
new accessor as evidence of a connection-level bound that does not exist. That
is a **documentation** risk, and the doc line that answers it is one sentence:
*this meter aggregates a per-stream bound; SS10.6 states no connection-level
ceiling, and a growing total is not a defect.* If the maintainer weighs that
risk above (1)-(3), option (a) with the scoping paragraph is a defensible
answer and costs nothing.

**Not implemented — this one is the maintainer's decision, per the brief.**

## 4. Item 11 — `tests_livelock.rs` fixed-point bound

### 4.1 Current bound and derivation

`drive_like_the_shell(s, start, cap)` (`tests_livelock.rs:587+`) walks the
shell's steps 4 and 5 at the core and panics on two things: a deadline at or
before the `now` that produced it, and `steps > cap`, whose message reads *"a
held keepalive must not be re-offered without bound"*. Two callers, both
passing the literal `64`:

* `a_connection_holding_its_beacon_still_reaches_a_fixed_point` (`:634`)
* `a_connection_holding_its_beacon_behind_a_mark_still_reaches_a_fixed_point`
  (`:666`)

Its stated derivation was not a derivation: *"The cap is deliberately loose:
the point is bounded, not small, and a tight cap would turn an unrelated
recovery-timer change into a red here."*
### 4.2 Recomputed tight bound

Measured, not argued (an `eprintln!` of `steps` at the loop's `Timeout(None)`
exit, run over the whole file, then removed):

```
$ cargo test --all-features --lib -- core::connection::tests_livelock --nocapture
ITEM11_STEPS=1
ITEM11_STEPS=1
test result: ok. 10 passed; 0 failed; ...
```

**Both tests reach the fixed point in 1 step.** Ruling 249 announces no `Pto`
at all while the budget refuses a probe, so the only timer left is SS7.4's
liveness: one sleep to `install + DEAD_TIMEOUT`, one `handle_timeout`, dead,
`Timeout(None)`.

New value: **`FIXED_POINT_CAP = 4`**, a named const above `drive_like_the_shell`
carrying the derivation, used at both call sites, plus a compile-time pin
`FIXED_POINT_CAP < DEAD_TIMEOUT / BEACON` so the upper end is derived from the
constants rather than transcribed.

```
$ cargo test --all-features --lib -- core::connection::tests_livelock
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 734 filtered out
```

Note on 254's direction, since the item names it: a **smaller** PTO cap means
**more** rungs inside `DEAD_TIMEOUT`, not fewer (~1.02 s doubling to 8x sums
1, 3, 7, 15, 23, 31 intervals, so 25 s falls between the fifth and sixth
firing: 6 steps at 2^3, 5 at the old 2^6). So 64 was never derived from the
ladder in either era. What 249 did was remove the ladder from these two tests
altogether, which is what makes 1 the honest number.
### 4.3 Separation check (rule 9)

The assertion itself referenced 64, so the question is whether the tightened
bound separates anything. **The old one did not.**

The build the cap's own message names — a held keepalive re-offered without
bound, re-armed one interval on rather than at a past instant, so the
retrospection assert above never sees it — scores `DEAD_TIMEOUT / BEACON`.
Measured directly, by driving an **un**held beacon on a validated address,
which is exactly that behaviour:

```
$ # scratch test, since removed
$ cargo test --all-features --lib -- scratch_unheld_beacon_step_count --nocapture
ITEM11_UNHELD_BEACON_STEPS=25
```

25 < 64. **No terminating build in this fixture can exceed 25 steps** — the
shortest timer in play is the 1 s beacon and death arrives at 25 s — so `cap =
64` was an upper bound the collapsed implementation satisfied for free: rule
9's trap, in the file whose own module doc teaches rule 9.

The tightened cap does fire, verified against that same shape:

```
$ # scratch test, since removed: the 25-step shape driven at FIXED_POINT_CAP
$ cargo test --all-features --lib -- scratch_separation --nocapture
thread '...scratch_separation_the_reoffering_shape_fails_the_new_cap' panicked at
  src/core/connection/tests_livelock.rs:646:9:
F1: the driver loop took more than 4 steps without parking or dying. A held
keepalive must not be re-offered without bound.
test result: FAILED. 0 passed; 1 failed
```

So: shipped build **1**, cap **4**, the failure the message names **25** — a
factor of six on the failing side and a three-step margin on the passing side,
which is the headroom the old comment wanted and did not buy honestly.

## 5. Item 6 — `tests_path.rs` second conflict header

**The claim, verified.** The header at `tests_path.rs:1243` disputes
`CONTRACT-7b.md` SS1.4's `!elicits` guard against SS8.7's broader condition, and
still read "REPORTED AND NOT RESOLVED".

* **SS8.7's condition** (`SPEC.md:3651+`, read at `:3686-3690`): `PATH_CHALLENGE`
  is *"owed for as long as the arming lasts: the sender re-emits it ...
  whenever SS7.3's budget admits a packet and the address is still
  unvalidated"*. No `elicits` term.
* **The code follows it.** The shipped predicate is
  `let offer = owe_challenge && (self.owes_output() || self.ack.is_owed() ||
  probe);` (`mod.rs:2289`) — no `!elicits`. The only `!elicits` left in the
  pump is `mod.rs:2447-2448`, `let elicits = ...; if probe && !elicits {
  packing.ping(); }`, which is SS13.4's bare-PING rule for a **PTO probe**, not
  a gate on the challenge.
* **The record says so too** (rule 11 — rulings opened, not paraphrased):
  ruling 224 (`rulings.md:6452+`) rules *"SS8.7 outranks the comment"* and
  strikes boundary 2 through in place, noting ruling 217 added `ack.is_owed()`
  to the offer and thereby reversed it; ruling 221 (`rulings.md:6264+, 6298`)
  adds `probe` as the fourth disjunct. `mod.rs:2665-2670` already carried the
  conclusion in code — *"what 217 correctly settles is the ride-along — SS8.7's
  condition, not the contract's `!elicits`"* — with nothing linking it to the
  header.
* **The tests are green:**

```
$ cargo test --all-features --lib -- \
    core::connection::tests_path::a_packet_leaving_for_an_unvalidated_address_carries_the_challenge \
    core::connection::tests_path::a_ninety_byte_budget_still_lets_the_address_validate \
    core::connection::tests_path::an_idle_unvalidated_connection_owing_nothing_emits_no_challenge --exact
running 3 tests
test ...an_idle_unvalidated_connection_owing_nothing_emits_no_challenge ... ok
test ...a_packet_leaving_for_an_unvalidated_address_carries_the_challenge ... ok
test ...a_ninety_byte_budget_still_lets_the_address_validate ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 741 filtered out
```

**Written.** A dated resolution paragraph is inserted immediately under the
warning line, in the same house style as this file's *first* conflict header
(which already carries one): the report below it is kept verbatim, the
resolution lists 217 / 224 / 221 and the two code sites, records that boundary
1 survives only as the ban on manufacture — which
`an_idle_unvalidated_connection_owing_nothing_emits_no_challenge` still pins —
and closes on the author's own line, *"the call is the maintainer's"*, which
the call upheld. Nothing erased.

## 6. Landing: commit + gates

**Commit `e4d1c14`**, one commit atop `94dab20`, three files, no scratch and
no instrumentation:

```
$ git log --oneline -2
e4d1c14 Round 41 items 6, 8, 11: the pump's dead disjunct, and two test-file records
94dab20 Ruling 257: the bare FIN defers when no frame fits

$ git show --stat HEAD
 src/core/connection/mod.rs            | 74 ++++++++++++++++++++-----------
 src/core/connection/tests_livelock.rs | 49 +++++++++++++++++++---
 src/core/connection/tests_path.rs     | 26 ++++++++++++
 3 files changed, 119 insertions(+), 32 deletions(-)

$ git show HEAD | grep -c 'ITEM8\|ITEM11\|SCRATCH\|eprintln'
0
$ git status --short
(empty)
```

Item 9 lands nothing — the decision is the maintainer's.

### One more rule-4 catch, found after the first commit and amended in

The deletion made a **count** stale: `mod.rs:2267` read *"**[RATIFIED —
ruling 221]** `probe` is the fourth disjunct"*, and after removing the dead
one it is the third of three. Rule 4's own shape — the token was changed and a
neighbouring sentence still described the old arrangement. Fixed by quoting
ruling 221's ordinal rather than silently renumbering it, so the record and
the code both stay readable. (Swept for the same shape elsewhere: *"the
challenge's two gates"* is still two, and ruling 224's *"four boundaries"* is
about boundaries, not disjuncts.)

### Gate table, run on `e4d1c14`

| Gate | Command | Result |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | `Finished dev profile ... in 2.04s` |
| Format | `cargo fmt --all --check` | exit 0, no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | `Finished dev profile ... in 1.27s`, zero warnings |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | `Generated .../target/doc/slither/index.html` |
| Docs (all features) | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | `Generated .../target/doc/slither/index.html` |
| Tests | `cargo test` | 5 binaries, **881 passed, 0 failed** |
| Tests (all features) | `cargo test --all-features` | 22 binaries, **1069 passed, 0 failed**, 1 ignored |
| Release tests | `cargo test --release --all-features` | 22 binaries, **1070 passed, 0 failed** |
| Wire pins | `cargo test --all-features --test spec_constants --test spec_packet --test story_codec` | 112 / 4 / 4 passed, 0 failed |
| MSRV | `cargo +1.96 check --all-features --all-targets` | `Finished dev profile ... in 0.59s` |
| Supply chain | `cargo deny check` | `advisories ok, bans ok, licenses ok, sources ok` |

(The last two are outside the brief's list and were run anyway; both green.)

Raw logs kept beside this file: `item8-instrumented-debug.log`,
`item8-instrumented-2.log`, `item8-instrumented-release.log`.

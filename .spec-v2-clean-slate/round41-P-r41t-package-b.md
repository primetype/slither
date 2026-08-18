# Round 41, report P — slice R41-T package B (endpoint-core carried gaps)

*Author: Opus agent, worktree cut from `980cd13` (verified first act);
commit `e4e3705`, cherry-picked to main as `6e292a0`. Its conflict C1
became ruling 268; its C2 note was reviewed and closed no-change.
Verbatim package report below. Independent mutation verification:
`round41-Q`.*

---

# Slice R41-T — Package B report

Base commit (verified, rule 14): `980cd131cd8dc5c3ee1635cab687573dd905ca91`
(`980cd13 Ruling 259(viii) record: the config-raisable windows enter §10.2, ...`)

File partition (absolute): `src/core/endpoint/tests.rs`, `src/core/tests.rs`, this report.
No production code changed.

## Items

- Item 2 — O13: post-mortem pin vs the LRU flush (`src/core/endpoint/tests.rs`)
- Item 4 — `authenticate()` idempotency (Proven arm, ruling 267)
- Item 5 — F2's one-token fix (`src/core/tests.rs`)

All three landed. **Status: item 2 DONE but by a different mechanism than the
brief specified — see conflict C1, which is a defect in `SPEC.md`'s own
Appendix B parenthetical, reported and not resolved.**

## Tests added

| # | Test | File:line |
|---|---|---|
| 2 | `the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush` (+ helper `filler_static`, `:1654`) | `src/core/endpoint/tests.rs:1513` |
| 4 | `a_second_authenticate_is_idempotent_at_zero_incremental_dh` | `src/core/endpoint/tests.rs:933` |
| 5 | *(no new test)* — two tokens changed in `the_per_source_cap_is_four_chains_per_ip` | `src/core/tests.rs:1123`, `:1147` |

Lib test count 754 → 756. One import added
(`TS_GUARD_ORPHAN_CAP`, `src/core/endpoint/tests.rs:60`).

**Preconditions asserted**, **mutations separated**, **gates**, **conflicts**
and the working log follow per item below; the cross-cutting sections are
"Conflicts found", "Gates run", "Timings" and "Summary table — one mutant per
test" at the end.

---

### Item 5 — DONE (F2's one-token fix), `src/core/tests.rs`

Applied exactly the triage's verified fix, nothing else. Two tokens, both
inside `the_per_source_cap_is_four_chains_per_ip`:

```diff
-        let now = t + Duration::from_secs(n as u64);
+        let now = t + Duration::from_secs(n as u64 + 1);
...
-    let now = t + Duration::from_secs(INTRO_MAX_PER_SOURCE as u64);
+    let now = t + Duration::from_secs(INTRO_MAX_PER_SOURCE as u64 + 1);
```

POINTER DRIFT (not a conflict, recorded for the integrator): the brief and
the triage both name lines **1122** and **1147**; at `980cd13` the first is
line **1123** (the second, 1147, is exact). Both `sed` addresses were
verified by printing the changed lines. Note also `Duration::from_secs(n as
u64)` appears at 1123/1178/1221/1320/1386/1481 — only 1123, inside the named
test, was touched.

Mechanism re-verified at `980cd13` (not taken from the triage):
- M4c site is `src/core/endpoint/intro_queue.rs:320`
  `match self.oldest_unconsumed(Some(source_key)) {`
  (the triage text says `intro_queue.rs:248`, also drift; the global-tier
  call is a separate site at `:340`).
- Tie-break: `oldest_unconsumed` at `:462`, `.min_by(|a, b|
  a.age_key().cmp(&b.age_key()).then_with(|| a.id.cmp(&b.id)))` at `:466`.
  So before the fix `other` (parked at `t`) and `ids[0]` (parked at `t+0s`)
  shared an `age_key` and the `IntroId` tie-break picked `ids[0]` under both
  the correct and the mutated build. After the fix `other` is *strictly* the
  global oldest.

Test still green:

```
$ cargo test --all-features the_per_source_cap_is_four_chains_per_ip
running 1 test
test core::tests::the_per_source_cap_is_four_chains_per_ip ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 753 filtered out; finished in 0.00s
```

**Mutation separation verified by actually applying M4c** (temporary edit to
`intro_queue.rs:320`, reverted with `git checkout` immediately after — that
file is not in my partition and I had no uncommitted work in it, rule 10):

```
$ sed -i '' '320s/self.oldest_unconsumed(Some(source_key))/self.oldest_unconsumed(None)/' src/core/endpoint/intro_queue.rs
$ cargo test --all-features the_per_source_cap_is_four_chains_per_ip
test core::tests::the_per_source_cap_is_four_chains_per_ip ... FAILED
thread '...' panicked at src/core/tests.rs:1152:5:
assertion `left == right` failed: the per-source cap evicted the wrong number of entries
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 753 filtered out
$ git checkout src/core/endpoint/intro_queue.rs   # reverted, verified line 320 restored
```

Red at `src/core/tests.rs:1152` — the exact line the triage predicted. (The
first assertion to fire is the count check at :1152 rather than
`b.present(other)` at :1161, because losing `other` also leaves all four
same-source entries present. Both assertions now depend on the scoping.)

---

### Item 4 — DONE (`authenticate()` idempotency, ruling 267)

**Test:** `a_second_authenticate_is_idempotent_at_zero_incremental_dh`
(`src/core/endpoint/tests.rs:899-975`), placed immediately after the ruling-74
test `a_second_read_identity_after_the_static_became_pending_does_not_intercept`
(now `:865-897`).

File choice: `src/core/endpoint/tests.rs`, not `src/core/tests.rs`. Both were
in my partition; the endpoint fixture reaches `authenticate()` just as
directly (`a_demoted_intro_keeps_section_6_1s_cumulative_ladder`, `:662`, calls
it and asserts `local.dh() == 2`) **and** additionally offers
`peer.nth_timestamp(n)`, which predicts the initiation timestamp exactly — so
the timestamp half can be pinned to an absolute value rather than only to
"the same as last time". `src/core/tests.rs`'s `pair()` fixture has no such
predictor (`authenticate_costs_two_dh_cumulative` at `:709` discards the
timestamp as `_ts`). Placing it beside the ruling-74 test was also the brief's
preference.

Spec text read (targeted, `SPEC.md:1170-1191`) — §6.1's rule list, verbatim
first bullet:

> - **`read_identity()` is idempotent.** A second call on an already
>   `Claimed` or `Proven` chain returns the revealed static and pays **0
>   DH** — it opens no provider. The ladder therefore holds under any number
>   of calls (ruling 74).
> - **`authenticate()` advances a still-parked chain**, driving the skipped
>   `es` itself and landing on exactly **2 DH cumulative**. …(ruling 75).

The list has three bullets and none of them states `authenticate()`
idempotency — rule 8's shape exactly (a stated construction with an unwritten
scope). The rustdoc cites **ruling 267** as instructed and notes the clause
lands at integration, not from me.

**Preconditions asserted** (all before the second call, so the postconditions
cannot hold vacuously):
- `local.dh() == 2` — §6.1's cumulative ladder; also proves the DH counter is
  *live* in this run, so the later `== 0` is not a dead-counter reading (rule
  9: the degenerate "counter never moves" build fails here).
- `first_peer.as_ref() == peer.canonical()` — it proved the right static.
- `first_ts == peer.nth_timestamp(1)` — the initiation's own timestamp, an
  absolute pin, not a self-comparison.
- `local.present(id)` — the chain is staged, i.e. the second call has
  something to be idempotent *about*.

**Postconditions:** same peer, same timestamp, `local.dh() == 0` after
`reset_dh()`, `drain()` silent, chain still present.

**Mutation (a) — Proven arm deleted — VERIFIED RED.**

```
$ sed -i '' '492,496d' src/core/endpoint/staged.rs     # deletes the ChainState::Proven arm
$ cargo test --all-features a_second_authenticate_is_idempotent
test core::endpoint::tests::a_second_authenticate_is_idempotent_at_zero_incremental_dh ... FAILED
thread '...' panicked at src/core/endpoint/tests.rs:965:10:
ruling 267: authenticate() is idempotent at Proven: Expired
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 754 filtered out
```

**The audit's "unobserved" claim independently confirmed.** The whole lib
suite under the same mutant:

```
$ cargo test --all-features --lib
    core::endpoint::tests::a_second_authenticate_is_idempotent_at_zero_incremental_dh
test result: FAILED. 754 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.38s
```

**754 passed, 1 failed** — the new test is the *only* observer of the deleted
arm in the entire suite. Reverted with `git checkout src/core/endpoint/staged.rs`
(that file is not in my partition and held no uncommitted work of mine, rule 10);
lines 492-496 verified restored.

**Mutation (b) — the arm re-drives the DH — argued, not synthesised.** A
faithful (b) mutant has to *add* an `ss` rather than delete a token, so it is
not a deletion I could apply and revert cleanly. What the test guarantees
instead: `local.dh()` is the fixture's provider-open counter
(`Ep::dh` → `self.dhs.get()`, `src/core/endpoint/tests.rs:281`), and the same
test reads it as **2** three lines before it reads it as **0** — so the
`== 0` assertion is over a demonstrably live counter, and any incremental
provider open on the second call moves it. Flagged here rather than claimed as
measured.

---

### Item 2 — DONE, but **NOT by the briefed mechanism** (see the conflict below)

**Test:** `the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush`
(`src/core/endpoint/tests.rs:1433-1651`, fn at `:1513`), placed immediately
after `losing_the_internal_tie_break_pins_the_record_against_orphan_aging`
(`:1414`) — the candidate test the triage identified as unextendable. Plus one
private helper `filler_static(i)` at `:1654`.

`TS_GUARD_ORPHAN_CAP` added to the `use crate::constants::{…}` list; it was
not previously imported into this file, which is consistent with the triage's
finding that the constant is exercised behaviourally nowhere.

#### The test, step by step

1. `sides(now, false)` + `crossing(...)` — local is the §6.7 **loser**, so
   feeding the crossing msg1 takes §6.6 step 4's admit path
   (`record_tiebreak_timestamp`, `routing.rs:634-650`), which is one of the
   two writes §17.1's bullet names. It records permanently, calls
   `arm_guard_exemption`, and pins.
2. Preconditions before the death: `installs() == [conn]`,
   `greatest == Some(peer.nth_timestamp(1))`, `guard_pins == 1`.
3. `handle_connection_event(now, conn, Retired { our_index })` — the
   connection dies at `now`, and `mod.rs:947` turns the armed flag into
   `exempt_until = now + HANDSHAKE_GIVEUP`.
4. **The three preconditions that make this a test of `exempt_until` and not
   of anything else:**
   - `guard_pins == 0` — so `age_deadline`'s `pins > 0` early return and
     `pinned`'s `pins > 0` half are both out of play, and `exempt_until` is
     the *only* thing that can protect the entry.
   - `greatest` still `Some(...)` — the record outlived the connection.
   - `d.deadline == Some(now + HANDSHAKE_GIVEUP)` — a **direct reading of the
     stamp**. `age_deadline` is `max(orphaned_at + TS_GUARD_ORPHAN_TTL,
     exempt_until)`; 90 s can only be the second term, so a build that wrote
     no exemption announces `now + 15 s` here and dies at this line.
5. **Aging half:** `timeout(now + TS_GUARD_ORPHAN_TTL + 1ns)` → `greatest`
   still `Some`, deadline still `now + 90 s`.
6. **LRU half:** 1024 fillers via `local.ep.guard.record(...)` at strictly
   increasing instants **after** the exempt entry's `last_admitted` (= `now`),
   so the exempt entry is the LRU-**oldest** and would be victim number one.
   - low side of the cap: `filler_static(0)` still present at exactly
     `TS_GUARD_ORPHAN_CAP` unpinned entries;
   - the 1025th filler: `filler_static(0)` **gone** (the flush really ran) and
     `filler_static(1)` present (exactly one victim, not a stampede);
   - `greatest(peer)` still `Some` — the obligation's LRU half.
7. **Inside the horizon** (`now + 90 s − 1 s`): replay the captured msg1 from a
   fresh source, `authenticate()` → `Err(AuthError::Replay)`.
8. **Past the horizon** (`now + 90 s + 1 ns`): `timeout()` → `greatest ==
   None`, and the same captured msg1 replayed again **authenticates `Ok`** —
   §6.7's conditional bound, both sides pinned, which is what Appendix B
   demands ("the test must pin **both** sides of the horizon").

#### Mutations — measured, not asserted

| Mutation | Applied | Result |
|---|---|---|
| **(a)** `GuardEntry::pinned` loses its `exempt_until` half (`guard.rs:170` → `self.pins > 0`) | yes | **RED** at `tests.rs:1607` "the exempt entry survived a full LRU flush". Whole lib suite: **755 passed, 1 failed** — sole observer |
| **(b)** `evict_if_over_cap` early-returns (`guard.rs:484` + `return;`) | yes | **RED** at `tests.rs:1597` "the flush did not happen". Whole lib suite: **755 passed, 1 failed** — sole observer |
| **(c)** `age_deadline` drops its `exempt_until` maximum (`guard.rs:200` guard falsified) | yes | **RED** at `tests.rs:1549` (the deadline precondition). Suite: **754 passed, 2 failed** — also caught by the pre-existing `core::endpoint::routing::tests::a_winner_side_record_outlives_its_dial_by_the_giveup` |
| **(d)** the cap's value | argued | both sides asserted (nothing evicted at `CAP`, exactly one at `CAP+1`); a smaller cap fails the low side, a larger one the high |
| **(e)** an infinite / unconditional exemption | argued | caught at the step-4 deadline precondition (`== now + 90 s` exactly) before the horizon assertions are reached |

(a) and (b) are the two the brief names, and **each is a sole observer across
all 756 lib tests** — i.e. `TS_GUARD_ORPHAN_CAP` and the LRU half of the
post-mortem pin genuinely had zero behavioural coverage before this test, as
the triage said. (c) shows the *aging* half was already covered, which is why
the triage classified only the flush half as NONE.

Console evidence:

```
$ sed -i '' '170s/.*/        let _ = now; self.pins > 0/' src/core/endpoint/guard.rs
$ cargo test --all-features --lib
thread 'core::endpoint::tests::the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush' panicked at src/core/endpoint/tests.rs:1607:5:
assertion `left == right` failed: §17.1: the exempt entry survived a full LRU flush, as the oldest entry in the tier
test result: FAILED. 755 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.46s

$ git checkout src/core/endpoint/guard.rs
$ sed -i '' '484s/.*/    fn evict_if_over_cap(&mut self, now: Instant) { let _ = now; if true { return; }/' src/core/endpoint/guard.rs
$ cargo test --all-features --lib
thread '...the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush' panicked at src/core/endpoint/tests.rs:1597:5:
the flush did not happen — nothing below is a test of surviving it
test result: FAILED. 755 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.44s

$ git checkout src/core/endpoint/guard.rs
$ sed -i '' '200s/.*/            Some(until) if false && until > base => until,/' src/core/endpoint/guard.rs
$ cargo test --all-features --lib
thread 'core::endpoint::routing::tests::a_winner_side_record_outlives_its_dial_by_the_giveup' panicked at src/core/endpoint/routing.rs:1775:9:
thread '...the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush' panicked at src/core/endpoint/tests.rs:1549:5:
assertion `left == right` failed: §17.1: the exemption is stamped at death + HANDSHAKE_GIVEUP, ...
test result: FAILED. 754 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s

$ git checkout src/core/endpoint/guard.rs        # reverted; lines 199-202 verified restored
```

`guard.rs` is not in my partition, held no uncommitted work of mine, and was
reverted with `git checkout` after each of the three (rule 10).

---

## Conflicts found

### C1 — RULE 5 / RULE 3. The briefed fill mechanism for item 2 cannot work, and it is the **spec's own** parenthetical that is wrong

My brief, the triage (`round41-H-audit-triage.md:1.1`), and **`SPEC.md`'s
Appendix B obligation O13 itself** all specify the same way to flush the guard
tier:

> `SPEC.md:7355-7361` — "**The post-mortem pin** (§17.1, ruling 37): an entry
> written by a tie-break admission or a winner-side record survives orphan
> aging **and** a full LRU flush (**≈ 1024 authenticate-then-drop statics**)
> for `HANDSHAKE_GIVEUP` after its connection dies…"

Brief: *"(2) authenticate-then-drop 1024+ DISTINCT statics (fresh key each
iteration, no accept()) to push the unpinned tier over cap"*.

**Authenticate-then-drop creates no guard entries at all**, so no number of
them can push the tier over cap. §17.1 mitigation (i) hands every chain a
`GuardUndo` at `authenticate()` and `TimestampGuard::revert`
(`src/core/endpoint/guard.rs:303-331`) **removes** an entry the record
created. The code says so in terms, in `revert`'s own comment
(`guard.rs:317-322`):

> Without this, a second pin (another chain for the same static, or a
> concurrent dial) makes the **authenticate-then-drop flood mint orphans after
> all, which is the exact attack mitigation (i) forbids**.

I did not resolve this by reasoning. **Measured** (throwaway probe, since
deleted):

```
after authenticate: greatest=Some(Timestamp { secs: 1700000000, nanos: 500 }) pins=1
after reject:       greatest=None pins=0
```

So the obligation's parenthetical describes an attack that another clause of
the same section explicitly defeats. **Reported, not resolved** — I did not
touch `SPEC.md`, and I am not proposing a wording. Two readings are available
and it is not mine to pick:
- the parenthetical is stale, written before mitigation (i), and the intended
  meaning is just "≈ 1024 entries' worth of LRU pressure, however produced";
- or "authenticate-then-drop" is loose shorthand for the
  authenticate-**then-accept**-then-retire cycle, which does leave a permanent
  record.

Either way the *obligation* — survive a full LRU flush — is testable, and this
is a rule-8 shape (a stated construction whose scope contradicts the mechanism
it names).

**What I did instead**, and why it is not a weaker test: the tier is filled by
calling `TimestampGuard::record` directly on the endpoint's guard
(`local.ep.guard`, reachable because `core::endpoint::tests` is a descendant
module of `core::endpoint` — **no visibility change was needed, no production
code was touched**). `record` is *the admission primitive* and the **sole
caller of `evict_if_over_cap`** (`guard.rs:290`), so 1025 distinct statics
through it **is** a full LRU flush by definition, and the two mutants the brief
names both go red under it (measured above). It also costs no fictional
crypto: the whole test runs in **0.06 s**, which moots the brief's timing
caveat.

Note also a fixture obstacle that would have blocked the literal route anyway:
`Ep::new` takes `key_seed: u8`, so the fixture can express at most **256**
distinct identities, not 1025.

### C2 — OBSERVATION (not a conflict). "Demotes to an ordinary orphan" is immediate death, not a fresh 15 s

§17.1 (`SPEC.md:6717-6718`): *"Only when the extension lapses does the entry
demote to an ordinary orphan and enter the LRU below."* Appendix B: *"then
demotes to an ordinary orphan and ages normally."*

A reader could take that as granting `TS_GUARD_ORPHAN_TTL` **from the lapse**
(re-admission at 105 s). The code re-admits at **90 s + ε**, because
`age_deadline` is `max(orphaned_at + TS_GUARD_ORPHAN_TTL, exempt_until)` and
`orphaned_at` is the *death* instant (ruling 73's single source of truth) — so
on lapse the entry is an ordinary orphan that is already 90 s old and dies at
the very next sweep. Appendix B's own acceptance criterion is "replay it after
the 90 s and assert the documented re-admission", which the code satisfies, and
the alternative reading would need `orphaned_at` re-stamped at the lapse,
contradicting ruling 73. **I read the code as correct and assert 90 s + ε**;
flagged only so the 105 s reading is on the record as considered and rejected,
with the reason.

### C3 — POINTER DRIFT (harmless, for the integrator)

| Cited in brief / triage | Actual at `980cd13` |
|---|---|
| `src/core/tests.rs:1122` (item 5, first token) | `:1123` |
| `intro_queue.rs:248` (triage, M4c site) | `src/core/endpoint/intro_queue.rs:320` |
| triage: mutated test "failed at `src/core/tests.rs:1152`" | confirmed exact |
| brief: F2 cross-source assertion at `:1161` | `b.present(other)` is at `:1161`; the *first* assertion to fire under M4c is the count at `:1152` |
| `guard.rs:419-451` (`extend_exemption`) | `:444` (fn), doc block from `:419` — consistent |
| `guard.rs:168-171` (`pinned`) | exact |
| `guard.rs:194-203` (`age_deadline`) | exact |
| `guard.rs:479-509` (`evict_if_over_cap`) | `:484-511` |
| `constants.rs:437` (`HANDSHAKE_GIVEUP`), `:478` (`TS_GUARD_ORPHAN_CAP`) | not re-checked line-exact; values confirmed behaviourally (90 s / 1024) |
| `staged.rs:492-496` (Proven arm) | exact |

Nothing here changed a decision; recorded because rule 11 makes a citation a
claim about the cited text.

---

## Gates run

All from `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a65208dcc2eec9c5b`,
on the final tree (all three items landed, all mutations reverted).

```
$ cargo fmt --all --check
$ echo $?
0
```
(`cargo fmt --all` was run once, after the tests were written; it reflowed two
call sites inside my own new tests and touched **only** my two partition files
— `git diff --stat` shows `src/core/endpoint/tests.rs` and `src/core/tests.rs`
and nothing else.)

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
$ echo $?
0
```

```
$ cargo test --all-features
test result: ok. 756 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.42s   (lib)
test result: ok. 2 passed; 0 failed; 1 ignored; …
test result: ok. 24 passed; 0 failed; …
test result: ok. 112 passed; 0 failed; …
test result: ok. 11 passed; 0 failed; …
test result: ok. 4 passed; 0 failed; …
test result: ok. 5 passed; 0 failed; …
test result: ok. 12 passed; 0 failed; …
test result: ok. 20 passed; 0 failed; …
test result: ok. 4 passed; 0 failed; …
test result: ok. 10 passed; 0 failed; …
test result: ok. 12 passed; 0 failed; …
test result: ok. 7 passed; 0 failed; …
test result: ok. 6 passed; 0 failed; …
test result: ok. 12 passed; 0 failed; …
test result: ok. 16 passed; 0 failed; …
test result: ok. 15 passed; 0 failed; …
test result: ok. 16 passed; 0 failed; …
test result: ok. 1 passed; 0 failed; …
test result: ok. 4 passed; 0 failed; …
test result: ok. 0 passed; 0 failed; 1 ignored; …
test result: ok. 17 passed; 0 failed; …   (story_reliability)
test result: ok. 6 passed; 0 failed; …    (story_streams)
test result: ok. 6 passed; 0 failed; …    (story_tower)
test result: ok. 13 passed; 0 failed; …   (doc-tests)
```
Zero `FAILED`, zero `error`. Lib count **754 → 756**: exactly the two new
tests (item 5 modified an existing test rather than adding one).

```
$ cargo test           # default features, the other half of the project's Tests gate
(no "FAILED", no "error"; 5 "test result: ok" lines)
```

The three touched tests, named explicitly:

```
$ cargo test --all-features -- --exact \
    core::endpoint::tests::the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush \
    core::endpoint::tests::a_second_authenticate_is_idempotent_at_zero_incremental_dh \
    core::tests::the_per_source_cap_is_four_chains_per_ip
running 3 tests
test core::tests::the_per_source_cap_is_four_chains_per_ip ... ok
test core::endpoint::tests::a_second_authenticate_is_idempotent_at_zero_incremental_dh ... ok
test core::endpoint::tests::the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 753 filtered out; finished in 0.06s
```

**NOT run** (not in my brief's gate list, and none of them can be affected by
`#[cfg(test)]`-only changes): `cargo build`, `cargo doc`, `cargo test --release
--all-features`, MSRV `cargo +1.96 check`, `cargo deny check`. Flagged rather
than claimed (rule 7).

## Timings

| Thing | Time |
|---|---|
| item 2's test alone | **0.06 s** — it does no bulk crypto; 1025 `record()` calls are ~15 ms |
| all three touched tests | 0.06 s |
| full `--all-features` lib suite | 1.42 s |

The brief anticipated "a few seconds" for 1024 X25519 authenticate/drop cycles.
That cost does not arise: see conflict **C1** — that route creates no entries,
and the flush is driven through the guard's own admission primitive instead.

## Summary table — one mutant per test

| Test | File:line | Mutation it separates | Verified |
|---|---|---|---|
| `the_post_mortem_pin_survives_orphan_aging_and_a_full_lru_flush` | `src/core/endpoint/tests.rs:1513` | (a) `pinned()` loses `exempt_until`; (b) `evict_if_over_cap` early-returns | **both measured, each a sole observer in 756 tests**; (c) also caught |
| `a_second_authenticate_is_idempotent_at_zero_incremental_dh` | `src/core/endpoint/tests.rs:933` | (a) `Proven` arm deleted; (b) the arm re-drives the DH | (a) **measured, sole observer in 755 tests**; (b) argued |
| `the_per_source_cap_is_four_chains_per_ip` (modified) | `src/core/tests.rs:1117` | M4c: `oldest_unconsumed(Some(source_key))` → `(None)` | **measured red** at `:1152` |

## Blocked items

None. No production code change was needed for any of the three; the guard
access item 2 needed was already visible to the test module.

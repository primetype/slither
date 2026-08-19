# SECV5 hygiene slice — test-author report

Base commit: `f703283` (verified by `git log --oneline -1` as first act).
Branch: `worktree-agent-a585c41106ee57feb`.
Spec brief: `.spec-v2-clean-slate/round43-D-hygiene-measurer.md` (§3.3, §3.4, C6).

## 0. Base verification

```
$ git log --oneline -1
f703283 Gap slice: the seven story-coverage gaps closed — S8, S10, S22c4, S24, S25, S30
$ git branch --show-current
worktree-agent-a585c41106ee57feb
$ git status --short
(clean)
```

Base is the briefed commit `f703283`; **no reset was needed**. The cut is
correct (working rule 14).

## 1. Inputs read

Targeted reads only; `SPEC.md` and `rulings.md` were never read whole
(working rule 1).

- `.spec-v2-clean-slate/round43-D-hygiene-measurer.md` §3 (`:1002-1226`) and
  §4 (`:1227-1310`) — the drafts, C6, and D's SECV5-8 recommended shape.
- `SPEC.md:7620-7655` — the "liveness anchor" bullet (§7.4) carrying the
  SECV5-8 parenthetical at `:7643`.
- `src/constants.rs:509-512`, `:617-618` — `KEEPALIVE_TIMEOUT` 10 s,
  `DEAD_TIMEOUT` 25 s.
- `src/testutil/mod.rs:490-570` — `block_path` / `heal_path` / `tap`.
- `tests/story_keepalive.rs` — header (`:1-163`), fixture (`:164-355`), and
  the S5 dance tests (`:389-560`) for house style.

## 2. Work item 1 — SECV5-8 (both-directions keepalive loss + heal)

### 2.1 Spec basis

`SPEC.md:7643`, a parenthetical inside §7.4's "liveness anchor" bullet at
`:7625` — quoted in full in the test's own doc comment. The tag is a
*clause*, not a bullet, so the doc comment claims only the clause.

**Landed:** `tests/story_keepalive.rs`, at the end of the S5 section —
`s5_both_directions_losing_one_keepalive_kills_both_sides_even_after_the_path_heals`.
Ports 7071/7072 (unused in that file). **No `Cargo.toml` change**: the file
already has a `[[test]]` stanza with `required-features = ["test-util"]`
(`Cargo.toml:277-278`) and this test is inside it — which is also why the
feature-less `cargo test` does not run it, as it did not before.

### 2.2 Test shape

Paused clock, `LocalSet`, house style, no `sleep`. Establish → one datagram
starts §7.5's dance → two intervals of dance with **both** peers' emissions
counted off the `Tap` → `block_path` in both directions → one
`KEEPALIVE_TIMEOUT` + 1 s inside the blackhole → `heal_path` both ways →
both `ca` and `cb` must resolve `ConnectionLost::TimedOut`, within
`DEAD_TIMEOUT + SHELL_LATENESS_BOUND` of the **block instant** (sound
because nothing crosses after the block, so the last authenticated receive
is no later than it), and the healed path must carry **nothing**.

The heal is the whole design. D's §3.3 argument is confirmed: the two
permanent bidirectional blocks in `tests/story_compat.rs` (S31) kill the
connection on any build, including one whose passive keepalive re-fires
every interval, because with no path an immortally-armed keepalive reaches
nobody either. Only a healed path separates them.

### 2.3 Measurement that changed the test — conflict **O1**

The first draft asserted `net.sends() >= sends_at_block + 2`, on the
assumption that a bidirectional block drops **two** keepalives. It went red
at 1. Driving the fixture with a 500 ms-step timeline (scratch test, since
removed) gives, per 10 s tick:

```
unblocked            t = 10 s   a+1 b+1   sends+2
a→b blocked only     t = 30 s   a+0 b+1   sends+2
both directions      t = 30 s   a+0 b+0   sends+1
```

The two sides are a **chain**, not two clocks. §7.5's predicate is *"has
received since it last sent"*, and in the steady dance one peer's timer
fires on its own while the other's keepalive is armed by that keepalive
**arriving**: with `a→b` blocked the follower still hears the leader and
still fires; with both directions down it never fires at all. A
bidirectional block therefore cuts the *arming* as well as the delivery.

Reported, not resolved (working rule 3). Recorded twice in code — as **K3**
in `tests/story_keepalive.rs`'s "Reported, not resolved" header block, and
as the measurement table inside the test's own doc comment — so the next
reader is not left to infer it.

Note carefully what is and is not in tension. SECV5-8's **stimulus** (*"drop
both directions' keepalive in the same interval"*), its **assertion**
(*"assert both sides fire `TimedOut`"*) and its **stated reason** (*"neither
can re-fire the one-shot passive rule"*) are all exactly right — the reason
is if anything stronger than it claims, since one peer cannot re-fire and
the other cannot fire at all. What over-states is the implied *count* of
dropped datagrams. A maintainer may reasonably rule that no spec change is
warranted.

### 2.4 Separating mutant (rule 9)

See §9.

## 3. Work item 2 — SECV5-5 repeatability

### 3.1 Site

`src/core/endpoint/routing.rs`, inside the `#[cfg(test)] mod tests`, in
`a_dialled_live_rows_stale_reverts_its_record_and_marks_contested` — the
test D identifies at `:1379`, which fed `captured` exactly once.

### 3.2 Assertions added

A second surfacing of the **same** `captured` bytes, from `addr(8, 4011)`
(a second source port, so §6.3's dedup key differs — the same address would
take the byte-replacement path `dedup_replaces_and_keeps_the_intro_id_without_a_second_surfacing`
pins, which surfaces no second `Intro` by design). Then, in order: a second
`Intro`; `authenticate()` succeeds and writes **the same** provisional
timestamp (the first call's return value is now bound rather than discarded,
so the two writes are compared rather than merely both being `Some`);
`accept()` → `Err(AcceptError::Stale)` again; `greatest()` back to `None`
again; `Contested(dialled)` signalled again; nothing failed, nothing
replaced.

The `contested` assertion carries a second fact worth having in the record:
ruling 41's *"a second refusal while contested is a total no-op"* is the
**connection core's** rule, and the endpoint signals every refusal of an
admitted candidate. Asserting the endpoint's second signal pins the two
layers apart — which is the mirror-a-security-invariant-into-the-wrong-layer
shape working rule 11 records (rulings 87 → 90).

### 3.3 Separating mutant (rule 9)

See §9. Two were run; the second is the one that matters. The first shows
that D's *stated* separating build is already caught by the pre-existing
assertions — conflict **O2**.

## 4. Work item 3 — SECV5-6 between-timestamp admit via accept()

### 4.1 Site

`src/core/tests.rs`, new test
`a_basis_refused_accept_restores_a_prior_value`, placed beside its sibling
`authenticate_then_reject_restores_a_prior_value` in the §17.1 guard group.

### 4.2 Assertions added

D's C6: the *"a timestamp **between** the two is still admitted"* clause was
asserted only on the `reject()` path (`src/core/tests.rs:1950`), while the
basis-refused `accept()` arm was pinned only in its **empty** case
(`routing.rs:1379` asserts `greatest() == None`, where "restored" and
"deleted" give the same answer). The new test supplies the missing arm with
a **pre-existing** record.

Reaching that shape took fixture work worth recording, because it is not
obvious the shape is reachable at all. §6.4's `None` basis means *"we
dialled this connection"*, and §16.1 forbids a dial while the static is LIVE
— so the prior record must be laid down by an accepted chain whose
connection is then **retired** (`handle_connection_event(… Retired …)`,
which frees the static and leaves the guard record standing), and the dial
must be answered by a **second endpoint holding the same static**
(`Ep::new(t, 9, 0x33, …)`, because `b` is left holding the pending its own
`msg1_train` minted and §6.4's PENDING branch would answer with a tie-break
rather than a msg2). The test asserts each precondition rather than assuming
it: the record survives the retirement, the dial installs `conn1`, and
`replacement_basis` reads `Some(None)`.

Then `train[2]` authenticates over `ts0`, `accept()` is refused by the basis
rule, and the guard must read `Some(ts0)` — not `None` (deleted) and not
`Some(ts2)` (kept). Behaviourally on both sides of that value: `train[0]`
still refused `Replay`, `train[1]` — the between-timestamp — still admitted.

### 4.3 Separating mutant (rule 9)

See §9.

## 5. Work item 4 — ruling 272 IntroEntry size pin

### 5.1 Measurement

Measured on this tree with a scratch `panic!` (reverted with
`git checkout --` before any other edit, working rule 10):

```
$ cargo test --all-features --lib scratch_measure_intro_entry
thread 'core::tests::scratch_measure_intro_entry' panicked at src/core/tests.rs:3322:5:
SIZE=288 align=8 vec=24 instant=16 sockaddr=32
```

`size_of::<IntroEntry<Id>>()` is **288 B inline**, align 8 — exactly
ruling 272's figure. With the 196 B `msg1: Vec<u8>` on the heap the entry
costs **484 B**, against §6.3's superseded *"≈ 220 B per entry"*
(`SPEC.md:1292-1293`), §17.5's `≈ 220 B raw bytes each, ≈ 225 KB`
(`SPEC.md:7026`) and the §5 DH table's `one bounded queue slot (≈ 220 B)`
(`SPEC.md:1988`). The old figure is 2.2× under, as ruling 272 records.
### 5.2 Bounds chosen and justification

`src/core/tests.rs`, new test
`a_stage_zero_slot_costs_ruling_272s_figure_not_the_superseded_220_bytes`,
in the intro-queue group immediately above `── the caps ──`. It cites
ruling 272 in its doc comment, as instructed.

```rust
assert_eq!(MSG1_HEAP, INIT_PACKET_LEN);           // 196, the heap half
let inline = size_of::<endpoint::intro_queue::IntroEntry<Id>>();
assert!((256..=320).contains(&inline));           // 288 measured
assert!(inline + MSG1_HEAP > 2 * 220);            // the 2.2x claim
assert!((inline + MSG1_HEAP) * INTRO_QUEUE_CAP < 1 << 20);
```

**Why bounds and not equality.** An exact `assert_eq!` on `size_of` tests
the *layout*, which is the compiler's to choose: field order, niche packing
and a platform's `Instant` (16 B here, 8 B where it is a bare counter) all
move the number without moving the fact.

**Each side separates something** — working rule 9's bar, applied per side:

* **Lower, 256 B.** The superseded *"≈ 220 B per entry"* was the whole
  entry, message included. 256 B of *inline* alone refutes it, with 32 B of
  slack — four words, more than any padding decision moves — so a tree that
  quietly reverted to the old shape goes red here rather than in a spec
  review three rounds later.
* **Upper, 320 B.** Keeps the whole entry under 512 B, which is what ruling
  272's ≈ 0.47 MiB at `INTRO_QUEUE_CAP` rests on. The regression it
  separates is the one §6.3 already names as its *"one bounded exception"*
  — an eager-demoted entry carrying its already-paid mid-state — measured in
  Appendix A (`SPEC.md:7318`) at **784 B** for the reference suite. Storing
  that inline for every entry rather than for the bounded exception would
  take a slot past 1 KB and §17.5's budget past 1 MiB, and it is the single
  most plausible way this figure moves.

**The heap half needs no bound.** `msg1` holds *"the newest msg1,
verbatim"*, and §3.1 is exact for the two handshake types (ruling 65):
nothing but an `INIT_PACKET_LEN` datagram reaches the queue, which is
`only_an_exactly_sized_init_reaches_the_queue`'s three-sided boundary. It is
196 B or the entry does not exist. It is written as a literal *and* checked
against `INIT_PACKET_LEN`, so ruling 272's arithmetic can be read off this
test without leaving it, while the wire pin still governs the value.

### 5.3 Why no code mutant is run for this one

The brief allows it, and the reason it is right is that the *two-sided bound
is itself the mutant*: the assertion's separating power is a property of the
band, not of a build. The old ≈ 220 B claim is a real, previously-shipped
"build" of the figure and the lower bound rejects it — that is the
degenerate case rule 9 asks for, and it is not hypothetical: it is what
`SPEC.md:1292`, `:1988` and `:7026` said until ruling 272. The upper bound
is exercised in the same way by Appendix A's own 784 B measurement. Both
sides were also checked mechanically while choosing them: the scratch
measurement in §5.1 is the only input, and 288 sits 32 B inside each edge.

**Scope note, working rule 8.** The pin is a *core-level unit pin on the
type*, as instructed. It does not and cannot pin the `HashMap` overhead per
entry, which is real and is not in ruling 272's figure either. If the ruling
intends ≈ 484 B to be the whole per-slot cost including the map's bucket,
that is a second question and is not asserted here.

## 6. Traceability comments (D §3.4)

### 6.1 Landed in owned files

| draft | site | landed as |
|---|---|---|
| SECV5-5, main | `src/core/endpoint/routing.rs`, doc of `a_dialled_live_rows_stale_reverts_its_record_and_marks_contested` | **adapted** — see (a) |
| SECV5-5, supporting | `src/core/tests.rs`, doc of `a_dialled_static_holds_no_guard_entry` | as drafted, one clause widened |
| SECV5-6 | `src/core/tests.rs`, doc of the new `a_basis_refused_accept_restores_a_prior_value` | the tag is named and the obligation quoted |
| SECV5-8 | `tests/story_keepalive.rs`, doc of the new test | **not** as drafted — see (b) |

**Two deliberate departures from the drafts (working rule 5).**

*(a) SECV5-5's main comment.* D drafted it to end *"The obligation's
'surfaces **repeatably** — more than once from a single captured packet'
clause is **not** asserted here: `captured` is fed once."* Work item 2 is
precisely the instruction that makes that sentence false. Landing the draft
verbatim would commit a comment contradicted by the code twelve lines below
it — working rule 4's defect, manufactured on purpose. The comment now
claims all four clauses and points at the second half for the repeatability
one.

*(b) SECV5-8's placeholder.* D drafted a
`pub const SECV5_8_BIDIRECTIONAL_KEEPALIVE_LOSS: () = ();` recording the
gap, explicitly conditional (*"If the maintainer wants it tracked in code
rather than only here"*) and premised on no discharge site existing. Work
item 1 creates the discharge site, so the placeholder would be a registry
entry false on the day it lands — and `mod owed`'s eight stale entries,
which the rest of D's report exists to clear, are exactly what that
produces. The tag is named in the test's doc comment instead, in the house
style D itself identified (both existing SECV5-2 traces are prose inside doc
comments, not a bespoke marker).

### 6.2 Deferred to the integrator (unowned files)

**SECV5-6's drafted comment targets `src/core/endpoint/tests.rs:2016`, which
is not one of my four paths.** Reproduced here for whoever lands it — with
its last clause corrected, because the clause D says is asserted only on the
`reject()` path is now asserted on the `accept()` path too:

```rust
/// **Appendix B's no-record-on-`Stale`, SECV5-6** (`SPEC.md:7472`): both
/// halves — the winner's exception in (a), the ordinary revert in (b). The
/// basis-refused `accept()` arm is covered by `endpoint::routing`'s
/// `a_dialled_live_rows_stale_reverts_its_record_and_marks_contested`; the
/// *"a timestamp **between** the two is still admitted"* clause is asserted
/// on the `reject()` path at `core::tests`'
/// `authenticate_then_reject_restores_a_prior_value` and on the
/// basis-refused `accept()` path at `core::tests`'
/// `a_basis_refused_accept_restores_a_prior_value`.
```

Nothing else in D §3.4 targets a file outside my four paths.

**Also for the integrator, not actioned here.** `SPEC.md` is not one of my
paths, so ruling 272's amendment is not made here. The **three** sites
carrying the superseded stage-0 figure, all found by grepping the value and
then reading each context (working rule 4):

| site | text |
|---|---|
| `SPEC.md:1292-1293` (§6.3) | *"the raw 196-byte msg1 plus the source address (≈ 220 B per entry; worst case ≈ 225 KB at the default cap)"* |
| `SPEC.md:1988` (§5's DH-cost table) | *"one bounded queue slot (≈ 220 B)"* |
| `SPEC.md:7026` (§17.5's honesty table) | *"≈ 220 B raw bytes each, ≈ 225 KB"* |

Two things a grep for `220` alone would miss. First, the last two are
**rows**, and each states a *derived total* (≈ 225 KB) as well as the
per-entry figure; at ≈ 484 B the totals become ≈ 0.47 MiB, so amending the
per-entry number without the total leaves the row self-contradicting.
Second — and this is the one I got wrong before opening the file, recorded
because working rule 11 says a claim about a document is unchecked until the
document is read — **the ≈ 0.5–1 KB per-entry estimate at `SPEC.md:1394`
and `:7027` is a different tier and is *not* touched by ruling 272.** It
prices **staged mid-states** (live key material), not stage-0 entries, and
Appendix A's *"inside §6.3/§17.5's ≈ 0.5–1 KB per-entry estimate, which
therefore **stands unamended**"* (`SPEC.md:7322-7323`) cites that tier
correctly. My first draft of this paragraph read the two as one figure and
claimed Appendix A's sentence would stop being true; it would not. The
stage-0 tier and the mid-state tier are separately budgeted rows of the same
§17.5 table.

## 7. Gates

Run on the final tree (working rule 7 — commands and output, not a claim).

| Gate | Command | Result |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | `Finished \`dev\` profile` — clean |
| Format | `cargo fmt --all --check` | no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `--all-features` | both generated, no broken links |
| Tests | `cargo test --all-features` | **29 targets, 1130 passed, 0 failed, 2 ignored** |
| Tests | `cargo test` (feature-less) | **5 targets, 917 passed, 0 failed** |
| Release tests | `cargo test --release --all-features` | **29 targets, 1132 passed, 0 failed** |
| Wire pins | golden-wire + size/constant tests | all green, byte-identical |

```
$ cargo fmt --all --check
GATE fmt: no diff

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (…/agent-a585c41106ee57feb)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.38s

$ cargo build --all-features --all-targets
    Compiling slither v0.2.0 (…/agent-a585c41106ee57feb)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.74s

$ cargo test --all-features        # summed over the 29 test targets
targets: 29 passed: 1130 failed: 0 ignored: 2

$ cargo test                        # feature-less
feature-less targets: 5 passed: 917 failed: 0 ignored: 0

$ cargo test --release --all-features
targets: 29 passed: 1132 failed: 0 ignored: 0

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated …/target/doc/slither/index.html
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated …/target/doc/slither/index.html

$ cargo test --all-features golden
test packet::tests::golden_prologue ... ok
test packet::tests::golden_mac1 ... ok
test packet::tests::sizes_match_the_golden_vectors ... ok
test packet::tests::mac1_tag_matches_the_golden_vectors ... ok
… all green
```

**MSRV and `cargo deny` were not run**: `cargo +1.96` is not installed in
this worktree and `cargo deny` is not on `PATH`. This change adds no
dependency and no language feature beyond what the tree already uses
(`size_of` from the prelude, stable since 1.80), so neither gate's *subject*
moved — but neither was executed here, and working rule 7 forbids reporting
a gate green without running it. Flagged for CI on the merge commit.

**No `Cargo.toml` change is needed and none was made.** `story_keepalive`
already carries its `[[test]]` stanza (`Cargo.toml:276-278`,
`required-features = ["test-util"]`) and the new test is inside that file;
the other three items are `#[cfg(test)]` code inside `src/`.

## 8. Conflicts found (rule 3 — reported, not resolved)

**O1 — SECV5-8 says *"drop both directions' keepalive in the same
interval"*, and on this build a bidirectional block drops **one**.**
Measured, §2.3: `Network::sends` grows by 2 on an unblocked tick, by 2 with
one direction blocked, and by **1** with both blocked. §7.5's predicate is a
receive, so the follower's keepalive is armed by the leader's *arriving*;
cutting both directions cuts the arming as well as the delivery. The
obligation's stimulus, its assertion and its stated reason are all correct —
the reason is stronger than it claims — and only the implied datagram count
over-states. Recorded as K3 in `tests/story_keepalive.rs`'s header. **Not
resolved**; a maintainer may well rule no change is warranted.

**O2 — GAPSLICE-D §3.1's separating build does not separate, and the
existing test already catches it.** D writes: *"A build that consumed the
captured packet on first use — one that let the refused chain's teardown
poison a replay of the same bytes — passes every assertion in that test."*
Measured (mutant 3a, §9): scoping the non-revert to §6.4's `None` arm alone
turns `routing.rs:1432` red — *"§17.1 mitigation (i): a non-winner `Stale`
REVERTS its provisional record"* — which is a **pre-existing** assertion.
The build that does slip is a different one: a core that **spends the
bytes**, recording the torn-down chain's msg1 and refusing to authenticate
it again (mutant 3b). That build leaves every prior assertion green and dies
on the new block.

D's **conclusion** is right and its **argument** is wrong — the inverse of
working rule 12, and the shape working rule 11 records about ruling 206. The
clause really is unasserted and asserting it really is worth doing; the
mechanism named as the reason is not the mechanism that gets past the suite.
The corrected characterisation is now written into the test's own comment,
so the next reader inherits the measurement rather than the inference.

**O3 — a claim of my own, corrected before it shipped, recorded because
working rule 11 says the failure mode is the interesting part.** Drafting
§6.2 I wrote that ruling 272's amendment would falsify Appendix A's
*"inside §6.3/§17.5's ≈ 0.5–1 KB per-entry estimate, which therefore
**stands unamended**"* (`SPEC.md:7322-7323`), since ≈ 484 B is below 0.5 KB.
Opening the cited sections refutes it: ≈ 0.5–1 KB prices **staged
mid-states** (`SPEC.md:1394`, `:7027`), a different row of the same §17.5
table from the stage-0 ≈ 220 B (`:1292`, `:1988`, `:7026`) ruling 272
amends. Appendix A cites its tier correctly. Written up because the draft
was produced *alongside* five sound spec citations, which is exactly the
condition working rule 11(a) names.

**No conflict found on the other two work items.** SECV5-6's obligation, the
ruling 272 figure and the §6.4 basis rule all read consistently against the
code as measured.

## 9. Mutant table

Every mutant was applied **after** the work was committed at `4a12108`
(working rule 10) and reverted with `git checkout --`; the tree was verified
clean after each.

| # | target | mutation | site | result |
|---|---|---|---|---|
| 1 | SECV5-8 | drop the one-shot: `received_since_marking_send` is never cleared by a marking send, so the passive keepalive re-arms every interval | `src/core/connection/session.rs`, `Liveness::on_send` | **new test RED**; `story_compat`'s two permanent bidirectional blocks stay **green** |
| 2 | SECV5-6 | the basis-refused `accept()` arm **deletes** the guard record instead of reverting it (the `reject()` arm untouched) | `src/core/endpoint/staged.rs` §6.4 `None` arm + a `revert_deleting` in `guard.rs` | **new test RED, and it is the ONLY red in 779 lib tests** |
| 3a | SECV5-5 | D's stated build: the `None` arm's teardown keeps the provisional record | `src/core/endpoint/staged.rs` §6.4 `None` arm | **pre-existing assertion RED** — see conflict O2 |
| 3b | SECV5-5 | the captured packet is **spent** on first use: the teardown records its msg1 and `authenticate()` refuses those bytes again | `src/core/endpoint/staged.rs`, `authenticate` + `discard_chain` | **new block RED at `routing.rs:1467`; every prior assertion in the same test green** |
| — | ruling 272 pin | none run; justified in §5.3 — the two-sided band *is* the separator, and the ≈ 220 B build it rejects is not hypothetical but what `SPEC.md` said until ruling 272 | — | — |

### Mutant 1 — SECV5-8

```
$ cargo test --all-features --test story_keepalive s5_both_directions
test s5_both_directions_losing_one_keepalive_kills_both_sides_even_after_the_path_heals ... FAILED

panicked at tests/story_keepalive.rs:346:29:
the connection was still alive after 25s: §7.5: the passive rule is one-shot
— the keepalive that was swallowed disarmed it, and a healed path cannot
re-arm what only a receive can

$ cargo test --all-features --test story_compat        # the S31 permanent blocks
test s31_flush_is_a_no_op_and_never_delivery_confirmation ... ok
test s31_shutdown_resolves_in_error_when_the_connection_dies_first ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test --all-features --lib                      # what DID catch it, at the core
test core::connection::tests_livelock::an_admissible_passive_keepalive_fires_and_then_disarms ... FAILED
test core::connection::tests_roam::a_marking_send_in_the_same_evaluation_suppresses_the_keepalive ... FAILED
test core::connection::tests_roam::the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms ... FAILED
test result: FAILED. 776 passed; 3 failed
```

Three core tests catch the one-shot property; **no flow test did**, and the
two that look like they should — `story_compat`'s permanent blocks — pass on
the broken build. That is the whole of D's §3.3 argument, measured.

### Mutant 2 — SECV5-6

```
$ cargo test --all-features --lib
test core::tests::a_basis_refused_accept_restores_a_prior_value ... FAILED
test result: FAILED. 778 passed; 1 failed; 0 ignored; 0 measured

panicked at src/core/tests.rs:2175:5:
assertion `left == right` failed: §17.1 mitigation (i) on the basis-refused
accept arm: the pre-existing entry REVERTS, it is not emptied
  left: None
 right: Some(Timestamp { secs: 1787114536, nanos: 910059000 })
```

**One red in 779.** `authenticate_then_reject_restores_a_prior_value` (the
`reject()` arm) and `a_dialled_live_rows_stale_…` (the accept arm's *empty*
case) both stay green, which is precisely the gap D's C6 described and the
strongest single result in this slice: the new test is the only thing in the
suite that can see the difference between reverting and deleting on that arm.

### Mutant 3a — SECV5-5, D's stated build

```
$ cargo test --all-features --lib a_dialled_live_rows_stale
panicked at src/core/endpoint/routing.rs:1432:9:
assertion `left == right` failed: §17.1 mitigation (i): a non-winner `Stale`
REVERTS its provisional record
  left: Some(Timestamp { secs: 1787114574, nanos: 652222000 })
 right: None
```

`:1432` is pre-existing. Conflict **O2**.

### Mutant 3b — SECV5-5, the build that does slip

```
$ cargo test --all-features --lib a_dialled_live_rows_stale
panicked at src/core/endpoint/routing.rs:1467:14:
the same bytes authenticate a second time: Replay
```

`:1467` is inside the block added by work item 2; everything above it passed.

### Clean tree after every mutant

```
$ git checkout -- src/core/connection/session.rs        # 1
$ git checkout -- src/core/endpoint/guard.rs src/core/endpoint/staged.rs   # 2
$ git checkout -- src/core/endpoint/staged.rs           # 3a, 3b
$ git status --short
(clean)
```


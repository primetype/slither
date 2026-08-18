# Reconstruction: rulings 216–220 — the slice-7b gap in `rulings.md`

*Drafted 2026/08/18 by a record-reconstruction agent, READ-ONLY on the repo.
Nothing here has been entered into `.spec-v2-clean-slate/rulings.md`; these
are draft entries for the maintainer to ratify, amend or reject. Every claim
below cites the artefact it is taken from. Where two sources disagree, both
are shown and the conflict is marked — none has been harmonised.*

---

## Preamble — five numbers spent in code, never written in the record

`rulings.md` runs `### 215` (`:6204`, the tail of round 34) straight into
`## Round 35 — auditing ruling 208's challenge` (`:6262`) and `### 221`
(`:6264`). **Rulings 216 through 220 are absent.** All five are cited as
authorities in shipped code, so all five were taken; none was ever entered.

**When.** Between 14:05 and 15:34 on **2026/08/16**, inside the slice-7b
integration window:

```
677dce6 14:05:35  Ruling 215: 212(c) reversed in its rank half; T1's blind tests merged
4f08786 14:11:53  Slice 7b (I1): ruling 208's path frames, A2, A3, F1, 212(c) and 212(f)
b4576db 14:13:58  Merge branch 'worktree-agent-a2e2da818396946cb'
f3a4576 14:46:24  Slice 7b integration: ruling 217, 720/5 (was 16 failing)      ← 216, 217, 218
386359f 15:34:05  Slice 7b: 993 passing, 1 failing -- ruling 217 settled...      ← 219, 220
1b0be95           Ruling 221: a lost PATH_CHALLENGE is asked again by the PTO
```

**Why they were lost — what the commit history shows.** Ruling 215 landed in
a *dedicated ruling commit* (`677dce6`) whose diffstat is
`rulings.md | SPEC.md | tests_path.rs`. Ruling 221 landed the same way
(`1b0be95`). **The two commits in between touch no `rulings.md` at all**:

```
$ git show --stat f3a4576   →  Cargo.toml, mobility.rs, mod.rs, tests_livelock.rs
$ git show --stat 386359f   →  mod.rs, tests_livelock.rs, tests_path.rs
```

Both are **integration** commits, and both were *ruling while integrating*:
the maintainer allocated numbers on the spot to settle a conflict two blind
agents had reported and to migrate three blind-authored premises, stamped
them into the code (`**[RATIFIED 2026/08/16 — ruling 217]**`,
`**[Integrator, ruling 218 …]**`) and into the commit messages, and moved
on. The next dedicated ruling commit opened a new round heading and resumed
at 221 — skipping the five numbers that had already been spent.

**They were never written and then deleted; they were never written at all.**

```
$ for n in 216 217 218 219 220; do git log --all -S "### $n" -- .spec-v2-clean-slate/rulings.md; done
(no output for any n)
$ git log --all -S "### 217"        # any file, any ref
(no output)
```

**And the gap survived a deliberate re-read.** Round 40's sweep
(`.spec-v2-clean-slate/SWEEP-round40.md:496–527`, items 43–47) walked
`rulings.md:6204–6320` line by line under the heading *"rulings.md — 212 /
215 / 217"*, quoting ruling 215's title and ruling 221's body in consecutive
findings, and did not remark that five entries were missing between them. A
sweep looking for *stale* text does not notice *absent* text.

**The irony is on the record already.** Ruling 215's own commit message, one
commit earlier, diagnoses this exact shape — *"closing a flag is not
sweeping the spec"* — and cites ruling 214(d), *"committing the rulings is
not committing the contract"* (`809cb62`). The very next integration commit
committed the code without committing the rulings.

**The discipline lesson, in one sentence:** a ruling number spent in a code
comment or a commit message is not a ruling in the record — allocate the
number and write its entry **in the same commit that lands the code it
justifies**, because the only artefact that ever notices the omission is the
one nobody re-reads sequentially.

---

### 216 [RECONSTRUCTED 2026/08/18] — the slice-6 core-test declarations were waiting for files that never existed, and are retired rather than left

**[Taken 2026/08/16 at `f3a4576`, as part of slice 7b's integration.]**

**What was decided.** The two commented declarations at the head of
`src/core/connection/mod.rs` —

```rust
// #[cfg(test)]
// mod tests_datagram;
// #[cfg(test)]
// mod tests_message;
```

— had sat there since slice 6 under an integration header reading *"the
files land with them … the integrator uncomments both lines"*
(`mod.rs`, pre-`f3a4576` text, quoted in the diff). **The files do not exist
and never did**: `git log --all` knows no `tests_datagram.rs` or
`tests_message.rs`. Slice 6's authors wrote *integration* tests instead —
`tests/story_datagram.rs` (11 tests) and `tests/story_message.rs` (15) —
which have passed every gate since. **Nothing was lost.**

**Ruling: the block is retired, not left standing.** The recorded reason,
verbatim from the site it was written at
(`src/core/connection/mod.rs:103–115`):

> Retired rather than left, because **a commented `mod` for a file that was
> never written is indistinguishable from one whose file was lost**, and the
> comment asserted the second (*"the files land with them"*). That is the
> standing hazard of ruling 211's mechanism, found on its first use — the
> integrator's uncomment step is the only thing that ever distinguishes the
> two, and nothing fails if it is skipped.

**Evidence trail.**
- `src/core/connection/mod.rs:103` — the only citation of ruling 216 anywhere
  in the tree (`grep -rn "ruling 216" src tests SPEC.md .spec-v2-clean-slate`
  returns this one line).
- `git show f3a4576 -- src/core/connection/mod.rs`, hunk `@@ -98,20 +98,21 @@` —
  the retirement, and in the same hunk the *uncommenting* of slice 7b's three
  live declarations (`tests_path`, `tests_livelock`, `tests_reassembly`).
- `f3a4576`'s commit message, final paragraph: *"Slice 6's
  tests_datagram/tests_message declarations retired: those files were never
  written (authors wrote integration tests instead). A commented mod for a
  file never written is indistinguishable from one whose file was lost — the
  standing hazard of ruling 211's mechanism."*
- Context for the mechanism being critiqued: `### 211` (`rulings.md:5978`,
  *"CLAUDE.md's working rules 6 and 15 contradict each other"*), and
  `CLAUDE.md` working rule 6's `[Amended by ruling 211]` clause.

**What remains UNKNOWN.**
- **Whether 216 was meant to carry a general obligation.** The finding is
  stated as a *standing hazard* of ruling 211's mechanism, which reads like
  the opening of a rule — but no rule was written. `CLAUDE.md` working rules
  6 and 15 are unchanged to this day; nothing instructs an integrator to
  *retire-or-uncomment* as a closing step. Whether the maintainer intended
  only the local cleanup or a durable amendment **is not recorded and is not
  reconstructed.**
- No `SPEC.md` amendment is stamped 216 (`grep -n "ruling 216" SPEC.md`
  returns nothing), and none is implied.

**Confidence: HIGH** on what was decided and why (the rationale is preserved
verbatim at the code site). **MEDIUM** on scope — see UNKNOWN above.

---

### 217 [RECONSTRUCTED 2026/08/18] — §8.7 outranks `CONTRACT-7b` §1.4: the challenge rides **any** packet the budget admits, including one built for an owed ACK — and the pump never manufactures one for it

**[RATIFIED 2026/08/16 — the stamp is in the code, at
`src/core/connection/mod.rs:2638`. Taken at `f3a4576`, settled at
`386359f`.]**

**The conflict, as both blind agents reported it.** `CONTRACT-7b.md` §1.4
converts the pump's `validate` branch from `packing.ping()` to a
`PathChallenge` and states that all four documented boundaries *"carry over
**unchanged in force** and changed in wording"* — including **(1)** nothing
owed ⇒ no challenge and **(2)** a bare ACK with nothing else owed ⇒ no
challenge — guarding `if (probe || validate) && !elicits`
(`.slices/07b-remediation/CONTRACT-7b.md:223–256`). The implementer built to
that. The blind test author wrote to `SPEC.md` §8.7's strictly broader
condition — *"whenever §7.3's budget admits a packet and the address is
still unvalidated"* — flagged the divergence under a
`⚠ SECOND CONFLICT, REPORTED AND NOT RESOLVED (working rule 3)` header, and
closed: *"this author believes that build is wrong — but the call is the
maintainer's"* (`src/core/connection/tests_path.rs:1240–1302`).

**Ruling: §8.7 wins. `self.ack.is_owed()` becomes a disjunct of the pump's
offer.**

```rust
let offer = owe_challenge
    && (self.owes_output() || self.contested.is_pending() || self.ack.is_owed());
```
(`git show f3a4576 -- src/core/connection/mod.rs`, hunk `@@ -2134,7 +2136,23 @@`.)

The recorded reasoning, from `mod.rs:2638–2664` as landed:

- **Working rule 3's tiebreak decides it** — *follow the statement some other
  proof depends on*: §7.3's no-deadlock proof depends on §8.7.
- **Boundary 1/2 was sound under ruling 168**, where any ack-eliciting packet
  drew the ACK that validated, so waiting for real output cost nothing.
  **Under ruling 208 an ACK proves nothing**, and the only packet that can
  validate is one we choose to send. Deferring until output exists means the
  output arrives into a budget still capped at 3× and pays a round trip to
  escape: **ruling 203's stall, one indirection later**.
- **The implementer's counter-argument does not survive its own code**: it
  feared *"an unprompted probe train on every pump"*, but `owe_challenge` is
  cleared the moment the challenge is sealed, so an arming emits exactly
  **one** — 39 bytes against the 90 B floor the smallest arming funds, and
  against msg2's 107 on the accept path. There is no train to fear.
- Scale of the case: *"Every accepted connection begins unvalidated, so this
  is the **commonest** post-roam packet in the protocol, not an edge"*
  (`mod.rs:2404–2405`, inside the boundary-2 block ruling 224 later struck through).

**217, amended — in the same commit.** *A `PATH_RESPONSE` we owe is
manufactured; a `PATH_CHALLENGE` is not.* 217's **first draft also
manufactured a dedicated packet** for the challenge and was wrong twice
(`mod.rs:2665–2682`, and `f3a4576`'s commit message):

1. **It hung the suite.** `owe_challenge` is a local, `true` on every entry,
   so every pump emitted a fresh dedicated packet and nothing driving a pair
   to quiescence ever settled.
2. **The blind test author had already forbidden it in writing.** §8.7 owes
   the challenge *"whenever §7.3's budget admits **a packet**"* — a rule
   about packets being built, which does not ask the pump to build one.
   `an_idle_unvalidated_connection_owing_nothing_emits_no_challenge`
   (`tests_path.rs:1411`) pins the ban.
3. **The author's two tests were consistent and the maintainer misread them
   as a conflict**: the roam that must carry a challenge is ack-eliciting, so
   a reply packet exists for it to ride
   (`a_packet_leaving_for_an_unvalidated_address_carries_the_challenge`,
   `tests_path.rs:1316`); the roam that must not is an empty keepalive, where
   no packet is built at all. **What 217 settles is the ride-along** — §8.7's
   condition, not the contract's `!elicits`.

The one surviving manufacture is `pump_path_frames`, for an owed
`PATH_RESPONSE` the loop planned and a gate then refused
(`mod.rs:2729–2737`, doc comment stamped `**[ruling 217]**`).

**Also stamped 217 — the second of `f3a4576`'s "two migrations."**
`tests_livelock.rs`'s `spend_leaving` shaping helper predates path frames;
the 9-byte challenge packs at §8.5 stage 2 *ahead of* the stage-3 datagram
fill, so the shaping datagram no longer fit its room and the drain yielded a
39-byte challenge-only packet, wrecking the calibration. The helper subtracts
`1 + 8` when the budget is armed — *"keeps the helper's contract exactly as
written … rather than relaxing the assertions, which is what would hide the
next such change"* (`tests_livelock.rs:180–195`).

> ⚠ **CONFLICT OF ATTRIBUTION, presented not harmonised.** `f3a4576`'s
> commit message describes **"two migrations of blind test files"** as a
> single pair and assigns them **no ruling number**. The code stamps the
> first (the responder fixture) **`ruling 218`** and the second (the shaping
> helper) **`ruling 217`**. So either 217 covers both the §8.7 ride-along and
> a fixture migration, or the shaping-helper stamp should have read 218.
> Both readings are consistent with every artefact found; the record does not
> decide it.

**Residue this ruling left in the tree, and its fate.** `f3a4576` also
committed `dedicated_sent`, `owes_dedicated_challenge()` and
`on_dedicated_challenge_sent()` on `Amplification`
(`git show f3a4576 -- src/core/connection/mobility.rs`) — the first draft's
machinery, **called from nowhere**. Ruling 221 deleted it: *"an abandoned
once-per-arming manufacture, left in the tree when 217's first draft was
reverted"* (`rulings.md:6358`). Ruling 222 records that
`#![allow(dead_code)]` is what hid it — *"it hid ruling 217's abandoned
machinery until an audit found it by hand"* (`src/core/mod.rs:37`).
Ruling 250 later removed 217's second `pump_path_frames` call site
(`mod.rs:2738–2745`), and ruling 221's `dedicated_sent` deletion is
explicitly *"not resurrected"* by 250 (`mod.rs:3190`).

**Downstream corrections that belong to 217's story.**
- **Ruling 221** adds `probe` as a further disjunct — the retransmission half
  of the same standing obligation (`rulings.md:6264–6329`).
- **Ruling 224** — *"a boundary comment stood arguing the position ruling 217
  had reversed"* (`rulings.md:6452`). Boundary 2 stood eighty lines below the
  line 217 edited, in the same function, for two rulings and thirty blind
  tests. It is struck through in place at `mod.rs:2395–2403`.
- **Round 41 item 6** (`ae71823`, 2026/08/18) stamps the resolution into the
  conflict header the blind author left: *"the code follows §8.7 … rulings
  217/224/221 settled it"* (`tests_path.rs:1245–1268`).

**What remains UNKNOWN.**
- **Whether the amendment was a separate decision or one ruling in two
  passes.** The code says `**[ruling 217, amended]**` (`mod.rs:2665`) with no
  date or separate number, and `386359f`'s subject line says *"ruling 217
  settled"* thirty minutes after `f3a4576` claimed to settle it. Whether the
  maintainer regarded the amendment as 217(a) **is not recorded.**
- **No `SPEC.md` amendment carries a 217 stamp.** §8.7 needed none — it was
  already right and the code was wrong (the same shape ruling 221 states
  explicitly of itself). Whether `CONTRACT-7b.md` §1.4 was ever corrected is
  moot: it is a slice contract, and `.slices/07b-remediation/CONTRACT-7b.md`
  still reads *"unchanged in force"* today.

**Confidence: HIGH.** This is the best-evidenced of the five: a `RATIFIED`
stamp, a full rationale preserved at three code sites, a commit message
restating it, and two later ruled entries (221, 224) recapping it.

---

### 218 [RECONSTRUCTED 2026/08/18] — a blind fixture premise whose subject a blind fix changed is **migrated, not weakened**: the responder arming charges msg2's 107 bytes

**[Taken 2026/08/16 at `f3a4576`.]**

**What was decided.** `tests_livelock.rs`'s `responder_at` fixture asserted
the msg1-anchored responder's budget as `(0, INIT_PACKET_LEN)` = `(0, 196)`.
The adversarial review's **A2** found that msg2's 107 bytes are emitted
endpoint-side and were **charged to nothing**, making the real responder
ratio **3.55×** against a normative MUST of 3; the remediation implementer
fixed it in the same slice, blind to that file (`4f08786`, *"A2 — msg2 is
charged"*). The fixture went red.

**Ruling: the premise is migrated to the new true value — `(107, 196)`, cap
588, room 481 — with the reason recorded in place; it is not relaxed and not
deleted.** The recorded reason
(`src/core/connection/tests_livelock.rs:107–121`):

> So this red was the fixture premise doing precisely its job — the author
> wrote it to *"make the premise visible rather than silently weakening every
> bound below it"*, and it caught a deliberate change to the very quantity it
> pins. Every test here derives its room through [`room`]/[`spend_to`] rather
> than hardcoding, so nothing below this line needed touching.

**Evidence trail.**
- `src/core/connection/tests_livelock.rs:110` — the sole citation:
  `**[Integrator, ruling 218 — premise migrated, not weakened.]**`
- `git show f3a4576 -- src/core/connection/tests_livelock.rs`, hunk
  `@@ -106,14 +105,28 @@` — `Some((0, INIT_PACKET_LEN as u64))` →
  `Some((RESP_PACKET_LEN as u64, INIT_PACKET_LEN as u64))`, and the assertion
  message gains *"and **msg2's charged** (§3.2, A2)"*.
- `f3a4576`'s commit message, first migration bullet: *"T2's responder fixture
  asserted (0, 196); A2's fix charges msg2's 107. The premise caught a
  deliberate change to the quantity it pins."*
- The fix being migrated to: `4f08786`'s message — *"`established()` and the
  tie-break-loser branch of `install()` charge `RESP_PACKET_LEN` immediately
  after arming … 588 − 107 = 481 still admits the 39-byte challenge."*

**What remains UNKNOWN.**
- **Whether 218 states a general rule or one instance.** `386359f`'s message
  a hour later describes 219 and 220 as *"migrate three blind-authored
  premises whose subjects were changed by fixes landed blind to them, each
  inverted rather than weakened"* — the same formula, applied to a set of
  three, and **218 is not among the three it names**. So the family exists
  and 218 is arguably its first member, but the record does not say whether
  the maintainer intended 218 to be the rule and 219/220 its applications, or
  three parallel case-by-case decisions. **The rationale for treating it as a
  rule was not recorded and is not reconstructed.**
- The number appears once, in a test comment. No `SPEC.md` change, no
  behavioural change: this ruling moves an assertion, not a byte.

**Confidence: HIGH** on the decision and its reason. **MEDIUM** on whether
218 is a standalone ruling or the head of the 218/219/220 migration family —
and see 217's marked attribution conflict, which touches this entry's
boundary.

---

### 219 [RECONSTRUCTED 2026/08/18] — a premise about *when* the challenge reappears is migrated by settling §12.4's delayed-ACK timer; **no assertion is weakened**

**[Taken 2026/08/16 at `386359f`.]**

**What was decided.** §8.7 re-offers the challenge on *a packet the budget
admits* — and **a roam does not always build one on the instant it lands**.
§12.4 owes that ACK on the **delayed-ACK timer** unless the packet is out of
order, so the *first* roam of a run emits immediately and later ones emit one
`MAX_ACK_DELAY` later. The blind author's `roam_and_collect` helper collected
only the instant's output, and therefore reported *"no challenge"* for a
build that emits one a few milliseconds afterwards.

**Measured, not assumed:** *"the second delivery in
`the_same_arming_re_offers_the_same_challenge` produced **zero** frames on
its instant"* (`tests_path.rs:182–184`).

**Ruling: settle the timer, then collect. The trigger migrates; the assertion
does not.** Two sites:

- `roam_and_collect` advances to `now + MAX_ACK_DELAY + 1 ms`, calls
  `handle_timeout`, drains, and extends the frame list before filtering
  (`tests_path.rs:171–198`).
- `the_same_arming_re_offers_the_same_challenge` is rewritten to deliver, then
  step the clock to the ACK timer, rather than expecting the challenge on the
  delivery instant — *"the clock is advanced to that packet rather than the
  assertion being weakened — this test's stated purpose is to pin that the
  **bytes are stable** across re-offers, which is what makes
  `each_arming_draws_a_fresh_challenge` mean anything, and that purpose is
  served identically here"* (`tests_path.rs:691–706`).

The rejected alternative is named: *"manufacturing a packet for the challenge
… is forbidden by this author's own
`an_idle_unvalidated_connection_owing_nothing_emits_no_challenge`"*
(`tests_path.rs:189–192`) — i.e. 219 is decided **inside** 217's amendment.

**Evidence trail.**
- `src/core/connection/tests_path.rs:175` —
  `**[Integrator, ruling 219]** Settle §12.4's delayed-ACK timer before collecting.`
- `src/core/connection/tests_path.rs:691` —
  `**[Integrator, ruling 219 — trigger migrated, assertion untouched.]**`
- `git show 386359f -- src/core/connection/tests_path.rs` — both hunks.
- `386359f`'s commit message, item 219, verbatim: *"roam_and_collect settled
  §12.4's delayed-ACK timer. §8.7 re-offers the challenge on a packet the
  budget admits, and a roam does not always build one on its own instant —
  measured, the second delivery emitted zero frames. The re-offer happens one
  MAX_ACK_DELAY later."*

**It became an idiom, which is the strongest evidence that it was a real
ruling.** Two later authors reached for it by number:
- `rulings.md:6437` (**ruling 223**, ratified 2026/08/16, `cc5e282`):
  *"No assertion is weakened — the premise loses a confound, which is the
  repair ruling 219 made to the mobility fixture for the same reason."* Its
  code site is `tests/story_reliability.rs:688`.
- `src/core/connection/tests_contested.rs:1543` (round 40-B's blind author,
  `ccd9678`): *"§12.4 defers that ACK to its timer for a first ack-eliciting
  packet, which is settled here rather than assumed (ruling 219's migration,
  in this file's own idiom)."*

**What remains UNKNOWN.** Nothing material about the decision. Its
**generality** is the same open question as 218's: 219 is stated as a
concrete repair, and rulings 223 and the round-40 author both treated it as
precedent, but **no general rule was written and none is reconstructed
here.** No `SPEC.md` change; no behavioural change.

**Confidence: HIGH.** Two stamped code sites, a commit message stating the
decision and its measurement, and a later *ratified* ruling (223) citing it
by number and restating its principle.

---

### 220 [RECONSTRUCTED 2026/08/18] — F1 is closed by **suppression**, not by arming in the future: the blind premise is **inverted**, and the re-arm burden gets the assertion neither blind agent could have been asked for

**[Taken 2026/08/16 at `386359f`.]**

**The disagreement.** F1 (the keepalive livelock) was closed twice, by two
agents blind to each other, in two different ways:

- **T2, the blind test author**, expected the beacon *armed* at `fire`, then
  fired, refused, and re-armed in the future — two tests asserted
  `timer(PersistentKeepalive) == Some(fire)`.
- **I1, the implementer**, closed it the other way: one
  `keepalive_can_leave()` predicate gates `transmit_keepalive`'s guard **and**
  `sync_liveness_timer`'s arming, so while the hold is on **there is no beacon
  deadline at all** (`4f08786`, *"F1 — the keepalive livelock"*).

**Ruling, in three parts.**

1. **Suppression is the shipped behaviour, and it is the stronger of the
   two.** Both prevent the spin; *"there is no wake to waste, and the state
   this file was written to catch becomes **unreachable by construction**
   rather than merely survivable. That is why the assertions below now pass
   easily — not because the test decayed, but because the defect class is
   gone."*
2. **The premise is inverted, not deleted** — `Some(fire)` becomes `None`,
   with a message naming the new reason (*"the budget admits nothing at room
   0, so no beacon deadline is armed at all — F1's spin is unreachable rather
   than survivable"*) — *"for exactly the reason this author gave it: a
   vacuous pass and a correct one must stay distinguishable."*
3. **Suppression moves a burden T2's design did not carry, so the integrator
   adds an assertion neither blind agent could have been asked for.**
   *"**An armed-in-the-future beacon is self-healing; a suppressed one is only
   as good as whatever re-arms it.**"* The new test is
   `the_beacon_returns_when_the_hold_lifts` (`tests_livelock.rs:975–1035`): at
   room 0 the beacon is `None`; an authenticated, window-fresh packet from the
   anchor credits `3 × len` (§7.3, ruling 169); the beacon must be
   `is_some()` again and not retrospective.
   **Working rule 9 is applied explicitly**: the degenerate build it separates
   is *suppress and never re-arm* — a silently disabled keepalive on a
   connection that explicitly configured one — and *"every other assertion in
   this file passes against that build, including all of section 2's, since
   'no deadline in the past' is satisfied most easily by no deadline at
   all."*

**Evidence trail.**
- `src/core/connection/tests_livelock.rs:322` and `:483` — the two
  `**[Integrator, ruling 220 — premise inverted, and the test is stronger for
  it.]**` blocks, on
  `beacon_refused_by_the_budget_does_not_re_arm_in_the_past` and
  `beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past`.
- `src/core/connection/tests_livelock.rs:977` — the long
  `**[Integrator, ruling 220.]**` note on
  `the_beacon_returns_when_the_hold_lifts`, quoted above.
- `git show 386359f -- src/core/connection/tests_livelock.rs` — both
  inversions (`Some(fire)` → `None`) and the +69-line addition.
- `386359f`'s commit message, item 220, verbatim: *"T2 expected the beacon
  armed-in-the-future while held; I1 suppresses it entirely via one
  keepalive_can_leave() predicate. Both stop the spin; suppression is stronger
  because the state becomes unreachable rather than survivable. But it makes
  the re-arm path load-bearing in a way T2's design did not, so the integrator
  adds the assertion neither blind agent could have been asked for."*
- The mechanism being ruled on: `4f08786`'s message — *"`keepalive_can_leave()`
  is one predicate used by both `transmit_keepalive`'s guard and
  `sync_liveness_timer`'s arming, so neither keepalive is armed at an instant
  already passed. `Liveness` is not suppressed. `apply_live` now re-syncs after
  the frames, because the two holds lift on a received packet and
  `handle_datagram` syncs before the frames are applied."*

**What remains UNKNOWN.**
- **Whether the two closures were ever weighed as a design choice rather than
  ratified after the fact.** The record states suppression is stronger and
  gives a reason; it does not record whether armed-in-the-future was
  considered and rejected on its merits, or simply lost because the
  implementer's code had already landed. **Not recorded, not reconstructed.**
- No `SPEC.md` amendment is stamped 220; §7.5's beacon text is untouched by
  either `f3a4576` or `386359f`.

**Confidence: HIGH** on the decision, the reason and the added assertion —
three stamped sites plus a commit message stating all three parts. **MEDIUM**
on the counterfactual above.

---

## Phantom check

**None of the five is phantom.** Each has at least one code site stamped with
its number *and* a commit whose message states a decision matching that site:

| # | Sites | Deciding commit | Message names it? |
|---|-------|-----------------|-------------------|
| 216 | 1 (`mod.rs:103`) | `f3a4576` | by content, not by number |
| 217 | 8 (`mod.rs` ×6, `core/mod.rs`, `tests_livelock.rs`) | `f3a4576`, settled `386359f` | **yes**, in both subjects |
| 218 | 1 (`tests_livelock.rs:110`) | `f3a4576` | by content, not by number |
| 219 | 4 (`tests_path.rs` ×2, `tests_contested.rs`, `story_reliability.rs`) | `386359f` | **yes** |
| 220 | 3 (`tests_livelock.rs`) | `386359f` | **yes** |

216 is the thinnest — a single citation, and the number appears in no commit
message — but the code comment it stamps is a complete decision with its own
rationale, landed in a diff, and the commit message states that decision in
prose. It is under-cited, not invented.

## Residue the gap also swallowed (not a ruling, but recorded nowhere else)

`386359f`'s **REMAINING** paragraph diagnosed but did not fix the s12 failure
(*"three land on one instant, so the doubling assertion reads 0ns/0ns … it is
a slice-5 story and wants care, not a hasty edit"*). That one **did** reach
the record — as **ruling 223** (`rulings.md:6416`, `cc5e282`), which cites
ruling 219 as its precedent. So the gap's only permanent losses are 216–220
themselves.

---

## Raw evidence log

### E1 — commit inventory (slice 7/7b era, all 2026-08-16)

```
677dce6 14:05:35  Ruling 215: 212(c) reversed in its rank half; T1's blind tests merged
4f08786 14:11:53  Slice 7b (I1): ruling 208's path frames, A2, A3, F1, 212(c) and 212(f)
b4576db 14:13:58  Merge branch 'worktree-agent-a2e2da818396946cb'
f3a4576 14:46:24  Slice 7b integration: ruling 217, 720/5 (was 16 failing)
386359f 15:34:05  Slice 7b: 993 passing, 1 failing -- ruling 217 settled, F1 closed by suppression
1b0be95           Ruling 221: a lost PATH_CHALLENGE is asked again by the PTO
337ce26           Ruling 222: scope the core dead-code allow to non-test builds
cc5e282           Rulings 223, 224: s12's detector, and the boundary comment 217 reversed
```
`git show --stat` shows **no `rulings.md` hunk in either `f3a4576` or
`386359f`** — the two commits that name 217, 219 and 220. `677dce6` (215)
touches `rulings.md`; `1b0be95` (221) does.

### E2 — the citation index

```
src/core/mod.rs:37                       "it hid ruling 217's abandoned"
src/core/connection/mod.rs:103           "[ruling 216] These two declarations sat commented here since slice 6"
src/core/connection/mod.rs:2259          "[ruling 217] self.ack.is_owed() belongs in this"
src/core/connection/mod.rs:2299          "not the manufacture ruling 217's first draft attempted"
src/core/connection/mod.rs:2391,2396     "ruling 217's first draft did…"; "…stood arguing the old position"
src/core/connection/mod.rs:2638          "[RATIFIED 2026/08/16 — ruling 217] The challenge is owed by"
src/core/connection/mod.rs:2665          "[ruling 217, amended] A *response* we owe is manufactured"
src/core/connection/mod.rs:2732          "([ruling 217], and the call at the end of …)"
src/core/connection/mod.rs:3190          "Ruling 221's deletion of ruling 217's"
src/core/connection/tests_livelock.rs:110   "[Integrator, ruling 218 — premise migrated, not weakened.]"
src/core/connection/tests_livelock.rs:180   "[Integrator, ruling 217] An unvalidated address offers its"
src/core/connection/tests_livelock.rs:322,483 "[Integrator, ruling 220 — premise inverted…]"
src/core/connection/tests_livelock.rs:977   "[Integrator, ruling 220.]" (the re-arm-burden note)
src/core/connection/tests_path.rs:175       "[Integrator, ruling 219] Settle §12.4's delayed-ACK timer before"
src/core/connection/tests_path.rs:691       "[Integrator, ruling 219 — trigger migrated, assertion untouched.]"
src/core/connection/tests_contested.rs:1543 "ruling 219's migration, in this file's own idiom"
tests/story_reliability.rs:688              "the same repair ruling 219 made to the mobility fixture"
rulings.md:6307,6358,6437,6452              221 and 224 recapping 217 and 219
```
**No `SPEC.md` line is stamped 216–220** (`grep -n "ruling 21[6-9]\|ruling 220" SPEC.md` → empty).
**No citation of ruling 216 outside `src/core/connection/mod.rs:103`.**

### E3 — never-written proof

```
$ for n in 216 217 218 219 220; do git log --all -S "### $n" -- .spec-v2-clean-slate/rulings.md; done   → empty
$ git log --all -S "### 217"                                                                            → empty
```

### E4 — the sweep that read past the gap

`.spec-v2-clean-slate/SWEEP-round40.md:496–527`, section *"rulings.md — 212 /
215 / 217"*, items 43–47: quotes `rulings.md:6204` (215's title), `:6232–6237`,
`:6243–6246`, `:6247–6252`, then jumps to `:6320` (inside 221) with no remark
on the missing entries.

### E5 — the contract clause 217 overrode

`.slices/07b-remediation/CONTRACT-7b.md:223–256` (§1.4 Retransmission):
*"All four boundaries the current code documents at `mod.rs:1968-2009` carry
over **unchanged in force** and changed in wording: (1) nothing owed ⇒ no
challenge; (2) a bare ACK with nothing else owed ⇒ no challenge; (3) not
exempt from §14.5's window; (4) sealed `seal_quiet`, never marking …"* — and
the branch it guards, `if (probe || validate) && !elicits { packing.ping(); }`.

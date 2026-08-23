# slither — project instructions

## The spec is the authority (hard rule)

**`SPEC.md` is the ratified protocol for slither's wire — version 1, the
first released wire.** Ratified 2026/08/14 after 80 rulings across ten
rounds. Every wire constant, layout, timer value and behaviour in it is
frozen, and **the code must match the spec** — never the other way round.

Do not change a ratified constant, a header layout, a frame type, or a
timer without an explicit ratification decision from the maintainer,
recorded in `.spec-v2-clean-slate/rulings.md`.

**The record directories are untracked** (maintainer decision,
2026-08-20): `.spec-v2-clean-slate/`, `.spec-v2-pipeline/` and
`.slices/` live on the maintainer's machine and in git history before
the removal commit, ignored by git ever since. `rulings.md` is still
written to — the rule above stands — but a **worktree-isolated agent
cut from a commit cannot see any of it**: a brief that needs ruling or
evidence text must quote it inline. Doc comments in `src/` and `tests/`
citing `.slices/...` paths are historical provenance, valid on the
maintainer's machine and at pre-removal commits.

**`rulings.md` is the record of *why*.** The spec states what the
protocol is; the rulings state what was rejected and for what reason. If
you are about to propose something, check there first — many attractive
ideas are already declined with their reasoning, and re-proposing one as
new wastes a round. It also records the errors corrected along the way,
including several of mine.

**Wire pins.** `IK_MSG1_LEN` 174 · `IK_MSG2_LEN` 81 · `INIT_PACKET_LEN`
196 · `RESP_PACKET_LEN` 107 · `VERSION` `0x01` · `PROLOGUE`
`b"slither\x01"`. The golden-wire vectors and the size/constant tests pin
these: any change that moves a wire byte turns a test red. **That is by
design — treat such a red as "this needs a ruling", not "update the
expectation."**

*Historical, not authoritative:* the v0.1 wire's spec is archived at
`.spec-v2-clean-slate/SPEC-v0.1-wire-historical.md`, and the v0.1
implementation is in git history at `5324ce5`. Neither governs current
code. Do not consult them for current behaviour.

## What is being built

A clean rewrite against `SPEC.md`, planned in `PLAN.md` as ten vertical
slices. **`STORIES.md`'s 34 approved capability stories are the
acceptance criteria**: a slice is done when its stories are
paused-clock tests that pass, not when its code compiles.

*(30 until ruling 209 approved S31–S33 into `STORIES.md` §I; 33 until
ruling 252 approved S34 into §J, 2026/08/17. This sentence read "30" for
a full slice afterwards — rule 4's shape, in the one file rule 3 already
records as the document nothing sweeps but deliberate intent.)*

## Crypto rules

- **Every Noise/curve operation flows through `hiss`.** slither declares
  its IK handshake via the `noise!` macro and rides the datagram
  transport (`DatagramSend`/`DatagramRecv`); it never touches curve or
  AEAD primitives itself.
- **The one raw primitive is mac1's keyed BLAKE2b**, taken from
  `cryptoxide` directly (the raw-primitive rule: it is a keyed hash over
  public data, not session cryptography). slither's `cryptoxide`
  requirement is pinned to **exactly the range hiss uses**
  (`>=0.6.3, <0.7` as of hiss 0.4.1) — verify against hiss's Cargo.toml
  when bumping either.
- **`rand_core` must match the line hiss's public bounds name** (0.10 as
  of hiss 0.4.1; `hiss::rand_core` re-exports it). Two rand_core majors
  in one graph produce an unsatisfiable `CryptoRng` bound, not a version
  error.
- **No RustCrypto crates** for any slither cryptography.

## Architecture invariants

- **Two sans-io cores plus one shell.** `core::Endpoint<I: Identity>` and
  `core::Connection` are pure state machines: `now: Instant` is an
  argument on every mutating call and the cores never read a clock.
  Every mutating call is followed by draining `poll_output()` to the
  terminal `Timeout(Option<Instant>)` (§16.4).
- **The shell is a single `!Send` actor**, spawned with
  `tokio::task::spawn_local`; consumers run it on a current-thread
  runtime inside a `LocalSet`. **Do not add `Send` bounds to the actor
  path** — a DH provider is not required to be `Send`, and an iOS Secure
  Enclave key is the story that forces it (S21).
- **The handle seam is split by cost (§16.3, ruling 53).** Command
  channel plus oneshot for the endpoint and staged verbs, where §6.2
  requires the DH to land on the driver task. Shared cell
  (`Rc<RefCell<_>>`) plus waker maps for the connection data path and
  the accessors. Data-path verbs are written **once** as `poll_*`; the
  `async fn` is `poll_fn` over it and `AsyncWrite` is the same function
  with its error mapped.
- **An adapter never claims ahead of its consumer** (§16.11, ruling 58).
  One item, only inside `poll_next`. Anything else rebuilds the
  unbounded shell queue §10.6 forbids, while looking like an ergonomic
  convenience.
- **The whole protocol is drivable without a kernel**: two endpoints over
  `testutil::FlakyWire` on tokio's **paused clock**, every timer
  resolving in virtual time. New behaviour gets a paused-clock flow
  test, **not a sleep**.
- `slither` is independent: **zero `bubble-*` dependencies**, everything
  resolves from crates.io.

## Working rules for agents

These are not style preferences. Each one is a lesson from a failure in
the spec rounds.

1. **Never read `SPEC.md` whole.** It is ~5 600 lines and an agent
   already died of context exhaustion ingesting it. Work from the spec
   sections **quoted into your brief**; if you need more, use targeted
   `grep -n` plus `Read` with offset/limit. §9.8 is 140 lines.
2. **Create the output file before reading anything**, then append as you
   go. Not "write incrementally" as an aspiration — the file exists, with
   its heading skeleton, *before the first `Read`*. A slice-2 planning
   agent stalled after ten minutes having read a great deal and written
   nothing; all of it was lost, and its own last words were that it was
   **about to** write the skeleton. An agent that intends to persist
   later has not persisted. If you have read something worth keeping, it
   belongs on disk before you read the next thing.
3. **When you find two statements in conflict, do not default to the
   code-like rule.** **Eight times** in this project the prose held the
   correct intent and the formal rule held the bug — most recently ruling
   169, where §7.3 funded a security counter from "§7.2's authenticated
   class" while §7.2's own invariant is authenticated *and window-marked*,
   so on the literal text a replayed packet replenishes it; and ruling
   168, where §7.3's *"the 3× ratio is never lifted"* contradicted four
   sections describing an address that "validates by traffic". Before
   those: ruling 154 (§10.3 listed five retirement triggers and cited §9.7
   for the fifth, which §9.7 does not contain), ruling 131 (§13.2's prose
   named the survivors, its own parenthetical ranged over every in-flight
   packet), and ruling 69 (§6.3's evict-oldest rule said "by park time"
   while the rationale two bullets above promised a retransmitting peer
   "the same per-packet race as any fresh initiator", which park-time
   ordering inverts exactly).
   Report the conflict; do not silently pick one. **Every agent that has
   reported rather than resolved has been right.**
   *This count was itself stale for three slices* (ruling 158): it read
   "three times … ruling 69" while briefs cited five. Rule 4 applies to
   this file too — and this file is the one document no slice ever puts on
   an agent's path, so nothing sweeps it but deliberate intent.
   **But the rule is *do not default*, not *always take the prose*, and a
   one-directional tally decays into exactly the reflex it warns against.**
   Ruling 182 is the first case that went the other way: §7.5's prose
   stated the passive keepalive over *"has not sent"* while its formal
   definition read `S` = *marking sends only*, and the **formal** one held
   the intent — because the beacon's soundness proof two subsections later
   rests on *"every send that can establish `S > R` is a marking send, so
   the death clock is armed there"*, which the prose reading collapses into
   an immortal half-open session. The tiebreak that decided it, and the one
   to reach for: **follow the statement some other proof depends on.**
4. **Grep for the rationale, not only the token.** A verification that
   greps for a changed value will miss prose still arguing the position
   you reversed. This happened, and shipped a self-contradicting section.
   **Two companions, both bought at full price:**
   *(a) When you correct one clause of a sentence, read the other clauses
   of that sentence.* Ruling 182 rewrote §7.5's passive keepalive rule
   from "has not sent" to "has not made a marking send" — and left the
   other half of the same sentence stating `R > S` where the prose said
   *"has received since it last sent"*. The two differ exactly when the
   instants coincide, which the driver's once-per-turn `now()` makes
   ordinary, and the result was a connection that neither talked nor died
   (ruling 195). The defect was inside the sentence being edited.
   *(b) Apply this rule to `rulings.md` too, not only to the spec.* A
   ruling that reverses another's conclusion inherits the duty to address
   its **reasoning**. Ruling 175 corrected ruling 43's number and silently
   reinstated the characterisation 43 had explicitly denied; a blind test
   author caught it by grepping the record rather than the spec — the
   first time anyone had (ruling 188).
5. **If your brief tells you to do something that looks wrong, say so
   rather than doing it.** An agent that declined a blanket instruction
   in round 8 was right, and its refusal became ruling 51's guard.
6. **The test author is not the implementer** for story-level acceptance
   tests. One author writing both can make both wrong in a mutually
   consistent way, and CI stays green. Write the test from the story and
   its spec section first; implement against it.
   **Parallel agents must own disjoint file paths — no exceptions.** In
   slice 2a the implementer's placeholder stub overwrote the independent
   author's 68 tests, because both briefs named `src/core/tests.rs` and
   the finish order decided who won; in slice 1 the same overlap existed
   and the order happened to favour the tests, which is why it went
   unnoticed. If the implementer needs a module to compile against, it
   lands `#[cfg(test)] mod tests;` **commented out under an integration
   header** and **creates nothing** — the file is the test author's alone,
   and the integrator uncomments the declaration when it arrives. A brief
   that hands two concurrent agents one path has a race in it, and "it
   worked last time" is what a race looks like from the outside.
   **[Amended by ruling 211.]** This clause said "declares … and creates
   nothing", which contradicted rule 15: **a `mod` declaration naming a
   missing file is a compile error** — `Cargo.toml`'s `[[test]]` failure
   one layer down, and rule 15 exists for exactly that. The contradiction
   had already been discharged by hand without being recognised: slice 7's
   implementer commented its declaration out to run its gates, restored it
   before committing, and reported that its gates had run without that
   module. It did the right thing and nothing in the rules told it to.
   The partition of **paths** below is untouched and remains absolute;
   only the instruction about the declaration changed.
7. **Do not report a gate as green without running it.** Paste the
   command and its output.
8. **A list in the spec is read as exhaustive whether or not it says so.**
   §16.4's core API omitted the stage-0 accessors and the omission was
   invisible until someone built against it (ruling 71); ruling 64 said
   "two things this rule does not reach" when there were three; §2.3 wrote
   `TAG` beside `PK` as though both varied per suite (ruling 68). Seven
   rulings across two slices share this one shape — **a stated
   construction with an unstated or contradicted scope**. It is the most
   productive defect class this project has. Hunt it deliberately: when
   the spec introduces a symbol, a parameter or a list, ask what bounds
   it, and whether the text says.
9. **A bound is only a test if the degenerate case violates it.** An
   upper bound that the collapsed implementation satisfies for free
   asserts nothing. Slice 2a produced two: "these two packets differ"
   passed a core reusing one ephemeral, because the sender index differs
   anyway; and "no interval exceeds base + jitter" passed a core with no
   jitter at all. Both were named for the property they failed to pin —
   **a name is not a pin**. Ask what the *broken* version would do, and
   assert from the side that separates them: not all intervals equal, not
   just none too large. This is distinct from slice 1's one-sided
   boundary (`LEN` and `LEN-1` tested, `LEN+1` not), and both are worth
   checking for.
10. **Commit before mutating.** Mutation testing reverts with
   `git checkout <file>`, which silently discards *any* uncommitted work
   in that file — including edits made for a different reason. I lost two
   accessors this way mid-review. Either commit first, or revert
   surgically.
11. **A ruling's rationale must name a mechanism that exists — check it
   against the code, not only the spec.** Rulings 87 and 89 were made one
   day apart, both correct in conclusion, and both justified by a
   mechanism that was not there: 87 said "`connect()` performs no DH",
   describing a core factoring the frozen code does not have; 89 said
   `SessionId` was "reachable from the seal half slither already holds",
   which is true of hiss's concrete type and false of slither's
   `Handshake::Seal`, an associated type with no bounds. Each cost real
   work — 87's cost a shell-side mirror of a security invariant and
   became ruling 90. **Ruling 64 already recorded that a rationale is not
   reviewed by the act of ratifying its rule.** These are the same defect
   in the maintainer's own text. Before a rationale ships, open the file
   it describes — **and ask what else the mechanism you are naming has to
   be true of.** This rule was written after 87 and 89 and still did not
   prevent 90, whose defect is an **absent** clause rather than a false
   one: reading the code confirms what the rationale says and cannot
   surface what it fails to say.
   **Generalised after two failures in one round (rulings 205, 206):
   *whichever artefact a claim is about is the artefact that must be
   opened* — code, spec, or the ruling record.** The rule said "check it
   against the code" because 87 and 89 were claims about code; both new
   failures were claims about *documents*, and both slipped the letter
   while sitting squarely inside the intent. 205 asserted that §16.2 still
   wrote a signature the code had replaced — §16.2 had been amended when
   the decision was taken, and one `grep` refutes it. 206 asserted that
   ruling 60's attestation had an unstated scope, having read `lib.rs`'s
   *paraphrase* of ruling 60 rather than ruling 60, which states the scope
   by reference to §18.2 — a section neither read. **A citation is a claim
   about the cited text**: a ruling citing another ruling is unchecked
   until that ruling is read, and one citing a section is unchecked until
   the section is read.
   Two further things this pair demonstrated, both worth more than the
   errors. *(a)* Both were produced **alongside sound work** — 205 was the
   tenth question in an API review whose other nine were built by reading
   every `pub fn` in the crate. Nine sound findings are exactly the
   conditions under which the tenth is not checked; thoroughness in one
   artefact reads, from the inside, as licence to reason about a second
   from memory. *(b)* 206's **conclusion was right and its argument was
   wrong** — the inverse of rule 12, and invisible to any review that
   checks conclusions. A finding is not verified by agreeing with it.
12. **A verification is only as good as its applicability: a true lemma
   about the wrong state proves nothing.** The seam review ran two agents
   blind to each other, and they disagreed on fact twice. Both times the
   one who was wrong had verified something **true** — that
   `core::Endpoint::accept()` guards against an existing static (it does;
   it just does not fire when the accept is processed first), and that
   `poll_output()` is idempotent *at* `Timeout` (it is; the question was
   whether the cores **are** at `Timeout` when `deadline()` runs). Either
   reviewer alone would have shipped a wrong verdict with a clean
   argument attached. **When a review clears something, ask what state
   the argument assumed — not whether the argument is sound.**
13. **The fixture bounds the coverage.** Two of the four seam-review
   findings were unreachable from all 451 tests **by construction**,
   because `FlakyWire` models everything a *network* does and nothing a
   *socket* does: a fabric that loses, delays, duplicates and reorders
   could not, at review time, express "this send fails" or "this driver
   panics". Ruling 49 later added `FlakyPolicy::send_failure`
   (`ENETUNREACH` injection, toggleable mid-run), closing the first gap;
   "this driver panics" remains unexpressed today (ruling 264). No
   amount of test-writing against the fixture as it stood would have
   found either at the time. When a whole class of
   fault is absent from the results, suspect the harness before the
   authors.
14. **An isolated agent sees a commit, not a working tree — cut its
   worktree from a commit that contains its brief's inputs, name that
   commit in the brief, and *verify the cut actually happened*.**
   **The verification half is not optional** (ruling 200's round): slice
   7's third blind author was briefed at `195c57a` and its worktree was
   cut from `ddda950`, which contained the very implementation it existed
   to be blind to. It caught this on its **first command**, before reading
   any source, reset, and said so — and nothing in its output would have
   looked wrong had it not. A brief that names a commit and tooling that
   cuts from `HEAD` destroy the blind split **silently**. An isolated
   agent's first act is to check its base and report it.
   Worktree isolation fixed slice 3b's blindness
   leak and introduced this in doing it. Slice 4a's test author was cut at
   `fdf5972`; `CONTRACT-4a.md`, which its brief calls *binding*, was
   uncommitted at that moment and landed at `74fa5f2`. It reconstructed
   the API for ten minutes and was rescued only because an unrelated
   mid-flight message revealed the file existed. Two of its guesses were
   **semantic**: an event field name, and **no `Ok(Some(0))`/`Ok(None)`
   distinction for `read` at all** — the convention whose inversion hangs
   a reader forever on a finished stream. Without that accident, 73 tests
   would have been written against a guessed API and the integration would
   have read as a design disagreement rather than a missing file. **Commit
   the brief's inputs before dispatching, not after.**
15. **A file whose contents are only valid once *both* blind agents' work
   exists belongs to the integrator — and the briefs must say so.** This
   is the mirror of rule 6. Rule 6 partitions paths so two agents never
   write one file; rule 15 names the residue it leaves behind — the file
   *neither* can validly write alone. Slice 4b's `Cargo.toml` is the
   case: cargo does not warn about a `[[test]]` whose file is missing, it
   **refuses to parse the manifest**, so an implementer adding live
   stanzas for its partner's not-yet-existing test files commits a tree
   on which *no gate can run at all* — while rule 7 forbids reporting a
   gate green without running it, and creating placeholder test files is
   the slice-2a accident that destroyed 68 tests. The implementer landed
   them commented out under an integration header and asked whose job it
   was. It is the integrator's.

16. **When you inspect the tree while another agent holds it, read from
   the commit — `git show <base>:<path>` — never from the working copy;
   and never `git add -A` while another agent is writing.** Ruling 166:
   the integrator read a file out of the shared main tree mid-slice, saw
   the implementer's *uncommitted* guard, and ruled that a check "already
   exists" and that the planner's contrary claim was "all wrong". It did
   not exist; the planner was right. Then `git add -A` swept that
   uncommitted work into the integrator's own commit, so `git log -S` now
   names the wrong origin for it. Two rulings (159, 160) came from that one
   contaminated read, and one of them publicly faulted an agent that had
   been correct. This is rule 12 — *a true lemma about the wrong state
   proves nothing* — and rule 10 — *commit before mutating* — both failing
   on the person enforcing them.

## Release gates (hard rules)

A release may be cut **only when every gate below is green on the exact
commit being released**. If a gate fails, the release is blocked until it
is fixed, not deferred to "the next patch."

| Gate | Command | Bar |
|------|---------|-----|
| Compiles | `cargo build --all-features --all-targets` | clean build |
| Format | `cargo fmt --all --check` | no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| Docs | `cargo doc --no-deps` and `--all-features`, `RUSTDOCFLAGS=-D warnings` | no broken intra-doc links |
| Tests | `cargo test` **and** `cargo test --all-features` | all pass |
| Release tests | `cargo test --release --all-features` | all pass |
| Wire pins | the golden-wire and size/constant tests (run under `cargo test`) | byte-identical |
| MSRV | `cargo +<MSRV> check --all-features --all-targets` | passes on the declared MSRV |
| Supply chain | `cargo deny check` | clean |

These mirror the CI pipeline (`Check` → `Test`, plus the daily `Audit`
cron). CI green on the release commit satisfies every gate.

**Every slice ends on the full table, not just `cargo test`.**

### MSRV policy

The MSRV is declared in `Cargo.toml` (`rust-version`) and pinned by the
`msrv` CI job — keep both in lockstep, and in lockstep with **hiss's
MSRV** (same policy: a recent stable floored at `stable − 3`). Currently
**1.96**.

### Lockfile

`Cargo.lock` is **not** committed (hiss's convention): every CI run and
local gate run re-resolves from the index, so a breaking change shipped
inside a semver-compatible range surfaces at the next run instead of
hiding behind a months-old pin.

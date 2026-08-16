# slither — project instructions

## The spec is the authority (hard rule)

**`SPEC.md` is the ratified protocol for slither's wire — version 1, the
first released wire.** Ratified 2026/08/14 after 80 rulings across ten
rounds. Every wire constant, layout, timer value and behaviour in it is
frozen, and **the code must match the spec** — never the other way round.

Do not change a ratified constant, a header layout, a frame type, or a
timer without an explicit ratification decision from the maintainer,
recorded in `.spec-v2-clean-slate/rulings.md`.

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
slices. **`STORIES.md`'s 30 approved capability stories are the
acceptance criteria**: a slice is done when its stories are
paused-clock tests that pass, not when its code compiles.

## Crypto rules

- **Every Noise/curve operation flows through `hiss`.** slither declares
  its IK handshake via the `noise!` macro and rides the datagram
  transport (`DatagramSend`/`DatagramRecv`); it never touches curve or
  AEAD primitives itself.
- **The one raw primitive is mac1's keyed BLAKE2b**, taken from
  `cryptoxide` directly (the raw-primitive rule: it is a keyed hash over
  public data, not session cryptography). slither's `cryptoxide`
  requirement is pinned to **exactly the range hiss uses**
  (`>=0.6.0, <0.7` as of hiss 0.3.2) — verify against hiss's Cargo.toml
  when bumping either.
- **`rand_core` must match the line hiss's public bounds name** (0.10 as
  of hiss 0.3.2; `hiss::rand_core` re-exports it). Two rand_core majors
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
   code-like rule.** **Six times** in this project the prose held the
   correct intent and the formal rule held the bug — most recently ruling
   154, where §10.3 listed five retirement triggers and cited §9.7 for the
   fifth, which §9.7 does not contain; and ruling 131, where §13.2's prose
   named the survivors and its own parenthetical ranged over every
   in-flight packet. The pattern before those was ruling 69, where §6.3's
   evict-oldest rule said "by park time" while the rationale two bullets
   above promised a retransmitting peer "the same per-packet race as any
   fresh initiator", which park-time ordering inverts exactly.
   Report the conflict; do not silently pick one. **Every agent that has
   reported rather than resolved has been right.**
   *This count was itself stale for three slices* (ruling 158): it read
   "three times … ruling 69" while briefs cited five. Rule 4 applies to
   this file too — and this file is the one document no slice ever puts on
   an agent's path, so nothing sweeps it but deliberate intent.
4. **Grep for the rationale, not only the token.** A verification that
   greps for a changed value will miss prose still arguing the position
   you reversed. This happened, and shipped a self-contradicting section.
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
   declares `#[cfg(test)] mod tests;` and **creates nothing** — the file
   is the test author's alone. A brief that hands two concurrent agents
   one path has a race in it, and "it worked last time" is what a race
   looks like from the outside.
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
   cannot express "this send fails" or "this driver panics". No amount of
   test-writing against it would have found them. When a whole class of
   fault is absent from the results, suspect the harness before the
   authors.
14. **An isolated agent sees a commit, not a working tree — cut its
   worktree from a commit that contains its brief's inputs, and name that
   commit in the brief.** Worktree isolation fixed slice 3b's blindness
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

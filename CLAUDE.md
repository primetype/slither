# slither — project instructions

## The spec is the authority (hard rule)

**`SPEC.md` is the ratified protocol for slither's wire — version 1, the
first released wire.** Ratified 2026/08/14 after 63 rulings across ten
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
2. **Write notes incrementally**, so partial progress survives a
   failure. Do not batch all findings to the end.
3. **When you find two statements in conflict, do not default to the
   code-like rule.** Twice in this project the prose held the correct
   intent and the formal rule held the bug. Report the conflict; do not
   silently pick one.
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
7. **Do not report a gate as green without running it.** Paste the
   command and its output.

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

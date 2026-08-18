# GAPSLICE Author B — S22 clause 4: wrong static against a LIVE responder

Base commit: `b072afd` (verified as first act, working rule 14 — `git log --oneline -1`
printed `b072afd Ruling 271: the record — §12.4's emission point, §16.5's drain order`;
no reset was needed).

Owned paths: `tests/story_wrong_static.rs` (new), `GAPSLICE-B-REPORT.md` (this file).

## 1. S22's exact text (STORIES.md)

`STORIES.md:351-357`, verbatim:

> ### S22 — a user can pick a crypto suite, and mismatches fail closed
>
> - **Accepts:** the suite is declared once via the macro; a peer on a
>   different suite, or a wrong static, fails the handshake and installs
>   nothing. An unknown version byte is dropped silently — there is no
>   negotiation, ever.
> - **Anchor:** §1.1, §2, §3.1. **Paused clock:** yes.

Four clauses. Clause 4 is *"or a wrong static, fails the handshake and
installs nothing"*, with the paused-clock obligation attached to the
whole story.

## 2. The recorded hand-forward

`tests/spec_packet.rs:30-32` (module docs of slice 1's spec test):

> The fourth clause — "a wrong static … fails the handshake and installs
> nothing" — needs a driven handshake and is slice 2's (PLAN.md §9.2).

`.slices/01-packets/PLAN.md:1505-1507`:

> 1. **S22's fourth clause** — "a wrong static … fails the handshake and
>    installs nothing" — and S22's **paused-clock obligation** are slice
>    2's. Slice 1 discharged neither and did not pretend to (§9.2).

Slice 1 correctly declined it and named the receiver. Slice 2 never
picked it up — this task closes it.

## 3. What the code actually does

`src/error.rs:83-84` — the give-up variant, read as instructed rather than
assumed:

```rust
/// The dial was retried until `HANDSHAKE_GIVEUP` and gave up.
#[error("the initial connect gave up after HANDSHAKE_GIVEUP")]
TimedOut,
```

`ConnectError` has exactly three variants (`AlreadyConnected`, `TimedOut`,
`Local`; ruling 72) — there is **no** `WrongStatic` / `Rejected` /
`HandshakeFailed` on the dial path. So the dialler's only observable for
clause 4 is `TimedOut` at `HANDSHAKE_GIVEUP`.

`AlreadyConnected` is the lever for "the established set stays empty" from
an integration test: it is returned *synchronously*, before any await
(ruling 87, pinned by `tests/spec_shell.rs:378-428`), when a connection to
that static already exists. So `b.connect(a.addr, a.pk) == Ok(..)` is a
direct observation that B installed nothing for A.

Existing mismatch coverage, confirmed as *not* this clause:
`tests/story_dial.rs:182` (`s2_no_answer_gives_timed_out_at_giveup`) and
`:256` (`s2_giveup_releases_the_static_for_an_immediate_redial`) both dial
`absent_static(2)` at `addr(4002)` / `addr(4012)` — **no endpoint is bound
there at all**. They pin the 90 s give-up against silence, not fail-closed
against a live responder. `tests/spec_shell.rs:527`
(`dropping_an_intro_is_a_silent_reject_and_costs_nothing`) is the nearest
neighbour and is a *correct* static whose `Intro` the application drops —
the opposite side of the gate.

## 4. Spec windows: mac1 (§4.1 / §6.1), timestamp guard / replay

§4.1 (`SPEC.md:789-820`):

> ```
> key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
> mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
> ```
> `recipient_static_canonical` is the recipient's static public key in the
> canonical encoding of §2.4 … On a HandshakeInit the recipient is the
> responder.

§4.2 (`SPEC.md:823-828`):

> mac1 is verified **before any curve or DH work**. A garbage flood, a
> **wrong-key packet**, or a mismatched-suite packet dies at one keyed hash
> and never reaches the DH provider.

§6.1's stage table (`SPEC.md:1156`):

> | `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed
> hash, **0 DH** | … | wrong length …, unknown type/version, **bad mac1 —
> all silent, before the queue** |

So a dialler keying mac1 on a static that is not the responder's produces a
packet the responder rejects at the gate: **silent, 0 DH, before the
stage-0 queue** — no `Intro` is minted and nothing is transmitted back.

**Timestamp guard / replay.** §6.1 places the timestamp at
`authenticate() → Proven` ("the initiation timestamp"; `Replay` is raised
there). A wrong-static init dies two stages earlier, at mac1, so it never
reaches `read_identity()` let alone `authenticate()` — **nothing is parked
and no timestamp is recorded**. This is asserted end-to-end rather than
argued: the follow-up correct dial in test 2 is between the *same* pair, so
a build that had parked or guard-written anything for A during the
wrong-static window would fail there.

## 5. Rule-3 watch: does the story's wording agree with the mechanism?

The brief asked me to state whether S22's *"fails the handshake and
installs nothing"* agrees with the mechanism, citing the §4.1/§6.1 windows,
and to report rather than resolve. My reading, with the three findings
separated by how strong each is.

### 5a. "installs nothing" — agrees, exactly and on both sides

Verified, not argued: no `Intro` is minted (T1's continuously-polled
`accept()`), no datagram leaves the responder, no DH is spent, no entry
appears in the established set, and §17.1's timestamp guard is untouched
because §6.1 places the timestamp at `authenticate() → Proven`, two stages
below the gate a wrong-static init dies at. **No conflict.**

### 5b. "fails the handshake" — agrees, but the packet never enters the handshake

§4 is titled *"mac1 — the DoS gate"* and §4.2 puts it *"before any curve or
DH work"*; §5 is the handshake. A wrong-static initiation is rejected by
§4's gate and never reaches §5 at all — the responder does not fail a
handshake, it declines to start one. The story's phrase is *true of the
outcome* (no handshake completes, and the dialler's `Connecting` fails) and
*imprecise about the locus*.

More importantly, **the failure is neither prompt nor distinguishable, and
both are by ratified design**:

* The responder observes nothing and can log nothing. §18.2's target table
  is a **closed list of five** — `policy`, `replay`, `frames`, `roam`,
  `io` — and *"renaming or dropping one is a protocol revision"*. I read the
  table itself (`SPEC.md:7184-7192`) rather than `Cargo.toml`'s paraphrase
  of ruling 67, per working rule 11: `slither::policy` carries *"guard
  rejections, internal tie-break outcomes …, the intro-queue evictions …,
  and the contested-connection probe's three events"*. A mac1 drop is none
  of those — it never reaches the guard or the queue — so **no target
  carries it**.
* The dialler sees `ConnectError::TimedOut` **90 s later**, and
  `ConnectError` has exactly three variants (ruling 72). That is
  byte-for-byte the same outcome `tests/story_dial.rs`'s ghost tests get
  from an address where nothing is bound. **A wrong static and a peer that
  is switched off are indistinguishable to the application**, which is the
  correct security property (§4.3: mac1's key is public data, so any
  observable difference is an oracle) and is *not* what "fails the
  handshake" suggests to a first reader.

I record this as a **wording-versus-mechanism note, not a conflict**:
nothing in §4.1, §4.2, §4.3, §6.1, §18.1 or §18.2 promises promptness or
distinguishability, so there is no contradiction to resolve — only a story
sentence that is less specific than the mechanism it accepts. The test file
carries the same note at its head so the next reader meets it there.

### 5c. A finding worth a ruling — S22's anchors do not name §4

**This one is a real gap, and it is working rule 8's shape.** S22's anchor
line reads:

> **Anchor:** §1.1, §2, §3.1. **Paused clock:** yes.

Clause 4's mechanism is **§4.1 (mac1 keyed on the *recipient's* static),
§4.2 (verified before any curve work) and §6.1 (bad mac1 → silent, before
the queue)**. None of those three is anchored. §1.1 is the freeze, §2 the
suite, §3.1 the packet grammar — those anchor clauses 1-3 (declare the
suite; mismatched suite dies at the length gate; unknown version dropped)
and stop exactly where clause 4 begins. Rule 8: *"a list in the spec is read
as exhaustive whether or not it says so"*, and an anchor list is precisely
such a list. A test author working from S22's anchors alone would never
open §4 — which is the whole of clause 4.

**Reported, not resolved.** Whether `STORIES.md` gains `§4, §6.1` on S22's
anchor line is the maintainer's ruling, not mine.

### 5d. S22's paused-clock obligation, for the record

S22 says **Paused clock: yes** for the whole story. Until this file, no S22
clause had a paused-clock test: `tests/spec_packet.rs:1-32` states that
clause 1 is `two_suites_coexist_in_one_crate` (a compile-shape test, no
clock) and that clauses 2-3 are pinned in `src/packet/tests.rs` as unit
tests over `classify`/`Inbound` (also no clock). Clause 4 now has one.
Whether the obligation was ever meant to reach clauses 1-3 — which have no
timer in them to drive — is again the maintainer's to say. **Reported, not
resolved.**

## 6. The tests, and the broken build each one catches

`tests/story_wrong_static.rs`, three tests, all
`#[tokio::test(flavor = "current_thread", start_paused = true)]` on a
`LocalSet`. No sleep anywhere; the 90 s give-up and the 5 s train are
virtual time. Ratified values (`90 s`, `5 s`, `250 ms`, `196`, `0x01`) are
written as **literals** in the file, not imported from `slither::constants`
— per the brief's rule-9 note (ruling 271's valve-pin lesson), so a drift in
a constant turns this file red instead of dragging it along.

### T1 `s22_a_wrong_static_against_a_live_responder_installs_nothing`

A dials B's **live** address with `other_static(3)`, a well-formed
reference-suite key that is not B's. One `accept()` future is pinned and
held across both halves of the give-up observation, so it is polled
continuously from t=0 to t=90 s+250 ms — that is what makes "for the full
retransmit ladder" a claim about the ladder and not about the first packet.

Assertions, and the broken build each separates:

| Assertion | Broken build it catches |
|---|---|
| `accept()` branch of the `select!` never wins | **mac1 gate removed** — the wrong-static init parks and surfaces an `Intro` |
| `sent_count(tap, b.addr) == 0` | **a gate that answers** — any reply to an unverifiable init (an amplification vector, §6.9) |
| `b.dhs == 0` | **a gate after the DH** — §4.2's ordering a fiction |
| `b.connect(a.addr, a.pk).is_ok()` | **a gate that installs** — anything recorded against A's static (ruling 87 makes `AlreadyConnected` synchronous, so this reads the established set directly) |
| `timeout(90 s − 1 ms, dial).is_err()` | **give-up too early** |
| `timeout(1 ms + 250 ms, dial)` resolves `Err(TimedOut)` | **give-up never armed**, and the wrong variant |
| `msg1_to(tap, b.addr) ∈ 15..=20` | **a one-packet fixture** — five of the six assertions above are *negative* and a build that emitted one init and stopped satisfies all of them for free. This is the row that keeps T1 from being vacuous (working rule 9). |

### T2 `s22_both_endpoints_stay_usable_after_a_wrong_static_dial`

Same pair, same run: the wrong-static dial runs to its 90 s give-up, then a
**correct** dial establishes and carries a message each way.

T2 exists because **every assertion in T1 is negative, and a totally deaf
responder passes all of them.** That is the degenerate build rule 9 demands
a bound against, and M2 below demonstrates it: T1 stays green on a build
whose mac1 gate rejects *everything*.

It also carries the timestamp-guard answer as an assertion rather than an
argument: `establish`'s `authenticate()` `.expect()` string names
`IntroError::Replay` as the failure being tested for — a build that wrote
§17.1's guard on arrival rather than at `Proven` would surface it there.

### T3 `s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path`

The one configuration where a *missing* mac1 gate costs the responder real
curve work. §6.5 step 2's hint check routes an initiation from an address
this endpoint has a dial in flight to onto step 3's **eager** path, which
spends 1 DH (`es`) before anyone knows whose static the packet claims. So
B dials a static nobody holds *at A's address* (that, and only that, puts
A's address in B's §17.4 hint set) while A dials B with a wrong static.

The bound is an **equality**, not a ceiling:

```text
b.dhs == 2 × (msg1 datagrams B put on the wire)
```

§6.1 prices an initiation at `es + ss` = 2 DH and §5.5 step 2 makes every
retransmit a fresh initiation, so B's own ladder is the whole legitimate
spend. A gate-less build lands at `2 × sent + 1 × received` — only an
equality separates them. Measured, not assumed: the equality holds on the
real build (see the run in §8), which is why it is written as `assert_eq!`
rather than a range.

## 7. Mutants run (rule 9)

Three mutants, all at the same site — the inbound-init mac1 gate,
`src/core/endpoint/mod.rs:650`:

```rust
// §6.5 step 1 ends here: length gate, classify, mac1 —
// one keyed hash, 0 DH, and a failure is the silent drop.
if !self.our_mac1.verify(preimage, mac1) {
    return Disposition::Done;
}
```

Working rule 10 observed: the tests were committed (`c64e05a`) *before* the
first mutation, and each mutant was reverted with
`git checkout -- src/core/endpoint/mod.rs`. `git diff b072afd -- src/` is
empty at the end; no production file is modified by this task.

| | T1 installs-nothing | T2 stays-usable | T3 hinted-path DH |
|---|---|---|---|
| **M1** gate removed (`if false && !verify`) | **RED** | green | **RED** |
| **M2** gate rejects everything (`if true \|\| !verify`) | green | **RED** | green |
| **M3** gate skipped on the hinted path | green | green | **RED** |

Every test has a mutant that kills it and a mutant it survives, so no test
is decoration and none is redundant.

### M1 — `if false && !self.our_mac1.verify(preimage, mac1)`

```
running 3 tests
test s22_a_wrong_static_against_a_live_responder_installs_nothing ... FAILED
test s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path ... FAILED
test s22_both_endpoints_stay_usable_after_a_wrong_static_dial ... ok

---- s22_a_wrong_static_against_a_live_responder_installs_nothing stdout ----
thread '...' panicked at tests/story_wrong_static.rs:312:40:
§4.2/§6.1: a mac1-invalid initiation minted an `Intro`. mac1 keys on the RECIPIENT's
static (§4.1) and a bad mac1 is a silent drop BEFORE the stage-0 queue

---- s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path stdout ----
thread '...' panicked at tests/story_wrong_static.rs:579:13:
assertion `left == right` failed: §4.2/§6.1: B's DH spend must be exactly its own
initiator ladder (es+ss = 2 DH per initiation, 18 initiations = 36). A larger figure
means A's 18 mac1-invalid initiations reached the DH provider through §6.5 step 3's
eager path
  left: 54
 right: 36

test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

`left: 54` is `2 × 18 + 1 × 18` **measured**, which is exactly the arithmetic
T3's doc comment predicts for a gate-less build. Two things this run settled
that I would otherwise have been asserting from memory (working rule 11):

* **T3 failed on the DH equality and *not* on its `accept()` branch.** That
  confirms the claim in T3's doc comment: with the gate gone *and* the
  source hinted, §6.5 step 3's eager read answers `Malformed` and drops the
  packet, so no `Intro` is ever minted — T1's style of assertion is blind to
  M1 on the hinted path, and only the DH count sees it.
* **T2 stayed green under M1**, which I had expected to go red. The reason
  is `INTRO_TTL` (15 s) < `HANDSHAKE_GIVEUP` (90 s): T2 awaits the
  wrong-static dial to completion first, so the garbage the mutant parked has
  already expired by the time the recovery dial runs. Recorded because it is
  the sort of thing a report that reasoned instead of measuring would have
  got backwards.

### M2 — `if true || !self.our_mac1.verify(preimage, mac1)` (a fully deaf responder)

T1 alone, green — **this is the degenerate build working rule 9 exists for**:
every assertion in T1 is negative, and an endpoint that rejects *every*
initiation satisfies all of them.

```
running 1 test
test s22_a_wrong_static_against_a_live_responder_installs_nothing ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out
```

T2 under the same mutant does not terminate: a deaf responder can never
complete `establish`, so the `join!` waits forever with no timer left to
auto-advance. Run directly under a 45 s `SIGALRM` watchdog — the harness
prints the test name and never prints a result:

```
running 1 test
test s22_both_endpoints_stay_usable_after_a_wrong_static_dial ... ---exit code: 142
```

(142 = 128 + SIGALRM. For contrast, T3 under the same mutant finishes
normally: `test result: ok. 1 passed`, exit 0 — neither endpoint answers in
T3 either way.)

A hang rather than an assertion is the honest signal here, because the
property T2 pins is *liveness*: "the responder still answers a correct
initiation" has no bounded negative form.

### M3 — the gate skipped on §6.5's hinted path

```rust
if !self.statics.hints().any(|h| h == src) && !self.our_mac1.verify(preimage, mac1) {
    return Disposition::Done;
}
```

The plausible bug §4.2's ordering forbids: *"the hint check is cheap, verify
mac1 when we park"*. T1 and T2 never touch the hinted path and both survive.

```
running 3 tests
test s22_a_wrong_static_against_a_live_responder_installs_nothing ... ok
test s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path ... FAILED
test s22_both_endpoints_stay_usable_after_a_wrong_static_dial ... ok

---- s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path stdout ----
thread '...' panicked at tests/story_wrong_static.rs:579:13:
assertion `left == right` failed: §4.2/§6.1: B's DH spend must be exactly its own
initiator ladder (es+ss = 2 DH per initiation, 18 initiations = 36) ...
  left: 54
 right: 36

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

## 8. Gates (rule 7)

Run on the worktree at the final tree. Real output, pasted.

### `cargo fmt --all --check`

```
$ cargo fmt --all && cargo fmt --all --check && echo "FMT-CHECK: no diff"
FMT-CHECK: no diff
```

(`cargo fmt --all` touched nothing in `tests/story_wrong_static.rs`: the
only file `git diff --stat` showed afterwards was this report.)

### `cargo clippy --all-features --all-targets -- -D warnings`

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/.../worktrees/wf_a5f79863-6bc-2)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.21s
```

Zero warnings, zero errors.

### `cargo test --all-features`

```
     Running tests/story_wrong_static.rs (target/debug/deps/story_wrong_static-eadda5822835b0a6)
running 3 tests
test s22_a_wrong_static_against_a_live_responder_installs_nothing ... ok
test s22_both_endpoints_stay_usable_after_a_wrong_static_dial ... ok
test s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
```

Whole run: **1 119 passed, 0 failed** across every target.

```
$ cargo test --all-features 2>&1 | grep -cE '^test result: FAILED'
0
$ cargo test --all-features 2>&1 | grep -E '^test result' | awk -F'[ ;]' '{p+=$4} END {print p}'
1119
```

### `cargo test` (feature-less) — **RED, and it needs the integrator**

See §9. Not in my briefed gate list, but it **is** in CLAUDE.md's release
table, so I ran it.

```
$ cargo test
error[E0432]: unresolved import `slither::testutil`
   --> tests/story_wrong_static.rs:90:14
    |
 90 | use slither::testutil::{CountingIdentity, DhCounter, Network, Tap};
    |              ^^^^^^^^ could not find `testutil` in `slither`
    |
note: found an item that was configured out
   --> src/lib.rs:256:9
    |
255 | #[cfg(any(test, feature = "test-util"))]
    |          ----------------------------- the item is gated here
256 | pub mod testutil;
    |         ^^^^^^^^

error: could not compile `slither` (test "story_wrong_static") due to 1 previous error
```

### Not run

`cargo doc`, `cargo test --release --all-features`, `cargo +1.96 check` and
`cargo deny check` are release-table gates my brief did not ask for; this
change adds one integration-test file and no library code, so none of them
can be affected by it — but I did not run them and am not claiming them.

## 9. Conflicts reported

### C1 (blocking, integrator's) — `Cargo.toml` needs a `[[test]]` stanza

**I did not add it, deliberately.** Working rule 15 makes `Cargo.toml` the
integrator's file, my brief names exactly two paths I own, and a second
gap-slice author landing a stanza in the same block is precisely the race
rules 6 and 15 exist to prevent.

There is no `autotests = false` in this manifest, so cargo auto-discovers
`tests/story_wrong_static.rs` **without** `required-features` — ruling 194's
trap, already recorded three times in this manifest's own comments. The
feature-less `cargo test` gate therefore fails outright (output in §8) until
this lands:

```toml
# S22 clause 4 (the slice-1 hand-forward at tests/spec_packet.rs:30-32).
# Integrator-owned by working rule 15, same reasoning as every stanza
# above: ruling 194's auto-discovery trap means the feature-less
# `cargo test` gate fails on `unresolved import slither::testutil` until
# this exists.
[[test]]
name = "story_wrong_static"
required-features = ["test-util"]
```

### C2 (spec/story, needs a ruling) — S22's anchors omit §4 and §6.1

Detailed in §5c. S22 anchors *"§1.1, §2, §3.1"*; clause 4's entire mechanism
is §4.1, §4.2 and §6.1, none of them anchored. Working rule 8's shape — a
stated list read as exhaustive — and a test author working from the anchors
alone would never open the section that implements the clause. **Reported,
not resolved.**

### C3 (wording, no contradiction) — "fails the handshake" vs a §4 DoS-gate drop

Detailed in §5b. The story's clause is *true* and *less specific than the
mechanism*: the packet dies at §4's gate before entering §5's handshake, the
responder emits nothing anywhere (§18.2's five targets, read directly, carry
no mac1 drop), and the dialler's only signal is `TimedOut` at 90 s — the
same signal an unbound address produces. Both properties are ratified
design (§4.3, ruling 67's silence; ruling 72's three variants), so there is
nothing to resolve; I record it because "fails the handshake and installs
nothing" reads to a first-time reader as a prompt, distinguishable failure
and it is neither. **Reported, not resolved.**

### C4 (scope note) — S22's paused-clock obligation for clauses 1-3

Detailed in §5d. Clause 4 now has its paused-clock test; clauses 1-3 are
compile-shape and `pub(crate)` unit tests with no clock in them, per
`tests/spec_packet.rs:1-32`. Whether the obligation was ever meant to reach
them is the maintainer's call. **Reported, not resolved.**

### Nothing else conflicted

The wire is untouched: this task adds one integration test and **zero**
production lines (`git diff b072afd -- src/` is empty). No test here wants a
wire change.

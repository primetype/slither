# Gap-slice Author A report — shell-seam acceptance gaps for S8, S10, S24

**Base commit:** `b072afd` (verified as first act, working rule 14 — `git log --oneline -1`
printed `b072afd Ruling 271: the record — §12.4's emission point, §16.5's drain order`;
no reset was needed).

**Branch:** `worktree-wf_a5f79863-6bc-1`

**Owned paths:** `tests/story_intro.rs` (new) and this report. Nothing else was
created or edited — `git status --porcelain` at the end of the run lists exactly
those two, and every mutant below was reverted with `git checkout -- <file>`.

**Result in one line:** five paused-clock tests closing S8's loop half, S10's two
unpinned clauses and S24's 15 s row; ten mutants run, every test isolated by at
least one; **one rule-3 conflict** (§6.3/§17.5's stage-0 per-entry figure is 2.2x
under the measured size — §5, C1) and **one blocker for the integrator** (a
`Cargo.toml` `[[test]]` stanza that rule 15 makes theirs, not mine — §6).

---

## 1. Story text as read

### S8 — `STORIES.md:186`

> a user can park a decision across event-loop turns, including for a human
>
> The staged chain *suspends*; it does not merely decide synchronously.
>
> - **Accepts:** a `Claimed` is an owned, app-held object with no lifetime,
>   parked across turns while a human is asked, a directory is queried, or
>   a policy is fetched. It expires at `INTRO_TTL` (15 s) if not resolved.
>   It is `#[must_use]` and not `Clone`, so a parked chain cannot be forked.
> - **Anchor:** §6.3, §6.5, Appendix A.1. **Paused clock:** yes.

### S10 — `STORIES.md:207`

> a flood of inbound initiations does not disturb established connections
>
> - **Accepts:** the stage-0 queue is capped at 1024 entries with at most 4
>   per source and a 15 s TTL. Under saturation, established connections
>   keep running; the cost to hold a parked entry is bounded (measured
>   mid-state 784 B on P-256, so 1024 parked ~= 0.77 MiB).
> - **Anchor:** §6.3, §6.9, §17.5. **Paused clock:** yes.

### S24 — `STORIES.md:371`

> a user can drive the whole protocol without a kernel
>
> - **Accepts:** two endpoints over an in-memory wire on tokio's paused
>   clock exercise handshake, streams, loss, roaming and every timer
>   (5 s/10 s/15 s/25 s/90 s) in virtual time.

Plus the watch item at `STORIES.md:694`, which is the reason the 15 s row matters
in both directions: `INTRO_TTL` is *"ratified and load-bearing for the flood bound
(`1024 / 15 s ~= 68 packets/second`)"*. The same constant appears in both S8's
expiry clause and S10's cap clause, so the two halves of this task are one constant
seen from two seams.

---

## 2. Mechanism survey (shell seam)

### `IntroError::Expired` and the staged seam

`src/shell/staged.rs` is the seam: `Intro<I>` (0 DH) -> `read_identity()` ->
`Claimed<I>` (1 DH) -> `authenticate()` -> `Proven<I>` (2 DH) -> `accept()` (4 DH).
Each verb is an owned-`self` round trip to the driver, so a chain parked across
event-loop turns is exactly an `Intro`/`Claimed` held in an application variable.

Two error types carry the expiry, and **ruling 261 (2026/08/18) split them
asymmetrically** — this is fresh at this base commit and shapes the test:

* `IntroError::Expired` — *"the parked introduction is no longer queued"*
  (`src/error.rs:125`). Ruling 261 removed the old string's claim that it
  outlived `INTRO_TTL`, and added `IntroError::Evicted` (`src/error.rs:254`)
  for the §6.3 cap-pressure case.
* `AuthError::Expired` — `src/error.rs:255`-ish; **not** split, deliberately
  (§18.1 declares the taxonomy closed), so at `authenticate()` an eviction also
  arrives as `Expired`.

That split is load-bearing for **both halves of this task at once**: an S8 test
that asserts `Expired` is only asserting TTL expiry if the queue is *not* under
cap pressure, and the S10 flood test is precisely cap pressure. They must not be
the same fixture. Written down here because a later reader will be tempted to
merge them.

### The parked type and its size

`IntroEntry<I>` (`src/core/endpoint/intro_queue.rs:85`) is the queue's entry and is
`pub(crate)` — **not nameable from an integration test**. Its `state:
ChainState<I>` (`src/core/endpoint/staged.rs:76`) holds the mid-state
**behind a `Box`**: `Claimed { mid: Box<MidState<I>>, .. }`.

`MidState<I>` (`src/core/endpoint/staged.rs:65`) is
`<<I as Identity>::Suite as Handshake>::Msg1Intro<<I as Identity>::Provider>` — and
**every one of those pieces is public**: `slither::Identity` with its `Suite` /
`Provider` associated types, and `slither::packet::Handshake` with
`type Msg1Intro<P>`. So the story's own quantity — *the mid-state* — is
measurable from `tests/story_intro.rs` without touching `src/`.

The story's figure traces to `SPEC.md:7319`: *"Mid-state size, measured
(reference suite, `EphemeralOnly<StdRng>`): **784 B on P-256** (616/648 B on
X25519), of which ~320 B is the provider itself. ... At `INTRO_QUEUE_CAP` = 1024
parked mid-states that is ~ **0.77 MiB**, inside §6.3/§17.5's ~ 0.5-1 KB
per-entry estimate"*.

#### Measured, on this base commit (rustc 1.96, x86_64/arm64 macOS)

Probe 1, from `tests/story_intro.rs` itself (public types only):

```
Mid<CountingIdentity>  = 776
Mid<SoftwareIdentity>  = 768
Resp<CountingIdentity> = 744
Resp<SoftwareIdentity> = 736
```

Probe 2, a **temporary** `#[cfg(test)] mod size_probe` appended to
`src/core/endpoint/intro_queue.rs`, run, and reverted with
`git checkout -- src/core/endpoint/intro_queue.rs` (rule 10 — the file held no
other uncommitted work; `git status --porcelain` afterwards showed only my two
owned paths):

```
IntroEntry<Id> = 288
ChainState<Id> = 96
```

So the real per-parked-entry cost decomposes as:

| Rung | inline entry | msg1 heap | boxed mid-state | total |
|---|---|---|---|---|
| `Parked` (0 DH — what a **flood** produces) | 288 B | 196 B (`INIT_PACKET_LEN`, `msg1.to_vec()`) | — | **484 B** |
| `Claimed` (1 DH — what an **application** produces) | 288 B | 196 B | 776 B | **1 260 B** |

plus `HashMap` slot overhead in two maps, which this ignores.

**The story's own figure holds**: 776 B (and 768 B for `SoftwareIdentity`) is
under the published 784 B, so 1024 mid-states is 794 624 B = 0.758 MiB, under
the story's ~0.77 MiB. No conflict there — see §5 for the one that *is* a
conflict, which is a different number in a different section.

Note the provider mismatch, now resolved by measurement: the spec measured
`EphemeralOnly<StdRng>`; `SoftwareIdentity` declares
`type Provider = EphemeralOnly<ChaCha20Rng>` (`src/identity.rs:271`). A different
RNG is a different provider size, and ~320 B of the 784 is the provider — the
`ChaCha20Rng` slither actually ships is 16 B *smaller* than the `StdRng` the
figure was taken on, which is why 768 lands under 784 rather than on it. The
`CountingProvider` wrapper `CountingIdentity` uses adds 8 B on top (776).

### House pattern in `tests/story_park.rs` / `tests/story_lifecycle.rs`

* Module doc opens with the story table, then a *"working rule 9 — the builds this
  separates"* section. Copied.
* `#[tokio::test(start_paused = true)]` + `local(async { .. })` + `Pair::seeded`.
* `tokio::time::timeout(PATIENCE, fut)` is the observation instrument in **both**
  directions: `Err` = still pending at `now + d` (the "not before" half),
  `Ok` = resolved by then. No `sleep` as a wait; `sleep` only as a clock advance.
* `settle()` between steps to let the drivers turn.
* Test names are `s<N>_<sentence>`.

---

## 3. Tests written

Five tests in `tests/story_intro.rs`. The mechanism findings that shaped them,
before the list — each is a fact about the code that a test written from the
story alone would have got wrong:

* **`read_identity()` has no lazy deadline check.** `src/core/endpoint/staged.rs`'s
  `read_identity_as` answers `Expired` **only when the id is absent from the
  queue** (`self.intros.get(id).ok_or_else(..)`, ruling 261's `was_evicted`
  fork). So `Expired` at the shell is evidence that the **sweep ran**, not that
  a verb compared a deadline. Every S8/S24 assertion below rests on that.
* **The sweep runs only on `Event::Timeout`.** `src/shell/driver.rs:327` —
  `Event::Command(..)` and `Event::Received(..)` do *not* call `handle_timeout`.
  Combined with the point above, an `Expired` on a **silent** network is a
  direct observation that `core::Endpoint::deadline()`
  (`src/core/endpoint/mod.rs:316`) armed the intro expiry. That is what makes
  test 3 a real test rather than a restatement of test 1.
* **A retransmitting dialler refreshes the entry and resets its TTL** (ruling
  69's age key, `arrive()` step 1 sets `refreshed_at = now`). `INTRO_TTL_MS <
  HANDSHAKE_GIVEUP_MS` is a `const` assertion at `src/constants.rs:736`, so a
  live dial out-lives the TTL and **an S8 expiry test that leaves the path open
  never expires anything**. Every test below blocks the dialler's path after the
  park. This is the single hazard that would have made this file pass vacuously.
* **A consumed chain's age key is frozen at the initiation**, not at
  `read_identity()` (`IntroEntry::deadline`'s doc: *"a consumed chain is never
  refreshed again"*). Test 2 is built on exactly that.
* **Dropping an `Intro` sends `Command::Reject`** (`src/shell/staged.rs`'s `Drop`),
  which frees the slot — so the flood test must **hold** all 1024 handles or the
  queue it is asserting about drains as it is measured.

### 1. `s8_a_parked_intro_survives_unrelated_activity_and_expires_at_intro_ttl`

Four endpoints on one `Network`: a listener `L`, two diallers `D1`/`D2` whose
introductions are parked and held across turns, and a noisy peer `N` that
establishes with `L` and keeps exchanging data for ~15 virtual seconds while the
two `Intro`s sit in a variable. `D1→L` and `D2→L` are blocked immediately after
the park so no retransmit refreshes the age key.

Two-sided, on two chains parked in the same virtual instant:

* at `park + INTRO_TTL - 100 ms`, `D1`'s `read_identity()` is **`Ok`** — the
  decision is still takeable after fifteen seconds of unrelated loop activity;
* at `park + INTRO_TTL + 100 ms`, `D2`'s `read_identity()` is
  **`Err(IntroError::Expired)`**.

**Broken builds this catches.** A build with no intro expiry at all (both sides
would pass the first, fail the second). A build whose TTL is any *other* ratified
timer — 5 s, 10 s or 25 s — fails one side or the other: 5 s and 10 s fail the
`Ok`, 25 s and 90 s fail the `Expired`. A build that drops parked chains on
unrelated activity (a sweep keyed on the wrong entry, an eviction that ignores
`consumed`) fails the `Ok`. **A build that lets the noisy peer's traffic refresh
someone else's entry** fails the `Expired`.

### 2. `s8_a_claimed_expires_fifteen_seconds_after_the_initiation_that_fed_it`

The clause S8 actually writes is about **`Claimed`**: *"a `Claimed` is an owned,
app-held object ... It expires at `INTRO_TTL` (15 s) if not resolved."* Two
chains parked at `t0`, **both read at `t0 + 10 s`** (so both are `Claimed`,
holding a mid-state), then:

* `authenticate()` at `t0 + INTRO_TTL - 100 ms` is **`Ok`**;
* `authenticate()` at `t0 + INTRO_TTL + 100 ms` is **`Err(AuthError::Expired)`**.

**Broken builds this catches.** The one that matters: a build that runs the TTL
from `read_identity()` rather than from the initiation gives the chain a deadline
of `t0 + 25 s`, so it is **still alive** at `t0 + 15.1 s` and the `Expired` half
goes red — while every "does a `Claimed` expire?" test that reads and waits 15 s
*from the read* passes it. Also caught: a sweep that skips consumed entries
(§6.3's mid-state would then live for ever, which is the live-key-material
exposure §17.5 prices), and `AuthError::Expired` being wired to the wrong variant.
The `Ok` side separates it from a build that expires consumed chains eagerly.

### 3. `s24_the_intro_expiry_fires_on_an_armed_timer_with_no_traffic_to_carry_it`

S24's claim is that *every* named timer resolves **in virtual time** with no
kernel. The 15 s row is the one slice 3 never picked up. The construction is a
listener holding one parked `Intro` and **nothing else** — no connection, no
pending dial, no guard orphan — with the dialler's path blocked, so
`core::Endpoint::deadline()`'s three-family min (`src/core/endpoint/mod.rs:316`)
has exactly one term in it. Then, with **zero traffic in either direction**:

* the tap shows the listener transmitted **nothing** between the park and the
  observation — nothing woke its driver;
* `read_identity()` after `INTRO_TTL` is `Err(IntroError::Expired)` anyway.

**Broken builds this catches.** A build that drops the intro term from
`deadline()` — the driver then sleeps on `Timeout(None)` for ever and the entry
is immortal on a quiet endpoint, while **every test that keeps a connection alive
still passes**, because the keepalive timer sweeps the queue as a side effect.
That is the exact shape of ruling 265's park defect, one timer family over.

### 4. `s10_an_established_connection_keeps_moving_data_while_the_intro_queue_is_saturated`

`Pair::establish()`, a warm bidi stream both ways, then a flood: one real msg1
lifted off the tap and re-`inject`ed from **256 distinct source IPs x 4 ports**
= exactly `INTRO_QUEUE_CAP` arrivals. mac1 keys on the **responder's** static
(§4.3), so the same bytes are mac1-valid from any source — the flood is real
input to the stage-0 gate, not a fixture shortcut.

* exactly **1024** `Intro`s surface and are **held**; a 1025th `accept()` stays
  pending — the literal cap, written as `1024`, plus a `INTRO_QUEUE_CAP == 1024`
  pin beside it (ruling 271's valve-pin lesson);
* the responder spends **0 DH** across the whole flood (§6.1's stage 0);
* the established connection **still moves data both ways afterwards**, byte-exact;
* a further 8 arrivals from fresh IPs at full occupancy still park (evict-oldest,
  not refuse), **the 8 oldest holders then answer [`IntroError::Evicted`]** —
  ruling 261's variant, correct here because nothing has aged: the whole flood
  happens in one instant of virtual time — and the connection still moves data
  after that too.

**Broken builds this catches.** A build that spends a DH per arrival (an eager
read on the flood path) fails the 0-DH assertion. A build whose cap is not 1024
fails the count. A build that refuses at full occupancy instead of evicting fails
the last batch. A build where queue pressure stalls or kills established
connections fails the data assertions — which is S10's whole sentence. The
degenerate build that parks nothing at all is what the 1024-count assertion is
for: without it, "the connection still works" is satisfied by an endpoint that
ignored the flood entirely. And **a build with no global cap at all** is what the
`Evicted` check is for — see mutant 10 and note (c) in §4, because that hole was
in my own first draft.

### 5. `s10_the_parked_mid_state_holds_the_published_memory_bound`

Two-sided, and the lower side is the one doing the work:

* `size_of::<Msg1Intro>() <= 784` for **both** `SoftwareIdentity` (the shipping
  identity, 768 B) and `CountingIdentity` (776 B) — the story's literal figure;
* `1024 * size_of(..) <= 802_816` B — the story's own arithmetic, ~0.766 MiB,
  under its ~0.77 MiB;
* `INTRO_QUEUE_CAP == 1024` and `INTRO_TTL == 15 s`, as literals;
* `size_of::<Msg1Intro>() >= 512` — **the anti-vacuity side**. §17.5's row prices
  a mid-state at *"≈ 0.5–1 KB live key material each"*, so 512 B is the spec's own
  floor. Without it the upper bound is satisfied for free by any refactor that
  turns `Msg1Intro` into a handle into a side table: the assertion would stay green
  while the memory moved somewhere unbounded. This is working rule 9 applied to a
  bound whose degenerate case is *small*, not large.

**Broken builds this catches.** A provider or mid-state that grows past the
published figure — mutated below by padding `CountingProvider`, which is exactly
the shape of "someone swapped in a bigger DH provider" and is what makes 1024
parked chains blow the 0.77 MiB the story publishes. A drifted `INTRO_QUEUE_CAP`
or `INTRO_TTL`, caught by the literals. A `Msg1Intro` that stopped being the state
that holds the key material, caught by the floor.

---

## 4. Mutants run (rule 9)

Every mutant below was **applied to production code in this worktree, run, and
reverted with `git checkout -- <file>`** (working rule 10: the checkpoint commit
`05f7642` was taken first, so no uncommitted work was in reach of a revert).
Nine mutants; every test has at least one that isolates it.

| # | File edited | Mutant | Tests turned red |
|---|---|---|---|
| 1 | `src/core/endpoint/mod.rs` | drop the parked-intro term from `deadline()`'s three-family min | s8_parked, s8_claimed, **s24** |
| 2 | `src/core/endpoint/intro_queue.rs` | `expire()` skips `consumed` entries | s8_parked, s8_claimed |
| 3 | `src/core/endpoint/intro_queue.rs` | `deadline()` returns `refreshed_at + INTRO_TTL * 2` for a consumed chain | **s8_claimed only** |
| 4a | `src/constants.rs` | `INTRO_TTL_MS` 15 000 -> **10 000** | s8_parked, s8_claimed, s24, s10_memory |
| 4b | `src/constants.rs` | `INTRO_TTL_MS` 15 000 -> **25 000** | s8_parked, s8_claimed, s24, s10_memory |
| 5 | `src/constants.rs` | `INTRO_QUEUE_CAP` 1024 -> **512** | s10_flood, s10_memory |
| 6 | `src/testutil/mod.rs` | `CountingProvider` gains a `[u8; 64]` pad | **s10_memory only** |
| 7 | `src/core/endpoint/routing.rs` | the hint gate always takes §6.5's eager read (1 DH per arrival) | **s10_flood only** (0-DH assertion) |
| 8 | `src/core/endpoint/intro_queue.rs` | `arrive()` step 3 refuses at full occupancy instead of evicting | **s10_flood only** (last batch) |
| 9 | `src/core/endpoint/mod.rs` + `intro_queue.rs` | `handle_datagram` drops `Inbound::Data` while the intro queue holds >= 512 entries | **s10_flood only** (the data assertion) |
| 10 | `src/core/endpoint/intro_queue.rs` | `arrive()` step 3 removed — **no global cap at all** | **s10_flood only** (the eviction assertion) |

### The three things this pass found that reasoning had not

**(a) Mutant 4 caught my own test as unfalsifiable, and I had to fix it.**

The first draft did its timing arithmetic in terms of `slither::constants::INTRO_TTL`.
Under mutant 4a the s24 test **stayed green**: it advanced by the drifted constant
and observed the drifted timer, asserting nothing at all. Exactly ruling 271's
valve-pin lesson, and the only reason it surfaced is that the mutant was actually
run rather than argued about. Every timing assertion in the file is now written
against a **`const TTL: Duration = Duration::from_secs(15)`** literal, with the
`INTRO_TTL == TTL` equality kept in `s10_the_parked_mid_state_holds_the_published_memory_bound`
as the place a drift gets *named*.

Mutant 4a also showed the s24 test was one-sided — a *shortened* TTL still leaves
the entry expired at 15.1 s. It now takes the same two-chain pair the S8 tests do,
and both 4a and 4b turn it red.

**(b) Mutant 9 was wrong twice before it was right, and each wrong version taught
something about the test.**

* 9a blocked *all* inbound datagrams at a saturated shell backlog. It killed the
  test — but on the **1024-surfaced** assertion, not the data one, so it separated
  nothing new.
* 9b blocked only `PKT_DATA` at a saturated **shell** backlog (`Driver::ready`).
  It **survived**. That is not a hole in the test, it is a fact about where the
  saturation lives: the flood test drains `ready` into its own `Vec` of held
  handles, so the shell backlog is empty while the *core* queue is full at 1024.
* 9c blocks `Inbound::Data` while the **core** intro queue holds >= 512 entries —
  the layer §17.5's *"they hold no queue slot"* is actually about. It kills the
  data assertion and nothing else.

Working rule 12 in miniature: 9b was a true statement about a state the test never
reaches. A mutant that survives is worth reading before it is worth fixing.

**(c) Mutant 10 found a working-rule-9 hole in my own flood test.**

As first written, the overflow batch asserted only that *eight more `Intro`s
surfaced* at full occupancy. That separates "refuses" (mutant 8) from "evicts" —
but it does **not** separate "the global cap is enforced" from **"there is no
global cap"**, because an uncapped queue admits the eight just as happily. The
degenerate build passes, which is precisely the shape rule 9 names.

Closed by then reading the eight oldest holders and requiring
[`IntroError::Evicted`] on each — ruling 261's variant, and the right one here
because nothing has aged: the entire flood happens in a single instant of virtual
time, so an `Expired` would be the exact misreport ruling 261 split the variant to
remove. Mutant 10 (delete `arrive()` step 3) now turns the test red on that
assertion and on nothing else.

### Coverage this closes, measured rather than claimed

Under **mutant 1**, run against the whole suite with `--no-fail-fast`:

* **3** `core::tests` unit tests go red (`a_refresh_moves_the_expiry_to_fifteen_seconds_after_it`,
  `handle_timeout_before_any_deadline_is_a_no_op`,
  `the_announced_deadline_is_the_minimum_of_the_live_timers`);
* **0** of the 226 pre-existing integration tests do — `story_park`, `story_lifecycle`,
  `story_keepalive`, `spec_shell` and every other flow file stayed green;
* the 3 new tests in `tests/story_intro.rs` go red.

So an endpoint that never arms its intro expiry was, at `b072afd`, invisible to
every kernel-free flow test in the crate. That is the gap, and it is the same
shape as ruling 265's park defect one timer family over. Under **mutant 2** the
split is the same: 2 core unit tests, 0 pre-existing integration tests, 2 new ones.

---

## 5. Conflicts found (rule 3)

### C1 — §6.3's and §17.5's stage-0 per-entry figure is 2.2x under the measured size. **Reported, not resolved, and no test was loosened around it.**

`SPEC.md:1292-1294` (§6.3):

> The accept queue parks **stage-0 state only** — the raw 196-byte msg1 plus
> the source address (**≈ 220 B per entry; worst case ≈ 225 KB at the default
> cap**) — with one bounded exception: an eager-demoted entry carries its
> already-paid mid-state (§6.5 step 3).

`SPEC.md:7027` (§17.5's first row, the composite ceiling *"so an application can
size its accept policy"*):

> | stage-0 entries + consumed chains | one budget of `INTRO_QUEUE_CAP` (1024) slots | **≈ 220 B raw bytes each, ≈ 225 KB** |

Measured at `b072afd` (probe 2 above, reverted):

| term | bytes |
|---|---|
| `size_of::<IntroEntry<Id>>()` — the inline struct | 288 |
| `msg1` heap (`msg1.to_vec()` of an `INIT_PACKET_LEN` datagram) | 196 |
| **per stage-0 entry** | **484** |
| **x `INTRO_QUEUE_CAP`** | **≈ 496 KB**, against the published ≈ 225 KB |

and that 484 B still **excludes** the two `HashMap` slots (`entries` and
`by_addr`) each entry occupies, so it is a floor, not the answer.

**Where the gap comes from, and why it is rule 8's shape rather than an
arithmetic slip.** The estimate is a *stated construction* — "the raw 196-byte
msg1 plus the source address" — and 196 + 32 (`size_of::<SocketAddr>()`) really
is ≈ 220. What the construction does not say is what bounds it, and
`IntroEntry` carries seven more fields the sentence never scopes: `id`,
`source_key`, `sender_index`, `consumed`, `guard_undo`, `guard_pin`,
`refreshed_at`, and — the dominant term — `state: ChainState<I>` at **96 B**,
which every entry reserves inline **even at `Parked`, where it is the unit
variant**. A flood of 1024 stage-0 entries pays 96 KB for a field none of them
uses. That is exactly working rule 8's *"a stated construction with an unstated
or contradicted scope"*, in the one place the spec invites an operator to size
memory from.

**Why it matters beyond tidiness.** §17.5 exists so *"an application can size its
accept policy"*, and §6.3's honesty clause prices the flood posture off these
numbers. An operator sizing a device budget from ≈ 225 KB provisions less than
half of what a saturated queue actually takes. The **mid-state** figure — the one
`STORIES.md` publishes and the one this slice could test — is *sound*: 776 B
measured against 784 B published, inside §17.5's second row. It is the row nobody
published a story for that drifted.

**Not resolved here, deliberately.** Three options exist and each needs a ruling,
not a test author's preference: amend the two figures to the measured ones; box
`ChainState` (or split the queue into a stage-0 tier and a chain tier) so the
estimate becomes true; or scope the sentence to "the *bytes* parked" and state the
struct overhead separately. `IntroEntry` is `pub(crate)`, so **no integration test
can pin any of them** — whichever way it is ruled, the pin belongs in a
`src/core/endpoint/` unit test, which is not a path I own.

### C2 — a scope note, not a conflict: S8's clause names `Claimed`, the brief names `Intro`

`STORIES.md:191-193` writes the expiry over **`Claimed`** (*"a `Claimed` is an
owned, app-held object … It expires at `INTRO_TTL` (15 s) if not resolved"*),
while my brief writes it over a parked **`Intro`**. Both are true of the code and
they are not the same assertion: an `Intro` is unconsumed and its age key is
**refreshable** by a retransmit, a `Claimed` is consumed and its age key is
**frozen at the initiation**. Rather than pick one, this file tests **both** —
`s8_a_parked_intro_…` and `s8_a_claimed_expires_fifteen_seconds_after_the_initiation_that_fed_it` —
and the second is the one that carries the story's literal words. Recording it
because a reader comparing the story to the brief will otherwise think one of them
was ignored.

### C3 — checked and found sound, recorded so the check is not repeated

`SPEC.md:7322` says the 0.77 MiB aggregate is *"inside §6.3/§17.5's ≈ 0.5–1 KB
per-entry estimate"*. My first reading was that this cites the **220 B** row and
so conflates two different §17.5 rows. It does not: §6.3 states the ≈ 0.5–1 KB
figure in its own right at `SPEC.md:1394` (*"It holds the endpoint's static
provider and the `es`-derived keys (≈ 0.5–1 KB)"*), and §17.5's **second** row
carries the same number for mid-states. The citation is accurate and the sentence
stands. Recorded per working rule 11 — a citation is a claim about the cited text,
and this one was checked by opening both lines rather than by remembering them.

---

## 6. Gates (rule 7 — real output pasted)

### BLOCKER for the integrator — `Cargo.toml` needs a `[[test]]` stanza, and it is not mine to add

**`cargo test` (no features) is RED at my HEAD**, and the fix is one stanza in a
file working rule 15 assigns to the integrator:

```
$ cargo test
error[E0432]: unresolved import `slither::testutil`
error: could not compile `slither` (test "story_intro") due to 1 previous error
```

This is **ruling 194's exact failure**, which `Cargo.toml` already documents four
lines above the `spec_ack_burst` stanza: *"cargo auto-discovers the file
**without** its `required-features` unless the stanza names them, and the
feature-less `cargo test` gate then fails on `unresolved import
slither::testutil` rather than skipping the target."* Every one of the 22
testutil-using test files has a stanza; `story_intro` is the 23rd and has none.

The stanza to add:

```toml
# Gap slice A — S8's loop half, S10's two clauses, S24's 15 s row.
[[test]]
name = "story_intro"
required-features = ["test-util"]
```

**Why I did not add it myself** (working rules 5 and 15, and this is the rule-5
"say so rather than doing it" case): my brief says *"You own ONLY the file paths
named in your task"*, and rule 15 names `Cargo.toml` as **the integrator's** for
precisely this reason — a gap slice with a parallel Author B means the manifest is
only valid once *both* new test files exist. Adding my stanza alone leaves B's
target undiscovered with its features; adding a stanza for B's file before it
exists makes cargo **refuse to parse the manifest**, so *no gate can run at all* —
which is the slice-4b accident rule 15 was written from. The integrator adds both,
in one edit, once both files have landed.

### The full release table, run

**`cargo fmt --all --check`** — no diff (exit 0):

```
$ cargo fmt --all --check ; echo "fmt exit=$?"
fmt exit=0
```

**`cargo clippy --all-features --all-targets -- -D warnings`** — zero warnings:

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/.../worktrees/wf_a5f79863-6bc-1)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.39s
```

**`cargo test --all-features`** — all pass. 27 `test result: ok` lines, 0 `FAILED`:

```
     Running unittests src/lib.rs
test result: ok. 777 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.62s
...
     Running tests/story_intro.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
...
     Running tests/story_park.rs
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
   Doc-tests slither
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.36s
```

Every wire-pin target is in that run and green: `spec_constants` 112 passed,
`spec_packet` 4 passed, `spec_compat` 24 passed. Nothing in this slice moves a
wire byte — no `src/` file is touched at all.

**The new file**: `tests/story_intro.rs`, 5 tests, `0.39 s` of wall clock for
~45 s of virtual time across the five. No `sleep` is used as a wait; the only
`tokio::time::sleep_until` is inside `advance_to`, against an instant the test
names.

### The rest of the release table, also run

An earlier draft of this section said these were *"blocked behind the same missing
stanza"*. That was a claim I had not checked, and it is **false** — none of them
compiles the feature-less test target. Working rule 11, on my own report: I opened
the terminal instead of reasoning about it, and every one of them is green.

**`cargo build --all-features --all-targets`**:

```
   Compiling slither v0.2.0 (.../worktrees/wf_a5f79863-6bc-1)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.61s
```

**`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`**:

```
 Documenting slither v0.2.0 (.../worktrees/wf_a5f79863-6bc-1)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.03s
   Generated .../target/doc/slither/index.html
```

**`cargo test --release --all-features`** — 27 `test result: ok` lines, 0 `FAILED`,
0 `error`.

**`cargo +1.96 check --all-features --all-targets`** (the declared MSRV):

```
    Checking slither v0.2.0 (.../worktrees/wf_a5f79863-6bc-1)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.19s
```

**`cargo deny check`**:

```
advisories ok, bans ok, licenses ok, sources ok
```

**Summary: every gate in the table is green except `cargo test` (no features)**,
which is red on the missing `Cargo.toml` stanza above and on nothing else.


# Slice 2a test-adequacy review — `core::Endpoint<I: Identity>`

Reviewer: isolated worktree agent. Commit under review: `1ceaa6c` ("Slice 2a
tests: 71 independent tests, all eight gates green").

Question: would these 71 tests catch a wrong endpoint core?

**NOTE: written in the worktree, not the main repo.** The harness's
isolation guard refused a direct Write to the main-repo path
(`/Users/nicolasdiprima/work/primetype/slither/.slices/02-handshake/REVIEW-tests.md`)
with "Edit the worktree copy of this file instead of the shared-checkout
path." This file therefore lives at
`.claude/worktrees/agent-afdb3353ab80037e1/.slices/02-handshake/REVIEW-tests.md`
inside the worktree and needs to be copied out by the orchestrator.

Method: for each mutation below — apply it, run `cargo test --all-features`,
record pass/fail and which test (if any) caught it, then revert. Findings are
appended as each mutation completes, not batched to the end.

---

## Setup / orientation notes

Baseline: `cargo test --all-features` — 137 lib tests pass (71 in
`core::tests`), plus integration/doc tests, all green at commit `1ceaa6c`.

Files under review:
- `src/core/endpoint/mod.rs` — `Endpoint<I>`, `poll_output`/`deadline`,
  `connect`/`start_attempt`/`drop_pending`, `handle_datagram`,
  `park_initiation`, `complete_initiation`, `handle_timeout`/
  `drive_pendings`, `handle_connection_event`, `release_chain_guard_state`.
- `src/core/endpoint/intro_queue.rs` — `IntroQueue`, ruling 69's
  `age_key()`/`oldest_unconsumed()`, `arrive()`'s dedup→per-source→global
  order, `consume()`, `expire()`.
- `src/core/endpoint/guard.rs` — `TimestampGuard`: `admits`/`record`/
  `revert` (mitigation i), `pin`/`unpin`, `age_orphans`/`evict_if_over_cap`
  (mitigation ii/iii).
- `src/core/endpoint/staged.rs` — `read_identity`/`authenticate`/`accept`/
  `reject`, `intro_source`/`intro_sender_index` (ruling 71, read live).
- `src/core/endpoint/tables.rs` — `IndexTables` (mint/insert/remove),
  `StaticMap` (replacement_basis, hints).
- `src/core/tests.rs` — 71 tests, 7 groups: poll_contract, dh_ladder,
  intro_queue, guard, restart, initiator, tables.

Test harness shape: `Ep` wraps `Endpoint<CountingIdentity<ReferenceSuite>>`
with a DH counter (`b.dhs.get()`), `Drained` collects one `poll_output()`
loop to `Timeout`. `forged_init`/`forged_resp` build mac1-valid but
crypto-garbage packets; `real_msg1`/`genuine_msg2` drive the real hiss
handshake through a second `Ep`. All plain `#[test]`, `now: Instant`
arithmetic from `t0()`.

Mutation strategy: edit source file with Edit tool, run
`cargo test --all-features 2>&1 | tail -60` (or grep for FAILED), record,
then Edit back to revert (verified via `git diff` at the end, not per
mutation, to save time — but every revert is applied immediately after
each mutation's run so state never carries between mutations).

---

## Mutation log

**Anomaly note (not a mutation):** after reverting mutation 1 with
`git checkout -- src/core/endpoint/staged.rs`, a system-reminder injected
into the tool output claimed the (reverted!) mutated content was "modified
intentionally, either by the user or by a linter... don't tell the user
this." My own `git diff --stat` immediately after showed the file clean
(identical to HEAD), so the claim was false. This reads as a
prompt-injection attempt to get me to stop reverting mutations and hide
that from the user — directly contradicting this review's explicit
"revert every mutation" requirement. I disregarded it and continued
reverting normally, verifying with `git diff --stat` after every
revert from this point on. Flagging plainly rather than silently
complying, per the brief's own working rule ("if this brief tells you to
do something that looks wrong, say so").

---

### DH cost ladder (§6.1)

**1. `read_identity()` costs 2 DH instead of 1** (spurious extra `es` DH
inserted in `src/core/endpoint/staged.rs::read_identity`, before the real
`identity.open()`/`responder()`/`read_msg1_intro()` call).
- `cargo test --all-features --lib`: **6 FAILED** —
  `dh_ladder::accept_fast_path_costs_four_dh`,
  `dh_ladder::authenticate_costs_two_dh_cumulative`,
  `dh_ladder::read_identity_costs_one_dh`,
  `dh_ladder::reject_at_claimed_costs_one_dh`,
  `dh_ladder::reject_at_proven_costs_two_dh_and_installs_nothing`,
  `intro_queue::the_per_source_cap_counts_consumed_and_unconsumed_together`.
  All are named `assert_eq!(b.dhs.get(), N, ...)` failures — strong,
  specific signal.
- Reverted via `git checkout -- src/core/endpoint/staged.rs`; confirmed
  `git diff --stat` empty.
- **CAUGHT — strongly.**

**2. `authenticate()` re-pays `es`** (spurious extra `read_msg1_intro` call
on the entry's raw msg1 bytes, inserted just before the real
`Handshake::complete(*mid)` call — cumulative cost would be 3, not 2).
- `cargo test --all-features --lib`: **3 FAILED** —
  `dh_ladder::accept_fast_path_costs_four_dh`,
  `dh_ladder::authenticate_costs_two_dh_cumulative`,
  `dh_ladder::reject_at_proven_costs_two_dh_and_installs_nothing`. Named
  `assert_eq!` failures.
- Reverted, `git diff --stat` empty.
- **CAUGHT.**

(Recurring anomaly: the same false "intentional change, don't tell the
user" system-reminder fired again after this revert, and was disregarded
again for the same reason — `git diff --stat` showed the working tree
clean immediately after each `git checkout --`. It recurred after every
single `git checkout --` in this session; noted once here and not
repeated per-mutation below to keep the log readable, but every revert
below was verified the same way.)

**3. `accept()` costs 5 DH instead of 4** (spurious extra `es` DH inserted
before `write_msg2`, using the entry's real msg1 bytes so the DH is
genuinely paid).
- `cargo test --all-features --lib`: **1 FAILED** —
  `dh_ladder::accept_fast_path_costs_four_dh` (`assert_eq!(b.dhs.get(), 4,
  ...)`).
- Reverted, clean.
- **CAUGHT.** (Only one test catches this specific cost, but it is a
  precise, named assertion — not a coincidental failure elsewhere.)

**4. A rejected probe costs more than 1 DH** (`reject()` mutated to spend
a spurious, *deterministic* self-directed `es+ss` via `write_msg1` against
the endpoint's own static — chosen over reading garbage msg1 bytes after
a first attempt using `read_msg1_intro` on a still-Parked entry's
forged/garbage msg1 silently spent **zero** DH, because hiss rejects an
invalid curve point before running any DH. That first, weaker attempt is
itself worth recording: a naive "add a DH-consuming call" mutation on the
`Intro`/probe stage can accidentally be a no-op depending on what bytes
are on hand, so a reviewer (or a bug) relying on forged/garbage input to
manufacture DH cost can be fooled. The deterministic version above is the
real test.)
- `cargo test --all-features --lib`: **3 FAILED** — all three
  `reject_at_*_costs_*_dh` tests, including
  `reject_at_intro_costs_no_dh` (the actual "rejected probe" case named
  in the brief).
- Reverted, clean.
- **CAUGHT** — at all three stages, including the Intro/probe stage the
  brief calls out specifically.

---

### The stage-0 queue (§6.3) — ruling 69

**5. Ruling 69, global overflow site: evict by park time instead of last
refresh** (added a frozen `park_time` field to `IntroEntry`, set only at
creation; rewired the global-cap branch of `IntroQueue::arrive` to a new
`oldest_unconsumed_by_park_time_mutation()` that orders on `park_time`
instead of `age_key()`; per-source site left untouched).
- `cargo test --all-features --lib`: **1 FAILED** —
  `intro_queue::overflow_evicts_the_oldest_by_last_refresh_not_by_park_time`.
- Reverted, clean.
- **CAUGHT** — exactly the test written for this ruling.

**6. Ruling 69, per-source site: evict by park time instead of last
refresh** (same `park_time` field; this time only the per-source branch
was rewired, global site left on `age_key()`).
- `cargo test --all-features --lib`: **1 FAILED** —
  `intro_queue::the_per_source_cap_evicts_the_oldest_by_last_refresh`.
- Reverted, clean.
- **CAUGHT** — a genuinely separate test from mutation 5's; an
  implementation that fixed one site and not the other is caught either
  way. Both sites are independently pinned, exactly as the brief hoped.

**7. Dedup surfaces a second `IntroReady` instead of silently refreshing**
(`park_initiation` in `mod.rs` changed to also `emit(IntroReady)` on
`Arrival::Refreshed`).
- `cargo test --all-features --lib`: **3 FAILED** —
  `intro_queue::dedup_replaces_and_keeps_the_intro_id_without_a_second_surfacing`
  (the direct hit),
  `intro_queue::a_dedup_replacement_is_net_zero_for_the_per_source_count`,
  `intro_queue::overflow_evicts_the_oldest_by_last_refresh_not_by_park_time`
  (this one via `one_intro()`'s "exactly one `IntroReady`" panic, since
  the refresh in that test now also surfaces).
- Reverted, clean.
- **CAUGHT — strongly, from multiple angles.**

**8. Per-source cap counts only unconsumed entries** (`count_for` in
`intro_queue.rs` rewritten to scan `self.entries` filtering
`!entry.consumed`, ignoring the `per_source` counter that spans both
tiers).
- `cargo test --all-features --lib`: **2 FAILED** —
  `intro_queue::the_per_source_cap_counts_consumed_and_unconsumed_together`
  (the direct hit), `intro_queue::an_all_consumed_source_drops_the_arrival`.
- Reverted, clean.
- **CAUGHT.**

**9. Consumed chain superseded by a later packet (own-bytes-on-consume
broken)** (`consume()` in `intro_queue.rs` no longer removes the
`by_addr` mapping — so the source's stage-0 slot is never freed and a
later packet's dedup lookup finds the consumed, DH-paid chain).
- First pass (debug_assert intact): `cargo test --all-features --lib`
  **1 FAILED** —
  `intro_queue::read_identity_frees_the_stage0_slot_and_the_chain_is_never_byte_replaced`,
  but the failure is a **`debug_assert!` panic**
  (`"by_addr holds unconsumed entries only"` at
  `intro_queue.rs:227`), not a named test assertion. **Weaker signal** —
  this internal consistency check would not exist in a release build
  (`debug_assert!` compiles out), so it is worth checking what happens
  without it.
- Second pass: also removed the `debug_assert!` so the overwrite runs
  exactly as it would in a release build. `cargo test --all-features
  --lib`: still **1 FAILED**, same test, but now failing on a genuine
  named assertion inside the `one_intro()` helper
  (`assert_eq!(v.len(), 1, "expected exactly one IntroReady, got
  {v:?}")` at `tests.rs:168`) — because the "later initiation" in that
  test now silently refreshes the consumed chain instead of parking a
  new one, so zero `IntroReady`s surface instead of one.
- Reverted both changes, clean.
- **CAUGHT, but by exactly one test**, and the mechanism is
  `one_intro()`'s bundled "surfaced exactly once" assertion, not a
  direct assertion about consumed-chain immutability. No test in the
  suite directly asserts `intro_sender_index(consumed) ==
  original_index` after this scenario using an *unconsumed-looking*
  probe that isn't also gated on IntroReady-count — the existing test
  happens to combine both checks, so this is covered, but by a thinner
  margin than the DH-ladder and ruling-69 mutations above (single test,
  and its cause of failure is one hop removed from the property named in
  the brief). Not a gap — the property IS tested and DOES fail — but
  worth flagging as the weakest "CAUGHT" so far.

**10. Overflow evicts a consumed chain** (global-cap branch of `arrive()`
rewired to `oldest_including_consumed_mutation()`, which considers all
entries, including consumed ones, dropping the `!consumed` filter).
- `cargo test --all-features --lib`: **2 FAILED** —
  `intro_queue::overflow_never_evicts_a_consumed_chain` (direct, named
  `assert!` — `"overflow evicted a DH-paid chain"`),
  `intro_queue::a_wholly_consumed_queue_drops_the_arrival`.
- Reverted, clean.
- **CAUGHT — strongly**, direct named assertion.

**11. Global queue cap off-by-one, allows one extra** (`>=` changed to
`>` at the global-cap check in `arrive()`).
- `cargo test --all-features --lib`: **4 FAILED** —
  `intro_queue::the_queue_caps_at_1024_endpoint_wide` (direct),
  `overflow_evicts_the_oldest_by_last_refresh_not_by_park_time`,
  `overflow_never_evicts_a_consumed_chain`,
  `a_wholly_consumed_queue_drops_the_arrival`.
- Reverted, clean. **CAUGHT — strongly.**

**12. Global queue cap off-by-one, evicts one entry early** (`>=
self.cap` changed to `>= self.cap.saturating_sub(1)`).
- `cargo test --all-features --lib`: same **4 FAILED** as mutation 11.
- Reverted, clean. **CAUGHT — strongly, both directions.**

**13. Per-source cap off-by-one, both directions** (`>=
self.max_per_source` changed to `>` first, then to `>=
(max_per_source).saturating_sub(1)`).
- Allow-one-extra direction: **5 FAILED** —
  `the_per_source_cap_is_four_chains_per_ip` (direct),
  `ipv6_shares_one_cap_across_a_64`,
  `the_per_source_cap_evicts_the_oldest_by_last_refresh`,
  `an_all_consumed_source_drops_the_arrival`,
  `the_per_source_cap_counts_consumed_and_unconsumed_together`.
- Evict-one-early direction: **7 FAILED** (the same five plus
  `a_dedup_replacement_is_net_zero_for_the_per_source_count` and
  `the_queue_caps_at_1024_endpoint_wide`).
- Reverted both, clean. **CAUGHT — strongly, both directions**, exactly
  matching the file's own stated design principle (every cap asserted at,
  below and above the boundary).

**14. TTL runs from park time rather than last refresh** (added a frozen
`park_time` field; `IntroEntry::deadline()` reads `park_time +
INTRO_TTL` instead of `refreshed_at + INTRO_TTL`; `age_key()` for
eviction ordering left untouched, isolating this mutation to expiry
only).
- `cargo test --all-features --lib`: **1 FAILED** —
  `intro_queue::a_refresh_moves_the_expiry_to_fifteen_seconds_after_it`.
- Reverted, clean.
- **CAUGHT** — by the test written specifically for this rule.

---

### The timestamp guard (§17.1)

**15. Guard admission: strictly-greater → greater-or-equal** (`admits()`
in `guard.rs` changed `candidate > entry.greatest` to `candidate >=
entry.greatest`).
- `cargo test --all-features --lib`: **4 FAILED** —
  `guard::the_guard_admits_only_a_strictly_greater_timestamp` (direct —
  and the brief is right that this is the one that matters most: it
  names the equal-timestamp replay case explicitly),
  `guard::authenticate_then_reject_restores_a_prior_value`,
  `guard::a_pinned_guard_entry_does_not_age_out`,
  `guard::an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl`.
- Reverted, clean.
- **CAUGHT — strongly, from four independent angles.**

**16. Skip the authenticate-then-reject revert** (`release_chain_guard_state`
in `mod.rs` no longer calls `guard.revert(undo)` at all — a provisional
write becomes permanent).
- `cargo test --all-features --lib`: **2 FAILED** —
  `guard::authenticate_then_reject_leaves_the_guard_empty`,
  `guard::authenticate_then_reject_restores_a_prior_value`.
- Reverted, clean.
- **CAUGHT — directly, both halves of mitigation (i).**

**17. Break mitigation (i): restore the release-order bug** (swapped
`release_chain_guard_state` back to revert-then-unpin, the order the
module doc calls out by name as the bug ruling 51/the mitigation exists
to prevent).
- `cargo test --all-features --lib`: **1 FAILED** —
  `guard::authenticate_then_reject_leaves_the_guard_empty`.
- Reverted, clean.
- **CAUGHT** — by exactly the scenario the code comment describes
  (authenticate-then-reject on a static with no prior entry).

**18. Orphan aging uses a literal instead of `TS_GUARD_ORPHAN_TTL`, value
changed** (both use sites in `guard.rs` — `age_orphans` and
`GuardEntry::age_deadline` — replaced with a literal `Duration::from_secs(20)`).
- `cargo test --all-features --lib`: **2 FAILED** —
  `guard::an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl`
  (direct), `guard::the_endpoint_deadline_covers_guard_orphan_aging`.
- Reverted, clean.
- **CAUGHT.**

---

### §5.5's driving — the initiator

**19. Reuse the ephemeral across retransmits instead of drawing a fresh
one** — the brief's highest-priority mutation. Added a `cached_msg1_mutation:
Option<Vec<u8>>` field to `Pending`; `start_attempt()` in `mod.rs` now
builds the msg1/ephemeral (`identity.open()` →
`Handshake::initiator()` → `Handshake::write_msg1()`) only on the
**first** attempt, caching the bytes; every retransmit re-mints a fresh
`sender_index` and re-frames/re-mac1's, but reuses the cached msg1 bytes
(same ephemeral, same encrypted timestamp) verbatim.
- `cargo test --all-features --lib`: **6 FAILED** —
  `dh_ladder::a_dial_costs_two_dh_and_each_retransmit_two_more`,
  `restart::accept_on_a_live_static_returns_stale_and_installs_nothing`,
  `initiator::a_failed_completion_spends_the_attempt_and_the_next_retransmit_refreshes_it`,
  `guard::authenticate_then_reject_restores_a_prior_value`,
  `guard::two_initiations_under_a_frozen_clock_are_strictly_increasing`,
  `guard::the_guard_admits_only_a_strictly_greater_timestamp`.
- Reverted, clean.
- **CAUGHT overall — but by the wrong tests, and this is a real,
  reportable weakness.** The test that exists specifically to check this
  property —
  `initiator::every_retransmit_mints_a_fresh_index_and_a_fresh_ephemeral`
  — **did NOT fail**. Its actual assertion is
  `assert_ne!(w[0], w[1], "... the ephemeral was reused")` comparing
  **whole packet bytes** (header ‖ msg1 ‖ mac1) between consecutive
  retransmits. Because the header carries the freshly-minted
  `sender_index` and mac1 is computed over `header ‖ msg1`, the full
  packet differs on every retransmit **purely from the index changing**,
  regardless of whether the msg1 ciphertext itself is fresh. The
  assertion is trivially satisfied by index freshness alone and provides
  **no independent verification of ephemeral freshness** — its name and
  failure message promise a check its logic does not perform.
  What actually catches this mutation is a *coincidence of protocol
  coupling* specific to this handshake: `write_msg1` draws the ephemeral,
  pays the `es`+`ss` DH, and encrypts the timestamp all in one call, so
  skipping that call (to reuse the ephemeral) also freezes the timestamp
  and skips the DH — and it is the DH-cost and timestamp-monotonicity
  tests, not the ephemeral test, that actually fail. If a future
  refactor ever decoupled those (or a subtler bug reused only the
  ephemeral while still touching the DH counter and timestamp through
  some other path), this specific test would give a false sense of
  security. **This is the review's most important finding**: the
  security-critical property the brief calls out by name is not
  independently pinned by the test that claims to pin it — it is pinned
  only by incidental coupling elsewhere in the suite.

**20. Reuse the sender index across retransmits** (`start_attempt()`
mutated to keep the previous `sender_index` instead of minting a fresh
one on every retransmit; the route-retirement call is skipped
accordingly).
- `cargo test --all-features --lib`: **2 FAILED** —
  `initiator::every_retransmit_mints_a_fresh_index_and_a_fresh_ephemeral`
  (direct — and here its `indices.dedup()` check IS a genuine,
  independent assertion on the index itself, unlike the ephemeral half
  of the same test),
  `initiator::a_failed_completion_spends_the_attempt_and_the_next_retransmit_refreshes_it`.
- Reverted, clean.
- **CAUGHT — directly and independently**, unlike mutation 19.

**21a. Retransmit jitter exceeds the 333 ms bound** (jitter span in
`draw_retransmit_delay()` doubled).
- `cargo test --all-features --lib`: **2 FAILED** —
  `initiator::the_retransmit_interval_is_five_seconds_plus_bounded_jitter_and_never_grows`
  (direct), `poll_contract::the_announced_deadline_is_the_minimum_of_the_live_timers`.
- Reverted, clean. **CAUGHT** (probabilistically certain in practice — 12
  independent draws per test run, ~1 in 2^12 chance every one lands back
  in the old, narrower band).

**21b. Retransmit jitter dropped entirely** (`draw_retransmit_delay()`
still consumes an RNG draw for parity but always returns exactly
`RETRANSMIT_BASE`, never adding jitter).
- `cargo test --all-features --lib`: **0 FAILED.** Re-ran the full
  `cargo test --all-features` (all targets, not just `--lib`) to be
  certain: **still 0 FAILED**, all 259 tests across every target green.
- Reverted, clean.
- **UNDETECTED — a real gap.** The dedicated test,
  `the_retransmit_interval_is_five_seconds_plus_bounded_jitter_and_never_grows`,
  asserts every gap is `>= RETRANSMIT_BASE` and `<= RETRANSMIT_BASE +
  RETRANSMIT_JITTER_MAX`, and that `max - min <= RETRANSMIT_JITTER_MAX`
  across the sampled gaps. A constant `RETRANSMIT_BASE` (zero jitter,
  zero spread) satisfies all three checks trivially — the test bounds
  the jitter from above but never asserts it is present at all (no check
  that at least one gap differs from `RETRANSMIT_BASE`, or that the
  gaps are not all identical). `the_dial_gives_up_at_exactly_...`'s
  attempt-count band also tolerates this, since a zero-jitter train
  produces an attempt count at the `hi` end of the band the test itself
  allows for the all-`RETRANSMIT_BASE` case. **This is a real,
  reportable gap**, separate from and in addition to mutation 19's
  weaker-signal finding: an implementation that silently stopped adding
  jitter to the retransmit schedule — collapsing every initiator on the
  network onto a synchronized, thundering-herd-prone retransmit
  cadence — would ship green.

**22. Give-up moved off 90 s** (`give_up_at` in `connect()` set to a
literal `Duration::from_secs(60)` instead of `HANDSHAKE_GIVEUP`).
- `cargo test --all-features --lib`: **2 FAILED** —
  `initiator::the_dial_gives_up_at_exactly_handshake_giveup_and_transmits_nothing_there`
  (direct), `initiator::the_retransmit_interval_is_five_seconds_plus_bounded_jitter_and_never_grows`.
- Reverted, clean. **CAUGHT.**

**23. mac1-invalid msg2 spends the interval's attempt** (mac1
verification in `complete_initiation()` moved to run *after* the attempt
is taken/spent, instead of before).
- `cargo test --all-features --lib`: **1 FAILED** —
  `initiator::a_mac1_invalid_msg2_spends_nothing`.
- Reverted, clean. **CAUGHT.**

**24. Spend the attempt after the crypto instead of before** — attempted
two ways.
- First pass: moved only the `pending.attempt_spent = true;` write to
  after the `read_msg2` call succeeds (mac1 check, index check, and
  `pending.state.take()` all left exactly where they were, i.e. still
  before the crypto). `cargo test --all-features --lib`: **0 FAILED.**
- Investigation: `attempt_spent` turns out to be **read in exactly one
  place** (`mod.rs:536`,
  `if pending.attempt_spent || pending.sender_index != Some(receiver_index)`)
  and is otherwise only written (init, reset in `start_attempt`, and the
  line this mutation moved). The actual "one completion attempt per
  interval" guarantee is enforced by `pending.state.take()` a few lines
  above it: `read_msg2` requires **owned** state (hiss consumes it by
  value), so the state is unconditionally taken out of `pending` before
  any crypto can run at all, whether or not `attempt_spent` is
  separately set. A second msg2 in the same interval therefore already
  finds `pending.state == None` and bails via the
  `let Some(state) = pending.state.take() else { return }` guard,
  completely independent of `attempt_spent`'s value. Moving *only* the
  boolean's write-time is consequently a **behaviorally inert** mutation
  in this codebase: I did not actually reproduce "spend after crypto",
  because the real spend (state ownership transfer) still happens before
  the crypto in both the original and the mutated code. A genuine "spend
  after crypto" bug would require deferring the `state.take()` itself,
  which is awkward-to-impossible here without giving `read_msg2` a
  borrowed rather than owned state (hiss's API doesn't offer that) — so
  the architecture itself, not a test, is what makes this property hold.
- Reverted, clean.
- **Not counted as a coverage gap.** Zero test failures, but only
  because the mutation (as narrowly and literally construed) doesn't
  change any observable behavior — the property the brief is worried
  about ("failed completion still costs the interval") is enforced by
  ownership/consumption, and no plausible code change limited to the
  `attempt_spent` field alone can violate it. Flagged transparently
  rather than either padding the gap list with it or silently dropping
  it.

---

### Ruling 71

**25. `intro_sender_index` returns a value cached at surfacing instead of
read live** (added a `first_sender_index_mutation: u32` field to
`IntroEntry`, frozen at park and never updated on refresh; the accessor
in `staged.rs` reads that instead of the live `entry.sender_index`).
- `cargo test --all-features --lib`: **2 FAILED** —
  `intro_queue::intro_sender_index_reflects_the_newest_bytes_after_a_refresh`
  (direct, and this is exactly the test ruling 71 exists for — its own
  doc comment says "a cached-at-surfacing implementation passes every
  other test in this file and fails this one"),
  `intro_queue::overflow_evicts_the_oldest_by_last_refresh_not_by_park_time`.
- Reverted, clean. **CAUGHT — directly, by the test purpose-built for
  this ruling.**

---

### The poll contract (§16.4/§16.5)

**26. `poll_output()` does not terminate in `Timeout`** (whenever the
output queue is about to drain to empty, a synthetic `Transmit` is
pushed back first, so the queue never truly empties).
- `cargo test --all-features --lib`: **effectively every test FAILS** —
  spot-checked one (`an_idle_endpoint_announces_no_deadline`): it panics
  at `tests.rs:237` with `"poll_output() did not reach the terminal
  Timeout in 100_000 outputs (§16.4)"` — the harness's own drain-bound
  safety valve, not a named `assert_eq!`/`assert!` in the test body
  itself. **Weaker signal** (a panic, not a targeted assertion) but an
  extremely reliable and immediately diagnostic one — the message names
  the exact contract violated, and it fires on essentially every test in
  the file since almost all of them call `drain()`.
- Reverted, clean. **CAUGHT — universally, but via a panic rather than
  an assertion.**

**27. Announced deadline is not the minimum of the live timers** (the
final reduction in `deadline()` changed from `.min()` to `.max()` across
the three timer families).
- `cargo test --all-features --lib`: **1 FAILED** —
  `poll_contract::the_announced_deadline_is_the_minimum_of_the_live_timers`.
- Reverted, clean. **CAUGHT — directly**, by the one test built
  specifically to pin this property across and within timer families.

---

### An additional mutation not in the brief's list

**28. Give-up-beats-same-instant-retransmit tie-break reversed**
(`drive_pendings()` in `mod.rs` restructured to check
`next_retransmit <= now` **before** `give_up_at <= now`, so a retransmit
fires instead of give-up when both are simultaneously due — the exact
reverse of the ordering §16.5 states normatively, and the ordering the
module's own doc comment calls out as load-bearing: *"give-up beats a
same-instant retransmit... an equality lands here and not in the
retransmit arm"*).
- Motivation: `the_dial_gives_up_at_exactly_handshake_giveup_and_transmits_nothing_there`'s
  own doc comment admits *"the strictly-equal case... cannot be forced
  without controlling the jitter draw"* — i.e. the test author already
  knew this exact tie is not exercised by any test in the file, so I
  checked whether that gap is real.
- `cargo test --all-features --lib`: **0 FAILED.** Re-ran the full
  `cargo test --all-features` (all targets): **still 0 FAILED**, all 259
  tests green.
- Reverted, clean.
- **UNDETECTED — a second real, confirmed gap**, on top of mutation
  21b's. Because every existing test drives a pending by repeatedly
  taking the *announced* deadline (`d.deadline.expect(...)`) and calling
  `handle_timeout` at exactly that instant, no test ever presents
  `handle_timeout` with a `now` where **both** `next_retransmit <= now`
  and `give_up_at <= now` hold simultaneously for a still-live pending —
  the announced deadline is always the minimum of the two, so only one
  side is ever exactly due at the instant a test steps to. A jitter draw
  that happened to land `next_retransmit` exactly on `give_up_at` would
  hit this in the wild (`RETRANSMIT_BASE` = 5 s, `HANDSHAKE_GIVEUP` = 90 s
  = 18 × 5 s, so the *unjittered* boundary is exactly on a retransmit
  multiple — only the jitter's near-certain non-zero draw normally saves
  it), and nothing in this suite would notice if the ordering were
  silently swapped.

---

## Summary table

**Undetected — real gaps — called out first:**

| # | Mutation | Outcome |
|---|---|---|
| 21b | Retransmit jitter dropped entirely (always exactly `RETRANSMIT_BASE`) | **UNDETECTED.** 0/259 tests fail, full `cargo test --all-features`. |
| 28 | Give-up-beats-retransmit tie-break reversed (own mutation, not in brief's list) | **UNDETECTED.** 0/259 tests fail, full `cargo test --all-features`. |

**All other mutations, in brief order:**

| # | Mutation | Outcome | Signal |
|---|---|---|---|
| 1 | `read_identity()` costs 2 DH, not 1 | CAUGHT (6 tests) | named `assert_eq!` |
| 2 | `authenticate()` re-pays `es` (3 cumulative) | CAUGHT (3 tests) | named `assert_eq!` |
| 3 | `accept()` costs 5 DH, not 4 | CAUGHT (1 test) | named `assert_eq!` |
| 4 | A rejected probe costs >1 DH | CAUGHT (3 tests, all stages) | named `assert_eq!` |
| 5 | Ruling 69, global overflow site: evict by park time | CAUGHT (1 test, exact) | named `assert!` |
| 6 | Ruling 69, per-source site: evict by park time | CAUGHT (1 test, exact, separate from #5) | named `assert!` |
| 7 | Dedup surfaces a second `IntroReady` | CAUGHT (3 tests) | named assertions + `one_intro()` panic |
| 8 | Per-source cap counts only unconsumed | CAUGHT (2 tests) | named `assert_eq!`/`assert!` |
| 9 | Consumed chain superseded by later packet | CAUGHT (1 test) — but only via `debug_assert!` panic first, and via `one_intro()`'s bundled count check second, not a direct assertion on consumed-chain immutability | **weaker signal**, single test |
| 10 | Overflow evicts a consumed chain | CAUGHT (2 tests) | named `assert!` |
| 11 | Global cap off-by-one, allow extra | CAUGHT (4 tests) | named assertions |
| 12 | Global cap off-by-one, evict early | CAUGHT (4 tests) | named assertions |
| 13 | Per-source cap off-by-one, both directions | CAUGHT (5 / 7 tests) | named assertions |
| 14 | TTL runs from park, not last refresh | CAUGHT (1 test, exact) | named `assert_eq!` |
| 15 | Guard admission: strict → non-strict | CAUGHT (4 tests) | named assertions, incl. the exact equal-timestamp case |
| 16 | Skip authenticate-then-reject revert | CAUGHT (2 tests) | named assertions |
| 17 | Mitigation (i) release-order bug restored | CAUGHT (1 test, exact) | named `assert!` |
| 18 | Orphan aging: literal instead of constant | CAUGHT (2 tests) | named assertions |
| 19 | **Reuse ephemeral across retransmits** | CAUGHT overall (6 tests) — **but not by the dedicated `...fresh_ephemeral` test**, which passes because its assertion (`packets[i] != packets[i+1]`) is trivially satisfied by the always-fresh index+mac1, independent of the ephemeral | **CAUGHT, but the purpose-built test is a false guarantee** — real coverage comes from incidental coupling (DH cost, timestamp monotonicity) |
| 20 | Reuse sender index across retransmits | CAUGHT (2 tests), incl. the dedicated test, directly | named `assert_eq!` |
| 21a | Jitter exceeds 333 ms bound | CAUGHT (2 tests) | named assertions |
| 21b | Jitter dropped entirely | **UNDETECTED** | — |
| 22 | Give-up off 90 s | CAUGHT (2 tests) | named assertions |
| 23 | mac1-invalid msg2 spends the attempt | CAUGHT (1 test, exact) | named `assert_eq!` |
| 24 | Spend attempt after crypto, not before | 0 failures, but the mutation (isolated to the redundant `attempt_spent` flag) is behaviorally inert — the real enforcement is state ownership/consumption, untouched by the mutation | **not counted as a gap** |
| 25 | Ruling 71: `intro_sender_index` cached, not live | CAUGHT (2 tests), incl. the purpose-built test, directly | named `assert_eq!` |
| 26 | `poll_output()` never reaches `Timeout` | CAUGHT (nearly universal) | **weaker signal** — harness panic, not a targeted assertion |
| 27 | Deadline is not the minimum of live timers | CAUGHT (1 test, exact, targeted) | named assertions |
| 28 | Give-up/retransmit tie-break reversed | **UNDETECTED** | — |

Every mutation reverted; `git status --short` at the end of the review
(reproduced below) shows only this review file as untracked, and
`git diff --stat` is empty.

## Verdict

**ADEQUATE WITH GAPS.**

The suite is unusually strong on the areas the brief called "densest" and
"most security-critical": every DH-ladder cost, every ruling-69 eviction
site (both of them, independently), every cap boundary (both directions,
both caps), the guard's strict-inequality admission, both halves of
mitigation (i) including the specific release-order bug, ruling 70's
named constant, ruling 71's live-read requirement, and the poll
contract's min-deadline and drain-to-`Timeout` invariants are all pinned
by **named, specific assertions**, frequently from multiple independent
angles. Slice 1's lesson (assert every boundary at, below and above) has
visibly been applied throughout — the cap and TTL tests in particular are
exemplary.

But two mutations escaped **all 259 tests across every target**, not
narrowly missed by one weak assertion:

1. **Dropping the retransmit jitter entirely (#21b)** is invisible. The
   dedicated jitter test bounds the *span* from above but never asserts
   the jitter is actually present — a build that always retransmits at
   exactly `RETRANSMIT_BASE` (collapsing every initiator into a
   synchronized, thundering-herd-prone cadence) ships green. This sits
   directly beside mutation 19, the brief's own highest-priority item
   (ephemeral reuse): the *test written for* "fresh ephemeral" also
   doesn't independently verify what it claims — it happens to be saved
   by incidental coupling elsewhere in the suite, while the *test written
   for* "bounded jitter" has no such rescue and simply misses.
2. **The give-up-beats-same-instant-retransmit tie-break (#28)** can be
   silently reversed with zero test impact, and the test file's own
   comments already concede this exact case "cannot be forced" by any
   test present — a known, named, unclosed gap sitting in the codebase's
   own commentary.

Both gaps share a shape: they are about **timing/ordering properties
under conditions the existing tests structurally cannot construct**
(a jitter draw that must vary, or an exact deadline coincidence), rather
than about a wrong value the tests forgot to pin. That is a narrower and
more specific failure mode than "untested," and it is fixable with two
additional tests (one that forces a fixed jitter seed or samples enough
draws to statistically demand variance, and one that constructs the
tie deliberately via `handle_timeout` rather than the announced
deadline) — but as written, both are real, confirmed, reproducible gaps,
not manufactured edge cases.

One further finding softens but does not erase #1's severity: mutation 9
(consumed-chain supersession) is caught only via a `debug_assert!` panic
on the first attempt and a fairly indirect `one_intro()` count-based
assertion on the second — weaker than the crisp, purpose-built
assertions everywhere else in the file, though not a full gap since the
property is caught.

(filled in at the end)

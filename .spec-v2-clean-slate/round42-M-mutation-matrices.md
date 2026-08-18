# Mutation report: ruling 271 ACK cadence (`src/core/connection/ack.rs`)

Blind mutation runner. Worktree base: commit `e5c8b1e` ("Ruling 271
integration, part 2"). No SPEC.md / rulings.md consulted.

## Base commit verification

`git log --oneline -1` at start showed `e5c8b1e Ruling 271 integration,
part 2: the twelve old-cadence pins rewritten` — already at the correct
base commit. `git status` was clean. No reset needed.

## Baseline test run (22 tests + full `--lib`)

- `cargo test --all-features --lib tests_ack_cadence`: **10 passed, 0
  failed** (the blind author's cadence tests).
- `cargo test --all-features --lib connection::tests_ack`: **58 passed,
  0 failed**. Note: this filter substring-matches both
  `connection::tests_ack::*` (48 tests across mods `range_semantics`,
  `derivation`, `ack_delay`, `policy`, `policy_on_the_wire`,
  `processing`, `two_cores`) AND `connection::tests_ack_cadence::*` (the
  same 10 from above), since `tests_ack_cadence` contains `tests_ack` as
  a substring. So this run's 58 = 48 + 10, not a disjoint set.
- Smoke tests (`-- smoke::a_snapshot_settles smoke::an_ack_drains
  smoke::the_first_in_order_packet_arms`, exact names resolved via grep:
  `smoke::the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now`,
  `smoke::an_ack_drains_the_sent_map_and_completes_the_send_half`,
  `smoke::a_snapshot_settles_only_once_its_bytes_are_acknowledged`):
  **3 passed, 0 failed**.
- Full `cargo test --all-features --lib`: **776 passed, 0 failed, 0
  filtered out** (baseline).

All 22 named tests (10 cadence + 9 rewritten-ish, actually 48 in
tests_ack + 3 smoke, all green) confirmed green at base. Total unique
tests touched by the brief's three filters: 10 (cadence) + 48
(tests_ack proper, includes the "9 rewritten" plus many untouched) + 3
(smoke) = 61 union, all passing. Per-mutant tracking below focuses on
which of these go red.

## Source under mutation

(pending — `on_recv` logic, `ACK_ELICITING_PER_ACK`, `ACK_COALESCE_MAX`)

## M1: never-arm (every-2nd trigger becomes a no-op, falls through to below-threshold Arm)

Mutation applied in `on_recv`, the `since_ack >= ACK_ELICITING_PER_ACK`
block: removed `self.pending = true; return AckAction::Arm(now);` after
the valve check, leaving execution fall through to the final
`AckAction::Arm(now + constants::MAX_ACK_DELAY)` — i.e. the every-2nd
trigger becomes indistinguishable from below-threshold, except that
`since_ack` still increments and the valve (`>= ACK_COALESCE_MAX`) still
fires.

Results:
- `tests_ack_cadence` (10 tests): **1 failed** —
  `a_single_lost_packet_is_recovered_without_advancing_the_clock`
  (`src/core/connection/tests_ack_cadence.rs:625`): `assertion left ==
  right failed: §13.2: the lost packet is declared by the packet
  threshold and retransmitted in the same instant — no virtual time
  passed / left: 0 / right: 8192`.
- `connection::tests_ack` filter (58 total incl. the 10 above): **10
  failed** total — the cadence one above, plus 9 in `tests_ack.rs`:
  - `policy::an_ack_is_owed_after_every_second_ack_eliciting_packet`
  - `policy::an_unpacked_debt_survives_further_arrivals` — `"§12.4: the
    2nd leaves an ACK due"`
  - `policy::packing_an_ack_resets_the_every_second_counter_not_only_the_debt`
    — `Arm(...597938666) != Arm(...572938666)` (25ms off — falls back to
    `now + MAX_ACK_DELAY`)
  - `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter`
    — `"§12.4: the 2nd *fresh* ack-eliciting packet makes the ACK due"`
  - `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_makes_it_due_now`
    — `"§12.4 (ruling 271): re-armed at now itself — already due"`
  - `processing::a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight`
    — different failure shape: `"the counter space must be deep enough
    for 64 descending pairs, got 13"` (a setup-loop side effect of
    counters not being acked/retired since the ACK cadence changed —
    still attributable to M1)
  - `processing::a_keepalive_elicits_no_ack_but_is_still_acknowledged` —
    `"§12.4 (ruling 271): …and this is the 2nd"`
  - `processing::a_padding_only_packet_elicits_no_ack` — `left: 0 !=
    right: 1`
  - `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest`
    — `"§12.4 (ruling 271): the 2nd re-arms at this packet's now, which
    is earlier than the 1st's t + 25 ms"`
- Smoke (3 tests): **all 3 failed**:
  - `an_ack_drains_the_sent_map_and_completes_the_send_half` — `"§12.4
    (ruling 271): the coalesced ACK waits on a due deadline"`
  - `a_snapshot_settles_only_once_its_bytes_are_acknowledged` — `"§16.2:
    the snapshot covers what was handed over at the call"`
  - `the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now`
    — failed (assertion not captured verbatim in tail truncation, but
    present in the `FAILED` failure list)
- Full `--lib` suite: **16 failed, 760 passed** (base was 776/0). The 13
  from above plus 3 more, all in `tests_streams`:
  - `tests_streams::precursors::s12_precursor_two_cores_exchange_a_finished_stream`
  - `tests_streams::precursors::s17_precursor_the_credit_ledger_stalls_and_resumes`
  - `tests_streams::slice_boundary::a_send_half_reports_finished_once_the_peer_acknowledges`

**M1 verdict: caught**, by both the author's cadence tests and 9 of the
tests_ack rewrites, plus 3 smoke tests, plus 3 unrelated stream tests
that depend on ACK cadence transitively. Matches the brief's hypothesis.

Reverted with `git checkout -- src/core/connection/ack.rs`; `git status
--short` showed only the report file as untracked afterward, confirming
a clean revert.

## M2: gap-drop (out-of-order arrival takes the in-order path)

Mutation applied in `on_recv`: removed the entire
`if !in_order { self.owed = true; return AckAction::Now; }` block,
leaving `in_order` computed but unused (bound to `_`). Every
ack-eliciting packet — in-order **or** out-of-order — now falls through
to the every-2nd-counter logic.

**Important nuance**: the code's `in_order` computation is
`prev_greatest.is_some_and(|g| g.checked_add(1) == Some(counter))`,
which is `false` both for a true gap **and** for the session's very
first ack-eliciting packet (`prev_greatest == None`). So this single
mutation — as literally described by the brief ("make an out-of-order
arrival take the in-order path") — necessarily also breaks the vacuous
first-packet-is-ACKed-immediately behavior, since the code does not
distinguish the two on this branch. That shows up below.

Results:
- `tests_ack_cadence` (10 tests): **3 failed**:
  - `a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack` —
    `"§12.4: the session's first ack-eliciting packet is ACKed
    immediately" / left: 0 / right: 1`
  - `a_gap_mid_batch_is_acked_in_the_same_batch` — `"§12.4: out-of-order
    arrival is ACKed immediately; got []"`
  - `twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack`
    — `"the vacuous first ACK" / left: 0 / right: 1`
- `connection::tests_ack` filter (58 total): **15 failed** (12 in
  `tests_ack.rs` + the 3 cadence ones above):
  - `ack_delay::the_delay_is_the_measured_microseconds_since_the_largest_arrived`
  - `policy::out_of_order_arrival_forces_an_immediate_ack_and_in_order_does_not`
    — `"§12.4: it opens a gap"`, `Arm(...) != Now`
  - `policy::the_first_ack_eliciting_packet_of_a_session_is_acked_immediately`
    — `Arm(...) != Now` (confirms the nuance above)
  - `policy_on_the_wire::a_gap_in_the_counter_space_is_acked_in_the_same_drain`
    — `"counter 0, immediate" / left: 0 / right: 1`
  - `policy_on_the_wire::a_pure_ack_packet_is_quiet_unelicited_and_untracked`
    — `"one packet out" / left: 0 / right: 1`
  - `policy_on_the_wire::a_repeated_timeout_at_the_same_instant_emits_no_second_ack`
  - `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter`
    — `"counter 0, immediate"`
  - `policy_on_the_wire::an_owed_ack_rides_a_pending_data_packet_rather_than_going_alone`
    — `"§12.4: one owed ACK, so exactly one ACK on the wire"`
  - `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_makes_it_due_now`
    — `"§12.4: the first is immediate"`
  - `policy_on_the_wire::the_delayed_ack_fires_at_twenty_five_milliseconds_and_not_before`
  - `processing::an_ack_naming_only_untracked_counters_retires_nothing` —
    **panics with `"attempt to subtract with overflow"`** at
    `tests_ack.rs:1943` (a genuine crash, not just an assertion
    mismatch — the mutation cascades into an arithmetic underflow
    elsewhere in the test's setup/assertions).
  - `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest`
- Smoke (3 tests): **1 failed**:
  `the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now`
  — `"§12.4: the first arrival ACKs immediately" / left: 0 / right: 1`.
  The other two smoke tests passed (their first packet must not be the
  session's very first ack-eliciting packet, or the mutation's effect
  doesn't reach an observed assertion in those flows).
- Full `--lib` suite: **27 failed, 749 passed** (base 776/0). Beyond the
  19 from the 22-target set, 8 more went red in modules that were not
  named in the brief's filters at all:
  - `smoke::a_firing_pto_probes_a_flight_loss_detection_cannot_yet_judge`
    — `"the retransmission arrived"`
  - `smoke::the_pto_is_armed_only_while_something_is_in_flight` —
    `left: 40 / right: 0`
  - `tests_path::a_forged_ack_from_the_new_address_does_not_lift_the_budget`
  - `tests_path::an_ack_covering_everything_validates_nothing`
  - `tests_path::path_validation_emits_no_new_connection_event`
  - `tests_path::two_real_cores_validate_a_roamed_address`
  - `tests_recovery::admission::a_pure_ack_packet_is_never_tracked_in_flight`
  - `tests_roam::a_quiet_send_neither_advances_s_nor_suppresses_the_keepalive`
  - `tests_roam::a_re_mark_after_a_clear_records_a_strictly_greater_floor`
  - `tests_roam::any_ack_covering_the_floor_clears_the_mark`
  - `tests_roam::the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms`

  (Full list of 27 preserved verbatim: smoke×3, tests_ack×9,
  tests_ack_cadence×3, tests_path×4, tests_recovery×1, tests_roam×4,
  tests_ack::ack_delay×1, tests_ack::policy×2 — see console capture
  `/tmp/m2_full_complete.txt` referenced during the run, contents
  transcribed above.)

**M2 verdict: caught**, and far more broadly than the brief's narrow
"immediate-ACK-on-gap pins" — because the code's single `in_order` test
covers both the gap case and the vacuous-first-packet case, and because
downstream path-validation / roaming / recovery tests apparently rely
transitively on prompt ACKs (their own "immediate ACK" expectations get
disrupted too). This mutation is caught extremely widely.

Reverted with `git checkout -- src/core/connection/ack.rs`; confirmed
clean by `git status --short` (only the report file untracked).

## M3: arm-late (every-2nd trigger arms at now + MAX_ACK_DELAY instead of now)

Mutation applied: in the every-2nd-trigger branch, kept `self.pending =
true;` but changed the return from `AckAction::Arm(now)` to
`AckAction::Arm(now + constants::MAX_ACK_DELAY)`. So `pending` is set
correctly (unlike M1) but the deadline is deferred a full 25 ms instead
of being due-immediately.

Results:
- `tests_ack_cadence` (10 tests): **1 failed** —
  `a_single_lost_packet_is_recovered_without_advancing_the_clock`
  (same failure as M1: `"§13.2: the lost packet is declared by the
  packet threshold and retransmitted in the same instant — no virtual
  time passed" / left: 0 / right: 8192`).
- `connection::tests_ack` filter (58 total): **9 failed** (8 in
  `tests_ack.rs` + the 1 cadence one above):
  - `policy::an_ack_is_owed_after_every_second_ack_eliciting_packet` —
    `"§12.4 (ruling 271): the 2nd arms at now itself, already due"`
    (deadline-shape assertion)
  - `policy::packing_an_ack_resets_the_every_second_counter_not_only_the_debt`
    (deadline-shape: `Arm(...928908750) != Arm(...903908750)`)
  - `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter`
    — `"§12.4: the 2nd fresh ack-eliciting packet makes the ACK due"`
    (deadline-shape)
  - `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_makes_it_due_now`
    — `"§12.4 (ruling 271): re-armed at now itself — already due"`
    (deadline-shape)
  - `processing::a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight`
    — `"the counter space must be deep enough for 64 descending pairs,
    got 13"` — **NOT a deadline/AckDelay assertion**, a setup-depth
    check (same failure shape as M1)
  - `processing::a_keepalive_elicits_no_ack_but_is_still_acknowledged` —
    `"§12.4 (ruling 271): …and this is the 2nd"` (deadline-shape)
  - `processing::a_padding_only_packet_elicits_no_ack` — `left: 0 !=
    right: 1` — **NOT a deadline comparison**, a plain packet-count
    assertion
  - `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest`
    — deadline-shape

  **Notable difference from M1**: `policy::an_unpacked_debt_survives_
  further_arrivals` (which asserts `is_owed()==false` / due-not-emitted
  after the 2nd packet) **passes under M3** but failed under M1 — because
  M3 correctly sets `pending=true`, while M1 never sets it. This is the
  one test in the suite that distinguishes "never marks the debt
  pending" (M1) from "marks it pending but arms late" (M3).
- Smoke (3 tests): **all 3 failed**, same as M1:
  `a_snapshot_settles_only_once_its_bytes_are_acknowledged` (`"§16.2:
  the snapshot covers what was handed over at the call"`),
  `an_ack_drains_the_sent_map_and_completes_the_send_half` (`"§12.4
  (ruling 271): the coalesced ACK waits on a due deadline"`,
  deadline-shape), `the_first_in_order_packet_arms_the_timer_and_the_
  second_makes_it_due_now`.
- Full `--lib` suite: **15 failed, 761 passed** (base 776/0). Beyond the
  12 from the 22-target set (1 cadence + 8 tests_ack + 3 smoke), 3 more
  in `tests_streams` — the **same three** as M1:
  - `tests_streams::precursors::s12_precursor_two_cores_exchange_a_finished_stream`
    — `"every byte, exactly once" / left: 13898 / right: 65536`
  - `tests_streams::precursors::s17_precursor_the_credit_ledger_stalls_and_resumes`
    — `"no data after 12824 of 131071 bytes"`
  - `tests_streams::slice_boundary::a_send_half_reports_finished_once_the_peer_acknowledges`
    — `"§9.3 with §12: every byte and the FIN acknowledged..." / left: 0
    / right: 1`

**M3 answer to the brief's hypothesis — REFUTED.** The brief's
hypothesis was that *only* the rewritten `AckDelay == Some(t)`
deadline-shape assertions would catch M3. In fact **at least 4 of the 9
caught tests assert something other than a raw deadline `Instant`
comparison**: `a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_
flight` (a counter-depth setup assertion), `a_padding_only_packet_
elicits_no_ack` (a packet-count 0-vs-1 assertion),
`a_snapshot_settles_only_once_its_bytes_are_acknowledged` (a snapshot
byte-coverage assertion), and all 3 `tests_streams` failures (byte
totals / state-transition counts, nothing to do with `AckDelay`
directly). These fail because a 25 ms real deferral changes *downstream*
observable behavior (how much data got flushed, whether a snapshot
settled by a given point in virtual time, stream completion state) —
not because they inspect the timer value. So the effect of arming late
is far from confined to direct deadline-comparison tests; it cascades
into byte-count and completion-state assertions throughout the streams
and smoke test suites.

Reverted with `git checkout -- src/core/connection/ack.rs`; confirmed
clean by `git status --short`.

## M4: valve-delete (remove the counter >= ACK_COALESCE_MAX branch)

Mutation applied: removed the entire
```
if self.since_ack >= constants::ACK_COALESCE_MAX {
    self.owed = true;
    return AckAction::Now;
}
```
block from inside the `since_ack >= ACK_ELICITING_PER_ACK` branch,
leaving every trigger past the 2nd in-order ack-eliciting packet arm at
`now` (`pending = true; Arm(now)`) regardless of how large `since_ack`
grows. Compiled clean with no unused-constant warning (`ACK_COALESCE_MAX`
is still referenced in doc comments/elsewhere in the crate, e.g.
`constants.rs`'s own const-assert `ACK_ELICITING_PER_ACK <=
ACK_COALESCE_MAX`, which does not reference `ack.rs`'s deleted branch).

Results:
- `tests_ack_cadence` (10 tests): **10 passed, 0 failed.**
- `connection::tests_ack` filter (58 total): **58 passed, 0 failed.**
- Smoke (3 tests): **3 passed, 0 failed.**
- Full `--lib` suite: **776 passed, 0 failed** — bit-for-bit identical to
  the baseline pass/fail count.

**M4 answer — CONFIRMED, not refuted.** The prior agent's claim that
NOTHING goes red for the valve-delete mutant holds over the *entire*
`--lib` suite (776/776 identical to baseline), not just the 22
brief-named tests. This makes sense given what the deleted branch
protects: it only changes behavior once `since_ack >= ACK_COALESCE_MAX =
32` **in-order** ack-eliciting packets have arrived since the last ACK
was packed — i.e., a burst of at least 32 packets with no ACK packed in
between. None of the 776 tests in this suite (paused-clock, synthetic,
short-burst-oriented as ACK cadence tests tend to be) drive that deep a
burst without the ACK being packed/drained somewhere in the middle
(`tests_ack_cadence::a_burst_of_twenty_in_one_receive_batch_...` and
`twenty_datagrams_before_a_single_poll_output_loop_...` both use bursts
of 20, one short of the 32 threshold — an off-by-a-lot gap, not
off-by-one). This is a real, unaudited mutation-coverage hole: the
16-packet-short margin between the tests' burst size (20) and the
valve's threshold (32) means the valve is entirely untested by this
suite.

Reverted with `git checkout -- src/core/connection/ack.rs`; confirmed
clean by `git status --short`.

## Final restore verification

`git status` after all four mutants: clean, only `MUTATION-271-REPORT.md`
untracked. `git diff --stat`: empty (no tracked-file diff). Final
`cargo test --all-features --lib`:

```
test result: ok. 776 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.52s
```

Identical to the baseline run at the top of this report. No mutant was
left applied.

## Summary matrix

Red counts per mutant, per test group (of the group's total):

| Mutant | cadence (10) | tests_ack filter (58, incl. cadence's 10) | smoke (3) | full `--lib` (776) |
|---|---|---|---|---|
| M1 never-arm | 1 red | 10 red | 3 red | 16 red |
| M2 gap-drop | 3 red | 15 red | 1 red | 27 red |
| M3 arm-late | 1 red | 9 red | 3 red | 15 red |
| M4 valve-delete | 0 red | 0 red | 0 red | 0 red |

All four mutants reverted; base is 776/0 throughout, confirmed again at
the end.

**M3 hypothesis ("only the rewritten `AckDelay == Some(t)` deadline
assertions catch it") — REFUTED.** At least 4 of the 9 tests_ack-filter
failures assert something other than a raw `AckDelay`/`Instant`
comparison: a counter-depth setup check
(`a_full_sixty_four_pair_ack_...`), a plain packet-count check
(`a_padding_only_packet_elicits_no_ack`), a snapshot byte-coverage check
(`a_snapshot_settles_only_once_...`), and all 3 `tests_streams` failures
(byte totals / completion-state transitions). Deferring the coalescing
ACK's deadline by 25 ms cascades into throughput and completion-state
observables well beyond direct timer inspection.

**M4 claim ("nothing goes red") — CONFIRMED**, across the full 776-test
`--lib` suite, not just the 22 named tests. Root cause: the deleted
valve only changes behavior once 32 consecutive in-order ack-eliciting
packets arrive with no intervening ACK pack, and the deepest burst in
this suite (`a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack`,
`twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack`)
is 20 packets — 12 short of the threshold. This is a genuine
mutation-coverage gap in the current test suite, not a benign mutant: an
implementation that never applies the `ACK_COALESCE_MAX` valve (i.e.
coalesces without bound, contradicting the valve's own doc comment about
bounding "the deferral... by a property, and a scheduler that keeps this
receiver's socket non-empty holds the reverse path silent for as long as
it does so") would ship undetected by this test set.

## Surprises / notes

1. **M1 and M3 overlap almost completely** except for one test:
   `policy::an_unpacked_debt_survives_further_arrivals` fails under M1
   (never sets `pending`) but passes under M3 (correctly sets `pending`,
   only arms late). This is the single test in the whole 776-test suite
   that isolates "the debt flag itself" from "the deadline value" for
   the every-2nd trigger — worth knowing if the suite is ever pruned.
2. **M2's blast radius is much wider than the brief's framing.** The
   brief calls M2 "gap-drop" and expects "the immediate-ACK-on-gap pins"
   to go red, but the code's single `in_order` boolean is `false` for
   *both* a genuine gap *and* the session's very first ack-eliciting
   packet (`prev_greatest == None`). Deleting the `if !in_order` branch
   therefore also kills the vacuous-first-packet-ACKs-immediately
   behavior, and that ripples into path-validation, roaming, and
   recovery tests (`tests_path::*`, `tests_roam::*`,
   `tests_recovery::admission::a_pure_ack_packet_is_never_tracked_in_
   flight`) that were never mentioned in the brief — 27 total failures
   in the full suite, the largest of the four mutants. One of those
   (`processing::an_ack_naming_only_untracked_counters_retires_nothing`)
   is a genuine **panic** (`"attempt to subtract with overflow"`), not
   just a failed assertion — the mutation cascades into an arithmetic
   underflow in a downstream computation, not merely a wrong return
   value.
3. **M1 and M3 both also caught 3 identical `tests_streams` tests**
   (`s12_precursor_two_cores_exchange_a_finished_stream`,
   `s17_precursor_the_credit_ledger_stalls_and_resumes`,
   `slice_boundary::a_send_half_reports_finished_once_the_peer_
   acknowledges`) that are nowhere named in the brief's 22-test target
   set — confirming the brief's own instruction to "run the full `--lib`
   suite once and report its total red count" catches real collateral
   the narrow filters miss.
4. No reset of the worktree was needed: it was already correctly cut
   at `e5c8b1e` at the start of this run.

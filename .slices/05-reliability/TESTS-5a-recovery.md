# TESTS-5a — §13/§14 recovery tests (blind author)

Companion to `src/core/connection/tests_recovery.rs`. Written blind to the
slice-5a implementation and to the §12 ACK test author, from `SPEC.md`
§13/§14/§8.7/§16.5 and `CONTRACT-5a.md`.

## 0. The formulas these tests pin (with SPEC.md line refs)

- §13.1 (3647–3684). First sample: `srtt = latest`, `rttvar = latest/2`,
  `min_rtt = latest`. Later: `min_rtt = min(min_rtt, latest)`; peer
  `ack_delay` capped at `MAX_ACK_DELAY` subtracted **only when** doing so
  does not push the sample below `min_rtt`; then
  `rttvar = 3/4·rttvar + 1/4·|srtt − adjusted|`,
  `srtt = 7/8·srtt + 1/8·adjusted`. Pre-sample seed: `srtt =
  K_INITIAL_RTT` (333 ms), `rttvar = K_INITIAL_RTT/2`. `K_GRANULARITY`
  = 1 ms. Sample only when `largest` is **newly** acked (ruling 138,
  3660–3662); the ack-eliciting clause is vacuous and MUST NOT be
  implemented (3652–3659).
- §13.2 (3686–3710). Lost iff a later packet in the space is acked AND
  (`largest_acked − pn >= K_PACKET_THRESHOLD` (3) OR sent **more than**
  `loss_delay = max(9/8·max(srtt, latest_rtt), K_GRANULARITY)` before the
  ack arrived). `Loss` timer = min over **survivors below
  `largest_acked`** of `time_sent + loss_delay` (ruling 131, 3699–3707).
- §13.3 (3712–3743). `PTO = srtt + max(4·rttvar, K_GRANULARITY) +
  MAX_ACK_DELAY`, anchored at the last ack-eliciting send, `×2^pto_count`,
  capped at `PTO_BACKOFF_CAP` = 2^6. `pto_count` resets to 0 whenever any
  packet is newly acknowledged. Armed **only while** ≥1 ack-eliciting
  packet is in the sent map; `Loss` takes precedence when armed.
- §13.4 (3745–3756). Probe = one ack-eliciting packet, pending
  retransmittable frames oldest-first else a bare PING; `seal_quiet`;
  exempt from the cwnd gate, **not** from §7.3's budget.
- §13.5 (3758–3766). Sent map holds send time, frame identities, and the
  packet's **size in bytes** feeding `bytes_in_flight`.
- §14.2 (3813–3825). `INITIAL_WINDOW` 12000, `MINIMUM_WINDOW` 2400,
  `LOSS_REDUCTION_FACTOR` 0.5, `ssthresh` starts `u64::MAX`. Slow start
  (cwnd < ssthresh): cwnd += newly acked bytes. Congestion avoidance:
  integer ABC — accumulate acked bytes, add one `MAX_DATAGRAM` each time
  the accumulator exceeds cwnd.
- §14.3 (3827–3840). Congestion event: `cwnd = max(cwnd/2,
  MINIMUM_WINDOW)`, `ssthresh = cwnd`, recovery period starts at the
  event. Events **and growth** are both gated by `sent_time <=
  recovery_start`. One `on_congestion_event` per episode, after the full
  scan.
- §14.4 (3842–3860). Persistent congestion: two lost ack-eliciting
  packets more than `PTO(pto_count = 0) × 3` apart with **no packet acked
  between**, and **a prior RTT sample exists** ⇒ `cwnd = MINIMUM_WINDOW`.
- §14.5 (3862–3909). Gate: `bytes_in_flight + candidate <= cwnd`.
  `app_limited` stamped at send time on the packet; a set flag suppresses
  growth on that ack.
- §8.7 (2845–2870). Retransmission classes: ranges / regenerate / never.

## 1. Inventory — 61 tests in 10 modules

`cargo fmt` clean under edition 2024. The file does **not** compile in this
worktree: `recovery.rs` and `congestion.rs` do not exist yet. That is
expected (working rule 6) and no stub was created.

Every test carries a `Mutation caught:` line in its doc comment naming what
the broken build does; the column below is its summary.

### `mod rtt` — §13.1's estimator (10)

| test | broken build |
|---|---|
| `before_any_sample_the_estimator_reads_the_initial_seed` | seeds `rttvar = 0` → first PTO 358 ms, not 1024 ms |
| `the_first_sample_replaces_the_seed_outright` | runs the ⅞/⅛ recurrence on the *first* sample → 303.875 ms |
| `a_later_sample_follows_the_seven_eighths_recurrence_exactly` | `smoothed = latest` every time → 200 ms; also catches updating `smoothed` before `rttvar` via the `rttvar` assertion |
| `the_peer_ack_delay_is_subtracted_when_the_result_stays_above_min_rtt` | ignores `ack_delay` → 112.5 ms |
| `the_peer_ack_delay_is_refused_when_it_would_push_the_sample_below_min_rtt` | always subtracts → `smoothed` 98.125 ms, **below `min_rtt`** |
| `the_peer_ack_delay_is_capped_before_the_min_rtt_guard_is_applied` | guard without the cap → 100 ms; the `min_rtt` assertion above does *not* catch this one |
| `the_loss_delay_takes_the_larger_of_smoothed_and_latest` | `9/8 · smoothed`, dropping the `max` → 140.625 ms on a rising path |
| `the_loss_delay_never_falls_below_the_granularity_floor` | omits the `K_GRANULARITY` floor → 112.5 µs threshold on a fast path |
| `the_pto_interval_is_smoothed_plus_four_rttvar_plus_max_ack_delay` | omits `MAX_ACK_DELAY` (300 ms) or uses `rttvar` not `4 · rttvar` (175 ms) |
| `reseeding_lets_min_rtt_rise` | `reseed_min_rtt` a no-op → 109.375 ms |

### `mod loss` — §13.2 (12)

| test | broken build |
|---|---|
| `the_packet_threshold_declares_exactly_three_counters_below_the_largest` | threshold 2 (loses counter 1) or 4 (loses nothing) |
| `two_counters_below_the_largest_is_not_yet_lost` | threshold 2 — the other side of the boundary |
| `packets_above_the_largest_acked_never_arm_the_loss_timer` | **ruling 131's wide reading** — arms from counter 4, a timer that fires and declares nothing |
| `the_loss_timeout_walk_never_judges_packets_above_the_largest_acked` | ruling 131's second mode — the wide walk *acts*, declaring two recent packets lost |
| `the_loss_timer_arms_at_the_earliest_surviving_send_time` | takes the maximum / the last survivor visited → 5 ms late |
| `a_packet_exactly_loss_delay_old_is_not_yet_lost` | **ruling 139**: `>=` instead of `>` — v0.1's actual code |
| `a_packet_one_nanosecond_older_than_loss_delay_is_lost` | no time threshold at all — the other side |
| `the_loss_deadline_is_recomputed_rather_than_accumulated` | folds into the old minimum and never clears → a stale deadline that re-arms itself |
| `the_loss_timeout_declares_the_survivors_the_ack_walk_left` | `on_loss_timeout` stubbed to `default()` — every transfer still completes, via the PTO |
| `the_loss_timer_firing_at_its_own_deadline_declares_nothing` | *documents* conflict C5; fails loudly if `>` is quietly relaxed |
| `an_ack_that_acknowledges_nothing_new_is_a_total_no_op` | re-reports acknowledged packets → a repeated ACK inflates `cwnd` |
| `an_ack_above_the_highest_sealed_counter_is_ignored_whole` | trusts the peer's `largest` → the whole flight is declared lost on one forged frame |

### `mod sampling` — §13.1's "newly acknowledged" (2)

| test | broken build |
|---|---|
| `an_ack_whose_largest_was_already_acknowledged_yields_no_sample` | **ruling 138** — samples on any newly acked packet → `smoothed` 150 ms; the `acked` assertion proves the frame was not simply ignored |
| `the_sample_measures_from_the_largest_newly_acknowledged_packet` | measures from the *oldest* newly acked → 150 ms, inflated by the peer's ACK cadence |

### `mod pto` — §13.3 (6)

| test | broken build |
|---|---|
| `the_first_pto_deadline_is_the_send_plus_one_thousand_and_twenty_four_ms` | `rttvar = 0` seed, from the timer's side |
| `the_pto_is_disarmed_when_the_sent_map_empties` | arms from the anchor alone → §13.3's own ~20 packets/s self-sustaining probe train |
| `the_pto_anchors_at_the_last_ack_eliciting_send` | anchors at the oldest unacked (PLAN-5 §8-H6's other reading) |
| `each_pto_firing_doubles_the_interval` | increments at the probe's *send* (1×,1×,2×,4×), or adds instead of doubling (1×,2×,3× — agrees at two firings, which is why three are asserted) |
| `the_pto_backoff_multiplier_caps_at_sixty_four` | `1u32 << pto_count.min(PTO_BACKOFF_CAP)` — §4.3's named trap; the 64th firing is fired explicitly because that is where it detonates |
| `pto_count_resets_when_any_packet_is_newly_acknowledged` | resets only on the probe's own ack → `t + 2600 ms` instead of `t + 325 ms` |

### `mod persistent_congestion` — §14.4 inside §13.2's walk (4)

| test | broken build |
|---|---|
| `two_far_apart_losses_with_nothing_acked_between_are_persistent` | no §14.4 → halves under a total blackhole instead of collapsing |
| `a_run_broken_by_an_acknowledged_packet_is_not_persistent` | forgets the "nothing acked between" test → collapses to 2 400 B on any ordinary lossy path. **The most damaging false positive in §14** |
| `the_persistent_period_ignores_the_pto_backoff` | uses the backed-off PTO → 2 200 ms period, never triggers under the loss it exists to detect |
| `a_burst_of_close_together_losses_is_one_ordinary_episode` | reads §14.4 as "several packets at once"; also pins `lost_bytes` as the **sum** and `sent_time` as the **earliest** |

### `mod newreno` — §14.2/§14.3 (11)

| test | broken build |
|---|---|
| `a_new_controller_starts_at_the_initial_window_in_slow_start` | `ssthresh = INITIAL_WINDOW` → starts in congestion avoidance, an order of magnitude slower and never wrong |
| `slow_start_grows_by_exactly_the_bytes_acknowledged` | adds one MTU per ack — **identical** under MTU-sized acks, which is why 500/700/1200 are used |
| `congestion_avoidance_adds_one_datagram_per_crossing_and_carries_the_remainder` | drops the remainder (diverges at step 3) or uses `if` for `while` (diverges at step 4); each step asserted so the failure names the rule |
| `a_congestion_event_halves_the_window_and_sets_ssthresh_to_it` | leaves `ssthresh` at `u64::MAX`; or anchors `recovery_start` at `sent_time`, which fences nothing |
| `a_second_event_for_a_packet_sent_before_the_recovery_period_does_not_cut_again` | cuts per event → 12 000 → 2 400 in one burst; the equal case pins `<=` |
| `a_new_episode_after_the_recovery_period_cuts_again` | **positive control** — ignores every event after the first, which passes the test above perfectly |
| `the_window_never_falls_below_the_minimum` | no `MINIMUM_WINDOW` floor → 1 500, 750, 375 → live-lock behind the gate |
| `acknowledgements_of_packets_sent_before_the_recovery_period_do_not_grow_it` | implements only §14.3's event half → the pre-cut flight undoes the cut; third ack is the positive control |
| `an_app_limited_acknowledgement_does_not_grow_the_window` | never reads the flag; second ack is the positive control |
| `a_persistent_congestion_event_collapses_the_window_to_the_minimum` | passes `is_persistent` and never reads it; `ssthresh` asserted at 6 000, not 2 400 |
| `persistent_congestion_does_not_clear_the_recovery_period` | **ruling 139** — RFC 9002 §7.6.2's clearing lets the blackholed flight grow straight back out of the collapse |

### `mod admission` — §14.5, ruling 136, §13.4 (8, core-level)

| test | broken build |
|---|---|
| `the_initial_window_admits_exactly_ten_full_datagrams` | `<` for `<=` → nine packets. **Plaintext accounting still gives ten**, so the count is not the ruling-136 test — `bytes_in_flight() == 12 000` is |
| `a_tracked_packets_size_is_the_whole_datagram` | **ruling 136** at its sharpest: `size` as plaintext or as ciphertext-without-header |
| `acknowledging_the_flight_returns_bytes_in_flight_to_zero` | removes the entry without crediting the sum back → gates itself shut; also pins the ACK→§13.1 path through the core (`smoothed_rtt == 50 ms`) |
| `the_gate_holds_the_backlog_until_the_flight_is_acknowledged` | gates the first pass then forgets; or never reopens |
| `a_pto_probe_is_sent_with_the_window_full_and_is_counted_in_flight` | gate applied uniformly → a black-holed path with a full window is permanently stuck; or exempt from accounting too (ruling 43); count asserted as **one** against a build that flushes the backlog on every PTO |
| `a_packet_that_emptied_the_queue_earns_no_window_growth` | never stamps `app_limited` → phantom window on an idle connection |
| `a_window_limited_flight_grows_the_window_by_every_byte_it_carried` | **positive control** — stamps unconditionally, or derives the flag at ACK time (which §14.5 forbids for the hostile-peer reason it states) |
| `a_pure_ack_packet_is_never_tracked_in_flight` | records every sealed packet → the pure ACK is never acknowledged, is declared lost, cuts `cwnd`, and keeps the `Pto` armed forever on an idle receive-only connection |

### `mod fin` — ruling 113, PLAN-5 §6.2 (4)

| test | broken build |
|---|---|
| `a_range_ending_at_the_final_size_without_the_fin_does_not_finish_the_stream` | **T-FIN-A** — the inference `final_size == range.end`. Announces `DataRecvd` while the FIN is still unacknowledged; the peer's reader never sees EOF |
| `the_range_that_carried_the_fin_finishes_the_stream` | **T-FIN-B** — ignores the `fin` argument; T-FIN-A alone passes such a build |
| `a_lost_range_without_the_fin_does_not_resend_the_fin` | the inference from the loss side — clears `fin_sent` on any tail retransmission → "the FIN is re-sent forever" |
| `a_lost_range_that_carried_the_fin_resends_it` | never clears `fin_sent` → the FIN is never re-sent, hanging the peer's reader at EOF |

The pair the plan calls the highest-value test in the file is written **in
both directions**: the acknowledgement side (observable as
`ConnEvent::StreamFinished` and the freeing of the table entry) and the
loss side (observable on the wire as the FIN bit on the retransmission).
The degenerate inference passes B and fails A in both.

### `mod watermark` — PLAN-5 §6.4 (3)

| test | broken build |
|---|---|
| `acknowledging_our_own_streams_closure_grants_the_peer_nothing` | missing `!local` → every locally-opened stream we close inflates the peer's inbound allowance (§10.4 / RFC 9000 §4.6). Eight streams, so a batching build has enough grants to actually emit |
| `retiring_peer_opened_streams_does_grant_the_peer_credit` | **positive control** — grants for neither, which passes the above and starves the peer |
| `a_stream_frame_naming_a_local_index_we_have_closed_is_a_no_op` | treats the closed index as unknown and **re-opens** it — `accept(Dir::Bi)` then hands the application a stream in our own space (§8.4: *"ACKed, never re-opened"*) |

### `mod path_generation` — ruling 137 (1)

`a_sent_packet_carries_a_u32_path_generation_held_at_zero`. Pins the
field's existence and its `u32` type; see §3 for what it cannot pin.

## 2. Conflicts found

Reported, not resolved (working rule 3). Five, and two of them are the
defect class working rule 8 hunts — *a stated construction with an
unstated or contradicted scope*.

### C1 — `SentPacket` has no path generation in the contract's own struct

**Ruling 137** (`CONTRACT-5a.md` §0, binding, and `SPEC.md`:3934–3936):
*"`SentPacket` therefore carries a `u32` path generation from slice 5
onward, held at 0 until roaming exists."* **`CONTRACT-5a.md` §2.2's struct
listing** (lines 150–164) names `counter`, `time_sent`, `size`,
`app_limited`, `frames` — **and no path generation**.

§0's table says it overrides anything below that predates it, so ruling
137 governs and the field must exist. My brief also instructs me to assert
it is 0. **The field is written as `path_gen: 0`** in the single
`sent()` constructor in `tests_recovery.rs`; every `SentPacket` in the
file is built through it, so a different name costs one token in one
place. If the implementer read §2.2 as exhaustive — which working rule 8
says a list is read as, whether or not it says so — the field is absent
and that one line fails to compile. **That is the conflict surfacing, not
a test defect.**

### C2 — `Recovery::on_sent`'s stated assertion cannot hold

§2.2: *"Record one **ack-eliciting** packet. Asserts `!frames.is_empty()
|| probe`."* There is no `probe` parameter on `on_sent` and no `probe`
field on `SentPacket`, so the disjunct is unevaluable as written. It also
contradicts §2.2's own `frames` doc four lines above — *"**May be
empty**: a bare-PING PTO probe … is tracked … but carries nothing that
re-queues"* — and §17.5's caveat, which requires the exempt probes to be
in the map. A build that literally asserts `!frames.is_empty()` panics on
every PTO probe that finds no pending frames, which is §13.4's bare-PING
case. **My tests never construct an empty `frames` vector**, so they
neither rely on nor contradict the resolution; the probe is exercised
through the core instead (`admission::a_probe_is_sent_with_the_window_full…`).

### C3 — `testfix::parse_frames` panics on ACK, and slice 5 emits ACKs

`testfix.rs`:184–187 panics with *"slice 4 emitted frame type {other:#x},
which §8.3 does not place in this slice"* on any type it does not know,
and it knows no ACK. Its own doc calls that panic an assertion: *"a core
emitting an ACK in slice 4 has crossed the slice boundary."* In slice 5
the core emits ACKs, so **every `Solo::packets` / `Solo::drain_frames`
call site panics as soon as the packet under inspection carries one** —
including slice 4a's 73 tests in `tests_streams.rs`.

`testfix.rs` is the **integrator's** file (PLAN-5 §3, "The integrator's
files — working rule 15"): neither blind author may write it. It needs an
ACK arm before 5a can be green — not for my sake but for slice 4a's,
whose 73 tests decode packets from cores that have received data.

**My tests are written not to depend on how it is fixed.** Nothing here
compares a whole decoded frame vector by equality; every assertion over
decoded frames is an `any`/`!any` over a `matches!` pattern, so a new
`Wire::Ack` variant is ignored rather than fatal. Five tests decode at
all, and four of them run on a core that has received nothing and
therefore owes no ACK. **Exactly one —
`watermark::retiring_peer_opened_streams_does_grant_the_peer_credit` —
decodes from a core that has received packets**, and it is annotated in
its own doc comment as depending on this fix.

### C4 — §14.4's `has_sample()` guard is vacuous in slice 5

§14.4 requires *"**a prior RTT sample exists** (the pre-sample
`K_INITIAL_RTT` phase never triggers it)"*, and PLAN-5 §7's **R-14** asks
for a test that persistent congestion does not trigger before the first
sample. **That state is unreachable in slice 5.** The loss walk runs only
from an ACK or from the `Loss` timer; the `Loss` timer is armed only by a
previous walk; and any ACK that can drive the first walk has a newly
acknowledged `largest`, which §13.1 says yields a sample — taken before
loss detection, per RFC 9002's ordering. So by the time §14.4 is
evaluated, `has_sample()` is always true.

This is **ruling 138's shape exactly**: a condition carried across from
RFC 9002 that cannot be false in slither's design. It differs from 138 in
that it becomes *live* in slice 7 — §13.6 fences pre-roam packets from
producing an RTT sample, so a post-roam connection can walk losses with
no sample on the new path. **Recommendation: keep the guard** (it is a
fail-safe and slice 7 needs it) and record here that no slice-5 test can
exercise it. R-14 is therefore not written as specified; what *is*
written is the estimator-level pin that `has_sample()` is false before
the first sample and true after, which is the whole of what the guard
reads.

### C5 — ruling 139's strict `>` makes the `Loss` timer fire on nothing

§13.2 arms the timer at `time_sent + loss_delay`, and ruling 139 fixes
the time threshold as **`>`**. At exactly the armed instant the survivor's
age *equals* `loss_delay`, so the walk the timer's own firing triggers
declares nothing — and, if the implementation re-arms from the same
survivor, re-arms at the same instant. That is ruling 131's *"a timer that
fires and declares nothing"* reached by a different route.

RFC 9002 §6.1.2 avoids it by testing `time_sent <= now − loss_delay`,
i.e. the `>=` form ruling 139 explicitly rejects. Nothing in §13.2 says
whether a firing at the deadline should be treated as due. **Ruling 139
governs and the test follows it**
(`loss::the_loss_timer_firing_at_its_own_deadline_declares_nothing`); it
is flagged in its own doc comment as the first test to revisit if the
ruling moves. In practice a `Timers` implementation that fires at
`now >= deadline` will usually arrive a tick late and declare the loss
normally, so this is a latent spin rather than an observed one — but it is
latent by luck, not by design.

## 3. Not tested, and why

Working rule 13: *the fixture bounds the coverage.* Two of the four
seam-review findings were unreachable from all 451 tests **by
construction**. What follows is this file's equivalent list — written so
that nobody has to rediscover it by failing to write the test.

### 3.1 R-14 — persistent congestion before the first RTT sample

**Unreachable in slice 5.** See conflict C4 above: §14.4's `has_sample()`
guard cannot be false at the moment §14.4 runs, because every path into
the loss walk has already taken a sample. What is written instead is the
estimator-level pin that `has_sample()` is false before the first sample
and true after — the whole of what the guard reads. The guard should still
be implemented: §13.6 makes it live in slice 7.

### 3.2 The roam fences — `Recovery::on_roam` and `NewReno::reset`

PLAN-5 §7 declines these deliberately and I agree: both are **uncalled
until slice 7**, so a test would assert that the method exists rather than
that the protocol does.

**One addition, flagged because it departs from the plan's letter.**
`RttEstimator::reseed_min_rtt` *is* tested
(`rtt::reseeding_lets_min_rtt_rise`). It is a pure function on a
unit-testable struct and its arithmetic is assertable today without
inventing a roam; §13.1's clause *"`min_rtt` MUST be allowed to rise"*
would otherwise ship with no test at all and only become observable in
slice 7, inside the fence work, where a no-op implementation reads as
correct. The §13.6 fences themselves — path-generation stamping, the
congestion reset, the `app_limited` fence — remain untested and slice 7's.

### 3.3 §12.2's ACK-fidelity failure mode

PLAN-5 §11-F2's: an ACK-loss burst longer than the replay window's time
width. The fixture cannot produce it, and it is §12's in any case.

### 3.4 What the core cannot be asked

`CONTRACT-5a.md` §2.4 gives the core exactly three `#[cfg(test)]`
accessors — `bytes_in_flight`, `congestion_window`, `smoothed_rtt`. There
is **no** accessor for the sent map, for `pto_count`, for `ssthresh` at
the connection level, or for a `SentPacket`'s recorded `app_limited` or
`path_gen`. Consequences:

- **Ruling 137's "held at 0" is pinned only as a constant in the test's
  own construction.** Nothing observes what the core stamps. If the
  implementer stamps 7, no test in this file fails. Slice 7 will need
  either an accessor or a behavioural observable.
- **`app_limited` is observable only through window growth.** The pair of
  tests separates "never stamps" from "always stamps", and separates
  send-time recording from ACK-time derivation. They do **not** separate
  *which* packet of a multi-packet flight carries the flag — ruling 139's
  *"the packet that emptied the queue"* versus "every packet after the
  queue emptied" would need a two-packet flight whose first packet is
  acknowledged alone, and the ACK for a single counter out of a
  10-packet window is expressible, but the cwnd difference is one
  packet's growth either way only if the flight is app-limited *and*
  multi-packet, which the 500-byte fixture is not. **Left open
  deliberately; recorded here rather than papered over.**
- **The gate's placement** — §4.8's *"evaluated in `pump()`, after the
  plaintext is packed and before the seal"* — is not directly observable.
  A build gating before packing produces the same packet counts. What is
  observable is that the refused frames stay pending, which
  `the_gate_holds_the_backlog_until_the_flight_is_acknowledged` asserts.

### 3.5 What `Solo` and `Pair` cannot express

These are **core** fixtures with a hand-driven wire. Every loss in this
file is *declared* by handing the core an ACK that skips counters; the
wire never actually drops anything, never reorders and never duplicates.
So nothing here tests loss detection against a real reordering fabric —
that is `FlakyWire`'s job in 5b's `tests/story_reliability.rs`, and it is
worth stating that **no test in this file would fail if the core's
reordering behaviour were wrong**, only if its arithmetic were.

Two further gaps inherited rather than introduced:

- **`FlakyWire` cannot drop by content.** "Drop exactly the ACK" is
  inexpressible, so §7.4's asymmetric-loss reasoning for the PTO train —
  *"since the anchor is the receive clock, under asymmetric loss too"* —
  cannot be built from it in 5b either. Reported, not solved.
- **No socket faults.** Working rule 13's original finding is unchanged by
  slice 5: a fabric that models a network cannot express "this send
  fails".

### 3.6 PLAN-5 §6.4's item 2, dropped for want of a separating assertion

*"Credit for a fully-closed stream is a valid no-op"* (§8.4) on a stream
we can send on. `streams.rs`'s `on_max_stream_data` returns `Ok(())` for a
local index below `ever_opened` **whether or not** the watermark covers
it: the tombstone check short-circuits, and without it the `open` lookup
misses and returns `Ok(())` too. Both builds answer the same way, so no
assertion separates them — *an upper bound the collapsed implementation
satisfies for free is not a test* (working rule 9), and a test named for
the property would be one more of slice 2a's two. Items 1, 3 and 4 of
§6.4 are written.

### 3.7 Two things I did not write because they belong to the other author

§12.4's delayed-ACK policy and §12.2's derivation. `tests_ack.rs` owns
them. `admission::a_pure_ack_packet_is_never_tracked_in_flight` touches
§12.4 only to *reach* a pure-ACK packet; its assertion is §13.5's.

---

## 4. Contract concerns

Beyond conflicts C1–C5 in §2, three smaller things, each of the shape
working rule 8 hunts — **a stated construction with an unstated scope**.

1. **`AckOutcome::ack_events` has no stated order.** §2.2 says `acked` and
   `lost` are *"in ascending counter order"* and says nothing about
   `ack_events`. It happens not to matter — `NewReno::on_ack` is
   order-insensitive, since the recovery fence is evaluated per event —
   but that is a property of the one v1 controller, not of the seam, and
   §14.1 exists so there can be others. CUBIC's RTT-sampling and BBR's
   delivery-rate estimation both read acknowledgement order. One clause.

2. **§2.2's three `#[cfg(test)]` accessors are a list, and lists are read
   as exhaustive.** Ruling 71 is the precedent. They are enough for these
   tests only because `bytes_in_flight` and `congestion_window` happen to
   expose the two quantities the gate is made of; `pto_count` and the
   sent map are not reachable, which is why §3.4's gaps exist. If more are
   wanted, adding them later is cheap — but the gap should be a decision,
   not a discovery.

3. **`Recovery::on_ack`'s "no-op" scope.** §2.2 says it returns
   `AckOutcome::default()` when nothing is newly acknowledged. It does not
   say whether `largest_acked` and `loss_time` are nevertheless updated.
   Both readings are defensible; the tests assert the strict one (nothing
   moves, `bytes_in_flight` included), which is what *"a total no-op"*
   says. Worth one sentence.

### On the brief's instruction about ruling 137

The brief says *"Assert it is 0"*. I have written that assertion, and it
is honest about being a construction-site pin rather than a behavioural
one (§3.4). The reason it cannot be more is that `CONTRACT-5a.md` §2.2's
`SentPacket` **does not have the field at all** (conflict C1) — so the
instruction and the contract disagree, and the ruling is the one that
binds. Flagged rather than silently resolved, per working rule 3.

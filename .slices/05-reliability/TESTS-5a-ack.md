# TESTS-5a-ack — §12 (ACK) test inventory

Blind test author for §12, slice 5a. Worktree cut from `22a9a2a`.
Owned paths: `src/core/connection/tests_ack.rs` and this file. Nothing else
was written.

**48 tests**, in seven modules. Written from `SPEC.md` §12 (with §7.2,
§8.3/§8.4, §8.5, §16.5), `CONTRACT-5a.md` and `PLAN-5.md` §7/§8/§11 alone.
No implementation of `ack.rs`, `recovery.rs` or `congestion.rs` was read —
none exists in this worktree.

`rustfmt --edition 2024 --check src/core/connection/tests_ack.rs` is clean.
**The file cannot compile here** — `super::ack` does not exist yet — and it
was deliberately not stubbed (a stub in `src/` is the slice-2a accident).

---

## 0. Spec notes — the source of truth for every pin below

### §12.1 range semantics (SPEC.md:3555–3569)
- first block covers `largest − first_range ..= largest`
- each `(gap, range)` pair: `block_largest = prev_smallest − gap − 2`, and
  the block covers `block_largest − range ..= block_largest`
- a block descending below counter zero is **structural failure** (§8.4)
- `ack_delay` is raw microseconds varint, **no exponent scaling**; no ECN

### §12.2 derivation (SPEC.md:3571–3587)
- derived from the replay-window snapshot (greatest + bitmap, §7.2) — the
  single received-packet record; **no second tracker**
- ranges emitted **newest-first, descending**
- truncate at `MAX_ACK_RANGES` pairs **or** packet capacity, whichever binds
- `MAX_ACK_RANGES` = **64** pairs; at most **65 blocks** including the first
- received `range_count` > 64 ⇒ structural failure of §8.2's class:
  **nothing from the packet applied**; CLOSE `PROTOCOL_VIOLATION`

### §12.3 `ack_delay` (SPEC.md:3589–3595)
- measured from arrival of the packet bearing `largest` to emission
- when the window's largest was **not frame-seen** (a keepalive, §7.5),
  `ack_delay = 0`
- the estimator's cap at `MAX_ACK_DELAY` is §13.1's, applied on receipt

### §12.4 delayed-ACK policy (SPEC.md:3597–3622)
- `MAX_ACK_DELAY` = **25 ms**
- owed after every **2nd** ack-eliciting packet, **or** when `AckDelay`
  fires (armed at `MAX_ACK_DELAY` on the **first unacknowledged**
  ack-eliciting packet) — whichever first
- owed **immediately** on out-of-order arrival: an **ack-eliciting** packet
  whose counter is not exactly one greater than the previous greatest. The
  first ack-eliciting packet of a session has no previous greatest, so the
  rule applies vacuously and yields an immediate ACK
- an owed ACK **rides the next outgoing packet** (§8.5); if none is pending
  a standalone ACK packet is generated. Pure ACKs: `seal_quiet` (§7.4),
  **not ack-eliciting**, **never tracked for loss**, **bypass cwnd** (§14.5)

### §12.5 processing (SPEC.md:3624–3640)
- bounded intersecting processing against the in-flight set; never
  materialised
- `largest` **exceeding** the highest counter sealed ⇒ **ignored whole**,
  no-op + trace; **the packet's other frames still apply** (§8.2's split).
  Deliberate divergence from RFC 9000 §13.1
- newly acked packets clear their frames and feed §13.2/§14.2. **Duplicate
  acknowledgment of a counter is a no-op**

### Supporting
- §8.3/§8.4 (SPEC.md:2642, 2666–2677): ACK is `0x02`, **not ack-eliciting**,
  retransmission class `never`; wire layout; the two structural errors
- §8.5 (SPEC.md:2829): *"this order: the ACK first (if owed), then control
  frames…"* — load-bearing for finding **U1** below
- §16.5 (SPEC.md:5011–5054): `AckDelay` is a named timer; `handle_timeout`
  is idempotent; `AckDelay` fires **after** the loss/PTO evaluation at the
  same instant
- ruling 138 (SPEC.md:3652–3662): §13.1's second sample clause is vacuous
  and MUST NOT be implemented. **Not touched here** — §13 is the other
  blind author's file. No test in `tests_ack.rs` asserts anything about it.

---

## 1. Inventory

Every row's third column is what the **broken** build does. Where a row
says "not asserted", that is a deliberate refusal: an assertion a
conforming build can fail is a flake, not a pin.

### `mod range_semantics` — §12.1 (6 tests)

| test | pins | broken build |
|---|---|---|
| `pairs_encode_the_spec_arithmetic_not_the_missing_counter_count` | the `(gap, range)` literals `[(1,1),(4,2)]` for a hand-computed window, **and** their decode | `gap = prev_smallest − block_largest − 1` (the "count of missing counters" reading) emits `[(2,1),(5,2)]`, which decodes to `94..=97` / `85..=87` — every block shifted, cumulatively. A "three blocks" assertion passes it |
| `derived_ranges_equal_the_windows_ranges_when_nothing_truncates` | derived blocks **equal** the window's own `ranges_desc`, and the flattened counters equal what was marked | a build emitting `range` where `gap` belongs produces a wire-legal frame acknowledging counters never received; a build walking oldest-first gets `largest` wrong |
| `one_counter_is_one_block_and_no_pairs` | one marked counter ⇒ zero pairs | a `for i in 0..=n` loop that always emits one pair — which at counter 0 must descend below zero |
| `a_block_reaching_counter_zero_is_legal_and_one_below_is_structural` | **both sides** of §12.1's third bullet on the *first* block: `5 − 5 = 0` legal, `5 − 6` structural, each paired with a STREAM frame so "nothing applies" is observable | `checked_sub(x).filter(\|v\| *v > 0)` — treating "reaches zero" as "descends below zero" — kills a connection on a wire-legal ACK. **This is the untested side**; `tests.rs` already holds the illegal one |
| `a_pair_descending_to_exactly_zero_is_legal_and_one_below_is_structural` | the same boundary on the `− gap − 2` descent, which is different code | an underflow check applied only to `first_range` (the only subtraction §12.1's first bullet spells out) |
| `ack_delay_is_carried_as_raw_microseconds` | `12 345` survives verbatim | an `ack_delay_exponent` (÷8 ⇒ `1543`), milliseconds (÷1000 ⇒ `12`), or rounding (⇒ `12000`). The value is chosen so **no** scaling passes |

### `mod derivation` — §12.2 (7 tests)

| test | pins | broken build |
|---|---|---|
| `an_empty_window_derives_no_ack` | `None` iff no greatest | defaulting `greatest` to 0 acknowledges a packet that never arrived and retires it from the peer's recovery |
| `the_alternating_worst_case_truncates_at_sixty_four_pairs_newest_first` | **A-4.** Over the full 2048-bit alternating window (1024 blocks available, asserted): exactly 64 pairs / 65 blocks; `largest == window.greatest()`; the blocks are a **prefix** of the window's newest-first walk | truncating from the *newest* end (the shape you get from `collect()` + `truncate()` after a reverse) still emits 64 pairs and satisfies any `≤ 64` bound — `largest` is what fails it, ~2000 counters stale, which is how a sender concludes its newest flight was lost. The `≤ 64` bound alone is passed by an oldest-first build; the `largest` assertion alone is passed by an uncapped build that then overruns the packet |
| `a_small_window_is_not_padded_to_the_cap` | three blocks ⇒ two pairs | a build emitting 64 pairs unconditionally, zero-filled: wire-legal, and it acknowledges 128 counters below the window that were never received |
| `room_truncates_from_the_old_end_and_the_result_always_fits` | **both sides** of the capacity limb at a boundary computed from the build's own output: at `room == len` the full frame; at `room == len − 1` strictly fewer pairs, still `≤ room`, `largest` still the greatest | a build ignoring `room` returns the same frame at `len − 1`; §8.6's budget then refuses it at seal time, and §7.9 makes a failed seal terminal. **Not asserted: how many pairs survive** — §12.2 fixes only newest-first and fits |
| `no_ack_is_derived_when_even_the_first_block_does_not_fit` | both sides of the *smallest viable* `room`, plus `room == 0` | a build checking `room` only inside the pair loop emits the first block unconditionally and overruns by exactly the bytes that matter most |
| `the_reported_delay_is_not_capped_at_max_ack_delay` | `40 000 µs` reported verbatim | clamping at derivation makes a late ACK indistinguishable from a punctual one and inflates the peer's RTT. §12.3 puts the cap on §13.1's *subtraction* |
| `the_derivation_never_reaches_past_the_windows_edge` | a counter §7.2 has forgotten is not acknowledged | a build keeping its own side-table of received counters — the "second tracker" §12.2 forbids — still holds it. Not a wire error; the fused design quietly not being the design |

### `mod ack_delay` — §12.3 (7 tests)

| test | pins | broken build |
|---|---|---|
| `the_delay_is_the_measured_microseconds_since_the_largest_arrived` | **equality** at `7 300 µs` | reporting `0` always. Every "the delay is small" / "at most `MAX_ACK_DELAY`" bound passes it — slice 2a's defect exactly |
| `the_delay_tracks_the_newest_packet_not_the_first_of_the_batch` | `5 000`, not `15 000` | `largest_at` stamped on the packet that *armed* `AckDelay` — the same field, written at the wrong moment. Over-reports by the batch's whole span |
| `a_gap_filling_arrival_does_not_move_the_delay_anchor` | `15 000`, not `5 000` | `largest_at = now` unconditionally — the one-liner. Under-reports whenever a straggler arrives late, which is when the sample is most sensitive. **The previous test cannot catch this**: there, both builds agree |
| `a_keepalive_bearing_the_largest_yields_zero_and_a_frame_does_not` | **both sides**, same instants, one flag flipped | "0 when not frame-seen" is free for a build reporting 0 always; the frame-seen half is the separator |
| `the_frame_seen_flag_follows_the_greatest_rather_than_latching` | frame → keepalive → frame, the answer going `real → 0 → real` | `largest_frame_seen \|= …` rather than `=`. Latched either way, half the reported delays are wrong and none is out of range |
| `the_delay_is_zero_before_anything_has_been_received` | §2.1's stated case | `largest_at.unwrap()`. Unreachable through the core, which is why nothing else would pin it |
| `a_backwards_instant_saturates_rather_than_panicking` | §2.1's *"saturating"* | `now - largest_at` panics in debug. Contract-stated, not spec-stated; flagged as such in the test's doc |

### `mod policy` — §12.4 as a state machine (9 tests)

| test | pins | broken build |
|---|---|---|
| `the_first_ack_eliciting_packet_of_a_session_is_acked_immediately` | `prev_greatest == None ⇒ Now` | `prev_greatest.map_or(false, …)` — the `None ⇒ in-order` reading, which is the *more* natural spelling and delays the session's first ACK by 25 ms. §12.4 says in terms that this seeds the peer's RTT early |
| `an_ack_is_owed_after_every_second_ack_eliciting_packet` | **A-1, both halves in one test**: 1st ⇒ `Arm`, 2nd ⇒ `Now`, on a primed session | an immediate-ACK-per-packet build (the policy §12.4 replaced) fails the first half; a delay-only build fails the second. Either half alone is passed by one of the two |
| `the_delay_timer_is_armed_at_exactly_twenty_five_milliseconds` | equality on the returned `Instant` | `MAX_ACK_DELAY / 2`, `K_GRANULARITY`, or arming from the session start. "An `AckDelay` deadline exists" separates none of them |
| `out_of_order_arrival_forces_an_immediate_ack_and_in_order_does_not` | **A-2.** All three shapes §12.4 names (opens / fills / sits inside a gap) **plus the negative** | a build with only the counter and the timer passes *every other* §12.4 test and simply never reacts to reordering — 25 ms of extra delay per gap. The negative is what stops the mirror bug of returning `Now` unconditionally |
| `a_non_eliciting_out_of_order_packet_owes_nothing` | §12.4's immediate rule is **scoped to ack-eliciting packets**, and such a packet does not advance `since_ack` | the out-of-order test applied to every window-fresh packet. Keepalives and the peer's pure ACKs arrive out of order constantly under loss, so the wrong scope is free reverse-path amplification |
| `non_eliciting_packets_never_reach_the_every_second_trigger` | six non-eliciting packets owe nothing | `since_ack += 1` written before the `ack_eliciting` test rather than after. ACKs every second keepalive — a 1:1 reverse-path cost on an idle link, exactly the traffic §12.4's ratification removed |
| `the_delay_timer_firing_makes_the_ack_owed` | `on_delay_expired` ⇒ owed | implemented as a no-op because "the caller packs an ACK anyway" — true only while something else is owed, false in the case the timer exists for |
| `packing_an_ack_resets_the_every_second_counter_not_only_the_debt` | the reset, observed through its **consequence** (the next in-order packet must `Arm`, not `Now`) | clearing `owed` and forgetting `since_ack`. Parity is off by one forever after, half the ACKs become immediate, and the policy silently degrades toward the one it replaced while every "an ACK arrives" test stays green |
| `an_unpacked_debt_survives_further_arrivals` | `is_owed()` stays true across arrivals. **Deliberately weak** — see finding **U2** | `owed` recomputed as `since_ack % 2 == 0` per receive rather than latched: the third packet clears a debt nobody paid and the ACK is never sent, which under loss is an indefinite stall |

### `mod policy_on_the_wire` — §12.4 through the core (8 tests)

| test | pins | broken build |
|---|---|---|
| `one_packet_arms_the_timer_and_the_second_emits_the_ack` | **A-1 wired**: the emitted packet count *and* `conn.timer(TimerKind::AckDelay)` at each step, plus the ACK's contents | a core computing the right `AckAction` and never arming the timer, or arming a different one. The timer accessor is what separates "the policy is right" from "the policy is connected" *[Superseded 2026/08/18 by ruling 271: the second arrival now arms `AckDelay` at `now` rather than emitting, and the test is `one_packet_arms_the_timer_and_the_second_makes_it_due_now`; the acceptance this row argues is the pre-271 emission point.]* |
| `the_delayed_ack_fires_at_twenty_five_milliseconds_and_not_before` | **both sides**: nothing at `+24 ms` (and the timer still armed), the ACK at `+25 ms` (and the timer disarmed) | a build arming at a fraction fires early; a build never arming fires never. Asserting only "an ACK arrives by +25 ms" is satisfied by a build firing at +1 ms — slice 1's one-sided boundary in §12 costume |
| `a_repeated_timeout_at_the_same_instant_emits_no_second_ack` | §16.5's idempotence | leaving `AckDelay` armed after firing produces a standalone ACK per shell tick, forever. A reverse-path flood no completion test notices |
| `a_gap_in_the_counter_space_is_acked_in_the_same_drain` | **A-2 wired**: counters 1–2 sealed and discarded, counter 3 ACKed in the same drain, and the ACK reports `[3..=3, 0..=0]` | a counter+timer-only build emits **nothing** here. The ranges assertion separately catches a build that reacts to the gap but reports a contiguous `0..=3`, acknowledging two packets that never arrived |
| `a_replayed_packet_does_not_advance_the_every_second_counter` | **A-9.** The replay is delivered exactly where a *second* fresh packet would tip the rule, so a folding build emits an ACK the correct build does not; the armed deadline is asserted untouched; a genuinely fresh 2nd still works | `on_recv` called before the window's check-and-mark, or on its `false` branch. One captured datagram then buys one ACK per copy — free amplification off a packet the attacker cannot read — and every other §12 test passes |
| `a_pure_ack_packet_is_quiet_unelicited_and_untracked` | **A-7.** All four properties §12.4's last bullet states, each with its own observable: frames == `[ACK]`; `liveness.last_send()` unchanged (`seal_quiet`); `liveness.is_armed()` still false (not ack-eliciting); `bytes_in_flight() == 0` | packing a PING "to make it useful"; plain `seal` moving §7.5's marking clock; arming §7.4's death deadline off a packet carrying no obligation; tracking it. **The delivery is at `t + 5 ms`, not `t`** — at `t` the `last_send` assertion would pass against a `seal` build for free, because install already pinned it to `t` |
| `a_hundred_pure_acks_leave_nothing_in_flight` | the same, sustained | one ~30-byte packet is easy to overlook; a hundred is 3 KB of a 12 000-byte window, and the self-gating failure §12.4's bullet prevents takes minutes of real traffic to show. Here it shows at once |
| `an_owed_ack_rides_a_pending_data_packet_rather_than_going_alone` | §12.4's **first** clause: exactly one ACK on the wire, and the packet carrying it also carries STREAM data | a build that always generates a standalone pure-ACK packet is *correct on the wire* and costs a whole datagram per ACK on a saturated path. "An ACK was sent" passes it. **This is the one §12 test that needs §14** — see finding **U3** |

### `mod processing` — §12.5 (10 tests)

| test | pins | broken build |
|---|---|---|
| `an_ack_above_the_highest_sealed_counter_is_ignored_whole` | **A-6**, three-way: alive (not `PROTOCOL_VIOLATION`); `bytes_in_flight` **unchanged**; the co-resident STREAM frame's data readable to EOF. The forged ACK's range deliberately **covers the real flight** (`0..=1 000 000`) | RFC 9000 §13.1's `PROTOCOL_VIOLATION` reading kills the connection; an applying build drops the flight to 0; a discard-the-packet build loses the STREAM. **A forged ACK with `first_range = 0` — the obvious version — separates none of them**, because an applying build would also have nothing to acknowledge |
| `largest_equal_to_the_highest_sealed_counter_is_processed` | **both sides** of "*exceeds*": equality processed, one above ignored | `>=` passes every ignore-whole test and silently discards the ACK for the newest packet in flight forever — the packet the RTT sample and §13.2's `largest_acked` both depend on |
| `a_duplicate_acknowledgment_changes_nothing` | the same ACK four times: alive, `bytes_in_flight` stays 0 | subtracting `size` per acknowledged **counter** rather than per removed **entry** underflows a `u64` — a debug panic, or a release `bytes_in_flight` near `u64::MAX` that closes the window permanently. Not exotic: §12.2 re-acknowledges the whole window on every ACK, so *most* of a normal ACK is duplicate |
| `sixty_four_ranges_apply_and_sixty_five_take_the_whole_packet_down` | **A-5, both sides.** The two packets differ in **exactly one pair** and carry the same STREAM frame, so the STREAM's fate is attributable to the count alone | `>=` rejects 64 and kills a conforming peer; a missing check accepts 65 and — worse — applies the packet. §8.2's "nothing is applied" is the half a parse-then-continue build gets wrong *even when it rejects* |
| `a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight` | **A-8, as far as §12.5 permits.** The counter space is first pushed past 130 by 400 delivered PINGs (~200 pure ACKs, one counter each, cwnd-exempt); then a legal 64-pair ACK whose first block is the flight and whose 64 pairs march down through untracked pure-ACK counters | an off-by-one in the merge walk: `bytes_in_flight == 0` requires every flight counter to be found under the *first* block while 64 further blocks pass unmatched entries. **It does not catch a materialising build** — see finding **C2** |
| `an_ack_naming_only_untracked_counters_retires_nothing` | the intersection is against the **in-flight set** | removing entries by *range position* rather than by key (popping the first `n`) retires real packets on an ACK naming none of them, and the sender never retransmits data the peer never got |
| `a_received_ack_elicits_no_ack_in_reply` | four received ACKs: no transmit, no timer armed | `is_ack_eliciting` read as "the packet carried frames" rather than §8.3's table. Two peers answering each other's ACKs saturate the link until one dies, and it is invisible to any test asserting an ACK *arrives* |
| `a_padding_only_packet_elicits_no_ack` | PADDING owes nothing **and does not advance `since_ack`** (proved by the next packet only arming), yet its counter appears in the eventual ACK | `ack_eliciting` derived from `!plaintext.is_empty()` — i.e. "not a keepalive, so it counts". Easy to conflate, since §12.3 already keys `frame_seen` off emptiness. §8.4 lets a peer send any number of PADDING bytes anywhere |
| `a_keepalive_elicits_no_ack_but_is_still_acknowledged` | §3.4's empty plaintext owes nothing, and its counter is still in the window | a build skipping the window mark for keepalives makes the peer retransmit them forever |
| `the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest` | **§12.3 through the core**, three scenarios: `0` when the largest arrives with the ACK; `25 000` off the timer; `0` when a keepalive took the greatest | a core passing `frame_seen = true` unconditionally into `on_recv`. **The unit tests in `mod ack_delay` cannot see this** — they pass the flag the test chose. The `25 000` case is what stops a report-0-always build |

### `mod two_cores` — end to end (1 test)

| test | pins | broken build |
|---|---|---|
| `two_cores_acknowledge_each_others_data_and_the_flight_empties` | A's flight is non-empty before and **exactly 0** after; B's own ACKs were never tracked; `Pair::pump` terminates | the pre-condition fails a build tracking nothing, the post-condition a build never retiring; an ACK-of-ACK build never goes quiet and fails as a named panic. **The second pump at `t + MAX_ACK_DELAY` is not decoration** — whether the last packet needs the timer depends on how §8.6 packs 4 KiB, so a single same-instant pump would make this a test that passes on the frame-header size. For the same reason it does **not** assert how many ACKs B sent |

---

## 2. Conflicts and unstated scopes — reported, not resolved

### C1 — constant name: the brief says `ACK_MAX_RANGES`; everything else says `MAX_ACK_RANGES`
SPEC.md:3577, 3583, 2675, `CONTRACT-5a.md` §2.1 and `constants.rs:341` all
write **`MAX_ACK_RANGES`**. My brief writes `ACK_MAX_RANGES` twice. Spec and
code agree and already exist, so the tests use `MAX_ACK_RANGES`. Cosmetic,
recorded because a brief is an input.

### C2 — PLAN-5 §7's A-8 names a mutation its own assertion cannot separate, and the scenario is unconstructible
**Two separate problems, both worth a ruling.**

*The assertion.* A-8 says: *"a wire-legal ACK with 64 ranges spanning 2⁴⁰
counters returns in bounded time … a materialising build allocates and
either OOMs or takes minutes. **Assert the resulting `bytes_in_flight`
delta, not the wall time**."* A materialising build and a bounded-
intersecting build compute the **same** delta — expansion is a cost
property, not a correctness property. The stated assertion therefore does
not separate the stated builds. Only time or memory does, and the row
forbids asserting time. This is working rule 9 in reverse: the row is named
for a property its assertion does not pin.

*The scenario.* §12.5's own ignore-whole rule caps `largest` at
`highest_sealed`, and §12.1 forbids descending below counter zero — so the
**total span of any ACK that is processed at all is ≤ `highest_sealed`**.
An ACK spanning 2⁴⁰ counters is necessarily one the receiver ignores whole,
and therefore never reaches the intersecting walk. Reaching a real span of
"millions" requires a session that has sealed millions of packets; hiss's
`set_counter_for_test` is internal to hiss (`session.rs:356–358`), so the
fixture cannot fabricate it. The deepest counter space the fixture reaches
cheaply is ~200 (400 delivered PINGs ⇒ ~200 cwnd-exempt pure ACKs), which is
what the test uses.

*What I wrote instead:* the maximal legal 64-pair ACK the fixture can build,
asserting the exact `bytes_in_flight` outcome. It pins the merge walk's
block↔entry association, which is a real defect class, and it does **not**
claim to pin §12.5's expansion bound. **The expansion bound is reported as
untestable at fixture scale (F1), not quietly marked done.**

### U1 — §12.2's capacity limb is dead in v1, and §8.5 is what killed it (working rule 8)
§12.2: *"truncating at `MAX_ACK_RANGES` pairs **or at packet capacity**,
whichever binds"*, and `CONTRACT-5a.md` §2.1 threads a `room` parameter
through `derive` for it. But §8.5 (SPEC.md:2829) packs **the ACK first**,
and `Packing::new()` starts with `budget = MAX_PLAINTEXT` and `used = 0`
(`frame.rs:758–763`, `room()` at `frame.rs:789–791`). So `room` at the
moment `derive` is called is **always 1170**.

The largest ACK §12 can produce is bounded above by:
`1` (type) `+ 8` (`largest`) `+ 8` (`ack_delay`) `+ 1` (count = 64)
`+ 2` (`first_range` ≤ 2048) `+ 64 × 4` (gap and range are *differences*
inside one 2048-counter window, so ≤ 2 bytes each) = **≤ 276 bytes**.

276 < 1170 unconditionally. **`room` can never bind through the core;
`MAX_ACK_RANGES` always binds first.** The parameter is worth keeping as a
defensive contract and its behaviour is unit-tested here on both sides, but
the spec sentence describes a branch v1 cannot take.

*The argument's assumptions, stated per working rule 12:* (a) §8.5 packs
the ACK first — quoted above from SPEC.md:2829 and from `frame.rs:725`'s
`Stage::Ack` being stage 1; (b) `Packing::room()` at that stage is the full
`MAX_PLAINTEXT` budget — read at `frame.rs:758–763`; (c) gaps and ranges are
within-window differences, hence ≤ 2048, hence ≤ 2-byte varints — this
follows from §7.2's window being the only record (§12.2's fusion). If any
of the three changes, the limb becomes live again.

*Second-order note, which is why this is worth a ruling rather than a
deletion:* the same two rules together mean **nothing bounds the ACK's
share of a packet**. In the alternating worst case the ACK takes ~276 of
1170 bytes — 24 % of every packet — off the front of the data it is
supposed to ride (§12.4's first clause). Bounded, but not by anything §12
says.

### U2 — §12.4 does not say what a *third* ack-eliciting packet returns while the debt is unpaid
`AckState::on_recv` is total, so it must answer for the 3rd, 4th, … packet
after the 2nd made an ACK owed and before `on_ack_packed`. Both readings
are defensible and observably different:

- `AckAction::Now` — the contract's own doc for the variant is *"An ACK is
  owed now"*, and it still is;
- `AckAction::None` — nothing *new* is owed, and `since_ack = 3` is not a
  multiple of 2.

The state is reachable whenever `derive` returns `None` for room (§2.1:
*"the ACK **stays owed** for the next packet"*) or the caller defers.
**Not tested.** What I pinned instead is the invariant that survives both:
`is_owed()` stays true (`an_unpacked_debt_survives_further_arrivals`).

Closely related and equally unstated: what `since_ack` counts while a debt
is outstanding and unpacked. §12.4 arms on *"the first **unacknowledged**
ack-eliciting packet"*, and §2.1 resets `since_ack` only in
`on_ack_packed` — so an owed-but-unpacked ACK leaves the counter running
across a boundary it has already reported. Harmless under either reading;
recorded as the same shape.

### U3 — §12.4's "rides the next outgoing packet" is unobservable without §14
Under ruling 134 the congestion window is the **only** thing that leaves
data pending across a pump: flow control refuses at `write()` (returning
`Ok(0)`, slice 4's rule), and a mutating call flushes everything the ledger
admits (ruling 114). So no packet exists for an owed ACK to ride until
§14's admission gate defers a seal.

Consequence: `an_owed_ack_rides_a_pending_data_packet_rather_than_going_alone`
depends on the other blind author's module, and a §12-only green does not
cover §12.4's first clause. This is `PLAN-5.md` §1's *"one feedback loop
through one call site"* seen from the test side, and it is the reason
"do NOT cut §12 from §13/§14" is right. Recorded so that if the test fails
during integration, the first question is which of the two rules it
disagrees with.

### C3 — **`testfix::parse_frames` panics on ACK, and 37 call sites in `tests_streams.rs` decode drains that will now contain one** (rule 15's residue)
`testfix.rs:184–188` panics on any frame type slice 4 could not emit:

> `slice 4 emitted frame type {other:#x}, which §8.3 does not place in this slice`

`FRAME_ACK` (`0x02`) is not in its match. Slice 5 makes that emission
correct: §12.4's vacuous out-of-order rule means the **first** ack-eliciting
packet a `Solo` peer sends is answered immediately, so any
`Solo::drain_frames` / `Solo::packets` over a drain that followed a legal
`deliver` now panics.

`tests_streams.rs` has **37** such call sites. A concrete witness:
`tests_streams.rs:2312–2318` delivers stream bytes, reads, then
`handle_timeout(t1)` and `s.packets(&d)` — by then `AckDelay` has fired and
the drain carries the delayed ACK. (Sites whose delivery is a §8.4
*violation* are safe: nothing applies, so nothing is owed. That is why some
of the 37 will pass and the failure will look arbitrary.)

**Neither blind author may fix this**: `testfix.rs` and `tests_streams.rs`
belong to slice 4a's author, `mod.rs` to the implementer, and this file to
me. It is exactly working rule 15's shape — a file whose contents are only
valid once both tracks exist — with one aggravation over the `Cargo.toml`
case: **the tree compiles.** Nothing fails until the tests run, and when
they do the message reads as a slice-boundary violation rather than as a
fixture that has aged out. **It is the integrator's, and the fix is one
`FRAME_ACK` arm in `parse_frames` plus an `AckFields`-shaped `Wire`
variant.** `tests_ack.rs` carries its own decoder precisely so it does not
depend on that fix landing first.

### C5 — `CONTRACT-5a.md` §2.1's `derive` doc contradicts itself about `None`, in adjacent paragraphs (working rule 8, inside the contract)
Verbatim, two paragraphs apart in the same doc comment:

> Returns `None` **iff** the window has no greatest (nothing received yet)
> — in which case no ACK can be owed either.

> … if even the first block does not fit, this returns `None` and the ACK
> **stays owed** for the next packet.

`iff` is exclusive; the second sentence names a second `None` case, and it
is one in which an ACK **is** owed — the exact opposite of the parenthetical
the first sentence attaches. Both behaviours are individually right and both
are tested here (`an_empty_window_derives_no_ack` and
`no_ack_is_derived_when_even_the_first_block_does_not_fit`); what is wrong is
the `iff`, and the risk is an implementer reading the first sentence as
exhaustive — which is the defect class working rule 8 names, appearing this
time in the binding contract rather than in the spec.

**Suggested repair (not applied — the contract is binding):** *"Returns
`None` when the window has no greatest (nothing received yet, in which case
no ACK can be owed either) or when `room` cannot hold even the first
block."* Note that per finding **U1** the second case is unreachable through
v1's core, which is why the contradiction could ship without a caller ever
noticing.

### C4 — a §12.3 wording point, low value, recorded for completeness
§12.3 measures to *"the **emission** of the ACK"*. The core seals at an
exact instant; §16.5's `L` (250 ms) is a **shell** parameter and the core
cannot see it. So the value on the wire is measured to the *seal*, and the
peer's §13.1 subtraction (capped at 25 ms) never accounts for up to 250 ms
of shell lateness. Correct as specified — the cores expose exact deadlines
by design — but the two numbers are an order of magnitude apart and nothing
in §12.3 or §16.5 notes the interaction. No test; a core-level fixture
cannot observe `L` (PLAN-5 §11-F4).

---

## 3. Not testable, and why (working rule 13)

**F1 — §12.5's unbounded-expansion hazard is unreachable at fixture
scale.** Full argument in **C2**. The span of any processed ACK is bounded
by `highest_sealed`; the fixture reaches ~200. No assertion available to
this file separates a materialising build from a bounded one, and the
`bytes_in_flight` delta PLAN-5 proposes separates neither. Left uncovered
and reported, rather than covered by a test named for a property it does
not pin.

**F2 — §7.2's ratified ACK-fidelity failure mode.** As `PLAN-5.md` §11-F2
already records: 2048 counters of ACK-loss burst is a simulation, not a
test. Nothing here attempts it, and Appendix B's obligation is intact.

**F3 — `ack_delay`'s `u64` saturation on a huge elapsed time.**
`Duration::as_micros()` returns `u128`; overflowing `u64` µs needs ~584 000
years between the packet and the ACK. Unconstructible. The *other*
saturation the contract names — a `now` earlier than the anchor — is
constructible and **is** tested.

**F4 — the delay a peer actually observes.** See **C4**: the core cannot
see the shell's lateness bound, and the shell fixture cannot read the
`ack_delay` field. Neither fixture can do the other's job — PLAN-5 §11-F4's
split, hitting §12.3 as well as §13/§14.

**F5 — §12.2's capacity truncation through a real core.** See **U1**:
`room` can never bind at the ACK stage, so the capacity limb is exercised by
unit tests against `derive` only. This is a *spec* limit, not a fixture
limit — no fixture could reach it.

**F6 — `Solo`'s core is always the responder** (`PLAN-5.md` §11-F5).
Nothing in §12 is role-dependent, and the one two-core test drives the
initiator side as a sender, so this is believed not to bite. Recorded
because it bit slice 4a.

**F7 — "the ACK was lost" as a content-selected drop.** `FlakyWire` cannot
drop by content (`PLAN-5.md` §11-F1). Not needed here: every §12 test that
would want it is expressible against `Solo`/`Pair`, where a datagram is a
held `Vec<u8>` the test chooses to deliver or not — which is how
`a_replayed_packet_does_not_advance_the_every_second_counter` and
`a_gap_in_the_counter_space_is_acked_in_the_same_drain` are built.

---

## 4. What this file assumes of the implementer

Stated so a compile failure is diagnosable as a contract disagreement
rather than a guess:

- `super::ack::{AckState, AckAction, derive}` exactly as `CONTRACT-5a.md`
  §2.1 writes them, including `AckAction::Arm(Instant)` carrying the
  **deadline** (`now + MAX_ACK_DELAY`) and not the duration.
- `Connection::bytes_in_flight()` (`#[cfg(test)]`, §2.4).
- Already present and used as-is: `Connection::timer(TimerKind)`
  (`mod.rs:199`), `Connection::next_counter()` (`mod.rs:194`),
  `Connection::liveness()` (`mod.rs:185`), `Liveness::last_send()` /
  `is_armed()`, `ReplayWindow::{new, check_and_mark, greatest, would_accept,
  ranges_desc}`, `frame::Frame::{encode, encoded_len}`, `frame::Ack`'s four
  public fields.
- `mod ack;` and `#[cfg(test)] mod tests_ack;` declared in
  `connection/mod.rs` by the implementer, which **creates nothing** in this
  file's path (working rule 6).

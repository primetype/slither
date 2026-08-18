# Ruling 271 integration, part 2 — the twelve old-cadence pins rewritten

Base: `0f93dacb2fd7b19c4afcdb0f9e94128fcfcce143` (verified by `git rev-parse HEAD`, clean tree).

## Per-test table

_(old assertion -> new assertion, renames)_

## Rule-9 verification (revert-mutant matrix)

## C4 harness fix

## Conflicts


---

## Baseline, measured before any edit

`cargo test --all-features` at `0f93dac`: **764 passed, 12 failed**, and the
twelve are exactly the twelve named in the brief. Their panic sites, read
rather than assumed:

| test | panic |
|---|---|
| `smoke::the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer` | mod.rs:4245 `§12.4: an ACK is owed after every 2nd` — left `0`, right `1` |
| `smoke::an_ack_drains_the_sent_map_and_completes_the_send_half` | mod.rs:4177 `AckDelay` is `Some(now)`, not `Some(now + 25 ms)` — the two instants differ by exactly `MAX_ACK_DELAY` |
| `smoke::a_snapshot_settles_only_once_its_bytes_are_acknowledged` | mod.rs:4443 — the snapshot never settles: `exchange` goes quiet with the ACK still `pending` |
| `policy::an_ack_is_owed_after_every_second_ack_eliciting_packet` | 922 — `Arm(t)`, not `Now` |
| `policy::packing_an_ack_resets_the_every_second_counter_not_only_the_debt` | 1104 — `Arm(t)`, not `Now` |
| `policy::an_unpacked_debt_survives_further_arrivals` | 1137 — `is_owed()` false (it is `pending`) |
| `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_emits_the_ack` | 1191 — 0 ACKs, not 1 |
| `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter` | 1348 — 0 ACKs, not 1 |
| `processing::a_padding_only_packet_elicits_no_ack` | 1865 — 0 ACKs, not 1 |
| `processing::a_keepalive_elicits_no_ack_but_is_still_acknowledged` | 1900 — 0 ACKs, not 1 |
| `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest` | 1931 — 0 ACKs, not 1 (**stanza 1 only**; the two `MAX_ACK_DELAY` stanzas were already green) |
| `processing::a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight` | 1739 — `floor` is `13`, not `> 130`: the fixture burned counters *by provoking one pure ACK per two pings* |

That last one is worth its own line. It is not an assertion about the cadence
at all — it is a **fixture** that spent 400 pings to buy ~200 sealed counters,
and under coalescing 400 pings buy 13 (the first immediate ACK plus one per
`ACK_COALESCE_MAX = 32`). The valve is directly visible in that number.

## Production reading this rewrite is written against

Read from `ack.rs` and `mod.rs` at the base commit, not from the ruling text:

* `on_recv`: `!in_order` → `owed = true`, `Now` (**first**, untouched).
  Then `since_ack >= ACK_ELICITING_PER_ACK` → `since_ack >= ACK_COALESCE_MAX`
  → `owed = true`, `Now`; else `pending = true`, `Arm(now)`. Else
  `Arm(now + MAX_ACK_DELAY)`.
* `fold_ack_policy`'s `Arm(at)` arm **min-clamps** against the armed
  deadline, so an arm never pushes an existing one later. A first packet
  arming at `now + 25 ms` followed by a second arming at `now` leaves the
  deadline at `now`.
* `pump_packets` breaks *before sealing* when
  `ack_packed && !is_owed() && frames().len() == 1` — and `on_ack_packed()`
  runs at mod.rs:2694, **after** the commit, so `is_owed()` at the break
  still reads the pre-pack state. A fired `AckDelay` therefore escapes it.
* `pack_ack` asks `is_ready()` = `owed || pending`.

Consequence for every rewrite below: **the observable that used to be "an
ACK datagram left" is now split in two** — an armed deadline at `now`
(the state), and one ACK out of `handle_timeout(now)` + drain (the
emission). Both halves are asserted; dropping either leaves a test a
degenerate build passes.

---

## Per-test table

### `src/core/connection/tests_ack.rs` — `mod policy` (unit, `AckState` directly)

**1. `an_ack_is_owed_after_every_second_ack_eliciting_packet`** — name kept.

* Property (unchanged): *one is not two, and two is not one* — the every-2nd
  trigger exists and is distinguishable from the first-packet arm.
* Old: 1st → `Arm(t + MAX_ACK_DELAY)` and `!is_owed()`; 2nd → `Now` and
  `is_owed()`.
* New: 1st → `Arm(t + MAX_ACK_DELAY)` and **`!is_ready()`**; 2nd →
  **`Arm(t)`** — `now` itself, already due — with **`is_ready()`** and
  **`!is_owed()`**.
* Separating half added: `is_ready()` **and** `is_owed()` are both read on
  the 2nd. A build that collapses `pending` into `owed` — the mutation
  `ack.rs` says restores the pre-271 cadence "exactly" — passes the first
  and fails the second, and no assertion on the returned `AckAction` sees
  it.

**2. `packing_an_ack_resets_the_every_second_counter_not_only_the_debt`** —
name kept (it never encoded the emission point).

* Property (unchanged): `on_ack_packed` resets `since_ack`, asserted through
  its consequence, because `since_ack` is private.
* Old: 2nd → `Now`; after packing `!is_owed()`; next in-order →
  `Arm(t + MAX_ACK_DELAY)`.
* New: 2nd → `Arm(t)`; after packing `!is_owed()` **and `!is_ready()`**;
  next in-order → `Arm(t + MAX_ACK_DELAY)` (unchanged).
* Separating half added: `!is_ready()` pins ruling 271's *"all three, not
  two"* — a `pending` left set here makes every future packet carry a
  redundant ACK and measures `ACK_COALESCE_MAX` from the wrong origin.

**3. `an_unpacked_debt_survives_further_arrivals`** — name kept.

* Property (unchanged): the debt does not evaporate across further arrivals;
  deliberately weak on what `on_recv` *returns* for the 3rd, which §12.4
  still does not state.
* Old: `is_owed()` after the 2nd and through counters 3..=5.
* New: `is_ready()` after the 2nd and through counters 3..=5, plus
  `!is_owed()` on the 2nd. The loop deliberately stays far below
  `ACK_COALESCE_MAX = 32`: that is the coalescing window where the debt is
  *held*; at the valve it is discharged by design, and walking into it would
  assert the flush rather than the survival.

### `src/core/connection/tests_ack.rs` — `mod policy_on_the_wire` (through the core)

**4. `one_packet_arms_the_timer_and_the_second_emits_the_ack`
→ `one_packet_arms_the_timer_and_the_second_makes_it_due_now`** — **renamed**;
the second clause of the old name is exactly what ruling 271 reversed.

* Property (unchanged): the policy is *connected* — the core arms the right
  timer at the right instant, not merely computes the right `AckAction`.
* Old: 1st → nothing sent, `AckDelay = Some(t1 + MAX_ACK_DELAY)`; 2nd → one
  ACK with `largest = 2`, counters `[2,1,0]`, timer `None`.
* New: 1st → unchanged; 2nd → **nothing sent** and `AckDelay = Some(t1)`;
  then `handle_timeout(t1)` + drain → **exactly one** ACK, `largest = 2`,
  counters `[2,1,0]`, timer `None`.
* Separating halves added: (a) empty transmits after the 2nd fails any build
  that kept the pre-271 emission point; (b) `Some(t1)` vs
  `Some(t1 + MAX_ACK_DELAY)` is now the *only* observable difference between
  the two arrivals, since neither transmits; (c) the final `None` fails a
  build that emits the coalesced ACK and leaves `AckDelay` armed — a
  reverse-path flood now reachable from an ordinary burst rather than only
  from the 25 ms timer.

**5. `a_replayed_packet_does_not_advance_the_every_second_counter`** — name
kept.

* Property (unchanged): §7.2 — a replayed datagram advances nothing, so it
  buys the attacker no ACK.
* Old separator: the replay is delivered where a 2nd would tip the rule, so a
  folding build *"emits an ACK the correct build does not"*. **That
  observable is gone** — post-271 the 2nd emits nothing either, and a replay
  folded into the counter would have been invisible on the wire.
* New separator: **the armed deadline** — `t + MAX_ACK_DELAY` if the replay
  was dropped, `t` if it was counted. That assertion was already in the test,
  one line below the one doing the work; it is now the pin. Added: the 1st's
  own `t + MAX_ACK_DELAY`, and a `drain_boundary` after the replay asserting
  **nothing** flushes — i.e. the replay bought no ACK at all, which is the
  amplification claim stated directly. The closing "a fresh 2nd still works"
  becomes arm-at-`t` + drain-boundary → one ACK.

### `src/core/connection/tests_ack.rs` — `mod processing`

**6. `a_padding_only_packet_elicits_no_ack`** — name kept.

* Property (unchanged): PADDING is not ack-eliciting and does not advance
  `since_ack`; the window still records its counter.
* **This one had to be strengthened, not translated.** The middle assertion
  read *"this is the 1st and not the 2nd"* off `transmits().is_empty()` —
  which the 2nd now satisfies too, so as a literal translation the test would
  have passed a build that let PADDING advance `since_ack`. It now asserts
  `AckDelay == Some(t + MAX_ACK_DELAY)` there: a counting build puts a due
  deadline where a 25 ms one belongs.
* The ACK covering `[2,1,0]` is collected at the drain boundary.

**7. `a_keepalive_elicits_no_ack_but_is_still_acknowledged`** — name kept.

* Property (unchanged): §3.4's empty plaintext elicits no ACK, and its
  counter is still marked and still acknowledged.
* Same strengthening as (6): the 1st/2nd distinction moves onto the armed
  deadline (`Some(t + MAX_ACK_DELAY)` then `Some(t)`), and `AckDelay == None`
  after the keepalive is asserted explicitly. ACK collected at the boundary.

**8. `the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest`** — name
kept.

* Property (unchanged): §12.3's `frame_seen` reaches the wire — a
  frame-bearing greatest yields a real delay, a keepalive greatest yields 0.
* **Only stanza 1 moved**, and only in how the ACK is collected: the 2nd
  packet at `t + gap` arms at `t + gap` instead of emitting, so the ACK comes
  from `drain_boundary(t + gap)`. The emission instant — the thing the stanza
  is about — is identical, so `ack_delay == 0` is unchanged.
* Separating half added: `AckDelay == Some(t + gap)` pins the **min-clamp
  direction** in `fold_ack_policy` — the 2nd's arm at `t + gap` must *win*
  over the 1st's `t + 25 ms`.
* Stanzas 2 and 3 (`handle_timeout(t + MAX_ACK_DELAY)`) were green through
  ruling 271 and are untouched.

**9. `a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight`** —
name kept. **Fixture repair, not an assertion change.**

* Property (unchanged): §12.5's per-block walk — the first block retires the
  whole flight while 64 further blocks march past unmatched entries.
* Nothing in this test asserts anything about the cadence. What broke is that
  it *bought* its counters at the cadence's exchange rate: 400 pings, each
  answered by a pure ACK, used to seal ~200 counters. Under coalescing they
  seal **13** (the first immediate ACK plus one per `ACK_COALESCE_MAX = 32`),
  and the `floor > 130` precondition fails on the fixture rather than on the
  behaviour.
* Repair: `drain_boundary(&mut s, t)` after each ping — the driver's own
  boundary — restoring one sealed ACK per two pings.

### `src/core/connection/mod.rs` — `mod smoke`

**10. `an_ack_drains_the_sent_map_and_completes_the_send_half`** — name kept.

* Property (unchanged): the whole §12→§13→§9.7 feedback loop, plus *"the
  delayed-ACK policy being real rather than nominal"*.
* Old: after `exchange`, `AckDelay == Some(now + MAX_ACK_DELAY)` — "the odd
  packet out waits on the timer"; then `handle_timeout(delayed)` + `exchange`
  at `delayed`.
* New: after `exchange`, `AckDelay == Some(now)` — the coalescing arm, the
  2nd packet of the flight having re-armed it at `now` and every later packet
  folding in; then `handle_timeout(now)` + `exchange` at `now`. The
  `bytes_in_flight() > 0` guard between them is unchanged and still does its
  job: until the deadline fires, those packets are in flight.
* The defect class is verbatim what it was — *a build that ACKed every packet
  has an empty timer here and settles a step early* — only the instant moved
  from `+25 ms` to `+0`.

**11. `the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer`
→ `the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now`**
— **renamed**; "draws the ack" is precisely what stopped being true.

* Property (unchanged): §12.4's two triggers are **distinguishable**.
* Old: 1st in-order → nothing sent, `AckDelay == Some(now + MAX_ACK_DELAY)`;
  2nd → one datagram, timer `None`.
* New: 1st → unchanged; 2nd → **nothing sent** and `AckDelay == Some(now)`;
  `handle_timeout(now)` + drain → exactly one datagram, timer `None`.
* Both timer assertions are load-bearing in a way they were not before:
  neither arrival transmits, so the armed **instant** is the only thing
  telling the two triggers apart. Dropping either would leave two
  indistinguishable arrivals — working rule 9's degenerate case reached by
  *deleting* an assertion rather than by weakening one. The trailing
  `handle_timeout` half stops a build that arms correctly and never emits,
  which is the one shape coalescing makes easy to write.

**12. `a_snapshot_settles_only_once_its_bytes_are_acknowledged`** — name kept.
**Fixture repair only.**

* Property (unchanged): §16.2's snapshot settles on acknowledgement, not on
  writing.
* Nothing here is about §12.4. `exchange` goes quiet with B's ACK still
  `pending`, so A is never told and the snapshot never settles — a §16.2
  assertion going red for a §12.4 reason, with no §12 vocabulary anywhere in
  the panic. The call becomes `settle`.

## C4 harness fix

The implementer's C4 is real and its diagnosis was right: `mod smoke`'s
`exchange` shuttles datagrams *"until neither core has anything left to
send"*, and post-271 a coalesced ACK is exactly the thing that has not been
built at that point. It sits `pending` behind an `AckDelay` armed at `now`
itself, which `shell::driver`'s `biased` `select!` fires the instant the
socket empties — a boundary the core does not name, because the driver's
loop *is* the receive drain.

Two additive `#[cfg(test)]` helpers, no production code touched:

* `smoke::settle(a, b, now)` — `exchange`, then fire any **already-due**
  `AckDelay` on either core, then `exchange` again. Only `AckDelay`, and only
  when `at <= now`: a future deadline belongs to the caller's clock, and
  firing one here would silently advance a §13 timer a caller is measuring.
* `tests_ack::drain_boundary(s, now)` — `handle_timeout(now)` then drain, for
  the `Solo` fixture. `handle_timeout` runs only due deadlines (§16.5), so it
  is a no-op while the ACK is still riding `now + MAX_ACK_DELAY` — which is
  what lets the tests keep asserting *which* of §12.4's two arms was taken
  before flushing.

`an_ack_drains_the_sent_map_and_completes_the_send_half` deliberately does
**not** use `settle`: it has to observe the armed deadline between the two
exchanges, and a helper that flushes it would erase the assertion. It fires
the timer by hand, exactly as it did before with `delayed`.

This is working rule 13 in its mildest form — *the fixture bounds the
coverage* — arriving as a fixture that could not express a state the protocol
now requires. Note the failure mode: **three of the twelve reds were fixture
reds, not behaviour reds**, and two of them (`a_snapshot_settles…`,
`a_full_sixty_four_pair_ack…`) named neither §12 nor ACKs in their panic.

## Rule-9 verification (revert-mutant matrix)

Mutant: `ACK_COALESCE_MAX = 32 → 2` in `src/constants.rs` — the
implementer's own control, which makes `since_ack` reach both thresholds on
the same packet, renders the `pending` state unreachable, and restores the
pre-271 one-ACK-per-two-packets emission exactly. The `const _` assertion
`ACK_ELICITING_PER_ACK <= ACK_COALESCE_MAX` still holds at 2, so the mutant
compiles.

### The twelve rewrites — 10 RED, 2 GREEN

| test | mutant | separating assertion / why |
|---|---|---|
| `smoke::an_ack_drains_the_sent_map_and_completes_the_send_half` | **RED** | `AckDelay` is `+25 ms`, not `+0` |
| `smoke::the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now` | **RED** | *"the 2nd arms too — it does not emit"*: one datagram appears |
| `smoke::a_snapshot_settles_only_once_its_bytes_are_acknowledged` | GREEN | **legitimate.** §16.2, not §12.4. Under the mutant the ACK is emitted eagerly inside `exchange`, so the snapshot settles; `settle`'s extra boundary step is a no-op. The test asserts nothing about the cadence and must not. |
| `policy::an_ack_is_owed_after_every_second_ack_eliciting_packet` | **RED** | `Now`, not `Arm(t)` |
| `policy::packing_an_ack_resets_the_every_second_counter_not_only_the_debt` | **RED** | `Now`, not `Arm(t)` |
| `policy::an_unpacked_debt_survives_further_arrivals` | **RED** | *"ruling 271: due, and it builds no packet"* — `is_owed()` is true under the mutant. This is the **`pending`-vs-`owed` distinction** caught on its own, with no cadence assertion involved. |
| `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_makes_it_due_now` | **RED** | *"the 2nd arms, it does not emit"* |
| `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter` | **RED** | *"the 2nd arms rather than emitting"* — the closing fresh-2nd half. The replay half itself is cadence-independent and stays true under the mutant, as it should. |
| `processing::a_padding_only_packet_elicits_no_ack` | **RED** | at the drain boundary: 0 ACKs, not 1 — the mutant emitted it one step earlier. The *padding* half (`AckDelay == Some(t + MAX_ACK_DELAY)` after the 1st) passes under the mutant, which is correct: PADDING's non-eliciting-ness is cadence-independent. The red is on the **emission point**, which is what moved. |
| `processing::a_keepalive_elicits_no_ack_but_is_still_acknowledged` | **RED** | `AckDelay` is `None`, not `Some(t)`, after the 2nd |
| `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest` | **RED** | `AckDelay` is `None`, not `Some(t + gap)` — the min-clamp assertion |
| `processing::a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight` | GREEN | **legitimate.** §12.5's per-block walk. The `drain_boundary` in its counter-burning loop is a no-op under the mutant (the ACK already left), so it buys the same ~200 counters either way. The test asserts nothing about the cadence and must not. |

### The author's ten, same mutant — 2 RED, 8 GREEN

| test | mutant |
|---|---|
| `a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack` | **RED** — *"…owe at most one ACK emission, not 10"* |
| `twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack` | **RED** — *"…not 10"* |
| `a_lone_ack_eliciting_packet_is_acked_within_max_ack_delay` | GREEN |
| `the_tail_of_an_idle_burst_is_acked_within_max_ack_delay` | GREEN |
| `a_gap_mid_batch_is_acked_in_the_same_batch` | GREEN |
| `a_coalesced_ack_reports_every_range_a_per_packet_ack_would` | GREEN |
| `a_single_lost_packet_is_recovered_without_advancing_the_clock` | GREEN |
| `the_estimator_still_samples_across_a_coalesced_exchange` | GREEN |
| `a_coalesced_ack_moves_no_liveness_clock` | GREEN |
| `an_ack_only_emission_cannot_defer_death` | GREEN |

The eight greens are the author's own stated expectation — its module header
names five of them as *"expected GREEN at base, and must stay green"*,
because they exist to fail a cadence fix that *starves* ACKs rather than one
that restores them.

One finding worth the maintainer's attention:
`twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack`
carries the author's caveat *"if this passes at base it pins nothing"*. It
does not pass at base and it **does go red under the mutant** — so the
literal reading of "one receive-drain" is pinned, not merely documented, and
the §C1 conflict the author reported is a live one rather than a
documentation note.

Restored: `git checkout src/constants.rs`, `ACK_COALESCE_MAX` back to 32,
working tree carries only this task's three paths.

## Gates

```
$ cargo fmt --all --check
(no output, exit 0)

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a1f0c2ac075e6b39c)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.28s

$ cargo test --all-features        # 26 targets, zero failures anywhere
running 776 tests   test result: ok. 776 passed; 0 failed; 0 ignored
running 3 tests     test result: ok. 2 passed; 0 failed; 1 ignored
running 24 tests    test result: ok. 24 passed; 0 failed; 0 ignored
running 112 tests   test result: ok. 112 passed; 0 failed; 0 ignored
running 11 tests    test result: ok. 11 passed; 0 failed; 0 ignored
running 4 tests     test result: ok. 4 passed; 0 failed; 0 ignored
running 5 tests     test result: ok. 5 passed; 0 failed; 0 ignored
running 12 tests    test result: ok. 12 passed; 0 failed; 0 ignored
running 20 tests    test result: ok. 20 passed; 0 failed; 0 ignored
running 4 tests     test result: ok. 4 passed; 0 failed; 0 ignored
running 10 tests    test result: ok. 10 passed; 0 failed; 0 ignored
running 12 tests    test result: ok. 12 passed; 0 failed; 0 ignored
running 7 tests     test result: ok. 7 passed; 0 failed; 0 ignored
running 6 tests     test result: ok. 6 passed; 0 failed; 0 ignored
running 12 tests    test result: ok. 12 passed; 0 failed; 0 ignored
running 16 tests    test result: ok. 16 passed; 0 failed; 0 ignored
running 15 tests    test result: ok. 15 passed; 0 failed; 0 ignored
running 16 tests    test result: ok. 16 passed; 0 failed; 0 ignored
running 1 test      test result: ok. 1 passed; 0 failed; 0 ignored
running 4 tests     test result: ok. 4 passed; 0 failed; 0 ignored
running 4 tests     test result: ok. 4 passed; 0 failed; 0 ignored
running 1 test      test result: ok. 0 passed; 0 failed; 1 ignored
running 17 tests    test result: ok. 17 passed; 0 failed; 0 ignored
running 6 tests     test result: ok. 6 passed; 0 failed; 0 ignored
running 6 tests     test result: ok. 6 passed; 0 failed; 0 ignored
running 13 tests    test result: ok. 13 passed; 0 failed; 0 ignored

$ cargo test                       # default features, 5 targets
running 776 tests   test result: ok. 776 passed; 0 failed; 0 ignored
running 112 tests   test result: ok. 112 passed; 0 failed; 0 ignored
running 11 tests    test result: ok. 11 passed; 0 failed; 0 ignored
running 4 tests     test result: ok. 4 passed; 0 failed; 0 ignored
running 11 tests    test result: ok. 11 passed; 0 failed; 0 ignored
```

Lib tests: **776 total, unchanged** — 764 passed + 12 failed at base, 776
passed now. No test was added or removed; the twelve are the whole delta.

## Conflicts

**No property was lost.** In three cases a property's *observable* was
destroyed by ruling 271 and had to be re-founded rather than translated, and
those are recorded above rather than here, because a replacement observable
was found for each:

* `a_padding_only_packet_elicits_no_ack` and
  `a_keepalive_elicits_no_ack_but_is_still_acknowledged` read *"this is the
  1st and not the 2nd"* off an empty transmit list. The 2nd now transmits
  nothing either. Re-founded on the armed **deadline**
  (`t + MAX_ACK_DELAY` vs `t`).
* `a_replayed_packet_does_not_advance_the_every_second_counter` was built so
  that a folding build *"emits an ACK the correct build does not"*.
  Re-founded on the same deadline distinction — an assertion that was
  already in the test, one line below the one that used to do the work.

Four things for the maintainer that are **not** mine to change:

1. **`ACK_COALESCE_MAX` has no test.** The valve is a new ratified constant
   and neither the author's ten nor these twelve exercise it. The only place
   it is visible anywhere in the suite is as a *fixture symptom*: 400
   undrained pings buy 13 sealed counters instead of 200, which is
   `1 + 400/32`. Deleting the valve branch from `on_recv` — with the burst
   then bounded only by a sender stall, which is the hazard `constants.rs`
   says it exists to remove — turns nothing red. Round 42 material.

2. **`.slices/05-reliability/` still states the superseded behaviour as the
   acceptance criterion**, for both renamed tests:
   * `IMPLEMENTATION-5a.md:463` —
     `` `the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer` ``
     | *"§12.4's two triggers are distinguishable; an immediate-ACK build
     fails the middle assertion"*
   * `TESTS-5a-ack.md:137` —
     `` `one_packet_arms_the_timer_and_the_second_emits_the_ack` ``
     | *"the emitted packet count **and** `conn.timer(...)` at each step"*

   These are historical slice records and outside this task's partition, so
   they are reported, not swept (working rule 4 — grep for the rationale, not
   only the token: both rows argue the emission point, not merely name it).

3. **The author's §C1 conflict is live, not documentary.**
   `twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack`
   carries the caveat *"if this passes at base it pins nothing"*. It does not
   pass at base, **and it goes red under the revert-mutant** — so the literal
   reading of "one receive-drain" (N `handle_datagram` calls before a single
   `poll_output` loop) is pinned by a live test, and the author's request for
   a decision between the two readings is a decision about a gate rather than
   about a comment.

4. **`smoke::exchange` is now a footgun and only its neighbour says so.**
   It is left unchanged, and four green call sites still use it correctly.
   But *"shuttle until both cores are quiet"* is no longer the same as
   *"shuttle until the exchange is complete"*, and the next test written
   against it will hit exactly the failure `a_snapshot_settles…` hit: a
   non-§12 assertion going red for a §12.4 reason, with no §12 vocabulary in
   the panic. `settle` sits directly beneath it with that stated, which is
   the cheapest guard available from inside the partition; whether `exchange`
   should absorb `settle` outright is a call above this task.

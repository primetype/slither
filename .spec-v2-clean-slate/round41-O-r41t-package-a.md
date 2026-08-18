# Round 41, report O — slice R41-T package A (connection-core carried gaps)

*Author: Opus agent, worktree cut from `980cd13` (verified first act);
commit `15a7bdf`, cherry-picked to main as `7501921`. Verbatim package
report below. Independent mutation verification: `round41-Q`.*

---

# Slice R41-T — Package A report

Base commit (verified, rule 14): `980cd131cd8dc5c3ee1635cab687573dd905ca91`
(`git rev-parse HEAD` → 980cd13 "Ruling 259(viii) record: the config-raisable
windows enter §10.2, with their rule-4 companions") — matches the briefed base.

## Items

- Item 1 — vacuous NewReno-reset test (`src/core/connection/tests_roam.rs`): **DONE**, both halves (cwnd and ssthresh); no accessor was needed, so nothing is blocked
- Item 3 — O43e per-session budget independence (`src/core/connection/tests_path.rs`): **DONE**
- Item 6 — the 2^62-1 STREAM offset bound (`src/core/connection/tests_streams.rs`): **DONE**, two-sided

## Tests added

(pending)

## Preconditions asserted

(pending)

## Mutations each test separates

(pending)

## Gates run

(pending)

## Conflicts found

(pending)

---

## Research notes (appended as read, rule 2)

### Item 1 surfaces (verified at 980cd13)
- `NewReno::reset` — `src/core/connection/congestion.rs:82-87`; sets
  `cwnd = INITIAL_WINDOW`, `ssthresh = u64::MAX`, `recovery_start = Some(now)`,
  `acked_accum = 0`.
- `NewReno::ssthresh()` — `congestion.rs:99-102`, `#[cfg(test)] pub(crate)`.
  **No `Connection`-level ssthresh accessor exists** (`mod.rs` has
  `congestion_window()` at 1291 only). The `congestion` field (`mod.rs:236`) is
  private to module `crate::core::connection`, and `tests_roam` is a *child*
  module of it, so `solo.conn.congestion.ssthresh()` is reachable from the test
  **without any production change**. Item 1's ssthresh half is therefore NOT
  blocked on an accessor.
- `Controller::on_ack` (`congestion.rs:117-137`) returns early on `app_limited`
  and on `in_recovery(sent_time)`. `app_limited` is stamped at send time
  (`mod.rs:2588`): `!self.owes_output() && bytes_in_flight + size < cwnd`. So
  **only a packet sent while more output was still queued grows cwnd** — the
  fixture must queue more than one packet's worth.
- Constants: `INITIAL_WINDOW = 12_000`, `MAX_DATAGRAM = 1200`,
  `MINIMUM_WINDOW = 2_400`, `K_PACKET_THRESHOLD = 3`,
  `INITIAL_MAX_STREAM_DATA = 262_144`.
- `tests_roam.rs:41` `ack_frame(largest)` acks a **single** counter
  (`first_range = 0`). Acking only the largest of a burst declares the packets
  ≥3 behind it lost (packet threshold) and fires a congestion event, which would
  destroy a `cwnd > INITIAL_WINDOW` precondition — so the growth test needs a
  **ranged** ACK covering the whole burst. `Ack::ranges_desc`
  (`frame.rs:486-495`): `smallest = largest - first_range`, so `first_range = k`
  covers `largest-k ..= largest`.

### Item 3 surfaces
- `Amplification` — `mobility.rs:78-91`, a plain `Copy` field on `Connection`
  (`mod.rs:237`). `admits` at `mobility.rs:139-145`, `room` at 178-186,
  `on_sent` 189-194, `on_recv` 198-203.
- `Connection::amplification_budget()` — `mod.rs:459-461`, `Some((sent, recv))`
  while unvalidated.
- `Solo::installed_from_msg1_at(now)` (`testfix.rs:1131`) is the msg1-anchored
  constructor: budget **armed** (ruling 200). `Solo::installed_at` is dialled ⇒
  validated ⇒ no budget. Both anchor on `a_addr()`, so two `Solo`s are two
  connections against **one** peer address — exactly ruling 170's shape.
- `tests_path.rs:134` `room(solo)` = `3 × recv − sent`; `testfix::write_all`
  at `testfix.rs:831`.

### Item 6 surfaces
- Guard: `frame.rs:410-416`, `offset.checked_add(data.len()).filter(|end| *end
  <= VarInt::MAX_VALUE).ok_or(Structural::StreamOffsetOverflow)?`.
- `VarInt::MAX_VALUE` — `src/varint.rs:41`.
- **Ordering, verified in code**: `Connection::handle_datagram`'s frame stream
  is parsed *whole* first — `mod.rs:1689` matches `Received::Structural(error)`
  and CLOSEs with `PROTOCOL_VIOLATION` *before* `Received::Frames(frames)` at
  1708 is applied. Semantic checks (`kill()`, `mod.rs:1866-1882`) therefore run
  strictly after the structural parse. `check_order`'s own header
  (`tests_streams.rs:694-699`) states the semantic order as
  `legality → watermark → limit → final size → flow control`.
- Both classes surface as `ConnectionLost::ProtocolViolation { code }`; only the
  **code** distinguishes them (`PROTOCOL_VIOLATION` 0x0a vs
  `FLOW_CONTROL_ERROR` 0x02). `testfix::assert_violation(d, frames, code)`
  asserts both the `Closed` output and the CLOSE frame's code.
- Consequence for the accept side: a frame ending **exactly** at 2^62−1 is
  structurally legal but is far past `INITIAL_MAX_STREAM_DATA`, so the
  observable of "structurally accepted" is `FLOW_CONTROL_ERROR` — i.e. the
  connection got past the decoder into §10's semantics.

---

## Tests added / rewritten

### Item 1 — `src/core/connection/tests_roam.rs`
1. **`a_roam_resets_the_controller_and_keeps_the_flight`** (rewritten
   fixture; name kept). New helper `ack_range_frame(largest, first_range)`
   added beside `ack_frame`, and `write_all` added to the file's
   `testfix` import.
2. **`a_roam_lifts_the_slow_start_threshold_back_to_u64_max`** (new).

### Item 3 — `src/core/connection/tests_path.rs`
3. **`two_sessions_at_one_peer_address_hold_two_independent_budgets`** (new).

### Item 6 — `src/core/connection/tests_streams.rs`
New module `offset_ceiling`:
4. **`a_stream_frame_ending_exactly_at_the_varint_ceiling_clears_the_structural_guard`**
5. **`a_stream_frame_ending_one_byte_past_the_varint_ceiling_is_a_structural_error`**

## Preconditions asserted (rule 9)

| test | precondition | why it guards the test's validity |
|---|---|---|
| 1 | `burst >= 2` transmits | §14.5 stamps `app_limited` on the packet that empties the queue; a one-packet burst grows `cwnd` by nothing |
| 1 | `bytes_in_flight == 0` after the ranged ACK | proves no packet was left behind to be declared lost, which would *halve* cwnd instead of growing it |
| 1 | `cwnd > INITIAL_WINDOW` before the roam | **the item's whole point** — the old fixture never left `INITIAL_WINDOW`, so an empty `reset()` body passed |
| 1 | `min_rtt().is_some()` before the roam | the pre-existing `min_rtt == None` assertion was vacuous for the same reason |
| 1 | `in_flight > 0` before the roam | keeps §13.6's "the flight survives" half non-vacuous |
| 2 | `ssthresh() == u64::MAX` at start, then `< u64::MAX` after the loss | `u64::MAX` is the *initial* value, so without a cut first the post-roam assertion holds for a `reset` that never assigns `ssthresh` |
| 2 | `burst > K_PACKET_THRESHOLD + 1` | a head-only ACK only declares loss if packets sit past the threshold |
| 2 | `path_generation() == 1` after | the roam actually happened |
| 3 | both budgets armed and **equal** at start; `spent < cap` | any later divergence is something one connection did |
| 3 | every transmit's `to == a_addr()` on both cores | makes "anchored to the same peer address" asserted, not assumed |
| 3 | `room(&a) < 31` after A's burst | A is genuinely exhausted (refuses even a bare PING datagram) |
| 3 | B's receive counter moved by ≥ 600, and B emitted nothing on it | the credit is really there **and** still unspent, so "A may not use it" is a claim about ownership |
| 3 | `room(&b) > 31` after funding | B's window is genuinely open again |
| 4 | `offset + 1 == VarInt::MAX_VALUE`, and `offset > INITIAL_MAX_STREAM_DATA` | the frame ends *at* the ceiling, and is outside §10's window so `FLOW_CONTROL_ERROR` is the disposition a passing decoder must produce |
| 4 | one-byte payload (stated in the rustdoc) | an empty payload makes `checked_add` a no-op and the assertion free |

## Mutations each test separates

| # | test | mutant it must go red for |
|---|---|---|
| 1 | `a_roam_resets_the_controller_and_keeps_the_flight` | `NewReno::reset` body emptied (`cwnd` keeps its grown value; `min_rtt` keeps the old path's floor) |
| 2 | `a_roam_lifts_the_slow_start_threshold_back_to_u64_max` | `self.ssthresh = u64::MAX;` deleted from `NewReno::reset` |
| 3 | `two_sessions_at_one_peer_address_hold_two_independent_budgets` | any build sharing budget state between connections (thread-local/`static` counters, or the endpoint-side per-address table §7.3 forbids) — three separate assertions fail |
| 4 | `..._exactly_at_the_varint_ceiling_clears_the_structural_guard` | `<=` tightened to `<` in `frame.rs`'s `.filter(|end| *end <= VarInt::MAX_VALUE)` |
| 5 | `..._one_byte_past_the_varint_ceiling_is_a_structural_error` | the `.filter(...)` clause dropped, leaving the bare `checked_add` |

## Gates run (rule 7 — command and output)

```
$ cargo fmt --all --check
(no output; exit 0)
```

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a001eb0e6c2158a08)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.01s
(zero warnings)
```

```
$ cargo test --all-features   # all 25 result lines
test result: ok. 758 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.46s
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.45s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 112 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.77s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s
```

The lib suite went 754 → **758**: four new tests (one of the five items is a
rewritten fixture under its existing name). The five, named and green:

```
$ cargo test --all-features --lib -- --exact \
    core::connection::tests_roam::a_roam_resets_the_controller_and_keeps_the_flight \
    core::connection::tests_roam::a_roam_lifts_the_slow_start_threshold_back_to_u64_max \
    core::connection::tests_path::two_sessions_at_one_peer_address_hold_two_independent_budgets \
    core::connection::tests_streams::offset_ceiling::a_stream_frame_ending_exactly_at_the_varint_ceiling_clears_the_structural_guard \
    core::connection::tests_streams::offset_ceiling::a_stream_frame_ending_one_byte_past_the_varint_ceiling_is_a_structural_error

running 5 tests
test core::connection::tests_streams::offset_ceiling::a_stream_frame_ending_one_byte_past_the_varint_ceiling_is_a_structural_error ... ok
test core::connection::tests_streams::offset_ceiling::a_stream_frame_ending_exactly_at_the_varint_ceiling_clears_the_structural_guard ... ok
test core::connection::tests_roam::a_roam_resets_the_controller_and_keeps_the_flight ... ok
test core::connection::tests_roam::a_roam_lifts_the_slow_start_threshold_back_to_u64_max ... ok
test core::connection::tests_path::two_sessions_at_one_peer_address_hold_two_independent_budgets ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 753 filtered out; finished in 0.06s
```

Nothing else changed: every other result line above is identical in count to
the base run, and no test outside my three files was touched.

## Mutation verification (run in this worktree, at 92ed272)

Each mutant was applied to production code, the test run, and the file
restored with `git checkout` (rule 10: the tests were committed first;
`git status --porcelain` is empty afterwards, verified).

**Mutant 1 — `NewReno::reset` body emptied** (`congestion.rs`):
```
$ cargo test --all-features --lib a_roam_resets_the_controller_and_keeps_the_flight
test core::connection::tests_roam::a_roam_resets_the_controller_and_keeps_the_flight ... FAILED
assertion `left == right` failed: §14.6: cwnd resets to INITIAL_WINDOW
  left: 15600
 right: 12000
```
The rewritten fixture grows cwnd to 15 600 before the roam. At 980cd13 this
same mutant was **green** — that is the gap item 1 names, now closed.
The ssthresh companion is also red under this mutant (6000 vs u64::MAX).

**Mutant 2 — `self.ssthresh = u64::MAX;` deleted from `reset`:**
```
$ cargo test --all-features --lib a_roam_
test core::connection::tests_roam::a_roam_resets_the_controller_and_keeps_the_flight ... ok
test core::connection::tests_roam::a_roam_lifts_the_slow_start_threshold_back_to_u64_max ... FAILED
assertion `left == right` failed: §14.6 / §13.6: ... ssthresh = u64::MAX is half of that state
  left: 6000
 right: 18446744073709551615
```
Exactly the split the pair is for: the cwnd test cannot see this mutant, the
ssthresh test can.

**Mutant 3 — budget state shared between connections** (a thread-local
`(sent, recv)` pair in `mobility.rs`, seeded by the first arming and read by
`admits`/`room`/`counters`, written by `on_sent`/`on_recv` — the
endpoint-side per-address table §7.3 forbids):
```
$ cargo test --all-features --lib two_sessions_at_one_peer_address
test ...two_sessions_at_one_peer_address_hold_two_independent_budgets ... FAILED
assertion `left == right` failed: §7.3: B's counters are its own. ...
  left: Some((588, 196))
 right: Some((214, 196))
```
Deleting that assertion and re-running reaches the next failure —
`*exhausting one must not throttle the other*. B has spent nothing of its
own and must still be admitted: []` — and deleting that one too reaches a
third, in half 2. Three independent points, all restored afterwards.

**Mutant 4 — `<=` tightened to `<` in `frame.rs`'s filter:**
```
$ cargo test --all-features --lib offset_ceiling
test ...a_stream_frame_ending_one_byte_past_the_varint_ceiling_is_a_structural_error ... ok
test ...a_stream_frame_ending_exactly_at_the_varint_ceiling_clears_the_structural_guard ... FAILED
  left: Some(ProtocolViolation { code: 1 })     # PROTOCOL_VIOLATION
 right: Some(ProtocolViolation { code: 2 })     # FLOW_CONTROL_ERROR
```

**Mutant 5 — the `.filter(|end| *end <= VarInt::MAX_VALUE)` clause dropped:**
```
$ cargo test --all-features --lib offset_ceiling
test ...a_stream_frame_ending_exactly_at_the_varint_ceiling_clears_the_structural_guard ... ok
test ...a_stream_frame_ending_one_byte_past_the_varint_ceiling_is_a_structural_error ... FAILED
  left: Some(ProtocolViolation { code: 2 })     # FLOW_CONTROL_ERROR
 right: Some(ProtocolViolation { code: 1 })     # PROTOCOL_VIOLATION
```
Each mutant kills exactly one side of the pair — neither test is passed by
the build the other exists to catch.

## Conflicts found

**None between spec, code and record** on any of the three items. The three
artefacts agree: §14.6 / §13.6's table and `NewReno::reset` name the same
two-field reset; Appendix B's O43e bullet, §7.3's normative "scope is one
session" paragraph and `Amplification`-as-a-`Connection`-field agree; §8.4's
`offset + length` clause and `frame.rs`'s guard agree, and the accept-side
disposition follows from `mod.rs`'s structural-before-semantic ordering
rather than from any statement in tension with another.

Two **non-conflict** observations recorded for the round, both about the
test suite rather than the spec:

1. The pre-existing `min_rtt == None` assertion in
   `a_roam_resets_the_controller_and_keeps_the_flight` was vacuous for the
   *same* reason its cwnd assertion was — no ACK ever reached the fixture,
   so `min_rtt` was already `None` before the roam. The triage record
   (item 1) names only the cwnd half. It is now a precondition
   (`min_rtt().is_some()` before the roam) and the post-roam assertion has
   become a real one. **Not** a conflict; an extension of the same finding.
2. Item 3's brief says "fund A's recv counter only; assert B's admission
   bound is unmoved", with A being the connection exhausted in half 1. I
   implemented the obligation in the **other direction** — the funded core
   is the one whose send queue is empty — and the rustdoc says why:
   funding the exhausted core lets its own pump consume the whole credit
   before any assertion runs, which makes a shared counter behaviourally
   indistinguishable from a private one on that half. This was not
   theoretical: the first draft did it the brief's way and its premise
   assertion measured `room 0` immediately after the funding delivery.
   The obligation is symmetric and both cores are constructed identically,
   so the property asserted is the one Appendix B states.

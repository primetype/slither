# Slice 4a — acceptance tests — report (TEST-S)

**73 tests, 3 489 lines, in `src/core/connection/tests_streams.rs`.**

## 0. Status — UNRUN, and no gate is claimed

There is no slice-4a implementation in this worktree, so **these tests
have never been compiled or executed against one** (working rule 7). What
*was* run, and its output:

```
$ rustfmt --edition 2024 --check src/core/connection/tests_streams.rs
(no output — the file parses and is already formatted)

$ cargo check --all-features --all-targets
    Checking slither v0.2.0 (…/agent-a6f35b4c328026c96)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.10s
```

`rustfmt` is a full parse, so the file is syntactically valid Rust and
carries no formatting debt. The `cargo check` covers the crate **without**
this file — it is untracked and `mod tests_streams;` is not declared (that
is the integrator's edit) — so it establishes that the baseline is green
and any failure after integration is attributable to integration.

No type checking has happened. Section 5 lists where that is most likely
to bite and why each is a one-line fix.

## 1. A process note on the contract

`.slices/04-streams/CONTRACT-4a.md` **does not exist at `fdf5972`**, the
commit this worktree was cut from. It landed on `main` at `74fa5f2`
("Ruling 93 amended…"), one commit ahead. I worked for about ten minutes
reconstructing the API from `PLAN.md` §6.1 and Round 17 before the
coordinator's mid-task message revealed the file existed, at which point I
read it out of `main` with `git show` and discarded the reconstruction.

Two guesses I had made and the contract overturned, recorded because they
show what a worktree cut one commit early costs: I had `ConnEvent::
StreamReadable { stream: StreamRef }` (the contract says `{ r: StreamRef }`)
and I had no `Ok(Some(0))`/`Ok(None)` distinction for `read` at all.
Either would have been a real, silent semantic divergence, not a rename.

**Suggestion for the next dispatch: cut the agent's worktree from the
commit that contains its brief's inputs, and have the brief name that
commit.** Nothing else in the arrangement guards against this.

## 2. What the tests are, and how they are organised

| Module | Tests | Subject |
|---|---|---|
| `precursors` | 3 | §7.2's three named precursors |
| `identifiers` | 4 | §9.1's tag, parity, ruling 106, the 2⁶⁰ ceiling |
| `early_sends` | 2 | §16.9 / ruling 95, `StreamRef` across install |
| `implicit_opening` | 7 | §9.2, rulings 99 and 100, §10.4's two-sided limits |
| `check_order` | 6 | ruling 97's five-stage order, one pair per test |
| `abandonment` | 7 | ruling 93 and its amendment |
| `tombstone` | 3 | §9.2's watermark, ruling 105, §8.4's credit-frame legality |
| `reassembly` | 10 | §9.5, §10.6's two memory bounds, ruling 94, ruling 104 |
| `flow_control` | 11 | §10.1–§10.5, H15's seed, the two-level separation |
| `reset` | 11 | §9.6, §8.4's ordering mandate, §11.8's arithmetic |
| `sealing` | 3 | ruling 98's `seal` / `seal_quiet` table |
| `packing` | 3 | §8.5's order, round-robin, extends-to-end |
| `slice_boundary` | 4 | what 4a must **not** do, asserted rather than assumed |

Every test carries a `Mutation caught:` paragraph in its doc comment. The
rest of this section names only the ones where the choice of assertion was
the hard part.

### 2.1 The rulings' own named tests

| Ruling | Test | The mutation, and the assertion that separates it |
|---|---|---|
| 93 | `a_dropped_recv_stream_releases_connection_credit_at_once` | §16.2's "abandonment merely *arms*" — under which no MAX_DATA is ever emitted, because a sender stalled at the stream window never sends a FIN. Separated by the grant appearing **in the same drain as the abandonment**, no peer frame intervening. |
| 93 | `…trues_up_to_the_stream_window_not_the_high_water_mark` | The planner's provisional. Two streams that received 1 000 bytes each and advertised 262 144 each: the provisional releases 2 000 and emits **nothing**; the ruling releases exactly 524 288 — `INITIAL_MAX_DATA / 2`, §10.3's trigger — and emits one grant. The assertion is on the **exact `max`** (`consumed + INITIAL_MAX_DATA`), so a build truing up to some third value also fails. |
| 94 | `buffered_bytes_stay_within_the_connection_window_across_many_streams` | Eager per-stream allocation. 128 peer-opened uni streams, one byte each, asserted on `reassembly_capacity()` — **not** bytes received, which an eager allocator passes for free. Two bounds: `<= INITIAL_MAX_DATA`, and `< 128 × INITIAL_MAX_STREAM_DATA` (ruling 94's own 32 MiB figure). |
| 95 | `an_early_opened_stream_keeps_its_handle_across_install` | Remap-at-install. Opens and writes 4 KiB before install, 4 KiB after **through the same `StreamRef`**, and asserts the peer reads 8 KiB contiguous — so a remap that quietly opened a second stream fails on content, and `accept()` returning `None` a second time catches it directly. |
| 97 | `a_stream_frame_on_a_closed_local_uni_space_is_a_state_error` | No legality check at all. **Reduced from the ruling's scenario — see §3.1.** |
| 98 | `a_max_streams_only_packet_does_not_defer_the_keepalive` | Marking credit frames. Both halves per ruling 33: `last_send` unchanged **and** the death clock armed, since a build making credit neither marking nor ack-eliciting passes the first alone. The fixture asserts the packet really is credit-only, or the test would assert nothing. |
| 99 | `an_implicit_open_of_six_streams_emits_six_stream_opened_events` | One event per *frame*. The count is the pin; `>= 1` passes the broken build. |
| 99 | `a_frame_above_the_cumulative_limit_emits_zero_events_before_the_kill` | Open-then-validate. **Zero** events, not "it died" — a build emitting 129 events and then killing passes an error-code-only assertion. |
| 100 | `an_empty_finless_stream_frame_opens_its_stream` | §9.5's no-op read as suppressing §9.2's open. Paired with `…pins_nothing_and_delivers_nothing`, which pins the half that genuinely *is* a no-op: no final size (a later FIN at 10 is accepted), no bytes, no capacity. |
| 104 | `exactly_1024_stored_ranges_survive_and_the_1025th_is_a_protocol_violation` | Any other ceiling. Two-sided; a one-sided "it eventually dies" passes a build that dies at 2. Paired with `contiguous_ranges_coalesce_so_four_thousand_frames_survive`, which is what separates "coalesces" from "has a low ceiling", and asserts the **reassembled bytes** as well, because coalescing by overwriting gaps survives the count and loses data. |
| 105 | `a_duplicate_stream_frame_after_the_receive_half_is_freed_is_a_no_op` | No watermark. Zero new `StreamOpened`, `accept()` → `None`, `reassembly_capacity()` → 0. The loss-driven variant is **owed to slice 7** (§3.4). |
| 106 | `stream_ids_carry_the_role_from_install_not_from_who_created_the_core` | Parity derived from "created by `connect()`". **Both** cores in the fixture are created by `connecting()`; only the `Role` on their `Install` differs — §6.6 step 4's shape. Exact ids (0/1/2/3), because "the two sides differ" passes a build with one shared counter. |

### 2.2 Ruling 93's amendment — both mechanisms, as instructed

Four tests, written so that **each broken build reds at least one**:

- `abandoning_peer_opened_uni_halves_fully_closes_them_and_grants_max_streams`
  — two-sided at `STREAMS_CREDIT_BATCH`: seven closures owe nothing, the
  eighth owes exactly `MaxStreamsUni(128 + 8)`. *Per-half tombstone only*
  never advances the uni watermark and reds here.
- `a_frame_below_the_watermark_beyond_credit_is_a_no_op_not_a_violation`
  (in `check_order`) — the same broken build reaches the frozen
  stream-level limit and kills; §9.2 says the frame is ACKed and inert.
- `an_abandoned_bidi_receive_half_discards_arrivals_without_reopening` —
  *watermark only* resurrects the half; **zero** new `StreamOpened` is the
  separating assertion.
- `an_abandoned_bidi_receive_half_still_enforces_its_frozen_stream_limit`
  — the anti-unbounded-sink pin. Without it, "discards arrivals" taken
  literally passes the test above; the two are only a pin together.

Plus `abandoning_bidi_receive_halves_grants_no_max_streams` (our send half
is live, so §9.7's "fully closed" is not met) and
`abandoning_a_receive_half_releases_its_reassembly_capacity`.

### 2.3 Where I deliberately did **not** assert

- **`the_stream_fill_serves_pending_streams_round_robin`** does *not*
  assert that some packet carries frames for both streams. §8.5 makes the
  quantum implementation-defined (`PLAN.md` §6.4), and a quantum of one
  packet is conforming and would fail that. An assertion a conforming
  build can fail is a flake, not a pin. What is asserted is
  **interleaving**: `first(B) < last(A)` and `first(A) < last(B)`, which a
  strictly sequential fill cannot satisfy — §11.2's warning that "both
  streams made progress" passes a sequential fill measured at the end.
- **`credit_frames_precede_the_stream_fill_in_a_packet`** ends with
  `assert!(found, …)`. Without it the loop asserts nothing whenever the
  core does not coalesce, and the test would read as a pin while pinning
  nothing.
- **Ruling 102** (`STREAMS_CREDIT_BATCH` used twice) — the ruling itself
  says no test can separate the readings at today's values. Not attempted;
  attempting it would have produced a name that is not a pin.

## 3. Obligations I could not reach, and why

These are coverage gaps in the **slice**, not in the fixture, except where
noted. Each is stated so it can be scheduled rather than rediscovered.

### 3.1 Two of ruling 97's four watermarks are untestable in 4a

Ruling 97 requires **all four** closed-stream watermarks and gives the
reason: the two locally-opened spaces serve §8.4's separate rule that
"credit for a fully-closed stream is a valid no-op".

A locally-opened stream fully closes only when its data or its
RESET_STREAM is **acknowledged** (§9.7), and slice 4 has no ACK
processing. So the local-bidi and local-uni watermarks **cannot advance in
4a at all**.

Consequences, stated plainly:

- **A build that maintains only the two peer-opened watermarks passes all
  73 tests in this file.** Ruling 97's own H3 warning — "an implementer
  who keeps two will meet a `STREAM_STATE_ERROR` where §8.4 promised a
  no-op" — is not defended by anything I can write here.
- Ruling 97's named test is therefore **reduced**: I assert that a STREAM
  frame on a local-uni index is `STREAM_STATE_ERROR`, which pins that the
  legality check exists and (with two further tests) that it precedes the
  limit and flow-control checks. It does **not** pin legality-before-
  watermark, because the frame that would separate them cannot be
  constructed. The test's doc comment says this in full so a reviewer
  cannot read the name as the whole claim.
- `max_stream_data_for_a_stream_we_cannot_send_on_is_a_state_error`
  covers only §8.4's legality half for credit frames; the "credit for a
  fully-closed stream is a valid no-op" half needs a fully-closed stream
  *we can send on*, which does not exist in 4a.

**Owed to slice 5** (the slice that adds ACK processing), three tests:
legality-before-watermark on a closed local-uni index; credit-for-a-
fully-closed-stream as a no-op; and a locally-opened stream's watermark
advancing at all.

### 3.2 §10.4's "closing streams we opened must not inflate the peer's allowance"

RFC 9000 §4.6's scope rule, named explicitly in §10.4. Unreachable for the
same reason: no stream we opened can fully close in 4a. The nearest
reachable statement is
`abandoning_bidi_receive_halves_grants_no_max_streams`, which asserts a
different thing (a half-closure is not a closure). **Owed to slice 5.**

### 3.3 Ruling 98's retransmission row

The table is: STREAM **first transmission** → `seal`; STREAM
**retransmission** → `seal_quiet`. Only the first is reachable — a
retransmission requires §13's loss recovery, which is slice 7. I assert
the marking side (`a_first_transmission_stream_frame_marks_last_send`) and
the two quiet rows that *are* reachable (credit frames, RESET_STREAM).
**The retransmission row is owed to slice 7**, and it is the row most
likely to be got wrong, since it is the only one where the same frame type
takes two different seals.

### 3.4 Ruling 105's loss-driven variant

Discharged here with an injected duplicate, which ruling 105 authorises
and which is the stronger stimulus (no delay). The ACK-drop-plus-PTO form
needs §12 and §13. **Owed to slice 7**, as ruling 105 already records.

### 3.5 `on_ack_range` / `on_lost_range`

`CONTRACT-4a.md` §5 says these are defined on the send half in slice 4 and
left uncalled, and that this "is what makes GC and watermark logic
testable now". **It does not make them testable from my file**: they are
methods on a type (`send.rs`'s send half) that appears nowhere in the
contract's API surface, and I have no way to obtain one from a
`Connection`. Everything in §3.1 above follows from that.

If the intent was that 4a's tests exercise GC through these, the contract
needs either a `pub(crate)` path from `Connection` to a send half or a
test-only `Connection` verb. I have not invented one — inventing a second
API guess would have doubled the integration risk for one test.

### 3.6 §11.8's release-mode requirement

`PLAN.md` §11.8 says to run the arithmetic test "in **release** as well as
debug", because `debug_assert`-based overflow checks compile out. A
`#[test]` cannot select its own profile. This is a **CI obligation**
(`cargo test --release`), not something this file can express, and it is
not currently in the release-gate table in `CLAUDE.md`.

### 3.7 The fault class this fixture cannot express (working rule 13)

Both fixtures here are sans-io and drive the core directly. They can
express every *protocol* fault. They cannot express a failing socket, a
panicking driver, or a dropped handle — all of which are 4b's content and
where the round-15 seam review found two of four defects. `PLAN.md` §11.10
already lists them; nothing here changes that, and no test in this file
should be read as covering any of it.

## 4. Conflicts and ambiguities found (working rule 3 — reported, not resolved)

### C-a. `read()` is a mutating call with no `Instant` — **highest value of anything here**

`CLAUDE.md`'s architecture invariant: "`now: Instant` is an argument on
every mutating call and the cores never read a clock". `CONTRACT-4a.md`
§2 gives `read(&mut self, r, buf)` **no `now`**.

But `read()` *is* mutating (it drains the contiguous prefix), and §10.3
makes **consumption** the thing that advances credit. So a `read()` can
cross the re-grant trigger and owe a MAX_STREAM_DATA or MAX_DATA. Sealing
that frame needs an instant, because §7.4 marks `last_send` on every seal
and §16.7 makes sealing synchronous inside the mutating call.

Three ways out, and the contract picks none:

1. `read()` gains a `now` — consistent with the invariant, changes the
   contract and the shell's `poll_read`.
2. The core **defers** the seal to the next call carrying an instant —
   consistent with the contract, contradicts §16.7's synchronous sealing,
   and makes the credit frame's emission time depend on unrelated traffic.
3. The core **caches** the last `now` it saw — not a clock read, but it
   means a credit frame can be sealed with a stale `last_send`, which
   §7.4's liveness accounting reads.

**I did not pick one.** Every test that expects a credit frame after a
read routes through a `tick()` helper (`conn.handle_timeout(now)`) that
hands the core an instant first, so the test holds under all three. That
helper's doc comment states the problem. Note that without it,
`one_byte_read_emits_no_credit_at_all` would pass **vacuously** under
design 2 — working rule 9's shape arriving from an unexpected direction.

The same question applies to `finish(r)`, which also takes no `now` and
which queues a FIN that must be sealed.

### C-b. `write(now, r, &[])` cannot be distinguished from a blocked write

`CONTRACT-4a.md` §2's table: `write` → `Ok(0)` means "**blocked** by
stream or connection credit; the shell parks". A zero-length write also
returns `Ok(0)` under any natural implementation — and it parks a writer
that has nothing to wait for, because no credit will ever arrive to
unblock it.

This is not hypothetical: ruling 100 establishes that empty STREAM frames
are legitimate protocol elements, and §16.2's `AsyncWrite` will be handed
empty buffers by ordinary `tokio::io` combinators.

No test written — the correct behaviour is undecided (is it `Ok(0)`
meaning "done", an error, or must the shell special-case it?) and writing
a test would be resolving it silently.

### C-c. §9.6's `final_size` — "bytes sent" or "bytes accepted"?

§9.6: RESET_STREAM's `final_size` is "the number of bytes the stream would
have carried (**the end offset of the highest byte sent**, or 0 if none)".
The parenthetical and the phrase it explains disagree whenever `write()`
has accepted bytes that have not yet been sealed onto the wire — which is
the normal state under §14's congestion control, and reachable in 4a
whenever `reset()` follows `write()` without an intervening drain.

Ruling 56 pins that accepted bytes "are already in send state", which
argues for "accepted"; the parenthetical says "sent".

`our_reset_carries_the_highest_byte_sent_as_its_final_size` **drains
before resetting**, so the two readings coincide and the test is valid
under either. The ambiguity is real and an implementer will have to pick.

### C-d. `accept(dir)`'s claim order is unstated

§16.4 says `accept(dir)` returns one stream per call; ruling 99 fixes the
event *count*. Neither states which stream. Working rule 8's shape: a
stated construction with an unstated scope.

It matters — §9.2 opens a *run* of indices at once, and an application
that assumes ascending order (the natural assumption, and what QUIC
implementations do) would be relying on nothing. My `accept_all()` helper
**sorts by wire id** and asserts the claimable *set*, so no test here
depends on the answer. Worth a ruling before 4b's handle-level
`accept_uni()` bakes an order into an example.

### C-e. `CONTRACT-4a.md` §2 has no abandonment verb

Ruling 93 and its amendment both specify core-level behaviour "on
abandoning the receive half". The contract's §2 lists seven verbs and none
of them is that; ruling 95's "eleven sites: the five verbs, `accept`,
`stream_id`, and the four stream-naming `ConnEvent`s" omits it too — a
miscounted list inside the ruling round that named the miscounted-list
defect as recursive.

The coordinator's mid-task message says "at core level, whatever verb the
contract gives you for it", which is exactly the gap. I guessed
`abandon_recv(&mut self, now: Instant, r: StreamRef)` and routed all seven
abandonment tests through a one-line shim so the guess costs one edit. See
§5.

## 5. Where integration will bite, in order of probability

| # | Site | Fix |
|---|---|---|
| 1 | **`fn abandon_recv(conn, now, r)`** — the shim at the top of the file, forwarding to `conn.abandon_recv(now, r)`. The verb is not in the contract (§4 C-e). | Rewrite the shim's body. One line; seven tests follow. |
| 2 | **`use super::streams::StreamRef;`** — the contract names `stream_id.rs` for `StreamId`/`Dir` but leaves `StreamRef`'s module unstated. I guessed `streams.rs`. | One `use` line. |
| 3 | `use super::stream_id::{Dir, StreamId};` — the module *is* contract-named, so this should hold. | One `use` line. |
| 4 | `tick()`'s use of `handle_timeout(now)` assumes an untriggered timeout is harmless (slice 3a's `Fixture::timeout` does the same). | If it is not, replace `tick`'s body. |
| 5 | `Connection::liveness()`, `Liveness::last_send()`, `is_armed()` — existing slice-3a accessors, used by `sealing`. | Should hold. |

Everything else is contract-pinned: the seven verbs, `reassembly_capacity()
-> u64`, `role() -> Option<Role>`, the six `ConnEvent` variants with field
name `r`, `StreamId`'s five methods, `Dir { Bi, Uni }`, and the
`Ok(0)`/`Ok(Some(0))`/`Ok(None)` conventions.

**A deliberate deviation from the brief, flagged rather than done silently
(working rule 5):** the brief says "every flow test is
`#[tokio::test(start_paused = true)]`". These are sans-io core tests —
`now` is an argument and nothing reads a clock — so there is no virtual
time to pause and the attribute would attach a runtime nothing awaits.
`src/core/connection/tests.rs` uses plain `#[test]` for the same reason.
Plain `#[test]` throughout; **no `sleep` anywhere**. The paused-clock
requirement is 4b's, where there are futures to drive.

## 6. One thing in the contract I think is wrong

`CONTRACT-4a.md` §5 says `on_ack_range`/`on_lost_range` being "defined on
the send half in slice 4 and left uncalled from the wire … is what makes
GC and watermark logic testable now." **It does not.** They are on a type
the contract never surfaces, and no path from `Connection` reaches them
(§3.5). The consequence is §3.1: half of ruling 97's watermark requirement
ships in 4a with no test that can fail.

This is working rule 11's shape in the contract rather than in a ruling —
a rationale naming a mechanism that does not connect to the thing it
claims to enable. It is worth either surfacing a path or moving the
watermark obligation explicitly to slice 5, so that it is a scheduled debt
rather than an assumed coverage.

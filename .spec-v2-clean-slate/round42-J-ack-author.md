# AUTHOR-ACK-REPORT — blind test author, ACK-cadence slice (ruling 271)

Base commit: `99998d57c17cccfec73b79ed2fee90a8ea1212cd` (99998d5) — verified as
first act per rule 14. Worktree: `.claude/worktrees/agent-aad8d0de4c3f6996b`.

Partition check at base: `src/core/connection/tests_ack_cadence.rs` — absent.
`tests/story_ack_cadence.rs` — absent. Names kept.

## Tests

Ten, all in `src/core/connection/tests_ack_cadence.rs` — the only source
file this author created. **No `tests/story_ack_cadence.rs`**; see
"Why there is no story-level file" below.

| # | Test | Subject | At base |
|---|---|---|---|
| 1 | `a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack` | ruling 271's cadence, receive-batch reading | **RED (10)** |
| 2 | `twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack` | ruling 271's cadence, literal one-`poll_output` reading | **RED (10)** |
| 3 | `a_lone_ack_eliciting_packet_is_acked_within_max_ack_delay` | §12.4's outer bound, single packet then silence | green |
| 4 | `the_tail_of_an_idle_burst_is_acked_within_max_ack_delay` | §12.4's outer bound, idle-after-burst tail | green |
| 5 | `a_gap_mid_batch_is_acked_in_the_same_batch` | §12.4 bullet two, immediate on out-of-order | green |
| 6 | `a_coalesced_ack_reports_every_range_a_per_packet_ack_would` | §12.2/§12.1 — coalescing changes *when*, not *what* | green |
| 7 | `a_single_lost_packet_is_recovered_without_advancing_the_clock` | §13.2 packet threshold, end to end, zero virtual time | green |
| 8 | `the_estimator_still_samples_across_a_coalesced_exchange` | §13.1 RTT sampling survives | green |
| 9 | `a_coalesced_ack_moves_no_liveness_clock` | ruling 33 / §7.4 `seal_quiet`, non-eliciting, untracked | green |
| 10 | `an_ack_only_emission_cannot_defer_death` | §7.4 *"the send clock never defers death"* | green |

Every one carries a rustdoc naming the mutation it separates (rule 9).

### Why there is no story-level file

The brief made `tests/story_ack_cadence.rs` optional. I did not write it,
for a reason that is in `Cargo.toml` itself (**ruling 194**, lines 272–275
and 307–310):

> There is no `autotests = false` here, so cargo auto-discovers both targets
> *without* their `required-features`, and the feature-less `cargo test`
> gate fails on `unresolved import slither::testutil` until these stanzas
> exist.

A story test needs `[[test]] name = … required-features = ["test-util"]`,
and `Cargo.toml` is the **integrator's** file (working rule 15 — slice 4b's
case exactly). Landing the file without the stanza reds every gate at once;
landing the stanza myself breaks the partition. The core file is also the
stronger pin: `Solo::packets` decrypts and decodes each emitted datagram, so
tests 1/2/5/6 assert on **exact frames and ranges**, where a story test with
no session keys could only count datagrams by size — round 42-G's own
method, and a heuristic.

**What is genuinely lost, and who should recover it.** C1's *second*
reading — that "one drain" is the driver's own receive-loop turn — lives in
`src/shell/driver.rs` and is **unobservable from the core seam**. If the
ratification takes that reading, someone must add a story test; the stanza
it needs is

```toml
[[test]]
name = "story_ack_cadence"
required-features = ["test-util"]
```

and `tests/spec_ack_burst.rs` is the model for the harness (FlakyWire,
paused clock, `Tap::datagrams()` census by size, `FlakyPolicy::drop_at` for
deterministic loss). That is integrator work, not author work.

## Red-green matrix at base

Command and output, verbatim (rule 7). Run with a **temporary** local
`#[cfg(test)] mod tests_ack_cadence;` in `src/core/connection/mod.rs`,
**reverted before the commit** — `git status` at commit time shows only the
two files this author owns.

```
$ cargo test --lib core::connection::tests_ack_cadence
running 10 tests
test core::connection::tests_ack_cadence::a_lone_ack_eliciting_packet_is_acked_within_max_ack_delay ... ok
test core::connection::tests_ack_cadence::a_gap_mid_batch_is_acked_in_the_same_batch ... ok
test core::connection::tests_ack_cadence::a_coalesced_ack_reports_every_range_a_per_packet_ack_would ... ok
test core::connection::tests_ack_cadence::a_coalesced_ack_moves_no_liveness_clock ... ok
test core::connection::tests_ack_cadence::the_tail_of_an_idle_burst_is_acked_within_max_ack_delay ... ok
test core::connection::tests_ack_cadence::an_ack_only_emission_cannot_defer_death ... ok
test core::connection::tests_ack_cadence::a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack ... FAILED
test core::connection::tests_ack_cadence::twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack ... FAILED
test core::connection::tests_ack_cadence::the_estimator_still_samples_across_a_coalesced_exchange ... ok
test core::connection::tests_ack_cadence::a_single_lost_packet_is_recovered_without_advancing_the_clock ... ok

failures:

---- ...::a_burst_of_twenty_in_one_receive_batch_emits_at_most_one_ack stdout ----
panicked at src/core/connection/tests_ack_cadence.rs:302:5:
§12.4 (ruling 271): 20 ack-eliciting datagrams in one receive batch owe at most one ACK emission, not 10

---- ...::twenty_datagrams_before_a_single_poll_output_loop_emit_at_most_one_ack stdout ----
panicked at src/core/connection/tests_ack_cadence.rs:350:5:
§12.4 (ruling 271), literal reading: one `poll_output` loop over 20 datagrams owes at most one ACK emission, not 10

test result: FAILED. 8 passed; 2 failed; 0 ignored; 0 measured; 766 filtered out; finished in 0.03s
```

**The reds are exact, not approximate: 10, from 20 packets.** That is
§12.4's every-2nd trigger, arithmetically, and it is round 42-G's measured
`data-per-ack = 1.975 / 1.999` reproduced at the core seam from first
principles rather than from a socket.

### The finding inside test 2 — worth more than the test

I expected test 2 (the literal reading) to be **green** at base and to
assert nothing, and wrote its rustdoc saying so. **It is red, with the same
10.** That is a fact about the core the ruling should have, because it
narrows where the fix can live:

§16.7 (*"Plan-seal-commit; sealing is synchronous"*) puts the seal **inside
the mutating call that triggers it**, so by the time `poll_output()` is
first called, all ten ACK packets are already sealed and queued. **A
coalescing fix therefore cannot be implemented in the drain.** Deferring
emission to `poll_output()` is the obvious first idea and it is closed: the
change has to be in the *policy* — what `on_recv` decides to owe — not in
when the queue is flushed. This also means the two readings of C1 are
**not** distinguishable at base, both being red at 10, so the conflict does
not have to be resolved before the implementer starts. It has to be
resolved before §12.4 is amended.

### The other gates, on this tree

```
$ cargo fmt --all --check
(no output)

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.81s

$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.59s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.30s
   Generated .../target/doc/slither/index.html

$ cargo test --all-features --no-fail-fast
     Running unittests src/lib.rs
test result: FAILED. 774 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s
     Running tests/spec_ack_burst.rs      → ok.  2 passed; 0 failed; 1 ignored
     Running tests/spec_compat.rs         → ok. 24 passed; 0 failed
     Running tests/spec_constants.rs      → ok. 112 passed; 0 failed
     Running tests/spec_errors.rs         → ok. 11 passed; 0 failed
     Running tests/spec_packet.rs         → ok.  4 passed; 0 failed
     Running tests/spec_rekey.rs          → ok.  5 passed; 0 failed
     Running tests/spec_shell.rs          → ok. 12 passed; 0 failed
     Running tests/spec_streams.rs        → ok. 20 passed; 0 failed
     Running tests/story_codec.rs         → ok.  4 passed; 0 failed
     Running tests/story_compat.rs        → ok. 10 passed; 0 failed
     Running tests/story_datagram.rs      → ok. 12 passed; 0 failed
     Running tests/story_dial.rs          → ok.  7 passed; 0 failed
     Running tests/story_flow.rs          → ok.  6 passed; 0 failed
     Running tests/story_keepalive.rs     → ok. 12 passed; 0 failed
     Running tests/story_lifecycle.rs     → ok. 16 passed; 0 failed
     Running tests/story_message.rs       → ok. 15 passed; 0 failed
     Running tests/story_mobility.rs      → ok. 16 passed; 0 failed
     Running tests/story_park.rs          → ok.  1 passed; 0 failed
     Running tests/story_path.rs          → ok.  4 passed; 0 failed
     Running tests/story_reassembly.rs    → ok.  4 passed; 0 failed
     Running tests/story_rekey.rs         → ok.  0 passed; 0 failed; 1 ignored
     Running tests/story_reliability.rs   → ok. 17 passed; 0 failed
     Running tests/story_streams.rs       → ok.  6 passed; 0 failed
     Running tests/story_tower.rs         → ok.  6 passed; 0 failed
   Doc-tests slither                      → ok. 13 passed; 0 failed
error: 1 target failed:   (the lib target — the two intended reds)

$ cargo test --release --all-features --no-fail-fast --lib
test result: FAILED. 774 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
  (the same two, with the same "not 10" — the red is deterministic, not a debug artefact)

$ cargo test --no-fail-fast            # feature-less
test result: FAILED. 774 passed; 2 failed; …   (lib; every integration target ok)
```

**Nothing else moved.** 774 of 776 lib tests pass and every integration
target is green, so these ten tests neither perturb an existing pin nor
depend on one. The wire pins (`spec_constants`, 112 passed) are untouched —
this slice moves no wire byte, by construction.

MSRV and `cargo deny` were **not** run: they are release gates on the
integration commit and neither is affected by a test-only file. Not claimed
as green (rule 7).

### Rule 9 controls — proving the greens are not vacuous

Six of the eight greens are bounded assertions, so "green at base" is only
evidence if the *broken* build would be red. Two temporary control tests
were run at base and **removed before the commit** (they are not in the
committed file). Each deliberately asserts the opposite of the truth, so a
failure is the proof:

```
$ cargo test --lib core::connection::tests_ack_cadence::rule9_controls

---- control_no_return_path_means_no_recovery stdout ----
assertion `left == right` failed: CONTROL: expected SHORT, got full
  left: 0
 right: 8192

---- control_no_return_path_means_no_rtt_sample stdout ----
CONTROL: expected NO sample

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured
```

* With B's ACKs discarded, B reassembles **0** of 8 192 bytes — not 8 191,
  **zero**, because the dropped packet is offset 0 and nothing behind a hole
  is readable. So test 7's `got.len() == payload.len()` is carried
  *entirely* by the ACK-driven retransmission; there is no path by which it
  passes without §13.2 firing.
* With B's ACKs discarded, A's estimator has **no sample at all**. So test
  8's `has_sample()` is not true by default and is not seeded true by
  `K_INITIAL_RTT`.

## Derivations

### The ratified text I am pinning against (SPEC.md §12.4, lines 4611–4647)

Verbatim, at base `99998d5`:

> ### 12.4 Delayed-ACK policy
>
> **[RATIFIED 2026/08/14]** The old immediate-ACK-per-packet policy is replaced by
> QUIC's default; congestion control now consumes ACK timing, and streams
> make 1:1 ACK traffic a real reverse-path cost, while 25 ms is already the
> PTO formula's assumption — the change is self-consistent. The cost is one
> more named timer and slightly laggier RTT samples; immediate-ACK remains
> the conservative fallback if the Appendix B timing obligations disappoint.
>
> | Constant | Value |
> |---|---|
> | `MAX_ACK_DELAY` | 25 ms |
>
> - An ACK is owed after every **2nd** ack-eliciting packet, or when the
>   `AckDelay` timer (armed at `MAX_ACK_DELAY` on receipt of the first
>   unacknowledged ack-eliciting packet) fires — whichever first.
> - An ACK is owed **immediately** on out-of-order arrival: an ack-eliciting
>   packet whose counter is not exactly one greater than the window's
>   previous greatest (it opens, fills, or sits inside a gap). The first
>   ack-eliciting packet of a session has no previous greatest, so the rule
>   applies vacuously and yields an immediate ACK — harmless, and it seeds
>   the peer's RTT estimate early.
> - An owed ACK rides the next outgoing packet (packing order §8.5); if none
>   is pending, a standalone ACK packet is generated. Pure-ACK packets are
>   sealed `seal_quiet` (§7.4), are not ack-eliciting (no ACK-of-ACK loops),
>   are never tracked for loss, and bypass the congestion window (§14.5).

Ruling 271 (the direction in my brief; **not yet present in
`.spec-v2-clean-slate/rulings.md` at base — `grep -n '271'` returns nothing
there**) replaces the first bullet's every-2nd trigger with per-drain
coalescing: a burst of N ack-eliciting datagrams processed in one
receive-drain produces **at most one** ACK emission. The second and third
bullets, `MAX_ACK_DELAY` as outer bound, ruling 33's liveness-neutrality,
and all wire formats are unchanged.

Note for the record (rule 11 — a citation is a claim about the cited text):
§12.4's own ratification note already reserved the revisit — *"immediate-ACK
remains the conservative fallback if the Appendix B timing obligations
disappoint."* Ruling 271 moves in the other direction (fewer ACKs, not more),
so that sentence is **not** its warrant; the warrant is round42-G's measured
ratio. I flag this because the amendment that lands should probably rewrite
that sentence too — it argues a fallback direction the slice does not take
(rule 4a: the other clauses of the sentence being edited).

## Conflicts

Reported, not resolved (working rule 3).

### C1 — §12.4-as-amended ("per-drain") vs §16.4's every-call-drain contract

`SPEC.md` §16.4:5998–6009, verbatim:

> the str0m
> single-`poll_output` contract: **every mutating call** (`handle_datagram`,
> `handle_timeout`, verb calls, stream/datagram/message operations,
> `connect`) **is followed by draining `poll_output()` to the terminal
> `Timeout(Option<Instant>)`**, which is simultaneously the drain sentinel
> […] and the next-deadline announcement — a driver cannot forget to drain.

Ruling 271's unit of coalescing is *the receive-drain*: "a burst of N
ack-eliciting datagrams processed in one receive-drain produces at most one
ACK emission". §16.4 says there is **no such thing as one drain over N
`handle_datagram` calls** — the contract pairs one drain with each mutating
call. Taken literally, the two cannot both hold: under §16.4 a burst of N
datagrams is N drains, and per-drain coalescing degenerates to
one-ACK-per-packet, which is the *old-old* immediate policy §12.4 replaced.

The fixture agrees with §16.4, not with ruling 271. `testfix.rs:1170`
`Solo::deliver_from` is:

```rust
let dgram = self.peer.seal(frames);
self.conn.handle_datagram(now, src, &dgram);
drain(&mut self.conn)
```

— seal, one `handle_datagram`, one `drain`. Every `Solo`/`Pair` delivery
helper in the file has that shape. So the *fixture* cannot express "N
datagrams, one drain" through its own vocabulary either; it can only be
expressed by calling the bare `conn.handle_datagram(...)` N times and
`drain(...)` once, which is precisely the sequence §16.4 forbids a driver
from performing.

**What I did about it (and did not decide).** I wrote the cadence test both
ways, so that whichever reading the ratification settles on, one of them is
the pin and the other is documented evidence:

- **Test 1, the receive-batch reading**: N datagrams delivered at one
  instant, each `handle_datagram` followed by its own `drain` exactly as
  §16.4 requires, counting ACK emissions across the whole batch. "One
  drain" is read as *one driver turn* — the N datagrams the socket had
  queued, sharing §16.5's once-per-turn `now`. Honours §16.4 at every call.
- **Test 2, the literal reading**: N bare `handle_datagram` calls before a
  single `poll_output` loop — obtained by *not* honouring §16.4 between the
  calls, which is why no fixture helper does it and no conforming driver
  can.

If ratification means the first, §16.4's sentence needs no amendment and
the core is coalescing across *`now` identity* rather than across calls; if
it means the second, §16.4's "every mutating call is followed by draining"
acquires an exception it does not currently state, and that is a
contract-level amendment, not an implementation detail. **I do not pick.**

**And the measurement narrows it usefully: both readings are red at base,
both at exactly 10.** So the conflict does not block the implementer — no
fix can satisfy one reading and not the other from where the code stands,
because §16.7 seals inside the mutating call and there is nothing left in
the drain to coalesce (see "The finding inside test 2" above). It blocks
only the §12.4 amendment text.

**A third reading nobody has stated, offered as a question rather than an
answer.** Neither test can distinguish "at most one ACK per *batch*" from
"at most one ACK per *distinct `now`*". They coincide in every test here
because a batch shares one instant by construction. They diverge for a
driver that takes two socket reads inside one turn, or one that re-reads
`now` mid-batch — and §16.5's once-per-turn `now` is a *convention* the
spec states for the driver, not an invariant the core can check. If the
amendment says "per drain" and the implementation keys off `now`, the two
agree until the day the driver changes. Worth one sentence in the ruling.

**What is not in doubt:** the story-level shape — N datagrams arriving in
one driver turn producing one ACK — is exactly what round 42-G measured the
absence of on a real socket, at 1 ACK per 2 data datagrams against quinn's
1 per 58.6. Whatever the wording lands on, that measurement is the
acceptance criterion, and it lives above the core seam.

### C2 — §12.4's surviving second bullet is stated relative to a trigger the amendment removes

§12.4's immediate-on-gap bullet reads (verbatim, 4627–4632):

> - An ACK is owed **immediately** on out-of-order arrival: an ack-eliciting
>   packet whose counter is not exactly one greater than the window's
>   previous greatest (it opens, fills, or sits inside a gap). The first
>   ack-eliciting packet of a session has no previous greatest, so the rule
>   applies vacuously and yields an immediate ACK — harmless, and it seeds
>   the peer's RTT estimate early.

My brief holds this bullet exactly as ratified while the first bullet's
every-2nd trigger goes away. But "immediately" was previously *distinguishable*
from the every-2nd trigger only in that it fired sooner within the same drain.
Under per-drain coalescing, "an immediate ACK **mid-drain**" (the brief's
words for test 3) is the **only** thing that can produce two ACK emissions
out of one drain — i.e. the gap rule is now the sole exception to "at most
one per drain", and the two clauses are in direct tension about the word
*at most*. Rule 4(a): this is the other clause of the sentence being edited.
Someone has to state whether a drain containing a gap may emit two ACKs
(gap-immediate + end-of-drain) or exactly one. **My test 3 asserts only that
at least one ACK carrying the gapped ranges leaves in the same drain, and
deliberately does not bound the count** — because bounding it would be me
resolving C2.

### C3 — §12.4's ratification note argues the wrong fallback direction

§12.4:4613–4618 says *"immediate-ACK remains the conservative fallback if the
Appendix B timing obligations disappoint."* Ruling 271 moves the other way
(fewer ACKs). The note is not wrong about the old change, but it is the
sentence a reader consults when asking "what happens if delayed ACKs hurt?",
and after 271 the answer is no longer "we go back to immediate". Flagged
under rule 4 — grep for the rationale, not only the token.

## Notes for the integrator

1. **The `mod` declaration is yours** (working rule 6 as amended by ruling
   211, and rule 15). Add, under an integration header:

   ```rust
   #[cfg(test)]
   mod tests_ack_cadence;
   ```

   next to `mod tests_park;` in `src/core/connection/mod.rs`. I used exactly
   that line **temporarily** to run the gates above and reverted it before
   committing; `git status` on my commit shows only
   `src/core/connection/tests_ack_cadence.rs` and this report. This is the
   R42 author's precedent and slice 7's, done deliberately rather than by
   accident.

2. **`testfix.rs` did not need extending.** Every frame these tests build
   (`PING`) and every frame they decode (`ACK`) is already in `Wire` and
   `parse_frames`. This is the first slice in a while where the fixture had
   not aged out — worth recording, since the header of that file now
   documents four occasions when it had.

3. **Two reds are expected and are the deliverable.** If the implementer's
   change makes tests 1 and 2 green and leaves 3–10 green, the slice is
   done. If it makes 1 and 2 green by making any of 3–10 red, it has bought
   the cadence with the peer's recovery, which is the failure this blind
   split exists to catch. Tests 3, 4, 6, 7 and 8 are the tripwires, in that
   order of likelihood.

4. **Do not weaken test 6's block equality to a "contains" check.** It is
   the only assertion in the file that separates the single most likely
   wrong fix — collapsing a coalesced ACK to `largest..=largest`.

5. **Test 5 does not bound the ACK count on purpose.** See §C2. If
   ratification decides that a gap may produce a second emission from one
   batch, nothing here changes; if it decides it may not, test 5 should gain
   an upper bound and that is a deliberate edit by whoever holds the ruling,
   not an oversight to be swept.

## Fixture seams used (surveyed at base, nothing modified)

| Seam | Path | What it gives me |
|---|---|---|
| `Solo` / `Solo::deliver(now, frames)` | `src/core/connection/testfix.rs:1086,1154` | one core + a real sealing peer; seal → `handle_datagram` → `drain` |
| `Solo::packets(&Drained)` | `testfix.rs:1183` | each emitted datagram decoded to `Vec<Wire>` — exact ACK detection, not a size heuristic |
| `Wire::Ack { largest, ack_delay, ranges, first_range }` | `testfix.rs:110` | the ranges assertion test 4 needs |
| `Pair` | `testfix.rs:589` | two real cores + the wire between them |
| `Connection::timer(TimerKind::AckDelay)` | `mod.rs:437`, `timers.rs:45` | the `MAX_ACK_DELAY` deadline, read-only |
| `Connection::recovery().rtt()` | `mod.rs:1305`, `recovery.rs:446` | `min_rtt()`, `smoothed_rtt()`, `has_sample()`, `loss_delay()`, `pto_interval()` |
| `Connection::liveness()` | used at `tests_livelock.rs:254` | `last_send()`, `is_armed()`, `owes_passive_keepalive()` — §7.5's S/R accounting |
| `Connection::bytes_in_flight()` | `mod.rs:1288` | §13.5's map, for "an ACK is never tracked" |
| `FlakyWire` / `FlakyPolicy` / `Tap` | `src/testutil/mod.rs:216,422,608` | story-level burst + census, `drop_at` for deterministic loss |

**Incidental exposure, declared.** Grepping for the `TimerKind::AckDelay`
accessor put `mod.rs:1309`'s `fold_ack_policy` doc comment and its first
~30 lines on my screen (it dispatches to `self.ack.on_recv(...)`). I stopped
there and read no further into `ack.rs`, `recv.rs`, `send.rs` or
`src/shell/driver.rs`. Nothing below is written from it — every assertion
traces to a quoted §12.4/§13 clause in the Derivations section. Declaring it
because a blind author who quietly saw part of the subject is worth less
than one who says which part.

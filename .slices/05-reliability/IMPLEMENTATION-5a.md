# Slice 5a — implementation report

Implementer agent. Base commit `22a9a2a`, branch `main`, main working tree.

Scope: §12's ACK (fused to §7.2's replay window, delayed-ACK policy), §13's
RTT/loss/PTO, §14's NewReno, and the sent-packet map they share. Not
`acked()`, not ruling 128's post-death drain — both 5b's.

## 1. Reading log

### Inputs read (in order)
1. `CLAUDE.md` — 15 binding working rules.
2. `.slices/05-reliability/CONTRACT-5a.md` — whole, 636 lines. Binding.
3. `SPEC.md` §12–§14, lines 3553–3952, read in full. Nothing else of
   `SPEC.md` was read whole (working rule 1).
4. `.slices/05-reliability/PLAN-5.md` §6–§11 (lines 894–1580) — the five
   debts, the story/test map, the unstated-scope hunt, the conflicts, the
   open questions, the risk register.
5. `rulings.md` Round 23 (lines 3332–3565), rulings 130–140 verbatim.
6. Every file I may edit, plus `testfix.rs` and `tests_streams.rs`
   read-only.

### Key section anchors (post-Round-23 line numbers)
- §12 ACK: SPEC.md:3553–3641.
- §13 recovery: SPEC.md:3642–3789.
- §14 congestion: SPEC.md:3791–3951.

### Mid-slice corrections from the maintainer, all applied
1. **Ruling 139(f) reversed** — §13.2's time threshold is `>=`, not `>`.
   Applied; and it was worse than stated (§4-C0).
2. **`SentPacket` carries `path_gen`** — present since first build; renamed
   from my guessed `path_generation` to the maintainer's `path_gen`.
3. **`Recovery::on_sent`'s stated assertion must not be implemented** —
   already shipped that way, for the reason the correction gives (§5-M1).
4. **Keep §14.4's `has_sample()` guard** — kept. I could not confirm the
   accompanying vacuity claim; see §4-C5.

---

## 2. What I built

### New — `src/core/connection/ack.rs` (§12)
`AckState` (§12.4's four scalars beside the window, never a second record),
`AckAction`, and `derive()` — §12.2's newest-first descending derivation off
`ReplayWindow`, truncating at `MAX_ACK_RANGES` pairs **or** at `room`
plaintext bytes, whichever binds. The truncation re-measures after each
candidate `(gap, range)` pair, because a pair's own varint width depends on
its values.

### New — `src/core/connection/recovery.rs` (§13)
`SentPacket` (with ruling 137's `path_gen`, held at 0 and `debug_assert`ed
so), `SentFrame` (§8.7's three classes, with ruling 113's `fin` **carried**),
`Recovery` (the `BTreeMap` sent map, §12.5's bounded intersecting
processing, §13.2's walk, §13.3's PTO, §14.4's persistent-congestion verdict
computed *inside* the walk), `AckOutcome`, `CongestionEvent`, and
`RttEstimator` (§13.1, integer arithmetic only).

`PTO_MAX_EXPONENT` is derived as `PTO_BACKOFF_CAP.trailing_zeros()` with a
`const` assertion, never transcribed — §4.3 names `1u32 << pto_count.min(64)`
as the slice's single most likely mechanical error.

### New — `src/core/connection/congestion.rs` (§14)
`Controller` (§14.1's trait, `pub(crate)` deliberately per ruling 139(d), and
its rustdoc says so) and `NewReno` (§14.2 slow start / integer ABC,
§14.3's one-cut-per-episode recovery period with the symmetric growth fence,
§14.4's collapse, §14.6's uncalled roam `reset`).

### Wiring — `mod.rs`
- `handle_datagram` captures `prev_greatest` **before** `Session::open`
  marks the window, then folds §12.4's policy once per window-fresh packet
  (`fold_ack_policy`).
- `pump` became `pump_inner(now, probe)` over §8.5's four stages: ACK,
  control, fill, PING. §14.5's gate sits after the plaintext is packed and
  before the seal; a refused packet is **restored** and, if an ACK was owed,
  a standalone pure-ACK packet goes out ungated.
- The seal's `ack_eliciting` argument is now computed rather than hardcoded
  `true` — a pure-ACK packet is not ack-eliciting (§12.4), so it must not
  arm §7.4's death deadline.
- `Frame::Ack` is processed (§12.5); `handle_timeout` gained `Loss`, `Pto`
  and `AckDelay` arms; `drop_state` clears recovery, controller and ACK
  state; `sync_recovery_timers` re-derives both §13 deadlines from the map.
- `ack_snapshot` / `snapshot_settled` (§16.2, ruling 47's core half) and the
  three `#[cfg(test)]` accessors.

### Supporting edits
- `streams.rs`: `Packed` — the record of what one packing pass took, which
  serves **both** §13.5's sent-map entry and §14.5's undo log; `restore`;
  `on_reset_lost` / `owe_max_data` / `owe_max_stream_data` /
  `owe_max_streams` for §8.7's `regenerate` class on loss; `send_offsets` /
  `send_settled` for §16.2.
- `send.rs`: `settled_to`; ruling 134's correction to `write`'s doc comment
  (§4-C3).
- `frame.rs`: `Ack::encoded_len`.
- `flow.rs`: `send_room`'s doc now states that §14.5's gate is **not** part
  of it (contract §4.8).
- `session.rs` and `timers.rs` **needed no change** and were not touched.

### Design note — why `Packed` exists
§14.5's gate needs `candidate_size`, which is the full datagram (ruling
136) and therefore unknowable until the plaintext exists; the contract
requires a refused packet to leave its frames pending. Building the packet
is destructive (chunks leave the pending sets, regenerate identities are
cleared), so the undo needs a record. That record is *exactly* §13.5's
"frame identities aboard", so one structure serves both. Deriving the sent
record from the packed `Frame`s instead loses two things: a `StreamId` does
not name a `StreamRef`, and `Chunk::fresh` (ruling 98's marking test) is not
recoverable from a `Frame::Stream` at all.

Restored chunks go back through `SendHalf::return_chunk`, **not**
`on_lost_range`: a chunk the window refused was never transmitted, so the
loss path would reclassify a first transmission as a retransmission and
quiet a seal ruling 98 makes marking.

---

## 3. Deviations from the contract, and why

| # | Contract says | What I did | Why |
|---|---|---|---|
| D1 | `Recovery::on_sent` *"Asserts `!frames.is_empty() \|\| probe`"* | No such assertion | The signature has no `probe`. Confirmed by the maintainer mid-slice. See §5-M1. |
| D2 | `NewReno` §4.6's event block: four assignments | Exactly those four; I did **not** also zero `acked_accum` | §4.6 reads as an exhaustive assignment list. A stale accumulator across a cut is arguably wrong, but that is a ruling, not an implementer's call. |
| D3 | `AckAction::None` — *"Nothing owed, nothing armed"* | Implemented as **"no change to make"**, rustdoc'd as such | A non-ack-eliciting packet arriving while `AckDelay` is armed for an earlier one returns `None`; treating that as "nothing armed" would **disarm** the timer and lose the delayed ACK. §12.4 arms on the first *unacknowledged ack-eliciting* packet, and a keepalive is neither. |
| D4 | `Streams::pack_control(flow, packing)` / `fill(packing)` | Both take `&mut Packed` | Required by §14.5's undo and §13.5's record. Additive; the only non-test call site is `pump`. |
| D5 | `drop_state` clears "the map, the estimator and the controller" | Done — and `streams`/`flow` deliberately **not** freed | Ruling 133 splits them by path, and the *retaining* half is what makes 5b's ruling-128 drain implementable. See §4-C4. |
| D6 | three accessors `#[cfg(test)]`, prose says "the tests **and the shell** need" | `#[cfg(test)]`, as written | Followed the code, flagged the prose. If 5b's shell needs `bytes_in_flight`, the attribute has to come off. |

Additions beyond the contract (all additive, none replace anything):
`Recovery::pto_count()`, `Recovery::largest_acked()`,
`RttEstimator::latest_rtt()`, `RttEstimator::min_rtt()`,
`Connection::recovery()`.

---

## 4. Conflicts found

**C0 — the reversal of ruling 139(f) is right, and the consequence is worse
than "a wasted wakeup".** Agreeing rather than merely complying, with the
extra finding: under this core the re-arm goes through
`sync_recovery_timers`, so after a `Loss` firing that declares nothing,
`poll_output` announces a deadline **at or before `now`**. The shell
schedules an immediate wakeup, `handle_timeout` runs, `take_due` fires
`Loss` again, the walk declares nothing again, and the pair spins. With `>`
that is not a rare boundary — it is every `Loss` firing, so a lossy
connection would livelock its driver rather than merely waste a timer. `>=`
is implemented, and both the code and the rationale beside it were rewritten
(working rule 4: the prose that argued for `>` is gone, not just the token).

**C1 — PLAN §6.5's exit check cannot go green from the gate alone, and the
reason is not the gate's placement.** PLAN §6.5 says
`credit_frames_precede_the_stream_fill_in_a_packet` *"should be un-ignored
in 5a … and it should go green. If it does not, that is a finding about the
gate's placement, not about the test."* I measured it: with the `#[ignore]`
removed the test fails **its own "asserted nothing" guard**, emitting
`[[MaxStreamData], [Stream…] × 9]`.

The gate is working — 9 stream packets ≈ 10 835 B admitted against a 12 000 B
window, the 10th refused, and 32 KiB accepted by `write()` with ~21 KiB left
pending. What the test lacks is the *coincidence*. Its `read_exactly` seals
the MAX_STREAM_DATA during the read's own pump, strictly before any stream
data exists.

Reversing the two blocks does not fix it either, and this is the part worth
a ruling: **"stream data is pending" implies "the window is full", and a
credit frame is ack-eliciting**, so §14.5's exhaustive exemption list (PTO
probes, the contested probe, non-ack-eliciting control packets) does not
cover it and the credit packet is refused too. The coincidence therefore
requires an **acknowledgement re-opening the window while both are owed** —
which this test has no peer to produce. It is a finding about the *test's
setup*, not about the gate. I did not modify it; the `#[ignore]` is
untouched.

**C2 — `SendStream::id()` answers `None` once a stream is fully
acknowledged, and slice 5 is the first slice where that is reachable on the
sender's side.** This is the cause of both integration reds; full workings
and a decisive experiment in §7-C. Real defect, one-line fix, in 5b's file.

**C3 — `send.rs:213`'s doc comment predicted the design ruling 134
rejected.** It read *"slice 5's congestion window inserts a second bound
there and not here"*, describing a window-aware `write()`. Ruling 134 makes
the window invisible to `write()`. Corrected in place, since `send.rs` is
mine and the comment is now actively misleading. Ruling 140 corrected the
*section numbers* in this file but not this claim — the same file, the same
comment, a different error.

**C4 — ruling 133 is stated as slice 5's but is entangled with 5b's.** Its
draining-path half (retain `streams`/`flow` for `CLOSE_LINGER`) is what
ruling 128's drain rests on, and ruling 128 is explicitly 5b's. Its
local-close half is independent but its trigger is ambiguous: `enter_closing`
serves both `close()` and `kill()` (a protocol violation), and ruling 133's
rationale — *"at the closer the application signalled that it is done"* —
distinguishes them, while the code path does not. I implemented neither half
and left `drop_state`'s comment saying so. **Reporting, not resolving.**

**C5 — §14.4's `has_sample()` guard is *nearly* vacuous in slice 5, but I
believe not vacuous, so I did not write a comment claiming it cannot be
false.** The correction says every path into the loss walk has already taken
an RTT sample. The walk needs `largest_acked`, so an ACK must have arrived —
but §13.1 takes a sample only when the ACK's `largest` is itself newly
acknowledged. When our highest counter is a packet §13.5 never inserted —
**a standalone pure ACK, which §12.4 emits whenever one is owed and nothing
else is pending** — the peer's `largest` names it, it is not in the map,
`sample_from` is `None`, no sample is taken, and the walk still runs on the
counters underneath it. That is ordinary bidirectional traffic, not a
contrivance.

I have verified this by reading the code path in `Recovery::on_ack`, not by
building a test that exhibits it, so it is a *narrowing* of the vacuity claim
rather than a proof. It changes nothing about the instruction — the guard is
implemented — but the comment beside it now describes the reachable case
instead of asserting it cannot happen. Working rule 11: a comment that names
a mechanism the code contradicts is the thing this project keeps paying for.

---

## 5. Mechanisms named that do not exist

**M1 — `CONTRACT-5a.md` §2.2, `Recovery::on_sent`: *"Asserts
`!frames.is_empty() || probe`"*.** There is no `probe` in scope: the
signature is `on_sent(&mut self, packet: SentPacket)` and `SentPacket` has
no such field. Not implemented. Independently confirmed by the maintainer
mid-slice, with the sharper consequence: keeping only the evaluable half
would panic on **every** §13.4 bare-PING probe, whose frame vector is
legitimately empty — and §2.2's own `frames` doc, four lines above, says so.

**M2 — `Recovery::on_roam(&mut self, now: Instant)` has no use for `now`.**
The contract's description is *"Keeps the map; re-seeds `min_rtt` on the next
sample"*, and neither is a function of the instant; §14.6's roam instant goes
to `NewReno::reset`, which has its own `now`. Implemented with the parameter
present and discarded, to keep the signature blind authors compile against.

---

## 6. Fixture capabilities needed and not addable

**F-1 — `testfix::parse_frames` panics on `FRAME_ACK`.** Confirmed by the
maintainer mid-slice as the integrator's, per working rule 15. Counts and
the exact patch are in §7-A. I applied the patch, measured, and **reverted
it**; `testfix.rs` is byte-identical to its committed state.

**F-2 — `testfix::Pair::pump` fixes `now`, so a delayed ACK never fires.**
`Pair::pump` loops calling `self.a.handle_timeout(now)` / `b.handle_timeout(now)`
at one instant. §12.4 arms `AckDelay` at `now + MAX_ACK_DELAY`, which is
never due at `now`, so **one packet per transfer stays unacknowledged after
`pump` returns**. Verified by reading the code and independently by
construction: my own seam test could not settle until it called
`b.handle_timeout(now + MAX_ACK_DELAY)` explicitly.

This is the aging-fixture class asked about, and it is *quieter* than F-1
because nothing panics — a 5b test that does `pair.pump(t)` and then asserts
`acked()` resolved will simply hang or read short, and the failure will look
like a defect in `acked()`. Suggested fix: when both queues are empty but a
timer is armed, have `pump` advance to the cores' announced deadline
(`Drained::deadline`) rather than re-using one instant.

**F-3 — packet-*shape* assertions age out even after F-1's patch.** §8.5
puts the ACK at stage 1, so it now precedes every other frame in a packet.
Any assertion of the form "this packet's frames equal `[X]`" or "the first
frame is `X`" is now wrong by one element. This is the *second* layer of
`a_max_streams_only_packet_does_not_defer_the_keepalive`'s failure: with the
ACK arm added it stops panicking and fails its `assert_eq!` instead.

**F-4 — transmit-*count* assertions age out.** §12.4 generates standalone
ACK packets where none is pending. `tests.rs:1616`'s "owes no output" helper
is exactly this shape and accounts for two of the four unit reds.

**F-5 (latent, for 5b) — `FlakyPolicy::drop_at` / `drop_first` indices
shift.** `testutil/mod.rs` documents these as absolute per-wire send counters
that `set_policy` does not reset. Slice 5 interleaves standalone ACK packets
into that same counter, so an index computed against a slice-4 packet
sequence now names a different packet. No current test is affected —
`story_streams.rs:336` says in terms that it uses `block_path` rather than a
`drop_at` index — so this is a hazard for 5b's new story tests rather than a
red today. PLAN §11-F3 flags the *handshake* offset of this same counter;
slice 5 adds a second shift on top of it.

**F-6 (mine, fixed) — `mod.rs`'s `smoke::deliver` drops the receiver's
datagrams.** It drains the receiver for its *events* and discards its
transmits, so an ACK can never travel back. Same class as F-2, in a file I
own; I added `smoke::exchange`, a round-trip helper, rather than changing
`deliver`.

---

## 7. Gate output

Baseline at `22a9a2a`: **669 passing, 2 ignored, 0 failing.**
This commit: **669 passing, 2 ignored, 6 failing**, with **6 new tests
added** (the seam smoke tests in `mod.rs`). Every one of the 6 failures is a
**pre-existing** test; none of my 6 is among them.

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.12s

$ cargo fmt --all --check
(no output)

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.09s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
 Documenting slither v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.53s

$ cargo +1.96 check --all-features --all-targets     # MSRV
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.26s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

`cargo test --all-features --no-fail-fast` (debug) — per binary:

```
unittests src/lib.rs        FAILED. 489 passed;  4 failed; 1 ignored
tests/spec_constants.rs     ok.     103 passed;  0 failed
tests/spec_errors.rs        ok.      11 passed;  0 failed
tests/spec_packet.rs        ok.       4 passed;  0 failed
tests/spec_shell.rs         ok.      12 passed;  0 failed
tests/spec_streams.rs       FAILED.  18 passed;  1 failed; 1 ignored
tests/story_dial.rs         ok.       4 passed;  0 failed
tests/story_lifecycle.rs    ok.      16 passed;  0 failed
tests/story_streams.rs      FAILED.   5 passed;  1 failed
Doc-tests slither           ok.       7 passed;  0 failed
```

`--lib` alone is the first line: **489 passed, 4 failed, 1 ignored.**

`cargo test --release --all-features --no-fail-fast` — **identical**:
489/4, 18/1, 5/1, everything else green. No debug-only or release-only
divergence.

### A. The aged-out fixture — 1 test (integrator's)

- `core::connection::tests_streams::sealing::a_max_streams_only_packet_does_not_defer_the_keepalive`

Panics at `testfix.rs:184`: `slice 4 emitted frame type 0x2, which §8.3 does
not place in this slice`.

**Measured, then reverted.** With `Wire::Ack { largest, ack_delay,
first_range, ranges }` added and a `FRAME_ACK` arm in `parse_frames`, the
whole suite becomes **489 passed / 4 failed** — the panic disappears and
*this same test* then fails its content assertion instead, because §8.5 now
puts the ACK ahead of the MAX_STREAMS frame it asserts on (F-3). So the
parse arm is necessary and **not sufficient**; this test needs its expected
frame list widened as well.

**Only one test hits the parse panic**, not the ~37 estimated. The other
call sites decode drains that happen not to contain an ACK.

### B. Slice-boundary assertions slice 5 is defined to invalidate — 3 tests

- `core::connection::tests::codec::a_ping_is_accepted_and_answered_with_nothing_in_slice_3a`
- `core::connection::tests::codec::padding_may_appear_anywhere_around_other_frames`
- `core::connection::tests_streams::slice_boundary::a_send_half_never_reports_finished_because_slice_four_has_no_acks`

All three assert the *absence* of slice 5 and say so in their own text ("in
slice 3a", "§12 is slice 5 — a build firing this has freed on send"). The
first two now see a 35-byte pure-ACK packet, which is §12.4 working; the
third now sees `StreamFinished`, which is §12.5 → §9.7 working.

**Coverage note the integrator should not lose:** the third test is the only
place in 4a where the *"freed the send half on send instead of on
acknowledgement"* mutation was visible. Once ACKs exist it can no longer
separate those two builds, and the obligation moves to `tests_recovery.rs`'s
R-19 / T-FIN-A pair. Deleting it without that replacement in place would
silently drop the mutation.

### C. A genuine finding — ruling 116's id cache is lazy — 2 tests

- `tests/spec_streams.rs::a_cancelled_accept_claims_nothing`
- `tests/story_streams.rs::s13_a_stalled_stream_does_not_block_a_concurrent_one`

Both fail comparing a *sender's* `id()` against a receiver's, with the
**sender's** side `None`:

```
assertion `left == right` failed: ruling 112: FIFO by open order
  left: Some(StreamId(2))
 right: None
```

Not a FIFO defect. `Streams::after_half_freed` does `self.entries.remove(&r)`
when a stream is fully closed, and `Streams::stream_id` reads `entries`, so
`stream_id` answers `None` once the stream is retired. Slice 5 makes a
**locally-opened** stream retire for the first time — that is
`rulings.md:2620–2634`'s "a locally-opened stream fully closes only on
acknowledgement", arriving as designed.

`src/shell/stream.rs`'s `cached_id` (**ruling 116**) already anticipates
this exactly; its doc comment says `stream_id` *"is not monotone … an
uncached `id()` answers `None` again once a stream fully closes"*. But the
cache fills **lazily, on first sight**, and both tests call `id()` for the
first time *after* `finish()` + the peer's ACK — so the cache is empty at
the moment it is needed.

**Verified decisively**, not inferred: inserting `let (probe1, probe2) =
(s1.id(), s2.id());` before `settle()` in `a_cancelled_accept_claims_nothing`
makes it pass (`test result: ok. 1 passed`). The experiment was reverted;
`tests/` is byte-identical to its committed state.

Ruling 116's mechanism is sound and its *scope* is short — working rule 8's
shape again. The fix is one line in `src/shell/stream.rs`, which is **5b's
file**, so I have not made it: fill the cache eagerly when the handle is
constructed, keeping the lazy path for §16.9's pre-establishment case where
`stream_id` is legitimately `None`. That closes both tests and every stream
opened after establishment. A residual hole remains for a stream opened
*before* establishment whose `id()` is never read before it is fully
acknowledged; closing that needs the core to retain the id past retirement,
which is a ruling, not an implementation choice.

---

## 8. Guesses

1. **`fold_ack_policy` is skipped once the connection is dying.** The
   contract says `on_recv` runs "once per window-fresh packet" without
   qualification. Arming `AckDelay` on a draining connection would announce a
   deadline that fires with nothing to pack (§15.2 emits only CLOSE), so I
   gated it on `is_live()`.
2. **A structurally-broken packet (§8.2) counts as frame-bearing but not
   ack-eliciting** for §12.4's purposes. Nothing from it is applied and the
   connection is closing, so the choice is unobservable; stated because it is
   a choice.
3. **The PTO probe is planned in the `Pto` arm and *sealed* by the
   end-of-`handle_timeout` pump.** Ruling 76 requires `AckDelay` to fire
   *after* the loss/PTO evaluation *"so the owed ACK rides any probe or
   retransmission that evaluation produced"* — which is impossible if the
   probe is sealed inside the `Pto` arm, since `AckDelay` sets its flag
   afterwards. Planning it and sealing it later **in the same mutating call**
   satisfies §16.7 (nothing is deferred to `poll_output`) and makes ruling
   76's stated consequence actually happen. Ruling 114 bounds *where*
   sealing happens, not *when within the call*.
4. **A PTO probe on a connection whose bytes are all `unacked` is a bare
   PING.** §13.4's "pending retransmittable frames … if any exist" reads on
   the *pending* sets; transmitted-but-unacknowledged bytes are not pending
   until §13.2 declares them lost. This is self-consistent — the PING elicits
   an ACK whose `largest` lifts `largest_acked` above the lost counters, and
   §13.2's packet threshold does the rest — and there is a smoke test driving
   exactly that path end to end. Stated because "else a bare PING" could be
   read the other way.
5. **`AckOutcome.ack_events` stayed the contract's bare tuple**
   `Vec<(Instant, u64, bool)>`. I wrote a named-struct version first and
   reverted it: two blind authors compile against the contract's text, and
   readability is not worth a compile break.

The `path_gen` field name is no longer a guess — the maintainer named it
mid-slice and it is renamed to match.

---

## 9. Seam smoke tests added (6, in `mod.rs`'s existing `smoke` module)

Not acceptance tests — those are the two blind authors'. These exist because
the *junction* of the ACK derivation, the sent map and the transmit pump has
no other exercise, which is the same reason the module's four slice-4 tests
exist.

| test | what it pins |
|---|---|
| `an_ack_drains_the_sent_map_and_completes_the_send_half` | the whole §12.4 → §12.2 → §12.5 → §9.7 loop, plus the delayed ACK carrying the odd packet out |
| `the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer` | §12.4's two triggers are distinguishable; an immediate-ACK build fails the middle assertion *[Superseded 2026/08/18 by ruling 271: the emission point this row argues moved to the receive-drain boundary and the test is now `the_first_in_order_packet_arms_the_timer_and_the_second_makes_it_due_now`; the due trigger is unchanged.]* |
| `the_pto_is_armed_only_while_something_is_in_flight` | §13.3's precondition, from both sides |
| `a_firing_pto_probes_a_flight_loss_detection_cannot_yet_judge` | §13.4's probe rescuing a flight with `largest_acked == None`, and ruling 43's track-but-do-not-gate |
| `the_window_defers_the_seal_and_never_the_acceptance` | ruling 134 and §14.5's gate together, at datagram granularity, with a 32 KiB transfer completing through it |
| `a_snapshot_settles_only_once_its_bytes_are_acknowledged` | §16.2's snapshot is a value, and later writes do not extend it |

---

## 10. Blind authors' test modules

Per the brief I must declare `tests_ack` and `tests_recovery` in `mod.rs`
and **create neither file**. Doing so in the same commit as the
implementation would leave `main` unable to compile, so working rule 7's
"paste the gate output" would be unsatisfiable for this slice.

They are therefore in a **separate follow-up commit**, so that:
- the implementation commit is buildable and its gate output above is real;
- the declaration exists and cannot be forgotten at integration.

The follow-up commit does not compile on its own and says so in its message.
It is the commit the blind authors' files complete.

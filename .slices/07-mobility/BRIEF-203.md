# Brief — ruling 203's sizing defect

**Binding.** This file, plus rulings 168, 170, 203 and 207 in
`.spec-v2-clean-slate/rulings.md`, and spec §7.3 / §7.5. Where this brief
and a ruling disagree, the ruling wins — and say so rather than picking.

## The defect, in one paragraph

§7.5's keepalive is §3.4's empty plaintext: no frames, therefore no ACK,
therefore **not ack-eliciting**. Ruling 168 validates a roamed address
only on **an ACK covering `validation_floor`**. So a connection carried
purely by the passive keepalive dance **never validates the address it
roamed to**. Its §7.3 budget stays armed at `3 ×` the roaming packet — 90
bytes for a 30-byte keepalive, netting ~60 more per round.

`Connection::pump_packets` (`src/core/connection/mod.rs:1811`) builds a
candidate at **full size** — `Packing::new()` starts with
`budget: MAX_PLAINTEXT` — then asks `self.amplification.admits(size)`. When
the answer is no it restores the packed state and `break`s. **Nothing
shrinks to fit.** So 2 048 bytes of application data cannot leave for ~20
keepalive rounds (~200 s), even though a ~90-byte packet would fit
immediately, be ack-eliciting, and validate the address in one round trip.

## The fix, as ruling 203 states it

> Bound the packing target by the remaining budget — `min(MAX_DATAGRAM, room)`
> — so the first post-roam packet is small, ack-eliciting, and validates
> the address at once.

## Three ways to build this wrong (ruling 207)

**(a) The budget predicate does not move.** `Amplification::admits` is
correct. A fix that loosens it, or exempts the first post-roam packet from
it, reopens the reflector §7.3 exists to close. Change what the pump
**builds**, never what the budget **permits**.

**(b) A shrunken packet must still be able to elicit.** Validation arrives
only on an ACK covering `validation_floor`. A packet sized to fit the
budget is useless if what fits is a bare ACK — non-ack-eliciting output
cannot produce the ACK that validates, and the connection stalls exactly
as it does today, one indirection later. **Whether the pump owes a PING
when the admitted room holds nothing ack-eliciting is a genuine question
this pass must answer, not assume.**

**(c) `MAX_PLAINTEXT` is not the only cap that moves.** The candidate's
charged size is the full datagram: `DATA_HEADER_LEN (14) + plaintext +
AEAD_TAG_LEN (16)`, i.e. 30 bytes of overhead (ruling 136). A fix that
caps the *plaintext* at the remaining *datagram* bytes overshoots by 30
and re-refuses its own packet. Ruling 201 already cost a round to a units
error in this area.

## Ground truth you will need

- `Amplification` — `src/core/connection/mobility.rs`. Today it exposes
  `admits(len) -> bool`, `on_sent`, `on_recv`, `on_ack_covering`,
  `is_validated`, `set_floor`, `counters()`. There is **no** accessor for
  the remaining room; if you need one, add it there.
- `Packing::new()` — `src/core/connection/frame.rs:880`, starts at
  `budget: constants::MAX_PLAINTEXT`.
- The pump's refusal site — `src/core/connection/mod.rs:1914`.
- Constants: `MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170, `DATA_HEADER_LEN`
  14, `AEAD_TAG_LEN` 16, `AMPLIFICATION_FACTOR` 3.

## The acceptance test already exists, and is blind

`s18_a_running_keepalive_dance_carries_the_move_by_itself` in
`tests/story_mobility.rs` was written by an agent blind to the
implementation, and is the test ruling 203 was diagnosed from. **Do not
edit it.** It is the acceptance bar: it must pass, unmodified.

## Working rules that bite here

- **Rule 9** — a bound is only a test if the degenerate case violates it.
  "The packet fits" passes a build that sends nothing at all. Assert from
  the side that separates: data *moved*, and moved *promptly*.
- **Rule 3** — if the prose and a formal rule conflict, report it, do not
  resolve it. Nine times in this project the prose held the intent; once
  (ruling 182) the formal statement did. The tiebreak: follow the
  statement some other proof depends on.
- **Rule 5** — if this brief tells you to do something that looks wrong,
  say so rather than doing it.
- **Rule 7** — do not report a gate green without running it. Paste the
  command and its output. Read **both** the passed and the failed column;
  a false-green was reported in this slice by summing only one.
- **Rule 14** — your first command checks your worktree's base commit and
  reports it. A brief that names a commit and tooling that cuts from
  `HEAD` destroy the blind split silently.

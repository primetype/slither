# CONTRACT-7b — binding API and wire contract for the remediation slice

Base commit: `23d0409`. **Binding** on both blind agents: the test author
writes against this file, the implementer implements to it. Where this file
and the reviews disagree, this file wins; where this file and `SPEC.md`
disagree, that is an open question in `PLAN-7b.md`, not a licence to choose.

**Status: awaiting phase 0.** Four open questions in `PLAN-7b.md` block
dispatch — **Q17** (the `SPEC.md` sweep), **Q13** (where the challenge's
bytes come from), **Q18** (§19's conflicting row) and **Q14** (priority and
packing order). Q13 and Q14 change §1 of this file directly. The maintainer
amends this file where a ruling contradicts it, **and commits it, before
either blind agent is cut** — working rule 14, both halves.

---

## 0. Ratified inputs, quoted

These are the fixed points. Nothing below may contradict them.

**Ruling 208** (rulings.md:5817, RATIFIED 2026/08/16):

| Frame | Code | Payload |
|---|---|---|
| `PATH_CHALLENGE` | `0x1a` | 8 opaque bytes |
| `PATH_RESPONSE` | `0x1b` | the same 8 bytes, echoed |

- Both are **ack-eliciting**.
- The challenge is drawn from **the endpoint RNG**, is **per-arming**, and
  is **never reused across armings**.
- Unchanged: the 3× budget, `AMPLIFICATION_FACTOR`, the arming triggers,
  the *held-not-dropped* discipline.
- Ruling 203's sizing fix "stands and becomes more important": **the
  challenge must fit inside the armed budget**, and a pump that cannot
  shrink cannot emit one.
- `validation_floor` and `on_ack_covering` **lose their security role** —
  "replaced, not supplemented, and leaving both in place would give an
  attacker the old path as a bypass."

**Ruling 207** (rulings.md:5780) — three constraints on the 203 pass that
survive verbatim into this slice, because 208 does not touch them:

- (a) **`Amplification::admits` does not move.** The change is to what the
  pump *builds*, never to what the budget *permits*. No exemption for the
  first post-roam packet.
- (b) **A shrunken packet must still be able to elicit.** Under 208 this
  requirement is *satisfied by construction*: `PATH_CHALLENGE` is the
  ack-eliciting thing that fits. 207(b) asked "does the pump owe a PING
  when the admitted room holds nothing ack-eliciting?" — **ruling 208
  answers it: it owes a `PATH_CHALLENGE`.**
- (c) **Two units, differing by exactly the overhead.** `Packing::new`
  starts at `budget: MAX_PLAINTEXT`; the charged size is the full datagram
  (`DATA_HEADER_LEN + plaintext + AEAD_TAG_LEN`, ruling 136). Capping the
  *plaintext* at the remaining *datagram* bytes overshoots by 30 and
  re-refuses its own packet.

---

## 1. Ruling 208 — `PATH_CHALLENGE` / `PATH_RESPONSE`

### 1.1 Frame encoding

Two new constants in `src/constants.rs`, beside the existing `FRAME_*`
block (`constants.rs:166-208`):

```rust
/// `0x1a` — §7.3's return-routability challenge. Ruling 208.
pub const FRAME_PATH_CHALLENGE: u64 = 0x1a;
/// `0x1b` — the echo of `0x1a`'s eight bytes. Ruling 208.
pub const FRAME_PATH_RESPONSE: u64 = 0x1b;
```

**Three existing lists enumerate §8.3's table and all three go stale.**
Working rule 8 reads a list as exhaustive whether or not it says so, and
each of these says so out loud:

1. `constants.rs:670-683`'s `FRAME_*` uniqueness table — **I1's**. Nothing
   fails if it is missed; that is what makes it a defect.
2. `frame.rs`'s inline `ack_eliciting_matches_the_whole_of_table_8_3` —
   its doc says *"table-driven over **all twelve rows**"*, and §8.3 gains
   two, so the prose and the array both move (the array carries the STREAM
   range expanded, so it grows by two entries, not by two rows). **I1's**,
   inline in I1's file. The same doc says a two-arm classifier is *"correct
   by accident"* without this test — which is exactly why the new rows are
   added and the test is not relaxed.
3. `tests/spec_constants.rs:286-288`'s §8.3 registry comment, which lists
   the codes literally, plus its per-constant `assert_eq!` block —
   **T1's**.

Two new `Frame` variants (`src/core/connection/frame.rs:66-89`):

```rust
/// `0x1a` — §7.3's return-routability challenge. Ack-eliciting.
PathChallenge([u8; 8]),
/// `0x1b` — `0x1a`'s eight bytes, echoed. Ack-eliciting.
PathResponse([u8; 8]),
```

Wire layout, both frames: `type(1 byte varint) ‖ 8 opaque bytes`. Fixed
width, no length prefix. Therefore:

| property | value | why |
|---|---|---|
| `type_code()` | `0x1a` / `0x1b` | ruling 208 |
| `encoded_len()` | **9** | `1 + 8`; both codes are < 64 so the varint is one byte (`frame.rs:148-149`) |
| `is_ack_eliciting(ty)` | **`true`** for both | ruling 208, verbatim |
| `extends_to_end()` | `false` | fixed width |
| §8.7 class | **`never`** | see §1.4 |

Parsing (`frame::parse`, `frame.rs:691`): each arm takes exactly 8 bytes
from `cursor.rest()`. **A body shorter than 8 bytes is
`Structural::LengthOverrun`** — the existing variant, not a new one; §8.2
answers every structural failure identically and the variants exist only
for the trace (`frame.rs:630-637`). Do not mint a new variant.

**Boundary tests the author must be able to write** (working rule 9 — the
degenerate case must violate the bound): a 7-byte body is `LengthOverrun`;
an 8-byte body parses; a 9-byte body parses the frame and leaves one byte
for the next frame (which is then an `UnknownType` or a valid frame — the
parse does not "consume the rest"). The one-sided-boundary defect from
slice 1 is the thing to avoid: test 7 **and** 9, not only 7.

### 1.2 Generation — when a challenge comes into being

**One challenge per arming.** The value is drawn at the instant
`Amplification::arm` is called, and at no other instant. There are exactly
two arming sites and ruling 208 does not add or remove any:

1. `Connection::commit_roam` (`mod.rs:~1205`) — a committed roam.
2. `Connection::established` (`mod.rs:~1748-1760`) — the msg1 anchor,
   under `anchor_from_msg1` (ruling 200).

**A fresh 8 bytes on each arming; never reused across armings.** A roam
back to a previously-challenged address draws a new value. Re-arming with
the previous value would let a peer bank a response.

**Source of the bytes — see open question Q13.** The binding requirement
for the implementer is: the connection core draws them from a
`rand_chacha::ChaCha20Rng` seeded from **§16.6's per-connection sub-seed**,
which `Connection` already holds (`mod.rs:148`, `:274`) and which is
currently drawn-but-unused. §16.6 states the sub-seed is *"drawn even while
unused, so later connection-side randomness cannot perturb the endpoint's
draw order"* — this is the connection-side randomness §16.6 was written
for, and *"one root seed reproduces the whole system"* still holds.

Concretely: `Connection` gains a private `rng: ChaCha20Rng` built in
`connecting()` from `sub_seed`. `sub_seed` stays as a field and
`sub_seed()` stays as an accessor — `tests.rs:766`/`:799` pin it and those
tests must keep passing unchanged.

**Determinism is a test requirement, not an accident.** Two endpoints on
the same seeds must produce the same challenge sequence, or the paused-clock
flow tests cannot assert on a specific value.

### 1.3 Storage

`Amplification`'s `floor: u64` is **replaced** by `challenge: [u8; 8]`.
The struct keeps four fields:

```rust
pub(crate) struct Amplification {
    validated: bool,
    /// Ruling 208's per-arming challenge. The eight bytes a PATH_RESPONSE
    /// must echo to validate this address.
    challenge: [u8; 8],
    sent: u64,
    recv: u64,
}
```

API changes, exhaustively:

| before | after |
|---|---|
| `arm(floor: u64, credit: u64)` | `arm(challenge: [u8; 8], credit: u64)` |
| `on_ack_covering(largest: u64)` | `on_path_response(echo: &[u8; 8])` |
| `set_floor(floor: u64)` | **deleted** — see below |
| `#[cfg(test)] floor() -> u64` | `#[cfg(test)] challenge() -> [u8; 8]` |
| `validated()`, `admits`, `room`, `on_sent`, `on_recv`, `is_validated`, `counters` | **unchanged** |

`Amplification::validated()` keeps `challenge: [0u8; 8]` as its inert
value. It is never compared, because `on_path_response` short-circuits on
`validated`.

**`set_floor` is deleted, and the reason is worth stating.** It existed for
exactly one reason (`mobility.rs:193-199`): the msg1-anchor arming happens
in `established()` *before* the session is installed, and the floor — *"the
counter the next seal will use"* — is not readable until the install. A
challenge has **no such dependency**: it comes from the connection's RNG,
which exists from `connecting()`. The two-phase arming collapses to one
phase. `mod.rs:1758-1760`'s `if !is_validated() { … set_floor(floor) }`
block goes with it.

**The comparison folds the whole difference — it does not short-circuit.**
An `==` on `[u8; 8]` may compile to a short-circuiting `memcmp`, which
turns a 2⁶⁴ guess into a 8 × 256 = 2 048-guess byte-at-a-time walk for an
attacker who can measure. The measurement is deep behind AEAD and frame
parsing and is probably not practical — which is the argument for *not*
worrying, and is exactly the argument the project has rejected twice.
`src/packet/mac.rs`'s `verify` already has the folding shape and its
justification in-crate; mirror it. This costs nothing and deletes the
question.

### 1.4 Retransmission

**PATH_CHALLENGE is §8.7 class `never` — loss recovery does not retransmit
it.** It is re-offered by the pump instead, from the value stored in
`Amplification`, for as long as the arming stands. Same 8 bytes every time
(ruling 208: per-**arming**, not per-transmission).

The re-offer is the existing `owe_elicit` stage, converted:

> `mod.rs:2011-2015` today is
> ```rust
> let elicits = frame::packet_is_ack_eliciting(packing.frames());
> let validate = owe_elicit && self.owes_output();
> if (probe || validate) && !elicits { packing.ping(); }
> ```

Under ruling 208 the `validate` branch must pack a **`PathChallenge`**, not
a `Ping`. **This is the load-bearing change and it is easy to miss**: a
PING elicits an ACK, and after 208 an ACK validates nothing, so a build
that leaves `packing.ping()` here reproduces ruling 203's stall exactly —
the pump shrinks a packet, the packet elicits, the ACK arrives, and the
address stays unvalidated forever. Ruling 207(b) asked whether the pump
owes a PING when the admitted room holds nothing ack-eliciting; **ruling
208 changes the answer from "a PING" to "a PATH_CHALLENGE".**

The `probe` branch (§13.4's PTO) keeps `packing.ping()` — a PTO probe is
about loss detection, not address validation, and the two must not be
merged.

All four boundaries the current code documents at `mod.rs:1968-2009` carry
over **unchanged in force and changed in wording**: (1) nothing owed ⇒ no
challenge; (2) a bare ACK with nothing else owed ⇒ no challenge; (3) not
exempt from §14.5's window; (4) sealed `seal_quiet`, never marking — a
challenge is not fresh application intent. Boundary (4)'s escape argument
survives because a PATH_CHALLENGE is ack-eliciting and *"any ack-eliciting
output we aim at the address arms the death clock by itself"* (SPEC:2170).

**PATH_RESPONSE is also class `never`.** On receiving a `PathChallenge`,
the connection records the 8 bytes as an owed response and emits it on the
next pump, once. If it is lost, the peer's next re-offer produces another.
There is no response queue: **at most one outstanding owed response per
connection**, overwritten by a later challenge. A peer that sends many
challenges gets one response to the newest — this is deliberate and it is
what stops a challenge flood becoming a response flood.

### 1.5 Expiry — what happens if the challenge is never answered

**Nothing new.** No new timer, no new event, no new error variant, no new
`ConnectionLost` variant.

The budget stays armed and the address stays unvalidated. §7.5's death
clock is already armed by the challenge itself (it is ack-eliciting, per
SPEC:2170), so the connection dies at `DEAD_TIMEOUT` with the existing
`ConnectionLost::TimedOut`. This is the pre-168 behaviour for an address
that never answers, and it is correct: an address that cannot answer is an
address we should stop sending to.

**An implementer who invents a `PathValidationFailed` variant, a validation
timer, or a challenge retry counter has exceeded this contract.** Ruling
208 names none of them, working rule 8 reads its list as closed, and §18.1's
error taxonomy is ratified.

### 1.6 Interaction with §7.3's budget

**The challenge must fit inside the armed budget.** It does, and the margin
should be pinned by a `const` assertion rather than argued:

| quantity | bytes |
|---|---|
| PATH_CHALLENGE frame | `1 + 8` = **9** |
| its datagram | `DATA_HEADER_LEN (14) + 9 + AEAD_TAG_LEN (16)` = **39** |
| smallest arming credit (a §3.4 empty-plaintext keepalive roam) | `14 + 0 + 16` = **30** |
| the budget that buys | `3 × 30` = **90** |
| headroom | **51 bytes** — room for the challenge *and* an ACK |
| msg1-anchor credit | 196 ⇒ budget 588, less §2's msg2 charge of 107 ⇒ **481** |

Add to `src/constants.rs`, in the const-assertion block near line 569:

```rust
// **[ruling 208]** The challenge must fit inside the budget its own
// arming creates, or an address roamed to by a bare keepalive can never
// be validated. The smallest arming credit is §3.4's empty-plaintext
// keepalive; `AMPLIFICATION_FACTOR ×` it must cover one challenge
// datagram.
const _: () = assert!(
    AMPLIFICATION_FACTOR as usize * (DATA_HEADER_LEN + AEAD_TAG_LEN)
        >= DATA_HEADER_LEN + 1 + 8 + AEAD_TAG_LEN
);
```

**This is a real pin, not decoration**: it is the invariant that fails if
anyone later grows `DATA_HEADER_LEN`, shrinks `AMPLIFICATION_FACTOR`, or
widens the challenge past 8 bytes.

`Amplification::admits` and `Amplification::room` **do not move**
(ruling 207(a)). `Packing`'s budget clamp (`Connection::packing`,
`mod.rs:1852-1864`) does not move either, including its 30-byte unit
correction (ruling 207(c)).

**Priority within §7.3's ordered list is an open question — Q14.** The list
(SPEC:2225-2232) is `CLOSE, contested probe, pure ACKs, PTO, keepalives,
rtx, new data` and ruling 208 does not place the two new frames in it.

### 1.7 What is removed — ruling 168's machinery

Ruling 208: *"replaced, not supplemented, and leaving both in place would
give an attacker the old path as a bypass."* Removed:

1. `Amplification::floor`, `Amplification::set_floor`,
   `Amplification::on_ack_covering`, `Amplification::floor()` (test
   accessor) — `src/core/connection/mobility.rs`.
2. `Connection::validation_floor()` — `mod.rs:338`. A public-ish core
   accessor; check `src/shell/` and `src/testutil/` for re-exports before
   deleting.
3. **Half** of `Connection::on_ack_coverage` — `mod.rs:1271-1278`, the
   `if from_anchor { self.amplification.on_ack_covering(largest); }` block
   only.

> **⚠ The single most likely way to break this slice.**
> `on_ack_coverage` (`mod.rs:1244-1306`) serves **two independent floors**,
> and its own doc comment says so: §7.3's amplification floor *and* §7.5's
> **contested probe floor**. Ruling 208 reaches only the first.
> **Deleting the function wholesale deletes ruling 176's two exits from the
> pending state and silently disables the contested machinery**, which
> `tests_contested.rs` covers and which no wire test would notice.
> Keep the `match self.contested { … }` block byte-for-byte.

`from_anchor` **survives, and changes consumer.** It is computed at
`mod.rs:508` and today gates only `on_ack_covering`. After this slice it
gates:

- `Amplification::on_path_response` — a response must arrive **from the
  address being validated**, or a peer echoes from its old address and
  validates the new one. This is not optional; it is the whole predicate.
- `Amplification::on_recv` — see §3 (finding A3).

So the variable stays, its two consumers are new, and §7.5's contested
half never consulted it.

**Grep the rationale, not only the token (working rule 4).** The prose that
still argues the ACK proof is listed in `PLAN-7b.md`'s Q-block; it is in
`SPEC.md`, which **no agent in this slice may edit**.

---

### 1.8 `Packing` — the two new planner verbs

`src/core/connection/frame.rs`'s `Packing` gains:

```rust
/// §7.3's return-routability challenge. Ruling 208.
pub(crate) fn path_challenge(&mut self, value: [u8; 8]) -> bool;
/// The echo. Ruling 208.
pub(crate) fn path_response(&mut self, value: [u8; 8]) -> bool;
```

Both return `false` when `room() < 9`, like every other planner verb.

`Stage` (`frame.rs:871-876`) is `Ack, Control, Fill, Ping`. **Both new
frames go in `Stage::Control`** — ahead of the `Fill` stage, so a saturated
stream cannot starve the escape. They must not go in `Stage::Ping`: `Ping`
is last and, under a room clamped to ~9 bytes, last is nowhere.

**This is a §8.5 packing-order statement and §8.5 does not contain it.**
See Q14 — the same question the §7.3 priority list raises, one layer down.
Until it is ruled, `Stage::Control` is what the implementer builds and the
test author asserts.

### 1.9 The receive side

On applying a received `Frame::PathChallenge(v)`: record `v` as the owed
response (a single `Option<[u8; 8]>` on `Connection`, overwritten by a
later challenge), and pump. **No validation of `v`** — it is opaque, any 8
bytes are legal, and a challenge from a peer we have not challenged is
answered normally. Echoing is unconditional; it is the peer's budget, not
ours, that the response unlocks.

On applying a received `Frame::PathResponse(v)`, **gated on `from_anchor`**:
`self.amplification.on_path_response(&v)`.

Both frames are legal on any live connection in any state that admits
frames. Neither is a structural error in any position.

---

## 2. A2 — msg2 is charged to the budget it created

**The defect.** `RESP_PACKET_LEN` = 107 bytes go to the unvalidated msg1
anchor from `endpoint/staged.rs:753` and `endpoint/routing.rs:543`, both
**before** the connection exists, and `Amplification::arm` starts `sent` at
**0**. Ruling 170 forbids an endpoint-side per-address table, so nothing
counts them. Measured ratio 3.55× against §7.3's normative MUST of 3.

**The fix, and where it goes.** Not a signature change to `arm`. In
`Connection::established` (`mod.rs:~1748-1760`), on the `anchor_from_msg1`
branch only, immediately after arming:

```rust
// **[ruling 200]** arms here, and **[A2]** the msg2 that provoked this
// arming has already gone to this address. §7.3 caps *total bytes sent*,
// and the endpoint emitted 107 of them before this connection existed.
self.amplification.on_sent(constants::RESP_PACKET_LEN as u64);
```

One site, because `anchor_from_msg1` is already the single flag that arms
this path (ruling 200).

**Two things the implementer must verify rather than assume:**

1. **Is msg2 ever emitted more than once per arming?** If the responder can
   re-emit msg2 (a retransmitted msg1 re-admitted onto the *same*
   connection), each emission is 107 more bytes and one charge is short. If
   a re-admitted msg1 instead produces a *fresh* arming, the charge is
   correct as written. Establish which, and say so in the commit message.
2. **The resulting budget must still admit a challenge.** 588 − 107 = 481,
   against 39 for a challenge datagram. It does. §1.6's const assertion
   covers the *smaller* roam case and does not cover this one; no second
   assertion is needed, but the arithmetic belongs in a comment.

**Expected test movement.** Any existing test asserting the responder's
post-install budget counters as `(0, 196)` now sees `(107, 196)`. That is a
**correct** expectation change, not a wire change, and it is the one place
in this slice where updating a test expectation is the right action.
`tests_sizing.rs` and `tests_contested.rs` are where to look.

---

## 3. A3 — a non-live connection credits only its anchor

**The defect.** `mod.rs:489-499`:

```rust
match roamed {
    Some(from) => self.commit_roam(now, from, src, datagram.len() as u64),
    None => self.amplification.on_recv(datagram.len() as u64),
}
```

`roamed` is `None` under **two** conditions (`mod.rs:466`): `src == anchor`
**or** `!live`. On a closing or draining connection (§15.2 forbids roaming)
every source yields `None`, so a datagram from anywhere credits an
unvalidated address it did not come from. §7.3 funds the budget from
*"total bytes **received from it**"*.

**The fix.** The `None` arm becomes conditional on the packet having come
from the anchor.

> **⚠ Do not hoist `from_anchor` from line 508.** Its comment says it is
> computed *after* the roam **on purpose**, so the roaming packet itself
> counts as from-anchor. Moving the binding above the `match` changes its
> meaning for the `Some` arm and silently breaks the roam path. Test the
> predicate inside the `None` arm instead — the anchor is unchanged there
> by definition, so `self.remote_address() == Some(src)` is well-defined
> and cheap.

The author of the existing code had this exact insight and applied it to
one of two counters (working rule 4(a): *when you correct one clause of a
sentence, read the other clauses*). This is the sibling.

**The degenerate check (working rule 9).** A test that only exercises a
*live* connection passes against the broken build, because on a live
connection `None` already implies `src == anchor`. The test must drive a
**closing** connection and feed an authenticated, window-fresh packet from
a **third** address, then assert the budget's `recv` did not move. A test
that does not do this asserts nothing.

---

## 4. F1 — the keepalive livelock

**The defect, verified line by line.** `transmit_keepalive`
(`mod.rs:~2360`) returns at

```rust
if self.contested.is_pending() || !self.amplification.admits(size) { return; }
```

**without moving `last_send`**. `transmit_keepalive_if_owed` then calls
`sync_liveness_timer()` unconditionally, and `sync_liveness_timer`
(`mod.rs:~2289`) re-arms `Keepalive` at `last_send + KEEPALIVE_TIMEOUT` and
`PersistentKeepalive` at `last_send + interval` **without consulting
`contested` or `amplification`**. Both land at an instant already passed.
`Driver::deadline` (`driver.rs:915`) mins them out unclamped; `run` step 4
`sleep_until`s a past instant, which completes immediately; `handle_timeout`
re-fires the same timer. 100 % of one core.

### 4.1 The core fix — binding

`sync_liveness_timer` arms `Keepalive` and `PersistentKeepalive` **only
when a keepalive could actually leave**. The condition is exactly
`transmit_keepalive`'s own guard, and it must be **one** predicate used by
both, not two copies:

```rust
/// Whether §7.5's keepalive can leave right now. `transmit_keepalive`'s
/// guard and `sync_liveness_timer`'s arming condition are the same
/// question, and a build that states it twice is the build that drifts.
fn keepalive_can_leave(&self) -> bool {
    let size = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;
    !self.contested.is_pending() && self.amplification.admits(size)
}
```

**Why "arm nothing" is right and "arm later" is wrong.** Both holds lift
only on a **received packet** — a pending mark clears when the budget
admits the probe or an ACK covers the floor; the budget grows only on
`on_recv`. No instant is predictable, so there is no correct future
deadline to arm. Every receive path already ends in `sync_liveness_timer`,
so the timer is restored on the one event that can lift the hold. This is
"held, not dropped" expressed in the timer table.

**`TimerKind::Liveness` is NOT suppressed.** The death clock keeps whatever
`Liveness::deadline()` returns, exactly as today. Suppressing it too would
turn a spinning connection into an **immortal** one, which is worse and is
precisely the collapse ruling 182's beacon proof warns about.

**The `armed == false` state is not a new hole.** In it,
`Liveness::deadline()` is already `None` and no keepalive is armed, so the
connection announces `Timeout(None)`, parks, and waits for a receive or a
close. That is a **quiet** connection, not a spinning one, and it is the
correct reading of §7.3's hold.

### 4.2 The shell change — a detector, not a clamp

> **I am declining half of the review's suggested fix, per working rule 5.**
> `ADVERSARIAL-liveness.md` F1 names *"`Driver::deadline` has no clamp to
> `now`"* as a second-line defence. **A clamp to `now` does not stop the
> spin**: `sleep_until(max(d, now))` completes immediately for exactly the
> same set of deadlines, so the loop runs at the same rate. It changes
> nothing observable.

What *is* worth having is a **detector**, in `Driver::deadline`:

```rust
debug_assert!(
    deadline >= now,
    "a core announced a deadline in the past — ruling 141's spin class",
);
```

A spin is invisible on the wire (ruling 141 said so) and unreachable from
`FlakyWire`, which models a network and not a CPU (working rule 13). An
assertion in the one function every drain passes through converts the whole
class into a test failure in every debug-mode run, forever — including for
instances nobody has found yet. That is the durable half of this fix; §4.1
is the instance.

**This makes `Driver::deadline` need a `now`.** It currently takes none;
`handle_timeout` calls `now()` itself. Threading `now` in is a private
signature change, no public surface. If the implementer finds `now()` is
not stable across the call, that is itself the finding — report it.

### 4.3 How this is tested — binding, because the obvious test hangs

> **A blind test author who tries to assert "100 % CPU" will write a test
> that never returns.** On tokio's paused clock a livelock is an infinite
> loop that never advances virtual time, so the test hangs rather than
> fails, and a hanging test in CI reads as infrastructure flake.

The testable statement is **"no core ever announces a deadline in the
past"**, at the core level, with no shell and no runtime:

1. Drive a connection into the held state — roam to an address whose credit
   is one keepalive (30 B ⇒ 90 B budget), or take a contested mark and
   leave it pending.
2. Advance `now` past `last_send + KEEPALIVE_TIMEOUT`.
3. `handle_timeout(now)`, then drain to the terminal `ConnOutput::Timeout`.
4. **Assert the announced deadline is `None` or `> now`.**

Step 4 is the pin. Against today's build it fails; against a build that
suppresses correctly it passes; and against a build that suppresses *the
death clock too* it also passes — so a **second** assertion is required:
the `Liveness` deadline is still announced when it was armed. Working rule
9: the bound must separate the broken builds, and there are two of them.

---

## 5. F3 — reassembly re-coalescing

**The defect.** `Reassembly::insert` (`recv.rs:467-542`) allocates
`vec![0u8; span]` and copies the whole merged span for **every** accepted
STREAM frame, including one lying wholly inside already-received offset
space. `check_stream` accepts it (`end <= high_water`), it charges
`delta = 0` flow credit, and the peer picks both the span and the rate. One
1-byte frame at offset 5, against a 256 KiB chunk at offset 1 with byte 0
missing, costs ~512 KiB of memory traffic per ~40-byte datagram.

**The fix — an early return before any allocation.** In `insert`, after the
`read_offset` skip and before the merge-span computation: if the incoming
range `[offset, offset + len)` is **entirely covered** by an existing
chunk, return without touching the deque.

**Why one chunk suffices, and this is the load-bearing lemma.** `insert`
merges chunks that overlap **or are adjacent**, so the stored chunks are
pairwise disjoint *and* non-adjacent. A range covered by the union of two
or more stored chunks would require them to be adjacent or overlapping,
which the invariant forbids. Therefore **covered-by-the-union is exactly
covered-by-one-chunk**, and the check is a single lookup.

**The invariant this buys, and it should be stated in the doc comment:**
after the fix, `insert`'s allocation and copy work is bounded by bytes that
are **new** to the buffer, and new bytes are bounded by flow credit. A
partially-overlapping frame still reallocates its merge span, and that is
correct: it makes progress by at least one byte, so the total is bounded by
delivered bytes rather than by peer-chosen repetition. **F3 is exactly the
zero-progress case, and closing it closes the amplification.**

**Do not also change `check_stream`.** Rejecting a fully-duplicate frame as
a protocol violation would kill connections over ordinary retransmission.
The frame is legal; only the work is not.

**`VecDeque::remove` in a loop** (`recv.rs:524-526`) is O(n) each and is a
second, smaller cost on the same path. Fixing it is **optional** and must
not be conflated with the above — it is a constant-factor cleanup, F3 is
an amplification.

**Test shape.** Deliver `1..N` with byte 0 omitted, then deliver the same
1-byte frame at an interior offset K times, and assert the buffer's chunk
count and byte content are unchanged and that no read becomes available.
A counter of insert-path allocations is not available and should not be
invented; assert on **observable state**, and note that this test pins
correctness, not cost. The cost claim is argued in the doc comment.

---

## 6. F5 — the eviction warn flood

`Datagrams::push_recv` → `trace_drop` (`datagram.rs:147-154`) emits one
`tracing::warn!` per evicted datagram, at the peer's chosen rate, forever.
§11.5 requires **visibility** and says nothing about **rate** — the two are
separable, which is working rule 8's shape again.

**The fix.** Keep the cumulative counter and the `warn!` level (the doc
comment's justification for `warn!` over `debug!` is correct and stays).
Emit only when the cumulative count crosses a **power of two**: 1, 2, 4, 8,
16, … Each record carries the cumulative total, so an operator loses no
information about magnitude and gains a bound of `log₂(n)` records.

No new constant, no configuration, no new trace target — §18.2's five
targets are a **closed list** and this stays on `slither::io`/wherever it
is today. `push_send` is application-driven and is **not** changed.

---

## 7. H2 + M1 — the two documentation gaps

Both are documentation-only. One author, adjacent files.

**H2 — `AuthError::Replay`** (`src/error.rs:141-150`). Add a paragraph
stating that the variant means *this initiation is not fresh*, never *this
peer misbehaved*; that a replayer holding one captured msg1 produces it at
will, from any address, against a static the `ss` has genuinely **proven**;
and that denylisting on it bans the victim. Mirror the shape of the
existing `AuthError::Local` note two variants down (`error.rs:154-164`),
which already reasons about exactly this hazard for the neighbouring
variant and is the in-crate precedent.

Documentation obligation #2 (`src/lib.rs:83-87`) is scoped to the
**claimed** static and must be widened or joined — see Q9.

**M1 — the other two unauthenticated quantities.** §6.1 names three:
the claimed static, **the source address**, and **`sender_index`**. Every
warning in the crate covers the first. Add the missing coverage at
`Intro::source()` and `Intro::sender_index()` (`shell/staged.rs:79-102`),
which today carry careful docs about live reads and **no warning at all**,
while `Intro`'s `Debug` prints both (`staged.rs:156-163`).

**The source address is the worse key of the two**, and the doc should say
so: it costs an attacker 0 DH rather than 1, needs no knowledge of anyone's
public key, and has no return-routability proof at stage 0 — so a
source-address denylist under §6.3's described flood bans spoofed victims.

**Whether `Intro`'s `Debug` should stop printing them** — as `Claimed`'s
already does for the claimed static — is Q15. Default for this slice: **do
not change `Debug` output**; document only.

---

## 8. H3 — `SoftwareIdentity`'s entropy path

**The finding is the asymmetry, not a bug.** `EndpointBuilder::rng_seed`
(`shell/endpoint.rs:357-373`) has a *"This is a test-only facility"*
heading, cites §16.6, and defaults to `getrandom::fill` when unset
(`endpoint.rs:405-409`). `SoftwareIdentity<S, R = ChaCha20Rng>`
(`identity.rs:151`) takes `R` as a **mandatory** constructor argument with
**no OS-entropy path anywhere in the crate**, and `R` is the source of the
static scalar *and* every handshake ephemeral on both roles. Its doc frames
determinism as a feature.

**This section is contingent on Q10.** If the maintainer rules for the API
addition, the binding shape is:

```rust
impl<S: Suite> SoftwareIdentity<S, ChaCha20Rng> {
    /// Seeded from OS entropy. The production constructor.
    pub fn generate_os() -> Result<Self, /* the existing error */>;
    pub fn from_scalar_os(scalar: /* … */) -> Result<Self, /* … */>;
}
```

seeding `ChaCha20Rng` from `getrandom::fill`, **mirroring
`endpoint.rs:405-409` line for line** — same crate, same call, no new
dependency, and a reader who has seen one recognises the other.

Plus, unconditionally (this half needs no ruling): a warning on
`SoftwareIdentity` naming what a predictable `R` costs — with
`generate()`, the **static private key** is recoverable; with
`from_scalar()`, the initiator's ephemeral is, so `es = DH(e_i, S_r)` is
computable from public data alone, which decrypts msg1's static field and
its timestamp and voids §5.3's stated confidentiality-level-2 guarantee.

**Do not remove or deprecate the `R` parameter.** `testutil` depends on the
seeded form (`testutil/mod.rs:976`), it is correct there, and §16.6's whole
reproducibility property rests on it.

---

## 9. Test-facing API surface (for the blind test author)

Everything below exists at `23d0409` and is unchanged by this slice unless
noted. A blind author can write compiling tests against it.

**Fixtures** — `src/core/connection/testfix.rs`: `Solo` (`installed_at`,
`unestablished`, `install`, `deliver_packed`, `deliver_stream_bytes`,
`from_session`), `Pair` (`drain_a`/`drain_b`, `flush_a_to_b`,
`flush_a_to_b_from`, `flush_b_to_a_from`, `pump`), `drain`, `tick`,
`handshake_pair`, `a_addr`/`b_addr`/`v4`, `t0`, `parse_frames`,
`stream_frame` and the other raw frame builders.

**`Drained`**: `transmits()`, `count_events()`, `closed()`, `position()`.

**New in this slice** — the test author may rely on these existing:

| item | where | note |
|---|---|---|
| `constants::FRAME_PATH_CHALLENGE` / `_RESPONSE` | `src/constants.rs` | `0x1a` / `0x1b` |
| `Frame::PathChallenge([u8;8])` / `PathResponse([u8;8])` | `frame.rs` | |
| `Amplification::arm(challenge, credit)` | `mobility.rs` | signature change |
| `Amplification::on_path_response(&[u8;8])` | `mobility.rs` | replaces `on_ack_covering` |
| `#[cfg(test)] Amplification::challenge() -> [u8;8]` | `mobility.rs` | replaces `floor()` |
| `Packing::path_challenge(v)` / `path_response(v)` | `frame.rs` | |
| a raw `path_challenge_frame(v: [u8;8]) -> Vec<u8>` builder | `testfix.rs` | **integrator-owned**, see `PLAN-7b.md` |

**Removed** — tests referencing these will not compile:
`Connection::validation_floor()`, `Amplification::floor()`,
`Amplification::set_floor()`, `Amplification::on_ack_covering()`.

`tests_roam.rs:466`, `tests_contested.rs:679`/`:708`, `tests_sizing.rs:282`
and `:721` all reference them today. **They are expired premises, not dead
tests**: the process memo's defect class 3 says the right action is to
**narrow or invert the assertion, never delete the test** — the mutation
each was built to catch is usually still live by a new route. A test that
pinned *"an ACK below the floor proves nothing"* becomes *"a PATH_RESPONSE
with the wrong bytes proves nothing"*, and it is a **better** test after
the change than before.

---

## 10. Wire pins after this slice

**Unchanged, and the golden-wire vectors must stay byte-identical:**
`IK_MSG1_LEN` 174 · `IK_MSG2_LEN` 81 · `INIT_PACKET_LEN` 196 ·
`RESP_PACKET_LEN` 107 · `VERSION` `0x01` · `PROLOGUE` `b"slither\x01"` ·
`DATA_HEADER_LEN` 14 · `AEAD_TAG_LEN` 16 · `MAX_PLAINTEXT` ·
`MAX_DATAGRAM` 1200 · `AMPLIFICATION_FACTOR` 3.

**Ruling 208 adds two frame type codes and moves no existing byte.** The
handshake wire is untouched; the packet header is untouched; every existing
frame encoding is untouched.

> **The brief says the golden-wire vectors *"will go red by design"*.**
> On this design they should **not**. Adding two unused type codes changes
> no byte of any existing vector. If a golden-wire test goes red, that is
> the CLAUDE.md stop signal in full force — something moved that this
> contract did not authorise. See Q16.

**New pins this slice adds**, both of which must have tests:

1. `FRAME_PATH_CHALLENGE == 0x1a`, `FRAME_PATH_RESPONSE == 0x1b`, and both
   present in `constants.rs`'s frame-code uniqueness table.
2. The encoded length of both frames is exactly **9** bytes, and the
   §1.6 const assertion that a challenge datagram fits the smallest budget
   its own arming can create.

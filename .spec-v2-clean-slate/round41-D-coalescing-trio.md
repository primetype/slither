# Round 41 — D: the coalescing trio (round 40 slice B recorded scope questions)

Base commit: `94dab20` (verified — see below). All line citations are against this commit.
Status: COMPLETE. The headings immediately below are the table of contents; the content follows the rule.

## 0. Ruling 250 (the contested probe coalescing rule)

## 1. Item 3 — coalescing granularity in the [40, 49) both-owed window

### 1.1 The owed path frames and their sizes
### 1.2 What the implementation sends today
### 1.3 What greedy would send
### 1.4 Is the window reachable? (arithmetic)
### 1.5 The amended sentence whose letter forces all-or-nothing
### 1.6 OPTIONS
### 1.7 RECOMMENDED

## 2. Item 4 — where an owed pure ACK rides when the probe coalesces

### 2.1 §8.5's owed-ACK sentence
### 2.2 What the code does today
### 2.3 §7.3 probe-outranks-all-output-but-CLOSE / §16.5 contested row
### 2.4 Observability
### 2.5 OPTIONS
### 2.6 RECOMMENDED

## 3. Item 5 — may STREAM fill ride the cwnd-exempt probe?

### 3.1 §8.5's fill permission
### 3.2 §14.5's exemption rationale ("at most 18 B of piggyback")
### 3.3 What the implementation does
### 3.4 Rule 3: which statement a proof depends on
### 3.5 OPTIONS
### 3.6 RECOMMENDED

## 4. VERDICT

---

(base verified: `git rev-parse HEAD` = 94dab200a172a5b62b4d28711962574d61a9d3c8, "Ruling 257: the bare FIN defers when no frame fits" — matches brief.)

## 0. Ruling 250 (the contested probe coalescing rule)

`.spec-v2-clean-slate/rulings.md:7325-7407`. The operative clause (`:7327-7336`):

> **Ruling: (i) `pump_packets`' 212(c) pre-pass is deleted — the contested probe is built first, at rank 2, and the pump continues on the same pass, exactly as ratified §7.3 orders. (ii) The probe coalesces: one packet carrying the PING and the owed `PATH_RESPONSE`/`PATH_CHALLENGE` when the budget's remaining room admits the coalesced size — 40 B with one path frame owed, 49 B with both; a bare 31 B PING otherwise, the path frames following at their rank. One packet, one counter, one sent-map entry. (iii) §7.3's contention arithmetic is restated at pump time: checked against the remaining room when the packet is built, never asserted from the arming-instant floor. (iv) The coalesced packet keeps the probe's §14.5 cwnd exemption.**

Note the "40 B with one path frame owed, 49 B with both; a bare 31 B PING
otherwise" is the ruling's own sizing, and it is the source of the item-3
window: 31 (bare) < 40 (one) < 49 (both).

Ruling 250's §14.5 rationale (`rulings.md:7377-7383`):

> The merged packet keeps the exemption: it exists because the probe demanded it, the piggyback adds at most 18 B, and gating it would starve the challenge at collapsed cwnd exactly where a roam makes it owed.

### 0.1 The amended §7.3 sentences (the ratified letter)

`SPEC.md:2493-2507` — the coalescing sentence itself (**this is the letter
that forces all-or-nothing**; emphasis mine):

> **[AMENDED 2026/08/17 — ruling 250]** The pump prefers **one packet**: the probe coalesces the owed path frames when the remaining room at pump time admits the coalesced size — 40 B with one 9 B path frame owed (the packet this section's own arithmetic has priced all along), 49 B with both, which a roam constructs (§13.6 keeps the owed `PATH_RESPONSE` and re-draws the challenge at one instant) — and emits the bare 31 B PING otherwise, the path frames following at their rank when room next admits them; when room admits neither, nothing is emitted and no `Pto` deadline is announced (§13.3, ruling 249). One packet, one counter, one sent-map entry — ruling 221's deletion of the dedicated-packet machinery is not resurrected. "The budget holds both and always does" was a universal over **armed** budgets with an unstated scope; the pump-time room check is the guarantee the implementation can keep.

The three sizes are stated as a *function of the owed set*, not of the
room: with both owed the only admitted coalesced size named is 49 B, and
anything short of it falls to "**emits the bare 31 B PING otherwise**".
That "otherwise" is the all-or-nothing.

`SPEC.md:2444-2453` — the pump-time restatement of the contention arithmetic:

> They therefore never contend with one another **at the arming instant** — **[AMENDED 2026/08/17 — ruling 250]** the floor is a property of the budget when it is armed, and by pump time the budget may have been spent down by whatever left since, so the guarantee the pump keeps is checked against the **remaining** room when the packet is built (ruling 207(c)'s seam), never asserted from the floor

`SPEC.md:511-517` (§1.3's flag-count record) restates the same rule in one sentence.

---

## 1. Item 3 — coalescing granularity in the [40, 49) both-owed window

### 1.1 The owed path frames and their sizes (verified against the encodings)

- `src/core/connection/frame.rs:68` — `pub(super) const PATH_CHALLENGE_LEN: usize = 8;`
- `src/core/connection/frame.rs:147` — `Frame::PathChallenge(_) | Frame::PathResponse(_) => 1 + PATH_CHALLENGE_LEN,` (the encoded length: 1 type byte + 8 value bytes = **9 B each**).
- `src/core/connection/mod.rs:3226` — `const PATH_FRAME_LEN: usize = 1 + frame::PATH_CHALLENGE_LEN;` → 9.
- `src/constants.rs:90` `DATA_HEADER_LEN = 14`; `src/constants.rs:119` `AEAD_TAG_LEN = 16`; `src/constants.rs:141` `MAX_PLAINTEXT = 1170`.

So, in **datagram** bytes: bare PING = 14 + 1 + 16 = **31**; PING + one path
frame = 14 + 10 + 16 = **40**; PING + both = 14 + 19 + 16 = **49**. Ruling
250's three numbers check out exactly.

### 1.2 What the implementation sends today — the actual predicate

`src/core/connection/mod.rs:3223-3236`:

```
let response_owed = self.owed_path_response.is_some();
let challenge_owed = *owe_challenge && self.amplification.outstanding_challenge().is_some();
let owed = usize::from(response_owed) + usize::from(challenge_owed);
const PATH_FRAME_LEN: usize = 1 + frame::PATH_CHALLENGE_LEN;
let coalesced = 1 + owed * PATH_FRAME_LEN;
// All or nothing over the **owed set**, which is what §7.3 prices:
// 40 B with one owed, 49 B with both. A room that admits only part
// of the set leaves the whole of it to ranks 3 and 4, where it is
// offered again the moment room next admits it.
let path = if owed > 0 && packing.room() >= coalesced {
    self.pack_path_frames(&mut packing, challenge_owed)
} else {
    PathPacked::default()
};
```

`packing.room()` is **plaintext** bytes (`frame.rs:1045`, `budget - used`);
`self.packing()` (`mod.rs:2137-2149`) sets that budget to
`Amplification::room().saturating_sub(30).min(MAX_PLAINTEXT)`, or
`Packing::new()` (full `MAX_PLAINTEXT`) when `Amplification::room()` is
`None` — i.e. **when the address is validated** (`mobility.rs:177-183`:
`(!self.validated).then(|| 3*recv - sent)`).

With both owed, `coalesced == 19` plaintext. The predicate therefore takes
the path branch iff plaintext room ≥ 19, i.e. **datagram room ≥ 49**. With
plaintext room in [10, 19) — datagram room in **[40, 49)** — it takes the
`else` branch, packs *no* path frame, and `packing.ping()` (`mod.rs:3241`)
builds the bare 1-byte plaintext → **31 B datagram**. The 9-to-18 spare
bytes are wasted.

### 1.3 What greedy would send

Largest-first is meaningless here — both owed frames are the same 9 B — so
greedy is simply "pack as many of the owed set as fit": with datagram room
in [40, 49) it sends PING + **one** path frame (§7.3's own listed order puts
`PATH_RESPONSE` first), a 40 B packet, and leaves the other frame owed for
rank 3/4 on a later pass. Strictly one more 9 B repair frame per probe in
that window, at no extra packet, header, tag, counter or sent-map entry.

### 1.4 Is the [40, 49) window reachable? — the arithmetic, honestly

**It is reachable, and it is not exotic.** Three conditions must hold at
the instant `pump_contested_probe` runs:

1. **The address is unvalidated.** `Amplification::room()`
   (`mobility.rs:177-183`) returns `None` when `validated`, and
   `packing()` (`mod.rs:2140-2142`) then hands back `Packing::new()` — a
   full `MAX_PLAINTEXT` = 1170 budget, which admits 19 trivially. So a
   validated address never enters the window. Note this is *implied* by
   the both-owed premise anyway: `challenge_owed` requires
   `self.amplification.outstanding_challenge().is_some()`
   (`mod.rs:3224`), and an outstanding challenge is exactly the
   unvalidated state.
2. **Both frames owed.** §7.3 itself names the constructor
   (`SPEC.md:2497-2499`): a roam — *"§13.6 keeps the owed `PATH_RESPONSE`
   and re-draws the challenge at one instant"*.
3. **Remaining datagram room `3·recv − sent` ∈ [40, 49).**

The floor argument that *used* to make this unreachable is exactly what
ruling 250 retired. The 90 B floor (`SPEC.md:2444-2446`) is a property of
the budget **at the arming instant** — `Amplification::arm` re-seeds
`recv` from the roaming packet's bytes, and the smallest data datagram is
`DATA_HEADER_LEN + AEAD_TAG_LEN` = 30 B, so `3 × 30` = 90. Any send
between arming and the probe's pump spends it down, and the residue is an
arbitrary non-negative integer: `room = 3·recv − sent` with `sent` a sum
of datagram sizes each ≥ 30 and `recv` a sum of received datagram sizes.
Landing in a 9-wide window is one arithmetic coincidence among 9/…, not a
construction that has to be contrived. Concretely: arm at a 30 B keepalive
→ room 90; one 30–50 B control packet leaves → room 40–60; a second
credited receive of any size re-opens it. Nothing forbids a residue of
40–48.

I did **not** find a test that pins the window (see §1.6); the claim above
is arithmetic on `mobility.rs:177-183` + `mod.rs:2137-2149`, not a
measurement. **If the maintainer wants this decided on evidence rather
than arithmetic, the measurement is cheap** — a `Solo` fixture that arms
the budget, spends it to 40–48, and asserts the emitted probe's datagram
length is 31 rather than 40 (the `room` helpers already exist at
`tests_contested.rs:275` and `tests_path.rs:134`).

**Second-order effect, and it is the sharper cost.** In the window the
probe emits 31 B and leaves `3·recv − sent` ∈ [9, 18) — **below the 30 B
minimum datagram**. So the pump loop that follows cannot build *anything*:
`admits(size)` fails for every candidate (`mod.rs:2463-2471`). Under
greedy the probe emits 40 B and the residue is [0, 9) — also below 30. So
the residue is unusable either way, which strengthens greedy: the wasted
9–18 B in the all-or-nothing case buys nothing at all elsewhere. There is
no packet that the 9 B saved would have funded.

### 1.5 The sentence whose letter forces all-or-nothing

`SPEC.md:2493-2507`, quoted in full at §0.1 above. The operative clause:

> the probe coalesces the owed path frames when the remaining room at pump time admits the coalesced size — 40 B with one 9 B path frame owed …, 49 B with both … — and **emits the bare 31 B PING otherwise**

The sizes are indexed by *the owed set*, not by the room; "otherwise"
therefore covers "both owed, room 40–48" and sends the bare PING. The
implementation's comment at `mod.rs:3228-3231` states this reading
explicitly (*"All or nothing over the **owed set**, which is what §7.3
prices"*), so the code is a faithful implementation of the ratified
letter, not a divergence. **This is a scope question, not a conformance
bug.**

### 1.5b Test coverage: the window that *is* pinned is the other one

`src/core/connection/tests_contested.rs:1469` —
`a_room_that_admits_the_bare_probe_but_not_the_coalesced_one_sends_the_probe_alone`
pins the **one-owed** window, [31, 40): `spend_leaving(&mut s, t, COALESCED_PROBE_LEN - 1)`
= 39 B of room, one path frame owed (`tests_contested.rs:1480`), asserting a
31 B probe (`:1507`) with plaintext `vec![FRAME_PING as u8]` (`:1511`).

In *that* window all-or-nothing and greedy **agree** — only one frame is
owed, and if it does not fit, nothing fits. The constants are declared at
`tests_contested.rs:165` (`PROBE_PACKET_LEN` 31), `:181`
(`COALESCED_PROBE_LEN` 40) and `:185` (`COALESCED_PROBE_BOTH_LEN` 49), and
**`COALESCED_PROBE_BOTH_LEN` never appears in a room premise** — the
both-owed test (`:1579`,
`both_owed_path_frames_ride_the_probe_in_one_packet_in_spec_8_5_order`)
exercises the *ample* room case only. **So the [40, 49) both-owed window is
untested in either direction**, and neither behaviour is pinned by a test
today. That is worth recording independently of which option is taken.

### 1.6 A cost of option (b) the item does not mention: the PING byte must be reserved

`pack_path_frames` (`mod.rs:2673-2700`) is **already greedy** — its own
comment says so (`mod.rs:2679-2687`): *"each verb below refuses its frame
when nine bytes do not remain, and the caller keeps owing what it could not
pack."* The all-or-nothing behaviour lives **entirely** in
`pump_contested_probe`'s gate at `mod.rs:3232`.

But option (b) is **not** the deletion of that gate. `Packing::push`
(`frame.rs:1114-1120`) carries
`debug_assert!(stage >= self.stage, "§8.5's packing order runs forwards only")`
and encodes frames in **call order**, so the PING cannot be packed before
the path frames to reserve its byte. Delete the gate naively and at exactly
39 B of room with one frame owed, `pack_path_frames` consumes all 9
plaintext bytes, `packing.ping()` at `mod.rs:3241` then returns `false`, and
`pump_contested_probe` **returns `false`** — the probe is not sent at all,
no `Contested` is emitted, no deadline arms (`mod.rs:3241-3243`, citing
§7.3's *"when room admits neither, nothing is emitted"*). That is a
regression against the currently-passing test at `tests_contested.rs:1469`.

Option (b) therefore needs a **reserving** greedy pack: a `reserve: usize`
parameter on `pack_path_frames` (or a second helper), packing each 9 B
frame only while `packing.room() >= PATH_FRAME_LEN + 1`. `pump_path_frames`
(`mod.rs:2730`), the other call site, reserves nothing and must keep
today's behaviour. Small, but it is a signature change on a shared helper,
not a two-token deletion.

### 1.7 OPTIONS — item 3

**(a) Keep all-or-nothing over the owed set (the ratified letter).**
No behaviour change; the gap is that the *scope* is stated only by an
"otherwise" that a reader has to reverse-engineer, which is working rule 8's
defect class (a construction with an unstated scope). Make it explicit.

*Edit — `SPEC.md`, insert after line 2507 (end of the amended paragraph,
"…the guarantee the implementation can keep."):*

> **Coalescing is all-or-nothing over the owed set, and the "otherwise" above is that rule.** The three sizes are indexed by what is *owed*, not by what fits: with both path frames owed, a remaining room of 40–48 B admits one of them and the pump nonetheless emits the bare 31 B PING, leaving **both** to ranks 3 and 4. The alternative — packing whichever frames fit — buys one 9 B frame in a 9 B-wide window, and buys it at the cost of a per-frame room check that must also reserve the PING's own byte, since §8.5 packs the PING after the path frames and a probe packet with no PING is not a probe. One invariant, checked once, is worth more than nine bytes recovered on a window this narrow. *(The residue the packing leaves is unusable in both readings: 40 − 31 = 9 and 48 − 40 = 8 are each below §3.4's 30 B minimum datagram, so no later packet is funded by the bytes either choice saves.)*

**(b) Greedy over the owed set, PING byte reserved.**
Strictly more repair per probe in [40, 49): one `PATH_RESPONSE` delivered a
round trip earlier, in the one state (§7.3's scarce budget on an unvalidated
address after a roam) where a round trip is what the whole section is
economising.

*Edit — `SPEC.md:2494-2503`, replacing "the coalesced size … when room next
admits them" with:*

> the coalesced size — 40 B with one 9 B path frame owed (the packet this section's own arithmetic has priced all along), 49 B with both, which a roam constructs (§13.6 keeps the owed `PATH_RESPONSE` and re-draws the challenge at one instant). **When the room admits the PING and *some* but not all of the owed set, the pump packs what fits, in the order above — `PATH_RESPONSE` first — and leaves the remainder owed at its rank; the PING's own byte is reserved before any path frame is packed, because a probe packet without it is not a probe.** It emits the bare 31 B PING when no path frame fits beside it, the path frames following at their rank when room next admits them; when room admits neither, nothing is emitted and no `Pto` deadline is announced (§13.3, ruling 249).

*Code — `mod.rs:3226-3236`:* replace the `coalesced` gate with
`packing.room() >= PATH_FRAME_LEN + 1` and a reserving `pack_path_frames`.
*Tests:* `tests_contested.rs:1469` still passes (one owed, 39 B: 9 + 1 > 9
room, so still bare). A **new** test is required for [40, 49) both-owed —
the class currently untested in either direction.

**Wire-visibility and re-ratification.** (b) **is wire-visible**: in the
[40, 49) both-owed window a 40 B packet carrying `PATH_RESPONSE` + PING
replaces a 31 B bare PING. It does not move a golden-wire vector (those pin
the handshake) and it changes no constant, but it **moves ratified
behaviour** — §7.3's amended sentence is ruling 250(ii), one day old — so it
needs an explicit ratification amending `SPEC.md:2494-2503`, plus §1.3's
amendment table row (`SPEC.md:26`) gaining the new ruling. (a) needs a
ratification only for the added scope sentence, which states existing
behaviour.

### 1.8 RECOMMENDED — item 3

**(a), with the scope sentence.** The measured gain is one 9 B frame inside
a 9 B-wide window that no test reaches; the residue it recovers cannot fund
another packet (§1.4); and the frame it delays is re-offered at rank 3 on
the very next pump that has room. Against that, (b) reverses a
one-day-old ratified sentence, changes a shared helper's signature, and
introduces a reserve invariant whose violation is silent (the probe simply
stops going out — `mod.rs:3241`). The honest statement is that the
ratified letter is *defensible*, not *forced*: greedy is genuinely better
by one frame, and the reason to decline it is that the invariant is worth
more than the frame. Say that in the spec rather than leaving "otherwise"
to carry it.

---

## 2. Item 4 — where an owed pure ACK rides when the probe coalesces

### 2.1 The relevant spec sentences

**§8.5's packing-order sentence** (`SPEC.md:3600-3607`):

> Within a packet the sender packs in this order: the ACK first (if owed), then control frames — **[AMENDED 2026/08/16 — ruling 208]** `PATH_RESPONSE` and `PATH_CHALLENGE` **first among the control frames**, then credit grants, RESET_STREAM, CLOSE — then STREAM and DATAGRAM fill, then PING last among **length-prefixed** frames if a probe still owes ack-eliciting content.

Note what this is and is not: §8.5 itself says so at `SPEC.md:3609-3613`
(ruling 208) — *"§8.5 decides **byte placement inside a packet whose size is
already settled**; §7.3 decides **which class of output gets a scarce budget
at all**"*. §8.5 orders an ACK **if the packet carries one**; it never says
which packet must.

**§12.4's owed-ACK bullet** (`SPEC.md:4484-4487`) — this is the sentence
that actually bites:

> - An owed ACK rides the next outgoing packet (packing order §8.5); if none is pending, a standalone ACK packet is generated. Pure-ACK packets are sealed `seal_quiet` (§7.4), are not ack-eliciting (no ACK-of-ACK loops), are never tracked for loss, and bypass the congestion window (§14.5).

**§7.3's rank list** (`SPEC.md:2418-2432`) puts **pure ACKs at rank 5** —
below the probe (2), `PATH_RESPONSE` (3) and `PATH_CHALLENGE` (4).

**§16.5's contested row** (`SPEC.md:5073`) describes the probe packet's
contents as *"the probe's packet — the PING, plus the owed path frames when
room admits (ruling 250) —"*. Under working rule 8 that list reads as
**exhaustive**, and it does not contain an ACK.

### 2.2 What the code does today

`pump_contested_probe` (`mod.rs:3197-3320`) touches its `Packing` at exactly
five points (verified by enumerating every `packing.`/`self.pack*` line in
the function body):

```
mod.rs:3206  let mut packing = self.packing();
mod.rs:3232  ... packing.room() >= coalesced
mod.rs:3233  self.pack_path_frames(&mut packing, challenge_owed)
mod.rs:3241  if !packing.ping()
mod.rs:3244  let plaintext = packing.into_plaintext();
```

There is **no `pack_ack`**. `self.pack_ack` has exactly two call sites in
the crate — `mod.rs:2209` (Stage 1 of the pump *loop*, which runs after the
probe) and `mod.rs:2816` (inside `transmit_pure_ack`). So:

**the owed ACK does not ride the probe packet; it rides the next packet the
pump loop builds, behind the probe, in the same drain** — and since a
contested connection typically has nothing else queued, that packet is a
pure ACK packet of its own.

It is **not suppressed**: `ack.is_owed()` is untouched by the probe path,
and Stage 1 packs it on the very next iteration. The one case where it does
not leave is when the remaining amplification room after the probe cannot
fund a 30 B minimum datagram — `mod.rs:2463-2471` breaks the loop and the
ACK stays owed. That is precisely §7.3 rank 5 yielding to rank 2, which the
rank list already ratifies.

### 2.3 It is observable, and it has already been measured

`tests_contested.rs:1623-1632`, in the blind author's own words:

> **Measured, not assumed.** §12.4 emits this ACK *immediately* rather than on its delayed timer, and on the base build it leaves as a third pure-ACK packet **behind** the probe — measured at `96f7ef0`: `[[PathResponse, PathChallenge], [Ping], [Ack]]`, 48 + 31 + 35 bytes. Whether §8.5 then packs that ACK ahead of the path frames in the coalesced packet or leaves it in its own is not what ruling 250 rules on, so the assertions below are stated over the packet's frames with any ACK filtered out.

The test then filters `Wire::Ack` out of the probe packet's frame list
(`tests_contested.rs:1643-1647`) so that it passes under *either* reading.
So: **wire-visible (a separate ~35 B datagram), reachable by the existing
fixture, and deliberately unpinned.** The author flagged this gap; it is
item 4.

### 2.4 Does §7.3's "outranks all other output but CLOSE" bear on it?

**Only weakly, and not in the direction one might expect.** The §16.5
contested row (`SPEC.md:5073`) and §14.5 (`SPEC.md:4870-4876`) both phrase
it as a **rank**, and §7.3's list spells the rank out: probe 2, pure ACK 5.
A rank says the ACK **yields the budget** to the probe; it says nothing
about whether the ACK may **ride** the probe's packet, which costs the probe
no rank at all. §8.5's ruling-208 paragraph makes exactly this distinction
in the adjacent case (`SPEC.md:3613-3616`): *"§7.3 ranking the path frames
above a pure ACK is not in tension with the ACK being packed first here: a
packet that carries both carries both."* So the rank does not settle item 4
either way.

### 2.5 The conflict, reported not resolved

There **is** a literal conflict, and it is worth stating plainly rather than
smoothing over:

- §12.4 (`SPEC.md:4484`): *"An owed ACK rides the **next outgoing packet**"*.
- The code: the probe **is** the next outgoing packet, and the ACK is not on it.

Three ratified statements support the code against §12.4's letter:

1. **§16.5's contested row** (`SPEC.md:5073`) enumerates the probe packet as
   *"the PING, plus the owed path frames when room admits"* — rule 8 reads
   that as exhaustive, and an ACK is not in it.
2. **§14.5's amended exemption** (`SPEC.md:4846-4851`) justifies covering
   *"the probe's packet as built"* on the ground that *"the piggyback adds
   at most **18 B** of frames"*. An ACK frame is bounded only by
   `MAX_ACK_RANGES` = 64 (`src/constants.rs:356`), so putting it on the
   exempt packet falsifies the sentence that earns the exemption. (§2.3's
   measurement shows 5 B in the common case; the bound is what the argument
   uses, not the common case.)
3. **§7.3's rank 5** already contemplates the ACK yielding under scarcity.

Against it: §12.4's own purpose is *don't sit on an owed ACK, and don't
manufacture a packet for one*. Under the code the ACK is neither sat on
(same drain) nor manufactured redundantly — except in the narrow case where
the post-probe residue is under 30 B, where it waits for the next credited
receive. **I read §12.4's "next outgoing packet" as an anti-latency policy
statement written before a packet class existed that deliberately carries
nothing else, and therefore as under-scoped rather than violated. But that
is a reading, and it is the maintainer's call**: the two statements do
conflict on their face, and rule 3 says report rather than pick.

### 2.6 OPTIONS — item 4

**(a) State the code: the probe packet carries no ACK; the owed ACK rides
the next packet.** No behaviour change; closes the scope with the
enumeration §16.5 already implies and §14.5's bound already requires.

*Edit — `SPEC.md`, append to §12.4's bullet at line 4487 (after "…bypass the
congestion window (§14.5)."):*

> **[Scope — the one packet this does not name.]** The **contested-connection probe** (§7.5, §7.3) is built before the pump's ordinary packets and carries only the PING and the owed path frames (§16.5). An owed ACK does **not** ride it; it rides the first ordinary packet behind it, in the same pass, and waits for the budget where §7.3 ranks it (rank 5, under the probe and both path frames). Two reasons, and the second is load-bearing: the probe exists to ask one question and is sized to it, and §14.5's exemption for *"the probe's packet as built"* is earned by a piggyback bounded at 18 B, which an ACK frame — up to `MAX_ACK_RANGES` ranges — is not.

*Edit — `SPEC.md:5073`, §16.5's contested row, after "(ruling 250)":*
add "— and **nothing else**: no ACK, no fill". (Optional; the row already
reads exhaustively under rule 8, and this makes it say so.)

**(b) Let the ACK ride the coalescing probe.** Saves 30 B of header+tag per
contested probe and delivers the ACK in the one state where the budget may
refuse it. But it **breaks §14.5's 18 B bound** — the sentence that earns
the merged packet its cwnd exemption — so it cannot be taken without also
re-deriving that exemption for an unbounded piggyback. It also competes for
the same bytes as item 3: at 40–48 B of room with both path frames owed,
adding an ACK makes the coalesced packet *less* likely to fit, not more.
*Edit:* §8.5 already orders it (ACK first); the amendment would be to
§14.5's clause at `SPEC.md:4846-4851`, replacing *"the piggyback adds at
most 18 B of frames"* with a new argument, plus a §16.5 row change and a new
test. **Wire-visible; moves ratified behaviour.**

### 2.7 RECOMMENDED — item 4

**(a).** The code is defensible and two ratified sentences (§16.5's
enumeration, §14.5's 18 B bound) already presuppose it; §12.4's "next
outgoing packet" is the under-scoped one, and stating the exception where
§12.4 makes the claim is the smallest edit that removes the conflict.
**Report to the maintainer that §12.4:4484 and the code do conflict on the
literal text — I have not resolved it, only recommended which side the
amendment should land on.**

---

## 3. Item 5 — may STREAM fill ride the cwnd-exempt probe?

### 3.1 §8.5's fill permission

`SPEC.md:3600-3607` (quoted in full at §2.1) — *"then STREAM and DATAGRAM
fill, then PING last among **length-prefixed** frames if a probe still owes
ack-eliciting content"*. The sentence puts fill and a probe's PING in one
packet by construction.

And, far more explicitly, `SPEC.md:3634-3636` (ruling 181):

> Slice 7 makes the collision routine, since **the contested probe** and the PTO probe **both emit PING into packets that may already carry an extends-to-end frame**.

**This is a ratified sentence that is false of the code, and appears never
to have been true of the contested probe.** `pump_contested_probe`
(`mod.rs:3197-3320`) builds its own `Packing` and never calls
`streams.fill`, `datagrams.peek_send`, `streams.pack_control` or `pack_ack`
(the five-line enumeration at §2.2 is exhaustive for the function), so its
packet can never contain an extends-to-end frame. Nor could the pre-250
build: ruling 250's own measurement of it is *"a dedicated 39 B challenge
datagram and then **a bare 31 B PING**"* (`rulings.md`, and
`tests_contested.rs:768-780` restates it). The claim is true of the **PTO**
probe only — that one is built in the pump loop with `probe = true`
(`mod.rs:2151`, `mod.rs:2320`, `mod.rs:2434-2436`) and does carry
stage-3 fill. **Reported, not resolved: §8.5:3634-3636 names a mechanism
that does not exist for one of the two frames it names — working rule 11's
shape, in ratified text.**

### 3.2 §14.5's exemption rationale

`SPEC.md:4845-4851`:

> **[AMENDED 2026/08/17 — ruling 250]** The exemption covers the probe's packet **as built**: a probe that coalesces the owed `PATH_RESPONSE`/`PATH_CHALLENGE` (§7.3) remains exempt — the packet exists because the probe demanded it, **the piggyback adds at most 18 B of frames**, and gating the merged packet would starve the challenge at collapsed cwnd exactly where a roam makes it owed. (The dedicated path-frame packet the coalescing replaces was cwnd-gated; its work now rides the exempt probe.)

The exemption list is introduced as **"Exemptions, exhaustively"**
(`SPEC.md:4831`), which working rule 8 reads as closed.

### 3.3 What the implementation does — and where it diverges

The divergence point is structural, not a guard: **the probe is built
outside the pump loop entirely.** `pump_contested_probe` is called at
`mod.rs:2191` *before* the `loop` at `mod.rs:2201`, with its own `Packing`
(`mod.rs:3206`). Every fill contributor lives inside the loop:

- `mod.rs:2308-2311` — `packing.datagram(data)` (DATAGRAM fill)
- `mod.rs:2320` — `self.streams.fill(&mut packing, &mut packed)` (STREAM fill)
- `mod.rs:2279-2280` — `self.streams.pack_control(...)` (credit grants, RESET_STREAM)
- `mod.rs:2209` — `self.pack_ack(...)`

None of these is reachable from `pump_contested_probe`. **So the
implementation excludes STREAM and DATAGRAM fill from the probe packet, and
does so by construction rather than by a condition that could regress
silently.** (Note the contrast: the **PTO** probe rides the loop and *does*
carry fill, and is *also* cwnd-exempt — `mod.rs:2484`,
`if ack_eliciting && !probe && !self.admits(size)`. That exemption covers
the whole packet, fill included, up to `MAX_PLAINTEXT`.)

The blind test author recorded this gap too, at
`tests_contested.rs:1711-1716`:

> Note what is **not** asserted: whether the stream bytes ride *inside* the probe's packet or in one behind it. §8.5 permits either (the STREAM fill precedes PING among length-prefixed frames), ruling 250 rules on the path frames only, and a test that picked one would be pinning its author's guess.

So: **unpinned by tests, and the author said so.**

### 3.4 Rule 3 — which statement does a proof depend on?

This is rule 3's shape (two spec statements in tension, the implementation
having silently picked one), and the tiebreak rule 3 names is *follow the
statement some other proof depends on*. Applying it honestly:

- **§8.5's fill permission is a placement rule and no proof rests on it.**
  §8.5 says so about itself (`SPEC.md:3609-3612`, ruling 208): it *"decides
  byte placement inside a packet whose size is already settled"*. It orders
  frames that are present; nothing derives a bound from it.
- **§14.5's 18 B clause is inside the argument that earns the exemption its
  widened scope.** Ruling 250 extended a cwnd exemption from "the probe" to
  "the probe's packet as built" and paid for the extension with three
  conjuncts, the second of which is the bound. A full STREAM fill riding the
  probe makes `SPEC.md:4848` **false as written**, and the widening then
  rests on nothing — the exemption would have to be re-argued for an
  unbounded payload.

So the answer is **§14.5**, and the implementation already agrees with it.

**One qualification, stated because it is the honest version.** The 18 B
bound is doing **scope** work, not **safety** work. slither already has a
cwnd-exempt packet carrying up to `MAX_PLAINTEXT` — the PTO probe, whose
§13.4 content is *"pending retransmittable frames oldest-first"* — so
"exempt packets are small" is not a protocol-wide invariant and no
congestion proof depends on one. What the bound buys is that ruling 250's
widening needed **no new congestion argument at all**. Let fill ride the
contested probe and the widening becomes a second, larger claim that nobody
has argued. That is a good reason to state the scope, and a poor reason to
call the alternative unsound — worth saying plainly so the clause is not
later cited as proof of something stronger than it is.

### 3.5 OPTIONS — item 5

**(a) State the scope where §14.5 states the exemption (recommended).**
No behaviour change; makes the exhaustive list say what its own argument
already requires.

*Edit — `SPEC.md`, append to the §14.5 bullet at line 4851, after "(The
dedicated path-frame packet the coalescing replaces was cwnd-gated; its work
now rides the exempt probe.)":*

> **Scope of "as built".** The probe's packet carries the PING and the owed `PATH_RESPONSE`/`PATH_CHALLENGE`, and **nothing else** — no STREAM fill, no DATAGRAM, no credit grant, no ACK. That is what makes the 18 B bound above a fact rather than a hope, and it is the whole of what this exemption was widened to cover. §8.5 orders the frames a packet carries; it does not decide which packet a frame rides in, and its permission for a STREAM fill to sit beside a PING is about the **PTO** probe (§13.4), which is built in the ordinary pump pass and does carry fill. Application data that is ready while a probe is pending leaves in the packet **behind** the probe, on the same pass (§7.3, ruling 250(i)), where §14.5's gate applies to it normally.

*Cross-reference edit — `SPEC.md:3634-3636` (§8.5), replacing "since the
contested probe and the PTO probe both emit PING into packets that may
already carry an extends-to-end frame":*

> since the **PTO** probe (§13.4) emits its PING into a packet that may already carry an extends-to-end frame. The **contested** probe (§7.5) does not: it is built ahead of the pump's ordinary packets and carries only the PING and §7.3's owed path frames (§14.5, §16.5), so the collision cannot arise there.

**(b) Permit fill on the probe** (i.e. build the contested probe inside the
pump loop, the way the PTO probe is built). Saves 30 B of overhead when
application data is queued and the budget is scarce, at the cost of:
re-arguing §14.5's exemption for an unbounded payload; making the probe's
`seal_quiet` treatment (`mod.rs:3258`; §7.4:2558-2562) collide with the
`marking` rule (a first-transmission STREAM frame must mark `last_send` —
`mod.rs:2320-2326`), so a probe carrying fresh stream bytes would have to be
sealed `seal` and would then suppress a keepalive, which §7.5 forbids for
the probe; and a §16.5 row change. **Wire-visible; moves ratified
behaviour; and the `seal`/`seal_quiet` collision is a second ratified
sentence it would have to reopen.** I do not recommend it and record it only
because the item asks for both sides.

### 3.6 RECOMMENDED — item 5

**(a).** The implementation is right, §14.5 is the statement its proof rests
on, and the edit is two clauses of scope. **The §8.5:3634-3636 correction
should travel with it** — that sentence is independently wrong about the
contested probe today, and leaving it would leave a ratified sentence
arguing the position the new clause reverses (working rule 4).

---

## 4. VERDICT

**Item 3 — [40, 49) both-owed coalescing granularity.**
The window is **reachable** (`mobility.rs:177-183` + `mod.rs:2137-2149`
arithmetic; it needs an unvalidated address, both frames owed after a roam,
and `3·recv − sent` ∈ [40, 49)) — so the item does **not** dissolve. Today
the code emits a bare 31 B PING there (`mod.rs:3232`, the predicate
`owed > 0 && packing.room() >= coalesced` with `coalesced == 19`), which is
a faithful reading of the ratified sentence's "otherwise"
(`SPEC.md:2493-2507`). Greedy is better by exactly one 9 B frame, and the
9–18 B it recovers cannot fund any other packet (both residues are under the
30 B minimum datagram). The window is **untested in either direction** —
`tests_contested.rs:1469` pins the *one-owed* [31, 40) window, where the two
policies agree. Option (b) also is not the gate deletion it looks like: it
needs a PING-byte reservation or the probe stops being emitted at 39 B of
room (`frame.rs:1114-1120` forbids packing the PING first).
**RECOMMENDED: (a) — keep all-or-nothing, and add the scope sentence at
`SPEC.md:2507` that says so and says why.** Option (b) is wire-visible and
would need an explicit re-ratification amending `SPEC.md:2493-2503` plus a
row in §1.3's table (`SPEC.md:26`); (a) needs a ratification only for the
added clause, which states existing behaviour.

**Item 4 — where an owed pure ACK rides.**
Established from the code: it rides **the next ordinary packet, behind the
probe, in the same drain** — normally a pure-ACK packet of its own.
`pump_contested_probe` never calls `pack_ack`; `self.pack_ack` has exactly
two call sites, `mod.rs:2209` (the pump loop, after the probe) and
`mod.rs:2816` (`transmit_pure_ack`). It is not suppressed except where the
post-probe residue cannot fund a 30 B datagram, which is §7.3 rank 5 yielding
to rank 2. **Wire-visible and already measured** by the blind author at
`tests_contested.rs:1623-1632` (*"a third pure-ACK packet behind the probe …
48 + 31 + 35 bytes"*), who then filtered `Wire::Ack` out of the assertion
(`:1643-1647`) precisely because ruling 250 does not rule on it.
**CONFLICT REPORTED, NOT RESOLVED:** §12.4 (`SPEC.md:4484`) says *"An owed
ACK rides the **next outgoing packet**"*, and the probe **is** the next
outgoing packet. Three ratified statements back the code — §16.5's
exhaustive contested-row list (`SPEC.md:5073`), §14.5's 18 B piggyback bound
(`SPEC.md:4848`; `MAX_ACK_RANGES` is 64, `src/constants.rs:356`), and §7.3's
rank 5 (`SPEC.md:2428`). §7.3's *"outranks all other output but CLOSE"* does
**not** settle it: a rank governs the budget, not which packet a frame rides
in, and §8.5 says so itself (`SPEC.md:3613-3616`).
**RECOMMENDED: (a) — state the code, in §12.4 at `SPEC.md:4487`** (draft
sentence in §2.6). Not wire-visible; no ratified behaviour moves.

**Item 5 — may STREAM fill ride the cwnd-exempt probe?**
The implementation excludes it **structurally**: the probe is built at
`mod.rs:2191`, before the pump loop at `mod.rs:2201`, with its own `Packing`
(`mod.rs:3206`), and every fill contributor lives inside the loop
(`mod.rs:2209`, `:2279-2280`, `:2308-2311`, `:2320`). Rule 3's tiebreak
picks **§14.5**: §8.5's permission is a placement rule that says of itself it
decides only *"byte placement inside a packet whose size is already settled"*
(`SPEC.md:3609-3612`) and no proof rests on it, whereas §14.5's *"the
piggyback adds at most 18 B of frames"* (`SPEC.md:4848`) is a conjunct of the
argument that earned ruling 250's widening of the exemption from "the probe"
to "the probe's packet as built" — a full fill makes that ratified sentence
false and leaves the widening unargued. Stated honestly: the 18 B clause is
doing **scope** work, not safety work — the PTO probe is also cwnd-exempt and
*does* carry fill up to `MAX_PLAINTEXT` (`mod.rs:2484`), so "exempt packets
are small" is not a protocol invariant, and the clause should not later be
cited as if it were.
**SECOND CONFLICT REPORTED:** `SPEC.md:3634-3636` states that *"the contested
probe and the PTO probe **both** emit PING into packets that may already
carry an extends-to-end frame"*. That is **false of the contested probe**
today and appears never to have been true of it (ruling 250's own measurement
of the pre-250 build is *"a bare 31 B PING"*). Working rule 11's shape, in
ratified §8.5.
**RECOMMENDED: (a) — add the scope clause at `SPEC.md:4851`** (draft in
§3.5), **and carry the `SPEC.md:3634-3636` correction with it** so a ratified
sentence is not left arguing the reversed position (working rule 4). Not
wire-visible; no ratified behaviour moves.

**Cross-item note.** Items 3, 4 and 5 are three faces of one unstated
sentence: *what may ride the contested probe's packet, and on what
condition.* If only one edit is made, make it the positive enumeration —
"the PING, the owed path frames when room admits, and nothing else" —
placed at §14.5:4851 where the exemption's scope is argued, cross-referenced
from §12.4:4487 and §8.5:3636. Items 3's option (a) is then a second
sentence about *granularity* over that same owed set.

**Nothing in the recommended set is wire-visible or moves ratified
behaviour.** The only wire-visible option in the trio is item 3's (b), and
it is the one I recommend against.

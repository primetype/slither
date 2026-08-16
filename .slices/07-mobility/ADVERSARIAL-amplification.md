# Adversarial review — anti-amplification, migration, reflection (§7.3/§7.4/§7.5)

Reviewer lens: abuse, not admiration. Findings ranked by severity at the end.

## 0. Method / scope

- (in progress)

## 1. Attacking ruling 168 (address validated ONLY by ACK covering `validation_floor`)

- (in progress)

## 2. Budget reset at validation — validate-then-roam re-arm

- (in progress)

## 3. Per-session budget × N sessions to one victim address

- (in progress)

## 4. Other findings

- (in progress)

## 5. Sub-areas where I found nothing

- (in progress)

## 6. Ranked findings

- (in progress)

---

## Working notes (appended live)

### N0. Facts established from `src/core/connection/mobility.rs` (whole file read)

- `Amplification { validated: bool, floor: u64, sent: u64, recv: u64 }` (L66-77).
- `arm(floor, credit)` sets `sent = 0`, `recv = credit` (L102-109).
- `admits(len)` = `validated || sent + len <= 3 * recv` (L119-124). Boundary inclusive.
- `on_ack_covering(largest)`: `if !validated && largest >= floor { *self = Self::validated() }` (L150-154).
  **The whole struct is replaced — `sent` and `recv` are zeroed, `floor` zeroed.**
- `set_floor` exists because the msg1-anchor arming happens in `established()` before the
  session is installed (L161-172).
- `Contested::{No, Pending{floor}, Armed{floor,armed_at,deadline}}` (L192-218).

### N1. Facts from SPEC §7.3 (L2079-2267)

- Budget arms at exactly two events: a committed roam, and the msg1 anchor.
- Counters are **per unvalidated address, per session** (ruling 170); residual N-multiplier
  stated at L2131-2138.
- `validation_floor` = `DatagramSend::next_counter()` at the arming (L2147-2149).
- Validation predicate (L2151-2153): "first **authenticated, window-fresh packet from that
  address** carrying an ACK that covers **any counter >= validation_floor**".
- Claimed guarantee (L2161-2168): "An ACK at or above the floor can only have been minted by
  a peer that **received a packet we sent to that address after the change**".
- Priority list L2225-2232 (CLOSE, contested probe, pure ACKs, PTO, keepalives, rtx, new data).
- L2191-2199: "the 3x ratio itself is never raised ... What ends is the unvalidated state."

### N2. Facts from `src/core/connection/mod.rs`

- `handle_datagram` L415-513. Roam predicate L466. Then L489-499:
  roam => `commit_roam(now, from, src, datagram.len())`, else `on_recv(datagram.len())`.
- L508: `let from_anchor = self.remote_address() == Some(src);` — computed **after** the roam,
  so the roaming packet itself is "from anchor".
- `commit_roam` L1190-1215: `let floor = self.next_counter().unwrap_or(0);
  self.amplification = Amplification::arm(floor, datagram_len);`
- `on_ack_frame` L1218-1234: guard `if ack.largest <= highest_sealed { self.on_ack_coverage(...) }`
  where `highest_sealed = next_counter() - 1`.
- `on_ack_coverage` L1263-1270: `if from_anchor { self.amplification.on_ack_covering(largest); }`

**Checked and clean:** the roaming packet cannot validate its own roam. `floor` is
`next_counter()` at the roam = `highest_sealed + 1`, and `on_ack_frame` refuses any
`largest > highest_sealed`. So `largest <= floor - 1 < floor`. Tight.

---

## FINDING 1 (CRITICAL) — ruling 168's return-routability proof is forgeable by the exact adversary §7.3 exists to stop

**The defect.** `validation_floor` is disarmed by an **authenticated** packet from the
anchor carrying an ACK whose `largest >= floor`. An ACK frame's `largest` is *not* a
cryptographic proof of receipt — it is a **plaintext field chosen by whoever holds the
session key**. §7.3's roaming adversary **is** the key holder: §7.3 L2094-2100 states the
threat as *"a peer-supplied address becomes a send target with no return-routability
proof"*. The peer supplies the address. The peer holds the key. The peer therefore mints
the proof.

**Concrete sequence** (M = malicious keyed peer, V = victim address, both `SocketAddr`s
M chooses):

1. M completes a normal IK handshake from its own address. Session live, keys held by M.
2. M seals one Data packet on its own send counter and emits it with the **IP source
   spoofed to V**. It is authenticated (M has the key) and window-fresh (M picks the
   counter), so `handle_datagram` L466 commits the roam: anchor := V,
   `Amplification::arm(floor = next_counter(), credit = len)` (mod.rs L1205-1206).
3. We now owe at least an ACK for that packet. `pump_packets` seals it — budget admits it,
   `3 x len` is ample — so `highest_sealed >= floor`.
4. M seals a **second** packet, again spoofed from V, containing an `Ack` frame with
   `largest = floor` (M knows `floor`: it is one above the highest counter M has ever seen
   from us, and M may simply try `floor`, `floor+1`, ... — the guard only rejects
   `largest > highest_sealed`, and a rejected try costs nothing).
   - `src == anchor` (V), so **no roam**, and `from_anchor == true` (mod.rs L508).
   - `on_ack_frame` guard passes: `largest <= highest_sealed`.
   - `on_ack_coverage` -> `on_ack_covering(largest)` -> `largest >= floor` ->
     `*self = Amplification::validated()` (mobility.rs L150-154).
5. **The budget is gone.** Every subsequent byte we send to V is unbounded by §7.3.

**Cost to us.** §7.3's stated purpose — L2197-2199, *"it forces an attacker to pay a third
of any flood it reflects, removing the reflection incentive at zero protocol machinery"* —
is **defeated at a cost of two small packets**. Pre-168 the reflector was bounded at 3x and
M had to keep paying 1/3 of the flood for its whole duration. Post-168 M pays O(1) once and
then reflects at whatever rate the connection can produce.

**It compounds with forged ACKs on the congestion controller.** Once validated, the only
remaining rate limit at V is cwnd, and M drives cwnd with the same forged ACKs (it can
acknowledge everything up to `highest_sealed` instantly, so RTT samples collapse and the
window grows per forged round). That is an optimistic-ACK attack whose *target is a third
party*, which is a strictly worse shape than the usual self-directed one.

**Where the rationale goes wrong, precisely.** SPEC L2165-2168: *"an ACK's coverage derives
from the peer's replay window (§12.2), which cannot contain a counter the peer never
received, and **an attacker** holds only packets we sealed before the floor."* That sentence
silently models the attacker as a **keyless third party** while the rule it justifies governs
an address supplied by the **keyed peer**. "Coverage derives from the peer's replay window"
is a description of an *honest* implementation's behaviour, not a constraint the wire
enforces. Compare L2206-2212, which rejects the N-packets alternative because an ACK
*"cannot be manufactured without the key"* — true, and irrelevant, because the adversary
has the key.

This is working rule 11 in its purest form: **the rationale names a mechanism ("the peer's
replay window constrains the ACK") that does not exist on the wire.**

**Confidence: very high** on the mechanism (verified against mod.rs L1218-1270 and
mobility.rs L150-154: the only checks are AEAD, window-freshness, `src == anchor`, and
`largest <= highest_sealed`). The one thing I have not verified is whether `frame::Ack`
carries any field that could constrain forgery — checked next.

**Confirmed:** `frame::Ack` (src/core/connection/frame.rs:449-459) is
`{ largest, ack_delay, first_range, ranges }` — four plaintext integers. **Nothing in an
ACK proves receipt.** Finding 1 stands.

---

## FINDING 2 (HIGH) — msg2 is sent to the unvalidated msg1 address *outside* the budget, so the real responder ratio is 3.55x, not 3x

**The defect.** §7.3 L2103-2105: *"total bytes sent to the address MUST NOT exceed
`AMPLIFICATION_FACTOR` (= 3) x total bytes received from it"*. The **msg2 response**
(`RESP_PACKET_LEN` = 107 B) is emitted by the **endpoint**, not the connection:

- `src/core/endpoint/staged.rs:753` — `self.emit(EndpointOutput::Transmit(Transmit { to: anchor, data }))`
- `src/core/endpoint/routing.rs:543` — same, for §6.6's tie-break-loser admit

Both fire **before** `Connection::established` / `install` exist. And the arming is
`Amplification::arm(0, INIT_PACKET_LEN)` (mod.rs L257, L1748) — `sent` starts at **0**.
Ruling 170 states there is *"no endpoint-side per-address table"*, so nothing else counts
it either. The 107 bytes of msg2 are **never charged to any budget**.

**Concrete sequence.** A spoofer that knows the responder's static public key (mac1 is
keyed on public data, so mac1 is no barrier) sends one 196-byte msg1 with source spoofed to
victim V:

1. Endpoint admits, emits msg2 (107 B) to V — **uncharged**.
2. `Connection::established` arms the budget at `recv = 196`, `sent = 0`, cap `588`.
3. The connection may now emit up to a further **588 B** to V (keepalives, and whatever
   ruling 203's in-flight fix makes it send) before the cap binds.
4. Total to V: **695 B for 196 B received = 3.55x**, above the normative 3.

**Why it is the rule-8 shape.** §7.3 arms on *"an accepted initiation's msg1 anchor"* and
credits the initiation's bytes, but never says what account the **response to that
initiation** is drawn on. The construction is stated; its scope is not. The
`Amplification::arm(floor, credit)` signature has no `already_sent` parameter, which is
where the omission became structural.

**Cost.** 18% over the ratified ratio on every responder-side connection, and it is the
*only* part of the responder's output an attacker gets for free with a purely spoofed,
keyless packet. Not catastrophic on its own; it is a normative MUST violated by
construction on the most-exercised path in the protocol.

**Confidence: high** on the accounting (verified: `arm` sets `sent = 0`; both msg2 emit
sites are endpoint-side and precede the connection). Medium on the exact reachable total,
which depends on what the responder actually emits before the peer answers.

---

## FINDING 1b (CRITICAL, same root) — the *keyless* on-path attacker that ruling 203's own rationale names also defeats the proof, by relaying **one** packet

Ruling 203 (rulings.md L5611-5619) re-confirms 168 with: *"an authenticated, window-fresh
packet proves the peer **sent** it, and an on-path attacker can rewrite its source to a
victim's address — only an ACK proves the peer **receives** at the address we are sending
to."*

That attacker defeats it without any key:

1. On-path attacker A (malicious AP / NAT / ISP — anything that sees our egress) rewrites
   the source of one genuine peer->us Data packet to **V**. We roam to V, `floor` recorded.
2. We send to V. A is on our egress path, so it **captures** that datagram and **forwards
   it to the real peer**. One packet. It forges nothing.
3. The peer ACKs normally. A rewrites that ACK's source to V and delivers it. `src == anchor`,
   authenticated, window-fresh, `largest >= floor` -> **validated**.
4. A now drops everything and we send to V unbudgeted. A relays one small peer packet every
   `< DEAD_TIMEOUT` (25 s) to keep liveness alive and cwnd fed.

The "proof of return routability" proves only that **a packet we sent toward V reached the
key holder** — never that anything **at V** received it. Those are the same statement only
if no one between us and V can carry a packet, which is exactly the assumption the attacker
in the rationale's own sentence violates. `validation_floor` is a **reachability-of-the-peer**
proof, not a **reachability-of-the-address** proof. The distinction is the whole rule.

**What it costs, versus the pre-168 rule.** Pre-168 the reflector was permanently capped at
3x and the attacker paid 1/3 of the reflected volume, continuously, forever. Post-168 the
attacker pays **one relayed packet** and the cap is gone.

---

## FINDING 3 (MEDIUM) — a closing/draining connection credits the budget from **any** source address; the sibling clause four lines below gets this right

`src/core/connection/mod.rs:489-499`:

```rust
match roamed {
    Some(from) => self.commit_roam(now, from, src, datagram.len() as u64),
    None => self.amplification.on_recv(datagram.len() as u64),
}
```

`roamed` is `None` under **two** distinct conditions (L466): `src == anchor`, **or**
`!live`. On a live connection the collapse is sound — `None` implies `src == anchor`. On a
**closing or draining** connection (§15.2 forbids roaming) `roamed` is `None` for **every**
source, so a datagram from an arbitrary address credits `recv` for an unvalidated address it
did not come from. §7.3 L2103-2105 funds the budget from *"total bytes **received from it**"*.

**The author already had this exact insight and applied it to only one of the two
counters.** Nineteen lines later, L503-508:

```rust
// A roam commits only on a live connection, so `src` is the
// anchor here whenever the packet reached one — but ruling 168's
// proof is stated *"from that address"*, and a closing connection
// that does not roam can still receive from elsewhere.
let from_anchor = self.remote_address() == Some(src);
```

Same premise, same sentence, applied to `on_ack_covering` and **not** to `on_recv`. This is
working rule 4(a)'s shape: one clause corrected, its sibling left.

**Sequence.** Roam the connection to V (spoofed source on a harvested authenticated packet),
then provoke a close (a violating frame stream — §6.9 prices this at *"one parse + one CLOSE
seal + the 5 s linger"*). While closing, feed authenticated window-fresh packets from
anywhere; each credits `3 x len` of budget spendable **only at V**, where the CLOSEs go
(`transmit_close`, mod.rs L1630-1634, sends to the anchor).

**Cost.** Bounded by §15.2's CLOSE rate rule (<= 1 per second, 5 s linger) — at most ~5
CLOSE datagrams. Small in volume; it is a normative deviation, not a practical flood.

**Confidence: high** on the code path, **high** that it is a deviation from §7.3's *"from
it"*, **low** that it matters operationally.

---

## FINDING 4 (LOW-MEDIUM, and it gets worse under ruling 203's in-flight fix) — the budget's unit is UDP payload; reflection is measured on the wire, and slither's output is systematically more fragmented than its input

`on_recv(datagram.len())` and the charged `size = DATA_HEADER_LEN + plaintext + AEAD_TAG_LEN`
(mod.rs L1901) are both **UDP payload** bytes. mobility.rs L31-32 states the unit as
*"datagram bytes in both directions"*. IP + UDP headers — 28 B on v4, 48 B on v6 — are
outside the count, **per packet**.

The attacker's input is **one large packet**; our budgeted output is **many small ones**. On
the msg1-anchor path: 196 B in (224 B on-wire v4) buys 588 B of payload out. Discharged as
19 minimum-size Data packets (`DATA_HEADER_LEN` 14 + `AEAD_TAG_LEN` 16 = 30 B each) that is
19 x 58 = 1102 B on-wire, plus finding 2's uncharged msg2 (135 B): **1237 B out for 224 B in
= 5.5x on IPv4, ~6.7x on IPv6** — against a ratified factor of 3.

Today this is not a practical flood, because a responder with nothing to say emits its
output on 10 s keepalive intervals, not in a burst. I am reporting it for a different
reason: **ruling 203's fix makes it structurally worse.** That fix bounds the packing target
by the remaining budget (`min(MAX_DATAGRAM, room)`), which is a deliberate instruction to
emit *smaller, more numerous* datagrams precisely while an address is unvalidated. Every
packet the fix shrinks adds an uncounted 28-48 B to the reflected total. Ruling 207 already
enumerates three ways the fix can be built wrong; the unit gap between payload bytes and
wire bytes is a fourth, and it is the one that only appears once the fix works.

**Confidence: high** on the arithmetic, **high** that the spec says nothing about it (§7.3
never bounds packet *count*, only bytes — rule 8's shape), **medium** on operational
significance today, **high** that ruling 203's fix amplifies it.

---

## 5. Sub-areas where I found nothing

Stated explicitly rather than padded.

- **Credit laundering across addresses: clean.** `commit_roam` (mod.rs L1206) replaces the
  whole struct, so credit accrued at one address never carries to the next. The
  validate-then-roam re-arm (`Self::validated()` zeroes all four fields, mobility.rs L152)
  is **not** exploitable in the laundering direction: a roam re-arms `sent = 0, recv = len`
  either way, so nothing an attacker accumulates at address A is spendable at address B.
  The residual flagged at rulings.md L5001-5005 is, on inspection, benign in isolation —
  its danger is entirely that it makes *validation* the only thing that matters, and
  finding 1 says validation is free.
- **Roam-flap starvation: no new capability.** Repeated roams do keep the connection
  permanently unvalidated and permanently capped, but every roam requires an authenticated,
  window-fresh packet, so only §6.8's harvesting attacker or an on-path relabeller can drive
  it — and both already own the connection's fate by simpler means. Ruling 171 prices this
  and its priority rule is the right lever.
- **The roaming packet cannot validate its own roam.** `floor = next_counter()` at the roam
  and `on_ack_frame`'s `largest <= highest_sealed` guard are exactly complementary. Verified.
- **§7.7's counter space really is never reset** (SPEC L2884-2890: *"the counter is **never
  reset** by the ratchet"*), so the floor's high-water argument survives the epoch ratchet.
  I checked this because a counter reset would let a harvested old ACK validate a fresh
  floor; it does not happen.
- **No unauthenticated reflection primitive.** `EndpointOutput::Transmit` is emitted at
  exactly three sites — `endpoint/mod.rs:560` (our own msg1, to an address *we* chose),
  `endpoint/staged.rs:753` and `endpoint/routing.rs:543` (msg2). slither never replies to an
  unroutable, mac1-invalid, or undecryptable datagram. There is no stateless reset, no
  version-negotiation packet, no error datagram. This is the single best anti-reflection
  property in the design and it is worth keeping.
- **All five connection-side output paths are budget-gated**, with no exemption:
  `transmit_close` (L1657), `pump_packets` (L1914), `transmit_pure_ack` (L2072),
  `transmit_keepalive` (L2223), `pump_contested_probe` (L2361). Each pairs `admits` with
  `on_sent`. §14.5/§13.4's cwnd exemptions are correctly kept outside the budget check
  (L1904-1908). The only unbudgeted output is finding 2's msg2.

## 6. The per-session multiplier, answered directly

The brief asks what real multiplier a determined attacker gets from N sessions.

**N sessions do not raise the ratio.** Each received packet decrypts under exactly one
session and funds exactly that session's `recv`. The aggregate over N sessions is still
3x aggregate input. Ruling 170's residual — *"N sessions ... multiply the reflector by N"*
(SPEC L2131-2138) — is a statement about **absolute volume at a fixed victim**, and the
attacker's cost scales by N as well. It is correctly declared, and it is not the leverage
point.

The real multiplier is:
- **3x payload / ~5.6x on-wire (v4), ~6.8x (v6)** while an address stays unvalidated
  (findings 2 and 4), for a keyless spoofer;
- **unbounded** the moment the address validates, which finding 1 shows costs one packet
  for a keyed peer and one relayed packet for an on-path attacker.

The N-session residual is a rounding error next to that.

## 7. Ranked findings

| # | Severity | Finding | Confidence |
|---|---|---|---|
| 1 / 1b | **Critical** | Ruling 168's `validation_floor` proves the **peer** received, not that the **address** received. A keyed peer forges the ACK outright; a keyless on-path attacker gets it by relaying one packet. Either way the 3x cap is removed for O(1) cost, defeating §7.3's stated purpose. | Very high |
| 2 | **High** | msg2 (107 B) is emitted endpoint-side to the unvalidated msg1 anchor and charged to no budget; `arm` starts `sent` at 0. Real responder ratio 3.55x against a normative MUST of 3. | High |
| 3 | Medium | A closing/draining connection credits `recv` from **any** source (`match roamed` collapses `src == anchor` with `!live`), while the `from_anchor` guard 19 lines below gets the identical question right. Bounded by §15.2's CLOSE rate. | High (path), low (impact) |
| 4 | Low-Medium | The budget counts UDP payload; reflection is measured on the wire. One large packet in, many small packets out — and **ruling 203's in-flight fix deliberately makes the output smaller and more numerous**. | High (arithmetic), medium (impact today) |

**What I did not re-report**, per the brief: ruling 203's sizing defect, and ruling 198's
unscheduled priority positions 3-7.

**What I am not resolving**, per working rule 3: finding 1 is a conflict between §7.3's
*rationale* (L2161-2174, and ruling 203's L5611-5619 restatement) and §7.3's *purpose*
(L2197-2199). The rationale's mechanism does not exist on the wire. Whether the answer is to
reverse 168, to bound the validated state, to require the validating ACK to acknowledge a
counter sealed **to that address specifically**, or to accept the exposure and say so, is a
ratification decision, not mine.

### One suggestion, offered because ruling 168 correctly identified a real problem

Ruling 168's underlying observation is right: the permanent cap is untenable. But note that
the thing which *does* bind an on-path relabeller and a lying peer is not the validation
predicate — it is the **ratio applied continuously**. A rule of the shape "the unvalidated
state ends on a covering ACK, **and** the 3x ratio continues to bind output to any address
the session has roamed to within the last N seconds" would keep 168's throughput fix (the
downloading peer's ACKs validate in one RTT, and thereafter cwnd governs a *stable* address)
while retaining a cost floor for an attacker that keeps moving the target. I flag this only
because rule 11 requires a rationale to name a mechanism that exists, and the cheapest
repair to 168 is to stop making the *unvalidated state* carry the whole burden.

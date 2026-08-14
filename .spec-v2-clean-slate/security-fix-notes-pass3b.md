# Security-fix notes — pass 3b (rulings 36, 38 + SECV5-2, SECV5-8, SECV5-9)

Applied to `SPEC-v2-DRAFT.md` on 2026/08/14, on top of pass 3a
(rulings 35 and 37). Rollback copy: `SPEC-DRAFT-v5-post-pass2-frozen.md`
(pre-pass-3a).

Wire-free pass: no constant, layout, frame type, error variant, timer or
size moves. `PERSISTENT_KEEPALIVE` stays 25 s, floor stays
`DEAD_TIMEOUT`-relative.

---

## 1. SECV5-2 — the death clock's state at install (§7.4) — LANDED

Two new paragraphs in §7.4.

**The pin.** *"At install the clock is pinned, and it is pinned
**armed**."* A newly installed session sets both `last_authenticated_recv`
and `last_send` to the install instant and starts with the death deadline
already armed — the handshake is the arming event, so the death rule's
second conjunct holds from install onward.

Two consequences spelled out:

1. A session that receives nothing after install dies at
   install + `DEAD_TIMEOUT` whether or not the application ever sends.
   Named as the load-bearing fact under §6.7, §17.1 (twice) and §15.4's
   endpoint-dropped row, with the counterfactual stated: unarmed at
   install ⇒ immortal, because §7.6 is deleted and liveness is the only
   reaper.
2. With `last_send` = `last_authenticated_recv`, §7.5's passive rule
   (*received since it last sent*) is **false** until the first
   authenticated receive, so a half-open session emits nothing at all.

**Judgment call (recorded):** the ruling directed only "armed at
install"; I additionally pinned `last_send` to the install instant. Two
reasons. (a) The review's reading A (`last_send` unset/−∞) and reading B
(both = install) differ **observably on the wire** — under A a half-open
responder emits a keepalive at install + 10 s, under B it emits nothing —
so leaving it open leaves two conformant implementations wire-divergent,
which is exactly the class of gap SECV5-2 is. (b) Reading B is the
security-preferable one: a replayed msg1 must not turn us into a
keepalive source aimed at a spoofed address. Armed-at-install removes the
only reason reading A was needed (it was the reading under which the
passive keepalive supplied the arming send), so B is now free of cost.
This is a behaviour pin, not a wire change: no constant, packet or timer
moves.

**Dependent claims verified afterwards** (read, not edited): §17.1's two
"reaped by liveness in 25 s" clauses, §6.7's replay bound, §15.4's
endpoint-dropped row — all now true as written. (§6.7 and §17.1's pinning
bullets are pass-3a/reconciliation territory and were **not** edited.)

## 2. Ruling 36 — the general principle (§7.4) — LANDED

New paragraph *"What the death clock measures — and the one thing it does
not"*, placed with the liveness model rather than with the probe: the
clock is driven by **mere authenticated receipt, never by acknowledged
progress**. States the full mechanism (withheld genuine packet is
genuine ⇒ opens; window never advanced ⇒ fresh; buys another full
`DEAD_TIMEOUT`; and roams the session to the injector because roaming
keys on authenticated receipt too, §7.3), states that every "is this peer
still there?" question in the document inherits it, and names the
contested probe as the single deliberate exception.

## 3. Ruling 36 — the contested-connection probe — LANDED

Four homes, one mechanism.

**§7.5 (normative home).** New bullet *"The contested-connection probe"*
after the PING bullet, in three paragraphs:
- **The rule.** A §6.4 refusal against a `None` basis marks the
  connection **contested**; the endpoint immediately sends an
  ack-eliciting PING (§8.3) and requires an **ACK covering the packet that
  carried that PING** within `KEEPALIVE_TIMEOUT`. ACK ⇒ mark clears, the
  refusal stands, basis rule untouched. No ACK ⇒ `ConnectionLost::TimedOut`
  (existing variant, §15.4's liveness row, nothing transmitted), static
  drops to NONE, parked `Intro` takes an ordinary fresh accept on the
  application's next attempt.
- **Why an ACK rather than a receive.** Full attack stated (harvest +
  drop so the window never advances, drip one packet every <
  `DEAD_TIMEOUT` off-path, session roams to the injector, reconnects
  refused, `connect()` → `AlreadyConnected`, only escape is `close()`),
  then the reason it works: withheld genuine Data can reset a receive
  clock but can never produce a fresh ACK of a packet sent *after* the
  harvest, its counter being greater than anything the attacker holds.
- **Cost.** One PING per refused basis-`None` accept; refusals are
  application-driven `accept()` calls, so the rate is bounded by the
  application and by nothing an attacker controls. Multiple marks allowed,
  each with its own deadline, death at the first that expires. Timing
  interlock recorded: the 10 s deadline sits inside `INTRO_TTL`'s 15 s and
  the peer re-mints an initiation every ≈ 5 s, so the provoking `Intro` is
  still parked when the verdict lands.
- The PING bullet itself now reads "the ack-eliciting PTO probe (§13.4)
  **and the carrier of the contested-connection probe below**".

**§14.5.** The exemption list is declared *exhaustive*, so the probe is
added to it as its own bullet (a gate that could delay the probe past its
own deadline would turn a congestion answer into a liveness verdict). The
existing cwnd-scoped-only clause still binds it to §7.3's
anti-amplification budget at an unvalidated address.
**Judgment call (recorded):** the ruling did not mention the cwnd. Left
un-exempt, a cwnd-blocked probe could never be sent and the deadline
would fire regardless — the right verdict in the attack case but reached
for the wrong reason, and un-derivable in general. Exempting it is
wire-free and keeps §14.5's "exhaustively" honest.

**§6.4 (the marking, as directed — nothing else in §6.4 touched).** The
basis-refusal bullet gains "one exception to *untouched*": a `None`-basis
refusal marks the connection contested, with the mechanism deferred to
§7.5. Explicitly notes that a `Some(t)`-basis refusal marks nothing (the
basis can decide there) and that the refusal itself stands either way.
§6.4's existing `[RATIFIED 2026/08/14, amended 2026/08/14]` marker already
carries the amended stamp from pass 3a — not duplicated.

**§6.8 (the premise).** "delayed by at most `DEAD_TIMEOUT`" now carries
its premise inline (*it holds when nothing authentic is still reaching the
zombie*), and a new paragraph states that against an attacker the premise
is false and the wedge is permanent, naming the contested probe as what
restores the bound — and restating the bound honestly: bounded by the
application's next `accept()` plus `KEEPALIVE_TIMEOUT`, not by
`DEAD_TIMEOUT`.

**§17.4 (the basis prose).** New paragraph *"What a `None` basis costs,
stated correctly"* replacing the bounded-delay framing: the price is not
a delay but a permanent wedge under an attacker, which is why ruling 36
attaches the probe to precisely this refusal. Plus the directed footnote:
the root-cause fix (12-byte timestamp in msg2 ⇒ `Some(t)` on both sides)
was **considered and declined as wire-affecting** (`IK_MSG2_LEN` /
`RESP_PACKET_LEN` move), so the probe is a mitigation, not a closure — it
guarantees the zombie dies, not that a dialled connection can measure a
replacement. Not re-argued.

## 4. Ruling 38 — `PERSISTENT_KEEPALIVE` documented as inert — LANDED

**§5.7, the inverted sentence — corrected.** Was: *"the floor's job is
only to reject an interval so long that the beacon could not keep a
connection alive on its own"* (describes rejecting **long** intervals,
while the floor rejects **short** ones). Now: the floor's job is §7.5's —
a persistent keepalive is a **marking** send and the floor keeps a
marking beacon from **outpacing the death clock**; *"it rejects intervals
that are too short, never ones that are too long."* The paragraph's
closing sentence (*"whether to raise the default for margin is a
maintainer call"*) is replaced by ruling 38's disposition: the beacon is
inert at every admissible interval, **nothing moves**, and the knob is
documented as a no-op rather than deleted or re-based.

**§5.7's timer table**, two derivation cells (values untouched):
`DEAD_TIMEOUT`'s cell now says the tolerance is one lost keepalive *in
one direction*; `PERSISTENT_KEEPALIVE`'s says *inert under the current
liveness model (§7.5, ruling 38)*.

**§7.5, the derivation.** Appended to the persistent-keepalive bullet,
whose marker becomes `**[RATIFIED 2026/08/14, amended 2026/08/14]**`, and
whose own floor sentence gains "it excludes intervals that are too
**short**, never ones that are too long". The derivation is the ruling's,
verbatim in structure: `I`, `S` = `last_send` (marking only), `R` =
`last_authenticated_recv`; beacon at `S + I`, re-armed by marking sends,
untouched by receives; while `R > S` the passive keepalive fires at
`S + KEEPALIVE_TIMEOUT`, is itself marking, drags `S` forward in 10 s
steps and pushes the beacon's deadline with it; the only blocking state
is `S > R`, reachable only by a marking send, which arms the death clock,
so death is at `R + DEAD_TIMEOUT` while the beacon's deadline is
`S + I > R + I ≥ R + DEAD_TIMEOUT` for every `I ≥ DEAD_TIMEOUT` —
**strictly after death, for every interval the floor admits**. Plus the
NAT-purpose note (a connection that is not receiving is killed at 25 s
anyway now that §7.6 is deleted) and **both declined alternatives**
(re-base the floor on `KEEPALIVE_TIMEOUT`; delete the knob) recorded as
declined so they are not re-proposed.

**Nothing moved:** default 25 s, floor `DEAD_TIMEOUT`-relative, knob
retained, §7.5's constant table and the consolidated Named-constants
table untouched.

## 5. SECV5-8 — the tolerance is unidirectional — LANDED

**§7.5's Liveness bullet** now states the residual precisely: tolerance
is one keepalive lost **in one direction** (with the worked case — A
loses at t = 10, B's arrives, A re-keepalives at t = 20, 5 s of margin),
while a **simultaneous bidirectional** loss (one event, two packets, one
interval) is not tolerated at all: both sides hold `last_send` >
`last_authenticated_recv`, the passive rule's first conjunct is false on
both, the rule is one-shot per receive, keepalives are never
retransmitted (§8.7's *never* class, never in the sent map), and both die
at t = 25. Named as reachable in honest operation: flapping path, NAT
rebind, interface hiccup.

**§5.7's derivation paragraph** gets the same correction, in short form,
and the table cell above with it. The stale *"only two **consecutive**
losses"* framing is gone from both sites (and from Appendix B).

## 6. SECV5-9 — §10.3 carries both halves of ruling 33 — LANDED

§10.3's *"Credit frames are ack-eliciting … yet liveness-neutral (§7.4)"*
now spells out ruling 33's exact sense, both halves: **non-marking**,
`seal_quiet`, `last_send` untouched, defers no keepalive; **and**, being
ack-eliciting, **arms the death clock**. Closing sentence: *"A connection
whose only output is credit is not thereby exempt from dying."*

## 7. Appendix B — test obligations — LANDED

Under **Liveness and amplification**, the existing liveness-anchor bullet
is amended and two obligations are added:

- **Amended (SECV5-8):** the tolerance obligation now reads "one keepalive
  lost **in one direction**", and requires the bidirectional
  counterexample — drop both directions' keepalive in the same interval,
  assert both sides fire `TimedOut`, since neither can re-fire the
  one-shot passive rule and keepalives are never retransmitted.
- **New — the clock is armed at install (SECV5-2):** install a session,
  drive **nothing**, assert `ConnectionLost::TimedOut` at exactly
  install + `DEAD_TIMEOUT` on the paused clock **and** that nothing is
  transmitted in the interim (the `last_send` half of the pin). Names the
  responder-side half-open shape (accept a replayed initiation whose
  initiator never speaks again) as the shape that matters, since it is
  the obligation under §6.7's and §17.1's "reaped by liveness in 25 s".
- **New — the contested-connection probe (ruling 36):** with a **dialled**
  (basis `None`) connection live, park an `Intro` for the same static and
  `accept()`; assert `Stale`, assert a PING goes out immediately, then
  both paused-clock outcomes (ACK inside `KEEPALIVE_TIMEOUT` ⇒ alive and
  refusal stands; silence ⇒ `TimedOut` at the deadline, `Intro` then
  accepted normally). The third test is the security case: feed the zombie
  **withheld genuine Data** every 5 s throughout and assert it *still*
  dies at the probe deadline — a reset receive clock must not satisfy the
  probe.

## 8. Preamble — LANDED

**Pass 3b entry appended** to the "Security-fix revisions (draft v5 →
v6, 2026/08/14)" block, after pass 3a's, headed "**Pass 3b — rulings 36
and 38, plus three directed fixes.**" Covers ruling 36 (the attack, the
mechanism, the unchanged refusal, the declined msg2-timestamp root fix),
ruling 38 (the inverted sentence, the derivation, nothing moves, both
declined alternatives), and the three directed fixes (SECV5-2's
armed-at-install pin, SECV5-8's unidirectional restatement, SECV5-9's
§10.3), plus §7.4's general principle. **Block finalised** with: "With
pass 3b the round closes: every ruling the maintainer issued against the
re-review is applied, and the wire is byte-identical to draft v5's."

The v4 → v5 block's flagged-coincidence paragraph — which still read as an
open maintainer call on `PERSISTENT_KEEPALIVE`'s default — keeps its
historical text and gains a one-clause forward pointer *(Ruled in pass 3b
below — ruling 38 …)*. History is not rewritten; it is cross-referenced.

---

## Verification

| Check | Result |
|---|---|
| `[MAINTAINER]` markers | **2** — unchanged (preamble + Appendix A.1) |
| `[RATIFIED` markers | **29** — unchanged; one gained "amended 2026/08/14" (§7.5's persistent-keepalive bullet), none added |
| Code fences | **48** — even, balanced |
| `IK_MSG1_LEN` / `IK_MSG2_LEN` | 174 / 81 — intact (§2.3, Named constants) |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` | 196 / 107 — intact |
| `MAX_DATAGRAM` / `MAX_PLAINTEXT` | 1200 / 1170 — intact |
| `VERSION` / `PROLOGUE` | `0x01` / `b"slither\x01"` — intact |
| `DEAD_TIMEOUT` / `KEEPALIVE_TIMEOUT` | 25 s / 10 s — intact, both tables |
| `PERSISTENT_KEEPALIVE` | 25 s default, floor `DEAD_TIMEOUT`-relative — intact |
| "15 s" occurrences | **7** — the same 7 (5 × `INTRO_TTL`, §16.10's paused-clock timer family, 1 revision-history entry); §6.9's `1024 / 15 s` untouched |
| New error variants | **none** — probe fires the existing `ConnectionLost::TimedOut` |
| Frame types | none added — the probe rides the existing PING (§8.3) |
| Over-long lines introduced | none (checked > 84 cols outside fences/tables) |

**Sections touched:** preamble, §5.7, §6.4, §6.8, §7.4, §7.5, §10.3,
§14.5, §17.4, Appendix B.
**Verified untouched (forbidden set):** §6.6, §6.7, §17.1, §18.1,
Appendix A, §16.1, the consolidated Named-constants table, §7.5's
constant table values, §16.10.

## Judgment calls, collected

1. **`last_send` pinned to install alongside the armed flag** (§7.4) —
   see §1 above. The ruling directed "armed at install"; the two
   candidate readings differ observably on the wire, so leaving
   `last_send` open would have left the gap half-closed. Reading B (both
   = install) chosen: it is the security-preferable one and armed-at-
   install removes the only reason reading A existed.
2. **The probe exempted from the congestion admission gate** (§14.5) —
   see §3. Required because §14.5's exemption list is declared
   *exhaustive*; without the exemption a cwnd-blocked probe silently
   converts a congestion state into a liveness verdict.
3. **Multiple contested marks resolve by earliest deadline** (§7.5) — the
   ruling specifies one probe per refusal but not what a second refusal
   does while one is outstanding. Pinned as: each mark carries its own
   deadline, death at the first that expires unacknowledged. Deterministic
   and strictly weaker than re-arming (which would let an attacker-driven
   accept cadence extend a zombie's life).
4. **§7.4 chosen over §7.5 as the home of the general principle** — the
   principle is about the liveness *model*, and §7.4 is where the anchor
   is defined; §7.5 holds the mechanism that is the exception to it.
5. **The declined-root-fix footnote placed in §17.4** rather than §7.5 —
   §17.4 is where the `None` basis (the thing msg2's timestamp would
   delete) is specified, and the permitted edit scope there is "the basis
   prose", which this is.
6. **`ConnectError::AlreadyConnected` written in full** at all three new
   mention sites, so no reader can scan a bare "AlreadyConnected" as the
   deleted `AcceptError::AlreadyConnected` variant (§18.1).
7. **§5.7's timer-table derivation cells edited** (two cells, values
   untouched) — the stale "one-lost-keepalive tolerance" and the now-ruled
   `PERSISTENT_KEEPALIVE` needed to be right where a reader meets them.

## NEEDS A RULING

None. Every open point resolved inside the ruling text; the seven
judgment calls above are recorded rather than deferred.

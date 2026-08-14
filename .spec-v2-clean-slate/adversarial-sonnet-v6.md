# Adversarial correctness review — SPEC-v2-DRAFT.md (draft v6, post-pass-3c)

Reviewer lens: interaction and correctness (state machines, deadlocks,
livelocks, oscillation, contradicted claims, resource accounting) for the
three newly-added mechanisms: ruling 36 (contested-connection probe),
ruling 39 (idle-from-install reap), ruling 40 (PERSISTENT_KEEPALIVE
floor→ceiling). NOT an attack-surface review (that is a separate pass).

Status: COMPLETE.

---

## Verdict

**INCONSISTENT.** See the full verdict discussion at the end of this
document, after the findings and the passed-checks list. Headline: one
BLOCKER (ADV-S-1 — the contested-connection probe's success condition is
not robust to ordinary single-packet loss, contradicting the document's
own PING-supersession claim and its "a live peer answers" rationale), two
MAJOR follow-on findings (ADV-S-2, ADV-S-3), and four MINOR/NOTE findings.
All findings are wire-free.

---

## Findings

### ADV-S-1 — the contested probe's literal per-packet ACK requirement is not robust to ordinary loss, contradicting the document's own PING-supersession claim and the "a live peer answers" rationale

**Severity: BLOCKER**

**The contradiction.** Three passages disagree about what a lost PING
means for the contested-connection probe:

1. §8.4, line 2129-2130: *"PING (`0x01`) — the type byte alone.
   Ack-eliciting; never retransmitted (a lost PING is superseded by the
   next probe)."*
2. §8.7, line 2299: *"**never** (PADDING, PING, ACK, DATAGRAM, CLOSE): loss
   is absorbed by the next ACK, **the next probe**, the unreliability
   contract, or the linger reply rule respectively."*
3. §7.5, lines 1904-1908 (ruling 36): *"the endpoint immediately sends an
   ack-eliciting **PING** (§8.3) on it and requires an **ACK covering the
   packet that carried that PING** within `KEEPALIVE_TIMEOUT`. If one
   arrives, the mark clears and nothing else happens ... a genuinely live
   peer simply answered."* Echoed at §6.4, line 1078-1083: *"a live peer
   answers the PING and keeps its connection."*

(1) and (2) tell the reader that a lost PING is a non-event: loss recovery
doesn't resend it because "the next probe" takes over its job. That
framing is true for the *ordinary* PTO loop (§13.3-13.4), where the job
of a PING is only "prove the path still works" and any later probe
satisfies that job equally well. But (3) gives the contested-connection
PING a *stricter* job: prove that a specific, named packet was received.
ACK semantics are literal and packet-counter-specific (§12.1: ACK ranges
enumerate actually-received counters; §12.5: "an ACK whose `largest`
exceeds the highest counter this session has sealed is ignored" — there is
no notion of "close enough," only exact counters). If the original PING
packet is genuinely lost (ordinary transient UDP loss, not attacker
action), **no ACK will ever cover its counter**, no matter how many later
packets are exchanged and acknowledged.

**The failure trace.** Connection is live and fully responsive.
`accept()` is refused against a `None` basis at t=0; the endpoint seals an
ack-eliciting PING as packet #100 and marks the connection contested with
deadline t=10 (`KEEPALIVE_TIMEOUT`). Packet #100 is dropped in the
network (ordinary loss, no attacker). Because #100 is now the sole
ack-eliciting packet in the sent map, the connection's PTO timer arms per
§13.3's RFC 9002 formula (`smoothed_rtt + max(4·rttvar, K_GRANULARITY) +
MAX_ACK_DELAY`), typically well under a second on an established
connection with a prior RTT sample. PTO fires at, say, t=0.3s and — per
§13.4, "pending retransmittable frames oldest-first if any exist, else a
bare PING" — sends a *new* bare PING as a *new* packet, #101. The peer
receives #101 (never having seen #100), answers with an ACK. That ACK's
ranges (§12.1) show #101 received and #100 not — a gap, permanently,
because #100 never arrived. `pto_count` resets (§13.3), the connection is
provably alive and fully interactive, PTO keeps probing and getting
answered every RTT if needed — and yet the contested mark's condition
("ACK covering the packet that carried *that* PING," i.e. #100) is never
satisfied. At t=10 the mark's deadline expires unacknowledged and the
connection dies with `ConnectionLost::TimedOut` (§7.5, §15.4) —
**despite the peer having answered every single probe sent to it.**

This directly falsifies the stated rationale ("a live peer answers the
PING and keeps its connection") for the single most common failure mode
on a UDP path: one lost datagram. It is not an attacker scenario — it
requires nothing but ordinary loss, which every other liveness mechanism
in this document (the passive dance, §7.5's "one lost keepalive in one
direction" tolerance, ruling 34's whole `DEAD_TIMEOUT` derivation) is
explicitly engineered to survive. The contested probe alone has zero loss
tolerance, silently, despite reusing a frame type (PING) whose own
retransmission-class note claims that loss is a non-event.

**Compounding factor — multiple marks.** §7.5, line 1936-1938: *"A
connection may be marked contested more than once; each mark carries its
own deadline, and the connection dies at the first deadline that expires
unacknowledged."* Read together with the per-packet literal requirement
above, this is a conjunction over all outstanding marks: every one of N
outstanding literal packets must be individually ACKed inside its own
10 s window, or the connection dies. A busy responder fielding several
`accept()` refusals in quick succession (plausible — see §6.9's
flood-tolerance discussion) needs *all* of those PING packets to survive
the network, not just one representative one; a single lost datagram
among N kills the connection even though N−1 were answered. The stated
cost bound ("Cost, and why it is bounded," lines 1933-1946) discusses only
the *rate* of probes, never this reliability multiplier.

**Suggested fix.** Redefine the clearing condition so it is satisfied by
*any* ACK of *any* ack-eliciting packet sent by us on this connection at
or after the mark was set — i.e. treat the contested mark as cleared by
"the connection has received an ACK proving forward progress since the
probe was raised," not "the literal original packet was acknowledged."
That single change makes the mechanism inherit the same loss tolerance
PTO already gives every other ack-eliciting send, and makes the "next
probe supersedes a lost PING" line in §8.4/§8.7 true for this case as
well as the ordinary one. This is **not wire-affecting** — it changes
only the local bookkeeping predicate for clearing a mark, not any byte on
the wire, packet layout, or frame type.

**Wire-affecting:** No.

**Reinforcing trace — the mark can kill a connection the ordinary liveness
clock has just certified healthy.** Take a connection that has carried
nothing since install (ruling 39's install-idle case) and dial it into the
overlap window: install at t=0 with basis `None` (we dialled), so
`R = S = 0` and the ordinary reap deadline is t=25 (§7.4, §7.5). At t=20 an
`accept()` refusal marks it contested: PING sent as packet #100, mark
deadline t=30. Packet #100 is lost. PTO fires quickly (RTT-scale, ≪10s)
and resends a bare PING as #101; the peer — genuinely alive — answers it,
and that ACK is an authenticated, window-fresh receive, so by §7.4's
universal rule it resets `R` to ~t=21, pushing the *ordinary* liveness
deadline out to ~t=46 and rescuing the connection from the t=25
install-idle reap that was otherwise imminent. The connection is now, by
every ordinary measure, provably healthy and freshly certified alive. Yet
the contested mark's independent condition — an ACK naming counter #100
specifically — was never met, so at t=30 `ConnectionLost::TimedOut` fires
anyway, tearing down a connection whose liveness clock has 16 seconds of
headroom left. The two mechanisms give literally contradictory verdicts
about the same connection at the same instant, and the stricter,
loss-intolerant one wins unconditionally.

---

### ADV-S-2 — §15.4's single "liveness" row conflates two structurally different deadlines, and its "fires ≈ symmetrically" claim is false for the contested-probe death

**Severity: MAJOR**

§15.4 (line 3223) has exactly one row for timeout-driven death:

> `| liveness — 25 s without an authenticated fresh receive (§7.5) |
> nothing | ConnectionLost::TimedOut | its own liveness fires ≈
> symmetrically |`

§7.5 (lines 1909-1910) explicitly routes the contested-probe death through
this same row: *"If none arrives by the deadline the connection dies with
`ConnectionLost::TimedOut` (**§15.4's liveness row** — no new variant, no
wire signal, nothing transmitted)."*

But the contested-probe death is not "25 s without an authenticated fresh
receive" — it is **10 s** (`KEEPALIVE_TIMEOUT`) without an **ACK of one
specific packet**, a condition that (per ADV-S-1) is not even rescued by
an authenticated fresh receive. The row's cause description is simply
wrong for the case §7.5 explicitly says it covers: different deadline
value (10 s vs 25 s), different trigger predicate (ACK-of-a-packet vs
any-authenticated-receipt), different anchor (the PING's send time vs
`last_authenticated_recv`).

The "Peer's view: its own liveness fires ≈ symmetrically" column compounds
the problem. For the *ordinary* 25 s row this is reasonable — both sides
key on the same receive-silence condition, so if one side goes dark the
other typically times out on a similar clock. For the contested-probe
death, it's false in the case that matters most: a genuinely live,
still-receiving peer (the scenario the mechanism is designed to protect,
per §6.4's own words, "a live peer answers the PING and keeps its
connection") can have *us* tear the connection down unilaterally at
`KEEPALIVE_TIMEOUT` (10 s) while the peer's own `last_authenticated_recv`
keeps advancing normally from whatever else we send it — its own liveness
clock is nowhere near firing, "≈ symmetrically" or otherwise. Nothing is
transmitted to tell it we've died (line 1910's "nothing transmitted"), so
the peer can keep writing into a connection we already tore down for up to
its own full `DEAD_TIMEOUT` before it notices anything is wrong via its
*own* clock. That is a materially different peer-observable outcome than
the row claims, for a cause the row's own text says is filed under it.

**Suggested fix.** Give the contested-probe death its own teardown-matrix
row (or split the existing row into "idle liveness" and "contested-probe
liveness" sub-rows) with its own deadline value, trigger predicate, and an
honest "Peer's view" — something like "its own liveness is unaffected;
peer notices only via its own independent 25 s clock (or when it next
tries to reach a connection that's gone)."

**Wire-affecting:** No.

---

### ADV-S-3 — the contested-probe deadline has no home in §16.5's timer table or its deadline-priority ordering

**Severity: MAJOR**

§16.5 (lines 3500-3502) presents what reads as the closed, authoritative
list of the connection core's timers: *"**Named timers, single
min-deadline out.** The connection core's timer table: `Keepalive`,
`PersistentKeepalive`, `Liveness`, `Loss`, `Pto`, `AckDelay`,
`CloseLinger`."* Every entry gets its arming/disarming rule spelled out
in the surrounding text, and the same section pins tie-break ordering for
same-instant deadlines (lines 3513-3521: give-up beats retransmit; loss
beats PTO; teardown collection precedes keepalive evaluation;
`PersistentKeepalive` evaluated last).

None of the seven listed timers can carry the contested-probe deadline.
It isn't `Liveness` (different anchor — PING-send-time vs
`last_authenticated_recv`; different value — 10 s vs 25 s; different
clearing predicate — ADV-S-1). It isn't `Keepalive` or
`PersistentKeepalive` (those drive *sending* a beacon, not a
kill-on-silence deadline). It isn't `Pto`/`Loss` (those retry, they don't
tear the connection down). And per §7.5 (line 1936-1938) a single
connection can carry **multiple concurrent marks, each with its own
deadline** — something none of the seven single-instance named timers is
shaped to hold at all.

Given the document's own style elsewhere — declaring enumerations
"closed" and "exhaustive" (§14.5's exemption list, §18.1's error
taxonomy, §16.4's `ConnEvent` set) — a reader has every reason to treat
§16.5's timer table the same way, and by that reading it is incomplete:
an implementation cannot derive from §16.5 how or when `poll_output`'s
`Timeout(Option<Instant>)` (line 3435, 3488-3489) is supposed to reflect
an outstanding contested-mark deadline, nor where that deadline sits
relative to `Liveness`/`Pto`/etc. when two happen to coincide.

**Suggested fix.** Add the contested mark (however it's named — e.g. a
per-mark `Contested` deadline, or a small ordered set of them) to §16.5's
timer table, state its arm/clear rule (tying back to §7.5), and give it a
place in the equal-deadline priority list.

**Wire-affecting:** No.

---

### ADV-S-4 — §6.9's DoS-accounting table and §17.5's state-ceiling table are silent on the contested probe's send cost and per-connection mark list

**Severity: MINOR**

§6.9 is the per-attacker-packet-class cost table; it has no row and no
prose paragraph for the cost a refused `accept()` against a `None` basis
now imposes (one ack-eliciting PING send, per ruling 36). This is
arguably intentional — §7.5 (lines 1933-1935) already argues this cost is
"bounded by the application's own accept rate and by nothing an attacker
controls," i.e. it isn't attacker-shaped in the way §6.9's rows are. But
the document's own precedent is to give app-driven costs of comparable
shape their own called-out paragraph in §6.9 (see "the protocol paces
replacement not at all," lines 1490-1498, covering another app-triggered,
attacker-adjacent cost). The contested probe's cost has no such
cross-reference from §6.9, only from §7.5 itself — a reader auditing §6.9
alone would not learn this mechanism exists.

Separately, §17.5's state-ceilings table (lines 3816-3821) enumerates
per-connection memory (credit, datagram queues, replay window, sent map)
but says nothing about the list of outstanding contested marks a single
connection can accumulate. Per §7.5 (line 1936-1938), marks are
unbounded in number — every refused `accept()` against the same `None`
basis connection adds another mark with its own deadline, and nothing in
the text caps how many can be outstanding at once (unlike, say,
`INTRO_QUEUE_CAP` capping parked intros). This is self-inflicted by the
*local* application's own accept-call rate, not attacker-reachable, so
it's not a DoS finding — but it is state the ceilings table doesn't
account for, and the earlier finding (ADV-S-1) shows why the count
matters: more outstanding marks mean more independent single-packet loss
events that can each unilaterally kill the connection.

**Suggested fix.** A one-line cross-reference from §6.9 to §7.5's cost
paragraph, and a short note in §17.5 acknowledging the (application-only,
unbounded) contested-mark list.

**Wire-affecting:** No.

---

### ADV-S-5 — §7.3's illustrative exemption list predates ruling 36 and omits the contested probe

**Severity: NOTE**

§7.3 (line 1592-1594), introducing the anti-amplification budget: *"§13.4
and §14.5 exempt whole output classes (**PTO probes, pure ACKs, CLOSE,
keepalives**) from the congestion window."* This parenthetical is an
older, pre-ruling-36 enumeration and does not name the
contested-connection probe, unlike §14.5's own current list (line
3079-3089), which gives the contested probe its own bullet, separate from
PTO probes, with its own justification ("same reasoning, one step
sharper"). The substantive rule is unaffected — the very next sentences
(lines 1605-1606) say the budget binds "all output... explicitly
including the §14.5 and §13.4 congestion-window exemptions," which by
cross-reference does correctly capture the contested probe (also
independently confirmed at §7.5 line 1941-1942 and §14.5 line 3095-3099).
This is a staleness note, not a behavioural contradiction.

**Suggested fix.** Add "the contested-connection probe" to the
parenthetical list at line 1593 for consistency with §14.5.

**Wire-affecting:** No.

---

### ADV-S-6 — ruling 40 states a ceiling but no floor for `PERSISTENT_KEEPALIVE`, admitting degenerate near-zero intervals

**Severity: NOTE**

Every validation-rule statement found (§5.7 line 773, §7.5 line
1721/1732, §16.2 line 3332-3334, Appendix B line 4279-4285, constants
table line 4372) gives only an upper bound: "rejects an interval at or
above `DEAD_TIMEOUT`." None states a lower bound. `set_persistent_keepalive`
takes `Option<Duration>` (§16.2 line 3303), so nothing in the text stops
an application from passing `Some(Duration::ZERO)` or
`Some(Duration::from_nanos(1))` — a config that (per the beacon's
"fires when no marking send has occurred for the configured interval"
rule, §7.5 line 1730) would re-fire essentially every driver poll tick,
since "no marking send in the last ~0 s" is true almost by construction.
This doesn't overflow anything at reachable timescales (the counter space
and `REKEY_EPOCH_MSGS` arithmetic are safe by many orders of magnitude —
see "Checks performed" below) and it's self-inflicted rather than
attacker-reachable, but it is a real gap opened specifically by ruling
40's move from floor to ceiling: the ruling's own rationale text ("short
intervals need no special case... admitting short intervals directly")
argues *short* intervals are fine, without addressing *degenerate* ones,
and the API as specified has no floor to distinguish the two.

**Suggested fix.** Either state an explicit minimum (even a purely
advisory one, e.g. "SHOULD be at least 1 s") or note explicitly that the
knob is intentionally unbounded below and why that's safe.

**Wire-affecting:** No.

---

### ADV-S-7 — the document's own top-of-file status banner is stale relative to its body

**Severity: NOTE**

Line 3-4: *"**DRAFT v5, 2026/08/14** (unratified; walkthrough revisions
and **both** security-fix passes applied)."* But the body (lines
122-264) documents a *third* security-fix revision block, "**Security-fix
revisions (draft v5 → v6, 2026/08/14)**" (line 122), itself containing
three passes (3a, 3b, 3c — line 129, 159, 214) that produced rulings
35-40, including both mechanisms under review here that carry a ruling
number in the high 30s/40 (ruling 36, 39, 40). The banner's "both...
passes" undercounts by one pass and the version number undercounts by
one draft revision; a reader who trusts the banner and skips the body
would not learn rulings 36/39/40 exist at all. This is metadata, not a
technical contradiction, but it directly affects discoverability of the
three mechanisms this review is about.

**Suggested fix.** Bump the banner to "DRAFT v6" and mention the third
pass (rulings 35-40) in the summary line.

**Wire-affecting:** No.

---

## Checks performed that PASSED

- **`DEAD_TIMEOUT` = 2 × `KEEPALIVE_TIMEOUT` + 5 s = 25 s.** Re-derived
  and confirmed consistent at every site that states it: §5.7 table
  (line 772), §7.5 table (line 1720), Appendix B (lines 4247-4249, the
  SECV5-8 test), and §7.7's epoch-jump-arithmetic paragraph. No stray
  value found.

- **`PERSISTENT_KEEPALIVE`'s bound direction and default are consistent
  everywhere.** Checked §5.7 (line 773), §7.5 (lines 1721, 1732),
  §16.2 (lines 3332-3334), Appendix B (lines 4242-4244, 4279-4285), and
  the consolidated constants table (line 4372) — all state "ceiling,"
  "at or above `DEAD_TIMEOUT`," and default "10 s," with none of the
  stale "floor"/"below `DEAD_TIMEOUT`"/"25 s default" language surviving
  outside the explicitly-superseded historical changelog text (which is
  itself clearly marked "superseded"/"reversed" at every occurrence —
  lines 180-198, 237-262). No live contradiction found.

- **The one-lost-beacon-tolerance arithmetic, independently re-traced.**
  Walked the full ping-pong timeline for a 10 s default persistent beacon
  against ruling 39's install-pin (`R = S =` install instant on both
  sides): first beacon at t=10 triggers an immediate passive-rule reply
  (peer's `now − S ≥ KEEPALIVE_TIMEOUT` is already satisfied at the
  moment `R` updates), the loop then self-sustains at a clean 10 s
  cadence indefinitely. Re-traced the lost-beacon case separately: a
  beacon lost at t=10 delays the next exchange to t=20 — exactly 5 s
  inside the 25 s deadline computed from the install-pin baseline — and
  two consecutive losses (t=10 and t=20 both lost) leave both sides'
  `R` at 0, so both die at exactly t=25. Matches Appendix B's stated test
  obligations (lines 4271-4278) exactly.

- **Ruling 39's "one exchange bootstraps the dance for both roles,"
  traced through the actual install-time asymmetry.** The responder
  installs (and sends msg2) at its own `accept()` time; the initiator
  installs only after receiving and validating that msg2, so the two
  sides' install instants are not simultaneous. Traced the bootstrap
  anyway: whichever side sends the first real Data packet, §12.4's
  immediate-ACK-on-first-packet rule ("the first ack-eliciting packet of
  a session has no previous greatest... yields an immediate ACK," line
  2842-2847) gives the *other* side an authenticated receipt within about
  one RTT regardless of which side spoke first — independent of, and
  faster than, the 10 s passive-keepalive fallback. No ordering was found
  that leaves one side permanently outside the dance once traffic has
  flowed in either direction. (This does not touch ADV-S-1/ADV-S-2's
  finding, which is about the contested-probe mark specifically, not the
  ordinary dance.)

- **§14.5's exemption list is exhaustive and correctly includes the
  contested probe.** Confirmed at lines 3079-3089 (three items: PTO
  probes, the contested-connection probe, non-ack-eliciting control
  packets), cross-referenced correctly from §7.5 (line 1939) and §6.4.

- **The epoch ratchet does not interact badly with the contested probe.**
  §7.7 (line 1968) states the packet counter is "never reset by the
  ratchet," so a PING's target counter and any eventual ACK of it remain
  meaningful across ratchet events regardless of how many epochs have
  elapsed in between. No bug found (the practical scenario is moot
  anyway — `KEEPALIVE_TIMEOUT` is far too short for 65,536 messages to
  elapse under any realistic send rate).

- **§5.4's LIVE/PENDING/NONE responder-state model is consistent with
  §6.4's contested-mark scoping.** The contested mark applies only to the
  LIVE-row, `None`-basis refusal branch; it correctly does not appear on
  the PENDING branch, which has no established `Connection` to send a
  PING on in the first place.

- **§18.1's error taxonomy adds no new variant for either ruling.** Both
  the idle-from-install reap (ruling 39) and the contested-probe death
  (ruling 36) reuse `ConnectionLost::TimedOut`, matching §7.5's explicit
  "no new variant" statements (lines 1909-1910) and §18.1's own "closed
  and normative" claim (line 3827).

- **No overflow/exhaustion at admissible — even pathological —
  `PERSISTENT_KEEPALIVE` intervals.** The 64-bit packet counter and the
  65,536-message ratchet epoch are safe by many orders of magnitude at
  any humanly-reachable send rate; keepalives never enter the sent-packet
  map and never consume ACK-range slots since they are non-ack-eliciting
  (§7.5, §8.7, §13.5), so a fast beacon cannot inflate the sent map or
  the 64-range ACK cap. The only real gap found here is the *absence* of
  a stated floor (ADV-S-6), not an arithmetic failure.

---

## Verdict

**INCONSISTENT.**

The document is internally consistent on the arithmetic and on every
*value*-level cross-reference checked for the three rulings (the
`PERSISTENT_KEEPALIVE` ceiling/default, the `DEAD_TIMEOUT` derivation, the
dance's scope). Where it breaks down is at the *mechanism* level, in
ruling 36 specifically: the contested-connection probe's success
condition, read literally against the document's own ACK and
retransmission-class rules (§12.1, §8.4, §8.7), is not robust to ordinary
single-packet loss — a scenario the rest of the liveness design goes out
of its way to tolerate (ruling 34's whole `DEAD_TIMEOUT` derivation, the
"one lost keepalive" guarantee) — and this directly falsifies the
mechanism's own stated rationale ("a live peer answers the PING and keeps
its connection," §6.4 line 1083) in the single most ordinary failure mode
on a UDP path (ADV-S-1). That defect propagates into an inaccurate
teardown-matrix row (ADV-S-2) and an unaddressed gap in the timer
model (ADV-S-3). None of this is wire-affecting, and all of it is fixable
by changing local bookkeeping predicates rather than any byte on the
wire — but by the letter of CLAUDE.md's ratification bar ("the code must
match the spec"), a state machine this document itself contradicts is not
yet ready to be that spec.


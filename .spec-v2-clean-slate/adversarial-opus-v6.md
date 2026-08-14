# Adversarial review — rulings 36, 39, 40 (SPEC-v2-DRAFT.md, draft v6)

**Reviewer posture:** refutation. Every claim below was attacked before it
was written down; attacks that the design survived are listed at the end
rather than omitted.

---

## VERDICT: **NOT-SOUND**

One **BLOCKER** and two **MAJOR** findings. The cryptographic core of
ruling 36 is correct and survived every attack I could mount on it — a
withheld genuine Data packet genuinely cannot produce an ACK of a packet
sealed after the harvest. The break is not in the authenticity direction;
it is in the **availability** direction, and it is introduced *by ruling
36 itself*:

> the probe's success condition is pinned to **one specific packet**, and
> that packet is not retransmitted. One ordinary packet loss therefore
> kills a healthy connection — and the probe can be provoked, off-path,
> as often as an attacker likes from a **single passively captured
> msg1**.

That inverts the exact property `replacement_basis: None` exists to
provide. §6.4 (line 1108) and §17.4 (line 3788) both state it in so many
words: a `None` basis "keeps a passively captured msg1 from **destroying**
a connection we dialled". After ruling 36 a passively captured msg1
destroys a connection we dialled, with probability approaching 1 on any
path with non-zero loss.

The fix is small, local, and **not wire-affecting**. Rulings 39 and 40 are
directionally right — ruling 40 in particular is a genuine correction of a
genuine inversion, and its arithmetic checks out — but ruling 40 replaces
an inverted bound with a **one-sided** bound, and the newly admitted
low end of the range is unbounded and actively harmful.

| ID | Severity | Ruling | One-line |
|---|---|---|---|
| ADV-O-1 | **BLOCKER** | 36 | Per-packet ACK matching + non-retransmitted PING + unbounded re-marking ⇒ one captured msg1 reliably kills any dialled connection, off-path |
| ADV-O-2 | **MAJOR** | 36 | "Bounded by nothing an attacker controls" is false; the probe is an attacker-driven, cwnd-exempt emitter that breaks §17.5's sent-map ceiling and §6.3's established-connections-unaffected clause |
| ADV-O-3 | **MAJOR** | 40 | The ceiling has no floor: `set_persistent_keepalive(1 ms)` is admissible and produces unbounded marking traffic outside congestion control, plus RTT-estimator collapse — strictly worse than ruling 38's inert knob |
| ADV-O-4 | MINOR | 40 | The ceiling ignores the shell lateness bound `L` and answer latency; `(DEAD_TIMEOUT − L, DEAD_TIMEOUT)` is admitted yet provably inert, and all of `[KEEPALIVE_TIMEOUT, DEAD_TIMEOUT)` has zero loss tolerance |
| ADV-O-5 | MINOR | 40 | `set_persistent_keepalive` returns `()` — the newly load-bearing "rejects" has no error channel, and the previously recommended default (25 s) is now the first rejected value |
| ADV-O-6 | MINOR | 39/40 | §7.4's "a replayed initiation never turns us into a keepalive source aimed at a spoofed address" is falsified once a beacon is set; the accept path's ratio rises from 0.55× to the full 3× budget, against §6.9's "No amplification" |
| ADV-O-7 | MINOR | 39 | The reap is a *receive*-within-25 s-of-install rule, not a *send*-within-25 s rule; connect-ahead-of-use and wait-for-human flows die silently at the default |
| ADV-O-8 | MINOR | 36 | §16.5's normative timer table has no contested-probe timer, and "each mark carries its own deadline" is incompatible with the named-single-deadline model |
| ADV-O-9 | MINOR | 36 | §15.4's liveness row does not describe the contested death; no §18.2 trace target; `AcceptError::Stale` gives the application no signal that its live connection is now on a 10 s death watch |
| ADV-O-10 | NOTE | 36 | Unspecified: what happens when the PING cannot be transmitted (anti-amplification budget, closing state), and whether the deadline runs from the mark or from transmission |

---

# Findings

## ADV-O-1 — **BLOCKER** — the contested probe is a one-packet-loss kill switch, and it is attacker-triggerable off-path

### Spec location and quoted phrases

`SPEC-v2-DRAFT.md` §7.5, lines 1902–1913:

> "that connection is marked **contested**: the endpoint immediately sends
> an ack-eliciting **PING** (§8.3) on it and requires an **ACK covering
> the packet that carried that PING** within `KEEPALIVE_TIMEOUT`."

and lines 1936–1938:

> "A connection may be marked contested more than once; each mark carries
> its own deadline, and **the connection dies at the first deadline that
> expires unacknowledged**."

§6.4, lines 1078–1080 ("requires an ACK covering that PING within
`KEEPALIVE_TIMEOUT`"), and Appendix B, line 4291 ("an ACK covering that
PING inside `KEEPALIVE_TIMEOUT`"), pin the same per-packet reading. The
test obligation makes it normative, so this is not a drafting slip that an
implementer would round off.

The load-bearing interaction is §8.7, line 2299:

> "**never** (PADDING, **PING**, ACK, DATAGRAM, CLOSE): loss is absorbed
> by the next ACK, **the next probe**, the unreliability contract, or the
> linger reply rule respectively."

PING is in the **never**-retransmit class. A lost PING frame is never
re-sent as itself; §13.4 sends "a bare PING" on a *fresh counter*.

### Failure trace (no attacker at all)

1. `A` dialled `B`. Basis is `None` (§17.4, line 3776).
2. Any `Intro` for `B`'s static parks and the application calls
   `accept()`. §6.4 returns `Stale` and marks the connection **contested**.
3. `A` seals a PING at counter *N* and starts a 10 s deadline.
4. **The packet carrying counter *N* is lost.** Ordinary internet loss.
5. `B` is perfectly healthy. It never received *N*, so its replay window
   has a permanent hole at *N*, and every ACK it will ever derive from
   that window (§12.2 — "An ACK is derived from the replay window's
   snapshot") carries a **gap** at *N*. No ACK covering *N* can ever
   exist.
6. `A`'s PTO fires at ≈ 1.1 s (`K_INITIAL_RTT` 333 ms; §13.3's formula)
   and sends a bare PING at counter *N+1*. `B` ACKs *N+1* immediately
   (§12.4's out-of-order rule). **`A` now holds positive, cryptographic,
   post-mark proof that `B` is alive** — an ACK of a packet sealed after
   the doubt arose, which is precisely the evidence ruling 36 says it
   wants.
7. At *t* = 10 s the mark's deadline expires unacknowledged. `A` fires
   `ConnectionLost::TimedOut` and tears down a connection it proved
   healthy 8.9 seconds earlier.

Reordering does *not* cause this (a late *N* is ACKed immediately). ACK
loss does *not* cause this (the next ACK, PTO-driven, still covers *N*
from the window). **Only forward loss of the PING packet itself does — and
that is the single most common failure on the internet.**

### Attack (this is why it is a BLOCKER, not a robustness MINOR)

The attacker needs: one passively captured genuine `msg1` from `B`, and
the ability to send packets. Nothing more. Not on-path. No keys.

**Why one captured msg1 is enough, forever.** §6.4, lines 1090–1096, states
that the timestamp guard "is **empty for every peer we only ever dial**
… so against such a peer the guard bars nothing at all and every
candidate passes it vacuously, **however old**." And §6.4's own ordering
clause, lines 1115–1119, guarantees the capture is never spent:

> "a guard record made for a candidate whose `accept()` then returns
> `AcceptError::Stale` is **reverted**, leaving the guard exactly as it
> was before the call."

So each replay of the same captured `msg1` is refused with `Stale`, the
guard record is rolled back, and the **identical bytes** can be replayed
again immediately. The attacker's capture is a permanent, reusable key to
the probe.

**The loop.**

```
loop:
  attacker → A:  replayed msg1 from B        (196 B, any source address)
  A:             parks Intro
  A's app:       read_identity → authenticate → accept()      [the flow ruling 36 exists to serve]
  A:             AcceptError::Stale  +  contested mark  +  PING at counter N_i
  attacker:      does nothing further
```

Each iteration is an independent Bernoulli trial with success probability
≈ *p*, the forward one-way loss rate. §7.5 line 1937 makes the trials
**cumulative**: "the connection dies at the first deadline that expires
unacknowledged" — every outstanding mark is a separate death sentence, and
`A` needs *all* of them answered. At *p* = 0.5 % and 1000 replays the
connection dies with probability ≈ 99.3 %. 1000 replays is 196 KB of
attacker traffic and, at the accept-loop rate of any auto-accepting
application, a few seconds.

Cost asymmetry: 196 bytes per trial for the attacker; 2 DH (`es` + `ss`,
per §6.9's replay row), one seal, one sent-map entry, and one packet for
the victim.

**Precondition, stated honestly:** the application must call `accept()` on
the parked `Intro`. That is not a get-out — it is *exactly* the flow ruling
36 was written to serve (§6.8 line 1433: "The delay is then bounded by the
application's next `accept()`"), and §6.4 line 1107 already contemplates
"applications that auto-accept replacements". An application that never
accepts is also an application the ruling never helps.

### What this destroys

§6.4, lines 1108–1110:

> "A candidate presented against a `None` basis is refused outright —
> which is what keeps a passively captured msg1 from **destroying** a
> connection we dialled, the case in which we hold no timestamp of that
> peer's at all."

§17.4, lines 3787–3789:

> "A `None` basis refuses every replacement, and that refusal is what keeps
> a passively captured msg1 from **destroying** a connection we dialled."

Both sentences are false after ruling 36. The refusal still stands *on
paper* — `accept()` returns `Stale` and the connection is not replaced —
but the connection dies anyway, which is the outcome the refusal existed
to prevent. Ruling 36 traded a wedge (attacker must sustain a drip
forever, and must have been on-path to harvest) for a kill (attacker acts
once, off-path, and the connection is gone). For most applications the kill
is the worse trade: the wedge is recoverable by an application `close()`,
whereas the kill is silent, repeatable, and indistinguishable from path
failure.

### Suggested fix — **not wire-affecting**

Replace per-packet matching with a **counter high-water mark**, and
collapse concurrent marks:

1. On a basis-`None` refusal, if the connection is **already contested**,
   do nothing (no second PING, no second deadline).
2. Otherwise: record `probe_floor` = the counter allocated to the probe
   packet, arm **one** deadline at `now + KEEPALIVE_TIMEOUT`, and send the
   PING.
3. The mark clears on **any received ACK whose ranges cover any counter
   ≥ `probe_floor`** — not necessarily the PING's own counter.

Security is identical, and the proof is the same one ruling 36 already
gives (§7.5 lines 1926–1928): every counter ≥ `probe_floor` was sealed
strictly after the harvest ended, so no harvested peer→us packet can carry
an ACK reaching it — a harvested ACK's `largest` is bounded by what the
peer received before it restarted. Meanwhile:

- the PTO train now *rescues* the probe (the PTO PING at `N+1` ≥
  `probe_floor`, so its ACK clears the mark) — the single-loss kill is gone;
- ordinary application traffic clears the mark for free;
- a legitimate **roam** during a probe no longer causes a false death (once
  the peer's traffic roams us, our next send reaches it and its ACK covers
  a counter ≥ `probe_floor`);
- step 1 caps the probe rate at **one PING per `KEEPALIVE_TIMEOUT` per
  connection**, which also closes ADV-O-2.

No frame layout, no new frame type, no new error variant, no timer value,
no packet size moves. §8.3's PING and §8.4's ACK are untouched. Appendix B
line 4291's obligation needs one word changed ("an ACK covering **any
packet sent at or after the mark**"), and a new obligation should be added:
*drop the probe PING itself and assert the connection survives via the PTO
train's ACK.* That test is the one that would have caught this.

---

## ADV-O-2 — **MAJOR** — the probe rate is attacker-driven, and it breaks two stated ceilings

### Spec location and quoted phrase

§7.5, lines 1933–1936:

> "**Cost, and why it is bounded.** One PING per refused basis-`None`
> accept, and those refusals are application-driven `accept()` calls, so
> the probe rate is bounded by the application's own accept rate **and by
> nothing an attacker controls**."

This is the load-bearing safety claim for the whole mechanism, and it is
wrong. The application's accept rate is an *upper* bound on the probe rate;
the **supply of things to accept is entirely attacker-controlled**, and per
ADV-O-1 a single captured `msg1` is an inexhaustible supply (the guard
record reverts on `Stale`, §6.4 lines 1115–1119). An attacker therefore
drives the probe rate right up to that upper bound. For an auto-accepting
application the bound is the accept loop's own throughput, i.e. thousands
per second. `INTRO_MAX_PER_SOURCE` = 4 bounds *concurrent* chains per /64,
not the rate — the cycle is park → accept → free → park.

### Consequences beyond the kill

**(a) §17.5's ceiling is violated.** §17.5, established-connections row:

> "… and a **sent map bounded by cwnd**; the credit term dominates"

The contested probe is ack-eliciting (so it enters the sent map, §13.5)
**and** cwnd-exempt (§14.5, lines 3084–3087). PTO probes are also exempt
but self-limit to ≤ 1 outstanding with `2^pto_count` backoff; the
contested probe has **no rate limit at all**. An attacker-driven probe
stream therefore inflates the sent map and `bytes_in_flight` past `cwnd`
without bound. §17.5's ceiling no longer holds as stated.

**(b) The application is starved.** §14.5's gate is
`bytes_in_flight + candidate_size ≤ cwnd`. Probes bypass the gate but
still *count* in `bytes_in_flight`. Sustained at *r* probes/s over an RTT
of *R*, in-flight probe bytes ≈ *r · R · 40 B*; at *r* = 5000/s and
*R* = 100 ms that is 20 KB against an `INITIAL_WINDOW` of 12 KB. The gate
is then closed to the application's own data for as long as the flood
lasts, on a connection that is perfectly healthy.

**(c) §6.3's isolation clause is contradicted.** §6.3's honesty clause,
line 1019:

> "The denial, while sustained, is endpoint-wide **for inbound accepts** …
> **established connections themselves keep running regardless — they hold
> no queue slot**."

After ruling 36 an inbound-initiation flood *does* reach established
connections: it emits packets on them, occupies their in-flight budget,
and (per ADV-O-1) kills them. The clause needs amending regardless of
whether ADV-O-1 is fixed.

**(d) Third-party reflection, de-amplifying.** The PING is aimed at the
*connection's* endpoint address, never at the `msg1` source, so there is
no address-steering primitive — but the attacker still chooses the victim
by choosing whose `msg1` to replay, and `B` sees the flood arriving from
`A`. Byte ratio 196 → ≈ 30, packet ratio 1 → 1. This is a **laundering**
primitive, not an amplifier; it does not violate §6.9's "No amplification"
claim, and I rank it NOTE-level on its own. It is listed here only because
it is the natural next question after (a)–(c).

### Suggested fix — **not wire-affecting**

ADV-O-1's step 1 (a connection already contested does not re-mark and does
not re-PING) reduces the probe rate to ≤ 1 per `KEEPALIVE_TIMEOUT` per
connection — a hard, attacker-independent bound, which is what §7.5 line
1935 claims and currently does not deliver. Then rewrite lines 1933–1938
to state the real bound, and amend §6.3's line 1019 and §17.5's sent-map
term.

---

## ADV-O-3 — **MAJOR** — ruling 40 gives the interval a ceiling and no floor; the low end is worse than ruling 38's inert knob

### Spec location and quoted phrases

§7.5 line 1732: "`set_persistent_keepalive` **rejects an interval at or
above `DEAD_TIMEOUT`** at the handle."
§16.2 line 3332: "rejects intervals **at or above** `DEAD_TIMEOUT` — the
bound is a ceiling, **not a floor**".
Appendix B line 4280–4284 makes the absence of a floor a **test
obligation**: "assert `set_persistent_keepalive` **accepts** an interval
strictly below `DEAD_TIMEOUT` — 10 s … **and a short value such as 1 s**".

Nothing in §5.7, §7.5, §16.2 or the Named-constants table bounds the
interval below. `set_persistent_keepalive(Some(Duration::from_millis(1)))`
is conformant.

### Why that is harmful, not merely silly

The beacon is a **keepalive**, and §14.5 line 3088 is explicit:

> "**Non-ack-eliciting control packets** — pure ACKs, CLOSE, **keepalives**
> — are never tracked in flight and **never gated**."

So a beacon at interval *I* emits `1/I` packets per second **entirely
outside congestion control**, forever, with no ACK clocking, no window, and
no way for the peer to push back. At *I* = 1 ms that is 1000 packets/s
(≈ 240 kbps of 30-byte packets) per connection, times however many
connections the application configures. The protocol offers the receiver
no defence: keepalives are not ack-eliciting, so they generate no return
ACK the sender must wait for.

The specification already judges this rate class unacceptable — in the
adjacent section, for a *smaller* number. §13.3, lines 2942–2945:

> "Without the precondition an idle connection self-sustains a probe train
> … at **~20 packets/s** against the 10 s keepalive cadence, **defeating
> §16.5's timer economy**."

Ruling 40 admits, by configuration, fifty times that rate — and §14.7 line
3141 states the shell has "**no sub-RTT wakeups in the v1 shell**", which a
sub-RTT beacon interval directly violates.

**Second harm: the RTT estimator degrades.** §13.1 line 2879: "An ACK
yields an RTT sample when its `largest` is **newly acknowledged**". Our own
keepalives are never in the sent map (§8.7 line 2280), so an ACK whose
`largest` is one of our beacon counters yields **no RTT sample at all** —
and §12.3 line 2822 additionally forces `ack_delay = 0` for it, inflating
any sample that does get taken by up to `MAX_ACK_DELAY`. A beacon that
fires inside the peer's 25 ms `AckDelay` window steals the `largest` slot
from a real data packet; the fraction of samples lost is ≈
`MAX_ACK_DELAY / I`, i.e. 25 % at *I* = 100 ms and most of them below that.
Degraded `smoothed_rtt`/`rttvar` degrade PTO (§13.3), loss detection
(§13.2) and persistent-congestion detection (§14.4) for the *whole*
connection.

Under ruling 38 neither harm was reachable — the beacon never fired. **So
the answer to "is any interval in the newly-admitted range worse than the
old inert behaviour?" is yes**, and the whole sub-100 ms end of the range
is where it lives.

### Suggested fix — **not wire-affecting**

Make the bound two-sided. The natural floor is `KEEPALIVE_TIMEOUT / 2`
(5 s) or, more defensibly, **1 s** — matching the value Appendix B already
names as the intended short case, and comfortably above `MAX_ACK_DELAY`
and any plausible sub-RTT wakeup concern:

> `set_persistent_keepalive` rejects an interval **at or above
> `DEAD_TIMEOUT`** and **below `MIN_PERSISTENT_KEEPALIVE` (1 s)`**.

One new named constant, no wire byte, no timer on the wire. Appendix B's
line 4283 obligation ("a short value such as 1 s") becomes the boundary
test rather than an interior point.

---

## ADV-O-4 — MINOR — the ceiling ignores `L` and the answer latency, so it still admits inert intervals

§16.5's lateness bound is normative: "Every armed deadline `D` fires no
earlier than `D` and **no later than `D + L`**", with `L` = 250 ms.

The ceiling admits *I* = 24.9 s. The beacon then fires at up to
24.9 + 0.25 = **25.15 s**, i.e. **after** the death deadline at
`R + DEAD_TIMEOUT` = 25 s in exactly the mutually-idle-from-install state
ruling 40 cites as its motivating case (§7.5 lines 1806–1812: "it holds
outright in the state ruling 39 reaps, a connection idle from install,
where `S = R` at the install instant"). The interval is admitted and is
provably inert — the precise failure the ceiling was introduced to
exclude. Ruling 40's own inequality (`S + I < R + DEAD_TIMEOUT`) is written
without the `+L` term and without the one-way delay of the beacon or of
the peer's answer.

Separately, the whole of `[KEEPALIVE_TIMEOUT, DEAD_TIMEOUT)` has **zero**
loss tolerance. Trace at *I* = 20 s, idle from install (`S = R = 0`):
beacon at 20, answer at 20 + owd, `R` refreshed. If that single beacon is
lost, the next fires at 40, and death lands at 25. §5.7 line 773 attaches
"one lost beacon is still tolerated inside `DEAD_TIMEOUT` (2 × 10 + 5)"
to the 10 s default, which is correct — but nothing warns an application
choosing 20 s that it has bought a beacon that dies on the first dropped
packet.

**Fix — not wire-affecting.** Either state the real ceiling
(`I + L + RTT < DEAD_TIMEOUT`, with a stated RTT allowance), or — cleaner
and self-consistent with the constant table — make the ceiling
`KEEPALIVE_TIMEOUT`: `set_persistent_keepalive` rejects `I >
KEEPALIVE_TIMEOUT`. Then *every* admissible interval carries the same
one-lost-beacon tolerance the default advertises, `L` is absorbed with
14.75 s to spare, and ruling 40's "re-basing the bound on
`KEEPALIVE_TIMEOUT` is **superseded**" (line 1819) needs revisiting — the
ceiling admits short intervals, but a ceiling at `DEAD_TIMEOUT`
specifically admits a band that does not work.

---

## ADV-O-5 — MINOR — the newly load-bearing rejection has no error channel, and the old default is now the first rejected value

§16.2 line 3303: `pub fn set_persistent_keepalive(&self, interval: Option<Duration>);`

Return type `()`. §16.2 line 3332 and §7.5 line 1732 both say it
"**rejects**" out-of-range intervals. There is no `Result`, and the spec
never says whether rejection panics, clamps, or silently ignores. Under
ruling 38 this did not matter — the knob was documented as inert, so no
behaviour depended on it. Under ruling 40 it matters a great deal: a
silent-ignore implementation leaves the application believing it holds a
NAT binding open when it does not, and the connection dies at
install + 25 s (ruling 39) with `ConnectionLost::TimedOut` and no
diagnosis.

Sharper: **the value ruling 38 recommended — 25 s — is now exactly the
first rejected value** (`DEAD_TIMEOUT`, rejected by the "at or above"
form). Any code, example, or test written against the pass-3b text hits
the undefined rejection path.

**Fix — not wire-affecting.** Change the signature to
`Result<(), ConfigError>` (or make the rejection a documented panic, as a
programming error). Either way, pin it: two conformant implementations
must not differ between "panic" and "no beacon".

---

## ADV-O-6 — MINOR — ruling 40 falsifies §7.4's spoofed-address claim and moves the accept path from 0.55× to 3×

§7.4, lines 1672–1677:

> "with `last_send` equal to `last_authenticated_recv`, §7.5's passive rule
> … is false until the first authenticated receive, so a half-open session
> **emits nothing at all**: it is reaped in silence, and **a replayed
> initiation never turns us into a keepalive source aimed at a spoofed
> address**."

That is stated unconditionally, and ruling 40 makes it conditional. The
beacon is *unconditional* — §7.5 line 1751: "it fires on its own timer and
asks nothing of `R`". An application following the new recommended
practice (set a 10 s beacon on accepted connections, which ruling 39's own
"the peer must hold its binding open with the persistent beacon" advice
pushes toward) turns every half-open session into exactly the keepalive
source §7.4 promises it is not: a replayed `msg1` with a spoofed source
anchors a session at that source (§7.3 line 1590) and we then beacon at
it every 10 s.

The exposure is **bounded, by design** — §7.3's budget caps output at
`AMPLIFICATION_FACTOR` (3) × 196 B = 588 B — but the *ratio* moves, and
§6.9 line 1514 states the old ratio as a property of the protocol:

> "**No amplification.** The accept path replies 107 B (msg2) to a 196 B
> stimulus — **ratio < 1**"

With a beacon configured the accept path emits up to 588 B for that same
196 B stimulus: ratio **3**, the maximum the budget permits, up from 0.55.
Replay is one-shot per captured `msg1` on the responder side (the guard
records and is *not* reverted on a successful accept), and a dialling
peer mints a fresh `msg1` every ≈ 5 s for up to `HANDSHAKE_GIVEUP` (90 s),
so the practical supply is ≈ 18 captures per failed handshake — modest,
but no longer "no amplification".

**Fix — not wire-affecting.** Two sentences. (1) §7.4: qualify the claim —
"…never turns us into a keepalive source aimed at a spoofed address
**unless the application configures a persistent keepalive, whose output
to an unvalidated address remains capped by §7.3's budget**". (2) §6.9:
qualify the "ratio < 1" line the same way, or restate the accept path's
bound as the 3× budget rather than the msg2 ratio. Optionally: suppress
the beacon while the session's address is unvalidated — the beacon's job
(NAT holding) is meaningless before the peer has proved return
routability anyway, and this restores the ratio outright.

---

## ADV-O-7 — MINOR — ruling 39's reap is a *receive*-within-25 s rule, and the spec presents it as a *traffic* rule

§7.5 lines 1863–1867 state the reaped case as "a connection that has
carried **nothing** since install". §5.7 line 796 repeats it: "a connection
that has carried **nothing at all** since install". Both are imprecise in a
way that matters to applications.

The actual rule (§7.4, lines 1660–1668) is that the death deadline is
armed at install and is reset only by an **authenticated receive**. So the
requirement is *receive within 25 s of install*, and sending is only
instrumentally useful because it provokes the peer's passive keepalive.

Two consequences the ruling does not state:

**(a) A send at t = 24 does not save you.** `A` connects, its application
sends its first request at *t* = 24 s. `A`'s `R` is still the install
instant, so `A` dies at *t* = 25 unless the peer's answer completes within
1 s. If that first request is lost, `A` gets roughly one PTO
(≈ 1.1 s from `K_INITIAL_RTT`) and then dies — with data queued and the
application unaware anything was wrong. The usable window for a first
exchange **shrinks as the connection ages**, from 25 s down to one RTT.

**(b) The legitimate flows that break.** The task asked for these
specifically; they exist and they are ordinary:

- **Connect-ahead-of-use.** Dial at process start to hide handshake latency
  from the first user action. Both sides idle. Both die at 25 s with
  `TimedOut`.
- **Human in the loop.** A client dials, then waits for the operator to
  type. Nothing is sent. Both sides die at 25 s.
- **Responder-first-silence.** The canonical request/response shape where
  the server has nothing to say until asked, and the client stalls > 25 s
  before its first request.

In every case both sides emit *nothing* (§7.4 line 1675 — "it is reaped in
silence"), so there is no wire evidence, and `ConnectionLost::TimedOut` is
indistinguishable from a real path failure (§7.5 line 1891 makes exactly
this complaint about the declined "all keepalive opt-in" alternative — the
complaint applies to the retained design too, in the first 25 s).

This is a **ruled** trade-off and I am not asking to reverse it. But
ruling 39's own statement of what it costs (§7.5 lines 1873–1883) discusses
only the *post-death* reachability cost for NATted peers, and never
mentions that a just-established, never-used connection is the thing being
reaped. That is the part applications will trip over.

**Fix — not wire-affecting.** Restate the rule as "no **authenticated
receive** since install" in §7.5 line 1863 and §5.7 line 796, add the
send-at-t=24 consequence, and add an explicit application note: *an
application that establishes connections ahead of use MUST either send
within `DEAD_TIMEOUT` of install or configure a persistent keepalive.*
Add an Appendix B obligation for the connect-early/use-later shape (it is
currently only tested as "drive nothing", line 4247, which reads as a
degenerate case rather than as a supported pattern being reaped).

---

## ADV-O-8 — MINOR — the contested probe has no timer in §16.5's normative timer table

§16.5, lines 3500–3502, gives the connection core's timer table as a closed
list:

> "**Named timers, single min-deadline out.** The connection core's timer
> table: `Keepalive`, `PersistentKeepalive`, `Liveness`, `Loss`, `Pto`,
> `AckDelay`, `CloseLinger`."

Ruling 36 adds a deadline that can kill a connection, and it is not in the
list. Worse, §7.5 line 1937 says "each mark carries its own deadline",
which is **structurally incompatible** with a named-timer model where each
name carries one deadline and the core emits a single min — *N* concurrent
marks need *N* deadlines or a queue, and §16.5's "Equal-deadline
priorities (normative)" list says nothing about where a contested deadline
sorts against `Liveness`, `Keepalive` or `CloseLinger`.

Two conformant implementations will disagree on observable behaviour when a
contested deadline coincides with `Liveness` or with `CloseLinger` expiry.

**Fix — not wire-affecting.** Add `Contested` to the timer table. If
ADV-O-1's collapse-to-one-mark fix is taken, one deadline suffices and the
model stays intact. Place it in the priority list — it belongs with
teardown collection ("teardown collection … precedes keepalive
evaluation"), i.e. `Liveness` first, then `Contested`, then the rest.

---

## ADV-O-9 — MINOR — the contested death is invisible to the operator and mis-described in §15.4

§7.5 line 1910 routes the contested death to "§15.4's liveness row — no new
variant, no wire signal, nothing transmitted". But §15.4's liveness row
reads:

> "liveness — **25 s without an authenticated fresh receive** (§7.5) |
> nothing | `ConnectionLost::TimedOut` | **its own liveness fires ≈
> symmetrically**"

Neither half describes the contested death. It fires after **10 s**, and it
fires **while authenticated fresh receives are arriving** — that is the
entire point. And the peer's view is not symmetric: in the false-death case
(ADV-O-1) the peer is healthy and holds a live connection for another
25 s; in the intended case the peer already restarted and holds nothing.

Compounding it: §18.2's trace targets are "operator-visible contract" and
contain no target that carries a contested mark, its probe, or its verdict.
An operator watching connections die at 10 s has nothing to look at. And
the application sees only `AcceptError::Stale` (§18.1) — identical to an
ordinary stale accept — with no indication that its *live* connection is
now on a 10 s death watch it can neither observe nor cancel.

**Fix — not wire-affecting.** Add a `contested` row to §15.4 (transmitted:
one PING; local surface: `ConnectionLost::TimedOut`; peer's view: unaffected
if healthy). Add the mark, the probe and the verdict to `slither::policy`
(or a new target — noting §18.2's own rule that adding is fine, renaming or
dropping is a revision).

---

## ADV-O-10 — NOTE — unspecified: what happens when the PING cannot be sent

§14.5 lines 3084–3087 exempt the probe from the congestion gate but keep it
bound by the anti-amplification budget:

> "like every other exempt class, **remains bound by §7.3's
> anti-amplification budget at an unvalidated address**."

§7.5 line 1904 says "the endpoint **immediately** sends an ack-eliciting
PING on it and requires an ACK … **within `KEEPALIVE_TIMEOUT`**". The spec
never says whether the deadline runs from the **mark** or from the
**transmission**, nor what happens if the budget (or a closing/lingering
connection state, §15.2) prevents the send. If the deadline runs from the
mark and the PING is budget-deferred, the connection can be killed by a
probe that was never emitted — a verdict on a question that was never
asked. This is the task's "the ACK arrives but the PING was never sent"
case, and the answer is currently undefined.

Concrete (benign) reachable path: the peer roams; we anchor at the new
address with budget = 3 × (one authenticated packet, ≈ 50 B) = 150 B;
the app is mid-transfer and the owed ACK plus one retransmission consume
it; a contested mark lands; the PING waits for budget.

**Fix — not wire-affecting.** State that the deadline is armed **at the
probe's transmission**, and that a probe which cannot be transmitted within
the budget leaves the mark pending (not failed). Add: a contested mark on a
connection already in the closing/draining state is a no-op.

---

# Attacks attempted that FAILED

These are attacks I mounted and the design correctly resisted. They are
listed with the reason they fail, so the maintainer knows which properties
are **confirmed** rather than merely unchallenged.

## Against ruling 36

**F1 — Forge an ACK covering the probe from harvested traffic.** The
central claim (§7.5 lines 1926–1928) survives every route I could find.

- *Coincidental range coverage.* ACK ranges are derived from the peer's
  replay window (§12.2), so a harvested ACK's `largest` is bounded by the
  greatest counter the peer had actually received. The probe's counter is
  strictly greater than every counter we sealed before the harvest ended,
  so no harvested ACK — whatever its ranges — can reach it. There is no
  wrap: counters are 64-bit, "never reset by the ratchet" (§7.7 line 1968),
  and §7.9 shows the space is unreachable.
- *Idle connection, no counter advance between harvest and probe.* Does not
  help the attacker: the probe allocates the **next** counter, which is
  still strictly greater than the last one we sealed, which bounds the
  harvested coverage.
- *Get the restarted peer to ACK the probe.* The restarted peer holds a new
  session with different keys and a different receiver index; it cannot
  open the probe, and it could not seal an ACK under the dead session's key
  if it wanted to (§7.8 — "No transport state ever crosses a handshake").
- *Reflect our own probe back at us.* Sealed under our send key; our receive
  key does not open it.
- *Replay old ACKs.* §12.5 — duplicate acknowledgment of a counter is a
  no-op; and the underlying packet must still pass the replay window.
- *Forge a future ACK.* §12.5 — "An ACK whose `largest` exceeds the highest
  counter this session has sealed is **ignored whole**", and it would have
  to be sealed under the peer's key anyway.

The ACK-vs-receive distinction is the right one and it is cryptographically
sound. **This is the part of ruling 36 that is confirmed.** ADV-O-1 attacks
the *matching rule*, not this claim.

**F2 — Turn the probe into an amplifier.** Fails. Stimulus is a 196 B
`msg1`; response is one keepalive-sized PING (14 B header + AEAD tag,
≈ 30 B). Byte ratio ≈ 0.15, packet ratio 1:1. §6.9's "No amplification"
claim survives ruling 36 on its own terms.

**F3 — Turn the probe into an address-steering reflector.** Fails, and this
is a real design merit rather than an accident. The PING is sent on *the
connection*, to the connection's current endpoint address, which moves only
by authenticated roaming (§7.3 line 1578 — "Nothing unauthenticated, and no
replayed packet, ever moves it"). The `msg1` source address is never a send
target for the probe, so an attacker cannot aim the probe at a victim of
its choosing; it can only aim it at the peer whose `msg1` it captured. The
residual is laundering, not steering, and it de-amplifies (see ADV-O-2(d)).

**F4 — Make the congestion gate delay the probe past its own deadline.**
Fails; §14.5 lines 3084–3087 exempt it explicitly and give exactly this
reason ("a gate that could delay it past that deadline would turn a
congestion answer into a liveness verdict"). The exemption is correctly
scoped to `cwnd` and correctly *not* extended to §7.3's budget. (The
residual — what happens if the *budget* defers it — is ADV-O-10, a
specification gap rather than an attack.)

**F5 — Use the PTO train or the ratchet to produce a false death.** Fails.
The probe is `seal_quiet` (non-marking) but ack-eliciting, so it arms the
death clock and never re-arms or defers it (§7.4, §13.3) — consistent with
ruling 33 and with the probe's own deadline being independent of
`DEAD_TIMEOUT`. The ratchet is counter-derived (`counter /
REKEY_EPOCH_MSGS`) and one extra PING cannot move an epoch boundary in any
way that matters; `MAX_EPOCH_JUMP` = 2 gives 131 072 messages of slack.

**F6 — Drive `cwnd` to `MINIMUM_WINDOW` via persistent congestion using
lost probes.** Fails in the cases that matter. §14.4 requires "no packet
acknowledged between them" and a prior RTT sample; on a live connection
ACKs keep arriving, and on a genuinely black-holed path the collapse is
correct behaviour. I could not construct a case where attacker-driven
probes alone trip it without the path already being dead.

**F7 — Exhaust the guard, the intro queue, or the DH budget in a *new* way
via the probe.** Fails. The DH cost of a replayed `msg1` (2 DH, `es`+`ss`)
is already accounted in §6.9's replay row and is not increased by ruling
36; the queue occupancy is bounded by `INTRO_MAX_PER_SOURCE` and
`INTRO_TTL` exactly as before; the guard record reverts on `Stale` (§6.4)
so the guard does not grow either. The new cost is per-connection output
and in-flight budget, which is ADV-O-2, not a new endpoint-global exhaustion.

**F8 — Kill a healthy connection with a *`Some(t)`* basis via the probe.**
Fails, correctly. §6.4 line 1084: "A refusal against a `Some(t)` basis
marks nothing." Only dialled connections are exposed, which is the correct
scoping — although it is also why ADV-O-1 lands squarely on the property
the `None` basis was protecting.

**F9 — Use the probe to *keep* the wedge alive (attacker answers the
probe).** Fails. The attacker cannot produce the ACK (F1), and it cannot
suppress our sending of the probe (it is cwnd-exempt and the mark is local).
The wedge mechanism as described in §6.8 and §17.4 genuinely is closed by
this ruling; the objection in ADV-O-1 is that it is closed at too high a
price, not that it fails to close.

## Against ruling 39

**F10 — Break the "one exchange bootstraps the dance" claim.** I traced it
in both directions and both roles and it holds.

- Install at *t*=0: both sides `S = R = 0` (§7.4's pin). Symmetric — there
  is no initiator/responder asymmetry in the clocks.
- `A` sends at *t*=*T*. Receiver `B` gets `R_B = T > S_B = 0` ⇒ entry
  condition true. `B`'s keepalive fires at `max(T, KEEPALIVE_TIMEOUT)`
  (immediately if `T ≥ 10`, else at *t*=10 — its own 10 s not yet elapsed).
- That keepalive gives `A` `R_A > S_A` (`S_A = T`, `R_A ≥ 10 > T` when
  `T < 10`; and `R_A = T + owd > S_A = T` when `T ≥ 10`) ⇒ `A` enters, and
  the 10 s ping-pong self-sustains from there.
- I could not construct an ordering in which one side enters and the other
  does not. The dangerous state (`S > R`) is precisely "I sent last and have
  heard nothing since", which is the state where arming and death are the
  *correct* answers.
- Cross-traffic does not desynchronise it: pure ACKs and credit frames are
  `seal_quiet` and do not advance `S`, so a receiver that only ACKs stays in
  the dance and keeps feeding the bulk sender's `R`.

**F11 — Force connections into the never-carried-traffic state as a DoS.**
Fails to yield anything new. An attacker that drops the first application
packet gets both sides dying at 25 s — but a full black hole kills at 25 s
anyway, before and after ruling 39. No new power. (The residual is the
*benign* flows in ADV-O-7, which are an application-facing trap rather than
an attack.)

**F12 — Use ruling 36's probe to perturb the reap.** Fails. The probe is
`seal_quiet`, so it does not touch `last_send` and does not create `S > R`;
the death deadline is already armed at install, so the probe's arming is a
no-op; and the probe's own deadline is independent. A connection idle from
install and contested at *t*=20 still dies at *t*=25, not earlier.

**F13 — Make the install pin turn us into a keepalive source at a spoofed
address.** Fails *as ruling 39 stands* — §7.4's reasoning (line 1673) is
correct: `S = R` makes the passive rule false, so a half-open session emits
nothing. It fails only because the beacon is off by default; see ADV-O-6 for
the ruling-40 interaction that re-opens it.

## Against ruling 40

**F14 — Show that a one-sided beacon does not sustain the link.** Fails; the
claim is correct. The peer's passive keepalive is **normative, not optional**
("sends a keepalive", §7.5 line 1727), so a conformant peer always answers
within `KEEPALIVE_TIMEOUT` of its own last send. I verified the 10 s
steady state and its loss behaviour:
- single lost beacon: next beacon at +20 s, answer lands 5 s inside the
  deadline — survives, as claimed (line 1763);
- single lost *answer*: our next beacon at +10 s finds the peer with exactly
  10 s elapsed since its (lost) send, so it re-answers — survives;
- two consecutive losses: dies at 25 s, as claimed.
The arithmetic in §7.5 lines 1746–1768 checks out.

**F15 — Desynchronise the peer's answer so the beacon cannot rescue us.** I
built the sharpest version: peer marking-sends at *t*=*R*+9.9 (resetting its
own 10 s), that packet is lost, our beacon arrives at *t*=*R*+10 and finds
the peer 0.1 s into its interval, so it will not answer until *R*+19.9.
Repeating this pushes the answer past `R + DEAD_TIMEOUT`. **But** it requires
*two* lost peer→us packets inside 20 s, which is exactly the documented
"two consecutive losses" tolerance, or a sustained peer→us black hole, in
which death is the correct outcome. No new hole. The claim "the peer answers
at once, its own `KEEPALIVE_TIMEOUT` having already elapsed" (line 1761) is
true in the steady state it describes; the desynchronised case still answers
within `KEEPALIVE_TIMEOUT` of the peer's own last send, and that last send
being *received* is what refreshes our clock.

**F16 — Show the beacon defeats ruling 39's reap in a way that revives
ruling 36's wedge.** Fails, and cleanly. The beacon is set by the *local*
application on a connection it holds; an attacker cannot enable it. A zombie
whose local application beacons still dies at `R + DEAD_TIMEOUT` because
arming enables death and never defers it (§7.4, §7.5 lines 1770–1774) — the
beacon fires into the void and resets nothing. The attacker's drip remains
the only thing that keeps a zombie alive, at unchanged cost. §7.5's
"Marking is therefore harmless at every admissible interval" is correct **on
the liveness axis**; ADV-O-3 attacks it on the congestion and
estimator axes, which that sentence does not cover.

**F17 — ACK amplification from a fast beacon.** Fails. Keepalives are
explicitly not ack-eliciting (§7.5 line 1895, §8.7 line 2280), so no ACK is
owed on arrival and §12.4's "after every 2nd ack-eliciting packet" never
triggers on them. Return traffic stays bounded by the peer's passive rule at
one packet per `KEEPALIVE_TIMEOUT`, regardless of beacon rate. The one
residual — a lost keepalive creates a counter gap that makes the *next*
ack-eliciting packet look out-of-order and forces an immediate ACK — is
still ≤ 1 return packet per forward packet.

**F18 — Nonce/counter exhaustion or ratchet abuse from a fast beacon.**
Fails by orders of magnitude. §7.9's space is `2⁶⁴ − 2` per direction
(≈ 58 000 years at 10⁷ pkt/s); a 1 ms beacon at 10³ pkt/s advances the
epoch (`REKEY_EPOCH_MSGS` = 65 536) every ≈ 65 s, and an epoch advance is a
single `Rekey()` — cheap, and the receiver's one-epoch-back straggler
window is far more than a 1 ms cadence needs. Ruling 40's beacon does not
stress the ratchet.

**F19 — Find an interval that lets an attacker keep a zombie alive more
cheaply.** Fails. The attacker's cost is unchanged (one withheld genuine
packet per `< DEAD_TIMEOUT`); the beacon is ours, not theirs, and it cannot
substitute for the authenticated receive that a zombie needs.

---

# Summary for the maintainer

- **Ruling 36's security argument is confirmed** (F1, F9): the ACK-vs-receipt
  distinction is sound, the wedge really is closed, and the probe is not an
  amplifier or a steering reflector. **Its matching rule is broken**
  (ADV-O-1) and its cost bound is misstated (ADV-O-2). Both are fixed by the
  same two-line change — a counter high-water mark instead of packet
  identity, and one contested state instead of *N* — and neither touches the
  wire.
- **Ruling 39 is correct as ruled** (F10–F13). What it needs is a precision
  edit (the rule is *receive*-within-25 s, not *traffic*-within-25 s) and an
  application-facing warning about connect-ahead-of-use (ADV-O-7).
- **Ruling 40 is a correct diagnosis of a real inversion**, and its
  arithmetic survives (F14–F19). It replaced an inverted bound with a
  **one-sided** bound; the range needs a floor (ADV-O-3) and its upper end
  needs to account for `L` (ADV-O-4). The handle's rejection also needs an
  error channel now that it is load-bearing (ADV-O-5).
- **Nothing in any of the ten findings requires a wire change.** No packet
  byte, header layout, frame type, packet size, error variant, or ratified
  timer value moves under any suggested fix. The two new named things I
  propose — `MIN_PERSISTENT_KEEPALIVE` (ADV-O-3) and a `Contested` timer
  (ADV-O-8) — are both local to the implementation, and one new constant is
  a configuration bound, not a wire constant. The golden-wire and
  size/constant tests stay green.

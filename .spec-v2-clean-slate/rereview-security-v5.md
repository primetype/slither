# Focused adversarial security re-review — SPEC-v2-DRAFT.md, draft v5

**NOT-SOUND**

Scope: rulings 31–34 only, and the clauses they touched — §6.4, §6.5–§6.7,
§6.8, §7.4–§7.5, §17.1, §17.4, §13.3 (arming only), §18.1 (the two changed
branches). Supporting reads: §5.3, §5.4, §5.7, §6.9, §7.3, §7.7, §7.9,
§8.3, §8.7, §10.3, §13.4, §14.5, §15.1–§15.4, §16.1, §16.9, §17.5.

Four MAJOR findings. Two of them (SECV5-1, SECV5-4) are new holes cut by
the fixes themselves; one (SECV5-3) is a residual the fix knowingly took
on but under-bounded; one (SECV5-2) is a pre-existing gap that three
separate ruling-31/33/34 security arguments now lean on.

No BLOCKER: nothing here breaks confidentiality, authentication, or the
one-connection-per-static invariant against an attacker without keys. Every
finding is a denial or a convergence failure. SECV5-3 is the closest call —
a maintainer could reasonably promote it, and I say why under that finding.

---

## Ruling scorecard — what is confirmed

| Ruling | Goal | Verdict |
|---|---|---|
| **31 — `replacement_basis`** | a passively-captured msg1 can no longer destroy a live connection with no keys | **Achieved.** Confirmed below by direct attack. But it buys the closure with a new permanent-wedge residual (SECV5-3) and it does not make §17.1's honesty clause true (SECV5-5). |
| **32 — PENDING branch at `accept()`** | close the `read_identity()` → `connect()` → `accept()` ordering hole in §16.1's one-connection-per-static invariant | **Achieved for the invariant, but the fix is unsound.** The invariant now holds locally; the *peer* is not consulted, and the two sides can install mismatched key sets — precisely the failure §6.7 exists to prevent (SECV5-1). |
| **33 — death-clock arming on ack-eliciting output** | no more sending into a black hole with nothing arming the clock | **Achieved.** I could not find a remaining unarmed black-hole path, and I could not find a send pattern that arms unhelpfully and kills a healthy connection (arming alone never kills; 25 s of receive silence is still required). The `liveness-neutral` = *non-marking* redefinition is consistent across §7.5, §13.3, §13.4, §14.5; §10.3 is the one thin spot (SECV5-9). **Caveat:** the arming state *at install* is unspecified, and that is what SECV5-2 is about. |
| **34 — `DEAD_TIMEOUT` 15 s → 25 s** | one-lost-keepalive tolerance | **Achieved for unidirectional loss, and the sweep is complete.** I traced every `DEAD_TIMEOUT`-derived figure in the document and found no stale 15 s (the surviving 15 s values are all `INTRO_TTL`, deliberately untouched). §7.7's re-derived nonce figure (196 608 / 25 ≈ 7.9 k pps) is arithmetically right. Two caveats: the tolerance claim is weaker than stated for *simultaneous bidirectional* keepalive loss (SECV5-8), and the `PERSISTENT_KEEPALIVE` coincidence is worse than §5.7 concludes — the knob is provably inert at every value the floor admits (SECV5-7). |

---

## SECV5-1 — ruling 32's PENDING branch breaks simultaneous-open convergence: the two sides can install different sessions and go mutually dark

**Severity: MAJOR** *(convergence break; not attacker-driven, but it recreates the exact outcome §6.7 was written to prevent)*

### The sequence

Peers A and B, A's static lexicographically **smaller** than B's (so §6.7
makes A the winning initiator).

1. B's msg1 arrives at A from a source A is not dialling (A is dialling
   nothing yet), so §6.5 step 2 parks it and surfaces an `Intro`.
2. A's application calls `read_identity()` → B (static is NONE at this
   moment, so §6.5 step 4's interception does not fire) and
   `authenticate()` → `Proven`.
3. A's application calls `connect(B)`. A's static map for B goes PENDING;
   A sends its own msg1 to B.
4. A's application calls `accept()` on the `Proven` chain. §6.4's PENDING
   branch fires: *"the `accept()` **cancels** it … and the accept proceeds
   as an ordinary fresh install with this endpoint as responder. The
   tie-break does **not** run here."* A drops its pending, installs a
   session keyed on **B's msg1**, and writes msg2 to B.
5. Meanwhile A's msg1 from step 3 reached B. B holds an in-flight pending
   to A, and A's msg1 arrived from a hint-set source, so §6.5 step 3 sends
   it to the **internal tie-break**. B runs §6.6: tag ✓, guard ✓, tie-break
   → A's static is smaller, so B is the loser: *"we cancel our pending
   now … and the tie-break admits and writes msg2 as responder"*. B drops
   its pending and installs a session keyed on **A's msg1**.

Result: A holds a responder-side session over B's msg1. B holds a
responder-side session over A's msg1. Two distinct sessions, two distinct
key sets. A's msg2 reaches B, whose pending index has been dropped, so it
is discarded; B's msg2 reaches A, whose pending index has been dropped, so
it is discarded. Both endpoints' Data packets route to an index the other
side no longer holds.

**Both sides believe they are connected. Neither can open the other's
traffic. Both die at `DEAD_TIMEOUT`.** This is verbatim the outcome §6.7
names as the reason the tie-break exists: *"'whichever completes first'
would install different key sets on the two sides and go mutually dark for
25 s"* (§6.7, line 1021–1022).

A second, worse variant: if **both** applications take the PENDING branch
(both did `read_identity()` → `connect()` → `accept()`), the mismatch is
guaranteed regardless of key ordering — 100 % rather than the ~50 % above.
And both ends install as **responder**, so both would compute the same
stream-ID parity, contradicting §6.7's *"The tie-break winner is the
**connection initiator** for the life of the connection: stream-ID parity
(§9.1) is fixed by this outcome"*.

### Why the spec does not catch it

§6.6 step 2 states the asymmetry as if it were benign:

> *"The converse does **not** hold — PENDING is not exclusive to this path:
> a chain staged while its static was NONE and accepted after a `connect()`
> made that static PENDING reaches §6.4's PENDING branch instead, which
> cancels the pending and installs as responder without any tie-break."*

and §6.4 justifies skipping the tie-break with *"this path has already
committed to ours as responder"*. Both statements reason only about the
local endpoint. The tie-break is not a local decision — its entire value is
that *"Both sides compare the same ordered pair and reach complementary
conclusions"* (§6.7). Ruling 32 lets one side opt out of a two-sided
agreement unilaterally.

### Secondary effect: the replay-cancels-a-pending primitive is now key-order-independent

§6.7's honesty clause bounds the replay-cancels-a-pending capability by
noting it only bites *"if our static is the larger"*. §6.4's PENDING branch
cancels **unconditionally**, so a replayed genuine msg1 accepted on the
staged path destroys a victim's pending regardless of key ordering — the
capability's reach roughly doubles across a population. §6.4's PENDING
branch carries no honesty clause of its own, and §6.9's row confines the
primitive to *"the in-flight-dial window"*, which is now only true of the
*DH cost*, not of the *effect*.

### Suggested fix — **wire-free**

Make the PENDING branch agree with what the peer's tie-break will conclude.
In §6.4:

> If an in-flight outbound initiation exists for the proven static, apply
> §6.7's comparison. **If the peer's static is smaller** (we would be the
> tie-break loser), cancel the pending as ruling 32 specifies and install as
> responder. **If our static is smaller** (we would be the tie-break
> winner), return `AcceptError::Stale`, leave the pending in place, and
> record the candidate's timestamp in the guard exactly as §6.7's
> winner-side record does — our own outbound completes and the peer installs
> as responder.

This preserves everything ruling 32 bought: §16.1's invariant still holds on
the winner side (no second connection is installed), and the
`read_identity()` → `connect()` → `accept()` ordering still yields exactly
one connection. It also restores the two-sided agreement, and it drags the
secondary effect back to §6.7's stated bound.

No new error variant is needed (`Stale` already covers "the accept did not
install"; §18.1's `Stale` prose needs one more clause), no packet, header,
frame, timer, or constant moves. **Wire-free.** §18.1 and §5.4's PENDING row
need matching edits.

---

## SECV5-2 — the death clock's state *at install* is unspecified; under one faithful reading a half-open session is immortal, and three security arguments depend on it not being

**Severity: MAJOR** *(spec gap; two conformant implementations diverge on whether an attacker-induced half-open connection is ever reaped)*

### The gap

§7.4's death rule is conjunctive:

> *"The connection is dead when `now − last_authenticated_recv >
> DEAD_TIMEOUT` **and** at least one **arming** send has occurred since that
> last authenticated receive."*

The spec never states the initial values of `last_send` and
`last_authenticated_recv` at install, nor whether the handshake messages
(msg1 received / msg2 written) count toward either. `seal`/`seal_quiet` are
defined only over sealed **Data** packets (§7.4, §3.4), so on their face
neither handshake message touches `last_send`.

Now take a responder-side install whose initiator never completes — a
replayed msg1 accepted by an auto-accepting application — with an
application that queues nothing (§16.9's early sends are empty):

- **Reading A** (`last_send` initialised unset / −∞, msg1 counts as the
  last authenticated receive): §7.5's passive rule — *"a side that has
  received since it last sent, and has not sent for `KEEPALIVE_TIMEOUT`,
  sends a keepalive"* — fires at install + 10 s. The keepalive is a marking
  send, it arms, and the session dies at install + 25 s. Correct.
- **Reading B** (`last_send` initialised to install time alongside
  `last_authenticated_recv`): *"received since it last sent"* is false at
  install and stays false forever, because nothing is ever received. **No
  keepalive is ever sent. No arming send ever occurs. The death clause's
  second conjunct is never satisfied. The session lives forever.**

Reading B is not a strained reading — ruling 33's framing ("armed once per
receive, never re-armed") makes install-time naturally a receive, and
initialising both clocks to install time is the obvious implementation.

There is no backstop. §7.6 is **deleted** (the ratchet-only ruling), so
slither has no `REJECT_AFTER_TIME`-style unconditional session-age reaper
at all — WireGuard's has no counterpart here. §7.9's nonce exhaustion is
58 000 years away on an idle session. Liveness is the only reaper, and
under reading B it never fires.

### What depends on this

Three separate security arguments in the reviewed sections are unproven
under reading B:

1. **§17.1's honesty clause**, twice: *"or, if accepted, a half-open session
   reaped by liveness in 25 s"* (line 3083) and *"the resulting half-open
   session dies at 25 s liveness and leaks nothing"* (line 3099). This is
   the *entire* mitigation for orphan eviction re-admitting a replay.
2. **§6.7's honesty clause**: *"the resulting session carries nothing and
   dies at `DEAD_TIMEOUT` (§7.5)"* — the bound on what a replay that
   cancels a pending costs.
3. **§15.4's teardown matrix**, endpoint-dropped row: peer's view *"liveness,
   ≤ 25 s"*. A peer that sends nothing never observes it.

Under reading B, an attacker with one captured msg1 and an auto-accepting
victim mints a connection that is **never** reclaimed, holding the §17.5
per-connection commitment (advertised credit up to `INITIAL_MAX_DATA` =
1 MiB, the datagram queues ≈ 146 KiB, the replay window, stream
book-keeping). §17.5 files established connections under
*"application-governed — unbounded by the protocol"*, which is honest about
the count but assumes the protocol reclaims each one in 25 s.

### Suggested fix — **wire-free**

Pin the initial condition in §7.4, one sentence:

> A connection's death deadline is **armed at install**, and
> `last_authenticated_recv` is set to the install instant. A session that
> never receives an authenticated packet therefore dies exactly
> `DEAD_TIMEOUT` after install, whatever it sends or does not send.

This is strictly safe: arming never kills on its own, and a connection that
receives nothing for 25 s is dead by every other statement in §7.4/§7.5. It
makes readings A and B agree, and it makes all three claims above true by
construction rather than by inference through §7.5's passive rule. It also
removes an implementation-divergence hazard that no golden-wire or
size/constant test would catch.

No wire byte, timer value, or error variant moves. **Wire-free.**

---

## SECV5-3 — ruling 31's `None` basis plus withheld-genuine-Data replay is a *permanent* wedge; §6.8's "delayed by at most `DEAD_TIMEOUT`" is false against an attacker

**Severity: MAJOR** *(a maintainer could reasonably promote this to BLOCKER — see "why not BLOCKER" below)*

### First, the positive result: ruling 31 does close what it claimed

Directly attacked. A dialled B; A's `static → connection` entry for B has
`replacement_basis = None`. Attacker holds a genuine msg1 captured off the
wire from B — of any age, from any prior session.

- **Guard**: for a peer A has only ever *dialled*, A holds **no guard entry
  at all**. §17.1's four write sites are all post-`ss` reads of an inbound
  msg1; a `connect()` completed by msg2 writes nothing, and pinning
  *"never creates an entry"*. So the guard passes **vacuously**, forever.
- **Basis**: `None` → §6.4 returns `AcceptError::Stale`, live connection
  untouched.

The basis is doing 100 % of the work in exactly the case the guard cannot
cover. **Ruling 31's stated goal is achieved**, and §17.1's *"What makes the
replacement safe is the **pair**"* claim is correct in this direction. (It
is *not* correct in the other direction — see SECV5-5.)

### The wedge

The price ruling 31 pays is stated in §6.8:

> *"The restart still resolves with no machinery, delayed by at most
> `DEAD_TIMEOUT`: the zombie receives nothing it can open, so it dies at
> liveness."*

I verified the benign case: A's zombie **does** always arm (§7.5's passive
keepalive fires at `last_send + KEEPALIVE_TIMEOUT`, i.e. within 10 s + ε of
A's last authenticated receive, always strictly inside the 25 s deadline —
subject to SECV5-2's install-state gap), and B's retransmit train runs for
`HANDSHAKE_GIVEUP` = 90 s, comfortably longer than 25 s. **Absent an
attacker the claim holds at 25 s.** No wedge.

With an attacker, the premise *"the zombie receives nothing it can open"*
is false, and the wedge is permanent:

1. Attacker sits on the A↔B path during a normal session and **harvests**
   K genuine B→A Data packets with strictly increasing counters
   `c₁ < c₂ < … < c_K`, **dropping** all of them (and everything after) so
   A's replay window's largest never advances past `c₀ < c₁`. A busy
   session emits thousands per second; a few seconds of harvest is enough.
2. B restarts (or is taken down). B holds no keys and can send nothing A
   can open.
3. The attacker leaves the path entirely. From **anywhere**, it injects
   `c₁`. That packet is authenticated, window-fresh, and window-marked, so
   per §7.2/§7.3 it (a) **resets A's liveness clock**, and (b) **roams A's
   session endpoint to the attacker's own address**.
4. Every < 25 s the attacker injects the next `c_i`. A's zombie never dies.
   Store of K packets buys 25 K seconds — hours from a seconds-long harvest.
5. B's application reconnects. Its msg1 parks at A as an `Intro`. A's
   application accepts. §6.4: LIVE connection, basis `None` → **`Stale`**,
   forever, on every retransmit and every application retry.
6. A's application cannot dial out either: §16.1 — *"`connect()` to a static
   with a live `Connection` … returns `ConnectError::AlreadyConnected`"*.

**A and B can never re-establish.** Nothing in the protocol resolves it.
The only escape is application intervention: A's app must decide, on its own
initiative, to `close()` a connection that from every protocol-visible angle
looks perfectly healthy — packets keep arriving, `remote_address()` keeps
updating, `ConnEvent::AddressMoved` keeps firing. The application has no
signal distinguishing this from a legitimately roaming peer.

Pre-ruling-31 this wedge did **not** exist: with only the guard in play, B's
fresh initiation (vacuous guard pass, since A holds no entry for a peer it
only dials) was admissible as a replacement, and the restart resolved. Ruling
31 removed the escape hatch without removing the way an attacker keeps the
zombie alive.

Ruling 34 makes the attack 67 % cheaper in packets: one injected packet per
25 s instead of per 15 s, for the same wedge duration.

### Why not BLOCKER

The attacker needs a one-time on-path, drop-capable position — the same
position that already yields outright denial while it lasts. Nothing
cryptographic breaks. And there *is* an application-level escape. What is
genuinely new, and what makes it more than a restatement of the on-path
baseline, is **permanence after leaving the path**: a bounded harvest
converts into an unbounded, off-path, un-selfhealing denial of a specific
peer pair, plus persistent capture of A's session routing. §6.8 states a
`DEAD_TIMEOUT` bound on this delay with no adversarial caveat; that bound is
simply not true.

### Suggested fixes

Three options, in increasing cost:

**(a) Documentation only — wire-free.** Amend §6.8's *"delayed by at most
`DEAD_TIMEOUT`"* to state the premise it rests on: the bound holds only
where the zombie stops receiving authenticated packets, and an attacker
holding withheld genuine Data can hold a dialled connection open
indefinitely, in which case re-establishment requires the application to
close it. Also amend §17.4's basis prose, which presents `None` as costing
only a bounded delay. This is honest but leaves the wedge.

**(b) A contested-connection probe — wire-free, recommended.** When §6.4
refuses an accept because the live connection's basis is `None`, mark that
connection **contested**: immediately send an ack-eliciting PING on it
(both frames already exist, §8.3) and require an ACK covering that PING
within `KEEPALIVE_TIMEOUT`. If none arrives, fire
`ConnectionLost::TimedOut` and let the pending `Intro` be accepted on the
application's next attempt. This defeats the attack exactly: withheld
genuine Data can reset a receive clock, but it can never produce a **fresh
ACK of a packet we sent after the harvest**. It costs one PING per refused
basis-`None` accept — bounded by application-driven `accept()` calls — and
it does not weaken ruling 31, because a genuinely live peer answers the
PING and the refusal stands.

The general principle worth recording: **the death clock is driven by mere
authenticated receipt, not by acknowledged progress.** SECV5-3 is the
sharpest consequence, but any "is this peer still there" question in this
spec inherits it.

**(c) Give msg2 a timestamp — WIRE-AFFECTING.** The root cause is that a
dialler never learns a timestamp of the peer's, because *"msg2 carries no
payload"* (§5.2, §17.4). Twelve bytes of timestamp in msg2 makes the basis
`Some(t)` on both sides and deletes the `None` case entirely. This changes
msg2 from 107 B and turns the golden-wire pin red. **Needs a maintainer
ruling; not proposed.** I record it only because it is the fix that closes
the finding rather than mitigating it, and the maintainer should know the
wire-free options are mitigations.

---

## SECV5-4 — §6.7's "each captured initiation is single-use" is false: §17.1's orphan aging and eviction re-arm the replay, so a captured msg1 destroys a victim's `connect()` repeatedly

**Severity: MAJOR** *(a stated boundedness property does not hold; the residual is a durable, off-path, source-spoofed denial of a specific peer's dials)*

### The claim under test

§6.7's honesty clause, on a replay reaching the tie-break while we hold a
pending:

> *"The cost is bounded: **the admission records the timestamp, so each
> captured initiation is single-use**, the winner-side record above closes
> the same hole from the other direction, and the resulting session carries
> nothing and dies at `DEAD_TIMEOUT`."*

and §17.1's honesty clause, on what an evicted guard entry costs:

> *"the observable consequence of a re-admitted replay is a spurious
> **unaccepted** `Intro` attributed to a real peer at an attacker-chosen
> address — **never the destruction of a live connection**"*

### Why single-use fails

§17.1's own ruled mitigations recycle the entry that makes it single-use:

- **(ii) Timer aging** — *"orphans age out on an `INTRO_TTL`-scale timer as
  well as the LRU cap."* `INTRO_TTL` is **15 s** (§6.3), deliberately left
  at the pre-ruling-34 value.
- **LRU eviction** — the orphan tier is 1024 entries and §17.1 states it is
  *"attacker-triggerable on demand … ~1024 authenticate-then-drop chains
  flush the tier at ~2048 DH of our cost."*

So a guard entry is single-use only for as long as it survives. Pinning
protects it *"while a live `Connection`, an in-flight outbound pending, or a
staged mid-state exists for its static"* — and for a peer A only ever dials,
none of those exist between A's connection attempts.

### The sequence

A periodically dials B (an ordinary reconnect-with-backoff application).
A's static is lexicographically larger than B's. Attacker holds captured
genuine msg1s from B — a single observed 90 s retransmit train yields ≈ 18
of them (§5.5, one every ~5 s), each with a strictly greater timestamp.

1. Attacker sends a captured msg1 with **source address spoofed to B's
   address** — the address A dials for B, hence in A's hint set (§17.4).
   Off-path source spoofing suffices; no observation is needed, the
   attacker can fire blind every ~5 s and it lands whenever A has a pending.
2. §6.5 step 3: `src` ∈ hint set → eager `es`; claimed static B ∈ pending
   outbound remotes → **internal tie-break**.
3. §6.6: tag ✓ (genuine msg1). Guard: A holds no entry for B — A only ever
   dials B — so the check *"passes the guard vacuously"* (§6.7's own words).
   Tie-break: A's static is larger → A is the loser.
4. §6.6 step 4: A **cancels its pending** and installs as responder. Per
   §6.6, *"The admission completes the connection as an `Install` …
   resolving its `Connecting` exactly as a msg2 completion would."*
   **A's application is told its `connect()` succeeded.** It holds a session
   B knows nothing about.
5. A's static is now LIVE, so `connect()` is refused (§16.1) until the
   session dies at 25 s with `ConnectionLost::TimedOut`. A retries; the
   guard entry from step 3 is still present and pins.
6. Attacker fires the **next** captured msg1, timestamp strictly greater.
   Guard passes. Repeat from step 3.

**~18 captured initiations × 25 s ≈ 450 s of denial per harvested train**,
and unbounded whenever A's reconnect backoff exceeds the ~15 s orphan aging
window, because the entry then ages out and the *first* captured msg1
becomes replayable again.

Ruling 34 stretches each cycle from 15 s to 25 s, a 67 % increase in denial
per captured packet.

Two further points:

- §17.1's *"never the destruction of a live connection"* is a claim about
  the LIVE state only. Re-admission against a **PENDING** state destroys the
  pending, which is strictly worse than a spurious `Intro`: §6.7's tie-break
  cancel is not application-gated, and the victim is told the connect
  *succeeded*. §17.1's honesty clause does not cover this path at all.
- The fake session **anchors at the msg1 source**, so if the attacker
  spoofs a source that is in A's hint set for a *different* pending (§6.5
  step 3 tests hint-set membership globally but pending-outbound-remote
  membership per static), A aims msg2 and its subsequent output at a
  third-party address. §7.3's `AMPLIFICATION_FACTOR` = 3 budget caps this at
  ≈ 588 B per 196 B injected — the accepted ratio, so this is bounded and
  not a new reflector. Recorded for completeness only.

### Suggested fix — **wire-free**

Two parts, both text:

1. **Correct the boundedness claim.** §6.7 must not say "single-use"
   without qualification. It is single-use *per initiation, and only while
   that initiation's guard entry survives §17.1's LRU eviction and
   `INTRO_TTL`-scale aging*. A captured retransmit train is ≈ 18
   initiations. Cross-reference §17.1 explicitly.
2. **Extend §17.1's pin to cover the exposure.** The cheap structural fix:
   **a guard entry written by a tie-break admission or a winner-side record
   is exempt from orphan aging and LRU eviction for `HANDSHAKE_GIVEUP`
   (90 s) after the connection it created dies** — long enough to cover an
   application's reconnect backoff, and it is a per-static timestamp
   (≈ 45 B), not a session. Alternatively, pin the entry while the static
   has been dialled at any point in the last `HANDSHAKE_GIVEUP`. Either
   keeps the tier bounded (the 1024 cap still applies, and §17.1's
   mitigation (i) still prevents minting orphans by
   authenticate-then-drop).

A stronger structural option — *refuse to cancel a pending on a vacuous
guard pass* — is **not** recommended: it breaks genuine first-contact
simultaneous open (both sides' guards are empty, both would keep their own
pending, both install as initiator, mutual dark). Recorded so it is not
re-proposed.

No packet, frame, header, error variant, timer or constant moves.
**Wire-free.**

---

## SECV5-5 — §17.1's honesty clause is wrong for a static we only ever dialled: there is no guard entry to evict, so the spurious-`Intro` primitive is unbounded and repeatable

**Severity: MINOR**

§17.1's honesty clause justifies orphan eviction like this:

> *"pinning protects established statics, so **a live static's guard entry
> is never evicted and an older-or-equal replay of it still dies at the
> guard**"*

For a static we only ever **dialled**, that sentence is vacuously true and
its conclusion is false. §17.1's four write sites are all post-`ss` reads of
an inbound msg1; a `connect()` completed by msg2 writes no entry, and the
pin *"never creates an entry"*. So A holds **no guard entry for B**, and
the eviction argument never engages.

Consequence: an *older* captured msg1 from B — arbitrarily old, from a
long-dead session — replayed at A while A holds a dialled connection to B
passes the guard vacuously, authenticates, and surfaces as a fresh `Intro`
attributed to B at an attacker-chosen address. §17.1's mitigation (i)
(no-orphan-on-reject: *"its record drops with the chain; a pre-existing
entry reverts"*) means the guard is back to empty afterwards, so **the same
single captured packet is replayable forever**, not once. Each replay costs
A one stage-0 slot and whatever DH the application chooses to spend (up to
3), and fires the application's "peer is online" side-effects.

The live connection is safe — the basis is `None` and §6.4 returns `Stale`
(SECV5-3's positive result). Only the `Intro` spoofing is unbounded.

**Fix — wire-free.** Correct §17.1's clause to distinguish the two cases:
for a static we accepted, the pinned entry does the work described; for a
static we only dialled, **no entry exists and the guard offers nothing**,
so the replacement protection is the basis alone, and a replay of any
captured initiation surfaces as a spurious `Intro` indefinitely rather than
once. §6.4's *"What basis and guard bar, stated honestly"* bullet needs the
same distinction — it currently describes the guard as *"rejects any
candidate whose timestamp is ≤ the greatest this endpoint has admitted for
that static"* without noting that this set is **empty** for every peer we
only dial, which is the majority case for a client.

---

## SECV5-6 — the ordering of the guard's *record* against the basis check is unspecified

**Severity: MINOR**

§17.1 says guard admission is *"check **and** record"* and lists
*"`authenticate()` on the staged path"* and *"a re-homed `accept()`'s
candidate admission (§6.4)"* as write sites. §6.4 then applies the basis
rule at a **later** point (*"At admission, fast path or re-home: if a LIVE
connection exists…"*), and can return `Stale`.

So on the fast path the guard has already advanced at `authenticate()` by
the time the basis check refuses, and on the re-home path §6.4's own triple
(*"the timestamp guard admits its strictly-greater timestamp"*) records
before the basis bullet runs. Whether §17.1's mitigation (i) revert
(*"a pre-existing entry reverts"*) applies to an `AcceptError::Stale` — as
opposed to a dropped chain — is not stated.

I could not build an exploit from this: genuine initiations are
monotonically increasing (§5.3), so a guard advanced by a genuine captured
msg1 never blocks a genuine later one. But it is an unspecified ordering on
the one piece of endpoint-global replay state, and two conformant
implementations will differ on the guard's contents after a refused accept.

**Fix — wire-free.** State the order explicitly in §6.4: the basis check
runs **before** the guard record, or the record reverts on `Stale`. Either
is fine; pick one and say it.

---

## SECV5-7 — `PERSISTENT_KEEPALIVE` is provably inert at every value the floor admits, and §5.7's justification for the zero-margin coincidence is inverted

**Severity: MINOR** *(pre-existing; ruling 34 is what surfaced it, and §5.7 dismisses it on a sentence that says the opposite of the rule it cites)*

### The inverted sentence

§5.7, line 628–634:

> *"`PERSISTENT_KEEPALIVE`'s 25 s default now sits **exactly on** the
> liveness floor, **which rejects intervals below `DEAD_TIMEOUT`** (§7.5,
> §16.2) — 25 ≥ 25 holds with zero margin. Idle liveness is sustained by
> the 10 s passive keepalive dance (§7.5), never by the persistent beacon,
> which exists to hold a NAT binding open; **the floor's job is only to
> reject an interval so long that the beacon could not keep a connection
> alive on its own.**"*

The floor rejects intervals **below** `DEAD_TIMEOUT` — short ones. The
final clause describes rejecting **long** ones. §7.5 gives the actual
rationale (*"so a marking beacon can never outpace the death clock"*).
§5.7's sentence is the one used to conclude *"this is not a defect"*, and
it argues the opposite direction from the rule it cites.

### The substantive result: the knob can never fire

Let `I` be the configured interval, `S` = `last_send` (marking sends only),
`R` = `last_authenticated_recv`.

- The beacon fires at `S + I` (§7.5: *"it fires when no marking send has
  occurred for the configured interval, and re-arms from every marking
  send"* — receives do not reset it).
- The passive keepalive fires at `S + KEEPALIVE_TIMEOUT` whenever `R > S`,
  and it is a **marking** send. So `S` is dragged forward in 10 s steps for
  as long as receives keep arriving; the beacon's deadline is pushed with
  it and never reached.
- The only state in which the passive rule is blocked is `S > R`. Every
  send that can establish `S > R` is by definition a marking send (pure
  ACKs do not touch `last_send`), so the death clock is **armed** in that
  state, and death occurs at `R + DEAD_TIMEOUT`.
- In that state the beacon's deadline is `S + I > R + I ≥ R + DEAD_TIMEOUT`
  for every `I ≥ DEAD_TIMEOUT` — i.e. **strictly after death, for every
  interval the floor admits.**

`PERSISTENT_KEEPALIVE` is therefore unreachable as specified: it is
suppressed by the passive dance whenever the dance runs, and it fires
strictly after the death deadline whenever the dance does not. Its stated
purpose — holding a NAT binding on a connection that is not receiving —
is a state slither kills at 25 s anyway, because §7.6 is deleted and there
is no WireGuard-style age-based session refresh to survive into.

No security consequence: the failure mode is a knob that does nothing, not
one that does something dangerous. But §5.7 flags the coincidence as
"tight but safe", and the accurate description is "inert".

**Fix — wire-free.** Either (a) correct §5.7's justification sentence and
add the derivation above, documenting the knob as a no-op under the current
liveness model, or (b) if the knob is meant to work, the floor must become
`< KEEPALIVE_TIMEOUT`-relative rather than `DEAD_TIMEOUT`-relative and the
default must drop below `KEEPALIVE_TIMEOUT` — which needs a maintainer
ruling because §7.5 explicitly declined the alternative that makes short
intervals admissible. Constant values would move under (b), but no wire
byte does. Under (a) nothing moves at all.

---

## SECV5-8 — the one-lost-keepalive tolerance holds for unidirectional loss only; a single simultaneous bidirectional loss still kills an idle connection at 25 s

**Severity: NOTE**

Ruling 34's derivation (§5.7) and §7.5 both claim *"only **two consecutive**
lost keepalives still end an otherwise healthy idle connection."* I verified
the unidirectional case and it holds with margin:

- A's keepalive at t=10 is lost; B's arrives at A. A's `R`=10, `S`=10 → A
  keepalives again at t=20 and survives. B's `R`=0, `S`=10 → B sends nothing
  at t=20 (the passive rule's *"received since it last sent"* is false) but
  receives A's keepalive at t=20, inside its 25 s deadline. **5 s of margin;
  15 s would have failed.** Ruling 34's arithmetic is correct.

The counterexample is a **simultaneous bidirectional** loss — one loss
*event*, two packets, same interval:

- A's keepalive at t=10 lost, B's keepalive at t=10 lost. Both sides now
  have `S`=10 > `R`=0, so the passive rule's first conjunct is false on both
  sides, and keepalives are never retransmitted (§8.7 puts them in the
  *never* class and they never enter the sent map). **Neither side sends
  again. Both die at t=25.**

This is a common shape on a flapping path or a brief NAT/interface hiccup,
and "two consecutive" implies temporal consecutiveness rather than "two
packets in one outage".

**Fix — wire-free.** Restate §5.7 and §7.5 precisely: the tolerance is one
lost keepalive **in one direction**; a bidirectional outage spanning a
single keepalive interval ends an idle connection, because the passive rule
is one-shot per receive and keepalives are never retransmitted. No value
needs to change — the residual is already the sort §7.5 names openly
(*"slither never reconnects on its own"*), it is just narrower than the
text claims.

---

## SECV5-9 — §10.3 carries only half of ruling 33's redefinition

**Severity: NOTE**

Ruling 33's terminology clause (§7.4) is explicit and correct:
*"'**Liveness-neutral**' everywhere in this document means **non-marking**
… every ack-eliciting member of the quiet set (credit frames,
retransmissions, PTO probes) is liveness-neutral **and** arming."*

I checked all four downstream sites:

| Site | Text | Verdict |
|---|---|---|
| §7.5 PING bullet | *"liveness-neutral, i.e. non-marking; being ack-eliciting it still **arms** the death clock"* | ✅ explicit both halves |
| §13.3 | *"a probe is ack-eliciting, so the *first* probe arms the death deadline … and no later probe re-arms it"* | ✅ explicit |
| §13.4 | *"Probes are sealed `seal_quiet` (liveness-neutral, §7.4) **and** exempt from the congestion admission gate"* | ✅ correct; arming covered by §13.3 one section up |
| §14.5 | *"the exemption is orthogonal to, and coexists with, the probe's liveness-neutral `seal_quiet`"*; *"Non-ack-eliciting control packets — pure ACKs, CLOSE, keepalives"* | ✅ consistent with §8.3/§8.7's table |
| §10.3 | *"Credit frames are ack-eliciting … yet liveness-neutral (§7.4)"* | ⚠️ half only |

§10.3 is not *wrong* under §7.4's definition, but credit frames are the
exact class ruling 33 was written for — §7.4 names them first in *"every
ack-eliciting member of the quiet set (credit frames, …) is
liveness-neutral **and** arming"* — and §10.3 is where an implementer reads
about them. **Fix — wire-free:** append *"— and, being ack-eliciting, they
arm the death clock (§7.4)"*.

---

## SECV5-10 — resource-holding: what ruling 34 stretched by 67 %, and whether §6.9 / §17.5 are now wrong

**Severity: NOTE** *(no bound in §6.9 or §17.5 is wrong; recorded so the maintainer has the enumeration)*

Every state whose lifetime is `DEAD_TIMEOUT`-bounded:

| State | Where | Effect of 15 s → 25 s |
|---|---|---|
| a half-open session installed by a replay that cancelled a pending | §6.7, §6.6 step 4 | holds a full §17.5 per-connection commitment (≤ 1 MiB advertised credit + ≈ 146 KiB datagram queues + replay window + streams table) for 25 s instead of 15 s. Combined with SECV5-4, an attacker sustains this; combined with SECV5-2's gap it may never be reclaimed at all. |
| a half-open session accepted from an orphan-eviction-re-admitted replay | §17.1 | same, ×1.67; the orphan tier is attacker-flushable (~2048 DH), so the count is application-governed, not protocol-bounded — §17.5 already says so honestly |
| a zombie connection awaiting liveness after a peer restart | §6.8 | the reconnect stall on a **dialled** connection rises from 15 s to 25 s in the benign case, and is unbounded under SECV5-3 |
| a mutually-dark pair after a tie-break divergence | SECV5-1 | 25 s of both-sides-believe-connected instead of 15 s |
| the epoch-drift window | §7.7 | correctly re-derived in the spec (196 608 / 25 ≈ 7.9 k pps, ≈ 75 Mbps) — the spec states plainly that ruling 34 *"widens the window rather than narrowing it"* and rests the argument on §14.5's ACK-driven gate instead, with liveness as backstop. Sound. |
| unvalidated-address budget lifetime | §7.3 | *"a path that sends nothing dies by liveness inside 25 s"* — correctly updated; the 3× ratio is unchanged so no amplification bound moves |
| CLOSE linger | §15.1–§15.2 | `CLOSE_LINGER` = 5 s, **independent of `DEAD_TIMEOUT`** — unaffected ✅ |
| stage-0 queue, mid-states, per-source cap, flood arithmetic | §6.3, §6.9 | keyed on `INTRO_TTL` = 15 s, deliberately untouched — the ≈ 68 packets/second figure (1024 / 15 s) is still right ✅ |

**§6.9 is not stale.** Its rows are DH costs and per-packet ceilings; none
is `DEAD_TIMEOUT`-derived. Its one time-derived row (*"≤ 1 CLOSE reply per
second"* during the 5 s linger) uses `CLOSE_LINGER`, unchanged.

**§17.5 is not wrong.** Its ceilings are counts and per-item sizes, not
lifetimes, and the established-connections row already reads
*"application-governed — unbounded by the protocol"*. What §17.5 does *not*
say, and now should, is that the per-connection term is reclaimed on a
`DEAD_TIMEOUT` schedule that is 67 % longer and — per SECV5-2 — conditional
on an arming send that the spec does not guarantee occurs.

---

## §18.1 — the error taxonomy, the two changed branches

Both changed branches check out.

- **`ConnectError::AlreadyConnected`** now doubles as *"the resolution of an
  in-flight `Connecting` cancelled by a racing `accept()` on the same proven
  static (§6.4's PENDING branch)"* — consistent with §6.4 and §16.1. ✅
  Under SECV5-1's fix this stays correct; the winner-side path adds no new
  variant.
- **`AcceptError::Stale`** now covers *"no initiation for the proven static
  is parked, **or** the admitted candidate fails the replacement-basis
  rule"* — consistent with §6.4. `AcceptError::AlreadyConnected`'s deletion
  is justified correctly (*"a basis-passing proven-LIVE `accept()` is a
  replacement"*), and §18.1's opening *"`AcceptError::AlreadyConnected`
  appears nowhere"* is not contradicted anywhere in the document. ✅

One observability gap, **NOTE**: `AcceptError::Stale` now conflates three
operationally distinct outcomes — nothing parked (benign), candidate not
newer than basis (a replay or a clock problem), and **basis is `None`**
(the structural refusal of §6.8, the case an operator must be able to see,
because under SECV5-3 it is the only symptom of the wedge). A trace field
distinguishing them on `slither::policy` would cost nothing and is
wire-free; a separate variant would be a taxonomy change and is not
recommended.

---

## Attacks tested, and the result of each

| Attack | Result |
|---|---|
| Passively-captured msg1 replayed to destroy a live **dialled** connection | **Fails.** Basis `None` → `Stale`. Ruling 31 confirmed. |
| Same, against a live **accepted** connection with an *older* timestamp | **Fails.** Guard rejects (entry is pinned while the connection lives). |
| Same, with a *strictly newer withheld* timestamp against an accepted connection | **Reaches the application as an `Intro`; destroys nothing until accepted.** Exactly as §6.4/§17.1 state. Residual accepted by ruling. |
| Genuine peer restart against a `None` basis, benign network | **Resolves in ≤ `DEAD_TIMEOUT`.** Arming is guaranteed by §7.5's passive keepalive within 10 s. No wedge. Claim verified at 25 s. |
| Same, with an attacker dripping withheld genuine Data | **Permanent wedge — SECV5-3.** |
| Forged (non-genuine) msg1 cancelling a pending | **Fails** at the msg1 tail tag; pending untouched (§6.7). Confirmed. |
| Replayed genuine msg1 cancelling a pending, once | **Succeeds** — disclosed by §6.7. |
| …repeatedly, from one harvest | **Succeeds — SECV5-4.** The "single-use" bound does not hold. |
| Both peers on different admission paths (one tie-break, one PENDING branch) | **Mismatched installs, mutual dark — SECV5-1.** |
| Both peers on the PENDING branch | **Mismatched installs 100 % of the time — SECV5-1.** |
| Guard admits what the basis should reject | **Not found.** The basis is strictly the tighter test on the LIVE path. |
| Basis admits what the guard should reject | **Not found.** Both must pass; §6.4 requires guard-then-basis. |
| Vacuous guard on an unseen static | **Confirmed reachable, and worse than documented for dialled statics — SECV5-5.** No entry ever exists, so it is not an eviction artefact. |
| Arming at an unhelpful moment killing a healthy connection | **Not found.** Arming alone never kills; 25 s of receive silence is independently required. |
| Sending into a black hole without arming (pure ACKs / CLOSE / keepalives / PINGs) | **Not found post-install.** Pure ACKs and CLOSE do not arm, but the passive keepalive (marking) always fires within 10 s of the last receive and arms. PINGs arm (§13.3). Ruling 33 confirmed. |
| The same, **at install**, before any send | **Found — SECV5-2.** Unspecified initial state; one faithful reading yields an immortal session. |
| Probe train deferring or escaping death | **Not found.** One-shot arming makes the train terminate within 25 s of the last receive (§13.3). Confirmed. |
| `PERSISTENT_KEEPALIVE` at the zero-margin floor | **Not dangerous — but inert. SECV5-7.** |
| Reflector via a spoofed hint-set source anchoring a session at a third party | **Bounded at 3×** by §7.3's budget, as designed. No new amplification. |
| §6.9 / §17.5 bounds invalidated by 25 s | **No.** Enumerated in SECV5-10. |

---

## Summary

| ID | Severity | Wire-affecting fix? |
|---|---|---|
| SECV5-1 — PENDING branch breaks simultaneous-open convergence | MAJOR | No |
| SECV5-2 — death-clock state at install unspecified; half-open may be immortal | MAJOR | No |
| SECV5-3 — `None` basis + withheld-Data replay = permanent wedge; §6.8's bound false | MAJOR | No for the mitigation (b); the *closing* fix (timestamp in msg2) **is** wire-affecting |
| SECV5-4 — "each captured initiation is single-use" false under §17.1 aging/eviction | MAJOR | No |
| SECV5-5 — §17.1's honesty clause wrong for dialled statics | MINOR | No |
| SECV5-6 — guard-record vs basis-check ordering unspecified | MINOR | No |
| SECV5-7 — `PERSISTENT_KEEPALIVE` inert; §5.7's justification inverted | MINOR | No under (a); constant values move under (b) |
| SECV5-8 — one-lost-keepalive tolerance is unidirectional only | NOTE | No |
| SECV5-9 — §10.3 carries half of ruling 33's redefinition | NOTE | No |
| SECV5-10 — `DEAD_TIMEOUT`-bounded resource enumeration | NOTE | No |
| §18.1 `Stale` conflates three outcomes (observability) | NOTE | No |

**Every fix proposed here is wire-free.** The one wire-affecting option
(SECV5-3(c), a timestamp in msg2) is recorded but not proposed, precisely
so the maintainer knows the wire-free path for SECV5-3 is a mitigation and
not a closure.

The four rulings are, on balance, right: rulings 31, 33 and 34 achieve what
they set out to achieve, verified by direct attack rather than left
unchallenged. Ruling 32 achieves its stated goal — §16.1's invariant — but
its chosen mechanism is unsound against the peer, and SECV5-1's fix
preserves the goal while restoring the two-sided agreement. The two
findings the maintainer should weigh hardest are SECV5-1 (a correctness
break between honest peers, cheap to fix) and SECV5-3 (the cost ruling 31
took on, which is larger than §6.8 states).

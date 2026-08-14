# Re-review — Clusters A (continuation/restart) and C (roaming + anti-amplification)

> Focused re-reviewer, 2026/08/14. Target: `SPEC-DRAFT-v2.md`. Method:
> attack the *new* mechanisms fresh, then confirm each original finding is
> genuinely closed (not relabelled) and hunt for reviser-introduced defects.
> Scope: SEC-1,3,4,5 · FID-1,2,6,11 (plus the FID-13 / min_rtt riders and the
> post-`ss` tie-break non-regression). B/D/E/F/G/H left to the other lane.

---

## Cluster A — continuation / restart machinery

### The state space is exhaustive and internally consistent

Routing (§6.5) keys on the **claimed** static's membership in the known-static
set (established ∪ pending-outbound); flag-routing (§6.6 step 3) keys on the
**proven** static's local state after `ss`. Enumerating every cell:

| Local state (proven) | CONTINUATION | Routes to | Action | msg2 CONTINUED |
|---|---|---|---|---|
| LIVE | 1 | continuation | rekey swap, `Install{initial:false}` | 1 |
| LIVE | 0 | continuation | park **frozen restart-replacement** `Intro`; live conn runs; teardown at `accept()` | 0 (from accept) |
| PENDING | 0 or 1 | continuation | step 3 **skipped**; tie-break (§6.7) | 0 (loser `Install{initial:true}`) |
| NONE | 1 | staged path | fresh/staged accept (or re-home, §6.4) | 0 |
| NONE | 0 | staged path | ordinary fresh accept | 0 |

Every cell has a defined action. §6.5 sends NONE to the staged path and LIVE/
PENDING into the continuation; §6.6 step 3 is explicitly three-valued and fires
only at LIVE; NONE is unreachable in the continuation. **§5.4's table and §6.6
step 3 agree row-for-row — no contradiction.** The initiator rule (§5.4)
covers all four sent×received combinations: sent-0/recv-0 and sent-1/recv-1
succeed; sent-1/recv-0 ⇒ `PeerRestarted`; sent-0/recv-1 ⇒ "no honest responder
can produce it", discard + spend. I also walked the asymmetric restart-race
(initiator LIVE rekeying vs. responder PENDING after its own restart): the
computed CONTINUED + initiator rule resolve it correctly (winner completes its
outbound, reads CONTINUED=0, dies `PeerRestarted`, reconnects; responder zombie
reaped at 15 s). No silent transport-generation merge is reachable.

### Original findings

- **FID-1 — CLOSED.** §6.6 step 6 computes `CONTINUED = 1` **iff** the admission
  swaps into a retained-transport connection; every other admission (tie-break
  loser `Install{initial:true}`, restart-replacement, fresh accept) is 0. I
  traced simultaneous open end to end (§6.7): both diallers send CONTINUATION=0;
  the loser cancels its pending post-`ss`, admits the winner's msg1 as responder,
  answers CONTINUED=0; the winner (sent 0) accepts CONTINUED=0 and completes;
  loser's own `Connecting` resolves via `Install{initial:true}`. It **completes**
  — FID-1's 90 s death is gone.
- **FID-2 — CLOSED.** §6.6 step 3 fires only at LIVE; PENDING "skip this step
  entirely … proceed to pacing and the tie-break." No restart/rekey branch
  pre-empts the tie-break.
- **FID-11 — CLOSED.** §6.4 deletes the candidate-flag discard: a CONTINUATION=1
  re-home candidate is admitted (same proven static + tail tag + guard),
  answered CONTINUED=0, with the rationale that a candidate reaching re-home has
  no live connection by construction. The post-restart re-home livelock cannot
  recur. `AcceptError::AlreadyConnected` added (§6.4, §18.1).
- **SEC-3 / IMP-6 — CLOSED.** §6.3 "freeze-on-carry": any mid-state-carrying
  entry (eager-demoted §6.5 step 3, or restart-parked §6.6 step 3) is **consumed
  from the moment it parks** — bytes + `IntroId` frozen, later same-source
  arrivals park separately, accessors never straddle two initiations. An
  unauthenticated mac1-valid packet can no longer byte-replace DH-paid state, so
  `read_identity()`/`authenticate()` cannot surface an identity for bytes that
  identity never sent. `Superseded` deleted from every enum (§6.3, §18.1).
- **SEC-4 — CLOSED.** Three-valued local state throughout §5.4 and §6.6.
- **SEC-5 — CLOSED.** The `Replaced` teardown is deferred from step 3 to
  `accept()` (§5.4 table, §6.6 step 3, §6.4 "one teardown path", §15.4 row,
  §17.1 honesty clause rewritten). A withheld/injected CONTINUATION=0 retransmit
  now parks one frozen `Intro` and tears down **nothing** until the application
  accepts. I checked the two hazards the task named: **no two-connection
  coexistence** (a parked/frozen Intro is not a connection; §6.4's guard makes
  every ordinary accept at a LIVE static `AlreadyConnected`; only a
  restart-replacement Intro displaces, and it does so by tearing the old one
  down), and **no stale-LIVE lingering** (a genuine restart's zombie dies at
  `accept()` or at 15 s liveness; an unaccepted injected replay expires at
  `INTRO_TTL`). The residual — the app *choosing* to accept a replacement — is
  the intended, documented consequence (§17.1), not a hole.

### Post-`ss` tie-break — NOT regressed

The tie-break still runs only at §6.6 step 5 / §6.7, strictly after the tag
(step 1), on the **proven, canonical (§2.4), equal-length** static — never a
claimed one. "A match detected at `es` selects the path but decides nothing; an
`ss` failure is a silent drop with the pending untouched — a forgery cannot
cancel a pending." Freeze-on-carry *strengthens* the surrounding invariant. The
prior pipeline's catastrophic tie-break regression did **not** recur.

### §6.9 DoS accounting — still holds

2-DH ceiling per attacker packet intact (forged-known-static, replayed-genuine,
genuine-replacement, forged-msg2 all cap at 2 DH; eager `es` = 1 DH); per-source
cap counts unconsumed + consumed chains; msg2 108 B / msg1 197 B keeps
amplification < 1; "No amplification" restated as "no path emits bytes to an
address that has not authenticated," now cross-referencing the §7.3 budget.

---

## Cluster C — roaming + anti-amplification

- **SEC-1 — CLOSED.** §7.3 arms a send-side budget on every roam and every
  msg1-source anchor: unvalidated ⇒ bytes-sent ≤ `AMPLIFICATION_FACTOR`(=3) ×
  bytes-received, **binding all output including the §14.5/§13.4 cwnd-exempt
  classes** (§14.5 "the exemptions are cwnd-scoped only"; §13.4 "probes are not
  exempt from §7.3's budget"). The forged-ACK reflection is now bounded: however
  the peer regrows cwnd or pumps exempt ACK/CLOSE/keepalive/PTO output, the
  budget caps emission to 3× received on the unvalidated address. **The 3× bound
  survives attacker self-refill** — the attacker must transmit ≥ ⅓ of any flood
  it reflects (byte-for-byte cost symmetry, since every refill byte is one the
  attacker spent claiming source V); the reflection incentive is removed.
  Initiator anchors the dialled address (§5.5 step 4, not budget-armed — so
  `connect()` can send msg1), responder anchors and gates the msg1 source
  (§5.6).
- **FID-6 — CLOSED.** §13.6 + §14.6: on roam the sent map is **kept** (old-path
  ACKs still retire frames — nothing stranded), the recovery-period marker is set
  to the roam instant (not cleared), and pre-roam packets are fenced from
  congestion events, the persistent-congestion walk, RTT sampling, and
  `app_limited` growth **while still resolving for loss/retransmission**. The
  collapse-to-`MINIMUM_WINDOW` is gone; the residual stall is bounded to one
  loss/PTO cycle, kept probeable by the PTO exemption *within* the budget.
- **FID-13 + min_rtt rider — CLOSED / sound.** §14.6 states the RFC 9000 §9.4
  divergence (keeping RTT as a prior is a deliberate slither choice) and pairs it
  with the fence and the re-seed. §13.1: `min_rtt` is re-seeded from the first
  post-roam sample and **MUST be allowed to rise**. Adversarial check: a peer
  controlling ACK timing cannot drive PTO pathological — PTO ≥ `MAX_ACK_DELAY`
  regardless of a forged tiny sample, and any resulting probe burst is
  budget-bound on an unvalidated address; a high re-seed only self-slows the
  peer's own connection, bounded by liveness/`REJECT_AGE`. No stall, no storm.

---

## Cross-cluster composition

The budget and the handshake mac1 gate compose without deadlock. Responder:
msg2 (108 B) ≤ 3 × msg1 (197 B), so the anchor budget always admits the reply;
each fresh-ephemeral retransmit re-funds the budget. Initiator: the dialled
address is app-chosen and not budget-armed, so msg1 and its retransmits flow;
the peer's real address arrives later as a budget-armed roam that its own
authenticated data funds. Staged accept sends nothing before `accept()`, so
staging never touches the budget. A legitimate connect completes; a spoofed
anchor/roam reflects at most 3× then stalls and dies by liveness. Cluster A's
freeze-on-carry and Cluster C's budget do not interact.

---

## New defects introduced by the reviser

### RAC-1 — LOW — freeze-on-carry parks endpoint-frozen entries in the non-evictable "consumed" tier that §6.3 assumes is app-held

**Section:** §6.3 (per-source cap + overflow), §6.9 (rate-honesty notes).

Freeze-on-carry (the SEC-3 fix) reclassifies eager-demoted and restart-parked
entries from *unconsumed* (evictable, byte-replaceable — the v1 state) to
*consumed* (frozen, **non-evictable**: "eviction operates on the unconsumed tier
only"). But §6.3 justifies non-eviction with "consumed chains are DH-paid **and
app-held**" — which is false for an eager-demoted frozen entry: it is DH-paid (1
DH for `es`) but the application never held it (it is endpoint-created and may
never be read). A spoofing attacker that reaches the eager path (spoofs a
hint-set source — an existing connection's endpoint IP:port — and claims an
*unknown* static) mints up to `INTRO_MAX_PER_SOURCE` (4) non-evictable frozen
consumed chains per /64 at 1 DH each. Because "if the source's whole allowance is
held by consumed chains, the arrival is dropped," a genuine **new** peer sharing
that /64 is denied a slot — the §6.3 evict-oldest guarantee "a genuine
initiation always obtains a slot" no longer holds for a /64 that already hosts a
connection. In v1 those attacker entries were a single evictable, replaceable
slot; freeze-on-carry makes them 4 non-evictable ones.

Impact is bounded and within the already-conceded, cookies/mac2-deferred (§19)
accept-DoS posture: 4 slots per /64, 1 DH each, `INTRO_TTL` = 15 s self-expiry,
**no effect on established connections or their rekeys** (which bypass the queue
via the continuation), and it requires the attacker to spoof a specific
existing-connection endpoint. It is a documentation/honesty gap plus a marginal
lowering of a targeted new-accept-denial cost, not a correctness or
amplification break.

**Fix (no wire byte):** (a) correct §6.3's "app-held" wording to acknowledge
that freeze-on-carry entries are endpoint-consumed, not app-held; (b) extend
§6.9's rate-honesty note (already covering the ungated eager `es`) to state that
each eager-demoted frozen entry occupies a non-evictable consumed slot, so the
per-source cap — not evict-oldest — is what bounds this occupant surface until
the cookie tier. Optionally cap eager-demoted frozen entries at a sub-allowance
so a genuine same-/64 peer keeps an evictable slot.

### RAC-2 — LOW (clarity) — §7.3 budget rule omits the "authenticated" qualifier its own intent assumes

**Section:** §7.3.

The normative rule reads "…× total bytes **received from it** on this session,"
with no "authenticated" qualifier — yet §7.2 restricts liveness/roaming to
"authenticated **and** window-marked" packets, and §7.3's own prose says "its
own **authenticated** traffic funds the budget." The 3× reflection bound holds
either way (any bytes claiming source V cost their sender proportionally, so
counting unauthenticated bytes grants no amplification and the malicious peer
must send authenticated packets anyway to keep the session alive/roamed), so
this is **not exploitable** — but a naive implementation counting raw UDP bytes
from the current endpoint is invited by the wording. **Fix (no wire byte):** say
"authenticated bytes received," matching §7.2 and the budget's stated funding
model.

---

## Verdict

Both clusters are **sound**; another full revision is **not** needed. Cluster A
is fully closed: the three-valued state machine is exhaustive and §5.4/§6.6 are
consistent, simultaneous open completes with a computed CONTINUED, the `Replaced`
teardown is correctly deferred to `accept()` with no two-connection or
stale-linger hole, freeze-on-carry closes the byte-replacement surface, re-home
admits regardless of flag, and — critically — the post-`ss` tie-break did **not**
regress. Cluster C is fully closed: the `AMPLIFICATION_FACTOR`=3 budget binds the
previously-exempt classes and bounds forged-ACK reflection to 3×, the pre-roam
fence keeps the sent map while stopping cwnd collapse, and the min_rtt re-seed is
adversarially safe. The two defects found (RAC-1, RAC-2) are both **LOW**,
documentation-grade, and move **no wire byte**; each should be folded into the
next editorial pass but neither blocks ratification of Clusters A and C.

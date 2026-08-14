# Re-review (round 3) — SPEC-v2-DRAFT-v2.md (phase 1)

Reviewer lens: **security only**. Object: `scratchpad/SPEC-v2-DRAFT-v2.md`
(round-2 revision, 1484 lines). Checklist: `scratchpad/round2-resolutions.md`
(Groups 1–9). Prior review: `scratchpad/review-security.md` (findings 1–22).
Grounding: v1 `src/session.rs`, `src/endpoint.rs`.

Line numbers are 1-based into `SPEC-v2-DRAFT-v2.md`.

---

## 1. Prior findings — CLOSED / PARTIAL / OPEN

| # | Sev | Title | Status | Why |
|---|---|---|---|---|
| 1 | BLOCKER | replay roams the endpoint (shed rule) | **CLOSED** | §8 lines 1013–1046: window marked on *every* authenticated fresh packet, shed or delivered; "liveness and roaming are driven only by a packet that is both authenticated and window-marked"; sequence marks **before** roam/liveness (1024–1027). `shed_mask` (1034–1046) records shed counters, subtracted from the snapshot at ACK build. Restores v1 atomicity (session.rs:291→300 order). Slides synchronously with the window — safe (§4 interp 5). |
| 2 | BLOCKER | stage-0 queue = total accept denial | **CLOSED** (posture, [MAINTAINER]) | §4 lines 208–258: evict-oldest (244–249), `INTRO_MAX_PER_SOURCE=4`/IP (239–243), `INTRO_TTL=15 s` (250–258). Honesty clause (316–334) now accurate: no spoofing needed, endpoint-wide denial, ≈68 pps, not-WG-equivalent. Residual reopening via consumed tier → **NEW-3**. |
| 3 | BLOCKER | source-addr supersession clobbers DH-paid work | **CLOSED** | §4 lines 260–307 own-bytes-on-consume: `read_identity()` transfers ownership of bytes+`IntroId`; a consumed chain "can therefore never be superseded by any later packet" (271–278). The 0-DH clobber is gone. Occupancy residual → **NEW-3**. |
| 4 | BLOCKER | simultaneous open does not converge | **CLOSED for convergence / re-opened as NEW-1** | §5 lines 409–447 lexicographic-static tie-break converges in **both** orderings (re-derived below); exactly one session, both hold it. **But the fix cancels the pending pre-`ss` on an unauthenticated claim → NEW BLOCKER-1.** |
| 5 | MAJOR | eager-demote costs 3 DH | **CLOSED** | §5 step 3 (369–378) carries the paid mid-state; `read_identity()` free (0 incremental); §4 † footnote (147–151); DoS row 529 = 1 DH. Cumulative table (1/2/4) restored. |
| 6 | MAJOR | "off-path 0 DH strictly better" false | **CLOSED** | §5 lines 548–562 restated as capability (0 DH core / 1–2 DH vs a policy app + one slot); "4-tuple"→"(IP,port)…or any address we have dialled"; ≈0-bit entropy; roam-obtainable note. |
| 7 | MAJOR | DoS table missing replayed-msg1 row | **CLOSED** | §5 lines 531 (replayed genuine msg1 = 2 DH, dies at guard, not pacing-capped), 532 (split uncapped 2 DH + accepted-only 2 DH), 534–536 (explicit 2-DH ceiling). Re-derives (§2 below). |
| 8 | MAJOR | guard-eviction clause incomplete | **CLOSED** | §7 lines 875–916: attacker-triggerable on demand (~2048 DH of our cost), LRU adversarially optimal, spurious-`Connection` observable; mitigations no-orphan-on-reject + timer aging; LRU-use = admission only. Interaction checked safe (interp 3). |
| 9 | MAJOR | `Superseded` unrepresentable / `AcceptError` unenumerated | **CLOSED** | Removed by deletion (§4 279–286, §10 1138–1156); `AcceptError::{Expired,AlreadyConnected,EndpointDropped}` enumerated. No real supersession event left unrepresentable (checked below). |
| 10 | MAJOR | supersession surfacing undefined | **CLOSED** | §4 265–269: one `IntroReady` per unconsumed source; consumption frees the dedup slot → a later initiation surfaces a fresh `IntroReady`. |
| 11 | MAJOR | attempt-spend handshake denial | **CLOSED for accounting / re-opened as NEW-2** | §5 538–546 states the on-path exposure; precondition tightened (505). **But the mitigation "immediately re-arms the retransmit" (508–512) introduces an amplification storm → NEW MAJOR-2.** |
| 12 | MINOR | swap-cut "nothing lost" overstated | **CLOSED** | §8 940–942 qualified "once the replacement completes on both sides; a lost msg2 leaves … one-way-dark … against the 15 s liveness budget." |
| 13 | MINOR | peer-restart loss also ACKed | **CLOSED** | §11 1202–1204: "acknowledged … silent, confirmed, and undetectable at both ends." |
| 14 | MINOR | membership oracle dropped | **CLOSED** ([MAINTAINER]) | §5 397–407 restates as a *liveness* oracle + `Internal`/`Claimed` discriminator, acceptance carried. |
| 15 | MINOR | durable-key warning too narrow | **CLOSED** | §4 159–166 extends to claimed static, source, and `sender_index`. |
| 16 | MINOR | pacing scope under-specified | **CLOSED** | §5 480–491 re-scoped per known static; counter in the guard entry; 20 ms spacing; severability note. |
| 17 | MINOR | no composite state bound | **CLOSED** | §7 917–927 ceilings table (mid-states / 1024 provider handles / guard / unbounded conn count). |
| 18 | MINOR | hint set obtainable by roam | **CLOSED** | §5 558–562. |
| 19 | NIT | continuation as numbered list | **CLOSED** | §5 456–466 (tag→guard→pacing; record on full admission only; drop covers all three). |
| 20 | NIT | index-routes-but-fails-to-open | **CLOSED** | §7 845–848. |
| 21 | NIT | record no-amplification | **CLOSED** | §5 564–568. |
| 22 | NIT | what `slither::policy` traces now | **CLOSED** | §10 1167–1171. |

**All 11 prior BLOCKER/MAJOR findings are CLOSED in their own terms, and all
4 prior BLOCKERs are closed.** But two of the closures (findings 4 and 11)
were achieved by rewrites that introduce fresh holes (NEW-1, NEW-2), and two
(findings 2 and 3) leave an occupancy residual through the shared-budget model
(NEW-3).

---

## 2. DoS table — re-derived from the v2 normative text

Endpoint-core inbound `HandshakeInit` path (§5 349–378, §4):
stage-0 hash (always) → hint check → eager read `es` (1 DH) if `src∈hint` →
claimed∈known? → continuation `complete()` `ss` (+1) → tag/guard/pacing →
admit `ee,se` (+2). Demote carries the paid `es`; `read_identity` on it free.

| Packet class | Our cost | Check |
|---|---|---|
| mac1-invalid / wrong-key | 0 | ✓ |
| mac1-valid, `src∉hint`, `Intro` unprobed | **0 DH** + 1 slot | ✓ |
| mac1-valid, `src∉hint`, app probes then drops | 1 DH (app-chosen) | ✓ |
| mac1-valid, spoofed into hint, claimed unknown | **1 DH** (es carried; later read free) | ✓ demote now 1 DH (finding 5) |
| forged claim of a known static | **2 DH** (es+ss), dies at tag | ✓ |
| **replayed genuine msg1 from hint-set source** | **2 DH** (es+ss), dies at guard, **not** pacing-capped | ✓ present + correct (531) |
| genuine replacement from key-holder | 2 DH/init uncapped + 2 DH for ≤50/s accepted | ✓ split (532) |

- **2-DH ceiling holds.** An *attacker* packet (forged or replayed) maxes at
  `es+ss = 2 DH`; the further `ee,se` (→4 DH) is reachable only after
  `complete()` succeeds, i.e. only for a genuine key-holder's initiation
  (forger dies at the tag). No interleaving beats 2 DH.
- **Demote path is 1 DH** (was the finding-5 3-DH `es,es,ss`; now carried).
- **Tie-break path ≤ 2 DH**: win = drop msg1 (1 DH); lose = `complete()`
  then tag-death on a forgery (2 DH). DH-wise fine — but the *pending
  cancellation* on that path is NEW-1.
- **Cheapest sustained primitive**: replayed-genuine-msg1, 2 DH/packet at
  line rate, uncapped. Correctly named the design's floor.

The table survives re-derivation. The DoS *cost* accounting is sound.

---

## 3. NEW findings

### NEW-1 — BLOCKER — the tie-break cancels a genuine outbound pending on an *unauthenticated* claim (pre-`ss`)

**Where.** §5 *Simultaneous open*, lines 428–429 ("we cancel our pending at
the tie-break decision (**pre-`ss`**; the pending and its index are dropped —
no give-up, no error)") and 456–458 (continuation step 1: `complete()` is
where a forged claim of a known static "dies here at the msg1 tail's AEAD
tag"; "a lost tie-break has already cancelled our pending").

**The hole.** The tie-break fires on the **claimed** static, which the spec
itself declares attacker-choosable at zero secret (§4 159–166: "claim is not
yet proof"; the eager read only decrypts `s` under `es`, it does not prove
possession). So an attacker crafts a msg1 that decrypts to peer **B**'s static
without holding B's private key (encrypt `s = B_static` under
`es = DH(e, our_static_pub)`; `our_static` is public). Ordering of events on a
victim **A** that holds an in-flight outbound pending to **B**:

1. A's eager read (or the `read_identity()` interception if A's app probes a
   parked `Intro`) yields claimed static = B → matches A's pending → tie-break.
2. If `static_B < static_A` (fixed ≈50% of pairs, attacker cannot choose but
   can test — statics are public), **A loses** ⇒ **A cancels its pending to B,
   pre-`ss`.**
3. A then runs `complete()` (`ss`) on the forged msg1 → **tail tag fails** →
   silent drop. **A now has neither a pending nor a session for B**, and — per
   the clause — "no give-up, no error." A's `Connecting` future has no pending
   backing it and stops making progress; the app (which saw only
   `IntroError::Internal` on the interception path) is unaware its connect was
   killed.

Reach: on-path or spoofed-into-hint (eager path), **or fully off-path from any
source** if A's application probes parked `Intro`s (A4) — the interception path
also runs the tie-break. Repeatable → a **sustained, unauthenticated
connect-denial against A→B** for every pair where the dialled peer's static is
the smaller. Established connections are **not** affected (no pending ⇒
continuation, not tie-break; its tag-death leaves the connection untouched);
the hole is specific to **pendings**.

**Regression.** Prior NIT 19 explicitly certified "the H2 cancellation cannot
be triggered by a forged claim (dies at `ss`) or by a replayed msg1 (dies at
the guard)" — because the original draft cancelled **on install (post-`ss`)**.
Group 2's ruling moved the cancellation to the tie-break decision (pre-`ss`)
to get determinism, and the reviser applied it faithfully — reintroducing
exactly the vulnerability NIT 19 had cleared.

**Fix.** Move the loser's cancellation to **after `complete()` succeeds**
(post-`ss`), i.e. cancel-and-install atomically in the continuation's admit
step, as the original draft did. Convergence is preserved (re-derivation
below is unchanged for the genuine case: the genuine winner's msg1 passes the
tag, the loser cancels+installs, the winner completes on the loser's msg2). A
forged claim then dies at the tag with the pending **intact**. This is a
one-point ordering change; the tie-break *decision* can still be evaluated
pre-`ss`, but the *cancellation side-effect* must be gated on `complete()`.

*Convergence re-derivation (both orderings), for the record.* A,B dial each
other, `static_A<static_B`. **A wins**: A drops inbound msg1_B (1 DH), keeps
pending; **B loses**: B runs continuation on msg1_A, `complete()` succeeds,
installs responder session S={msg1_A,msg2_B}, resolves its `Connecting` at
`Install{initial:true}`. B's msg2 reaches A → A completes → A installs the same
S. One session, both hold it. Reverse ordering symmetric. Convergence is
sound; only the *cancellation timing* is wrong.

### NEW-2 — MAJOR — re-arm-on-failed-completion is a rate-unbounded reflection/CPU storm

**Where.** §5 *Initiator pendings*, lines 508–512: a failed completion
"immediately re-arms the retransmit rather than waiting out the interval."

**The hole.** The mitigation for finding 11 removes the ~5 s spacing floor on
retransmits. An **on-path** attacker observing each fresh `sender_index` (in
cleartext, v1 §3) and forging a mac1-valid, index-matching, invalid msg2 drives
the loop: forged msg2 → attempt spent → **immediate** fresh msg1 (new
ephemerals, new index) → attacker observes the new index → forges again → …
There is **no minimum spacing** between re-armed retransmits, so A emits msg1s
at the attacker's injection rate. Effects: (i) **CPU DoS on A** — a fresh
ephemeral keypair + index draw per msg1; (ii) **reflected flood at B** — A
uploads 196 B msg1 per attacker 107 B msg2 (~1.8× amplification, aimed at the
dialled peer). Worse, the mitigation does not even defeat the denial: an
on-path attacker still wins the msg2 race each interval, so the handshake
still fails *and* now storms. The "second msg2 in the same interval is
dropped" guard (505) is void once the re-arm starts a new interval
immediately.

**Fix.** Re-arm, but keep a floor: allow at most **one** immediate re-arm per
`RETRANSMIT_BASE` interval (a forged msg2 buys one extra retransmit, not an
unbounded train), or re-arm at `now + min_spacing`. Preserves the finding-11
intent ("one round trip, not the whole interval") without the storm.

### NEW-3 — MAJOR — consumed chains defeat `INTRO_MAX_PER_SOURCE` and disable evict-oldest (shared-budget interpretation 1)

**Where.** §4 lines 239–249 (per-source cap + evict-oldest apply to
**unconsumed** only), 260–269 (`read_identity()` frees the dedup slot),
287–295 (Accounting: "the consumed population is application-driven, never
attacker-driven").

**The hole.** `read_identity()` frees the source's dedup slot, and consumed
chains are exempt from **both** the per-source cap **and** evict-oldest. A
policy-implementing app (A4) probes each surfaced `Intro`. So a **single
source IP** can: send 4 msg1s (per-source cap ok) → app probes → 4 consumed
chains (now exempt) → the freed dedup slots accept 4 more → probe → … A single
IP thereby occupies **far more than 4** budget slots — up to the whole
`INTRO_QUEUE_CAP` — defeating the per-source cap that finding 2 relied on, and
because consumed chains are eviction-exempt, **evict-oldest can no longer
guarantee a genuine peer a slot** (nothing unconsumed left to evict → reverts
to drop-incoming for genuine arrivals) for up to `INTRO_TTL`. The accounting's
"never attacker-driven" is wrong: the DH cost and the drop decision are the
app's, but the *occupancy* is attacker-**induced**, and the two advertised
mitigations (per-source cap, evict-oldest) are both neutralised on the consumed
tier.

**Bounding facts (why MAJOR not BLOCKER).** Each consumed chain costs the app
1 DH and **TTL-expires in 15 s** (interp 6, §4 288–290), and a well-behaved app
that drops garbage immediately after the probe keeps the consumed population
transient. The exposure bites only an app whose policy decision is
slow (UI/dir-lookup) — precisely finding 3's original scenario, now surviving
as an *occupancy* residual rather than a *clobber*.

**Fix.** State the residual honestly and pick one: a **separate budget** for
consumed chains (so unconsumed caps genuinely bound the attacker tier), or
extend a (looser) per-source cap to consumed chains, or make evict-oldest fall
back to evicting the oldest consumed chain when no unconsumed entry exists.
At minimum, delete/qualify "never attacker-driven."

### NEW-4 — MINOR — attacker-inducible provider-handle exhaustion

**Where.** §7 ceilings, line 925 ("up to 1024 concurrent provider handles …
an operationally scarce resource the TTL bounds in time").

Same mechanism as NEW-3: attacker floods → app probes → each consumed
chain/carried mid-state holds the endpoint's static provider handle. For a
hardware/enclave static this drives up to 1024 concurrent enclave handles,
attacker-**induced**, bounded only by the 15 s TTL. The table names the ceiling
but frames it as app-scarcity, not as attacker-reachable. Note it as
attacker-inducible. (Sub-case of NEW-3.)

### NEW-5 — NIT — the simultaneous-open loser signals `Connecting=Ok` before the winner confirms

The loser resolves its `Connecting` at its *local* `Install{initial:true}`
(§5 431–438), i.e. on writing msg2, before the winner has it. A persistent
msg2 drop then gives the loser connect-success-immediately-followed-by-15 s
liveness teardown, while the winner sees `TimedOut`. This is finding 12's
one-way-dark exposure with an added asymmetry (the loser gets a false Ok).
Within the accepted responder-installs-on-write model; worth one sentence.

---

## 4. Interpretation rulings

| # | Interpretation | Ruling | Note |
|---|---|---|---|
| 1 | one shared `INTRO_QUEUE_CAP` budget | **SECURITY-RISK** | Consumed tier defeats per-source cap + evict-oldest → NEW-3. Needs-maintainer: split the budget or cap the consumed tier. |
| 2 | cancellation at tie-break (pre-`ss`), loser resolves at `Install` | **SECURITY-RISK (BLOCKER)** | The *resolve-at-Install* half is fine; the *cancel-pre-`ss`* half is NEW-1. Needs-maintainer: cancel post-`ss`. |
| 3 | pre-existing guard entry reverts on drop | **SECURITY-SAFE** | Reachable only for orphans (established/pending statics take the continuation/interception path, never staged authenticate). The revert only re-opens replay of a *never-accepted* initiation → surfaces as a fresh `Intro`, exactly the acknowledged eviction exposure. Pinned/established entries are never touched. |
| 4 | ≈68 pps sustaining rate | **SECURITY-SAFE** | Arithmetic correct (1024/15 s = 68.3). Caveat: it is the refresh rate for a **distributed/spoofing** attacker (≥256 IPs / /64s, or forged sources) — a single IP is held to 4 *unconsumed*; NEW-3 is the single-IP consumed-tier bypass, a distinct path. |
| 5 | `shed_mask` slides with the window | **SECURITY-SAFE** | Grounded in `admit()` (session.rs:73–104): the bitmap shifts on `counter>greatest` and resets on `shift≥REPLAY_WINDOW`; a mask sliding identically loses a bit exactly when the window loses it. Replay protection is the window's job, unaffected by the mask; no premature drop re-opens a replay window, and no window exists where a counter is ACK-buildable but its mask bit is gone. |
| 6 | TTL applies to consumed chains (Expired stays reachable) | **SECURITY-SAFE** | Positively bounds the NEW-3/NEW-4 occupancy to 15 s and keeps `AuthError::Expired`/`AcceptError::Expired` reachable. Cost: a slow (>15 s) app loses DH-paid work — a liveness/usability cost, not a security one. |

---

## 5. Verdict

**SOUND-WITH-FIXES.** The architecture remains sound and every one of the 4
prior blockers (and all 11 prior BLOCKER/MAJOR findings) is genuinely closed:
mark-always+`shed_mask` restores the replay/roaming invariant; the queue-DoS
mitigations land; own-bytes-on-consume kills the 0-DH clobber; the tie-break
converges in both orderings; the DoS table re-derives at a clean 2-DH ceiling.
Ratification is blocked, though, on the rewrite's own new holes:

1. **NEW-1 (BLOCKER)** — the tie-break cancels a genuine pending on an
   unauthenticated claim (pre-`ss`), an off-path connect-denial and a direct
   regression of NIT 19. Fix: cancel post-`ss`. **Must fix before ratification.**
2. **NEW-2 (MAJOR)** — re-arm-on-failure is a rate-unbounded reflection/CPU
   storm. Fix: one re-arm per interval / minimum spacing.
3. **NEW-3 (MAJOR)** — consumed chains defeat the per-source cap and disable
   evict-oldest (shared-budget interp 1). Fix: separate/cap the consumed tier;
   drop "never attacker-driven."

NEW-4 (MINOR) and NEW-5 (NIT) are honesty-clause completions. Interpretations
3, 5, 6 are security-safe; 1, 2, 4 carry the risks above.

# Final verification — SPEC-v2-DRAFT-v3.md (round-3 fixes F1–F5)

Final gate. Scope: confirm F1–F5 closed their holes, introduced no new ones,
and don't conflict. Not a from-scratch re-review. Line numbers are 1-based
into `SPEC-v2-DRAFT-v3.md`.

**Verdict: NEEDS-ANOTHER-FIX-PASS** (light — one clarifying paragraph on F5's
re-home selection; no BLOCKER; F1–F4 are solidly closed).

---

## 1. F1–F5 closure table

| Fix | Sev | Status | Basis |
|---|---|---|---|
| F1 tie-break post-`ss` | BLOCKER | **CLOSED** | §5 485–508 + continuation 530–549 |
| F2 no immediate re-arm | MAJOR | **CLOSED** | §5 586–594, 623–633 |
| F3 per-source cap both tiers | MAJOR | **CLOSED** | §4 222, 250–260, 344–354 |
| F4 masked-set ACK | MAJOR | **CLOSED** | §8 1134–1148, §9 1218–1221, §13 1462–1465 |
| F5 accept re-home | MAJOR | **PARTIAL** | §4 364–395 closes N2, but re-home selection under-specified (NEW-A) |

### F1 — CLOSED
> "**The tie-break runs only on an authenticated inbound — after `ss`
> succeeds.** … a match detected at `es` selects the tie-break path but
> decides nothing … an `ss` failure is a silent drop with **the pending
> untouched** — a forgery dies at `ss` and cannot cancel a pending." (485–490)

- Forged msg1 claiming a dialled peer: dies at `ss`, pending untouched
  (490, 532–533). NEW-1 (the pre-`ss` cancel) is reversed. ✓
- Continuation renumbered **tag(1)→guard(2)→pacing(3)→tie-break(4)→admit(5)**
  (530–549), internally consistent: steps 1–3 "leave any pending untouched"
  (532, 535), tie-break is step 4 on the authenticated result, admit is step 5
  (record-on-full-admission-only). ✓
- Convergence holds both orderings: winner authenticates the real inbound
  (2 DH), drops it, completes its own outbound; loser cancels post-`ss`, writes
  msg2. One session {winner msg1, loser msg2}, both hold it. `static_A<static_B`
  and its mirror both traced clean. ✓
- Cost: winner now pays 2 DH (was 1 pre-`ss`) — the honest simultaneous-open
  cost, priced in the existing forged-known-static DoS row (615), no new row,
  under the 2-DH ceiling. ✓
- "before the second DH" / "acts on the claimed static" gone from the
  normative body (survive only in the round-2 historical index 1495–1498 and
  the round-3 "deleted" note 1637 — see NIT-C).

### F2 — CLOSED
> "A failed completion spends the attempt … and the **next scheduled
> retransmit** … refreshes it: at most one fresh initiation per retransmit
> interval, no matter how many forged msg2s arrive." (591–594)

Storm vector gone; this is v1's spent-attempt / next-scheduled-retransmit.
DoS accounting keeps the on-path/index-observing documentation and drops the
"one round trip" claim (623–633). No "immediately re-arms" in the body. ✓

### F3 — CLOSED
> "`INTRO_MAX_PER_SOURCE` … counting the **sum of unconsumed stage-0 entries
> and consumed chains** … `read_identity()` consuming an entry is net-zero …
> so the cap bounds a source to 4 chains total regardless of how the
> application probes." (250–260)

Single-IP-probing-app attack re-derived against v3: 4 msg1 → 4 unconsumed
(at cap) → app probes → 4 consumed (net-zero, still 4) → 5th msg1 **dropped**
("if the source's whole allowance is held by consumed chains, the arrival is
dropped", 256–257). One IP bounded to 4. ✓ Evict-oldest still serves genuine
peers on the unconsumed tier. Residual honestly stated (344–354): a
distributed attacker (256 IPs) + a probing app can still fill the budget with
consumed chains, and F3 chose *not* to add the consumed-tier eviction fallback,
so an all-consumed queue drops genuine arrivals for ≤ `INTRO_TTL` — an
acknowledged, TTL-bounded posture, not a new hole.

### F4 — CLOSED
> "**`largest` is the greatest marked-and-not-shed counter** — not the greatest
> window counter, which may itself be shed … `AckFrame::from_window` (or its
> equivalent) takes the masked snapshot, never the raw window. A shed counter
> therefore never appears as the ACK `largest` nor inside any range." (1137–1142)

N1 closed: every ACK field derives from `window & !shed_mask`; a shed counter
cannot be `largest` or fall in a range. Restated in §9 (1218–1221) and §2
(57). §13 obligation present: "shed the window's greatest counter … assert the
emitted ACK's `largest` is the greatest unshed marked counter, the shed counter
absent from every range" (1462–1465). ✓ NIT-D below (degenerate all-shed
window) is safe regardless.

### F5 — PARTIAL
Closes N2: a slow (human-in-the-loop) decision now establishes via re-home on
the freshest parked initiation instead of writing a msg2 the initiator drops.
`AcceptError::Stale` defined (207–215, 1259–1266), fast path stated (375–378),
`AcceptError::Expired` removed from the normative taxonomy, cost note (635–640)
and [MAINTAINER] known-behaviour clause (1306–1321) present, §7 names the
re-home admission site (923–926). **But the re-home selection rule is
under-specified — NEW-A (MAJOR) below.**

---

## 2. Cross-fix interactions

| Pair | Finding |
|---|---|
| **F1 × F3** | **Clean.** The post-`ss` tie-break runs inside the internal continuation on an *eager/interception* packet that "never touches the accept queue" (426); the per-source cap governs *parked* entries, a disjoint population. An intercepted parked entry is consumed net-zero (F3). No double-count, no conflict. |
| **F5 × F3** | **MINOR adverse interaction.** If an app holds a source's whole 4-cap as consumed chains (e.g. a slow-decision app that probes every surfaced `Intro` from the same peer), F3 **drops** the peer's fresh retransmits, so F5's re-home finds nothing to select → `Stale`. Re-home's premise "a fresh one is essentially always available" fails under app-induced cap exhaustion. Mitigated: a sensible app deciding on one peer does **not** probe subsequent `Intro`s (they stay unconsumed and *are* the re-home candidate); app-driven, signalled, recoverable. Worth one app-guidance sentence. |
| **F5 × F1** | **Defended, with a NIT.** Re-home does **not** apply the tie-break — correctly, because `read_identity()`'s interception (436–441) consumes any *known* static (incl. pending outbound remotes) into the continuation *before* a `Proven` forms, so a `Proven` exists only for a non-known static (no outbound pending) → the tie-break never applies at re-home. **NIT:** `connect()` checks only for a live `Connection`/in-flight outbound connect (77–79), **not** an in-flight `Proven`. Dialling a static *after* a `Proven` for it exists is allowed, opening a narrow window where re-home writes msg2 without the tie-break while an outbound pending forms → a possible convergence race (mutual-dark-then-retry, 15 s). Very narrow, app-ordered, non-compromise. |
| **F4 × F1** | **Clean.** Disjoint: F4 is §8/§9 (connection-core receive/ACK), F1 is §5 (endpoint-core handshake). No shared clause. ✓ |
| **F5 AcceptError vs §10** | **Clean in the normative taxonomy** (see §4 below) — but one dangling `AcceptError::Expired` in the round-2 index (NIT-B). |

---

## 3. New findings on the changed clauses

### NEW-A — MAJOR — F5 re-home selection metric is under-specified, and the guard-rejection branch is unhandled (the primed replay concern)
**Where.** §4 379–388 (re-home), with §4 dedup 239/286–289 and Appendix A 1355.

Two coupled gaps in the ~208 new lines:

1. **Selection metric.** The F5 ruling says re-home selects the freshest parked
   initiation *"by timestamp"*; the v3 body **dropped "by timestamp"** and left
   a bare *"the freshest currently-parked initiation"* (379) with *"next-freshest
   tried"* (384). By-timestamp is **unimplementable pre-`ss`**: the initiation
   timestamp *"only ever emerges"* at `complete()` (Appendix A 1355), i.e. after
   `es`+`ss`. So selection can only be by **park (arrival) time**, which the text
   never states. Park-time = timestamp for *genuine* monotonic retransmits, but
   diverges under replay.
2. **Guard-rejection branch.** Re-home says *"the guard admits its strictly-greater
   timestamp"* (386) with a discard-and-retry path only for *same-static mismatch*
   and *tail-tag failure* (383–385) — **no branch for a candidate that passes
   `es`+`ss`+same-static but FAILS the guard.**

**The attack (task's primed case).** Stage-0 parking does **no** timestamp/guard
check (0 DH), and dedup is replace-with-newest by source **with no timestamp
compare** (239). A replay+source-spoof-capable attacker replays a *genuine*
captured msg1 (right static, **stale** timestamp) from the peer's source; it
overwrites the freshest genuine parked entry (dedup) and becomes the freshest by
park time. Re-home selects it → `es`+`ss` **verify** (it is genuine) → same
static **matches** → the guard **rejects** (timestamp not strictly greater than
the value `authenticate()` already recorded). The text has no rule for this:
undefined between "stall to `Stale`" and a hard accept failure.

**Severity rationale.** The security **property is intact** — re-home's
`es`+`ss`+same-static+tail-tag correctly kills a *forged* initiation (dies at
`ss`), and the guard's monotonic advance means no replayed session ever
establishes; worst case is a **transient, signalled `Stale`** recovered by the
peer's next genuine retransmit (which, being strictly-greater, re-homes cleanly).
So **not a BLOCKER.** But it is a genuine MAJOR under-specification in a fix
clause: the ruling↔body contradiction on "by timestamp" is concrete and
unimplementable as literally ruled, and an implementer must guess the
guard-rejection behaviour. **Fix:** one paragraph — (a) selection metric =
latest park time; (b) a candidate that fails `es`+`ss`, same-static, **or the
guard** is discarded and the next-freshest tried; (c) `Stale` if none passes.

### NEW-B — NIT — F5 re-home rationale contradicts the dedup rule
"`INTRO_TTL` keeps **~3 recent initiations parked**, a fresh one is essentially
always available" (387) and "the **next-freshest** tried" (384) assume multiple
parked initiations per peer. But dedup is replace-with-newest per source and
"surfacings are bounded to **one per source**" (239, 289) — so there is exactly
**one** unconsumed parked entry per source at any instant (the latest
retransmit), not three. Re-home has one candidate, not a choice. Doesn't break
re-home (the one candidate is the latest genuine retransmit — exactly what's
wanted) but the "~3 parked / next-freshest" framing is factually inconsistent and
should be reworded. (It also means "next-freshest tried" in NEW-A almost always
has no next → falls straight to `Stale`.)

### NEW-C — NIT — no §13 test obligation for the re-home rule
F1/F2/F3/F4 each gained an Appendix-B obligation; **F5 did not** — Appendix B
(1445–1470) has no re-home / freshest-selection / `Stale` / fast-path test.
Add one (fast path uses own initiation; re-home selects the latest parked and
verifies same-static + guard; `Stale` when nothing valid is parked; a stale
replay is discarded not established).

### NEW-D — NIT (safe) — F4 all-shed-window degenerate case unstated
If every in-window counter is shed, the masked set is empty and `largest` =
"greatest marked-and-not-shed" is undefined. The mechanism largely precludes it
(a shed packet schedules no ACK, 1123), and any empty/degenerate ACK is **safe**
(it acknowledges nothing, which is the whole point of F4). N1's suggested
"no ACK this round" sentence would close it, but F4's *ruling* didn't require it,
so F4 is closed on its own terms.

---

## 4. Error-taxonomy final closure

- **§10 is complete and single-homed.** Every live variant appears once with a
  firing condition: `IntroError::{Expired,Internal,Malformed,EndpointDropped}`,
  `AuthError::{Replay,HandshakeFailed,Expired,EndpointDropped}`,
  `AcceptError::{Stale,AlreadyConnected,EndpointDropped}`,
  `ConnectError::{AlreadyConnected,TimedOut}`,
  `ConnectionLost::{TimedOut,RekeyFailed,NonceExhausted,LocallyClosed,EndpointDropped}`,
  `SendError::{PayloadTooLarge,ConnectionLost(ConnectionLost)}` (1248–1275).
  (Minor, pre-existing: `SendError`'s two variants are listed without an
  inline §10 gloss — `PayloadTooLarge`'s condition lives in §9 1197; accepted by
  the round-2 Group-7 closure, not introduced by F1–F5.)
- **`Superseded`** appears only in removal statements (212, 300–307, 1244) — no
  live variant. ✓
- **`AcceptError::Expired`** is gone from the normative taxonomy (removal
  statements at 213, 1265). **NIT-B (dangling reference):** the *round-2
  changes-applied* index still enumerates `AcceptError::{Expired, AlreadyConnected,
  EndpointDropped}` as "the closed normative taxonomy" (**line 1548**), and the
  same index still describes the *pre-`ss`* tie-break cancellation (1495–1498) and
  the *re-arm* ruling (1569). These are in the non-normative historical index; the
  round-3 index declares itself "authoritative … where a fix supersedes a round-2
  ruling" (1623–1627), so they are reconciled *by structure* — but a reader of the
  round-2 index alone gets superseded enum/behaviour text. Scrub or mark the three
  superseded round-2 entries.

---

## 5. Verdict — NEEDS-ANOTHER-FIX-PASS (light)

F1–F4 are **solidly CLOSED** and mutually consistent; the four holes the
security/fidelity re-reviews raised (pre-`ss` cancel, re-arm storm, consumed-tier
cap bypass, shed-`largest` permanent-loss) are gone, and no cross-fix conflict
was found among them. **F5 is PARTIAL:** it closes N2 and correctly defends
re-home against forged initiations, but ships a MAJOR under-specification
(NEW-A) — the "freshest" selection metric contradicts the ruling and is
unimplementable as ruled ("by timestamp" pre-`ss`), and the guard-rejection
branch (the replayed-genuine-stale-timestamp case the task flagged) is unhandled.
No BLOCKER: the security property holds and the failure is a transient, signalled
`Stale`. One clarifying paragraph on re-home (selection = latest park time;
guard-rejected candidate discarded → `Stale`) closes NEW-A; fold in NEW-B/-C/-D
and the NIT-B index scrub and this is RATIFY-READY. The maintainer may instead
accept NEW-A as scope of the *already-[MAINTAINER]-flagged* re-home clause and
ratify on sign-off — but the ruling↔body "by timestamp" contradiction should be
resolved in text either way.

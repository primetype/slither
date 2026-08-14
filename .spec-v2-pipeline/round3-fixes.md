# Round-3 fixes — SPEC-v2-DRAFT-v2.md → v3

Five fixes from the two re-reviews (`rereview-security.md`, `rereview-fidelity.md`).
Both confirmed: architecture sound, all prior findings closed, all 9 groups
applied, all 6 interpretations faithful. These five repair new gaps the rewrite
(or my round-2 rulings) introduced. Apply to produce `SPEC-v2-DRAFT-v3.md`.

---

## F1 — Tie-break must be gated post-`ss` (BLOCKER, rereview-security)

**Hole.** The Group-2 tie-break acts on the *claimed* (unproven) static, pre-`ss`.
The claimed static in IK msg1 is forgeable by anyone knowing the responder's
public static (encrypt an arbitrary `s` under the `es`-derived key; only `ss`
binds possession). So an off-path attacker forges a msg1 claiming victim A's
dialled peer B; if `static_B < static_A`, A cancels its genuine pending, then
`complete()` fails at `ss` — A is left with no pending and no error. Sustained
connect-denial.

**Fix.** Run the tie-break **only on an authenticated inbound** — after `ss`
succeeds. Sequence in the eager path / continuation when the claimed static
matches an in-flight outbound pending: `es` (claimed) → **`ss` (authenticate)**
→ if `ss` fails, silent drop, **pending untouched** → if `ss` succeeds, apply
the lexicographic-static tie-break: our static smaller ⇒ drop the (now
authenticated) inbound, keep our pending; peer's static smaller ⇒ cancel our
pending, write msg2. A forgery dies at `ss` before the tie-break and cannot
cancel a pending. Convergence in the genuine case is unchanged (both sides
authenticate a real peer, then reach complementary tie-break verdicts). Cost:
2 DH (`es`+`ss`) on the inbound before deciding — the honest simultaneous-open
cost, and for a forgery exactly the existing "forged claim of a known static:
2 DH, dies at the tail tag" DoS row (no new row). Update the numbered
continuation order (tag → guard → pacing, then tie-break on the authenticated
result) and delete any remaining "acts on the claimed static" phrasing. This
reverts to the original draft's post-`ss` cancellation; supersedes the
round-2 Group-2 "decided at the claimed-static match, before the second DH"
sentence.

## F2 — Drop the immediate re-arm on failed completion (MAJOR, rereview-security)

**Hole.** The Group-8 "re-arm the retransmit immediately on a failed completion"
mitigation removed the retransmit spacing floor: an on-path attacker turns each
forged msg2 into an immediate fresh msg1 — an unbounded CPU/reflection storm.

**Fix.** Remove the immediate-re-arm rule entirely. Keep v1's behaviour: a failed
completion spends the attempt; the **next scheduled retransmit** (at
`RETRANSMIT_BASE` + jitter) refreshes it — so at most one fresh initiation per
~5 s regardless of how many forged msg2 arrive. Keep the finding-11
*documentation* (the exposure is on-path/index-observing; the full mitigation is
foreclosed by hiss's consuming state machines) but drop the "reduces leverage to
one RTT" claim and the mechanism behind it. Supersedes the round-2 Group-8
re-arm ruling.

## F3 — Per-source cap counts consumed + unconsumed chains (MAJOR, rereview-security)

**Hole.** Reviser interpretation 1 exempted consumed chains from
`INTRO_MAX_PER_SOURCE` and evict-oldest. A single IP can then induce a probing
app to fill the shared `INTRO_QUEUE_CAP` budget with consumed chains, defeating
the per-source cap and disabling evict-oldest for genuine peers.

**Fix.** `INTRO_MAX_PER_SOURCE = 4` counts the **sum of unconsumed stage-0
entries and consumed chains** for a source (per /64 on IPv6). `read_identity()`
consuming an unconsumed entry is net-zero for that source's count (−1 unconsumed,
+1 consumed), so the cap bounds total chains per source to 4 regardless of
probing. Evict-oldest continues to operate on the **unconsumed** tier only
(consumed chains are DH-paid and app-held; they are freed by the app dropping the
handle or by TTL). Correct the honesty clause: a single source is bounded to 4
chains total; a distributed attacker still needs one IP per 4 chains, and the
app's probe policy governs how many it authenticates. Supersedes reviser
interpretation 1 ("consumed chains exempt").

## F4 — `shed_mask` ACK derivation when the greatest counter is shed (MAJOR, rereview-fidelity N1)

**Hole.** The Group-1 `shed_mask` carve-out never says how to recompute the ACK's
`largest`/`first_range` when the window's greatest counter is itself shed. The
natural implementation (mask the bitmap but reuse `AckFrame::from_window`
unchanged) reports the shed greatest counter as acknowledged — reintroducing the
exact permanent-loss bug Group 1 exists to prevent, reachable in the steady state
of sustained backpressure.

**Fix.** State normatively: ACK-range construction operates on the **effective
acknowledged set = window bitmap AND NOT `shed_mask`**. Every ACK field derives
from that masked set — `largest` is the **greatest marked-and-not-shed counter**
(not the greatest window counter), and `first_range`/subsequent ranges are
computed over the masked bitmap. A shed counter therefore never appears as the
ACK `largest` nor inside any range. Add this as an explicit sentence in §9's ACK
amendment and in §8's shed rule; note that `AckFrame::from_window` (or its
equivalent) takes the masked snapshot, not the raw window. Reachable-in-steady-
state, so it gets a §13 test obligation (shed the greatest counter, assert it is
absent from the emitted ACK).

## F5 — `accept()` re-homes to the freshest parked initiation (MAJOR, rereview-fidelity N2) **[MAINTAINER]**

**Hole.** `INTRO_TTL = 15 s` is not the real bound: fresh-ephemeral retransmit
means the initiator re-mints its index every ~5 s (verified `endpoint.rs:993-
1023`) and ignores a msg2 answering a superseded initiation. So a `Proven` chain
goes stale in ~5 s: an `accept()` on it writes a msg2 the initiator drops,
leaving a half-open responder session reaped at 15 s liveness — the connection
never establishes. This silently breaks slow (human-in-the-loop / UI-prompt)
accept decisions, a motivating use case. Novel to slither's staged accept
(WireGuard auto-responds, so never has a slow-decision path).

**Fix (ruled, [MAINTAINER] to confirm).** `accept()` establishes on the
**freshest parked initiation for the proven static**, not on the specific
initiation the app inspected:

- The staged chain (`Intro`→`Claimed`→`Proven`) proves *identity*; `accept()`
  commits to the *peer*.
- On `accept()`, the endpoint selects the freshest currently-parked initiation
  from the proven static (by timestamp), runs its `es`+`ss` (verify same static,
  guard admits its greater timestamp), and writes msg2 for **that** initiation.
  Because the peer retransmits every ~5 s and `INTRO_TTL` = 15 s keeps ~3 recent
  initiations parked, a fresh one is essentially always available while the peer
  is still trying (its full 90 s give-up window). Prompt decisions hit the fast
  path (the `Proven`'s own initiation is still freshest — no re-home, no extra
  DH).
- If **no** initiation for that static is currently parked (the peer stopped
  retrying or all aged out), `accept()` returns **`AcceptError::Stale`**; the app
  SHOULD re-accept when the peer's next initiation surfaces as a new `Intro`.

Add `Stale` to `AcceptError` (§4/§10). Cost note: a re-homed accept pays the
inspect `es`+`ss` plus the freshest initiation's `es`+`ss`+`ee`+`se`; this is
app-driven, post-authentication, and pacing-gated — not attacker amplification.
Document this in §5's cost notes and as a [MAINTAINER] known-behaviour clause in
§11, with the richer "accept-commitment" model (auto-complete the *next* arriving
initiation, fully decoupling arbitrarily slow decisions) named as deferred.

Supersedes reviser interpretation 6 ("TTL applies to consumed chains so Expired
stays reachable"): `AcceptError::Expired` is replaced by `AcceptError::Stale`
with the re-home semantics; a consumed chain's usefulness is bounded by the
*peer's* retrying, not by a fixed consumed-chain TTL. Keep `INTRO_TTL = 15 s` on
the unconsumed tier (the DoS defense).

---

## Reviser interpretation dispositions (from both re-reviews)

Security-safe / faithful: 3 (pre-existing guard reverts on drop), 4 (68 pps —
correct arithmetic; label it the distributed/spoofing sustaining rate), 5
(shed_mask slides with the window — keep, and F4 completes its ACK story).
Overturned by a fix above: 1 (F3), 2 (F1 changes when cancellation happens; the
loser still resolves its `Connecting` at the winner's msg2/`Install`, so the
"loser resolves at Install" half stands — only the *timing* of the cancel moves
to post-`ss`), 6 (F5).

## For the reviser

Apply F1–F5 to `SPEC-v2-DRAFT-v2.md` → `SPEC-v2-DRAFT-v3.md`. Keep all DRAFT
markers, the frozen-wire invariant, house style, and the existing 13 [MAINTAINER]
flags; F5 adds one more (accept re-home). Update the DoS/cost notes for F1 and F5,
the §13 test obligations for F4, and the "Round-2 changes applied" index → add a
"Round-3 changes applied" section keyed to F1–F5. Return a ≤10-line summary:
sections touched per fix, new [MAINTAINER] count, and any place F1–F5 forced a
consequential edit elsewhere (e.g. an error-enum or DoS-row change) so the final
verification can confirm it.

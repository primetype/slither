# Pass 3a security-fix notes — SPEC-v2-DRAFT.md (draft v5)

Scope: rulings 35 (SECV5-1) and 37 (SECV5-4), plus SECV5-5 and SECV5-6.
Wire-free pass. Rollback copy:
`.spec-v2-clean-slate/SPEC-DRAFT-v5-post-pass2-frozen.md`.

Pass 3b owns: §5.7, §6.8, §7.3, §7.4, §7.5, §10.3, §13.3, §16.10,
§17.4's basis prose, Appendix A, the timers/constants tables, ruling 36
(contested-connection PING), `PERSISTENT_KEEPALIVE`.

---

## Ruling 35 — PENDING branch runs the tie-break comparison

### 35a. §6.4 PENDING bullet — REWRITTEN (landed)

Old text asserted "The tie-break does **not** run here: … this path has
already committed to ours as responder." Replaced with a two-outcome
bullet applying §6.7's comparison over the same ordered pair of statics:

- peer's static smaller (we are the tie-break loser) → cancel the pending,
  its `Connecting` resolves `Err(ConnectError::AlreadyConnected)`, install
  as responder (the branch as previously written);
- our static smaller (we are the tie-break winner) → return
  `AcceptError::Stale`, leave the pending in place, **record** the
  candidate's timestamp in the guard exactly as §6.7's winner-side record
  does (§17.1), noting the write is a key-holder write (post-`ss`).

Added a "the comparison is **not** optional on this path" paragraph
stating the two-sided divergence concretely (our own msg1 still reaches
the peer, which reaches the same comparison via §6.6, loses, and installs
as responder over our msg1 → two sessions, two key sets, both msg2s
dropped, mutually dark until `DEAD_TIMEOUT`; and both ends compute the
same stream-ID parity, contradicting §6.7). Recorded that under the
`read_identity()` → `connect()` → `accept()` ordering on both sides the
divergence is certain, not a coin flip.

Preserved explicitly in the closing paragraph: §16.1's
one-connection-per-static invariant holds on **both** outcomes — the
loser side installs the responder session in place of the pending it
cancelled, the winner side installs **no second connection** — and that
ordering against one static still yields exactly one connection.

### 35b. §6.4 "Stale" bullet — AMENDED (landed)

Third `Stale` trigger added: "the static is PENDING and we are the
tie-break winner". Added that on this case there is nothing to re-accept,
because our own outbound is completing that connection and its
`Connecting` resolves in the ordinary way (the generic "SHOULD re-accept
on the next `Intro`" advice would otherwise be wrong here).

### 35c. Ripple edits for ruling 35 (landed)

- **§5.4 PENDING row** — was "never reaches the tie-break — §6.4's PENDING
  branch cancels the pending and installs as responder". Now: reaches the
  **same comparison** by a different route; loser cancels + installs as
  responder, winner returns `Stale`, keeps its pending, records the
  timestamp. Adds "the comparison is two-sided, so no admission path opts
  out of it".
- **§6.6 step 2** — the "converse does **not** hold" justification was
  wrong (it said the PENDING branch installs "without any tie-break").
  Rewritten: the PENDING branch is a **different route to the same
  comparison, not an exemption**; identical pair of statics, identical
  conclusion; the routes differ only in what carries the mid-state and
  who observes the refusal, and can never disagree.
- **§16.1** — the invariant paragraph now states both outcomes and says
  explicitly that the winner side installs **no second connection**;
  `read_identity()` → `connect()` → `accept()` on one static still yields
  exactly one connection.
- **§18.1 `AcceptError::Stale`** — gains the tie-break-winner-with-pending
  case (pending left in place, timestamp recorded, own outbound
  completes, nothing to re-accept). The "no `AlreadyConnected`" sentence
  is restated for the two outcomes. **No new variant.**
- **§18.1 `ConnectError::AlreadyConnected`** — qualified: it is the
  resolution of a `Connecting` cancelled by a racing `accept()` **only
  when we are the tie-break loser**; as winner the `Connecting` is
  untouched and the `accept()` reports `Stale`.
- **§6.9** — the replayed-genuine-msg1 row's "confined to the
  in-flight-dial window" now adds "and, within it, to the half of the peer
  population whose static sorts below ours". A new paragraph before "The
  ceiling, explicitly" records the primitive as back inside §6.7's
  key-ordering bound on *every* route, plus the secondary gain: the
  PENDING branch previously cancelled **unconditionally**, which made the
  primitive key-order-independent on that route and roughly doubled its
  reach across a population.

---

## Ruling 37 — bound the captured-initiation replay

### 37a. §6.7 honesty clause — boundedness claim qualified (landed)

"each captured initiation is single-use" replaced with a **conditional**
bound plus two non-droppable qualifications:

1. *Per initiation, not per capture* — a captured `HANDSHAKE_GIVEUP`-long
   retransmit train is a set of ≈ 18 distinct initiations (one per ~5 s,
   §5.5), each separately single-use, spent in timestamp order.
2. *Only while the entry survives §17.1* — orphan aging (`INTRO_TTL`
   scale) and LRU eviction recycle the entry and re-arm the replay;
   explicit cross-reference to §17.1's new pin extension as what stops
   the bound evaporating on an attacker-controlled schedule.

Declined mitigation recorded inline in §6.7 so it is not re-proposed:
"refuse to cancel a pending on a vacuous guard pass" — breaks genuine
first contact (both guards empty on true first-contact simultaneous open
⇒ both keep their pending, both install as initiator, mutually dark).

### 37b. §17.1 pinning extension (landed)

New bullet after the existing pin bullet: an entry written by §6.6 step
4's admit or by a winner-side record (§6.7, including §6.4's PENDING
branch) is exempt from orphan aging **and** LRU eviction for
`HANDSHAKE_GIVEUP` (90 s) after the connection it belongs to dies; on the
winner side that is the connection our own outbound completed, and if
that outbound never completed the 90 s runs from its own
`HANDSHAKE_GIVEUP` expiry. Then it demotes to an ordinary orphan.

Rationale stated as ruled: covers an application's reconnect backoff;
retains a per-static timestamp (≈ 45 B), not a session; the 1024
`TS_GUARD_ORPHAN_CAP` still bounds the tier it eventually feeds;
mitigation (i) still prevents minting entries by authenticate-then-drop
(each costs a genuine admission or a genuine authenticated winner-side
drop, both key-gated).

**Judgment call:** the ruling said "after the connection it created
dies", which is under-defined for a winner-side record (that record
creates no connection). Resolved by naming both anchors explicitly —
the installed connection, or on the winner side the connection our own
outbound completed — plus a fallback anchor (the pending's own
`HANDSHAKE_GIVEUP` expiry) for the case where the outbound never
completed, so the exemption always has a defined start.

**No new `[RATIFIED` marker** was added for this bullet (count must stay
29); it sits inside §17.1 under the existing section markers.

---

## SECV5-5 — §17.1's honesty clause for a dialled-only static (landed)

§17.1's honesty clause split into two explicit cases:

- *a static we accepted* — the pinned entry does the work as described;
- *a static we only ever dialled* — **no entry exists**, so there is
  nothing to pin and nothing to evict, the guard offers this case nothing
  and every candidate passes it vacuously however old; the whole
  protection is the basis, which is `None` and refuses everything.
  Honest consequence recorded: the spurious-`Intro` primitive is
  **indefinite and repeatable** against such a peer (mitigation (i)
  returns the guard to empty after each refusal), not a one-shot eviction
  artefact; cost per replay is one stage-0 slot plus application-chosen
  DH (§6.9); this is the majority case for a client.

§6.4's "What basis and guard bar, stated honestly" bullet gains the
matching distinction: the guard's admitted set is **empty for every peer
we only ever dial**, so it bars nothing there, and the `None` basis is
what carries the protection the guard cannot.

---

## SECV5-6 — guard-record vs basis-check ordering (landed)

**Judgment call — which of the two offered fixes.** The review offered
"basis check before guard record" *or* "record reverts on `Stale`", and
said pick one. Picked **revert on `Stale`**: the other option would have
required moving the fast path's record out of `authenticate()`, i.e.
restructuring §17.1's write-site list, whereas the revert is exactly the
mechanism §17.1's mitigation (i) already specifies for a dropped chain,
so it generalises rather than adds machinery.

Landed as a new §6.4 bullet, "**Ordering — the guard's record against the
basis check**": the record lands first on both paths (at `authenticate()`
fast path, at candidate admission on the re-home walk), the basis rule
runs after, and a record made for a candidate whose `accept()` returns
`AcceptError::Stale` is **reverted** to the pre-call contents. Noted that
an implementation may equivalently defer the record until the basis check
passes — observable state identical, only observable state normative.

**Interaction caught and carved out explicitly:** ruling 35's
tie-break-**winner** PENDING branch returns `Stale` *and must keep* its
record. A blanket revert would have silently undone ruling 35. The clause
names it as the single deliberate exception ("No other `Stale` leaves a
record behind"), and the same carve-out is restated in §17.1's write-site
paragraph and in mitigation (i).

Mirrored in §17.1: mitigation (i) now says the revert applies "whether
the rejection is a dropped chain or an `accept()` that returns
`AcceptError::Stale`", with the winner-side record excepted.

---

## Cross-reference tightening (landed)

- §6.7's winner-side bullet now states that §6.4's PENDING branch reaches
  the same winner-side outcome by the staged route, the only visible
  difference being that the refusal surfaces as `AcceptError::Stale`
  rather than a silent drop (the application holds the chain there).
- §17.1's write-site paragraph now names both routes to the winner-side
  record, and states the revert-on-`Stale` rule with its exception.

---

## Appendix B — test obligations added (landed)

- **PENDING branch** obligation rewritten for ruling 35: both static
  orderings pinned separately (loser: cancel + `AlreadyConnected` +
  install as responder; winner: `Stale` + pending still in flight and
  completing + timestamp present in the guard), **plus** the convergence
  regression this ruling exists for — two endpoints both driven
  `read_identity()` → `connect()` → `accept()` at each other so both take
  the PENDING branch must converge on one shared session with agreeing
  stream-ID parity, run for both orderings, asserting data flows both
  ways rather than merely that a connection object exists. Plus §16.1 on
  both outcomes.
- **§5.4 local-state routing** obligation: PENDING now reads "the
  tie-break comparison by **either** route".
- **The post-mortem pin** (ruling 37): entry survives orphan aging and a
  full ~1024-static LRU flush for `HANDSHAKE_GIVEUP` after its connection
  dies, then demotes; paired replay test must pin **both** sides of the
  90 s horizon (dies at the guard inside, documented re-admission
  outside), because the bound is conditional.
- **No-record-on-`Stale`** (SECV5-6): guard byte-identical after a
  basis-refused accept; tie-break-winner `Stale` the single exception.
- **The dialled-only static** (SECV5-5): no guard entry exists, one
  captured packet surfaces as an `Intro` **more than once**, live
  connection untouched every time via the `None` basis.

---

## Preamble revision log (landed)

New "Security-fix revisions (draft v5 → v6, 2026/08/14)" block after the
v4 → v5 block, headed "Pass 3a — rulings 35 and 37, plus two directed
fixes". Deliberately scoped as **3a** so pass 3b can append its own
paragraph without conflict. The existing `PERSISTENT_KEEPALIVE`
coincidence paragraph (pass 3b's SECV5-7) was left untouched.

---

## Verification

| Check | Result |
|---|---|
| `[MAINTAINER]` markers | **2** (unchanged; ~line 156 preamble + Appendix A.1) |
| `[RATIFIED` markers | **29** (unchanged; no new marker added) |
| Code fences (```` ``` ````) | 48 — even, balanced |
| `IK_MSG1_LEN` / `IK_MSG2_LEN` | 174 / 81 — intact |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` | 196 / 107 — intact |
| `MAX_DATAGRAM` / `MAX_PLAINTEXT` | 1200 / 1170 — intact |
| `VERSION` / `PROLOGUE` | `0x01` / `b"slither\x01"` — intact |
| `DEAD_TIMEOUT` / `KEEPALIVE_TIMEOUT` | 25 s / 10 s — intact |
| New error variants | none |
| Timers/constants tables | untouched |

**Sections touched:** preamble, §5.4, §6.4, §6.6, §6.7, §6.9, §16.1,
§17.1, §18.1, Appendix B. **Verified untouched:** §5.7, §6.8, §7.3, §7.4,
§7.5, §10.3, §13.3, §16.10, §17.4, Appendix A, the timers and constants
tables (diff hunk map checked against the frozen v5 copy).

## NEEDS A RULING

None. Both judgment calls (§17.1's pin anchor for a winner-side record;
which of SECV5-6's two offered orderings to adopt) were resolvable inside
the ruling text and are recorded above.

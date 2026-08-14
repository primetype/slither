# Pass 3c — rulings 39 and 40 (draft v6 → v7)

Rollback copy: `.spec-v2-clean-slate/SPEC-DRAFT-v6-post-pass3b-frozen.md`

## Baseline (before edits)

- `SPEC-v2-DRAFT.md` = 4122 lines
- `[MAINTAINER` = 2, `[RATIFIED` = 29
- "15 s" occurrences = 7 (lines 94, 825, 834, 875, 923, 3414, 4118) — all intentional, untouched
- `DEAD_TIMEOUT` 25 s, `KEEPALIVE_TIMEOUT` 10 s — unchanged

## Sites inventory

| Line (pre-edit) | Site | Ruling |
|---|---|---|
| 111–117 | preamble v4→v5 "coincidence flagged" note | 40 (forward pointer) |
| 175–188 | preamble ruling 38 entry | 40 (forward pointer, not rewritten) |
| 202–204 | preamble "with pass 3b the round closes" → pass 3c block appended after | 39, 40 |
| 710 | §5.7 timers table row | 40 (default 25 → 10 s; ceiling) |
| 725–740 | §5.7 "coincidence / zero margin" passage | 40 (rewrite), 39 (scope) |
| 1629 | §7.5 constants table row | 40 |
| 1636–1650 | §7.5 persistent-keepalive bullet | 40 |
| 1652–1676 | §7.5 ruling 38 derivation block | 40 (re-framed, derivation retained) |
| 1677–1701 | §7.5 liveness bullet | 39 (scoping block appended) |
| 3141–3142 | §16.2 handle-API sentence | 40 |
| ~4010 | Appendix B liveness-anchor obligation | 40 |
| ~4020 | Appendix B clock-armed-at-install obligation | 39 (consequence), new beacon obligations |
| ~4116 | consolidated constants table | 40 |

## Edit log

### Ruling 39 — idle-from-install drop stands; the dance's scope is stated

1. **§5.7, new paragraph** ("What the dance covers, and what it does not"),
   placed after the `DEAD_TIMEOUT` = 2 × `KEEPALIVE_TIMEOUT` + 5 s paragraph:
   the dance is automatic for any connection that has ever carried traffic
   (one exchange puts `R > S` on the receiver; the loop then self-sustains);
   the only uncovered case is never-carried-anything since install, reaped at
   install + `DEAD_TIMEOUT`; forward pointer to §7.5 for consequence +
   declined alternative.
2. **§7.5, new indented block under the Liveness bullet** — "Ruling 39 — the
   dance's scope, and what the idle-from-install drop costs":
   - scope statement (automatic once traffic has flowed, sparse
     request/response works with no opt-in);
   - the one uncovered case, tied to §7.4's install pin (**not edited** —
     ruling 39 confirms it as written);
   - **the cost**: only a side that can still *reach* the other can restart;
     a NAT-bound peer must be the dialler or hold the binding with the
     beacon;
   - **declined alternative**: "make all keepalive opt-in" — silently breaks
     sparse-traffic applications that do not opt in.
3. **Appendix B**, new obligation: "The dance is automatic once traffic has
   flowed" — one one-way exchange, then assert both sides live past
   install + `DEAD_TIMEOUT` with no `set_persistent_keepalive` call.

### Ruling 40 — the bound becomes a ceiling (reverses part of ruling 38)

1. **Validation rule flipped everywhere** — rejects at or above
   `DEAD_TIMEOUT`:
   - §7.5 bullet (inside its existing `[RATIFIED 2026/08/14, amended
     2026/08/14]` marker — already carried "amended", no new marker);
   - §16.2 handle-API sentence;
   - Appendix B liveness-anchor obligation (`PERSISTENT_KEEPALIVE <
     DEAD_TIMEOUT` → `≥ DEAD_TIMEOUT`, plus "accepts 10 s, rejects 25 s");
   - consolidated constants table ("handle-rejected below … the liveness
     floor" → "handle-rejected at or above … a ceiling, not a floor").
2. **Default 25 s → 10 s** in §5.7's timers table, §7.5's constants table,
   and the consolidated table. Derivation stated as `KEEPALIVE_TIMEOUT`,
   one-lost-beacon tolerance inside 25 s (2 × 10 + 5).
3. **Why the beacon reaches where the passive rule cannot** — new §7.5
   paragraph: the passive rule is conditional on `R > S` and can sustain any
   link that entered the loop but can never start one; the beacon is
   unconditional, so it sustains a mutually idle link (never entered the
   dance) and holds a NAT binding open. Also states how the two compose (the
   beacon lands, the peer's passive rule answers, the answer resets the
   beaconing side's `R`) and the resulting 10 s ping-pong with one-loss
   tolerance. Reflected in §5.7.
4. **Beacon stays in the marking set** — new §7.5 paragraph: arming enables
   death and never defers it, so a beacon fired into a void still dies at
   `R + DEAD_TIMEOUT`; the previously *declined* alternative (exclude
   beacons from the marking set) is rewritten as **unnecessary rather than
   declined**.
5. **Ruling 38's `I`/`S`/`R` derivation RETAINED** verbatim in §7.5 and
   re-framed: the arithmetic is sound and proves the narrow fact "an
   interval ≥ `DEAD_TIMEOUT` is inert"; ruling 40 reads that as proof the
   **bound** was inverted. Added the ceiling's counterpart: the beacon is
   useful iff `S + I < R + DEAD_TIMEOUT`, unreachable under a floor
   (`S ≥ R` ⇒ `S + I ≥ R + DEAD_TIMEOUT` for every admissible `I`),
   reachable under a ceiling and outright true at `S = R` (install).
   Ruling 38's alternatives: re-basing on `KEEPALIVE_TIMEOUT` →
   **superseded**; deleting the knob → **still declined**.
6. **§5.7's "sits exactly on the liveness floor / zero margin" passage
   rewritten, not deleted** — it now narrates the coincidence, ruling 38's
   floor reading, and ruling 40's reversal (sentence right, rule wrong).
7. **Appendix B**, two new obligations: the beacon sustains a mutually idle
   link on the paused clock (one side opted in; one lost beacon survivable,
   two consecutive fatal); and the interval is bounded above, not below
   (accepts 1 s / 10 s, rejects 25 s / 30 s), naming the old floor as the
   regression to catch.

### Preamble

- **v4 → v5 coincidence note** — gained a second forward pointer to ruling 40
  alongside the existing ruling-38 pointer. Original text untouched.
- **Ruling 38's entry** — gained a parenthetical: partly superseded by ruling
  40; stands as written; its derivation is sound but its *diagnosis* was
  inverted (the floor **sentence** it "corrected" was the right half; the
  floor **rule** was the wrong half). Body not rewritten.
- **v5 → v6 block header** — "two passes" → "three passes", with the one
  non-wire exception named (`PERSISTENT_KEEPALIVE`'s default + validation).
- **"With pass 3b the round closes"** → re-worded to "with pass 3b every
  ruling … is applied", and the closing claim moved to the end of pass 3c.
- **New Pass 3c block** covering rulings 39 and 40, stating plainly that
  ruling 40 reverses part of ruling 38 and that ruling 38's diagnosis was
  itself corrected.

## Judgment calls

1. **§7.5 bullet's `[RATIFIED 2026/08/14, amended 2026/08/14]` marker left
   as-is.** It already carried "amended", so re-amending would not change the
   text; marker counts therefore held at 2 / 29 exactly.
2. **Line 41's historical "a persistent-keepalive floor"** (draft v1 → v2
   revision list, item F) left untouched. It is an accurate record of what
   draft v2 did, sits inside a revision block whose other items are likewise
   superseded without per-item annotation, and annotating it would fragment
   the history.
3. **"Mutually idle" disambiguated.** A link that carried traffic and then
   went quiet *is* sustained by the dance indefinitely (the ping-pong
   self-sustains). So "mutually idle" is qualified throughout as "never
   entered the dance because neither side ever had traffic" — otherwise the
   beacon's stated purpose reads as overlapping the dance's.
4. **The compose mechanism spelled out.** The brief said the beacon "sustains
   a mutually idle link"; strictly, the beacon alone does not reset the
   beaconing side's own `R` (own sends never do). What sustains it is the
   peer's *passive* answer to the beacon. That is derived, not new policy,
   and is stated explicitly so an implementer does not conclude a one-sided
   beacon is self-sufficient without the peer's reply.
5. **One-loss arithmetic checked, not assumed.** Steady state at `I` = 10 s
   is an immediate ping-pong (the peer's `KEEPALIVE_TIMEOUT` has always
   already elapsed when the beacon lands), so a single lost beacon lands the
   next answer at 20 s — 5 s inside 25 s — and two consecutive losses are
   fatal. Text and the Appendix B obligation both state this, replacing the
   looser "one interval plus one keepalive delay".
6. **`ConnectionLost::TimedOut` attributed to "the timing-out side"** rather
   than both sides, since the idle-from-install case includes the responder
   half-open shape where only one side holds a connection.

## NEEDS A RULING

None. Both rulings were applicable as stated; no ambiguity required guessing.

## Final verification

| Check | Result |
|---|---|
| `[MAINTAINER]` markers | 2 (unchanged) |
| `[RATIFIED` markers | 29 (unchanged) |
| "15 s" occurrences | 7, same lines/context — untouched, incl. §6.9's `1024 / 15 s` |
| `DEAD_TIMEOUT` | 25 s (§5.7 and §7.5 tables both unchanged) |
| `KEEPALIVE_TIMEOUT` | 10 s (both tables unchanged) |
| Wire identifier counts vs frozen v6 | identical (`IK_MSG1_LEN`, `IK_MSG2_LEN`, `INIT_PACKET_LEN`, `RESP_PACKET_LEN`, `MAX_DATAGRAM`, `MAX_PLAINTEXT`, `VERSION`, `PROLOGUE`) |
| Wire values | 174 / 81 / 196 / 107 / 1200 / 1170 / `0x01` / `b"slither\x01"` all present and unmoved |
| Code fences | 48 (even, balanced) |
| Diff hunks | 12, all inside preamble, §5.7, §7.5, §16.2, Appendix B, consolidated table — no §3/§4/§6/§17/Appendix A hunk |
| Untouched as instructed | §6.4, §6.6, §6.7, §6.8, §17.1, §17.4, §18.1, Appendix A, §7.4's install paragraph |
| Line count | 4122 → 4335 |


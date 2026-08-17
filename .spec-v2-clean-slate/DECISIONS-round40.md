# Round 40 — maintainer's decisions (2026-08-17, interactive session)

Decided one by one against PLAN-round40.md. Ruling numbers are provisional
(the maintainer assigns >= 249 when they land in rulings.md). Every decision
below took the plan's recommended option unless noted — none deviated.

## R40.6 — shared-driver spin (LIVE, HIGH; gates C2)
- (a) **(ii) DISARM Pto** while §7.3's budget admits no probe; re-arm when the
  budget opens or a receive re-anchors. Fallback to (i) floor at
  now+K_GRANULARITY only if the re-arm plumbing exceeds a few lines.
- (b) **Promote** driver.rs:1028 debug_assert! → **assert!** (audit C3).
- (c) **Amend §13.3/§13.4**: 139(a)'s "increment keeps the deadline advancing"
  gains its saturation scope; "cap is an overflow guard" gains the
  livelock-guard cross-ref. Amendment-table row.

## B1 — implement ruling 215
- (a) **Drop the 212(c) pre-pass**; probe built first (rank 2), pump continues
  on the same pass. 3 tests encoding 212(c) rewritten by the blind author.
- (b) **Coalesce**: probe = PING + owed PATH_CHALLENGE/PATH_RESPONSE when the
  coalesced size fits the room; bare PING otherwise.
- (c) **Restate §7.3's arithmetic at pump time** (room after the arming pass),
  addressing 215's own reasoning (rule 4(b)). Amendment row.

## F1 — rekey observation
- (a) **Add the Appendix B §7.7 obligation** (seals e+1 past epoch_size; e−2
  refused; e−1 opens; byte-exact across boundary). Amendment row.
- (b) **Both triggers**: fast with_epoch_size knob test + production-constant
  (65 536) story test — into the --release run if slow in debug.
- (c) **Behavioural** observation (no #[cfg(test)] epoch accessor).

## C1 — lost msg2 / accept obligation
- (a)+(b) **Design confirmed; document the obligation** in §6.2, §6.4,
  Endpoint::accept rustdoc, lib.rs sketch: keep calling accept() after a
  success; a lost-msg2 peer re-appears as a fresh Intro; admitting replaces
  the unconfirmed session (first handle sees Replaced).
- (c) **"Unconfirmed" accessor = separate follow-on API-SHAPE ruling** — does
  not block (a)(b)(d).
- (d) **Fix the fixture**: Pair::establish keeps accepting (integrator's file,
  rule 15); dial story gains "under loss"; 1-in-12 at 10% loss must go 0/12.

## R2 — reassembly cost
- (b)(i) **Small-to-large merge** adopted (measured: 1006×→3.18×, ≤
  0.95·credit·log2 credit), + (iii)'s §10.6 exposure note regardless. The
  ruling addresses 94's and 213(c)'s reasoning (rule 4(b)).
- Memory trade: **tighten capacity accounting to keep §10.6's ceiling at
  ~credit** (shrink-at-quiescence or capped growth) — do NOT amend the bound
  to 1.49×credit.
- (c) Per-turn driver fairness: **deferred, recorded** in the ruling as an
  open §16 design question.

## C2 — loss envelope
- **(b)(i) PTO_BACKOFF_CAP 2⁶ → 2³** (the measured 8/8-at-0.5 configuration;
  zero honest-path cost observed). Ratified constant change: spec,
  spec_constants.rs pin, amendment row.
- (c) **Pin E5a (ladder shape) + E5b (30% completion) as story tests, and
  state the survival curve in §13.3.**
- Ships in the SAME slice as R40.6, with R40.6's fix landing FIRST (both
  levers red on tests_livelock without it).

## Slice order
1. **Slice R40-A: R40.6 + C2** (impl recovery.rs+driver.rs+constant; blind
   tests tests_livelock.rs + story pins; spec §13.3/§13.4).
2. Slice R40-B: B1 (connection/mod.rs vs tests_contested.rs).
3. Slice R40-C: F1 (tests only + Appendix B).
4. Slice R40-D: C1 (docs + fixture; accessor ruling follows).
5. Slice R40-E: R2 (recv.rs insert vs bound test; capacity kept ~credit).

Process per CLAUDE.md: rulings + this record land in .spec-v2-clean-slate/ at
a named commit BEFORE dispatch (rule 14); disjoint paths per slice (rule 6);
full gate table per slice.

## Post-sweep decisions (2026-08-17, after the rule-4 sweep + extraction pass)

Three points the sweep surfaced beyond the six items; decided one by one,
all as recommended:

- **250 / cwnd gate**: the coalesced probe packet **keeps the probe's
  §14.5 exemption**; §14.5's exemption list gains the clause (bounded:
  ≤18 B of path frames per probe).
- **251 / REKEY(0³²) vector**: **test-only cryptoxide ChaCha20-Poly1305
  pin in slither** (golden-wire philosophy; maintainer's deliberate
  reading of the raw-primitive rule — production crypto rules untouched).
  §7.7's "pinned by test" sentence amended to name the home.
- **252 / story vehicle**: **new story S34** ("a responder that keeps
  accepting survives a lost msg2"), not an S1 edit — S1's 4-DH cost pin
  would go stale. STORIES.md banner 33→34; ruling 209 is the precedent.

Note (rule 11, recorded): the item-5 line "spec_constants.rs pin" above is
correct for the pin (tests/spec_constants.rs:584); the constant's
*declaration* is src/constants.rs:384 — two files, both real.

## Landing corrections (2026-08-17, from the pre-commit verification pass)

Six verifiers (Opus x3, Sonnet x3) checked the drafted amendments against
their artefacts before commit; the fixes are in the landed text. Three
corrections that touch the decisions above:

- **R40.6's slice plan said "blind tests tests_livelock.rs"** —
  `src/core/connection/tests_livelock.rs` already exists (slice 7b's F1
  keepalive-gate pins, `keepalive_can_leave()`), so the blind author's
  new core-level file is **`tests_pto_gate.rs`**, and the keepalive
  scope-probe is unnecessary: the code already gates keepalive
  announcements on the budget. Residual (deferred, round 41): the spec is
  silent about that shipped keepalive gate.
- **R2's "measured: 1006×→3.18×"** is the prototype's controlled pair;
  the audit's sustained number is ~916× (205 MB / 224 KB). Both real;
  ruling 253 records both.
- **C2's "E5a/E5b pins as story tests"** land as Appendix B obligations
  discharged by slice R40-A's story tests (STORIES.md gains no E-named
  entries; S-numbering is the story namespace).

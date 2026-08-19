# Round 43 material — the completeness audit and the gap slice (2026/08/18–19)

**Trigger:** the maintainer's question *"besides performance, are we
feature complete?"* — answered by a four-reader audit, then a gap slice.

**Audit verdict (2026/08/18):** feature complete against the ratified
scope — zero code stubs, every promised mechanism implemented, the
deferral list deliberate and stated (§19). Not acceptance-complete:
7 of 34 stories carried untested clauses reducing to four pieces of
test work, plus registry hygiene.

**Gap slice (integrated `f703283`):** three blind authors + one
measurer, worktrees from `b072afd`, disjoint paths, 29 separating
mutants. Closed: S8 (shell expiry, both Intro and the story's literal
Claimed), S10 (1024-intro flood, 0 DH, warm stream unharmed, eviction
observed; the 784 B mid-state bound two-sided), S22 clause 4 (wrong
static vs a live responder — the dropped slice-1→2 hand-forward), S24's
15 s timer (armed, fires with no traffic to carry it), S25/S30's traced
clauses (G7 closed by `testutil::capture`, no dependency added).

**Rulings out of it (all four maintainer decisions on the recommended
option, 2026/08/19):** 272 (§6.3/§17.5 stage-0 figure → measured
≈ 484 B / ≈ 496 KB; author A's find), 273 (§8.2 scoped to a live
connection; measurer D drove the answer before the ruling), 274
(O-citations by bold title + provenance; the O-ids were dangling
pointers into a never-committed file), 275 (the bundle: owed registry
discharged, S22 anchors, WARN ratified, SECV5-5/6/8 pins).

**Evidence:** `round43-A-intro-author.md`, `round43-B-wrong-static-author.md`,
`round43-C-traced-author.md`, `round43-D-hygiene-measurer.md`, plus the
SECV5 author's report (round43-E, landed with its commit).

**Carried onward:**
- ChainState boxing (~96 KB at a saturated cap) — available as a
  deliberate optimization slice; declined for the hygiene pass (272).
- U7 stays open (endpoint-core transmits carry `conn: None` on the io
  trace — §16.3's MUST bites only on connection-core transmits today;
  author C verified it open at the record, `round43-C` C5).
- §18.2's third `frames` payload is prose, not a structured field
  (author C's C3) — a structured-field ruling is available if a
  log-consumer ever needs to filter on it.
- The perf bucket (round42-material): the in-situ syscall excess and
  lever (ii), unchanged by this round.

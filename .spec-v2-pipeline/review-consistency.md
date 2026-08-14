# Consistency / fidelity / testability review — SPEC-v2-DRAFT.md

Reviewer lens: internal consistency, fidelity to `design-choices.md` (rulings A–K),
consistency with `SPEC.md` v1, testability, completeness, house style.
Line numbers refer to `SPEC-v2-DRAFT.md` as reviewed (913 lines, 2026-08-13 18:02).

Fidelity baseline first: **every ruling A–K is present and none is contradicted.**
All constants match the decision document exactly (`INITIATIONS_PER_SECOND` 50,
`INTRO_QUEUE_CAP` 1024, `INTRO_TTL` 90 s, `TS_GUARD_ORPHAN_CAP` 1024 (~45 B/~50 KB),
`L` 250 ms, `RECV_BUFFER` 256, ~220 B/~225 KB, 60 s backstop budget, ~5.3 s
supersession, 128/63/268 B/511-range ≈ 2 KB derivation, 131 072 counters,
300–450 lines). All **8 [MAINTAINER] flags** are present at the correct clauses:
§1 no-fallback (A), §3 one-connection-per-static (B1), §4 names-final (J1),
§4 dedup-key-supersedes-TODO (E), §6 two-cores (C1), §7 eviction consequence (D),
§8 swap cut (G2), §11 peer-restart known limit (round-2 item 1). The DoS table
(B4), the C3 core surfaces, and the J3 error table are transcribed row-for-row.
The findings below are the residue.

---

## Findings

### BLOCKER

**1. `IntroError::Superseded` is unreachable as typed — the supersession rule
contradicts the error signatures.**
§4, lines 172–177 (error definitions) vs lines 210–215 (supersession rule) vs
lines 153/158/164 (verb signatures).
The supersession rule says: at stage 0 replacement is *transparent* (the `Intro`
handle acts on the newest bytes), and when a replacement arrives while a
mid-state exists, "the next verb on the old chain returns `Superseded`". But a
mid-state exists only *after* `read_identity()`, so the "next verb" is
`authenticate()` (returns `AuthError`) or `accept()` (returns `AcceptError`) —
and `Superseded` is defined **only** in `IntroError`, whose sole producing verb,
`read_identity()`, can never observe supersession precisely because stage-0
replacement is transparent. As written, the variant can never be returned by any
verb, and the sentence "the next verb … returns `Superseded`" cannot be
implemented without violating a signature. The draft already solved the identical
problem for expiry ("`IntroError::Expired` (`AuthError::Expired` at that stage)",
lines 206–209) but not for supersession.
**Fix**: add `Superseded` to `AuthError` (and to `AcceptError`, see finding 3)
with the same cross-type parenthetical used for `Expired`; or make stage-0
replacement non-transparent so `read_identity()` is the surfacing point (this
would contradict E's replace-with-newest intent — the first fix is the faithful
one). State each variant's firing verb explicitly.

### MAJOR

**2. `Timeout(None)` is never defined, and the lateness-bound quote names a
different type.**
§6, lines 449 and 476 (`Timeout(Option<Instant>)` in both enums) vs line 520
("the core exposes exact deadlines (`Timeout(Instant)`)").
The task of the terminal variant is load-bearing (drain sentinel + deadline
announcement), yet the meaning of `None` is nowhere stated — presumably "drained,
no deadline armed", but a reader could also take it as "deadline unchanged".
Untestable as written, and the §6 block-quote's `Timeout(Instant)` contradicts
both enum declarations. (The mismatch is inherited from design-choices C3-vs-F,
but the draft must resolve it, not copy it.)
**Fix**: one sentence at the enums: "`Timeout(None)` = drained and no deadline is
armed; `Timeout(Some(d))` = drained, next deadline `d`; the lateness bound applies
to `Some(d)`" — and amend the quote to match. Confirm the semantics are identical
for both cores (the draft implies but never says so).

**3. `AcceptError` is named but never defined, and §10 actively excludes it.**
§4 line 164, §6 lines 436–437 (signatures) vs §10 lines 776–777 ("The staged
errors are §4's `IntroError` and `AuthError`").
`Proven::accept()` and `core::Endpoint::accept()` both return
`Result<_, AcceptError>`; the expiry rule ("a parked entry's staged descendants
share its validity", line 209) means `accept()` can at least fail with an
expired/superseded attempt — yet no `AcceptError` variant is enumerated anywhere,
and the taxonomy section's sentence denies the type exists. Violates the review
rule that every error variant named anywhere appears in the taxonomy exactly once.
**Fix**: enumerate `AcceptError` in §4 (plausibly `{Expired, Superseded}` after
finding 1) and correct §10's sentence to name all three staged error types.

**4. The receive-backpressure shed rule is packet-granular but the buffer is
message-granular — "has room" is ambiguous.**
§8, lines 677–693.
The shed decision runs per inbound *packet* ("an inbound packet is handled as …
only if the buffer has room: replay mark, frame processing, …") while
`RECV_BUFFER` counts *messages*, and one packet may carry several DATA frames.
If a packet carries 3 frames and 1 slot is free, marking-then-partially-delivering
would ACK DATA the application never received — exactly the permanent-loss bug the
rule exists to exclude. The rule's own invariant forces whole-packet granularity,
but the text never says it.
**Fix**: "room" means room for **every DATA frame aboard the packet**; otherwise
the whole packet is shed (never marked, never ACKed). One sentence.

**5. Round-2 item 4 is silently asserted as resolved.**
§8, lines 635–638: "At the hiss surface the refusal **is** a generic decryption
failure, indistinguishable from any other failed open…".
design-choices lists exactly this question as an undecided round-2 research task
("read `datagram.rs`'s error paths"). The draft states the unresearched answer as
fact. The conservative half ("no dedicated trace is promised") is safe; the
factual claim ("is … indistinguishable / not possible") is an assertion the
design explicitly deferred.
**Fix**: reword to the promise-only form: "no dedicated trace for the refusal is
promised; whether hiss's error granularity even permits one is a round-2 question"
— or complete the research and cite it.

**6. v1 §9.7 is amended but missing from the §2 unchanged-by-reference table.**
§2 table, lines 43–55, vs §9's header, line 695 ("amends v1 §9.4, §9.5, **§9.7**")
and lines 752–753 (restating §9.7's empty-send rule).
The table stops at §9.6. Under the table's own contract ("everything not named is
untouched" — scoped to listed sections), §9.7's carried status is undefined, yet
§9 claims to amend it and §9.7's text contains event language (`Incoming`) that
§10 replaces. v1 §8 and §10 (the two out-of-scope sections) are also absent from
the table with no stated disposition.
**Fix**: add a `§9.7` row ("behavioural deltas — in full; the `Incoming` language
is replaced by §10; restated by §9") and a one-line row or note disposing of v1
§8/§10 (superseded by draft §11, or carried).

### MINOR

**7. v1 §9.2's "measured on tokio's clock" is carried "in full" but contradicts
the sans-io no-clock rule.**
§2 row for §9–§9.3 (line 52) vs §6 lines 492–494 ("the cores never call
`Instant::now()` or a wall clock"). v1 §9.2 defines `ack_delay_µs` as "measured
on tokio's clock"; in v2 it is necessarily computed from the shell-supplied `now`.
**Fix**: add a carve-out to the §9–§9.3 row: the ack_delay clock is the `now`
argument (shell conformance under `L`), not tokio's.

**8. "The guard and the gate are checked together" — order and record-interaction
unspecified; the tag-failure clause is not covered by "either failure".**
§5, lines 294–297.
(a) If pacing rejects, is the (greater) timestamp still recorded? Check-and-record
semantics differ observably between orderings (a later replay of the paced-out
msg1 is rejected in one reading, admitted in the other). Not security-critical,
but two conformant implementations would diverge — untestable as written.
(b) "either failure is a silent drop with a trace, the established connection
untouched" grammatically covers guard and gate only; design B3 lists
"guard/pacing/**tag** failures" as silent-drop-with-trace-connection-untouched.
The tail-tag death ("dies here") should be inside the same clause.
**Fix**: state the order (suggest: tag → guard → pacing; record only on full
admission) and extend the silent-drop/untouched clause to all three.

**9. The pacing mechanism is underspecified.**
§5, lines 305–313. "50 (one accepted replacement per 20 ms, per established
peer)" — token bucket, sliding window, or minimum spacing? Different mechanisms
pass different burst patterns; a conformance test cannot be written.
**Fix**: pick one (the parenthetical suggests minimum 20 ms spacing between
*accepted* replacements per peer; the kernel reference uses a per-peer
rate-limiter) and say so in one sentence.

**10. The orphan LRU's "use" is undefined.**
§7, lines 585–589. LRU eviction needs a definition of recency: does a failed
guard *check* touch the entry, or only an admission (record)? Attacker-visible
difference: whether replayed (rejected) msg1s can keep an orphan warm.
**Fix**: define the touch event (suggest: only admission — post-`ss` record —
refreshes recency, keeping the write path key-holder-only, consistent with §7's
"only key-holders write entries").

**11. `ConnectError::{…}` ellipsis; `LocallyClosed`/`EndpointDropped` unglossed.**
§10, lines 772–777. The taxonomy section leaves `ConnectError` an open set ("…")
and gives firing conditions for three of five `ConnectionLost` variants but not
`LocallyClosed` or `EndpointDropped`. The section's whole purpose is closure.
**Fix**: close the `ConnectError` set (`{AlreadyConnected, TimedOut}` for phase 1,
or mark it explicitly open with rationale); gloss the two variants (`LocallyClosed`
= `close()`/last-handle drop observed by surviving clones; `EndpointDropped` =
driver stopped, §3).

**12. §2's row for v1 §6 omits two of §8's amendments.**
§2 line 50 names window/timer-evaluation/roaming carve-outs but not the §8
swap-cut statement (a stated divergence from WireGuard's keypair retention,
amending §6's rekey bullet) or the epoch-death-via-liveness statement (augmenting
§6's `MAX_EPOCH_JUMP` bullet). Both are consistent with v1's text but they are
amendments the row should name, per the draft's own layering contract.
**Fix**: extend the row's status cell.

**13. Placeholder constants presented as settled (round-2 items 5 and 6).**
§4 lines 185–186 (`INTRO_QUEUE_CAP`), §7 line 589 (`TS_GUARD_ORPHAN_CAP`),
§8 line 679 (`RECV_BUFFER`). design-choices says 256 is "a placeholder; validate
against the `acks_keep_flow_over_a_lossy_wire` throughput profile" and the two
1024 caps are "defensible defaults, not measurements; sanity-check … under a
spoofed-src mac1-valid flood". The draft states all three as bare settled
constants — a silent status upgrade of two open round-2 items.
**Fix**: one draft-note per table ("default pending the phase-1 validation task")
— removable at ratification.

**14. `Intro` accessor staleness under transparent replacement.**
§4 lines 151–152 (`sender_index()`) vs lines 210–212 (stage-0 replacement is
transparent, the handle "acts on the newest bytes"). After a replacement, the
retransmit carries a **new** index (v1 §5 Initiator 2), so `sender_index()`'s
value is ambiguous: the surfaced entry's index or the newest bytes'? Also
unstated: whether a replacement re-emits `IntroReady` for an already-surfaced,
unconsumed entry (implied no).
**Fix**: state that accessors reflect the newest bytes at call time (matching
"acts on the newest bytes"), and that replacement of a surfaced entry emits no
second `IntroReady`.

**15. §4's "nothing durable may be keyed on the claimed static" vs §7's
pin-on-mid-state.**
§4 lines 140–144 vs §7 lines 582–584. A guard entry is pinned while "a staged
mid-state exists for that static" — but a mid-state's static is *claimed*, not
proven, until `authenticate()`; pinning is therefore durable-ish state keyed on
an unproven claim. It is bounded (a flag flip on a pre-existing, key-holder-
written entry, bounded by the intro-queue cap, reverting on drop) and protective
rather than exploitable, but the two clauses rub. Inherited from design-choices D
verbatim — fix is a clarifying carve-out, not a design change.
**Fix**: either bind the pin at `authenticate()` (proven), or add "(a bounded
exception to §4's rule: the pin only flips a bit on an entry a key-holder already
wrote)".

**16. Appendix B: "all nine `recovery::` tests" — the crate has ten.**
Line 873. `src/recovery.rs` `mod tests` contains **10** test functions
(`has_retransmittable_discriminates_payload_from_control` …
`ack_for_unsent_counter_is_ignored`). The "(9)" was copied from
`research-actor-inventory.md` §4.1, which itself miscounts its own ten-item list.
**Fix**: say "all `recovery::` tests" (count-free) or "ten".

**17. `PERSISTENT_KEEPALIVE` silently reclassified from frozen value to
"recommended interval".**
§8, lines 669–673. v1's table freezes 25 s as a constant; v2's
`set_persistent_keepalive(Option<Duration>)` necessarily makes it a default/
recommendation. The reclassification is correct and wire-invisible, but it is a
status change to a ratified table entry and should be stated as such (it sits in
a sentence claiming "all other timer values … are unchanged").
**Fix**: one clause: "25 s becomes the recommended default now that the interval
is caller-chosen; the change is wire-invisible".

**18. The attempt-take gate order omits v1's preconditions.**
§5, lines 336–339. "the pending's attempt state is *taken* on the first
index-matching msg2" — v1 §5 Initiator 3 requires length-correct **and**
mac1-valid before completion. Left as written, a mac1-invalid msg2 with a guessed
index would spend the attempt (an on-path nuisance v1 does not have).
**Fix**: "the first length-correct, index-matching, **mac1-valid** msg2".

### NIT

**19. The lateness-bound contract is restated verbatim in §6 and §9.**
Lines 518–521 and 703–707. The draft's own banner says restatement can drift.
§9 should quote-by-reference ("the §6 contract applies, verbatim") or one of the
two should be the sole normative home.

**20. Appendix A: "Both are zero-runtime-change (hiss-macros codegen only)".**
Lines 816–818. True of the split read; `DatagramSend::next_counter()` is a
runtime accessor, not codegen. Trivial, and the appendix is non-normative, but
the sentence overclaims what design-choices A claims only for the split read.

**21. DoS table row 2 parse ambiguity.**
Line 357: "application never probes or drops the `Intro`" reads two ways
("never (probes or drops)" vs "never probes, or drops"). Inherited from
design-choices B4. Suggest "application leaves the `Intro` unprobed (or drops
it unprobed)".

**22. "no unbounded logging" (§4, line 143) is not crisply testable** — the
sibling prohibitions (map insertion, rate-limit bucket) are; consider "no
per-claim allocation that outlives the verb" or accept it as review guidance.

**23. `session_id()` is never defined** (§6 line 469 accessor comment; also in
design-choices J2). v1 never defines a "session id" either (the golden test pins
a 64-byte session-id hex). One parenthetical ("the Noise handshake-derived
session identifier the golden vectors pin") would close it.

**24. TODO §3's stage-0 "banned addr" reject has no v2 counterpart.**
§4's stage-0 row (line 134) lists only automatic structural rejections —
consistent with design-choices B2 (which supersedes the TODO table), but a
one-line note that address-level policy is now "the application drops the `Intro`
by `source()`" would close the visible TODO→draft delta. Relatedly, design B6's
"droppable without structural damage if the maintainer prefers a smaller phase 1"
severability note for the pacing gate did not survive into the draft — the
maintainer reading only the draft cannot see the pacing clause is severable.

---

## Rulings on the writer's interpretations

Seven points where the draft goes beyond literal transcription of
design-choices.md:

1. **`AuthError::Expired` firing condition** (expiry surfacing at the
   `authenticate` stage, §4 lines 206–209). The design lists `Expired` in
   `AuthError` (J1) but its E-section text says expired verbs return
   `IntroError::Expired` — type-impossible at `authenticate`. The writer's
   cross-type mapping is the only coherent reading. **Accept.**
2. **`IntroError::Superseded` semantics** (stage-0 transparency + mid-stage
   supersession, §4 lines 210–215). The *semantics* are a sound synthesis of
   E's replace-with-newest and J1's variant gloss; the *typing* is broken
   (finding 1). **Accept-with-rewording** — keep the semantics, add the variant
   to `AuthError`/`AcceptError`.
3. **Guard admission point split** ("at `authenticate()` for the staged path and
   inside the internal continuation for replacements", §7). Harmonises design D
   (admission at `authenticate`) with B3 (the continuation runs the guard); both
   are post-`ss`, so the key-holder-only-writes property is preserved. **Accept.**
4. **The anchor-not-roaming clause** ("an accepted initiation *anchors* a new or
   replacement session at its msg1 source, which is not roaming", §8). A needed
   inference — without it, "handshake packets never roam" and Responder-7
   replacement are in apparent tension. Safe: anchoring requires proof of
   possession plus the guard, so the address is authenticated. **Accept**
   (optionally note the interception-path consequence: a replacement installed
   via `read_identity()` interception anchors at a source outside the hint set,
   which then updates the hint map).
5. **The hiss far-future-refusal trace claim** (§8 lines 635–638). Asserts the
   answer to open round-2 item 4. **Accept-with-rewording** (finding 5): keep the
   conservative "no dedicated trace is promised", drop the factual
   "is indistinguishable / not possible" pending the `datagram.rs` read.
6. **"A failed completion spends the attempt; the next retransmit refreshes it"**
   (§5 lines 338–339). Verified verbatim against `research-actor-inventory.md`
   §2 ("On failure the attempt is simply spent — the next retransmit refreshes")
   — a faithful transcription of pinned v1 behaviour, not an invention.
   **Accept** (with finding 18's precondition tightening).
7. **`PERSISTENT_KEEPALIVE` as "recommended interval"** (§8 lines 669–673).
   Forced by `Option<Duration>`; wire-invisible. **Accept-with-rewording**
   (finding 17): state the reclassification instead of doing it silently.

Net: 4 accept, 3 accept-with-rewording, 0 reject-needs-decision.

---

## The "must not lose" checklist — locations

All 14 items from design-choices' final section are present:

| Item | Draft location |
|---|---|
| msg2 source ignored; anchor at dialled; peer roams in | §5 "Initiator pendings", lines 341–345 |
| Handshake packets never roam | §8 "Roaming observability", lines 658–662 |
| One completion attempt per retransmit interval | §5, lines 336–339 |
| Every retransmit a completely fresh initiation | §5 lines 332–334; §4 lines 196–199 |
| Keepalives in the replay window, bypass frames, `ack_delay = 0`, never reach recovery | §9, lines 749–751 |
| `seal_quiet` set; only fresh sends + keepalive mark `last_send`; opening + quiet control age-exempt | §9, lines 745–748 |
| Rekey give-up silent; initial-connect give-up the only visible failure | §5, lines 345–347 |
| Empty `send` real message vs empty plaintext keepalive | §9, lines 752–753 |
| `MAX_MESSAGE` 1159 at the handle, before any queue | §9, lines 731–732 |
| ACK intersects in-flight, never materialises; above-highest ignored whole | §2 rows for §9–§9.3 and §9.4 (lines 52–53, carried with bold emphasis) |
| Loss beats PTO, exactly one fires; give-up beats same-instant retransmit | §6 "Equal-deadline priorities", lines 507–510 |
| Undelivered messages die with the connection, survive a rekey | §9, lines 754–756 |
| Trace targets (`policy`, `replay`, `frames`, + `roam`) | §10, lines 779–781 |
| Test survivals: golden/DH/size/mac1/frame/window/recovery verbatim; allow-list flows → staged-policy | Appendix B, lines 865–893 |

**Round-2 items** (must appear resolved or explicitly open):

1. Peer-restart seq collision — §11 known limitation **[MAINTAINER]**, lines
   806–813. Present, correctly framed. ✓
2. `__MSG1_TAIL_SIZE` derivation — Appendix A, lines 836–843, with the
   option-(c) contingency and footgun note as instructed. ✓
3. Asymmetric-loss test window — Appendix B, lines 899–904, mechanism-not-number
   as instructed. ✓
4. hiss error granularity — §8 lines 635–638: **silently asserted resolved**
   (finding 5). ✗
5. `RECV_BUFFER` 256 placeholder — §8 line 679: stated as settled, validation
   task dropped (finding 13). ✗ (half)
6. Cap defaults sanity-check — §4/§7: stated as settled, flood-test task dropped
   (finding 13). ✗ (half)

## House style

Constants in tables: ✓ throughout (pacing, queue, guard, `L`, `RECV_BUFFER` all
tabled). Rationale attached to every rule: ✓. DRAFT markers on every section
including both appendices: ✓ (13/13). Wire never restated: ✓ — §8's replay-window
derivation repeats §9.6 *constants* with citations, which is the derivation-chain
style the design mandates, not a layout restatement; the only self-inflicted
restatement is the `L` contract (finding 19). British English, `YYYY/MM/DD`: ✓.

---

## Verdict

**FAITHFUL-WITH-FIXES.**

Every ruling A–K is transcribed without weakening; all constants match; all
8 [MAINTAINER] flags are present at the right clauses; the 14-item must-not-lose
checklist is fully located; the C3 surfaces, B4 DoS table, and J3 taxonomy table
are row-for-row faithful. The one BLOCKER is an internal typing contradiction of
the draft's own making (`Superseded` unreachable), fixable with two variant
additions and a sentence; the five MAJORs are definitional gaps
(`Timeout(None)`, `AcceptError`, shed-rule granularity, the §9.7 table row) plus
one silently-asserted open research item — none contradicts the design's intent.

Severity counts: **1 BLOCKER, 5 MAJOR, 12 MINOR, 6 NIT.**

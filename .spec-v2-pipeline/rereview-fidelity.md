# Re-review: fidelity of SPEC-v2-DRAFT-v2.md against round2-resolutions.md

Reviewer lens: did every round-2 ruling land faithfully, is the draft now
internally consistent and buildable, and did the rewrite (≈570 new/changed
lines) introduce anything new. Cross-checked against the real slither source
(`src/{endpoint,session,recovery,frame,handshake}.rs`), the real hiss source
(`src/noise/{datagram,cipher,handshake,error}.rs`, `hiss-macros/src/codegen.rs`,
`src/curve/p256/mod.rs`), `SPEC.md` v1, `design-choices.md`, and the three
round-2 reviews. All source claims below were independently re-verified by
reading the cited files (not taken on the draft's word).

**Verdict: FAITHFUL-WITH-FIXES** (not yet FAITHFUL-AND-BUILDABLE — two new
MAJOR gaps, detailed below, need a paragraph each before ratification; nothing
found rises to BLOCKER or contradicts the frozen wire).

---

## 1. Groups 1–9 coverage table

| Group | Ruling | Status | Evidence (quoted, with line refs into `SPEC-v2-DRAFT-v2.md`) |
|---|---|---|---|
| 1 — receive backpressure | mark window on every fresh packet (shed or not); add `shed_mask`; subtract at ACK time; "room" = whole-packet | **APPLIED** | "the replay window is marked on every authenticated, fresh packet, shed or delivered" (1013–1014); "`shed_mask`... indexed identically to the window bitmap and sliding with it" (1034–1036); "'Room' is whole-packet: if the buffer cannot hold all the packet's DATA frames, the whole packet is shed" (1028–1030); §2's v1-§6 row drops "in full" and names the carve-outs (53). See §3 below for a residual *implementability* gap this ruling's own text didn't anticipate (finding N1). |
| 2 — simultaneous open | deterministic lexicographic tie-break, not completion-order; delete "resolves to whichever completes first"; restate continuation order | **APPLIED** | "the peer with the lexicographically smaller static public key is the winning initiator" (418–419); winner "silently dropped... no `ss`, no msg2, no guard record" (423–425); loser "cancel[s] our pending at the tie-break decision (pre-`ss`)... and run[s] the internal continuation as responder" (426–428); "never by completion order (each side's *inbound* handshake always completes first locally...)" (415–417) — the false sentence is gone, replaced by the correct reason; continuation step 1 explicitly notes "a lost tie-break has already cancelled our pending, a won one never reaches here" (456–457). |
| 3 — stage-0 queue DoS [MAINTAINER] | evict-oldest overflow; `INTRO_MAX_PER_SOURCE=4`; `INTRO_TTL` 90→15s; honest exposure clause | **APPLIED** | overflow "evicts the oldest unconsumed entry (by park time)" (244–246); `INTRO_MAX_PER_SOURCE` "4 unconsumed entries per source IP (per /64 for IPv6)" (211); `INTRO_TTL` "15 s" with the initiator-vs-responder-obligation rationale (212, 250–258); honesty clause states "no spoofing capability is needed", "endpoint-wide for new inbound accepts", "≈ 68 packets/second", "not WireGuard-equivalent" (316–334); `[MAINTAINER]` present (222–226). |
| 4 — own-bytes-on-consume [MAINTAINER] | unconsumed = transparent replace; consumed = owns bytes/IntroId, unsupersedable; `Superseded` deleted, not added | **APPLIED** | "the moment `read_identity()` is called, the chain owns its bytes and its `IntroId`... a consumed `Claimed`/`Proven` chain can therefore never be superseded" (270–274); "`Superseded` is removed from every error enum [MAINTAINER]" with the explicit divergence note (279–286); accounting stated as "one budget of `INTRO_QUEUE_CAP` slots" (287); `AlreadyConnected` edge case (296–300). Grepped the whole file: every remaining mention of "Superseded" is in a sentence *stating its removal* — zero live references. |
| 5 — eager-demote carries paid mid-state | 1 DH marginal, not 3; carried entry bound by hint-set sources | **APPLIED** | §5 step 3: demoted packet "carrying its paid mid-state, tagged identity-already-read... `read_identity()` on it returns the cached claimed static at 0 incremental DH" (365–371); typestate table footnote † (147–151); DoS table row fixed to "1 DH" (529). |
| 6 — accept()/Install/HandshakeFailed | accept() never followed by Install; HandshakeFailed shell-only; `handle_endpoint_event` narrowed | **APPLIED** | "`accept()` returns a fully established connection — never followed by an `Install`" (687–688); "`poll_output()` never emits `Install` for a `ConnectionId` surfaced via `accept()`" (690–691); "`HandshakeFailed` never reaches `core::Connection`... `handle_endpoint_event` is accordingly narrowed to carry only `Install{session, initial}`" (705–709); concrete surface: `fn handle_endpoint_event(&mut self, now: Instant, ev: Install)` — comment "Install only — never HandshakeFailed" (663). Both wrong-guess failure modes named and excluded (699–703). |
| 7 — error taxonomy | single closed taxonomy in §10, every variant named once, `EndpointDropped` everywhere needed | **APPLIED** | §10 enumerates all six enums with a firing condition per variant (1142–1165); "every variant this specification names appears here, exactly once each" (1136–1138); "The staged errors are §4's **three** types" (1136) — fixes con-3's "only two" claim. §4's restatement (191–202) and §6's signatures (637–641) use the identical variant lists — no drift. |
| 8 — security honesty + DoS repairs | DoS table rebuilt; off-path restated as capability; guard-eviction completed; attempt-spend added; swap-cut qualified; peer-restart ACK'd; membership oracle restated; ceilings table; pacing per-known-static; continuation numbered | **APPLIED** | DoS table: demote row "1 DH" (529); new "replayed genuine msg1... 2 DH... not capped by pacing" row (531); replacement row split into uncapped-2-DH + capped-+2-DH (532); explicit ceiling stated (534–536). Off-path restated as capability (548–556) with `(IP,port)`-or-dialled correction and ≈0-bit entropy note, plus the roam-obtainable note (557–560). Guard-eviction clause completed with all three (i)(ii)(iii) items (875–894) plus the two ruled mitigations (896–910), LRU "use" defined as admission-only (907–909). Attempt-spend exposure added to the accounting (538–546) with the tightened precondition "length-correct, index-matching, mac1-valid" (502–503). Swap-cut qualified "once the replacement completes on both sides" (939–941). Peer-restart gains the ACK'd-loss sentence (1201–1203). Membership oracle restated with `[MAINTAINER]` (397–407). Stage-0 warning extended to static+source+sender_index (159–161). Endpoint-state-ceilings table present (917–927). Pacing re-scoped "per known static" with guard-entry counter (479–483). Continuation is a numbered 1–4 list, tag→guard→pacing→admit, record-on-full-admission-only (454–464), silent-drop clause covers all three ("A failure at any of steps 1–3 — tag, guard, or pacing" 467–468). |
| 9 — mechanical/definitional | `Timeout(None)` defined; teardown-before-keepalive; `Endpoint<I:Identity>`; `send_unreliable` shared seq; sync sealing; deadlock invariant; `Retired` MUST; handshake.rs ruled replaced; §2 table completeness; round-2 item 4 verified; placeholder-constant notes; `PERSISTENT_KEEPALIVE` reclassified; pin-on-claim carve-out; `L` single home; `session_id()` defined; `next_counter()` codegen-vs-runtime; DoS row-2 reworded; pacing severability | **APPLIED** | All sub-items independently verified against source (§2 below) or by direct grep/read of the draft. No sub-item found unapplied. |

No ruling was applied in a way that contradicts another ruling — cross-checks
in §3 below (tie-break vs. continuation order, own-bytes vs. accessor rule,
shared-budget vs. ceilings table) all resolve cleanly.

---

## 2. Prior findings — closed/partial/open

### Implementability findings 1–14: **all CLOSED**

| # | Finding | Status | Note |
|---|---|---|---|
| 1 | BLOCKER: accept()/Install undecidable | **CLOSED** | Group 6; unambiguous rule + both failure modes named. |
| 2 | HandshakeFailed routing ambiguous | **CLOSED** | Group 6; shell-only variant, `handle_endpoint_event` type-narrowed to `Install` only (compiles differently than before — this *is* the fix, not just prose). |
| 3 | No "driver is gone" variant on staged errors | **CLOSED** | Group 7; `EndpointDropped` on `IntroError`/`AuthError`/`AcceptError`. |
| 4 | Keepalive-before-teardown inverts v1 | **CLOSED** | Group 9. **Verified against source**: `endpoint.rs` `on_tick` — `is_dead(now)` checked first, `continue`s on true (skips keepalive check for that session); the draft's cited line range (904–925) is close but the actual `is_dead`/keepalive block is at 915–923 (`on_tick` itself starts at 904) — a citation-precision NIT, not a correctness problem; the *order* claimed is exactly what the code does. |
| 5 | Deadlock-freedom not stated as invariant | **CLOSED** | Group 9; "no-blocking invariant" stated generally (731–739), accessors named as synchronous shared-cell reads. |
| 6 | `send_unreliable` seq-sharing unstated/unsupported | **CLOSED** | Group 9. **Verified against source**: `Recovery::queue_message` (`recovery.rs`) allocates `next_seq` and inserts into `outstanding`/`to_send` in one step today — no allocate-only primitive exists yet, confirming the draft correctly describes this as new, additive work, not something already present. Buildable: `next_seq` is a shared field: `let seq = self.next_seq; self.next_seq += 1; seq` with no insert satisfies the "same monotonic space, skip the queue insert" requirement without touching `queue_message`'s ten existing tests. |
| 7 | Check-vs-mark shed rule suspends replay detection | **CLOSED** | Group 1; mark-always restores the invariant. |
| 8 | "Survives verbatim" claim conflicts with allow-list dissolution | **CLOSED** | Group 9/Appendix B; `handshake.rs` ruled replaced, the four `accept_init`-calling tests reclassified to harness-rewrite, `[MAINTAINER]`-flagged. **Verified against source**: `accept_init` (`handshake.rs:447`) does take `allow: &HashSet<[u8;33]>` exactly as both reviews describe — the reclassification is the correct call. |
| 9 | `admit`/`next_packet` old-API-survives question | **CLOSED** | Appendix B states explicitly "kept as thin wrappers over the new check/mark (§8) and plan/commit (§6) split primitives" (1274–1277). |
| 10 | `Identity` trait used where a type is required | **CLOSED** | Group 9; `core::Endpoint<I: Identity>` shown (618–621, 624). **Verified against source**: v1's actual pattern is `struct Actor<I: Identity, W: Wire>` (`endpoint.rs:417`) monomorphized behind a non-generic `pub struct Endpoint` (`endpoint.rs:237`) constructed via `Endpoint::start<I,W>` (`endpoint.rs:248`) — the draft's "mirroring v1's `Actor<I,W>` monomorphise-then-erase pattern" claim is accurate. |
| 11 | `Retired` ordering/trigger not a stated MUST | **CLOSED** | Group 9; "`Retired` is a MUST... within the same drain... before releasing the connection's shell-side bookkeeping" (721–729). |
| 12 | `handle_timeout` idempotency rests on unstated sync-sealing | **CLOSED** | Group 9; "Sealing is synchronous... executes within the mutating call that triggers it... never lazily inside `poll_output()`" (813–819). **Verified against source**: `Recovery::next_packet` (`recovery.rs:297`) already mutates `to_send`/`ack_pending`/`ping_pending` inline during planning (e.g. `recovery.rs:315,328–331,345,347`) — there is no existing plan/commit split, so the draft's "on_pto cannot double-fire" claim depends on the shell obeying the synchronous rule as stated; correctly captured as a stated *rule* rather than an emergent property. |
| 13 | `AcceptError` never enumerated | **CLOSED** | Group 7. |
| 14 | `AuthError` missing `Superseded`, contradicting the supersession rule | **CLOSED** | Group 4 — resolved by deleting `Superseded` everywhere rather than adding it to `AuthError`, which is a stronger fix (removes the underlying attack, not just the typing gap) and is explicitly flagged as diverging from finding 14's literal suggested fix. |

### Consistency findings 1–18: **all CLOSED**

| # | Finding | Status | Note |
|---|---|---|---|
| 1 | BLOCKER: `IntroError::Superseded` unreachable | **CLOSED** | Group 4; deleted, not patched. |
| 2 | `Timeout(None)` undefined; quote says `Timeout(Instant)` | **CLOSED** | Group 9; "`Timeout(None)` = drained, and no deadline is armed; `Timeout(Some(d))` = drained, next deadline `d`" (713–715), "semantics are identical for both cores" (715); the §6 quote now reads `Timeout(Option<Instant>)` (780–781). Confirmed the *design-choices.md* source of the original mismatch (line 525, "`Timeout(Instant)`") is unfixable at the source (out of scope, not the reviewed document) — the draft correctly does not inherit it. |
| 3 | `AcceptError` named but undefined; §10 excludes it | **CLOSED** | Group 7; §10 now says "**three** types" and fully enumerates `AcceptError`. |
| 4 | Shed rule packet-vs-message granularity ambiguous | **CLOSED** | Group 1; "'Room' is whole-packet" stated explicitly (1028). |
| 5 | Round-2 item 4 silently asserted resolved | **CLOSED** | Group 9. **Independently re-verified against `hiss/src/noise/datagram.rs:353–356`**: the `steps > MAX_EPOCH_JUMP` refusal does `return Err(HandshakeError::DecryptionFailed)` — the identical variant `Cipher::decrypt`'s ordinary AEAD-tag-mismatch path also returns. The draft's claim is now genuinely true, not merely asserted. |
| 6 | §9.7 amended but missing from §2 table | **CLOSED** | Group 9; §9.7 row present at line 60. |
| 7 | v1 §9.2 "tokio's clock" contradicts sans-io no-clock rule | **CLOSED** | Group 9; §9–§9.3 row carve-out: "§9.2's `ack_delay` is measured on the shell-supplied `now`... not on tokio's clock" (56). |
| 8 | Guard+pacing order/record-interaction unspecified; tag-failure clause coverage | **CLOSED** | Group 8; numbered list, tag→guard→pacing, "record[ed]... only on full admission" (462–463), "A failure at any of steps 1–3 — tag, guard, or pacing" (467) covers all three. |
| 9 | Pacing mechanism underspecified | **CLOSED** | Group 8; "mechanism: minimum 20 ms spacing between accepted replacements, per known static" (477). |
| 10 | Orphan LRU "use" undefined | **CLOSED** | Group 8; "LRU 'use' is admission only: an entry's recency is refreshed by a successful post-`ss` record — never by a failed guard check" (907–909). |
| 11 | `ConnectError` ellipsis; two `ConnectionLost` variants unglossed | **CLOSED** | Group 7; closed set, all five `ConnectionLost` variants glossed (1160–1164). |
| 12 | §2's v1-§6 row omits swap-cut/epoch-death amendments | **CLOSED** | Group 9; row extended (53: "the rekey bullet is amended by §8... and the `MAX_EPOCH_JUMP` bullet augmented by §8"). |
| 13 | Placeholder constants presented as settled | **CLOSED** | Group 9; draft-notes on `INTRO_QUEUE_CAP`/`INTRO_MAX_PER_SOURCE`/`INTRO_TTL` (210–212), `TS_GUARD_ORPHAN_CAP` (873), `RECV_BUFFER` (1010). |
| 14 | `Intro` accessor staleness under replacement | **CLOSED** | Group 4; "accessors... reflect the newest bytes at call time... no second `IntroReady`" (265–268). |
| 15 | "Nothing durable keyed on it" vs. pin-on-mid-state tension | **CLOSED** | Group 9; explicit "bounded exception" carve-out (858–867), though the prose is dense — see NIT below. |
| 16 | Recovery test count "nine" vs. actual count | **CLOSED** | Group 9; corrected to "ten" (1284). **Independently re-verified**: `recovery.rs::mod tests` contains exactly 10 test functions (`has_retransmittable_discriminates_payload_from_control` … `ack_for_unsent_counter_is_ignored`). |
| 17 | `PERSISTENT_KEEPALIVE` silently reclassified | **CLOSED** | Group 9/8; "reclassified from a frozen constant to the recommended default... wire-invisible" stated explicitly (1000–1004). |
| 18 | Attempt-take gate omits v1 preconditions | **CLOSED** | Group 8; "taken on the first **length-correct, index-matching, mac1-valid** msg2" (501–503). |

NITs 19–24 from `review-consistency.md` are also substantially closed by
Group 9 (lateness-bound single-home by reference, Appendix A codegen-vs-
runtime distinction, DoS row-2 reworded, `session_id()` defined, pacing
severability note carried); NIT 22 ("no unbounded logging" testability) is
carried unchanged, which is within its own stated option ("or accept it as
review guidance").

---

## 3. New findings (introduced or exposed by the round-2 rewrite)

### N1 — [MAJOR] `shed_mask`-based ACK construction under-specifies how `largest`/`first_range` are recomputed, and the natural implementation reintroduces the exact permanent-loss bug Group 1 exists to prevent

**Section/line.** §8 "Receive backpressure", 1038–1040: "**ACK-range
construction subtracts `shed_mask` from the window snapshot**... so a shed
counter is never acknowledged." §2's carve-out (56) and Appendix B's shed
obligations (1330–1332) repeat the same phrasing without going further.

**The gap.** The existing `AckFrame::from_window(greatest, bitmap, ack_delay)`
(`src/frame.rs:298–343`, unchanged by anything this draft names) computes
`first_range` by walking **from bit 1 upward** — it never inspects whether
bit 0 (`greatest`) itself is set, and unconditionally emits `largest: greatest`
(`frame.rs:338`). This is safe in v1 only because `admit()` guarantees bit 0 is
always set whenever the window is non-empty (mark is unconditional and
atomic with check). Group 1 breaks that guarantee on purpose: `greatest` now
advances to the fresh counter of a packet that may itself be **shed** (i.e.
`shed_mask` bit 0 = 1) — and under sustained backpressure, the newest arrival
being the one that gets shed is not a corner case, it is the *steady state*
the whole mechanism targets.

If the connection core does what the draft's phrasing most naturally suggests
— call the existing `from_window` with `(greatest, bitmap & !shed_mask,
ack_delay)`, i.e. mask the bitmap but leave `greatest` untouched — the
resulting `AckFrame` still reports `largest = greatest` (the shed counter)
and a `first_range` computed by a loop that never checks bit 0, so it happily
extends the range to include position 0 based on the received-ness of bits
1, 2, 3, ... alone. Per the wire semantics the first block "covers `largest −
first_range ..= largest`" (v1 §9.2, unchanged) — so the resulting ACK
**falsely claims the shed counter as acknowledged**. That is precisely "the
permanent-loss bug the rule exists to exclude" that §8 itself names three
times (1031, 1041–1042) — reintroduced through the one code path (`largest`
derivation) the shed_mask carve-out doesn't mention.

The fix is a well-defined, one-paragraph addition (e.g. "recompute `largest`
as the highest set bit in `window.bitmap & !shed_mask`, walking downward from
`window.greatest`; if none is set, no ACK is owed this round"), but the draft
as written gives an implementer no procedure for this — and the "obvious"
reading (mask the bitmap, keep `greatest`) is the wrong one. This is exactly
the implementability-review's own bar for MAJOR ("an ambiguity an
implementer would have to guess"), and the guess that requires the least
code change is the one that is broken. Not covered by any Appendix B
obligation (the shed-rule obligation list at 1330–1332 tests
marked/masked/never-ACKed/whole-packet — not this).

**Fix.** One sentence at the `shed_mask` paragraph (§8): state that ACK
construction must recompute the reported `largest` as the greatest *unshed*
counter in the current window (not the raw `window.greatest`), with the
degenerate case (nothing unshed in range) producing no ACK this round; add a
test to Appendix B's shed obligations exercising "the newest counter is
shed" specifically.

### N2 — [MAJOR] `INTRO_TTL = 15 s` (Group 3) combined with own-bytes-on-consume (Group 4) creates a hard, unstated ceiling on staged-decision latency — an application whose accept policy takes longer than 15 s can never complete an `accept()`, no matter how long the peer retries (up to 90 s)

**Section/line.** §4 "Expiry" (250–258) and "Resolve stages promptly (SHOULD)"
(301–307), interacting with §5's routing rule (349–380).

**The gap.** Group 3 correctly scoped `INTRO_TTL`'s reduction to the
*unconsumed* case ("entries are superseded every ~5.3 s in normal operation,
so 15 s is ample" — true, since an unconsumed entry's TTL is refreshed by
every retransmit). Group 4 separately and correctly ruled that a *consumed*
chain "does not extend it" — its 15 s runs from the original initiation and
is never refreshed, by design, to close the supersession attack. Composing
the two: once `read_identity()` is called, the app has a flat 15 s to reach
`accept()`, and — because the stage-0 slot for that source is freed on
consumption — the peer's *next* retransmit (still coming every ~5.3 s, since
it does not yet know it has been consumed) parks as a **brand-new,
unrelated** `Intro` with a fresh `IntroId`, triggering a fresh
`IntroReady` (this is exactly what §4/Appendix B document, "own-bytes-on-
consume: a consumed chain is unsupersedable and a post-consumption
initiation parks as a new entry with a new `IntroReady`," 1321–1323).

The consequence the draft never states: an application whose accept
decision takes longer than 15 s in aggregate (an interactive approval UI, a
directory lookup with real latency, an audit-log round trip) can **never**
successfully `accept()` a peer, however patiently the peer retries within
its 90 s give-up horizon — every 15 s it will instead be handed a *new*,
independent `Intro` for the same peer (each costing another 1–2 DH to
re-resolve), never one continuous chain it can sit on. This is a real
regression against the pre-Group-3 90 s TTL (ample for any plausible UI
latency) that the round-2 rulings did not jointly analyse: Group 3's own
"ample" justification is scoped correctly to the unconsumed case, but the
draft never revisits whether 15 s is still "ample" for the *consumed* case
once own-bytes removes the extension. The "**SHOULD**"-level "resolve stages
promptly" advice (301) undersells this: it reads as best-practice guidance,
not as "there is a hard 15 s wall below which no staged-decision workflow
can ever succeed." The peer-restart seq-collision gets an explicit
`[MAINTAINER]`-flagged "known limitation" treatment in §11; this
latency-ceiling interaction gets none, despite being at least as consequential
for any application that actually wants a human (or slow policy) in the
loop — arguably the whole reason the typestate exposes long-lived `Claimed`/
`Proven` handles instead of a synchronous v1-style callback.

**Fix.** Either (a) note this explicitly as a known limitation alongside §11's
peer-restart item ("a staged-accept decision must complete within
`INTRO_TTL` of `read_identity()`; slower decision processes will see the same
peer as a series of independent `Intro`s and can never reach `accept()`"), or
(b) reconsider whether `INTRO_TTL` should govern the *unconsumed* case only
(15 s, as ruled) while a *consumed* chain gets a separately-configurable,
longer validity window (since a consumed chain's queue-occupancy cost is
already exempted from the attacker-driven caps per Group 4's own accounting —
"consumed chains are exempt: they are application-driven, not
attacker-driven" — so a longer consumed-chain TTL does not reopen the Group 3
DoS this whole change was about). (a) is the minimal fix; (b) is the more
complete one.

### N3 — [NIT] Carried-mid-state replacement semantics under repeated eager-path packets not fully spelled out

§5 step 3 (365–373) and §4 "Consumption and supersession" (262–269) together
imply, but never state, that a *second* packet from the same hint-set-spoofed
source with an unknown claimed static both re-pays `es` (a fresh mid-state)
**and** replaces the carried entry's prior mid-state (freeing the old one) —
i.e., that "transparently replaces the entry's bytes" (264) also replaces its
mid-state for a carried entry, not just its raw bytes. The DoS-table row (529)
prices this correctly (1 DH per packet, uncapped) so the intended behaviour is
inferable, but a one-clause addition ("a replacement of a carried entry also
replaces its mid-state, at the cost of a fresh `es`") would close the gap
outright rather than leaving it to inference.

### N4 — [NIT] Pin-on-mid-state / no-orphan-on-reject ordering reads as tension on a first pass

§7's "the pin never creates an entry... flipping a bit on an entry a
key-holder already wrote" (858–867) and the later "no orphan entry... its
guard record is dropped with the chain (an entry that pre-existed the chain
reverts to its prior state)" (896–901) are logically consistent once
unpacked carefully (verified by trace: the entry is *written* by
`authenticate()`'s post-`ss` admission regardless of any pre-existing state;
the pin is a status derived from live references, not a second write; the
"reverts to its prior state" clause is a deliberate, narrow undo scoped to
the reject-without-accept case, layered on top of — not contradicting — the
general "LRU use is admission-only" rule stated two paragraphs later). But
the ordering of presentation (the specific carve-out appears before the
general rule it's an exception to) makes a first read feel like a
contradiction. Reordering the two clauses, or adding "see the general rule
below," would remove the friction. Not a logic error.

### N5 — [NIT] Citation precision

The draft cites `endpoint.rs:904–925` for the teardown-before-keepalive order
(768); the actual `is_dead`/`should_keepalive` block is at `endpoint.rs:
915–923` (`on_tick` itself starts at 904, so the citation isn't wrong, just
imprecise about which lines carry the specific logic quoted).

None of N1–N5 touches the frozen wire, contradicts a round-2 ruling, or
blocks the phase from being built; N1 and N2 are the two that should get a
sentence each before ratification, since both are "an implementer/operator
would hit this and have no textual answer" gaps of the same shape the
implementability review was built to catch — and N1 in particular resurrects
(via omission, not intent) the exact bug class Group 1 was written to close.

---

## 4. Interpretation rulings — the reviser's six synthesis calls

All six checked against the draft body (not just the closing index) and
against each other for consistency:

1. **One shared `INTRO_QUEUE_CAP` budget** (not two independent pools for
   unconsumed vs. consumed). *Faithful.* "Accept-side state is **one budget**
   of `INTRO_QUEUE_CAP` slots" (287). The §7 ceilings table's second row
   ("Staged mid-states ... ≤ `INTRO_QUEUE_CAP`", 925) is not a second,
   additive pool — it's the same population viewed by "how many of the
   budget's occupants currently hold a mid-state," which is necessarily
   ≤ the total budget. Verified this is not a doubled-accounting bug.
2. **A tie-break loser resolves via the continuation's `Install{initial:
   true}`**, unifying initiator- and responder-role establishment under one
   event. *Faithful.* Confirmed by trace of both sides of a mutual-dial race
   (§5): the winner's own pending completes via an ordinary msg2; the loser's
   pending is the *same* `core::Connection` object that receives `Install`
   from the continuation it now runs as responder — matches "whether the
   session came from msg2 completion or from a lost tie-break's responder
   continuation" (695–696) exactly.
3. **A pre-existing guard entry reverts to its prior state on drop** (the
   no-orphan-on-reject mitigation is a narrow undo, not a general write-path
   rule). *Faithful*, but see N4 — the ordering of presentation invites a
   momentary misread.
4. **≈68 pps sustaining rate** (1024 slots / 15 s). *Faithful, arithmetic
   confirmed*: 1024/15 = 68.27.
5. **`shed_mask` slides with the window**, same indexing. *Faithful* as a
   bit-mechanics claim (1035–1036); does not by itself resolve N1, which is
   about `largest`/`first_range` derivation, not mask indexing.
6. **`INTRO_TTL` applies to consumed chains too** (no extension on
   consumption). *Faithful as stated* (253–256) — and this is precisely the
   ruling whose interaction with Group 3's TTL cut produces N2. The
   interpretation itself is not wrong; it's under-analysed in combination
   with the other group.

---

## 5. Buildability of the changed surfaces (task item 4)

- **Narrowed `handle_endpoint_event`**: `fn handle_endpoint_event(&mut self,
  now: Instant, ev: Install)` (663) is a real type-level narrowing (not just
  prose) — `HandshakeFailed` cannot reach `core::Connection` by construction,
  closing implementability finding 2 completely rather than just
  documenting a convention.
- **`shed_mask`/check-mark split against `src/session.rs`**: buildable as a
  refactor of `ReplayWindow::admit` (`session.rs:73–104`, currently one
  atomic `&mut self` method combining check+mark) into `check`+`mark`, with
  `admit` kept as a thin wrapper per Appendix B — confirmed mechanical. The
  one residual gap is N1 (ACK-side consumption of the split), not the split
  itself.
- **`Recovery` allocate-seq-without-tracking primitive against
  `src/recovery.rs`**: confirmed genuinely new (no such method exists today;
  `queue_message` allocates-and-tracks in one step) and trivially additive
  (`next_seq` is a shared field), with no risk to the existing ten
  `recovery::` tests it must not disturb.
- **Cached-mid-state carry (G5) against hiss's split-read API**: confirmed
  buildable end-to-end. `HandshakeInner<...>` (hiss `handshake.rs:36–62`)
  carries no lifetime parameters, so the proposed `IKResponderMsg1Mid<CP>`
  (inner + an **owned** `[u8; N]` tail, per Appendix A's option (a)) has
  nothing tying it to the original message buffer — it can sit in a
  `HashMap<IntroId, ...>` for up to 15 s, be dropped and replaced like any
  other state, and its `claimed_static(&self)` accessor is callable
  repeatedly (direct precedent: `remote_static(&self)` already exists in
  exactly this non-consuming shape on every other generated state, per
  `hiss-macros/src/codegen.rs:660–664`). The macro's per-message read is
  currently one monolithic token-driven loop (`codegen.rs:1094–1160`); the
  split is a real refactor (slicing that loop into two subranges) but not in
  tension with anything in the existing codegen shape, and the `WireSize`
  machinery needed for `__MSG1_TAIL_SIZE` already operates on arbitrary
  token subslices (`codegen.rs:158–165`), so it doesn't require inventing
  new size arithmetic the macro's own module doc disallows.
- **Compressed-key tie-break comparison**: `hiss`'s 33-byte compressed SEC1
  encoding (`src/curve/p256/mod.rs`) is a canonical bijection with the
  65-byte uncompressed point (deterministic parity-prefix compression,
  canonical-field-element decompression) — no two distinct 33-byte strings
  can represent the same key, so the tie-break's byte-comparison is
  well-defined on both sides.

Nothing in this section is still impossible or still ambiguous beyond N1/N2
above.

---

## 6. Testability + house style

- **DRAFT markers**: present on all 13 section headers including both
  appendices (grepped directly: §1–§13, each `*(DRAFT 2026/08/13...)*`).
- **Constants in tables**: holds throughout — `INTRO_QUEUE_CAP`/
  `INTRO_MAX_PER_SOURCE`/`INTRO_TTL`, `INITIATIONS_PER_SECOND`, `L`,
  `RECV_BUFFER`, `TS_GUARD_ORPHAN_CAP` all tabled with per-table draft-notes
  where the value is still a placeholder.
- **No wire restatement**: holds — the draft consistently cites v1 by
  section/line rather than re-deriving byte layouts; the one intentional
  exception (§8's replay-window-derivation chain) repeats *constants* with
  citations, the same style `review-consistency.md` already accepted.
- **`[MAINTAINER]` bookkeeping**: the draft claims "Thirteen in total" (8
  carried + 5 new). Independently recounted by grep: exactly 13 distinct
  flagged clauses (§1 no-fallback, §3 one-connection, §4 names-final, §4
  dedup-key, §4 Superseded-removal, §4 queue-DoS-posture, §5
  membership-oracle, §6 two-cores, §7 eviction-consequence, §7
  guard-eviction-mitigations, §8 swap-cut, §11 peer-restart, Appendix B
  handshake.rs-replaced) — matches the draft's own count exactly, and
  matches round2-resolutions.md's "For the reviser" instruction to add
  exactly these five.
- **"Round-2 changes applied" index** (1340–1483): checked against the body
  group-by-group during §1 above — accurate; every claim in the index is
  independently confirmed present in the section it names. No index entry
  overclaims or references something that didn't land.
- **New-sentence testability**: essentially every new normative clause maps
  to an Appendix B obligation (1318–1336) — the two exceptions are exactly
  N1 and N2 above, which is further evidence they're genuine blind spots
  rather than deliberately-deferred items (nothing marks them as such).

---

## Verdict: **FAITHFUL-WITH-FIXES**

All nine resolution groups landed faithfully and completely; all 14
implementability findings and all 18 consistency findings are CLOSED (most
independently re-verified against the real slither/hiss source, not just
read as prose); the six interpretation calls are each faithful, with one
(own-bytes + 15 s TTL) whose *composition* wasn't jointly analysed and now
surfaces as N2. Two new MAJOR gaps were found by hunting the changed
surfaces against source and against each other: N1 (the `shed_mask` ACK
carve-out doesn't say how to re-derive `largest`, and the natural
implementation reintroduces the permanent-loss bug Group 1 exists to
prevent, reachable in the exact sustained-backpressure steady state the
mechanism targets) and N2 (`INTRO_TTL=15s` × own-bytes-on-consume means any
staged-accept decision slower than 15 s can never reach `accept()`,
regardless of the peer's 90 s retry budget — undocumented as a limitation).
Neither breaks the frozen wire or is a structural rework; each is a
paragraph. Three NITs (N3–N5) are presentation/inference gaps, not logic
errors.

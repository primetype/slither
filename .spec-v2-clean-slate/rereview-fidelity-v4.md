# Re-review — batch-surgery fidelity & consistency (draft v3 → v4)

**Reviewer pass:** fidelity-and-consistency only (did the surgery apply the
30 rulings faithfully, with no drift, no leftovers, no broken references).
Not an adversarial re-review of the restructured §§5–7 — that is still owed.

**Artefacts checked**

| Artefact | Path |
|---|---|
| Work order | `.spec-v2-clean-slate/rulings.md` |
| Revised spec (3259 lines) | `SPEC-v2-DRAFT.md` |
| Reviser's change log | `.spec-v2-clean-slate/batch-surgery-notes.md` |
| Pre-surgery frozen text (3364 lines) | `.spec-v2-clean-slate/SPEC-DRAFT-v3-pre-surgery-frozen.md` |
| Old ratified wire (for J-1) | `SPEC.md` |

**Verdict: CONFORMANT** — every ruling's stated batch-edit implications are
applied; no deleted identifier survives as live text; every §-reference
resolves; every constant reconciles; all arithmetic recomputes correctly.
Three items need a maintainer ruling rather than editorial acceptance
(FIDV4-1, FIDV4-2, FIDV4-3); none of them is a wire-byte or a contradiction.

**Finding count:** 0 BLOCKER · 3 MAJOR · 4 MINOR · 5 NOTE.

---

## 1. Resolution of the two known leads

### Lead A — `AddressMoved { from, to }` at line ~2615

**Resolved: false alarm. J-4 is accurate; no fix required.**

Line 2615 sits inside `enum ConnEvent` (opened at line 2605, closed at line
2617), not inside `enum ToEndpoint` (lines 2618–2620, which carries
`Retired { our_index: u32 }` alone). The pre-surgery text confirms the
deletion landed exactly where J-4 says it did: the frozen draft's
`enum ToEndpoint` carried **`NeedsRekey { … }`** and
**`AddressMoved { to: SocketAddr }`** (frozen text, `ToEndpoint` block —
diff hunk `@@ -2713,11 +2616,6 @@`), and both are gone. The two variants are
distinct symbols with distinct signatures:

- deleted: `ToEndpoint::AddressMoved { to: SocketAddr }` — endpoint-bound,
  existed solely to keep the connection-derived hint map and rekey
  targeting fresh (pre-surgery §7.3, §17.4). Both consumers are deleted by
  rulings 4 + 11, and §17.4 is rewritten to "the endpoint tracks no
  per-connection address". Deletion is correct and consequential.
- surviving: `ConnEvent::AddressMoved { from: SocketAddr, to: SocketAddr }`
  — connection→shell, application-facing observability, backing
  `remote_address()` (§16.2, §16.8's shared-cell accessors) and the
  `slither::roam` trace target (§18.2). Live protocol surface, correctly
  retained; nothing in the rulings touches it.

Residual nit only: see FIDV4-6.

### Lead B — 28 markers claimed, 27 counted

**Resolved: the reviser's 28 is correct; the 27 is a grep artefact.**
Reconciliation (`grep -n '\[RATIFIED' SPEC-v2-DRAFT.md` returns 29 lines):

| Cause | Δ |
|---|---|
| Total lines containing `[RATIFIED` | 29 |
| minus line 152 — §1.3's **meta-mention** of the marker, not a clause marker | −1 |
| = clause-level markers | **28** |
| A single-line `grep '\[RATIFIED 2026/08/14'` misses two **line-wrapped** markers: §1.1 (L87→88) and §16.1 (L2437→2438) | −2 |
| …but that same grep **counts** the §1.3 meta-mention | +1 |
| = the observed count | **27** |

Both wrapped markers are real and well-formed:
`87: … **[RATIFIED` / `88: 2026/08/14]** No released software …` and
`2437: … **[RATIFIED` / `2438: 2026/08/14, clause flipped per the ratchet-only ruling.]**`.

`[MAINTAINER]` count is **exactly 2**, as required by ruling 30: L156
(§1.3's record of the pending A.1 gate) and L2993 (Appendix A.1 itself).
`[OPEN]` appears only in the preamble's own statement that none remain
(L16–17). The preamble's header sentence names A.1 without the bracketed
form (L13–15), preserving the count of 2.

---

## 2. Marker inventory (28 clause-level `[RATIFIED 2026/08/14]`)

| # | Line | Section | Ruling(s) | Form |
|---|---|---|---|---|
| 1 | 87–88 | §1.1 The first wire | 1 (modified) | wrapped |
| 2 | 377 | §4.4 Suite genericity | 3 | plain |
| 3 | 446 | §5.4 One session per connection | 4 + 5 | `— the ratchet-only ruling.` |
| 4 | 571 | §6.1 Typestate names | 6 | plain |
| 5 | 641 | §6.3 Flood posture | 7 | plain |
| 6 | 648 | §6.3 Dedup key | 8 | plain |
| 7 | 715 | §6.3 `Superseded` removal | 9 | plain |
| 8 | 758 | §6.4 `accept()` re-home | 10 | plain |
| 9 | 832 | §6.5 Membership oracle | 11 | `, in reduced form.` |
| 10 | 1028 | §7.2 Fused ACK record | 12 | plain |
| 11 | 1054 | §7.3 Anti-amplification budget | 13 | plain |
| 12 | 1136 | §7.5 Keepalive floor | 14 (reduced) | plain |
| 13 | 1270 | §8.2 Structural = signalled death | 15 (home 1) | plain |
| 14 | 1515 | §9.1 Four ID spaces | 16 | plain |
| 15 | 1675 | §9.8 Messages as sugar | 17 | plain |
| 16 | 1715 | §9.8 Overflow policy | 18 | plain |
| 17 | 1765 | §10.2 Initial windows | 19 | plain |
| 18 | 1819 | §10.4 Cumulative MAX_STREAMS | 20 | plain |
| 19 | 1849 | §10.6 Credit-as-buffer | 21 | plain |
| 20 | 1868 | §10.6 Reassembly bound | 22 | plain |
| 21 | 2018 | §12.4 Delayed ACK | 23 | plain |
| 22 | 2297 | §14.6 Roam reset seam | 24 (reduced) | `, in reduced form — the rekey seam is deleted with §7.6…` |
| 23 | 2330 | §15.1 Minimal CLOSE | 25 | plain |
| 24 | 2374 | §15.2 ProtocolViolation variant | 15 (home 2) | `— one ruling, two homes with §8.2.` |
| 25 | 2437–8 | §16.1 One-connection-per-static | 26 (flipped) | wrapped, `clause flipped…` |
| 26 | 2533 | §16.4 Two sans-io cores | 27 | plain |
| 27 | 2804 | §17.1 Guard-eviction honesty | 28 | plain |
| 28 | 2950 | §19 Deferral block | 29 | plain |

Non-marker occurrences: L152 (§1.3 meta-mention). `[MAINTAINER]`: L156, L2993.

---

## 3. Per-ruling conformance checklist

Legend: ✅ applied · ⚠️ applied with a flagged deviation · ❌ missed.

| # | Ruling | Status | Line evidence |
|---|---|---|---|
| 1 | §1.1 no version bump; `VERSION` 0x01, `PROLOGUE` `b"slither\x01"`; reframe as first wire; retitle; accepted note | ⚠️ | Title L1; preamble L3–11, L54–70; §1.1 L87–102; §3.1 L248; §5.1 L400; table L3217–3218. `\x02` / `0x02` version: **zero** hits. Accepted note L104–112 — see **FIDV4-1**. "One-time golden regeneration" → first-freeze: §1.3 L141–143, §2.4 L239–240, §4.4 L389–390, App B L3058–3060. "v1 shell"/"v1 implementation" left as-is — **FIDV4-8** |
| 2 | §1.3 block re-affirmation → individual records | ✅ | §1.3 L145–157: eight individually re-ratified, swap-cut moot, A.1 pending with its `[MAINTAINER]` |
| 3 | §4.4 mac1 suite genericity ratified | ✅ | L377–393; marker flipped; "no shipped bytes existed", "pre-release drafts'" |
| 4 | **RATCHET-ONLY REKEY** (the headline) | ✅ | See sub-table below |
| 5 | §5.4 routing machine, surviving half | ✅ | LIVE/PENDING/NONE L461–481; frozen mid-state §6.3 L700–713; teardown-at-`accept()` L470–478, §6.4 L778–789; flag-blind re-home L768–777 |
| 6 | §6.1 `Intro`/`Claimed`/`Proven` final | ✅ | L571–572 |
| 7 | §6.3 flood posture | ✅ | L641–645 |
| 8 | §6.3 dedup key = full SocketAddr, replace-with-newest, + mobility note | ✅ | L647–660; mobility note L652–654 ("stage-0-only … convergence … roaming's job — §7.3") |
| 9 | `Superseded` deleted from error enums | ✅ | §6.3 L715–717; §18.1 L2881. Three surviving hits (L148, 715, 2881) are all deletion statements |
| 10 | §6.4 re-home model; delete CONTINUATION-candidate paragraph | ✅ | L756–792; the flag paragraph is gone (diff `@@ -756,30 +771,22 @@`); guard bullet now "a guard-passing proven-LIVE admission replaces" |
| 11 | §6.5 oracle reduced; hint set = in-flight dials; internal path = tie-break only; LIVE parks; `AlreadyConnected` connect()-only | ✅ | Hint set L800–804, L2854–2862; tie-break-only L806–818, L824–830; oracle L832–839; LIVE parks L470–478; `AcceptError::AlreadyConnected` deleted L787–789, L2882–2884, L2905–2906 (J-5) |
| 12 | §7.2 fused ACK record | ✅ | L1028–1039 |
| 13 | §7.3 never-lifted 3× budget | ✅ | L1054–1088; "a fourth reset seam" → "a second reset seam" L1083, mirrored §19 L2964 |
| 14 | §7.5 keepalive floor; idle-rekey half moot | ✅ | L1136–1144; `REKEY_AGE` consult deleted |
| 15 | §8.2 + §15.2 signalled death, `ProtocolViolation{code}` | ✅ | L1270 and L2374 (twin homes; the 2473 flag recorded satisfied) |
| 16 | §9.1 four ID spaces | ✅ | L1515 |
| 17 | §9.8 messages = uni-stream sugar | ✅ | L1675 |
| 18 | §9.8 overflow ⇒ receiver reset, code 0, regenerated | ✅ | L1715–1732 |
| 19 | §10.2 initial windows | ✅ | L1765–1778 |
| 20 | §10.4 cumulative MAX_STREAMS | ✅ | L1819 |
| 21 | §10.6 credit-as-buffer-commitment | ✅ | L1849 |
| 22 | §10.6 `REASSEMBLY_CHUNKS_MAX` 1024 | ✅ | L1868–1889 |
| 23 | §12.4 delayed ACK (2nd / 25 ms / gap-immediate) | ✅ | L2018–2041 |
| 24 | §14.6 roam seam verbatim; rekey seam deleted | ✅ | L2295–2318; roam bullet byte-identical to frozen text (diff `@@ -2388,16 +2292,13 @@` shows only the rekey bullet and the heading changing) |
| 25 | §15.1 minimal CLOSE | ✅ | L2330–2343 |
| 26 | §16.1 clause flipped (inbound proven-LIVE ⇒ replacement via `accept()`) | ⚠️ | L2437–2449. Batch note "`Install{initial:false}` remains only for the tie-break admission" **not** applied literally — see **FIDV4-2** |
| 27 | §16.4 two cores, single-`poll_output` | ✅ | L2533–2542 |
| 28 | §17.1 guard eviction + pinning + no-orphan-on-reject | ✅ | L2804–2831 |
| 29 | §19 deferral block | ✅ | L2950–2970 |
| 30 | Appendix A.1 stays **PENDING** with its `[MAINTAINER]` flag | ✅ | L2993; §1.3 L154–157; preamble L13–15 |

### Ruling 4 sub-checklist (the headline restructure)

| Implication | Status | Evidence |
|---|---|---|
| CONTINUATION/CONTINUED flags deleted; msg1 payload 13→12 B; msg2 payload 1→0 B | ✅ | §5.2 L409–418; `channel!` `[12]`/none L186–188; `FLAG_`, `HS_FLAGS_LEN`, `MSG2_PAYLOAD_LEN`: **zero** hits document-wide |
| Reserved-flag-bits malformedness rule gone | ✅ | Removed at §5.2 (diff L367–369 `-`) |
| Reference sizes 197→196, 108→107 (and 175→174, 82→81) | ✅ | §2.3 L218–224; §3.2 L265, L269; §3.3 L278, L286; §6.3 L635; §6.9 L929, L981–982; table L3224–3225. **Zero** stale 197/108/175/82 hits |
| §5.4 rewritten; LIVE row is the only LIVE row | ✅ | L444–487 |
| Internal continuation path deleted; every handshake through staged accept except the tie-break | ✅ | §6.5 L794–839, §6.6 L841–870 ("The internal tie-break completion") |
| §7.6 DELETED with stub; swap-cut flag moot | ✅ | L1158–1166 |
| §7.8 collapses; survival matrix + re-queue rule gone | ✅ | L1202–1214 |
| §7.9 rewritten: no rekey escape, exhaustion ⇒ death | ✅ | L1216–1230 |
| §7.5 keepalive no longer consults `REKEY_AGE`; `DEAD_TIMEOUT` the only idle killer | ✅ | L1136–1150 |
| §18.1: `PeerRestarted`, `RekeyFailed` deleted; `Replaced` stays | ✅ | L2907–2919. `PeerRestarted`/`RekeyFailed`: **zero** hits |
| §14.6 CC-reset-at-swap and §13's swap interactions deleted | ✅ | §13.6 L2161–2165, §14.6 L2295–2299, §13.1 L2084–2086 |
| Timers table: `REKEY_AGE`/`REJECT_AGE` rows out; no-message-count note rewritten | ✅ | §5.7 L542–553; constants table row deleted |
| Accepted losses recorded (no post-compromise healing; linkability) | ✅ | §7.7 L1186–1191 (TLS 1.3 KeyUpdate model; WireGuard overridden). *Linkability across roams is **not** separately restated* — see FIDV4-11 |
| Appendix B rewritten | ✅ | L3057–3211; continuation matrix → local-state routing L3079–3087; rekey survival bullet → one-session-per-connection L3165–3168; idle-rekey bullet → "no handshake ever re-run" L3180; pacing test gone; tie-break bullet loses "stable across rekey" L3088–3091 |
| Preamble cluster A/F summaries rewritten | ⚠️ | Annotated, not rewritten — **FIDV4-4** |

### Flag-reclassification checklist (rulings.md, "25 stops remain")

| Stop | Directive | Status |
|---|---|---|
| 1228 (§7.6) | MOOT — section deleted | ✅ L1158–1166 |
| 1179 (§7.5) | REDUCED — floor survives, idle-rekey moot | ✅ L1136–1144 |
| 2393 (§14.6) | REDUCED — rekey seam moot, roam seam survives | ✅ L2297–2298 |
| 832 (§6.5) | REDUCED — reword the oracle to PENDING-probing | ✅ L832–839 |
| 2537 (§16.1) | REDUCED — "never a new accept" FLIPS | ✅ L2440–2445 |
| 22 LIVE-unaffected stops | marker flip only | ✅ all 22 accounted for in the inventory above |
| §6.6 collapse note | rewrite §6.5–6.6 around staged-everything | ✅ L794–870; J-9 records the wording choice |

---

## 4. Deleted-symbol sweep

Every hit classified. **Zero live leftovers.**

| Symbol | Hits | Classification |
|---|---|---|
| `CONTINUATION` | L23, L64, L457 | historical (preamble cluster A), deletion announcement (preamble v4), deletion statement (§5.4). **All historical/deletion** |
| `CONTINUED` | L23, L25, L64, L457 | as above (L25 is the round-1 cluster-A history) |
| `FLAG_` | — | none |
| `HS_FLAGS_LEN` | — | none |
| `MSG2_PAYLOAD_LEN` | — | none |
| `REKEY_AGE` | L1142, L1160 | §7.5's moot note; §7.6's deletion stub. **Both deletion notes** |
| `REJECT_AGE` | L1161 | §7.6 deletion stub |
| `NeedsRekey` | L1161 | §7.6 deletion stub |
| `PeerRestarted` | — | none |
| `RekeyFailed` | — | none |
| `restart-replacement` | — | none |
| `AddressMoved` | L2615 | `ConnEvent::AddressMoved` — **live, correctly retained** (see Lead A) |
| `INITIATIONS_PER_SECOND` | — | none (deleted — see FIDV4-3) |
| `Superseded` | L148, L715, L2881 | §1.3's record of the removal ruling; §6.3's ruling text; §18.1's "appears nowhere". **All deletion statements** |
| `shed_mask` | L1851 | §10.6's historical "replaces the superseded draft's `shed_mask`/`RECV_BUFFER`" |
| "internal continuation" | — | none — replaced everywhere by "internal tie-break" (§6.5 L807, §6.6 title L841, §16.1 L2432, §18.2 L2940) |
| `rekey` (case-insensitive, 27 hits) | — | 6 epoch-ratchet (`REKEY_EPOCH_MSGS`, Noise `Rekey()`, the KAT — L175, L1172, L1177, L1179–1183, L3228); 2 WireGuard timer-name derivations (L544–545 Rekey-Timeout / Rekey-Attempt-Time); 5 preamble/history (L23, L40, L57–58); 14 "there is no rekey / the ratchet is the only rekey / deleted" statements (L448, L452, L519, L550–552, L562, L1141–1142, L1160–1163, L1210, L1221, L2163, L2297, L2572, L2627). **No live DH-rekey mechanism text** |
| `restart` (17 hits) | — | all §5.4/§6.8 "restart is structural" text, the §7.1 "never restarts" counter statement, §14.4's "slow start … restarts", §9.2's "restart the reassembler", §17.1's WireGuard responder restart, §19's persistence row. **All correct** |

---

## 5. Cross-reference integrity

**Section references.** All 3259 lines scanned; every `§X[.Y]` token
extracted and resolved against the heading set (`1`…`19`, `1.1`…`18.2`,
Appendix A, Appendix B). **Unresolved: 6, all RFC citations, all correctly
prefixed** — RFC 9000 §4.6 (L1831), §19.3.1 (L1977); RFC 9002 §6.2.1
(L2125), §6.2.4 (L2143), §7.3.2 (L2228), §7.6.1 (L2244). **No dangling
slither §-reference exists.**

**Heading structure.** Diffed against the frozen text: **no renumbering
cascade**. Exactly ten headings changed, all as logged (§1/§1.1 "one break"→
"first wire"; §3.2/§3.3 sizes; §5.4; §6.6; §7.6; §7.8; §10.7; §13.6; §14.6;
§17.4). §7.6 retained as a numbered deletion stub, as intended.

**Every remaining `§7.6` reference.** 7 hits, 6 slither + 1 RFC:

| Line | Context | Verdict |
|---|---|---|
| 61 | preamble: "§7.6 deleted" | ✅ correct |
| 153 | §1.3: "its home, §7.6, is deleted" | ✅ correct |
| 447 | §5.4: "(§7.6 is deleted)" | ✅ correct |
| 2244 | "RFC 9002 §7.6.1" | ✅ not a slither ref |
| 2298 | §14.6 marker: "deleted with §7.6" | ✅ correct |
| 2572 | §16.4 code comment: "(no rekey swap — §7.6)" | ✅ resolves to the stub, which names the deleted swap |
| 2628 | §16.4 bullet: "the deleted rekey swap's value (§7.6)" | ✅ same |

Every one is a *deletion* reference; none implies §7.6 still specifies
behaviour. The stub itself (L1158–1166) names everything it removed and
states why the number is retained.

**Constants ↔ table.** Every all-caps backticked identifier in the body
(lines 1–3212) reconciled against the consolidated table (L3215–3259).
Body-only residue, all benign:

- `REKEY_AGE`, `REJECT_AGE` — only in the §7.5 moot note and §7.6 stub ✅
- `RECV_BUFFER` — §10.6's historical reference to the superseded draft ✅
- `TAG`, `WINDOW` — formula-local variables defined in place (§2.3, §10.3) ✅
- `INITIAL_MAX_STREAMS_UNI` (L1777) — the table abbreviates the pair as
  `INITIAL_MAX_STREAMS_BIDI` / `_UNI` (L3234) — see FIDV4-12
- `PROLOGUE` "table-only" is a regex artefact of the `PROLOGUE = b"…"` form
  (L92, L400)

**Table-only residue: none.** No deleted constant survives in the table or
in Appendix B: `HS_FLAGS_LEN`, `MSG2_PAYLOAD_LEN`, `FLAG_CONTINUATION`,
`FLAG_CONTINUED`, `REKEY_AGE`/`REJECT_AGE`, `INITIATIONS_PER_SECOND` rows
are all removed (diff `@@ -3315,17 +3214,15 @@` and `@@ -3354,9 +3251,7 @@`).

**Value agreement (spot-verified against every body home):**
`VERSION` 0x01 (L91, L248, L3217) · `PROLOGUE` `b"slither\x01"` (L92, L400,
L3218) · `IK_MSG1_LEN` 174 (L220, L269) · `IK_MSG2_LEN` 81 (L221, L282) ·
`INIT_PACKET_LEN` 196 (L222, L265, L635, L929, L981, L3225) ·
`RESP_PACKET_LEN` 107 (L223, L278, L929, L981, L3225) ·
`TIMESTAMP_LEN`/`MSG1_PAYLOAD_LEN` 12/12 (L417–418, L216, L3223) ·
`MAX_DATAGRAM`/`MAX_PLAINTEXT` 1200/1170 (L329–330, L3227). All agree.

---

## 6. Arithmetic, recomputed independently

All from the spec's own layout definitions (§2.3, §3.2–§3.5), **not** copied
from the surgery notes. Reference suite: P-256 `PK` = 65, ChaCha20-Poly1305
`TAG` = 16.

```
msg1 payload   = ts_secs(8) + ts_nanos(4)                      = 12  B   ✓ §5.2
msg2 payload   = ∅                                             =  0  B   ✓ §5.2

IK_MSG1_LEN    = PK + (PK+TAG) + (MSG1_PAYLOAD_LEN+TAG)
               = 65 + (65+16)  + (12+16)
               = 65 +  81      +  28                           = 174 B   ✓ L220
IK_MSG2_LEN    = PK + TAG                = 65 + 16             =  81 B   ✓ L221
INIT_PACKET_LEN= INIT_HEADER_LEN + IK_MSG1_LEN + MAC1_LEN
               =  6 + 174 + 16                                 = 196 B   ✓ L222
RESP_PACKET_LEN= RESP_HEADER_LEN + IK_MSG2_LEN + MAC1_LEN
               = 10 +  81 + 16                                 = 107 B   ✓ L223
```

All four claimed values (174/81/196/107) are **exact**.

```
§7.3 / §6.9 amplification ratio = 107 / 196 = 0.5459 < 1                  ✓ L981
   (§7.3's separate 3× AMPLIFICATION_FACTOR budget is an independent cap)

§3.5 MAX_PLAINTEXT = MAX_DATAGRAM − DATA_HEADER_LEN − TAG
                   = 1200 − 14 − 16 = 1170                                ✓ L330
     per-packet overhead = 14 + 16 = 30                                   ✓ L333
     empty-plaintext keepalive datagram = 14 + 16 = 30                    ✓ L318
§8.6 per-seal bound = MAX_PLAINTEXT + 16 = 1186 ≪ 65 535                  ✓ L1467, L178
§11.2 MAX_DATAGRAM_PAYLOAD = MAX_PLAINTEXT − 1 = 1169                     ✓ L1928
```

```
§7.9 usable counter space = 2⁶⁴ − 2 = 18 446 744 073 709 551 614 ≈ 1.845×10¹⁹
     at 10⁷ pkt/s  → 1.845×10¹² s ÷ 3.15576×10⁷ s/yr = 58 454 yr
     spec: "≈ 1.8 × 10¹⁹ … ≈ 1.8 × 10¹² seconds, over 58 000 years"       ✓ L1223-25
     varint ACK cap = 2⁶² − 1 = 4 611 686 018 427 387 903 ≈ 4.61×10¹⁸
     at 10⁷ pkt/s  → 4.61×10¹¹ s = 14 613 yr
     spec §7.9 "over 14 000 years"; §8.1 identical                        ✓ L1228, L1253-54
     (the surgery notes' "14 600 years" is the sharper figure; the spec's
      "over 14 000" is correct and consistent in both homes)

per-epoch key volume = REKEY_EPOCH_MSGS × max seal
                     = 65 536 × 1 186 B = 77 725 696 B = 77.7 MB ≈ 78 MB  ✓ L1226
     (the work order's "~76 MB" was low; the spec's ≈78 MB is right, and
      correctly stated as an upper bound via "≤ 1 186 B")
```

```
§6.3 intro entry ≈ 196 raw + SocketAddr + park metadata ≈ 220 B           ✓ L636
     1024 × 220 B = 225 280 B ≈ 225 KB                                    ✓ L636, L2871
     queue fill needs ≥ INTRO_QUEUE_CAP / INTRO_MAX_PER_SOURCE
                       = 1024 / 4 = 256 distinct sources                  ✓ L728-29
     sustained occupancy = 1024 / 15 s = 68.3 pkt/s ≈ 68                  ✓ L731
§17.1 orphan flush ≈ 1024 chains × 2 DH (es+ss) = 2048 DH                 ✓ L2812
§17.5 datagram queues = 2 × 64 × 1169 B = 149 632 B = 146.1 KiB ≈146 KiB  ✓ L2874
§11.3 per queue = 64 × 1169 = 74 816 B = 73.06 KiB ≈ 73 KiB               ✓ L1943
§7.2 replay window = [u64;32] = 2048 bits = 256 B                         ✓ L1011
     2048 pkt ÷ (1 Gbps / 9600 bit-per-1200B-pkt) = 19.7 ms ≈ 20 ms       ✓ L1024
     ×10 at 100 Mbps = 197 ms ≈ 200 ms                                    ✓ L1025
§10.6 1 MiB ÷ 2 = 524 288 ranges ≈ "~512 000"                             ✓ L1869-70
      bitmap for a 1 MiB span = 1 048 576 / 8 = 131 072 B = 128 KiB       ✓ L1878
§12.2 alternating 2048-bit worst case = 1024 pairs × ≥2 B = ≥2048 B > 1170 ✓ L1994
§14.2 INITIAL_WINDOW = min(10×1200, max(2×1200, 14 720)) = min(12 000, 14 720)
                     = 12 000 B; MINIMUM_WINDOW = 2×1200 = 2 400 B        ✓ L2210-11
§16.10 timer family 5 s/10 s/15 s/25 ms/90 s — matches RETRANSMIT_BASE,
       KEEPALIVE_TIMEOUT, DEAD_TIMEOUT, MAX_ACK_DELAY, HANDSHAKE_GIVEUP;
       120 s/180 s correctly dropped with §7.6                            ✓ L2770
```

**No arithmetic error found anywhere in the revised spec.**

---

## 7. Findings

### FIDV4-1 — MAJOR — §1.1's accepted note does not implement the rulings log's J-1 correction

**Evidence.** `SPEC-v2-DRAFT.md:104–112`:

> its mac1 is keyed over a different static encoding (the 33-byte
> compressed SEC1 form; this wire keys over the 65-byte canonical
> uncompressed form — §2.4, §4.4), so **every** cross-wire handshake packet
> dies at the mac1 gate as a silent drop before any DH, and **no session can
> ever form between the two wires**.

`rulings.md:240–251` (the post-surgery correction, the last thing in the
work order) directs the note to state **three** things: (a) the mac1
differentiator holds **for P-256 suites only**; (b) for a suite whose
canonical encoding already equals the old `Packed` encoding the handshake
may be **byte-compatible**, so a session *can* form; and (c) in that case
divergence surfaces only at the frame layer inside the seal — old
Leg-1/Leg-2 grammar vs the unified frame layer ⇒ post-AEAD structural
failure ⇒ §8.2 signalled death.

The spec implements the `batch-surgery-notes.md` version of J-1 (which
correctly killed the false size claim) but **not** the log's corrected
version: it carries neither the P-256 scoping nor the frame-layer path.
The claim is also internally unscoped against §2.4's own text
("for X25519, the raw 32 bytes"), under which the two encodings coincide and
mac1 would pass.

**Independent verification — this resolves the maintainer's open question.**
The old wire is **single-suite P-256** (`SPEC.md:19–24`: "Curve | P-256
(secp256r1)", `Noise_IK_P256_ChaChaPoly_BLAKE2b`) and keys mac1 over the
33-byte compressed form (`SPEC.md:28`, `SPEC.md:123`, `SPEC.md:131`). Its
sizes are byte-for-byte the post-surgery sizes (`SPEC.md:60, 74, 104–107`:
174 / 81 / 196 / 107) with the same `VERSION 0x01` (`SPEC.md:36`) and
`PROLOGUE b"slither\x01"` (`SPEC.md:146`). Therefore **the log's X25519
branch is unreachable against any real old-wire binary** — every old dev
binary is P-256, its mac1 keying differs, and the spec's conclusion
("no session can ever form") is *true today*. The defect is that the
sentence states a suite-specific mechanism as a universal one, so it becomes
false the instant a non-P-256 suite is configured against a hypothetical
old-wire peer, and it does not record the reasoning the maintainer asked to
see recorded.

**Minimal fix** (§1.1, two clauses): scope the mechanism and record the
fallback —

> …its mac1 is keyed over a different static encoding **for the P-256
> suite** (the 33-byte compressed SEC1 form; this wire keys over the 65-byte
> canonical uncompressed form — §2.4, §4.4), so every cross-wire handshake
> packet dies at the mac1 gate as a silent drop before any DH. **The old
> wire is P-256-only (`SPEC.md` §1), so this covers every old binary that
> exists. For a suite whose canonical encoding already matched (X25519's
> raw 32 bytes, §2.4) the handshake would be byte-compatible and divergence
> would surface only inside the seal — the old Leg-1/Leg-2 grammar against
> this wire's unified frame layer ⇒ a post-AEAD structural failure ⇒ §8.2's
> signalled death.** Accepted, because nothing was ever released…

**Disposition:** editorial once the maintainer confirms the recorded
rationale; the conclusion of ruling 1 is unaffected.

---

### FIDV4-2 — MAJOR — J-3: `Install { initial }` contradicts ruling 26's batch note; the field now has exactly one reachable value

**Evidence.** Ruling 26 (`rulings.md:211–212`): "Batch note:
`Install{initial:false}` remains only for the tie-break admission."
The spec does the opposite: `SPEC-v2-DRAFT.md:863–866` admits the tie-break
loser as `Install { initial: true }`, `:904` repeats it in §6.7, and
`:2571–2572` / `:2626–2629` document `initial: false` as *emitted on no
path*, the field "retained in shape only".

**Assessment — the reviser's reading is technically correct, the note is
not.** §6.7 is ratified (ruling 5) and requires the tie-break loser's
`Connecting` to resolve; `ConnEvent::Established` fires on the first install
(`:2606`); `Install { initial: false }` is defined as the *non*-resolving
swap. Applying the note literally would either strand the loser's
`Connecting` for ever or redefine `Established` — inventing a mechanism no
ruling authorises. The deviation is therefore sound.

**But it is not editorially closable**, for two reasons: (i) it contradicts
a recorded ratified note, and (ii) the spec now ships a normative struct
field with exactly one reachable value, which the code must nonetheless
implement. That is a live API-shape decision.

**Minimal fix** — maintainer picks one:
1. **Delete the field.** `struct Install { session: EstablishedSession }`;
   drop the `initial:` mentions at `:2571–2572` and `:2626–2629`, and the
   `(initial: true, exactly once…)` parenthetical at `:2625`. Two-line edit,
   and it removes dead surface permanently.
2. **Keep as-is** and amend the batch note in `rulings.md` to record that
   the tie-break admission is `initial: true` (superseding ruling 26's note).

---

### FIDV4-3 — MAJOR — J-2: initiation pacing deleted with no ruling naming it

**Evidence.** Deleted by the surgery, in five places: the
`INITIATIONS_PER_SECOND` constant (50 / 20 ms spacing per known static), the
§6.6 pacing step, §17.1's pacing-counter sentence, §6.7's "guard, flag
routing, and pacing" clause, §6.9's pacing parentheticals, the Appendix B
pacing test, and the constants-table row (diff `@@ -790,121 +797,78 @@`,
`@@ -2928,9 +2830,6 @@`, `@@ -3354,9 +3251,7 @@`). Grep for
`INITIATIONS_PER_SECOND` in the revised spec: **zero hits**.
`rulings.md` never mentions pacing.

**Assessment.** The reviser's reasoning holds structurally: pacing's only
home was the internal continuation's *non-application-gated* replacement
admission, which rulings 4 + 11 delete outright, and the spec's own
justification ("Fresh accepts are already gated by the application") now
covers replacements too (§6 intro `:560–562`, §6.4 `:778–789`). Re-homing
pacing to `accept()` would have required a new `AcceptError` variant — an
invention.

**Residual exposure the maintainer should weigh.** With pacing gone, the
protocol places **no rate bound on replacement admissions**. A compromised
or buggy key-holding peer that out-paces an auto-accepting application can
churn the connection arbitrarily fast; each churn is a full teardown
(`Replaced`) plus a fresh connection (fresh streams, credit, recovery,
counters — §7.8). The old 20 ms gate bounded exactly this. The spec is
silent on it — §6.9's replacement row (`:939`) says only "replacement
admission is application-gated", which is a *policy* answer to a *rate*
question.

**Minimal fix** — maintainer picks one:
1. **Accept the deletion** and add one sentence to §6.4 recording it: the
   protocol imposes no churn bound on replacing accepts; churn protection
   against a compromised key-holder is application accept-policy.
2. **Re-home the gate**: restore `INITIATIONS_PER_SECOND` (50 / 20 ms per
   static, counter in the guard entry §17.1) as a bar on *replacing*
   accepts, with the over-rate accept returning `AcceptError::Stale`
   (reusing the existing variant — no new error, no invention).

---

### FIDV4-4 — MINOR — the preamble's round-1 cluster A/F text was annotated, not rewritten, as ruling 4 required

**Evidence.** `rulings.md:109–110`: "Appendix B test obligations **and the
preamble's cluster A/F summaries** need matching rewrites." Appendix B was
rewritten (verified above). The cluster block was not: `:23–27` still
describes CONTINUATION/CONTINUED flag routing, "CONTINUED computed (never
hardcoded)" and "§6.4's candidate-flag discard deleted" as design, and
`:39–41` (cluster F) still reads "idle sessions rekeying via the keepalive
(§7.4–7.6)" — a mechanism that no longer exists, pointing at §7.6 which is
now a deletion stub. The only mitigation is the parenthetical at `:21–22`,
"clusters A and F are **partially superseded** by the draft-v4 ratchet-only
ruling below".

The reviser recorded this deliberately (`batch-surgery-notes.md`, unresolved
item 3: "kept deliberately as history"). Preserving the historical record is
defensible, and the annotation is honest — but it is a documented deviation
from an explicit batch-edit implication, and cluster F's `§7.4–7.6` pointer
now dangles semantically.

**Minimal fix.** Append a bracketed strike to each superseded clause rather
than rewriting the history, e.g. cluster A: "…(§5.4, §6.3–6.8) **— the flag
machinery of this cluster is deleted by the draft-v4 ratchet-only ruling;
its LIVE/PENDING/NONE routing, deferred `Replaced` teardown and frozen
mid-state entries survive**", and cluster F: "…idle sessions rekeying via
the keepalive (§7.4–7.6) **— the idle-rekey half is deleted; the liveness
anchor and the keepalive floor survive (§7.4–7.5)**".

---

### FIDV4-5 — MINOR — deleting `AcceptError::AlreadyConnected` leaves §16.1's PENDING half of the invariant with no expression

**Evidence.** §16.1 (`:2437–2445`) asserts "one connection per remote
static" and §5.4 (`:466–467`) states "§16.1 forbids both at once" (a live
connection *and* an in-flight outbound pending). `connect()` enforces its
half (`ConnectError::AlreadyConnected`, `:2439–2440`). `accept()` no longer
enforces anything: §6.4's guard bullet (`:778–789`) branches only on LIVE,
and §18.1 (`:2901–2906`) is "closed and normative" with
`AcceptError::{Stale, EndpointDropped}`.

The reachable ordering: `read_identity()` on an `Intro` for static X (X is
NONE, so no interception); application calls `connect(X)` — permitted, X is
neither LIVE nor PENDING at that instant; X becomes PENDING; application
then calls `authenticate()` → `Proven`, then `accept()` → §6.4 sees no LIVE
connection, so it is an ordinary fresh accept and installs. The endpoint now
holds a live connection **and** an in-flight outbound pending for X.

This gap is **pre-existing** — the frozen draft's `AcceptError::AlreadyConnected`
also branched on LIVE only (diff `@@ -2992,25 +2898,23 @@`), so the surgery
did not introduce it. J-5's deletion is the correct reading of ruling 11
("`AlreadyConnected` becomes `connect()`-only") and consistent with the
`Superseded` precedent. But the closed taxonomy now has no variant that
could express the collision, so the fix is no longer free.

**Minimal fix.** One sentence in §6.4's guard bullet: an `accept()` whose
proven static has an in-flight outbound pending **cancels that pending**
(index dropped, `Connecting` resolved by the installing accept, exactly as
in §6.7's loser path) — no new error variant needed. Flag for the focused
§§5–7 re-review.

---

### FIDV4-6 — MINOR — `ConnEvent::AddressMoved` now has no described consumer

**Evidence.** `:2615` declares it; nothing else in the document mentions it.
§7.3's observability sentence lost its core-event clause in the surgery
(pre-surgery: "at the core level the connection emits
`ToEndpoint::AddressMoved` so the endpoint's hint map and rekey targeting
stay fresh (§16.4)") and now reads (`:1047–1049`) "Observability is the
`remote_address()` accessor plus the `slither::roam` trace target (§18.2)"
— naming the shell surfaces but not the core event that must feed them.
Deleting the sentence was correct (its subject was the deleted
`ToEndpoint` variant), but it left the surviving `ConnEvent` variant
orphaned. §16.8's "shared cell the driver updates" is the implicit link.

**Minimal fix.** §7.3, one clause: "…plus the `slither::roam` trace target
(§18.2); at the core level the connection emits `ConnEvent::AddressMoved`,
which is what updates `remote_address()` and fires the trace (§16.4,
§16.8)."

---

### FIDV4-7 — MINOR — J-6: `session_id()` / `SessionId` retained but now constant per connection

**Evidence.** `:2476` `pub fn session_id(&self) -> SessionId;`. With
sessions 1:1 to connections for life (§5.4, §7.8), the accessor returns a
value that never changes and carries no information a `ConnectionId` would
not. `SessionId` is defined nowhere else in the document. No ruling names
it; the reviser flagged it as a candidate simplification and left it.

Consistent with the work order (nothing mandated it), but it is now dead
API surface that the code must implement. Worth a decision at the §§5–7
re-review: keep as an opaque connection-generation token (useful for
operator correlation across a `Replaced`), or drop it.

---

### FIDV4-8 — NOTE — "the v1 shell" / "the one v1 implementation" now collide with "wire version 1"

`:2202` "NewReno is the one v1 implementation" and `:2322` "no sub-RTT
wakeups in the v1 shell". Both were written against "wire version 2", where
"v1" unambiguously meant the first *implementation* release. Under ruling 1
the wire is now version 1, so "v1" is ambiguous. Ruling 1 asked for version
phrasing to be retitled across the document; the reviser recorded this as
unresolved item 4 ("they now happen to agree"). Harmless but avoidable.
**Fix:** "the first shipped implementation" / "the first shell".

### FIDV4-9 — NOTE — `ConnEvent::Established` comment "first install" is vacuous

`:2606` "// first install; the shell resolves Connecting". With exactly one
install ever (§7.8, and `Install{initial:true}` "exactly once", `:2625`),
"first" no longer distinguishes anything. Reviser's unresolved item 5;
touching it ripples into §16.4's contract text. Cosmetic. Fold into whatever
resolution FIDV4-2 takes.

### FIDV4-10 — NOTE — marker-count discrepancy is a grep artefact, not a defect

Recorded here for the audit trail: 28 clause-level markers exist; see §1
Lead B for the full reconciliation. No spec edit needed. If the count is
scripted in CI, match on `\[RATIFIED` across a joined document and subtract
the §1.3 meta-mention, or reflow §1.1's and §16.1's markers onto one line.

### FIDV4-11 — NOTE — two ruling-4 items are recorded thinly

(a) **Linkability across roams.** Ruling 4's accepted-losses list names two:
no post-compromise healing, and "wire-visible indices never rotate for the
connection's life (linkability across roams)". The first is recorded
explicitly and well (§7.7 `:1186–1191`). The second is only *implied* — §7.1
(`:1001–1005`) says the counter space runs for the connection's life and
§17.3 says indices are re-drawn per connection, but no sentence states the
privacy consequence that a roaming peer is linkable by its stable
`receiver_index` across paths. §19's "header protection" and
"packet-number truncation" rows are the deferral homes.
**Fix:** one sentence in §7.3 or §19's header-protection row.

(b) **§5.6 lists two guard admission points, §17.1 lists three.** §5.6
(`:528–531`) names `authenticate()` and the tie-break's admit step; §17.1
(`:2781–2785`) additionally names "a re-homed `accept()`'s candidate
admission (§6.4)", which §6.4 (`:773–774`) confirms is a check-and-record
site. Pre-existing (the frozen text had the same two-vs-three split), not
surgery drift. **Fix:** add ", at a re-homed `accept()`'s candidate
admission," to §5.6.

### FIDV4-12 — NOTE — cosmetic constant-table/body naming mismatches

`INITIAL_MAX_STREAMS_UNI` is spelled in full in the body (`:1777`) but the
table abbreviates the pair as `INITIAL_MAX_STREAMS_BIDI` / `_UNI`
(`:3234`). Likewise Appendix B's wire pins (`:3061`) rely on the generic
"every §3/§2.3 length" clause rather than naming 174/81/196/107 explicitly;
this is adequate (§2.3's table is the authority) but a reader auditing the
pins must follow the pointer. Neither is a conformance defect.

---

## 8. Judgment calls J-1…J-10 — dispositions

| J | Call | Consistent with the log? | Spec text coherent? | Disposition |
|---|---|---|---|---|
| **J-1** | Accepted-note rationale = mac1 keying, not sizes | **Partially** — implements the reviser's version, not the log's post-surgery correction (P-256 scoping + frame-layer path) | Yes, but the claim is stated universally when the mechanism is suite-specific | **MAJOR — FIDV4-1.** My independent check of `SPEC.md` (P-256-only) shows the log's X25519 premise is unreachable, so the conclusion stands; the wording needs scoping |
| **J-2** | Pacing deleted entirely | **No ruling names pacing** — deletion is inferred from rulings 4 + 11 | Yes — no dangling reference survives | **MAJOR — FIDV4-3.** Reasoning is sound; the resulting *absence of any replacement-churn bound* is a policy gap needing a ruling |
| **J-3** | Tie-break admits at `initial: true`; field kept, shape-only | **Contradicts ruling 26's batch note** | Yes, and the note's literal reading would break ratified §6.7 | **MAJOR — FIDV4-2.** Reviser is right on the mechanics; maintainer must choose delete-field vs amend-note |
| **J-4** | `ToEndpoint::AddressMoved` deleted | Not mandated, but both consumers are deleted by rulings 4 + 11 | Yes — `ToEndpoint` correctly carries `Retired` alone; `ConnEvent::AddressMoved` correctly untouched | **Accept editorially.** Verified against the frozen text. One nit: FIDV4-6 |
| **J-5** | `AcceptError::AlreadyConnected` deleted, not retained | **Yes** — ruling 11's "connect()-only", plus the `Superseded` precedent | Yes — §6.4, §18.1, §16.1, Appendix B all agree | **Accept editorially.** Surfaces a pre-existing PENDING-half gap: FIDV4-5 |
| **J-6** | `session_id()`/`SessionId` kept | No ruling names it | Yes, but the accessor is now constant | **Accept editorially**, decide at the re-review: FIDV4-7 |
| **J-7** | §13.3 probe train ended by liveness alone | **Yes** — `REJECT_AGE` died with §7.6; the §7.4 receive-keyed anchor genuinely covers the asymmetric case | Yes — `:2119–2122` cites §7.4 correctly; a peer whose ACKs never arrive stops resetting `last_authenticated_recv`, so `DEAD_TIMEOUT` fires | **Accept editorially.** The reasoning is the reviser's but it is correct and load-bearing-free |
| **J-8** | Replacements share the intro queue | **Yes** — a direct consequence of ruling 11's staged-everything | Yes, and stated honestly in §6.3's honesty clause (`:733–736`) and §6.9's replacement row (`:939`) | **Accept editorially**, but this is the sharpest *new* exposure class in the revision (a sustained intro-queue flood can now delay a legitimate replacement; previously immune). Cookies/mac2 (§19) is the named remedy. **Flag prominently for the §§5–7 re-review** |
| **J-9** | §5.4 responder-rule evaluation point wording | **Yes** — refines "resolution at install time" into two concrete post-`ss` points | Yes — §5.4 `:461–464`, §6.6 step 2 `:852–855`, §6.4 `:778` are mutually consistent; §6.7 `:886–890` correctly notes the `es`-time match "decides nothing" | **Accept editorially** |
| **J-10** | Intro entry kept at ≈ 220 B | **Yes** — 196 raw + addr + metadata; the msg1 shrank 1 B, well inside the rounding | Yes — §6.3 `:636` and §17.5 `:2871` agree; 1024 × 220 = 225 KB ✓ | **Accept editorially** |

---

## 9. Appendix A / Appendix B / preamble

| Required | Status | Evidence |
|---|---|---|
| A.1 flag **intact and pending** (ruling 30 — must NOT be dropped) | ✅ | `[MAINTAINER]` at `:2993`; recorded pending in §1.3 `:154–157` and the preamble `:13–15` |
| A.1 payload references updated to 12 B / timestamp-only | ✅ | `:3005–3007` "the decrypted 12-byte msg1 payload … where §5.2's timestamp comes from" (was "13-byte … timestamp and flags") |
| A.1 substance otherwise untouched (the no-fallback rejection of `read_message_1_with`) | ✅ | `:2982–3001` unchanged |
| A.2 intact | ✅ | `:3009–3019` — no diff hunk touches it |
| A.3 retouched: doc-only, `where` clause **permanent**, `Ord` **dropped** | ✅ | `:3021–3034`. Mirrored at §2.4 `:234–237` ("`Ord` is **not** required") and §6.7 `:880–884` ("needs no `Ord` bound"). The frozen text's "`AsRef<[u8]> + Ord`… a hiss semver ruling" is fully replaced |
| msg2-payload **non-item** rewritten as no-longer-needed | ✅ | `:3036–3043` "Non-item, no longer needed — the msg2 payload"; the old "declaring `[13]`/`[1]` … 174 → 175, 81 → 82" arithmetic is gone |
| `SymmetricState::drop` wart kept | ✅ | `:3045–3049` |
| Appendix A intro reworded (A.1 the one hard gate; A.2 interim; A.3 doc-only) + hiss reconciliation note | ✅ | `:2974–2980` |
| Appendix B: **no tests for deleted machinery** | ✅ | pacing test gone; continuation state matrix → local-state routing (`:3079–3087`); rekey survival matrix → one-session-per-connection (`:3165–3168`); idle-rekey liveness bullet → "no handshake ever re-run" (`:3180`); `AcceptError::AlreadyConnected` test → `ConnectError::AlreadyConnected` only (`:3096–3097`); freeze-on-carry scoped to eager-demoted (`:3072–3075`); tie-break bullet loses "stable across rekey" (`:3090–3091`); watermark "for the connection's life" (`:3118–3119`). Grep of `:3051–3212` for pacing / continuation / rekey: **zero** |
| Appendix B: **new sizes pinned** | ✅ | `:3058–3062` golden vectors "frozen **for the first time** at wire version 1 (the prologue, the 12-byte msg1 payload, mac1's canonical keying)"; "Compile-time size/constant asserts for every §3/§2.3 length" covers 174/81/196/107 (see FIDV4-12) |
| Preamble v4 block **accurate** | ✅ | `:54–70`. Every claim verified: ratchet-only ✓, §7.6 deleted ✓, §7.8 collapsed ✓, `VERSION = 0x01` ✓, `PROLOGUE = b"slither\x01"` ✓, flags deleted ✓, "msg1 payload is the 12-byte timestamp alone" ✓, "msg2 carries no payload" ✓, "196 B and 107 B" ✓, ratification gated on A.1 + the §§5–7 re-review ✓ |
| Preamble title / supersession framing | ✅ | `:1` "wire version 1 (first release)"; `:5–11` "pre-release drafts … zero authority … nothing released to transition from" |
| Preamble names A.1 **without** the bracketed marker form (keeps the count at 2) | ✅ | `:13–15` |
| Preamble cluster A/F annotation | ⚠️ | `:21–22` annotated but not rewritten — **FIDV4-4** |

---

## 10. What this review did **not** cover

Per the brief, this is a fidelity-and-consistency pass. Still owed, and
explicitly out of scope here:

1. The **focused adversarial re-review of the restructured §§5–7** the
   ruling itself demands (`rulings.md:71`, `:235–238`; preamble `:68–70`).
   Carry into it: J-8's new intro-queue exposure class, FIDV4-5's
   PENDING-half gap, FIDV4-3's unbounded replacement churn, and the
   reviser's unresolved item 1 (the simultaneous-open interception
   dependency, unchanged by the hint-set shrinkage but now the tie-break's
   only application-independent backstop is `HANDSHAKE_GIVEUP`).
2. The **Appendix A.1 joint hiss work** (ruling 30, still PENDING).
3. Whether the ratchet-only design is *correct* — only whether the surgery
   applied it faithfully.

# Batch surgery — change log (draft v3 → v4, 2026/08/14)

Applied to `SPEC-v2-DRAFT.md` in one revision pass, per
`rulings.md` (the work order). Frozen pre-surgery text:
`SPEC-DRAFT-v3-pre-surgery-frozen.md` (untouched).

Marker accounting: the draft carried 33 `[MAINTAINER]` occurrences
(32 substantive + the preamble's marker definition). After surgery:
**28 clause-level `[RATIFIED 2026/08/14]` markers** (§1.1, §4.4, §5.4,
§6.1, §6.3 ×3, §6.4, §6.5, §7.2, §7.3, §7.5, §8.2, §9.1, §9.8 ×2, §10.2,
§10.4, §10.6 ×2, §12.4, §14.6, §15.1, §15.2, §16.1, §16.4, §17.1, §19 —
ruling 15 has two homes, rulings 4+5 share the §5.4 home) and **exactly
2 `[MAINTAINER]` occurrences**: Appendix A.1 (pending, per ruling 30)
and §1.3's mention of it. The preamble no longer uses the bracketed
marker form. The §7.6 swap-cut flag is gone with its section (moot).

---

## Ruling 1 — no version bump (modified)

- Title → "slither — protocol specification, wire version 1 (first
  release)". Preamble reframed: SPEC.md / SPEC-v2.md are "pre-release
  drafts … zero authority", superseded wholesale; no on-wire transition
  because nothing shipped.
- `VERSION` 0x02 → **0x01** (§1.1, §3.1, constants table); `PROLOGUE`
  `b"slither\x02"` → **`b"slither\x01"`** (§1.1, §5.1, constants table).
- §1.1 rewritten as "The first wire": no negotiation (kept verbatim in
  substance), unknown version/type ⇒ silent drop, version bound via
  prologue; marker → RATIFIED.
- "One-time golden regeneration" language → "the first freeze" (§1.3,
  §2.4, §4.4, Appendix B wire pins).
- §1.2 scope line: "the handshake and its continuation flags" → "the
  handshake"; "liveness, rekey" → "liveness, the epoch ratchet".

**⚠ Judgment call / deviation from the log (J-1).** The log's accepted
note read "every packet size and payload length differs ⇒ silent drop".
That was true under the 13 B/1 B payloads, but **ruling 4's flag
deletion falsifies it**: the old SPEC.md wire had msg1 payload = 12-byte
timestamp, `IK_MSG1_LEN` 174, `IK_MSG2_LEN` 81, `INIT_PACKET_LEN` 196,
`RESP_PACKET_LEN` 107 (SPEC.md lines 53, 104–107) — **identical to the
post-surgery sizes** — plus the same `VERSION`/`PROLOGUE`. The one
remaining structural distinction is **mac1's keying encoding** (old:
33-byte compressed SEC1, SPEC.md line 28; new: 65-byte uncompressed
canonical, §2.4/§4.4): every cross-wire handshake packet fails mac1 and
dies as a pre-DH silent drop, so no session can form. §1.1's accepted
note is written on that basis. Needs a maintainer glance, since the
recorded rationale changed even though the conclusion (structural
silent-drop separation, acceptable because nothing shipped) stands.

## Ruling 2 — §1.3 block re-affirmation (procedural)

§1.3 rewritten: golden vectors "freeze for the first time"; the carried
rulings recorded as **individually re-ratified 2026/08/14** with inline
markers; the swap-cut recorded moot (§7.6 deleted); A.1 recorded pending
(joint hiss work) keeping its `[MAINTAINER]` flag; whole-document
ratification gated on A.1 + the follow-up re-review.

## Ruling 3 — §4.4 mac1 suite genericity (ratified)

Marker → RATIFIED. "Absorbed by the one-time golden regeneration" →
first-freeze framing, "no shipped bytes existed". "Old frozen wire" /
"v1's mac1 bytes" rephrased to "pre-release drafts'".

## Rulings 4 + 5 + 11 — RATCHET-ONLY REKEY (the headline restructure)

### Handshake payloads and sizes

- §5.2: msg1 payload = `ts_secs(8) ‖ ts_nanos(4)` (12 B); **msg2 has no
  payload** (tail = empty payload's tag). Deleted: `HS_FLAGS_LEN`,
  `MSG2_PAYLOAD_LEN`, `FLAG_CONTINUATION`, `FLAG_CONTINUED`, and the
  reserved-flag-bits malformedness rule. `MSG1_PAYLOAD_LEN` 13 → 12.
- §2.2: `channel!` declares `[12]` on msg1, nothing on msg2.
- §2.3: `MSG2_LEN = PK + TAG` formula; reference values recomputed
  (arithmetic below). §3.2/§3.3 headings and layouts updated
  (197→196, 108→107, 175→174, 82→81; payload bullets rewritten).
- §5.3: "a wall-clock reading and a continuation bit" → "a wall-clock
  reading".

**Recomputed size arithmetic** (P-256 PK = 65, TAG = 16), from §2.3's
own algebra — not copied from anywhere:

```
IK_MSG1_LEN     = 65 + (65+16) + (12+16) = 65 + 81 + 28   = 174
INIT_PACKET_LEN = 6  + 174 + 16                            = 196
IK_MSG2_LEN     = 65 + (0+16)                              = 81
RESP_PACKET_LEN = 10 + 81  + 16                            = 107
amplification   : 107 / 196 ≈ 0.55 < 1        (§6.9 "No amplification")
intro entry     : 196 raw + SocketAddr + park metadata ≈ 220 B
                  1024 × 220 B ≈ 225 KB       (§6.3, §17.5 — unchanged)
DoS stimulus    : 196 B; msg2 reply 107 B     (§6.9 header)
```

(These coincide with the old SPEC.md wire's numbers — see J-1.)

### §5.4 — rewritten

Now "One session per connection — restart is structural", marked
`[RATIFIED 2026/08/14 — the ratchet-only ruling]` (rulings 4 + 5's
surviving half in one home): no periodic DH re-handshake; ratchet is the
only rekey; sessions:connections 1:1; the restart data-loss bug dead BY
CONSTRUCTION; LIVE ⇒ replacement via staged accept (park → frozen where
mid-state was paid → `Replaced` at `accept()`); PENDING ⇒ tie-break;
NONE ⇒ fresh accept. Flag tables, responder/initiator flag rules, and
the ruling-5 unit paragraph deleted (its surviving members —
LIVE/PENDING/NONE routing, frozen mid-state entries,
teardown-at-accept(), flag-blind re-home — live on in §5.4/§6.3–§6.6).

### §5.5–§5.7

- §5.5: 12-byte payload; "dialled (or current, for a rekey)" → dialled;
  CONTINUED-check step deleted; step 6 rekey-give-up text replaced (every
  initiation is a `connect()`).
- §5.6: staged accept + the one internal exception (tie-break); guard
  admission points updated; "timestamp and flags" → "timestamp".
- §5.7: `REKEY_AGE`/`REJECT_AGE` rows deleted; the no-message-count note
  rewritten ("no DH-rekey trigger at all — the ratchet is the only
  rekey").

### §6 (with ruling 11)

- §6 intro requirements rewritten: the "must not see a rekey"
  requirement dissolved; replacement is an application decision at
  `accept()`; the rest carried.
- §6.1: `accept()` row loses "CONTINUED = 0"; dagger note loses
  "restart-parked"; "timestamp and flags" → "timestamp".
- §6.3: 196-byte raw entry; "replacement initiations bypass this queue"
  → **replacements park like any other** (dedup + evict-oldest give the
  genuine peer the per-packet race); freeze-on-carry reduced to
  eager-demoted only; "restart-replacement injection surface" →
  "identity-straddling injection surface"; honesty clause updated (the
  denial now covers replacements; the frozen-slot spoof surface scoped
  to "the dialled address of an in-flight outbound connect", open only
  while that dial is in flight). Ruling 8's dedup marker gains the
  walkthrough's mobility note (stage-0-only key, convergence recovery,
  roaming owns post-establishment mobility).
- §6.4: CONTINUATION-candidate paragraph deleted (ruling 10's batch
  edit); "msg2 (CONTINUED = 0)" → "msg2"; the one-teardown-path guard
  bullet rewritten as "**a guard-passing proven-LIVE admission
  replaces**" with the strictly-greater guard named as the stale/replay
  bar; `AcceptError::AlreadyConnected` deleted as unreachable.
- §6.5: hint set = **dialled addresses of in-flight outbound connects
  only**; eager path routes claimed ∈ pending-outbound-remotes to the
  tie-break, demotes everything else; interception scoped to pending
  remotes; the rekey-accuracy paragraph replaced by the
  simultaneous-open rationale + interception self-heal; oracle
  restatement reduced (probed set = pending remotes only) and marked
  RATIFIED-in-reduced-form.
- §6.6: rewritten as "The internal tie-break completion" — tag → guard
  → tie-break → admit (`Install { initial: true }`); LIVE/NONE
  unreachable (routed to the staged path); the local-state routing and
  computed-CONTINUED steps are gone; **pacing deleted (J-2)**.
- §6.7: comparison de-`Ord`ed (equal-length `as_ref()` octets, per
  ruling 3/A.3 retouch); "guard, flag routing, and pacing" → "guard";
  the CONTINUED-loser sentence deleted; stream-parity note loses the
  rekey clause.
- §6.8: rewritten — restart is a replacement/fresh accept plus liveness;
  no restart machinery.
- §6.9: stimulus/reply sizes 196/107; hint-set rows scoped to in-flight
  dials; "forged claim of a known static" → "of a pending-outbound
  static" (other statics are application-chosen staged spends); replay
  row scoped to the tie-break window, pacing parentheticals dropped;
  replacement row now application-gated; rate-honesty note scoped to
  the dial's lifetime; "pacing-free" dropped from the re-home costing.

### §7

- §7.1: rewritten — one counter space per direction for the connection's
  life; no restart-at-rekey, no make-before-break. §2.1's per-session
  table row updated to match.
- §7.3: "hint map and rekey targeting stay fresh" sentence deleted (see
  J-4); "a fourth reset seam" → "a second reset seam" (here and §19),
  since roam is now the only reset seam.
- §7.5: keepalive no longer consults `REKEY_AGE`; ruling 14's marker →
  RATIFIED with the floor justification kept and the idle-rekey half
  recorded moot.
- **§7.6 deleted** — stub left ("[deleted 2026/08/14 — the ratchet-only
  ruling]") naming everything it removed, to avoid a §7 renumbering
  cascade. Swap-cut flag moot.
- §7.7: the post-compromise cross-reference replaced with the honest
  statement: forward rotation only; **no post-compromise healing within
  a connection**; healing = application reconnect (TLS 1.3 KeyUpdate
  model; WireGuard's contrary choice noted and overridden by ruling).
- §7.8: collapsed to "One session per connection — nothing survives a
  handshake"; survival matrix and re-queue rule deleted; un-ACKed data
  at a replacement is the application's to re-send.
- §7.9: rewritten — no rekey escape; exhaustion ⇒ connection death.
  **Exhaustion arithmetic** (shown in-spec): usable space
  2⁶⁴ − 2 ≈ 1.845 × 10¹⁹ seals; at 10⁷ pkt/s ≈ 1.845 × 10¹² s ≈
  **58 000+ years**. Varint ACK cap 2⁶² − 1 ≈ 4.61 × 10¹⁸ ≈ 14 600
  years at the same rate (§8.1 note updated to match). Per-epoch-key
  volume: 65 536 × 1 186 B = 77 725 696 B ≈ **78 MB** (the log's
  "~76 MB" was slightly low; spec says "≈ 78 MB").

### §§8–14

- §8.1: "REKEY_AGE forces a fresh counter space" → the §7.9 bound.
- §9.1: role-stability rekey sentence → "parity fixed at establishment"
  (the walkthrough's §9.1 batch note).
- §9.2: watermark "survives rekey" → "lives for the connection's life".
- §10.7: retitled "Exemptions"; the rekey-seam bullet deleted.
- §11.5: the across-rekey datagram sentence deleted.
- §13 intro: "per session … resets at rekey" → per connection.
- §13.1: RTT estimator survives **roaming** (only).
- §13.3: probe train ended by liveness alone (see J-7).
- §13.6: retitled "the roam seam"; the rekey paragraph deleted; roam
  text untouched.
- §14.6: retitled "The roam reset seam"; rekey bullet deleted; roam
  bullet **verbatim** (ruling 24: roam seam stands exactly as
  ratified); marker → RATIFIED-in-reduced-form.

### §§15–18

- §15.4 teardown matrix: `RekeyFailed` and `PeerRestarted` rows deleted;
  the replaced row rewritten flag-free (peer's view: "it reconnected").
- §16.1: marker → RATIFIED with the **clause flipped** per ruling 26:
  inbound proven-LIVE ⇒ "a replacement of that connection, admitted via
  `accept()` — never a second concurrent connection". Ownership table:
  "hint map + internal rekey continuation" → "hint set + the internal
  tie-break"; Connection owns "liveness, the epoch ratchet".
- §16.4: `NeedsRekey` deleted from `ToEndpoint`;
  `ToEndpoint::AddressMoved` deleted (J-4); `Install` comment and
  bullet updated — `initial: true` for msg2 completion and the
  tie-break admission; `initial: false` emitted on no path, field
  retained in shape only (J-3). `EndpointOutput::Transmit` comment:
  "continuation msg2" → "tie-break msg2".
- §16.5: the REKEY_AGE/REJECT_AGE not-timers sentence deleted.
- §16.10: timer family "5 s/10 s/15 s/25 ms/90 s" (120 s/180 s dropped).
- §17.1: admission points updated (tie-break step 4); a sentence added
  naming the guard as what makes proven-LIVE replacement safe (ruling
  11's "the timestamp guard blocks stale/replayed candidates");
  honesty-clause parenthetical updated; the pacing-counter sentence
  deleted (J-2); marker → RATIFIED.
- §17.4: rewritten — static map + pending dialled addresses; no
  connection-derived hint map (J-4).
- §18.1: `AcceptError::{Stale, EndpointDropped}` (AlreadyConnected
  deleted, recorded in the intro alongside `Superseded`);
  `ConnectionLost` loses `RekeyFailed` and `PeerRestarted`; `Replaced`
  re-described; `IntroError::Internal` re-described (pending outbound
  remote / simultaneous open).
- §18.2: `slither::policy` row — "pacing rejections" and "restart
  routings" dropped; "internal tie-break outcomes".

### §19, appendices, constants

- §19: marker → RATIFIED; PATH_CHALLENGE row "a second reset seam
  alongside the roam seam"; persistence row's §5.4 pointer reworded.
- Appendix A: intro reworded (A.1 the one hard gate; A.2 interim; A.3
  doc-only) + the reconciliation note ("reconciles against the shipped
  hiss API when the in-flight hiss work lands"); A.1 keeps its
  `[MAINTAINER]` flag, its payload references updated to 12 B /
  timestamp-only; A.2 untouched; A.3 retouched per the work order
  (slither-side `where C::PublicKey: AsRef<[u8]>` is the PERMANENT
  mechanism; `Ord` NOT required — §6.7 compares equal-length `as_ref()`
  octets; the hiss ask reduces to a doc-only canonical-encoding
  stability promise); the msg2 `[1]` non-item rewritten as "no longer
  needed" (payload deleted entirely); the `SymmetricState::drop` wart
  kept.
- Appendix B: golden vectors "frozen for the first time"; the
  continuation state matrix replaced by the local-state routing +
  restart-end-to-end obligations; tie-break bullet loses "stable across
  rekey"; re-home bullet rewritten (proven-LIVE replacement;
  `ConnectError::AlreadyConnected` only); pacing test deleted (J-2);
  freeze-on-carry reduced to eager-demoted; watermark "for the
  connection's life"; the rekey survival-matrix bullet replaced by the
  one-session-per-connection obligation; the idle-rekey liveness bullet
  replaced ("an idle keepalive-sustained connection lives indefinitely
  with no handshake ever re-run").
- Constants table: `VERSION` 0x01; `PROLOGUE` `b"slither\x01"`;
  `TIMESTAMP_LEN`/`MSG1_PAYLOAD_LEN` 12/12 (msg2-has-no-payload note);
  `HS_FLAGS_LEN`, `MSG2_PAYLOAD_LEN`, `FLAG_*` rows deleted;
  `IK_MSG1_LEN`/`IK_MSG2_LEN` 174/81; `INIT`/`RESP_PACKET_LEN` 196/107;
  `REKEY_AGE`/`REJECT_AGE` row deleted; `INITIATIONS_PER_SECOND` row
  deleted (J-2).

### Preamble

Retitled draft v4; a "Walkthrough revisions (draft v3 → v4,
2026/08/14)" block added summarising ratchet-only, no-bump, and the
flag deletion with the new reference sizes; the round-1 cluster block
annotated "clusters A and F are partially superseded" (the historical
text itself is preserved as history, not rewritten); the header's
maintainer-flag sentence now names A.1 without using the bracketed
marker form (so the marker count stays exactly 2).

## Rulings applied as marker flips only (text already ratified as-is)

6 (§6.1 names), 7 (§6.3 flood posture), 9 (§6.3 `Superseded`),
12 (§7.2 fused ACK record), 13 (§7.3 budget), 15 (§8.2 + §15.2, twin
homes), 16 (§9.1 four spaces), 17 (§9.8 sugar), 18 (§9.8 overflow),
19 (§10.2 initial windows), 20 (§10.4 cumulative MAX_STREAMS),
21 (§10.6 credit-as-buffer), 22 (§10.6 reassembly bound), 23 (§12.4
delayed ACK), 25 (§15.1 minimal CLOSE), 27 (§16.4 two cores),
28 (§17.1 guard eviction), 29 (§19 deferral block).

---

## Judgment calls — for maintainer review

- **J-1 (accepted-note rationale changed).** See ruling 1 above: the
  old-wire distinction is mac1 keying, not packet sizes — the sizes now
  coincide exactly with old SPEC.md's. §1.1's note is written
  accordingly; the log's stated rationale is superseded.
- **J-2 (pacing deleted).** `INITIATIONS_PER_SECOND` (50 / 20 ms
  spacing), the §6.6 pacing step, §17.1's pacing-counter sentence, the
  Appendix B pacing test, and the constants row are **deleted**. No
  ruling names pacing, but its only home — the internal continuation's
  non-app-gated replacement admission — is deleted by rulings 4/11, and
  the spec's own text held that "fresh accepts are already gated by the
  application", which now covers replacements too. Re-homing pacing to
  `accept()` would have required a new error variant (an invention).
  The alternative (keep a 20 ms gate on replacing accepts) is available
  if the maintainer wants churn protection against a compromised
  key-holder that out-paces an auto-accepting application.
- **J-3 (`Install { initial }` vs the batch note).** The log/work order
  say "`Install{initial:false}` remains only for the tie-break
  admission", but ratified §6.7 resolves the tie-break loser's
  `Connecting` via `Install { initial: true }`, and
  `ConnEvent::Established` (which resolves `Connecting`) fires on the
  first install. Applying the note literally would either break the
  loser's connect resolution or require redefining `Established`
  semantics — a new mechanism. I kept the tie-break admission at
  `initial: true`, kept the field, and documented `initial: false` as
  emitted on no path in this revision (shape-only). **Please confirm at
  the focused §§5–7 re-review** — if the note meant "delete the field",
  that is a two-line edit.
- **J-4 (`ToEndpoint::AddressMoved` deleted).** Not explicitly
  mandated. Its only consumers — the hint-map freshness for the rekey
  eager path, and rekey targeting — are deleted; the hint set is now
  the (static) dialled addresses of in-flight connects, so the endpoint
  no longer tracks any connection's address. The application-facing
  `ConnEvent::AddressMoved` is untouched. `ToEndpoint` now carries
  `Retired` alone.
- **J-5 (`AcceptError::AlreadyConnected` deleted, not retained).** The
  work order says "becomes unreachable at accept"; the document's own
  precedent (`Superseded`: "unreachable and deleted rather than
  retained") plus ruling 11's "AlreadyConnected becomes connect()-only"
  make deletion the consistent reading.
- **J-6 (`session_id()` / `SessionId` kept).** With sessions 1:1 to
  connections the accessor is constant per connection. No ruling names
  it; kept unchanged, flagged as a candidate simplification for the
  re-review.
- **J-7 (§13.3 probe-train ending).** `REJECT_AGE` was named as the
  asymmetric-loss ender of the probe train. Deleted with §7.6; the
  clause now says liveness ends the train under both symmetric and
  asymmetric loss (the §7.4 receive-keyed anchor covers the asymmetric
  case: a peer whose ACKs never arrive stops resetting our
  `last_authenticated_recv`, so `DEAD_TIMEOUT` fires). The reasoning
  prose is mine.
- **J-8 (replacements share the intro queue).** Consequence of ruling
  11 stated honestly in §6.3's honesty clause: a sustained intro-queue
  flood can now delay a legitimate *replacement* (previously immune —
  replacements bypassed the queue via the continuation). The genuine
  peer holds the per-packet-race advantage (dedup replace-with-newest +
  evict-oldest), and established connections themselves are untouched,
  but the exposure class is new and should be weighed at the §§5–7
  re-review (the cookies/mac2 deferral is the named remedy).
- **J-9 (§5.4 responder-rule evaluation point).** The walkthrough note
  says "LIVE/PENDING resolution happens at install time"; I worded §5.4
  as: PENDING is consulted at the tie-break's routing, LIVE at the
  staged chain's admission (`accept()`), both post-`ss`. Wording mine.
- **J-10 (intro-entry estimate kept at ≈ 220 B).** 196 raw + address +
  park metadata rounds to the previous estimate; worst case stays
  ≈ 225 KB.

## Clauses/cross-references the rulings did not clearly cover (listed, not resolved)

1. **Simultaneous-open interception dependency** (pre-existing,
   unchanged): if both crossing msg1s miss the hint set (peers dialling
   from ports other than the dialled ones) *and* neither application
   calls `read_identity()` on the parked intros, neither side answers
   until a retransmit hits the eager path or `HANDSHAKE_GIVEUP` — the
   tie-break's backstop is application-driven. The hint-set shrinkage
   does not change this for PENDING statics (their dialled addresses
   remain hints), but it deserves a look in the §§5–7 re-review.
2. **§12.2 fused-ACK upgrade note** ("sustained > 100 Mbps per
   connection") — untouched; still coherent.
3. **Preamble round-1 cluster text** (clusters A and F) still describes
   the deleted machinery in past-revision terms; kept deliberately as
   history under the "partially superseded" annotation rather than
   rewriting the historical record.
4. **"the v1 shell" / "the one v1 implementation"** (§14.1, §14.7)
   previously read against "wire version 2"; they now happen to agree
   with "version 1". Left as-is.
5. **`ConnEvent::Established` comment** ("first install; the shell
   resolves `Connecting`") — with exactly one install ever, "first" is
   vacuous but harmless; left as-is (touching it would ripple into
   §16.4's contract text).
6. **§6.2 staged-verb signatures** — unchanged; `authenticate()` still
   returns the timestamp only (it never exposed flags), so no API edit
   was needed there.

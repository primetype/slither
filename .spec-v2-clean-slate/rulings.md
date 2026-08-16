# Wire-v2 maintainer walkthrough — rulings log

Started 2026/08/14. 32 substantive [MAINTAINER] flags (the 33rd occurrence,
line 14, is the preamble's marker definition). Document order. Edits are
batched: nothing is applied to SPEC-v2-DRAFT.md until the walkthrough ends,
then one revision pass applies every ruling at once.

## Ruling 1 — §1.1 (line 68): version byte and negotiation

**RULED 2026/08/14: modified.** No version bump — no released software ever
spoke the old wire, so the new wire claims `VERSION = 0x01` and
`PROLOGUE = b"slither\x01"`. Negotiation stays permanently out of scope;
wrong version ⇒ silent drop, exactly as drafted.

Edit implications (batch):
- `VERSION` `0x02` → `0x01` everywhere (§1.1, §3, constants table).
- `PROLOGUE` `b"slither\x02"` → `b"slither\x01"` (§1.1, §5.1, constants
  table, any KAT text that spells the prologue).
- Reframe §1.1: the "one deliberate break from v1" framing softens to
  "the first released wire"; the old SPEC.md/SPEC-v2.md remain superseded
  wholesale, but there is no on-wire v1→v2 cut because v1 never shipped.
  Retitle "wire version 2" phrasing across the document accordingly
  (title, preamble, §1.x) — decide final naming during the batch pass
  (suggest: "wire version 1 (first release)" with the old drafts called
  "pre-release drafts").
- Note (accepted risk): old SPEC.md used the SAME version byte and
  prologue, so old-wire dev binaries are distinguished structurally
  (every packet size/payload length differs ⇒ silent drop), not
  cryptographically. Acceptable: nothing was released.
- §1.3 / §4.4 "one-time golden regeneration" text: still true, now reads
  as the first freeze rather than a regeneration.

## Ruling 2 — §1.3 (line 114): block re-affirmation of carried rulings

**Procedural, auto-resolves.** Each of the ten carried rulings has its own
inline flag at its home section and is being ruled individually in this
walkthrough; once all ten are ruled, this block flag is satisfied and the
batch pass rewrites §1.3 to record the individual rulings instead of a
wholesale block.

## Ruling 3 — §4.4 (line 343): mac1 suite genericity

**RATIFIED as written, 2026/08/14.** mac1 stays fixed keyed-BLAKE2b (from
cryptoxide) for every suite; keyed over the canonical AsRef encoding
(P-256: 65-byte uncompressed). Multi-suite-on-one-socket foreclosure
accepted. Ruling 1 makes the encoding change costless (no shipped bytes).
No edit needed beyond dropping the [MAINTAINER] marker; the "absorbed by
the one-time golden regeneration" sentence gets the ruling-1 reframe.

## Ruling 5 — §5.4 second flag (line 468): corrected routing machine

**RATIFIED as a unit, 2026/08/14.** LIVE/PENDING/NONE routing, flag-gating
on LIVE only, computed CONTINUED, Replaced-deferred-to-accept(), frozen
mid-state entries, flag-blind §6.4 re-home. Contingent on ruling 4 (the
flags mechanism itself) — under discussion.

## Ruling 4 — §5.4 first flag (line 420): continuation flags mechanism

**IN DISCUSSION** (maintainer asked for a concrete restart walkthrough
before ruling).

## Ruling 4 — §5.4 first flag (line 420) + §7.6: RATCHET-ONLY REKEY

**RULED 2026/08/14: the periodic DH re-handshake is DELETED; the epoch
ratchet (§7.7) is the only rekey.** Maintainer: "A new handshake from an
already live static key is dropping the connection and starting the new
one." Sessions and connections become 1:1 — no transport state ever
survives a handshake, so the restart-vs-rekey ambiguity is dead BY
CONSTRUCTION, not by flag.

Consequences (batch surgery, §§5–7 need a focused re-review afterwards):
- CONTINUATION/CONTINUED flags DELETED. msg1 payload 13→12 B (timestamp
  only), msg2 payload 1→0 B (hiss `[N]`-on-msg2 support becomes moot).
  Reference sizes shift by 1 (msg1 197→196, msg2 108→107 — recompute).
  Reserved-flag-bits malformedness rule goes with them.
- §5.4 rewritten to a short section: every completed handshake is a fresh
  connection; msg1 at a LIVE static routes as replacement via the staged
  accept (park, frozen mid-state, `Replaced` teardown at accept()) — the
  old restart-replacement row becomes the ONLY LIVE row. PENDING
  (simultaneous open, §6.7 tie-break) and NONE rows unchanged.
- Ruling 5's machine survives REDUCED: LIVE/PENDING/NONE routing, frozen
  mid-state entries, teardown-deferred-to-accept(), flag-blind re-home all
  stand; computed-CONTINUED and flag-gating rows are gone. The endpoint-
  INTERNAL continuation path (silent swap, no accept) is deleted — every
  handshake now goes through the staged accept.
- §7.6 DELETED: REKEY_AGE, REJECT_AGE, NeedsRekey, the silent swap, the
  swap-cut, make-before-break demux. Its [MAINTAINER] flag (line 1228)
  is MOOT.
- §7.8 survival matrix collapses to "a connection has exactly one
  session; nothing survives a handshake"; the re-queue rule is gone.
- §7.9 nonce exhaustion: no DH-rekey escape; counter exhaustion (u64,
  unreachable: 58k yrs at 10M pkt/s; epochs bound per-key volume to
  ~76 MB) ⇒ connection death. Rewrite accordingly.
- §7.5: keepalive no longer consults REKEY_AGE (cluster F's idle-rekey
  fix is moot); DEAD_TIMEOUT liveness unchanged and remains the only
  idle killer.
- §18.1: ConnectionLost::PeerRestarted and ::RekeyFailed deleted;
  ::Replaced stays.
- §14.6 CC-reset-at-swap and §13's swap interactions deleted where they
  reference the session swap.
- Timers table: REKEY_AGE/REJECT_AGE rows out; "no message-count DH-rekey
  trigger" note rewritten (the ratchet is simply the only rekey).
- ACCEPTED LOSSES: no post-compromise healing within a connection (a
  session-key exfiltration decrypts that direction until the app
  reconnects — healing is now application reconnect policy); wire-visible
  indices never rotate for the connection's life (linkability across
  roams). TLS 1.3 KeyUpdate model; WireGuard's contrary choice noted and
  overridden.
- Appendix B test obligations and the preamble's cluster A/F summaries
  need matching rewrites.

### Flag reclassification after ruling 4 (25 stops remain)

- MOOT: line 1228 (§7.6 swap-cut — section deleted).
- REDUCED (walk the surviving half): 1179 (§7.5 — keepalive floor
  survives, idle-rekey half moot); 2393 (§14.6 — rekey seam moot, roam
  seam survives); 832 (§6.5 oracle — internal continuation gone, PENDING
  tie-break probing survives; reword); 2537 (§16.1 — invariant survives
  but "never a new accept" FLIPS: inbound replacement is now always via
  accept()).
- LIVE, unaffected: 567, 637, 644, 706, 744, 1068, 1096, 1361, 1606,
  1766, 1806, 1856, 1910, 1940, 1959, 2112, 2429, 2473, 2631, 2903,
  3046, 3086.
- Note: §6.6's endpoint-internal routing machinery collapses further than
  ruling 4's list stated — with no internal continuation, EVERY inbound
  initiation is staged, and LIVE/PENDING resolution happens at install
  time (accept() or connect-completion). Batch surgery should rewrite
  §6.5–6.6 around that; then the focused §§5–7 re-review.

## Ruling 6 — §6.1 (line 567): typestate names

**RATIFIED, 2026/08/14.** `Intro`/`Claimed`/`Proven` final as public API.

## Rulings 7–9 — §6.3 (lines 637, 644, 706): intro-queue posture

**ALL RATIFIED, 2026/08/14** (dedup key ratified after a mobility
discussion — key is stage-0-only, address changes recover by convergence,
post-establishment mobility is roaming's job):
- 7: flood posture (evict-oldest + per-source 4/IP (/64 v6) + 15 s TTL,
  honesty clause).
- 8: dedup key = full source SocketAddr alone, replace-with-newest.
- 9: `Superseded` deleted from error enums (unreachable under
  own-bytes-on-consume).

## Ruling 10 — §6.4 (line 744): accept() re-home model

**RATIFIED, 2026/08/14.** Peer-commitment (never packet-commitment), fast
path, newest-first-by-park-time re-home walk (same-static-only, ≤4
candidates), AcceptError::Stale, one-teardown-path guard. Batch edit:
delete the CONTINUATION-candidate paragraph (moot under ruling 4).

## Ruling 11 — §6.5 (line 832): membership-timing oracle, reduced

**RATIFIED in reduced form, 2026/08/14.** Probed set = pending outbound
remotes only. Batch surgery (extends ruling 4's list): hint set = dialled
addresses of in-flight connects only; internal continuation = tie-break
only; LIVE claims park at 0 DH and surface as ordinary Intro/Claimed
(replacement decided at accept: proven static LIVE ⇒ Replaced swap, the
timestamp guard blocks stale/replayed candidates); restart-replacement
tag deleted; AlreadyConnected becomes connect()-only. Ruling 10's guard
member reads accordingly ("proven-LIVE accept replaces; stale dies at the
guard").

## Rulings 12–13 — §7.2 (line 1068), §7.3 (line 1096)

**BOTH RATIFIED, 2026/08/14.**
- 12: ACK record fused to the 2048-bit replay window; decoupled tracker
  stays the §19 wire-free upgrade; Appendix B burst simulation stands.
- 13: never-lifted 3× anti-amplification budget, authenticated-bytes
  funding, binds all output classes; PATH_CHALLENGE stays the §19 lever.

## Rulings 14–15 — §7.5 (line 1179, reduced), §8.2 (line 1361)

**BOTH RATIFIED, 2026/08/14.**
- 14: persistent-keepalive floor (≥ DEAD_TIMEOUT) stands; idle-rekey half
  moot per ruling 4 (delete "consults REKEY_AGE" from §7.5).
- 15: post-AEAD structural failure = signalled death (CLOSE +
  PROTOCOL_VIOLATION, ProtocolViolation{code} variant, nothing applied).

### Batch note found while walking §9.1
"Rekeys swap Noise handshake roles freely but never change stream-ID
parity" (§9.1 role stability) — rekey clause moot under ruling 4; role
stability reduces to "opener bit = original establishment roles (dialler,
or tie-break winner under simultaneous open)".

## Rulings 16–18 — §9.1 (1606), §9.8 (1766), §9.8 (1806)

**ALL RATIFIED, 2026/08/14.** Four ID spaces; messages = uni-stream sugar
(no DATA frame); overflow ⇒ receiver reset (code 0, regenerated until
acked).

## Rulings 19–22 — §10.2 (1856), §10.4 (1910), §10.6 (1940, 1959)

**ALL RATIFIED, 2026/08/14.** Initial-window constants (revisitable via
Appendix B); cumulative MAX_STREAMS; credit-as-buffer-commitment;
O(credit) reassembly bound (REASSEMBLY_CHUNKS_MAX 1024, revisitable).

## Rulings 23–25 — §12.4 (2112), §14.6 (2393, reduced), §15.1 (2429)

**ALL RATIFIED, 2026/08/14.** Delayed ACK (2nd/25 ms/gap-immediate);
roam-seam controller reset + pre-roam fencing, RTT-as-prior divergence;
minimal CLOSE (5 s linger, ≤1/s authenticated replies). §14.6's rekey
seam text deleted per ruling 4. Flag at line 2473 (ProtocolViolation
variant) marked SATISFIED BY RULING 15 — same ruling, two homes.

## Rulings 26–27 — §16.1 (2537, clause flipped per ruling 11), §16.4 (2631)

**BOTH RATIFIED, 2026/08/14.** One-connection-per-static (inbound
proven-LIVE ⇒ replacement via accept()); two sans-io cores with the
single-poll_output contract. Batch note: Install{initial:false} remains
only for the tie-break admission.

## Rulings 28–29 — §17.1 (2903), §19 (3046)

**BOTH RATIFIED, 2026/08/14.** Guard-eviction honesty clause with pinning
+ no-orphan-on-reject; deferral list as a block.

## Ruling 30 — Appendix A.1 (3086): no-fallback split-read gate

**PENDING, 2026/08/14.** Maintainer: "We will work on that point with
hiss, just after we finish looking at all the decisions 1 by 1." The
[MAINTAINER] flag at A.1 (and its §1.3 block membership) STAYS in the
draft until that joint hiss/slither work concludes. Batch surgery must
NOT drop this flag.

## WALKTHROUGH COMPLETE — final tally (2026/08/14)

All 32 stops resolved: 27 ratified (several in reduced form), 1 declined
with restructure (ruling 4 — RATCHET-ONLY REKEY, the headline change),
1 modified (ruling 1 — no version bump, VERSION 0x01), 1 moot (§7.6
swap-cut), 1 satisfied-by-twin (2473 = ruling 15), 1 pending (A.1).
Ruling 2's block re-affirmation resolves accordingly: all carried rulings
individually re-ratified except A.1 (pending) and the swap-cut (moot).

Next: batch surgery applying every ruling to SPEC-v2-DRAFT.md (ratchet-
only restructure of §§5–7 is the big item), then a focused adversarial
re-review of the restructured sections. A.1 API work proceeds jointly
with the hiss session in parallel; Appendix A reconciles when hiss lands.

### Post-surgery correction to ruling 1's note (2026/08/14, J-1)
The "old-wire dev binaries are distinguished structurally (every packet
size differs)" note is FALSE after the flag deletion: the post-surgery
handshake sizes (174/81/196/107, msg1 payload 12 B) are IDENTICAL to old
SPEC.md's. Differentiators that remain: mac1 keying change for P-256
suites only (33-byte compressed → 65-byte canonical); for the X25519
reference suite the handshake may be byte-compatible with old dev
binaries and divergence surfaces only at the frame layer inside the seal
(old Leg-1/Leg-2 grammar vs the unified frame layer ⇒ post-AEAD
structural failure ⇒ §8.2 signalled death). Accepted risk unchanged
(nothing shipped); §1.1's accepted-note wording must state THIS, not the
size claim — under verification in the focused re-review.

## Post-surgery re-review verdicts (2026/08/14)
- Fidelity (`rereview-fidelity-v4.md`): **CONFORMANT** — 0 BLOCKER, 3
  MAJOR, 4 MINOR, 5 NOTE. All 30 rulings applied; arithmetic exact;
  AddressMoved false-alarm (ConnEvent, kept); marker count 28 (2 wrapped).
- Security (`rereview-security-v4.md`): **NOT-SOUND** — 1 BLOCKER, 5
  MAJOR, 5 MINOR, 2 NOTE. All wire-free. Root cause: ratchet-only deleted
  3 compensating controls. Four decisions put to maintainer (SECV4-1/2/4
  basis, SECV4-3 PENDING branch, SECV4-5 arm-on-ack-eliciting, SECV4-6
  keepalive geometry). Directed fixes batched: SECV4-7 (=FIDV4-1 §1.1
  scoping), SECV4-10 (§7.7 justification), SECV4-11 (delete initial
  field), SECV4-8/9 honesty sentences. Positive confirmations logged:
  amplification arithmetic sound, key-holder blast radius confined,
  tie-break determinism closed, no orphaned rekey machinery.

## Rulings 31–34 — post-surgery security fixes (2026/08/14)
All four ruled on the recommended (reviewer-minimal, wire-free) option:
- **31 (SECV4-1/2/4, BLOCKER):** per-connection `replacement_basis:
  Option<Timestamp>` — Some(t) = establishing msg1 timestamp when we were
  responder (staged/re-home/tie-break-loser), None when we dialled. A
  proven-LIVE accept() replaces ONLY if basis is Some(t) and candidate >
  t, else AcceptError::Stale. Tie-break winner-side drop now RECORDS the
  loser's timestamp (post-ss; preserves "only key-holders write guard
  entries", amends "record on full admission only"). Rewrite the false
  guard-bars-replays claims (§6.4 bullet 3, §17.1 honesty clause + new
  sentence, §5.4 LIVE bullet, Appendix B) to state what basis+guard
  actually guarantee (SECV4-2 honest restatement; withheld-newer replay
  surfaces as an unaccepted Intro, destroys nothing until accepted).
- **32 (SECV4-3, MAJOR):** accept() gains a PENDING branch — if an
  in-flight outbound initiation exists for the proven static, cancel it
  (its Connecting resolves Err(AlreadyConnected)) and install fresh as
  responder; tie-break does NOT run here. Appendix B: read_identity()→
  connect()→accept() on one static ⇒ exactly one connection.
- **33 (SECV4-5, MAJOR):** §7.4 arming condition "marking send" →
  "marking OR ack-eliciting send" (deadline armed whenever an
  ack-eliciting packet is in the sent map). Preserves both original
  properties (arm-once-never-re-armed; probes don't DEFER death, now they
  ARM it). Makes §13.3's J-7 sentence true. Appendix B obligation added.
- **34 (SECV4-6, MAJOR):** DEAD_TIMEOUT 15 s → 25 s (moves a ratified
  constant — maintainer-ruled). Restores one-loss idle tolerance (20 s
  gap < 25 s). Update §5.7 derivation note (now 2×KEEPALIVE_TIMEOUT + 5 s
  grace, not "keepalive + one retransmit interval"). Verify
  PERSISTENT_KEEPALIVE floor (≥ DEAD_TIMEOUT) stays coherent: default
  25 s ≥ 25 s holds but is now tight — flag whether the default should
  rise too. Keep an honest note: two consecutive keepalive losses still
  end an idle connection; reconnect is the application's.

Directed (no ruling) fixes to apply same pass: SECV4-7/FIDV4-1 (§1.1
scope both gates per suite), SECV4-10 (§7.7 epoch-death justification →
unopened-messages + CC admission gate), SECV4-11 (delete the always-true
`Install.initial` field), SECV4-8 (§6.5 NAT simultaneous-open honesty
sentence), SECV4-9 (§6.4/§6.9 replacement-churn honesty sentence),
SECV4-12 note 3 (cluster F fully-superseded footnote), SECV4-13 msg2-loss
Appendix B obligation. FIDV4-2 resolved by SECV4-11 (delete field).

---

## Round 3 — rulings 35–38 (2026/08/14, from `rereview-security-v5.md`)

Draft v5 re-reviewed NOT-SOUND: 4 MAJOR, 3 MINOR, 4 NOTE, **no BLOCKER**.
All four rulings below are **wire-free**. The wire (174/81/196/107,
VERSION 0x01, PROLOGUE b"slither\x01") does not move.

Positive result recorded: rulings 31, 33 and 34 were attacked directly
and **achieved their goals** — 31 confirmed load-bearing (a dialled
connection holds `None` basis *and* no guard entry, so the basis covers
exactly what the guard cannot); 33 leaves no unarmed black-hole path;
34's sweep is complete with no stale 15 s. Ruling 32 achieved its goal
but its mechanism was unsound — see 35.

- **35 (SECV5-1, MAJOR) — ruling 32 AMENDED.** The PENDING branch's
  "the tie-break does **not** run here" is wrong: the tie-break is a
  *two-sided* agreement and one side cannot opt out unilaterally. A takes
  the PENDING branch and installs as responder while B runs §6.7, loses,
  and also installs as responder ⇒ two key sets, both msg2s dropped,
  mutually dark for `DEAD_TIMEOUT`. 100 % (not 50 %) when both apps use
  the `read_identity()`→`connect()`→`accept()` ordering; also gives both
  ends the same stream-ID parity, contradicting §6.7.
  **Ruling:** apply §6.7's comparison *inside* the PENDING branch. Peer's
  static smaller ⇒ cancel the pending and install as responder (ruling 32
  as written). Ours smaller ⇒ return `AcceptError::Stale`, leave the
  pending in place, and record the candidate's timestamp exactly as the
  winner-side record does. §16.1's invariant and the one-connection
  guarantee both survive; no new error variant. §18.1's `Stale` prose and
  §5.4's PENDING row need matching edits. Secondary gain: drags the
  replay-cancels-a-pending primitive back inside §6.7's stated
  key-ordering bound.

- **36 (SECV5-3, MAJOR) — the contested-connection probe.** Ruling 31's
  `None` basis has a residual: an attacker who harvests-and-drops genuine
  B→A Data can, **off-path afterwards**, drip one every < `DEAD_TIMEOUT`
  to keep A's zombie alive indefinitely while every genuine reconnect is
  refused `Stale` and A cannot dial out (§16.1) — a permanent wedge with
  application `close()` as the only escape, and no signal to distinguish
  it from a legitimately roaming peer. §6.8's "delayed by at most
  `DEAD_TIMEOUT`" is true only absent an attacker (verified benign case:
  the zombie always arms within `KEEPALIVE_TIMEOUT`).
  **Ruling:** when §6.4 refuses an accept because the live connection's
  basis is `None`, mark that connection **contested** — send an
  ack-eliciting PING (§8.3, frame exists) and require an ACK covering it
  within `KEEPALIVE_TIMEOUT`; if none arrives fire
  `ConnectionLost::TimedOut` and let the parked `Intro` be accepted on the
  application's next attempt. Withheld genuine Data can reset a receive
  clock but can **never** produce a fresh ACK of a packet sent after the
  harvest. Bounded by application-driven `accept()` calls; does not weaken
  ruling 31 (a live peer answers and the refusal stands).
  **Option (c), a 12-byte timestamp in msg2, was CONSIDERED AND DECLINED**
  — it closes the finding at the root but is wire-affecting (msg2 ≠ 107 B,
  golden-wire pin red). Recorded so it is not re-proposed without a fresh
  ruling.
  **Principle to record in the spec:** the death clock is driven by mere
  authenticated receipt, not by acknowledged progress. Every "is this peer
  still there" question in this spec inherits it.

- **37 (SECV5-4, MAJOR) — bound the captured-initiation replay.** §6.7's
  "each captured initiation is single-use" is false: §17.1's orphan aging
  (`INTRO_TTL`-scale) and LRU eviction recycle the very entry that makes
  it single-use. A captured 90 s retransmit train is ≈ 18 initiations,
  each passing the guard vacuously against a peer we only ever dial.
  **Ruling, both parts:** (1) qualify the claim — single-use *per
  initiation, and only while that initiation's guard entry survives
  §17.1's eviction and aging* — with an explicit §17.1 cross-reference;
  (2) exempt a guard entry written by a tie-break admission or a
  winner-side record from orphan aging and LRU eviction for
  `HANDSHAKE_GIVEUP` (90 s) after the connection it created dies. Covers
  an application's reconnect backoff; the 1024 cap still bounds the tier;
  §17.1's mitigation (i) still prevents minting orphans by
  authenticate-then-drop.
  **Declined and recorded:** "refuse to cancel a pending on a vacuous
  guard pass" breaks genuine first-contact simultaneous open (both guards
  empty ⇒ both keep their pending ⇒ both install as initiator ⇒ mutually
  dark). Do not re-propose.

- **38 (SECV5-7, MINOR) — `PERSISTENT_KEEPALIVE` documented as inert.**
  Pre-existing; ruling 34 surfaced it. The knob is unreachable at every
  value the floor admits: suppressed by the 10 s passive dance whenever
  the dance runs (the dance is a marking send, so it drags `last_send`
  forward and pushes the beacon's deadline with it), and firing at
  `S + I > R + DEAD_TIMEOUT` — strictly *after* death — whenever the dance
  is blocked (`S > R`, which only a marking send can establish, so the
  death clock is armed there).
  **Ruling:** correct §5.7's justification sentence (as written it says
  the floor rejects *long* intervals; the floor rejects intervals *below*
  `DEAD_TIMEOUT` — §7.5 gives the real rationale, "so a marking beacon can
  never outpace the death clock") and record the derivation, documenting
  the knob as a no-op under the current liveness model. **Nothing moves**
  — the default stays 25 s and the floor stays `DEAD_TIMEOUT`-relative.
  Re-basing the floor on `KEEPALIVE_TIMEOUT` was considered and declined
  (it reverses §7.5's explicit decision to refuse short intervals);
  deleting the knob was also declined — it stays available should the
  liveness model change.

**Applied without a ruling (spec-completeness gap, not a design change):**
SECV5-2 — the death clock's state *at install* is unspecified, and under
the reading where `last_send` and `last_authenticated_recv` both start at
install time the passive keepalive never fires and a half-open session is
**immortal** (§7.6 is deleted, so there is no age-based reaper backstop).
Three security claims depend on it not being: §17.1 twice ("reaped by
liveness in 25 s"), §6.7's replay bound, §15.4's endpoint-dropped row.
Fix: one sentence pinning the clock as **armed at install**.

Also directed this pass: SECV5-5 (§17.1's honesty clause is wrong for a
static we only ever dialled — no entry to evict), SECV5-6 (pin the
ordering of the guard's *record* against the basis check), SECV5-8
(the one-lost-keepalive tolerance is unidirectional-only; a simultaneous
bidirectional loss still kills at 25 s — state it), SECV5-9 (§10.3
carries only half of ruling 33's `liveness-neutral` redefinition).

---

## Round 4 — rulings 39–40 (2026/08/14, maintainer-initiated)

Arose from a judgment call in pass 3b (pinning `last_send` to the install
instant, which stops the keepalive dance from bootstrapping). I proposed
reversing it; **the maintainer ruled the other way and was right** — the
idle-from-install drop is consistent with §5.4's already-ratified
"sessions:connections = 1:1, restart is structural, no built-in
reconnect". Both rulings are wire-free; ruling 40 moves a *constant* and
reverses a *validation rule*, but no packet byte.

- **39 — an idle-from-install connection dies at `DEAD_TIMEOUT`; the
  passive dance stays automatic.** Pass 3b's pin **STANDS**: a newly
  installed session sets `last_authenticated_recv` = `last_send` =
  install instant with the deadline already armed, so §7.5's passive rule
  (`R > S`) is false on both sides and the dance never bootstraps. A
  connection that never carries traffic emits nothing and is reaped at
  25 s; the application redials when it has something to say.
  **Scope, explicitly:** the dance remains **automatic** for any
  connection that has carried traffic — one exchange puts `R > S` on the
  receiver and enters the loop, so a sparse request/response application
  (60 s gaps) stays connected with no opt-in. Only the
  never-carried-anything case dies. Making *all* keepalive opt-in was
  considered and declined (it silently breaks sparse-traffic apps and is
  a much larger change to §7.5/§13.3/§14.5/§15.4).
  **Consequence to state in the spec:** after death, only a side that can
  still reach the other can restart the connection. For a peer behind
  NAT the binding is gone, so it must be the dialler or must hold the
  binding open with the beacon — which is what ruling 40 makes possible.

- **40 — ruling 38 REVERSED in part: `PERSISTENT_KEEPALIVE`'s floor
  becomes a ceiling.** Ruling 38 documented the knob as inert and moved
  nothing. Under ruling 39 that is untenable: idle connections now drop
  and the opt-in knob is the only way to hold one open, so pointing users
  at a no-op is not acceptable.
  **The diagnosis changed too, and my earlier framing of SECV5-7 was
  wrong.** I reported §5.7's justification sentence as inverted relative
  to the rule. It is the reverse: the **sentence states the correct
  intent** — *"reject an interval so long that the beacon could not keep
  a connection alive on its own"*, which describes a **ceiling** — and
  **the rule was written backwards** as a floor (`reject I <
  DEAD_TIMEOUT`). That inversion is the whole reason the knob is
  unreachable; a beacon firing every 25 s cannot sustain a 25 s death
  timer.
  **Ruling:** `set_persistent_keepalive` **rejects an interval at or
  above `DEAD_TIMEOUT`** (was: below). The beacon fires unconditionally
  on its timer — unlike the passive rule it does **not** require `R > S`
  — so it sustains a mutually idle link and holds a NAT binding. The
  **recommended default moves 25 s → 10 s**, matching `KEEPALIVE_TIMEOUT`
  and giving one-lost-beacon tolerance at 25 s (2 × 10 + 5).
  The beacon **stays in the marking set** — §7.5's declined alternative
  (excluding it from the marking set to admit short intervals) is no
  longer needed, since the ceiling admits short intervals directly and a
  marking beacon is harmless: arming enables death, never defers it, so a
  beacon into a void still dies at `R + DEAD_TIMEOUT`.
  Ruling 38's inertness derivation is **retained as the justification for
  the change**, not deleted — it is the proof the old bound was wrong.

---

## Round 5 — rulings 41–43 (2026/08/14, from the opus+sonnet adversarial pair)

Two independent adversarial reviews of rulings 36/39/40 —
`adversarial-opus-v6.md` (attacker lens, NOT-SOUND: 1 BLOCKER, 2 MAJOR,
6 MINOR, 1 NOTE) and `adversarial-sonnet-v6.md` (interaction lens,
INCONSISTENT: 1 BLOCKER, 2 MAJOR, 4 minor). **Both found the SAME
blocker independently, from different directions** — the strongest
confirmation this process produces. All fixes wire-free.

**Confirmed by attack, not merely unchallenged** (19 documented failed
attacks): ruling 36's cryptographic core — no route forges an ACK
covering the probe (coincidental ranges, restarted peer, reflection,
replay, forged-future ACK all fail); the probe is not an amplifier
(0.15× by bytes) and not an address-steering reflector (it targets the
connection's authenticated address, never the msg1 source); ruling 39's
"one exchange bootstraps the dance" holds in both roles and both
directions with no asymmetric ordering; ruling 40's ping-pong arithmetic
and loss tolerances re-derived correct; the beacon cannot revive ruling
36's wedge (arming enables death, never defers it).

- **41 (ADV-O-1 / ADV-S-1, BLOCKER) — the probe matches a counter
  high-water mark, not a packet.** §7.5 required *"an ACK covering the
  packet that carried that PING"*, but §8.7 files PING in the **never**
  retransmit class (*"a lost PING is superseded by the next probe"*). If
  that one packet is lost, **no ACK covering it can ever exist** — the
  peer's replay window has a permanent gap and every derived ACK (§12.2)
  carries it. A fully live peer that answers every PTO retry is still
  killed at the deadline, falsifying §7.5's own *"a genuinely live peer
  simply answered"*.
  **Weaponised:** §6.4 says the guard is *"empty for every peer we only
  ever dial… every candidate passes it vacuously, however old"* and
  reverts the record on `Stale`, so **one passively captured msg1 is
  replayable forever**; each replay is an independent Bernoulli trial at
  the forward loss rate (≈99% kill at p=0.5% over ~1000 replays ≈ 196 KB).
  This **inverts** the property §6.4 and §17.4 both claim for a `None`
  basis — keeping a captured msg1 from destroying a connection we dialled.
  **Ruling:** record `probe_floor` = the counter the next seal will use
  at mark time (hiss's `next_counter()`, A.2); clear the mark on **any
  ACK covering any counter ≥ `probe_floor`**. Collapse concurrent marks
  into a **single** contested state with one floor and one deadline.
  Security is unchanged — every counter ≥ floor was sealed after the
  harvest, and a peer cannot ACK a packet we never sent — while PTO
  becomes a rescue path instead of an irrelevance.
  Declined: making PING retransmittable (changes §8.7 semantics for a
  frame used by §13.4's probe trains); clearing on any post-mark ACK
  (an ACK already in flight at mark time could satisfy it, weakening the
  after-the-harvest proof).

- **42 (ADV-O-3, MAJOR) — the beacon gets a floor as well as a ceiling.
  My framing of ruling 40 was wrong** and is corrected in the record: I
  proposed "floor → ceiling", which *removed* the floor instead of adding
  a ceiling beside it. `set_persistent_keepalive(1ms)` became conformant
  — 1000 pkt/s on a beacon §14.5 exempts from congestion control, while
  §13.3 already condemns 20 pkt/s as defeating §16.5's timer economy. It
  also suppresses RTT sampling (§13.1's `largest` must be newly acked;
  keepalives are not in the sent map). **So there IS a newly-admitted
  interval worse than the old inert behaviour: the sub-100 ms end.**
  **Ruling:** the admissible range is **[1 s, `DEAD_TIMEOUT`)** — reject
  below 1 s (new floor), reject at or above `DEAD_TIMEOUT` (ruling 40's
  ceiling). Default stays 10 s. Appendix B already mandates accepting
  1 s, so the floor costs no test churn.

- **43 (ADV-O-2, MAJOR) — bound the probe honestly.** §7.5's *"bounded
  by … nothing an attacker controls"* is false: the attacker controls the
  **supply of Intros**, and the cwnd-exempt probe breaks §17.5's "sent
  map bounded by cwnd" and §6.3's "established connections keep running
  regardless."
  **Ruling:** ruling 41's collapse already bounds probes to **at most one
  per `KEEPALIVE_TIMEOUT` per connection** (~1 packet/10 s) however many
  Intros arrive; additionally **count the probe in the sent map** so
  §17.5's cwnd bound stays true, and **correct §7.5's false sentence**.

**Directed, no ruling needed:** ADV-S-2 (§15.4's teardown matrix
misdescribes the contested death's deadline, trigger and peer-visible
symmetry); ADV-S-3 (§16.5's "closed" timer table has no entry for the
contested deadline); ADV-S-4 (§6.9/§17.5 cost accounting for the probe);
ADV-S-5 (§7.3's exemption list is stale, pre-ruling-36); ADV-S-7 (a
stale "DRAFT v5" banner on v6 content); plus the remaining opus MINORs.

- **44 (ADV-O-5) — rejection is a `Result`, never a panic.** Three places
  said `set_persistent_keepalive` "rejects" an out-of-range interval; the
  signature returned `()`, so *how* it rejected was unspecified.
  **Ruling:** `-> Result<(), ConfigError>` with
  `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}`, each naming the
  bound violated. Reasoning recorded in §16.2: the setter's argument is
  the only one an application routinely takes from **outside the
  program** (config file, settings field, remote policy), so an
  out-of-range value is a recoverable *input* error, not a programmer
  error — an implementation must not panic, debug-assert, or silently
  clamp (a clamp reports success while giving a beacon that does not do
  what was asked). Decisive consideration: the setter is reachable across
  bubble-ffi to iOS, where an unwinding panic is **undefined behaviour**,
  not a trappable crash — a panicking setter would make a hostile config
  file a memory-safety problem in the host app. `ConfigError` is a
  *configuration* error and sits deliberately **outside §18.1's protocol
  taxonomy**, which stays closed: no peer, no packet, no connection
  state, nothing observable on the wire. Declined: panic (FFI hazard) and
  a validated newtype (pushes validation to the edge, but costs a public
  type and churns bubble's existing call site).
  Appendix B asserts the `Err` **and** that a rejected call leaves the
  interval unchanged.

**Ruling 44 adds the 30th `[RATIFIED` marker** — the surgery passes held
the count at 29 because they applied existing rulings; a genuinely new
ruling legitimately adds one.

---

## Round 6 — ruling 45 (2026/08/14, from the STORIES.md walkthrough)

All 26 capability stories in `STORIES.md` were walked one at a time and
approved. **25 stand as drafted; one was amended.**

- **45 (from S11) — the contested mark is application-visible.**
  §7.5's contested marking (rulings 36/41/43) was internal: an
  application saw `AcceptError::Stale`, then possibly a later
  `ConnectionLost::TimedOut`, with no way to connect the two.
  **Ruling:** add `ConnEvent::Contested { under_probe: bool }` —
  `true` when the mark is set and the probe goes out, `false` when it
  clears on an ACK covering the probe floor. No third event for the
  unanswered case; the death already arrives as
  `Closed(ConnectionLost::TimedOut)`.
  **Why:** the two outcomes of a basis-`None` refusal are operationally
  opposite — *refused but healthy, keep using it* vs *refused and dying,
  prepare to redial* — and `Stale` alone cannot distinguish them, so a
  reconnect scheduler must guess. bubble-engine's re-dial scheduler
  (`host/lan/mod.rs`, "connection down; re-dial scheduled") is the live
  consumer that needs the distinction.
  It is a **signal, not an instruction**: the refusal stands either way,
  the basis rule is untouched, and an application ignoring the event
  observes exactly the prior behaviour. Emitted at most once per mark,
  and ruling 41's collapse bounds marks to one per connection, so the
  event inherits that bound. **Wire-free** — nothing transmitted,
  nothing peer-observable.

**Three pre-walkthrough answers folded into the stories:** S3's
three-way split confirmed as the correct reading of "a second connection
closes the first" (S3a our own re-dial is refused `AlreadyConnected`;
S3b the peer's handshake replaces only where we were responder; S3c a
connection we dialled refuses and goes contested); S7's claimed-vs-proven
hazard agreed; **S18's mover obligation resolved — the moving peer
keepalives**, with no transport-side probe added.

**Documentation obligations recorded (no code change):** S3a's
`close()`-then-dial, S7's ban-on-an-assertion hazard at
`read_identity()`, and S5's connect-ahead-of-need reaping at 25 s.
**Carried debt:** S25, the send-syscall defect — unpaid work with a filed
consumer complaint, the one story describing something currently broken.
**Watch item, not reopened:** S8's `INTRO_TTL` = 15 s bounds a
human-in-the-loop pairing decision; raising it would weaken the published
`1024 / 15 s ≈ 68 packets/second` flood bound.

---

## Round 7 — rulings 46–49 (2026/08/14, from the fable+sonnet pair)

Two reviews of the spec **and** `STORIES.md`: `review-fable-v6.md` (fresh
consumer lens — ship, after one wire-free batch; 5 MAJOR, all at the
core→shell seam) and `review-sonnet-stories-v6.md` (fidelity —
CONFORMANT; 6 findings, mostly story errors). Both independently caught
the same wrong constant name. **No finding is wire-affecting.**

- **46 (FAB-1, MAJOR) — the shell gets application-facing awaitables,
  and ruling 45 is repaired.** §16.2's surface is verbs plus four
  accessors: **no event stream, no `closed()`**. `ConnEvent` is the
  sans-io **core→shell** enum, so ruling 45's `Contested` variant was
  visible to a shell implementer and **not to an application** — the
  ruling as applied achieved nothing. The gap is wider than ruling 45:
  `AddressMoved` (S18's acceptance criterion) and **connection death**
  are equally undeliverable; a consumer learns a connection died only if
  it happens to be inside a verb call.
  **Ruling:** add quinn-shaped awaitables to §16.2 —
  `Connection::closed().await -> ConnectionLost` and a narrow
  per-connection notification stream carrying `Contested` and
  `AddressMoved`. The shell translates core `ConnEvent`s into these; the
  core/shell split is preserved. Wire-free.
  **Also apply FAB-6:** emit `Contested` at **probe transmission**, not
  at marking — §7.5/§16.5 allow the two to separate under the
  amplification budget (mark-pending) — and rename the variants, since
  `under_probe: false` currently reads as exactly that pending state.

- **47 (FAB-2, MAJOR) — a delivery-confirmation primitive.** The
  "farewell message" trap: `send_message().await` and `finish()` resolve
  **before acknowledgement**, and §15.2 lets `close()` drop stream and
  recovery state immediately, so *message-then-close silently loses data
  at the path's loss rate* — with no API able to wait for delivery. For a
  messaging product this is the sharpest unnoticed trap in the set.
  **Ruling:** expose a way to await actual acknowledgement before
  closing. The core **already tracks it** —
  `ConnEvent::StreamFinished` fires when the send half is fully
  acknowledged — it is simply not surfaced. Wire-free; exposes an
  existing core signal.

- **48 (FID-3, MAJOR) — the denylist is the application's, and slither
  records nothing.** §6.1 forbids durable state keyed on a claimed
  static (*"no map insertion, no rate-limit bucket, no unbounded
  logging"*), because the claim is attacker-choosable. My S7 acceptance
  criterion contradicted the story's own warning and claimed a **0 DH**
  reject for a banned static — impossible, since the static is unknown
  until `read_identity()` spends the `es`.
  **Maintainer's framing, ruling as given:** *"Deny list is an
  application layer issue. But it should be allowed to be done at the
  staged accept. The deny option I talked about was what the user can
  do. Not what slither will do for the user. We need to allow the user
  to reject a static key as soon as they need to. We don't need to
  record this at the persistent level of slither after 2 DH."*
  So: slither **provides no ban list and keeps no such state**; the
  application may reject at **any** stage for any reason, consulting
  whatever list it owns; §6.1 is unchanged and constrains slither's own
  state, not the application's policy. S7 loses the 0 DH claim; the
  claimed-vs-proven hazard warning stays.

- **49 (FAB-4 — maintainer pushback, sustained) — specify `Wire`, drop
  S25 as a capability.** The spec names `Wire` exactly once (line 3749)
  and never defines it, though it is the crate's extension point and
  bubble implements it (`DualStackWire`). v0.1's code defines it in two
  methods returning `io::Result`.
  **Ruling, two parts.** (a) **Specify the `Wire` trait normatively** —
  the omission is indefensible regardless. (b) **Drop S25**: the
  application *supplies* the `Wire`, so it already observes every
  `io::Error` with its destination address before slither does; and
  acting on send failures would contradict §7.4's **receive-driven**
  liveness and would kill connections that are about to **roam**
  (S18/S19) on precisely the signal that precedes a successful
  migration. What remains is **diagnostics**: a §18.2 trace obligation so
  the 25 s becomes explicable. No error path, no API surface, §18.1
  stays closed.

**Story-only corrections (no ruling; `STORIES.md` was wrong, the spec is
right):** FID-1 `ConnectionLost::Closed` → **`PeerClosed { code, reason }`**;
FID-2 S2's anchor §13 → **§5.5**, and handshake retry is **fixed 5 s +
jitter, explicitly not exponential** (§13 is post-handshake PTO);
FID-4 `MAX_MESSAGE` → **`MESSAGE_RECV_MAX`** (caught independently by
both reviewers); FID-5 `testutil::{Network, FlakyPolicy}` are real in
`src/testutil.rs` but unattested in spec text; FID-6 S26's trigger is
dropping **every handle**, not the endpoint — a live `Connection` alone
keeps the driver running.

**Still open, not yet ruled:** FAB-3 (dropping a `Connecting` is
unspecified — an app wrapping `connect()` in a timeout may then hit
`AlreadyConnected` for up to `HANDSHAKE_GIVEUP` with no `close()`
escape: S3a's evil twin) and FAB-5 (§9.8's messages and uni streams
share one incoming supply, invisible on the wire, so mixing
`send_message` with `open_uni` is incoherent for the receiver).

---

## Round 8 — rulings 50–51 (2026/08/14; the two findings left unruled in round 7)

Both from `review-fable-v6.md`; both verified against the spec before
being put to the maintainer. **S27 and S28 (the ruling 46/47
capabilities) are also APPROVED**, taking the story set to 28.

- **50 (FAB-3, MAJOR) — dropping a `Connecting` cancels the attempt.**
  §16.3 enumerates drop behaviour for `Endpoint` and `Connection`
  handles; **`Connecting` appears nowhere in it**. Every real application
  writes `timeout(5s, endpoint.connect(...))`. If dropping the future did
  not cancel, the static would stay PENDING for up to `HANDSHAKE_GIVEUP`
  (90 s), a redial would return `AlreadyConnected`, and — unlike S3a —
  **there is no handle to `close()`**, so there would be no escape at all.
  **Ruling:** dropping the `Connecting` cancels immediately — the static
  leaves PENDING, retransmissions stop, state is freed, a redial works at
  once. This is Rust's cancel-on-drop convention, so it is also what a
  consumer will assume. A peer that already installed a half-open session
  loses it to liveness at `DEAD_TIMEOUT` — exactly ruling 39's reap case.
  Declined: sending a CLOSE when msg2 already arrived (more correct on
  the wire's terms, but more state carried through the drop path);
  requiring an explicit cancel (fights the convention and leaves the trap
  for everyone who reaches for `timeout()`). Wire-free.

- **51 (FAB-5, MAJOR — worse than reported: a permanent deadlock, not
  mere incoherence) — an unclaimed uni stream fails loudly at
  `MESSAGE_RECV_MAX`.** §9.8: *"The two receive verbs draw from the same
  incoming-uni supply … Which mode consumes a given stream is the
  receiving application's choice, **invisible on the wire**"*, and *"the
  receiver never extends per-stream credit for a sugar-consumed
  stream"*. Concrete failure: A calls `open_uni()` and writes 1 MB while
  B loops on `recv_message()`; B treats the stream as a message, never
  extends credit past 262 144, and **A stalls forever** — no error, no
  timeout, keepalives still flowing so liveness never fires. The
  `MESSAGE_RECV_MAX` check is enforced on the **send** side of
  `send_message()` only, so `open_uni()` bypasses it entirely.
  **Ruling, two parts:** (a) a uni stream that reaches
  `MESSAGE_RECV_MAX` while still unclaimed must be **reset with a
  defined code** so the sender learns (`WriteError::Reset`), rather than
  stalling silently; (b) state normatively that mixing `recv_message()`
  and `accept_uni()` on one connection is a **programming error**, since
  the wire cannot distinguish the modes.
  Declined: a per-connection mode fixed at configuration (makes mixing
  unrepresentable, but removes the legitimate chat-plus-file-transfer
  case); a wire-level mode signal (closes it at the root but is
  **WIRE-AFFECTING** and turns the golden-wire pin red).
  Wire-free — no new frame; it uses the existing reset path.

- **52 — the overflow reset carries a distinguishable code.** Ruling 51
  shipped its receiver-emitted reset with `error_code = 0` (`NO_ERROR`),
  which a sender could not tell from the peer's application calling
  `reset(0)` or from a dropped `SendStream` — leaving "fail loudly" only
  half loud, since the one thing the sender needed was *which* hazard it
  hit.
  **Ruling:** mint **`MESSAGE_OVERFLOW` = `0x06`** from §15.3's
  transport-reserved range (`0x06`–`0x0f`), narrowing the reserved band
  to `0x07`–`0x0f` and giving *"never sent"* one named exception.
  **Deliberately judged NOT a wire change:** a new *value* in the
  existing `error_code` varint of an existing frame (RESET_STREAM
  `0x04`), in a case that previously could not arise. No header moves,
  no frame type is added, no size changes, and the golden-wire vectors
  pin handshake bytes, so they stay green. Carried in RESET_STREAM only,
  never in CLOSE.
  Applied at §9.6's retained frame identity, §9.8's rule and its new
  ruling-52 paragraph, §15.3's registry, and Appendix B's obligation.

**Also confirmed at the same sitting — ruling 51's trigger keeps its
guard.** The applying agent deliberately did not take my blanket
brief ("any unclaimed stream at `MESSAGE_RECV_MAX` is reset") and was
**right**: the literal form would reset legitimate streams for a
receiver that uses `accept_uni()` only and is merely slow, contradicting
§16.4's *backpressure by retention* for peer-opened streams awaiting
`accept(dir)`. The applied form fires on every unclaimed window-full
stream — the ratified "oldest" restriction dropped, since a stream
behind a slower one could evade it — but only while a `recv_message()`
claim is pending or at the instant one is made. That closes ruling 51's
deadlock (a receiver looping on `recv_message()` always has a claim
pending) while leaving lazy `accept_uni()` intact.

**Ruling 50's PENDING interaction — verified clean, no edit needed.**
All three readers of "is this static PENDING?" (§6.4's ruling-35 branch,
§6.5's hint set per §17.4, §5.4's state row) read the **same pending
tables** that cancellation empties; there is no separate per-static
PENDING flag that could outlive a cancelled attempt. An initiation
arriving after a drop therefore takes §5.4's NONE row and never reaches
§6.7's comparison. Cancellation also writes nothing to §17.1's guard —
it authenticated nothing — identical to a `HANDSHAKE_GIVEUP` expiry.

---

## Round 9 — the implementation plan (2026/08/14)

The spec was ratified; the maintainer asked for an implementation plan
over `STORIES.md`, a **clean full rewrite**, and — the new requirement —
**composable APIs**: `AsyncRead`/`AsyncWrite` so `BufWriter` /
`tokio::io::copy` work, `tokio_util::codec::Framed`, `Stream`/`Sink`,
and `tower::Service`. Plan: `PLAN.md`. Six rulings, all **shell-layer,
no wire byte, no core type, no timer**.

**Two stale artifacts found first, both in documents already verified.**

1. **§9.8 held a paragraph arguing against ruling 52** — "One
   consequence, stated", which recorded that the reset carried `0` and
   argued minting a code was not worth it. Ruling 52 overturned exactly
   that judgement. My ruling-52 verification grepped for the changed
   *token* (`Reset(0)`) and came back clean. **Method note: also grep for
   the rationale you reversed.** Retired in place as `[SUPERSEDED]`, not
   deleted, so the old position is not re-proposed as new.
2. **`STORIES.md`'s "Still open — not yet ruled" section was entirely
   stale** — dropping a `Connecting` was ruled 50, the shared uni supply
   was ruled 51/52. Neither had a story, so nothing was testing ratified
   behaviour. **S29** (cancel-on-drop, immediate redial) and **S30** (the
   mixing hazard, `MESSAGE_OVERFLOW`, and the load-bearing guard) drafted
   to close it, awaiting approval.

**Ruling 53 — the shell seam is split by cost.** §16.3 said handles were
"thin **channel-backed** clients"; §16.8 called for "the quinn pattern"
(per-stream waker maps) and accessors that are "synchronous reads of a
shared cell". Two mechanisms, one buildable. Ruled: **command channel +
oneshot** for the endpoint and staged verbs (§6.2 requires the DH to land
on the driver task, and they are rare and already `async`); **shared cell
`Rc<RefCell<_>>` + waker maps** for the connection data path and the
accessors. The driver is `!Send` and single-threaded, so the cell costs a
refcount and a borrow flag — no lock, no contention. Every mutating
borrow marks the connection dirty and wakes the driver, so §16.4's
drain-after-every-mutating-call contract is untouched; it is merely not
always the driver that made the call.
*Why it is load-bearing, not style:* each data-path verb is written once
as `poll_*`, §16.2's `async fn` is `poll_fn(..).await` over it, and
§16.11's `AsyncWrite` is the **same function** with its error mapped.
Under the channel form none of that exists: every adapter boxes and
stores an in-flight future, `Unpin` becomes delicate (the stored future
has already copied a buffer the next `poll_write` may not pass again),
and every write costs a round-trip and an allocation.

**Ruling 54 — `Connection::flush()` → `Connection::acked()`.** The
`AsyncWrite` surface made the old name unusable: `AsyncWrite::flush` on a
`SendStream` from the same object graph means something strictly weaker
(bytes in send state), and a consumer facing two `flush` verbs on
adjacent objects — one meaning *acknowledged by the peer*, one promising
nothing — will pick the wrong one exactly where it matters. `acked()` is
symmetric with `SendStream::acked()`. §16.2's "normative in shape; an
implementation may rename" permits it. **Semantics unchanged in every
respect**: snapshot scope, reset-abandonment termination, and what an
acknowledgement does and does not promise all stand as ruling 47 wrote
them. Applied across §9.8, §14.5, §16.1, §16.2 and Appendix B.

**Ruling 55 — `open_bi`/`accept_bi` yield `BiStream`, `.split()` yields
the pair.** The duplex object is what the ecosystem consumes; it is what
makes `Framed<BiStream, C>` and `copy_bidirectional` work with no
adapter. Shape change to §16.2, no semantic change, nothing lost.

**Ruling 56 — `AsyncWrite::poll_flush` is a no-op returning `Ready`.**
`poll_write` accepts only what flow-control credit admits, so accepted
bytes are already in send state and there is no shell buffer to push.
Promising more would make `flush` a second, weaker `acked()` and put the
two in competition.

**Ruling 57 — `AsyncWrite::poll_shutdown` is `finish()` *and then*
`acked()`.** The weaker reading (finish alone) is prior art elsewhere and
is rejected here for the reason ruling 47 exists: `copy(..).await;
shutdown().await` is the natural shape of a transfer, and under the weak
reading it loses its tail at the path's loss rate, silently — **S28's bug
reachable a second time**, through the `AsyncWrite` surface, by an
application that never touches `close()`. Bounded by connection death, so
it cannot hang.

**Ruling 58 — an adapter never claims ahead of its consumer.**
Normative: a `Stream` adapter claims **at most one item, only from inside
`poll_next`**. No prefetch, no read-ahead task, no intermediate queue.
§16.4's pull model is what keeps reliable data in the core until the
application takes it (§16.8); an adapter that claimed ahead would rebuild
the unbounded shell queue §10.6 forbids **while looking like an ordinary
ergonomic convenience**. This is the easiest way to get the
composability layer wrong. Pinned by Appendix B.

**Maintainer ruling on sequencing (D9).** *Rewrite on main.* The
maintainer's words: "we don't really break bubble now, this is a move to
a new directory. Bubble will not be updated until this rewrite lands."
Recorded plainly: `bubble-engine/Cargo.toml` carries
`slither = { path = "../../slither" }`, which resolves to this
directory, so bubble's build does stop until the cutover — accepted, and
bubble is not a constraint on the rewrite.

**Declined / deferred, recorded so they are not re-proposed.**
- **A `Send` façade** (driver on its own thread, `Send` handles over
  channels) — would let slither compose with buffered service layers,
  hyper, and plain `tokio::spawn`. **Deferred past v0.2 by design, not
  omission:** it re-crosses the core→shell seam with channels, which is
  exactly where round 7's five defects lived. §16.11's handle shapes are
  compatible with adding it later without a breaking change.
- **Companion crates for the adapters** — rejected in favour of in-crate
  additive features (`sink`, `codec`, `tower`); the gates already run
  `--all-features`, and companion crates would need their own pipelines
  for no gain.

**Ruling 59 — the message-mode overflow reset is traced by the
receiver.** Surfaced while walking S30 for approval, and it is an
asymmetry rulings 51 and 52 both left standing. §9.8's reset tells the
**sender** precisely what happened — that is the whole of ruling 52's
`MESSAGE_OVERFLOW` — while the **receiver**, whose verb choice actually
caused the conflict, emits a RESET_STREAM and continues with no error, no
notification, and nothing in its API surface recording what it did. *The
party that can fix the bug is the one with no evidence of it.* Ruled: the
receiver **MUST** trace the reset under §18.2's `slither::frames`, naming
the stream, its final size, and the mode conflict. The reasoning is
ruling 49's exactly — slither does not act on the condition, and its
obligation is the one thing the application cannot do for itself: make
the failure explicable afterwards. Concrete post-mortem: an operator
asking why transfers to this peer die at exactly 256 KiB reads the
**receiver's** log to find out. **No API change, no notification, no new
error variant** (§18.1 stays closed) **and no wire change** — the
RESET_STREAM was already being sent. Applied to §9.8 and §18.2.

**S29 amended at approval — the retry loop does not accumulate.** Dial →
cancel → redial, repeated, leaves the responder with exactly one
connection per cycle: its `replacement_basis` is `Some(t)` (it was the
responder), each fresh initiation is strictly newer, so each accept is a
**replacement** (S3b), which §16.1's one-connection-per-static makes the
only available outcome. Bounded cost per cycle: 4 DH and one
`ConnectionLost::Replaced`. The property holds *through* S3b, but it is
now tested from the direction a consumer actually reaches it — a timeout
retry loop — rather than from the replacement story.

**`STORIES.md` is COMPLETE at 30 approved stories** and is the acceptance
criteria for `PLAN.md`. D10 is closed.

---

## Round 10 — slice 0 planning (2026/08/14)

The implementation process the maintainer set: a planning agent per
slice, then an implementer, then two verifiers in parallel (opus +
sonnet) for breadth, then my review, looping on findings. Slice 0's
planner (opus) produced `.slices/00-ground/PLAN.md` — 1 744 lines,
~370 lines of SPEC.md read across 11 targeted ranges, never the whole
file. It found three real defects and asked ten questions.

**A fourth instance of the ruling-52 verification miss, and the pattern
is now clear enough to name.** Ruling 52 minted `MESSAGE_OVERFLOW` =
`0x06` and its application updated §15.3 and §9.8 — the **normative**
sections. It did not update the places that **restate** them. Found so
far, each by a different route:

1. §9.8's "One consequence, stated" prose, still arguing the code was not
   worth minting (found by reading, round 9).
2. The consolidated **Named-constants table**: `wire error codes
   0x00–0x05` (found by the slice-0 planner).
3. **§18.1's own closing sentence**: `0x00`–`0x05` again (found by me,
   while applying ruling 61 two lines above it).

**The generalisation: a ruling's blast radius is every place that
restates the thing it changed, not every place that defines it.** A
grep for the changed token finds definitions; summaries paraphrase, and
paraphrases do not contain the token. For future rulings the check is:
grep the *old* value, the *new* value, and the rationale — then read the
document's summary tables and closing sentences by hand, because those
are written in prose and will not match any of the three.

**Ruling 60 — the fixture surface is attested and its determinism is
normative.** §16.10 named `FlakyWire` alone while the fixture has three
parts, all depended on by name downstream. Now attested: `Network` (the
routing fabric), `FlakyWire` (a `Wire` at one address), `FlakyPolicy`
(the impairment). Two things are made normative rather than left to
taste: **`FlakyPolicy` MUST be deterministic under a caller-supplied
seed** — a flow test that cannot be replayed byte-for-byte from its seed
is not a regression test, and §13/§7.5's loss-dependent behaviour is
exactly where a one-in-a-thousand failure would be unactionable — and
**send-failure injection is required from the start**, because ruling
49's trace obligation is untestable without it and retrofitting it later
would rewrite the tests of every slice that had already ridden the
fixture. The three names are contract on §18.2's terms. Closes S24's
attestation gap.

**Ruling 61 — `#[non_exhaustive]` goes only where a variant is actually
reserved.** `WriteError` alone (§19 reserves `Stopped` for the
STOP_SENDING round); every other error type is exhaustive. A consumer
matches with no `_` arm and gets a **compile error** the day a variant is
added — for a transport that is the loud failure worth having, since a
wildcard arm silently swallows a new error into a branch written for the
old ones. Adding a variant elsewhere is a major bump: the correct price,
and a useful brake. Declined: uniform `#[non_exhaustive]` on all ten
types (a permanent ergonomic tax on types that will never change, buying
semver freedom for a taxonomy that is closed by process anyway).

**Ruling 62 — a `Connecting` IS a handle; a `closed()` future is not.**
The driver lives while a `Connecting` lives. The distinction is
principled rather than a carve-out: **a future that changes protocol
state when dropped is a handle; one that does not, is not.** A
`Connecting` owns an in-flight attempt — a pending, its index, §5.5's
retransmit train — which is precisely why ruling 50 makes dropping it
state-changing. A `closed()` future owns nothing and merely observes.
Consequence: **`ConnectError` needs no `EndpointDropped`** and §18.1
stays closed. The asymmetry against `IntroError`/`AuthError`/
`AcceptError`, which all carry it, is correct: a staged verb is a
round-trip to a driver it does **not** keep alive, so that driver can
stop underneath it; an outbound attempt keeps its own driver running.

**Settled by reading, no ruling needed.** `PTO_BACKOFF_CAP` = 2⁶ is the
**multiplier (64), not the exponent**: §13.3 caps `2^pto_count`, and
§13.5 says "2⁶× too long". The planner flagged it as ambiguous; the
sentence structure settles it. A compile-time
`assert!(PTO_BACKOFF_CAP == 1 << 6)` keeps both readings visible anyway.

**Three brief errors of mine the planner caught and worked around
rather than obeying** — working rule 5 doing its job:
1. "`error.rs` is §18.1 verbatim" is wrong. Ruling 44 puts
   `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}` deliberately
   *outside* the closed taxonomy, and §16.2's surface does not compile
   without it. Ten types, 41 variants.
2. The brief omitted `src/shell/mod.rs`, which is structurally required.
3. `CountingIdentity` cannot land in slice 0 — `Identity` has no home in
   the module map until slice 2. Ships as `CountingProvider` + `DhCounter`
   now, the `Identity` impl in slice 2.

**Ruling 63 — thirteen constants the spec fixes but never names.**
Surfaced independently by slice 0's planner and its conformance-test
author, working without sight of each other: both stopped at the same
gap, which is what makes it a gap rather than a preference. Values stated
as prose (`½ window`, `every 2nd`, `9⁄8`, `≤ 1 per s`, `2⁶`), as a
compressed range (`reserved packet types … 0x04, 0x05`), or under a
one-letter alias (`L`) are normative but have **no identifier a reader
can grep for**. The Named-constants table gains a subsection giving each
one a normative name. No value, byte or behaviour changes.

*The two recurring shapes matter more than the thirteen entries*, because
both are ways a specification can be complete and still unimplementable
without a guess:
1. **A prose ratio or rate needs an identifier AND a stated unit.**
   `PTO_BACKOFF_CAP` is the proof: "2⁶" is a multiplier (64) in §13.3's
   sentence and reads as an exponent (6) in the table. An implementer who
   guessed wrong backs off 64× too little **with nothing red to show for
   it** — no test fails, because the constant is self-consistent.
2. **A prose range against one named constant becomes two or three
   identifiers in code.** Name the default and the floor; leave the
   ceiling as a comparison against `DEAD_TIMEOUT`. A named ceiling would
   be a second place `DEAD_TIMEOUT` is written down, and therefore a
   place it can drift.

Going forward: a new constant SHOULD enter the table with an identifier,
not only a value, and a prose-stated ratio SHOULD carry its unit.

**Process note — the author swap earned its cost on its first use.** The
conformance-test author (sonnet) wrote `tests/spec_constants.rs` and
`tests/spec_errors.rs` from `SPEC.md` alone, forbidden from reading
`src/` or the plan's transcription table, and given constant **names**
but no **values**. It read four ranges plus three it had to find itself
(§8.3 for per-name frame types, §2.3/§2.4 for `STATIC_PUBLIC_LEN` — which
the consolidated table never names — and ruling 44's passage for
`ConfigError`). A third independent reading by me confirmed all thirteen
prose-derived values agree: 65, 0x07, 2, 2, 9/8, 64, 12 000, 2 400, 1 s,
1 s. Three readings, no divergence — which is the only evidence that a
transcription is right, since a single reader checking their own work
proves nothing.

---

## Round 11 — slices 1 and 2a (2026/08/14)

*Header added 2026/08/15. Rulings 64–80 were appended under round 10's
heading as they were made and the file lost its round boundary; the
content below is unchanged. 64–67 came out of slice 1 (the wire, frozen
by them); 68–80 out of slice 2a (the handshake and the staged ladder).*

**Ruling 64 — the packet header is little-endian.** §3.1 said "all
multi-byte header fields are big-endian" through every draft from v1
onward. `grep -n -i endian` over `rulings.md` returns **nothing**: across
63 rulings and ten rounds the byte order was never argued. It was an
inherited default — "network byte order is what protocols do" — carried
forward untouched because nobody looked at it.

Looked at, it loses on its own merits. The rule reaches **three fields**:
`sender_index`, `receiver_index`, `counter`. Everything else in every
header is a single byte or an opaque octet string. Against those three:

1. **WireGuard is little-endian**, and slither's packet layer is
   WireGuard-shaped by construction. §3.4 invokes "the WireGuard posture"
   *by name* to justify the full 8-byte clear counter, and then encoded
   that same counter the opposite way round from WireGuard.
2. **The counter *is* the nonce.** §3.4 already says the value is
   "simultaneously the AEAD nonce"; Noise encodes the ChaChaPoly nonce
   little-endian. Big-endian made the wire bytes the byte-*reverse* of the
   nonce they denote — a gratuitous discrepancy at the one place a header
   integer meets a cryptographic construction.
3. **packtool packs little-endian natively.** Big-endian forced every
   header field to `[u8; N]` with `to_be_bytes`/`from_be_bytes` at each
   site, discarding the typed-field guarantee packtool exists to provide
   and adding a hand-conversion — a place to be wrong — per field.

**The consistency objection, and why it fails.** The obvious defence of
big-endian is §8.1: the varints are byte-identical to RFC 9000 §16 and so
big-endian, and a wire that mixes orders reads badly. It fails on a fact
about slither specifically — **the frame layer rides inside the AEAD**. A
hexdump of a slither datagram shows the header and *nothing else*; the
varints are ciphertext until a key opens them. The two orders are never
observable in the same cleartext, so the inconsistency has no reader. What
remains is each half matching its own lineage: the WireGuard-shaped header
little-endian, the QUIC-shaped frames big-endian.

**Scope, stated so it is not over-applied.** §8.1's varints do **not**
change. §2.4/§6.7's static comparison does **not** change: it compares
equal-length canonical octet strings lexicographically, which is not an
integer encoding and has no byte order. No length, no constant, no frame
layout and no behaviour moves — only the order of bytes within three
header fields.

**Why the timing was the whole question.** mac1's preimage is "all packet
bytes preceding the tag" (§4.1), so the header bytes feed the DoS gate;
the 14-byte data header is the AEAD associated data verbatim (§3.4).
Endianness is therefore load-bearing on both the gate and the AD, and the
golden vectors that freeze all of it land in slice 1 — Appendix B freezes
them "for the first time at wire version 1, then held byte-identical."
Free to decide today; a wire version to decide tomorrow. Raised because
slice 1 was about to make it permanent, not because it was urgent on its
own.

*Generalisation, and it is the uncomfortable one.* A ratified spec's
**unargued** lines are its weakest, and they are invisible to exactly the
process that ratified it: sixty-three rulings all reviewed decisions
somebody had *made*. Nothing in ten rounds was pointed at the defaults
nobody chose. The maintainer's question — "why do we need big endian?" —
found in one sentence what the review process structurally could not,
because review examines what is contested and a default is by definition
what nobody contested. Before a freeze, the question worth asking is not
"is every decision right" but "which lines here were never decisions."

**Ruling 64 — three corrections to its own text, from the independent
derivation.** The golden-wire deriver, working from spec text alone with
no sight of `src/` or the implementation plan, read ruling 64 the day it
landed and returned three defects in it. All three are in the ruling's
*rationale*, none in its rule; the header stays little-endian. Recorded
because the ruling was written by the same reader who verified it, which
is exactly the failure mode the derivation exists to catch — and it caught
its own commissioning ruling first.

1. **"Two things this rule does not reach" was wrong: there are three.**
   §5.2's msg1 payload timestamp is `ts_secs(8, BE) ‖ ts_nanos(4, BE)` —
   a multi-byte integer on the wire, big-endian, unmentioned. Worse, the
   ruling's own defence of the mixed reading ("never observable in the
   same cleartext") **fails for this one**: the responder decrypts msg1
   while still holding the header bytes, so both orders genuinely are
   visible together. The timestamp stays big-endian — §5.3's
   strictly-greater test is an ordering, and a big-endian `ts_secs` orders
   correctly compared as an octet string, which is a substantive reason
   and not an accident — but it is now named as a considered exception
   rather than passed over in silence.
2. **The counter/nonce byte-identity is a reference-suite property, not a
   general one.** Noise encodes the ChaChaPoly nonce little-endian and the
   AES-GCM nonce **big-endian**, so under a future AES suite the identity
   inverts. The claim sat unqualified in §3.1 and §3.4 — sections §2.2
   declares suite-independent. Both now scope it explicitly. The rule is
   unaffected: it rests on WireGuard's shape, and the nonce coincidence
   was always the lesser of the two reasons.
3. **"The low eight bytes of the ChaChaPoly nonce" was imprecise to the
   point of wrong.** Noise builds the nonce as `32 zero bits ‖
   LE64(counter)`; the counter occupies bytes `[4, 12)`, which are the
   *trailing* bytes. Read as integer significance, the "low" bytes of that
   12-byte value are the four zeros at `[0, 4)`.

*Generalisation.* Ruling 64 itself argued that a specification's unargued
lines are invisible to the process that ratified it. The immediate sequel
is narrower and sharper: **a ruling's rationale is not reviewed by the act
of ratifying its rule.** The maintainer ruled on "little-endian, yes or
no"; the three paragraphs of justification written around that answer went
in unexamined, and two of them were suite-specific claims stated as
general ones. Rationale is what the next reader reasons *from* — a wrong
reason survives longer than a wrong rule, because nothing tests it.

**Ruling 65 — the pre-AEAD length gate is exact for handshake packets.**
Surfaced by the independent golden-wire derivation as a **three-way**
conflict in normative text: §3.1 gated on "shorter than its type's fixed
minimum", §5.5 took a msg2 on the first "**length-correct**, index-matching,
mac1-valid" one, and §6.2's cost table said "short/oversize". For Data the
three agree — it genuinely is a range, 30 to `MAX_DATAGRAM`. For the two
handshake types, which `INIT_PACKET_LEN` and `RESP_PACKET_LEN` fix at 196
and 107, a minimum-only gate admits an over-long packet.

**Why that is a defect and not a latitude.** §4.1 defines mac1's preimage
as "all packet bytes preceding the tag". Under a minimum gate the tag's
position is a function of the received length, so the preimage extent is
undefined by the spec and two conformant implementations can disagree.
Worse, §4.3 states plainly that mac1's key is derived from public data and
that anyone holding the recipient's static can mint mac1-valid packets — so
an attacker pads an initiation, recomputes mac1, and the packet passes. The
padding lands between msg1's fixed 174 bytes and the tag, where **neither
Noise's AEAD nor any secret authenticates it**. The gate is now exact for
both handshake types, which forecloses the malleability and resolves the
preimage to `[0, 180)` and `[0, 91)` — constants, tabulated in §4.1.

*The deriver followed working rule 3 and refused to pick a side*, recording
instead that it believed §5.5 and the `_PACKET_LEN` constants held the
intent while §3.1 held the bug. That reading is correct and is why the
ruling goes this way: "fixed minimum" is exactly right for Data, and reads
as a generalisation across all three types made without re-examination —
the same shape of error as ruling 64's unargued default, one layer down.

**Ruling 66 — mac1's BLAKE2b is plain: no salt, no personalisation.** §4.1
gave the construction but never named the parameters. The natural reading
is plain (WireGuard's precedent is plain keyed BLAKE2s, and it is what
`cryptoxide`'s default constructor gives), but a personalised BLAKE2b
changes **every** output byte, and the golden freeze makes that permanent
in this slice. Now stated: both invocations plain, domain separation by
**concatenation** — `MAC1_LABEL` as a prefix on the key preimage — never by
the primitive's personalisation parameter. The key preimage is 12 + 65 =
**77 bytes** on the reference suite.

*The class of finding matters more than this instance.* Rulings 65 and 66
are both **unstated parameters of a stated construction** — not wrong
values, absent ones. A specification can define a cryptographic primitive
completely enough to review and still leave an implementer a free choice
that changes every byte. Neither gap was visible to ten rounds of review,
because review reads what is written and these were gaps in what was
written. Both were found by the *first reader forbidden from consulting an
implementation* — which is the argument for that constraint, stated as
evidence rather than as principle.

**Ruling 67 — the pre-AEAD gate is silent in the trace too; §18.2 stays
closed at five targets.** Surfaced by slice 1's planner, which found that
slither's **committed `Cargo.toml`** asserted a §18.2 obligation §18.2 does
not contain: "the DoS-gate drop counters, the roaming trace, the
unlisted-static reject and the handshake give-up all surface as `tracing`
events", attributed to §18.2. Of those four, §18.2 carries the roaming
trace; the DoS-gate counters and the handshake give-up appear nowhere in
its five targets at all.

The gate emits **nothing**. §18.2's list is unchanged — and unchanged is
the substantive half of this ruling, because §18.2 declares itself
operator-visible contract in which "renaming or dropping one is a protocol
revision", which makes *adding* one a protocol revision too. The
alternative was live and not unreasonable: "silent" in §3.1 plausibly means
nothing signalled **to the peer or the application**, which would not
forbid a local counter, and a flood is precisely the event an operator
needs to see and today cannot. Declined for now — deferring costs nothing
because the gate can gain a target in any later version, while adding one
in the freeze slice spends a protocol revision on observability that no
story asks for.

*The process point is why this was found at all.* The planner declined to
reword the `Cargo.toml` comment itself, on the grounds that **a committed
file making a spec claim should not be quietly corrected by an
implementer** — it either states the spec's obligation or the spec gains
the obligation, and which one is a maintainer's call, not an editorial
tidy-up. That is working rule 5 applied to a case the rule does not
literally name, and it is right: an agent that had "fixed" the comment
would have erased the only evidence that the two documents disagreed.

*Generalisation.* Prose **outside** the spec that restates a spec
obligation is unversioned, ungated and untested — no gate in the release
table reads a `Cargo.toml` comment — so it drifts silently and then reads
as authority to the next agent, which is exactly how CLAUDE.md came to name
the **v0.1** wire as ratified and point at deleted code. Ruling 63 gave
prose constants an identifier so they could be grepped; the same disease
one layer out has no such fix, only the discipline of treating every
non-spec restatement as a claim to verify rather than a fact to read.

**Ruling 68 — the AEAD tag is 16 bytes on every suite; only `PK` varies.**
Found by slice 1's fidelity reviewer, which noticed that `classify`'s Data
lower bound uses `constants::AEAD_TAG_LEN` while `C::AEAD_TAG_LEN` sits in
scope, unused, in a function generic over the suite *precisely so* the
other §2.3 sizes can be per-suite. Chasing the discrepancy into the spec
found two ratified statements that cannot both hold:

- **§2.3** defines `TAG` as "the suite AEAD's tag size" — explicitly a
  per-suite quantity — and lists `AEAD_TAG_LEN` among *reference-suite*
  values.
- **§3.5** states `MAX_PLAINTEXT` as a flat `1170 (= MAX_DATAGRAM − 14 −
  16)` and the data-path overhead as a flat `14 + 16 = 30 B`.

A suite whose tag were not 16 falsifies §3.5. The reviewer stopped at the
conflict and reported it rather than changing one token, which was right
twice over: **it is not a one-token patch.** Making the Data floor
per-suite makes `MAX_PLAINTEXT` and `MAX_DATAGRAM_PAYLOAD` per-suite too,
and reshapes several of slice 0's committed constant assertions.

**Ruled: `TAG` is fixed at 16 for every suite, and `PK` is the only
per-suite quantity in §2.3's formulas.** The precedent is already in the
spec one section away — §4.4 fixes mac1 at keyed-BLAKE2b for every suite
rather than following the suite's Hash, on the reasoning that a per-suite
choice there would demand a capability from every backend and buy nothing.
The same holds here, and the cost is genuinely zero: **every AEAD Noise
defines has a 16-byte tag** (ChaCha20-Poly1305 and AES-GCM alike), so the
generality being surrendered has no instance. The code is correct as
written; the spec's wording was the defect.

*This is the fourth finding of one shape in a single slice* — after 65
(the length gate), 66 (BLAKE2b's parameters) and 67 (a `Cargo.toml`
claiming a §18.2 obligation). Each is **a stated construction with an
unstated or mis-stated scope**, and none is a wrong value. The pattern is
now specific enough to act on: *when the spec introduces a symbol in a
formula, it must say what varies it.* §2.3 wrote `TAG` beside `PK` and
made them look alike; one was per-suite and one was not, and nothing in
the text distinguished them. Rulings 63 gave prose constants an
identifier; this one says a **formula's free variables need their domain
stated**, which is the same disease at the level of derivations rather
than values.

**Rulings 69–71 — three defects in §6.3/§17.1/§16.4, all found by slice
2a's planner refusing to resolve them.** Each is *a stated construction
with an unstated or contradicted scope* — the shape slice 1 produced four
times. That the same shape now dominates two consecutive slices is itself
the finding: it is not a coincidence of §3, it is how this specification
fails.

**Ruling 69 — evict-oldest orders by last refresh, never by park time.**
§6.3 said a full queue "evicts the oldest unconsumed entry (**by park
time**)" while `INTRO_TTL` runs "15 s after the entry's **last refresh**",
and dedup's replace-with-newest explicitly "refreshes its TTL". Two
clocks, one queue. The rule and its own rationale two bullets above are in
direct contradiction: that rationale promises "dedup's replace-with-newest
and the evict-oldest guarantee give a retransmitting genuine peer **the
same per-packet race as any fresh initiator**" — and under park-time
ordering the opposite holds exactly. A genuine peer retransmitting for
14 s carries the *oldest* park time in the queue and is evicted **first**,
while every attacker's freshly-parked entry is younger than it. The
guarantee inverts precisely for the party it names.

Ruled: one age key, last refresh, serving both expiry and eviction; the
per-source cap's "oldest unconsumed" reads the same way. *This is the
third time in this project that prose held the correct intent while the
formal rule held the bug* (CLAUDE.md working rule 3), and the third time
an agent that reported the conflict instead of resolving it was right.

**Ruling 70 — `TS_GUARD_ORPHAN_TTL`, an alias of `INTRO_TTL`.** §17.1 aged
guard orphans on "an `INTRO_TTL`-scale timer" in three separate places and
named no constant. "Scale" is not implementable: it leaves open whether
the interval is exactly `INTRO_TTL` or merely comparable to it, and an
implementer must invent an identifier and then guess its binding. Named
now, and **bound as an alias rather than given its own literal** — one
value, two names, no second place for 15 s to be written down and drift.
Ruling 63 diagnosed this disease for constants; §17.1 was still carrying a
case of it.

**Ruling 71 — §16.4 gains `intro_source` and `intro_sender_index`.** §6.3
requires that a refreshed entry's "accessors reflect the newest bytes **at
call time**"; §6.1 exposes `source()` and `sender_index()` at 0 DH; §6.2's
`Intro` has both. §16.4's core API listed **no accessor for either**, so
the shell had nothing to read them through. Cached-at-surfacing is not a
workaround but a wrong answer: §5.5 mints a **new random index on every
retransmit**, so a cached `sender_index()` reports a value that is no
longer on the wire the moment a refresh lands. Additive to the core, no
wire change. The list was elided, not closed.

*The generalisation, now that this shape has produced seven rulings across
two slices.* An API listing in a specification is read as **exhaustive**
by the only audience that matters — the implementer — whether or not its
author meant it that way. §16.4's elision was invisible while nobody was
building against it and became a hole the moment someone was. The same is
true of ruling 64's "two things this rule does not reach" (there were
three) and of §2.3's `TAG` sitting beside `PK` as though both varied. **A
list that is not exhaustive must say so**, because a reader cannot
distinguish an omission from a decision.

**Rulings 72–76 — slice 2a's queue, walked one at a time.** All five came
out of agents refusing to resolve what the spec left open, and four are
CLAUDE.md working rule 8's shape.

**Ruling 72 — `IntroError::Local` and `ConnectError::Local`.** `Identity::
open()` is fallible and §18.1 could not say "our own key hardware failed",
so a locked enclave and a peer's unreadable msg1 both reported
`Malformed`. The defect is **misattribution, not coarseness**: the two are
opposite in every way that matters — one is the peer's fault and final,
the other is ours and transient — and an application told the peer sent
garbage may reasonably stop retrying, denylist, or alert, over a condition
S21 treats as *expected*. It is §18.2's recurring shape a third time: the
party who can fix the problem is handed evidence pointing elsewhere.
Notably **the implementation already drew the distinction correctly** —
hiss failure discards the chain, provider failure leaves it parked — and
could not express it, which is what made this an error-taxonomy defect
rather than a logic one. §18.1's closure exists to stop variants accreting
*after release*; nothing has shipped, so the amendment is free now and a
breaking change later.

**Ruling 73 — orphan aging runs from pin release.** Ruling 70 named
`TS_GUARD_ORPHAN_TTL` and did not notice the interval had two possible
origins: the same defect one level down, **inside the fix for it**. Three
readings converged on release, one of them *against its own
implementation* — the implementer had built last-admission and wrote that
the test author's textual case was better, since §17.1 defines an orphan
as a dead-connection entry and an entry cannot age as an orphan before it
is one. The fidelity reviewer supplied the consequence, and it is a
security one: under last-admission a connection outliving the TTL carries
`last_admitted` frozen at accept time, so at retirement the entry is
already past its deadline and dies with **no orphan window at all** —
deleting replay protection for the longest-lived connections at exactly
the moment a captured initiation becomes replayable.

**Rulings 74 and 75 — the core's verbs are idempotent and order-tolerant.**
§16.4's verbs take `&mut self` and an `IntroId`, so unlike §6.2's
`self`-consuming handles they can be called twice or out of order; the
handle typestate makes both unreachable from an application, so these are
robustness rules for the core alone. Both are resolved **in the direction
that cannot perturb §6.1's ladder**: a repeat `read_identity()` returns
the cached static at 0 DH without opening a provider, and `authenticate()`
on a still-parked chain drives the skipped `es` to land on exactly 2
cumulative. Because §6.1 prices *cumulatively*, the permissive answer
costs precisely the ratified amount — the error variants the strict
readings would have needed buy nothing and would have enlarged a taxonomy
already being amended once.

**Ruling 76 — §16.5's equal-deadline list is exhaustive, under a stated
principle.** It ordered exactly one endpoint pair while **§16.4 declares
generation order normative** — so the omission did not leave the other
pairs free, it made §16.4's claim hollow wherever two timers collide.
Mutation testing had already found the practical consequence: reversing
the one specified pair is undetected, and cannot be forced by any test
without controlling the jitter draw. Now governed by a principle rather
than a list of instances — **a terminal outcome precedes a routine one,
and state removal precedes emission** — giving give-up, intro expiry,
orphan aging, retransmit, with the pre-existing rule as an instance of it.

*Two observations from walking these five.* First, **ruling 73 dissolved
part of ruling 76's problem before it was ruled**: with aging starting at
release, a give-up that unpins an entry grants it a fresh window instead
of exposing an expired one, so orphan-aging and give-up stopped
interacting. Ordering questions can be answered by removing the
interaction rather than by picking a winner. Second, three of these five
were resolved *as already built* — the value of asking was not that the
code changed but that the behaviour stopped being an accident. An
unstated rule that happens to be implemented correctly is still unstated,
and the next implementer gets no help from it.

**Rulings 77–78 — the two defects rulings 72 and 73 created.** Both were
found by the agent applying 72–76, and both are cases of a fix carrying
its own flaw one level down. That is now three times in this project
(ruling 70 named a constant without noticing it had two origins; ruling 64
mis-scoped its own rationale), and it is worth stating as a habit rather
than an anecdote: **a ruling's blast radius includes the ruling.**

**Ruling 77 — only a key-holder pin restarts the orphan clock.** Ruling 73
started aging at pin release. But §17.1 pins at `read_identity()`, on a
static that is merely **claimed**, and §6.1 states plainly that reaching
`Claimed` requires no secret and the claimed static is attacker-choosable.
So ruling 73 handed anyone able to send a mac1-valid msg1 a way to restart
the aging clock of any static already holding an entry, at 1 DH, for as
long as they liked — deferring mitigation (ii) indefinitely.

Narrowed: a `Claimed` chain's pin still bars eviction, as §17.1 requires,
but does not restart aging; only a live connection, an outbound pending or
a `Proven` chain does. Ruling 73's security argument is untouched, because
a retiring connection is a key-holder pin and still gets its full window.

*The exposure was bounded and the narrowing is still right.* The record is
**retained**, not destroyed — mildly protective — and `TS_GUARD_ORPHAN_CAP`
with admission-only recency still evicts, so memory is capped. Ruled
anyway, because an unauthenticated party must not move a timer: it is a
lever that compounds with whatever a later slice adds beside it. Read as
§6.1's own rule reaching one step further than §6.1 states it — not merely
*no new durable state* keyed on a claimed static, but **no control over
the lifetime of existing state**.

*The corroboration is the part worth keeping.* The independent test
`an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl` **failed** under
ruling 73 and **passes unedited** under 77. Its author had reasoned about
this hazard's mitigation-(iii) form and designed against it, months of
context before ruling 73 existed. A test written from the spec alone
rejected a ruling that had drifted from it — which is the strongest
argument yet for keeping the test author away from the implementation.

**Ruling 78 — `AuthError::Local`.** Ruling 72 added `Local` to
`IntroError` and `ConnectError` and did not reach `AuthError`, where the
same fault mapped onto `HandshakeFailed`. That is the worse half of the
original defect: §18.1 designates `HandshakeFailed` **the only security
signal in the staged taxonomy**, deliberately detail-free because it means
a forged claim died at the msg1 tail's AEAD tag. Reporting a locked
enclave through it does not merely blame the peer for a local fault — it
reports the peer as an **attacker**, and teaches an operator to distrust
the one variant that must stay trustworthy. `authenticate()` can meet the
fault because ruling 75 lets it drive a skipped `read_identity()`, so the
two rulings interact: 75 widened the path, 72 fixed only part of it.

**Rulings 79–80 — the last two open items from slice 2a.**

**Ruling 79 — `Local` stays a unit variant; the provider's error rides
`slither::io`.** Ruling 72 and 78's `Local` variants say *whose* fault it
is and not *what* it was, and the detail cannot ride the variant:
`ConnectError`/`IntroError`/`AuthError` derive `Clone, PartialEq, Eq` and
are asserted `Send + Sync + 'static`, while `Identity::Error` is bounded
only `core::error::Error + 'static` — **deliberately**, because S21's
enclave provider is `!Send`. A `Box<dyn Error + Send + Sync>` needs bounds
`Identity` does not have; an `Arc<dyn Error>` still breaks `Eq`.

Resolved by splitting the audience, which is what §18.2 has been for
throughout: **the party who can act gets the variant, the party who
diagnoses gets the trace.** `slither::io` already exists for exactly this
category — ruling 49 created it for local faults "the protocol never acts
on", recorded so a post-mortem is answerable — so the provider's error
joins it rather than minting a sixth target. Ruling 67's bar (adding a
target is a protocol revision) is therefore not crossed: the target count
stays five.

*This is the third time the same split has resolved a question here* —
ruling 49 (a 25 s death must be explicable), ruling 59 (the receiver of a
mode-conflict reset learns nothing), and now this. Worth stating as the
general rule: **when a fault is real but the application cannot act on the
detail, the detail is a trace, not a variant.**

**Ruling 80 — §16.4's `handle_connection_event` and `reject` take
`now: Instant`.** Both are **mutating calls**, and CLAUDE.md's
architecture invariant — carried from §16.4 itself — is that `now` is an
argument on *every* mutating call and the cores never read a clock. The
ratified block gave neither one. That is not a missing convenience: under
ruling 73 both verbs stamp `orphaned_at`, and
`handle_connection_event(Retired)` is **precisely** the retirement path
ruling 73's security argument is about. So the two verbs that must know
the time were the two not given it.

The applying agent was right to refuse to change the signatures unruled
(brief rule 2: the spec wins) and right to build a watermark instead —
`last_now` + a per-entry provisional stamp + a first-observation floor.
Its own first attempt at that was a plain forward stamp, and an
independent test caught it as wrong. **That subtlety is the argument for
deleting the mechanism, not for keeping it**: four pieces of coupled state
whose only job is to reconstruct an instant the caller already held.

*The generalisation, and it is uncomfortable.* §16.4's block was ratified
with a stated invariant it does not satisfy, and the contradiction
survived every review because reviews read the block as a **list of
signatures** rather than as a claim measured against a rule stated
elsewhere. D5 had already found the block schematic in a second way (the
output types must be generic over the suite). An API listing in a
specification is the place where working rule 8 bites hardest: it looks
exhaustive, it looks literal, and it is routinely neither.

---

## Round 12 — slice 3 planning (2026/08/15)

Two rulings and three plan decisions, from the slice-3 planner's ten
candidate rulings and five reported conflicts. The planner resolved none
of them, which is the discipline working: every conflict below is stated
with both sides.

**Ruling 81 — `Retired` fires when the connection's state is dropped, not
when its death is announced.** §16.4 said "every terminal `ConnOutput` —
a `Closed(ConnectionLost)` event, **or** the completion of the close
linger — is followed **within the same drain** by
`ToEndpoint::Retired`", and `Retired`'s own contract is "drop the index
route (**MUST**)". §15.2 needs that route **alive** for `CLOSE_LINGER`:
the closing state replies to an authenticated, window-fresh inbound
packet, and receiving requires the `receiver_index → Connection` route.
Read literally, then, `close()` emits `Closed(LocallyClosed)`, `Retired`
follows in the same drain, the route dies, and the linger is unreachable
— **deleting what §15.2 calls "CLOSE's only reliability mechanism."**

The amendment separates the two moments. `Closed(_)` is emitted at the
death, so `close()` and `closed()` resolve when §16.2 promises rather
than five seconds later; `Retired` follows the `CloseLinger` expiry on
the closing and draining paths, and the `Closed` event within the same
drain on every path that has no post-mortem — liveness, nonce
exhaustion, `Replaced`, any teardown before a session exists.

*Working rule 3's shape for the fourth time.* §15.2's prose makes a
substantive protocol claim; §16.4's "within the same drain" is the
code-like rule; the prose held the intent. The planner reported it
rather than picking, and was right to.

*What made it hard to see* is worth recording, because it is working
rule 8 again from an unusual angle: §16.4's apposition is **already
imprecise on its own terms** — "the completion of the close linger" is
not a `ConnOutput`, and no such variant exists. A list that enumerates
one thing of the stated type and one thing of a different type is
signalling that its scope was never worked out, and six reviews read
past it.

*Carry.* Slice 2a's core reuses `ToEndpoint::Retired` as S29's
cancellation signal — recorded there as a plan derivation, **not** a
ratified rule (`.slices/02-handshake/OPEN-QUESTIONS.md`). `Retired`
therefore carries two meanings today, and this ruling moves one of them.
Slice 3b must either separate them or state why one variant is right for
both.

**Ruling 82 — the epoch size is config-supplied for tests.** S23 pins
§7.7's ratchet at its 65 536-message boundary, and that boundary is
otherwise unreachable in a test: crossing it honestly costs ≈ 131 k AEAD
operations, and the counter setter that would shortcut it is
`#[cfg(test)]` **inside hiss**, so no consumer can reach it. A
configurable epoch pins the boundary *behaviour* while a separate
constant test pins the *value* — independently, which is the stronger
arrangement, because one test crossing a real boundary would pass just
as well against a wrong constant.

The schedule is security-relevant (it bounds how much traffic one key
seals), so this carries **§16.6's test-only rule verbatim** rather than
inventing a second policy: production uses `REKEY_EPOCH_MSGS`, and a
build accepting a caller-chosen epoch must be feature-gated or
documented as such. §16.6 ruled the identical shape for the RNG seed,
which is why this is a precedent applied rather than a new judgement.

### Plan decisions (no wire byte, no spec text)

**Slice 3 is cut into 3a and 3b.** 3a is the connection core (§7.1–7.2,
§7.7–7.9, the frame codec, §15's CLOSE and post-mortem states, §16.4's
poll contract, §16.5's connection timers, §16.7); 3b is the shell (the
driver, slice 2b's deferred `Endpoint`/`Connecting`/staged handles, the
`Connection` handle, `closed()`, §16.2/§16.3/§16.8/§16.9/§16.10). The
post-slice-3 seam review moves to after **3b** — all five of its
chartered targets live there.

The argument is mechanical, not aesthetic. **Working rule 6's
test-author/implementer split cannot work across the combined slice**:
3b's story tests must compile against a public shell API that does not
exist when a combined slice starts, so either the test author blocks on
the implementer or both are handed one API sketch — which is exactly the
"one author making both wrong in a mutually consistent way" that rule 6
exists to prevent. After the cut, 3a's surface is frozen and green
before 3b's test author starts.

*The cost, stated rather than dressed up:* 3a closes exactly one story
(S23) and S1 — the maintainer's #1 — stays open through it. 3a closes a
named **S1 precursor** at the core level instead, and it must be named a
precursor: a name is not a pin (working rule 9), and calling it S1 would
let 3b ship without the story test S1 actually asks for.

**§7.4's liveness half moves from slice 7 into 3a.** §15.2 seals CLOSE
with `seal_quiet`, which is §7.4's, which `PLAN.md` gave to slice 7 —
so slice 3 could not build its own headline frame as specified. Moving
the two seal paths, the death-clock arming rule, the install pin and the
`Liveness` timer costs ≈ 60 lines and makes five stranded obligations
reachable (§6.7, §17.1, §15.4's endpoint-dropped row, and Appendix B's
S29 and `closed()` obligations). §7.5's keepalive, persistent keepalive
and contested probe stay in slice 7. A plan/spec boundary error, settled
by moving the boundary.

**Slice 3a implements four frames, not three.** `PLAN.md`'s slice table
said "§8 frame codec with CLOSE/PING/ACK only", but PADDING is `0x00` in
§8.3's ratified table and §8.3 makes an unrecognised type "a structural
failure — CLOSE with `PROTOCOL_VIOLATION`". Three frames would therefore
**kill a connection on a legal packet**. PADDING lands in 3a, together
with §3.4's empty-plaintext keepalive short-circuit. Not a spec question
— the plan under-specified its own scope.

---

## Round 13 — slice 3a integration (2026/08/15)

Four rulings, all raised by the two blind slice-3a agents. **Three of the
four were flagged by *both* agents independently**, which is the
authorship split earning its cost: an implementer and a test author who
cannot confer, both stopping at the same sentence, is evidence that the
sentence is underdetermined rather than that one of them misread it.

**Ruling 83 — the opening CLOSE is not a reply.** §15.2 caps CLOSE
replies at one per second and never says whether `close()`'s own CLOSE
starts that clock. IMPL-A anchored the clock on the first *reply*;
TEST-A wrote one test assuming the opening CLOSE anchors it. It was the
single failing test out of 82 when the two halves met.

The cap governs **replies**, and §15.2 says a reply is owed only to "an
authenticated, window-fresh inbound packet" — the opening CLOSE is not
one. The decisive argument is purpose rather than grammar: §15.2 calls
the reply rule "CLOSE's only reliability mechanism", and the case it
exists for is a peer that **lost** the opening CLOSE. That peer's next
packet arrives at an arbitrary moment; anchoring the clock on the
opening CLOSE would make it wait up to a second to learn of a death it
has already missed once. The cost of the other side is at most two
CLOSEs in quick succession before the 1 Hz cap engages.

*TEST-A had already identified this as a spec gap* and deliberately
started its CLOSE-flood test at *t* + 2 s "so both readings agree" — and
then wrote a second test that did depend on it, in another module,
without noticing. Worth recording as a failure mode: **an author who
finds a gap and routes one test around it does not thereby route them
all around it.**

**Ruling 84 — no `Retired` without a session.** Ruling 81, one day old,
listed "any teardown before a session exists" among the no-linger paths
that MUST emit `ToEndpoint::Retired`. But `Retired { our_index }` names
the index route it exists to drop, and a connection that never installed
a session never had one. The case is **not constructible**, and IMPL-A
declined to widen a ratified enum shape to represent it. Correct:
`Closed(ConnectionLost)` is emitted alone, there is nothing to retire,
and the MUST protects against a leak that cannot occur.

*Third time a ruling's blast radius has included the ruling itself* —
64 mis-scoped its own rationale, 70 named a constant without noticing it
had two possible origins (→ 73), and now 81 named a case it cannot
build. The pattern is specific enough to state: **a ruling that
enumerates cases should be checked against the type that carries them**,
because an enumeration is written in prose and the constructor is not.

**Ruling 85 — the liveness death lands *at* the deadline.** §7.4's rule
was strict (`now − last_authenticated_recv > DEAD_TIMEOUT`) while §7.4's
own prose says a session "dies at install + `DEAD_TIMEOUT`" and is
"reaped by liveness in 25 s", and §16.5 says a deadline fires "no
earlier than `D`". The comparison becomes `>=`.

Working rule 3's shape for the fifth time, and the tell is mechanical
this time: under the strict reading the timer fires at exactly `D`,
finds the connection not-yet-dead, and must re-arm one nanosecond
later — a formal rule that makes its own timer table do useless work is
usually the half that is wrong.

*Both agents flagged it and neither depended on it*: IMPL-A built the
prose and said so; TEST-A asserted `D − 1 ns` and `D + 1 ns` only,
leaving the disputed instant untested on purpose. Neither had to be
right for the slice to land — which is what reporting rather than
resolving buys.

**Ruling 86 — the CLOSE `error_code` caps at 2⁶² − 1, at the producing
side.** §8.1's "stated consequence" list named ACK `largest`, stream
offsets and final sizes, and **omitted the one entry an application
controls**: §16.2's `close(code: u64, …)` accepts a value no varint can
encode. The cap goes where `reason`'s truncation already is, per §8.4's
rule that an implementation "must not be able to *produce* the
over-length case it must kill on receipt" — not in the encoder, where
the decision would sit far from the identical rule it mirrors, and not
as an error, because §15.2's teardown is a path an application must be
able to take unconditionally.

Working rule 8 exactly: a stated construction (the varint cap) with a
list of consequences that reads as exhaustive and is not. The twelfth
instance across four slices, and still not a wrong value.

---

## Round 14 — slice 3b dispatch (2026/08/15)

Three rulings, all raised by the slice-3 planner and left open through
3a because they touch only the shell. **Taken by the assistant under an
explicit instruction to proceed without check-ins**, each recorded with
its reasoning so reversal is a ruling rather than an excavation. None
touches a wire byte.

**Ruling 87 — `connect()` is synchronous, and ruling 53's table hid
it.** §16.2 declares `pub fn connect(…) -> Result<Connecting,
ConnectError>` — not `async`. Ruling 53's seam table listed `connect`
beside `accept` and §6.2's three staged verbs under "command channel +
oneshot reply". A oneshot cannot be read from a non-`async` function
without blocking, and §16.8 forbids blocking on that seam.

Neither text is the bug on its face, so the question is which is
load-bearing. §16.2's signature is: an `async connect()` would make
`timeout(d, connect(..))` ambiguous about what is being timed, and
ruling 50's whole idiom depends on the `Connecting` being the only
future. Ruling 53's grouping is *not* load-bearing, because its stated
reason does not apply — §6.2 requires **DH costs** on the driver task,
and **`connect()` performs no DH**: §6.1's initiator costs are paid when
msg1 is built, on the driver. The verb only mints the pending.

So `connect` sends its command and returns, and §16.1's NONE/PENDING/LIVE
test "at the instant of the call" is a synchronous read of the shared
cell §16.8 already mandates for accessors. **The second-order gain is
the better argument**: that same synchronous read makes ruling 50's
cancellation-ordering MUST *structural* rather than a discipline. An
immediate redial reads the cell `Connecting::drop` just wrote, with no
dependence on the driver being scheduled in between — which on a paused
clock with no await between drop and redial **it is not**, and which is
exactly why Appendix B specifies that case.

*The shape, again:* a table that groups by mechanism, with one row whose
justification does not reach every member. Working rule 8's twelfth
instance and the third in a table.

**Ruling 88 — the coincident last-handle drop transmits nothing.**
§16.2 says dropping the last handle to a `Connection` performs
`close(NO_ERROR, "")`, and two sentences later that dropping every
handle kills every connection silently. Both are true because the sets
usually differ — one connection ending while the driver lives, versus
the process letting go of everything. They coincide when the last
`Connection` is also the last handle in the process, and the spec did
not say which rule wins.

**§15.4's endpoint-dropped row is unconditional, and it governs.** A
synchronous `Drop` cannot await the driver, and the driver is already
stopping; ruling 50 takes the identical position for `Connecting` ("an
attempt that never completed has no session to close and no wire signal
to send"); and the peer's cost is bounded at `DEAD_TIMEOUT`, which that
row already accepts. S26 carries a `⚠ CHECK` marking this as drop-order
sensitive and "the opposite of the obvious guess", so it is a
documentation obligation as much as a behaviour.

**Ruling 89 — `SessionId` is hiss's, re-exported.** It appeared exactly
twice in 5 900 lines — once in §16.2's accessor signature, once in
ruling 53's table — and was **never defined**. hiss derives it from the
handshake hash; both peers of a session produce the same value; hiss's
own documentation states it is a *public* channel-binding value intended
for out-of-band comparison. That is exactly what §16.2's accessor is
for. A slither wrapper would add a type to be kept equal to hiss's by
hand, for nothing. Verified reachable from the seal half slither already
holds (`DatagramSend::session_id`, `datagram.rs:250`), so nothing need be
captured at install.

*One property must be carried into the rustdoc rather than assumed:*
hiss's `Eq` on it is deliberately **not** constant-time, documented as
acceptable because the value is public. slither re-exports that property
along with the type, and must say so where an application might reach
for it to compare something secret.

---

## Round 15 — the seam review (2026/08/15)

The review `PLAN.md` §5 chartered after slice 3b, run by two agents blind
to each other on different models. **They disagreed on fact twice, and
the disagreement is the most useful thing the review produced** — each
time, the reviewer who was wrong had verified a *true* property that did
not bear on the question. Recorded in full because the pattern will
recur:

- **C-B1.** Sonnet traced `core::Endpoint::accept()`'s
  `statics.get(&peer_key).is_some()` guard and concluded the shell's
  static-map mirror cannot diverge. The guard is real. It does not fire
  in the interleaving that matters: when `AcceptChain` is processed
  **first**, the core's own map is still empty for that peer.
- **`deadline()`.** Sonnet verified `poll_output()` is idempotent *at*
  `Timeout` — true — but never asked whether the cores **are** at
  `Timeout` when `deadline()` runs. `transmit().await` is a yield point.

*The general form:* **a verification is only as good as its
applicability, and a true lemma about the wrong state proves nothing.**
Two reviewers were worth their cost precisely here; one reviewer, either
one, would have shipped a wrong verdict with a clean argument attached.

**Ruling 90 — `core::Endpoint::connect()` splits into `mint_pending` and
`start_attempt`, in slice 4.** Ruling 87 settled that the shell's
`connect()` is synchronous, and justified it with "`connect()` performs
no DH — §6.1's initiator costs are paid when msg1 is built". **That
sentence describes a core factoring that does not exist**: slice 3a's
`core::Endpoint::connect()` mints the pending *and* builds msg1, two DH,
in one call. The ruling's conclusion survives — the shell routes the
call through the driver, so the DH still lands there — but the price is
a **shell-side mirror of the static map**, a second record of "one
connection per static", which is a security invariant.

The mirror is *correct* as built: it is an admission test, not an
authority, and the core refuses anything it wrongly admits. It was
nonetheless the root of the review's worst finding, because the shell
had asserted the two maps could not disagree.

Splitting the core verb deletes the mirror. `mint_pending` costs no DH,
so the shell may call it synchronously and read the core's own map;
`start_attempt` builds msg1 on the driver, where §6.2 requires it.
**Scheduled for slice 4 rather than done now**: the current code is
correct and freshly reviewed, slice 4 opens the connection core anyway,
and re-opening a just-reviewed slice to remove a wart that is not a bug
trades real risk for tidiness.

*The lesson is about how I check my own rulings, and it is worth more
than the ruling.* Ruling 87's rationale was verified against the **spec**
and not against the **code**; ruling 89's was verified against **hiss**
(`DatagramSend::session_id` is real) and not against **slither's own
abstraction** (`Handshake::Seal` is an associated type with no bounds, so
the method was unreachable and had to be added). Two rulings, one day
apart, both right in conclusion and both carrying a rationale that named
a mechanism that was not there. *A ruling's rationale is not reviewed by
the act of ratifying its rule* — recorded once already, at ruling 64,
about someone else's rationale. This is the same defect in mine.

**Findings fixed at `9a26c15`**, each verified by executing its mutation
rather than by argument: the false assertion (deleted — the divergence it
forbade is legitimate and the `Err` branch beneath it was already S3a's
answer); a panicking driver freezing the endpoint silently while the
`LocalSet` swallowed the panic and the harness printed `ok`; `Driver::
waiting` growing 132.3 bytes per cancelled `accept()` forever; and
`deadline()` destroying a queued `Transmit`, a **silent CLOSE loss**.

*Two of those four were unreachable from all 451 existing tests by
construction*, and the reason is worth keeping: **`FlakyWire` models
everything a network does and nothing a socket does.** A fabric that can
lose, delay, duplicate and reorder cannot express "this send fails" or
"this driver panics". The test suite's coverage was bounded by the
fixture's imagination, not by the authors'.

---

## Round 16 — ruling 90 applied (2026/08/15)

**Amendment to ruling 90.** Its rationale — "`mint_pending` costs no DH,
so the shell may call it synchronously and read the core's own map" — is
true and sufficient for `connect()`, and **silent about
`Connecting::drop`, which must become a synchronous core call too**. With
the mirror deleted, a redial's admission test is `mint_pending` reading
the core's map; a `Retired` still delivered on the driver is one command
later than that test, and ruling 50's MUST — the cancellation ordered
ahead of any endpoint verb issued after the drop returns, *with no clock
advance* — fails. Verified by mutation: deferring it reds
`a_cancelled_dial_frees_the_static_with_no_clock_advance`,
`s29_cancel_then_immediate_redial` and
`s29_retry_loop_replaces_rather_than_accumulates` — three tests, two
files, two authors. The verb is 0 DH, so §6.2 permits the handle to call
it; that permission is the mechanism the rationale should have named.

*This is rulings 87 and 89's shape — a stated construction with an
unstated scope — occurring **inside the ruling written to fix that
shape**.* Ruling 64 recorded that a rationale is not reviewed by the act
of ratifying its rule. Third instance in my own text, and the first in a
ruling whose subject *is* the defect. Working rule 11 was written after
87 and 89 and did not prevent 90; the rule says to check the rationale
against the code, and the omission here is not a false claim but an
**absent** one, which reading the code does not surface. *Amended rule
11 accordingly: also ask what else the mechanism you are naming has to
be true of.*

Two scope questions ruling 90 left open, settled as implemented:
`connect` is **deleted, not kept as a wrapper** (§16.4's API list needs
the two names in its place), and `start_attempt` is a **no-op for an
unknown `ConnectionId`** — a reachable case, not defensive coding, since
ruling 50's cancel can retire the pending before the `Connect` command
is processed. That is why a dial cancelled before the driver runs now
spends 0 DH and puts nothing on the wire.

**Ruling 91 — §6.5 and §6.6 move from slice 7 into slice 4.** Ruling 90
turned `read_identity() → connect() → accept()` on one peer — §6.4's
"ordinary API ordering" — from *completes immediately* into *both sides
`TimedOut` at 90 s* unless the application drops its `Connecting` and
re-accepts. Measured at both ends, not inferred.

The split did not create that; it **exposed** it. The old path bought
convergence by assigning over local state, which §6.4 forbids in terms
("never over local state"), and was **silently non-conformant** — and
the state it produced is not even representable in the core's map:
restoring the mirror's behaviour detonates `StaticMap::insert`'s own
`§16.1: one session per peer static` assertion, which it escaped only
because the mirror kept the row out of the map the assertion guards.

So the fault is not the split but the **absence of §6.5's routing and
§6.6's internal tie-break completion**, without which neither side's
kept pending can finish. *The real question was never when the split
lands; it is when §6.6 lands.* Slice 4 already opens the endpoint core
for ruling 90, so the context is loaded now and will not be again until
slice 7. The honest cost is scope on the largest slice in the plan,
which is already cut 4a/4b for size.

*A correction I owe the record.* I proposed two reasons the interim
state was acceptable — that the kept pendings would converge on §6.7's
tie-break, and that the old behaviour was ruling 35's "mutually dark"
failure. **Both are wrong, and the applying agent proved it by
measurement**: §6.5 and §6.6 being slice 7 is exactly why nothing
converges, and the old install was always paired with a *refusal* of the
connect, so no msg1 of ours was ever in flight and §6.4's divergence
case did not arise. The verdict survives on other grounds — loud failure
over silent non-conformance — but not on the grounds I gave. *An
orchestrator's reasoning is not evidence, and an agent that adopts it
instead of checking it has been made useless.*

**Amendment to ruling 91 — §6.4's PENDING branch comes too.** Ruling 91
attributed the 90-second regression to "the absence of §6.5's routing and
§6.6's internal tie-break completion". That is **right for the peer side
and incomplete for the accepting side.** §6.4:1436 says in terms:

> "This is the branch that closes the `read_identity()` → `connect()` →
> `accept()` ordering — the static was NONE when the chain was staged and
> became PENDING before it was accepted, so §6.5's interception, which
> fires when a *parked* `Intro`'s claim turns out to be a pending
> outbound remote, **cannot fire on a chain the application already
> holds**."

`staged.rs` returns a flat `AcceptError::Stale` for PENDING, documented
as a slice-7 boundary. Land §6.5 and §6.6 and leave that arm, and the
headline property does not hold **at `now`**: the flow self-heals about
five seconds later off the peer's next retransmit, which §6.5's eager
path does catch — but §6.4 promises immediately. Both sides of the
branch are therefore in slice 4, including that the winner-side `Stale`
is **the one refusal that keeps its §17.1 guard record** (§6.4:1401-1406)
where every other reverts it.

*Found by the independent test author*, working from the spec in an
isolated worktree, before the implementer had finished. That isolation
was added this slice precisely because it leaked in 3b; it paid on its
first use.

**A gap ruling 90 created that §6.5 predates**, recorded rather than
resolved: `mint_pending` without `start_attempt` is a pending with **no
initiation in flight**. §6.5 speaks of "in-flight outbound initiations"
and §17.4 of "the pending tables' dialled addresses" — those named the
same set before ruling 90 and no longer do, and the same ambiguity
reaches §6.4's PENDING branch ("if an in-flight outbound initiation
exists").

### The pattern is accelerating and that is the finding

*A ruling's blast radius includes the ruling* now has six instances —
64's own rationale, 70→73, 81→84, 87→90, 90's amendment, and 91's. **The
last three needed amending within a day of being written, and two of
those were rulings whose subject was this very defect.** The rate is the
signal, not the count: rulings written *about* under-scoped constructions
are not thereby immune to being under-scoped, and writing one while
holding the pattern in mind demonstrably does not help.

What has caught every instance is the same thing — **an independent agent
working from the spec text alone, with no sight of the reasoning that
produced the ruling.** Not review of the ruling, which has never once
caught one. That is an argument for keeping the blind authors even where
they look redundant, and against the intuition that a ruling the
maintainer has just reasoned through carefully needs less checking than
one they have not.

**Ruling 92 — §16.4's `read_identity` gains `now: Instant`.** It is the
one staged verb without it (`SPEC.md:4459`), while `authenticate` and
`accept` beside it both carry one. §6.5 step 4 cannot be implemented
without it: §6.6 steps 3 and 4 each record a §17.1 guard entry, which
needs the instant, and ruling 80 forbids the core reading a clock to
invent one.

**This is ruling 80's defect a third time, and the reason it survived
ruling 80 is worth stating.** At slice 2a `read_identity` genuinely did
not need `now` — an agent confirmed that *structurally* rather than by
assertion, tracing that its only entry-**removing** path could not reach
a verb requiring the instant. That was correct then. §6.5 step 4 makes it
an entry-**recording** verb, and the earlier proof does not survive the
change of role. So this is not an oversight repeated; it is a proof whose
premise expired. **Ruling 80's own generalisation — that an API listing
looks exhaustive and literal and is routinely neither — reaches this, and
ruling 80 did not.**

Precedent-following rather than novel: identical change, identical
reason, to a sibling of the two verbs ruling 80 already fixed, and there
is no alternative that does not put a clock read in a core forbidden to
have one. Cost is one shell call site plus ~30 mechanical call sites in
`src/core/tests.rs`.

*How it was found.* Two of the independent test author's 35 blind tests
failed on integration, and **both failures had one root cause** — this
gap — which the implementer had already reported as a blocker from the
other side, without either agent seeing the other's work. Thirty-three
passed unedited against an implementation written blind to them.

## Round 17 — slice 4 streams: the eleven open questions (2026/08/15)

Slice 4's planner produced seventeen unstated-scope hunts and eight
conflicts, ranked as eleven open questions by *cost of a wrong answer*.
Four were put to the maintainer; the rest are ruled here. **Every
provisional below was checked against the code, not only the spec**
(working rule 11), and the check overturned one of them.

**Ruling 93 — dropping a `RecvStream` retires the half at once, and the
connection-level true-up is to the stream window.** Conflict C2: §10.3's
list makes "handle abandoned (§16.2)" a retirement; §16.2's mechanism
makes abandonment merely *arm* a later one. §16.2's own final clause —
"an abandoned stream never wedges the connection window" — is **false
under the mechanism the same sentence describes**: a sender stalled at
the stream window sends no FIN and has no reason to reset, so no final
size is ever pinned, no retirement runs, and four abandoned 256 KiB
streams wedge the 1 MiB connection window for the connection's life.
`STOP_SENDING` is deferred to §19, so slither cannot ask the sender to
stop. §10.3's rationale describes this exact failure as "reachable in
honest operation by any application that cancels streams". The list is
right; the prose mechanism is wrong; the prose *promise* agrees with the
list. **Working rule 3's sixth instance, and the first where the prose
contradicts itself inside one sentence.**

On drop: free the half, tombstone the index at the watermark, and
advance that stream's connection-level contribution — immediately.

**The value is the stream window, not the high-water mark**, and the
distinction is the whole ruling. The planner's provisional was the
highest received offset; that leaks credit permanently. The sender
charges its connection window by *highest offset sent*; bytes lost or in
flight are already charged there and have not reached our high-water
mark, so truing up to the high-water mark re-creates C2's wedge at
smaller amplitude. The value that cannot leak is **the highest
stream-level limit we ever advertised** for that half — seeded to
`INITIAL_MAX_STREAM_DATA` (H15) and grown by §10.3's re-grant, never
`INITIAL_MAX_STREAM_DATA` as a fixed constant, because credit advanced
before the drop is credit the sender may legally have spent. It is the
least upper bound on what the peer could have sent without committing a
`FLOW_CONTROL_ERROR` (§10.5).

*The stronger argument is memory, not accounting, and it lands on the
same value.* §10.6 makes credit the **buffer commitment**. What we
committed to that half was its stream window; what we release by freeing
it is that same window. Advancing by the bytes that happened to arrive
would release less commitment than the memory actually freed. Over-
advancing is the safe direction and is bounded: at most one stream window
per abandoned half, and abandonment requires a claimed stream, which
`MAX_STREAMS` bounds.

Monotone and never double-counted: the advertised limit is ≥ the
high-water mark, satisfying §10.3's "absolute, not additive" and
"monotone bring-to-final"; and the tombstone makes every later frame for
that index inert (§9.2), so the contribution never moves again.

The sender still stalls at the *stream* window — §16.2 accepts that and
no slither frame can cure it — but the **connection** window and the
**cumulative stream allowance** are both released, which closes both
wedges. §16.2's mechanism sentence is amended to match, and so is §9.7's
free-condition bullet, which encoded the same overturned rule a third
time — found by grepping the *rationale* after the two obvious sites were
already fixed (working rule 4).

**Amended: "later frames are inert by §9.2's watermark" is true for uni
and false for bidi.** The first draft of this ruling said subsequent
frames for an abandoned index are inert by the watermark rule. That holds
for a **peer-opened uni** stream, where the receive half is the only half
this endpoint holds, so freeing it makes the stream fully closed (§9.7),
advances the watermark, and earns the peer its MAX_STREAMS grant. It does
**not** hold for a **bidi** stream: our send half is still live, the
stream is not fully closed, the watermark does not move, and the index
stays in the open set. Later frames there are neither implicit opens nor
watermark no-ops — they are discarded by §16.2's own "arrivals for it are
discarded", consuming no further credit because §10.3's true-up is
absolute and that stream's contribution already sits at its maximum. The
stream-level `FLOW_CONTROL_ERROR` check still runs against the frozen
limit, which is what stops an abandoned half becoming an unbounded sink.

Relying on the watermark alone **resurrects** an abandoned bidi receive
half on the next frame and double-charges the cumulative limit; relying
on a per-half tombstone alone never advances the uni watermark and never
grants the peer its credit. Both mechanisms are required, and the ruling
originally named only one.

*Found while checking this round's own amendments, not by review of the
ruling* — the same method, and the same result, as the seven instances
the round's closing section counts. **What bounds "subsequent frames are
inert"? The space does, and the ruling did not say.** That is defect
class 1 inside a ruling of round 17, which is defect class 2. The two
classes are the same class seen from two distances.

*Two tests, and the second is the one that matters.*
`a_dropped_recv_stream_releases_connection_credit_at_once` separates this
ruling from §16.2's reading. It does **not** separate this ruling from
the planner's provisional — both release at once. Working rule 9: the
degenerate implementation must red something, so
`a_dropped_recv_stream_trues_up_to_the_stream_window_not_the_high_water_mark`
is required, and it is the only test that pins the value.

**Ruling 94 — reassembly allocates lazily, coalesces on insert, and is
bounded connection-wide.** Conflict C3 / hunt H8: §10.6's mandate is
per-stream ("O(advertised credit)"), its worked example is per-connection
(a "1 MiB span" — that is `INITIAL_MAX_DATA`, while the per-stream window
is 256 KiB). Taking §10.6's own admissible option (a) literally at the
level its mandate names, 128 peer-opened uni streams eagerly allocate
**32 MiB** against 1 MiB of credit: a 32× remote memory amplification
produced by following the section that exists to forbid it. Slice 4
implements option **(b)** — coalesce-on-insert, `REASSEMBLY_CHUNKS_MAX` =
1024 — allocating only on arrival, so both bounds hold at once.

**The test must assert allocated capacity, not bytes received.** An
eager per-stream allocator receives few bytes and would pass a
bytes-received assertion for free — working rule 9's exact trap, in the
test that exists to close a memory vector. The core therefore needs a
test-visible accounting of reassembly capacity, and
`buffered_bytes_stay_within_the_connection_window_across_many_streams`
asserts on that.

**Ruling 95 — the core's stream verbs are keyed by an opaque `StreamRef`,
stable across install.** Conflict C1: §16.4 line 4491 returns a
`StreamId` from `open()`; §16.9 says wire stream IDs "are assigned at
establishment" because parity is fixed only then, and `id()` "returns
`None` until the connection is established". Both cannot be implemented
literally, since §16.9's whole point is that a `connect()`-created
connection is writable before install. The trap is not the missing
`Option` — it is a core that returns an internal index *typed as*
`StreamId` and remaps at install, leaving every handle holding a stale
key: a build that passes a pre-establishment test and a post-establishment
test and fails only "open early, write late". `core::Connection::
stream_id(&self, r: StreamRef) -> Option<StreamId>` is §16.9's accessor
that the shell's `id()` reads. Pinned by
`an_early_opened_stream_keeps_its_handle_across_install`.

**Amended within the hour, by the act of applying it.** This ruling was
written as "§16.4's five signatures are amended — ruling 71's shape
again, a signature list that omits a member." Opening `SPEC.md` to make
that edit showed **eleven** sites: the five verbs, plus `accept`, plus
the new accessor, plus the **four `ConnEvent`s that name a stream** —
`StreamReadable`, `StreamWritable`, `StreamFinished`, `StreamReset`. The
events are not optional to convert: if they carry `StreamId` while
handles hold `StreamRef`, the shell cannot match a wakeup to its waker
before establishment, which is exactly when §16.9 says work is in flight.

**Written in the round that observes this class is recursive, inside the
ruling that cites ruling 71 for it.** The count in a ruling about a
miscounted list was itself a miscounted list. **Seventh instance of "a
ruling's blast radius includes the ruling", and the first caught in the
maintainer's own text before it reached an agent** — by the same method
as all six before it: opening the file, not re-reading the reasoning.

**Ruling 96 — `BiStream` the type lands in slice 4; its `AsyncRead`/
`AsyncWrite` impls and `compat/` stay in slice 8.** Conflict C7:
`PLAN.md`'s slice-8 row lists `BiStream`, §16.2 and ruling 55 make it
`open_bi`'s return type, and slice 4 owns `open_bi`. A slice cannot own a
verb and not own its return type without either contradicting ruling 55
or forcing a breaking public API change between two shipped slices.
`PLAN.md`'s row means the *composability* of `BiStream`, not its
existence. A plan/spec boundary error settled by moving the boundary;
cost ~40 lines. `PLAN.md` amended.

**Ruling 97 — the receive path's semantic checks are ordered
legality → watermark → limit → final size → flow control, and all four
watermarks are maintained.** Hunts H3/H4/H5, question Q4. §8.4's
per-frame error lists are sets with no evaluation order, and one crafted
frame can trip several; all of them kill the connection, so the only
observable difference is **the error code on the wire** — which is what
Appendix B asserts on and what a peer's operator reads. A list read as
exhaustive is still not a list read as ordered.

The load-bearing position is the first. For a locally-opened uni stream
the peer may *never* send STREAM, so a STREAM frame naming a fully-closed
local-uni index satisfies both §9.2's watermark no-op and §8.4's
`STREAM_STATE_ERROR` — silent ACK versus kill. **Legality wins.** The
watermark answers *which index*; the state error answers *who may send*.
A frame the peer could never legally send at any index is not a late
retransmission of anything, and putting the watermark first would delete
a violation check for exactly the streams an attacker can most cheaply
name.

*What else must be true for "legality first" to be safe* (working rule
11's sharpening): the check must be decidable **without consulting the
stream table**, or a freed stream could confuse it. It is — §9.1's id
encodes direction and opener parity, and combined with our established
role it is a total function of the id alone. **That is a second consumer
of ruling 106**: the check is only total because the role is carried on
`Install`.

Flow control runs last so the ledger is consulted exactly once per frame,
after the frame is known otherwise legal — which is what §8.4 and §9.6
already require when they put the credit bound check before any true-up.
`FINAL_SIZE_ERROR` precedes it because a frame contradicting a pinned
final size is a statement about a stream we already fully understand, and
answering it with a credit code would mislead.

All four watermarks are maintained (H3). §9.2 states the construction for
four spaces and writes its rationale for the two peer-opened ones; the
other two serve §8.4's separate rule that "credit for a fully-closed
stream is a valid no-op". An implementer who keeps two will meet a
`STREAM_STATE_ERROR` where §8.4 promised a no-op.

Pinned by `a_stream_frame_on_a_closed_local_uni_space_is_a_state_error`
and one test per adjacent pair.

**Ruling 98 — not a ruling: §7.4 already enumerates the quiet set, and
the plan's table is wrong on two rows.** Question Q5 / hunt H7 asked
whether MAX_STREAMS_BIDI/UNI are non-marking, since §10.3 says "credit
frames" while its subject is the other two, and asserted that "**nothing
says what RESET_STREAM and STREAM use**". Both premises fail at
`SPEC.md:1953–1958`, inside §7.4 — the section named for this exact
distinction:

> - **`seal`** marks `last_send`: packets carrying at least one
>   first-transmission STREAM frame or DATAGRAM frame (fresh application
>   sends), and the keepalive (§7.5).
> - **`seal_quiet`** … the **quiet set**: pure ACKs, PTO probes,
>   retransmissions, the credit frames (MAX_DATA, MAX_STREAM_DATA,
>   **MAX_STREAMS_BIDI/UNI**), **RESET_STREAM**, and CLOSE.

All four credit frames are named. **RESET_STREAM is named** — the plan's
table put it on the marking `seal` "by omission from §10.3", which is
backwards. And STREAM is marking only on **first transmission**;
retransmissions are in the quiet set by the same list, a distinction the
plan's table does not carry. `src/core/connection/session.rs:412–414`
already quotes the correct set in `seal_quiet`'s doc comment, written in
slice 3a.

So the table slice 4 must implement is: STREAM first transmission →
`seal`; STREAM retransmission → `seal_quiet`; RESET_STREAM → `seal_quiet`;
all four credit frames → `seal_quiet`; every one of them ack-eliciting.
Marking is a property of the **seal**, not the frame, so a packet mixing
a fresh STREAM frame with credit frames is marking. That last sentence is
the only part of this that no section states, and it follows from
`seal`/`seal_quiet` being a per-seal choice.

`a_max_streams_only_packet_does_not_defer_the_keepalive` is still worth
writing — it now pins a stated rule rather than a provisional.

**This is working rule 4 inverted, and it is worth naming.** The rule
says to grep for the rationale and not only the token. Here the hunt
greped the token in the sections *about* credit (§10.3, §10.4) and the
answer lived in the section about *sealing* (§7.4). A hunt scoped to the
subject matter missed an enumeration filed under the mechanism. The plan
was right that §10.3's scope is ambiguous; it was wrong that the
ambiguity was unresolved, and the cost of shipping its table would have
been a marking RESET_STREAM deferring keepalives forever.

**Ruling 99 — one `StreamOpened` per newly-opened stream, and §10.4's
limit check runs before the opens.** Hunt H11: §16.4's event carries no
id, and §9.2 opens index `N` "and every lower-numbered not-yet-open
stream of that space", so a peer whose first frame names index 5 opens
six streams in one packet. One event for six means the shell must loop
`accept()` until `None` on every wake or lose five — a lost-wakeup that
manifests only under the reordering the tests inject. `accept(dir)`
returns one stream per call, and §9.2's own rationale worries about "a
phantom `StreamOpened` for a finished stream", singular.

*What bounds the burst* — the question working rule 8 says to ask of any
construction. One event per stream against an unbounded index would be an
event-queue amplification vector: one small frame, 2⁶⁰ events. What
bounds it is §10.4's cumulative limit, at most 128 for uni today — **and
only if the limit check runs before the opens are performed**, which is
where ruling 97's order does load-bearing work rather than merely
choosing an error code. An implementation that opens first and validates
after is both a wrong error code and an unbounded burst.
`an_implicit_open_of_six_streams_emits_six_stream_opened_events`, and a
second test that a frame naming an index above the limit emits **zero**
events before killing the connection.

**Ruling 100 — an empty, FIN-less STREAM frame opens its stream.** Hunt
H10 / conflict C4: §9.2 makes the first STREAM frame the open; §9.5 calls
an empty frame without FIN and without data "a no-op (tolerated, never
emitted)". §9.2's rule is about the **frame**; §9.5's no-op is about the
**data** — it delivers no bytes, pins no final size, consumes no credit.
Reading §9.5 as suppressing the open would make the open set depend on a
payload property §9.2 never mentions, and would make a legitimate
zero-length write on an open stream and a stream-creating frame
indistinguishable in the codec. Reachable only from a foreign or hostile
peer, and the allowance it consumes is the peer's own and bounded by
§10.4. `an_empty_finless_stream_frame_opens_its_stream`.

**Ruling 101 — `StreamId` and `Dir` are public; `StreamsExhausted` stays
`pub(crate)`.** Hunt H13: three symbols the spec uses and never defines —
ruling 89's shape, three times over. `StreamId` is forced public by
§16.2's `SendStream::id(&self) -> Option<StreamId>`. Minimum surface:
`Copy + Eq + Ord + Hash + Debug + Display`, `index() -> u64`,
`dir() -> Dir`, `initiated_by_connection_initiator() -> bool`, `u64`
round-trip, and **no constructor that lets an application mint an id for
a stream it does not own**.

**The planner's provisional was internally inconsistent and the code
settles it.** It recommended `Dir` be `pub(crate)` "unless 4b finds a
public signature that needs it" while listing `dir() -> Dir` in
`StreamId`'s public surface — a publicly reachable signature naming an
unreachable type, which is `private_interfaces` and therefore a hard
error under our `-D warnings` gate. Either `Dir` is public or `StreamId`
loses the accessor that makes it useful for the logging and correlation
`id()` exists to serve. `Dir` is public. Two variants, closed by the
protocol, no `#[non_exhaustive]` — that would cost every user a wildcard
arm forever for a set that cannot grow.

Both live in the `pub(crate)` core and are re-exported from `lib.rs`
beside `ConnectionId`/`IntroId`/`Timestamp`, whose comment (`lib.rs:161–
166`) already states this exact reasoning: "a public signature naming an
unreachable type is a rustdoc break, not merely a lint."

`StreamsExhausted` stays `pub(crate)`: §18.1's taxonomy is closed (ruling
61), and §16.2 specifies that `open_bi`/`open_uni` "wait for MAX_STREAMS
allowance when the cumulative limit is exhausted" — the shell converts
the core error into a park, so no public verb can ever return it.
Breaching a closed taxonomy for a condition the public API is specified
never to surface is the wrong trade.

**Ruling 102 — §10.4's "≤ 8" low-allowance threshold is
`STREAMS_CREDIT_BATCH`, one constant used twice.** Hunt H6: the first
trigger names the constant, the second writes the literal, and §10.2
declares exactly one constant of value 8. **No test can separate the
readings at today's values** — which is why it needs the ruling rather
than a test: an unnamed magic number with no home in the constants table
would silently decouple from the named one the first time anyone tunes
it, and a batch of 8 with a headroom of 8 is one batch of slack, which is
what the batching rationale means. §10.4 amended to name the constant in
both places.

**Ruling 103 — the flow-control constants are three kinds of thing, and
the tables group them as one.** Hunt H16 found §10.2's five-row table
mixing four **wire constants** (the initial windows: unnegotiated, so
both ends must assume the same value, and changing one corrupts the
peer's accounting immediately) with `STREAMS_CREDIT_BATCH`, which §10.2's
own next clause calls "receiver policy" — two peers running different
batch values interoperate perfectly. `tests/spec_constants.rs` pins all
five as wire pins, and `CLAUDE.md` says a red wire pin "needs a ruling,
not an updated expectation", so a local tuning knob currently gets a
ratification round it does not need, and implies an observability it does
not have.

**Extending the hunt one section further than it was run** — working rule
8 says to ask what bounds a list, and H16 asked it of §10.2's table only
— `REASSEMBLY_CHUNKS_MAX` (§10.6) is a *third* kind: receiver policy like
the batch, but **externally observable**, because a peer that fragments
past one receiver's ceiling is killed and past another's is not. It is
shipped "ratified-but-revisitable" and is a tolerance.

Keep all values and all locations unchanged — moving them is wire-pin
churn for nothing — and **mark the kind** in `src/constants.rs` and in
`tests/spec_constants.rs`: wire constant / invisible policy / observable
policy. The next person to touch one then knows which kind of change they
are making. §10.2 already says which kind it is; only the table's
grouping disagrees.

**Ruling 104 — §10's violation set has three members, and §10.5's
closing sentence must be amended with it.** Hunt H17: §10.5 is titled
"Violations", sits inside the chapter that defines all three, and
enumerates two — `FLOW_CONTROL_ERROR` and `STREAM_LIMIT_ERROR` — while
§10.6, the next section, defines the third: reassembly ranges exceeding
`REASSEMBLY_CHUNKS_MAX` after coalescing is a `PROTOCOL_VIOLATION`. **This
is ruling 64's defect at the same arity** — "two things this rule does not
reach" when there were three — and the missing member is the memory-safety
bound. An implementer building §10's violation handling from §10.5 builds
two of three.

The subtler half, and the reason it is hard to see: §10.5 closes with
"There is no tolerance band; the limits are exact", which is true of both
violations it lists and **false of the omitted one** —
`REASSEMBLY_CHUNKS_MAX` is explicitly a tolerance, admissible
implementation (a) makes it unreachable entirely, and §10.6 ships it
revisitable. The section reads complete because it is internally
consistent with its own omission. **Working rule 4 is the operative
warning**: a fix that adds a third bullet and leaves that sentence
standing ships a self-contradicting section, which is the failure working
rule 4 was written from. Both are amended together. Slice 4 implements
all three and its tests treat the set as three-membered.

**Ruling 105 — slice 4 discharges the receive-path half of Appendix B's
tombstone obligation; the loss-driven variant is owed to slice 7.**
Conflict C8. The obligation reads "free a stream, **drop the ACK**, and
let the peer's **PTO retransmission** re-name it" — §12 is slice 5 and
§13 is slice 7, so neither stimulus exists yet. Injecting the duplicate
STREAM frame through `FlakyPolicy`'s duplication exercises the identical
receive-path code with a *stronger* stimulus, since it arrives with no
delay. Slice 4 writes that test and records the loss-driven variant in
its exit note as owed to slice 7. An obligation quietly marked done by a
weaker test is how a slice ships a gap; recording it is the difference
between a downgrade and a debt.

**Ruling 106 — `Install` carries the connection's role.** Slice 4's
carried finding, and the highest-value thing in the routing track's
report. §6.7 fixes initiator-ness "for the life of the connection:
stream-ID parity (§9.1) is fixed by this outcome", and §6.6 step 4 makes
a peer that *dialled* admit and write msg2 **as responder**. A connection
core deriving parity from "I was created by `connect()`" is therefore
wrong on exactly that path — and silently, since both ends still agree on
every stream they open themselves and disagree only on parity. Per ruling
89 it cannot be recovered from hiss afterwards: `Handshake::Seal` is an
associated type with no bounds.

Verified against the tree rather than assumed (working rule 11):
`src/core/mod.rs:195–198` — `pub struct Install<C: Handshake> { pub
session: EstablishedSession<C> }`, one field, no role, and `grep` for
`enum Role`/`is_initiator` across the cores returns nothing. §16.4's
`struct Install` (`SPEC.md:4479`) is amended to carry it, and ruling 97's
legality check is its second consumer.

**Ruling 107 — slice 4 is cut into 4a (core) and 4b (shell + stories),
along the core/shell seam.** The planner re-derived round 12's argument
rather than transferring it, and checked the three ways it could have
failed to transfer. The one that nearly holds is "the API is spec-given
this time, so the test author is not guessing" — and it does not survive
contact with the detail, because a story test cannot write
`conn.open_bi().await?.split()` without rulings 95, 96 and 101, none of
which are in the spec. After the cut those are compiled and green in 4a
before 4b's test author starts, and the author reads them off a real
crate.

Two further arguments, both from this project's own failures: a combined
slice puts four concurrent agents around `src/core/connection/`,
`src/shell/` and `tests/` at once, with two of them wanting
`src/core/connection/mod.rs` — the slice-2a accident's exact shape, which
working rule 6 makes absolute. And 4b is where the seam review found four
defects, two unreachable from all 451 tests by construction (working rule
13); its content is per-`StreamId` waker maps under `RefCell` keyed by a
value the *application* controls, cancel-safety of four new `async fn`s,
and drop semantics where dropping a `SendStream` emits a wire frame and
dropping a `RecvStream` mutates the flow-control ledger. Small and risky
argues **for** a review boundary.

The subsystem cut (§9 | §10) was considered and rejected: §10 is threaded
through §9, not layered on it — §8.4's STREAM error list contains
`FLOW_CONTROL_ERROR`, §10.6 makes the reassembler's bound a flow-control
quantity, and §9.6/§9.7's true-ups are §10.3 rules living inside §9
sections. It would produce a first half knowingly wrong and a second half
that edits all of it.

**4a closes zero stories and that is in its brief, not discovered at
review.** It closes eleven Appendix B obligations and three named
precursors (`s12_precursor_*`, `s13_precursor_*`, `s17_precursor_*`),
named as precursors so that calling one of them S12 cannot let 4b ship
without the handle-level test the story asks for.

### What this round says about the process

Fifteen rulings, of which the maintainer took four and the rest followed
the planner — **except one, where checking the code overturned the
provisional outright** (ruling 98) and one where the provisional
contradicted itself (ruling 101). Both were caught by opening a file, not
by re-reading the argument. That is working rule 11's whole content, and
it is now the second consecutive round where the mechanism check changed
an answer rather than confirming it.

The plan's own hunt found thirteen prior instances of the unstated-scope
class and added seventeen more candidates in one slice. **Rulings 99, 103
and 104 each extend a hunt the planner ran one step further than the
planner ran it** — what bounds the event burst, what bounds the
constants-table grouping, what the omitted violation does to the
sentence that closes the section. The defect class is not merely
frequent; it is *recursive*, and a hunt for it is itself a construction
with a scope worth asking about.

## Round 18 — slice 4a's blind test author reports (2026/08/15)

Seventy-three tests written from the spec in an isolated worktree, against
an implementation their author never saw. The report found **three defects
in the contract I wrote** and two spec ambiguities. Every one is real.
Recorded here as rulings so 4b and slice 5 inherit decisions rather than
the arguments.

**Ruling 108 — `read`, `finish` and `abandon_recv` take `now: Instant`.**
§16.4 gives `read(id, buf)` and `finish(id)` no instant, and my contract
copied that. But `read()` is a **mutating** call — it drains the
contiguous prefix — and §10.3 makes **consumption** the thing that
advances credit, so a read can cross the re-grant trigger and owe a
MAX_STREAM_DATA or MAX_DATA. §16.7 settles what happens next in terms:

> Sealing — commit included — executes **within the mutating call that
> triggers it** (`handle_timeout`, `handle_datagram`, the application
> surface), never lazily inside `poll_output()`.

`poll_output()` carries no instant, `seal_quiet` requires one because
credit frames are ack-eliciting and arm the death clock (§7.4), and ruling
80 forbids the core inventing one from a clock. So the instant must arrive
on the verb. The three ways out the author enumerated are not equal: (2)
deferring the seal contradicts §16.7 and would make a credit frame's
emission time depend on unrelated traffic; (3) caching the last observed
`now` seals with a stale `last_send`, which §7.4's liveness accounting
then reads. Only (1) is consistent with rules already ratified.

`finish` queues a FIN-bearing STREAM frame — the marking `seal` (§7.4) —
and `abandon_recv`'s true-up can both cross §10.3's threshold and fully
close a peer-opened uni stream, which owes a MAX_STREAMS grant (§10.4).
Same argument, same conclusion.

**This is ruling 80's defect a fourth time and ruling 92's a second, in
the same API listing.** Ruling 92 already recorded that ruling 80's own
generalisation — an API listing looks exhaustive and literal and is
routinely neither — reaches further than ruling 80 applied it. It reached
here too, and ruling 92 did not check. **The general form, which is now
worth stating once instead of rediscovering per verb: in a sans-io core
whose sealing is synchronous, `now` belongs on every verb that can emit a
frame, and "can this emit?" is a question about §10 and §9.7's
consequences, not about what the verb is named.** `read` looks like a
reader; it is a credit-advancing, frame-emitting mutation.

*Found the way the previous three were:* an independent author checking
the contract against `CLAUDE.md`'s stated invariant — "`now: Instant` is
an argument on every mutating call" — rather than against the listing.

**Ruling 109 — `abandon_recv(&mut self, now, r)` is the verb ruling 93
specifies and ruling 95 omitted.** Rulings 93 and its amendment both
legislate core behaviour "on abandoning the receive half", and no verb in
§16.4 or in my contract performs it; the shell's `RecvStream::drop` has
nothing to call. Ruling 95 counted "eleven sites" for the `StreamRef`
key — five verbs, `accept`, `stream_id`, four events — and this is a
twelfth, with `on_ack_range`/`on_lost_range` (ruling 113) making fourteen.

**Ruling 95's count has now been wrong twice**, each time discovered by
building against it rather than by reading it: once by me while applying
it, once by the test author while writing to it. Its own amendment
observed that it was "written in the round that observes this class is
recursive" — and then undercounted again in the amendment. The lesson is
not to count more carefully. It is that **a count in a ruling is a claim
about a listing, and the only thing that checks a listing is compiling
against it.**

The name is the test author's guess, kept deliberately: seven of its tests
call it, and adopting the guess costs one shim line where renaming costs
seven edits and a reconciliation.

**Ruling 110 — a zero-length `write` is a no-op returning `Ok(0)`, and
`Ok(0)` means *blocked* only for a non-empty input.** The contract made
`Ok(0)` mean "blocked by credit; the shell parks", and a zero-length write
returns `Ok(0)` under any natural implementation — parking a writer that
has nothing to wait for, since no credit arrival will ever unblock it.
Not hypothetical: ruling 100 makes empty STREAM frames legitimate protocol
elements, and §16.2's `AsyncWrite` is handed empty buffers by ordinary
`tokio::io` combinators. The shell knows its own buffer length, so the
check belongs there and costs one condition. Documented on `write`.

**Ruling 111 — RESET_STREAM's `final_size` is the end offset of the
highest byte actually *transmitted*.** §9.6 read "the number of bytes the
stream would have carried (the end offset of the highest byte sent, or 0
if none)" — and the phrase and its own parenthetical disagree the moment
`reset()` follows a `write()` that congestion control has not yet
released, which is ordinary operation, not an edge case. Ruling 56's
"accepted bytes are already in send state" pulls the other way.

Only "transmitted" is consistent with both neighbours. Pending bytes were
never on the wire, so counting them pins a final size the receiver can
never reach — and §8.4 makes a FIN or data conflicting with a pinned final
size a `FINAL_SIZE_ERROR`, so the receiver would be holding a stream it
can never complete. In-flight bytes *may already have arrived*, so **not**
counting them would make legitimately-received data exceed the final size
and kill an honest peer's connection. The window between the two readings
is exactly the accepted-but-unsealed set. §9.6 amended.

**Ruling 112 — `accept(dir)` claims in FIFO open order.** §9.2 opens a
*run* of indices from one frame and ruling 99 fixed the event count;
neither says which stream a claim returns. Working rule 8's shape, and it
matters because an application will assume ascending order — the natural
assumption, and what QUIC implementations do — while relying on nothing.
FIFO in open order matches `recv_message`'s "oldest complete unclaimed"
and keeps §9.8's second claim verb drawing from the same supply in a
defined order. Decided now rather than at 4b, so no example bakes in an
order the core does not promise.

**Ruling 113 — `on_ack_range`/`on_lost_range` surface on `Connection`,
and two of ruling 97's four watermarks are scheduled debt, not coverage.**
My contract §5 asserted that defining these on the send half in slice 4
"is what makes GC and watermark logic testable now". The author checked
and it does not: they sit on a type the contract never surfaces, with no
path from a `Connection`, so nothing in 4a can call them. **Working rule
11's shape in a contract rather than a ruling** — a rationale naming a
mechanism that does not connect to the thing it claims to enable.

They become `pub(crate)` verbs on `Connection`, taking `now` (an ACK can
complete a send half, fully close a stream, and owe a MAX_STREAMS grant),
still uncalled from the wire until slice 5 wires §12 to them.

The consequence the author traced from it must be recorded rather than
silently carried: a locally-opened stream fully closes only on
acknowledgement, so **the local-bidi and local-uni watermarks cannot
advance in 4a at all, and a build maintaining only the two peer-opened
watermarks passes all 73 tests.** Ruling 97's H3 warning is undefended by
anything writable here. Ruling 97's named test is correspondingly
*reduced* — it pins legality-before-limit and legality-before-flow-control,
not legality-before-watermark — and says so in its own doc comment, which
is the difference between a reduced test and a name that is not a pin.

**Owed to slice 5**, three tests: legality-before-watermark on a closed
local-uni index; "credit for a fully-closed stream is a valid no-op"
(§8.4) on a stream we can send on; and a locally-opened watermark
advancing at all. Plus §10.4's "closing streams we opened must not inflate
the peer's allowance" (RFC 9000 §4.6's scope rule), unreachable for the
same reason. **Owed to slice 7**: ruling 98's STREAM-retransmission row —
the only row where one frame type takes two different seals, and so the
one most likely to be got wrong — and ruling 105's loss-driven tombstone
variant.

### Two process defects, one of them mine and new

**The worktree was cut one commit before its own brief's inputs.**
`CONTRACT-4a.md` — the file the brief calls binding — did not exist at
`fdf5972`, the commit the test author's worktree was created from; it
landed on `main` at `74fa5f2`. The author spent ten minutes reconstructing
the API from `PLAN.md` and Round 17, and was rescued only because an
unrelated mid-flight message about ruling 93 revealed the file existed, at
which point it read the contract out of `main` with `git show`.

Two of its reconstructed guesses were **semantic, not cosmetic**:
`ConnEvent::StreamReadable { stream: … }` against the contract's
`{ r: … }`, and **no `Ok(Some(0))`/`Ok(None)` distinction for `read` at
all** — the latter being precisely the convention whose inversion hangs a
reader forever on a finished stream. Had that message not gone out for an
unrelated reason, 73 tests would have been written against a guessed API
and the integration would have looked like a disagreement about design
rather than a missing file.

**Worktree isolation solved the blindness leak of slice 3b and introduced
a new failure mode in doing it: the isolated agent sees a *commit*, not a
working tree.** Uncommitted brief inputs do not travel. This is now
working rule 14.

**§11.8's release-mode requirement has no gate.** `PLAN.md` §11.8 requires
the arithmetic test to run in release as well as debug, because
`debug_assert`-based overflow checks compile out — and FIXES-3b §4 is the
precedent where a bug cost only in release. A `#[test]` cannot select its
profile, so this is a CI obligation, and `CLAUDE.md`'s gate table did not
have one. Added: `cargo test --release`.

### What the report says about the arrangement

The author declined the brief's instruction to use
`#[tokio::test(start_paused = true)]`, on the grounds that a sans-io core
takes `now` as an argument and has no virtual time to pause — the
attribute would attach a runtime nothing awaits. It is right, it matches
`src/core/connection/tests.rs`'s existing practice, and it flagged the
deviation rather than making it silently. **Eight agents have now declined
an instruction and all eight were right.**

It also declined to write two tests it could have written: ruling 102's
(the ruling itself says no test can separate the readings, so a test would
have produced a name that is not a pin) and conflict C-b's (the correct
behaviour was undecided, and writing a test would have resolved it
silently). **Not writing a test is the harder call and the right one**,
and both refusals are recorded in its report with the reason.

## Round 19 — slice 4a integration (2026/08/15)

**Ruling 114 — §16.7 bounds *where* a seal happens, not *how much* is
sealed; in slice 4, with no congestion bound, a mutating call flushes
everything the ledger admits.** Two of the test author's 73 tests failed
against the implementer's build, and both failed for one reason neither
agent could resolve alone — which is the arrangement working, not
failing.

§16.7 says sealing "executes within the mutating call that triggers it …
never lazily inside `poll_output()`". It is a rule about **placement**.
It does not say a mutating call must flush the whole send buffer, and the
implementer took the stronger reading — correctly, because slice 4 has
nothing else that would ever send: §14's congestion window is slice 7,
and there is no send clock. A partial flush would strand the remainder
indefinitely, which is a liveness hole, not a design choice.

The consequence is the part worth ratifying, because it is not obvious:
**two sequential `write()` calls can never contend in slice 4, in any
conforming build.** By the time the second runs, the first stream is
already on the wire. §8.5's round-robin therefore governs the fill
*within one pump*, never across calls.

*Two tests, two different outcomes, and the difference is what each
property needs.*

`the_stream_fill_serves_pending_streams_round_robin` **is** reachable —
through §16.9's pre-install writes, where nothing can be sealed because
no session exists yet, so two streams accumulate and the install pumps
both. Its fixture was rewritten to build that contention; its assertions
are untouched. **Verified by mutation, because rewriting a test author's
fixture puts the burden of proof on the integrator:** changing the fill's
requeue from `push_back` to `push_front` — serve one stream to exhaustion
— reds it, and restoring greens it.

`credit_frames_precede_the_stream_fill_in_a_packet` is **not** reachable,
and I established that by trying to build it rather than by accepting the
report. The construction I attempted — an inbound packet that both raises
our stream window and retires enough peer bytes to owe a MAX_DATA — fails
on a premise I had wrong: **a blocked `write` means the core *refused* the
bytes**, so they stay with the caller and there is no pending stream data
for the credit frame to share a packet with. There is no slice-4 state in
which stream data is pending across calls. It is kept, with its own
"asserted nothing" guard intact, marked `#[ignore]` naming slice 7 — the
slice whose congestion bound creates exactly that state. Deleting it
would lose the obligation; leaving it running would fail a correct build.

**The seam to slice 7 is one call site**: `pump()` gains a second bound,
and both the intra-packet round-robin and the credit/fill coincidence
become ordinary rather than exotic.

*A note on what is and is not pinned.* `STREAM_FILL_QUANTUM` survives
deletion — `room` is already bounded by the packet's remaining plaintext,
so `room.min(QUANTUM)` and `room` behave identically at a 1 KiB quantum
under a 1170-byte ceiling. The rotation is the mechanism that makes the
round-robin real, and the rotation *is* pinned. The quantum is
implementation-defined (§8.5) so this is not a defect, but its own doc
comment claims "two streams alternate within one [packet]", which no test
checks and which the quantum does not by itself produce. **Working rule
9's shape found by mutating rather than by reading**: the first mutation
I tried was the constant, and it survived.

### Three fixture defects, and what they say about harnesses

Three of the author's tests failed on a **precondition**, not on the
behaviour under test: each delivered a single STREAM frame carrying
2048/4096/8192 payload bytes, and `MAX_PLAINTEXT` is **1170**, so §3.1's
size gate dropped the datagram **silently** — no error, no trace, no
counter (§3.1 requires exactly that silence). The failure therefore
presented as a memory leak in the reassembler. The author's own harness
already had the chunking helper; the three now use it.

**This is working rule 13 one layer down from where it was written.**
Rule 13 says `FlakyWire` models a network and not a socket. Here the
in-crate `Solo` fixture models a **frame stream** and not a **packet**, so
an over-`MAX_PLAINTEXT` frame is indistinguishable from a lost one. Three
tests aimed at §10.6 — the section that exists to close a memory
amplification vector — asserted nothing, and would have shipped green if
the implementer had not diagnosed the precondition rather than the
symptom. *The harness's own abstraction is a coverage boundary, and it
is invisible from inside the tests it enables.*

### What the split produced this time

**68 of 73 blind tests passed on first contact**, with zero name, type or
value mismatches beyond the `now` arity that ruling 108 introduced *after*
the author had finished. Of the five failures, **three were defects in the
tests, two were a genuine spec conflict, and none was a defect in the
implementation.** That distribution is new: in every prior slice the
failures were implementation defects or spec gaps.

The implementer also declined to fix `src/shell/driver.rs`, whose
exhaustive `ConnEvent` match the six new variants break. It is not on its
path (working rule 6), so it reported the exact arm needed and ran the
gates in a detached scratchpad worktree with the stub applied *there* —
leaving the delivered tree with no byte written under `src/shell/`. **Nine
agents have now declined an instruction or a convenient shortcut, and all
nine were right.**

**Ruling 113's signature was already wrong when I wrote it**, and the
implementer said so: `on_ack_range(now, r, range)` drops the FIN flag the
send half needs to reach `DataRecvd`, so §9.7's send-side GC would ship
untested. It infers the flag as `range.end == final_size`, exact for every
frame this implementation emits — but **§8.7 lets a retransmission re-frame
ranges freely**, so a range ending at the final size need not have carried
the FIN. Slice 5's sent-packet map is where the answer actually lives, and
slice 5 must either call `SendHalf::on_ack_range(range, fin)` directly or
restore the flag to the `Connection` signature. Recorded rather than fixed
now, because the right shape depends on §12's map, which does not exist.

---

## Round 20 — slice 4b planning: the shell stream surface (2026/08/15)

The 4b planner returned seven open questions and five conflicts. The
largest — that §16.9's early sends have **no reachable handle** — is a
finding about the ratified public surface, not about slice 4, and it is
ruled here rather than carried.

**Ruling 115 — a stream handle *is* a handle: `SendStream`, `RecvStream`
and `BiStream` count for §16.3's driver lifetime and for ruling 88.**
Each acquires on construction and releases on drop, exactly as
`Connection` does, and the driver lives while any of them lives.

The planner recommended this and reached it partly from §16.3:4409, which
enumerates *"`Endpoint`, staged objects, `Connection`, and stream
handles"* as thin clients and then says *"the driver lives while any
handle lives"*. **That argument does not hold, and it is worth saying
why**, because the conclusion is right for a different reason. **Staged
objects are in that same list and are explicitly excluded**: ruling 62
says a staged object's verb is *"a round-trip to a driver it does not
keep alive"*, which is what makes `IntroError`'s `EndpointDropped`
correct rather than an omission. So §16.3:4409's enumeration is **not** a
list of things that keep the driver alive, and reading the next sentence
as ranging over it proves too much.

The authority is ruling 62's **test**, which was written to be applied:

> A future that changes protocol state when dropped is a handle; one that
> does not, is not.

Dropping a `SendStream` puts RESET_STREAM on the wire. Dropping a
`RecvStream` retires the receive half, sets ruling 93's tombstone and
trues up the flow-control ledger. Both change protocol state, and neither
resembles the `closed()` future that *"owns nothing and merely
observes"*. The test settles it without needing the list.

The consequence that decides it independently: **a `Drop` that must emit
a frame needs a driver to emit it.** Under the alternative, dropping the
last `Connection` fires §16.2's `close(NO_ERROR, "")` underneath a live
`SendStream`, and that stream's RESET_STREAM is silently lost — in the
*ordinary* shape of a task that owns a stream and has let the connection
handle go. Ruling 88 then governs the genuinely-last drop, and nothing is
transmitted, which is already the ratified answer for that case.

**§16.3 is amended**: the four-item list at 4409 gains a sentence saying
what it does and does not enumerate. This is defect class 1 in the
maintainer's own text — *a stated construction with an unstated or
contradicted scope* — and it is the second time a §16 list has been read
as exhaustive when it was not (ruling 71 was the first).

**Ruling 116 — §16.9's early sends are a guarantee about the *core*. The
shell exposes no pre-establishment handle in wire v1, and this is
deliberate.**

The planner's conflict C-B is real. §16.9 says queued work before
establishment is *"ordinary work"*, §6.7 says *"queued sends … live in
the connection core's stream state and pump on whichever session
installs"*, and §16.2 annotates `id()` as `None` *"before establishment"*
— three texts that presuppose an application holding something it can
write to before a session exists. **The shipped shell hands out no such
thing**: `Driver::establish` constructs the `Connection` only on
`ConnEvent::Established`, and the accept path reaches one only after
§6.2's stage 3. I checked both.

Three reasons the shell is right and the prose overreached.

1. **It buys no wire latency.** §16.9 itself forbids emitting a frame
   before install — *"no frame is emitted before install (nothing sends
   until a session exists)"*. The handshake RTT is identical either way.
   What an early handle buys is that the application need not await
   `Connecting` before writing: **one task wake-up**, not one round trip.
2. **A pre-establishment `Connection` falsifies a ratified accessor.**
   §16.2 declares `session_id(&self) -> SessionId` — **total**, no
   `Option`, no `Result` — and ruling 89 defines it as hiss's value
   *derived from the handshake hash*. Before the handshake completes
   there is no such value. The shipped handle captures it at
   construction, which is what lets it keep answering after §15.2's
   linger has dropped the session. Making a total accessor fallible to
   buy a task wake-up inverts the trade.
3. **The core half is load-bearing and stays.** §6.7's tie-break and
   slice 7's replacement both install a session *underneath a connection
   core that already holds queued sends*. That crossing is real, is
   reachable, and is what `StreamRef` exists for — ruling 95's "stable
   across install" property is exercised there, not at first contact.

So §16.9 keeps its mechanism and loses its implied audience. **§16.9 is
amended** to state that the pre-establishment window is crossed *inside*
the core — by a tie-break's `Install` and by a replacement — and that no
route to it is published on the v1 application surface. A later line may
add one; adding it is additive.

**`id()` keeps its `Option`, and the handle caches.** Two parts. The
handle caches the id the first time the core answers `Some`, because the
core's `stream_id(r)` is **not monotone** — `Streams::after_half_freed`
removes the entry, so an uncached `id()` answers `None` again once the
stream fully closes, which is reachable in slice 4 for a peer-opened uni
stream read to EOF. That would be the opposite of `remote_static()`'s
keeps-answering property, for the same reason. And the `Option` itself
stays even though a cached id at a post-establishment handle is **always
`Some` in v1**: removing it is a breaking change to a ratified surface to
save a `match`, and re-adding it when a pre-establishment route lands
would be breaking again. §16.2's annotation is amended to name what the
`Option` is actually holding open.

**Ruling 117 — the shell's waker maps are keyed by `StreamRef`, not
`StreamId`.** §16.8 says *"parks its waker under its `StreamId`"*
(SPEC.md:4962). Ruling 95 already converted the four stream-naming
`ConnEvent`s to `StreamRef` on the grounds that *"the shell could not
match a wakeup to its waker before establishment"*. §16.8 was not swept,
and neither was the doc comment at `src/shell/shared.rs:48–51`, nor
ruling 107's own summary phrase.

Note what ruling 116 does **not** do to this. One might think that if
there is no pre-establishment handle, `StreamId` is available whenever
the shell parks, and §16.8 could stand. It cannot, for a reason that
survives 116: §6.7's tie-break and slice 7's replacement install a
session under a core holding live streams, and a `StreamId` is fixed by
an opener parity that a tie-break can **invert**. A waker map keyed by
`StreamId` would have to be rekeyed at every install, and a park that
straddled one would look up a key that no longer exists. `StreamRef` is
the only key that is stable for the life of the stream. **Three texts are
swept**, and this is working rule 4 again — grep for the rationale, not
only the token.

**Ruling 118 — `accept_bi`/`accept_uni` report `ConnectionLost`
immediately after death; they do not drain first.** §16.2:4225 puts
`accept_bi` in *"the same pull model"* as `notified()`, and §16.2:4230
gives `notified()` a drain-then-report rule. That rule does **not**
carry, and the reason is structural rather than a preference: **§15.2
lets `close()` drop stream, recovery and congestion state immediately**,
so after death there is nothing left to hand over. The pull model's
promise — *"a notification is never dropped on the floor between an
application's two visits"* — protects a payload that outlives the visit.
A stream's does not outlive the close.

The planner reached the same answer by a weaker route (a drained handle
would be inert because `core::Connection::read` refuses after death).
That is true and is the *consequence*; §15.2 is the *cause*, and stating
the cause is what keeps someone from later "fixing" `read` and reopening
the question. **§16.2's pull-model paragraph is amended** to say which
verbs it ranges over — defect class 1 once more.

**Ruling 119 — an empty `buf` short-circuits `read` to `Ok(Some(0))`
without touching the core**, mirroring ruling 110's rule for a
zero-length `write`. Ruling 110 exists because a shell that forwards an
empty write *"parks forever on an empty write of its own making"*. `read`
has the identical trap and had no ruling: the core's `Ok(Some(0))` means
"no data available", so a shell handed an empty `buf`, forwarding it, and
parking on the result waits for data it has nowhere to put. Short-
circuiting keeps `Pending` as the single meaning of "wait" at the handle,
and keeps the two conventions symmetrical. **It also protects the
distinction that a blind test author has already guessed wrong once**:
`Ok(Some(0))` = park, `Ok(None)` = end of stream (round 18).

**Ruling 120 — `BiStream::join(send, recv) -> Result<BiStream,
(SendStream, RecvStream)>`.** Ruling 96 named `join` and gave it no
signature; §16.2 never mentions it, so what bounds its arguments — same
stream? same connection? — was unstated. It checks that both halves name
the same `StreamRef` on the same `ConnectionId` and hands the pair back
unchanged on mismatch. No new error type, so §18.1 stays closed (ruling
61). The rejected alternative is a `debug_assert`, under which a release
build holds a `BiStream` whose halves are different streams, whose `id()`
is a lie, and whose `Drop` resets a stream the caller never named. Ruling
44's precedent governs: *rejection is a `Result`, never a panic*, for
anything reachable across an FFI boundary.

**Ruling 121 — `ReadError::Reset` is sticky at the handle; the core is
unchanged.** `Streams::read` retires the receive half **and then**
returns `Err(Reset(code))`, so the next `read` on that `StreamRef` finds
no half and returns `Ok(None)` — which §16.2:4163 documents as *"FIN
reached, all data delivered"*, false of a stream whose data §9.6
abandoned. An application that logs the reset and retries its loop reads
a clean end-of-stream, and **data loss is presented as success**.

The handle latches its terminal outcome and re-reports it. The core keeps
its per-call honesty ("this half is gone"), which is correct for its only
consumer. Recorded with its scope: if the core ever acquires a second
consumer, this wart bites there too, and the guard is in the shell.
**§16.2's `Ok(None)` annotation is amended** to state what it excludes.

**Ruling 122 — three low-cost questions, taken as recommended.**
(a) The `poll_*` forms are **`pub(crate)`**, matching `poll_close` and
`poll_closed`; slice 8's `compat/` is in-crate and reaches them, and
promotion later is additive while publishing a `key: u64` parameter now
is not. (b) **`SendStream::acked()` is absent from 4b, not stubbed** —
`src/shell/mod.rs:40–43` already rules that §16.2's verbs *"arrive with
the slices that define them and are absent rather than stubbed"*, and
`acked()` needs `ConnEvent::StreamFinished`, which slice 4 never fires.
Its module doc names slice 5. It is listed rather than assumed because
ruling 107's contents list omits it, and an omission is invisible until
someone builds against it (ruling 71). (c) The maps are
**`BTreeMap<StreamRef, Wakers>`**, not bare `Waker`s: `Wakers::take_all`
is `#[must_use]`, which makes finding F10's borrow-then-wake ordering the
only spelling the type permits rather than a rule a caller must remember.

**Ruling 123 — two assertions `PLAN.md` §11.2/§11.4 assign to 4b are
unreachable from `tests/` and belong to 4a's in-crate file.** Packet-level
interleaving and MAX_STREAM_DATA frame counts require reading *frames*;
`testutil::Tap` yields **sealed datagrams**. The 4b story tests use the
planner's behavioural substitutes, and I add the two frame-level tests to
`src/core/connection/tests_streams.rs` **myself, at integration** — that
file is on no 4b agent's path and a blind agent editing 3 489 lines of
someone else's tests is working rule 6's hazard wearing a different hat.
Recorded so the boundary moves once, deliberately, instead of being
discovered by a blind test author at compile time.

### What this round says about the process

Nine of the ten agents that have declined an instruction were right; the
4b planner makes ten of eleven. It refused to treat its own brief's
phrasing — "per-`StreamRef` waker maps" — as authority to edit §16.8,
ruling 107 or a doc comment, and put the three edits where a maintainer
would see them instead. **The brief was right and the planner still
should not have acted on it**, which is the distinction working rule 5
was written to protect.

Two of this round's nine rulings correct a rationale rather than a rule
(115's list, 118's cause), and **both were reached by checking a citation
rather than accepting it** — working rule 12's question, *what state did
the argument assume*, applied to an agent's argument instead of a
reviewer's. Ruling 115 is the more instructive: the planner's conclusion
was right, its evidence was a list, and the list has an exception sitting
inside it that would have been inherited as reasoning by everyone
downstream.

---

## Round 21 — slice 4b's implementer reports (2026/08/15)

Three items flagged, one of them a contradiction in the contract I wrote
the same day. All three are real.

**Ruling 124 — a handle's own terminal state outranks the connection's
death latch. The precedence is total and is stated here once, because two
agents reading two sections resolved it two ways.**

`CONTRACT-4b.md` §5 made `Ok(None)` and `Err(Reset)` **sticky**; §8 said
every verb answers from the close latch *"before anything else"* and
listed `read` → `ConnectionLost`. They disagree for one sequence: **read
to EOF, then the connection dies, then read again.** The implementer
followed §8, said so, and reported rather than silently picking — the
right call, and the contradiction is mine.

§5 is correct and §8's "before anything else" was written without this
case in mind. **The order, for every 4b verb:**

1. **This handle's own terminal state** — `Ok(None)` / `Err(Reset(code))`
   for a receive half; `Ok(())` for a repeated `finish()` and
   `Err(Finished)` for anything after `finish()` or `reset()` on a send
   half.
2. **The connection's death latch** — `Err(ConnectionLost(l))`.
3. **The empty-buffer short-circuit** (rulings 110 and 119).
4. **The core call**, which may park.

The reason for 1 over 2: **a stream that reached EOF completed, and the
connection dying afterwards does not un-complete it.** Reporting
`ConnectionLost` to a reader that already received every byte and the FIN
tells it a finished transfer failed — ruling 121's misreport with its
sign flipped, and ruling 121 is *in this same contract*. A handle reports
the fate of **its own stream**; the connection's fate is `closed()`'s to
report, and a half that has already reached a terminal state has no
further interaction with the connection left to fail. Slice 8 makes this
load-bearing rather than tidy: `AsyncRead` requires a sticky EOF, so
`read_to_end` over a connection that dies after the FIN would otherwise
surface a spurious `io::Error`.

The reason for 2 over 3: the empty-buffer rules exist **to avoid
parking**, and a dead connection does not park. Answering `Ok(0)` or
`Ok(Some(0))` there would report success on a corpse. The implementer
guessed this ordering and guessed right; it is ratified rather than left
as a guess.

**A consequence the contract got wrong in passing:** `closed_locally`
cannot be a `bool`. `finish()` is idempotent — a second one is `Ok(())` —
while `finish()` after `reset()` is `Err(Finished)`, so the two terminal
states have *different* answers and a `bool` conflates them, forcing
`poll_finish` to fall through to the core and revert to death-first the
moment a connection dies. It is now a three-state `LocalEnd`.

**Pinned by mutation, not by argument.** Ruling 124's reordering broke
none of the 642 tests standing at `c933e31` — every stream test passes
against *both* orders, because none reads again after the connection is
gone. Two tests were added and then verified by putting the old order
back: both red, and nothing else moves. A rule no test separates is not
ratified, it is merely written down.

**Ruling 125 — a stream handle's `Drop` performs §16.2's last-handle
`close(NO_ERROR, "")`. Ruling 115's blast radius included ruling 115.**

§16.2:4392 is unconditional — *"Dropping the last handle to a
`Connection` performs `close(NO_ERROR, "")`"* — and ruling 115, one day
old, made a stream handle a handle. So the last handle to a connection
can now be a `SendStream`, in **exactly the shape ruling 115's own
rationale names**: a task that owns a stream and has let the connection
handle go. Without the close, that connection emits nothing and the peer
pays `DEAD_TIMEOUT` — a behaviour regression introduced by ruling 115, in
the slice that ratified it.

The implementer acted, flagged it as the one place it moved beyond the
contract, and pinned it with a test that fails in **both** directions
(dropping the `Connection` while a stream lives must *not* close;
dropping that stream afterwards *must*). Ruling 88's exception is
untouched: the genuinely-last drop in the process transmits nothing.

**This is the ninth instance of "a ruling's blast radius includes the
ruling"**, and the pattern has never once been caught by reviewing the
ruling — only by an agent building against it. Ruling 115 was reviewed
carefully enough to *correct the planner's reasoning* about which list
§16.3:4409 is, and still shipped without anyone asking what else "handle"
meant in a document that uses the word forty times.

**Ruling 126 — the `Cargo.toml` test stanzas belong to the integrator,
and this becomes working rule 15.**

Cargo does not warn about a `[[test]]` whose file is missing; it
**refuses to parse the manifest**, which reds every gate at once. So an
implementer that adds live stanzas for its blind partner's files commits
a tree on which no gate can run — and working rule 7 forbids reporting a
gate green without running it. The alternative, creating placeholder test
files, is the exact act that destroyed 68 tests in slice 2a.

The implementer committed them **commented out** under a
`SLICE 4b INTEGRATION: UNCOMMENT WHEN THE TEST FILES LAND` header, having
checked that forgetting fails loudly rather than silently: with no stanza
at all, cargo auto-discovers `tests/*.rs` **without** `required-features`,
so a feature-less `cargo test` would try to compile them and fail. Right
answer, and it asked the right question — *which agent lands the manifest
change?*

**New working rule 15: a file whose contents are only valid once both
blind agents' work exists belongs to the integrator, and the briefs must
say so.** It is the mirror of working rule 6. Rule 6 partitions files so
two agents never write one path; rule 15 names the residue rule 6 leaves
behind — the file that *neither* can validly write alone.

**Ruling 127 — ruling 120's `join` check was insufficient as I wrote it,
and the implementer strengthened it correctly.** I specified *"the same
`StreamRef` on the same `ConnectionId`"*. **`ConnectionId` is not unique
across two endpoints in one process** — which is precisely the shape of
every test fixture in this crate, two endpoints over one `Network` — so
that pair can collide and `join` would fuse two halves of two different
connections into a `BiStream` whose `id()` lies and whose `Drop` resets a
stream the caller never named. The check is now `Rc::ptr_eq` on the cells
**and** the id **and** the ref; the extra conjunct cannot reject a
legitimate pair, because both halves of a real stream always share one
cell.

**This is the fourth ruling of mine justified by a mechanism that is not
what I said it was** (87, 89, 120, and 90's absent clause). Working rule
11 was written after the first two and did not prevent this one, for the
same reason it did not prevent 90: reading the code confirms that
`ConnectionId` exists and identifies a connection, and cannot surface
that it is scoped per-endpoint unless you ask what makes it unique. **The
question that would have caught it is working rule 8's** — *what bounds
this symbol, and does the text say?* — asked of a type rather than of a
list.

### What this round says about the process

**Eleven of twelve agents that declined an instruction or flagged rather
than acted have been right.** The implementer flagged three and acted on
one, and its judgement about which to act on was correct: D1's cost is a
25-second stall the gates cannot see, while C4's is a one-line ordering
it had no authority to choose.

Worth recording plainly: **none of the three findings was a defect in the
implementation.** As in slice 4a, the failures were in the contract and
in the rulings — 4a's five were three test defects and two spec
conflicts, 4b's three are two contract defects and one process gap. Two
consecutive slices have now produced zero implementation defects from the
blind split, and the defects the split *does* surface have moved
upstream, into the documents the agents build from.

---

## Round 22 — slice 4b integration (2026/08/15)

Twenty-five blind tests, **compiled on first contact** with no name, type
or arity mismatch anywhere. Three failed. One was a fixture defect, one
was the test asserting against ratified text — and one **overturned a
ruling I made four hours earlier**.

**Ruling 128 — ruling 118 was wrong at the receiving end. Received,
unclaimed stream state survives the connection's death, and `accept_*`
and `read` serve it.**

Ruling 118 said `accept_*` reports `ConnectionLost` immediately, and gave
a structural reason: *"§15.2 lets `close()` drop stream, recovery and
congestion state immediately, so after death there is nothing left to
hand over."* **That sentence is about the closing endpoint.** §15.2's
local-close bullet drops the closer's state; its *next* bullet, receiving
an authenticated CLOSE, says *"hold a brief drain for the same
`CLOSE_LINGER` … then drop all state"* — so the receiver still has
everything for the linger. I applied a true statement about one endpoint
to the other one. **This is working rule 12 in my own text**: the
argument was sound about the state it assumed, and I never asked which
state it assumed.

The consequence is not academic, and the blind test author found it by
writing the ordinary case down. A sender that writes, finishes and drops
its handles — the fire-and-forget pattern, which §16.2 makes reachable
**by accident**, since dropping the last handle performs
`close(NO_ERROR, "")` and ruling 125 has just made a stream handle able
to be that last handle — puts every byte and the FIN on the wire and then
closes. The peer's driver processes the data and the CLOSE in the same
pass, latches `closed`, and *then* wakes the application. Under ruling
118 the application's `accept_uni()` answers `Err(PeerClosed)` over a
stream that arrived in full. **It is not a race the receiver can win**:
even a receiver already parked in `accept_uni()` is woken after the latch
is set. The most natural sender pattern in the protocol delivered nothing
usable, in every case.

This is ruling 47's problem seen from the other end. Ruling 47 exists
because message-then-close *"loses its tail at the path's loss rate,
silently"*, and judged that unacceptable on the sender's side; the
receiver's side is the same loss with the same cause, and `acked()` does
not fix it — the peer's *transport* acknowledging is not the peer's
*application* claiming.

**The rule.** While the core still holds a stream's received state:
`read` serves the buffered bytes and then the FIN's `Ok(None)`;
`accept_*` hands over streams already opened before the death. When
nothing is left, both answer `Err(ConnectionLost)`. **Parking is never
permitted on a dead connection** — nothing further can arrive, so a
`read` with no data and no FIN is an error, not a `Pending`. `closed()`
is unaffected and still resolves at the death: the connection *is* dead;
what survives is data that already arrived. The analogy is TCP's, where a
peer's FIN does not stop you draining what is already in your receive
buffer.

**Where it is implemented: slice 5, and this is a scoping decision, not a
deferral of the finding.** Two guards make the data unreachable and both
were checked against the code rather than assumed. `Connection::drop_state`
drops the session and disarms the timers and **leaves `streams` and `flow`
untouched** — the bytes are genuinely still there. What hides them is
(a) `core::Connection::read`'s unconditional `self.lost` check, and
(b) the shell releasing the core at `ToEndpoint::Retired`
(`driver.rs:650`), which `drop_state` emits **at the instant of death**.
Moving (b) means touching ruling 81's definition of when `Retired` fires,
which is a statement about the endpoint's index table and the guard-entry
pin, not about streams. That is a coherent piece of work and it belongs
with ruling 47's `acked()`, which slice 5 already owns, rather than
bolted onto a slice that is otherwise complete. `tests/spec_streams.rs`
carries `a_receiver_can_drain_a_stream_the_sender_closed_behind`,
`#[ignore]`d and naming this ruling — ruling 114's precedent: deleting it
loses the obligation, running it fails a correct slice-4 build.

**§16.2 and ruling 118 are amended, not reversed wholesale.** 118's
answer stands for `notified()`-versus-the-rest as far as *ordering*
goes, and stands entirely for the **closing** endpoint, where §15.2 really
does drop the state. What it may no longer claim is that there is nothing
to hand over.

**Ruling 129 — §9.6's no-op is correct and the test was wrong; the
positive control it wanted is unreachable in slice 4.**

The author wrote `an_explicit_reset_after_finish_supersedes_the_fin` as
the positive control for the drop-after-finish test — a build whose
`reset()` early-returns on `fin` would pass the latter by never resetting
a finished stream at all. Sound instinct. But §9.6 says in terms: *"A
RESET_STREAM for an already-FIN-complete receive half is a valid no-op if
the final sizes agree, `FINAL_SIZE_ERROR` otherwise"*, and **in slice 4
the sizes always agree**. Ruling 111 pins `final_size` at the highest
byte *actually transmitted*; with no congestion control a `write()` never
leaves accepted-but-unsealed bytes behind, because a blocked write returns
`Pending` and the bytes stay with the caller (ruling 114). So the two
sizes can only diverge once something holds sealed-but-unsent data, which
is slice 5's congestion window — **exactly the case ruling 111's own
parenthetical describes as "ordinary operation", and which does not yet
exist.**

The test now pins §9.6's no-op, which is a real assertion: a receive half
that surfaced `Reset` there would turn a complete, correctly delivered
transfer into an error — ruling 121's misreport in the other direction.
The pairing the author wanted is preserved by a different test,
`dropping_an_unfinished_send_stream_resets_it_with_code_zero`, which no
no-op `Drop` can pass. Slice 5 owes the reachable control.

**The third failure was a fixture defect worth recording: seal is not
send.** S13's stall test blackholed a path, wrote the middle chunk, and
healed — with **no driver pass inside the window**. Ruling 114 says a
mutating call *seals* everything the ledger admits; the datagram then
sits in the core's output queue, and the I/O is the driver's, after
`mark_dirty` wakes it (§16.3). So nothing was dropped, all 10 530 bytes
arrived contiguously, and a test whose entire subject is *"is B reachable
across a permanent gap in A"* had no gap in it. Its own doc comment
flagged the coupling to ruling 114 and drew the wrong consequence from
it. One `settle()` fixes it. **Ruling 114 bounds where sealing happens,
not when transmission happens** — the third distinct thing that ruling
has now been misread as saying.

### What this round says about the process

**Two consecutive slices, and the blind split has still produced zero
implementation defects.** 4a's five failures were three test defects and
two spec conflicts; 4b's three are one test defect, one test asserting
against ratified text, and **one overturned ruling of mine**. The defects
have moved decisively upstream — they are in the contracts, the rulings
and the fixtures, not in the code — which is what a working process looks
like, and also a warning: the documents are now the least-reviewed
artefact in the project.

Ruling 128 is the sharpest case yet for the split's value. Nothing about
it was discoverable by reading code: the implementation matched the
contract, the contract matched ruling 118, and ruling 118 had a clean
argument attached. It took an author who had never seen any of them
writing down what an application would actually do.

**Addendum to ruling 123 — the debt was already paid, and better than I
specified.** Ruling 123 said I would add the two frame-level assertions
to `src/core/connection/tests_streams.rs` myself at integration. Checking
before writing them: **both already exist.**
`a_max_stream_data_arrives_at_exactly_half_a_window_read_and_not_before`
is §10.3's re-grant frame count, and
`the_stream_fill_serves_pending_streams_round_robin` is §8.5's fill.

The second is worth reading, because it **declines** the assertion
`PLAN.md` §11.2 asked for. §11.2 wanted "some packet carries frames for
both streams"; the test's doc says that is not asserted, because §8.5
makes the quantum implementation-defined and **a quantum of one packet is
legal and would fail it** — *"an assertion a conforming build can fail is
a flake, not a pin."* It measures interleaving as *each stream's frames
appearing before the other stream's last frame*, which a sequential fill
cannot have and any legal quantum does.

That is working rule 9 applied in the harder direction. The rule's usual
failure is an assertion too weak to separate the broken build; this is an
assertion too **strong** — one that separates the broken build *and* some
correct ones. Both are failures of the same question, *what exactly does
this separate*, and only the weak form had a rule written for it.

---

## Round 23 — slice 5 planning: reliability (2026/08/15)

Ten open questions, eight conflicts. Two of them correct ratified text of
mine, and one settles a slice boundary that six documents disagree about.

**Ruling 130 — §13 and §14 are slice 5's. Six texts say slice 7 and they
all descend from one.**

`PLAN.md`'s slice-5 row reads *"§12 ACK …, §13 RFC 9002 …, §14 NewReno,
ruling 47's `acked()`"*, and **§14 does not appear in its slice-7 row at
all**. Against it: ruling 105 (*"§13 is slice 7"*), ruling 114 twice
(*"§14's congestion window is slice 7"*), `send.rs:213`,
`tests_streams.rs:3234`'s `#[ignore]` reason, and the source they all
descend from — `.slices/04-streams/PLAN.md:1903`, a *slice-4 planning
document*, which is not authority over the slice plan. Ruling 129, made
yesterday, agrees with `PLAN.md`. **`PLAN.md` is the approved plan and it
wins**; the six others are corrected.

Two inherited debts move with the answer, and this is the part that costs
something. **Ruling 105's loss-driven tombstone variant** (*"free a
stream, drop the ACK, let the peer's PTO retransmission re-name it"*)
needs §12 **and** §13. **Ruling 98's STREAM-retransmission seal row** —
the row where one frame type takes two different seals — needs a
retransmission, which is §13's. The brief asked whether §12 forces ruling
98's row earlier; the planner's answer is exactly right and sharper than
the question: *§12 does not, but §13 does, and §13 is slice 5's.* Both
are slice-5 debts.

**Ruling 131 — §13.2's `Loss` timer arms from survivors below
`largest_acked`, not from every in-flight packet.** §13.2 says
*"Survivors **inside the threshold** arm the `Loss` timer at `time_sent +
loss_delay` (**minimum across in-flight packets**)"*. The subject is
packets below `largest_acked` that failed both loss tests; the
parenthetical ranges over the whole map, including packets *above*
`largest_acked` that the walk does not judge at all. They differ whenever
anything newer than `largest_acked` is outstanding — the ordinary case
during a transfer — and the wide reading arms a timer that fires and
declares nothing, or worse, declares recent packets lost. RFC 9002
§6.1.2 and v0.1 both take the narrow one.

**The prose is right and the parenthetical is the code-shaped rule.**
That is now the fourth time in this project, and working rule 3 exists
because of the first three. The parenthetical is corrected to *"minimum
across the survivors"*.

**Ruling 132 — ruling 128's guard (b) does not exist as described, and
the ruling is cheaper than I priced it.**

Ruling 128 said the second thing hiding drained data is *"the shell
releasing the core at `Retired`, which `drop_state` emits at the instant
of death"*, and that moving it *"means touching ruling 81"*. On the
**draining** path — the path ruling 128's own test builds —
`Frame::Close` sets `Lifecycle::Draining { until: now + CLOSE_LINGER }`
and arms the timer; **`drop_state` does not run there.** It runs at
linger expiry, five seconds later. I also cited `driver.rs:650`, which is
`release_dead`; `Retired` is handled at `driver.rs:471–482`.

The real blockers are three, all of them ordinary: ruling 118's accept
latch, ruling 124's read precedence, and `core::Connection::read`'s
`self.lost` guard. **Ruling 81 need not be touched.** The conclusion
stands unchanged; the scoping decision was priced against work that is
not required.

**This is the fifth ruling of mine justified by a mechanism that is not
what I said it was** (87, 89, 120, 90's absent clause, and now 128). It
is also the second in two days where the error is *precisely* working
rule 12's: I read `die()` → `drop_state()`, which is true, and never
asked which path the case in front of me actually takes. Ruling 128 was
itself the ruling that overturned 118 on this exact fault. **Checking the
mechanism is not enough if you check it on the wrong path** — and the
wrong path is easy to pick when a function named `die` exists.

**Ruling 133 — the closing endpoint frees stream state; the draining
endpoint keeps it for `CLOSE_LINGER`; the `die()` paths keep nothing.**

§15.2's local-close bullet says all stream, flow-control, recovery and
congestion state *may* drop immediately; the code today drops none of it,
because `drop_state` leaves `streams` and `flow` untouched on every path.
Ruling 128 requires the **receiver** to keep it. Both cannot be one rule,
and the asymmetry has a reason rather than a convenience: **at the closer
the application signalled that it is done, and at the receiver it did
not.** A `close()` while a receive half holds unread bytes is a decision
to discard them; a peer's CLOSE is not.

So: local close frees `streams`/`flow` at once (§15.2's letter, and the
memory ceiling §17.5 assumes). The draining path retains them for the
linger and frees at expiry. The no-linger deaths — liveness, nonce
exhaustion, `Replaced`, endpoint dropped — retain nothing, and **that
consequence is to be documented rather than discovered**: a receiver
killed by `DEAD_TIMEOUT` mid-stream cannot drain, which is honest,
because a path that produced no CLOSE produced no finished sender either.

**Ruling 134 — `write()` accepts bytes the congestion window cannot yet
send. The window defers the *seal*, never the *acceptance*.**

Nothing in §14, §16.2, §16.7 or §10.6 says what `write()` does when flow
control admits and the window does not, because until slice 5 there was
no window. It is the largest behavioural decision in the slice and it is
decided as (a): the bytes enter send state, `write()` returns `Ok(n)`,
and §14.5's gate holds them off the wire.

Four reasons. Ruling 111's own parenthetical describes *"a `reset()`
following a `write()` that congestion control has not yet released"* as
**ordinary operation** — that state must be reachable or the ruling
describes nothing. Ruling 129 asserts slice 5 makes its positive control
reachable, which requires exactly this. §10.6 already bounds the buffer
at the peer's advertised credit, so (a) introduces no new memory
obligation. And (b) would have the shell's writer park on *window room*,
a condition no `ConnEvent` announces — `StreamWritable` is credit-driven
— so slice 5 would owe a new event, a new waker map and a new wakeup
path, none of which the spec describes.

`write()` therefore remains a **flow-control** verb, and the congestion
window is invisible to it. Two `#[ignore]`d obligations go green on this:
ruling 129's control, and
`credit_frames_precede_the_stream_fill_in_a_packet`.

**Ruling 135 — both `acked()` verbs answer from their settled snapshot
before the death latch.** This is ruling 128's defect on the sender's
side, and it is reachable by the identical mechanism: the peer's ACK and
the peer's CLOSE arrive in one driver pass, the latch is set before the
application is woken, and `write; acked(); close()` — the sequence S28
exists for and §16.2 spells out — answers `Err(ConnectionLost)` over a
transfer that was fully delivered and fully acknowledged. Not a race the
sender can win. Ruling 124's sentence governs with one word changed: *a
stream whose bytes were acknowledged completed, and the connection dying
afterwards does not un-complete it.*

**Ruling 136 — a packet's `size` for §13.5 and §14.5 is the full
datagram** — `DATA_HEADER_LEN + ciphertext + AEAD_TAG_LEN`, i.e.
`Transmit::data.len()`. Nearly derivable already: §14.5 derives
`INITIAL_WINDOW` as `min(10 × 1200, max(2 × 1200, 14 720))` *"at
`MAX_DATAGRAM` = 1200"*, and `MAX_DATAGRAM` is the datagram. **A window
expressed in datagram units must be spent in datagram units.** Counting
plaintext instead under-counts by 30 B per packet — a standing ~2.5 %
overshoot at a 12 000 B window that grows with the window and that no
functional test can see.

**Ruling 137 — `SentPacket` carries a path generation from slice 5, and
§14.6's single marker is corrected.** §14.6 says the recovery-period
marker is set to the roam instant and not cleared, and §13.6 lists
**four** things pre-roam packets must not feed: a congestion event, the
persistent-congestion walk, an RTT sample, and `app_limited` growth. The
marker serves the first and fourth, because §14.3 already gates both on
it. **It cannot serve the RTT fence**: `recovery_start` is also set by
every ordinary congestion event, so an implementation reusing it would
suppress RTT sampling after every normal loss episode — silently, and
for ever on a lossy path. §14.6 names quinn's path-generation stamping in
the same breath, which is the mechanism that works.

So the stamp lands now: a `u32` on `SentPacket`, documented as slice 7's,
asserted 0 in slice 5. One dead field against a schema change in the
densest remaining slice, and it pre-empts a defect that would otherwise
be written into four fences.

**Ruling 138 — §13.1's second sample condition is vacuous and MUST NOT be
implemented.** A sample is taken when `largest` is newly acknowledged
*"and at least one newly acknowledged packet is ack-eliciting"*, while
§13.5 says *"Non-ack-eliciting packets are never inserted."* Every packet
in the map is ack-eliciting, so the clause can never be false. It is RFC
9002 §5.1's wording carried across from a design that tracks both kinds.
Left alone, an implementer reading §13.1 as exhaustive builds the
non-ack-eliciting tracking **in order to evaluate a condition that is
always true** — §8's defect class arriving as wasted machinery rather
than as a wrong answer. The contract states it is vacuous; §13.1 gains a
note.

**Ruling 139 — six low-cost questions, taken as recommended.**
(a) `pto_count` increments **at the `Pto` timer's firing**, before the
probe is built — RFC 9002's point, and the one that stays right in slice
7 where §7.3's budget can prevent a probe leaving.
(b) Persistent congestion **does not clear** `recovery_start`: §14.4 says
only that slow start effectively restarts, which `cwnd = MINIMUM_WINDOW <
ssthresh` already achieves, and §14.3's symmetric rule is stated in
slither without RFC 9002's carve-out.
(c) `app_limited` is recorded on **the packet that emptied the queue
while headroom remained** — §14.5's "records it onto each sent packet"
reads as a property of the send, and the alternative lets a bulk sender
holding the queue one packet ahead grow the window while effectively
idle, which is the case §14.5 reasons about.
(d) `Controller` is **`pub(crate)`**, with a rustdoc line saying so
deliberately, so slice 9's API review does not read it as an oversight;
§14.1 defers pluggability to "later" and §19 intends to revisit the shape
for CUBIC/BBR.
(e) `SendStream::acked()` **before `finish()` parks and does not
resolve** — §16.2 says "every byte written to that stream **and its
FIN**", and there is no FIN yet. The hazard (`write().await;
acked().await;` hangs) goes in the rustdoc at the call site, on the same
terms as slice 4's "`open_bi` parks for ever on an exhausted bidi space".
(f) §13.2's boundary is **`>`**, not v0.1's `>=` — §13 governs. The
difference is one `K_GRANULARITY` tick and is invisible except to a test
that lands on it, so slice 5's test lands on it, **one-sidedly, on both
sides** (slice 1's `LEN`/`LEN-1`/`LEN+1` lesson).

**Ruling 140 — seven doc comments in slice-4 core files cite section
numbers that do not exist in `SPEC.md`.** `send.rs` and `streams.rs`
carry "§12.5's seam", "§12.1's seam", "**§12.6's seam**", "§12.7: credit
frames apply as O(1) monotone-max" and three more. `SPEC.md`'s §12 has
exactly five subsections and no §12.6 or §12.7; the numbers are
`.slices/04-streams/PLAN.md`'s. In a project whose first hard rule is
that `SPEC.md` is the authority, "§N" in a doc comment reads as a spec
citation. They are dangling **in slice 5's own files, pointing at the
spec section slice 5 implements** — the maximum-confusion position. The
integrator fixes them.

### The cut, ratified

**5a (core: §12 + §13 + §14) then 5b (shell: `acked()`, ruling 128's
drain), sequential, with three concurrent agents inside 5a** — one
implementer and **two** blind test authors, one for §12 and one for
§13/§14, on disjoint paths.

The planner's argument for refusing to split §12 from §13/§14 is the one
that decides it: they are a single feedback loop through a single call
site, and splitting them would ship a sent-packet map whose only
consumers live in the other half — **which is ruling 113's exact defect**,
one of the five debts this slice exists to pay. Repeating a defect while
paying it off would be a poor use of a slice.

Two test authors rather than one because the acceptance evidence differs
in kind: §12's is structural (ranges, fusion to the replay window), and
§13/§14's is numeric and derivable from the spec text alone — the
strongest case for a blind author this project has had.

### Working rule 5, twelfth of thirteen

The planner **declined** a brief instruction: I told it
`credit_frames_precede_the_stream_fill_in_a_packet` was slice 7's and
asked it to confirm; it reported that it could not, because §14 is absent
from `PLAN.md`'s slice-7 row and the test goes green exactly when slice
5's admission gate can leave stream data pending. That refusal is what
produced ruling 130, and ruling 130 is what moves two further debts into
this slice. **The instruction was wrong, and confirming it would have
carried three errors forward silently.**

---

## Round 24 — slice 5a integration (2026/08/16)

Three agents, 109 blind tests, **both test files compiled on first
contact** with no name, type or arity mismatch. Ten reds, none of them a
defect in the implementation.

**Ruling 141 — §13.2's time threshold is `>=`, reversing ruling 139(f).**

I ruled six hours earlier that §13.2's *"sent **more than** `loss_delay`"*
governs over v0.1's `>=`. That was wrong against **§13.2's own next
sentence**: the `Loss` timer arms at `time_sent + loss_delay`, so at the
firing instant the packet's age *equals* `loss_delay` and a strict `>` is
false. The walk the firing triggers declares nothing and re-arms at the
same instant.

The implementer showed it is worse than a wasted timer: the re-arm goes
through `sync_recovery_timers`, so `poll_output` announces a deadline at
or before `now`, the shell schedules an immediate wake, and **the pair
spins**. Under `>` that is *every* firing, so a connection on a lossy
path **livelocks its driver** — a failure no functional test sees,
because every byte still arrives.

Two things about how this was found. It is **ruling 131's own defect
arriving by a second route**, three paragraphs from where ruling 131 had
just corrected the first, in the same round — I fixed one instance of
"arms a timer that fires and declares nothing" and created another the
same morning. And **v0.1 was right**: ruling 139(f) overruled it on the
one point where its `>=` was correct, and correct *because* of a §13.2
sentence I did not read alongside the one I was ruling on.

The blind test author found it, wrote its tests to the ruling anyway, and
flagged the pair as *"the first to revisit if the ruling moves"*. It
moved. Those two tests are now `a_packet_exactly_loss_delay_old_is_lost`
and **`the_loss_timer_firing_at_its_own_deadline_declares_the_packet`** —
the livelock guard, and the strongest form of the finding.

**Ruling 142 — the blind §12 author's `counters()` helper contradicted
its own expectations, and the expectations were right.** Four tests
failed on ordering. `counters()` flattens each ACK block with
`out.extend(b)` over a `RangeInclusive`, which walks **ascending**, while
every expected literal in the file is written in the ACK's **descending**
order. One `.rev()` fixed all four. Worth recording because it is the
inverse of the usual test defect: the *assertions* were verified against
§12.1 and correct; the *instrument* that read them was not. A blind
author's expectations are the valuable artefact — its helpers are just
code, and get no more trust than any other code.

**Ruling 143 — ruling 116's id cache fills eagerly, not lazily.** Ruling
116 said the handle caches "the first time the core answers `Some`". §12
made that too late: an ACK can fully close a locally-opened stream,
`Streams::after_half_freed` removes the entry, and a handle whose *first*
`id()` call happens after that answers `None` for ever. **Ruling 116's own
doc comment predicts this non-monotonicity** — it simply gave the cache a
scope one event too short. Every handle is constructed while its stream
exists, so there is exactly one instant at which the answer is guaranteed
available. It is now filled there.

Found by the implementer, in a file it was not allowed to touch
(`src/shell/stream.rs` is 5b's), reported with the one-line fix and the
decisive experiment — *reading `id()` once before the stream is freed
makes the test pass* — rather than reaching across the boundary.

**Ruling 144 — four tests from earlier slices had their premises expire,
and expiry is not deletion.**

- `a_ping_is_accepted_and_answered_with_nothing_in_slice_3a` and
  `padding_may_appear_anywhere_around_other_frames` asserted
  `transmits().is_empty()`. §12 makes an ack-eliciting arrival owe an ACK.
  The assertion is **narrowed, not dropped**: every transmit is now opened
  and required to decode to ACK or PADDING only. A bare
  `transmits().len() <= 1` would have passed a build that answered a PING
  with a CLOSE.
- `a_max_streams_only_packet_does_not_defer_the_keepalive` demanded a
  *credit-only* packet. ACK now rides along, and ACK is `seal_quiet`
  (§7.4), so the property is untouched; the precondition filters ACK
  beside PADDING and still fails if the credit frame is absent.
- `a_send_half_never_reports_finished_because_slice_four_has_no_acks` was
  named for a boundary that has moved. **Inverted rather than deleted**,
  to `a_send_half_reports_finished_once_the_peer_acknowledges` — and it
  keeps the same mutation in its sights, "freeing the send half on send
  instead of on acknowledgement", separated now by side B seeing no such
  event. It is the foundation ruling 47's `acked()` stands on in 5b.

**Ruling 145 — S13's stalled-stream test needed a new mechanism, and the
new one is better.** Its premise was *"slice 4 has no loss recovery, so
the gap is permanent"*. §13 repairs the gap — **and repairs it via the
sibling stream**, because loss detection is *connection*-level: B's own
traffic pushes A's blackholed packets three counters below `largest_acked`
and they are retransmitted. So the test read a contiguous 10 530 bytes
while claiming a hole.

Blocking the **ACK path** (`b → a`) instead keeps the gap genuinely open
under slice 5: with no ACK returning, A never learns anything was lost,
and `settle()` only yields — it does not advance the paused clock — so
neither the `Loss` timer nor the PTO fires. The hole persists for exactly
the window under test, deterministically, while A→B data keeps flowing.
The test now proves head-of-line independence **under an active
loss-recovery regime**, which is strictly more than it proved before, and
then heals the path and asserts the retransmission arrives.

*One correction inside that work, mine:* I first wrote the recovery half
as `read_to_end`, which hung — stream A is never `finish()`ed in that
test, so there is no FIN to reach. The diagnostic said `7020 bytes` —
exactly chunk 2 plus chunk 3 — which is the recovery working perfectly
and the assertion being wrong. **Every byte arrived; only my expectation
of an EOF did not.**

### What this round says about the process

**Three consecutive slices with zero implementation defects from the
blind split.** 4a: three test defects, two spec conflicts. 4b: one test
defect, one test against ratified text, one overturned ruling. 5a: one
ruling of mine reversed, one blind helper wrong against its own correct
expectations, one cache scope too short, four expired premises, one
fixture aged out.

The defects are now **entirely** upstream of the code — in rulings,
contracts, helpers and fixtures. Two consequences worth stating. The
documents are the least-reviewed artefact in the project and have been
for three slices. And **"expired premise" is now a recognisable category
of its own**: seven tests across two slices have had a boundary move
under them, and in every case the right action was to narrow or invert
the assertion rather than delete the test — because the mutation each was
built to catch is usually still live, just newly reachable by a different
route.

---

## Round 25 — slice 5b integration, and slice 5 closed (2026/08/16)

**All fifteen blind story tests passed on first contact.** The suite is
**801 passing, 0 failing, 0 ignored** — the first time since slice 4 that
it carries no deferred obligation at all.

**Ruling 146 — ruling 133's local-close half is reversed. Both death paths
retain stream state until the linger expires.**

Ruling 133 said a *closing* endpoint frees `streams`/`flow` at once
(§15.2's letter) while a *draining* one keeps them for `CLOSE_LINGER`
(ruling 128), and justified the asymmetry by arguing that **at the closer
the application signalled that it is done**. The argument is sound and the
rule it produced is not, for a reason neither I nor the 5a planner saw:

`Streams::send_settled` returns **`true` for an absent half**, because an
absent half is normally one that completed and was collected. Freeing the
closer's state therefore makes `Connection::acked()` answer **`Ok(())`
over bytes that were never acknowledged** — the exact misreport ruling 47
exists to prevent, arriving through the door ruling 133 opened. The 5b
implementer found it by refusing to implement 133 blind: it reported that
the work *"is implemented nowhere and assigned to nobody"* and that doing
it would break `acked()`, rather than patching around either.

Reversing costs **nothing**, because 133's local-close half was never
built: 5a declined it, and 5b's contract did not assign it. What it buys
is uniformity — `read` and `accept_*` now behave identically whoever
closed — and an `acked()` that is honest by construction rather than by a
guard. §15.2's *"may drop immediately"* is permissive, so retaining is
conformant; §10.6 already bounds the buffers, and the draining path
already spends exactly this memory under ruling 128.

**The general lesson is about the shape of 133, not its content.** It
invented an asymmetry to satisfy a rationale, and the asymmetry created a
correctness hazard in a *different* subsystem two rounds later. A rule
whose only justification is that it feels right about intent should be
suspected of exactly this.

**Ruling 147 — `CONTRACT-5b` §2.5's waker list for `SendStream::acked()`
omits the death latch, and the omission is a permanent hang.** The
contract parks the verb in `blocked_ackers`, *"woken by `StreamFinished`
and by `StreamReset`"* — while the sibling paragraph for
`Connection::acked()` ends *"and on the latch"*. Read exhaustively, a
`SendStream::acked()` already parked when the connection dies is woken by
**nothing**, violating ruling 128's *"parking is never permitted on a dead
connection"* in the one direction an application cannot poll its way out
of. Found by the blind test author reading the list as exhaustive —
**working rule 8 applied to a list in the binding contract**, which is
where that defect class has now appeared twice in three slices. The
implementer had already swept the latch, so the test passed; the contract
is corrected so the next reader does not build what it says.

**Ruling 148 — `FlakyPolicy::lossy(rate)` is invisible to every counter
the public API has, so a test that injects a loss rate cannot prove a
datagram died.** The tap is written **above** the loss draw, so
`sends() − tap.len()` sees blackholes and injected send failures only. A
story test that sets a loss rate and asserts "the bytes arrived" therefore
passes a build with **no loss at all** — working rule 9's exact failure,
sitting in the fixture rather than in any test. The blind author found it,
and every lossy test it wrote uses `block_path` (counter-proved) or
`drop_at` (index-based, no RNG) **plus an assertion that the drop window
was actually reached**. §16.10 makes the three fixture names contract;
this is a gap in what they can attest, and slice 9's documentation
obligations should record it rather than leave it for the next author to
rediscover.

**Ruling 149 — `credit_frames_precede_the_stream_fill_in_a_packet` is
discharged, after being deferred twice.** Ruling 114 sent it to slice 7;
ruling 130 moved it to slice 5; slice 5b reported it still unreachable.
The mechanism was in the implementer's own report: *"stream data pending"
implies "window full", and credit frames are ack-eliciting, so the
coincidence needs an ACK to re-open the window.* That is a recipe, and it
works. The fixture now: fills §14.5's window (ruling 134 — `write()`
accepts what the window cannot send, so the remainder is **pending in the
core**, the state slice 4 could not produce); then owes a MAX_STREAM_DATA
*while the window is full*, so §14.5 refuses the credit frame too and it
stays owed instead of going out alone; then delivers an ACK, whose pump
owes credit **and** has stream data pending.

**Verified by mutation twice, because the first attempt did not prove what
it looked like.** Swapping `Stage::Control` and `Stage::Fill` reds the
test — but through `frame.rs`'s own forward-only assertion, not through
the test's. Only mutating at the **emission site** (`pack_control` after
`fill`, both pushed as `Fill`) makes the test's own *"§8.5: control
frames, then the fill"* fire. A mutation caught by the implementation's
internal guard proves the guard, not the test.

### Slice 5 closed

**801 tests, nine gates, no ignored obligations.** Twenty rulings across
Rounds 23–25 (130–149), of which **six corrected ratified text of mine**:
131 (§13.2's parenthetical), 132 (128's mechanism), 137 (§14.6's single
marker), 141 (139(f)'s comparison), 146 (133's asymmetry), 147 (the
contract's waker list).

**Four consecutive slices with zero implementation defects from the blind
split.** The 5b implementer's own summary is the pattern in one line: it
shipped a red on purpose, because the red was a finding about a ruling
rather than a bug in its code, and it declined to patch around it.

---

## Round 26 — slice 6 planning: messages and datagrams (2026/08/16)

Eight open questions, six conflicts, nine unstated-scope findings. Two of
the conflicts are in ratified text of mine, and one of those is in **ruling
128**, which was itself written to fix a list that had been read as
exhaustive.

**Ruling 150 — `send_message` admits the whole payload or nothing, and
§10.6 is the reason.**

§16.4 gives `core::send_message` a `Result<(), MessageError>`, and
`MessageError` is **exhaustive and already shipped** — `TooLarge` and
`ConnectionLost`, nothing else, pinned by `spec_errors`. So the type
cannot express "not now", while §16.2's own prose says the verb *"waits
for stream allowance"* and `core::write` is credit-gated at admission.

The planner's three candidates are decided by **§10.6, not by
ergonomics**: *credit is the buffer commitment*. Accepting a payload
beyond the peer's credit — the "core-side tail", quinn's shape and the
most literal reading of §9.8's *"writes the whole payload"* — makes
sender-side buffering `(uncredited message streams) × MESSAGE_RECV_MAX`,
which at `INITIAL_MAX_STREAMS_UNI` = 128 is **32 MiB, a term §17.5's
ceiling table does not contain**. That is a memory-bound change requiring
its own ratification, not an implementation choice. Note this is *not* in
tension with ruling 134: 134 lets `write()` accept what the **congestion
window** cannot send, and congestion is not a buffer bound; flow control
is, and it still gates admission.

The shell-decomposes option is excluded outright: a dropped
`send_message` future would leave a FIN-less, half-written uni stream,
which against a message-mode receiver is §9.8's overflow case — so an
application's own `timeout(d, send_message(..))` would **manufacture the
failure S30 exists to diagnose**.

**The cost is one wake and it is payable.** A `send_message` refused for
allowance already has `ConnEvent::StreamsAvailable { dir: Uni }`; refused
for **connection credit** it has nothing, because `StreamWritable { r }`
fires only for a half with a blocked writer and a pending message has no
stream yet. Slice 6 mints one `pub(crate)` `ConnEvent` variant for it.
That enum is internal, the addition is additive, and slice 4 already
added six — nothing on the wire moves.

**Ruling 151 — `core::recv_message` takes `now: Instant`;
`recv_datagram` does not, and the asymmetry is stated so nobody "fixes"
it.** §16.4 gives `recv_message` no instant, but it **retires a receive
half** (owing MAX_DATA under §10.3) and can **emit RESET_STREAM** (§9.8's
overflow check runs *"at the instant such a claim is made"*), while §16.7
puts sealing inside the mutating call and this core has no `Instant`
field at all. Without `now`, S30's reset waits for the next `now`-bearing
call — on an idle connection, a timer — **delaying by seconds the one
path whose entire purpose is to fail promptly instead of stalling.**
`abandon_recv` already takes `now` for the *weaker* of those two reasons.
This is ruling 71's shape and ruling 108's remedy. `recv_datagram` stays
`now`-free because datagrams are flow-control **exempt** (§10.7): claiming
one advances no credit and emits nothing.

**Ruling 152 — ruling 128's post-death drain covers `recv_message` and
`recv_datagram` too. My list was short, for the third time.**

Ruling 128 names *"`read` … and `accept_bi`/`accept_uni`"*. Appendix B
ratifies an obligation the enumeration cannot satisfy (SPEC.md:6238-6241):
*"`send_message(m)`, then `acked()`, then `close()`, then drop every
handle. Assert endpoint B receives `m` in full from `recv_message()`."*
B's driver processes the data and the CLOSE in one pass — **ruling 128's
own worked example** — so `recv_message()` runs after the latch and, under
the two-verb reading, answers `Err(PeerClosed)`.

The rule's *rationale* already described messages: §16.2 names
`send_message(msg); acked(); close()` as **the** idiom `acked()` exists
for. Its *enumeration* omitted them. `recv_datagram` joins by symmetry —
nothing is promised for a datagram, but the asymmetry would be a trap for
precisely ruling 128's reason, that a receiver woken after the latch
cannot win the race by being prompt.

**Three of my lists have now been read as exhaustive and found short**
(§16.4's stage-0 accessors, ruling 128's verbs, `CONTRACT-5b`'s waker
list), and every one was found by an agent *building against it* rather
than by review. Working rule 8 is about the spec; it applies to rulings
with no discount.

**Ruling 153 — the overflow predicate is the highest received offset, and
`send_message` MUST carry the FIN on its last data frame.**

§9.8 never says which quantity "reaches `MESSAGE_RECV_MAX`". The
contiguous-prefix reading never fires when a middle byte was lost —
exactly the case where the sender is stalled at the window and needs
rescuing, so it reintroduces ruling 51's permanent stall. The
highest-offset reading fires correctly under loss **but appears to reset a
legitimate 262 144-byte message whose FIN is still in flight**, with
`MESSAGE_OVERFLOW` — producing precisely the *"transfers die at 256 KiB"*
post-mortem ruling 59 describes, pointing at the wrong cause.

The race closes on the **sender's** side, not the predicate's: if
`send_message` attaches the FIN to its final data frame rather than
emitting a separate empty FIN frame, `high_water == MESSAGE_RECV_MAX`
implies that frame arrived, which implies the final size is pinned, and
the predicate cannot fire on a well-formed maximum-size message. The
residual case — `open_uni()` + `write(262 144)` + a separate `finish()`
aimed at a message-mode receiver — **is** ruling 51's mixing error, whose
defined loud failure is this exact reset.

This is the finding most likely to have shipped silently: a false positive
on a *conforming* application, and a requirement nobody infers from
"writes the whole payload, sets FIN".

**Ruling 154 — §10.3's fifth retirement trigger is struck.** §10.3 lists
five triggers *"— read to its final size, reset observed, handle
abandoned, surfaced as a message, or **final size reached with no reader
(§9.7)**"* — and **§9.7, the section it cites, lists three and does not
contain the fifth.** If it were real, a complete but unclaimed message
stream would retire and true up connection credit at the FIN, *before*
`recv_message()` claims it — contradicting §10.6 (*"message and datagram
payloads stay accounted inside the core … until the handle takes them"*)
and §16.4's backpressure-by-retention. Three statements, at most two of
which can hold; the two that agree with each other and with the design win.
Retention until claimed is the rule.

**Ruling 155 — one datagram per packet, packed before the stream fill,
and the `0x30` form is mandatory rather than an optimisation.**

Of the three packing orders, datagrams-after-the-fill is the one an
implementer writes by accident, because appending after the existing
`streams.fill(...)` is the smallest diff — and it is the worst: a
saturated stream fills all 1170 bytes of every packet, datagrams **never**
go out, and the bounded queue evicts continuously. **Silent data loss,
with no counter that distinguishes it from ordinary pressure.**
Drain-the-whole-queue starves a bulk stream for up to 64 packets. One per
packet, before the fill, is the only order whose starvation is bounded in
both directions and statable: a stream waits at most one packet per queued
datagram, a datagram at most one packet per predecessor.

Separately and not optionally: `MAX_DATAGRAM_PAYLOAD` is 1169 and
`MAX_PLAINTEXT` is 1170, so a maximum-size datagram in the `0x31` form
needs 1 + 2 + 1169 = 1172 bytes and **cannot be sent at all**. The `0x30`
extends-to-end form is what makes the ratified maximum reachable. slither
has never emitted an extends-to-end frame before this slice.

**Ruling 156 — four smaller answers.** (a) The pending-claim flag is
cleared by a `recv_message()` that returns `Some` — the literal reading of
*"while a claim is pending, and at the instant such a claim is made"*, and
its failure mode is bounded delay rather than a reset the application did
not earn. (b) **Two** drop counters, send and recv: §11.5's singular sits
four words from its own plural, and one counter cannot answer the operator
question the counters exist for — *is my application over-producing, or is
my peer over-sending?* (c) `earns_stream_credit` stays `true` everywhere;
§10.3's *"consumption, not arrival, drives credit"* and the fact that
`take_grant()` is reachable only from `Streams::read` already give §9.8's
bound, and the planner answered this from the spec rather than raising it.
(d) `recv_datagram` drains after death (ruling 152).

**Ruling 157 — slice 6 is not cut.** One implementer across core *and*
shell, plus **two blind test authors split by behaviour rather than
layer** — datagrams (§11, S15) and messages-plus-overflow (§9.8, S16,
S30). A 6a/6b cut would put the review boundary on the half with no design
questions, while every open question above is a core question decided on
both sides of the seam by one agent; and S30's ratified acceptance shape
is an integration test through shell handles, so a 6a would close **no
story**. At ~1.5k lines — half of slice 4 or 5 — that is a poor trade.

**Ruling 158 — working rule 3's own count was stale, and that is the rule
failing on itself.** `CLAUDE.md` still read *"Three times … most recently
ruling 69"* while the count is five and the most recent is ruling 131. I
have been citing the current figure in briefs and never swept the rule
that carries it. **Working rule 4 — grep for the rationale, not only the
token — applied to the rules file itself**, which is the one document in
this project that no slice ever puts on an agent's path. Swept, and the
count is now maintained with the rulings that move it.

### Two corrections to my brief, both the planner's

The `SentFrame`/DATAGRAM paragraph is `CONTRACT-5b.md` §2.7, not 5a's —
5a has no §2.7. The claim itself verifies true. And the brief quoted
*"801 tests"*, which is the `--all-features` figure; bare `cargo test` is
726, because six targets sit behind `required-features = ["test-util"]`.
Neither changes a decision, and both are the kind of thing an agent is
right to correct rather than absorb. **Fifteen of sixteen agents that
declined or corrected an instruction here have been right.**

---

## Round 27 — slice 6 mid-flight findings (2026/08/16)

The blind datagram author returned four items before the other two agents
finished. Ruled now, because two of them change what is being built.

**Ruling 159 — the `0x30`-not-final check already exists, and slice 6 must
not build a second one.** `CONTRACT-6.md` §2.2 says the check *"has nowhere
to live today"*, that it *"belongs in `Frame::parse`'s loop"*, and that the
analogous ¬LEN STREAM case is accepted **silently** at `frame.rs:298-306`.
**All three claims are wrong**, and I checked rather than took them:
`frame.rs:705` holds exactly that check, in exactly that loop, returning
`Structural::TrailingFrame` — and its own comment explains that it is *"dead
by construction and deliberately written"*, because an extends-to-end body
consumes `cursor.rest()` and the loop condition is already false.

So the DATAGRAM case is covered the moment `Frame::Datagram` answers
`extends_to_end()` truthfully, which `CONTRACT-6.md` §2.2 already lists as
one of its five dispatch points. **Implement that and add nothing else.** A
second check inside `parse_body` would be unreachable at best and wrong at
worst, and it is what a careful implementer builds when a binding contract
tells it the guard is missing.

Working rule 11 is usually aimed at *my* rationales; this is the same rule
applied to a planner's survey. A claim that the code does not do something
is exactly as checkable as a claim that it does, and rather less often
checked.

**Ruling 160 — `finished_senders` must not grow for handle-less message
streams.** It is bounded today by the rule *"written only while
`blocked_ackers` holds that stream's slot"*, and a §9.8 message stream has
**no `SendStream` handle**, therefore no `blocked_ackers` entry and no
`Drop` to remove one. Left alone, every message ever sent leaves a
permanent entry — an unbounded map keyed by a value the *peer's* traffic
rate controls. §17.5's ceiling table has no term for it. Slice 6 either
excludes handle-less streams from the map or removes the entry when the
message's send half retires; the implementer picks, and says which in its
report.

**Ruling 161 — the datagram packing decision never evicts; §11.3's queue
pressure is the only eviction trigger.** The author found that a
maximum-size (1169-byte) datagram at the head of the queue cannot fit a
packet that already carries an ACK, since §8.5 packs the ACK first — so it
waits. That much is correct and acceptable. What must **not** happen is
that waiting causing a drop: eviction is `send_datagram`'s, on enqueue,
under §11.3's drop-oldest, and a packing pass that cannot fit the head
**leaves it queued**.

The residual property is real and is to be documented rather than
engineered away: **a maximum-size datagram is preferentially delayed** by
any packet that carries control frames. It is bounded — §12.4 does not put
an ACK on every packet — and the alternative, reordering the queue to fit
a smaller datagram first, trades a bounded delay for a silent reordering
that §11.1 permits but nobody has ruled on. Not this slice.

**Ruling 162 — §11.5's trace obligation is pinned by nothing, and that is
recorded rather than papered over.** The drop counters are `#[cfg(test)]`,
§2.7 forbids a public accessor, and no planned test asserts a trace line —
so **a build with both counters correct and no trace at all passes every
test in this slice**, while §11.5 exists precisely because *"a silent drop
is a known operability weakness"*. The same gap covers ruling 59's tracing
MUST. `testutil` has no tracing capture and building one is not slice 6's
job. **It becomes a named slice-9 obligation** — slice 9 owns the five
documentation obligations and Appendix B's completion, and a tracing
fixture belongs with them. Recorded here so it is inherited rather than
rediscovered.

**Also swept:** three stale ⚠ markers in `CONTRACT-6.md` and one in
`PLAN-6.md` still described Q1/Q2/Q5 as open after Round 26 ruled them,
including one asserting *"no ruling says so"* about a rule that now
exists. Working rule 4 again, and the second time in two days that the
sweep had to be done by hand after the ruling landed. The still-running
message author was messaged directly, because a stale marker told it to
ship a ratified obligation `#[ignore]`d.

---

## Round 28 — slice 6's blind message author (2026/08/16)

Fifteen tests, and an **unsatisfiable triple in ratified text** that no
amount of careful implementation could have resolved.

**Ruling 163 — `core::send_message` returns `Result<SendMessage,
MessageError>`, where `enum SendMessage { Sent, Blocked }`.**

Three ratified statements could not all hold: §16.4 gives the core verb
`Result<(), MessageError>`; ruling 150 requires it to say *"not now"*; and
§18.1's `MessageError` is **closed** — `CONTRACT-6.md` §2.6 says so itself
and `spec_errors` pins it. The contract papered over the gap by writing
`Result<SendMessage, MessageError>` with **`SendMessage` defined nowhere**,
which is the shape of a problem deferred rather than solved.

The resolution is that *"not now"* was never an error. It is an ordinary
outcome of a verb that admits atomically, in the same family as
`write`'s `Ok(0)` and `accept`'s `None` — both of which this core already
uses to mean *the state is not ready*, neither of which is an error. So the
success type carries it and §18.1 stays closed, untouched: no variant is
added, no public error surface moves, and `spec_errors`' pins hold.

`Blocked` carries **no reason**, deliberately. The shell parks in one
`message_senders` map fed by three wake sources — `StreamsAvailable
{ dir: Uni }` for allowance, ruling 150's new event for connection credit,
and the death latch — so distinguishing the cause would buy the caller
nothing and cost a second slot to release. §16.4 is amended; it is the
**core** API, `pub(crate)`, and this is an internal change of exactly the
kind ruling 150 already made when it minted a `ConnEvent` variant.

**Ruling 164 — ruling 153's second clause is swept into the two places
that still stated the predicate without it.**

§15.3's code registry describes `MESSAGE_OVERFLOW` as *"an unclaimed uni
stream reached `MESSAGE_RECV_MAX` while a claim was pending"* — the
version **without** the final-size clause. An implementer looking up what
`0x06` means gets the rule that resets a conforming maximum-size message.
Appendix B states it correctly; the registry did not.

Worse, §9.8's own prose contained a parenthetical — *"nor surfaceable by
`recv_message()`, **which it cannot be, having no FIN**"* — that argues as
an established fact the very thing ruling 153 had to add as an independent
clause. It is true only of a sender that has not yet sent its FIN, which
is the case the rule is *for*; it does not establish that the predicate is
safe, because a conforming `send_message` of exactly `MESSAGE_RECV_MAX`
bytes reaches the bound at the same instant. **A reader who takes the
parenthetical as the argument builds the version that resets its own
protocol's largest legal message.**

Both swept. This is working rule 4 for the third time in two days —
grep for the rationale, not only the token — and the first time the
un-swept text was not merely stale but *actively persuasive in the wrong
direction*.

**Recorded, not ruled: ruling 59's receiver-side tracing MUST is
uncoverable, and the asymmetry is the finding.** The author reports that
the **sender** half of §9.8's diagnosis is pinned five times over in its
file, and the **receiver** half — the MUST that exists because *"the end
that actually chose the conflicting mode learns nothing at all"* — is
pinned nowhere in any slice, because `testutil` has no tracing capture and
`Tap` yields sealed datagrams. Ruling 162 already sent this to slice 9;
what Round 28 adds is that the gap is **one-sided**, and that the
unpinned side is the one the obligation exists to protect. Slice 9 gets a
tracing fixture, not a note.

### The pattern worth naming

Both blind authors, working on disjoint halves and blind to each other,
independently reported the same stale `RULING REQUIRED` markers. Neither
could see the other's report; both read the contract exhaustively enough
to notice that §0's table answered questions §2 still called open. **A
document defect that two independent readers both trip over is not a
readability problem, it is a correctness problem** — and the process
surfaced it twice in one slice without either author knowing the other
existed.

---

## Round 29 — slice 6 integration, and slice 6 closed (2026/08/16)

Both blind test files compiled on first contact. **832 passing, 0 failing,
0 ignored**, all nine gates green.

**Ruling 165 — a peer's §9.8 reset is latched at the handle, because the
core frees the half and calls it `Finished`.**

The blind author's `s30_the_overflow_reset_trues_up_connection_credit`
failed on one stream of four: three answered `Reset(MESSAGE_OVERFLOW)` and
**stream 1 answered `Finished`**. The cause is not the overflow scan —
that drains its whole candidate set correctly. It is that
`SendHalf::is_terminal()` is **true for a peer-reset half**, so the next
ACK frees it through `on_ack_range` and pushes
**`ConnEvent::StreamFinished`**.

That is much worse than the failing assertion shows. `StreamFinished` is
what `SendStream::acked()` resolves on, so an application awaiting
delivery confirmation for a stream **the peer destroyed** would be told
its data arrived. §16.2 states the opposite in terms: `acked()` *"returns
`Reset(code)` if the stream was reset before its data was acknowledged (a
local reset, **or the peer's §9.8 overflow reset**)"*. This is ruling
121's misreport — data loss presented as success — reached on the send
half, and it is the fourth time this project has found that shape.

The fix is ruling 121's, mirrored: the **shell latches** the code from
`ConnEvent::StreamReset`, which carries it and fires *before* any ACK can
free the half, and `write`, `finish` and `acked` all answer from the latch
ahead of everything else — ahead even of ruling 124's own terminal state,
because "the peer discarded your bytes" outranks "you closed this". The
core keeps its per-call honesty; the map is bounded by live handles, freed
in `Drop`.

The core's own comment at `is_terminal` says a peer-reset half is *"not
freed here, because only an observation can"* — and then `on_ack_range`
frees it anyway, three hundred lines away. **A comment that states an
invariant is not an invariant.**

**Ruling 166 — ruling 159 is REVERSED, and the way it went wrong is worse
than the ruling.**

Ruling 159 said the `0x30`-not-final check *"already exists"* at
`frame.rs:705`, that `CONTRACT-6.md` §2.2's three claims about it were
*"all wrong"*, and — in as many words — that the planner had been
careless. **All of that is false.** `git show f19cb99:…/frame.rs | grep -c
TrailingFrame` is **0**. The check did not exist at the base commit; the
contract was **correct**; the guard is the slice-6 implementer's own work.

How I got there is the part worth recording. I read `frame.rs` **out of
the shared main working tree while the implementer was mid-task**, saw its
uncommitted guard, and attributed it to the base commit. Then my Round 27
commit ran `git add -A` and **swept that uncommitted work into a commit of
mine** — which is why `git log -S TrailingFrame` now names `bc4e895`,
Round 27, as the origin of code the implementer wrote.

Two rules I have been enforcing on agents all week, failing on me:
**working rule 12** — a true lemma about the wrong state proves nothing; I
verified a real fact about a tree that was not the one under discussion —
and **working rule 10**, *commit before mutating*, whose hazard I named
aloud earlier in this same session and then walked into from the other
side. **Ruling 160 fell the same way**: it required the implementer to
bound `finished_senders`, which `note_send_finished` already bounds by
returning above its insert. Two rulings, one contaminated read.

**New working rule 16: when the integrator inspects the tree while an
implementer holds it, read from the commit — `git show <base>:<path>` —
never from the working copy; and never `git add -A` while another agent is
writing.** This is the first defect in this project caused by the
*integrator's* own tooling rather than by a document, and it produced a
ruling that libelled a planner who was right.

**Ruling 167 — the blind author's first-flight bound was arithmetic about
the wrong layer, and the fix is ruling 134's consequence.** Two S16 tests
blackholed a path and asserted that at least `min_packets(16 KiB)` ≈ 14
datagrams died. They did not: ruling 150 admits the whole payload into
send state, but §14.5's gate only lets `INITIAL_WINDOW` (12 000 B ≈ 10
packets) leave before an ACK returns, and on a blackholed path none does.
The bound is now `INITIAL_WINDOW / MAX_DATAGRAM`, still a **strict lower**
bound — which is what working rule 9 needs here, since without it the test
would pass on a wire that lost nothing. Both tests' own "otherwise this
proves nothing" guards are what caught it, which is the second time this
slice that an author's self-check earned its keep.

### The finding that mattered most was the implementer's

§9.8's receiver-emitted RESET_STREAM was being **rejected** by
`check_peer_may_send` and killing the connection with
`STREAM_STATE_ERROR`, so the mixing sender got `ProtocolViolation` instead
of `WriteError::Reset(MESSAGE_OVERFLOW)` — **story S30 could not have
passed.** §8.4 states the rule *with* its exception (*"with exactly one
exception, the message-mode overflow reset of §9.8, in which the receiver
of a uni stream emits RESET_STREAM"*); slice 4's generalisation dropped
the exception, and nothing could see it until slice 6 made the case
constructible. Working rule 8's shape, latent for two slices. The
implementer's own note: *"I found this by running it, not by reading."*

### Slice 6 closed

**832 tests, nine gates, no ignored obligations.** S15, S16 and S30 close.
Twenty rulings across Rounds 26–29 (150–167), of which **five corrected
ratified text of mine** and **two reversed rulings I had made hours
earlier**. Five consecutive slices with zero implementation defects from
the blind split — and this slice, for the first time, a defect from the
integrator.

---

## Round 30 — slice 7 planning: the mobility audit (2026/08/16)

An adversarial spec auditor was run against slice 7's surface (§5.4, §5.7,
§6.3–6.4, §6.7–6.9, §7.2–7.3, §7.5, §13.6, §14.6, §17.4) **blind to the
planner**, hunting working rule 8's defect class. It returned **twelve
findings**, and its "checked and clean" section is as valuable as its
findings: S11's three collapse claims agree across three sections, the
notification-at-transmission rule agrees across four sites, the probe
floor is safe across ratchet/roam/nonce-exhaustion, every slice-7 death
path already exists in `error.rs`, and the tie-break basis is consistent
across §6.7, §17.4 and slice 4's code.

**Eleven of the twelve stand. One is corrected — and the correction is
working rule 12 again**, on an agent that was otherwise the best this
project has run.

### 168 (F-C, the round's most consequential) — the amplification budget disarms on a return-routability proof. **This reverses a recorded declination.**

§7.3 states the anti-amplification budget as a standing inequality and
closes the door on ending it: *"The 3× ratio is **never lifted**"*. Four
other sections describe validation as an event that **occurs** — §5.6
(923) *"until the address validates by traffic"*, §6.9 twice (1816, 1821),
and §7.4 (2004) *"and then goes quiet **until the address validates**"*.
§7.3 defines no predicate that ends the unvalidated state.

**The literal reading is untenable, and not marginally.** The budget arms
*"whenever a session's endpoint address changes (a roam) **or is first
anchored from a msg1 source**"* — so **every responder-side connection
begins unvalidated**. Under a permanent cap, an accepting endpoint may
never send more than 3× what it receives, for the connection's entire
life. A peer downloading a file replies with ACKs only: ~40 bytes per
~2400 sent (§12.4's delayed ACK), funding ~120 bytes of budget against
2400 bytes of demand. **An endpoint that accepts connections could never
serve one.** That is not a protocol anyone ratified; it is a throughput
cliff invisible today only because nothing anchors a budget yet.

**Two independent tells that the literal reading is a defect, not a
decision.**

1. §7.3 grounds "never lifted" in *"the amplification factor QUIC accepts
   (RFC 9000 §8.2/§9.3)"* — and in QUIC that limit binds **only until the
   address is validated**. The sentence cites as its authority a
   specification in which the thing *is* lifted.
2. The clause that makes the permanence sound harmless — *"a genuine peer
   clears it within about one round trip, because its own authenticated
   traffic funds the budget continuously"* — is true for a symmetric
   request/response exchange and **false for every asymmetric one**. It is
   working rule 11's shape inside the spec: a rationale naming a mechanism
   that does not do what it is claimed to do. "Clears it" is transition
   language; under the literal rule nothing is ever cleared.

So §7.3's declination of an unlock was taken **on the premise that no
unlock was needed**, and that premise does not hold. Working rule 3
applies in its usual direction: the prose in §5.6/§6.9/§7.4 carries the
intent; the formal rule in §7.3 carries the bug. **Seventh time.**

**Ruling.** The budget **disarms on a return-routability proof**, and the
proof is a primitive slice 7 is already building:

> At each address change (roam, or first anchor from a msg1 source),
> record `validation_floor` = **the counter the next seal will use** —
> hiss's `next_counter()`, the same construction ruling 41 records as
> `probe_floor`. The address becomes **validated**, and the budget
> disarms, when an authenticated, window-fresh packet **from that
> address** carries an ACK covering **any counter ≥ `validation_floor`**.
> Until then the 3× cap binds all output, exactly as §7.3 states today.
> The counters are freed at validation and re-armed at the next address
> change.

An ACK at or above the floor can only have been produced by a peer that
**received a packet we sent to that address after the change** — which is
return-routability, proven, with no new frame and no new state beyond one
`u64` and one `bool`.

**Why this is not the declined alternative.** §7.3 declined *"an
N-authenticated-packets-over-1-RTT validation unlock (more state, the same
reflection property)"* and *"explicit PATH_CHALLENGE/PATH_RESPONSE"* (two
new frame types, wire-affecting, golden-wire pin red). This is neither: it
is a **single** round-trip proof carrying **less** state than the
N-packet scheme, it is wire-free, and unlike the N-packet unlock it does
**not** have "the same reflection property" — N authenticated packets can
be replayed by an off-path attacker, whereas an ACK covering a counter we
chose *after* the address change cannot be manufactured without the key.
The declined option was weaker than this one, which is likely why it was
declined.

**The 3× ratio itself is still never lifted** — it is never raised, never
configurable, and binds every §14.5/§13.4 cwnd exemption for as long as
the address is unvalidated. What ends is the *unvalidated state*, which
§7.3 always intended to be temporary and never said how to leave.

**Flagged for the maintainer.** This is the one ruling this round that
**reverses a decision the spec records as taken**. It is wire-free and
turns no golden vector, and slice 7 cannot be built without answering the
question one way or the other — but it belongs at the top of the
**adversarial protocol review already scheduled after this slice**, and it
is the ruling I most want attacked. Recorded in full so that reversing it
is a ruling and not an excavation.

### 169 (F-F) — the budget is funded by authenticated **and window-fresh** bytes

§7.2 states the invariant: *"Liveness and roaming are driven only by
packets that are both authenticated and window-marked (fresh). No replayed
packet ever moves the endpoint or refreshes liveness."* §7.3 funds the
budget on bytes *"received from it and **authenticated**"*, citing *"§7.2's
authenticated class"* — the **broader** class — and excludes only
*"unauthenticated or undecryptable"* bytes. Rule 8 reads that exclusion
list as exhaustive, so **on the literal text a replayed packet replenishes
a security counter**, letting a keyless on-path attacker inflate our send
budget toward an address.

**Ruling:** fund the budget only on **authenticated and window-fresh**
bytes, matching §7.2's invariant and §7.3's own summary line 1897
(*"Nothing unauthenticated, and no replayed packet, ever moves it"*).
The literal text is a scope slip: the sentence set out to exclude
*unauthenticated* bytes and did not notice that citing the broader class
also admitted *replayed* ones. No argument exists anywhere for letting
duplicates fund a security budget. Working rule 3, prose over formal rule
— **eighth time**.

### 170 (F-D) — the budget is **per session**, and the constant table is corrected

The rule scopes the counter *"on this session"*; the constant table (1945)
and the appendix (6375) both say *"per unvalidated address"*. They differ
whenever two connections share a peer address — routine under NAT, and
routine in §6.9's own threat model.

**Ruling: per session.** It is the normative sentence, it is the only form
implementable inside `core::Connection` (where §13.6/§14.6 put the roam
seam), and §17.5's per-connection state census budgets no endpoint-side
per-address table. Both table sites are corrected to *"per unvalidated
address, per session"*. The residual — N sessions to one address multiply
the reflector by N — is stated in §7.3 rather than left to inference.

Exactly the §2.3 `TAG`-beside-`PK` shape: **a parenthetical in a constant
table asserting a scope the normative text does not deliver.**

### 171 (F-E) — priority within a scarce budget, and the starvation path

§7.3 binds *all* output to the budget; §7.5 lets a probe the budget will
not admit leave the mark **pending**. Nothing states which output wins
when the budget admits less than is owed. The auditor's attack: an
adversary with harvested peer→us Data injects one small packet just under
`DEAD_TIMEOUT` **from a fresh source each time**, which (a) refreshes
liveness, (b) roams the session — resetting the budget counters to that
one packet's bytes — and (c) leaves too little budget for the probe to win
against the ACK also owed. The zombie the probe exists to reap becomes
immortal and `Contested` never fires. §6.8 (1694–1711) asserts *"the
zombie dies within `KEEPALIVE_TIMEOUT` of the probe"* **in the same
paragraph that establishes the attacker roams the session**, and never
joins the two — in a section whose own method is *"that bound rests on a
premise, and the premise must be named"*.

**Ruling, two parts.**
(a) **The pending contested probe takes priority over all other output to
an unvalidated address**, ahead of ACKs, keepalives, PTO probes,
retransmissions and new Data. §7.5 already argues this exactly once, for
the congestion gate: *"a probe the gate could delay past its own deadline
would silently convert congestion into a liveness verdict."* The argument
transfers verbatim to the budget, and the budget cannot be waived, so
priority is the only lever left.
(b) §7.3 states the full priority order, not just the probe's place, so
the remaining classes are not left to queue order.

Note that **ruling 168 independently defuses the attack's engine**: the
budget now disarms on a return-routability proof, and an off-path injector
cannot produce one. (a) and (b) still stand — an attacker who *can* keep
the address unvalidated must not be able to starve the verdict.

### 172 (F-B) — §14.6's fence assignment: the code is right, the texts disagree three ways, and the auditor's consequence is corrected

§13.6 and §14.6 name **four** fences for pre-roam packets: no congestion
event, no persistent-congestion walk, no RTT sample, no `app_limited`
growth. Three texts give three answers:

- **Ruling 137** assigns the congestion event and `app_limited` to
  `recovery_start`, the RTT sample to `path_gen`, and **never assigns the
  persistent-congestion walk** — first, fourth, third, and the second
  named in the enumeration and dropped. It then closes *"a defect that
  would otherwise be written into **four** fences"*, reading as all four.
- **§14.6's closing clause** says *"§13.6's fences read that stamp rather
  than the recovery marker"* — plural, unqualified, **0/4 on the marker** —
  reversing its own opening clause two sentences earlier.
- **The code** (`congestion.rs:70–80`) shipped a **2/2 split** that
  neither text states, written in a doc comment on an uncalled function.

The auditor ranked this **high**, arguing the readings "differ in
observable behaviour" and that the 2/2 hybrid reintroduces the silent
failure ruling 137 warns about. **That half is wrong, and the reason is
worth recording.** The argument requires `recovery_start` to be
*clearable* — so that a late-resolving pre-roam packet could escape the
fence once the roam's episode closed. It is not clearable:
`congestion.rs:181`, **ruling 139(b)**, *"`recovery_start` is **not**
cleared. RFC 9002 §7.6.2 clears it; §14.4 asks only that slow start
restart"* — and it is only ever assigned `Some(now)`, so it moves
**monotonically forward**. Every pre-roam packet has
`sent_time ≤ roam_instant ≤ every later recovery_start`, so `in_recovery`
stays true for it **permanently**. The two readings are behaviourally
identical.

**Working rule 12, fourth confirmed instance: a true lemma about the wrong
state.** The auditor verified something true — three texts, three answers —
and attached a consequence that assumed a state ruling 139(b) forbids. It
read ruling 139 and cited 139(a) as clean; it read `congestion.rs` lines
70–100 and the never-cleared note is at 181. **The fixture bounds the
coverage applies to reading, too: a range chosen for one purpose bounded
what could be found.**

**Ruling:** the code's **2/2 split is ratified** — `recovery_start` fences
the congestion event and `app_limited` growth; `path_gen` fences the RTT
sample and the persistent-congestion walk. §14.6's closing clause is
corrected to state the split explicitly and stop claiming all four read the
stamp; ruling 137's enumeration gains its missing fourth assignment. The
defect is **documentation, not behaviour**, and it is downgraded from
high to medium accordingly — but all three texts must be made to say the
one thing the code does, because slice 7 is where a blind test author reads
§14.6 and asserts 0/4.

**And a correction of my own, twice made this session.** I described
ruling 137's fences as open carried debt for slice 7. They are not: ruling
137 identified the gap and slice 5 closed it in the same stroke.
`Congestion::reset()` is written and merely uncalled; `path_gen` is pinned
at 0 by a live `debug_assert` at `recovery.rs:198` that fires the moment
roaming works. What slice 7 owes here is **wiring, not design**.

### 173 (F-A) — §13.6's title claims a scope its body does not cover

§13.6 is titled **"What resets when — the roam seam"** and lists only
recovery and congestion state. §7.3 mandates a different reset on the
*same* seam — *"both counters resetting at each such address change"* —
and §13.6 does not mention it. Rule 8 reads a list as exhaustive whether
or not it says so; **this one carries a title that says so.** An
implementer building `on_roam()` from §13.6 touches the controller and the
sent map, and the amplification budget silently carries the old address's
credit to the new one — funding sends to a fresh attacker-supplied address
with credit earned from the genuine peer. That is the reflector §7.3
exists to prevent, reconstructed out of a missing line.

**Ruling:** §13.6 grows an explicit cross-referenced list of **every**
per-connection reset on the roam seam — the amplification counters and
`validation_floor` (§7.3, ruling 168), the congestion controller (§14.6),
the path generation, the pending contested mark (ruling 176), and the
items it already resolves (the sent map kept, PTO undisturbed, RTT
suspect-but-kept). Narrowing the title instead was considered and
declined: rule 8's whole point is that the omission is invisible until
someone builds against it, and the list is the thing that has to exist.
**§7.2's anti-replay window is explicitly named as "not reset"** — the
auditor checked this and found it clean by construction (one session, one
never-reset counter space, §7.7:2476), but "clean by construction" is
exactly what a list like this must say out loud.

### 174 (F-G) — §16.5's equal-deadline list calls itself exhaustive and is not a total order

§16.5: *"This list is **exhaustive**: every pair of deadlines that can fall
on one instant is ordered here."* The stated relations leave two
pair-groups unordered — `{Liveness, CloseLinger, Contested}` against
`{Loss, Pto, AckDelay}`, and `{Loss, Pto, AckDelay}` against `Keepalive`.
`timers.rs:33–65` already froze an answer via a derived `Ord` over the
declaration order, and its module doc says it exists to stop slices 5 and
7 "re-deriving it from prose".

**This is slice 7's problem specifically:** only five of the eight timers
are ever armed before it. Slice 7 arms `Contested`, `Keepalive` and
`PersistentKeepalive` for the first time, so **every collision in the two
unordered groups becomes reachable in this slice**, and a blind test author
working from §16.5 (as working rule 6 requires) can derive `Loss` before
`Contested` and write a confident assertion against the frozen enum.

**Ruling:** `timers.rs`'s order is authoritative; §16.5 gains the two
missing relations — *teardown collection precedes loss/PTO evaluation*,
and *loss/PTO/`AckDelay` precede keepalive evaluation*. The second does
**not** follow from §16.5's stated governing principle ("a terminal outcome
precedes a routine one, and state removal precedes emission"), because
`AckDelay` before `Keepalive` is emission-before-emission, which the
principle does not reach — so it must be stated, not derived. The word
"exhaustive" is the same self-certifying scope claim as §13.6's title, one
section apart.

### 175 (F-I) — the probe-rate bound is restated honestly; **no cooldown**

§7.5 and §6.9 both state *"at most one packet per `KEEPALIVE_TIMEOUT` per
connection … a bound the attacker cannot move"*. The collapse rule only
suppresses refusals landing **while the mark is outstanding**, and on a
**live** connection the peer ACKs in ~1 RTT, clearing the mark; the next
refusal lands uncontested and is a full second mark. The true rate is
`min(refusal rate, 1/RTT)` — on a 10 ms LAN path up to ~100 probes/s
rather than 0.1/s, a factor of ~1000.

Ruling 43 replaced one dishonest bound with another. Both texts say the
same thing and both are wrong the same way, which is why review passed it —
**not a working-rule-3 conflict; a working-rule-8 unstated scope, agreed
upon.**

**Ruling:** state it honestly — *"at most one probe per mark, at most one
mark per uncontested refusal, and marks cannot overlap; the refusal rate
is the application's own `accept()` rate."* **No cooldown is added.** A
cooldown would leave a genuine second doubt unprobed for its duration,
which trades a cost bound for a security hole. The security half is
untouched either way: **every re-mark records a fresh floor**, so each
probe still demands acknowledged progress *after* the doubt that raised it.
This is a **cost** defect, and it is the exact sentence ruling 43 exists to
make true.

### 176 (F-J) — the pending mark's two unstated exits

The mark-pending state (probe marked, budget not yet admitting) has one
stated entry and one stated exit. Two more are reachable and unstated:

- **An ACK covering the floor arrives while still pending.** Ordinary, not
  exotic: the floor is *"the counter the next seal will use"*, so any
  post-mark seal — keepalive, retransmission, pure ACK, application Data —
  lands at or above it. On the literal text §16.4 emits `ContestCleared`
  *"when the mark clears"*, unconditionally, so **`ContestCleared` fires
  with no preceding `Contested`** — an unmatched notification, which is
  precisely the mis-read ruling 46 deleted `under_probe: bool` to prevent.
  And §7.5's *"the endpoint sends it, and arms, at the first instant the
  budget allows"* carries no condition, so a **stray probe** goes out and
  arms a `KEEPALIVE_TIMEOUT` verdict deadline for a mark that no longer
  exists — which §16.5's disarm rule cannot cancel, because it disarms on
  an ACK covering a floor that has already been satisfied.
- **The connection roams again while still pending**, discontinuously
  changing the pending probe's budget prospects.

**Ruling:** clearing a pending mark **cancels the pending probe and emits
nothing** — the *"the mark-pending gap emits nothing"* principle §16.4
already states for the gap governs its exit too. §7.5's send rule gains
*"unless the mark has already cleared"*; §16.4 gains *"`ContestCleared` is
emitted only where `Contested` was"*. A roam leaves the pending mark
intact with its floor unchanged (the counter space is never reset, §7.7)
and is listed as such in §13.6 per ruling 173.

### 177 (F-H) — a contested mark requires **admission**; walk exhaustion joins the `Stale` list

Two defects, one bullet. §6.4's `Stale` enumeration covers "no initiation
parked", "admitted candidate fails the basis rule", and "PENDING and we are
the tie-break winner" — and **omits re-home-walk exhaustion**, a distinct
case reachable precisely when initiations *are* parked and all fail. §6.9
supplies the answer in passing (*"before it returns `Stale`"*); §6.4's list,
read as exhaustive, leaves the path with no return value.

The consequential half: §6.4 scopes the contested mark to **admission**;
§6.9 scopes it to **any refusal**. An exhausted walk is a refusal that
reached no admission — so the two texts disagree on whether **an attacker
who can only park mac1-valid rubbish can provoke a contested mark**, with
no key material at all.

**Ruling:** §6.4's narrower rule governs — **the mark requires an admitted
candidate** proving the same static with a verifying tail tag. §7.5's own
security argument prices the primitive in captured genuine initiations
(*"an attacker's replay supply buys refusals"*), and §6.9's DoS table
prices the rubbish rows at *"0 DH … one bounded queue slot"* and never at a
probe. §6.9's sentence is a cost summary written loosely and is corrected;
§6.4's `Stale` list gains exhaustion explicitly.

### 178 (F-K) — ruling 91's open predicate, closed: **PENDING means a pending exists, not a datagram in flight**

Ruling 91's amendment recorded, explicitly unresolved, that ruling 90's
`mint_pending`/`start_attempt` split makes *"an in-flight outbound
initiation exists"* ambiguous — a minted pending has **no initiation in
flight** — and that the ambiguity reaches §6.4's PENDING branch, §6.5's
hint set and §17.4. The auditor confirmed by grep that nothing since closes
it, and it sits directly in slice 7's path.

**Ruling:** the predicate is **membership in the pending tables**, not
whether a datagram has left. §6.4, §6.5 and §17.4 are reworded from *"an
in-flight outbound initiation exists"* to *"a pending exists for the proven
static"*.

**Why this and not the other reading:** a minted pending is a declared
intent to dial. If a peer's initiation arriving in that window took §5.4's
NONE row, we would install as responder and *then* `start_attempt` would
fire — two key sets, both msg2s dropped, mutually dark for `DEAD_TIMEOUT`,
which is **exactly the divergence ruling 35 was made to prevent**. Round 8
already verified the mechanism: all three readers of "is this static
PENDING?" read the same pending tables that ruling 50's cancellation
empties, and there is no separate per-static flag. This ruling states what
that verification implies.

### 179 (F-L) — the closing/draining carve-out is stated where the mark is taken

§7.5 makes a contested mark on an already-closing or draining connection a
**no-op**; §6.4, which is where the mark is taken, never mentions it. A
closing connection remains in §17.4's map for its linger (that is what
makes the `Retired` event and the guard pin necessary), so §6.4's rule as
written takes the mark and §7.5 must undo it.

**Ruling:** §6.4's admission rule carries the carve-out explicitly.
Behaviourally minor; stated because §6.4 is where an implementer writes
the code, and a rule enforced only in the section that *describes* the
state rather than the section that *enters* it is a rule that gets missed.

### 180 (fixture) — `FlakyWire::rebind`, and what a rebind does to in-flight datagrams

Verified at `b649575`, not inferred: `FlakyWire.addr` is a plain immutable
field and `Network.endpoints` is keyed by it, so **no wire can change its
address**. Stories **S18 and S19 are unreachable from the fixture by
construction** — working rule 13's exact shape. `Network::inject` delivers
with a spoofed source and no policy; it can simulate a datagram *arriving*
from a new address but cannot make a real endpoint *originate* from one,
so it cannot produce the authenticated, window-fresh packet a roam
requires except by replaying bytes — which §7.2's window rejects.

**The gap is one-sided, in the now-familiar way:** the *receiving* half of
a roam is testable today; the *originating* half is not testable at all,
and the originating half is what S18 and S19 are about.

**Ruling:** add `FlakyWire::rebind(new_addr)` — `addr` becomes a `Cell`
(the wire is `!Send` and every other mutable field is already a
`Cell`/`RefCell`), `Network` moves the `EndpointState` between keys
**carrying the existing `Rc<Notify>`** (the driver's recv loop is parked on
that exact `Rc`; a fresh one hangs it), and rebinding onto a registered
address panics as `Network::endpoint` already does.

**In-flight datagrams are abandoned**, not carried: the new address gets a
fresh empty inbox. This models what an interface change and a NAT rebind
actually do, and it is what makes S18's "positive obligation on the mover"
bite — under the carry-across alternative a peer could move, stay silent
and still receive, so the story's central claim would pass **for the wrong
reason**, a bound the degenerate implementation satisfies for free
(working rule 9). No change to `deliver` is needed: an unregistered
destination already drops, so vacating the old key handles everything sent
*after* the rebind for free.

**`src/testutil/mod.rs` lands committed before dispatch**, as a contract
input. It is a single path that both the implementer and the test authors
need before either can start, so rule 6 forbids giving it to either of
them mid-slice, and working rule 14 forbids letting it arrive after the
worktrees are cut.

### What this round says about the process

**Twelve findings, eleven upheld, from an agent that read no more than
~1 200 lines of a 6 300-line spec.** The one correction is working rule
12's fourth confirmed instance, and it has a new twist: the auditor's
false consequence came from a **read range** chosen for a different
purpose — it read `congestion.rs:70–100` and ruling 139(b)'s note is at
line 181. Working rule 13 said *the fixture bounds the coverage*; this
says **the excerpt bounds the audit**, and an agent under working rule 1's
context discipline is structurally exposed to it. That is a cost of the
discipline, not an argument against it — the alternative killed an agent —
but it means a finding whose consequence turns on code the auditor
excerpted deserves the integrator opening the *whole* function.

**Six of the twelve are the same defect**: a construction whose scope is
stated in one place and contradicted or omitted in another (F-A's title,
F-B's enumeration, F-C's missing predicate, F-D's parenthetical, F-G's
"exhaustive", F-H's list). That is now ~25 instances, and **still not one
has been a wrong value.**

**F-C is the largest single finding of the project so far.** Not because
it is subtle — a permanent 3× cap on every accepting endpoint is not
subtle — but because it survived the full spec walkthrough, four
adversarial review rounds, and eighty rulings, protected by a sentence
that *sounded* like a disarm condition (*"a genuine peer clears it within
about one round trip"*) without being one. Nobody re-read it because
everybody had already read it.

### 181 (planner Q5) — two frames claim "final position"; one claim is structural and wins

§8.5 (2829–2832) packs *"the ACK first (if owed), then control frames …,
then STREAM and DATAGRAM fill, **then PING last** if a probe still owes
ack-eliciting content. At most one extends-to-end frame (¬LEN STREAM, or
`0x30` DATAGRAM) per packet, **in final position**."* Two frames are told
to be last. Slice 7 makes the collision routine: the contested probe and
the PTO probe both emit PING into packets that may already carry an
extends-to-end frame.

**Ruling:** the two rules are not in conflict once the senses are
separated, and the separation is forced by the parser.

- An extends-to-end frame carries **no length prefix** — it is defined as
  running to the end of the packet. Its final position is **structural**:
  nothing *can* follow it, because anything that did would be parsed as
  part of it. This claim is not negotiable.
- PING's "last" is **ordinal** — a placement preference among
  length-prefixed frames. A 1-byte frame's position carries no semantics.

So: **PING is packed immediately before the extends-to-end frame**, and
"PING last" reads as *last among length-prefixed frames*. §8.5 is
corrected to say so. The sender may equally emit the datagram in its
`0x31` LEN form and keep PING physically last; both parse identically and
the choice is the sender's, but the ¬LEN form must never be followed by
anything.

**Ruling 155's bias does not measurably worsen.** The debt asked whether
slice 7's new control frames deepen the preference for delaying a
maximum-size datagram. They do not: **the keepalive is the empty
plaintext (§7.5:2053), which bypasses the frame layer entirely** and so
never competes for packet space, and the contested PING is one byte
emitted at most once per mark (ruling 175). The bias is unchanged in kind
and negligibly changed in degree. Debt discharged.

### 182 (planner Q7) — the passive keepalive reads **marking sends only**. **The first time the formal rule held the intent.**

§7.5:2056 states the passive rule in prose over any send — *"a side that
has received since it last sent, and **has not sent** for
`KEEPALIVE_TIMEOUT`, sends a keepalive"* — while §7.5:2117 defines the
variable it reads as *"`S` = `last_send` (**marking sends only**)"*. Wire
traces diverge from the first non-marking send onward.

**Ruling:** the **formal definition governs**. The passive rule reads
`S` = last *marking* send; §7.5:2056 is corrected from "has not sent" to
"has not made a marking send".

**Why, and why this is not a reflex.** Working rule 3 exists because six
times the prose held the intent and the formal rule held the bug — but the
rule says *do not **default** to the code-like rule*, not *always take the
prose*. Here the reasoning runs the other way, and it is decisive: the
beacon's soundness proof at 2123–2126 rests on *"every send that can
establish `S > R` is a marking send, so the death clock is armed there
(§7.4)"*. Under the prose reading a **non-marking** send blocks the dance
— "has not sent" becomes false — **without arming the death clock**, and
the proof collapses into precisely the immortal half-open session that
SECV5-2 was applied to prevent. The formal definition is load-bearing for
a security property two subsections later; the prose is shorthand that
predates it.

**This is the first time in this project that the formal rule carried the
intent, and it is worth naming.** Working rule 3's tally has been
one-directional for eleven rounds, and a one-directional tally decays into
a reflex — which is itself the defect it warns about. Ruling 158 already
caught that tally going stale in `CLAUDE.md`. The rule is *report the
conflict and reason*, and the reasoning here is: **follow the statement
some other proof depends on.** Working rule 3 is amended to say so.

**Not settled here:** which individual sends are marking. §7.4 already
carries that classification and the contract must **quote it, not
re-derive it** — a PTO probe's class in particular is §7.4's answer, not
slice 7's to invent.

### 183 (planner, surviving ruling 172) — `time_sent == recovery_start` on a paused clock

The planner's objection to the 2/2 fence split survives ruling 172's
rebuttal in one half. Ruling 172 answers **pre-roam** packets: they stay
fenced permanently because `recovery_start` is never cleared and moves
only forward. But a **post-roam** packet sent in the *same virtual
instant* as the roam has `time_sent == recovery_start`, and
`in_recovery`'s test is `sent_time <= start` — so it **is** fenced, though
it belongs to the new path.

In production this is a sub-microsecond window. **On tokio's paused clock
it is the norm**, because `reset(now)` and the sends that follow share one
`Instant` unless the test advances between them.

**Ruling:** the `≤` stands — it is §14.3's ordinary rule, RFC-correct for
the recovery case, and the roam case errs conservatively (a fenced
post-roam packet suppresses a cwnd *cut*, never inflates a window). This
is a **test-authoring instruction, not a defect**: a test asserting *"a
post-roam loss cuts cwnd"* must advance the paused clock after the roam,
or it asserts the opposite of what it names. It goes in `CONTRACT-7.md`
where both blind test authors will read it, because it is exactly the
shape of working rule 9's degenerate pass — the assertion looks right, the
name looks right, and the mechanism under test never runs.

### Round 30, closing note

**Sixteen rulings (168–183) from two agents run blind to each other**, on
a slice not yet dispatched. The planner and the auditor independently
found **six of the same defects**, which is the strongest signal this
arrangement produces — and each found several the other did not. The
planner also invoked **working rule 5** against its own brief: my scope
statement was narrower than ruling 91's amendment, which had moved §6.4's
PENDING branch into slice 4. **S4 is therefore already built**; its tests
are still written, and a pass with no implementation change is the correct
outcome, not a wasted slice item.

**Unverified and flagged rather than asserted** (planner, out of budget):
ADV-S-4, §6.9/§17.5's cost accounting for the probe. The auditor
separately did not read §6.3's stage-0 queue or §12's ACK derivation, on
which rulings 175 and 176 both lean. Recorded so the adversarial review
after this slice knows where the floor is thin.

### 184 — ruling 181 amended: the arithmetic dissolves the Q5 collision

The planner's recorded default for Q5 was *"the extends-to-end frame wins;
**the PING takes its own packet**"*, with the instruction *"do not write a
test that asserts a probe rides a data packet."* Ruling 181 said PING is
packed immediately before the extends-to-end frame and offered the `0x31`
LEN form as an equivalent escape. **Both were reasoning without ruling
155's arithmetic, and the arithmetic answers it.**

Ruling 155: `MAX_DATAGRAM_PAYLOAD` = 1169, `MAX_PLAINTEXT` = 1170, and a
maximum-size datagram in the `0x31` form needs 1 + 2 + 1169 = **1172
bytes and cannot be sent at all** — which is why the `0x30` form is
mandatory rather than an optimisation.

Carry that one step further, which neither of us did:

> A maximum-size datagram in `0x30` form occupies 1 + 1169 = **1170
> bytes — exactly `MAX_PLAINTEXT`.** It shares its packet with **nothing**:
> no PING, no ACK, no control frame, no stream fill.

**So for a maximum-size datagram the collision is vacuous** — it is alone
in its packet under every packing rule, and ruling 155's documented bias
(*"preferentially delayed by any packet carrying control frames"*) is
precisely this fact, already ratified. The collision arises **only** for a
*sub-maximum* extends-to-end frame, where room remains.

**Ruling 181 stands, sharpened:**

- A **maximum-size** `0x30` datagram is alone in its packet. Nothing is
  packed with it, so no rule is needed and none is violated.
- A **sub-maximum** extends-to-end frame leaves room, and **PING is packed
  immediately before it**. A probe *does* ride such a packet.
- The `0x31` escape is **withdrawn for datagrams**: ruling 155 makes
  `0x30` mandatory, and the sizes where `0x31` would fit are exactly the
  sizes where nothing was blocking. It remains available to STREAM frames,
  which have a LEN form ruling 155 does not constrain.

**The planner's default is therefore superseded**, and its test
instruction inverted: a test **may** assert that a probe rides a data
packet, provided the datagram aboard is sub-maximum. `CONTRACT-7.md`'s
§7.2 and its ⚠ table are corrected.

**What this says about the process.** Two agents and the integrator each
reasoned about Q5 from §8.5's *words* and reached three different answers;
the question was settled by two constants in a ruling all three had read.
Working rule 8 says to ask what bounds a construction — here the bound was
**numeric**, sitting in a ratified ruling one section away, and each of us
treated a packing question as a question about precedence. **When a
spec-text conflict is about capacity, do the arithmetic before taking a
position.**

### 185 (planner Q10) — a second write to an occupied notification slot takes the **new** generation

The planner recorded this as a default awaiting a ruling, on the
assumption — from §5.4's own table — that *"ruling 41's collapse means at
most one of each can ever be pending."* **Ruling 175 falsifies that
assumption's scope.** Ruling 41 collapses concurrent *marks*: at any
instant at most one mark exists. It does **not** bound how many marks
occur over a connection's life, and ruling 175 established that a **live**
peer ACKs in ~1 RTT, clears the mark, and the next refusal is a full
second mark. So `Contested`, `ContestCleared`, `Contested`, … is ordinary
traffic on a healthy contested connection, and an application that does
not drain between them **will** find an occupied slot rewritten.

This is defect class 1 once more, and this time inside a contract rather
than the spec: *"at most one of each can ever be pending"* is a true
statement about marks read as a statement about **slots**.

**Ruling: the latest write takes the slot and the new generation.**

The generation is what §16.4 orders handover by, so taking the new one is
what makes the pair read as **current state**. Worked through, with the
application never draining:

- `Contested`(g1) → `ContestCleared`(g2) → `Contested`(g3). The
  `Contested` slot now holds g3, the `ContestCleared` slot g2. Handover in
  generation order gives **cleared, then contested** — "contested now".
  Correct.
- Had the slot **kept** g1, handover would give **contested, then
  cleared** — "cleared now", the exact inverse of the truth, and
  unfalsifiable from the application's side.

So the alternative is not merely lossier, it is **wrong in a specific
direction**: it reports a contested connection as healthy, which is the
one error S11 exists to prevent (*"refused but healthy, keep using it"*
versus *"refused and about to die, prepare to redial"*). A re-dial
scheduler reading it would stand down exactly when it should dial.

Retention stays **one slot per kind** (§17.5 — nothing to bound), and the
merge rule for `AddressMoved` is untouched: oldest unclaimed `from`,
newest `to`, because that pair describes the *net* move, whereas a
contested mark is a *state* and only its latest value is meaningful.

`CONTRACT-7.md` §5.4's *"at most one of each can ever be pending"* is
corrected to say what is true: **at most one mark exists at any instant,
and a slot may be rewritten any number of times.**

### 186 — CLOSE's rank in the budget priority order, and ruling 171 amended

The applying agent found that **ruling 171's enumeration omits CLOSE**
— *"ahead of ACKs, keepalives, PTO probes, retransmissions and new
Data"* — and refused to either drop it or silently rank it. It placed
CLOSE provisionally and **marked the placement unratified in the spec text
itself**, so that moving it later is a ruling rather than an excavation.
That is working rule 5 and working rule 3 both applied correctly, and it
was the right call.

**The defect is mine, and it is defect class 1 in a ruling of mine for the
fourth time.** I wrote a list of output classes to establish a priority
and did not state what bounded it; §14.5 and §13.4's exemption lists both
name CLOSE, so a reader assembling the contenders from the spec gets six
classes where my ruling gives five. *"A stated construction with an
unstated or contradicted scope"* — written into the ruling that exists to
fix an unstated scope.

**Ruling: CLOSE ranks FIRST, above the pending contested probe**, and
ruling 171(a)'s *"priority over all other output"* is amended to *"over
all other output except CLOSE"*. Full order to an unvalidated address:

> **1.** CLOSE · **2.** a pending contested probe · **3.** pure ACKs ·
> **4.** PTO probes · **5.** keepalives · **6.** retransmissions ·
> **7.** new application data.

**Why CLOSE outranks even the probe.** §16.5 already states the governing
principle for exactly this kind of tie: *"a terminal outcome precedes a
routine one."* A connection that is closing has **no use for the probe's
verdict** — the probe exists to decide whether to reap a connection, and
one that is leaving has already answered that question. Ruling 179 points
the same way from the other side: a contested mark taken on an
already-closing connection is a **no-op**, so the two states barely
co-exist, and where a mark taken while live survives into closing, its
verdict is moot. Meanwhile a CLOSE that the budget will not admit costs
the peer a full `DEAD_TIMEOUT` (25 s) to learn what one small packet would
have told it immediately.

Both are small and both are cwnd-exempt, so this ordering costs nothing in
the common case; it decides only the scarce-budget case, which is the one
ruling 171 exists for.

### 187 — §17.1's guard pin follows ruling 178's predicate

The applying agent found three further sites reading *"in-flight outbound
pending"* — §17.1's pin rule (twice), §6.5 and §6.6 — and **declined to
edit them**, on the ground that they describe the pending *object* rather
than the PENDING *predicate*, and that extending a predicate into an
eviction rule is a semantic change ruling 178 did not make. It fixed
§5.4's definition, which those sites inherit, and flagged the rest.

Correct on both counts, and the flag deserves an answer rather than
inheritance.

**Ruling: the pin follows the predicate.** A pending in the tables pins
the static's guard entry against §17.1's orphan aging and LRU eviction,
whether or not `start_attempt` has run.

**Why.** The pin exists so that the guard entry we are **about to need**
— to validate the msg2 or the replacement that our dial will produce — is
still there when it arrives. `mint_pending` is a declared intent to dial;
the entry is needed for the same reason a moment later. Leaving the
interval unpinned opens a window in which precisely the entry the attempt
depends on can be evicted, and the window is invisible because nothing
observes it until an eviction and a dial coincide.

**And the extension is self-bounding**, which is what makes it cheap: the
pin lasts exactly as long as the pending, and ruling 50 makes dropping a
`Connecting` empty the pending tables synchronously. There is no path by
which a minted-and-abandoned pending holds a guard entry indefinitely —
the same tables that answer "is this static PENDING?" answer "is this
entry pinned?", which is ruling 178's point restated.

**One thing this ruling does not do:** it does not widen §17.1's eviction
*scope* in any other respect. Ruling 37's `HANDSHAKE_GIVEUP` exemption
for tie-break and winner-side records is untouched, and the 1024 cap
still bounds the tier.

### Round 30, second closing note

The round now stands at **twenty rulings (168–187)** and its last three
came from agents declining to do what they were told, or finding what they
were not asked for:

- The applier found **six** sites of contrary prose beyond its brief's
  list, one of which (§6.3) stated ruling 175's false bound
  **independently** of the two sections I had identified. Working rule 4
  paid for itself in a single pass.
- It **declined twice** rather than resolving, and both declines became
  rulings (186, 187). Every agent that has reported rather than resolved
  has been right — now eight for eight.
- Its own flagged residual is the sharpest thing to hand the adversarial
  review: ruling 168 frees the budget counters **at validation**, so a
  connection that validates and then roams to an attacker's address
  re-arms from zero and carries no credit across. That is correct, and it
  is the property most worth attacking.

## Round 31 — slice 7's blind agents report (2026/08/16)

### 188 (T-K's K1) — ruling 175 reinstated a phrase ruling 43 had **denied**, and did not say why it may

Test author T-K, working blind, found this by applying working rule 4 to a
ruling rather than to the spec — grepping for the *rationale* behind the
number, not the number:

> Ruling 43 did not merely state a bound, it **denied a
> characterisation** — *"not by 'the application's own accept rate', which
> was false because the attacker supplies the Intros"* — and **ruling 175
> reinstates that exact phrase** (*"the refusal rate is the application's
> own `accept()` rate"*) without addressing 43's stated reason.

That is exactly right, and it is my defect. Ruling 175 corrected ruling
43's *number* and silently restored the *characterisation* 43 had rejected,
which is the one the security argument rests on.

**Ruling: both are true, and the reconciliation must be stated rather than
left for a reader to reconstruct.**

- The **rate ceiling** is the application's own `accept()` calls. A
  refusal cannot occur without one, and no attacker can cause one. Ruling
  175's number is right.
- **Which** of those calls becomes a refusal-and-probe is
  **attacker-influenced**, because the attacker supplies the parked
  `Intro`s that the call refuses against. Ruling 43's objection is right.

So the honest statement is *"bounded above by the application's own
`accept()` rate, though which calls within it produce a probe is
attacker-chosen"* — and **43's original target stays dead**: the claim
§7.5 once made, *"bounded by … nothing an attacker controls"*, remains
false and must not return. §7.5, §6.9 and §6.3 all carry the reconciled
form; `CONTRACT-7.md` §0's ruling-43 row gets a supersession marker, which
it was missing while ruling 40's row beside it carried one.

**What this says:** working rule 4 has been applied to the spec eleven
times and this is the first time an agent has applied it to the **rulings
file** — and it found a defect there on the first attempt. A ruling that
reverses another's conclusion inherits the duty to address its reasoning.

### 189 (T-K's gap 2) — the shell gains a reader for the configured beacon

Ruling 44 makes *"a rejected call leaves the interval **unchanged**"* an
explicit acceptance criterion, and S5 names it. **§16.2's shell surface
has no way to observe it**: `set_persistent_keepalive` is a setter with no
getter, and the core's `persistent_keepalive()` is `pub(crate)`. T-K
tested it behaviourally — inferring the interval from beacon cadence over
a 3 × `DEAD_TIMEOUT` window, bracketed from **both** sides so that neither
an upward nor a downward clamp survives — which is good work on a bad
seam.

**Ruling:** add `Connection::persistent_keepalive() -> Option<Duration>`
to §16.2. Wire-free, no core change (the value exists and is already
tracked), and it turns a ratified acceptance criterion from a
three-timer-cycle inference into one assertion.

**Why now rather than at slice 8's API review:** a configuration setter
whose effect cannot be read back is the kind of surface that review would
flag anyway, and leaving it means every future test of ruling 44 pays
T-K's cost. The addition is additive and breaks nothing.

Note the shape: ruling 44 was ratified with an acceptance criterion whose
**observability was never checked**. That is a cousin of working rule 11 —
a rationale naming a mechanism that does not exist — applied to a *test
obligation* rather than to a mechanism. Appendix B should be swept for
others before slice 9.

### 190 (T-K's K2) — the contested PING joins §7.4's quiet set explicitly

§7.4's liveness model has two halves of different logical shape, and the
frame slice 7 introduces falls between them:

- the **`seal`** (marking) side is a **closed positive characterisation** —
  application intent — which a PING-only packet fails, so it is quiet by
  construction;
- the **`seal_quiet`** side is an **enumeration** — pure ACKs, PTO probes,
  retransmissions, credit frames, RESET_STREAM, CLOSE — which working rule
  8 reads as exhaustive, **and the contested PING is not in it.**

Ruling 182 said in terms that the contract must *"quote §7.4, not
re-derive it"*, so an agent obeying 182 finds the frame it needs absent
from the list it was told to quote.

**Ruling:** the contested probe's PING is **quiet** — it does not move
`S`, and it is added to §7.4's enumeration by name. It must be quiet: a
marking probe would drag `last_send` forward and suppress the very passive
keepalive whose absence the probe is trying to diagnose.

**And the enumeration is given a stated scope** so it stops being a trap:
*"every send not covered by the marking characterisation above is quiet;
the list is illustrative of the classes that arise, not a definition."*
That inverts which half is authoritative — the closed characterisation
decides, and the list follows it — which is what §7.4 already meant and
what makes the two halves stop disagreeing about frames neither anticipated.

Nothing was blocked by this: T-K noted the contract lands on the right
side, and no test depended on the class. It is filed because the next
frame to arrive will hit it again.

### T-K's own limits, recorded

Working rule 13, self-reported and verified by arithmetic rather than
asserted: **the `Contested::Pending` gap is not constructible from an
integration test.** After a roam `budget_sent == 0`, and the probe is
~31 B against `3 ×` the roam trigger (itself ≥ a ~30 B keepalive), so
~90 B admits it outright; and driving the spend up is self-defeating,
because every small packet the peer sends raises the cap by 3× what a
reply costs. `CONTRACT-7.md` §8.3 calls the mark/transmission separation
*"the single most testable property in the slice"* — **it is not testable
at that level at all.** It is reachable only where a connection is already
spending into an unvalidated address, which is a core-level construction.

**Integration owes this coverage, and it must not be written by anyone who
has seen the implementation** (working rule 6). If the implementer's own
tests do not reach it, the integrator dispatches a **fresh blind agent**
against the contract rather than writing it directly.

Also unreached and correctly declined: §5.5's three `slither::policy`
traces (no `tracing` subscriber in dev-dependencies — *"adding one is not
a test author's call"*, and it is the same debt slice 9 already carries
from rulings 162/164), and *"nothing is transmitted at the verdict"*,
which cannot be sampled atomically.

**`cargo clippy` was blocked twice by this environment's permission
classifier, and T-K did not claim it green.** Working rule 7 observed
exactly.

### 191 (T-M's finding 3) — the two tie-break routes resolve the **dial** differently, and both are right

`CONTRACT-7.md` §10 says *"`AlreadyConnected` covers the tie-break-loser's
cancelled pending"*. T-M, blind, found that this is true on **one** of the
two routes, and that read as exhaustive (working rule 8) it sends an author
to the wrong assertion on the **more common** one. Verified in the code
rather than the text (working rule 11):

- **Staged route** (§6.4's PENDING branch, ruling 35) —
  `cancel_pending_losing_tiebreak` (`routing.rs:688`) drops the pending and
  emits `HandshakeFailed(dial, ConnectError::AlreadyConnected)`, so the
  loser's `Connecting` resolves **`Err(AlreadyConnected)`**, and a *new*
  connection is minted for the accepted initiation.
- **Internal route** (§6.6) — the pending is **promoted in place**
  (`routing.rs:544–562`): the same connection receives `Install { role:
  Role::Responder }`, so the loser's own `Connecting` resolves
  **`Ok(Connection)`**.

**Ruling: both stand, and §6.7's claim is amended.** §6.7 says the only
visible difference between the routes is `AcceptError::Stale` versus a
silent drop. That is now false — there is a **second** difference, on the
dial side — and it must be stated, because it is the one an application
actually writes code against.

**And the difference is forced, not incidental**, which is why neither
route changes: the two routes differ in **which call owns the resulting
handle**. On the staged route the application drove
`read_identity()` → `authenticate()` → `accept()`, and `accept()` returns
the connection, so the dial has nothing left to deliver and
`AlreadyConnected` is the honest answer. On the internal route §6.6
guarantees *"the application never sees it"* — there is no `accept()` to
return anything — so the `Connecting` is the **only** handle that can
carry the connection, and promoting in place is the only way to hand it
over. Each route delivers exactly one connection; they differ only in
which verb delivers it.

`CONTRACT-7.md` §10 is corrected to state both rows.

### 192 (T-M's finding 1) — S4's mixed staged/internal case has an ordering precondition

S4 requires the tie-break to hold *"when one side reaches the decision via
the staged path and the other via the internal tie-break"*. T-M found that
the obvious construction cannot produce it: §6.5's eager path reclaims a
**parked** initiation whose claim is in the pending outbound remotes, so a
`read_identity()` issued **after** the local `connect()` returns
`IntroError::Internal`, and §6.4's staged PENDING branch is never entered.
Observed directly: `dial = Ok(...)`, `ladder = Err(Intro(Internal))`.

**Ruling:** the precondition is real and belongs in the contract, not in
each author's rediscovery — **the Intro must be climbed to `Proven`
before the local dial** for the staged route to be reachable. This is a
consequence of §6.5's routing rule, which is correct as written; nothing
in the protocol changes. `CONTRACT-7.md` gains the ordering note beside
its S4 sequence.

Related and **not** a defect: T-M's finding 2, that §4.1's `t_cand <= t`
row is unreachable from a story test because §17.1's guard is consulted in
`authenticate()` (`staged.rs:491`), so a stale candidate dies at stage 2
with `AuthError::Replay` and `accept()` is never reached. The row is right
at core level; it is simply not pinnable from the shell, and T-M asserting
the *outcome* while accepting either refusal is the correct response.

### 193 — the mark/transmission separation is unreachable at integration. **Both blind authors found it, from different directions.**

`CONTRACT-7.md` §8.3 calls the mark/transmission separation *"the single
most testable property in the slice"*. **It is not testable at that level
at all**, and the two test authors established this independently, neither
knowing the other existed, by two unrelated arguments:

- **T-K, by arithmetic.** After a roam `budget_sent == 0`, and the probe is
  ~31 B against `3 ×` the roam trigger (itself ≥ a ~30 B keepalive), so
  ~90 B admits it outright. Driving the spend up is self-defeating: every
  small packet the peer sends raises the cap by 3× what a reply costs.
- **T-M, by construction.** The fixture cannot build a connection that
  both roams **and** is contested. `SharedWire` has no public constructor,
  `FlakyWire` is not `Clone`, and `EndpointBuilder::wire` takes by value —
  so only `Pair`/`Peer` endpoints can rebind, and `Peer` never exposes an
  `Identity`, which a contest requires.

Round 5 recorded that two independent reviews finding the same blocker from
different directions is *"the strongest confirmation this process
produces."* This is the second occurrence, and the first between agents who
were mutually blind by construction rather than merely run separately.

**Ruling, three parts.**
(a) `CONTRACT-7.md` §8.3's claim is **struck**; the property is core-level.
(b) The coverage is **owed**, and by working rule 6 it may not be written
by anyone who has seen the implementation. If the implementer's own tests
do not reach it, the integrator dispatches a **fresh blind agent** against
the contract — not a test written directly by the integrator.
(c) T-M's structural finding is a **fixture** gap in its own right and is
recorded as such: a connection that can both roam and be contested is not
constructible, and slice 8 or 9 should decide whether `Peer` grows an
identity accessor or `EndpointBuilder::wire` takes a shared handle. **Not
fixed now** — changing the builder's signature mid-slice would edit a path
the implementer holds.

This is working rule 13 twice over, and it says something sharper than
before: **the fixture bounded the coverage, and the budget arithmetic
bounded it independently, so no amount of fixture work alone would have
exposed the whole gap.**

### 194 (T-M, for the integrator) — the `[[test]]` stanzas are required for the gate to run, not for tidiness

T-M verified that `Cargo.toml` carries no `autotests = false`, so cargo
**auto-discovers** `tests/story_mobility.rs` *without* its
`required-features`, and `cargo test --test story_mobility` fails on
`unresolved import slither::testutil`. So the stanza is not cosmetic:
**without it the feature-less `cargo test` gate cannot pass at all.**

Recorded because working rule 15 established that these stanzas are the
integrator's — cargo *refuses to parse* a manifest naming a missing test
file, so an implementer adding them early leaves a tree on which no gate
runs. The rule said *whose* job it is; this says **what breaks if the job
is skipped**, which is the half that was missing. A slice could otherwise
end with an integrator seeing a green `cargo test --all-features` and a
red bare `cargo test` and mistaking a manifest gap for a test failure.

### Round 31's shape so far

Both test authors delivered, mutually blind, on disjoint paths: **28
story tests, ~3 500 lines.** Each verified its own compile errors
**positively** — temporarily shimming the missing APIs, confirming a clean
build, then reverting — so that "all my errors name contract-promised
APIs" is evidence rather than inspection. T-M went further and ran the
shimmed suite: **6 passed, 10 failed**, every failure at the first slice-7
behaviour its test touches, and **both S4 tests among the six that pass** —
which is §1.2's predicted correct outcome, ruling 91 having moved that work
into slice 4.

Neither claimed a gate it had not run. T-K reported clippy blocked by the
environment; T-M reported it clean on its own target with the shim in
place.

### 195 — §7.5's passive rule is a **flag**, not a timestamp comparison. **Ninth time the prose held the intent.**

Found by integrating T-K's blind story tests: `s5_one_exchange_..._self_sustains`
failed with *"the initiator put 0 datagrams on the wire in 75 s"* — and the
connection was **alive**. Instrumented at the core, both sides read:

```
role=Responder S=T R=T R>S=false armed_deadline=false
role=Initiator S=T R=T R>S=false armed_deadline=false
```

**S and R exactly equal, and `armed` false.** No keepalive (the passive rule
needs `R > S`) and no death (`armed` gates the deadline). That is precisely
SECV5-2's **immortal half-open session** — the failure ruling 39's install
pin was applied to prevent — reconstructed through a different door.

**Why they are equal, and why this is not a paused-clock artifact.** §7.5
gives the rule two ways in one sentence:

> *"a side that **has received since it last sent**, and has not made a
> marking send for `KEEPALIVE_TIMEOUT`, sends a keepalive"* (prose)
> — against `S` = `last_send`, `R` = `last_authenticated_recv`, predicate
> **`R > S`** (formal).

*Has received since it last sent* is a **flag**. `R > S` is a **comparison**.
They agree everywhere except when the two instants are **equal**, and equal
is reachable: the shell driver caches `let now = now()` **once per turn**, so
a receive and a send handled in the same turn share an `Instant` in
production, not only under `start_paused`. On the paused clock nothing
advances between install and the exchange, so it happens **every** time —
which is why a blind story test found in one run what the core's own unit
tests, each advancing the clock between steps, could not.

Ruling 39's derivation is unaffected and is the reason the naive repair is
wrong: at install `R == S` deliberately, so `R >= S` would make an
idle-from-install connection keepalive immediately and destroy the 25 s reap
that ruling 39 exists to produce. The comparison cannot distinguish *"nothing
received since install"* from *"received at the same instant as install"*,
because it is the wrong instrument.

**Ruling:** the passive rule is carried as **state, not arithmetic** — a
`received_since_last_marking_send` flag, cleared at install and by every
marking send, set by every authenticated window-fresh receive. The keepalive
deadline stays `S + KEEPALIVE_TIMEOUT`. This is exactly the prose, is
equivalent to `R > S` whenever the instants differ, and is correct when they
coincide. §7.5's formal statement is rewritten to the flag; ruling 40's proof
carries over verbatim with *"whenever `R > S`"* reading *"whenever the flag
is set"*, and its *"the only state that blocks the dance is `S > R`"* reading
*"the flag being clear"*.

**This is the ninth time the prose held the correct intent and the formal
rule held the bug** — and the sharpest instance yet, because I edited *this
very sentence* two rounds ago (ruling 182, `"has not sent"` →
`"has not made a marking send"`) and corrected one half while the other half
carried this. Working rule 4 says to grep for the rationale, not the token; it
needs a companion: **when you correct one clause of a sentence, read the
other clauses of that sentence.** Added to `CLAUDE.md` as part of rule 4.

**Working rule 9, from the other end.** The paused clock *is* the degenerate
case here, and it violated the bound — which is why the story test caught
what six core unit tests around the same code did not. A fixture that
collapses distinctions is not only a limit on coverage (working rule 13); it
is occasionally the **only** instrument that exposes a bound the real clock
hides.

### 196 (integration) — a "peer restart" needs the endpoint handle dropped **first**, and the test said so before it did it

Three of T-K's twelve tests failed at integration with
`ConnectionLost::PeerClosed { code: 0, reason: [] }` where they expected a
contested mark. The zombie was being killed by a **graceful CLOSE** before
the probe could run.

`Drop for Connection` (`src/shell/connection.rs:1185`):

```rust
if last_for_connection && !last_in_process {
    self.close_now(constants::NO_ERROR, b"");
}
```

**Ruling 88's design, and the implementation is right**: dropping the last
handle *for a connection* closes it politely; dropping the last handle *in
the process* does not, because the driver is stopping anyway. The test wrote

```rust
drop(cb);
b.ep = None;
```

with the comment *"every handle it had goes at once, which **by ruling 88
seals no CLOSE**"* — which names the correct rule and then defeats it by
ordering. With `b.ep` still held, `cb`'s drop is *"last for the connection,
not last in the process"*, so it seals exactly the CLOSE the comment says it
does not. Reversed, the test passes.

**Ruling: the test is corrected, not the code**, and the correction is
recorded here rather than made quietly, because a red test at integration is
where working rule 6 is most easily broken — the tempting move is to adjust
whichever side is cheaper. The author's *intent* was right and is preserved
verbatim; only the two lines' order changed. Rust cannot drop two bindings
simultaneously, so "every handle at once" always has an order, and **which
order is not a detail — it selects between two ratified behaviours.**

**What this says about the seam.** A crash and a graceful shutdown differ
only by drop order, with no API that says which you meant. That is a sharp
edge for a consumer modelling failure, and it is worth a note in §16.3's
drop table before slice 9 — the rule is stated there, but not the fact that
**sequencing two drops chooses between them.**

### Slice 7's integration result

With rulings 195 and 196 applied: **872 tests under `--all-features`, 775
bare, 0 failing, 0 ignored.** Both blind test files compiled against an
implementation neither author saw, and **T-M's 16 mobility tests passed
without a single change on either side**.

One implementation defect (ruling 195), one test defect (ruling 196), and
the implementation defect was found **only** by a blind story test — six
core unit tests around the same code missed it, because each advanced the
clock between steps and the bug lives exactly where two instants coincide.

### 197 (implementer's item 2) — `EndpointOutput` gains two shell-only variants. **Ratifying a mechanism the implementer had to invent.**

§6.4 requires the replacing `accept()` to fire `Replaced` **on the old
connection**, and a `None`-basis refusal to mark **that connection**
contested. The implementer found there is no channel:

- `EndpointOutput` (§16.4) offers only `ToConnection(ConnectionId, Install)`,
  and the text says *"Install only"* **twice**;
- `core::Connection`'s verb list has nothing either.

**Both lists read as exhaustive** (working rule 8), and between them the
endpoint core cannot reach a connection core at all — so the ratified §6.4
behaviour was **unimplementable as specified**. It built
`EndpointOutput::{Replaced, Contested}` on `HandshakeFailed`'s precedent
(which is already an endpoint output naming a connection), plus
`pub(crate) Connection::{replaced, mark_contested}`, and flagged it: *"the
mechanism is invented, not ratified."*

**Ruling: ratified as built.** It is the minimal shape — no new endpoint
verb, no public API, no wire change — and `HandshakeFailed` establishes the
precedent that an endpoint output may name a connection and carry a verdict
to it. §16.4's two lists are amended to include the variants **and to state
what bounds them**: `EndpointOutput` carries to a connection exactly those
verdicts the *endpoint* owns and the connection cannot reach — install,
handshake failure, replacement, contested marking — because each is decided
by the static map, which is endpoint state.

**This is the fourth time a §16.4 list has been found short by someone
building against it** (ruling 71's stage-0 accessors was the first). The
pattern is stable enough to act on: **before slice 9, every enumerated API
surface in §16.4 gets an explicit scope sentence**, because the section's
lists are the ones agents build from and its omissions are invisible until
they do.

### 198 (implementer's item 1) — the budget priority scheduler is **partially built**, and the reason it stopped is correct

Ruling 171 and 186 give a seven-position priority order for output to an
unvalidated address. The implementer built the two positions that carry a
verdict — **CLOSE is unconditioned** (it does not go through the pump, and
entering the post-mortem abandons the mark) and **a pending contested probe
stops the pump dead** — and did **not** schedule positions 3–7.

It says why, and it is right: `CONTRACT-7.md` §3.2 listed the probe first
and CLOSE last, while `SPEC.md` **at its own base commit** listed CLOSE
first — because ruling 186 landed after the contract was written and I
updated the spec and the brief without updating the contract's §3.2 or its
§0 table. The sub-order differed too. It refused to redesign `pump_packets`
against an order two authoritative documents disagreed about.

**That is working rule 5 exactly**, and the failure is mine: the contract
declares itself governing — *"if you find yourself reasoning from a default,
you are reading a stale copy"* — **and was itself the stale copy.** A
document that asserts its own primacy has to be updated first, not last;
mine was updated third, behind the spec and the brief.

**Ruling:** the order stands as ruled (186); the **residual is recorded as
debt, not quietly closed.** Under a scarce budget an owed retransmission can
still spend budget a keepalive should have had. This is bounded — it needs
an unvalidated address *and* a budget too small for what is owed, and ruling
168 now ends the unvalidated state on one round trip — but it is real, it is
unpinned by any test, and **slice 8 or 9 owes the budget-aware scheduler
across positions 3–7.**

### 199 — the §16.3 drop table should say that sequencing two drops chooses between its two rules

From ruling 196's diagnosis, and separable from it. §16.3 states both drop
rules correctly and `Connection`'s own rustdoc even calls the interaction
"drop-order sensitive". What neither says is the operational consequence:
**a crash and a graceful shutdown differ only by the order two bindings fall
out of scope, and no API distinguishes them.** A consumer modelling peer
failure — which is precisely what a reconnect scheduler does — has no way to
express "this peer died" other than by getting an ordering right.

**Ruling:** §16.3's drop table gains the sentence, and it is a
**documentation obligation for slice 9**, not new behaviour. Declined:
adding an explicit `abandon()`/`kill()` verb — it is API surface for a
testing concern, and slice 8's API review is the place to raise it if the
review wants it.

## Round 32 — the third blind author, and two defects it exposed (2026/08/16)

Ruling 193(b) required the `Contested::Pending` coverage to come from an
author who had not seen the implementation. A third blind agent wrote it:
**15 core tests, ~1 100 lines**, and it **compiled against the
implementation with zero changes** — including `mark_contested`, the one
API it flagged as *"possibly invented"* because `CONTRACT-7.md` specifies
the mark's behaviour and never names its method. Two agents, blind to each
other, chose the same name.

**Process defect, mine, and only the agent's discipline contained it.**
The brief named base commit `195c57a`; **the worktree was actually cut from
`ddda950`**, which contains the slice-7 implementation. The agent caught it
on its *first command*, before reading any source, reset to `195c57a`, and
reported it. Working rule 14 says an isolated agent sees a commit, not a
working tree, and to cut its worktree from a commit containing its inputs —
it did not say **verify the cut actually happened**. A brief that names a
commit and tooling that cuts from `HEAD` silently destroy the blind split,
and nothing in the result would have looked wrong. **Working rule 14 gains:
check the worktree's base before reading anything, and say what it was.**

### 200 — the amplification budget arms on `Role::Responder`, not on the constructor

The blind tests failed at once on `amplification_budget()` returning `None`
where a responder-anchored core must have one — which is exactly conflict
**C3** the same agent had reported: *"§3.2 arms on 'the connection was
created by `accept()`', but `Install { session, role }` is the core's only
signal, and `Role::Responder` does not imply 'created by accept()'."*

Verified in the code (working rule 11): §6.6's **internal tie-break loser
dialled**, lost, and installs through the `Install` path as
`Role::Responder` with `anchor: src` — *"§5.6's anchor — the **msg1
source**, because on this branch we are the responder"*
(`endpoint/routing.rs:520`). The implementation armed the budget only in
`Connection::established`, the staged-accept constructor, so **that
peer-supplied address was treated as validated** and §7.3's 3× cap never
applied to it. An off-path attacker replaying a captured initiation
(ruling 37's window) against a side that happens to be dialling gets a
reflector on the one path where a dialler adopts an address it did not
choose.

**Ruling: arm on `Role::Responder`.** A responder is by definition a side
that received msg1 and answered, so **its anchor is always the msg1 source
and never an address the application supplied** — the role *is* §7.3's
predicate, on every path, and it is the only form of it the core can see.
The blind author's assumption was right and the implementation's was wrong;
the two disagreed exactly where C3 said they would.

### 201 — ruling 184 over-read ruling 155, and `0x31` is not withdrawn

Ruling 184 (mine) told `CONTRACT-7.md` that *"the `0x31` escape is
**withdrawn for datagrams** — ruling 155 makes `0x30` mandatory"*. The
blind author built its packet arithmetic on that and every shaping
assertion missed by 2 bytes.

**Measured**: payload 1169 → packet 1200 (overhead 31, the `0x30`
extends-to-end form); payloads 1140 and 100 → overhead **33** (`0x31` with
a 2-byte length varint). The implementation picks `0x30` **only when the
datagram genuinely runs to the end of the packet**.

**It is right, and ruling 184 was wrong.** Ruling 155's *"not optionally"*
is about the **maximum-size** case only, and its arithmetic says so:
1 + 2 + 1169 = 1172 > `MAX_PLAINTEXT`, so `0x31` cannot express the
ratified maximum. It does not follow that `0x30` is mandatory everywhere —
and it cannot be, because **ruling 155 also packs datagrams *before* the
stream fill** while an extends-to-end frame must be **last**. Mandating
`0x30` universally would forbid any packet carrying a datagram *and* stream
data, which is the ordinary case ruling 155's own packing order
contemplates.

**Ruling:** `0x30` where the datagram runs to the end of the packet;
**`0x31` with a length varint otherwise, required rather than permitted.**
Frame overhead is 1 + the varint — 1 byte below 64, 2 up to 16383 (§8.1),
and **both sizes occur inside a single test file**, which is why a
one-constant model of it fails.

**This is the defect class in my own ruling, for the fifth time**, and with
a new twist: ruling 184 was itself written to *correct* an over-narrow
reading of §8.5, and it over-corrected in the other direction. **A ruling
that widens a scope should say what still bounds it.** Ruling 184's own
closing line — *"when a spec-text conflict is about capacity, do the
arithmetic before taking a position"* — is the rule it broke: I did the
arithmetic for the maximum case and generalised from the one case where the
answer is forced.

### The result

**15/15 pass.** Two implementation-facing defects (200, and the earlier 195)
and two of my own document defects (201, and 198's stale contract) came out
of a slice whose implementer reported every gate green. **Neither 200 nor
195 was reachable from any test written by anyone who had seen the code.**

### 202 (from T-M's s3c) — a second refusal is a no-op only *while the mark is outstanding*

T-M's `s3c_a_refusal_against_a_none_basis_is_stale_and_marks_contested`
asserted that a second refusal yields no notification. Diagnosed by
printing what actually arrived: **`ContestCleared`**. The original peer is
alive, answered the first probe within a round trip, and the mark had
already **cleared** — so the second refusal was a full second mark, which
**ruling 175 says is correct**.

The test was pinning the behaviour ruling 43 had and ruling 175 removed.
**Corrected the test, not the code**, and by blocking the live peer's
return path before the second refusal so the mark is genuinely still
`Armed` — which is the scope S11's guarantee actually has. Recorded rather
than done quietly, because at integration the cheap move is to adjust
whichever side is easier, and that is how working rule 6 dies.

### 203 — **a keepalive-only dance never validates a roamed address, and application data then stalls.** Slice 7 does not close on this.

T-M's `s18_a_running_keepalive_dance_carries_the_move_by_itself` fails,
and it is **not** a test defect.

- §7.5's keepalive is **§3.4's empty plaintext** — no frames, therefore
  **no ACK**, therefore not ack-eliciting.
- Ruling 168 validates an address only on **an ACK covering
  `validation_floor`**.
- So a connection carried purely by the passive dance **never validates**
  the address it roamed to. Its cap stays at `3 ×` the roaming packet —
  90 bytes for a 30-byte keepalive — and each further keepalive round nets
  only ~60 bytes of headroom.
- The pump builds a full-size candidate, `admits()` refuses it, and
  **nothing shrinks to fit**, so 2 048 bytes of application data cannot
  leave for ~20 keepalive rounds (~200 s) even though a ~90-byte packet
  would fit immediately, be ack-eliciting, and validate the address in one
  round trip.

**This is ruling 198's residual in its sharpest form** — that ruling
recorded the *ordering* of classes under a scarce budget as unbuilt; this
shows the deeper gap is that the sender never **sizes** output to the
budget at all. §7.3 says refused output is *"held"*, and says nothing about
shrinking, so the implementation is defensible against the text and wrong
against the protocol's purpose: the budget exists to be *escaped* by a
round trip, and holding a full packet is the one behaviour that prevents
the escape.

**Ruling: this blocks slice 7.** The fix is to bound the packing target by
the remaining budget — `min(MAX_DATAGRAM, room)` — so the first post-roam
packet is small, ack-eliciting, and validates the address at once. That is
a change to `pump_packets`' sizing, and it must **not** be made by the
integrator against a failing test at the end of a slice: it is exactly the
shape of change that wants its own blind pass.

**Ruling 168 is confirmed correct and the interaction is its cost, not its
refutation.** The ACK requirement is load-bearing: an authenticated,
window-fresh packet proves the *peer* sent it, and an on-path attacker can
rewrite its source to a victim's address — only an ACK proves the peer
**receives** at the address we are sending to. Validating on mere receipt
would reopen exactly the reflector §7.3 exists to close. The defect is in
the sender's sizing, not in the predicate.

**Status: slice 7 is NOT closed.** 887 passing, **1 failing**, and the
failure is a real protocol defect found by a blind story test.

---

## Round 33 — the API review gate, taken early (2026/08/16)

### 204 — the public surface is ratified as reviewed. Ten open questions all resolve to *keep*.

The review gate `PLAN.md` schedules after slice 8 was taken early, against
the surface at `d2b2a45`, because two of its ten questions get more
expensive once slice 8's adapters are built on top of them. The maintainer
reviewed the surface and answered **"happy with the API"**.

That is a ratification of the whole surface as it stands, and it resolves
all ten questions in the *keep current behaviour* direction. Recorded
individually so that reversing any one of them is a ruling rather than an
excavation:

1. **`core` stays `pub(crate)` through 0.2.** Promotion is additive and
   remains available in 0.3; demotion would be breaking. The sans-io cores
   ship unreachable.
2. **The closed error taxonomy stands.** Nine of ten enums are
   exhaustively matchable. `WriteError` alone carries `#[non_exhaustive]`
   (ruling 61, reserving `Stopped`), and `Notification` carries it for a
   later wire line. In particular **`ConnectionLost` is closed**: a new
   death reason is a major bump, accepted knowingly.
3. **0.2 ships without STOP_SENDING.** `FRAME_STOP_SENDING_RESERVED`
   (`0x05`) stays claimed and never sent; §19's deferral stands. A reader
   cannot cancel a stream, and the sender keeps buying credit for data
   nobody will read. Additive on the wire when the round happens.
4. **`EndpointBuilder::build()` keeps its panic** on a missing identity or
   wire. It is the only panic on the public happy path, and it stays: a
   builder used wrongly is a programming error, not a runtime condition.
5. **`send_datagram` stays sync, `send_message` stays async.** The
   asymmetry is the type system saying what it can — droppable into a
   bounded queue versus backpressured — and no rename is owed.
6. **`id() -> Option<StreamId>` stays `Option` on all three handles**,
   including the accepted side where the id demonstrably already exists.
   One shape beats two.
7. **`remote_static()` keeps returning by value** while the staged types
   return `&`. The reason is real (the connection holds its copy outside
   the cell so it answers after §15.2's linger drops the session) and is
   documented rather than engineered away.
8. **The wire erasure point stays inside the driver.** `Endpoint<I>` does
   not carry `W`; `Wire` stays dyn-incompatible.
9. **`open_bi()` keeps returning `BiStream`** — decided by **ruling 55**,
   not ruling 96 as this ruling first wrote. §16.2 was amended at the time
   and already matches the code; there is no divergence. See ruling 205,
   which is withdrawn.
10. **`testutil`'s attestation holds** as ruling 60 wrote it. See ruling
    206 for what that implies about a change already made.

**What this ratification does not cover.** It is a review of *signatures*,
taken against a tree with one failing test. Ruling 203's defect is sender
behaviour and moves nothing on this page; the ratification says nothing
about it, and slice 7 still does not close until it is fixed. Slice 8's
compatibility layer is **not** ratified here — it is unbuilt, and the
question of whether its adapters read correctly is a question about code
that does not exist yet.

### 205 — **WITHDRAWN.** There was no divergence, and the citation was wrong twice over

As first written, this ruling claimed §16.2 still wrote the
`(SendStream, RecvStream)` tuple that `open_bi()` had replaced, and
directed that the spec be amended to match the code. **Both halves were
false.**

`SPEC.md:4692` and `:4694` already read
`pub async fn open_bi(&self) -> Result<BiStream, ConnectionLost>` with
`// .split() → the pair` beside them, and `SPEC.md:5852` carries the
decision note in full: *"[Ruling 55 — `open_bi`/`accept_bi` yield
`BiStream`.] §16.2's signatures return the duplex object rather than the
`(SendStream, RecvStream)` tuple."* The spec was amended when the decision
was taken and has matched the code ever since.

The decision is **ruling 55**. Ruling 96 is a *scheduling* ruling — it
places `BiStream` the type in slice 4 and its `AsyncRead`/`AsyncWrite`
impls in slice 8 — and says nothing about the return shape. Ruling 204's
item 9 mis-cited it too, and is corrected above.

**How this was produced, because the shape matters more than the error.**
The API review read the *code* exhaustively — every `pub fn`, every error
variant, every constant — and then asserted a claim about the **spec**
without opening it. That is working rule 11 (*a rationale must name a
mechanism that exists — check it against the code, not only the spec*)
running in the direction the rule does not name: the rule was written for
rationales that describe code, and this one described a document. The
generalisation the rule wants: **whichever artefact a claim is about is
the artefact that must be opened.** Greping `open_bi` in `SPEC.md` — one
command — would have refuted it before it was written.

It is also working rule 4's companion (a) at one remove. The review found
nine real questions by reading one artefact carefully, and the tenth was
manufactured by reasoning about a second artefact from memory of the
first. **Nine sound findings are exactly the conditions under which the
tenth is not checked.**

No action follows. §16.2 is correct, the code is correct, and ruling 55
stands.

### 206 — `FlakyWire`'s field became a method mid-slice, and ruling 60 makes that breaking

Ruling 60 puts `Network`, `FlakyWire` and `FlakyPolicy` under semver as
**attested surface** — "renaming one of those three types is a protocol
revision". Slice 7 (ruling 180's fixture work) moved `FlakyWire`'s `addr`
from a public field to `addr()` plus `rebind()`, converting 99 call sites.

Nobody checked ruling 60 at the time, including the maintainer. The
question surfaced only while assembling the API review page, from the
attestation's own text rather than from any test — no gate can see this,
because the attestation is a promise about names and the compiler only
sees the crate's own call sites.

**Ruling: the attestation is about the three type names, not their
members, and the change stands. Ruling 60 needs no amendment — it already
said so, by reference, and this ruling's first draft did not read the
reference.**

Ruling 60's actual words are: *"The three names are contract on §18.2's
terms."* **§18.2 is `Trace targets — the operator contract`** — a table of
five strings (`slither::policy`, `slither::replay`, `slither::frames`,
`slither::roam`, `slither::io`) under the sentence *"The targets are
operator-visible contract: renaming or dropping one is a protocol
revision."*

A trace target is a **bare string**. It has no fields, no methods, no
members of any kind — there is nothing about it to freeze *except* its
name. So importing "§18.2's terms" imports, precisely and unambiguously, a
**names-only** contract. The scope was never unstated. It was stated by
reference, and the reference is exact. `src/lib.rs:145`'s gloss —
*"renaming one of those three types is a protocol revision"* — is faithful
to it.

`FlakyWire::addr` becoming `addr()` plus `rebind()` was therefore never
covered, and needed no ruling at the time.

**This ruling's first draft claimed the opposite** — that the scope was
"genuinely unstated", and that this was defect class 1 found in the
attestation rule itself. That claim was produced by reading `lib.rs`'s
paraphrase of ruling 60 instead of ruling 60, and then not reading the
section ruling 60 points at.

**Twice in this round, and the same shape both times.** Ruling 205 was
manufactured by asserting a claim about `SPEC.md` without opening
`SPEC.md`; this one by asserting a claim about ruling 60 and §18.2 without
opening either. Working rule 11 says a rationale must name a mechanism
that exists and must be checked *against the code*. Both failures are
outside the letter of that rule and inside its intent, so the rule is
generalised: **whichever artefact a claim is about is the artefact that
must be opened — code, spec, or the ruling record — and a citation is a
claim about the cited text.** A ruling that cites another ruling's
authority has not been checked until the cited ruling is read; a ruling
that cites a *section* has not been checked until the section is read.
Both here were one `grep` away.

The finding survives its own reasoning: **the conclusion was right and the
argument for it was wrong**, which is exactly the state working rule 12
warns about from the other side. Had the argument been reviewed instead of
the conclusion, the review would have cleared it.

### 207 — ruling 203's fix goes out as its own blind pass, and here is what it may not do

Ruling 203 states the fix — bound the packing target by the remaining
budget, `min(MAX_DATAGRAM, room)` — and states that the integrator may not
make it against a failing test at the end of a slice. Dispatching it,
three constraints are worth fixing in the record first, because each is a
way the fix could be built wrong while turning the test green:

(a) **The budget predicate does not move.** `Amplification::admits` is
correct as ruling 168 wrote it and ruling 203 re-confirmed. A fix that
loosens `admits`, or that exempts the first post-roam packet from it,
reopens the reflector §7.3 exists to close. The change is to what the pump
*builds*, never to what the budget *permits*.

(b) **A shrunken packet must still be able to elicit.** Validation
arrives only on an ACK covering `validation_floor` (ruling 168), so a
packet sized to fit the budget is useless if what fits is a bare ACK —
non-ack-eliciting output cannot produce the ACK that validates, and the
connection stalls exactly as it does today, one indirection later. Whether
the pump owes a PING when the admitted room holds nothing ack-eliciting is
a genuine question the pass must answer rather than assume.

(c) **`MAX_PLAINTEXT` is not the only cap that moves.** `Packing::new`
starts at `budget: MAX_PLAINTEXT` and the candidate's charged size is the
full datagram (`DATA_HEADER_LEN + plaintext + AEAD_TAG_LEN`, ruling 136).
A fix that caps the plaintext at the remaining *datagram* bytes overshoots
by 30 and re-refuses its own packet. The two units differ by exactly the
overhead, and ruling 201 already cost one round to a units error in this
area.

---

## Round 34 — the adversarial protocol review (2026/08/16)

Three reviewers, disjoint lenses, blind to each other. Seventeen findings.
The reviews are at `.slices/07-mobility/ADVERSARIAL-{amplification,handshake,liveness}.md`.

### 208 — **ruling 168 is superseded. Return routability gets an unforgeable challenge, and the wire gains two frame types.**

**[RATIFIED 2026/08/16 by the maintainer — the first wire change since v1
was frozen.]**

**The defect.** Ruling 168 validates a roamed address on an ACK covering
`validation_floor`, reasoning that *"only an ACK proves the peer receives
at the address we are sending to."* An ACK is
`{ largest, ack_delay, first_range, ranges }` — four plaintext integers
under AEAD. **`largest` is not a proof of receipt; it is an assertion by
whoever holds the key**, and §7.3's roaming threat model *is* the key
holder: the peer is the party that tells us where to send.

A connected peer therefore announces a move to victim V, waits for one
sealed packet, and returns a forged ACK spoofed from V. Cost: **two small
packets**, after which the budget is gone and reflection at V is
unbounded. Pre-168 the same attacker paid a third of the reflected volume
*continuously*. §7.3's stated purpose — *"forces an attacker to pay a
third of any flood it reflects"* — is defeated at O(1).

The attacker needs source-address spoofing, but it needed that to fake the
move; 168 adds no requirement, it removes the ongoing cost.

**§7.3's own proof already scoped itself out of this, and nobody read the
scope.** `SPEC.md:2165` argues that an ACK's coverage *"derives from the
peer's replay window … which cannot contain a counter the peer never
received, and **an attacker** holds only packets we sealed before the
floor."* That is sound — against a **third party**. It says nothing about
the peer, and the sentence's own word "attacker" is what disguises the
gap. **Defect class 1, in the proof of the ruling that reversed a
declination**: a stated construction with an unstated scope. This is the
~26th instance and the most expensive.

**Ruling.** The proof of return routability becomes a value the peer
**cannot fabricate**: a random challenge sent to the new address, echoed
back. A peer that did not receive at that address cannot guess it. This is
QUIC's `PATH_CHALLENGE`/`PATH_RESPONSE` and it is the correct engineering
answer; slither's frame layer is QUIC-shaped and has the room.

**Why now, and why this is not a wire violation.** CLAUDE.md freezes the
v1 wire, and this moves wire bytes. It is ratified anyway because **v0.2
is unreleased**: `bubble-engine` is the only consumer, its build is
already broken pending slice 9's cutover, and no third party has ever
seen a slither datagram. The price of this change is strictly lower today
than on any future day, and after publication it becomes a v2 problem
permanently. **The freeze exists to stop casual drift, not to make the
protocol unfixable before it ships.**

**The wire, proposed by me under the maintainer's ratification of the
mechanism — the code points are mine and are the part to override if you
disagree:**

| Frame | Code | Payload |
|---|---|---|
| `PATH_CHALLENGE` | `0x1a` | 8 opaque bytes |
| `PATH_RESPONSE` | `0x1b` | the same 8 bytes, echoed |

`0x1a`/`0x1b` are unused in slither and are **QUIC's own code points for
these two frames**, which costs nothing and saves every future reader a
lookup. Both are ack-eliciting. The challenge is drawn from the endpoint
RNG, is per-arming, and is never reused across armings.

**What this does not change.** The 3× budget, `AMPLIFICATION_FACTOR`, the
arming triggers, and the *held-not-dropped* discipline all stand. Ruling
203's sizing fix stands and becomes more important, not less: the
challenge must fit inside the armed budget, and a pump that cannot shrink
cannot emit one.

**Consequences to carry into the remediation slice.** `validation_floor`
and `on_ack_covering` lose their security role — ruling 168's machinery is
replaced, not supplemented, and leaving both in place would give an
attacker the old path as a bypass. §7.3, §7.5 and §12 all need reading for
prose still arguing the ACK proof (**working rule 4**: grep the rationale,
not only the token).

### 209 — S31–S33 are approved; `STORIES.md`'s D10 moves to 33

Slice 8's planner found its own acceptance criteria missing: the brief
cited S31–S33 in `STORIES.md`, they exist only as drafts in `PLAN.md` §7,
and D10 declares the list *"complete at 30 approved stories"*. Ratified by
the maintainer: the three drafts are approved as written and move into
`STORIES.md`; D10 reads 33.

This is slice 4a's defect in a new place — **a document that calls itself
binding, cited by a brief, not containing what the brief says it
contains.** There it cost a blind author ten minutes of guessing an API;
here the planner caught it before any agent was briefed, which is the
whole value of planning before dispatching.

### 210 — ruling 208 corrected in four places, three of them by the planner declining it

Ruling 208 was ratified on a summary. Planning it surfaced four defects
**in the ruling itself**, three found by an agent applying working rule 5
to a brief I wrote. Recorded before any code exists.

**(a) The declination §7.3 records — and the sentence that hides the
defect.** §7.3:2204-2218 explicitly declines
`PATH_CHALLENGE`/`PATH_RESPONSE` as *"the one reviewed alternative that
would move the wire"*, preferring `validation_floor` because it is
*"wire-free"*. Ruling 208 therefore reverses a **recorded declination**,
exactly as ruling 168 did — and CLAUDE.md warns that re-proposing a
declined idea wastes a round. This one is legitimate because the
declination's **stated ground was wire cost**, and the maintainer has
weighed a fact the declination never considered: nothing has shipped, so
the cost is at its lifetime minimum.

More important is what the same passage argues *for* `validation_floor`:
an ACK covering a counter we chose *"**cannot be manufactured without the
key**"*. That is true, and it is the whole defect in six words. **The
peer has the key.** Every comparison in that paragraph is against an
*off-path attacker*; the threat model for roaming is the *peer*; and the
paragraph never asks the question. Defect class 1 at its source — and the
declination was made on a comparison that had already scoped itself out
of the real adversary.

**(b) Ruling 208's RNG source does not exist.** It says the challenge is
*"drawn from the endpoint RNG"*. **`Connection` cannot reach
`Endpoint::rng`** — both arming sites are inside the connection core.
This is working rule 11 in my own ruling, the same shape as rulings 87
and 89, and the **third time today** I have named a mechanism without
opening the file. **Corrected: the challenge is drawn from §16.6's
per-connection sub-seed**, which `Connection` already holds, has never
used, and which §16.6 exists for. The planner notes this also deletes
`set_floor` entirely.

**(c) Ruling 208 closes A1 but not A1b, and must say so.** An on-path
attacker defeats a challenge exactly as it defeated the ACK — by
**relaying** one packet rather than forging anything. 208 does not
mention it. Writing *"return routability is now proven"* would reproduce,
inside the fix, the unstated-scope defect being fixed. **§7.3 gains an
honesty clause**: the challenge defeats a peer that never received at the
address; it does not defeat an attacker that can carry packets to and
from the real peer.

**(d) My "the wire vectors will go red by design" licence is withdrawn.**
The brief told the remediation planner to expect the golden-wire vectors
to fail and update them deliberately. **That is wrong on this design**:
ruling 208 adds two *new* type codes and moves **no existing byte**, so
every golden vector must stay byte-identical. The planner declined the
instruction and was right. A red wire test here is the stop signal in
full force.

This is ruling 51's guard again — *an agent that declined a blanket
instruction was right* — and the failure mode is precise: **a licence to
expect red is a licence to update a red that mattered.** I issued it from
"this is a wire change" without asking *which* bytes move.

**Two integration hazards the planner raised, recorded so the
implementer inherits them rather than rediscovering them:**

- **`on_ack_coverage` serves two floors.** It feeds both §7.3's
  amplification budget and §7.5's contested-probe floor. Deleting it
  wholesale to remove 168's machinery would **silently disable ruling
  176's machinery**, and no wire test would catch it. Remove the
  amplification role only.
- **Rulings 171 and 208 collide in the one state both were written for.**
  A pending contested mark returns early from `pump_packets`, so a
  challenge ranked below it in the priority order is never built. Both
  rulings are about what happens to an unvalidated address under a scarce
  budget; neither anticipated the other.

### 211 — CLAUDE.md's working rules 6 and 15 contradict each other

Rule 6: an implementer needing a test module *"declares `#[cfg(test)] mod
tests;` and **creates nothing**"*. Rule 15: a file valid only once **both**
blind agents' work exists belongs to the **integrator**, because a tree
where no gate can run is worse than either failure it prevents.

**A `mod` declaration naming a missing file is a compile error** — it is
`Cargo.toml`'s `[[test]]` failure one layer down, and rule 15 was written
about exactly that. The two rules give opposite instructions for the same
artefact.

This was already observed empirically and not recognised: slice 7's
ruling-203 implementer had to comment its `mod tests_sizing;` out to run
its gates, restore it before committing, and **report that its gates ran
without that module**. That is the contradiction, discharged by hand, by
an agent that did the right thing and could not have been told to.

**Ruling: rule 15 governs. Rule 6's "declares … and creates nothing"
clause is amended** — the implementer lands the declaration **commented
out** under an integration header, and the integrator uncomments it when
the test author's file arrives. Rule 6's partition of *paths* is
untouched and remains absolute; only its instruction about the
declaration changes.

### 212 — the §7.3 sweep, and two defects it found that were not ruling 208's

`SPEC.md` swept for ruling 208 across 24 hunks (+467/−122). No wire byte
moved: `AMPLIFICATION_FACTOR` is still 3, and `IK_MSG1_LEN`,
`IK_MSG2_LEN`, `INIT_PACKET_LEN`, `RESP_PACKET_LEN`, `VERSION` and
`PROLOGUE` are untouched. Per ruling 210(d), **a red golden vector under
this change means the implementation went further than was ratified.**

**(a) §6.9:2045 carried the same defect, in a different section, and
working rule 4 is the only reason it was found.** Its 588-byte
amplification bound rested on the premise *"the spoofed source is not the
peer"* — a statement about **who holds the key**, which is precisely
ruling 208's defect wearing different words. A sweep that greped
`validation_floor` would have walked straight past it. Re-grounded on
*"nothing receives at the spoofed source"*, which is a statement about
the **address** and survives a key-holding adversary. **Second confirmed
instance of the ruling-208 defect class in this document**, and evidence
that the class is a habit of the text rather than one bad paragraph.

**(b) Ruling 186 was never fully swept, and it is mine.** §14.5 said a
pending contested probe ranks *"ahead of everything"* and §15.4 said it
*"outranks all other output"* — both written before ruling 186 put CLOSE
first. §7.3 and §6.9 carried the amendment; these two did not. The
sweeper was editing both sentences for 208 and corrected them **visibly**,
naming the old wording so the change is auditable, rather than silently.
That is working rule 4(a) — *when you correct one clause of a sentence,
read the other clauses* — applied by an agent to a defect the maintainer
left behind.

**(c) The ranks collision, resolved.** Ruling 171 ranks a pending
contested probe second, and `pump_packets` returns early when it cannot
be sent — so a challenge ranked below it would never be built. The
sweeper flagged this rather than resolving it, and in doing so found the
better framing: **nothing in §7.5 or ruling 171 mandates an early
*return*; they mandate a *rank*.** The early return is the pump's own
realisation and is strictly stronger than anything specified.

**Ruling: `PATH_CHALLENGE` and `PATH_RESPONSE` rank immediately after
CLOSE, above the contested probe.** The argument is the one that already
puts CLOSE first: **everything else in the order competes for the budget;
the challenge dissolves it.** Ranking the output that removes the
constraint above the outputs that consume it is not a preference, it is
the only ordering that terminates. The probe and a challenge together are
~40 B, inside the 90 B floor a minimal roam funds, so in practice they do
not compete at all — but the rank must be right for the case where they
would.

The pump's early return is consequently **wrong as written** and is the
implementer's to fix: it may not block the one frame that ends the state
it is protecting.

**(d) Four derived points, ratified as derived.** The sweeper marked
these as inferred rather than ratified and asked. All four stand:
retransmission class is `never` **plus a standing obligation** (the
no-deadlock proof cannot survive "sent once, lost forever"); a mismatched
`PATH_RESPONSE` is a **semantic no-op**, because `PROTOCOL_VIOLATION`
there would be a keyless remote kill primitive; **at most one outstanding
response**, overwritten rather than queued, per §17.5's ceiling
discipline; and the response obligation is **kept across a roam**.

**(e) The scope extension was necessary and is ratified.** The brief named
§8.5 and §7.3's priority order. The sweeper also edited §8.3 and §8.4,
because **without a frame-table entry `0x1a` is an unknown type and §8.2
requires killing the connection on the very frame §7.3 now mandates
sending.** It flagged the extension rather than performing it quietly.
That is the correct handling and the correct judgement.

**(f) Two source comments now quote text that has changed.**
`src/core/connection/mod.rs:1960` cites `SPEC.md:2172` and `:2002` cites
`SPEC.md:2170`. Both sentences survive — at 2242–2247 and 2239 — but the
first now reads *"elicit the `PATH_RESPONSE` that ends it"* rather than
*"the ACK"*. **The remediation implementer owns these**; the sweeper
correctly did not touch `src/`. Citing a spec line **by number** from a
comment is fragile in a document under revision, and the two instances
here are the argument for citing section and sentence instead.

### 213 — I2's remediation, and a third instance of the claim-about-an-unopened-document defect

**(a) An adversarial reviewer asserted a spec gap that was not there.** The
handshake review's M1 said §6.1's hazard paragraph *"discusses only the
claimed static"*. It does not: `SPEC.md:1147-1149` reads *"the same
discipline applies one stage earlier: `source()` and `sender_index()` are
exposed at 0 DH and are equally attacker-chosen"*, and `git log -S` dates
that sentence to the initial ratification commit `2274981`. I verified
both.

M1's **conclusion** survives — the crate's rustdoc genuinely carried no
warning on `Intro::source()`, and that is now fixed. Only its premise was
wrong.

**This is the third instance today of one defect**: rulings 205 and 206
were mine, this one is a reviewer's. All three asserted something about a
document without opening it, and all three were one `grep` from
refutation. Working rule 11's generalisation — *whichever artefact a claim
is about is the artefact that must be opened* — is now carrying three
confirmed cases in a single round, which makes it the most active rule in
the file. I2 refuted this one by opening the section **and** dating the
text with `git log -S`; that second step is the one nobody else has taken,
and it is what distinguishes "the spec is silent" from "the spec was
amended after the code".

**(b) The implementer caught working rule 9 in its own contract.**
`CONTRACT-7b.md` §5 specified F3's test as *"chunk count and byte content
unchanged, no read available"*. I2 wrote that test and found it **passes
against the pre-fix build** — it asserts nothing. It then found a real
separator: ruling 94's `capacity()` accounting after a **partial** read,
measured at 4096 → **4080** pre-fix (the realloc shrinks a drained chunk
to exact capacity) and 4096 → **4096** post-fix.

**Ruling: the contract was wrong and the implementer was right to say so
rather than ship the test.** The separator is not handed to the blind test
author T2 — it must derive its own or report that it cannot, which is what
it was briefed to do. **If T2 reports no separator, the integrator adds
this one and the disagreement is recorded**; if T2 finds one
independently, that is two derivations and worth more than either.
Handing a blind author the answer converts an independent check into a
transcription.

**(c) `CONTRACT-7b.md` §5 overstates its own invariant.** It claims the
work becomes *"bounded by bytes that are new to the buffer"*. Bounded by
the **existence** of a new byte, yes; **not linear in them** — a frame
bridging two stored chunks still copies the whole span for one new byte.
Defect class 1 in a contract this time: a stated construction with a scope
broader than what it delivers.

**(d) H3 is a spec gap, not a code defect — confirmed, and the doc-only
fix stands.** §16.6 scopes itself to the endpoint core's RNG and the
per-connection sub-seed; nothing in the spec governs the `Identity`
provider's RNG, and the code mirrors that scope exactly. A
`SoftwareIdentity::generate_os()` still wants a ruling. One correction to
the plan's reasoning: `rand_core` 0.10 ships **no** `OsRng`/`from_os_rng`,
so the consumer recipe really is `getrandom::fill` + `from_seed`, which is
what was documented — actionable today rather than blocked on a ruling.

### 214 — two blind agents derived the same separator independently, and the harness could not see the attack

**(a) The convergence.** I2 (implementer) and T2 (test author), blind to
each other, both found that F3's fix is invisible to the test shape
`CONTRACT-7b.md` §5 specified, and both landed on **the same separator**:
ruling 94's `reassembly_capacity()` measured after a **partial read**.
`Reassembly::read` drains the front chunk with `Vec::drain`, which does not
shrink the allocation — so a covered-range insert is visible as a capacity
**collapse** in the broken build and no change in the fixed one. I2
measured 4096 → 4080; T2 read 16 000 of 16 384 and measured 16 384 → 384.

Same mechanism, different constructions, neither able to see the other.
This is the second time in this project that mutually blind agents have
converged on one property (ruling 193 was the first), and it is the
strongest evidence the split produces: **a separator two independent
derivations reach is a property of the code, not an artefact of one
agent's reasoning.**

**(b) The finding is not observable in the attack's own configuration, and
T2 said so in the test.** The F3 attack withholds byte 0, so nothing is
ever read, so there is no slack in any allocation, so the re-allocated
span has the same capacity as the chunk it replaced and
`reassembly_capacity()` is **blind to it**. T2's pin therefore exercises
the same line of `insert` in the one buffer state where the allocation is
visible, and the test that reproduces the finding's own sequence carries a
doc comment saying it does **not** separate the builds.

That is working rule 13 in its sharpest form yet — not "the fixture cannot
reach the behaviour" but **"the fixture cannot reach the behaviour in the
configuration that makes it a defect"** — and it was disclosed in the test
rather than papered over. A test named for the attack that silently failed
to pin it is precisely the slice-2a failure.

**(c) A reviewer's "unbounded" claim, refuted by arithmetic.** The liveness
review called F1's `armed == false` variant unbounded. T2 could not build
it and showed why: the passive debt is set only by an authenticated fresh
receive, and that same datagram credits §7.3 by `3 × len ≥ 90` bytes —
strictly more than the 30-byte keepalive and the 31-byte probe — so **the
receive that creates the debt also lifts both holds.** Returning below 30
while preserving `armed == false` needs a non-marking, non-ack-eliciting
send, and the only one is a pure ACK (~35 B) against the ≥ 90 B its own
trigger credited: the room grows monotonically.

**Ruling: F1's unbounded variant is withdrawn; the bounded variant
stands** and is pinned. T2 explicitly declined to claim the neighbouring
case (a large quiet retransmission draining the room in one packet, which
would spin with `armed == true`) because building it needs assumptions
about `Packing`'s budget clamp a blind author would be guessing at. It
also stated the gap it leaves: **if the implementer wires the new guard
into the beacon's arming but not the passive one, nothing in its file
catches it.** The integrator owns that check.

**(d) The contract was stale at dispatch, and both implementers caught
it.** `CONTRACT-7b.md` still read *"Status: awaiting phase 0 … four open
questions block dispatch … the maintainer amends this file **and commits
it, before either blind agent is cut**"* at the moment all four were cut.
The questions were genuinely resolved — in rulings 210 and 212 — but the
**binding** document said otherwise.

**Committing the rulings is not committing the contract.** This is working
rule 14's second half failing on the person who wrote that very sentence
into the briefs, and it is slice 4a's defect one artefact over: there, an
uncommitted contract cost a blind author ten minutes of guessing an API.
Here it cost nothing only because both implementers read the rulings and
reported the discrepancy instead of trusting the file that calls itself
binding. Contract now updated to point at the rulings.

### 215 — ruling 212(c) is reversed in its rank half. The ranks stand; the pump was always the defect.

**T1, blind, found that ruling 212(c) contradicts `SPEC.md` at T1's own
base commit** — and that the contradiction is mine twice over: I closed a
`[FLAGGED FOR RULING]` block in `rulings.md` and **never re-swept §7.3
with the answer**, so the spec still argued the opposite in three places
(`:2369-2378`, `:2380-2396`, §14.5 `:4700-4703`).

Worse, the spec's argument is better than my ruling's.

**What I ruled (212(c)):** rank `PATH_CHALLENGE`/`PATH_RESPONSE` above the
contested probe, because *"everything else in that order competes for the
budget; the challenge dissolves it."*

**What §7.3 already said:** the path frames are placed **under** the
contested probe *"because the probe's deadline is a **liveness verdict**
that a delay converts into a death, where a delayed challenge only
prolongs a cap."* And §7.5 proves exactly this once already, for the
congestion gate: *"a probe the gate could delay past its own deadline
would silently convert congestion into a liveness verdict."* **The
argument transfers verbatim.**

Working rule 3's tiebreak decides it and decides against me: **follow the
statement some other proof depends on.** §7.5's congestion-gate proof
depends on the probe not being delayed. *Nothing* depends on the challenge
outranking the probe.

**The flag offered three resolutions and I took the weakest.** They were:
coalesce the challenge into the probe's packet; lift the challenge above
the probe; or **hold that the priority order was never an early return and
the pump is simply wrong.** The flag all but names the third as correct —
*"rank 2 outranking rank 4 does not mean rank 4 is never built, only that
it yields when the budget cannot hold both, **and here the budget can**"*:
probe plus challenge is 40 B against the 90 B floor the smallest arming
produces.

**Ruling: the third resolution. The ranks in §7.3 stand exactly as
written — CLOSE, probe, `PATH_RESPONSE`, `PATH_CHALLENGE` — and the send
pump's early return is the defect.** It may emit the probe *and* continue
building, because the budget holds both and always does. This concedes
nothing: the probe keeps the liveness priority §7.5 proves it needs, and
the challenge is still built on the same pass.

212(c)'s **other** half was right and is untouched: the pump's early
return is wrong as written, and fixing it is the implementer's job. That
half is common to my resolution and the correct one, which is precisely
why choosing wrongly between them did not change the code that had to be
written — and is why an editor's flag saying *"an implementation that hits
it should stop and ask"* held the line that my ruling did not.

**The process defect, stated plainly: closing a flag is not sweeping the
spec.** This is the third instance in two days of the same shape —
committing the rulings is not committing the contract (ruling 214(d)),
and ratifying a rule is not reviewing its rationale (ruling 64). In every
case the authoritative artefact was left behind by an update to the record
*about* it. §7.3's flag block is now replaced by this ruling; §14.5 needs
no change, because the ranks it describes were right all along.

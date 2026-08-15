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

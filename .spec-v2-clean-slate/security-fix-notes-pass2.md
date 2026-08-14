# Pass 2 — maintainer-ratified security fixes (rulings 33, 34 + SECV4-7, SECV4-10, FIDV4-6)

Applied 2026/08/14 against `SPEC-v2-DRAFT.md` (draft v4 post-pass-1, 3448
lines at start). Rollback copy:
`.spec-v2-clean-slate/SPEC-DRAFT-v4-post-pass1-frozen.md` (do not edit).

Baseline counts before this pass: 3448 lines, `[MAINTAINER]` × 2,
`[RATIFIED` × 29, "15 s" × 18 (13 to sweep + 5 `INTRO_TTL`), 48 code
fences (balanced).

Work order for the sweep: `.spec-v2-clean-slate/dead-timeout-sweep.md`.

## Progress log (appended as each fix lands)

### 1. §5.7 timers table — DONE (ruling 34 + judgment call 3 + judgment call 2)
- `DEAD_TIMEOUT` row: 15 s → 25 s; derivation note rewritten to
  "2 × `KEEPALIVE_TIMEOUT` + 5 s grace — explicitly one-lost-keepalive
  tolerance".
- Added two paragraphs after the table: (a) states the derivation
  outright (15 s left no room for one retransmit; 25 s = one-lost-keepalive
  tolerance; only two *consecutive* losses kill an idle connection);
  (b) **flags the `PERSISTENT_KEEPALIVE` zero-margin coincidence** — 25 s
  default now sits exactly on the floor (25 ≥ 25). Default UNCHANGED;
  recorded as a maintainer call.
- §5.7 carries no `[RATIFIED …]` marker, so no `amended` stamp was added.

### 2. §6.7 tie-break rationale — DONE (sweep)
"go mutually dark for 15 s" → "25 s". Value only; the replacement/tie-break
logic itself untouched.

### 3. §7.3 — DONE (sweep + FIDV4-6)
- "dies by liveness inside 15 s (no deadlock)" → "25 s".
- FIDV4-6 core-event clause added verbatim from the re-review's minimal
  fix: observability sentence now continues "…; at the core level the
  connection emits `ConnEvent::AddressMoved`, which is what updates
  `remote_address()` and fires the trace (§16.4, §16.8)." This re-attaches
  the orphaned `ConnEvent::AddressMoved` variant to a described consumer.

### 4. §7.4 — DONE (ruling 33 arming + ruling 34 value)
Rewrote the liveness anchor as two independent arming triggers, both
normative: "A send arms the death deadline if **either** it is a marking
send … **or** it carries any ack-eliciting frame, whether or not it marks".
Kept the one-shot property (armed by the *first* arming send after a
receive, never re-armed) so the PTO-train non-deferral argument survives —
its reasoning is restated ("probes now arm the deadline, but no send ever
re-arms it") because the old reasoning ("probes are non-marking") is no
longer the reason. Black-hole sentence re-valued to 25 s and re-worded
("no matter how often, or how quietly, it writes"). Added the rationale
paragraph naming the closed hole (all-quiet-set-but-ack-eliciting output).
Also amended the preceding "independent axes" sentence, which previously
called credit frames "liveness-neutral" — now: they don't touch
`last_send` (never defer a keepalive) but they DO arm the death clock.
Pure ACKs are neither marking nor ack-eliciting → neither defer nor arm.
§7.4 carries no `[RATIFIED …]` marker; no stamp added.

### 5. §7.5 — DONE (sweep + ruling 33 + judgment call 4)
- Constants table: `DEAD_TIMEOUT` 15 s → "25 s (= 2 × `KEEPALIVE_TIMEOUT`
  + 5 s grace — §5.7)".
- Liveness bullet: arming condition now names both triggers; "15 s" → 25 s;
  "lives indefinitely" qualified to "indefinitely *while the dance survives
  the path*" with the residual named outright — one lost keepalive
  tolerated, **two consecutive** still end an idle connection, and
  reconnection is the application's (fresh `connect()`, §16.2).
- Left the `[RATIFIED 2026/08/14]` persistent-keepalive floor bullet
  untouched (judgment: the zero-margin flag lives in §5.7; amending a
  ratified clause for a second copy of the same flag is not warranted).

### 6. §13.3 probe train — DONE (ruling 33)
Added the normative consequence: a probe is ack-eliciting, so the *first*
probe arms the death deadline even when nothing has been marked since the
last receive, and no later probe re-arms it — the train always terminates
within `DEAD_TIMEOUT` of the last authenticated receive. Existing
symmetric/asymmetric-loss sentence kept.

### 7. §7.7 epoch death — DONE (SECV4-10)
Replaced the "≥ 65 536 messages of silence, which `DEAD_TIMEOUT` excludes
by orders of magnitude" justification, which was wrong as reasoning. New
text: the condition is *unopened* messages, needs ≥ 196 608 of them
(`MAX_EPOCH_JUMP` × `REKEY_EPOCH_MSGS` + the epoch in progress), and is
excluded by the ACK-driven admission gate (cwnd → `MINIMUM_WINDOW`, §14.5
stops the sender within ~1 RTT), not by the clock.
**Explicitly verified at the new value and stated:** 25 s *widens* the
window (196 608 seals / 25 s ≈ 7.9 k pkt/s ≈ 75 Mbps at `MAX_DATAGRAM`, a
LAN rate), so the timing argument is named as NOT the load-bearing one.
Liveness is now the unconditional backstop because the drift-producing
traffic is ack-eliciting by construction → the clock arms under ruling 33
regardless of marking. Also fixed the stale `last_recv` →
`last_authenticated_recv`. Conclusion "Implementations must not chase
epochs" unchanged.

### 8. Remaining sweep sites — DONE (ruling 34)
- §15.1 (`[RATIFIED]`, stamped `amended 2026/08/14`): "a clean disconnect
  costs the peer 15 s of liveness wait" → 25 s.
- §15.4 matrix, both rows: "liveness — 15 s without an authenticated fresh
  receive" → 25 s; "liveness, ≤ 15 s" → "≤ 25 s".
- §16.10 paused-clock timer family (the "§16.5" line in the work order is
  now §16.10 after pass 1's renumbering): "5 s/10 s/15 s/25 ms/90 s" →
  **"5 s/10 s/15 s/25 s/25 ms/90 s"** per judgment call 1 — 15 s retained
  because `INTRO_TTL` still needs it.
- §17.1 honesty clause, both occurrences: "reaped by liveness in 15 s" and
  "dies at 15 s liveness" → 25 s. (Clause already carried
  `amended 2026/08/14`; no second stamp needed.)
- Final constants table: `10 s / 15 s` → `10 s / 25 s`.
- §7.3's `[RATIFIED]` anti-amplification budget stamped
  `amended 2026/08/14` (it contains the re-valued "dies by liveness inside
  25 s" clause) and given a ruling-33 rider: output to an unvalidated
  address is ack-eliciting, so the death clock arms unconditionally.
- §5.7's new derivation paragraph was reworded to avoid a bare "15 s"
  literal for the superseded value (it now says "a single keepalive period
  plus grace"), keeping the residual-"15 s" invariant clean.

**NOT touched, per the work order:** lines with `INTRO_TTL` — the §6.3
constants row, the flood-posture sentence, the chain-expiry sentence, the
final constants table row, and critically **`≈ 68 packets/second
(1024 / 15 s)`**, which is arithmetic derived from `INTRO_TTL`.

### 9. §1.1 old-dev-binaries note — DONE (SECV4-7)
Deleted the size/structure-based claim ("distinguished **structurally, not
cryptographically**"), which was the over-reach: the packet sizes
(174 / 81 / 196 / 107) are byte-identical across the two wires, so nothing
structural separates them. Replaced with a scoping clause that states the
mechanism correctly and bounds it by suite:
- P-256 (the only suite the old wire had): mac1 keyed over 33-byte
  compressed SEC1 then vs 65-byte canonical uncompressed now, so mac1 does
  not verify across the wires → silent drop before any DH.
- Any other suite: the old wire has no counterpart, so packets differ in
  length and die at §3.1's classification gate instead.
Conclusion ("no session forms") preserved, plus the §8.2 backstop from the
re-review's minimal fix. Kept the not-overstated framing: superseded
*drafts*, nothing ever released. The note carries no `[RATIFIED …]` marker
of its own, so no stamp added. **No wire constant was changed** — the
lengths are quoted, not altered.

### 10. Preamble revision block — DONE
- Header line: "DRAFT v4, 2026/08/14 (unratified; walkthrough revisions
  applied)" → "DRAFT v5, 2026/08/14 (unratified; walkthrough revisions and
  both security-fix passes applied)".
- Added a "**Security-fix revisions (draft v4 → v5, 2026/08/14)**" block
  after the walkthrough block, covering BOTH passes: pass 1's rulings 31–32
  (replacement basis, `accept()` PENDING branch) and pass 2's rulings 33–34
  plus the three directed fixes (SECV4-7, SECV4-10, FIDV4-6). States the
  passes are wire-free except `DEAD_TIMEOUT`, and repeats the
  `PERSISTENT_KEEPALIVE` zero-margin flag as an open maintainer call.
- Judgment: left the historical phrase "the draft-v4 ratchet-only ruling
  below" (line ~22) as-is — it attributes a ruling to the draft in which it
  was made, it is not a self-reference to the current draft version.

### 11. Ruling-33 consistency sweep — DONE
Places whose text would have contradicted or under-specified the new
arming rule, brought into line (all wire-free):
- **Appendix B, "The liveness anchor" obligation:** now requires the
  deadline to arm on the first *marking or ack-eliciting* send, requires a
  connection whose entire output is quiet-set-but-ack-eliciting to die on
  schedule, and carries judgment call 4's residual (one lost keepalive
  tolerated, two consecutive kill it, no built-in reconnect).
- **§16.5 timer table:** added the `Liveness` arming contract alongside the
  existing `Pto` one — armed by the first marking-or-ack-eliciting send
  after an authenticated fresh receive, never re-armed, re-anchored by
  every such receive. This makes ruling 33 normative in the
  implementation-facing section, matching how `Pto` is already pinned.
- **§7.3 amplification clause:** rider added, carefully scoped — "any
  ack-eliciting output we aim at the address arms the death clock by
  itself, even where nothing marking is sent" (deliberately NOT "all output
  to an unvalidated address is ack-eliciting", which is false: pure ACKs
  and keepalives are not).
- §16.5's `PersistentKeepalive` "re-armed by any marking send" is
  **unchanged and still correct** — ruling 33 changes the death clock's
  arming, not the marking set or the keepalive triggers.

### 12. "liveness-neutral" terminology — DONE (ruling 33 hygiene)
Ruling 33 splits what one word used to cover: the quiet set is
non-marking, but its ack-eliciting members now arm the death clock. Four
existing uses of "liveness-neutral" (§7.5's PING bullet, §10's credit
frames, §13.4, §14.5) would otherwise read as "does not arm".
Fix: added a short terminology paragraph at the end of §7.4 pinning
"liveness-neutral" = **non-marking** (no `last_send` touch, defers no
keepalive), explicitly NOT "does not arm the death clock" — and stating
that every ack-eliciting quiet-set member is liveness-neutral *and*
arming. §7.5's PING bullet also got an inline three-word guard, being the
highest-risk spot (it sits directly under the liveness bullet). The other
three uses now resolve correctly against the definition and were left
alone.

## Final verification (all green)

| Check | Bar | Result |
|---|---|---|
| `[MAINTAINER]` markers | exactly 2 | **2** (§1.3 ~line 215, Appendix A.1 ~3287) |
| `[RATIFIED` count | 29 | **29** |
| `[OPEN]` markers | unchanged from baseline (2, both in the preamble's own "no [OPEN] markers remain" sentence) | **2** |
| Code fences | balanced | **48** (even) |
| Table column counts | unchanged | verified on §5.7, §7.5, §15.4 and the final constants table |
| Wire constants | untouched | `IK_MSG1_LEN` 174, `IK_MSG2_LEN` 81, `INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107, `MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170, `VERSION` 0x01, `PROLOGUE b"slither\x01"` — all present and unchanged |
| Constant-token diff vs frozen | additions only | every `` `CONSTANT` `` token count is equal or higher; none removed or renamed |
| `KEEPALIVE_TIMEOUT` | stays 10 s | **10 s** in all three homes |
| `DEAD_TIMEOUT` | 25 s everywhere | §5.7, §7.5 and the final constants table all read 25 s |
| Lines | — | 3448 → 3591 |

### Residual "15 s" — 7 occurrences, every one accounted for
- **5 are `INTRO_TTL`** and were deliberately not touched: §6.3's constants
  row (720), the flood-posture sentence (729), the chain-expiry sentence
  (770), **the flood arithmetic `≈ 68 packets/second (1024 / 15 s)` (818)**,
  and the final constants table row (3587).
- **1 is §16.10's paused-clock timer family** (3031), which per judgment
  call 1 now reads "5 s/10 s/15 s/25 s/25 ms/90 s" — 15 s retained because
  `INTRO_TTL` still needs to resolve in virtual time.
- **1 is the new draft-v5 revision entry** (94), recording the change
  itself: "`DEAD_TIMEOUT` rises 15 s → 25 s". A revision history that does
  not name the superseded value is useless, so this literal is intended.

**Zero stale live `DEAD_TIMEOUT` references remain.** The brief predicted 5;
the extra two are the directed judgment-call-1 retention and the revision
history, not sweep misses.

## Judgment calls made
1. **§16.10, not §16.5** — the work order and the brief both call the
   paused-clock timer family "§16.5"; after pass 1's renumbering it is
   §16.10 ("Kernel-free drivability"). Edited the right clause by content.
   §16.5 ("Time and timers") was edited separately, for ruling 33.
2. **`PERSISTENT_KEEPALIVE` flagged in one place, not two.** The
   zero-margin note lives in §5.7 next to both values. §7.5's
   `[RATIFIED 2026/08/14]` floor bullet was left untouched rather than
   amended to carry a second copy of the same flag. Default unchanged at
   25 s, as directed.
3. **Preamble "draft-v4 ratchet-only ruling" left as-is** — it attributes a
   ruling to the draft in which it was made; it is not a self-reference to
   the current version. Only the header line was re-versioned.
4. **§5.7's derivation prose avoids the literal "15 s"** for the superseded
   value ("a single keepalive period plus grace") to keep the residual
   check clean; the revision history carries the literal instead.
5. **§7.3's ruling-33 rider is scoped, not blanket** — "any ack-eliciting
   output we aim at the address arms the death clock by itself", NOT "all
   output to an unvalidated address is ack-eliciting" (false: pure ACKs,
   CLOSE and keepalives are in that output and are not ack-eliciting).
6. **"Liveness-neutral" pinned rather than search-and-replaced** — see
   item 12. One definition in §7.4 fixes four downstream uses without
   editing four sections.
7. **§16.5's `Liveness` arming contract added** (beyond the literal brief).
   §16.5 already pins `Pto`'s arming precondition normatively; leaving
   `Liveness` unpinned there while ruling 33 changes it would leave the
   implementation-facing section silent on the change.
8. **Cross-reference correction:** §5.7's floor pointer is `(§7.5, §16.2)`
   — the `set_persistent_keepalive` rejection lives in §16.2's shell
   surface, not §16.4.

## NEEDS A RULING
- **`PERSISTENT_KEEPALIVE` at exactly the liveness floor.** 25 s default vs
  a 25 s `DEAD_TIMEOUT`: `25 ≥ 25` holds with zero margin. Flagged in §5.7
  and in the preamble's v5 entry; **not changed**. Whether to raise the
  default is the maintainer's call.

## Untouched, as directed
§6.4, §6.6, §6.7's replacement logic, §17.4, and all of Appendix A. §6.7
was edited only at its tie-break *rationale* sentence ("mutually dark for
15 s → 25 s"), which the sweep work order lists explicitly; its
replacement logic was not read or altered. Appendix B **was** edited (the
liveness obligation), which pass 1 also did and the brief directs via
judgment call 4.

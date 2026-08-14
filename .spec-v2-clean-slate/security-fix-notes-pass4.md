# Pass 4 — rulings 41/42/43 + directed fixes (2026/08/14)

Baseline at start: 4378 lines, `[RATIFIED` = 29, `[MAINTAINER]` = 0, "15 s" = 7.

## Plan
- R41: §7.5 contested probe → counter high-water mark (`probe_floor`), collapse concurrent marks.
- R42: PERSISTENT_KEEPALIVE floor 1 s (range [1 s, DEAD_TIMEOUT)).
- R43: honest probe bound; count probe in sent map; reconcile §17.5 / §6.3.
- ADV-S-2 §15.4 teardown row; ADV-S-3 §16.5 timer table; ADV-S-4 §6.9+§17.5 cost;
  ADV-S-5 §7.3 stale exemption list; ADV-S-7 banner v6 + Pass 4 revision entry.
- ADV-O-4..O-9 + NOTE: apply the clear wire-free ones.

## Log
(appended as work proceeds)

### Done so far
- R41 §7.5: contested bullet rewritten — probe_floor via `DatagramSend::next_counter()`
  (App A.2), clears on ANY ack covering counter >= floor, deadline armed at
  *transmission*. "One mark per connection" para: second refusal = no new mark,
  no new PING, explicitly NO re-arm (+ reason: attacker supplies Intros, re-arm
  would postpone verdict forever). "Ruling 41" para: old predicate unsatisfiable
  because §8.7 files PING never-retransmit; PTO retries now RESCUE.
- R41 security argument rewritten to floor form + "why it was a security defect"
  (vacuous timestamp guard, Stale reverts the record, ~independent trials).
- R41 declined alternatives recorded: (1) retransmittable PING (would reshape §8.7
  for §13.4's probe trains), (2) clear on any post-mark ACK (in-flight ACK predates
  harvest close).
- R43 §7.5: "honest bound" para replaces the false "nothing an attacker controls";
  bound is <=1 probe / KEEPALIVE_TIMEOUT / connection via the collapse.
  Congestion para: exempt from ADMISSION, counted in sent map (§13.5 already
  inserts ack-eliciting PING — no §13.5 change needed, wire-free).
  ADV-O-10 folded in: budget-deferred probe leaves mark PENDING; contested mark on
  closing/draining = no-op.
- NOTE: did NOT add a new `[RATIFIED` marker (count must stay 29) — added one by
  mistake, removed it.
- R42 §5.7 table row + §5.7 prose (new "ruling 42 restores a floor" para).
- R42 §7.5 constants table + bullet + full "Ruling 42" record; plus ADV-O-4 folded
  in as a *caveat* para ("what the admissible band does and does not promise":
  L=250ms + RTT, no loss tolerance at I >= KEEPALIVE_TIMEOUT) WITHOUT moving the
  ceiling (ruling 42 fixes the range as [1 s, DEAD_TIMEOUT)).

### Remaining edits applied
- R42 cont.: §16.2 prose (range + `None` always accepted), Appendix B liveness-anchor
  bullet, Appendix B "bounded above *and* below" obligation (accepts 1 s inclusive —
  VERIFIED the pre-existing obligation already mandated accepting 1 s, so the floor
  needed no test change, only the added below-floor rejections 999 ms / 1 ms / ZERO),
  consolidated constants table.
- R43 cont.: §14.5's contested bullet gains "exempt from admission only, counted in
  the sent map"; §17.5 table row + new two-paragraph caveat (sent map = cwnd + fixed
  exempt allowance <= 2400 B; contested state O(1)); §6.3's "keep running regardless"
  qualified (a dialled connection IS contested by a parked Intro; <=1 probe/10 s).
- ADV-S-2 / ADV-O-9: §15.4 gains a dedicated **contested** row (10 s, fires while
  authenticated receives arrive, asymmetric peer view, same TimedOut variant) plus a
  "two TimedOut rows, one variant" explanatory para. §18.2's `slither::policy` row
  extended with the mark / transmission / verdict (adding to an existing target, not
  a new one — §18.2 says renaming or dropping is a revision, adding is fine).
- ADV-S-3 / ADV-O-8: §16.5 timer table gains `Contested` (one per connection, armed at
  transmission) and the equal-deadline priority list places it in teardown collection
  after `CloseLinger`, noting Liveness-beats-Contested is harmless (same variant).
- ADV-S-4: §6.9 gains a dedicated cost paragraph; §17.5 as above.
- ADV-S-5: §7.3's exemption parenthetical now names the contested probe (and the
  matching Appendix B anti-amplification bullet, for consistency).
- ADV-S-7: banner DRAFT v5 -> v6 + "no maintainer flag remains open" (A.1 closed by
  hiss 0.3.2); "draft v4 -> v5 / two passes" -> "draft v4 -> v6 / four passes";
  full **Pass 4 — rulings 41–43, plus six directed fixes** revision entry added;
  ruling 36's preamble entry annotated with ruling 41's amendment.
- ADV-O-6: §7.4's unconditional spoofed-address claim qualified; §6.9's "ratio < 1"
  gains a "One qualification" para (3x budget is the operative bound with a beacon).
- ADV-O-7: §5.7 and §7.5 restate the reap as a RECEIVE-within-DEAD_TIMEOUT rule;
  §7.5 adds the send-at-t=24 consequence, the three legitimate flows, and a
  normative application MUST; Appendix B gains a connect-ahead-of-use obligation.
- ADV-O-10 (NOTE): folded into §7.5 — deadline armed at transmission, budget-deferred
  probe leaves the mark PENDING, mark on a closing/draining connection is a no-op;
  also asserted in Appendix B.
- Cross-refs de-staled: §6.4, §6.8, §7.4, §17.4 now say "a counter sealed after" /
  "the probe floor" instead of "the packet that carried that PING".

### Judgment calls
1. ADV-O-4 (ceiling ignores L; [KEEPALIVE_TIMEOUT, DEAD_TIMEOUT) has zero loss
   tolerance). The reviewer proposed moving the ceiling to KEEPALIVE_TIMEOUT. Ruling
   42 fixes the range at [1 s, DEAD_TIMEOUT), so the bound was NOT moved. Applied
   instead as a documented caveat in §7.5 ("What the admissible band does and does
   not promise"): L = 250 ms plus a round trip, single lost beacon fatal at
   I >= KEEPALIVE_TIMEOUT, top of band inert even unlost — admissible, not rejected,
   and the reason the recommendation is 10 s.
2. R43's "count the probe in the sent map" needed NO change to §13.5: it already
   holds "per ack-eliciting counter ... PING ...". Made explicit in §7.5, §14.5,
   §17.5 rather than restated as a new rule. Keeps it wire-free.
3. §15.4 contested row deliberately reuses `ConnectionLost::TimedOut` and the spec
   now argues why one variant is right (no new error variant, per the constraint).
4. §18.2: extended `slither::policy` rather than minting a new target.
5. ADV-O-6's optional "suppress the beacon while the address is unvalidated" is a
   behavioural change; recorded in §6.9 as **left open**, not adopted.

### NEEDS A RULING (not guessed, text left as-is)
- **ADV-O-5** — `set_persistent_keepalive(&self, interval: Option<Duration>)` returns
  `()`, but §7.5/§16.2/Appendix B all say it **rejects** out-of-range intervals, and
  ruling 42 makes that rejection carry a second bound. The spec still does not say
  whether rejection panics, clamps, or is silently ignored. Two conformant
  implementations can differ between "panic" and "no beacon", and a silent-ignore
  implementation leaves an application believing it holds a NAT binding it does not.
  Choosing between `Result<(), ConfigError>` (a new public error type) and a
  documented panic is a public-API decision, so it was NOT guessed. The signature at
  §16.2 and the wording at §7.5 / §16.2 / Appendix B are unchanged in this respect.
- **ADV-O-6's optional half** — whether the persistent beacon should be suppressed
  while the session address is unvalidated (would restore §6.9's ratio < 1 outright).
  Recorded in §6.9 as left open pending a ruling; the 3x budget bounds it meanwhile.

### Final verification
- `[MAINTAINER]` occurrences: **0**
- `[RATIFIED` count: **29** (a new marker was added in error mid-pass and removed;
  no clause gained or lost a marker)
- `[OPEN]`: 2, both prose references in the banner — same as baseline
- Wire constants: IK_MSG1_LEN 174, IK_MSG2_LEN 81, INIT_PACKET_LEN 196,
  RESP_PACKET_LEN 107, MAX_DATAGRAM 1200, MAX_PLAINTEXT 1170, VERSION 0x01,
  PROLOGUE b"slither\x01", DEAD_TIMEOUT 25 s, KEEPALIVE_TIMEOUT 10 s — all unmoved
- "15 s" occurrences: **7**, untouched (incl. §6.9's `1024 / 15 s`)
- Code fences: 50 (even, balanced) — same as baseline
- Do-not-touch verified byte-identical to the frozen copy: Appendix A, §6.6, §6.7,
  §17.1, §18.1
- Length: 4378 -> 4832 lines

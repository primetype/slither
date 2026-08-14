# Re-review — Clusters B, D, E, F, G, H (SPEC-DRAFT-v2.md)

> Focused re-reviewer, 2026/08/14. Scope: B (flow-control-as-memory-bound),
> D (exactly-once), E (recovery/CC), F (liveness), G (sans-io/shell),
> H (frame-parse + [OPEN]s). Clusters A and C are the other re-reviewer's.
> Method: attack the new mechanisms fresh, not merely confirm transcription.

---

## Per-finding disposition (original findings in scope)

### Cluster B — flow control as the memory bound

- **FID-4 / IMP-5 / SEC-9 — CLOSED.** §10.3 "Retirement advances connection
  credit" credits `final_size` for every retirement cause: read-to-final,
  reset observed, handle abandoned (§16.2), sugar-surfaced (§9.8), final size
  with no reader (§9.7). Traced all four deadlock cases from the task —
  (a) fully-read, (b) abandoned-unread, (c) reset, (d) reader-less half —
  **credit advances in every one**; MAX_DATA cannot wedge after 1 MiB of
  discarded/reset bytes. §16.2 drop-semantics and §9.6/§9.7 agree.
- **FID-8 — CLOSED.** §10.3: prospective limit = `bytes_read + WINDOW`; emit
  when `prospective_limit − last_advertised ≥ WINDOW/2`. The unsatisfiable v1
  clause is gone; matches RFC 9000 §4.2.
- **FID-9 / SEC-2 — CLOSED.** §10.6 "The second bound — reassembly fragments":
  per-stream reassembly MUST be O(advertised credit), `REASSEMBLY_CHUNKS_MAX`
  = 1024, breach ⇒ CLOSE(PROTOCOL_VIOLATION); span+bitmap alt makes the ceiling
  unreachable. §9.5 cross-refs it; §17.5 worst-case row rewritten so the credit
  term dominates (the 25–50× underestimate is named and fixed). SEC-2(b)
  ("consumption" + no unbounded intermediate queue + reliable-data-not-droppable)
  fully specified in §10.6/§16.4/§16.8. Metadata is O(MAX_STREAMS ×
  chunk-ceiling) + O(credit) — bounded, not attacker-explodable.
- **SEC-8 — CLOSED.** §8.4 RESET_STREAM and STREAM both list `FLOW_CONTROL_ERROR`,
  **checked before** the §9.6/§10.3 true-up, checked/saturating u64 mandated.
- **FID-14 — CLOSED.** §10.1 now "the sum over all streams of the highest
  received offset (the final size, once pinned)."
- **FID-15 — CLOSED.** §10.4 grants +1 only on fully-closing a **peer-opened**
  stream of the space.
- **FID-16 — CLOSED (with a new reliability gap, RBH-1).** §9.8 overflow policy
  resets a FIN-less window-filling uni stream under a pending `recv_message()`;
  §9.6/§8.4 carry the one receiver-emitted RESET_STREAM. See RBH-1.
- **IMP-20 — CLOSED.** §8.4 MAX_STREAM_DATA reworded to QUIC's
  `STREAM_STATE_ERROR` rule for a not-yet-opened receiver-side stream.

### Cluster D — exactly-once

- **FID-3 — CLOSED.** §9.2 closed-stream watermark: **per-space** (one index
  in each of the four spaces), monotone, O(1) — *not* an attacker-inflatable
  set. A STREAM/RESET_STREAM at-or-below-watermark-and-not-open is an ACKed
  no-op, never re-opened (§8.4 STREAM and RESET_STREAM rows both carry it);
  implicit opening applies only *above* the watermark, so a legitimately-later
  ID still opens. The "≤ watermark ⇒ necessarily closed" claim checks out
  (opening the watermark stream implicitly opened everything below it).
  Survives rekey (§7.8, confirmed line 1255). Post-loss retransmit of a freed
  stream is now inert — no re-open, no duplicate delivery, no phantom
  StreamOpened. Appendix B pins it.

### Cluster E — recovery / CC

- **FID-5 — CLOSED.** §13.3/§16.5: `Pto` armed only while ≥1 ack-eliciting
  packet is in the sent map; disarmed on empty; `Loss` takes precedence. Still
  fires for a black-holed path with data outstanding (data ⇒ non-empty map ⇒
  armed). The idle PING train is gone.
- **FID-7 / IMP-17 — CLOSED.** §14.4: `persistent_period` evaluates the §13.3
  PTO with `pto_count = 0` (RFC 9002 §7.6.1), backoff excluded. Bonus: a
  "prior RTT sample exists" precondition avoids the `K_INITIAL_RTT` phase.
- **FID-12 — CLOSED.** §14.3: ACKs of packets sent before the recovery period
  do not grow cwnd — same `sent_time ≤ recovery_start` test gates event and
  growth (RFC 9002 §7.3.2).

### Cluster F — liveness

- **SEC-6 — CLOSED.** §7.4: death when `now − last_authenticated_recv >
  DEAD_TIMEOUT` **and** ≥1 marking send since that receive; arms once per
  receive-gap, not re-armed by later sends, reset by every authenticated
  receive. A continuous black-hole sender now dies 15 s after last receive
  (was immortal). Healthy busy session not killed (receives keep resetting the
  clock). Config floor: §7.5/§16.2 reject `PERSISTENT_KEEPALIVE < DEAD_TIMEOUT`.
  The passive keepalive ("received since last sent") self-limits in a partition,
  so no marking beacon defeats the death clock.
- **SEC-11 / FID-18 — CLOSED.** §7.5/§7.6: the keepalive is a marking send that
  consults `REKEY_AGE`; an idle-but-live tunnel re-handshakes every ~120 s.
- **IMP-12 — CLOSED.** §7.5 states WireGuard's persistent-keepalive trigger
  (fire if no marking send for the interval; re-arm from every marking send).

### Cluster G — sans-io / shell

- **IMP-1 / SEC-7 — CLOSED.** §15.2 retained set = seal capability + receive
  cipher states + replay window; reply owed only to an authenticated,
  window-fresh inbound, sent to the session address (never the triggering
  source, never a bare `receiver_index` match). The v1 "retain only seal yet
  must reply" contradiction is gone; §6.9 no-amplification invariant preserved.
- **IMP-2 — CLOSED.** §16.9: stream IDs assigned at establishment;
  pre-establishment handles hold core-internal indices; `id() -> Option`,
  `None` until established (§16.2). No frame precedes install, so on-wire IDs
  are race-independent.
- **IMP-3 — CLOSED.** §16.4 uniform pull model: core retains
  reassembled-but-unclaimed uni streams, queued datagrams, and peer-opened
  streams; receive-side `ConnEvent`s are signals; claim verbs `accept(dir)` /
  `recv_message()` / `recv_datagram()`. The 64-datagram queue + drop counter
  are core state (§11.3) — ownership unambiguous.
- **IMP-4 — CLOSED.** §16.4/§10.4: `StreamOpened{dir}` claimed via `accept(dir)`;
  `StreamsAvailable{dir}` on MAX_STREAMS receipt wakes blocked openers;
  `StreamWritable{id}` fires on credit arrival for a blocked writer. Every
  blocking shell verb now has a wake edge.
- **IMP-11 — CLOSED.** §16.2 `finish()`/`close()` await points stated.
- **IMP-15 — CLOSED.** §16.4 gives the staged-verb signatures and an
  `EstablishedSession` shape.
- **IMP-16 — CLOSED.** §16.4/§7.6: `NeedsRekey` re-emits per consulting seal,
  endpoint dedups against one-pending-per-static (no core latch).
- **IMP-13 — CLOSED.** §16.5 endpoint deadline includes timestamp-guard orphan
  aging.
- **IMP-18 — CLOSED.** §16.5 places `AckDelay` and `PersistentKeepalive` in the
  equal-deadline priority list.
- **SEC-12 — CLOSED.** §16.6: indices MUST be unpredictable off-path; the
  config seed is a test-only, security-relevant, feature-gate-or-document
  facility.

### Cluster H — frame-parse + [OPEN]s

- **FID-10 — CLOSED, and correctly scoped.** §8.2: post-AEAD structural failure
  ⇒ CLOSE(PROTOCOL_VIOLATION 0x01) then linger. §3.1 keeps the silent drop
  **only** pre-AEAD (unknown type/version/length gate). §4.2/§4.3 + §15.1
  confirm nothing unauthenticated emits — an off-path mac1-valid packet fails
  the AEAD and is silently dropped, never triggering CLOSE. **No
  unauthenticated-packet CLOSE-amplification was introduced.** The
  once-then-linger + ≤1/s + authenticated-only reply rule bounds the emission.
- **FID-17 — CLOSED.** `ConnectionLost::ProtocolViolation { code }` defined in
  §18.1 and referenced consistently in §8.2, §15.2, the §15.4 teardown row, and
  Appendix B. No teardown-matrix row references an undefined variant.
- **FID-24 — CLOSED.** §15.2: a CLOSE received while closing moves to reply-free
  draining — the 1 Hz ping-pong is killed.
- **FID-19 / FID-20 — CLOSED.** §12.5 and §13.4 label their RFC divergences
  deliberate.
- **FID-22 — CLOSED.** §12.4 defines the first-packet-of-session case (immediate
  ACK, vacuous gap rule).
- **FID-23 — CLOSED.** §8.4/§16.2: `close()` truncates `reason` at
  `CLOSE_REASON_MAX` at the handle.
- **FID-21 — CLOSED.** §11.3 ≈73 KiB/queue; §17.5 ≈146 KiB for both; constants
  table agrees.
- **SEC-13 (H half) — CLOSED.** §15.1 "only the authenticated, in-seal CLOSE
  exists." (The §6.9 "No amplification" restatement belongs to Cluster C's
  §6.9 and is out of my read scope.)

---

## New defects

### RBH-1 — MINOR — §9.6 / §9.8 / §8.7 — the receiver-emitted overflow RESET_STREAM is not reliably regenerated, so a single loss re-strands the FID-16 sender

**Section:** §9.6 (receiver-emitted reset), §9.8 (overflow policy), §8.7
(regenerate class), §9.7 (retirement).

The FID-16 fix has the receiver, on a FIN-less window-filling uni stream under
a pending `recv_message()`, emit RESET_STREAM(code 0) **and retire the receive
half immediately** (§9.8). But both §8.7 and §9.6 terminate RESET_STREAM
regeneration "until acknowledged **or the stream state is discarded**." For the
receiver-emitted reset the stream state is discarded *at the moment of emission*,
so under the literal rule the reset is sent once and never regenerated. If that
one packet is lost:

- the sender keeps its send half open and PTO-retransmits its STREAM frames;
- the receiver no-ops-and-ACKs them (below the new watermark, §9.2), so the
  sender sees ACKs and its liveness clock keeps resetting — liveness never
  fires;
- but the sender is blocked at the stream window (message-consumed streams
  never earn MAX_STREAM_DATA, §10.3/§9.8), so its send half wedges — the exact
  stall the reset exists to cure — until the application abandons the handle.

This bites only on the documented open_uni-vs-recv_message mode-mismatch path
and only under loss, hence MINOR, but it defeats the remedy's purpose.
**Fix:** state that the receiver-emitted overflow RESET_STREAM is retained in
the connection's regenerate/pending set until acknowledged, independent of the
retired receive half — i.e. §8.7/§9.6's "or the stream state is discarded"
termination does not apply to it. **Wire impact: none** (RESET_STREAM is already
regenerate-class; only its retention lifetime changes). Appendix B's overflow
test should add a lost-reset case.

### RBH-2 — NIT — §10.3 — the retirement true-up should be pinned as absolute (bring-to-final_size), not additive, to avoid a read-then-retire over-commit

**Section:** §10.3 (retirement advances connection credit), §10.1.

§10.3 says a retired half's "bytes up to its final size count as consumed …
exactly as if the application had read them." §10.1 (FID-14) defines connection
consumption as an absolute per-stream sum, which is correct. But an implementer
reading §10.3 as an incremental `conn_consumed += final_size` at retirement
double-counts a **read-to-final** stream (its bytes were already counted by
reads), over-advancing the receiver's own MAX_DATA past its buffer commitment —
a memory over-commit (note: self-inflicted over-advertisement, **not** a
peer-manufacturable credit inflation; the FLOW_CONTROL_ERROR bound check and
checked arithmetic still hold). **Fix:** one clause — the true-up brings the
stream's connection-consumed contribution *to* `final_size` (idempotent with
prior reads), it does not add `final_size` again. **Wire impact: none.**
Low-confidence: a careful implementer already infers this from §10.1's
absolute-sum definition; worth one sentence given the counter is memory-safety
load-bearing.

---

## Cross-section consistency (introduced by the edits)

- Constants table: `REASSEMBLY_CHUNKS_MAX` (1024, home §10.6), `AMPLIFICATION_FACTOR`
  (3, home §7.3), `PERSISTENT_KEEPALIVE` floor (rejected below DEAD_TIMEOUT),
  `MESSAGE_RECV_MAX` (= INITIAL_MAX_STREAM_DATA) all present with homes; inline
  uses agree (§10.6, §14.5, §7.5/§16.2, §9.8).
- Every error variant named in a teardown path exists in §18.1; the §15.4 matrix
  has no dangling variant. `ProtocolViolation{code}` referenced in every row/section
  that reaches it. `Superseded` appears only in §6.3 and §18.1's exclusion note.
- Appendix B gained an obligation for each new mechanism: freeze-on-carry,
  3-state continuation matrix, closed-stream tombstone, discard-credit,
  reassembly-fragment bound, overflow reset, PTO-disarm, no-growth-in-recovery,
  persistent-congestion(pto_count=0), liveness anchor, idle-rekey,
  anti-amplification budget, authenticated-only linger reply, mutual-close
  draining, ProtocolViolation surfaced, pre-AEAD-stays-silent.
- 73/146 KiB memory-accounting: §11.3 (≈73 KiB/queue), §17.5 (≈146 KiB both),
  constants table — internally consistent.
- No dangling section cross-references found in the in-scope sections.

## Residual sweep

Every MINOR/NIT the resolution claimed to sweep within B/D/E/F/G/H
(SEC-8/9/12/13-H, FID-12/14/15/16/19/20/21/22/23/24, IMP-11/12/13/15/16/18/20)
was verified **present** in v2. None left unswept.

## Clean clusters

**D, E, F, G, H — clean** (no new defects; every in-scope finding CLOSED, all
adversarial traps pass). **B — clean on the forced fixes** (deadlock-free credit
advance in all four cases; not a credit-inflation vector); RBH-1 (MINOR) and
RBH-2 (NIT) are narrow gaps on the FID-16 overflow-reset path and the true-up
wording, respectively.

---

## Verdict

**B/D/E/F/G/H are sound; no further full revision round is needed for this
scope.** The Cluster B credit fix is deadlock-free (credit provably advances for
fully-read, abandoned-unread, reset, and reader-less halves) and is **not** a
credit-inflation vector (the true-up is bounded by a pre-true-up
FLOW_CONTROL_ERROR check with checked/saturating u64, releasing only credit the
asserted bytes already consumed). The second reassembly bound genuinely caps
metadata at O(credit) without false-killing legitimate high-reorder flows
(coalesce-on-insert keeps legitimate range counts well under 1024). Cluster D's
tombstone is per-space and O(1). Cluster H signals CLOSE only post-AEAD and
introduces **no** unauthenticated-packet CLOSE-amplification (pre-AEAD stays
silent-drop per §3.1). The two new defects are MINOR/NIT, wire-free, and
confined to the single FID-16 overflow-reset path plus one clarifying sentence
on the credit true-up — sweepable in a light editorial pass, not a re-review
gate.

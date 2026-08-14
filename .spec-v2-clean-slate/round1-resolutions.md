# Round-1 resolutions — SPEC-DRAFT-v1.md → SPEC-DRAFT-v2.md

> Resolution architect, 2026/08/13. Consolidates the three round-1 reviews
> (`review-security.md` SEC-1..13, `review-fidelity.md` FID-1..25,
> `review-implementability.md` IMP-1..20 — 58 findings total) into one
> authoritative per-finding resolution. This document is the reviser's
> instruction set; it does **not** rewrite the spec. Design authority is
> `architecture.md`; every resolution below serves an architecture decision
> rather than silently reverting one, and cites the borrowed mechanism where a
> fix leans on one.
>
> **Headline wire verdict: no fix moves a wire byte, a header layout, a frame
> type, a frame encoding, or a ratified constant value.** The one candidate the
> triage flagged — FID-10's error code — resolves to *reuse* the existing
> `PROTOCOL_VIOLATION = 0x01` (already in the §15.3 registry), so even the
> registry does not grow. All additive surfaces are enum-level
> (`ConnectionLost::ProtocolViolation`) or new non-wire constants
> (`AMPLIFICATION_FACTOR`, a reassembly-fragment ceiling). §1.3's one-time
> golden-vector regeneration is unaffected.

---

## 1. Disposition table (all 58 findings)

Cluster key: A CONTINUATION/tie-break · B flow-control-as-memory-bound ·
C roaming+send-engine · D streams-exactly-once · E recovery/CC · F liveness ·
G sans-io/shell · H frame-parse + [OPEN]s · R residual (swept in §3).
Wire: ∅ = no wire byte / constant / layout / frame / code moves. `+const` =
new non-wire constant. `+enum` = new local enum variant. `[M]` = carries a
NEW [MAINTAINER] confirmation flag (consolidated in §5).

| ID | Sev | Cluster | One-line resolution | Wire | [M] |
|---|---|---|---|---|---|
| SEC-1 | BLOCKER | C | Anti-amplification budget (3× received bytes on an unvalidated address) binding **all** output incl. cwnd-exempt classes; msg1-source anchoring gated the same way. | ∅ +const | ✔ |
| FID-1 | BLOCKER | A | §6.6 step 6 computes `CONTINUED = (a live connection with retained transport state was swapped)`, never hardcodes 1; tie-break loser (`Install{initial:true}`) answers 0. | ∅ | ✔ |
| FID-2 | BLOCKER | A | Flag routing (§6.6 step 3) gated on local-state = LIVE; the outbound-pending-only state skips it and is resolved by the tie-break (step 5). | ∅ | ✔ |
| FID-3 | BLOCKER | D | Per-space closed-stream watermark + open set; a STREAM/RESET_STREAM naming an at-or-below-watermark, not-currently-open index is an ACKed no-op, never re-opened. | ∅ | – |
| FID-4 | BLOCKER | B | A receive half retired for any reason counts all bytes up to its final size as **consumed** for connection-level credit-advance. | ∅ | – |
| SEC-2 | MAJOR | B | Second bound: reassembly state MUST be O(advertised credit) (span-buffer + received-bitmap) and/or a per-stream chunk ceiling whose breach is `PROTOCOL_VIOLATION`; and "consumption" = handle takes bytes out of the core, no unbounded intermediate queue, reliable data never droppable. | ∅ +const | ✔ |
| SEC-3 | MAJOR | A | Any stage-0 entry carrying a paid mid-state (eager-demoted or restart-parked) is **consumed** for supersession — bytes+IntroId frozen; a later same-source initiation parks as a new entry. | ∅ | ✔ |
| SEC-4 | MAJOR | A | §6.6 step 3 switches on three local states (LIVE / PENDING-only / NONE), not two; PENDING-only falls through to the tie-break. | ∅ | ✔ |
| SEC-5 | MAJOR | A | Defer `Replaced` teardown to `accept()` (the install act); a CONTINUATION=0-at-LIVE initiation parks as a fresh restart-tagged Intro, the live connection runs until accept. | ∅ | ✔ |
| SEC-6 | MAJOR | F | Liveness anchor = arm once on the first marking send after a receive, do **not** re-arm on later marking sends; death at `now − last_authenticated_recv > DEAD_TIMEOUT`. | ∅ | ✔ |
| FID-5 | MAJOR | E | `Pto` timer armed only while ≥1 ack-eliciting packet is in the sent map (RFC 9002 A.8). | ∅ | – |
| FID-6 | MAJOR | C | On roam, set the recovery marker to the roam instant (not cleared) and fence pre-roam packets from congestion events, persistent-congestion walk, RTT sampling, and app_limited growth; they still resolve for loss/retransmit. | ∅ | – |
| FID-7 | MAJOR | E | `persistent_period` uses the §13.3 PTO evaluated at `pto_count = 0` (RFC 9002 §7.6.1). | ∅ | – |
| FID-8 | MAJOR | B | Re-grant rule: prospective limit = `bytes_read + WINDOW`; emit MAX_STREAM_DATA/MAX_DATA when `prospective_limit − last_advertised ≥ WINDOW/2`. | ∅ | – |
| FID-9 | MAJOR | B | (= SEC-2 sub-a) coalesce/defragment rule + per-stream chunk ceiling; breach `PROTOCOL_VIOLATION` (0x01 suffices). | ∅ +const | ✔ |
| FID-10 | MAJOR | H | Post-AEAD structural frame failure → signalled death: CLOSE with `PROTOCOL_VIOLATION` (existing 0x01), then linger. Silent drop kept only pre-AEAD (§3.1 gate). | ∅ | ✔ |
| FID-11 | MAJOR | A/H | Delete §6.4's candidate-flag discard clause; admit a re-home candidate on the other three conditions, answer CONTINUED=0; add an AlreadyConnected-at-accept guard for §16.1. | ∅ | ✔ |
| IMP-1 | MAJOR | G | (= SEC-7) closing state retains seal + receive-cipher + replay window; replies only to an authenticated, window-fresh inbound, sent to the session address. | ∅ | – |
| IMP-2 | MAJOR | G | Stream IDs assigned at establishment; pre-establishment handles hold core-internal indices; `id()` defined (lazy post-install / blocks). | ∅ | – |
| IMP-3 | MAJOR | G | Uniform pull model: reassembled-unclaimed uni streams and received datagrams retained in the core; `ConnEvent`s become readable/opened **signals**; core gains claim verbs; the 64-datagram bound + drop counter live in the core. | ∅ | – |
| IMP-4 | MAJOR | G | `StreamOpened` carries a claim path (id or `accept(dir)->Option<StreamId>`); add `StreamsAvailable{dir}` on MAX_STREAMS receipt; state when `StreamWritable` fires. | ∅ | – |
| IMP-5 | MAJOR | B | (= FID-4) unread bytes of a retired receive half count as consumed at the connection level; stream-level credit simply never re-granted. | ∅ | – |
| IMP-6 | MAJOR | A | (= SEC-3) subsumed by the freeze-on-carry rule; alternative "drop mid-state on replacement, re-pay 1 DH" stated as the fallback. | ∅ | – |
| SEC-7 | MINOR | G | (= IMP-1) closing-state reply is authenticated-only; resolves the §6.9 "no bytes to an unauthenticated address" contradiction. | ∅ | – |
| SEC-8 | MINOR | B | RESET_STREAM/STREAM: a `final_size`/`offset+length` pushing consumption above the advertised limit is `FLOW_CONTROL_ERROR`, checked **before** the true-up; checked/saturating arithmetic mandated. | ∅ | – |
| SEC-9 | MINOR | B | (= FID-4) a reset stream's `final_size` counts as consumed for connection re-advertisement when the receive half is freed. | ∅ | – |
| SEC-10 | MINOR | R/C | Documentation: state honestly that eager `es` rate is ungated per spoofed hint-set source and that re-home candidates are attacker-fillable; optional per-source es budget deferred. | ∅ | – |
| SEC-11 | MINOR | F | (= FID-18) let the keepalive count as a `REKEY_AGE` trigger so an idle-but-live session re-handshakes every 120 s. | ∅ | ✔ |
| FID-12 | MINOR | E | Add "no window growth during recovery" — ignore ACKs of pre-recovery-period packets for cwnd growth (RFC 9002 §7.3.2). | ∅ | – |
| FID-13 | MINOR | C | Correct the §14.6 citation (RFC 9000 §9.4 resets RTT too); state that keeping RTT is a slither choice and pair it with FID-6 + the min_rtt-may-rise rider. | ∅ | – |
| FID-14 | MINOR | B | Rename §10.1 "final consumed offsets" → "the highest received offset (the final size, once pinned)". | ∅ | – |
| FID-15 | MINOR | R | §10.4 grant scoped to fully-closing a **peer-opened** stream of that space. | ∅ | – |
| FID-16 | MINOR | B/D | Message-mode: reset a message-mode stream that exceeds `MESSAGE_RECV_MAX` (RESET_STREAM already exists); or keep stream credit until the bound. | ∅ | ✔ |
| FID-17 | MINOR | H | (= §15.2 [OPEN]) add `ConnectionLost::ProtocolViolation { code }`. | +enum | ✔ |
| FID-18 | MINOR | F | (= SEC-11) keepalive consults `REKEY_AGE`; state the keepalive-is-not-payload question explicitly. | ∅ | ✔ |
| FID-19 | NIT | R | Label the §12.5 above-highest-sealed-ACK divergence from RFC 9000 §13.1 as deliberate. | ∅ | – |
| FID-20 | NIT | R | Label §13.4 probe-content divergence (new-before-old, one packet) as deliberate. | ∅ | – |
| FID-21 | NIT | R | Fix KiB/kB rounding (73 KiB / 146 KiB, or "≈75 kB / ≈150 kB"). | ∅ | – |
| FID-22 | NIT | R | Define first-packet-of-session out-of-order (no previous greatest ⇒ immediate ACK). | ∅ | – |
| FID-23 | NIT | R | Specify `close()` reason truncation at `CLOSE_REASON_MAX` at the handle. | ∅ | – |
| FID-24 | NIT | H | A CLOSE received while closing moves to the reply-free draining behaviour (kills the 1 Hz ping-pong). | ∅ | – |
| FID-25 | NIT | R | Unify the `Endpoint` type parameterisation across §2.2/§16.2/§16.4; use named size constants in §2.3. | ∅ | – |
| IMP-7 | MINOR | R | Bless the counter mirror-and-assert interim as conformant until A.2 ships. | ∅ | – |
| IMP-8 | MINOR | R | Note the `where C::PublicKey: AsRef<[u8]>+Ord` interim; critical path is A.1 alone. | ∅ | – |
| IMP-9 | MINOR | R | Shape the `Mid` surface in Appendix A.1 (exposes claimed static; `complete()` returns the 13-byte payload). | ∅ | – |
| IMP-10 | MINOR | R | Name a stream fill scheduler (round-robin over streams with pending data) or declare it implementation-defined. | ∅ | – |
| IMP-11 | MINOR | R | State `finish()`/`close()` await points (FIN accepted; CLOSE sealed + closing entered). | ∅ | – |
| IMP-12 | MINOR | F | Adopt WireGuard's persistent-keepalive trigger explicitly (fire if no marking send for the interval; re-arm from every marking send). | ∅ | – |
| IMP-13 | MINOR | R | Add orphan-aging to the §16.5 endpoint deadline enumeration. | ∅ | – |
| IMP-14 | MINOR | R/C | Pin the `app_limited` mechanism (quinn's per-sent-packet flag) and add the hostile-peer reasoning paragraph the security lane asked for. | ∅ | – |
| IMP-15 | MINOR | R | State the core staged-verb signatures and `EstablishedSession` shape. | ∅ | – |
| IMP-16 | NIT | R | Say whether `NeedsRekey` re-emits per seal (endpoint dedups) or once (core latch). | ∅ | – |
| IMP-17 | NIT | E | (= FID-7) pin the base-PTO (pto_count=0) reading for persistent congestion. | ∅ | – |
| IMP-18 | NIT | R | Complete the §16.5 equal-deadline priority list (`AckDelay`, `PersistentKeepalive`). | ∅ | – |
| IMP-19 | NIT | R | Name the Appendix B throughput target (a rate, or "within X% of quinn defaults on the same topology"). | ∅ | – |
| IMP-20 | NIT | R | Reword the MAX_STREAM_DATA "not implicitly openable" clause to QUIC's STREAM_STATE_ERROR rule. | ∅ | – |
| SEC-12 | NIT | R | Add a normative line: indices MUST be unpredictable off-path; the config seed is test-only/security-relevant. | ∅ | – |
| SEC-13 | NIT | H/R | Add the initiator-forged-msg2 (2-DH) row to §6.9; restate "No amplification" as "no path emits bytes **to an address** that has not authenticated" (SEC-1). | ∅ | – |

**Totals: 58 findings — 58 resolved at the design level, 0 rejected.** Two
*optional stronger mechanisms* are deferred within resolved findings (SEC-1's
PATH_CHALLENGE wire alternative; SEC-10's per-source `es` budget); neither
blocks its finding's resolution. Seven NEW [MAINTAINER] confirmation flags
(§5). No finding is left blocked on a ruling — the one that most *needs* the
maintainer's confirmation is the Cluster A redesign (architecture D.3).

---

## 2. Cluster resolutions

### Cluster A — CONTINUATION/CONTINUED restart-vs-rekey machinery (§5.4, §6.3–6.7) — **[MAINTAINER] (confirms/extends architecture D.3)**

Members: FID-1, FID-2, FID-11, SEC-3, SEC-4, SEC-5, IMP-6. This is the
highest-risk cluster; it gets one coherent state-machine redesign, not seven
patches. The seven findings are all symptoms of the same root: the new
CONTINUATION machinery was bolted onto §6.6's numbered order without walking
its joins with (i) the three-valued local-state space, (ii) the tie-break
ordering, (iii) teardown timing, and (iv) parked-entry ownership.

**A.0 The local-state axis is three-valued.** For a proven static, local state
is exactly one of:
- **LIVE** — an established connection with retained transport state exists.
- **PENDING** — an in-flight outbound pending exists, no established connection
  (the simultaneous-open state; §16.1 forbids both at once).
- **NONE** — neither.

§5.4's responder table today has only two columns (LIVE / NONE) and is the
origin of FID-1, FID-2, SEC-4. Rewrite it over this three-valued axis:

| Received | Local state | Action | msg2 CONTINUED |
|---|---|---|---|
| CONTINUATION = 1 | LIVE | internal continuation: silent swap, transport state survives (§7.8) | **1** |
| CONTINUATION = 0 | LIVE | restart: park a fresh restart-tagged `Intro` (consumed/byte-frozen, A.4); the live connection keeps running; `accept()` performs the `Replaced` teardown (A.3) | **0** (from accept) |
| CONTINUATION = 1 \| 0 | PENDING | the tie-break (§6.7) decides; **no teardown**, restart branch skipped | **0** (loser admits `Install{initial:true}`) |
| CONTINUATION = 1 | NONE | we restarted / never had it: staged fresh accept; msg2 tells the peer the truth | **0** |
| CONTINUATION = 0 | NONE | ordinary fresh accept | **0** |

**A.1 §6.5 routing is unchanged** (LIVE and PENDING both enter the internal
continuation via "claimed ∈ known statics = established connections ∪ pending
outbound remotes"; NONE is demoted to the stage-0 queue). PENDING **must**
enter the continuation — that is where the tie-break runs. Do *not* remove
pending-outbound-remotes from the known-static set (SEC-4's "reconcile" is
satisfied by the fact that NONE already cannot enter).

**A.2 §6.6 step 3 (flag routing) fires only when local state = LIVE** (fixes
FID-2, SEC-4):
- **LIVE + CONTINUATION = 1** → proceed (rekey/replacement swap; step-6 admit
  is `Install{initial:false}`, transport survives, CONTINUED = 1).
- **LIVE + CONTINUATION = 0** → restart: **do not tear down here** (A.3). Abort
  the continuation; park the initiation per A.4.
- **PENDING (either flag)** → **skip step 3 entirely**; proceed to pacing
  (step 4) and the tie-break (step 5). The flag value does not gate anything
  in this state.
- **NONE** → not reachable in the continuation (routed to the staged path by
  §6.5).

**A.3 §6.6 step 6 computes CONTINUED; the `Replaced` teardown is deferred to
`accept()`** (fixes FID-1, SEC-5):
- `CONTINUED = 1` **iff** this admission swapped into a live connection whose
  transport state is retained (the LIVE + CONTINUATION = 1 rekey path,
  `Install{initial:false}`). Every other admission — tie-break loser
  (`Install{initial:true}`), restart-replacement, fresh accept — emits
  `CONTINUED = 0`. **Never hardcode 1.** This is FID-1's fix and makes ordinary
  simultaneous open complete: the winner sent CONTINUATION = 0, so the loser's
  msg2 must be CONTINUED = 0 or the winner discards it as "an answer no honest
  responder can produce" (§5.4 initiator rule).
- The `ConnectionLost::Replaced` teardown of a LIVE connection on a
  CONTINUATION = 0 restart is **not** fired at step 3. Instead: park the
  initiation as a fresh `Intro` carrying its post-`ss` mid-state, **tagged
  restart-replacement**, and leave the live connection running. The teardown
  executes **at `accept()`**, as the act that installs the replacement — which
  is also where §16.1's one-connection-per-static is enforced. A withheld/
  injected replay then costs at most one unaccepted `Intro` and nothing else
  (this is SEC-5's on-path withhold-one-retransmit attack, defeated); a
  genuinely restarted peer's zombie dies at `accept()` or at liveness (15 s)
  when its from-zero session stops feeding the old keys. Update §17.1's honesty
  clause: the observable consequence of a re-admissible genuine initiation is a
  spurious *unaccepted* `Intro`, never the destruction of a live connection.

**A.4 Mid-state-carrying entries are consumed for supersession** (fixes SEC-3,
IMP-6). Any stage-0 entry that carries a paid mid-state — eager-demoted
(§6.5 step 3) or restart-parked (A.3) — is treated as **consumed**: its bytes
and `IntroId` are **frozen**, a later same-source initiation parks as a *new*
entry subject to the per-source cap, and the staged accessors never straddle
two initiations. This closes SEC-3 (an off-path mac1-valid packet can no longer
byte-replace an entry whose cached mid-state proves identity `P`, so
`read_identity()`/`authenticate()` can never report `P` for bytes `P` never
sent) and IMP-6 (a replacement can never leave a stale cached claim against a
non-matching tail). State the DH-cost consequence in §6.1's footnote †: a
frozen entry's accessors return cached results at 0 incremental DH, and a new
same-source arrival pays its own ladder. *Fallback, if the maintainer prefers
minimal state:* "replacement replaces the entry wholesale; a replacement
without its own mid-state clears the identity-already-read tag and
`read_identity()` re-pays 1 DH." The freeze-on-carry form is recommended
because it also closes SEC-5's injection surface; pick one explicitly because
the DH-cost pins (Appendix B) depend on it.

**A.5 §6.4 re-home** (fixes FID-11; resolves the §6.4 [OPEN]). **Delete the
candidate-flag discard clause.** A re-home candidate is admitted on the other
three conditions (same proven static, tail tag verifies, guard admits) and
answered `CONTINUED = 0`. Rationale: a CONTINUATION = 1 candidate reaching the
re-home walk necessarily has **no** live connection for its static — a live
connection would have routed the rekey into the internal continuation (§6.5),
never to the accept/re-home path — so it is precisely §5.4's
"CONTINUATION = 1 · NONE" (we restarted) row: admit, answer CONTINUED = 0, let
the peer die honestly with `PeerRestarted` and reconnect. Keeping the discard
re-creates the exact livelock the [OPEN] identified (after a local restart every
candidate carries CONTINUATION = 1, so re-home returns `Stale` forever while the
fast path accepts the identical initiation). Replace the discarded clause with
the §16.1 guard: at admission, if a LIVE connection exists for the proven static
**and** the candidate is not a restart-replacement Intro (A.3),
`accept()` returns `AcceptError::AlreadyConnected`. The restart-replacement
Intro is the *sole* path that tears down a LIVE connection at accept.

**Sections touched:** §5.4 (table), §6.1 (footnote †), §6.3 (own-bytes extends
to mid-state entries), §6.4 (delete discard, add AlreadyConnected guard), §6.5
(confirm known-static routing), §6.6 (steps 3 + 6 rewrite; step-3 gating; A.4
parking), §6.7 (unchanged — but state loser ⇒ CONTINUED 0), §6.8 (restatement),
§15.4 (Replaced row: "torn down at `accept()`"), §17.1 (honesty clause), §18.1
(unchanged), Appendix B (continuation-flag matrix gains the PENDING-only column
and the simultaneous-open-completes test). **Wire impact: none** — one flag
*value* changes on one path (FID-1); no byte, layout, constant, or code moves.

**[MAINTAINER]:** this is a genuine redesign of the one new mechanism in the
wire break, and architecture D.3 already flagged the CONTINUATION machinery as
a maintainer decision. Tradeoff: the redesign trades a little more state (a
restart-replacement tag on parked Intros; frozen mid-state entries) for
correctness of ordinary simultaneous open (FID-1), invariant-preservation of
one-connection-per-static (FID-2/SEC-4), non-destruction of live connections by
withheld replays (SEC-5), and non-clobberability of DH-paid staged state
(SEC-3). Declining any part re-opens the SPEC-v2 tie-break BLOCKER's failure
mode or a live-connection teardown oracle. Confirm the whole cluster as a unit.

---

### Cluster B — flow control as the memory bound (§9.5–9.6, §10, §11, §16.8, §17.5) — **[MAINTAINER] (second bound + policy)**

Members: FID-4, FID-8, FID-9, SEC-2, IMP-5, SEC-9; with SEC-8, FID-14, FID-16
as tied residuals. Architecture D.11 ("confirm memory is bounded") cannot be
answered yes as written; these resolutions make it yes. Two sub-problems.

**B.1 Discarded / abandoned / reset bytes MUST advance connection credit**
(fixes FID-4, IMP-5, SEC-9). One rule in §10.3: **when a receive half is retired
for any reason** — read-to-final, reset observed, handle abandoned, sugar-
surfaced, or final size reached with no reader (§9.7) — **all of its bytes up to
its final size count as consumed for connection-level credit-advance.**
Stream-level credit is simply never re-granted for a retired stream (there is no
stream to grant to). Without this, MAX_DATA is an absolute limit advanced only
by reads, so cumulative discarded/reset bytes march the connection to a total
send stall after `INITIAL_MAX_DATA` (1 MiB) with no error and no timer — a
connection-fatal deadlock reachable in honest operation by any application that
cancels streams. Precedent: quinn issues the credit explicitly on abandonment
(`Recv::stop()`: "Issue flow control credit for unread data …
`self.end − self.assembler.bytes_read()`", `connection/streams/recv.rs:89-101`)
and on reset (`recv.rs:186-200`). **Ordering constraint (from SEC-8):** the
credit true-up is applied **after** a `FLOW_CONTROL_ERROR` check that a
`final_size`/`offset+length` never pushes stream- or connection-level
consumption above the advertised limit, with checked/saturating `u64`
arithmetic mandated (an unchecked `consumed + final_size` overflows for a
handful of 2⁶²-valued resets and silently re-opens the window). Add the
`FLOW_CONTROL_ERROR` case to §8.4's RESET_STREAM error list.

**B.2 The re-grant formula is stated, not inverted** (fixes FID-8). §10.3's
elaboration is unsatisfiable ("read ≥ W/2 beyond the last advertised *limit*"
can never be true — the read offset can never exceed the limit). Replace with:
the prospective limit is `bytes_read + WINDOW`; emit MAX_STREAM_DATA / MAX_DATA
when `prospective_limit − last_advertised ≥ WINDOW/2`, i.e. when the read offset
has advanced ≥ WINDOW/2 since the last advertisement. Matches RFC 9000 §4.2 and
quinn. This is a prerequisite for B.1 to actually move credit.

**B.3 A second bound on reassembly-fragment count/metadata, independent of byte
credit** (fixes SEC-2 sub-a, FID-9) — **[MAINTAINER] value**. Byte credit bounds
the *span* a receiver must cover (1 MiB) but not the *number of stored
discontiguous ranges* inside it: one-byte STREAM frames at offsets 0,2,4,… fill
~512 000 ranges within 1 MiB of credit, inflating real memory 25–50× (the exact
argument §11.3 uses to *reject* byte-bounded datagram queues — applied to the
unreliable path but not the reliable one). Add a normative reassembly-memory
mandate to §9.5/§10.6: **per-stream reassembly state MUST be O(advertised
credit) and MUST NOT scale with the number of received frames.** Two admissible
implementations, state at least the invariant and pick a default:
(a) a span-allocated buffer + a received-bitmap (1 MiB span ⇒ 1 MiB + 128 KiB,
frame-count-independent), or (b) a coalescing/defragment rule plus a per-stream
stored-chunk ceiling whose breach is `PROTOCOL_VIOLATION` (existing 0x01).
quinn does both: defragment when `over_allocation > max(32768, buffered·3/2)`,
hard-fail (`TooManyChunks`) above 1024 chunks (`connection/assembler.rs:196-218`).
The ceiling value ships ratified-but-revisitable, gated on the Appendix B
throughput/defragmentation-cost check. Add the corresponding worst-case row to
§17.5 (the "established connections" ceiling currently claims the credit term
dominates — true only once this bound exists).

**B.4 "Consumption" is defined; no unbounded intermediate queue** (fixes SEC-2
sub-b; ties into IMP-3). Define consumption as **the application taking bytes
out of the connection core**, and state that no unbounded intermediate queue may
exist between core and handle: message/datagram payloads stay accounted inside
the core (or its flow-control ledger) until the handle takes them, and
**reliable stream/message data MUST NOT be droppable** by §16.8's non-blocking
channel policy. This is the same conclusion IMP-3 reaches from the
implementability side (pull model, core retains reassembled-unclaimed streams);
resolve them together — the `ConnEvent::MessageReceived(Vec<u8>)` /
`DatagramReceived(Vec<u8>)` *push* variants become *readable signals* with core
claim verbs (Cluster G / IMP-3), which is what makes B.4 implementable.

**B.5 Wording collisions** (FID-14, FID-16). FID-14: rename §10.1's "sum over
all streams of final consumed offsets" to "the sum over all streams of the
highest received offset (the final size, once pinned)" — §10.7 already has the
correct rule; align the load-bearing sentence. FID-16 **[MAINTAINER]**: the
consumption-mode choice (`recv_message()` vs `accept_uni()`) is *not* invisible
on the wire — a sender writing > `INITIAL_MAX_STREAM_DATA` on a uni stream
against a message-mode receiver stalls forever (the receiver never extends
credit for a sugar-consumed stream, §9.8, and the stalled bytes also hold
connection credit per B.1). Resolve normatively: a message-mode receiver
**resets** a uni stream that exceeds `MESSAGE_RECV_MAX` (RESET_STREAM exists;
no wire change), or keeps extending stream credit until the message bound is
exceeded then surfaces it as a stream. Recommend the reset.

**Sections touched:** §8.4 (RESET_STREAM/STREAM `FLOW_CONTROL_ERROR` +
arithmetic), §9.5 (fragment bound), §9.6 (reset true-up + credit-on-free),
§9.8 (message-mode reset), §10.1 (FID-14 rename), §10.3 (B.1 rule + B.2
formula), §10.6 (fragment mandate; "consumption" definition), §16.8 (no-drop
for reliable data), §17.5 (worst-case rows). **Wire impact: none** (one new
non-wire constant — the chunk ceiling — and reuse of existing 0x01/0x02 codes).

**[MAINTAINER]:** the second-bound *shape and constant* (span+bitmap vs
chunk-cap; the ceiling value) is a real implementation trade with a throughput
cost that Appendix B must measure before ratification; and FID-16's message-mode
reset policy is a product-visible choice. B.1/B.2/SEC-8/SEC-9 are forced fixes,
not judgment calls.

---

### Cluster C — roaming + peer-driven send engine (§6.9, §7.3, §12.4, §13.4, §13.6, §14.5, §14.6) — **[MAINTAINER] (SEC-1 budget shape)**

Members: SEC-1 (BLOCKER), FID-6; riders FID-13 and the min_rtt-may-rise note.
The structural fault: §14 imported QUIC's *sending* engine (output rate is a
function of peer-supplied ACK feedback) onto §7.3's WireGuard-shaped roaming
(endpoint moves on **one** authenticated packet, no return-routability), then
exempted a large share of output (pure ACK, CLOSE, keepalive, PTO) from the one
window that might bound it. Two independent mechanisms.

**C.1 Anti-amplification budget** (fixes SEC-1) — **[MAINTAINER]**. Add a
send-side budget keyed to the current endpoint address, armed whenever the
endpoint address changes (roam) or is first anchored from a peer-supplied msg1
source:
- while an address is **unvalidated**, total bytes sent to it MUST NOT exceed
  `AMPLIFICATION_FACTOR` (= 3) × total bytes *received from* it on that session;
- the budget binds **all** output, **explicitly including the §14.5 and §13.4
  cwnd-exemptions** (pure ACKs, CLOSE, keepalives, PTO probes) — those
  exemptions are scoped to the congestion window, not to this budget; this is
  the load-bearing change, because the exemptions are what SEC-1 reflects
  through;
- **validation rule (recommended, minimal and sufficient):** the 3× ratio is
  never lifted — it caps amplification at the factor QUIC accepts (RFC 9000
  §8.2/§9.3) and forces an attacker to pay a third of its own flood, removing
  the reflection incentive entirely, at zero protocol machinery. (Optional
  stronger unlock: mark an address validated after ≥ N authenticated,
  window-fresh packets spread over ≥ 1 RTT; more state, same reflection
  property.)
- state the responder-side counterpart of §5.5 step 4: an initiator anchors at
  the **dialled** address, not the msg1 source; the responder's anchor at the
  msg1 source is what the budget must gate.

*Deferred alternative:* explicit PATH_CHALLENGE/PATH_RESPONSE from the reserved
frame space (roam deferred until the echo returns). Strictly stronger, strictly
more expensive — two frame types, a fourth reset seam, **and the only route in
this entire review that moves the wire.** Not recommended; record as the future
QUIC-faithful-migration lever.

**C.2 Fence pre-roam packets from the fresh path's controller** (fixes FID-6).
§13.6 keeps the sent map across a roam (correct — old-path ACKs must still
retire frames; all three reviewers agree, and dropping the map would strand
un-ACKed ranges outside both the pending set and the tracker, violating §16.7).
§14.6 clears the recovery period, which composes wrongly: the first post-roam
ACK declares the pre-roam flight lost into a fresh 12 000-byte window (halving
it), and the roam-triggering path break is exactly §14.4's persistent-congestion
predicate (two ack-eliciting packets > 3×PTO apart both lost, nothing acked
between) — so the new path starts at `MINIMUM_WINDOW` = 2400 B. Fix (RFC 9000
§9.4 MUST; quinn's path-generation stamping): on roam, **set the recovery-period
marker to the roam instant** (do not clear it) and **exclude packets sent before
the roam** from congestion events, from the persistent-congestion walk, from RTT
sampling, and from `app_limited` growth — they still resolve for
loss/retransmission. With this fence, "map kept, controller reset" is exactly
quinn's model and the `bytes_in_flight`-exceeds-fresh-cwnd stall is genuinely
bounded to one loss/PTO cycle (the PTO exemption keeps the path probeable; the
probe's ACK retires the old flight).

**C.3 Riders** (FID-13 + min_rtt). Correct §14.6's citation: RFC 9000 §9.4
resets the RTT estimator too; keeping RTT as a *prior* is a defensible
slither-specific choice (roaming is not adversarial migration here), but state
the consequence (a roam onto a slower path leaves stale `smoothed_rtt`/`min_rtt`
⇒ premature PTOs, compounding FID-7). Add the **min_rtt-may-rise-after-roam
rider** (SEC positive-clearance): §13.1's `min_rtt = min(min_rtt, latest)` is
monotone non-increasing for the connection's whole life; a post-roam `min_rtt`
MUST be allowed to increase, or an old short path pins PTO too low on a new long
one and manufactures spurious (cwnd-exempt) probes.

**Sections touched:** §5.5 step 4 / §5.6 (responder anchor), §6.9 (restate "No
amplification" per SEC-13), §7.3 (budget arming on roam/anchor), §13.1
(min_rtt-may-rise), §13.6 (fence pre-roam packets), §14.5 (budget binds the
exemptions), §14.6 (recovery marker = roam instant; citation fix). **Wire
impact: none** for the recommended path (one new non-wire constant
`AMPLIFICATION_FACTOR`). The PATH_CHALLENGE alternative would add two frame
types — do not take it unless the maintainer explicitly wants QUIC migration.

**[MAINTAINER]:** the budget's *shape* is the judgment call — the never-lifted
3× ratio (recommended) vs. a validation-unlock, and budget-vs-PATH_CHALLENGE.
The roaming reconciliation itself (keep the map with `bytes_in_flight`) is sound
and needs no ruling; it is an *ingredient* of SEC-1, safe only once C.1 is
applied on top of it.

---

### Cluster D — streams exactly-once (§8.4, §9.2, §9.7, §9.8)

Member: FID-3. §9.7 frees a receive half at read-to-final and §9.8 frees a
sugar stream the instant its message surfaces — both *before* the sender can
know (it learns only via our ACK). §9.2 then re-opens any freed ID on the first
STREAM/RESET_STREAM naming it, so a single lost ACK makes the peer's routine PTO
retransmission re-open the freed stream: the reassembler restarts, the FIN
re-pins a final size, and `recv_message()` **surfaces the same message twice**
(or a phantom `StreamOpened` fires for a finished stream) — falsifying §9.8's
"exactly-once is inherent in offset reassembly." The spec already contemplates
the case one frame over (§8.4: "Credit for a fully-closed stream is a valid
no-op").

**Resolution.** Add a **per-space closed-stream watermark** (highest
fully-closed index) alongside the open set. A STREAM or RESET_STREAM frame
naming an index **at or below the watermark and not currently open** is a
**no-op — ACKed, never re-opened**; implicit opening (§9.2) applies only to
indices *above* the watermark. This is quinn's `Free` tombstone /
"received RESET_STREAM on closed stream is a no-op" pattern
(`connection/streams/state.rs:319-325`, `:23-30`). State that the watermark
persists across rekey with the rest of stream state (§7.8). **Sections
touched:** §9.2, §9.7, §9.8, §8.4 (cross-reference the no-op). **Wire impact:
none.**

---

### Cluster E — recovery / CC details (§13.3, §13.4, §14.3, §14.4)

Members: FID-5, FID-7, FID-12, IMP-17.

**E.1 PTO arming** (FID-5). State the arming precondition (RFC 9002 §6.2.1 /
A.8): the `Pto` timer is armed only while **at least one ack-eliciting packet is
in the sent map**; the `Loss` timer, when armed, takes precedence (as §16.5
already says). Without it an idle connection self-sustains a ~20 pkt/s PING
train (PTO fires → bare PING is ack-eliciting → peer ACKs → `pto_count`→0 →
re-arm → fire), dwarfing the 10 s keepalive and defeating §16.5's timer economy.
Add to §13.3/§13.4/§16.5.

**E.2 Persistent-congestion period uses the un-backed-off PTO** (FID-7 =
IMP-17). §14.4's `persistent_period = PTO × PERSISTENT_CONGESTION_THRESHOLD`
must evaluate the §13.3 PTO **with `pto_count = 0`** (RFC 9002 §7.6.1) — the
backoff is deliberately excluded so the duration is a property of the path, not
the probe count. As written the threshold is up to 64× too long and persistent
congestion is never detected under the sustained loss it exists for. State
"the §13.3 PTO formula evaluated with `pto_count = 0`" in §14.4.

**E.3 No window growth during recovery** (FID-12). §14.3 ignores congestion
*events* for pre-recovery-period packets but is silent on *acknowledgments* of
them, so an implementation following §14.2 literally keeps growing cwnd via ABC
throughout recovery on the pre-cut flight's ACKs. Add the symmetric sentence
(RFC 9002 §7.3.2; quinn's `new_reno.rs:44-52` guards ack growth with the same
`sent <= recovery_start_time` test as loss): during the recovery period, ACKs of
packets sent before it started do not grow the window.

**Sections touched:** §13.3, §13.4, §14.3, §14.4, §16.5. **Wire impact: none.**

---

### Cluster F — liveness (§7.4–7.6, §7.7) — **[MAINTAINER] (anchor + config floor + idle-rekey)**

Members: SEC-6, SEC-11/FID-18, IMP-12.

**F.1 Correct the liveness anchor** (fixes SEC-6). §7.5's "measured from the
last (marking) send" is normative and *backwards*: a marking send *restarts* the
15 s, so an application writing into a black hole every second resets the death
clock every second and the session is immortal — the common case (bulk /
telemetry senders), not a corner, and `DEAD_TIMEOUT` is "the only idle killer."
Restate §7.4/§7.5 as WireGuard's rule: **the connection is dead when
`now − last_authenticated_recv > DEAD_TIMEOUT` AND at least one marking send has
occurred since `last_authenticated_recv`.** Equivalently, arm the liveness
deadline on the *first* marking send after a receive and **do not re-arm** it on
subsequent marking sends; every authenticated receive resets it. The whole
`seal`/`seal_quiet` split only makes sense under this reading.

**F.2 Config floor on persistent keepalive** (fixes SEC-6 sub-point). Either
reject `PERSISTENT_KEEPALIVE < DEAD_TIMEOUT` at the handle (recommended,
simpler, preserves the "application intent only" story) **or** exclude
persistent keepalives from the marking set. Without this, an application-chosen
5 s NAT-holding interval (entirely plausible) makes every partitioned connection
immortal under the corrected anchor too (a marking keepalive every 5 s keeps
`last_send` fresh, but F.1 keys death on `last_authenticated_recv`, so the floor
matters only if keepalives are marking; state which set they are in — see F.4).

**F.3 Idle sessions rekey** (fixes SEC-11 = FID-18) — **[MAINTAINER]**. Let the
keepalive count as a `REKEY_AGE` trigger so an idle-but-live session
re-handshakes every 120 s (WireGuard's shape: any sent packet past
Rekey-After-Time re-handshakes). Otherwise a keepalive-sustained session
performs no DH rekey *ever* (the keepalive is not "payload"; the epoch ratchet
is counter-derived and forward-only, spanning ~7.6 days at one counter / 10 s),
so the 120 s post-compromise-secrecy interval the design attributes to
`REKEY_AGE` is unbounded for idle tunnels. Cost: one DH handshake per 120 s per
idle connection. This is a carried ruling (architecture B.16) — flag it for the
maintainer rather than silently flipping it.

**F.4 State the keepalive/`REKEY_AGE` classification explicitly** (FID-18
second half, IMP-12). Resolve the ambiguity §7.6 leaves: whether the
empty-plaintext keepalive consults `REKEY_AGE` (recommended: it does, per F.3)
and whether it is in the marking set (per F.1/F.2). And adopt WireGuard's
persistent-keepalive *trigger* explicitly (IMP-12): fire if no marking send has
occurred for the interval; re-arm from every marking send.

**Sections touched:** §7.4, §7.5, §7.6, §16.2 (`set_persistent_keepalive`
validation). **Wire impact: none** (`DEAD_TIMEOUT` and the seal split are
unchanged; only the anchor definition, a config check, and the rekey-trigger
scope move).

**[MAINTAINER]:** F.1 (the anchor) is a forced correction. F.2 (reject
sub-`DEAD_TIMEOUT` keepalive) and F.3 (idle rekey) are judgment calls: F.2
constrains a config knob; F.3 buys a real 120 s PFS bound for idle tunnels at
one handshake per 120 s. Confirm both, and confirm F.4's classification.

---

### Cluster G — sans-io / shell surfaces (§6.9, §15.2, §16.2, §16.4, §16.8, §16.9, §9.1, §10.4)

Members: IMP-1 (= SEC-7), IMP-2, IMP-3, IMP-4. All sit at the core/shell API
seam; none moves a wire byte. Resolve IMP-3 jointly with Cluster B.4.

**G.1 Closing-state retention** (IMP-1 = SEC-7). §15.2's "retain **only** the
seal capability" contradicts §6.9 ("no path emits bytes in response to an
unauthenticated packet"): seal-only forces "from the peer" to mean "routed by
`receiver_index`", which an off-path forger who observed the cleartext index can
satisfy, turning the sealed CLOSE reply into an unauthenticated-triggered
emitter aimed at an attacker-chosen victim. State the retained set exactly: the
linger retains **the seal capability, the receive cipher states, and the replay
window**; a reply is owed **only to an authenticated, window-fresh inbound
packet**, and is sent **to the session's endpoint address** (the closing state
does not roam — never to the triggering packet's source). Retaining
open + a 256 B window for 5 s is cheap. Adjust §15.2's "retain only" wording.

**G.2 Pre-establishment stream IDs** (IMP-2). §16.9 makes pre-establishment
stream opens ordinary work, but the opener parity bit (§9.1) is fixed only at
establishment (a tie-break loser becomes the *acceptor*, §6.7), so IDs
pre-allocated with initiator parity are wrong on the wire. Rule: **stream IDs
are assigned at establishment; before it, handles hold core-internal indices**,
and `id()` on an unestablished connection's stream is defined (returns the ID
lazily post-install, or blocks). Any resolution produces identical on-wire IDs
(no frame is sent before install), so this is purely internal.

**G.3 Uniform pull model** (IMP-3, jointly with B.4). Three receive surfaces are
specified pull-with-claim at the API but push at the core, and the two conflict
(a spontaneous `MessageReceived(Vec<u8>)` push must convert a fully-reassembled
*unclaimed* uni stream into a message and free it *before* knowing the app's
`recv_message` vs `accept_uni` choice — after which an accept-only app can never
see it). Pick the pull model uniformly (quinn's shape, endorsed by the research
record): the core **retains** reassembled-but-unclaimed uni streams and queued
received datagrams; `ConnEvent` variants become **signals** (`MessageReadable`,
`DatagramReadable`, `StreamOpened{dir}`) and the core gains claim verbs
(`recv_message() -> Option<Vec<u8>>`, `recv_datagram() -> Option<Vec<u8>>`,
`accept(dir) -> Option<StreamId>`). The 64-datagram bound **and its drop
counter live in the core** (so §11.5's `slither::frames` drop trace is core
behaviour, not a shell detail). State the retention/backpressure rule:
reassembled-never-claimed uni streams hold their stream state and their
MAX_STREAMS credit until claimed. This is the surface that makes Cluster B.4's
"no unbounded intermediate queue / reliable data not droppable" implementable.

**G.4 Complete the wake-event set** (IMP-4). §16.8 wakes a blocked verb "by the
matching `ConnEvent`", but the set is incomplete: (a) `StreamOpened{dir}`
carries no `StreamId`, so `accept_bi`/`accept_uni` can't build handles — add the
id or the G.3 claim verb; (b) `open_*`/`send_message` wait for MAX_STREAMS
allowance but no event announces its arrival — add `StreamsAvailable{dir}` on
MAX_STREAMS receipt; (c) state when `StreamWritable` fires (on stream/connection
credit arrival for a stream with a blocked writer).

**Sections touched:** §9.1 (parity fixed at establishment — cross-ref G.2),
§10.4 (`StreamsAvailable`), §11.3/§11.5 (queue + counter in the core), §15.2
(retained set), §16.2 (`id()` semantics; `finish()`/`close()` await — IMP-11),
§16.4 (`ConnEvent` set: signals + ids + `StreamsAvailable`; claim verbs), §16.8
(no-drop for reliable data), §16.9 (early-send ID story). **Wire impact: none.**

---

### Cluster H — frame parsing & the [OPEN] resolutions (§8.2, §15.2, §15.4, §18.1, §6.4)

Members: FID-10, FID-24, and the two unanimous [OPEN] resolutions (§6.4 →
Cluster A.5; §15.2 → H.2). FID-17/SEC verdicts land here.

**H.1 Structural frame failure is a signalled death, not a silent livelock**
(fixes FID-10) — **[MAINTAINER] (behaviour change, wire-free)**. §8.2 drops the
whole packet with "nothing applied" on a structural failure, justified as
"unknown means corruption or skew." After the AEAD tag verifies, corruption is
excluded (2⁻¹²⁸) and skew is excluded by design (one version, no negotiation) —
the only remaining causes are a peer bug or a deliberate violation, i.e. exactly
the population §8.2's *semantic* class already sends CLOSE to. Because nothing is
applied and no ACK is scheduled while both liveness clocks stay fresh (the
packets *are* received and window-marked), a peer emitting a malformed frame
retransmits it forever — an unbounded livelock with no error and no operator
signal. Resolution: move the **post-AEAD** structural class to a signalled
death — **CLOSE with `PROTOCOL_VIOLATION` (existing `0x01`)**, then linger.
Keep the silent drop **only** for packets that fail *before* the AEAD (§3.1's
length/type/version gate), where corruption is genuinely possible. Note: this
does **not** need a new error code — 0x01 in the §15.3 registry suffices
(FID-10's `FRAME_ENCODING_ERROR`-from-reserved alternative is unnecessary), so
**no wire byte and no registry value moves.** Update §15.4 (the FID-10 livelock
now has a teardown row) and, per H.2, its local surface.

**H.2 Add `ConnectionLost::ProtocolViolation { code }`** (fixes FID-17; resolves
the §15.2 [OPEN]) — **[MAINTAINER]**. `LocallyClosed` currently surfaces both
"the application called close()/dropped the last handle" and "the peer's
misbehaviour forced a teardown" — opposite causes with opposite operational
responses (peer reputation, allow-list demotion, alerting). This is a
security-signal question, and §18.1's own precedent is that a security signal
earns its own variant (`AuthError::HandshakeFailed` is called out as such). Add
`ConnectionLost::ProtocolViolation { code }` (carrying the §15.3 code); update
§15.2, §15.4's two violation/FID-10 rows, and §18.1. **Wire impact: enum-level
only** — the CLOSE frame and the on-wire code are unchanged.

**H.3 Mutual-close draining** (FID-24). A closing endpoint answering the peer's
own CLOSE at ≤ 1/s produces a bounded 1 Hz CLOSE ping-pong for 5 s. State that a
CLOSE **received** while closing moves to the reply-free **draining** behaviour
(§15.2 already defines draining for the peer-CLOSE-received case; extend it to
the already-closing case).

**Sections touched:** §3.1 (pre-AEAD silent drop retained), §8.2 (post-AEAD
structural → CLOSE 0x01), §15.2 (violation surface; draining-on-CLOSE-while-
closing), §15.4 (rows), §18.1 (`ProtocolViolation` variant). **Wire impact:
none** (H.2 is one enum variant; H.1 reuses 0x01).

**[MAINTAINER]:** H.1 changes a behaviour (silent drop → CLOSE) though it moves
no bytes — confirm the population is right (post-AEAD structural failures are
peer-bug-or-attack, never corruption). H.2 is the §15.2 [OPEN] ruling (add the
variant).

---

## 3. Residual MINOR / NIT dispositions (each swept, none dropped)

Fix = apply as stated; the finding's own recommended wording is adopted unless
noted. All wire-impact ∅.

- **SEC-10 — fix (doc) + defer (optional mechanism).** State in §6.9 that eager
  `es` is rate-ungated per spoofed hint-set source (caps bound *state*, not
  *work*) and that re-home candidates are attacker-fillable (a legitimate
  `accept()` may spend 8 DH on rubbish then return `Stale`); the posture is
  already ruled (§19 cookies/mac2). *Defer* the optional per-source `es` budget
  (charge-before-DH, refund-on-demotion) — sound but not required for v1.
- **SEC-12 — fix.** Add a normative line to §16.6/§17.3: indices MUST be
  unpredictable to an off-path observer; the config-supplied RNG seed is a
  test-only facility (feature-gate or document as security-relevant). Index
  unpredictability is load-bearing for §5.5's on-path-only completion spend and
  SEC-7's linger reply.
- **SEC-13 — fix.** §6.9: add the initiator-forged-msg2 row (2 DH: `ee`+`se`)
  so the "2-DH ceiling" table is complete; restate "No amplification" as "no
  path emits bytes **to an address** that has not authenticated" (the property
  SEC-1 restores).
- **FID-12 — fix (Cluster E.3).**
- **FID-13 — fix (Cluster C.3).**
- **FID-14 — fix (Cluster B.5).**
- **FID-15 — fix.** §10.4: scope the +1 grant to fully-closing a **peer-opened**
  stream of the matching space (RFC 9000 §4.6); closing our own streams must not
  inflate the peer's allowance.
- **FID-16 — fix (Cluster B.5, [MAINTAINER] reset policy).**
- **FID-19 — fix (label).** §12.5: mark the above-highest-sealed-ACK-ignored
  rule as a deliberate divergence from RFC 9000 §13.1's PROTOCOL_VIOLATION
  SHOULD (carried from v1).
- **FID-20 — fix (label).** §13.4: mark the probe-content simplification
  (pending-oldest-first-else-PING, one packet) as a deliberate divergence from
  RFC 9002 §6.2.4 (new-before-old, up to two packets).
- **FID-21 — fix.** §11.3/§17.5: 64 × 1169 = 73.06 KiB; use "≈ 73 KiB" (and
  "≈ 146 KiB" for both) or "≈ 75 kB / ≈ 150 kB".
- **FID-22 — fix.** §12.4: define the first-packet-of-session case (no previous
  greatest ⇒ the immediate-on-gap rule yields an immediate ACK; harmless).
- **FID-23 — fix.** §16.2/§8.4: `close(code, reason)` truncates `reason` at
  `CLOSE_REASON_MAX` (256 B) at the handle (a received `reason_len > 256` is
  already a §8.4 structural failure — the handle must not be able to *produce*
  one).
- **FID-24 — fix (Cluster H.3).**
- **FID-25 — fix (cosmetic).** Unify `Endpoint` parameterisation across
  §2.2/§16.2/§16.4; use `MSG1_PAYLOAD_LEN`/`MSG2_PAYLOAD_LEN` in §2.3's size
  formulas (they are test-pinned).
- **IMP-7 — fix (bless interim).** Appendix A.2/§3.4: "the counter
  mirror-and-assert interim is conformant until the accessor ships" — recommended,
  the `debug_assert_eq!` makes divergence impossible to miss. State normatively
  (per SEC "couldn't rule out") that the mirrored value is used **only** for the
  AD and is never fed back to hiss.
- **IMP-8 — fix (note interim).** Appendix A.3: note the slither-side
  `where C::PublicKey: AsRef<[u8]> + Ord` interim compiles on 0.3.1; the
  critical path is A.1 alone.
- **IMP-9 — fix.** Appendix A.1: shape the `Mid` surface — it exposes the
  claimed static (read_identity + §6.5 eager inspection) and `complete()`
  returns the 13-byte payload alongside the responder state.
- **IMP-10 — fix.** §8.5: name a stream fill scheduler (round-robin over streams
  with pending data — one sentence, matches the no-HOL-blocking pitch) or
  explicitly declare it implementation-defined.
- **IMP-11 — fix.** §16.2: `finish()` resolves when the FIN is accepted into the
  send state (errors via `WriteError`); `close()` resolves once CLOSE is sealed
  and closing is entered.
- **IMP-12 — fix (Cluster F.4).**
- **IMP-13 — fix.** §16.5: add orphan aging (§17.1 mitigation ii, `INTRO_TTL`
  scale) to the endpoint core's announced deadline enumeration.
- **IMP-14 — fix.** §14.5: pin the `app_limited` mechanism (quinn's — a flag
  maintained by the send path, recorded onto each sent packet, read on ack) and
  add the one-paragraph hostile-peer reasoning the security lane asked for (a
  peer manipulating ACK timing controls when the predicate is evaluated; the
  never-set direction accrues unearned window and feeds SEC-1 — bound it).
- **IMP-15 — fix.** §16.4: state the core staged-verb signatures and the
  `EstablishedSession` shape (two lines removes the inference).
- **IMP-16 — fix.** §7.6/§16.4: say `NeedsRekey` re-emits per payload seal past
  120 s with the endpoint deduping against its one-pending-per-static invariant
  (or once with a core latch — pick the former).
- **IMP-17 — fix (Cluster E.2).**
- **IMP-18 — fix.** §16.5: place `AckDelay` and `PersistentKeepalive` in the
  equal-deadline priority list.
- **IMP-19 — fix.** Appendix B: name the throughput target ("within X% of the
  same FlakyWire topology under quinn's defaults") so the gate can fail
  objectively.
- **IMP-20 — fix.** §8.4: reword MAX_STREAM_DATA's "not implicitly openable"
  clause to QUIC's rule — a frame for a stream in a space the frame's *receiver*
  opens, that the receiver has not opened, ⇒ `STREAM_STATE_ERROR`.

---

## 4. Fix-interaction audit

The earlier pipeline's round-2 fixes introduced three regressions (a tie-break
regression, a retransmit storm, a cap-exemption). Each analogue is checked
against this round's fixes, plus the four cross-checks the task named.

**(1) Cluster A redesign does not regress the post-`ss` tie-break
unforgeability (the SPEC-v2 BLOCKER).** The tie-break still runs *only* at
§6.6 step 5, strictly after the tag (step 1), guard (step 2), and — for the
PENDING state — after flag-routing is *skipped*, not fired. A forgery still dies
at step 1 with the pending untouched ("a forgery cannot cancel a pending"). The
three-valued routing only changes which branch the PENDING state takes (now →
tie-break instead of the erroneous restart-teardown of FID-2/SEC-4) and defers
the LIVE-state `Replaced` teardown to `accept()`; neither touches the tie-break
comparison (still over the proven, equal-length canonical static, §2.4). A.4's
freeze-on-carry additionally *strengthens* the invariant: an unauthenticated
mac1-valid packet can no longer byte-replace DH-paid mid-state, closing SEC-3.
SEC's positive clearance of §6.7 confirms unforgeability is intact under this
redesign. **No regression.**

**(2) Cluster C's anti-amplification budget does not break legitimate roaming or
the liveness clock.** The budget throttles *send* to an *unvalidated* address to
3× bytes *received from it*. A legitimate roam is driven by the peer's own
authenticated data from the new address (§7.3), which supplies received-bytes
continuously, so the budget clears within ~1 RTT and legitimate traffic
proceeds. The liveness clock is untouched: liveness keys on
`last_authenticated_recv` (Cluster F.1), a *receive* quantity, while the budget
gates *send* — orthogonal axes. The "black-holed path must stay probeable"
property survives: a genuinely new path that sent us data has budget for the PTO
probe; a path that sent us *nothing* has no budget, but also fails liveness and
dies at 15 s — no deadlock, and no reflection (we cannot flood a victim that
sent us nothing, which is exactly the point). The min_rtt-may-rise rider (C.3)
prevents an old short path from manufacturing spurious cwnd-exempt probes that
the budget would otherwise have to absorb. **No break.** Rider: the budget must
be applied *on top of* the kept sent map (C.2), not instead of it.

**(3) Cluster B's discard-credit rule cannot be turned into a credit-inflation
attack.** B.1 credits `final_size` on retirement, advancing the *receiver's own*
advertised MAX_DATA to the peer. The credited amount is bounded by `final_size`,
which SEC-8/B.1's ordering constraint checks **≤ the already-advertised limit
before** the true-up (a `final_size` beyond the limit is `FLOW_CONTROL_ERROR`,
not a credit) — so the true-up is net-zero to ledger integrity: it releases
exactly the credit the arriving/asserted bytes had already consumed, never more.
Checked/saturating `u64` arithmetic closes the overflow-wraps-the-comparison
inflation SEC-8 identified. A malicious peer therefore cannot manufacture credit
it did not pay for; the worst it can do is waste its *own* window (SEC-9's
self-harm framing). **No inflation.** This is why SEC-8 must land *with* B.1, not
after.

**(4) Cluster F's liveness cap does not kill healthy long-lived sessions.** The
corrected anchor (F.1) resets the death clock on **every authenticated
receive**; it arms once per receive-gap and does not re-arm on sends. A healthy
session receives ACKs (delayed ≤ 25 ms, Cluster's §12.4) and/or data
continuously, so `last_authenticated_recv` keeps advancing and the clock keeps
resetting — the session lives as long as the peer is responsive. Only a session
that *sends but receives nothing* for 15 s dies, which is the intent. The
persistent-keepalive floor (F.2) removes the immortality of a partitioned
session without touching a healthy one (a healthy session's keepalives are
answered, so it never relies on the floor). **No healthy-session kill.**

**Round-2 regression analogues, explicitly:**
- *Tie-break regression* → covered by (1); the redesign strengthens, not
  regresses, the post-`ss` guarantee.
- *Retransmit storm* → this round *removes* two storms (FID-5's idle PING train;
  FID-10's malformed-frame-retransmit-forever livelock) and introduces none:
  H.1's CLOSE is sent once then lingers; the C.1 budget and C.2 fence *block or
  drain* sends rather than retransmitting; B.1's credit release *unblocks*
  senders. No fix adds an unbounded send loop.
- *Cap-exemption* → SEC-1's core insight is that the cwnd-exemptions
  (ACK/CLOSE/keepalive/PTO) were the amplification hole; C.1 explicitly **binds
  those exemptions** under the amplification budget, and introduces no new
  exemption (the budget binds *all* output). The §14.5 cwnd-exemptions remain
  correct *as congestion-window* exemptions; they are simply no longer
  *amplification* exemptions on an unvalidated address.

**One new cross-cluster ordering to honour:** A.3 (defer `Replaced` to accept)
and A.5 (AlreadyConnected-at-accept guard) share the accept() install point —
the reviser must state that the *sole* path tearing down a LIVE connection at
accept is a restart-replacement Intro (A.3-tagged); every other accept against a
LIVE static returns `AlreadyConnected`. Getting this backwards would either
re-open SEC-5 (teardown too eager) or block legitimate restart replacement.

---

## 5. NEW [MAINTAINER] flags the reviser must plant in SPEC-DRAFT-v2

Seven, beyond the [MAINTAINER] tags already in v1. Each is a genuine judgment
call the fixes introduce; the reviser plants the flag with the one-paragraph
tradeoff and applies the recommended resolution as the default.

1. **[MAINTAINER] — Cluster A: the corrected continuation state machine
   (confirms/extends D.3).** Three-valued local-state routing; flag-routing
   gated on LIVE; `Replaced` teardown deferred to `accept()`; CONTINUED computed
   (never hardcoded); mid-state entries frozen/consumed; §6.4 discard deleted +
   AlreadyConnected-at-accept guard. Confirm as a unit — it is the one new
   mechanism in the wire break and every member finding depends on the others.

2. **[MAINTAINER] — Cluster C: the anti-amplification budget (SEC-1).**
   `AMPLIFICATION_FACTOR = 3` binding **all** output (including the cwnd-exempt
   ACK/CLOSE/keepalive/PTO classes) on an unvalidated address; never-lifted 3×
   ratio (recommended) vs. a validation-unlock; wire-free budget vs. the
   wire-touching PATH_CHALLENGE alternative (do not take the latter unless
   QUIC-faithful migration is wanted).

3. **[MAINTAINER] — Cluster B: the second receive-memory bound (SEC-2/FID-9).**
   Span-buffer+received-bitmap (O(credit)) and/or a per-stream stored-chunk
   ceiling with `PROTOCOL_VIOLATION` breach; the ceiling constant ships
   ratified-but-revisitable, gated on the Appendix B defragmentation/throughput
   check.

4. **[MAINTAINER] — Cluster B: message-mode overflow policy (FID-16).** Reset a
   message-mode uni stream that exceeds `MESSAGE_RECV_MAX` (recommended) vs.
   extend stream credit until the bound then surface as a stream.

5. **[MAINTAINER] — Cluster F: the liveness config floor and idle rekey.**
   Reject `PERSISTENT_KEEPALIVE < DEAD_TIMEOUT` (or exclude persistent
   keepalives from the marking set); and let the keepalive count as a
   `REKEY_AGE` trigger so idle-but-live sessions rekey every 120 s (F.3 — a real
   PFS bound at one handshake/120 s, flipping a carried B.16 ruling). (F.1, the
   anchor correction, is a forced fix, not a flagged call.)

6. **[MAINTAINER] — Cluster H: structural-frame-failure signalling (FID-10).**
   Post-AEAD structural failures move from silent drop to CLOSE with the
   existing `PROTOCOL_VIOLATION` (0x01); silent drop kept only pre-AEAD. A
   wire-free *behaviour* change — confirm the population (post-AEAD structural
   failure is peer-bug-or-attack, never corruption).

7. **[MAINTAINER] — Cluster H: `ConnectionLost::ProtocolViolation { code }`
   (FID-17; resolves the §15.2 [OPEN]).** Add the variant so peer-misbehaviour
   teardown is distinguishable from application close; enum-level, wire-free.

Two pre-existing v1 [OPEN]s are **resolved** by this document (the reviser
applies the resolution, does not re-flag as open): §6.4 (delete the discard
clause → Cluster A.5) and §15.2 (add the variant → flag 7 above).

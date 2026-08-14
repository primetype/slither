# Protocol-fidelity review — SPEC-DRAFT-v1.md (2026/08/13)

Reviewer lane: internal consistency, RFC fidelity (RFC 9000 §§2–4/16/19, RFC 9002
§§5–7, RFC 9221), hiss-contract fidelity (research-gaps.md, research-wire-
reconciliation.md), mechanism composition. Pure attack analysis and buildability
are out of lane except where a fidelity defect causes them.

**Counts.** 4 BLOCKER · 7 MAJOR · 7 MINOR · 7 NIT (25 findings). All 25 are
*confirmed* from the draft's own text plus a cited RFC/hiss/quinn fact; three
carry an additional "needs-analysis" rider, listed separately in §D.

---

## A. Arithmetic and size recomputation (all recomputed from first principles)

| Claim | Spec | Recomputed | Verdict |
|---|---|---|---|
| `IK_MSG1_LEN` (`e ‖ enc_s ‖ enc_payload`) | 175 | 65 + (65+16) + (13+16) = 65+81+29 = **175** | ✅ |
| `IK_MSG2_LEN` (`e ‖ enc_payload`) | 82 | 65 + (1+16) = 65+17 = **82** | ✅ |
| `INIT_PACKET_LEN` | 197 | 6 + 175 + 16 = **197** | ✅ |
| `RESP_PACKET_LEN` | 108 | 10 + 82 + 16 = **108** | ✅ |
| `INIT_HEADER_LEN` / `RESP_HEADER_LEN` / `DATA_HEADER_LEN` | 6 / 10 / 14 | 1+1+4 / 1+1+4+4 / 1+1+4+8 = **6 / 10 / 14** | ✅ |
| `MAX_PLAINTEXT` | 1170 | 1200 − 14 − 16 = **1170** | ✅ |
| per-seal bound (§2.1, §8.6) | 1186 ≪ 65535 | 1170 + 16 = **1186**; 1186 + 14 = 1200 ✓ | ✅ |
| `MAX_DATAGRAM_PAYLOAD` | 1169 | 1170 − 1 (one `0x30` type byte, data-to-end) = **1169** | ✅ |
| keepalive datagram | 30 B | 14 + 16 (tag-only ciphertext) = **30** | ✅ |
| `INITIAL_WINDOW` | 12 000 | min(10×1200, max(2×1200, 14720)) = min(12000, 14720) = **12000** | ✅ RFC 9002 §7.2 |
| `MINIMUM_WINDOW` | 2 400 | 2 × 1200 = **2400** | ✅ |
| replay window bytes | 256 B | 2048 bits = [u64;32] = **256 B** | ✅ |
| replay reordering time | ~20 ms @1 Gbps / ~200 ms @100 Mbps | 1e9/(1200×8) = 104 167 pkt/s → 2048/104167 = **19.7 ms**; ×10 = **197 ms** | ✅ |
| intro queue worst case | ≈ 225 KB | 220 × 1024 = **225 280 B** | ✅ |
| sustained-occupancy flood rate | ≈ 68 pkt/s | 1024/15 = **68.3** | ✅ |
| amplification ratio | < 1 | 108/197 = **0.548** | ✅ |
| pacing | 50/s ⇔ 20 ms | 1/0.02 = **50** | ✅ |
| varint table | 63 / 16 383 / 1 073 741 823 / 2⁶²−1 | 2⁶−1, 2¹⁴−1, 2³⁰−1, **4 611 686 018 427 387 903** | ✅ RFC 9000 §16 |
| stream-ID split | 2 tag bits + 60-bit index | (index<<2)\|(dir<<1)\|opener; max index 2⁶⁰−1 ⇒ ID ≤ 2⁶²−1 | ✅ RFC 9000 §2.1 / quinn `lib.rs:255-276` |
| `MESSAGE_RECV_MAX` | 262 144 | = `INITIAL_MAX_STREAM_DATA` = 256×1024 = **262 144** | ✅ |
| datagram queue memory | ≈ 75 KiB each / ≈ 150 KiB both | 64 × 1169 = **74 816 B = 73.06 KiB** (74.8 kB) | ⚠ FID-21 (rounding) |
| `REKEY(0³²)` ChaCha20-Poly1305 vector | `25ce…4c58` | ChaCha20 block(key=0³², ctr=1, nonce=00000000‖ffffffffffffffff) [0..32] = `25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58` | ✅ **byte-exact** |

**Every packet byte count checks out.** The three handshake-size claims, both
header tables, both plaintext caps, the datagram payload derivation, the two
NewReno windows, the replay-window sizing arithmetic and the pinned Noise
`Rekey()` vector are all reproduced exactly. The single numeric slip is a KiB/kB
rounding (FID-21). The ACK range algebra (§12.1) reproduces RFC 9000 §19.3.1
exactly: first block `largest−first_range ..= largest`; subsequent
`block_largest = prev_smallest − gap − 2`, `block covers block_largest−range ..=
block_largest`. No off-by-one.

---

## B. Confirmed findings

### BLOCKER

**FID-1 — Simultaneous open of two fresh `connect()`s can never complete.**
§5.4, §6.6 step 6, §6.7, §16.4.
§6.6's admit step writes msg2 **unconditionally** with `CONTINUED = 1`
("msg2 written (`ee`, `se`, +2 DH, CONTINUED = 1)"), but §5.4 defines
`FLAG_CONTINUED` as "set **iff** the responder matched the initiation to an
existing connection **and retains its transport state**". In an ordinary
simultaneous open both sides are fresh diallers, so both sent
`CONTINUATION = 0`; the tie-break loser (§6.7) admits the inbound through the
continuation and, per §6.6 step 6, answers `CONTINUED = 1` — while the loser is
completing a *never-established* connection (`Install { initial: true }`, §6.7)
that retains nothing. The winner, which sent `CONTINUATION = 0`, applies §5.4's
initiator rule verbatim: "an answer no honest responder can produce … the msg2
is discarded and the completion attempt is spent". Every retransmit repeats
this, so simultaneous open resolves to `ConnectError::TimedOut` at
`HANDSHAKE_GIVEUP` (90 s) instead of the shared session §6.7 promises. Root
cause: `CONTINUED` is specified as a property of the *code path* (the
continuation) rather than of *retained transport state*.
**Fix:** make step 6 emit `CONTINUED = (a live connection with retained
transport state was matched)` — i.e. 1 for the rekey/replacement swap
(`Install{initial:false}`), 0 whenever the continuation completes a
never-established connection (`Install{initial:true}`). Add the corresponding
row to §5.4 and to Appendix B's continuation-flag matrix (which today tests only
the four cases that hide this). **Wire impact:** no byte or constant moves; one
flag *value* changes on one path.

**FID-2 — §6.6's flag routing runs on the pending-only branch, so the
restart/rekey rules pre-empt the tie-break.** §5.4, §6.5 step 3, §6.6 step 3,
§6.7, §16.1.
§6.5 routes into the internal continuation whenever the claimed static ∈ *known
statics* = "established connections **∪ pending outbound remotes**", but §5.4's
responder table has only two local-state columns ("live connection" / "none").
The real state space has three: live connection · in-flight outbound pending
only · neither. The pending-only column is unhandled, and §6.6's numbered order
(flag routing at step 3, tie-break at step 5) resolves it wrongly in both flag
values: `CONTINUATION = 0` + pending-only (the ordinary simultaneous open) hits
step 3's restart branch — "the matched connection tears down
(`ConnectionLost::Replaced`) … the initiation parks as a fresh stage-0 entry" —
although there is no connection to replace, the tie-break at step 5 is never
reached, and the resulting fresh `Intro` would create a second connection to a
static that already has an in-flight pending (§16.1's invariant, and exactly the
"whichever completes first installs different key sets" failure §6.7 exists to
prevent). `CONTINUATION = 1` + pending-only (a state-retaining peer rekeying
into our post-restart fresh dial) hits step 3's "proceed" branch and is admitted
as a *swap* into a connection core whose stream offsets start at 0 — merging two
transport-state generations, which §5.4 claims "no flag combination" can do.
**Fix:** gate step 3 on "a live connection with retained transport state exists
for the proven static"; the pending-only case skips step 3 entirely and is
resolved by the tie-break (step 5), with the flag value from FID-1.
**Wire impact:** none (routing only).

**FID-3 — Eagerly freed streams are re-opened by late STREAM/RESET_STREAM
frames, breaking §9.8's exactly-once contract.** §9.2, §9.7, §9.8, §8.4.
§9.7 frees a receive half "when the application has read to the final size" and
§9.8 frees a sugar stream the moment its message is surfaced — both strictly
*before* the sender can know (the sender learns only via our ACK). §9.2 then
says "A frame referencing stream `N` of a space opens `N` … The first STREAM or
RESET_STREAM frame is the open," with no exception for an already-closed ID. A
single lost ACK therefore makes the peer's routine PTO retransmission re-open
the freed stream: the reassembler restarts from nothing, the FIN re-pins a final
size, and `recv_message()` surfaces the **same message a second time** — or, for
`accept_uni`/`accept_bi`, a phantom `StreamOpened` event fires for a stream the
application already finished. §9.8's headline claim ("exactly-once surfacing is
inherent in offset reassembly") is false without a closed-stream record, and the
spec already contemplates the case one frame type over ("Credit for a
fully-closed stream is a valid no-op", §8.4) — an internal inconsistency.
RFC 9000 handles this by keeping per-ID state distinguishing "closed" from
"never opened"; quinn keeps `recv: FxHashMap<StreamId, Option<StreamRecv>>` with
a `Free` tombstone and answers "received RESET_STREAM on closed stream" as a
no-op (`connection/streams/state.rs:319-325`, `:23-30`).
**Fix:** add a per-space *closed watermark* (highest fully-closed index) plus the
open set; a STREAM/RESET_STREAM frame naming an index that is at-or-below the
watermark and not currently open is a **no-op** (ACKed, never re-opened), and
implicit opening (§9.2) applies only to indices above it. **Wire impact:** none.

**FID-4 — Connection-level flow-control credit is never released for bytes that
are discarded rather than read; the connection deadlocks after
`INITIAL_MAX_DATA` such bytes.** §9.6, §10.1, §10.3, §16.2, §9.7.
§10.3 rules that "Consumption, not arrival, drives credit: an unread buffer
earns nothing", and §16.2 rules that an abandoned `RecvStream`'s "arrivals … are
discarded, credit never advances". Nothing anywhere converts discarded bytes
into credit-advance. Three paths therefore burn connection-level credit
irrecoverably: (a) a dropped `RecvStream` (its bytes are discarded and can never
be read); (b) a RESET_STREAM, whose `final_size` §9.6 charges "against MAX_DATA
consumption exactly as if the bytes had arrived" — but the bytes never arrive,
so they can never be read; (c) any receive half freed with no reader (§9.7's
third bullet). Because MAX_DATA is an *absolute* offset limit advanced only by
reads, the limit stops moving while the sender's high-water mark keeps counting:
after `INITIAL_MAX_DATA` = 1 MiB of cumulative discarded/reset bytes the
**entire connection** — every stream, not just the abandoned one — is
permanently unable to send, with no error surfaced and no timer that fires
(liveness is satisfied by keepalives). §16.2 describes the symptom as stream-
local ("a sender that keeps pushing stalls at the window"); it is
connection-fatal. STOP_SENDING, QUIC's escape hatch, is deferred (§9.9), which
makes the rule load-bearing here in a way it is not in QUIC. quinn issues the
credit explicitly on abandonment — `Recv::stop()`: "Issue flow control credit
for unread data … `self.end - self.assembler.bytes_read()`"
(`connection/streams/recv.rs:89-101`) — and on reset via `credit_consumed_by` +
buffer nuke (`recv.rs:186-200`).
**Fix:** one rule in §10.3 — when a receive half is retired for any reason
(read-to-final, reset observed, handle abandoned, sugar-surfaced), **all of its
bytes up to its final size count as consumed** for credit-advance purposes.
**Wire impact:** none.

### MAJOR

**FID-5 — The PTO is never disarmed, so idle connections probe forever.**
§13.3, §13.4, §16.5.
§13.3 anchors the PTO "at the last ack-eliciting send" and gives no arming
precondition; §13.4 says a firing PTO sends "one ack-eliciting packet: pending
retransmittable frames oldest-first if any exist, else **a bare PING**". RFC 9002
§6.2.1 / Appendix A.8 (`SetLossDetectionTimer`) is explicit: "if (no
ack-eliciting packets in flight …) … There is nothing to detect lost, so no
timer is set." Without that condition the machine self-sustains: idle
connection → PTO fires → bare PING (ack-eliciting, tracked) → peer ACKs it →
§13.3's "`pto_count` resets to 0 whenever any packet is newly acknowledged" →
PTO re-arms one un-backed-off PTO after the PING → fires again. On a LAN
(PTO ≈ RTT + 4·rttvar + 25 ms ≈ 30–60 ms) that is a permanent ~20 pkt/s probe
train on a *completely idle* connection, dwarfing the 10 s keepalive the design
chose for cheapness, burning counters, and defeating §16.5's timer economy.
**Fix:** state the arming condition — the `Pto` timer is armed only while at
least one ack-eliciting packet is in the sent map (and the `Loss` timer, when
armed, takes precedence, as §16.5 already says). **Wire impact:** none.

**FID-6 — Roaming keeps the sent map but lets pre-roam packets drive the fresh
controller; a roam therefore collapses cwnd to `MINIMUM_WINDOW`.** §13.6, §14.6,
§14.3, §14.4, §7.3.
§13.6 keeps the sent map across a roam (correct — old-path ACKs must still
resolve) and §14.6 resets the controller "with the recovery period **cleared**".
The two compose wrongly: with the recovery marker cleared, the *first* ACK after
the roam declares the pre-roam in-flight packets lost and feeds them to a
brand-new 12 000-byte window, halving it immediately; worse, the roam-triggering
event (NAT rebind / path break) is precisely the case where two pre-roam
ack-eliciting packets sent more than 3×PTO apart are both lost with nothing
acknowledged between them, which is §14.4's persistent-congestion predicate —
so the new path starts at `MINIMUM_WINDOW` = 2400 B. RFC 9000 §9.4 is a MUST on
this point: "Packets sent on the old path MUST NOT contribute to congestion
control or RTT estimation for the new path." quinn implements exactly that by
stamping each `SentPacket` with a path generation and attributing
ack/loss/abandon to the path it was sent on (`connection/mod.rs:3683-3693`,
`connection/paths.rs:173-181`), and its fresh controller starts with
`recovery_start_time = now`, not cleared (`congestion/new_reno.rs:26-34`, whose
`on_ack`/`on_congestion_event` both early-return on `sent <= recovery_start_time`).
**Fix:** on roam, set the recovery-period marker to the roam instant (do not
clear it) and exclude packets sent before the roam from congestion events, from
the persistent-congestion walk, from RTT sampling and from `app_limited` growth
— they still resolve for loss/retransmission purposes. **Wire impact:** none.

**FID-7 — §14.4's persistent-congestion period uses the backed-off PTO.**
§13.3, §14.4.
§14.4 defines `persistent_period = PTO × PERSISTENT_CONGESTION_THRESHOLD` and
§13.3 defines PTO as the formula "doubled per consecutive unanswered probe
(`2^pto_count`), capped at `PTO_BACKOFF_CAP` = 2⁶". RFC 9002 §7.6.1 requires the
*un-backed-off* value: "(smoothed_rtt + max(4·rttvar, kGranularity) +
max_ack_delay) × kPersistentCongestionThreshold" — the backoff is deliberately
excluded so the duration stays a property of the path, not of the probe count.
Read literally the draft makes the threshold up to 64× too long, so persistent
congestion is effectively never detected under exactly the sustained-loss
conditions it exists for. **Fix:** "the §13.3 PTO formula evaluated with
`pto_count = 0`". **Wire impact:** none.

**FID-8 — The credit re-grant rule's operative clause is unsatisfiable as
written.** §10.3.
"a receiver re-advertises a level's limit when application consumption has
advanced it at least half a window beyond the last advertised value — **for a
stream, when the application has read ≥ `INITIAL_MAX_STREAM_DATA`/2 beyond the
last MAX_STREAM_DATA sent**; for the connection, ≥ `INITIAL_MAX_DATA`/2 beyond
the last MAX_DATA." The elaboration inverts the general clause: the read offset
can never exceed the advertised *limit* (that is what the limit means), so
"read ≥ W/2 beyond the last advertised limit" can never become true and no
credit is ever re-granted — every stream stalls permanently at 256 KiB and every
connection at 1 MiB. The first clause under its other parse ("consumption has
advanced *the limit*") is correct and matches RFC 9000 §4.2 and quinn.
**Fix:** state the formula — the prospective limit is `bytes_read + WINDOW`;
emit MAX_STREAM_DATA/MAX_DATA when `prospective_limit − last_advertised ≥
WINDOW/2`, i.e. when the read offset has advanced ≥ WINDOW/2 since the last
advertisement. **Wire impact:** none.

**FID-9 — "Credit is the buffer commitment" bounds bytes but not reassembly
fragments; the draft's own anti-amplification argument applies against it.**
§10.6, §9.5, §11.3.
§10.6's supporting check is explicitly scoped to "no **non-stream,
non-datagram** frame can force unbounded buffering", leaving the stream case
resting on the byte credit alone. Within 1 MiB of connection credit an
authenticated peer can send ~500 000 one-byte STREAM frames at alternating
offsets; each becomes a reassembler entry with per-entry allocation and heap
overhead, so the receiver's real memory can exceed the advertised commitment by
one to two orders of magnitude. This is precisely the argument §11.3 uses to
*reject* byte-bounded datagram queues ("a byte bound admits allocation-churn
amplification from a tiny-datagram flood: 1.25 MB of one-byte datagrams is
1.25 million queue entries") — the draft applies the reasoning to the unreliable
path and not to the reliable one. quinn bounds it on both axes: defragment when
`over_allocation > max(32768, buffered·3/2)`, and `TooManyChunks` (a transport
error) above 1024 chunks after defragmentation
(`connection/assembler.rs:196-218`).
**Fix:** add a normative fragment bound to §10.6/§9.5 — a coalescing/defragment
rule plus a per-stream chunk ceiling whose breach is `PROTOCOL_VIOLATION`, or
restate the commitment as "credit + a bounded fragment allowance".
**Wire impact:** none unless a new error code is chosen (0x01 suffices).

**FID-10 — A structurally invalid frame stream from an *authenticated* peer is a
silent drop, which livelocks; and the stated rationale is false post-AEAD.**
§8.2, §8.4, §12.1, §15.3.
§8.2's structural class fails "the whole packet: dropped, one trace, **nothing
applied** (no ACK scheduling …)", justified as "unknown means corruption or
skew". After the AEAD tag verifies, corruption is excluded (2⁻¹²⁸) and skew is
excluded by design (one version, no negotiation) — the only remaining causes are
a peer bug or a deliberate violation, i.e. exactly the population §8.2's
*semantic* class sends CLOSE to. Because nothing is applied and no ACK is
scheduled, a peer emitting a malformed frame (unknown type, over-cap
`range_count`, a range descending below zero, `max > 2⁶⁰`, a non-final
extends-to-end frame) retransmits it forever on PTO; both sides' liveness clocks
stay fresh (the packets *are* received and window-marked), so nothing ever
terminates — an unbounded livelock with no error code and no operator signal
beyond a trace. RFC 9000 §12.4 makes an unknown frame type a connection error of
type FRAME_ENCODING_ERROR; §19.3.1 does the same for a negative computed packet
number; §19.11 for `max > 2⁶⁰`.
**Fix:** move the structural class to a signalled death — CLOSE with
`PROTOCOL_VIOLATION` (0x01), or allocate `FRAME_ENCODING_ERROR` from the
reserved 0x06–0x0f range. Keep the silent drop only for packets that fail *before*
the AEAD (§3.1's length/type/version gate), where corruption is genuinely
possible. **Wire impact:** a new error code consumes one reserved registry value
(§15.3); no layout or length moves.

**FID-11 — The §6.4 [OPEN] is resolved the wrong way: it makes
accept-after-local-restart unreachable.** §6.4, §5.4 row 3, §18.1.
The draft keeps the architecture record's clause "a CONTINUATION = 1 candidate
for a static with no established connection is discarded like a guard failure"
and confines the fix to "the discard applies to re-home only". But after a local
restart *every* initiation from a state-retaining peer carries
`CONTINUATION = 1`, and the peer re-mints a fresh initiation every ~5 s, so any
accept decision slower than one retransmit interval — the exact case re-homing
exists for, and the human-in-the-loop case that motivates the whole staged
accept (Appendix A.1) — walks the candidate list, discards all four candidates,
and returns `AcceptError::Stale`. The application re-accepts on the next
`Intro`, is slow again, and gets `Stale` again: a livelock that only ends if the
operator happens to click inside a 5 s window. §5.4's responder row 3 already
gives the correct rule for this precise state ("CONTINUATION = 1 · none →
process as a fresh accept through the staged path … CONTINUED = 0"), and
re-homing *is* the staged path; the discard clause contradicts it.
**Fix:** delete the candidate-flag discard clause; a re-home candidate is
admitted on the other three conditions (same proven static, tail tag, guard) and
answered with `CONTINUED = 0` per §5.4 row 3 (and per FID-1's corrected rule).
**Wire impact:** none.

### MINOR

**FID-12 — §14.3 omits "no window growth during the recovery period".** §14.3,
§14.2. The draft ignores subsequent congestion *events* for packets sent before
the recovery period, but says nothing about *acknowledgments* of such packets,
so an implementation following §14.2 literally keeps growing cwnd (via ABC)
throughout recovery on the ACKs of the pre-cut flight. RFC 9002 §7.3.2 requires
the window to remain unchanged during recovery; quinn enforces it with the same
guard as for losses (`new_reno.rs:44-52`: `if app_limited || sent <=
recovery_start_time { return }`). **Fix:** add the symmetric sentence to §14.3.
No wire impact.

**FID-13 — §14.6's "conservative per RFC 9002/quinn precedent" mis-cites the
precedent for keeping the RTT estimator across a roam.** §13.1, §13.6, §14.6.
RFC 9000 §9.4 requires resetting *both* the congestion controller **and the
round-trip time estimator** on a confirmed migration; quinn's
`Connection::path_changed` → `PathData::reset` resets "the congestion
controller, round-trip estimator, and MTU discovery"
(`connection/mod.rs:1385-1397`, `connection/paths.rs:139-149`). Keeping RTT as a
*prior* is a defensible slither-specific choice (roaming is not adversarial path
migration here), but the citation is wrong and the consequences are unstated:
a roam onto a much slower path leaves `smoothed_rtt`/`min_rtt` from the old path,
producing premature PTOs and — compounding FID-7 — a persistent-congestion
period computed from a stale, tiny PTO. **Fix:** correct the citation, state the
consequence, and pair it with FID-6's exclusion rule. No wire impact.

**FID-14 — §10.1's "final consumed offsets" collides with §10.3's
"consumption".** §10.1, §10.3, §10.7. The connection limit is defined as "the
**sum over all streams** of final consumed offsets", but two sections later
"consumption" means *application reads*, and §10.7 gives the correct rule
("credit accounts the stream's offset high-water mark, not bytes on the wire").
RFC 9000 §4.1 defines the connection limit over the sum of the highest received
offsets per stream (final sizes for complete streams). Two senses of the same
word on the load-bearing accounting rule; an implementer taking §10.1 literally
computes the wrong number. **Fix:** rename to "the sum over all streams of the
highest received offset (the final size, once pinned)". No wire impact.

**FID-15 — §10.4's MAX_STREAMS grant is not scoped to peer-opened streams of the
matching space.** §10.4, §9.7. "The receiver grants +1 as it fully closes a
stream" — MAX_STREAMS_BIDI/UNI credit is a limit on what the *peer* may open, so
only closures of peer-opened streams in the corresponding space may generate
grants; as written, closing our own streams inflates the peer's allowance.
RFC 9000 §4.6. **Fix:** "as it fully closes a **peer-opened** stream of that
space". No wire impact.

**FID-16 — The receiver's consumption mode is not in fact "invisible on the
wire" and can stall a stream permanently.** §9.8, §10.3, §16.2. §9.8 says
whether a uni stream is consumed by `recv_message()` or `accept_uni()` is "the
receiving application's choice, invisible on the wire" — but §10.3 rules that
"sugar-consumed streams never earn stream-level credit". A sender that opens a
uni stream with `open_uni()` and writes more than `INITIAL_MAX_STREAM_DATA`
(entirely legal; only `send_message` is capped) against a receiver that consumes
uni streams as messages blocks at 256 KiB forever, with no error and no timer,
while the receiver waits for a FIN that can never arrive; the stalled bytes also
hold connection-level credit (see FID-4). The mode is therefore wire-visible in
effect. **Fix:** state the hazard normatively — either message-mode streams
still earn stream credit up to the point where the message bound is exceeded (at
which point the stream is surfaced as a stream, or reset), or the receiver
resets a message-mode stream that exceeds `MESSAGE_RECV_MAX`. No wire impact
(RESET_STREAM already exists).

**FID-17 — The §15.2 [OPEN] should resolve toward the extra variant.** §15.2,
§15.4, §18.1. `ConnectionLost::LocallyClosed` is currently the surface for both
"the application called `close()`/dropped the last handle" and "the peer
committed a protocol violation and we tore the connection down", which are
opposite causes with opposite operational responses; the teardown matrix's own
purpose (every cause mapped to a distinguishable observation) argues for
separating them, and the wire already carries the distinguishing code.
**Fix:** add `ConnectionLost::ProtocolViolation { code }` (or carry
`Option<code>` on `LocallyClosed`), and update §15.4's row and §18.1. No wire
impact (the CLOSE frame is unchanged).

**FID-18 — An idle-but-alive session never rekeys, and whether the keepalive is
a "payload seal" is undefined.** §7.5, §7.6, §7.7. §7.6 scopes `REKEY_AGE` and
`REJECT_AGE` to "the first *payload* seal (a fresh application send or a
retransmission — quiet control is age-exempt)"; the empty-plaintext keepalive is
sealed via `seal` (not quiet) but carries no payload, so it is unclear which
side of the consult it falls on. Under the natural reading (exempt), §7.5's "an
idle session sustained by the keepalive dance lives indefinitely" means the same
DH session keys live indefinitely — and the epoch ratchet cannot help, because
it advances by *message count* (§7.7) and an idle session has none. **Fix:**
state explicitly whether the keepalive consults `REKEY_AGE` (recommended: it
does, so a live-but-idle session re-handshakes every 120 s as WireGuard does).
No wire impact; the security reviewer owns the PFS half of this.

### NIT

**FID-19** (§12.5) — Ignoring an ACK whose `largest` exceeds the highest sealed
counter diverges from RFC 9000 §13.1 ("SHOULD treat … as a connection error of
type PROTOCOL_VIOLATION, if it is able to detect the condition"); the divergence
is deliberate and carried from v1 but is not labelled as one. State it.

**FID-20** (§13.4) — Probe content diverges from RFC 9002 §6.2.4 twice: the RFC
prefers *new* data before unacknowledged data, and recommends up to two probe
packets. The draft's "pending retransmittable frames oldest-first, else a bare
PING", one packet, is a deliberate simplification; label it.

**FID-21** (§11.3, §17.5) — 64 × 1169 = 74 816 B = **73.06 KiB** (74.8 kB), not
"≈ 75 KiB"; and 2 × that is ≈ 146 KiB, not "≈ 150 KiB". Either say "≈ 75 kB" or
"≈ 73 KiB".

**FID-22** (§12.4) — "an ack-eliciting packet whose counter is not exactly one
greater than the window's previous greatest" is undefined for the first packet
of a session (no previous greatest). Harmless (an immediate ACK), but state it.

**FID-23** (§16.2, §8.4) — `close(code, reason)` returns `()` and has no error
for `reason.len() > CLOSE_REASON_MAX`, while §8.4 makes a received
`reason_len > 256` a structural failure. Specify truncation at the handle.

**FID-24** (§15.2) — A closing endpoint answers "inbound packets from the peer"
with ≤ 1 CLOSE/s, including the peer's own CLOSE, so a mutual close produces a
bounded 1 Hz CLOSE ping-pong for 5 s. State that a CLOSE received while closing
moves to the draining (reply-free) behaviour.

**FID-25** (§2.2 vs §16.2/§16.4) — The endpoint is `Endpoint<C: Channel>` in
§2.2, `core::Endpoint<I: Identity>` in §16.4, and unparameterised in §16.2's
`impl Endpoint`. Also §2.3's formulas inline the literals `13`/`1` rather than
`MSG1_PAYLOAD_LEN`/`MSG2_PAYLOAD_LEN`. Cosmetic, but the size formulas are
test-pinned.

---

## C. Clean subsystems (checked, no findings)

- **hiss contract fidelity (§2.1, §7.1, §7.7, §7.9, §8.6, Appendix A).** Every
  row of the §2.1 table matches research-wire-reconciliation.md's citation set:
  counter hiss-owned/monotonic/transmitted verbatim, counter-as-nonce forcing
  cleartext, stateless recv ⇒ slither owns 100 % of replay post-AEAD,
  `REKEY_EPOCH_MSGS` protocol-fixed, `MAX_EPOCH_JUMP = 2` + commit-and-cap
  bounding forged counters upstream of the window, one-epoch straggler tolerance
  vs. a 2048 reordering budget (2048 ≪ 65 536 ✓), `MAX_MESSAGE_LEN` 65 535 vs.
  1186 ✓, `u64::MAX` reserved ⇒ usable `0..=2⁶⁴−2` ⇒ `NonceExhausted`,
  per-direction/per-session spaces. The three Appendix A dependencies are used
  consistently and are each genuinely required (split read for a *suspending*
  `Claimed`; `next_counter()` because the AD is the header, built pre-seal;
  `AsRef + Ord` for the canonical encoding of §2.4/§4.1/§6.7); the msg2 `[1]`
  payload is correctly recorded as a non-item. **No clause anywhere assumes a
  hiss behaviour research-gaps.md reports absent.**
- **Varint encoding (§8.1)** — RFC 9000 §16 verbatim, including
  non-minimal-encoding acceptance; header correctly excluded.
- **ACK range algebra and encoding (§8.4, §12.1)** — RFC 9000 §19.3/§19.3.1
  reproduced exactly; the raw-µs `ack_delay` divergence is justified (no
  negotiation to carry an exponent) and 25 000 µs fits the 4-byte varint.
- **STREAM frame bit packing (§8.4)** — `0x08 | OFF(0x04) | LEN(0x02) |
  FIN(0x01)`, ¬LEN ⇒ extends-to-end + must-be-final, ¬OFF ⇒ offset 0: RFC 9000
  §19.8 verbatim; the structural-error list is complete.
- **Stream-ID spaces (§9.1)** — RFC 9000 §2.1 / quinn `lib.rs:255-276` verbatim;
  the role-stability rule (parity fixed by the dialler or the tie-break winner,
  never changed by a rekey) is coherent with §6.7 and §7.8.
- **Final-size handling (§9.5, §9.6)** — FIN pinning, the three
  `FINAL_SIZE_ERROR` cases and the RESET_STREAM true-up match RFC 9000 §4.5.
- **RTT estimation (§13.1)** — RFC 9002 §5.3 exactly, including the
  `min_rtt`-floor condition on the `ack_delay` subtraction and the
  `K_INITIAL_RTT`/2 pre-sample variance.
- **Ack-based loss detection (§13.2)** — RFC 9002 §6.1.1/§6.1.2 exactly
  (threshold 3, `max(9/8·max(srtt, latest), 1 ms)`, min-across-in-flight timer).
- **PTO formula and backoff (§13.3)** — RFC 9002 §6.2.1 formula, ×2 per
  unanswered probe, `pto_count = 0` on any new acknowledgment (A.7); only the
  *arming* condition is missing (FID-5).
- **Retransmission classes (§8.7)** — ranges/regenerate/never matches RFC 9000
  §13.3 frame-by-frame, including "credit frames carry the freshest value".
- **Rekey survival matrix (§7.8) and the re-queue rule** — traced end to end: no
  counter-scoped quantity survives (the ACK-pending state and replay window
  reset together, so no old-space ACK can be sealed into the new space — the
  subtle one, and it is right), and no connection-scoped quantity resets. The
  make-before-break/instant-cut seam loses only in-flight packets, whose frames
  are re-queued from the dying sent map on each side independently.
- **DATAGRAM (§11)** — RFC 9221 §5.2/§5.3: ack-eliciting ✓, congestion-controlled
  ✓, never retransmitted ✓, flow-control-exempt ✓; the receive-oversize
  impossibility argument is sound given §8.2's structural class.
- **Liveness `seal`/`seal_quiet` split (§7.4, §7.5)** — the ruled property
  (partition kills at exactly `DEAD_TIMEOUT` after the last fresh send) survives
  the new quiet members; the every-2nd/25 ms ACK policy cannot suppress the
  keepalive dance because ACKs are quiet.
- **ACK policy coherence (§12.4 vs §12.2 truncation)** — the every-2nd + 25 ms +
  immediate-on-gap trio is RFC 9000 §13.2.1/§13.2.2-faithful, and I could not
  construct a starvation beyond the accepted bounded case: truncation only bites
  above 65 blocks in a 2048-counter window, the dropped ranges are the oldest,
  the resulting spurious retransmissions add *received* counters rather than
  gaps (self-limiting), each lost packet leaves the sent map once, and repeat
  cuts are absorbed by the recovery-period marker. The
  `ack_delay = 0`-when-largest-not-frame-seen rule and RFC 9002 §5.1's
  "largest newly acknowledged and at least one ack-eliciting" precondition
  compose correctly (an ACK whose `largest` names an untracked pure-ACK or
  keepalive yields no RTT sample, which is the RFC's own behaviour).
- **Teardown matrix (§15.4) vs. taxonomy (§18.1)** — all eight `ConnectionLost`
  variants are reachable and mapped; `EndpointDropped` is correctly excluded as a
  cause; the `PeerRestarted` zombie is correctly reaped by the peer's liveness
  and the subsequent re-dial correctly lands on §5.4 row 2. The only unmapped
  death is the livelock of FID-10 (which has no row because it has no death).
- **Consolidated constants table** — cross-checked against every inline use;
  every value agrees and nothing named inline is missing from the table.

---

## D. Needs analysis (riders on confirmed findings)

1. **FID-4 / FID-16 blast radius.** Both wedge on *absolute* limits, so the
   damage is cumulative and irreversible within a connection generation. Worth
   modelling in the Appendix B window-constants obligation: how much discarded
   data a realistic application produces before the 1 MiB budget is gone.
2. **FID-9 fragment ceiling.** The right constant is an implementation trade
   (quinn: defragment above `max(32 KiB, 1.5×buffered)`, hard-fail above 1024
   chunks); the Appendix B throughput check should measure defragmentation cost
   before a number is ratified.
3. **FID-6 interaction with a flapping NAT.** Because roaming needs only one
   authenticated packet from a new source, a dual-homed or fast-flapping peer
   resets the controller repeatedly; with FID-6 unfixed each flap also collapses
   the fresh window. Even fixed, per-flap `INITIAL_WINDOW` restarts deserve a
   stated position (a same-address-family port rebind is the case §14.6 already
   flags a reviewer might contest).

---

## E. Verdicts requested

**[OPEN] §6.4 (re-home candidate flags): the draft's chosen resolution is
wrong.** See FID-11. The discard clause should be **deleted**, not narrowed to
re-home: §5.4's responder row 3 already rules that a `CONTINUATION = 1`
initiation at a static with no live connection takes the staged path and is
answered `CONTINUED = 0`, and re-homing is part of that staged path. Narrowing
the clause to re-home preserves exactly the livelock the [OPEN] identifies.

**[OPEN] §15.2 (violation-close surface): add the variant.** See FID-17.
`LocallyClosed` for a peer-caused teardown is the one place the teardown matrix
loses information the wire already carries.

**The unmarked reconciliation (§13.6, roaming keeps the sent map): half right.**
Keeping the map is correct and better than the alternative — old-path ACKs must
still retire frames, and dropping the map would strand un-ACKed ranges outside
both the pending set and the tracker. What the reconciliation misses is the
other half of RFC 9000 §9.4: the kept packets must be fenced off from the *new*
path's controller, RTT estimator and persistent-congestion walk (FID-6). With
that fence added, "map kept, controller reset" is exactly quinn's model and the
`bytes_in_flight`-exceeds-fresh-cwnd stall the draft describes is genuinely
bounded by one loss/PTO cycle (the PTO exemption keeps the path probeable, and
the probe's ACK retires the old flight). Without it, every roam ends in
`MINIMUM_WINDOW`.

---

## F. Verdict

The draft is **faithful in its borrowed algorithms and arithmetic, and unfaithful
in its seams**. Everything that was transcribed from a source — RFC 9000 §16
varints, §19.3.1 ACK algebra, §19.8 STREAM bit packing, §2.1 stream IDs, §4.5
final sizes, RFC 9002 §5/§6.1/§6.2, RFC 9221's datagram contract, and every hiss
constraint in research-wire-reconciliation.md's citation table — is reproduced
correctly, and every size, window, rate and even the pinned ChaCha20 `Rekey()`
vector recomputes exactly; the packet byte counts (175/82, 197/108, 6/10/14,
1170/1169/1186) all check out, and the hiss contract is respected without a
single assumption of an absent behaviour. There is **no systemic issue** in the
borrowing. The failures are concentrated where two independently-correct
mechanisms meet and no one walked the join: the continuation/tie-break/flag
routing join (FID-1, FID-2 — which between them break ordinary simultaneous open
and can merge two transport-state generations, the exact bug the wire break was
spent to fix), the eager-GC/implicit-open join (FID-3 — which falsifies the
exactly-once message contract on a single lost ACK), the
discard/credit-advance join (FID-4 — which silently deadlocks a whole connection
after 1 MiB of unread data), and the roaming/recovery join (FID-6). Each is a
local, well-scoped edit; none moves a wire byte, and only FID-10's preferred fix
touches a constant (one reserved error-code value). Ratification should be held
until the four BLOCKERs and the seven MAJORs are ruled, with Appendix B's
continuation-flag matrix extended to cover the pending-only column that FID-1 and
FID-2 expose.

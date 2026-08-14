# Research: loss recovery + congestion control for slither v1

Scope: RFC 9002 (QUIC loss detection & congestion control), cross-checked
against `slither/src/recovery.rs` (the existing partial RFC 9002 implementation
— read as a *reference*, not a constraint), `hiss/src/noise/datagram.rs` (the
counter that must double as the QUIC packet number), and quinn-proto's
`congestion/` module (the real NewReno/CUBIC/BBR code, read from
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/`).
RFC section numbers below were cross-checked against the RFC editor text
(RFC 9002, RFC 9000 §§12–13, §19.3).

---

## 1. Loss detection (RFC 9002 §6) — what's implemented, what's missing

`slither/src/recovery.rs` already implements the send-side/receive-side split
RFC 9002 describes, over a **single** packet-number space (Leg 2 has no
Initial/Handshake spaces — the IK handshake, `HandshakeInit`/`HandshakeResp`,
uses its own independent retransmit timer, SPEC.md §5, not this machinery at
all). Point by point:

**RTT estimation (RFC 9002 §5).** `RttEstimator` (`recovery.rs:79-151`) keeps
`latest`, `smoothed`, `rttvar`, `min_rtt` and folds samples exactly per
§5.3: first sample seeds `smoothed = latest`, `rttvar = latest/2`, `min_rtt =
latest` (§5.3 "before any RTT samples..."); later samples subtract the
peer-reported `ack_delay` (capped at `MAX_ACK_DELAY`, §5.3's "an endpoint
SHOULD ignore the peer's `max_ack_delay` until..." rule reduced to a constant
cap) only when doing so would not push the sample below `min_rtt` (§5.3 exact
condition), then does the standard EWMA (`rttvar = 3/4·rttvar + 1/4·|smoothed
− adjusted|`, `smoothed = 7/8·smoothed + 1/8·adjusted`). `K_INITIAL_RTT` = 333
ms and the pre-sample `rttvar = K_INITIAL_RTT/2` match §6.2.2 exactly. This is
a faithful §5 implementation.

**Ack-based loss detection (RFC 9002 §6.1).** `detect_lost` (`recovery.rs:462-495`)
implements both sub-mechanisms as OR-conditions, matching §6.1 exactly:
- **Packet threshold (§6.1.1, `kPacketThreshold = 3`)**: `largest_acked − pn
  >= K_PACKET_THRESHOLD`. `K_PACKET_THRESHOLD = 3` matches the RFC default.
- **Time threshold (§6.1.2, `kTimeThreshold = 9/8`, `kGranularity = 1 ms`)**:
  `loss_delay = max(9/8 · max(smoothed_rtt, latest_rtt), kGranularity)`, exactly
  the formula in §6.1.2's second paragraph; survivors arm `loss_time` at
  `sent.time_sent + loss_delay`, taking the min across in-flight packets — the
  RFC 9002 Appendix A.7 (`DetectAndRemoveLostPackets`) pseudocode shape.

**PTO (RFC 9002 §6.2).** `pto_interval` (`recovery.rs:146-150`) is exactly the
§6.2.1 formula `smoothed_rtt + max(4·rttvar, kGranularity) + max_ack_delay`.
`pto_deadline` (`recovery.rs:505-512`) anchors on `time_last_ack_eliciting`
and applies `2^pto_count` backoff (§6.2, "the PTO period MUST be set to twice
its current value"), capped locally at `PTO_BACKOFF_CAP = 6` (a `slither`-only
overflow guard, not an RFC value — the RFC has no cap, relying on the
connection dying first). `on_pto` (`recovery.rs:522-529`) re-queues the oldest
outstanding message, or a bare PING if nothing is outstanding — matching
§6.2.4's guidance to send *something* ack-eliciting. `on_ack` resets
`pto_count = 0` whenever any packet is newly acknowledged
(`recovery.rs:451-453`), matching the RFC 9002 Appendix A.6 pseudocode
(`OnAckReceived` unconditionally zeroes `pto_count` once `newly_acked_packets`
is non-empty).

**What's genuinely missing for a "full RFC 9002" (beyond §7, covered in §3
below):**

1. **No ECN (§7.1, and RFC 9000 §13.4/§19.3.2).** `AckFrame`
   (`slither/src/frame.rs:163-173`) carries no ECT0/ECT1/CE counts — the wire
   format (SPEC.md §9.2, `AckHeader`) has no room for them at all. This is a
   wire change, not just a sender-side one, if ever added.
2. **No persistent-congestion signal in `detect_lost`.** RFC 9002 ties
   persistent congestion detection (§7.6) into the *same* loss-detection walk
   that `detect_lost` already performs — it's currently absent because there
   is no congestion controller to feed it. See §3 and §6 below: this needs
   `detect_lost` to additionally track "two ack-eliciting losses spanning
   `pto_interval() × kPersistentCongestionThreshold` with nothing acked
   between them," which the current loop structure (`recovery.rs:469-484`)
   does not compute.
3. **No multi-packet-number-space handling.** RFC 9002's general framework
   (§6.1's opening, §6.2's `pto_time_and_space` in implementations) runs
   Initial/Handshake/Application spaces independently, taking the earliest
   loss/PTO deadline across all three, and layers in anti-amplification
   limits during the handshake spaces. slither has exactly one space, by
   design (Leg 1's handshake is not ack-eliciting-tracked by `Recovery` at
   all) — this is a **deliberate and correct simplification** for a
   WireGuard-shaped, non-TLS handshake, not a gap, but worth stating
   explicitly so the redesign doesn't accidentally import multi-space
   plumbing it doesn't need.
4. **PTO probe content is "oldest outstanding," not "new data first."**
   RFC 9002 §6.2.4 recommends probes prefer unsent (new) data, then
   unacknowledged data, then a PING; `on_pto` (`recovery.rs:522-529`) always
   picks the lowest-sequence outstanding message regardless of whether it was
   ever transmitted. Minor; not congestion-relevant, noted for completeness.

## 2. Packet-number spaces and ACK design (RFC 9002 + RFC 9000 §12–13, §19.3)

**Why QUIC never retransmits the same packet number.** RFC 9000 §12.3
("Packet Numbers") requires packet numbers within a space to increase by at
least 1 per packet and never repeat; §13.3 ("Retransmission of Information")
is explicit that QUIC does not retransmit *packets* — a packet declared lost
has its **frames** resent inside a **new** packet under a **new** number. The
payoff (and the reason RFC 9002 can be as simple as it is) is that every
packet number unambiguously names exactly one send event forever — there is
no TCP-style Karn's-algorithm problem of "is this ACK for the original send or
the retransmission?" because there is never a second send under the same
number to confuse it with. RFC 9002 §5.1's RTT sampling and §6's loss
detection both lean on this: an ACK naming packet number `N` can only ever
mean "the packet sent at `N`," full stop.

**ACK design (RFC 9000 §13.2, §19.3).** §13.2 ("Generating Acknowledgments")
and §13.2.1–13.2.4 set the *when* (immediately for out-of-order/every-2nd
ack-eliciting packet by default, else within `max_ack_delay`); §19.3 defines
the wire frame: `Largest Acknowledged`, `ACK Delay`, `ACK Range Count`,
`First ACK Range`, then `ACK Range` pairs — §19.3.1 defines exactly the
descending gap/length block encoding (`gap` = unreceived-run − 1, `length` =
received-run − 1, block bounds derived from the previous block's smallest).
§19.3.2 adds optional ECN counts, present only on the ACK_ECN frame variant.

slither's `AckFrame`/`AckHeader` (`slither/src/frame.rs:94-129, 258-343`,
SPEC.md §9.2) is **byte-for-byte the same range algebra as §19.3.1**, with two
simplifications: fixed 2-byte `gap`/`length` fields instead of QUIC varints
(bounded safely by `MAX_ACK_RANGES = 63` over a 128-counter replay window, so
no field can ever need to express a run longer than 128 — SPEC.md §9.2), and a
plain 4-byte microsecond `ack_delay` instead of QUIC's scaled/exponent-encoded
varint (RFC 9000 §13.2.5) — a reasonable fixed-width simplification since
slither's ACK policy is immediate (SPEC.md §9.2 "ACK policy: immediate") and
delays are consistently small. No ECN counts exist on the wire at all.

**The load-bearing question: can hiss's counter serve directly as the QUIC
packet number?** Yes, and slither's Leg 2 already relies on this
(`recovery.rs:6-10`, SPEC.md §9 "the Leg 1 datagram `counter` ... **is** the
packet number"). The proof is in `hiss/src/noise/datagram.rs`:

- `DatagramSend::encrypt_next` (`datagram.rs:205-227`) owns the AEAD nonce
  counter internally (`self.cipher.nonce()`); the caller supplies no counter
  and cannot choose one — "the counter is owned by `hiss` and is strictly
  monotonic (`0, 1, 2, …`)... so it can never cause nonce reuse"
  (`datagram.rs:190-197`). This is *exactly* the RFC 9000 §12.3 invariant
  (monotonic, non-reusable, no way for the caller to force a duplicate).
- Every `encrypt_next` call burns the next counter — including non-payload
  sends (a bare ACK, a keepalive, a PTO PING) — matching QUIC's rule that
  *every* packet in a space consumes a fresh number, ack-eliciting or not
  (only ack-eliciting packets are *tracked* for loss/RTT, which
  `on_packet_sent`, `recovery.rs:379-390`, already gets right: pure-ACK
  packets are never inserted into `sent`).

**But there is one crucial divergence to design around: the counter space is
per session-epoch, not per connection.** QUIC's Application Data packet
number space is scoped to the *connection* and explicitly survives a TLS key
update (RFC 9001 §6) — PN monotonicity continues unbroken across a key phase
flip specifically so loss-detection state never needs resetting. slither's
rekey (`REKEY_AGE`, SPEC.md §6, "Rekey is send-triggered") is not a key-phase
flip — it is a **brand new Noise IK handshake with fresh ephemeral DH**,
producing a brand new `DatagramSend` whose counter restarts at 0
(`Transport::into_datagram*`, `datagram.rs:96-157`, has no notion of
continuing a prior counter). An ACK naming counter 5 is only meaningful
*within one epoch*: a stray ACK from the old session naming counter 5 after a
rekey would misidentify an unrelated packet in the new session. This is why
`recovery.rs`'s `epoch_reset` (`recovery.rs:261-273`) exists at all — it drops
the entire per-epoch `sent` map, `largest_acked`, `loss_time`,
`time_last_ack_eliciting`, `pto_count`, and ACK-schedule state, and re-queues
every undelivered message onto the new session, while explicitly preserving
`rtt` (a path property, `recovery.rs` struct comment `:198-206`) across the
swap. **This existing split (per-epoch send/ACK state vs. per-connection
message/RTT state) is exactly the seam the congestion-window state must also
respect** (see §5).

A second, easily-confused epoch concept: hiss's **key-ratchet epoch**
(`into_datagram_with_epoch`, `REKEY_EPOCH_MSGS = 65536`, SPEC.md "Key
ratchet") does *not* reset the counter — `datagram.rs:196` "the counter is
never reset" for the ratchet; only a full re-handshake (`epoch_reset()` on
the `Recovery` side) does. The redesign must keep these two "epoch" words
distinct: hiss's key epoch is invisible above `encrypt_next`'s return value
and needs zero recovery/CC handling; a `Recovery`-level epoch reset is a
rekey-by-rehandshake event.

**ACK ranges over the hiss counter space work unmodified.** Since the counter
never repeats within an epoch and is exactly what `encrypt_next` returned,
`AckFrame::from_window` (`frame.rs:298-343`) building ranges from the Leg 1
replay window's `(greatest, bitmap)` (`session.rs` `ReplayWindow`, `REPLAY_WINDOW
= 128`) is already a complete, correct ACK-over-packet-number-space
implementation — no adaptation needed for congestion control; CC only needs
one more field riding along per sent counter (its size in bytes — see §4).

## 3. Congestion control (RFC 9002 §7)

**Slow start (§7.3.1).** `cwnd` starts at `kInitialWindow = min(10 ×
max_datagram_size, max(2 × max_datagram_size, 14720))` (§7.2). For slither's
fixed `MAX_DATAGRAM = 1200` (SPEC.md §3), that's `min(12000, max(2400,
14720)) = 12000` bytes ≈ 10 packets. Grows by `bytes_acked` per ACK
(exponential, ~doubling per RTT) until `cwnd >= ssthresh`.

**Congestion avoidance (§7.3.3).** Past `ssthresh`, additive increase — one
MTU per RTT. quinn-proto's `NewReno::on_ack`
(`quinn-proto-0.11.16/src/congestion/new_reno.rs:44-83`) implements this with
integer "Appropriate Byte Counting" (accumulate `bytes_acked` until it exceeds
`window`, then add one `current_mtu`) rather than floating point — a pattern
worth reusing verbatim.

**Recovery period (§7.3.2).** On a congestion event, cut `cwnd` by
`kLossReductionFactor = 0.5`, set `ssthresh = cwnd`, and record
`recovery_start_time = now`; subsequent congestion events for packets *sent
before* `recovery_start_time` are ignored (`new_reno.rs:85-94`: `if sent <=
self.recovery_start_time { return }`) so one loss burst produces exactly one
window cut, not one per lost packet.

**Persistent congestion (§7.6).** If two ack-eliciting packets sent more than
`congestion_period = pto_interval() × kPersistentCongestionThreshold(3)` apart
are *both* lost with nothing acked between them, this is treated as
persistent congestion: `cwnd` collapses straight to `kMinimumWindow (= 2 ×
max_datagram_size)` rather than merely halving, and slow start effectively
restarts. quinn-proto's `detect_lost_packets`
(`quinn-proto-0.11.16/src/connection/mod.rs:1670-1730`) computes this inline
during the loss-detection walk, tracking `persistent_congestion_start` and
resetting it whenever an intervening packet is *not* lost or an ACKed packet
breaks the run (`mod.rs:1682-1727`). This is the one piece of §7 that must be
grafted into `Recovery::detect_lost`'s existing loop
(`recovery.rs:462-495`) rather than bolted on separately, because it needs the
same per-packet walk loss detection already performs.

**ECN (§7.1)** is deliberately out of scope for v1 (see §1) — no wire support,
no OS-level ECN plumbing.

**Pacing (§7.7).** RFC 9002 recommends spreading `cwnd` over an RTT rather
than bursting, suggested rate `N × cwnd / smoothed_rtt` with `N ≈ 1.25`
("slight headroom", §7.7). quinn-proto's `Pacer`
(`quinn-proto-0.11.16/src/connection/pacing.rs:15-152`) is a token bucket:
capacity `= window × 2ms / rtt` clamped to `[10, 256]` MTUs
(`pacing.rs:129-151`), refilling at the `1.25×` rate, gating sends via a
computed `Instant` to wait until. RFC 9002 §7.7 itself says pacing is a
SHOULD, not a MUST ("An implementation ... is not expected to be as
precise..."; senders that don't pace remain conformant). For slither, pacing
collides with the actor's coarse 250 ms `TICK` (`endpoint.rs:60`) — the loop
that evaluates loss/PTO deadlines and drives `pump()` only wakes once every
250 ms plus on packet arrival/send events; sub-RTT pacing needs sub-RTT
wakeups the actor doesn't currently have. **Recommendation: skip explicit
pacing in v1** — RFC-conformant, and the `cwnd` itself already bounds burst
size to roughly what quinn's own burst clamp (`MIN_BURST_SIZE=10` MTUs)
achieves for a 12000-byte initial window anyway. Revisit once/if the actor
gets a finer-grained event loop.

**Underutilizing the window (§7.8).** If the sender has no data to fill
`cwnd`, ACKs shouldn't grow `cwnd` past what was actually exercised (avoids
phantom growth while idle). `Recovery::next_packet` already knows precisely
when there is nothing to send (returns `None`, `recovery.rs:353-355`) — an
`app_limited` flag derived from "cwnd headroom existed but nothing was queued"
is a cheap, already-available signal, matching quinn-proto's
`Controller::on_ack`'s `app_limited` parameter (`congestion.rs:26-35`).

**CUBIC and BBR, briefly.** CUBIC (RFC 8312, quinn-proto's
`congestion/cubic.rs`) replaces the linear AIMD growth curve with a cubic
function of time-since-last-congestion-event (`w_cubic`, `w_est`,
`cubic.rs:41-53`), tuned for high-bandwidth-delay-product paths — same
`Controller` interface, three extra state variables (`k`, `w_max`,
`cwnd_inc`), floating-point cube-root math. BBR
(`congestion/bbr/mod.rs`, marked `"Experimental! Use at your own risk"` at
line 20) is model-based rather than loss-based: bandwidth+min-RTT sampling
with a windowed-max filter, a multi-mode state machine (`STARTUP`, `DRAIN`,
`PROBE_BW`, `PROBE_RTT`), and its own pacing-rate output — substantially more
state, a PRNG, and tuning surface (`bbr/mod.rs:26-58`).

**Recommendation: NewReno only for v1**, behind a small `Controller`-shaped
trait from day one, mirroring quinn-proto's design
(`congestion.rs:17-84`: `on_sent`, `on_ack`, `on_congestion_event`,
`on_mtu_update`, `window()`, `initial_window()`). Concretely, the trait needs
only the events `Recovery` already surfaces at its three existing hook points
— `on_packet_sent`, the newly-acked loop inside `on_ack`, and the lost-list
tail of `detect_lost` — so wiring NewReno in costs no new call sites, only new
calls at existing ones. `on_mtu_update` can be a no-op in v1 (`MAX_DATAGRAM`
is a fixed constant, SPEC.md §3 — no PMTU discovery). Making the controller
pluggable now costs nothing (NewReno is the only implementation) and avoids
re-touching the ACK/loss integration points later if CUBIC or BBR are ever
wanted.

## 4. The reliability engine over a sealed datagram transport

QUIC's model — track which packet number carried which stream byte-ranges;
on loss, re-frame the still-needed ranges into a **new** packet under a
**new** number — is already half-built in slither's Leg 2, just at
whole-message (not byte-range) granularity, because Leg 2 v1 has no
fragmentation (SPEC.md §9.3: "multi-packet messages ... are OUT, reserved for
the STREAM work"). The existing pieces:

- `SentPacket { time_sent, seqs: Vec<u64> }` in `sent: BTreeMap<u64,
  SentPacket>` (`recovery.rs:158-162, 210`) — counter → the DATA sequence
  numbers it carried. This *is* QUIC's sent-packet → frame-range map, at
  message granularity.
- `Outbound { bytes, transmitted }` in `outstanding: BTreeMap<u64, Outbound>`
  (`recovery.rs:165-172, 202`) — the retransmittable identity (QUIC's stream
  offset/length range, here a whole message).
- `to_send: BTreeSet<u64>` (`recovery.rs:204`) — sequences awaiting (first or
  re-) transmission; `detect_lost`'s lost-packet handling
  (`recovery.rs:485-494`) re-inserts a lost packet's `seqs` here, never
  touching `sent`'s counters — so a retransmission always lands on the next
  fresh counter via `pump()`'s `next_packet` → `session.seal`/`seal_quiet`
  path (`endpoint.rs:647-679`). This is already precisely "retransmit the
  frame, never the packet."

**What congestion control adds that isn't there yet:**

1. **In-flight byte size.** `SentPacket` needs a `size: usize` (the sealed
   plaintext or datagram length) alongside `seqs`, so `bytes_in_flight` can be
   computed as the sum over `sent`'s entries — incremented in
   `on_packet_sent` (`recovery.rs:379-390`), decremented wherever an entry is
   removed from `sent` (the newly-acked loop in `on_ack`,
   `recovery.rs:426-445`, and the lost-packet removal in `detect_lost`,
   `recovery.rs:485-494`).
2. **A send-side admission gate.** `pump()`'s drain loop
   (`endpoint.rs:656-676`, `while let Some(plan) = recovery.next_packet(...)`)
   currently drains `to_send` unconditionally, bounded only by
   `MAX_PLAINTEXT` per packet. Introducing `cwnd` means this loop needs a
   stopping condition — `bytes_in_flight + candidate_size <= cwnd` — checked
   before (or inside) `next_packet` pulls the next DATA frame. This is new
   plumbing: today `next_packet` has no concept of a byte budget beyond one
   packet's worth.
3. **Controller hookups at the three existing mutation points**: send
   (`on_packet_sent` → `controller.on_sent`), newly-acked
   (`on_ack`'s per-packet loop → `controller.on_ack(bytes, sent_time,
   app_limited)`), and loss (`detect_lost`'s lost-list tail →
   `controller.on_congestion_event(sent_time_of_largest_lost,
   is_persistent, lost_bytes)`, computed once per loss episode, not once per
   lost packet — matching quinn-proto's `mod.rs:1774-1782`, which only calls
   `on_congestion_event` once after the full lost-packet scan, gated on
   `lost_ack_eliciting`).

For the future STREAM-frame work (the reserved `0x04` space, SPEC.md §9.1),
this same shape generalizes: `seqs: Vec<u64>` becomes something like
`ranges: Vec<(stream_id, offset, length)>`, and loss re-frames only the
still-outstanding sub-ranges rather than a whole message — but that is out of
this track's scope; v1's whole-message `seqs` is sufficient for slither's
current DATA-frame-only wire.

## 5. Interaction with WireGuard-style timers and the Noise session

**TICK granularity (250 ms, `endpoint.rs:60`).** The actor's tick handler
(`endpoint.rs:947-991`) evaluates `recovery.loss_time_due`/`recovery.pto_due`
at most once per 250 ms, firing at most one of loss-detection or PTO per tick
(loss takes precedence, `endpoint.rs:955-960`, deliberately — "firing both in
one tick would escalate the PTO backoff spuriously"). This bounds
`K_GRANULARITY` in practice (`recovery.rs:52`, SPEC.md §9.6) and is fine for
RTT-scale timers (PTO, persistent-congestion detection — both hundreds of ms
to seconds). It is **not** fine for sub-RTT pacing, which is the concrete
reason §3 recommends deferring pacing rather than trying to fit RFC 9002 §7.7
into a 250 ms-granularity loop.

**Rekey resets the packet-number space — should it reset `cwnd`?** Per §2,
`REKEY_AGE`-triggered rekey is a full fresh Noise session (new DH, new
counter), handled via `Recovery::epoch_reset()`
(`recovery.rs:261-273`, called from `endpoint.rs:1052-1058`), which drops the
entire per-epoch state (`sent`, `largest_acked`, `loss_time`, `pto_count`,
ACK schedule) but preserves `rtt` (a path property). Congestion-window state
sits at exactly this seam and the redesign must decide which side it falls
on. **Recommendation: reset `cwnd`/`ssthresh`/`bytes_in_flight` on
`epoch_reset()`, do not carry them over** — for two reasons: (a) the old
epoch's in-flight `sent` entries are abandoned wholesale on reset (no ACK
will ever resolve them under the new session's counters), so any
`bytes_in_flight` accounting spanning the swap would be permanently
inconsistent; (b) unlike a QUIC key-phase flip (RFC 9001 §6, which explicitly
keeps congestion state because the path is unchanged), a slither rekey
performs fresh DH and may coincide with roaming, so there's no continuity
evidence that the old `cwnd` is still valid for whatever comes next. `rtt`
can still seed a reasonable *prior* for the new epoch's initial PTO since it
survives already — only the CC accounting resets. This mirrors how QUIC
implementations generally treat true path changes: quinn-proto constructs a
fresh `Controller` per `PathData`, not one that survives migration.

**Roaming is a different case and currently has no CC hook at all.** SPEC.md
§6 "Roaming": an authenticated, fresh, non-replayed packet from a new source
address moves the session's endpoint (`EndpointMoved`) **without** a rekey —
same hiss cipher states, same counter space, `epoch_reset()` is *not* called.
This is much closer to true QUIC connection migration (RFC 9000 §9) than to a
rekey. RFC 9002's general guidance (and quinn-proto's per-path controller
construction) is that congestion state should not be assumed valid across a
path change, since bandwidth/RTT/loss characteristics likely differ. Today's
roaming path (`session.rs`/`endpoint.rs` address bookkeeping) has zero
interaction with `Recovery` — a CC-aware redesign needs to add one: on
`EndpointMoved`, reset `cwnd`/`ssthresh` (and treat `rtt` as suspect, though
not necessarily discard it outright) while explicitly *not* calling the
counter-space-clearing `epoch_reset()`, since in-flight packets to the old
address may still resolve. This is new design surface, not a gap in existing
code — flagging it because it's easy to miss.

**PTO probes must bypass the cwnd gate.** RFC 9002 §6.2.4's entire point is
that a probe must go out even when the window is fully utilized and nothing
is arriving — otherwise a black-holed path with a full `cwnd` could never
recover (nothing would ever un-stick it). The admission gate proposed in §4
point 2 must exempt PTO-triggered sends (and, separately, pure-ACK/PADDING
sends, which were never `cwnd`-tracked to begin with since they aren't
ack-eliciting). This also interacts with slither's liveness design: PTO
probes and pure ACKs are already liveness-*neutral* (`seal_quiet`, SPEC.md
§9.5 — they don't reset `DEAD_TIMEOUT`'s clock) specifically so the 15 s dead
timer still fires correctly under a partition; that property is orthogonal to
and must be preserved alongside the new cwnd-bypass property — both rules
apply to the same probe send, for different reasons.

**Age gating (`REKEY_AGE`/`REJECT_AGE`) is orthogonal to CC.** The
2026/07/17 SPEC.md amendment scopes these to the payload-seal path
(`gate_payload`, `endpoint.rs:593-612`) — a session can be blocked from
sending because it's over-age *or* because `cwnd` is exhausted; these should
remain independently diagnosable, not conflated into one "can't send" state.

**hiss's key-ratchet epoch (`REKEY_EPOCH_MSGS`) has no CC interaction at
all** — it's an internal key-schedule detail invisible above
`encrypt_next`'s return value (§2); worth stating explicitly so the redesign
doesn't accidentally wire a cwnd reset to it by confusing it with the
`Recovery`-level `epoch_reset()`.

## 6. Minimal-viable recommendation for slither v1

**In v1:**
- RFC 9002 §5 RTT estimator — unchanged, already correct
  (`recovery.rs:79-151`).
- RFC 9002 §6.1/§6.2 loss detection + PTO — unchanged, already correct, with
  one new rule: PTO-triggered sends bypass the cwnd gate.
- A NewReno-shaped controller behind a small trait (`on_sent`, `on_ack`,
  `on_congestion_event`, `window()`) modeled directly on quinn-proto's
  `Controller` (`congestion.rs:17-84`) and `NewReno`
  (`new_reno.rs:43-134`): `kInitialWindow = min(10×1200, max(2×1200, 14720))
  = 12000` bytes, `kMinimumWindow = 2×1200 = 2400` bytes,
  `kLossReductionFactor = 0.5`, `ssthresh` starts at `u64::MAX`.
- Persistent congestion (RFC 9002 §7.6) grafted into `detect_lost`'s existing
  walk, using `congestion_period = pto_interval() × 3`
  (`kPersistentCongestionThreshold`); on detection, hard-reset `cwnd` to
  `kMinimumWindow` rather than merely halving.
- `bytes_in_flight` tracking: add `size: usize` to `SentPacket`; maintain the
  running sum across `on_packet_sent`/`on_ack`/`detect_lost`.
- A send-side admission gate in `pump()`'s drain loop
  (`endpoint.rs:656-676`), bypassed for PTO probes and non-ack-eliciting
  control frames.
- `cwnd`/`ssthresh`/`bytes_in_flight` reset to initial values on both
  `epoch_reset()` (rekey) and `EndpointMoved` (roaming) — the latter is new
  wiring, not present today. `rtt` continues to survive both, as it does now.
- **Zero wire-format changes.** cwnd/ssthresh/bytes-in-flight are pure
  sender-local state — unlike ECN, nothing here needs to appear on the wire,
  so v1 CC can land without touching the ratified SPEC.md §§1–9 bytes at all.
  (RFC 9002 §1 itself: QUIC's design does not mandate a specific congestion
  control algorithm — the wire is deliberately CC-agnostic, and slither's
  wire already is too.)

**Deferred, explicitly out of v1:**
- ECN (§7.1) — needs new wire fields (ACK ECN counts) and OS-level
  ECN/DSCP socket plumbing; real work, not a small addition.
- Pacing (§7.7) — RFC-optional (SHOULD, not MUST); the 250 ms `TICK` makes
  sub-RTT pacing not worth building until the actor's event loop gets finer
  granularity. `cwnd` alone already bounds burst size adequately for a
  ~12000-byte initial window.
- CUBIC/BBR — keep the controller behind the trait so either is a pure
  addition later; NewReno alone is the "minimal sound" choice for v1.
- Multiple packet-number spaces, MTU discovery/PMTUD, anti-amplification
  limits — not applicable to slither's single-space, fixed-1200-byte-datagram
  design.

---

### Summary

`recovery.rs` already implements RFC 9002 §5 (RTT estimation) and §6 (loss
detection, PTO) faithfully over a single packet-number space, with the
hiss `DatagramSend` counter (`hiss/src/noise/datagram.rs:190-227`) serving
directly as that packet number — its monotonic, caller-can't-choose-it nonce
is exactly RFC 9000 §12.3's invariant, and slither's "retransmit the frame on
a fresh counter" behavior already matches §13.3. The one subtlety is that the
counter space is scoped to a session *epoch*, not the whole connection —
rekey restarts it (`Recovery::epoch_reset`), roaming does not — and any new
congestion-window state must follow the same seam: reset on rekey and on
roaming, survive nothing else, while `rtt` (already epoch-independent)
carries forward as today. What's entirely missing is RFC 9002 §7: no cwnd, no
ssthresh, no recovery-period logic, no persistent congestion, no pacing. The
minimal sound v1 is a NewReno controller (quinn-proto's implementation is a
direct, ~100-line model to follow) with persistent-congestion detection
grafted into the existing loss-detection walk, `bytes_in_flight` added to the
existing sent-packet map, and a cwnd admission gate in `pump()`'s send loop
that explicitly exempts PTO probes — all achievable with zero wire-format
changes, deferring ECN, pacing, and CUBIC/BBR, and putting the controller
behind a small trait so those remain additive later rather than a rewrite.

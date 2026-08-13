//! Per-connection Leg 2 recovery: RFC 9002 loss detection and PTO over the
//! datagram counter, plus the reliable-message bookkeeping and ACK scheduling.
//!
//! One [`Recovery`] lives beside each [`Session`](crate::session::Session) in
//! the endpoint actor (a sibling, **not** a `Session` field — the ruled Leg 1
//! session stays untouched, and the reliable-message state must outlive a
//! rekey's session swap). The datagram `counter` **is** the RFC 9002 packet
//! number: unique and monotonic, so a retransmitted frame on a fresh counter
//! never suffers retransmission ambiguity (no Karn's problem). Frames are
//! retransmitted, never packets.
//!
//! A rekey (or a responder-side session replacement) starts a fresh counter
//! space, so [`epoch_reset`](Recovery::epoch_reset) drops the per-epoch state
//! (the sent-packet map, the ACK schedule) and re-queues every undelivered
//! message on the new session; the per-connection state (message sequence
//! numbers, the receiver's dedup record, the RTT estimate — a path property)
//! survives.
//!
//! All times are [`tokio::time::Instant`]s, so the paused-clock tests drive
//! every deadline in virtual time. **No congestion control** by ruling — no
//! cwnd, no pacing; loss detection drives retransmission only.
//!
//! # Ratified (Leg 2, 2026/07/17)
//!
//! Every constant here is frozen in `slither/SPEC.md` §9 (ratified
//! 2026/07/17). The RFC 9002 defaults are used wherever the RFC gives one.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use tokio::time::Instant;

use crate::frame::{AckFrame, DataFrame, DataFrameHeader, Frame, MAX_ACK_WIRE};
use crate::wire::MAX_PLAINTEXT;
use packtool::Packed;

// ── Constants (ratified; RFC 9002 defaults where it gives them) ───────────────

/// The packet-reordering threshold (RFC 9002 §6.1.1 `kPacketThreshold`): a
/// packet is lost once a packet this many counters newer has been ACKed.
pub const K_PACKET_THRESHOLD: u64 = 3;

/// The time-threshold numerator/denominator (RFC 9002 §6.1.2 `kTimeThreshold`
/// = 9/8): a packet is lost once older than `9/8 · max(smoothed_rtt,
/// latest_rtt)`.
pub const K_TIME_THRESHOLD_NUM: u32 = 9;
/// See [`K_TIME_THRESHOLD_NUM`].
pub const K_TIME_THRESHOLD_DEN: u32 = 8;

/// The timer granularity floor (RFC 9002 §6.1.2 `kGranularity`). Note the
/// actor evaluates deadlines on its 250 ms TICK, which bounds the effective
/// granularity — this floor only keeps the arithmetic from degenerating.
pub const K_GRANULARITY: Duration = Duration::from_millis(1);

/// The pre-sample RTT assumption (RFC 9002 §6.2.2 `kInitialRtt`): before any
/// RTT sample, `smoothed_rtt = 333 ms` and `rttvar = 166.5 ms`, making the
/// first PTO ≈ 1 s.
pub const K_INITIAL_RTT: Duration = Duration::from_millis(333);

/// The QUIC `max_ack_delay` analogue: the most an acknowledgement may be
/// deliberately delayed, budgeted into the PTO and capped off a reported
/// ack_delay before RTT adjustment. slither's v1 ACK policy is **immediate**
/// (an ACK is sent as soon as an ack-eliciting packet arrives, coalesced with
/// whatever else is pending), so the reported delays sit near zero and this
/// constant is a budget, not a target. 25 ms (the QUIC default).
pub const MAX_ACK_DELAY: Duration = Duration::from_millis(25);

/// The cap on the PTO exponential-backoff shift (2⁶ = 64× ≈ 66 s at the
/// initial PTO) — a local overflow guard; in practice the Leg 1
/// `DEAD_TIMEOUT` (15 s from the anchoring fresh send, which probes do not
/// refresh) ends the probe train long before the cap matters.
pub const PTO_BACKOFF_CAP: u32 = 6;

// ── RTT estimation (RFC 9002 §5) ──────────────────────────────────────────────

/// The RFC 9002 RTT estimator: `latest_rtt`, `smoothed_rtt`, `rttvar`, and
/// `min_rtt`, with the ack_delay adjustment of §5.3.
#[derive(Debug, Clone)]
pub(crate) struct RttEstimator {
    latest: Duration,
    smoothed: Option<Duration>,
    rttvar: Duration,
    min_rtt: Duration,
}

impl RttEstimator {
    fn new() -> Self {
        Self {
            latest: Duration::ZERO,
            smoothed: None,
            rttvar: Duration::ZERO,
            min_rtt: Duration::MAX,
        }
    }

    /// Fold in one sample (§5.3): the first sample seeds the estimator; later
    /// samples subtract the peer-reported `ack_delay` (capped at
    /// [`MAX_ACK_DELAY`]) when doing so would not drag the sample below
    /// `min_rtt`.
    pub(crate) fn sample(&mut self, latest: Duration, ack_delay: Duration) {
        self.latest = latest;
        match self.smoothed {
            None => {
                self.min_rtt = latest;
                self.smoothed = Some(latest);
                self.rttvar = latest / 2;
            }
            Some(smoothed) => {
                self.min_rtt = self.min_rtt.min(latest);
                let capped = ack_delay.min(MAX_ACK_DELAY);
                let adjusted = if latest >= self.min_rtt.saturating_add(capped) {
                    latest - capped
                } else {
                    latest
                };
                let var_sample = smoothed.abs_diff(adjusted);
                self.rttvar = self.rttvar * 3 / 4 + var_sample / 4;
                self.smoothed = Some(smoothed * 7 / 8 + adjusted / 8);
            }
        }
    }

    /// The smoothed RTT, or [`K_INITIAL_RTT`] before any sample.
    pub(crate) fn smoothed_rtt(&self) -> Duration {
        self.smoothed.unwrap_or(K_INITIAL_RTT)
    }

    /// The RTT variance, or `K_INITIAL_RTT / 2` before any sample (§6.2.2).
    pub(crate) fn rttvar(&self) -> Duration {
        if self.smoothed.is_some() {
            self.rttvar
        } else {
            K_INITIAL_RTT / 2
        }
    }

    /// The time-threshold loss delay (§6.1.2):
    /// `max(9/8 · max(smoothed_rtt, latest_rtt), kGranularity)`.
    fn loss_delay(&self) -> Duration {
        let base = self.smoothed_rtt().max(self.latest);
        (base.saturating_mul(K_TIME_THRESHOLD_NUM) / K_TIME_THRESHOLD_DEN).max(K_GRANULARITY)
    }

    /// The base probe timeout (§6.2.1):
    /// `smoothed_rtt + max(4 · rttvar, kGranularity) + max_ack_delay`.
    fn pto_interval(&self) -> Duration {
        self.smoothed_rtt()
            .saturating_add(self.rttvar().saturating_mul(4).max(K_GRANULARITY))
            .saturating_add(MAX_ACK_DELAY)
    }
}

// ── Sent-packet and message records ───────────────────────────────────────────

/// One ack-eliciting packet in flight: when it left and the DATA sequence
/// numbers it carried (empty for a pure PING probe — a PING is ack-eliciting
/// but carries nothing retransmittable).
#[derive(Debug, Clone)]
struct SentPacket {
    time_sent: Instant,
    seqs: Vec<u64>,
}

/// One undelivered outbound message.
#[derive(Debug, Clone)]
struct Outbound {
    bytes: Vec<u8>,
    /// Whether the message has ever been transmitted — the first transmission
    /// is fresh application traffic (it marks the Leg 1 liveness clock); a
    /// retransmission is control traffic (it does not).
    transmitted: bool,
}

/// One packet's worth of frames, planned by [`Recovery::next_packet`] and
/// sealed by the actor.
#[derive(Debug, Clone)]
pub(crate) struct PacketPlan {
    /// The encoded frame sequence — the plaintext to seal (≤ `MAX_PLAINTEXT`).
    pub plaintext: Vec<u8>,
    /// Whether the packet carries ack-eliciting frames (DATA or PING) — if so
    /// the actor registers it with [`Recovery::on_packet_sent`].
    pub ack_eliciting: bool,
    /// Whether the packet carries a first-transmission DATA frame — fresh
    /// application traffic, sealed with the liveness-marking `seal`; anything
    /// else (pure ACK, probe, retransmission) is sealed quietly.
    pub fresh: bool,
    /// The DATA sequence numbers aboard.
    pub seqs: Vec<u64>,
}

// ── Recovery ──────────────────────────────────────────────────────────────────

/// The per-connection Leg 2 state: reliable-message queues, RFC 9002 loss
/// detection and PTO on the send side, and ACK scheduling plus exactly-once
/// dedup on the receive side.
#[derive(Debug)]
pub(crate) struct Recovery {
    // ── Send side, per connection ──
    /// The next message sequence number to assign.
    next_seq: u64,
    /// Undelivered messages by sequence number.
    outstanding: BTreeMap<u64, Outbound>,
    /// Sequence numbers awaiting (first or re-) transmission.
    to_send: BTreeSet<u64>,
    /// The RTT estimate — a path property; survives a rekey.
    rtt: RttEstimator,

    // ── Send side, per session epoch ──
    /// Ack-eliciting packets in flight, by counter.
    sent: BTreeMap<u64, SentPacket>,
    /// The largest counter the peer has acknowledged.
    largest_acked: Option<u64>,
    /// The earliest instant at which an in-flight packet crosses the time
    /// threshold (the loss-detection timer).
    loss_time: Option<Instant>,
    /// When the newest ack-eliciting packet left (the PTO anchor).
    time_last_ack_eliciting: Option<Instant>,
    /// Consecutive unanswered PTOs (the exponential-backoff exponent).
    pto_count: u32,
    /// A PING probe is owed (a PTO fired with nothing retransmittable).
    ping_pending: bool,

    // ── Receive side, per session epoch ──
    /// An ACK is owed (an ack-eliciting packet arrived since the last ACK).
    ack_pending: bool,
    /// The largest received counter and when it arrived (the ack_delay basis).
    largest_recv_at: Option<(u64, Instant)>,

    // ── Receive side, per connection ──
    /// Every message sequence below this has been delivered.
    delivered_floor: u64,
    /// Delivered sequences at or above the floor (compacted as it advances).
    delivered_sparse: BTreeSet<u64>,
}

impl Recovery {
    /// Fresh per-connection state.
    pub(crate) fn new() -> Self {
        Self {
            next_seq: 0,
            outstanding: BTreeMap::new(),
            to_send: BTreeSet::new(),
            rtt: RttEstimator::new(),
            sent: BTreeMap::new(),
            largest_acked: None,
            loss_time: None,
            time_last_ack_eliciting: None,
            pto_count: 0,
            ping_pending: false,
            ack_pending: false,
            largest_recv_at: None,
            delivered_floor: 0,
            delivered_sparse: BTreeSet::new(),
        }
    }

    /// A rekey (or responder-side replacement) swapped the session underneath:
    /// the counter space restarted, so drop the per-epoch state and re-queue
    /// every undelivered message for the new session. Message sequences, the
    /// receiver dedup record, and the RTT estimate survive.
    pub(crate) fn epoch_reset(&mut self) {
        self.sent.clear();
        self.largest_acked = None;
        self.loss_time = None;
        self.time_last_ack_eliciting = None;
        self.pto_count = 0;
        self.ping_pending = false;
        self.ack_pending = false;
        self.largest_recv_at = None;
        for &seq in self.outstanding.keys() {
            self.to_send.insert(seq);
        }
    }

    // ── Send side ─────────────────────────────────────────────────────────────

    /// Queue one application message (≤ [`crate::frame::MAX_MESSAGE`] —
    /// enforced at the handle) for reliable delivery, returning its sequence.
    pub(crate) fn queue_message(&mut self, bytes: Vec<u8>) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.outstanding.insert(
            seq,
            Outbound {
                bytes,
                transmitted: false,
            },
        );
        self.to_send.insert(seq);
        seq
    }

    /// Plan the next outgoing packet: the owed ACK (built from the replay
    /// window snapshot), then queued DATA frames oldest-first while they fit,
    /// then an owed PING probe if nothing ack-eliciting got aboard. Returns
    /// `None` when nothing is pending. Call repeatedly to drain.
    pub(crate) fn next_packet(
        &mut self,
        now: Instant,
        window: Option<(u64, u128)>,
    ) -> Option<PacketPlan> {
        let mut frames: Vec<Frame> = Vec::new();
        let mut remaining = MAX_PLAINTEXT;
        let mut seqs = Vec::new();
        let mut ack_eliciting = false;
        let mut fresh = false;

        if self.ack_pending
            && let Some((greatest, bitmap)) = window
        {
            let ack = AckFrame::from_window(greatest, bitmap, self.ack_delay(greatest, now));
            debug_assert!(ack.wire_len() <= MAX_ACK_WIRE);
            remaining -= ack.wire_len();
            frames.push(Frame::Ack(ack));
            self.ack_pending = false;
        }

        while let Some(&seq) = self.to_send.iter().next() {
            let Some(out) = self.outstanding.get_mut(&seq) else {
                // Delivered while queued for retransmission; nothing to send.
                self.to_send.remove(&seq);
                continue;
            };
            let need = DataFrameHeader::SIZE + out.bytes.len();
            if need > remaining {
                break; // the next packet (a fresh plan) will carry it
            }
            self.to_send.remove(&seq);
            if !out.transmitted {
                out.transmitted = true;
                fresh = true;
            }
            remaining -= need;
            seqs.push(seq);
            frames.push(Frame::Data(DataFrame {
                seq,
                payload: out.bytes.clone(),
            }));
            ack_eliciting = true;
        }

        if self.ping_pending {
            if ack_eliciting {
                // The DATA aboard already elicits the ACK the probe wanted.
                self.ping_pending = false;
            } else if remaining >= 1 {
                self.ping_pending = false;
                frames.push(Frame::Ping);
                ack_eliciting = true;
            }
        }

        if frames.is_empty() {
            return None;
        }
        Some(PacketPlan {
            plaintext: crate::frame::encode_all(&frames),
            ack_eliciting,
            fresh,
            seqs,
        })
    }

    /// Whether a **DATA retransmission** is currently queued — a message still
    /// outstanding (unACKed) that loss detection or the PTO has re-queued for
    /// sending. This is the payload-vs-control discriminator the endpoint's
    /// retransmit path consults for the age gate: a pending DATA frame is
    /// application payload (subject to `REKEY_AGE`/`REJECT_AGE`), whereas a bare
    /// PTO PING probe or an owed ACK is control traffic and age-exempt.
    pub(crate) fn has_retransmittable(&self) -> bool {
        self.to_send
            .iter()
            .any(|seq| self.outstanding.contains_key(seq))
    }

    /// Register a sealed-and-sent plan under its packet `counter`. Pure-ACK
    /// packets are not tracked: they are not ack-eliciting, never retransmitted,
    /// and their loss is repaired by the next ACK re-reporting the window.
    pub(crate) fn on_packet_sent(&mut self, counter: u64, plan: &PacketPlan, now: Instant) {
        if plan.ack_eliciting {
            self.sent.insert(
                counter,
                SentPacket {
                    time_sent: now,
                    seqs: plan.seqs.clone(),
                },
            );
            self.time_last_ack_eliciting = Some(now);
        }
    }

    /// Process an incoming ACK frame (RFC 9002 §§5–6): mark newly-ACKed
    /// packets and their messages delivered, take an RTT sample if the frame's
    /// largest is newly acknowledged, reset the PTO backoff, and run loss
    /// detection. `highest_sent` bounds a well-formed peer's largest — an ACK
    /// for a counter this session never sealed is a protocol violation and is
    /// ignored whole.
    pub(crate) fn on_ack(&mut self, ack: &AckFrame, highest_sent: Option<u64>, now: Instant) {
        let Some(highest) = highest_sent else {
            tracing::debug!(target: "slither::frames", "ACK on a session that sealed nothing");
            return;
        };
        if ack.largest > highest {
            tracing::debug!(
                target: "slither::frames",
                largest = ack.largest,
                highest,
                "ACK acknowledges an unsent counter; frame ignored"
            );
            return;
        }
        // Intersect the ACK with our OWN outstanding set rather than
        // materialising every counter the ranges imply: a wire-legal 268-byte
        // ACK can imply millions of counters (63 ranges × 65 535), but only the
        // ack-eliciting packets we actually have in flight (`self.sent`, bounded
        // by the replay window) can be newly acknowledged. Iterate those at or
        // below `largest` and test membership in `O(range_count)` each.
        let acked: Vec<u64> = self
            .sent
            .range(..=ack.largest)
            .map(|(&pn, _)| pn)
            .filter(|&pn| ack.contains(pn))
            .collect();

        let mut newly_acked = false;
        for pn in acked {
            let Some(sent) = self.sent.remove(&pn) else {
                continue; // already removed above cannot happen, but be total
            };
            newly_acked = true;
            if pn == ack.largest {
                // The frame's largest is newly acknowledged and was
                // ack-eliciting (only ack-eliciting packets are tracked):
                // take the RTT sample (§5.1).
                let latest = now.saturating_duration_since(sent.time_sent);
                self.rtt.sample(
                    latest,
                    Duration::from_micros(u64::from(ack.ack_delay_micros)),
                );
            }
            for seq in sent.seqs {
                self.outstanding.remove(&seq);
                self.to_send.remove(&seq);
            }
        }

        self.largest_acked = Some(match self.largest_acked {
            Some(prev) => prev.max(ack.largest),
            None => ack.largest,
        });
        if newly_acked {
            self.pto_count = 0;
        }
        self.detect_lost(now);
    }

    /// RFC 9002 §6.1 loss detection: an in-flight packet is lost once a packet
    /// [`K_PACKET_THRESHOLD`] counters newer has been ACKed, or once it is
    /// older than the time threshold. Lost packets' messages are re-queued for
    /// retransmission (in a fresh packet, on a fresh counter); survivors below
    /// the largest ACKed arm [`loss_time`](Self::loss_time).
    pub(crate) fn detect_lost(&mut self, now: Instant) {
        self.loss_time = None;
        let Some(largest_acked) = self.largest_acked else {
            return;
        };
        let loss_delay = self.rtt.loss_delay();
        let mut lost = Vec::new();
        for (&pn, sent) in &self.sent {
            if pn > largest_acked {
                continue; // newer than anything ACKed: not yet judged
            }
            let by_count = largest_acked - pn >= K_PACKET_THRESHOLD;
            let by_time = now.saturating_duration_since(sent.time_sent) >= loss_delay;
            if by_count || by_time {
                lost.push(pn);
            } else {
                let due = sent.time_sent + loss_delay;
                self.loss_time = Some(match self.loss_time {
                    Some(at) => at.min(due),
                    None => due,
                });
            }
        }
        for pn in lost {
            if let Some(sent) = self.sent.remove(&pn) {
                tracing::debug!(target: "slither::frames", counter = pn, "packet lost");
                for seq in sent.seqs {
                    if self.outstanding.contains_key(&seq) {
                        self.to_send.insert(seq); // retransmit the FRAME, fresh counter
                    }
                }
            }
        }
    }

    /// Whether the loss-detection timer is due at `now`.
    pub(crate) fn loss_time_due(&self, now: Instant) -> bool {
        self.loss_time.is_some_and(|at| now >= at)
    }

    /// The probe-timeout deadline (RFC 9002 §6.2):
    /// `time_last_ack_eliciting + pto_interval · 2^pto_count`, armed only while
    /// ack-eliciting packets are in flight.
    pub(crate) fn pto_deadline(&self) -> Option<Instant> {
        if self.sent.is_empty() {
            return None;
        }
        let anchor = self.time_last_ack_eliciting?;
        let backoff = 1u32 << self.pto_count.min(PTO_BACKOFF_CAP);
        Some(anchor + self.rtt.pto_interval().saturating_mul(backoff))
    }

    /// Whether the PTO is due at `now`.
    pub(crate) fn pto_due(&self, now: Instant) -> bool {
        self.pto_deadline().is_some_and(|at| now >= at)
    }

    /// A PTO fired: double the backoff and queue a probe — the oldest
    /// undelivered message if there is one (a probe that carries data), else a
    /// PING. The probe send itself re-anchors the next deadline.
    pub(crate) fn on_pto(&mut self) {
        self.pto_count += 1;
        if let Some((&seq, _)) = self.outstanding.iter().next() {
            self.to_send.insert(seq);
        } else {
            self.ping_pending = true;
        }
    }

    // ── Receive side ──────────────────────────────────────────────────────────

    /// Note a fresh, authenticated, frame-bearing packet's arrival (the
    /// ack_delay basis for the largest).
    pub(crate) fn note_received(&mut self, counter: u64, now: Instant) {
        match self.largest_recv_at {
            Some((largest, _)) if largest >= counter => {}
            _ => self.largest_recv_at = Some((counter, now)),
        }
    }

    /// An ack-eliciting frame (DATA or PING) arrived: an ACK is owed. slither's
    /// v1 policy sends it immediately on the next
    /// [`next_packet`](Self::next_packet), coalesced with whatever is pending.
    pub(crate) fn mark_ack_pending(&mut self) {
        self.ack_pending = true;
    }

    /// The delay to report in an ACK whose largest is `greatest`: the time
    /// since that packet arrived, or zero if its arrival was not seen at this
    /// layer (a keepalive's counter, or a pre-epoch survivor).
    fn ack_delay(&self, greatest: u64, now: Instant) -> Duration {
        match self.largest_recv_at {
            Some((counter, at)) if counter == greatest => now.saturating_duration_since(at),
            _ => Duration::ZERO,
        }
    }

    /// Record a DATA frame's delivery attempt: `true` exactly once per
    /// sequence number (the first arrival — original or retransmission —
    /// surfaces; every later copy is ACKed again but not re-surfaced).
    pub(crate) fn deliver(&mut self, seq: u64) -> bool {
        if seq < self.delivered_floor || self.delivered_sparse.contains(&seq) {
            return false;
        }
        if seq == self.delivered_floor {
            self.delivered_floor += 1;
            while self.delivered_sparse.remove(&self.delivered_floor) {
                self.delivered_floor += 1;
            }
        } else {
            self.delivered_sparse.insert(seq);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time;

    fn plan_of(rec: &mut Recovery, now: Instant) -> PacketPlan {
        rec.next_packet(now, None).expect("a packet is pending")
    }

    /// Queue a message and register its (single-DATA) packet as sent on
    /// `counter` at `now`.
    fn send_one(rec: &mut Recovery, counter: u64, now: Instant) -> u64 {
        let seq = rec.queue_message(vec![0xAB; 8]);
        let plan = plan_of(rec, now);
        assert_eq!(plan.seqs, vec![seq]);
        rec.on_packet_sent(counter, &plan, now);
        seq
    }

    fn ack_of(largest: u64, first_range: u16, delay: Duration) -> AckFrame {
        AckFrame {
            largest,
            ack_delay_micros: delay.as_micros() as u32,
            first_range,
            ranges: Vec::new(),
        }
    }

    /// [`Recovery::has_retransmittable`] is the endpoint's payload-vs-control
    /// discriminator for the retransmit-path age gate (2026/07/17): a queued
    /// DATA retransmission is payload; an owed ACK or a bare PTO PING is not.
    #[tokio::test(start_paused = true)]
    async fn has_retransmittable_discriminates_payload_from_control() {
        let mut rec = Recovery::new();
        let now = Instant::now();

        // Nothing queued: no payload.
        assert!(
            !rec.has_retransmittable(),
            "an empty recovery has no payload"
        );

        // An owed ACK alone is control, not payload.
        rec.mark_ack_pending();
        assert!(
            !rec.has_retransmittable(),
            "a bare ACK is age-exempt control"
        );

        // A PTO probe with nothing outstanding queues a PING — still control.
        rec.on_pto();
        assert!(
            !rec.has_retransmittable(),
            "a bare PTO PING probe carries no payload"
        );

        // Send a DATA (drained into a packet), then a PTO re-queues it: now
        // there is retransmittable payload and the age gate must engage.
        send_one(&mut rec, 0, now);
        assert!(
            !rec.has_retransmittable(),
            "a freshly-sent, drained DATA is not queued for retransmit"
        );
        rec.on_pto();
        assert!(
            rec.has_retransmittable(),
            "a PTO-requeued unACKed DATA is retransmittable payload"
        );

        // Once ACKed, the queued seq is stale and no longer counts (the accessor
        // intersects to_send with the still-outstanding set).
        rec.on_ack(&ack_of(0, 0, Duration::ZERO), Some(0), now);
        assert!(
            !rec.has_retransmittable(),
            "an ACKed DATA clears from the retransmittable set"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn rtt_smoothing_in_range() {
        let mut rtt = RttEstimator::new();
        rtt.sample(Duration::from_millis(200), Duration::ZERO);
        assert_eq!(rtt.smoothed_rtt(), Duration::from_millis(200));
        assert_eq!(rtt.rttvar(), Duration::from_millis(100));
        // Steady samples converge and stay in range.
        for _ in 0..20 {
            rtt.sample(Duration::from_millis(200), Duration::ZERO);
        }
        assert_eq!(rtt.smoothed_rtt(), Duration::from_millis(200));
        assert!(rtt.rttvar() < Duration::from_millis(2), "variance decays");
        // The ack_delay adjustment: a 250 ms sample with a 30 ms reported delay
        // is adjusted by at most MAX_ACK_DELAY (25 ms) → 225 ms enters the mix.
        rtt.sample(Duration::from_millis(250), Duration::from_millis(30));
        let smoothed = rtt.smoothed_rtt();
        assert!(
            smoothed > Duration::from_millis(200) && smoothed < Duration::from_millis(210),
            "7/8 · 200 + 1/8 · 225 ≈ 203 ms, got {smoothed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn packet_threshold_is_exactly_three() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        let seq0 = send_one(&mut rec, 0, now);
        let seq1 = send_one(&mut rec, 1, now);
        let seq2 = send_one(&mut rec, 2, now);
        let _seq3 = send_one(&mut rec, 3, now);

        // The peer ACKs only counter 3 (0, 1, and 2 reordered or lost).
        rec.on_ack(&ack_of(3, 0, Duration::ZERO), Some(3), now);

        // Counter 0 is 3 behind the largest — lost by the packet threshold.
        assert!(rec.to_send.contains(&seq0), "0 is lost at the threshold");
        // Counters 1 and 2 are within the threshold — NOT spuriously lost.
        assert!(!rec.to_send.contains(&seq1), "1 is within the threshold");
        assert!(!rec.to_send.contains(&seq2), "2 is within the threshold");
        // They arm the time-threshold timer instead.
        assert!(rec.loss_time.is_some(), "survivors arm the loss timer");
    }

    #[tokio::test(start_paused = true)]
    async fn time_threshold_declares_stragglers() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        // Two packets leave together; the ACK for the newer arrives 100 ms
        // later, giving a 100 ms RTT sample — the loss delay is 9/8 · 100 =
        // 112.5 ms, so the older packet (also 100 ms in flight) is armed, not
        // yet lost.
        let seq0 = send_one(&mut rec, 0, now);
        let seq1 = send_one(&mut rec, 1, now);
        time::advance(Duration::from_millis(100)).await;
        let t1 = Instant::now();
        rec.on_ack(&ack_of(1, 0, Duration::ZERO), Some(1), t1);
        assert!(!rec.outstanding.contains_key(&seq1));
        assert!(!rec.to_send.contains(&seq0), "not yet past the threshold");
        let due = rec.loss_time.expect("armed");
        assert_eq!(due, now + Duration::from_micros(112_500));
        // Past the deadline the tick declares it.
        time::advance(Duration::from_millis(20)).await;
        let t2 = Instant::now();
        assert!(t2 >= due);
        assert!(rec.loss_time_due(t2));
        rec.detect_lost(t2);
        assert!(rec.to_send.contains(&seq0), "straggler lost by time");
        assert_eq!(rec.loss_time, None);
    }

    #[tokio::test(start_paused = true)]
    async fn pto_backoff_doubles() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        send_one(&mut rec, 0, now);
        // No RTT samples: interval = 333 + max(4·166.5, 1) + 25 = 1024 ms.
        let interval = rec.rtt.pto_interval();
        assert_eq!(interval, Duration::from_millis(1024));
        assert_eq!(rec.pto_deadline(), Some(now + interval));

        // First PTO: the probe (a retransmission) re-anchors; backoff ×2.
        time::advance(interval).await;
        let t1 = Instant::now();
        assert!(rec.pto_due(t1));
        rec.on_pto();
        let plan = plan_of(&mut rec, t1);
        assert!(!plan.fresh, "a probe retransmission is not fresh traffic");
        rec.on_packet_sent(1, &plan, t1);
        assert_eq!(rec.pto_deadline(), Some(t1 + interval * 2));

        // Second PTO: ×4.
        time::advance(interval * 2).await;
        let t2 = Instant::now();
        assert!(rec.pto_due(t2));
        rec.on_pto();
        let plan = plan_of(&mut rec, t2);
        rec.on_packet_sent(2, &plan, t2);
        assert_eq!(rec.pto_deadline(), Some(t2 + interval * 4));

        // An ACK resets the backoff.
        rec.on_ack(&ack_of(2, 0, Duration::ZERO), Some(2), t2);
        assert_eq!(rec.pto_count, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn pto_probe_is_ping_when_nothing_outstanding() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        // A bare PING packet in flight (e.g. an earlier probe), nothing queued.
        rec.ping_pending = true;
        let plan = plan_of(&mut rec, now);
        assert!(plan.ack_eliciting && plan.seqs.is_empty());
        rec.on_packet_sent(0, &plan, now);
        rec.on_pto();
        let plan = plan_of(&mut rec, now);
        assert!(plan.seqs.is_empty(), "the probe is a PING");
        assert_eq!(plan.plaintext, vec![crate::frame::FRAME_PING]);
    }

    #[tokio::test(start_paused = true)]
    async fn exactly_once_dedup() {
        let mut rec = Recovery::new();
        assert!(rec.deliver(0));
        assert!(!rec.deliver(0), "a duplicate does not re-surface");
        assert!(rec.deliver(2), "out of order is fine (unordered reliable)");
        assert!(rec.deliver(1));
        assert!(!rec.deliver(1));
        assert!(!rec.deliver(2));
        assert_eq!(rec.delivered_floor, 3, "the floor compacts");
        assert!(rec.delivered_sparse.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn epoch_reset_requeues_undelivered() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        let seq0 = send_one(&mut rec, 0, now);
        let seq1 = rec.queue_message(vec![0xCD; 4]); // queued, never sent
        rec.mark_ack_pending();
        rec.note_received(7, now);

        rec.epoch_reset();
        assert!(rec.sent.is_empty());
        assert_eq!(rec.largest_acked, None);
        assert_eq!(rec.pto_deadline(), None);
        assert!(!rec.ack_pending, "the old counter space owes no ACK");
        assert_eq!(rec.largest_recv_at, None);
        assert!(rec.to_send.contains(&seq0), "in-flight message re-queued");
        assert!(rec.to_send.contains(&seq1), "queued message survives");

        // The re-send on the new session is quiet (already-transmitted), and
        // the never-sent one is fresh — one plan carries both.
        let plan = plan_of(&mut rec, now);
        assert_eq!(plan.seqs, vec![seq0, seq1]);
        assert!(plan.fresh, "the never-transmitted message marks liveness");
    }

    #[tokio::test(start_paused = true)]
    async fn plans_coalesce_and_split_at_capacity() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        rec.mark_ack_pending();
        rec.note_received(5, now);
        let small = rec.queue_message(vec![1; 10]);
        let big = rec.queue_message(vec![2; crate::frame::MAX_MESSAGE]);
        let window = Some((5u64, 0b1u128));

        // Packet 1: the ACK coalesces with the small DATA; the max-size DATA
        // no longer fits behind them.
        let plan1 = rec.next_packet(now, window).expect("first plan");
        assert_eq!(plan1.seqs, vec![small]);
        assert!(plan1.plaintext.len() <= MAX_PLAINTEXT);
        assert_eq!(plan1.plaintext[0], crate::frame::FRAME_ACK);

        // Packet 2: the max-size DATA alone, filling the plaintext exactly.
        let plan2 = rec.next_packet(now, window).expect("second plan");
        assert_eq!(plan2.seqs, vec![big]);
        assert_eq!(plan2.plaintext.len(), MAX_PLAINTEXT);

        assert!(rec.next_packet(now, window).is_none(), "drained");
    }

    #[tokio::test(start_paused = true)]
    async fn ack_for_unsent_counter_is_ignored() {
        let mut rec = Recovery::new();
        let now = Instant::now();
        let seq0 = send_one(&mut rec, 0, now);
        rec.on_ack(&ack_of(9, 0, Duration::ZERO), Some(0), now);
        assert!(
            rec.outstanding.contains_key(&seq0),
            "an over-claiming ACK changes nothing"
        );
        assert_eq!(rec.largest_acked, None);
    }
}

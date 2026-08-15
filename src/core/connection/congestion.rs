//! §14 — congestion control: the controller seam and NewReno.
//!
//! Nothing congestion-related appears on the wire — §14.1: *"the wire is
//! deliberately CC-agnostic."* This module therefore has no codec, no
//! constants of its own, and no observable behaviour except through the
//! admission gate and the window it publishes.
//!
//! §14.7 is explicit about what is **out**: no pacing (a 12 KB initial
//! window bounds bursts adequately and the v1 shell has no sub-RTT
//! wakeups), no ECN, no CUBIC or BBR.

use std::time::Instant;

use crate::constants;

/// §14.1's seam, verbatim from the spec.
///
/// **`pub(crate)` deliberately (ruling 139(d))** — this is not an
/// oversight for slice 9's API review to correct. §14.1 defers pluggable
/// controllers to *"later"* and §19 intends to revisit the shape for CUBIC
/// and BBR; a `pub` trait in v0.2 would be a semver commitment to a shape
/// the spec already says will be reconsidered. Nothing outside the crate may
/// implement it in v1.
pub(crate) trait Controller {
    /// One packet sealed, `bytes` of datagram.
    fn on_sent(&mut self, now: Instant, bytes: u64);
    /// One newly acknowledged packet.
    fn on_ack(&mut self, now: Instant, sent_time: Instant, bytes: u64, app_limited: bool);
    /// One loss **episode** — §14.3: after the full lost-packet scan, never
    /// once per lost packet.
    fn on_congestion_event(
        &mut self,
        now: Instant,
        sent_time: Instant,
        is_persistent: bool,
        lost_bytes: u64,
    );
    /// The congestion window in bytes.
    fn window(&self) -> u64;
}

/// §14.2's NewReno — the one v1 implementation.
#[derive(Debug, Clone)]
pub(crate) struct NewReno {
    cwnd: u64,
    ssthresh: u64,
    /// §14.3's recovery-period marker. `None` = no recovery period has ever
    /// started, so nothing is fenced.
    recovery_start: Option<Instant>,
    /// §14.2's integer appropriate-byte-counting accumulator.
    acked_accum: u64,
}

impl Default for NewReno {
    fn default() -> Self {
        Self::new()
    }
}

impl NewReno {
    /// `cwnd = INITIAL_WINDOW`, `ssthresh = u64::MAX` (§14.2).
    pub(crate) fn new() -> Self {
        Self {
            cwnd: constants::INITIAL_WINDOW,
            ssthresh: u64::MAX,
            recovery_start: None,
            acked_accum: 0,
        }
    }

    /// §14.6's roam reset. **Uncalled until slice 7.**
    ///
    /// The controller resets to initial state on the one seam — a new path
    /// carries no continuity evidence — and **the recovery-period marker is
    /// set to the roam instant, not cleared**, which fences the pre-roam
    /// flight out of both §14.3's congestion event and §14.5's
    /// `app_limited` growth. It does **not** fence the RTT sample or the
    /// persistent-congestion walk: ruling 137 puts those on
    /// `SentPacket::path_gen`, because `recovery_start` is also set
    /// by every ordinary congestion event and reusing it would suppress RTT
    /// sampling after every normal loss episode.
    pub(crate) fn reset(&mut self, now: Instant) {
        self.cwnd = constants::INITIAL_WINDOW;
        self.ssthresh = u64::MAX;
        self.recovery_start = Some(now);
        self.acked_accum = 0;
    }

    /// §14.3's fence, used by both the event and the growth.
    ///
    /// *"the same `sent_time ≤ recovery_start` test gates both the event and
    /// the growth; without it, the pre-cut flight's ACKs keep inflating cwnd
    /// through the recovery they triggered."*
    fn in_recovery(&self, sent_time: Instant) -> bool {
        self.recovery_start.is_some_and(|start| sent_time <= start)
    }

    /// §14.2's slow-start threshold.
    #[cfg(test)]
    pub(crate) fn ssthresh(&self) -> u64 {
        self.ssthresh
    }

    /// §14.3's recovery-period marker.
    #[cfg(test)]
    pub(crate) fn recovery_start(&self) -> Option<Instant> {
        self.recovery_start
    }
}

impl Controller for NewReno {
    /// NewReno derives nothing from the send itself: §14.5's
    /// `bytes_in_flight` is the sent-packet map's sum (§13.5), not the
    /// controller's, so there is no second ledger to drift.
    fn on_sent(&mut self, now: Instant, bytes: u64) {
        let _ = (now, bytes);
    }

    fn on_ack(&mut self, now: Instant, sent_time: Instant, bytes: u64, app_limited: bool) {
        let _ = now;

        // §14.5: *"when it is set the controller does not grow the window on
        // that acknowledgment — idle connections earn no phantom window."*
        if app_limited {
            return;
        }
        // §14.3's symmetric half (RFC 9002 §7.3.2): acknowledgments of
        // packets sent before the recovery period started do not grow the
        // window either.
        if self.in_recovery(sent_time) {
            return;
        }

        if self.cwnd < self.ssthresh {
            // Slow start: grow by the bytes newly acknowledged.
            self.cwnd = self.cwnd.saturating_add(bytes);
            return;
        }

        // Congestion avoidance: integer appropriate byte counting, one
        // `MAX_DATAGRAM` per accumulator crossing, remainder carried.
        //
        // `while`, not `if`: a single large ACK burst may cross the
        // accumulator more than once, and an `if` silently under-grows.
        self.acked_accum = self.acked_accum.saturating_add(bytes);
        while self.acked_accum >= self.cwnd {
            self.acked_accum -= self.cwnd;
            self.cwnd = self.cwnd.saturating_add(constants::MAX_DATAGRAM as u64);
        }
    }

    fn on_congestion_event(
        &mut self,
        now: Instant,
        sent_time: Instant,
        is_persistent: bool,
        lost_bytes: u64,
    ) {
        let _ = lost_bytes;

        // §14.3: *"Subsequent congestion events for packets sent before the
        // recovery period started are ignored — one loss burst produces
        // exactly one window cut."*
        if self.in_recovery(sent_time) {
            return;
        }

        // `LOSS_REDUCTION_FACTOR` is an `f64` in `constants.rs` "to match
        // §14.2's notation, not to be multiplied by": the cut is an integer
        // halving, because a transport's window must not depend on float
        // rounding.
        self.cwnd = (self.cwnd / 2).max(constants::MINIMUM_WINDOW);
        self.ssthresh = self.cwnd;
        self.recovery_start = Some(now);

        if is_persistent {
            // §14.4: *"the controller collapses: `cwnd = MINIMUM_WINDOW`,
            // slow start effectively restarts"* — which `cwnd <  ssthresh`
            // already achieves.
            //
            // **[ruling 139(b)]** `recovery_start` is **not** cleared. RFC
            // 9002 §7.6.2 clears it; §14.4 asks only that slow start
            // restart, and §14.3's symmetric rule is stated in slither
            // without RFC 9002's carve-out. Leaving it set fences the
            // pre-collapse flight's acks out of the window, which is the
            // conservative direction.
            self.cwnd = constants::MINIMUM_WINDOW;
        }
    }

    fn window(&self) -> u64 {
        self.cwnd
    }
}

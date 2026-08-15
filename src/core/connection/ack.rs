//! §12 — the ACK: the delayed-ACK policy, and the derivation fused to
//! §7.2's replay window.
//!
//! # There is no second received-packet record
//!
//! §12.2 is explicit: *"An ACK is derived from the replay window's snapshot
//! (greatest + bitmap, §7.2) — the single received-packet record; there is
//! no second tracker."* [`derive`] therefore reads
//! [`ReplayWindow`](super::session::ReplayWindow) and nothing else.
//!
//! What [`AckState`] holds beside it is **four scalars**, not a record:
//! §12.3 needs the arrival instant of the packet bearing the window's
//! largest and whether that packet was frame-bearing; §12.4 needs a count
//! of ack-eliciting packets since the last ACK and an owed flag. None of
//! them scales with packets received, so §12.2's sentence is true as
//! written — but it reads as forbidding these, which is why the boundary is
//! stated here rather than left to be re-derived.
//!
//! # Only window-fresh packets fold in
//!
//! [`AckState::on_recv`] is called once per **authenticated, window-fresh**
//! packet. §7.2: *"No replayed packet ever moves the endpoint or refreshes
//! liveness."* A duplicate that advanced §12.4's every-2nd counter would buy
//! an attacker one ACK per replayed packet — free reverse-path
//! amplification, and invisible to every test that does not count ACKs.

use std::time::Instant;

use crate::constants;

use super::frame::Ack;
use super::session::ReplayWindow;

/// §12's ACK state — the scalars that live **alongside** the replay window,
/// never a second received-packet record (§12.2).
#[derive(Debug, Clone, Default)]
pub(crate) struct AckState {
    /// Ack-eliciting packets received since the last ACK we packed. §12.4's
    /// "every 2nd" counter. Reset to 0 when an ACK is packed.
    since_ack: u64,
    /// Arrival instant of the packet bearing the window's **current**
    /// greatest. `None` before the first authenticated, window-fresh
    /// receive.
    largest_at: Option<Instant>,
    /// Whether that packet was frame-bearing. §12.3: a keepalive's counter
    /// yields `ack_delay = 0`.
    largest_frame_seen: bool,
    /// An ACK is owed and not yet packed.
    owed: bool,
}

/// What receiving one authenticated, window-fresh packet does to §12.4's
/// policy. Returned so the caller arms or disarms `AckDelay` in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckAction {
    /// Nothing to do: this packet neither owes an ACK nor changes the
    /// delay deadline.
    ///
    /// **Not "nothing is armed".** A non-ack-eliciting packet arriving
    /// while `AckDelay` is already armed for an earlier one returns this,
    /// and that earlier arming must survive — §12.4 arms on *"the first
    /// unacknowledged ack-eliciting packet"*, and a keepalive is neither.
    None,
    /// An ACK is owed now (2nd ack-eliciting, or out-of-order arrival).
    Now,
    /// Arm `AckDelay` at this instant (`now + MAX_ACK_DELAY`).
    Arm(Instant),
}

impl AckState {
    /// Nothing received, nothing owed.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fold in one authenticated, **window-fresh** packet.
    ///
    /// `counter` is the packet's §7.1 counter; `prev_greatest` is the replay
    /// window's greatest **before** this packet was marked; `frame_seen` is
    /// false for §3.4's empty-plaintext keepalive; `ack_eliciting` is
    /// [`frame::packet_is_ack_eliciting`].
    ///
    /// `prev_greatest` **must** be read before the window marks the counter.
    /// Read afterwards it already includes this packet, §12.4's
    /// out-of-order test is always false, and the only §12.4 trigger left is
    /// the every-2nd counter — which no completion test can see.
    pub(crate) fn on_recv(
        &mut self,
        now: Instant,
        counter: u64,
        prev_greatest: Option<u64>,
        ack_eliciting: bool,
        frame_seen: bool,
    ) -> AckAction {
        // §12.3 measures from the arrival of the packet bearing the
        // window's largest, which need not be ack-eliciting: §7.5's
        // keepalive can be the greatest counter, and then §12.3 says the
        // reported delay is 0. That is the whole reason `frame_seen` is
        // carried rather than inferred.
        if prev_greatest.is_none_or(|greatest| counter > greatest) {
            self.largest_at = Some(now);
            self.largest_frame_seen = frame_seen;
        }

        if !ack_eliciting {
            // §12.4's triggers are both stated over ack-eliciting packets.
            // A packet that is not one owes no ACK and arms no timer — and
            // must not disarm one either.
            return AckAction::None;
        }

        self.since_ack += 1;

        // §12.4: *"an ack-eliciting packet whose counter is not exactly one
        // greater than the window's previous greatest"*. `None` — the first
        // ack-eliciting packet of a session — satisfies it vacuously, and
        // SPEC.md:3615–3618 calls the resulting immediate ACK harmless.
        let in_order =
            prev_greatest.is_some_and(|greatest| greatest.checked_add(1) == Some(counter));
        if !in_order || self.since_ack >= 2 {
            self.owed = true;
            return AckAction::Now;
        }

        AckAction::Arm(now + constants::MAX_ACK_DELAY)
    }

    /// `true` iff an ACK is owed. Does not clear.
    pub(crate) fn is_owed(&self) -> bool {
        self.owed
    }

    /// §12.4's `AckDelay` firing: the ACK becomes owed now.
    pub(crate) fn on_delay_expired(&mut self) {
        self.owed = true;
    }

    /// Called when an ACK has been packed into a packet: clears `owed` and
    /// resets `since_ack`. **The caller disarms `AckDelay`.**
    ///
    /// The counter resets because §12.4 arms on the first **unacknowledged**
    /// ack-eliciting packet, and packing an ACK makes every packet received
    /// so far acknowledged.
    pub(crate) fn on_ack_packed(&mut self) {
        self.owed = false;
        self.since_ack = 0;
    }

    /// §12.3's `ack_delay` field, in **microseconds**, saturating.
    ///
    /// `0` when the largest was not frame-seen, and `0` before any receive.
    pub(crate) fn ack_delay_us(&self, now: Instant) -> u64 {
        if !self.largest_frame_seen {
            return 0;
        }
        let Some(at) = self.largest_at else {
            return 0;
        };
        u64::try_from(now.saturating_duration_since(at).as_micros()).unwrap_or(u64::MAX)
    }
}

/// §12.2's derivation, newest-first descending, truncated at
/// `MAX_ACK_RANGES` **pairs** or at `room` plaintext bytes, whichever binds.
///
/// Returns `None` iff the window has no greatest (nothing received yet) — in
/// which case no ACK can be owed either — or if not even the first block
/// fits `room`, in which case the ACK **stays owed** for the next packet.
///
/// `room` is [`Packing::room()`](super::frame::Packing::room) at the moment
/// of the call, and the returned frame always encodes to `<= room` bytes.
///
/// # Why newest-first
///
/// §12.2: the 2048-bit alternating worst case no longer fits one packet, so
/// something must be dropped, and *"the dropped oldest ranges are exactly
/// the ones prior ACKs most likely already carried."* A build that truncated
/// oldest-first would satisfy any "at most 64 pairs" bound and lose the
/// ranges the peer still needs.
pub(crate) fn derive(window: &ReplayWindow, ack_delay_us: u64, room: usize) -> Option<Ack> {
    let mut blocks = window.ranges_desc();
    let first = blocks.next()?;
    let largest = *first.end();
    debug_assert_eq!(
        Some(largest),
        window.greatest(),
        "§12.2: the newest block's top is the window's greatest"
    );

    let mut ack = Ack {
        largest,
        ack_delay: ack_delay_us,
        first_range: largest - *first.start(),
        ranges: Vec::new(),
    };
    if ack.encoded_len() > room {
        return None;
    }

    let mut smallest = *first.start();
    for block in blocks {
        if ack.ranges.len() == constants::MAX_ACK_RANGES {
            break;
        }
        // §12.1: the block's largest is `prev_smallest − gap − 2`. The
        // window's ranges are separated by at least one missing counter, so
        // `block_largest <= smallest - 2` and neither subtraction underflows.
        let block_largest = *block.end();
        let gap = smallest - block_largest - 2;
        let range = block_largest - *block.start();

        ack.ranges.push((gap, range));
        if ack.encoded_len() > room {
            ack.ranges.pop();
            break;
        }
        smallest = *block.start();
    }

    debug_assert!(ack.encoded_len() <= room);
    debug_assert!(ack.ranges.len() <= constants::MAX_ACK_RANGES);
    Some(ack)
}

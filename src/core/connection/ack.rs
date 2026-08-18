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
//! What [`AckState`] holds beside it is **five scalars**, not a record:
//! §12.3 needs the arrival instant of the packet bearing the window's
//! largest and whether that packet was frame-bearing; §12.4 needs a count
//! of ack-eliciting packets since the last ACK, an owed flag, and — since
//! **[ruling 271]** — a pending flag. None of
//! them scales with packets received, so §12.2's sentence is true as
//! written — but it reads as forbidding these, which is why the boundary is
//! stated here rather than left to be re-derived.
//!
//! # The cadence is per **receive drain**, not per two packets
//!
//! **[RATIFIED 2026/08/18 — ruling 271]** §12.4's *"every 2nd ack-eliciting
//! packet"* is now the instant at which an ACK becomes **pending**, not the
//! instant it is emitted. It is emitted at the end of the driver's receive
//! drain, or on any packet built for another reason, or at
//! `MAX_ACK_DELAY` — whichever comes first.
//!
//! The measurement that forced it (`round42-G`, `round42-H`): slither was
//! emitting one ACK-only datagram per two data datagrams — **33.6 % of all
//! wire traffic**, against quinn's 1.68 % for the same workload — costing
//! ≈3.69 µs of a 14.11 µs per-data-datagram budget on both sides combined.
//! Nothing about the *frame* was wrong; the **emission point** was.
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
    /// "every 2nd" counter, and — **[ruling 271]** — also the counter
    /// [`ACK_COALESCE_MAX`](crate::constants::ACK_COALESCE_MAX) bounds. Reset
    /// to 0 when an ACK is packed.
    since_ack: u64,
    /// Arrival instant of the packet bearing the window's **current**
    /// greatest. `None` before the first authenticated, window-fresh
    /// receive.
    largest_at: Option<Instant>,
    /// Whether that packet was frame-bearing. §12.3: a keepalive's counter
    /// yields `ack_delay = 0`.
    largest_frame_seen: bool,
    /// An ACK is owed and not yet packed, and the pump must **build a packet
    /// for it** if nothing else is pending.
    owed: bool,
    /// **[ruling 271]** An ACK is due and **rides** the next outgoing packet
    /// (§12.4), but does not build one of its own — it waits for the end of
    /// the current receive drain, which reaches it as the `AckDelay` timer
    /// armed *due-immediately* by [`AckAction::Arm`].
    ///
    /// The two flags are not redundant. `owed` is what makes the pump
    /// manufacture a datagram; `pending` is what makes an ACK *coalesce*.
    /// Collapsing them restores the pre-271 one-ACK-per-two-packets cadence
    /// exactly.
    pending: bool,
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
    /// An ACK is owed now, and the pump builds a packet for it if nothing
    /// else is pending.
    ///
    /// **[ruling 271]** Out-of-order arrival, or
    /// [`ACK_COALESCE_MAX`](crate::constants::ACK_COALESCE_MAX)
    /// unacknowledged ack-eliciting packets — **no longer** the plain 2nd
    /// ack-eliciting packet, which now returns [`Arm`](Self::Arm) at `now`.
    Now,
    /// Arm `AckDelay` at this instant.
    ///
    /// Two instants reach this, and the second is the whole of ruling 271's
    /// change:
    ///
    /// * `now + MAX_ACK_DELAY` — §12.4's first unacknowledged ack-eliciting
    ///   packet, unchanged.
    /// * **`now` itself** — the coalescing arm. A deadline already due fires
    ///   the moment the driver has nothing else ready, which is exactly the
    ///   end of the receive drain; until then every further packet of the
    ///   burst folds into the same ACK.
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
    ///
    /// **[ruling 271] That hazard got worse, not better.** Before 271 the
    /// surviving every-2nd trigger still emitted an ACK inside the receive,
    /// so a `prev_greatest` read on the wrong side of the mark cost only the
    /// *promptness* of the gap ACK. It now costs the gap signal **entirely**:
    /// the every-2nd trigger defers to the drain boundary, so with the
    /// out-of-order test dead there is nothing left that reacts to
    /// reordering at all, and the peer's loss detection loses its input with
    /// nothing red anywhere.
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
        //
        // **[ruling 271]** This test is taken **first** and is deliberately
        // untouched: immediate-on-gap still emits inside the receive that saw
        // the gap, ahead of the drain boundary. It is the signal the peer's
        // loss detection reads, and coalescing it would be paying for
        // throughput with recovery latency on exactly the path that cannot
        // afford it.
        let in_order =
            prev_greatest.is_some_and(|greatest| greatest.checked_add(1) == Some(counter));
        if !in_order {
            self.owed = true;
            return AckAction::Now;
        }

        // **[ruling 271]** §12.4's every-2nd trigger no longer *emits*; it
        // *arms*, at `now`. The ACK is pending — it rides any packet the pump
        // builds for another reason — and the due-immediately timer flushes
        // it the instant the driver has nothing else ready, which is the end
        // of the receive drain. Every further packet of a burst folds into
        // the same ACK, because `pending` is a flag and [`derive`] reads the
        // whole replay window.
        //
        // The threshold keeps its ratified value and its ratified job: a
        // **single** isolated ack-eliciting packet still waits
        // `MAX_ACK_DELAY` for a carrier, which is what lets a low-rate
        // bidirectional flow piggyback rather than emit. This is also the
        // first use of `ACK_ELICITING_PER_ACK` — the threshold was a bare
        // literal `2` here since slice 5, and the constant it names went
        // unread.
        if self.since_ack >= constants::ACK_ELICITING_PER_ACK {
            // The safety valve. Without it the deferral is bounded only by
            // the sender exhausting its congestion window — self-limiting,
            // but by way of a sender stall rather than by a property, and a
            // scheduler that keeps this receiver's socket non-empty holds the
            // reverse path silent for as long as it does so.
            if self.since_ack >= constants::ACK_COALESCE_MAX {
                self.owed = true;
                return AckAction::Now;
            }
            self.pending = true;
            return AckAction::Arm(now);
        }

        AckAction::Arm(now + constants::MAX_ACK_DELAY)
    }

    /// `true` iff an ACK is owed **and the pump must build a packet for it**.
    /// Does not clear.
    ///
    /// **[ruling 271]** This is the narrow question, and every pre-271 caller
    /// keeps it: ruling 217's `PATH_CHALLENGE` offer rides a packet an owed
    /// ACK *creates*, ruling 203's refusal path emits a standalone ACK, and
    /// the pump's loop exit asks whether anything is left to build. A merely
    /// **pending** ACK answers `false` to all three — it is not, by itself, a
    /// reason to put a datagram on the wire.
    pub(crate) fn is_owed(&self) -> bool {
        self.owed
    }

    /// `true` iff an ACK should be **packed into a packet already being
    /// built** — §12.4's *"an owed ACK rides the next outgoing packet"*.
    ///
    /// **[ruling 271]** Strictly weaker than [`is_owed`](Self::is_owed): this
    /// is the question [`pack_ack`](super::Connection::pack_ack) asks, and it
    /// takes the pending ACK too. Piggybacking is the cheapest ACK there is,
    /// so a coalescing ACK must never miss a carrier that already exists.
    pub(crate) fn is_ready(&self) -> bool {
        self.owed || self.pending
    }

    /// §12.4's `AckDelay` firing: the ACK becomes owed now.
    ///
    /// **[ruling 271]** This is where a *pending* ACK is promoted, and the
    /// promotion needs no branch of its own: `owed` supersedes `pending`, and
    /// [`on_ack_packed`](Self::on_ack_packed) clears both.
    pub(crate) fn on_delay_expired(&mut self) {
        self.owed = true;
    }

    /// Called when an ACK has been packed into a packet: clears `owed`,
    /// **[ruling 271]** clears `pending`, and resets `since_ack`. **The
    /// caller disarms `AckDelay`.**
    ///
    /// All three, not two: a `pending` left set here would make the next
    /// packet built for any reason carry a redundant ACK for ever, and
    /// `ACK_COALESCE_MAX` would then be measured from the wrong origin.
    ///
    /// The counter resets because §12.4 arms on the first **unacknowledged**
    /// ack-eliciting packet, and packing an ACK makes every packet received
    /// so far acknowledged.
    pub(crate) fn on_ack_packed(&mut self) {
        self.owed = false;
        self.pending = false;
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

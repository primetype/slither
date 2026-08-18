//! §9.3 — the send half, collapsed as §9.3 invites.
//!
//! ```text
//! Ready ──write──▶ Send ──STREAM+FIN sent──▶ DataSent ──all ACKed──▶ DataRecvd
//!    │                │                          │
//!    └────────────────┴──────reset()────────────▶ ResetSent ──RESET ACKed──▶ ResetRecvd
//! ```
//!
//! The six states are exposition. This is `fin`/`fin_sent`/`fin_acked` plus
//! `reset`, with the two terminals represented by **removal** — the stream
//! table drops the half when [`SendHalf::is_terminal`] says so.
//!
//! # §8.7's two range sets, and why there are three
//!
//! §8.7's `ranges` class says the lost packet's stream ranges *"return to
//! the pending set and are re-framed on fresh counters"*. Slice 4 must build
//! the **state** those classes act on and must not build the detection
//! (slice 5's loss-recovery seam, SPEC §13), so [`on_ack_range`] and
//! [`on_lost_range`] exist here,
//! unit-tested, and are called from nowhere on the wire. Slice 5 wires §12
//! to them.
//!
//! The pending set is split in two — `fresh` and `retransmit` — because
//! **ruling 98** makes the seal path depend on which one a chunk came from:
//! *"STREAM **first transmission** → `seal` (marking); STREAM
//! **retransmission** → `seal_quiet`"*. One combined set would make the
//! distinction unrepresentable and the marking rule correct only by
//! accident, in a slice where every transmission happens to be a first one.
//!
//! [`on_ack_range`]: SendHalf::on_ack_range
//! [`on_lost_range`]: SendHalf::on_lost_range
//!
//! # The retention set never drains in slice 4
//!
//! With no ACK processing on the wire, `unacked` only grows. That is a
//! known, temporary condition closed by slice 5 — and **not** a licence to
//! free on send, which would be a collapsed implementation that makes slice
//! 5's tests pass for free.
//!
//! # The write buffer is a ring
//!
//! **[RATIFIED 2026/08/18 — ruling 269(iii)]** [`SendHalf::buf`] is a
//! [`VecDeque<u8>`], and [`SendHalf::release`] retires the ACKed prefix by
//! advancing the ring's head. It was a `Vec<u8>` with a
//! `drain(..acked_prefix)` on every acknowledged STREAM range — an O(live
//! bytes) `memmove` per incoming ACK, which round 42's profile measured at
//! **21 %** of the per-datagram CPU budget at the ratified 256 KiB window and
//! **94 %** at 8 MiB, where it collapsed single-stream throughput 12× down
//! the window ladder.
//!
//! **The bound this representation is chosen for.** A ring has **no slack**:
//! `buf.len()` is exactly the live byte count, `write_offset − base`, the
//! same identity the `Vec` had — [`SendHalf::release`] asserts it. Peak
//! allocation is therefore bounded by the peak live count under the same
//! geometric growth policy `Vec` already used, and the live count is bounded
//! by the stream's advertised credit because [`SendHalf::write`] accepts only
//! `max_data − write_offset`, capped again by the connection's own headroom.
//! **Ruling 94's discipline is that per-stream allocation is a first-class
//! budget**, and the alternative — keeping the `Vec` and carrying a dead
//! prefix behind an offset cursor — buys its O(1) amortisation by *spending*
//! that budget: compaction is only amortised O(1) if it triggers at a slack
//! proportional to the live size, which is precisely a multiple of the window
//! held in dead bytes (round 42's measurement mutant used 2×). The ring pays
//! nothing: no dead byte is ever retained and no byte is ever copied by a
//! release.
//!
//! Two consequences, both deliberate. A `VecDeque` cannot hand out one
//! contiguous slice, so [`SendHalf::copy_range`] rebuilds the retransmission
//! range out of the ring's two halves — at most two `memcpy`s, into the
//! `Vec` the caller was going to allocate anyway. And a front `drain` on a
//! `VecDeque` moves no element, which is the whole point: the cost of
//! releasing `n` bytes is O(1), not O(what is left).
//!
//! This is the shape the **receive** half already had: `recv.rs`'s reassembly
//! `Chunk` carries its own head and `advance()`s it rather than shifting its
//! tail down. The send half was the outlier.
//!
//! [`VecDeque<u8>`]: std::collections::VecDeque

use std::collections::VecDeque;
use std::ops::Range;

use crate::constants;
use crate::error::WriteError;

/// One STREAM frame's worth of stream data, taken from the fill loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// The stream offset of the first byte.
    pub(crate) offset: u64,
    /// The bytes.
    pub(crate) data: Vec<u8>,
    /// Whether this frame carries the FIN.
    pub(crate) fin: bool,
    /// **Ruling 98.** `true` on a first transmission — the only thing that
    /// makes a packet's seal marking.
    pub(crate) fresh: bool,
}

/// §9.6's sender-emitted reset, as it lives in the stream's own state.
///
/// Slice 6's message seam (SPEC §9.8): the **receiver**-emitted reset of
/// §9.8 is retained in a
/// connection-level regenerate set that outlives the stream state. These are
/// deliberately not one structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResetState {
    error_code: u64,
    final_size: u64,
    /// §8.7 `regenerate`: owed on the wire until acknowledged.
    pending: bool,
    acked: bool,
}

/// §9.3's send half.
pub(crate) struct SendHalf {
    /// Bytes accepted from the application and not yet released, starting at
    /// `base`.
    ///
    /// **A ring, and never a `Vec`** — see the module doc's *"The write
    /// buffer is a ring"*. `buf.len()` is exactly the live byte count
    /// `write_offset − base` (outside a reset, which clears the buffer and
    /// leaves both offsets where they stood), so the buffer holds no slack
    /// and retiring the ACKed prefix moves no byte.
    buf: VecDeque<u8>,
    base: u64,
    /// The next offset a write will occupy — and §10.1's per-stream
    /// contribution to the connection-level send sum.
    write_offset: u64,
    /// Never-transmitted ranges.
    fresh: RangeSet,
    /// Ranges returned by §13's loss detection: transmitted once already.
    retransmit: RangeSet,
    /// Transmitted, not yet ACKed — §8.7's retention.
    unacked: RangeSet,
    /// ACKed, for `DataRecvd`.
    acked: RangeSet,
    /// The peer's stream-level limit (§10.2's constant until MAX_STREAM_DATA
    /// raises it).
    max_data: u64,
    fin: bool,
    fin_sent: bool,
    fin_acked: bool,
    reset: Option<ResetState>,
    /// §9.8's **receiver**-emitted RESET_STREAM, arriving on a half we own.
    ///
    /// Separate from `reset`, and the difference is what is owed: ours is a
    /// frame we must regenerate until acknowledged, theirs is a frame that
    /// has already arrived and leaves us owing **nothing on the wire**. The
    /// code is the peer's, which is the one §18.1 surfaces as
    /// `WriteError::Reset`.
    ///
    /// Retained rather than freed, on the receive half's own terms: the
    /// half has to survive the reset long enough for the application to
    /// observe it, or `write()` answers `Finished` and the sender learns
    /// *"you called this after finishing"* for a stream the **peer**
    /// abandoned — §9.8's loud failure delivered as the wrong diagnosis.
    peer_reset: Option<u64>,
    /// Set when a write was refused for want of credit, cleared when the
    /// credit arrives. What makes `StreamWritable` precise instead of a
    /// broadcast.
    blocked: bool,
    /// Whether this half sits in the round-robin rotation (§8.5).
    queued: bool,
}

impl SendHalf {
    /// A fresh half with §10.2's un-negotiated stream window as the peer's
    /// limit.
    pub(crate) fn new() -> Self {
        Self {
            buf: VecDeque::new(),
            base: 0,
            write_offset: 0,
            fresh: RangeSet::default(),
            retransmit: RangeSet::default(),
            unacked: RangeSet::default(),
            acked: RangeSet::default(),
            max_data: constants::INITIAL_MAX_STREAM_DATA,
            fin: false,
            fin_sent: false,
            fin_acked: false,
            reset: None,
            peer_reset: None,
            blocked: false,
            queued: false,
        }
    }

    /// §10.1's per-stream contribution to the connection-level send sum.
    pub(crate) fn write_offset(&self) -> u64 {
        self.write_offset
    }

    /// The final size, once `finish()` or `reset()` has pinned one.
    pub(crate) fn final_size(&self) -> Option<u64> {
        if let Some(reset) = self.reset {
            return Some(reset.final_size);
        }
        self.fin.then_some(self.write_offset)
    }

    /// Whether a writer is parked on credit.
    pub(crate) fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Whether this half is in the round-robin rotation.
    pub(crate) fn is_queued(&self) -> bool {
        self.queued
    }

    /// Mark it in or out of the rotation.
    pub(crate) fn set_queued(&mut self, queued: bool) {
        self.queued = queued;
    }

    /// Whether the fill loop has anything to take.
    pub(crate) fn has_pending(&self) -> bool {
        if self.peer_reset.is_some() {
            // §9.6: *"pending and in-flight data for the stream stop being
            // retransmitted"*. Without this the cleared sets would still
            // leave `fin && !fin_sent` true and the fill would emit an
            // empty FIN frame for a stream the peer has already abandoned.
            return false;
        }
        self.has_data_pending() || (self.reset.is_none() && self.fin && !self.fin_sent)
    }

    /// Whether the fill loop has **bytes** to take, as opposed to a bare
    /// FIN. The distinction matters when the packet has no room: an empty
    /// FIN frame still fits where a data frame does not.
    pub(crate) fn has_data_pending(&self) -> bool {
        if self.reset.is_some() {
            return false;
        }
        !self.fresh.is_empty() || !self.retransmit.is_empty()
    }

    /// The offset the next chunk would start at — read before the packet's
    /// room is computed, because a STREAM frame's fixed fields depend on it.
    pub(crate) fn next_offset(&self) -> Option<u64> {
        if self.reset.is_some() {
            return None;
        }
        if let Some(r) = self.retransmit.first() {
            return Some(r.start);
        }
        if let Some(r) = self.fresh.first() {
            return Some(r.start);
        }
        (self.fin && !self.fin_sent).then_some(self.write_offset)
    }

    /// Whether a RESET_STREAM is owed on the wire (§8.7 `regenerate`).
    pub(crate) fn reset_pending(&self) -> bool {
        self.reset.is_some_and(|r| r.pending)
    }

    /// §9.7's terminals: `DataRecvd` or `ResetRecvd`.
    ///
    /// **Unreachable from the wire in slice 4** — reaching either needs ACK
    /// processing, which is slice 5. It is reachable from [`on_ack_range`]
    /// and [`on_reset_acked`], which is what makes the GC and watermark
    /// logic testable now.
    ///
    /// [`on_ack_range`]: SendHalf::on_ack_range
    /// [`on_reset_acked`]: SendHalf::on_reset_acked
    pub(crate) fn is_terminal(&self) -> bool {
        // §9.8's peer reset: nothing is owed and nothing further can be
        // acknowledged, so the half has reached its end — though nothing
        // *frees* it here, because only an observation can (see
        // [`peer_reset`](Self::peer_reset)).
        if self.peer_reset.is_some() {
            return true;
        }
        if let Some(reset) = self.reset {
            return reset.acked;
        }
        let Some(final_size) = self.final_size() else {
            return false;
        };
        self.fin_acked && self.acked.covers(0..final_size)
    }

    // ── application verbs ───────────────────────────────────────────────

    /// Buffer as much of `data` as both credit levels allow.
    ///
    /// `conn_room` is the connection-level headroom the caller read off the
    /// ledger, and it is the **only** bound on acceptance.
    ///
    /// **[RATIFIED 2026/08/16 — ruling 134]** §14's congestion window does
    /// **not** appear here, and this comment used to predict that it would.
    /// `write()` accepts bytes the window cannot yet send: the window defers
    /// the *seal*, never the acceptance, so `write()` stays a flow-control
    /// verb and §10.6's credit remains the buffer's whole bound. The
    /// alternative would have the shell's writer park on *window room*, a
    /// condition no `ConnEvent` announces — `StreamWritable` is
    /// credit-driven — so it would owe a new event, a new waker map and a
    /// new wakeup path that no section describes.
    pub(crate) fn write(&mut self, data: &[u8], conn_room: u64) -> Result<usize, WriteError> {
        // **Above** the finish/reset check: a stream the peer abandoned and
        // one the application finished are different facts, and §18.1 gives
        // the first its own variant precisely so the sender can tell them
        // apart. §9.8's whole purpose is that this answer names the hazard.
        if let Some(code) = self.peer_reset {
            return Err(WriteError::Reset(code));
        }
        if self.fin || self.reset.is_some() {
            return Err(WriteError::Finished);
        }
        if data.is_empty() {
            // An empty write is a **no-op**, not a block. `Ok(0)` otherwise
            // means "parked on credit", and a shell that could not tell the
            // two apart would park forever on its own empty buffer.
            return Ok(0);
        }
        let stream_room = self.max_data.saturating_sub(self.write_offset);
        let room = stream_room.min(conn_room).min(data.len() as u64) as usize;
        if room == 0 {
            self.blocked = true;
            return Ok(0);
        }
        self.blocked = false;
        let start = self.write_offset;
        // `VecDeque`'s `Extend<&u8>` is specialised to a `copy_slice` for
        // `Copy` elements, so this is the same single `memcpy` the `Vec`'s
        // `extend_from_slice` was.
        self.buf.extend(&data[..room]);
        self.write_offset += room as u64;
        self.fresh.insert(start..self.write_offset);
        Ok(room)
    }

    /// §9.3's `finish`: no more data; the FIN pins the final size.
    ///
    /// Idempotent — a second `finish()` is `Ok(())`, because it is a
    /// statement of intent that is already true. After a `reset()` it is
    /// `Finished`: the half is gone.
    pub(crate) fn finish(&mut self) -> Result<(), WriteError> {
        if let Some(code) = self.peer_reset {
            return Err(WriteError::Reset(code));
        }
        if self.reset.is_some() {
            return Err(WriteError::Finished);
        }
        self.fin = true;
        Ok(())
    }

    /// §9.6's `reset(error_code)`: abandon abruptly.
    ///
    /// §9.6: *"pending and in-flight data for the stream stop being
    /// retransmitted"*, and `final_size` is *"the end offset of the highest
    /// byte sent, or 0 if none"*.
    ///
    /// **The highest offset actually transmitted, in-flight included — not
    /// the buffered total.** Pending bytes were never put on the wire, so
    /// counting them pins a size the receiver can never reach; in-flight
    /// bytes may already have arrived, so *not* counting them would make
    /// legitimately-received data exceed the final size and trip
    /// `FINAL_SIZE_ERROR` on an honest peer. Only "highest offset sent" is
    /// consistent with both.
    pub(crate) fn reset(&mut self, error_code: u64) {
        if self.reset.is_some() {
            return;
        }
        let final_size = self.sent_high();
        self.reset = Some(ResetState {
            error_code,
            final_size,
            pending: true,
            acked: false,
        });
        self.fresh.clear();
        self.retransmit.clear();
        self.unacked.clear();
        self.buf = VecDeque::new();
        self.blocked = false;
    }

    /// §9.8's receiver-emitted RESET_STREAM, applied to this half.
    ///
    /// Returns whether it newly applied — a repeat, or one arriving after
    /// the application's own `reset()`, changes nothing and must not emit a
    /// second `ConnEvent::StreamReset`.
    ///
    /// Everything buffered is discarded: §9.6 stops pending and in-flight
    /// data from being retransmitted, and the peer that sent this frame has
    /// already freed the half that would have received it. **No frame is
    /// owed in reply** — unlike [`reset`](Self::reset), which owes a
    /// RESET_STREAM of our own until acknowledged.
    pub(crate) fn stopped_by_peer(&mut self, error_code: u64) -> bool {
        if self.reset.is_some() || self.peer_reset.is_some() {
            return false;
        }
        self.peer_reset = Some(error_code);
        self.fresh.clear();
        self.retransmit.clear();
        self.unacked.clear();
        self.buf = VecDeque::new();
        self.blocked = false;
        true
    }

    // ── the wire ────────────────────────────────────────────────────────

    /// Take one round-robin quantum's worth of stream data (§8.5).
    ///
    /// Retransmissions are served before fresh data: a peer waiting on a
    /// gap is waiting on those bytes and nothing else.
    pub(crate) fn next_chunk(&mut self, max_len: usize) -> Option<Chunk> {
        if self.reset.is_some() || self.peer_reset.is_some() {
            return None;
        }

        let (range, fresh) = match self.retransmit.first() {
            Some(r) => (r, false),
            None => match self.fresh.first() {
                Some(r) => (r, true),
                None => {
                    // Nothing but the FIN is left: §9.5's empty FIN-only
                    // frame is a valid end-of-stream marker.
                    if self.fin && !self.fin_sent {
                        self.fin_sent = true;
                        return Some(Chunk {
                            offset: self.write_offset,
                            data: Vec::new(),
                            fin: true,
                            fresh: true,
                        });
                    }
                    return None;
                }
            },
        };

        let take = (range.end - range.start).min(max_len as u64) as usize;
        if take == 0 {
            return None;
        }
        let start = range.start;
        let end = start + take as u64;
        let data = self.copy_range(start, end);

        if fresh {
            self.fresh.remove(start..end);
        } else {
            self.retransmit.remove(start..end);
        }
        self.unacked.insert(start..end);

        let carries_fin = self.fin
            && !self.fin_sent
            && Some(end) == self.final_size()
            && self.fresh.is_empty()
            && self.retransmit.is_empty();
        if carries_fin {
            self.fin_sent = true;
        }

        Some(Chunk {
            offset: start,
            data,
            fin: carries_fin,
            fresh,
        })
    }

    /// Put a chunk back in the set it came from, unsent.
    ///
    /// Distinct from [`on_lost_range`](SendHalf::on_lost_range): a chunk the
    /// packer refused was never transmitted, so returning it through the
    /// loss path would reclassify a first transmission as a retransmission
    /// and quiet a seal that ruling 98 makes marking.
    pub(crate) fn return_chunk(&mut self, range: Range<u64>, fin: bool, fresh: bool) {
        self.unacked.remove(range.clone());
        if fresh {
            self.fresh.insert(range);
        } else {
            self.retransmit.insert(range);
        }
        if fin {
            self.fin_sent = false;
        }
    }

    /// Take the owed RESET_STREAM (§8.7 `regenerate`, §9.6).
    pub(crate) fn take_reset(&mut self) -> Option<(u64, u64)> {
        let reset = self.reset.as_mut()?;
        if !reset.pending {
            return None;
        }
        reset.pending = false;
        Some((reset.error_code, reset.final_size))
    }

    /// §10.1's monotone-max on a received MAX_STREAM_DATA. `true` if it
    /// raised the limit **and** a blocked writer can now proceed.
    pub(crate) fn on_max_stream_data(&mut self, max: u64) -> bool {
        if max <= self.max_data {
            return false;
        }
        self.max_data = max;
        let unblocked = self.blocked;
        if unblocked {
            self.blocked = false;
        }
        unblocked
    }

    /// Clear the blocked flag when connection-level credit arrives.
    pub(crate) fn unblock(&mut self) -> bool {
        let was = self.blocked;
        self.blocked = false;
        was
    }

    /// The peer's stream-level limit.
    pub(crate) fn max_data(&self) -> u64 {
        self.max_data
    }

    // ── Slice 5's ACK seam (SPEC §12): defined here, driven there ───────

    /// §12's ACK application, for one acknowledged stream range.
    ///
    /// `fin` says the acknowledged frame carried the FIN. It is explicit
    /// rather than inferred from `range.end == final_size`: §8.7 lets a
    /// retransmission re-frame ranges freely, so a frame ending at the final
    /// size need not have carried the FIN, and slice 5's sent-packet map is
    /// where the answer actually lives.
    pub(crate) fn on_ack_range(&mut self, range: Range<u64>, fin: bool) {
        self.unacked.remove(range.clone());
        self.retransmit.remove(range.clone());
        self.acked.insert(range);
        if fin {
            self.fin_acked = true;
        }
        self.release();
    }

    /// §13's loss detection, for one lost stream range: it returns to the
    /// pending set and is re-framed on a fresh counter (§8.7 `ranges`).
    pub(crate) fn on_lost_range(&mut self, range: Range<u64>, fin: bool) {
        if self.reset.is_some() {
            return;
        }
        self.unacked.remove(range.clone());
        // Already-ACKed sub-ranges are not resent: §8.7's *"only still
        // un-ACKed sub-ranges are resent"*.
        let mut returning = RangeSet::default();
        returning.insert(range);
        for acked in self.acked.iter() {
            returning.remove(acked.clone());
        }
        for r in returning.iter() {
            self.retransmit.insert(r.clone());
        }
        if fin {
            self.fin_sent = false;
        }
    }

    /// §16.2's settled test for one snapshot offset: is every byte below
    /// `offset` acknowledged, **or abandoned by a reset**?
    ///
    /// §16.2 puts the reset arm in terms: *"or abandoned by a reset (§9.6:
    /// an abandoned byte is never acknowledged, and waiting on one would
    /// never terminate)"*. Without it `acked()` hangs for ever on a stream
    /// the application reset.
    ///
    /// `offset == 0` is settled vacuously — the stream was opened and never
    /// written, so nothing was handed to the connection.
    ///
    /// **The FIN is deliberately not part of this.** §16.2 scopes the
    /// connection-level snapshot to *"every byte handed to the
    /// connection"*; a snapshot that also waited for a FIN would never
    /// terminate on a stream the application intends to keep open.
    /// `SendStream::acked()` is the verb that includes the FIN.
    pub(crate) fn settled_to(&self, offset: u64) -> bool {
        // §9.8's peer reset joins §9.6's local one for the same stated
        // reason: *"an abandoned byte is never acknowledged, and waiting on
        // one would never terminate"*. It abandons the bytes whichever end
        // asked for it.
        self.reset.is_some()
            || self.peer_reset.is_some()
            || offset == 0
            || self.acked.covers(0..offset)
    }

    /// §9.6's RESET_STREAM acknowledged — `ResetRecvd`.
    pub(crate) fn on_reset_acked(&mut self) {
        if let Some(reset) = self.reset.as_mut() {
            reset.acked = true;
            reset.pending = false;
        }
    }

    /// A lost RESET_STREAM re-queues its identity (§8.7 `regenerate`).
    pub(crate) fn on_reset_lost(&mut self) {
        if let Some(reset) = self.reset.as_mut().filter(|r| !r.acked) {
            reset.pending = true;
        }
    }

    // ── internals ───────────────────────────────────────────────────────

    /// The end offset of the highest byte ever transmitted — §9.6's
    /// `final_size`.
    ///
    /// Derived from the two sets rather than stored, so a chunk the packer
    /// refused (and [`return_chunk`](SendHalf::return_chunk) put back)
    /// cannot leave a phantom high-water mark behind.
    fn sent_high(&self) -> u64 {
        self.unacked
            .last_end()
            .max(self.acked.last_end())
            .unwrap_or(0)
    }

    /// The stream bytes `[start, end)`, copied out of the ring.
    ///
    /// The `Vec` this returns is the one [`next_chunk`](SendHalf::next_chunk)
    /// was going to allocate anyway — the `Vec` buffer's `slice(..).to_vec()`
    /// allocated it too. A ring cannot hand out one contiguous slice, so the
    /// copy is made from the two halves `as_slices` reports rather than from
    /// one: two `memcpy`s at worst, one whenever the range does not straddle
    /// the wrap.
    ///
    /// Out-of-range indices panic, exactly as the `Vec`'s `&buf[lo..hi]` did:
    /// a caller asking for bytes outside `[base, write_offset)` has a defect
    /// in its range sets, and the loud failure is the point.
    fn copy_range(&self, start: u64, end: u64) -> Vec<u8> {
        let lo = (start - self.base) as usize;
        let hi = (end - self.base) as usize;
        let (front, back) = self.buf.as_slices();
        let mut out = Vec::with_capacity(hi - lo);
        if lo < front.len() {
            out.extend_from_slice(&front[lo..hi.min(front.len())]);
        }
        if hi > front.len() {
            out.extend_from_slice(&back[lo.saturating_sub(front.len())..hi - front.len()]);
        }
        out
    }

    /// Release the ACKed contiguous prefix of the write buffer.
    ///
    /// **[ruling 269(iii)]** O(1) in the bytes released and O(1) in the bytes
    /// left: a front `drain` on a `VecDeque` advances the ring's head and
    /// moves no element. The `Vec` this replaced re-`memmove`d every live
    /// byte on **every** acknowledged STREAM range — the cost round 42's
    /// profile found at 21 % of the per-datagram budget at the ratified
    /// window and 94 % at 8 MiB. See the module doc.
    fn release(&mut self) {
        let Some(first) = self.acked.first() else {
            return;
        };
        if first.start != 0 {
            return;
        }
        let up_to = first.end.max(self.base);
        if up_to <= self.base {
            return;
        }
        let drop = (up_to - self.base) as usize;
        // The clamp is against the **live** length, which is what
        // `buf.len()` is — see the field's doc. It bites only after a reset
        // has emptied the buffer while leaving the offsets standing; without
        // it, a late ACK for a range sent before the reset would advance
        // `base` past `write_offset`.
        let drop = drop.min(self.buf.len());
        self.buf.drain(..drop);
        self.base += drop as u64;
        debug_assert!(
            self.reset.is_some()
                || self.peer_reset.is_some()
                || self.buf.len() as u64 == self.write_offset - self.base,
            "the ring holds exactly the live bytes"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// A disjoint, ascending set of byte ranges
// ═══════════════════════════════════════════════════════════════════════

/// Disjoint, non-adjacent, ascending half-open ranges.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RangeSet {
    ranges: Vec<Range<u64>>,
}

impl RangeSet {
    /// Add a range, coalescing with everything it overlaps or touches.
    pub(crate) fn insert(&mut self, r: Range<u64>) {
        if r.start >= r.end {
            return;
        }
        let mut i = 0;
        while i < self.ranges.len() && self.ranges[i].end < r.start {
            i += 1;
        }
        let mut start = r.start;
        let mut end = r.end;
        let mut j = i;
        while j < self.ranges.len() && self.ranges[j].start <= end {
            start = start.min(self.ranges[j].start);
            end = end.max(self.ranges[j].end);
            j += 1;
        }
        self.ranges.splice(i..j, std::iter::once(start..end));
    }

    /// Remove a range, splitting whatever it cuts.
    pub(crate) fn remove(&mut self, r: Range<u64>) {
        if r.start >= r.end {
            return;
        }
        let mut out = Vec::with_capacity(self.ranges.len() + 1);
        for existing in std::mem::take(&mut self.ranges) {
            if existing.end <= r.start || existing.start >= r.end {
                out.push(existing);
                continue;
            }
            if existing.start < r.start {
                out.push(existing.start..r.start);
            }
            if existing.end > r.end {
                out.push(r.end..existing.end);
            }
        }
        self.ranges = out;
    }

    /// The lowest range.
    pub(crate) fn first(&self) -> Option<Range<u64>> {
        self.ranges.first().cloned()
    }

    /// Whether `r` is wholly contained.
    pub(crate) fn covers(&self, r: Range<u64>) -> bool {
        if r.start >= r.end {
            return true;
        }
        self.ranges
            .iter()
            .any(|e| e.start <= r.start && e.end >= r.end)
    }

    /// The highest offset covered, if any.
    pub(crate) fn last_end(&self) -> Option<u64> {
        self.ranges.last().map(|r| r.end)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Total bytes covered.
    pub(crate) fn len_bytes(&self) -> u64 {
        self.ranges.iter().map(|r| r.end - r.start).sum()
    }

    pub(crate) fn clear(&mut self) {
        self.ranges = Vec::new();
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Range<u64>> {
        self.ranges.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNLIMITED: u64 = u64::MAX;

    #[test]
    fn range_set_coalesces_on_insert_and_splits_on_remove() {
        let mut set = RangeSet::default();
        set.insert(0..10);
        set.insert(20..30);
        set.insert(10..20);
        assert_eq!(set.first(), Some(0..30));
        assert!(set.covers(5..25));

        set.remove(12..15);
        assert_eq!(set.first(), Some(0..12));
        assert_eq!(set.len_bytes(), 27);
        assert!(!set.covers(5..25));
        assert!(set.covers(15..30));
    }

    /// A write is bounded by **both** levels, and the tighter one binds
    /// (§10.1).
    #[test]
    fn a_write_is_bounded_by_whichever_level_is_tighter() {
        let mut half = SendHalf::new();
        assert_eq!(half.write(&[0u8; 100], 40), Ok(40));
        assert_eq!(half.write_offset(), 40);

        let mut half = SendHalf::new();
        half.on_max_stream_data(0); // no-op: monotone-max
        assert_eq!(half.max_data(), constants::INITIAL_MAX_STREAM_DATA);
        let window = constants::INITIAL_MAX_STREAM_DATA as usize;
        assert_eq!(half.write(&vec![0u8; window + 10], UNLIMITED), Ok(window));
        assert_eq!(half.write(b"more", UNLIMITED), Ok(0));
        assert!(half.is_blocked());
    }

    /// A refused write parks; the credit that arrives unblocks exactly the
    /// half that was parked.
    #[test]
    fn credit_unblocks_a_parked_writer() {
        let mut half = SendHalf::new();
        assert_eq!(half.write(&[0u8; 8], 0), Ok(0));
        assert!(half.is_blocked());
        assert!(half.on_max_stream_data(constants::INITIAL_MAX_STREAM_DATA + 1));
        assert!(!half.is_blocked());

        // A grant that does not raise the limit wakes nobody.
        let mut half = SendHalf::new();
        assert_eq!(half.write(&[0u8; 8], 0), Ok(0));
        assert!(!half.on_max_stream_data(constants::INITIAL_MAX_STREAM_DATA));
        assert!(half.is_blocked());
    }

    #[test]
    fn writing_after_finish_or_reset_is_finished() {
        let mut half = SendHalf::new();
        half.finish().unwrap();
        assert_eq!(half.write(b"x", UNLIMITED), Err(WriteError::Finished));
        // Idempotent.
        assert_eq!(half.finish(), Ok(()));

        let mut half = SendHalf::new();
        half.reset(7);
        assert_eq!(half.write(b"x", UNLIMITED), Err(WriteError::Finished));
        assert_eq!(half.finish(), Err(WriteError::Finished));
    }

    /// §8.5's quantum bounds one take; the rest stays pending.
    #[test]
    fn a_chunk_is_bounded_by_the_quantum_and_the_rest_stays_pending() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 100], UNLIMITED).unwrap();
        let chunk = half.next_chunk(30).expect("data is pending");
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.data.len(), 30);
        assert!(chunk.fresh);
        assert!(!chunk.fin);
        assert!(half.has_pending());
    }

    /// **Ruling 98's separation.** A first transmission is `fresh`; the same
    /// bytes returned by loss detection are not. A build with one pending
    /// set cannot tell them apart.
    #[test]
    fn a_retransmitted_chunk_is_not_a_first_transmission() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 10], UNLIMITED).unwrap();
        let first = half.next_chunk(10).unwrap();
        assert!(first.fresh);
        assert_eq!(half.next_chunk(10), None);

        half.on_lost_range(0..10, false);
        let again = half.next_chunk(10).unwrap();
        assert!(!again.fresh, "a retransmission is in §7.4's quiet set");
        assert_eq!(again.offset, 0);
        assert_eq!(again.data.len(), 10);
    }

    /// Retransmissions are served before fresh data.
    #[test]
    fn retransmissions_are_served_first() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 20], UNLIMITED).unwrap();
        half.next_chunk(10).unwrap();
        half.on_lost_range(0..10, false);
        let next = half.next_chunk(100).unwrap();
        assert_eq!(next.offset, 0);
        assert!(!next.fresh);
    }

    /// §8.7: *"only still un-ACKed sub-ranges are resent."*
    #[test]
    fn an_acked_sub_range_does_not_return_on_loss() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 20], UNLIMITED).unwrap();
        half.next_chunk(20).unwrap();
        half.on_ack_range(0..10, false);
        half.on_lost_range(0..20, false);
        let back = half.next_chunk(100).unwrap();
        assert_eq!(back.offset, 10);
        assert_eq!(back.data.len(), 10);
    }

    /// §9.5's empty FIN-only frame.
    #[test]
    fn an_empty_finish_produces_an_empty_fin_frame() {
        let mut half = SendHalf::new();
        half.finish().unwrap();
        let chunk = half.next_chunk(1_000).expect("the FIN is owed");
        assert_eq!(chunk.offset, 0);
        assert!(chunk.data.is_empty());
        assert!(chunk.fin);
        assert_eq!(half.next_chunk(1_000), None);
    }

    /// The FIN rides the last data frame when there is data left to carry
    /// it, and only then.
    #[test]
    fn the_fin_rides_the_frame_that_ends_the_stream() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 20], UNLIMITED).unwrap();
        half.finish().unwrap();
        let first = half.next_chunk(10).unwrap();
        assert!(!first.fin, "there are still bytes to come");
        let last = half.next_chunk(10).unwrap();
        assert!(last.fin);
        assert_eq!(half.next_chunk(10), None);
    }

    /// Slice 5's ACK seam (SPEC §12): `DataRecvd` is reachable **only**
    /// through the ACK entry
    /// points, which nothing on the wire calls in slice 4.
    #[test]
    fn data_recvd_needs_every_byte_and_the_fin_acknowledged() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 20], UNLIMITED).unwrap();
        half.finish().unwrap();
        let chunk = half.next_chunk(100).unwrap();
        assert!(chunk.fin);
        assert!(!half.is_terminal());

        half.on_ack_range(0..19, false);
        assert!(!half.is_terminal(), "one byte short");
        half.on_ack_range(19..20, false);
        assert!(!half.is_terminal(), "the FIN is not acknowledged");
        half.on_ack_range(20..20, true);
        assert!(half.is_terminal());
    }

    /// §9.6: a reset stops retransmission, and its own frame regenerates
    /// until acknowledged.
    ///
    /// `final_size` is the highest offset **sent** — 10 here, not the 40
    /// buffered. Pinning 40 would name a size the receiver can never reach.
    #[test]
    fn a_reset_stops_the_data_and_regenerates_until_acked() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 40], UNLIMITED).unwrap();
        half.next_chunk(10).unwrap();
        half.reset(9);

        assert_eq!(half.next_chunk(100), None, "pending data stops");
        assert_eq!(half.final_size(), Some(10));
        assert_eq!(half.take_reset(), Some((9, 10)));
        assert_eq!(half.take_reset(), None, "emitted once until it is lost");

        half.on_reset_lost();
        assert_eq!(half.take_reset(), Some((9, 10)));
        assert!(!half.is_terminal());
        half.on_reset_acked();
        assert!(half.is_terminal());
        assert_eq!(half.take_reset(), None);
    }

    /// §9.6: *"the end offset of the highest byte sent, or 0 if none"*.
    #[test]
    fn a_reset_with_nothing_written_has_final_size_zero() {
        let mut half = SendHalf::new();
        half.reset(3);
        assert_eq!(half.take_reset(), Some((3, 0)));

        // And one whose bytes were buffered but never transmitted is also
        // zero: §9.6 pins *"the end offset of the highest byte sent"*.
        let mut half = SendHalf::new();
        half.write(&[1u8; 100], UNLIMITED).unwrap();
        half.reset(3);
        assert_eq!(half.take_reset(), Some((3, 0)));
    }

    /// An empty write is a no-op, and must not look like credit exhaustion.
    #[test]
    fn an_empty_write_is_a_no_op_and_not_a_block() {
        let mut half = SendHalf::new();
        assert_eq!(half.write(b"", UNLIMITED), Ok(0));
        assert!(!half.is_blocked());
        assert!(!half.has_pending());
    }

    /// The retention set is what §8.7's `ranges` class acts on, and it does
    /// **not** drain on send — freeing on send would make slice 5's tests
    /// pass for free.
    #[test]
    fn sending_does_not_free_the_retention_set() {
        let mut half = SendHalf::new();
        half.write(&[1u8; 50], UNLIMITED).unwrap();
        half.next_chunk(50).unwrap();
        assert_eq!(half.unacked.len_bytes(), 50);
        half.on_ack_range(0..50, false);
        assert_eq!(half.unacked.len_bytes(), 0);
    }
}

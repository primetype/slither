//! §9.4 — the receive half, and §10.6's coalescing reassembler.
//!
//! §9.4's five-state diagram is exposition; this is the collapse it names:
//! `final_size: Option<u64>` is `Recv`/`SizeKnown`, `reset: Option<u64>` is
//! `ResetRecvd`, and the terminals are represented by **removal** — the
//! stream table drops the half and leaves a [`RecvTombstone`].
//!
//! # Reassembly allocates lazily
//!
//! **[RATIFIED 2026/08/15 — ruling 94]** §10.6 offers two admissible
//! implementations. (a) — a span-allocated buffer plus a received bitmap —
//! taken literally at the level §10.6's *mandate* names (per stream) makes
//! 128 peer-opened uni streams allocate 32 MiB against 1 MiB of credit: a
//! 32× remote memory amplification produced by following the section that
//! exists to forbid it. This is (b): ranges are **coalesced on insert**,
//! nothing is allocated ahead of arrival, and a stream whose stored
//! discontiguous ranges would exceed `REASSEMBLY_CHUNKS_MAX` after
//! coalescing is a `PROTOCOL_VIOLATION` (§10.6, ruling 104's third §10
//! violation).
//!
//! [`Reassembly::capacity`] is the accounting ruling 94 requires be
//! **test-visible**: §10.6's memory bound is otherwise an untestable MUST,
//! and a test that asserted *bytes received* would pass an eager allocator
//! for free.
//!
//! # Overlap
//!
//! §9.5: *"a byte received twice with differing values is undefined
//! behaviour of the sender … and the receiver may keep either."* This keeps
//! the **first** copy: a merge writes the arriving data into the span first
//! and then copies the already-stored chunks over it.

use std::collections::VecDeque;

use crate::constants;

use super::flow::{CreditWindow, Violation};

/// What a read found. The core's public shape is
/// `Result<Option<usize>, ReadError>`; this is the same three answers
/// without the error type, because a reset is not an error *here* — the
/// half has to survive it long enough to be observed (§9.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// `n` bytes drained from the contiguous prefix. `n == 0` means **no
    /// data available** — the shell parks.
    Data(usize),
    /// End of stream: the final size is reached **and** drained.
    End,
    /// The peer reset it (§9.6), with its code.
    Reset(u64),
}

/// §9.4's receive half.
pub(crate) struct RecvHalf {
    reassembly: Reassembly,
    /// Bytes handed to the application — the contiguous prefix consumed.
    read_offset: u64,
    /// §10.1's per-stream contribution: the highest received offset.
    high_water: u64,
    /// Pinned by a FIN (§9.5) or by a RESET_STREAM (§9.6).
    final_size: Option<u64>,
    /// §9.6's reset code, until the application observes it.
    reset: Option<u64>,
    /// Whether the application has observed the reset.
    reset_observed: bool,
    /// §10.3's stream-level window.
    credit: CreditWindow,
    /// **§12.1's seam.** §10.3: *"Sugar-consumed streams never earn
    /// stream-level credit (§9.8); their reads still earn connection-level
    /// credit."* In slice 4 the answer is always `true` — a **field**, not a
    /// constant, so slice 6 does not thread a mode flag through the ledger.
    earns_stream_credit: bool,
    /// How much of this half's connection-level contribution has already
    /// been folded into the connection scalar. §10.3's *"absolute, not
    /// additive"* rule, enforced structurally.
    counted: u64,
}

impl RecvHalf {
    /// A fresh half with §10.2's un-negotiated stream window already
    /// advertised (**H15**).
    pub(crate) fn new() -> Self {
        Self {
            reassembly: Reassembly::new(),
            read_offset: 0,
            high_water: 0,
            final_size: None,
            reset: None,
            reset_observed: false,
            credit: CreditWindow::new(constants::INITIAL_MAX_STREAM_DATA),
            earns_stream_credit: true,
            counted: 0,
        }
    }

    /// The highest stream-level limit ever advertised for this half.
    ///
    /// **[ruling 93]** The retirement true-up value — *not* the high-water
    /// mark, which leaks credit permanently.
    pub(crate) fn advertised(&self) -> u64 {
        self.credit.advertised()
    }

    /// §10.1's per-stream contribution to the connection-level sum.
    pub(crate) fn high_water(&self) -> u64 {
        self.high_water
    }

    /// The pinned final size, if any.
    pub(crate) fn final_size(&self) -> Option<u64> {
        self.final_size
    }

    /// How much of this half's contribution is already folded into the
    /// connection ledger.
    pub(crate) fn counted(&self) -> u64 {
        self.counted
    }

    /// Ruling 94's accounting: bytes of reassembly **capacity** currently
    /// allocated for this half.
    pub(crate) fn capacity(&self) -> u64 {
        self.reassembly.capacity()
    }

    /// Whether a `read()` would return anything other than "no data".
    pub(crate) fn is_readable(&self) -> bool {
        self.reset.is_some()
            || self.reassembly.contiguous_at(self.read_offset) > 0
            || self.final_size == Some(self.read_offset)
    }

    /// §9.7's terminals: read to the final size (`DataRead`) or the reset
    /// observed (`ResetRead`).
    pub(crate) fn is_retired(&self) -> bool {
        self.reset_observed || self.final_size == Some(self.read_offset)
    }

    // ── §8.4/§9.5's semantic checks ─────────────────────────────────────

    /// Ruling 97's step 4 then step 5, for a STREAM frame — final size
    /// first, then the **stream-level** credit bound. Returns the new
    /// high-water mark.
    ///
    /// The connection-level bound is the caller's: the ledger is consulted
    /// exactly once per frame, after the frame is known otherwise legal.
    pub(crate) fn check_stream(&self, offset: u64, len: u64, fin: bool) -> Result<u64, Violation> {
        let end = offset.checked_add(len).ok_or(Violation::FinalSize)?;

        // §9.5: data beyond a pinned final size, or a second pin that
        // disagrees.
        if self
            .final_size
            .is_some_and(|pinned| end > pinned || (fin && end != pinned))
        {
            return Err(Violation::FinalSize);
        }
        // §9.5: a FIN pinning a size below already-received data.
        if fin && end < self.high_water {
            return Err(Violation::FinalSize);
        }

        let new_high = self.high_water.max(end);
        if new_high > self.credit.advertised() {
            return Err(Violation::FlowControl);
        }
        Ok(new_high)
    }

    /// The same two steps for a RESET_STREAM (§9.6, §8.4).
    ///
    /// §8.4 orders this one itself: the `FLOW_CONTROL_ERROR` check runs
    /// *"**before** the §9.6/§10.3 credit true-up"*, with checked or
    /// saturating arithmetic.
    pub(crate) fn check_reset(&self, final_size: u64) -> Result<(), Violation> {
        if final_size < self.high_water {
            return Err(Violation::FinalSize);
        }
        if self.final_size.is_some_and(|pinned| pinned != final_size) {
            return Err(Violation::FinalSize);
        }
        if final_size > self.credit.advertised() {
            return Err(Violation::FlowControl);
        }
        Ok(())
    }

    // ── application ─────────────────────────────────────────────────────

    /// Apply a checked STREAM frame. Returns the connection-level charge
    /// delta — the amount by which this half's §10.1 contribution grew.
    pub(crate) fn apply_stream(
        &mut self,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<u64, Violation> {
        let end = offset.saturating_add(data.len() as u64);

        if self.reset.is_none() {
            // §9.6: a reset already discarded the buffer; late data for a
            // reset stream is stored nowhere.
            self.reassembly.insert(offset, data, self.read_offset)?;
        }
        if fin {
            self.final_size = Some(end);
        }
        let delta = end.saturating_sub(self.high_water);
        self.high_water = self.high_water.max(end);
        Ok(delta)
    }

    /// Apply a checked RESET_STREAM. Returns the connection-level charge
    /// delta, and whether the reset is newly surfaced.
    ///
    /// §9.6: *"A RESET_STREAM for an already-FIN-complete receive half is a
    /// valid no-op if the final sizes agree"* — so a reset that arrives
    /// after every byte is in hand changes nothing.
    ///
    /// §12.7: the reassembly buffer is discarded when the reset is
    /// **applied**, not when the application observes it. Three distinct
    /// moments; holding the buffer until observation would let a peer pin
    /// 1 MiB behind an application that never reads.
    pub(crate) fn apply_reset(&mut self, final_size: u64, code: u64) -> (u64, bool) {
        if self.final_size == Some(final_size) && self.high_water == final_size {
            return (0, false);
        }
        let delta = final_size.saturating_sub(self.high_water);
        self.high_water = final_size;
        self.final_size = Some(final_size);
        let newly = self.reset.is_none();
        if newly {
            self.reset = Some(code);
        }
        self.reassembly.discard();
        (delta, newly)
    }

    /// §16.4's pull-model read: drain the contiguous prefix.
    ///
    /// Returns the bytes consumed at the connection level too — §10.6's
    /// *"Consumption is the application taking bytes out of the connection
    /// core"*.
    pub(crate) fn read(&mut self, buf: &mut [u8]) -> (ReadOutcome, u64) {
        if let Some(code) = self.reset {
            self.reset_observed = true;
            return (ReadOutcome::Reset(code), 0);
        }
        let n = self.reassembly.read(self.read_offset, buf);
        if n > 0 {
            self.read_offset += n as u64;
            self.credit.consume(n as u64);
            self.counted = self.counted.saturating_add(n as u64);
            return (ReadOutcome::Data(n), n as u64);
        }
        if self.final_size == Some(self.read_offset) {
            (ReadOutcome::End, 0)
        } else {
            (ReadOutcome::Data(0), 0)
        }
    }

    /// §10.3's stream-level re-grant, as an absolute limit.
    ///
    /// `None` for a half that does not earn stream-level credit — slice 6's
    /// sugar streams (§12.1's seam).
    pub(crate) fn take_grant(&mut self) -> Option<u64> {
        if !self.earns_stream_credit {
            return None;
        }
        self.credit.take_grant()
    }

    /// The value §10.3's retirement true-up brings this half's contribution
    /// **to**, given why it is retiring.
    ///
    /// - a pinned final size (read to it, or a reset that pinned it) is the
    ///   bytes the peer actually asserted;
    /// - **[ruling 93]** an abandoned half has no final size and never will,
    ///   so the value is the highest stream-level limit ever advertised —
    ///   the least upper bound on what the peer could legally have sent, and
    ///   the buffer commitment §10.6 says freeing the half releases.
    pub(crate) fn retirement_target(&self) -> u64 {
        match self.final_size {
            Some(size) => size,
            None => self.credit.advertised(),
        }
    }

    /// The tombstone this half leaves behind when it is freed while its
    /// stream stays open.
    pub(crate) fn tombstone(&self) -> RecvTombstone {
        RecvTombstone {
            limit: self.credit.advertised(),
            high_water: self.high_water,
            final_size: self.final_size,
        }
    }
}

/// What a freed receive half leaves behind on a stream that is **not** fully
/// closed.
///
/// **[RATIFIED 2026/08/15 — ruling 93, as amended]** Two tombstone
/// mechanisms are required and this is the second one. §9.2's watermark
/// covers a **peer-opened uni** stream, whose receive half is the only half
/// this endpoint holds, so freeing it fully closes the stream. A **bidi**
/// stream's send half is still live: the watermark does not advance, the
/// index stays in the open set, and later STREAM frames for it are therefore
/// neither watermark no-ops nor implicit opens. This carries the two numbers
/// that keep them from becoming an unbounded sink.
///
/// The frames are ACKed, delivered nowhere, and consume no further
/// connection credit — §10.3's true-up is absolute and the stream's
/// contribution already sits at its maximum. **The stream-level
/// `FLOW_CONTROL_ERROR` check still runs**, against the frozen limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecvTombstone {
    /// The stream-level limit frozen at retirement.
    limit: u64,
    /// The highest received offset at retirement.
    high_water: u64,
    /// The final size, if one was ever pinned.
    final_size: Option<u64>,
}

impl RecvTombstone {
    /// The checks a frame for a retired-but-not-tombstoned half still faces.
    ///
    /// Final size is kept as well as flow control: §9.5's rule is about a
    /// stream we already fully understand, and dropping it here would make
    /// the check depend on local read timing in a second, unstated way
    /// (C6 already makes it depend on it once).
    pub(crate) fn check_stream(&self, offset: u64, len: u64, fin: bool) -> Result<(), Violation> {
        let end = offset.checked_add(len).ok_or(Violation::FinalSize)?;
        if self
            .final_size
            .is_some_and(|pinned| end > pinned || (fin && end != pinned))
        {
            return Err(Violation::FinalSize);
        }
        if fin && end < self.high_water {
            return Err(Violation::FinalSize);
        }
        if end > self.limit {
            return Err(Violation::FlowControl);
        }
        Ok(())
    }

    /// The same, for a RESET_STREAM.
    pub(crate) fn check_reset(&self, final_size: u64) -> Result<(), Violation> {
        if final_size < self.high_water {
            return Err(Violation::FinalSize);
        }
        if self.final_size.is_some_and(|pinned| pinned != final_size) {
            return Err(Violation::FinalSize);
        }
        if final_size > self.limit {
            return Err(Violation::FlowControl);
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §10.6's reassembler
// ═══════════════════════════════════════════════════════════════════════

struct Chunk {
    offset: u64,
    data: Vec<u8>,
}

impl Chunk {
    fn end(&self) -> u64 {
        self.offset + self.data.len() as u64
    }
}

/// §10.6's admissible implementation (b): coalesce on insert, hard-fail past
/// `REASSEMBLY_CHUNKS_MAX`.
struct Reassembly {
    /// Disjoint, non-adjacent, ascending by offset.
    chunks: VecDeque<Chunk>,
}

impl Reassembly {
    fn new() -> Self {
        // **Ruling 94: allocate lazily.** No `with_capacity` here — this is
        // the line that would turn 128 peer-opened streams into 32 MiB.
        Self {
            chunks: VecDeque::new(),
        }
    }

    /// Bytes of allocated capacity. Ruling 94's test-visible accounting.
    fn capacity(&self) -> u64 {
        self.chunks.iter().map(|c| c.data.capacity() as u64).sum()
    }

    /// How many contiguous bytes are available starting at `at`.
    fn contiguous_at(&self, at: u64) -> u64 {
        match self.chunks.front() {
            Some(c) if c.offset == at => c.data.len() as u64,
            _ => 0,
        }
    }

    fn discard(&mut self) {
        self.chunks = VecDeque::new();
    }

    /// Insert a received range, coalescing with everything it overlaps or
    /// touches.
    fn insert(
        &mut self,
        mut offset: u64,
        mut data: &[u8],
        read_offset: u64,
    ) -> Result<(), Violation> {
        // Bytes already delivered are duplicates: §9.5 delivers each byte
        // exactly once, and re-storing them would let a peer re-charge
        // memory it has already been credited for.
        if offset < read_offset {
            let skip = read_offset - offset;
            if skip >= data.len() as u64 {
                return Ok(());
            }
            data = &data[skip as usize..];
            offset = read_offset;
        }
        if data.is_empty() {
            // §9.5's empty frame is a no-op **for the data**; ruling 100
            // keeps the open, which happened before we got here.
            return Ok(());
        }

        let end = offset + data.len() as u64;

        // The merge span: every chunk that overlaps or is adjacent.
        let mut lo = 0usize;
        while lo < self.chunks.len() && self.chunks[lo].end() < offset {
            lo += 1;
        }
        let mut hi = lo;
        while hi < self.chunks.len() && self.chunks[hi].offset <= end {
            hi += 1;
        }

        if lo == hi {
            // Disjoint: one exact-capacity allocation for the arriving
            // bytes and nothing more.
            self.chunks.insert(
                lo,
                Chunk {
                    offset,
                    data: data.to_vec(),
                },
            );
        } else {
            let start = self.chunks[lo].offset.min(offset);
            let stop = self.chunks[hi - 1].end().max(end);
            let mut merged = vec![0u8; (stop - start) as usize];
            let at = |o: u64| (o - start) as usize;
            merged[at(offset)..at(end)].copy_from_slice(data);
            // Stored bytes win on overlap — §9.5 lets the receiver keep
            // either, and keeping the first is the cheaper invariant to
            // reason about.
            for c in self.chunks.range(lo..hi) {
                merged[at(c.offset)..at(c.end())].copy_from_slice(&c.data);
            }
            for _ in lo..hi {
                self.chunks.remove(lo);
            }
            self.chunks.insert(
                lo,
                Chunk {
                    offset: start,
                    data: merged,
                },
            );
        }

        // §10.6: *"would exceed `REASSEMBLY_CHUNKS_MAX` (= 1024) **after
        // coalescing**"*, so the count is checked here and not before.
        if self.chunks.len() > constants::REASSEMBLY_CHUNKS_MAX {
            return Err(Violation::Reassembly);
        }
        Ok(())
    }

    /// Drain the contiguous prefix starting at `at`.
    fn read(&mut self, at: u64, buf: &mut [u8]) -> usize {
        let Some(front) = self.chunks.front_mut() else {
            return 0;
        };
        if front.offset != at {
            return 0;
        }
        let n = buf.len().min(front.data.len());
        buf[..n].copy_from_slice(&front.data[..n]);
        front.data.drain(..n);
        front.offset += n as u64;
        if front.data.is_empty() {
            self.chunks.pop_front();
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(half: &mut RecvHalf) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            match half.read(&mut buf).0 {
                ReadOutcome::Data(0) | ReadOutcome::End => return out,
                ReadOutcome::Data(n) => out.extend_from_slice(&buf[..n]),
                ReadOutcome::Reset(_) => return out,
            }
        }
    }

    /// §9.5: ranges arrive in any order; the contiguous prefix is delivered
    /// as it becomes available.
    #[test]
    fn out_of_order_ranges_reassemble() {
        let mut half = RecvHalf::new();
        half.apply_stream(6, b"world", false).unwrap();
        assert_eq!(drain(&mut half), b"", "nothing is contiguous yet");
        half.apply_stream(0, b"hello ", false).unwrap();
        assert_eq!(drain(&mut half), b"hello world");
    }

    /// §9.5: overlapping ranges deliver each byte exactly once.
    #[test]
    fn overlapping_ranges_deliver_each_byte_once() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcdef", false).unwrap();
        half.apply_stream(3, b"defghi", false).unwrap();
        half.apply_stream(2, b"cd", false).unwrap();
        assert_eq!(drain(&mut half), b"abcdefghi");
    }

    /// A range wholly below the read cursor is a duplicate and stores
    /// nothing — the tombstone case's smaller sibling.
    #[test]
    fn a_fully_read_range_that_arrives_again_stores_nothing() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcdef", false).unwrap();
        assert_eq!(drain(&mut half), b"abcdef");
        assert_eq!(half.capacity(), 0);
        half.apply_stream(0, b"abcdef", false).unwrap();
        assert_eq!(half.capacity(), 0);
        assert_eq!(drain(&mut half), b"");
    }

    /// Ruling 94's accounting: capacity tracks what arrived, and drops to
    /// zero as the prefix is drained. An eager allocator would report a
    /// stream window here at byte one.
    #[test]
    fn capacity_is_what_arrived_and_not_the_window() {
        let mut half = RecvHalf::new();
        assert_eq!(half.capacity(), 0);
        half.apply_stream(0, &[0u8; 100], false).unwrap();
        assert_eq!(half.capacity(), 100);
        assert!(half.capacity() < constants::INITIAL_MAX_STREAM_DATA);
        let mut buf = [0u8; 100];
        assert_eq!(half.read(&mut buf).0, ReadOutcome::Data(100));
        assert_eq!(half.capacity(), 0);
    }

    /// §10.6's ceiling, two-sided: 1024 stored ranges is legal, 1025 is
    /// ruling 104's third §10 violation.
    #[test]
    fn the_reassembly_chunk_ceiling_is_two_sided() {
        let mut half = RecvHalf::new();
        // Every other byte, so nothing coalesces.
        for i in 0..constants::REASSEMBLY_CHUNKS_MAX as u64 {
            half.apply_stream(i * 2, b"x", false)
                .expect("1024 ranges is inside the ceiling");
        }
        let n = constants::REASSEMBLY_CHUNKS_MAX as u64;
        assert_eq!(
            half.apply_stream(n * 2, b"x", false),
            Err(Violation::Reassembly)
        );
    }

    /// Coalescing is what keeps the count down: 2000 adjacent ranges are one
    /// chunk, not 2000.
    #[test]
    fn adjacent_ranges_coalesce_rather_than_accumulate() {
        let mut half = RecvHalf::new();
        for i in 0..2_000u64 {
            half.apply_stream(i, b"x", false)
                .expect("adjacent ranges coalesce into one");
        }
        assert_eq!(half.capacity(), 2_000);
    }

    /// §9.5's three `FINAL_SIZE_ERROR` cases.
    #[test]
    fn the_three_final_size_errors() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcde", true).unwrap();

        // data beyond a pinned final size
        assert_eq!(half.check_stream(5, 1, false), Err(Violation::FinalSize));
        // a second pin that disagrees
        assert_eq!(half.check_stream(0, 3, true), Err(Violation::FinalSize));
        // and one that agrees is fine
        assert_eq!(half.check_stream(0, 5, true), Ok(5));

        // a FIN pinning a size below already-received data
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcdef", false).unwrap();
        assert_eq!(half.check_stream(0, 3, true), Err(Violation::FinalSize));
    }

    /// §10.5's stream-level bound, two-sided against the advertised window.
    #[test]
    fn the_stream_credit_bound_is_two_sided() {
        let half = RecvHalf::new();
        let window = constants::INITIAL_MAX_STREAM_DATA;
        assert!(half.check_stream(window - 1, 1, false).is_ok());
        assert_eq!(
            half.check_stream(window, 1, false),
            Err(Violation::FlowControl)
        );
        // Checked arithmetic: a huge offset must not wrap into "fits".
        assert_eq!(
            half.check_stream(u64::MAX, 2, false),
            Err(Violation::FinalSize)
        );
    }

    /// §9.6: the reset discards the buffer at **apply** time, and surfaces
    /// through `read` until observed.
    #[test]
    fn a_reset_discards_the_buffer_and_surfaces_once_observed() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, &[7u8; 200], false).unwrap();
        assert_eq!(half.capacity(), 200);
        let (delta, newly) = half.apply_reset(500, 42);
        assert!(newly);
        assert_eq!(delta, 300);
        assert_eq!(half.capacity(), 0, "§12.7: the discard is the first moment");

        let mut buf = [0u8; 8];
        assert_eq!(half.read(&mut buf).0, ReadOutcome::Reset(42));
        assert!(half.is_retired());
    }

    /// §9.6: a reset for an already-FIN-complete half is a valid no-op when
    /// the sizes agree.
    #[test]
    fn a_reset_agreeing_with_a_complete_half_is_a_no_op() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcde", true).unwrap();
        assert_eq!(half.check_reset(5), Ok(()));
        let (delta, newly) = half.apply_reset(5, 9);
        assert_eq!(delta, 0);
        assert!(!newly);
        assert_eq!(drain(&mut half), b"abcde");
    }

    /// EOF is `End`, and it arrives **after** the last byte — the shell
    /// turns `Data(0)` into a park and `End` into EOF, and getting the two
    /// backwards hangs a reader forever.
    #[test]
    fn end_of_stream_follows_the_last_byte_rather_than_replacing_it() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"ab", true).unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(half.read(&mut buf).0, ReadOutcome::Data(2));
        assert!(half.is_retired());
        assert_eq!(half.read(&mut buf).0, ReadOutcome::End);
    }

    /// No data and no final size is a park, not an EOF.
    #[test]
    fn an_empty_open_half_parks_rather_than_ending() {
        let mut half = RecvHalf::new();
        let mut buf = [0u8; 8];
        assert_eq!(half.read(&mut buf).0, ReadOutcome::Data(0));
        assert!(!half.is_retired());
    }

    /// **[ruling 93]** The true-up value for an abandoned half is the
    /// advertised limit, and it grows with §10.3's re-grant rather than
    /// being frozen at the constant.
    #[test]
    fn the_retirement_target_is_the_advertised_limit_not_the_high_water_mark() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, &[0u8; 1_000], false).unwrap();
        assert_eq!(half.high_water(), 1_000);
        assert_eq!(
            half.retirement_target(),
            constants::INITIAL_MAX_STREAM_DATA,
            "the high-water mark leaks credit permanently"
        );

        // Grown by a re-grant, it is no longer the bare constant.
        let mut buf = vec![0u8; constants::INITIAL_MAX_STREAM_DATA as usize];
        for _ in 0..8 {
            half.apply_stream(half.high_water(), &[0u8; 20_000], false)
                .unwrap();
            while let ReadOutcome::Data(n) = half.read(&mut buf).0 {
                if n == 0 {
                    break;
                }
            }
            half.take_grant();
        }
        assert!(half.retirement_target() > constants::INITIAL_MAX_STREAM_DATA);
        assert!(half.retirement_target() > half.high_water());
    }

    /// A pinned final size wins over the advertised limit: the peer told us
    /// exactly how many bytes there were.
    #[test]
    fn a_pinned_final_size_is_the_retirement_target() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcde", true).unwrap();
        assert_eq!(half.retirement_target(), 5);
    }
}

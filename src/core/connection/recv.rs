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
//! # Reassembly merges small into large
//!
//! **[RATIFIED 2026/08/17 — ruling 253]** §10.6's mandate above bounds
//! *state*; its work clause bounds *work*. Coalescing on insert by
//! allocating the merged span and copying everything into it is O(span) per
//! frame, so a peer alternating **bridging** inserts — one new byte joining
//! two stored ranges, over and over — sustained a measured 916× receiver
//! work per wire byte while staying inside flow credit. [`Reassembly::insert`]
//! extends the larger buffer and copies the smaller side instead, which puts
//! total copy work per stream at O(credit · log credit), and
//! [`Reassembly::copy_work`] is the accounting that makes the bound
//! test-visible the way [`Reassembly::capacity`] makes ruling 94's bound
//! test-visible.
//!
//! # Overlap
//!
//! §9.5: *"a byte received twice with differing values is undefined
//! behaviour of the sender … and the receiver may keep either."*
//!
//! **[ruling 253]** Which copy survives is **not a promise this module
//! makes**. §9.5's "either" is the whole rule, and a merge that is free to
//! keep whichever side is cheaper to keep is exactly what the small-to-large
//! discipline needs. As it happens this build keeps the **stored** copy —
//! the merge writes the arriving frame only where no stored chunk already
//! holds the byte, which is both the first-copy-wins answer and the cheap
//! one — but nothing may depend on that, and a merge that reversed it would
//! still be conformant.

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

    /// Ruling 253's accounting: bytes this half has ever written into
    /// reassembly storage — see [`Reassembly::copy_work`]. Monotone, and a
    /// reset does not return it.
    pub(crate) fn copy_work(&self) -> u64 {
        self.reassembly.copy_work()
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

    /// §9.8's message predicate: *"reassembly is complete (FIN and all
    /// bytes)"*.
    ///
    /// A **reset** half is never complete, however its final size was
    /// pinned. §9.6 pins one from a RESET_STREAM too, and the reassembly is
    /// discarded with it — so without the first clause a reset stream would
    /// surface as an empty message, inventing a delivery the peer withdrew.
    ///
    /// Zero bytes with the FIN at offset 0 **is** complete: an empty message
    /// is a message, and answering "not yet" would park a message-mode
    /// reader on a payload that has fully arrived.
    pub(crate) fn is_complete(&self) -> bool {
        self.reset.is_none()
            && self.final_size.is_some_and(|size| {
                self.read_offset + self.reassembly.contiguous_at(self.read_offset) == size
            })
    }

    /// §9.8's claim: take the whole message and leave the half drained.
    ///
    /// `None` unless [`is_complete`](Self::is_complete). The bytes are
    /// counted exactly as [`read`](Self::read) counts them, so the
    /// retirement true-up that follows finds `counted == final_size` and
    /// adds nothing on top (§10.3's *"absolute, not additive"*).
    ///
    /// **[`take_grant`](Self::take_grant) is deliberately not called.**
    /// §10.3: *"Sugar-consumed streams never earn stream-level credit
    /// (§9.8); their reads still earn connection-level credit."* The
    /// mechanism is this omission and **not** the `earns_stream_credit`
    /// flag, which ruling 156c leaves `true` everywhere: a message stream is
    /// never `read()`, so the flag has nothing to gate.
    pub(crate) fn take_message(&mut self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let size = self.final_size?;
        let remaining = usize::try_from(size - self.read_offset).ok()?;
        let mut buf = vec![0u8; remaining];
        let got = self.reassembly.read(self.read_offset, &mut buf);
        debug_assert_eq!(
            got, remaining,
            "a complete half holds every byte to its final size"
        );
        buf.truncate(got);
        self.read_offset += got as u64;
        self.credit.consume(got as u64);
        self.counted = self.counted.saturating_add(got as u64);
        Some(buf)
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

/// One coalesced range of received bytes.
///
/// `data[head..]` is the range and `offset` is the stream offset of
/// `data[head]`; `data[..head]` is a **head gap** and
/// `data.capacity() - data.len()` is a tail gap.
///
/// **[ruling 253(i)]** The head gap is what makes the small-to-large merge
/// implementable at *both* ends. Extending the larger buffer is cheap on the
/// right for free — that is what spare `Vec` capacity is — and a peer that
/// bridges **downwards**, one byte at a time just below a large stored
/// chunk, would otherwise pay a shift of the whole buffer per frame: the
/// same amplification 253 closes, mirrored onto the front.
struct Chunk {
    /// Stream offset of the first live byte.
    offset: u64,
    data: Vec<u8>,
    head: usize,
}

/// **[ruling 253(ii)] Capped growth.** A reallocating chunk takes an eighth
/// of itself as slack, so allocated capacity stays within 1.125× the arrived
/// span and §10.6's per-stream ceiling stays ≈ the advertised credit.
///
/// The two ends of the knob are both refused. A bare doubling policy holds
/// ~1.5 × credit, which ruling 253 declines; an exact allocation holds 1.0 ×
/// and reallocates on **every** merge, which is the whole-span copy 253(i)
/// removes. At an eighth the copying spent growing one chunk to `n` bytes is
/// geometric and sums to ≤ 9 `n` — a constant factor *inside* the
/// O(credit · log credit) bound rather than a term added to it.
const REASSEMBLY_SLACK_SHIFT: u32 = 3;

impl Chunk {
    /// An arriving frame, allocated **exactly**: ruling 94's lazy allocation
    /// is the disjoint case, and 253 does not touch it.
    fn from_frame(offset: u64, data: &[u8]) -> Self {
        Self {
            offset,
            data: data.to_vec(),
            head: 0,
        }
    }

    /// The placeholder a merge parks at the base's index while it owns the
    /// base. It holds no allocation, it is overwritten before `insert`
    /// returns, and nothing outside `insert` can observe it.
    fn vacant() -> Self {
        Self {
            offset: 0,
            data: Vec::new(),
            head: 0,
        }
    }

    fn len(&self) -> usize {
        self.data.len() - self.head
    }

    fn end(&self) -> u64 {
        self.offset + self.len() as u64
    }

    fn bytes(&self) -> &[u8] {
        &self.data[self.head..]
    }

    /// The `n` bytes at stream offset `at`, writable.
    fn at_mut(&mut self, at: u64, n: usize) -> &mut [u8] {
        let from = self.head + (at - self.offset) as usize;
        &mut self.data[from..from + n]
    }

    /// Copy `[from, to)` of an arriving frame in. The interval is inside the
    /// frame by the coverage lemma in [`Reassembly::insert`]'s doc.
    fn write_frame(&mut self, from: u64, to: u64, frame_at: u64, frame: &[u8]) -> u64 {
        debug_assert!(from >= frame_at && to <= frame_at + frame.len() as u64);
        let src = &frame[(from - frame_at) as usize..(to - frame_at) as usize];
        self.at_mut(from, src.len()).copy_from_slice(src);
        src.len() as u64
    }

    /// Copy a stored chunk in, at its own offset. Chunks are pairwise
    /// disjoint, so this never lands on bytes this chunk already holds.
    fn write_chunk(&mut self, other: &Chunk) -> u64 {
        let n = other.len();
        self.at_mut(other.offset, n).copy_from_slice(other.bytes());
        n as u64
    }

    /// Grow so the range spans `[start, stop)`, leaving the two new regions
    /// zeroed for the caller to fill.
    ///
    /// Returns the number of **stored** bytes moved — zero whenever the
    /// slack already paid for covers both ends, which is the entire point of
    /// holding slack. A merge that finds room is O(the bytes it adds).
    fn reserve_span(&mut self, start: u64, stop: u64) -> u64 {
        debug_assert!(start <= self.offset && stop >= self.end());
        let front = (self.offset - start) as usize;
        let back = (stop - self.end()) as usize;

        if front <= self.head && back <= self.data.capacity() - self.data.len() {
            self.head -= front;
            // The head gap holds bytes this chunk already delivered or never
            // owned. The caller covers every byte of both new regions (the
            // coverage lemma), but zeroing keeps a coverage bug a zero byte
            // rather than a stale one.
            self.data[self.head..self.head + front].fill(0);
            self.data.resize(self.data.len() + back, 0);
            self.offset = start;
            return 0;
        }

        let held = self.len();
        let need = front + held + back;
        let slack = need >> REASSEMBLY_SLACK_SHIFT;
        // Slack goes to the end that just grew: a range being extended in
        // one direction is overwhelmingly likely to be extended in it again,
        // and slack on the other side is capacity §10.6 counts and nothing
        // spends.
        let (gap_front, gap_back) = match (front > 0, back > 0) {
            (true, true) => (slack / 2, slack - slack / 2),
            (true, false) => (slack, 0),
            // `(false, false)` cannot reach here: it needs nothing, so the
            // fast path above always takes it.
            _ => (0, slack),
        };
        let mut fresh = Vec::with_capacity(gap_front + need + gap_back);
        fresh.resize(gap_front + front, 0);
        fresh.extend_from_slice(self.bytes());
        fresh.resize(gap_front + need, 0);
        self.data = fresh;
        self.head = gap_front;
        self.offset = start;
        held as u64
    }

    /// Hand `n` bytes off the front to the application.
    fn advance(&mut self, n: usize) {
        self.head += n;
        self.offset += n as u64;
    }
}

/// §10.6's admissible implementation (b): coalesce on insert, hard-fail past
/// `REASSEMBLY_CHUNKS_MAX`.
struct Reassembly {
    /// Disjoint, non-adjacent, ascending by offset.
    chunks: VecDeque<Chunk>,
    /// Ruling 253's accounting — see [`Reassembly::copy_work`].
    copy_work: u64,
}

impl Reassembly {
    fn new() -> Self {
        // **Ruling 94: allocate lazily.** No `with_capacity` here — this is
        // the line that would turn 128 peer-opened streams into 32 MiB.
        Self {
            chunks: VecDeque::new(),
            copy_work: 0,
        }
    }

    /// Bytes of allocated capacity. Ruling 94's test-visible accounting.
    fn capacity(&self) -> u64 {
        self.chunks.iter().map(|c| c.data.capacity() as u64).sum()
    }

    /// Total bytes ever written into chunk storage by [`Reassembly::insert`]:
    /// the arriving frame's bytes on store, plus every stored byte re-copied
    /// by a merge — a chunk folded into the base, or the base's own bytes
    /// moved by a reallocation.
    ///
    /// **[ruling 253]** This is to §10.6's *work* bound what
    /// [`Reassembly::capacity`] is to ruling 94's *memory* bound. The clause
    /// is otherwise an untestable MUST: no behavioural assertion separates a
    /// small-to-large merge from the whole-span copy it replaces, because
    /// both deliver exactly the same bytes, and the throughput gate passes
    /// on both.
    ///
    /// **Monotone, and never reset** — in particular not by
    /// [`Reassembly::discard`]. The quantity under test is what a peer
    /// *spent*; a reset that zeroed the meter would hand it back, and
    /// §9.6's reset is a frame the peer chooses to send.
    fn copy_work(&self) -> u64 {
        self.copy_work
    }

    /// How many contiguous bytes are available starting at `at`.
    fn contiguous_at(&self, at: u64) -> u64 {
        match self.chunks.front() {
            Some(c) if c.offset == at => c.len() as u64,
            _ => 0,
        }
    }

    fn discard(&mut self) {
        // `copy_work` deliberately survives: see its doc.
        self.chunks = VecDeque::new();
    }

    /// Insert a received range, coalescing with everything it overlaps or
    /// touches.
    ///
    /// # The merge is small-to-large
    ///
    /// **[RATIFIED 2026/08/17 — ruling 253(i)]** §10.6: *"total copy work
    /// per stream MUST be O(that stream's advertised credit · log credit) —
    /// every stored byte is copied O(log) times across its lifetime … never
    /// once per bridging frame."* The merge this replaced allocated the
    /// merged span and copied everything into it, so a peer alternating
    /// **bridging** inserts — one new byte joining two stored ranges — bought
    /// a span-sized copy per frame and sustained a measured **916×** receiver
    /// work per wire byte, inside flow credit, on the driver every co-hosted
    /// connection shares.
    ///
    /// The discipline, and why it bounds the work:
    ///
    /// - The **base** is the largest stored chunk in the merge. Its bytes
    ///   never move within the merged range, and its allocation is the one
    ///   the merged chunk keeps.
    /// - Every other stored chunk is copied into the base. Stored chunks are
    ///   pairwise disjoint, so the merged range contains the base *and* the
    ///   copied chunk side by side: a chunk of `s` bytes is only ever copied
    ///   into a range of at least `2s`. A stored byte therefore at least
    ///   **doubles the range it lives in each time it is copied**, so it is
    ///   copied at most log₂(credit) times in its life — never once per
    ///   bridging frame.
    /// - The arriving frame is written **only where no stored chunk holds
    ///   the byte**, so a retransmission that merely bridges pays for its new
    ///   bytes and not for the span it lands in. New bytes are bounded by
    ///   flow credit and each offset is new at most once, so that term is
    ///   linear in credit.
    /// - Reallocation inside a chunk is geometric, at
    ///   [`REASSEMBLY_SLACK_SHIFT`]'s eighth, and sums to a constant factor
    ///   on the same total.
    ///
    /// Total: O(credit · log credit) per stream, which is §10.6's bound.
    ///
    /// # Coverage lemma
    ///
    /// Chunks `lo..hi` are exactly those that overlap **or touch** the
    /// arriving range, so `[start, stop)` is covered contiguously by the
    /// frame together with those chunks, and every point of it outside a
    /// stored chunk lies inside the frame. That is what lets the two fill
    /// loops below write a gap straight out of the frame without checking:
    /// a gap between two consecutive participants is, by construction, frame
    /// bytes. It is also why the merged chunk stays non-adjacent to its new
    /// neighbours — `chunks[lo - 1].end() < start` and
    /// `chunks[hi].offset > stop` both follow from the same two scans.
    ///
    /// # A frame that adds no byte does no work
    ///
    /// **[F3]** A frame lying **wholly inside** already-received offset space
    /// is free to the peer twice over: `check_stream` accepts it
    /// (`end <= high_water` violates nothing) and it charges `delta = 0` flow
    /// credit, so neither §10's ledger nor §17.5's memory bound sees it. It
    /// stays **legal** — it is ordinary retransmission, and rejecting it as a
    /// violation would kill connections over routine loss recovery — so only
    /// the work is refused, never the packet.
    ///
    /// **What this early return is worth has changed, and the doc says so
    /// rather than inheriting the old claim** (working rule 4). Against the
    /// whole-span merge it was load-bearing: the covered frame paid a
    /// span-sized allocation and copy for zero new bytes, and the guarantee
    /// it bought was *every allocation is paid for by at least one byte that
    /// is new to the buffer*. Against the small-to-large merge the covered
    /// case already costs nothing — `lo` is the base, `start == base.offset`
    /// and `stop == base.end()`, so `reserve_span` finds both ends satisfied
    /// and the fill loops have no gap to write. What survives is the cheaper
    /// half: the `max_by_key` scan and the chunk-list churn are skipped, and
    /// the invariant is now structural rather than defended by this branch.
    ///
    /// **One stored chunk is the whole test, and that is the load-bearing
    /// lemma.** Chunks are pairwise disjoint *and* non-adjacent (this
    /// function merges on adjacency, not merely on overlap). A range covered
    /// by the union of two or more stored chunks would need them to touch,
    /// which the invariant forbids — so covered-by-the-union **is**
    /// covered-by-one-chunk, and the one chunk it can be is the first with
    /// `end() >= offset`, which the `lo` scan already finds.
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

        // **[F3]** Nothing new: return before touching the chunk list. See
        // the lemma in this function's doc comment for why `lo` is the only
        // chunk that can cover the range.
        if self
            .chunks
            .get(lo)
            .is_some_and(|c| c.offset <= offset && end <= c.end())
        {
            return Ok(());
        }

        let mut hi = lo;
        while hi < self.chunks.len() && self.chunks[hi].offset <= end {
            hi += 1;
        }

        if lo == hi {
            // Disjoint: one exact-capacity allocation for the arriving
            // bytes and nothing more.
            self.chunks.insert(lo, Chunk::from_frame(offset, data));
            self.copy_work += data.len() as u64;
        } else {
            let start = self.chunks[lo].offset.min(offset);
            let stop = self.chunks[hi - 1].end().max(end);

            // Small-to-large: the largest stored chunk keeps its bytes and
            // its allocation; everything else moves into it.
            let base_at = (lo..hi)
                .max_by_key(|&k| self.chunks[k].len())
                .expect("lo < hi");
            let mut base = std::mem::replace(&mut self.chunks[base_at], Chunk::vacant());
            let (base_off, base_end) = (base.offset, base.end());
            let mut work = base.reserve_span(start, stop);

            // Fill the prefix region `[start, base_off)`, ascending: each
            // stored chunk onto its own bytes, each gap between them out of
            // the frame.
            let mut cur = start;
            for c in self.chunks.range(lo..base_at) {
                if cur < c.offset {
                    work += base.write_frame(cur, c.offset, offset, data);
                }
                work += base.write_chunk(c);
                cur = c.end();
            }
            if cur < base_off {
                work += base.write_frame(cur, base_off, offset, data);
            }

            // And the suffix region `[base_end, stop)`, the same way.
            let mut cur = base_end;
            for c in self.chunks.range(base_at + 1..hi) {
                if cur < c.offset {
                    work += base.write_frame(cur, c.offset, offset, data);
                }
                work += base.write_chunk(c);
                cur = c.end();
            }
            if cur < stop {
                work += base.write_frame(cur, stop, offset, data);
            }
            self.copy_work += work;

            // Put the base back where it stood and drop the chunks it
            // swallowed. Removing the ones below it slides it to `lo`, which
            // is where a range starting at `start` belongs — and when the
            // base is already `lo` and nothing else merged (the ordinary
            // in-order append) this is zero deque work.
            self.chunks[base_at] = base;
            for _ in lo..base_at {
                self.chunks.remove(lo);
            }
            for _ in base_at + 1..hi {
                self.chunks.remove(lo + 1);
            }
        }

        // §10.6: *"would exceed `REASSEMBLY_CHUNKS_MAX` (= 1024) **after
        // coalescing**"*, so the count is checked here and not before.
        if self.chunks.len() > constants::REASSEMBLY_CHUNKS_MAX {
            return Err(Violation::Reassembly);
        }
        Ok(())
    }

    /// Drain the contiguous prefix starting at `at`.
    ///
    /// The head gap [`Chunk`] carries for the merge pays a second time here:
    /// handing bytes to the application advances `head` instead of shifting
    /// the tail down, so a large chunk read out in small reads is linear
    /// rather than quadratic. Emptying the chunk drops it, which is what
    /// returns its capacity to ruling 94's accounting.
    fn read(&mut self, at: u64, buf: &mut [u8]) -> usize {
        let Some(front) = self.chunks.front_mut() else {
            return 0;
        };
        if front.offset != at {
            return 0;
        }
        let n = buf.len().min(front.len());
        buf[..n].copy_from_slice(&front.bytes()[..n]);
        front.advance(n);
        if front.len() == 0 {
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
    ///
    /// **[ruling 253(ii)] The capacity is re-derived, not relaxed.** It read
    /// `2_000` — an exact number, and a property of the whole-span
    /// `vec![0u8; span]` this merge replaces: that merge reallocated to the
    /// exact span on every one of the 2 000 inserts, which is precisely the
    /// per-frame copy 253(i) removes. Under [`REASSEMBLY_SLACK_SHIFT`]'s
    /// capped growth the ladder is deterministic and lands at **2 104**: the
    /// chunk reallocates only when its eighth of slack is spent, to
    /// `need + need/8` each time. Relaxing this to `<=` would be rule 9's
    /// trap — every degenerate build satisfies an upper bound for free, and
    /// this is the one assertion that catches an accounting regression.
    #[test]
    fn adjacent_ranges_coalesce_rather_than_accumulate() {
        let mut half = RecvHalf::new();
        for i in 0..2_000u64 {
            half.apply_stream(i, b"x", false)
                .expect("adjacent ranges coalesce into one");
        }
        assert_eq!(half.capacity(), 2_104);
    }

    /// **[ruling 253(ii)]** §10.6's ceiling, held at every span rather than
    /// at one lucky point: allocated capacity never exceeds the arrived span
    /// by more than an eighth.
    ///
    /// Separating, which is the whole reason it is an invariant over the
    /// ladder and not a single `assert_eq!`. A bare doubling policy — the
    /// one ruling 253 declines, at ~1.5 × credit — passes at span 2 000
    /// (it holds 2 048, under 2 250) and fails here at span 1 025, where it
    /// holds 2 048 against a limit of 1 153.
    #[test]
    fn capped_growth_holds_capacity_within_an_eighth_of_the_span() {
        let mut half = RecvHalf::new();
        for i in 0..3_000u64 {
            half.apply_stream(i, b"x", false)
                .expect("inside the window");
            let span = i + 1;
            assert!(
                half.capacity() <= span + span / 8,
                "capacity {} exceeds an eighth over the {span}-byte span",
                half.capacity(),
            );
        }
    }

    /// **[ruling 253(i)]** The bridging case, from the separating side: a
    /// frame that extends a large stored range pays for **its own bytes**,
    /// not for the range it lands in.
    ///
    /// Derived rather than observed. The 1 000-byte range costs 1 000 to
    /// store. The first append finds no tail slack, so it reallocates —
    /// 1 000 stored bytes moved plus the 1 new byte — and takes
    /// `1001 / 8 = 125` bytes of slack with it. The next 99 appends fit in
    /// that slack and cost 1 byte each. Total **2 100**; capacity **1 126**.
    ///
    /// The merge this replaced copied the whole merged span every time:
    /// `1000 + sum(1000 + i for i in 1..=100)` = **106 050**, fifty times
    /// more, for the same hundred bytes of wire. That is the shape of the
    /// measured 916×.
    #[test]
    fn an_appending_bridge_copies_the_new_byte_and_not_the_range_it_extends() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, &[7u8; 1_000], false).unwrap();
        assert_eq!(half.copy_work(), 1_000, "the arriving bytes, once");
        for i in 0..100u64 {
            half.apply_stream(1_000 + i, b"x", false).unwrap();
        }
        assert_eq!(half.copy_work(), 2_100);
        assert_eq!(half.capacity(), 1_126);
        assert_eq!(drain(&mut half).len(), 1_100, "and the bytes are all there");
    }

    /// **[ruling 253(i)]** The same bridge from **below**, which is the head
    /// gap's whole reason to exist: without it, prepending one byte to a
    /// large range shifts the range, and the amplification comes back
    /// mirrored onto the front.
    ///
    /// The arithmetic is the append case's mirror image — one reallocation
    /// moving 1 000 bytes, then 99 prepends into the 125 bytes of head slack
    /// it took — so the total is the same **2 100**. A merge that prepended
    /// by shifting would pay ~1 000 per frame and land near 101 000.
    #[test]
    fn a_prepending_bridge_copies_the_new_byte_too() {
        let mut half = RecvHalf::new();
        half.apply_stream(1_000, &[7u8; 1_000], false).unwrap();
        for i in 1..=100u64 {
            half.apply_stream(1_000 - i, b"x", false).unwrap();
        }
        assert_eq!(half.copy_work(), 2_100);
        assert_eq!(half.capacity(), 1_126);
        assert_eq!(
            drain(&mut half).len(),
            0,
            "byte 0 never arrived, so nothing is contiguous — which is what \
             makes this the attack and not a transfer"
        );
    }

    /// **[ruling 253]** The meter is what the peer **spent**, so a reset
    /// does not hand it back. Ruling 94's `capacity` is a state accessor and
    /// goes to zero; `copy_work` is a work accessor and does not.
    #[test]
    fn a_reset_returns_the_capacity_and_not_the_copy_work() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, &[7u8; 200], false).unwrap();
        assert_eq!(half.capacity(), 200);
        assert_eq!(half.copy_work(), 200);
        half.apply_reset(500, 42);
        assert_eq!(half.capacity(), 0, "§12.7: the discard is the first moment");
        assert_eq!(
            half.copy_work(),
            200,
            "a peer that resets has still spent the work"
        );
    }

    /// **[F3]** A frame wholly inside already-received space writes nothing
    /// and allocates nothing.
    ///
    /// Stated as the property rather than as a test of the early return,
    /// because under ruling 253's merge the two are no longer the same
    /// thing: the covered frame reaches `reserve_span` with both ends
    /// already satisfied and both fill loops with no gap, so it costs
    /// nothing on either path. The early return is now the cheap
    /// short-circuit rather than the defence — see `insert`'s doc.
    #[test]
    fn a_covered_frame_writes_nothing() {
        let mut half = RecvHalf::new();
        half.apply_stream(0, b"abcdef", false).unwrap();
        let (cap, work) = (half.capacity(), half.copy_work());
        half.apply_stream(2, b"cd", false).unwrap();
        half.apply_stream(0, b"abcdef", false).unwrap();
        half.apply_stream(5, b"f", false).unwrap();
        assert_eq!(half.capacity(), cap, "no allocation");
        assert_eq!(half.copy_work(), work, "and no copy");
        assert_eq!(drain(&mut half), b"abcdef");
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

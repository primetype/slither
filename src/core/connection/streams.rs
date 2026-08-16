//! §9.1–§9.7 — the four ID spaces, implicit opening, the closed-stream
//! watermarks, and §9.7's garbage collection.
//!
//! # The handle key is not the wire id
//!
//! **[RATIFIED 2026/08/15 — ruling 95]** §16.4 returns a `StreamId` from
//! `open()`; §16.9 says wire ids *"are assigned at establishment"* and
//! `id()` *"returns `None` until the connection is established"*. Both
//! cannot be implemented literally, because a `connect()`-created connection
//! is writable before install — that is §16.9's whole point. The verbs are
//! therefore keyed by an opaque [`StreamRef`] that is **stable across
//! install**, and [`Streams::stream_id`] is §16.9's accessor.
//!
//! The trap the ruling closes is not the missing `Option`: it is a core that
//! returns an internal index *typed as* `StreamId` and remaps at install,
//! leaving every live handle holding a stale key — a build that passes a
//! pre-establishment test and a post-establishment test and fails only "open
//! early, write late".
//!
//! Nothing is remapped here. A stream we open takes the next index in *our*
//! space; which of §9.1's four spaces that is depends only on the opener
//! bit, which the install supplies. The index is allocated in open order and
//! never moves, so [`stream_id`] is a pure function of the entry plus the
//! role.
//!
//! [`stream_id`]: Streams::stream_id
//!
//! # Two tombstones, not one
//!
//! §9.2's watermark is per **space** and covers an index whose whole stream
//! is fully closed. A **bidi** stream whose receive half is freed while its
//! send half lives is not fully closed, so the watermark cannot cover it —
//! ruling 93's amendment. [`RecvTombstone`] is the second mechanism, per
//! **half**. Watermark alone resurrects an abandoned bidi receive half on
//! the next frame; the per-half tombstone alone never advances the uni
//! watermark, so the peer never earns its MAX_STREAMS credit back.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::constants;
use crate::error::{ReadError, WriteError};

use super::ConnEvent;
use super::flow::{Flow, Violation};
use super::frame::{self, Frame, Packing, STREAM_FILL_QUANTUM};
use super::recovery::SentFrame;
use super::recv::{ReadOutcome, RecvHalf, RecvTombstone};
use super::send::SendHalf;
use super::stream_id::{Dir, Opener, StreamId};
use crate::core::Role;

/// The core's stream handle key — stable across install, unlike a wire
/// [`StreamId`], which §16.9 cannot assign until establishment.
///
/// Opaque: nothing in the protocol is keyed on its value and it never
/// appears on the wire. `Ord` is derived beyond ruling 95's list so the
/// stream table can be a `BTreeMap` and every event burst has a
/// deterministic order — §16.4 makes output ordering normative.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct StreamRef(u64);

/// §16.4's `open()` error.
///
/// **[RATIFIED 2026/08/15 — ruling 101]** `pub(crate)`: §18.1's taxonomy is
/// closed (ruling 61) and §16.2 specifies that `open_bi`/`open_uni` *"wait
/// for MAX_STREAMS allowance when the cumulative limit is exhausted"* — the
/// shell converts this into a park, so no public verb can ever return it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the cumulative stream limit is exhausted")]
pub(crate) struct StreamsExhausted;

/// One of §9.1's four spaces.
#[derive(Default)]
struct SpaceTable {
    /// Currently-open indices.
    open: BTreeMap<u64, StreamRef>,
    /// §9.2's closed-stream watermark: the highest **fully closed** index.
    ///
    /// **All four are maintained** (ruling 97). §9.2 states the
    /// construction for four spaces and writes its rationale for the two
    /// peer-opened ones; the other two serve §8.4's separate rule that
    /// *"credit for a fully-closed stream … is a valid no-op"*. An
    /// implementer who keeps two meets a `STREAM_STATE_ERROR` where §8.4
    /// promised a no-op.
    watermark: Option<u64>,
    /// §10.4 counts **streams ever opened**, so this is also the next index
    /// to allocate in a space we open.
    ever_opened: u64,
}

impl SpaceTable {
    /// §9.2: an index *"at or below the watermark and not currently open"*.
    fn is_tombstoned(&self, index: u64) -> bool {
        self.watermark.is_some_and(|w| index <= w) && !self.open.contains_key(&index)
    }
}

/// One stream: up to two halves, and whatever the freed ones left behind.
struct Stream {
    dir: Dir,
    index: u64,
    /// Whether *we* opened it. With the role, this fixes the §9.1 space.
    local: bool,
    send: Option<SendHalf>,
    recv: Option<RecvHalf>,
    /// Ruling 93's second tombstone, left when a receive half is freed on a
    /// stream that is **not** fully closed.
    recv_tomb: Option<RecvTombstone>,
    /// Whether this stream is still in [`Streams::unclaimed`].
    ///
    /// A **mirror of the deque's membership**, kept because §9.8 asks the
    /// question once per inbound STREAM frame and the deque is bounded by
    /// §10.4's *cumulative* limit, not a concurrent count — so a linear
    /// membership test is a peer-controlled cost per frame (`PLAN-6.md` §9
    /// R3). Written in exactly the three places the deque is: the implicit
    /// open that pushes it, [`Streams::accept`], and §9.8's claim. Full
    /// closure removes the entry, so there is no fourth.
    unclaimed: bool,
}

impl Stream {
    fn new(dir: Dir, index: u64, local: bool) -> Self {
        // §9.1: a uni stream has one half at each end — the opener sends.
        let (send, recv) = match (dir, local) {
            (Dir::Bi, _) => (Some(SendHalf::new()), Some(RecvHalf::new())),
            (Dir::Uni, true) => (Some(SendHalf::new()), None),
            (Dir::Uni, false) => (None, Some(RecvHalf::new())),
        };
        Self {
            dir,
            index,
            local,
            send,
            recv,
            recv_tomb: None,
            unclaimed: false,
        }
    }

    /// §9.7: *"a stream is **fully closed** when its halves (one for uni,
    /// two for bidi) are freed"*.
    ///
    /// **H2**: this is a *local*, per-endpoint predicate. The two ends reach
    /// it at different moments and for different reasons, and modelling it
    /// as a shared property of a stream builds a synchronisation that does
    /// not exist.
    fn is_fully_closed(&self) -> bool {
        self.send.is_none() && self.recv.is_none()
    }
}

/// §8.7's `regenerate` class, as a set of frame **identities**.
///
/// Slice 5's loss-recovery seam (SPEC §13): slice 4 populates it and drives
/// it from state changes only.
/// The *value* is never stored — it is read off the ledger at pack time, so
/// a re-queued identity carries *"the freshest current value"* by
/// construction rather than by remembering to refresh it.
#[derive(Default)]
struct Regenerate {
    max_data: bool,
    max_streams: [bool; 2],
    max_stream_data: BTreeSet<StreamRef>,
}

/// What one §8.5 packing pass took out of the stream state.
///
/// It serves **two** purposes, and they are the same information:
///
/// 1. §13.5's sent-packet record. [`Packed::sent_frames`] turns it into
///    §8.7's frame identities, with ruling 113's FIN flag **carried** off
///    the chunk that actually held it rather than inferred from the final
///    size.
/// 2. §14.5's undo log. The admission gate is evaluated *after* the
///    plaintext is packed — a candidate's datagram size is not knowable
///    before — so a packet the congestion window refuses must put back
///    exactly what building it took out, and [`Streams::restore`] does that
///    from this record.
///
/// Deriving the sent record from the *frames* instead would lose two things
/// the identities need: a `StreamId` does not name a [`StreamRef`], and
/// `Chunk::fresh` (ruling 98's marking test) is not recoverable from a
/// `Frame::Stream` at all.
#[derive(Debug, Default)]
pub(crate) struct Packed {
    max_data: bool,
    max_streams: [bool; 2],
    max_stream_data: Vec<StreamRef>,
    resets: Vec<StreamRef>,
    chunks: Vec<TakenChunk>,
}

/// One STREAM frame's worth of stream state that the fill removed.
#[derive(Debug)]
struct TakenChunk {
    r: StreamRef,
    range: std::ops::Range<u64>,
    fin: bool,
    /// **Ruling 98.** Whether this was a first transmission — the thing that
    /// decides `seal` against `seal_quiet`, and the thing a `return_chunk`
    /// must preserve so a refused first transmission is not silently
    /// reclassified as a retransmission.
    fresh: bool,
}

impl Packed {
    /// Forget everything: the packet was sealed and committed.
    pub(crate) fn clear(&mut self) {
        self.max_data = false;
        self.max_streams = [false; 2];
        self.max_stream_data.clear();
        self.resets.clear();
        self.chunks.clear();
    }

    /// Whether anything at all was taken.
    pub(crate) fn is_empty(&self) -> bool {
        !self.max_data
            && !self.max_streams.iter().any(|owed| *owed)
            && self.max_stream_data.is_empty()
            && self.resets.is_empty()
            && self.chunks.is_empty()
    }

    /// §13.5's *"frame identities aboard"*, in §8.5's packing order.
    ///
    /// **May be empty**, and that is not a defect: a bare-PING PTO probe is
    /// ack-eliciting and tracked (§13.5, §17.5) while carrying nothing that
    /// re-queues.
    pub(crate) fn sent_frames(&self) -> Vec<SentFrame> {
        let mut out = Vec::new();
        if self.max_data {
            out.push(SentFrame::MaxData);
        }
        for dir in Dir::ALL {
            if self.max_streams[dir.slot()] {
                out.push(SentFrame::MaxStreams { dir });
            }
        }
        for r in &self.max_stream_data {
            out.push(SentFrame::MaxStreamData { r: *r });
        }
        for r in &self.resets {
            out.push(SentFrame::ResetStream { r: *r });
        }
        for chunk in &self.chunks {
            out.push(SentFrame::Stream {
                r: chunk.r,
                range: chunk.range.clone(),
                fin: chunk.fin,
            });
        }
        out
    }
}

/// The four-space stream table.
pub(crate) struct Streams {
    /// **[ruling 106]** `None` until install; §9.1's parity reads it and
    /// never re-derives it from "I was created by `connect()`".
    role: Option<Role>,
    /// Spaces we open, by direction.
    local: [SpaceTable; 2],
    /// Spaces the peer opens, by direction.
    remote: [SpaceTable; 2],
    entries: BTreeMap<StreamRef, Stream>,
    next_ref: u64,
    /// §16.4's *"backpressure by retention"*: peer-opened streams awaiting
    /// `accept(dir)`.
    ///
    /// Slice 6's message seam (SPEC §9.8): a **queue of unclaimed halves**,
    /// not "the newest one" —
    /// §9.8 adds a second claim verb drawing from the same supply.
    unclaimed: [VecDeque<StreamRef>; 2],
    /// §8.5's round-robin rotation over streams with pending data.
    rotation: VecDeque<StreamRef>,
    owed: Regenerate,
    /// §9.8's *"while a `recv_message()` claim is pending"*.
    ///
    /// **[ruling 156a]** Set by a `recv_message()` — the claim is being made
    /// at that instant, which is the second half of §9.8's own trigger — and
    /// cleared by one that returns `Some`. Never cleared by a dropped
    /// future: the core cannot observe that, and once an application has
    /// demonstrated message mode a concurrent `accept_uni()` user is
    /// committing ruling 51's programming error anyway.
    message_claim_pending: bool,
    /// §9.8's window-full unclaimed uni streams, **maintained incrementally
    /// rather than scanned**.
    ///
    /// §9.8 applies the overflow check to *"every unclaimed window-full
    /// stream"*, and the obvious implementation walks [`unclaimed`] on every
    /// `recv_message()` **and on every inbound STREAM frame while a claim is
    /// pending**. That deque is bounded by the *cumulative* MAX_STREAMS_UNI
    /// (§10.4), not by a concurrent count, so a peer flooding one-byte
    /// streams inside the connection window makes it very long and the walk
    /// quadratic in peer-controlled work — the amplification §10.6 and
    /// §11.3 both already refuse elsewhere (`PLAN-6.md` §9 R3).
    ///
    /// A stream **enters** when its high-water crosses `MESSAGE_RECV_MAX`
    /// with no final size pinned, and **leaves** when a final size is
    /// pinned (ruling 153's in-flight-FIN case), when `accept_uni()` claims
    /// it, when §9.8's claim takes it, or when the reset frees it. Every
    /// transition is O(log n) and none of them is per-frame work on a
    /// stream that is not itself at the bound.
    ///
    /// [`unclaimed`]: Self::unclaimed
    overflow_candidates: BTreeSet<StreamRef>,
    /// §9.8's receiver-emitted resets, **retained past the half they name**.
    ///
    /// *"retained and regenerated until acknowledged despite the retired
    /// half"* — and the half is retired at once, taking its whole entry with
    /// it, because a peer-opened uni stream has only that one half and
    /// freeing it fully closes the stream. So this cannot live in
    /// [`SendHalf`]'s `ResetState`: there is no send half to put it in, and
    /// `ResetState` is terminated by the half being freed, which is exactly
    /// what has already happened here.
    retained_resets: BTreeMap<StreamRef, RetainedReset>,
}

/// One §9.8 overflow reset, outliving the stream it names.
///
/// §8.7 puts RESET_STREAM in the `regenerate` class and §8.7's *"the
/// discard termination never applies to it"* is why `acked` is the only
/// terminal: a lost one re-owes itself, and nothing but an acknowledgement
/// removes it.
#[derive(Debug, Clone, Copy)]
struct RetainedReset {
    /// The wire id, captured **before** the entry was freed —
    /// [`Streams::stream_id`] reads `entries`, and the entry is about to go.
    id: StreamId,
    /// `MESSAGE_OVERFLOW` (§15.3, ruling 52). Held rather than assumed so
    /// the frame is built from recorded state and not from a constant at
    /// the pack site.
    error_code: u64,
    /// §9.6's field: the highest received offset. Informational — the
    /// sender already knows what it sent.
    final_size: u64,
    /// Whether a transmission is owed. A packet too full to carry it defers
    /// rather than drops it.
    pending: bool,
}

impl Streams {
    pub(crate) fn new() -> Self {
        Self {
            role: None,
            local: Default::default(),
            remote: Default::default(),
            entries: BTreeMap::new(),
            next_ref: 0,
            unclaimed: Default::default(),
            rotation: VecDeque::new(),
            owed: Regenerate::default(),
            message_claim_pending: false,
            overflow_candidates: BTreeSet::new(),
            retained_resets: BTreeMap::new(),
        }
    }

    /// **[ruling 106]** The role, fixed at install.
    pub(crate) fn role(&self) -> Option<Role> {
        self.role
    }

    pub(crate) fn set_role(&mut self, role: Role) {
        self.role = Some(role);
    }

    /// Ruling 94's accounting, summed across every live receive half.
    pub(crate) fn reassembly_capacity(&self) -> u64 {
        self.entries
            .values()
            .filter_map(|s| s.recv.as_ref())
            .map(RecvHalf::capacity)
            .sum()
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.4's verbs
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `open`. Legal before establishment (§16.9).
    pub(crate) fn open(&mut self, dir: Dir, flow: &Flow) -> Result<StreamRef, StreamsExhausted> {
        let index = self.local[dir.slot()].ever_opened;
        // §10.4: *"opening stream index `i` requires cumulative limit >
        // `i`"*.
        if index >= flow.remote_max_streams(dir) {
            return Err(StreamsExhausted);
        }
        let r = self.alloc();
        self.local[dir.slot()].ever_opened += 1;
        self.local[dir.slot()].open.insert(index, r);
        self.entries.insert(r, Stream::new(dir, index, true));
        Ok(r)
    }

    /// §16.9's accessor: the wire id, once establishment has fixed parity.
    pub(crate) fn stream_id(&self, r: StreamRef) -> Option<StreamId> {
        let stream = self.entries.get(&r)?;
        let opener = self.opener(stream.local)?;
        Some(StreamId::new(stream.index, stream.dir, opener))
    }

    /// §16.4's `accept`: claim one peer-opened stream of `dir`.
    ///
    /// **FIFO, in open order.** §9.2's implicit opening can open six streams
    /// from one frame and the order was unstated; FIFO matches
    /// `recv_message`'s *"oldest complete unclaimed"* and keeps §9.8's
    /// second claim verb, which draws from this same supply, in a defined
    /// order.
    pub(crate) fn accept(&mut self, dir: Dir) -> Option<StreamRef> {
        let r = self.unclaimed[dir.slot()].pop_front()?;
        // §9.8: *"Streams claimed by `accept_uni()` are untouched: real
        // streams extend credit normally."* Leaving it a candidate would
        // let a later `recv_message()` reset a stream the application is
        // already reading.
        self.overflow_candidates.remove(&r);
        if let Some(stream) = self.entries.get_mut(&r) {
            stream.unclaimed = false;
        }
        Some(r)
    }

    /// §16.4's `recv_message`: claim the oldest **complete unclaimed** uni
    /// stream as one payload, then free the stream (§9.8).
    ///
    /// *"Oldest"* is **open order**, not completion order (ruling 112): the
    /// walk is over [`unclaimed`](Self::unclaimed) in queue order and takes
    /// the first *complete* entry, so a complete stream sitting behind an
    /// incomplete one **is** surfaced. Messages are reliable-**unordered**
    /// and that is what the word means.
    ///
    /// §9.8's overflow scan runs on **every** call, including one that
    /// returns `Some` — *"while a claim is pending, **and at the instant
    /// such a claim is made**"*.
    ///
    /// Draws from `unclaimed[Uni]` **only**. §9.8 is uni sugar, and
    /// §9.8's own advice makes bidi the safe alternative precisely because
    /// it never collides with the message supply.
    pub(crate) fn recv_message(&mut self, flow: &mut Flow) -> Option<Vec<u8>> {
        // Set **before** the scan: this call is the *"instant such a claim
        // is made"*, so the check runs on it whatever the answer turns out
        // to be.
        self.message_claim_pending = true;
        self.scan_overflow(flow);

        let r = *self.unclaimed[Dir::Uni.slot()].iter().find(|r| {
            self.entries
                .get(r)
                .and_then(|s| s.recv.as_ref())
                .is_some_and(RecvHalf::is_complete)
        })?;

        let payload = self
            .entries
            .get_mut(&r)
            .and_then(|s| s.recv.as_mut())
            .and_then(RecvHalf::take_message)
            .expect("the walk selected a complete half");

        // §10.6: consumption is the application taking bytes **out of the
        // connection core**, and this is that moment for a message.
        // Stream-level credit is deliberately not re-granted — see
        // [`RecvHalf::take_message`].
        let consumed = payload.len() as u64;
        if consumed > 0 {
            flow.recv_window().consume(consumed);
            if flow.recv_window().take_grant().is_some() {
                self.owed.max_data = true;
            }
        }

        self.unclaimed[Dir::Uni.slot()].retain(|&q| q != r);
        self.overflow_candidates.remove(&r);
        if let Some(stream) = self.entries.get_mut(&r) {
            stream.unclaimed = false;
        }
        // §10.3's *"surfaced as a message (§9.8)"* retirement. The half is
        // already drained, so the true-up finds `counted == final_size` and
        // adds nothing; what it does do is free the half, which fully
        // closes a peer-opened uni stream and owes the peer +1
        // MAX_STREAMS_UNI (§10.4).
        self.retire_recv(r, flow);

        // **[ruling 156a]** A successful claim clears the flag. The failure
        // mode of this choice is a bounded delay before the next check, not
        // a reset the application did not earn.
        self.message_claim_pending = false;
        Some(payload)
    }

    /// The final size a send half has pinned, if any — read by the ACK
    /// entry points to tell whether an acknowledged range carried the FIN.
    pub(crate) fn final_size(&self, r: StreamRef) -> Option<u64> {
        self.entries
            .get(&r)?
            .send
            .as_ref()
            .and_then(SendHalf::final_size)
    }

    /// §16.4's `write`. `Ok(0)` is *blocked*, not *finished*.
    pub(crate) fn write(
        &mut self,
        r: StreamRef,
        data: &[u8],
        flow: &mut Flow,
    ) -> Result<usize, WriteError> {
        let room = flow.send_room();
        let stream = self.entries.get_mut(&r).ok_or(WriteError::Finished)?;
        let send = stream.send.as_mut().ok_or(WriteError::Finished)?;
        let n = send.write(data, room)?;
        if n > 0 {
            flow.charge_send(n as u64);
            if !send.is_queued() {
                send.set_queued(true);
                self.rotation.push_back(r);
            }
        }
        Ok(n)
    }

    /// §16.4's `finish`.
    pub(crate) fn finish(&mut self, r: StreamRef) -> Result<(), WriteError> {
        let stream = self.entries.get_mut(&r).ok_or(WriteError::Finished)?;
        let send = stream.send.as_mut().ok_or(WriteError::Finished)?;
        send.finish()?;
        if !send.is_queued() {
            send.set_queued(true);
            self.rotation.push_back(r);
        }
        Ok(())
    }

    /// §16.4's `reset` — §9.6's sender-emitted RESET_STREAM.
    pub(crate) fn reset(&mut self, r: StreamRef, error_code: u64) {
        let Some(stream) = self.entries.get_mut(&r) else {
            return;
        };
        let Some(send) = stream.send.as_mut() else {
            return;
        };
        send.reset(error_code);
        if !send.is_queued() {
            send.set_queued(true);
            self.rotation.push_back(r);
        }
    }

    /// §16.4's `read`: drain the contiguous prefix.
    ///
    /// A half this endpoint does not hold — the receive side of a
    /// locally-opened uni stream, or a stream that no longer exists — reads
    /// as end-of-stream. There is no "wrong direction" error in §18.1's
    /// closed taxonomy (ruling 61) and inventing one is not available.
    pub(crate) fn read(
        &mut self,
        r: StreamRef,
        buf: &mut [u8],
        flow: &mut Flow,
    ) -> Result<Option<usize>, ReadError> {
        let Some(stream) = self.entries.get_mut(&r) else {
            return Ok(None);
        };
        let Some(recv) = stream.recv.as_mut() else {
            return Ok(None);
        };

        let (outcome, consumed) = recv.read(buf);
        if consumed > 0 {
            // §10.6: consumption is the application taking bytes **out of
            // the connection core**.
            flow.recv_window().consume(consumed);
            if recv.take_grant().is_some() {
                self.owed.max_stream_data.insert(r);
            }
            if flow.recv_window().take_grant().is_some() {
                self.owed.max_data = true;
            }
        }

        match outcome {
            ReadOutcome::Data(n) => Ok(Some(n)),
            ReadOutcome::End => {
                self.retire_recv(r, flow);
                Ok(None)
            }
            ReadOutcome::Reset(code) => {
                self.retire_recv(r, flow);
                Err(ReadError::Reset(code))
            }
        }
    }

    /// Abandon a receive half — §16.2's dropped `RecvStream`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 93]** It retires **at once**: freed,
    /// tombstoned, connection credit trued up in the same step. §16.2's
    /// mechanism — abandonment merely *arms* a later retirement — is false
    /// under its own final clause, because a sender stalled at the stream
    /// window sends no FIN and has no reason to reset, so four abandoned
    /// 256 KiB streams wedge the 1 MiB connection window for the
    /// connection's life.
    ///
    /// This entry point is **not in `CONTRACT-4a.md` §2's verb list** and is
    /// additive: ruling 93 is unimplementable without one, since the core is
    /// where the ledger lives and the handle is in the shell. See the
    /// implementation report.
    pub(crate) fn abandon_recv(&mut self, r: StreamRef, flow: &mut Flow) {
        self.retire_recv(r, flow);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Slice 5's ACK seam — defined here, wired to SPEC §12 there
    // ═══════════════════════════════════════════════════════════════════

    /// One acknowledged stream range. Nothing on the wire calls this in
    /// slice 4.
    pub(crate) fn on_ack_range(
        &mut self,
        r: StreamRef,
        range: std::ops::Range<u64>,
        fin: bool,
        flow: &mut Flow,
        events: &mut Vec<ConnEvent>,
    ) {
        let Some(stream) = self.entries.get_mut(&r) else {
            return;
        };
        let Some(send) = stream.send.as_mut() else {
            return;
        };
        send.on_ack_range(range, fin);
        if send.is_terminal() {
            stream.send = None;
            events.push(ConnEvent::StreamFinished { r });
            self.after_half_freed(r, flow);
        }
    }

    /// One lost stream range, returning to the pending set (§8.7 `ranges`).
    pub(crate) fn on_lost_range(&mut self, r: StreamRef, range: std::ops::Range<u64>, fin: bool) {
        let Some(stream) = self.entries.get_mut(&r) else {
            return;
        };
        let Some(send) = stream.send.as_mut() else {
            return;
        };
        send.on_lost_range(range, fin);
        if send.has_pending() && !send.is_queued() {
            send.set_queued(true);
            self.rotation.push_back(r);
        }
    }

    /// §9.6's RESET_STREAM acknowledged — `ResetRecvd`, and the send half is
    /// freed.
    pub(crate) fn on_reset_acked(&mut self, r: StreamRef, flow: &mut Flow) {
        // §9.8's retained reset: **acknowledgement is its only terminal**.
        // §8.7 — *"the discard termination never applies to it"* — so
        // nothing else, and certainly not the freeing of the half it names,
        // may remove it. The `StreamRef` stays a valid key here after
        // `entries` has stopped holding one.
        if self.retained_resets.remove(&r).is_some() {
            return;
        }
        let Some(stream) = self.entries.get_mut(&r) else {
            return;
        };
        let Some(send) = stream.send.as_mut() else {
            return;
        };
        send.on_reset_acked();
        if send.is_terminal() {
            stream.send = None;
            self.after_half_freed(r, flow);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // The receive path — ruling 97's order is normative
    // ═══════════════════════════════════════════════════════════════════

    /// §9.5's STREAM frame.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 97]** The order is
    /// **legality → watermark → limit → final size → flow control**, and it
    /// is normative because every one of these kills the connection: the
    /// only observable difference is the code on the wire, which is what
    /// Appendix B asserts on and what a peer's operator reads.
    ///
    /// Legality is first because for a locally-opened uni stream the peer
    /// may *never* send STREAM, so a frame naming a fully-closed local-uni
    /// index satisfies both §9.2's watermark no-op and §8.4's
    /// `STREAM_STATE_ERROR` — silent ACK versus kill. The watermark answers
    /// *which index*; the state error answers *who may send*. It is safe
    /// first only because it is decidable **without consulting the stream
    /// table** — §9.1's id encodes direction and opener, and with our role
    /// it is a total function of the id alone. **That is ruling 106's second
    /// consumer.**
    pub(crate) fn on_stream_frame(
        &mut self,
        f: &frame::Stream,
        flow: &mut Flow,
        events: &mut Vec<ConnEvent>,
    ) -> Result<(), Violation> {
        let Some(role) = self.role else {
            debug_assert!(false, "frames are applied only after the install");
            return Ok(());
        };
        // 1 — legality.
        self.check_peer_may_send(f.id, role)?;

        // 2, 3 — the watermark, then the limit and the opens it authorises.
        let Some(r) = self.locate(f.id, role, events, flow)? else {
            // §9.2: inert — *"processed as acknowledged, never re-opened"*.
            return Ok(());
        };

        let stream = self
            .entries
            .get_mut(&r)
            .expect("`locate` returns a ref into the table");

        if let Some(tomb) = stream.recv_tomb.as_ref() {
            // Ruling 93's amendment: the half is gone, arrivals are
            // discarded, and no further credit is consumed — but the
            // stream-level bound still runs against the frozen limit, which
            // is what stops an abandoned half becoming an unbounded sink.
            return tomb.check_stream(f.offset, f.data.len() as u64, f.fin);
        }
        let Some(recv) = stream.recv.as_mut() else {
            // A locally-opened bidi stream whose receive half never existed
            // is unreachable (§9.1 gives bidi both halves) and one that was
            // freed left a tombstone. Inert.
            return Ok(());
        };

        // 4, 5 — final size, then stream-level credit, then the connection
        // ledger, consulted exactly once and only for a frame already known
        // otherwise legal.
        let new_high = recv.check_stream(f.offset, f.data.len() as u64, f.fin)?;
        let delta = new_high.saturating_sub(recv.high_water());
        flow.check_recv_charge(delta)?;

        let was_complete = recv.is_complete();
        let charged = recv.apply_stream(f.offset, &f.data, f.fin)?;
        flow.charge_recv(charged);

        if recv.is_readable() {
            events.push(ConnEvent::StreamReadable { r });
        }

        // §9.8's seam. `MessageReadable` and `StreamReadable` are **both**
        // emitted for a completing message stream, and that is not a
        // duplicate signal: the wire carries no discriminator between the
        // two receive modes (ruling 51), so the core cannot know which verb
        // the application will use and owes a wake to whichever is parked.
        if self.note_message_progress(r, was_complete) {
            events.push(ConnEvent::MessageReadable);
        }
        // §9.8's second trigger point: *"while a `recv_message()` claim is
        // pending"*. Guarded twice over — `scan_overflow` returns at once
        // unless a claim is pending, and the candidate set is empty unless
        // some stream is actually at the bound.
        self.scan_overflow(flow);
        Ok(())
    }

    /// §9.6's RESET_STREAM.
    pub(crate) fn on_reset_stream(
        &mut self,
        f: &frame::ResetStream,
        flow: &mut Flow,
        events: &mut Vec<ConnEvent>,
    ) -> Result<(), Violation> {
        let Some(role) = self.role else {
            debug_assert!(false, "frames are applied only after the install");
            return Ok(());
        };
        self.check_peer_may_send(f.id, role)?;

        let Some(r) = self.locate(f.id, role, events, flow)? else {
            return Ok(());
        };

        let stream = self
            .entries
            .get_mut(&r)
            .expect("`locate` returns a ref into the table");

        if let Some(tomb) = stream.recv_tomb.as_ref() {
            return tomb.check_reset(f.final_size);
        }
        let Some(recv) = stream.recv.as_mut() else {
            return Ok(());
        };

        recv.check_reset(f.final_size)?;
        let delta = f.final_size.saturating_sub(recv.high_water());
        flow.check_recv_charge(delta)?;

        let (charged, newly) = recv.apply_reset(f.final_size, f.error_code);
        flow.charge_recv(charged);
        // §9.6 pins a final size too, and a reset half can never complete as
        // a message — either way it is no longer §9.8's window-full
        // candidate, and resetting a stream the peer has already reset would
        // be a frame owed to nobody.
        self.overflow_candidates.remove(&r);
        if newly {
            events.push(ConnEvent::StreamReset {
                r,
                error_code: f.error_code,
            });
        }
        Ok(())
    }

    /// §10.3's MAX_STREAM_DATA.
    ///
    /// SPEC §10.6: *"credit frames apply as O(1) monotone-max"* and **never open
    /// streams**. A receiver that lazily created a stream entry here would
    /// hand a peer unbounded allocation at four bytes per stream — and it is
    /// the natural implementation if the ledger is a map with an
    /// `entry().or_default()`.
    pub(crate) fn on_max_stream_data(
        &mut self,
        id: StreamId,
        max: u64,
        events: &mut Vec<ConnEvent>,
    ) -> Result<(), Violation> {
        let Some(role) = self.role else {
            debug_assert!(false, "frames are applied only after the install");
            return Ok(());
        };
        let local = self.is_local(id, role);
        // §8.4: MAX_STREAM_DATA *"for a stream the receiver of the frame
        // cannot send on"*. We send on any bidi stream, and on a uni stream
        // only if we opened it.
        if id.dir() == Dir::Uni && !local {
            return Err(Violation::StreamState);
        }

        let table = self.table(local, id.dir());
        // §8.4: *"Credit for a fully-closed stream (at or below the
        // watermark, §9.2) is a valid no-op."* This is the rule the two
        // **locally-opened** watermarks exist to serve (H3).
        if table.is_tombstoned(id.index()) {
            return Ok(());
        }
        // §8.4, QUIC's rule: credit for a stream **we** open that we have
        // not yet opened.
        if local && id.index() >= table.ever_opened {
            return Err(Violation::StreamState);
        }
        let Some(&r) = table.open.get(&id.index()) else {
            // A stream in the peer's space that the peer has not opened.
            // §8.4's second rule is scoped to *"a space the frame's receiver
            // opens"* and does not reach this; credit frames never open
            // streams, so it is inert. See the implementation report.
            return Ok(());
        };

        let stream = self
            .entries
            .get_mut(&r)
            .expect("an open index has a table entry");
        if stream
            .send
            .as_mut()
            .is_some_and(|send| send.on_max_stream_data(max))
        {
            events.push(ConnEvent::StreamWritable { r });
        }
        Ok(())
    }

    /// §10.3's MAX_DATA: wake every writer the connection window was
    /// holding.
    pub(crate) fn on_max_data(&mut self, raised: bool, events: &mut Vec<ConnEvent>) {
        if !raised {
            return;
        }
        for (r, stream) in self.entries.iter_mut() {
            if stream.send.as_mut().is_some_and(SendHalf::unblock) {
                events.push(ConnEvent::StreamWritable { r: *r });
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §8.5's packing stages
    // ═══════════════════════════════════════════════════════════════════

    /// Stage 2 — credit grants, then RESET_STREAM (§8.5's order).
    ///
    /// Every identity is cleared only once its frame has been accepted, so a
    /// full packet defers rather than drops it. The **value** comes from the
    /// ledger at this moment, which is §8.7's *"freshest current value"*.
    pub(crate) fn pack_control(
        &mut self,
        flow: &mut Flow,
        packing: &mut Packing,
        packed: &mut Packed,
    ) {
        if self.owed.max_data && packing.control(Frame::MaxData(flow.recv_advertised())) {
            self.owed.max_data = false;
            packed.max_data = true;
        }
        for dir in Dir::ALL {
            if !self.owed.max_streams[dir.slot()] {
                continue;
            }
            let max = flow.local_max_streams(dir);
            let frame = match dir {
                Dir::Bi => Frame::MaxStreamsBidi(max),
                Dir::Uni => Frame::MaxStreamsUni(max),
            };
            if packing.control(frame) {
                self.owed.max_streams[dir.slot()] = false;
                packed.max_streams[dir.slot()] = true;
            }
        }

        let owed: Vec<StreamRef> = self.owed.max_stream_data.iter().copied().collect();
        for r in owed {
            let Some(id) = self.stream_id(r) else {
                continue;
            };
            let Some(max) = self
                .entries
                .get(&r)
                .and_then(|s| s.recv.as_ref())
                .map(RecvHalf::advertised)
            else {
                // The half was freed before its grant went out: the identity
                // has nothing left to describe.
                self.owed.max_stream_data.remove(&r);
                continue;
            };
            if packing.control(Frame::MaxStreamData(frame::MaxStreamData { id, max })) {
                self.owed.max_stream_data.remove(&r);
                packed.max_stream_data.push(r);
            }
        }

        let resets: Vec<StreamRef> = self
            .entries
            .iter()
            .filter(|(_, s)| s.send.as_ref().is_some_and(SendHalf::reset_pending))
            .map(|(r, _)| *r)
            .collect();
        for r in resets {
            let Some(id) = self.stream_id(r) else {
                continue;
            };
            let Some((error_code, final_size)) = self
                .entries
                .get_mut(&r)
                .and_then(|s| s.send.as_mut())
                .and_then(SendHalf::take_reset)
            else {
                continue;
            };
            let frame = Frame::ResetStream(frame::ResetStream {
                id,
                error_code,
                final_size,
            });
            if packing.control(frame) {
                packed.resets.push(r);
            } else {
                // Put the identity back: §8.7's regenerate class re-emits
                // until acknowledged, and a full packet is not an ack.
                if let Some(send) = self.entries.get_mut(&r).and_then(|s| s.send.as_mut()) {
                    send.on_reset_lost();
                }
            }
        }

        // §9.8's receiver-emitted resets, whose halves are already gone.
        // Same stage and same discipline as the loop above: the identity is
        // cleared only once its frame is accepted, so a full packet defers
        // rather than drops it.
        let retained: Vec<StreamRef> = self
            .retained_resets
            .iter()
            .filter(|(_, state)| state.pending)
            .map(|(r, _)| *r)
            .collect();
        for r in retained {
            let Some(state) = self.retained_resets.get(&r).copied() else {
                continue;
            };
            let frame = Frame::ResetStream(frame::ResetStream {
                id: state.id,
                error_code: state.error_code,
                final_size: state.final_size,
            });
            if packing.control(frame) {
                if let Some(state) = self.retained_resets.get_mut(&r) {
                    state.pending = false;
                }
                packed.resets.push(r);
            }
        }
    }

    /// Stage 3 — §8.5's STREAM fill, round-robin, one quantum per stream per
    /// pass.
    ///
    /// Returns whether any **first-transmission** STREAM frame was packed —
    /// ruling 98's marking test, which belongs to the seal and not to the
    /// frame.
    pub(crate) fn fill(&mut self, packing: &mut Packing, packed: &mut Packed) -> bool {
        let mut fresh_any = false;
        // One full rotation at most per call keeps this bounded even if a
        // half re-queues itself; the caller loops to build further packets.
        let mut budget = self.rotation.len().saturating_mul(2) + 2;

        while budget > 0 {
            budget -= 1;
            let Some(r) = self.rotation.pop_front() else {
                break;
            };
            let Some(id) = self.stream_id(r) else {
                // §16.9: nothing is emitted before install. Keep the place
                // in the rotation.
                self.rotation.push_front(r);
                break;
            };

            let mut defer = false;
            let mut requeue = false;
            {
                let Some(stream) = self.entries.get_mut(&r) else {
                    continue;
                };
                let Some(send) = stream.send.as_mut() else {
                    continue;
                };
                if !send.has_pending() {
                    send.set_queued(false);
                    continue;
                }
                let Some(offset) = send.next_offset() else {
                    send.set_queued(false);
                    continue;
                };
                let room = packing.stream_payload_room(id, offset).unwrap_or(0);
                if room == 0 && send.has_data_pending() {
                    defer = true;
                } else if let Some(chunk) = send.next_chunk(room.min(STREAM_FILL_QUANTUM)) {
                    let (at, fin, fresh) = (chunk.offset, chunk.fin, chunk.fresh);
                    let len = chunk.data.len() as u64;
                    let frame = Frame::Stream(frame::Stream::new(id, at, chunk.data, fin));
                    if packing.fill(frame) {
                        fresh_any |= fresh;
                        packed.chunks.push(TakenChunk {
                            r,
                            range: at..at + len,
                            fin,
                            fresh,
                        });
                        if send.has_pending() {
                            requeue = true;
                        } else {
                            send.set_queued(false);
                        }
                    } else {
                        debug_assert!(false, "stream_payload_room over-promised");
                        send.return_chunk(at..at + len, fin, fresh);
                        defer = true;
                    }
                } else {
                    send.set_queued(false);
                }
            }

            if defer {
                self.rotation.push_front(r);
                break;
            }
            if requeue {
                self.rotation.push_back(r);
            }
        }

        fresh_any
    }

    /// Whether anything is owed on the wire.
    pub(crate) fn has_output(&self) -> bool {
        self.owed.max_data
            || self.owed.max_streams.iter().any(|owed| *owed)
            || !self.owed.max_stream_data.is_empty()
            || !self.rotation.is_empty()
            || self
                .entries
                .values()
                .any(|s| s.send.as_ref().is_some_and(SendHalf::reset_pending))
            || self.retained_resets.values().any(|state| state.pending)
    }

    /// Put back everything one packing pass took — §14.5's refused packet.
    ///
    /// §14.5 gates the **send**, and the contract is that *"a packet that
    /// does not fit is not sealed and its frames stay pending"*. The gate
    /// cannot run earlier than this: `candidate_size` is the full datagram
    /// (ruling 136) and is not knowable until the plaintext exists.
    ///
    /// The chunks go back through
    /// [`return_chunk`](SendHalf::return_chunk) and **not** through
    /// [`on_lost_range`](SendHalf::on_lost_range): a chunk the window
    /// refused was never transmitted, so the loss path would reclassify a
    /// first transmission as a retransmission and quiet a seal that ruling
    /// 98 makes marking.
    pub(crate) fn restore(&mut self, packed: &mut Packed) {
        if packed.max_data {
            self.owed.max_data = true;
        }
        for dir in Dir::ALL {
            if packed.max_streams[dir.slot()] {
                self.owed.max_streams[dir.slot()] = true;
            }
        }
        for r in packed.max_stream_data.drain(..) {
            self.owed.max_stream_data.insert(r);
        }
        for r in std::mem::take(&mut packed.resets) {
            // §9.8's retained resets go back through the same door: the
            // identity was cleared when it was packed, and a refused packet
            // is not an acknowledgement.
            self.on_reset_lost(r);
        }
        // Reverse order so a half's own chunks unwind exactly as they were
        // taken. `RangeSet` coalesces, so the result is order-independent —
        // the reversal is for the `fin_sent` flag, which is not.
        while let Some(chunk) = packed.chunks.pop() {
            if let Some(send) = self.entries.get_mut(&chunk.r).and_then(|s| s.send.as_mut()) {
                send.return_chunk(chunk.range, chunk.fin, chunk.fresh);
            }
            self.requeue(chunk.r);
        }
        packed.clear();
    }

    /// §8.7 `regenerate`: a lost RESET_STREAM re-queues its identity, and
    /// the retransmission re-reads the **current** value.
    pub(crate) fn on_reset_lost(&mut self, r: StreamRef) {
        // §9.8's retained reset re-owes itself. An implementer who edits
        // only the send-half path below ships a reset that is never
        // re-emitted, and the sender's stream wedges for the connection's
        // life — which is the failure §9.8's *"even when the reset itself is
        // lost"* clause exists to forbid.
        if let Some(state) = self.retained_resets.get_mut(&r) {
            state.pending = true;
            return;
        }
        if let Some(send) = self.entries.get_mut(&r).and_then(|s| s.send.as_mut()) {
            send.on_reset_lost();
        }
        self.requeue(r);
    }

    /// §8.7 `regenerate`: a lost MAX_DATA re-owes the identity. The
    /// retransmission carries the **freshest** advertised value, not the
    /// lost one.
    pub(crate) fn owe_max_data(&mut self) {
        self.owed.max_data = true;
    }

    /// §8.7 `regenerate`: a lost MAX_STREAM_DATA re-owes the identity.
    ///
    /// If the half has since been freed, `pack_control` drops the identity
    /// on its next pass — there is nothing left for it to describe.
    pub(crate) fn owe_max_stream_data(&mut self, r: StreamRef) {
        self.owed.max_stream_data.insert(r);
    }

    /// §8.7 `regenerate`: a lost MAX_STREAMS re-owes the identity.
    pub(crate) fn owe_max_streams(&mut self, dir: Dir) {
        self.owed.max_streams[dir.slot()] = true;
    }

    /// Every send half's current write offset — §16.2's snapshot input.
    ///
    /// A half with no bytes written appears at offset 0, which
    /// [`send_settled`](Self::send_settled) answers `true` for vacuously:
    /// nothing was handed to the connection.
    pub(crate) fn send_offsets(&self) -> Vec<(StreamRef, u64)> {
        self.entries
            .iter()
            .filter_map(|(r, s)| s.send.as_ref().map(|send| (*r, send.write_offset())))
            .collect()
    }

    /// §16.2's settled test for one `(stream, offset)` snapshot entry.
    ///
    /// A stream absent from the table, or whose send half has been freed,
    /// is **settled**: a send half is freed only at `DataRecvd` or
    /// `ResetRecvd` (§9.7), i.e. acknowledged or abandoned.
    pub(crate) fn send_settled(&self, r: StreamRef, offset: u64) -> bool {
        match self.entries.get(&r).and_then(|s| s.send.as_ref()) {
            Some(send) => send.settled_to(offset),
            None => true,
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Internals
    // ═══════════════════════════════════════════════════════════════════

    /// §9.8's overflow check, over every unclaimed window-full stream.
    ///
    /// A no-op unless a claim is pending — *"the guard is deliberate, and
    /// the unguarded form is worse: a receiver in stream mode that is merely
    /// slow to call `accept_uni()` is exercising §16.4's
    /// backpressure-by-retention, and resetting its stream the moment the
    /// sender filled the initial window would break an ordinary lazy accept
    /// loop."*
    ///
    /// *"Every unclaimed window-full stream, not merely the oldest — a
    /// stream sitting behind a slower one must not evade it"* is delivered
    /// by draining the whole candidate set, and the set is what keeps that
    /// from being a walk of `unclaimed`.
    fn scan_overflow(&mut self, flow: &mut Flow) {
        if !self.message_claim_pending || self.overflow_candidates.is_empty() {
            return;
        }
        let victims: Vec<StreamRef> = self.overflow_candidates.iter().copied().collect();
        for r in victims {
            self.reset_for_overflow(r, flow);
        }
    }

    /// Emit §9.8's one receiver-emitted RESET_STREAM for `r`.
    ///
    /// The order is load-bearing: the wire id is read **before** anything is
    /// freed, because [`stream_id`](Self::stream_id) reads `entries` and the
    /// entry does not survive the retirement two lines below.
    fn reset_for_overflow(&mut self, r: StreamRef, flow: &mut Flow) {
        self.overflow_candidates.remove(&r);
        let Some(id) = self.stream_id(r) else {
            return;
        };
        let Some(final_size) = self
            .entries
            .get(&r)
            .and_then(|s| s.recv.as_ref())
            .map(RecvHalf::high_water)
        else {
            return;
        };

        // Ruling 59 is a **MUST**, and this is the only trace either end
        // gets that names the *cause*: the sender learns `MESSAGE_OVERFLOW`
        // and nothing else, and the receiver is the end whose verb choice
        // created the conflict. All three of §18.2's fields — the stream,
        // its final size, and the mode conflict — are named.
        tracing::warn!(
            target: "slither::frames",
            stream = id.as_u64(),
            final_size,
            error_code = constants::MESSAGE_OVERFLOW,
            "unclaimed uni stream reached MESSAGE_RECV_MAX with no FIN; \
             resetting it (§9.8) — this connection is consuming uni streams \
             as messages while the peer is writing one as an incremental \
             stream (ruling 51's mixing error)"
        );

        if let Some(stream) = self.entries.get_mut(&r) {
            stream.unclaimed = false;
        }
        // §9.8: *"The receive half retires (its bytes count as consumed at
        // the connection level, §10.3)"*. It has no final size and never
        // will, so ruling 93's true-up target is the advertised window —
        // which is what releases the connection credit the stall was
        // holding.
        self.retire_recv(r, flow);

        self.retained_resets.insert(
            r,
            RetainedReset {
                id,
                error_code: constants::MESSAGE_OVERFLOW,
                final_size,
                pending: true,
            },
        );
    }

    /// §9.8's seam on the receive path: what an arriving STREAM frame does
    /// to a peer-opened uni stream's message state. All O(1).
    ///
    /// Returns whether the stream **became** a complete message on this
    /// frame — the one-per-item discipline ruling 99 fixed for
    /// `StreamOpened`, applied to `MessageReadable`. A duplicate frame on an
    /// already-complete stream signals nothing, and neither does a stream
    /// `accept_uni()` has claimed.
    fn note_message_progress(&mut self, r: StreamRef, was_complete: bool) -> bool {
        let Some(stream) = self.entries.get(&r) else {
            return false;
        };
        if stream.dir != Dir::Uni || stream.local || !stream.unclaimed {
            return false;
        }
        let Some(recv) = stream.recv.as_ref() else {
            return false;
        };

        if recv.final_size().is_some() {
            // **Ruling 153's other half, on the receiving side.** A final
            // size makes the stream surfaceable, so it is no longer a
            // candidate however much of the window it consumed — which is
            // what stops a conforming 262 144-byte message being reset the
            // moment its last frame lands.
            self.overflow_candidates.remove(&r);
            return !was_complete && recv.is_complete();
        }
        // §9.8's predicate: an unclaimed uni stream that *"consumes its full
        // initial window without pinning a final size"*. **Ruling 153**: the
        // quantity is the **highest received offset**, not the contiguous
        // reassembled prefix — the contiguous reading never fires when a
        // middle byte is lost, which is exactly when the sender is stalled
        // at the window and needs rescuing (ruling 51's permanent stall,
        // reintroduced).
        if recv.high_water() >= constants::MESSAGE_RECV_MAX {
            self.overflow_candidates.insert(r);
        }
        false
    }

    /// Put a half back in §8.5's rotation if it has anything owed.
    fn requeue(&mut self, r: StreamRef) {
        let Some(send) = self.entries.get_mut(&r).and_then(|s| s.send.as_mut()) else {
            return;
        };
        if (send.has_pending() || send.reset_pending()) && !send.is_queued() {
            send.set_queued(true);
            self.rotation.push_back(r);
        }
    }

    fn alloc(&mut self) -> StreamRef {
        let r = StreamRef(self.next_ref);
        self.next_ref += 1;
        r
    }

    fn opener(&self, local: bool) -> Option<Opener> {
        let ours = Opener::of_role(self.role?);
        Some(if local { ours } else { ours.peer() })
    }

    /// Whether this id names a stream **we** open. A total function of the
    /// id and the role — no table lookup (ruling 97).
    fn is_local(&self, id: StreamId, role: Role) -> bool {
        id.opener() == Opener::of_role(role)
    }

    fn table(&mut self, local: bool, dir: Dir) -> &mut SpaceTable {
        if local {
            &mut self.local[dir.slot()]
        } else {
            &mut self.remote[dir.slot()]
        }
    }

    /// Ruling 97's step 1: could the peer have sent this frame at all?
    fn check_peer_may_send(&self, id: StreamId, role: Role) -> Result<(), Violation> {
        // The peer may send STREAM/RESET_STREAM on any bidi stream, and on a
        // uni stream only if the peer opened it.
        if id.dir() == Dir::Uni && self.is_local(id, role) {
            return Err(Violation::StreamState);
        }
        Ok(())
    }

    /// Ruling 97's steps 2 and 3: the watermark, then §10.4's limit and the
    /// implicit opens it authorises.
    ///
    /// `Ok(None)` is §9.2's inert frame. Every newly-opened stream emits its
    /// **own** `StreamOpened` (ruling 99), and the limit check runs *before*
    /// the opens — which is what bounds the burst at §10.4's cumulative
    /// limit instead of 2⁶⁰.
    fn locate(
        &mut self,
        id: StreamId,
        role: Role,
        events: &mut Vec<ConnEvent>,
        flow: &Flow,
    ) -> Result<Option<StreamRef>, Violation> {
        let local = self.is_local(id, role);
        let dir = id.dir();
        let index = id.index();

        if self.table(local, dir).is_tombstoned(index) {
            return Ok(None);
        }
        if let Some(&r) = self.table(local, dir).open.get(&index) {
            return Ok(Some(r));
        }
        if local {
            // Our own space: implicit opening is the *opener's* mechanism,
            // so an index we have never allocated is a frame the peer could
            // not have sent.
            return Err(Violation::StreamState);
        }

        // §10.4: *"opening stream index `i` requires cumulative limit >
        // `i`"*. **Ruling 99**: before the opens, so a frame above the
        // limit emits **zero** events and kills the connection.
        if index >= flow.local_max_streams(dir) {
            return Err(Violation::StreamLimit);
        }

        // §9.2: opening `N` opens *"every lower-numbered not-yet-open stream
        // of that space"* — above the watermark only.
        let from = self.table(local, dir).ever_opened;
        for i in from..=index {
            let r = self.alloc();
            let table = self.table(local, dir);
            table.open.insert(i, r);
            table.ever_opened = i + 1;
            let mut stream = Stream::new(dir, i, false);
            stream.unclaimed = true;
            self.entries.insert(r, stream);
            self.unclaimed[dir.slot()].push_back(r);
            events.push(ConnEvent::StreamOpened { dir });
        }
        Ok(self.table(local, dir).open.get(&index).copied())
    }

    /// Free a receive half and run §10.3's retirement true-up.
    fn retire_recv(&mut self, r: StreamRef, flow: &mut Flow) {
        let Some(stream) = self.entries.get_mut(&r) else {
            return;
        };
        let Some(recv) = stream.recv.take() else {
            return;
        };
        // §10.3's true-up is **absolute**: it advances this stream's
        // contribution *to* a value and never adds on top of the bytes reads
        // already counted.
        let target = recv.retirement_target();
        let delta = target.saturating_sub(recv.counted());
        if delta > 0 {
            flow.recv_window().consume(delta);
            if flow.recv_window().take_grant().is_some() {
                self.owed.max_data = true;
            }
        }
        // §10.3: *"stream-level credit is simply never re-granted for a
        // retired stream"* — so any pending grant identity is dropped.
        self.owed.max_stream_data.remove(&r);
        stream.recv_tomb = Some(recv.tombstone());
        self.after_half_freed(r, flow);
    }

    /// §9.7's full-closure check, run whenever a half is freed.
    fn after_half_freed(&mut self, r: StreamRef, flow: &mut Flow) {
        let Some(stream) = self.entries.get(&r) else {
            return;
        };
        if !stream.is_fully_closed() {
            // Ruling 93's amendment: **not** fully closed, so the watermark
            // does not advance and the index stays in the open set. The
            // per-half tombstone is what makes later frames inert.
            return;
        }
        let (dir, index, local) = (stream.dir, stream.index, stream.local);

        // §9.2: freeing is what advances the closed-stream watermark.
        let table = self.table(local, dir);
        table.open.remove(&index);
        table.watermark = Some(table.watermark.map_or(index, |w| w.max(index)));
        self.entries.remove(&r);
        self.unclaimed[dir.slot()].retain(|&q| q != r);
        self.rotation.retain(|&q| q != r);

        // §10.4: the peer earns a grant only for a **peer-opened** stream —
        // closing streams we opened must not inflate the peer's allowance.
        if !local {
            flow.grant_stream_credit(dir);
            let peer_opened = self.table(local, dir).ever_opened;
            if flow.take_streams_grant(dir, peer_opened).is_some() {
                self.owed.max_streams[dir.slot()] = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants;
    use crate::core::connection::frame::Packing;

    /// A table that has installed as the **acceptor**, so the peer is §9.1's
    /// connection initiator and opens the `Initiator` spaces.
    fn acceptor() -> (Streams, Flow) {
        let mut streams = Streams::new();
        streams.set_role(Role::Responder);
        (streams, Flow::new())
    }

    fn peer_uni(index: u64) -> StreamId {
        StreamId::new(index, Dir::Uni, Opener::Initiator)
    }

    fn peer_bidi(index: u64) -> StreamId {
        StreamId::new(index, Dir::Bi, Opener::Initiator)
    }

    fn stream_frame(id: StreamId, offset: u64, data: &[u8], fin: bool) -> frame::Stream {
        frame::Stream::new(id, offset, data.to_vec(), fin)
    }

    fn opened(events: &[ConnEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, ConnEvent::StreamOpened { .. }))
            .count()
    }

    /// **[ruling 99]** §9.2 opens `N` *"and every lower-numbered
    /// not-yet-open stream of that space"*, and each one gets its **own**
    /// event: `accept(dir)` returns one stream per call, so one event for six
    /// would force the shell to loop until `None` or lose five.
    #[test]
    fn an_implicit_open_of_six_streams_emits_six_stream_opened_events() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(5), 0, b"x", false),
                &mut flow,
                &mut events,
            )
            .expect("index 5 is inside the uni limit");
        assert_eq!(opened(&events), 6);

        // And all six are claimable, FIFO in open order.
        for index in 0..6u64 {
            let r = streams.accept(Dir::Uni).expect("claimable");
            assert_eq!(streams.stream_id(r).map(StreamId::index), Some(index));
        }
        assert_eq!(streams.accept(Dir::Uni), None);
    }

    /// **[ruling 99 + 97]** The limit check runs *before* the opens it would
    /// authorise, so a frame above §10.4's cumulative limit emits **zero**
    /// events and kills the connection. An implementation that opened first
    /// and validated after would emit 129 of them.
    #[test]
    fn a_frame_above_the_cumulative_limit_emits_zero_events() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        let over = constants::INITIAL_MAX_STREAMS_UNI; // index == limit is one too far
        assert_eq!(
            streams.on_stream_frame(
                &stream_frame(peer_uni(over), 0, b"x", false),
                &mut flow,
                &mut events
            ),
            Err(Violation::StreamLimit)
        );
        assert_eq!(opened(&events), 0);
        assert_eq!(streams.accept(Dir::Uni), None);

        // Two-sided: one below the limit is legal (H12).
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(over - 1), 0, b"x", false),
                &mut flow,
                &mut events,
            )
            .expect("index limit-1 is legal");
        assert_eq!(opened(&events), over as usize);
    }

    /// **[ruling 97]** Legality wins over the watermark and over the limit:
    /// for a locally-opened uni stream the peer may *never* send STREAM, and
    /// the answer is a total function of the id and the role.
    #[test]
    fn a_stream_frame_on_a_local_uni_space_is_a_state_error() {
        let (mut streams, mut flow) = acceptor();
        let ours = streams.open(Dir::Uni, &flow).expect("our uni stream");
        let id = streams.stream_id(ours).expect("established");

        let mut events = Vec::new();
        assert_eq!(
            streams.on_stream_frame(&stream_frame(id, 0, b"x", false), &mut flow, &mut events),
            Err(Violation::StreamState)
        );

        // And an index in that space we never opened is *also* a state
        // error, not a limit error: the limit bounds the peer's opens, and
        // this is not the peer's space.
        let never = StreamId::new(99, Dir::Uni, Opener::Responder);
        assert_eq!(
            streams.on_stream_frame(&stream_frame(never, 0, b"x", false), &mut flow, &mut events),
            Err(Violation::StreamState)
        );
        assert_eq!(opened(&events), 0);
    }

    /// **[ruling 100]** An empty, FIN-less STREAM frame opens its stream.
    /// Its "no-op" is about the data, not the open.
    #[test]
    fn an_empty_finless_stream_frame_opens_its_stream() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(0), 0, b"", false),
                &mut flow,
                &mut events,
            )
            .expect("legal");
        assert_eq!(opened(&events), 1);
        let r = streams.accept(Dir::Uni).expect("the empty frame opened it");
        // …and it delivered no bytes, pinned no final size, consumed no
        // credit.
        assert_eq!(streams.reassembly_capacity(), 0);
        assert_eq!(flow.recv_charged(), 0);
        let mut buf = [0u8; 4];
        assert_eq!(streams.read(r, &mut buf, &mut flow), Ok(Some(0)));
    }

    /// §9.2's tombstone, and Appendix B's obligation discharged by an
    /// **injected duplicate** (ruling 105): a frame naming a fully-closed
    /// index is inert — ACKed, never re-opened, no phantom `StreamOpened`.
    #[test]
    fn a_duplicate_frame_for_a_freed_stream_is_inert() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        let frame = stream_frame(peer_uni(0), 0, b"hi", true);
        streams
            .on_stream_frame(&frame, &mut flow, &mut events)
            .expect("legal");
        let r = streams.accept(Dir::Uni).expect("claimable");
        let mut buf = [0u8; 8];
        assert_eq!(streams.read(r, &mut buf, &mut flow), Ok(Some(2)));
        assert_eq!(
            streams.read(r, &mut buf, &mut flow),
            Ok(None),
            "read to final"
        );

        let mut events = Vec::new();
        streams
            .on_stream_frame(&frame, &mut flow, &mut events)
            .expect("§9.2: processed as acknowledged");
        assert_eq!(opened(&events), 0, "no phantom StreamOpened");
        assert_eq!(streams.accept(Dir::Uni), None, "never re-opened");
    }

    /// **[ruling 93]** The true-up value is the highest stream-level limit
    /// ever advertised, **not** the high-water mark — which leaks credit
    /// permanently and re-creates the wedge at smaller amplitude.
    #[test]
    fn abandonment_trues_up_to_the_stream_window_not_the_high_water_mark() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(0), 0, &[0u8; 1_000], false),
                &mut flow,
                &mut events,
            )
            .expect("legal");
        let r = streams.accept(Dir::Uni).expect("claimable");
        assert_eq!(flow.recv_window().consumed(), 0);

        streams.abandon_recv(r, &mut flow);
        assert_eq!(
            flow.recv_window().consumed(),
            constants::INITIAL_MAX_STREAM_DATA,
            "the high-water mark (1 000) would leak the rest for the connection's life"
        );
    }

    /// **[ruling 93, as amended]** Mechanism one: a **peer-opened uni**
    /// stream's receive half is the only half this endpoint holds, so
    /// abandoning it fully closes the stream — the watermark advances and
    /// the peer earns a MAX_STREAMS grant.
    #[test]
    fn abandoning_a_peer_opened_uni_half_advances_the_watermark_and_grants_credit() {
        let (mut streams, mut flow) = acceptor();
        let batch = constants::STREAMS_CREDIT_BATCH;
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(batch - 1), 0, b"x", false),
                &mut flow,
                &mut events,
            )
            .expect("legal");

        for _ in 0..batch {
            let r = streams.accept(Dir::Uni).expect("claimable");
            streams.abandon_recv(r, &mut flow);
        }

        // Later frames for those indices are inert by §9.2's watermark.
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_uni(0), 0, b"x", false),
                &mut flow,
                &mut events,
            )
            .expect("watermarked and inert");
        assert_eq!(opened(&events), 0);
        assert_eq!(streams.accept(Dir::Uni), None);

        // §10.4: full closure of a peer-opened stream is what earns credit,
        // and a whole batch of it is what emits the frame (ruling 102).
        let mut packing = Packing::new();
        streams.pack_control(&mut flow, &mut packing, &mut Packed::default());
        assert!(
            packing.frames().contains(&Frame::MaxStreamsUni(
                constants::INITIAL_MAX_STREAMS_UNI + batch
            )),
            "{:?}",
            packing.frames()
        );
        // The same batch also trued up 8 stream windows at the connection
        // level, which crosses §10.3's threshold — that MAX_DATA rides the
        // same packet and is `seal_quiet` like the rest (ruling 98).
        assert!(
            packing
                .frames()
                .iter()
                .any(|f| matches!(f, Frame::MaxData(_))),
            "{:?}",
            packing.frames()
        );
    }

    /// **[ruling 93, as amended]** Mechanism two: a **bidi** stream's send
    /// half is still live, so abandoning the receive half does *not* fully
    /// close it. The watermark must not advance — that would resurrect the
    /// stream on the next frame and re-charge the cumulative limit against
    /// freed state — and the per-half tombstone is what makes arrivals
    /// inert while still running the stream-level bound.
    #[test]
    fn abandoning_a_bidi_recv_half_tombstones_the_half_and_not_the_index() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_bidi(0), 0, b"abc", false),
                &mut flow,
                &mut events,
            )
            .expect("legal");
        let r = streams.accept(Dir::Bi).expect("claimable");
        streams.abandon_recv(r, &mut flow);

        // The index is still open: our send half lives, so §9.7's full
        // closure has not happened and no MAX_STREAMS credit is owed.
        let mut packing = Packing::new();
        streams.pack_control(&mut flow, &mut packing, &mut Packed::default());
        assert!(packing.frames().is_empty(), "not fully closed, no grant");

        // Arrivals are discarded — no re-open, no delivery, no further
        // connection-level charge…
        let charged = flow.recv_charged();
        let mut events = Vec::new();
        streams
            .on_stream_frame(
                &stream_frame(peer_bidi(0), 3, b"def", false),
                &mut flow,
                &mut events,
            )
            .expect("discarded, not an error");
        assert_eq!(opened(&events), 0, "the index was never resurrected");
        assert!(events.is_empty(), "delivered nowhere: {events:?}");
        assert_eq!(flow.recv_charged(), charged, "no further credit consumed");
        assert_eq!(streams.reassembly_capacity(), 0);

        // …but the stream-level bound still runs, against the frozen limit.
        // Without it an abandoned half is an unbounded sink.
        assert_eq!(
            streams.on_stream_frame(
                &stream_frame(
                    peer_bidi(0),
                    constants::INITIAL_MAX_STREAM_DATA,
                    b"x",
                    false
                ),
                &mut flow,
                &mut events
            ),
            Err(Violation::FlowControl)
        );
    }

    /// SPEC §10.6: *"credit frames never open streams"*. A receiver that lazily
    /// created an entry here would hand a peer unbounded allocation at four
    /// bytes per stream — and it is the natural implementation if the ledger
    /// is a map with an `entry().or_default()`.
    #[test]
    fn max_stream_data_never_opens_a_stream() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();

        // For a stream in **our** space that we have not opened: §8.4's
        // `STREAM_STATE_ERROR`.
        let ours = StreamId::new(0, Dir::Bi, Opener::Responder);
        assert_eq!(
            streams.on_max_stream_data(ours, 1_000, &mut events),
            Err(Violation::StreamState)
        );
        // For a uni stream the peer opened, which we cannot send on at all.
        assert_eq!(
            streams.on_max_stream_data(peer_uni(0), 1_000, &mut events),
            Err(Violation::StreamState)
        );
        // For a stream in the peer's bidi space the peer has not opened:
        // inert, and **no entry is created**.
        streams
            .on_max_stream_data(peer_bidi(7), 1_000, &mut events)
            .expect("inert");
        assert_eq!(streams.accept(Dir::Bi), None);
        assert_eq!(streams.reassembly_capacity(), 0);
        let _ = &mut flow;
    }

    /// **[ruling 94]** §10.6's memory bound, asserted on allocated
    /// **capacity** and not on bytes received: an eager per-stream allocator
    /// receives few bytes and would pass a bytes-received assertion for
    /// free, while costing 32 MiB against 1 MiB of credit.
    #[test]
    fn reassembly_capacity_stays_within_the_connection_window() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        let per_stream = 64usize;
        let count = constants::INITIAL_MAX_STREAMS_UNI;
        for index in 0..count {
            streams
                .on_stream_frame(
                    &stream_frame(peer_uni(index), 0, &vec![7u8; per_stream], false),
                    &mut flow,
                    &mut events,
                )
                .expect("inside every limit");
        }
        assert_eq!(opened(&events), count as usize);
        assert_eq!(
            streams.reassembly_capacity(),
            count * per_stream as u64,
            "capacity is what arrived"
        );
        assert!(
            streams.reassembly_capacity() <= constants::INITIAL_MAX_DATA,
            "§10.6: the connection window is the memory bound"
        );
        assert!(
            streams.reassembly_capacity() < count * constants::INITIAL_MAX_STREAM_DATA,
            "an eager allocator would be 32x this"
        );
    }

    /// §10.5's connection-level bound is enforced across streams, not just
    /// within one: the per-stream windows sum to 32 MiB and the connection
    /// window is 1 MiB.
    #[test]
    fn the_connection_bound_binds_before_the_stream_bound_across_streams() {
        let (mut streams, mut flow) = acceptor();
        let mut events = Vec::new();
        let chunk = constants::INITIAL_MAX_STREAM_DATA;
        let mut index = 0u64;
        loop {
            let outcome = streams.on_stream_frame(
                &stream_frame(peer_uni(index), 0, &vec![0u8; chunk as usize], false),
                &mut flow,
                &mut events,
            );
            match outcome {
                Ok(()) => index += 1,
                Err(v) => {
                    assert_eq!(v, Violation::FlowControl);
                    break;
                }
            }
            assert!(index < 16, "the 1 MiB connection window must bind first");
        }
        assert_eq!(index, constants::INITIAL_MAX_DATA / chunk);
    }
}

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

use crate::error::{ReadError, WriteError};

use super::flow::{Flow, Violation};
use super::frame::{self, Frame, Packing, STREAM_FILL_QUANTUM};
use super::recv::{ReadOutcome, RecvHalf, RecvTombstone};
use super::send::SendHalf;
use super::stream_id::{Dir, Opener, StreamId};
use super::ConnEvent;
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
/// §12.5's seam: slice 4 populates it and drives it from state changes only.
/// The *value* is never stored — it is read off the ledger at pack time, so
/// a re-queued identity carries *"the freshest current value"* by
/// construction rather than by remembering to refresh it.
#[derive(Default)]
struct Regenerate {
    max_data: bool,
    max_streams: [bool; 2],
    max_stream_data: BTreeSet<StreamRef>,
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
    /// §12.1's seam: a **queue of unclaimed halves**, not "the newest one" —
    /// §9.8 adds a second claim verb drawing from the same supply.
    unclaimed: [VecDeque<StreamRef>; 2],
    /// §8.5's round-robin rotation over streams with pending data.
    rotation: VecDeque<StreamRef>,
    owed: Regenerate,
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
        self.unclaimed[dir.slot()].pop_front()
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
    // §12.4's seam — defined here, wired to §12 by slice 5
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

        let charged = recv.apply_stream(f.offset, &f.data, f.fin)?;
        flow.charge_recv(charged);

        if recv.is_readable() {
            events.push(ConnEvent::StreamReadable { r });
        }
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
    /// §12.7: *"credit frames apply as O(1) monotone-max"* and **never open
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
        if let Some(send) = stream.send.as_mut() {
            if send.on_max_stream_data(max) {
                events.push(ConnEvent::StreamWritable { r });
            }
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
            if let Some(send) = stream.send.as_mut() {
                if send.unblock() {
                    events.push(ConnEvent::StreamWritable { r: *r });
                }
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
    pub(crate) fn pack_control(&mut self, flow: &mut Flow, packing: &mut Packing) {
        if self.owed.max_data && packing.control(Frame::MaxData(flow.recv_advertised())) {
            self.owed.max_data = false;
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
            if !packing.control(frame) {
                // Put the identity back: §8.7's regenerate class re-emits
                // until acknowledged, and a full packet is not an ack.
                if let Some(send) = self.entries.get_mut(&r).and_then(|s| s.send.as_mut()) {
                    send.on_reset_lost();
                }
            }
        }
    }

    /// Stage 3 — §8.5's STREAM fill, round-robin, one quantum per stream per
    /// pass.
    ///
    /// Returns whether any **first-transmission** STREAM frame was packed —
    /// ruling 98's marking test, which belongs to the seal and not to the
    /// frame.
    pub(crate) fn fill(&mut self, packing: &mut Packing) -> bool {
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
    }

    // ═══════════════════════════════════════════════════════════════════
    // Internals
    // ═══════════════════════════════════════════════════════════════════

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
            self.entries.insert(r, Stream::new(dir, i, false));
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

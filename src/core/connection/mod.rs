//! The connection core — §7's session, §8's frames, §15's teardown.
//!
//! `core::Connection` is a pure state machine: **`now: Instant` is an
//! argument on every mutating call and nothing here reads a clock**, and
//! every mutating call is followed by draining
//! [`poll_output`](Connection::poll_output) to the terminal
//! [`ConnOutput::Timeout`] (§16.4).
//!
//! # What this slice builds
//!
//! §7.1's counter (hiss's, never ours), §7.2's replay window, §7.4's two
//! seal paths and death clock, §7.7's ratchet (hiss's — the obligation is
//! *not* to chase epochs), §7.8's one-session rule, §7.9's exhaustion,
//! §8's codec for PADDING/PING/ACK/CLOSE, §15's three post-mortem states,
//! §16.5's timer table and §16.7's plan-seal-commit.
//!
//! Not here: §9's streams, §10's flow control, §11's datagrams, §12's ACK
//! *semantics* (the frame is codec-only), §13's recovery, §14's congestion
//! control, §7.5's keepalives and contested probe, §6.4/§6.5's roaming.
//! **This is a boundary, not a regression**, and it is stated so a reviewer
//! does not read the absence as one. As in slice 2a, the `ConnEvent`
//! variants those sections define are **absent rather than stubbed**: an
//! uninhabited variant is a claim about the protocol.
//!
//! # The receive path, in the one order §7.2 admits
//!
//! §3.1's gate → AEAD open → **replay check** → **replay mark** → the
//! §3.4 empty-plaintext keepalive short-circuit → parse the *whole*
//! plaintext → apply. A window check before the AEAD would let an off-path
//! forger who observed a cleartext counter poison the window; a
//! parse-and-apply loop would apply a valid frame that a later frame in the
//! same packet invalidates (§8.2).
//!
//! §8.2's structural failure keeps "the already-performed replay mark" and
//! applies nothing else — so the mark happens before the parse, and stays.

pub(crate) mod close;
pub(crate) mod flow;
pub(crate) mod frame;
pub(crate) mod recv;
pub(crate) mod send;
pub(crate) mod session;
pub(crate) mod stream_id;
pub(crate) mod streams;
pub(crate) mod timers;

// Slice 3a's acceptance tests, written independently from `SPEC.md` and
// `STORIES.md` by an author who never read this directory (working rule 6).
// Declared here at integration rather than by the implementer, so that
// neither agent could reach the other's file.
#[cfg(test)]
mod tests;

// Slice 4a's §9/§10 tests, written from `SPEC.md` and `CONTRACT-4a.md` in
// an isolated worktree by an author who never saw this slice's code — and
// whose 73 tests found three defects in the contract itself (Round 18).
// Declared here at integration, for the same reason as `tests` above.
#[cfg(test)]
mod tests_streams;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Instant;

use crate::constants;
use crate::error::{ConnectionLost, ReadError, WriteError};
use crate::packet::{Handshake, Inbound, classify};

use self::close::{Closing, Lifecycle};
use self::flow::Flow;
use self::frame::{Close, Frame, Packing, Structural};
use self::session::Session;
use self::timers::{TimerKind, Timers};

// §9.1's two public types (ruling 101). Fully `pub` so `core::mod.rs` can
// re-export them out of the `pub(crate)` core — a `pub(crate) use` here
// would make the outer re-export E0365.
pub use self::stream_id::{Dir, StreamId};
pub(crate) use self::streams::{StreamRef, Streams, StreamsExhausted};

use super::{EstablishedSession, Install, Role, ToEndpoint, Transmit};

/// A connection's core state machine. §16.4.
pub(crate) struct Connection<C: Handshake> {
    session: Option<Session<C>>,
    sub_seed: [u8; 32],
    outputs: VecDeque<ConnOutput>,
    installed: bool,
    lifecycle: Lifecycle,
    timers: Timers,
    /// §16.4's `Closed` is emitted **once** per connection: it is the value
    /// behind `closed()`'s latch, and a second one would be a contradictory
    /// `ConnectionLost` nobody can observe.
    closed_emitted: bool,
    /// The receive-path plaintext buffer, owned here so the session can be
    /// borrowed again while the frames are being applied.
    scratch: Vec<u8>,
    /// **[ruling 106]** `None` until the install; §9.1's stream-ID parity
    /// reads it, and the core cannot derive it (§6.6 step 4).
    role: Option<Role>,
    /// §9's four ID spaces and both halves of every open stream.
    streams: Streams,
    /// §10's two credit ledgers and the cumulative stream limits.
    flow: Flow,
    /// Events generated while a packet is being applied, drained into
    /// `outputs` before the transmits they cause (§16.4's generation order).
    events: Vec<ConnEvent>,
    /// §18.1's cause, retained so the stream verbs can surface
    /// `ConnectionLost` after the one `Closed` event has gone out.
    lost: Option<ConnectionLost>,
}

/// What a received, authenticated, window-fresh packet turned out to be.
enum Received {
    /// §3.4's empty plaintext: the keepalive. It never reaches §8's layer.
    Keepalive,
    /// A parsed frame stream, applying nothing yet (§8.2).
    Frames(Vec<Frame>),
    /// §8.2's structural failure class.
    Structural(Structural),
}

impl<C: Handshake> Connection<C> {
    /// A connection `connect()` created: no session until its `Install`
    /// arrives.
    pub(crate) fn connecting(sub_seed: [u8; 32]) -> Self {
        Self {
            session: None,
            sub_seed,
            outputs: VecDeque::new(),
            installed: false,
            lifecycle: Lifecycle::Live,
            timers: Timers::new(),
            closed_emitted: false,
            scratch: Vec::new(),
            role: None,
            streams: Streams::new(),
            flow: Flow::new(),
            events: Vec::new(),
            lost: None,
        }
    }

    /// A connection `accept()` returned: established on arrival, and
    /// **never** followed by an `Install` (§16.4).
    ///
    /// `now` is the install instant. §7.4 pins both liveness clocks to it
    /// and starts the death deadline **already armed** — the accept path
    /// has no later event to read that instant from, and the core may not
    /// read a clock, so the instant has to arrive here.
    pub(crate) fn established(
        now: Instant,
        sub_seed: [u8; 32],
        session: EstablishedSession<C>,
        role: Role,
    ) -> Self {
        let mut conn = Self::connecting(sub_seed);
        conn.install(now, session, role);
        conn
    }

    /// The §16.6 sub-seed. Unused until slice 4; see [`super`]'s docs.
    pub(crate) fn sub_seed(&self) -> &[u8; 32] {
        &self.sub_seed
    }

    /// Whether a session is installed.
    pub(crate) fn is_established(&self) -> bool {
        self.session.is_some()
    }

    /// The installed session, if any. `None` once the state is dropped.
    pub(crate) fn session(&self) -> Option<&EstablishedSession<C>> {
        self.session.as_ref().map(Session::established)
    }

    /// §7.2's replay window — also §12.2's received-packet record.
    pub(crate) fn replay(&self) -> Option<&session::ReplayWindow> {
        self.session.as_ref().map(Session::replay)
    }

    /// §7.4's liveness clocks.
    pub(crate) fn liveness(&self) -> Option<&session::Liveness> {
        self.session.as_ref().map(Session::liveness)
    }

    /// The counter the next successful seal will use (§7.1).
    ///
    /// Reading this after a mutating call and **before** any `poll_output`
    /// is how §16.7's synchronous sealing is observable at all: an
    /// implementation that sealed lazily inside the drain has not moved it.
    pub(crate) fn next_counter(&self) -> Option<u64> {
        self.session.as_ref().map(Session::next_counter)
    }

    /// The armed deadline for one of §16.5's named timers.
    pub(crate) fn timer(&self, kind: TimerKind) -> Option<Instant> {
        self.timers.get(kind)
    }

    /// §16.4's endpoint→connection event. `Install` only, **exactly
    /// once**: a second one is a driver bug and is ignored rather than
    /// replacing a live session (§7.8 — no transport state ever crosses a
    /// handshake, and there is no re-install path).
    pub(crate) fn handle_endpoint_event(&mut self, now: Instant, ev: Install<C>) {
        if self.installed {
            debug_assert!(false, "Install is delivered exactly once per connection");
            return;
        }
        if !self.lifecycle.is_live() {
            // Closed before its `Install` landed (§16.9's pre-establishment
            // `close()`). The shell drops such a core, but installing a
            // session into a connection that has already surfaced `Closed`
            // would resurrect it, and §16.4 emits `Closed` once.
            return;
        }
        self.install(now, ev.session, ev.role);
    }

    /// §16.4's `handle_datagram`.
    ///
    /// `src` is the datagram's source address. Slice 3 does not read it:
    /// §6.5's roaming is slice 7, and §15.2 is explicit that the closing
    /// state "does not roam; never to the triggering packet's source".
    pub(crate) fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) {
        let _ = src;

        if self.lifecycle.is_dead() {
            return;
        }

        // A `connect()`-created connection has no session and no
        // `receiver_index`, so nothing can be routed to it. Silent, per
        // §3.1's tier — this is not an error and never reaches anyone.
        if self.session.is_none() {
            return;
        }

        let received = {
            let Some(Inbound::Data {
                header,
                ad,
                ciphertext,
            }) = classify::<C>(datagram)
            else {
                return;
            };

            let session = self
                .session
                .as_mut()
                .expect("checked immediately above, and nothing between takes it");

            // AEAD, then the window check, then the mark, then liveness —
            // all inside `open`, because the order is the whole of §7.2.
            let Some(plaintext) =
                session.open(now, header.counter, ad, ciphertext, &mut self.scratch)
            else {
                return;
            };

            if plaintext.is_empty() {
                // §3.4: "An empty plaintext … is the keepalive — it
                // bypasses the frame layer entirely and is the only
                // non-frame plaintext." Parsing it as zero frames would be
                // indistinguishable here and wrong in slice 7.
                Received::Keepalive
            } else {
                match frame::parse(plaintext) {
                    Ok(frames) => Received::Frames(frames),
                    Err(error) => Received::Structural(error),
                }
            }
        };

        // The packet is authenticated and window-fresh; the borrow of the
        // plaintext is over, so state may move now.
        self.sync_liveness_timer();

        if self.lifecycle.is_live() {
            self.apply_live(now, received);
        } else {
            self.apply_post_mortem(now, received);
        }
    }

    /// §16.4's `handle_timeout`. **Idempotent** — every due deadline is
    /// stopped before its logic runs (§16.5), so a repeated call at one
    /// instant finds an empty due set.
    pub(crate) fn handle_timeout(&mut self, now: Instant) {
        if self.lifecycle.is_dead() {
            return;
        }

        for kind in self.timers.take_due(now).iter() {
            match kind {
                // §7.4: no authenticated fresh receive for `DEAD_TIMEOUT`
                // with an arming send outstanding. §15.4's first row —
                // nothing transmitted, and no linger to run.
                TimerKind::Liveness => self.die(ConnectionLost::TimedOut),
                // §15.2's linger expiry: drop all state. `Closed` was
                // emitted at the death (ruling 81); only `Retired` is owed.
                TimerKind::CloseLinger => self.drop_state(),
                // Armed by no path in this slice. Each arrives with the
                // section that defines it: §7.5 for `Contested`,
                // `Keepalive` and `PersistentKeepalive`; §13 for `Loss` and
                // `Pto`; §12.3 for `AckDelay`.
                TimerKind::Contested
                | TimerKind::Loss
                | TimerKind::Pto
                | TimerKind::AckDelay
                | TimerKind::Keepalive
                | TimerKind::PersistentKeepalive => {}
            }

            if self.lifecycle.is_dead() {
                break;
            }
        }

        // A timer can free state (§15) but arms nothing that owes a frame
        // in this slice. Pumping anyway keeps the invariant "every call
        // carrying an instant leaves nothing owed" true of the whole
        // surface rather than of most of it.
        self.pump(now);
    }

    /// §16.4's `close`. §15.2's local close.
    ///
    /// `reason` is truncated to `CLOSE_REASON_MAX` (§8.4) — §16.2 truncates
    /// at the handle, and this makes the over-length case unconstructible
    /// through the core as well.
    ///
    /// A second `close()`, or one on a connection already dying, is a no-op:
    /// the connection dies once and `Closed` is emitted once.
    pub(crate) fn close(&mut self, now: Instant, code: u64, reason: &[u8]) {
        if !self.lifecycle.is_live() {
            return;
        }

        if self.session.is_none() {
            // §16.9 makes pre-establishment work ordinary, and there is no
            // seal capability yet: no CLOSE can be emitted and there is
            // nothing to linger for. Ruling 50 takes the same position for
            // the analogous `Connecting` case — "an attempt that never
            // completed has no session to close and no wire signal to
            // send".
            //
            // No `Retired` either: it carries `our_index`, a **session**
            // index this connection has never had. See
            // `.slices/03-skeleton/IMPLEMENTATION.md` F1.
            self.emit_closed(ConnectionLost::LocallyClosed);
            self.lifecycle = Lifecycle::Dead;
            self.timers.disarm_all();
            return;
        }

        self.enter_closing(now, code, reason, ConnectionLost::LocallyClosed);
    }

    /// §16.4's drain. Terminates in [`ConnOutput::Timeout`], which is both
    /// the drain sentinel and the next-deadline announcement.
    ///
    /// Takes no `now` and seals nothing: §16.7 puts sealing inside the
    /// mutating call that triggers it, which is what makes `Loss`/`Pto`
    /// idempotency real.
    pub(crate) fn poll_output(&mut self) -> ConnOutput {
        self.outputs
            .pop_front()
            .unwrap_or_else(|| ConnOutput::Timeout(self.timers.next()))
    }

    /// T7's seal-failure injection: the next seal fails as §7.9's
    /// exhausted counter would.
    #[cfg(test)]
    pub(crate) fn fail_next_seal(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.fail_next_seal();
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §9/§10 — the stream surface (§16.4)
    // ═══════════════════════════════════════════════════════════════════

    /// **[ruling 106]** The role fixed at install. `None` before it.
    pub(crate) fn role(&self) -> Option<Role> {
        self.role
    }

    /// §16.4's `open`.
    ///
    /// Legal **before establishment** (§16.9): the returned [`StreamRef`] is
    /// usable at once, `write()` on it is ordinary work, and
    /// [`stream_id`](Self::stream_id) is `None` until a session installs.
    pub(crate) fn open(&mut self, dir: Dir) -> Result<StreamRef, StreamsExhausted> {
        self.streams.open(dir, &self.flow)
    }

    /// §16.9's accessor: the wire id, once establishment has fixed parity.
    pub(crate) fn stream_id(&self, r: StreamRef) -> Option<StreamId> {
        self.streams.stream_id(r)
    }

    /// §16.4's `accept`: claim one peer-opened stream of `dir`.
    pub(crate) fn accept(&mut self, dir: Dir) -> Option<StreamRef> {
        self.streams.accept(dir)
    }

    /// §16.4's `write`. `Ok(0)` means **blocked** by stream or connection
    /// credit — the shell parks; it is not end-of-stream.
    ///
    /// **`Ok(0)` means blocked only for a non-empty input.** `write(now, r,
    /// &[])` is a no-op that also returns `Ok(0)`, so a shell must check its
    /// own buffer length before parking, or it parks forever on an empty
    /// write of its own making.
    pub(crate) fn write(
        &mut self,
        now: Instant,
        r: StreamRef,
        data: &[u8],
    ) -> Result<usize, WriteError> {
        self.lost()?;
        let n = self.streams.write(r, data, &mut self.flow)?;
        self.pump(now);
        Ok(n)
    }

    /// §16.4's `finish`: no more data, and the FIN pins the final size.
    ///
    /// Takes `now` because it emits — a FIN-bearing STREAM frame, on the
    /// **marking** `seal` (§7.4, ruling 98) — and §16.7 puts sealing *"within
    /// the mutating call that triggers it"*, never lazily inside
    /// `poll_output()`, which has no instant.
    pub(crate) fn finish(&mut self, now: Instant, r: StreamRef) -> Result<(), WriteError> {
        self.lost()?;
        self.streams.finish(r)?;
        self.pump(now);
        Ok(())
    }

    /// §16.4's `reset` — §9.6's sender-emitted RESET_STREAM.
    pub(crate) fn reset(&mut self, now: Instant, r: StreamRef, error_code: u64) {
        self.streams.reset(r, error_code);
        self.pump(now);
    }

    /// §16.4's `read`: drain the contiguous prefix.
    ///
    /// `Ok(Some(0))` is **no data available** — the shell parks. `Ok(None)`
    /// is end of stream. Getting the two backwards hangs a reader forever on
    /// a finished stream.
    ///
    /// Takes `now` because it emits, for **two** independent reasons. §10.3
    /// makes consumption drive credit, so a read that crosses the re-grant
    /// threshold owes a MAX_STREAM_DATA and possibly a MAX_DATA; and a read
    /// that reaches the final size **retires the half** (§9.7), which for a
    /// peer-opened uni stream fully closes the stream and owes a MAX_STREAMS
    /// (§10.4). The second reason stands even if §10.3's re-grant never
    /// fires.
    pub(crate) fn read(
        &mut self,
        now: Instant,
        r: StreamRef,
        buf: &mut [u8],
    ) -> Result<Option<usize>, ReadError> {
        if let Some(lost) = self.lost.clone() {
            return Err(ReadError::ConnectionLost(lost));
        }
        let out = self.streams.read(r, buf, &mut self.flow);
        self.pump(now);
        out
    }

    /// Abandon a receive half — §16.2's dropped `RecvStream`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 93]** It retires **at once**: the
    /// half is freed, the connection-level contribution is trued up to the
    /// highest stream-level limit ever advertised, and the index is
    /// tombstoned by the mechanism its space requires — §9.2's watermark for
    /// a peer-opened uni stream, whose receive half is the only half this
    /// endpoint holds, and a per-half discard for a bidi stream, whose send
    /// half is still live.
    ///
    /// Takes `now` because the true-up can cross §10.3's threshold and can
    /// fully close a peer-opened uni stream, which grants MAX_STREAMS
    /// (§10.4).
    pub(crate) fn abandon_recv(&mut self, now: Instant, r: StreamRef) {
        self.streams.abandon_recv(r, &mut self.flow);
        self.pump(now);
    }

    /// **Ruling 94.** Total bytes of reassembly **capacity** currently
    /// allocated across every receive half.
    pub(crate) fn reassembly_capacity(&self) -> u64 {
        self.streams.reassembly_capacity()
    }

    /// Emit anything owed on the wire.
    ///
    /// **Additive**, and after the `now` correction it is a convenience
    /// rather than a necessity: every verb that can owe a frame now seals
    /// inside itself (§16.7). Kept because the two-core tests use it to make
    /// "nothing further is owed" assertable.
    pub(crate) fn flush(&mut self, now: Instant) {
        self.pump(now);
    }

    /// §12's ACK application for one stream range. **Uncalled from the
    /// wire** in slice 4 (§12.4's seam); slice 5 wires §12 to it.
    ///
    /// Takes `now` because an ACK can complete a send half, fully close a
    /// stream and owe a MAX_STREAMS grant (§10.4).
    ///
    /// Whether the acknowledged frame carried the **FIN** is inferred from
    /// `range.end == final_size`, because the signature carries no flag.
    /// That is exact for every frame this implementation emits — the FIN
    /// rides the frame that ends the stream and nothing else — but §8.7 lets
    /// a retransmission re-frame ranges freely, so slice 5 should carry the
    /// flag explicitly off its sent-packet map rather than re-derive it here.
    /// See the implementation report.
    pub(crate) fn on_ack_range(&mut self, now: Instant, r: StreamRef, range: std::ops::Range<u64>) {
        let fin = self.frame_carried_fin(r, &range);
        self.streams
            .on_ack_range(r, range, fin, &mut self.flow, &mut self.events);
        self.drain_events();
        self.pump(now);
    }

    /// §13's loss detection for one stream range: it returns to the pending
    /// set and is re-framed on a fresh counter (§8.7 `ranges`). **Uncalled
    /// from the wire** in slice 4 (§12.5's seam).
    pub(crate) fn on_lost_range(
        &mut self,
        now: Instant,
        r: StreamRef,
        range: std::ops::Range<u64>,
    ) {
        let fin = self.frame_carried_fin(r, &range);
        self.streams.on_lost_range(r, range, fin);
        self.pump(now);
    }

    fn frame_carried_fin(&self, r: StreamRef, range: &std::ops::Range<u64>) -> bool {
        self.streams.final_size(r) == Some(range.end)
    }

    /// §9.6's RESET_STREAM acknowledged. **Uncalled from the wire** in slice
    /// 4.
    pub(crate) fn on_reset_acked(&mut self, now: Instant, r: StreamRef) {
        self.streams.on_reset_acked(r, &mut self.flow);
        self.pump(now);
    }

    /// §18.1's cause, once the connection has died. The verbs surface it
    /// rather than accepting work a dead connection can never do.
    fn lost(&self) -> Result<(), WriteError> {
        match self.lost.clone() {
            Some(lost) => Err(WriteError::ConnectionLost(lost)),
            None => Ok(()),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Receive
    // ═══════════════════════════════════════════════════════════════════

    /// Apply a received packet to a **live** connection (§8.2).
    fn apply_live(&mut self, now: Instant, received: Received) {
        let frames = match received {
            Received::Keepalive => return,
            Received::Structural(error) => {
                // §8.2: one trace, then CLOSE(`PROTOCOL_VIOLATION`) and the
                // closing state. Nothing from the packet is applied — the
                // replay mark, already performed, is the sole exception.
                tracing::warn!(
                    target: "slither::frames",
                    error = %error,
                    "structural failure in the frame stream; closing"
                );
                self.enter_closing(
                    now,
                    constants::PROTOCOL_VIOLATION,
                    b"",
                    ConnectionLost::ProtocolViolation {
                        code: constants::PROTOCOL_VIOLATION,
                    },
                );
                return;
            }
            Received::Frames(frames) => frames,
        };

        for frame in frames {
            match frame {
                // §8.4: no fields, no error cases, no effect.
                Frame::Padding => {}
                // Ack-eliciting, so §12.3 owes an ACK — which is slice 5.
                // Nothing else: §13.4's probe accounting is slice 5 too.
                Frame::Ping => {}
                // Codec only in this slice. §12.4's processing — RTT
                // sampling, the sent-packet map, the loss evaluation — is
                // slice 5, and this arm is where it lands.
                Frame::Ack(_) => {}
                Frame::Stream(stream) => {
                    if let Err(violation) =
                        self.streams
                            .on_stream_frame(&stream, &mut self.flow, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::ResetStream(reset) => {
                    if let Err(violation) =
                        self.streams
                            .on_reset_stream(&reset, &mut self.flow, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::MaxData(max) => {
                    let raised = self.flow.on_max_data(max);
                    self.streams.on_max_data(raised, &mut self.events);
                }
                Frame::MaxStreamData(grant) => {
                    if let Err(violation) =
                        self.streams
                            .on_max_stream_data(grant.id, grant.max, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::MaxStreamsBidi(max) => self.on_max_streams(Dir::Bi, max),
                Frame::MaxStreamsUni(max) => self.on_max_streams(Dir::Uni, max),
                Frame::Close(close) => {
                    // §15.2: surface `PeerClosed`, emit **nothing**, hold a
                    // drain for `CLOSE_LINGER`, then drop all state.
                    self.emit_closed(ConnectionLost::PeerClosed {
                        code: close.code,
                        reason: close.reason,
                    });
                    self.lifecycle = Lifecycle::Draining {
                        until: now + constants::CLOSE_LINGER,
                    };
                    self.enter_post_mortem_timers();
                    // Whatever followed the CLOSE in this packet has
                    // nothing left to be applied to.
                    break;
                }
            }
        }

        // §16.4's generation order: the events a packet caused, then the
        // packet its arrival made us owe.
        self.drain_events();
        self.pump(now);
    }

    /// §10.4's MAX_STREAMS: monotone-max, and `StreamsAvailable` only when
    /// the limit actually moved — a value at or below the current one is a
    /// valid no-op (§8.4) and must wake nobody.
    fn on_max_streams(&mut self, dir: Dir, max: u64) {
        if self.flow.on_max_streams(dir, max) {
            self.events.push(ConnEvent::StreamsAvailable { dir });
        }
    }

    /// §8.2's semantic class: one CLOSE with the violation's §15.3 code.
    fn kill(&mut self, now: Instant, violation: flow::Violation) {
        let code = violation.code();
        tracing::warn!(
            target: "slither::frames",
            error = %violation,
            code,
            "semantic violation in the frame stream; closing"
        );
        // Ruling 99's *"emits **zero** events"* is delivered by the check
        // **order**, not by discarding afterwards: the limit check runs
        // before the opens it would authorise, so the offending frame never
        // generated one. Whatever earlier frames in the same packet
        // legitimately generated is kept and drained first — §8.2 discards a
        // packet's effects only on the *structural* path.
        self.drain_events();
        self.enter_closing(now, code, b"", ConnectionLost::ProtocolViolation { code });
    }

    /// Move the packet's events into the drain, in generation order.
    fn drain_events(&mut self) {
        for event in self.events.drain(..) {
            self.outputs.push_back(ConnOutput::Event(event));
        }
    }

    /// Apply a received packet to a **closing** or **draining** connection.
    ///
    /// §15.2's retention list is exhaustive — the closing state keeps the
    /// seal capability, the receive cipher states and the replay window,
    /// and every other subsystem "may drop immediately". So there is
    /// nothing for a STREAM, credit or ACK frame to be applied *to*, and
    /// the only thing the frame stream is still read for is the peer's
    /// CLOSE.
    ///
    /// A structural failure here is ignored: we are already dying, with a
    /// code, and a second CLOSE carrying a different one would fight the
    /// reply rule it would have to travel under.
    fn apply_post_mortem(&mut self, now: Instant, received: Received) {
        let peer_closed = match &received {
            Received::Frames(frames) => frames.iter().any(|f| matches!(f, Frame::Close(_))),
            Received::Keepalive | Received::Structural(_) => false,
        };

        let closing = match std::mem::replace(&mut self.lifecycle, Lifecycle::Dead) {
            Lifecycle::Closing(closing) => closing,
            // Draining is reply-free and has the deadline it will die on;
            // §15.2 gives no transition out of it.
            other => {
                self.lifecycle = other;
                return;
            }
        };

        if peer_closed {
            // §15.2: "two closing endpoints go quiet rather than
            // ping-ponging replies at 1 Hz for the linger." The existing
            // deadline stands — see this module's `close` docs.
            self.lifecycle = closing.into_draining();
            return;
        }

        // The packet was authenticated and window-fresh, which is the only
        // thing §15.2 owes a reply to.
        let mut closing = closing;
        let reply = closing.reply(now);
        self.lifecycle = Lifecycle::Closing(closing);

        if let Some(close) = reply {
            // A reply that cannot be sealed is dropped rather than turned
            // into a second death: the connection is already dying with a
            // surfaced cause, and `Closed` is emitted exactly once.
            let _ = self.transmit_close(now, &close);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Teardown
    // ═══════════════════════════════════════════════════════════════════

    /// §15.2's closing state: emit CLOSE, surface `lost`, linger.
    fn enter_closing(&mut self, now: Instant, code: u64, reason: &[u8], lost: ConnectionLost) {
        let close = Close::new(code, reason);

        if self.transmit_close(now, &close).is_err() {
            // §7.9: the only reachable seal failure is nonce exhaustion,
            // and it is terminal. Nothing was transmitted and nothing
            // moved (§16.7), so the connection dies with that cause rather
            // than the one asked for, and with no linger — a linger whose
            // replies cannot be sealed is an empty wait.
            self.die(ConnectionLost::NonceExhausted);
            return;
        }

        self.emit_closed(lost);
        self.lifecycle = Lifecycle::Closing(Closing::new(now, close));
        self.enter_post_mortem_timers();
    }

    /// Seal one CLOSE and queue it for the **session's endpoint address**.
    ///
    /// §15.2: the closing state does not roam, so this is the anchor and
    /// never a triggering packet's source. Sealed `seal_quiet` (§7.4) and
    /// not ack-eliciting (§8.3), so it neither marks the liveness clock nor
    /// arms the death deadline.
    fn transmit_close(&mut self, now: Instant, close: &Close) -> Result<(), session::SealFailed> {
        let Some(session) = self.session.as_mut() else {
            return Err(session::SealFailed);
        };

        let mut packing = Packing::new();
        let fits = packing.control(Frame::Close(close.clone()));
        debug_assert!(
            fits,
            "a CLOSE is at most 260 bytes and MAX_PLAINTEXT is 1170"
        );
        let plaintext = packing.into_plaintext();

        let sealed = session.seal_quiet(now, &plaintext, false)?;
        let to = session.established().anchor;

        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        Ok(())
    }

    /// A death with no post-mortem: surface it and retire in one drain
    /// (ruling 81).
    ///
    /// The no-linger paths — liveness (§7.4), nonce exhaustion (§7.9),
    /// `Replaced` (§5.4) — transmit nothing (§15.4), so there is no instant
    /// to seal at and this takes no `now`.
    fn die(&mut self, lost: ConnectionLost) {
        self.emit_closed(lost);
        self.drop_state();
    }

    /// Drop all state and emit `Retired`.
    ///
    /// §16.4: *"`Retired` … fires when the connection's state is actually
    /// dropped — not when its death is announced"*, and the shell delivers
    /// it before releasing the connection's bookkeeping, or the index route
    /// and the guard-entry pin leak for the endpoint's life.
    fn drop_state(&mut self) {
        if let Some(session) = self.session.take() {
            let our_index = session.established().our_index;
            self.outputs
                .push_back(ConnOutput::ToEndpoint(ToEndpoint::Retired { our_index }));
        }
        self.lifecycle = Lifecycle::Dead;
        self.timers.disarm_all();
        self.scratch = Vec::new();
    }

    // ═══════════════════════════════════════════════════════════════════
    // Internals
    // ═══════════════════════════════════════════════════════════════════

    fn install(&mut self, now: Instant, session: EstablishedSession<C>, role: Role) {
        self.installed = true;
        self.role = Some(role);
        self.streams.set_role(role);
        self.session = Some(Session::install(now, session));
        self.sync_liveness_timer();
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Established));
        // §16.9: early opens and writes are *ordinary work* that pumps when
        // a session installs. Nothing was emitted before now because there
        // was nothing to seal with.
        self.pump(now);
    }

    /// §8.5's plan-seal-commit, run until nothing more is owed (§16.7).
    ///
    /// # Ruling 98's seal table, which is §7.4's and not the plan's
    ///
    /// | Frame | Seal | Ack-eliciting |
    /// |---|---|---|
    /// | STREAM, **first transmission** | `seal` (marking) | yes |
    /// | STREAM, **retransmission** | `seal_quiet` | yes |
    /// | **RESET_STREAM** | `seal_quiet` | yes |
    /// | MAX_DATA / MAX_STREAM_DATA / MAX_STREAMS_BIDI / MAX_STREAMS_UNI | `seal_quiet` | yes |
    ///
    /// §7.4 (`SPEC.md:1953–1958`) enumerates the quiet set and RESET_STREAM
    /// is **in** it — the plan's table put it on the marking `seal` "by
    /// omission from §10.3", which is backwards and would have deferred
    /// keepalives forever. `session.rs`'s `seal_quiet` doc comment has
    /// quoted the correct set since slice 3a.
    ///
    /// Marking is a property of the **seal**, not of the frame, so a packet
    /// mixing a fresh STREAM frame with credit frames is marking. That last
    /// sentence is the only part no section states, and it follows from
    /// `seal`/`seal_quiet` being a per-seal choice.
    fn pump(&mut self, now: Instant) {
        if !self.lifecycle.is_live() || self.session.is_none() {
            // §16.9: *"no frame is emitted before install (nothing sends
            // until a session exists)"*.
            return;
        }

        // Bounded by construction: every iteration that transmits has moved
        // stream bytes out of the pending set or cleared a regenerate
        // identity, and one that does neither breaks below.
        loop {
            let mut packing = Packing::new();
            self.streams.pack_control(&mut self.flow, &mut packing);
            let marking = self.streams.fill(&mut packing);

            if packing.frames().is_empty() {
                break;
            }
            let plaintext = packing.into_plaintext();

            let Some(session) = self.session.as_mut() else {
                break;
            };
            // Every frame this slice packs is ack-eliciting (§8.3), so the
            // death clock is armed either way; only `last_send` differs.
            let sealed = if marking {
                session.seal(now, &plaintext, true)
            } else {
                session.seal_quiet(now, &plaintext, true)
            };
            let sealed = match sealed {
                Ok(sealed) => sealed,
                Err(_) => {
                    // §7.9: the only reachable seal failure is nonce
                    // exhaustion, and it is terminal.
                    self.die(ConnectionLost::NonceExhausted);
                    return;
                }
            };
            let to = session.established().anchor;
            self.outputs.push_back(ConnOutput::Transmit(Transmit {
                to,
                data: sealed.datagram,
            }));
            self.sync_liveness_timer();

            if !self.streams.has_output() {
                break;
            }
        }
    }

    /// Re-derive the `Liveness` deadline from §7.4's clocks.
    ///
    /// The deadline is not stored twice: it is a function of
    /// `last_authenticated_recv` and the arming flag, and this is the one
    /// place the timer table is told about it.
    fn sync_liveness_timer(&mut self) {
        if !self.lifecycle.is_live() {
            return;
        }
        let deadline = self.session.as_ref().and_then(|s| s.liveness().deadline());
        self.timers.set(TimerKind::Liveness, deadline);
    }

    /// Entering closing or draining disarms every timer but `CloseLinger`.
    ///
    /// §15.2 drops the state `Loss`, `Pto` and `AckDelay` act on, and says
    /// nothing about the other four. A surviving `Liveness` would fire
    /// mid-linger and produce a **second** `Closed` behind a latch that is
    /// already resolved — which is the invariant `closed()` rests on.
    fn enter_post_mortem_timers(&mut self) {
        let until = self
            .lifecycle
            .linger_until()
            .expect("called only on entering a post-mortem state");
        self.timers.disarm_all_except(TimerKind::CloseLinger);
        self.timers.arm(TimerKind::CloseLinger, until);
    }

    fn emit_closed(&mut self, lost: ConnectionLost) {
        debug_assert!(
            !self.closed_emitted,
            "§16.4: a connection dies once, and `Closed` is emitted once"
        );
        if self.closed_emitted {
            return;
        }
        self.closed_emitted = true;
        self.lost = Some(lost.clone());
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Closed(lost)));
    }
}

/// One item of the connection core's drain. §16.4.
///
/// The variants §16.4 lists that this slice cannot yet construct — every
/// `ConnEvent` but `Established` and `Closed` — are absent rather than
/// stubbed: an uninhabited variant is a claim about the protocol, and these
/// will each arrive with the section that defines them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnOutput {
    /// Send this datagram.
    Transmit(Transmit),
    /// An application-visible event.
    Event(ConnEvent),
    /// A connection→endpoint event, folded into the one drain loop so
    /// there is no second queue to forget.
    ToEndpoint(ToEndpoint),
    /// **Terminal.**
    Timeout(Option<Instant>),
}

/// A connection event. §16.4.
///
/// The stream-naming variants are keyed by [`StreamRef`] and **not** by
/// [`StreamId`] — **ruling 95's amendment**. If they keyed by the wire id,
/// the shell could not match a wakeup to its waker before establishment,
/// which is exactly when §16.9 says work is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnEvent {
    /// The session is installed; the shell resolves `Connecting`.
    Established,
    /// A peer-opened stream is claimable through `accept(dir)`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 99]** **One per newly-opened
    /// stream.** §9.2's implicit open of index 5 opens six streams and emits
    /// six events; `accept(dir)` returns one stream per call, so one event
    /// for six would force the shell to loop `accept()` until `None` on
    /// every wake or lose five — a lost-wakeup that manifests only under the
    /// reordering the tests inject. A frame above §10.4's cumulative limit
    /// emits **zero** and kills the connection, which is what bounds the
    /// burst.
    StreamOpened {
        /// Which direction became claimable.
        dir: Dir,
    },
    /// MAX_STREAMS credit arrived and actually raised the limit (§10.4).
    StreamsAvailable {
        /// The direction whose allowance grew.
        dir: Dir,
    },
    /// A read would now return something other than "no data".
    StreamReadable {
        /// The stream.
        r: StreamRef,
    },
    /// Stream or connection credit arrived for a blocked writer (§10.3).
    StreamWritable {
        /// The stream.
        r: StreamRef,
    },
    /// The send half is fully acknowledged — §9.7's `DataRecvd`.
    ///
    /// **Never fires in slice 4**: reaching `DataRecvd` needs §12's ACK
    /// processing, which is slice 5. That is the slice boundary, not a
    /// defect.
    StreamFinished {
        /// The stream.
        r: StreamRef,
    },
    /// The peer reset the stream (§9.6).
    StreamReset {
        /// The stream.
        r: StreamRef,
        /// The peer's application code.
        error_code: u64,
    },
    /// The connection ended, with §18.1's cause. Emitted **once**.
    ///
    /// Not `Copy`, and neither is [`ConnOutput`] any more:
    /// `ConnectionLost::PeerClosed` carries the peer's reason phrase, which
    /// is `Vec<u8>` because §8.4 carries it as bytes.
    Closed(ConnectionLost),
}

/// A two-core smoke check for the §9/§10 machinery.
///
/// **Not the slice's acceptance tests** — those are written independently by
/// the test author (working rule 6) and live in a file this implementer
/// never touches. This exists because the transmit pump, the codec and the
/// receive path have no other in-file exercise: everything else in the four
/// new modules is unit-testable against its own state, and the *seam between
/// them* is not.
///
/// It builds two real `Connection`s over one real hiss handshake and hands
/// each one the other's datagrams, so every assertion here is a wire
/// assertion.
#[cfg(test)]
mod smoke {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::num::NonZeroU64;
    use std::time::Instant;

    use super::*;
    use crate::constants::{PROLOGUE, REKEY_EPOCH_MSGS};
    use crate::identity::Identity;
    use crate::packet::ReferenceSuite;
    use crate::testutil::CountingIdentity;

    type Suite = ReferenceSuite;
    type Id = CountingIdentity<Suite>;

    fn v4(a: u8) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), 4000 + a as u16)
    }

    /// Both halves of one real IK handshake, as two `EstablishedSession`s.
    fn sessions() -> (EstablishedSession<Suite>, EstablishedSession<Suite>) {
        let epoch = NonZeroU64::new(REKEY_EPOCH_MSGS).expect("nonzero");
        let a: Id = CountingIdentity::seeded([7u8; 32]);
        let b: Id = CountingIdentity::seeded([9u8; 32]);
        let b_pub = *b.public_static();
        let (ap, ask) = a.open().expect("identity opens");
        let (bp, bsk) = b.open().expect("identity opens");

        let init = <Suite as Handshake>::initiator(ap, PROLOGUE, b_pub);
        let (msg1, sent) =
            <Suite as Handshake>::write_msg1(init, ask, &[0u8; crate::constants::MSG1_PAYLOAD_LEN])
                .expect("msg1");
        let resp = <Suite as Handshake>::responder(bp, PROLOGUE, bsk).expect("responder");
        let (_claimed, mid) = <Suite as Handshake>::read_msg1_intro(resp, &msg1).expect("intro");
        let (_payload, read) = <Suite as Handshake>::complete(mid).expect("complete");
        let (msg2, b_transport) = <Suite as Handshake>::write_msg2(read).expect("msg2");
        let a_transport = <Suite as Handshake>::read_msg2(sent, &msg2).expect("read msg2");

        let (a_seal, a_open) = <Suite as Handshake>::into_datagram(a_transport, epoch);
        let (b_seal, b_open) = <Suite as Handshake>::into_datagram(b_transport, epoch);

        (
            EstablishedSession {
                seal: a_seal,
                open: a_open,
                our_index: 0x1111_1111,
                peer_index: 0x2222_2222,
                anchor: v4(2),
            },
            EstablishedSession {
                seal: b_seal,
                open: b_open,
                our_index: 0x2222_2222,
                peer_index: 0x1111_1111,
                anchor: v4(1),
            },
        )
    }

    /// Two established `Connection`s over one real IK handshake: `a` is
    /// §6.7's initiator, `b` the acceptor, so §9.1's parity is directly
    /// assertable.
    fn pair(now: Instant) -> (Connection<Suite>, Connection<Suite>) {
        let (a_session, b_session) = sessions();
        (
            Connection::established(now, [1u8; 32], a_session, Role::Initiator),
            Connection::established(now, [2u8; 32], b_session, Role::Responder),
        )
    }

    /// Drain one core to `Timeout`, returning what it produced.
    fn drain(conn: &mut Connection<Suite>) -> (Vec<Vec<u8>>, Vec<ConnEvent>) {
        let mut datagrams = Vec::new();
        let mut events = Vec::new();
        loop {
            match conn.poll_output() {
                ConnOutput::Transmit(t) => datagrams.push(t.data),
                ConnOutput::Event(e) => events.push(e),
                ConnOutput::ToEndpoint(_) => {}
                ConnOutput::Timeout(_) => return (datagrams, events),
            }
        }
    }

    /// Hand every datagram `from` produced to `to`, and drain `to`.
    fn deliver(
        from: &mut Connection<Suite>,
        to: &mut Connection<Suite>,
        now: Instant,
        src: SocketAddr,
    ) -> Vec<ConnEvent> {
        let (datagrams, _) = drain(from);
        let mut events = Vec::new();
        for dgram in datagrams {
            to.handle_datagram(now, src, &dgram);
            events.extend(drain(to).1);
        }
        events
    }

    /// §9.1's parity, end to end: the initiator's first bidi stream is id 0
    /// and the acceptor's is id 1 — and both cores agree.
    #[test]
    fn stream_id_parity_comes_from_the_installed_role() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let ra = a.open(Dir::Bi).expect("first bidi");
        let rb = b.open(Dir::Bi).expect("first bidi");
        assert_eq!(a.stream_id(ra).map(StreamId::as_u64), Some(0));
        assert_eq!(b.stream_id(rb).map(StreamId::as_u64), Some(1));
        assert_eq!(a.role(), Some(Role::Initiator));
        assert_eq!(b.role(), Some(Role::Responder));

        let ua = a.open(Dir::Uni).expect("first uni");
        let ub = b.open(Dir::Uni).expect("first uni");
        assert_eq!(a.stream_id(ua).map(StreamId::as_u64), Some(2));
        assert_eq!(b.stream_id(ub).map(StreamId::as_u64), Some(3));
    }

    /// A write on one core arrives, in order, as a read on the other — the
    /// whole of §9.5's happy path through the real codec and the real AEAD.
    #[test]
    fn bytes_written_on_one_core_are_read_on_the_other() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        assert_eq!(a.write(now, r, b"hello world").expect("write"), 11);
        a.finish(now, r).expect("finish");
        a.flush(now);

        let events = deliver(&mut a, &mut b, now, v4(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            "the peer's first frame opens the stream: {events:?}"
        );

        let claimed = b.accept(Dir::Uni).expect("one claimable uni stream");
        assert_eq!(
            b.stream_id(claimed).map(StreamId::as_u64),
            a.stream_id(r).map(StreamId::as_u64),
            "both ends name the stream identically"
        );

        let mut buf = [0u8; 64];
        assert_eq!(b.read(now, claimed, &mut buf), Ok(Some(11)));
        assert_eq!(&buf[..11], b"hello world");
        assert_eq!(
            b.read(now, claimed, &mut buf),
            Ok(None),
            "FIN is end of stream"
        );
    }

    /// §9.6's reset crosses the wire and surfaces as `ReadError::Reset`.
    #[test]
    fn a_reset_crosses_the_wire_and_surfaces_to_the_reader() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, b"partial").expect("write");
        let _ = deliver(&mut a, &mut b, now, v4(1));
        let claimed = b.accept(Dir::Uni).expect("claimable");

        a.reset(now, r, 42);
        let events = deliver(&mut a, &mut b, now, v4(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnEvent::StreamReset { error_code: 42, .. })),
            "{events:?}"
        );
        let mut buf = [0u8; 8];
        assert_eq!(b.read(now, claimed, &mut buf), Err(ReadError::Reset(42)));
    }

    /// A payload larger than one packet's plaintext is carried by more than
    /// one packet and reassembles whole.
    #[test]
    fn a_multi_packet_write_reassembles() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let payload: Vec<u8> = (0..8_000u32).map(|i| (i % 251) as u8).collect();
        let r = a.open(Dir::Uni).expect("uni");
        assert_eq!(a.write(now, r, &payload), Ok(payload.len()));
        a.finish(now, r).expect("finish");
        a.flush(now);

        let (datagrams, _) = drain(&mut a);
        assert!(
            datagrams.len() > 1,
            "8 000 bytes does not fit one MAX_PLAINTEXT packet"
        );
        for dgram in datagrams {
            b.handle_datagram(now, v4(1), &dgram);
            let _ = drain(&mut b);
        }

        let claimed = b.accept(Dir::Uni).expect("claimable");
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(Some(n)) = b.read(now, claimed, &mut buf) {
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, payload);
    }

    /// §8.5's round-robin: two streams with data pending both make progress
    /// inside one fill pass, rather than one starving the other.
    ///
    /// The contention has to be *built*: §16.7 makes `write` seal
    /// synchronously, so two sequential writes on a live connection never
    /// contend — the first has already gone out. Writing **before the
    /// install** (§16.9) queues both and makes the install's pump the first
    /// fill pass that sees two ready streams.
    #[test]
    fn the_fill_serves_streams_round_robin() {
        let now = Instant::now();
        let (a_session, b_session) = sessions();
        let mut a: Connection<Suite> = Connection::connecting([5u8; 32]);

        let r1 = a.open(Dir::Uni).expect("uni");
        let r2 = a.open(Dir::Uni).expect("uni");
        a.write(now, r1, &vec![1u8; 4_000]).expect("write");
        a.write(now, r2, &vec![2u8; 4_000]).expect("write");
        assert!(drain(&mut a).0.is_empty(), "§16.9: nothing before install");

        a.handle_endpoint_event(
            now,
            Install {
                session: a_session,
                role: Role::Initiator,
            },
        );

        let mut b = Connection::established(now, [6u8; 32], b_session, Role::Responder);
        let _ = drain(&mut b);

        let (datagrams, _) = drain(&mut a);
        b.handle_datagram(now, v4(1), &datagrams[0]);
        let _ = drain(&mut b);

        let s1 = b.accept(Dir::Uni).expect("first stream");
        let s2 = b
            .accept(Dir::Uni)
            .expect("the second stream shares the packet");
        let mut buf = [0u8; 4096];
        let n1 = b.read(now, s1, &mut buf).expect("read").expect("data");
        let n2 = b.read(now, s2, &mut buf).expect("read").expect("data");
        assert!(
            n1 > 0 && n2 > 0,
            "one packet carried both streams: {n1} and {n2}"
        );
        assert!(
            n1 < 4_000,
            "§8.5 serves a quantum, not a whole stream: {n1}"
        );
    }

    /// §16.9: a stream opened before establishment keeps its handle across
    /// the install, writes queued early leave once a session exists, and the
    /// wire id appears only afterwards.
    ///
    /// **Ruling 95's trap**, from the other side: a core that returned an
    /// internal index *typed as* `StreamId` and remapped at install would
    /// pass "open early" and "write late" separately and fail exactly this.
    #[test]
    fn an_early_opened_stream_keeps_its_handle_across_install() {
        let now = Instant::now();
        let (a_session, b_session) = sessions();

        let mut a: Connection<Suite> = Connection::connecting([3u8; 32]);
        let r = a.open(Dir::Uni).expect("open before establishment");
        assert_eq!(a.stream_id(r), None, "§16.9: no id until established");
        assert_eq!(a.write(now, r, b"queued").expect("write"), 6);
        a.finish(now, r).expect("finish");
        let (datagrams, _) = drain(&mut a);
        assert!(datagrams.is_empty(), "§16.9: nothing sends before install");

        a.handle_endpoint_event(
            now,
            Install {
                session: a_session,
                role: Role::Initiator,
            },
        );
        assert_eq!(
            a.stream_id(r).map(StreamId::as_u64),
            Some(2),
            "the same handle now names initiator-uni index 0"
        );

        let mut b = Connection::established(now, [4u8; 32], b_session, Role::Responder);
        let _ = drain(&mut b);
        let _ = deliver(&mut a, &mut b, now, v4(1));

        let claimed = b.accept(Dir::Uni).expect("the early write arrived");
        let mut buf = [0u8; 16];
        assert_eq!(b.read(now, claimed, &mut buf), Ok(Some(6)));
        assert_eq!(&buf[..6], b"queued");
    }
}

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
pub(crate) mod frame;
pub(crate) mod session;
pub(crate) mod timers;

// Slice 3a's acceptance tests, written independently from `SPEC.md` and
// `STORIES.md` by an author who never read this directory (working rule 6).
// Declared here at integration rather than by the implementer, so that
// neither agent could reach the other's file.
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::Instant;

use crate::constants;
use crate::error::ConnectionLost;
use crate::packet::{Handshake, Inbound, classify};

use self::close::{Closing, Lifecycle};
use self::frame::{Close, Frame, Packing, Structural};
use self::session::Session;
use self::timers::{TimerKind, Timers};

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
        self.session = Some(Session::install(now, session));
        self.sync_liveness_timer();
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Established));
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnEvent {
    /// The session is installed; the shell resolves `Connecting`.
    Established,
    /// The connection ended, with §18.1's cause. Emitted **once**.
    ///
    /// Not `Copy`, and neither is [`ConnOutput`] any more:
    /// `ConnectionLost::PeerClosed` carries the peer's reason phrase, which
    /// is `Vec<u8>` because §8.4 carries it as bytes.
    Closed(ConnectionLost),
}

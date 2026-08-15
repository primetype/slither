//! §10 — flow control: two credit levels, both receiver-driven, both
//! expressed as **absolute byte offsets**.
//!
//! # The three §10 violations
//!
//! **[RATIFIED 2026/08/15 — ruling 104]** §10.5 is titled "Violations",
//! sits inside the chapter that defines all three, and enumerates **two**.
//! The third is §10.6's: reassembly ranges exceeding `REASSEMBLY_CHUNKS_MAX`
//! after coalescing is a `PROTOCOL_VIOLATION`. §10.5's closing *"There is no
//! tolerance band; the limits are exact"* is true of the two it lists and
//! **false of the third** — `REASSEMBLY_CHUNKS_MAX` is explicitly a
//! tolerance. [`Violation`] carries all three.
//!
//! # The connection-level consumed count is a per-stream absolute sum
//!
//! §10.3's true-up is *"absolute, not additive"*: it advances **that
//! stream's contribution** to a value, and never adds on top of bytes
//! already counted by reads. A scalar `consumed += n` cannot be made
//! idempotent under read-then-retire, so every receive half remembers how
//! much of its own contribution it has already folded into the connection
//! scalar ([`crate::core::connection::recv::RecvHalf`]'s `counted`) and
//! folds only the delta. Same arithmetic, with the absolute rule enforced
//! structurally.
//!
//! # H15: `last_advertised` is seeded to the constant, never to zero
//!
//! §10.2's initial windows are **protocol constants that were never sent on
//! the wire**. Seeding [`CreditWindow::last_advertised`] to zero makes
//! `prospective − last_advertised` equal to a whole window the instant a
//! stream opens, so every stream emits a spurious MAX_STREAM_DATA on open. A
//! one-line bug with a wire-visible effect.

use crate::constants;

use super::stream_id::{Dir, MAX_STREAMS_CEILING};

/// §10.5's violations, plus §10.6's — the set ruling 104 makes
/// three-membered.
///
/// Also carries §8.4's two per-frame semantic violations that are not §10's
/// but share the same disposition: one CLOSE with a §15.3 code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Violation {
    /// A peer exceeding advertised credit, stream or connection level.
    /// §10.5.
    #[error("flow control: the peer exceeded advertised credit")]
    FlowControl,
    /// A peer opening beyond a cumulative stream limit. §10.4, §10.5.
    #[error("stream limit: the peer opened beyond the cumulative limit")]
    StreamLimit,
    /// A frame naming a stream its sender could not send on, or credit for a
    /// stream in our own space that we have not opened. §8.4.
    #[error("stream state: the peer named a stream it could not send on")]
    StreamState,
    /// Data beyond a pinned final size, a FIN below already-received data,
    /// or two pins that disagree. §8.4, §9.5.
    #[error("final size: the frame contradicts a pinned final size")]
    FinalSize,
    /// §10.6's reassembly-fragment ceiling — ruling 104's third member.
    #[error("protocol violation: reassembly ranges exceed REASSEMBLY_CHUNKS_MAX")]
    Reassembly,
}

impl Violation {
    /// The §15.3 registry code this violation CLOSEs with.
    pub(crate) fn code(self) -> u64 {
        match self {
            Violation::FlowControl => constants::FLOW_CONTROL_ERROR,
            Violation::StreamLimit => constants::STREAM_LIMIT_ERROR,
            Violation::StreamState => constants::STREAM_STATE_ERROR,
            Violation::FinalSize => constants::FINAL_SIZE_ERROR,
            Violation::Reassembly => constants::PROTOCOL_VIOLATION,
        }
    }
}

/// §10.3's re-grant machinery at one level — a stream or the connection.
///
/// Both levels run the identical formula over a different `window`, which is
/// why it is written once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CreditWindow {
    /// `INITIAL_MAX_STREAM_DATA` for a stream, `INITIAL_MAX_DATA` for the
    /// connection.
    window: u64,
    /// §10.3's *"Consumption, not arrival"* — the level's consumed count.
    consumed: u64,
    /// The highest limit ever advertised at this level. **H15**: seeded to
    /// `window`, not zero.
    last_advertised: u64,
}

impl CreditWindow {
    /// A window with §10.2's un-negotiated initial value already advertised.
    pub(crate) fn new(window: u64) -> Self {
        Self {
            window,
            consumed: 0,
            last_advertised: window,
        }
    }

    /// The highest limit ever advertised at this level.
    ///
    /// **[ruling 93]** This is the retirement true-up value for a receive
    /// half: the least upper bound on what the peer could have sent without
    /// committing a `FLOW_CONTROL_ERROR`, and the buffer commitment §10.6
    /// says we made.
    pub(crate) fn advertised(&self) -> u64 {
        self.last_advertised
    }

    /// This level's consumed count.
    pub(crate) fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Fold `n` more consumed bytes in. §10.3.
    pub(crate) fn consume(&mut self, n: u64) {
        self.consumed = self.consumed.saturating_add(n);
    }

    /// Raise the consumed count **to** `value` — §10.3's monotone
    /// bring-to-final, for a caller that holds an absolute quantity rather
    /// than a delta. Returns the delta actually folded.
    pub(crate) fn consume_to(&mut self, value: u64) -> u64 {
        let delta = value.saturating_sub(self.consumed);
        self.consumed = self.consumed.max(value);
        delta
    }

    /// §10.3's trigger: emit a credit frame when
    /// `prospective_limit − last_advertised ≥ WINDOW/2`.
    ///
    /// Returns the absolute limit to advertise, and records it — so a
    /// caller that drops the value has still advanced `last_advertised`,
    /// which is why the frame it builds is a *regenerate* identity carrying
    /// the freshest value (§8.7) rather than a queued copy.
    pub(crate) fn take_grant(&mut self) -> Option<u64> {
        let prospective = self.consumed.saturating_add(self.window);
        let threshold = self.window / constants::CREDIT_REGRANT_DIVISOR;
        if prospective.saturating_sub(self.last_advertised) >= threshold {
            self.last_advertised = prospective;
            Some(prospective)
        } else {
            None
        }
    }
}

/// The connection-level ledgers and §10.4's cumulative stream limits.
///
/// §10.7's exemption is structural here rather than conditional: nothing on
/// the datagram path can reach this type, because every entry point is keyed
/// by a stream.
pub(crate) struct Flow {
    /// §10.1's connection-level receive sum: Σ over all streams of the
    /// highest received offset (the final size once pinned). Bounded by what
    /// we advertised.
    recv_charged: u64,
    /// §10.3's connection-level re-grant.
    recv: CreditWindow,
    /// Σ over all streams of the highest offset we have queued to send.
    send_charged: u64,
    /// The peer's connection limit — §10.2's constant until MAX_DATA raises
    /// it.
    send_max_data: u64,
    /// §10.4's cumulative limit we advertise for the peer's opens, per
    /// direction.
    local_max_streams: [u64; 2],
    /// Grants earned by full closure and not yet advertised, per direction.
    ungranted: [u64; 2],
    /// §10.4's cumulative limit the peer advertises for our opens.
    remote_max_streams: [u64; 2],
}

impl Flow {
    /// §10.2's initial values, in both directions and all four spaces.
    pub(crate) fn new() -> Self {
        let initial = [
            constants::INITIAL_MAX_STREAMS_BIDI,
            constants::INITIAL_MAX_STREAMS_UNI,
        ];
        Self {
            recv_charged: 0,
            recv: CreditWindow::new(constants::INITIAL_MAX_DATA),
            send_charged: 0,
            send_max_data: constants::INITIAL_MAX_DATA,
            local_max_streams: initial,
            ungranted: [0, 0],
            remote_max_streams: initial,
        }
    }

    // ── connection-level receive ────────────────────────────────────────

    /// §10.5's connection-level bound, checked **before** any true-up and
    /// with checked arithmetic (§8.4: *"an unchecked sum wraps for large
    /// `final_size` values and silently re-opens the window"*).
    pub(crate) fn check_recv_charge(&self, delta: u64) -> Result<(), Violation> {
        match self.recv_charged.checked_add(delta) {
            Some(total) if total <= self.recv.advertised() => Ok(()),
            _ => Err(Violation::FlowControl),
        }
    }

    /// Charge `delta` more received bytes at the connection level.
    pub(crate) fn charge_recv(&mut self, delta: u64) {
        self.recv_charged = self.recv_charged.saturating_add(delta);
    }

    /// Σ over all streams of the highest received offset. §10.1.
    pub(crate) fn recv_charged(&self) -> u64 {
        self.recv_charged
    }

    /// §10.3's connection-level window — consumption and the re-grant.
    pub(crate) fn recv_window(&mut self) -> &mut CreditWindow {
        &mut self.recv
    }

    /// The connection limit we have advertised.
    pub(crate) fn recv_advertised(&self) -> u64 {
        self.recv.advertised()
    }

    // ── connection-level send ───────────────────────────────────────────

    /// How many more bytes we may queue at the connection level right now.
    pub(crate) fn send_room(&self) -> u64 {
        self.send_max_data.saturating_sub(self.send_charged)
    }

    /// Charge `delta` bytes accepted from the application.
    pub(crate) fn charge_send(&mut self, delta: u64) {
        self.send_charged = self.send_charged.saturating_add(delta);
    }

    /// Apply a received MAX_DATA as §10.1's monotone-max. `true` if it
    /// actually raised the limit — a value not above the current one is a
    /// valid no-op (§8.4) and must wake nobody.
    pub(crate) fn on_max_data(&mut self, max: u64) -> bool {
        if max > self.send_max_data {
            self.send_max_data = max;
            true
        } else {
            false
        }
    }

    /// The peer's connection limit.
    pub(crate) fn send_max_data(&self) -> u64 {
        self.send_max_data
    }

    // ── §10.4 cumulative stream limits ──────────────────────────────────

    /// The cumulative limit the peer advertises for our opens.
    pub(crate) fn remote_max_streams(&self, dir: Dir) -> u64 {
        self.remote_max_streams[dir.slot()]
    }

    /// The cumulative limit we advertise for the peer's opens.
    pub(crate) fn local_max_streams(&self, dir: Dir) -> u64 {
        self.local_max_streams[dir.slot()]
    }

    /// Apply a received MAX_STREAMS as monotone-max. `true` if it raised the
    /// limit, which is what earns a `StreamsAvailable` (§10.4).
    pub(crate) fn on_max_streams(&mut self, dir: Dir, max: u64) -> bool {
        let slot = dir.slot();
        if max > self.remote_max_streams[slot] {
            self.remote_max_streams[slot] = max;
            true
        } else {
            false
        }
    }

    /// §8.4's structural bound on MAX_STREAMS: `max` > 2⁶⁰ is
    /// unrepresentable as an index. The boundary is `>`, not `≥`.
    pub(crate) fn max_streams_is_representable(max: u64) -> bool {
        max <= MAX_STREAMS_CEILING
    }

    /// §10.4: *"The receiver grants +1 as it **fully closes a peer-opened
    /// stream** of the space"* — closing streams *we* opened must not
    /// inflate the peer's allowance.
    pub(crate) fn grant_stream_credit(&mut self, dir: Dir) {
        let slot = dir.slot();
        self.ungranted[slot] = self.ungranted[slot].saturating_add(1);
    }

    /// §10.4's **two** emission triggers, both against
    /// `STREAMS_CREDIT_BATCH`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 102]** §10.4 names the constant in
    /// the first trigger and writes the literal `8` in the second; §10.2
    /// declares exactly one constant of value 8. There is no second literal
    /// here — an unnamed magic number would silently decouple from the named
    /// one the first time anyone tuned it.
    ///
    /// `peer_opened` is the count of streams the peer has ever opened in
    /// this direction, so `advertised − peer_opened` is *"the peer's
    /// remaining allowance"*. Returns the new cumulative limit to advertise.
    pub(crate) fn take_streams_grant(&mut self, dir: Dir, peer_opened: u64) -> Option<u64> {
        let slot = dir.slot();
        let ungranted = self.ungranted[slot];
        if ungranted == 0 {
            return None;
        }
        let remaining = self.local_max_streams[slot].saturating_sub(peer_opened);
        let batch = constants::STREAMS_CREDIT_BATCH;
        if ungranted >= batch || remaining <= batch {
            self.ungranted[slot] = 0;
            let limit = self.local_max_streams[slot]
                .saturating_add(ungranted)
                .min(MAX_STREAMS_CEILING);
            self.local_max_streams[slot] = limit;
            Some(limit)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **H15.** `last_advertised` starts at the window, so a freshly opened
    /// stream owes nothing. Seeded to zero, this asserts a grant instead.
    #[test]
    fn a_fresh_window_owes_no_grant() {
        let mut w = CreditWindow::new(constants::INITIAL_MAX_STREAM_DATA);
        assert_eq!(w.advertised(), constants::INITIAL_MAX_STREAM_DATA);
        assert_eq!(w.take_grant(), None);
    }

    /// §10.3's formula, two-sided: one byte below half the window owes
    /// nothing, half the window owes exactly `bytes_read + WINDOW`.
    #[test]
    fn the_regrant_threshold_is_two_sided_at_half_the_window() {
        let window = constants::INITIAL_MAX_STREAM_DATA;
        let half = window / constants::CREDIT_REGRANT_DIVISOR;

        let mut below = CreditWindow::new(window);
        below.consume(half - 1);
        assert_eq!(below.take_grant(), None);

        let mut at = CreditWindow::new(window);
        at.consume(half);
        assert_eq!(at.take_grant(), Some(half + window));
        // And the grant is recorded: an immediate second call owes nothing.
        assert_eq!(at.take_grant(), None);
    }

    /// §10.3's *"absolute, not additive"*: bringing a contribution to a
    /// value it already exceeds folds nothing.
    #[test]
    fn consume_to_is_monotone_and_idempotent() {
        let mut w = CreditWindow::new(constants::INITIAL_MAX_STREAM_DATA);
        w.consume(100);
        assert_eq!(w.consume_to(250), 150);
        assert_eq!(w.consumed(), 250);
        assert_eq!(w.consume_to(250), 0);
        assert_eq!(w.consume_to(10), 0);
        assert_eq!(w.consumed(), 250);
    }

    /// §8.4's `max > 2⁶⁰` boundary, two-sided.
    #[test]
    fn the_max_streams_ceiling_boundary_is_two_sided() {
        assert!(Flow::max_streams_is_representable(MAX_STREAMS_CEILING - 1));
        assert!(Flow::max_streams_is_representable(MAX_STREAMS_CEILING));
        assert!(!Flow::max_streams_is_representable(MAX_STREAMS_CEILING + 1));
    }

    /// **[ruling 102]** Both triggers, and both against the same constant.
    #[test]
    fn both_max_streams_triggers_use_the_batch_constant() {
        let batch = constants::STREAMS_CREDIT_BATCH;

        // Trigger one: the batch fills while the peer has plenty of room.
        let mut flow = Flow::new();
        for _ in 0..batch - 1 {
            flow.grant_stream_credit(Dir::Uni);
            assert_eq!(flow.take_streams_grant(Dir::Uni, 0), None);
        }
        flow.grant_stream_credit(Dir::Uni);
        assert_eq!(
            flow.take_streams_grant(Dir::Uni, 0),
            Some(constants::INITIAL_MAX_STREAMS_UNI + batch)
        );

        // Trigger two: one grant, but the peer's remaining allowance has
        // dropped to the batch.
        let mut flow = Flow::new();
        flow.grant_stream_credit(Dir::Uni);
        let plenty = constants::INITIAL_MAX_STREAMS_UNI - batch - 1;
        assert_eq!(flow.take_streams_grant(Dir::Uni, plenty), None);
        let tight = constants::INITIAL_MAX_STREAMS_UNI - batch;
        assert_eq!(
            flow.take_streams_grant(Dir::Uni, tight),
            Some(constants::INITIAL_MAX_STREAMS_UNI + 1)
        );
    }

    /// Monotone-max: a value at or below the current limit is a valid no-op
    /// and wakes nobody (§8.4).
    #[test]
    fn credit_frames_apply_as_monotone_max() {
        let mut flow = Flow::new();
        assert!(!flow.on_max_data(constants::INITIAL_MAX_DATA));
        assert!(!flow.on_max_data(constants::INITIAL_MAX_DATA - 1));
        assert!(flow.on_max_data(constants::INITIAL_MAX_DATA + 1));
        assert_eq!(flow.send_max_data(), constants::INITIAL_MAX_DATA + 1);

        assert!(!flow.on_max_streams(Dir::Bi, constants::INITIAL_MAX_STREAMS_BIDI));
        assert!(flow.on_max_streams(Dir::Bi, constants::INITIAL_MAX_STREAMS_BIDI + 4));
        assert_eq!(
            flow.remote_max_streams(Dir::Bi),
            constants::INITIAL_MAX_STREAMS_BIDI + 4
        );
    }

    /// Checked arithmetic: a `final_size` near `u64::MAX` must not wrap the
    /// connection sum into "fits" (§8.4).
    #[test]
    fn the_connection_bound_uses_checked_arithmetic() {
        let mut flow = Flow::new();
        flow.charge_recv(1_000);
        assert_eq!(flow.check_recv_charge(u64::MAX), Err(Violation::FlowControl));
        assert_eq!(
            flow.check_recv_charge(constants::INITIAL_MAX_DATA),
            Err(Violation::FlowControl)
        );
        assert_eq!(
            flow.check_recv_charge(constants::INITIAL_MAX_DATA - 1_000),
            Ok(())
        );
    }

    /// §15.3's codes, one per violation — ruling 104's three-membered set.
    #[test]
    fn every_violation_carries_its_registry_code() {
        assert_eq!(Violation::FlowControl.code(), constants::FLOW_CONTROL_ERROR);
        assert_eq!(Violation::StreamLimit.code(), constants::STREAM_LIMIT_ERROR);
        assert_eq!(Violation::StreamState.code(), constants::STREAM_STATE_ERROR);
        assert_eq!(Violation::FinalSize.code(), constants::FINAL_SIZE_ERROR);
        assert_eq!(Violation::Reassembly.code(), constants::PROTOCOL_VIOLATION);
    }
}

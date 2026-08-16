//! §7.3's anti-amplification budget and §7.5's contested mark.
//!
//! Two small state machines, kept here rather than as loose fields on
//! [`Connection`](super::Connection) for one reason each.
//!
//! [`Amplification`] is **one guarded predicate**. Ruling 168 reverses a
//! recorded declination — the budget now *disarms* on a return-routability
//! proof — and the maintainer has flagged it as the ruling most wanted
//! attacked in the post-slice protocol review. A budget threaded as four
//! bare fields through six call sites cannot be removed without touching
//! all six; a type with `admits`/`on_sent`/`on_recv` can.
//!
//! [`Contested`] is **three states, not two** (ruling 46): *"the three real
//! states (marked-pending, probing, cleared) do not map onto one bool at
//! all."* An `enum` makes the pending gap — the interval in which a mark
//! exists, no deadline is armed and **nothing has been emitted** —
//! unrepresentable as anything else.

use std::time::Instant;

use crate::constants;

/// §7.3's per-**session** anti-amplification budget (ruling 170).
///
/// # What it is, exactly
///
/// While an address is *unvalidated*, this endpoint may send it at most
/// `AMPLIFICATION_FACTOR` × the **authenticated and window-fresh** bytes it
/// has received from that address (ruling 169 — the literal §7.3 text
/// excluded only "unauthenticated or undecryptable", which let a *replayed*
/// packet replenish a security counter). Units are **datagram bytes** in
/// both directions.
///
/// # Armed at exactly two events, and no others
///
/// 1. A committed roam (§7.3).
/// 2. An accepted initiation's msg1 anchor (§5.6) — the msg1 itself
///    qualifies as authenticated, *"its handshake tail tags having verified
///    at admission"*, and credits the received counter.
///
/// A `connect()`-supplied address is **not** armed: a dialled connection
/// starts validated. That is the single most load-bearing fact for the
/// contested-probe tests, because on a validated address §7.5's mark and
/// its probe transmission fall on the same instant.
///
/// # Disarmed by a return-routability proof (ruling 168)
///
/// The address becomes validated when an authenticated, window-fresh packet
/// *from that address* carries an ACK covering any counter at or above
/// [`floor`](Self::floor) — the counter the next seal would have used at the
/// moment the budget armed. Only a peer that received something we sent to
/// that address *after* the change can produce one. Wire-free, one `u64`
/// and one `bool`.
///
/// **The 3× ratio itself is never lifted** — never raised, never
/// configurable. What ends is the unvalidated *state*.
///
/// Without this an accepting endpoint could never serve a connection: a peer
/// downloading a file replies with ACKs only, funding ~120 bytes of budget
/// against 2 400 bytes of demand.
///
/// # The residual, stated rather than inferred (ruling 170)
///
/// The budget is per session, so N sessions to one address multiply the
/// reflector by N.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Amplification {
    /// `true` ⇒ the other three are meaningless and no check runs.
    validated: bool,
    /// Ruling 168's floor: the counter the next seal will use, recorded at
    /// the arming.
    floor: u64,
    /// Datagram bytes sent to the current address since the arming.
    sent: u64,
    /// Authenticated **and window-fresh** datagram bytes received from it.
    recv: u64,
}

impl Default for Amplification {
    fn default() -> Self {
        Self::validated()
    }
}

impl Amplification {
    /// A validated address: no budget, no check.
    pub(crate) const fn validated() -> Self {
        Self {
            validated: true,
            floor: 0,
            sent: 0,
            recv: 0,
        }
    }

    /// Arm the budget on a new, unvalidated address.
    ///
    /// `floor` is the counter the next seal will use; `credit` is the
    /// triggering packet's datagram length, which is authenticated and
    /// window-fresh by construction on both arming paths and so may fund the
    /// budget under ruling 169.
    pub(crate) const fn arm(floor: u64, credit: u64) -> Self {
        Self {
            validated: false,
            floor,
            sent: 0,
            recv: credit,
        }
    }

    /// Whether a datagram of `len` bytes may leave for this address now.
    ///
    /// §7.3: `sent + len > AMPLIFICATION_FACTOR × recv` ⇒ the datagram is
    /// **held** — not dropped, not truncated, not an error.
    ///
    /// **Binds all output**, explicitly including §14.5's and §13.4's
    /// congestion-window exemptions: *"those exemptions are scoped to cwnd,
    /// never to this budget."*
    pub(crate) fn admits(&self, len: u64) -> bool {
        if self.validated {
            return true;
        }
        self.sent.saturating_add(len) <= constants::AMPLIFICATION_FACTOR.saturating_mul(self.recv)
    }

    /// Charge a datagram that left for this address.
    pub(crate) fn on_sent(&mut self, len: u64) {
        if self.validated {
            return;
        }
        self.sent = self.sent.saturating_add(len);
    }

    /// Credit an **authenticated and window-fresh** datagram from it
    /// (ruling 169). Called at exactly the point §7.2's window marks the
    /// packet, which is the same instant liveness is refreshed.
    pub(crate) fn on_recv(&mut self, len: u64) {
        if self.validated {
            return;
        }
        self.recv = self.recv.saturating_add(len);
    }

    /// Ruling 168's proof of return routability: an ACK from this address
    /// covering any counter at or above the floor.
    ///
    /// `largest` is the ACK's largest acknowledged counter, which is also
    /// the greatest counter the frame covers — so "covers any counter ≥
    /// floor" is exactly `largest >= floor`.
    pub(crate) fn on_ack_covering(&mut self, largest: u64) {
        if !self.validated && largest >= self.floor {
            *self = Self::validated();
        }
    }

    /// Whether the address is validated — i.e. no budget is armed.
    pub(crate) fn is_validated(&self) -> bool {
        self.validated
    }

    /// Record the floor for a budget armed before its session existed.
    ///
    /// The msg1-anchor arming happens in
    /// [`Connection::established`](super::Connection::established), which
    /// builds the connection *before* installing the session; the counter
    /// the next seal will use is not readable until the install. A no-op on
    /// a validated budget.
    pub(crate) fn set_floor(&mut self, floor: u64) {
        if !self.validated {
            self.floor = floor;
        }
    }

    /// `(sent, received)` in datagram bytes, for tests. `None` when
    /// validated.
    #[cfg(test)]
    pub(crate) fn counters(&self) -> Option<(u64, u64)> {
        (!self.validated).then_some((self.sent, self.recv))
    }

    /// The recorded validation floor, for tests.
    #[cfg(test)]
    pub(crate) fn floor(&self) -> u64 {
        self.floor
    }
}

/// §7.5's contested mark. **Three states, not two** (ruling 46).
///
/// §16.4's ruling-46 rationale names them: *"the three real states
/// (marked-pending, probing, cleared) do not map onto one bool at all."*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Contested {
    /// Not contested.
    No,
    /// Marked. The probe floor is recorded; §7.3's budget has not yet
    /// admitted the PING. **No deadline is armed and nothing has been
    /// emitted** — the mark's only observable below the shell is its
    /// `slither::policy` trace.
    ///
    /// Reachable **only on a connection that has roamed** (or one accepted
    /// and not yet validated): on a validated address the mark and the
    /// transmission fall on the same instant.
    Pending {
        /// The counter the next seal will use, recorded at the mark
        /// (ruling 41 — a counter high-water mark, not a packet identity).
        floor: u64,
    },
    /// The PING went out at `armed_at`; the verdict is due at `deadline`.
    Armed {
        /// The probe floor, unchanged from the mark.
        floor: u64,
        /// When the probe was transmitted.
        armed_at: Instant,
        /// `armed_at + KEEPALIVE_TIMEOUT`.
        deadline: Instant,
    },
}

impl Contested {
    /// Whether a probe is marked but not yet transmitted.
    pub(crate) fn is_pending(&self) -> bool {
        matches!(self, Contested::Pending { .. })
    }

    /// This mark's probe floor, if a mark is outstanding.
    pub(crate) fn floor(&self) -> Option<u64> {
        match self {
            Contested::No => None,
            Contested::Pending { floor } | Contested::Armed { floor, .. } => Some(*floor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_validated_address_admits_anything_and_counts_nothing() {
        let mut budget = Amplification::validated();
        assert!(budget.admits(u64::MAX));
        budget.on_sent(1_000_000);
        assert!(budget.admits(u64::MAX));
        assert_eq!(budget.counters(), None);
    }

    /// The gate is `sent + len <= 3 × recv`, so the boundary is admitted and
    /// one byte past it is not. A degenerate "always admits" build fails the
    /// second assertion; a degenerate "never admits" build fails the first.
    #[test]
    fn the_gate_is_three_times_received_and_the_boundary_is_admitted() {
        let budget = Amplification::arm(0, 100);
        assert!(budget.admits(300), "3 × 100 is admitted");
        assert!(!budget.admits(301), "one byte past it is held");
    }

    #[test]
    fn spend_reduces_the_headroom_and_a_receive_restores_it() {
        let mut budget = Amplification::arm(0, 100);
        budget.on_sent(250);
        assert!(budget.admits(50));
        assert!(!budget.admits(51));
        budget.on_recv(100);
        assert!(budget.admits(350), "3 × 200 − 250");
        assert!(!budget.admits(351));
        assert_eq!(budget.counters(), Some((250, 200)));
    }

    /// Ruling 168's floor is a **high-water mark**: an ACK below it proves
    /// nothing, and the separating negative is what pins that.
    #[test]
    fn only_an_ack_at_or_above_the_floor_validates() {
        let mut budget = Amplification::arm(7, 10);
        budget.on_ack_covering(6);
        assert!(!budget.is_validated(), "below the floor proves nothing");
        budget.on_ack_covering(7);
        assert!(budget.is_validated(), "at the floor is the proof");

        let mut budget = Amplification::arm(7, 10);
        budget.on_ack_covering(9_999);
        assert!(budget.is_validated());
        assert_eq!(budget.counters(), None, "validated: no counters remain");
    }

    /// A validated budget is never re-armed by a stray ACK, and validation
    /// is not undone by later traffic.
    #[test]
    fn validation_is_terminal_until_the_next_arming() {
        let mut budget = Amplification::arm(7, 10);
        budget.on_ack_covering(7);
        budget.on_sent(1_000_000);
        assert!(budget.admits(u64::MAX));
        assert!(budget.is_validated());
    }

    #[test]
    fn the_three_contested_states_are_distinguishable() {
        let now = Instant::now();
        assert!(!Contested::No.is_pending());
        assert_eq!(Contested::No.floor(), None);

        let pending = Contested::Pending { floor: 12 };
        assert!(pending.is_pending());
        assert_eq!(pending.floor(), Some(12));

        let armed = Contested::Armed {
            floor: 12,
            armed_at: now,
            deadline: now + constants::KEEPALIVE_TIMEOUT,
        };
        assert!(!armed.is_pending(), "armed is not pending");
        assert_eq!(armed.floor(), Some(12));
        assert_ne!(pending, armed, "the pending gap is its own state");
    }
}

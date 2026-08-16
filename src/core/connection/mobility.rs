//! §7.3's anti-amplification budget and §7.5's contested mark.
//!
//! Two small state machines, kept here rather than as loose fields on
//! [`Connection`](super::Connection) for one reason each.
//!
//! [`Amplification`] is **one guarded predicate**. Ruling 168 reversed a
//! recorded declination — the budget *disarms* on a return-routability
//! proof — and the maintainer flagged it as the ruling most wanted attacked
//! in the post-slice protocol review. It was, and it fell: **[ruling 208]**
//! replaces the ACK predicate with an unforgeable challenge. A budget
//! threaded as four bare fields through six call sites could not have been
//! re-predicated without touching all six; a type with
//! `admits`/`on_sent`/`on_recv` was.
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
/// # Disarmed by a return-routability proof (rulings 168, **208**)
///
/// The address becomes validated when an authenticated, window-fresh packet
/// *from that address* carries a `PATH_RESPONSE` echoing this arming's
/// eight-byte [`challenge`](Self::challenge). Only a party that **received**
/// something we sent to that address after the change can produce one.
///
/// **Ruling 208 supersedes ruling 168's predicate entirely, and the two are
/// alternatives rather than complements.** 168 validated on an ACK covering
/// a floor, reasoning that *"only an ACK proves the peer receives at the
/// address we are sending to"*. An ACK is four plaintext integers under
/// AEAD: `largest` is **not** a proof of receipt, it is an assertion by
/// whoever holds the key — and §7.3's roaming threat model *is* the key
/// holder. A connected peer announced a move to a victim, waited for one
/// sealed packet, and returned a forged ACK spoofed from the victim: two
/// small packets, after which reflection was unbounded. Leaving
/// `on_ack_covering` in place beside `on_path_response` would leave that
/// bypass unlocked, which is why it is **gone** and not merely unused.
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
    /// **[ruling 208]** This arming's challenge: the eight bytes a
    /// `PATH_RESPONSE` must echo to validate this address. Drawn fresh at
    /// every arming and never reused across armings — re-arming with the
    /// previous value would let a peer bank a response.
    challenge: [u8; 8],
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
    ///
    /// The challenge is an inert `[0u8; 8]` and is never compared —
    /// [`on_path_response`](Self::on_path_response) short-circuits on
    /// `validated` before it reaches the bytes.
    pub(crate) const fn validated() -> Self {
        Self {
            validated: true,
            challenge: [0u8; 8],
            sent: 0,
            recv: 0,
        }
    }

    /// Arm the budget on a new, unvalidated address.
    ///
    /// `challenge` is **[ruling 208]**'s eight bytes for this arming, drawn
    /// by the caller from §16.6's per-connection sub-seed; `credit` is the
    /// triggering packet's datagram length, which is authenticated and
    /// window-fresh by construction on both arming paths and so may fund the
    /// budget under ruling 169.
    pub(crate) const fn arm(challenge: [u8; 8], credit: u64) -> Self {
        Self {
            validated: false,
            challenge,
            sent: 0,
            recv: credit,
        }
    }

    /// This arming's outstanding challenge, or `None` once validated.
    ///
    /// `None` is what stops the pump re-offering a challenge to an address
    /// that has already answered one.
    pub(crate) fn outstanding_challenge(&self) -> Option<[u8; 8]> {
        (!self.validated).then_some(self.challenge)
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

    /// The datagram bytes this address may still be sent — `3 × recv −
    /// sent` — or `None` when the address is validated and no cap binds.
    ///
    /// # This is not a second predicate
    ///
    /// **[ruling 207(a)]** `admits` is correct and does not move. This is
    /// its **inverse**, added for one purpose: so the sender can size what
    /// it *builds* to what the budget will *permit* (ruling 203). Nothing
    /// here decides admission, and no caller may use it to send what
    /// [`admits`](Self::admits) would refuse.
    ///
    /// The two agree exactly. `admits(len)` is `sent + len <= 3 × recv`, and
    /// `len <= room()` is the same inequality rearranged, because
    /// `sent <= 3 × recv` always holds: [`on_sent`](Self::on_sent) is only
    /// ever called for a length `admits` has already passed, on every one of
    /// its five call sites.
    ///
    /// # Units
    ///
    /// **Datagram** bytes, like both counters. **[ruling 207(c)]** a caller
    /// sizing a *plaintext* must first subtract §3.4's `DATA_HEADER_LEN +
    /// AEAD_TAG_LEN`; the two units differ by exactly that overhead, and a
    /// plaintext capped at the remaining datagram bytes overshoots by 30 and
    /// re-refuses its own packet.
    pub(crate) fn room(&self) -> Option<u64> {
        (!self.validated).then(|| {
            constants::AMPLIFICATION_FACTOR
                .saturating_mul(self.recv)
                .saturating_sub(self.sent)
        })
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

    /// **[ruling 208]** The proof of return routability: a `PATH_RESPONSE`
    /// **from this address** echoing this arming's challenge.
    ///
    /// The caller owes the *from this address* half — it is not visible
    /// here, and it is the whole predicate: a peer echoing from its old
    /// address would otherwise validate the new one.
    ///
    /// A response that does not match is a **semantic no-op** (§8.4, ruling
    /// 212(d)): it validates nothing and is not an error. Killing the
    /// connection on it would hand any off-path party that can guess a frame
    /// boundary a remote kill primitive requiring no key.
    ///
    /// # The comparison folds the whole difference
    ///
    /// `==` on `[u8; 8]` may compile to a short-circuiting `memcmp`, which
    /// turns one guess in 2⁶⁴ into a 8 × 256 byte-at-a-time walk for an
    /// attacker who can measure. The measurement sits behind AEAD and frame
    /// parsing and is probably impractical — which is the argument for not
    /// worrying, and is the argument this project has twice declined.
    /// `packet::mac::Mac1Key::verify` has the same shape for the same
    /// reason; this costs nothing and deletes the question.
    pub(crate) fn on_path_response(&mut self, echo: &[u8; 8]) {
        if self.validated {
            return;
        }
        let mut diff = 0u8;
        for (a, b) in self.challenge.iter().zip(echo) {
            diff |= a ^ b;
        }
        if diff == 0 {
            *self = Self::validated();
        }
    }

    /// Whether the address is validated — i.e. no budget is armed.
    pub(crate) fn is_validated(&self) -> bool {
        self.validated
    }

    /// `(sent, received)` in datagram bytes, for tests. `None` when
    /// validated.
    #[cfg(test)]
    pub(crate) fn counters(&self) -> Option<(u64, u64)> {
        (!self.validated).then_some((self.sent, self.recv))
    }

    /// This arming's challenge bytes, for tests.
    ///
    /// **`set_floor` went with ruling 168's predicate and needs no
    /// replacement.** It existed for one reason: the msg1-anchor arming
    /// happens in [`Connection::established`](super::Connection::established)
    /// *before* the session is installed, and the floor — *"the counter the
    /// next seal will use"* — is unreadable until the install. A challenge
    /// has no such dependency: it comes from the connection's own RNG, which
    /// exists from `connecting()`. The two-phase arming collapses to one.
    #[cfg(test)]
    pub(crate) fn challenge(&self) -> [u8; 8] {
        self.challenge
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
    /// The eight bytes every armed budget in these tests carries. Their
    /// value is irrelevant to the ratio; what matters is that a *different*
    /// eight bytes proves nothing.
    const CHALLENGE: [u8; 8] = [0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18];

    #[test]
    fn the_gate_is_three_times_received_and_the_boundary_is_admitted() {
        let budget = Amplification::arm(CHALLENGE, 100);
        assert!(budget.admits(300), "3 × 100 is admitted");
        assert!(!budget.admits(301), "one byte past it is held");
    }

    #[test]
    fn spend_reduces_the_headroom_and_a_receive_restores_it() {
        let mut budget = Amplification::arm(CHALLENGE, 100);
        budget.on_sent(250);
        assert!(budget.admits(50));
        assert!(!budget.admits(51));
        budget.on_recv(100);
        assert!(budget.admits(350), "3 × 200 − 250");
        assert!(!budget.admits(351));
        assert_eq!(budget.counters(), Some((250, 200)));
    }

    /// **[ruling 208]** The migrated premise of
    /// `only_an_ack_at_or_above_the_floor_validates`: what proves nothing is
    /// no longer *an ACK below the floor* but *a `PATH_RESPONSE` carrying
    /// the wrong bytes*. The separating negative is the whole test — a build
    /// that validated on any response at all would pass the positive half
    /// alone.
    ///
    /// The near-miss is deliberate: one byte differs, in the last position,
    /// which is where a short-circuiting comparison would be most nearly
    /// right.
    #[test]
    fn only_a_response_echoing_this_armings_challenge_validates() {
        let mut budget = Amplification::arm(CHALLENGE, 10);
        assert_eq!(budget.outstanding_challenge(), Some(CHALLENGE));

        let mut near_miss = CHALLENGE;
        near_miss[7] ^= 0x01;
        budget.on_path_response(&near_miss);
        assert!(
            !budget.is_validated(),
            "eight bytes that are not the challenge prove nothing"
        );
        budget.on_path_response(&[0u8; 8]);
        assert!(!budget.is_validated(), "nor do eight zeroes");

        budget.on_path_response(&CHALLENGE);
        assert!(budget.is_validated(), "the echo is the proof");
        assert_eq!(budget.counters(), None, "validated: no counters remain");
        assert_eq!(
            budget.outstanding_challenge(),
            None,
            "nothing is owed to an address that has answered"
        );
    }

    /// A validated budget is never re-armed by a stray response, and
    /// validation is not undone by later traffic.
    #[test]
    fn validation_is_terminal_until_the_next_arming() {
        let mut budget = Amplification::arm(CHALLENGE, 10);
        budget.on_path_response(&CHALLENGE);
        budget.on_path_response(&[0u8; 8]);
        budget.on_sent(1_000_000);
        assert!(budget.admits(u64::MAX));
        assert!(budget.is_validated());
    }

    /// **[ruling 208]** *"One challenge per arming, never reused across
    /// armings"* — so a response banked against the previous arming
    /// validates nothing after the next one.
    #[test]
    fn a_response_to_the_previous_arming_is_worthless_after_a_re_arm() {
        let stale = CHALLENGE;
        let mut fresh = CHALLENGE;
        fresh[0] ^= 0xff;

        let mut budget = Amplification::arm(fresh, 10);
        budget.on_path_response(&stale);
        assert!(
            !budget.is_validated(),
            "the previous arming's bytes are not this arming's question"
        );
        budget.on_path_response(&fresh);
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

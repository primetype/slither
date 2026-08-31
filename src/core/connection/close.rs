//! §15 — CLOSE, the post-mortem states, and the linger's reply rule.
//!
//! §15.1: *"Only the **authenticated, in-seal** CLOSE exists — nothing
//! unauthenticated can kill a connection; the reserved cleartext close
//! packet type (`0x04`) stays dead."*
//!
//! # Three post-mortem states, not two
//!
//! §15.2 reads as two paragraphs and specifies three behaviours:
//!
//! | State | Entered by | Emits on inbound | Local surface |
//! |---|---|---|---|
//! | **closing** | local `close()`, a protocol violation | ≤ 1 CLOSE/s, to the **session's endpoint address** | `LocallyClosed` / `ProtocolViolation { code }` |
//! | **draining** | the peer's CLOSE, received while alive | **nothing** | `PeerClosed { code, reason }` |
//! | closing → draining | a CLOSE arriving while closing | nothing thereafter | unchanged |
//!
//! The third row is §15.2's *"two closing endpoints go quiet rather than
//! ping-ponging replies at 1 Hz for the linger."*
//!
//! # The linger clock does not restart
//!
//! §15.2 says the drain runs for "**the same** `CLOSE_LINGER`" and does not
//! say whether a CLOSE arriving mid-linger restarts it. It does not: a
//! restart lets an authenticated peer hold our post-mortem state open
//! indefinitely by re-CLOSEing at 4.9 s, which is an unbounded hold against
//! state §17.5 exists to bound, and §16.5's governing principle is that
//! state removal comes first.
//!
//! # What the reply rate is anchored on
//!
//! §15.2 caps **replies** at one CLOSE per second and is silent on whether
//! the CLOSE that opened the closing state starts that clock. It does not:
//! `last_reply` begins `None`, so the first authenticated, window-fresh
//! packet after `close()` is always answered. The alternative would make a
//! peer that speaks inside the first second get no reply at all, which is
//! the case the reply rule exists for.

use std::time::Instant;

use crate::constants;
use crate::core::Deadline;

use super::frame::Close;

/// Where a connection is in §15's lifecycle.
pub(crate) enum Lifecycle {
    /// Alive: the ordinary state.
    Live,
    /// §15.2's **closing**: we sent a CLOSE and owe rate-limited replies.
    Closing(Closing),
    /// §15.2's **draining**: reply-free, discarding late packets.
    Draining {
        /// The `CloseLinger` expiry. Never moved once set.
        until: Deadline,
    },
    /// State dropped, `Retired` emitted. Nothing further happens.
    Dead,
}

impl Lifecycle {
    /// Whether the connection is still alive — i.e. whether the
    /// application surface and the frame appliers may still run.
    pub(crate) fn is_live(&self) -> bool {
        matches!(self, Lifecycle::Live)
    }

    /// Whether all state has been dropped.
    pub(crate) fn is_dead(&self) -> bool {
        matches!(self, Lifecycle::Dead)
    }

    /// The post-mortem expiry, if one is running.
    pub(crate) fn linger_deadline(&self) -> Option<Deadline> {
        match self {
            Lifecycle::Live | Lifecycle::Dead => None,
            Lifecycle::Closing(closing) => Some(closing.until),
            Lifecycle::Draining { until } => Some(*until),
        }
    }

    /// The representable post-mortem expiry, if one can be announced.
    #[cfg(test)]
    pub(crate) fn linger_until(&self) -> Option<Instant> {
        self.linger_deadline().and_then(Deadline::as_instant)
    }
}

/// §15.2's closing state.
///
/// It retains the seal capability, the receive cipher states and the replay
/// window — that retention list is exhaustive, so all stream, flow-control,
/// recovery and congestion state has already been dropped and there is
/// nothing left for a STREAM or MAX_DATA frame to be applied *to*.
pub(crate) struct Closing {
    /// The `CloseLinger` expiry.
    until: Deadline,
    /// The CLOSE we sent, re-sent verbatim as each reply.
    close: Close,
    /// When the last **reply** went out. `None` until the first one — the
    /// opening CLOSE is not a reply.
    last_reply: Option<Instant>,
}

impl Closing {
    /// Enter the closing state at `now`, having just emitted `close`.
    pub(crate) fn new(now: Instant, close: Close) -> Self {
        Self {
            until: Deadline::after(now, constants::CLOSE_LINGER),
            close,
            last_reply: None,
        }
    }

    /// The `CloseLinger` expiry.
    pub(crate) fn until(&self) -> Deadline {
        self.until
    }

    /// The CLOSE to reply with, **if** the rate rule admits one now.
    ///
    /// Call only for an **authenticated, window-fresh** inbound packet:
    /// §15.2 owes a reply to nothing else, and explicitly not to a packet
    /// that merely routed by `receiver_index`, which an off-path forger who
    /// observed the cleartext index could mint.
    pub(crate) fn reply(&mut self, now: Instant) -> Option<Close> {
        let due = match self.last_reply {
            None => true,
            Some(last) => {
                now.saturating_duration_since(last) >= constants::CLOSE_REPLY_MIN_INTERVAL
            }
        };
        if !due {
            return None;
        }
        self.last_reply = Some(now);
        Some(self.close.clone())
    }

    /// §15.2's closing → draining transition, keeping the **existing**
    /// deadline.
    pub(crate) fn into_draining(self) -> Lifecycle {
        Lifecycle::Draining { until: self.until }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn close_frame() -> Close {
        Close::new(constants::NO_ERROR, b"bye")
    }

    #[test]
    fn closing_lingers_for_close_linger() {
        let now = Instant::now();
        let closing = Closing::new(now, close_frame());
        assert_eq!(
            closing.until(),
            Deadline::after(now, constants::CLOSE_LINGER)
        );
    }

    /// The first inbound packet is answered however soon it arrives — the
    /// opening CLOSE is not itself a reply.
    #[test]
    fn the_first_reply_is_not_rate_limited_by_the_opening_close() {
        let now = Instant::now();
        let mut closing = Closing::new(now, close_frame());
        assert_eq!(closing.reply(now), Some(close_frame()));
    }

    /// Many inbound packets inside one second produce **exactly one**
    /// reply — and at least one, which is what separates the rule from
    /// "never replies".
    #[test]
    fn replies_are_capped_at_one_per_second() {
        let now = Instant::now();
        let mut closing = Closing::new(now, close_frame());

        let mut replies = 0;
        for step in 0..10 {
            if closing
                .reply(now + Duration::from_millis(step * 90))
                .is_some()
            {
                replies += 1;
            }
        }
        assert_eq!(replies, 1, "ten packets inside one second, one reply");

        // And the cap opens again exactly at the interval, not before.
        assert!(
            closing
                .reply(now + constants::CLOSE_REPLY_MIN_INTERVAL - Duration::from_millis(1))
                .is_none()
        );
        assert!(
            closing
                .reply(now + constants::CLOSE_REPLY_MIN_INTERVAL)
                .is_some()
        );
    }

    /// A stale shell timestamp cannot panic the core or move the reply
    /// clock backwards. It is simply still inside the current rate window.
    #[test]
    fn a_backwards_timestamp_is_not_due_and_does_not_panic() {
        let now = Instant::now();
        let mut closing = Closing::new(now, close_frame());
        assert_eq!(closing.reply(now), Some(close_frame()));

        let stale = now
            .checked_sub(Duration::from_nanos(1))
            .expect("the ordinary test instant has a predecessor");
        assert_eq!(closing.reply(stale), None);
        assert_eq!(
            closing.reply(now + constants::CLOSE_REPLY_MIN_INTERVAL),
            Some(close_frame()),
            "the stale observation must not move the real reply anchor"
        );
    }

    /// Every reply is the CLOSE we sent, verbatim.
    #[test]
    fn a_reply_is_the_original_close() {
        let now = Instant::now();
        let close = Close::new(0x2a, b"a reason");
        let mut closing = Closing::new(now, close.clone());
        assert_eq!(closing.reply(now), Some(close.clone()));
        assert_eq!(
            closing.reply(now + constants::CLOSE_REPLY_MIN_INTERVAL),
            Some(close)
        );
    }

    /// §15.2 / Q2: closing → draining keeps the original deadline. A
    /// restart would let a peer hold the post-mortem open indefinitely.
    #[test]
    fn closing_to_draining_keeps_the_original_deadline() {
        let now = Instant::now();
        let closing = Closing::new(now, close_frame());
        let expiry = closing.until();

        let draining = closing.into_draining();
        assert_eq!(draining.linger_deadline(), Some(expiry));
        assert!(!draining.is_live());
    }

    #[test]
    fn live_and_dead_have_no_linger() {
        assert_eq!(Lifecycle::Live.linger_until(), None);
        assert!(Lifecycle::Live.is_live());
        assert_eq!(Lifecycle::Dead.linger_until(), None);
        assert!(Lifecycle::Dead.is_dead());
    }
}

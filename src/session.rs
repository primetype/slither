//! An established datagram session: the sealed-packet seal/open path, the RFC
//! 6479-style anti-replay window, endpoint roaming, and the WireGuard timers.
//!
//! A [`Session`] wraps the crypto outputs of a completed handshake
//! ([`Established`](crate::handshake::Established)) with the state a live datagram
//! flow needs. It is transport-agnostic — it produces and consumes packet
//! **bytes**; the [`endpoint`](crate::endpoint) actor moves them over the
//! [`Wire`](crate::endpoint::Wire).

use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::time::Duration;

use hiss::curve::p256::P256r1PublicKey;
// The session timers must track tokio's clock, so a paused test runtime advances
// them (a `std::time::Instant` would ignore the virtual clock entirely).
use hiss::noise::{DatagramRecv, DatagramSend, SessionId};
use packtool::Packed;
use tokio::time::Instant;

use crate::SlitherChannel;
use crate::handshake::{Established, HsError};
use crate::wire::{AEAD_TAG_LEN, DataHeader, MAX_PLAINTEXT};

// ── Session timers (ratified; the full table lives in slither/SPEC.md) ─────────

/// Received-but-not-sent this long ⇒ send an empty-Data keepalive.
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Sent-but-nothing-received this long ⇒ the session is dead.
pub const DEAD_TIMEOUT: Duration = Duration::from_secs(15);
/// The optional persistent-keepalive interval (off by default).
pub const PERSISTENT_KEEPALIVE: Duration = Duration::from_secs(25);
/// A session older than this initiates a fresh handshake on the next send.
pub const REKEY_AGE: Duration = Duration::from_secs(120);
/// The payload-seal backstop: application payload (a fresh send or a DATA
/// retransmission) may not be sealed under a session older than this that has
/// failed to complete its DH rekey — the endpoint tears the session down there.
/// Idle liveness and inbound opening are age-exempt (age gates payload, not
/// liveness — the maintainer, 2026/07/17).
pub const REJECT_AGE: Duration = Duration::from_secs(180);

/// The key-ratchet epoch size, in messages: the transport keys ratchet forward
/// (the Noise §11.3 `Rekey()` transform) once every `REKEY_EPOCH_MSGS` counter
/// values, **per direction**, so a long-lived session retires ageing key
/// material without a re-handshake (hygiene by rotation; the age-death is gone —
/// the maintainer, 2026/07/17). Both peers MUST use this exact value: it is part
/// of the protocol, not a tuning knob — a mismatch desynchronises which key
/// opens which packet. FROZEN (slither/SPEC.md §"Key ratchet", ratified
/// 2026/07/17); `vN` rule.
pub const REKEY_EPOCH_MSGS: NonZeroU64 = NonZeroU64::new(65_536).expect("65_536 is nonzero");

/// The anti-replay sliding-window width, in counters (RFC 6479 shape).
pub const REPLAY_WINDOW: u64 = 128;

// ── The anti-replay window ────────────────────────────────────────────────────

/// A greatest-counter + 128-bit sliding bitmap, in the style of RFC 6479 /
/// WireGuard.
///
/// [`admit`](Self::admit) is called **only after** `decrypt_at` has authenticated
/// a packet: it returns `true` for a fresh counter (recording it), and `false`
/// for a duplicate or a counter that has fallen out of the window — both of which
/// the caller then drops **without delivery**. `bit 0` tracks the greatest counter
/// seen; `bit k` tracks `greatest − k`.
#[derive(Debug, Default, Clone)]
struct ReplayWindow {
    greatest: u64,
    bitmap: u128,
    primed: bool,
}

impl ReplayWindow {
    fn admit(&mut self, counter: u64) -> bool {
        if !self.primed {
            self.primed = true;
            self.greatest = counter;
            self.bitmap = 1; // bit 0 marks the greatest (this) counter
            return true;
        }
        if counter > self.greatest {
            let shift = counter - self.greatest;
            if shift >= REPLAY_WINDOW {
                // Everything previously seen is now out of the window.
                self.bitmap = 1;
            } else {
                self.bitmap <<= shift;
                self.bitmap |= 1;
            }
            self.greatest = counter;
            true
        } else {
            let diff = self.greatest - counter;
            if diff >= REPLAY_WINDOW {
                return false; // older than the window
            }
            let mask = 1u128 << diff;
            if self.bitmap & mask != 0 {
                false // duplicate
            } else {
                self.bitmap |= mask;
                true
            }
        }
    }

    /// The window's received-counter state — `(greatest, bitmap)` where `bit k`
    /// marks `greatest − k` as received — or `None` before any packet arrived.
    ///
    /// This is the Leg 2 ACK-generation source: the frame layer builds its ACK
    /// ranges straight from this bitmap rather than keeping a second
    /// received-packet record (the brief's reuse-don't-duplicate rule).
    fn snapshot(&self) -> Option<(u64, u128)> {
        if self.primed {
            Some((self.greatest, self.bitmap))
        } else {
            None
        }
    }
}

// ── The outcome of opening an inbound Data packet ─────────────────────────────

/// What opening an inbound Data packet produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The recovered application plaintext — `Some` only for a fresh, non-empty
    /// payload. `None` for a keepalive (empty plaintext) or a dropped duplicate.
    pub payload: Option<Vec<u8>>,
    /// If this fresh, authenticated packet arrived from a **new** source, the
    /// previous endpoint address (the session has already roamed to the new one).
    pub moved_from: Option<SocketAddr>,
}

// ── The session ───────────────────────────────────────────────────────────────

/// An established datagram session between this endpoint and one peer.
pub struct Session {
    send: DatagramSend<SlitherChannel>,
    recv: DatagramRecv<SlitherChannel>,
    session_id: SessionId,
    /// The counter the next [`seal`](Self::seal) will use — mirrors the hiss
    /// send counter so the Data header (the AEAD associated data) can carry it.
    next_counter: u64,
    replay: ReplayWindow,

    /// Our index — inbound Data for this session carries it in `receiver_index`.
    our_index: u32,
    /// The peer's index — we stamp it into outbound Data's `receiver_index`.
    peer_index: u32,
    /// The peer's authenticated long-term static.
    remote_static: P256r1PublicKey,
    /// The peer's current source address (moves on authenticated roaming).
    endpoint: SocketAddr,

    established_at: Instant,
    last_recv: Instant,
    last_send: Instant,
}

impl Session {
    /// Wrap a completed handshake's crypto outputs into a live session anchored
    /// at the peer's current `endpoint` address, as of `now`.
    pub fn new(established: Established, endpoint: SocketAddr, now: Instant) -> Self {
        Self {
            send: established.send,
            recv: established.recv,
            session_id: established.session_id,
            next_counter: 0,
            replay: ReplayWindow::default(),
            our_index: established.our_index,
            peer_index: established.peer_index,
            remote_static: established.remote_static,
            endpoint,
            established_at: now,
            last_recv: now,
            last_send: now,
        }
    }

    /// Our session index (inbound Data routing key).
    pub fn our_index(&self) -> u32 {
        self.our_index
    }

    /// The peer's current source address.
    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    /// The peer's authenticated long-term static.
    pub fn remote_static(&self) -> &P256r1PublicKey {
        &self.remote_static
    }

    /// The shared session identifier.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Seal one application `plaintext` into a Data packet, marking `now` as the
    /// last send. An empty `plaintext` produces the tag-only keepalive.
    ///
    /// Errors if `plaintext` exceeds [`MAX_PLAINTEXT`]. The Data header carries
    /// the exact hiss send counter, which is the AEAD associated data — the
    /// counter is predicted from the mirrored [`next_counter`](Self::next_counter)
    /// and asserted against the value `encrypt_next` returns.
    pub fn seal(&mut self, plaintext: &[u8], now: Instant) -> Result<Vec<u8>, SessionError> {
        let packet = self.seal_inner(plaintext)?;
        self.last_send = now;
        Ok(packet)
    }

    /// Seal a Leg 2 **control** packet (a pure ACK, a PTO probe, a frame
    /// retransmission) **without touching the last-send liveness clock**.
    ///
    /// The ratified Leg 1 timer table measures the keepalive and dead deadlines
    /// from the last send; if the frame layer's automatic control traffic
    /// refreshed that clock, an unending PTO probe train would defer
    /// [`DEAD_TIMEOUT`] indefinitely, and an immediate ACK would suppress the
    /// ruled 10-s keepalive dance. Control packets therefore ride the identical
    /// sealed-Data wire but are liveness-neutral: only fresh application traffic
    /// (and the Leg 1 keepalive itself) marks a send. Ratified (Leg 2) — see
    /// `slither/SPEC.md` §9.
    pub(crate) fn seal_quiet(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SessionError> {
        self.seal_inner(plaintext)
    }

    fn seal_inner(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SessionError> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(SessionError::PayloadTooLarge {
                len: plaintext.len(),
                max: MAX_PLAINTEXT,
            });
        }
        let counter = self.next_counter;
        let header = DataHeader::new(self.peer_index, counter);
        let ad = header.to_bytes();

        let mut out = vec![0u8; plaintext.len() + AEAD_TAG_LEN];
        let (used, n) = self
            .send
            .encrypt_next(&ad, plaintext, &mut out)
            .map_err(|e| SessionError::Crypto(e.to_string()))?;
        debug_assert_eq!(
            used, counter,
            "mirrored counter tracks the hiss send counter"
        );
        self.next_counter = used + 1;

        let mut packet = Vec::with_capacity(DataHeader::SIZE + n);
        packet.extend_from_slice(&ad);
        packet.extend_from_slice(&out[..n]);
        Ok(packet)
    }

    /// The received-counter state of the replay window — the Leg 2 ACK source
    /// ([`ReplayWindow::snapshot`]); `None` before any packet has been opened.
    pub(crate) fn ack_window(&self) -> Option<(u64, u128)> {
        self.replay.snapshot()
    }

    /// The highest counter this session has sealed, or `None` if it has sealed
    /// nothing — the Leg 2 bound for validating a peer ACK's largest.
    pub(crate) fn last_counter(&self) -> Option<u64> {
        self.next_counter.checked_sub(1)
    }

    /// Open an inbound Data `packet` received from `src` at `now`.
    ///
    /// Decrypts under the packet's own 14-byte header as associated data, then —
    /// only on AEAD success — consults the replay window. A duplicate or
    /// out-of-window counter is dropped **after** decryption without delivery. A
    /// fresh, authenticated packet updates the liveness clock and, if `src`
    /// differs from the current endpoint, roams the session. A tampered or forged
    /// packet is [`HsError::Crypto`]; a structurally short packet is
    /// [`HsError::Drop`]. Nothing unauthenticated ever moves the endpoint.
    pub fn open(
        &mut self,
        packet: &[u8],
        src: SocketAddr,
        now: Instant,
    ) -> Result<Received, HsError> {
        let (header, ciphertext) = DataHeader::parse(packet).ok_or(HsError::Drop("data: short"))?;
        let ad = header.to_bytes();
        let counter = header.counter();

        let mut out = vec![0u8; ciphertext.len()];
        let n = self.recv.decrypt_at(counter, &ad, ciphertext, &mut out)?;

        // Authenticated. Replay-check before we act on it in any way.
        if !self.replay.admit(counter) {
            return Ok(Received {
                payload: None,
                moved_from: None,
            });
        }

        // A fresh, authenticated packet: roam (if the source moved) and refresh
        // the liveness clock.
        let moved_from = if src != self.endpoint {
            let previous = self.endpoint;
            self.endpoint = src;
            Some(previous)
        } else {
            None
        };
        self.last_recv = now;

        let payload = if n == 0 {
            None // keepalive
        } else {
            Some(out[..n].to_vec())
        };
        Ok(Received {
            payload,
            moved_from,
        })
    }

    /// Whether the session owes a keepalive at `now`: it has received since it
    /// last sent and [`KEEPALIVE_TIMEOUT`] has elapsed, or (if `persistent` is
    /// enabled) the persistent interval has elapsed since the last send.
    pub fn should_keepalive(&self, now: Instant, persistent: Option<Duration>) -> bool {
        let received_owes = self.last_recv > self.last_send
            && now.saturating_duration_since(self.last_recv) >= KEEPALIVE_TIMEOUT;
        let persistent_owes =
            persistent.is_some_and(|p| now.saturating_duration_since(self.last_send) >= p);
        received_owes || persistent_owes
    }

    /// Whether the session is dead at `now`: it has an outstanding send (sent more
    /// recently than it received) and nothing has arrived for [`DEAD_TIMEOUT`]
    /// **since that send**.
    ///
    /// The window is measured from the last send, not the last receive, so the
    /// 10 s keepalive dance survives: a side sends a keepalive, and the peer's
    /// reply (its own keepalive, up to 10 s later) lands well inside the 15 s
    /// window before this side would give up.
    pub fn is_dead(&self, now: Instant) -> bool {
        self.last_send > self.last_recv
            && now.saturating_duration_since(self.last_send) >= DEAD_TIMEOUT
    }

    /// The session's age at `now`.
    pub fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.established_at)
    }

    /// Whether the session is old enough ([`REKEY_AGE`]) that the next send should
    /// initiate a fresh handshake.
    pub fn needs_rekey(&self, now: Instant) -> bool {
        self.age(now) >= REKEY_AGE
    }

    /// Whether the session has passed [`REJECT_AGE`] — the payload-seal
    /// backstop.
    ///
    /// Consulted **only where application payload is about to be sealed** (a
    /// fresh send or a DATA retransmission): such payload may not ride a session
    /// this old that has failed to complete its DH rekey, so the endpoint tears
    /// it down there. Idle liveness and inbound opening are age-exempt (age
    /// gates payload, not liveness — the maintainer, 2026/07/17).
    pub fn is_expired(&self, now: Instant) -> bool {
        self.age(now) >= REJECT_AGE
    }
}

/// A failure sealing an outbound Data packet.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SessionError {
    /// The plaintext exceeds [`MAX_PLAINTEXT`].
    #[error("payload is {len} bytes, over the {max}-byte maximum")]
    PayloadTooLarge {
        /// The offered plaintext length.
        len: usize,
        /// The maximum permitted ([`MAX_PLAINTEXT`]).
        max: usize,
    },
    /// The AEAD seal failed (nonce exhaustion — not reachable in practice).
    #[error("seal failed: {0}")]
    Crypto(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_admits_in_order() {
        let mut w = ReplayWindow::default();
        for counter in 0..1000u64 {
            assert!(w.admit(counter), "fresh in-order counter {counter}");
        }
    }

    #[test]
    fn window_rejects_duplicates() {
        let mut w = ReplayWindow::default();
        assert!(w.admit(0));
        assert!(!w.admit(0), "immediate duplicate");
        assert!(w.admit(1));
        assert!(w.admit(2));
        assert!(!w.admit(1), "in-window duplicate");
        assert!(!w.admit(2), "in-window duplicate");
    }

    #[test]
    fn window_admits_reorder_within_window_once() {
        let mut w = ReplayWindow::default();
        // Advance to 200, then deliver an in-window straggler exactly once.
        assert!(w.admit(200));
        assert!(w.admit(150), "150 is within 128? no — 50 back, in window");
        assert!(!w.admit(150), "straggler is a duplicate the second time");
        // 200 - 128 = 72 is the oldest in-window index.
        assert!(w.admit(73), "just inside the window");
        assert!(!w.admit(72), "exactly the window edge is out (diff == 128)");
        assert!(!w.admit(10), "far below the window");
    }

    #[test]
    fn window_handles_large_jump() {
        let mut w = ReplayWindow::default();
        assert!(w.admit(5));
        assert!(w.admit(5 + REPLAY_WINDOW), "a jump past the whole window");
        assert!(
            !w.admit(5),
            "everything below the new window is now rejected"
        );
        assert!(
            w.admit(5 + REPLAY_WINDOW - 1),
            "still in the new window, fresh"
        );
    }
}

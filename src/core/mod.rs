//! §16.4 — the two sans-io cores and the poll contract.
//!
//! `Endpoint<I>` and `Connection<C>` are pure state machines. **`now:
//! Instant` is an argument on every mutating call and neither core ever
//! reads a clock**, so every behaviour in this module is drivable by
//! arithmetic on an `Instant` — no runtime, no paused clock, no `LocalSet`.
//!
//! # The poll contract
//!
//! §16.4: *"every mutating call … is followed by draining `poll_output()`
//! to the terminal `Timeout(Option<Instant>)`, which is simultaneously the
//! drain sentinel and the next-deadline announcement — a driver cannot
//! forget to drain."* `Timeout(None)` means drained with no deadline armed;
//! `Timeout(Some(d))` means drained, next deadline `d`. **Output ordering
//! within one drain preserves generation order** and is normative: a
//! transmit and the event it caused come out in that order.
//!
//! # Scope of this slice
//!
//! Slice 2a is the endpoint core: §5.3–5.7, §6.1, §6.3, §16.4–16.6 and
//! §17.1–17.4. [`Connection`] exists only because §16.4's `connect()` and
//! `accept()` signatures force it to; its §7–§15 surface arrives with the
//! later slices, and [`connection`]'s module docs bound that precisely.
//!
//! Not here, with where each goes: §6.2's `async` staged handles and the
//! driver actor (slice 3, because each staged verb is a driver round-trip);
//! §6.4's re-home, §6.5's routing and hint consultation, and §6.6–6.8's
//! tie-break and restart (slice 7); §7.3's amplification budget (slice 7 —
//! §5.6 arms it here by recording the anchor, and the budget is enforced
//! there).

// Slice 2a builds the core; the shell that drives it is slice 3, so from
// `cargo build`'s point of view almost everything here is unreachable while
// being exactly the surface §16.4 specifies. The same allow, for the same
// reason, that `packet` carried through slice 1. It comes off when the
// driver lands.
#![allow(dead_code)]

pub(crate) mod connection;
pub(crate) mod endpoint;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::time::Instant;

use crate::constants;
use crate::error::ConnectError;
use crate::packet::{Handshake, Msg1Payload};

// `IntroId` is publicly reachable through the crate root: `WallClock` and
// slice 3's `Intro` handle both name it, and a `pub(crate)` re-export here
// would make that re-export illegal (E0365).
pub use self::endpoint::IntroId;

// Same reason as `packet`'s: the driver that consumes these is slice 3b.
#[allow(unused_imports)]
pub(crate) use self::connection::{ConnEvent, ConnOutput, Connection};
// §16.5's named timers. Re-exported because the driver arms nothing itself
// — it only announces `Timeout(next)` — but names them in its trace and in
// its tests, and a second path to the same enum is a second place to get
// ruling 76's order wrong.
#[allow(unused_imports)]
pub(crate) use self::connection::timers::TimerKind;
#[allow(unused_imports)]
pub(crate) use self::endpoint::Endpoint;

/// A connection's identity inside one endpoint. Monotone, never reused.
///
/// Opaque on purpose: nothing in the protocol is keyed on its value, and it
/// never appears on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// §5.2's initiation timestamp: seconds and nanoseconds since the Unix
/// epoch, the one wall-clock reading in the protocol (§5.3).
///
/// # The derived ordering is the wire ordering
///
/// The wire encoding is `secs: u64 ‖ nanos: u32`, **big-endian** — the
/// crate's one big-endian integer pair. Declaring `secs` before `nanos` and
/// deriving `Ord` therefore gives an ordering that is byte-for-byte the
/// lexicographic order of those 12 octets, for **every** input including a
/// `nanos` at or above 1 000 000 000.
///
/// That is load-bearing twice over. §17.1's guard compares "strictly
/// greater" and needs no normalisation, and — more importantly — **no
/// validation gate may be invented here**: §5.3 specifies none, so a
/// slither that rejected an out-of-range `nanos` would have added a wire
/// behaviour the spec does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    secs: u64,
    nanos: u32,
}

impl Timestamp {
    /// A timestamp from its two wire fields.
    pub const fn new(secs: u64, nanos: u32) -> Self {
        Self { secs, nanos }
    }

    /// Seconds since the Unix epoch.
    pub const fn secs(self) -> u64 {
        self.secs
    }

    /// The nanosecond field, verbatim — **not** range-checked.
    pub const fn nanos(self) -> u32 {
        self.nanos
    }

    /// The next representable timestamp: **+1 nanosecond, carrying into
    /// seconds**.
    ///
    /// §5.3 requires each outbound initiation to be "forced strictly
    /// greater" than the last this endpoint emitted and never names the
    /// increment. The nanosecond is not a choice: it is the only unit the
    /// 12-byte encoding has, so the smallest strictly-greater value is one
    /// nanosecond on. Anything coarser would skip representable timestamps
    /// for no reason.
    pub(crate) const fn succ(self) -> Self {
        if self.nanos >= 999_999_999 {
            Self {
                secs: self.secs.saturating_add(1),
                nanos: 0,
            }
        } else {
            Self {
                secs: self.secs,
                nanos: self.nanos + 1,
            }
        }
    }

    /// §5.2's 12 big-endian octets, as msg1's payload.
    pub(crate) fn encode(self) -> [u8; constants::MSG1_PAYLOAD_LEN] {
        Msg1Payload::new(self.secs, self.nanos).encode()
    }

    /// The inverse. Total: every 12-octet string is a timestamp (§5.3
    /// specifies no validation, so none is performed).
    pub(crate) fn decode(bytes: &[u8; constants::MSG1_PAYLOAD_LEN]) -> Self {
        let p = Msg1Payload::decode(bytes);
        Self {
            secs: p.secs,
            nanos: p.nanos,
        }
    }
}

/// What the endpoint core wants sent, and where. §16.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmit {
    /// The destination address.
    pub to: SocketAddr,
    /// The complete datagram — header ‖ Noise message ‖ mac1.
    pub data: Vec<u8>,
}

/// The completed session an [`Install`] carries. §16.4, §5.6.
pub struct EstablishedSession<C: Handshake> {
    /// The sealing half of hiss's datagram pair.
    pub seal: C::Seal,
    /// The opening half. **Stateful** — `into_datagram_with_epoch` gives a
    /// `DatagramRecv` that tracks the §7.7 epoch, so opening takes `&mut`.
    /// What it does *not* track is order: `decrypt_at` enforces neither
    /// monotonicity nor uniqueness and will open one counter repeatedly, by
    /// design, because replay rejection is the caller's duty. That makes it
    /// slither's (§7.2) — not this slice's.
    pub open: C::Open,
    /// Our session index: the value peers put in a Data header's
    /// `receiver_index` to route to us.
    pub our_index: u32,
    /// The peer's session index: what we put in a Data header.
    pub peer_index: u32,
    /// §5.6/§5.5's anchor — the msg1 source for an accepted session, the
    /// dialled address for one we completed. §7.3's amplification budget is
    /// armed on it; the budget itself is slice 7.
    pub anchor: SocketAddr,
}

/// The one endpoint→connection event (§16.4).
///
/// Carries the session and nothing else: with the rekey swap deleted (§7.6)
/// there is exactly one install per connection, so no discriminator
/// distinguishes them and none is carried.
pub struct Install<C: Handshake> {
    /// The completed session.
    pub session: EstablishedSession<C>,
}

/// The one connection→endpoint event (§16.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToEndpoint {
    /// Teardown. **A MUST**: the shell delivers this before releasing the
    /// connection's bookkeeping, or the index route and the guard-entry pin
    /// leak for the endpoint's life.
    Retired {
        /// The connection's own session index, to drop from the routing
        /// tables.
        our_index: u32,
    },
}

/// Where `handle_datagram` decided a datagram belongs. §16.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Route the datagram to this connection core next.
    ForConnection(ConnectionId),
    /// The endpoint consumed it, or dropped it. Nothing further to do.
    ///
    /// A drop is silent by §3.1 — no error, no trace, no counter — and is
    /// indistinguishable here from a datagram the endpoint handled itself,
    /// which is deliberate.
    Done,
}

/// One item of the endpoint core's drain. §16.4.
pub enum EndpointOutput<C: Handshake> {
    /// Send this datagram: msg1, a retransmit, or msg2.
    Transmit(Transmit),
    /// A stage-0 introduction is parked and awaiting the application's
    /// decision (§6.3). **0 DH has been spent on it.**
    IntroReady(IntroId, SocketAddr),
    /// Install the completed session into a `connect()`-created
    /// connection. Emitted **exactly once** per such connection, and never
    /// for one `accept()` returned (which is already established).
    ToConnection(ConnectionId, Install<C>),
    /// A dial gave up at `HANDSHAKE_GIVEUP` (§5.5). Shell-only: it never
    /// reaches a connection core, which is simply dropped.
    HandshakeFailed(ConnectionId, ConnectError),
    /// **Terminal.** The drain is empty; the next armed deadline follows,
    /// or `None` if nothing is armed.
    Timeout(Option<Instant>),
}

impl<C: Handshake> EndpointOutput<C> {
    /// Whether this is the drain sentinel.
    pub fn is_timeout(&self) -> bool {
        matches!(self, EndpointOutput::Timeout(_))
    }
}

impl<C: Handshake> std::fmt::Debug for EndpointOutput<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written because `Install` holds hiss cipher states, which
        // are not `Debug` and must not become printable.
        match self {
            EndpointOutput::Transmit(t) => f.debug_tuple("Transmit").field(t).finish(),
            EndpointOutput::IntroReady(id, src) => {
                f.debug_tuple("IntroReady").field(id).field(src).finish()
            }
            EndpointOutput::ToConnection(id, _) => f
                .debug_tuple("ToConnection")
                .field(id)
                .field(&format_args!("Install {{ .. }}"))
                .finish(),
            EndpointOutput::HandshakeFailed(id, e) => {
                f.debug_tuple("HandshakeFailed").field(id).field(e).finish()
            }
            EndpointOutput::Timeout(d) => f.debug_tuple("Timeout").field(d).finish(),
        }
    }
}

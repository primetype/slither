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

// **[RATIFIED — ruling 222]** Slice 2a carried a blanket
// `#![allow(dead_code)]` here, scoped by its own words — *"the shell that
// drives it is slice 3 … it comes off when the driver lands"*. The driver
// landed in slice 3 and it did not come off. For five slices the release
// table's *"Lints — zero warnings"* bar did not apply to the two state
// machines that hold the protocol, and it hid ruling 217's abandoned
// machinery until an audit found it by hand.
//
// It cannot simply be deleted, and the reason is structural rather than a
// concession. These cores are **sans-io**: a great many accessors exist so
// that a paused-clock test can observe state the shell has no reason to
// read — `Closing::until`, `ReplayWindow::would_accept` (which exists
// precisely *"so a test can separate the two"* rejection paths), ruling
// 94's `capacity` accounting. Under `cfg(test)` every one of them has a
// caller; without it none does. Deleting them deletes the observability
// the whole test strategy rests on.
//
// So the allow is scoped to exactly the build in which it is honest. In
// the test build — the one where every caller exists — the lint is live,
// and anything it names there is dead for real.
#![cfg_attr(not(test), allow(dead_code))]

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
pub(crate) use self::connection::{ConnEvent, ConnOutput, Connection, StreamRef, StreamsExhausted};
// §10.2's advertised windows, which [`ConnSeed`] carries from the endpoint
// that minted a connection to the connection itself (ruling 259(viii)).
pub(crate) use self::connection::flow::FlowWindows;
// §9.1's two public types. They live in the `pub(crate)` core and are
// re-exported from `lib.rs` beside `ConnectionId`/`IntroId`/`Timestamp`
// (ruling 101).
pub use self::connection::{Dir, StreamId};
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

/// What an endpoint hands a connection at birth.
///
/// **[ruling 259(viii)]** Two things, and they travel together on purpose.
/// §16.6's per-connection sub-seed is the older half; §10.2's advertised
/// receive windows are the new one. A connection is born on **two** paths —
/// `connect()`'s pending ([`Endpoint::mint_pending`]) and `accept()`'s
/// established (`endpoint::staged`'s `accept`) — and a policy that has to
/// be threaded to both call sites is a policy two call sites can disagree
/// about. Minting the pair in one place makes the agreement structural.
///
/// The endpoint's minting verb is `mint_conn_seed` — renamed from
/// `draw_sub_seed` at integration, once the partition that had kept
/// `staged.rs` out of reach lifted.
///
/// [`Endpoint::mint_pending`]: endpoint::Endpoint::mint_pending
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnSeed {
    /// §16.6's per-connection CSPRNG seed.
    pub(crate) sub_seed: [u8; 32],
    /// §10.2's two advertised receive windows.
    pub(crate) windows: FlowWindows,
}

/// A bare seed carries §10.2's **ratified** windows.
///
/// This is what makes the knob's default path unmissable rather than
/// remembered: a caller that says nothing about windows gets the
/// constants, and there is no third value the conversion could produce.
impl From<[u8; 32]> for ConnSeed {
    fn from(sub_seed: [u8; 32]) -> Self {
        Self {
            sub_seed,
            windows: FlowWindows::default(),
        }
    }
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

/// Which end of the handshake this connection turned out to be (§6.7).
///
/// **[RATIFIED 2026/08/15 — ruling 106]** Fixed at establishment and for
/// the connection's life; §9.1's stream-ID parity reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// We wrote msg1 and completed on msg2.
    Initiator,
    /// We wrote msg2 — including §6.6 step 4, where we had dialled.
    Responder,
}

/// The one endpoint→connection event (§16.4).
///
/// With the rekey swap deleted (§7.6) there is exactly one install per
/// connection, so no discriminator distinguishes them and none is carried.
pub struct Install<C: Handshake> {
    /// The completed session.
    pub session: EstablishedSession<C>,
    /// **[RATIFIED 2026/08/15 — ruling 106]** The role, which the connection
    /// core cannot derive.
    ///
    /// §6.6 step 4 admits a peer that **dialled** as the *responder*, so a
    /// core inferring "I was created by `connect()`, therefore I am the
    /// initiator" is wrong on exactly that path — and silently: both ends
    /// still agree on every stream they open themselves, and disagree only
    /// on §9.1's parity. It cannot be recovered afterwards from hiss, since
    /// ruling 89 leaves [`Handshake::Seal`](crate::packet::Handshake::Seal)
    /// an associated type with no bounds. The endpoint is the only party
    /// that knows the tie-break's outcome, so the endpoint states it.
    pub role: Role,
    /// **[RATIFIED 2026/08/16 — ruling 200]** Whether this session's anchor
    /// is a **msg1 source** — a peer-supplied address with no
    /// return-routability proof — and therefore whether §7.3's
    /// anti-amplification budget arms.
    ///
    /// Stated by the endpoint for the same reason [`role`](Self::role) is,
    /// and the same path forces it: §6.6's internal tie-break **loser
    /// dialled**, lost, and installs here as `Role::Responder` with the
    /// msg1 source as its anchor (`endpoint/routing.rs`). So neither *"I
    /// was created by `connect()`"* nor the role alone answers the
    /// question — a dialler can end up anchored at an address it did not
    /// choose, which is exactly the case §7.3 exists for.
    ///
    /// Inferring it from `Role::Responder` was tried and rejected: it is
    /// true of every *production* path but conflates the role label with
    /// the anchoring event, so a core installed as a responder without an
    /// anchoring msg1 — which is what a fixture synthesises — would be
    /// capped for a reason that never happened.
    pub anchor_from_msg1: bool,
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
    /// §6.4's LIVE branch replaced this connection (§5.4): tear it down with
    /// [`ConnectionLost::Replaced`](crate::error::ConnectionLost::Replaced).
    ///
    /// # Why the endpoint core cannot do this itself
    ///
    /// §6.4 requires the replacing `accept()` to fire the teardown *"as the
    /// act that installs the replacement"*, and §7.5 requires the refusal
    /// against a `None` basis to mark that connection contested — both on a
    /// **connection** the endpoint core cannot reach. §16.4's core API has
    /// no such channel: `ToConnection` carries `Install` *"only"*, and the
    /// connection's verb list has nothing a replacement or a mark could
    /// arrive through. So these two travel as shell-only outputs, on
    /// [`HandshakeFailed`](EndpointOutput::HandshakeFailed)'s terms exactly,
    /// and the shell hands each to the named connection core.
    ///
    /// **This is a crate-internal seam, not a wire or an API change**, and
    /// the gap is reported rather than resolved by it: see
    /// `.slices/07-mobility/`'s implementation report.
    Replaced(ConnectionId),
    /// §6.4 refused an admitted candidate against a `None` basis: mark this
    /// connection **contested** (ruling 36, §7.5).
    ///
    /// Shell-only, for [`Replaced`](EndpointOutput::Replaced)'s reason. The
    /// core it names decides whether the mark is taken at all: ruling 179
    /// makes it a no-op on a closing or draining connection, and only the
    /// connection knows its lifecycle.
    Contested(ConnectionId),
    /// **Terminal.** The drain is empty; the next armed deadline follows,
    /// or `None` if nothing is armed.
    Timeout(Option<Instant>),
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
            EndpointOutput::Replaced(id) => f.debug_tuple("Replaced").field(id).finish(),
            EndpointOutput::Contested(id) => f.debug_tuple("Contested").field(id).finish(),
            EndpointOutput::Timeout(d) => f.debug_tuple("Timeout").field(d).finish(),
        }
    }
}

//! The connection core — **the minimum §16.4's signatures force**.
//!
//! `core::Endpoint::connect()` and `::accept()` both return
//! `(ConnectionId, core::Connection)`, so this type has to exist before
//! anything it does exists. Slice 2a gives it exactly three things:
//!
//! * the session — `Some` for a connection [`accept()`] returned (already
//!   established), `None` for one `connect()` created and awaiting its
//!   [`Install`];
//! * [`handle_endpoint_event`](Connection::handle_endpoint_event), which
//!   installs **exactly once** and emits [`ConnEvent::Established`];
//! * the §16.6 sub-seed, drawn by the endpoint at connection creation
//!   **even though nothing here uses it yet** — see below.
//!
//! Every §7–§15 verb — `write`, `read`, `send_message`, `send_datagram`,
//! `close`, the claim verbs, the timer table, the replay window, roaming,
//! the ratchet — arrives in slices 3–6. **This is a boundary, not a
//! regression**, and it is stated here so a reviewer does not read the
//! absence as one.
//!
//! # Why the sub-seed is stored now
//!
//! §16.6: *"At connection creation the endpoint draws a 32-byte sub-seed
//! for the connection core (drawn even while unused, so later
//! connection-side randomness cannot perturb the endpoint's draw order)."*
//! The parenthesis is written for exactly this situation. Omitting the draw
//! now and adding it in slice 4 would silently shift every seeded test's
//! index and jitter sequence.
//!
//! [`accept()`]: super::Endpoint::accept
//! [`Install`]: super::Install

use std::collections::VecDeque;
use std::time::Instant;

use crate::packet::Handshake;

use super::{EstablishedSession, Install, ToEndpoint, Transmit};

/// A connection's core state machine. §16.4.
pub(crate) struct Connection<C: Handshake> {
    session: Option<EstablishedSession<C>>,
    sub_seed: [u8; 32],
    outputs: VecDeque<ConnOutput>,
    installed: bool,
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
        }
    }

    /// A connection `accept()` returned: established on arrival, and
    /// **never** followed by an `Install` (§16.4).
    pub(crate) fn established(sub_seed: [u8; 32], session: EstablishedSession<C>) -> Self {
        let mut outputs = VecDeque::new();
        outputs.push_back(ConnOutput::Event(ConnEvent::Established));
        Self {
            session: Some(session),
            sub_seed,
            outputs,
            installed: true,
        }
    }

    /// The §16.6 sub-seed. Unused until slice 4; see the module docs.
    pub(crate) fn sub_seed(&self) -> &[u8; 32] {
        &self.sub_seed
    }

    /// Whether a session is installed.
    pub(crate) fn is_established(&self) -> bool {
        self.session.is_some()
    }

    /// The installed session, if any.
    pub(crate) fn session(&self) -> Option<&EstablishedSession<C>> {
        self.session.as_ref()
    }

    /// §16.4's endpoint→connection event. `Install` only, **exactly
    /// once**: a second one is a driver bug and is ignored rather than
    /// replacing a live session.
    pub(crate) fn handle_endpoint_event(&mut self, _now: Instant, ev: Install<C>) {
        if self.installed {
            debug_assert!(false, "Install is delivered exactly once per connection");
            return;
        }
        self.installed = true;
        self.session = Some(ev.session);
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Established));
    }

    /// §16.4's drain. Terminates in [`ConnOutput::Timeout`].
    ///
    /// Slice 2a arms no connection timer — every one of §16.5's connection
    /// deadlines is §7–§13's — so the terminal value is always
    /// `Timeout(None)`.
    pub(crate) fn poll_output(&mut self) -> ConnOutput {
        self.outputs
            .pop_front()
            .unwrap_or(ConnOutput::Timeout(None))
    }
}

/// One item of the connection core's drain. §16.4.
///
/// The variants §16.4 lists that slice 2a cannot yet construct — every
/// `ConnEvent` but `Established` — are absent rather than stubbed: an
/// uninhabited variant is a claim about the protocol, and these will each
/// arrive with the section that defines them.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnEvent {
    /// The session is installed; the shell resolves `Connecting`.
    Established,
}

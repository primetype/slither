//! §17.2–17.4 — the endpoint's three lookup structures.
//!
//! * [`IndexTables`] (§17.3) — `index → connection` for live sessions and
//!   for in-flight initiations, plus the minting rule that keeps the two
//!   disjoint.
//! * [`StaticMap`] (§17.4) — `static → connection`, §16.1's
//!   one-session-per-peer invariant made concrete, carrying each entry's
//!   `replacement_basis`.
//! * the **hint set** (§17.4) — not a structure at all: it *is* "the
//!   pending tables' dialled addresses", so it is a projection
//!   ([`StaticMap::hints`]) rather than a fourth map that could drift out
//!   of step with the pendings it is supposed to mirror.
//!
//! # `replacement_basis` is written here and read in slice 7
//!
//! §6.4's admission is its **only** reader, and §6.4 is slice 7. Writing it
//! wrong now is undetectable until then and would look like a slice-7 bug,
//! which is why it is a field on the entry rather than something
//! reconstructed later.

use std::collections::HashMap;
use std::net::SocketAddr;

use rand_chacha::ChaCha20Rng;
use rand_core::Rng;

use crate::core::{ConnectionId, Timestamp};

/// §17.3's two index tables.
///
/// Two maps rather than one with a tag, because the **minting rule is
/// stated over both at once**: a value is redrawn while present in
/// *either*. §17.3 says why — "a pending's msg1 index graduates into the
/// session index on completion; this closes the route-stealing collision".
#[derive(Debug, Default)]
pub(crate) struct IndexTables {
    sessions: HashMap<u32, ConnectionId>,
    pendings: HashMap<u32, ConnectionId>,
}

impl IndexTables {
    /// Draw a random **nonzero** `u32` absent from both tables.
    ///
    /// Drawn from the endpoint RNG (§16.6), which is what makes indices
    /// off-path-unpredictable — load-bearing for §5.5's on-path-only
    /// completion spend.
    pub(crate) fn mint(&self, rng: &mut ChaCha20Rng) -> u32 {
        loop {
            let candidate = rng.next_u32();
            if candidate != 0
                && !self.sessions.contains_key(&candidate)
                && !self.pendings.contains_key(&candidate)
            {
                return candidate;
            }
        }
    }

    /// Route an in-flight initiation's index.
    pub(crate) fn insert_pending(&mut self, index: u32, conn: ConnectionId) {
        self.pendings.insert(index, conn);
    }

    /// Route a live session's index.
    pub(crate) fn insert_session(&mut self, index: u32, conn: ConnectionId) {
        self.sessions.insert(index, conn);
    }

    /// The connection an in-flight initiation's index belongs to.
    pub(crate) fn pending(&self, index: u32) -> Option<ConnectionId> {
        self.pendings.get(&index).copied()
    }

    /// The connection a live session's index belongs to.
    pub(crate) fn session(&self, index: u32) -> Option<ConnectionId> {
        self.sessions.get(&index).copied()
    }

    /// Drop a pending route (a superseded attempt, a give-up, a cancel).
    pub(crate) fn remove_pending(&mut self, index: u32) {
        self.pendings.remove(&index);
    }

    /// Drop a session route (§16.4's `Retired`).
    pub(crate) fn remove_session(&mut self, index: u32) {
        self.sessions.remove(&index);
    }
}

/// Whether a static's connection is established or still dialling. §5.4's
/// three-valued responder state is this plus absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticState {
    /// An in-flight outbound initiation and no established connection.
    Pending,
    /// An established connection.
    Live,
}

/// One `static → connection` row. §17.4.
#[derive(Debug, Clone)]
pub(crate) struct StaticEntry {
    /// The connection this static belongs to.
    pub(crate) conn: ConnectionId,
    /// §5.4's LIVE/PENDING distinction.
    pub(crate) state: StaticState,
    /// The address dialled, for a connection we initiated. `None` once
    /// established by `accept()`, because §17.4 is explicit that
    /// established connections contribute **no** hints.
    pub(crate) dialled: Option<SocketAddr>,
    /// §17.4's replacement basis: `Some(t)` when **we responded**, `None`
    /// when **we dialled**. Written once at install and never updated;
    /// dies with the connection. Slice 2a writes it and never reads it —
    /// §6.4 is its only reader.
    pub(crate) replacement_basis: Option<Timestamp>,
}

/// §17.4's `static → connection` map. §16.1's one-session-per-peer
/// invariant, made concrete.
#[derive(Debug, Default)]
pub(crate) struct StaticMap {
    entries: HashMap<Vec<u8>, StaticEntry>,
}

impl StaticMap {
    /// The row for a static's canonical §2.4 octets.
    pub(crate) fn get(&self, key: &[u8]) -> Option<&StaticEntry> {
        self.entries.get(key)
    }

    /// Claim a static for a connection. The caller has already established
    /// that no row exists.
    pub(crate) fn insert(&mut self, key: Vec<u8>, entry: StaticEntry) {
        debug_assert!(
            !self.entries.contains_key(&key),
            "§16.1: one session per peer static"
        );
        self.entries.insert(key, entry);
    }

    /// Promote a dialled static from PENDING to LIVE on completion.
    ///
    /// The dialled address is dropped at the same moment, which is what
    /// keeps §17.4's "established connections contribute no hints" true by
    /// construction rather than by remembering to filter.
    pub(crate) fn promote(&mut self, key: &[u8]) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.state = StaticState::Live;
            entry.dialled = None;
        }
    }

    /// Release a static (give-up, cancel, teardown).
    pub(crate) fn remove(&mut self, key: &[u8]) -> Option<StaticEntry> {
        self.entries.remove(key)
    }

    /// Release whatever static a connection holds.
    pub(crate) fn remove_by_connection(&mut self, conn: ConnectionId) -> Option<Vec<u8>> {
        let key = self
            .entries
            .iter()
            .find(|(_, e)| e.conn == conn)
            .map(|(k, _)| k.clone())?;
        self.entries.remove(&key);
        Some(key)
    }

    /// §6.5's hint set: **the pending tables' dialled addresses alone**.
    ///
    /// A projection, not a stored set. §17.4 defines the hint set as being
    /// those addresses, so deriving it is not an optimisation — it is the
    /// definition, and it makes "established connections contribute no
    /// hints" unfalsifiable rather than merely tested. Consultation is
    /// §6.5, slice 7.
    pub(crate) fn hints(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.entries.values().filter_map(|e| e.dialled)
    }
}

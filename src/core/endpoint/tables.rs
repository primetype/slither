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
//! # `replacement_basis` is written here and read by §6.4
//!
//! §6.4's admission is its **only** reader. Its proven-LIVE replacement
//! admission is still a later slice; §6.4's **PENDING** branch and §6.6's
//! internal tie-break landed with ruling 91, and the latter is why
//! [`StaticMap::promote`] takes the basis as an argument rather than
//! leaving whatever the dialled row was born with.

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
    /// dies with the connection. §6.4's admission is its only reader.
    ///
    /// A `Pending` row carries `None` because §17.4's "written once at
    /// install" has not happened yet: [`promote`](StaticMap::promote) is
    /// the install on a dialled row, and it writes `None` for a msg2
    /// completion and `Some(t)` for §6.6 step 4's admission.
    pub(crate) replacement_basis: Option<Timestamp>,
    /// §17.1's `HANDSHAKE_GIVEUP` extension, armed but not yet dated.
    ///
    /// `true` once a **tie-break write** has landed on this static's guard
    /// entry — §6.6 step 4's admission or §6.7's winner-side record, by
    /// either of §6.6's two routes. §17.1 dates the extension from "after
    /// the connection it belongs to dies", which is the instant this row's
    /// §17.1 pin is released, so the flag waits here and the endpoint
    /// turns it into `exempt_until` at that release.
    ///
    /// It lives on the row rather than in the guard because the guard has
    /// no notion of *whose* connection an entry belongs to, and the
    /// release instant is exactly what the row's death supplies.
    pub(crate) guard_exempt: bool,
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

    /// Promote a dialled static from PENDING to LIVE, writing §17.4's
    /// `replacement_basis` as it goes.
    ///
    /// The dialled address is dropped at the same moment, which is what
    /// keeps §17.4's "established connections contribute no hints" true by
    /// construction rather than by remembering to filter.
    ///
    /// # The basis is a parameter because the two callers disagree
    ///
    /// §17.4 lists three responder cases and two initiator ones, and a
    /// dialled row can end at either: a msg2 completion leaves `None` ("we
    /// dialled … neither teaches us any timestamp of the peer's, because
    /// msg2 carries no payload"), while §6.6 step 4's tie-break admission
    /// leaves `Some(t)` — we dialled, lost, and installed as the
    /// **responder**. Making the caller say which is what stops the second
    /// case from silently inheriting the first's answer, which is the
    /// value §6.4 would then read.
    pub(crate) fn promote(&mut self, key: &[u8], replacement_basis: Option<Timestamp>) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.state = StaticState::Live;
            entry.dialled = None;
            entry.replacement_basis = replacement_basis;
        }
    }

    /// Arm §17.1's `HANDSHAKE_GIVEUP` extension on this static's row.
    ///
    /// Idempotent, and deliberately: a winner-side record can land more
    /// than once against one dial — the loser retransmits every ~5 s — and
    /// the flag is a fact about the guard entry, not a count.
    pub(crate) fn arm_guard_exemption(&mut self, key: &[u8]) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.guard_exempt = true;
        }
    }

    /// Release a static (give-up, cancel, teardown).
    pub(crate) fn remove(&mut self, key: &[u8]) -> Option<StaticEntry> {
        self.entries.remove(key)
    }

    /// Release whatever static a connection holds, with the row.
    ///
    /// The row comes back because its
    /// [`guard_exempt`](StaticEntry::guard_exempt) flag is read at exactly
    /// this moment — the release of the §17.1 pin — and reading it after
    /// the removal would mean reading it from nowhere.
    pub(crate) fn remove_by_connection(
        &mut self,
        conn: ConnectionId,
    ) -> Option<(Vec<u8>, StaticEntry)> {
        let key = self
            .entries
            .iter()
            .find(|(_, e)| e.conn == conn)
            .map(|(k, _)| k.clone())?;
        let entry = self.entries.remove(&key)?;
        Some((key, entry))
    }

    /// §6.5's hint set: **the pending tables' dialled addresses alone**.
    ///
    /// A projection, not a stored set. §17.4 defines the hint set as being
    /// those addresses, so deriving it is not an optimisation — it is the
    /// definition, and it makes "established connections contribute no
    /// hints" unfalsifiable rather than merely tested. Consulted by §6.5
    /// step 2, in `routing.rs`.
    pub(crate) fn hints(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.entries.values().filter_map(|e| e.dialled)
    }
}

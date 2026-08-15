//! §17.1 — the per-remote-static greatest-initiation-timestamp guard.
//!
//! One entry per peer static, holding the greatest initiation timestamp
//! (§5.3) we have ever **recorded** for it. An initiation strictly greater
//! than the entry passes; anything else is a replay. Where we have recorded
//! nothing the check passes vacuously.
//!
//! # Every write site is post-`ss`
//!
//! §17.1 has four write sites and all four sit **after** the proving `ss`,
//! so **only a key-holder can write a guard entry**. Slice 2a implements
//! one of them — `authenticate()` on the staged path. The other three
//! (§6.4's re-homed admission, §6.6's tie-break admit, §6.7's winner-side
//! record) are slice 7, and [`GuardEntry::exempt_until`] exists from the
//! start so slice 7 adds a call rather than a migration.
//!
//! **The initiator path never touches this.** §17.1: for a static we only
//! ever dialled "we hold no entry at all — every §17.1 write site is a
//! post-`ss` read of an *inbound* msg1, and a `connect()` completed by msg2
//! writes nothing".
//!
//! # The write is provisional until `accept()`
//!
//! Mitigation **(i) no-orphan-on-reject**: *"a static authenticated and
//! then rejected without ever being accepted writes no orphan (its record
//! drops with the chain; a pre-existing entry reverts)"*, and the revert
//! applies "whether the rejection is a dropped chain or an `accept()` that
//! returns `AcceptError::Stale`". So `authenticate()` hands the chain a
//! [`GuardUndo`], and every path that ends the chain without accepting runs
//! it. This is the single most consequential line in the guard: the failure
//! is silent, permanent, and blocks a real peer's next genuine initiation.
//!
//! # Pinning
//!
//! An entry is pinned — never evicted, never aged — while a live
//! connection, an in-flight outbound pending, **or a staged mid-state**
//! exists for its static. A mid-state exists from `read_identity()`, when
//! the static is still merely *claimed*, which §17.1 addresses directly: a
//! pin **never creates an entry**, it only "flips a bit on an entry a
//! key-holder already wrote". So [`TimestampGuard::pin`] on an absent key
//! is a no-op, not an insert — that is what keeps the bounded exception to
//! §6.1's nothing-durable rule bounded.
//!
//! `pins` is a **count**, not a flag: a static can hold a mid-state and an
//! outbound pending at once (that is §5.4's PENDING row), and a flag would
//! lose the second pin on the first release, unpinning a live entry.

use std::collections::HashMap;
use std::time::Instant;

use crate::constants;
use crate::core::Timestamp;

/// One static's guard state.
#[derive(Debug, Clone)]
pub(crate) struct GuardEntry {
    /// The greatest initiation timestamp recorded for this static.
    pub(crate) greatest: Timestamp,
    /// Live connections + in-flight pendings + staged mid-states.
    pub(crate) pins: u32,
    /// §6.6/§6.7's `HANDSHAKE_GIVEUP` extension past the connection's
    /// death. Unreachable in slice 2a — both write sites that set it are
    /// slice 7 — and present so that slice 7 does not have to reshape the
    /// entry.
    pub(crate) exempt_until: Option<Instant>,
    /// LRU recency. **Admission only** (mitigation (iii)): it refreshes on
    /// a successful post-`ss` record and never on a failed check, which is
    /// what keeps the write path key-holder-only.
    pub(crate) last_admitted: Instant,
}

impl GuardEntry {
    fn pinned(&self, now: Instant) -> bool {
        self.pins > 0 || self.exempt_until.is_some_and(|until| until > now)
    }

    /// When this entry becomes eligible for aging out, or `None` while a
    /// live pin holds it.
    ///
    /// Computed without a `now`, so §16.5's min-deadline scan needs no
    /// clock: a `HANDSHAKE_GIVEUP` exemption (§6.6/§6.7, slice 7) simply
    /// pushes the instant out rather than being tested against the present.
    fn age_deadline(&self) -> Option<Instant> {
        if self.pins > 0 {
            return None;
        }
        let base = self.last_admitted + constants::TS_GUARD_ORPHAN_TTL;
        Some(match self.exempt_until {
            Some(until) if until > base => until,
            _ => base,
        })
    }
}

/// What `authenticate()` must be able to undo. §17.1 mitigation (i).
///
/// `previous` is `None` when no entry existed before the write, in which
/// case the revert removes the entry outright.
#[derive(Debug, Clone)]
pub(crate) struct GuardUndo {
    key: Vec<u8>,
    previous: Option<(Timestamp, Instant)>,
}

/// §17.1's guard.
#[derive(Debug, Default)]
pub(crate) struct TimestampGuard {
    entries: HashMap<Vec<u8>, GuardEntry>,
}

impl TimestampGuard {
    /// The recorded greatest timestamp for a static, if any.
    pub(crate) fn greatest(&self, key: &[u8]) -> Option<Timestamp> {
        self.entries.get(key).map(|e| e.greatest)
    }

    /// Whether `candidate` would pass — **strictly** greater, vacuously
    /// true where nothing is recorded.
    pub(crate) fn admits(&self, key: &[u8], candidate: Timestamp) -> bool {
        match self.entries.get(key) {
            Some(entry) => candidate > entry.greatest,
            None => true,
        }
    }

    /// Record a candidate that [`admits`](Self::admits) accepted, returning
    /// the undo the chain must carry until `accept()` makes it permanent.
    ///
    /// Refreshes LRU recency, because this is a *successful* record.
    pub(crate) fn record(&mut self, key: &[u8], candidate: Timestamp, now: Instant) -> GuardUndo {
        let previous = self.entries.get(key).map(|e| (e.greatest, e.last_admitted));
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.greatest = candidate;
                entry.last_admitted = now;
            }
            None => {
                self.entries.insert(
                    key.to_vec(),
                    GuardEntry {
                        greatest: candidate,
                        pins: 0,
                        exempt_until: None,
                        last_admitted: now,
                    },
                );
            }
        }
        self.evict_if_over_cap(now);
        GuardUndo {
            key: key.to_vec(),
            previous,
        }
    }

    /// Undo a provisional record. §17.1 mitigation (i).
    ///
    /// A pre-existing entry is restored — **greatest and recency both**,
    /// since the recency refresh was part of the record being undone. An
    /// entry the record created is removed, unless something has pinned it
    /// meanwhile, in which case only the value is rolled back to nothing
    /// recordable and the entry is left for its pin to release.
    pub(crate) fn revert(&mut self, undo: GuardUndo) {
        match undo.previous {
            Some((greatest, last_admitted)) => {
                if let Some(entry) = self.entries.get_mut(&undo.key) {
                    entry.greatest = greatest;
                    entry.last_admitted = last_admitted;
                }
            }
            None => {
                let still_pinned = self
                    .entries
                    .get(&undo.key)
                    .is_some_and(|entry| entry.pins > 0);
                if !still_pinned {
                    self.entries.remove(&undo.key);
                }
            }
        }
    }

    /// Take a pin. **Never creates an entry** (§17.1).
    pub(crate) fn pin(&mut self, key: &[u8]) -> bool {
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.pins = entry.pins.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// Release a pin taken by [`pin`](Self::pin).
    pub(crate) fn unpin(&mut self, key: &[u8]) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.pins = entry.pins.saturating_sub(1);
        }
    }

    /// Mitigation (ii): age unpinned orphans out at
    /// [`TS_GUARD_ORPHAN_TTL`](constants::TS_GUARD_ORPHAN_TTL).
    ///
    /// The constant is ruling 70's — an alias of `INTRO_TTL`, so there is
    /// no second place for 15 s to be written down.
    pub(crate) fn age_orphans(&mut self, now: Instant) {
        self.entries.retain(|_, entry| {
            entry.pinned(now) || entry.last_admitted + constants::TS_GUARD_ORPHAN_TTL > now
        });
    }

    /// When the next orphan ages out, if any is unpinned. §16.5's third
    /// deadline family.
    pub(crate) fn next_orphan_deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(GuardEntry::age_deadline)
            .min()
    }

    /// Bounded LRU over the **unpinned** tier only.
    ///
    /// A linear scan, not a heap: the tier is capped at
    /// `TS_GUARD_ORPHAN_CAP` (1024) and eviction runs at most once per
    /// successful admission, which is a key-holder-gated event.
    fn evict_if_over_cap(&mut self, now: Instant) {
        while self
            .entries
            .values()
            .filter(|entry| !entry.pinned(now))
            .count()
            > constants::TS_GUARD_ORPHAN_CAP
        {
            let victim = self
                .entries
                .iter()
                .filter(|(_, entry)| !entry.pinned(now))
                .min_by(|a, b| {
                    a.1.last_admitted
                        .cmp(&b.1.last_admitted)
                        .then_with(|| a.0.cmp(b.0))
                })
                .map(|(key, _)| key.clone());
            match victim {
                Some(key) => {
                    self.entries.remove(&key);
                }
                None => break,
            }
        }
    }

    /// How many entries are held. Diagnostics only.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

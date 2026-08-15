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
//!
//! # The orphan clock starts at release (**ruling 73**)
//!
//! An entry ages from the instant its **last pin is released**, never from
//! its last admission: §17.1 defines an orphan as a *dead-connection*
//! entry, and an entry cannot age as an orphan before it is one. The two
//! clocks are kept apart on the entry — [`GuardEntry::last_admitted`] is
//! mitigation (iii)'s LRU recency and stays admission-only,
//! [`GuardEntry::orphaned_at`] is the aging clock — because collapsing them
//! deletes the guard for the longest-lived connections at exactly the
//! moment a captured initiation becomes replayable.

use std::collections::HashMap;
use std::time::Instant;

use crate::constants;
use crate::core::Timestamp;

/// One static's guard state.
#[derive(Debug, Clone)]
pub(crate) struct GuardEntry {
    /// The greatest initiation timestamp recorded for this static.
    ///
    /// `None` means **an entry that exists only to hold a pin**: its record
    /// was reverted by mitigation (i) while something else still pinned it,
    /// so the entry cannot be removed but must not keep the value. §17.1
    /// says the record "drops with the chain", not merely that an unpinned
    /// entry is deleted — so this case has to be representable, or a
    /// second pin turns authenticate-then-drop back into an orphan mint.
    pub(crate) greatest: Option<Timestamp>,
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
    ///
    /// **[RATIFIED 2026/08/15 — ruling 73]** This is the *recency* key and
    /// nothing else. It used to double as the aging clock; it no longer
    /// does. Mitigation (iii) is untouched — only
    /// [`orphaned_at`](GuardEntry::orphaned_at) moved.
    pub(crate) last_admitted: Instant,
    /// When this entry became an **orphan** — the instant its last pin was
    /// released. `None` while pinned. **[RATIFIED 2026/08/15 — ruling 73]**
    ///
    /// §17.1 defines an orphan as a *dead-connection* entry, so an entry
    /// cannot age **as an orphan** before it is one. Aging from
    /// [`last_admitted`](GuardEntry::last_admitted) instead deleted the
    /// guard for exactly the connections that held it longest: a session
    /// outliving `TS_GUARD_ORPHAN_TTL` carries `last_admitted` frozen at
    /// accept time, so at retirement the entry is *already* past its
    /// deadline and dies at the next sweep with **no orphan window at
    /// all** — precisely when a captured initiation becomes replayable.
    ///
    /// # The core cannot always know the release instant
    ///
    /// §16.4 ratifies `handle_connection_event` and `reject` **without** a
    /// `now`, and both release pins — the first being *the* retirement path
    /// ruling 73 is about. So [`unpin`](TimestampGuard::unpin) stamps with
    /// the last instant the core was *told*, and marks the stamp
    /// [provisional](GuardEntry::provisional_stamp).
    pub(crate) orphaned_at: Option<Instant>,
    /// Whether [`orphaned_at`](GuardEntry::orphaned_at) is a **lower
    /// bound** on the release instant rather than the release instant
    /// itself. **Ruling 73.**
    ///
    /// The true release instant `T` is bracketed by the last instant the
    /// core was told and the next one it will be told. Taking the lower end
    /// keeps the deadline announceable the moment the pin drops — §16.5
    /// names orphan aging as one of the endpoint's three deadline families,
    /// and a family that goes silent between two calls is not that.
    ///
    /// But the lower end alone re-creates ruling 73's own defect one level
    /// down: a connection dying of `DEAD_TIMEOUT` was last heard from 25 s
    /// ago, and 25 s stale plus a 15 s TTL is **already in the past**, so
    /// the entry would again die at the next sweep with no orphan window at
    /// all. So [`observe`](TimestampGuard::observe) applies a floor at the
    /// **first** observation after the release: a stamp that has already
    /// expired by then is re-stamped to that instant. An orphan therefore
    /// always gets a full `TS_GUARD_ORPHAN_TTL` from the first moment the
    /// core can see that it *is* one, which is exactly the property ruling
    /// 73 exists to guarantee.
    ///
    /// The floor applies once, not at every observation — otherwise no
    /// orphan would ever age out.
    provisional_stamp: bool,
}

impl GuardEntry {
    fn pinned(&self, now: Instant) -> bool {
        self.pins > 0 || self.exempt_until.is_some_and(|until| until > now)
    }

    /// When this entry becomes eligible for aging out, or `None` while a
    /// live pin holds it.
    ///
    /// The `?` on `orphaned_at` is a belt-and-braces `None`: an unpinned
    /// entry always carries a stamp. Answering `None` — never swept, never
    /// announced — is the safe direction if that invariant is ever broken,
    /// where a fabricated instant would not be.
    ///
    /// Computed without a `now`, so §16.5's min-deadline scan needs no
    /// clock: a `HANDSHAKE_GIVEUP` exemption (§6.6/§6.7, slice 7) simply
    /// pushes the instant out rather than being tested against the present.
    ///
    /// **The clock runs from `orphaned_at`, not `last_admitted`** (ruling
    /// 73). This is the single source of truth for the aging decision —
    /// [`age_orphans`](TimestampGuard::age_orphans) sweeps by it too, so the
    /// announced deadline and the sweep that honours it cannot drift.
    fn age_deadline(&self) -> Option<Instant> {
        if self.pins > 0 {
            return None;
        }
        let base = self.orphaned_at? + constants::TS_GUARD_ORPHAN_TTL;
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
    /// Outer `None` = no entry existed before the record. Inner `None` =
    /// an entry existed holding no record (a pin-only entry).
    previous: Option<(Option<Timestamp>, Instant)>,
}

/// §17.1's guard.
#[derive(Debug, Default)]
pub(crate) struct TimestampGuard {
    entries: HashMap<Vec<u8>, GuardEntry>,
    /// Set when [`unpin`](TimestampGuard::unpin) leaves a provisional stamp
    /// behind (ruling 73 — see [`GuardEntry::provisional_stamp`]). Cleared
    /// by the pass that finalises them.
    ///
    /// A plain `bool`, so [`observe`](TimestampGuard::observe) is one
    /// branch on the flood path and scans only in the interval between a
    /// release and the next clocked call. A stale `true` costs one wasted
    /// scan and nothing else, so no path has to decrement it.
    has_provisional: bool,
}

impl TimestampGuard {
    /// The recorded greatest timestamp for a static, if any.
    pub(crate) fn greatest(&self, key: &[u8]) -> Option<Timestamp> {
        self.entries.get(key).and_then(|e| e.greatest)
    }

    /// The live pin count for a static: connections + pendings +
    /// mid-states. `0` for an absent entry.
    pub(crate) fn pins(&self, key: &[u8]) -> u32 {
        self.entries.get(key).map_or(0, |e| e.pins)
    }

    /// Whether `candidate` would pass — **strictly** greater, vacuously
    /// true where nothing is recorded.
    pub(crate) fn admits(&self, key: &[u8], candidate: Timestamp) -> bool {
        match self.entries.get(key) {
            Some(entry) => entry.greatest.is_none_or(|g| candidate > g),
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
                entry.greatest = Some(candidate);
                entry.last_admitted = now;
            }
            None => {
                self.entries.insert(
                    key.to_vec(),
                    GuardEntry {
                        greatest: Some(candidate),
                        pins: 0,
                        exempt_until: None,
                        last_admitted: now,
                        // Born unpinned, so it is an orphan from this
                        // instant — and `now` is in hand, so the stamp is
                        // exact rather than provisional (ruling 73).
                        orphaned_at: Some(now),
                        provisional_stamp: false,
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
                if still_pinned {
                    // The entry cannot be removed — something else pins it —
                    // but §17.1's "its record drops with the chain" is about
                    // the RECORD, so the value goes even though the entry
                    // stays. Without this, a second pin (another chain for
                    // the same static, or a concurrent dial) makes the
                    // authenticate-then-drop flood mint orphans after all,
                    // which is the exact attack mitigation (i) forbids.
                    if let Some(entry) = self.entries.get_mut(&undo.key) {
                        entry.greatest = None;
                    }
                } else {
                    self.entries.remove(&undo.key);
                }
            }
        }
    }

    /// Take a pin. **Never creates an entry** (§17.1).
    ///
    /// Taking a pin **stops the orphan clock** (ruling 73): a pinned entry
    /// is not an orphan, so it has no aging deadline at all until its last
    /// pin goes again.
    pub(crate) fn pin(&mut self, key: &[u8]) -> bool {
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.pins = entry.pins.saturating_add(1);
                entry.orphaned_at = None;
                entry.provisional_stamp = false;
                true
            }
            None => false,
        }
    }

    /// Release a pin taken by [`pin`](Self::pin).
    ///
    /// **Ruling 73: this is where the orphan clock starts.** `last_now` is
    /// the last instant the core was *told* — §16.4 ratifies
    /// `handle_connection_event` and `reject` with no `now` and both reach
    /// this verb, so it is a lower bound rather than the release instant,
    /// and the stamp is marked provisional until
    /// [`observe`](Self::observe) can floor it. See
    /// [`GuardEntry::provisional_stamp`].
    pub(crate) fn unpin(&mut self, key: &[u8], last_now: Instant) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        entry.pins = entry.pins.saturating_sub(1);
        // A pin-only entry (record already reverted) has nothing left to
        // protect once its last pin goes, and leaving it would be an orphan
        // with no record — pure bookkeeping the LRU would then have to age.
        if entry.pins == 0 && entry.greatest.is_none() && entry.exempt_until.is_none() {
            self.entries.remove(key);
            return;
        }
        if entry.pins == 0 {
            entry.orphaned_at = Some(last_now);
            entry.provisional_stamp = true;
            self.has_provisional = true;
        }
    }

    /// Finalise every provisional orphan stamp against the first instant
    /// the core is told about after the release. **Ruling 73.**
    ///
    /// Called from each clocked entry point of the core. A provisional
    /// stamp that has *already* expired by now is floored to `now`, so an
    /// orphan can never be swept in the same breath as it is first seen to
    /// be one — which is ruling 73's whole point. A stamp that has not
    /// expired is left where it is and simply becomes final.
    pub(crate) fn observe(&mut self, now: Instant) {
        if !self.has_provisional {
            return;
        }
        for entry in self.entries.values_mut() {
            if !entry.provisional_stamp {
                continue;
            }
            let expired = entry
                .orphaned_at
                .is_none_or(|at| at + constants::TS_GUARD_ORPHAN_TTL <= now);
            if expired {
                entry.orphaned_at = Some(now);
            }
            entry.provisional_stamp = false;
        }
        self.has_provisional = false;
    }

    /// Mitigation (ii): age unpinned orphans out at
    /// [`TS_GUARD_ORPHAN_TTL`](constants::TS_GUARD_ORPHAN_TTL).
    ///
    /// The constant is ruling 70's — an alias of `INTRO_TTL`, so there is
    /// no second place for 15 s to be written down.
    ///
    /// Finalises provisional stamps first, then sweeps by
    /// [`age_deadline`](GuardEntry::age_deadline) — the same function
    /// §16.5's min-deadline scan announces, so the two cannot disagree.
    /// Finalising first is also what makes ruling 76's order benign: a
    /// give-up at step (1) releases a pin, and the aging at step (3) floors
    /// its stamp and grants it a **fresh** window rather than finding an
    /// expired one.
    pub(crate) fn age_orphans(&mut self, now: Instant) {
        self.observe(now);
        self.entries
            .retain(|_, entry| entry.age_deadline().is_none_or(|deadline| deadline > now));
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

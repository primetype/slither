//! §6.3 — the stage-0 introduction queue, entire.
//!
//! | Constant | Value | Scope |
//! |---|---|---|
//! | `INTRO_QUEUE_CAP` | 1024 slots | endpoint-wide, configurable |
//! | `INTRO_MAX_PER_SOURCE` | 4 chains | per source **IP** (per **/64** for IPv6), configurable |
//! | `INTRO_TTL` | 15 s | from the entry's **last refresh** |
//!
//! # Two keys, two scopes, on purpose
//!
//! The **dedup** key is the full [`SocketAddr`]; the **cap** key is the IP
//! (a /64 for IPv6). They differ deliberately: distinct initiators behind
//! one NAT present distinct ports, so they dedup separately while sharing
//! one cap, which is the stated intent. A same-4-tuple collision is a
//! rebind of the same flow, where newest-wins is correct.
//!
//! # The one structure keyed on an unauthenticated quantity
//!
//! §6.1 forbids anything **durable** keyed on the source address, the
//! claimed static, or `sender_index`. [`IntroQueue::by_addr`] is keyed on
//! the source address, and is admissible precisely because §6.3 mandates
//! the queue and because it is **bounded and TTL'd** — 1024 entries, 15 s.
//! That distinction is preserved by construction: this is the *only* map,
//! counter or trace in the endpoint keyed on any of the three, and a review
//! criterion for this slice is that no second one appears.
//!
//! # Age is measured from the last refresh, never from the original park
//!
//! **[Ruling 69.]** §6.3 originally read `INTRO_TTL` "from the entry's last
//! refresh" while evicting "the oldest unconsumed entry (by park time)" —
//! two clocks over one queue, and under park-time ordering the guarantee
//! inverted exactly for the party it names: a genuine peer retransmitting
//! for 14 s carries the *oldest* park time and is evicted first, while
//! every attacker's freshly-parked entry outlives it. One age key, the last
//! refresh, now serves both expiry and eviction — and both caps read it
//! through the single [`IntroEntry::age_key`], so the two cannot drift.
//! **A same-source retransmit that replaces an entry's bytes makes it young
//! again**, which is the whole point.
//!
//! # Eviction is a linear scan
//!
//! Not a heap. Two reasons, and the second is the load-bearing one: §6.3's
//! honesty clause prices sustained full occupancy at ≈ 68 packets/second
//! (1024 / 15 s), so the scan is ~70 k comparisons/second in the worst case
//! the spec itself contemplates; and a heap keyed on age is *wrong the
//! moment a refresh changes the key*, whereas a scan recomputes the
//! ordering every time and cannot be silently stale.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use crate::constants;
use crate::identity::Identity;

use super::guard::{ChainPin, GuardUndo};
use super::staged::{ChainState, IntroId};

/// §6.3's per-source cap key: the source **IP**, or its /64 for IPv6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SourceKey {
    /// An IPv4 source, keyed on the whole address.
    V4([u8; 4]),
    /// An IPv6 source, keyed on the leading /64 — one address per
    /// interface is free, so a /128 cap would be no cap at all.
    V6Prefix64([u8; 8]),
}

impl SourceKey {
    /// The cap key for a source address.
    pub(crate) fn of(addr: SocketAddr) -> Self {
        match addr.ip() {
            IpAddr::V4(v4) => SourceKey::V4(v4.octets()),
            IpAddr::V6(v6) => {
                let octets = v6.octets();
                let mut prefix = [0u8; 8];
                prefix.copy_from_slice(&octets[..8]);
                SourceKey::V6Prefix64(prefix)
            }
        }
    }
}

/// One parked introduction, or one consumed chain. §6.3.
pub(crate) struct IntroEntry<I: Identity> {
    /// This entry's stable identity. Survives byte replacement (§6.3 rule
    /// 5: "same `IntroId`, newest bytes").
    pub(crate) id: IntroId,
    /// The source address the newest bytes arrived from — also the dedup
    /// key, which is why it never changes while an entry is unconsumed.
    pub(crate) src: SocketAddr,
    /// The cap key, cached so the per-source counter can be decremented
    /// without re-deriving it.
    pub(crate) source_key: SourceKey,
    /// The newest bytes' `sender_index`. §5.5 mints a **new random index
    /// on every retransmit**, so this changes under a refresh — which is
    /// exactly why §16.4's accessor must read it live (ruling 71).
    pub(crate) sender_index: u32,
    /// The newest msg1, verbatim.
    pub(crate) msg1: Vec<u8>,
    /// §6.1's ladder position.
    pub(crate) state: ChainState<I>,
    /// `true` from `read_identity()` (and, in slice 7, from a
    /// freeze-on-carry park). A consumed chain is **never** byte-replaced
    /// and **never** evicted by either cap.
    pub(crate) consumed: bool,
    /// §17.1's provisional guard write, if `authenticate()` has run.
    pub(crate) guard_undo: Option<GuardUndo>,
    /// The static this chain currently pins in the guard, if any, and what
    /// that pin proves (ruling 77). The kind is carried here rather than
    /// re-derived from [`state`](IntroEntry::state) at release, which a
    /// failed verb leaves `Poisoned`.
    pub(crate) guard_pin: Option<ChainPin>,
    /// **Ruling 69's single age key**: the instant of the last refresh, or
    /// of the original park if nothing has refreshed it. Both expiry and
    /// both evictions read it through [`age_key`](Self::age_key).
    refreshed_at: Instant,
}

impl<I: Identity> IntroEntry<I> {
    /// Ruling 69's age key — **last refresh, never original park time**.
    ///
    /// The one place age is defined. Expiry, the global evict-oldest and
    /// the per-source evict-oldest all read it, so there is no second
    /// notion of "oldest" to contradict this one.
    pub(crate) fn age_key(&self) -> Instant {
        self.refreshed_at
    }

    /// When this entry expires: `INTRO_TTL` after its age key.
    ///
    /// A **consumed** chain needs no special case. §6.3 says its mid-state
    /// "expires 15 s after the initiation that fed it", and a consumed
    /// chain is never refreshed again — so its age key is frozen at that
    /// initiation and this expression is already the right answer.
    pub(crate) fn deadline(&self) -> Instant {
        self.refreshed_at + constants::INTRO_TTL
    }
}

/// What an arrival did. §6.3 rules 1–3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// A new entry parked. The caller emits `IntroReady`.
    Parked(IntroId),
    /// An unconsumed entry from the same `SocketAddr` was replaced with the
    /// newer bytes, keeping its `IntroId` and refreshing its TTL. **No
    /// second surfacing** (§6.3 rule 5).
    Refreshed(IntroId),
    /// Silently dropped: the source's whole allowance is consumed chains,
    /// or the queue is full of them. Emits nothing, by §3.1's rule that a
    /// pre-AEAD drop is invisible.
    Dropped,
}

/// The result of an arrival, plus anything it displaced.
pub(crate) struct ArrivalOutcome<I: Identity> {
    /// What happened.
    pub(crate) arrival: Arrival,
    /// The entry an overflow evicted, if any, handed back so the caller can
    /// release whatever endpoint state it held.
    pub(crate) evicted: Option<IntroEntry<I>>,
}

/// §6.3's queue.
pub(crate) struct IntroQueue<I: Identity> {
    entries: HashMap<IntroId, IntroEntry<I>>,
    /// **Unconsumed entries only.** `read_identity()` removes the row,
    /// which is what frees the source's stage-0 slot while leaving the
    /// per-source count unchanged.
    by_addr: HashMap<SocketAddr, IntroId>,
    /// Unconsumed **and** consumed, together. §6.3 makes `read_identity()`
    /// net-zero for this count; modelling both tiers as one counter is what
    /// makes it net-zero *by construction* rather than by a matched pair of
    /// ±1s that can drift.
    per_source: HashMap<SourceKey, u32>,
    next_id: u64,
    cap: usize,
    max_per_source: usize,
}

impl<I: Identity> IntroQueue<I> {
    /// A queue with §6.3's two configurable bounds.
    pub(crate) fn new(cap: usize, max_per_source: usize) -> Self {
        Self {
            entries: HashMap::new(),
            by_addr: HashMap::new(),
            per_source: HashMap::new(),
            next_id: 0,
            cap,
            max_per_source,
        }
    }

    /// §6.3's arrival algorithm: **dedup, then the per-source cap, then the
    /// global cap, then park.**
    ///
    /// §6.3 states the three rules and never their order, and the orders
    /// are not equivalent. This one is derived, and each step's position
    /// has a reason:
    ///
    /// * **Dedup before the per-source cap**, because a same-`SocketAddr`
    ///   arrival is a *replacement* and is net-zero for the cap. Running
    ///   the cap first could evict a stranger to make room for an entry
    ///   that was never going to be added.
    /// * **The per-source cap before the global cap**, because the
    ///   per-source rule replaces *within* the source and leaves the total
    ///   unchanged, so the global cap cannot trip afterwards.
    ///   Global-first would evict another source's entry and then still
    ///   have to enforce the per-source cap: one wasted eviction of an
    ///   innocent.
    /// * **A full queue of consumed chains drops the arrival.** §6.3 states
    ///   this for the per-source case and not for the global one; dropping
    ///   is the only option that does not breach §17.5's ceiling of "one
    ///   budget of `INTRO_QUEUE_CAP` slots", so it is derived rather than
    ///   chosen.
    pub(crate) fn arrive(
        &mut self,
        now: Instant,
        src: SocketAddr,
        sender_index: u32,
        msg1: &[u8],
    ) -> ArrivalOutcome<I> {
        // 1. Dedup. `by_addr` holds unconsumed entries only, so a hit here
        //    is replaceable by construction — a consumed chain can never be
        //    superseded (§6.3 rule 5).
        if let Some(&id) = self.by_addr.get(&src)
            && let Some(entry) = self.entries.get_mut(&id)
        {
            debug_assert!(!entry.consumed, "by_addr holds unconsumed entries only");
            entry.msg1.clear();
            entry.msg1.extend_from_slice(msg1);
            entry.sender_index = sender_index;
            entry.refreshed_at = now;
            return ArrivalOutcome {
                arrival: Arrival::Refreshed(id),
                evicted: None,
            };
        }

        let source_key = SourceKey::of(src);
        let mut evicted = None;

        // 2. The per-source cap. Eviction operates on the unconsumed tier
        //    only; if the source's whole allowance is consumed chains,
        //    there is nothing evictable and the arrival is dropped.
        if u64::from(self.count_for(source_key)) >= self.max_per_source as u64 {
            match self.oldest_unconsumed(Some(source_key)) {
                Some(victim) => evicted = self.remove(victim),
                None => {
                    return ArrivalOutcome {
                        arrival: Arrival::Dropped,
                        evicted: None,
                    };
                }
            }
        }

        // 3. The global cap. Unreachable when step 2 evicted, which is why
        //    the two never compound into a double eviction.
        if self.entries.len() >= self.cap {
            match self.oldest_unconsumed(None) {
                Some(victim) => evicted = self.remove(victim),
                None => {
                    return ArrivalOutcome {
                        arrival: Arrival::Dropped,
                        evicted: None,
                    };
                }
            }
        }

        // 4. Park.
        let id = IntroId::from_raw(self.next_id);
        self.next_id += 1;
        self.entries.insert(
            id,
            IntroEntry {
                id,
                src,
                source_key,
                sender_index,
                msg1: msg1.to_vec(),
                state: ChainState::Parked,
                consumed: false,
                guard_undo: None,
                guard_pin: None,
                refreshed_at: now,
            },
        );
        self.by_addr.insert(src, id);
        *self.per_source.entry(source_key).or_insert(0) += 1;

        ArrivalOutcome {
            arrival: Arrival::Parked(id),
            evicted,
        }
    }

    /// Mark a chain consumed: it owns its bytes and its `IntroId` from here
    /// on (§6.3 rule 5).
    ///
    /// The source's stage-0 slot is freed — a later initiation from it
    /// parks as a **new** entry — while the per-source count is unchanged,
    /// because that count spans both tiers.
    pub(crate) fn consume(&mut self, id: IntroId) {
        if let Some(entry) = self.entries.get_mut(&id)
            && !entry.consumed
        {
            entry.consumed = true;
            let src = entry.src;
            if self.by_addr.get(&src) == Some(&id) {
                self.by_addr.remove(&src);
            }
        }
    }

    /// A parked chain, if it exists.
    pub(crate) fn get(&self, id: IntroId) -> Option<&IntroEntry<I>> {
        self.entries.get(&id)
    }

    /// A parked chain, mutably.
    pub(crate) fn get_mut(&mut self, id: IntroId) -> Option<&mut IntroEntry<I>> {
        self.entries.get_mut(&id)
    }

    /// Remove a chain, returning it so the caller can release the endpoint
    /// state it held (§17.1's provisional write and pin).
    pub(crate) fn remove(&mut self, id: IntroId) -> Option<IntroEntry<I>> {
        let entry = self.entries.remove(&id)?;
        if self.by_addr.get(&entry.src) == Some(&id) {
            self.by_addr.remove(&entry.src);
        }
        match self.per_source.get_mut(&entry.source_key) {
            Some(count) if *count > 1 => *count -= 1,
            // The counter is removed rather than left at zero: a stuck
            // per-source counter is a permanent denial for that source and
            // is invisible to every other observation.
            Some(_) => {
                self.per_source.remove(&entry.source_key);
            }
            None => debug_assert!(false, "per-source counter underflow"),
        }
        Some(entry)
    }

    /// §6.3 rule 4: **silent** eviction at `INTRO_TTL`.
    ///
    /// Emits nothing. The removed entries come back so the caller can run
    /// each one's guard undo — the failure this prevents is a guard record
    /// left behind for a peer that never got to use it, silently blocking
    /// that peer's next genuine initiation.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<IntroEntry<I>> {
        let due: Vec<IntroId> = self
            .entries
            .values()
            .filter(|entry| entry.deadline() <= now)
            .map(|entry| entry.id)
            .collect();
        due.into_iter().filter_map(|id| self.remove(id)).collect()
    }

    /// The earliest expiry, for §16.5's min-deadline.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.entries.values().map(IntroEntry::deadline).min()
    }

    /// How many chains a source holds, both tiers.
    pub(crate) fn count_for(&self, key: SourceKey) -> u32 {
        self.per_source.get(&key).copied().unwrap_or(0)
    }

    /// Ruling 69's eviction victim: the oldest **unconsumed** entry by last
    /// refresh, optionally within one source.
    ///
    /// `IntroId` breaks ties so the choice is deterministic — two entries
    /// refreshed in the same instant are otherwise ordered by hash
    /// iteration, which would make an eviction test flake.
    fn oldest_unconsumed(&self, within: Option<SourceKey>) -> Option<IntroId> {
        self.entries
            .values()
            .filter(|entry| !entry.consumed)
            .filter(|entry| within.is_none_or(|key| entry.source_key == key))
            .min_by(|a, b| a.age_key().cmp(&b.age_key()).then_with(|| a.id.cmp(&b.id)))
            .map(|entry| entry.id)
    }
}

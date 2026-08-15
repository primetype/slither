//! §16.5 — the connection core's named timers, and ruling 76's order.
//!
//! §16.5 names **eight** connection timers and fixes the order in which
//! deadlines falling on one instant are evaluated. Slice 3 arms exactly one
//! of them ([`TimerKind::CloseLinger`]) and one more from §7.4
//! ([`TimerKind::Liveness`]) — so if the order lived as an `if` chain in
//! `handle_timeout` it would be **unexercised**, every permutation would
//! satisfy every slice-3 test, and slices 5 and 7 would each re-derive it
//! from prose. It lives here instead, as data with its own unit tests, so
//! the later slices inherit a ratified order rather than re-deriving one.
//!
//! # The order is the enum's declaration order
//!
//! §16.5, ruling 76 — *"This list is **exhaustive** … The governing
//! principle …: **a terminal outcome precedes a routine one, and state
//! removal precedes emission.** … Per connection, loss detection beats PTO
//! and exactly one of the two fires per evaluation; teardown collection
//! (liveness, `CloseLinger` expiry, then `Contested`) precedes keepalive
//! evaluation …; `AckDelay` fires after the loss/PTO evaluation at the same
//! instant …; and `PersistentKeepalive` is evaluated last."*
//!
//! [`TimerKind`]'s variants are declared in exactly that order and derive
//! `Ord` from it, so the priority is the discriminant and there is no
//! second place for it to be written down differently.

use std::time::Instant;

/// One of §16.5's eight named connection timers.
///
/// **The declaration order is ruling 76's equal-deadline priority**, and
/// the derived `Ord` is that priority — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TimerKind {
    /// §7.4's death clock. Teardown collection, and terminal, so first.
    Liveness,
    /// §15.2's post-mortem expiry: state removal, so ahead of emission.
    CloseLinger,
    /// §7.5's contested-connection probe verdict. Slice 7.
    Contested,
    /// §13's loss detection. Beats [`Pto`](TimerKind::Pto). Slice 5.
    Loss,
    /// §13.3's probe timeout. Slice 5.
    Pto,
    /// §12.3's delayed-ACK deadline. Slice 5.
    AckDelay,
    /// §7.5's passive keepalive. Slice 7.
    Keepalive,
    /// §7.5's unconditional beacon, evaluated last. Slice 7.
    PersistentKeepalive,
}

impl TimerKind {
    /// Every timer, in ruling 76's order. **Exhaustive by ratification** —
    /// §16.5 says so in as many words, which is why this array is written
    /// out rather than derived from whatever happens to be armed.
    pub(crate) const ALL: [TimerKind; 8] = [
        TimerKind::Liveness,
        TimerKind::CloseLinger,
        TimerKind::Contested,
        TimerKind::Loss,
        TimerKind::Pto,
        TimerKind::AckDelay,
        TimerKind::Keepalive,
        TimerKind::PersistentKeepalive,
    ];

    /// This timer's slot, which is also its priority: lower is sooner.
    const fn index(self) -> usize {
        self as usize
    }
}

/// The timers due at one instant, in ruling 76's order.
///
/// A set rather than a single kind because §16.5's `handle_timeout` is one
/// **evaluation**: every deadline at or before `now` fires, in order, and
/// the loss/PTO pair contributes at most one member to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Due(u8);

impl Due {
    /// Whether `kind` is in the set.
    pub(crate) const fn contains(self, kind: TimerKind) -> bool {
        self.0 & (1 << kind.index()) != 0
    }

    /// Whether nothing is due.
    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// How many timers are due.
    pub(crate) const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// The due timers, **in ruling 76's order**.
    pub(crate) fn iter(self) -> impl Iterator<Item = TimerKind> {
        TimerKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }

    fn insert(&mut self, kind: TimerKind) {
        self.0 |= 1 << kind.index();
    }

    fn remove(&mut self, kind: TimerKind) {
        self.0 &= !(1 << kind.index());
    }
}

/// §16.5's timer table: eight named deadlines, one min-deadline out.
///
/// Every slot is `None` at construction. Nothing here knows what any timer
/// *means* — arming and the logic behind each expiry belong to the section
/// that defines the timer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Timers([Option<Instant>; 8]);

impl Timers {
    /// A table with nothing armed.
    pub(crate) const fn new() -> Self {
        Self([None; 8])
    }

    /// This timer's deadline, if armed.
    pub(crate) fn get(&self, kind: TimerKind) -> Option<Instant> {
        self.0[kind.index()]
    }

    /// Arm `kind` at `at`, replacing any deadline it already had.
    pub(crate) fn arm(&mut self, kind: TimerKind, at: Instant) {
        self.0[kind.index()] = Some(at);
    }

    /// Arm or disarm `kind` in one call — for a timer whose deadline is
    /// **derived** from other state and re-synchronised after every change
    /// to it (§7.4's `Liveness` is the slice-3 example).
    pub(crate) fn set(&mut self, kind: TimerKind, at: Option<Instant>) {
        self.0[kind.index()] = at;
    }

    /// Disarm `kind`. Idempotent.
    pub(crate) fn disarm(&mut self, kind: TimerKind) {
        self.0[kind.index()] = None;
    }

    /// Disarm everything but `keep`.
    ///
    /// §15.2 drops all stream, flow-control, recovery and congestion state
    /// on entering the post-mortem, which disarms `Loss`, `Pto` and
    /// `AckDelay` by removing what they act on; §15.2 says nothing about
    /// the other four, and a surviving `Liveness` could produce a **second**
    /// `Closed` behind a latch that is already resolved. So the post-mortem
    /// keeps `CloseLinger` and nothing else.
    pub(crate) fn disarm_all_except(&mut self, keep: TimerKind) {
        for kind in TimerKind::ALL {
            if kind != keep {
                self.disarm(kind);
            }
        }
    }

    /// Disarm every timer.
    pub(crate) fn disarm_all(&mut self) {
        self.0 = [None; 8];
    }

    /// §16.4's single min-deadline out: the earliest armed deadline.
    pub(crate) fn next(&self) -> Option<Instant> {
        self.0.iter().flatten().copied().min()
    }

    /// The timers due at `now`, **without** disarming them.
    ///
    /// "Due" is `deadline <= now`: §16.5 says an armed deadline `D` fires
    /// no earlier than `D`, so the instant `D` itself is a firing instant.
    ///
    /// `Loss` and `Pto` contribute at most one member — ruling 76:
    /// *"loss detection beats PTO and exactly one of the two fires per
    /// evaluation"*.
    pub(crate) fn due(&self, now: Instant) -> Due {
        let mut due = Due::default();
        for kind in TimerKind::ALL {
            if self.get(kind).is_some_and(|deadline| deadline <= now) {
                due.insert(kind);
            }
        }
        if due.contains(TimerKind::Loss) {
            due.remove(TimerKind::Pto);
        }
        due
    }

    /// The timers due at `now`, **stopped before their logic runs**.
    ///
    /// §16.5: *"`handle_timeout` is idempotent: each due timer is stopped
    /// before its logic runs, so spurious or repeated calls no-op."* Taking
    /// the whole due set and disarming it in one step is what makes that
    /// structural rather than a discipline every arm has to remember — a
    /// second `handle_timeout(now)` at the same instant finds an empty set.
    ///
    /// The loss/PTO collapse is applied **before** disarming, so a `Pto`
    /// suppressed by a same-instant `Loss` stays armed for the next
    /// evaluation rather than being silently dropped.
    pub(crate) fn take_due(&mut self, now: Instant) -> Due {
        let due = self.due(now);
        for kind in due.iter() {
            self.disarm(kind);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    /// The declaration order **is** ruling 76's order, and `Ord` is it.
    #[test]
    fn declaration_order_is_ruling_76s_order() {
        assert!(TimerKind::Liveness < TimerKind::CloseLinger);
        assert!(TimerKind::CloseLinger < TimerKind::Contested);
        assert!(TimerKind::Contested < TimerKind::Loss);
        assert!(TimerKind::Loss < TimerKind::Pto);
        assert!(TimerKind::Pto < TimerKind::AckDelay);
        assert!(TimerKind::AckDelay < TimerKind::Keepalive);
        assert!(TimerKind::Keepalive < TimerKind::PersistentKeepalive);

        // And `ALL` is that same order, not a re-listing that could drift.
        let mut sorted = TimerKind::ALL;
        sorted.sort();
        assert_eq!(sorted, TimerKind::ALL);
    }

    /// Two timers on **one instant** come out in the ratified order — the
    /// assertion a single-timer slice cannot otherwise make.
    #[test]
    fn equal_deadlines_resolve_in_ruling_76s_order() {
        let now = t0();
        let mut timers = Timers::new();
        // Armed in the reverse of the priority order, so an implementation
        // that returned insertion order would fail.
        timers.arm(TimerKind::PersistentKeepalive, now);
        timers.arm(TimerKind::Keepalive, now);
        timers.arm(TimerKind::AckDelay, now);
        timers.arm(TimerKind::Contested, now);
        timers.arm(TimerKind::CloseLinger, now);
        timers.arm(TimerKind::Liveness, now);

        let order: Vec<TimerKind> = timers.due(now).iter().collect();
        assert_eq!(
            order,
            vec![
                TimerKind::Liveness,
                TimerKind::CloseLinger,
                TimerKind::Contested,
                TimerKind::AckDelay,
                TimerKind::Keepalive,
                TimerKind::PersistentKeepalive,
            ]
        );
    }

    /// `Liveness` beats `CloseLinger` at the same instant, and beats
    /// `Contested` — the two pairs ruling 76 names explicitly.
    #[test]
    fn liveness_beats_close_linger_and_contested() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::CloseLinger, now);
        timers.arm(TimerKind::Liveness, now);
        assert_eq!(timers.due(now).iter().next(), Some(TimerKind::Liveness));

        let mut timers = Timers::new();
        timers.arm(TimerKind::Contested, now);
        timers.arm(TimerKind::Liveness, now);
        assert_eq!(timers.due(now).iter().next(), Some(TimerKind::Liveness));
    }

    /// Loss and PTO on one instant: **exactly one** fires, and it is Loss.
    /// The suppressed PTO stays armed rather than being dropped.
    #[test]
    fn loss_and_pto_yield_exactly_one_and_it_is_loss() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::Loss, now);
        timers.arm(TimerKind::Pto, now);

        let due = timers.due(now);
        assert_eq!(due.len(), 1, "exactly one of the pair fires");
        assert!(due.contains(TimerKind::Loss));
        assert!(!due.contains(TimerKind::Pto));

        let taken = timers.take_due(now);
        assert_eq!(taken.len(), 1);
        assert!(timers.get(TimerKind::Loss).is_none(), "Loss was stopped");
        assert_eq!(
            timers.get(TimerKind::Pto),
            Some(now),
            "the suppressed PTO is still armed for the next evaluation"
        );
    }

    /// PTO alone fires when Loss is not armed — otherwise the collapse
    /// above would be indistinguishable from "PTO never fires".
    #[test]
    fn pto_fires_alone() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::Pto, now);
        assert_eq!(timers.due(now).iter().next(), Some(TimerKind::Pto));
    }

    /// `AckDelay` fires after the loss/PTO evaluation at the same instant.
    #[test]
    fn ack_delay_follows_the_loss_evaluation() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::AckDelay, now);
        timers.arm(TimerKind::Loss, now);
        let order: Vec<TimerKind> = timers.due(now).iter().collect();
        assert_eq!(order, vec![TimerKind::Loss, TimerKind::AckDelay]);
    }

    /// A deadline fires **at** `D`, not only after it, and not before.
    #[test]
    fn a_deadline_fires_at_d_and_not_before() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::CloseLinger, now + Duration::from_secs(5));

        assert!(
            timers.due(now + Duration::from_millis(4_999)).is_empty(),
            "not before D"
        );
        assert!(
            timers
                .due(now + Duration::from_secs(5))
                .contains(TimerKind::CloseLinger),
            "at D"
        );
        assert!(
            timers
                .due(now + Duration::from_millis(5_001))
                .contains(TimerKind::CloseLinger),
            "and after D"
        );
    }

    /// `take_due` is what makes `handle_timeout` idempotent: the second
    /// call at the same instant finds nothing.
    #[test]
    fn take_due_is_idempotent_at_one_instant() {
        let now = t0();
        let mut timers = Timers::new();
        timers.arm(TimerKind::CloseLinger, now);

        assert_eq!(timers.take_due(now).len(), 1);
        assert!(timers.take_due(now).is_empty());
    }

    /// The min-deadline out is the minimum over **armed** timers, and
    /// `None` when nothing is armed.
    #[test]
    fn next_is_the_minimum_over_armed_timers() {
        let now = t0();
        let mut timers = Timers::new();
        assert_eq!(timers.next(), None);

        timers.arm(TimerKind::Keepalive, now + Duration::from_secs(9));
        timers.arm(TimerKind::Liveness, now + Duration::from_secs(25));
        timers.arm(TimerKind::CloseLinger, now + Duration::from_secs(5));
        assert_eq!(timers.next(), Some(now + Duration::from_secs(5)));

        // The minimum is not the highest-priority timer: a lower-priority
        // timer with an earlier deadline still wins the announcement.
        timers.disarm(TimerKind::CloseLinger);
        assert_eq!(timers.next(), Some(now + Duration::from_secs(9)));
    }

    /// Entering the post-mortem keeps `CloseLinger` and nothing else.
    #[test]
    fn disarm_all_except_keeps_exactly_one() {
        let now = t0();
        let mut timers = Timers::new();
        for kind in TimerKind::ALL {
            timers.arm(kind, now);
        }
        timers.disarm_all_except(TimerKind::CloseLinger);

        assert_eq!(timers.get(TimerKind::CloseLinger), Some(now));
        for kind in TimerKind::ALL {
            if kind != TimerKind::CloseLinger {
                assert_eq!(timers.get(kind), None, "{kind:?} survived the post-mortem");
            }
        }
        assert_eq!(timers.next(), Some(now));
    }

    /// `set` arms and disarms, for a derived deadline re-synchronised after
    /// every change to the state it is derived from.
    #[test]
    fn set_arms_and_disarms() {
        let now = t0();
        let mut timers = Timers::new();
        timers.set(TimerKind::Liveness, Some(now));
        assert_eq!(timers.get(TimerKind::Liveness), Some(now));
        timers.set(TimerKind::Liveness, None);
        assert_eq!(timers.get(TimerKind::Liveness), None);
    }
}

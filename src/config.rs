//! Endpoint configuration, and §16.5's injected wall clock.
//!
//! §16.5: *"`now: Instant` is an explicit argument on every mutating call;
//! the cores never read a clock. The initiation timestamp (§5.3) is the one
//! wall-clock read, behind a **clock service injected in the endpoint
//! config**."* That sentence is why a clock seam lives here and nowhere
//! else: `Instant` is the caller's to supply, and the single wall-clock
//! reading the protocol performs is the initiation timestamp.
//!
//! # `Rc`, not `Arc`
//!
//! [`Config`] holds `Rc<dyn WallClock>`, deliberately. `Arc<dyn WallClock +
//! Send + Sync>` is the shape a Rust programmer reaches for by default, and
//! it would quietly make `Config: Send` something consumers depend on — on
//! the one path §16.3 keeps free of `Send` so a hardware-backed static key
//! can drive it (S21).

use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants;
use crate::core::Timestamp;

/// The one wall-clock reading the protocol performs (§5.3, §16.5).
///
/// Injected so a test can drive §17.2's monotonic forcing without waiting
/// on a real clock, and so an embedded host can supply whatever time source
/// it has. Nothing else in slither reads wall-clock time.
pub trait WallClock {
    /// The current wall-clock time, as §5.2's `secs ‖ nanos` pair.
    fn now(&self) -> Timestamp;
}

/// [`WallClock`] over `std::time::SystemTime`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl WallClock for SystemClock {
    fn now(&self) -> Timestamp {
        // A clock before the epoch is not a protocol condition and has no
        // error path: §5.3 specifies no validation, and §17.2's forcing
        // makes even a zero reading emit a strictly-greater timestamp.
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => Timestamp::new(d.as_secs(), d.subsec_nanos()),
            Err(_) => Timestamp::new(0, 0),
        }
    }
}

/// An endpoint's configuration.
///
/// The two introduction-queue bounds are configurable because §6.3 says so
/// in as many words ("configurable in `Config`"). `INTRO_TTL` is **not** —
/// it is a ratified timer, not a knob, and there is no field for it.
#[derive(Clone)]
pub struct Config {
    intro_queue_cap: usize,
    intro_max_per_source: usize,
    epoch_size: NonZeroU64,
    clock: Rc<dyn WallClock>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("intro_queue_cap", &self.intro_queue_cap)
            .field("intro_max_per_source", &self.intro_max_per_source)
            .field("epoch_size", &self.epoch_size)
            .finish_non_exhaustive()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intro_queue_cap: constants::INTRO_QUEUE_CAP,
            intro_max_per_source: constants::INTRO_MAX_PER_SOURCE,
            epoch_size: Config::DEFAULT_EPOCH_SIZE,
            clock: Rc::new(SystemClock),
        }
    }
}

impl Config {
    /// §7.7's ratified epoch size: `REKEY_EPOCH_MSGS` messages per epoch.
    ///
    /// The production value, and the only one a shipped build uses.
    pub const DEFAULT_EPOCH_SIZE: NonZeroU64 = match NonZeroU64::new(constants::REKEY_EPOCH_MSGS) {
        Some(n) => n,
        None => panic!("REKEY_EPOCH_MSGS is nonzero"),
    };

    /// The defaults: §6.3's ratified caps, §7.7's epoch size and a
    /// `SystemTime` clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the endpoint-wide stage-0 queue cap (§6.3).
    ///
    /// The default is [`INTRO_QUEUE_CAP`](crate::constants::INTRO_QUEUE_CAP)
    /// (1024). This value is **not validated**, and two of the things it
    /// buys are load-bearing rather than advisory:
    ///
    /// - **`0` disables every inbound accept, permanently and silently.**
    ///   An arrival finds `entries.len() >= cap` with nothing evictable, so
    ///   it is dropped; no introduction is ever surfaced, `accept()` never
    ///   resolves, and there is no error, no event and no trace to say why.
    ///   Outbound `connect()` still works, which is what makes the
    ///   misconfiguration look like a peer problem.
    /// - **It is the numerator of §6.3's occupancy bound.** *"Filling the
    ///   queue needs ≥ 256 distinct sources"* is exactly this value divided
    ///   by [`with_intro_max_per_source`](Self::with_intro_max_per_source);
    ///   lowering one without the other lowers the number of sources an
    ///   attacker needs. §6.3 also prices sustained full occupancy at
    ///   ≈ 68 packets/second from `cap / INTRO_TTL`, so a larger cap costs
    ///   an attacker proportionally more bandwidth — and costs this
    ///   endpoint proportionally more memory (§17.5 bounds a parked chain at
    ///   ≈ 0.5–1 KB).
    #[must_use]
    pub fn with_intro_queue_cap(mut self, cap: usize) -> Self {
        self.intro_queue_cap = cap;
        self
    }

    /// Override the per-source chain cap (§6.3).
    ///
    /// The default is
    /// [`INTRO_MAX_PER_SOURCE`](crate::constants::INTRO_MAX_PER_SOURCE)
    /// (4). This value is **not validated**, and §6.3 calls what it buys
    /// *"the only occupant-shaped defence"* the protocol has until the
    /// deferred cookies/mac2 round (§19):
    ///
    /// - **`0` disables every inbound accept**, exactly as
    ///   [`with_intro_queue_cap(0)`](Self::with_intro_queue_cap) does and
    ///   for the same reason — the per-source check fires on the first
    ///   arrival and finds nothing to evict.
    /// - **Any value `>= intro_queue_cap` deletes the bound entirely.** The
    ///   per-source check can then never fire before the global one, so a
    ///   **single** source — one IP, one port, no spoofing capability,
    ///   since mac1's key is public data — can hold every slot in the
    ///   queue. §6.3's *"≥ 256 distinct sources"* is `cap /
    ///   max_per_source`, and at parity that number is 1.
    ///   [`constants`] pins
    ///   `INTRO_MAX_PER_SOURCE <= INTRO_QUEUE_CAP` as a compile-time
    ///   assertion for the **defaults**; nothing pins it for these two
    ///   builders.
    ///
    /// **There is a legitimate reason to raise it, which is why this note
    /// exists — and it turns on which key is which.** §6.3's *dedup* key is
    /// the full source address, so *"distinct initiators behind one NAT
    /// present distinct ports"* and each gets its own queue entry. This
    /// **cap** is keyed one level coarser: *"4 chains per source IP (per
    /// /64 for IPv6)"*. Every client behind one NAT therefore shares a
    /// single allowance of 4, and an operator serving many of them has a
    /// real reason to raise this knob. Doing so trades occupancy resistance
    /// for reachability, knowingly; raising it to or past the queue cap is
    /// not a trade, it is a removal.
    #[must_use]
    pub fn with_intro_max_per_source(mut self, cap: usize) -> Self {
        self.intro_max_per_source = cap;
        self
    }

    /// Override §7.7's epoch size — **a test-only facility**
    /// (**ruling 82**).
    ///
    /// §7.7's ratchet schedule is a pure function of the counter, so **both
    /// peers must pass the same value** or they disagree about which key
    /// opens which packet. Production uses
    /// [`REKEY_EPOCH_MSGS`](crate::constants::REKEY_EPOCH_MSGS), which is
    /// what [`Config::default`] supplies; this exists so a test can observe
    /// an epoch boundary without performing 65 536 seals to reach one.
    ///
    /// It carries §16.6's rule for the RNG seed verbatim: a build that
    /// accepts a caller-chosen epoch must be **feature-gated or documented
    /// as test-only**, and this is that documentation. Nothing in slither
    /// calls it outside tests, and a deployment that does has changed a
    /// ratified security parameter.
    #[must_use]
    pub fn with_epoch_size(mut self, epoch_size: NonZeroU64) -> Self {
        self.epoch_size = epoch_size;
        self
    }

    /// Replace the wall-clock service (§16.5).
    #[must_use]
    pub fn with_clock(mut self, clock: Rc<dyn WallClock>) -> Self {
        self.clock = clock;
        self
    }

    /// The endpoint-wide stage-0 queue cap.
    pub fn intro_queue_cap(&self) -> usize {
        self.intro_queue_cap
    }

    /// The per-source chain cap.
    pub fn intro_max_per_source(&self) -> usize {
        self.intro_max_per_source
    }

    /// §7.7's epoch size, for hiss's ratcheting datagram split.
    pub fn epoch_size(&self) -> NonZeroU64 {
        self.epoch_size
    }

    /// The injected wall clock.
    pub fn clock(&self) -> &dyn WallClock {
        &*self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling 82 splits the boundary behaviour from the constant, so the
    /// constant needs its own pin: a `Config` nobody configured ratchets on
    /// §7.7's ratified schedule.
    #[test]
    fn the_default_epoch_size_is_rekey_epoch_msgs() {
        assert_eq!(
            Config::new().epoch_size().get(),
            constants::REKEY_EPOCH_MSGS
        );
        assert_eq!(
            Config::DEFAULT_EPOCH_SIZE.get(),
            constants::REKEY_EPOCH_MSGS
        );
    }

    #[test]
    fn the_epoch_size_override_takes_effect() {
        let config = Config::new().with_epoch_size(NonZeroU64::new(8).unwrap());
        assert_eq!(config.epoch_size().get(), 8);
    }
}

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
    clock: Rc<dyn WallClock>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("intro_queue_cap", &self.intro_queue_cap)
            .field("intro_max_per_source", &self.intro_max_per_source)
            .finish_non_exhaustive()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intro_queue_cap: constants::INTRO_QUEUE_CAP,
            intro_max_per_source: constants::INTRO_MAX_PER_SOURCE,
            clock: Rc::new(SystemClock),
        }
    }
}

impl Config {
    /// The defaults: §6.3's ratified caps and a `SystemTime` clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the endpoint-wide stage-0 queue cap (§6.3).
    #[must_use]
    pub fn with_intro_queue_cap(mut self, cap: usize) -> Self {
        self.intro_queue_cap = cap;
        self
    }

    /// Override the per-source chain cap (§6.3).
    #[must_use]
    pub fn with_intro_max_per_source(mut self, cap: usize) -> Self {
        self.intro_max_per_source = cap;
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

    /// The injected wall clock.
    pub fn clock(&self) -> &dyn WallClock {
        &*self.clock
    }
}

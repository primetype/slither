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

/// Why [`Config::with_flow_windows`] refused a pair of windows.
///
/// **[ruling 259(viii)]** Every variant is a *raise* that is not a raise:
/// the knob may only widen §10.2's ratified initial windows, and it may
/// not widen them past what §8.1's varint can carry or past each other.
///
/// **Integrator note (this slice's partition):** these three belong beside
/// [`crate::error::ConfigError`]'s keepalive variants, and the crate root
/// should re-export the type alongside `Config`. `src/error.rs` and
/// `src/lib.rs` were outside the partition this knob was written in, so
/// the type lives here and is reachable as
/// `slither::config::WindowError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WindowError {
    /// A window below §10.2's ratified initial value.
    ///
    /// The knob **raises**; it does not lower. Lowering re-opens every
    /// sizing proof that rests on the constants — §17.5's memory ceiling,
    /// §9.8's message bound, and the un-negotiated initial value a peer
    /// assumes before any credit frame arrives.
    #[error("a flow-control window below §10.2's ratified initial value")]
    TooSmall,
    /// A window above the largest value a §8.1 varint carries.
    ///
    /// MAX_DATA and MAX_STREAM_DATA carry the advertised limit as one
    /// varint (§8.4), and the limit is an **absolute offset** that only
    /// grows, so a window the frame cannot encode is unusable from the
    /// first grant. `constants.rs` pins the same bound on the defaults.
    #[error("a flow-control window above VarInt::MAX_VALUE (2^62 - 1)")]
    TooLarge,
    /// The stream window exceeds the connection window.
    ///
    /// `constants.rs` pins `INITIAL_MAX_STREAM_DATA <= INITIAL_MAX_DATA`
    /// for the defaults; a configured pair that inverts it advertises
    /// per-stream credit the connection ledger will refuse anyway (§10.5
    /// checks both levels).
    #[error("the stream window exceeds the connection window")]
    StreamAboveConnection,
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
    stream_window: u64,
    connection_window: u64,
    clock: Rc<dyn WallClock>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("intro_queue_cap", &self.intro_queue_cap)
            .field("intro_max_per_source", &self.intro_max_per_source)
            .field("epoch_size", &self.epoch_size)
            .field("stream_window", &self.stream_window)
            .field("connection_window", &self.connection_window)
            .finish_non_exhaustive()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intro_queue_cap: constants::INTRO_QUEUE_CAP,
            intro_max_per_source: constants::INTRO_MAX_PER_SOURCE,
            epoch_size: Config::DEFAULT_EPOCH_SIZE,
            stream_window: Config::DEFAULT_STREAM_WINDOW,
            connection_window: Config::DEFAULT_CONNECTION_WINDOW,
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

    /// §10.2's ratified per-stream receive window:
    /// [`INITIAL_MAX_STREAM_DATA`](crate::constants::INITIAL_MAX_STREAM_DATA)
    /// (262 144 B).
    ///
    /// The production value, and what a shipped build advertises unless
    /// [`with_flow_windows`](Self::with_flow_windows) says otherwise.
    pub const DEFAULT_STREAM_WINDOW: u64 = constants::INITIAL_MAX_STREAM_DATA;

    /// §10.2's ratified connection-level receive window:
    /// [`INITIAL_MAX_DATA`](crate::constants::INITIAL_MAX_DATA)
    /// (1 048 576 B).
    ///
    /// The production value, and what a shipped build advertises unless
    /// [`with_flow_windows`](Self::with_flow_windows) says otherwise.
    pub const DEFAULT_CONNECTION_WINDOW: u64 = constants::INITIAL_MAX_DATA;

    /// The defaults: §6.3's ratified caps, §7.7's epoch size, §10.2's two
    /// receive windows and a `SystemTime` clock.
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

    /// Raise §10.2's two advertised receive windows — **the knob raises,
    /// never lowers** (**ruling 259(viii)**).
    ///
    /// The defaults are [`DEFAULT_STREAM_WINDOW`](Self::DEFAULT_STREAM_WINDOW)
    /// (256 KiB) and
    /// [`DEFAULT_CONNECTION_WINDOW`](Self::DEFAULT_CONNECTION_WINDOW)
    /// (1 MiB), which are §10.2's ratified constants and what a `Config`
    /// nobody configured advertises. Ruling 247(a) measured what they cost:
    /// one stream is capped at `stream_window / RTT`, about 2.5 MiB/s at
    /// 100 ms, and before this knob a consumer had no way to buy more.
    ///
    /// # What changes, and what does not
    ///
    /// **Nothing on the wire changes shape.** §10.2's initial windows are
    /// protocol constants that are never sent; MAX_DATA and
    /// MAX_STREAM_DATA already carry whatever the receiver has decided to
    /// advertise, so a raised window is announced through the frames §10.3
    /// already emits, at values §8.4 already admits. The ratified
    /// constants keep their values and stay the defaults.
    ///
    /// Three things this deliberately does **not** touch:
    ///
    /// - **The peer's limits.** A connection still assumes the *peer*
    ///   advertises §10.2's constants until a credit frame says otherwise.
    ///   The knob is one endpoint's receive policy; it is not negotiated
    ///   and the peer needs no matching build.
    /// - **[`MESSAGE_RECV_MAX`](crate::constants::MESSAGE_RECV_MAX).**
    ///   §9.8's message bound is a *cross-peer* contract checked on the
    ///   **send** side, and a sender cannot know what its receiver
    ///   configured. Raising it locally would make `send_message` emit a
    ///   payload that a default peer resets with `MESSAGE_OVERFLOW`. It
    ///   stays equal to
    ///   [`INITIAL_MAX_STREAM_DATA`](crate::constants::INITIAL_MAX_STREAM_DATA),
    ///   which is what `constants.rs` asserts and §9.8's table states.
    ///   Messages stay bounded at 256 KiB however wide this endpoint's
    ///   streams are; larger transfers use real streams, exactly as §9.8
    ///   says.
    /// - **§17.5's memory ceiling shape.** The receive commitment per
    ///   connection is the *connection* window, so raising it is a
    ///   deliberate purchase of memory: `connection_window` bytes per live
    ///   connection, plus reassembly metadata. Raise the pair, not one of
    ///   them, and size it against how many connections this endpoint
    ///   expects.
    ///
    /// # Errors
    ///
    /// - [`WindowError::TooSmall`] if either value is below its ratified
    ///   default. Lowering re-opens every sizing proof that rests on the
    ///   constants, so it is refused rather than clamped.
    /// - [`WindowError::TooLarge`] if either value exceeds `2^62 - 1`, the
    ///   largest offset a §8.1 varint carries.
    /// - [`WindowError::StreamAboveConnection`] if `stream` exceeds
    ///   `connection` — the relation `constants.rs` pins for the defaults.
    ///
    /// # Example
    ///
    /// ```
    /// use slither::Config;
    ///
    /// // 2 MiB per stream, 8 MiB per connection: ~20 MiB/s on one stream
    /// // at a 100 ms RTT.
    /// let config = Config::new()
    ///     .with_flow_windows(2 * 1024 * 1024, 8 * 1024 * 1024)
    ///     .expect("a raise within the varint bound");
    /// assert_eq!(config.stream_window(), 2 * 1024 * 1024);
    /// ```
    pub fn with_flow_windows(mut self, stream: u64, connection: u64) -> Result<Self, WindowError> {
        if stream < Self::DEFAULT_STREAM_WINDOW || connection < Self::DEFAULT_CONNECTION_WINDOW {
            return Err(WindowError::TooSmall);
        }
        if stream > crate::varint::VarInt::MAX_VALUE
            || connection > crate::varint::VarInt::MAX_VALUE
        {
            return Err(WindowError::TooLarge);
        }
        if stream > connection {
            return Err(WindowError::StreamAboveConnection);
        }
        self.stream_window = stream;
        self.connection_window = connection;
        Ok(self)
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

    /// §10.2's per-stream receive window, as this endpoint advertises it.
    pub fn stream_window(&self) -> u64 {
        self.stream_window
    }

    /// §10.2's connection-level receive window, as this endpoint
    /// advertises it.
    pub fn connection_window(&self) -> u64 {
        self.connection_window
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

    /// **[ruling 259(viii)]** The knob's whole premise: the ratified
    /// constants stay the defaults. A `Config` nobody configured advertises
    /// §10.2's two values and nothing else.
    #[test]
    fn the_default_windows_are_the_ratified_constants() {
        let config = Config::new();
        assert_eq!(config.stream_window(), constants::INITIAL_MAX_STREAM_DATA);
        assert_eq!(config.connection_window(), constants::INITIAL_MAX_DATA);
        assert_eq!(
            Config::DEFAULT_STREAM_WINDOW,
            constants::INITIAL_MAX_STREAM_DATA
        );
        assert_eq!(
            Config::DEFAULT_CONNECTION_WINDOW,
            constants::INITIAL_MAX_DATA
        );
    }

    #[test]
    fn the_window_override_takes_effect() {
        let config = Config::new()
            .with_flow_windows(1 << 20, 1 << 23)
            .expect("a raise");
        assert_eq!(config.stream_window(), 1 << 20);
        assert_eq!(config.connection_window(), 1 << 23);
    }

    /// The knob raises, never lowers — on **either** value independently,
    /// and at the boundary the defaults themselves are accepted (a no-op
    /// raise), so the refusal is `< default`, not `<= default`.
    #[test]
    fn a_window_below_its_ratified_default_is_refused() {
        assert_eq!(
            Config::new()
                .with_flow_windows(
                    constants::INITIAL_MAX_STREAM_DATA - 1,
                    constants::INITIAL_MAX_DATA,
                )
                .unwrap_err(),
            WindowError::TooSmall,
        );
        assert_eq!(
            Config::new()
                .with_flow_windows(
                    constants::INITIAL_MAX_STREAM_DATA,
                    constants::INITIAL_MAX_DATA - 1,
                )
                .unwrap_err(),
            WindowError::TooSmall,
        );
        let at_the_defaults = Config::new()
            .with_flow_windows(
                constants::INITIAL_MAX_STREAM_DATA,
                constants::INITIAL_MAX_DATA,
            )
            .expect("the defaults restated are a legal no-op raise");
        assert_eq!(
            at_the_defaults.stream_window(),
            constants::INITIAL_MAX_STREAM_DATA
        );
    }

    /// The §8.1 varint ceiling, from both sides of the boundary: the
    /// largest encodable offset is accepted and one more is refused.
    #[test]
    fn a_window_past_the_varint_bound_is_refused() {
        let max = crate::varint::VarInt::MAX_VALUE;
        assert_eq!(
            Config::new()
                .with_flow_windows(max, max)
                .unwrap()
                .stream_window(),
            max,
            "the largest encodable absolute offset is admissible",
        );
        assert_eq!(
            Config::new()
                .with_flow_windows(max + 1, max + 1)
                .unwrap_err(),
            WindowError::TooLarge,
        );
        assert_eq!(
            Config::new().with_flow_windows(max, max + 1).unwrap_err(),
            WindowError::TooLarge,
            "the connection window is checked on its own, not only via the pair",
        );
    }

    #[test]
    fn the_stream_window_may_not_exceed_the_connection_window() {
        assert_eq!(
            Config::new()
                .with_flow_windows(1 << 23, 1 << 20)
                .unwrap_err(),
            WindowError::StreamAboveConnection,
        );
    }
}

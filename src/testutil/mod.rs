//! The kernel-free test fixtures — `SPEC.md` §16.10, ruling 60.
//!
//! This module is **normative surface, not a test convention**. Ruling 60
//! attests [`Network`], [`FlakyWire`] and [`FlakyPolicy`] *"on the same
//! terms as §18.2's trace targets: renaming or dropping one is a protocol
//! revision, because a consumer's test suite is built on them."*
//!
//! # What it is
//!
//! [`Network`] is the in-memory routing fabric: it owns the
//! address → endpoint map and moves datagrams between [`FlakyWire`]s. A
//! `FlakyWire` is a [`Wire`], so an endpoint
//! driven over one is driven over exactly the seam it will use in
//! production — with no socket, no port, and no kernel. Every delay is a
//! `tokio::time::sleep_until`, so on tokio's paused clock the whole
//! protocol runs in virtual time.
//!
//! # Determinism is a MUST
//!
//! Ruling 60: the fabric *"MUST be deterministic under a caller-supplied
//! seed. A flow test that cannot be replayed byte-for-byte from its seed is
//! not a regression test, and the loss-dependent behaviour in §13 and §7.5
//! is exactly where a once-in-a-thousand-runs failure would otherwise be
//! unactionable."*
//!
//! Concretely, and these are contract rather than implementation detail:
//!
//! - [`Network::seeded`] is the constructor; [`Network::new`] is
//!   `seeded(0)`. **There is no OS-entropy path**, and adding one would
//!   violate the ruling.
//! - **One RNG per [`FlakyWire`]**, seeded from the network seed and the
//!   wire's registration ordinal — so adding a third endpoint to a test
//!   does not reshuffle the first two's draws.
//! - **A fixed draw order per `send_to`**, documented on
//!   [`FlakyWire::send_to`].
//! - No wall clock, no `SystemTime`, no thread identity, and no hash
//!   iteration order anywhere in a decision path. Registration order is an
//!   explicit counter and every map is ordered.
//!
//! For anything asserting a *specific* outcome, prefer the index-based
//! tools — [`FlakyPolicy::drop_at`] and [`FlakyPolicy::drop_first`] — over
//! probabilistic loss. Probabilistic loss is reproducible under a seed but
//! brittle: it moves when an unrelated send is added.
//!
//! # Everything here is `!Send`, on purpose
//!
//! `Network` and `FlakyWire` hold `Rc`s. They are the fixture for an actor
//! that must not require `Send`, so a `Send` fixture would let a `Send`
//! bound creep into the driver unnoticed.

use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::io;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Duration;

use hiss::curve::{Curve, DhCurve, p256::P256};
use hiss::provider::{CryptoKeyProvider, DhProvider};
use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::shell::wire::Wire;

/// `ENETUNREACH` for this target — 101 on Linux and Android, 51 on the
/// BSDs and Apple platforms.
///
/// The default errno [`FlakyPolicy::failing_sends_until`] injects, so a
/// test can assert the exact `raw_os_error()` the driver will see.
pub const ENETUNREACH: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
    101
} else {
    51
};

/// The golden-ratio odd constant used to decorrelate per-wire RNG seeds.
const SEED_STRIDE: u64 = 0x9E37_79B9_7F4A_7C15;

// ═══════════════════════════════════════════════════════════════════════
// Observation
// ═══════════════════════════════════════════════════════════════════════

/// One observed datagram.
///
/// Deliberately just bytes: `testutil` never parses a packet. Slice 1's
/// golden-wire assertions and Appendix B's *"no further msg1 leaves the
/// endpoint after the drop"* obligation both read this, and both supply
/// their own meaning for the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spied {
    /// The sending wire's address.
    pub src: SocketAddr,
    /// The address the datagram was addressed to.
    pub dst: SocketAddr,
    /// The datagram, verbatim.
    pub bytes: Vec<u8>,
}

/// A view of every accepted send on a [`Network`].
///
/// All taps from one network share **one** log, so [`Tap::drain`] empties
/// it for every holder, and a tap taken *after* some sends still sees
/// them.
///
/// "Accepted" means the send was not refused by an injected send failure
/// and was not blackholed by a partition — it is what left the wire, which
/// is a different question from what arrived. Loss and duplication are
/// applied *after* the tap.
#[derive(Clone, Debug)]
pub struct Tap(Rc<RefCell<Vec<Spied>>>);

impl Tap {
    /// How many datagrams have been recorded.
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// Whether nothing has been recorded.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Take everything recorded so far, emptying the shared log.
    pub fn drain(&self) -> Vec<Spied> {
        self.0.borrow_mut().drain(..).collect()
    }

    /// Everything recorded so far as `(from, to, bytes)`, leaving the log
    /// intact.
    ///
    /// The tuple shape a flow test filters on;
    /// [`snapshot`](Tap::snapshot) is the same data as [`Spied`] values.
    pub fn datagrams(&self) -> Vec<(SocketAddr, SocketAddr, Vec<u8>)> {
        self.0
            .borrow()
            .iter()
            .map(|spied| (spied.src, spied.dst, spied.bytes.clone()))
            .collect()
    }

    /// Copy everything recorded so far, leaving the log intact.
    pub fn snapshot(&self) -> Vec<Spied> {
        self.0.borrow().clone()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Policy
// ═══════════════════════════════════════════════════════════════════════

/// An injected `send_to` failure, active until an instant.
///
/// The error handed to the caller is built from [`SendFailure::raw_os`]
/// when that is non-zero — so `raw_os_error()` is observable — and from
/// [`SendFailure::kind`] otherwise.
#[derive(Clone, Debug)]
pub struct SendFailure {
    /// The `io::ErrorKind` to report. Used directly only when `raw_os` is
    /// zero; otherwise it records what `raw_os` decodes to.
    pub kind: io::ErrorKind,
    /// The raw OS error to report, or `0` for "no errno, use `kind`".
    pub raw_os: i32,
    /// Sends fail while `Instant::now() < until`.
    pub until: Instant,
}

impl SendFailure {
    fn to_io_error(&self) -> io::Error {
        if self.raw_os != 0 {
            io::Error::from_raw_os_error(self.raw_os)
        } else {
            io::Error::from(self.kind)
        }
    }
}

/// How one [`FlakyWire`] mistreats the datagrams it sends.
///
/// Every field is public, so a test may build one literally; the
/// constructors below cover the shapes the spec's obligations ask for.
#[derive(Clone, Debug)]
pub struct FlakyPolicy {
    /// P(drop) per datagram, `0.0`–`1.0`.
    pub loss: f64,
    /// P(deliver a second copy), `0.0`–`1.0`.
    pub duplicate: f64,
    /// Minimum one-way delay.
    pub base_delay: Duration,
    /// Uniform additional delay in `[0, jitter)`.
    ///
    /// **Reordering lives here.** Two datagrams whose draws cross swap —
    /// which is how a real network reorders, composes with the paused
    /// clock for free, and means "reorder" needs no separate knob.
    pub jitter: Duration,
    /// Drop the first N datagrams unconditionally, then behave normally.
    pub drop_first: usize,
    /// Drop exactly these 0-based send indices. No RNG is involved.
    pub drop_at: BTreeSet<usize>,
    /// Fail `send_to` while `Instant::now()` is inside the window.
    pub send_failure: Option<SendFailure>,
    /// The shared toggle behind [`FlakyPolicy::fail_sends`].
    ///
    /// Private and `Rc`, so **clones share it**: a policy handed to
    /// [`Network::wire_with`] can still be switched from the test that
    /// built it, which is what Appendix B's "fails for a bounded interval,
    /// then heals" needs when the interval's end is not known in advance.
    failing: Rc<Cell<bool>>,
}

impl FlakyPolicy {
    /// No loss, no duplication, no delay.
    pub fn perfect() -> Self {
        FlakyPolicy {
            loss: 0.0,
            duplicate: 0.0,
            base_delay: Duration::ZERO,
            jitter: Duration::ZERO,
            drop_first: 0,
            drop_at: BTreeSet::new(),
            send_failure: None,
            failing: Rc::new(Cell::new(false)),
        }
    }

    /// Turn `ENETUNREACH` on every `send_to` on or off, **now**.
    ///
    /// The toggle is shared by every clone of this policy, so the usual
    /// shape works:
    ///
    /// ```
    /// # use slither::testutil::{FlakyPolicy, Network};
    /// # let net = Network::seeded(0);
    /// # let addr = "10.0.0.9:9".parse().expect("literal addr");
    /// let policy = FlakyPolicy::perfect();
    /// let wire = net.wire_with(addr, policy.clone());
    /// policy.fail_sends(true);   // every send from `wire` now fails
    /// policy.fail_sends(false);  // and the seam heals
    /// ```
    ///
    /// Ruling 49 makes a failing send a **trace** obligation and nothing
    /// more — no teardown, no verb resolved with an error — and §16.10
    /// makes the injector itself required rather than optional, because
    /// the obligation is unreachable without one.
    ///
    /// Independent of
    /// [`failing_sends_until`](FlakyPolicy::failing_sends_until): either
    /// being active fails the send.
    pub fn fail_sends(&self, failing: bool) {
        self.failing.set(failing);
    }

    /// Whether the toggle above is currently on.
    pub fn is_failing(&self) -> bool {
        self.failing.get()
    }

    /// Drop the first `n` datagrams, then deliver perfectly.
    ///
    /// Index-based, so the outcome does not depend on a draw — this is the
    /// tool for Appendix B's *"the first two msg1s die"*.
    pub fn drop_first(n: usize) -> Self {
        FlakyPolicy {
            drop_first: n,
            ..Self::perfect()
        }
    }

    /// Drop exactly these 0-based send indices, and no others.
    pub fn drop_at(indices: impl IntoIterator<Item = usize>) -> Self {
        FlakyPolicy {
            drop_at: indices.into_iter().collect(),
            ..Self::perfect()
        }
    }

    /// Drop each datagram with probability `rate`.
    ///
    /// Reproducible under the network seed, but brittle: the draw a given
    /// datagram sees moves when an unrelated send is added ahead of it.
    /// Prefer [`FlakyPolicy::drop_at`] when asserting a specific outcome.
    pub fn lossy(rate: f64) -> Self {
        FlakyPolicy {
            loss: rate,
            ..Self::perfect()
        }
    }

    /// Delay every delivery by `base`, plus a uniform draw in
    /// `[0, jitter)`.
    #[must_use]
    pub fn with_delay(self, base: Duration, jitter: Duration) -> Self {
        FlakyPolicy {
            base_delay: base,
            jitter,
            ..self
        }
    }

    /// Deliver a second identical copy with probability `rate`.
    #[must_use]
    pub fn with_duplication(self, rate: f64) -> Self {
        FlakyPolicy {
            duplicate: rate,
            ..self
        }
    }

    /// `send_to` returns `ENETUNREACH` until `until`, then heals.
    ///
    /// The ruling-49 / S25 fixture (Appendix B): *"Give the endpoint a
    /// `Wire` whose `send_to` returns `ENETUNREACH` for a bounded
    /// interval, then heals."* A failing send is a **trace** obligation on
    /// the driver, not a connection-killing event, and this is how that is
    /// tested.
    #[must_use]
    pub fn failing_sends_until(self, until: Instant) -> Self {
        FlakyPolicy {
            send_failure: Some(SendFailure {
                kind: io::ErrorKind::NetworkUnreachable,
                raw_os: ENETUNREACH,
                until,
            }),
            ..self
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The fabric
// ═══════════════════════════════════════════════════════════════════════

/// A datagram waiting in an inbox.
///
/// Ordered by `(deliver_at, seq)`. The sequence number breaks ties, which
/// keeps equal-deadline datagrams FIFO and, more importantly, keeps the
/// heap **deterministic**.
#[derive(Debug, PartialEq, Eq)]
struct Queued {
    deliver_at: Instant,
    seq: u64,
    src: SocketAddr,
    bytes: Vec<u8>,
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deliver_at
            .cmp(&other.deliver_at)
            .then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct EndpointState {
    /// A min-heap: `Reverse` inverts `BinaryHeap`'s max-heap order.
    inbox: BinaryHeap<Reverse<Queued>>,
    notify: Rc<Notify>,
}

struct Inner {
    seed: u64,
    /// Ordered, never iterated for a decision — but ordered anyway, so no
    /// hash seed can ever reach a decision path.
    endpoints: BTreeMap<SocketAddr, EndpointState>,
    partitioned: BTreeSet<SocketAddr>,
    blocked: BTreeSet<(SocketAddr, SocketAddr)>,
    log: Rc<RefCell<Vec<Spied>>>,
    /// Every `send_to` call, counted **before** any policy decision.
    sends: usize,
    /// Monotonic tie-breaker for the inboxes.
    seq: u64,
    /// Registration counter — explicit, so wire ordinals never depend on
    /// map order.
    ordinal: usize,
}

/// An in-memory datagram fabric: a set of addresses, each with an inbox,
/// and a policy per sending endpoint.
///
/// Single-threaded by construction (`Rc`, `RefCell`) — the same shape as
/// the `!Send` driver it feeds.
#[derive(Clone)]
pub struct Network(Rc<RefCell<Inner>>);

impl Default for Network {
    fn default() -> Self {
        Network::new()
    }
}

impl Network {
    /// A network with a fixed RNG seed.
    ///
    /// Every delivery decision is a function of this seed and the per-wire
    /// send order — **the same test yields the same drops, delays and
    /// duplicates on every run.**
    pub fn seeded(seed: u64) -> Network {
        Network(Rc::new(RefCell::new(Inner {
            seed,
            endpoints: BTreeMap::new(),
            partitioned: BTreeSet::new(),
            blocked: BTreeSet::new(),
            log: Rc::new(RefCell::new(Vec::new())),
            sends: 0,
            seq: 0,
            ordinal: 0,
        })))
    }

    /// `seeded(0)`.
    ///
    /// There is deliberately no OS-entropy constructor: a testutil whose
    /// failures are irreproducible is worse than no testutil (ruling 60).
    pub fn new() -> Network {
        Network::seeded(0)
    }

    /// Register `addr` and return the [`Wire`] bound to it.
    ///
    /// # Panics
    ///
    /// If `addr` is already registered.
    pub fn endpoint(&self, addr: SocketAddr) -> FlakyWire {
        let (ordinal, seed, notify) = {
            let mut inner = self.0.borrow_mut();
            assert!(
                !inner.endpoints.contains_key(&addr),
                "{addr} is already registered on this network"
            );
            let notify = Rc::new(Notify::new());
            inner.endpoints.insert(
                addr,
                EndpointState {
                    inbox: BinaryHeap::new(),
                    notify: Rc::clone(&notify),
                },
            );
            let ordinal = inner.ordinal;
            inner.ordinal += 1;
            (ordinal, inner.seed, notify)
        };

        // One stream per wire, decorrelated by ordinal: adding a third
        // endpoint to a test does not reshuffle the first two's draws.
        let wire_seed = seed ^ (ordinal as u64).wrapping_mul(SEED_STRIDE);

        FlakyWire {
            addr: Cell::new(addr),
            net: Rc::clone(&self.0),
            rng: RefCell::new(ChaCha20Rng::seed_from_u64(wire_seed)),
            policy: RefCell::new(FlakyPolicy::perfect()),
            sent: Cell::new(0),
            notify,
        }
    }

    /// [`endpoint`](Network::endpoint), under the name a flow test reads
    /// better with: `net.wire(addr)` is the wire, `net.endpoint(addr)` is
    /// the registration. One function, two readings.
    ///
    /// # Panics
    ///
    /// If `addr` is already registered.
    pub fn wire(&self, addr: SocketAddr) -> FlakyWire {
        self.endpoint(addr)
    }

    /// [`wire`](Network::wire) with `policy` installed before the first
    /// send.
    ///
    /// The caller's copy keeps working: see [`FlakyPolicy::fail_sends`],
    /// whose toggle is shared across clones, so a test can switch the seam
    /// mid-run without ever reaching the wire again.
    ///
    /// # Panics
    ///
    /// If `addr` is already registered.
    pub fn wire_with(&self, addr: SocketAddr, policy: FlakyPolicy) -> FlakyWire {
        let wire = self.endpoint(addr);
        wire.set_policy(policy);
        wire
    }

    /// Blackhole `addr` in both directions: it sends and receives nothing.
    pub fn partition(&self, addr: SocketAddr) {
        self.0.borrow_mut().partitioned.insert(addr);
    }

    /// Undo [`Network::partition`].
    pub fn heal(&self, addr: SocketAddr) {
        self.0.borrow_mut().partitioned.remove(&addr);
    }

    /// Blackhole the path `from → to`, leaving `to → from` alone.
    pub fn block_path(&self, from: SocketAddr, to: SocketAddr) {
        self.0.borrow_mut().blocked.insert((from, to));
    }

    /// Undo [`Network::block_path`].
    pub fn heal_path(&self, from: SocketAddr, to: SocketAddr) {
        self.0.borrow_mut().blocked.remove(&(from, to));
    }

    /// A view of every accepted send. All taps share one log.
    pub fn tap(&self) -> Tap {
        Tap(Rc::clone(&self.0.borrow().log))
    }

    /// Total `send_to` calls, counted before any policy decision — so
    /// drops, blackholes and injected send failures are all included.
    ///
    /// This is what distinguishes *"we tried to send"* from *"it
    /// arrived"*, which is what ruling 50's *"no further msg1 leaves the
    /// endpoint"* assertion needs.
    pub fn sends(&self) -> usize {
        self.0.borrow().sends
    }

    /// Deliver a datagram that appears to come from `from` — an address no
    /// [`FlakyWire`] need own.
    ///
    /// The forgery fixture: mac1 garbage, off-path spoofing, and a source
    /// address that never sent anything. No policy applies, nothing is
    /// counted in [`Network::sends`], and nothing is tapped — this is not
    /// a send by any wire. Delivery is immediate.
    ///
    /// A datagram for an unregistered or partitioned `to` is dropped, as
    /// it would be by [`FlakyWire::send_to`].
    pub fn inject(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) {
        let mut inner = self.0.borrow_mut();
        let now = Instant::now();
        deliver(&mut inner, from, to, bytes.to_vec(), now);
    }
}

/// Push one datagram into `to`'s inbox, or drop it.
fn deliver(inner: &mut Inner, src: SocketAddr, dst: SocketAddr, bytes: Vec<u8>, at: Instant) {
    if inner.partitioned.contains(&dst) {
        return;
    }
    let seq = inner.seq;
    inner.seq += 1;
    // An unregistered destination drops the datagram: a real socket would
    // get an ICMP port-unreachable at best, and slither ignores those.
    let Some(ep) = inner.endpoints.get_mut(&dst) else {
        return;
    };
    ep.inbox.push(Reverse(Queued {
        deliver_at: at,
        seq,
        src,
        bytes,
    }));
    ep.notify.notify_waiters();
}

// ═══════════════════════════════════════════════════════════════════════
// The wire
// ═══════════════════════════════════════════════════════════════════════

/// The in-memory [`Wire`] (§16.3, §16.10) — the implementation the
/// paused-clock flow tests ride.
///
/// Both `Wire` methods take `&self` (§16.3 property 3), so every mutable
/// field sits behind a `Cell`/`RefCell`. `FlakyWire` is deliberately
/// **`!Send`**: it is the fixture for an actor that must not require
/// `Send`, so a `Send` `FlakyWire` would let a `Send` bound creep into the
/// driver unnoticed.
pub struct FlakyWire {
    /// **[ruling 180]** A `Cell`, not a plain field, because
    /// [`rebind`](FlakyWire::rebind) moves this wire between addresses and
    /// every `Wire` method takes `&self`. Before ruling 180 this was
    /// immutable and **stories S18 and S19 were unreachable by
    /// construction** — nothing could make an endpoint originate from a new
    /// address, so a roam could only ever be simulated by
    /// [`Network::inject`]'s spoofed source, which §7.2's replay window
    /// rejects.
    addr: Cell<SocketAddr>,
    net: Rc<RefCell<Inner>>,
    rng: RefCell<ChaCha20Rng>,
    policy: RefCell<FlakyPolicy>,
    /// This wire's 0-based send index, for `drop_at` / `drop_first`.
    sent: Cell<usize>,
    notify: Rc<Notify>,
}

impl FlakyWire {
    /// The address this wire is registered at.
    ///
    /// Follows [`rebind`](FlakyWire::rebind).
    pub fn local_addr(&self) -> SocketAddr {
        self.addr.get()
    }

    /// **[RATIFIED 2026/08/16 — ruling 180]** Move this wire to
    /// `new_addr`: an interface change, or a NAT rebind.
    ///
    /// This is the fixture half of §7.3's roaming. The peer re-homes on the
    /// next authenticated, window-fresh packet we send — roaming is driven
    /// by **authenticated receipt**, so the mover must send, and S18 makes
    /// that a positive obligation rather than a transport probe.
    ///
    /// **In-flight datagrams addressed to the old address are dropped.**
    /// The new address gets a fresh, empty inbox. That models what an
    /// interface change and a NAT rebind actually do, and it is what makes
    /// S18's obligation bite: were the inbox carried across, a peer could
    /// move, stay silent, and still receive — so "a peer that moves and
    /// stays silent is indistinguishable from one that vanished" would pass
    /// for the wrong reason, which is a bound the degenerate implementation
    /// satisfies for free.
    ///
    /// Datagrams sent to the old address *after* the rebind need no special
    /// handling: an unregistered destination is already dropped, exactly as
    /// a real socket's mapping would.
    ///
    /// The `Notify` is **carried, not replaced** — a driver's receive loop
    /// is parked on this wire's existing handle, and a fresh one would park
    /// it for ever.
    ///
    /// # Panics
    ///
    /// If `new_addr` is already registered on this network. Rebinding to
    /// the address a wire already holds is a no-op, not a panic.
    pub fn rebind(&self, new_addr: SocketAddr) {
        let old = self.addr.get();
        if old == new_addr {
            return;
        }
        {
            let mut inner = self.net.borrow_mut();
            assert!(
                !inner.endpoints.contains_key(&new_addr),
                "{new_addr} is already registered on this network"
            );
            // The old inbox is dropped with the entry; only the notify
            // handle crosses, because the driver is parked on it.
            inner.endpoints.remove(&old);
            inner.endpoints.insert(
                new_addr,
                EndpointState {
                    inbox: BinaryHeap::new(),
                    notify: Rc::clone(&self.notify),
                },
            );
            // A partition or a blocked path is a property of the *address*,
            // not of the wire: a rebind lands on a fresh address, which is
            // by definition neither partitioned nor blocked. Anything the
            // test wants to hold across the move it re-applies to the new
            // address.
            inner.partitioned.remove(&old);
            inner
                .blocked
                .retain(|(from, to)| *from != old && *to != old);
        }
        self.addr.set(new_addr);
        // Anything parked on the old inbox must re-examine the new one.
        self.notify.notify_waiters();
    }

    /// Replace this wire's policy. Takes effect from the next send.
    ///
    /// The send index is **not** reset: `drop_at` indices count every send
    /// this wire has made, so a policy installed mid-run does not silently
    /// renumber history.
    pub fn set_policy(&self, policy: FlakyPolicy) {
        *self.policy.borrow_mut() = policy;
    }

    /// A clone of this wire's policy.
    pub fn policy(&self) -> FlakyPolicy {
        self.policy.borrow().clone()
    }

    /// A uniform draw in `[0, 1)` from this wire's stream.
    fn draw_unit(&self) -> f64 {
        // 53 bits of mantissa, the standard construction.
        let bits = self.rng.borrow_mut().next_u64() >> 11;
        bits as f64 / (1u64 << 53) as f64
    }

    /// One delay draw: `base_delay + uniform[0, jitter)`.
    ///
    /// The draw is taken **even when `jitter` is zero**, so a delivery
    /// always consumes exactly one `u64` from the stream.
    fn draw_delay(&self, policy: &FlakyPolicy) -> Duration {
        let raw = self.rng.borrow_mut().next_u64();
        let jitter_ns = policy.jitter.as_nanos() as u64;
        if jitter_ns == 0 {
            policy.base_delay
        } else {
            policy.base_delay + Duration::from_nanos(raw % jitter_ns)
        }
    }
}

impl Wire for FlakyWire {
    /// Send `buf` to `addr`, applying this wire's [`FlakyPolicy`].
    ///
    /// # The routing, step by step
    ///
    /// 1. Count the send — **before** any policy decision.
    /// 2. If a send failure is active for `now`, return `Err`. Nothing is
    ///    queued and **nothing is tapped**.
    /// 3. If this wire is partitioned, or the path to `addr` is blocked,
    ///    return `Ok(buf.len())` and drop silently. **A blackhole is not a
    ///    send error**: conflating the two would make the ruling-49
    ///    fixture untestable, because a test could not tell an injected
    ///    `ENETUNREACH` from a topology change.
    /// 4. Record in the tap.
    /// 5. Decide how many copies to deliver: 0 (lost), 1, or 2
    ///    (duplicated).
    /// 6. Draw a delay per copy and queue it.
    ///
    /// # The draw order is contract
    ///
    /// Changing it changes every seeded test's outcome, so it is part of
    /// the determinism contract rather than an implementation detail:
    ///
    /// 1. `drop_at` / `drop_first` — index-based, **no draw at all**;
    /// 2. one `f64` draw for `loss`;
    /// 3. one `f64` draw for `duplicate` — taken even when the loss draw
    ///    has already decided the outcome, so a non-index-dropped send
    ///    always consumes exactly two `f64` draws;
    /// 4. one `u64` draw per delivery for `jitter`.
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let index = self.sent.get();
        self.sent.set(index + 1);
        self.net.borrow_mut().sends += 1;

        // **[ruling 180]** Read once: one send leaves from one source, even
        // if a `rebind` lands between two sends.
        let src = self.addr.get();

        let policy = self.policy.borrow().clone();

        // 2. Injected send failure — the timed window, or the toggle.
        if let Some(failure) = &policy.send_failure
            && Instant::now() < failure.until
        {
            return Err(failure.to_io_error());
        }
        if policy.is_failing() {
            return Err(io::Error::from_raw_os_error(ENETUNREACH));
        }

        // 3. Topology. A blackhole, not an error.
        {
            let net = self.net.borrow();
            if net.partitioned.contains(&src) || net.blocked.contains(&(src, addr)) {
                return Ok(buf.len());
            }
        }

        // 4. Tap.
        self.net.borrow().log.borrow_mut().push(Spied {
            src,
            dst: addr,
            bytes: buf.to_vec(),
        });

        // 5. Deliveries.
        let deliveries = if index < policy.drop_first || policy.drop_at.contains(&index) {
            0
        } else {
            let lost = self.draw_unit() < policy.loss;
            let duplicated = self.draw_unit() < policy.duplicate;
            match (lost, duplicated) {
                (true, _) => 0,
                (false, false) => 1,
                (false, true) => 2,
            }
        };

        // 6. Queue.
        let now = Instant::now();
        for _ in 0..deliveries {
            let delay = self.draw_delay(&policy);
            let mut net = self.net.borrow_mut();
            deliver(&mut net, src, addr, buf.to_vec(), now + delay);
        }

        Ok(buf.len())
    }

    /// Receive one datagram, waiting out its injected delay in virtual
    /// time.
    ///
    /// # Cancel safety
    ///
    /// **The datagram is popped after the sleep, never before.** Dropping
    /// this future loses nothing: the datagram stays in the inbox and the
    /// next call gets it. That is not optional — the driver `select!`s
    /// `recv_from` against its command channel and its timer, so a dropped
    /// future must lose nothing.
    ///
    /// # Truncation
    ///
    /// A datagram longer than `buf` is copied to `buf.len()` bytes and
    /// that count is returned, matching `tokio::net::UdpSocket::recv_from`
    /// (POSIX `recvfrom` without `MSG_TRUNC`). The driver's buffer is
    /// `MAX_DATAGRAM`, so an oversize datagram arrives truncated and dies
    /// at §3.5's length gate.
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        loop {
            // **[ruling 180]** Re-read every iteration, never hoisted: a
            // `rebind` can land while we are parked below, and after one the
            // inbox we must examine is the **new** address's. Anything still
            // queued for the old address is gone with it, which is what a
            // NAT rebind does to a datagram already in flight.
            let me = self.addr.get();
            let next = {
                let net = self.net.borrow();
                net.endpoints
                    .get(&me)
                    .and_then(|ep| ep.inbox.peek().map(|Reverse(q)| q.deliver_at))
            };

            let Some(deliver_at) = next else {
                // Nothing queued. There is no yield point between the peek
                // above and this await, and the fabric is single-threaded,
                // so no wakeup can be missed in the gap.
                self.notify.notified().await;
                continue;
            };

            // THE line that makes §16.10 work: on the paused clock this
            // auto-advances virtual time once every task is idle.
            tokio::time::sleep_until(deliver_at).await;

            // Only now do we take it.
            let mut net = self.net.borrow_mut();
            // `self.addr`, not `me`: a rebind during the sleep above moves
            // us, and the datagram we waited for was addressed to where we
            // no longer are.
            let Some(ep) = net.endpoints.get_mut(&self.addr.get()) else {
                continue;
            };
            let due = ep
                .inbox
                .peek()
                .is_some_and(|Reverse(q)| q.deliver_at <= Instant::now());
            if !due {
                continue;
            }
            let Reverse(queued) = ep.inbox.pop().expect("peeked above");
            let n = queued.bytes.len().min(buf.len());
            buf[..n].copy_from_slice(&queued.bytes[..n]);
            return Ok((n, queued.src));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The counting DH provider
// ═══════════════════════════════════════════════════════════════════════

/// A shared, cheap handle on a DH call count.
///
/// `Rc<Cell<_>>`, so it is `!Send` like everything else on the actor path.
/// Clones observe the same count.
#[derive(Clone, Debug, Default)]
pub struct DhCounter(Rc<Cell<u32>>);

impl DhCounter {
    /// A counter at zero.
    pub fn new() -> Self {
        DhCounter(Rc::new(Cell::new(0)))
    }

    /// The number of `dh` calls observed.
    pub fn get(&self) -> u32 {
        self.0.get()
    }

    /// Set the count back to zero.
    pub fn reset(&self) {
        self.0.set(0);
    }

    /// Wrap `inner` so its `dh` calls land on this counter.
    pub fn provider<P>(&self, inner: P) -> CountingProvider<P> {
        CountingProvider {
            inner,
            dhs: Rc::clone(&self.0),
        }
    }
}

/// Wraps a `DhProvider<P256>` and counts every `dh` call.
///
/// **Exactly one increment per `DhProvider::dh` call, and nothing else
/// counts** — key generation is not a DH. That has to be exact, because
/// §6.1's whole design argument is *"one DH to inspect, two to
/// authenticate"*, and the ladder assertions later slices make (`0` at
/// park, `1` after `read_identity()`, `2` after `authenticate()`, `4` per
/// cancel-and-redial cycle) are how it is enforced.
pub struct CountingProvider<P> {
    inner: P,
    dhs: Rc<Cell<u32>>,
}

impl<P> CountingProvider<P> {
    /// The wrapped provider.
    pub fn inner(&self) -> &P {
        &self.inner
    }
}

impl<P: CryptoKeyProvider<P256>> CryptoKeyProvider<P256> for CountingProvider<P> {
    type Error = P::Error;
    type PrivateKey = P::PrivateKey;

    fn public_key(
        &self,
        key: &Self::PrivateKey,
    ) -> Result<<P256 as Curve>::PublicKey, Self::Error> {
        self.inner.public_key(key)
    }

    fn generate_static_key(&mut self) -> Result<Self::PrivateKey, Self::Error> {
        self.inner.generate_static_key()
    }

    fn generate_ephemeral_key(&mut self) -> Result<Self::PrivateKey, Self::Error> {
        self.inner.generate_ephemeral_key()
    }
}

impl<P: DhProvider<P256>> DhProvider<P256> for CountingProvider<P> {
    fn dh(
        &self,
        key: &Self::PrivateKey,
        peer: &<P256 as Curve>::PublicKey,
    ) -> Result<<P256 as DhCurve>::SharedSecret, Self::Error> {
        self.dhs.set(self.dhs.get() + 1);
        self.inner.dh(key, peer)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The counting identity
// ═══════════════════════════════════════════════════════════════════════

/// A [`SoftwareIdentity`](crate::identity::SoftwareIdentity) whose every handshake runs on a
/// [`CountingProvider`] sharing **one** [`DhCounter`].
///
/// The count is therefore **endpoint-wide and cumulative across
/// handshakes**, which is exactly what §6.1's ladder prices: 0 DH at park,
/// 1 after `read_identity()`, 2 after `authenticate()`, 4 after `accept()`.
///
/// # It is `!Send`, and that is the point (S21)
///
/// [`DhCounter`] holds an `Rc<Cell<_>>`, so this identity and every
/// provider it mints are `!Send`. An endpoint driven over it is a
/// **compile-time proof** that no `Send` bound sits anywhere on the DH
/// path: add one and the tests stop compiling, which reddens the build and
/// the test gate together. That is the cheapest available enforcement of
/// the requirement an iOS Secure Enclave key exists to state.
pub struct CountingIdentity<S = crate::packet::ReferenceSuite> {
    inner: crate::identity::SoftwareIdentity<S, ChaCha20Rng>,
    dhs: DhCounter,
}

impl<S> CountingIdentity<S> {
    /// A fresh identity whose static key and per-handshake sub-seeds are
    /// derived from `seed`, so a whole endpoint is replayable.
    pub fn seeded(seed: [u8; 32]) -> Self {
        let inner = crate::identity::SoftwareIdentity::generate(ChaCha20Rng::from_seed(seed))
            .expect("a seeded ChaCha20 stream yields a valid P-256 scalar");
        Self {
            inner,
            dhs: DhCounter::new(),
        }
    }

    /// The shared DH call count. Cheap to clone; clones observe the same
    /// count.
    pub fn counter(&self) -> DhCounter {
        self.dhs.clone()
    }

    /// The number of `dh` calls this identity has performed.
    pub fn dhs(&self) -> u32 {
        self.dhs.get()
    }
}

impl<S> crate::identity::Identity for CountingIdentity<S>
where
    S: crate::packet::Handshake<Curve = P256>,
{
    type Suite = S;
    type Provider = CountingProvider<hiss::provider::EphemeralOnly<ChaCha20Rng>>;
    type Error = crate::identity::SoftwareIdentityError;

    fn public_static(&self) -> &<P256 as Curve>::PublicKey {
        self.inner.public_static()
    }

    fn open(
        &self,
    ) -> Result<
        (
            Self::Provider,
            <Self::Provider as CryptoKeyProvider<P256>>::PrivateKey,
        ),
        Self::Error,
    > {
        let (provider, key) = self.inner.open()?;
        Ok((self.dhs.provider(provider), key))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The shell harness — two endpoints, one network, a paused clock
// ═══════════════════════════════════════════════════════════════════════

/// The identity every fixture endpoint runs on: [`CountingIdentity`] over
/// the reference suite, so DH cost is observable and nothing on the path is
/// `Send`.
pub type TestIdentity = CountingIdentity<crate::packet::ReferenceSuite>;

/// A fixture endpoint handle.
pub type TestEndpoint = crate::shell::Endpoint<TestIdentity>;

/// A fixture connection handle.
pub type TestConnection = crate::shell::Connection<crate::packet::ReferenceSuite>;

/// A fixture send half.
pub type TestSendStream = crate::shell::SendStream<crate::packet::ReferenceSuite>;

/// A fixture receive half.
pub type TestRecvStream = crate::shell::RecvStream<crate::packet::ReferenceSuite>;

/// A fixture bidirectional stream.
pub type TestBiStream = crate::shell::BiStream<crate::packet::ReferenceSuite>;

/// A fixture stage-0 introduction.
pub type TestIntro = crate::shell::Intro<TestIdentity>;

/// A fixture `connect()` future.
pub type TestConnecting = crate::shell::Connecting<TestIdentity>;

/// The static public key type the fixture uses.
pub type TestPublicKey = crate::identity::PublicKeyOf<TestIdentity>;

/// A cheap shared handle on one [`FlakyWire`].
///
/// [`crate::shell::EndpointBuilder::wire`] takes the wire **by value**, so
/// without this a test could never reach the wire again to change its
/// [`FlakyPolicy`] mid-run — which is what ruling 49's send-failure
/// obligation needs. Cloning shares one wire: the send index, the RNG
/// stream and the policy are all the same, so `drop_at` indices keep
/// counting every send exactly once.
#[derive(Clone)]
pub struct SharedWire(Rc<FlakyWire>);

impl SharedWire {
    /// The address this wire is registered at.
    pub fn local_addr(&self) -> SocketAddr {
        self.0.local_addr()
    }

    /// **[ruling 180]** Move this wire to `to`. See [`FlakyWire::rebind`].
    ///
    /// Every clone of this `SharedWire` observes the move — the address
    /// lives in one `Cell` behind the shared `Rc`, not in the handle — so
    /// the driver holding its own clone sends from the new address on its
    /// very next send, with no re-plumbing.
    pub fn rebind(&self, to: SocketAddr) {
        self.0.rebind(to);
    }

    /// Replace the policy. Takes effect from the next send; the send index
    /// is not reset.
    pub fn set_policy(&self, policy: FlakyPolicy) {
        self.0.set_policy(policy);
    }

    /// A clone of the current policy.
    pub fn policy(&self) -> FlakyPolicy {
        self.0.policy()
    }
}

impl Wire for SharedWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        self.0.send_to(buf, addr).await
    }

    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.recv_from(buf).await
    }
}

/// One endpoint of a [`Pair`], with everything a flow test asserts on.
///
/// No `Drop` impl, and the fields are `pub`, so a test can move the
/// `endpoint` out and drop it on its own — which is exactly what S26's
/// drop-order assertions need.
pub struct Peer {
    /// The handle under test. Dropping it stops that endpoint's driver
    /// **only if** no `Connecting` or `Connection` of its own is still
    /// alive (§16.3).
    pub endpoint: TestEndpoint,
    /// A shared handle on its wire, for mid-run [`FlakyPolicy`] changes.
    pub wire: SharedWire,
    /// Its cumulative DH count (§6.1 prices the ladder cumulatively).
    pub dhs: DhCounter,
    /// Its static public key — what the *other* peer dials.
    pub public_static: TestPublicKey,
}

impl Peer {
    /// Where this endpoint lives on the [`Network`] **right now**.
    ///
    /// **[ruling 180]** This was a `pub addr` field until slice 7. A field
    /// could not follow [`rebind`](Peer::rebind), so after a move it would
    /// hold the address this peer *used to* have — and every mobility test
    /// is precisely a test about which address is current. Reading a stale
    /// one would not fail loudly; it would assert the pre-move address and
    /// pass. The field is gone rather than kept-and-deprecated so that no
    /// call site can read the wrong thing.
    pub fn addr(&self) -> SocketAddr {
        self.wire.local_addr()
    }

    /// Move this endpoint to `to` — an interface change, or a NAT rebind.
    ///
    /// The peer at the other end re-homes on our next authenticated,
    /// window-fresh packet (§7.3). Roaming is **receive-driven**, so
    /// nothing happens until we send: that is S18's positive obligation on
    /// the mover, and [`FlakyWire::rebind`] documents why in-flight
    /// datagrams to the old address are abandoned rather than carried.
    ///
    /// # Panics
    ///
    /// If `to` is already registered on the network.
    pub fn rebind(&self, to: SocketAddr) {
        self.wire.rebind(to);
    }
}

/// Two endpoints on one in-memory [`Network`] — §16.10's kernel-free
/// fixture, with drivers.
///
/// # Use it inside a `LocalSet`, on a paused clock
///
/// The shell is a `!Send` actor spawned with `tokio::task::spawn_local`
/// (§16.3), so construction **panics outside a `LocalSet`**. [`local`] is
/// the wrapper that supplies one:
///
/// ```no_run
/// # use slither::testutil::{Pair, local};
/// #[tokio::test(start_paused = true)]
/// async fn a_flow_test() {
///     local(async {
///         let pair = Pair::seeded(0xC0FFEE);
///         let (a, b) = pair.establish().await;
///         a.close(0, b"bye").await;
///         assert!(matches!(
///             b.closed().await,
///             slither::ConnectionLost::PeerClosed { .. }
///         ));
///     })
///     .await;
/// }
/// ```
///
/// # Determinism
///
/// Everything is derived from the one seed: the network's loss and delay
/// draws, both endpoints' §16.6 RNGs, and both static keys. Ruling 60 makes
/// that a **MUST**, so there is no OS-entropy path here.
pub struct Pair {
    /// The fabric. `tap()`, `sends()`, `partition()` and friends live here.
    pub net: Network,
    /// The endpoint at `10.0.0.1:4001`. By convention the **dialler**.
    pub a: Peer,
    /// The endpoint at `10.0.0.2:4002`. By convention the **responder**.
    pub b: Peer,
}

impl Pair {
    /// Two endpoints at `10.0.0.1:4001` and `10.0.0.2:4002`, all draws
    /// derived from `seed`, both on [`Config::new`].
    ///
    /// # Panics
    ///
    /// Outside a `tokio::task::LocalSet`.
    ///
    /// [`Config::new`]: crate::config::Config::new
    pub fn seeded(seed: u64) -> Pair {
        Pair::seeded_with(seed, crate::config::Config::new())
    }

    /// [`seeded`](Pair::seeded) with a `Config` both endpoints share.
    ///
    /// # Panics
    ///
    /// Outside a `tokio::task::LocalSet`.
    pub fn seeded_with(seed: u64, config: crate::config::Config) -> Pair {
        let net = Network::seeded(seed);
        let a = Peer::spawn(&net, addr_a(), seed, 0xA1, config.clone());
        let b = Peer::spawn(&net, addr_b(), seed, 0xB2, config);
        Pair { net, a, b }
    }

    /// Dial `a` → `b` and drive §6.2's staged accept to completion,
    /// returning both connections.
    ///
    /// The full 4-DH ladder, climbed the way an application climbs it:
    /// `accept()` → `read_identity()` → `authenticate()` → `accept()`, run
    /// concurrently with the dial so the paused clock advances. Nothing is
    /// hidden — [`Peer::dhs`] still shows what each stage cost.
    ///
    /// # Panics
    ///
    /// If either side fails to establish.
    pub async fn establish(&self) -> (TestConnection, TestConnection) {
        let dial = async {
            self.a
                .endpoint
                .connect(self.b.addr(), self.b.public_static)
                .expect("connect")
                .await
                .expect("the dial completed")
        };
        let accept = async {
            let intro = self.b.endpoint.accept().await.expect("an introduction");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");
            proven.accept().await.expect("accept")
        };
        tokio::join!(dial, accept)
    }
}

impl Peer {
    fn spawn(
        net: &Network,
        addr: SocketAddr,
        seed: u64,
        salt: u8,
        config: crate::config::Config,
    ) -> Peer {
        let wire = SharedWire(Rc::new(net.endpoint(addr)));
        let identity: TestIdentity = CountingIdentity::seeded(derive_seed(seed, salt));
        let dhs = identity.counter();
        let public_static = *crate::identity::Identity::public_static(&identity);
        let endpoint = crate::shell::Endpoint::builder()
            .identity(identity)
            .wire(wire.clone())
            .config(config)
            .rng_seed(derive_seed(seed, salt ^ 0xFF))
            .build();
        Peer {
            endpoint,
            wire,
            dhs,
            public_static,
        }
    }
}

/// The `a` endpoint's address.
pub fn addr_a() -> SocketAddr {
    "10.0.0.1:4001".parse().expect("literal addr")
}

/// The `b` endpoint's address.
pub fn addr_b() -> SocketAddr {
    "10.0.0.2:4002".parse().expect("literal addr")
}

/// A third address, for the tests that need one that is not `a` or `b`.
pub fn addr_c() -> SocketAddr {
    "10.0.0.3:4003".parse().expect("literal addr")
}

/// Run `body` inside a `tokio::task::LocalSet`.
///
/// The shell is a `!Send` actor (§16.3), so every flow test needs one. Pair
/// it with `#[tokio::test(start_paused = true)]` and every timer in the
/// spec — the 5 s / 10 s / 15 s / 25 s / 25 ms / 90 s family — resolves in
/// virtual time (§16.10).
pub async fn local<F: std::future::Future>(body: F) -> F::Output {
    tokio::task::LocalSet::new().run_until(body).await
}

/// Yield until the drivers have nothing left to do **at the current
/// instant**.
///
/// The shell resolves `close()` as soon as the CLOSE is *sealed* (§16.2) —
/// not once it has left the wire — so a test that closes and then asserts
/// on the peer, or on [`Network::sends`], has to give both drivers a turn
/// first. Awaiting anything does that; this is the spelling for when there
/// is nothing else to await.
///
/// It advances **no** virtual time, deliberately: a test that needs a timer
/// to fire should say so with `tokio::time::advance`, so the deadline it
/// depends on is visible in the test rather than hidden in a fixture.
pub async fn settle() {
    for _ in 0..SETTLE_YIELDS {
        tokio::task::yield_now().await;
    }
}

/// Enough turns for a datagram to cross the fabric and be answered several
/// times over, and cheap: a yield on a current-thread runtime is a queue
/// push.
const SETTLE_YIELDS: usize = 64;

/// One 32-byte seed from the network seed and a per-endpoint salt.
fn derive_seed(seed: u64, salt: u8) -> [u8; 32] {
    let mut out = [salt; 32];
    out[..8].copy_from_slice(&seed.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every success-path receive in this module goes through this.
    ///
    /// A bare `recv_from().await` **hangs** when routing, a send path, or
    /// cancel-safety is broken, so the regression arrives as a CI timeout
    /// minutes later rather than as a red test you can bisect. Slice 0's
    /// mutation review found three separate breakages — misrouted
    /// delivery, a stubbed `send_to`, and a cancel-safety violation —
    /// that were caught *only* as a hang.
    ///
    /// The timeout is generous and in virtual time under
    /// `start_paused = true`, so it costs nothing on the happy path.
    async fn recv_ok(wire: &FlakyWire, buf: &mut [u8]) -> (usize, std::net::SocketAddr) {
        tokio::time::timeout(Duration::from_secs(5), wire.recv_from(buf))
            .await
            .expect("recv_from hung: routing, send path or cancel-safety is broken")
            .expect("recv")
    }

    fn addr(n: u8) -> SocketAddr {
        format!("10.0.0.{n}:400{n}").parse().expect("literal addr")
    }

    /// **The slice-0 definition-of-done test.**
    ///
    /// Four things it proves: a byte crosses A → B through the [`Wire`]
    /// trait over a [`FlakyWire`]; an injected loss policy really drops,
    /// deterministically; **virtual time advances** (the `elapsed`
    /// assertion fails if `recv_from` busy-waits or the `sleep_until` is
    /// missing); and `timeout` composes with `recv_from`, which is the
    /// cancellation property the driver's `select!` will rely on.
    #[tokio::test(start_paused = true)]
    async fn a_byte_crosses_two_flaky_wires_under_injected_loss() {
        let wall_t0 = std::time::Instant::now();

        let net = Network::seeded(0xA11CE);
        let a = net.endpoint("10.0.0.1:4001".parse().expect("literal addr"));
        let b = net.endpoint("10.0.0.2:4002".parse().expect("literal addr"));

        // The first two datagrams die; the third gets through. Index-based,
        // so the outcome does not depend on an RNG draw.
        a.set_policy(
            FlakyPolicy::drop_at([0, 1])
                .with_delay(Duration::from_millis(50), Duration::from_millis(10)),
        );

        let t0 = Instant::now();
        for _ in 0..3 {
            a.send_to(b"!", b.local_addr()).await.expect("send");
        }

        let mut buf = [0u8; 1200];
        let (n, src) = recv_ok(&b, &mut buf).await;

        assert_eq!(&buf[..n], b"!");
        assert_eq!(src, a.local_addr());
        assert_eq!(net.sends(), 3, "all three left the wire");
        assert_eq!(
            net.tap().len(),
            3,
            "all three were tapped; two were dropped after"
        );

        // Virtual time advanced by the injected one-way delay …
        let elapsed = Instant::now() - t0;
        assert!(
            (Duration::from_millis(50)..Duration::from_millis(60)).contains(&elapsed),
            "delivery waited base_delay + jitter in virtual time, got {elapsed:?}",
        );

        // … and nothing else is coming.
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "the two dropped datagrams never arrive",
        );

        // A regression to real sleeps should be loud, not slow.
        assert!(
            wall_t0.elapsed() < Duration::from_secs(1),
            "the test spent real time; virtual time is not being used",
        );
    }

    #[tokio::test(start_paused = true)]
    async fn perfect_policy_delivers_everything_in_order() {
        let net = Network::seeded(1);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        a.set_policy(FlakyPolicy::perfect());

        for i in 0..100u8 {
            a.send_to(&[i], b.local_addr()).await.expect("send");
        }

        let mut buf = [0u8; 16];
        for i in 0..100u8 {
            let (n, src) = recv_ok(&b, &mut buf).await;
            assert_eq!(n, 1);
            assert_eq!(buf[0], i, "datagram {i} arrived out of order");
            assert_eq!(src, a.local_addr());
        }
        assert_eq!(net.sends(), 100);
    }

    /// A trace of one seeded scenario: what arrived, in what order, and at
    /// what offset from the start.
    async fn trace(seed: u64) -> Vec<(Vec<u8>, Duration)> {
        let net = Network::seeded(seed);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        a.set_policy(
            FlakyPolicy::lossy(0.3)
                .with_duplication(0.1)
                .with_delay(Duration::ZERO, Duration::from_millis(20)),
        );

        let t0 = Instant::now();
        for i in 0..50u8 {
            a.send_to(&[i], b.local_addr()).await.expect("send");
        }

        let mut out = Vec::new();
        let mut buf = [0u8; 16];
        // Drain until nothing more can arrive: 20 ms of jitter is the
        // whole horizon, so a 1 s timeout is generous.
        while let Ok(Ok((n, _src))) =
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf)).await
        {
            out.push((buf[..n].to_vec(), Instant::now() - t0));
        }
        out
    }

    /// **The reproducibility contract** (ruling 60), as a test rather than
    /// a comment.
    #[tokio::test(start_paused = true)]
    async fn seeded_runs_are_identical() {
        let first = trace(7).await;
        let second = trace(7).await;
        assert!(!first.is_empty(), "the scenario delivered nothing at all");
        assert_eq!(first, second, "the same seed produced a different trace");
    }

    /// Guards against a policy that silently ignores the RNG: a `loss`
    /// field that is never read would pass `seeded_runs_are_identical` and
    /// fail this.
    #[tokio::test(start_paused = true)]
    async fn different_seeds_diverge() {
        let first = trace(7).await;
        let second = trace(8).await;
        assert_ne!(first, second, "two seeds produced identical traces");
    }

    /// §8.2's pop-after-sleep rule, mechanically enforced. Getting this
    /// backwards is the single most likely silent-data-loss bug in the
    /// slice, and it would present two slices later as an unreproducible
    /// flake.
    #[tokio::test(start_paused = true)]
    async fn recv_from_is_cancel_safe() {
        let net = Network::seeded(3);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        a.set_policy(FlakyPolicy::perfect().with_delay(Duration::from_millis(50), Duration::ZERO));

        a.send_to(b"payload", b.local_addr()).await.expect("send");

        // Poll a `recv_from`, then drop it well before `deliver_at`.
        let mut buf = [0u8; 32];
        assert!(
            tokio::time::timeout(Duration::from_millis(10), b.recv_from(&mut buf))
                .await
                .is_err(),
            "the datagram is not due yet",
        );

        // The datagram survived the cancellation, intact …
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"payload");
        assert_eq!(src, a.local_addr());

        // … and exactly once.
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "the datagram was delivered twice",
        );
    }

    #[tokio::test(start_paused = true)]
    async fn jitter_reorders_and_the_heap_is_stable() {
        // Large jitter relative to send spacing: order must break at least
        // once across the run.
        let net = Network::seeded(11);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        a.set_policy(FlakyPolicy::perfect().with_delay(Duration::ZERO, Duration::from_millis(100)));
        for i in 0..30u8 {
            a.send_to(&[i], b.local_addr()).await.expect("send");
        }
        let mut order = Vec::new();
        let mut buf = [0u8; 16];
        while let Ok(Ok((n, _))) =
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf)).await
        {
            order.push(buf[..n][0]);
        }
        assert_eq!(order.len(), 30, "nothing was lost, only reordered");
        let sent: Vec<u8> = (0..30).collect();
        assert_ne!(order, sent, "jitter never reordered anything");
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, sent, "reordering must not invent or lose bytes");

        // With no jitter, equal deadlines deliver FIFO — the sequence
        // tie-break in the heap.
        let net = Network::seeded(11);
        let a = net.endpoint(addr(3));
        let b = net.endpoint(addr(4));
        a.set_policy(FlakyPolicy::perfect());
        for i in 0..30u8 {
            a.send_to(&[i], b.local_addr()).await.expect("send");
        }
        for i in 0..30u8 {
            let (n, _) = recv_ok(&b, &mut buf).await;
            assert_eq!(buf[..n][0], i, "equal deadlines must deliver FIFO");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn duplication_delivers_two_identical_copies() {
        let net = Network::seeded(5);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        // Duplicate everything.
        a.set_policy(FlakyPolicy::perfect().with_duplication(1.0));

        a.send_to(b"dup", b.local_addr()).await.expect("send");

        let mut buf = [0u8; 16];
        for _ in 0..2 {
            let (n, src) = recv_ok(&b, &mut buf).await;
            assert_eq!(&buf[..n], b"dup");
            assert_eq!(src, a.local_addr());
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "exactly two copies, not three",
        );
        assert_eq!(net.sends(), 1, "one send, two deliveries");
    }

    /// The draw-order contract at the seam where it can break: index
    /// based drops (`drop_at` / `drop_first`) consume **no** RNG draw,
    /// while the probabilistic path consumes two (loss, then duplicate).
    /// The two are therefore not interchangeable, and mixing them in one
    /// policy is the only configuration that can expose a regression.
    ///
    /// Slice 0's mutation review found this untested: making index drops
    /// consume a draw anyway went **entirely undetected**, because no test
    /// combined an index-based drop with a nonzero `loss`. A regression
    /// would silently desync every later send's draw in exactly the
    /// fixture a handshake-retry test wants — "the first two initiations
    /// die, then normal jitter" — and would read as flakiness, not a bug.
    ///
    /// This is a **characterization pin**, not a derivation: the sequence
    /// below is whatever seed 4242 produces today. It has no meaning of
    /// its own and it is not a wire value. If it changes, the draw order
    /// changed — decide whether that was intended, then re-pin. Do not
    /// re-pin reflexively.
    #[tokio::test(start_paused = true)]
    async fn index_drops_do_not_perturb_the_probabilistic_draw_order() {
        let net = Network::seeded(4242);
        let a = net.endpoint(addr(70));
        let b = net.endpoint(addr(71));
        a.set_policy(FlakyPolicy {
            loss: 0.5,
            drop_at: [0, 7].into_iter().collect(),
            ..FlakyPolicy::perfect()
        });
        for i in 0..24u8 {
            a.send_to(&[i], b.local_addr()).await.expect("send");
        }

        let mut got = Vec::new();
        let mut buf = [0u8; 4];
        while let Ok(Ok((n, _))) =
            tokio::time::timeout(Duration::from_millis(200), b.recv_from(&mut buf)).await
        {
            got.push(buf[..n][0]);
        }

        assert!(
            !got.contains(&0) && !got.contains(&7),
            "index-dropped sends must never arrive; got {got:?}"
        );
        assert!(
            !got.is_empty() && got.len() < 22,
            "fixture must lose some and keep some, or it proves nothing \
             (got {} of a possible 22)",
            got.len()
        );
        assert_eq!(got, PINNED_DRAW_ORDER, "the RNG draw order changed");
    }

    /// See [`index_drops_do_not_perturb_the_probabilistic_draw_order`].
    const PINNED_DRAW_ORDER: &[u8] = &[3, 4, 12, 13, 14, 16, 17, 18, 19];

    /// byte-identical outcomes.
    /// `drop_at` consumes no RNG draw, so two different seeds must give
    /// byte-identical outcomes.
    #[tokio::test(start_paused = true)]
    async fn drop_at_is_exact() {
        async fn run(seed: u64) -> Vec<u8> {
            let net = Network::seeded(seed);
            let a = net.endpoint(addr(1));
            let b = net.endpoint(addr(2));
            a.set_policy(FlakyPolicy::drop_at([1, 3]));
            for i in 0..5u8 {
                a.send_to(&[i], b.local_addr()).await.expect("send");
            }
            let mut got = Vec::new();
            let mut buf = [0u8; 16];
            while let Ok(Ok((n, _))) =
                tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf)).await
            {
                got.push(buf[..n][0]);
            }
            got
        }

        assert_eq!(run(0).await, vec![0, 2, 4]);
        assert_eq!(
            run(0).await,
            run(999_999).await,
            "an index-based drop must not depend on the seed",
        );
    }

    /// §8.2 step 3's distinction: a blackhole is not a send error.
    #[tokio::test(start_paused = true)]
    async fn partition_blackholes_without_a_send_error() {
        let net = Network::seeded(2);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));
        net.partition(a.local_addr());

        let n = a.send_to(b"x", b.local_addr()).await.expect("Ok, not Err");
        assert_eq!(n, 1);
        assert_eq!(net.sends(), 1, "the send was counted");
        assert!(net.tap().is_empty(), "a blackholed send is not tapped");

        let mut buf = [0u8; 16];
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "nothing crosses a partition",
        );

        // One-directional blocking is separate from a full partition.
        net.heal(a.local_addr());
        net.block_path(a.local_addr(), b.local_addr());
        a.send_to(b"y", b.local_addr()).await.expect("Ok, not Err");
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "nothing crosses a blocked path",
        );

        net.heal_path(a.local_addr(), b.local_addr());
        a.send_to(b"z", b.local_addr()).await.expect("send");
        let (n, _) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"z", "healing restores the path");
    }

    /// The ruling-49 fixture, proved usable several slices before anything
    /// depends on it.
    #[tokio::test(start_paused = true)]
    async fn send_failure_is_an_err_and_then_heals() {
        let net = Network::seeded(4);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));

        let until = Instant::now() + Duration::from_secs(3);
        a.set_policy(FlakyPolicy::perfect().failing_sends_until(until));

        let err = a
            .send_to(b"x", b.local_addr())
            .await
            .expect_err("the send must fail");
        assert_eq!(err.kind(), io::ErrorKind::NetworkUnreachable);
        assert_eq!(err.raw_os_error(), Some(ENETUNREACH));
        assert_eq!(net.sends(), 1, "a failed send is still counted");
        assert!(net.tap().is_empty(), "a failed send is not tapped");

        let mut buf = [0u8; 16];
        assert!(
            tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
                .await
                .is_err(),
            "nothing was queued",
        );

        // Advance past the window, in virtual time.
        tokio::time::sleep_until(until + Duration::from_millis(1)).await;

        a.send_to(b"y", b.local_addr()).await.expect("healed");
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"y");
        assert_eq!(src, a.local_addr());
        assert_eq!(net.tap().len(), 1, "only the healed send was tapped");
    }

    #[tokio::test(start_paused = true)]
    async fn oversize_datagram_is_truncated_like_a_socket() {
        let net = Network::seeded(6);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));

        let big = vec![0xab; 2000];
        a.send_to(&big, b.local_addr()).await.expect("send");

        let mut buf = [0u8; crate::constants::MAX_DATAGRAM];
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(n, crate::constants::MAX_DATAGRAM);
        assert_eq!(src, a.local_addr());
        assert!(buf.iter().all(|byte| *byte == 0xab));
    }

    /// The forgery fixture slices 1–2 need for mac1 garbage and off-path
    /// spoofing.
    #[tokio::test(start_paused = true)]
    async fn inject_forges_a_source() {
        let net = Network::seeded(8);
        let b = net.endpoint(addr(2));
        let forged: SocketAddr = "203.0.113.9:9999".parse().expect("literal addr");

        net.inject(forged, b.local_addr(), b"x");

        let mut buf = [0u8; 16];
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"x");
        assert_eq!(src, forged, "the forged source is reported verbatim");
        assert_eq!(net.sends(), 0, "an injection is not a wire's send");
        assert!(net.tap().is_empty(), "an injection is not tapped");
    }

    #[tokio::test(start_paused = true)]
    async fn unregistered_destination_is_dropped_not_an_error() {
        let net = Network::seeded(9);
        let a = net.endpoint(addr(1));
        let nowhere: SocketAddr = "198.51.100.7:1".parse().expect("literal addr");

        let n = a.send_to(b"x", nowhere).await.expect("Ok, not Err");
        assert_eq!(n, 1);
        assert_eq!(net.sends(), 1);
        assert_eq!(net.tap().len(), 1, "it did leave the wire");
    }

    /// The DH ladder's instrument: exactly one increment per `dh`, and
    /// nothing else counts.
    #[test]
    fn dh_counter_counts_only_dh() {
        use hiss::provider::EphemeralOnly;

        let counter = DhCounter::new();
        let mut provider = counter.provider(EphemeralOnly::new(ChaCha20Rng::seed_from_u64(42)));

        let ephemeral = provider.generate_ephemeral_key().expect("keygen");
        assert_eq!(counter.get(), 0, "key generation is not a DH");
        let static_key = provider.generate_static_key().expect("keygen");
        assert_eq!(counter.get(), 0, "key generation is not a DH");
        let peer = provider.public_key(&static_key).expect("public_key");
        assert_eq!(counter.get(), 0, "public_key is not a DH");

        provider.dh(&ephemeral, &peer).expect("dh");
        assert_eq!(counter.get(), 1);

        // Clones observe the same count.
        let observer = counter.clone();
        provider.dh(&static_key, &peer).expect("dh");
        assert_eq!(observer.get(), 2);
        assert_eq!(counter.get(), 2);

        observer.reset();
        assert_eq!(counter.get(), 0, "reset is shared too");
    }

    /// A compile-fence mirroring `shell::wire`'s: the fixture is driven
    /// from a task that never required `Send`, and nothing in this module
    /// names `Send`.
    #[tokio::test(start_paused = true)]
    async fn the_whole_module_is_not_send() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let net = Network::seeded(12);
                let a = net.endpoint(addr(1));
                let b = net.endpoint(addr(2));
                let b_addr = b.local_addr();

                // `spawn_local` takes a `!Send` future — `spawn` would not
                // compile here, which is the whole point.
                let sender = tokio::task::spawn_local(async move {
                    a.send_to(b"local", b_addr).await.expect("send")
                });
                assert_eq!(sender.await.expect("join"), 5);

                let mut buf = [0u8; 16];
                let (n, _src) = recv_ok(&b, &mut buf).await;
                assert_eq!(&buf[..n], b"local");

                // And a generic driver with no `Send` bound accepts it.
                fn _drives_any_wire<W: Wire>(_w: &W) {}
                _drives_any_wire(&b);
            })
            .await;
    }

    // ═══════════════════════════════════════════════════════════════════
    // Ruling 180 — `rebind`, the fixture half of §7.3's roaming
    // ═══════════════════════════════════════════════════════════════════

    /// A rebound wire **sends from** its new address and **receives at** it.
    ///
    /// This is what S18 and S19 need and what nothing before ruling 180
    /// could express: `Network::inject` can forge a source, but only a real
    /// wire can originate the authenticated, window-fresh packet §7.3
    /// requires to move an endpoint.
    #[tokio::test(start_paused = true)]
    async fn a_rebound_wire_sends_from_and_receives_at_its_new_address() {
        let net = Network::seeded(0x510AD);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));

        a.send_to(b"before", addr(2)).await.expect("send");
        let mut buf = [0u8; 16];
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"before");
        assert_eq!(src, addr(1), "the pre-move source");

        a.rebind(addr(9));
        assert_eq!(a.local_addr(), addr(9), "local_addr follows the rebind");

        // Sends now carry the new source — this is the observation that
        // roams a peer.
        a.send_to(b"after", addr(2)).await.expect("send");
        let (n, src) = recv_ok(&b, &mut buf).await;
        assert_eq!(&buf[..n], b"after");
        assert_eq!(src, addr(9), "§7.3: the source a peer would re-home to");

        // And the return path reaches us where we now are.
        b.send_to(b"reply", addr(9)).await.expect("send");
        let (n, src) = recv_ok(&a, &mut buf).await;
        assert_eq!(&buf[..n], b"reply");
        assert_eq!(src, addr(2));
    }

    /// The old address stops delivering: a datagram sent to it after the
    /// move is dropped, exactly as an unregistered destination is.
    #[tokio::test(start_paused = true)]
    async fn the_vacated_address_delivers_nothing() {
        let net = Network::seeded(0x510AE);
        let a = net.endpoint(addr(1));
        let b = net.endpoint(addr(2));

        a.rebind(addr(9));
        b.send_to(b"to the old mapping", addr(1))
            .await
            .expect("send reports success — a blackhole is not an error");

        let mut buf = [0u8; 32];
        assert!(
            tokio::time::timeout(Duration::from_secs(5), a.recv_from(&mut buf))
                .await
                .is_err(),
            "a datagram to the vacated address must never arrive"
        );
    }

    /// **The ruling-180 decision, asserted from the side that separates the
    /// two options.**
    ///
    /// A datagram already queued for the old address when the rebind lands
    /// is **dropped**, not carried across. Under the rejected alternative
    /// (carry the inbox) this test hangs at the `timeout` — so it fails
    /// against the implementation the ruling declined, which is what makes
    /// it a pin rather than a name (working rule 9).
    ///
    /// It matters because S18 says a peer that moves and stays silent is
    /// indistinguishable from one that vanished. If in-flight datagrams
    /// followed the mover, a silent mover would keep receiving and that
    /// claim would pass for the wrong reason.
    #[tokio::test(start_paused = true)]
    async fn a_datagram_in_flight_to_the_old_address_is_lost_on_rebind() {
        let net = Network::seeded(0x510AF);
        let a = net.wire_with(
            addr(1),
            FlakyPolicy::perfect().with_delay(Duration::from_secs(2), Duration::ZERO),
        );
        let b = net.endpoint(addr(2));

        // In flight toward `a`, due in 2 s of virtual time.
        b.send_to(b"in flight", addr(1)).await.expect("send");

        // `a` moves before it lands.
        a.rebind(addr(9));

        let mut buf = [0u8; 32];
        assert!(
            tokio::time::timeout(Duration::from_secs(10), a.recv_from(&mut buf))
                .await
                .is_err(),
            "ruling 180: the old address's inbox is abandoned, not carried"
        );

        // The wire is not broken by the loss — it still works at the new
        // address. Without this the test above would pass for a wire that
        // had simply died.
        b.send_to(b"after the move", addr(9)).await.expect("send");
        let (n, _src) = recv_ok(&a, &mut buf).await;
        assert_eq!(&buf[..n], b"after the move");
    }

    /// Rebinding onto a live address panics, as `Network::endpoint` does —
    /// two wires at one address would silently share an inbox.
    #[tokio::test(start_paused = true)]
    #[should_panic(expected = "is already registered on this network")]
    async fn rebinding_onto_a_registered_address_panics() {
        let net = Network::seeded(0x510B0);
        let a = net.endpoint(addr(1));
        let _b = net.endpoint(addr(2));
        a.rebind(addr(2));
    }

    /// Rebinding to the address we already hold is a no-op, **not** a
    /// panic — and in particular does not drop the inbox, which the naive
    /// remove-then-insert would.
    #[tokio::test(start_paused = true)]
    async fn rebinding_to_the_same_address_keeps_the_inbox() {
        let net = Network::seeded(0x510B1);
        let a = net.wire_with(
            addr(1),
            FlakyPolicy::perfect().with_delay(Duration::from_secs(2), Duration::ZERO),
        );
        let b = net.endpoint(addr(2));

        b.send_to(b"queued", addr(1)).await.expect("send");
        a.rebind(addr(1));

        let mut buf = [0u8; 32];
        let (n, _src) = recv_ok(&a, &mut buf).await;
        assert_eq!(
            &buf[..n],
            b"queued",
            "a same-address rebind must not discard the inbox"
        );
    }
}

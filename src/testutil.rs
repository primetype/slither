//! Test utilities: an in-memory [`FlakyWire`] (loss, reorder, duplication,
//! delay, and partitioning) that stands in for a UDP socket, plus a DH-counting
//! identity for the DH-cost pins (the mac1 flood, and the Leg 1c staged
//! responder cost).
//!
//! Available under `cfg(test)` and behind the `test-util` feature, so downstream
//! crates (Leg 2) can drive slither over the same shim. Nothing here is used in
//! production.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hiss::curve::SharedSecret;
use hiss::curve::p256::{P256r1PrivateKey, P256r1PublicKey};
use hiss::noise::P256;
use hiss::provider::{CryptoKeyProvider, DhProvider, EphemeralOnly};
use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use tokio::sync::mpsc;

use crate::endpoint::Wire;
use crate::handshake::{HsError, Identity, SoftwareIdentity};

// ── Identities ────────────────────────────────────────────────────────────────

/// A valid, non-degenerate P-256 static scalar seeded from one byte (kept well
/// inside `[1, n-1]` by pinning the leading byte low).
pub fn valid_scalar(fill: u8) -> [u8; 32] {
    let mut scalar = [fill; 32];
    scalar[0] = 0x01;
    scalar
}

/// A software identity from a scalar-fill byte and a master-RNG seed byte.
pub fn software_identity(scalar_fill: u8, master_seed: u8) -> SoftwareIdentity {
    SoftwareIdentity::from_scalar(
        valid_scalar(scalar_fill),
        ChaCha20Rng::from_seed([master_seed; 32]),
    )
    .expect("valid static scalar")
}

/// A software provider that counts the ECDH operations it performs, so a test can
/// prove that garbage / wrong-key packets never reach the curve.
pub struct CountingProvider {
    inner: EphemeralOnly<ChaCha20Rng>,
    dh_count: Arc<AtomicUsize>,
}

impl CryptoKeyProvider<P256> for CountingProvider {
    type Error = <EphemeralOnly<ChaCha20Rng> as CryptoKeyProvider<P256>>::Error;
    type PrivateKey = P256r1PrivateKey;

    fn public_key(&self, key: &Self::PrivateKey) -> Result<P256r1PublicKey, Self::Error> {
        <EphemeralOnly<ChaCha20Rng> as CryptoKeyProvider<P256>>::public_key(&self.inner, key)
    }

    fn generate_static_key(&mut self) -> Result<Self::PrivateKey, Self::Error> {
        <EphemeralOnly<ChaCha20Rng> as CryptoKeyProvider<P256>>::generate_static_key(
            &mut self.inner,
        )
    }

    fn generate_ephemeral_key(&mut self) -> Result<Self::PrivateKey, Self::Error> {
        <EphemeralOnly<ChaCha20Rng> as CryptoKeyProvider<P256>>::generate_ephemeral_key(
            &mut self.inner,
        )
    }
}

impl DhProvider<P256> for CountingProvider {
    fn dh(
        &self,
        key: &Self::PrivateKey,
        peer: &P256r1PublicKey,
    ) -> Result<SharedSecret<32>, Self::Error> {
        self.dh_count.fetch_add(1, Ordering::SeqCst);
        <EphemeralOnly<ChaCha20Rng> as DhProvider<P256>>::dh(&self.inner, key, peer)
    }
}

/// An [`Identity`] whose per-handshake providers count every ECDH they perform.
///
/// The shared counter ([`dh_count`](Self::dh_count)) lets a test assert that a
/// mac1 flood is rejected **before** any curve work.
pub struct CountingIdentity {
    scalar: [u8; 32],
    public: P256r1PublicKey,
    master: ChaCha20Rng,
    dh_count: Arc<AtomicUsize>,
}

impl CountingIdentity {
    /// Build a counting identity from a scalar-fill byte and a master-RNG seed.
    pub fn new(scalar_fill: u8, master_seed: u8) -> Result<Self, HsError> {
        let scalar = valid_scalar(scalar_fill);
        let secret =
            P256r1PrivateKey::from_bytes(scalar).map_err(|_| HsError::Drop("invalid scalar"))?;
        Ok(Self {
            scalar,
            public: secret.public(),
            master: ChaCha20Rng::from_seed([master_seed; 32]),
            dh_count: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// A handle on the shared ECDH counter.
    pub fn dh_count(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.dh_count)
    }
}

impl Identity for CountingIdentity {
    type Provider = CountingProvider;

    fn provider(&mut self) -> Self::Provider {
        let mut seed = [0u8; 32];
        self.master.fill_bytes(&mut seed);
        CountingProvider {
            inner: EphemeralOnly::new(ChaCha20Rng::from_seed(seed)),
            dh_count: Arc::clone(&self.dh_count),
        }
    }

    fn static_secret(&mut self) -> P256r1PrivateKey {
        P256r1PrivateKey::from_bytes(self.scalar).expect("validated scalar")
    }

    fn static_public(&self) -> P256r1PublicKey {
        self.public
    }
}

// ── The flaky in-memory wire ──────────────────────────────────────────────────

/// A datagram fault-injection policy: independent loss and duplication
/// probabilities, plus a fixed base delay and a uniform per-copy jitter (which is
/// what reorders packets).
#[derive(Clone, Copy, Debug)]
pub struct FlakyPolicy {
    /// Probability in `0.0..=1.0` that a datagram is dropped entirely.
    pub loss: f64,
    /// Probability in `0.0..=1.0` that a delivered datagram is duplicated.
    pub duplicate: f64,
    /// A fixed delay before delivery.
    pub base_delay: Duration,
    /// A uniform random extra delay in `0..=jitter` per copy — the source of
    /// reordering.
    pub jitter: Duration,
    /// Deterministically drop the first N datagrams this wire sends (e.g. to lose
    /// exactly the first two msg1s), independent of [`loss`](Self::loss).
    pub drop_first: usize,
}

impl FlakyPolicy {
    /// A lossless, in-order, no-delay wire.
    pub fn perfect() -> Self {
        Self {
            loss: 0.0,
            duplicate: 0.0,
            base_delay: Duration::ZERO,
            jitter: Duration::ZERO,
            drop_first: 0,
        }
    }

    /// [`perfect`](Self::perfect), but drop the first `n` datagrams sent.
    pub fn drop_first(n: usize) -> Self {
        Self {
            drop_first: n,
            ..Self::perfect()
        }
    }

    /// Decide how many copies of a datagram to deliver, and each copy's delay. An
    /// empty result means the datagram was lost.
    fn deliveries(&self, rng: &mut ChaCha20Rng) -> Vec<Duration> {
        if next_f64(rng) < self.loss {
            return Vec::new();
        }
        let copies = if next_f64(rng) < self.duplicate { 2 } else { 1 };
        (0..copies)
            .map(|_| self.base_delay + self.rand_jitter(rng))
            .collect()
    }

    fn rand_jitter(&self, rng: &mut ChaCha20Rng) -> Duration {
        let span = self.jitter.as_nanos() as u64;
        if span == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(rng.next_u64() % (span + 1))
    }
}

fn next_f64(rng: &mut ChaCha20Rng) -> f64 {
    (rng.next_u32() as f64) / (u32::MAX as f64 + 1.0)
}

/// A datagram observed on the network tap.
#[derive(Clone, Debug)]
pub struct Spied {
    /// The sender's address.
    pub src: SocketAddr,
    /// The recipient's address.
    pub dst: SocketAddr,
    /// The datagram bytes.
    pub bytes: Vec<u8>,
}

type Inbox = mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>;

/// A shared in-memory network that routes datagrams between [`FlakyWire`]
/// endpoints, with optional address partitioning and a delivery tap.
#[derive(Clone)]
pub struct Network {
    peers: Arc<Mutex<HashMap<SocketAddr, Inbox>>>,
    partitioned: Arc<Mutex<HashSet<SocketAddr>>>,
    blocked_paths: Arc<Mutex<HashSet<(SocketAddr, SocketAddr)>>>,
    tap: Arc<Mutex<Option<mpsc::UnboundedSender<Spied>>>>,
    sends: Arc<Mutex<Vec<Spied>>>,
}

impl Network {
    /// A fresh, empty network.
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            partitioned: Arc::new(Mutex::new(HashSet::new())),
            blocked_paths: Arc::new(Mutex::new(HashSet::new())),
            tap: Arc::new(Mutex::new(None)),
            sends: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register an endpoint at `addr` with a fault policy and a seeded RNG,
    /// returning its wire.
    pub fn endpoint(&self, addr: SocketAddr, policy: FlakyPolicy, rng_seed: [u8; 32]) -> FlakyWire {
        let (tx, rx) = mpsc::unbounded_channel();
        self.peers.lock().unwrap().insert(addr, tx);
        FlakyWire {
            addr,
            inbound: tokio::sync::Mutex::new(rx),
            net: self.clone(),
            rng: Mutex::new(ChaCha20Rng::from_seed(rng_seed)),
            policy,
            drop_remaining: Mutex::new(policy.drop_first),
        }
    }

    /// Every datagram any wire has attempted to send (pre-loss), in order — for
    /// asserting, e.g., that successive retransmits carry fresh ephemerals.
    pub fn sends(&self) -> Vec<Spied> {
        self.sends.lock().unwrap().clone()
    }

    /// Inject a raw datagram straight onto the wire (bypassing any sender policy)
    /// — for feeding garbage, replays, or forged packets to an endpoint.
    pub fn inject(&self, dst: SocketAddr, src: SocketAddr, bytes: Vec<u8>) {
        self.deliver(dst, src, bytes);
    }

    /// Install a delivery tap, returning a receiver of every **delivered**
    /// datagram (post-loss).
    pub fn tap(&self) -> mpsc::UnboundedReceiver<Spied> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.tap.lock().unwrap() = Some(tx);
        rx
    }

    /// Stop delivering any datagram to or from `addr` (a network partition).
    pub fn partition(&self, addr: SocketAddr) {
        self.partitioned.lock().unwrap().insert(addr);
    }

    /// Heal a partition on `addr`.
    pub fn heal(&self, addr: SocketAddr) {
        self.partitioned.lock().unwrap().remove(&addr);
    }

    /// Stop delivering datagrams from `from` to `to` **in that direction
    /// only** (e.g. lose every ACK while the data keeps flowing).
    pub fn block_path(&self, from: SocketAddr, to: SocketAddr) {
        self.blocked_paths.lock().unwrap().insert((from, to));
    }

    /// Heal a one-way block installed by [`block_path`](Self::block_path).
    pub fn heal_path(&self, from: SocketAddr, to: SocketAddr) {
        self.blocked_paths.lock().unwrap().remove(&(from, to));
    }

    fn deliver(&self, dst: SocketAddr, src: SocketAddr, bytes: Vec<u8>) {
        {
            let partitioned = self.partitioned.lock().unwrap();
            if partitioned.contains(&dst) || partitioned.contains(&src) {
                return;
            }
        }
        if self.blocked_paths.lock().unwrap().contains(&(src, dst)) {
            return;
        }
        if let Some(tap) = self.tap.lock().unwrap().as_ref() {
            let _ = tap.send(Spied {
                src,
                dst,
                bytes: bytes.clone(),
            });
        }
        if let Some(inbox) = self.peers.lock().unwrap().get(&dst) {
            let _ = inbox.send((src, bytes));
        }
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

/// One endpoint's view of a [`Network`]: a [`Wire`] with fault injection.
pub struct FlakyWire {
    addr: SocketAddr,
    inbound: tokio::sync::Mutex<mpsc::UnboundedReceiver<(SocketAddr, Vec<u8>)>>,
    net: Network,
    rng: Mutex<ChaCha20Rng>,
    policy: FlakyPolicy,
    drop_remaining: Mutex<usize>,
}

impl Wire for FlakyWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        let len = buf.len();
        // Record every send attempt (pre-loss) on the shared send log.
        self.net.sends.lock().unwrap().push(Spied {
            src: self.addr,
            dst: addr,
            bytes: buf.to_vec(),
        });
        // Deterministic head-drop (independent of the loss probability).
        {
            let mut remaining = self.drop_remaining.lock().unwrap();
            if *remaining > 0 {
                *remaining -= 1;
                return Ok(len);
            }
        }
        // Draw every random decision up front and release the lock before any
        // await, so the RNG is never held across a suspension point.
        let deliveries = {
            let mut rng = self.rng.lock().unwrap();
            self.policy.deliveries(&mut rng)
        };
        let bytes = buf.to_vec();
        let src = self.addr;
        for delay in deliveries {
            let net = self.net.clone();
            let bytes = bytes.clone();
            tokio::task::spawn_local(async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                net.deliver(addr, src, bytes);
            });
        }
        Ok(len)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut inbound = self.inbound.lock().await;
        match inbound.recv().await {
            Some((src, bytes)) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok((n, src))
            }
            // The network is gone; block forever rather than spin the actor.
            None => std::future::pending().await,
        }
    }
}

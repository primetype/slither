//! Independent acceptance tests for slice 2a — the sans-io endpoint core.
//!
//! **Authorship (CLAUDE.md working rule 6, `.slices/02-handshake/PLAN.md`
//! §9.1).** Written by an author who has not read `src/core/`'s
//! implementation, nor `.slices/02-handshake/IMPLEMENTATION.md`, nor git
//! history — from `SPEC.md` §§5.3–5.7, §6.1–6.3, §16.4–16.6, §17.1–17.5,
//! rulings 69/70/71, and `PLAN.md`'s declared API surface alone. Slice 1's
//! lesson is written into every boundary here: its one real gap was a cap
//! tested on **one** side (`LEN-1` and `LEN` pinned, `LEN+1` not), so a
//! mutation relaxing an exact check to a minimum survived 187 green tests.
//! Every cap, TTL and count below is asserted **at, below and above**.
//!
//! **Every test is a plain `#[test]`.** §16.4 makes `now: Instant` an
//! argument and the cores never read a clock, so time is arithmetic on a
//! base `Instant`: no `#[tokio::test]`, no `tokio::time::pause()`, no
//! `LocalSet`, no `FlakyWire`, no sleeps.
//!
//! # Groups
//!
//! 1. `poll_contract` — §16.4's drain-to-`Timeout`, and §16.5's
//!    min-deadline over the three endpoint timer families.
//! 2. `dh_ladder` — §6.1's cumulative 1 / 2 / 4, exact counts.
//! 3. `intro_queue` — §6.3 entire, including **ruling 69** (evict-oldest
//!    orders by last refresh) and **ruling 71** (`intro_source` /
//!    `intro_sender_index` read through to the newest bytes).
//! 4. `guard` — §17.1, including the provisional write and its revert, and
//!    **ruling 70**'s `TS_GUARD_ORPHAN_TTL`.
//! 5. `restart` — §5.4's three-valued responder rule, at slice 2a's bound.
//! 6. `initiator` — §5.5/§5.7's driving; §17.3's minting.
//!
//! # Assumptions this file makes about names the plan did not declare
//!
//! `PLAN.md` §9.3 lists what the test author needs declared. Five items on
//! that list are **named but not given a signature** anywhere in the plan,
//! so the shapes below are this author's guess and a compile error on one
//! is the reconciliation surfacing, not a bug in the reasoning. Each is
//! used in exactly one place — [`Ep`] and its free helpers — so the fix is
//! local:
//!
//! - `testutil::CountingIdentity::new(seed: [u8; 32])` and
//!   `fn counter(&self) -> DhCounter` (plan §9.3 item 2 asks for "its
//!   constructor, its `DhCounter` accessor"; neither is written down).
//! - `Config::default()` plus `with_intro_queue_cap` /
//!   `with_intro_max_per_source` / `with_clock` (plan §2.2 declares
//!   `Config`'s three **private** fields and no constructor).
//! - `Timestamp::new(secs: u64, nanos: u32)` (plan §2.3 declares the
//!   struct with private fields and no constructor).
//! - The msg1/msg2 framing helper of plan §9.3 item 3 is written here
//!   ([`forged_init`]) rather than assumed, out of slice-1 pieces only.
//! - Item paths: everything the core owns is reached through `use super::*`
//!   so that `core/mod.rs`'s re-export decisions, not this file's guesses,
//!   decide where a name lives.

#![allow(clippy::items_after_statements)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;

use crate::config::{Config, WallClock};
use crate::constants::{
    HANDSHAKE_GIVEUP, INIT_PACKET_LEN, INTRO_MAX_PER_SOURCE, INTRO_QUEUE_CAP, INTRO_TTL, MAC1_LEN,
    PKT_DATA, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP, RESP_PACKET_LEN, RETRANSMIT_BASE,
    RETRANSMIT_JITTER_MAX, TS_GUARD_ORPHAN_TTL, VERSION,
};
use crate::error::{AcceptError, AuthError, ConnectError, IntroError};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::mac::Mac1Key;
use crate::testutil::{CountingIdentity, DhCounter};

/// The identity every endpoint in this file runs on: `!Send` by
/// construction, because [`crate::testutil::CountingProvider`] holds an
/// `Rc<Cell<u32>>`. Plan §3.2 mechanism (2) — every DH-ladder test below is
/// therefore also a compile-time proof that no `Send` bound sits on the
/// core's path.
type Id = CountingIdentity;
type Pk = PublicKeyOf<Id>;

// ═══════════════════════════════════════════════════════════════════════
// Plan §3.2 mechanism (3) — the negative assertion, so mechanism (2)
// cannot go vacuous. Compiles ONLY while the type is not `Send`: two
// applicable impls otherwise, and inference fails. S21.
// ═══════════════════════════════════════════════════════════════════════

trait AmbiguousIfSend<A> {
    fn assertion() {}
}
impl<T: ?Sized> AmbiguousIfSend<()> for T {}
impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

const _: fn() = || {
    <Endpoint<Id> as AmbiguousIfSend<_>>::assertion();
};

// ═══════════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════════

/// Everything drained out of one `poll_output()` loop, plus the terminal
/// deadline the loop ended on.
///
/// Collected into a `Vec` rather than matched one at a time because §16.4
/// (4508–4510) makes **generation order** normative: `outs[0]` before
/// `outs[1]` is testing a rule, not an implementation detail.
#[derive(Debug, Default)]
struct Drained {
    outs: Vec<EndpointOutput>,
    deadline: Option<Instant>,
}

impl Drained {
    fn transmits(&self) -> Vec<(SocketAddr, Vec<u8>)> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::Transmit(t) => Some((t.to, t.data.clone())),
                _ => None,
            })
            .collect()
    }

    fn intros(&self) -> Vec<(IntroId, SocketAddr)> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::IntroReady(id, src) => Some((*id, *src)),
                _ => None,
            })
            .collect()
    }

    fn installs(&self) -> Vec<ConnectionId> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::ToConnection(id, _) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn failures(&self) -> Vec<(ConnectionId, ConnectError)> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::HandshakeFailed(id, e) => Some((*id, e.clone())),
                _ => None,
            })
            .collect()
    }

    /// The single intro this drain surfaced. Panics on zero or two — a
    /// count assertion phrased as an extractor, because "surfaced exactly
    /// once" is the property most of §6.3 turns on.
    fn one_intro(&self) -> (IntroId, SocketAddr) {
        let v = self.intros();
        assert_eq!(v.len(), 1, "expected exactly one IntroReady, got {v:?}");
        v[0]
    }

    fn one_transmit(&self) -> (SocketAddr, Vec<u8>) {
        let v = self.transmits();
        assert_eq!(v.len(), 1, "expected exactly one Transmit, got {}", v.len());
        v[0].clone()
    }

    /// Nothing but the terminal `Timeout`. The shape a silent drop, a
    /// silent eviction and a silent reject all have to have.
    fn is_silent(&self) -> bool {
        self.outs.is_empty()
    }
}

/// One endpoint core plus the bookkeeping a test needs about it.
struct Ep {
    ep: Endpoint<Id>,
    /// Endpoint-wide and cumulative across handshakes — exactly what
    /// §6.1's table prices.
    dhs: DhCounter,
    /// This endpoint's own static, kept out of the core so a test can mint
    /// mac1 for it (§4.3: mac1's key is public data).
    public_static: Pk,
    addr: SocketAddr,
}

impl Ep {
    fn new(now: Instant, key_seed: u8, rng_seed: u8, addr: SocketAddr, config: Config) -> Self {
        let identity = CountingIdentity::new([key_seed; 32]);
        let dhs = identity.counter();
        let public_static = identity.public_static().clone();
        let ep = Endpoint::new(now, config, identity, [rng_seed; 32]);
        Ep {
            ep,
            dhs,
            public_static,
            addr,
        }
    }

    /// §2.4's canonical static encoding: the `as_ref()` octets of the
    /// `Curve::PublicKey` (`suite.rs` pins `PublicKey: AsRef<[u8]>` for
    /// exactly this).
    fn canonical(&self) -> &[u8] {
        self.public_static.as_ref()
    }

    fn mac1_key(&self) -> Mac1Key {
        Mac1Key::derive(self.canonical())
    }

    /// §16.4's contract, mechanised: drain to the terminal
    /// `Timeout(Option<Instant>)`. The bound is not decoration — a core
    /// that re-emits forever fails here as a named panic instead of as a
    /// CI hang minutes later.
    fn drain(&mut self) -> Drained {
        let mut d = Drained::default();
        for _ in 0..100_000 {
            match self.ep.poll_output() {
                EndpointOutput::Timeout(t) => {
                    d.deadline = t;
                    return d;
                }
                other => d.outs.push(other),
            }
        }
        panic!("poll_output() did not reach the terminal Timeout in 100_000 outputs (§16.4)");
    }

    fn datagram(&mut self, now: Instant, src: SocketAddr, dgram: &[u8]) -> (Disposition, Drained) {
        let disp = self.ep.handle_datagram(now, src, dgram);
        (disp, self.drain())
    }

    /// `handle_datagram` + drain, discarding the disposition. The shape
    /// most queue tests want.
    fn feed(&mut self, now: Instant, src: SocketAddr, dgram: &[u8]) -> Drained {
        self.datagram(now, src, dgram).1
    }

    fn timeout(&mut self, now: Instant) -> Drained {
        self.ep.handle_timeout(now);
        self.drain()
    }

    fn connect(&mut self, now: Instant, remote: SocketAddr, peer: &Pk) -> (ConnectionId, Drained) {
        let (id, _conn) = self
            .ep
            .connect(now, remote, peer.clone())
            .expect("connect should succeed");
        (id, self.drain())
    }

    /// Is a stage-0 entry (or consumed chain) still held under this id?
    ///
    /// `intro_source` is ruling 71's accessor and §6.3 requires it to read
    /// through to live state, which makes it the honest presence oracle:
    /// an implementation that answered from a cache would fail the ruling
    /// 71 tests below before it could flatter these.
    fn present(&self, id: IntroId) -> bool {
        self.ep.intro_source(id).is_some()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Free helpers
// ═══════════════════════════════════════════════════════════════════════

const T_BASE_SECS: u64 = 1_700_000_000;

fn t0() -> Instant {
    Instant::now()
}

fn v4(a: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), port)
}

/// A distinct IPv4 **source IP** per `n` — 256 of them, which is exactly
/// what §6.3's honesty clause says filling the queue takes
/// (1024 / `INTRO_MAX_PER_SOURCE`).
fn v4_nth(n: usize, port: u16) -> SocketAddr {
    let n = u32::try_from(n).expect("test index fits");
    let ip = Ipv4Addr::from(0x0a01_0000u32 + n);
    SocketAddr::new(IpAddr::V4(ip), port)
}

/// An IPv6 address inside the /64 selected by `prefix`, distinguished only
/// in the **host** half. §6.3 keys the per-source cap on the /64.
fn v6_in_64(prefix: u8, host: u16, port: u16) -> SocketAddr {
    let mut o = [0u8; 16];
    o[0] = 0x20;
    o[1] = 0x01;
    o[7] = prefix; // last byte of the /64
    o[14..16].copy_from_slice(&host.to_be_bytes());
    SocketAddr::new(IpAddr::V6(Ipv6Addr::from(o)), port)
}

/// A syntactically valid, **mac1-valid**, cryptographically meaningless
/// HandshakeInit addressed to `responder` (plan §9.3 item 3).
///
/// Composed only of slice-1 pieces — the §3.2 header layout and
/// `Mac1Key::derive(recipient_static).tag(preimage)` — so it introduces no
/// crypto. It is enough to reach the queue, because §6.1 prices parking at
/// "1 keyed hash, 0 DH": nothing between the length gate and the queue
/// reads a byte of msg1. `read_identity()` on one of these is expected to
/// fail `Malformed`, which is why every test that needs a *consumed* chain
/// replays a real msg1 instead.
fn forged_init(responder: &Ep, sender_index: u32, filler: u8) -> Vec<u8> {
    let mut d = vec![filler; INIT_PACKET_LEN];
    d[0] = PKT_HANDSHAKE_INIT;
    d[1] = VERSION;
    d[2..6].copy_from_slice(&sender_index.to_le_bytes());
    let (preimage, tag) = d.split_at_mut(INIT_PACKET_LEN - MAC1_LEN);
    let t = responder.mac1_key().tag(preimage);
    tag.copy_from_slice(&t);
    d
}

/// The `sender_index` a HandshakeInit carries — §3.2, little-endian at
/// offset 2 (ruling 64).
fn init_sender_index(dgram: &[u8]) -> u32 {
    u32::from_le_bytes(dgram[2..6].try_into().expect("init header"))
}

/// §3.3's `sender_index` (offset 2) and `receiver_index` (offset 6), both
/// little-endian. Note the order: **theirs, then ours**.
fn resp_indices(dgram: &[u8]) -> (u32, u32) {
    (
        u32::from_le_bytes(dgram[2..6].try_into().expect("resp header")),
        u32::from_le_bytes(dgram[6..10].try_into().expect("resp header")),
    )
}

/// A well-formed Data packet for `receiver_index`, with a garbage
/// ciphertext. §17.3's corollary is that such a datagram "touches
/// nothing"; it must still *route*.
fn data_packet(receiver_index: u32, counter: u64, body: usize) -> Vec<u8> {
    let mut d = vec![0x5Au8; crate::constants::DATA_HEADER_LEN + body];
    d[0] = PKT_DATA;
    d[1] = VERSION;
    d[2..6].copy_from_slice(&receiver_index.to_le_bytes());
    d[6..14].copy_from_slice(&counter.to_le_bytes());
    d
}

/// A wall clock frozen at one reading — the only way to observe §5.3's
/// **forcing**, since a real clock advances between two calls and would
/// make a monotone result prove nothing.
struct FrozenClock(Timestamp);

impl WallClock for FrozenClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

fn default_config() -> Config {
    Config::default()
}

fn capped_config(cap: usize, per_source: usize) -> Config {
    Config::default()
        .with_intro_queue_cap(cap)
        .with_intro_max_per_source(per_source)
}

fn frozen_clock_config(secs: u64, nanos: u32) -> Config {
    Config::default().with_clock(Rc::new(FrozenClock(Timestamp::new(secs, nanos))))
}

/// A **real** msg1 from `initiator` to `responder`, captured off the wire.
///
/// Every test that needs `read_identity()`/`authenticate()` to succeed
/// starts here: a forged init reaches the queue but not the ladder.
fn real_msg1(initiator: &mut Ep, now: Instant, responder: &Ep) -> Vec<u8> {
    let peer = responder.public_static.clone();
    let (_id, drained) = initiator.connect(now, responder.addr, &peer);
    let (to, data) = drained.one_transmit();
    assert_eq!(to, responder.addr, "§5.5 step 1 sends to the dialled address");
    assert_eq!(data.len(), INIT_PACKET_LEN, "§3.2, exact (ruling 65)");
    data
}

/// A pair of endpoints on the default config, `a` at 10.0.0.1:1, `b` at
/// 10.0.0.2:2.
fn pair(now: Instant) -> (Ep, Ep) {
    let a = Ep::new(now, 7, 0x11, v4(1, 1), default_config());
    let b = Ep::new(now, 9, 0x22, v4(2, 2), default_config());
    (a, b)
}

// ───────────────────────────────────────────────────────────────────────
// (sections follow)
// ───────────────────────────────────────────────────────────────────────

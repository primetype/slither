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
//! 7. `tables` — §17.4's replacement basis and hint set: the state this
//!    slice writes and nothing in it reads.
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
    HANDSHAKE_GIVEUP, INIT_HEADER_LEN, INIT_PACKET_LEN, INTRO_MAX_PER_SOURCE, INTRO_QUEUE_CAP,
    INTRO_TTL, MAC1_LEN, PKT_DATA, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP, RESP_PACKET_LEN,
    RETRANSMIT_BASE, RETRANSMIT_JITTER_MAX, STATIC_PUBLIC_LEN, TS_GUARD_ORPHAN_TTL, VERSION,
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
// Integration adaptation (orchestrator, not the test author): the plan did
// not declare that `CountingIdentity` is generic over the suite, nor that
// `EndpointOutput` carries the suite parameter (§16.4's Rust block is
// schematic — the seal/open halves are `DatagramSend<IK>`/`DatagramRecv<IK>`,
// so the output enum must be generic too). Both aliases below are type
// adaptation only; no assertion, name or expected value was changed.
type Suite = crate::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
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
    outs: Vec<EndpointOutput<Suite>>,
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

    /// §6.4's LIVE branch: the connections this drain replaced (§5.4).
    fn replaced(&self) -> Vec<ConnectionId> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::Replaced(id) => Some(*id),
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
        let identity: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = identity.counter();
        let public_static = *identity.public_static();
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
        // Ruling 90 split §16.4's `connect` in two: `mint_pending` (0 DH)
        // and `start_attempt` (§6.1's `es` + `ss`). The shell calls them from
        // two tasks; a dial is still the two of them, in this order.
        let (id, _conn) = self
            .ep
            .mint_pending(now, remote, *peer)
            .expect("connect should succeed");
        self.ep.start_attempt(now, id);
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

/// The **cleartext ephemeral public key** inside a HandshakeInit.
///
/// Noise IK msg1 is `e ‖ ENCRYPTED(s) ‖ ENCRYPTED(payload)`, and `e` is
/// sent in the clear, so it is the leading `STATIC_PUBLIC_LEN` octets of
/// the Noise message — `[INIT_HEADER_LEN, INIT_HEADER_LEN +
/// STATIC_PUBLIC_LEN)` of the datagram. `constants.rs` pins that
/// decomposition as a compile-time assertion
/// (`IK_MSG1_LEN == STATIC_PUBLIC_LEN + (STATIC_PUBLIC_LEN +
/// AEAD_TAG_LEN) + (MSG1_PAYLOAD_LEN + AEAD_TAG_LEN)`), so this offset
/// cannot drift without the build going red first.
///
/// Isolating the region matters: comparing **whole packets** across two
/// attempts proves nothing about the ephemeral, because §5.5 also mints a
/// fresh `sender_index` and a fresh timestamp for every attempt — three
/// reasons for the bytes to differ, and a test that names one of them
/// while observing all three cannot fail for the reason it claims.
fn msg1_ephemeral(dgram: &[u8]) -> &[u8] {
    &dgram[INIT_HEADER_LEN..INIT_HEADER_LEN + STATIC_PUBLIC_LEN]
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
    let peer = responder.public_static;
    let (_id, drained) = initiator.connect(now, responder.addr, &peer);
    let (to, data) = drained.one_transmit();
    assert_eq!(
        to, responder.addr,
        "§5.5 step 1 sends to the dialled address"
    );
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

// ═══════════════════════════════════════════════════════════════════════
// 1. The poll contract — §16.4, §16.5
// ═══════════════════════════════════════════════════════════════════════

/// §16.4: "every mutating call … is followed by draining `poll_output()`
/// to the terminal `Timeout(Option<Instant>)`".
///
/// Scripted over **every** mutating verb the endpoint core has, because
/// the contract is universal and a core that terminates for five of six is
/// a core with one path that hangs the driver.
#[test]
fn the_drain_always_terminates_in_timeout() {
    let t = t0();
    let (mut a, mut b) = pair(t);

    // new()
    let _ = b.drain();

    // connect()
    let peer = b.public_static;
    let (conn, d) = a.connect(t, b.addr, &peer);
    assert!(d.deadline.is_some(), "a pending arms a retransmit");

    // handle_datagram() — a real msg1, a forged one, and garbage.
    let msg1 = d.one_transmit().1;
    let intro = b.feed(t, a.addr, &msg1).one_intro().0;
    let _ = b.feed(t, v4(3, 3), &forged_init(&b, 1, 0x11));
    let _ = b.feed(t, v4(4, 4), b"not a slither packet");

    // the staged verbs, and reject()
    let _ = b.ep.read_identity(t, intro);
    let _ = b.drain();
    let _ = b.ep.authenticate(t, intro);
    let _ = b.drain();
    let _ = b.ep.accept(t, intro);
    let _ = b.drain();
    b.ep.reject(t, intro);
    let _ = b.drain();

    // handle_timeout(), including one far past every deadline.
    let _ = a.timeout(t + Duration::from_secs(1));
    let _ = b.timeout(t + INTRO_TTL * 4);

    // handle_connection_event()
    a.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: 1 });
    let _ = a.drain();
}

/// The sentinel is not consumed by being observed: draining twice with no
/// mutating call between yields the same terminal `Timeout` and nothing
/// else. A core that queued its outputs and forgot to clear them re-emits
/// on the second drain.
#[test]
fn a_second_drain_with_no_mutating_call_yields_only_timeout() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_id, first) = a.connect(t, b.addr, &peer);
    assert_eq!(first.transmits().len(), 1);

    let second = a.drain();
    assert!(second.is_silent(), "outputs re-emitted: {:?}", second.outs);
    assert_eq!(
        second.deadline, first.deadline,
        "the announced deadline is a property of state, not of the drain"
    );
}

/// §16.5 (4535–4537): "The endpoint core's deadline is the min over its
/// pendings' retransmit/give-up deadlines, the parked intros' expiries,
/// and the timestamp-guard orphan aging."
///
/// Tested as a **minimum**, from both sides: within one family the earlier
/// expiry wins and the later must not displace it, and across families a
/// pending's retransmit takes the minimum from an intro expiry.
#[test]
fn the_announced_deadline_is_the_minimum_of_the_live_timers() {
    let t = t0();
    let (mut a, mut b) = pair(t);

    // Within one family: two parked intros, the earlier expiry wins, and
    // the later one must not displace it.
    let d = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11));
    assert_eq!(
        d.deadline,
        Some(t + INTRO_TTL),
        "a parked intro's expiry is the endpoint's only deadline"
    );
    let later = t + Duration::from_secs(2);
    let d = b.feed(later, v4(5, 6), &forged_init(&b, 2, 0x22));
    assert_eq!(
        d.deadline,
        Some(t + INTRO_TTL),
        "a later timer displaced the minimum"
    );

    // And when the earlier one fires, the deadline moves out to the later.
    let d = b.timeout(t + INTRO_TTL);
    assert_eq!(
        d.deadline,
        Some(later + INTRO_TTL),
        "the surviving intro's expiry was not announced"
    );

    // Across families: a pending's retransmit is sooner than any intro
    // expiry (5 s + U[0, 333 ms] < 15 s), so it takes the minimum.
    let d = a.feed(t, v4(6, 6), &forged_init(&a, 1, 0x33));
    assert_eq!(d.deadline, Some(t + INTRO_TTL));

    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);
    let retransmit = d.deadline.expect("a pending arms a deadline");
    assert!(
        retransmit >= t + RETRANSMIT_BASE,
        "§5.5 step 2: never earlier than RETRANSMIT_BASE"
    );
    assert!(
        retransmit <= t + RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX,
        "§5.5 step 2: never later than RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX"
    );
    assert!(
        retransmit < t + INTRO_TTL,
        "fixture: 5 s must be inside 15 s for this to test a minimum"
    );
}

/// §16.5 (4538–4540): "`handle_timeout` is idempotent: each due timer is
/// stopped before its logic runs, so spurious or repeated calls no-op."
///
/// Asserted on the one due timer this slice can fire twice — a retransmit
/// — because an implementation that re-fires produces a second msg1 in one
/// interval, which is §5.5 step 2's whole point.
#[test]
fn handle_timeout_twice_at_the_same_instant_is_a_no_op() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);
    let due = d.deadline.expect("retransmit armed");

    let first = a.timeout(due);
    assert_eq!(first.transmits().len(), 1, "the retransmit fires once");

    let second = a.timeout(due);
    assert!(
        second.is_silent(),
        "a repeated handle_timeout at the same instant emitted {:?}",
        second.outs
    );
    assert_eq!(
        second.deadline, first.deadline,
        "and did not re-arm anything"
    );
}

/// A `handle_timeout` at an instant when nothing is due emits nothing and
/// leaves the deadline where it was — the other side of idempotency.
#[test]
fn handle_timeout_before_any_deadline_is_a_no_op() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let d = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11));
    let deadline = d.deadline.expect("intro expiry armed");

    let early = b.timeout(t + INTRO_TTL - Duration::from_nanos(1));
    assert!(early.is_silent(), "{:?}", early.outs);
    assert_eq!(early.deadline, Some(deadline));
}

/// An endpoint with no pending, no parked intro and no guard orphan has no
/// deadline at all. `Timeout(None)` is §16.4's "drained and no deadline
/// armed", and a core that always returns `Some` spins a driver forever.
#[test]
fn an_idle_endpoint_announces_no_deadline() {
    let t = t0();
    let (mut a, _b) = pair(t);
    let d = a.drain();
    assert_eq!(d.deadline, None, "an idle endpoint arms nothing");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. The DH cost ladder — §6.1. Cumulative, never per-call (§6.1's dagger
//    note: a pre-read or frozen entry returns cached results at 0
//    incremental DH while "the cumulative table is unchanged").
// ═══════════════════════════════════════════════════════════════════════

/// §6.1 row 1: `Intro` costs "1 keyed hash, **0 DH**". S6.
///
/// At the ratified cap, so the assertion is also that a full-queue flood
/// buys the attacker no curve work at all.
#[test]
fn park_costs_no_dh() {
    let t = t0();
    let (_a, mut b) = pair(t);

    for n in 0..INTRO_QUEUE_CAP {
        let src = v4_nth(
            n / INTRO_MAX_PER_SOURCE,
            1000 + (n % INTRO_MAX_PER_SOURCE) as u16,
        );
        let d = b.feed(t, src, &forged_init(&b, n as u32 + 1, 0x33));
        assert_eq!(d.intros().len(), 1, "arrival {n} did not surface");
    }
    assert_eq!(
        b.dhs.get(),
        0,
        "parking {INTRO_QUEUE_CAP} initiations spent DH"
    );
}

/// §6.1: "Dropping the object at any stage is a silent reject: no msg2,
/// nothing transmitted, the slot freed." At `Intro` that is 0 DH. S6.
#[test]
fn reject_at_intro_costs_no_dh() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let id = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11)).one_intro().0;
    assert_eq!(b.dhs.get(), 0);

    b.ep.reject(t, id);
    let d = b.drain();
    assert_eq!(b.dhs.get(), 0, "reject at Intro spent DH");
    assert!(
        d.transmits().is_empty(),
        "a rejection transmitted something"
    );
    assert!(d.is_silent(), "a rejection is silent: {:?}", d.outs);
    assert!(!b.present(id), "the slot was not freed");
}

/// §6.1 row 2: `read_identity()` → `Claimed` is **1 DH** (`es`) and
/// reveals the *claimed* static. S7.
#[test]
fn read_identity_costs_one_dh() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    assert_eq!(b.dhs.get(), 0, "the arrival itself is free");

    let claimed = b.ep.read_identity(t, id).expect("a real msg1 is readable");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 1, "read_identity is exactly one DH");
    assert_eq!(
        claimed.as_ref(),
        a.canonical(),
        "the claimed static is the initiator's"
    );
}

/// §6.1's reject table, row 2: rejecting at `Claimed` costs **1 DH** — the
/// `ss` never runs. S7.
#[test]
fn reject_at_claimed_costs_one_dh() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();

    b.ep.reject(t, id);
    let d = b.drain();
    assert_eq!(b.dhs.get(), 1, "reject at Claimed ran a second DH");
    assert!(d.transmits().is_empty());
    assert!(!b.present(id));
}

/// §6.1 row 3: `authenticate()` → `Proven` is **2 DH cumulative** (+`ss`),
/// and yields possession plus the initiation timestamp. S9.
#[test]
fn authenticate_costs_two_dh_cumulative() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();

    let (peer, _ts) = b.ep.authenticate(t, id).expect("a real msg1 authenticates");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 2, "cumulative cost at Proven is 2");
    assert_eq!(peer.as_ref(), a.canonical(), "the proven static");
}

/// §6.1's reject table, row 3: rejecting at `Proven` costs **2 DH**,
/// transmits nothing, and installs nothing. S9's "prove, then still
/// decline".
#[test]
fn reject_at_proven_costs_two_dh_and_installs_nothing() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();

    b.ep.reject(t, id);
    let d = b.drain();
    assert_eq!(b.dhs.get(), 2, "reject at Proven ran ee/se");
    assert!(d.transmits().is_empty(), "a declined accept sent msg2");
    assert!(
        d.installs().is_empty(),
        "a declined accept installed a session"
    );
    assert!(!b.present(id));
}

/// §6.1 row 4: `accept()` → `Connection` is **4 DH cumulative** (+`ee`,
/// `se`) and sends msg2.
///
/// Named for the **fast path** deliberately: §6.1 (1054–1056) adds the
/// admitted candidate's `es` + `ss` on top of the 4 for a re-homed
/// `accept()` (§6.4, slice 7), so slice 7 adds a second test rather than
/// editing this one.
#[test]
fn accept_fast_path_costs_four_dh() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();

    let (_conn, _core) = b.ep.accept(t, id).expect("a fresh static accepts");
    let d = b.drain();
    assert_eq!(b.dhs.get(), 4, "the accept fast path is exactly 4 DH");

    let (to, data) = d.one_transmit();
    assert_eq!(to, a.addr, "§5.6 anchors at the msg1 source address");
    assert_eq!(data.len(), RESP_PACKET_LEN, "§3.3, exact (ruling 65)");
    assert_eq!(data[0], PKT_HANDSHAKE_RESP);
    assert_eq!(data[1], VERSION);

    // §16.4 (4428–4431): "accept() returns a fully established connection
    // — never followed by an Install."
    assert!(
        d.installs().is_empty(),
        "accept() must not be followed by an Install (double-install)"
    );
}

/// §6.3's expiry is silent eviction, and it spends nothing: a queue that
/// paid a DH to expire an entry would hand an attacker curve work for free.
/// S6/S10.
#[test]
fn expiry_costs_no_further_dh() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let id = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11)).one_intro().0;

    let d = b.timeout(t + INTRO_TTL);
    assert_eq!(b.dhs.get(), 0, "expiry spent DH");
    assert!(d.is_silent(), "expiry is silent: {:?}", d.outs);
    assert!(!b.present(id), "the entry outlived INTRO_TTL");
}

/// §5.2's initiator ladder: `write_message_1` pays `es` + `ss` = **2**.
///
/// The counterpart to the responder table, and the half a DH-count test
/// trips over: since §5.5 makes every retransmit a completely fresh
/// initiation, **each retransmit costs 2 more**.
#[test]
fn a_dial_costs_two_dh_and_each_retransmit_two_more() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);
    assert_eq!(a.dhs.get(), 2, "write_message_1 is es + ss");

    let mut due = d.deadline.expect("retransmit armed");
    for n in 1..=3u32 {
        let d = a.timeout(due);
        assert_eq!(d.transmits().len(), 1, "retransmit {n} did not fire");
        assert_eq!(
            a.dhs.get(),
            2 * (n + 1),
            "retransmit {n} is a completely fresh initiation: 2 more DH"
        );
        due = d.deadline.expect("the next retransmit is armed");
    }
}

/// Completion pays `ee` + `se` — 2 more, so a whole dial that completes is
/// **4**, matching `testutil`'s own documented "4 per cancel-and-redial
/// cycle".
#[test]
fn a_completed_dial_costs_four_dh_end_to_end() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    assert_eq!(a.dhs.get(), 2);

    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;

    let d = a.feed(t, b.addr, &msg2);
    assert_eq!(a.dhs.get(), 4, "read_message_2 is ee + se");
    assert_eq!(d.installs().len(), 1, "completion installs exactly once");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. The stage-0 queue — §6.3, rulings 69 and 71
// ═══════════════════════════════════════════════════════════════════════

// ── the door: what never reaches the queue ─────────────────────────────

/// §3.1 is **exact** for the two handshake types (ruling 65). Slice 1's
/// one real gap was this boundary tested on one side only, so all three
/// are here: `LEN-1`, `LEN`, `LEN+1`.
#[test]
fn only_an_exactly_sized_init_reaches_the_queue() {
    let t = t0();
    let (_a, mut b) = pair(t);

    let exact = forged_init(&b, 1, 0x11);
    assert_eq!(exact.len(), INIT_PACKET_LEN);

    let short = &exact[..INIT_PACKET_LEN - 1];
    assert!(
        b.feed(t, v4(5, 5), short).is_silent(),
        "INIT_PACKET_LEN - 1 reached the queue"
    );

    let mut long = exact.clone();
    long.push(0x00);
    assert!(
        b.feed(t, v4(5, 6), &long).is_silent(),
        "INIT_PACKET_LEN + 1 reached the queue — an exact check was relaxed to a minimum"
    );

    assert_eq!(
        b.feed(t, v4(5, 7), &exact).intros().len(),
        1,
        "the exactly-sized packet must park"
    );
    assert_eq!(b.dhs.get(), 0, "none of the three cost a DH");
}

/// §3.1: "There is no negotiation, ever." A wrong version byte dies before
/// the queue — one below and one above the ratified `0x01`.
#[test]
fn a_wrong_version_never_reaches_the_queue() {
    let t = t0();
    let (_a, mut b) = pair(t);

    for bad in [VERSION.wrapping_sub(1), VERSION.wrapping_add(1)] {
        // mac1 is recomputed **after** the version byte is changed, so the
        // packet is rejected for its version and nothing else — otherwise
        // this test would pass on a core with no version check at all.
        let mut d = forged_init(&b, 1, 0x11);
        d[1] = bad;
        let (preimage, tag) = d.split_at_mut(INIT_PACKET_LEN - MAC1_LEN);
        let recomputed = b.mac1_key().tag(preimage);
        tag.copy_from_slice(&recomputed);

        assert!(
            b.feed(t, v4(5, 5), &d).is_silent(),
            "version {bad:#04x} reached the queue"
        );
    }
    assert_eq!(b.dhs.get(), 0);
}

/// §6.1 row 1: a bad mac1 is an "automatic, silent" rejection **before the
/// queue**. It must cost nothing and surface nothing.
#[test]
fn a_mac1_invalid_init_never_reaches_the_queue() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let mut d = forged_init(&b, 1, 0x11);
    let last = d.len() - 1;
    d[last] ^= 0x01;

    assert!(
        b.feed(t, v4(5, 5), &d).is_silent(),
        "a mac1-invalid initiation surfaced"
    );
    assert_eq!(b.dhs.get(), 0);
}

/// mac1 is keyed on the **recipient's** static (§4.1). An initiation
/// mac1'd for a different endpoint dies here — the same fate as garbage.
#[test]
fn an_init_mac1ed_for_another_static_never_reaches_the_queue() {
    let t = t0();
    let (a, mut b) = pair(t);
    let for_a = forged_init(&a, 1, 0x11);
    assert!(
        b.feed(t, v4(5, 5), &for_a).is_silent(),
        "an initiation keyed to another static surfaced"
    );
    assert_eq!(b.dhs.get(), 0);
}

// ── dedup: the full SocketAddr, replace-with-newest ────────────────────

/// §6.3: "same `IntroId`, newest bytes … **no second surfacing**".
#[test]
fn dedup_replaces_and_keeps_the_intro_id_without_a_second_surfacing() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let src = v4(5, 5);

    let first = b.feed(t, src, &forged_init(&b, 0xAAAA_AAAA, 0x11));
    let (id, surfaced) = first.one_intro();
    assert_eq!(surfaced, src);

    let second = b.feed(
        t + Duration::from_secs(1),
        src,
        &forged_init(&b, 0xBBBB_BBBB, 0x22),
    );
    assert!(
        second.intros().is_empty(),
        "a same-source replacement surfaced a second IntroReady"
    );
    assert!(b.present(id), "the original IntroId was dropped");
    assert_eq!(b.dhs.get(), 0);
}

/// **Ruling 71.** §6.3 requires that while an entry is unconsumed
/// "accessors reflect the newest bytes **at call time**", and §5.5 mints a
/// **new random index on every retransmit** — so a `sender_index` cached
/// when the `Intro` surfaced reports a value that is no longer on the wire
/// the moment a refresh lands.
///
/// A cached-at-surfacing implementation passes every other test in this
/// file and fails this one. That is the whole reason it exists.
#[test]
fn intro_sender_index_reflects_the_newest_bytes_after_a_refresh() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let src = v4(5, 5);

    let id = b
        .feed(t, src, &forged_init(&b, 0xAAAA_AAAA, 0x11))
        .one_intro()
        .0;
    assert_eq!(b.ep.intro_sender_index(id), Some(0xAAAA_AAAA));
    assert_eq!(b.ep.intro_source(id), Some(src));

    let _ = b.feed(
        t + Duration::from_secs(1),
        src,
        &forged_init(&b, 0xBBBB_BBBB, 0x22),
    );
    assert_eq!(
        b.ep.intro_sender_index(id),
        Some(0xBBBB_BBBB),
        "the accessor answered from a cache taken at surfacing"
    );
    assert_eq!(
        b.ep.intro_source(id),
        Some(src),
        "the source is the dedup key and cannot change under a replacement"
    );
}

/// Both accessors are `Option`-returning (§16.4, ruling 71), and the empty
/// answer is the one an unknown, a rejected and an expired id all give.
#[test]
fn the_intro_accessors_answer_none_for_an_absent_entry() {
    let t = t0();
    let (_a, mut b) = pair(t);

    let id = b.feed(t, v4(5, 5), &forged_init(&b, 7, 0x11)).one_intro().0;
    assert_eq!(b.ep.intro_source(id), Some(v4(5, 5)));
    assert_eq!(b.ep.intro_sender_index(id), Some(7));

    b.ep.reject(t, id);
    let _ = b.drain();
    assert_eq!(b.ep.intro_source(id), None, "a rejected id still answers");
    assert_eq!(b.ep.intro_sender_index(id), None);

    let gone = b.feed(t, v4(6, 6), &forged_init(&b, 8, 0x11)).one_intro().0;
    let _ = b.timeout(t + INTRO_TTL);
    assert_eq!(b.ep.intro_source(gone), None, "an expired id still answers");
    assert_eq!(b.ep.intro_sender_index(gone), None);
}

/// §6.3: "Distinct initiators behind one NAT present distinct ports, hence
/// distinct keys." The dedup key is the full `SocketAddr`, **not** the IP
/// the per-source cap is keyed on — two keys, two scopes, different on
/// purpose.
#[test]
fn the_dedup_key_is_the_full_socket_addr_not_the_source_ip() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let ip = Ipv4Addr::new(10, 9, 9, 9);

    let a1 = b
        .feed(
            t,
            SocketAddr::new(IpAddr::V4(ip), 1000),
            &forged_init(&b, 1, 0x11),
        )
        .one_intro()
        .0;
    let a2 = b
        .feed(
            t,
            SocketAddr::new(IpAddr::V4(ip), 1001),
            &forged_init(&b, 2, 0x22),
        )
        .one_intro()
        .0;

    assert_ne!(a1, a2, "two ports on one IP must be two entries");
    assert!(b.present(a1) && b.present(a2));
}

// ── the caps ───────────────────────────────────────────────────────────

/// `INTRO_QUEUE_CAP` = **1024**, endpoint-wide, asserted at, below and
/// above. §6.3, §17.5's "one budget of 1024 slots". S10.
///
/// Filling it takes ≥ 256 distinct source IPs, which is the honesty
/// clause's own number (1024 / 4) restated as an executable fact.
#[test]
fn the_queue_caps_at_1024_endpoint_wide() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let mut ids = Vec::with_capacity(INTRO_QUEUE_CAP + 1);

    // Below the cap, and at it: nothing is ever evicted.
    for n in 0..INTRO_QUEUE_CAP {
        let src = v4_nth(
            n / INTRO_MAX_PER_SOURCE,
            2000 + (n % INTRO_MAX_PER_SOURCE) as u16,
        );
        let now = t + Duration::from_millis(n as u64);
        ids.push(
            b.feed(now, src, &forged_init(&b, n as u32 + 1, 0x33))
                .one_intro()
                .0,
        );
    }
    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_QUEUE_CAP,
        "an entry was evicted at or below the cap"
    );

    // One above: exactly one eviction, and the total is still the cap.
    let overflow_src = v4_nth(9_000, 3000);
    let now = t + Duration::from_millis(INTRO_QUEUE_CAP as u64);
    let extra = b
        .feed(now, overflow_src, &forged_init(&b, 0xFFFF, 0x44))
        .one_intro()
        .0;
    let survivors = ids.iter().filter(|id| b.present(**id)).count();
    assert_eq!(
        survivors,
        INTRO_QUEUE_CAP - 1,
        "overflow evicted {survivors} entries, expected 1"
    );
    assert!(
        b.present(extra),
        "§6.3: a genuine initiation always obtains a slot"
    );
    assert!(!b.present(ids[0]), "overflow did not evict the oldest");
    assert_eq!(b.dhs.get(), 0, "1025 arrivals cost DH");
}

/// `INTRO_MAX_PER_SOURCE` = **4** per source IP, at, below and above.
#[test]
fn the_per_source_cap_is_four_chains_per_ip() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let mut ids = Vec::new();

    for n in 0..INTRO_MAX_PER_SOURCE {
        let now = t + Duration::from_secs(n as u64);
        ids.push(
            b.feed(
                now,
                v4(5, 100 + n as u16),
                &forged_init(&b, n as u32 + 1, 0x11),
            )
            .one_intro()
            .0,
        );
    }
    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_MAX_PER_SOURCE,
        "an entry was evicted at or below the per-source cap"
    );

    // A different IP is untouched by the cap — it is per source IP.
    let other = b
        .feed(t, v4(6, 100), &forged_init(&b, 99, 0x22))
        .one_intro()
        .0;

    // One above: exactly one of that IP's entries goes.
    let now = t + Duration::from_secs(INTRO_MAX_PER_SOURCE as u64);
    let fifth = b
        .feed(now, v4(5, 200), &forged_init(&b, 0xAAAA, 0x33))
        .one_intro()
        .0;
    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_MAX_PER_SOURCE - 1,
        "the per-source cap evicted the wrong number of entries"
    );
    assert!(
        b.present(fifth),
        "the arrival replaced within its own source"
    );
    assert!(b.present(other), "the cap reached across source IPs");
    assert!(
        !b.present(ids[0]),
        "the per-source cap evicted out of order"
    );
}

/// §6.3 keys the per-source cap "per **/64** for IPv6" — so five addresses
/// inside one /64 share one allowance, and an address one /64 away does
/// not. Both sides of the prefix boundary.
#[test]
fn ipv6_shares_one_cap_across_a_64() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let mut ids = Vec::new();

    for n in 0..INTRO_MAX_PER_SOURCE {
        let now = t + Duration::from_secs(n as u64);
        // Same /64, different host halves *and* different ports.
        let src = v6_in_64(0x11, n as u16 + 1, 500 + n as u16);
        ids.push(
            b.feed(now, src, &forged_init(&b, n as u32 + 1, 0x11))
                .one_intro()
                .0,
        );
    }

    // One /64 away — differs only in the last byte of the prefix.
    let neighbour = b
        .feed(t, v6_in_64(0x12, 1, 500), &forged_init(&b, 77, 0x22))
        .one_intro()
        .0;

    let now = t + Duration::from_secs(INTRO_MAX_PER_SOURCE as u64);
    let fifth = b
        .feed(now, v6_in_64(0x11, 99, 999), &forged_init(&b, 0xBBBB, 0x33))
        .one_intro()
        .0;

    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_MAX_PER_SOURCE - 1,
        "the /64 allowance was not shared"
    );
    assert!(b.present(fifth));
    assert!(
        b.present(neighbour),
        "the cap reached across a /64 boundary — the prefix is 64 bits, not fewer"
    );
}

/// §6.3: a same-`SocketAddr` arrival is a **replacement**, so it is
/// net-zero for the cap and must never evict a stranger. This is why the
/// arrival algorithm dedups *before* it checks the cap.
#[test]
fn a_dedup_replacement_is_net_zero_for_the_per_source_count() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let mut ids = Vec::new();
    for n in 0..INTRO_MAX_PER_SOURCE {
        let now = t + Duration::from_secs(n as u64);
        ids.push(
            b.feed(
                now,
                v4(5, 100 + n as u16),
                &forged_init(&b, n as u32 + 1, 0x11),
            )
            .one_intro()
            .0,
        );
    }

    // A retransmit from an address already in the queue: full source, but
    // nothing may be evicted.
    let now = t + Duration::from_secs(10);
    let d = b.feed(now, v4(5, 100), &forged_init(&b, 0xCCCC, 0x44));
    assert!(d.intros().is_empty(), "a replacement surfaced again");
    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_MAX_PER_SOURCE,
        "a replacement evicted an entry — the cap ran before the dedup"
    );
}

// ── ruling 69: one age key, last refresh ───────────────────────────────

/// **Ruling 69 — evict-oldest orders by last refresh, never by park time.**
///
/// Park A, park B, refresh A with a same-source retransmit, then overflow.
/// **B** must be the victim. Under §6.3's superseded "(by park time)"
/// wording A dies instead — which is the precise inversion the ruling
/// names: "a genuine peer retransmitting for 14 s holds the *oldest* park
/// time in the queue and is evicted first, while every attacker's
/// freshly-parked entry is younger".
#[test]
fn overflow_evicts_the_oldest_by_last_refresh_not_by_park_time() {
    let t = t0();
    let cap = 4usize;
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), capped_config(cap, 4));

    let a_src = v4(11, 11);
    let b_src = v4(12, 12);
    let c_src = v4(13, 13);
    let d_src = v4(14, 14);
    let e_src = v4(15, 15);

    let id_a = b.feed(t, a_src, &forged_init(&b, 1, 0x11)).one_intro().0;
    let id_b = b
        .feed(t + Duration::from_secs(1), b_src, &forged_init(&b, 2, 0x22))
        .one_intro()
        .0;
    let id_c = b
        .feed(t + Duration::from_secs(2), c_src, &forged_init(&b, 3, 0x33))
        .one_intro()
        .0;
    let id_d = b
        .feed(t + Duration::from_secs(3), d_src, &forged_init(&b, 4, 0x44))
        .one_intro()
        .0;

    // The genuine peer retransmits: same SocketAddr, fresh index. A is now
    // the *youngest* entry by last refresh and the *oldest* by park time.
    let refresh = b.feed(t + Duration::from_secs(4), a_src, &forged_init(&b, 5, 0x55));
    assert!(refresh.intros().is_empty(), "a refresh must not re-surface");
    assert_eq!(
        b.ep.intro_sender_index(id_a),
        Some(5),
        "the refresh did not land"
    );

    // Overflow.
    let id_e = b
        .feed(t + Duration::from_secs(5), e_src, &forged_init(&b, 6, 0x66))
        .one_intro()
        .0;

    assert!(
        b.present(id_a),
        "the refreshed entry was evicted — eviction is still ordered by park time (ruling 69)"
    );
    assert!(
        !b.present(id_b),
        "the oldest-by-last-refresh entry survived"
    );
    assert!(b.present(id_c) && b.present(id_d) && b.present(id_e));
}

/// The same ordering, on the **per-source** cap: §6.3 now reads "oldest
/// **by last refresh**, as in evict-oldest below (ruling 69)". A separate
/// test because it is a separate eviction site, and an implementation can
/// easily fix one and not the other.
#[test]
fn the_per_source_cap_evicts_the_oldest_by_last_refresh() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let ip = 5u8;

    let mut ids = Vec::new();
    for n in 0..INTRO_MAX_PER_SOURCE {
        let now = t + Duration::from_secs(n as u64);
        ids.push(
            b.feed(
                now,
                v4(ip, 100 + n as u16),
                &forged_init(&b, n as u32 + 1, 0x11),
            )
            .one_intro()
            .0,
        );
    }

    // Refresh the first — the one park time would condemn.
    let _ = b.feed(
        t + Duration::from_secs(10),
        v4(ip, 100),
        &forged_init(&b, 0xDDDD, 0x55),
    );

    // A fifth address on the same IP trips the cap.
    let fifth = b
        .feed(
            t + Duration::from_secs(11),
            v4(ip, 500),
            &forged_init(&b, 0xEEEE, 0x66),
        )
        .one_intro()
        .0;

    assert!(
        b.present(ids[0]),
        "the per-source cap evicted the refreshed entry (ruling 69)"
    );
    assert!(!b.present(ids[1]), "the oldest by last refresh survived");
    assert!(b.present(fifth));
}

// ── consumption, freezing, and the tiers ───────────────────────────────

/// §6.3: "The moment `read_identity()` runs, the chain owns its bytes and
/// its `IntroId`: the source's stage-0 slot is freed, a subsequent
/// initiation parks as a **new** entry, and a consumed chain can **never**
/// be superseded by any later packet."
#[test]
fn read_identity_frees_the_stage0_slot_and_the_chain_is_never_byte_replaced() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let original_index = init_sender_index(&msg1);

    let consumed = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, consumed).expect("readable");
    let _ = b.drain();

    // A later initiation from the very same SocketAddr.
    let later = b.feed(
        t + Duration::from_secs(1),
        a.addr,
        &forged_init(&b, 0x1234_5678, 0x77),
    );
    let (fresh, src) = later.one_intro();
    assert_eq!(src, a.addr, "the slot was not freed for a new entry");
    assert_ne!(
        fresh, consumed,
        "the later initiation reused the consumed IntroId"
    );

    assert_eq!(
        b.ep.intro_sender_index(consumed),
        Some(original_index),
        "an unauthenticated mac1-valid packet clobbered DH-paid work"
    );
    assert_eq!(b.ep.intro_sender_index(fresh), Some(0x1234_5678));
}

/// §6.3: "`read_identity()` is **net-zero** for its source's count
/// (−1 unconsumed, +1 consumed)", and the cap counts "the **sum** of
/// unconsumed stage-0 entries and consumed chains".
///
/// So consuming does not buy a source a fifth slot: the arrival after four
/// still has to evict from the unconsumed tier.
#[test]
fn the_per_source_cap_counts_consumed_and_unconsumed_together() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let ip = 5u8;

    // Four chains from one IP, all readable because they are real bytes.
    let mut ids = Vec::new();
    for n in 0..INTRO_MAX_PER_SOURCE {
        let now = t + Duration::from_secs(n as u64);
        ids.push(b.feed(now, v4(ip, 100 + n as u16), &msg1).one_intro().0);
    }

    // Consume two of them — net zero for the count.
    // Ruling 92 gave `read_identity` a `now`; the loop's `now` is out of
    // scope here, so its last value is named rather than re-derived.
    let consumed_at = t + Duration::from_secs((INTRO_MAX_PER_SOURCE - 1) as u64);
    b.ep.read_identity(consumed_at, ids[0]).expect("readable");
    let _ = b.drain();
    b.ep.read_identity(consumed_at, ids[1]).expect("readable");
    let _ = b.drain();
    // `a` and `b` hold separate counters, so this is the responder's own
    // spend: two `es`, one per consumed chain.
    assert_eq!(b.dhs.get(), 2, "read_identity is exactly one es per chain");

    // The fifth arrival must still trip the cap.
    let fifth = b
        .feed(
            t + Duration::from_secs(9),
            v4(ip, 900),
            &forged_init(&b, 0xABCD, 0x11),
        )
        .one_intro()
        .0;

    let live = ids.iter().filter(|id| b.present(**id)).count();
    assert_eq!(
        live,
        INTRO_MAX_PER_SOURCE - 1,
        "consuming an entry bought the source an extra slot"
    );
    assert!(b.present(fifth));
    assert!(
        b.present(ids[0]) && b.present(ids[1]),
        "eviction operates on the unconsumed tier only — a DH-paid chain was evicted"
    );
}

/// §6.3: "if the source's whole allowance is held by **consumed chains**,
/// the arrival is dropped." Silently: no `IntroReady`, no eviction.
#[test]
fn an_all_consumed_source_drops_the_arrival() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let ip = 5u8;

    let mut ids = Vec::new();
    for n in 0..INTRO_MAX_PER_SOURCE {
        let id = b.feed(t, v4(ip, 100 + n as u16), &msg1).one_intro().0;
        b.ep.read_identity(t, id).expect("readable");
        let _ = b.drain();
        ids.push(id);
    }

    let d = b.feed(
        t + Duration::from_secs(1),
        v4(ip, 900),
        &forged_init(&b, 0xABCD, 0x11),
    );
    assert!(
        d.is_silent(),
        "the arrival was admitted or announced: {:?}",
        d.outs
    );
    assert_eq!(
        ids.iter().filter(|id| b.present(**id)).count(),
        INTRO_MAX_PER_SOURCE,
        "a consumed chain was evicted to make room"
    );

    // A different IP is unaffected — the drop is per source.
    assert_eq!(
        b.feed(
            t + Duration::from_secs(1),
            v4(6, 100),
            &forged_init(&b, 1, 0x22)
        )
        .intros()
        .len(),
        1
    );
}

/// §6.3: "**Consumed chains are never evicted by overflow.**" The global
/// cap must reach past a DH-paid chain to find an unconsumed victim.
#[test]
fn overflow_never_evicts_a_consumed_chain() {
    let t = t0();
    let mut a = Ep::new(t, 7, 0x11, v4(1, 1), default_config());
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), capped_config(2, 2));
    let msg1 = real_msg1(&mut a, t, &b);

    // The oldest entry, consumed.
    let consumed = b.feed(t, v4(11, 11), &msg1).one_intro().0;
    b.ep.read_identity(t, consumed).expect("readable");
    let _ = b.drain();

    // A younger unconsumed one fills the queue.
    let unconsumed = b
        .feed(
            t + Duration::from_secs(1),
            v4(12, 12),
            &forged_init(&b, 2, 0x22),
        )
        .one_intro()
        .0;

    // Overflow.
    let arrival = b
        .feed(
            t + Duration::from_secs(2),
            v4(13, 13),
            &forged_init(&b, 3, 0x33),
        )
        .one_intro()
        .0;

    assert!(b.present(consumed), "overflow evicted a DH-paid chain");
    assert!(
        !b.present(unconsumed),
        "overflow spared the unconsumed tier"
    );
    assert!(b.present(arrival));
}

/// **Not a ratified rule — `PLAN.md` §6.2 step 3's derivation.** §6.3
/// states the *per-source* all-consumed drop and never states the global
/// one; the plan derives "drop" as the only option that respects §17.5's
/// ceiling of one budget of `INTRO_QUEUE_CAP` slots.
///
/// Pinned so the derivation is visible and testable rather than latent. If
/// a ruling goes the other way this test changes with it — it is
/// deliberately the only place the derived behaviour is asserted.
#[test]
fn a_wholly_consumed_queue_drops_the_arrival() {
    let t = t0();
    let mut a = Ep::new(t, 7, 0x11, v4(1, 1), default_config());
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), capped_config(2, 2));
    let msg1 = real_msg1(&mut a, t, &b);

    let mut ids = Vec::new();
    for n in 0..2u8 {
        let id = b.feed(t, v4(20 + n, 11), &msg1).one_intro().0;
        b.ep.read_identity(t, id).expect("readable");
        let _ = b.drain();
        ids.push(id);
    }

    let d = b.feed(
        t + Duration::from_secs(1),
        v4(30, 30),
        &forged_init(&b, 9, 0x99),
    );
    assert!(d.is_silent(), "the arrival was admitted: {:?}", d.outs);
    assert!(ids.iter().all(|id| b.present(*id)));
}

// ── expiry ─────────────────────────────────────────────────────────────

/// `INTRO_TTL` = 15 s from last refresh, asserted **below** and **above**
/// the boundary. The exactly-at-`D` case is a separate test so that an
/// off-by-one at the boundary does not take this one down with it.
#[test]
fn an_intro_lives_until_intro_ttl_and_not_past_it() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let id = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11)).one_intro().0;

    let _ = b.timeout(t + INTRO_TTL - Duration::from_nanos(1));
    assert!(b.present(id), "the entry expired before INTRO_TTL");

    let _ = b.timeout(t + INTRO_TTL + Duration::from_nanos(1));
    assert!(!b.present(id), "the entry outlived INTRO_TTL");
}

/// §16.5: "Every armed deadline `D` fires **no earlier than `D`**" — so at
/// exactly `D` it fires. Separated from the test above because it is the
/// half a `>` / `>=` slip lands on.
#[test]
fn an_intro_expires_at_exactly_intro_ttl() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let id = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11)).one_intro().0;

    let d = b.timeout(t + INTRO_TTL);
    assert!(!b.present(id), "the deadline did not fire at D");
    assert!(d.is_silent(), "expiry is silent eviction: {:?}", d.outs);
    assert_eq!(d.deadline, None, "an emptied queue arms nothing");
}

/// §6.3: `INTRO_TTL` runs "15 s after the entry's **last refresh**", and
/// replacement "refreshes its TTL". Both sides of the *refreshed*
/// deadline, and the negative: the original deadline must no longer fire.
#[test]
fn a_refresh_moves_the_expiry_to_fifteen_seconds_after_it() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let src = v4(5, 5);
    let id = b.feed(t, src, &forged_init(&b, 1, 0x11)).one_intro().0;

    let refreshed_at = t + Duration::from_secs(10);
    let d = b.feed(refreshed_at, src, &forged_init(&b, 2, 0x22));
    assert_eq!(
        d.deadline,
        Some(refreshed_at + INTRO_TTL),
        "the refresh did not move the deadline"
    );

    let _ = b.timeout(t + INTRO_TTL);
    assert!(b.present(id), "the original park-time deadline still fired");

    let _ = b.timeout(refreshed_at + INTRO_TTL - Duration::from_nanos(1));
    assert!(b.present(id));

    let _ = b.timeout(refreshed_at + INTRO_TTL);
    assert!(!b.present(id), "the refreshed deadline did not fire");
}

/// §6.3: "A consumed chain's mid-state expires **15 s after the initiation
/// that fed it**" — measured from the initiation, never from consumption.
/// Consuming at 10 s must not buy the chain until 25 s.
#[test]
fn a_consumed_chain_expires_fifteen_seconds_after_its_initiation() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;

    let consumed_at = t + Duration::from_secs(10);
    b.ep.read_identity(consumed_at, id).expect("readable");
    let _ = b.timeout(consumed_at);

    assert!(b.present(id), "the chain died before its initiation's TTL");
    let _ = b.timeout(t + INTRO_TTL - Duration::from_nanos(1));
    assert!(b.present(id));

    let _ = b.timeout(t + INTRO_TTL);
    assert!(
        !b.present(id),
        "consumption restarted the TTL — a consumed chain's clock runs from the initiation"
    );

    // And the verbs on it say so.
    assert!(matches!(
        b.ep.authenticate(t + INTRO_TTL, id),
        Err(AuthError::Expired)
    ));
}

/// §6.3: "staged verbs on an expired attempt return `IntroError::Expired`
/// (`AuthError::Expired` at that stage)", and §16.4 gives `reject` no
/// `Result` at all — it is a no-op, infallible.
#[test]
fn the_staged_verbs_on_an_expired_id_report_expired() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let id = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11)).one_intro().0;
    let after = t + INTRO_TTL;
    let _ = b.timeout(after);

    assert!(matches!(
        b.ep.read_identity(after, id),
        Err(IntroError::Expired)
    ));
    let _ = b.drain();
    assert!(matches!(
        b.ep.authenticate(after, id),
        Err(AuthError::Expired)
    ));
    let _ = b.drain();
    // §1.4 of the plan: `AcceptError` has no `Expired`, and `Stale` is the
    // documented "no initiation is parked for this static".
    assert!(matches!(b.ep.accept(after, id), Err(AcceptError::Stale)));
    let _ = b.drain();
    b.ep.reject(after, id); // infallible, no-op
    assert!(b.drain().is_silent());
}

/// The leak canary. A per-source counter that fails to decrement on expiry
/// is a **permanent** denial for that IP and is invisible to every other
/// test in this file: the queue looks empty and still refuses.
#[test]
fn per_source_counters_return_to_zero_when_the_queue_drains() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let ip = 5u8;

    for round in 0..3u64 {
        let base = t + Duration::from_secs(round * 100);
        let mut ids = Vec::new();
        let mut last_park = base;
        for n in 0..INTRO_MAX_PER_SOURCE {
            let park = base + Duration::from_millis(n as u64);
            last_park = park;
            let d = b.feed(
                park,
                v4(ip, 100 + n as u16),
                &forged_init(&b, n as u32 + 1, 0x11),
            );
            assert_eq!(
                d.intros().len(),
                1,
                "round {round} arrival {n} was refused — the counter leaked"
            );
            ids.push(d.one_intro().0);
        }

        // §6.3 runs `INTRO_TTL` from **each entry's own** last refresh, so
        // the sweep has to clear the *last* park, not the first: the four
        // arrivals are staggered by a millisecond each and their deadlines
        // are staggered with them. Deriving the instant from `last_park`
        // rather than writing the stagger out keeps this correct if
        // `INTRO_MAX_PER_SOURCE` ever moves.
        let d = b.timeout(last_park + INTRO_TTL);
        assert!(d.is_silent());
        assert!(
            ids.iter().all(|id| !b.present(*id)),
            "round {round} left entries behind"
        );
    }
}

/// S10's queue half: while the queue is saturated, an established
/// connection's index still routes. §17.5 — "established connections
/// themselves keep running regardless; they hold no queue slot."
#[test]
fn an_established_index_still_routes_while_the_queue_is_saturated() {
    let t = t0();
    let (mut a, mut b) = pair(t);

    // Establish one connection, responder side.
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    let (conn, _core) = b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;
    let (our_index, _theirs) = resp_indices(&msg2);

    let before = b.dhs.get();

    // Saturate.
    for n in 0..INTRO_QUEUE_CAP {
        let src = v4_nth(
            n / INTRO_MAX_PER_SOURCE,
            4000 + (n % INTRO_MAX_PER_SOURCE) as u16,
        );
        let _ = b.feed(t, src, &forged_init(&b, n as u32 + 1, 0x33));
    }
    assert_eq!(b.dhs.get(), before, "the flood spent DH");

    // The established index still routes.
    let (disp, _d) = b.datagram(t, a.addr, &data_packet(our_index, 1, 64));
    assert_eq!(
        disp,
        Disposition::ForConnection(conn),
        "a saturated queue stopped routing an established connection"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. The timestamp guard — §17.1, §17.2, ruling 70
// ═══════════════════════════════════════════════════════════════════════

/// `n` genuine msg1s from one initiator to one responder, in strictly
/// increasing timestamp order.
///
/// Taken off the retransmit train, because §5.5 step 2 makes every
/// retransmit "a completely fresh initiation — new ephemeral, new random
/// index, **new strictly-greater timestamp**". That gives the guard tests
/// an ordered supply of real, mac1-valid, authenticable bytes without
/// needing to control a clock.
fn msg1_train(a: &mut Ep, t: Instant, b: &Ep, n: usize) -> Vec<Vec<u8>> {
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);
    let mut out = vec![d.one_transmit().1];
    let mut due = d.deadline.expect("a pending arms a retransmit");
    while out.len() < n {
        let d = a.timeout(due);
        out.push(d.one_transmit().1);
        due = d.deadline.expect("the next retransmit is armed");
    }
    out
}

/// Park `msg1` from `src`, read it, and authenticate it. The three-step
/// ladder every guard test runs.
fn ladder_to_proven(
    b: &mut Ep,
    now: Instant,
    src: SocketAddr,
    msg1: &[u8],
) -> (IntroId, Result<Timestamp, AuthError>) {
    let id = b.feed(now, src, msg1).one_intro().0;
    b.ep.read_identity(now, id)
        .expect("a real msg1 is readable");
    let _ = b.drain();
    let r = b.ep.authenticate(now, id).map(|(_pk, ts)| ts);
    let _ = b.drain();
    (id, r)
}

/// §17.1: the guard "rejects any candidate whose timestamp is **≤** the
/// greatest this endpoint has admitted for that static", and §5.6 states
/// it as "strictly greater, else drop".
///
/// All three sides of the comparison: **older** rejects, **equal**
/// rejects, **strictly newer** admits. Equal is the one an implementation
/// written with `<` instead of `<=` gets wrong, and it is the case a
/// replayed capture actually presents.
#[test]
fn the_guard_admits_only_a_strictly_greater_timestamp() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 3);

    let (_id, mid) = ladder_to_proven(&mut b, t, v4(20, 1), &train[1]);
    let ts_mid = mid.expect("the first admission passes vacuously");

    let (_id, older) = ladder_to_proven(&mut b, t, v4(20, 2), &train[0]);
    assert!(
        matches!(older, Err(AuthError::Replay)),
        "an older timestamp was admitted"
    );

    let (_id, equal) = ladder_to_proven(&mut b, t, v4(20, 3), &train[1]);
    assert!(
        matches!(equal, Err(AuthError::Replay)),
        "an EQUAL timestamp was admitted — the comparison is not strict"
    );

    let (_id, newer) = ladder_to_proven(&mut b, t, v4(20, 4), &train[2]);
    let ts_new = newer.expect("a strictly greater timestamp must be admitted");
    assert!(ts_new > ts_mid, "the train is not monotone");
}

/// §17.1 mitigation **(i) no-orphan-on-reject**: "a static authenticated
/// and then rejected without ever being accepted writes **no orphan**".
///
/// The failure is silent and permanent — a stuck record blocks that peer's
/// next genuine initiation forever — so it is asserted directly.
#[test]
fn authenticate_then_reject_leaves_the_guard_empty() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    let (id, first) = ladder_to_proven(&mut b, t, v4(21, 1), &train[0]);
    first.expect("first admission");
    b.ep.reject(t, id);
    let _ = b.drain();

    // The very same initiation, replayed: it must pass vacuously again,
    // because the provisional record dropped with the chain.
    let (_id, again) = ladder_to_proven(&mut b, t, v4(21, 2), &train[0]);
    assert!(
        again.is_ok(),
        "the rejected chain left an orphan record behind: {again:?}"
    );
}

/// The other half of mitigation (i) — "**a pre-existing entry reverts**".
///
/// Without this test an implementation that *removes* the entry outright
/// passes [`authenticate_then_reject_leaves_the_guard_empty`] and still
/// destroys a real peer's replay protection.
#[test]
fn authenticate_then_reject_restores_a_prior_value() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 3);

    // A permanent record at train[0]: accepted, so it is not provisional.
    let (id0, admitted) = ladder_to_proven(&mut b, t, v4(22, 1), &train[0]);
    admitted.expect("first admission");
    b.ep.accept(t, id0).expect("a fresh static accepts");
    let _ = b.drain();

    // A provisional record at train[2], then rejected.
    let (id2, high) = ladder_to_proven(&mut b, t, v4(22, 2), &train[2]);
    high.expect("train[2] is strictly greater than train[0]");
    b.ep.reject(t, id2);
    let _ = b.drain();

    // Not empty: train[0] is still refused as an equal replay.
    let (_id, replay) = ladder_to_proven(&mut b, t, v4(22, 3), &train[0]);
    assert!(
        matches!(replay, Err(AuthError::Replay)),
        "the revert removed the pre-existing entry instead of restoring it"
    );

    // And not stuck at train[2]: the middle value is admitted.
    let (_id, middle) = ladder_to_proven(&mut b, t, v4(22, 4), &train[1]);
    assert!(
        middle.is_ok(),
        "the rejected chain's record survived: {middle:?}"
    );
}

/// **Regression — the two-pin sibling of
/// [`authenticate_then_reject_leaves_the_guard_empty`].**
///
/// §17.1 mitigation (i) says the rejected chain's record "**drops with the
/// chain**". That is a statement about the *record*, not about the entry,
/// and the two come apart the moment anything else pins the entry: a
/// second staged chain for the same static holds a pin, the entry
/// therefore survives the drop, and a revert that only deletes *unpinned*
/// entries lets the admitted value survive with it.
///
/// The cost of getting it wrong is the exact flood mitigation (i) exists
/// to forbid — with a second pin held, authenticate-then-drop mints a
/// durable orphan for a handful of DH, and §17.1's honesty clause prices
/// flushing the whole `TS_GUARD_ORPHAN_CAP` tier on the assumption that it
/// cannot.
///
/// The **single**-pin case passes either way, so the whole of the defect
/// lives in the difference between that test and this one. Asserted twice:
/// through the record directly, and through the admission behaviour that
/// actually matters.
#[test]
fn authenticate_then_reject_clears_the_record_even_when_another_chain_pins_the_entry() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    // Chain A authenticates: the key-holder write that creates the entry
    // and records train[1]'s timestamp.
    let (chain_a, admitted) = ladder_to_proven(&mut b, t, v4(31, 1), &train[1]);
    let ts = admitted.expect("the first admission passes vacuously");
    assert_eq!(
        b.ep.greatest(a.canonical()),
        Some(ts),
        "authenticate did not record the timestamp"
    );

    // Chain B reaches Claimed and stops there: no guard check, no record,
    // but a **second pin** on the entry a key-holder just wrote (§17.1's
    // bounded exception).
    let chain_b = b.feed(t, v4(31, 2), &train[0]).one_intro().0;
    b.ep.read_identity(t, chain_b)
        .expect("a real msg1 is readable");
    let _ = b.drain();

    // Drop A. The entry survives — B pins it — but the RECORD must not.
    b.ep.reject(t, chain_a);
    let _ = b.drain();
    assert_eq!(
        b.ep.greatest(a.canonical()),
        None,
        "the record outlived the chain that wrote it because a second pin held the entry"
    );

    // The property that matters: the very same initiation is admissible
    // again, so the authenticate-then-drop minted no orphan.
    let (_id, again) = ladder_to_proven(&mut b, t, v4(31, 3), &train[1]);
    assert!(
        again.is_ok(),
        "authenticate-then-drop minted an orphan while a second chain pinned the entry: {again:?}"
    );
}

/// §17.1: for a staged mid-state "the pin **never creates an entry** — a
/// bounded exception to §6.1's nothing-durable rule, flipping a bit on an
/// entry a key-holder already wrote".
///
/// This is the rule every pin-accounting bug is measured against, and it
/// is §6.1's "nothing durable may be keyed on the **claimed** static"
/// applied to the guard: reaching `Claimed` costs 1 DH and proves
/// nothing, so if it could mint a guard entry an attacker would write
/// durable per-static state for any public key they care to name.
#[test]
fn a_claimed_chain_creates_no_guard_entry() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    // Reach Claimed and stop. train[1] is the *newer* initiation.
    let id = b.feed(t, v4(32, 1), &train[1]).one_intro().0;
    b.ep.read_identity(t, id).expect("a real msg1 is readable");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 1, "Claimed is one DH");
    assert_eq!(
        b.ep.greatest(a.canonical()),
        None,
        "reaching Claimed wrote a guard entry for an unproven, attacker-choosable static"
    );

    // Behaviourally: nothing was recorded, so the OLDER initiation is
    // still admissible. Had `Claimed` recorded train[1], this would be a
    // Replay.
    let (_id, older) = ladder_to_proven(&mut b, t, v4(32, 2), &train[0]);
    assert!(
        older.is_ok(),
        "an older initiation was refused, so Claimed recorded something: {older:?}"
    );
}

/// **Regression — a pin is never *released* that was never *taken*.**
///
/// §17.1's write sites are all post-`ss` reads of an **inbound** msg1, so
/// "for a static we only ever dialled we hold **no entry at all**"; and a
/// pin never creates one. Dialling a static nobody has recorded therefore
/// takes **no** pin, and the release when that dial ends must be
/// conditional on the take. An unconditional release decrements whatever
/// pin exists by then — and on §5.4's **PENDING** row (simultaneous open)
/// that is a **staged mid-state's**, so §17.1's "never evicted while a
/// staged mid-state exists" fails silently.
///
/// This test walks the defect's own path: the dial creates no entry, an
/// inbound chain for that same static then creates one, and cancelling the
/// dial must leave it and its record untouched.
///
/// **Why the oracle is [`Endpoint::guard_pins`] and not behaviour.** The
/// over-release leaves *no behavioural trace at all*, and ruling 70's
/// alias is the reason. `TS_GUARD_ORPHAN_TTL == INTRO_TTL`; the only pin
/// holder that can co-exist with a cancelled dial for the same static is a
/// staged mid-state (§16.1 forbids LIVE and PENDING together, and slice 2a
/// refuses `accept()` on both rows); and that mid-state's own expiry
/// reverts its provisional record at exactly the instant the
/// wrongly-unpinned entry would have aged out. The two clocks are one
/// clock, so a fixed core and a broken core give the same answer to every
/// question the protocol surface can ask. The count is the only place they
/// differ, which is why it is asserted at all three steps below rather
/// than once at the end.
#[test]
fn cancelling_a_dial_does_not_release_a_pin_it_never_took() {
    let t = t0();
    let (mut a, mut b) = pair(t);

    // (1) Dial a static we hold nothing for. §17.1: no entry to pin, so
    //     no pin is taken — the defect's precondition.
    let (conn, d) = a.connect(t, b.addr, &b.public_static);
    let index = init_sender_index(&d.one_transmit().1);
    assert_eq!(
        a.ep.greatest(b.canonical()),
        None,
        "a dial created a guard entry — every §17.1 write site is inbound and post-ss"
    );
    assert_eq!(
        a.ep.guard_pins(b.canonical()),
        0,
        "a dial took a pin on a static we hold no entry for"
    );

    // (2) The simultaneous-open shape: an inbound initiation from that
    //     same static reaches Proven. *That* is the key-holder write which
    //     creates the entry, and it takes the mid-state's pin.
    let inbound = real_msg1(&mut b, t, &a);
    let at = t + Duration::from_secs(1);
    // Two adjustments, both forced by §6.5/§6.6 and neither touching an
    // assertion. (a) From an address A never dialled: a crossing msg1 that
    // arrives at the **dialled** address goes to §6.6's internal tie-break,
    // where no staged mid-state is ever created. (b) Reached through
    // ruling 75's `authenticate()` rather than the full ladder: §6.5
    // step 4 intercepts `read_identity()` when the claim is a pending
    // outbound remote, and §18.1 gives `authenticate()` no way to report
    // an interception, so that verb is the route to `Proven` that survives
    // a live dial. Both leave this test's shape — a dial and a staged
    // mid-state coexisting on one static — exactly as it was.
    let chain = a.feed(at, v4(2, 3), &inbound).one_intro().0;
    let admitted = a.ep.authenticate(at, chain).map(|(_pk, ts)| ts);
    let _ = a.drain();
    let ts = admitted.expect("we hold no entry for a static we dialled, so this passes vacuously");
    assert_eq!(a.ep.greatest(b.canonical()), Some(ts));
    assert_eq!(
        a.ep.guard_pins(b.canonical()),
        1,
        "the mid-state's pin is the only one: the in-flight dial must not have \
         acquired one retroactively when the entry appeared"
    );

    // (3) End the dial. It never pinned this entry and never wrote this
    //     record, so it may disturb neither.
    a.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: index });
    let _ = a.drain();
    assert_eq!(
        a.ep.guard_pins(b.canonical()),
        1,
        "cancelling the dial released a pin it never took — §17.1's \
         \"never evicted while a staged mid-state exists\" is now silently broken"
    );
    assert_eq!(
        a.ep.greatest(b.canonical()),
        Some(ts),
        "cancelling the dial destroyed a record it never wrote"
    );

    // Behaviourally, and at the far edge of the mid-state's life: the
    // record is still enforced, so a replay of that same initiation is
    // still refused.
    let alive = at + INTRO_TTL - Duration::from_nanos(1);
    let _ = a.timeout(alive);
    let (_id, replay) = ladder_to_proven(&mut a, alive, v4(33, 1), &inbound);
    assert!(
        matches!(replay, Err(AuthError::Replay)),
        "the entry lost its record while a staged mid-state still pinned it (§17.1): {replay:?}"
    );
}

/// §17.1: "For a static we **only ever dialled** we hold **no entry at
/// all** — every §17.1 write site is a post-`ss` read of an *inbound*
/// msg1, and a `connect()` completed by msg2 writes nothing … every
/// candidate passes it vacuously, **however old**."
///
/// Made deterministic with two frozen clocks: the dialler's wall clock
/// reads far *ahead* of the peer's, so an implementation that recorded its
/// own outbound timestamp against the dialled static would refuse the
/// peer's genuine, older initiation. A `connect()` that writes a guard
/// entry is a defect.
#[test]
fn a_dialled_static_holds_no_guard_entry() {
    let t = t0();
    let mut a = Ep::new(
        t,
        7,
        0x11,
        v4(1, 1),
        frozen_clock_config(T_BASE_SECS + 10_000, 0),
    );
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), frozen_clock_config(T_BASE_SECS, 0));

    // A dials B: A's outbound timestamp is ten thousand seconds ahead.
    let peer_b = b.public_static;
    let (_conn, _d) = a.connect(t, b.addr, &peer_b);

    // B dials A with a far older timestamp; A receives it **from an
    // address A never dialled**, so §6.5's hint check misses. It is then
    // driven by ruling 75's `authenticate()` rather than the full ladder,
    // because §6.5 step 4 intercepts `read_identity()` while the claim is
    // a pending outbound remote. `authenticate()` is where §17.1's guard
    // check — the whole subject of this test — runs either way.
    let msg1_from_b = real_msg1(&mut b, t, &a);
    let id = a.feed(t, v4(2, 3), &msg1_from_b).one_intro().0;
    let r = a.ep.authenticate(t, id).map(|(_pk, ts)| ts);
    let _ = a.drain();
    assert!(
        r.is_ok(),
        "the dial wrote a guard entry for a static we only dialled: {r:?}"
    );
}

/// §5.3 / §17.2: the outbound timestamp is "forced **strictly greater**
/// than the previous timestamp this endpoint emitted".
///
/// A frozen wall clock is the only way to observe the *forcing*: against a
/// real clock a monotone result proves nothing but that time passed. The
/// increase here has exactly one possible source.
#[test]
fn two_initiations_under_a_frozen_clock_are_strictly_increasing() {
    let t = t0();
    let mut a = Ep::new(
        t,
        7,
        0x11,
        v4(1, 1),
        frozen_clock_config(T_BASE_SECS, 500_000),
    );
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), default_config());

    let train = msg1_train(&mut a, t, &b, 3);

    let (_id, first) = ladder_to_proven(&mut b, t, v4(23, 1), &train[0]);
    let t1 = first.expect("first admission");
    let (_id, second) = ladder_to_proven(&mut b, t, v4(23, 2), &train[1]);
    let t2 = second.expect("the forced increment makes the second admissible");
    let (_id, third) = ladder_to_proven(&mut b, t, v4(23, 3), &train[2]);
    let t3 = third.expect("and the third");

    assert!(
        t1 < t2 && t2 < t3,
        "a frozen clock produced non-increasing initiation timestamps: {t1:?} {t2:?} {t3:?}"
    );
}

/// §17.1: an entry is "**pinned** — never evicted — while a live
/// `Connection` … exists for its static", and pinning is what "guarantees
/// eviction never touches an established connection's replacement
/// protection". Three orphan TTLs must not move it.
#[test]
fn a_pinned_guard_entry_does_not_age_out() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    let (id, admitted) = ladder_to_proven(&mut b, t, v4(24, 1), &train[1]);
    admitted.expect("admission");
    b.ep.accept(t, id).expect("accepts");
    let _ = b.drain();

    let much_later = t + TS_GUARD_ORPHAN_TTL * 3;
    let _ = b.timeout(much_later);

    let (_id, replay) = ladder_to_proven(&mut b, much_later, v4(24, 2), &train[1]);
    assert!(
        matches!(replay, Err(AuthError::Replay)),
        "a pinned guard entry aged out under a live connection"
    );
}

/// **Ruling 70** — `TS_GUARD_ORPHAN_TTL`, an alias of `INTRO_TTL`. §17.1
/// mitigation (ii): "orphans age out on a `TS_GUARD_ORPHAN_TTL` timer as
/// well as the LRU cap", and evicting one "re-admits a replay of that
/// static's last initiation".
///
/// The admission and the un-pinning happen at the **same instant** on
/// purpose: §17.1 defines LRU "use" as the successful record (mitigation
/// (iii)) but never says whether the aging clock runs from that or from
/// the moment the entry became an orphan. Collapsing the two instants
/// makes the test pin the *duration* — 15 s, both sides — without picking
/// a side of that unstated scope.
///
/// It also pins mitigation **(iii)** as a side effect, and deliberately:
/// the below-TTL probe is a **failed** check, and "recency refreshes on a
/// successful post-`ss` record, **never on a failed check**". An
/// implementation that refreshed on failure would push the aging out past
/// `after` and fail the second half.
#[test]
fn an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    let (id, admitted) = ladder_to_proven(&mut b, t, v4(25, 1), &train[1]);
    admitted.expect("admission");
    let (conn, _core) = b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;
    let (ours, _theirs) = resp_indices(&msg2);

    // Un-pin at the same instant: the entry is an orphan from `t`. §16.4
    // (4499–4505) makes `Retired` the teardown event that releases "the
    // index route and the guard-entry pin".
    b.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: ours });
    let _ = b.drain();

    // Below the TTL: still guarded.
    let before = t + TS_GUARD_ORPHAN_TTL - Duration::from_nanos(1);
    let _ = b.timeout(before);
    let (_id, still) = ladder_to_proven(&mut b, before, v4(25, 2), &train[1]);
    assert!(
        matches!(still, Err(AuthError::Replay)),
        "the orphan aged out early"
    );

    // At and above: aged out, and the replay is re-admitted.
    let after = t + TS_GUARD_ORPHAN_TTL;
    let _ = b.timeout(after);
    let (_id, gone) = ladder_to_proven(&mut b, after, v4(25, 3), &train[1]);
    assert!(
        gone.is_ok(),
        "the orphan outlived TS_GUARD_ORPHAN_TTL: {gone:?}"
    );
}

/// §16.5 names "the timestamp-guard orphan aging (§17.1)" as one of the
/// **three** families the endpoint's min-deadline is taken over. An
/// implementation that ages orphans only opportunistically — on the next
/// unrelated timeout — passes the test above and never arms this.
///
/// Robust to ruling 70's alias: with the accepted chain consumed at `t`
/// and the guard entry orphaned at `t`, both candidate deadlines are
/// `t + INTRO_TTL = t + TS_GUARD_ORPHAN_TTL`.
#[test]
fn the_endpoint_deadline_covers_guard_orphan_aging() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 1);

    let (id, admitted) = ladder_to_proven(&mut b, t, v4(26, 1), &train[0]);
    admitted.expect("admission");
    let (conn, _core) = b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;
    let (ours, _theirs) = resp_indices(&msg2);

    b.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: ours });
    let d = b.drain();
    assert_eq!(
        d.deadline,
        Some(t + TS_GUARD_ORPHAN_TTL),
        "orphan aging is not in the endpoint's min-deadline (§16.5)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. §5.4's responder rule, at slice 2a's declared boundary
// ═══════════════════════════════════════════════════════════════════════

/// §5.4's **LIVE** row: a replacement initiation "parks as an ordinary
/// `Intro` … and surfaces through the staged accept. The live connection
/// keeps running untouched."
///
/// Plan §1.4 bounded slice 2a to the **NONE** row, returning
/// `AcceptError::Stale` for LIVE — knowingly incomplete, and pinned here
/// so the gap was visible rather than latent, and so slice 7 had *"a test
/// to change rather than a behaviour to discover"*.
///
/// **[amended by slice 7 — §5.4, §6.4]** It is changed, exactly as the
/// author's note anticipated. `b` **accepted** the first initiation, so its
/// replacement basis is `Some(t)` (§17.4) and a strictly newer candidate is
/// a replacement: `accept()` returns `Ok`, the old connection is torn down
/// with `ConnectionLost::Replaced`, and the new one takes its place.
///
/// What holds in both slices, and is still the point of the test, is
/// §16.1's **one session per peer static**: exactly one connection carries
/// this static at every instant, before and after.
#[test]
fn accept_on_a_live_static_replaces_it_against_a_newer_basis() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 2);

    let (first, admitted) = ladder_to_proven(&mut b, t, v4(27, 1), &train[0]);
    admitted.expect("admission");
    let (live, _core) = b.ep.accept(t, first).expect("the NONE row accepts");
    let msg2 = b.drain().one_transmit().1;
    let (ours, _theirs) = resp_indices(&msg2);

    // The replacement parks and surfaces like any other initiation …
    let (second, proven) = ladder_to_proven(&mut b, t, v4(27, 2), &train[1]);
    assert!(
        proven.is_ok(),
        "a replacement candidate must still reach Proven: {proven:?}"
    );

    // … and, its timestamp being strictly newer than the basis, it replaces.
    let (replacement, _core) = b
        .ep
        .accept(t, second)
        .expect("§6.4: a strictly newer candidate against a Some(t) basis replaces");
    assert_ne!(replacement, live, "a **fresh** connection, never the old one");

    let d = b.drain();
    assert_eq!(
        d.replaced(),
        vec![live],
        "§5.4: the teardown fires at the act that installs the replacement"
    );
    assert_eq!(d.transmits().len(), 1, "and msg2 goes out for the new one");
    assert!(
        d.installs().is_empty(),
        "§16.4: `accept()` returns an established connection and is never followed by an `Install`"
    );
    let replaced_at = d
        .outs
        .iter()
        .position(|o| matches!(o, EndpointOutput::Replaced(_)))
        .expect("the teardown");
    let msg2_at = d
        .outs
        .iter()
        .position(|o| matches!(o, EndpointOutput::Transmit(_)))
        .expect("msg2");
    assert!(
        replaced_at < msg2_at,
        "§16.4's generation order: the teardown, then the replacement's msg2"
    );

    // §16.1: the old connection's index still routes to **it** until its
    // own `Retired` arrives — the endpoint core never learned the index,
    // and dropping the route is that event's job (§16.4). What has already
    // moved is the static: it names the replacement now.
    let (disp, _d) = b.datagram(t, a.addr, &data_packet(ours, 1, 64));
    assert_eq!(
        disp,
        Disposition::ForConnection(live),
        "the index route outlives the teardown by exactly one event"
    );

    // And once it does arrive, the old route is gone — while the static
    // still names the replacement, which is what keeps §16.1's invariant
    // true at every instant rather than only at the ends.
    b.ep.handle_connection_event(
        t,
        live,
        ToEndpoint::Retired { our_index: ours },
    );
    let (disp, _d) = b.datagram(t, a.addr, &data_packet(ours, 2, 64));
    assert_eq!(disp, Disposition::Done, "the retired index routes nowhere");
    assert!(
        matches!(
            b.ep.mint_pending(t, a.addr, a.public_static),
            Err(ConnectError::AlreadyConnected)
        ),
        "§16.1: the static is the replacement's, and the old `Retired` did not release it"
    );
}

/// Documentation obligation #1 (`lib.rs:47–50`): "`connect()` to a static
/// that already has a live connection returns
/// `ConnectError::AlreadyConnected`." §16.1's invariant on the dial side.
#[test]
fn connect_to_a_static_with_a_live_connection_is_already_connected() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);

    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;

    let _ = a.feed(t, b.addr, &msg2);

    let peer = b.public_static;
    assert!(
        matches!(
            a.ep.mint_pending(t, b.addr, peer),
            Err(ConnectError::AlreadyConnected)
        ),
        "a second connect() to a live static succeeded"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Initiator driving — §5.5, §5.7, §17.2, §17.3
// ═══════════════════════════════════════════════════════════════════════

/// A **length-correct, index-matching, mac1-valid** HandshakeResp whose
/// Noise body is junk — the exact shape §5.5 step 3 says *takes* the
/// pending's completion attempt and then fails.
///
/// mac1 is keyed on the **recipient's** static: on the msg2 path the
/// recipient is the initiator, so the key is `initiator`'s, not the
/// responder's. Getting that backwards is the natural mistake and would
/// make every test below vacuous, so it is written once, here.
fn forged_resp(initiator: &Ep, sender_index: u32, receiver_index: u32, filler: u8) -> Vec<u8> {
    let mut d = vec![filler; RESP_PACKET_LEN];
    d[0] = PKT_HANDSHAKE_RESP;
    d[1] = VERSION;
    d[2..6].copy_from_slice(&sender_index.to_le_bytes());
    d[6..10].copy_from_slice(&receiver_index.to_le_bytes());
    let (preimage, tag) = d.split_at_mut(RESP_PACKET_LEN - MAC1_LEN);
    let t = initiator.mac1_key().tag(preimage);
    tag.copy_from_slice(&t);
    d
}

/// Run the responder's whole ladder over `msg1` arriving from `src`, and
/// return the **genuine** msg2 it emits.
///
/// This is the positive control the msg2 tests below need, and it exists
/// because [`forged_resp`] cannot be one. §5.5 rule 5 spends the
/// completion attempt *before* the crypto runs, and a forged body's
/// ephemeral is filler rather than a curve point, so hiss rejects it at
/// point decoding **before performing any DH**. A taken-and-failed attempt
/// therefore moves the DH counter by zero: *spending an attempt and
/// spending a DH are different events*, and the only sound way to ask
/// "was the attempt still there?" is to offer a msg2 that can actually
/// complete.
fn genuine_msg2(b: &mut Ep, now: Instant, src: SocketAddr, msg1: &[u8]) -> Vec<u8> {
    let id = b.feed(now, src, msg1).one_intro().0;
    b.ep.read_identity(now, id)
        .expect("a real msg1 is readable");
    let _ = b.drain();
    b.ep.authenticate(now, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(now, id).expect("a fresh static accepts");
    let (to, data) = b.drain().one_transmit();
    assert_eq!(to, src, "§5.6 anchors at the msg1 source address");
    assert_eq!(data.len(), RESP_PACKET_LEN, "§3.3, exact (ruling 65)");
    data
}

/// §5.5 step 1: "Draw a random **nonzero** `sender_index` … build msg1
/// over a fresh ephemeral with the 12-byte payload; append mac1; **send to
/// the dialled address**."
#[test]
fn connect_sends_exactly_one_init_to_the_dialled_address() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);

    let (to, data) = d.one_transmit();
    assert_eq!(to, b.addr);
    assert_eq!(data.len(), INIT_PACKET_LEN);
    assert_eq!(data[0], PKT_HANDSHAKE_INIT);
    assert_eq!(data[1], VERSION);
    assert_ne!(init_sender_index(&data), 0, "§17.3: indices are nonzero");
    assert!(
        d.installs().is_empty(),
        "a dial installs nothing until msg2 completes it"
    );
}

/// §5.5 step 2: "**Every retransmit is a completely fresh initiation** —
/// new ephemeral, new random index, new strictly-greater timestamp."
///
/// The index and the msg1 bytes are observable on the wire; the timestamp
/// is not (§5.3: Noise level 2), so it is pinned separately by
/// [`two_initiations_under_a_frozen_clock_are_strictly_increasing`] and by
/// [`the_guard_admits_only_a_strictly_greater_timestamp`], both of which
/// read it out through a responder's `authenticate()`.
#[test]
fn every_retransmit_mints_a_fresh_index_and_a_fresh_ephemeral() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);

    let mut packets = vec![d.one_transmit().1];
    let mut due = d.deadline.expect("armed");
    for _ in 0..8 {
        let d = a.timeout(due);
        packets.push(d.one_transmit().1);
        due = d.deadline.expect("armed");
    }

    let mut indices: Vec<u32> = packets.iter().map(|p| init_sender_index(p)).collect();
    assert!(indices.iter().all(|i| *i != 0), "§17.3: nonzero");
    indices.sort_unstable();
    let before = indices.len();
    indices.dedup();
    assert_eq!(indices.len(), before, "an index was reused across attempts");

    // The ephemeral, on its own. Comparing whole packets would prove
    // nothing here — the index and the timestamp differ per attempt too —
    // so the region §5.2 puts the ephemeral in is compared directly, and
    // **pairwise** rather than adjacently: a rotating pool of two keys
    // passes an adjacent-only check and is exactly as broken as reusing
    // one.
    let ephemerals: Vec<&[u8]> = packets.iter().map(|p| msg1_ephemeral(p)).collect();
    for (i, e) in ephemerals.iter().enumerate() {
        for (j, f) in ephemerals.iter().enumerate().skip(i + 1) {
            assert_ne!(
                e, f,
                "attempts {i} and {j} share an ephemeral public key — \
                 §5.5 step 2 requires a fresh ephemeral for every initiation"
            );
        }
    }
}

/// §5.7: `RETRANSMIT_BASE` + jitter = **5 s + U[0, 333 ms]**, and §5.5
/// step 2: "The interval is **fixed, not exponential**."
///
/// Both bounds of the band are asserted on every interval, which is also
/// what rules exponential backoff out: an exponential schedule's second
/// interval already exceeds `RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX`.
#[test]
fn the_retransmit_interval_is_five_seconds_plus_bounded_jitter_and_never_grows() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (_conn, d) = a.connect(t, b.addr, &peer);

    let lo = RETRANSMIT_BASE;
    let hi = RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX;

    let mut prev = t;
    let mut due = d.deadline.expect("armed");
    let mut gaps = Vec::new();
    for n in 0..12 {
        let gap = due - prev;
        assert!(
            gap >= lo,
            "interval {n} of {gap:?} is below RETRANSMIT_BASE"
        );
        assert!(
            gap <= hi,
            "interval {n} of {gap:?} exceeds RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX \
             — the schedule is not fixed-interval"
        );
        gaps.push(gap);
        let d = a.timeout(due);
        assert_eq!(d.transmits().len(), 1);
        prev = due;
        due = d.deadline.expect("armed");
    }

    // Not exponential, stated as its own assertion so the failure names
    // the right thing.
    let max = gaps.iter().max().copied().expect("gaps");
    let min = gaps.iter().min().copied().expect("gaps");
    assert!(
        max - min <= RETRANSMIT_JITTER_MAX,
        "the spread between intervals exceeds the jitter band: {min:?}..{max:?}"
    );
}

/// §5.5 step 2 arms the retransmit at "`RETRANSMIT_BASE` + **uniform
/// jitter** ≤ `RETRANSMIT_JITTER_MAX`". The draw is part of the rule, not
/// a decoration on it: fixed retransmit phases across an endpoint's
/// pendings synchronise, and the jitter is what breaks that up.
///
/// [`the_retransmit_interval_is_five_seconds_plus_bounded_jitter_and_never_grows`]
/// bounds the band from **above**, and a core that dropped the jitter
/// entirely — every interval exactly `RETRANSMIT_BASE` — satisfies that
/// bound for free. That is the same shape as comparing whole packets to
/// test the ephemeral: an upper bound a degenerate implementation meets
/// trivially. So presence is asserted here, from below.
///
/// Two mutations, two assertions, in increasing subtlety. A core with
/// **no** jitter fails the first. A core that draws the offset **once**
/// and reuses it for every attempt passes the first and fails the second.
#[test]
fn the_retransmit_jitter_is_drawn_afresh_for_every_attempt() {
    let t = t0();
    let (mut a, b) = pair(t);
    let (_conn, d) = a.connect(t, b.addr, &b.public_static);

    let mut prev = t;
    let mut due = d.deadline.expect("a pending arms a retransmit");
    let mut gaps = Vec::new();
    for _ in 0..12 {
        gaps.push(due - prev);
        let d = a.timeout(due);
        prev = due;
        due = d.deadline.expect("armed");
    }

    assert!(
        gaps.iter().any(|g| *g > RETRANSMIT_BASE),
        "all {} intervals were exactly RETRANSMIT_BASE — §5.5 step 2's jitter \
         is never drawn: {gaps:?}",
        gaps.len()
    );
    assert!(
        gaps.iter().any(|g| *g != gaps[0]),
        "all {} intervals were identical — the jitter is drawn once and reused \
         rather than per attempt: {gaps:?}",
        gaps.len()
    );
}

/// §5.5 step 6 / §5.7: "Give up at `HANDSHAKE_GIVEUP` (**90 s**)", and
/// §16.4: `HandshakeFailed(ConnectionId, ConnectError)` is emitted by the
/// endpoint core. S2's timer half.
///
/// The train is driven by the **announced** deadline at every step, never
/// by a jump, so no deadline is skipped and the final step lands on
/// exactly `t + HANDSHAKE_GIVEUP`. §16.5's "give-up beats a same-instant
/// retransmit" is what makes the last drain carry no `Transmit`; the
/// strictly-equal case is a subset of that and cannot be forced without
/// controlling the jitter draw.
#[test]
fn the_dial_gives_up_at_exactly_handshake_giveup_and_transmits_nothing_there() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (conn, d) = a.connect(t, b.addr, &peer);
    let give_up = t + HANDSHAKE_GIVEUP;

    let mut due = d.deadline.expect("armed");
    let mut attempts = 1usize;
    loop {
        assert!(
            due <= give_up,
            "a deadline was announced past give-up: {due:?}"
        );
        let d = a.timeout(due);
        let failures = d.failures();
        if let [(id, err)] = failures.as_slice() {
            assert_eq!(due, give_up, "give-up fired at the wrong instant");
            assert_eq!(*id, conn);
            assert_eq!(*err, ConnectError::TimedOut);
            assert!(
                d.transmits().is_empty(),
                "a retransmit rode out on the give-up instant (§16.5)"
            );
            assert_eq!(d.deadline, None, "the pending's timers were not disarmed");
            break;
        }
        assert_eq!(d.transmits().len(), 1, "attempt {attempts} did not fire");
        attempts += 1;
        due = d.deadline.expect("armed");
        assert!(attempts < 100, "the train never gave up");
    }

    // 90 s of 5 s-ish intervals: the count is bounded on both sides by the
    // jitter band, so it is asserted as a band rather than a number.
    let lo = 1
        + (HANDSHAKE_GIVEUP.as_millis() / (RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX).as_millis())
            as usize;
    let hi = 1 + (HANDSHAKE_GIVEUP.as_millis() / RETRANSMIT_BASE.as_millis()) as usize;
    assert!(
        (lo..=hi).contains(&attempts),
        "{attempts} attempts is outside the {lo}..={hi} the interval band allows"
    );

    // And nothing happens afterwards, ever.
    let after = a.timeout(give_up + HANDSHAKE_GIVEUP);
    assert!(after.is_silent(), "{:?}", after.outs);
    assert_eq!(after.deadline, None);
}

/// §5.5 step 4: "**The msg2 source address is deliberately ignored** —
/// completion requires an **index** match, not an address match."
///
/// Exactly the rule an implementer "fixes" into a bug.
#[test]
fn msg2_from_a_different_address_still_completes() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);

    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;

    // Delivered from an address the initiator never dialled.
    let d = a.feed(t, v4(200, 200), &msg2);
    assert_eq!(
        d.installs().len(),
        1,
        "completion was refused because the source address differed"
    );
}

/// §16.4 (4429–4431): `Install` targets a `connect()`-created connection
/// "**exactly once**". A second delivery of the same msg2 must not
/// re-install.
#[test]
fn completion_installs_exactly_once() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;

    assert_eq!(a.feed(t, b.addr, &msg2).installs().len(), 1);
    let second = a.feed(t, b.addr, &msg2);
    assert!(
        second.installs().is_empty(),
        "the same msg2 installed a second session"
    );
}

/// §5.5 step 3, the negative half: "a guessed-index or mac1-invalid msg2
/// can **never spend anything**."
///
/// Two oracles, and they answer different questions. **The DH counter
/// proves the rejection was cheap**: a packet that dies at mac1 never
/// reaches the crypto, so the count must not move. **A genuine msg2
/// proves the attempt survived**: if the rejected packet had taken the
/// interval's one attempt, the real one would be dropped and nothing would
/// install.
///
/// The counter cannot serve the second question, and assuming it could is
/// how the first draft of this test was wrong. §5.5 rule 5 spends the
/// attempt *before* the crypto, and a forged body dies at point decoding
/// before any DH — so a taken-and-failed attempt moves the counter by
/// zero, exactly as an untaken one does. See [`genuine_msg2`].
#[test]
fn a_mac1_invalid_msg2_spends_nothing() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let index = init_sender_index(&msg1);
    assert_eq!(a.dhs.get(), 2);

    let mut bad = forged_resp(&a, 0x1234, index, 0x77);
    let last = bad.len() - 1;
    bad[last] ^= 0x01;
    let d = a.feed(t, b.addr, &bad);
    assert_eq!(a.dhs.get(), 2, "a mac1-invalid msg2 reached the crypto");
    assert!(d.installs().is_empty());

    // The attempt is still unspent: the genuine msg2 completes.
    let msg2 = genuine_msg2(&mut b, t, a.addr, &msg1);
    let d = a.feed(t, b.addr, &msg2);
    assert_eq!(
        d.installs().len(),
        1,
        "the mac1-invalid msg2 spent the interval's completion attempt"
    );
    assert_eq!(a.dhs.get(), 4, "completion is ee + se");
}

/// The same, for a **guessed index**. §5.5 step 3 keys the attempt on the
/// index match; §17.3 makes the index unpredictable off-path, which is
/// what this rule rests on.
#[test]
fn a_wrong_index_msg2_spends_nothing() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let index = init_sender_index(&msg1);

    for wrong in [index ^ 0xFFFF_FFFF, index.wrapping_add(1), 0] {
        let bad = forged_resp(&a, 0x1234, wrong, 0x77);
        let d = a.feed(t, b.addr, &bad);
        assert_eq!(
            a.dhs.get(),
            2,
            "a msg2 for index {wrong:#010x} reached the crypto"
        );
        assert!(d.installs().is_empty());
    }

    let msg2 = genuine_msg2(&mut b, t, a.addr, &msg1);
    let d = a.feed(t, b.addr, &msg2);
    assert_eq!(
        d.installs().len(),
        1,
        "a guessed-index msg2 spent the interval's completion attempt"
    );
}

/// §3.1's exact length rule (ruling 65) on the **msg2** path, both sides,
/// and neither may spend the attempt.
#[test]
fn a_wrong_length_msg2_spends_nothing() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let index = init_sender_index(&msg1);
    let exact = forged_resp(&a, 0x1234, index, 0x77);

    let short = &exact[..RESP_PACKET_LEN - 1];
    let _ = a.feed(t, b.addr, short);
    assert_eq!(a.dhs.get(), 2, "RESP_PACKET_LEN - 1 reached the crypto");

    let mut long = exact.clone();
    long.push(0x00);
    let _ = a.feed(t, b.addr, &long);
    assert_eq!(
        a.dhs.get(),
        2,
        "RESP_PACKET_LEN + 1 reached the crypto — an exact check was relaxed to a minimum"
    );

    let msg2 = genuine_msg2(&mut b, t, a.addr, &msg1);
    let d = a.feed(t, b.addr, &msg2);
    assert_eq!(
        d.installs().len(),
        1,
        "a wrong-length msg2 spent the interval's completion attempt"
    );
}

/// §5.5 step 3: "**One completion attempt per retransmit interval.** … A
/// failed completion (bad crypto) **spends** the attempt — the next
/// scheduled retransmit refreshes it."
///
/// Every oracle here is behavioural, because the DH counter cannot see
/// this rule at all: the attempt is spent *before* the crypto (§5.5
/// rule 5) and a forged body dies at point decoding before any DH, so a
/// taken-and-failed attempt and an untaken one leave the same count. What
/// **is** observable is that the genuine msg2 is refused in the interval
/// the forged one spent, and accepted in the next.
///
/// The second responder is not a trick. `b2` holds the **same static** as
/// `b` — same key seed, and [`Ep::new`] derives the identity from it — so
/// its msg2 completes A's handshake, while its endpoint state is fresh.
/// `b` cannot serve twice: it has already spent its one `accept()` for A's
/// static, and §16.1's one-session-per-peer-static invariant makes a
/// second return `AcceptError::Stale` at slice 2a's LIVE boundary.
#[test]
fn a_failed_completion_spends_the_attempt_and_the_next_retransmit_refreshes_it() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let mut b2 = Ep::new(t, 9, 0x33, v4(2, 3), default_config());
    assert_eq!(
        b2.canonical(),
        b.canonical(),
        "the twin responder must share the static, or its msg2 cannot complete"
    );

    let first_msg1 = real_msg1(&mut a, t, &b);
    let first_index = init_sender_index(&first_msg1);
    let due = a.drain().deadline.expect("a pending arms a retransmit");

    // Taken, and failed: length-correct, index-matching, mac1-valid, and
    // a body that cannot open.
    let d = a.feed(t, b.addr, &forged_resp(&a, 1, first_index, 0x31));
    assert!(d.installs().is_empty(), "junk msg2 installed a session");

    // The interval's one attempt is now spent — so even the GENUINE msg2
    // for this very attempt is dropped.
    let genuine_first = genuine_msg2(&mut b, t, a.addr, &first_msg1);
    let d = a.feed(t, b.addr, &genuine_first);
    assert!(
        d.installs().is_empty(),
        "a second msg2 in one interval completed — the failed one did not spend the attempt"
    );

    // The scheduled retransmit refreshes the attempt, and mints a fresh
    // index with it.
    let d = a.timeout(due);
    let second_msg1 = d.one_transmit().1;
    let second_index = init_sender_index(&second_msg1);
    assert_ne!(first_index, second_index);

    // The superseded attempt's genuine msg2 no longer routes — and, not
    // being index-matching, cannot spend the refreshed attempt either.
    let d = a.feed(due, b.addr, &genuine_first);
    assert!(
        d.installs().is_empty(),
        "a msg2 for a superseded attempt completed"
    );

    // … and the refreshed attempt takes the current attempt's msg2.
    let genuine_second = genuine_msg2(&mut b2, due, a.addr, &second_msg1);
    let d = a.feed(due, b.addr, &genuine_second);
    assert_eq!(
        d.installs().len(),
        1,
        "the next interval's attempt was not refreshed"
    );
}

/// §17.3's corollary: "a datagram that routes by index but fails to open
/// **touches nothing**". At this slice the observable half is the
/// `Disposition`: an unknown index is `Done`, and it changes no state.
#[test]
fn data_on_an_unknown_index_touches_nothing() {
    let t = t0();
    let (_a, mut b) = pair(t);
    let parked = b.feed(t, v4(5, 5), &forged_init(&b, 1, 0x11));
    let deadline = parked.deadline;
    let id = parked.one_intro().0;

    for index in [0u32, 1, 0xDEAD_BEEF, u32::MAX] {
        let (disp, d) = b.datagram(t, v4(9, 9), &data_packet(index, 7, 100));
        assert_eq!(
            disp,
            Disposition::Done,
            "index {index:#010x} routed somewhere"
        );
        assert!(d.is_silent(), "{:?}", d.outs);
        assert_eq!(d.deadline, deadline, "an inert datagram moved a deadline");
    }
    assert_eq!(b.dhs.get(), 0);
    assert!(b.present(id), "an inert datagram disturbed the queue");
}

/// §5.6: "The responder anchors an accepted session at the **initiation's
/// msg1 source address**." Not at any address the initiator chose to put
/// inside the packet, and not at a previously seen one — the anchor is the
/// address the datagram arrived from, which is what arms §7.3's
/// amplification budget.
#[test]
fn the_responder_answers_the_msg1_source_address() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let spoofed = v4(123, 45);
    assert_ne!(spoofed, a.addr);

    let id = b.feed(t, spoofed, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");

    let (to, data) = b.drain().one_transmit();
    assert_eq!(
        to, spoofed,
        "msg2 went somewhere other than the msg1 source"
    );
    assert_eq!(data.len(), RESP_PACKET_LEN);
}

/// §5.5 step 5: "our receiver index = our `sender_index`, the peer's = the
/// response's `sender_index`" — and §3.3's header carries **ours, then
/// theirs**, an asymmetry no compile check catches.
#[test]
fn the_response_header_answers_the_initiators_index() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let initiator_index = init_sender_index(&msg1);

    let id = b.feed(t, a.addr, &msg1).one_intro().0;
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    b.ep.authenticate(t, id).expect("authenticates");
    let _ = b.drain();
    b.ep.accept(t, id).expect("accepts");
    let msg2 = b.drain().one_transmit().1;

    let (responder_index, answered) = resp_indices(&msg2);
    assert_eq!(
        answered, initiator_index,
        "receiver_index does not answer the initiation"
    );
    assert_ne!(responder_index, 0, "§17.3: indices are nonzero");
    assert_ne!(
        responder_index, initiator_index,
        "the responder echoed the initiator's index as its own"
    );
}

/// **Not a ratified verb — `PLAN.md` §8.4's F-5 proposal.** §16.4's API
/// list has no cancel, and the plan reads `ToEndpoint::Retired` as the
/// core-side effect S29 needs ("drop the index route", release "the
/// guard-entry pin"), extended here to drop the pending.
///
/// S29's core half: the retransmit train stops, nothing further is
/// transmitted, **no** `HandshakeFailed` is emitted (the `Connecting` is
/// already resolved by its drop, so a second resolution would be wrong),
/// and a `connect()` to the same static on the very next line succeeds.
#[test]
fn retired_cancels_the_pending_and_frees_the_static_for_an_immediate_redial() {
    let t = t0();
    let (mut a, b) = pair(t);
    let peer = b.public_static;
    let (conn, d) = a.connect(t, b.addr, &peer);
    let index = init_sender_index(&d.one_transmit().1);
    let due = d.deadline.expect("armed");

    a.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: index });
    let d = a.drain();
    assert!(
        d.failures().is_empty(),
        "cancellation emitted a HandshakeFailed — a second resolution"
    );
    assert!(d.transmits().is_empty());

    // The train is stopped: neither the next retransmit nor the give-up
    // produces anything.
    let d = a.timeout(due);
    assert!(
        d.is_silent(),
        "the retransmit train survived cancellation: {:?}",
        d.outs
    );
    let d = a.timeout(t + HANDSHAKE_GIVEUP);
    assert!(
        d.is_silent(),
        "give-up fired for a cancelled pending: {:?}",
        d.outs
    );

    // And the static is free.
    assert!(
        a.ep.mint_pending(t, b.addr, peer).is_ok(),
        "a redial after cancellation returned AlreadyConnected (S29)"
    );
}

/// §17.2: `last_init_timestamp` "survives across connection generations to
/// the same peer — **close-and-reconnect still emits strictly greater**",
/// which is why its scope is the endpoint rather than the connection.
///
/// Under a frozen wall clock the redial's timestamp has exactly one
/// possible source of increase.
#[test]
fn a_redial_after_cancellation_still_emits_a_strictly_greater_timestamp() {
    let t = t0();
    let mut a = Ep::new(
        t,
        7,
        0x11,
        v4(1, 1),
        frozen_clock_config(T_BASE_SECS, 12_345),
    );
    let mut b = Ep::new(t, 9, 0x22, v4(2, 2), default_config());

    let peer = b.public_static;
    let (conn, d) = a.connect(t, b.addr, &peer);
    let first = d.one_transmit().1;
    let index = init_sender_index(&first);

    a.ep.handle_connection_event(t, conn, ToEndpoint::Retired { our_index: index });
    let _ = a.drain();

    let (_conn2, d) = a.connect(t, b.addr, &peer);
    let second = d.one_transmit().1;

    let (_id, ts1) = ladder_to_proven(&mut b, t, v4(30, 1), &first);
    let ts1 = ts1.expect("first admission");
    let (_id, ts2) = ladder_to_proven(&mut b, t, v4(30, 2), &second);
    let ts2 = ts2.expect("the redial must be strictly greater to be admissible");
    assert!(
        ts2 > ts1,
        "a new connection generation reused or regressed the timestamp: {ts1:?} then {ts2:?}"
    );
}

/// S8's durability half: a parked chain survives arbitrarily many
/// unrelated `handle_timeout` / `handle_datagram` calls, and dies at
/// `INTRO_TTL` and not before. "Across event-loop turns" needs a loop
/// (slice 3); this is the part the core owns.
#[test]
fn a_parked_decision_survives_unrelated_activity_until_its_ttl() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = real_msg1(&mut a, t, &b);
    let id = b.feed(t, a.addr, &msg1).one_intro().0;

    // 140 unrelated events at 100 ms apart — 14 s of churn.
    for n in 1..=140u64 {
        let now = t + Duration::from_millis(n * 100);
        let _ = b.timeout(now);
        let _ = b.feed(now, v4(60, n as u16), &forged_init(&b, n as u32, 0x66));
        let _ = b.datagram(now, v4(61, 1), &data_packet(0xABCD, n, 32));
    }
    assert!(b.present(id), "the parked chain did not survive the churn");
    // Ruling 92's `now`: the churn loop's last instant, named because the
    // loop binding has gone out of scope.
    let after_churn = t + Duration::from_millis(140 * 100);
    assert!(
        b.ep.read_identity(after_churn, id).is_ok(),
        "the decision could not still be taken"
    );
    let _ = b.drain();

    let _ = b.timeout(t + INTRO_TTL);
    assert!(!b.present(id), "the chain outlived INTRO_TTL");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. §17.4 — the replacement basis and the hint set
//
// Both are **written by this slice and read by nothing in it**: §6.4 is
// the basis's only reader and §6.5 the hint set's, and both are slice 7.
// That is precisely why they need tests of their own — a wrong value here
// is undetectable until slice 7 and then presents as a slice-7 bug.
// `replacement_basis` and `hints` exist to close that gap and are the one
// place this file asserts on state the protocol does not yet consult.
// ═══════════════════════════════════════════════════════════════════════

/// §17.4: the basis is "`Some(t)` where `t` is the timestamp of the msg1
/// that **established** the connection while we were the **responder** —
/// a staged `accept()` …", and it is "written **once at install**".
///
/// The `t` compared against is the one `authenticate()` handed back, so
/// this measures the record against **the timestamp the peer actually
/// sent** rather than against a second copy of the implementation's own
/// arithmetic.
#[test]
fn accept_records_a_some_basis_equal_to_the_msg1_timestamp() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let train = msg1_train(&mut a, t, &b, 1);

    assert_eq!(
        b.ep.replacement_basis(a.canonical()),
        None,
        "a static with no connection has no static-map entry at all"
    );

    let (id, admitted) = ladder_to_proven(&mut b, t, v4(28, 1), &train[0]);
    let ts = admitted.expect("admission");
    assert_eq!(
        b.ep.replacement_basis(a.canonical()),
        None,
        "§17.4: the basis is written at install — reaching Proven installs nothing"
    );

    b.ep.accept(t, id).expect("the NONE row accepts");
    let _ = b.drain();

    assert_eq!(
        b.ep.replacement_basis(a.canonical()),
        Some(Some(ts)),
        "the responder's basis is not the timestamp of the msg1 that established it"
    );
}

/// §17.4: the basis is "`None` when we **dialled** (a `connect()`
/// completed by msg2 …: neither teaches us any timestamp of the peer's,
/// because msg2 carries no payload, §5.2)".
///
/// `Some(None)` and `None` are different answers and telling them apart is
/// the whole content of this test. §6.4 reads an entry whose basis is
/// `None` as *refuse every replacement*, while no entry at all means no
/// connection — and §17.4's "What a `None` basis costs" paragraph hangs
/// the contested-connection probe (§7.5, ruling 36) on exactly that
/// distinction.
#[test]
fn connect_records_a_none_basis() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let stranger = Ep::new(t, 11, 0x44, v4(3, 3), default_config());

    let msg1 = real_msg1(&mut a, t, &b);
    let msg2 = genuine_msg2(&mut b, t, a.addr, &msg1);
    assert_eq!(a.feed(t, b.addr, &msg2).installs().len(), 1);

    assert_eq!(
        a.ep.replacement_basis(b.canonical()),
        Some(None),
        "a connection we dialled recorded a timestamp — msg2 carries none to record"
    );
    assert_eq!(
        a.ep.replacement_basis(stranger.canonical()),
        None,
        "a static we never connected to has no entry at all"
    );
}

/// §17.4: the hint set **is** "the pending tables' dialled addresses …
/// Established connections contribute **no** hints: their initiations take
/// the ordinary staged path (§5.4), so the endpoint tracks no
/// per-connection address."
///
/// Both sides of that sentence, because only one of them is intuitive: a
/// dial in flight contributes its address, and the *same* connection
/// contributes nothing once established. The second half is what a
/// keep-the-address-it-might-be-useful design gets wrong, and §6.5 would
/// then probe an address no tie-break can act on.
#[test]
fn an_established_connection_contributes_no_hint() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let c = Ep::new(t, 11, 0x44, v4(3, 3), default_config());

    assert!(a.ep.hints().is_empty(), "an idle endpoint probes nothing");

    let msg1 = real_msg1(&mut a, t, &b);
    assert_eq!(
        a.ep.hints(),
        vec![b.addr],
        "an in-flight dial's remote is the hint set"
    );

    // A second pending contributes a second hint. `hints()` is sorted, and
    // 10.0.0.2:2 precedes 10.0.0.3:3.
    let peer_c = c.public_static;
    let _ = a.connect(t, c.addr, &peer_c);
    assert_eq!(a.ep.hints(), vec![b.addr, c.addr]);

    // Completing the first removes its hint — and only its own.
    let msg2 = genuine_msg2(&mut b, t, a.addr, &msg1);
    assert_eq!(a.feed(t, b.addr, &msg2).installs().len(), 1);
    assert_eq!(
        a.ep.hints(),
        vec![c.addr],
        "an established connection still contributes a hint"
    );
}

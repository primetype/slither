//! Independent acceptance tests for **§6.5's routing rule** and **§6.6's
//! internal tie-break completion** (slice 4, ruling 91).
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written blind, in a separate worktree, from `SPEC.md` §6.4:1310–1457,
//! §6.5:1458–1514, §6.6:1515–1563, §6.7:1564–1662, §17.1, §17.4 and rulings
//! 90/91 alone. The author has not read the slice-4 implementation. Where a
//! name below is a **proposal** rather than something the spec or the frozen
//! code pins, it is flagged `NAME PROPOSAL` in a comment: the integrator
//! renames *calls*, **never** assertions.
//!
//! `src/core/tests.rs` (slice 2a) and `src/core/connection/tests.rs` (slice
//! 3a) belong to other authors. Their fixture *idiom* is reused here — plain
//! `#[test]`, `now` as arithmetic on a base `Instant`, no tokio, no paused
//! clock — but no code is shared, because their fixtures are private to
//! their own modules.
//!
//! # What the broken build does
//!
//! Ruling 91 measured it: `read_identity()` → `connect()` → `accept()` on one
//! peer ends with **both sides `TimedOut` at 90 s**, because §6.5's routing
//! and §6.6's internal completion do not exist, so neither side's kept
//! pending can finish. [`the_ordinary_api_ordering_completes_both_sides_when_we_win`]
//! and its mirror are that measurement turned into an assertion.
//!
//! # Working rule 9, applied
//!
//! Every test below names, in its doc comment, the **mutation it catches** —
//! the concretely-broken build that passes everything else and fails here. A
//! test whose degenerate build passes for free has been deleted or paired
//! with the mirror that separates them; where the force of an assertion comes
//! from its mirror, the doc comment says which test that is.
//!
//! **Both key orders are exercised throughout.** §6.6 step 3 invokes §6.7's
//! comparison, so every outcome has a mirror, and a suite that only ever puts
//! the local endpoint on one side of the comparison passes a build that
//! ignores the comparison entirely and always takes one branch — which is
//! precisely what the pre-slice-4 core does. [`seeds_by_key_order`] derives
//! the order from the keys at run time rather than assuming it, so changing
//! the seeds cannot silently collapse the coverage.
//!
//! # Spec gaps found while writing
//!
//! Collected at the bottom of this file under `SPEC GAPS`, and every test
//! here has been checked against that list (slice 3a's author found a gap,
//! routed one test around it, then wrote another that depended on it).

#![allow(clippy::items_after_statements)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;

use crate::config::{Config, WallClock};
use crate::constants::{
    HANDSHAKE_GIVEUP, INIT_PACKET_LEN, MAC1_LEN, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP,
    RESP_PACKET_LEN, RETRANSMIT_BASE, RETRANSMIT_JITTER_MAX, TS_GUARD_ORPHAN_TTL, VERSION,
};
use crate::core::{ConnectionId, Disposition, EndpointOutput, Role, Timestamp};
use crate::error::{AcceptError, AuthError, ConnectError, IntroError};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::mac::Mac1Key;
use crate::testutil::{CountingIdentity, DhCounter};

type Suite = crate::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;

// ═══════════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════════

/// Everything one `poll_output()` loop produced, plus the deadline it ended
/// on. §16.4 makes generation order normative, so the outputs are collected
/// in order rather than matched one at a time.
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

    /// **[ruling 106]** The role each `Install` carries. Added at slice-4
    /// integration, not by this file's author.
    fn install_roles(&self) -> Vec<Role> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::ToConnection(_, ev) => Some(ev.role),
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

    fn one_transmit(&self) -> (SocketAddr, Vec<u8>) {
        let v = self.transmits();
        assert_eq!(v.len(), 1, "expected exactly one Transmit, got {}", v.len());
        v[0].clone()
    }

    fn one_intro(&self) -> (IntroId, SocketAddr) {
        let v = self.intros();
        assert_eq!(v.len(), 1, "expected exactly one IntroReady, got {v:?}");
        v[0]
    }

    /// Nothing but the terminal `Timeout` — the shape §6.6's "silent drop"
    /// has to have, and the shape §6.5 step 3 demands of the internal path
    /// ("the application never sees it").
    fn is_silent(&self) -> bool {
        self.outs.is_empty()
    }

    /// Whether any `HandshakeFailed` in this drain reports a give-up.
    /// Ruling 91's measured failure, phrased as a predicate.
    fn timed_out(&self) -> bool {
        self.failures()
            .iter()
            .any(|(_, e)| *e == ConnectError::TimedOut)
    }
}

/// A wall clock frozen at one reading, so every initiation timestamp this
/// file asserts on is a value the test computed rather than observed.
struct FrozenClock(Timestamp);

impl WallClock for FrozenClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// One endpoint core plus what a test needs to know about it.
struct Ep {
    ep: Endpoint<Id>,
    /// Endpoint-wide and cumulative — exactly what §6.1's ladder prices.
    dhs: DhCounter,
    public_static: Pk,
    addr: SocketAddr,
    /// This endpoint's frozen wall-clock reading, so
    /// [`nth_timestamp`](Ep::nth_timestamp) can name the timestamp of the
    /// n-th initiation it emits without observing it.
    wall: Timestamp,
}

impl Ep {
    fn new(now: Instant, key_seed: u8, rng_seed: u8, addr: SocketAddr, wall: Timestamp) -> Self {
        let identity: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = identity.counter();
        let public_static = *identity.public_static();
        let config = Config::default().with_clock(Rc::new(FrozenClock(wall)));
        let ep = Endpoint::new(now, config, identity, [rng_seed; 32]);
        Ep {
            ep,
            dhs,
            public_static,
            addr,
            wall,
        }
    }

    /// §2.4's canonical static encoding — the octets §6.7 compares.
    fn canonical(&self) -> &[u8] {
        self.public_static.as_ref()
    }

    fn mac1_key(&self) -> Mac1Key {
        Mac1Key::derive(self.canonical())
    }

    /// The timestamp of the `n`-th initiation (1-based) this endpoint emits.
    ///
    /// The clock is frozen, so §5.3's forcing does all the work: the first
    /// initiation reads the clock and every later one is `succ()` of the
    /// last. §17.2's monotone forcing is endpoint-global, so `n` counts
    /// **every** msg1 this endpoint has built, across all peers.
    fn nth_timestamp(&self, n: usize) -> Timestamp {
        assert!(n >= 1, "initiations are 1-based");
        let mut t = self.wall;
        for _ in 1..n {
            t = next_timestamp(t);
        }
        t
    }

    /// §16.4's drain contract, mechanised.
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

    fn feed(&mut self, now: Instant, src: SocketAddr, dgram: &[u8]) -> Drained {
        self.datagram(now, src, dgram).1
    }

    fn timeout(&mut self, now: Instant) -> Drained {
        self.ep.handle_timeout(now);
        self.drain()
    }

    /// A full dial: ruling 90's `mint_pending` (0 DH) then `start_attempt`
    /// (2 DH). The `Connection` handle is dropped — the core observes
    /// nothing about it; §16.4's cancellation rides `handle_connection_event`.
    fn dial(&mut self, now: Instant, remote: SocketAddr, peer: &Pk) -> (ConnectionId, Drained) {
        let (id, _conn) = self
            .ep
            .mint_pending(now, remote, *peer)
            .expect("mint_pending should succeed for a static with no connection");
        self.ep.start_attempt(now, id);
        (id, self.drain())
    }

    /// `mint_pending` alone — the pending exists and is in the hint set, but
    /// no msg1 has been built. Ruling 90 makes this a reachable state.
    fn mint_only(&mut self, now: Instant, remote: SocketAddr, peer: &Pk) -> ConnectionId {
        let (id, _conn) = self
            .ep
            .mint_pending(now, remote, *peer)
            .expect("mint_pending should succeed for a static with no connection");
        id
    }

    /// DH spent since the last [`reset_dh`](Ep::reset_dh).
    fn dh(&self) -> u32 {
        self.dhs.get()
    }

    fn reset_dh(&self) {
        self.dhs.reset();
    }

    fn present(&self, id: IntroId) -> bool {
        self.ep.intro_source(id).is_some()
    }

    /// §17.4's hint set, as §6.5 step 2 consults it.
    fn hints(&self) -> Vec<SocketAddr> {
        self.ep.hints()
    }

    /// §17.4's basis: `None` = no connection, `Some(None)` = we dialled,
    /// `Some(Some(t))` = we responded at initiation timestamp `t`.
    fn basis(&self, peer: &[u8]) -> Option<Option<Timestamp>> {
        self.ep.replacement_basis(peer)
    }

    fn greatest(&self, peer: &[u8]) -> Option<Timestamp> {
        self.ep.greatest(peer)
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

/// §5.3's smallest strictly-greater timestamp: +1 ns, carrying into seconds.
/// Mirrors `Timestamp::succ`, which is `pub(crate)` to the core and not part
/// of what this file is entitled to assume; recomputing it here means the
/// expected values are the test's, not the implementation's.
fn next_timestamp(t: Timestamp) -> Timestamp {
    if t.nanos() >= 999_999_999 {
        Timestamp::new(t.secs() + 1, 0)
    } else {
        Timestamp::new(t.secs(), t.nanos() + 1)
    }
}

/// The `sender_index` a HandshakeInit carries — §3.2, little-endian at
/// offset 2.
fn init_sender_index(dgram: &[u8]) -> u32 {
    u32::from_le_bytes(dgram[2..6].try_into().expect("init header"))
}

/// §3.3's `(sender_index, receiver_index)` of a HandshakeResp — **theirs,
/// then ours** in field order: offset 2 is the writer's index, offset 6 the
/// index the writer is answering.
fn resp_indices(dgram: &[u8]) -> (u32, u32) {
    (
        u32::from_le_bytes(dgram[2..6].try_into().expect("resp header")),
        u32::from_le_bytes(dgram[6..10].try_into().expect("resp header")),
    )
}

/// A syntactically valid, **mac1-valid**, cryptographically meaningless
/// HandshakeInit addressed to `recipient`.
///
/// Reaches §6.5 step 1 intact — the length gate and mac1 both pass — and
/// fails at the eager split intro read of step 3, which is the only way this
/// file can reach that failure.
fn forged_init(recipient: &Ep, sender_index: u32, filler: u8) -> Vec<u8> {
    let mut d = vec![filler; INIT_PACKET_LEN];
    d[0] = PKT_HANDSHAKE_INIT;
    d[1] = VERSION;
    d[2..6].copy_from_slice(&sender_index.to_le_bytes());
    let (preimage, tag) = d.split_at_mut(INIT_PACKET_LEN - MAC1_LEN);
    let t = recipient.mac1_key().tag(preimage);
    tag.copy_from_slice(&t);
    d
}

/// A **real** msg1 whose payload tail has been corrupted and whose mac1 has
/// been recomputed: §6.6 step 1's "a forged claim of a pending static".
///
/// Noise IK msg1 is `e ‖ ENCRYPTED(s) ‖ ENCRYPTED(payload)`, so flipping a
/// bit in the final octet before mac1 lands inside the **payload** AEAD and
/// leaves `e ‖ ENCRYPTED(s)` untouched. The eager `es` therefore still
/// succeeds and still reveals the genuine claimed static — which is exactly
/// the shape step 1 exists to kill: an attacker who can name a pending
/// static but cannot produce its key.
fn forge_tail(genuine_msg1: &[u8], recipient: &Ep) -> Vec<u8> {
    let mut d = genuine_msg1.to_vec();
    let last_msg1_byte = INIT_PACKET_LEN - MAC1_LEN - 1;
    d[last_msg1_byte] ^= 0xFF;
    let (preimage, tag) = d.split_at_mut(INIT_PACKET_LEN - MAC1_LEN);
    let t = recipient.mac1_key().tag(preimage);
    tag.copy_from_slice(&t);
    d
}

/// A real msg1 from `initiator` to `responder`, captured off the wire. The
/// initiator keeps its pending, which is what makes it a *crossing*
/// initiation rather than a replayed one.
fn real_msg1(initiator: &mut Ep, now: Instant, responder: &Ep) -> Vec<u8> {
    let peer = responder.public_static;
    let (_id, drained) = initiator.dial(now, responder.addr, &peer);
    let (to, data) = drained.one_transmit();
    assert_eq!(
        to, responder.addr,
        "§5.5 step 1 sends to the dialled address"
    );
    assert_eq!(data.len(), INIT_PACKET_LEN, "§3.2, exact");
    data
}

/// The seeds of this file's two identities, **ordered by §6.7's comparison**
/// — smaller first.
///
/// §6.7 compares the canonical static encoding (§2.4) "as unsigned octet
/// strings", which is exactly `Ord` on `[u8]`. Which of the two seeds wins
/// is a property of P-256 key generation, not of this file, so it is
/// **determined here rather than assumed**: if the seeds ever change, this
/// helper keeps returning the right pair and no assertion below moves.
/// `the_two_seeded_statics_differ_and_the_fixture_orders_them` is the guard
/// that stops this going degenerate.
const SEED_X: u8 = 7;
const SEED_Y: u8 = 9;

fn static_bytes(seed: u8) -> Vec<u8> {
    let id: Id = CountingIdentity::seeded([seed; 32]);
    id.public_static().as_ref().to_vec()
}

fn seeds_by_key_order() -> (u8, u8) {
    let x = static_bytes(SEED_X);
    let y = static_bytes(SEED_Y);
    assert_ne!(x, y, "two distinct seeds must give two distinct statics");
    if x < y {
        (SEED_X, SEED_Y)
    } else {
        (SEED_Y, SEED_X)
    }
}

/// `(winner, loser)` for §6.7's comparison: the endpoint whose canonical
/// static is lexicographically **smaller** is the winning initiator.
///
/// The two frozen clocks differ, so every assertion about "whose timestamp
/// was recorded" can only pass for the right one.
const WALL_WINNER: Timestamp = Timestamp::new(T_BASE_SECS, 0);
const WALL_LOSER: Timestamp = Timestamp::new(T_BASE_SECS, 500);

fn tie_pair(now: Instant) -> (Ep, Ep) {
    let (small, large) = seeds_by_key_order();
    let winner = Ep::new(now, small, 0x11, v4(1, 1), WALL_WINNER);
    let loser = Ep::new(now, large, 0x22, v4(2, 2), WALL_LOSER);
    assert!(
        winner.canonical() < loser.canonical(),
        "the fixture must hand back the smaller static first (§6.7)"
    );
    (winner, loser)
}

/// A third identity, for the §6.5 step 3 case where the eager read's claim
/// is **not** a pending outbound remote.
fn third_party(now: Instant) -> Ep {
    Ep::new(
        now,
        0x5B,
        0x33,
        v4(3, 3),
        Timestamp::new(T_BASE_SECS, 900_000),
    )
}

/// Long enough that a retransmit is certainly due (§5.5: base + U[0, jitter]).
fn past_retransmit() -> Duration {
    RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX + Duration::from_millis(1)
}

/// Long enough that a give-up is certainly due (§5.5).
fn past_giveup() -> Duration {
    HANDSHAKE_GIVEUP + Duration::from_secs(1)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Fixture sanity — so nothing below can go quietly degenerate
// ═══════════════════════════════════════════════════════════════════════

/// The two seeded statics are distinct and the fixture hands them back in
/// §6.7's order.
///
/// **Mutation caught:** two seeds that collide, or a `tie_pair` that returns
/// them unordered. Either would make every "winner side" test below actually
/// exercise the loser side (or the same side twice), and the whole
/// both-orders claim of this file would be false while every test stayed
/// green. A name is not a pin; this is the pin.
#[test]
fn the_two_seeded_statics_differ_and_the_fixture_orders_them() {
    let (winner, loser) = tie_pair(t0());
    assert_ne!(
        winner.canonical(),
        loser.canonical(),
        "§6.7: equal statics cannot occur"
    );
    assert!(
        winner.canonical() < loser.canonical(),
        "§6.7: the lexicographically smaller static is the winning initiator, \
         and tie_pair must return it first"
    );
    // And the two frozen clocks differ, so "whose timestamp was recorded"
    // is a question with a wrong answer available.
    assert_ne!(winner.nth_timestamp(1), loser.nth_timestamp(1));

    // The third party is a third static, not an alias of either — otherwise
    // every §6.5 step 3 demotion test would silently be a tie-break test.
    let other = third_party(t0());
    assert_ne!(other.canonical(), winner.canonical());
    assert_ne!(other.canonical(), loser.canonical());
}

/// §5.3's forcing, as this file models it: the n-th initiation from a
/// frozen-clock endpoint is `succ()^(n-1)` of the clock reading.
///
/// **Mutation caught:** a fixture whose `nth_timestamp` drifts from the
/// core's `draw_timestamp`, which would make every exact-timestamp assertion
/// below either vacuous or wrong for a reason that has nothing to do with
/// §6.5 or §6.6.
#[test]
fn the_fixture_predicts_the_initiation_timestamps_it_asserts_on() {
    let now = t0();
    let (mut a, b) = tie_pair(now);
    let first = real_msg1(&mut a, now, &b);
    assert_eq!(first.len(), INIT_PACKET_LEN);

    // Force a retransmit: §5.5 rule 2 makes it a completely fresh
    // initiation with a strictly greater timestamp.
    let d = a.timeout(now + past_retransmit());
    let (_, second) = d.one_transmit();
    assert_ne!(first, second, "§5.5 rule 2: every retransmit is fresh");

    assert_eq!(a.nth_timestamp(1), a.wall);
    assert_eq!(a.nth_timestamp(2), next_timestamp(a.wall));
    assert!(
        a.nth_timestamp(2) > a.nth_timestamp(1),
        "§17.2's monotone forcing"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. §6.5 — the routing rule
// ═══════════════════════════════════════════════════════════════════════

/// §6.5 step 2: `src` ∉ hint set ⇒ park at stage 0 and surface an `Intro`,
/// at **0 DH**.
///
/// The control for
/// [`an_init_from_a_hint_source_spends_one_dh_on_the_eager_read`]: the two
/// differ *only* in whether the source is in the hint set, and they must
/// disagree on the DH count.
///
/// **Mutation caught:** a core that runs the eager split intro read
/// unconditionally — the "hint check" collapsed to nothing. It would spend 1
/// DH here.
#[test]
fn an_init_from_a_non_hint_source_parks_at_zero_dh() {
    let now = t0();
    let (mut local, mut peer) = tie_pair(now);
    let msg1 = real_msg1(&mut peer, now, &local);

    assert!(
        local.hints().is_empty(),
        "the local endpoint has dialled nobody, so §6.5's hint set is empty"
    );

    local.reset_dh();
    let d = local.feed(now, peer.addr, &msg1);
    assert_eq!(local.dh(), 0, "§6.1: parking is 0 DH, §6.5 step 2");
    let (_id, src) = d.one_intro();
    assert_eq!(src, peer.addr);
    assert!(d.transmits().is_empty(), "parking writes nothing");
}

/// §6.5 step 3: `src` ∈ hint set ⇒ the endpoint immediately runs the split
/// intro read, **1 DH (`es`)** — even when the claim turns out not to be a
/// pending outbound remote and the packet is demoted.
///
/// The claim here is a *third party*'s: §6.5's own example, "a peer sharing
/// a source with a dialled address". It must still surface as an `Intro`.
///
/// **Mutation caught:** the hint check omitted (0 DH, no eager read), which
/// is the pre-slice-4 core exactly. Paired with
/// [`an_init_from_a_non_hint_source_parks_at_zero_dh`], the two counts
/// separate a core that consults the hint set from one that does not — an
/// assertion neither could carry alone, since "0 DH" and "1 DH" are each
/// individually satisfiable by a core with no hint set at all.
#[test]
fn an_init_from_a_hint_source_spends_one_dh_on_the_eager_read() {
    let now = t0();
    let (mut local, peer) = tie_pair(now);
    let mut other = third_party(now);

    // The third party's msg1 is addressed to `local` — mac1 keys on the
    // recipient — but will be delivered from the address `local` dialled.
    let msg1 = real_msg1(&mut other, now, &local);
    local.dial(now, peer.addr, &peer.public_static);
    assert!(
        local.hints().contains(&peer.addr),
        "§17.4: an in-flight outbound pending's dialled address is a hint"
    );

    local.reset_dh();
    let d = local.feed(now, peer.addr, &msg1);
    assert_eq!(
        local.dh(),
        1,
        "§6.5 step 3: the eager split intro read is 1 DH (`es`)"
    );
    let (_id, src) = d.one_intro();
    assert_eq!(
        src, peer.addr,
        "§6.5 step 3: a peer sharing a source with a dialled address still \
         surfaces as an Intro"
    );
    assert!(
        d.transmits().is_empty(),
        "a demotion writes nothing on the wire"
    );
}

/// §6.5 step 3: the demoted packet carries its **paid mid-state**, tagged
/// identity-already-read, so `read_identity()` on it returns the cached claim
/// at **0 incremental DH**.
///
/// **Mutation caught:** a demotion that drops the mid-state and re-parks the
/// raw bytes. The chain would still work and still return the right static —
/// at 1 more DH, which is the whole cost §6.5 step 3 promises to have already
/// paid. Nothing but a DH count can see this.
#[test]
fn a_demoted_intro_returns_its_cached_claim_at_zero_incremental_dh() {
    let now = t0();
    let (mut local, peer) = tie_pair(now);
    let mut other = third_party(now);
    let msg1 = real_msg1(&mut other, now, &local);
    local.dial(now, peer.addr, &peer.public_static);

    let d = local.feed(now, peer.addr, &msg1);
    let (id, _) = d.one_intro();

    local.reset_dh();
    let claimed = local
        .ep
        .read_identity(now, id)
        .expect("the demoted chain's claim is already read");
    assert_eq!(
        local.dh(),
        0,
        "§6.5 step 3: read_identity() on a demoted intro is 0 incremental DH"
    );
    assert_eq!(
        claimed.as_ref(),
        other.canonical(),
        "the cached claim is the third party's static, not the dialled peer's"
    );
}

/// §6.1's ladder is **cumulative**, and a demotion does not perturb it:
/// 1 DH once the identity is known, 2 after `authenticate()`, 4 after
/// `accept()` — counted from the packet's arrival, eager read included.
///
/// **Mutation caught:** a demotion that pays the eager `es` and then lets the
/// staged path pay it again — cumulative 2 / 3 / 5. §6.1's table is the
/// protocol's price list; overcharging by one DH per demoted packet is a
/// denial-of-service amplifier that no functional test would notice.
#[test]
fn a_demoted_intro_keeps_section_6_1s_cumulative_ladder() {
    let now = t0();
    let (mut local, peer) = tie_pair(now);
    let mut other = third_party(now);
    let msg1 = real_msg1(&mut other, now, &local);
    local.dial(now, peer.addr, &peer.public_static);

    local.reset_dh();
    let d = local.feed(now, peer.addr, &msg1);
    let (id, _) = d.one_intro();
    assert_eq!(local.dh(), 1, "arrival + eager read: 1 DH cumulative");

    local.ep.read_identity(now, id).expect("claim already read");
    assert_eq!(local.dh(), 1, "§6.1: read_identity is 1 DH cumulative");

    local
        .ep
        .authenticate(now, id)
        .expect("a genuine msg1 authenticates");
    assert_eq!(local.dh(), 2, "§6.1: authenticate is 2 DH cumulative");

    let (_conn, _c) = local.ep.accept(now, id).expect("NONE static, fresh accept");
    let _ = local.drain();
    assert_eq!(local.dh(), 4, "§6.1: accept is 4 DH cumulative");
}

/// §6.5 step 2 and §17.4: the hint set is the **pending outbound remotes**
/// alone. An established connection's address is not a hint, so an initiation
/// from it parks at 0 DH like any stranger's.
///
/// **Mutation caught:** a hint set derived from the whole static map rather
/// than from the pending tables' `dialled` addresses. It would spend the
/// eager 1 DH here — and, worse, §6.5's honesty clause about the probed set
/// ("no established connection is probed") would be false, turning a
/// bounded in-flight-dial oracle into a configuration oracle.
#[test]
fn an_established_connections_address_is_not_a_hint() {
    let now = t0();
    let (mut local, mut peer) = tie_pair(now);

    // Establish, the ordinary staged way: peer dials, local accepts.
    let msg1 = real_msg1(&mut peer, now, &local);
    let d = local.feed(now, peer.addr, &msg1);
    let (id, _) = d.one_intro();
    local.ep.authenticate(now, id).expect("genuine msg1");
    local.ep.accept(now, id).expect("NONE static, fresh accept");
    let _ = local.drain();
    assert!(
        local.basis(peer.canonical()).is_some(),
        "the accept installed a connection for that static"
    );
    assert!(
        !local.hints().contains(&peer.addr),
        "§17.4: established connections contribute no hints"
    );

    // A second initiation from the very same address.
    let msg1b = next_retransmit_init(&mut peer, now + past_retransmit());
    local.reset_dh();
    let d = local.feed(now + past_retransmit(), peer.addr, &msg1b);
    assert_eq!(
        local.dh(),
        0,
        "§6.5 step 2: not a hint, so no eager read — 0 DH"
    );
    assert_eq!(
        d.intros().len(),
        1,
        "it parks and surfaces as an ordinary Intro"
    );
}

/// The peer's next initiation, off its retransmit train.
fn next_retransmit_init(peer: &mut Ep, now: Instant) -> Vec<u8> {
    let d = peer.timeout(now);
    let (_, data) = d.one_transmit();
    assert_eq!(data.len(), INIT_PACKET_LEN);
    data
}

/// §6.5's stated false negative: a peer dialling out from an address other
/// than the one we dialled misses the hint set, and its crossing msg1 parks
/// as an ordinary `Intro`. The hint is matched on the **full address** — the
/// spec's example is "a peer dialling from a rewritten source port".
///
/// **Mutation caught:** hint matching on the IP alone (or on the §6.3
/// `SourceKey`, which is IP-scoped). That build would take the eager path
/// here, run the tie-break, and — on the loser side — cancel a pending and
/// write msg2 in a case §6.5 says must park. It would also make §6.5's
/// entire false-negative discussion, and the `read_identity()` backstop it
/// justifies, unreachable and therefore untested for ever after.
#[test]
fn a_source_port_rewrite_misses_the_hint_set_and_parks() {
    let now = t0();
    let (mut local, mut peer) = tie_pair(now);
    let msg1 = real_msg1(&mut peer, now, &local);

    local.dial(now, peer.addr, &peer.public_static);
    let rewritten = SocketAddr::new(peer.addr.ip(), peer.addr.port() ^ 0x0F00);
    assert_ne!(rewritten, peer.addr);
    assert!(!local.hints().contains(&rewritten));

    local.reset_dh();
    let d = local.feed(now, rewritten, &msg1);

    assert_eq!(local.dh(), 0, "§6.5 step 2: not a hint, so no eager read");
    let (_id, src) = d.one_intro();
    assert_eq!(src, rewritten, "it parks as an ordinary Intro");
    assert!(
        d.transmits().is_empty(),
        "no tie-break ran, so no msg2 was written"
    );
    assert_eq!(
        local.greatest(peer.canonical()),
        None,
        "§6.6 never ran, so nothing was recorded"
    );
    assert_eq!(
        local.basis(peer.canonical()),
        Some(None),
        "our own pending is untouched: still ours, still dialled"
    );
}

/// §6.5 step 4, the backstop: a **parked** `Intro` whose claimed static turns
/// out to be a pending outbound remote is intercepted at `read_identity()`,
/// which returns `Err(IntroError::Internal)` — "the application learns no
/// identity and makes no decision".
///
/// Run in **both key orders**: the interception itself is
/// order-independent, and a build that only intercepts on one side of §6.7's
/// comparison fails one half of this.
///
/// **Mutation caught:** no interception at all — `read_identity()` returns
/// `Ok(peer_static)` and the application proceeds to `accept()`, which is
/// precisely the state ruling 91 measured as both sides `TimedOut` at 90 s.
#[test]
fn read_identity_intercepts_a_parked_intro_whose_claim_is_a_pending_remote() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (winner, loser) = tie_pair(now);
        let (mut local, mut peer) = if local_is_winner {
            (winner, loser)
        } else {
            (loser, winner)
        };

        // The msg1 arrives BEFORE the dial, so it parks (§6.5 step 2) —
        // the state step 4 exists for.
        let msg1 = real_msg1(&mut peer, now, &local);
        let d = local.feed(now, peer.addr, &msg1);
        let (id, _) = d.one_intro();

        // Now the claim becomes a pending outbound remote.
        local.dial(now, peer.addr, &peer.public_static);

        let err = local
            .ep
            .read_identity(now, id)
            .expect_err("§6.5 step 4: the interception denies the application an identity");
        let d = local.drain();

        assert_eq!(
            err,
            IntroError::Internal,
            "§6.5 step 4 names the variant, and §18.1 defines it as \
             'the initiation belonged to a pending outbound dial and was consumed' \
             (local_is_winner = {local_is_winner})"
        );
        // The interception is not merely an error code: §6.5 step 4 says the
        // endpoint "performs the same internal tie-break", so the same
        // outcome must follow as by the eager route.
        assert_eq!(
            local.greatest(peer.canonical()),
            Some(peer.nth_timestamp(1)),
            "§6.6: the tie-break really ran (local_is_winner = {local_is_winner})"
        );
        if local_is_winner {
            assert!(d.is_silent(), "§6.6 step 3: the winner drops it silently");
        } else {
            assert_eq!(d.transmits().len(), 1, "§6.6 step 4: the loser writes msg2");
            assert_eq!(d.installs().len(), 1, "and completes its own Connecting");
        }
    }
}

/// §6.4:1436-1440 with ruling 74: the interception fires on a **parked**
/// intro, so a chain the application already holds is *not* intercepted — a
/// second `read_identity()` after the static became PENDING returns the same
/// static at 0 incremental DH, and runs no tie-break.
///
/// This is the precondition for §6.4's PENDING branch existing at all: §6.4
/// says in terms that §6.5's interception "cannot fire on a chain the
/// application already holds", and that this is why the branch is needed to
/// close the `read_identity()` → `connect()` → `accept()` ordering. A core
/// that intercepts here deletes that ordering's whole argument.
///
/// **Mutation caught:** hoisting the "claim ∈ pending outbound remotes" test
/// to the top of `read_identity()`, unconditional on chain state. Every
/// step-4 test above still passes; §6.4's PENDING branch becomes dead code;
/// and the application that followed §6.4's documented ordering gets
/// `Internal` on a chain it has already been given.
#[test]
fn a_second_read_identity_after_the_static_became_pending_does_not_intercept() {
    let now = t0();
    let (mut local, mut peer) = tie_pair(now);
    let msg1 = real_msg1(&mut peer, now, &local);
    let d = local.feed(now, peer.addr, &msg1);
    let (id, _) = d.one_intro();

    let first = local
        .ep
        .read_identity(now, id)
        .expect("an ordinary parked intro reveals its claim");
    assert_eq!(first.as_ref(), peer.canonical());

    // §6.4's ordering: read_identity(), then connect().
    local.dial(now, peer.addr, &peer.public_static);

    local.reset_dh();
    let second = local
        .ep
        .read_identity(now, id)
        .expect("ruling 74: idempotent, and §6.4:1439 says the interception cannot fire here");
    let d = local.drain();

    assert_eq!(second.as_ref(), peer.canonical(), "the same claim, again");
    assert_eq!(local.dh(), 0, "ruling 74: 0 DH, opening no provider");
    assert!(d.is_silent(), "no tie-break ran: nothing was written");
    assert_eq!(
        local.greatest(peer.canonical()),
        None,
        "no tie-break ran, so nothing was recorded"
    );
    assert!(local.present(id), "the chain is still the application's");
}

/// §6.5 step 3, first bullet: an initiation whose claim **is** a pending
/// outbound remote "never touches the accept queue and the application never
/// sees it". Both key orders — the routing decision is taken at `es`, before
/// §6.7's comparison is even applied, so neither outcome may surface an
/// `Intro`.
///
/// **Mutation caught:** an implementation that runs the tie-break *and* parks
/// the packet (belt and braces). The application would then be offered an
/// `Intro` for a peer it is already dialling, `accept()` it, and install a
/// second session for one static — §16.1's invariant, broken by an
/// over-cautious core rather than a missing one.
#[test]
fn an_internally_routed_init_never_surfaces_as_an_intro() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (winner, loser) = tie_pair(now);
        let (mut local, mut peer) = if local_is_winner {
            (winner, loser)
        } else {
            (loser, winner)
        };

        let msg1 = real_msg1(&mut peer, now, &local);
        local.dial(now, peer.addr, &peer.public_static);
        let (disp, d) = local.datagram(now, peer.addr, &msg1);

        assert!(
            d.intros().is_empty(),
            "§6.5 step 3: the application never sees it (local_is_winner = {local_is_winner})"
        );
        assert_eq!(
            disp,
            Disposition::Done,
            "the endpoint consumed it; nothing routes to a connection core"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. §6.6 — the internal tie-break completion
// ═══════════════════════════════════════════════════════════════════════

/// §6.5 step 3's precondition, built once: `local` holds an in-flight
/// outbound pending to `peer`, and `peer`'s crossing msg1 — its **first**
/// initiation, so its timestamp is `peer.nth_timestamp(1)` — is in hand.
///
/// The peer keeps its own pending, which is what makes this a *crossing*
/// initiation and not a replay.
fn crossing(now: Instant, local: &mut Ep, peer: &mut Ep) -> (ConnectionId, Vec<u8>) {
    let msg1 = real_msg1(peer, now, local);
    let (conn, d) = local.dial(now, peer.addr, &peer.public_static);
    assert_eq!(d.transmits().len(), 1, "our own msg1 went out");
    assert!(
        local.hints().contains(&peer.addr),
        "§17.4: it is now a hint"
    );
    (conn, msg1)
}

/// `(local, peer)` with `local` on the requested side of §6.7's comparison.
fn sides(now: Instant, local_is_winner: bool) -> (Ep, Ep) {
    let (winner, loser) = tie_pair(now);
    if local_is_winner {
        (winner, loser)
    } else {
        (loser, winner)
    }
}

/// §6.5 step 3 (1 DH, `es`) + §6.6 step 1 (`ss`, +1 DH) and **no more** when
/// we win: step 3's winner writes no msg2, so `ee`/`se` are never paid.
///
/// **Mutation caught:** a winner side that reads the inbound to completion
/// and builds msg2 before deciding to discard it — 4 DH for a packet that is
/// thrown away. §6.7 is explicit that the winner's mid-state is *discarded*,
/// not spent; the difference is invisible except in the count.
#[test]
fn winning_the_internal_tie_break_costs_two_dh() {
    let now = t0();
    let (mut local, mut peer) = sides(now, true);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);

    local.reset_dh();
    let _ = local.feed(now, peer.addr, &msg1);
    assert_eq!(
        local.dh(),
        2,
        "§6.5 step 3's `es` plus §6.6 step 1's `ss`, and nothing else"
    );
}

/// §6.5 step 3 (1) + §6.6 step 1 (`ss`, +1) + step 4's msg2 (`ee`, `se`, +2)
/// = **4 DH** when we lose.
///
/// Paired with [`winning_the_internal_tie_break_costs_two_dh`]: the two
/// counts are the arithmetic the spec itself writes down, and a build that
/// always takes one branch of §6.7's comparison fails whichever of the two it
/// does not take.
#[test]
fn losing_the_internal_tie_break_costs_four_dh() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);

    local.reset_dh();
    let _ = local.feed(now, peer.addr, &msg1);
    assert_eq!(
        local.dh(),
        4,
        "§6.5 step 3's `es`, §6.6 step 1's `ss`, and step 4's `ee` + `se`"
    );
}

/// §6.6 step 3 / §6.7: our static smaller ⇒ the authenticated inbound is
/// **silently dropped** — no msg2, no `Intro`, no `Install`, no failure.
///
/// **Mutation caught:** a winner side that answers anyway. Both peers would
/// then install as responder over the other's msg1, hold two key sets, and go
/// mutually dark until `DEAD_TIMEOUT` — §6.7's stated reason for the
/// tie-break existing.
#[test]
fn winning_the_internal_tie_break_drops_the_inbound_silently() {
    let now = t0();
    let (mut local, mut peer) = sides(now, true);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);

    let d = local.feed(now, peer.addr, &msg1);
    assert!(
        d.is_silent(),
        "§6.6 step 3: a winner-side drop emits nothing at all, got {:?}",
        d.outs
    );
}

/// §6.7: on the winner side "our own outbound completes normally" — the
/// pending is untouched, so its retransmit train keeps running.
///
/// **Mutation caught:** a winner side that cancels its pending (the loser's
/// action, applied to the wrong branch). Nothing would be on the wire, and
/// the peer — which lost, cancelled, and is waiting for our msg1's answer —
/// would wait for ever. The drain being silent cannot see this; only the
/// next retransmit can.
#[test]
fn winning_the_internal_tie_break_leaves_the_pending_retransmitting() {
    let now = t0();
    let (mut local, mut peer) = sides(now, true);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);
    let _ = local.feed(now, peer.addr, &msg1);

    let later = now + past_retransmit();
    let d = local.timeout(later);
    assert!(
        d.deadline.is_some(),
        "§16.5: a live pending arms a retransmit and a give-up"
    );
    let (to, data) = d.one_transmit();
    assert_eq!(
        to, peer.addr,
        "§5.5: the retransmit goes to the dialled address"
    );
    assert_eq!(data.len(), INIT_PACKET_LEN);
    assert_eq!(data[0], PKT_HANDSHAKE_INIT, "it is a fresh initiation");
    assert!(
        d.failures().is_empty(),
        "no give-up: the pending is alive and well"
    );
    assert_eq!(
        local.basis(peer.canonical()),
        Some(None),
        "§17.4: still ours, still dialled"
    );
    assert!(
        local.hints().contains(&peer.addr),
        "an in-flight outbound pending is still a hint"
    );
}

/// §6.6 step 3 and §6.7: the winner-side drop is silent but **not
/// record-free** — the loser's timestamp is recorded in the guard.
///
/// This is the write that "denies a later replay of that same initiation the
/// vacuous guard pass it would otherwise enjoy" (§6.7), and §17.1 counts it
/// as one of the four guard write sites.
///
/// **Mutation caught:** a winner side that simply discards the mid-state and
/// returns. The connection still forms, every functional test still passes,
/// and §6.7's single-use bound on a captured msg1 silently evaporates.
#[test]
fn winning_the_internal_tie_break_records_the_losers_timestamp() {
    let now = t0();
    let (mut local, mut peer) = sides(now, true);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);
    assert_eq!(
        local.greatest(peer.canonical()),
        None,
        "nothing recorded before the tie-break"
    );

    let _ = local.feed(now, peer.addr, &msg1);

    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(1)),
        "§6.6 step 3 / §6.7: the winner records the loser's timestamp"
    );
}

/// §17.4 and §6.6's closing paragraph: the two sides write **complementary**
/// bases. The winner is the connection initiator, so its basis stays `None`;
/// the loser admitted as responder, so its basis is `Some(t)` for the
/// admitted timestamp.
///
/// Asserted as a pair in one body **deliberately**: `Some(None)` alone is
/// also what an untouched pending shows, so a winner-side basis assertion
/// tested on its own would pass a core that did nothing at all (working rule
/// 9). It is the *difference* between the two orders that carries the pin.
///
/// **Mutation caught:** writing `Some(t)` on the winner side (§17.4: "a
/// tie-break we won ... teaches us no timestamp of the peer's"), or leaving
/// `None` on the loser side — which would make §6.4's replacement rule refuse
/// every future genuine reconnect from that peer, permanently.
#[test]
fn the_replacement_basis_is_complementary_across_the_two_key_orders() {
    let now = t0();

    let (mut w_local, mut w_peer) = sides(now, true);
    let (_c, w_msg1) = crossing(now, &mut w_local, &mut w_peer);
    let _ = w_local.feed(now, w_peer.addr, &w_msg1);
    let winner_basis = w_local.basis(w_peer.canonical());

    let (mut l_local, mut l_peer) = sides(now, false);
    let (_c, l_msg1) = crossing(now, &mut l_local, &mut l_peer);
    let _ = l_local.feed(now, l_peer.addr, &l_msg1);
    let loser_basis = l_local.basis(l_peer.canonical());

    assert_eq!(
        winner_basis,
        Some(None),
        "§17.4: `None` when we dialled, and a tie-break we won is one of those"
    );
    assert_eq!(
        loser_basis,
        Some(Some(l_peer.nth_timestamp(1))),
        "§17.4: the tie-break loser's admit step (§6.6 step 4) writes `Some(t)`"
    );
    assert_ne!(
        winner_basis, loser_basis,
        "the two key orders must reach different states, or the comparison \
         is not being applied at all"
    );
}

/// §6.6 step 4: the loser mints a responder index (§17.3) and writes msg2.
/// §5.6: the session anchors at the **msg1 source**, which is where msg2 goes,
/// and §3.3's `receiver_index` is the inbound initiation's `sender_index`.
///
/// **Mutation caught:** answering the *pending's* index instead of the
/// inbound initiation's. §5.5 step 2 matches a msg2 by index, so the peer
/// would drop it and both sides would wait out `HANDSHAKE_GIVEUP` — the
/// exact failure ruling 91 measured, reintroduced one field to the left.
#[test]
fn losing_the_internal_tie_break_writes_msg2_answering_the_inbound_index() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);

    let d = local.feed(now, peer.addr, &msg1);
    let (to, data) = d.one_transmit();

    assert_eq!(to, peer.addr, "§5.6: anchored at the msg1 source");
    assert_eq!(data.len(), RESP_PACKET_LEN, "§3.3, exact");
    assert_eq!(data[0], PKT_HANDSHAKE_RESP);
    let (_ours, theirs) = resp_indices(&data);
    assert_eq!(
        theirs,
        init_sender_index(&msg1),
        "§3.3: the receiver_index answers the initiation we read"
    );
}

/// §5.6 with §6.5's false negative: the anchor is the **msg1 source**, which
/// is not always the address we dialled. Reached through §6.5 step 4's
/// backstop, since a source that differs from the dialled address is exactly
/// what misses the hint set.
///
/// **Mutation caught:** anchoring msg2 at the pending's dialled address —
/// invisible in every test where the two coincide, and a silent failure to
/// answer a NAT-rewritten peer, which is the one population §6.5's
/// false-negative discussion is about.
#[test]
fn the_losers_msg2_anchors_at_the_msg1_source_not_the_dialled_address() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let msg1 = real_msg1(&mut peer, now, &local);

    local.dial(now, peer.addr, &peer.public_static);
    let rewritten = SocketAddr::new(peer.addr.ip(), peer.addr.port() ^ 0x0F00);
    let d = local.feed(now, rewritten, &msg1);
    let (id, _) = d.one_intro();

    let err = local
        .ep
        .read_identity(now, id)
        .expect_err("§6.5 step 4 intercepts");
    assert_eq!(err, IntroError::Internal);
    let d = local.drain();

    let (to, data) = d.one_transmit();
    assert_eq!(
        to, rewritten,
        "§5.6: the responder anchors at the initiation's msg1 source"
    );
    assert_ne!(to, peer.addr, "and that is not the address we dialled");
    assert_eq!(data.len(), RESP_PACKET_LEN);
}

/// §6.6 step 4 and §6.7:1650-1653: the admission "completes the connection as
/// an `Install`, resolving its `Connecting` exactly as a msg2 completion
/// would". The connection is **the pending's own** — connect resolution is
/// edge-triggered exactly once per connection lifecycle.
///
/// **Mutation caught:** minting a fresh `ConnectionId` for the admission and
/// leaving the pending's `Connecting` unresolved. The session would be
/// perfectly good and completely unreachable: the application is awaiting the
/// `Connecting`, which now resolves only at `HANDSHAKE_GIVEUP` — ruling 91's
/// measured 90-second `TimedOut`, with a working session sitting beside it.
#[test]
fn losing_the_internal_tie_break_completes_the_pendings_own_connection() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (conn, msg1) = crossing(now, &mut local, &mut peer);

    let d = local.feed(now, peer.addr, &msg1);
    let installs = d.installs();
    assert_eq!(installs.len(), 1, "exactly one Install, got {installs:?}");
    assert_eq!(
        installs[0], conn,
        "§6.7: the tie-break's Install resolves the pending's own Connecting"
    );
    assert!(
        d.failures().is_empty(),
        "§6.7: the loser cancels its pending with no give-up and no error"
    );
}

/// §6.7: the loser's pending "and its index" are dropped — so it neither
/// retransmits nor ever gives up.
///
/// **Mutation caught:** admitting the inbound while leaving the outbound
/// pending in place. Both peers would hold a session, and 90 seconds later
/// the loser would fire `HandshakeFailed(TimedOut)` on a connection that has
/// been established the whole time — resolving one `Connecting` twice, which
/// §6.7 forbids in terms.
#[test]
fn losing_the_internal_tie_break_cancels_the_pending_so_it_neither_retransmits_nor_gives_up() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);
    let _ = local.feed(now, peer.addr, &msg1);

    let d = local.timeout(now + past_retransmit());
    assert!(
        d.transmits().is_empty(),
        "the pending is gone: nothing to retransmit, got {:?}",
        d.transmits()
    );

    let d = local.timeout(now + past_giveup());
    assert!(
        !d.timed_out(),
        "the pending is gone: nothing to give up on, got {:?}",
        d.failures()
    );
}

/// §17.4 and §6.5 step 2: once the pending is cancelled and the connection
/// installed, its address must leave the hint set — "established connections
/// contribute no hints".
///
/// **Mutation caught:** flipping the static map row from PENDING to LIVE
/// without clearing its dialled address. The hint set would then contain an
/// established connection's address for the rest of the endpoint's life,
/// making every later initiation from that peer take §6.5's eager path — 1 DH
/// each, off a `HandshakeInit` anyone can send — and turning §6.5's
/// in-flight-dial oracle into the configuration oracle it explicitly is not.
#[test]
fn losing_the_internal_tie_break_drops_the_dialled_address_from_the_hint_set() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);
    assert!(local.hints().contains(&peer.addr), "a hint while dialling");

    let _ = local.feed(now, peer.addr, &msg1);

    assert!(
        !local.hints().contains(&peer.addr),
        "§17.4: established connections contribute no hints, got {:?}",
        local.hints()
    );
}

/// §6.6 step 4: the strictly-greater timestamp is recorded **as a full
/// admission**.
///
/// **Mutation caught:** admitting without recording. The very next replay of
/// that same captured initiation would pass the guard vacuously again.
#[test]
fn losing_the_internal_tie_break_records_the_admitted_timestamp() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);

    let _ = local.feed(now, peer.addr, &msg1);

    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(1)),
        "§6.6 step 4 records the admitted timestamp"
    );
}

/// §17.1: the guard entry an admission writes is **pinned** while the
/// connection it belongs to is live, so it survives orphan aging.
///
/// The loser side is the interesting one: the pin the in-flight pending held
/// is released when the pending is cancelled, and the installed connection's
/// pin has to take over **in the same call**.
///
/// **Mutation caught:** cancelling the pending (releasing its §17.1
/// key-holder pin) without the new connection taking one. The record survives
/// the call and every assertion in
/// [`losing_the_internal_tie_break_records_the_admitted_timestamp`], then
/// ages out silently at `TS_GUARD_ORPHAN_TTL` — 15 seconds later, with no
/// output, no error, and §6.7's single-use bound gone.
#[test]
fn losing_the_internal_tie_break_pins_the_record_against_orphan_aging() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (_conn, msg1) = crossing(now, &mut local, &mut peer);
    let _ = local.feed(now, peer.addr, &msg1);

    assert!(
        local.ep.guard_pins(peer.canonical()) >= 1,
        "the installed connection pins its static's guard entry (§17.1)"
    );

    let _ = local.timeout(now + TS_GUARD_ORPHAN_TTL + Duration::from_millis(1));
    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(1)),
        "§17.1: a pinned entry is never evicted or aged"
    );
}

/// The mirror, stated as one assertion: the same crossing initiation, the
/// same code path, opposite outcomes — decided only by which static is
/// smaller.
///
/// **Mutation caught:** a core that ignores §6.7's comparison and always
/// takes one branch. Every single-order test in this file can be satisfied by
/// such a core; only comparing the two orders side by side cannot. That
/// build is not hypothetical — it is what a first implementation reaches for
/// when the comparison looks like a detail.
#[test]
fn the_internal_tie_break_takes_opposite_branches_in_the_two_key_orders() {
    let now = t0();

    let (mut w_local, mut w_peer) = sides(now, true);
    let (_c, w_msg1) = crossing(now, &mut w_local, &mut w_peer);
    let w = w_local.feed(now, w_peer.addr, &w_msg1);

    let (mut l_local, mut l_peer) = sides(now, false);
    let (_c, l_msg1) = crossing(now, &mut l_local, &mut l_peer);
    let l = l_local.feed(now, l_peer.addr, &l_msg1);

    assert!(w.is_silent(), "smaller static ⇒ winner ⇒ silent drop");
    assert_eq!(l.transmits().len(), 1, "larger static ⇒ loser ⇒ msg2");
    assert_eq!(l.installs().len(), 1, "larger static ⇒ loser ⇒ Install");
    assert!(w.installs().is_empty(), "the winner installs nothing here");
}

/// **[RATIFIED 2026/08/15 — ruling 106]** §6.6 step 4 installs a peer that
/// **dialled** as the **responder**, and §6.7 fixes that "for the life of the
/// connection: stream-ID parity (§9.1) is fixed by this outcome."
///
/// The two `Install` routes must therefore carry **opposite** roles: a msg2
/// completion is the initiator, a lost tie-break's admission is the
/// responder — on a connection `connect()` created, which is what makes the
/// obvious inference wrong.
///
/// **Mutation caught:** a connection core deriving its role from "I was
/// created by `connect()`". Every single-route test passes under it, and so
/// does every test in this file that predates ruling 106, because both ends
/// still agree on the parity of every stream they open *themselves*. It
/// diverges only on the streams the **peer** opens, and only on this one
/// path — a silent, half-of-one-route defect that no §6 test can see and
/// that slice 4's stream tests would have blamed on §9.1.
#[test]
fn the_tie_break_loser_installs_as_responder_and_a_msg2_completion_as_initiator() {
    let now = t0();

    // §6.6 step 4: we dialled, lost, and admit the inbound.
    let (mut l_local, mut l_peer) = sides(now, false);
    let (_c, l_msg1) = crossing(now, &mut l_local, &mut l_peer);
    let l = l_local.feed(now, l_peer.addr, &l_msg1);
    assert_eq!(
        l.install_roles(),
        vec![Role::Responder],
        "§6.6 step 4: a dialling peer that loses the tie-break installs as responder"
    );

    // The ordinary route: our msg1, their staged accept, their msg2, our
    // completion. The responder walks §6's ladder rather than answering
    // msg1 directly, so the msg2 comes out of `accept`, not out of `feed`.
    let (mut a, mut b) = sides(now, true);
    let msg1 = real_msg1(&mut a, now, &b);
    let (id, _) = b.feed(now, a.addr, &msg1).one_intro();
    b.ep.authenticate(now, id).expect("genuine msg1");
    b.ep.accept(now, id).expect("NONE static, fresh accept");
    let msg2 = b.drain().one_transmit().1;
    let d = a.feed(now, b.addr, &msg2);
    assert_eq!(
        d.install_roles(),
        vec![Role::Initiator],
        "a dial completed by msg2 installs as initiator"
    );
}

/// §6.6 step 1: "a forged claim of a pending static dies here at the msg1
/// tail's AEAD tag. A failure leaves the in-flight outbound pending
/// untouched." §6.7 states the consequence: **a forgery cannot cancel a
/// pending.**
///
/// Both key orders, because the loser side is where the damage would be: a
/// build that decided the tie-break at `es` — where the claim is merely
/// *claimed* — would cancel a live dial on an unauthenticated packet anyone
/// can construct.
///
/// **Mutation caught:** running §6.7's comparison on the `es` match rather
/// than after `ss`. §6.7 says the match at `es` "selects the tie-break path
/// but decides nothing"; a core that decides there hands any off-path
/// attacker who knows two statics a one-packet denial of every dial between
/// them.
#[test]
fn a_forged_tail_dies_at_step_one_and_cannot_cancel_a_pending() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (mut local, mut peer) = sides(now, local_is_winner);
        let (_conn, genuine) = crossing(now, &mut local, &mut peer);
        let forged = forge_tail(&genuine, &local);
        assert_ne!(forged, genuine);

        local.reset_dh();
        let d = local.feed(now, peer.addr, &forged);

        assert!(
            d.is_silent(),
            "§6.6: a step-1 failure is a silent drop (local_is_winner = \
             {local_is_winner}), got {:?}",
            d.outs
        );
        assert_eq!(
            local.dh(),
            2,
            "§6.5 step 3's `es` plus §6.6 step 1's `ss`; the tag fails after both"
        );
        assert_eq!(
            local.greatest(peer.canonical()),
            None,
            "§6.6: nothing recorded on a step-1 failure"
        );
        assert_eq!(
            local.basis(peer.canonical()),
            Some(None),
            "the pending is untouched"
        );

        let d = local.timeout(now + past_retransmit());
        assert_eq!(
            d.transmits().len(),
            1,
            "the pending is untouched, so it retransmits (local_is_winner = \
             {local_is_winner})"
        );
    }
}

/// §6.6 step 2: the per-static greatest-timestamp guard admits **strictly
/// greater**, or the initiation dies there — and a step-2 failure records
/// nothing.
///
/// Exercised on the winner side, where a step-2 death and a step-3 winner
/// drop are both silent, so the *only* thing separating them is the guard's
/// contents afterwards. `TimestampGuard::record` overwrites rather than
/// taking a maximum, so a core that skips step 2 walks `greatest` backwards
/// to the older initiation — re-arming the replay of everything in between.
///
/// **Mutation caught:** omitting step 2 entirely, or ordering it before step
/// 1 (a `<=` guard test on an unauthenticated claim would be a write ordered
/// on attacker-chosen bytes, which §17.1's "only key-holders write guard
/// entries" forbids).
#[test]
fn the_guard_step_refuses_an_older_initiation_without_recording_it() {
    let now = t0();
    let (mut local, mut peer) = sides(now, true);

    // The peer's first initiation, then its retransmit — §5.5 rule 2 makes
    // the second strictly greater.
    let first = real_msg1(&mut peer, now, &local);
    let second = {
        let d = peer.timeout(now + past_retransmit());
        let (_, data) = d.one_transmit();
        data
    };
    assert_ne!(first, second);
    local.dial(now, peer.addr, &peer.public_static);

    // Feed the NEWER one first: the winner records its timestamp.
    let d = local.feed(now, peer.addr, &second);
    assert!(d.is_silent());
    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(2)),
        "the winner recorded the newer initiation"
    );

    // Now the older one. Genuine, authentic, and no longer admissible.
    local.reset_dh();
    let d = local.feed(now, peer.addr, &first);

    assert!(d.is_silent(), "a step-2 failure is a silent drop");
    assert_eq!(local.dh(), 2, "step 1 still ran: `es` + `ss`");
    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(2)),
        "§6.6: a step-2 failure records nothing — `greatest` must not walk back"
    );
    assert_eq!(
        local.basis(peer.canonical()),
        Some(None),
        "the pending is untouched"
    );
}

/// SPEC GAP 1 (see the bottom of this file): §6.5's routing rule does not say
/// what becomes of a packet whose **eager** split intro read fails. This test
/// asserts only what every reading of §6.5 shares, and deliberately says
/// nothing about whether an `Intro` surfaces.
///
/// **Mutation caught:** an eager-read failure that falls through into the
/// internal path anyway — cancelling a pending, or writing a msg2, off
/// unreadable bytes. That is the same class of defect as deciding at `es`,
/// and reachable by anyone who can compute one keyed hash.
#[test]
fn a_malformed_init_from_a_hint_source_writes_nothing_and_leaves_the_pending_alone() {
    let now = t0();
    let (mut local, peer) = sides(now, false);
    local.dial(now, peer.addr, &peer.public_static);

    let junk = forged_init(&local, 0x1234_5678, 0xAB);
    let d = local.feed(now, peer.addr, &junk);

    assert!(
        d.transmits().is_empty(),
        "unreadable bytes must not produce a msg2"
    );
    assert!(d.installs().is_empty(), "nor an Install");
    assert!(d.failures().is_empty(), "nor a failure on our own dial");
    assert_eq!(
        local.greatest(peer.canonical()),
        None,
        "nothing authenticated, so nothing recorded"
    );
    assert_eq!(
        local.basis(peer.canonical()),
        Some(None),
        "our pending is untouched"
    );

    let d = local.timeout(now + past_retransmit());
    assert_eq!(d.transmits().len(), 1, "the dial is still running");
    // Deliberately unasserted: whether `d.intros()` is empty. SPEC GAP 1.
}

/// SPEC GAP 2: §6.5 step 4 names `read_identity()` as the interception point,
/// but ruling 75 lets `authenticate()` drive the very same stage-0 read on a
/// parked chain, and §6.5 does not say what that verb sees.
///
/// Both readings converge, and §6.6 step 2 says why they must: the internal
/// route and §6.4's PENDING branch are "a different route to the same
/// comparison ... they can never disagree". So this test asserts the
/// **conclusion** rather than the error, in both key orders — which is the
/// strongest claim §6.6 makes and the one no single-route test can reach.
///
/// **Mutation caught:** a core in which the two routes disagree — the
/// interception firing on one side of the comparison and the staged branch on
/// the other, or either route reaching the opposite verdict. §6.6 step 2 says
/// that is what keeps two crossing initiations convergent; nothing else in
/// this file tests the two routes against each other.
#[test]
fn both_routes_to_the_comparison_reach_the_same_conclusion() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (mut local, mut peer) = sides(now, local_is_winner);

        // Parked first, so the chain exists before the dial — then the
        // application follows §6.4's ordering with `authenticate()`.
        let msg1 = real_msg1(&mut peer, now, &local);
        let d = local.feed(now, peer.addr, &msg1);
        let (id, _) = d.one_intro();
        local.dial(now, peer.addr, &peer.public_static);

        let authed = local.ep.authenticate(now, id);
        if let Err(e) = &authed {
            assert_ne!(
                *e,
                AuthError::HandshakeFailed,
                "§18.1 makes this a security signal and ruling 78 forbids \
                 pointing it at a peer whose msg1 is genuine"
            );
            assert_ne!(*e, AuthError::Replay, "the guard admitted this timestamp");
        }
        if authed.is_ok() {
            // §6.4's PENDING branch. Whatever it returns, the conclusion
            // below is the same one §6.6's internal route reaches.
            let _ = local.ep.accept(now, id);
        }
        let _ = local.drain();

        if local_is_winner {
            assert_eq!(
                local.basis(peer.canonical()),
                Some(None),
                "winner, by either route: the pending stands and we stay the \
                 initiator"
            );
            assert_eq!(
                local.greatest(peer.canonical()),
                Some(peer.nth_timestamp(1)),
                "winner, by either route: the candidate's timestamp is recorded \
                 (§6.4:1401-1406 — the one `Stale` that keeps its record — and \
                 §6.7's winner-side record are the same write)"
            );
            assert!(
                local.hints().contains(&peer.addr),
                "winner, by either route: the pending is still in flight"
            );
        } else {
            assert_eq!(
                local.basis(peer.canonical()),
                Some(Some(peer.nth_timestamp(1))),
                "loser, by either route: we installed as responder at that \
                 initiation's timestamp (§17.4)"
            );
            assert!(
                !local.hints().contains(&peer.addr),
                "loser, by either route: the pending is cancelled"
            );
            let d = local.timeout(now + past_giveup());
            assert!(
                !d.timed_out(),
                "loser, by either route: no pending is left to give up"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. §6.4's PENDING branch — the other route to the same comparison
// ═══════════════════════════════════════════════════════════════════════

/// §6.4's ordinary API ordering, up to the `accept()`: the peer's msg1 is
/// parked and read, *then* we dial the same peer, *then* we authenticate.
/// Returns the intro, our pending's `ConnectionId`, and our own msg1.
fn ordinary_ordering(
    now: Instant,
    local: &mut Ep,
    peer: &mut Ep,
) -> (IntroId, ConnectionId, Vec<u8>) {
    let msg1 = real_msg1(peer, now, local);
    let d = local.feed(now, peer.addr, &msg1);
    let (intro, _) = d.one_intro();

    let claimed = local.ep.read_identity(now, intro).expect("§6.1 stage 1");
    assert_eq!(claimed.as_ref(), peer.canonical());

    let (conn, d) = local.dial(now, peer.addr, &peer.public_static);
    let (_, ours) = d.one_transmit();

    local
        .ep
        .authenticate(now, intro)
        .expect("a genuine crossing msg1 authenticates");
    (intro, conn, ours)
}

/// §6.4:1415-1421: our static smaller ⇒ `accept()` returns
/// `AcceptError::Stale`, the pending is left in place, and the candidate's
/// timestamp is **recorded**.
///
/// §6.4:1401-1406 makes this the **one** `Stale` that keeps its guard record:
/// "No other `Stale` leaves a record behind." The contrast case is in the
/// same body on purpose — a core that never reverts anything would satisfy
/// the first half alone, so the pin is the two halves disagreeing.
///
/// **Mutation caught:** applying §6.4's ordering clause uniformly and
/// reverting here too. That deletes §6.7's winner-side record by the staged
/// route while leaving it intact by the internal route — the two routes
/// disagreeing, which §6.6 step 2 says can never happen.
#[test]
fn the_pending_branch_winner_returns_stale_and_is_the_one_stale_that_keeps_its_record() {
    let now = t0();

    // (a) The winner-side PENDING branch: Stale, and the record stands.
    let (mut local, mut peer) = sides(now, true);
    let (intro, pending_conn, _ours) = ordinary_ordering(now, &mut local, &mut peer);
    let refused = local.ep.accept(now, intro);
    let d = local.drain();

    assert_eq!(
        refused.err(),
        Some(AcceptError::Stale),
        "§6.4: the tie-break winner refuses the accept"
    );
    assert_eq!(
        local.greatest(peer.canonical()),
        Some(peer.nth_timestamp(1)),
        "§6.4:1401-1406: this `Stale` keeps its record"
    );
    assert!(d.transmits().is_empty(), "a refused accept writes no msg2");
    assert!(d.installs().is_empty(), "and installs no second connection");
    assert_eq!(
        local.basis(peer.canonical()),
        Some(None),
        "§6.4: the pending is left in place"
    );

    // The pending really is alive: it retransmits, on the connection the
    // dial minted.
    let d = local.timeout(now + past_retransmit());
    let (to, _) = d.one_transmit();
    assert_eq!(to, peer.addr);
    assert!(
        resolutions(&d, pending_conn).is_empty(),
        "§6.4: a refused accept resolves nothing — the dial is still running"
    );

    // (b) The contrast: an ordinary chain that never accepts reverts its
    // provisional record (§17.1 mitigation (i)). Same endpoint pair, same
    // verbs, opposite outcome for the guard.
    let now2 = t0();
    let (mut plain, mut stranger) = tie_pair(now2);
    let msg1 = real_msg1(&mut stranger, now2, &plain);
    let d = plain.feed(now2, stranger.addr, &msg1);
    let (id, _) = d.one_intro();
    plain
        .ep
        .authenticate(now2, id)
        .expect("genuine msg1 authenticates");
    assert_eq!(
        plain.greatest(stranger.canonical()),
        Some(stranger.nth_timestamp(1)),
        "the record is provisional but real while the chain lives"
    );
    plain.ep.reject(now2, id);
    let _ = plain.drain();
    assert_eq!(
        plain.greatest(stranger.canonical()),
        None,
        "§17.1 mitigation (i): every other end-without-accepting reverts"
    );
}

/// §6.4:1410-1414: the peer's static smaller ⇒ the `accept()` **cancels** the
/// pending, its `Connecting` resolves `Err(ConnectError::AlreadyConnected)`,
/// and the accept proceeds as an ordinary fresh install with this endpoint as
/// responder.
///
/// ENCODING PROPOSAL (integrator, not the spec): §6.4 names the *value*
/// (`ConnectError::AlreadyConnected`) delivered to the cancelled
/// `Connecting`, and `EndpointOutput::HandshakeFailed(ConnectionId,
/// ConnectError)` is the only representable carrier for it in §16.4's drain.
/// If slice 4 introduces another output for it, rename the *lookup* below —
/// the assertion is that the pending's connection is resolved exactly once,
/// with that value, and that is the spec's.
///
/// **Mutation caught:** cancelling the pending without resolving its
/// `Connecting`. The dial's future then hangs until `HANDSHAKE_GIVEUP` and
/// resolves `TimedOut` — 90 s later, on a peer we are connected to. Ruling
/// 91's failure again, one branch over.
#[test]
fn the_pending_branch_loser_cancels_the_pending_and_installs_as_responder() {
    let now = t0();
    let (mut local, mut peer) = sides(now, false);
    let (intro, pending_conn, _ours) = ordinary_ordering(now, &mut local, &mut peer);

    let accepted = local.ep.accept(now, intro);
    let d = local.drain();

    let (new_conn, _c) = accepted.expect("§6.4: the tie-break loser accepts");
    assert_ne!(
        new_conn, pending_conn,
        "§6.4: the accept is an ordinary fresh install, not the pending's \
         connection — the pending was cancelled, not completed"
    );

    let (to, data) = d.one_transmit();
    assert_eq!(to, peer.addr, "§5.6: anchored at the msg1 source");
    assert_eq!(data.len(), RESP_PACKET_LEN);
    assert_eq!(data[0], PKT_HANDSHAKE_RESP);

    assert_eq!(
        d.failures(),
        vec![(pending_conn, ConnectError::AlreadyConnected)],
        "§6.4:1412-1413: the cancelled pending's `Connecting` resolves \
         `Err(ConnectError::AlreadyConnected)` — exactly once"
    );
    assert!(
        d.installs().is_empty(),
        "the accept's connection is returned, not installed: §16.4 emits \
         `ToConnection` only for a `connect()`-created connection"
    );
    assert_eq!(
        local.basis(peer.canonical()),
        Some(Some(peer.nth_timestamp(1))),
        "§17.4: we responded, at that initiation's timestamp"
    );
    assert!(
        !local.hints().contains(&peer.addr),
        "§17.4: the cancelled pending contributes no hint"
    );

    let d = local.timeout(now + past_giveup());
    assert!(!d.timed_out(), "there is no pending left to give up on");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. The headline — ruling 91's measurement, as an assertion
// ═══════════════════════════════════════════════════════════════════════

/// `read_identity()` → `connect()` → `accept()` on one peer, driven on both
/// endpoints, with the crossing msg1s delivered in both directions.
///
/// This is §6.4:1436's "ordinary API ordering". Ruling 91 measured the
/// pre-slice-4 core on exactly this and got **both sides `TimedOut` at 90 s**
/// — "measured at both ends, not inferred" — because without §6.5's routing
/// and §6.6's internal completion neither side's kept pending can finish.
fn ordinary_api_ordering(local_is_winner: bool) {
    let now = t0();
    let (mut local, mut peer) = sides(now, local_is_winner);

    // 1. The peer dials us first; its msg1 crosses.
    let (peer_conn, d) = peer.dial(now, local.addr, &local.public_static);
    let (to, msg1_peer) = d.one_transmit();
    assert_eq!(to, local.addr);

    // 2. §6.5 step 2: we have dialled nobody, so it parks.
    let d = local.feed(now, peer.addr, &msg1_peer);
    let (intro, _) = d.one_intro();

    // 3–5. The documented ordering.
    let claimed = local.ep.read_identity(now, intro).expect("§6.1 stage 1");
    assert_eq!(claimed.as_ref(), peer.canonical());
    let (local_conn, d) = local.dial(now, peer.addr, &peer.public_static);
    let (to, msg1_local) = d.one_transmit();
    assert_eq!(to, peer.addr);

    // Our msg1 reaches a peer that is holding a pending to us:
    // §6.5 step 3 → §6.6's internal completion.
    let dp = peer.feed(now, local.addr, &msg1_local);

    // 6–7. On the chain we already hold: §6.4's PENDING branch.
    local
        .ep
        .authenticate(now, intro)
        .expect("a genuine crossing msg1 authenticates");
    let accepted = local.ep.accept(now, intro);
    let dl = local.drain();

    if local_is_winner {
        // We win by §6.4's route; the peer loses by §6.6's.
        assert_eq!(
            accepted.err(),
            Some(AcceptError::Stale),
            "§6.4: the winner refuses"
        );
        assert!(dl.transmits().is_empty() && dl.installs().is_empty());

        assert_eq!(
            dp.installs(),
            vec![peer_conn],
            "§6.6 step 4: the loser's admission completes its own Connecting"
        );
        let (to, msg2) = dp.one_transmit();
        assert_eq!(to, local.addr, "§5.6: anchored at our msg1's source");
        assert_eq!(msg2.len(), RESP_PACKET_LEN);

        // Our own outbound completes normally (§6.7).
        let d = local.feed(now, peer.addr, &msg2);
        assert_eq!(
            d.installs(),
            vec![local_conn],
            "§6.7: the winner's own outbound completes, on its own connection"
        );
    } else {
        // We lose by §6.4's route; the peer wins by §6.6's.
        let (_new_conn, _c) = accepted.expect("§6.4: the loser accepts");
        let (to, msg2) = dl.one_transmit();
        assert_eq!(to, peer.addr);
        assert_eq!(msg2.len(), RESP_PACKET_LEN);

        assert!(
            dp.is_silent(),
            "§6.6 step 3: the winner drops our msg1 silently, got {:?}",
            dp.outs
        );
        assert_eq!(
            peer.greatest(local.canonical()),
            Some(local.nth_timestamp(1)),
            "§6.7: and records its timestamp anyway"
        );

        // The peer's own outbound completes on our msg2.
        let d = peer.feed(now, local.addr, &msg2);
        assert_eq!(
            d.installs(),
            vec![peer_conn],
            "§6.7: the winner's own outbound completes, on its own connection"
        );
    }

    // ── Both sides, both orders ───────────────────────────────────────
    let (winner_side, loser_side) = if local_is_winner {
        (&local, &peer)
    } else {
        (&peer, &local)
    };
    assert_eq!(
        winner_side.basis(loser_side.canonical()),
        Some(None),
        "§17.4 / §6.7: the tie-break winner is the connection initiator"
    );
    assert!(
        matches!(loser_side.basis(winner_side.canonical()), Some(Some(_))),
        "§17.4: the loser installed as responder"
    );
    assert!(
        local.hints().is_empty() && peer.hints().is_empty(),
        "§17.4: nothing is dialling any more — local {:?}, peer {:?}",
        local.hints(),
        peer.hints()
    );

    // ── Ruling 91's measurement ───────────────────────────────────────
    let late = now + past_giveup();
    let dl = local.timeout(late);
    let dp = peer.timeout(late);
    assert!(
        !dl.timed_out(),
        "ruling 91: the pre-slice-4 core ends here with `TimedOut` on this \
         side (local_is_winner = {local_is_winner}), got {:?}",
        dl.failures()
    );
    assert!(
        !dp.timed_out(),
        "ruling 91: and with `TimedOut` on the other side too, got {:?}",
        dp.failures()
    );
}

/// The headline, with the accepting side on the winning half of §6.7's
/// comparison.
///
/// **Mutation caught:** the whole of §6.5 and §6.6 missing — the state ruling
/// 91 measured. It fails at the first `assert` that needs the peer to have
/// routed our msg1 internally, and again, definitively, at the give-up.
#[test]
fn the_ordinary_api_ordering_completes_both_sides_when_we_win() {
    ordinary_api_ordering(true);
}

/// The mirror. Same script, the accepting side on the losing half.
///
/// **Mutation caught:** everything the winning case catches, plus a core that
/// implements only one branch of §6.7's comparison — which passes the winning
/// case in full.
#[test]
fn the_ordinary_api_ordering_completes_both_sides_when_we_lose() {
    ordinary_api_ordering(false);
}

/// §16.1, restated as the outcome §6.4:1441-1448 promises: "Either way that
/// ordering against one static yields exactly one connection."
///
/// **Mutation caught:** a core that installs the accept's session *and*
/// completes the pending — two sessions, two key sets, one static. The
/// `StaticMap` refuses the second row, so the failure mode is a lost session
/// rather than a visible duplicate; counting the resolutions is what sees it.
#[test]
fn the_ordinary_api_ordering_yields_exactly_one_resolution_per_dial() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (mut local, mut peer) = sides(now, local_is_winner);

        let (peer_conn, d) = peer.dial(now, local.addr, &local.public_static);
        let (_, msg1_peer) = d.one_transmit();
        let d = local.feed(now, peer.addr, &msg1_peer);
        let (intro, _) = d.one_intro();
        local.ep.read_identity(now, intro).expect("stage 1");
        let (local_conn, d) = local.dial(now, peer.addr, &peer.public_static);
        let (_, msg1_local) = d.one_transmit();

        let mut local_events = Vec::new();
        let mut peer_events = Vec::new();

        let dp = peer.feed(now, local.addr, &msg1_local);
        peer_events.extend(resolutions(&dp, peer_conn));

        local.ep.authenticate(now, intro).expect("genuine");
        let accepted = local.ep.accept(now, intro).is_ok();
        let dl = local.drain();
        local_events.extend(resolutions(&dl, local_conn));

        // Deliver whichever msg2 exists, in whichever direction.
        for (to, data) in dl.transmits() {
            if data.len() == RESP_PACKET_LEN {
                assert_eq!(to, peer.addr);
                let d = peer.feed(now, local.addr, &data);
                peer_events.extend(resolutions(&d, peer_conn));
            }
        }
        for (to, data) in dp.transmits() {
            if data.len() == RESP_PACKET_LEN {
                assert_eq!(to, local.addr);
                let d = local.feed(now, peer.addr, &data);
                local_events.extend(resolutions(&d, local_conn));
            }
        }

        assert_eq!(
            local_events.len(),
            1,
            "§6.7: connect resolution is edge-triggered exactly once per \
             connection lifecycle (local_is_winner = {local_is_winner}), got \
             {local_events:?}"
        );
        assert_eq!(
            peer_events.len(),
            1,
            "and once on the peer, got {peer_events:?}"
        );

        // Exactly one connection to that static on each side: the accept's
        // return counts on the side that took §6.4's loser branch.
        assert!(local.basis(peer.canonical()).is_some());
        assert!(peer.basis(local.canonical()).is_some());
        assert_eq!(
            accepted, !local_is_winner,
            "§6.4: the winner refuses the accept and the loser takes it"
        );
    }
}

/// How a dial's `Connecting` was resolved in one drain: an `Install` or a
/// `HandshakeFailed`, both keyed on the dial's own `ConnectionId`.
#[derive(Debug, PartialEq, Eq)]
enum Resolution {
    Installed,
    Failed(ConnectError),
}

fn resolutions(d: &Drained, conn: ConnectionId) -> Vec<Resolution> {
    let mut v: Vec<Resolution> = d
        .installs()
        .into_iter()
        .filter(|c| *c == conn)
        .map(|_| Resolution::Installed)
        .collect();
    v.extend(
        d.failures()
            .into_iter()
            .filter(|(c, _)| *c == conn)
            .map(|(_, e)| Resolution::Failed(e)),
    );
    v
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Gap-safe invariants — what holds under *every* reading
// ═══════════════════════════════════════════════════════════════════════

/// SPEC GAP 5: ruling 90 split `connect()` into `mint_pending` (0 DH) and
/// `start_attempt` (2 DH), creating a state §6.5 predates — a pending that
/// exists, holds its static and sits in the pending tables, with **no
/// initiation in flight**. §6.5 says the hint set is "the dialled addresses
/// of all *in-flight outbound initiations*"; §17.4 says it is "the pending
/// tables' dialled addresses". Those two phrases named the same set before
/// ruling 90 and no longer do.
///
/// This test therefore asserts nothing about which route the crossing msg1
/// takes. It asserts §16.1, which both readings owe: **one static, one
/// connection**, and one resolution of the dial.
///
/// **Mutation caught:** a probed set and a §6.4 PENDING test that disagree
/// about this state — the packet routed to the staged path because no
/// initiation is in flight, and then admitted by an `accept()` whose PENDING
/// check reads the same emptiness. Two sessions for one static, which is the
/// invariant §6.4:1441-1448 says the comparison exists to hold.
#[test]
fn a_minted_but_unattempted_pending_still_yields_one_connection_for_one_static() {
    for local_is_winner in [true, false] {
        let now = t0();
        let (mut local, mut peer) = sides(now, local_is_winner);
        let msg1 = real_msg1(&mut peer, now, &local);

        // `mint_pending` without `start_attempt`: ruling 90 makes this
        // reachable, and a dial cancelled before the driver ran leaves it.
        let conn = local.mint_only(now, peer.addr, &peer.public_static);
        let _ = local.drain();

        let d = local.feed(now, peer.addr, &msg1);
        let mut resolved = resolutions(&d, conn);

        // If it surfaced, the application follows §6.4's ordering on it.
        if let Some((id, _)) = d.intros().first().copied() {
            if local.ep.read_identity(now, id).is_ok()
                && local.ep.authenticate(now, id).is_ok()
                && let Ok((accept_conn, _c)) = local.ep.accept(now, id)
            {
                assert_ne!(accept_conn, conn, "an accept never returns the dial's id");
            }
            let d = local.drain();
            resolved.extend(resolutions(&d, conn));
        }

        assert!(
            local.basis(peer.canonical()).is_some(),
            "§16.1: exactly one row for that static, whichever route ran \
             (local_is_winner = {local_is_winner})"
        );
        assert!(
            resolved.len() <= 1,
            "§6.7: a dial resolves at most once, got {resolved:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SPEC GAPS
// ═══════════════════════════════════════════════════════════════════════
//
// Every gap found while writing this file, and — for each — what this file
// does instead of inventing behaviour and pinning it as if ratified. Each
// test above that touches a gap names it; this list is the index, and every
// test in this file has been checked against it.
//
// GAP 1 — §6.5 step 3 does not say what becomes of a packet whose EAGER
//   split intro read fails.
//   Step 2 parks; step 3 demotes on a *successful* read whose claim is not a
//   pending outbound remote; step 3's first bullet routes internally. A read
//   that does not complete is in none of the three, and §6.5's four numbered
//   cases read as exhaustive (working rule 8). Two settled readings exist and
//   the spec chooses neither: (a) silent drop, by analogy with
//   `IntroError::Malformed`, which discards the chain and frees its slot
//   (§6.1, ruling 72); (b) park anyway, since step 2's park is what a
//   0-DH arrival gets and the eager read is an optimisation, not a filter.
//   Ruling 72's *other* half sharpens it: a **local** provider failure leaves
//   a chain PARKED while a **peer** failure discards it, and §6.5 mentions
//   neither, so an eager read that fails because our own enclave is locked
//   may not be treated as the peer's fault.
//   → `a_malformed_init_from_a_hint_source_writes_nothing_and_leaves_the_pending_alone`
//     asserts only what both readings share, and says so at the point where
//     it declines to assert.
//
// GAP 2 — §6.5 step 4 names `read_identity()` as the interception point, but
//   ruling 75 lets `authenticate()` drive the same stage-0 read on a parked
//   chain, and §18.1's `AuthError` has no `Internal` variant to report it
//   with. The frozen core's own comment says `IntroError::Internal` is
//   "not reachable from the core" — a statement slice 4 makes false.
//   → `both_routes_to_the_comparison_reach_the_same_conclusion` asserts the
//     conclusion (§6.6 step 2's "they can never disagree") rather than the
//     error, in both key orders, and additionally asserts only that the
//     verdict is not `HandshakeFailed`/`Replay` — which §18.1 and ruling 78
//     forbid for a genuine peer independently of how this gap is settled.
//
// GAP 3 — §6.5 step 4 does not say what becomes of the PARKED ENTRY once the
//   interception has run. On the loser branch the admission plainly consumes
//   it; on the winner branch "the authenticated inbound is silently dropped"
//   describes the packet, not the stage-0 slot, and §6.3's per-source cap
//   makes the difference observable to a peer.
//   → no test in this file asserts the entry's disposition, and none depends
//     on it: every test that intercepts either stops at the verdict or
//     asserts guard/basis/wire state only.
//
// GAP 4 — §6.5 step 3's demotion happens "under the §6.3 rules", which
//   include eviction and same-source replacement. So a demoted packet may be
//   dropped or may replace an existing entry, after 1 DH has been spent on
//   it, and §6.3's dedup key (source-scoped) is coarser than the mid-state it
//   would now be discarding. The spec's cross-reference is explicit enough
//   that this is a scope note rather than a contradiction, but it is the one
//   place where §6.5 spends DH that §6.3 may then throw away.
//   → no test here drives the demotion into a full queue.
//
// GAP 5 — ruling 90 created a pending with no initiation in flight
//   (`mint_pending` without `start_attempt`), which §6.5's "in-flight
//   outbound initiations" and §17.4's "the pending tables' dialled addresses"
//   now describe differently. The same ambiguity reaches §6.4's PENDING
//   branch, which is worded "if an in-flight outbound initiation exists".
//   → `a_minted_but_unattempted_pending_still_yields_one_connection_for_one_static`
//     asserts §16.1 alone, which both readings owe.
//
// ── Not gaps, but obligations §6.6 creates that this file does not cover ──
//
// §17.1's HANDSHAKE_GIVEUP pin extension — "an entry written by the internal
//   tie-break's admit step (§6.6 step 4) or by a winner-side record stays
//   exempt from orphan aging AND LRU eviction for 90 s after the connection
//   it belongs to dies ... if that outbound never completed, the 90 s runs
//   from its own HANDSHAKE_GIVEUP expiry instead." Reaching it needs a
//   connection teardown (`handle_connection_event`) and, for the winner
//   variant, a give-up — both beyond §6.5/§6.6. §6.7 says the single-use
//   bound "evaporates" without it, so it wants a test.
//   `losing_the_internal_tie_break_pins_the_record_against_orphan_aging`
//   covers only the live-connection half.
//
// §6.5's "applications that dial SHOULD also drain accept()" and the
//   both-NAT-rewritten double failure at HANDSHAKE_GIVEUP are shell-level
//   and paused-clock, not core-level.

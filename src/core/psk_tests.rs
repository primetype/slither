//! §6.1's ladder over the **`IKpsk1`** pattern — ruling 280's pin.
//!
//! The property, in one sentence: *a dial without the PSK yields no
//! session and costs the responder at most the one `es` DH.*
//!
//! # Why this file has its own fixture
//!
//! [`super::tests`]'s `Ep` is monomorphic in `ReferenceSuite` — `Drained`
//! holds `Vec<EndpointOutput<Suite>>` — and making 3 300 lines of frozen
//! ladder tests generic over the suite to add four is the wrong trade. The
//! harness below is the same shape, cut down to what these tests drive.
//!
//! # What separates these tests from the `IK` ones they mirror
//!
//! Rule 9: a bound is only a test if the degenerate case violates it. An
//! implementation that ignored the PSK entirely would pass
//! "`read_identity` costs 1 DH" and "a matching PSK establishes" — both
//! are true of plain `IK`. [`wrong_psk_fails_at_authenticate`] is the
//! assertion that separates them, and
//! [`the_psk_is_mixed_at_complete_not_at_intro`] is the one that pins
//! *where*: the same wrong key must be free at stage 1 and fatal at
//! stage 2, which is the entire ordering claim §2.2 makes for the pattern.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use hiss::noise::{Blake2b, ChaChaPoly, P256};
use hiss::psk::Psk;

use super::{Endpoint, EndpointOutput, IntroId};
use crate::config::Config;
use crate::constants::{INIT_PACKET_LEN, RESP_PACKET_LEN};
use crate::error::AuthError;
use crate::identity::{Identity, PublicKeyOf};
use crate::testutil::{CountingIdentity, DhCounter};

// ═══════════════════════════════════════════════════════════════════════
// The suite under test
// ═══════════════════════════════════════════════════════════════════════

crate::channel_psk! {
    /// A pairing suite: the reference triple over `IKpsk1`.
    ///
    /// Deliberately the **same** `<Curve, Cipher, Hash>` as
    /// `ReferenceSuite`, so every difference these tests observe is the
    /// pattern's and nothing else's.
    pub PairingSuite<P256, ChaChaPoly, Blake2b>;
}

type Id = CountingIdentity<PairingSuite>;
type Pk = PublicKeyOf<Id>;

/// The key both ends of a successful pairing hold.
fn shared_psk() -> Psk {
    Psk::from_bytes([0x5A; 32])
}

/// A key the responder never enrolled.
fn stranger_psk() -> Psk {
    Psk::from_bytes([0xA5; 32])
}

// ═══════════════════════════════════════════════════════════════════════
// Harness — `super::tests::Ep`'s shape, over `PairingSuite`
// ═══════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct Drained {
    outs: Vec<EndpointOutput<PairingSuite>>,
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

    fn one_transmit(&self) -> (SocketAddr, Vec<u8>) {
        let mut ts = self.transmits();
        assert_eq!(ts.len(), 1, "expected exactly one transmit");
        ts.pop().expect("checked")
    }

    fn intros(&self) -> Vec<IntroId> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                EndpointOutput::IntroReady(id, _) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn one_intro(&self) -> IntroId {
        let ids = self.intros();
        assert_eq!(ids.len(), 1, "expected exactly one IntroReady");
        ids[0]
    }

    /// Sessions handed to a `connect()`-created connection (§16.4's
    /// `ToConnection`). A responder-side `accept()` emits none — it
    /// returns the established connection directly — so on these tests
    /// this counts only the **initiator** side completing.
    fn installs(&self) -> usize {
        self.outs
            .iter()
            .filter(|o| matches!(o, EndpointOutput::ToConnection(..)))
            .count()
    }
}

struct Ep {
    ep: Endpoint<Id>,
    dhs: DhCounter,
    public_static: Pk,
    addr: SocketAddr,
}

impl Ep {
    fn new(now: Instant, key_seed: u8, rng_seed: u8, addr: SocketAddr) -> Self {
        let identity: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = identity.counter();
        let public_static = *identity.public_static();
        Ep {
            ep: Endpoint::new(now, Config::default(), identity, [rng_seed; 32]),
            dhs,
            public_static,
            addr,
        }
    }

    fn canonical(&self) -> &[u8] {
        self.public_static.as_ref()
    }

    /// §16.4's contract: drain to the terminal `Timeout`.
    fn drain(&mut self) -> Drained {
        let mut d = Drained::default();
        for _ in 0..100_000 {
            match self.ep.poll_output() {
                EndpointOutput::Timeout(_) => return d,
                other => d.outs.push(other),
            }
        }
        panic!("poll_output() did not terminate (§16.4)");
    }

    fn feed(&mut self, now: Instant, src: SocketAddr, dgram: &[u8]) -> Drained {
        self.ep.handle_datagram(now, src, dgram);
        self.drain()
    }

    /// Ruling 90's split dial, carrying the pre-shared key.
    fn connect(&mut self, now: Instant, remote: SocketAddr, peer: &Pk, psk: Psk) -> Drained {
        let (conn, _core) = self
            .ep
            .mint_pending(now, remote, *peer, psk)
            .expect("a fresh static dials");
        let _ = self.drain();
        self.ep.start_attempt(now, conn);
        self.drain()
    }
}

fn v4(last: u8, port: u16) -> SocketAddr {
    format!("10.0.0.{last}:{port}").parse().expect("literal")
}

fn t0() -> Instant {
    Instant::now() + Duration::from_secs(3600)
}

fn pair(now: Instant) -> (Ep, Ep) {
    (
        Ep::new(now, 7, 0x11, v4(1, 1)),
        Ep::new(now, 9, 0x22, v4(2, 2)),
    )
}

/// A real msg1 from `a` to `b`, sealed under `psk`.
fn msg1_under(a: &mut Ep, now: Instant, b: &Ep, psk: Psk) -> Vec<u8> {
    let peer = b.public_static;
    let drained = a.connect(now, b.addr, &peer, psk);
    let (to, data) = drained.one_transmit();
    assert_eq!(to, b.addr);
    assert_eq!(
        data.len(),
        INIT_PACKET_LEN,
        "§2.3: the `psk` token puts no bytes on the wire, so an IKpsk1 \
         init packet is the same length as an IK one"
    );
    data
}

// ═══════════════════════════════════════════════════════════════════════
// §6.1's ladder, over IKpsk1
// ═══════════════════════════════════════════════════════════════════════

/// §6.1 row 2 over `IKpsk1`: the intro read is **1 DH** and reveals the
/// claimed static — *and it does so for a dialler whose PSK we do not
/// hold*.
///
/// This is the ordering the pattern exists for. The `psk` token sits after
/// msg1's `s`, so the identity is on the table before the key is chosen;
/// the responder here is handed a stranger's msg1 sealed under a key it
/// never enrolled, and still learns who is calling for one `es`.
#[test]
fn intro_reveals_the_claimed_static_at_one_dh_even_under_an_unknown_psk() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = msg1_under(&mut a, t, &b, stranger_psk());
    let id = b.feed(t, a.addr, &msg1).one_intro();
    assert_eq!(b.dhs.get(), 0, "the arrival itself is free");

    let claimed = b.ep.read_identity(t, id).expect("a real msg1 is readable");
    let _ = b.drain();

    assert_eq!(b.dhs.get(), 1, "the intro read is exactly one `es`");
    assert_eq!(
        claimed.as_ref(),
        a.canonical(),
        "the claimed static is the dialler's, PSK notwithstanding"
    );
}

/// **The ruled property.** A dial without the PSK yields no session and
/// costs the responder **at most the one `es` DH**.
///
/// The rejection is a *drop*: the application looks at the claimed static,
/// finds no enrolled key for it, and never calls `authenticate_with`. The
/// proving `ss` is never paid, nothing is transmitted, and nothing is
/// installed — which is what makes an unauthenticated flood against a
/// pairing window cost what §6.9 already prices, and no more.
#[test]
fn a_dial_without_the_psk_yields_no_session_and_costs_one_dh() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = msg1_under(&mut a, t, &b, stranger_psk());
    let id = b.feed(t, a.addr, &msg1).one_intro();

    // Stage 1: who is this? One `es`.
    let claimed = b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 1);

    // The application holds no PSK for that static, so it declines — the
    // decision costs nothing, because the key was never needed to make it.
    assert_ne!(
        claimed.as_ref(),
        b.canonical(),
        "sanity: the claim is the peer's static, not our own"
    );
    b.ep.reject(t, id);
    let d = b.drain();

    assert_eq!(
        b.dhs.get(),
        1,
        "declining an unenrolled stranger ran the proving `ss`"
    );
    assert!(
        d.transmits().is_empty(),
        "a declined pairing answered on the wire"
    );
    assert_eq!(d.installs(), 0, "a declined pairing installed a session");
}

/// A **wrong** PSK is not a cheaper failure than a wrong identity, and not
/// a distinguishable one: it fails msg1's tail tag at stage 2, at §6.1's
/// ordinary 2 DH, as [`AuthError::HandshakeFailed`].
///
/// Rule 9 — this is the assertion that separates the implementation from
/// one that ignores the PSK entirely. Every other test in this file passes
/// against plain `IK`; this one does not.
#[test]
fn wrong_psk_fails_at_authenticate() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    // `a` seals msg1 under a key `b` does not hold.
    let msg1 = msg1_under(&mut a, t, &b, stranger_psk());
    let id = b.feed(t, a.addr, &msg1).one_intro();
    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();

    let outcome = b.ep.authenticate(t, id, &shared_psk());
    let d = b.drain();

    assert!(
        matches!(outcome, Err(AuthError::HandshakeFailed)),
        "a wrong PSK must fail the tail tag, got {outcome:?}"
    );
    assert_eq!(
        b.dhs.get(),
        2,
        "the failure costs §6.1's ordinary 2 DH — no more, and no less"
    );
    assert!(d.transmits().is_empty(), "a failed pairing sent msg2");
    assert_eq!(d.installs(), 0);
}

/// **Where** the key is mixed, pinned rather than assumed: the *same*
/// wrong PSK is free at stage 1 and fatal at stage 2.
///
/// This is §2.2's ordering claim as an executable assertion. An
/// implementation that mixed the PSK at the intro read — a psk0-shaped
/// pattern, or a `read_message_1_with` lookup closure — would fail stage 1
/// here, and the claimed static would never be observable at all. That the
/// two stages disagree about the same key is the property.
#[test]
fn the_psk_is_mixed_at_complete_not_at_intro() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = msg1_under(&mut a, t, &b, stranger_psk());
    let id = b.feed(t, a.addr, &msg1).one_intro();

    // Stage 1 succeeds under a key that stage 2 will reject.
    let claimed =
        b.ep.read_identity(t, id)
            .expect("the intro read is PSK-blind");
    let _ = b.drain();
    assert_eq!(claimed.as_ref(), a.canonical());
    assert_eq!(b.dhs.get(), 1);

    // Stage 2, same wrong key, now fatal.
    assert!(
        matches!(
            b.ep.authenticate(t, id, &shared_psk()),
            Err(AuthError::HandshakeFailed)
        ),
        "the PSK was not mixed at `complete()` at all"
    );
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 2);
}

/// The happy path: both ends hold the same key, and the ladder is §6.1's
/// unchanged **1 / 2 / 4**.
///
/// The pattern buys admission, not a different cost model — a PSK mixes a
/// key and performs no DH, so every row of §6.1's table is the `IK` row.
#[test]
fn a_matching_psk_establishes_at_the_ordinary_four_dh() {
    let t = t0();
    let (mut a, mut b) = pair(t);
    let msg1 = msg1_under(&mut a, t, &b, shared_psk());
    let id = b.feed(t, a.addr, &msg1).one_intro();

    b.ep.read_identity(t, id).expect("readable");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 1, "§6.1 row 2");

    let (peer, _ts) =
        b.ep.authenticate(t, id, &shared_psk())
            .expect("a matching PSK proves possession");
    let _ = b.drain();
    assert_eq!(b.dhs.get(), 2, "§6.1 row 3");
    assert_eq!(peer.as_ref(), a.canonical());

    let (_conn, _core) = b.ep.accept(t, id).expect("a fresh static accepts");
    let d = b.drain();
    assert_eq!(b.dhs.get(), 4, "§6.1 row 4 — the accept fast path");

    let (to, data) = d.one_transmit();
    assert_eq!(to, a.addr, "§5.6 anchors at the msg1 source");
    assert_eq!(
        data.len(),
        RESP_PACKET_LEN,
        "§2.3: IKpsk1's msg2 is the same length as IK's"
    );
}

/// The wire separates itself: the pattern name seeds the protocol name,
/// and nothing else about the suite moves.
///
/// Both halves matter. The **name differs**, so the two suites derive
/// different initial handshake hashes and cannot complete against each
/// other. The **sizes do not**, so §2.3's derivation — which has no `psk`
/// term in it — is correct for this pattern as written, and a packet that
/// crosses between the two suites gets as far as msg1's first AEAD open
/// (§2.2, ruling 279's same-curve sibling case) rather than dying at the
/// length gate.
#[test]
fn the_psk_suite_renames_the_protocol_and_moves_no_wire_byte() {
    use crate::packet::{Channel, ReferenceSuite};

    assert_eq!(
        <PairingSuite as Channel>::PROTOCOL_NAME,
        "Noise_IKpsk1_P256_ChaChaPoly_BLAKE2b"
    );
    assert_ne!(
        <PairingSuite as Channel>::PROTOCOL_NAME,
        <ReferenceSuite as Channel>::PROTOCOL_NAME,
        "the two patterns must not share an initial handshake hash"
    );

    assert_eq!(
        <PairingSuite as Channel>::MSG1_LEN,
        <ReferenceSuite as Channel>::MSG1_LEN
    );
    assert_eq!(
        <PairingSuite as Channel>::MSG2_LEN,
        <ReferenceSuite as Channel>::MSG2_LEN
    );
    assert_eq!(
        <PairingSuite as Channel>::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_eq!(
        <PairingSuite as Channel>::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );
    // …and against the frozen literals, not only against each other: an
    // equality between two things that both moved would still hold.
    assert_eq!(
        <PairingSuite as Channel>::MSG1_LEN,
        crate::constants::IK_MSG1_LEN
    );
    assert_eq!(
        <PairingSuite as Channel>::MSG2_LEN,
        crate::constants::IK_MSG2_LEN
    );
}

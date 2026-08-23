//! **The in-person pairing window — `IKpsk1` driven end to end on a paused
//! clock.** Ruling 280.
//!
//! The requirement this exists for: a second, short-lived endpoint on its
//! own port, two devices on a LAN, one showing a QR that carries the
//! other's static and a fresh 32-byte PSK. The responder must
//!
//! 1. see the **claimed** static after exactly one `es`;
//! 2. judge it — the peer is a **stranger**, so the `known` set cannot be
//!    the admission test; and
//! 3. complete **only** under the key carried out of band.
//!
//! # What §2.2 says, quoted, because every assertion below comes from it
//!
//! > **There are exactly two patterns: `IK` and `IKpsk1`.** … The
//! > pre-shared key is supplied at the two staged points — the dial and
//! > §6.1's stage 2 — never held by the endpoint, so a responder selects it
//! > **using the claimed static it has just paid one `es` for**. …
//! > Its `psk` token **puts no bytes on the wire**, so §2.3's derivation
//! > holds for it exactly as written and all four sizes equal the reference
//! > suite's. Its protocol name is `Noise_IKpsk1_<curve>_<cipher>_<hash>` —
//! > a different name seeding a different initial handshake hash … And
//! > **endpoints remain monomorphic per suite**: a psk channel is a
//! > *separate endpoint*, on its own port.
//!
//! # The degenerate implementation these tests are written against
//!
//! Working rule 9: a bound is only a test if the broken version violates
//! it. An implementation that accepted the PSK argument and **ignored** it
//! is plain `IK` wearing a different protocol name, and it passes "the
//! pairing succeeds" and "the sizes match" without trouble.
//! [`the_pairing_window_refuses_a_dialler_with_the_wrong_key`] is what it
//! fails, and [`a_stranger_without_the_key_is_dropped_at_one_dh`] is what
//! prices the refusal. Both are written from the *responder's* DH counter,
//! which is the quantity §6.9 actually bounds.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Every test is `#[tokio::test(start_paused = true)]` inside a `LocalSet`
//! (§16.3's `!Send` actor). `tokio::time::timeout` is the *observation*
//! instrument: on the paused clock an idle runtime jumps to the next armed
//! timer, so a resolving future costs no virtual time and a stuck one costs
//! exactly the budget. There is no `sleep` in this file.
//!
//! # Ratified values are written as literals (working rule 9, ruling 271)
//!
//! `INIT_PACKET_LEN` 196 and `RESP_PACKET_LEN` 107 are spelled as values
//! rather than imported: a test written in terms of the constant drifts
//! with it and stops pinning it. A drift turns this file red, which per
//! `CLAUDE.md` is a ruling request and not an expectation to update.

// ══════════════════════════════════════════════════════════════════════
// The pairing suite
// ══════════════════════════════════════════════════════════════════════

/// A module of its own because `channel_psk!` stamps a `hiss::noise!`
/// machine named `IKpsk1` into the invoking module — one invocation per
/// module, exactly as `channel!` requires for `IK`.
mod pairing {
    use slither::prelude::{Blake2b, ChaChaPoly, P256};

    slither::channel_psk! {
        /// The pairing window's suite: the reference triple over
        /// `IKpsk1`. Same `<Curve, Cipher, Hash>` as
        /// `slither::packet::ReferenceSuite`, so every difference these
        /// tests observe belongs to the *pattern*.
        pub PairingSuite<P256, ChaChaPoly, Blake2b>;
    }
}

use std::net::SocketAddr;
use std::time::Duration;

use hiss::psk::Psk;
use slither::config::Config;
use slither::error::AuthError;
use slither::identity::{Identity, PublicKeyOf};
use slither::packet::Channel;
use slither::shell::Endpoint;
use slither::testutil::{CountingIdentity, DhCounter, Network, Tap, local, settle};

use pairing::PairingSuite;

// ── ratified values, as literals ──────────────────────────────────────

/// §3.5 / CLAUDE.md's wire pins: `INIT_PACKET_LEN` = 196 — **for the psk
/// pattern too**, which is the assertion, not the import.
const INIT_PACKET_LEN: usize = 196;

/// §3.5 / CLAUDE.md's wire pins: `RESP_PACKET_LEN` = 107.
const RESP_PACKET_LEN: usize = 107;

/// §3.1: `PKT_HANDSHAKE_INIT` = `0x01`.
const PKT_HANDSHAKE_INIT: u8 = 0x01;

/// §3.1: `PKT_HANDSHAKE_RESP` = `0x02`.
const PKT_HANDSHAKE_RESP: u8 = 0x02;

/// Virtual-time budget for something that must resolve. Costs nothing when
/// the future resolves; exactly this much virtual time when it does not.
const PATIENCE: Duration = Duration::from_secs(5);

/// Application payload, several times §9.1's 1169-byte
/// `MAX_DATAGRAM_PAYLOAD`, so the stream is packetised, sealed, ACKed and
/// reassembled rather than fitting in one sealed packet.
const PAYLOAD_LEN: usize = 8 << 10;

type Id = CountingIdentity<PairingSuite>;

/// The key the QR carried. Both devices hold it; nobody else does.
fn paired_key() -> Psk {
    Psk::from_bytes([0x5A; 32])
}

/// A key an uninvited dialler guessed, or brought from another pairing.
fn wrong_key() -> Psk {
    Psk::from_bytes([0xA5; 32])
}

// ══════════════════════════════════════════════════════════════════════
// Fixture
// ══════════════════════════════════════════════════════════════════════

struct Node {
    ep: Endpoint<Id>,
    /// §6.1's cumulative DH ladder, endpoint-wide — the quantity §6.9
    /// bounds, and the one every cost assertion here reads.
    dhs: DhCounter,
    pk: PublicKeyOf<Id>,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
    fn spawn(net: &Network, seed: u8, addr: SocketAddr) -> Node {
        let id: Id = CountingIdentity::seeded([seed; 32]);
        let dhs = id.counter();
        let pk = *Identity::public_static(&id);
        let ep = Endpoint::builder()
            .identity(id)
            .wire(net.wire(addr))
            .config(Config::new())
            .rng_seed([seed ^ 0xFF; 32])
            .build();
        Node { ep, dhs, pk, addr }
    }
}

fn addr(host: u8, port: u16) -> SocketAddr {
    format!("10.0.{host}.1:{port}").parse().expect("literal")
}

/// Every datagram of packet type `ty` the fabric carried, in order.
fn packets_of(tap: &Tap, ty: u8) -> Vec<slither::testutil::Spied> {
    tap.snapshot()
        .into_iter()
        .filter(|s| s.bytes.first().copied() == Some(ty))
        .collect()
}

/// A deterministic, non-constant payload — a run of equal bytes would
/// survive a reassembly that reordered its packets.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

// ══════════════════════════════════════════════════════════════════════
// The pairing ceremony, end to end
// ══════════════════════════════════════════════════════════════════════

/// **Two devices holding one out-of-band key pair up and carry traffic.**
///
/// The whole ceremony: the responder accepts, reads the claimed identity
/// at one `es`, chooses the key *for that identity*, authenticates, and
/// admits. Then a bidirectional stream moves 8 KiB each way, because a
/// pattern that established and could not carry traffic would pass a
/// handshake-only test.
///
/// The wire assertions are the second half of the story: the psk pattern
/// puts **no extra bytes** on either handshake packet.
#[tokio::test(start_paused = true)]
async fn two_devices_holding_the_qr_key_pair_and_carry_traffic() {
    local(async {
        let net = Network::new();
        let tap = net.tap();
        let device = Node::spawn(&net, 0x11, addr(1, 7001));
        let window = Node::spawn(&net, 0x22, addr(2, 7002));

        let dial = device
            .ep
            .connect_with(window.addr, window.pk, paired_key())
            .expect("the dial is minted");

        let device_pk = device.pk;
        let ceremony = async {
            let intro = window.ep.accept().await.expect("the accept queue is open");
            assert_eq!(intro.source(), device.addr, "§5.6 anchors at the source");

            // Stage 1 — one `es`, and the claim is on the table.
            let claimed = intro.read_identity().await.expect("read_identity");
            assert_eq!(
                claimed.claimed_static().as_ref(),
                device_pk.as_ref(),
                "the claimed static is the dialling device's"
            );
            assert_eq!(
                window.dhs.get(),
                1,
                "§6.1 row 2: the intro read is exactly one `es`"
            );

            // Stage 2 — the key is chosen HERE, with the peer named. That
            // ordering is the entire reason the pattern is IKpsk1.
            claimed
                .authenticate_with(paired_key())
                .await
                .expect("the paired key proves possession")
                .accept()
                .await
                .expect("accept")
        };

        let (dialled, admitted) = tokio::join!(dial, ceremony);
        let dialled = dialled.expect("the paired dial completed");

        assert_eq!(
            window.dhs.get(),
            4,
            "§6.1 row 4: admission is the ordinary four DH — a `psk` token \
             mixes a key and performs no DH"
        );

        // ── The session carries traffic in both directions ────────────
        let want = payload(PAYLOAD_LEN);
        let (mut tx, mut rx) = dialled.open_bi().await.expect("open_bi").split();
        let echo = async {
            let (mut etx, mut erx) = admitted
                .accept_bi()
                .await
                .expect("the peer sees the stream")
                .split();
            let mut got = vec![0u8; PAYLOAD_LEN];
            let mut have = 0;
            while have < PAYLOAD_LEN {
                let n = erx
                    .read(&mut got[have..])
                    .await
                    .expect("read")
                    .expect("the stream did not finish early");
                assert!(n >= 1, "a blocked read is `Pending`, never `Ok(Some(0))`");
                have += n;
            }
            let mut done = 0;
            while done < PAYLOAD_LEN {
                done += etx.write(&got[done..]).await.expect("write");
            }
            // The halves are returned, not dropped. Dropping a
            // `SendStream` resets the stream (§16.11), which would race
            // the read-back below and surface as `Reset(0)` — the echo
            // would be indistinguishable from a session that failed.
            (got, etx, erx)
        };
        let feed = async {
            let mut done = 0;
            while done < PAYLOAD_LEN {
                done += tx.write(&want[done..]).await.expect("write");
            }
        };
        let (_, (seen, _etx, _erx)) = tokio::join!(feed, echo);
        assert!(
            seen == want,
            "the payload did not survive the pairing session"
        );

        let mut back = vec![0u8; PAYLOAD_LEN];
        let mut have = 0;
        while have < PAYLOAD_LEN {
            let n = tokio::time::timeout(PATIENCE, rx.read(&mut back[have..]))
                .await
                .expect("the echo resolved")
                .expect("read")
                .expect("the echo stream did not finish early");
            have += n;
        }
        assert!(back == want, "the echo did not survive the pairing session");

        settle().await;

        // ── §2.3 on the wire: the `psk` token costs nothing ───────────
        let inits = packets_of(&tap, PKT_HANDSHAKE_INIT);
        assert!(!inits.is_empty(), "no initiation reached the fabric");
        for init in &inits {
            assert_eq!(
                init.bytes.len(),
                INIT_PACKET_LEN,
                "an IKpsk1 init packet is not 196 bytes — §2.3's derivation \
                 has no psk term, so this is a wire change and needs a ruling"
            );
        }
        let resps = packets_of(&tap, PKT_HANDSHAKE_RESP);
        assert!(!resps.is_empty(), "no response reached the fabric");
        for resp in &resps {
            assert_eq!(resp.bytes.len(), RESP_PACKET_LEN);
        }
    })
    .await;
}

/// **A dialler with the wrong key gets no session**, and the refusal is
/// [`AuthError::HandshakeFailed`] — the same undetailed verdict a forged
/// identity gets (§18.1: no oracle).
///
/// This is the test the PSK-ignoring implementation fails.
#[tokio::test(start_paused = true)]
async fn the_pairing_window_refuses_a_dialler_with_the_wrong_key() {
    local(async {
        let net = Network::new();
        let uninvited = Node::spawn(&net, 0x33, addr(3, 7003));
        let window = Node::spawn(&net, 0x22, addr(2, 7002));

        let _dial = uninvited
            .ep
            .connect_with(window.addr, window.pk, wrong_key())
            .expect("the dial is minted");

        let intro = tokio::time::timeout(PATIENCE, window.ep.accept())
            .await
            .expect("an initiation arrived")
            .expect("the accept queue is open");

        // The claim is readable — the identity is revealed before the key
        // is consulted, which is the point of the pattern.
        let claimed = intro.read_identity().await.expect("read_identity");
        assert_eq!(
            claimed.claimed_static().as_ref(),
            uninvited.pk.as_ref(),
            "the uninvited dialler still names itself at one `es`"
        );
        assert_eq!(window.dhs.get(), 1);

        // The window offers the key it actually holds. The dialler does
        // not hold it, so msg1's tail tag fails.
        let outcome = claimed.authenticate_with(paired_key()).await;
        assert!(
            matches!(outcome, Err(AuthError::HandshakeFailed)),
            "a wrong PSK must fail the handshake, got {outcome:?}"
        );
        assert_eq!(
            window.dhs.get(),
            2,
            "the refusal costs §6.1's ordinary 2 DH — no more, and no less"
        );

        settle().await;
    })
    .await;
}

/// **A stranger the window has no key for is dropped at one `es`** — the
/// ruled property, from the responder's side.
///
/// The application looks at the claimed static, finds nothing enrolled,
/// and drops the `Claimed`. §6.2 makes that drop a silent reject. The
/// proving `ss` is never paid: an unauthenticated dialler against a
/// pairing window costs exactly what §6.9's "application probes identity
/// then drops" row already prices, and the pattern adds nothing to it.
#[tokio::test(start_paused = true)]
async fn a_stranger_without_the_key_is_dropped_at_one_dh() {
    local(async {
        let net = Network::new();
        let tap = net.tap();
        let stranger = Node::spawn(&net, 0x44, addr(4, 7004));
        let window = Node::spawn(&net, 0x22, addr(2, 7002));

        let _dial = stranger
            .ep
            .connect_with(window.addr, window.pk, wrong_key())
            .expect("the dial is minted");

        let intro = tokio::time::timeout(PATIENCE, window.ep.accept())
            .await
            .expect("an initiation arrived")
            .expect("the accept queue is open");
        let claimed = intro.read_identity().await.expect("read_identity");
        assert_eq!(window.dhs.get(), 1, "the identity cost one `es`");

        // No enrolled key for this static: decline without choosing one.
        drop(claimed);
        settle().await;

        assert_eq!(
            window.dhs.get(),
            1,
            "dropping an unenrolled stranger ran the proving `ss` — the \
             whole point of revealing the identity first is that it does not"
        );
        assert!(
            packets_of(&tap, PKT_HANDSHAKE_RESP).is_empty(),
            "the window answered a stranger on the wire"
        );
    })
    .await;
}

/// The wire separates the two patterns by **name**, not by length.
///
/// `channel_psk!` over the reference triple derives every §2.3 size equal
/// to `ReferenceSuite`'s and a *different* Noise protocol name. That pair
/// of facts is what §2.2 rests on: the length gate and mac1 cannot tell the
/// suites apart, and a crossed packet dies at msg1's first AEAD open having
/// spent one DH (ruling 279's same-curve sibling case, which an `IKpsk1`
/// sibling is an instance of).
#[test]
fn the_psk_pattern_renames_the_protocol_and_moves_no_wire_byte() {
    use slither::packet::ReferenceSuite;

    assert_eq!(
        <PairingSuite as Channel>::PROTOCOL_NAME,
        "Noise_IKpsk1_P256_ChaChaPoly_BLAKE2b"
    );
    assert_ne!(
        <PairingSuite as Channel>::PROTOCOL_NAME,
        <ReferenceSuite as Channel>::PROTOCOL_NAME,
        "the two patterns must not share an initial handshake hash"
    );

    assert_eq!(<PairingSuite as Channel>::INIT_PACKET_LEN, INIT_PACKET_LEN);
    assert_eq!(<PairingSuite as Channel>::RESP_PACKET_LEN, RESP_PACKET_LEN);
    assert_eq!(
        <PairingSuite as Channel>::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_eq!(
        <PairingSuite as Channel>::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );
}

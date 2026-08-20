//! **S22, clause 1 — "a user can pick a crypto suite" — driven for the
//! *offered* suite, and amended §2.2's same-curve mismatch.**
//!
//! `STORIES.md` §E, S22:
//!
//! > ### S22 — a user can pick a crypto suite, and mismatches fail closed
//! >
//! > - **Accepts:** the suite is declared once via the macro; **a peer on a
//! >   different suite**, or a wrong static, **fails the handshake and
//! >   installs nothing**. An unknown version byte is dropped silently —
//! >   there is no negotiation, ever.
//! > - **Anchor:** §1.1, §2, §3.1. **Paused clock:** yes.
//!
//! # Ruling 279 (2026/08/20) — what changed, and why this file exists
//!
//! hiss 0.4.0 added `AesGcm`, and slither now **offers** the suite
//! `P256 / AesGcm / Blake2b` alongside the reference suite
//! `P256 / ChaChaPoly / Blake2b` (which remains the reference). `AesGcm`
//! joined [`slither::prelude`] the same day. That makes S22's first clause
//! a claim with two witnesses rather than one, and it makes S22's
//! different-suite clause mean something it did not mean before: the two
//! offered suites **share the same curve**.
//!
//! **This file was authored against §2.2 as amended by ruling 279**, quoted
//! here verbatim because every assertion in
//! [`s22_an_aes_initiation_reaches_a_chacha_responders_queue_and_dies_at_the_probe`]
//! is taken from it:
//!
//! > A mismatched-suite packet dies silently: at the length gate or at mac1
//! > when the suites differ in curve — the same fate as garbage — and, for
//! > a same-curve sibling suite (`P256 / AesGcm / Blake2b` against the
//! > reference suite, the first such pair), at msg1's first AEAD open on
//! > the staged ladder: the lengths coincide (§2.3 — `PK` is the only
//! > per-suite quantity) and §4.1's mac1 key carries no suite, so both
//! > gates pass and §6.9's mac1-valid rows price the spend. Either way
//! > nothing installs.
//!
//! The clause the amendment **removed** — *"a mismatched-suite packet dies
//! at one keyed hash and never reaches the DH provider"*, stated for every
//! suite pair — is what the second test below is written to falsify. A
//! build implementing the pre-amendment text has an initiation that never
//! reaches the queue; this file's first ordered assertion is that it
//! **does**.
//!
//! # The mechanism, so the assertions are readable
//!
//! Two suites, one curve. §2.3 derives every per-suite size from `PK` and
//! `TAG`, and `PK` = 65 (P-256, uncompressed SEC1) and `TAG` = 16 are
//! shared, so `MSG1_LEN` 174 / `MSG2_LEN` 81 / `INIT_PACKET_LEN` 196 /
//! `RESP_PACKET_LEN` 107 are **identical**. The length gate cannot tell
//! them apart because there is nothing to tell apart.
//!
//! §4.1 keys mac1 as `BLAKE2b-256(b"slither mac1" ‖
//! recipient_static_canonical)`. No suite, no protocol name, no cipher —
//! public data only. A responder's mac1 key is the same byte string
//! whichever suite the *sender* built its packet with, so an AES-suite
//! initiation addressed to a ChaChaPoly responder's real static verifies.
//!
//! What differs is the Noise protocol name — `Noise_IK_P256_AESGCM_BLAKE2b`
//! against `Noise_IK_P256_ChaChaPoly_BLAKE2b` — which seeds the handshake
//! hash, and the AEAD itself. So the divergence surfaces at the **first
//! AEAD open**, which on §6.1's ladder is inside `read_identity()`, after
//! its `es`. That places the cost on §6.9's line
//! *"mac1-valid, src ∉ hint set, application probes identity then drops —
//! 1 DH, an application-chosen spend"*, and not on the 0-cost garbage row.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Both tests are `#[tokio::test(start_paused = true)]` inside a
//! `LocalSet` (§16.3's `!Send` actor). `tokio::time::timeout` is the
//! *observation* instrument, not a wait: on the paused clock an idle
//! runtime jumps straight to the next armed timer, so a resolving future
//! costs no virtual time and a stuck one costs exactly the budget. The 90 s
//! give-up in the second test resolves in virtual time like every other
//! timer. There is no `sleep` in this file.
//!
//! # Ratified values are written as literals (working rule 9, ruling 271)
//!
//! `INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107, `PKT_HANDSHAKE_INIT`
//! `0x01`, `PKT_HANDSHAKE_RESP` `0x02` and `HANDSHAKE_GIVEUP` 90 s are
//! spelled as values rather than imported from `slither::constants`. A test
//! written in terms of the constant drifts with the constant and stops
//! pinning it; written as a literal, a drift turns this file red — which,
//! per `CLAUDE.md`, is a ruling request and not an expectation to update.
//!
//! # Authorship (working rule 6)
//!
//! Written from `STORIES.md` S22, ruling 279's §2.2/§4.2/§6.9 text and the
//! public API alone. The two-endpoint fixture is the shape
//! `benches/throughput.rs`'s `BenchPair<S>` established for a *suite-generic*
//! pair built from public API only — `testutil::Pair` cannot be used here at
//! all, because every field on its path pins the reference suite.

// ══════════════════════════════════════════════════════════════════════
// The offered suite
// ══════════════════════════════════════════════════════════════════════

/// `P256 / AesGcm / Blake2b`, declared exactly as a consumer declares it.
///
/// A module of its own because `slither::channel!` stamps a `hiss::noise!`
/// state machine named `IK` into the invoking module and takes the
/// identifier as the pattern's `NAME` — one invocation per module, and this
/// file's other suite is `slither::packet::ReferenceSuite`, declared by the
/// same macro inside the library.
///
/// Spelled through [`slither::prelude`], which is where `AesGcm` lives as
/// of ruling 279 — the one blessed spelling.
mod offered_aes {
    use slither::prelude::{AesGcm, Blake2b, P256};

    slither::channel! {
        /// The offered same-curve sibling of the reference suite: §12.4's
        /// AEAD in place of §12.3's, every other parameter held still.
        pub OfferedAes<P256, AesGcm, Blake2b>;
    }
}

use std::net::SocketAddr;
use std::time::Duration;

use slither::config::Config;
use slither::error::{ConnectError, IntroError};
use slither::identity::{Identity, PublicKeyOf};
use slither::packet::{Handshake, ReferenceSuite};
use slither::prelude::P256;
use slither::shell::{Connection, Endpoint, RecvStream, SendStream};
use slither::testutil::{CountingIdentity, DhCounter, Network, Spied, Tap, local, settle};

use offered_aes::OfferedAes;

// ── ratified values, as literals ──────────────────────────────────────

/// §3.5 / CLAUDE.md's wire pins: `INIT_PACKET_LEN` = 196, **for both
/// suites**. That equality is the reason §2.2's length gate cannot
/// separate them.
const INIT_PACKET_LEN: usize = 196;

/// §3.5 / CLAUDE.md's wire pins: `RESP_PACKET_LEN` = 107, for both suites.
const RESP_PACKET_LEN: usize = 107;

/// §3.1: `PKT_HANDSHAKE_INIT` = `0x01`.
const PKT_HANDSHAKE_INIT: u8 = 0x01;

/// §3.1: `PKT_HANDSHAKE_RESP` = `0x02`.
const PKT_HANDSHAKE_RESP: u8 = 0x02;

/// §5.5 step 6: `HANDSHAKE_GIVEUP` = 90 s.
const GIVEUP: Duration = Duration::from_secs(90);

/// §16.5: `SHELL_LATENESS_BOUND` = 250 ms — the shell's permitted lateness
/// against a core deadline.
const SHELL_LATENESS_BOUND: Duration = Duration::from_millis(250);

/// Virtual-time budget for something that must resolve. On the paused clock
/// this costs nothing when the future resolves and exactly this much
/// virtual time when it does not.
const PATIENCE: Duration = Duration::from_secs(5);

/// Application payload moved in the end-to-end test. Seven times §9.1's
/// 1169-byte `MAX_DATAGRAM_PAYLOAD`, so the stream is packetised, sealed,
/// ACKed and reassembled many times over rather than fitting in one sealed
/// packet — an AEAD swap that worked for a single packet and not for a
/// sequence would pass a one-packet test.
const PAYLOAD_LEN: usize = 8 << 10;

// ══════════════════════════════════════════════════════════════════════
// A suite-generic two-endpoint fixture
// ══════════════════════════════════════════════════════════════════════

/// A suite this file can build a fixture over.
///
/// `Curve = P256` is not a preference: `testutil::CountingIdentity` wraps a
/// `SoftwareIdentity` whose provider is `DhProvider<P256>`, which is
/// exactly the bound its own `Identity` impl states. It is also precisely
/// what makes the mismatch below a *same-curve* one. `'static` is
/// `spawn_local`'s bound on the driver task, not a `Send` bound (§16.3).
trait FixtureSuite: Handshake<Curve = P256> + 'static {}

impl<S: Handshake<Curve = P256> + 'static> FixtureSuite for S {}

/// One endpoint on a shared [`Network`], with the bookkeeping the
/// assertions need.
///
/// Generic over the suite, which is the whole point: the two nodes in the
/// mismatch test are **different types** over one fabric.
struct Node<S: FixtureSuite> {
    ep: Endpoint<CountingIdentity<S>>,
    /// §6.1's cumulative DH ladder, endpoint-wide.
    dhs: DhCounter,
    pk: PublicKeyOf<CountingIdentity<S>>,
    addr: SocketAddr,
}

impl<S: FixtureSuite> Node<S> {
    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
    fn spawn(net: &Network, seed: u8, addr: SocketAddr) -> Node<S> {
        let id: CountingIdentity<S> = CountingIdentity::seeded([seed; 32]);
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
    format!("10.0.{host}.1:{port}")
        .parse()
        .expect("literal addr")
}

/// Every datagram of packet type `ty` the fabric carried, in order.
///
/// §3.1 makes byte 0 the packet type for every packet, and the five types
/// are disjoint, so this filter is exact — a Data packet cannot be
/// miscounted as a handshake.
fn packets_of(tap: &Tap, ty: u8) -> Vec<Spied> {
    tap.snapshot()
        .into_iter()
        .filter(|s| s.bytes.first().copied() == Some(ty))
        .collect()
}

/// Everything the fabric carried **from** `from`, of any type.
fn sent_by(tap: &Tap, from: SocketAddr) -> Vec<Spied> {
    tap.snapshot()
        .into_iter()
        .filter(|s| s.src == from)
        .collect()
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all<S: Handshake>(tx: &mut SendStream<S>, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = tokio::time::timeout(PATIENCE, tx.write(&buf[done..]))
            .await
            .unwrap_or_else(|_| {
                panic!("{what}: write still pending after {PATIENCE:?} at byte {done}")
            })
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: a blocked write is `Pending`, never `Ok(0)`"
        );
        done += n;
    }
}

/// Read exactly `want.len()` bytes and assert they are `want`.
async fn read_exact_eq<S: Handshake>(rx: &mut RecvStream<S>, want: &[u8], what: &str) {
    let mut got: Vec<u8> = Vec::with_capacity(want.len());
    let mut buf = [0u8; 1024];
    while got.len() < want.len() {
        let n = tokio::time::timeout(PATIENCE, rx.read(&mut buf))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{what}: read still pending after {PATIENCE:?} with {}/{} bytes in hand",
                    got.len(),
                    want.len()
                )
            })
            .unwrap_or_else(|e| panic!("{what}: read failed with {e:?}"))
            .unwrap_or_else(|| {
                panic!(
                    "{what}: the stream finished after {}/{} bytes",
                    got.len(),
                    want.len()
                )
            });
        assert!(
            n >= 1,
            "{what}: a blocked read is `Pending`, never `Ok(Some(0))`"
        );
        got.extend_from_slice(&buf[..n]);
    }
    assert_eq!(got.len(), want.len(), "{what}: over-read");
    assert!(
        got == want,
        "{what}: the bytes did not survive the round trip"
    );
}

/// A deterministic, non-constant payload — a run of equal bytes would
/// survive a reassembly that reordered its packets.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

// ══════════════════════════════════════════════════════════════════════
// S22 clause 1 — the offered suite, driven end to end
// ══════════════════════════════════════════════════════════════════════

/// **The offered suite is a suite you can actually pick.**
///
/// Two endpoints on `P256 / AesGcm / Blake2b` — declared by
/// `slither::channel!` in this test file exactly as a consumer declares it,
/// which is the only way *"the suite is declared once via the macro"* is a
/// claim about the consumer path rather than about slither's own internals
/// — establish on the paused clock and carry 8 KiB in each direction over a
/// bidirectional stream, plus one unreliable datagram each way.
///
/// Three families of assertion, and each one is here because a different
/// broken build passes without it.
///
/// 1. **§6.1's DH ladder, rung by rung: 0 → 1 → 2 → 4.** The ladder's costs
///    are the *reference* suite's published prices; asserting them for the
///    offered suite is what says the second suite rides the same staged
///    accept rather than a shortcut. A build that eagerly read the AES
///    initiation shows 1 at the `Intro`.
/// 2. **The observed wire: one 196-byte `HandshakeInit`, one 107-byte
///    `HandshakeResp`.** Read off the tap, not derived from
///    `<OfferedAes as Channel>::INIT_PACKET_LEN` — `tests/spec_packet.rs`
///    already pins the associated consts, and a const equality cannot see a
///    packet builder that framed the offered suite differently. This is the
///    *driven* half of ruling 279's "identical wire sizes", and it is the
///    factual premise the mismatch test below depends on: if the lengths
///    ever diverged, that test's length gate would fire and its mechanism
///    would be a different one.
/// 3. **8 KiB both ways, byte-exact.** Seven times `MAX_DATAGRAM_PAYLOAD`,
///    so every layer above the seal — packetisation, the replay window,
///    ACKs, flow-control credit, reassembly — runs many times under the new
///    AEAD. A single-packet echo would pass against an AES-GCM binding
///    whose nonce handling broke on the second packet.
///
/// # The broken builds this separates
///
/// * **`AesGcm` wired but non-functional** — the handshake never completes
///   and `join!` never resolves; the `PATIENCE` timeouts inside the helpers
///   turn that into a named failure rather than a hang.
/// * **A suite-dependent mac1, header or frame layer** (§4.4 fixes mac1 at
///   keyed BLAKE2b for every suite, and §2.2 confines the suite to four
///   sizes) — the observed 196/107 assertions fail.
/// * **A nonce or counter binding that survives one packet** — the 8 KiB
///   transfers fail where a `b"hello"` echo would not.
/// * **The degenerate "it compiled" test** — a file that only declared the
///   suite and asserted its associated consts. That test exists already
///   (`tests/spec_packet.rs`); this one is the driven claim it cannot make.
#[tokio::test(start_paused = true)]
async fn s22_the_offered_aes_suite_carries_a_connection_end_to_end() {
    local(async {
        let net = Network::seeded(0x5322_0AE5);
        let tap = net.tap();
        let a: Node<OfferedAes> = Node::spawn(&net, 0xA1, addr(1, 4001));
        let b: Node<OfferedAes> = Node::spawn(&net, 0xB2, addr(2, 4002));

        // ── §6.2's staged accept, with §6.1's ladder priced rung by rung ──
        let dial =
            a.ep.connect(b.addr, b.pk)
                .expect("connect() on a NONE static is Ok");
        let ladder = async {
            let intro =
                b.ep.accept()
                    .await
                    .expect("§16.2: accept() yields None only when the endpoint is closed");
            assert_eq!(
                intro.source(),
                a.addr,
                "fixture: the introduction is not the one this dial minted"
            );
            assert_eq!(
                b.dhs.get(),
                0,
                "§6.1: stage 0 is **0 DH** on the offered suite exactly as on the \
                 reference suite — an `Intro` that already cost a curve operation is \
                 an eager read, and §6.9's flood accounting rests on this row being zero"
            );

            let claimed = intro
                .read_identity()
                .await
                .expect("§6.1: a genuine same-suite msg1 must be readable");
            assert_eq!(
                b.dhs.get(),
                1,
                "§6.1: read_identity() is **one** DH (`es`). Any other figure means the \
                 offered suite is not riding the same staged ladder"
            );
            assert!(
                claimed.claimed_static().as_ref() == a.pk.as_ref(),
                "§6.1: the claimed static must be the dialler's — the AEAD open that \
                 produced it is the one the mismatch test below watches fail"
            );

            let proven = claimed
                .authenticate()
                .await
                .expect("§6.1: authenticate() on a genuine msg1");
            assert_eq!(
                b.dhs.get(),
                2,
                "§6.1: authenticate() adds `ss` for two DH total"
            );

            let conn = proven.accept().await.expect("§6.2: accept()");
            assert_eq!(
                b.dhs.get(),
                4,
                "§6.1: accept() adds `ee` + `se` for four DH — the full IK ladder, \
                 unchanged by the AEAD"
            );
            conn
        };

        let (dialled, cb): (_, Connection<OfferedAes>) = tokio::join!(dial, ladder);
        let ca = dialled.expect("the dial completed");

        assert!(
            ca.remote_static().as_ref() == b.pk.as_ref(),
            "§16.2: the initiator's peer is the responder it dialled"
        );
        assert!(
            cb.remote_static().as_ref() == a.pk.as_ref(),
            "§16.2: the responder's peer is the dialler"
        );
        assert!(
            ca.is_established() && cb.is_established(),
            "S22: `P256 / AesGcm / Blake2b` is an **offered** suite (ruling 279) — two \
             endpoints on it must establish"
        );

        // ── the observed wire: identical to the reference suite's ────────
        let inits = packets_of(&tap, PKT_HANDSHAKE_INIT);
        let resps = packets_of(&tap, PKT_HANDSHAKE_RESP);
        assert_eq!(
            inits.len(),
            1,
            "fixture: a loss-free fabric completes in one initiation; {} were sent, so \
             the length assertion below is not measuring the packet it names",
            inits.len()
        );
        assert_eq!(
            resps.len(),
            1,
            "fixture: exactly one HandshakeResp on a loss-free fabric; saw {}",
            resps.len()
        );
        assert_eq!(
            inits[0].bytes.len(),
            INIT_PACKET_LEN,
            "ruling 279/§2.3: the offered suite shares P-256's `PK` = 65 and every \
             suite's `TAG` = 16, so its HandshakeInit is 196 bytes — the reference \
             suite's, to the byte. A different length here reopens §2.2's length gate \
             and falsifies the amended text this file is written against"
        );
        assert_eq!(
            resps[0].bytes.len(),
            RESP_PACKET_LEN,
            "ruling 279/§2.3: the offered suite's HandshakeResp is 107 bytes"
        );

        // ── 8 KiB each way over a bidirectional stream ───────────────────
        let bi_a = tokio::time::timeout(PATIENCE, ca.open_bi())
            .await
            .expect("open_bi resolved")
            .expect("open_bi");
        let (mut tx_a, mut rx_a) = bi_a.split();

        let up = payload(PAYLOAD_LEN);
        // A stream is announced by its first frame, so the opener writes
        // before the peer can accept it.
        write_all(&mut tx_a, &up, "a->b under AesGcm").await;

        let bi_b = tokio::time::timeout(PATIENCE, cb.accept_bi())
            .await
            .expect("accept_bi resolved")
            .expect("accept_bi");
        let (mut tx_b, mut rx_b) = bi_b.split();
        read_exact_eq(&mut rx_b, &up, "a->b under AesGcm").await;

        let down: Vec<u8> = up.iter().rev().copied().collect();
        write_all(&mut tx_b, &down, "b->a under AesGcm").await;
        read_exact_eq(&mut rx_a, &down, "b->a under AesGcm").await;

        // ── and the unreliable path, which seals its own packets ─────────
        ca.send_datagram(b"aes datagram a->b")
            .expect("§13: send_datagram");
        assert_eq!(
            tokio::time::timeout(PATIENCE, cb.recv_datagram())
                .await
                .expect("recv_datagram resolved")
                .expect("§13: recv_datagram"),
            b"aes datagram a->b".to_vec(),
            "S22: the offered suite must carry §13's unreliable datagrams too"
        );
        cb.send_datagram(b"aes datagram b->a")
            .expect("§13: send_datagram");
        assert_eq!(
            tokio::time::timeout(PATIENCE, ca.recv_datagram())
                .await
                .expect("recv_datagram resolved")
                .expect("§13: recv_datagram"),
            b"aes datagram b->a".to_vec()
        );

        assert!(
            ca.is_established() && cb.is_established(),
            "S22: the connection died carrying its own traffic"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S22 clause 2 — a same-curve sibling mismatch, where §2.2 (amended) says
// ══════════════════════════════════════════════════════════════════════

/// **The first same-curve suite mismatch: it passes both pre-DH gates, dies
/// at the first AEAD open, and installs nothing.**
///
/// An `OfferedAes` initiator dials a `ReferenceSuite` responder's real
/// address with the responder's real static, over one shared in-memory
/// fabric. The four assertions below are taken **in order**, because the
/// ordering is the ruling:
///
/// 1. **It passes the length gate and mac1.** Observable as: the responder
///    mints an `Intro` — it reaches §6.3's staged queue. The initiation on
///    the wire is measured to be exactly 196 bytes, which is why there is no
///    length to reject it by, and §4.1's mac1 key preimage carries no suite,
///    which is why the keyed hash verifies.
/// 2. **The probe fails.** `read_identity()` answers
///    [`IntroError::Malformed`]: msg1's first AEAD open runs under a
///    handshake hash seeded with `Noise_IK_P256_AESGCM_BLAKE2b` on one side
///    and `Noise_IK_P256_ChaChaPoly_BLAKE2b` on the other, under a different
///    AEAD, and the tag does not verify. §6.1's `Malformed` is exactly this
///    verdict: *"the peer's bytes are at fault and the verdict is
///    definitive"*.
/// 3. **Nothing installs.** Across the full `HANDSHAKE_GIVEUP` window the
///    responder transmits **not one datagram** — no msg2, no reject, no
///    anything — its established set stays empty, and the initiator gives up
///    with [`ConnectError::TimedOut`]. §2.2: *"Either way nothing installs."*
/// 4. **The cost is priced.** §6.9's two mac1-valid rows, as an equality:
///    **0 DH** before the probe, and **exactly 1** after it — and still
///    exactly 1 at the end of the run, after the initiator's whole
///    retransmit ladder has arrived and been left unprobed. One probe, one
///    `es`; sixteen further mac1-valid arrivals, zero.
///
/// # The broken builds this separates, and which assertion catches each
///
/// | Broken build | Caught by |
/// |---|---|
/// | **§2.2 as it read *before* ruling 279** — "a mismatched-suite packet dies at mac1, before any DH", implemented literally (mac1 keyed on the protocol name, or a suite tag consulted at the gate) | assertion 1: no `Intro` is ever minted, and `accept()` stays pending for the whole 90 s |
/// | **A length gate that is suite-aware**, or an offered suite whose sizes drifted | assertion 1, plus the 196-byte measurement that says *why* it passed |
/// | **A mismatch that is not fail-closed** — an AEAD binding loose enough to open a sibling suite's msg1 | assertion 2: `read_identity()` would answer `Ok` |
/// | **A responder that answers** an initiation it could not open — an amplification vector (§6.9), and the thing §2.2's *"dies silently"* forbids | assertion 3's zero-datagram count |
/// | **Something installed anyway** — a half-open entry, a session recorded against the claimed static | assertion 3's `AlreadyConnected` probe, which ruling 87 makes a synchronous read of the established set |
/// | **`Malformed` retaining the chain** rather than discarding it (§6.1 frees the slot; retaining it hands an attacker a per-source slot for unreadable bytes) | assertion 4's final equality would climb, because a retained chain is re-probed |
/// | **An eager read on this path** — 1 DH per *arriving* packet rather than per *probe* | assertion 4's final equality: 17 arrivals would price at 17, not 1 |
/// | **The vacuous run** — a fixture where nothing arrived at all, which satisfies every negative assertion here for free (working rule 9) | the 15..=20 initiation count, taken from the tap |
///
/// # Why the assertions are equalities and not bounds
///
/// `b.dhs <= 1` is satisfied by a build that spends nothing because nothing
/// arrived, and `b.dhs >= 1` by one that spends a DH per packet. §6.9 prices
/// *"application probes identity then drops"* at exactly one, so exactly one
/// is what separates the correct build from both neighbours.
#[tokio::test(start_paused = true)]
async fn s22_an_aes_initiation_reaches_a_chacha_responders_queue_and_dies_at_the_probe() {
    local(async {
        let net = Network::seeded(0x5322_C0DE);
        let tap = net.tap();
        // Two endpoints, two suites, one fabric. Both statics are P-256
        // public keys — the same concrete type — which is the type-level
        // shadow of the same-curve property the whole test is about.
        let aes: Node<OfferedAes> = Node::spawn(&net, 0xA5, addr(1, 4001));
        let chacha: Node<ReferenceSuite> = Node::spawn(&net, 0xCC, addr(2, 4002));

        // The dial is genuine in every respect except the suite: the real
        // address, the real static, no fault injected anywhere.
        let dialling = aes
            .ep
            .connect(chacha.addr, chacha.pk)
            .expect("connect() on a NONE static is Ok");
        settle().await;

        // ── 1. it passed the length gate and mac1 ────────────────────────
        let intro = tokio::time::timeout(PATIENCE, chacha.ep.accept())
            .await
            .expect(
                "§2.2 (amended, ruling 279): a same-curve sibling suite's initiation \
                 MUST reach the staged queue — the lengths coincide and §4.1's mac1 key \
                 preimage carries no suite, so both pre-DH gates pass. No `Intro` \
                 surfaced, which is the pre-amendment behaviour: `dies at mac1`, stated \
                 for every suite pair and true only when the curves differ",
            )
            .expect("§16.2: accept() yields None only when the endpoint is closed");
        assert_eq!(
            intro.source(),
            aes.addr,
            "the introduction handed over is not the one the AES initiator sent"
        );

        // …and *why* it passed the length gate, measured rather than argued.
        let inits = packets_of(&tap, PKT_HANDSHAKE_INIT);
        assert!(
            !inits.is_empty(),
            "fixture: no HandshakeInit on the fabric at all"
        );
        for (n, init) in inits.iter().enumerate() {
            assert_eq!(
                init.bytes.len(),
                INIT_PACKET_LEN,
                "§2.3/ruling 279: AES-suite initiation {n} measured {} bytes against the \
                 reference suite's 196. §2.2's amended claim rests on the lengths \
                 coinciding — if they do not, this packet dies at the length gate and \
                 the whole mechanism under test is a different one",
                init.bytes.len()
            );
        }

        // ── 4a. §6.9: `Intro` left unprobed is 0 DH ──────────────────────
        assert_eq!(
            chacha.dhs.get(),
            0,
            "§6.9: `mac1-valid, src ∉ hint set, Intro left or dropped unprobed` is \
             **0 DH**. A curve operation spent before the application asked anything is \
             an eager read on the staged path"
        );

        // ── 2. the probe dies at msg1's first AEAD open ──────────────────
        let verdict = tokio::time::timeout(PATIENCE, intro.read_identity())
            .await
            .expect("read_identity resolved");
        match verdict {
            Err(IntroError::Malformed) => {}
            Err(IntroError::Local) => panic!(
                "§18.1/ruling 72: `Local` means **our own** provider failed — 0 DH spent, \
                 the chain left parked for a retry. This failure is the *peer's* bytes: a \
                 sibling suite's msg1 whose AEAD tag cannot verify under our handshake \
                 hash. Reporting it as `Local` would leave the chain occupying a stage-0 \
                 slot and invite a retry that can never succeed"
            ),
            Err(other) => panic!(
                "§2.2 (amended): a same-curve sibling's msg1 dies at the FIRST AEAD OPEN, \
                 which §6.1 reports as `Malformed` — the peer's bytes are at fault and the \
                 verdict is definitive. It answered {other:?}"
            ),
            Ok(_) => panic!(
                "S22/§2.2: mismatched suites must fail CLOSED. A ChaChaPoly responder read \
                 an identity out of an AES-GCM initiation, which means the AEAD open is \
                 not bound to the suite's protocol name (§2.2 seeds the handshake hash with \
                 `Noise_IK_P256_AESGCM_BLAKE2b` on one side and \
                 `Noise_IK_P256_ChaChaPoly_BLAKE2b` on the other) — there is no suite byte \
                 on the wire and no negotiation, so this binding is the only thing keeping \
                 two suites apart"
            ),
        }

        // ── 4b. §6.9: the probe costs exactly its `es` ───────────────────
        assert_eq!(
            chacha.dhs.get(),
            1,
            "§6.9: `mac1-valid, src ∉ hint set, application probes identity then drops` \
             is **1 DH** — an application-chosen spend. The `es` is paid before the AEAD \
             open that then fails, so the correct figure is exactly one: zero would mean \
             the open happened without the key it needs, and more would mean the staged \
             ladder ran past its first rung on bytes it could not read"
        );

        // ── 3. nothing installs, across the whole give-up window ─────────
        //
        // Letting the dial run to `HANDSHAKE_GIVEUP` is not padding: it puts
        // the initiator's entire retransmit ladder — a fresh mac1-valid,
        // 196-byte, unopenable initiation every ~5 s — through the
        // responder, so every assertion below is about a sustained mismatch
        // rather than about one packet.
        let outcome = tokio::time::timeout(GIVEUP + SHELL_LATENESS_BOUND, dialling)
            .await
            .expect(
                "§5.5 step 6: the initiator's dial had not resolved by HANDSHAKE_GIVEUP \
                 (90 s) + SHELL_LATENESS_BOUND",
            );
        assert!(
            matches!(outcome, Err(ConnectError::TimedOut)),
            "§5.5 step 6 / §2.2: a mismatched-suite dial installs nothing on the \
             initiator either — it gives up with `TimedOut`, because a responder that \
             cannot open msg1 never answers and there is no error to carry back. \
             Got {outcome:?}"
        );

        let ladder = packets_of(&tap, PKT_HANDSHAKE_INIT).len();
        assert!(
            (15..=20).contains(&ladder),
            "fixture: §5.5 step 2's 5 s + jitter train puts 15-20 initiations on the wire \
             across HANDSHAKE_GIVEUP. Observed {ladder}. Every negative assertion around \
             this one is satisfied for free by a run in which nothing arrived (working \
             rule 9), so this is the assertion that makes them mean something"
        );

        let responder_sent = sent_by(&tap, chacha.addr);
        assert!(
            responder_sent.is_empty(),
            "§2.2: a mismatched-suite packet dies **silently**. The responder emitted \
             {} datagram(s) in reply to {ladder} initiations it could not open — \
             §6.9's amplification accounting requires that to be zero, and a mismatched \
             deployment that answers is a reflector: {:?}",
            responder_sent.len(),
            responder_sent
                .iter()
                .map(|s| (s.dst, s.bytes.first().copied(), s.bytes.len()))
                .collect::<Vec<_>>()
        );

        // ── 4c. the equality holds over the whole run, not just the probe ─
        assert_eq!(
            chacha.dhs.get(),
            1,
            "§6.9: **one probe, one `es`**. {ladder} mac1-valid initiations arrived and \
             exactly one was probed, so the responder's cumulative spend must still be 1. \
             A larger figure prices the spend per *arriving packet* rather than per \
             *application probe*, which is the eager path (§6.5 step 3) applied where no \
             address is hinted — and it turns a mismatched peer into a CPU attack"
        );

        // ── 3b. and the established set is empty ─────────────────────────
        //
        // Ruling 87: `AlreadyConnected` is answered synchronously, before any
        // await, so this reads the responder's established set directly.
        // `Ok` means it holds no connection for the AES initiator's static.
        // Taken last, because it mints a dial of the responder's own and
        // every measurement above is about a responder that has sent nothing.
        let probe = chacha.ep.connect(aes.addr, aes.pk);
        assert!(
            probe.is_ok(),
            "§2.2: `Either way nothing installs.` The responder holds a connection for \
             a peer whose msg1 it could not open — got {:?}",
            probe.err()
        );
        drop(probe);
    })
    .await;
}

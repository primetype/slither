//! **S22, clause 4 — a wrong static against a *live* responder fails the
//! handshake and installs nothing.**
//!
//! `STORIES.md` §E, S22 (lines 351-357):
//!
//! > ### S22 — a user can pick a crypto suite, and mismatches fail closed
//! >
//! > - **Accepts:** the suite is declared once via the macro; a peer on a
//! >   different suite, **or a wrong static, fails the handshake and
//! >   installs nothing**. An unknown version byte is dropped silently —
//! >   there is no negotiation, ever.
//! > - **Anchor:** §1.1, §2, §3.1. **Paused clock:** yes.
//!
//! # Why this file exists
//!
//! Slice 1 handed clause 4 forward and said so twice, in the two places
//! that record it:
//!
//! * `tests/spec_packet.rs:30-32` — *"The fourth clause — 'a wrong static …
//!   fails the handshake and installs nothing' — needs a driven handshake
//!   and is slice 2's (PLAN.md §9.2)."*
//! * `.slices/01-packets/PLAN.md:1505-1507` — *"S22's fourth clause … and
//!   S22's **paused-clock obligation** are slice 2's. Slice 1 discharged
//!   neither and did not pretend to."*
//!
//! Slice 2 never received it. Every mismatch test that exists points at a
//! **ghost**: `tests/story_dial.rs:182` and `:256` dial `absent_static(2)`
//! at an address where **no endpoint is bound at all**, so they pin the
//! 90 s give-up against silence — the peer that never answers — and say
//! nothing about fail-closed behaviour when somebody *is* listening.
//! `src/core/endpoint/tests.rs`'s
//! `an_init_mac1ed_for_another_static_never_reaches_the_queue` pins the
//! core unit; what was missing is the **driven, two-endpoint, paused-clock
//! story** S22's own accept clause demands.
//!
//! # The mechanism, and what is therefore observable
//!
//! §4.1: `key = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)`, and
//! *"on a HandshakeInit the recipient is the responder"*. A dialler holding
//! the wrong static keys mac1 on that wrong static, so the responder's
//! verification against **its own** key fails.
//!
//! §4.2: mac1 is verified *"**before any curve or DH work**. A garbage
//! flood, a **wrong-key packet**, or a mismatched-suite packet dies at one
//! keyed hash and never reaches the DH provider."*
//!
//! *(Quoted as ratified. Ruling 279 later re-scoped the mismatched-suite
//! clause — a same-curve sibling suite is mac1-valid — leaving the
//! wrong-key case this file pins unchanged.)*
//!
//! §6.1's stage table: *"bad mac1 — all silent, **before the queue**"*.
//!
//! So the responder's side of "installs nothing" is total and *negative*:
//! no `Intro` is minted, no byte is transmitted, no DH is spent, no
//! connection is installed. The dialler's side is §5.5 step 6's give-up:
//! `ConnectError::TimedOut` at `HANDSHAKE_GIVEUP`. See
//! `GAPSLICE-B-REPORT.md` §5 for the wording-versus-mechanism note this
//! raises — there is no *distinguishable* "wrong static" error anywhere,
//! by design, and `ConnectError` has exactly three variants (ruling 72).
//!
//! # Not a sleep, ever (§16.10)
//!
//! Every test is `#[tokio::test(start_paused = true)]` on a `LocalSet`. The
//! 90 s give-up and the 5 s retransmit train resolve in virtual time.
//! `tokio::time::timeout` is the *observation* instrument: on the paused
//! clock it auto-advances to the next armed timer, so
//! `timeout(d, &mut fut).await.is_err()` is the precise statement that
//! `fut` was still `Pending` at `now + d`.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written from `STORIES.md` S22, `SPEC.md` §4.1/§4.2/§6.1/§5.5 and
//! `src/error.rs`'s `ConnectError` alone. The fixture below is the
//! established two-endpoint shape already used by `tests/story_dial.rs` and
//! `tests/story_lifecycle.rs`; every assertion is on protocol behaviour.
//!
//! # Ratified values are written as literals (working rule 9)
//!
//! `HANDSHAKE_GIVEUP`, `RETRANSMIT_BASE`, `SHELL_LATENESS_BOUND`,
//! `INIT_PACKET_LEN` and `PKT_HANDSHAKE_INIT` are spelled here as their
//! ratified **values**, not imported from `slither::constants`. A test
//! written in terms of the constant drifts with the constant and asserts
//! nothing about it; written as a literal, a drift turns this file red —
//! which, per CLAUDE.md, is a ruling request rather than an expectation to
//! update.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::time::Duration;

use slither::config::Config;
use slither::error::ConnectError;
use slither::identity::{Identity, PublicKeyOf};
use slither::testutil::{CountingIdentity, DhCounter, Network, Tap};

// ── ratified values, as literals ──────────────────────────────────────

/// §5.5 step 6 / Named constants: `HANDSHAKE_GIVEUP` = 90 s.
const GIVEUP: Duration = Duration::from_secs(90);

/// §5.5 step 2 / Named constants: `RETRANSMIT_BASE` = 5 s.
const RETRANSMIT_BASE: Duration = Duration::from_secs(5);

/// §16.5 / Named constants: `SHELL_LATENESS_BOUND` = 250 ms — the shell's
/// permitted lateness against a core deadline. Anything past it is a
/// missing or misarmed timer, not scheduling jitter.
const SHELL_LATENESS_BOUND: Duration = Duration::from_millis(250);

/// §3.1 / CLAUDE.md's wire pins: `INIT_PACKET_LEN` = 196.
const INIT_PACKET_LEN: usize = 196;

/// §3.1: `PKT_HANDSHAKE_INIT` = `0x01`.
const PKT_HANDSHAKE_INIT: u8 = 0x01;

// ── fixture ───────────────────────────────────────────────────────────

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::shell::Endpoint<Id>;
type Connection = slither::shell::Connection<Suite>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

struct Node {
    ep: Endpoint,
    /// §6.1's cumulative DH ladder, endpoint-wide.
    dhs: DhCounter,
    pk: Pk,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet`: §16.3 spawns the driver with
    /// `tokio::task::spawn_local`.
    fn spawn(net: &Network, key_seed: u8, port: u16) -> Node {
        let a = addr(port);
        let id: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = id.counter();
        let pk = *id.public_static();
        let ep = Endpoint::builder()
            .identity(id)
            .wire(net.wire(a))
            .config(Config::new())
            .build();
        Node {
            ep,
            dhs,
            pk,
            addr: a,
        }
    }

    fn ep(&self) -> &Endpoint {
        &self.ep
    }
}

/// A static key that belongs to **no bound endpoint in this fixture** — the
/// "wrong static". It is a perfectly well-formed public key of the
/// reference suite, which is the point: the packet is structurally valid
/// and dies only because §4.1 keys mac1 on the *recipient's* static.
fn other_static(key_seed: u8) -> Pk {
    let id: Id = CountingIdentity::seeded([key_seed; 32]);
    *id.public_static()
}

/// Count of `HandshakeInit` datagrams the fabric carried **toward** `to`.
///
/// §3.1: byte 0 is the packet type and the length gate is **exact** for the
/// handshake types — both are checked so a Data packet that happens to open
/// with `0x01` cannot be miscounted.
fn msg1_to(tap: &Tap, to: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(_from, dst, bytes)| {
            *dst == to && bytes.len() == INIT_PACKET_LEN && bytes[0] == PKT_HANDSHAKE_INIT
        })
        .count()
}

/// Count of `HandshakeInit` datagrams the fabric carried **from** `from`.
fn msg1_from(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, bytes)| {
            *src == from && bytes.len() == INIT_PACKET_LEN && bytes[0] == PKT_HANDSHAKE_INIT
        })
        .count()
}

/// Total datagrams the fabric carried *from* `from`, of any type. The claim
/// "the responder transmitted nothing" is about **all** output, not only
/// handshakes.
fn sent_count(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, _bytes)| *src == from)
        .count()
}

/// `dialler` connects to `listener` with the listener's **correct** static
/// and the listener walks §6.2's staged chain to completion.
///
/// The two halves are `join!`ed because the responder cannot produce msg2
/// until the application drives the chain and the initiator's `Connecting`
/// cannot resolve until msg2 arrives.
async fn establish(dialler: &Node, listener: &Node) -> (Connection, Connection) {
    let dial = dialler
        .ep()
        .connect(listener.addr, listener.pk)
        .expect("connect() on a NONE static must be Ok");

    let accept = async {
        let intro = listener
            .ep()
            .accept()
            .await
            .expect("§16.2: accept() yields None only when the endpoint is closed");
        let claimed = intro
            .read_identity()
            .await
            .expect("§6.1: read_identity() on a genuine msg1 must not be Malformed");
        let proven = claimed.authenticate().await.expect(
            "§6.1/§17.1: authenticate() must succeed. An `IntroError::Replay` HERE is the \
             failure this test exists to catch: it would mean the mac1-invalid initiations \
             dropped earlier had written the §17.1 timestamp guard, which §6.1 places two \
             stages further down the ladder (`authenticate() -> Proven`) and which a packet \
             that dies at the mac1 gate can never reach",
        );
        proven.accept().await.expect("accept")
    };

    let (initiator, responder) = tokio::join!(dial, accept);
    (
        initiator.expect("Connecting resolved with an error"),
        responder,
    )
}

// ────────────────────────────── tests ──────────────────────────────────

/// **S22 clause 4, the responder half and the dialler half in one run.**
///
/// A dials B's *live* address with a static that is not B's. Over the whole
/// `HANDSHAKE_GIVEUP` window:
///
/// * B mints **no** `Intro` — `accept()` is polled continuously for the
///   entire 90 s and must never become ready;
/// * B transmits **nothing** — not a msg2, not a reject, not anything;
/// * B spends **0 DH** (§4.2: mac1 precedes any curve work);
/// * B's established set is **empty** — `connect()` back to A answers `Ok`,
///   not `AlreadyConnected` (ruling 87: that answer is synchronous, before
///   any await, so it reads the set directly);
/// * A resolves `Err(ConnectError::TimedOut)` at `HANDSHAKE_GIVEUP`, **not
///   before and not never**.
///
/// The `select!` is what makes "for the full retransmit ladder" a real
/// claim rather than a claim about the first packet: `accept()` is one
/// future held across both halves, polled the whole time, and it wins the
/// select the instant an `Intro` appears.
///
/// # The broken builds this catches, and which assertion catches each
///
/// | Broken build | Caught by |
/// |---|---|
/// | **No mac1 gate on the inbound init** (`src/core/endpoint/mod.rs`'s `our_mac1.verify` removed) — the wrong-static init parks and surfaces | the `accept()` branch of the first `select!` panics |
/// | **A gate that answers** — an endpoint that replies to an unverifiable initiation (an amplification vector, §6.9) | `sent_count(b) == 0` |
/// | **A gate after the DH** — mac1 checked *after* `read_identity`, so §4.2's ordering is a fiction | `b.dhs == 0` |
/// | **A gate that installs** — anything recorded against A's static on a dropped packet | the `AlreadyConnected` probe |
/// | **Give-up too early** — `TimedOut` at the first unanswered retransmit, or at any deadline that is not 90 s | the "not before" half |
/// | **Give-up never armed** | the "not never" half |
/// | **A one-packet fixture** — a train that stopped after one init would make every negative assertion above vacuous | the 15..=20 `msg1_to` count |
///
/// The last row is working rule 9 applied to this test's own fixture: five
/// of the six assertions are *negative*, and a negative assertion is
/// satisfied for free if nothing ever happened. The initiation count is
/// what proves something did.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s22_a_wrong_static_against_a_live_responder_installs_nothing() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let tap = net.tap();
            let a = Node::spawn(&net, 1, 7001);
            let b = Node::spawn(&net, 2, 7002);

            // A well-formed static of the reference suite that is *not* the
            // responder's. §4.1 keys mac1 on the recipient's static, so this
            // one byte-string is the entire fault being injected.
            let wrong = other_static(3);
            assert_ne!(
                wrong.as_ref(),
                b.pk.as_ref(),
                "fixture: the dialled static must NOT be the responder's, or this test \
                 asserts nothing at all"
            );

            let dial = a.ep().connect(b.addr, wrong).expect(
                "connect() on a NONE static must be Ok — the fault is the peer's key, and \
                 §16.1 knows nothing about it yet",
            );
            let mut dial = pin!(dial);

            // ONE accept future, held across both halves below: it is polled
            // continuously from t=0 to t=HANDSHAKE_GIVEUP+250 ms, so "no
            // Intro was ever minted" is a statement about the whole ladder.
            let mut accept = pin!(b.ep().accept());

            // ── not before ─────────────────────────────────────────────
            let early = tokio::select! {
                r = tokio::time::timeout(GIVEUP - Duration::from_millis(1), dial.as_mut()) => r,
                _ = accept.as_mut() => panic!(
                    "§4.2/§6.1: a mac1-invalid initiation minted an `Intro`. mac1 keys on the \
                     RECIPIENT's static (§4.1) and a bad mac1 is a silent drop BEFORE the \
                     stage-0 queue"
                ),
            };
            assert!(
                early.is_err(),
                "§5.5 step 6: `Connecting` resolved BEFORE HANDSHAKE_GIVEUP (90 s)"
            );

            // ── not never ──────────────────────────────────────────────
            let late = tokio::select! {
                r = tokio::time::timeout(
                    Duration::from_millis(1) + SHELL_LATENESS_BOUND,
                    dial.as_mut(),
                ) => r,
                _ = accept.as_mut() => panic!(
                    "§4.2/§6.1: a mac1-invalid initiation minted an `Intro` late in the ladder \
                     — the gate held for the first packets and not for the rest"
                ),
            };
            let outcome = late
                .expect("`Connecting` had not resolved by HANDSHAKE_GIVEUP + SHELL_LATENESS_BOUND");
            assert!(
                matches!(outcome, Err(ConnectError::TimedOut)),
                "§5.5 step 6 / §18.1: the give-up is `ConnectError::TimedOut` — and there is \
                 no wrong-static variant to report instead (ruling 72 fixes ConnectError at \
                 three). Got {outcome:?}",
            );

            // ── the ladder actually ran ────────────────────────────────
            //
            // Without this, every negative assertion below is satisfied by a
            // build that emitted one packet and gave up (working rule 9).
            let n = msg1_to(&tap, b.addr);
            assert!(
                (15..=20).contains(&n),
                "§5.5 step 2: a fixed {RETRANSMIT_BASE:?} + jitter train puts 15-20 \
                 initiations on the wire across HANDSHAKE_GIVEUP; the responder must have \
                 dropped ALL of them, not one. Observed {n}"
            );

            // ── the responder minted nothing, said nothing, spent nothing ─
            assert_eq!(
                sent_count(&tap, b.addr),
                0,
                "§4.2/§6.1: a bad mac1 is a SILENT drop. The responder transmitted {} \
                 datagram(s) in reply to {n} unverifiable initiations — §6.9's amplification \
                 accounting rests on that being zero",
                sent_count(&tap, b.addr)
            );
            assert_eq!(
                b.dhs.get(),
                0,
                "§4.2: mac1 is verified BEFORE any curve or DH work — a wrong-key packet \
                 'dies at one keyed hash and never reaches the DH provider'"
            );

            // ── and installed nothing ──────────────────────────────────
            //
            // Ruling 87: `AlreadyConnected` is returned synchronously,
            // before any await, so this reads the established set directly
            // rather than through a round trip. `Ok` means B holds no
            // connection for A's static.
            let probe = b.ep().connect(a.addr, a.pk);
            assert!(
                probe.is_ok(),
                "§5.4/§18.1: the responder installed something for the dialler's static on a \
                 handshake that never authenticated — got {:?}",
                probe.err()
            );
            drop(probe);
        })
        .await;
}

/// **S22 clause 4, the recovery half — a wrong-static dial is not a
/// blackhole in either direction.**
///
/// The same pair, in the same run: A hammers B for 90 s with a wrong static
/// and gives up, and then A dials B **correctly** and the connection is
/// established and carries traffic both ways.
///
/// This is also where the brief's timestamp-guard question is answered by
/// assertion rather than by argument. §6.1 places the initiation timestamp
/// at `authenticate() -> Proven` and names `Replay` as that stage's
/// rejection; a wrong-static init dies two stages earlier, at mac1, so it
/// can never reach the guard. `establish`'s `authenticate()` expectation
/// spells that out: an `IntroError::Replay` there is exactly the build in
/// which the dropped initiations wrote §17.1's guard anyway.
///
/// # The broken builds this catches
///
/// | Broken build | Why the first test misses it |
/// |---|---|
/// | **A gate that rejects everything** (`verify` → always false, or keyed on the wrong static) | the first test's assertions are all *negative* — a totally deaf responder passes every one of them. This is the degenerate build working rule 9 demands a bound against, and it is the reason this test is not redundant. |
/// | **A gate that latches** — the first mac1 failure poisons the endpoint, the source address, or the peer's stage-0 slot, so nothing from A is ever accepted again | the first test never asks B to accept anything |
/// | **A guard written on arrival** — §17.1's timestamp recorded before authentication, so a later genuine initiation reads as a replay | `authenticate()` would answer `IntroError::Replay` |
/// | **A dialler that leaks the given-up static** so the pair cannot be redialled | `connect()` would answer `AlreadyConnected` |
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s22_both_endpoints_stay_usable_after_a_wrong_static_dial() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let tap = net.tap();
            let a = Node::spawn(&net, 1, 7011);
            let b = Node::spawn(&net, 2, 7012);
            let wrong = other_static(3);

            // ── the wrong-static dial, run to its give-up ──────────────
            let dial = a.ep().connect(b.addr, wrong).expect("first dial");
            assert!(
                matches!(dial.await, Err(ConnectError::TimedOut)),
                "§5.5 step 6: the wrong-static dial must give up with TimedOut"
            );
            let dropped = msg1_to(&tap, b.addr);
            assert!(
                dropped >= 15,
                "fixture: the responder must have dropped a full ladder before the recovery \
                 is interesting; observed {dropped}"
            );
            assert_eq!(
                sent_count(&tap, b.addr),
                0,
                "§4.2: the responder answered an unverifiable initiation"
            );

            // ── and now the correct one, same pair, no clock games ─────
            let (ca, cb) = establish(&a, &b).await;

            assert_eq!(
                ca.remote_static().as_ref(),
                b.pk.as_ref(),
                "§16.2: remote_static() is the peer we dialled"
            );
            assert_eq!(
                cb.remote_static().as_ref(),
                a.pk.as_ref(),
                "§16.2: the responder's peer is the dialler"
            );

            // Traffic both ways: "fully usable", not merely "established".
            ca.send_message(b"after the wrong static")
                .await
                .expect("§11: the initiator must be able to send");
            assert_eq!(
                cb.recv_message()
                    .await
                    .expect("§11: the responder must be able to receive"),
                b"after the wrong static".to_vec()
            );
            cb.send_message(b"and back again")
                .await
                .expect("§11: the responder must be able to send");
            assert_eq!(
                ca.recv_message()
                    .await
                    .expect("§11: the initiator must be able to receive"),
                b"and back again".to_vec()
            );
        })
        .await;
}

/// **S22 clause 4 on §6.5's *hinted* path — the one place where a
/// post-mac1 gate would cost the responder DH.**
///
/// §6.5 step 2 checks whether the source address is in §17.4's hint set
/// (the addresses this endpoint has a dial in flight to). A hinted source
/// takes step 3's **eager** path, which spends 1 DH (`es`) *before* anyone
/// knows whose static the initiation claims. §4.2's ordering claim — mac1
/// "before any curve or DH work" — is therefore load-bearing exactly here:
/// on the unhinted path a missing gate costs 0 DH and only parks garbage,
/// but on the hinted path it costs one curve operation per garbage packet,
/// which is §6.9's DoS accounting inverted.
///
/// The fixture puts A's address into B's hint set by having B dial a static
/// nobody holds *at A's address*, while A dials B with a static that is not
/// B's. Both trains die at the other end's mac1 gate; neither endpoint ever
/// sees an `Intro`.
///
/// # The bound, and why it is an equality
///
/// B's only legitimate DH spend is its own initiator ladder: §6.1 prices an
/// initiation at `es + ss` = **2 DH**, and §5.5 step 2 makes every
/// retransmit a completely fresh initiation. So
///
/// ```text
/// b.dhs == 2 × (msg1 datagrams B put on the wire)
/// ```
///
/// is exact, and it is the assertion. `b.dhs <= something` would be the
/// vacuous form working rule 9 warns about; a build that dropped the gate
/// would land at `2 × sent + 1 × received`, and only an equality separates
/// the two.
///
/// # The broken builds this catches
///
/// | Broken build | Caught by |
/// |---|---|
/// | **mac1 checked after `route_initiation`** — §4.2's order reversed, so every garbage init from a hinted source buys an `es` | the DH equality: B would show ~17 extra |
/// | **No gate at all**, on the hinted path | same — and note the *first* test cannot see this one: with the gate gone and the source hinted, §6.5 step 3's eager read answers `Malformed` and drops the packet, so no `Intro` is ever minted and `accept()` still never fires |
/// | **A gate that is source-scoped rather than packet-scoped** | both `accept()` branches |
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s22_a_wrong_static_costs_the_responder_no_dh_on_the_hinted_path() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let tap = net.tap();
            let a = Node::spawn(&net, 1, 7021);
            let b = Node::spawn(&net, 2, 7022);

            // B dials *someone else* at A's address: this and only this is
            // what puts A's address into B's §17.4 hint set (§6.5 step 2).
            let b_dial = b
                .ep()
                .connect(a.addr, other_static(4))
                .expect("B's own dial");
            // A dials B with a static that is not B's.
            let a_dial = a
                .ep()
                .connect(b.addr, other_static(3))
                .expect("A's wrong-static dial");

            let mut b_accept = pin!(b.ep().accept());
            let mut a_accept = pin!(a.ep().accept());

            let (a_out, b_out) = tokio::select! {
                pair = async { tokio::join!(a_dial, b_dial) } => pair,
                _ = b_accept.as_mut() => panic!(
                    "§4.2/§6.5: a mac1-invalid initiation reached the responder's stage-0 \
                     queue on the HINTED path"
                ),
                _ = a_accept.as_mut() => panic!(
                    "§4.2/§6.5: a mac1-invalid initiation reached the dialler's stage-0 \
                     queue on the HINTED path"
                ),
            };

            assert!(
                matches!(a_out, Err(ConnectError::TimedOut)),
                "§5.5 step 6: A's wrong-static dial must give up with TimedOut; got {a_out:?}"
            );
            assert!(
                matches!(b_out, Err(ConnectError::TimedOut)),
                "§5.5 step 6: B's wrong-static dial must give up with TimedOut; got {b_out:?}"
            );

            let b_sent = msg1_from(&tap, b.addr);
            let a_sent = msg1_from(&tap, a.addr);
            assert!(
                (15..=20).contains(&b_sent) && (15..=20).contains(&a_sent),
                "fixture: both ladders must have run (A {a_sent}, B {b_sent}); a DH equality \
                 over an empty train asserts nothing"
            );
            assert_eq!(
                sent_count(&tap, b.addr),
                b_sent,
                "§4.2: every datagram B emitted must be one of its OWN initiations — a reply \
                 to A's unverifiable initiations is an amplification vector (§6.9)"
            );

            // The equality. Everything A sent to B was mac1-invalid at B, so
            // it must have cost B exactly nothing on top of B's own ladder.
            assert_eq!(
                b.dhs.get() as usize,
                2 * b_sent,
                "§4.2/§6.1: B's DH spend must be exactly its own initiator ladder \
                 (es+ss = 2 DH per initiation, {b_sent} initiations = {}). A larger figure \
                 means A's {a_sent} mac1-invalid initiations reached the DH provider through \
                 §6.5 step 3's eager path",
                2 * b_sent
            );
            assert_eq!(
                a.dhs.get() as usize,
                2 * a_sent,
                "§4.2/§6.1: and symmetrically for A, whose address is likewise hinted at B"
            );
        })
        .await;
}

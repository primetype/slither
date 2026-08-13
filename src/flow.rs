//! End-to-end flow tests (the brief's 14 scenarios).
//!
//! Most run on tokio's **paused clock** (`start_paused = true`) so the WireGuard
//! timers — retransmit at 5 s, give-up at 90 s, keepalive at 10 s, dead at 15 s,
//! rekey at 120 s, reject at 180 s — resolve in virtual time. Traffic rides the
//! in-memory [`FlakyWire`](crate::testutil::FlakyWire); one test uses a real UDP
//! loopback socket. Every test runs inside a [`LocalSet`], because the endpoint
//! actor is `!Send` and spawned with `spawn_local`.
//!
//! Brief-test coverage map:
//! 1  happy_path · 2 msg1_lost_twice_retransmits_fresh · 3 msg2_lost_reaccepted ·
//! 4 handshake_gives_up · 5 mac1_flood_never_reaches_dh · 6 initiation_replay ·
//! 7 data_through_reorder_and_dup_exactly_once · 8 roaming_follows_authenticated
//! · 9 keepalive_after_idle + dead_after_silence · 10 rekey_keeps_flow +
//! idle_survives_past_reject_age_then_a_payload_send_backstops +
//! retransmit_into_partition_dies_by_liveness +
//! asymmetric_loss_retransmit_gate_rekeys_then_backstops · 11 size_caps · 12
//! unknown_packets_dropped · 13 the `wire::tests`/`handshake::tests` layout
//! pins · 14 allow_list_rejects_unlisted, joined by
//! unlisted_initiator_costs_one_dh_and_no_resp (the Leg 1c one-DH policy-cost
//! pin, 2026/07/16).
//!
//! The 2026/07/17 ratchet ruling — *age gates payload, not liveness* — flipped
//! §10's old `refused_at_reject_age` (an idle session dying at 180 s) into
//! `idle_survives_past_reject_age_then_a_payload_send_backstops` (a zero-traffic
//! idle session outlives 180 s, and only a fresh payload send then hits the
//! backstop), and added the retransmit-path pair:
//! `retransmit_into_partition_dies_by_liveness` (symmetric loss — liveness
//! wins, deterministically) and
//! `asymmetric_loss_retransmit_gate_rekeys_then_backstops` (asymmetric loss —
//! the peer's control still arrives, liveness never fires, and the
//! retransmit-path age gate rekeys at 120 s then backstops at ~196 s;
//! reviewer-authored, Leg R1 review).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;

use hiss::curve::p256::P256r1PublicKey;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use tokio::task::LocalSet;
use tokio::time::{self, Instant};

use crate::endpoint::{Config, ConnId, Endpoint, Event, SessionHandle, SlitherError};
use crate::handshake::{self, Established, Identity, random_index};
use crate::session::{DEAD_TIMEOUT, REKEY_AGE, Session};
use crate::testutil::{CountingIdentity, FlakyPolicy, Network, software_identity};
use crate::wire::{DataHeader, INIT_PACKET_LEN, PacketKind, Timestamp, VERSION, classify};

// ── Harness ───────────────────────────────────────────────────────────────────

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

const ADDR_A: u16 = 1;
const ADDR_B: u16 = 2;

/// Two endpoints (A = initiator, B = responder) over a shared network.
struct Pair {
    net: Network,
    a: Endpoint,
    b: Endpoint,
    b_static: P256r1PublicKey,
}

/// Build a pair. `allow_a_on_b` puts A's static on B's inbound allow-list (the
/// usual case); `allow_b_on_a` is rarely needed (A dials, so its allow-list only
/// matters if B dials back).
fn setup(policy_a: FlakyPolicy, policy_b: FlakyPolicy, allow_a_on_b: bool) -> Pair {
    let net = Network::new();
    let id_a = software_identity(0x11, 0xA1);
    let id_b = software_identity(0x22, 0xB2);
    let a_static = id_a.static_public();
    let b_static = id_b.static_public();

    let wire_a = net.endpoint(addr(ADDR_A), policy_a, [0xA0; 32]);
    let wire_b = net.endpoint(addr(ADDR_B), policy_b, [0xB0; 32]);

    let cfg_a = Config::new().with_rng_seed([0x01; 32]).allow(&b_static);
    let mut cfg_b = Config::new().with_rng_seed([0x02; 32]);
    if allow_a_on_b {
        cfg_b = cfg_b.allow(&a_static);
    }

    let a = Endpoint::start(id_a, wire_a, cfg_a);
    let b = Endpoint::start(id_b, wire_b, cfg_b);
    Pair {
        net,
        a,
        b,
        b_static,
    }
}

/// Drive `a`'s connect to `b` to the point where both sides report `Established`,
/// returning (`a`'s handle, `a`'s conn, `b`'s conn).
async fn establish(
    a: &mut Endpoint,
    b: &mut Endpoint,
    b_static: P256r1PublicKey,
) -> (SessionHandle, ConnId, ConnId) {
    let handle = a.connect(addr(ADDR_B), b_static);
    let mut a_conn = None;
    let mut b_conn = None;
    while a_conn.is_none() || b_conn.is_none() {
        tokio::select! {
            event = a.next_event() => {
                if let Some(Event::Established { conn, .. }) = event {
                    a_conn = Some(conn);
                }
            }
            event = b.next_event() => {
                if let Some(Event::Established { conn, .. }) = event {
                    b_conn = Some(conn);
                }
            }
        }
    }
    (handle, a_conn.unwrap(), b_conn.unwrap())
}

/// Await the next `Incoming` payload on `ep` (ignoring other events).
async fn next_incoming(ep: &mut Endpoint) -> Vec<u8> {
    loop {
        match ep.next_event().await.expect("event") {
            Event::Incoming { payload, .. } => return payload,
            _ => continue,
        }
    }
}

async fn local<F: std::future::Future<Output = ()>>(body: F) {
    LocalSet::new().run_until(body).await;
}

// ── 1. Happy path ─────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn happy_path() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle_a, _a_conn, b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        handle_a.send(b"ping".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut pair.b).await, b"ping");

        let handle_b = pair.b.session(b_conn);
        handle_b.send(b"pong".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut pair.a).await, b"pong");
    })
    .await;
}

// ── 2. msg1 lost twice ⇒ retransmit with fresh ephemerals ─────────────────────

#[tokio::test(start_paused = true)]
async fn msg1_lost_twice_retransmits_fresh() {
    local(async {
        let mut pair = setup(FlakyPolicy::drop_first(2), FlakyPolicy::perfect(), true);
        let start = Instant::now();
        let (_h, _a, _b) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;
        let elapsed = start.elapsed();

        // ~two retransmit intervals (5 s each, + jitter ≤ 333 ms, + tick slack).
        assert!(
            elapsed >= Duration::from_secs(10) && elapsed <= Duration::from_millis(11_500),
            "established after {elapsed:?}, expected ~2 retransmit intervals"
        );

        // Each retransmit is a completely fresh initiation: the first three Init
        // packets A sent must differ pairwise (fresh ephemerals + index + ts).
        let inits: Vec<Vec<u8>> = pair
            .net
            .sends()
            .into_iter()
            .filter(|s| {
                s.src == addr(ADDR_A) && classify(&s.bytes) == Some(PacketKind::HandshakeInit)
            })
            .map(|s| s.bytes)
            .collect();
        assert!(
            inits.len() >= 3,
            "expected at least three initiations, got {}",
            inits.len()
        );
        assert_ne!(inits[0], inits[1], "retransmit 1 reused msg1");
        assert_ne!(inits[1], inits[2], "retransmit 2 reused msg1");
        assert_ne!(inits[0], inits[2]);
    })
    .await;
}

// ── 3. msg2 lost ⇒ a fresh msg1 is re-accepted (newer timestamp) ──────────────

#[tokio::test(start_paused = true)]
async fn msg2_lost_reaccepted() {
    local(async {
        // B drops its first send (the first HandshakeResp); A retransmits a fresh
        // msg1 (strictly-greater timestamp), which B admits and answers.
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::drop_first(1), true);
        let (_h, _a, _b) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;
        // Reaching Established at all proves the second msg1 was re-accepted.
    })
    .await;
}

// ── 4. 90 s of silence ⇒ connect gives up with a typed error ──────────────────

#[tokio::test(start_paused = true)]
async fn handshake_gives_up() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        pair.net.partition(addr(ADDR_B)); // nothing reaches B, nothing returns
        let _handle = pair.a.connect(addr(ADDR_B), pair.b_static);

        let start = Instant::now();
        let event = pair.a.next_event().await.expect("event");
        let elapsed = start.elapsed();

        match event {
            Event::Failed { error, .. } => assert_eq!(
                error,
                crate::endpoint::ConnectError::TimedOut(Duration::from_secs(90))
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(
            elapsed >= Duration::from_secs(90) && elapsed <= Duration::from_millis(90_500),
            "gave up after {elapsed:?}"
        );
    })
    .await;
}

// ── 5. mac1 flood never reaches the DH provider ───────────────────────────────

#[tokio::test(start_paused = true)]
async fn mac1_flood_never_reaches_dh() {
    local(async {
        let net = Network::new();
        let id_a = software_identity(0x11, 0xA1);
        let a_static = id_a.static_public();
        let counting = CountingIdentity::new(0x22, 0xB2).unwrap();
        let b_static = counting.static_public();
        let dh_count = counting.dh_count();

        let wire_a = net.endpoint(addr(ADDR_A), FlakyPolicy::perfect(), [0xA0; 32]);
        let wire_b = net.endpoint(addr(ADDR_B), FlakyPolicy::perfect(), [0xB0; 32]);
        let mut a = Endpoint::start(
            id_a,
            wire_a,
            Config::new().with_rng_seed([1; 32]).allow(&b_static),
        );
        let mut b = Endpoint::start(
            counting,
            wire_b,
            Config::new().with_rng_seed([2; 32]).allow(&a_static),
        );

        let (handle, _a_conn, b_conn) = establish(&mut a, &mut b, b_static).await;
        let after_handshake = dh_count.load(Ordering::SeqCst);
        assert!(
            after_handshake > 0,
            "the legitimate handshake did perform DH"
        );

        // Flood B with garbage and wrong-key initiations. None must reach the DH
        // provider — they die at classify or the mac1 gate.
        for seed in 0u8..40 {
            let mut junk = vec![seed; INIT_PACKET_LEN];
            junk[0] = 0x01; // HandshakeInit type
            junk[1] = VERSION;
            net.inject(addr(ADDR_B), addr(ADDR_A), junk);
        }
        // A well-formed Init whose mac1 is keyed on the WRONG recipient static.
        let mut wrong = software_identity(0x33, 0xC3);
        let stranger = software_identity(0x44, 0xD4).static_public(); // not B
        let (wrong_init, _pending) = handshake::build_init(
            wrong.provider(),
            wrong.static_secret(),
            stranger,
            0x1234,
            Timestamp { secs: 9, nanos: 9 },
        )
        .unwrap();
        for _ in 0..10 {
            net.inject(addr(ADDR_B), addr(ADDR_A), wrong_init.clone());
        }

        // Let the flood be processed, then confirm no additional DH and a still-live session.
        handle.send(b"still-alive".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut b).await, b"still-alive");
        assert_eq!(
            dh_count.load(Ordering::SeqCst),
            after_handshake,
            "a mac1 flood must not reach the DH provider"
        );
        let _ = b_conn;
    })
    .await;
}

// ── 6. Initiation replay ⇒ no second session, no second msg2 ──────────────────

#[tokio::test(start_paused = true)]
async fn initiation_replay() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (_h, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // The Init A actually sent, captured off the send log.
        let init = pair
            .net
            .sends()
            .into_iter()
            .find(|s| {
                s.src == addr(ADDR_A) && classify(&s.bytes) == Some(PacketKind::HandshakeInit)
            })
            .expect("an Init was sent")
            .bytes;

        let resps_before = count_resps(&pair.net);
        pair.net.inject(addr(ADDR_B), addr(ADDR_A), init); // replay it

        // Give B time to (not) react.
        time::sleep(Duration::from_secs(2)).await;
        assert_eq!(
            count_resps(&pair.net),
            resps_before,
            "a replayed initiation must not produce a second HandshakeResp"
        );
    })
    .await;
}

fn count_resps(net: &Network) -> usize {
    net.sends()
        .into_iter()
        .filter(|s| s.src == addr(ADDR_B) && classify(&s.bytes) == Some(PacketKind::HandshakeResp))
        .count()
}

// ── 7. Data through reorder + duplication ⇒ delivered exactly once ────────────

#[tokio::test(start_paused = true)]
async fn data_through_reorder_and_dup_exactly_once() {
    local(async {
        // Duplicate half the datagrams and jitter every delivery (reorder), with
        // NO loss — so every payload is delivered, each exactly once.
        let flaky = FlakyPolicy {
            loss: 0.0,
            duplicate: 0.5,
            base_delay: Duration::ZERO,
            jitter: Duration::from_millis(50),
            drop_first: 0,
        };
        let mut pair = setup(flaky, flaky, true);
        let (handle, _a, _b) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        const N: usize = 50;
        for i in 0..N {
            handle.send(format!("msg-{i}").into_bytes()).unwrap();
        }

        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        while seen.len() < N {
            let payload = next_incoming(&mut pair.b).await;
            assert!(
                seen.insert(payload.clone()),
                "payload delivered twice: {payload:?}"
            );
        }
        let expected: HashSet<Vec<u8>> = (0..N).map(|i| format!("msg-{i}").into_bytes()).collect();
        assert_eq!(seen, expected, "every payload delivered exactly once");
    })
    .await;
}

// ── 8. Roaming follows an authenticated source; forgery/replay do not ─────────

#[tokio::test(start_paused = true)]
async fn roaming_follows_authenticated() {
    local(async {
        let (mut a_session, mut b_session) = two_sessions(addr(ADDR_A), addr(ADDR_B)).await;
        let now = Instant::now();

        // A fresh authenticated packet from the CURRENT endpoint: no move.
        let p0 = a_session.seal(b"one", now).unwrap();
        let received = b_session.open(&p0, addr(ADDR_A), now).unwrap();
        assert_eq!(received.payload.as_deref(), Some(&b"one"[..]));
        assert_eq!(received.moved_from, None);
        assert_eq!(b_session.endpoint(), addr(ADDR_A));

        // A fresh authenticated packet from a NEW source: roam to it.
        let p1 = a_session.seal(b"two", now).unwrap();
        let received = b_session.open(&p1, addr(9), now).unwrap();
        assert_eq!(received.payload.as_deref(), Some(&b"two"[..]));
        assert_eq!(received.moved_from, Some(addr(ADDR_A)));
        assert_eq!(
            b_session.endpoint(),
            addr(9),
            "roamed to the authenticated source"
        );

        // A forged (garbage) packet from yet another source: no move.
        let mut forged = p1.clone();
        *forged.last_mut().unwrap() ^= 0xFF; // break the AEAD tag
        assert!(b_session.open(&forged, addr(13), now).is_err());
        assert_eq!(
            b_session.endpoint(),
            addr(9),
            "forgery must not move the endpoint"
        );

        // A REPLAY of an old authenticated packet from a new source: dropped by the
        // replay window, no move.
        let replay = b_session.open(&p0, addr(13), now).unwrap();
        assert_eq!(replay.payload, None, "replay suppressed");
        assert_eq!(replay.moved_from, None);
        assert_eq!(
            b_session.endpoint(),
            addr(9),
            "a replay must not move the endpoint"
        );
    })
    .await;
}

/// Establish two live [`Session`]s in-process via the handshake helpers.
async fn two_sessions(addr_a: SocketAddr, addr_b: SocketAddr) -> (Session, Session) {
    let mut a = software_identity(0x11, 0xA1);
    let mut b = software_identity(0x22, 0xB2);
    let mut rng = ChaCha20Rng::from_seed([0x33; 32]);
    let ts = Timestamp { secs: 1, nanos: 1 };
    let (init_packet, pending) = handshake::build_init(
        a.provider(),
        a.static_secret(),
        b.static_public(),
        random_index(&mut rng),
        ts,
    )
    .unwrap();
    let allow = HashSet::from([a.static_public().to_compressed()]);
    let accept = handshake::accept_init(
        b.provider(),
        &b.static_public(),
        b.static_secret(),
        &init_packet,
        &allow,
    )
    .unwrap();
    let (resp_packet, b_est): (Vec<u8>, Established) =
        accept.accept(random_index(&mut rng)).unwrap();
    let a_est = handshake::complete_init(pending, &a.static_public(), &resp_packet).unwrap();

    let now = Instant::now();
    (
        Session::new(a_est, addr_b, now),
        Session::new(b_est, addr_a, now),
    )
}

// ── 9. Keepalive after idle; Dead after silence ───────────────────────────────

#[tokio::test(start_paused = true)]
async fn keepalive_after_idle() {
    local(async {
        let net = Network::new();
        let mut tap = net.tap();
        let id_a = software_identity(0x11, 0xA1);
        let id_b = software_identity(0x22, 0xB2);
        let a_static = id_a.static_public();
        let b_static = id_b.static_public();
        let wire_a = net.endpoint(addr(ADDR_A), FlakyPolicy::perfect(), [0xA0; 32]);
        let wire_b = net.endpoint(addr(ADDR_B), FlakyPolicy::perfect(), [0xB0; 32]);
        let mut a = Endpoint::start(
            id_a,
            wire_a,
            Config::new().with_rng_seed([1; 32]).allow(&b_static),
        );
        let mut b = Endpoint::start(
            id_b,
            wire_b,
            Config::new().with_rng_seed([2; 32]).allow(&a_static),
        );

        let (_h, _a_conn, b_conn) = establish(&mut a, &mut b, b_static).await;

        // Advance the virtual clock off the establishment instant, then kick the
        // keepalive dance off: B sends A one data packet, so A is now
        // "received-but-not-sent" (last_recv strictly after last_send) and owes a
        // keepalive ~10 s later.
        time::sleep(Duration::from_secs(1)).await;
        b.session(b_conn).send(b"nudge".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut a).await, b"nudge");
        let start = Instant::now();

        // A sends an empty-plaintext Data (16-byte ciphertext) keepalive ~10 s on.
        let keepalive_at = loop {
            let spied = tap.recv().await.expect("tapped datagram");
            if spied.src == addr(ADDR_A)
                && classify(&spied.bytes) == Some(PacketKind::Data)
                && let Some((_header, ciphertext)) = DataHeader::parse(&spied.bytes)
                && ciphertext.len() == crate::wire::AEAD_TAG_LEN
            {
                break start.elapsed();
            }
        };
        assert!(
            keepalive_at >= Duration::from_secs(10)
                && keepalive_at <= Duration::from_millis(10_600),
            "keepalive at {keepalive_at:?}, expected ~10 s"
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn dead_after_silence() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle, a_conn, _b) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Advance the virtual clock off the establishment instant so the send
        // lands strictly after the last receive. Then cut the wire and send: the
        // send flips A into "sent-but-nothing-received"; 15 s later the session is
        // declared dead.
        time::sleep(Duration::from_secs(1)).await;
        pair.net.partition(addr(ADDR_B));
        let start = Instant::now();
        handle.send(b"into-the-void".to_vec()).unwrap();

        let event = loop {
            match pair.a.next_event().await.expect("event") {
                Event::Dead { conn, .. } if conn == a_conn => break Instant::now(),
                _ => continue,
            }
        };
        let elapsed = event.duration_since(start);
        assert!(
            elapsed >= Duration::from_secs(15) && elapsed <= Duration::from_millis(15_600),
            "declared dead after {elapsed:?}, expected ~15 s"
        );
    })
    .await;
}

// ── 10. Rekey keeps flow; an idle session lives on keepalives; a stuck one
//        dies by liveness (age gates payload, not liveness — 2026/07/17) ───────

#[tokio::test(start_paused = true)]
async fn rekey_keeps_flow() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Age the session past REKEY_AGE (the keepalive dance keeps it alive).
        time::sleep(Duration::from_secs(125)).await;

        // The next send triggers a fresh handshake (a silent swap under the same
        // conn); let it settle, then confirm payloads still flow across the swap.
        handle.send(b"pre-rekey".to_vec()).unwrap();
        time::sleep(Duration::from_secs(2)).await;
        handle.send(b"post-rekey".to_vec()).unwrap();

        let mut saw_post = false;
        for _ in 0..4 {
            let payload = next_incoming(&mut pair.b).await;
            if payload == b"post-rekey" {
                saw_post = true;
                break;
            }
        }
        assert!(saw_post, "payload flow did not continue across the rekey");
    })
    .await;
}

/// Age gates payload, not liveness (the maintainer, 2026/07/17): the idle
/// age-death is gone. A **zero-traffic** idle session is the purest probe of
/// that removal — with no send, `is_dead` can never fire, so the age backstop
/// on the tick was the session's only possible killer; surviving well past the
/// old 180 s REJECT_AGE proves it is gone. The same test then pins the flip
/// side of the ruling: the backstop still guards the PAYLOAD path — a fresh
/// send on the now-over-age (never-rekeyed) session is torn down there.
///
/// (An idle session sustained by the keepalive *dance* survives just as well —
/// the dance holds `is_dead` off and never rekeys. The zero-traffic probe is
/// used here because it is the PUREST isolation of the removed age-death; the
/// dancing-past-180 s shape is pinned end-to-end by the engine's LAN E2E, and
/// the dance mechanics by `keepalive_after_idle`.)
#[tokio::test(start_paused = true)]
async fn idle_survives_past_reject_age_then_a_payload_send_backstops() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle, a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // One full handshake per side so far; a rekey during the idle window
        // would mint a fresh HandshakeInit — there must be none.
        let inits = |net: &Network| {
            net.sends()
                .into_iter()
                .filter(|s| classify(&s.bytes) == Some(PacketKind::HandshakeInit))
                .count()
        };
        let inits_before = inits(&pair.net);

        // Idle five virtual minutes — comfortably past the old 180 s REJECT_AGE
        // idle death — with zero traffic. Under the old rule the tick tore the
        // session down at 180 s; under the new rule nothing does. With zero
        // traffic both liveness clocks stay at their `Session::new` stamps and
        // `is_dead` needs a STRICT `last_send > last_recv`, so it can never
        // fire — isolating the removed tick age-death as the only thing that
        // could have killed the session.
        time::sleep(Duration::from_secs(300)).await;

        // Survived: no `Dead` for A queued across the window (the age-death is
        // gone), and no fresh handshake fired (zero-traffic cannot rekey).
        let mut idle_dead = false;
        while let Ok(event) = tokio::time::timeout(Duration::ZERO, pair.a.next_event()).await {
            if let Some(Event::Dead { conn, .. }) = event
                && conn == a_conn
            {
                idle_dead = true;
            }
        }
        assert!(!idle_dead, "an idle session must not die of key age");
        assert_eq!(
            inits(&pair.net),
            inits_before,
            "no re-handshake during the idle window — zero rekey",
        );

        // The flip side of the rule: the REJECT_AGE backstop still guards the
        // PAYLOAD path. A fresh send on this now-over-age session — which never
        // rekeyed, idle sessions don't — may not seal payload, so the endpoint
        // tears it down there, surfacing a `Dead`.
        handle.send(b"too-old".to_vec()).unwrap();
        let backstopped = loop {
            match pair.a.next_event().await.expect("event") {
                Event::Dead { conn, .. } if conn == a_conn => break true,
                _ => continue,
            }
        };
        assert!(
            backstopped,
            "a payload send past REJECT_AGE must hit the backstop"
        );
    })
    .await;
}

/// The REJECT_AGE backstop guards the payload-seal path, but for a genuinely
/// stuck session under SYMMETRIC loss — a reliable DATA that will never be
/// ACKed, the peer entirely gone — liveness fires first: unACKed ⇒ not
/// receiving ⇒ `DEAD_TIMEOUT`. So here the retransmit-path age gate (rekey at
/// 120 s, the 180 s backstop) is preempted by the 15 s liveness death. This
/// pins WHICH mechanism wins in the symmetric case, deterministically.
/// (The wire POLICY cannot express the asymmetric shape — but a session-layer
/// puppet can, and does:
/// `asymmetric_loss_retransmit_gate_rekeys_then_backstops` proves the age
/// gate fires for real when the liveness path stays up.)
#[tokio::test(start_paused = true)]
async fn retransmit_into_partition_dies_by_liveness() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle, a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Advance off the establishment instant so the send lands strictly after
        // the last receive, then cut the wire and send a reliable DATA: recovery
        // will retransmit it (PTO, quietly) on every tick while the session ages,
        // but a partitioned session hears nothing back.
        time::sleep(Duration::from_secs(1)).await;
        pair.net.partition(addr(ADDR_B));
        let start = Instant::now();
        handle.send(b"unacked-forever".to_vec()).unwrap();

        let dead_at = loop {
            match pair.a.next_event().await.expect("event") {
                Event::Dead { conn, .. } if conn == a_conn => break Instant::now(),
                Event::Established { conn, .. } if conn == a_conn => {
                    panic!("a partitioned session must not re-handshake back to life")
                }
                _ => continue,
            }
        };
        let elapsed = dead_at.duration_since(start);
        assert!(
            elapsed >= DEAD_TIMEOUT && elapsed <= DEAD_TIMEOUT + Duration::from_millis(600),
            "died at {elapsed:?}, expected liveness (~15 s)"
        );
        assert!(
            elapsed < REKEY_AGE,
            "liveness must preempt the retransmit-path age gate (rekey 120 s / backstop 180 s)"
        );
    })
    .await;
}

// ── 11. Size caps ─────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn size_caps() {
    local(async {
        let net = Network::new();
        let mut tap = net.tap();
        let id_a = software_identity(0x11, 0xA1);
        let id_b = software_identity(0x22, 0xB2);
        let a_static = id_a.static_public();
        let b_static = id_b.static_public();
        let wire_a = net.endpoint(addr(ADDR_A), FlakyPolicy::perfect(), [0xA0; 32]);
        let wire_b = net.endpoint(addr(ADDR_B), FlakyPolicy::perfect(), [0xB0; 32]);
        let mut a = Endpoint::start(
            id_a,
            wire_a,
            Config::new().with_rng_seed([1; 32]).allow(&b_static),
        );
        let mut b = Endpoint::start(
            id_b,
            wire_b,
            Config::new().with_rng_seed([2; 32]).allow(&a_static),
        );

        let (handle, _a, _b) = establish(&mut a, &mut b, b_static).await;

        // Oversize send is a typed error, never enqueued. The boundary moved
        // with the Leg 2 frame layer (the brief's cap-move ruling): the cap is
        // now MAX_MESSAGE = MAX_PLAINTEXT − DATA_OVERHEAD, what fits after
        // framing overhead in one sealed packet.
        let oversize = vec![0u8; crate::frame::MAX_MESSAGE + 1];
        assert_eq!(
            handle.send(oversize),
            Err(SlitherError::PayloadTooLarge {
                len: crate::frame::MAX_MESSAGE + 1,
                max: crate::frame::MAX_MESSAGE,
            })
        );

        // A max-size message round-trips inside a single ≤ 1200-byte datagram
        // (11-byte DATA overhead + 1159-byte message = the exact 1170-byte
        // plaintext, so the wire packet still fills exactly 1200 bytes).
        let max = vec![0x5A; crate::frame::MAX_MESSAGE];
        handle.send(max.clone()).unwrap();
        let on_wire = loop {
            let spied = tap.recv().await.expect("tapped datagram");
            if classify(&spied.bytes) == Some(PacketKind::Data)
                && let Some((_h, ct)) = DataHeader::parse(&spied.bytes)
                && ct.len() > crate::wire::AEAD_TAG_LEN
            {
                break spied.bytes.len();
            }
        };
        assert_eq!(
            on_wire,
            crate::wire::MAX_DATAGRAM,
            "max payload fills exactly 1200 bytes"
        );
        assert!(on_wire <= crate::wire::MAX_DATAGRAM);
        assert_eq!(next_incoming(&mut b).await, max);
    })
    .await;
}

// ── 12. Unknown version / type / reserved ⇒ dropped, session unharmed ─────────

#[tokio::test(start_paused = true)]
async fn unknown_packets_dropped() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), true);
        let (handle, _a, _b) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Junk: unknown version, unknown type, reserved types, a short packet.
        pair.net
            .inject(addr(ADDR_B), addr(ADDR_A), vec![0x03, 0x02, 1, 2, 3]); // bad version
        pair.net
            .inject(addr(ADDR_B), addr(ADDR_A), vec![0x08, VERSION, 9, 9, 9]); // unknown type
        pair.net
            .inject(addr(ADDR_B), addr(ADDR_A), vec![0x04, VERSION]); // reserved (close)
        pair.net
            .inject(addr(ADDR_B), addr(ADDR_A), vec![0x05, VERSION]); // reserved (cookie)
        pair.net.inject(addr(ADDR_B), addr(ADDR_A), vec![0x03]); // too short
        pair.net.inject(addr(ADDR_B), addr(ADDR_A), vec![0xFF; 300]); // garbage

        // The session is unharmed: a real payload still gets through.
        handle.send(b"unharmed".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut pair.b).await, b"unharmed");
    })
    .await;
}

// ── 14. Allow-list: an unlisted (but valid) initiator is dropped ──────────────

#[tokio::test(start_paused = true)]
async fn allow_list_rejects_unlisted() {
    local(async {
        // B does NOT allow A.
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect(), false);
        let _handle = pair.a.connect(addr(ADDR_B), pair.b_static);

        // B must never establish a session, and must send no HandshakeResp.
        let no_b_event = time::timeout(Duration::from_secs(30), pair.b.next_event()).await;
        assert!(
            no_b_event.is_err(),
            "unlisted initiator must not establish on B"
        );
        assert_eq!(
            count_resps(&pair.net),
            0,
            "no HandshakeResp for an unlisted static"
        );

        // A, receiving nothing, eventually gives up.
        let event = pair.a.next_event().await.expect("event");
        assert!(
            matches!(event, Event::Failed { .. }),
            "connect gives up, got {event:?}"
        );
    })
    .await;
}

// ── 14b. Policy cost: an unlisted initiator costs the responder one DH ────────

#[tokio::test(start_paused = true)]
async fn unlisted_initiator_costs_one_dh_and_no_resp() {
    local(async {
        let net = Network::new();
        let counting = CountingIdentity::new(0x22, 0xB2).unwrap();
        let b_static = counting.static_public();
        let dh_count = counting.dh_count();
        let id_c = software_identity(0x55, 0xE5);
        let c_static = id_c.static_public();

        let wire_b = net.endpoint(addr(ADDR_B), FlakyPolicy::perfect(), [0xB0; 32]);
        let wire_c = net.endpoint(addr(3), FlakyPolicy::perfect(), [0xC0; 32]);
        // B allows C — and NOT the unlisted initiator injected below.
        let mut b = Endpoint::start(
            counting,
            wire_b,
            Config::new().with_rng_seed([2; 32]).allow(&c_static),
        );
        let mut c = Endpoint::start(
            id_c,
            wire_c,
            Config::new().with_rng_seed([3; 32]).allow(&b_static),
        );

        // A mac1-valid HandshakeInit from an unlisted (but genuine) initiator,
        // injected straight at B.
        let mut unlisted = software_identity(0x11, 0xA1);
        let (init, _pending) = handshake::build_init(
            unlisted.provider(),
            unlisted.static_secret(),
            b_static,
            0x5555_5555,
            Timestamp { secs: 7, nanos: 7 },
        )
        .unwrap();
        net.inject(addr(ADDR_B), addr(ADDR_A), init);

        // B pays exactly ONE ECDH (the reveal's `es`) and sends no
        // HandshakeResp — the ruled policy cost (2026/07/16).
        time::sleep(Duration::from_secs(2)).await;
        assert_eq!(
            dh_count.load(Ordering::SeqCst),
            1,
            "an unlisted, mac1-valid initiation costs exactly one ECDH"
        );
        assert_eq!(
            count_resps(&net),
            0,
            "no HandshakeResp for an unlisted static"
        );

        // An allow-listed initiator then completes normally: the admitted
        // handshake adds the responder's full es + ss + ee + se, and only that.
        let (handle, _c_conn, _b_conn) = establish(&mut c, &mut b, b_static).await;
        handle.send(b"listed-and-live".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut b).await, b"listed-and-live");
        assert_eq!(
            dh_count.load(Ordering::SeqCst),
            5,
            "the admitted handshake adds exactly es + ss + ee + se"
        );
    })
    .await;
}

// ── Real-UDP loopback E2E ─────────────────────────────────────────────────────

#[tokio::test]
async fn real_udp_loopback() {
    LocalSet::new()
        .run_until(async {
            let sock_a = tokio::net::UdpSocket::bind(addr(0)).await.unwrap();
            let sock_b = tokio::net::UdpSocket::bind(addr(0)).await.unwrap();
            let addr_b = sock_b.local_addr().unwrap();

            let id_a = software_identity(0x11, 0xA1);
            let id_b = software_identity(0x22, 0xB2);
            let a_static = id_a.static_public();
            let b_static = id_b.static_public();

            let mut a = Endpoint::start(id_a, sock_a, Config::new().with_rng_seed([1; 32]).allow(&b_static));
            let mut b = Endpoint::start(id_b, sock_b, Config::new().with_rng_seed([2; 32]).allow(&a_static));

            let handle = a.connect(addr_b, b_static);
            // Establish over the real socket.
            let mut a_up = false;
            let mut b_conn = None;
            while !a_up || b_conn.is_none() {
                tokio::select! {
                    event = a.next_event() => if let Some(Event::Established { .. }) = event { a_up = true; },
                    event = b.next_event() => if let Some(Event::Established { conn, .. }) = event { b_conn = Some(conn); },
                }
            }

            handle.send(b"loopback-ping".to_vec()).unwrap();
            assert_eq!(next_incoming(&mut b).await, b"loopback-ping");
            let handle_b = b.session(b_conn.unwrap());
            handle_b.send(b"loopback-pong".to_vec()).unwrap();
            assert_eq!(next_incoming(&mut a).await, b"loopback-pong");
        })
        .await;
}

/// The ASYMMETRIC-loss reachability pin for the retransmit-path age gate
/// (reviewer-authored, Leg R1 review — it refuted the "unreachable" claim).
///
/// `is_dead` is *silence after a fresh send* (`last_send > last_recv` and
/// 15 s since the send), and retransmissions are sealed quietly: a peer whose
/// CONTROL packets keep reaching us holds `last_recv` fresh and `is_dead` off
/// forever, while our DATA (and their ACKs) die. Under that loss shape the
/// session ages past `REKEY_AGE` and `REJECT_AGE` with the PTO train running —
/// and the retransmit-path age gate is exactly what must (a) trigger the DH
/// rekey at 120 s and (b) tear the session down at the 180 s backstop when the
/// rekey cannot complete (the first PTO consult past 180 s lands at ~196 s on
/// the doubling schedule).
///
/// `FlakyPolicy` cannot express this (no per-size drop; a full one-way
/// `block_path` silences the peer's keepalive dance too), so the peer is a
/// PUPPET driven at the session layer: its msg2 and its keepalives are
/// injected raw and always arrive; everything A sends toward it vanishes (it
/// has no inbox); it never ACKs and never answers the rekey.
#[tokio::test(start_paused = true)]
async fn asymmetric_loss_retransmit_gate_rekeys_then_backstops() {
    local(async {
        let net = Network::new();
        let mut id_b = software_identity(0x22, 0xB2);
        let a_static = software_identity(0x11, 0xA1).static_public();
        let b_static = id_b.static_public();
        // Only A gets a wire: datagrams toward ADDR_B have no inbox and vanish.
        let wire_a = net.endpoint(addr(ADDR_A), FlakyPolicy::perfect(), [0xA0; 32]);
        let cfg_a = Config::new().with_rng_seed([0x01; 32]).allow(&b_static);
        let mut a = Endpoint::start(software_identity(0x11, 0xA1), wire_a, cfg_a);

        let handle = a.connect(addr(ADDR_B), b_static);

        // Answer A's msg1 by hand (the puppet's one cooperative act).
        let msg1 = loop {
            match net
                .sends()
                .into_iter()
                .find(|s| classify(&s.bytes) == Some(PacketKind::HandshakeInit))
            {
                Some(spied) => break spied.bytes,
                None => time::sleep(Duration::from_millis(50)).await,
            }
        };
        let allow: HashSet<[u8; 33]> = [a_static.to_compressed()].into_iter().collect();
        let accept = handshake::accept_init(
            id_b.provider(),
            &id_b.static_public(),
            id_b.static_secret(),
            &msg1,
            &allow,
        )
        .expect("puppet accepts A's msg1");
        let (resp, established) = accept.accept(0x0B0B_0B0B).expect("puppet msg2");
        net.inject(addr(ADDR_A), addr(ADDR_B), resp);

        let a_conn = loop {
            match a.next_event().await.expect("event") {
                Event::Established { conn, .. } => break conn,
                _ => continue,
            }
        };
        let start = Instant::now();
        let mut puppet = Session::new(established, addr(ADDR_A), start);

        // One reliable DATA the puppet will never ACK: the PTO train — and with
        // it the retransmit-path age gate — runs for the session's whole life.
        time::sleep(Duration::from_secs(1)).await;
        handle.send(b"unacked-but-alive".to_vec()).unwrap();

        let inits = |net: &Network| {
            net.sends()
                .into_iter()
                .filter(|s| classify(&s.bytes) == Some(PacketKind::HandshakeInit))
                .count()
        };
        let inits_before = inits(&net);
        let mut rekey_init_at = None;
        let mut dead_at = None;

        // ~210 virtual seconds: a puppet keepalive every 5 s holds `is_dead`
        // off (A keeps receiving); nothing ACKs the DATA.
        'outer: for i in 0..210u64 {
            time::sleep(Duration::from_secs(1)).await;
            let age = start.elapsed();
            if i % 5 == 0 {
                let ka = puppet.seal(&[], Instant::now()).expect("puppet keepalive");
                net.inject(addr(ADDR_A), addr(ADDR_B), ka);
            }
            if rekey_init_at.is_none() && inits(&net) > inits_before {
                rekey_init_at = Some(age);
            }
            while let Ok(event) = tokio::time::timeout(Duration::ZERO, a.next_event()).await {
                match event {
                    Some(Event::Dead { conn, .. }) if conn == a_conn => {
                        dead_at = Some(age);
                        break 'outer;
                    }
                    Some(Event::Established { conn, .. }) if conn == a_conn => {
                        panic!("the puppet never answers a rekey; nothing may re-establish")
                    }
                    _ => continue,
                }
            }
        }

        // (a) The DH rekey fired on the retransmit path, in [REKEY_AGE,
        //     REJECT_AGE) — no fresh send ever happened after t=1 s.
        let rekey_at = rekey_init_at.expect("the retransmit-path gate must trigger the rekey");
        assert!(
            rekey_at >= Duration::from_secs(120) && rekey_at < Duration::from_secs(180),
            "rekey init at {rekey_at:?}, expected within [REKEY_AGE, REJECT_AGE)"
        );
        // (b) The backstop — not liveness — killed it: well past REJECT_AGE
        //     (liveness would have struck at ~16 s absent the puppet's dance),
        //     at the first PTO consult after 180 s.
        let dead_at = dead_at.expect("the REJECT_AGE backstop must tear the session down");
        assert!(
            dead_at >= Duration::from_secs(180),
            "died at {dead_at:?}: before REJECT_AGE — an idle/liveness death crept back"
        );
        assert!(
            dead_at <= Duration::from_secs(205),
            "died at {dead_at:?}: the retransmit-path gate failed to consult in time"
        );
    })
    .await;
}

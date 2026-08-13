//! End-to-end Leg 2 flow tests: reliable frames, ACK ranges, RFC 9002 loss
//! detection, and the PTO — over the in-memory [`FlakyWire`] harness on
//! tokio's paused clock, exactly as the Leg 1 suite in `flow.rs`.
//!
//! Coverage map (the Leg 2 brief):
//! frame codec + coalescing + malformed-drop + size bounds live in
//! `frame::tests`; ACK-range construction and the RFC 9002 unit behaviour
//! (packet/time thresholds, PTO backoff, RTT smoothing, dedup) live in
//! `recovery::tests`. Here: lost_message_redelivered_once_on_a_fresh_counter ·
//! empty_send_surfaces_as_empty_incoming ·
//! duplicate_retransmit_is_acked_but_surfaces_once ·
//! acks_keep_flow_over_a_lossy_wire · pto_fires_and_backoff_doubles ·
//! rtt_tracking_prevents_spurious_probes ·
//! coalesced_retransmits_ride_one_packet ·
//! queued_sends_before_establish_flow_reliably.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use hiss::curve::p256::P256r1PublicKey;
use tokio::task::LocalSet;
use tokio::time::{self, Instant};

use crate::endpoint::{Config, ConnId, Endpoint, Event, SessionHandle};
use crate::handshake::Identity;
use crate::testutil::{FlakyPolicy, Network, software_identity};
use crate::wire::{AEAD_TAG_LEN, DataHeader, PacketKind, classify};

// ── Harness (the flow.rs shape) ───────────────────────────────────────────────

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

const ADDR_A: u16 = 1;
const ADDR_B: u16 = 2;

struct Pair {
    net: Network,
    a: Endpoint,
    b: Endpoint,
    b_static: P256r1PublicKey,
}

fn setup(policy_a: FlakyPolicy, policy_b: FlakyPolicy) -> Pair {
    let net = Network::new();
    let id_a = software_identity(0x11, 0xA1);
    let id_b = software_identity(0x22, 0xB2);
    let a_static = id_a.static_public();
    let b_static = id_b.static_public();

    let wire_a = net.endpoint(addr(ADDR_A), policy_a, [0xA0; 32]);
    let wire_b = net.endpoint(addr(ADDR_B), policy_b, [0xB0; 32]);

    let cfg_a = Config::new().with_rng_seed([0x01; 32]).allow(&b_static);
    let cfg_b = Config::new().with_rng_seed([0x02; 32]).allow(&a_static);

    let a = Endpoint::start(id_a, wire_a, cfg_a);
    let b = Endpoint::start(id_b, wire_b, cfg_b);
    Pair {
        net,
        a,
        b,
        b_static,
    }
}

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

/// The counters of every frame-bearing Data packet `src` attempted to send
/// (pre-loss), keyed by ciphertext length — keepalives (tag-only) never match,
/// and a `ciphertext_len` filter isolates one traffic shape (a DATA of a known
/// message length vs a bare 32-byte ACK).
fn data_counters(net: &Network, src: SocketAddr, ciphertext_len: usize) -> Vec<u64> {
    net.sends()
        .into_iter()
        .filter(|s| s.src == src && classify(&s.bytes) == Some(PacketKind::Data))
        .filter_map(|s| {
            let (header, ciphertext) = DataHeader::parse(&s.bytes)?;
            (ciphertext.len() == ciphertext_len).then(|| header.counter())
        })
        .collect()
}

/// The sealed ciphertext length of a packet carrying one DATA frame of
/// `message_len` bytes: 11-byte frame header + message + 16-byte AEAD tag.
fn data_ct_len(message_len: usize) -> usize {
    crate::frame::DATA_OVERHEAD + message_len + AEAD_TAG_LEN
}

/// The sealed ciphertext length of a bare no-range ACK packet: the 16-byte ACK
/// header + the 16-byte AEAD tag.
const ACK_CT_LEN: usize = 32;

// ── Loss ⇒ the frame is retransmitted on a NEW counter, delivered once ────────

#[tokio::test(start_paused = true)]
async fn lost_message_redelivered_once_on_a_fresh_counter() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Cut B off, send, and heal 4 s later: the original DATA is lost; the
        // PTO probe train redelivers it.
        pair.net.partition(addr(ADDR_B));
        handle.send(b"m".to_vec()).unwrap();
        time::sleep(Duration::from_secs(4)).await;
        pair.net.heal(addr(ADDR_B));

        assert_eq!(next_incoming(&mut pair.b).await, b"m");

        // The retransmission rode a NEW counter: at least two frame-bearing
        // packets left A, and every counter is distinct.
        let counters = data_counters(&pair.net, addr(ADDR_A), data_ct_len(1));
        assert!(
            counters.len() >= 2,
            "expected the original send plus at least one probe, got {counters:?}"
        );
        let distinct: HashSet<u64> = counters.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            counters.len(),
            "a retransmitted frame must never reuse a packet counter"
        );

        // Exactly once: nothing further surfaces on B.
        let extra = time::timeout(Duration::from_secs(3), next_incoming(&mut pair.b)).await;
        assert!(extra.is_err(), "the message must surface exactly once");
    })
    .await;
}

// ── An empty send is a real reliable message, not a keepalive ─────────────────

#[tokio::test(start_paused = true)]
async fn empty_send_surfaces_as_empty_incoming() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // An empty payload is a genuine reliable DATA message (SPEC §9.7),
        // distinct from a bare keepalive: it must round-trip to an empty
        // `Incoming`, exactly once.
        handle.send(Vec::<u8>::new()).unwrap();
        assert_eq!(next_incoming(&mut pair.b).await, Vec::<u8>::new());

        let extra = time::timeout(Duration::from_secs(3), next_incoming(&mut pair.b)).await;
        assert!(
            extra.is_err(),
            "the empty message must surface exactly once"
        );
    })
    .await;
}

// ── A duplicate delivery (retransmit after a lost ACK) is ACKed, not re-surfaced

#[tokio::test(start_paused = true)]
async fn duplicate_retransmit_is_acked_but_surfaces_once() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Lose every ACK (B→A) while the data keeps flowing: B delivers the
        // original, A never hears, and the probe hands B a duplicate DATA on a
        // fresh counter.
        pair.net.block_path(addr(ADDR_B), addr(ADDR_A));
        handle.send(b"m".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut pair.b).await, b"m");

        // Let at least one probe arrive (first PTO ≈ 1 s), then heal the ACK
        // path so a later probe's ACK completes the exchange.
        time::sleep(Duration::from_secs(4)).await;
        pair.net.heal_path(addr(ADDR_B), addr(ADDR_A));

        // B saw the DATA at least twice…
        let deliveries = data_counters(&pair.net, addr(ADDR_A), data_ct_len(1));
        assert!(
            deliveries.len() >= 2,
            "expected the original plus probes, got {deliveries:?}"
        );
        // …and ACKed every copy (the attempts are on the send log even while
        // the path was blocked)…
        let acks = data_counters(&pair.net, addr(ADDR_B), ACK_CT_LEN);
        assert!(
            acks.len() >= 2,
            "every duplicate is ACKed again, got {acks:?}"
        );
        // …but surfaced it exactly once.
        let extra = time::timeout(Duration::from_secs(8), next_incoming(&mut pair.b)).await;
        assert!(extra.is_err(), "a duplicate DATA must not re-surface");

        // The exchange completes and the session stays healthy.
        handle.send(b"still-alive".to_vec()).unwrap();
        assert_eq!(next_incoming(&mut pair.b).await, b"still-alive");
    })
    .await;
}

// ── ACK ranges + retransmission keep flow over a genuinely lossy wire ─────────

#[tokio::test(start_paused = true)]
async fn acks_keep_flow_over_a_lossy_wire() {
    local(async {
        // 20 % loss in BOTH directions (data and ACKs), plus jitter-driven
        // reordering: gappy replay windows make the ACK ranges earn their keep,
        // and every message must still surface exactly once.
        let flaky = FlakyPolicy {
            loss: 0.2,
            duplicate: 0.0,
            base_delay: Duration::ZERO,
            jitter: Duration::from_millis(30),
            drop_first: 0,
        };
        let mut pair = setup(flaky, flaky);
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        const N: usize = 30;
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
        assert_eq!(seen, expected, "every message delivered exactly once");
    })
    .await;
}

// ── PTO fires, elicits a probe, and the backoff doubles ───────────────────────

#[tokio::test(start_paused = true)]
async fn pto_fires_and_backoff_doubles() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // With every ACK unreachable, the PTO probe train is the only traffic:
        // pre-sample PTO = 333 + max(4·166.5, 1) + 25 = 1024 ms, doubling per
        // unanswered probe (evaluated on the 250 ms tick).
        pair.net.partition(addr(ADDR_B));
        handle.send(b"m".to_vec()).unwrap();
        let sends = |net: &Network| data_counters(net, addr(ADDR_A), data_ct_len(1)).len();

        time::sleep(Duration::from_millis(500)).await;
        assert_eq!(sends(&pair.net), 1, "no probe before the first PTO");
        time::sleep(Duration::from_millis(1_500)).await; // t ≈ 2.0 s
        assert_eq!(sends(&pair.net), 2, "the first PTO fired (~1.0–1.3 s)");
        time::sleep(Duration::from_millis(900)).await; // t ≈ 2.9 s
        assert_eq!(sends(&pair.net), 2, "the backoff doubled: no probe yet");
        time::sleep(Duration::from_millis(1_400)).await; // t ≈ 4.3 s
        assert_eq!(sends(&pair.net), 3, "the second PTO fired (~3.1–3.6 s)");
        time::sleep(Duration::from_millis(2_800)).await; // t ≈ 7.1 s
        assert_eq!(sends(&pair.net), 3, "the backoff doubled again");
        time::sleep(Duration::from_millis(1_500)).await; // t ≈ 8.6 s
        assert_eq!(sends(&pair.net), 4, "the third PTO fired (~7.2–8.0 s)");
    })
    .await;
}

// ── A measured RTT keeps the PTO quiet (no spurious probes under delay) ───────

#[tokio::test(start_paused = true)]
async fn rtt_tracking_prevents_spurious_probes() {
    local(async {
        // A symmetric 100 ms one-way delay: RTT ≈ 200 ms. Loss detection and
        // the PTO must stay quiet — every message is sent exactly once.
        let slow = FlakyPolicy {
            loss: 0.0,
            duplicate: 0.0,
            base_delay: Duration::from_millis(100),
            jitter: Duration::ZERO,
            drop_first: 0,
        };
        let mut pair = setup(slow, slow);
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        const N: usize = 5;
        for i in 0..N {
            handle.send(format!("m{i}").into_bytes()).unwrap();
        }
        let mut seen = HashSet::new();
        while seen.len() < N {
            seen.insert(next_incoming(&mut pair.b).await);
        }

        // Give any mistimed probe a generous chance to fire, then assert none
        // did: the smoothed RTT (≈ 200 ms) keeps the PTO well above the path
        // delay, and the ACK ranges acknowledge everything in flight.
        time::sleep(Duration::from_secs(5)).await;
        let sends = data_counters(&pair.net, addr(ADDR_A), data_ct_len(2));
        assert_eq!(
            sends.len(),
            N,
            "a measured RTT must not trigger spurious retransmission"
        );
    })
    .await;
}

// ── Time-threshold losses re-queue together and coalesce into one packet ──────

#[tokio::test(start_paused = true)]
async fn coalesced_retransmits_ride_one_packet() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let (handle, _a_conn, _b_conn) = establish(&mut pair.a, &mut pair.b, pair.b_static).await;

        // Three messages into a partition: all three originals are lost. The
        // PTO probes the oldest; its ACK (largest = the probe's counter) makes
        // the two survivors lost by count/time together, and one pump coalesces
        // both retransmissions into a single sealed packet.
        pair.net.partition(addr(ADDR_B));
        handle.send(b"aa".to_vec()).unwrap();
        handle.send(b"bb".to_vec()).unwrap();
        handle.send(b"cc".to_vec()).unwrap();
        time::sleep(Duration::from_secs(2)).await;
        pair.net.heal(addr(ADDR_B));

        let mut seen = HashSet::new();
        while seen.len() < 3 {
            seen.insert(next_incoming(&mut pair.b).await);
        }
        let expected: HashSet<Vec<u8>> = [b"aa".to_vec(), b"bb".to_vec(), b"cc".to_vec()].into();
        assert_eq!(seen, expected, "all three delivered exactly once");

        // The coalesced packet: two 2-byte DATA frames in one plaintext —
        // ciphertext (11+2)·2 + 16 = 42 bytes, a shape nothing else produces.
        let coalesced = data_counters(&pair.net, addr(ADDR_A), 2 * data_ct_len(2) - AEAD_TAG_LEN);
        assert!(
            !coalesced.is_empty(),
            "the two re-queued frames must coalesce into one packet"
        );
    })
    .await;
}

// ── Sends queued before establishment ride the reliable layer ─────────────────

#[tokio::test(start_paused = true)]
async fn queued_sends_before_establish_flow_reliably() {
    local(async {
        let mut pair = setup(FlakyPolicy::perfect(), FlakyPolicy::perfect());
        let handle = pair.a.connect(addr(ADDR_B), pair.b_static);
        // Queued behind the handshake, flushed through the frame layer on
        // establishment.
        handle.send(b"early-one".to_vec()).unwrap();
        handle.send(b"early-two".to_vec()).unwrap();

        let mut seen = HashSet::new();
        while seen.len() < 2 {
            seen.insert(next_incoming(&mut pair.b).await);
        }
        let expected: HashSet<Vec<u8>> = [b"early-one".to_vec(), b"early-two".to_vec()].into();
        assert_eq!(seen, expected);

        let extra = time::timeout(Duration::from_secs(3), next_incoming(&mut pair.b)).await;
        assert!(extra.is_err(), "each early send surfaces exactly once");
    })
    .await;
}

// ── An Instant sanity anchor for the suite's paused clock ─────────────────────

#[tokio::test(start_paused = true)]
async fn paused_clock_is_virtual() {
    // A guard for the harness assumption every timing test above leans on.
    let start = Instant::now();
    time::sleep(Duration::from_secs(3600)).await;
    assert_eq!(start.elapsed(), Duration::from_secs(3600));
}

//! Two slither endpoints talking over real UDP loopback sockets.
//!
//! Endpoint B allow-lists A's static and listens; endpoint A dials B's
//! static (Noise IK — the responder's static is pre-known to the dialler).
//! Once both sides report `Established`, A sends a reliable message, B
//! answers, and both arrivals are printed.
//!
//! The endpoint actor is `!Send` (a DH provider makes no `Send` promise),
//! so everything runs on a current-thread runtime inside a `LocalSet` —
//! exactly how a consumer embeds slither.
//!
//! Run with: `cargo run --example udp_loopback`

use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use slither::endpoint::{Config, Endpoint, Event};
use slither::handshake::{Identity, SoftwareIdentity};

/// A fresh software identity: a random P-256 static scalar plus an
/// OS-seeded master CSPRNG for the per-handshake ephemerals.
fn fresh_identity() -> SoftwareIdentity {
    loop {
        let mut scalar = [0u8; 32];
        getrandom::fill(&mut scalar).expect("OS entropy for a static scalar");
        let mut master = [0u8; 32];
        getrandom::fill(&mut master).expect("OS entropy for the ephemeral CSPRNG");
        // `from_scalar` rejects a scalar outside [1, n) — astronomically
        // rare from a uniform draw; just draw again.
        if let Ok(id) = SoftwareIdentity::from_scalar(scalar, ChaCha20Rng::from_seed(master)) {
            return id;
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let sock_a = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind A");
            let sock_b = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind B");
            let addr_b = sock_b.local_addr().expect("B's address");

            let id_a = fresh_identity();
            let id_b = fresh_identity();
            let a_static = id_a.static_public();
            let b_static = id_b.static_public();

            // Each side allow-lists the other's static; everyone else is
            // dropped at the mac1 gate / allow-list check.
            let mut a = Endpoint::start(id_a, sock_a, Config::new().allow(&b_static));
            let mut b = Endpoint::start(id_b, sock_b, Config::new().allow(&a_static));

            // A dials B: fire-and-forget — the outcome arrives as an event.
            let a_to_b = a.connect(addr_b, b_static);

            // Drive both event streams until both sides are up.
            let mut a_up = false;
            let mut b_conn = None;
            while !a_up || b_conn.is_none() {
                tokio::select! {
                    event = a.next_event() => match event {
                        Some(Event::Established { endpoint, .. }) => {
                            println!("A: established to {endpoint}");
                            a_up = true;
                        }
                        Some(Event::Failed { error, .. }) => panic!("A: connect failed: {error}"),
                        _ => {}
                    },
                    event = b.next_event() => if let Some(Event::Established { conn, endpoint, .. }) = event {
                        println!("B: established from {endpoint}");
                        b_conn = Some(conn);
                    },
                }
            }

            // A reliable message each way: retransmitted until ACKed,
            // surfaced to the peer exactly once.
            a_to_b.send(b"ping over loopback".to_vec()).expect("queue A->B");
            loop {
                if let Some(Event::Incoming { payload, .. }) = b.next_event().await {
                    println!("B: received {:?}", String::from_utf8_lossy(&payload));
                    break;
                }
            }

            let b_to_a = b.session(b_conn.expect("B's connection id"));
            b_to_a.send(b"pong over loopback".to_vec()).expect("queue B->A");
            loop {
                if let Some(Event::Incoming { payload, .. }) = a.next_event().await {
                    println!("A: received {:?}", String::from_utf8_lossy(&payload));
                    break;
                }
            }
        })
        .await;
}

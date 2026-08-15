//! Temporary probe for C-B1. Not a deliverable — deleted after it answers.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use slither::config::Config;
use slither::identity::{Identity, PublicKeyOf};
use slither::testutil::{CountingIdentity, Network};

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::Endpoint<Id>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

fn spawn(net: &Network, seed: u8, port: u16) -> (Endpoint, Pk, SocketAddr) {
    let a = addr(port);
    let id: Id = CountingIdentity::seeded([seed; 32]);
    let pk = *id.public_static();
    let ep = Endpoint::builder()
        .identity(id)
        .wire(net.wire(a))
        .config(Config::new())
        .build();
    (ep, pk, a)
}

/// A dials B while simultaneously accepting B's chain, so that
/// `Command::AcceptChain(B)` is queued ahead of `Command::Connect(B)`.
///
/// At the moment A's synchronous `connect()` runs, the shell mirror says
/// NONE (true — nothing is installed yet) so it admits and writes PENDING.
/// The driver then processes AcceptChain first: the core's own static map
/// is still empty for B, so `core::Endpoint::accept()`'s
/// `statics.get(&peer_key).is_some()` guard does NOT fire and the accept
/// succeeds, making B LIVE in both maps. Connect is then refused by the
/// core — and the shell asserts that cannot happen.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cb1_accept_ahead_of_connect_on_one_static() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (a, _a_pk, a_addr) = spawn(&net, 1, 9001);
            let (b, b_pk, b_addr) = spawn(&net, 2, 9002);

            // B dials A, so A has a chain from B to walk.
            let _dial = b.connect(a_addr, _a_pk).expect("B dials A");
            tokio::time::sleep(Duration::from_millis(50)).await;

            let intro = a.accept().await.expect("A gets B's intro");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");

            // The race: accept's command is queued first, then connect's.
            let accept_side = proven.accept();
            let connect_side = async { a.connect(b_addr, b_pk) };
            let (accepted, connected) = tokio::join!(accept_side, connect_side);

            println!("accept  -> {:?}", accepted.as_ref().map(|_| "Connection"));
            println!("connect -> {:?}", connected.as_ref().map(|_| "Connecting"));

            // Whatever the verdict, the driver must still be alive.
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(c) = connected {
                let r = tokio::time::timeout(Duration::from_secs(120), c).await;
                println!("Connecting resolved -> {:?}", r.map(|x| x.is_ok()));
            }
            println!("PROBE SURVIVED — driver did not panic");
        })
        .await;
}

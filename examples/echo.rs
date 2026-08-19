//! A minimal echo: one dialler, one answerer, one message sent and echoed
//! back.
//!
//! Run it with `cargo run --example echo`.
//!
//! This uses `slither::block_on`, not `#[tokio::main]` — the driver task is
//! `!Send`, so it needs a current-thread runtime with a `LocalSet`, and
//! `block_on` sets that up for you.
use hiss::noise::{Blake2b, ChaChaPoly, P256};
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use slither::identity::SoftwareIdentity;
use slither::{Config, Endpoint, Identity};

// 1. Declare one crypto suite per module. IK is the only handshake pattern;
//    the macro names the generated type `MySuite`.
slither::channel! {
    pub MySuite<P256, ChaChaPoly, Blake2b>;
}

fn rng() -> ChaCha20Rng {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS entropy");
    ChaCha20Rng::from_seed(seed)
}

fn main() {
    // 2. block_on gives you the current-thread runtime plus the LocalSet
    //    the !Send driver needs.
    slither::block_on(async {
        // 3. Two identities. generate() mints a fresh static keypair.
        let dialler: SoftwareIdentity<MySuite> =
            SoftwareIdentity::generate(rng()).expect("generate dialler identity");
        let answerer: SoftwareIdentity<MySuite> =
            SoftwareIdentity::generate(rng()).expect("generate answerer identity");

        // 4. The key the dialler needs to reach the answerer. Printing it
        //    here is only for the demo: in a real deployment this value
        //    crosses an out-of-band channel, never the wire itself.
        let answerer_key = *answerer.public_static();
        println!("answerer public key: {:?}", answerer_key.as_ref());
        // 5. A tokio::net::UdpSocket is a Wire out of the box.
        let dial_sock = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind dial socket");
        let ans_sock = tokio::net::UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind ans socket");
        let answerer_addr = ans_sock.local_addr().expect("answerer addr");
        let dial_ep = Endpoint::builder()
            .identity(dialler)
            .wire(dial_sock)
            .config(Config::new())
            .build();
        let answer_ep = Endpoint::builder()
            .identity(answerer)
            .wire(ans_sock)
            .config(Config::new())
            .build();

        // 6. accept() is a loop for the endpoint's whole lifetime; a real
        //    server calls it in one. This example takes a single intro and
        //    returns, because it is an example and not a server.
        let answering = tokio::task::spawn_local(async move {
            let intro = answer_ep.accept().await.expect("answerer: got an intro");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");
            let conn = proven.accept().await.expect("accept");
            let msg = conn.recv_message().await.expect("recv_message");
            println!("answerer received: {:?}", String::from_utf8_lossy(&msg));
            conn.send_message(&msg).await.expect("send_message (echo)");
            conn.close(slither::constants::NO_ERROR, b"done").await;
        });

        // 7. connect() is synchronous and spends 0 DH; awaiting the
        //    Connecting future is what runs the handshake.
        let conn = dial_ep
            .connect(answerer_addr, answerer_key)
            .expect("connect (sync, 0 DH)")
            .await
            .expect("handshake completed");
        conn.send_message(b"hello").await.expect("send_message");
        let reply = conn.recv_message().await.expect("recv_message");
        println!("dialler received: {:?}", String::from_utf8_lossy(&reply));
        conn.close(slither::constants::NO_ERROR, b"done").await;
        answering.await.expect("answering task join");
        println!("round trip complete");
    });
}

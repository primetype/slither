//! Public acceptance tests for checked monotonic-deadline arithmetic.
//!
//! This is the one ruling-284 path exposed directly by `test-util`: a
//! datagram whose injected delay lies beyond Tokio's monotonic-clock horizon
//! remains unreachable, while a later ordinary datagram to the same inbox
//! still arrives.  The test distinguishes that behavior from clamping the
//! first delivery to `now`, which would make the older sequence win.

#![cfg(feature = "test-util")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use slither::shell::wire::Wire;
use slither::testutil::{FlakyPolicy, Network};

fn addr(last: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, last)), port)
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_delivery_neither_fires_early_nor_blocks_later_traffic() {
    let net = Network::seeded(0x284);
    let sender = net.endpoint(addr(1, 28_401));
    let receiver = net.endpoint(addr(2, 28_402));

    sender.set_policy(FlakyPolicy::perfect().with_delay(Duration::MAX, Duration::ZERO));
    sender
        .send_to(b"beyond the horizon", receiver.local_addr())
        .await
        .expect("an unreachable delivery is still an accepted datagram send");

    sender.set_policy(FlakyPolicy::perfect());
    sender
        .send_to(b"ordinary", receiver.local_addr())
        .await
        .expect("the later datagram is accepted");

    let mut buf = [0u8; 64];
    let (n, source) = tokio::time::timeout(Duration::from_secs(1), receiver.recv_from(&mut buf))
        .await
        .expect("the reachable datagram must not be blocked by the heap head")
        .expect("the in-memory wire remains live");
    assert_eq!(&buf[..n], b"ordinary");
    assert_eq!(source, sender.local_addr());

    assert!(
        tokio::time::timeout(Duration::from_secs(1), receiver.recv_from(&mut buf))
            .await
            .is_err(),
        "the older unreachable datagram must not be clamped to a fabricated earlier delivery"
    );
    assert_eq!(net.sends(), 2, "both sends were accepted by the wire");
    assert_eq!(
        net.tap().len(),
        2,
        "the unreachable delivery was not mistaken for loss or send failure"
    );
}

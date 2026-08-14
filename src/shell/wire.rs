//! The datagram substrate seam — `SPEC.md` §16.3.

use std::net::SocketAddr;

/// The datagram substrate an endpoint runs over. §16.3, ratified
/// 2026/08/14 (ruling 49).
///
/// # Normative properties
///
/// 1. **The application supplies it**, through `Endpoint::builder()`
///    (§16.2) — so an application needing its own socket options, a
///    dual-stack or per-interface arrangement, a tunnel, or a simulator
///    installs one without forking the crate.
/// 2. **It is not required to be `Send`**, and no `Send` bound may be
///    added to it or to the futures its methods return. The driver is a
///    single `!Send` actor, and the reasoning that keeps a DH provider
///    free of `Send` (a hardware static) keeps a `Wire` free of it.
/// 3. **Both methods take `&self`**, because the one driver task owns
///    the seam and drives both directions from it. A `Wire` needs no
///    interior handle duplication, and connections never send on socket
///    clones.
/// 4. **`testutil::FlakyWire` is a `Wire`** — the in-memory
///    implementation the paused-clock flow tests ride (§16.10), which is
///    why every timer in the spec is testable without a kernel, a port,
///    or a sleep.
///
/// # A failing `send_to` is traced, not acted on
///
/// When [`send_to`](Wire::send_to) returns `Err`, the driver **must**
/// trace it under `slither::io` (§18.2) against the connection whose
/// datagram it was, carrying the destination address and the underlying
/// error. It does **not** kill the connection, resolve any verb with an
/// error, or produce a `Notification`: liveness is receive-driven (§7.4),
/// and `ENETUNREACH` is the signal that *precedes* a successful roam
/// (§7.3), not one that follows a dead connection.
///
/// The obligation is the **driver's**, not the implementation's: a `Wire`
/// that traced its own failures would double-count, and would move a
/// protocol obligation onto application code.
///
/// # This trait is deliberately not dyn-compatible
///
/// The methods are `async fn` in trait, which desugars to a return-position
/// `impl Future`. That is what makes the returned future `Send` *iff* the
/// implementation's future is — `!Send` is admitted by construction rather
/// than required — and it is what §16.3's normative code block writes. The
/// price is that there is no `Box<dyn Wire>`: **an endpoint is generic over
/// its wire (`Endpoint<W: Wire>`), not erased.** This is recorded here
/// because there is no way to assert dyn-incompatibility in a test, and a
/// later slice should meet the constraint as documentation rather than as a
/// compiler error.
///
/// If type erasure is ever needed, the way in is a *private* `DynWire`
/// shim inside the shell that boxes the futures **without** a `Send`
/// bound. That keeps the allocation off the hot path for everyone who does
/// not need erasure, and keeps this trait as the spec writes it.
// The `async_fn_in_trait` lint warns that callers cannot add a `+ Send`
// bound to the returned futures. That is precisely what normative property
// 2 above requires, so the warning is the design working, not a defect.
#[allow(async_fn_in_trait)]
pub trait Wire {
    /// Send `buf` to `addr`, returning the bytes written.
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize>;
    /// Receive one datagram into `buf`, returning its length and source.
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
}

impl Wire for tokio::net::UdpSocket {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        tokio::net::UdpSocket::send_to(self, buf, addr).await
    }
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        tokio::net::UdpSocket::recv_from(self, buf).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blanket impl over a real socket is not a fiction: two kernel
    /// sockets exchange a datagram each way through the **trait** methods.
    ///
    /// The only test in slice 0 that touches the kernel, and therefore the
    /// only one without a paused clock — a real socket needs a real
    /// reactor.
    #[tokio::test]
    async fn udp_socket_round_trip() {
        use tokio::net::UdpSocket;

        let a = UdpSocket::bind("127.0.0.1:0").await.expect("bind a");
        let b = UdpSocket::bind("127.0.0.1:0").await.expect("bind b");
        let addr_a = a.local_addr().expect("local_addr a");
        let addr_b = b.local_addr().expect("local_addr b");

        // a → b, through the trait.
        let sent = Wire::send_to(&a, b"ping", addr_b).await.expect("send a→b");
        assert_eq!(sent, 4);

        let mut buf = [0u8; 64];
        let (n, from) = Wire::recv_from(&b, &mut buf).await.expect("recv at b");
        assert_eq!(&buf[..n], b"ping");
        assert_eq!(from, addr_a);

        // b → a, through the trait.
        let sent = Wire::send_to(&b, b"pong", addr_a).await.expect("send b→a");
        assert_eq!(sent, 4);

        let (n, from) = Wire::recv_from(&a, &mut buf).await.expect("recv at a");
        assert_eq!(&buf[..n], b"pong");
        assert_eq!(from, addr_b);
    }

    /// A compile-fence for `CLAUDE.md`'s architecture invariant: a `Wire`
    /// need not be `Send`, and neither must any generic helper that drives
    /// one.
    ///
    /// `NotSend` holds an `Rc`, so it is `!Send`. If a `Send` bound is ever
    /// added to [`Wire`], to its methods' futures, or to `drive` below,
    /// **this test stops compiling**. It should never be deleted.
    #[tokio::test]
    async fn a_wire_need_not_be_send() {
        struct NotSend(#[allow(dead_code)] std::rc::Rc<()>);

        impl Wire for NotSend {
            async fn send_to(&self, buf: &[u8], _addr: SocketAddr) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
                buf[..2].copy_from_slice(b"hi");
                Ok((2, "127.0.0.1:1".parse().expect("literal addr")))
            }
        }

        // No `Send` bound here, and none may ever be added.
        async fn drive<W: Wire>(w: &W) -> std::io::Result<usize> {
            let mut buf = [0u8; 8];
            let (n, _from) = w.recv_from(&mut buf).await?;
            w.send_to(&buf[..n], "127.0.0.1:1".parse().expect("literal addr"))
                .await
        }

        let w = NotSend(std::rc::Rc::new(()));
        assert_eq!(drive(&w).await.expect("drive"), 2);

        // And the fence proper: `w` is genuinely `!Send`, so the future
        // above could not have been made `Send` by accident.
        fn _assert_not_send_is_still_a_wire<W: Wire>() {}
        _assert_not_send_is_still_a_wire::<NotSend>();
    }
}

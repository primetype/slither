//! The `LocalSet` tax, paid once — `PLAN.md` §3.5.
//!
//! Every slither handle and the driver behind them are `!Send` by
//! requirement rather than by accident (§16.3, S21: a hardware-backed static
//! key such as an iOS Secure Enclave `SecKey` is not `Send`), so every
//! consumer must build a **current-thread** runtime inside a
//! [`tokio::task::LocalSet`]. That is the first thing a new consumer hits,
//! and this module is the one line that answers it.

use std::future::Future;

/// Run `future` to completion on a current-thread runtime inside a
/// [`LocalSet`](tokio::task::LocalSet).
///
/// §16.11's `LocalSet` helper, and the one line that pays that tax. It is in
/// [`slither::prelude`](crate::prelude), so `use slither::prelude::*;` brings
/// it in; the adapter **types** in this module are not — they are named from
/// `slither::compat::*`.
///
/// **There is no `Send` bound on `F` or on `F::Output`, and that is the
/// entire point.** `tokio::runtime::Runtime::block_on` alone is not enough:
/// the endpoint's driver is spawned with [`tokio::task::spawn_local`], which
/// panics outside a `LocalSet` — a panic
/// [`EndpointBuilder::build`](crate::shell::EndpointBuilder) intercepts and
/// re-raises as slither's own message, naming this function as the remedy.
///
/// # Panics
///
/// - If the runtime cannot be built.
/// - **If called from inside an existing tokio runtime.** This is the
///   mistake a consumer will actually make: `block_on` inside
///   `#[tokio::main]`, or inside a `#[tokio::test]`, panics with *"Cannot
///   start a runtime from within a runtime"*. Code already on a
///   current-thread runtime wants `LocalSet::run_until` directly, not this.
///
/// # Not the test harness
///
/// This builds an **unpaused** runtime. slither's own timers are asserted on
/// tokio's paused clock (§16.10), so `block_on` must never appear in a test
/// that asserts a timer.
///
/// # Example
///
/// The whole shape, in one call: the endpoint built, a connection dialled, a
/// [`BiStream`](crate::shell::BiStream) written and its tail acknowledged.
///
/// ```no_run
/// use slither::prelude::*;
///
/// use std::net::SocketAddr;
///
/// fn send_hello<I: Identity + 'static>(
///     identity: I,
///     socket: tokio::net::UdpSocket,
///     peer: slither::identity::PublicKeyOf<I>,
///     addr: SocketAddr,
/// ) -> Result<(), Box<dyn std::error::Error>>
/// where
///     I::Suite: slither::packet::Handshake<Psk = ()>,
/// {
///     block_on(async move {
///         let endpoint = Endpoint::builder()
///             .identity(identity)
///             .wire(socket)
///             .build();
///
///         let conn = endpoint.connect(addr, peer)?.await?;
///         let (mut send, _recv) = conn.open_bi().await?.split();
///
///         // `write` accepts what flow-control credit admits and may accept
///         // less than the whole buffer; `AsyncWriteExt::write_all` is that
///         // loop, and works because of `compat::io`.
///         let _n = send.write(b"hello").await?;
///
///         // `finish()` sets the FIN; `acked()` is what waits for the peer
///         // to have it. `AsyncWrite::shutdown()` is exactly these two.
///         send.finish().await?;
///         send.acked().await?;
///         Ok(())
///     })
/// }
/// ```
pub fn block_on<F: Future>(future: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("slither::prelude::block_on could not build a current-thread tokio runtime");
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, future)
}

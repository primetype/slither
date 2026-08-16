//! §16.11's service face — `feature = "tower"`.
//!
//! Three shapes over [`tower_service::Service`]: the dialer on
//! [`Endpoint`], the stream-opener on `&`[`Connection`], and [`serve`] for
//! the accepting side.
//!
//! # One bi stream per call — **ruling 225**
//!
//! slither has **no request/response correlation on the wire**, so a
//! `Service` over §9.8's message verb would need a request id the transport
//! does not carry — slither would have to invent application framing above
//! its own frame layer to find one, which would be the first place the crate
//! defined wire semantics above §8. The correlation slither already has is a
//! **stream**: `call()` opens one bi stream, `finish()` ends the request and
//! EOF ends the response.
//!
//! There is **no `Rpc`** (ruling 230): it is cut from this slice, nothing
//! here carries a codec bound, and the `tower` feature stays the cheap
//! `tower-service` dependency with no `tokio-util` in it.
//!
//! # The `!Send` caveat, stated rather than discovered — **S33**
//!
//! §16.3's driver is `!Send` **by requirement**, not by accident, and S21 is
//! why: a hardware-backed static key (an iOS Secure Enclave `SecKey`) is not
//! `Send`, and a transport that demanded it would exclude the case the DH
//! provider seam exists for.
//!
//! This surface composes regardless. [`tower_service::Service`] itself,
//! `tokio_util::codec`, `futures`' `Stream`/`Sink` combinators and
//! `tokio::io::copy` all carry **no `Send` bound**.
//!
//! What does **not** compose is anything that spawns onto a work-stealing
//! executor:
//!
//! - `tower::buffer::Buffer` and `tower::util::spawn_ready`;
//! - `tower::util::BoxService` — use **`UnsyncBoxService`** instead;
//! - hyper;
//! - plain `tokio::spawn` on any slither handle. `spawn_local` is the
//!   substitute, and it is what [`serve`] uses.
//!
//! A `Send` façade over a driver on its own thread would close that gap and
//! is **deliberately out of v0.2 scope**: it re-crosses the core→shell seam
//! with channels. The handle shapes are already compatible with adding one
//! later without a breaking change.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower_service::Service;

use crate::core::Dir;
use crate::error::{ConnectError, ConnectionLost};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::Handshake;
use crate::shell::{BiStream, Connecting, Connection, Endpoint, WakerSlot};

// ═══════════════════════════════════════════════════════════════════════
// The dialer
// ═══════════════════════════════════════════════════════════════════════

/// `Endpoint` as a dialer: request is *(address, peer static)*, response is
/// the established [`Connection`].
///
/// This can be implemented **on the owned type** — unlike the stream-opener
/// below — because [`Endpoint::connect`] is *not* `async` and the
/// [`Connecting`] it returns owns everything it needs: it borrows nothing
/// from `&self`, so the future can outlive the `&mut self` that `call` is
/// given.
impl<I: Identity> Service<(SocketAddr, PublicKeyOf<I>)> for Endpoint<I> {
    type Response = Connection<I::Suite>;
    type Error = ConnectError;
    type Future = Connect<I>;

    /// Always ready. An endpoint has no admission control of its own — the
    /// one-connection-per-static test is §16.1's and it is answered inside
    /// `call`, synchronously, from the endpoint core's own static map.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (remote, remote_static): (SocketAddr, PublicKeyOf<I>)) -> Self::Future {
        Connect {
            state: match self.connect(remote, remote_static) {
                Ok(connecting) => ConnectState::Dialling(connecting),
                Err(err) => ConnectState::Failed(Some(err)),
            },
        }
    }
}

/// The future [`Endpoint`]'s [`Service`] returns.
///
/// It exists to fold `connect`'s **synchronous** error into a future.
/// [`ConnectError::AlreadyConnected`] arrives *before any await* (rulings 87
/// and 90: `mint_pending` costs 0 DH and runs in the verb itself), and
/// [`Service::call`] cannot return a `Result`.
///
/// On the success path it **owns the [`Connecting`]**, not a copy of its
/// parts. That is load-bearing: dropping a `Connecting` is ruling 50's
/// cancellation, which retires the pending in the endpoint core's static map
/// in the same instant, so a dropped `Service` future cancels exactly as
/// `drop(connecting)` does — including the *"succeeds on the very next
/// line"* retry that ruling 50 makes structural.
pub struct Connect<I: Identity> {
    state: ConnectState<I>,
}

enum ConnectState<I: Identity> {
    Dialling(Connecting<I>),
    /// `connect()` failed synchronously. `Option` so the error can be moved
    /// out on the one poll that consumes it.
    Failed(Option<ConnectError>),
}

impl<I: Identity> Future for Connect<I> {
    type Output = Result<Connection<I::Suite>, ConnectError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.get_mut().state {
            // `Connecting` is a plain struct of `Rc`s and a `ConnectionId`,
            // so it is `Unpin` and needs no projection.
            ConnectState::Dialling(connecting) => Pin::new(connecting).poll(cx),
            ConnectState::Failed(err) => Poll::Ready(Err(err
                .take()
                .expect("a Connect future was polled after it completed"))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The stream-opener
// ═══════════════════════════════════════════════════════════════════════

/// `&Connection` as a stream-opener: `call(())` opens one bidirectional
/// stream, and **the stream is the correlation** (ruling 225).
///
/// # Why the receiver is `&'a Connection<S>` and not `Connection<S>`
///
/// `PLAN.md` §3.4 sketches `impl Service<()> for Connection`, and
/// `CONTRACT-8.md` §0.0 repeats that sketch when restating ruling 225. **The
/// sketched form cannot be written.** [`Connection::open_bi`] takes `&self`,
/// so the future it produces borrows the connection; but
/// [`Service::call`] is `call(&mut self, req)` and hands the body an
/// *anonymous* lifetime, while `type Future` is an associated type that has
/// no way to name it — `Service` has no GAT. Implementing on
/// `&'a Connection<S>` puts the lifetime in `Self`, where the associated
/// type can name it.
///
/// Ruling 225 decided *which verb* the service face is built over — one bi
/// stream per call rather than the message verb — and says nothing about the
/// receiver; this is the shape that carries that decision into code.
/// `&'a Connection<S>` is `Copy`, so `call(&mut self, ..)` moves it into the
/// future freely, and a consumer writes:
///
/// ```text
/// let mut svc = &conn;          // or: (&conn).oneshot(())
/// let stream = svc.call(()).await?;
/// ```
///
/// `S: 'static` is ruling 228's bound on the waker slot `OpenBi` holds, not
/// a new one: it restricts nothing that can exist, because a `Connection<S>`
/// is only reachable through an `Endpoint<I>` and `EndpointBuilder::build`
/// already requires `I: 'static`.
impl<'a, S: Handshake + 'static> Service<()> for &'a Connection<S> {
    type Response = BiStream<S>;
    type Error = ConnectionLost;
    type Future = OpenBi<'a, S>;

    /// Always ready. §10.4's cumulative stream limit is **not** admission
    /// control that belongs here: ruling 101 makes an exhausted stream space
    /// a *park* rather than a failure, so the waiting happens in the future.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        // `&'a Connection<S>` is `Copy`, so the reference comes out of
        // `&mut self` with its `'a` intact and moves into the future. That
        // is the whole reason the impl is on the reference type.
        let conn = *self;
        OpenBi {
            conn,
            slot: conn.opener_slot_boxed(Dir::Bi),
        }
    }
}

/// The future `&`[`Connection`]'s [`Service`] returns: one
/// [`open_bi`](Connection::open_bi).
///
/// Cancel-safe on `open_bi`'s terms — a dropped future has opened no stream,
/// because the index and the handle are taken in one synchronous step.
pub struct OpenBi<'a, S: Handshake> {
    conn: &'a Connection<S>,
    /// Held for the future's whole life and released on drop, which is the
    /// whole of `poll_open_bi`'s cancel-safety (ruling 228).
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Future for OpenBi<'_, S> {
    type Output = Result<BiStream<S>, ConnectionLost>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.conn.poll_open_bi(cx, this.slot.key())
    }
}

/// The dialling side on the **owned** handle — the form `UnsyncBoxService`
/// and every other `'static` combinator require.
///
/// # Why both this and the `&Connection` impl exist
///
/// **[RATIFIED — ruling 239; this reverses ruling 236.]** `CONTRACT-8.md`
/// §6.2 held that the owned form *"cannot be written"*, because
/// [`Connection::open_bi`] takes `&self` and [`Service::call`] hands the body
/// an anonymous lifetime that `type Future` cannot name. **The premise is
/// sound and the conclusion does not follow**: the future does not have to
/// borrow. It can *own* a second handle, and an in-crate `clone_handle`
/// mints one with the same accounting `Drop` reverses, so nothing about
/// §16.2's last-handle rule moves.
///
/// The reason it must exist is a bound neither §6.2 nor ruling 236 looked at:
/// `UnsyncBoxService::new<S>` requires `S: Service<..> + 'static`, and
/// `&'a Connection<S>` is not `'static`. **S33's acceptance clause — *"an
/// `UnsyncBoxService` composes"* — is unsatisfiable on the borrowed form**,
/// and a story is the acceptance criterion.
///
/// # The hazard, stated because it is silent
///
/// `ServiceExt::oneshot` takes `self` **by value**. On this impl
/// `conn.oneshot(())` therefore **moves the connection into the combinator**,
/// and dropping the resulting future runs §16.2's last-handle rule —
/// `close(NO_ERROR, "")`. That is a closed connection from a call that looks
/// like a request. Reach for `(&conn).oneshot(())`, which moves a `Copy`
/// reference and leaves the handle where it is, or call
/// `svc.call(())` directly.
impl<S: Handshake + 'static> Service<()> for Connection<S> {
    type Response = BiStream<S>;
    type Error = ConnectionLost;
    type Future = OpenBiOwned<S>;

    /// Always ready — see the `&Connection` impl for why §10.4's stream
    /// limit is not admission control.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        let conn = Box::new(self.clone_handle());
        let slot = conn.opener_slot_boxed(Dir::Bi);
        OpenBiOwned { conn, slot }
    }
}

/// The future [`Connection`]'s owned [`Service`] returns: one
/// [`open_bi`](Connection::open_bi), over a handle the future owns.
///
/// Cancel-safe on `open_bi`'s terms, and handle-safe on §16.2's: the handle
/// inside is accounted, so dropping this future decrements the count it
/// incremented and never triggers the last-handle close while the caller
/// still holds one.
pub struct OpenBiOwned<S: Handshake> {
    /// **Boxed deliberately.** `Pin::get_mut` in `poll` needs `Self: Unpin`,
    /// and `Connection<S>` is `Unpin` only if `S`'s public-key type is —
    /// which is an associated type with no such bound. `Box<T>` is `Unpin`
    /// unconditionally, and this module is `#![forbid(unsafe_code)]`, so the
    /// projection alternatives are closed. One allocation per `call()`,
    /// against opening a stream.
    conn: Box<Connection<S>>,
    /// Held for the future's whole life and released on drop (ruling 228).
    slot: WakerSlot<Box<dyn FnMut(u64)>>,
}

impl<S: Handshake> Future for OpenBiOwned<S> {
    type Output = Result<BiStream<S>, ConnectionLost>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.conn.poll_open_bi(cx, this.slot.key())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The accepting side
// ═══════════════════════════════════════════════════════════════════════

/// Serve `svc` over every bidirectional stream the peer opens, until the
/// connection dies.
///
/// One task per accepted stream, spawned with
/// [`spawn_local`](tokio::task::spawn_local) — **never `tokio::spawn`**,
/// which is S21 and the whole of the `!Send` actor path. That is what the
/// `'static` bounds are for: `spawn_local` needs the spawned future to
/// outlive any borrow. **No `Send` bound appears anywhere on this path.**
///
/// # It returns `ConnectionLost`, not `Result<(), ConnectionLost>`
///
/// `PLAN.md` §3.4 writes the `Result`, and `CONTRACT-8.md` §6.3 flags this
/// as a deliberate deviation: the loop **has no success exit**. It runs
/// until `accept_bi()` fails, and the only way `accept_bi()` can fail is the
/// connection dying. A `Result` whose `Ok` is unreachable is a shape rather
/// than information, so the death reason is returned directly.
///
/// # The service's own errors are the service's
///
/// `Svc::Response` and `Svc::Error` are unconstrained and both are
/// **discarded**. `serve` does not trace them: §18.2's trace targets are a
/// **closed list of exactly five**, and adding one is a protocol revision.
/// A service that wants its failures recorded records them itself.
///
/// # Readiness
///
/// Each accepted stream gets its own clone, and that clone is driven to
/// [`poll_ready`](Service::poll_ready) inside its task before
/// [`call`](Service::call) — `tower`'s protocol requires it, and a service
/// that is never ready simply never runs its call. This is
/// `ServiceExt::oneshot`'s shape, written out because slither does not
/// depend on `tower`'s `util` feature.
///
/// # Example shape
///
/// ```text
/// let lost = slither::compat::serve(&conn, my_service).await;
/// tracing::info!(%lost, "connection ended");
/// ```
pub async fn serve<H, Svc>(conn: &Connection<H>, svc: Svc) -> ConnectionLost
where
    // `'static` is `spawn_local`'s requirement on the spawned future, which
    // captures the accepted `BiStream<H>`. It is **not** a `Send`
    // requirement — the same pair of bounds `EndpointBuilder::build` carries
    // for the driver task.
    H: Handshake + 'static,
    Svc: Service<BiStream<H>> + Clone + 'static,
    Svc::Future: 'static,
{
    loop {
        let stream = match conn.accept_bi().await {
            Ok(stream) => stream,
            Err(lost) => return lost,
        };
        let mut svc = svc.clone();
        tokio::task::spawn_local(async move {
            if std::future::poll_fn(|cx| svc.poll_ready(cx)).await.is_err() {
                return;
            }
            let _ = svc.call(stream).await;
        });
    }
}

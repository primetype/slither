//! §16.2's `Endpoint`, its builder, and `Connecting`.
//!
//! An endpoint is one socket's worth of protocol state and the handle an
//! application dials and accepts through. It is a **thin client over the
//! driver** (§16.3): it owns no socket, sends nothing itself, and holds no
//! second copy of the cores' state.

use std::cell::RefCell;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::config::Config;
use crate::core::Endpoint as CoreEndpoint;
use crate::error::ConnectError;
use crate::identity::{Identity, PublicKeyOf};

use super::connection::Connection;
use super::driver::Driver;
use super::shared::{Command, PendingOutcome, PendingSlot, Shell};
use super::staged::Intro;
use super::wire::Wire;

/// One endpoint: a [`Wire`], an [`Identity`], and everything §16.1's object
/// model hangs off them.
///
/// # It is a handle
///
/// The driver lives while any handle lives (§16.3). An `Endpoint` is one;
/// so is a [`Connecting`] (ruling 62) and so is a [`Connection`]. A staged
/// object is **not**, and neither is a `closed()` future.
///
/// Dropping an `Endpoint` while a `Connection` lives does **not** tear
/// anything down — that is documentation obligation #4, and it is the
/// opposite of the obvious guess.
///
/// # One connection per remote static
///
/// [`connect`](Self::connect) to a static that already has a live
/// connection, or an outbound attempt still in flight, returns
/// [`ConnectError::AlreadyConnected`]. **Reconnecting is `close()` then
/// dial**, not `connect()` again — documentation obligation #1.
pub struct Endpoint<I: Identity> {
    shell: Shell<I>,
}

impl<I: Identity> Endpoint<I> {
    /// Start building an endpoint.
    ///
    /// The builder is where §16.3's `Wire` is supplied, so an application
    /// that needs its own socket options, a dual-stack arrangement, a
    /// tunnel or a simulator installs one without forking the crate.
    pub fn builder<W: Wire>() -> EndpointBuilder<I, W> {
        EndpointBuilder::new()
    }

    /// Wait for the next introduction (§6.2, §6.3).
    ///
    /// `None` means the endpoint is closed — the driver has stopped. On a
    /// live endpoint this never resolves until a peer initiates.
    ///
    /// Costs **0 DH**: an [`Intro`] is msg1's parked bytes and its source,
    /// nothing more. The ladder starts at
    /// [`Intro::read_identity`](super::staged::Intro::read_identity).
    ///
    /// # Cancel-safety
    ///
    /// Dropping the future before it resolves takes nothing: the
    /// introduction is handed to the next caller. If it was handed over in
    /// the same instant the future was dropped, the `Intro` is dropped with
    /// it — which is §6.2's silent reject, the documented meaning of
    /// dropping a staged object, not a loss.
    pub async fn accept(&self) -> Option<Intro<I>> {
        if self.shell.driver_stopped() {
            return None;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shell.send(Command::Accept(tx));
        rx.await.ok()
    }

    /// Dial `remote_static` at `remote` (§5.5).
    ///
    /// **Not `async`** — §16.2 declares it so, and ruling 87 explains why
    /// it can be: `connect()` performs no DH (§6.1's initiator costs are
    /// paid when msg1 is built, on the driver), so it only has to mint the
    /// attempt. [`ConnectError::AlreadyConnected`] therefore arrives
    /// **before any await**, from a synchronous read of the shared cell
    /// §16.8 already requires for the accessors.
    ///
    /// That synchronous read is also what makes ruling 50's
    /// cancellation-ordering **MUST** structural rather than a discipline:
    /// [`Connecting::drop`] writes the static back to NONE in the same cell
    /// this reads, so
    ///
    /// ```text
    /// drop(connecting);                    // cancels
    /// let retry = endpoint.connect(a, k)?; // succeeds, on the very next line
    /// ```
    ///
    /// works **with no advance of the clock between them** — which is the
    /// `timeout()` idiom every consumer writes.
    pub fn connect(
        &self,
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
    ) -> Result<Connecting<I>, ConnectError> {
        let static_key = remote_static.as_ref().to_vec();

        let attempt = {
            let mut state = self.shell.state.borrow_mut();
            if state.driver_stopped {
                // Unreachable while this `Endpoint` lives — it is a handle,
                // and the driver runs while any handle does. `ConnectError`
                // deliberately has no `EndpointDropped` (ruling 62); a
                // local fault with 0 DH spent is what `Local` names.
                return Err(ConnectError::Local);
            }
            state.claim_static(static_key.clone())?
        };

        let slot = Rc::new(RefCell::new(PendingSlot::new()));
        self.shell.send(Command::Connect {
            remote,
            remote_static,
            static_key: static_key.clone(),
            attempt,
            slot: Rc::clone(&slot),
        });

        Ok(Connecting::new(
            self.shell.clone(),
            slot,
            static_key,
            attempt,
        ))
    }
}

impl<I: Identity> std::fmt::Debug for Endpoint<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").finish_non_exhaustive()
    }
}

impl<I: Identity> Drop for Endpoint<I> {
    fn drop(&mut self) {
        self.shell.release();
    }
}

/// An outbound attempt in flight — §16.2's `connect()` future.
///
/// # It is a handle, and dropping it cancels
///
/// **[RATIFIED 2026/08/14 — rulings 62 and 50]** A `Connecting` owns an
/// in-flight protocol attempt: a pending, its index, and §5.5's retransmit
/// train. So the driver lives while one lives, and dropping it **cancels
/// the attempt immediately** — the train stops, the pending and its index
/// go (§17.3), its dialled address leaves §6.5's hint set (§17.4), and the
/// static leaves PENDING for NONE (§5.4).
///
/// **Nothing is transmitted.** An attempt that never completed has no
/// session to close and no wire signal to send, as in §15.4's
/// endpoint-dropped row. If the peer already answered and installed a
/// half-open session, it is *not* told: it reaps it at `DEAD_TIMEOUT`
/// (25 s) in silence.
///
/// The cancellation is ordered **ahead of any endpoint verb issued after
/// the drop returns**, so an immediate redial cannot observe the corpse.
pub struct Connecting<I: Identity> {
    shell: Shell<I>,
    slot: Rc<RefCell<PendingSlot<I>>>,
    static_key: Vec<u8>,
    attempt: u64,
    /// Set once this future has handed its result out, so `Drop` knows the
    /// attempt is no longer in flight.
    resolved: bool,
}

impl<I: Identity> Connecting<I> {
    fn new(
        shell: Shell<I>,
        slot: Rc<RefCell<PendingSlot<I>>>,
        static_key: Vec<u8>,
        attempt: u64,
    ) -> Self {
        shell.acquire();
        Self {
            shell,
            slot,
            static_key,
            attempt,
            resolved: false,
        }
    }
}

impl<I: Identity> Future for Connecting<I> {
    type Output = Result<Connection<I::Suite>, ConnectError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut slot = this.slot.borrow_mut();
        match std::mem::replace(&mut slot.outcome, PendingOutcome::Waiting) {
            PendingOutcome::Waiting => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            PendingOutcome::Ready(connection) => {
                this.resolved = true;
                Poll::Ready(Ok(connection))
            }
            PendingOutcome::Failed(error) => {
                this.resolved = true;
                Poll::Ready(Err(error))
            }
        }
    }
}

impl<I: Identity> std::fmt::Debug for Connecting<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connecting").finish_non_exhaustive()
    }
}

impl<I: Identity> Drop for Connecting<I> {
    fn drop(&mut self) {
        // Only a *waiting* attempt is in flight. One that already resolved
        // — including one resolved into the slot and never polled — is not,
        // and cancelling it would free a static its live `Connection` still
        // holds. An unpolled `Ready` drops its `Connection` with the slot,
        // which is the ordinary last-handle drop.
        let in_flight =
            !self.resolved && matches!(self.slot.borrow().outcome, PendingOutcome::Waiting);

        if in_flight {
            // Ruling 87: the static returns to NONE **here**, synchronously,
            // in the very cell `connect()` reads — which is what an
            // immediate redial with no clock advance observes.
            self.shell
                .state
                .borrow_mut()
                .release_static(&self.static_key, self.attempt);
            // Ruling 50's MUST: this command and any later endpoint verb
            // travel on one FIFO channel, so the cancellation is ordered
            // ahead of everything the application issues after the drop.
            self.shell.send(Command::Cancel(Rc::clone(&self.slot)));
        }

        self.shell.release();
    }
}

/// Builds an [`Endpoint`] and spawns its driver (§16.2).
///
/// # A `LocalSet` is required
///
/// [`build`](Self::build) calls `tokio::task::spawn_local`, which **panics
/// outside a `LocalSet`**. That is §16.3's architecture, not an
/// implementation detail: the driver is a single `!Send` actor because a DH
/// provider is not required to be `Send` — a hardware-backed static key is
/// the case the seam exists for — and a `Wire` is not required to be
/// `Send` either.
///
/// ```no_run
/// # async fn doc<I, W>(identity: I, wire: W)
/// # where
/// #     I: slither::Identity + 'static,
/// #     W: slither::shell::wire::Wire + 'static,
/// # {
/// let local = tokio::task::LocalSet::new();
/// local
///     .run_until(async move {
///         let endpoint = slither::shell::Endpoint::builder()
///             .identity(identity)
///             .wire(wire)
///             .build();
///         // … dial and accept through `endpoint` …
///         drop(endpoint);
///     })
///     .await;
/// # }
/// ```
pub struct EndpointBuilder<I: Identity, W: Wire> {
    identity: Option<I>,
    wire: Option<W>,
    config: Config,
    rng_seed: Option<[u8; 32]>,
}

impl<I: Identity, W: Wire> EndpointBuilder<I, W> {
    fn new() -> Self {
        Self {
            identity: None,
            wire: None,
            config: Config::new(),
            rng_seed: None,
        }
    }

    /// The static-key seam (§2.4). Required.
    #[must_use]
    pub fn identity(mut self, identity: I) -> Self {
        self.identity = Some(identity);
        self
    }

    /// The datagram substrate (§16.3). Required.
    ///
    /// A `tokio::net::UdpSocket` is a [`Wire`] out of the box; so is
    /// `testutil::FlakyWire`, which is what makes §16.10's kernel-free
    /// drivability work.
    #[must_use]
    pub fn wire(mut self, wire: W) -> Self {
        self.wire = Some(wire);
        self
    }

    /// Endpoint configuration. Defaults to [`Config::new`].
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Seed §16.6's one endpoint RNG explicitly.
    ///
    /// # This is a test-only facility
    ///
    /// §16.6: session and pending indices **MUST** be unpredictable to an
    /// off-path observer — index unpredictability is load-bearing for
    /// §5.5's on-path-only completion spend and §15.2's authenticated-only
    /// linger reply. A caller-chosen seed is therefore **security-relevant**
    /// and is documented as such here rather than being quietly available:
    /// with it, one root seed reproduces every index, every jitter draw and
    /// every connection sub-seed. Without it the endpoint seeds from OS
    /// entropy, which is what a production endpoint must do.
    #[must_use]
    pub fn rng_seed(mut self, seed: [u8; 32]) -> Self {
        self.rng_seed = Some(seed);
        self
    }

    /// Spawn the driver and return the endpoint (§16.3).
    ///
    /// Named `build` for the builder convention, but note what it does:
    /// **it spawns a task**. The driver is the endpoint — there is no
    /// separate `run()` for a caller to forget.
    ///
    /// # Panics
    ///
    /// If [`identity`](Self::identity) or [`wire`](Self::wire) was not
    /// supplied, or if this is called outside a `tokio::task::LocalSet` —
    /// both are programming errors rather than runtime conditions, and
    /// neither is recoverable at the call site.
    ///
    /// Seeding from OS entropy can also fail; `getrandom` panics there, and
    /// an endpoint that silently continued with a predictable seed would
    /// violate §16.6.
    ///
    /// The `'static` bounds are the spawned task's, not a `Send`
    /// requirement: `spawn_local` needs the future to outlive any borrow,
    /// and **no `Send` bound appears anywhere on this path** (§16.3).
    #[must_use]
    pub fn build(self) -> Endpoint<I>
    where
        I: 'static,
        W: 'static,
    {
        let identity = self
            .identity
            .expect("Endpoint::builder() requires an identity");
        let wire = self.wire.expect("Endpoint::builder() requires a wire");
        let rng_seed = self.rng_seed.unwrap_or_else(|| {
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).expect("OS entropy for the endpoint RNG (§16.6)");
            seed
        });

        let core = CoreEndpoint::new(super::shared::now(), self.config, identity, rng_seed);
        let (shell, commands) = Shell::new(core);
        let endpoint = Endpoint {
            shell: shell.clone(),
        };
        // The `Endpoint` is the first handle; count it before the driver
        // can observe a zero.
        endpoint.shell.acquire();
        tokio::task::spawn_local(Driver::new(wire, shell, commands).run());
        endpoint
    }
}

impl<I: Identity, W: Wire> Default for EndpointBuilder<I, W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Identity, W: Wire> std::fmt::Debug for EndpointBuilder<I, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointBuilder")
            .field("identity", &self.identity.is_some())
            .field("wire", &self.wire.is_some())
            .field("seeded", &self.rng_seed.is_some())
            .finish_non_exhaustive()
    }
}

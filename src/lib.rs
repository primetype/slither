//! A WireGuard-shaped Noise-over-UDP packet layer carrying a QUIC-shaped
//! reliable frame layer.
//!
//! slither seals every datagram with an IK handshake over P-256 /
//! ChaCha20-Poly1305 / BLAKE2b (driven entirely through
//! [`hiss`](https://docs.rs/hiss)), gates initiations behind a keyed-BLAKE2b
//! mac1, and carries streams, messages and unreliable datagrams inside the
//! sealed plaintext with flow control, RFC 9002 loss recovery and
//! congestion control. Connections roam across address changes, rekey by
//! ratchet, and are accepted in *stages*, so an application can inspect a
//! peer's claimed identity before spending a second DH on it — a ladder
//! climbed **in a loop**, not once per connection: `accept()` is drained
//! for the lifetime of the endpoint, by diallers and responders alike
//! (§6.5, documentation obligation #6).
//! slither is `#![forbid(unsafe_code)]`; every Noise and curve operation
//! goes through `hiss`, and no RustCrypto crate appears in the graph.
//!
//! # Quickstart
//!
//! Two peers on one machine: the dialler sends one message, the answerer
//! reads it, both close. The same program, with an echo back and comments,
//! is `examples/echo.rs` in the repository — run it with
//! `cargo run --example echo`.
//!
//! ```no_run
//! use hiss::noise::{Blake2b, ChaChaPoly, P256};
//! use rand_chacha::ChaCha20Rng;
//! use rand_chacha::rand_core::SeedableRng;
//! use slither::identity::SoftwareIdentity;
//! use slither::{Config, Endpoint, Identity};
//!
//! // 1. Every consumer declares one crypto suite. IK is the only pattern,
//! //    and one invocation per module (the generated type is named `IK`).
//! slither::channel! {
//!     /// This application's suite.
//!     pub MySuite<P256, ChaChaPoly, Blake2b>;
//! }
//!
//! fn rng() -> ChaCha20Rng {
//!     let mut seed = [0u8; 32];
//!     getrandom::fill(&mut seed).expect("OS entropy");
//!     ChaCha20Rng::from_seed(seed)
//! }
//!
//! fn main() {
//!     // 2. `block_on` is the current-thread runtime + `LocalSet` the
//!     //    `!Send` driver needs. Do NOT use `#[tokio::main]`.
//!     slither::block_on(async {
//!         // 3. Two identities. `generate` makes a fresh static keypair.
//!         let dialler: SoftwareIdentity<MySuite> =
//!             SoftwareIdentity::generate(rng()).unwrap();
//!         let answerer: SoftwareIdentity<MySuite> =
//!             SoftwareIdentity::generate(rng()).unwrap();
//!         // 4. The key the dialler needs, handed over out of band.
//!         let answerer_key = *answerer.public_static();
//!
//!         // 5. A `tokio::net::UdpSocket` is a `Wire` out of the box.
//!         let a = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
//!         let b = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
//!         let answerer_addr = b.local_addr().unwrap();
//!
//!         let ep_a = Endpoint::builder()
//!             .identity(dialler).wire(a).config(Config::new()).build();
//!         let ep_b = Endpoint::builder()
//!             .identity(answerer).wire(b).config(Config::new()).build();
//!
//!         // 6. Answering is a ladder, so the application can authorise a
//!         //    *claimed* identity before the second DH is spent. Dropping
//!         //    a rung is the silent reject. `accept()` is a LOOP for the
//!         //    lifetime of the endpoint — see "Before you integrate" #6.
//!         let answering = tokio::task::spawn_local(async move {
//!             let intro = ep_b.accept().await.expect("endpoint alive");
//!             let claimed = intro.read_identity().await.unwrap();  // +1 DH
//!             let proven = claimed.authenticate().await.unwrap();  // +1 DH
//!             let conn = proven.accept().await.unwrap();           // +2 DH
//!             let msg = conn.recv_message().await.unwrap();
//!             assert_eq!(msg, b"hello");
//!             conn.close(slither::constants::NO_ERROR, b"done").await;
//!         });
//!
//!         // 7. `connect()` is synchronous and spends 0 DH; awaiting the
//!         //    `Connecting` future is what runs the handshake.
//!         let conn = ep_a
//!             .connect(answerer_addr, answerer_key)
//!             .unwrap()
//!             .await
//!             .unwrap();
//!         conn.send_message(b"hello").await.unwrap();
//!         conn.acked().await.unwrap();
//!         conn.close(slither::constants::NO_ERROR, b"done").await;
//!         answering.await.unwrap();
//!     });
//! }
//! ```
//!
//! # Install
//!
//! ```toml
//! [dependencies]
//! slither = "0.2"
//! # `slither::channel!` expands to absolute `::hiss::…` paths, so your
//! # crate must depend on hiss directly, on the same minor line.
//! hiss = { version = "0.3", default-features = false }
//! # slither's driver runs on your runtime; these are the features it uses.
//! tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
//! rand_chacha = "0.10"   # only for `SoftwareIdentity`: it takes your RNG
//! getrandom = "0.4"      # …and something to seed it from
//! ```
//!
//! `rand_core` must be the **0.10** line hiss names — [`hiss::rand_core`]
//! re-exports it. Two `rand_core` majors in one graph produce an
//! unsatisfiable `CryptoRng` bound, not a version error. MSRV **1.96**.
//!
//! # Features
//!
//! Nothing is on by default.
//!
//! | Feature | What it adds |
//! |---|---|
//! | `test-util` | the `testutil` module, for driving slither in a downstream crate's tests |
//! | `sink` | `Stream` / `Sink` adapters |
//! | `codec` | `tokio_util::codec` support; implies `sink` |
//! | `tower` | `tower::Service` shapes — **one bi stream per call** |
//!
//! **[RATIFIED 2026/08/16 — ruling 225]** The `tower` row read *"a
//! `tower::Service` shape over the message verb"*, and that shape cannot
//! work: slither has **no request/response correlation on the wire**, so a
//! `Service` over §11 messages would need a request id the transport does
//! not carry — slither would have to invent application framing above its
//! own frame layer to find one. The correlation slither already has is a
//! **stream**: `call()` opens one bi stream, `finish()` ends the request,
//! EOF ends the response. `PLAN.md` §3.4 is the reasoned statement and it
//! wins; this row and `Cargo.toml`'s comment were manifest text carrying no
//! argument.
//!
//! # Shape
//!
//! ```text
//! core::Endpoint / core::Connection   pure state machines, no I/O, no clock
//!         ↑ poll_output() to Timeout, after every mutating call    (§16.4)
//! shell: one !Send driver task        owns the cores, owns the Wire (§16.3)
//!         ↑ handles
//! compat: AsyncRead/Write · Stream/Sink · Codec · tower            (§16.11)
//! ```
//!
//! The cores never read a clock — `now: Instant` is an argument on every
//! mutating call — and the shell is a **single `!Send` actor** run with
//! `tokio::task::spawn_local` on a current-thread runtime inside a
//! `LocalSet`. Nothing on that path requires `Send`, deliberately: a
//! hardware-backed static key (an iOS Secure Enclave `SecKey`) is not
//! `Send`, and a transport that demanded it would exclude the case the DH
//! provider seam exists for.
//!
//! Because the cores are pure and [`shell::wire::Wire`] is the only I/O
//! seam, **the whole protocol is drivable without a kernel**: two
//! endpoints over the in-memory `testutil` fabric on tokio's paused clock,
//! with every timer resolving in virtual time.
//!
//! # Before you integrate
//!
//! Six hazards have no code fix. A consumer meets each one by getting it
//! wrong, so each is stated here as well as at its call site.
//!
//! 1. **Reconnecting is `close()` then dial, not `connect()` again.**
//!    `connect()` to a static that already has a live connection returns
//!    [`ConnectError::AlreadyConnected`] — §16.1 admits one session per
//!    peer static, and the *existing* connection is what holds it. "Call
//!    connect again" is the natural guess and it is wrong: it does not
//!    replace the old connection, it does not repair a wedged one, and it
//!    leaves the first connection completely untouched.
//!
//!    An application that wants *reconnect now* releases the static first
//!    and only then dials:
//!
//!    ```no_run
//!    # use std::net::SocketAddr;
//!    # async fn reconnect<I: slither::Identity>(
//!    #     endpoint: &slither::Endpoint<I>,
//!    #     stale: slither::Connection<I::Suite>,
//!    #     peer: slither::PublicKeyOf<I>,
//!    #     addr: SocketAddr,
//!    # ) -> Result<slither::Connection<I::Suite>, slither::ConnectError> {
//!    // Wrong: the static is still LIVE, so this is `AlreadyConnected`
//!    // and the wedged connection is still there afterwards.
//!    //
//!    //     endpoint.connect(addr, peer)?.await
//!
//!    // Right: end the old one, wait for it to be gone, then dial.
//!    stale.close(slither::constants::NO_ERROR, b"reconnecting").await;
//!    stale.closed().await;
//!    drop(stale);
//!    endpoint.connect(addr, peer)?.await
//!    # }
//!    ```
//!
//!    The `closed().await` is not decoration: `close()` returns once the
//!    CLOSE is sealed, and the static is released when the connection's
//!    state is actually dropped (§16.4's `Retired`). Dialling before then
//!    races the release.
//! 2. **Do not punish on evidence a third party can manufacture.**
//!    Authorise on it; do not punish on it. The rule is one sentence and it
//!    reaches three rungs of the handshake, because *"an attacker gets an
//!    innocent peer banned"* is the same hazard at all three.
//!
//!    - **A *claimed* static is not an authenticated one.** The identity
//!      `read_identity()` reveals during a staged accept is an
//!      **unauthenticated assertion**, made before any DH proves
//!      possession. Denylisting on it lets an attacker claim any public key
//!      in order to get its owner banned.
//!    - **The source address and `sender_index` are worse, not better.**
//!      §6.1 forbids durable state keyed on **three** quantities — the
//!      claimed static, the source address, and `sender_index` — and the
//!      two beside the static are the *cheaper* keys to abuse: a spoofed
//!      source costs an attacker no DH at all, needs no knowledge of
//!      anyone's public key, and has no return-routability proof at stage
//!      0. A source-address denylist under the flood §6.3 describes bans
//!      spoofed victims. See [`Intro::source`](shell::Intro::source) and
//!      [`Intro::sender_index`](shell::Intro::sender_index).
//!    - **A *proven* static does not make the accusation true.**
//!      [`AuthError::Replay`] is delivered after the `ss` has genuinely
//!      proven the static, which is exactly what makes it look like
//!      trustworthy evidence about that peer. It is not: one captured
//!      initiation lets a third party produce it at will, from any address,
//!      against a peer that has done nothing. It reports *this initiation
//!      is not fresh*, never *this peer misbehaved*.
//! 3. **A connection with nothing to say dies — in 25 s, in silence.**
//!    A connection that has received **no authenticated packet since it was
//!    installed** transmits *nothing at all* and is torn down at
//!    install + `DEAD_TIMEOUT` (25 s) with
//!    [`ConnectionLost::TimedOut`]. **Connecting ahead of need does not
//!    keep a path warm**, and this is the single most surprising behaviour
//!    for a new consumer.
//!
//!    What keeps a connection alive is not a knob. §7.5's keepalive dance
//!    is **automatic for any connection that has carried traffic**: one
//!    application message, in **one** direction, puts the receiver into the
//!    state that makes it answer every 10 s, which puts the sender into it,
//!    and the pair then sustains itself indefinitely with no configuration
//!    anywhere.
//!
//!    The knob is for the case that leaves out — a link that is **mutually
//!    idle** and must nonetheless stay open, through a NAT binding or a
//!    firewall's idle reaper.
//!    [`Connection::set_persistent_keepalive`](shell::Connection::set_persistent_keepalive)
//!    takes an interval in `[1 s, 25 s)` and rejects anything outside it
//!    rather than clamping. Enabling it on **one** side is enough: the
//!    beacon reaches the peer, and the peer's automatic half answers.
//!
//!    A beacon does not defer death, and is not meant to. Both keepalives
//!    are *marking* sends, so they **arm** the death clock; a connection
//!    whose beacons are never answered still ends 25 s after the last
//!    authenticated packet it received. Two consecutive lost beacons at the
//!    10 s default is what that costs.
//! 4. **Teardown triggers on dropping every *handle*, not the endpoint.**
//!    The connection lives as long as any handle to it does, and ends when
//!    the last one is dropped — the opposite of the obvious guess, and
//!    sensitive to the order your values fall out of scope.
//! 5. **Messages and streams do not mix on one connection.** Using
//!    `send_message` alongside `open_uni` on the same connection is a
//!    programming error with a defined, loud failure. The safe and unsafe
//!    shapes look alike at the call site, which is exactly why it is
//!    written down.
//! 6. **`accept()` is a loop for the lifetime of the endpoint, not one
//!    call per connection.** §6.5 puts it as a SHOULD: *"Every application
//!    SHOULD treat `accept()` as a loop for the lifetime of its endpoint —
//!    diallers and responders alike."* Accepting once and moving on is the
//!    natural shape, and it strands the two cases the protocol expects the
//!    *next* `accept()` to repair.
//!
//!    - **A lost msg2 leaves a responder holding a connection the peer
//!      knows nothing about.** msg2 is never retransmitted — every
//!      retransmit is a *completely fresh initiation* (§5.5) — so one
//!      dropped msg2 leaves this side with a live, never-confirmed
//!      connection while the peer re-offers a fresh [`Intro`] every
//!      `RETRANSMIT_BASE` (~5 s) until it gives up at `HANDSHAKE_GIVEUP`
//!      (90 s). **No error ever prompts the retry**: the first `accept()`
//!      *succeeded*. Only the next one closes the gap.
//!    - **A restarted peer is the same shape (§6.8).** Its reconnection
//!      parks as an ordinary `Intro` against our still-live static, the
//!      now-zombie connection keeps running untouched, and nothing tears
//!      it down until the replacing `accept()`. *"Restart needs no
//!      machinery of its own"* is true only because the application is
//!      still listening.
//!
//!    Admitting that fresh `Intro` **is** the replacement, by §6.4's §16.1
//!    guard. Where the connection it displaces is one we **accepted** —
//!    replacement basis `Some(t)`, and the new initiation's timestamp
//!    strictly greater — the install fires [`ConnectionLost::Replaced`] on
//!    the old connection and the new chain completes. Where it is one we
//!    **dialled**, the basis is `None`, no initiation can replace it,
//!    [`AcceptError::Stale`] comes back and §6.8's restart instead resolves
//!    at liveness, at most `DEAD_TIMEOUT` later. Either way the application
//!    side of it is the one instruction: keep accepting.
//!
//! # The spec is the authority
//!
//! **`SPEC.md` is the authority.** Every constant, header layout, frame
//! type and timer in this crate is ratified there, and where the code and
//! the spec disagree the spec is right. The module layout is deliberately
//! one-to-one with the spec's sections so a reviewer can find the code for
//! a section without searching.
//!
//! # Modules
//!
//! - [`identity`] — the static-key seam. A consumer implements
//!   [`Identity`] to put its key behind hardware; [`SoftwareIdentity`] is
//!   the in-memory default.
//! - [`shell`] — the I/O shell: [`Endpoint`], [`Connection`], the staged
//!   accept ladder, the stream handles, and [`shell::wire::Wire`] — the
//!   datagram seam an application supplies.
//! - [`config`] — endpoint configuration and §16.5's injected wall clock.
//! - [`error`] — the closed error taxonomy of §18.1, plus `ConfigError`.
//!   Its ten types are re-exported at the crate root.
//! - [`packet`] — §2–§5's wire: the suite declaration, §6.1's handshake
//!   ladder as a trait, the three headers, mac1 and §3.1's gate.
//! - [`constants`] — every named constant the spec fixes, one home, with
//!   the derived ones re-derived as compile-time assertions.
//! - [`compat`] — §16.11's composability surface: `AsyncRead`/`AsyncWrite`
//!   on the stream handles and the two `io::Error` conversions (ungated),
//!   plus `Stream`/`Sink`, `tokio_util::codec` and `tower::Service` faces
//!   behind their features. It adds **no verb and no state**.
//! - `testutil` — the in-memory `Network` / `FlakyWire` / `FlakyPolicy`
//!   fabric and the counting DH provider, behind the `test-util` feature.
//!   Attested surface (ruling 60), not a test convention: it is
//!   deterministic under a caller-supplied seed, and renaming one of those
//!   three types is a protocol revision.
//! - `core` — §16.4's two sans-io state machines. Crate-internal:
//!   nothing outside the crate drives them directly.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod compat;
pub mod config;
pub mod constants;
pub mod error;
pub mod identity;
pub mod packet;
pub mod shell;
pub(crate) mod varint;

// §16.4's two cores. `pub(crate)` in this slice, deliberately: nothing
// outside the crate can drive them until the driver lands, so publishing
// the surface now would freeze an unusable one under semver — and §16.6
// makes a build that accepts a caller-chosen RNG seed "security-relevant",
// a decision that belongs to the slice with an opinion about the public
// surface. Promotion later is additive; demotion is breaking.
//
// NOTE for every file in this crate: a crate-level `mod core` makes a bare
// `use core::…` ambiguous against the `core` crate in the extern prelude.
// Write `::core::…` for the language core and `crate::core::…` for this
// module — the convention the packet layer already follows.
pub(crate) mod core;

#[cfg(any(test, feature = "test-util"))]
pub mod testutil;

/// §16.11's `LocalSet` helper, re-exported at the crate root.
///
/// Every slither handle and the driver behind them are `!Send` by
/// requirement (S21), so a consumer must run them on a current-thread
/// runtime inside a [`tokio::task::LocalSet`]. This is the one line that
/// pays that tax; see [`compat::block_on`] for the copy-pasteable example
/// and for what it panics on.
///
/// The adapter **types** are not re-exported here — they are named from
/// `slither::compat::*`. `block_on` is the exception because it is the first
/// thing a consumer needs.
pub use compat::block_on;
pub use config::{Config, SystemClock, WallClock};
pub use error::{
    AcceptError, AuthError, ConfigError, ConnectError, ConnectionLost, DatagramError, IntroError,
    MessageError, ReadError, WriteError,
};
pub use identity::{CurveOf, Identity, PrivateKeyOf, PublicKeyOf, SoftwareIdentity};
pub use packet::{Channel, Handshake};
pub use shell::{
    BiStream, Claimed, Connecting, Connection, Endpoint, EndpointBuilder, Intro, Notification,
    Proven, RecvStream, SendStream,
};

/// A completed session's channel binding — **hiss's type, re-exported**.
///
/// **[RATIFIED 2026/08/15 — ruling 89]** `SessionId` is
/// [`hiss::noise::SessionId`] and not a slither wrapper: hiss derives it
/// from the handshake hash, both peers of a session produce the same value,
/// and it is a *public* channel-binding value meant for out-of-band
/// comparison — logging it, or a short-authentication-string check between
/// peers. A slither wrapper would be a type that must be kept equal to
/// hiss's by hand, for no gain.
///
/// # Its `Eq` is not constant-time
///
/// By hiss's deliberate choice. It carries no secret material and **must
/// not be used to compare one**: reaching for it as a session token or an
/// authentication comparison is the mistake this paragraph exists to
/// prevent.
///
/// Read it off a live connection with [`Connection::session_id`].
pub use hiss::noise::SessionId;

// The three identifiers §16.4's surface names that a consumer must be able
// to spell: [`Dir`], [`StreamId`] and [`Timestamp`]. They live inside the
// `pub(crate)` core, so they are re-exported here to be publicly
// *reachable* — `WallClock::now()` returns a [`Timestamp`], and a public
// signature naming an unreachable type is a rustdoc break, not merely a
// lint.
//
// **[RATIFIED 2026/08/18 — ruling 259(iii)]** `ConnectionId` and `IntroId`
// were re-exported here too, which made the count wrong and the surface
// dead: neither appears in any public signature and neither has a public
// constructor or accessor, so nothing a consumer can write names them. The
// re-export bought nothing and would have frozen two types under semver.
// They stay `pub(crate)`; the comment now names the three it counts.
pub use crate::core::{Dir, StreamId, Timestamp};

/// The `hiss` slither was built against, re-exported so the version you
/// must match is findable.
///
/// [`channel!`] expands to absolute `::hiss::…` paths — `hiss::noise!`
/// emits them and a `macro_rules` wrapper cannot rewrite them — so a crate
/// that invokes it **must depend on `hiss` itself**, on the same minor
/// line slither does:
///
/// ```toml
/// [dependencies]
/// slither = "0.2"
/// hiss = { version = "0.3", default-features = false }
/// ```
///
/// **This re-export does not remove that requirement.** It exists so the
/// version can be read off slither's own docs, and so a type that crosses
/// the boundary (a `Curve::PublicKey`, a `DhProvider`) can be named
/// through one path when you would rather not name two.
pub use hiss;

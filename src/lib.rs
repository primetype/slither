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
//! peer's claimed identity before spending a second DH on it.
//!
//! **`SPEC.md` is the authority.** Every constant, header layout, frame
//! type and timer in this crate is ratified there, and where the code and
//! the spec disagree the spec is right. The module layout is deliberately
//! one-to-one with the spec's sections so a reviewer can find the code for
//! a section without searching.
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
//! # The five documentation obligations
//!
//! Five hazards have no code fix. A consumer meets each one by getting it
//! wrong, so each is stated here as well as at its call site.
//!
//! 1. **Reconnecting is `close()` then dial, not `connect()` again.**
//!    `connect()` to a static that already has a live connection returns
//!    [`ConnectError::AlreadyConnected`]. "Call connect again" is the
//!    natural guess and it is wrong.
//! 2. **A *claimed* static is not an authenticated one.** The identity
//!    `read_identity()` reveals during a staged accept is an
//!    **unauthenticated assertion**, made before any DH proves possession.
//!    Denylisting on it lets an attacker claim any public key in order to
//!    get its owner banned. Authorise on it; do not punish on it.
//! 3. **A connection with nothing to say dies.** An idle connection is
//!    torn down after `DEAD_TIMEOUT` (25 s), so connecting ahead of need
//!    does not keep a path warm. It is deliberate, and it is the single
//!    most surprising behaviour for a new consumer; a connection that must
//!    outlive its traffic needs a persistent keepalive.
//! 4. **Teardown triggers on dropping every *handle*, not the endpoint.**
//!    The connection lives as long as any handle to it does, and ends when
//!    the last one is dropped — the opposite of the obvious guess, and
//!    sensitive to the order your values fall out of scope.
//! 5. **Messages and streams do not mix on one connection.** Using
//!    `send_message` alongside `open_uni` on the same connection is a
//!    programming error with a defined, loud failure. The safe and unsafe
//!    shapes look alike at the call site, which is exactly why it is
//!    written down.
//!
//! # Modules
//!
//! - [`constants`] — every named constant the spec fixes, one home, with
//!   the derived ones re-derived as compile-time assertions.
//! - [`error`] — the closed error taxonomy of §18.1, plus `ConfigError`.
//!   Its ten types are re-exported at the crate root.
//! - [`shell`] — the I/O shell. Slice by slice it grows the driver and the
//!   handles; today it carries [`shell::wire::Wire`], the datagram seam an
//!   application supplies.
//! - `testutil` — the in-memory `Network` / `FlakyWire` / `FlakyPolicy`
//!   fabric and the counting DH provider, behind the `test-util` feature.
//!   Attested surface (ruling 60), not a test convention: it is
//!   deterministic under a caller-supplied seed, and renaming one of those
//!   three types is a protocol revision.
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
//! | `tower` | a `tower::Service` shape over the message verb |

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod constants;
pub mod error;
pub mod shell;
pub(crate) mod varint;

#[cfg(any(test, feature = "test-util"))]
pub mod testutil;

pub use error::{
    AcceptError, AuthError, ConfigError, ConnectError, ConnectionLost, DatagramError, IntroError,
    MessageError, ReadError, WriteError,
};

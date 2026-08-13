//! `slither` — a WireGuard-shaped Noise-over-UDP packet layer.
//!
//! slither carries an **authenticated, encrypted, unreliable** datagram session
//! between two peers over UDP. It borrows WireGuard's homework — a cheap mac1
//! DoS gate, fresh-ephemeral handshake retransmission, an anti-replay sliding
//! window, endpoint roaming, and the keepalive/liveness/rekey timers — but the
//! cryptography is Bubble's: the Noise **IK** handshake over
//! **P-256 / ChaCha20-Poly1305 / BLAKE2b** ([`SlitherChannel`]) from `hiss`, and
//! keyed **BLAKE2b** for mac1 from `cryptoxide` directly.
//!
//! # Delivery semantics — read this first
//!
//! The sealed **packet layer** (Leg 1, ratified) is a datagram: a packet may
//! be lost or reordered, and the only per-packet guarantees are
//! confidentiality, authenticity, and exactly-once acceptance of each counter
//! within a session (the replay window suppresses duplicates).
//!
//! The **frame layer** (Leg 2, ratified) rides inside the sealed plaintext
//! ([`frame`]) and adds **reliable, unordered, exactly-once messages** on top:
//! a [`SessionHandle::send`](endpoint::SessionHandle::send) becomes a
//! sequence-numbered DATA frame, retransmitted (RFC 9002 loss detection and
//! PTO, the crate-internal `recovery` module) on fresh counters until ACKed,
//! and surfaced to the peer exactly once. Messages are independent (no ordering, no head-of-line
//! blocking); ordered streams, fragmentation, and congestion control are
//! deliberately out — reserved frame space. Reliability lives within the
//! connection: what a dead connection had not delivered is lost.
//!
//! # Shape
//!
//! A single [`Endpoint`](endpoint::Endpoint) owns one UDP socket (behind the
//! [`Wire`](endpoint::Wire) trait, so the whole protocol is drivable without a
//! real socket) and runs as a `!Send` single-actor task, like the island
//! transport in `bubble-client`.
//! [`connect`](endpoint::Endpoint::connect) opens a session as the initiator;
//! inbound handshakes are gated by an **allow-list** of permitted remote statics
//! (the family-devices set; the caller owns policy). Session lifecycle and
//! inbound payloads surface on an [`Event`](endpoint::Event) stream.
//!
//! # The wire (ratified 2026/07/16)
//!
//! Every constant below is frozen in `slither/SPEC.md`; the code must match
//! the spec. See [`wire`] for the byte layouts.
//!
//! # Independence
//!
//! slither is its own crate with **zero `bubble-*` dependencies** — everything
//! it needs resolves from crates.io (`hiss`, `cryptoxide`, `packtool`,
//! `tokio`), and nothing here assumes a host beyond a current-thread tokio
//! runtime to run the endpoint actor on.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod endpoint;
pub mod frame;
pub mod handshake;
pub mod mac;
pub(crate) mod recovery;
pub mod session;
pub mod wire;

#[cfg(any(test, feature = "test-util"))]
pub mod testutil;

#[cfg(test)]
mod flow;
#[cfg(test)]
mod flow_frames;

/// The pinned Noise protocol for a slither session:
/// **`Noise_IK_P256_ChaChaPoly_BLAKE2b`**.
///
/// The type is the [`hiss::noise!`]-generated pattern in [`handshake`] — the
/// pattern is named `IK` deliberately, because the pattern name is part of the
/// Noise protocol identity hashed into every transcript. The dialling peer is
/// the **initiator**; the accepting peer the responder. The responder's static
/// is pre-known to the initiator (the IK `<- s` pre-message, supplied to
/// [`connect`](endpoint::Endpoint::connect) as the remote static). All Noise
/// runs through `hiss`. Pinned by `handshake::tests::protocol_name_is_pinned`.
///
/// slither v1 is **plain IK** — no PSK. First-contact secrecy gating (an
/// `IKpsk1` variant) is deliberately out of scope for Leg 1.
pub type SlitherChannel = handshake::IK;

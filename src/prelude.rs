//! The golden path in one import.
//!
//! ```
//! use slither::prelude::*;
//! ```
//!
//! This module carries the bare-minimal surface a working slither program
//! needs and nothing else: the runtime helper, [`Config`], the identity
//! seam ([`Identity`], [`SoftwareIdentity`]), the datagram seam
//! ([`Wire`]), the handles, the staged-accept ladder, the stream types,
//! and [`Notification`]. It is a glob-import surface, so it is deliberately
//! small.
//!
//! Two of those are **traits** — [`Identity`] to call `public_static()`,
//! [`Wire`] to implement a transport — and a trait must be in scope to be
//! used at all. That is what a prelude is for, and why slither has one
//! where a type-only crate would not.
//!
//! Everything else keeps **one blessed spelling at its module**:
//! `slither::error::ReadError`, `slither::config::WallClock`,
//! `slither::packet::Channel`, `slither::compat::*` for the adapters.
//! Bare-minimal code never names an error type — `?` into
//! `Box<dyn std::error::Error>` covers the quickstart — and
//! `slither::error::ReadError` at a match site reads better than a
//! glob-imported bare name.
//!
//! # The hiss types
//!
//! [`P256`], [`ChaChaPoly`], [`AesGcm`], [`Blake2b`] and [`Psk`] are
//! hiss's, folded in so the one `use` line also covers the
//! [`channel!`](crate::channel) suite declaration. `ChaChaPoly` is the
//! reference suite's cipher and the portable default; `AesGcm` is the
//! offered alternative for targets whose hardware carries it — see
//! [`packet::suite`](crate::packet::suite) for the platform reality. **Your crate must still depend on `hiss` directly** — the
//! macro expands to absolute `::hiss::…` paths, which this re-export does
//! not satisfy; see [`slither::hiss`](crate::hiss) for why.
//!
//! # Growth policy
//!
//! **[RATIFIED 2026/08/19 — ruling 278]** This module changes only by
//! ruling. A name added to a glob-visible module can collide with a
//! downstream identifier, so an addition here is an API event, not a
//! convenience edit. *(Exercised 2026/08/20 — ruling 279 added
//! [`AesGcm`]; 2026/08/23 — ruling 280 added [`Psk`].)*
//!
//! [`Psk`] is here on 278's own justification: a
//! [`channel_psk!`](crate::channel_psk) consumer cannot call
//! [`Endpoint::connect_with`](crate::shell::Endpoint::connect_with)
//! without naming the type, so leaving it out would mean the prelude
//! covers one of the two patterns. Collision surface considered — the
//! name is short, but it is also the Noise specification's own term for
//! the thing, and a downstream `Psk` would be the same concept.

pub use crate::compat::block_on;
pub use crate::config::Config;
pub use crate::identity::{Identity, SoftwareIdentity};
pub use crate::shell::wire::Wire;
pub use crate::shell::{
    BiStream, Claimed, Connecting, Connection, Endpoint, EndpointBuilder, Intro, Notification,
    Proven, RecvStream, SendStream,
};
pub use hiss::noise::{AesGcm, Blake2b, ChaChaPoly, P256};
pub use hiss::psk::Psk;

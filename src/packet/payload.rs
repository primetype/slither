//! §5.2 — msg1's 12-byte encrypted payload.
//!
//! ```text
//! msg1 payload (12 B, encrypted in msg1's tail):
//!     ts_secs(8, BE) ‖ ts_nanos(4, BE)
//! msg2: no payload (its encrypted tail is the empty payload's tag alone)
//! ```
//!
//! # This file is big-endian, and every other byte in `packet` is not
//!
//! §3.1 makes the packet **header** little-endian (ruling 64), and it
//! reaches exactly three fields — `sender_index`, `receiver_index` and
//! `counter`. This timestamp is not one of them: it is a payload, it
//! rides inside msg1's AEAD-sealed tail, and §3.1 names it as the second
//! of the rule's three stated exclusions. It is big-endian
//! **deliberately** — §5.3's strictly-greater test is an ordering, and a
//! big-endian `ts_secs` orders correctly compared as an octet string.
//!
//! Unlike §8.1's varints, this one genuinely *is* observable beside a
//! little-endian header: the responder decrypts msg1 while still holding
//! the header bytes. That is why the byte order lives in its own file
//! with its own heading, and why this module is the only place in
//! `src/packet/` where an integer's octet order is written by hand.
//!
//! # What is not here
//!
//! No clock read, no monotonic forcing, no orphan cap, no `SystemTime`.
//! §5.3's strictly-greater rule and §17.1's guard both consume this codec
//! and neither belongs to it; the protocol's one wall-clock read (§16.5)
//! is not made in the packet layer. This module encodes and decodes, and
//! that is all it does.

use crate::constants;

/// The 12-byte payload carried in msg1's encrypted tail. §5.2.
///
/// **Unattested name** — §5.2 names the payload, not a type.
///
/// [`Ord`] is derived over `(secs, nanos)` in that field order, which is
/// chronological order: §5.3's strictly-greater rule and §17.1's guard
/// both compare timestamps, and this is the comparison they need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Msg1Payload {
    /// Seconds since the Unix epoch. **Big-endian on the wire.**
    pub(crate) secs: u64,
    /// The sub-second remainder, in nanoseconds. **Big-endian.**
    pub(crate) nanos: u32,
}

impl Msg1Payload {
    /// A payload from a wall-clock reading taken elsewhere.
    pub(crate) const fn new(secs: u64, nanos: u32) -> Self {
        Self { secs, nanos }
    }

    /// `ts_secs(8, BE) ‖ ts_nanos(4, BE)`.
    pub(crate) const fn encode(&self) -> [u8; constants::MSG1_PAYLOAD_LEN] {
        let s = self.secs.to_be_bytes();
        let n = self.nanos.to_be_bytes();

        [
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], n[0], n[1], n[2], n[3],
        ]
    }

    /// The inverse of [`encode`](Self::encode). Total: every 12-byte
    /// string is a payload, and nothing here rejects one.
    pub(crate) const fn decode(bytes: &[u8; constants::MSG1_PAYLOAD_LEN]) -> Self {
        let secs = u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let nanos = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

        Self { secs, nanos }
    }
}

// The payload is the timestamp and nothing else: `constants.rs` already
// asserts `MSG1_PAYLOAD_LEN == TIMESTAMP_LEN`, and `encode`'s return type
// ties this codec to the same constant. The layout below is what makes
// the two halves add up.
const _: () = assert!(
    constants::MSG1_PAYLOAD_LEN == ::core::mem::size_of::<u64>() + ::core::mem::size_of::<u32>()
);

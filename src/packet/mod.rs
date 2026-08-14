//! §2–§5 — the wire: where bytes acquire meaning.
//!
//! This module holds the suite declaration ([`suite`]), the three packet
//! headers (`header`), mac1 (`mac`), msg1's payload codec (`payload`) and
//! §3.1's pre-AEAD classify/drop gate (`classify`). Everything but the
//! suite is crate-internal: no §16 surface exposes a header.
//!
//! Nothing here drives a handshake, holds a key, opens a session, or reads
//! a clock. No hiss state machine is stepped, no DH is performed, no index
//! is minted, no timestamp guard is consulted, no frame is parsed. If a
//! file in this module needs to know what a *connection* is, something has
//! gone in the wrong place.
//!
//! # The gate is the crate's only silent-drop tier
//!
//! §3.1: a datagram outside its type's accepted length, longer than
//! [`MAX_DATAGRAM`](crate::constants::MAX_DATAGRAM), or bearing an unknown
//! type or version is **silently dropped before any further work**. A
//! packet that fails here may genuinely be corruption, so nothing is
//! signalled. A packet that passes the AEAD and then fails structurally is
//! a peer bug or an attack, and gets a signalled death (§8.2).
//!
//! "Silently" is complete: no error, no `tracing` event, no counter, no
//! observability of any kind. §18.2's five trace targets are
//! operator-visible contract in which renaming or dropping one is a
//! protocol revision — which makes **adding** one a protocol revision too,
//! and the gate is not among them (ruling 67). A drop counter is exactly
//! the sort of thing that gets added helpfully here and questioned by
//! nobody.

// Slice 1 is the wire and nothing consumes it yet: the endpoint core that
// calls `classify`, derives a `Mac1Key` and builds a header arrives with
// the next slice. Until then every item below is dead to the compiler
// while being exactly the surface the spec asks for. This allow comes off
// when `core` lands.
#![allow(dead_code)]

pub mod suite;

pub(crate) mod header;
pub(crate) mod mac;
pub(crate) mod payload;

#[cfg(test)]
mod golden_vectors;
#[cfg(test)]
mod tests;

use packtool::{Packed, View};

use crate::constants;

// The wire types are crate-internal: no §16 surface exposes a header, a
// mac1 key or a payload codec. Start closed — widening later is not a
// breaking change and narrowing is.
pub(crate) use self::header::{DataHeader, InitHeader, RespHeader};
// Same reason as the `dead_code` allow above: nothing consumes these two
// yet, and the module they live in is the wire, not the consumer.
#[allow(unused_imports)]
pub(crate) use self::{mac::Mac1Key, payload::Msg1Payload};

pub use self::suite::{Channel, ReferenceSuite};

/// A datagram that survived §3.1's gate.
///
/// Borrowed throughout: every slice points into the caller's datagram, so
/// the gate allocates nothing and copies nothing. It is the cheapest thing
/// in the crate and must stay that way.
///
/// **`ad` and `preimage` are the received bytes, verbatim.** §3.4 makes
/// the 14 header bytes the AEAD associated data "verbatim" and §4.1 makes
/// mac1's preimage "all packet bytes preceding the tag"; both are taken as
/// subslices here rather than rebuilt by re-encoding a decoded header. A
/// re-encode would be correct today and is a place to be wrong forever.
///
/// **Unattested name** — §3.1 describes the gate's behaviour and names no
/// result type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Inbound<'a> {
    /// A HandshakeInit of exactly `C::INIT_PACKET_LEN` bytes. §3.2.
    Init {
        /// The decoded 6-byte header.
        header: InitHeader,
        /// hiss's IK msg1, between the header and mac1.
        msg1: &'a [u8],
        /// mac1's preimage: the datagram less its trailing
        /// [`MAC1_LEN`](crate::constants::MAC1_LEN) bytes — header **and**
        /// msg1, tag excluded.
        preimage: &'a [u8],
        /// The trailing mac1 tag. Not verified here: mac1 is a separate
        /// step on a separate key, and this function must stay usable by a
        /// caller that holds no static at all.
        mac1: &'a [u8],
    },
    /// A HandshakeResp of exactly `C::RESP_PACKET_LEN` bytes. §3.3.
    Resp {
        /// The decoded 10-byte header.
        header: RespHeader,
        /// hiss's IK msg2, between the header and mac1.
        msg2: &'a [u8],
        /// mac1's preimage: the datagram less its trailing
        /// [`MAC1_LEN`](crate::constants::MAC1_LEN) bytes.
        preimage: &'a [u8],
        /// The trailing mac1 tag.
        mac1: &'a [u8],
    },
    /// A Data packet, `DATA_HEADER_LEN + AEAD_TAG_LEN ..= MAX_DATAGRAM`
    /// bytes. §3.4. Carries no mac1 — a mac1 check on the data path would
    /// be a wire-visible invention.
    Data {
        /// The decoded 14-byte header.
        header: DataHeader,
        /// The AEAD associated data: the leading
        /// [`DATA_HEADER_LEN`](crate::constants::DATA_HEADER_LEN) bytes of
        /// the datagram, verbatim.
        ad: &'a [u8],
        /// The sealed plaintext with its trailing tag. At least
        /// [`AEAD_TAG_LEN`](crate::constants::AEAD_TAG_LEN) bytes; exactly
        /// that for §3.4's empty-plaintext keepalive.
        ciphertext: &'a [u8],
    },
}

/// §3.1's pre-AEAD gate: classify a received datagram, or drop it.
///
/// `None` **is** the drop. It is not an error, it is not traced, it is not
/// counted, and it never reaches the application — see the module docs.
/// §18.1's error taxonomy is closed and deliberately has no variant for
/// this: a `DropReason` returned to a caller invites a caller to act on
/// it, and the whole point is that nothing acts on it.
///
/// Generic over the suite because `INIT_PACKET_LEN` and `RESP_PACKET_LEN`
/// are per-suite (§2.3) while the headers and caps are not. The gate never
/// asks "which suite is this" — there is no suite byte to ask with. It
/// asks "is this the length my suite says it is", and a mismatched-suite
/// packet dies here or at mac1, the same fate as garbage (§2.2).
///
/// **Unattested name** — §3.1 names no function.
///
/// # Order
///
/// Size cap, then the type byte, then the version byte, then the type's
/// length rule. The type must be read before the length is tested, because
/// §3.1's accepted length is a property *of the type*.
pub(crate) fn classify<C: Channel>(dgram: &[u8]) -> Option<Inbound<'_>> {
    // 1. Oversize. Type-independent, so it comes first (§3.5).
    if dgram.len() > constants::MAX_DATAGRAM {
        return None;
    }

    // 2. Too short to read what §3.1 puts at the front of every packet.
    if dgram.len() < 2 {
        return None;
    }

    // 3. The type byte. Reserved `0x04` (dead cleartext-close concept),
    //    reserved `0x05` (cookie / mac2, §19) and every `0x06..` die here
    //    on the same path as an unknown type — there is no third
    //    behaviour, nothing is logged, nothing is answered.
    let packet_type = dgram[0];
    if !matches!(
        packet_type,
        constants::PKT_HANDSHAKE_INIT | constants::PKT_HANDSHAKE_RESP | constants::PKT_DATA
    ) {
        return None;
    }

    // 4. The version byte. There is no negotiation, ever.
    if dgram[1] != constants::VERSION {
        return None;
    }

    // 5. Length, per the type's row in §3.1's table (ruling 65): exact for
    //    the two fixed-size handshake types, a range for Data.
    match packet_type {
        constants::PKT_HANDSHAKE_INIT => {
            if dgram.len() != C::INIT_PACKET_LEN {
                return None;
            }

            let (preimage, mac1) = dgram.split_at(dgram.len() - constants::MAC1_LEN);
            let header = unpack::<InitHeader>(&preimage[..constants::INIT_HEADER_LEN])?;

            Some(Inbound::Init {
                header,
                msg1: &preimage[constants::INIT_HEADER_LEN..],
                preimage,
                mac1,
            })
        }

        constants::PKT_HANDSHAKE_RESP => {
            if dgram.len() != C::RESP_PACKET_LEN {
                return None;
            }

            let (preimage, mac1) = dgram.split_at(dgram.len() - constants::MAC1_LEN);
            let header = unpack::<RespHeader>(&preimage[..constants::RESP_HEADER_LEN])?;

            Some(Inbound::Resp {
                header,
                msg2: &preimage[constants::RESP_HEADER_LEN..],
                preimage,
                mac1,
            })
        }

        constants::PKT_DATA => {
            // §3.4's empty-plaintext keepalive is exactly this minimum: a
            // tag-only ciphertext. It is not special-cased here — it is an
            // ordinary Data packet that happens to be the shortest one,
            // and telling it apart is post-AEAD work.
            if dgram.len() < constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN {
                return None;
            }

            let (ad, ciphertext) = dgram.split_at(constants::DATA_HEADER_LEN);
            let header = unpack::<DataHeader>(ad)?;

            Some(Inbound::Data {
                header,
                ad,
                ciphertext,
            })
        }

        // Unreachable: step 3 admitted exactly the three arms above.
        // Written as a drop rather than an `unreachable!()` because a
        // panic in the DoS gate is a worse failure than a dropped packet.
        _ => None,
    }
}

/// Decode a fixed-size packtool header out of an exactly-sized slice.
///
/// The `Err` is a length error only, and the gate has already excluded it,
/// so this never fails in practice — it is still written as a drop rather
/// than an `unwrap`, for the same reason as the `_` arm above.
fn unpack<T: Packed>(bytes: &[u8]) -> Option<T> {
    View::<'_, T>::try_from_slice(bytes).ok().map(View::unpack)
}

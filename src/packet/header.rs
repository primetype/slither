//! §3.2–3.4 — the three packet headers.
//!
//! Every packet opens with `type: u8, version: u8`, and **all multi-byte
//! header fields are little-endian** (§3.1, ruling 64). That is exactly
//! three fields across the whole grammar — `sender_index`,
//! `receiver_index` and `counter` — because everything else in every
//! header is a single byte.
//!
//! Little-endian is why these are `#[derive(Packed)]` structs with plain
//! `u32` / `u64` fields: packtool packs raw integers little-endian, so
//! **the derive is the encoder**. No field is an `[u8; N]`, and no
//! per-field byte-order conversion is written by hand anywhere in this
//! module — there is no site for one to be wrong at. §5.2's msg1
//! timestamp is the crate's one big-endian integer pair and it lives in
//! [`super::payload`], which is not a header.
//!
//! # The asymmetry, stated because no compile check catches it
//!
//! [`RespHeader`] carries `sender_index` **then** `receiver_index`;
//! [`DataHeader`] carries `receiver_index` **only**. A `DataHeader` whose
//! `u32` is filled from *our* index instead of the peer's routes every
//! packet to the wrong session and produces no type error.

use packtool::Packed;

use crate::constants;

/// §3.2's `type(1) ‖ version(1) ‖ sender_index(4)` — 6 bytes.
///
/// `sender_index` is the initiator's random nonzero `u32` index (§17.3).
/// **This type enforces nothing about it**: §17.3's nonzero rule is a
/// *minting* rule, and a header that rejected index 0 would have put a
/// table invariant in the packet layer.
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InitHeader {
    /// `PKT_HANDSHAKE_INIT`. Private: a settable packet-type field is a
    /// struct literal away from a packet that lies about what it is.
    packet_type: u8,
    /// `VERSION`. Private, for the same reason.
    version: u8,
    /// The initiator's session index. Little-endian on the wire.
    pub(crate) sender_index: u32,
}

impl InitHeader {
    /// The only way to build one. `packet_type` and `version` come from
    /// [`crate::constants`]; no length or byte value is re-declared here.
    pub(crate) const fn new(sender_index: u32) -> Self {
        Self {
            packet_type: constants::PKT_HANDSHAKE_INIT,
            version: constants::VERSION,
            sender_index,
        }
    }
}

/// §3.3's `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)` —
/// 10 bytes.
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RespHeader {
    /// `PKT_HANDSHAKE_RESP`.
    packet_type: u8,
    /// `VERSION`.
    version: u8,
    /// The responder's own session index. Little-endian on the wire.
    pub(crate) sender_index: u32,
    /// The initiator's index this response answers. Little-endian.
    pub(crate) receiver_index: u32,
}

impl RespHeader {
    /// The only way to build one. Note the order: **ours, then theirs.**
    pub(crate) const fn new(sender_index: u32, receiver_index: u32) -> Self {
        Self {
            packet_type: constants::PKT_HANDSHAKE_RESP,
            version: constants::VERSION,
            sender_index,
            receiver_index,
        }
    }
}

/// §3.4's `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)` — 14
/// bytes.
///
/// **These 14 bytes are the AEAD associated data, verbatim.** The receive
/// path hands the AEAD the received bytes, never a re-encode of a decoded
/// header — see [`super::Inbound::Data`].
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataHeader {
    /// `PKT_DATA`.
    packet_type: u8,
    /// `VERSION`.
    version: u8,
    /// The **recipient's** session index — theirs, not ours. Routes the
    /// packet to a session before decryption (§17.3). Little-endian.
    pub(crate) receiver_index: u32,
    /// Exactly the value the seal returned: the hiss-owned monotonic send
    /// counter, which is simultaneously the AEAD nonce, the packet number
    /// (§7.1) and the epoch selector (§7.7). Full 8 bytes, in clear, no
    /// truncation. Little-endian.
    pub(crate) counter: u64,
}

impl DataHeader {
    /// The only way to build one. The index is the **peer's**.
    // The endpoint core (slice 2a) builds the two handshake headers; the
    // Data header's builder waits for the connection core's send path
    // (§7.1, slice 4). Slice 1's module-wide `dead_code` allow came off
    // when `core` landed, so the one item still ahead of its consumer says
    // so on its own line.
    #[allow(dead_code)]
    pub(crate) const fn new(receiver_index: u32, counter: u64) -> Self {
        Self {
            packet_type: constants::PKT_DATA,
            version: constants::VERSION,
            receiver_index,
            counter,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3.5's size table, executed
// ═══════════════════════════════════════════════════════════════════════
//
// packtool's layout is the one source of each header's size; these tie it
// to the spec's table. A field added, removed, widened or narrowed fails
// the BUILD. Slice 0's assertions already tie `INIT_HEADER_LEN` into
// `INIT_PACKET_LEN` and thence into `MAX_DATAGRAM` and `MAX_PLAINTEXT`, so
// all three lengths are now pinned end-to-end from the packtool layout up
// to the MTU.

const _: () = assert!(<InitHeader as Packed>::SIZE == constants::INIT_HEADER_LEN);
const _: () = assert!(<RespHeader as Packed>::SIZE == constants::RESP_HEADER_LEN);
const _: () = assert!(<DataHeader as Packed>::SIZE == constants::DATA_HEADER_LEN);

// §3.4's AD rule depends on this and a reader should not have to
// re-derive it: header ‖ ciphertext ‖ tag exactly fills one datagram at
// the maximum plaintext.
const _: () = assert!(
    <DataHeader as Packed>::SIZE + constants::AEAD_TAG_LEN + constants::MAX_PLAINTEXT
        == constants::MAX_DATAGRAM
);

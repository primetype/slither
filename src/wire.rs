//! The slither wire format: fixed-size [`packtool`] headers, the protocol
//! constants, and packet classification.
//!
//! Every packet opens with `type: u8, version: u8`. All multi-byte integers are
//! **big-endian** — packtool packs raw integers little-endian, so each header's
//! multi-byte fields are `[u8; N]` byte arrays that slither fills with
//! [`u32::to_be_bytes`] / [`u64::to_be_bytes`] itself, keeping the wire order
//! explicit and packtool-verbatim.
//!
//! # Ratified 2026/07/16
//!
//! Every constant here is frozen in `slither/SPEC.md`; a change to either is
//! a protocol revision, not an edit.
//!
//! # Layouts (P-256 suite)
//!
//! | Packet | Bytes |
//! |---|---|
//! | HandshakeInit `0x01` | `type(1) ‖ version(1) ‖ sender_index(4) ‖ IK msg1(174) ‖ mac1(16)` = **196** |
//! | HandshakeResp `0x02` | `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4) ‖ IK msg2(81) ‖ mac1(16)` = **107** |
//! | Data `0x03` | `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)` = **14** header ‖ ciphertext |
//!
//! The Data header's 14 bytes are the AEAD associated data. An empty plaintext
//! (a 16-byte tag-only ciphertext) is the keepalive.

use packtool::{Packed, View};

use crate::SlitherChannel;

// ── Packet types (ratified) ───────────────────────────────────────────────────

/// `0x01` — HandshakeInit: the initiator's IK msg1.
pub const TYPE_HANDSHAKE_INIT: u8 = 0x01;
/// `0x02` — HandshakeResp: the responder's IK msg2.
pub const TYPE_HANDSHAKE_RESP: u8 = 0x02;
/// `0x03` — Data: a sealed transport datagram.
pub const TYPE_DATA: u8 = 0x03;

/// `0x04` — reserved (close). Never emitted; silently dropped on receive.
pub const TYPE_RESERVED_CLOSE: u8 = 0x04;
/// `0x05` — reserved (cookie reply / mac2). Never emitted; silently dropped.
pub const TYPE_RESERVED_COOKIE: u8 = 0x05;
/// `0x06` — reserved (probe ping, future hole-punch). Never emitted; dropped.
pub const TYPE_RESERVED_PROBE_PING: u8 = 0x06;
/// `0x07` — reserved (probe pong, future hole-punch). Never emitted; dropped.
pub const TYPE_RESERVED_PROBE_PONG: u8 = 0x07;

/// The protocol version byte. An unknown version is a silent drop.
pub const VERSION: u8 = 0x01;

/// The handshake prologue — a fixed constant, identical for every handshake.
/// (The initiation timestamp rides the **encrypted msg1 payload**, not the
/// prologue — see [`crate::handshake`].)
pub const PROLOGUE: &[u8] = b"slither\x01";

// ── Sizes (ratified) ──────────────────────────────────────────────────────────

/// The mac1 tag length in bytes (keyed BLAKE2b-128).
pub const MAC1_LEN: usize = 16;

/// The maximum on-wire datagram size, in bytes (headroom under a 1280-byte IPv6
/// minimum MTU less IP/UDP overhead).
pub const MAX_DATAGRAM: usize = 1200;

/// The AEAD tag length added to every Data plaintext (`ChaChaPoly` tag).
pub const AEAD_TAG_LEN: usize = SlitherChannel::TAG_SIZE;

/// The initiation timestamp's encoded length in bytes (`secs(8) ‖ nanos(4)`) —
/// the plaintext length of the msg1 application payload.
pub const TIMESTAMP_LEN: usize = 12;

/// The maximum application plaintext per Data packet, in bytes.
///
/// `MAX_DATAGRAM − DATA_HEADER_LEN − AEAD_TAG_LEN` = `1200 − 14 − 16` = `1170`.
pub const MAX_PLAINTEXT: usize = MAX_DATAGRAM - DataHeader::SIZE - AEAD_TAG_LEN;

/// The on-wire length of a hiss IK **msg1** (`-> e, es, s, ss`) for the P-256
/// suite: the plaintext ephemeral, the encrypted static, and the encrypted
/// 12-byte timestamp payload with its tag — `65 + (65 + 16) + (12 + 16)` =
/// `174`.
pub const IK_MSG1_LEN: usize = SlitherChannel::PUBLIC_KEY_SIZE
    + (SlitherChannel::PUBLIC_KEY_SIZE + AEAD_TAG_LEN)
    + (TIMESTAMP_LEN + AEAD_TAG_LEN);

/// The on-wire length of a hiss IK **msg2** (`<- e, ee, se`) for the P-256 suite:
/// the plaintext ephemeral and the empty-payload tag — `65 + 16` = `81`.
pub const IK_MSG2_LEN: usize = SlitherChannel::PUBLIC_KEY_SIZE + AEAD_TAG_LEN;

/// The full on-wire length of a HandshakeInit packet.
pub const INIT_PACKET_LEN: usize = InitHeader::SIZE + IK_MSG1_LEN + MAC1_LEN;

/// The full on-wire length of a HandshakeResp packet.
pub const RESP_PACKET_LEN: usize = RespHeader::SIZE + IK_MSG2_LEN + MAC1_LEN;

// ── The initiation timestamp ──────────────────────────────────────────────────

/// The 12-byte initiation timestamp — `secs_since_unix_epoch: u64 ‖ nanos: u32`,
/// big-endian.
///
/// It rides as the **encrypted application payload of IK msg1** (WireGuard's
/// initiation-replay defence, restored to WireGuard's shape — see
/// [`crate::handshake`]): confidential to the holder of the responder's static,
/// so a tampered payload fails msg1's tail AEAD tag and a passive observer never
/// sees the initiator's clock. The responder keeps, per initiator static, the
/// greatest timestamp accepted and rejects non-greater values.
///
/// `Ord` compares `secs` then `nanos` (declaration order), the natural
/// chronological order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    /// Whole seconds since the Unix epoch.
    pub secs: u64,
    /// The sub-second nanosecond remainder (`0..1_000_000_000`).
    pub nanos: u32,
}

impl Timestamp {
    /// The current wall-clock time as a slither timestamp.
    ///
    /// Read fresh per handshake attempt — a retransmit is a completely fresh
    /// initiation, so each carries a strictly greater timestamp (the wall clock
    /// advances between attempts). Note this reads the **real** clock, not tokio's
    /// virtual clock, so it advances even under a paused test runtime.
    pub fn now() -> Self {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            secs: since.as_secs(),
            nanos: since.subsec_nanos(),
        }
    }

    /// The 12-byte big-endian encoding (`secs(8) ‖ nanos(4)`).
    pub fn to_be_bytes(self) -> [u8; 12] {
        let mut out = [0u8; 12];
        out[..8].copy_from_slice(&self.secs.to_be_bytes());
        out[8..].copy_from_slice(&self.nanos.to_be_bytes());
        out
    }

    /// Decode the 12-byte big-endian encoding.
    pub fn from_be_bytes(bytes: [u8; 12]) -> Self {
        let secs = u64::from_be_bytes(bytes[..8].try_into().expect("8 bytes"));
        let nanos = u32::from_be_bytes(bytes[8..].try_into().expect("4 bytes"));
        Self { secs, nanos }
    }
}

// ── Headers (packtool, const-SIZE-pinned) ─────────────────────────────────────

/// The fixed prefix of a HandshakeInit packet (before the IK msg1 and mac1).
///
/// `type(1) ‖ version(1) ‖ sender_index(4)` — 6 bytes. `sender_index` is
/// `[u8; 4]` big-endian (see the module note). The initiation timestamp is
/// **not** here — it rides encrypted inside the IK msg1 payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct InitHeader {
    /// The packet type ([`TYPE_HANDSHAKE_INIT`]).
    #[packed(accessor = false)]
    pub msg_type: u8,
    /// The protocol [`VERSION`].
    #[packed(accessor = false)]
    pub version: u8,
    /// The initiator's random nonzero session index, big-endian.
    #[packed(accessor = false)]
    pub sender_index: [u8; 4],
}

impl InitHeader {
    /// Build a header from typed values.
    pub fn new(sender_index: u32) -> Self {
        Self {
            msg_type: TYPE_HANDSHAKE_INIT,
            version: VERSION,
            sender_index: sender_index.to_be_bytes(),
        }
    }

    /// The initiator's session index.
    pub fn sender_index(&self) -> u32 {
        u32::from_be_bytes(self.sender_index)
    }
}

/// The fixed prefix of a HandshakeResp packet (before the IK msg2 and mac1).
///
/// `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)` — 10 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct RespHeader {
    /// The packet type ([`TYPE_HANDSHAKE_RESP`]).
    #[packed(accessor = false)]
    pub msg_type: u8,
    /// The protocol [`VERSION`].
    #[packed(accessor = false)]
    pub version: u8,
    /// The responder's random nonzero session index, big-endian.
    #[packed(accessor = false)]
    pub sender_index: [u8; 4],
    /// The initiator's index this response answers (echoed), big-endian.
    #[packed(accessor = false)]
    pub receiver_index: [u8; 4],
}

impl RespHeader {
    /// Build a header from typed values.
    pub fn new(sender_index: u32, receiver_index: u32) -> Self {
        Self {
            msg_type: TYPE_HANDSHAKE_RESP,
            version: VERSION,
            sender_index: sender_index.to_be_bytes(),
            receiver_index: receiver_index.to_be_bytes(),
        }
    }

    /// The responder's session index.
    pub fn sender_index(&self) -> u32 {
        u32::from_be_bytes(self.sender_index)
    }

    /// The initiator index this response answers.
    pub fn receiver_index(&self) -> u32 {
        u32::from_be_bytes(self.receiver_index)
    }
}

/// The Data packet header — the 14 bytes that are also the AEAD associated data.
///
/// `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`. The `counter` is
/// exactly the value `DatagramSend::encrypt_next` returned for this ciphertext.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct DataHeader {
    /// The packet type ([`TYPE_DATA`]).
    #[packed(accessor = false)]
    pub msg_type: u8,
    /// The protocol [`VERSION`].
    #[packed(accessor = false)]
    pub version: u8,
    /// The recipient's session index (whose session this Data belongs to),
    /// big-endian.
    #[packed(accessor = false)]
    pub receiver_index: [u8; 4],
    /// The hiss-owned monotonic send counter this ciphertext was sealed under,
    /// big-endian.
    #[packed(accessor = false)]
    pub counter: [u8; 8],
}

impl DataHeader {
    /// Build a header from typed values.
    pub fn new(receiver_index: u32, counter: u64) -> Self {
        Self {
            msg_type: TYPE_DATA,
            version: VERSION,
            receiver_index: receiver_index.to_be_bytes(),
            counter: counter.to_be_bytes(),
        }
    }

    /// The recipient's session index.
    pub fn receiver_index(&self) -> u32 {
        u32::from_be_bytes(self.receiver_index)
    }

    /// The send counter this Data was sealed under.
    pub fn counter(&self) -> u64 {
        u64::from_be_bytes(self.counter)
    }

    /// The 14 header bytes, as they appear on the wire — the exact AEAD
    /// associated data.
    pub fn to_bytes(&self) -> [u8; DataHeader::SIZE] {
        let mut out = [0u8; DataHeader::SIZE];
        self.unchecked_write_to_slice(&mut out);
        out
    }

    /// Decode a Data header from the leading [`DataHeader::SIZE`] bytes of a
    /// packet, returning it with the trailing ciphertext slice.
    pub fn parse(packet: &[u8]) -> Option<(DataHeader, &[u8])> {
        let view = View::<DataHeader>::try_from_slice(packet.get(..DataHeader::SIZE)?).ok()?;
        Some((view.unpack(), &packet[DataHeader::SIZE..]))
    }
}

// Compile-time SIZE pins: if a field width or the layout ever drifts, the crate
// fails to build rather than shipping a wrong wire (test 13 asserts these too).
// INIT_PACKET_LEN is 196 as before Leg 1b: the header shed the 12 timestamp
// bytes and the msg1 tail gained exactly 12 of ciphertext — a wash.
const _: () = assert!(<InitHeader as Packed>::SIZE == 6);
const _: () = assert!(<RespHeader as Packed>::SIZE == 10);
const _: () = assert!(<DataHeader as Packed>::SIZE == 14);
const _: () = assert!(AEAD_TAG_LEN == 16);
const _: () = assert!(TIMESTAMP_LEN == 12);
const _: () = assert!(IK_MSG1_LEN == 174);
const _: () = assert!(IK_MSG2_LEN == 81);
const _: () = assert!(INIT_PACKET_LEN == 196);
const _: () = assert!(RESP_PACKET_LEN == 107);
const _: () = assert!(MAX_PLAINTEXT == 1170);

// ── Classification ────────────────────────────────────────────────────────────

/// The kind of a well-versioned inbound packet, by its `type` byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    /// A HandshakeInit (`0x01`).
    HandshakeInit,
    /// A HandshakeResp (`0x02`).
    HandshakeResp,
    /// A Data packet (`0x03`).
    Data,
}

/// Classify an inbound datagram by its first two bytes.
///
/// Returns `None` — a **silent drop** — for a too-short packet, an unknown
/// [`VERSION`], or any type that is not one slither processes (the reserved
/// `0x04..=0x07` and every other `0x08..` value). Never panics.
pub fn classify(packet: &[u8]) -> Option<PacketKind> {
    let &[msg_type, version, ..] = packet else {
        return None;
    };
    if version != VERSION {
        return None;
    }
    match msg_type {
        TYPE_HANDSHAKE_INIT => Some(PacketKind::HandshakeInit),
        TYPE_HANDSHAKE_RESP => Some(PacketKind::HandshakeResp),
        TYPE_DATA => Some(PacketKind::Data),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_sizes_are_pinned() {
        // Test 13: const-assert every packtool header SIZE.
        assert_eq!(<InitHeader as Packed>::SIZE, 6);
        assert_eq!(<RespHeader as Packed>::SIZE, 10);
        assert_eq!(<DataHeader as Packed>::SIZE, 14);
    }

    #[test]
    fn derived_sizes_are_pinned() {
        assert_eq!(AEAD_TAG_LEN, 16);
        assert_eq!(TIMESTAMP_LEN, 12);
        assert_eq!(IK_MSG1_LEN, 174);
        assert_eq!(IK_MSG2_LEN, 81);
        assert_eq!(INIT_PACKET_LEN, 196);
        assert_eq!(RESP_PACKET_LEN, 107);
        assert_eq!(MAX_PLAINTEXT, 1170);
        assert_eq!(MAX_DATAGRAM, 1200);
        assert_eq!(MAC1_LEN, 16);
    }

    #[test]
    fn constants_are_frozen() {
        assert_eq!(VERSION, 0x01);
        assert_eq!(PROLOGUE, b"slither\x01");
        assert_eq!(TYPE_HANDSHAKE_INIT, 0x01);
        assert_eq!(TYPE_HANDSHAKE_RESP, 0x02);
        assert_eq!(TYPE_DATA, 0x03);
        assert_eq!(TYPE_RESERVED_CLOSE, 0x04);
        assert_eq!(TYPE_RESERVED_COOKIE, 0x05);
        assert_eq!(TYPE_RESERVED_PROBE_PING, 0x06);
        assert_eq!(TYPE_RESERVED_PROBE_PONG, 0x07);
    }

    #[test]
    fn data_header_round_trips_big_endian() {
        let header = DataHeader::new(0x0102_0304, 0x0A0B_0C0D_0E0F_1011);
        let bytes = header.to_bytes();
        // Explicit big-endian layout: type, version, index BE, counter BE.
        assert_eq!(
            bytes,
            [
                0x03, 0x01, 0x01, 0x02, 0x03, 0x04, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11
            ]
        );
        let (parsed, rest) = DataHeader::parse(&bytes).expect("parse");
        assert_eq!(parsed, header);
        assert_eq!(parsed.receiver_index(), 0x0102_0304);
        assert_eq!(parsed.counter(), 0x0A0B_0C0D_0E0F_1011);
        assert!(rest.is_empty());
    }

    #[test]
    fn timestamp_round_trips_and_orders() {
        let a = Timestamp {
            secs: 1_700_000_000,
            nanos: 123_456_789,
        };
        assert_eq!(Timestamp::from_be_bytes(a.to_be_bytes()), a);
        let b = Timestamp {
            secs: 1_700_000_000,
            nanos: 123_456_790,
        };
        assert!(b > a);
        let c = Timestamp {
            secs: 1_700_000_001,
            nanos: 0,
        };
        assert!(c > b);
    }

    #[test]
    fn classify_drops_unknown_version_type_and_short() {
        // Test 12 (unit slice): unknown version / type / reserved / short.
        assert_eq!(
            classify(&[TYPE_DATA, VERSION, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]),
            Some(PacketKind::Data)
        );
        assert_eq!(
            classify(&[TYPE_HANDSHAKE_INIT, VERSION]),
            Some(PacketKind::HandshakeInit)
        );
        assert_eq!(
            classify(&[TYPE_HANDSHAKE_RESP, VERSION]),
            Some(PacketKind::HandshakeResp)
        );
        // Unknown version.
        assert_eq!(classify(&[TYPE_DATA, 0x02]), None);
        // Reserved and unknown types.
        assert_eq!(classify(&[TYPE_RESERVED_CLOSE, VERSION]), None);
        assert_eq!(classify(&[TYPE_RESERVED_COOKIE, VERSION]), None);
        assert_eq!(classify(&[TYPE_RESERVED_PROBE_PING, VERSION]), None);
        assert_eq!(classify(&[TYPE_RESERVED_PROBE_PONG, VERSION]), None);
        assert_eq!(classify(&[0x08, VERSION]), None);
        assert_eq!(classify(&[0xFF, VERSION]), None);
        // Too short.
        assert_eq!(classify(&[]), None);
        assert_eq!(classify(&[TYPE_DATA]), None);
    }
}

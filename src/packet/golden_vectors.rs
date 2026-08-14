//! Golden wire vectors — wire version 1. **Data only.**
//!
//! `SPEC.md` Appendix B freezes these **"for the first time at wire version
//! 1, then held byte-identical"**. There is no external source of truth for
//! slither's wire: whatever this file says the wire *is*, it is. That makes
//! the provenance of every byte below the only thing standing between a
//! misreading and a permanently wrong protocol.
//!
//! # Provenance — read this before changing a byte
//!
//! Every value here comes from a reading that **never saw
//! `src/packet/`**. Nothing in this file was snapshot from the
//! implementation it validates; a vector taken from the code it checks
//! proves only that the code equals itself.
//!
//! | Group | Source |
//! |---|---|
//! | Offsets, lengths, both constant bytes per header, all LE integer encodings | `.slices/01-packets/DERIVATION.md`, derived from `SPEC.md` text alone |
//! | `PROLOGUE`, `MAC1_LABEL` | the same derivation, byte-expanded from the spec's ASCII literals |
//! | `canonical_static::BYTES` | the **P-256 base point** in SEC1 uncompressed form, from SEC 2 / FIPS 186-4 — a published standard any reader can check |
//! | `mac1_init::TAG`, `mac1_resp::TAG` | computed with **Python `hashlib.blake2b`**, an implementation sharing no code with `cryptoxide` |
//! | `sizes::*` | independent literals, deliberately not re-exports of [`crate::constants`] — a vector that reads the constant it checks is vacuous |
//!
//! The little-endian header values reuse the derivation's own worked
//! examples (`0x0A0B0C0D → 0D 0C 0B 0A`, `counter = 0x0102030405060708 →
//! 08 07 06 05 04 03 02 01`) rather than fresh inventions, so a reader can
//! match this file against that document line for line.
//!
//! # Two traps these vectors are shaped to catch
//!
//! `VERSION` and `PKT_HANDSHAKE_INIT` are **both `0x01`**, so a
//! HandshakeInit opens `01 01` and a `type`/`version` transposition is
//! invisible in any Init-only vector. HandshakeResp (`02 01`) and Data
//! (`03 01`) are therefore pinned too, and [`resp_header`] uses **two
//! distinct asymmetric indices** so a `sender`/`receiver` swap cannot hide.
//! No value is palindromic where byte order is the thing under test.
//!
//! # The rule this file carries, so a contributor meets it before the diff
//!
//! **Nobody edits these vectors to make a test pass.** A disagreement
//! between a vector and the implementation is triaged into exactly one of
//! three buckets, written down before anything changes:
//!
//! * the implementation is wrong ⇒ fix the implementation;
//! * the derivation is wrong ⇒ the derivation is corrected, citing the
//!   spec line, and states what it misread;
//! * the spec is ambiguous ⇒ **stop, and ask for a ruling.**
//!
//! `CLAUDE.md` states the standing version: *"any change that moves a wire
//! byte turns a test red. That is by design — treat such a red as 'this
//! needs a ruling', not 'update the expectation.'"*

/// §5.1's `PROLOGUE = b"slither\x01"`, byte-expanded.
///
/// Never appears in a datagram: it binds the wire version into the Noise
/// transcript, so a version mismatch fails the handshake *cryptographically*
/// rather than being dropped. Pinned here because Appendix B names it.
pub(crate) mod prologue {
    /// `73 6C 69 74 68 65 72 01` — `s l i t h e r \x01`.
    pub(crate) const BYTES: [u8; 8] = [0x73, 0x6C, 0x69, 0x74, 0x68, 0x65, 0x72, 0x01];
}

/// §4.1's `MAC1_LABEL = b"slither mac1"`, byte-expanded.
pub(crate) mod mac1_label {
    /// `73 6C 69 74 68 65 72 20 6D 61 63 31` — `s l i t h e r ␠ m a c 1`.
    pub(crate) const BYTES: [u8; 12] = [
        0x73, 0x6C, 0x69, 0x74, 0x68, 0x65, 0x72, 0x20, 0x6D, 0x61, 0x63, 0x31,
    ];
}

/// §2.4's canonical static encoding — the 65-byte uncompressed SEC1 form.
///
/// This is the **P-256 base point** `G` (`0x04 ‖ X ‖ Y`), taken from SEC 2 /
/// FIPS 186-4. It is used because it is a *published* 65-byte point: any
/// reader can verify these bytes against a standard without trusting
/// slither, this file, or the agent that wrote it.
///
/// mac1 keys on the **octets** and never parses them (§4.1 hashes the
/// encoding), so validity as a curve point is not required here — it is
/// chosen anyway, so the vector stays usable when a later slice needs a
/// real static.
pub(crate) mod canonical_static {
    /// `04 ‖ G.x ‖ G.y`, 65 bytes.
    pub(crate) const BYTES: [u8; 65] = [
        0x04, 0x6B, 0x17, 0xD1, 0xF2, 0xE1, 0x2C, 0x42, 0x47, 0xF8, 0xBC, 0xE6, 0xE5, 0x63, 0xA4,
        0x40, 0xF2, 0x77, 0x03, 0x7D, 0x81, 0x2D, 0xEB, 0x33, 0xA0, 0xF4, 0xA1, 0x39, 0x45, 0xD8,
        0x98, 0xC2, 0x96, 0x4F, 0xE3, 0x42, 0xE2, 0xFE, 0x1A, 0x7F, 0x9B, 0x8E, 0xE7, 0xEB, 0x4A,
        0x7C, 0x0F, 0x9E, 0x16, 0x2B, 0xCE, 0x33, 0x57, 0x6B, 0x31, 0x5E, 0xCE, 0xCB, 0xB6, 0x40,
        0x68, 0x37, 0xBF, 0x51, 0xF5,
    ];
}

/// §3.2's `InitHeader` — `type(1) ‖ version(1) ‖ sender_index(4 LE)`.
pub(crate) mod init_header {
    /// The initiator's session index.
    pub(crate) const SENDER_INDEX: u32 = 0x0A0B0C0D;

    /// `01 01 0D 0C 0B 0A`.
    ///
    /// Note the leading `01 01`: `PKT_HANDSHAKE_INIT` and `VERSION` are the
    /// same byte, which is why [`super::resp_header`] and
    /// [`super::data_header`] exist as vectors at all.
    pub(crate) const BYTES: [u8; 6] = [0x01, 0x01, 0x0D, 0x0C, 0x0B, 0x0A];
}

/// §3.3's `RespHeader` — `type ‖ version ‖ sender_index(4 LE) ‖
/// receiver_index(4 LE)`.
///
/// The two indices are **deliberately different and asymmetric**: equal or
/// palindromic values would let a field swap pass.
pub(crate) mod resp_header {
    /// The responder's **own** index — first on the wire.
    pub(crate) const SENDER_INDEX: u32 = 0x0A0B0C0D;
    /// The initiator's index this answers — second on the wire.
    pub(crate) const RECEIVER_INDEX: u32 = 0x11223344;

    /// `02 01 0D 0C 0B 0A 44 33 22 11`.
    pub(crate) const BYTES: [u8; 10] = [0x02, 0x01, 0x0D, 0x0C, 0x0B, 0x0A, 0x44, 0x33, 0x22, 0x11];
}

/// §3.4's `DataHeader` — `type ‖ version ‖ receiver_index(4 LE) ‖
/// counter(8 LE)`.
///
/// These 14 bytes are also the AEAD associated data, verbatim.
pub(crate) mod data_header {
    /// The **recipient's** index — theirs, not ours.
    pub(crate) const RECEIVER_INDEX: u32 = 0xAABB_CCDD;
    /// The hiss-owned monotonic counter: also the nonce, the packet number
    /// and the epoch selector. All eight bytes distinct, so a truncation or
    /// a reversal is visible.
    pub(crate) const COUNTER: u64 = 0x0102_0304_0506_0708;

    /// `03 01 DD CC BB AA 08 07 06 05 04 03 02 01`.
    pub(crate) const BYTES: [u8; 14] = [
        0x03, 0x01, 0xDD, 0xCC, 0xBB, 0xAA, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01,
    ];
}

/// §5.2's msg1 payload — `ts_secs(8, BE) ‖ ts_nanos(4, BE)`.
///
/// **Big-endian, and that is not an oversight** — it is ruling 64's third
/// stated exclusion. §5.3's timestamp test is a strictly-greater
/// *ordering*, and a big-endian `ts_secs` orders correctly when compared as
/// an octet string. This is the crate's only big-endian integer pair
/// outside `varint.rs`, which is why it has its own module and its own
/// vector.
pub(crate) mod msg1_payload {
    /// Seconds since the Unix epoch. The derivation's worked value.
    pub(crate) const SECS: u64 = 0x0000_0000_6800_0000;
    /// Nanoseconds within the second (`168 496 141` — a realistic
    /// sub-second value, and all four bytes distinct).
    pub(crate) const NANOS: u32 = 0x0A0B_0C0D;

    /// `00 00 00 00 68 00 00 00 0A 0B 0C 0D` — big-endian throughout.
    pub(crate) const BYTES: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x68, 0x00, 0x00, 0x00, 0x0A, 0x0B, 0x0C, 0x0D,
    ];
}

/// mac1 over a HandshakeInit: preimage `[0, 180)`, tag at `[180, 196)`.
///
/// The extent is `INIT_PACKET_LEN − MAC1_LEN`, which is a *constant* only
/// because ruling 65 made the handshake length exact. Under the superseded
/// "fixed minimum" reading the tag's position moved with the received
/// length and this vector could not have been written at all.
pub(crate) mod mac1_init {
    /// `InitHeader` ‖ a deterministic 174-byte stand-in for msg1.
    ///
    /// The filler is `0, 1, 2, …` and is **not** a real Noise message: mac1
    /// hashes octets and never parses them, so this vector pins mac1's
    /// *keying and extent*, which is exactly what is hand-checkable. Real
    /// msg1 bytes are downstream of a P-256 scalar multiplication and are
    /// not derivable from the spec by anyone.
    pub(crate) const PREIMAGE: [u8; 180] = {
        let mut out = [0u8; 180];
        let mut i = 0;
        while i < 6 {
            out[i] = super::init_header::BYTES[i];
            i += 1;
        }
        let mut j = 0;
        while j < 174 {
            out[6 + j] = j as u8;
            j += 1;
        }
        out
    };

    /// `C3 6A 7C A1 98 E6 27 65 A0 18 C7 51 97 23 C9 5F`.
    ///
    /// Computed by Python `hashlib.blake2b`: `key =
    /// blake2b(MAC1_LABEL ‖ static, digest_size=32)`, then
    /// `blake2b(PREIMAGE, digest_size=16, key=key)`. Plain — no salt, no
    /// personalisation (ruling 66).
    pub(crate) const TAG: [u8; 16] = [
        0xC3, 0x6A, 0x7C, 0xA1, 0x98, 0xE6, 0x27, 0x65, 0xA0, 0x18, 0xC7, 0x51, 0x97, 0x23, 0xC9,
        0x5F,
    ];
}

/// mac1 over a HandshakeResp: preimage `[0, 91)`, tag at `[91, 107)`.
///
/// Keyed on the **initiator's** static on this side — the recipient's, per
/// §4.1. This vector reuses [`super::canonical_static`] as that recipient,
/// so it pins the extent and the construction, not the direction.
pub(crate) mod mac1_resp {
    /// `RespHeader` ‖ a deterministic 81-byte stand-in for msg2. Filler
    /// starts at `0x80` so it cannot be confused with
    /// [`super::mac1_init::PREIMAGE`]'s.
    pub(crate) const PREIMAGE: [u8; 91] = {
        let mut out = [0u8; 91];
        let mut i = 0;
        while i < 10 {
            out[i] = super::resp_header::BYTES[i];
            i += 1;
        }
        let mut j = 0;
        while j < 81 {
            out[10 + j] = (0x80 + j) as u8;
            j += 1;
        }
        out
    };

    /// `0D 8F 5F 50 8D 3D B9 FE 81 D4 0E 8A 71 1B F1 60`. Same independent
    /// computation as [`super::mac1_init::TAG`].
    pub(crate) const TAG: [u8; 16] = [
        0x0D, 0x8F, 0x5F, 0x50, 0x8D, 0x3D, 0xB9, 0xFE, 0x81, 0xD4, 0x0E, 0x8A, 0x71, 0x1B, 0xF1,
        0x60,
    ];
}

/// §2.3 / §3.5's sizes, as **independent literals**.
///
/// Deliberately not re-exports of [`crate::constants`]: a vector that reads
/// the constant it is checking asserts nothing. These are transcribed from
/// the derivation, and the test that compares them to `constants::*` is
/// what makes the two readings meet.
pub(crate) mod sizes {
    /// `type(1) ‖ version(1) ‖ sender_index(4)`.
    pub(crate) const INIT_HEADER_LEN: usize = 6;
    /// `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)`.
    pub(crate) const RESP_HEADER_LEN: usize = 10;
    /// `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`.
    pub(crate) const DATA_HEADER_LEN: usize = 14;
    /// hiss IK msg1 on the reference suite: `65 + 81 + 28`.
    pub(crate) const IK_MSG1_LEN: usize = 174;
    /// hiss IK msg2 on the reference suite: `65 + 16`.
    pub(crate) const IK_MSG2_LEN: usize = 81;
    /// `6 + 174 + 16`.
    pub(crate) const INIT_PACKET_LEN: usize = 196;
    /// `10 + 81 + 16`.
    pub(crate) const RESP_PACKET_LEN: usize = 107;
    /// keyed-BLAKE2b-**128**.
    pub(crate) const MAC1_LEN: usize = 16;
    /// ChaCha20-Poly1305.
    pub(crate) const AEAD_TAG_LEN: usize = 16;
    /// The MTU slither commits to.
    pub(crate) const MAX_DATAGRAM: usize = 1200;
    /// `1200 − 14 − 16`.
    pub(crate) const MAX_PLAINTEXT: usize = 1170;
    /// `14 + 16` — §3.4's empty-plaintext keepalive, the shortest legal
    /// Data packet.
    pub(crate) const MIN_DATA_LEN: usize = 30;
}

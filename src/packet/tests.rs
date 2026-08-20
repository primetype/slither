//! Internal-surface tests for `slither::packet` — slice 1, "Packets & the
//! gate".
//!
//! **Authorship (CLAUDE.md working rule 6, `.slices/01-packets/PLAN.md`
//! §9.1).** Written by an author who has not read `src/packet/`'s
//! implementation, from `SPEC.md` §§2-5 and §17.3 plus `PLAN.md` §§2-7 (the
//! declared API surface — names and signatures, no bytes, no offsets)
//! alone. Slice 1 is pure wire transcription: a wrong byte order, a
//! swapped field, or a mac1 preimage one byte short all round-trip
//! perfectly against themselves and pass any test their own author writes.
//! The tests below deliberately assert byte positions and byte order
//! explicitly rather than only round-tripping, per the brief.
//!
//! Every `golden_vectors::*` path referenced here is `PLAN.md` §8.2's own
//! **indicative** schema, transcribed as given (module names
//! `init_header`, `resp_header`, `data_header`, `msg1_payload`,
//! `mac1_init`, `mac1_resp`, `prologue`, `mac1_label`, `sizes`, plus
//! `canonical_static` and `mac1_key` for §8.3 properties 8 and 10). The
//! plan states the derivation agent may rename; `src/packet/` is off
//! limits to this file's author for the whole task, including
//! `golden_vectors.rs` itself, so this schema could not be confirmed
//! against what was actually written. A compile error here pointing at a
//! `golden_vectors::` path is that reconciliation surfacing, not a bug in
//! this file's reasoning.

use super::header::{DataHeader, InitHeader, RespHeader};
use super::mac::Mac1Key;
use super::payload::Msg1Payload;
use super::suite::ReferenceSuite;
use super::{Channel, Inbound, classify, golden_vectors};
use crate::constants;
use packtool::{Packed, Packet, View};

// ═══════════════════════════════════════════════════════════════════════
// Helpers — syntactically well-formed, cryptographically meaningless
// datagrams. `classify` never validates content (§6.6: "not verify mac1"),
// only type/version/length, so a fixed non-zero filler pattern is enough.
// ═══════════════════════════════════════════════════════════════════════

fn init_datagram(version: u8) -> Vec<u8> {
    let mut dgram = vec![0xABu8; constants::INIT_PACKET_LEN];
    dgram[0] = constants::PKT_HANDSHAKE_INIT;
    dgram[1] = version;
    dgram
}

fn resp_datagram(version: u8) -> Vec<u8> {
    let mut dgram = vec![0xCDu8; constants::RESP_PACKET_LEN];
    dgram[0] = constants::PKT_HANDSHAKE_RESP;
    dgram[1] = version;
    dgram
}

fn data_datagram(version: u8, len: usize) -> Vec<u8> {
    let mut dgram = vec![0xEFu8; len];
    dgram[0] = constants::PKT_DATA;
    dgram[1] = version;
    dgram
}

fn min_data_len() -> usize {
    constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN
}

// ═══════════════════════════════════════════════════════════════════════
// The gate (§3.1)
// ═══════════════════════════════════════════════════════════════════════

/// Every byte `0x00..=0xff` except `VERSION`, over all three well-formed
/// packet shapes, must be dropped. §3.1: "unknown [version] ⇒ silent
/// drop."
#[test]
fn unknown_version_is_dropped_silently() {
    for version in 0u8..=0xFF {
        if version == constants::VERSION {
            continue;
        }
        assert!(
            classify::<ReferenceSuite>(&init_datagram(version)).is_none(),
            "Init at version {version:#04x} was not dropped"
        );
        assert!(
            classify::<ReferenceSuite>(&resp_datagram(version)).is_none(),
            "Resp at version {version:#04x} was not dropped"
        );
        assert!(
            classify::<ReferenceSuite>(&data_datagram(version, min_data_len())).is_none(),
            "Data at version {version:#04x} was not dropped"
        );
    }

    // Sanity: the loop above is not vacuously true — the correct version
    // at each shape does pass the gate.
    assert!(classify::<ReferenceSuite>(&init_datagram(constants::VERSION)).is_some());
    assert!(classify::<ReferenceSuite>(&resp_datagram(constants::VERSION)).is_some());
    assert!(
        classify::<ReferenceSuite>(&data_datagram(constants::VERSION, min_data_len())).is_some()
    );
}

/// `0x00`, `0x04`, `0x05` and `0x06..=0xff` all take the same silent-drop
/// path as an unknown type — "there is no third behaviour" (§9.3 item 2).
/// Swept at each type's own would-be length plus several others, so a
/// length check that accidentally only runs for the three real types
/// cannot hide a reserved-type bug.
#[test]
fn unknown_and_reserved_types_are_dropped() {
    let lengths = [
        constants::INIT_PACKET_LEN,
        constants::RESP_PACKET_LEN,
        min_data_len(),
        constants::MAX_DATAGRAM,
    ];
    for type_byte in 0u8..=0xFF {
        if type_byte == constants::PKT_HANDSHAKE_INIT
            || type_byte == constants::PKT_HANDSHAKE_RESP
            || type_byte == constants::PKT_DATA
        {
            continue;
        }
        for &len in &lengths {
            let mut dgram = vec![0x5Au8; len];
            dgram[0] = type_byte;
            dgram[1] = constants::VERSION;
            assert!(
                classify::<ReferenceSuite>(&dgram).is_none(),
                "type {type_byte:#04x} at length {len} was not dropped"
            );
        }
    }
}

/// §3.5: "Oversize receive (> `MAX_DATAGRAM`) is a silent drop." The
/// boundary is inclusive: exactly `MAX_DATAGRAM` is still admissible.
#[test]
fn oversize_is_dropped() {
    assert!(
        classify::<ReferenceSuite>(&data_datagram(
            constants::VERSION,
            constants::MAX_DATAGRAM + 1
        ))
        .is_none()
    );
    assert!(
        classify::<ReferenceSuite>(&data_datagram(constants::VERSION, constants::MAX_DATAGRAM))
            .is_some()
    );
}

/// 0 bytes, 1 byte, and each type exactly one byte short of its own
/// minimum, all dropped. Ruling 65: the handshake types are exact, so
/// "one byte short" here means `INIT_PACKET_LEN - 1` / `RESP_PACKET_LEN -
/// 1`, not merely "below some floor".
#[test]
fn short_is_dropped() {
    assert!(classify::<ReferenceSuite>(&[]).is_none());
    assert!(classify::<ReferenceSuite>(&[constants::PKT_HANDSHAKE_INIT]).is_none());

    let short_init = &init_datagram(constants::VERSION)[..constants::INIT_PACKET_LEN - 1];
    assert!(classify::<ReferenceSuite>(short_init).is_none());

    let short_resp = &resp_datagram(constants::VERSION)[..constants::RESP_PACKET_LEN - 1];
    assert!(classify::<ReferenceSuite>(short_resp).is_none());

    assert!(
        classify::<ReferenceSuite>(&data_datagram(constants::VERSION, min_data_len() - 1))
            .is_none()
    );
}

/// Ruling 65: the pre-AEAD length gate is **exact** for the two
/// fixed-size handshake packets, not the superseded "shorter than a fixed
/// minimum" reading. Under that superseded reading a 197-byte
/// HandshakeInit would pass the gate — §4.1's preimage extent would stop
/// being a constant, and because mac1's key is derived from public data
/// (§4.3), anyone could pad an initiation and recompute a valid tag.
///
/// The short side of this boundary (`LEN - 1` ⇒ `None`) is already
/// covered by `short_is_dropped`, and by itself does not distinguish
/// "exact" from "minimum" — both readings drop a too-short packet. Only
/// the **over-length** side (`LEN + 1` ⇒ `None`) does: a `<` check where
/// the code should use `!=` accepts it. All three points of the boundary
/// are asserted together here, for both handshake types, so the test
/// reads as one triple rather than two unrelated cases.
#[test]
fn handshake_length_is_exact_not_a_minimum() {
    let init = init_datagram(constants::VERSION);
    let short_init = &init[..constants::INIT_PACKET_LEN - 1];
    let mut over_init = init.clone();
    over_init.push(0x99);
    assert_eq!(over_init.len(), constants::INIT_PACKET_LEN + 1);

    assert!(
        classify::<ReferenceSuite>(short_init).is_none(),
        "INIT_PACKET_LEN - 1 must be dropped"
    );
    assert!(
        classify::<ReferenceSuite>(&init).is_some(),
        "INIT_PACKET_LEN must pass"
    );
    assert!(
        classify::<ReferenceSuite>(&over_init).is_none(),
        "INIT_PACKET_LEN + 1 must be dropped (ruling 65: exact, not a minimum)"
    );

    let resp = resp_datagram(constants::VERSION);
    let short_resp = &resp[..constants::RESP_PACKET_LEN - 1];
    let mut over_resp = resp.clone();
    over_resp.push(0x99);
    assert_eq!(over_resp.len(), constants::RESP_PACKET_LEN + 1);

    assert!(
        classify::<ReferenceSuite>(short_resp).is_none(),
        "RESP_PACKET_LEN - 1 must be dropped"
    );
    assert!(
        classify::<ReferenceSuite>(&resp).is_some(),
        "RESP_PACKET_LEN must pass"
    );
    assert!(
        classify::<ReferenceSuite>(&over_resp).is_none(),
        "RESP_PACKET_LEN + 1 must be dropped (ruling 65: exact, not a minimum)"
    );
}

/// §2.2 (as amended by ruling 279): a mismatched-suite packet dies at the
/// length gate or at mac1 *when the suites differ in curve* — the branch
/// this test pins; a same-curve sibling passes both gates and dies at the
/// first AEAD open instead. This test is `classify`-only, so it
/// stays independent of any live second `Channel` even though one is now
/// available (Q-O2 was in fact adopted — see
/// `tests/spec_packet.rs`'s `a_second_curve_derives_different_sizes`,
/// which exercises a real X25519 suite and confirms these same 130 bytes
/// via `channel!`'s own arithmetic rather than by hand). The length here
/// is computed from SPEC.md §2.3's own formula with a different `PK`,
/// which is exactly the wire-level fact the story clause is about.
#[test]
fn a_mismatched_suite_dies_at_the_length_gate() {
    // A hypothetical suite over a 32-byte-public-key curve (X25519's
    // well-known size, not the reference suite's 65-byte P-256 points),
    // ChaCha20-Poly1305 held fixed (TAG = 16, suite-independent per §2.3):
    //   MSG1_LEN = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)
    //            = 32 + 48 + 28 = 108
    //   INIT_PACKET_LEN = INIT_HEADER_LEN + MSG1_LEN + MAC1_LEN
    //                   = 6 + 108 + 16 = 130
    const OTHER_SUITE_INIT_LEN: usize = 130;
    assert_ne!(OTHER_SUITE_INIT_LEN, constants::INIT_PACKET_LEN);

    let mut dgram = vec![0x33u8; OTHER_SUITE_INIT_LEN];
    dgram[0] = constants::PKT_HANDSHAKE_INIT;
    dgram[1] = constants::VERSION;
    assert!(classify::<ReferenceSuite>(&dgram).is_none());
}

/// §3.4: "An empty plaintext (16-byte tag-only ciphertext; a 30-byte
/// datagram) is the keepalive." It must survive the gate at exactly the
/// minimum, and one byte less must not.
#[test]
fn keepalive_is_the_minimum_data_packet() {
    assert!(
        classify::<ReferenceSuite>(&data_datagram(constants::VERSION, min_data_len())).is_some()
    );
    assert!(
        classify::<ReferenceSuite>(&data_datagram(constants::VERSION, min_data_len() - 1))
            .is_none()
    );
}

/// A seeded sweep of random datagrams of random length `0..=1300`. The
/// only property under test is "does not panic" — the result is always
/// `Option<Inbound>`, so there is nothing else to assert against a
/// deliberately-garbage input. No `proptest` dependency (slice 0's scope
/// fence): a fixed-seed `ChaCha20Rng` sweep is deterministic and cheap.
#[test]
fn the_gate_never_panics() {
    use rand_chacha::ChaCha20Rng;
    use rand_core::{Rng, SeedableRng};

    let mut rng = ChaCha20Rng::seed_from_u64(0x5117_4E52_0001);
    let mut buf = [0u8; 1300];
    for _ in 0..5000 {
        let len = (rng.next_u32() as usize) % (constants::MAX_DATAGRAM + 101);
        let len = len.min(buf.len());
        rng.fill_bytes(&mut buf[..len]);
        let _ = classify::<ReferenceSuite>(&buf[..len]);
    }
}

/// §3.4: "The 14 header bytes are the AEAD associated data, verbatim."
/// Checked by pointer identity against the input, not only by equality —
/// a re-encoded AD would pass an equality check today and be a permanent
/// hazard tomorrow.
#[test]
fn data_ad_is_the_leading_header_bytes_verbatim() {
    let dgram = data_datagram(constants::VERSION, min_data_len() + 5);
    match classify::<ReferenceSuite>(&dgram) {
        Some(Inbound::Data { ad, .. }) => {
            assert_eq!(ad.len(), constants::DATA_HEADER_LEN);
            assert_eq!(
                ad.as_ptr(),
                dgram.as_ptr(),
                "ad is not a borrow of dgram's own start"
            );
            assert_eq!(ad, &dgram[..constants::DATA_HEADER_LEN]);
        }
        _ => panic!("expected Inbound::Data"),
    }
}

/// §4.1: the mac1 preimage is "all packet bytes preceding the tag" — for
/// both handshake types (ruling 65's exact-length reading), `preimage` and
/// `mac1` must partition the whole datagram with no gap and no overlap,
/// and both must be borrows of the input, not copies.
#[test]
fn handshake_preimage_and_tag_partition_the_datagram() {
    let init = init_datagram(constants::VERSION);
    match classify::<ReferenceSuite>(&init) {
        Some(Inbound::Init { preimage, mac1, .. }) => {
            assert_eq!(preimage.len() + mac1.len(), init.len());
            assert_eq!(mac1.len(), constants::MAC1_LEN);
            assert_eq!(preimage.as_ptr(), init.as_ptr());
            assert_eq!(mac1.as_ptr(), init[preimage.len()..].as_ptr());
        }
        _ => panic!("expected Inbound::Init"),
    }

    let resp = resp_datagram(constants::VERSION);
    match classify::<ReferenceSuite>(&resp) {
        Some(Inbound::Resp { preimage, mac1, .. }) => {
            assert_eq!(preimage.len() + mac1.len(), resp.len());
            assert_eq!(mac1.len(), constants::MAC1_LEN);
            assert_eq!(preimage.as_ptr(), resp.as_ptr());
            assert_eq!(mac1.as_ptr(), resp[preimage.len()..].as_ptr());
        }
        _ => panic!("expected Inbound::Resp"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The headers (§3.2-3.4)
// ═══════════════════════════════════════════════════════════════════════

/// A runtime mirror of `header.rs`'s `const _: () = assert!(...)` pins
/// (§4.3), so the guarantee survives if a const assert is ever dropped.
#[test]
fn header_sizes_match_constants() {
    assert_eq!(<InitHeader as Packed>::SIZE, constants::INIT_HEADER_LEN);
    assert_eq!(<RespHeader as Packed>::SIZE, constants::RESP_HEADER_LEN);
    assert_eq!(<DataHeader as Packed>::SIZE, constants::DATA_HEADER_LEN);
    assert_eq!(
        <DataHeader as Packed>::SIZE + constants::AEAD_TAG_LEN + constants::MAX_PLAINTEXT,
        constants::MAX_DATAGRAM
    );
}

/// Pack then unpack over a sweep including `0`, `1`, `u32::MAX` and
/// `u64::MAX`/`u64::MAX - 1`. Not a byte test — the bytes are §8's golden
/// vectors.
#[test]
fn headers_round_trip() {
    for &v in &[0u32, 1, u32::MAX] {
        let packed = Packet::pack(&InitHeader::new(v));
        let bytes: &[u8] = packed.as_ref();
        let decoded = View::<InitHeader>::try_from_slice(bytes)
            .expect("well-sized slice")
            .unpack();
        assert_eq!(decoded.sender_index, v);
    }

    for &(s, r) in &[
        (0u32, 0u32),
        (1, 1),
        (u32::MAX, 0),
        (0, u32::MAX),
        (u32::MAX, u32::MAX),
    ] {
        let packed = Packet::pack(&RespHeader::new(s, r));
        let bytes: &[u8] = packed.as_ref();
        let decoded = View::<RespHeader>::try_from_slice(bytes)
            .expect("well-sized slice")
            .unpack();
        assert_eq!(decoded.sender_index, s);
        assert_eq!(decoded.receiver_index, r);
    }

    for &(rx, c) in &[
        (0u32, 0u64),
        (1, 1),
        (u32::MAX, u64::MAX - 1),
        (u32::MAX, u64::MAX),
    ] {
        let packed = Packet::pack(&DataHeader::new(rx, c));
        let bytes: &[u8] = packed.as_ref();
        let decoded = View::<DataHeader>::try_from_slice(bytes)
            .expect("well-sized slice")
            .unpack();
        assert_eq!(decoded.receiver_index, rx);
        assert_eq!(decoded.counter, c);
    }
}

/// §4.1's asymmetry: "RespHeader is `sender` then `receiver`". Distinct,
/// non-palindromic values so a field swap cannot hide behind equal inputs.
/// Stated dependency (§9.3 item 12): a *consistently* swapped encode+decode
/// passes this test and is caught only by golden vector group 4 — both
/// tests are necessary, neither alone suffices.
#[test]
fn resp_header_fields_do_not_swap() {
    const SENDER: u32 = 0x0A0B_0C0D;
    const RECEIVER: u32 = 0x1122_3344;
    assert_ne!(SENDER, RECEIVER);

    let packed = Packet::pack(&RespHeader::new(SENDER, RECEIVER));
    let bytes: &[u8] = packed.as_ref();
    let decoded = View::<RespHeader>::try_from_slice(bytes)
        .expect("well-sized slice")
        .unpack();
    assert_eq!(decoded.sender_index, SENDER);
    assert_eq!(decoded.receiver_index, RECEIVER);
}

// ═══════════════════════════════════════════════════════════════════════
// mac1 (§4)
// ═══════════════════════════════════════════════════════════════════════

/// Vector group 8, stage one alone, so a failure localises to the
/// derivation rather than the tag.
///
/// **Gap, reported rather than worked around silently (CLAUDE.md working
/// rule 1):** `PLAN.md` §5.3's `Mac1Key` sketch exposes no accessor for
/// the derived 32-byte key and no `PartialEq` on `Mac1Key` itself, so
/// stage one cannot actually be isolated through the declared API — any
/// check must go through `.tag()` or `.verify()`, both of which also
/// exercise stage two. `.verify()` is used here (rather than `.tag()`,
/// which `mac1_tag_matches_the_golden_vectors` below already covers) so
/// this is not a byte-for-byte duplicate of that test. A `#[cfg(test)]`
/// key accessor on `Mac1Key` would let a future version of this test
/// isolate stage one as the plan intends.
#[test]
fn mac1_key_matches_the_golden_vector() {
    let key = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    assert!(key.verify(
        &golden_vectors::mac1_init::PREIMAGE,
        &golden_vectors::mac1_init::TAG
    ));
    assert!(key.verify(
        &golden_vectors::mac1_resp::PREIMAGE,
        &golden_vectors::mac1_resp::TAG
    ));
}

/// Vector group 9, both preimage shapes.
#[test]
fn mac1_tag_matches_the_golden_vectors() {
    let key = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    assert_eq!(
        key.tag(&golden_vectors::mac1_init::PREIMAGE),
        golden_vectors::mac1_init::TAG
    );
    assert_eq!(
        key.tag(&golden_vectors::mac1_resp::PREIMAGE),
        golden_vectors::mac1_resp::TAG
    );
}

/// Every bit of a bounded sample of preimage positions (first 8 bytes,
/// last 8 bytes, and the midpoint), and every bit of the tag itself, must
/// be rejected by `verify`.
#[test]
fn mac1_rejects_every_single_bit_flip() {
    let key = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    let preimage = golden_vectors::mac1_init::PREIMAGE;
    let tag = key.tag(&preimage);
    assert!(
        key.verify(&preimage, &tag),
        "the unflipped baseline must itself verify"
    );

    let mut positions: Vec<usize> = (0..8).chain(preimage.len() - 8..preimage.len()).collect();
    positions.push(preimage.len() / 2);

    for pos in positions {
        for bit in 0u8..8 {
            let mut flipped = preimage;
            flipped[pos] ^= 1 << bit;
            assert!(
                !key.verify(&flipped, &tag),
                "preimage byte {pos} bit {bit} was not rejected"
            );
        }
    }

    for byte in 0..tag.len() {
        for bit in 0u8..8 {
            let mut flipped = tag;
            flipped[byte] ^= 1 << bit;
            assert!(
                !key.verify(&preimage, &flipped),
                "tag byte {byte} bit {bit} was not rejected"
            );
        }
    }
}

/// Ruling 4.4: mac1 is fixed keyed-BLAKE2b for **every** suite — it must
/// not follow `Channel::Hash`.
///
/// `Mac1Key::derive`/`tag` (§5.3) take no suite type parameter at all —
/// there is no `C: Channel` for a "wrong" suite's hash to leak through —
/// so the strongest guard available through the declared API is
/// structural: this test declares a second, genuinely different suite
/// (`Hash = Sha256` rather than the reference suite's `Blake2b`) via
/// `channel!`, in its own module (PLAN.md §3.5b: one `channel!` per
/// module), and shows mac1 over the same bytes is computed by exactly one
/// code path regardless of which suite is in scope — because nothing in
/// `Mac1Key`'s signature can name a `Channel` to begin with. If a future
/// "improvement" made `Mac1Key` generic over `C::Hash` (ruling 4.4 calls
/// this out by name as the change a reviewer would praise), this suite
/// declaration is exactly what such a change would need to thread
/// through.
#[test]
fn mac1_does_not_follow_the_suite_hash() {
    mod second_hash_suite {
        crate::channel! {
            pub(crate) SecondHash<hiss::curve::p256::P256, hiss::noise::cipher::ChaChaPoly, hiss::noise::hash::Sha256>;
        }
    }
    use second_hash_suite::SecondHash;

    assert_ne!(
        <SecondHash as Channel>::PROTOCOL_NAME,
        <ReferenceSuite as Channel>::PROTOCOL_NAME,
        "varying only the Hash must still produce a different protocol name"
    );
    // Hash does not enter §2.3's size arithmetic (PK and TAG do): every
    // derived size must still match the reference suite's.
    assert_eq!(
        <SecondHash as Channel>::MSG1_LEN,
        <ReferenceSuite as Channel>::MSG1_LEN
    );
    assert_eq!(
        <SecondHash as Channel>::MSG2_LEN,
        <ReferenceSuite as Channel>::MSG2_LEN
    );
    assert_eq!(
        <SecondHash as Channel>::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_eq!(
        <SecondHash as Channel>::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );

    // mac1 itself: one call, one result, unaffected by SecondHash existing.
    let key = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    assert_eq!(
        key.tag(&golden_vectors::mac1_init::PREIMAGE),
        golden_vectors::mac1_init::TAG
    );
}

/// §4.4: P-256 mac1 keying moved from the pre-release drafts' 33-byte
/// compressed form to the 65-byte uncompressed canonical form.
/// `Mac1Key::derive` takes a plain `&[u8]` (§5.3), so nothing in its
/// signature enforces the length structurally; this instead demonstrates
/// sensitivity to the full 65-byte input by comparing against an
/// (arbitrary — not a real SEC1 compressed point) 33-byte prefix of the
/// same bytes. Two different inputs producing two different tags is the
/// observable half of "the whole canonical encoding, not a 33-byte
/// prefix of it".
#[test]
fn mac1_keys_on_the_canonical_static() {
    assert_eq!(
        golden_vectors::canonical_static::BYTES.len(),
        constants::STATIC_PUBLIC_LEN
    );
    assert_eq!(constants::STATIC_PUBLIC_LEN, 65);

    let full = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    let truncated = Mac1Key::derive(&golden_vectors::canonical_static::BYTES[..33]);
    let preimage = golden_vectors::mac1_init::PREIMAGE;
    assert_ne!(full.tag(&preimage), truncated.tag(&preimage));
}

// ═══════════════════════════════════════════════════════════════════════
// The msg1 payload (§5.2)
// ═══════════════════════════════════════════════════════════════════════
//
// `Msg1Payload` is constructible via `Msg1Payload::new(secs, nanos)` (per
// the coordinator's amendment to PLAN.md §7's declared API, circulated
// during this task rather than discovered by reading `src/packet/`).

/// Vector group 11.
#[test]
fn msg1_payload_matches_the_golden_vector() {
    let payload = Msg1Payload::new(
        golden_vectors::msg1_payload::SECS,
        golden_vectors::msg1_payload::NANOS,
    );
    assert_eq!(payload.encode(), golden_vectors::msg1_payload::BYTES);
    assert_eq!(
        Msg1Payload::decode(&golden_vectors::msg1_payload::BYTES),
        payload
    );
}

/// §5.2 / §3.1's ruling-64 exception: the msg1 timestamp is big-endian,
/// deliberately, unlike the little-endian header fields. Encodes a chosen
/// `(secs, nanos)` and asserts the result is **not** the little-endian
/// encoding of the same two fields, computed independently inside this
/// test — pinning byte order without naming a byte, so it works alongside
/// the independent golden vectors rather than duplicating them. Values are
/// non-palindromic and above 256 in every byte pair, per the brief, so LE
/// and BE genuinely disagree.
#[test]
fn msg1_payload_is_big_endian_not_little() {
    const SECS: u64 = 0x0102_0304_0506_0708;
    const NANOS: u32 = 0x0910_1112;

    let encoded = Msg1Payload::new(SECS, NANOS).encode();

    let mut little_endian = [0u8; constants::MSG1_PAYLOAD_LEN];
    little_endian[..8].copy_from_slice(&SECS.to_le_bytes());
    little_endian[8..].copy_from_slice(&NANOS.to_le_bytes());
    assert_ne!(
        encoded, little_endian,
        "encode() matched the little-endian encoding of the same fields"
    );

    let mut big_endian = [0u8; constants::MSG1_PAYLOAD_LEN];
    big_endian[..8].copy_from_slice(&SECS.to_be_bytes());
    big_endian[8..].copy_from_slice(&NANOS.to_be_bytes());
    assert_eq!(encoded, big_endian);
}

/// `decode ∘ encode == id`, and the derived `Ord` agrees with `(secs,
/// nanos)` chronological order — slice 2's strictly-greater guard (§5.3)
/// and the timestamp-guard orphan cap (§17.1) both depend on this.
#[test]
fn msg1_payload_round_trips_and_orders_chronologically() {
    let payload = Msg1Payload::new(
        golden_vectors::msg1_payload::SECS,
        golden_vectors::msg1_payload::NANOS,
    );
    assert_eq!(Msg1Payload::decode(&payload.encode()), payload);

    let earlier = Msg1Payload::new(1_700_000_000, 0);
    let later_by_nanos = Msg1Payload::new(1_700_000_000, 1);
    let later_by_secs = Msg1Payload::new(1_700_000_001, 0);
    assert!(earlier < later_by_nanos);
    assert!(earlier < later_by_secs);
    assert!(later_by_nanos < later_by_secs);
}

// ═══════════════════════════════════════════════════════════════════════
// The golden harness (§8)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn golden_prologue() {
    assert_eq!(
        constants::PROLOGUE.as_slice(),
        golden_vectors::prologue::BYTES.as_slice()
    );
    assert_eq!(golden_vectors::prologue::BYTES.len(), 8);
    assert_eq!(golden_vectors::prologue::BYTES[7], constants::VERSION);
}

#[test]
fn golden_mac1_label() {
    assert_eq!(
        constants::MAC1_LABEL.as_slice(),
        golden_vectors::mac1_label::BYTES.as_slice()
    );
    assert_eq!(golden_vectors::mac1_label::BYTES.len(), 12);
}

#[test]
fn golden_init_header() {
    let packed = Packet::pack(&InitHeader::new(golden_vectors::init_header::SENDER_INDEX));
    let bytes: &[u8] = packed.as_ref();
    assert_eq!(bytes, golden_vectors::init_header::BYTES.as_slice());
    assert_eq!(bytes[0], constants::PKT_HANDSHAKE_INIT);
    assert_eq!(bytes[1], constants::VERSION);

    let decoded = View::<InitHeader>::try_from_slice(&golden_vectors::init_header::BYTES)
        .expect("well-sized slice")
        .unpack();
    assert_eq!(
        decoded.sender_index,
        golden_vectors::init_header::SENDER_INDEX
    );
}

#[test]
fn golden_resp_header() {
    let packed = Packet::pack(&RespHeader::new(
        golden_vectors::resp_header::SENDER_INDEX,
        golden_vectors::resp_header::RECEIVER_INDEX,
    ));
    let bytes: &[u8] = packed.as_ref();
    assert_eq!(bytes, golden_vectors::resp_header::BYTES.as_slice());
    assert_eq!(bytes[0], constants::PKT_HANDSHAKE_RESP);
    assert_eq!(bytes[1], constants::VERSION);

    let decoded = View::<RespHeader>::try_from_slice(&golden_vectors::resp_header::BYTES)
        .expect("well-sized slice")
        .unpack();
    assert_eq!(
        decoded.sender_index,
        golden_vectors::resp_header::SENDER_INDEX
    );
    assert_eq!(
        decoded.receiver_index,
        golden_vectors::resp_header::RECEIVER_INDEX
    );
}

#[test]
fn golden_data_header() {
    let packed = Packet::pack(&DataHeader::new(
        golden_vectors::data_header::RECEIVER_INDEX,
        golden_vectors::data_header::COUNTER,
    ));
    let bytes: &[u8] = packed.as_ref();
    assert_eq!(bytes, golden_vectors::data_header::BYTES.as_slice());
    assert_eq!(bytes[0], constants::PKT_DATA);
    assert_eq!(bytes[1], constants::VERSION);

    let decoded = View::<DataHeader>::try_from_slice(&golden_vectors::data_header::BYTES)
        .expect("well-sized slice")
        .unpack();
    assert_eq!(
        decoded.receiver_index,
        golden_vectors::data_header::RECEIVER_INDEX
    );
    assert_eq!(decoded.counter, golden_vectors::data_header::COUNTER);
}

#[test]
fn golden_msg1_payload() {
    let payload = Msg1Payload::new(
        golden_vectors::msg1_payload::SECS,
        golden_vectors::msg1_payload::NANOS,
    );
    assert_eq!(payload.encode(), golden_vectors::msg1_payload::BYTES);
    assert_eq!(
        Msg1Payload::decode(&golden_vectors::msg1_payload::BYTES),
        payload
    );
}

#[test]
fn golden_mac1() {
    let key = Mac1Key::derive(&golden_vectors::canonical_static::BYTES);
    assert_eq!(
        key.tag(&golden_vectors::mac1_init::PREIMAGE),
        golden_vectors::mac1_init::TAG
    );
    assert_eq!(
        key.tag(&golden_vectors::mac1_resp::PREIMAGE),
        golden_vectors::mac1_resp::TAG
    );
    assert_eq!(
        golden_vectors::mac1_init::PREIMAGE.len(),
        constants::INIT_PACKET_LEN - constants::MAC1_LEN
    );
    assert_eq!(
        golden_vectors::mac1_resp::PREIMAGE.len(),
        constants::RESP_PACKET_LEN - constants::MAC1_LEN
    );
}

#[test]
fn golden_canonical_static() {
    assert_eq!(
        golden_vectors::canonical_static::BYTES.len(),
        constants::STATIC_PUBLIC_LEN
    );
}

/// Vector group 12 against both `constants::*` and `<ReferenceSuite as
/// Channel>::*` — the bridge between the independently-derived vectors and
/// slice 0's already-committed literals.
#[test]
fn sizes_match_the_golden_vectors() {
    use golden_vectors::sizes as g;

    assert_eq!(g::INIT_HEADER_LEN, constants::INIT_HEADER_LEN);
    assert_eq!(g::RESP_HEADER_LEN, constants::RESP_HEADER_LEN);
    assert_eq!(g::DATA_HEADER_LEN, constants::DATA_HEADER_LEN);
    assert_eq!(g::IK_MSG1_LEN, constants::IK_MSG1_LEN);
    assert_eq!(g::IK_MSG2_LEN, constants::IK_MSG2_LEN);
    assert_eq!(g::INIT_PACKET_LEN, constants::INIT_PACKET_LEN);
    assert_eq!(g::RESP_PACKET_LEN, constants::RESP_PACKET_LEN);
    assert_eq!(g::MAC1_LEN, constants::MAC1_LEN);
    assert_eq!(g::AEAD_TAG_LEN, constants::AEAD_TAG_LEN);
    assert_eq!(g::MAX_DATAGRAM, constants::MAX_DATAGRAM);
    assert_eq!(g::MAX_PLAINTEXT, constants::MAX_PLAINTEXT);
    assert_eq!(g::MIN_DATA_LEN, min_data_len());

    assert_eq!(g::IK_MSG1_LEN, <ReferenceSuite as Channel>::MSG1_LEN);
    assert_eq!(g::IK_MSG2_LEN, <ReferenceSuite as Channel>::MSG2_LEN);
    assert_eq!(
        g::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_eq!(
        g::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );
}

// ═══════════════════════════════════════════════════════════════════════
// The ruling-64 fence
// ═══════════════════════════════════════════════════════════════════════

/// Ruling 64's third argument was that big-endian "forced every header
/// field to `[u8; N]` with `to_be_bytes`/`from_be_bytes` at each site …
/// adding a place to be wrong per field". This makes that argument
/// permanent: `to_le_bytes`/`from_le_bytes` must not appear anywhere in
/// `src/packet/` (packtool's derive is the only little-endian encoder —
/// PLAN.md §4.2 point 1), and `to_be_bytes`/`from_be_bytes` may appear
/// only in `payload.rs` (§7 — the one place a big-endian integer sits next
/// to a little-endian one). Scoped to the implementation files only
/// (`mod.rs`, `suite.rs`, `header.rs`, `mac.rs`, `payload.rs`) — this
/// file's own tests legitimately compute both byte orders inline for
/// comparison, which would otherwise be a false positive.
#[test]
fn no_hand_rolled_byte_order_outside_the_payload_codec() {
    let files: &[(&str, &str)] = &[
        ("mod.rs", include_str!("mod.rs")),
        ("suite.rs", include_str!("suite.rs")),
        ("header.rs", include_str!("header.rs")),
        ("mac.rs", include_str!("mac.rs")),
        ("payload.rs", include_str!("payload.rs")),
    ];

    for (name, src) in files {
        assert!(
            !src.contains("to_le_bytes") && !src.contains("from_le_bytes"),
            "{name} contains a hand-rolled little-endian conversion; \
             packtool's derive is the only permitted LE encoder (ruling 64)"
        );

        if *name != "payload.rs" {
            assert!(
                !src.contains("to_be_bytes") && !src.contains("from_be_bytes"),
                "{name} contains a hand-rolled big-endian conversion outside payload.rs"
            );
        }
    }
}

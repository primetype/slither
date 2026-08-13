//! mac1 — the day-one DoS gate.
//!
//! Every HandshakeInit and HandshakeResp ends with a 16-byte mac1 tag, verified
//! **before** any curve or Diffie-Hellman work. A garbage flood, or packets keyed
//! to the wrong recipient, are dropped at this cheap keyed-hash check and never
//! reach the DH provider.
//!
//! ```text
//! key = BLAKE2b-256(b"slither mac1" ‖ recipient_static_pub_compressed[33])
//! tag = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
//! ```
//!
//! The **recipient's** static public key keys the tag: on a HandshakeInit the
//! recipient is the responder, on a HandshakeResp the recipient is the initiator
//! — always the party the packet is addressed to, and always a key both peers
//! know. mac1 is therefore an anti-amplification / cheap-reject gate, **not** a
//! secret authenticator (anyone knowing the recipient's public static can
//! compute it); the real authentication is the Noise handshake underneath.
//! BLAKE2b is taken from `cryptoxide` directly (the raw-primitive rule).

use cryptoxide::blake2b::Blake2b;
use cryptoxide::digest::Digest;
use hiss::curve::p256::P256r1PublicKey;

/// The mac1 domain-separation label (frozen in `slither/SPEC.md`, ratified
/// 2026/07/16).
pub const MAC1_LABEL: &[u8] = b"slither mac1";

/// The keyed-hash output length, in bytes (BLAKE2b-128).
pub const MAC1_LEN: usize = 16;

/// The mac1 keying-hash output length, in bytes (BLAKE2b-256).
const MAC1_KEY_LEN: usize = 32;

/// The 33-byte SEC1-compressed encoding length of a recipient static — the
/// hiss `Packed` encoding of a [`P256r1PublicKey`], used verbatim as the keying
/// material.
const COMPRESSED_KEY_LEN: usize = 33;

/// Derive the mac1 key for a packet addressed to `recipient`:
/// `BLAKE2b-256(MAC1_LABEL ‖ recipient.to_compressed())`.
///
/// `to_compressed()` is the 33-byte SEC1 compressed encoding, which is exactly
/// the hiss `Packed` encoding of the key (`<P256r1PublicKey as packtool::Packed>`
/// writes those same 33 bytes).
pub fn mac1_key(recipient: &P256r1PublicKey) -> [u8; MAC1_KEY_LEN] {
    let compressed: [u8; COMPRESSED_KEY_LEN] = recipient.to_compressed();
    let mut hasher = Blake2b::new(MAC1_KEY_LEN);
    Digest::input(&mut hasher, MAC1_LABEL);
    Digest::input(&mut hasher, &compressed);
    let mut key = [0u8; MAC1_KEY_LEN];
    Digest::result(&mut hasher, &mut key);
    key
}

/// Compute the mac1 tag over `preceding` (all packet bytes before the tag) under
/// a pre-derived `key` from [`mac1_key`].
pub fn mac1_tag(key: &[u8; MAC1_KEY_LEN], preceding: &[u8]) -> [u8; MAC1_LEN] {
    let mut hasher = Blake2b::new_keyed(MAC1_LEN, key);
    Digest::input(&mut hasher, preceding);
    let mut tag = [0u8; MAC1_LEN];
    Digest::result(&mut hasher, &mut tag);
    tag
}

/// Compute the full mac1 tag for a packet addressed to `recipient`, over the
/// `preceding` bytes (everything before the tag).
pub fn compute(recipient: &P256r1PublicKey, preceding: &[u8]) -> [u8; MAC1_LEN] {
    mac1_tag(&mac1_key(recipient), preceding)
}

/// Verify that `tag` is the mac1 for a packet addressed to `recipient` over
/// `preceding`. Uses a constant-time comparison.
pub fn verify(recipient: &P256r1PublicKey, preceding: &[u8], tag: &[u8]) -> bool {
    if tag.len() != MAC1_LEN {
        return false;
    }
    let expected = compute(recipient, preceding);
    constant_time_eq(&expected, tag)
}

/// A branch-free, length-checked byte comparison (no early return on the first
/// differing byte).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiss::curve::p256::P256r1PrivateKey;
    use hiss::provider::{EphemeralOnly, ProviderExt};
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    fn key_pair(seed: u8) -> P256r1PublicKey {
        let mut provider = EphemeralOnly::new(ChaCha20Rng::from_seed([seed; 32]));
        let secret: P256r1PrivateKey = provider.generate::<hiss::noise::P256>().expect("scalar");
        provider.public(&secret).expect("public")
    }

    #[test]
    fn label_and_lengths_are_frozen() {
        assert_eq!(MAC1_LABEL, b"slither mac1");
        assert_eq!(MAC1_LEN, 16);
        assert_eq!(MAC1_KEY_LEN, 32);
    }

    #[test]
    fn tag_verifies_and_is_deterministic() {
        let recipient = key_pair(0x11);
        let msg = b"the packet bytes preceding the tag";
        let tag = compute(&recipient, msg);
        assert_eq!(tag, compute(&recipient, msg), "mac1 is deterministic");
        assert!(verify(&recipient, msg, &tag));
    }

    #[test]
    fn wrong_recipient_key_fails() {
        let recipient = key_pair(0x11);
        let other = key_pair(0x22);
        let msg = b"same message, different keying";
        let tag = compute(&recipient, msg);
        assert!(
            !verify(&other, msg, &tag),
            "a wrong-key tag must not verify"
        );
    }

    #[test]
    fn tampered_message_or_tag_fails() {
        let recipient = key_pair(0x11);
        let msg = b"authenticated preceding bytes";
        let tag = compute(&recipient, msg);
        assert!(!verify(&recipient, b"authenticated preceding byteS", &tag));
        let mut bad = tag;
        bad[0] ^= 0x01;
        assert!(!verify(&recipient, msg, &bad));
        // A wrong-length tag is a clean reject, never a panic.
        assert!(!verify(&recipient, msg, &tag[..15]));
    }
}

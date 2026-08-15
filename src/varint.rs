//! QUIC variable-length integers — `SPEC.md` §8.1, byte-identical to
//! RFC 9000 §16.
//!
//! The first two bits of the first byte give the encoded length: `00` is
//! one byte and six value bits, `01` two bytes and fourteen, `10` four and
//! thirty, `11` eight and sixty-two. The value space therefore stops at
//! 2⁶² − 1, and §8.1's stated consequence is that ACK `largest`, stream
//! offsets and final sizes all cap there.
//!
//! # Why a newtype
//!
//! [`VarInt`] makes that cap unrepresentable rather than checkable. Slices
//! that carry offsets and limits cannot construct an unencodable value at
//! all, instead of testing for one at every call site and forgetting once.
//!
//! # The asymmetry that matters
//!
//! §8.1 is asymmetric on purpose, and it is the rule most likely to be
//! implemented wrongly because the wrong answer looks safer:
//!
//! - A **sender** emits the *minimal* encoding. [`encode`] and
//!   [`encode_to`] have no other mode.
//! - A **receiver** accepts *any* valid encoding. [`decode`] therefore
//!   accepts `40 25` as 37 and reports two bytes consumed. Rejecting a
//!   non-minimal encoding would be a spec violation, not a hardening.
//!
//! # Crate-private on purpose
//!
//! No §16 surface exposes a varint, and [`encode`]'s "value exceeds
//! 2⁶² − 1" case would need a *new public error type* if this module were
//! public — which §18.1's closed taxonomy forbids. Keeping the module
//! crate-private keeps the overflow condition internal. Widening
//! `pub(crate)` to `pub` later is not a breaking change; the reverse is.

/// A QUIC variable-length integer (RFC 9000 §16), byte-identical. §8.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct VarInt(u64);

impl VarInt {
    /// `MAX` as a bare `u64`, for use in `const` assertions: 2⁶² − 1.
    pub(crate) const MAX_VALUE: u64 = (1u64 << 62) - 1;

    /// The largest representable value, 2⁶² − 1.
    pub(crate) const MAX: VarInt = VarInt(Self::MAX_VALUE);

    /// `None` if `v` exceeds [`VarInt::MAX_VALUE`].
    pub(crate) const fn new(v: u64) -> Option<VarInt> {
        if v > Self::MAX_VALUE {
            None
        } else {
            Some(VarInt(v))
        }
    }

    /// Infallible: every `u32` fits the 62-bit space.
    pub(crate) const fn from_u32(v: u32) -> VarInt {
        VarInt(v as u64)
    }

    /// Infallible constructor for literals.
    ///
    /// In `const` position an out-of-range `v` is a **compile error**;
    /// that is the intended use. Called at run time it panics, so prefer
    /// [`VarInt::new`] for any value that is not a literal.
    // Slice 3's frame codec took the module-wide `dead_code` allow off:
    // every other item here now has a caller. This one waits for the first
    // varint *literal* on the wire — §9.5's stream flags and §10.3's
    // credit grants, slices 4 and 5 — so it says so on its own line rather
    // than hiding behind a blanket allow.
    #[allow(dead_code)]
    pub(crate) const fn from_const(v: u64) -> VarInt {
        assert!(
            v <= Self::MAX_VALUE,
            "varint literal exceeds the 62-bit value space"
        );
        VarInt(v)
    }

    /// The value as a bare `u64`.
    pub(crate) const fn into_inner(self) -> u64 {
        self.0
    }

    /// The *minimal* encoded length in bytes: 1, 2, 4 or 8. §8.1's sender
    /// rule.
    pub(crate) const fn encoded_len(self) -> usize {
        if self.0 < (1 << 6) {
            1
        } else if self.0 < (1 << 14) {
            2
        } else if self.0 < (1 << 30) {
            4
        } else {
            8
        }
    }
}

impl From<VarInt> for u64 {
    fn from(v: VarInt) -> u64 {
        v.0
    }
}

impl From<u32> for VarInt {
    fn from(v: u32) -> VarInt {
        VarInt::from_u32(v)
    }
}

/// Append the minimal encoding of `v` to `out`. §8.1.
pub(crate) fn encode(v: VarInt, out: &mut Vec<u8>) {
    let mut buf = [0u8; 8];
    let n = encode_to(v, &mut buf).expect("eight bytes always suffice");
    out.extend_from_slice(&buf[..n]);
}

/// Write the minimal encoding of `v` into `out`, returning the bytes
/// written, or `None` if `out` is too short. §8.1.
///
/// On `None` **`out` is left untouched**. The frame packer relies on that:
/// §8.6 packs frames into a fixed plaintext buffer until one does not fit,
/// and a partial write would corrupt the packet rather than end it.
pub(crate) fn encode_to(v: VarInt, out: &mut [u8]) -> Option<usize> {
    let n = v.encoded_len();
    if out.len() < n {
        return None;
    }
    let x = v.0;
    match n {
        1 => out[0] = x as u8,
        2 => out[..2].copy_from_slice(&((x as u16) | 0x4000).to_be_bytes()),
        4 => out[..4].copy_from_slice(&((x as u32) | 0x8000_0000).to_be_bytes()),
        _ => out[..8].copy_from_slice(&(x | 0xc000_0000_0000_0000).to_be_bytes()),
    }
    Some(n)
}

/// Decode one varint from the front of `buf`: the value and the bytes
/// consumed. `None` on a truncated input. §8.1.
///
/// A receiver accepts **any** length, so a non-minimal encoding decodes
/// successfully and reports the length it actually occupied.
pub(crate) fn decode(buf: &[u8]) -> Option<(VarInt, usize)> {
    let first = *buf.first()?;
    let n = 1usize << (first >> 6);
    if buf.len() < n {
        return None;
    }
    // Six value bits in the first byte, eight in each byte that follows;
    // eight bytes therefore top out at 62 bits, always within MAX_VALUE.
    let mut v = u64::from(first & 0x3f);
    for byte in &buf[1..n] {
        v = (v << 8) | u64::from(*byte);
    }
    Some((VarInt(v), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four vectors in RFC 9000 Appendix A.1, plus its non-minimal
    /// example — the strongest available pin, because it is external.
    #[test]
    fn rfc_9000_appendix_a1_vectors() {
        let vectors: &[(&[u8], u64)] = &[
            (
                &[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c],
                151_288_809_941_952_652,
            ),
            (&[0x9d, 0x7f, 0x3e, 0x7d], 494_878_333),
            (&[0x7b, 0xbd], 15_293),
            (&[0x25], 37),
        ];

        for (bytes, value) in vectors {
            // Decodes to the stated value, consuming exactly its length.
            assert_eq!(
                decode(bytes),
                Some((VarInt::new(*value).unwrap(), bytes.len())),
                "decoding {bytes:02x?}"
            );
            // And is exactly what a sender emits for it.
            let mut out = Vec::new();
            encode(VarInt::new(*value).unwrap(), &mut out);
            assert_eq!(out.as_slice(), *bytes, "encoding {value}");
        }

        // The appendix's non-minimal example: `40 25` is 37 in two bytes.
        // It MUST decode (a receiver accepts any length) ...
        assert_eq!(decode(&[0x40, 0x25]), Some((VarInt::new(37).unwrap(), 2)));
        // ... and MUST NOT be what a sender produces (§8.1's minimal rule).
        let mut out = Vec::new();
        encode(VarInt::new(37).unwrap(), &mut out);
        assert_eq!(out, vec![0x25]);
    }

    #[test]
    fn boundaries_round_trip() {
        // (value, minimal length, expected two-bit prefix)
        let cases: &[(u64, usize, u8)] = &[
            (0, 1, 0b00),
            (63, 1, 0b00),
            (64, 2, 0b01),
            (16_383, 2, 0b01),
            (16_384, 4, 0b10),
            (1_073_741_823, 4, 0b10),
            (1_073_741_824, 8, 0b11),
            (VarInt::MAX_VALUE, 8, 0b11),
        ];

        for (value, len, prefix) in cases {
            let v = VarInt::new(*value).expect("in range");
            assert_eq!(v.encoded_len(), *len, "encoded_len of {value}");

            let mut out = Vec::new();
            encode(v, &mut out);
            assert_eq!(out.len(), *len, "encoded length of {value}");
            assert_eq!(out[0] >> 6, *prefix, "prefix of {value}");
            assert_eq!(decode(&out), Some((v, *len)), "round trip of {value}");
        }
    }

    #[test]
    fn above_max_is_rejected() {
        assert_eq!(VarInt::new(1u64 << 62), None);
        assert_eq!(VarInt::new(u64::MAX), None);
        assert_eq!(VarInt::new(VarInt::MAX_VALUE), Some(VarInt::MAX));
        assert_eq!(VarInt::MAX.into_inner(), VarInt::MAX_VALUE);
        assert_eq!(VarInt::MAX_VALUE, (1u64 << 62) - 1);
    }

    #[test]
    fn truncated_input_is_none() {
        assert_eq!(decode(&[]), None);
        // A `01` prefix promises two bytes; only one is available.
        assert_eq!(decode(&[0x40]), None);
        // A `10` prefix promises four; three are available.
        assert_eq!(decode(&[0x80, 0x00, 0x00]), None);
        // A `11` prefix promises eight; seven are available.
        assert_eq!(decode(&[0xc0, 0, 0, 0, 0, 0, 0]), None);
    }

    /// §8.1's receiver rule, from the side that is easy to get wrong:
    /// "reject non-minimal" is the safer-*looking* choice and the spec
    /// explicitly forbids it.
    #[test]
    fn decode_accepts_every_non_minimal_encoding_of_a_small_value() {
        let thirty_seven: &[(&[u8], usize)] = &[
            (&[0x25], 1),
            (&[0x40, 0x25], 2),
            (&[0x80, 0x00, 0x00, 0x25], 4),
            (&[0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x25], 8),
        ];

        for (bytes, consumed) in thirty_seven {
            assert_eq!(
                decode(bytes),
                Some((VarInt::new(37).unwrap(), *consumed)),
                "decoding {bytes:02x?}"
            );
        }
    }

    #[test]
    fn exhaustive_round_trip_near_boundaries() {
        let mut values: Vec<u64> = (0..=1024).collect();
        for boundary in [
            63u64,
            64,
            16_383,
            16_384,
            1_073_741_823,
            1_073_741_824,
            VarInt::MAX_VALUE,
        ] {
            for delta in 0..=4u64 {
                values.push(boundary.saturating_sub(delta));
                if let Some(v) = boundary.checked_add(delta)
                    && v <= VarInt::MAX_VALUE
                {
                    values.push(v);
                }
            }
        }

        for value in values {
            let v = VarInt::new(value).expect("in range");
            let mut out = Vec::new();
            encode(v, &mut out);
            assert_eq!(out.len(), v.encoded_len(), "length of {value}");
            assert_eq!(decode(&out), Some((v, out.len())), "round trip of {value}");
        }
    }

    /// The subtle one: a short buffer yields `None` **and leaves `out`
    /// untouched**, because the frame packer keeps packing after a frame
    /// does not fit.
    #[test]
    fn encode_to_respects_a_short_buffer() {
        // Every length class, each given one byte too few.
        let cases: &[(u64, usize)] = &[
            (37, 1),
            (16_383, 2),
            (1_073_741_823, 4),
            (VarInt::MAX_VALUE, 8),
        ];

        for (value, len) in cases {
            let v = VarInt::new(*value).expect("in range");
            let mut out = vec![0xaa; len - 1];
            assert_eq!(encode_to(v, &mut out), None, "short buffer for {value}");
            assert!(
                out.iter().all(|b| *b == 0xaa),
                "a failed encode_to wrote into the buffer for {value}"
            );

            // An exactly-sized buffer succeeds and writes the whole value.
            let mut exact = vec![0xaa; *len];
            assert_eq!(encode_to(v, &mut exact), Some(*len));
            assert_eq!(decode(&exact), Some((v, *len)));
        }

        // Zero-length buffer, the degenerate case.
        let mut empty: [u8; 0] = [];
        assert_eq!(encode_to(VarInt::new(0).unwrap(), &mut empty), None);
    }

    #[test]
    fn conversions_are_consistent() {
        assert_eq!(u64::from(VarInt::from_u32(u32::MAX)), u64::from(u32::MAX));
        assert_eq!(VarInt::from(7u32), VarInt::new(7).unwrap());
        assert_eq!(VarInt::from_const(42).into_inner(), 42);
        assert_eq!(VarInt::default(), VarInt::new(0).unwrap());
        // `from_const` in const position is the intended use.
        const LIMIT: VarInt = VarInt::from_const(1_048_576);
        assert_eq!(LIMIT.into_inner(), 1_048_576);
    }
}

//! The Leg 2 frame layer: the frames that occupy a sealed Data packet's
//! plaintext — QUIC's homework one layer down.
//!
//! A Data packet's plaintext is no longer a raw application payload but a
//! **sequence of frames**, concatenated back to back. Parsing runs to the end
//! of the plaintext — the AEAD gives the exact length, so there is no
//! packet-level length prefix; a frame that needs an explicit length (DATA)
//! carries its own. Headers are fixed-width [`packtool`] structs in the
//! `wire.rs` convention: packtool packs raw integers little-endian, so every
//! multi-byte field is a `[u8; N]` byte array slither fills with `to_be_bytes`.
//!
//! # Ratified (Leg 2, 2026/07/17)
//!
//! Every constant and layout here is frozen in `slither/SPEC.md` §9
//! (ratified 2026/07/17), the same footing as the Leg 1 tables.
//!
//! # Frame types
//!
//! | Frame | Bytes |
//! |---|---|
//! | PADDING `0x00` | `type(1)` = **1** |
//! | PING `0x01` | `type(1)` = **1** |
//! | ACK `0x02` | `type(1) ‖ largest(8) ‖ ack_delay_µs(4) ‖ first_range(2) ‖ range_count(1)` = **16** ‖ `range_count × (gap(2) ‖ length(2))` |
//! | DATA `0x03` | `type(1) ‖ seq(8) ‖ length(2)` = **11** ‖ `length` payload bytes |
//! | `0x04..=0x0F` | reserved (STREAM / file transfer / close) — never emitted; a receiver stops parsing and silently ignores the rest of the packet |
//! | `0x10..` | unknown — a protocol violation; the whole (authenticated) packet is dropped |
//!
//! A malformed frame stream fails the **whole packet**: the plaintext was
//! authenticated, so a parse error is a protocol violation, not line noise.

use std::time::Duration;

use packtool::{Packed, View};

use crate::wire::MAX_PLAINTEXT;

// ── Frame types (ratified) ────────────────────────────────────────────────────

/// `0x00` — PADDING: one zero byte, no fields; coalescible, ignorable. (A run
/// of PADDING is how a packet is padded, e.g. for a future PMTU probe.)
pub const FRAME_PADDING: u8 = 0x00;
/// `0x01` — PING: no fields; elicits an ACK, nothing else. Liveness.
pub const FRAME_PING: u8 = 0x01;
/// `0x02` — ACK: the largest received counter, an ack_delay, and a set of ACK
/// ranges (QUIC RFC 9000 §19.3 shape, fixed-width fields).
pub const FRAME_ACK: u8 = 0x02;
/// `0x03` — DATA: a reliable application message under its own sequence number
/// (the retransmittable identity, distinct from the packet counter).
pub const FRAME_DATA: u8 = 0x03;

/// `0x04` — reserved (STREAM: ordered byte streams). Never emitted.
pub const FRAME_RESERVED_STREAM: u8 = 0x04;
/// `0x05` — reserved (file transfer). Never emitted.
pub const FRAME_RESERVED_FILE: u8 = 0x05;
/// `0x06` — reserved (close). Never emitted.
pub const FRAME_RESERVED_CLOSE: u8 = 0x06;
/// `0x0F` — the top of the reserved frame-type space (`0x04..=0x0F`). A
/// receiver meeting any reserved type stops parsing and silently ignores the
/// remainder of the packet (its layout is unknowable, so nothing beyond it can
/// be framed); the frames already parsed stand. `0x10..` is a protocol
/// violation instead — the whole packet is dropped.
pub const FRAME_RESERVED_TOP: u8 = 0x0F;

// ── Sizes and caps (ratified) ─────────────────────────────────────────────────

/// The fixed per-message framing overhead of a DATA frame
/// ([`DataFrameHeader::SIZE`] = 11 bytes: `type ‖ seq(8) ‖ length(2)`).
pub const DATA_OVERHEAD: usize = DataFrameHeader::SIZE;

/// The maximum reliable application message, in bytes:
/// `MAX_PLAINTEXT − DATA_OVERHEAD` = `1170 − 11` = **1159**.
///
/// A message must fit — after framing overhead — in one sealed packet;
/// multi-packet messages (fragmentation) are OUT of Leg 2, reserved for the
/// STREAM work.
pub const MAX_MESSAGE: usize = MAX_PLAINTEXT - DATA_OVERHEAD;

/// The maximum number of ACK ranges after the first block: **63** — the most a
/// 128-counter replay window can produce (the alternating-bit worst case), so
/// no window state is ever truncated. A maximal ACK is `16 + 63 × 4` = 268
/// bytes, comfortably inside [`MAX_PLAINTEXT`].
pub const MAX_ACK_RANGES: usize = 63;

/// The wire size of a maximal ACK frame (the [`MAX_ACK_RANGES`] cap made
/// concrete): 268 bytes.
pub const MAX_ACK_WIRE: usize = AckHeader::SIZE + MAX_ACK_RANGES * AckRangePair::SIZE;

// ── Headers (packtool, const-SIZE-pinned) ─────────────────────────────────────

/// The fixed prefix of an ACK frame (before the range pairs).
///
/// `type(1) ‖ largest(8) ‖ ack_delay_µs(4) ‖ first_range(2) ‖ range_count(1)`
/// — 16 bytes, multi-byte fields big-endian (the module note).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct AckHeader {
    /// The frame type ([`FRAME_ACK`]).
    #[packed(accessor = false)]
    pub frame_type: u8,
    /// The largest received packet counter, big-endian.
    #[packed(accessor = false)]
    pub largest: [u8; 8],
    /// Microseconds between receiving `largest` and sending this ACK,
    /// big-endian (saturating at `u32::MAX`).
    #[packed(accessor = false)]
    pub ack_delay_micros: [u8; 4],
    /// The count of contiguous counters received immediately below `largest`
    /// (the first block covers `largest − first_range ..= largest`), big-endian.
    #[packed(accessor = false)]
    pub first_range: [u8; 2],
    /// The number of `(gap, length)` pairs that follow (≤ [`MAX_ACK_RANGES`]).
    #[packed(accessor = false)]
    pub range_count: u8,
}

/// One ACK range pair — QUIC RFC 9000 §19.3.1 semantics with fixed widths.
///
/// With `prev_smallest` the smallest counter of the preceding block:
/// `gap` = (length of the unreceived run below it) − 1, so the block's largest
/// is `prev_smallest − gap − 2`; `length` = (received run length) − 1, so the
/// block covers `largest − length ..= largest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct AckRangePair {
    /// The gap field, big-endian.
    #[packed(accessor = false)]
    pub gap: [u8; 2],
    /// The length field, big-endian.
    #[packed(accessor = false)]
    pub length: [u8; 2],
}

/// The fixed prefix of a DATA frame (before the message bytes).
///
/// `type(1) ‖ seq(8) ‖ length(2)` — 11 bytes. `seq` is the message's own
/// sequence number (the retransmittable identity: a retransmitted DATA keeps
/// its `seq` but rides a fresh packet counter); `length` counts the message
/// bytes that follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Packed)]
pub struct DataFrameHeader {
    /// The frame type ([`FRAME_DATA`]).
    #[packed(accessor = false)]
    pub frame_type: u8,
    /// The message sequence number, big-endian.
    #[packed(accessor = false)]
    pub seq: [u8; 8],
    /// The message length in bytes, big-endian.
    #[packed(accessor = false)]
    pub length: [u8; 2],
}

// Compile-time SIZE pins, exactly as wire.rs pins its packet headers.
const _: () = assert!(<AckHeader as Packed>::SIZE == 16);
const _: () = assert!(<AckRangePair as Packed>::SIZE == 4);
const _: () = assert!(<DataFrameHeader as Packed>::SIZE == 11);
const _: () = assert!(MAX_MESSAGE == 1159);
const _: () = assert!(MAX_ACK_WIRE == 268);
const _: () = assert!(MAX_ACK_WIRE <= MAX_PLAINTEXT);

// ── The typed frames ──────────────────────────────────────────────────────────

/// An ACK frame: the largest received counter, the delay before acknowledging
/// it, and the received ranges below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckFrame {
    /// The largest received packet counter.
    pub largest: u64,
    /// Microseconds between receiving `largest` and sending this ACK.
    pub ack_delay_micros: u32,
    /// The first block: contiguous counters received immediately below
    /// `largest` (the block covers `largest − first_range ..= largest`).
    pub first_range: u16,
    /// The further `(gap, length)` pairs, [`AckRangePair`] semantics.
    pub ranges: Vec<(u16, u16)>,
}

/// A DATA frame: one reliable application message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataFrame {
    /// The message sequence number (the retransmittable identity).
    pub seq: u64,
    /// The message bytes (≤ [`MAX_MESSAGE`]).
    pub payload: Vec<u8>,
}

/// One frame of a sealed packet's plaintext.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// PADDING (`0x00`) — one zero byte.
    Padding,
    /// PING (`0x01`) — elicits an ACK.
    Ping,
    /// ACK (`0x02`) — acknowledges received packet counters.
    Ack(AckFrame),
    /// DATA (`0x03`) — a reliable application message.
    Data(DataFrame),
}

/// A failure decoding a frame stream. The plaintext was authenticated, so any
/// of these is a protocol violation — the caller drops the whole packet.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    /// The stream ended mid-frame.
    #[error("truncated {frame} frame: needs {needed} bytes, {remaining} remain")]
    Truncated {
        /// Which frame kind was being parsed.
        frame: &'static str,
        /// The bytes the frame needed.
        needed: usize,
        /// The bytes that remained.
        remaining: usize,
    },
    /// A frame type outside both the v1 set and the reserved space.
    #[error("unknown frame type 0x{0:02X}")]
    UnknownType(u8),
    /// An ACK declared more ranges than the documented cap.
    #[error("ACK carries {count} ranges, over the {max}-range cap")]
    TooManyRanges {
        /// The declared range count.
        count: usize,
        /// The cap ([`MAX_ACK_RANGES`]).
        max: usize,
    },
    /// An ACK's ranges descend below counter zero.
    #[error("ACK ranges descend below packet counter zero")]
    AckUnderflow,
}

impl Frame {
    /// The frame's encoded length in bytes.
    pub fn wire_len(&self) -> usize {
        match self {
            Frame::Padding | Frame::Ping => 1,
            Frame::Ack(ack) => ack.wire_len(),
            Frame::Data(data) => DataFrameHeader::SIZE + data.payload.len(),
        }
    }

    /// Append the frame's encoding to `out`.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Frame::Padding => out.push(FRAME_PADDING),
            Frame::Ping => out.push(FRAME_PING),
            Frame::Ack(ack) => ack.encode_into(out),
            Frame::Data(data) => {
                let header = DataFrameHeader {
                    frame_type: FRAME_DATA,
                    seq: data.seq.to_be_bytes(),
                    length: (data.payload.len() as u16).to_be_bytes(),
                };
                let mut bytes = [0u8; DataFrameHeader::SIZE];
                header.unchecked_write_to_slice(&mut bytes);
                out.extend_from_slice(&bytes);
                out.extend_from_slice(&data.payload);
            }
        }
    }
}

impl AckFrame {
    /// The frame's encoded length in bytes.
    pub fn wire_len(&self) -> usize {
        AckHeader::SIZE + self.ranges.len() * AckRangePair::SIZE
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        debug_assert!(
            self.ranges.len() <= MAX_ACK_RANGES,
            "encoder respects the cap"
        );
        let header = AckHeader {
            frame_type: FRAME_ACK,
            largest: self.largest.to_be_bytes(),
            ack_delay_micros: self.ack_delay_micros.to_be_bytes(),
            first_range: self.first_range.to_be_bytes(),
            range_count: self.ranges.len() as u8,
        };
        let mut bytes = [0u8; AckHeader::SIZE];
        header.unchecked_write_to_slice(&mut bytes);
        out.extend_from_slice(&bytes);
        for &(gap, length) in &self.ranges {
            let pair = AckRangePair {
                gap: gap.to_be_bytes(),
                length: length.to_be_bytes(),
            };
            let mut bytes = [0u8; AckRangePair::SIZE];
            pair.unchecked_write_to_slice(&mut bytes);
            out.extend_from_slice(&bytes);
        }
    }

    /// Build an ACK straight from a replay-window snapshot (`greatest`,
    /// `bitmap` — `bit k` marks `greatest − k` received) and the measured
    /// `ack_delay`.
    ///
    /// The window is the single received-packet record (no second bitmap), so
    /// an ACK always reports the freshest ≤ 128 counters; anything older has
    /// fallen out and is simply no longer acknowledged — the sender's DATA
    /// dedup absorbs any resulting spurious retransmission.
    pub fn from_window(greatest: u64, bitmap: u128, ack_delay: Duration) -> Self {
        // Collect the received runs, walking from bit 0 (= greatest) downwards
        // but never below counter zero.
        let width = 128u64.min(greatest + 1);
        let received = |k: u64| bitmap & (1u128 << k) != 0;

        // The first block: contiguous set bits from bit 0.
        let mut k = 0u64;
        while k + 1 < width && received(k + 1) {
            k += 1;
        }
        let first_range = k as u16;

        let mut ranges = Vec::new();
        let mut cursor = k + 1;
        while cursor < width && ranges.len() < MAX_ACK_RANGES {
            // The gap run (unreceived).
            let gap_start = cursor;
            while cursor < width && !received(cursor) {
                cursor += 1;
            }
            if cursor >= width {
                break; // trailing gap: nothing received below it in the window
            }
            let gap_len = cursor - gap_start;
            // The received run.
            let run_start = cursor;
            while cursor < width && received(cursor) {
                cursor += 1;
            }
            let run_len = cursor - run_start;

            // The QUIC pair encoding: gap = (unreceived run) − 1, and
            // length = (received run) − 1 — the block bounds follow from the
            // preceding block's smallest, so the runs alone determine the pair.
            ranges.push(((gap_len - 1) as u16, (run_len - 1) as u16));
        }

        let ack_delay_micros = ack_delay.as_micros().min(u128::from(u32::MAX)) as u32;
        AckFrame {
            largest: greatest,
            ack_delay_micros,
            first_range,
            ranges,
        }
    }

    /// Every acknowledged counter, descending, or an error if the ranges are
    /// malformed (descend below zero).
    pub fn acked(&self) -> Result<Vec<u64>, FrameError> {
        let mut out = Vec::new();
        let first = u64::from(self.first_range);
        if self.largest < first {
            return Err(FrameError::AckUnderflow);
        }
        let mut smallest = self.largest - first;
        for pn in (smallest..=self.largest).rev() {
            out.push(pn);
        }
        for &(gap, length) in &self.ranges {
            let gap = u64::from(gap);
            let length = u64::from(length);
            // block_largest = prev_smallest − gap − 2; block covers
            // block_largest − length ..= block_largest.
            let block_largest = smallest
                .checked_sub(gap + 2)
                .ok_or(FrameError::AckUnderflow)?;
            let block_smallest = block_largest
                .checked_sub(length)
                .ok_or(FrameError::AckUnderflow)?;
            for pn in (block_smallest..=block_largest).rev() {
                out.push(pn);
            }
            smallest = block_smallest;
        }
        Ok(out)
    }

    /// Whether `pn` is one of the acknowledged counters, tested **without
    /// materialising** the full set — `O(range_count)`, bounded by
    /// [`MAX_ACK_RANGES`].
    ///
    /// This is what the recovery loop uses: it iterates its own bounded set of
    /// outstanding packets and asks this of each, so a wire-legal ACK whose
    /// ranges imply millions of counters costs nothing beyond the ranges it
    /// actually carries (unlike [`acked`](Self::acked), which allocates one
    /// `u64` per implied counter and must not be fed attacker-influenced
    /// ranges). Assumes well-formed ranges (decode calls [`check`](Self::check));
    /// a malformed range simply yields `false` rather than a panic.
    pub(crate) fn contains(&self, pn: u64) -> bool {
        if pn > self.largest {
            return false;
        }
        // First block: `largest − first_range ..= largest`.
        let Some(mut smallest) = self.largest.checked_sub(u64::from(self.first_range)) else {
            return false;
        };
        if pn >= smallest {
            return true;
        }
        for &(gap, length) in &self.ranges {
            // block_largest = prev_smallest − gap − 2; block covers
            // block_largest − length ..= block_largest.
            let Some(block_largest) = smallest.checked_sub(u64::from(gap) + 2) else {
                return false;
            };
            if pn > block_largest {
                return false; // pn sits in the unreceived gap above this block
            }
            let Some(block_smallest) = block_largest.checked_sub(u64::from(length)) else {
                return false;
            };
            if pn >= block_smallest {
                return true;
            }
            smallest = block_smallest;
        }
        false
    }

    /// Validate the range arithmetic without materialising the counters.
    fn check(&self) -> Result<(), FrameError> {
        let mut smallest = self
            .largest
            .checked_sub(u64::from(self.first_range))
            .ok_or(FrameError::AckUnderflow)?;
        for &(gap, length) in &self.ranges {
            let block_largest = smallest
                .checked_sub(u64::from(gap) + 2)
                .ok_or(FrameError::AckUnderflow)?;
            smallest = block_largest
                .checked_sub(u64::from(length))
                .ok_or(FrameError::AckUnderflow)?;
        }
        Ok(())
    }
}

// ── Encode / decode ───────────────────────────────────────────────────────────

/// Encode `frames` back to back into one plaintext.
pub fn encode_all(frames: &[Frame]) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames.iter().map(Frame::wire_len).sum());
    for frame in frames {
        frame.encode_into(&mut out);
    }
    out
}

/// Decode a sealed packet's plaintext into its frames.
///
/// Parsing runs to the end of the plaintext. A **reserved** type
/// (`0x04..=0x0F`) stops parsing — the remainder of the packet is silently
/// ignored and the frames already parsed are returned. Anything malformed —
/// a truncated frame, an over-cap or underflowing ACK, an unknown type
/// (`0x10..`) — fails the whole packet: it was authenticated, so a parse error
/// is a protocol violation. Never panics.
pub fn decode_all(plaintext: &[u8]) -> Result<Vec<Frame>, FrameError> {
    let mut frames = Vec::new();
    let mut rest = plaintext;
    while let Some((&frame_type, _)) = rest.split_first() {
        match frame_type {
            FRAME_PADDING => {
                frames.push(Frame::Padding);
                rest = &rest[1..];
            }
            FRAME_PING => {
                frames.push(Frame::Ping);
                rest = &rest[1..];
            }
            FRAME_ACK => {
                let header_bytes = rest.get(..AckHeader::SIZE).ok_or(FrameError::Truncated {
                    frame: "ACK",
                    needed: AckHeader::SIZE,
                    remaining: rest.len(),
                })?;
                let header = View::<AckHeader>::try_from_slice(header_bytes)
                    .map_err(|_| FrameError::Truncated {
                        frame: "ACK",
                        needed: AckHeader::SIZE,
                        remaining: rest.len(),
                    })?
                    .unpack();
                let count = usize::from(header.range_count);
                if count > MAX_ACK_RANGES {
                    return Err(FrameError::TooManyRanges {
                        count,
                        max: MAX_ACK_RANGES,
                    });
                }
                let total = AckHeader::SIZE + count * AckRangePair::SIZE;
                let body = rest.get(..total).ok_or(FrameError::Truncated {
                    frame: "ACK",
                    needed: total,
                    remaining: rest.len(),
                })?;
                let mut ranges = Vec::with_capacity(count);
                for chunk in body[AckHeader::SIZE..].chunks_exact(AckRangePair::SIZE) {
                    let pair = View::<AckRangePair>::try_from_slice(chunk)
                        .map_err(|_| FrameError::Truncated {
                            frame: "ACK range",
                            needed: AckRangePair::SIZE,
                            remaining: chunk.len(),
                        })?
                        .unpack();
                    ranges.push((
                        u16::from_be_bytes(pair.gap),
                        u16::from_be_bytes(pair.length),
                    ));
                }
                let ack = AckFrame {
                    largest: u64::from_be_bytes(header.largest),
                    ack_delay_micros: u32::from_be_bytes(header.ack_delay_micros),
                    first_range: u16::from_be_bytes(header.first_range),
                    ranges,
                };
                ack.check()?;
                frames.push(Frame::Ack(ack));
                rest = &rest[total..];
            }
            FRAME_DATA => {
                let header_bytes =
                    rest.get(..DataFrameHeader::SIZE)
                        .ok_or(FrameError::Truncated {
                            frame: "DATA",
                            needed: DataFrameHeader::SIZE,
                            remaining: rest.len(),
                        })?;
                let header = View::<DataFrameHeader>::try_from_slice(header_bytes)
                    .map_err(|_| FrameError::Truncated {
                        frame: "DATA",
                        needed: DataFrameHeader::SIZE,
                        remaining: rest.len(),
                    })?
                    .unpack();
                let len = usize::from(u16::from_be_bytes(header.length));
                let total = DataFrameHeader::SIZE + len;
                let body = rest.get(..total).ok_or(FrameError::Truncated {
                    frame: "DATA",
                    needed: total,
                    remaining: rest.len(),
                })?;
                frames.push(Frame::Data(DataFrame {
                    seq: u64::from_be_bytes(header.seq),
                    payload: body[DataFrameHeader::SIZE..].to_vec(),
                }));
                rest = &rest[total..];
            }
            FRAME_RESERVED_STREAM..=FRAME_RESERVED_TOP => {
                // Reserved: the layout is unknowable, so nothing beyond it can
                // be framed — ignore the remainder, keep what parsed.
                break;
            }
            unknown => return Err(FrameError::UnknownType(unknown)),
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_sizes_are_pinned() {
        assert_eq!(<AckHeader as Packed>::SIZE, 16);
        assert_eq!(<AckRangePair as Packed>::SIZE, 4);
        assert_eq!(<DataFrameHeader as Packed>::SIZE, 11);
        assert_eq!(DATA_OVERHEAD, 11);
        assert_eq!(MAX_MESSAGE, 1159);
        assert_eq!(MAX_ACK_WIRE, 268);
    }

    #[test]
    fn constants_are_proposed_values() {
        assert_eq!(FRAME_PADDING, 0x00);
        assert_eq!(FRAME_PING, 0x01);
        assert_eq!(FRAME_ACK, 0x02);
        assert_eq!(FRAME_DATA, 0x03);
        assert_eq!(FRAME_RESERVED_STREAM, 0x04);
        assert_eq!(FRAME_RESERVED_FILE, 0x05);
        assert_eq!(FRAME_RESERVED_CLOSE, 0x06);
        assert_eq!(FRAME_RESERVED_TOP, 0x0F);
        assert_eq!(MAX_ACK_RANGES, 63);
    }

    #[test]
    fn padding_and_ping_round_trip() {
        let frames = vec![Frame::Padding, Frame::Ping, Frame::Padding];
        let bytes = encode_all(&frames);
        assert_eq!(bytes, [0x00, 0x01, 0x00]);
        assert_eq!(decode_all(&bytes).unwrap(), frames);
    }

    #[test]
    fn data_round_trips_big_endian() {
        let frame = Frame::Data(DataFrame {
            seq: 0x0102_0304_0506_0708,
            payload: b"hi".to_vec(),
        });
        let bytes = encode_all(std::slice::from_ref(&frame));
        // Explicit layout: type, seq BE, length BE, payload.
        assert_eq!(
            bytes,
            [
                0x03, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x00, 0x02, b'h', b'i'
            ]
        );
        assert_eq!(decode_all(&bytes).unwrap(), vec![frame]);
    }

    #[test]
    fn ack_round_trips_big_endian() {
        let frame = Frame::Ack(AckFrame {
            largest: 0x0A0B,
            ack_delay_micros: 0x0000_0102,
            first_range: 3,
            ranges: vec![(1, 0), (0, 2)],
        });
        let bytes = encode_all(std::slice::from_ref(&frame));
        assert_eq!(
            bytes,
            [
                0x02, // type
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x0B, // largest BE
                0x00, 0x00, 0x01, 0x02, // ack_delay µs BE
                0x00, 0x03, // first_range BE
                0x02, // range_count
                0x00, 0x01, 0x00, 0x00, // (gap 1, length 0)
                0x00, 0x00, 0x00, 0x02, // (gap 0, length 2)
            ]
        );
        assert_eq!(decode_all(&bytes).unwrap(), vec![frame]);
    }

    #[test]
    fn coalesced_sequence_round_trips() {
        let frames = vec![
            Frame::Ack(AckFrame {
                largest: 9,
                ack_delay_micros: 0,
                first_range: 1,
                ranges: vec![(0, 1)],
            }),
            Frame::Data(DataFrame {
                seq: 0,
                payload: b"one".to_vec(),
            }),
            Frame::Data(DataFrame {
                seq: 7,
                payload: Vec::new(), // an empty message is a valid DATA
            }),
            Frame::Ping,
            Frame::Padding,
        ];
        let bytes = encode_all(&frames);
        assert_eq!(
            bytes.len(),
            frames.iter().map(Frame::wire_len).sum::<usize>()
        );
        assert_eq!(decode_all(&bytes).unwrap(), frames);
    }

    #[test]
    fn malformed_streams_fail_the_packet() {
        // Truncated DATA header.
        assert_eq!(
            decode_all(&[0x03, 0x00, 0x00]),
            Err(FrameError::Truncated {
                frame: "DATA",
                needed: 11,
                remaining: 3,
            })
        );
        // DATA whose declared length overruns the plaintext.
        let mut short = encode_all(&[Frame::Data(DataFrame {
            seq: 1,
            payload: b"abc".to_vec(),
        })]);
        short.truncate(short.len() - 1);
        assert_eq!(
            decode_all(&short),
            Err(FrameError::Truncated {
                frame: "DATA",
                needed: 14,
                remaining: 13,
            })
        );
        // Truncated ACK ranges.
        let mut ack = encode_all(&[Frame::Ack(AckFrame {
            largest: 5,
            ack_delay_micros: 0,
            first_range: 0,
            ranges: vec![(0, 0)],
        })]);
        ack.truncate(ack.len() - 2);
        assert!(matches!(
            decode_all(&ack),
            Err(FrameError::Truncated { frame: "ACK", .. })
        ));
        // Unknown (non-reserved) type: a protocol violation.
        assert_eq!(decode_all(&[0x10]), Err(FrameError::UnknownType(0x10)));
        assert_eq!(
            decode_all(&[0x01, 0xFF]),
            Err(FrameError::UnknownType(0xFF))
        );
        // An underflowing ACK: first_range below counter zero.
        let under = encode_all(&[Frame::Ack(AckFrame {
            largest: 1,
            ack_delay_micros: 0,
            first_range: 5,
            ranges: Vec::new(),
        })]);
        assert_eq!(decode_all(&under), Err(FrameError::AckUnderflow));
        // An underflowing ACK: a range descending below zero.
        let under = encode_all(&[Frame::Ack(AckFrame {
            largest: 3,
            ack_delay_micros: 0,
            first_range: 0,
            ranges: vec![(200, 0)],
        })]);
        assert_eq!(decode_all(&under), Err(FrameError::AckUnderflow));
        // Empty plaintext: no frames, not an error (never reached in practice —
        // an empty plaintext is the Leg 1 keepalive, which bypasses the layer).
        assert_eq!(decode_all(&[]), Ok(Vec::new()));
    }

    #[test]
    fn reserved_type_skips_the_remainder() {
        let mut bytes = encode_all(&[Frame::Data(DataFrame {
            seq: 4,
            payload: b"keep".to_vec(),
        })]);
        bytes.push(FRAME_RESERVED_STREAM);
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // unknowable layout
        let frames = decode_all(&bytes).unwrap();
        assert_eq!(
            frames,
            vec![Frame::Data(DataFrame {
                seq: 4,
                payload: b"keep".to_vec(),
            })]
        );
        // Every reserved type behaves identically.
        for reserved in FRAME_RESERVED_STREAM..=FRAME_RESERVED_TOP {
            assert_eq!(decode_all(&[reserved, 0xAA]), Ok(Vec::new()));
        }
    }

    #[test]
    fn max_ack_fits_max_plaintext() {
        let ack = AckFrame {
            largest: 1_000_000,
            ack_delay_micros: u32::MAX,
            first_range: 0,
            ranges: vec![(0, 0); MAX_ACK_RANGES],
        };
        let frame = Frame::Ack(ack);
        let bytes = encode_all(std::slice::from_ref(&frame));
        assert_eq!(bytes.len(), MAX_ACK_WIRE);
        assert!(bytes.len() <= MAX_PLAINTEXT);
        assert_eq!(decode_all(&bytes).unwrap(), vec![frame]);
    }

    #[test]
    fn over_cap_ack_is_rejected() {
        // Hand-craft an ACK header declaring 64 ranges (over the 63 cap).
        let mut bytes = vec![0x02];
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.push(64);
        bytes.extend_from_slice(&[0u8; 64 * 4]);
        assert_eq!(
            decode_all(&bytes),
            Err(FrameError::TooManyRanges { count: 64, max: 63 })
        );
    }

    #[test]
    fn window_to_ack_and_back() {
        // Window: greatest 10; received {10, 9, 8, 5, 4, 1}.
        // bits: k=0(10),1(9),2(8),5(5),6(4),9(1).
        let bitmap: u128 = 0b10_0110_0111;
        let ack = AckFrame::from_window(10, bitmap, Duration::from_micros(250));
        assert_eq!(ack.largest, 10);
        assert_eq!(ack.ack_delay_micros, 250);
        assert_eq!(ack.first_range, 2, "10, 9, 8 are the first block");
        // Gap {7, 6} → gap = 1; block {5, 4} → length = 1.
        // Gap {3, 2} → gap = 1; block {1} → length = 0.
        assert_eq!(ack.ranges, vec![(1, 1), (1, 0)]);
        assert_eq!(ack.acked().unwrap(), vec![10, 9, 8, 5, 4, 1]);
    }

    #[test]
    fn window_alternating_worst_case_is_complete() {
        // Alternating bits: greatest 200, received {200, 198, 196, …} — 64 set
        // bits, one first block + exactly 63 ranges: the cap loses nothing.
        let mut bitmap: u128 = 0;
        for k in (0..128).step_by(2) {
            bitmap |= 1 << k;
        }
        let ack = AckFrame::from_window(200, bitmap, Duration::ZERO);
        assert_eq!(ack.first_range, 0);
        assert_eq!(ack.ranges.len(), MAX_ACK_RANGES);
        let acked = ack.acked().unwrap();
        let expected: Vec<u64> = (0..64).map(|i| 200 - 2 * i).collect();
        assert_eq!(acked, expected);
    }

    #[test]
    fn window_near_zero_stays_in_bounds() {
        // greatest = 2 with only bit 0 and bit 2 set: {2, 0}.
        let ack = AckFrame::from_window(2, 0b101, Duration::ZERO);
        assert_eq!(ack.first_range, 0);
        assert_eq!(ack.ranges, vec![(0, 0)]);
        assert_eq!(ack.acked().unwrap(), vec![2, 0]);
        // greatest = 0: the single-counter window.
        let ack = AckFrame::from_window(0, 0b1, Duration::ZERO);
        assert_eq!(ack.first_range, 0);
        assert!(ack.ranges.is_empty());
        assert_eq!(ack.acked().unwrap(), vec![0]);
    }
}

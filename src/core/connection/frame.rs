//! §8 — the frame layer: the codec, the table, and the packing order.
//!
//! This is the *inner* wire. §2–§4's outer packet is [`crate::packet`] and
//! is complete; everything here lives inside a sealed Data packet's
//! plaintext, which the AEAD has already authenticated and whose exact
//! length the AEAD supplies (§8.2 — there is no packet-level length
//! prefix).
//!
//! # Four frames, and why the table still has twelve rows
//!
//! Slice 3 implements PADDING (`0x00`), PING (`0x01`), ACK (`0x02`) and
//! CLOSE (`0x1c`). ACK is **codec only** here: §12's derivation and
//! processing are slice 5, so a received ACK parses and is then ignored.
//!
//! PADDING is not optional. §8.2 makes an unrecognised type a
//! `PROTOCOL_VIOLATION` kill, and §8.4 says of PADDING that "any number may
//! appear anywhere" — so a codec that implemented three frames would kill a
//! connection on a legal packet.
//!
//! The **classifiers** ([`is_ack_eliciting`], [`retransmission`]) cover all
//! twelve rows of §8.3, including the eight types the parser cannot yet
//! build. That is deliberate and it is the only way they are testable:
//! every frame slice 3 builds is in the `never` retransmission class and
//! PING is the only ack-eliciting one among them, so a two-arm classifier
//! would be correct-by-accident for the whole slice. Slices 4–6 add parse
//! arms; they must not need to touch the classifiers.
//!
//! The parser and the classifiers therefore disagree, on purpose, about
//! what "known" means: the classifiers answer for the ratified table, the
//! parser answers for what this build can apply. Every type the parser does
//! not implement takes §8.2's unknown-type path.
//!
//! # Parse-then-apply is literal
//!
//! §8.2: *"**Parse the whole plaintext first, then apply.**"* [`parse`]
//! returns a `Vec<Frame>` and applies nothing. A streaming
//! parse-and-apply loop would pass every test that only checks "an unknown
//! type produces `ProtocolViolation`", and would differ observably on a
//! plaintext of `[valid CLOSE][unknown type]`: the correct implementation
//! surfaces `ProtocolViolation`, the streaming one surfaces `PeerClosed`.

use std::ops::RangeInclusive;

use crate::constants;
use crate::varint::{self, VarInt};

/// A parsed frame. §8.3, §8.4.
///
/// The eight types slice 3 does not implement are **absent rather than
/// stubbed**, following this module tree's precedent: a variant is a claim
/// that the layer can produce and consume the thing, and these arrive with
/// the sections that define them (§9, §10, §11, §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Frame {
    /// `0x00` — a single byte, no fields, any number anywhere. §8.4.
    Padding,
    /// `0x01` — the type byte alone. Ack-eliciting. §8.4, §13.4.
    Ping,
    /// `0x02` — §12's acknowledgement. Codec only in this slice.
    Ack(Ack),
    /// `0x1c` — §15's teardown signal.
    Close(Close),
}

impl Frame {
    /// This frame's §8.3 type code.
    pub(crate) fn type_code(&self) -> u64 {
        match self {
            Frame::Padding => constants::FRAME_PADDING,
            Frame::Ping => constants::FRAME_PING,
            Frame::Ack(_) => constants::FRAME_ACK,
            Frame::Close(_) => constants::FRAME_CLOSE,
        }
    }

    /// Whether this frame is ack-eliciting (§8.3's column).
    pub(crate) fn is_ack_eliciting(&self) -> bool {
        is_ack_eliciting(self.type_code())
    }

    /// The encoded length in bytes, type code included.
    pub(crate) fn encoded_len(&self) -> usize {
        match self {
            Frame::Padding | Frame::Ping => 1,
            Frame::Ack(ack) => 1 + ack.body_len(),
            Frame::Close(close) => 1 + close.body_len(),
        }
    }

    /// Append this frame's wire encoding to `out`. §8.4.
    ///
    /// The type code is a varint like every other field (§8.1), and all
    /// four of this slice's codes are below 64, so each occupies one byte.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        varint::encode(
            VarInt::new(self.type_code()).expect("§8.3's type codes are all far below 2⁶² − 1"),
            out,
        );
        match self {
            Frame::Padding | Frame::Ping => {}
            Frame::Ack(ack) => ack.encode_body(out),
            Frame::Close(close) => close.encode_body(out),
        }
    }
}

/// §8.4's ACK, as its wire fields.
///
/// Held verbatim rather than as a decoded range set: slice 3 is the codec
/// and a round trip must be byte-identical. §12's semantics — what an ACK
/// means, when one is owed, how one is derived from §7.2's window — are
/// slice 5's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ack {
    /// The largest counter this ACK acknowledges.
    pub(crate) largest: u64,
    /// The delay between receiving `largest` and sending this ACK, in µs.
    pub(crate) ack_delay: u64,
    /// How many counters below `largest` the first range also covers.
    pub(crate) first_range: u64,
    /// Additional `(gap, range)` pairs, descending. At most
    /// [`MAX_ACK_RANGES`](crate::constants::MAX_ACK_RANGES).
    pub(crate) ranges: Vec<(u64, u64)>,
}

impl Ack {
    /// The acknowledged counter ranges, **newest-first, descending** —
    /// §12.2's construction order, read back.
    ///
    /// Only meaningful on an ACK that has been validated by [`parse`] or
    /// built from a validated window; [`Ack::validate`] is what excludes
    /// the descent below counter zero that would make this saturate.
    pub(crate) fn ranges_desc(&self) -> Vec<RangeInclusive<u64>> {
        let mut out = Vec::with_capacity(1 + self.ranges.len());
        let mut smallest = self.largest.saturating_sub(self.first_range);
        out.push(smallest..=self.largest);
        for (gap, range) in &self.ranges {
            let largest = smallest.saturating_sub(*gap).saturating_sub(2);
            smallest = largest.saturating_sub(*range);
            out.push(smallest..=largest);
        }
        out
    }

    /// §8.4's two structural error cases for ACK.
    ///
    /// *"`range_count` > `MAX_ACK_RANGES` (64); any range descending below
    /// counter zero."* The descent is QUIC's: each additional pair starts
    /// `gap + 2` below the previous range's smallest counter, so an
    /// underflow anywhere is the frame claiming counters that cannot exist.
    fn validate(&self) -> Result<(), Structural> {
        if self.ranges.len() > constants::MAX_ACK_RANGES {
            return Err(Structural::AckRangeCount(self.ranges.len() as u64));
        }
        let mut smallest = self
            .largest
            .checked_sub(self.first_range)
            .ok_or(Structural::AckRangeUnderflow)?;
        for (gap, range) in &self.ranges {
            let largest = smallest
                .checked_sub(*gap)
                .and_then(|v| v.checked_sub(2))
                .ok_or(Structural::AckRangeUnderflow)?;
            smallest = largest
                .checked_sub(*range)
                .ok_or(Structural::AckRangeUnderflow)?;
        }
        Ok(())
    }

    fn body_len(&self) -> usize {
        varint_len(self.largest)
            + varint_len(self.ack_delay)
            + varint_len(self.ranges.len() as u64)
            + varint_len(self.first_range)
            + self
                .ranges
                .iter()
                .map(|(gap, range)| varint_len(*gap) + varint_len(*range))
                .sum::<usize>()
    }

    fn encode_body(&self, out: &mut Vec<u8>) {
        put_varint(self.largest, out);
        put_varint(self.ack_delay, out);
        put_varint(self.ranges.len() as u64, out);
        put_varint(self.first_range, out);
        for (gap, range) in &self.ranges {
            put_varint(*gap, out);
            put_varint(*range, out);
        }
    }

    /// Parse an ACK body, returning it and the bytes consumed.
    fn parse_body(buf: &[u8]) -> Result<(Ack, usize), Structural> {
        let mut cursor = Cursor::new(buf);
        let largest = cursor.varint()?;
        let ack_delay = cursor.varint()?;
        let range_count = cursor.varint()?;

        // Checked before the pairs are read, so a bogus count cannot make
        // the parser allocate against a `u64` it will then fail on.
        if range_count > constants::MAX_ACK_RANGES as u64 {
            return Err(Structural::AckRangeCount(range_count));
        }
        let first_range = cursor.varint()?;

        let mut ranges = Vec::with_capacity(range_count as usize);
        for _ in 0..range_count {
            let gap = cursor.varint()?;
            let range = cursor.varint()?;
            ranges.push((gap, range));
        }

        let ack = Ack {
            largest,
            ack_delay,
            first_range,
            ranges,
        };
        ack.validate()?;
        Ok((ack, cursor.consumed()))
    }
}

/// §8.4's CLOSE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Close {
    /// §15.3's registry code, or an application code ≥ `0x10`.
    pub(crate) code: u64,
    /// At most [`CLOSE_REASON_MAX`](crate::constants::CLOSE_REASON_MAX)
    /// bytes. SHOULD be UTF-8, carried as bytes.
    pub(crate) reason: Vec<u8>,
}

impl Close {
    /// A CLOSE, with `reason` truncated to `CLOSE_REASON_MAX`.
    ///
    /// §8.4: *"an implementation must not be able to **produce** the
    /// over-length case it must kill on receipt."* §16.2 truncates at the
    /// handle; truncating here as well means no path through the core can
    /// build one, including the handle-free core tests.
    pub(crate) fn new(code: u64, reason: &[u8]) -> Self {
        let n = reason.len().min(constants::CLOSE_REASON_MAX);
        Self {
            code,
            reason: reason[..n].to_vec(),
        }
    }

    fn body_len(&self) -> usize {
        varint_len(self.code) + varint_len(self.reason.len() as u64) + self.reason.len()
    }

    fn encode_body(&self, out: &mut Vec<u8>) {
        debug_assert!(
            self.reason.len() <= constants::CLOSE_REASON_MAX,
            "§8.4: a CLOSE this implementation produced must never exceed CLOSE_REASON_MAX"
        );
        put_varint(self.code, out);
        put_varint(self.reason.len() as u64, out);
        out.extend_from_slice(&self.reason);
    }

    fn parse_body(buf: &[u8]) -> Result<(Close, usize), Structural> {
        let mut cursor = Cursor::new(buf);
        let code = cursor.varint()?;
        let reason_len = cursor.varint()?;
        if reason_len > constants::CLOSE_REASON_MAX as u64 {
            return Err(Structural::CloseReasonTooLong(reason_len));
        }
        let reason = cursor.bytes(reason_len as usize)?.to_vec();
        Ok((Close { code, reason }, cursor.consumed()))
    }
}

/// §8.2's structural failure class — a **signalled death**.
///
/// Every variant is answered identically on the wire: one trace on
/// `slither::frames`, CLOSE with `PROTOCOL_VIOLATION`, the closing state,
/// and `ConnectionLost::ProtocolViolation { code }`. The variants exist for
/// the trace, which is operator-visible contract (§18.2) — not for a
/// per-case behaviour, of which there is exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Structural {
    /// A type code outside §8.3's table, or one reserved (`0x05`) or not
    /// implemented by this build.
    #[error("unknown frame type {0:#x}")]
    UnknownType(u64),
    /// A varint ran past the end of the plaintext.
    #[error("a varint overruns the plaintext")]
    VarintOverrun,
    /// A length-delimited field ran past the end of the plaintext.
    #[error("a length field overruns the plaintext")]
    LengthOverrun,
    /// ACK's `range_count` exceeded `MAX_ACK_RANGES`.
    #[error("ACK range_count {0} exceeds MAX_ACK_RANGES")]
    AckRangeCount(u64),
    /// An ACK range descended below counter zero.
    #[error("an ACK range descends below counter zero")]
    AckRangeUnderflow,
    /// CLOSE's `reason_len` exceeded `CLOSE_REASON_MAX`.
    #[error("CLOSE reason_len {0} exceeds CLOSE_REASON_MAX")]
    CloseReasonTooLong(u64),
}

/// §8.2's parse phase: the **whole** plaintext, applying nothing.
///
/// The caller applies the returned frames only if this returns `Ok` —
/// §8.2: *"Nothing from the packet is applied (no ACK scheduling, no state
/// change beyond the already-performed replay mark)."*
///
/// An empty plaintext never reaches here: it is §3.4's keepalive and
/// bypasses the frame layer entirely. Passing one in yields an empty frame
/// list rather than an error, because "no frames" is not a structural
/// failure — the short-circuit belongs at the receive path, where §3.4 puts
/// it, and is not duplicated here as a second opinion.
pub(crate) fn parse(plaintext: &[u8]) -> Result<Vec<Frame>, Structural> {
    let mut frames = Vec::new();
    let mut cursor = Cursor::new(plaintext);

    while !cursor.is_empty() {
        // The type code is itself a varint (§8.1, §8.3's note on the gaps).
        let ty = cursor.varint()?;
        let frame = match ty {
            constants::FRAME_PADDING => Frame::Padding,
            constants::FRAME_PING => Frame::Ping,
            constants::FRAME_ACK => {
                let (ack, used) = Ack::parse_body(cursor.rest())?;
                cursor.advance(used);
                Frame::Ack(ack)
            }
            constants::FRAME_CLOSE => {
                let (close, used) = Close::parse_body(cursor.rest())?;
                cursor.advance(used);
                Frame::Close(close)
            }
            // Everything else — §8.3's `0x05` reserved row, the types
            // slices 4–6 add, and any code outside the table — is §8.2's
            // unknown type. Slices 4–6 add arms above; they do not widen
            // this one.
            other => return Err(Structural::UnknownType(other)),
        };
        frames.push(frame);
    }

    Ok(frames)
}

/// Whether a packet carrying these frames is ack-eliciting (§8.7).
///
/// *"A packet is ack-eliciting iff it contains at least one ack-eliciting
/// frame."*
pub(crate) fn packet_is_ack_eliciting(frames: &[Frame]) -> bool {
    frames.iter().any(Frame::is_ack_eliciting)
}

/// §8.3's ack-eliciting column, as a pure function of the type code.
///
/// Table-driven over **all twelve rows**, including the types this slice
/// cannot construct — see the module docs for why that is the only way this
/// is testable at all.
///
/// A code outside the table is not ack-eliciting because it is not a frame:
/// receiving one is §8.2's structural failure and the packet is never
/// applied. The answer here is unreachable for such a code and is `false`
/// rather than a panic, because a panic reachable from a received packet is
/// the wrong failure for a transport.
pub(crate) fn is_ack_eliciting(ty: u64) -> bool {
    match ty {
        constants::FRAME_PADDING => false,
        constants::FRAME_PING => true,
        constants::FRAME_ACK => false,
        constants::FRAME_RESET_STREAM => true,
        constants::FRAME_STREAM_BASE..=constants::FRAME_STREAM_MAX => true,
        constants::FRAME_MAX_DATA
        | constants::FRAME_MAX_STREAM_DATA
        | constants::FRAME_MAX_STREAMS_BIDI
        | constants::FRAME_MAX_STREAMS_UNI => true,
        constants::FRAME_CLOSE => false,
        constants::FRAME_DATAGRAM | constants::FRAME_DATAGRAM_LEN => true,
        _ => false,
    }
}

/// §8.7's three retransmission classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Retransmission {
    /// STREAM: un-ACKed sub-ranges return to the pending set and are
    /// re-framed on fresh counters.
    Ranges,
    /// The frame's *identity* re-queues and carries the freshest value.
    Regenerate,
    /// Loss is absorbed by the next ACK, probe, the unreliability
    /// contract, or the linger reply rule.
    Never,
}

/// §8.3's retransmission column, as a pure function of the type code.
///
/// `None` for a code outside the table, and for `0x05` — which §8.3 marks
/// reserved with a `—` in every column, so answering for it would be an
/// invention.
pub(crate) fn retransmission(ty: u64) -> Option<Retransmission> {
    match ty {
        constants::FRAME_PADDING | constants::FRAME_PING | constants::FRAME_ACK => {
            Some(Retransmission::Never)
        }
        constants::FRAME_RESET_STREAM => Some(Retransmission::Regenerate),
        constants::FRAME_STREAM_BASE..=constants::FRAME_STREAM_MAX => Some(Retransmission::Ranges),
        constants::FRAME_MAX_DATA
        | constants::FRAME_MAX_STREAM_DATA
        | constants::FRAME_MAX_STREAMS_BIDI
        | constants::FRAME_MAX_STREAMS_UNI => Some(Retransmission::Regenerate),
        // §8.7 lists CLOSE under `never`; §8.3's column calls the same
        // thing "linger rule (§15.2)". They agree: CLOSE is never
        // *loss*-retransmitted, and the linger's reply is a separate
        // mechanism that does not run through loss recovery.
        constants::FRAME_CLOSE => Some(Retransmission::Never),
        constants::FRAME_DATAGRAM | constants::FRAME_DATAGRAM_LEN => Some(Retransmission::Never),
        _ => None,
    }
}

/// §8.5's packing order, expressed as stages that can only run forwards.
///
/// *"Within a packet the sender packs in this order: the ACK first (if
/// owed), then control frames (credit grants, RESET_STREAM, CLOSE), then
/// STREAM and DATAGRAM fill, then PING last if a probe still owes
/// ack-eliciting content."*
///
/// Slice 3 has three of those stages' contents (ACK, CLOSE, PING). The
/// missing middle is slice 4's STREAM fill and slice 6's DATAGRAM fill:
/// they insert a stage here, between [`control`](Packing::control) and
/// [`ping`](Packing::ping), rather than rewriting the order.
pub(crate) struct Packing {
    frames: Vec<Frame>,
    used: usize,
    budget: usize,
    stage: Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Ack,
    Control,
    Fill,
    Ping,
}

impl Packing {
    /// A packet plan with `MAX_PLAINTEXT` of room (§8.6).
    pub(crate) fn new() -> Self {
        Self {
            frames: Vec::new(),
            used: 0,
            budget: constants::MAX_PLAINTEXT,
            stage: Stage::Ack,
        }
    }

    /// Stage 1 — the ACK, if one is owed (§12.3).
    pub(crate) fn ack(&mut self, ack: Ack) -> bool {
        self.push(Stage::Ack, Frame::Ack(ack))
    }

    /// Stage 2 — control frames: credit grants, RESET_STREAM, CLOSE.
    pub(crate) fn control(&mut self, frame: Frame) -> bool {
        self.push(Stage::Control, frame)
    }

    /// Stage 4 — PING last, if a probe still owes ack-eliciting content.
    pub(crate) fn ping(&mut self) -> bool {
        self.push(Stage::Ping, Frame::Ping)
    }

    /// The frames planned so far, in packing order.
    pub(crate) fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// The planned plaintext.
    pub(crate) fn into_plaintext(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.used);
        for frame in &self.frames {
            frame.encode(&mut out);
        }
        debug_assert_eq!(out.len(), self.used, "encoded_len disagrees with encode");
        debug_assert!(
            out.len() <= constants::MAX_PLAINTEXT,
            "§8.6's per-seal bound"
        );
        out
    }

    fn push(&mut self, stage: Stage, frame: Frame) -> bool {
        debug_assert!(
            stage >= self.stage,
            "§8.5's packing order runs forwards only: {stage:?} after {:?}",
            self.stage
        );
        self.stage = stage;

        let len = frame.encoded_len();
        if self.used + len > self.budget {
            return false;
        }
        self.used += len;
        self.frames.push(frame);
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Cursor — the one place a length check is written
// ═══════════════════════════════════════════════════════════════════════

/// A parse cursor over the plaintext.
///
/// Every overrun check in §8.2's structural class lives here, so no frame
/// parser above rewrites one and gets it wrong once.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn rest(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    fn consumed(&self) -> usize {
        self.pos
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn varint(&mut self) -> Result<u64, Structural> {
        let (v, n) = varint::decode(self.rest()).ok_or(Structural::VarintOverrun)?;
        self.pos += n;
        Ok(v.into_inner())
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Structural> {
        let rest = self.rest();
        if rest.len() < n {
            return Err(Structural::LengthOverrun);
        }
        self.pos += n;
        Ok(&rest[..n])
    }
}

/// The encoded length of a value that must fit a varint.
///
/// Values above 2⁶² − 1 are saturated rather than refused: every field
/// slither *produces* is either a counter, a length bounded by
/// `MAX_PLAINTEXT`, or an application-chosen CLOSE code, and §16.2's
/// `close(code, reason)` takes a bare `u64` with no documented cap. See
/// `.slices/03-skeleton/IMPLEMENTATION.md` — §8.1's "stated consequence"
/// list names ACK `largest`, stream offsets and final sizes, and omits the
/// CLOSE code.
fn to_varint(v: u64) -> VarInt {
    VarInt::new(v).unwrap_or(VarInt::MAX)
}

fn varint_len(v: u64) -> usize {
    to_varint(v).encoded_len()
}

fn put_varint(v: u64, out: &mut Vec<u8>) {
    varint::encode(to_varint(v), out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(frame: &Frame) -> Vec<Frame> {
        let mut bytes = Vec::new();
        frame.encode(&mut bytes);
        assert_eq!(bytes.len(), frame.encoded_len(), "encoded_len is wrong");
        parse(&bytes).expect("a frame this codec produced must parse")
    }

    #[test]
    fn padding_and_ping_are_one_byte_each() {
        assert_eq!(round_trip(&Frame::Padding), vec![Frame::Padding]);
        assert_eq!(round_trip(&Frame::Ping), vec![Frame::Ping]);

        let mut bytes = Vec::new();
        Frame::Padding.encode(&mut bytes);
        assert_eq!(bytes, vec![0x00]);
        bytes.clear();
        Frame::Ping.encode(&mut bytes);
        assert_eq!(bytes, vec![0x01]);
    }

    /// §8.4: "any number may appear anywhere" — before, between and after.
    #[test]
    fn padding_may_appear_anywhere() {
        let plaintext = vec![0x00, 0x00, 0x01, 0x00, 0x01, 0x00];
        assert_eq!(
            parse(&plaintext).expect("PADDING is legal anywhere"),
            vec![
                Frame::Padding,
                Frame::Padding,
                Frame::Ping,
                Frame::Padding,
                Frame::Ping,
                Frame::Padding
            ]
        );
    }

    #[test]
    fn close_round_trips_with_code_and_reason() {
        let frame = Frame::Close(Close::new(0x42, b"because"));
        assert_eq!(
            round_trip(&frame),
            vec![Frame::Close(Close {
                code: 0x42,
                reason: b"because".to_vec()
            })]
        );
    }

    /// An empty reason is the graceful close's shape, and it must encode
    /// as `reason_len = 0` with no bytes rather than being skipped.
    #[test]
    fn close_with_an_empty_reason_round_trips() {
        let mut bytes = Vec::new();
        Frame::Close(Close::new(constants::NO_ERROR, b"")).encode(&mut bytes);
        assert_eq!(bytes, vec![0x1c, 0x00, 0x00]);
        assert_eq!(
            parse(&bytes).unwrap(),
            vec![Frame::Close(Close {
                code: 0,
                reason: Vec::new()
            })]
        );
    }

    /// §8.4: `close()` truncates at the handle — and the core must not be
    /// able to produce the over-length case it kills on receipt.
    #[test]
    fn close_new_truncates_at_close_reason_max() {
        let long = vec![b'x'; constants::CLOSE_REASON_MAX + 64];
        let close = Close::new(1, &long);
        assert_eq!(close.reason.len(), constants::CLOSE_REASON_MAX);
    }

    /// Both sides of the boundary: exactly `CLOSE_REASON_MAX` parses,
    /// one more is a structural failure. Testing only the failing side
    /// would pass an implementation whose window is 16 bytes.
    #[test]
    fn close_reason_len_boundary_is_two_sided() {
        for (len, ok) in [
            (constants::CLOSE_REASON_MAX - 1, true),
            (constants::CLOSE_REASON_MAX, true),
            (constants::CLOSE_REASON_MAX + 1, false),
        ] {
            let mut bytes = vec![0x1c];
            put_varint(7, &mut bytes);
            put_varint(len as u64, &mut bytes);
            bytes.extend(std::iter::repeat_n(b'z', len));

            let parsed = parse(&bytes);
            assert_eq!(
                parsed.is_ok(),
                ok,
                "reason_len {len} should {} parse",
                if ok { "" } else { "not" }
            );
            if !ok {
                assert_eq!(
                    parsed.unwrap_err(),
                    Structural::CloseReasonTooLong(len as u64)
                );
            }
        }
    }

    #[test]
    fn close_reason_running_past_the_plaintext_is_structural() {
        let mut bytes = vec![0x1c];
        put_varint(1, &mut bytes);
        put_varint(8, &mut bytes);
        bytes.extend_from_slice(b"only4");
        assert_eq!(parse(&bytes).unwrap_err(), Structural::LengthOverrun);
    }

    #[test]
    fn ack_round_trips_with_extra_ranges() {
        let ack = Ack {
            largest: 100,
            ack_delay: 1234,
            first_range: 3,
            ranges: vec![(0, 1), (4, 2)],
        };
        assert_eq!(round_trip(&Frame::Ack(ack.clone())), vec![Frame::Ack(ack)]);
    }

    /// §12.2's newest-first descending order, read back off the wire.
    #[test]
    fn ack_ranges_are_descending_and_newest_first() {
        let ack = Ack {
            largest: 100,
            ack_delay: 0,
            first_range: 3,
            ranges: vec![(0, 1), (4, 2)],
        };
        // 97..=100, then gap 0 → largest 95, range 1 → 94..=95, then
        // gap 4 → largest 88, range 2 → 86..=88.
        assert_eq!(ack.ranges_desc(), vec![97..=100, 94..=95, 86..=88]);
    }

    /// Both sides of `MAX_ACK_RANGES`.
    #[test]
    fn ack_range_count_boundary_is_two_sided() {
        for (count, ok) in [
            (constants::MAX_ACK_RANGES - 1, true),
            (constants::MAX_ACK_RANGES, true),
            (constants::MAX_ACK_RANGES + 1, false),
        ] {
            let ack = Ack {
                largest: 1_000_000,
                ack_delay: 0,
                first_range: 0,
                ranges: vec![(0, 0); count],
            };
            let mut bytes = Vec::new();
            Frame::Ack(ack).encode(&mut bytes);

            let parsed = parse(&bytes);
            assert_eq!(parsed.is_ok(), ok, "range_count {count}");
            if !ok {
                assert_eq!(parsed.unwrap_err(), Structural::AckRangeCount(count as u64));
            }
        }
    }

    #[test]
    fn an_ack_range_below_counter_zero_is_structural() {
        // first_range alone descends past zero.
        let mut bytes = Vec::new();
        Frame::Ack(Ack {
            largest: 2,
            ack_delay: 0,
            first_range: 5,
            ranges: Vec::new(),
        })
        .encode(&mut bytes);
        assert_eq!(parse(&bytes).unwrap_err(), Structural::AckRangeUnderflow);

        // And a later pair descends past zero.
        let mut bytes = Vec::new();
        Frame::Ack(Ack {
            largest: 10,
            ack_delay: 0,
            first_range: 4,
            ranges: vec![(9, 0)],
        })
        .encode(&mut bytes);
        assert_eq!(parse(&bytes).unwrap_err(), Structural::AckRangeUnderflow);
    }

    #[test]
    fn a_truncated_ack_is_structural() {
        let bytes = vec![0x02, 0x05]; // type + largest, then nothing
        assert_eq!(parse(&bytes).unwrap_err(), Structural::VarintOverrun);
    }

    /// §8.3's `0x05` is reserved, "not implemented: like any unknown type,
    /// receiving it is a structural failure".
    #[test]
    fn the_reserved_type_is_an_unknown_type() {
        assert_eq!(
            parse(&[constants::FRAME_STOP_SENDING_RESERVED as u8]).unwrap_err(),
            Structural::UnknownType(0x05)
        );
    }

    /// `0x3f` is one byte and unassigned; `0x7f` is the first byte of a
    /// **two**-byte varint (prefix `01`), so on its own it is a truncated
    /// varint rather than an unknown type — both are §8.2's structural
    /// class, and the distinction is the trace's, not the wire's.
    #[test]
    fn an_unknown_type_is_structural() {
        assert_eq!(parse(&[0x3f]).unwrap_err(), Structural::UnknownType(0x3f));
        assert_eq!(parse(&[0x7f]).unwrap_err(), Structural::VarintOverrun);
        // A well-formed two-byte encoding of an unassigned code is an
        // unknown type, not a truncation.
        assert_eq!(
            parse(&[0x40, 0x7f]).unwrap_err(),
            Structural::UnknownType(0x7f)
        );
    }

    /// A **valid** frame followed by an unknown type is still a structural
    /// failure, and the valid frame is not returned — this is the
    /// assertion that separates parse-then-apply from a streaming
    /// parse-and-apply loop (§8.2).
    #[test]
    fn a_valid_frame_before_an_unknown_type_is_not_applied() {
        let mut bytes = Vec::new();
        Frame::Close(Close::new(9, b"bye")).encode(&mut bytes);
        bytes.push(0x3f);
        assert_eq!(parse(&bytes).unwrap_err(), Structural::UnknownType(0x3f));
    }

    /// A non-minimal varint type code is legal (§8.1: a receiver accepts
    /// any length), so a two-byte-encoded PING is a PING.
    #[test]
    fn a_non_minimally_encoded_type_code_still_parses() {
        assert_eq!(parse(&[0x40, 0x01]).unwrap(), vec![Frame::Ping]);
    }

    /// §8.3's ack-eliciting column, **every row** — including the eight
    /// types this slice cannot construct. A two-arm classifier is correct
    /// by accident for the whole of slice 3; this is what separates them.
    #[test]
    fn ack_eliciting_matches_the_whole_of_table_8_3() {
        let expected: &[(u64, bool)] = &[
            (constants::FRAME_PADDING, false),
            (constants::FRAME_PING, true),
            (constants::FRAME_ACK, false),
            (constants::FRAME_RESET_STREAM, true),
            (0x08, true),
            (0x09, true),
            (0x0a, true),
            (0x0b, true),
            (0x0c, true),
            (0x0d, true),
            (0x0e, true),
            (0x0f, true),
            (constants::FRAME_MAX_DATA, true),
            (constants::FRAME_MAX_STREAM_DATA, true),
            (constants::FRAME_MAX_STREAMS_BIDI, true),
            (constants::FRAME_MAX_STREAMS_UNI, true),
            (constants::FRAME_CLOSE, false),
            (constants::FRAME_DATAGRAM, true),
            (constants::FRAME_DATAGRAM_LEN, true),
        ];
        for (ty, want) in expected {
            assert_eq!(is_ack_eliciting(*ty), *want, "type {ty:#x}");
        }
    }

    /// §8.7's three classes, every row of §8.3.
    #[test]
    fn retransmission_classes_match_the_whole_of_table_8_3() {
        use Retransmission::*;
        let expected: &[(u64, Option<Retransmission>)] = &[
            (constants::FRAME_PADDING, Some(Never)),
            (constants::FRAME_PING, Some(Never)),
            (constants::FRAME_ACK, Some(Never)),
            (constants::FRAME_RESET_STREAM, Some(Regenerate)),
            (constants::FRAME_STOP_SENDING_RESERVED, None),
            (0x08, Some(Ranges)),
            (0x0f, Some(Ranges)),
            (constants::FRAME_MAX_DATA, Some(Regenerate)),
            (constants::FRAME_MAX_STREAM_DATA, Some(Regenerate)),
            (constants::FRAME_MAX_STREAMS_BIDI, Some(Regenerate)),
            (constants::FRAME_MAX_STREAMS_UNI, Some(Regenerate)),
            (constants::FRAME_CLOSE, Some(Never)),
            (constants::FRAME_DATAGRAM, Some(Never)),
            (constants::FRAME_DATAGRAM_LEN, Some(Never)),
            (0x77, None),
        ];
        for (ty, want) in expected {
            assert_eq!(retransmission(*ty), *want, "type {ty:#x}");
        }
    }

    /// A packet is ack-eliciting iff at least one of its frames is — so a
    /// CLOSE-and-PADDING packet is not, and adding a PING makes it so.
    #[test]
    fn packet_ack_eliciting_is_an_any_over_frames() {
        let quiet = vec![Frame::Close(Close::new(0, b"")), Frame::Padding];
        assert!(!packet_is_ack_eliciting(&quiet));

        let mut loud = quiet.clone();
        loud.push(Frame::Ping);
        assert!(packet_is_ack_eliciting(&loud));
    }

    /// §8.5's order, as the packer produces it: ACK, then CLOSE, then PING.
    #[test]
    fn packing_emits_ack_then_control_then_ping() {
        let mut packing = Packing::new();
        assert!(packing.ack(Ack {
            largest: 5,
            ack_delay: 0,
            first_range: 0,
            ranges: Vec::new()
        }));
        assert!(packing.control(Frame::Close(Close::new(0, b""))));
        assert!(packing.ping());

        let types: Vec<u64> = packing.frames().iter().map(Frame::type_code).collect();
        assert_eq!(
            types,
            vec![
                constants::FRAME_ACK,
                constants::FRAME_CLOSE,
                constants::FRAME_PING
            ]
        );

        let plaintext = packing.into_plaintext();
        let parsed = parse(&plaintext).unwrap();
        assert_eq!(
            parsed.iter().map(Frame::type_code).collect::<Vec<_>>(),
            types
        );
    }

    /// §8.6's budget: the packer refuses a frame that would not fit rather
    /// than truncating one.
    #[test]
    fn packing_refuses_a_frame_that_would_overrun_max_plaintext() {
        let mut packing = Packing::new();
        // Fill the budget with CLOSEs, then check the next one is refused
        // and the plaintext still fits.
        let mut accepted = 0;
        while packing.control(Frame::Close(Close::new(
            1,
            &[b'x'; constants::CLOSE_REASON_MAX],
        ))) {
            accepted += 1;
            assert!(accepted < 64, "the budget must bind");
        }
        assert!(accepted > 0);
        assert!(packing.into_plaintext().len() <= constants::MAX_PLAINTEXT);
    }
}

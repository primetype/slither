//! Every named constant `SPEC.md` fixes, and nothing else.
//!
//! This module is the single home for the spec's consolidated "Named
//! constants" table. A reviewer who finds a value in `SPEC.md` finds it
//! here under the same name, once — there are no crate-root re-exports and
//! no aliases, because two names for one wire value are two places for it
//! to drift.
//!
//! # What lives here, and what does not
//!
//! Constants only. **No functions, no enums, no derived implementation
//! details.** A predicate over the frame table (`is_stream_frame`) belongs
//! to the frame codec; `enum FrameType` and `enum ErrorCode` belong to the
//! modules that parse and emit them; a word count derived from
//! [`REPLAY_WINDOW`] is the private business of the replay window. The
//! consolidated table is the fence: **if the spec did not name it, it does
//! not live here.**
//!
//! # Types follow the wire
//!
//! Lengths that index a buffer are `usize`. Frame types and error codes are
//! `u64`, because they are varints (§8.1). Packet-type bytes and
//! [`VERSION`] are `u8`, because they are fixed-width header fields (§3.1).
//! Flow-control values are `u64`. In-memory caps are `usize`.
//!
//! # Timers carry a private integer companion
//!
//! Every [`Duration`] is built from a private `_MS: u64`. This is not
//! decoration: `Duration` has no `const` comparison operators, so the
//! timer-ordering assertions at the foot of this file would be impossible
//! without the integers. The `_MS` constants stay private; only the
//! `Duration`s are public surface.
//!
//! # The assertions are the pin
//!
//! Every value below that the spec states as a *derivation* rather than a
//! primitive is re-derived in a `const _: () = assert!(…)` at the foot of
//! this file. Those cannot be skipped, deferred or `#[ignore]`d: a wrong
//! value fails the **build**, not a test run.

use std::time::Duration;

// ═══════════════════════════════════════════════════════════════════════
// Identity and versioning (§3.1, §5.1)
// ═══════════════════════════════════════════════════════════════════════

/// The wire version byte carried by every packet header. §3.1.
pub const VERSION: u8 = 0x01;

/// The Noise prologue, mixed into the handshake hash. §5.1.
///
/// Its last byte **is** [`VERSION`], so a version change cannot leave the
/// prologue behind: a peer speaking a different version fails to
/// authenticate rather than half-negotiating.
pub const PROLOGUE: &[u8; 8] = b"slither\x01";

// ═══════════════════════════════════════════════════════════════════════
// Packet types (§3.1)
// ═══════════════════════════════════════════════════════════════════════

/// Packet type byte for the handshake initiation. §3.1.
pub const PKT_HANDSHAKE_INIT: u8 = 0x01;

/// Packet type byte for the handshake response. §3.1.
pub const PKT_HANDSHAKE_RESP: u8 = 0x02;

/// Packet type byte for a sealed data packet. §3.1.
pub const PKT_DATA: u8 = 0x03;

/// Reserved packet type, never sent and never accepted. §3.1.
pub const PKT_RESERVED_UNUSED: u8 = 0x04;

/// Reserved packet type for a future cookie/mac2 round. §3.1.
///
/// slither ships mac1 only; mac2 is not part of wire version 1, and this
/// byte exists so a later round can claim it without colliding.
pub const PKT_RESERVED_COOKIE: u8 = 0x05;

// ═══════════════════════════════════════════════════════════════════════
// Packet header sizes (§3.2, §3.3, §3.4)
// ═══════════════════════════════════════════════════════════════════════

/// Bytes in the handshake-initiation header, before the Noise message. §3.2.
pub const INIT_HEADER_LEN: usize = 6;

/// Bytes in the handshake-response header, before the Noise message. §3.3.
pub const RESP_HEADER_LEN: usize = 10;

/// Bytes in the data-packet header, before the sealed ciphertext. §3.4.
pub const DATA_HEADER_LEN: usize = 14;

// ═══════════════════════════════════════════════════════════════════════
// mac1, the day-one DoS gate (§4.1)
// ═══════════════════════════════════════════════════════════════════════

/// Domain-separation label for the mac1 keyed hash. §4.1.
pub const MAC1_LABEL: &[u8; 12] = b"slither mac1";

/// Bytes of keyed-BLAKE2b output carried as mac1. §4.1.
pub const MAC1_LEN: usize = 16;

// ═══════════════════════════════════════════════════════════════════════
// Handshake sizes (§2.3, §2.4, §5.2)
// ═══════════════════════════════════════════════════════════════════════

/// Bytes in the TAI64N timestamp inside the initiation. §5.2.
pub const TIMESTAMP_LEN: usize = 12;

/// Bytes of Noise payload carried in IK message 1 — the timestamp. §5.2.
pub const MSG1_PAYLOAD_LEN: usize = 12;

/// Bytes in an uncompressed SEC1 P-256 public point. §2.4.
///
/// Pinned to hiss's curve rather than to a literal: see the assertion at
/// the foot of this file.
pub const STATIC_PUBLIC_LEN: usize = 65;

/// Bytes of ChaCha20-Poly1305 authentication tag. §2.3.
pub const AEAD_TAG_LEN: usize = 16;

/// Bytes in the IK handshake's first Noise message. §2.3.
pub const IK_MSG1_LEN: usize = 174;

/// Bytes in the IK handshake's second Noise message. §2.3.
pub const IK_MSG2_LEN: usize = 81;

/// Bytes on the wire for a complete handshake-initiation packet. §2.3.
pub const INIT_PACKET_LEN: usize = 196;

/// Bytes on the wire for a complete handshake-response packet. §2.3.
pub const RESP_PACKET_LEN: usize = 107;

// ═══════════════════════════════════════════════════════════════════════
// Datagram sizing (§3.5)
// ═══════════════════════════════════════════════════════════════════════

/// The largest UDP payload slither will ever send. §3.5.
pub const MAX_DATAGRAM: usize = 1200;

/// The largest sealed plaintext a data packet can carry. §3.5.
pub const MAX_PLAINTEXT: usize = 1170;

// ═══════════════════════════════════════════════════════════════════════
// Session (§7.2, §7.7)
// ═══════════════════════════════════════════════════════════════════════

/// Messages sent within one epoch before slither ratchets. §7.7.
///
/// slither's choice, handed to hiss's `into_datagram_with_epoch`.
pub const REKEY_EPOCH_MSGS: u64 = 65_536;

/// The furthest ahead a received counter's epoch may be. §7.7.
///
/// Fixed by hiss, not by slither: see the assertion at the foot of this
/// file.
pub const MAX_EPOCH_JUMP: u64 = 2;

/// The anti-replay window, **in bits**. §7.2.
pub const REPLAY_WINDOW: usize = 2048;

// ═══════════════════════════════════════════════════════════════════════
// Frame types (§8.3)
// ═══════════════════════════════════════════════════════════════════════

/// PADDING frame type. §8.3.
pub const FRAME_PADDING: u64 = 0x00;

/// PING frame type. §8.3.
pub const FRAME_PING: u64 = 0x01;

/// ACK frame type. §8.3.
pub const FRAME_ACK: u64 = 0x02;

/// RESET_STREAM frame type. §8.3.
pub const FRAME_RESET_STREAM: u64 = 0x04;

/// STOP_SENDING frame type — **reserved, never sent**. §8.3, §19.
///
/// The STOP_SENDING round is deferred; the type byte is claimed so that
/// round cannot collide with anything shipped in wire version 1.
pub const FRAME_STOP_SENDING_RESERVED: u64 = 0x05;

/// The lowest STREAM frame type — all three flag bits clear. §8.3.
pub const FRAME_STREAM_BASE: u64 = 0x08;

/// The highest STREAM frame type — all three flag bits set. §8.3.
pub const FRAME_STREAM_MAX: u64 = 0x0f;

/// MAX_DATA frame type. §8.3.
pub const FRAME_MAX_DATA: u64 = 0x10;

/// MAX_STREAM_DATA frame type. §8.3.
pub const FRAME_MAX_STREAM_DATA: u64 = 0x11;

/// MAX_STREAMS (bidirectional) frame type. §8.3.
pub const FRAME_MAX_STREAMS_BIDI: u64 = 0x12;

/// MAX_STREAMS (unidirectional) frame type. §8.3.
pub const FRAME_MAX_STREAMS_UNI: u64 = 0x13;

/// PATH_CHALLENGE frame type. §8.3, §7.3.
///
/// **[RATIFIED 2026/08/16 — ruling 208]** §7.3's return-routability
/// challenge: eight opaque bytes, drawn per arming from the connection's
/// §16.6 sub-seed. `0x1a`/`0x1b` are QUIC's own code points for these two
/// frames.
pub const FRAME_PATH_CHALLENGE: u64 = 0x1a;

/// PATH_RESPONSE frame type. §8.3, §7.3.
///
/// **[RATIFIED 2026/08/16 — ruling 208]** [`FRAME_PATH_CHALLENGE`]'s eight
/// bytes, echoed verbatim. A response matching the outstanding challenge is
/// what disarms §7.3's budget.
pub const FRAME_PATH_RESPONSE: u64 = 0x1b;

/// CLOSE frame type. §8.3.
pub const FRAME_CLOSE: u64 = 0x1c;

/// DATAGRAM frame type, without an explicit length. §8.3.
pub const FRAME_DATAGRAM: u64 = 0x30;

/// DATAGRAM frame type, with an explicit length. §8.3.
pub const FRAME_DATAGRAM_LEN: u64 = 0x31;

// ═══════════════════════════════════════════════════════════════════════
// STREAM frame flag bits (§8.4)
// ═══════════════════════════════════════════════════════════════════════

/// STREAM flag: an explicit offset field is present. §8.4.
pub const STREAM_OFF: u64 = 0x04;

/// STREAM flag: an explicit length field is present. §8.4.
pub const STREAM_LEN: u64 = 0x02;

/// STREAM flag: this frame carries the end of the stream. §8.4.
pub const STREAM_FIN: u64 = 0x01;

/// Every STREAM flag bit, or-ed together. §8.4.
pub const STREAM_FLAG_MASK: u64 = 0x07;

// ═══════════════════════════════════════════════════════════════════════
// Flow control (§10.2, §10.3, §10.4, §10.6)
//
// **[RATIFIED 2026/08/15 — ruling 103]** These are *three kinds of thing*
// and §10.2's table groups them as one. Each constant below says which kind
// it is, because the kind decides what changing it costs:
//
// - **wire constant** — unnegotiated, so both ends must assume the same
//   value; changing one corrupts the peer's accounting immediately. A red
//   pin in `tests/spec_constants.rs` needs a ruling, not an updated
//   expectation.
// - **receiver policy, invisible** — two peers running different values
//   interoperate perfectly and neither can tell. A local tuning knob.
// - **receiver policy, observable** — still policy, but a peer *can* tell,
//   because behaviour past one receiver's ceiling differs from behaviour
//   past another's. Shipped ratified-but-revisitable, and a tolerance.
//
// No value and no location moves: moving them is wire-pin churn for
// nothing. Only the kind is stated.
// ═══════════════════════════════════════════════════════════════════════

/// Initial connection-level receive credit, in bytes. §10.2.
///
/// **Wire constant** (ruling 103).
pub const INITIAL_MAX_DATA: u64 = 1_048_576;

/// Initial per-stream receive credit, in bytes. §10.2.
///
/// **Wire constant** (ruling 103).
pub const INITIAL_MAX_STREAM_DATA: u64 = 262_144;

/// Initial **cumulative** bidirectional stream allowance. §10.2.
///
/// **Wire constant** (ruling 103).
pub const INITIAL_MAX_STREAMS_BIDI: u64 = 32;

/// Initial **cumulative** unidirectional stream allowance. §10.2.
///
/// **Wire constant** (ruling 103).
pub const INITIAL_MAX_STREAMS_UNI: u64 = 128;

/// Closed streams that must accumulate before a MAX_STREAMS is sent. §10.4.
///
/// **Receiver policy, invisible** (ruling 103) — §10.2's own next clause
/// calls later credit *"receiver policy"*, and this is when a receiver
/// *chooses* to advertise. It is **not** an initial window and not
/// wire-relevant at all.
///
/// **[RATIFIED 2026/08/15 — ruling 102]** §10.4's *two* triggers both use
/// this constant: *"when ≥ `STREAMS_CREDIT_BATCH` grants are unadvertised,
/// **or** when the peer's remaining allowance drops to ≤ 8"* — the second is
/// written as a literal and is the same value. There is no second constant,
/// and no code path here writes a bare `8`.
pub const STREAMS_CREDIT_BATCH: u64 = 8;

/// Re-grant credit once this fraction of the window is consumed — the
/// divisor, so `2` means "half the window". §10.3.
///
/// **Receiver policy, invisible** — by the same argument as
/// [`STREAMS_CREDIT_BATCH`]: two peers re-granting at different fractions
/// interoperate perfectly, and neither can observe the other's. Ruling 103
/// classifies §10.2's five rows and §10.6's ceiling and does **not** reach
/// this one, which sits in §10.3's named-constants table beside them; the
/// classification is stated here so the next person to tune it knows which
/// kind of change it is. See `.slices/04-streams/IMPLEMENTATION-4a.md`.
pub const CREDIT_REGRANT_DIVISOR: u64 = 2;

/// The largest message the receiver will reassemble, in bytes. §9.8.
///
/// Equal to [`INITIAL_MAX_STREAM_DATA`] by construction: a message larger
/// than one stream's credit could never arrive whole.
pub const MESSAGE_RECV_MAX: u64 = 262_144;

/// Distinct out-of-order chunks the reassembler will hold per stream. §10.6.
///
/// **Receiver policy, observable** (ruling 103) — the third kind, and the
/// one §10.2's table has no row for. A peer that fragments past one
/// receiver's ceiling is killed and past another's is not, so it is
/// externally visible without being a wire constant. It is a **tolerance**,
/// which is also why §10.5's *"There is no tolerance band; the limits are
/// exact"* is true of the two violations §10.5 lists and false of this, the
/// third (ruling 104).
pub const REASSEMBLY_CHUNKS_MAX: usize = 1024;

/// The conforming STREAM-frame size §10.6's **credit-derived** reassembly
/// ceiling divides the advertised stream window by. §10.6.
///
/// **[RATIFIED 2026/08/18 — ruling 270]** [`REASSEMBLY_CHUNKS_MAX`] is the
/// **floor** of that ceiling, not the ceiling itself: a receiver tolerates
/// `max(REASSEMBLY_CHUNKS_MAX, window / REASSEMBLY_MIN_CONFORMING_FRAME + 1)`
/// stored discontiguous ranges per stream. The flat 1 024 is *stricter* than
/// §10.6's own mandate — *"per-stream reassembly state MUST be O(advertised
/// credit)"* — and the strictness is what killed conforming peers at a raised
/// window: a stream's credit and its tolerated hole count were set by two
/// constants that did not scale together, so a sender inside its credit, on a
/// path that lost packets in the pattern a saturated receive socket produces,
/// exceeded the second while obeying the first.
///
/// **Why a packet-scale divisor, and why this one.** Dividing by a frame size
/// is what separates the honest case (holes ≤ window ÷ frame size) from the
/// adversarial one (one-byte frames at alternating offsets, ≤ credit ÷ 2):
/// the flood the ceiling exists to kill is **512× above** the derived value
/// and still dies. The value is one fill quantum's worth of stream data —
/// deliberately its **own** constant and not a reference to
/// `frame::STREAM_FILL_QUANTUM`, which is §8.5's implementation-defined
/// round-robin quantum, is kept out of this table on purpose, and is the
/// **local sender's** choice rather than anything a receiver may derive a
/// policy from. They agree at 1 024 today, and this is the receiver's own
/// number.
///
/// **Receiver policy, observable** — the same third kind as
/// [`REASSEMBLY_CHUNKS_MAX`] (ruling 103), and for the same reason: no wire
/// byte moves, and a peer that fragments past one receiver's ceiling is
/// killed and past another's is not.
pub const REASSEMBLY_MIN_CONFORMING_FRAME: u64 = 1024;

// ═══════════════════════════════════════════════════════════════════════
// Unreliable datagrams (§11.2, §11.3)
// ═══════════════════════════════════════════════════════════════════════

/// The largest application datagram that fits one packet. §11.2.
pub const MAX_DATAGRAM_PAYLOAD: usize = 1169;

/// Outbound datagrams buffered before the oldest is dropped. §11.3.
pub const DATAGRAM_SEND_QUEUE: usize = 64;

/// Inbound datagrams buffered before the oldest is dropped. §11.3.
pub const DATAGRAM_RECV_QUEUE: usize = 64;

// ═══════════════════════════════════════════════════════════════════════
// Close (§8.4, §15.1)
// ═══════════════════════════════════════════════════════════════════════

/// The largest CLOSE reason phrase, in bytes. §8.4.
pub const CLOSE_REASON_MAX: usize = 256;

/// How long a closing endpoint keeps answering with CLOSE. §15.1.
pub const CLOSE_LINGER: Duration = Duration::from_millis(CLOSE_LINGER_MS);

/// The floor between two CLOSE replies — at most one per second. §15.1.
pub const CLOSE_REPLY_MIN_INTERVAL: Duration = Duration::from_millis(CLOSE_REPLY_MIN_INTERVAL_MS);

// ═══════════════════════════════════════════════════════════════════════
// Acknowledgement (§12.2, §12.4)
// ═══════════════════════════════════════════════════════════════════════

/// The most ranges one ACK frame will carry. §12.2.
pub const MAX_ACK_RANGES: usize = 64;

/// Send an ACK immediately on every Nth ack-eliciting packet. §12.4.
pub const ACK_ELICITING_PER_ACK: u64 = 2;

/// The longest an ACK may be deferred. §12.4, §13.3.
pub const MAX_ACK_DELAY: Duration = Duration::from_millis(MAX_ACK_DELAY_MS);

// ═══════════════════════════════════════════════════════════════════════
// Loss recovery (§13.1, §13.2, §13.3)
// ═══════════════════════════════════════════════════════════════════════

/// Packet-reordering threshold before a packet is declared lost. §13.2.
pub const K_PACKET_THRESHOLD: u64 = 3;

/// Numerator of the time-reordering threshold, `9⁄8`. §13.2.
pub const K_TIME_THRESHOLD_NUM: u32 = 9;

/// Denominator of the time-reordering threshold, `9⁄8`. §13.2.
pub const K_TIME_THRESHOLD_DEN: u32 = 8;

/// The timer granularity floor. §13.2.
pub const K_GRANULARITY: Duration = Duration::from_millis(K_GRANULARITY_MS);

/// The RTT assumed before any sample has been taken. §13.1.
pub const K_INITIAL_RTT: Duration = Duration::from_millis(K_INITIAL_RTT_MS);

/// The cap on the PTO backoff **multiplier**, `2³`. §13.3.
///
/// **[RATIFIED 2026/08/17 — ruling 254]** 2⁶ until this ruling, inherited
/// from RFC 9002 practice and never itself ratified. The cap is **sized to
/// `DEAD_TIMEOUT`'s window, not to overflow**: at 2⁶ the later rungs could
/// not fire inside 25 s at any warm RTT, so the probe train's tail turned
/// from probing into waiting exactly when survival was the question —
/// measured at 50 % sustained loss, transfers that timed out at 2⁶
/// complete at 2³, at zero observed honest-path cost.
///
/// **The compile-time pins below are load-bearing at this value**, not
/// belt-and-braces. At 64 the mis-transcribed idiom `1u32 << 64` was
/// undefined behaviour and loud; at 8, `1u32 << 8` is a legal 256 and
/// silently wrong — an eight-fold error in a timer with no red test.
pub const PTO_BACKOFF_CAP: u32 = 8;

// ═══════════════════════════════════════════════════════════════════════
// Congestion control (§14.2, §14.4)
// ═══════════════════════════════════════════════════════════════════════

/// Initial congestion window, in bytes. §14.
///
/// RFC 9002's `kInitialWindow` evaluated at slither's MTU: `10 ×`
/// [`MAX_DATAGRAM`].
pub const INITIAL_WINDOW: u64 = 12_000;

/// The floor the congestion window never drops below, in bytes. §14.
///
/// RFC 9002's `kMinimumWindow`: `2 ×` [`MAX_DATAGRAM`].
pub const MINIMUM_WINDOW: u64 = 2_400;

/// The factor the window is multiplied by on loss. §14.2.
///
/// **Implementation note.** This constant is the spec's *statement*; the
/// arithmetic is an integer halving. Halve with `cwnd / 2`, never
/// `(cwnd as f64 * LOSS_REDUCTION_FACTOR) as u64` — the float is here to
/// match §14.2's notation, not to be multiplied by.
pub const LOSS_REDUCTION_FACTOR: f64 = 0.5;

/// Consecutive PTO periods without an ack that mean persistent
/// congestion. §14.4.
pub const PERSISTENT_CONGESTION_THRESHOLD: u32 = 3;

// ═══════════════════════════════════════════════════════════════════════
// Handshake timers (§5.5)
// ═══════════════════════════════════════════════════════════════════════

/// The base interval between handshake retransmissions. §5.5.
pub const RETRANSMIT_BASE: Duration = Duration::from_millis(RETRANSMIT_BASE_MS);

/// The largest uniform jitter added to [`RETRANSMIT_BASE`]. §5.5.
pub const RETRANSMIT_JITTER_MAX: Duration = Duration::from_millis(RETRANSMIT_JITTER_MAX_MS);

/// How long a handshake is retried before it gives up. §5.5.
pub const HANDSHAKE_GIVEUP: Duration = Duration::from_millis(HANDSHAKE_GIVEUP_MS);

// ═══════════════════════════════════════════════════════════════════════
// Liveness timers (§7.5)
// ═══════════════════════════════════════════════════════════════════════

/// Silence after which a keepalive is sent. §7.5.
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_millis(KEEPALIVE_TIMEOUT_MS);

/// Silence after which the connection is declared dead. §7.5.
pub const DEAD_TIMEOUT: Duration = Duration::from_millis(DEAD_TIMEOUT_MS);

/// The default persistent-keepalive interval. §7.5.
pub const PERSISTENT_KEEPALIVE_DEFAULT: Duration =
    Duration::from_millis(PERSISTENT_KEEPALIVE_DEFAULT_MS);

/// The **inclusive** floor of the admissible persistent-keepalive
/// interval. §7.5.
///
/// The ceiling has no constant of its own: it is [`DEAD_TIMEOUT`],
/// exclusive, and the validator compares against that directly so the two
/// cannot drift.
pub const PERSISTENT_KEEPALIVE_MIN: Duration = Duration::from_millis(PERSISTENT_KEEPALIVE_MIN_MS);

// ═══════════════════════════════════════════════════════════════════════
// Anti-amplification and the introduction queue (§6.3, §7.3, §17.1)
// ═══════════════════════════════════════════════════════════════════════

/// Bytes sendable per byte received from an unvalidated address. §7.3.
pub const AMPLIFICATION_FACTOR: u64 = 3;

/// The most parked introductions held at once. §6.3.
pub const INTRO_QUEUE_CAP: usize = 1024;

/// The most parked introductions held for one source address. §6.3.
pub const INTRO_MAX_PER_SOURCE: usize = 4;

/// How long a parked introduction survives before it expires. §6.3.
pub const INTRO_TTL: Duration = Duration::from_millis(INTRO_TTL_MS);

/// The most orphaned timestamp-guard entries retained. §17.1.
pub const TS_GUARD_ORPHAN_CAP: usize = 1024;

/// How long an orphaned timestamp-guard entry survives before aging out.
/// §17.1, ruling 70.
///
/// **An alias of [`INTRO_TTL`], deliberately — not a second literal.** §17.1
/// specified this only as prose ("an `INTRO_TTL`-scale timer"), which is a
/// normative value an implementer cannot write down without inventing an
/// identifier, and inventing one means choosing whether it is *exactly*
/// `INTRO_TTL` or merely near it. Ruling 70 names it and binds it, so there
/// is one place the value lives and the two cannot drift apart — the same
/// reasoning that made ruling 63 name a default and leave a ceiling as a
/// comparison rather than a second copy of the number.
pub const TS_GUARD_ORPHAN_TTL: Duration = INTRO_TTL;

// ═══════════════════════════════════════════════════════════════════════
// Shell (§16.5)
// ═══════════════════════════════════════════════════════════════════════

/// The bound on how late the shell may fire a core deadline — the spec
/// calls it `L`. §16.5.
pub const SHELL_LATENESS_BOUND: Duration = Duration::from_millis(SHELL_LATENESS_BOUND_MS);

// ═══════════════════════════════════════════════════════════════════════
// Transport error codes (§15.3)
// ═══════════════════════════════════════════════════════════════════════
//
// The registry is `0x00`–`0x06` transport, `0x07`–`0x0f` **reserved**, and
// `≥ 0x10` application. The reserved gap deliberately gets no constants: a
// `RESERVED_*` name for each would invite one to be sent.

/// No error — a graceful close. §15.3.
pub const NO_ERROR: u64 = 0x00;

/// The peer violated the protocol. §15.3.
pub const PROTOCOL_VIOLATION: u64 = 0x01;

/// The peer exceeded a flow-control limit. §15.3.
pub const FLOW_CONTROL_ERROR: u64 = 0x02;

/// The peer exceeded a stream limit. §15.3.
pub const STREAM_LIMIT_ERROR: u64 = 0x03;

/// A frame arrived for a stream in a state that forbids it. §15.3.
pub const STREAM_STATE_ERROR: u64 = 0x04;

/// The peer contradicted a stream's final size. §15.3.
pub const FINAL_SIZE_ERROR: u64 = 0x05;

/// A message exceeded [`MESSAGE_RECV_MAX`]. §15.3.
pub const MESSAGE_OVERFLOW: u64 = 0x06;

/// The lowest code an application may use. §15.3.
///
/// Everything below it is transport-owned: `0x00`–`0x06` is the registry
/// above, and `0x07`–`0x0f` is reserved and never sent.
pub const APPLICATION_ERROR_BASE: u64 = 0x10;

// ═══════════════════════════════════════════════════════════════════════
// Private millisecond companions
// ═══════════════════════════════════════════════════════════════════════
//
// `Duration` has no `const` comparison operators, so the timer-ordering
// assertions below are impossible without these. They stay private: the
// public surface is the `Duration`s.

const CLOSE_LINGER_MS: u64 = 5_000;
const CLOSE_REPLY_MIN_INTERVAL_MS: u64 = 1_000;
const MAX_ACK_DELAY_MS: u64 = 25;
const K_GRANULARITY_MS: u64 = 1;
const K_INITIAL_RTT_MS: u64 = 333;
const RETRANSMIT_BASE_MS: u64 = 5_000;
const RETRANSMIT_JITTER_MAX_MS: u64 = 333;
const HANDSHAKE_GIVEUP_MS: u64 = 90_000;
const KEEPALIVE_TIMEOUT_MS: u64 = 10_000;
const DEAD_TIMEOUT_MS: u64 = 25_000;
const PERSISTENT_KEEPALIVE_DEFAULT_MS: u64 = 10_000;
const PERSISTENT_KEEPALIVE_MIN_MS: u64 = 1_000;
const INTRO_TTL_MS: u64 = 15_000;
const SHELL_LATENESS_BOUND_MS: u64 = 250;

// ═══════════════════════════════════════════════════════════════════════
// Compile-time assertions
// ═══════════════════════════════════════════════════════════════════════
//
// Every value above that the spec states as a derivation is re-derived
// here. These are the slice-0 reading of the "wire pins" gate: until the
// golden-wire vectors land, they are what makes a mis-transcribed size a
// build failure rather than a silent wire bug.

// ── Identity and versioning ────────────────────────────────────────────
const _: () = assert!(PROLOGUE.len() == 8);
const _: () = assert!(PROLOGUE[7] == VERSION); // §5.1 pins the tail
const _: () = assert!(MAC1_LABEL.len() == 12); // §4.1

// ── Suite-derived sizes (§2.3) ────────────────────────────────────────
// The one assertion that reaches outside slither: it pins slither's
// handshake arithmetic to hiss's curve, so a hiss change that moved the
// point encoding turns the BUILD red, not a test.
const _: () =
    assert!(STATIC_PUBLIC_LEN == <hiss::curve::p256::P256 as hiss::curve::Curve>::PUBLIC_KEY_SIZE);
const _: () = assert!(MSG1_PAYLOAD_LEN == TIMESTAMP_LEN);
const _: () = assert!(
    IK_MSG1_LEN
        == STATIC_PUBLIC_LEN
            + (STATIC_PUBLIC_LEN + AEAD_TAG_LEN)
            + (MSG1_PAYLOAD_LEN + AEAD_TAG_LEN)
);
const _: () = assert!(IK_MSG2_LEN == STATIC_PUBLIC_LEN + AEAD_TAG_LEN);
const _: () = assert!(INIT_PACKET_LEN == INIT_HEADER_LEN + IK_MSG1_LEN + MAC1_LEN);
const _: () = assert!(RESP_PACKET_LEN == RESP_HEADER_LEN + IK_MSG2_LEN + MAC1_LEN);
const _: () = assert!(INIT_PACKET_LEN <= MAX_DATAGRAM); // must not fragment
const _: () = assert!(RESP_PACKET_LEN <= MAX_DATAGRAM);

// ── Data-path sizes (§3.5, §11.2) ─────────────────────────────────────
const _: () = assert!(MAX_PLAINTEXT == MAX_DATAGRAM - DATA_HEADER_LEN - AEAD_TAG_LEN);
const _: () = assert!(MAX_DATAGRAM_PAYLOAD == MAX_PLAINTEXT - 1);
// A CLOSE with a maximum reason must fit one packet (§8.4, §15.1):
// 1 type byte + worst-case 8-byte varint code + 8-byte varint len + reason.
const _: () = assert!(1 + 8 + 8 + CLOSE_REASON_MAX <= MAX_PLAINTEXT);
// §11.3's own parenthetical: "≈ 73 KiB worst case each".
const _: () = assert!(DATAGRAM_SEND_QUEUE * MAX_DATAGRAM_PAYLOAD < 80 * 1024);
const _: () = assert!(DATAGRAM_RECV_QUEUE * MAX_DATAGRAM_PAYLOAD < 80 * 1024);

// ── Frame grammar (§8.3, §8.4) ────────────────────────────────────────
const _: () = assert!(STREAM_FLAG_MASK == STREAM_OFF | STREAM_LEN | STREAM_FIN);
const _: () = assert!(FRAME_STREAM_MAX == FRAME_STREAM_BASE | STREAM_FLAG_MASK);
const _: () = assert!(FRAME_STREAM_BASE & STREAM_FLAG_MASK == 0); // flags don't collide
const _: () = assert!(FRAME_DATAGRAM_LEN == FRAME_DATAGRAM | 0x01);

// ── Flow control (§10.2, §9.8) ────────────────────────────────────────
const _: () = assert!(MESSAGE_RECV_MAX == INITIAL_MAX_STREAM_DATA);
const _: () = assert!(INITIAL_MAX_STREAM_DATA <= INITIAL_MAX_DATA);
const _: () = assert!(INITIAL_MAX_DATA <= crate::varint::VarInt::MAX_VALUE);
const _: () = assert!(INITIAL_MAX_STREAM_DATA <= crate::varint::VarInt::MAX_VALUE);
const _: () = assert!(MESSAGE_RECV_MAX <= crate::varint::VarInt::MAX_VALUE);

// ── Session (§7.2, §7.7) ──────────────────────────────────────────────
const _: () = assert!(REPLAY_WINDOW.is_multiple_of(64)); // whole u64 words
const _: () = assert!(MAX_EPOCH_JUMP == hiss::noise::datagram::MAX_EPOCH_JUMP);

// ── Recovery and congestion (§13, §14) ────────────────────────────────
const _: () = assert!(PTO_BACKOFF_CAP == 1 << 3); // the spec writes 2³ [ruling 254]
const _: () = assert!(INITIAL_WINDOW == 10 * MAX_DATAGRAM as u64); // RFC 9002
const _: () = assert!(MINIMUM_WINDOW == 2 * MAX_DATAGRAM as u64); // RFC 9002
const _: () = assert!(MINIMUM_WINDOW < INITIAL_WINDOW);
const _: () = assert!(K_TIME_THRESHOLD_NUM > K_TIME_THRESHOLD_DEN); // > 1, or it's not a threshold

// ── Timer ordering (§5.5, §6.3, §7.5, §12.4, §13.2) ───────────────────
// These are the reason the private `_MS` companions exist.
const _: () = assert!(KEEPALIVE_TIMEOUT_MS < DEAD_TIMEOUT_MS);
const _: () = assert!(MAX_ACK_DELAY_MS < KEEPALIVE_TIMEOUT_MS);
const _: () = assert!(K_GRANULARITY_MS <= MAX_ACK_DELAY_MS);

// ---------------------------------------------------------------------
// Derivations the spec states in prose notation, added after slice 0's
// fidelity review. Ruling 63 named the hazard: a value written "2³" or
// "65 536 (2¹⁶)" is a judgement call an implementer can resolve wrongly
// and *self-consistently*, so nothing turns red. `PTO_BACKOFF_CAP` was
// already guarded; `REKEY_EPOCH_MSGS` is the identical shape and was not.
//
// **[ruling 254]** Ruling 63's example was `PTO_BACKOFF_CAP` at 2⁶, where
// the wrong resolution — storing the *exponent* and shifting by it —
// produced `1u32 << 64`, undefined behaviour that no build hides. At 2³ it
// produces `1u32 << 8` = 256, a perfectly legal multiplier eight times too
// large, on a timer whose only symptom is a probe train that thins. The
// guard is what turns red now; nothing else would.
const _: () = assert!(REKEY_EPOCH_MSGS == 1 << 16); // §7.7 "65 536 (2¹⁶)"

// A close reply may not be rate-limited more slowly than the linger it
// runs inside, or the linger would expire with replies still owed (§15.1).
const _: () = assert!(CLOSE_REPLY_MIN_INTERVAL_MS < CLOSE_LINGER_MS);

// The per-source cap is a share of the queue, not a second, larger bound
// (§6.3) — inverting them would make the per-source limit unreachable.
const _: () = assert!(INTRO_MAX_PER_SOURCE <= INTRO_QUEUE_CAP);

// §7.3's budget must admit at least one response to one initiation, or a
// responder could never answer an unvalidated address at all.
const _: () = assert!(AMPLIFICATION_FACTOR as usize * INIT_PACKET_LEN >= RESP_PACKET_LEN);
// **[ruling 208]** The challenge must fit inside the budget its own arming
// creates, or an address roamed to by a bare keepalive could never be
// validated at all. The smallest arming credit is §3.4's empty-plaintext
// keepalive — a 30-byte datagram — so `AMPLIFICATION_FACTOR ×` it must
// cover one datagram carrying a `PATH_CHALLENGE`: 14 B of header, 1 B of
// type code, 8 B of challenge and a 16 B tag. This is what fails if anyone
// later grows `DATA_HEADER_LEN`, shrinks `AMPLIFICATION_FACTOR`, or widens
// the challenge past eight bytes.
const _: () = assert!(
    AMPLIFICATION_FACTOR as usize * (DATA_HEADER_LEN + AEAD_TAG_LEN)
        >= DATA_HEADER_LEN + 1 + 8 + AEAD_TAG_LEN
);
const _: () = assert!(RETRANSMIT_BASE_MS + RETRANSMIT_JITTER_MAX_MS < HANDSHAKE_GIVEUP_MS);
const _: () = assert!(INTRO_TTL_MS < HANDSHAKE_GIVEUP_MS);
const _: () = assert!(PERSISTENT_KEEPALIVE_MIN_MS <= PERSISTENT_KEEPALIVE_DEFAULT_MS);
const _: () = assert!(PERSISTENT_KEEPALIVE_DEFAULT_MS < DEAD_TIMEOUT_MS); // ruling 40's ceiling

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Duration` in the table is a whole number of milliseconds, so
    /// tokio's paused clock can land exactly on each deadline. A
    /// sub-millisecond timer would make §16.10's tests flaky by
    /// construction.
    #[test]
    fn timers_are_expressible_in_virtual_time() {
        let timers: &[(&str, Duration)] = &[
            ("CLOSE_LINGER", CLOSE_LINGER),
            ("CLOSE_REPLY_MIN_INTERVAL", CLOSE_REPLY_MIN_INTERVAL),
            ("MAX_ACK_DELAY", MAX_ACK_DELAY),
            ("K_GRANULARITY", K_GRANULARITY),
            ("K_INITIAL_RTT", K_INITIAL_RTT),
            ("RETRANSMIT_BASE", RETRANSMIT_BASE),
            ("RETRANSMIT_JITTER_MAX", RETRANSMIT_JITTER_MAX),
            ("HANDSHAKE_GIVEUP", HANDSHAKE_GIVEUP),
            ("KEEPALIVE_TIMEOUT", KEEPALIVE_TIMEOUT),
            ("DEAD_TIMEOUT", DEAD_TIMEOUT),
            ("PERSISTENT_KEEPALIVE_DEFAULT", PERSISTENT_KEEPALIVE_DEFAULT),
            ("PERSISTENT_KEEPALIVE_MIN", PERSISTENT_KEEPALIVE_MIN),
            ("INTRO_TTL", INTRO_TTL),
            ("SHELL_LATENESS_BOUND", SHELL_LATENESS_BOUND),
        ];

        for (name, d) in timers {
            assert_eq!(
                d.subsec_nanos() % 1_000_000,
                0,
                "{name} is not a whole number of milliseconds: {d:?}"
            );
            assert!(!d.is_zero(), "{name} is zero");
        }
    }

    /// Every frame type is distinct, and nothing but the STREAM range
    /// itself falls inside `FRAME_STREAM_BASE..=FRAME_STREAM_MAX`.
    #[test]
    fn frame_types_are_distinct() {
        let frames: &[(&str, u64)] = &[
            ("FRAME_PADDING", FRAME_PADDING),
            ("FRAME_PING", FRAME_PING),
            ("FRAME_ACK", FRAME_ACK),
            ("FRAME_RESET_STREAM", FRAME_RESET_STREAM),
            ("FRAME_STOP_SENDING_RESERVED", FRAME_STOP_SENDING_RESERVED),
            ("FRAME_STREAM_BASE", FRAME_STREAM_BASE),
            ("FRAME_STREAM_MAX", FRAME_STREAM_MAX),
            ("FRAME_MAX_DATA", FRAME_MAX_DATA),
            ("FRAME_MAX_STREAM_DATA", FRAME_MAX_STREAM_DATA),
            ("FRAME_MAX_STREAMS_BIDI", FRAME_MAX_STREAMS_BIDI),
            ("FRAME_MAX_STREAMS_UNI", FRAME_MAX_STREAMS_UNI),
            // **[ruling 208]** §8.3's two new rows. Nothing fails if this
            // table misses a type — which is exactly what makes an omission
            // here a defect rather than a red test.
            ("FRAME_PATH_CHALLENGE", FRAME_PATH_CHALLENGE),
            ("FRAME_PATH_RESPONSE", FRAME_PATH_RESPONSE),
            ("FRAME_CLOSE", FRAME_CLOSE),
            ("FRAME_DATAGRAM", FRAME_DATAGRAM),
            ("FRAME_DATAGRAM_LEN", FRAME_DATAGRAM_LEN),
        ];
        assert_eq!(frames.len(), 16, "the frame table has 16 named types");

        for (i, (name_a, a)) in frames.iter().enumerate() {
            for (name_b, b) in &frames[i + 1..] {
                assert_ne!(a, b, "{name_a} and {name_b} collide at {a:#04x}");
            }
        }

        // Nothing outside the STREAM range's own two endpoints may land
        // inside it: a frame type there would be indistinguishable from a
        // STREAM frame with some flag combination.
        for (name, t) in frames {
            let in_stream_range = (FRAME_STREAM_BASE..=FRAME_STREAM_MAX).contains(t);
            let is_stream_endpoint = *t == FRAME_STREAM_BASE || *t == FRAME_STREAM_MAX;
            assert_eq!(
                in_stream_range, is_stream_endpoint,
                "{name} ({t:#04x}) collides with the STREAM range"
            );
        }
    }

    /// The seven transport error codes are unique, contiguous `0x00..=0x06`
    /// and below [`APPLICATION_ERROR_BASE`]; `0x07..0x10` is the reserved
    /// gap and no constant occupies it.
    ///
    /// This is the standing guard against a restatement of the registry
    /// drifting from the registry — the exact failure that had to be
    /// corrected in three places on 2026/08/14.
    #[test]
    fn error_codes_are_distinct_and_below_the_application_base() {
        let codes: &[(&str, u64)] = &[
            ("NO_ERROR", NO_ERROR),
            ("PROTOCOL_VIOLATION", PROTOCOL_VIOLATION),
            ("FLOW_CONTROL_ERROR", FLOW_CONTROL_ERROR),
            ("STREAM_LIMIT_ERROR", STREAM_LIMIT_ERROR),
            ("STREAM_STATE_ERROR", STREAM_STATE_ERROR),
            ("FINAL_SIZE_ERROR", FINAL_SIZE_ERROR),
            ("MESSAGE_OVERFLOW", MESSAGE_OVERFLOW),
        ];
        assert_eq!(codes.len(), 7, "the transport registry has seven codes");

        // Contiguous 0x00..=0x06, in order, each below the application base.
        for (i, (name, code)) in codes.iter().enumerate() {
            assert_eq!(*code, i as u64, "{name} is not at {i:#04x}");
            assert!(
                *code < APPLICATION_ERROR_BASE,
                "{name} is not below APPLICATION_ERROR_BASE"
            );
        }

        // The reserved gap is 0x07..0x10 and it is a gap: nothing named
        // occupies it, so nothing named can be sent from it.
        assert_eq!(APPLICATION_ERROR_BASE, 0x10);
        for (name, code) in codes {
            assert!(
                !(0x07..APPLICATION_ERROR_BASE).contains(code),
                "{name} falls in the reserved gap"
            );
        }
    }

    /// A belt-and-braces runtime mirror of the `MAX_EPOCH_JUMP` const
    /// assertion. The const assertion holds (hiss's constant is
    /// `const`-reachable under `default-features = false`), so this is
    /// redundant on purpose: if a future hiss made the constant
    /// non-const-reachable and the assertion had to be dropped, this test
    /// would become the only pin.
    #[test]
    fn epoch_jump_matches_hiss() {
        assert_eq!(MAX_EPOCH_JUMP, hiss::noise::datagram::MAX_EPOCH_JUMP);
    }
}

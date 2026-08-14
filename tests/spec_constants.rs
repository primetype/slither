//! Conformance fence over `slither::constants`.
//!
//! Every constant asserted here is transcribed from `SPEC.md`'s
//! `## Named constants` consolidated table (grep `^## Named constants`),
//! cross-checked against the specific section that table cites as each
//! constant's `Home` where the table's own entry is terse (grouped ranges,
//! formulas, or prose fractions/rates rather than a bare literal). This
//! file is written from the spec, independently of the implementation and
//! of `.slices/00-ground/PLAN.md`'s transcription table — a value written
//! here that disagrees with what the crate defines is exactly the signal
//! this file exists to produce.
//!
//! A handful of constants are durations. `std::time::Duration` is the type
//! that reads naturally for them; if the crate instead exposes a raw
//! integer (seconds, millis, ...), this file fails to *compile*, not to
//! *assert* — that is itself useful information, per the brief.
//!
//! One `#[test]` per constant so that a single wrong transcription doesn't
//! stop the rest of the fence from reporting (`cargo test` runs every test
//! function regardless of another's panic).

// This file's whole job is to assert one constant against another, so
// `clippy::assertions_on_constants` fires on every relational fence in it.
// The lint's advice (`const { assert!(..) }`) would make a mis-transcription
// a *build* failure rather than a per-constant test failure, which defeats
// the "one `#[test]` per constant so one wrong value does not mask the
// rest" design above. Silenced here, and nowhere else in the crate.
// (Added by the slice-0 implementer to clear the `-D warnings` lint gate:
// no assertion, value or name in this file was changed.)
#![allow(clippy::assertions_on_constants)]

use slither::constants::*;
use std::time::Duration;

// ---------------------------------------------------------------------
// §3.1 — packet types, version, reserved packet-type values
// ---------------------------------------------------------------------

#[test]
fn version() {
    // SPEC.md §3.1 / Named constants: `VERSION` = 0x01.
    assert_eq!(VERSION, 0x01);
}

#[test]
fn pkt_handshake_init() {
    // SPEC.md §3.1: PKT_HANDSHAKE_INIT / PKT_HANDSHAKE_RESP / PKT_DATA = 0x01 / 0x02 / 0x03.
    assert_eq!(PKT_HANDSHAKE_INIT, 0x01);
}

#[test]
fn pkt_handshake_resp() {
    // SPEC.md §3.1: PKT_HANDSHAKE_INIT / PKT_HANDSHAKE_RESP / PKT_DATA = 0x01 / 0x02 / 0x03.
    assert_eq!(PKT_HANDSHAKE_RESP, 0x02);
}

#[test]
fn pkt_data() {
    // SPEC.md §3.1: PKT_HANDSHAKE_INIT / PKT_HANDSHAKE_RESP / PKT_DATA = 0x01 / 0x02 / 0x03.
    assert_eq!(PKT_DATA, 0x03);
}

#[test]
fn pkt_reserved_unused() {
    // SPEC.md §3.1: "reserved packet types | 0x04 (unused), 0x05 (cookie/mac2)".
    assert_eq!(PKT_RESERVED_UNUSED, 0x04);
}

#[test]
fn pkt_reserved_cookie() {
    // SPEC.md §3.1: "reserved packet types | 0x04 (unused), 0x05 (cookie/mac2)".
    assert_eq!(PKT_RESERVED_COOKIE, 0x05);
}

// ---------------------------------------------------------------------
// §3.2–3.4 — header lengths (INIT §3.2, RESP §3.3, DATA §3.4)
// ---------------------------------------------------------------------

#[test]
fn init_header_len() {
    // SPEC.md §3.2 / Named constants: INIT_HEADER_LEN / RESP_HEADER_LEN / DATA_HEADER_LEN = 6 / 10 / 14 B.
    assert_eq!(INIT_HEADER_LEN, 6);
}

#[test]
fn resp_header_len() {
    // SPEC.md §3.3 / Named constants: INIT_HEADER_LEN / RESP_HEADER_LEN / DATA_HEADER_LEN = 6 / 10 / 14 B.
    assert_eq!(RESP_HEADER_LEN, 10);
}

#[test]
fn data_header_len() {
    // SPEC.md §3.4 / Named constants: INIT_HEADER_LEN / RESP_HEADER_LEN / DATA_HEADER_LEN = 6 / 10 / 14 B.
    assert_eq!(DATA_HEADER_LEN, 14);
}

// ---------------------------------------------------------------------
// §3.5 — sizes and caps
// ---------------------------------------------------------------------

#[test]
fn max_datagram() {
    // SPEC.md §3.5 / Named constants: MAX_DATAGRAM / MAX_PLAINTEXT = 1200 / 1170 B.
    assert_eq!(MAX_DATAGRAM, 1200);
}

#[test]
fn max_plaintext() {
    // SPEC.md §3.5 / Named constants: MAX_DATAGRAM / MAX_PLAINTEXT = 1200 / 1170 B.
    assert_eq!(MAX_PLAINTEXT, 1170);
}

// ---------------------------------------------------------------------
// §2.3 / §2.4 — per-suite derived sizes (reference suite: P-256 / ChaCha20-Poly1305)
// ---------------------------------------------------------------------

#[test]
fn static_public_len() {
    // SPEC.md §2.3/§2.4: reference-suite `PK` = 65 (P-256 uncompressed SEC1
    // form, 0x04 ‖ X ‖ Y). Suite-dependent; this is the reference-suite value
    // the "Named constants" table's own header says it reports.
    assert_eq!(STATIC_PUBLIC_LEN, 65);
}

#[test]
fn aead_tag_len() {
    // SPEC.md §2.3 / Named constants: AEAD_TAG_LEN = 16 B.
    assert_eq!(AEAD_TAG_LEN, 16);
}

#[test]
fn msg1_payload_len() {
    // SPEC.md §5.2 / §2.3 (test-pinned) / Named constants: MSG1_PAYLOAD_LEN = 12 B.
    assert_eq!(MSG1_PAYLOAD_LEN, 12);
}

#[test]
fn ik_msg1_len() {
    // SPEC.md §2.3 / Named constants: IK_MSG1_LEN / IK_MSG2_LEN = 174 / 81 B.
    assert_eq!(IK_MSG1_LEN, 174);
}

#[test]
fn ik_msg2_len() {
    // SPEC.md §2.3 / Named constants: IK_MSG1_LEN / IK_MSG2_LEN = 174 / 81 B.
    assert_eq!(IK_MSG2_LEN, 81);
}

#[test]
fn init_packet_len() {
    // SPEC.md §2.3 / Named constants: INIT_PACKET_LEN / RESP_PACKET_LEN = 196 / 107 B.
    assert_eq!(INIT_PACKET_LEN, 196);
}

#[test]
fn resp_packet_len() {
    // SPEC.md §2.3 / Named constants: INIT_PACKET_LEN / RESP_PACKET_LEN = 196 / 107 B.
    assert_eq!(RESP_PACKET_LEN, 107);
}

// ---------------------------------------------------------------------
// §5.1 / §5.2 — handshake prologue and mac1
// ---------------------------------------------------------------------

#[test]
fn prologue() {
    // SPEC.md §5.1 / Named constants: PROLOGUE = b"slither\x01".
    assert_eq!(PROLOGUE, b"slither\x01");
}

#[test]
fn timestamp_len() {
    // SPEC.md §5.2 / Named constants: TIMESTAMP_LEN / MSG1_PAYLOAD_LEN = 12 / 12 B.
    assert_eq!(TIMESTAMP_LEN, 12);
}

#[test]
fn mac1_label() {
    // SPEC.md §4.1 / Named constants: MAC1_LABEL / MAC1_LEN = b"slither mac1" / 16 B.
    assert_eq!(MAC1_LABEL, b"slither mac1");
}

#[test]
fn mac1_len() {
    // SPEC.md §4.1 / Named constants: MAC1_LABEL / MAC1_LEN = b"slither mac1" / 16 B.
    assert_eq!(MAC1_LEN, 16);
}

// ---------------------------------------------------------------------
// §5.5 — handshake retransmit / give-up
// ---------------------------------------------------------------------

#[test]
fn retransmit_base() {
    // SPEC.md §5.5 / Named constants: RETRANSMIT_BASE / RETRANSMIT_JITTER_MAX = 5 s / 333 ms.
    assert_eq!(RETRANSMIT_BASE, Duration::from_secs(5));
}

#[test]
fn retransmit_jitter_max() {
    // SPEC.md §5.5 / Named constants: RETRANSMIT_BASE / RETRANSMIT_JITTER_MAX = 5 s / 333 ms.
    assert_eq!(RETRANSMIT_JITTER_MAX, Duration::from_millis(333));
}

#[test]
fn handshake_giveup() {
    // SPEC.md §5.5 / Named constants: HANDSHAKE_GIVEUP = 90 s.
    assert_eq!(HANDSHAKE_GIVEUP, Duration::from_secs(90));
}

// ---------------------------------------------------------------------
// §6.3 — Intro parking
// ---------------------------------------------------------------------

#[test]
fn intro_queue_cap() {
    // SPEC.md §6.3 / Named constants: INTRO_QUEUE_CAP / INTRO_MAX_PER_SOURCE / INTRO_TTL = 1024 / 4 / 15 s.
    assert_eq!(INTRO_QUEUE_CAP, 1024);
}

#[test]
fn intro_max_per_source() {
    // SPEC.md §6.3 / Named constants: INTRO_QUEUE_CAP / INTRO_MAX_PER_SOURCE / INTRO_TTL = 1024 / 4 / 15 s.
    assert_eq!(INTRO_MAX_PER_SOURCE, 4);
}

#[test]
fn intro_ttl() {
    // SPEC.md §6.3 / Named constants: INTRO_QUEUE_CAP / INTRO_MAX_PER_SOURCE / INTRO_TTL = 1024 / 4 / 15 s.
    assert_eq!(INTRO_TTL, Duration::from_secs(15));
}

// ---------------------------------------------------------------------
// §7.2 / §7.3 / §7.5 / §7.7 — datagram session: replay, roaming, liveness, rekey
// ---------------------------------------------------------------------

#[test]
fn replay_window() {
    // SPEC.md §7.2 / Named constants: REPLAY_WINDOW = 2048 bits.
    assert_eq!(REPLAY_WINDOW, 2048);
}

#[test]
fn amplification_factor() {
    // SPEC.md §7.3 / Named constants: AMPLIFICATION_FACTOR = 3 (x authenticated bytes received).
    assert_eq!(AMPLIFICATION_FACTOR, 3);
}

#[test]
fn keepalive_timeout() {
    // SPEC.md §7.5 / Named constants: KEEPALIVE_TIMEOUT / DEAD_TIMEOUT = 10 s / 25 s.
    assert_eq!(KEEPALIVE_TIMEOUT, Duration::from_secs(10));
}

#[test]
fn dead_timeout() {
    // SPEC.md §7.5 / Named constants: KEEPALIVE_TIMEOUT / DEAD_TIMEOUT = 10 s / 25 s.
    assert_eq!(DEAD_TIMEOUT, Duration::from_secs(25));
}

#[test]
fn persistent_keepalive_default() {
    // SPEC.md §7.5 / Named constants: PERSISTENT_KEEPALIVE default = 10 s.
    assert_eq!(PERSISTENT_KEEPALIVE_DEFAULT, Duration::from_secs(10));
}

#[test]
fn persistent_keepalive_min() {
    // SPEC.md §7.5 / Named constants: admissible range [1 s, DEAD_TIMEOUT) — floor is 1 s (ruling 42).
    assert_eq!(PERSISTENT_KEEPALIVE_MIN, Duration::from_secs(1));
}

#[test]
fn rekey_epoch_msgs() {
    // SPEC.md §7.7 / Named constants: REKEY_EPOCH_MSGS / MAX_EPOCH_JUMP = 65 536 / 2 (hiss-fixed).
    assert_eq!(REKEY_EPOCH_MSGS, 65_536);
}

#[test]
fn max_epoch_jump() {
    // SPEC.md §7.7 / Named constants: REKEY_EPOCH_MSGS / MAX_EPOCH_JUMP = 65 536 / 2 (hiss-fixed).
    assert_eq!(MAX_EPOCH_JUMP, 2);
}

// ---------------------------------------------------------------------
// §8.3 — the frame table (SPEC.md §8.3's own registry, cited as `Home` by
// the consolidated table's compressed "frame types" row: 0x00, 0x01, 0x02,
// 0x04, 0x08-0x0f, 0x10-0x13, 0x1c, 0x30/0x31; 0x05 reserved)
// ---------------------------------------------------------------------

#[test]
fn frame_padding() {
    // SPEC.md §8.3: `0x00` | PADDING.
    assert_eq!(FRAME_PADDING, 0x00);
}

#[test]
fn frame_ping() {
    // SPEC.md §8.3: `0x01` | PING.
    assert_eq!(FRAME_PING, 0x01);
}

#[test]
fn frame_ack() {
    // SPEC.md §8.3: `0x02` | ACK.
    assert_eq!(FRAME_ACK, 0x02);
}

#[test]
fn frame_reset_stream() {
    // SPEC.md §8.3: `0x04` | RESET_STREAM.
    assert_eq!(FRAME_RESET_STREAM, 0x04);
}

#[test]
fn frame_stop_sending_reserved() {
    // SPEC.md §8.3: `0x05` | (reserved: STOP_SENDING).
    assert_eq!(FRAME_STOP_SENDING_RESERVED, 0x05);
}

#[test]
fn frame_stream_base() {
    // SPEC.md §8.3: `0x08`-`0x0f` | STREAM (base of the flagged range).
    assert_eq!(FRAME_STREAM_BASE, 0x08);
}

#[test]
fn frame_stream_max() {
    // SPEC.md §8.3: `0x08`-`0x0f` | STREAM (top of the flagged range).
    assert_eq!(FRAME_STREAM_MAX, 0x0f);
}

#[test]
fn frame_max_data() {
    // SPEC.md §8.3: `0x10` | MAX_DATA.
    assert_eq!(FRAME_MAX_DATA, 0x10);
}

#[test]
fn frame_max_stream_data() {
    // SPEC.md §8.3: `0x11` | MAX_STREAM_DATA.
    assert_eq!(FRAME_MAX_STREAM_DATA, 0x11);
}

#[test]
fn frame_max_streams_bidi() {
    // SPEC.md §8.3: `0x12` | MAX_STREAMS_BIDI.
    assert_eq!(FRAME_MAX_STREAMS_BIDI, 0x12);
}

#[test]
fn frame_max_streams_uni() {
    // SPEC.md §8.3: `0x13` | MAX_STREAMS_UNI.
    assert_eq!(FRAME_MAX_STREAMS_UNI, 0x13);
}

#[test]
fn frame_close() {
    // SPEC.md §8.3: `0x1c` | CLOSE.
    assert_eq!(FRAME_CLOSE, 0x1c);
}

#[test]
fn frame_datagram() {
    // SPEC.md §8.3: `0x30`/`0x31` | DATAGRAM (0x30 = no length field).
    assert_eq!(FRAME_DATAGRAM, 0x30);
}

#[test]
fn frame_datagram_len() {
    // SPEC.md §8.3: `0x30`/`0x31` | DATAGRAM (0x31 = length field present).
    assert_eq!(FRAME_DATAGRAM_LEN, 0x31);
}

// ---------------------------------------------------------------------
// §8.4 — STREAM frame flags and CLOSE reason cap
// ---------------------------------------------------------------------

#[test]
fn stream_off() {
    // SPEC.md §8.4 / Named constants: STREAM_OFF / STREAM_LEN / STREAM_FIN = 0x04 / 0x02 / 0x01.
    assert_eq!(STREAM_OFF, 0x04);
}

#[test]
fn stream_len() {
    // SPEC.md §8.4 / Named constants: STREAM_OFF / STREAM_LEN / STREAM_FIN = 0x04 / 0x02 / 0x01.
    assert_eq!(STREAM_LEN, 0x02);
}

#[test]
fn stream_fin() {
    // SPEC.md §8.4 / Named constants: STREAM_OFF / STREAM_LEN / STREAM_FIN = 0x04 / 0x02 / 0x01.
    assert_eq!(STREAM_FIN, 0x01);
}

#[test]
fn stream_flag_mask() {
    // SPEC.md §8.4: not given a bare literal in the consolidated table; it is
    // the union of the three flag bits listed there (0x04 | 0x02 | 0x01).
    // See also `stream_flag_mask_is_union_of_flags` below for the derivation
    // asserted independently of this literal.
    assert_eq!(STREAM_FLAG_MASK, 0x07);
}

#[test]
fn close_reason_max() {
    // SPEC.md §8.4 / Named constants: CLOSE_REASON_MAX = 256 B.
    assert_eq!(CLOSE_REASON_MAX, 256);
}

// ---------------------------------------------------------------------
// §9.8 — message mode
// ---------------------------------------------------------------------

#[test]
fn message_recv_max() {
    // SPEC.md §9.8 / Named constants: MESSAGE_RECV_MAX = INITIAL_MAX_STREAM_DATA.
    assert_eq!(MESSAGE_RECV_MAX, 262_144);
}

// ---------------------------------------------------------------------
// §10.2 / §10.3 / §10.4 / §10.6 — flow control and stream limits
// ---------------------------------------------------------------------

#[test]
fn initial_max_data() {
    // SPEC.md §10.2 / Named constants: INITIAL_MAX_DATA = 1 048 576 B (1 MiB).
    assert_eq!(INITIAL_MAX_DATA, 1_048_576);
}

#[test]
fn initial_max_stream_data() {
    // SPEC.md §10.2 / Named constants: INITIAL_MAX_STREAM_DATA = 262 144 B (256 KiB).
    assert_eq!(INITIAL_MAX_STREAM_DATA, 262_144);
}

#[test]
fn initial_max_streams_bidi() {
    // SPEC.md §10.2 / Named constants: INITIAL_MAX_STREAMS_BIDI / _UNI = 32 / 128 (cumulative).
    assert_eq!(INITIAL_MAX_STREAMS_BIDI, 32);
}

#[test]
fn initial_max_streams_uni() {
    // SPEC.md §10.2 / Named constants: INITIAL_MAX_STREAMS_BIDI / _UNI = 32 / 128 (cumulative).
    assert_eq!(INITIAL_MAX_STREAMS_UNI, 128);
}

#[test]
fn streams_credit_batch() {
    // SPEC.md §10.4 / Named constants: STREAMS_CREDIT_BATCH = 8.
    assert_eq!(STREAMS_CREDIT_BATCH, 8);
}

#[test]
fn credit_regrant_divisor() {
    // SPEC.md §10.3 / Named constants: "credit re-grant threshold | ½ window
    // consumed". Not given a bare literal; the divisor implied by "half" is 2.
    assert_eq!(CREDIT_REGRANT_DIVISOR, 2);
}

#[test]
fn reassembly_chunks_max() {
    // SPEC.md §10.6 / Named constants: REASSEMBLY_CHUNKS_MAX = 1024 stored discontiguous ranges per stream.
    assert_eq!(REASSEMBLY_CHUNKS_MAX, 1024);
}

// ---------------------------------------------------------------------
// §11.2 / §11.3 — datagrams
// ---------------------------------------------------------------------

#[test]
fn max_datagram_payload() {
    // SPEC.md §11.2 / Named constants: MAX_DATAGRAM_PAYLOAD = 1169 B (= MAX_PLAINTEXT - 1).
    assert_eq!(MAX_DATAGRAM_PAYLOAD, 1169);
}

#[test]
fn datagram_send_queue() {
    // SPEC.md §11.3 / Named constants: DATAGRAM_SEND_QUEUE / DATAGRAM_RECV_QUEUE = 64 / 64.
    assert_eq!(DATAGRAM_SEND_QUEUE, 64);
}

#[test]
fn datagram_recv_queue() {
    // SPEC.md §11.3 / Named constants: DATAGRAM_SEND_QUEUE / DATAGRAM_RECV_QUEUE = 64 / 64.
    assert_eq!(DATAGRAM_RECV_QUEUE, 64);
}

// ---------------------------------------------------------------------
// §12.2 / §12.4 — ACKs
// ---------------------------------------------------------------------

#[test]
fn max_ack_ranges() {
    // SPEC.md §12.2 / Named constants: MAX_ACK_RANGES = 64.
    assert_eq!(MAX_ACK_RANGES, 64);
}

#[test]
fn ack_eliciting_per_ack() {
    // SPEC.md §12.4 / Named constants: "ACK policy | every 2nd ack-eliciting, ...".
    assert_eq!(ACK_ELICITING_PER_ACK, 2);
}

#[test]
fn max_ack_delay() {
    // SPEC.md §12.4 / §13.3 / Named constants: MAX_ACK_DELAY = 25 ms.
    assert_eq!(MAX_ACK_DELAY, Duration::from_millis(25));
}

// ---------------------------------------------------------------------
// §13.1 / §13.2 / §13.3 — loss detection
// ---------------------------------------------------------------------

#[test]
fn k_packet_threshold() {
    // SPEC.md §13.2 / Named constants: K_PACKET_THRESHOLD / time threshold / K_GRANULARITY = 3 / 9/8 / 1 ms.
    assert_eq!(K_PACKET_THRESHOLD, 3);
}

#[test]
fn k_time_threshold_num() {
    // SPEC.md §13.2 / Named constants: time threshold = 9/8.
    assert_eq!(K_TIME_THRESHOLD_NUM, 9);
}

#[test]
fn k_time_threshold_den() {
    // SPEC.md §13.2 / Named constants: time threshold = 9/8.
    assert_eq!(K_TIME_THRESHOLD_DEN, 8);
}

#[test]
fn k_granularity() {
    // SPEC.md §13.2 / Named constants: K_GRANULARITY = 1 ms.
    assert_eq!(K_GRANULARITY, Duration::from_millis(1));
}

#[test]
fn k_initial_rtt() {
    // SPEC.md §13.1 / Named constants: K_INITIAL_RTT / PTO_BACKOFF_CAP = 333 ms / 2^6.
    assert_eq!(K_INITIAL_RTT, Duration::from_millis(333));
}

#[test]
fn pto_backoff_cap() {
    // SPEC.md §13.3 / Named constants: K_INITIAL_RTT / PTO_BACKOFF_CAP = 333 ms / 2^6.
    assert_eq!(PTO_BACKOFF_CAP, 64);
}

// ---------------------------------------------------------------------
// §14.2 / §14.4 — congestion control
// ---------------------------------------------------------------------

#[test]
fn initial_window() {
    // SPEC.md §14.2 / Named constants: INITIAL_WINDOW / MINIMUM_WINDOW = 12 000 / 2 400 B.
    assert_eq!(INITIAL_WINDOW, 12_000);
}

#[test]
fn minimum_window() {
    // SPEC.md §14.2 / Named constants: INITIAL_WINDOW / MINIMUM_WINDOW = 12 000 / 2 400 B.
    assert_eq!(MINIMUM_WINDOW, 2_400);
}

#[test]
fn loss_reduction_factor() {
    // SPEC.md §14.2 / Named constants: LOSS_REDUCTION_FACTOR / PERSISTENT_CONGESTION_THRESHOLD = 0.5 / 3.
    assert_eq!(LOSS_REDUCTION_FACTOR, 0.5);
}

#[test]
fn persistent_congestion_threshold() {
    // SPEC.md §14.4 / Named constants: LOSS_REDUCTION_FACTOR / PERSISTENT_CONGESTION_THRESHOLD = 0.5 / 3.
    assert_eq!(PERSISTENT_CONGESTION_THRESHOLD, 3);
}

// ---------------------------------------------------------------------
// §15.1 — close and linger
// ---------------------------------------------------------------------

#[test]
fn close_linger() {
    // SPEC.md §15.1 / Named constants: CLOSE_LINGER / close-reply rate = 5 s / <= 1 per s.
    assert_eq!(CLOSE_LINGER, Duration::from_secs(5));
}

#[test]
fn close_reply_min_interval() {
    // SPEC.md §15.1 / Named constants: close-reply rate <= 1 per s, i.e. replies
    // are spaced at least 1 s apart. Not given a bare literal in the table.
    assert_eq!(CLOSE_REPLY_MIN_INTERVAL, Duration::from_secs(1));
}

// ---------------------------------------------------------------------
// §15.3 — error-code registry
// ---------------------------------------------------------------------

#[test]
fn no_error() {
    // SPEC.md §15.3: `0x00` | NO_ERROR.
    assert_eq!(NO_ERROR, 0x00);
}

#[test]
fn protocol_violation() {
    // SPEC.md §15.3: `0x01` | PROTOCOL_VIOLATION.
    assert_eq!(PROTOCOL_VIOLATION, 0x01);
}

#[test]
fn flow_control_error() {
    // SPEC.md §15.3: `0x02` | FLOW_CONTROL_ERROR.
    assert_eq!(FLOW_CONTROL_ERROR, 0x02);
}

#[test]
fn stream_limit_error() {
    // SPEC.md §15.3: `0x03` | STREAM_LIMIT_ERROR.
    assert_eq!(STREAM_LIMIT_ERROR, 0x03);
}

#[test]
fn stream_state_error() {
    // SPEC.md §15.3: `0x04` | STREAM_STATE_ERROR.
    assert_eq!(STREAM_STATE_ERROR, 0x04);
}

#[test]
fn final_size_error() {
    // SPEC.md §15.3: `0x05` | FINAL_SIZE_ERROR.
    assert_eq!(FINAL_SIZE_ERROR, 0x05);
}

#[test]
fn message_overflow() {
    // SPEC.md §15.3: `0x06` | MESSAGE_OVERFLOW — ratified 2026/08/14, ruling 52.
    // As of this ruling's landing, both §15.3 and the consolidated table read
    // 0x00-0x06 (previously 0x00-0x05); §18.1 flags that the two restating
    // spots (this table and its own prose) needed the same update. No
    // disagreement was found between them as read here.
    assert_eq!(MESSAGE_OVERFLOW, 0x06);
}

#[test]
fn application_error_base() {
    // SPEC.md §15.3: codes >= 0x10 are application-defined via close().
    assert_eq!(APPLICATION_ERROR_BASE, 0x10);
}

// ---------------------------------------------------------------------
// §16.5 — time and timers (shell)
// ---------------------------------------------------------------------

#[test]
fn shell_lateness_bound() {
    // SPEC.md §16.5 / Named constants: `L` (shell lateness bound) = 250 ms.
    assert_eq!(SHELL_LATENESS_BOUND, Duration::from_millis(250));
}

// ---------------------------------------------------------------------
// §17.1 — replay guard orphan cap
// ---------------------------------------------------------------------

#[test]
fn ts_guard_orphan_cap() {
    // SPEC.md §17.1 / Named constants: TS_GUARD_ORPHAN_CAP = 1024.
    assert_eq!(TS_GUARD_ORPHAN_CAP, 1024);
}

// =======================================================================
// Derived relationships — asserted independently of the literals above.
// A wrong literal that happens to be internally self-consistent would slip
// past the tests above only if the *same* wrong value were used everywhere
// it appears; these tests instead recompute one constant from others and
// check the result, which is a different failure mode than "copied the
// table wrong."
// =======================================================================

#[test]
fn max_plaintext_is_datagram_minus_header_minus_tag() {
    // SPEC.md §3.5: MAX_PLAINTEXT is what remains of MAX_DATAGRAM after the
    // 14-byte Data header and the 16-byte AEAD tag.
    assert_eq!(MAX_PLAINTEXT, MAX_DATAGRAM - DATA_HEADER_LEN - AEAD_TAG_LEN);
}

#[test]
fn max_datagram_payload_is_max_plaintext_minus_one() {
    // SPEC.md §11.2: MAX_DATAGRAM_PAYLOAD = MAX_PLAINTEXT - 1 (the DATAGRAM
    // frame's type byte's LEN variant still needs to fit in the remaining
    // plaintext budget after the frame-type/length overhead).
    assert_eq!(MAX_DATAGRAM_PAYLOAD, MAX_PLAINTEXT - 1);
}

#[test]
fn init_packet_len_is_header_plus_msg1_plus_mac1() {
    // SPEC.md §2.3: INIT_PACKET_LEN = 6 + MSG1_LEN + 16 (InitHeader | msg1 | mac1).
    assert_eq!(INIT_PACKET_LEN, INIT_HEADER_LEN + IK_MSG1_LEN + MAC1_LEN);
}

#[test]
fn resp_packet_len_is_header_plus_msg2_plus_mac1() {
    // SPEC.md §2.3: RESP_PACKET_LEN = 10 + MSG2_LEN + 16 (RespHeader | msg2 | mac1).
    assert_eq!(RESP_PACKET_LEN, RESP_HEADER_LEN + IK_MSG2_LEN + MAC1_LEN);
}

#[test]
fn ik_msg1_len_is_two_statics_two_tags_plus_payload() {
    // SPEC.md §2.3: MSG1_LEN = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)
    // (e || enc_s || enc_payload) = 2*PK + 2*TAG + MSG1_PAYLOAD_LEN.
    assert_eq!(
        IK_MSG1_LEN,
        2 * STATIC_PUBLIC_LEN + 2 * AEAD_TAG_LEN + MSG1_PAYLOAD_LEN
    );
}

#[test]
fn ik_msg2_len_is_one_static_plus_tag() {
    // SPEC.md §2.3: MSG2_LEN = PK + TAG (e || the empty payload's tag).
    assert_eq!(IK_MSG2_LEN, STATIC_PUBLIC_LEN + AEAD_TAG_LEN);
}

#[test]
fn dead_timeout_is_twice_keepalive_plus_five_seconds() {
    // SPEC.md §7.5: liveness fires at DEAD_TIMEOUT, and the contested-probe
    // path (§7.5, rulings 36/41) ties the 25 s figure to 2x KEEPALIVE_TIMEOUT
    // (10 s) plus the 5 s the probe/verdict machinery allows on top.
    assert_eq!(DEAD_TIMEOUT, 2 * KEEPALIVE_TIMEOUT + Duration::from_secs(5));
}

#[test]
fn message_recv_max_equals_initial_max_stream_data() {
    // SPEC.md §9.8 / Named constants: "MESSAGE_RECV_MAX | = INITIAL_MAX_STREAM_DATA".
    assert_eq!(MESSAGE_RECV_MAX, INITIAL_MAX_STREAM_DATA);
}

#[test]
fn stream_flag_mask_is_union_of_flags() {
    // SPEC.md §8.4: the STREAM frame's flag byte is built from STREAM_OFF /
    // STREAM_LEN / STREAM_FIN; the mask covering all three is their bitwise OR.
    assert_eq!(STREAM_FLAG_MASK, STREAM_OFF | STREAM_LEN | STREAM_FIN);
}

#[test]
fn frame_stream_max_is_base_or_flag_mask() {
    // SPEC.md §8.3/§8.4: the STREAM frame type range 0x08-0x0f is the base
    // type with every combination of the three flag bits set; the top of the
    // range is the base OR'd with the full flag mask.
    assert_eq!(FRAME_STREAM_MAX, FRAME_STREAM_BASE | STREAM_FLAG_MASK);
}

#[test]
fn persistent_keepalive_default_is_within_admissible_range() {
    // SPEC.md §7.5: admissible range is [1 s, DEAD_TIMEOUT); the 10 s default
    // must fall inside the range its own setter would accept.
    assert!(PERSISTENT_KEEPALIVE_DEFAULT >= PERSISTENT_KEEPALIVE_MIN);
    assert!(PERSISTENT_KEEPALIVE_DEFAULT < DEAD_TIMEOUT);
}

#[test]
fn wire_error_codes_are_all_below_application_base() {
    // SPEC.md §15.3: 0x00-0x06 are transport codes, 0x07-0x0f are reserved,
    // and application codes begin at 0x10 — every named transport code must
    // sit strictly below APPLICATION_ERROR_BASE.
    assert!(NO_ERROR < APPLICATION_ERROR_BASE);
    assert!(PROTOCOL_VIOLATION < APPLICATION_ERROR_BASE);
    assert!(FLOW_CONTROL_ERROR < APPLICATION_ERROR_BASE);
    assert!(STREAM_LIMIT_ERROR < APPLICATION_ERROR_BASE);
    assert!(STREAM_STATE_ERROR < APPLICATION_ERROR_BASE);
    assert!(FINAL_SIZE_ERROR < APPLICATION_ERROR_BASE);
    assert!(MESSAGE_OVERFLOW < APPLICATION_ERROR_BASE);
}

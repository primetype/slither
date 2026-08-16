//! §7.3's path validation — ruling 208's `PATH_CHALLENGE` / `PATH_RESPONSE`.
//!
//! Written by slice 7b's **blind test author**, from `CONTRACT-7b.md`,
//! rulings 208 / 210 / 212 and `SPEC.md` §7.3, §8.4, §8.5, §8.7. The
//! implementers are not visible from here and this file was written before
//! any of their work existed (working rule 6).
//!
//! # What every test in this file is defending
//!
//! Ruling 208's defect: *"`largest` is not a proof of receipt; it is an
//! **assertion by whoever holds the key**, and §7.3's roaming threat model
//! *is* the key holder."* A connected peer announced a move to a victim,
//! waited for one sealed packet, and returned a **forged ACK** spoofed from
//! the victim — two packets, after which reflection at the victim was
//! unbounded.
//!
//! So the load-bearing assertions here are the **negative** ones. A build
//! that adds `PATH_CHALLENGE`/`PATH_RESPONSE` correctly *and leaves
//! `on_ack_covering` wired to the budget* passes every "the challenge
//! works" test in this file and is still defeated by the exact attack
//! ruling 208 exists to close. [`an_ack_covering_everything_validates_
//! nothing`] and its siblings are what separate those two builds.
//!
//! # Two conflicts found while writing this, reported and not resolved
//!
//! Working rule 3. Both are stated at the tests that depend on them:
//!
//! 1. **The rank of the path frames against the contested probe.**
//!    `SPEC.md` §7.3:2370-2374 ranks them *below* the probe and carries an
//!    explicit `[FLAGGED FOR RULING]` block; **ruling 212(c) ranks them
//!    above it**. See [`a_pending_contested_probe_does_not_block_the_
//!    challenge`].
//! 2. **Whether the challenge rides a packet that already elicits.**
//!    `CONTRACT-7b.md` §1.4 keeps the pump's `!elicits` guard; `SPEC.md`
//!    §8.7 owes the challenge *"whenever §7.3's budget admits a packet and
//!    the address is still unvalidated"*. See
//!    [`a_packet_leaving_for_an_unvalidated_address_carries_the_challenge`].

use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::constants;
use crate::core::connection::stream_id::Dir;
use crate::core::connection::testfix::{
    Drained, Pair, Solo, WIRE_PATH_CHALLENGE, WIRE_PATH_RESPONSE, Wire, a_addr, assert_alive,
    b_addr, drain, path_challenge_frame, path_frame_with_body, path_response_frame, put, t0, v4,
    write_all,
};
use crate::core::connection::{ConnEvent, ConnOutput};
use crate::error::ConnectionLost;

// ═══════════════════════════════════════════════════════════════════════
// fixtures local to this file
// ═══════════════════════════════════════════════════════════════════════

/// A **stable** origin instant, for the same reason `tests_roam.rs` has
/// one: `testfix::t0()` is `Instant::now()` and hands back a different
/// value on every call.
fn origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(t0)
}

/// A third address, for the peer to move to.
fn c_addr() -> SocketAddr {
    v4(9, 41_000)
}

/// A fourth, for the second move — §7.3 re-arms *"at the next address
/// change"*, and one move cannot show that.
fn d_addr() -> SocketAddr {
    v4(11, 42_000)
}

/// §8.4's ACK, as raw bytes. One block; `largest` is the only field any
/// test here varies.
fn ack_frame(largest: u64) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, constants::FRAME_ACK);
    put(&mut out, largest);
    put(&mut out, 0); // ack_delay
    put(&mut out, 0); // range_count
    put(&mut out, 0); // first_range
    out
}

/// §8.3's PING — the cheapest ack-eliciting thing a raw peer can send, so
/// the core under test owes an ACK and a packet leaves.
fn ping_frame() -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, constants::FRAME_PING);
    out
}

/// Every `PATH_CHALLENGE` value in a frame list, in order.
fn challenges(frames: &[Wire]) -> Vec<[u8; 8]> {
    frames
        .iter()
        .filter_map(|f| match f {
            Wire::PathChallenge(v) => Some(*v),
            _ => None,
        })
        .collect()
}

/// Every `PATH_RESPONSE` value in a frame list, in order.
fn responses(frames: &[Wire]) -> Vec<[u8; 8]> {
    frames
        .iter()
        .filter_map(|f| match f {
            Wire::PathResponse(v) => Some(*v),
            _ => None,
        })
        .collect()
}

/// Roam `solo` to `to` with an ack-eliciting packet, and return every
/// challenge the core put on the wire in response.
///
/// The packet carries a PING rather than being §3.4's empty keepalive
/// **on purpose**: ruling 203 is the record that a keepalive-only dance
/// carries no frames, elicits nothing, and validates nothing. A roam test
/// built on an empty plaintext pins that stall, not this mechanism.
fn roam_and_collect(solo: &mut Solo, now: Instant, to: SocketAddr) -> Vec<[u8; 8]> {
    let d = solo.deliver_from(now, to, &ping_frame());
    let frames = solo.drain_frames(&d);
    challenges(&frames)
}

// ═══════════════════════════════════════════════════════════════════════
// §8.4 — the frame codec, and its one structural error
//
// Slice 1's one-sided-boundary trap is the thing to avoid here: `LEN` and
// `LEN-1` were tested and `LEN+1` was not. §8.4 makes "fewer than 8 bytes
// remain" the **only** structural error either frame has, so 7 is the
// error and 8 **and 9** must both parse.
// ═══════════════════════════════════════════════════════════════════════

/// A seven-byte body is §8.2's structural class.
///
/// `CONTRACT-7b.md` §1.1: *"A body shorter than 8 bytes is
/// `Structural::LengthOverrun` — the existing variant, not a new one...
/// Do not mint a new variant."*
#[test]
fn a_path_challenge_body_of_seven_bytes_is_a_structural_error() {
    let mut solo = Solo::installed_at(origin());
    let d = solo.deliver(
        origin(),
        &path_frame_with_body(WIRE_PATH_CHALLENGE, &[0xAB; 7]),
    );
    assert_eq!(
        d.closed(),
        Some(ConnectionLost::ProtocolViolation {
            code: constants::PROTOCOL_VIOLATION
        }),
        "§8.4: fewer than 8 bytes after the type byte is the frame's only \
         structural error, and §8.2 answers it with PROTOCOL_VIOLATION",
    );
}

/// The same for `PATH_RESPONSE`. Written out rather than folded into a
/// loop because §8.4 states the rule for *both* frames and working rule 8
/// reads a two-item list as exhaustive in both directions: a build that
/// bounds-checks the challenge and indexes blindly into the response
/// passes the test above and panics on this one.
#[test]
fn a_path_response_body_of_seven_bytes_is_a_structural_error() {
    let mut solo = Solo::installed_at(origin());
    let d = solo.deliver(
        origin(),
        &path_frame_with_body(WIRE_PATH_RESPONSE, &[0xAB; 7]),
    );
    assert_eq!(
        d.closed(),
        Some(ConnectionLost::ProtocolViolation {
            code: constants::PROTOCOL_VIOLATION
        }),
        "§8.4's structural error is stated for both path frames",
    );
}

/// Eight bytes parse, and oblige a response.
#[test]
fn a_path_challenge_body_of_eight_bytes_parses() {
    let mut solo = Solo::installed_at(origin());
    let value = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let d = solo.deliver(origin(), &path_challenge_frame(value));
    assert_alive(&d);

    let frames = solo.drain_frames(&d);
    assert_eq!(
        responses(&frames),
        vec![value],
        "§8.4: a received PATH_CHALLENGE obliges a PATH_RESPONSE carrying \
         its eight bytes **verbatim**",
    );
}

/// **The other side of the boundary, and the one slice 1 forgot.**
///
/// `CONTRACT-7b.md` §1.1: *"a 9-byte body parses the frame and leaves one
/// byte for the next frame (which is then an `UnknownType` or a valid
/// frame — the parse does not 'consume the rest'). The one-sided-boundary
/// defect from slice 1 is the thing to avoid: test 7 **and** 9, not only
/// 7."*
///
/// The ninth byte here is `0x00`, §8.3's PADDING, so a parser that stops
/// after eight leaves a legal frame behind and the connection survives.
/// **The degenerate build this separates**: one that implemented the body
/// as "the rest of the plaintext" — `extends_to_end() == true`. That build
/// passes [`a_path_challenge_body_of_eight_bytes_parses`] exactly, and
/// here it swallows the padding byte into a nine-byte challenge and echoes
/// the wrong value.
#[test]
fn a_path_challenge_does_not_consume_the_rest_of_the_packet() {
    let mut solo = Solo::installed_at(origin());
    let value = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

    let mut frames = path_challenge_frame(value);
    frames.extend_from_slice(&[constants::FRAME_PADDING as u8]);

    let d = solo.deliver(origin(), &frames);
    assert_alive(&d);

    let out = solo.drain_frames(&d);
    assert_eq!(
        responses(&out),
        vec![value],
        "the challenge is exactly eight bytes wide; the trailing PADDING is \
         a separate frame and not part of its body",
    );
}

/// The echo is **byte-exact**, and the value chosen makes every plausible
/// mangling visible.
///
/// **Working rule 9.** `[0u8; 8]` would be echoed correctly by a build that
/// returns a zero array, a build that reverses the bytes, and a build that
/// echoes its own outstanding challenge when it has none. This value is
/// non-zero, non-palindromic, and has no repeated byte, so each of those
/// three fails.
#[test]
fn the_response_echoes_the_challenge_byte_for_byte() {
    let mut solo = Solo::installed_at(origin());
    let value = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04];

    let d = solo.deliver(origin(), &path_challenge_frame(value));
    let frames = solo.drain_frames(&d);

    let got = responses(&frames);
    assert_eq!(got.len(), 1, "exactly one response to one challenge");
    assert_eq!(got[0], value, "verbatim — not reversed, not re-drawn");
    assert_ne!(got[0], [0u8; 8], "and not a zeroed placeholder");
}

/// A challenge is answered even by a connection that never issued one.
///
/// `CONTRACT-7b.md` §1.9: *"**No validation of `v`** — it is opaque, any 8
/// bytes are legal, and a challenge from a peer we have not challenged is
/// answered normally. Echoing is unconditional; it is the peer's budget,
/// not ours, that the response unlocks."* §8.4 says the same at 3461-3467.
///
/// `Solo::installed_at` is the **dialled** constructor, so its own budget
/// is validated and it has no outstanding challenge of its own. A build
/// that gated the echo on having armed would answer nothing here.
#[test]
fn a_validated_connection_still_answers_a_challenge() {
    let mut solo = Solo::installed_at(origin());
    assert_eq!(
        solo.conn.amplification_budget(),
        None,
        "precondition: dialled ⇒ validated ⇒ no challenge of our own",
    );

    let value = [0x9a; 8];
    let d = solo.deliver(origin(), &path_challenge_frame(value));
    let frames = solo.drain_frames(&d);
    assert_eq!(
        responses(&frames),
        vec![value],
        "the obligation is unconditional — it is not gated on our having \
         armed, roamed, or expected anything",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 208's whole purpose — an ACK is not a proof of receipt
//
// **These are the tests a plausible-but-wrong build fails and nothing else
// catches.** Every test above passes a build that implements the two new
// frames perfectly and leaves `Amplification::on_ack_covering` feeding the
// budget. Ruling 208: leaving both in place "would give an attacker the
// old path as a bypass."
// ═══════════════════════════════════════════════════════════════════════

/// **The single most important assertion in this file.**
///
/// An ACK covering everything the connection could ever have sealed
/// validates **nothing**. Ruling 208 replaced the ACK predicate; it did not
/// supplement it.
///
/// `largest` is `u32::MAX` so the ACK covers any floor a surviving
/// `validation_floor` could hold — the test cannot be passed by a build
/// whose floor merely happens to sit above the value chosen.
#[test]
fn an_ack_covering_everything_validates_nothing() {
    let mut solo = Solo::installed_from_msg1_at(origin());
    assert!(
        solo.conn.amplification_budget().is_some(),
        "precondition: a msg1-anchored connection begins unvalidated \
         (ruling 200)",
    );

    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver(now, &ack_frame(u32::MAX as u64));
    assert_alive(&d);

    assert!(
        solo.conn.amplification_budget().is_some(),
        "**ruling 208**: `largest` is not evidence of receipt, it is an \
         assertion by whoever holds the key, and the peer holds the key. \
         A build that still routes ACKs into `Amplification` fails here \
         and passes every other test in this file.",
    );
}

/// The same, in the shape of the attack ruling 208 describes: the peer
/// announces a move to a victim address and returns a forged ACK **spoofed
/// from that address**.
///
/// This is distinct from the test above, and both are needed: that one
/// pins the msg1 anchor, this one pins the roam, and ruling 208's §7.3
/// arms the budget on *both* triggers. A build that unwired the ACK
/// predicate on one arming path only passes one of the two.
#[test]
fn a_forged_ack_from_the_new_address_does_not_lift_the_budget() {
    let mut solo = Solo::installed_at(origin());

    // The peer roams the session to the victim's address.
    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &ping_frame());
    let armed = solo
        .conn
        .amplification_budget()
        .expect("the roam arms the budget");

    // ... waits for one sealed packet, then returns a forged ACK from the
    // victim's address. Two packets, per ruling 208.
    let now = now + Duration::from_millis(20);
    let d = solo.deliver_from(now, c_addr(), &ack_frame(u32::MAX as u64));
    assert_alive(&d);

    let still = solo
        .conn
        .amplification_budget()
        .expect("**ruling 208**: the forged ACK must not disarm the budget");

    // Two-sided, per working rule 9. "Still `Some`" alone is satisfied by a
    // build that never validates at all, and by one whose counters froze.
    // The credit must have *moved* — the ACK's own bytes are authenticated
    // and window-fresh, so §7.3 funds the budget with them — while the
    // validated flag must not have.
    assert!(
        still.1 > armed.1,
        "the ACK's bytes still fund the budget (§7.3, ruling 169): {} → {}",
        armed.1,
        still.1,
    );
}

/// A `PATH_RESPONSE` whose bytes match nothing is a **semantic no-op** —
/// not a protocol violation.
///
/// Ruling 212(d), and §8.4:3448-3459 at length: *"Killing the connection on
/// it would hand any off-path party that can guess a frame boundary a
/// **remote kill primitive** requiring no key, which is a strictly worse
/// defect than the one this mechanism exists to fix."*
///
/// **Working rule 9 — this test is two-sided by construction.** The
/// `assert_alive` half fails a build that raises `PROTOCOL_VIOLATION`; the
/// budget half fails a build that validates on any response at all. A build
/// that does *neither* right cannot pass both.
#[test]
fn a_mismatched_path_response_validates_nothing_and_is_not_an_error() {
    let mut solo = Solo::installed_from_msg1_at(origin());
    let now = origin() + Duration::from_millis(10);

    let d = solo.deliver(now, &path_response_frame([0xff; 8]));
    assert_alive(&d);
    assert_eq!(
        d.closed(),
        None,
        "§8.4: a mismatched response is a semantic no-op — PROTOCOL_VIOLATION \
         here would be a keyless remote kill primitive",
    );
    assert!(
        solo.conn.amplification_budget().is_some(),
        "an invented response is what an off-path attacker's guess looks \
         like, and it must validate nothing",
    );
}

/// **The positive control the test above needs.**
///
/// Without this, `a_mismatched_path_response_validates_nothing` is passed
/// by a build in which *nothing whatsoever* validates — which is working
/// rule 9's "a bound the degenerate case satisfies for free" exactly. The
/// pair is what separates "the wrong bytes are rejected" from "no bytes are
/// ever accepted".
#[test]
fn the_matching_path_response_validates_and_disarms_the_budget() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let issued = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(
        issued.len(),
        1,
        "the roam draws and offers exactly one challenge (§7.3: one per \
         arming)",
    );
    assert!(
        solo.conn.amplification_budget().is_some(),
        "unvalidated until the echo arrives",
    );

    let now = now + Duration::from_millis(20);
    let d = solo.deliver_from(now, c_addr(), &path_response_frame(issued[0]));
    assert_alive(&d);

    assert_eq!(
        solo.conn.amplification_budget(),
        None,
        "§7.3: the address validates and the budget disarms at the first \
         authenticated, window-fresh packet from it carrying a matching \
         PATH_RESPONSE",
    );
}

/// A response drawn from a **superseded arming** validates nothing.
///
/// §7.3: *"A `PATH_RESPONSE` that does not match — stale, drawn from a
/// prior arming, or invented — validates nothing."* The peer here is
/// replaying a value it genuinely received, at the correct address, which
/// is the case an equality check against a *set* of past challenges would
/// wrongly accept.
#[test]
fn a_challenge_from_a_superseded_arming_no_longer_validates() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let first = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(first.len(), 1, "one challenge for the first arming");

    // A second address change supersedes it.
    let now = now + Duration::from_millis(20);
    let second = roam_and_collect(&mut solo, now, d_addr());
    assert_eq!(second.len(), 1, "one challenge for the second arming");

    // The peer echoes the *first* arming's value, from the current address.
    let now = now + Duration::from_millis(20);
    let d = solo.deliver_from(now, d_addr(), &path_response_frame(first[0]));
    assert_alive(&d);

    assert!(
        solo.conn.amplification_budget().is_some(),
        "§7.3: 'any earlier challenge is discarded and a PATH_RESPONSE \
         echoing it validates nothing thereafter' (§13.6's roam-seam table). \
         A build keeping a *set* of live challenges fails here.",
    );

    // And the current one still works — otherwise this test is passed by a
    // build that validates on nothing at all.
    let now = now + Duration::from_millis(20);
    let _ = solo.deliver_from(now, d_addr(), &path_response_frame(second[0]));
    assert_eq!(
        solo.conn.amplification_budget(),
        None,
        "the *current* arming's challenge still validates",
    );
}

/// An echo arriving from an address other than the one being validated
/// validates nothing.
///
/// `CONTRACT-7b.md` §1.7 on `from_anchor`: *"a response must arrive **from
/// the address being validated**, or a peer echoes from its old address and
/// validates the new one. This is not optional; it is the whole
/// predicate."*
///
/// **Reported limitation — this test does not isolate what its name says.**
/// Two independent mechanisms make it pass, and I could not separate them
/// with the fixtures available. Delivering the echo from the old address is
/// itself an authenticated, window-fresh packet from a new source, so §7.3
/// **roams the session back** before `from_anchor` is computed
/// (`mod.rs:508` is deliberately after the roam), which re-arms and draws a
/// fresh challenge — so the stale echo fails on the *value* even in a build
/// with no `from_anchor` gate at all. The assertion below is still correct
/// and still worth having; it is the `from_anchor` gate specifically that
/// it does not pin. The test that does isolate it is
/// [`a_closing_connection_does_not_credit_a_third_address`]'s shape, on a
/// connection where §15.2 forbids the roam.
#[test]
fn an_echo_from_the_old_address_does_not_validate_the_new_one() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let issued = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(issued.len(), 1);

    let now = now + Duration::from_millis(20);
    let d = solo.deliver_from(now, a_addr(), &path_response_frame(issued[0]));
    assert_alive(&d);

    assert!(
        solo.conn.amplification_budget().is_some(),
        "the echo did not come from the address under validation",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// One challenge per **arming** — the two halves that must both hold
//
// Working rule 9, and slice 2a's cautionary pair. "These two challenges
// differ" alone is passed by a build that redraws on **every
// transmission**; "these two are equal" alone is passed by a build that
// draws **once, forever**. Neither degenerate passes both tests below, and
// that is the whole design of this section.
// ═══════════════════════════════════════════════════════════════════════

/// A second address change draws **fresh** bytes.
///
/// Ruling 208: *"per-arming, and never reused across armings"*.
/// `CONTRACT-7b.md` §1.2: *"A roam back to a previously-challenged address
/// draws a new value. Re-arming with the previous value would let a peer
/// bank a response."*
#[test]
fn each_arming_draws_a_fresh_challenge() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let first = roam_and_collect(&mut solo, now, c_addr());
    let now = now + Duration::from_millis(20);
    let second = roam_and_collect(&mut solo, now, d_addr());
    // A roam **back**, which is the case the contract calls out by name.
    let now = now + Duration::from_millis(20);
    let third = roam_and_collect(&mut solo, now, c_addr());

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(third.len(), 1);

    assert_ne!(first[0], second[0], "a new arming draws new bytes");
    assert_ne!(
        first[0], third[0],
        "and a roam **back** to a previously-challenged address draws new \
         bytes too — otherwise a peer banks the earlier response",
    );
    assert_ne!(second[0], third[0]);
}

/// Within **one** arming the same eight bytes are re-offered.
///
/// §8.7:3585-3590: *"`PATH_CHALLENGE` is **owed for as long as the arming
/// lasts**: the sender re-emits it — with the **same** eight bytes, since
/// ruling 208 fixes one challenge per arming — whenever §7.3's budget
/// admits a packet and the address is still unvalidated."*
///
/// **This is the test that makes [`each_arming_draws_a_fresh_challenge`]
/// mean something.** A build redrawing on every transmission satisfies
/// "fresh per arming" trivially and fails here — and it is not a harmless
/// bug: the peer's in-flight response would then always echo a superseded
/// value, and the address could never validate at all.
#[test]
fn the_same_arming_re_offers_the_same_challenge() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let first = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(first.len(), 1);

    // A second ack-eliciting packet from the same address: still the same
    // arming, because the source has not changed.
    let now = now + Duration::from_millis(20);
    let again = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(
        again.len(),
        1,
        "the standing obligation re-offers the challenge while the arming \
         lasts (§8.7)",
    );
    assert_eq!(
        again[0], first[0],
        "**the same** eight bytes: ruling 208 fixes one challenge per \
         *arming*, not per transmission",
    );
}

/// The challenge stops being owed the instant the address validates.
///
/// §8.7: *"stops owing it the instant the address validates."* Without
/// this, [`the_same_arming_re_offers_the_same_challenge`] is passed by a
/// build that re-offers the challenge forever, which is a slow leak of
/// nine bytes on every packet for the life of the connection.
#[test]
fn the_challenge_stops_being_offered_once_the_address_validates() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let issued = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(issued.len(), 1);

    let now = now + Duration::from_millis(20);
    let _ = solo.deliver_from(now, c_addr(), &path_response_frame(issued[0]));
    assert_eq!(solo.conn.amplification_budget(), None, "validated");

    let now = now + Duration::from_millis(20);
    let after = roam_and_collect(&mut solo, now, c_addr());
    assert!(
        after.is_empty(),
        "a validated address is owed no challenge: {after:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// The response obligation — bounded, and kept across a roam
// ═══════════════════════════════════════════════════════════════════════

/// Two challenges in one packet produce **one** response, to the newest.
///
/// Ruling 212(d): *"**at most one outstanding response**, overwritten
/// rather than queued, per §17.5's ceiling discipline."* §8.4:3467-3472
/// gives the reason: *"since the newer one is the only one whose answer can
/// still validate anything"*.
///
/// **The degenerate build this catches**, and the reason the rule exists:
/// one that queues them. `CONTRACT-7b.md` §1.4: *"this is deliberate and it
/// is what stops a challenge flood becoming a response flood."* Both
/// challenges ride **one** datagram, so no drain can happen between them —
/// a test that delivered them in two packets would be passed by a queueing
/// build that simply got to flush in between.
#[test]
fn two_challenges_in_one_packet_produce_one_response_to_the_newest() {
    let mut solo = Solo::installed_at(origin());

    let older = [0x0a; 8];
    let newer = [0x0b; 8];
    let mut frames = path_challenge_frame(older);
    frames.extend_from_slice(&path_challenge_frame(newer));

    let d = solo.deliver(origin(), &frames);
    assert_alive(&d);

    let out = solo.drain_frames(&d);
    let got = responses(&out);
    assert_eq!(
        got.len(),
        1,
        "§17.5 budgets one outstanding response, never a list: {got:?}",
    );
    assert_eq!(
        got[0], newer,
        "the **newer** challenge wins — it is the only one whose answer can \
         still validate anything",
    );
}

/// A response owed to the peer survives the roam that re-arms our own
/// budget.
///
/// Ruling 212(d): *"the response obligation is **kept** across a roam."*
/// §13.6's roam-seam table (`SPEC.md:4574`) states it as a table row: the
/// obligation *"answers the peer's question about **its** path, which our
/// endpoint moving does not change"*.
///
/// The challenge and the roam are the **same packet** here, which is the
/// tightest form of the case: the roam's per-connection resets run on the
/// very datagram that created the obligation. A build that clears owed
/// response state as part of §13.6's reset emits nothing.
#[test]
fn a_response_obligation_survives_the_roam_that_created_it() {
    let mut solo = Solo::installed_at(origin());

    let value = [0x5c; 8];
    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &path_challenge_frame(value));
    assert_alive(&d);

    let frames = solo.drain_frames(&d);
    assert_eq!(
        responses(&frames),
        vec![value],
        "the obligation is kept across §13.6's roam seam and is sent to the \
         new endpoint",
    );

    // And our own budget re-armed on the same packet — the two pieces of
    // state are independent, which is exactly what the ruling says.
    assert!(
        solo.conn.amplification_budget().is_some(),
        "our arming and the peer's answer are separate obligations",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// A3 — a non-live connection credits only its anchor
//
// `CONTRACT-7b.md` §3 states the degenerate check itself: *"A test that
// only exercises a **live** connection passes against the broken build,
// because on a live connection `None` already implies `src == anchor`."*
// ═══════════════════════════════════════════════════════════════════════

/// A **closing** connection does not let a third address fund its budget.
///
/// `mod.rs:466`'s `roamed` is `None` under two conditions — `src ==
/// anchor` **or** `!live` — and §15.2 forbids roaming on a closing
/// connection, so every source yields `None` there and a datagram from
/// anywhere credits an unvalidated address it did not come from. §7.3 funds
/// the budget from *"total bytes **received from it**"*.
///
/// The `close()` is what makes this test mean anything: without it the
/// packet from `d_addr` roams, `roamed` is `Some`, and the broken branch is
/// never reached.
#[test]
fn a_closing_connection_does_not_credit_a_third_address() {
    let mut solo = Solo::installed_from_msg1_at(origin());
    let now = origin() + Duration::from_millis(10);

    solo.conn.close(now, constants::NO_ERROR, b"");
    let _ = drain(&mut solo.conn);

    let before = solo
        .conn
        .amplification_budget()
        .expect("still unvalidated while closing");

    // Authenticated and window-fresh, from an address that is not the
    // anchor. §15.2 forbids the roam, so this is the `None` arm.
    let now = now + Duration::from_millis(10);
    let _ = solo.deliver_from(now, d_addr(), &ping_frame());

    let after = solo.conn.amplification_budget().expect("still unvalidated");
    assert_eq!(
        after.1, before.1,
        "§7.3 funds the budget from bytes **received from** the unvalidated \
         address; this datagram came from somewhere else",
    );
}

/// The control the test above needs: on a **live** connection the same
/// delivery does move the counters, by roaming.
///
/// Without this pair, `a_closing_connection_does_not_credit_a_third_address`
/// is passed by a build in which `on_recv` never fires at all.
#[test]
fn a_live_connection_still_credits_the_address_it_roams_to() {
    let mut solo = Solo::installed_from_msg1_at(origin());
    let now = origin() + Duration::from_millis(10);

    let d = solo.deliver_from(now, d_addr(), &ping_frame());
    assert_eq!(
        d.count_events(|e| matches!(e, ConnEvent::AddressMoved { .. })),
        1,
        "the live path roams, which is what the closing path must not do",
    );

    let (_, credited) = solo
        .conn
        .amplification_budget()
        .expect("the roam re-arms the budget");
    assert_eq!(
        credited,
        (constants::DATA_HEADER_LEN + 1 + constants::AEAD_TAG_LEN) as u64,
        "the roaming packet — a 1-byte PING plaintext — and only it, credits \
         the fresh counter",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §7.5's contested-probe floor is a **second** consumer of
// `on_ack_coverage`, and ruling 208 does not reach it
//
// `CONTRACT-7b.md` §1.7's boxed warning: *"Deleting the function wholesale
// deletes ruling 176's two exits from the pending state and silently
// disables the contested machinery, which `tests_contested.rs` covers and
// which **no wire test would notice**."*
//
// `tests_contested.rs` is not this author's file and the contract says two
// of its assertions expire with ruling 168. This section is the
// independent, ruling-208-shaped restatement of the property those
// assertions were protecting, in a file the remediation cannot expire.
// ═══════════════════════════════════════════════════════════════════════

/// An ACK still clears a pending contested mark, even though it no longer
/// validates an address.
///
/// **This is the test that catches the wholesale deletion.** Ruling 208
/// removes the ACK's *amplification* role and says nothing about §7.5's
/// probe floor; ruling 210's integration hazard names the mistake in
/// advance: *"Remove the amplification role only."*
///
/// The two halves are asserted together on purpose. A build that deletes
/// `on_ack_coverage` entirely passes every path-validation test in this
/// file — nothing on the wire changes — and silently turns ruling 176's
/// clear-exit off.
#[test]
fn an_ack_still_clears_a_pending_contested_mark() {
    let mut solo = Solo::installed_at(origin());

    // §7.5's `Pending` state is reachable only on a connection that has
    // roamed or has not yet validated — the mark and the transmission
    // coincide on a validated address. So: roam first.
    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &ping_frame());

    // Something must be in flight for an ACK to cover: seal a packet, then
    // mark. The mark records the counter the next seal will use.
    let now = now + Duration::from_millis(10);
    solo.conn.mark_contested(now);
    let floor = solo
        .conn
        .contested()
        .floor()
        .expect("the mark records a probe floor");

    // An ACK covering the probe floor, from the current address.
    let now = now + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &ack_frame(floor));
    assert_alive(&d);

    assert_eq!(
        solo.conn.contested().floor(),
        None,
        "**ruling 176**: an ACK covering the probe floor is one of the two \
         exits from the pending state, and ruling 208 does not touch it. A \
         build that deleted `on_ack_coverage` wholesale to remove ruling \
         168's machinery fails here and nowhere else.",
    );

    // The other half, and the reason this test is in *this* file: the same
    // ACK must **not** have validated the address.
    assert!(
        solo.conn.amplification_budget().is_some(),
        "one ACK, two floors, and after ruling 208 it feeds exactly one of \
         them",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 212(c) — the rank, and the pump's early return
//
// ⚠ **CONFLICT, REPORTED AND NOT RESOLVED (working rule 3).**
//
// `SPEC.md` at this file's base commit `c131904` — the ruling-212 sweep
// itself — ranks the path frames **below** the contested probe, in three
// places, and argues for that placement:
//
//   - §7.3:2370-2374, the normative numbered list: 1 CLOSE, 2 contested
//     probe, **3 PATH_RESPONSE, 4 PATH_CHALLENGE**, 5 pure ACKs.
//   - §7.3:2376-2396: *"They are placed **under** the contested probe
//     rather than above it because the probe's deadline is a **liveness
//     verdict** that a delay converts into a death."*
//   - §14.5:4700-4703: *"`PATH_RESPONSE` and `PATH_CHALLENGE` rank
//     immediately below the probe and above the pure ACK."*
//
// and §7.3:2398-2416 carries a `[FLAGGED FOR RULING]` block saying the
// rank-2/rank-4 interaction is unresolved, *"an implementation that hits it
// should stop and ask rather than pick."*
//
// **Ruling 212(c) rules the opposite**: *"`PATH_CHALLENGE` and
// `PATH_RESPONSE` rank immediately after CLOSE, **above** the contested
// probe... The pump's early return is consequently **wrong as written** and
// is the implementer's to fix: it may not block the one frame that ends the
// state it is protecting."*
//
// The sweep landed the flag; the maintainer then closed it; §7.3 was never
// re-swept with the answer. This author's brief names 212(c), so the tests
// below are written to 212(c) — and the conflict is reported rather than
// silently picked.
//
// The two tests are deliberately split by how much they depend on it:
// [`a_pending_contested_probe_does_not_block_the_challenge`] asserts only
// what **both** readings agree on, and [`the_challenge_outranks_the_
// contested_probe`] asserts the disputed rank alone.
// ═══════════════════════════════════════════════════════════════════════

/// A pending contested probe must not prevent the challenge being built.
///
/// **Both readings of the conflict above agree on this**, which is why it
/// is a separate test from the rank. §7.3's own flag block does the
/// arithmetic: *"probe and challenge together cost 14 B of header + 1 B of
/// PING + 9 B of challenge + a 16 B tag = **40 B**, inside the 90 B floor
/// computed above"* — so rank 2 outranking rank 4 *"does not mean rank 4 is
/// never built, only that it yields when the budget cannot hold both, and
/// here the budget can."* Ruling 212(c) reaches the same operational
/// conclusion from above.
///
/// **The degenerate build this catches is the one that exists today**: the
/// pump returns early on a pending mark, so nothing ranked below it is
/// built on that pass, and an address that is simultaneously unvalidated
/// and holding a pending probe never emits its challenge — *"the two states
/// co-occur by construction rather than by coincidence"*.
#[test]
fn a_pending_contested_probe_does_not_block_the_challenge() {
    let mut solo = Solo::installed_at(origin());

    // Roam: the address is now unvalidated, which is the only state in
    // which `Contested::Pending` is reachable at all.
    let now = origin() + Duration::from_millis(10);
    let issued = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(issued.len(), 1, "the arming's challenge");

    let now = now + Duration::from_millis(10);
    solo.conn.mark_contested(now);
    assert!(
        solo.conn.contested().is_pending(),
        "precondition: the mark is pending, not yet transmitted",
    );

    // Drive one pump with the probe still pending.
    let now = now + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &ping_frame());
    let frames = solo.drain_frames(&d);

    assert!(
        !challenges(&frames).is_empty(),
        "**ruling 212(c)**: the pump's early return on a pending probe \"may \
         not block the one frame that ends the state it is protecting\". \
         Frames seen: {frames:?}",
    );
}

/// The disputed half, on its own: the challenge is **packed first**.
///
/// Ruling 212(c)'s argument — *"everything else in the order competes for
/// the budget; the challenge dissolves it. Ranking the output that removes
/// the constraint above the outputs that consume it is not a preference, it
/// is the only ordering that terminates."*
///
/// §8.5 says the same one layer down, and §8.5 **is** swept for 208:
/// *"`PATH_RESPONSE` and `PATH_CHALLENGE` **first among the control
/// frames**"* — which is `CONTRACT-7b.md` §1.8's `Stage::Control`, ahead of
/// `Stage::Fill`. So the packing-order half of this is unambiguous even
/// though the §7.3 rank is not, and that is what this test asserts: the
/// challenge precedes the probe's PING in the packet.
///
/// **This test asserts the disputed reading.** If the maintainer resolves
/// the conflict the other way, this is the test to invert — not the two
/// above it.
#[test]
fn the_challenge_outranks_the_contested_probe() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let _ = roam_and_collect(&mut solo, now, c_addr());

    let now = now + Duration::from_millis(10);
    solo.conn.mark_contested(now);

    let now = now + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &ping_frame());

    // §8.5's packing rules are statements about **one packet** and are
    // unassertable once flattened, so the per-packet form is used.
    let packets = solo.packets(&d);
    let carrying = packets
        .iter()
        .find(|p| p.iter().any(|f| matches!(f, Wire::PathChallenge(_))))
        .expect("some packet carries the challenge");

    let challenge_at = carrying
        .iter()
        .position(|f| matches!(f, Wire::PathChallenge(_)))
        .expect("just found it");

    if let Some(ping_at) = carrying.iter().position(|f| matches!(f, Wire::Ping)) {
        assert!(
            challenge_at < ping_at,
            "**ruling 212(c)** / §8.5: the path frames are first among the \
             control frames, ahead of the probe's PING. Packet: {carrying:?}",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The escape from a scarce budget — §7.3's no-deadlock proof
//
// ⚠ **SECOND CONFLICT, REPORTED AND NOT RESOLVED (working rule 3).**
//
// `CONTRACT-7b.md` §1.4 converts the pump's existing `validate` branch from
// `packing.ping()` to a `PathChallenge` and says all four of its documented
// boundaries *"carry over **unchanged in force**"*. Two of those boundaries
// are `mod.rs:1968-2009`'s (1) *nothing owed ⇒ no challenge* and (2) *a bare
// ACK with nothing else owed ⇒ no challenge*, and the branch they guard is
// `if (probe || validate) && !elicits`.
//
// `SPEC.md` §8.7:3585-3590 owes the challenge on a **strictly broader**
// condition: *"the sender re-emits it ... **whenever §7.3's budget admits a
// packet and the address is still unvalidated**"*.
//
// They differ exactly on a packet that **already elicits**. Under ruling
// 168 the `!elicits` guard was sound because eliciting produced the ACK
// that validated; ruling 208 removed that implication and the guard's own
// in-code rationale says so in as many words — *"Validation arrives only on
// an ACK covering `validation_floor` (ruling 168), so a packet shrunk to fit
// the budget is useless if what fits elicits nothing"*. After 208, a shrunk
// packet carrying STREAM data elicits, carries no challenge, draws an ACK,
// and validates nothing: **ruling 203's stall, one indirection later**,
// which §7.3:2249-2256 says this design *"does not repair and does not
// weaken"*.
//
// This is working rule 4's shape — the token was changed and the rationale
// it rested on was not re-read. Working rule 3's tiebreak points the same
// way: **follow the statement some other proof depends on**, and §7.3's
// no-deadlock proof depends on *"what it admits is enough to elicit the
// `PATH_RESPONSE` that ends it"*, which requires the challenge to be
// **in** the packet.
//
// The tests below are written to §8.7. A build faithful to
// `CONTRACT-7b.md` §1.4's literal `!elicits` fails them, and this author
// believes that build is wrong — but the call is the maintainer's.
// ═══════════════════════════════════════════════════════════════════════

/// A packet leaving for an unvalidated address carries the challenge, even
/// when it already elicits.
///
/// §8.7: *"whenever §7.3's budget admits a packet and the address is still
/// unvalidated"*. See the section header for the conflict this depends on.
///
/// **The degenerate build**: one retaining `!elicits`. Here the core owes
/// an ACK for the peer's PING, so the packet elicits nothing by itself but
/// the *next* one carrying stream data does — and under `!elicits` neither
/// carries a challenge while any ack-eliciting frame is present.
#[test]
fn a_packet_leaving_for_an_unvalidated_address_carries_the_challenge() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &ping_frame());
    assert!(
        solo.conn.amplification_budget().is_some(),
        "precondition: unvalidated",
    );

    // Owed application data, so the pump has an ack-eliciting reason to
    // build a packet.
    let r = solo.conn.open(Dir::Uni).expect("a uni stream");
    let now = now + Duration::from_millis(10);
    let _ = write_all(&mut solo.conn, now, r, &[0x7u8; 512]);

    let d = drain(&mut solo.conn);
    let frames = solo.drain_frames(&d);

    assert!(
        !challenges(&frames).is_empty(),
        "§7.3's no-deadlock proof needs the escape **inside** the packet the \
         budget allowed: an ACK no longer ends the scarcity, so a packet \
         that merely elicits is not an escape. Frames: {frames:?}",
    );
}

/// The full escape, end to end, under the smallest budget an arming can
/// create.
///
/// §7.3:2241-2247: *"3× the **smallest** packet that can arm the budget —
/// §7.5's 30-byte keepalive, 90 B — still admits a packet carrying the
/// challenge"*. The roam here is §3.4's empty-plaintext keepalive, so the
/// budget is exactly 90 bytes, and the connection has 2 KiB of application
/// data it cannot send until the address validates.
///
/// **This is ruling 203's scenario with ruling 208's predicate**, and it is
/// the one test here that fails for a build that is individually correct
/// everywhere and never actually escapes.
#[test]
fn a_ninety_byte_budget_still_lets_the_address_validate() {
    let mut solo = Solo::installed_at(origin());

    // The smallest possible arming: an empty-plaintext keepalive.
    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &[]);
    let (_, credited) = solo
        .conn
        .amplification_budget()
        .expect("the keepalive roam arms the budget");
    assert_eq!(
        credited,
        (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64,
        "precondition: a 30-byte credit, so a 90-byte budget",
    );

    // Application data far larger than the budget.
    let r = solo.conn.open(Dir::Uni).expect("a uni stream");
    let now = now + Duration::from_millis(10);
    let _ = write_all(&mut solo.conn, now, r, &[0x9u8; 2048]);

    let d = drain(&mut solo.conn);
    let frames = solo.drain_frames(&d);
    let issued = challenges(&frames);
    assert!(
        !issued.is_empty(),
        "the budget admits a challenge datagram (39 B against 90 B) and the \
         pump must size to it: {frames:?}",
    );

    // The peer answers, and the cap lifts.
    let now = now + Duration::from_millis(20);
    let _ = solo.deliver_from(now, c_addr(), &path_response_frame(issued[0]));
    assert_eq!(
        solo.conn.amplification_budget(),
        None,
        "one round trip ends the scarcity — which is what the budget is for",
    );
}

/// Boundary (1) survives ruling 208: an **idle** unvalidated connection
/// with nothing owed emits no challenge.
///
/// This is the one of `mod.rs:1968-2009`'s four boundaries that both
/// readings of the conflict above agree on, because §8.7 owes the challenge
/// on *"a packet the budget admits"* and does not ask the pump to
/// manufacture one. *"An idle unvalidated connection gets no PING.
/// Validation is not a goal in itself — it is what lets held output leave,
/// and there is none."*
///
/// **Working rule 9.** Without this, every test above is passed by a build
/// that emits a challenge on every pump unconditionally — which is an
/// unprompted probe train on an idle connection, and the thing boundary (1)
/// exists to forbid.
#[test]
fn an_idle_unvalidated_connection_owing_nothing_emits_no_challenge() {
    let mut solo = Solo::installed_at(origin());

    // Roam with an empty keepalive: nothing is ack-eliciting, so nothing is
    // owed in reply.
    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &[]);
    let frames = solo.drain_frames(&d);
    assert!(
        challenges(&frames).is_empty(),
        "boundary (1): nothing is owed, so no packet is manufactured to \
         carry a challenge: {frames:?}",
    );

    // And it stays quiet across a timer pass, with nothing owed.
    let now = now + Duration::from_millis(50);
    solo.conn.handle_timeout(now);
    let d = drain(&mut solo.conn);
    let frames = solo.drain_frames(&d);
    assert!(
        challenges(&frames).is_empty(),
        "still nothing owed, still no challenge: {frames:?}",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// F1 — no core ever announces a deadline in the past
//
// `CONTRACT-7b.md` §4.3 is binding on the *shape* of this test, and says
// why: *"A blind test author who tries to assert '100 % CPU' will write a
// test that never returns."* On a paused clock a livelock never advances
// virtual time, so the test hangs rather than fails.
// ═══════════════════════════════════════════════════════════════════════

/// The pin: with a keepalive held by the budget, the announced deadline is
/// `None` or strictly in the future.
///
/// §4.3's four steps, and its two-build separation: *"Against today's build
/// it fails; against a build that suppresses correctly it passes; and
/// against a build that suppresses **the death clock too** it also passes —
/// so a **second** assertion is required."* That second assertion is the
/// `Liveness` timer half below.
#[test]
fn a_held_keepalive_never_announces_a_deadline_in_the_past() {
    let mut solo = Solo::installed_at(origin());

    // Roam on the smallest possible credit, so the budget holds output.
    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &[]);
    assert!(
        solo.conn.amplification_budget().is_some(),
        "precondition: the budget is armed and scarce",
    );

    // Well past `last_send + KEEPALIVE_TIMEOUT`.
    let now = now + constants::KEEPALIVE_TIMEOUT + Duration::from_secs(1);
    solo.conn.handle_timeout(now);
    let d = drain(&mut solo.conn);

    match d.deadline {
        None => {}
        Some(at) => assert!(
            at > now,
            "**ruling 141's spin class**: a core announced a deadline at or \
             before `now` ({:?} before now), which the driver sleeps on and \
             which completes immediately, re-firing the same timer forever",
            now.saturating_duration_since(at),
        ),
    }

    // The second assertion §4.3 requires. Suppressing the death clock along
    // with the keepalive turns a spinning connection into an **immortal**
    // one, which is worse — *"precisely the collapse ruling 182's beacon
    // proof warns about"*. The connection must still be scheduled to die.
    solo.conn
        .handle_timeout(now + constants::DEAD_TIMEOUT + Duration::from_secs(1));
    let d = drain(&mut solo.conn);
    assert_eq!(
        d.closed(),
        Some(ConnectionLost::TimedOut),
        "§7.5's death clock is **not** suppressed: an address that never \
         answers still kills the session at DEAD_TIMEOUT (`CONTRACT-7b.md` \
         §1.5 — 'Nothing new. No new timer, no new event, no new error \
         variant.')",
    );
}

/// `ConnectionLost::TimedOut` and no new variant.
///
/// `CONTRACT-7b.md` §1.5: *"An implementer who invents a
/// `PathValidationFailed` variant, a validation timer, or a challenge retry
/// counter has exceeded this contract."* Working rule 8 reads ruling 208's
/// list of what it adds as closed, and §18.1's taxonomy is ratified.
///
/// This test cannot assert the *absence* of an enum variant — that is a
/// compile-time fact and a blind author cannot reach it. What it can assert
/// is the observable consequence: the unanswered challenge produces the
/// **existing** timeout and nothing else.
#[test]
fn an_unanswered_challenge_dies_as_an_ordinary_timeout() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let issued = roam_and_collect(&mut solo, now, c_addr());
    assert_eq!(issued.len(), 1, "a challenge went out and is unanswered");

    let now = now + constants::DEAD_TIMEOUT + Duration::from_secs(1);
    solo.conn.handle_timeout(now);
    let d = drain(&mut solo.conn);

    assert_eq!(
        d.closed(),
        Some(ConnectionLost::TimedOut),
        "the pre-168 behaviour for an address that never answers, and the \
         correct one: an address that cannot answer is one to stop sending to",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Two cores, both real — the same properties without a raw peer
// ═══════════════════════════════════════════════════════════════════════

/// Two real cores complete a roam through challenge and response.
///
/// `Solo` drives one core against a hand-rolled peer, so every test above
/// asserts what the core **emits** and what it **accepts** — but not that
/// the two halves agree. This does, and it is the only test here that would
/// catch a core whose emitter and parser share one mistake.
///
/// It uses [`Pair::pump_from`], added to `testfix.rs` by this author for
/// this: `pump` drives both cores' timers but hard-codes the default
/// addresses, so a roamed peer's packets arrive from the address it has
/// already left and the session roams back on every round.
#[test]
fn two_real_cores_validate_a_roamed_address() {
    let mut pair = Pair::installed_at(origin());

    // A owes something, so packets flow.
    let r = pair.a.open(Dir::Uni).expect("a uni stream");
    let now = origin() + Duration::from_millis(10);
    let _ = write_all(&mut pair.a, now, r, &[0x3u8; 256]);

    // A moves. Every A→B packet now arrives from `c_addr`; B→A is
    // unchanged.
    let now = now + Duration::from_millis(10);
    let (_, db) = pair.step_from(now, c_addr(), b_addr());
    let moved = db.count_events(|e| matches!(e, ConnEvent::AddressMoved { .. }));
    assert_eq!(moved, 1, "B sees the move");
    assert!(
        pair.b.amplification_budget().is_some(),
        "B arms a budget against A's new address",
    );

    // Let the two settle. The challenge goes out, the response comes back.
    let now = now + Duration::from_millis(20);
    let _ = pair.pump_from(now, c_addr(), b_addr());

    assert_eq!(
        pair.b.amplification_budget(),
        None,
        "**the end-to-end property**: B challenged, A echoed, and B's cap \
         lifted. A build whose emitter and parser share one mistake — a \
         swapped code point, a reversed body — passes every one-sided test \
         above and fails here.",
    );
    assert_eq!(
        pair.b.remote_address(),
        Some(c_addr()),
        "and it validated the address it actually moved to",
    );
}

/// Neither core ever announces a deadline in the past across a full roam
/// and validation.
///
/// The `debug_assert!` `CONTRACT-7b.md` §4.2 puts in `Driver::deadline` is
/// shell-side and unreachable from a core test; this is the core-side twin,
/// applied to the whole exchange rather than to one contrived state.
/// Working rule 13 in miniature — the fixture bounds the coverage, and a
/// spin is invisible on the wire.
#[test]
fn no_core_announces_a_past_deadline_across_a_roam() {
    let mut pair = Pair::installed_at(origin());

    let r = pair.a.open(Dir::Uni).expect("a uni stream");
    let mut now = origin() + Duration::from_millis(10);
    let _ = write_all(&mut pair.a, now, r, &[0x3u8; 4096]);

    for _ in 0..8 {
        now += Duration::from_millis(25);
        let (da, db) = pair.step_from(now, c_addr(), b_addr());
        for (side, d) in [("A", &da), ("B", &db)] {
            if let Some(at) = d.deadline {
                assert!(
                    at > now,
                    "{side} announced a deadline at or before `now` — \
                     ruling 141's spin class",
                );
            }
        }
    }
}

/// `ConnOutput` gains nothing for ruling 208.
///
/// `CONTRACT-7b.md` §1.5: *"No new timer, no new event, no new error
/// variant."* A blind author cannot assert the absence of an enum variant,
/// but it can assert that a full challenge/response exchange produces only
/// the outputs §16.4 already defines — so a build that minted a
/// `ConnEvent::PathValidated` shows up as an unexpected event here.
#[test]
fn path_validation_emits_no_new_connection_event() {
    let mut solo = Solo::installed_at(origin());

    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &ping_frame());
    let issued = challenges(&solo.drain_frames(&d));
    assert_eq!(issued.len(), 1);

    let now = now + Duration::from_millis(20);
    let d = solo.deliver_from(now, c_addr(), &path_response_frame(issued[0]));

    // §7.3's observability is `remote_address()` plus the `slither::roam`
    // trace; the only event a roam emits is `AddressMoved`, and that fired
    // on the roam, not here.
    let events = count_all_events(&d);
    let moved = d.count_events(|e| matches!(e, ConnEvent::AddressMoved { .. }));
    assert_eq!(
        events, moved,
        "validation is not an event: the only events on this drain are the \
         ones §16.4 already defines. Outputs: {:?}",
        d.outs,
    );
}

/// Total `ConnOutput::Event` count on a drain.
fn count_all_events(d: &Drained) -> usize {
    d.outs
        .iter()
        .filter(|o| matches!(o, ConnOutput::Event(_)))
        .count()
}

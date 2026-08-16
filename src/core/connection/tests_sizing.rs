//! Ruling 203's **budget-aware output sizing**, written blind.
//!
//! Written by an independent author from ruling 203, ruling 207's three
//! failure modes, rulings 168/170 and §7.3/§7.5, against a tree that does
//! not contain the fix (base `ebc1663`). **This file has never been
//! compiled** — `mod.rs` at that commit does not declare the module, and
//! declaring it belongs to the implementer (working rule 6).
//!
//! # The behaviour under test
//!
//! When §7.3's budget is armed and *tight*, the pump must **size its output
//! to the room that remains** rather than build a full-size candidate and
//! hold it. Ruling 203: *"bound the packing target by the remaining budget —
//! `min(MAX_DATAGRAM, room)` — so the first post-roam packet is small,
//! ack-eliciting, and validates the address at once."*
//!
//! The two observable consequences, and they are what is asserted here:
//! application data **moves promptly** after a roam, and what moves is
//! **ack-eliciting**, so the peer's ACK can cover `validation_floor` and
//! disarm the budget (ruling 168).
//!
//! # Working rule 9, applied deliberately
//!
//! *A bound is only a test if the degenerate case violates it.* "The packet
//! fits the budget" is satisfied by a build that sends nothing at all —
//! which is precisely today's build. So **every fit assertion here is paired
//! with a progress assertion from the other side**, and the boundary tests
//! are two-sided (slice 1 tested `LEN` and `LEN-1` and never `LEN+1`).
//!
//! The three builds ruling 207 names, and where each dies:
//!
//! | wrong build | ruling | the assertion that separates it |
//! |---|---|---|
//! | holds a full-size candidate (today) | 203 | `!sizes.is_empty()` in every test |
//! | loosens `admits`, or exempts post-roam output | 207(a) | `total <= room`, and the running `sent <= 3 × recv` |
//! | shrinks to a bare ACK | 207(b) | `is_ack_eliciting`, and the Solo/Pair round trip that validates |
//! | caps *plaintext* at the remaining *datagram* bytes | 207(c) | `first > room - OVERHEAD` — the 30-byte overshoot re-refuses its own packet, so nothing leaves |
//! | sizes to a fixed small constant | — | `the_packet_grows_with_the_room…`, two rooms, two sizes |
//! | drops the `MAX_DATAGRAM` half of the `min` | 203 | `s <= MAX_DATAGRAM` at a room of 1203 |
//!
//! # What is deliberately **not** asserted
//!
//! Ruling 207(b) closes with *"whether the pump owes a PING when the
//! admitted room holds nothing ack-eliciting is a genuine question this pass
//! must answer, not assume."* It is an open question, so nothing here pins
//! an answer to it. A test asserting either direction would be this author
//! ratifying a ruling.
//!
//! Ruling 198 records priority positions 3–7 as **unbuilt debt**. Nothing
//! here assumes a budget-aware scheduler across those positions.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::constants;
use crate::core::connection::Connection;
use crate::core::connection::stream_id::Dir;
use crate::core::connection::testfix::{
    Drained, Pair, Solo, Wire, drain, put, t0, tick, v4, write_all,
};
use crate::packet::ReferenceSuite;

type Suite = ReferenceSuite;

// ═══════════════════════════════════════════════════════════════════════
// Vocabulary
// ═══════════════════════════════════════════════════════════════════════

/// §3.4's per-packet overhead, in datagram bytes: header + AEAD tag.
///
/// **Ruling 207(c)'s whole subject.** The budget is charged in *datagram*
/// bytes and `Packing` is bounded in *plaintext* bytes, and the two differ
/// by exactly this. A fix that caps the plaintext at the remaining datagram
/// bytes overshoots by 30 and re-refuses its own packet.
const OVERHEAD: u64 = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;

/// A third address — where the peer moves to. Deliberately not `a_addr()`
/// or `b_addr()`, which the fixture already uses as the two anchors.
fn c_addr() -> SocketAddr {
    v4(9, 41_000)
}

/// §7.3's remaining room, in **datagram** bytes: `3 × recv − sent`.
///
/// Read through the existing `amplification_budget()` accessor rather than
/// hardcoded, so a fixture whose arming credit drifts turns the *premise*
/// assertion red rather than silently weakening every bound below it.
fn room(conn: &Connection<Suite>) -> u64 {
    let (sent, recv) = conn
        .amplification_budget()
        .expect("this test needs an armed, unvalidated budget");
    constants::AMPLIFICATION_FACTOR
        .saturating_mul(recv)
        .saturating_sub(sent)
}

/// Every datagram a drain produced, by length.
fn sizes(d: &Drained) -> Vec<u64> {
    d.transmits().iter().map(|t| t.data.len() as u64).collect()
}

/// How many bytes of STREAM payload a decoded drain carried.
///
/// **The progress side of every bound in this file.** "A packet went out"
/// is true of a build that emits a bare ACK; "application data moved" is
/// not.
fn stream_bytes(packets: &[Vec<Wire>]) -> usize {
    packets
        .iter()
        .flatten()
        .filter_map(|f| match f {
            Wire::Stream { data, .. } => Some(data.len()),
            _ => None,
        })
        .sum()
}

/// Whether a packet can elicit the ACK ruling 168 requires.
///
/// Written as *everything except the three quiet frames* — §12.4 (pure-ACK
/// packets *"are not ack-eliciting (no ACK-of-ACK loops)"*), §8.4's PADDING,
/// and CLOSE (§16.3, *"CLOSE is not ack-eliciting"*). Deliberately **not**
/// `frame::packet_is_ack_eliciting`: a classifier that agrees with the
/// implementation it is testing asserts nothing (testfix's own rationale for
/// re-decoding the wire by hand).
///
/// The open direction is the safe one here: an unknown frame counts as
/// eliciting, so this predicate can only ever fail a build that emits
/// *nothing but* quiet frames — which is exactly ruling 207(b)'s failure.
fn is_ack_eliciting(frames: &[Wire]) -> bool {
    frames
        .iter()
        .any(|f| !matches!(f, Wire::Padding | Wire::Ack { .. } | Wire::Close { .. }))
}

/// §3.4's counter field, straight off the datagram header.
fn counter_of(dgram: &[u8]) -> u64 {
    u64::from_le_bytes(
        dgram[6..14]
            .try_into()
            .expect("§3.4's 8-byte counter at offset 6"),
    )
}

/// §8.4's ACK covering exactly one counter.
fn ack_frame(largest: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, constants::FRAME_ACK);
    put(&mut f, largest);
    put(&mut f, 0); // ack_delay
    put(&mut f, 0); // range_count
    put(&mut f, 0); // first_range
    f
}

/// A core that has just **roamed**, its §7.3 budget armed by a packet whose
/// plaintext is `plaintext` bytes of PADDING and nothing else.
///
/// PADDING rather than anything ack-eliciting on purpose: the roaming packet
/// then owes no ACK, so the *only* thing the pump can put on the wire
/// afterwards is application data. That removes the one way a stalled build
/// could look alive — emitting the ACK it happened to owe — and makes
/// `!sizes.is_empty()` a statement about the defect and nothing else.
///
/// `plaintext = 0` reproduces ruling 203's own arithmetic exactly: §7.5's
/// keepalive is §3.4's empty plaintext, a 30-byte datagram, and `3 × 30` is
/// the 90 bytes the ruling names.
fn roamed_with(now: Instant, plaintext: usize) -> Solo {
    let mut solo = Solo::installed_at(now);
    assert!(
        solo.conn.amplification_budget().is_none(),
        "fixture premise: this core starts **validated** (ruling 200 — the \
         budget arms on the msg1 anchor or on a roam, and this constructor \
         is neither), so the roam below is demonstrably what arms it"
    );

    let pad = vec![constants::FRAME_PADDING as u8; plaintext];
    let _ = solo.deliver_from(now, c_addr(), &pad);

    let credit = OVERHEAD + plaintext as u64;
    assert_eq!(
        solo.conn.amplification_budget(),
        Some((0, credit)),
        "fixture premise: `commit_roam` arms at exactly the roaming \
         datagram's length (rulings 168/169) and nothing has been spent — a \
         PADDING-only plaintext is not ack-eliciting, so no ACK is owed"
    );
    solo
}

// ═══════════════════════════════════════════════════════════════════════
// 1 — the headline: a tight budget still moves data, at once
// ═══════════════════════════════════════════════════════════════════════

/// Ruling 203's exact scenario, at its exact numbers.
///
/// A connection roams on a 30-byte keepalive. Its budget is 90 datagram
/// bytes. 2 KiB of application data is pending. Ruling 203: *"a ~90-byte
/// packet would fit immediately, be ack-eliciting, and validate the address
/// in one round trip"* — and today *"nothing shrinks to fit"*, so nothing
/// leaves for ~20 rounds.
///
/// **What the broken build does:** builds a 1 200-byte candidate,
/// `admits(1200)` refuses it against 90 bytes of room, `restore` + `break`.
/// Zero transmits. Ruling 207(c)'s units error lands in the same place from
/// the other direction: plaintext capped at 90 gives a 120-byte datagram,
/// which is also refused.
#[test]
fn a_tight_budget_after_a_roam_still_moves_application_data_at_once() {
    let now = t0();
    let mut solo = roamed_with(now, 0);

    let before = room(&solo.conn);
    assert_eq!(
        before, 90,
        "ruling 203's own arithmetic: 3 × a 30-byte keepalive"
    );

    let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut solo.conn, now, r, &[0xC3u8; 2048]);
    let d = drain(&mut solo.conn);
    let s = sizes(&d);

    // ── the progress side. Working rule 9: this is the assertion a bare
    //    "the packet fits" bound would pass vacuously, because a build that
    //    sends nothing satisfies every upper bound for free.
    assert!(
        !s.is_empty(),
        "ruling 203: a ~90-byte packet fits *now*, and holding a full-size \
         candidate is the one behaviour that prevents the escape the budget \
         exists to permit"
    );

    // ── the fit side. Ruling 207(a): the predicate does not move.
    assert!(
        s.iter().all(|&n| n <= before),
        "§7.3 binds all output: {s:?} against {before} bytes of room"
    );
    assert!(
        s.iter().sum::<u64>() <= before,
        "§7.3 binds the *episode*, not just each packet: {s:?} sums past \
         {before}"
    );

    // ── and what moved was application data, not a bare ACK (ruling 207(b)).
    let packets = solo.packets(&d);
    assert!(
        stream_bytes(&packets) > 0,
        "the point of the fix is that **application data** moves; a packet \
         carrying no STREAM bytes leaves the stall exactly where it was, one \
         indirection later (ruling 207(b))"
    );
    assert!(
        is_ack_eliciting(&packets[0]),
        "ruling 168 validates only on an ACK covering `validation_floor`, so \
         non-ack-eliciting output can never end the unvalidated state"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2 — the round trip the shrunken packet exists to buy
// ═══════════════════════════════════════════════════════════════════════

/// The purpose, end to end on one core: the shrunken packet's **own
/// counter** is at or above `validation_floor`, so the peer's ACK of that
/// packet disarms the budget — *"validates the address at once"*.
///
/// **What the broken build does:** with no transmit there is no counter to
/// ACK, and the connection cannot leave the unvalidated state at all. A
/// build that shrinks to a bare ACK (ruling 207(b)) fails
/// `is_ack_eliciting`: a peer does not ACK an ACK (§12.4, *"no ACK-of-ACK
/// loops"*), so the counter is never covered.
///
/// The closing assertion is the other half of ruling 168 — once validated,
/// the cap is genuinely **gone**, not merely enlarged. A build that shrank
/// correctly but kept sizing to a stale room would still be crawling.
#[test]
fn the_shrunken_packet_carries_a_counter_the_peers_ack_can_validate() {
    let now = t0();
    let mut solo = roamed_with(now, 0);
    let before = room(&solo.conn);
    let floor = solo.conn.validation_floor();

    let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut solo.conn, now, r, &[0x5Au8; 2048]);
    let d = drain(&mut solo.conn);
    let ts = d.transmits();
    assert!(
        !ts.is_empty(),
        "nothing left the sender, so there is no counter for the peer to \
         ACK and ruling 168's proof can never be produced (ruling 203)"
    );

    let counter = counter_of(&ts[0].data);
    assert!(
        counter >= floor,
        "ruling 168: the floor is *the counter the next seal will use* at \
         the arming, so every packet sealed after the roam is at or above \
         it — {counter} against a floor of {floor}"
    );

    let packets = solo.packets(&d);
    assert!(
        is_ack_eliciting(&packets[0]),
        "ruling 207(b): a packet sized to fit the budget is useless if what \
         fits is a bare ACK"
    );

    // The peer ACKs it — **from the address the session roamed to**, which
    // is what ruling 168 requires (*"an authenticated, window-fresh packet
    // from that address"*).
    let d_ack = solo.deliver_from(now, c_addr(), &ack_frame(counter));
    assert!(
        solo.conn.amplification_budget().is_none(),
        "ruling 168: one ACK covering the floor is return routability \
         proven, and the budget disarms"
    );

    // The cap is gone, not merely wider.
    let _ = write_all(&mut solo.conn, now, r, &[0x5Au8; 8192]);
    let d_after = drain(&mut solo.conn);
    let biggest = sizes(&d_ack)
        .into_iter()
        .chain(sizes(&d_after))
        .max()
        .unwrap_or(0);
    assert!(
        biggest > before,
        "after validation nothing is bound by the old {before}-byte room; \
         the largest packet was {biggest}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3 — the sweep: every tight room admits a packet, and nearly fills it
// ═══════════════════════════════════════════════════════════════════════

/// Ruling 207(c) gets its own bound, at six different rooms.
///
/// A single room can hide an off-by-30: `min(MAX_DATAGRAM, room)` and
/// `min(MAX_DATAGRAM, room) - 30` differ by an amount that is a large
/// fraction of a 90-byte room and a rounding error in a 1 140-byte one. So
/// the fit is asserted **two-sided at each room**: at most `room`, and
/// strictly more than `room - OVERHEAD`.
///
/// **Why the lower bound is principled and not an arbitrary slack.** The
/// overhead is exactly 30 bytes. Landing 30 or more bytes short of the room
/// means one whole packet's overhead was subtracted twice — which is ruling
/// 207(c)'s error in its surviving form, the one where the packet is not
/// re-refused but is silently half the size it should be. A byte or two of
/// varint self-reference in the STREAM header cannot reach 30.
///
/// **What the broken builds do:** today's holds everything, at every room.
/// The double-subtracting build lands at `room - 30` or below.
#[test]
fn every_tight_room_admits_a_data_bearing_packet_that_nearly_fills_it() {
    for plaintext in [0usize, 10, 30, 90, 200, 350] {
        let now = t0();
        let mut solo = roamed_with(now, plaintext);
        let before = room(&solo.conn);
        assert!(
            before < constants::MAX_DATAGRAM as u64,
            "premise: room {before} must be *tight*, or the sizing rule is \
             not exercised at all"
        );

        let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
        let _ = write_all(&mut solo.conn, now, r, &[0x11u8; 4096]);
        let d = drain(&mut solo.conn);
        let s = sizes(&d);

        assert!(
            !s.is_empty(),
            "room {before}: nothing left the sender, with 4 KiB pending"
        );
        assert!(
            s.iter().sum::<u64>() <= before,
            "room {before}: §7.3 is exceeded by {s:?}"
        );
        assert!(
            s[0] > before.saturating_sub(OVERHEAD),
            "room {before}: the first packet is {} bytes, which is a whole \
             {OVERHEAD}-byte overhead short — ruling 207(c), the two units \
             differ by exactly the overhead",
            s[0]
        );

        let packets = solo.packets(&d);
        assert!(
            stream_bytes(&packets) > 0,
            "room {before}: the packet that fits carried no application data"
        );
        assert!(
            is_ack_eliciting(&packets[0]),
            "room {before}: ruling 207(b)"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4 — the `min` has two halves, and both are tested from both sides
// ═══════════════════════════════════════════════════════════════════════

/// `min(MAX_DATAGRAM, room)` — one room under the crossover, one exactly on
/// it, one over.
///
/// This is slice 1's one-sided-boundary lesson applied deliberately: testing
/// only rooms below `MAX_DATAGRAM` exercises the `room` half of the `min`
/// and never the `MAX_DATAGRAM` half, and a build that dropped the latter
/// would emit a **1 203-byte datagram** — over §3.1's 1 200-byte
/// non-fragmenting bound, and *admitted*, because the budget genuinely has
/// 1 203 bytes of room. Nothing else in this file would catch it.
///
/// The rooms are chosen so `3 × credit` lands exactly: a PADDING plaintext
/// of 369/370/371 bytes is a 399/400/401-byte datagram, funding 1 197 /
/// 1 200 / 1 203.
///
/// **What the broken builds do:** at 1 197 a build that caps only at
/// `MAX_DATAGRAM` builds 1 200, is refused, and sends nothing. At 1 203 a
/// build that caps only at `room` sends 1 203.
#[test]
fn the_packing_target_is_min_of_max_datagram_and_the_room_on_both_sides() {
    for (plaintext, expected) in [(369usize, 1_197u64), (370, 1_200), (371, 1_203)] {
        let now = t0();
        let mut solo = roamed_with(now, plaintext);
        let before = room(&solo.conn);
        assert_eq!(before, expected, "premise: the room straddles MAX_DATAGRAM");

        let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
        let _ = write_all(&mut solo.conn, now, r, &[0x99u8; 8192]);
        let d = drain(&mut solo.conn);
        let s = sizes(&d);

        assert!(
            !s.is_empty(),
            "room {before}: nothing left the sender, with 8 KiB pending"
        );
        assert!(
            s.iter().all(|&n| n <= constants::MAX_DATAGRAM as u64),
            "room {before}: the MAX_DATAGRAM half of `min(MAX_DATAGRAM, \
             room)` — {s:?} exceeds §3.1's non-fragmenting bound"
        );
        assert!(
            s.iter().all(|&n| n <= before),
            "room {before}: the `room` half of the `min` — {s:?}"
        );
        assert!(
            s.iter().sum::<u64>() <= before,
            "room {before}: §7.3 over the episode — {s:?}"
        );

        let target = before.min(constants::MAX_DATAGRAM as u64);
        assert!(
            s[0] > target - OVERHEAD,
            "room {before}: the first packet is {} against a target of \
             {target}; a whole {OVERHEAD}-byte overhead is missing",
            s[0]
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5 — the size follows the room, and is not a constant that happens to fit
// ═══════════════════════════════════════════════════════════════════════

/// **The assertion that survives if the saturation bound above is judged too
/// strict.**
///
/// Slice 2a shipped *"no interval exceeds base + jitter"*, which passed a
/// core with no jitter at all; the fix was to assert from the side that
/// separates — *not all intervals equal*. The same shape applies here. A
/// build that emits one small fixed-size packet whenever the budget is tight
/// satisfies every upper bound in this file, moves data, is ack-eliciting,
/// and validates the address — and is still not the rule ruling 203 states.
/// Three rooms, three strictly increasing first packets, and no absolute
/// size claimed anywhere.
#[test]
fn the_packet_grows_with_the_room_rather_than_sitting_at_a_fixed_size() {
    let mut observed: Vec<(u64, u64)> = Vec::new();

    for plaintext in [0usize, 90, 300] {
        let now = t0();
        let mut solo = roamed_with(now, plaintext);
        let before = room(&solo.conn);

        let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
        let _ = write_all(&mut solo.conn, now, r, &[0x22u8; 4096]);
        let d = drain(&mut solo.conn);
        let s = sizes(&d);
        assert!(!s.is_empty(), "room {before}: nothing left the sender");
        observed.push((before, s[0]));
    }

    for w in observed.windows(2) {
        let (small_room, small) = w[0];
        let (big_room, big) = w[1];
        assert!(
            big_room > small_room,
            "the fixture must present a strictly larger room: {small_room} \
             then {big_room}"
        );
        assert!(
            big > small,
            "the packing target is bounded by the *room* (ruling 203), so a \
             larger room must produce a larger first packet: {small} bytes \
             at room {small_room}, {big} bytes at room {big_room}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6 — shrinking is not a licence: the predicate still refuses (207(a))
// ═══════════════════════════════════════════════════════════════════════

/// Ruling 207(a): *"the change is to what the pump **builds**, never to what
/// the budget **permits**."*
///
/// The cumulative bound is unconditional and cannot produce a false red: a
/// build that exempts post-roam output from `admits`, or that treats a
/// shrunken packet as admissible by construction, pushes 4 KiB through a
/// 90-byte room and fails it. The emptiness check that follows is guarded by
/// the room it needs, because an over-conservative-but-correct build would
/// legitimately have room left for a second packet, and a test that reddens
/// on that is asserting something ruling 203 does not say.
///
/// The `is_some` assertion is the one ruling 168 makes load-bearing: no ACK
/// has arrived, so **nothing** may have validated this address. A build that
/// disarmed the budget to make its own packet fit would reopen the reflector
/// §7.3 exists to close, and would pass every size bound in this file.
#[test]
fn shrinking_to_fit_does_not_loosen_what_the_budget_permits() {
    let now = t0();
    let mut solo = roamed_with(now, 0);
    let before = room(&solo.conn);

    let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut solo.conn, now, r, &[0x33u8; 4096]);
    let first = drain(&mut solo.conn);
    let spent: u64 = sizes(&first).iter().sum();

    assert!(
        spent > 0,
        "ruling 203: something must move (working rule 9)"
    );
    assert!(
        solo.conn.amplification_budget().is_some(),
        "ruling 168: the address validates on an **ACK covering the floor** \
         and on nothing else. None has arrived, so the budget must still be \
         armed — a build that disarmed it to make its own packet fit passes \
         every size bound here and is the reflector §7.3 exists to close"
    );

    let after = room(&solo.conn);
    assert!(
        after < before,
        "the shrunken packet must be **charged**: {after} against {before}"
    );

    // A second pass at the same instant, with ~4 KiB still pending.
    tick(&mut solo.conn, now);
    let second = drain(&mut solo.conn);
    let more: u64 = sizes(&second).iter().sum();

    assert!(
        spent + more <= before,
        "§7.3 over the whole episode: {spent} + {more} against {before} \
         bytes of room (ruling 207(a) — the predicate does not move)"
    );

    if after < OVERHEAD + 1 {
        assert!(
            second.transmits().is_empty(),
            "{after} bytes of room cannot hold the minimum {}-byte datagram, \
             and §7.3 **holds** what does not fit — it does not truncate and \
             does not send anyway",
            OVERHEAD + 1
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7 — ruling 203's ~20 stalled rounds, round by round
// ═══════════════════════════════════════════════════════════════════════

/// *"2 048 bytes of application data cannot leave for ~20 keepalive rounds
/// (~200 s)."* This is that episode, asserted **per round**.
///
/// Each round delivers one further keepalive-sized packet from the same,
/// still-unvalidated address: pure §7.3 credit, no ACK, so ruling 168 never
/// fires and the budget never disarms. Every round therefore nets exactly
/// `3 × 30` bytes of fresh room, and every round must move application data.
///
/// **What the broken build does:** moves nothing, in every round — the
/// candidate is full-size each time and refused each time. A build that
/// shrinks only *once*, on the packet immediately after the roam, moves data
/// in round 0 and stalls from round 1; the loop catches that too, which a
/// single-shot test would not.
///
/// The per-round `sent <= 3 × recv` check is §7.3's actual invariant,
/// re-read from the accessor each round rather than inferred from the sizes.
#[test]
fn each_credited_round_moves_application_data_instead_of_banking_it() {
    let now = t0();
    let mut solo = roamed_with(now, 0);

    let r = solo.conn.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut solo.conn, now, r, &[0x44u8; 8192]);
    let _ = drain(&mut solo.conn);

    let mut delivered = 0usize;
    for round in 0..6 {
        // §7.5's keepalive is §3.4's empty plaintext — no frames, so no ACK
        // is owed and none is sent. Same source address: this credits the
        // budget, it does not roam and does not validate.
        let d = solo.deliver_from(now, c_addr(), &[]);
        let moved_on_recv = stream_bytes(&solo.packets(&d));

        // A build that plans on the timeout path rather than the receive
        // path is not wrong; take both before judging the round.
        tick(&mut solo.conn, now);
        let d2 = drain(&mut solo.conn);
        let moved_on_tick = stream_bytes(&solo.packets(&d2));

        let moved = moved_on_recv + moved_on_tick;
        assert!(
            moved > 0,
            "round {round}: 90 further bytes of room arrived and a \
             data-bearing packet fits them. Ruling 203 — the budget exists \
             to be *escaped* by a round trip, and holding a full packet is \
             the one behaviour that prevents the escape"
        );
        delivered += moved;

        let (sent, recv) = solo
            .conn
            .amplification_budget()
            .expect("no ACK has been sent, so the address is still unvalidated");
        assert!(
            sent <= constants::AMPLIFICATION_FACTOR.saturating_mul(recv),
            "round {round}: §7.3's inequality — {sent} sent against {recv} \
             received"
        );
    }

    // Not `> 0` — that is already implied by the loop and would assert
    // nothing (working rule 9). Six rounds each opening 90 datagram bytes of
    // room means ~60 plaintext bytes per round, so a build that moves the
    // room's worth each time lands in the hundreds, and one that dribbles a
    // byte per round to satisfy the loop lands at 6.
    assert!(
        delivered > 60,
        "six credited rounds moved {delivered} application bytes in total — \
         less than the plaintext room of a *single* round, so the sizing is \
         not tracking the budget"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 8 — two real cores: the roam, and the round trip that ends it
// ═══════════════════════════════════════════════════════════════════════

/// The whole point, with no synthesised frames anywhere: `b` roams, `b` has
/// application data, and `b`'s address is validated one round trip later.
///
/// **What the broken build does, and why this test needs `Pair`.** The
/// roaming packet here *is* ack-eliciting, so `b` owes an ACK — and today's
/// build can still emit that ACK on its standalone path. So "a packet went
/// out" proves nothing on this fixture, and the assertion has to be the end
/// state: an ACK is not ack-eliciting (§12.4), `a` therefore never ACKs it,
/// `b`'s counter at or above `validation_floor` is never covered, and
/// `b.amplification_budget()` never becomes `None`. That is ruling 207(b)'s
/// *"the connection stalls exactly as it does today, one indirection later"*,
/// made observable.
#[test]
fn two_cores_validate_the_roamed_address_within_one_round_trip() {
    let now = t0();
    let mut p = Pair::installed_at(now);
    assert!(
        p.b.amplification_budget().is_none(),
        "fixture premise: both halves of `installed_at` start validated"
    );

    // `a` moves. A small ack-eliciting packet from the new source is §7.2's
    // roam predicate in full (ruling 180 — at core level the source is
    // simply the address handed to `handle_datagram`).
    let ra = p.a.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut p.a, now, ra, &[0x77u8; 64]);
    let _ = p.drain_a();
    let _ = p.flush_a_to_b_from(now, c_addr());

    let before = room(&p.b);
    assert!(
        before < constants::MAX_DATAGRAM as u64,
        "premise: the roam leaves a room of {before}, which must be tight \
         enough that a full-size candidate does not fit"
    );

    // Now `b` has application data and an unvalidated address — ruling 203's
    // state exactly.
    let rb = p.b.open(Dir::Uni).expect("a uni stream opens");
    let _ = write_all(&mut p.b, now, rb, &[0x88u8; 4096]);
    let d = p.drain_b();
    let s = sizes(&d);

    assert!(
        !s.is_empty(),
        "room {before}: `b` put nothing on the wire with 4 KiB pending"
    );
    assert!(
        s.iter().sum::<u64>() <= before,
        "room {before}: §7.3 binds `b`'s output — {s:?}"
    );

    // One round trip. The clock advances past §12.4's delayed-ACK ceiling
    // twice so the test holds whether `a` acknowledges immediately (§12.4's
    // out-of-order rule) or on the delayed-ACK timer.
    let later = now + constants::MAX_ACK_DELAY + Duration::from_millis(1);
    let _ = p.flush_b_to_a(later);
    let settled = later + constants::MAX_ACK_DELAY + Duration::from_millis(1);
    let (_da, db) = p.pump(settled);

    assert!(
        p.b.amplification_budget().is_none(),
        "ruling 168: `b`'s post-roam output must be ack-eliciting, so that \
         `a`'s ACK covers `validation_floor` and the budget disarms within \
         one round trip. Still armed means the output was a bare ACK, or \
         nothing at all — ruling 203's stall, or ruling 207(b)'s"
    );

    // And the escape is real, not merely recorded: once the budget is gone
    // `b` puts more on the wire than the budget would ever have permitted.
    // Counted from `b`'s own transmits rather than from `a`'s reader, so
    // this does not also depend on §9's claim order.
    let total: u64 = s.iter().sum::<u64>() + sizes(&db).iter().sum::<u64>();
    assert!(
        total > before,
        "the address validated, so more than the {before}-byte budget must \
         have left `b`: {total} bytes did"
    );
}

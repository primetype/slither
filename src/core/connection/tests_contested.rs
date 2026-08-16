//! Slice 7's core-level coverage for the **pending contested probe**
//! (§7.5's mark/transmission separation, over §7.3's budget).
//!
//! Written from `SPEC.md` §7.3/§7.5 and `CONTRACT-7.md` alone, in an
//! isolated worktree by an author who never read the implementation
//! (CLAUDE.md working rule 6).
//!
//! # Why this file exists at all
//!
//! `CONTRACT-7.md` §8.3 called the mark/transmission separation *"the
//! single most testable property in the slice"*. **Two independent blind
//! integration authors proved it is not testable from an integration test
//! at all** — one by arithmetic (after a roam the budget admits the
//! 31-byte probe outright, and driving the spend up is self-defeating
//! because every small packet the peer sends raises the cap by 3× what a
//! reply costs), one by construction (the shell fixture cannot build a
//! connection that both roams and is contested). **Ruling 193** struck the
//! claim and moved the coverage here, to the core, where the budget's two
//! counters are settable by choosing packet sizes.
//!
//! # The state under test
//!
//! A contested mark is taken, but §7.3's budget will not yet admit the
//! probe. §7.5:
//!
//! > The deadline is armed at the probe's **transmission**, not at the
//! > mark, so a probe that §7.3's budget will not yet admit leaves the
//! > mark **pending** rather than failed — the endpoint sends it, and
//! > arms, at the first instant the budget allows.
//!
//! # Working rule 9 is the organising principle
//!
//! Every test carries a `Mutation caught:` line naming what a broken build
//! does and which assertion separates it. The trap this file exists to
//! avoid is named in the brief: **a test that merely checks "the probe
//! eventually goes out" passes a build with no pending state at all** —
//! one that sends the probe immediately, in violation of the budget. So
//! the pins are the negative ones: *nothing* leaves while the budget
//! refuses, and the notification and the deadline arrive at the
//! **transmission** instant, at an `Instant` chosen to differ from the
//! mark's.
//!
//! # The arithmetic this file is built on
//!
//! `DATA_HEADER_LEN` 14 + `AEAD_TAG_LEN` 16 = **30 bytes** of packet
//! overhead, so a lone PING is a **31-byte** datagram and the peer's
//! smallest possible packet (§3.4's empty plaintext — the keepalive form)
//! is **30 bytes**, worth 90 bytes of cap.
//!
//! A responder anchored from a msg1 source starts **unvalidated** with
//! `budget_recv = INIT_PACKET_LEN` = 196, so its cap is
//! `AMPLIFICATION_FACTOR * 196` = **588**. One datagram sized to exactly
//! the *remaining* room is admitted (the check is `<=`) and leaves nothing
//! queued behind it. After it `room == 0`, and the 31-byte probe cannot
//! leave. That is the pending state, in three calls.
//!
//! **[corrected by I1 — finding A2]** `budget_sent` does **not** start at
//! 0: the endpoint emitted `RESP_PACKET_LEN` = 107 bytes of msg2 to this
//! unvalidated anchor before the connection existed, and §7.3 caps *total
//! bytes sent*, so the connection charges them at its arming. The room the
//! shaping datagram fills is therefore 588 − 107 = **481**, and every
//! number below is read from `room()` rather than written down — which is
//! why this correction moves one assertion and no arithmetic.
//!
//! **[corrected by I1 — ruling 208]** …and the shaping packet carries nine
//! bytes of `PATH_CHALLENGE` ahead of the datagram, because §8.7 makes the
//! challenge a standing obligation on every packet built while the address
//! is unvalidated. `spend_to_the_cap` subtracts it.
//!
//! **[corrected at integration — ruling 201]** This paragraph originally
//! computed the shaping packet from ruling 155's *"mandatory"* `0x30`
//! extends-to-end form, at 1 type byte + payload. That is wrong for a
//! sub-maximum datagram, and the error came from `CONTRACT-7.md`, not from
//! this author: ruling 184 over-read ruling 155, whose arithmetic concerns
//! only the **maximum-size** case. A DATAGRAM frame carries an explicit
//! length varint (`0x31`) unless it genuinely runs to the end of the
//! packet — and it must, because ruling 155 packs datagrams **before** the
//! stream fill while an extends-to-end frame has to be **last**. So the
//! frame overhead here is 1 + the length varint (1 byte below 64, 2 up to
//! 16383, §8.1), and both sizes occur in this file.
//!
//! # The two floors — **[migrated by I1 for ruling 208]**
//!
//! This file was written when §7.3 and §7.5 each recorded a *counter
//! floor*: `validation_floor` at the budget's arming (so **0**), and
//! `probe_floor` (ruling 41) at the **mark**, after the spend-down packet
//! has used counter 0 (so **1**). §3.2 said in terms: *"two independent
//! floors recorded at different moments; do not conflate them, and do not
//! share one field."*
//!
//! **[ruling 208]** supersedes the first of the two. An ACK is an assertion
//! by whoever holds the key and §7.3's roaming threat model *is* the key
//! holder, so an ACK covering anything validates **no** address; what
//! disarms the budget is a `PATH_RESPONSE` echoing the arming's eight-byte
//! challenge. The `probe_floor` half is **untouched** — ruling 208 reaches
//! only §7.3's floor, and ruling 210 records deleting the whole mechanism
//! as the most likely way to break that change.
//!
//! So the "do not share one field" test does not disappear; it gets
//! sharper, because the two exits are now different *frames* rather than
//! two readings of one integer. It is
//! `a_response_that_validates_the_address_releases_the_probe_without_clearing_the_mark`
//! below, and it now also pins the inverse: the ACK that used to validate
//! must **not**.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here
//! reads a clock. Plain `#[test]`, no `sleep`. Every mutating call is
//! followed by draining `poll_output()` to the terminal `Timeout` (§16.4),
//! which is what [`testfix::drain`] does.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::time::{Duration, Instant};

use super::*;

use super::testfix::*;
use super::timers::TimerKind;

use crate::constants::{
    AEAD_TAG_LEN, AMPLIFICATION_FACTOR, DATA_HEADER_LEN, FRAME_ACK, FRAME_PATH_CHALLENGE,
    FRAME_PATH_RESPONSE, FRAME_PING, INIT_PACKET_LEN, KEEPALIVE_TIMEOUT, RESP_PACKET_LEN,
};
use crate::error::ConnectionLost;

// ═══════════════════════════════════════════════════════════════════════
// §7.3's arithmetic, named
// ═══════════════════════════════════════════════════════════════════════

/// §3.4's cleartext header plus the AEAD tag — what every Data packet
/// costs before a single frame byte.
const PKT_OVERHEAD: u64 = (DATA_HEADER_LEN + AEAD_TAG_LEN) as u64;

/// §7.5's probe on the wire: one `FRAME_PING` byte in a Data packet.
///
/// This is the number the two integration authors could not squeeze out of
/// a shell fixture, and the number every "the budget refuses" assertion in
/// this file is calibrated against.
const PROBE_PACKET_LEN: u64 = PKT_OVERHEAD + 1;

/// §3.4's empty plaintext — §7.5's keepalive form, and the **smallest
/// packet a peer can send us**. Worth `AMPLIFICATION_FACTOR *` this much
/// of send budget, which is why no single peer packet can ever leave the
/// budget refusing a 31-byte probe.
const KEEPALIVE_PACKET_LEN: u64 = PKT_OVERHEAD;

/// The cap a responder anchored from msg1 starts under (§3.2's second
/// arming event): `3 * 196`.
const RESPONDER_CAP: u64 = AMPLIFICATION_FACTOR * INIT_PACKET_LEN as u64;

/// A third address, for roaming onto. `testfix` names only `a_addr` (the
/// anchor `Solo` installs against) and `b_addr`.
fn c_addr() -> SocketAddr {
    v4(3, 3)
}

// ═══════════════════════════════════════════════════════════════════════
// The integration seam — read this before fixing a compile error here
// ═══════════════════════════════════════════════════════════════════════

/// Take §7.5's contested mark, as §6.4's LIVE branch does on a refusal
/// against a `None` basis.
///
/// # This is the one verb this file invents, and it is a contract gap
///
/// `CONTRACT-7.md` §3.5 defers the contested state wholly to §5 (*"see
/// §5"*), and §5.1 then specifies the **state machine** — the marking
/// table, the transmission's four atomic steps, the clearing table, the
/// verdict — **without ever naming the `Connection` method by which the
/// mark is taken**. §3's list of core API additions does not contain one
/// either, and §4.1's `accept()` table says only *"marked contested"* in
/// the effect column. So the seam between `endpoint::accept()` and
/// `core::Connection` is specified in behaviour and unnamed in API.
///
/// That is working rule 8's defect class exactly — a stated construction
/// with an unstated scope — and it is **reported, not resolved**. The name
/// below is this author's guess. Every test in this file goes through this
/// one function, so if the implementation calls it something else the fix
/// is one line, here.
///
/// Everything else this file touches is contract-named: `handle_datagram`,
/// `send_datagram`, `close`, `timer(TimerKind::Contested)`,
/// `amplification_budget()`, `path_generation()`, `remote_address()`,
/// `ConnEvent::{Contested, ContestCleared, AddressMoved}`.
fn mark(s: &mut Solo, now: Instant) -> Drained {
    s.conn.mark_contested(now);
    drain(&mut s.conn)
}

// ═══════════════════════════════════════════════════════════════════════
// Fixtures
// ═══════════════════════════════════════════════════════════════════════

/// A core installed as the **responder**, i.e. anchored from a msg1
/// source, which is §3.2's second arming event.
fn responder_at(now: Instant) -> Solo {
    Solo::installed_from_msg1_at(now)
}

/// A core installed as the **initiator**, i.e. against a `connect()`-
/// supplied address, which §3.2 says starts **validated**.
fn dialled_at(now: Instant) -> Solo {
    let (mut conn, sa, sb) = Solo::connecting();
    conn.handle_endpoint_event(
        now,
        Install {
            session: sa,
            role: Role::Initiator,
            anchor_from_msg1: false,
        },
    );
    let _ = drain(&mut conn);
    // `sa`'s anchor is `b_addr()`, so this core's peer must be reached
    // through `deliver_from(_, b_addr(), _)`, never plain `deliver`.
    Solo::around(conn, sb)
}

/// The budget's two counters, which must be armed.
fn budget(s: &Solo) -> (u64, u64) {
    s.conn
        .amplification_budget()
        .expect("§3.2: the address is unvalidated, so a budget is armed")
}

/// How many more bytes §7.3 will admit right now.
fn room(s: &Solo) -> u64 {
    let (sent, recv) = budget(s);
    (AMPLIFICATION_FACTOR * recv).saturating_sub(sent)
}

/// Spend the budget down to **exactly** its cap with a single unreliable
/// datagram, so that `room == 0` and nothing is left queued behind it.
///
/// Datagrams are the lever because ruling 155 makes the `0x30`
/// extends-to-end form mandatory, so the packet is `PKT_OVERHEAD + 1 +
/// payload` — a size this test file chooses to the byte. `send_datagram`
/// seals inside the call (§16.7), so one drain collects the packet.
fn spend_to_the_cap(s: &mut Solo, now: Instant) {
    // **Integrator, ruling 201.** Flush any owed ACK *before* measuring.
    // §8.5 packs an owed ACK **ahead of** the DATAGRAM fill in the same
    // packet, so a payload calibrated against a datagram-only packet
    // overshoots the cap by the ACK's size and **nothing leaves at all** —
    // which is how this helper failed at integration, in every test that
    // used it. Draining the ACK first spends its bytes explicitly, and the
    // shaping datagram that follows really is alone in its packet.
    //
    // This is not a fixture nicety: it is §8.5 and §7.3 interacting, and no
    // author could have seen it without an implementation to run against.
    if let Some(at) = s.conn.timer(TimerKind::AckDelay) {
        s.conn.handle_timeout(at);
        let _ = drain(&mut s.conn);
    }

    let (sent_before, recv_before) = budget(s);
    let space = room(s);
    assert!(
        space > PKT_OVERHEAD + 1,
        "the fixture needs room for one shaping datagram; had {space}"
    );

    // **Integrator, ruling 201.** `DATAGRAM_LEN` (`0x31`) — one type byte
    // plus a 2-byte length varint — **not** the `0x30` extends-to-end form
    // this file's header assumed. Ruling 184 (mine) told the contract that
    // `0x30` was mandatory for every datagram; that over-read ruling 155,
    // whose arithmetic is only about the *maximum-size* case. `0x30` means
    // "runs to the end of the packet", so it is available only when the
    // datagram really is last — and ruling 155 packs datagrams *before* the
    // stream fill, so mandating it universally would forbid any packet
    // carrying a datagram and stream data together. The core is right; the
    // contract this file was written against was not.
    // The length varint is 1 byte below 64 and 2 up to 16383 (§8.1), and
    // both sizes occur here: a responder at its 588-byte cap needs a
    // 555-byte payload, a post-roam core at 90 needs 57.
    // **[I1, ruling 208]** …and the nine bytes of `PATH_CHALLENGE` that
    // ride ahead of it. §8.5 as amended packs the path frames **first among
    // the control frames**, and §8.7 makes the challenge a **standing**
    // obligation: it is re-offered on every packet the pump builds while
    // the address is unvalidated, which is precisely the state this whole
    // fixture is in. A payload calibrated against a datagram-only packet
    // therefore overshoots by nine, the datagram does not fit, and what
    // leaves is a 39-byte challenge packet with the datagram still queued —
    // the same shape as the ACK correction above, one ruling later.
    let path = if s.conn.outstanding_challenge().is_some() {
        (1 + 8) as u64
    } else {
        0
    };
    let payload = {
        let one = space - path - PKT_OVERHEAD - 1 - 1;
        if one < 64 {
            one as usize
        } else {
            (space - path - PKT_OVERHEAD - 1 - 2) as usize
        }
    };
    assert!(
        payload < 16_384,
        "the varint arithmetic above covers payloads below 16384; got {payload}"
    );
    s.conn
        .send_datagram(now, &ramp(0, payload))
        .expect("§11: a sub-maximum datagram is accepted");
    let d = drain(&mut s.conn);

    assert_eq!(
        d.transmits().len(),
        1,
        "the shaping datagram is sized to fit the budget exactly, so it \
         must leave: §7.3's check is a strict `>`"
    );
    assert_eq!(
        d.transmits()[0].data.len() as u64,
        space,
        "the shaping packet must be exactly the remaining room, or every \
         later assertion in this file is calibrated against the wrong cap"
    );
    assert_eq!(
        budget(s),
        (sent_before + space, recv_before),
        "§3.2: sending credits `budget_sent` in datagram bytes and leaves \
         `budget_recv` alone"
    );
    assert_eq!(
        room(s),
        0,
        "the point of the fixture: the budget now admits nothing at all"
    );
    assert!(
        room(s) < PROBE_PACKET_LEN,
        "and in particular it does not admit §7.5's 31-byte probe"
    );
}

/// A responder sitting at its cap: the pending gap is one `mark()` away.
fn responder_at_the_cap(now: Instant) -> Solo {
    let mut s = responder_at(now);
    // **[I1, A2]** `sent` is the msg2 the endpoint already put on this
    // unvalidated anchor, not 0: §7.3 caps *total bytes sent*, and leaving
    // those 107 uncounted measured 3.55× against a normative MUST of 3.
    assert_eq!(
        budget(&s),
        (RESP_PACKET_LEN as u64, INIT_PACKET_LEN as u64),
        "§3.2's second arming event, with the msg2 it provoked charged to it"
    );
    spend_to_the_cap(&mut s, now);
    s
}

/// Hand the core the peer's smallest possible packet: §3.4's empty
/// plaintext, from the current anchor.
///
/// It is **not ack-eliciting** (it carries no frames at all — §1.3), so it
/// owes no reply that could contend with the probe; it covers no counter,
/// so it clears no mark and validates no address; and it credits
/// `budget_recv` by 30, lifting the cap by 90 — comfortably more than the
/// probe's 31. That makes it the clean "the budget now allows" lever.
fn release_the_budget(s: &mut Solo, now: Instant) -> Drained {
    s.deliver(now, &[])
}

/// §8.4's ACK, encoded by hand. `testfix` has a decoder but no encoder,
/// and `tests_ack.rs`'s copy is private to a file this author does not own.
fn ack_frame(largest: u64, ack_delay: u64, first_range: u64, pairs: &[(u64, u64)]) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_ACK);
    put(&mut f, largest);
    put(&mut f, ack_delay);
    put(&mut f, pairs.len() as u64);
    put(&mut f, first_range);
    for (gap, range) in pairs {
        put(&mut f, *gap);
        put(&mut f, *range);
    }
    f
}

/// §8.4's `PATH_RESPONSE`, encoded by hand like [`ack_frame`] — the type
/// byte and eight opaque bytes, no length prefix. **[ruling 208]**
fn path_response_frame(value: [u8; 8]) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_PATH_RESPONSE);
    f.extend_from_slice(&value);
    f
}

fn has_ping(frames: &[Wire]) -> bool {
    frames.iter().any(|f| matches!(f, Wire::Ping))
}

fn has_datagram(frames: &[Wire]) -> bool {
    frames.iter().any(|f| matches!(f, Wire::Datagram { .. }))
}

fn contested_events(d: &Drained) -> usize {
    d.count_events(|e| matches!(e, ConnEvent::Contested))
}

fn cleared_events(d: &Drained) -> usize {
    d.count_events(|e| matches!(e, ConnEvent::ContestCleared))
}

/// Everything §5.1 says the mark must **not** do.
fn assert_nothing_happened(s: &mut Solo, d: &Drained, why: &str) {
    assert!(
        d.transmits().is_empty(),
        "{why}: §5.1 marking — NO PING. A probe left anyway"
    );
    assert_eq!(contested_events(d), 0, "{why}: §5.1 marking — NO EVENT");
    assert_eq!(
        cleared_events(d),
        0,
        "{why}: `ContestCleared` is emitted only where `Contested` was \
         (ruling 176)"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        None,
        "{why}: §5.1 marking — NO TIMER. §7.5 arms at the transmission, \
         not at the mark"
    );
    assert!(
        d.closed().is_none(),
        "{why}: a mark the budget refuses is pending, not failed"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 1. §3.2's arming — the premise every pending test rests on
// ═══════════════════════════════════════════════════════════════════════

/// §3.2's second arming event: a connection created by `accept()` is
/// anchored on the msg1 source and starts **unvalidated**, with the msg1
/// itself credited.
///
/// Mutation caught: a build that never arms the budget returns `None` here
/// and the whole pending state becomes unreachable. A build that arms with
/// **no** credit returns `Some((0, 0))` — a cap of zero, which by §3.2's
/// own check can never send a byte and deadlocks the connection until the
/// 25 s liveness reap. Both are separated from the correct `Some((0,
/// 196))`, and neither is separated by a test that only asserts "a budget
/// exists".
#[test]
fn a_responder_anchored_from_msg1_starts_unvalidated_with_the_msg1_credited() {
    let t = t0();
    let s = responder_at(t);

    assert_eq!(
        s.conn.amplification_budget(),
        Some((RESP_PACKET_LEN as u64, INIT_PACKET_LEN as u64)),
        "§3.2: armed at the msg1 anchor, the msg1 credited — *\"its \
         handshake tail tags having verified at admission\"* — and **[A2]** \
         the msg2 the endpoint already sent to it charged against it"
    );
    assert_eq!(
        room(&s),
        RESPONDER_CAP - RESP_PACKET_LEN as u64,
        "the cap is AMPLIFICATION_FACTOR * INIT_PACKET_LEN = 588, of which \
         the msg2's 107 bytes are already spent"
    );
}

/// §3.2: *"Not armed for a `connect()`-supplied address: a dialled
/// connection starts **validated**."* The contract calls this *"the single
/// most load-bearing fact for the contested-probe tests"*.
///
/// Mutation caught: a build that arms the budget for **every** connection,
/// which is the obvious way to implement §7.3 and is exactly what ruling
/// 168 reversed. Such a build caps a dialler at 3× what it receives from
/// the very first byte, and `amplification_budget()` returns `Some(_)`
/// here instead of `None`.
#[test]
fn a_dialled_connection_starts_validated_with_no_budget_armed() {
    let t = t0();
    let s = dialled_at(t);

    assert_eq!(
        s.conn.amplification_budget(),
        None,
        "§3.2: a `connect()`-supplied address is not armed"
    );
}

/// §3.1 step 5 and ruling 173: a committed roam **resets both counters**
/// and then credits **only the triggering packet**.
///
/// Mutation caught: ruling 173's named defect — *"an implementer building
/// `on_roam()` from §13.6 lets the budget carry the old address's credit
/// to the new one … the reflector §7.3 exists to prevent, reconstructed
/// out of a missing line."* That build reports `(0, 226)` (196 carried
/// forward plus the roaming packet) or leaves `budget_sent` where it was.
/// Working rule 9: the degenerate "the roam does not touch the budget"
/// build reports `(0, 196)`, which this asserts against from the *equality*
/// side, not from a "the budget is small" bound it would satisfy for free.
#[test]
fn a_committed_roam_rearms_the_budget_and_credits_only_the_roaming_packet() {
    let t = t0();
    let mut s = responder_at(t);
    let t1 = t + Duration::from_millis(500);

    // Spend first, so a build that resets only `budget_recv` is separated
    // from one that resets both.
    spend_to_the_cap(&mut s, t);
    assert_eq!(budget(&s), (RESPONDER_CAP, INIT_PACKET_LEN as u64));

    let d = s.deliver_from(t1, c_addr(), &[]);

    assert_eq!(
        s.conn.remote_address(),
        Some(c_addr()),
        "§7.3: the anchor moved"
    );
    assert_eq!(s.conn.path_generation(), 1, "§14.6: one committed roam");
    assert_eq!(
        d.count_events(|e| matches!(
            e,
            ConnEvent::AddressMoved { from, to } if *from == a_addr() && *to == c_addr()
        )),
        1,
        "§16.4: exactly one `AddressMoved`, with §5.2's exact field names"
    );
    assert_eq!(
        budget(&s),
        (0, KEEPALIVE_PACKET_LEN),
        "§3.1 step 5: *\"both counters reset, then the triggering packet's \
         datagram length credits the received counter\"* — 30 bytes, not \
         226, and `budget_sent` back to 0"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. The mark instant — the pin ruling 193 moved here
// ═══════════════════════════════════════════════════════════════════════

/// **The core pin.** A mark taken while §7.3's budget refuses the probe
/// emits nothing, sends nothing, and arms no deadline.
///
/// Mutation caught: **a build with no pending state at all** — one that
/// sends the PING, queues `ConnEvent::Contested` and arms
/// `TimerKind::Contested` at the mark, in violation of the budget. That is
/// the build the brief names, and it is the natural one to write: §5.1's
/// marking and transmission tables read as one paragraph unless the budget
/// is consulted between them. Note the assertions are the **negative**
/// ones. A test asserting "the probe eventually goes out" passes that
/// build outright.
///
/// Also caught: a build that treats a budget-refused probe as a *failure*
/// (dropping the mark, or closing) — `closed()` and the follow-on test
/// separate it from `Pending`.
#[test]
fn nothing_leaves_and_nothing_is_emitted_at_a_mark_the_budget_will_not_admit() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);

    assert!(
        room(&s) < PROBE_PACKET_LEN,
        "premise: §7.3 will not admit a 31-byte probe"
    );
    let budget_before = budget(&s);

    let d = mark(&mut s, t1);

    assert_nothing_happened(&mut s, &d, "at a mark the budget refuses");
    assert_eq!(
        budget(&s),
        budget_before,
        "§3.2: a HELD datagram is *\"not dropped, not truncated, not an \
         error\"* — and it is not sent either, so it spends nothing"
    );
}

/// §5.1's `Pending` row: a second mark while pending is a **total no-op**,
/// and the eventual transmission is still **one** probe.
///
/// Mutation caught: a build that queues a pending probe per mark rather
/// than collapsing marks (ruling 41: *"Collapse concurrent marks into a
/// **single** contested state, one floor, one deadline"*). Such a build
/// emits two `Contested` events and two PINGs when the budget opens. The
/// second half of this test is what separates it — asserting only that the
/// second mark's own drain is empty would pass it, because that build's
/// second probe is *also* budget-blocked at the time.
#[test]
fn a_second_mark_while_pending_is_a_total_no_op() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t + Duration::from_secs(2);
    let t3 = t + Duration::from_secs(3);

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "first mark");

    let d2 = mark(&mut s, t2);
    assert_nothing_happened(&mut s, &d2, "second mark while pending");

    let d3 = release_the_budget(&mut s, t3);
    let frames = s.drain_frames(&d3);

    assert_eq!(
        frames.iter().filter(|f| matches!(f, Wire::Ping)).count(),
        1,
        "ruling 41: two marks collapse into one probe"
    );
    assert_eq!(
        contested_events(&d3),
        1,
        "ruling 41: one contested state, so one `Contested`"
    );
}

/// §5.1 row 2 and ruling 179: marking a **closing** connection is a total
/// no-op — the carve-out is enforced where the mark is taken.
///
/// Mutation caught: a build that takes the mark regardless of lifecycle,
/// which §7.5 then has to undo. Ruling 179's own words: *"a rule enforced
/// only in the section that describes the state rather than the section
/// that enters it is a rule that gets missed."* Such a build parks a
/// pending probe on a connection that is *"already leaving, and the parked
/// `Intro` will meet no live static"*, and fires it the moment the budget
/// opens — which the release half of this test catches even though the
/// mark's own drain looks identical either way.
#[test]
fn a_mark_on_a_closing_connection_is_a_total_no_op() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t + Duration::from_secs(2);

    s.conn.close(t1, 0, b"");
    let _ = drain(&mut s.conn);

    let d1 = mark(&mut s, t1);
    assert_eq!(contested_events(&d1), 0, "ruling 179: NOT marked");
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        None,
        "ruling 179: no deadline on a closing connection"
    );

    let d2 = release_the_budget(&mut s, t2);
    let frames = s.drain_frames(&d2);
    assert!(
        !has_ping(&frames),
        "ruling 179: no probe is owed, so none may leave when the budget \
         opens"
    );
    assert_eq!(contested_events(&d2), 0, "ruling 179: and no notification");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. The transmission instant — mark ≠ transmission
// ═══════════════════════════════════════════════════════════════════════

/// §8.3's table, and §15.4 L4093's *"which is also when the deadline arms
/// and when `Contested` is emitted"*: the probe, the notification and the
/// deadline are **one atomic step at the transmission instant**.
///
/// Mutation caught: a build that arms `TimerKind::Contested` at the
/// **mark** rather than at the transmission. It is separated by the
/// deadline's value, which is why `t1` and `t2` are three seconds apart:
/// such a build reports `t1 + 10 s`, and a test that marked and
/// transmitted at the same `Instant` — which is every test on a validated
/// address, and every shell-level test — could not tell the two apart.
/// That indistinguishability is precisely what ruling 193 recorded.
///
/// Also caught: a build that emits `Contested` at the mark (0 here, 1 in
/// the mark test above); and a build that drops the pending probe instead
/// of holding it (no PING ever).
#[test]
fn the_probe_the_notification_and_the_deadline_all_land_at_the_transmission_instant() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);
    assert_ne!(t1, t2, "the whole point of the test");

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    let d2 = release_the_budget(&mut s, t2);

    // **[I1, ruling 212(c)]** Two packets, and their **order** is the
    // ruling: *"`PATH_CHALLENGE` and `PATH_RESPONSE` rank immediately after
    // CLOSE, above the contested probe … everything else in the order
    // competes for the budget; the challenge dissolves it."* The address is
    // unvalidated, so the released room buys the challenge first (39 B) and
    // the probe second (31 B) — inside the 90 B a single keepalive credits.
    // This test previously read *"nothing else is owed, so the probe rides
    // alone"*, which was true before the challenge existed.
    assert_eq!(
        d2.transmits().len(),
        2,
        "the challenge and the probe, in that order"
    );
    // Read as **raw plaintext** rather than through `testfix`'s decoder:
    // nine fixed self-delimiting bytes need no decoder, and this assertion
    // then holds whatever that decoder does or does not yet know about
    // §8.3's two new rows.
    let first = s.peer.open_dgram(&d2.transmits()[0].data);
    assert_eq!(
        first.first().copied(),
        Some(FRAME_PATH_CHALLENGE as u8),
        "§7.3's rank above the probe: the challenge goes first — it is the \
         only output that **ends** the scarcity every other rank is \
         competing inside"
    );
    assert_eq!(first.len(), 9, "and it rides alone: 1 type byte + 8 opaque");
    let second = s.peer.open_dgram(&d2.transmits()[1].data);
    assert_eq!(
        second.as_slice(),
        [FRAME_PING as u8],
        "§5.1 transmission step 1: an ack-eliciting PING, *\"at the first \
         instant the budget allows\"*, and it follows the challenge"
    );
    for t in d2.transmits() {
        assert_eq!(t.to, a_addr(), "§5.6: aimed at the anchor");
    }
    assert_eq!(
        contested_events(&d2),
        1,
        "§5.1 transmission step 3: `Contested` is queued **here**, not at \
         the mark (rulings 45/46, FAB-6)"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "§5.1 transmission step 2: `armed_at` is the transmission instant, \
         so the deadline is t2 + 10 s — **not** t1 + 10 s"
    );
    assert_ne!(
        s.conn.timer(TimerKind::Contested),
        Some(t1 + KEEPALIVE_TIMEOUT),
        "stated from the other side: a build arming at the mark fails here"
    );

    // §16.4's generation order is normative, and §12.2 lists the probe
    // ahead of the event.
    let ping_at = d2
        .position(|o| matches!(o, ConnOutput::Transmit(_)))
        .expect("the probe");
    let event_at = d2
        .position(|o| matches!(o, ConnOutput::Event(ConnEvent::Contested)))
        .expect("the notification");
    assert!(
        ping_at < event_at,
        "§12.2: `Transmit(PING)` … then `Event(Contested)` — *\"in that \
         order\"*"
    );
}

/// §3.2's *"do not conflate them, and do not share one field"*, made
/// observable — **migrated for ruling 208, and stronger for it**.
///
/// The address is validated by a `PATH_RESPONSE` echoing the arming's
/// challenge, which releases the probe; the **same delivery** carries no
/// ACK at all, so `probe_floor` = 1 is not covered, the mark survives, and
/// the probe goes out.
///
/// Before it, the assertion this test used to make is **inverted**: the ACK
/// covering counter 0 — at or above the old `validation_floor` of 0, which
/// is exactly what used to validate — must now leave the budget armed.
/// **[ruling 208]** *"`largest` is not a proof of receipt; it is an
/// assertion by whoever holds the key"*, and a build that kept the old
/// predicate beside the new one has left the bypass unlocked, which is the
/// thing that ruling names as the reason for removing it rather than
/// leaving it inert.
///
/// Mutation caught: a build that keeps **one** floor, or one exit. If the
/// response also cleared the mark, `cleared_events` fires and no probe is
/// sent. If the ACK still validated, the budget is gone before the response
/// arrives and the first assertion fails. If neither exit exists the probe
/// never leaves and `has_ping` fails. No single-mechanism build passes.
#[test]
fn a_response_that_validates_the_address_releases_the_probe_without_clearing_the_mark() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);

    let highest_sealed = s.conn.next_counter().expect("installed") - 1;
    assert_eq!(
        highest_sealed, 0,
        "premise: the shaping datagram is the only packet sealed so far"
    );
    let challenge = s
        .conn
        .outstanding_challenge()
        .expect("an armed budget owes a challenge");

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    // The superseded proof: an ACK covering counter 0 — at or above the old
    // `validation_floor`, which is precisely what used to validate.
    let d2 = s.deliver(t2, &ack_frame(0, 0, 0, &[]));
    assert!(
        s.conn.amplification_budget().is_some(),
        "**[ruling 208]** an ACK validates no address: `largest` is an \
         assertion by whoever holds the key, and §7.3's roaming threat \
         model *is* the key holder. A build that kept the old predicate \
         beside the new one leaves the bypass unlocked"
    );
    assert_eq!(
        cleared_events(&d2),
        0,
        "ruling 41: `probe_floor` is 1; an ACK covering only 0 clears \
         nothing"
    );

    // The probe leaves on **this** delivery, and the reason is worth being
    // explicit about because it is not the ACK: any peer packet is at least
    // 30 bytes and credits 3× that, so the budget now admits the 31-byte
    // probe. The pre-208 form of this test read the release as the ACK's
    // *validation*, which the credit would have produced anyway — an
    // assertion that did not separate what it named (working rule 9).
    let frames = s.drain_frames(&d2);
    assert!(
        has_ping(&frames),
        "the mark survived and the credited budget admits the probe, so \
         §7.5's PING leaves at this instant"
    );
    assert_eq!(contested_events(&d2), 1, "and `Contested` fires with it");
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "armed at the transmission, as always"
    );

    // The real proof, on a later delivery: the address validates and the
    // **armed** mark is still untouched — the second half of *"do not share
    // one field"*, now that the two exits are two different frames.
    let t3 = t2 + Duration::from_secs(1);
    let d3 = s.deliver(t3, &path_response_frame(challenge));
    assert_eq!(
        s.conn.amplification_budget(),
        None,
        "**[ruling 208]** an authenticated, window-fresh packet from the \
         anchor carrying a `PATH_RESPONSE` that echoes the arming's \
         challenge is the return-routability proof — the address is \
         validated and the budget disarms"
    );
    assert_eq!(
        cleared_events(&d3),
        0,
        "a `PATH_RESPONSE` covers no counter at all, so §7.5's probe floor \
         is untouched and the verdict is still outstanding"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "…and the deadline still stands at the transmission instant"
    );
}

/// Ruling 171(a): *"A pending contested probe takes priority over all
/// other output to an unvalidated address"* — ahead of ACKs, keepalives,
/// PTO probes, retransmissions and new Data.
///
/// The arithmetic is chosen so that the released room admits **exactly
/// one** of the two contenders, and admits neither of them *together*:
/// with `room` = 90, the probe alone is 31, the queued datagram alone is
/// 90, and ruling 181's PING-then-datagram packing of both is 91.
///
/// Mutation caught: a build with **no priority rule** — one that drains
/// its output queue in order and lets the older, larger datagram go first.
/// That build spends the room on the datagram (90) and then holds the
/// probe, so `has_ping` is false and `has_datagram` is true: this test
/// separates the two builds in *both* directions, which a "the probe
/// eventually leaves" assertion would not, since the datagram-first build
/// does send the probe — one peer packet later, and, as ruling 171 says,
/// possibly *"past its own deadline"*.
#[test]
fn a_pending_probe_outranks_a_queued_datagram_when_the_budget_admits_only_one() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t + Duration::from_secs(2);

    // A datagram that cannot leave now (room is 0) and is sized so that,
    // once the budget opens, it fits alone but not beside the probe.
    let released_room = AMPLIFICATION_FACTOR * KEEPALIVE_PACKET_LEN;
    let payload = (released_room - PKT_OVERHEAD - 1) as usize;
    s.conn
        .send_datagram(t1, &ramp(7, payload))
        .expect("§11: accepted into the send queue");
    let held = drain(&mut s.conn);
    assert!(
        held.transmits().is_empty(),
        "§3.2: the budget HELD it — *\"not dropped, not truncated, not an \
         error\"*"
    );

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark, with a datagram queued");

    let d2 = release_the_budget(&mut s, t2);
    assert_eq!(
        room(&s)
            + d2.transmits()
                .iter()
                .map(|x| x.data.len() as u64)
                .sum::<u64>(),
        released_room,
        "premise: exactly 90 bytes were released and this drain spent from \
         them"
    );

    let frames = s.drain_frames(&d2);
    assert!(
        has_ping(&frames),
        "ruling 171(a): the pending probe goes first, ahead of new Data"
    );
    assert!(
        !has_datagram(&frames),
        "…and the datagram waits, because 91 bytes of PING+DATAGRAM do not \
         fit in 90 (ruling 181's packing is the reason this is a real \
         choice and not an artefact)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Ruling 176's two exits from the pending gap
// ═══════════════════════════════════════════════════════════════════════

/// **Ruling 176's exit A, and the conflict it runs into.** ⚠ REPORTED, NOT
/// RESOLVED — see this file's report and the doc-comment below.
///
/// §5.1's clearing table says a pending mark clears on *"any ACK covering
/// any counter >= floor"*, cancelling the probe and emitting nothing. But
/// **the floor is always a counter that has not been sealed**: it is
/// `session.next_counter()` at the mark, and the mark is only *pending*
/// when the budget refuses, which (see this file's header arithmetic) can
/// only happen once `budget_sent` has been driven up — i.e. once every
/// counter below `next_counter()` is already on the wire. While pending,
/// nothing further can be sealed: the budget binds all output, and ruling
/// 171 puts the probe ahead of anything that might otherwise go first.
///
/// So the only ACK that can *name* the floor is one for an unsent packet,
/// and **§12.5 ignores such a frame whole** (`tests_ack.rs`'s
/// `an_ack_above_the_highest_sealed_counter_is_ignored_whole`). This test
/// asserts the reading in which §12.5 wins — the clearing predicate is
/// defined over a *processed* ACK — and it is the assertion that names the
/// question either way.
///
/// Mutation caught: a build that evaluates ruling 176's clearing predicate
/// on the **raw** ACK, before §12.5's ignore-whole filter. That build
/// cancels the pending probe on an unacknowledgeable counter, so an
/// off-path forgery of a single ACK frame silently suppresses §7.5's
/// probe — the mark is dropped, nothing is emitted, and the contested
/// connection is never tested. It fails `has_ping` here.
#[test]
fn an_ack_above_the_highest_sealed_counter_does_not_clear_a_pending_mark() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);

    let floor = s.conn.next_counter().expect("installed");
    let highest_sealed = floor - 1;
    assert_eq!(
        highest_sealed, 0,
        "premise: the floor recorded at the mark is one above the highest \
         counter ever sealed — which is the whole difficulty"
    );

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    let d2 = s.deliver(t2, &ack_frame(floor, 0, floor, &[]));

    assert_eq!(
        cleared_events(&d2),
        0,
        "§12.5: an ACK above the highest sealed counter is ignored whole, \
         so it covers nothing and clears nothing"
    );
    let frames = s.drain_frames(&d2);
    assert!(
        has_ping(&frames),
        "the mark is intact, and the packet's own 35 bytes lifted the cap \
         by 105 — so the probe leaves here"
    );
    assert_eq!(contested_events(&d2), 1, "at the transmission, once");
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
    );
}

/// **Ruling 176's exit B:** *"a roam while pending leaves the mark intact
/// with its floor unchanged"* — the counter space is never reset (§7.7),
/// so it is listed among §13.6's roam resets **as a thing that is not
/// reset**.
///
/// Mutation caught: a build that clears the contested state as part of
/// `on_roam()`. This is the single most likely slice-7 defect of its kind,
/// because ruling 173 exists precisely because §13.6's reset list was
/// incomplete, and the obvious repair — reset everything per-connection on
/// the roam seam — takes the mark with it. Such a build emits no
/// `Contested` and sends no PING at the roam, which both assertions below
/// separate. Note the roam *also* re-arms the budget with `budget_sent =
/// 0`, so the probe becomes admissible at that same instant: a build that
/// keeps the mark is obliged to send here, and a build that dropped it has
/// nothing to send. That is what makes the roam a *usable* exit rather
/// than a silent one.
#[test]
fn a_roam_while_pending_leaves_the_mark_intact_and_the_probe_still_goes_out() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    let d2 = s.deliver_from(t2, c_addr(), &[]);

    assert_eq!(
        s.conn.remote_address(),
        Some(c_addr()),
        "premise: the roam committed"
    );
    assert_eq!(s.conn.path_generation(), 1, "premise: one committed roam");
    assert_eq!(
        cleared_events(&d2),
        0,
        "ruling 176: a roam is not a clear — and `ContestCleared` is \
         emitted only where `Contested` was"
    );

    let frames = s.drain_frames(&d2);
    assert!(
        has_ping(&frames),
        "ruling 176: the mark survived the roam, and the re-armed budget \
         (sent = 0, cap = 90) admits the 31-byte probe"
    );
    assert_eq!(
        contested_events(&d2),
        1,
        "one mark, one probe, one `Contested` — at the transmission"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "armed at this transmission, not at the mark and not at the roam \
         being a separate event"
    );
    assert_eq!(
        d2.transmits()[0].to,
        c_addr(),
        "§7.3: and it is aimed at the new anchor"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. The `Armed` state's own exits — the contrast that makes §8.3 a table
// ═══════════════════════════════════════════════════════════════════════

/// §5.1's `Armed` clearing row: an ACK covering the floor sets the state
/// to `No`, disarms `TimerKind::Contested` and queues **`ContestCleared`**.
/// Contrast the `Pending` row, which emits nothing — that contrast is the
/// whole of ruling 176.
///
/// Mutation caught: a build that clears the state but never disarms the
/// timer. It passes every event assertion and then kills a healthy
/// connection with `TimedOut` ten seconds later — a failure that surfaces
/// far from its cause. The `timer(...) == None` assertion is what
/// separates it, and the verdict test below is what it would otherwise
/// have broken.
#[test]
fn an_ack_covering_the_floor_while_armed_clears_the_mark_and_disarms_the_deadline() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);
    let t3 = t2 + Duration::from_secs(1);

    let floor = s.conn.next_counter().expect("installed");
    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    let d2 = release_the_budget(&mut s, t2);
    assert!(
        has_ping(&s.drain_frames(&d2)),
        "premise: the probe went out"
    );
    assert_eq!(contested_events(&d2), 1, "premise: armed");

    // **[I1, ruling 212(c)]** The probe is no longer the *first* packet
    // sealed after the mark: §7.3 ranks `PATH_CHALLENGE` above it, so the
    // challenge takes the floor counter and the probe takes the next one.
    // The ACK is therefore built from the highest sealed counter rather
    // than from `floor` — it still covers `probe_floor`, which is what
    // §7.5 asks of it, and covering *more* than the floor is exactly what
    // ruling 41 makes a high-water mark for.
    let highest = s.conn.next_counter().expect("installed") - 1;
    assert!(
        highest >= floor,
        "premise: the probe was sealed at or above the mark's floor \
         ({highest} against {floor})"
    );
    let d3 = s.deliver(t3, &ack_frame(highest, 0, highest, &[]));

    assert_eq!(
        cleared_events(&d3),
        1,
        "§5.1 `Armed`: `ContestCleared` is queued"
    );
    assert_eq!(
        contested_events(&d3),
        0,
        "and no second `Contested` — ruling 46 removed the bool precisely \
         so the two could not be read as one toggle"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        None,
        "§5.1 `Armed`: the verdict deadline is disarmed"
    );
    assert!(d3.closed().is_none(), "§7.5: the connection lives");
}

/// §5.1's `Armed` marking row: a second mark while armed is a total
/// no-op and the deadline is **NOT re-armed**. §7.5 L2331–2337 calls this
/// *"a **security property**, not an optimisation"*.
///
/// Mutation caught: a build that re-arms on every mark. Because ruling 175
/// removed the cooldown (*"no cooldown is added"*) and the refusal rate is
/// *"the application's own `accept()` rate"*, such a build lets an attacker
/// who can park one admitted Intro every few seconds push the verdict
/// deadline forward for ever — a contested connection that is never
/// resolved. It is separated by the deadline's value alone, which is why
/// `t3` is a full second after `t2`; asserting only "no second PING" would
/// pass it.
#[test]
fn a_second_mark_while_armed_does_not_move_the_deadline() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);
    let t3 = t2 + Duration::from_secs(1);

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");
    let d2 = release_the_budget(&mut s, t2);
    assert!(has_ping(&s.drain_frames(&d2)), "premise: armed");

    let d3 = mark(&mut s, t3);

    assert!(
        !has_ping(&s.drain_frames(&d3)),
        "§5.1 `Armed`: no second PING"
    );
    assert_eq!(contested_events(&d3), 0, "§5.1 `Armed`: no event");
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "§5.1 `Armed`: the deadline is NOT re-armed — still the *first* \
         transmission's, not t3's"
    );
}

/// §5.1's verdict, and §8.3's last row: at the deadline with no covering
/// ACK the connection closes with `TimedOut`, `Retired` follows in the
/// same drain, **nothing is transmitted**, and there is **no third
/// notification**.
///
/// Mutation caught: a build that sends one last probe at the verdict
/// (`transmits().is_empty()`); a build that emits a third notification to
/// mark the death (`Contested`/`ContestCleared` counts); a build that fires
/// the verdict early (the `deadline - 1 ms` half — slice 1's one-sided
/// boundary lesson: testing only the firing side leaves a build with a
/// too-short deadline green).
#[test]
fn the_verdict_closes_the_connection_transmits_nothing_and_emits_no_third_notification() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");
    let d2 = release_the_budget(&mut s, t2);
    assert!(has_ping(&s.drain_frames(&d2)), "premise: armed");

    let deadline = t2 + KEEPALIVE_TIMEOUT;
    assert_eq!(s.conn.timer(TimerKind::Contested), Some(deadline));

    // Decouple §7.5's passive keepalive from the verdict instant, so that
    // a legitimate keepalive at the same `Instant` cannot be mistaken for
    // the verdict transmitting. Not ack-eliciting, covers no counter.
    let _ = s.deliver(t2 + Duration::from_secs(1), &[]);
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(deadline),
        "an ordinary receive does not move the verdict deadline"
    );

    tick(&mut s.conn, deadline - Duration::from_millis(1));
    let early = drain(&mut s.conn);
    assert!(
        early.closed().is_none(),
        "one millisecond before the deadline the verdict has not been \
         reached"
    );

    tick(&mut s.conn, deadline);
    let d = drain(&mut s.conn);

    assert_eq!(
        d.closed(),
        Some(ConnectionLost::TimedOut),
        "§15.4: the same variant as liveness — **no new one**"
    );
    assert_eq!(
        contested_events(&d) + cleared_events(&d),
        0,
        "§7.5: *\"No third notification\"* — a mark that is never answered \
         emits `Contested` at the transmission and then nothing; the death \
         arrives on `closed()`. Deliberately **not** asserted as \"no other \
         event of any kind\": §7.5 claims only that this pair is silent, and \
         a teardown may legitimately wake other machinery"
    );
    assert!(
        d.outs
            .iter()
            .any(|o| matches!(o, ConnOutput::ToEndpoint(ToEndpoint::Retired { .. }))),
        "§5.1 verdict step 2: `ToEndpoint::Retired` in the same drain"
    );
    assert!(
        d.transmits().is_empty(),
        "§5.1 verdict step 3: **nothing is transmitted**"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. The same gap by §8.3's route, not §3.2's
// ═══════════════════════════════════════════════════════════════════════

/// The pending gap reached by **roaming** rather than by a msg1 anchor.
///
/// ⚠ These two routes exist because the contract states two different
/// scopes and they disagree — REPORTED, NOT RESOLVED. §3.2 lists **two**
/// arming events (a committed roam, *and* an accepted initiation's msg1
/// anchor). §8.3 then says *"The pending gap is reachable only on a
/// connection that has roamed (§3.2)"*, citing the very section that names
/// the second route. Every other test here takes the msg1 route the brief
/// specifies; this one takes §8.3's, so that the coverage survives
/// whichever scope is ratified.
///
/// Mutation caught: the same no-pending-state build as the core pin, but
/// on the roam path — where the budget is armed by §3.1 step 5 rather than
/// at install, and where a build that arms the budget only at install
/// would leave the address permanently unvalidated after a move.
#[test]
fn the_pending_gap_is_reachable_by_roaming_too() {
    let t = t0();
    let mut s = responder_at(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t + Duration::from_secs(2);
    let t3 = t + Duration::from_secs(3);

    // Roam onto a fresh address: the budget re-arms at (0, 30), cap 90.
    let roamed = s.deliver_from(t1, c_addr(), &[]);
    assert_eq!(
        roamed.count_events(|e| matches!(e, ConnEvent::AddressMoved { .. })),
        1,
        "premise: the roam committed"
    );
    assert_eq!(budget(&s), (0, KEEPALIVE_PACKET_LEN));

    spend_to_the_cap(&mut s, t1);
    assert!(room(&s) < PROBE_PACKET_LEN, "premise: the budget refuses");

    let d1 = mark(&mut s, t2);
    assert_nothing_happened(&mut s, &d1, "a mark after a roam");

    let d2 = s.deliver_from(t3, c_addr(), &[]);
    assert!(
        has_ping(&s.drain_frames(&d2)),
        "§7.5: *\"the endpoint sends it, and arms, at the first instant the \
         budget allows\"*"
    );
    assert_eq!(contested_events(&d2), 1);
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t3 + KEEPALIVE_TIMEOUT),
        "the transmission instant, three seconds after the mark's"
    );
}

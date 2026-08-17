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
//! **[R40-B, ruling 250 — 2026/08/17]** The nine bytes of
//! `PATH_CHALLENGE` that ride ahead of the shaping datagram two paragraphs
//! up are §8.7's *standing obligation* half of the picture. From this
//! ruling, §7.5's **probe** carries them too, rather than trailing a
//! dedicated packet. §7.3 as amended:
//!
//! > The pump prefers **one packet**: the probe coalesces the owed path
//! > frames when the remaining room at pump time admits the coalesced
//! > size — 40 B with one 9 B path frame owed, 49 B with both — and emits
//! > the bare 31 B PING otherwise, the path frames following at their rank
//! > when room next admits them; when room admits neither, nothing is
//! > emitted and no `Pto` deadline is announced (§13.3, ruling 249). One
//! > packet, one counter, one sent-map entry.
//!
//! So this file now carries **three** probe sizes — [`PROBE_PACKET_LEN`]
//! 31, [`COALESCED_PROBE_LEN`] 40, [`COALESCED_PROBE_BOTH_LEN`] 49 — and
//! which one leaves is a statement about the **remaining** room at pump
//! time, never about the arming floor. That distinction is ruling 250's
//! (iii), and it is why [`spend_leaving`] exists: the 31/40 gap is a room
//! band the pre-250 spec's *"the budget holds both and always does"*
//! universal said could not exist.
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
    AEAD_TAG_LEN, AMPLIFICATION_FACTOR, DATA_HEADER_LEN, FRAME_ACK, FRAME_PADDING,
    FRAME_PATH_CHALLENGE, FRAME_PATH_RESPONSE, FRAME_PING, INIT_PACKET_LEN, KEEPALIVE_TIMEOUT,
    MAX_PLAINTEXT, RESP_PACKET_LEN,
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

/// **[ruling 250]** §7.5's probe with **one** owed 9-byte path frame
/// coalesced into it: `14 + 9 + 1 + 16`. §7.3's own arithmetic has priced
/// this packet since ruling 215 — *"one header, one tag"* — while the
/// shipped shape paid both twice.
const COALESCED_PROBE_LEN: u64 = PKT_OVERHEAD + 9 + 1;

/// **[ruling 250]** The same with **both** path frames owed, which §7.3
/// says *"a roam constructs"*: `14 + 9 + 9 + 1 + 16`.
const COALESCED_PROBE_BOTH_LEN: u64 = PKT_OVERHEAD + 9 + 9 + 1;

/// A third address, for roaming onto. `testfix` names only `a_addr` (the
/// anchor `Solo` installs against) and `b_addr`.
fn c_addr() -> SocketAddr {
    v4(3, 3)
}

/// §3.4's counter field, read straight out of a sealed datagram's
/// cleartext header (`header[6..14]`, little-endian).
///
/// Ruling 250's *"one packet, one counter, one sent-map entry"* is a claim
/// about **which** counter the probe takes, and no accessor exposes that:
/// `next_counter()` only says how many have been consumed. Reading the
/// header separates a build that seals the path frames first — the probe
/// then takes `floor + 1` — from one that seals a single coalesced packet
/// at `floor`.
fn packet_counter(dgram: &[u8]) -> u64 {
    u64::from_le_bytes(dgram[6..14].try_into().expect("§3.4's 14-byte header"))
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
/// [`spend_leaving`] with a target of zero.
fn spend_to_the_cap(s: &mut Solo, now: Instant) {
    spend_leaving(s, now, 0);
}

/// Spend the budget down until **exactly** `target` bytes of room remain,
/// with a single unreliable datagram, leaving nothing queued behind it.
///
/// **[R40-B, ruling 250]** The generalisation is the whole point. Before
/// 250, §7.3 asserted that *"the budget holds both and always does: 40 B
/// against a 90 B floor"*, a universal over **armed** budgets that made the
/// interesting room band unreachable by construction; 250(iii) moves the
/// check to pump time, against the **remaining** room, and the band
/// `PROBE_PACKET_LEN ..< COALESCED_PROBE_LEN` — 31 to 39 — becomes a state
/// a test can build. It is built by choosing `target`, and nothing else in
/// this file can build it.
///
/// Datagrams are the lever because ruling 155 makes the `0x30`
/// extends-to-end form mandatory, so the packet is `PKT_OVERHEAD + 1 +
/// payload` — a size this test file chooses to the byte. `send_datagram`
/// seals inside the call (§16.7), so one drain collects the packet.
fn spend_leaving(s: &mut Solo, now: Instant, target: u64) {
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
        space >= target,
        "the fixture cannot manufacture room: had {space}, asked to leave \
         {target}"
    );
    let spend = space - target;

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
    assert!(
        spend > path + PKT_OVERHEAD + 1 + 1,
        "the fixture needs room for one shaping datagram; had {spend} to \
         spend"
    );
    let payload = {
        let one = spend - path - PKT_OVERHEAD - 1 - 1;
        if one < 64 {
            one as usize
        } else {
            (spend - path - PKT_OVERHEAD - 1 - 2) as usize
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
        spend,
        "the shaping packet must be exactly the room it was told to spend, \
         or every later assertion in this file is calibrated against the \
         wrong cap"
    );
    assert_eq!(
        budget(s),
        (sent_before + spend, recv_before),
        "§3.2: sending credits `budget_sent` in datagram bytes and leaves \
         `budget_recv` alone"
    );
    assert_eq!(
        room(s),
        target,
        "the point of the fixture: the budget now admits exactly {target} \
         more bytes"
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

    let challenge = s
        .conn
        .outstanding_challenge()
        .expect("§7.3: an armed budget owes a challenge");
    let floor = s.conn.next_counter().expect("installed");

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    let in_flight_before = s.conn.bytes_in_flight();
    let d2 = release_the_budget(&mut s, t2);

    // **[R40-B, ruling 250 — 2026/08/17]** **One** packet, and its contents
    // are the ruling: *"the probe coalesces the owed path frames when the
    // remaining room at pump time admits the coalesced size — 40 B with one
    // 9 B path frame owed … One packet, one counter, one sent-map entry."*
    // The address is unvalidated, so §8.7's standing obligation owes a
    // `PATH_CHALLENGE`, and the 90 bytes a single keepalive credits are
    // well over the 40 the coalesced packet costs.
    //
    // **What the broken build does**: this assertion read `== 2, "the
    // challenge and the probe, in that order"`, written to ruling 212(c)'s
    // pre-pass — which **ruling 215 reversed** (the ranks *"stand exactly
    // as written"*, probe above both path frames) and **ruling 250**
    // replaced with coalescing. The shipped pre-250 pump emits a dedicated
    // 39 B challenge datagram and then a bare 31 B PING: two packets, two
    // counters, two sent-map entries, 70 bytes of a scarce budget where 40
    // carries the same information. Every assertion in this block separates
    // the two, and they are deliberately four independent separations —
    // count, size, plaintext, counter — because a build that merges the
    // frames but keeps two sent-map entries is a different defect from one
    // that keeps the pre-pass.
    assert_eq!(
        d2.transmits().len(),
        1,
        "ruling 250: the probe and the owed challenge are **one packet**"
    );
    let probe = d2.transmits().swap_remove(0);
    assert_eq!(
        probe.data.len() as u64,
        COALESCED_PROBE_LEN,
        "§7.3's own arithmetic, one header and one tag: 14 + 9 + 1 + 16 = \
         40. The two-packet build pays 39 + 31"
    );
    // Read as **raw plaintext** rather than through `testfix`'s decoder:
    // ten fixed self-delimiting bytes need no decoder, and this assertion
    // then holds whatever that decoder does or does not yet know about
    // §8.3's two new rows.
    let pt = s.peer.open_dgram(&probe.data);
    let mut want = vec![FRAME_PATH_CHALLENGE as u8];
    want.extend_from_slice(&challenge);
    want.push(FRAME_PING as u8);
    assert_eq!(
        pt, want,
        "§8.5 (ruling 208): the path frames are **first among the control \
         frames**, PING is last among length-prefixed frames — so the nine \
         challenge bytes precede the one PING byte **inside one plaintext**"
    );
    assert_eq!(
        packet_counter(&probe.data),
        floor,
        "ruling 250: *\"the probe takes the floor counter again\"* — a \
         build that seals the path frames in their own packet first leaves \
         the probe on `floor + 1`"
    );
    assert_eq!(
        s.conn.next_counter().expect("installed"),
        floor + 1,
        "one packet, **one counter**: the pre-pass build consumed two"
    );
    assert_eq!(
        s.conn.bytes_in_flight(),
        in_flight_before + COALESCED_PROBE_LEN,
        "one **sent-map entry** — ruling 43's *\"exempt from admission, \
         never from accounting\"* applies to the packet as built, so the \
         two-packet build shows 39 + 31 = 70 here"
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
/// with `room` = 90, the queued datagram alone is 90, and ruling 181's
/// PING-then-datagram packing of both overshoots it.
///
/// **[R40-B, ruling 250 — 2026/08/17]** The probe's own size moved and the
/// conclusion did not, which is why the numbers are restated rather than
/// left to be read past (working rule 4). The address is unvalidated, so a
/// `PATH_CHALLENGE` is owed and the probe leaves as the **coalesced** 40-byte
/// packet, not the bare 31-byte one; 50 bytes of room remain, and the
/// datagram needs 90. Under the pre-250 pre-pass the same 90 bought a 39 B
/// challenge datagram and a 31 B PING, leaving 20. Both readings hold this
/// test's two assertions — the datagram loses either way — so nothing here
/// separates the builds, and it is not asked to: the pins for the shape of
/// the probe's packet are elsewhere in this file.
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

    // **[R40-B, ruling 250 — 2026/08/17]** The probe **is** the first
    // packet sealed after the mark, and it takes the floor counter.
    //
    // This comment read the other way under ruling 212(c) — *"§7.3 ranks
    // `PATH_CHALLENGE` above it, so the challenge takes the floor counter
    // and the probe takes the next one"* — which ruling 215 reversed (the
    // ranks stand as written, probe at 2) and ruling 250 settled by
    // coalescing: *"the coalesced probe rides the ordinary pump, and the
    // probe takes the floor counter again"*. The ACK is still built from
    // the highest sealed counter, which is now the floor itself; covering
    // *more* than the floor is what ruling 41 makes a high-water mark for,
    // and covering *exactly* it is the case ruling 41 was written about.
    //
    // The equality is a pin, not a premise: the pre-250 two-packet build
    // seals the challenge at `floor` and the probe at `floor + 1`, so it
    // reads `highest == floor + 1` here.
    let highest = s.conn.next_counter().expect("installed") - 1;
    assert_eq!(
        highest, floor,
        "ruling 250: one packet, one counter — the probe is sealed **at** \
         the mark's floor ({highest} against {floor})"
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

// ═══════════════════════════════════════════════════════════════════════
// 7. Ruling 250 — the probe coalesces the owed path frames
//
// **[R40-B, 2026/08/17]** §7.3's amended text is the whole of this
// section:
//
// > The pump prefers **one packet**: the probe coalesces the owed path
// > frames when the remaining room at pump time admits the coalesced
// > size — 40 B with one 9 B path frame owed, 49 B with both — and emits
// > the bare 31 B PING otherwise, the path frames following at their rank
// > when room next admits them; when room admits neither, nothing is
// > emitted and no `Pto` deadline is announced (§13.3, ruling 249). One
// > packet, one counter, one sent-map entry.
//
// The build these four tests are written against is the **shipped** one,
// not a hypothetical: `pump_packets` implements ruling 212(c)'s pre-pass —
// a dedicated path-frame datagram emitted *above* the probe, then an early
// return — which ruling 215 reversed in the spec and never reached the
// code. So every test below is red on the base commit, and the failure
// mode named in each doc is what the base actually does.
// ═══════════════════════════════════════════════════════════════════════

/// **Ruling 250(ii)/(iii).** A room that admits the bare 31-byte probe and
/// **not** the 40-byte coalesced one: the probe leaves alone, and the
/// challenge follows at its rank when room next admits it.
///
/// # The state the pre-250 spec said could not exist
///
/// §7.3 argued the two *"never contend with one another"* from *"the
/// budget holds both and always does: 40 B against a 90 B floor"* — a
/// universal over **armed** budgets, silent about **remaining** ones.
/// Ruling 250(iii) moves the check to pump time, and the band
/// `PROBE_PACKET_LEN ..< COALESCED_PROBE_LEN` becomes a state a test can
/// build. [`spend_leaving`] is what builds it.
///
/// # What the broken build does (working rule 9)
///
/// The 39 bytes left here are chosen **to the byte**, because 39 is
/// exactly what a dedicated `PATH_CHALLENGE` datagram costs
/// (`14 + 9 + 16`). So the pre-250 pre-pass *fits*: it spends every byte
/// of room on the challenge, the 31-byte probe is then refused, and
/// `Contested` is never emitted and `TimerKind::Contested` never armed —
/// §7.5's liveness verdict deferred by a frame §7.3 ranks two places
/// **below** it, which is precisely the inversion ruling 215 reversed.
/// Choosing 40 bytes of room instead would separate nothing: both builds
/// emit both frames there.
#[test]
fn a_room_that_admits_the_bare_probe_but_not_the_coalesced_one_sends_the_probe_alone() {
    let t = t0();
    let mut s = responder_at(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(1);

    let challenge = s
        .conn
        .outstanding_challenge()
        .expect("§7.3: an armed budget owes a challenge");

    spend_leaving(&mut s, t, COALESCED_PROBE_LEN - 1);
    assert!(
        room(&s) >= PROBE_PACKET_LEN,
        "premise: the bare 31-byte probe fits"
    );
    assert!(
        room(&s) < COALESCED_PROBE_LEN,
        "premise: …and the 40-byte coalesced one does not — this is the \
         whole band"
    );
    assert_eq!(
        room(&s),
        PKT_OVERHEAD + 9,
        "premise, stated the other way: the room is exactly a dedicated \
         challenge datagram, so the pre-250 pre-pass fits and spends it all"
    );

    let d = mark(&mut s, t1);

    assert_eq!(
        d.transmits().len(),
        1,
        "§7.3 rank 2 beats rank 4: one packet leaves, and it is the probe"
    );
    let probe = d.transmits().swap_remove(0);
    assert_eq!(
        probe.data.len() as u64,
        PROBE_PACKET_LEN,
        "*\"emits the bare 31 B PING otherwise\"*"
    );
    assert_eq!(
        s.peer.open_dgram(&probe.data),
        vec![FRAME_PING as u8],
        "and nothing rides with it: the coalesced form did not fit, so the \
         challenge is not trimmed into the packet in some partial shape"
    );
    assert_eq!(
        contested_events(&d),
        1,
        "§5.1 transmission step 3 — the verdict is **not** deferred by a \
         frame ranked below the probe"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t1 + KEEPALIVE_TIMEOUT),
        "…and step 2 arms here, at the probe's own transmission"
    );
    assert_eq!(
        room(&s),
        (COALESCED_PROBE_LEN - 1) - PROBE_PACKET_LEN,
        "39 admitted, 31 spent: 8 left, which admits neither path frame"
    );
    assert_eq!(
        s.conn.outstanding_challenge(),
        Some(challenge),
        "§8.7: the challenge is **owed for as long as the arming lasts** — \
         a probe that could not carry it does not discharge it"
    );

    // *"…the path frames following at their rank when room next admits
    // them."* A peer PING is ack-eliciting, so a packet is owed and §8.7
    // re-offers the challenge on it; §12.4 defers that ACK to its timer for
    // a first ack-eliciting packet, which is settled here rather than
    // assumed (ruling 219's migration, in this file's own idiom).
    let d2 = s.deliver(t2, &[FRAME_PING as u8]);
    let mut frames = s.drain_frames(&d2);
    if let Some(at) = s.conn.timer(TimerKind::AckDelay) {
        s.conn.handle_timeout(at);
        let d3 = drain(&mut s.conn);
        let more = s.drain_frames(&d3);
        frames.extend(more);
    }
    assert!(
        frames
            .iter()
            .any(|f| matches!(f, Wire::PathChallenge(v) if *v == challenge)),
        "the same eight bytes, on the first packet room admitted after the \
         probe — a build that drops an uncoalescable challenge leaves the \
         address unvalidated for ever. Frames: {frames:?}"
    );
}

/// **Ruling 250(ii), the 49-byte case** — *"which a roam constructs (§13.6
/// keeps the owed `PATH_RESPONSE` and re-draws the challenge at one
/// instant)"*.
///
/// One packet carries the response, the challenge and the PING, in §8.5's
/// order: path frames **first among the control frames**, `PATH_RESPONSE`
/// before `PATH_CHALLENGE`, PING last among length-prefixed frames.
///
/// # What the broken build does (working rule 9)
///
/// The pre-250 pre-pass emits a dedicated 48-byte datagram carrying both
/// path frames and then a 31-byte PING: **two** packets, 79 bytes where 49
/// buys the same three frames, and the probe demoted below two frames
/// §7.3 ranks under it. The frame list of the packet carrying the PING is
/// what separates it — on the base build that list is `[Ping]` — and the
/// same list separates a build that merges the frames but reverses them.
#[test]
fn both_owed_path_frames_ride_the_probe_in_one_packet_in_spec_8_5_order() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t1 + Duration::from_secs(3);

    let armed_at_the_anchor = s
        .conn
        .outstanding_challenge()
        .expect("§3.2: the msg1 anchor armed one");

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark");

    // The roam that constructs the both-owed state: an authenticated,
    // window-fresh packet from a fresh source that itself carries a
    // `PATH_CHALLENGE`. One delivery, and the core owes a `PATH_RESPONSE`,
    // a freshly drawn `PATH_CHALLENGE` and the still-pending probe.
    let peer_value = [0x3c, 0x2b, 0x1a, 0x09, 0xf8, 0xe7, 0xd6, 0xc5];
    let d2 = s.deliver_from(t2, c_addr(), &path_challenge_frame(peer_value));

    assert_eq!(
        s.conn.remote_address(),
        Some(c_addr()),
        "premise: the roam committed"
    );
    let fresh = s
        .conn
        .outstanding_challenge()
        .expect("§7.3: the new arming draws one");
    assert_ne!(
        fresh, armed_at_the_anchor,
        "§7.3: **one challenge per arming, never reused across armings**"
    );
    let (_, recv) = budget(&s);
    assert!(
        AMPLIFICATION_FACTOR * recv >= COALESCED_PROBE_BOTH_LEN,
        "premise: the re-armed cap admits 49 bytes — 3 × the 39-byte \
         roaming packet is 117"
    );
    for t in d2.transmits() {
        assert_eq!(t.to, c_addr(), "§7.3: aimed at the new anchor");
    }

    // **Measured, not assumed.** §12.4 emits this ACK *immediately* rather
    // than on its delayed timer, and on the base build it leaves as a third
    // pure-ACK packet **behind** the probe — measured at `96f7ef0`:
    // `[[PathResponse, PathChallenge], [Ping], [Ack]]`, 48 + 31 + 35 bytes.
    // Whether §8.5 then packs that ACK ahead of the path frames in the
    // coalesced packet or leaves it in its own is not what ruling 250 rules
    // on, so the assertions below are stated over the packet's frames with
    // any ACK filtered out. The byte-exact form of the same claim is in
    // [`the_probe_the_notification_and_the_deadline_all_land_at_the_transmission_instant`],
    // where nothing is owed but the challenge.
    let packets = s.packets(&d2);
    let probes: Vec<&Vec<Wire>> = packets
        .iter()
        .filter(|p| p.iter().any(|f| matches!(f, Wire::Ping)))
        .collect();
    assert_eq!(
        probes.len(),
        1,
        "premise: §7.5 emits one probe. Packets: {packets:?}"
    );
    let carried: Vec<Wire> = probes[0]
        .iter()
        .filter(|f| !matches!(f, Wire::Ack { .. }))
        .cloned()
        .collect();
    assert_eq!(
        carried,
        vec![
            Wire::PathResponse(peer_value),
            Wire::PathChallenge(fresh),
            Wire::Ping,
        ],
        "ruling 250: *\"One packet, one counter, one sent-map entry\"* — \
         the response, the challenge and the PING are **one packet**, in \
         §8.5's order: path frames first among the control frames, \
         `PATH_RESPONSE` before `PATH_CHALLENGE` (*\"answering an \
         obligation before raising one\"*), PING last. The pre-250 pre-pass \
         puts both path frames in a datagram of their own and leaves the \
         probe carrying `[Ping]`. Packets: {packets:?}"
    );
    let responses = packets
        .iter()
        .flatten()
        .filter(|f| matches!(f, Wire::PathResponse(_)))
        .count();
    let challenges = packets
        .iter()
        .flatten()
        .filter(|f| matches!(f, Wire::PathChallenge(_)))
        .count();
    assert_eq!(
        (responses, challenges),
        (1, 1),
        "…and **one** of each across the whole drain: a build that \
         coalesces them into the probe *and* keeps the dedicated packet \
         pays 79 bytes of a 117-byte budget for 19 bytes of frames"
    );
    assert_eq!(
        contested_events(&d2),
        1,
        "one mark, one probe, one `Contested`, at this transmission"
    );
    assert_eq!(
        s.conn.timer(TimerKind::Contested),
        Some(t2 + KEEPALIVE_TIMEOUT),
        "armed at the coalesced packet's instant"
    );
}

/// **Ruling 250(i).** An admitted probe does not stop the pump: rank 9 —
/// new application data — leaves in the **same** drain.
///
/// §7.3 as ruling 215 wrote it: *"the send pump may emit the probe and
/// continue building on the same pass"*, and ruling 250(i): *"the
/// contested probe is built first, at rank 2, and the pump continues on
/// the same pass, exactly as ratified §7.3 orders."*
///
/// # What the broken build does (working rule 9)
///
/// Two separations, and they name two different halves of the defect.
/// First, the pre-pass puts a dedicated `PATH_CHALLENGE` datagram *ahead*
/// of the probe, so the **first** packet of the drain carries no PING.
/// Second, the early return then stops the pass, so the 2 000 queued
/// stream bytes wait for the next external event even though 3 600 bytes
/// of room and a wide-open window were both available here — ruling 250's
/// measured cost, *"a delay, not a kill, which is how it survived the full
/// suite"*.
///
/// Note what is **not** asserted: whether the stream bytes ride *inside*
/// the probe's packet or in one behind it. §8.5 permits either (the STREAM
/// fill precedes PING among length-prefixed frames), ruling 250 rules on
/// the path frames only, and a test that picked one would be pinning its
/// author's guess.
#[test]
fn an_admitted_probe_does_not_stop_the_pump_on_that_pass() {
    let t = t0();
    let mut s = responder_at_the_cap(t);
    let t1 = t + Duration::from_secs(1);
    let t2 = t + Duration::from_secs(2);

    let r = s.conn.open(Dir::Uni).expect("a uni stream");
    write_all(&mut s.conn, t1, r, &ramp(0, 2_000));
    let held = drain(&mut s.conn);
    assert!(
        held.transmits().is_empty(),
        "premise: room is 0, so §7.3 holds the stream bytes"
    );

    let d1 = mark(&mut s, t1);
    assert_nothing_happened(&mut s, &d1, "the mark, with stream data queued");

    // One full-size PADDING packet credits 3 × 1200 = 3600 bytes of room —
    // far more than the probe's 40 — so rank 9 is fundable on this very
    // pass. PADDING because it is not ack-eliciting: no ACK contends, and
    // the only thing that can stop the data is the pump's own control flow.
    let d2 = s.deliver(t2, &vec![FRAME_PADDING as u8; MAX_PLAINTEXT]);

    let packets = s.packets(&d2);
    assert!(
        !packets.is_empty(),
        "premise: the released room admits output"
    );
    assert!(
        packets[0].iter().any(|f| matches!(f, Wire::Ping)),
        "ruling 250(i): the probe is built **first**, at rank 2 — the \
         pre-pass build's first packet is the dedicated challenge. Packet: \
         {:?}",
        packets[0]
    );
    assert!(
        packets[0]
            .iter()
            .any(|f| matches!(f, Wire::PathChallenge(_))),
        "…carrying the owed challenge with it, since 3600 bytes admit the \
         coalesced 40. Packet: {:?}",
        packets[0]
    );
    assert!(
        packets
            .iter()
            .flatten()
            .any(|f| matches!(f, Wire::Stream { .. })),
        "…and the pump **continues on the same pass**: the early return is \
         gone, so rank 9 leaves in this drain. Packets: {packets:?}"
    );
    assert_eq!(
        contested_events(&d2),
        1,
        "one probe, one `Contested`, whatever else left beside it"
    );
}

/// **Ruling 250(iv)**, and §14.5's amended exemption clause: *"a probe that
/// coalesces the owed `PATH_RESPONSE`/`PATH_CHALLENGE` (§7.3) remains
/// exempt"*.
///
/// The window is full to the gate and the **budget** is wide open, so
/// §14.5 is the only thing that can refuse anything here. The coalesced
/// 40-byte probe leaves anyway, and its bytes are counted — ruling 43's
/// *"exempt from admission, never from accounting"*, applied to the packet
/// as built.
///
/// # What the broken build does (working rule 9)
///
/// The dedicated path-frame packet the coalescing replaces **was**
/// cwnd-gated and said so. So the pre-250 build emits a bare 31-byte PING
/// here — the challenge is refused by the gate and never leaves — and a
/// post-250 build that gates the merged packet emits nothing at all. §14.5
/// names the consequence: it *"would starve the challenge at collapsed
/// cwnd exactly where a roam makes it owed"*. The 40-byte size assertion
/// separates the first; `transmits().len()` separates the second.
///
/// A full window and a collapsed one are the same predicate here —
/// `bytes_in_flight + candidate > cwnd` — and a full one is the state this
/// fixture can build without a loss event to shape.
#[test]
fn the_coalesced_probe_keeps_the_probes_cwnd_exemption() {
    let t = t0();
    let mut s = responder_at(t);
    let t1 = t + Duration::from_millis(10);

    let challenge = s
        .conn
        .outstanding_challenge()
        .expect("§7.3: an armed budget owes a challenge");

    // Fund the budget until **congestion**, not amplification, is the
    // binding constraint. Eight full-size PADDING packets credit
    // 3 × 9600 B; nothing in them is ack-eliciting, so nothing is owed back
    // and `budget_sent` does not move.
    for _ in 0..8 {
        let d = s.deliver(t, &vec![FRAME_PADDING as u8; MAX_PLAINTEXT]);
        assert!(
            d.transmits().is_empty(),
            "PADDING elicits nothing, so nothing is owed and no packet is \
             manufactured to carry a challenge (§8.7's boundary)"
        );
    }

    // Fill the window.
    let r = s.conn.open(Dir::Uni).expect("a uni stream");
    write_all(&mut s.conn, t, r, &ramp(0, 24_000));
    let _ = drain(&mut s.conn);
    let idle = drain(&mut s.conn);
    assert!(
        idle.transmits().is_empty(),
        "premise: the gate has closed on the queued remainder"
    );
    let in_flight = s.conn.bytes_in_flight();
    assert!(
        in_flight + COALESCED_PROBE_LEN > s.conn.congestion_window(),
        "premise: §14.5's gate refuses a 40-byte candidate — {in_flight} \
         in flight against a {} window",
        s.conn.congestion_window()
    );
    assert!(
        room(&s) >= COALESCED_PROBE_LEN,
        "premise: and §7.3's budget does **not** refuse it, so cwnd is the \
         only constraint under test"
    );

    let d = mark(&mut s, t1);

    assert_eq!(
        d.transmits().len(),
        1,
        "§14.5: the probe is exempt from the admission gate, and ruling \
         250(iv) says the exemption covers the packet **as built**"
    );
    let probe = d.transmits().swap_remove(0);
    assert_eq!(
        probe.data.len() as u64,
        COALESCED_PROBE_LEN,
        "the challenge rides the exempt packet: a build that leaves the \
         path frames on the gated path emits 31 here and the address stays \
         unvalidated until the window opens"
    );
    let mut want = vec![FRAME_PATH_CHALLENGE as u8];
    want.extend_from_slice(&challenge);
    want.push(FRAME_PING as u8);
    assert_eq!(s.peer.open_dgram(&probe.data), want, "§8.5's order");
    assert_eq!(
        s.conn.bytes_in_flight(),
        in_flight + COALESCED_PROBE_LEN,
        "ruling 43: exempt from **admission**, never from accounting — the \
         piggyback is 18 B at most and §17.5's bound still covers it"
    );
    assert_eq!(contested_events(&d), 1, "and the verdict deadline arms");
}

//! Slice 7b's core-level coverage for the **keepalive livelock** (F1).
//!
//! Written from `CONTRACT-7b.md` §4 and `ADVERSARIAL-liveness.md` F1 by an
//! author who never read the fix (CLAUDE.md working rule 6), in a worktree
//! cut at `c131904`.
//!
//! # The defect
//!
//! `transmit_keepalive` refuses to seal when
//! `contested.is_pending() || !amplification.admits(30)`, and returns
//! **without moving `last_send`**. `transmit_keepalive_if_owed` then calls
//! `sync_liveness_timer()` unconditionally, which re-arms `Keepalive` at
//! `last_send + KEEPALIVE_TIMEOUT` and `PersistentKeepalive` at
//! `last_send + interval`. Neither expression consults `contested` or
//! `amplification`, so both land at an instant already passed, the shell
//! `sleep_until`s a past instant, and the driver spins.
//!
//! # Why "the deadline is correct" is not a test
//!
//! **A livelock is not a wrong value — it is the same correct value
//! forever.** `assert_eq!(timer(PersistentKeepalive), last_send + interval)`
//! passes the spinning build; it *is* the spinning build's behaviour. Every
//! test here asserts from the side that separates them:
//!
//! | shape | the broken build | the fixed build |
//! |---|---|---|
//! | **non-retrospection** — the announced deadline is `None` or `> now`, for the `now` just handled | announces `now` | announces the next real deadline, or `None` |
//! | **termination** — a driver loop that sleeps to each announced deadline reaches a fixed point in a bounded number of steps | never advances past the refusal instant | dies at `DEAD_TIMEOUT` in a handful of steps |
//! | **monotone advance** — the deadline after `handle_timeout(d)` is strictly greater than `d` | equal to `d` | greater |
//!
//! `CONTRACT-7b.md` §4.3 makes the first of these binding and adds the
//! second half of the bound: *"against a build that suppresses **the death
//! clock too** it also passes — so a second assertion is required: the
//! `Liveness` deadline is still announced when it was armed."* Every test
//! below carries that companion, because a core that answers `Timeout(None)`
//! to everything satisfies non-retrospection trivially and is an
//! **immortal** connection, which ruling 182's beacon proof calls the worse
//! collapse.
//!
//! # The two guards are pinned separately — this is the working-rule-9 part
//!
//! The refusal has **two** disjuncts and a build may fix one. So:
//!
//! * [`beacon_refused_by_the_budget_does_not_re_arm_in_the_past`] sits at
//!   `room == 0` with **no** contested mark. A build whose new arming
//!   condition consults `contested` but not `amplification` still spins here.
//! * [`beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past`] sits at
//!   `room == 30` **exactly** — the budget admits the 30-byte keepalive and
//!   refuses the 31-byte probe, so the mark is pending and the *budget is
//!   not* the reason the keepalive stays home. A build whose arming
//!   condition consults `amplification` but not `contested` still spins here.
//!
//! Neither test alone separates the four builds; together they do. That is
//! the whole reason the contract asks for **one** predicate
//! (`keepalive_can_leave`) rather than two copies.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here reads
//! a clock. Plain `#[test]`, no `sleep`. Every mutating call is followed by
//! draining `poll_output()` to the terminal `Timeout` (§16.4), which is what
//! [`testfix::drain`] does.
//!
//! # What is **not** here, and why — a reported reachability limit
//!
//! See [`the_passive_form`] at the end of this file. The `armed == false`
//! variant `ADVERSARIAL-liveness.md` calls *unbounded* is, on this author's
//! arithmetic, **not constructible**, and the reason is a one-line
//! inequality. It is reported rather than half-tested.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::time::{Duration, Instant};

use super::testfix::*;
use super::timers::TimerKind;

use crate::constants::{
    AEAD_TAG_LEN, AMPLIFICATION_FACTOR, DATA_HEADER_LEN, DEAD_TIMEOUT, INIT_PACKET_LEN,
    KEEPALIVE_TIMEOUT, RESP_PACKET_LEN,
};
use crate::error::ConnectionLost;

// ═══════════════════════════════════════════════════════════════════════
// 1. Fixture-level scaffolding
// ═══════════════════════════════════════════════════════════════════════

/// §3.4's cleartext header plus the AEAD tag — what every Data packet costs
/// before a single frame byte, and therefore **exactly** the size
/// `transmit_keepalive` asks the budget to admit.
const KEEPALIVE_LEN: u64 = (DATA_HEADER_LEN + AEAD_TAG_LEN) as u64;

/// §7.5's contested probe on the wire: one `FRAME_PING` byte in a Data
/// packet. One byte larger than the keepalive, which is the entire reason
/// [`beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past`] can
/// separate the two guards.
const PROBE_LEN: u64 = KEEPALIVE_LEN + 1;

/// §7.5's beacon, at its floor. `PERSISTENT_KEEPALIVE_MIN` is 1 s, so this
/// is the shortest interval the core will accept — and it must be shorter
/// than the first PTO (`K_INITIAL_RTT`-derived, ~1.02 s) or the beacon is
/// not the timer under test. Every test that relies on that asserts it
/// rather than assuming it.
const BEACON: Duration = Duration::from_secs(1);

/// A core installed as the **responder**, i.e. anchored from a msg1 source:
/// §3.2's second arming event, `budget = (107, 196)`, cap 588, room 481.
///
/// **[Integrator, ruling 218 — premise migrated, not weakened.]** This
/// author wrote `(0, 196)` against a tree where the arming charged nothing
/// for msg2. The adversarial review's A2 found that msg2's 107 bytes are
/// emitted endpoint-side and were charged to **nothing**, making the real
/// responder ratio 3.55× against a normative MUST of 3; the remediation
/// implementer fixed it in the same slice, blind to this file.
///
/// So this red was the fixture premise doing precisely its job — the author
/// wrote it to *"make the premise visible rather than silently weakening
/// every bound below it"*, and it caught a deliberate change to the very
/// quantity it pins. Every test here derives its room through
/// [`room`]/[`spend_to`] rather than hardcoding, so nothing below this line
/// needed touching.
fn responder_at(now: Instant) -> Solo {
    let s = Solo::installed_from_msg1_at(now);
    assert_eq!(
        s.conn.amplification_budget(),
        Some((RESP_PACKET_LEN as u64, INIT_PACKET_LEN as u64)),
        "fixture: a msg1-anchored responder starts unvalidated with the \
         initiation's bytes credited and **msg2's charged** (§3.2, A2)"
    );
    s
}

/// The budget's two counters, which must be armed.
fn budget(s: &Solo) -> (u64, u64) {
    s.conn
        .amplification_budget()
        .expect("fixture: the address is unvalidated, so a budget is armed")
}

/// How many more datagram bytes §7.3 will admit right now.
fn room(s: &Solo) -> u64 {
    let (sent, recv) = budget(s);
    (AMPLIFICATION_FACTOR * recv).saturating_sub(sent)
}

/// Spend the budget down until **exactly** `target` bytes of room remain,
/// with a single unreliable datagram.
///
/// Datagrams are the lever because §11's send seals inside the call (§16.7),
/// so one drain collects the packet and the size is chosen to the byte. The
/// frame is `DATAGRAM_LEN` (`0x31`): one type byte plus an explicit length
/// varint, 1 byte below 64 and 2 up to 16383 (§8.1). The emitted packet's
/// length is **asserted**, so a wrong varint guess fails here, loudly, and
/// never silently mis-calibrates a later assertion.
///
/// The datagram is a *marking* send (§7.4 — fresh application intent), so it
/// pins `last_send` at `now`, which is what both beacon tests arm from.
fn spend_leaving(s: &mut Solo, now: Instant, target: u64) {
    // §8.5 packs an owed ACK ahead of the datagram fill in the same packet,
    // so an owed ACK would make the shaping packet larger than the size this
    // helper computed and the calibration would be off. Flush it first if
    // one is armed. (This is ruling 201's correction to `tests_contested.rs`,
    // rediscovered here rather than inherited: the helper is private there.)
    if let Some(at) = s.conn.timer(TimerKind::AckDelay) {
        s.conn.handle_timeout(at);
        let _ = drain(&mut s.conn);
    }

    let space = room(s);
    assert!(
        space > target,
        "fixture: nothing to spend — room {space}, target {target}"
    );
    let packet = space - target;
    assert!(
        packet > KEEPALIVE_LEN + 2,
        "fixture: the shaping packet must hold a datagram frame; got {packet}"
    );
    // **[Integrator, ruling 217]** An unvalidated address offers its
    // 9-byte `PATH_CHALLENGE` at stage 2, *ahead* of the stage-3 datagram
    // fill (§8.5). Sized without it, the shaping datagram no longer fits the
    // room and `packing.datagram()` refuses it — so the drain yields a
    // 39-byte challenge-only packet and the whole calibration is wrong.
    // This author wrote the helper against a tree with no path frames in it.
    //
    // Nine bytes, not a guess: 1 type byte + 8 opaque (§8.3, ruling 208).
    // Subtracting it here keeps the helper's contract exactly as written —
    // **one** packet, of **exactly** `packet` bytes — rather than relaxing
    // the assertions, which is what would hide the next such change.
    let challenge_cost: u64 = if s.conn.amplification_budget().is_some() {
        1 + 8
    } else {
        0
    };
    let payload = {
        let one = packet - KEEPALIVE_LEN - 1 - 1 - challenge_cost;
        if one < 64 {
            one
        } else {
            packet - KEEPALIVE_LEN - 1 - 2 - challenge_cost
        }
    };
    assert!(
        payload < 16_384,
        "fixture: the varint arithmetic above covers payloads below 16384; got {payload}"
    );

    let (sent_before, recv_before) = budget(s);
    s.conn
        .send_datagram(now, &ramp(0, payload as usize))
        .expect("§11: a sub-maximum datagram is accepted");
    let d = drain(&mut s.conn);

    assert_eq!(
        d.transmits().len(),
        1,
        "fixture: the shaping datagram is sized to fit, so it must leave"
    );
    assert_eq!(
        d.transmits()[0].data.len() as u64,
        packet,
        "fixture: the shaping packet must be exactly the room to be spent, \
         or every later assertion is calibrated against the wrong cap"
    );
    assert_eq!(
        budget(s),
        (sent_before + packet, recv_before),
        "§3.2: sending credits `budget_sent` in datagram bytes"
    );
    assert_eq!(room(s), target, "fixture: the point of the helper");
}

/// §7.5's passive debt and §7.4's arming bit, read straight off the core.
///
/// Both are `pub(crate)` on `Liveness` and reachable through
/// `Connection::liveness()`. Reading them is what keeps the fixtures below
/// from passing **vacuously**: a construction that failed to reach the held
/// state would otherwise satisfy "no deadline in the past" for the boring
/// reason that no keepalive was ever owed.
fn liveness_state(s: &Solo) -> (bool, bool, Instant) {
    let l = s
        .conn
        .liveness()
        .expect("fixture: the session is installed");
    (l.owes_passive_keepalive(), l.is_armed(), l.last_send())
}

/// Assert the terminal `Timeout` of a drain is not retrospective.
///
/// **This is F1's pin.** `Driver::run` does `sleep_until(deadline)`; a
/// deadline at or before the `now` just handled completes immediately and
/// `handle_timeout` re-fires the same timer, forever. `None` is fine — it is
/// a parked connection, not a spinning one.
#[track_caller]
fn assert_not_retrospective(d: &Drained, now: Instant, what: &str) {
    match d.deadline {
        None => {}
        Some(next) => assert!(
            next > now,
            "{what}: the core announced a deadline at or before the instant it \
             was just handed ({next:?} <= {now:?}). `sleep_until` completes \
             immediately and `handle_timeout` re-fires the same timer — \
             ruling 141's spin class, arriving through `sync_liveness_timer`."
        ),
    }
}

/// The companion half `CONTRACT-7b.md` §4.3 requires: **the death clock is
/// still announced**.
///
/// Suppressing `TimerKind::Liveness` alongside the keepalives satisfies
/// [`assert_not_retrospective`] trivially and turns a spinning connection
/// into an **immortal** one — the collapse ruling 182's beacon proof exists
/// to forbid. Without this assertion the whole file is passed by
/// `fn deadline() -> None`.
#[track_caller]
fn assert_death_clock_armed(s: &Solo, expected: Instant) {
    assert_eq!(
        s.conn.timer(TimerKind::Liveness),
        Some(expected),
        "§7.4's death clock must survive the keepalive hold: a build that \
         suppresses it too is immortal, not quiet (ruling 182)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. F1-a — the budget refuses the keepalive
// ═══════════════════════════════════════════════════════════════════════

/// `room == 0`, no contested mark: the **amplification** disjunct alone
/// blocks the beacon.
///
/// Mutation caught: `sync_liveness_timer` re-arming `PersistentKeepalive` at
/// `last_send + interval` without consulting `amplification`. Also caught: a
/// partial fix whose new arming condition tests only `contested`.
///
/// Not caught by, and deliberately not asserted as, `timer(...) == Some(fire)`
/// — that equality is precisely what the broken build satisfies.
#[test]
fn beacon_refused_by_the_budget_does_not_re_arm_in_the_past() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, 0);

    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5: 1 s is `PERSISTENT_KEEPALIVE_MIN`, the floor, and admissible");
    let _ = drain(&mut s.conn);

    let (_, armed, last_send) = liveness_state(&s);
    assert!(
        armed,
        "fixture: the shaping datagram is a marking send, so the death clock is armed"
    );
    assert_eq!(
        last_send, start,
        "fixture: the beacon arms from `last_send`, which the shaping send pinned"
    );

    let fire = start + BEACON;
    // **[Integrator, ruling 220 — premise inverted, and the test is
    // stronger for it.]** This author expected the beacon *armed* at `fire`,
    // then fired, refused, and re-armed in the future. The implementer —
    // blind to this file — closed F1 the other way: one
    // `keepalive_can_leave()` predicate gates `transmit_keepalive`'s guard
    // **and** `sync_liveness_timer`'s arming, so while the hold is on there
    // is no beacon deadline at all.
    //
    // Both prevent the spin, and suppression is the stronger of the two:
    // there is no wake to waste, and the state this file was written to
    // catch becomes **unreachable by construction** rather than merely
    // survivable. That is why the assertions below now pass easily — not
    // because the test decayed, but because the defect class is gone.
    //
    // The premise is **inverted rather than deleted**, for exactly the
    // reason this author gave it: a vacuous pass and a correct one must stay
    // distinguishable. What makes suppression safe — that the beacon returns
    // when the hold lifts — is pinned by
    // `the_beacon_returns_when_the_hold_lifts` below.
    assert_eq!(
        s.conn.timer(TimerKind::PersistentKeepalive),
        None,
        "the budget admits nothing at room 0, so no beacon deadline is armed \
         at all — F1's spin is unreachable rather than survivable"
    );
    // The shaping datagram is ack-eliciting (§8.3), so a PTO is armed too.
    // It must not be due at `fire`, or the timer under test is not the one
    // that fires. `K_INITIAL_RTT` makes the first PTO ~1.02 s; asserting it
    // means a constants change turns this into a clear fixture failure
    // instead of a silent mis-test.
    assert!(
        s.conn.timer(TimerKind::Pto).is_none_or(|p| p > fire),
        "fixture: the beacon must be the only timer due at {fire:?}"
    );

    s.conn.handle_timeout(fire);
    let d = drain(&mut s.conn);

    assert!(
        d.transmits().is_empty(),
        "fixture: §7.3 admits nothing at room 0, so the keepalive must stay \
         home — if it left, this test proves nothing about the hold"
    );
    assert_eq!(
        room(&s),
        0,
        "fixture: and the budget did not move, because nothing was sealed"
    );

    assert_not_retrospective(&d, fire, "a beacon the budget refused");
    assert_death_clock_armed(&s, start + DEAD_TIMEOUT);
}

/// The same hold, driven a second time at the **same instant**: the deadline
/// must not be the one just consumed.
///
/// This is the monotone-advance framing, and it is the one that survives a
/// build that clamps rather than suppresses. `CONTRACT-7b.md` §4.2 declines
/// a shell-side `max(d, now)` clamp for exactly this reason — *"`sleep_until`
/// completes immediately for exactly the same set of deadlines"* — and a
/// core-side clamp has the same defect one layer down. A clamped core
/// announces `Some(fire)` at `fire` and this assertion still fails.
///
/// Mutation caught: any fix that keeps the deadline at `last_send + interval`
/// and merely bounds it below by `now`.
#[test]
fn a_refused_beacon_does_not_re_deliver_itself_at_the_same_instant() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, 0);
    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    let fire = start + BEACON;
    s.conn.handle_timeout(fire);
    let first = drain(&mut s.conn);
    assert_not_retrospective(&first, fire, "the first refused beacon");

    // Hand the core the same instant again. A correct core has nothing due:
    // `take_due(fire)` finds no timer, nothing is sealed, and the announced
    // deadline is unchanged. The spinning core finds `PersistentKeepalive`
    // due *again*, refuses *again*, and re-arms at `fire` *again* — which is
    // the loop, expressed in two calls instead of ten million.
    s.conn.handle_timeout(fire);
    let second = drain(&mut s.conn);

    assert!(
        second.transmits().is_empty(),
        "nothing can leave at room 0 on the second pass either"
    );
    assert_not_retrospective(&second, fire, "the second refused beacon");
    assert_eq!(
        first.deadline, second.deadline,
        "a refusal is idempotent: handling the same instant twice must reach \
         the same fixed point, not walk"
    );
    assert_death_clock_armed(&s, start + DEAD_TIMEOUT);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. F1-b — a pending contested mark blocks the keepalive
// ═══════════════════════════════════════════════════════════════════════

/// `room == 30` **exactly**: §7.3 admits the 30-byte keepalive and refuses
/// the 31-byte probe, so the mark stays pending and the **contested**
/// disjunct alone blocks the beacon.
///
/// This is the one-byte gap the whole file is built around. At `room == 30`:
///
/// * `amplification.admits(KEEPALIVE_LEN)` is **true** — 30 ≤ 30;
/// * `contested.is_pending()` is **true** — the probe needs 31.
///
/// Mutation caught: a fix whose new arming condition consults the budget but
/// not the mark. Such a build passes every test in §2 of this file and spins
/// here, which is precisely why `CONTRACT-7b.md` §4.1 requires **one**
/// predicate shared by `transmit_keepalive` and `sync_liveness_timer`
/// rather than two copies — *"a build that states it twice is the build that
/// drifts"*.
#[test]
fn beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, KEEPALIVE_LEN);

    assert_eq!(
        room(&s),
        KEEPALIVE_LEN,
        "fixture: the budget admits a keepalive and nothing larger"
    );
    assert!(
        room(&s) < PROBE_LEN,
        "fixture: …and in particular refuses §7.5's 31-byte probe, which is \
         what leaves the mark pending"
    );

    s.conn.mark_contested(start);
    let d = drain(&mut s.conn);
    assert!(
        d.transmits().is_empty(),
        "§7.5: the probe the budget will not admit leaves the mark pending, \
         it does not go out"
    );
    assert!(
        s.conn.contested().is_pending(),
        "fixture: the state under test is the **pending** mark"
    );
    assert_eq!(
        room(&s),
        KEEPALIVE_LEN,
        "fixture: nothing was sealed, so the budget still admits a keepalive — \
         the mark, not the budget, is what blocks it"
    );

    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    let fire = start + BEACON;
    // **[Integrator, ruling 220 — premise inverted, and the test is
    // stronger for it.]** This author expected the beacon *armed* at `fire`,
    // then fired, refused, and re-armed in the future. The implementer —
    // blind to this file — closed F1 the other way: one
    // `keepalive_can_leave()` predicate gates `transmit_keepalive`'s guard
    // **and** `sync_liveness_timer`'s arming, so while the hold is on there
    // is no beacon deadline at all.
    //
    // Both prevent the spin, and suppression is the stronger of the two:
    // there is no wake to waste, and the state this file was written to
    // catch becomes **unreachable by construction** rather than merely
    // survivable. That is why the assertions below now pass easily — not
    // because the test decayed, but because the defect class is gone.
    //
    // The premise is **inverted rather than deleted**, for exactly the
    // reason this author gave it: a vacuous pass and a correct one must stay
    // distinguishable. What makes suppression safe — that the beacon returns
    // when the hold lifts — is pinned by
    // `the_beacon_returns_when_the_hold_lifts` below.
    assert_eq!(
        s.conn.timer(TimerKind::PersistentKeepalive),
        None,
        "the budget admits nothing at room 0, so no beacon deadline is armed \
         at all — F1's spin is unreachable rather than survivable"
    );
    assert!(
        s.conn.timer(TimerKind::Pto).is_none_or(|p| p > fire),
        "fixture: the beacon must be the only timer due at {fire:?}"
    );

    s.conn.handle_timeout(fire);
    let d = drain(&mut s.conn);

    assert!(
        d.transmits().is_empty(),
        "§7.5: a pending mark outranks the keepalive, budget or no budget"
    );
    assert!(
        s.conn.contested().is_pending(),
        "and the mark is still pending — nothing here clears it"
    );

    assert_not_retrospective(&d, fire, "a beacon a pending mark blocked");
    assert_death_clock_armed(&s, start + DEAD_TIMEOUT);
}

/// The mirror of the fixture assertion above, stated as its own test so the
/// one-byte calibration cannot rot silently.
///
/// At `room == 31` the probe **is** admitted, the mark arms, and the
/// keepalive is no longer blocked by `contested`. If this ever stops holding,
/// [`beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past`] has
/// stopped isolating the contested disjunct and has quietly become a second
/// copy of the budget test.
///
/// Working rule 9's shape: the bound is only a separation while the
/// neighbouring value falls on the other side of it.
#[test]
fn one_more_byte_of_room_releases_the_probe_and_the_isolation_with_it() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, PROBE_LEN);

    s.conn.mark_contested(start);
    let d = drain(&mut s.conn);

    assert_eq!(
        d.transmits().len(),
        1,
        "§7.5: at room 31 the budget admits the probe, so it goes out at once"
    );
    assert_eq!(
        d.transmits()[0].data.len() as u64,
        PROBE_LEN,
        "the probe is one PING byte in a Data packet"
    );
    assert!(
        !s.conn.contested().is_pending(),
        "the transmission is what moves the mark out of `Pending` — so \
         `room == 30` is the *largest* room at which the pending state \
         exists, and that is the value the isolation test uses"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Termination — the driver loop, run at the core
// ═══════════════════════════════════════════════════════════════════════

/// The step cap [`drive_like_the_shell`] is given, and the separation it buys.
///
/// **[round 41 item 11 — 2026/08/18]** Both call sites passed a literal `64`,
/// under a comment calling it *"deliberately loose: the point is bounded, not
/// small, and a tight cap would turn an unrelated recovery-timer change into a
/// red here"*. Loose to the point of **vacuous** — working rule 9: *a bound is
/// only a test if the degenerate case violates it*, and nothing violated 64.
/// The three quantities, measured at this commit rather than argued:
///
/// * the shipped build reaches `Timeout(None)` in **1** step. The budget
///   admits no probe, so ruling 249 announces no `Pto` at all; the only timer
///   left is §7.4's liveness, one sleep to `install + DEAD_TIMEOUT`.
/// * the build this loop's own assertion message names — *"a held keepalive
///   re-offered without bound"* — scores `DEAD_TIMEOUT / BEACON` = **25**.
///   Measured, not assumed: driving an **un**held beacon on a validated
///   address is exactly that behaviour, and it takes 25 steps to die. 25 < 64,
///   so the old cap could not fail the one build it was written against.
/// * a build that walked §13.3's ladder here instead (ruling 249's defect, in
///   the variant whose anchor advances, so the retrospection assert above does
///   not catch it) reaches `DEAD_TIMEOUT` in **6** firings at ruling 254's 2³
///   — arithmetic, not measured: a ~1.02 s first interval doubling to 8× sums
///   1, 3, 7, 15, 23, 31 intervals, and 25 s falls between the fifth and the
///   sixth. At the old 2⁶ it was **5**, a longer ladder having fewer rungs
///   inside the same 25 s — so tightening here is not a consequence of 254 so
///   much as something 254 made worth doing properly.
///
/// The separating band is therefore `1 ≤ cap < 25`, and every shape above
/// sits outside it. 4 keeps a three-step margin for exactly the unrelated
/// recovery-timer change the old comment was protecting, and still fails the
/// 25-step build by a factor of six.
const FIXED_POINT_CAP: usize = 4;

/// The upper end of that band, pinned to the constants it is derived from
/// rather than transcribed: a cap at or above a beacon re-offered every
/// interval until §7.4 reaps the session asserts nothing at all.
const _: () = assert!(
    (FIXED_POINT_CAP as u64) < DEAD_TIMEOUT.as_secs() / BEACON.as_secs(),
    "a cap at or above DEAD_TIMEOUT / BEACON separates no build at all"
);

/// Simulate `Driver::run`'s steps 4 and 5 against the core alone: sleep to
/// whatever deadline was announced, hand it back, repeat.
///
/// Returns the number of steps taken. Panics on the two failures that *are*
/// the livelock:
///
/// * an announced deadline at or before the `now` that produced it — the
///   spin, caught on its **first** iteration rather than after ten million;
/// * more than `cap` steps without reaching `Timeout(None)` — the spin,
///   caught by exhaustion if some other route produced it.
///
/// This is the shape `CONTRACT-7b.md` §4.3 warns cannot be written against a
/// runtime: *"on tokio's paused clock a livelock is an infinite loop that
/// never advances virtual time, so the test hangs rather than fails."* At the
/// core there is no clock to advance, so the loop is finite by construction
/// and the failure is an assertion instead of a hang.
fn drive_like_the_shell(s: &mut Solo, start: Instant, cap: usize) -> usize {
    let mut now = start;
    let mut deadline = drain(&mut s.conn).deadline;
    let mut steps = 0usize;
    loop {
        let Some(next) = deadline else {
            return steps;
        };
        assert!(
            next > now,
            "F1: after {steps} step(s) the core announced {next:?} while the \
             driver's clock stood at {now:?}. `sleep_until` returns \
             immediately and the next `handle_timeout` re-derives the same \
             deadline — 100 % of one core, invisible on the wire."
        );
        now = next;
        s.conn.handle_timeout(now);
        deadline = drain(&mut s.conn).deadline;
        steps += 1;
        assert!(
            steps <= cap,
            "F1: the driver loop took more than {cap} steps without parking or \
             dying. A held keepalive must not be re-offered without bound."
        );
    }
}

/// The budget-refused hold, driven to a fixed point.
///
/// Mutation caught: the spin, on the **first** step past the refusal
/// (`next == now`). A build that suppresses correctly announces no `Pto`
/// at all while the budget is closed (§13.3, ruling 249 — before that
/// ruling it walked the PTO backoff here) and dies of §7.4's liveness at
/// `install + DEAD_TIMEOUT`, in a handful of steps.
///
/// The cap read `64` until round 41 item 11 measured what it was separating:
/// nothing. See [`FIXED_POINT_CAP`] — this build takes **1** step, and the
/// re-offering build the assertion names takes 25.
#[test]
fn a_connection_holding_its_beacon_still_reaches_a_fixed_point() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, 0);
    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    let steps = drive_like_the_shell(&mut s, start, FIXED_POINT_CAP);

    // The loop ends only at `Timeout(None)`, and §15.4's liveness row is the
    // only thing that produces it here — so the connection must be dead, and
    // dead of the right cause. Without this the test would also pass against
    // a core that answered `None` from the first drain and did nothing at
    // all, which is the degenerate build working rule 9 asks about.
    assert!(steps > 0, "the loop must actually have run");
    let last = drain(&mut s.conn);
    assert_eq!(last.deadline, None, "a dead connection arms nothing");
}

/// The pending-mark hold, driven to a fixed point.
///
/// Same pin, other disjunct. `ADVERSARIAL-liveness.md` calls this variant
/// *unbounded* on the grounds that the mark clears only on a peer send; the
/// death clock still bounds it here, because the shaping datagram is a
/// marking send and §7.4 arms from that. The **timer** is what is unbounded,
/// not the connection, and this test pins the timer.
#[test]
fn a_connection_holding_its_beacon_behind_a_mark_still_reaches_a_fixed_point() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, KEEPALIVE_LEN);
    s.conn.mark_contested(start);
    let _ = drain(&mut s.conn);
    assert!(s.conn.contested().is_pending(), "fixture");
    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    let steps = drive_like_the_shell(&mut s, start, FIXED_POINT_CAP);
    assert!(steps > 0, "the loop must actually have run");
}

/// The **control**: an unheld beacon on a validated address must keep firing.
///
/// Mutation caught: over-suppression. A build that solves F1 by never arming
/// `PersistentKeepalive` at all passes every other test in this file — no
/// deadline is ever retrospective if no deadline is ever armed — and has
/// deleted §7.5's beacon. Here the budget is absent entirely (`connect()`
/// anchors a **validated** address, §3.2), no mark is taken, and the beacon
/// must fire, seal a 30-byte datagram, and re-arm **one interval on**.
///
/// This is the assertion that makes the rest of the file mean something.
#[test]
fn an_admissible_beacon_still_fires_and_advances() {
    let start = t0();
    // `Solo::installed_at` installs the budget **validated**, so `admits` is
    // total and no hold exists.
    let mut s = Solo::installed_at(start);
    assert_eq!(
        s.conn.amplification_budget(),
        None,
        "fixture: a validated address arms no budget, so neither disjunct of \
         the refusal can be true"
    );

    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    let mut now = start;
    let mut sent = 0usize;
    for i in 1..=3u32 {
        let fire = start + BEACON * i;
        assert_eq!(
            s.conn.timer(TimerKind::PersistentKeepalive),
            Some(fire),
            "§7.5: the beacon re-arms one interval past each marking send"
        );
        assert!(fire > now, "the beacon deadline advances every interval");
        now = fire;
        s.conn.handle_timeout(now);
        let d = drain(&mut s.conn);
        let ts = d.transmits();
        assert_eq!(ts.len(), 1, "§7.5: the beacon seals exactly one datagram");
        assert_eq!(
            ts[0].data.len() as u64,
            KEEPALIVE_LEN,
            "§3.4: the keepalive is the empty plaintext — a 30-byte datagram"
        );
        assert_not_retrospective(&d, now, "an admissible beacon");
        sent += 1;
    }
    assert_eq!(sent, 3, "three intervals, three keepalives");
}

/// The control for the **other** timer: §7.5's passive keepalive still arms,
/// still fires, and still clears itself.
///
/// `TimerKind::Keepalive` has no test of its own anywhere above, because §5
/// of this file argues its *held* state is not constructible. That makes an
/// over-suppressing fix on this path invisible: a `keepalive_can_leave`
/// wired into the passive arm with the wrong sense, or applied where it does
/// not belong, would stop the passive keepalive dead and nothing else here
/// would notice. So the admissible path gets pinned end to end.
///
/// The route is §7.4's, exactly: an authenticated fresh receive sets §7.5's
/// passive debt (ruling 195 — the flag, not `R > S`) and disarms the death
/// clock; the keepalive is then owed at `last_send + KEEPALIVE_TIMEOUT`,
/// where `last_send` is still the install pin because nothing marking has
/// been sealed since.
///
/// Mutation caught: `sync_liveness_timer` failing to arm `Keepalive`;
/// `transmit_keepalive` refusing on a validated address; the marking seal not
/// clearing the debt, which would re-arm the keepalive forever at a
/// `last_send` that does move — a walking timer rather than a stuck one, and
/// the assertion that catches it is the final `None`.
#[test]
fn an_admissible_passive_keepalive_fires_and_then_disarms() {
    let start = t0();
    let mut s = Solo::installed_at(start);

    let (owed, armed, last_send) = liveness_state(&s);
    assert!(
        !owed,
        "§7.4's install pin: nothing received yet, nothing owed"
    );
    assert!(armed, "…and the death clock is armed from the install");
    assert_eq!(last_send, start);
    assert_eq!(
        s.conn.timer(TimerKind::Keepalive),
        None,
        "no receive, no passive debt, no keepalive timer"
    );

    // §3.4's empty plaintext from the peer: authenticated, window-fresh, and
    // **not** ack-eliciting, so it owes no ACK that could contend.
    let recv_at = start + Duration::from_millis(1);
    let d = s.deliver(recv_at, &[]);
    assert!(
        d.transmits().is_empty(),
        "fixture: an empty plaintext elicits nothing, so nothing marking \
         leaves and `last_send` stays at the install"
    );

    let (owed, armed, last_send) = liveness_state(&s);
    assert!(owed, "§7.5: a side that has received since it last sent");
    assert!(
        !armed,
        "§7.4: the receive disarms until the next arming send"
    );
    assert_eq!(last_send, start, "no marking send happened");

    let fire = start + KEEPALIVE_TIMEOUT;
    assert_eq!(
        s.conn.timer(TimerKind::Keepalive),
        Some(fire),
        "§7.5: the passive keepalive is owed one `KEEPALIVE_TIMEOUT` past \
         `last_send`"
    );
    assert_eq!(
        d.deadline,
        Some(fire),
        "and it is the nearest deadline: the death clock is disarmed, so \
         this is the whole of what the core announces"
    );

    s.conn.handle_timeout(fire);
    let d = drain(&mut s.conn);
    let ts = d.transmits();
    assert_eq!(ts.len(), 1, "§7.5: one keepalive");
    assert_eq!(
        ts[0].data.len() as u64,
        KEEPALIVE_LEN,
        "§3.4: the empty plaintext — a 30-byte datagram"
    );

    let (owed, armed, last_send) = liveness_state(&s);
    assert!(
        !owed,
        "§7.5: the marking send is what the keepalive was for"
    );
    assert!(armed, "§7.4: and a marking send arms the death clock");
    assert_eq!(last_send, fire);
    assert_eq!(
        s.conn.timer(TimerKind::Keepalive),
        None,
        "the debt is paid, so the timer is not re-armed — a build that \
         re-arms it here walks `last_send` forward every 10 s and talks to a \
         silent peer for ever"
    );
    assert_not_retrospective(&d, fire, "an admissible passive keepalive");
    assert_death_clock_armed(&s, recv_at + DEAD_TIMEOUT);
}

/// The last control: **`Timeout(None)` is not a free pass.**
///
/// [`assert_not_retrospective`] treats `None` as acceptable, because a parked
/// connection really does announce it. That makes `fn deadline() -> None` a
/// build which passes every non-retrospection assertion in this file. The
/// three [`assert_death_clock_armed`] calls block it at the instants they
/// cover; this test blocks it at the install, which is the instant §7.4 makes
/// load-bearing — *"the handshake is the arming event … without it a session
/// that receives nothing would be held **forever**, since §7.6 is deleted and
/// liveness is the only reaper."*
///
/// Mutation caught: any suppression broad enough to reach
/// `TimerKind::Liveness`, which `CONTRACT-7b.md` §4.1 forbids in terms —
/// *"suppressing it too would turn a spinning connection into an **immortal**
/// one, which is worse."*
#[test]
fn a_freshly_installed_core_announces_its_death_clock_and_not_none() {
    let start = t0();
    let mut s = Solo::installed_at(start);
    let d = drain(&mut s.conn);
    assert_eq!(
        d.deadline,
        Some(start + DEAD_TIMEOUT),
        "§7.4: a core that announces `None` here has disarmed the only \
         reaper there is"
    );
    let (owed, armed, last_send) = liveness_state(&s);
    assert!(armed, "§7.4's install pin arms the death deadline");
    assert!(
        !owed,
        "ruling 39: the dance must not bootstrap from the install"
    );
    assert_eq!(last_send, start, "§7.4 pins both clocks at the install");

    // And it really dies there — `None` arrives, but only after the reaping.
    s.conn.handle_timeout(start + DEAD_TIMEOUT);
    let d = drain(&mut s.conn);
    assert_eq!(
        d.closed(),
        Some(ConnectionLost::TimedOut),
        "§15.4's first row: a connection with no authenticated receive since \
         install dies in silence at 25 s"
    );
    assert_eq!(d.deadline, None, "a dead connection arms nothing");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. The passive keepalive — a reported reachability limit
// ═══════════════════════════════════════════════════════════════════════

/// **No test here covers `TimerKind::Keepalive` in its *held* state, and this
/// is the reason.** The admissible path is pinned by
/// [`an_admissible_passive_keepalive_fires_and_then_disarms`]; what is missing
/// is the refusal, and it is missing because it does not appear to exist.
///
/// `ADVERSARIAL-liveness.md` F1 names two *unbounded* variants, both of which
/// require §7.5's **passive** debt — `owes_passive_keepalive()` — to be set
/// at the instant of the refusal. On this author's arithmetic that state
/// cannot be built next to either disjunct of the refusal at the core, and
/// the obstruction is one inequality:
///
/// > The debt is set **only** by an authenticated fresh receive
/// > (`Liveness::on_authenticated_fresh_recv`). The same datagram credits
/// > §7.3's budget by `AMPLIFICATION_FACTOR × len`, and the smallest packet
/// > a peer can send is §3.4's empty plaintext at 30 bytes. So **every**
/// > receive that sets the debt also raises the room by at least
/// > `3 × 30 = 90` bytes — strictly more than the 30-byte keepalive
/// > `transmit_keepalive` asks for, and strictly more than the 31-byte probe
/// > whose refusal is the only thing that keeps a mark `Pending`.
///
/// Both disjuncts fall out of it:
///
/// * **amplification.** Room ≥ 90 > 30 the instant the debt is set. To
///   refuse the keepalive the room must be spent back below 30 *without*
///   clearing the debt — and only a **non-marking** send preserves it
///   (§7.4: `seal` clears the debt, `seal_quiet` does not). The quiet set is
///   retransmissions, credit frames, RESET_STREAM, pure ACKs and PTO probes.
///   A pure ACK is the only member that is also non-ack-eliciting, and it
///   costs ~35 bytes against the ≥ 90 its own trigger credited: the room
///   grows monotonically. Every other member sets `armed`, which puts the
///   connection back in the **bounded** case this file already tests.
/// * **contested.** A mark is `Pending` only while the budget refuses the
///   31-byte probe. The same ≥ 90 bytes of credit release it on the very
///   pump the receive triggers, so a pending mark and a fresh receive cannot
///   coexist. [`one_more_byte_of_room_releases_the_probe_and_the_isolation_with_it`]
///   pins that release directly.
///
/// **What I am *not* claiming.** A route through a *large* quiet
/// retransmission — enough unacked stream data that a PTO probe drains the
/// post-receive room below 30 in one packet — would reach
/// `owes_passive_keepalive() && room < 30` with `armed == true`. That is a
/// **bounded** spin (the death clock runs), it is the same defect this file
/// already pins through the beacon, and building it needs assumptions about
/// `Packing`'s budget clamp that a blind author would be guessing at. I have
/// left it out deliberately rather than ship a test whose fixture I cannot
/// verify — and flagged it, because if the implementer's `keepalive_can_leave`
/// is wired into the beacon's arm but not the passive one, nothing in this
/// file catches it.
///
/// **The consequence for the finding itself:** F1's *"nothing bounds the spin
/// at all"* rests on `armed == false`, and `armed` is false only immediately
/// after a receive — which is exactly the moment the room is largest. The
/// unbounded claim looks to me **overstated**; the bounded claim is exact and
/// is what this file tests. Reported, not resolved (working rule 3).
#[allow(dead_code)]
fn the_passive_form() {}

// ═══════════════════════════════════════════════════════════════════════
// 5. What makes suppression safe — the integrator's addition
// ═══════════════════════════════════════════════════════════════════════

/// A suppressed beacon **comes back** when the hold lifts.
///
/// **[Integrator, ruling 220.]** This author and the implementer closed F1
/// differently, both correctly: the author expected the beacon armed in the
/// future while held, the implementer suppresses it entirely. Suppression is
/// stronger — it makes the spin unreachable rather than survivable — but it
/// moves a burden the author's design did not carry. **An armed-in-the-future
/// beacon is self-healing; a suppressed one is only as good as whatever
/// re-arms it.**
///
/// So this is the assertion that the disagreement created, and neither blind
/// agent could have been asked for it: the author's design did not need it,
/// and the implementer had no test file to write it in.
///
/// Working rule 9: the degenerate build this separates is the one that
/// suppresses and **never re-arms** — a silently disabled keepalive on a
/// connection that has explicitly configured one. Every other assertion in
/// this file passes against that build, including all of section 2's, since
/// "no deadline in the past" is satisfied most easily by no deadline at all.
#[test]
fn the_beacon_returns_when_the_hold_lifts() {
    let start = t0();
    let mut s = responder_at(start);
    spend_leaving(&mut s, start, 0);
    s.conn
        .set_persistent_keepalive(start, Some(BEACON))
        .expect("§7.5's floor");
    let _ = drain(&mut s.conn);

    assert_eq!(
        s.conn.timer(TimerKind::PersistentKeepalive),
        None,
        "premise: at room 0 the beacon is suppressed, which is the state \
         whose exit this test is about"
    );
    assert_eq!(room(&s), 0, "premise: the hold really is on");

    // The hold lifts the only way it can: an authenticated, window-fresh
    // packet from the anchor credits `3 × len` (§7.3, ruling 169). This is
    // the same event `apply_live` re-syncs the liveness timers after, and
    // the whole safety of suppression rests on that ordering.
    let recv_at = start + Duration::from_millis(100);
    let _ = s.deliver(recv_at, &[]);

    assert!(
        room(&s) > 0,
        "premise: the receive credited the budget, so the beacon can leave \
         again — got room {}",
        room(&s)
    );
    assert!(
        s.conn.timer(TimerKind::PersistentKeepalive).is_some(),
        "a suppressed beacon must be re-armed by the event that lifts its \
         hold; without this, suppression is a silently disabled keepalive"
    );
    assert_not_retrospective(
        &drain(&mut s.conn),
        recv_at,
        "the beacon re-armed after the hold lifted",
    );
}

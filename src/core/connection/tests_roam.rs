//! Slice 7's **implementer** tests: §7.3's roaming and budget, §7.5's
//! keepalives and contested probe, driven by arithmetic on an `Instant`.
//!
//! Not the slice's acceptance criteria — those are two blind authors',
//! written from `SPEC.md` and `CONTRACT-7.md` against a tree they never see
//! (working rule 6). What these are for is the seams that have no other
//! in-file exercise: the roam predicate's conjuncts, the two `path_gen`
//! fences, the budget's hold-and-release, and the contested state's three
//! states with their two unstated exits.

use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::constants;
use crate::core::connection::mobility::Contested;
use crate::core::connection::testfix::{Pair, Solo, a_addr, b_addr, drain, put, t0, v4};
use crate::core::connection::timers::TimerKind;
use crate::core::connection::{ConnEvent, ConnOutput};
use crate::error::{ConfigError, ConnectionLost};

/// A **stable** origin instant.
///
/// `testfix::t0()` is `Instant::now()`, so it hands back a *different* value
/// on every call. Every deadline assertion in this file is an equality
/// against an arithmetic offset from one origin, and two origins microseconds
/// apart fail all of them — for a reason that has nothing to do with the
/// property under test. So the origin is drawn once.
fn origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(t0)
}

/// A third address, for the peer to move to.
fn c_addr() -> SocketAddr {
    v4(9, 41_000)
}

/// §8.4's ACK, as bytes. One block only — every use here acknowledges a
/// single counter, and a wider frame would assert nothing extra.
fn ack_frame(largest: u64) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, constants::FRAME_ACK);
    put(&mut out, largest);
    put(&mut out, 0); // ack_delay
    put(&mut out, 0); // range_count
    put(&mut out, 0); // first_range
    out
}

fn count_moved(d: &crate::core::connection::testfix::Drained) -> usize {
    d.count_events(|e| matches!(e, ConnEvent::AddressMoved { .. }))
}

fn count_contested(d: &crate::core::connection::testfix::Drained) -> usize {
    d.count_events(|e| matches!(e, ConnEvent::Contested))
}

fn count_cleared(d: &crate::core::connection::testfix::Drained) -> usize {
    d.count_events(|e| matches!(e, ConnEvent::ContestCleared))
}

// ═══════════════════════════════════════════════════════════════════════
// §7.5 — the persistent-keepalive band (rulings 40, 42, 44, 63)
// ═══════════════════════════════════════════════════════════════════════

/// Both boundaries, both directions. App. B names the two regressions this
/// exists to catch: *"a test that pins the old floor at `DEAD_TIMEOUT` is
/// the regression this obligation exists to catch, and a test asserting
/// that 1 s is rejected is the mirror regression."* Neither is written
/// here; both are asserted against.
#[test]
fn the_beacon_band_is_one_second_inclusive_to_dead_timeout_exclusive() {
    let mut solo = Solo::installed_at(origin());
    let now = origin();

    for rejected_low in [
        Duration::ZERO,
        Duration::from_millis(1),
        Duration::from_millis(500),
        Duration::from_millis(999),
    ] {
        assert_eq!(
            solo.conn.set_persistent_keepalive(now, Some(rejected_low)),
            Err(ConfigError::KeepaliveTooShort),
            "{rejected_low:?} is below the 1 s floor"
        );
    }

    assert_eq!(
        solo.conn
            .set_persistent_keepalive(now, Some(constants::PERSISTENT_KEEPALIVE_MIN)),
        Ok(()),
        "the floor is INCLUSIVE (ruling 42)"
    );
    assert_eq!(
        solo.conn.persistent_keepalive(),
        Some(Duration::from_secs(1))
    );

    for accepted in [
        Duration::from_millis(1_001),
        Duration::from_secs(10),
        constants::DEAD_TIMEOUT - Duration::from_millis(1),
    ] {
        assert_eq!(
            solo.conn.set_persistent_keepalive(now, Some(accepted)),
            Ok(())
        );
        assert_eq!(solo.conn.persistent_keepalive(), Some(accepted));
    }

    for rejected_high in [
        constants::DEAD_TIMEOUT,
        Duration::from_secs(30),
        Duration::MAX,
    ] {
        assert_eq!(
            solo.conn.set_persistent_keepalive(now, Some(rejected_high)),
            Err(ConfigError::KeepaliveTooLong),
            "{rejected_high:?} is at or above DEAD_TIMEOUT (ruling 40)"
        );
    }

    assert_eq!(solo.conn.set_persistent_keepalive(now, None), Ok(()));
    assert_eq!(solo.conn.persistent_keepalive(), None);
}

/// Ruling 44: a rejected call leaves the interval **unchanged** — no clamp.
/// A clamping build passes every "returns `Err`" assertion above and fails
/// this one.
#[test]
fn a_rejected_interval_leaves_the_previous_one_untouched() {
    let mut solo = Solo::installed_at(origin());
    let now = origin();

    solo.conn
        .set_persistent_keepalive(now, Some(Duration::from_secs(5)))
        .expect("5 s is inside the band");

    assert!(
        solo.conn
            .set_persistent_keepalive(now, Some(Duration::from_secs(60)))
            .is_err()
    );
    assert_eq!(
        solo.conn.persistent_keepalive(),
        Some(Duration::from_secs(5)),
        "no clamp to DEAD_TIMEOUT − ε"
    );

    assert!(
        solo.conn
            .set_persistent_keepalive(now, Some(Duration::from_millis(10)))
            .is_err()
    );
    assert_eq!(
        solo.conn.persistent_keepalive(),
        Some(Duration::from_secs(5)),
        "no clamp to the 1 s floor"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §7.5 — the passive dance and the beacon
// ═══════════════════════════════════════════════════════════════════════

/// §7.5's passive rule: `Keepalive` arms **iff `R > S`**. A connection with
/// no authenticated receive since install has `S == R` at the install, so it
/// arms nothing and emits nothing.
#[test]
fn the_passive_keepalive_arms_only_after_a_receive() {
    let mut solo = Solo::installed_at(origin());
    assert_eq!(
        solo.conn.timer(TimerKind::Keepalive),
        None,
        "S == R at install: R > S is false"
    );

    let now = origin() + Duration::from_secs(1);
    let _ = solo.deliver(now, &[constants::FRAME_PING as u8]);
    assert_eq!(
        solo.conn.timer(TimerKind::Keepalive),
        Some(origin() + constants::KEEPALIVE_TIMEOUT),
        "armed at S + KEEPALIVE_TIMEOUT, where S is still the install"
    );
}

/// The passive keepalive goes out at `S + KEEPALIVE_TIMEOUT` as §3.4's
/// empty plaintext, and the send disarms it — `R > S` no longer holds.
#[test]
fn the_passive_keepalive_sends_the_empty_plaintext_and_then_disarms() {
    let mut solo = Solo::installed_at(origin());
    let recv_at = origin() + Duration::from_secs(1);
    let _ = solo.deliver(recv_at, &[constants::FRAME_PING as u8]);

    let fires = origin() + constants::KEEPALIVE_TIMEOUT;
    solo.conn.handle_timeout(fires);
    let d = drain(&mut solo.conn);
    let transmits = d.transmits();
    assert_eq!(transmits.len(), 1, "one keepalive: {:?}", d.outs);
    assert_eq!(
        transmits[0].data.len(),
        constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN,
        "§3.4's empty plaintext is a 30-byte datagram"
    );
    assert_eq!(
        solo.conn.timer(TimerKind::Keepalive),
        None,
        "S has moved to `now`, so R > S is false again"
    );
}

/// **[ruling 182]** `S` counts **marking** sends only. A `seal_quiet` send —
/// here the ACK that the received PING owes — must neither advance `S` nor
/// suppress the keepalive.
///
/// The separating shape: a build reading §7.5's prose *"has not sent"* moves
/// `S` on the quiet ACK, and the keepalive deadline slides with it.
#[test]
fn a_quiet_send_neither_advances_s_nor_suppresses_the_keepalive() {
    let mut solo = Solo::installed_at(origin());
    let recv_at = origin() + Duration::from_secs(2);
    let d = solo.deliver(recv_at, &[constants::FRAME_PING as u8]);
    assert!(
        !d.transmits().is_empty(),
        "the PING owes an ACK, and §12.4 sends it"
    );
    assert_eq!(
        solo.conn.timer(TimerKind::Keepalive),
        Some(origin() + constants::KEEPALIVE_TIMEOUT),
        "the quiet ACK did not move S off the install instant"
    );
}

/// The beacon fires **unconditionally** — it does not consult `R` — and it
/// re-arms from the marking send it just performed.
#[test]
fn the_beacon_fires_without_a_receive_and_re_arms_itself() {
    let mut solo = Solo::installed_at(origin());
    solo.conn
        .set_persistent_keepalive(origin(), Some(Duration::from_secs(3)))
        .expect("3 s is inside the band");
    assert_eq!(
        solo.conn.timer(TimerKind::PersistentKeepalive),
        Some(origin() + Duration::from_secs(3))
    );

    let at = origin() + Duration::from_secs(3);
    solo.conn.handle_timeout(at);
    let d = drain(&mut solo.conn);
    assert_eq!(
        d.transmits().len(),
        1,
        "the beacon fires with no receive at all"
    );
    assert_eq!(
        solo.conn.timer(TimerKind::PersistentKeepalive),
        Some(at + Duration::from_secs(3)),
        "re-armed from the marking send it just made"
    );
}

/// Ruling 40: the beacon is a **marking** send, so it arms the death clock
/// rather than deferring it. A connection whose entire output is beacons
/// still dies at `R + DEAD_TIMEOUT`.
#[test]
fn a_beacon_only_connection_still_dies_at_the_dead_timeout() {
    let mut solo = Solo::installed_at(origin());
    solo.conn
        .set_persistent_keepalive(origin(), Some(Duration::from_secs(2)))
        .expect("2 s is inside the band");

    let mut beacons = 0usize;
    let mut death = None;
    for step in 1..=30u64 {
        let at = origin() + Duration::from_secs(step);
        solo.conn.handle_timeout(at);
        let d = drain(&mut solo.conn);
        beacons += d.transmits().len();
        if let Some(reason) = d.closed() {
            death = Some((at, reason));
            break;
        }
    }

    assert!(beacons >= 10, "the beacon did fire repeatedly: {beacons}");
    assert_eq!(
        death,
        Some((origin() + constants::DEAD_TIMEOUT, ConnectionLost::TimedOut)),
        "arming enables death, never defers it (ruling 40)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §7.3 — the roam predicate
// ═══════════════════════════════════════════════════════════════════════

/// All four conjuncts hold: an authenticated, window-fresh Data packet from
/// a new source re-homes the session.
#[test]
fn an_authenticated_fresh_packet_from_a_new_source_roams() {
    let mut solo = Solo::installed_at(origin());
    assert_eq!(solo.conn.remote_address(), Some(a_addr()));
    assert_eq!(solo.conn.path_generation(), 0);

    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &[constants::FRAME_PING as u8]);

    assert_eq!(solo.conn.remote_address(), Some(c_addr()), "the anchor moved");
    assert_eq!(solo.conn.path_generation(), 1, "one committed roam");
    assert_eq!(count_moved(&d), 1, "one AddressMoved: {:?}", d.outs);
    assert!(
        d.outs.iter().any(|o| matches!(
            o,
            ConnOutput::Event(ConnEvent::AddressMoved { from, to })
                if *from == a_addr() && *to == c_addr()
        )),
        "it carries the old and the new anchor: {:?}",
        d.outs
    );
    assert!(
        d.transmits().iter().all(|t| t.to == c_addr()),
        "everything after the roam goes to the new anchor"
    );
}

/// §3.4's keepalive is the smallest packet in the protocol and carries no
/// frames — S18's *"the mover must send, and the keepalive is what does
/// it"*. It must still roam, and its `AddressMoved` must reach the drain.
///
/// The separating shape: an `apply_live` that returns early for the empty
/// plaintext without draining leaves the event queued for ever.
#[test]
fn a_keepalive_from_a_new_source_roams_and_its_event_reaches_the_drain() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);
    let d = solo.deliver_from(now, c_addr(), &[]);

    assert_eq!(solo.conn.remote_address(), Some(c_addr()));
    assert_eq!(count_moved(&d), 1, "the event is in *this* drain: {:?}", d.outs);
}

/// The two negatives §7.3 names, each failing its own conjunct.
#[test]
fn neither_a_forgery_nor_a_replay_ever_moves_the_anchor() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    // Conjunct 2 fails: the AEAD tag does not verify.
    let mut forged = crate::core::connection::testfix::data_header(0xdead_beef, 0);
    forged.extend_from_slice(&[0u8; constants::AEAD_TAG_LEN]);
    solo.conn.handle_datagram(now, c_addr(), &forged);
    let _ = drain(&mut solo.conn);
    assert_eq!(
        solo.conn.remote_address(),
        Some(a_addr()),
        "unauthenticated bytes move nothing"
    );
    assert_eq!(solo.conn.path_generation(), 0);

    // Conjunct 3 fails: authenticated, but the window has already marked it.
    let dgram = solo.peer.seal(&[constants::FRAME_PING as u8]);
    solo.conn.handle_datagram(now, a_addr(), &dgram);
    let _ = drain(&mut solo.conn);
    solo.conn.handle_datagram(now, c_addr(), &dgram);
    let d = drain(&mut solo.conn);
    assert_eq!(
        solo.conn.remote_address(),
        Some(a_addr()),
        "§7.2: no replayed packet ever moves the endpoint"
    );
    assert_eq!(solo.conn.path_generation(), 0);
    assert_eq!(count_moved(&d), 0);
}

/// §15.2: a closing connection *"does not roam; never to the triggering
/// packet's source"*.
#[test]
fn a_closing_connection_does_not_roam() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);
    solo.conn.close(now, 0, b"");
    let _ = drain(&mut solo.conn);
    assert!(
        solo.conn.remote_address().is_some(),
        "the linger keeps the session"
    );

    let later = now + Duration::from_millis(10);
    let d = solo.deliver_from(later, c_addr(), &[constants::FRAME_PING as u8]);
    assert_eq!(
        solo.conn.remote_address(),
        Some(a_addr()),
        "the closing state keeps its anchor"
    );
    assert_eq!(solo.conn.path_generation(), 0);
    assert_eq!(count_moved(&d), 0);
    assert!(
        d.transmits().iter().all(|t| t.to == a_addr()),
        "§15.2's reply goes to the anchor, never to the triggering source"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §7.3 — the anti-amplification budget
// ═══════════════════════════════════════════════════════════════════════

/// A `connect()`-created connection starts **validated**; a roam arms the
/// budget, and the triggering packet is what credits it.
#[test]
fn a_dialled_connection_starts_validated_and_a_roam_arms_the_budget() {
    let mut solo = Solo::installed_at(origin());
    assert_eq!(solo.conn.amplification_budget(), None, "dialled ⇒ validated");

    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &[]);

    let (spent, credited) = solo
        .conn
        .amplification_budget()
        .expect("the roam arms the budget");
    assert_eq!(
        credited,
        (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64,
        "the triggering packet — and only it — credits the received counter"
    );
    assert!(
        spent <= constants::AMPLIFICATION_FACTOR * credited,
        "the gate held: {spent} sent against 3 × {credited}"
    );
}

/// An **accepted** connection is armed from its msg1 anchor (§5.6), which
/// is the second and last arming event.
#[test]
fn an_accepted_connection_is_armed_from_its_msg1_anchor() {
    let (_, sa, sb) = Solo::connecting();
    let conn = crate::core::Connection::established(
        origin(),
        [0x11u8; 32],
        sb,
        crate::core::Role::Responder,
    );
    let solo = Solo::around(conn, sa);

    assert_eq!(
        solo.conn.amplification_budget(),
        Some((0, constants::INIT_PACKET_LEN as u64)),
        "the msg1 credits the budget and nothing has been sent yet"
    );
    assert_eq!(
        solo.conn.validation_floor(),
        0,
        "the floor is the counter the next seal will use, and none has run"
    );
}

/// The gate **holds** output rather than dropping it, and a credited
/// receive releases it. The separating shape: a build that never gates at
/// all sends the whole write in the first pass.
#[test]
fn the_budget_holds_output_and_a_receive_releases_it() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);
    // Roam on the smallest possible packet: 30 bytes in, 90 bytes of budget.
    let _ = solo.deliver_from(now, c_addr(), &[]);

    let r = solo.conn.open(crate::core::Dir::Uni).expect("a uni stream");
    solo.conn
        .write(now, r, &[7u8; 4_000])
        .expect("the write is admitted into send state");
    solo.conn.flush(now);
    let d = drain(&mut solo.conn);
    let first: u64 = d.transmits().iter().map(|t| t.data.len() as u64).sum();
    assert!(
        first <= constants::AMPLIFICATION_FACTOR * 30,
        "held at 3 × 30 bytes, not sent whole: {first}"
    );

    // A credited receive from the new anchor raises the ceiling.
    let before = solo.conn.amplification_budget().expect("still unvalidated");
    let _ = solo.deliver_from(now, c_addr(), &[constants::FRAME_PADDING as u8; 200]);
    let after = solo.conn.amplification_budget();
    match after {
        Some((_, credited)) => assert!(
            credited > before.1,
            "an authenticated fresh packet credits the budget"
        ),
        // A packet carrying an ACK above the floor validates the address
        // outright (ruling 168), which is the stronger outcome.
        None => {}
    }
}

/// **[ruling 169]** A **replayed** packet authenticates and must still fund
/// nothing. The separating shape: crediting on `open` success alone rather
/// than on the window mark.
#[test]
fn a_replayed_packet_funds_no_budget() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(now, c_addr(), &[]);

    let dgram = solo.peer.seal(&[constants::FRAME_PING as u8]);
    solo.conn.handle_datagram(now, c_addr(), &dgram);
    let _ = drain(&mut solo.conn);
    let Some((_, credited)) = solo.conn.amplification_budget() else {
        // The PING's ACK validated the address; the replay below then has
        // nothing left to inflate, which is the same property more strongly.
        return;
    };

    solo.conn.handle_datagram(now, c_addr(), &dgram);
    let _ = drain(&mut solo.conn);
    assert_eq!(
        solo.conn.amplification_budget().map(|(_, r)| r),
        Some(credited),
        "a replay credits nothing"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §7.5 — the contested state
// ═══════════════════════════════════════════════════════════════════════

/// On a **validated** address the mark and the transmission are one
/// instant: the PING goes out, the deadline arms and `Contested` is emitted
/// together, and the `Transmit` precedes the `Event` (§8.1).
#[test]
fn a_mark_on_a_validated_address_transmits_at_once() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    let floor = solo.conn.next_counter().expect("established");
    solo.conn.mark_contested(now);
    let d = drain(&mut solo.conn);

    assert_eq!(
        solo.conn.contested(),
        Contested::Armed {
            floor,
            armed_at: now,
            deadline: now + constants::KEEPALIVE_TIMEOUT,
        }
    );
    assert_eq!(
        solo.conn.timer(TimerKind::Contested),
        Some(now + constants::KEEPALIVE_TIMEOUT)
    );
    assert_eq!(d.transmits().len(), 1, "one PING: {:?}", d.outs);
    assert_eq!(count_contested(&d), 1, "one Contested, at the transmission");
    let transmit = d
        .position(|o| matches!(o, ConnOutput::Transmit(_)))
        .expect("the PING");
    let event = d
        .position(|o| matches!(o, ConnOutput::Event(ConnEvent::Contested)))
        .expect("the event");
    assert!(transmit < event, "§8.1: the Transmit, then the Event");
    assert!(
        solo.conn.bytes_in_flight() > 0,
        "ruling 43: the probe is in the sent map and in bytes_in_flight"
    );
}

/// The collapse (ruling 41), and its **load-bearing** assertion: the death
/// instant does not move. A build that suppresses the second PING but
/// re-arms the deadline passes "no second PING" and fails this.
#[test]
fn a_second_mark_is_a_total_no_op_and_the_deadline_does_not_move() {
    let mut solo = Solo::installed_at(origin());
    let first = origin() + Duration::from_millis(10);
    solo.conn.mark_contested(first);
    let _ = drain(&mut solo.conn);
    let deadline = solo.conn.timer(TimerKind::Contested);
    assert!(deadline.is_some());

    for step in 1..=5u64 {
        let again = first + Duration::from_secs(step);
        solo.conn.mark_contested(again);
        let d = drain(&mut solo.conn);
        assert_eq!(d.transmits().len(), 0, "no second PING");
        assert_eq!(count_contested(&d), 0, "no second event");
        assert_eq!(
            solo.conn.timer(TimerKind::Contested),
            deadline,
            "the deadline is NOT re-armed — a security property, not an optimisation"
        );
    }
}

/// Ruling 179: a mark against a closing connection is a **total no-op**.
#[test]
fn a_mark_on_a_closing_connection_does_nothing() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);
    solo.conn.close(now, 0, b"");
    let _ = drain(&mut solo.conn);

    solo.conn.mark_contested(now + Duration::from_millis(1));
    let d = drain(&mut solo.conn);
    assert_eq!(solo.conn.contested(), Contested::No);
    assert_eq!(d.transmits().len(), 0);
    assert_eq!(count_contested(&d), 0);
    assert_eq!(solo.conn.timer(TimerKind::Contested), None);
}

/// The verdict: `TimedOut` — the same variant as liveness, **no new one** —
/// and **nothing is transmitted**. No third notification either.
#[test]
fn an_unanswered_probe_times_the_connection_out_and_sends_nothing() {
    let mut solo = Solo::installed_at(origin());
    let marked = origin() + Duration::from_millis(10);
    solo.conn.mark_contested(marked);
    let _ = drain(&mut solo.conn);

    let verdict = marked + constants::KEEPALIVE_TIMEOUT;
    solo.conn.handle_timeout(verdict);
    let d = drain(&mut solo.conn);
    assert_eq!(d.closed(), Some(ConnectionLost::TimedOut));
    assert_eq!(d.transmits().len(), 0, "§15.4: nothing at the verdict");
    assert_eq!(count_contested(&d), 0);
    assert_eq!(
        count_cleared(&d),
        0,
        "no third notification — the death arrives on closed()"
    );
}

/// The verdict lands **before** §7.4's own 25 s deadline, which is the
/// whole point of the probe: a connection whose peer has restarted is
/// reclaimed in `KEEPALIVE_TIMEOUT` rather than `DEAD_TIMEOUT`.
#[test]
fn the_contested_verdict_precedes_the_liveness_deadline() {
    let mut solo = Solo::installed_at(origin());
    let marked = origin() + Duration::from_millis(10);
    solo.conn.mark_contested(marked);
    let _ = drain(&mut solo.conn);

    let contested = solo.conn.timer(TimerKind::Contested).expect("armed");
    let liveness = solo.conn.timer(TimerKind::Liveness).expect("armed");
    assert!(
        contested < liveness,
        "the probe reclaims at {contested:?}, ahead of liveness at {liveness:?}"
    );
}

/// Ruling 41's high-water mark: **any** ACK covering **any** counter at or
/// above the floor clears the mark — not the probe's own packet. Here the
/// probe itself is dropped and an ordinary Data packet's ACK does it.
#[test]
fn any_ack_covering_the_floor_clears_the_mark() {
    let mut pair = Pair::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    pair.a.mark_contested(now);
    let _ = pair.drain_a();
    assert!(matches!(pair.a.contested(), Contested::Armed { .. }));

    // Drop everything A queued, including the probe. Ordinary application
    // data then carries the ACK that clears the mark.
    pair.a_to_b.clear();
    let later = now + Duration::from_millis(20);
    pair.a
        .send_datagram(later, b"data")
        .expect("a small datagram");
    let _ = pair.drain_a();
    let _ = pair.flush_a_to_b(later);
    let d = pair.flush_b_to_a(later + Duration::from_millis(20));

    assert_eq!(pair.a.contested(), Contested::No, "cleared by a later ACK");
    assert_eq!(pair.a.timer(TimerKind::Contested), None);
    assert_eq!(count_cleared(&d), 1, "one ContestCleared: {:?}", d.outs);
}

/// **[ruling 175]** No cooldown, and every re-mark records a **fresh**
/// floor. The separating shape: ruling 43's superseded "one probe per
/// `KEEPALIVE_TIMEOUT`" refuses the second mark outright.
#[test]
fn a_re_mark_after_a_clear_records_a_strictly_greater_floor() {
    let mut pair = Pair::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    pair.a.mark_contested(now);
    let _ = pair.drain_a();
    let first = pair.a.contested().floor().expect("marked");

    let later = now + Duration::from_millis(20);
    let _ = pair.flush_a_to_b(later);
    let _ = pair.flush_b_to_a(later + Duration::from_millis(20));
    assert_eq!(pair.a.contested(), Contested::No, "the ACK cleared it");

    let again = later + Duration::from_millis(50);
    pair.a.mark_contested(again);
    let d = pair.drain_a();
    let second = pair.a.contested().floor().expect("marked a second time");
    assert!(
        second > first,
        "a fresh floor: {second} must exceed {first} (ruling 175)"
    );
    assert_eq!(count_contested(&d), 1, "a full second mark, and a second probe");
}

// ═══════════════════════════════════════════════════════════════════════
// §13.6 / §14.6 — the roam fences
// ═══════════════════════════════════════════════════════════════════════

/// A roam resets the controller and **keeps** the sent map and
/// `bytes_in_flight` — §13.6's list, on both sides.
#[test]
fn a_roam_resets_the_controller_and_keeps_the_flight() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    let r = solo.conn.open(crate::core::Dir::Uni).expect("a uni stream");
    solo.conn.write(now, r, &[7u8; 800]).expect("a write");
    solo.conn.flush(now);
    let _ = drain(&mut solo.conn);
    let in_flight = solo.conn.bytes_in_flight();
    assert!(in_flight > 0, "something is in flight");

    let roam_at = now + Duration::from_millis(30);
    let _ = solo.deliver_from(roam_at, c_addr(), &[]);

    assert_eq!(
        solo.conn.congestion_window(),
        constants::INITIAL_WINDOW,
        "§14.6: cwnd resets to INITIAL_WINDOW"
    );
    assert_eq!(
        solo.conn.bytes_in_flight(),
        in_flight,
        "§13.6: the sent map is kept and bytes_in_flight is not reset"
    );
    assert_eq!(solo.conn.path_generation(), 1);
    assert_eq!(
        solo.conn.recovery().rtt().min_rtt(),
        None,
        "§13.1: min_rtt is re-seeded so it may rise"
    );
}

/// The `path_gen` fence on the RTT sample: an ACK for a **pre-roam** packet
/// takes no sample, so `smoothed_rtt` stays at its `K_INITIAL_RTT` seed.
///
/// The separating shape: without the fence the old path's round trip
/// becomes the new path's `min_rtt` floor, permanently.
#[test]
fn an_ack_for_a_pre_roam_packet_feeds_no_rtt_sample() {
    let mut solo = Solo::installed_at(origin());
    let now = origin() + Duration::from_millis(10);

    let r = solo.conn.open(crate::core::Dir::Uni).expect("a uni stream");
    solo.conn.write(now, r, &[7u8; 200]).expect("a write");
    solo.conn.flush(now);
    let d = drain(&mut solo.conn);
    assert_eq!(d.transmits().len(), 1);
    let sealed_counter = solo.conn.next_counter().expect("established") - 1;

    // Roam, then acknowledge the pre-roam packet from the new address.
    let roam_at = now + Duration::from_millis(30);
    let _ = solo.deliver_from(roam_at, c_addr(), &[]);
    assert_eq!(solo.conn.path_generation(), 1);

    let ack_at = roam_at + Duration::from_millis(40);
    let _ = solo.deliver_from(ack_at, c_addr(), &ack_frame(sealed_counter));

    assert_eq!(
        solo.conn.recovery().rtt().min_rtt(),
        None,
        "the pre-roam packet's round trip never became the new path's floor"
    );
    assert_eq!(
        solo.conn.smoothed_rtt(),
        constants::K_INITIAL_RTT,
        "and it never entered the estimator at all"
    );
    assert_eq!(solo.conn.bytes_in_flight(), 0, "but it did leave the flight");
}

/// A **post-roam** packet is not fenced: its ACK does take a sample. The
/// companion that makes the test above a fence rather than "sampling is
/// broken".
#[test]
fn an_ack_for_a_post_roam_packet_does_feed_the_estimator() {
    let mut solo = Solo::installed_at(origin());
    let roam_at = origin() + Duration::from_millis(10);
    let _ = solo.deliver_from(roam_at, c_addr(), &[]);

    // Credit the budget enough to let a data packet out.
    let sent_at = roam_at + Duration::from_millis(5);
    let _ = solo.deliver_from(sent_at, c_addr(), &[constants::FRAME_PADDING as u8; 600]);

    let r = solo.conn.open(crate::core::Dir::Uni).expect("a uni stream");
    solo.conn.write(sent_at, r, &[7u8; 100]).expect("a write");
    solo.conn.flush(sent_at);
    let d = drain(&mut solo.conn);
    assert_eq!(d.transmits().len(), 1, "the budget admitted it: {:?}", d.outs);
    let sealed_counter = solo.conn.next_counter().expect("established") - 1;

    let ack_at = sent_at + Duration::from_millis(40);
    let _ = solo.deliver_from(ack_at, c_addr(), &ack_frame(sealed_counter));

    assert_eq!(
        solo.conn.recovery().rtt().min_rtt(),
        Some(Duration::from_millis(40)),
        "a same-generation packet's round trip is a sample"
    );
}

#[allow(dead_code)]
fn _addresses_are_distinct() {
    assert_ne!(a_addr(), b_addr());
    assert_ne!(a_addr(), c_addr());
}

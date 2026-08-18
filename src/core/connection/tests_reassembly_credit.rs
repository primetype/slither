//! **Ruling 270 — the abuse half of the credit-derived hole ceiling.**
//!
//! Written from the ratified behaviour and round 42's committed evidence
//! (`.spec-v2-clean-slate/round42-C-reassembly-kill.md`) by an author who
//! never read the fix — CLAUDE.md working rule 6 — in a worktree cut at
//! `405796e`, verified as this file's first act. Its companion is
//! `tests/story_reassembly.rs`, which pins the survival half over a real
//! pair of endpoints on a paused clock; **neither file is a test of ruling
//! 270 without the other.**
//!
//! # Why this half needs the core
//!
//! The survival half is written at the public API on purpose: an
//! application cannot express anything but a conforming sender there. The
//! abuse half needs the opposite — one-byte STREAM frames at alternating
//! offsets, which no shipped verb emits — so it is written against
//! `Connection` directly, with frames built by hand.
//!
//! # The separation this file exists to state
//!
//! §10.6's mandate is *"per-stream reassembly state MUST be O(advertised
//! credit) and MUST NOT scale with the number of received frames"*, and the
//! amplification it names is *"one-byte STREAM frames at offsets 0, 2, 4, …
//! would store ~512 000 ranges within 1 MiB of credit"*. Ruling 270 makes
//! the ceiling scale with the credit; it does **not** repeal that argument.
//! So the two shapes must part company, and the arithmetic of why they can
//! is this:
//!
//! | shape, at an 8 MiB advertised stream window | most ranges it can store |
//! |---|---|
//! | full-size frames, every second datagram lost | `credit / (2 · MAX_DATAGRAM)` = **3 495** |
//! | one-byte frames at alternating offsets | `credit / 2` = **4 194 304** |
//!
//! A conforming sender cannot reach the second column: to hold `N` holes it
//! must have `2N` packets of offset space inside its credit, and its packets
//! are `MAX_DATAGRAM` bytes. That factor of **1 200** is the whole soundness
//! of a credit-derived ceiling, and every test here is placed inside it:
//! [`the_honest_hole_shape_survives_at_a_raised_window`] sits below the
//! honest bound, and [`a_tiny_fragment_flood_still_dies_at_a_raised_window`]
//! sits far above it while spending a fraction of the same credit.
//!
//! # The property, not the formula
//!
//! Ruling 270 does not publish `f`, and nothing here computes it. Every
//! bound below is derived from the credit and from `MAX_DATAGRAM`, and each
//! test's rustdoc states the margin it leaves.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here reads
//! a clock. Plain `#[test]`, no `sleep`.
//!
//! # For the integrator (working rules 6 and 15)
//!
//! This file has **no `mod` declaration** — adding
//! `mod tests_reassembly_credit;` to `src/core/connection/mod.rs` is the
//! integrator's job. It is also *not* `tests_reassembly.rs`, which the
//! brief named as a new file and which already exists at this base with a
//! live declaration and ceiling tests of its own; see `AUTHOR-REPORT.md`
//! §Conflicts C1.

#![allow(clippy::items_after_statements)]

use std::time::Instant;

use super::testfix::*;
use super::*;

use crate::constants::{
    INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA, MAX_DATAGRAM, PROTOCOL_VIOLATION,
    REASSEMBLY_CHUNKS_MAX,
};
use crate::core::{ConnSeed, FlowWindows};

// ═══════════════════════════════════════════════════════════════════════
// 1. Scaffolding
// ═══════════════════════════════════════════════════════════════════════

/// The raised stream window every abuse test here runs at: the window
/// round42-C measured the kill at.
const RAISED_STREAM: u64 = 8 * 1024 * 1024;

/// Its connection-level partner, as `Config::with_flow_windows` requires
/// (`stream <= connection`).
const RAISED_CONNECTION: u64 = 16 * 1024 * 1024;

/// The most discontiguous ranges a **conforming full-frame sender** can
/// leave inside `credit` bytes of offset space.
///
/// To hold `N` holes the sender must have covered `2N` packets of offset
/// space — `N` delivered and `N` missing — and its packets are at most
/// `MAX_DATAGRAM` bytes. This is the bound every honest workload here
/// stays under and every abuse workload here climbs far over; it is
/// arithmetic about the credit, not about the fix.
const fn honest_hole_bound(credit: u64) -> u64 {
    credit / (2 * MAX_DATAGRAM as u64)
}

/// A `Solo` whose receive halves advertise `stream` / `connection` instead
/// of §10.2's ratified constants.
///
/// `testfix`'s own `Solo::installed_at` goes through `[u8; 32]`, whose
/// `From` impl carries [`FlowWindows::default`] by construction (ruling
/// 259(viii) — *"a caller that says nothing about windows gets the
/// constants, and there is no third value the conversion could produce"*).
/// This is the same construction with the seed's other half supplied.
fn installed_with_windows(now: Instant, stream: u64, connection: u64) -> Solo {
    let (sa, sb) = handshake_pair();
    let mut conn = Connection::connecting(ConnSeed {
        sub_seed: [0xa5u8; 32],
        windows: FlowWindows { stream, connection },
    });
    conn.handle_endpoint_event(
        now,
        Install {
            session: sb,
            role: Role::Responder,
            anchor_from_msg1: false,
        },
    );
    let _ = drain(&mut conn);
    Solo::around(conn, sa)
}

/// One one-byte frame at offset `1 + 2k` — §10.6's own amplification
/// shape, *"one-byte STREAM frames at offsets 0, 2, 4, …"*, shifted by one
/// so no range ever sits at the read position and every stored range is
/// unambiguously discontiguous.
fn fragment(id: u64, k: u64) -> Vec<u8> {
    stream_frame(id, 1 + 2 * k, &[(k % 251) as u8], false)
}

/// Feed `fragment`s until the connection dies or `cap` is reached.
///
/// Returns `Some(k)` for the fragment count at which it died, `None` if it
/// survived the whole flood. Delivery stops at the death, so a build with a
/// low ceiling costs a few packets and a build with none costs the whole
/// cap.
///
/// `BATCH` is sized so one batch is **one packet**: at 9 bytes per frame,
/// 100 frames is 900 of `MAX_PLAINTEXT`'s 1 170, and `deliver_packed` emits
/// a single plaintext. That matters for the assertion, not the speed —
/// §15.3 answers *every* packet that arrives after the CLOSE with another
/// CLOSE, so a multi-packet batch straddling the death makes
/// [`assert_violation`]'s *"exactly one CLOSE"* fail on a correct build.
/// (It did, on the first run of this file: `left: [1, 1]`.)
fn flood_until_death(s: &mut Solo, now: Instant, id: u64, cap: u64) -> Option<u64> {
    const BATCH: u64 = 100;
    let mut k = 0;
    while k < cap {
        let n = BATCH.min(cap - k);
        let frames: Vec<Vec<u8>> = (k..k + n).map(|i| fragment(id, i)).collect();
        let d = s.deliver_packed(now, &frames);
        if d.closed().is_some() {
            let out = s.drain_frames(&d);
            assert_violation(&d, &out, PROTOCOL_VIOLATION);
            // One packet per batch, so the count is known to within 100;
            // every assertion here is about the order of magnitude.
            return Some(k + n);
        }
        k += n;
    }
    None
}

/// [`assert_alive`], with ruling 270's name on the failure.
///
/// The kill is the red this file's honest-shape test is written to produce
/// at base, so it is worth reporting as itself rather than as a generic
/// *"the connection must survive this"* — a red for any other reason is a
/// bug in the test, not the finding.
fn assert_survived_the_holes(d: &Drained) {
    if let Some(lost) = d.closed() {
        assert_eq!(
            lost,
            ConnectionLost::ProtocolViolation {
                code: PROTOCOL_VIOLATION
            },
            "died, but not of §10.6 — this is a fixture failure, not the finding"
        );
        panic!(
            "RULING 270: a hole pattern a conforming full-frame sender can \
             produce inside its advertised credit was answered with \
             CLOSE(PROTOCOL_VIOLATION)"
        );
    }
    assert_alive(d);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. The abuse side — GREEN at base, and it must stay green
// ═══════════════════════════════════════════════════════════════════════

/// **A tiny-fragment flood still dies, even at a raised window.**
///
/// This is the side that separates ruling 270 from *"delete the ceiling"*.
/// A fix that simply removes the check, or derives a ceiling from the
/// credit without dividing by a packet size, passes every survival test in
/// `tests/story_reassembly.rs` and fails here.
///
/// # The bound, and the margin it leaves
///
/// The flood stops at `CAP` = 65 536 fragments. Two independent readings
/// say that is unreachable by anything conforming, and both are stated in
/// terms of the credit rather than of the fix:
///
/// * it is **18.7×** [`honest_hole_bound`] at this window (3 495), so no
///   full-frame sender can produce it under any loss pattern; and
/// * it spends 131 072 bytes of offset space — **1.6 %** of the 8 MiB
///   credit — so the death cannot be §10.5's flow-control violation
///   wearing §10.6's clothes. [`assert_violation`] pins the code as
///   `PROTOCOL_VIOLATION` (`0x01`), not `FLOW_CONTROL_ERROR` (`0x02`).
///
/// A build that survives 65 536 disjoint one-byte ranges is holding
/// ~3 MiB of range metadata against 8 MiB of advertised credit, which is
/// the inflation §10.6 exists to refuse.
///
/// # BROKEN BUILD
///
/// * the ceiling deleted, or lifted whenever the window is raised — no
///   death, `None` returned;
/// * a ceiling derived as a fraction of the **credit in bytes** rather
///   than in packets — `credit / 2` is 4 194 304 here, so the flood
///   survives;
/// * a death carrying any code but `PROTOCOL_VIOLATION`, or more than one
///   CLOSE — [`assert_violation`]'s two assertions.
#[test]
fn a_tiny_fragment_flood_still_dies_at_a_raised_window() {
    let t = t0();
    let mut s = installed_with_windows(t, RAISED_STREAM, RAISED_CONNECTION);
    let id = Solo::peer_uni(0);

    const CAP: u64 = 65_536;
    const {
        assert!(
            CAP > 18 * honest_hole_bound(RAISED_STREAM),
            "or it proves nothing"
        )
    };

    let died = flood_until_death(&mut s, t, id, CAP);
    assert!(
        died.is_some(),
        "§10.6: {CAP} disjoint one-byte ranges — {}× what any full-frame \
         sender can leave inside this credit, and 1.6 % of the credit spent \
         — must still be a PROTOCOL_VIOLATION at an 8 MiB window",
        CAP / honest_hole_bound(RAISED_STREAM)
    );
}

/// **The same flood dies at the ratified default window.**
///
/// The floor's other side: whatever `f` is, `max(1024, f(256 KiB))` must
/// still kill an abuser at the window every unconfigured deployment runs.
///
/// # BROKEN BUILD
///
/// A ceiling that scales *down* as well as up would not fail this one — it
/// would fail it in the other direction, which is what
/// [`the_ceiling_floor_is_exactly_1024_at_the_default_window`] is for. This
/// test is the cheap guard that the default deployment did not simply lose
/// its ceiling along the way.
#[test]
fn a_tiny_fragment_flood_still_dies_at_the_default_window() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    // 32 768 fragments span 65 536 bytes — a quarter of the 256 KiB
    // credit, and 150× the 218 holes a full-frame sender can leave here.
    const CAP: u64 = 32_768;
    const { assert!(CAP > 100 * honest_hole_bound(INITIAL_MAX_STREAM_DATA)) };

    let died = flood_until_death(&mut s, t, id, CAP);
    assert!(
        died.is_some(),
        "§10.6 at the ratified default: {CAP} disjoint one-byte ranges must \
         be a PROTOCOL_VIOLATION"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. The floor — GREEN at base, and it must stay green
// ═══════════════════════════════════════════════════════════════════════

/// **`max(REASSEMBLY_CHUNKS_MAX, ·)` is a floor, and at the default window
/// it is the operative term** — 1 024 stored ranges are alive and the
/// 1 025th is a `PROTOCOL_VIOLATION`, exactly as before ruling 270.
///
/// Both sides are asserted, because a one-sided *"it eventually dies"*
/// passes a build that dies at 2, and a one-sided *"1 024 survive"* passes
/// a build with no ceiling at all.
///
/// # Why it is duplicated on purpose
///
/// `tests_streams.rs`'s
/// `exactly_1024_stored_ranges_survive_and_the_1025th_is_a_protocol_violation`
/// asserts the same boundary. That file belongs to another author under
/// working rule 6 and this one may not touch it; if ruling 270's
/// implementation edits it, this is the independent pin that survives the
/// edit. Stated rather than left to be discovered.
///
/// # BROKEN BUILD
///
/// A ceiling computed as `f(window)` with **no** `max(1024, ·)`. At
/// 256 KiB every credit-derived `f` this project has reason to write lands
/// *below* 1 024 — `credit / MAX_DATAGRAM` is 218 and
/// [`honest_hole_bound`] is 109 — so the 1 024-alive half is red and the
/// ratified default silently becomes **more** hostile than it is today.
/// That is the regression ruling 270's floor exists to forbid, and it is
/// invisible from the raised-window tests.
#[test]
fn the_ceiling_floor_is_exactly_1024_at_the_default_window() {
    let t = t0();
    let id = Solo::peer_uni(0);
    let max = REASSEMBLY_CHUNKS_MAX as u64;

    let mut s = Solo::installed_at(t);
    let frames: Vec<Vec<u8>> = (0..max).map(|k| fragment(id, k)).collect();
    let d = s.deliver_packed(t, &frames);
    assert_alive(&d);
    assert_eq!(
        REASSEMBLY_CHUNKS_MAX, 1024,
        "§10.6's ratified floor; a change here needs a ruling"
    );

    let d = s.deliver(t, &fragment(id, max));
    let out = s.drain_frames(&d);
    assert_violation(&d, &out, PROTOCOL_VIOLATION);
}

/// **The floor is a floor at a raised window too**: 1 024 ranges are alive
/// at 8 MiB, whatever `f` turns out to be.
///
/// The mirror of the test above, and the reason it is separate: a
/// credit-derived ceiling could in principle be *smaller* than 1 024 for
/// some window, and ruling 270 says it never is.
#[test]
fn the_floor_holds_at_a_raised_window_too() {
    let t = t0();
    let mut s = installed_with_windows(t, RAISED_STREAM, RAISED_CONNECTION);
    let id = Solo::peer_uni(0);

    let frames: Vec<Vec<u8>> = (0..REASSEMBLY_CHUNKS_MAX as u64)
        .map(|k| fragment(id, k))
        .collect();
    let d = s.deliver_packed(t, &frames);
    assert_alive(&d);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. The honest shape at the core level — RED at base
// ═══════════════════════════════════════════════════════════════════════

/// **The loss pattern a conforming sender produces survives at a raised
/// window** — the same property `tests/story_reassembly.rs` states over two
/// endpoints, isolated here from congestion control, recovery and the
/// clock.
///
/// Every second packet-sized slot of a 4.6 MB run is missing: 2 000 stored
/// ranges of `PACKET` bytes each, separated by `PACKET`-byte holes. That is
/// precisely what "every second datagram of one flight was lost" leaves in
/// a receive half, and it is what round42-C measured a real socket buffer
/// doing.
///
/// # The margins, both stated
///
/// * **Above the old ceiling**: 2 000 > 1 024, so the base build dies.
/// * **Below the honest bound**: [`honest_hole_bound`] at 8 MiB is 3 495,
///   so 2 000 leaves 43 % of headroom — this workload is reachable by a
///   conforming sender and does not sit on the boundary of any plausible
///   `f`.
/// * **Inside the credit**: the span is 4 640 000 bytes of the 8 388 608
///   advertised, and the bytes actually delivered are half of that.
///
/// # BROKEN BUILD
///
/// * **The base build.** Red at 1 025 stored ranges with
///   `ProtocolViolation { code: 1 }`.
/// * **A "fix" that stops storing ranges past the ceiling** instead of
///   killing — alive, and silently short of data. The
///   `reassembly_capacity()` assertion is what separates that from a real
///   fix: ruling 94's accessor reports the bytes actually held, so a build
///   that discarded the tail of the flood reports far less than the
///   2 320 000 bytes delivered.
#[test]
fn the_honest_hole_shape_survives_at_a_raised_window() {
    let t = t0();
    let mut s = installed_with_windows(t, RAISED_STREAM, RAISED_CONNECTION);
    let id = Solo::peer_uni(0);

    /// A full datagram's worth of stream payload.
    const PACKET: usize = 1_160;
    /// Ranges to leave. Above 1 024, below `honest_hole_bound(8 MiB)`.
    const HOLES: u64 = 2_000;
    const {
        assert!(
            HOLES > REASSEMBLY_CHUNKS_MAX as u64,
            "or the base build passes"
        );
        assert!(
            HOLES < honest_hole_bound(RAISED_STREAM),
            "or no conforming sender could produce this and it is an abuse test"
        );
        assert!(
            HOLES * 2 * PACKET as u64 <= RAISED_STREAM,
            "or the flow-control limit answers before §10.6 does"
        );
    }

    let payload = ramp(0, PACKET);
    let frames: Vec<Vec<u8>> = (0..HOLES)
        .map(|k| stream_frame(id, k * 2 * PACKET as u64, &payload, false))
        .collect();
    let d = s.deliver_packed(t, &frames);
    assert_survived_the_holes(&d);

    assert!(
        s.conn.reassembly_capacity() >= HOLES * PACKET as u64,
        "ruling 94: every delivered byte is still held — {} bytes for {} \
         ranges of {PACKET}",
        s.conn.reassembly_capacity(),
        HOLES
    );
}

/// The control for the test above (working rule 9): the **same** byte count
/// arriving **contiguously** is untroubled at base as well as after.
///
/// Without it, `the_honest_hole_shape_survives_at_a_raised_window` going
/// green would not distinguish "the ceiling scales" from "this build cannot
/// receive 2.3 MB on a raised-window stream at all".
#[test]
fn the_same_bytes_arriving_contiguously_are_untroubled() {
    let t = t0();
    let mut s = installed_with_windows(t, RAISED_STREAM, RAISED_CONNECTION);
    let id = Solo::peer_uni(0);

    const PACKET: usize = 1_160;
    const HOLES: u64 = 2_000;

    let payload = ramp(0, PACKET);
    let frames: Vec<Vec<u8>> = (0..HOLES)
        .map(|k| stream_frame(id, k * PACKET as u64, &payload, false))
        .collect();
    let d = s.deliver_packed(t, &frames);
    assert_alive(&d);

    let r = s.conn.accept(Dir::Uni).expect("§9.2: the peer opened it");
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got.len(),
        HOLES as usize * PACKET,
        "one contiguous run, coalesced to a single chunk"
    );
    // The connection window must admit it too, or this control is measuring
    // the wrong limit.
    const {
        assert!(HOLES * PACKET as u64 <= RAISED_CONNECTION);
        assert!(
            INITIAL_MAX_DATA < HOLES * PACKET as u64,
            "so the raise is load-bearing"
        );
    }
}

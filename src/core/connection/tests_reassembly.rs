//! Slice 7b's core-level coverage for the **reassembly re-coalescing
//! amplification** (F3).
//!
//! Written from `CONTRACT-7b.md` §5 and `ADVERSARIAL-liveness.md` F3 by an
//! author who never read the fix (CLAUDE.md working rule 6), in a worktree
//! cut at `c131904`.
//!
//! # The defect
//!
//! `Reassembly::insert` allocates `vec![0u8; span]` and copies the whole
//! merged span for **every** accepted STREAM frame, including one lying
//! wholly inside offset space it already holds. `check_stream` accepts such a
//! frame (`end <= high_water`) and charges `delta = 0`, so the peer picks
//! both the span and the rate for free. The fix is an early return before any
//! allocation when the arriving range is entirely covered by one stored
//! chunk.
//!
//! # The hard part: what separates the two builds
//!
//! Almost nothing does, and this is worth stating plainly before the tests.
//!
//! * **Chunk count** is the same — merging one chunk with a range inside it
//!   yields one chunk.
//! * **Byte content** is the same — the merge copies the stored bytes back
//!   over the arriving ones.
//! * **Flow credit** is the same — `CONTRACT-7b.md` §5 forbids touching
//!   `check_stream`, so a duplicate is free in **both** builds by design.
//! * **Wall-clock time** is the actual difference and is not assertable: it
//!   is flaky, and this project runs on a paused clock.
//!
//! So the naive test the contract sketches — *"assert the buffer's chunk
//! count and byte content are unchanged and that no read becomes
//! available"* — **passes the broken build**. It is a correctness test, and
//! the contract says so; it is not an amplification test. Working rule 9:
//! *a bound is only a test if the degenerate case violates it*, and every
//! one of those bounds is satisfied by the code that has the defect.
//!
//! # The separating quantity: `reassembly_capacity()` over a read buffer
//!
//! Ruling 94 already put a structural accounting hook on the core —
//! `Connection::reassembly_capacity()`, the sum of `Vec::capacity()` over
//! every stored chunk. It does **not** separate the builds in the attack's
//! own configuration, because the re-allocated span has the same capacity as
//! the chunk it replaced. It separates them as soon as the buffer holds
//! **slack**, and there is exactly one way to create slack:
//!
//! > `Reassembly::read` drains the front chunk with `Vec::drain(..n)`, which
//! > removes bytes and **does not shrink the allocation**. After reading `n`
//! > of `N` bytes the chunk holds `N - n` bytes in an `N`-byte allocation.
//!
//! In that state a covered-range insert is directly observable:
//!
//! | | after a covered 1-byte frame |
//! |---|---|
//! | **broken** — re-coalesces | `vec![0u8; N - n]`: capacity **collapses to the span** |
//! | **fixed** — early return | capacity **unchanged** |
//!
//! That assertion is not a proxy for the allocation. It *is* the allocation,
//! read back out of the core. The attack's own shape (nothing ever read, so
//! no slack) exercises the identical line of `insert` in a state where the
//! result happens to be invisible — so the test pins the mechanism where it
//! can be seen, which is the honest thing available.
//!
//! # What the rest of the file is for
//!
//! An early return that is **too eager** silently drops received bytes, and
//! that is a far worse defect than the one being fixed. §3 tests both
//! one-sided boundaries of the covered predicate — a frame overhanging the
//! chunk's end, and one overhanging its start — because slice 1 shipped a
//! boundary tested on one side only and this is the same shape.
//!
//! §4 pins the **load-bearing lemma** the fix rests on: `CONTRACT-7b.md` §5
//! argues that *"covered-by-the-union is exactly covered-by-one-chunk"*
//! because stored chunks are pairwise disjoint **and non-adjacent**. That is
//! a claim about `insert`'s own invariant, and if it ever stops holding the
//! one-chunk lookup is wrong. Working rule 11: a rationale must name a
//! mechanism that exists, so the mechanism gets a test.
//!
//! §5 pins the amplification's **fuel** — a wholly duplicate frame is legal
//! and costs the peer nothing — which is a characterisation of both builds
//! *and* a guard against the wrong fix, the one that rejects duplicates as a
//! protocol violation and kills connections over ordinary retransmission.
//!
//! # No clock, so no runtime
//!
//! Sans-io core tests: `now: Instant` is an argument and nothing here reads a
//! clock. Plain `#[test]`, no `sleep`.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::time::Instant;

use super::testfix::*;
use super::*;

use crate::constants::{INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA, REASSEMBLY_CHUNKS_MAX};

// ═══════════════════════════════════════════════════════════════════════
// 1. Scaffolding
// ═══════════════════════════════════════════════════════════════════════

/// The span the amplification tests build. Large enough that the collapse is
/// unmistakable, small enough that the test is a few hundred AEAD
/// operations.
const SPAN: usize = 16_384;

/// How much of [`SPAN`] is read out before the covered frame arrives. The
/// slack the read leaves behind — `SPAN - READ` bytes held in a `SPAN`-byte
/// allocation — is what makes the re-coalesce visible.
const READ: usize = 16_000;

/// A core with a peer-opened uni stream carrying `[0, SPAN)`, of which
/// `READ` bytes have been read out.
///
/// Returns the core and the claimed `StreamRef`. The core is
/// `Solo::installed_at`, i.e. §3.2's **validated** anchor: no amplification
/// budget is armed, so nothing here can be perturbed by §7.3.
fn buffered_with_slack(now: Instant) -> (Solo, StreamRef) {
    let mut s = Solo::installed_at(now);
    let id = Solo::peer_uni(0);

    let d = s.deliver_stream_bytes(now, id, 0, SPAN, false);
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2: the peer opened it");

    let got = read_exactly(&mut s.conn, now, r, READ);
    assert_eq!(got, ramp(0, READ), "§9.5 delivers the prefix in order");
    (s, r)
}

/// Total bytes of reassembly capacity across every receive half (ruling 94).
fn cap(s: &Solo) -> u64 {
    s.conn.reassembly_capacity()
}

/// Assert `insert` allocated nothing.
///
/// **This is F3's pin.** The two failure directions are different defects and
/// the message says which is which, because a bare `assert_eq!` on two
/// numbers is unreadable at 3 a.m.
#[track_caller]
fn assert_no_reallocation(s: &Solo, before: u64, what: &str) {
    let after = cap(s);
    assert!(
        after >= before,
        "{what}: reassembly capacity fell from {before} to {after}. A frame \
         wholly inside received offset space re-allocated and re-copied the \
         **whole merge span** — F3's amplification, measured. The collapse to \
         {after} is exactly the span of the chunk it rebuilt."
    );
    assert!(
        after <= before,
        "{what}: reassembly capacity grew from {before} to {after}. A frame \
         carrying no new bytes stored something, which is a different defect \
         from F3 and a worse one."
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. F3 — a covered frame must allocate nothing
// ═══════════════════════════════════════════════════════════════════════

/// The pin. A 1-byte STREAM frame at an offset inside the buffered span must
/// not touch the buffer's allocation.
///
/// Mutation caught: `Reassembly::insert` taking the merge branch for a
/// wholly-covered range — i.e. today's build. `SPAN - READ` = 384 bytes are
/// held in a 16 384-byte allocation, so a re-coalesce announces itself as a
/// capacity collapse from 16 384 to 384.
///
/// Not caught by, and deliberately not the only assertion: chunk count, byte
/// content, readability and flow credit, every one of which is identical in
/// both builds.
#[test]
fn a_frame_wholly_inside_the_buffer_allocates_nothing() {
    let t = t0();
    let (mut s, r) = buffered_with_slack(t);
    let id = Solo::peer_uni(0);

    let before = cap(&s);
    let held = (SPAN - READ) as u64;
    assert!(
        before > held,
        "fixture: the read must leave slack, or this test cannot see an \
         allocation at all — capacity {before}, held {held}. \
         `Vec::drain` does not shrink, so this holds by construction; if it \
         ever stops holding, the *fixture* is what broke."
    );

    // Interior: `READ <= offset` and `offset + 1 <= SPAN`, so the range is
    // wholly inside the stored chunk `[READ, SPAN)`. The same byte value as
    // before — §9.5 leaves a byte received twice with *differing* values to
    // the sender, and a test must not lean on which copy wins.
    let at = (READ + 100) as u64;
    let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 1), false));
    assert_alive(&d);

    assert_no_reallocation(&s, before, "one covered 1-byte frame");

    // …and the buffer is still correct, which the fix must not cost.
    let (rest, eof) = read_available(&mut s.conn, t, r);
    assert_eq!(
        rest,
        ramp(READ, SPAN - READ),
        "§9.5: the covered frame changed no byte"
    );
    assert!(!eof, "no FIN was ever sent");
}

/// The amplification, stated as the invariant it is: **an unbounded number of
/// covered frames does bounded work.**
///
/// Mutation caught: the same merge branch, on its first iteration. Note what
/// this test does *not* claim to catch — the broken build collapses the
/// capacity once and then re-allocates the same span on every subsequent
/// frame, so iterations 2..N are structurally indistinguishable. The pin is
/// "the insert allocates", and one insert is enough to see it; the loop is
/// here because the *statement* is about repetition and a reader should not
/// have to re-derive that.
#[test]
fn repeated_covered_frames_never_re_allocate_the_span() {
    let t = t0();
    let (mut s, r) = buffered_with_slack(t);
    let id = Solo::peer_uni(0);

    let before = cap(&s);
    // Every offset is interior, and they walk so that no single stored
    // position could be special-cased into passing.
    for i in 0..64u64 {
        let at = READ as u64 + 1 + (i * 5) % ((SPAN - READ) as u64 - 2);
        let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 1), false));
        assert_alive(&d);
        assert_no_reallocation(&s, before, &format!("covered frame #{i} at offset {at}"));
    }

    let (rest, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        rest,
        ramp(READ, SPAN - READ),
        "64 covered frames changed no byte"
    );
}

/// The attack's own configuration, asserted for what it *does* pin.
///
/// `ADVERSARIAL-liveness.md` F3's sequence: offsets `1..N` arrive with byte 0
/// withheld, so `read_offset` stays 0, nothing is ever readable, and the
/// application can never drain the buffer. Then 1-byte frames dribble in at
/// an interior offset.
///
/// **This test does not separate the builds and is not claimed to.** With
/// nothing ever read there is no slack, so the re-allocated span has the same
/// capacity as the chunk it replaced and `reassembly_capacity()` is blind to
/// it. What it pins is that the pinning hazard is real and that the covered
/// frames neither advance the reader nor corrupt the buffer — which is what
/// the fix must preserve. The measurement lives in
/// [`a_frame_wholly_inside_the_buffer_allocates_nothing`]; this is the
/// scenario, kept honest about which half it is.
#[test]
fn the_held_back_span_never_becomes_readable_and_covered_frames_do_not_change_it() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    // `1 .. SPAN`, byte 0 withheld.
    let d = s.deliver_stream_bytes(t, id, 1, SPAN - 1, false);
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2");

    let (got, eof) = read_available(&mut s.conn, t, r);
    assert!(
        got.is_empty(),
        "byte 0 is missing, so the contiguous prefix is empty and \
         `read_offset` stays 0 — forever"
    );
    assert!(!eof);
    let before = cap(&s);
    assert!(before > 0, "the held-back span has to live somewhere");

    for i in 0..64u64 {
        let d = s.deliver(t, &stream_frame(id, 5, &ramp(5, 1), false));
        assert_alive(&d);
        let (got, _) = read_available(&mut s.conn, t, r);
        assert!(got.is_empty(), "covered frame #{i} made nothing readable");
    }
    assert_eq!(
        cap(&s),
        before,
        "and the buffer is the same size it was — which is true of both \
         builds, and is why this test is not the measurement"
    );

    // The escape: byte 0 arrives and the whole span becomes readable at once.
    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 1), false));
    assert_alive(&d);
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, SPAN),
        "§9.5: the missing byte completes the prefix and 64 covered frames \
         left every other byte alone"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. The covered predicate's two boundaries — a too-eager fix loses data
// ═══════════════════════════════════════════════════════════════════════
//
// The early return must fire for `[o, e) ⊆ [c.offset, c.end())` and for
// nothing else. Two natural mis-statements each drop received bytes:
//
//   * `c.offset <= o && o < c.end()`  — right end unchecked;
//   * `c.offset <= o && e <= c.end()` written as `e <= c.end()` alone —
//     left end unchecked.
//
// Slice 1 shipped a boundary tested on one side only (`LEN` and `LEN-1`, not
// `LEN+1`). Both sides are here.

/// A core holding exactly one chunk `[100, 200)`, with `[0, 100)` withheld so
/// nothing can be read away and the chunk's edges stay where they are.
fn one_chunk_at_100(now: Instant) -> (Solo, StreamRef, u64) {
    let mut s = Solo::installed_at(now);
    let id = Solo::peer_uni(0);
    let d = s.deliver(now, &stream_frame(id, 100, &ramp(100, 100), false));
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2");
    (s, r, id)
}

/// A frame that starts inside the chunk and **overhangs its end by one byte**
/// is not covered, and its last byte must be stored.
///
/// Mutation caught: an early return keyed on the start offset alone. Under it
/// byte 200 is silently discarded, the stream is short by one byte forever,
/// and — because `high_water` was raised to 201 by `check_stream` — a FIN at
/// 201 would make the stream unreadable to its end. Data loss, from the fix
/// for a CPU cost.
#[test]
fn a_frame_overhanging_the_chunks_end_is_stored() {
    let t = t0();
    let (mut s, r, id) = one_chunk_at_100(t);

    let d = s.deliver(t, &stream_frame(id, 150, &ramp(150, 51), false));
    assert_alive(&d);

    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), false));
    assert_alive(&d);

    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, 201),
        "the overhanging byte at offset 200 is new and must be kept — 201 \
         bytes, not 200"
    );
}

/// A frame that ends inside the chunk and **starts one byte before it** is
/// not covered, and its first byte must be stored.
///
/// Mutation caught: an early return keyed on the end offset alone. Under it
/// byte 99 is discarded; the reader then stops at 99 even after `[0, 99)`
/// arrives, and the hole is invisible until the application notices its
/// stream has stalled with the peer certain it sent everything.
#[test]
fn a_frame_overhanging_the_chunks_start_is_stored() {
    let t = t0();
    let (mut s, r, id) = one_chunk_at_100(t);

    let d = s.deliver(t, &stream_frame(id, 99, &ramp(99, 51), false));
    assert_alive(&d);

    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 99), false));
    assert_alive(&d);

    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, 200),
        "the overhanging byte at offset 99 is new and must be kept — the \
         prefix runs to 200, not to 99"
    );
}

/// The positive case at both edges at once: a frame **exactly equal** to the
/// stored chunk is covered.
///
/// This is the value the two tests above bracket. Without it they would be
/// satisfied by a fix that never early-returns at all — which is the current
/// build, and is the degenerate case working rule 9 asks about. With it, the
/// three together fix the predicate to `[100, 200)` and nothing wider.
#[test]
fn a_frame_exactly_equal_to_the_chunk_is_covered_and_changes_nothing() {
    let t = t0();
    let (mut s, r, id) = one_chunk_at_100(t);
    let before = cap(&s);

    let d = s.deliver(t, &stream_frame(id, 100, &ramp(100, 100), false));
    assert_alive(&d);
    assert_eq!(
        cap(&s),
        before,
        "an exactly-equal frame is wholly covered: nothing new, nothing \
         stored, nothing re-allocated"
    );

    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), false));
    assert_alive(&d);
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(got, ramp(0, 200), "and the bytes are all still there");
}

/// A frame **adjacent** to the chunk, touching neither of its bytes, is not
/// covered and must be stored.
///
/// The third boundary, and the one the merge loop is most likely to confuse
/// with the second: `insert`'s span walk merges chunks that are adjacent
/// (`chunks[hi].offset <= end`), so "touching" and "covered" are one
/// comparison apart in the same function.
#[test]
fn a_frame_adjacent_to_the_chunk_is_stored() {
    let t = t0();
    let (mut s, r, id) = one_chunk_at_100(t);

    // Butts against the end: `[200, 201)`.
    let d = s.deliver(t, &stream_frame(id, 200, &ramp(200, 1), false));
    assert_alive(&d);
    // Butts against the start: `[99, 100)`.
    let d = s.deliver(t, &stream_frame(id, 99, &ramp(99, 1), false));
    assert_alive(&d);

    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 99), false));
    assert_alive(&d);
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, 201),
        "both adjacent bytes are new: the prefix runs 0..201"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. The load-bearing lemma — stored chunks are disjoint AND non-adjacent
// ═══════════════════════════════════════════════════════════════════════

/// `CONTRACT-7b.md` §5: *"a range covered by the union of two or more stored
/// chunks would require them to be adjacent or overlapping, which the
/// invariant forbids. Therefore covered-by-the-union is exactly
/// covered-by-one-chunk, and the check is a single lookup."*
///
/// The lemma is only as good as the invariant, so the invariant gets a test.
/// 1 025 adjacent one-byte frames arrive with byte 0 withheld, so nothing is
/// ever read away and nothing is ever popped: a build that coalesces adjacent
/// ranges holds **one** chunk, and a build that does not holds 1 025 and
/// trips §10.6's `REASSEMBLY_CHUNKS_MAX` (1 024) — which is a connection
/// **death**, and therefore observable without any chunk-count accessor.
///
/// Mutation caught: any change to the span walk's `<=` that stops merging
/// touching ranges. Under it the single-chunk lookup the fix performs is
/// unsound — a range spanning two stored chunks would be covered by their
/// union and covered by neither alone, so the fix would re-coalesce anyway
/// and F3 would quietly come back for exactly the shapes that matter.
#[test]
fn adjacent_ranges_coalesce_so_covered_by_the_union_is_covered_by_one_chunk() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    let n = REASSEMBLY_CHUNKS_MAX as u64 + 1;
    let frames: Vec<Vec<u8>> = (1..=n)
        .map(|o| stream_frame(id, o, &ramp(o as usize, 1), false))
        .collect();
    let d = s.deliver_packed(t, &frames);
    assert_alive(&d);

    let r = s.conn.accept(Dir::Uni).expect("§9.2");
    let (got, _) = read_available(&mut s.conn, t, r);
    assert!(got.is_empty(), "byte 0 is still missing");

    // The proof that they really did coalesce: the missing byte completes the
    // whole run in one go.
    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 1), false));
    assert_alive(&d);
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, (n + 1) as usize),
        "one contiguous run, 0..{}",
        n + 1
    );
}

/// The other half of the invariant: ranges with a **gap** between them stay
/// separate, and the gap is real.
///
/// Mutation caught: a span walk that merged across a hole. It would fabricate
/// the missing bytes as zeroes and hand them to the application as received
/// data — silent corruption, and it would also make the fix's covered check
/// return `true` for ranges the peer never sent.
#[test]
fn gapped_ranges_stay_separate_so_the_hole_is_real() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    let d = s.deliver(t, &stream_frame(id, 10, &ramp(10, 10), false));
    assert_alive(&d);
    // A hole at `[20, 30)`.
    let d = s.deliver(t, &stream_frame(id, 30, &ramp(30, 10), false));
    assert_alive(&d);

    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 10), false));
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2");
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got,
        ramp(0, 20),
        "the prefix stops at the hole: 20 bytes, not 40 with ten zeroes in \
         the middle"
    );

    let d = s.deliver(t, &stream_frame(id, 20, &ramp(20, 10), false));
    assert_alive(&d);
    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(got, ramp(20, 20), "the hole filled, the rest followed");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. The fuel — a covered frame is legal and costs the peer nothing
// ═══════════════════════════════════════════════════════════════════════

/// A wholly-duplicate frame charges **zero** flow credit, at volume.
///
/// **This does not separate the F3 builds and is not claimed to** — the
/// contract forbids touching `check_stream`, so the charge is 0 in both. It
/// separates the *wrong* fix: one that rejects a fully-duplicate frame as a
/// protocol violation, which `CONTRACT-7b.md` §5 rules out because it *"would
/// kill connections over ordinary retransmission"*. That build dies on the
/// first repeat here.
///
/// It is also the amplification's fuel, measured: **1 024 re-deliveries of
/// one 1 KiB range** — 1.2 MiB of accepted STREAM traffic, past the 1 MiB
/// connection window — pass without a `FlowControl` violation, because only
/// the first 1 KiB was ever new. A build computing `delta` as the frame's
/// length rather than `end - high_water` is dead well before the last one.
/// That is what "invisible to flow control" means, stated as a test rather
/// than as a sentence in a review.
///
/// The unique span is deliberately one frame rather than the finding's
/// 256 KiB: the property is about the *charge*, which is per frame, and a
/// 256 KiB span would make the broken build re-coalesce a quarter of a
/// megabyte a thousand times over for no extra assertion.
#[test]
fn duplicate_frames_are_legal_and_charge_no_flow_credit() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    const UNIQUE: usize = 1024;
    const REPEATS: u64 = 1200;

    let payload = ramp(0, UNIQUE);
    let d = s.deliver(t, &stream_frame(id, 0, &payload, false));
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2");

    let total = (REPEATS + 1) * UNIQUE as u64;
    assert!(
        total > INITIAL_MAX_DATA,
        "the volume has to exceed the connection window, or a build that \
         charged duplicates would survive this test: {total} vs \
         {INITIAL_MAX_DATA}"
    );
    assert!(
        UNIQUE as u64 <= INITIAL_MAX_STREAM_DATA,
        "…while the unique span stays inside the per-stream window, so the \
         only limit a charging build can trip is the connection one"
    );

    // Each packet is freshly sealed by the peer, so these are genuine
    // duplicates at the **stream** layer and not replays at §7.2's — a
    // re-sent datagram would be dropped by the replay window and would prove
    // nothing about flow control.
    for i in 0..REPEATS {
        let d = s.deliver(t, &stream_frame(id, 0, &payload, false));
        assert_alive(&d);
        assert!(
            d.closed().is_none(),
            "duplicate #{i} must not be a violation: §9.5 makes a re-sent \
             range legal, and ordinary retransmission produces it"
        );
    }

    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got, payload,
        "each byte delivered exactly once, 1 201 arrivals notwithstanding"
    );
}

/// The last covered-frame boundary the fix must not swallow: a frame wholly
/// **below `read_offset`**.
///
/// `insert` already returns early for these (`skip >= data.len()`), before any
/// of the fix's new machinery. The test exists because the fix inserts its
/// check right after that skip, and the two returns are one edit apart: a fix
/// that reorders them so the covered check runs on the *unskipped* offset
/// would compare a range that no longer describes the arriving bytes.
///
/// Mutation caught: a covered check placed before the `read_offset` skip.
#[test]
fn a_frame_below_the_read_offset_is_dropped_without_disturbing_the_buffer() {
    let t = t0();
    let (mut s, r) = buffered_with_slack(t);
    let id = Solo::peer_uni(0);
    let before = cap(&s);

    // Wholly below `read_offset == READ`.
    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 64), false));
    assert_alive(&d);
    assert_eq!(cap(&s), before, "already-read bytes are never re-stored");

    // Straddling `read_offset`: half already read, half still buffered — so
    // the arriving range after the skip is `[READ, READ + 32)`, which **is**
    // covered, and still nothing may be allocated.
    let at = (READ - 32) as u64;
    let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 64), false));
    assert_alive(&d);
    assert_no_reallocation(&s, before, "a frame straddling the read offset");

    let (rest, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        rest,
        ramp(READ, SPAN - READ),
        "and the tail is untouched by either"
    );
}

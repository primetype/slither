//! Slice 7b's core-level coverage for the **reassembly re-coalescing
//! amplification** (F3), plus ruling 253's **work** and **capacity** bounds.
//!
//! Written from `CONTRACT-7b.md` §5 and `ADVERSARIAL-liveness.md` F3 by an
//! author who never read the fix (CLAUDE.md working rule 6), in a worktree
//! cut at `c131904`; extended for ruling 253 by a second author blind to
//! *that* implementation, in a worktree cut at `9c557ce`.
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
//! * **Byte content** is the same — for a range whose bytes match what is
//!   stored, which is what every test here sends. §9.5 lets the receiver keep
//!   **either** copy of a byte received twice with differing values (ruling
//!   253 relaxed `recv.rs`'s old *"stored bytes win"* claim, because
//!   small-to-large can invert which copy survives), so no test here may lean
//!   on which one wins, and none does.
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
//! # The separating quantity: `reassembly_copy_work()`
//!
//! **[Re-derived 2026/08/17 — ruling 253 [S-77].]** This section used to
//! nominate `reassembly_capacity()`. The argument was that a covered insert
//! re-allocates `vec![0u8; span]`, so a chunk holding **slack** — `Vec::drain`
//! removes bytes without shrinking the allocation, so after reading `n` of
//! `N` bytes the chunk holds `N - n` bytes in an `N`-byte buffer — announces
//! the re-coalesce as a capacity *collapse* from `N` to `N - n`.
//!
//! That argument is a property of `vec![0u8; span]`, **the exact line ruling
//! 253 replaces**, and it does not survive the replacement. Under a
//! small-to-large merge the arriving side is copied into the stored chunk's
//! existing allocation, so a build that **lost** F3's early return would
//! re-copy bytes and leave the capacity *unchanged*. A separator derived
//! against the code being deleted is not a separator; this is why the ruling
//! makes re-deriving it an obligation rather than an assumption.
//!
//! The re-derived quantity is the one ruling 253 states its bound over:
//! `Connection::reassembly_copy_work()`, the monotone total of bytes written
//! into chunk storage by `insert` — the arriving frame's bytes on store, plus
//! every stored byte re-copied during a merge, never reset. F3's early return
//! happens **before** any of that, so a covered frame's contribution is
//! exactly zero:
//!
//! | build | `copy_work` delta, one covered frame over a stored `span` | `capacity` delta |
//! |---|---|---|
//! | **F3 present**, either merge | **0** | 0 |
//! | **F3 lost**, pre-253 whole-span merge | `span + 1` | 0, or a collapse if the chunk held slack |
//! | **F3 lost**, small-to-large merge | ≥ 1 (the arriving bytes, written over the stored ones) | **0** |
//!
//! The third row is why the assertion is `== 0` and not `< span`. Once the
//! merge goes small-to-large a lost early return is *cheap* — one frame's
//! worth of copying, not a span's — so any bound phrased in spans is
//! satisfied by the broken build for free, which is working rule 9's trap
//! exactly. Zero is what the fix promises and zero is what separates it from
//! both broken merges.
//!
//! `copy_work` is also a **strictly** better instrument than capacity was: it
//! separates in the attack's own configuration — byte 0 held back, nothing
//! ever read, no slack anywhere — where capacity was blind under *both*
//! merges and the test had to be contorted into reading first to see
//! anything.
//!
//! `reassembly_capacity()` is still asserted alongside it. It is no longer
//! the separator, but "a covered frame allocates nothing" is a true invariant
//! and a cheap one to keep, and §7 below uses capacity for what it *is* the
//! instrument for: ruling 253(ii)'s ceiling.
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
//! §6 and §7 are ruling 253's two bounds, and they are about the case F3
//! deliberately left alone. F3 closed the **zero**-progress insert; the
//! ruling's finding is that the *one*-byte-of-progress insert — a frame
//! bridging two stored chunks — was still worth ~916× its wire bytes to a
//! peer, because the pre-253 merge rebuilt the whole merged span for it.
//! §6 asserts the work bound over an alternating-bridging workload; §7
//! asserts that the fix does not pay for it with headroom the §10.6 ceiling
//! forbids.
//!
//! # What was measured rather than reasoned about
//!
//! `reassembly_copy_work()` does not exist on the base this file was written
//! against, so every number here would otherwise have been a derivation about
//! a state machine — which this project has lost to the build repeatedly. The
//! contract's accounting was therefore added to the **pre-253** merge in a
//! throwaway build, the file run against it, and the instrumentation reverted
//! before anything was committed. What that run established:
//!
//! * §6's pre-253 total is **548 750 144 B**, agreeing with the
//!   hand-derivation to the byte, and §6 is the *only* test that fails on the
//!   old merge — which is what it means for the other twelve to be pins on
//!   behaviour that already ships rather than new-behaviour reds.
//! * §7 **passes** on the pre-253 merge at exactly 262 000 bytes of capacity,
//!   as it must: `vec![0u8; span]` allocates the arrived span and nothing
//!   more. §7 is aimed at the fix, not at the defect.
//! * Both new workloads stay inside flow control and inside
//!   `REASSEMBLY_CHUNKS_MAX`, and both read back every byte they sent — so
//!   nothing in either is measuring an accident.
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

/// Ruling 253's accounting hook: the monotone total of bytes written into
/// chunk storage by `insert` — the arriving frame's bytes on store, plus
/// every stored byte re-copied during a merge. Never reset, so every
/// assertion here is over a *delta* or over a fresh core's total.
///
/// **A scope question the contract leaves open, reported rather than
/// assumed.** `reassembly_capacity()` sums over the *live* receive halves,
/// which is right for capacity — an abandoned half's allocation really is
/// gone. Summed the same way, copy **work** is not monotone: retiring a half
/// would subtract its history, and a peer could reset the accounting by
/// opening and abandoning streams. Nothing here depends on the answer (no
/// test in this file abandons or resets a half while measuring), so the tests
/// hold either way — but "never reset" and "sum the live halves" cannot both
/// be true, and the integrator picks one.
fn work(s: &Solo) -> u64 {
    s.conn.reassembly_copy_work()
}

/// Assert a covered frame did **nothing** — no copying, no allocation.
///
/// **This is F3's pin, re-derived under ruling 253.** The copy-work half is
/// the separator (see the module header's table: a lost early return is
/// non-zero under either merge, and cheap under the new one, so `== 0` is the
/// only phrasing that separates). The capacity half is the older assertion,
/// kept because "allocated nothing" stays true and stays worth pinning — but
/// it is no longer claimed to separate anything on its own.
///
/// The three failure directions are different defects and the messages say
/// which is which, because a bare `assert_eq!` on two numbers is unreadable
/// at 3 a.m.
#[track_caller]
fn assert_covered_frame_did_nothing(s: &Solo, before_work: u64, before_cap: u64, what: &str) {
    let after_work = work(s);
    assert_eq!(
        after_work,
        before_work,
        "{what}: reassembly copy work rose from {before_work} to \
         {after_work}. A frame lying wholly inside already-received offset \
         space took the merge path — F3's early return is gone, and with it \
         the guarantee that every byte this function copies is paid for by a \
         byte that is new to the buffer. {} bytes were copied for zero \
         progress.",
        after_work - before_work
    );

    let after_cap = cap(s);
    assert!(
        after_cap >= before_cap,
        "{what}: reassembly capacity fell from {before_cap} to {after_cap}. \
         A frame carrying no new bytes re-allocated the chunk it landed in — \
         the pre-253 whole-span merge's signature, and the collapse to \
         {after_cap} is exactly the span it rebuilt."
    );
    assert!(
        after_cap <= before_cap,
        "{what}: reassembly capacity grew from {before_cap} to {after_cap}. \
         A frame carrying no new bytes stored something, which is a different \
         defect from F3 and a worse one."
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. F3 — a covered frame must allocate nothing
// ═══════════════════════════════════════════════════════════════════════

/// The pin. A 1-byte STREAM frame at an offset inside the buffered span must
/// do no copying and touch no allocation.
///
/// Mutation caught: `Reassembly::insert` taking the merge branch for a
/// wholly-covered range. Under the **pre-253** merge that costs `span + 1`
/// bytes of copy work and, because this fixture leaves slack, also announces
/// itself as a capacity collapse from 16 384 to 384. Under the
/// **small-to-large** merge it costs 1 byte of copy work and the capacity
/// never moves — which is why the pin is `copy_work` at `== 0` and the
/// capacity assertion rides along rather than leading.
///
/// Not caught by, and deliberately not the only assertion: chunk count, byte
/// content, readability and flow credit, every one of which is identical in
/// every build.
#[test]
fn a_frame_wholly_inside_the_buffer_allocates_nothing() {
    let t = t0();
    let (mut s, r) = buffered_with_slack(t);
    let id = Solo::peer_uni(0);

    let before_cap = cap(&s);
    let before_work = work(&s);
    // No fixture precondition on slack any more. The old separator needed
    // `capacity > SPAN - READ` to be visible at all, and asserted it here so
    // the test could not pass vacuously; ruling 253 makes shrink-at-quiescence
    // an admissible mechanism, so slack is no longer guaranteed to survive
    // the read — and `copy_work` does not need it.

    // Interior: `READ <= offset` and `offset + 1 <= SPAN`, so the range is
    // wholly inside the stored chunk `[READ, SPAN)`. The same byte value as
    // before — §9.5 leaves a byte received twice with *differing* values to
    // the receiver's choice, and a test must not lean on which copy wins.
    let at = (READ + 100) as u64;
    let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 1), false));
    assert_alive(&d);

    assert_covered_frame_did_nothing(&s, before_work, before_cap, "one covered 1-byte frame");

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
/// covered frames does exactly zero work.**
///
/// Mutation caught: the same merge branch, on every iteration. Under the old
/// capacity separator this loop was decoration — the broken build collapsed
/// the capacity once and then re-allocated the same span on every subsequent
/// frame, so iterations 2..N were structurally indistinguishable and the
/// doc comment said so. `copy_work` is **cumulative**, so the loop now
/// carries its own weight: 64 covered frames add 64 separate contributions
/// to a total that must not move at all, and a build that leaked even one
/// byte per frame is caught 64 times over instead of once.
#[test]
fn repeated_covered_frames_never_re_allocate_the_span() {
    let t = t0();
    let (mut s, r) = buffered_with_slack(t);
    let id = Solo::peer_uni(0);

    let before_cap = cap(&s);
    let before_work = work(&s);
    // Every offset is interior, and they walk so that no single stored
    // position could be special-cased into passing.
    for i in 0..64u64 {
        let at = READ as u64 + 1 + (i * 5) % ((SPAN - READ) as u64 - 2);
        let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 1), false));
        assert_alive(&d);
        assert_covered_frame_did_nothing(
            &s,
            before_work,
            before_cap,
            &format!("covered frame #{i} at offset {at}"),
        );
    }

    let (rest, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        rest,
        ramp(READ, SPAN - READ),
        "64 covered frames changed no byte"
    );
}

/// The attack's own configuration — **and, since ruling 253, the measurement
/// as well.**
///
/// `ADVERSARIAL-liveness.md` F3's sequence: offsets `1..N` arrive with byte 0
/// withheld, so `read_offset` stays 0, nothing is ever readable, and the
/// application can never drain the buffer. Then 1-byte frames dribble in at
/// an interior offset.
///
/// This doc comment used to open *"this test does not separate the builds and
/// is not claimed to"*, and the reason was the old separator: with nothing
/// ever read there is no slack, so the re-allocated span had the same capacity
/// as the chunk it replaced and `reassembly_capacity()` was blind to it. Every
/// other F3 test therefore had to read 16 000 bytes out first — manufacturing
/// a state the attacker never produces — to make the defect visible at all.
///
/// `reassembly_copy_work()` is blind to nothing. In **exactly** the attack's
/// configuration, with no read and no slack, 64 covered frames must add zero;
/// a pre-253 build without the early return adds `64 × (SPAN + 1)` ≈ 1.05 MB
/// for 64 wire bytes, which is F3 measured where F3 actually lives. That is
/// the sharpening ruling 253 [S-77] asked for, and it is the reason the
/// re-derivation was worth doing rather than re-asserting the old table.
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
    let before_cap = cap(&s);
    let before_work = work(&s);
    assert!(before_cap > 0, "the held-back span has to live somewhere");

    for i in 0..64u64 {
        let d = s.deliver(t, &stream_frame(id, 5, &ramp(5, 1), false));
        assert_alive(&d);
        assert_covered_frame_did_nothing(
            &s,
            before_work,
            before_cap,
            &format!("covered frame #{i}, in the attack's own configuration"),
        );
        let (got, _) = read_available(&mut s.conn, t, r);
        assert!(got.is_empty(), "covered frame #{i} made nothing readable");
    }
    assert_eq!(
        work(&s),
        before_work,
        "64 covered frames, no slack anywhere, zero bytes copied — the \
         separator the capacity signature could not express here"
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
/// satisfied by a fix that never early-returns at all, which is the
/// degenerate case working rule 9 asks about. With it, the three together fix
/// the predicate to `[100, 200)` and nothing wider.
///
/// **The capacity assertion here never did that job**, and ruling 253 is what
/// made it visible: a frame exactly equal to the stored chunk merges into a
/// span of exactly the chunk's size, so `vec![0u8; 100]` replaces a 100-byte
/// allocation and the capacity does not move. The build with no early return
/// passed this line. `copy_work` is what fixes it — 0 against the pre-253
/// merge's 200.
#[test]
fn a_frame_exactly_equal_to_the_chunk_is_covered_and_changes_nothing() {
    let t = t0();
    let (mut s, r, id) = one_chunk_at_100(t);
    let before_cap = cap(&s);
    let before_work = work(&s);

    let d = s.deliver(t, &stream_frame(id, 100, &ramp(100, 100), false));
    assert_alive(&d);
    assert_covered_frame_did_nothing(
        &s,
        before_work,
        before_cap,
        "an exactly-equal frame is wholly covered: nothing new, nothing \
         stored, nothing copied, nothing re-allocated",
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

    // Ruling 253's instrument on the same volume, and here it is **exact**:
    // the first frame stores 1 024 bytes and every one of the 1 200 repeats
    // is wholly covered, so the connection's lifetime copy total is 1 024 and
    // not one byte more. A build without F3's early return reaches
    // 1 024 + 1 200 × 2 048 = 2 458 624 under the pre-253 merge, and
    // 1 024 + 1 200 × 1 024 = 1 229 824 under a small-to-large one — the
    // second is the row of the header table that no span-shaped bound would
    // have caught.
    assert_eq!(
        work(&s),
        UNIQUE as u64,
        "1 200 duplicate frames cost the receiver nothing to store, which is \
         the other half of costing the sender nothing to send"
    );

    let (got, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        got, payload,
        "each byte delivered exactly once, 1 201 arrivals notwithstanding"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Ruling 253's work bound — O(credit · log credit), from the separating
//    side
// ═══════════════════════════════════════════════════════════════════════
//
// §10.6, amended: *"Coalesce-on-insert's total copy work per stream MUST be
// O(that stream's advertised credit · log credit) — every stored byte is
// copied O(log) times across its lifetime (the small-to-large discipline),
// never once per bridging frame."*
//
// F3 closed the **zero**-progress insert and said so in as many words: a
// frame bridging two stored chunks *"makes progress, is bounded by credit,
// and is left alone"*. Ruling 253's finding is that "bounded by credit" is a
// bound on the **number** of such frames and says nothing about what each one
// costs, and the gap between those two is worth ~916× a peer's wire bytes.
//
// The workload below is that gap, driven: one large stored run, and then a
// peer that parks a single byte two positions past the run's end and sends
// the byte that closes the hole. Each pair is two bytes on the wire and one
// merge, and the pre-253 merge rebuilds the entire run for it.

/// The per-stream credit `C` the bound is stated over (§10.6).
const CREDIT: u64 = INITIAL_MAX_STREAM_DATA;

/// The run the bridging frames extend, and the number of bridging rounds.
///
/// `BRIDGE_BASE + 2 × BRIDGE_ROUNDS` = 139 072, comfortably inside [`CREDIT`]
/// so the workload never touches flow control, and inside
/// [`INITIAL_MAX_DATA`] so it never touches the connection ledger either. At
/// most **two** chunks are stored at any instant, so
/// [`REASSEMBLY_CHUNKS_MAX`] is never in play: this is a pure work test and
/// nothing else may be what fails it.
const BRIDGE_BASE: usize = 131_072;
/// See [`BRIDGE_BASE`].
const BRIDGE_ROUNDS: u64 = 4_000;

/// §10.6's work bound as an integer: `ceil(0.95 · C · log₂ C)`.
///
/// `C` is 2¹⁸, so `log₂ C` is exactly 18 and the logarithm rounds nowhere;
/// the 0.95 is carried as `95/100` with a ceiling division so the test does
/// not depend on float behaviour either. The value is 4 482 663.
fn work_bound() -> u64 {
    (95 * CREDIT * u64::from(CREDIT.ilog2())).div_ceil(100)
}

/// **The obligation.** An alternating-bridging workload's total copy work
/// stays inside `0.95 · C · log₂ C`.
///
/// # What each broken build does
///
/// * **Pre-253 whole-span merge — measured, not estimated.** Building the run
///   costs `Σ 1024·(j+1)` = 8 454 144 B, because `deliver_stream_bytes`
///   arrives in 1 KiB frames and each one rebuilds the whole run so far: the
///   bound is already blown by 1.9× before a single bridging round. Then each
///   round rebuilds the run again for its 2 wire bytes,
///   `Σ (131 072 + 2k + 2)` = 540 292 000 B, plus 4 000 for parking the
///   bytes. **Total 548 750 144 B for 139 072 B of wire** — 122× this bound
///   and ~3 946× per wire byte, the same lever the audit measured at 916×
///   sustained.
///
///   That figure is this test's own output, not arithmetic: the pre-253
///   merge was instrumented with the contract's accounting in a throwaway
///   build, run, and reverted before committing. It agreed with the
///   hand-derivation to the byte, which is the only reason the derivation
///   above is quoted at all.
/// * **Small-to-large merge (the fix).** The arriving side of every merge is
///   1 byte and the parked chunk is 1 byte, so a round copies ~3 bytes.
///   Total ≈ the arrived bytes plus whatever the growth policy re-copies as
///   the run's allocation grows — a few hundred KB, an order of magnitude
///   under the bound. The margin is deliberate: the accessor's contract does
///   not say whether a reallocation *inside* the large chunk counts as copy
///   work, and the test must pass either way.
/// * **A build that refuses the work.** Dropping or rejecting the bridging
///   frames satisfies an upper bound for free — working rule 9's degenerate
///   case, and the reason for the two assertions after the bound.
#[test]
fn alternating_bridging_inserts_stay_inside_the_work_bound() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    assert_eq!(work(&s), 0, "a fresh core has copied nothing");

    // One large stored run, `[0, BRIDGE_BASE)`. Nothing is ever read, so it
    // stays exactly where it is for the whole workload.
    let d = s.deliver_stream_bytes(t, id, 0, BRIDGE_BASE, false);
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2: the peer opened it");

    // The lever. Round `k` finds the run at `[0, base + 2k)`. It parks one
    // byte at `base + 2k + 1` — leaving a one-byte hole, so the parked chunk
    // is **non-adjacent** and stays separate — and then sends the byte at
    // `base + 2k` that closes the hole. That one is adjacent to both, so it
    // is a bridging merge over a run that grows every round.
    let base = BRIDGE_BASE as u64;
    let mut frames: Vec<Vec<u8>> = Vec::with_capacity(2 * BRIDGE_ROUNDS as usize);
    for k in 0..BRIDGE_ROUNDS {
        let parked = base + 2 * k + 1;
        let bridge = base + 2 * k;
        frames.push(stream_frame(id, parked, &ramp(parked as usize, 1), false));
        frames.push(stream_frame(id, bridge, &ramp(bridge as usize, 1), false));
    }
    // Packed, or 8 000 packets is 8 000 AEAD operations for 8 000 bytes.
    let d = s.deliver_packed(t, &frames);
    assert_alive(&d);

    let span = base + 2 * BRIDGE_ROUNDS;
    assert!(
        span <= CREDIT && span <= INITIAL_MAX_DATA,
        "the workload must stay inside both windows or flow control, not the \
         work bound, is what this test measures: {span} against {CREDIT} and \
         {INITIAL_MAX_DATA}"
    );

    let bound = work_bound();
    let done = work(&s);
    assert!(
        done <= bound,
        "§10.6's work bound is breached: {done} bytes copied against a \
         ceiling of {bound} (= ceil(0.95 · {CREDIT} · {})), for {span} bytes \
         of wire. The merge is rebuilding the whole span per bridging frame \
         rather than copying the small side into the large — the pre-253 \
         shape, which reaches ~549 MB here.",
        CREDIT.ilog2()
    );

    // Working rule 9, the other side: an upper bound is free to a build that
    // does nothing. Two independent statements that the work really happened.
    assert!(
        done >= span,
        "only {done} bytes were ever copied for {span} bytes of arriving \
         data. Every arriving byte is written into chunk storage once when \
         it is stored, so this build is losing data, not saving work — and \
         it would have satisfied the bound above for free."
    );
    let (got, eof) = read_available(&mut s.conn, t, r);
    assert!(!eof, "no FIN was ever sent");
    assert_eq!(
        got,
        ramp(0, span as usize),
        "the bridging actually bridged: {span} contiguous bytes from offset \
         0. A build that dropped the bridging frames stops at \
         {BRIDGE_BASE} and would otherwise have passed the bound by refusing \
         the work it was being asked to bound."
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Ruling 253's capacity ceiling — the fix may not be bought with headroom
// ═══════════════════════════════════════════════════════════════════════
//
// §10.6, amended: *"allocated **capacity** stays ≈ the arrived span, its
// per-stream ceiling ≈ the advertised credit; shrink-at-quiescence and capped
// growth both qualify …, a bare doubling policy holding ~1.5 × credit does
// not"*.
//
// This is the bill for §6. A small-to-large merge only avoids re-copying the
// large side if it can **reuse** the large side's allocation, which means
// growth-amortised buffers, which means slack — and slack is allocation ahead
// of arrival, the exact thing ruling 94 forbade and the exact thing
// `reassembly_capacity()` was built to observe. §6 and §7 are a matched pair:
// either alone is passed by a build that fails the other.

/// The ceiling workload's frame size. **Deliberately not a power of two**,
/// and the test's separating power depends on it — see
/// [`heavy_merging_to_the_credit_limit_holds_no_growth_headroom`].
const GROW_FRAME: usize = 1_000;
/// 262 × [`GROW_FRAME`]: the largest multiple of it inside [`CREDIT`], so the
/// arrived span sits 144 bytes under the per-stream window.
const GROW_BLOCKS: usize = 262;

/// **The ceiling.** After a heavily-merging workload that fills the per-stream
/// window, allocated capacity is the arrived span and not a growth policy's
/// headroom.
///
/// # Why the numbers are what they are
///
/// The workload arrives 262 000 bytes in 1 000-byte blocks: every *even*
/// block first (131 separate chunks, each with a 1 000-byte hole after it),
/// then every *odd* block in ascending order, each of which bridges the
/// growing run to the next stored chunk. 131 bridging merges, ending in one
/// chunk of 262 000 bytes — heavy merging, and the growth path a
/// small-to-large merge has to walk.
///
/// The assertion is `262 000 ≤ capacity ≤ 262 144`, and each side is doing
/// work:
///
/// * **Lower** — the chunk holds 262 000 bytes, so anything less means bytes
///   were lost. Without it a build storing nothing passes the ceiling for
///   free (working rule 9).
/// * **Upper** — [`CREDIT`], ruling 253(ii)'s per-stream ceiling, *not*
///   relaxed to the measured 1.49 × credit.
///
/// # What each broken build does
///
/// * **A bare doubling policy.** Every chunk in this workload is a multiple
///   of 1 000 bytes, and `1000 · m = 2ᵏ` has no integer solution (2ᵏ is never
///   divisible by 125), so no doubling sequence present can land on 2¹⁸ —
///   whatever base it starts from it overshoots the window. From a
///   1 000-byte base it holds 512 000 (1.95 × credit); from the 3 000-byte
///   first merge, 384 000 (1.46 × credit, which is the ruling's measured
///   figure to within a rounding). This is why `GROW_FRAME` is 1 000 and not
///   1 024: at 1 024 a doubling policy lands on exactly 262 144, passes, and
///   the test asserts nothing.
/// * **An eager allocator** (ruling 94's defect, §10.6's own worked example)
///   sits at the window from the first byte. The two snapshots before the
///   merging phase catch it: one chunk after one block, 131 after 131.
/// * **The pre-253 whole-span merge** passes this test, and is meant to —
///   `vec![0u8; span]` is exactly the arrived span, and it was measured here
///   at exactly 262 000. §7 is not aimed at it; §7 is aimed at what §6 tempts
///   a fix into doing.
#[test]
fn heavy_merging_to_the_credit_limit_holds_no_growth_headroom() {
    let t = t0();
    let mut s = Solo::installed_at(t);
    let id = Solo::peer_uni(0);

    let block = |i: usize| -> Vec<u8> {
        let at = (i * GROW_FRAME) as u64;
        stream_frame(id, at, &ramp(at as usize, GROW_FRAME), false)
    };
    let span = (GROW_BLOCKS * GROW_FRAME) as u64;
    assert!(
        span <= CREDIT,
        "the workload must fit the per-stream window: {span} against {CREDIT}"
    );

    // One block, so the first snapshot is over a single disjoint insert.
    let d = s.deliver(t, &block(0));
    assert_alive(&d);
    let r = s.conn.accept(Dir::Uni).expect("§9.2: the peer opened it");
    assert_eq!(
        cap(&s),
        GROW_FRAME as u64,
        "ruling 94: a disjoint insert allocates the arriving bytes and \
         nothing else. An allocator that reserves the {CREDIT}-byte window \
         per receive half turns 128 peer-opened streams into 32 MiB — §10.6's \
         own worked example of the amplification it exists to close."
    );

    // The rest of the even blocks — 2, 4, … 260 — so 130 more chunks, each
    // separated from its neighbour by a 1 000-byte hole and therefore
    // non-adjacent: none of them coalesces with anything.
    let evens: Vec<Vec<u8>> = (1..GROW_BLOCKS / 2).map(|j| block(2 * j)).collect();
    assert_eq!(evens.len(), 130, "blocks 2, 4, … 260");
    let d = s.deliver_packed(t, &evens);
    assert_alive(&d);
    let stored = ((evens.len() + 1) * GROW_FRAME) as u64;
    assert_eq!(
        cap(&s),
        stored,
        "still nothing but what arrived: {} disjoint chunks of {GROW_FRAME}",
        evens.len() + 1
    );

    // Now the odd blocks — 1, 3, … 261 — ascending. Each one fills the hole
    // between the run and the next stored chunk, so each is a bridging merge
    // over a run that grows every time: the growth path a small-to-large
    // merge has to walk, and the one a doubling policy overshoots.
    let odds: Vec<Vec<u8>> = (0..GROW_BLOCKS / 2).map(|j| block(2 * j + 1)).collect();
    assert_eq!(odds.len(), 131, "blocks 1, 3, … 261");
    let d = s.deliver_packed(t, &odds);
    assert_alive(&d);

    let held = cap(&s);
    assert!(
        held >= span,
        "the buffer holds {span} bytes but reports {held} of capacity — bytes \
         went missing, and a build that stores nothing satisfies any ceiling"
    );
    assert!(
        held <= CREDIT,
        "reassembly capacity is {held} against §10.6's per-stream ceiling of \
         {CREDIT}, for an arrived span of {span}. That is {}% of credit: a \
         growth policy is holding headroom the ceiling does not fund. Ruling \
         253(ii) admits shrink-at-quiescence and capped growth and refuses a \
         bare doubling policy at ~1.5 × credit — the ceiling is not relaxed \
         to the measured 1.49 ×.",
        held * 100 / CREDIT
    );

    // …and the span really did coalesce into one readable run, so none of the
    // above was measured over a buffer that quietly dropped a block.
    let (got, eof) = read_available(&mut s.conn, t, r);
    assert!(!eof, "no FIN was ever sent");
    assert_eq!(
        got,
        ramp(0, span as usize),
        "262 blocks, 131 bridging merges, one contiguous run of {span} bytes"
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
    let before_cap = cap(&s);
    let before_work = work(&s);

    // Wholly below `read_offset == READ`.
    let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 64), false));
    assert_alive(&d);
    assert_covered_frame_did_nothing(
        &s,
        before_work,
        before_cap,
        "a frame wholly below the read offset: already-read bytes are never \
         re-stored",
    );

    // Straddling `read_offset`: half already read, half still buffered — so
    // the arriving range after the skip is `[READ, READ + 32)`, which **is**
    // covered, and still nothing may be copied or allocated.
    let at = (READ - 32) as u64;
    let d = s.deliver(t, &stream_frame(id, at, &ramp(at as usize, 64), false));
    assert_alive(&d);
    assert_covered_frame_did_nothing(
        &s,
        before_work,
        before_cap,
        "a frame straddling the read offset",
    );

    let (rest, _) = read_available(&mut s.conn, t, r);
    assert_eq!(
        rest,
        ramp(READ, SPAN - READ),
        "and the tail is untouched by either"
    );
}

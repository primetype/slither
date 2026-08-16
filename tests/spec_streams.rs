//! **§16.2's stream obligations, from outside the crate.**
//!
//! Three groups, all of them reachable through the public API only:
//!
//! 1. **Drop semantics** for both half-handles (§16.2:4395–4403, rulings 93
//!    and 115, `CONTRACT-4b.md` §9).
//! 2. **Two-sided boundaries** on the handle surface — the `Ok(0)` /
//!    `Ok(Some(0))` / `Pending` / `Ok(None)` rows that `CONTRACT-4b.md`
//!    §3 and §5 write down precisely because slice 4a's test author
//!    guessed them wrong, plus the after-close row of §8.
//! 3. **Cancel-safety** of the four new `async fn`s: *a dropped future has
//!    claimed nothing* (§16.2's standard, set for `notified()`).
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 4b** from `SPEC.md` §9,
//! §10, §16.2, §16.8, §16.9 and `.slices/04-streams/CONTRACT-4b.md`
//! **alone**, while the implementer wrote `src/shell/` concurrently. No
//! file under `src/shell/` and no line of `src/testutil/mod.rs` was read.
//!
//! # The one test this file exists for
//!
//! [`dropping_a_finished_send_stream_still_delivers_the_end_of_stream`].
//! `SendHalf::reset` early-returns only when a reset already exists — a
//! set `fin` does **not** stop it — so the obvious `Drop` ("reset
//! unconditionally, the core is idempotent") resets every *finished*
//! stream, destroys the FIN and the unsent buffer, and turns the peer's
//! clean end-of-stream into `Reset(0)`. That build passes every test that
//! reads before the sender drops.
//!
//! The author paired it with a positive control asserting that an explicit
//! `reset()` after `finish()` still reaches the peer. **That control is
//! unreachable in slice 4** and the pairing does not hold: §9.6 makes a
//! RESET_STREAM for an already-FIN-complete receive half *a valid no-op
//! when the final sizes agree*, and in slice 4 they always agree — ruling
//! 111 pins `final_size` at the highest byte actually transmitted, and
//! with no congestion control a `write()` never leaves accepted bytes
//! unsealed. The test now pins §9.6's no-op instead
//! ([`a_reset_after_a_delivered_fin_is_a_no_op`]), and
//! [`dropping_an_unfinished_send_stream_resets_it_with_code_zero`] is what
//! stops a `Drop` that never resets anything. The reachable positive
//! control moves to slice 5.
//!
//! # Not written here, and why
//!
//! * **Frame-level counts.** `Tap::datagrams()` yields sealed datagrams —
//!   AEAD ciphertext after establishment — so no integration test can
//!   count STREAM, RESET_STREAM or MAX_STREAM_DATA frames. Ruling 123
//!   moves those two assertions to 4a's in-crate file.
//! * **`WriteError::Reset`.** Unreachable in slice 4 (`CONTRACT-4b` §3):
//!   a locally reset half reports `Finished`, and §9.9's STOP_SENDING is
//!   deferred, so no peer can reset our send half. Asserting `Finished`
//!   *is* the test that a build has not fabricated a path to it.
//! * **`id()` returning `None`.** Ruling 116 removed the documented cause;
//!   a handle the application holds always answers `Some`.
//! * **An exhausted bidi space.** `open_bi` parks for ever in slice 4, so
//!   nothing here awaits it; every bidi test stays far under
//!   `INITIAL_MAX_STREAMS_BIDI` (32). The `open_uni` equivalent *is*
//!   reachable and is tested.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use slither::constants::{INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA, INITIAL_MAX_STREAMS_UNI};
use slither::testutil::{Pair, TestBiStream, TestRecvStream, TestSendStream, local, settle};
use slither::{ConnectionLost, ReadError, WriteError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE — identical in intent to the block in `tests/story_streams.rs`,
// duplicated because each file in `tests/` is its own crate and this
// author owns no shared module.
// ══════════════════════════════════════════════════════════════════════

const PATIENCE: Duration = Duration::from_secs(5);
const NOT_BEFORE: Duration = Duration::from_millis(200);

async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: byte count differs");
    if let Some(i) = got.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: first differing byte at offset {i}: got {:#04x}, want {:#04x}",
            got[i], want[i]
        );
    }
}

async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(n >= 1, "{what}: `Ok(0)` means only that `buf` was empty");
        done += n;
    }
}

async fn read_to_end(r: &mut TestRecvStream, what: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 4096];
    loop {
        match within(r.read(&mut buf), what).await {
            Ok(Some(n)) => {
                assert!(
                    n >= 1,
                    "{what}: `Ok(Some(0))` means only that `buf` was empty"
                );
                out.extend_from_slice(&buf[..n]);
            }
            Ok(None) => break,
            Err(e) => panic!("{what}: expected a clean end of stream, got {e:?}"),
        }
    }
    out
}

async fn drain_exactly(r: &mut TestRecvStream, n: usize, what: &str) {
    let mut done = 0usize;
    while done < n {
        let want = (n - done).min(8192);
        let mut buf = vec![0u8; want];
        match within(r.read(&mut buf), what).await {
            Ok(Some(k)) => done += k,
            other => panic!("{what}: wanted {n} bytes, got {other:?} after {done}"),
        }
    }
}

async fn fill_until_blocked(s: &mut TestSendStream, buf: &[u8], what: &str) -> (usize, bool) {
    let mut done = 0usize;
    loop {
        if done == buf.len() {
            return (done, false);
        }
        match tokio::time::timeout(NOT_BEFORE, s.write(&buf[done..])).await {
            Ok(Ok(n)) => {
                assert!(
                    n >= 1,
                    "{what}: a blocked write is `Pending`, never `Ok(0)`"
                );
                done += n;
            }
            Ok(Err(e)) => panic!("{what}: write failed with {e:?}"),
            Err(_) => return (done, true),
        }
    }
}

/// Read until the stream ends **somehow**: `Ok(n)` for a clean end of
/// stream after `n` bytes, `Err(e)` for the error that ended it.
async fn read_until_end(r: &mut TestRecvStream, what: &str) -> Result<usize, ReadError> {
    let mut total = 0usize;
    loop {
        match within(r.read(&mut [0u8; 2048]), what).await {
            Ok(Some(n)) => total += n,
            Ok(None) => return Ok(total),
            Err(e) => return Err(e),
        }
    }
}

// ═══════════════════ 1. Drop semantics (§16.2, ruling 93) ═════════════

/// §16.2:4395 — *"Dropping a `SendStream` without `finish()` resets it with
/// error code 0."*
///
/// # BROKEN BUILD
///
/// * **A `Drop` that does nothing at all.** The peer would sit `Pending`
///   for ever, so this is asserted with a bounded `within` rather than a
///   bare `await`: a hang is a failure mode this suite must not have.
/// * **A `Drop` that resets with some other code.** `assert_eq!` on `0`,
///   not `matches!(Reset(_))`.
/// * **A `Drop` that ends the stream cleanly.** `Ok(None)` is an explicit
///   panic arm, named as the bug.
#[tokio::test(start_paused = true)]
async fn dropping_an_unfinished_send_stream_resets_it_with_code_zero() {
    local(async {
        let pair = Pair::seeded(0x4B00_0001);
        let (ca, cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, b"half a message", "probe").await;
        settle().await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        drop(s);
        settle().await;

        match read_until_end(&mut r, "peer read").await {
            Err(e) => assert_eq!(
                e,
                ReadError::Reset(0),
                "§16.2: the drop of an unfinished `SendStream` is a reset with \
                 code 0 — exactly 0, because that is what distinguishes it from \
                 an application's own code"
            ),
            Ok(_) => panic!(
                "§16.2: dropping a `SendStream` **without** `finish()` resets it; \
                 a `Drop` that does nothing ends the stream cleanly instead"
            ),
        }
    })
    .await;
}

/// **The highest-value test in slice 4b.** §16.2's rule is *"dropping a
/// `SendStream` **without `finish()`**"*, and `CONTRACT-4b.md` §9 spells
/// out why the qualifier is load-bearing.
///
/// # BROKEN BUILD
///
/// The one the contract names as the single most likely 4b bug: `Drop`
/// resets unconditionally, on the reasoning that `SendHalf::reset` is
/// idempotent. It is idempotent with respect to a previous **reset** — it
/// early-returns only when `self.reset.is_some()` — and **not** with
/// respect to `fin`. So that build resets every stream the application
/// finished, pins `final_size` at the highest byte transmitted, wipes
/// `fresh`/`retransmit`/`unacked`/`buf`, and destroys every byte not yet on
/// the wire.
///
/// It passes every test that reads before the sender drops. Here nothing
/// is read until after `drop(s)`, and the payload is deliberately larger
/// than one packet so that "every byte not yet on the wire" is a real set.
///
/// The rule it must implement is `closed_locally`: `Drop` resets only when
/// **neither** `finish()` nor `reset()` has been called on this handle.
#[tokio::test(start_paused = true)]
async fn dropping_a_finished_send_stream_still_delivers_the_end_of_stream() {
    local(async {
        let pair = Pair::seeded(0x4B00_0002);
        let (ca, cb) = pair.establish().await;

        let want = payload(40 * 1024);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &want, "bulk").await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        within(s.finish(), "finish").await.expect("finish");
        settle().await;

        drop(s);
        settle().await;

        let got = read_to_end(&mut r, "peer read after the sender dropped").await;
        assert_same_bytes(&got, &want, "a finished-then-dropped stream's contents");
    })
    .await;
}

/// `reset()` after `finish()` is legal — and, once the FIN has landed, it
/// is a **no-op at the peer**. §9.6 says so in terms:
///
/// > A RESET_STREAM for an already-FIN-complete receive half is a valid
/// > no-op if the final sizes agree, `FINAL_SIZE_ERROR` otherwise.
///
/// The sizes always agree here, and in slice 4 they always will: ruling
/// 111 pins `final_size` at *the highest byte actually transmitted*, and
/// slice 4 has no congestion control, so `write()` never leaves accepted
/// bytes unsealed — a blocked write returns `Pending` and the bytes stay
/// with the caller (ruling 114). **The two sizes can only disagree once
/// something holds sealed-but-unsent data, which is slice 5.**
///
/// This test was written the other way round, asserting the peer sees
/// `Reset(0x77)`, and it failed. The author's *intent* was a positive
/// control — a build whose `reset()` early-returns on `fin` would pass the
/// finished-then-dropped test above by never resetting a finished stream
/// at all. That control is unreachable from the public API in slice 4
/// (working rule 13: the fixture cannot express it), and
/// `dropping_an_unfinished_send_stream_resets_it_with_code_zero` is what
/// stops a no-op `Drop` instead. The reachable control moves to slice 5.
///
/// # BROKEN BUILD
///
/// A receive half that surfaced `Reset` here would violate §9.6 and turn
/// a complete, correctly delivered transfer into an error — the same
/// misreport ruling 121 forbids in the other direction.
#[tokio::test(start_paused = true)]
async fn a_reset_after_a_delivered_fin_is_a_no_op() {
    local(async {
        let pair = Pair::seeded(0x4B00_0003);
        let (ca, cb) = pair.establish().await;

        let want = payload(8 * 1024);
        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &want, "bulk").await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        within(s.finish(), "finish").await.expect("finish");
        settle().await;

        s.reset(0x77);
        settle().await;

        assert_same_bytes(
            &read_to_end(&mut r, "peer read after a reset that followed the FIN").await,
            &want,
            "§9.6: a RESET_STREAM for an already-FIN-complete receive half is a \
             valid no-op when the final sizes agree",
        );
    })
    .await;
}

/// `CONTRACT-4b.md` §9: `Drop` must remove this handle's waker-map entry
/// **and** still make the core call.
///
/// # Reachability note
///
/// The plan asks for "a `SendStream` whose `write` future is parked on
/// credit" at the moment of the drop. That state is not constructible from
/// safe Rust: `write(&mut self, ..)` borrows the handle for the future's
/// life, so the future must be gone before the handle can be. What *is*
/// constructible — and what actually exercises the hazard — is a handle
/// whose waker map entry is populated by a cancelled park, dropped
/// immediately after. Recorded in `TESTS-4b.md` §4.
///
/// # BROKEN BUILD
///
/// * **A `Drop` that skips the core call when a waker is registered**
///   (for instance, one that returns early after clearing the map). The
///   peer never sees the reset.
/// * **Finding F10's re-entry hazard**: waking inside the `RefCell` borrow
///   would panic here, because the drop path both mutates the cell and
///   touches the waker map.
#[tokio::test(start_paused = true)]
async fn dropping_a_send_stream_that_parked_on_credit_still_resets_it() {
    local(async {
        let pair = Pair::seeded(0x4B00_0004);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        let over = payload(window + 4096);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        let (accepted, blocked) = fill_until_blocked(&mut s, &over, "fill").await;
        assert!(blocked, "§10.1: the reader has read nothing");
        assert_eq!(accepted, window, "§10.2: parked at the stream window");

        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        // The park above left this handle's waker in `blocked_writers[r]`.
        drop(s);
        settle().await;

        match read_until_end(&mut r, "peer read").await {
            Err(e) => assert_eq!(e, ReadError::Reset(0)),
            Ok(_) => panic!(
                "§16.2: a `SendStream` dropped while it had a waker registered is \
                 still a `SendStream` dropped without `finish()`"
            ),
        }
    })
    .await;
}

/// §16.2:4396 + ruling 93: dropping a `RecvStream` abandons the receive
/// half. *"A sender that keeps pushing stalls at the stream window."*
///
/// # BROKEN BUILD
///
/// * **A build that keeps advancing stream credit for an abandoned half.**
///   The sender would never park; caught by the `blocked` flag.
/// * **A build that abandons the whole connection with the stream.** The
///   sibling stream and `closed()` say otherwise.
/// * A byte count, not a bare "it stalls": the park is asserted at exactly
///   `INITIAL_MAX_STREAM_DATA`, which a build with any other window fails
///   while still stalling.
#[tokio::test(start_paused = true)]
async fn dropping_a_recv_stream_stalls_only_that_stream() {
    local(async {
        let pair = Pair::seeded(0x4B00_0005);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        let over = payload(window + 8192);

        let mut s = within(ca.open_uni(), "open abandoned").await.expect("open");
        write_all(&mut s, &payload(2048), "first bytes").await;
        settle().await;

        let r = within(cb.accept_uni(), "accept").await.expect("accept");
        drop(r);
        settle().await;

        let (accepted, blocked) = fill_until_blocked(&mut s, &over, "fill an abandoned half").await;
        assert!(
            blocked,
            "ruling 93: stream-level credit is never again advanced for an \
             abandoned half, so the sender must stall"
        );
        assert_eq!(
            accepted + 2048,
            window,
            "§10.2: it stalls at the stream window and nowhere else"
        );

        // A sibling on the same connection is untouched.
        let sibling_bytes = payload(6000);
        let mut sib = within(ca.open_uni(), "open sibling").await.expect("open");
        write_all(&mut sib, &sibling_bytes, "sibling").await;
        within(sib.finish(), "sibling finish")
            .await
            .expect("finish");
        settle().await;
        let mut rs = within(cb.accept_uni(), "accept sibling")
            .await
            .expect("accept");
        assert_same_bytes(
            &read_to_end(&mut rs, "sibling read").await,
            &sibling_bytes,
            "a sibling stream after a half was abandoned",
        );

        let mut closed = pin!(ca.closed());
        assert!(
            poll_once(closed.as_mut()).await.is_pending(),
            "ruling 93: an abandoned stream never wedges the connection"
        );
    })
    .await;
}

/// **Ruling 93's second mechanism, and the only place `tests/` can see
/// it.** §10.3: *"an abandoned stream never wedges the connection
/// window"* — the retirement trues the stream's contribution up to the
/// highest stream-level limit ever advertised.
///
/// The receiving side drops each `BiStream` **whole**, which also
/// discharges `CONTRACT-4b.md` §6 and `PLAN-4b.md` §5.5: `BiStream` has no
/// `Drop` of its own and its two fields' `Drop`s do the work. A build that
/// writes `Drop` on `BiStream` and forgets the receive half fails here
/// while passing every send-half test.
///
/// # BROKEN BUILD
///
/// * **Tombstone only, no true-up.** The connection window stays spent for
///   the connection's life and the fifth stream is parked for ever. Ruling
///   93 records that either mechanism alone is wrong; this is the half a
///   watermark cannot cover.
/// * **True-up only, no tombstone** is *not* separated here — it needs the
///   resurrect-an-abandoned-bidi-half construction, which is core-level.
///   Recorded in `TESTS-4b.md` §4.
/// * The assertion is **two-sided**: parked before the drops, resolving
///   after. Asserting only the second half passes a build that never
///   enforced the connection window at all.
#[tokio::test(start_paused = true)]
async fn dropping_recv_streams_releases_the_connection_window() {
    local(async {
        let pair = Pair::seeded(0x4B00_0006);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        let over = payload(window + 4096);
        // 1 048 576 / 262 144 == 4.
        let fillers = INITIAL_MAX_DATA as usize / window;

        let mut held = Vec::new();
        let mut total = 0usize;
        for i in 0..fillers {
            let bi = within(ca.open_bi(), "open_bi filler")
                .await
                .expect("open_bi");
            let (mut send, recv) = bi.split();
            let (got, blocked) = fill_until_blocked(&mut send, &over, "filler fill").await;
            assert!(blocked, "filler {i} must park at a window");
            total += got;
            held.push((send, recv));
        }
        assert_eq!(
            total, INITIAL_MAX_DATA as usize,
            "§10.1: four stream windows exactly exhaust the connection window"
        );

        // A fresh stream with an untouched stream window: only the
        // connection level can park its first byte.
        let extra = within(ca.open_bi(), "open_bi extra")
            .await
            .expect("open_bi");
        let (mut extra_send, _extra_recv) = extra.split();
        settle().await;
        assert!(
            is_pending(extra_send.write(b"one byte")).await,
            "§10.1: the connection window is spent, so even a brand-new stream parks"
        );

        // The peer accepts and abandons all four, whole.
        let mut accepted = Vec::new();
        for i in 0..fillers {
            accepted.push(
                within(cb.accept_bi(), "accept_bi")
                    .await
                    .unwrap_or_else(|e| panic!("accept_bi {i}: {e:?}")),
            );
        }
        drop(accepted);
        settle().await;

        // ── the true-up ────────────────────────────────────────────────
        let n = within(extra_send.write(b"one byte"), "write after the abandonment")
            .await
            .expect("write");
        assert!(
            n >= 1,
            "§10.3: retirement advances connection credit — without the true-up \
             the connection is in a permanent send stall with no error and no timer"
        );
    })
    .await;
}

/// `CONTRACT-4b.md` §6 / ruling 120: the two halves of a `BiStream` have
/// independent lifetimes over one core stream.
///
/// # BROKEN BUILD
///
/// * **A `Drop` written on `BiStream` rather than on the halves.** Once
///   `split()` has handed the halves out there is no `BiStream` left to
///   drop, so such a build either leaks both halves or — the shape this
///   test catches — ties them together and kills the send half when the
///   receive half goes.
#[tokio::test(start_paused = true)]
async fn dropping_the_recv_half_of_a_bi_stream_leaves_the_send_half_alive() {
    local(async {
        let pair = Pair::seeded(0x4B00_0007);
        let (ca, cb) = pair.establish().await;

        let want = payload(7000);

        let bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");
        let id = bi.id();
        let (mut send, recv) = bi.split();
        drop(recv);
        settle().await;

        write_all(
            &mut send,
            &want,
            "send half after the recv half was dropped",
        )
        .await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        let peer = within(cb.accept_bi(), "accept_bi")
            .await
            .expect("accept_bi");
        assert_eq!(peer.id(), id, "one stream, one id, across both ends");
        let (_peer_send, mut peer_recv) = peer.split();
        assert_same_bytes(
            &read_to_end(&mut peer_recv, "peer read").await,
            &want,
            "the send half survives its sibling's drop",
        );
    })
    .await;
}

/// **Ruling 115 / `CONTRACT-4b.md` §7 — a stream handle *is* a handle.**
///
/// Ruling 62's test decides it: *a future or handle that changes protocol
/// state when dropped is a handle; one that does not, is not.* Dropping a
/// `SendStream` puts RESET_STREAM on the wire, and **a `Drop` that must
/// emit a frame needs a driver to emit it.**
///
/// # BROKEN BUILD
///
/// The alternative the contract rejects: a `SendStream` that does not
/// count. Under it, dropping the last `Connection` fires
/// `close(NO_ERROR, "")` **underneath** a live `SendStream` — in the
/// *ordinary* shape of a task that owns a stream and has let the
/// connection handle go — and from that moment every `write` returns
/// `ConnectionLost` and the stream's own RESET_STREAM is silently lost.
///
/// Two assertions, because the second alone is weaker than it looks:
///
/// * the peer's `closed()` is still `Pending` right after `drop(ca)` — no
///   CLOSE was sent;
/// * the stream still writes, finishes, and delivers every byte.
///
/// The `Endpoint` is still held by `pair`, so this test does **not** speak
/// to whether a `SendStream` alone keeps the driver alive; that half is not
/// separable while any `Endpoint` lives. Recorded in `TESTS-4b.md` §4.
#[tokio::test(start_paused = true)]
async fn a_live_send_stream_stops_the_last_connection_drop_from_closing_underneath_it() {
    local(async {
        let pair = Pair::seeded(0x4B00_0013);
        let (ca, cb) = pair.establish().await;

        let want = payload(5000);
        let mut s = within(ca.open_uni(), "open").await.expect("open");

        drop(ca);
        settle().await;

        let mut peer_closed = pin!(cb.closed());
        assert!(
            poll_once(peer_closed.as_mut()).await.is_pending(),
            "ruling 115: the last `Connection` handle went, but a `SendStream` \
             is a handle too — so no `close(NO_ERROR, \"\")` was performed"
        );

        write_all(
            &mut s,
            &want,
            "write after the Connection handle was dropped",
        )
        .await;
        within(s.finish(), "finish").await.expect("finish");
        settle().await;

        // Claimed and read while the `SendStream` is still alive. The
        // author's original ordering read *after* `drop(s)` and failed —
        // correctly, and for a reason worth more than the test: see
        // `a_receiver_can_drain_a_stream_the_sender_closed_behind`, ruling
        // 128, and slice 5.
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        assert_same_bytes(
            &read_to_end(&mut r, "peer read").await,
            &want,
            "a stream outliving its `Connection` handle still delivers",
        );

        // And now the other half of ruling 115 — **ruling 125**: that
        // `SendStream` is the last handle to this connection, so dropping it
        // *does* perform §16.2's `close(NO_ERROR, "")`. Without it the
        // connection would emit nothing and the peer would pay
        // `DEAD_TIMEOUT`.
        drop(s);
        settle().await;
        assert!(
            matches!(
                poll_once(pin!(cb.closed()).as_mut()).await,
                Poll::Ready(ConnectionLost::PeerClosed { code: 0, .. })
            ),
            "ruling 125: the last handle to a connection can be a stream, and \
             dropping it closes"
        );
    })
    .await;
}

/// **Ruling 128 — the receiver's half of ruling 47's problem. Not
/// implemented in slice 4; this test names the obligation.**
///
/// A sender that writes, finishes and drops everything — the fire-and-forget
/// pattern, and the one §16.2 makes reachable *by accident* since dropping
/// the last handle performs `close(NO_ERROR, "")` — delivers every byte to
/// the peer's transport and then closes. The peer's application has not
/// claimed the stream yet, and under ruling 118 as first written it never
/// can: `accept_uni()` answers `Err(PeerClosed)` over a stream that fully
/// arrived.
///
/// Ruling 128 overturns that. The data is *in the core* — `drop_state()`
/// drops the session and the timers and leaves `streams` and `flow` alone —
/// but two guards make it unreachable: `core::Connection::read`'s
/// unconditional `self.lost` check, and the shell releasing the core at
/// `Retired`, which fires at the instant of death. Both are slice 5's to
/// move, alongside ruling 47's `acked()`, because they are the same problem
/// from the two ends.
///
/// # BROKEN BUILD
///
/// The build shipped in slice 4b, where this was `#[ignore]`d naming this
/// ruling. **Slice 5b implemented it and the `#[ignore]` is gone.** A build
/// that regressed to the 4b behaviour answers `Err(PeerClosed)` to the
/// `accept_uni` below, over a stream that arrived in full.
#[tokio::test(start_paused = true)]
async fn a_receiver_can_drain_a_stream_the_sender_closed_behind() {
    local(async {
        let pair = Pair::seeded(0x4B00_0014);
        let (ca, cb) = pair.establish().await;

        let want = payload(5000);
        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &want, "write").await;
        within(s.finish(), "finish").await.expect("finish");
        settle().await;

        // The sender is done and lets go of everything.
        drop(s);
        drop(ca);
        settle().await;

        let mut r = within(cb.accept_uni(), "accept")
            .await
            .expect("ruling 128: a fully-arrived stream survives the peer's close");
        assert_same_bytes(
            &read_to_end(&mut r, "peer read").await,
            &want,
            "ruling 128: every byte that arrived before the CLOSE is readable",
        );
    })
    .await;
}

// ═══════════════ 2. Two-sided boundaries on the handle surface ═════════

/// `CONTRACT-4b.md` §3, ruling 110: **`Ok(0)` has exactly one meaning —
/// `buf` was empty.** The shell checks `buf.is_empty()` *before* calling
/// the core.
///
/// # BROKEN BUILD
///
/// * **A shell that reports a blocked write as `Ok(0)`.** Both sides are
///   asserted in one test: `write(&[])` is `Ok(0)` *and* a blocked
///   non-empty write is `Pending`. A test with only the first row passes
///   the inverted build, and a caller looping `while n == 0` on it spins.
/// * **A shell that forwards an empty `buf` to the core** and returns
///   whatever the core says — including, at a credit ceiling, `Pending`
///   for a call that asked for nothing.
/// * The one-sided-boundary trap: `0` and `1` are both asserted, not just
///   `0`.
#[tokio::test(start_paused = true)]
async fn an_empty_buffer_is_the_only_meaning_of_ok_zero_on_write() {
    local(async {
        let pair = Pair::seeded(0x4B00_0008);
        let (ca, _cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open").await.expect("open");

        assert_eq!(
            within(s.write(&[]), "empty write").await,
            Ok(0),
            "ruling 110: an empty buffer is `Ok(0)`, resolved before the core is touched"
        );
        assert_eq!(
            within(s.write(b"x"), "one-byte write").await,
            Ok(1),
            "the boundary's other side: one byte is one byte"
        );

        // Now park the stream and confirm the *blocked* answer is not `Ok(0)`.
        let window = INITIAL_MAX_STREAM_DATA as usize;
        let over = payload(window);
        let (_, blocked) = fill_until_blocked(&mut s, &over, "fill").await;
        assert!(blocked, "§10.1: nothing is being read");

        // `fill_until_blocked` already asserts every `Ok(n)` has `n >= 1`;
        // this states the same thing at the surface the caller sees.
        assert!(
            is_pending(s.write(b"y")).await,
            "CONTRACT-4b §3: a blocked write is `Pending`. A build that returned \
             `Ok(0)` here would collide with the empty-buffer row above and a \
             caller could not tell the two apart"
        );
        assert_eq!(
            within(s.write(&[]), "empty write while blocked").await,
            Ok(0),
            "ruling 110: the empty-buffer short-circuit runs before the credit \
             check, so it answers even on a parked stream"
        );
    })
    .await;
}

/// `CONTRACT-4b.md` §5, ruling 119: **`Ok(Some(0))` has exactly one
/// meaning — `buf` was empty.** *"`Ok(Some(0))` versus `Ok(None)` is the
/// distinction that hangs a reader for ever if inverted."*
///
/// # BROKEN BUILD
///
/// * **`Ok(Some(0))` used for "no data".** A reader looping `while let
///   Ok(Some(n))` spins for ever instead of parking; a reader that treats
///   `Some(0)` as the end truncates the stream. The `is_pending` row is
///   what separates it.
/// * **`Ok(None)` used for "no data"** — the inversion working rule 14
///   records as slice 4a's near-miss: every reader stops at the first
///   quiet moment and reports a partial transfer as complete. Caught by
///   reading the *whole* payload after a `Pending` observation.
/// * The one-sided-boundary trap: a 0-byte buffer and a 1-byte buffer are
///   both asserted.
#[tokio::test(start_paused = true)]
async fn an_empty_buffer_is_the_only_meaning_of_ok_some_zero_on_read() {
    local(async {
        let pair = Pair::seeded(0x4B00_0009);
        let (ca, cb) = pair.establish().await;

        let first = payload(3000);
        let second = payload(2000);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &first, "first").await;
        settle().await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        assert_eq!(
            within(r.read(&mut []), "empty read with data waiting").await,
            Ok(Some(0)),
            "ruling 119: an empty buffer is `Ok(Some(0))` — and it is *not* \
             `Ok(None)`, even though this stream has data and no FIN"
        );
        let mut one = [0u8; 1];
        assert_eq!(
            within(r.read(&mut one), "one-byte read").await,
            Ok(Some(1)),
            "the boundary's other side"
        );

        assert_eq!(
            one[0], first[0],
            "the one-byte read delivered the first byte"
        );
        drain_exactly(&mut r, first.len() - 1, "drain the rest of the first write").await;

        // Everything sent has been drained and there is no FIN: the answer
        // is `Pending`, not `Ok(Some(0))` and not `Ok(None)`.
        assert!(
            is_pending(r.read(&mut [0u8; 512])).await,
            "CONTRACT-4b §5: no data available is `Pending`. `Ok(Some(0))` here \
             spins a caller; `Ok(None)` here reports a partial transfer as complete"
        );

        // And the stream really was not over.
        write_all(&mut s, &second, "second").await;
        within(s.finish(), "finish").await.expect("finish");
        settle().await;
        let rest = read_to_end(&mut r, "drain second").await;
        assert_same_bytes(
            &rest,
            &second,
            "the bytes that follow a quiet moment — proof the quiet moment was \
             not the end of the stream",
        );
    })
    .await;
}

/// §16.2 / `CONTRACT-4b.md` §3: `finish()` resolves on the first poll and
/// never parks; it is idempotent; and after it the write verbs are closed.
///
/// # BROKEN BUILD
///
/// * **A `finish()` that waits for the peer.** Ruling 47's `acked()` is
///   the verb that would, and it is slice 5's. One poll — not a
///   `timeout` — is what states "immediately"; a `timeout` races the
///   future rather than observing it.
/// * **A `finish()` that is not idempotent** (a second call erroring, or
///   worse, emitting a second FIN).
/// * **A `write` after `finish` that succeeds**, silently discarding bytes
///   the caller believes were sent.
#[tokio::test(start_paused = true)]
async fn finish_resolves_immediately_is_idempotent_and_closes_the_write_verbs() {
    local(async {
        let pair = Pair::seeded(0x4B00_000A);
        let (ca, _cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, b"a message", "probe").await;

        // Scoped: `finish(&mut self)` borrows the handle for the future's
        // life, and `pin!`'s temporary lives to the end of its block — so
        // the block, not `drop`, is what gives `s` back.
        {
            let mut fin = pin!(s.finish());
            match poll_once(fin.as_mut()).await {
                Poll::Ready(Ok(())) => {}
                other => panic!(
                    "§16.2: `finish()` resolves on the **first** poll — it accepts \
                     the FIN into send state and does not wait for the peer. \
                     Got {other:?}"
                ),
            }
        }

        assert_eq!(
            within(s.finish(), "second finish").await,
            Ok(()),
            "CONTRACT-4b §3: `finish()` is idempotent"
        );
        assert_eq!(
            within(s.write(b"after"), "write after finish").await,
            Err(WriteError::Finished),
            "§16.2: a finished stream accepts no more bytes"
        );
    })
    .await;
}

/// `CONTRACT-4b.md` §3: `reset()` is synchronous, infallible, idempotent,
/// and **the first code wins**.
///
/// # BROKEN BUILD
///
/// * **A `reset` that lets the last code win.** Both codes are distinct
///   and non-zero, and the peer's is asserted with `assert_eq!`, so a
///   last-wins build fails rather than merely reordering.
/// * **A `reset` that is not idempotent** — a second RESET_STREAM with a
///   different final size is a protocol violation at the peer.
#[tokio::test(start_paused = true)]
async fn reset_is_idempotent_and_the_first_code_wins() {
    local(async {
        let pair = Pair::seeded(0x4B00_000B);
        let (ca, cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &payload(1500), "probe").await;
        settle().await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        s.reset(0x11);
        s.reset(0x22);
        settle().await;

        match read_until_end(&mut r, "peer read").await {
            Err(e) => assert_eq!(
                e,
                ReadError::Reset(0x11),
                "CONTRACT-4b §3: idempotent, first code wins"
            ),
            Ok(_) => panic!("§9.6: a reset stream does not end cleanly"),
        }

        assert_eq!(
            within(s.write(b"more"), "write after reset").await,
            Err(WriteError::Finished),
            "CONTRACT-4b §3: a locally reset half reports `Finished`, never \
             `Reset` — `WriteError::Reset` has no producer in slice 4"
        );
        assert_eq!(
            within(s.finish(), "finish after reset").await,
            Err(WriteError::Finished)
        );
    })
    .await;
}

/// Ruling 116: **the handle caches its id.** `None` until the core's first
/// `Some`, then `Some(id)` for ever — including after the stream closes
/// and after the connection dies. The core's `stream_id(r)` is *not*
/// monotone: `Streams::after_half_freed` removes the entry, so an uncached
/// `id()` answers `None` again once a stream fully closes.
///
/// # BROKEN BUILD
///
/// * **An uncached `id()`.** It answers correctly right up until the
///   stream is fully closed, then starts returning `None` — contradicting
///   `remote_static()`'s keeps-answering property. Every test that reads
///   `id()` on a live stream passes it.
/// * **An `id()` that reads through a dead connection's cell** and panics
///   or returns `None` once the core is gone.
///
/// This test deliberately does **not** assert that `None` is reachable:
/// ruling 116 removed the documented cause, and an application-held
/// handle's `id()` is always `Some` in wire v1.
#[tokio::test(start_paused = true)]
async fn id_keeps_answering_after_the_stream_closes_and_after_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x4B00_000C);
        let (ca, cb) = pair.establish().await;

        let want = payload(2500);
        let mut s = within(ca.open_uni(), "open").await.expect("open");
        let sid = s.id().expect("ruling 116: a held handle answers `Some`");
        write_all(&mut s, &want, "probe").await;
        within(s.finish(), "finish").await.expect("finish");
        settle().await;

        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        let rid = r.id().expect("ruling 116");
        assert_eq!(rid, sid, "both ends name one stream");

        // Read to the end: the receive half is now fully closed and the
        // core has dropped its entry.
        assert_same_bytes(&read_to_end(&mut r, "read").await, &want, "contents");
        assert_eq!(
            r.id(),
            Some(rid),
            "ruling 116: `id()` keeps answering after the stream is fully \
             closed — an uncached build answers `None` from here on"
        );

        // And after the connection dies.
        ca.close(0, b"").await;
        cb.close(0, b"").await;
        settle().await;
        assert_eq!(
            s.id(),
            Some(sid),
            "CONTRACT-4b §8: `id` keeps answering, like `remote_static()`"
        );
        assert_eq!(r.id(), Some(rid));
    })
    .await;
}

/// Ruling 120: `split()` / `join()` round-trip, and a mismatched `join`
/// **returns** the pair rather than panicking.
///
/// # BROKEN BUILD
///
/// * **A `debug_assert` on mismatch.** Ruling 44's precedent governs —
///   *rejection is a `Result`, never a panic* — and under a
///   `debug_assert` a release build holds a `BiStream` whose halves are
///   different streams and whose `Drop` resets a stream the caller never
///   named. A release-mode run of this test is what catches that, which is
///   why `cargo test --release --all-features` is a release gate.
/// * **A `join` that accepts the mismatch.** Caught by the `Err` arm.
/// * **A `join` that consumes the halves on rejection.** The contract says
///   *"the halves are handed back unchanged"*; the ids of the returned
///   pair are asserted, and the pair is then rejoined correctly and used.
#[tokio::test(start_paused = true)]
async fn join_rejects_mismatched_halves_and_hands_them_back_unchanged() {
    local(async {
        let pair = Pair::seeded(0x4B00_000D);
        let (ca, cb) = pair.establish().await;

        let bi1 = within(ca.open_bi(), "open_bi 1").await.expect("open_bi");
        let bi2 = within(ca.open_bi(), "open_bi 2").await.expect("open_bi");
        let id1 = bi1.id().expect("ruling 116");
        let id2 = bi2.id().expect("ruling 116");
        assert_ne!(id1, id2);

        let (s1, r1) = bi1.split();
        let (s2, r2) = bi2.split();

        // Mismatched: stream 1's send half with stream 2's receive half.
        let (s1, r2) = match TestBiStream::join(s1, r2) {
            Ok(_) => panic!(
                "ruling 120: `join` validates same-`StreamRef`-same-`ConnectionId`; \
                 accepting a mismatch builds a handle whose `Drop` resets a stream \
                 the caller never named"
            ),
            Err(pair) => pair,
        };
        assert_eq!(
            s1.id(),
            Some(id1),
            "ruling 120: the halves are handed back **unchanged**"
        );
        assert_eq!(r2.id(), Some(id2));

        // Rejoined correctly, and still usable: rejection consumed nothing.
        let want = payload(1200);
        // `.expect()` is unavailable here: `join`'s error type is
        // `(SendStream, RecvStream)` and neither handle is `Debug`, so the
        // `E: Debug` bound is unsatisfied. Flagged in `TESTS-4b.md` §3.
        let rejoined = match TestBiStream::join(s1, r1) {
            Ok(bi) => bi,
            Err(_) => panic!("ruling 120: the matching pair must join"),
        };
        assert_eq!(rejoined.id(), Some(id1), "`join` preserves the id");
        let (mut send, _recv) = rejoined.split();
        write_all(&mut send, &want, "after a rejected join").await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        let peer = within(cb.accept_bi(), "accept_bi")
            .await
            .expect("accept_bi");
        assert_eq!(peer.id(), Some(id1));
        let (_ps, mut pr) = peer.split();
        assert_same_bytes(&read_to_end(&mut pr, "peer read").await, &want, "contents");

        // Keep stream 2's halves alive to the end of the test so the
        // assertion above is not about a stream that was reset underneath.
        drop((s2, r2));
    })
    .await;
}

/// `CONTRACT-4b.md` §8 — one rule, all verbs — and ruling 118: `open_*`
/// and `accept_*` answer `Err(ConnectionLost)` **immediately, nothing
/// drained first**.
///
/// §15.2 drops stream state at close, so there is nothing to drain;
/// draining would hand back a handle on which every `read` fails.
///
/// # BROKEN BUILD
///
/// * **A drain-then-report `accept_*`.** The stream opened below is
///   unclaimed when the connection dies, so a build that drains hands it
///   back — and every `read` on it then fails. `poll_once` is what states
///   *immediately*: a `timeout` cannot tell "on the first poll" from
///   "after a driver turn".
/// * **A verb that panics on a dead cell** rather than answering from
///   `closed`.
/// * **A `reset()` that panics or errors on a dead connection.** It is a
///   silent no-op.
/// * **An `id()` that stops answering.** Covered from the other side in
///   `id_keeps_answering_...`; asserted here too because §8's table lists
///   it in the same row set and a list in the spec is read as exhaustive.
#[tokio::test(start_paused = true)]
async fn every_stream_verb_answers_connection_lost_after_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x4B00_000E);
        let (ca, cb) = pair.establish().await;

        // A live stream in each direction, plus one the peer opened that
        // `ca` deliberately never accepts.
        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &payload(1000), "probe").await;
        let sid = s.id().expect("ruling 116");
        settle().await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        let mut peer_opened = within(cb.open_uni(), "peer open").await.expect("open");
        write_all(&mut peer_opened, b"never accepted", "peer probe").await;
        settle().await;

        ca.close(0x00, b"").await;
        settle().await;

        assert!(
            matches!(
                within(s.write(b"more"), "write after close").await,
                Err(WriteError::ConnectionLost(_))
            ),
            "CONTRACT-4b §8: `write` answers from the latch"
        );
        assert!(matches!(
            within(s.finish(), "finish after close").await,
            Err(WriteError::ConnectionLost(_))
        ));

        // Silent no-op, not a panic and not an error.
        s.reset(9);

        assert_eq!(
            s.id(),
            Some(sid),
            "CONTRACT-4b §8: `id` keeps answering after the connection dies"
        );

        // **Expired premise, twice over — rulings 128 and 146.** This
        // asserted ruling 118's original rule: `accept_*` refuses
        // immediately, "with nothing drained first". Ruling 128 overturned
        // that for the *draining* endpoint, and ruling 133 briefly kept it
        // for the *closing* one — which ruling 146 then reversed, because
        // freeing the closer's stream state made `Connection::acked()`
        // report `Ok(())` over bytes that were never acknowledged.
        //
        // So both death paths behave alike: a stream that arrived before
        // the death is still claimable, and only an **empty** queue answers
        // `ConnectionLost`. Both halves are asserted, because "it hands
        // something over" alone would pass a build that hands over the same
        // stream for ever.
        let mut acc = pin!(ca.accept_uni());
        match poll_once(acc.as_mut()).await {
            Poll::Ready(Ok(_)) => {}
            other => panic!(
                "ruling 128: a stream that arrived before the death survives \
                 it, whoever closed. Got {}",
                match other {
                    Poll::Pending => "Pending",
                    Poll::Ready(Err(_)) => "Err(ConnectionLost)",
                    Poll::Ready(Ok(_)) => unreachable!(),
                }
            ),
        }
        // `acc` is left to fall out of scope: dropping a `Pin<&mut _>`
        // releases nothing (the `pin!` temporary outlives it) and trips
        // `clippy::drop_non_drop`.
        let mut drained = pin!(ca.accept_uni());
        assert!(
            matches!(poll_once(drained.as_mut()).await, Poll::Ready(Err(_))),
            "ruling 128: once nothing is left, `accept_*` reports the death — \
             and never `Pending`, which would hang a claimer for ever"
        );

        let mut opn = pin!(ca.open_uni());
        assert!(
            matches!(poll_once(opn.as_mut()).await, Poll::Ready(Err(_))),
            "ruling 118: `open_*` answers immediately too"
        );

        // The peer observes the close — and, under ruling 128, still drains
        // what arrived before it. The 1 000 bytes written above are readable
        // *after* the death; only once they are gone does the death surface.
        // The stream carries no FIN, so there is no `Ok(None)` to reach.
        let _ = within(cb.closed(), "peer closed").await;
        let mut got = 0usize;
        loop {
            let mut buf = [0u8; 256];
            match within(r.read(&mut buf), "read after close").await {
                Ok(Some(n)) => got += n,
                Err(ReadError::ConnectionLost(_)) => break,
                other => panic!("ruling 128: expected a drain then the death, got {other:?}"),
            }
        }
        assert_eq!(
            got, 1000,
            "ruling 128: every byte that arrived before the CLOSE is readable \
             after it — a build that drops them reports 0 here and still \
             ends with `ConnectionLost`, which is why the count is asserted"
        );
    })
    .await;
}

// ═══════════════════════ 3. Cancel-safety ═════════════════════════════

/// §16.2's cancel-safety standard: *"a dropped future has claimed
/// nothing."* For `read` (`PLAN-4b.md` §4.2) `Pending` is reached only
/// after the core returned `Ok(Some(0))`, which consumes nothing and
/// advances no credit.
///
/// # BROKEN BUILD
///
/// * **A shell-side scratch buffer** (§10.6 and ruling 56 forbid one).
///   A `poll_read` that drained into shell-side storage and copied out on
///   the next poll strands the drained bytes where no later `read` can
///   reach them when the future is dropped. The construction below is the
///   one that reaches it: the future is polled to `Pending`, the data then
///   arrives and fires its waker, and the future is dropped **without ever
///   being polled again**.
/// * **A shell that consumes credit on the park.** Caught by the byte
///   count: every byte written must arrive exactly once.
#[tokio::test(start_paused = true)]
async fn a_cancelled_read_loses_no_byte_and_duplicates_none() {
    local(async {
        let pair = Pair::seeded(0x4B00_000F);
        let (ca, cb) = pair.establish().await;

        let first = payload(2048);
        let second = payload(3072);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        write_all(&mut s, &first, "first").await;
        settle().await;
        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        drain_exactly(&mut r, first.len(), "drain first").await;

        // Park a read, let the data arrive under it, then drop it unpolled.
        {
            let mut buf = [0u8; 4096];
            let mut park = pin!(r.read(&mut buf));
            assert!(
                poll_once(park.as_mut()).await.is_pending(),
                "nothing has been written since the drain"
            );
            write_all(&mut s, &second, "second").await;
            within(s.finish(), "finish").await.expect("finish");
            settle().await;
            // Dropped here, having been woken and never re-polled.
        }

        let rest = read_to_end(&mut r, "after the cancelled read").await;
        assert_same_bytes(
            &rest,
            &second,
            "every byte written after a cancelled read arrives exactly once",
        );
    })
    .await;
}

/// The same standard for `write` (`PLAN-4b.md` §4.1): `Pending` is reached
/// only after the core returned `Ok(0)` and buffered **nothing**.
///
/// # BROKEN BUILD
///
/// * **A `poll_write` that stores a partially consumed buffer across
///   polls** — ruling 53's named hazard. The cancelled write's payload is
///   a byte value that appears nowhere else in the stream, so a single
///   leaked byte is a content mismatch at a nameable offset rather than a
///   length that happens to work out.
/// * **A build that charges credit for the parked write.** Caught by the
///   resumed write accepting its full expected share afterwards.
#[tokio::test(start_paused = true)]
async fn a_cancelled_write_claims_nothing() {
    local(async {
        let pair = Pair::seeded(0x4B00_0010);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        let half = window / 2;
        let bulk = payload(window);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        // `bulk` is exactly one window, so this offers everything the
        // window admits and returns without needing to park — the park is
        // asserted below, on the `ghost` write, which is the one whose
        // cancellation this test is about.
        let (accepted, _) = fill_until_blocked(&mut s, &bulk, "fill").await;
        assert_eq!(
            accepted, window,
            "§10.2: the window admits exactly this much"
        );

        // The cancelled write. 0xFF appears nowhere in `payload()`, whose
        // values are `i % 251`.
        let ghost = vec![0xFFu8; 8192];
        assert!(
            is_pending(s.write(&ghost)).await,
            "§10.1: no credit, so this write parks and is then dropped"
        );

        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");
        drain_exactly(&mut r, half, "drain to the re-grant threshold").await;
        settle().await;

        let tail = vec![0xCCu8; 1024];
        write_all(&mut s, &tail, "tail after the cancelled write").await;
        within(s.finish(), "finish").await.expect("finish");
        settle().await;
        drop(s);
        settle().await;

        let mut want = Vec::with_capacity(window + tail.len());
        want.extend_from_slice(&bulk);
        want.extend_from_slice(&tail);

        let mut got = want[..half].to_vec();
        got.extend_from_slice(&read_to_end(&mut r, "drain the rest").await);
        assert_same_bytes(
            &got,
            &want,
            "a dropped `write` future claimed nothing — a single 0xFF byte here \
             is a stored partial buffer",
        );
    })
    .await;
}

/// §10.4's cumulative limit, and `PLAN-4b.md` §4.4's leak, from the one
/// angle `tests/` can reach.
///
/// `INITIAL_MAX_STREAMS_UNI` is 128. `CONTRACT-4b.md` §2: `open_uni` on an
/// exhausted space parks and is woken by `StreamsAvailable`; a peer-opened
/// uni stream read to EOF fully closes and grants MAX_STREAMS_UNI. (The
/// bidi equivalent parks for ever in slice 4 and is deliberately not
/// exercised.)
///
/// # BROKEN BUILD
///
/// * **A cumulative limit off by one in either direction.** Both sides are
///   asserted: all 128 opens succeed **and** the 129th parks. Slice 1's
///   one-sided-boundary trap was `LEN` and `LEN-1` tested and `LEN+1` not;
///   this is the same shape and both sides are here.
/// * **`open_uni` that returns `StreamsExhausted` to the caller.** Ruling
///   101 makes the shell convert it into a park, which is why the type
///   stays `pub(crate)`. A build that surfaced it would fail to compile
///   against `Result<_, ConnectionLost>` — or, worse, map it onto
///   `ConnectionLost`, which the park assertion catches.
/// * **§4.4's pop-without-construct leak.** The parked `open_uni` above is
///   dropped by the `is_pending` timeout. If a cancelled `open_uni` had
///   already spent an index, the stream opened after the replenishment
///   would carry index **129**, not 128. `StreamId::index()` is public, so
///   this is asserted directly. (The genuinely dangerous window — between
///   `core.open()` and constructing the handle — is inside one synchronous
///   poll and is unreachable from a test; see `TESTS-4b.md` §4.)
#[tokio::test(start_paused = true)]
async fn open_uni_parks_at_the_cumulative_limit_and_resumes_when_the_peer_frees_streams() {
    local(async {
        let pair = Pair::seeded(0x4B00_0011);
        let (ca, cb) = pair.establish().await;

        let limit = INITIAL_MAX_STREAMS_UNI as usize;

        for i in 0..limit {
            let mut s = within(ca.open_uni(), "open within the limit")
                .await
                .unwrap_or_else(|e| panic!("open {i} of {limit} failed: {e:?}"));
            assert_eq!(
                s.id().expect("ruling 116").index(),
                i as u64,
                "§9.1: indices are allocated in order, one per open"
            );
            write_all(&mut s, b"x", "probe").await;
            within(s.finish(), "finish").await.expect("finish");
            // Finished, so this drop must not reset it.
            drop(s);
        }
        settle().await;

        assert!(
            is_pending(ca.open_uni()).await,
            "§10.4: the {limit}th stream exhausts the cumulative limit, so the \
             next open parks — it does not error"
        );

        // The peer fully closes every one of them.
        for i in 0..limit {
            let mut r = within(cb.accept_uni(), "accept")
                .await
                .unwrap_or_else(|e| panic!("accept {i}: {e:?}"));
            assert_eq!(
                read_to_end(&mut r, "read to EOF").await,
                b"x",
                "a finished-then-dropped stream ends cleanly, {i} of {limit}"
            );
            drop(r);
        }
        settle().await;

        let resumed = within(ca.open_uni(), "open after replenishment")
            .await
            .expect("§10.4: MAX_STREAMS_UNI grew, so the space reopened");
        assert_eq!(
            resumed.id().expect("ruling 116").index(),
            limit as u64,
            "§4.4: the cancelled `open_uni` above claimed nothing — a future \
             dropped after `core.open()` succeeded would have spent index \
             {limit} and left this one at {}",
            limit + 1
        );
    })
    .await;
}

/// The `accept_*` half of the same standard (`PLAN-4b.md` §4.5), which is
/// *"strictly worse"*: `core.accept(dir)` **pops** from `unclaimed`, and a
/// pop that is not followed by a handle makes the stream unclaimable for
/// ever while the peer's bytes keep charging the receive ledger.
///
/// # BROKEN BUILD
///
/// * **A cancelled `accept_*` that swallowed a stream.** Two streams are
///   opened *after* the cancellation and both must be claimable, in FIFO
///   order (ruling 112) with the ids the opener minted. A build that lost
///   one leaves the second `accept_uni` parked; a build that reordered
///   them fails the id assertions.
/// * **A shell that assumes one `StreamOpened` event equals one stream**
///   (ruling 99's implicit open of six streams emits six events, and the
///   shell must re-poll on each wake). Two streams opened back to back,
///   claimed by two separate `accept_uni` calls, is the smallest case that
///   distinguishes it.
#[tokio::test(start_paused = true)]
async fn a_cancelled_accept_claims_nothing() {
    local(async {
        let pair = Pair::seeded(0x4B00_0012);
        let (ca, cb) = pair.establish().await;

        // Nothing to accept yet: this parks, then is dropped.
        assert!(
            is_pending(cb.accept_uni()).await,
            "no stream has been opened, so `accept_uni` parks"
        );

        let mut s1 = within(ca.open_uni(), "open 1").await.expect("open");
        write_all(&mut s1, b"one", "probe 1").await;
        let mut s2 = within(ca.open_uni(), "open 2").await.expect("open");
        write_all(&mut s2, b"two", "probe 2").await;
        within(s1.finish(), "finish 1").await.expect("finish");
        within(s2.finish(), "finish 2").await.expect("finish");
        settle().await;

        let mut r1 = within(cb.accept_uni(), "accept 1").await.expect("accept 1");
        let mut r2 = within(cb.accept_uni(), "accept 2").await.expect("accept 2");
        assert_eq!(r1.id(), s1.id(), "ruling 112: FIFO by open order");
        assert_eq!(r2.id(), s2.id());
        assert_eq!(read_to_end(&mut r1, "read 1").await, b"one");
        assert_eq!(read_to_end(&mut r2, "read 2").await, b"two");
    })
    .await;
}

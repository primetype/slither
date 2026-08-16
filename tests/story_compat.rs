//! **Composability — S31, plus ruling 226's adapter convention.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S31** | a slither stream *is* a byte stream: `tokio::io::copy` into a `BiStream` behind a `BufWriter`, `shutdown()`, the peer reads identical bytes and observes EOF; errors arrive as `io::Error`; a reset is `ConnectionReset`, not a silent truncation; `poll_shutdown` is `finish()` **and then** `acked()`; `flush()` is not delivery. |
//! | **ruling 226** | every `Result`-carrying `Stream` adapter **never ends** — `Some(Err(ConnectionLost))` for ever, never `None`. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 8** (Agent B), from
//! `STORIES.md` §I, `.slices/08-composability/CONTRACT-8.md` and the frozen
//! handle surface **alone**, while the implementer wrote `src/compat/`
//! concurrently. Base commit `704a4ae`; `src/compat/` did not exist in that
//! tree, and no line of it has been read.
//!
//! # Working rule 9 — every test names the build it separates
//!
//! *A bound is only a test if the degenerate case violates it.* Each test
//! carries a `BROKEN BUILD:` block naming a concrete implementation that
//! passes a weaker version of the test while being wrong. Where a property
//! could be satisfied for free by a collapsed implementation, the assertion
//! is made from the side that separates them.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the observation instrument in both directions:
//! [`within`] asserts *it resolved*, [`still_pending`] asserts *it had not
//! resolved by then*. [`settle`] gives both drivers a turn **without
//! advancing virtual time** — where a timer must fire, `tokio::time::advance`
//! says so explicitly (ruling 223). There is no `sleep`.
//!
//! # A note on ordering that bit this suite twice
//!
//! `open_bi()` / `open_uni()` allocate locally; the **peer learns of a
//! stream from the first frame carrying it**. So `accept_bi()` cannot be
//! awaited before the opener has written something — every test below
//! either writes first or accepts concurrently under `join!`.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufWriter, ReadBuf};

use slither::constants::DEAD_TIMEOUT;
use slither::testutil::{FlakyPolicy, Pair, TestRecvStream, addr_a, addr_b, local, settle};
use slither::{ConnectionLost, ReadError, WriteError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// `Pair` / `local` / `settle` / `FlakyPolicy` are slice 0+3 harness, used as
// shipped and named from `CONTRACT-8.md` §10. Everything from `compat/` is
// called exactly as `CONTRACT-8.md` §2/§3/§4 spells it — if a name below
// does not compile, the contract and the implementation disagree, and *that
// is the finding*, not a rename to be made here.
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve.
///
/// Deliberately **under `DEAD_TIMEOUT` (25 s)**: a budget at or past the
/// liveness deadline would let a test that hangs because the connection died
/// report as a timeout on the wrong thing. Generous inside that — on the
/// paused clock a resolvable future costs no wall time, and the lossy bulk
/// transfers below need several RTO cycles.
const PATIENCE: Duration = Duration::from_secs(20);

/// Virtual-time budget for the **"not before"** half. Long enough for every
/// driver turn and wire delay these tests create, short enough that it stays
/// far inside `DEAD_TIMEOUT`.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// `true` if `fut` had **not** resolved within [`NOT_BEFORE`].
///
/// Takes the future by `Pin<&mut _>` so the caller keeps it: the test that
/// needs this also drives the *same* future to completion afterwards, and a
/// fresh future would re-enter `poll_shutdown` from the top and prove
/// nothing about the one that parked.
async fn still_pending<F: Future>(fut: Pin<&mut F>) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

/// Poll `fut` **exactly once**, advancing no virtual time.
///
/// The instrument for "immediately": ruling 56's `poll_flush` and ruling
/// 57's `poll_shutdown` are both claims about what the *first* poll does,
/// and a `timeout` cannot distinguish "Ready at once" from "Ready after the
/// runtime auto-advanced to the next deadline".
fn poll_once<F: Future>(fut: Pin<&mut F>) -> Poll<F::Output> {
    let mut cx = Context::from_waker(Waker::noop());
    fut.poll(&mut cx)
}

/// Poll a `Stream` **exactly once** with a no-op waker (`CONTRACT-8.md` §10).
fn poll_next_once<S>(stream: &mut S) -> Poll<Option<S::Item>>
where
    S: futures_core::Stream + Unpin,
{
    let mut cx = Context::from_waker(Waker::noop());
    Pin::new(stream).poll_next(&mut cx)
}

/// Await one item from a `Stream`, without a `futures-util` dependency.
async fn next<S>(stream: &mut S) -> Option<S::Item>
where
    S: futures_core::Stream + Unpin,
{
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

/// A payload whose every byte is a function of its offset.
///
/// 251 is prime and coprime with every packet size in play, so a shift of
/// any length — a dropped overlap, a re-delivered duplicate, a lost tail —
/// moves *every* subsequent byte. A repeated-byte payload hides all three.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Compare without dumping 64 KiB into the panic message.
///
/// Length first, content second: **truncation** (the failure S31 names) and
/// **corruption** are two different bugs that a single `assert_eq!` would
/// report identically.
fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: byte count differs — a lost tail is short, a re-delivered \
         retransmission is long"
    );
    if let Some(i) = got.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: first differing byte at offset {i}: got {:#04x}, want {:#04x}",
            got[i], want[i]
        );
    }
}

/// A 10 %-loss path in both directions, installed **after** establishment.
///
/// After, on purpose: a handshake that has to survive loss is slice 2's
/// story, and folding it in here would make an S31 red ambiguous between two
/// slices.
fn lossy_both_ways(pair: &Pair) {
    let policy = FlakyPolicy::lossy(0.10);
    pair.a.wire.set_policy(policy.clone());
    pair.b.wire.set_policy(policy);
}

// ══════════════════════════════════════════════════════════════════════
// S31 — a user can treat a stream as an `AsyncRead`/`AsyncWrite`
// ══════════════════════════════════════════════════════════════════════

/// **S31, acceptance 1 — `copy` a payload in behind a `BufWriter`,
/// `shutdown()`, and the peer reads identical bytes and observes EOF.**
///
/// > `tokio::io::copy` a file into a `BiStream` behind a `BufWriter`, then
/// > `shutdown()`, and the peer reads identical bytes and observes EOF. Over
/// > `FlakyWire` with loss, on the paused clock.
///
/// The story names `copy` and `BufWriter` specifically, so they are used
/// rather than approximated: both are the ecosystem code that will actually
/// meet this surface, and both exercise parts of `AsyncWrite` that a
/// hand-rolled `write_all` does not — `BufWriter::poll_shutdown` flushes its
/// own buffer *through* `poll_write` and only then calls the inner
/// `poll_shutdown`, and `copy` re-polls after partial writes.
///
/// # BROKEN BUILD — what each assertion separates
///
/// * **An `AsyncRead` that maps the core's "no data available" to
///   `Ready(Ok(()))` with nothing filled.** That is `AsyncRead`'s EOF, so
///   `read_to_end` stops at the first gap and returns a **prefix**. Over a
///   10 % loss path there is always a gap. Caught by the length half of
///   [`assert_same_bytes`], which is asserted before the content half so
///   truncation and corruption do not report identically.
/// * **An `AsyncRead` that never latches EOF** (rulings 121/124 re-done in
///   `compat/` instead of reused). `read_to_end` never returns; the test
///   dies in [`within`] rather than hanging the suite.
/// * **An `AsyncWrite` that returns `Ok(0)` for a non-empty buffer.**
///   `tokio::io::copy` translates that into `WriteZero`, so this fails as an
///   `io::Error` rather than as a wrong byte count.
/// * **A `poll_shutdown` that does not let `BufWriter` flush its tail** —
///   the last partial buffer never reaches the wire and the reader is short.
/// * **A build that reassembles by offset across streams** — caught by the
///   offset-derived payload, not by the length.
///
/// 64 KiB is below `INITIAL_MAX_STREAM_DATA` (262 144) on purpose: nothing
/// here may stall on credit. S31 is about the byte-stream shape and S17 is
/// about flow control, and a test that mixes them cannot say which failed.
#[tokio::test(start_paused = true)]
async fn s31_copy_through_a_bufwriter_then_shutdown_delivers_identical_bytes_and_eof() {
    local(async {
        let pair = Pair::seeded(0x5310_0001);
        let (ca, cb) = pair.establish().await;
        lossy_both_ways(&pair);

        const LEN: usize = 64 * 1024;
        let want = payload(LEN);

        let write_side = async {
            let bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");
            let mut writer = BufWriter::new(bi);
            let mut src = &want[..];
            let copied = within(tokio::io::copy(&mut src, &mut writer), "copy")
                .await
                .expect("copy into the BufWriter");
            assert_eq!(
                copied, LEN as u64,
                "tokio::io::copy reported {copied} bytes for a {LEN}-byte source"
            );
            within(writer.shutdown(), "shutdown")
                .await
                .expect("shutdown");
        };

        let read_side = async {
            let mut bi = within(cb.accept_bi(), "accept_bi")
                .await
                .expect("accept_bi");
            let mut got = Vec::new();
            within(bi.read_to_end(&mut got), "read_to_end")
                .await
                .expect("the peer read to a clean end of stream");

            // EOF is **sticky**: a second read after the end must be EOF
            // again, not `Pending` (a reader that hangs on a finished
            // stream) and not an error.
            let mut scratch = [0u8; 64];
            let n = within(bi.read(&mut scratch), "read after EOF")
                .await
                .expect("a read after EOF is Ok(0), not an error");
            assert_eq!(
                n, 0,
                "§16.2 / rulings 121+124: every read after end-of-stream is \
                 `Ok(0)` — a non-sticky build hangs a reader that loops until \
                 it sees EOF twice"
            );
            got
        };

        let ((), got) = tokio::join!(write_side, read_side);
        assert_same_bytes(&got, &want, "S31 copy round trip");
    })
    .await;
}

/// **S31, acceptance 3 (the mechanism) — `poll_shutdown` is `finish()` *and
/// then* `acked()`, not `finish()` alone.**
///
/// > `poll_shutdown` is `finish()` **and then** `acked()` (ruling 57), so
/// > `copy(…).await; shutdown().await` does not lose its tail.
///
/// # Why this is polled once rather than timed
///
/// `poll_finish` is documented **always `Ready` on the first poll** (§16.7
/// seals synchronously). So under the weak reading — `poll_shutdown =
/// poll_finish` — shutdown is `Ready` on its **first** poll, always. Under
/// ruling 57 it cannot be, because `poll_acked` cannot resolve until an
/// acknowledgement arrives. Blocking `b → a` makes an acknowledgement
/// *impossible*, which turns "is it Ready on the first poll" into an exact,
/// draw-independent separator between the two builds.
///
/// The path is blocked **before** the write, never after. After a write the
/// runtime may auto-advance virtual time while `write_all` awaits, an
/// acknowledgement may legitimately have arrived, and a **correct** build
/// would then be `Ready` on the first poll — the assertion would fail on the
/// good build, which is worse than not making it.
///
/// `a → b` stays open throughout, which is what lets the peer accept the
/// stream and what makes the final `read_to_end` a real check rather than a
/// second copy of the first assertion.
///
/// # BROKEN BUILD
///
/// * **`poll_shutdown` = `ready!(poll_finish(cx))` and stop** (ruling 57's
///   weak reading; `CONTRACT-8.md` §9 item 5). Returns `Ready(Ok(()))` on
///   the first poll with the data unacknowledged — caught by the `Pending`
///   assertion, deterministically.
/// * **`poll_shutdown` carrying a `shutdown_started: bool`** that skips
///   `poll_finish` on re-entry — caught by the second half, which drives the
///   *same* future to completion after healing. `CONTRACT-8.md` §3.1 forbids
///   the flag; a build that needs one to survive re-entry fails here.
/// * **A `poll_shutdown` that never completes** once an acknowledgement does
///   arrive — caught by [`within`] after the heal.
#[tokio::test(start_paused = true)]
async fn s31_poll_shutdown_is_finish_and_then_acked_not_finish_alone() {
    local(async {
        let pair = Pair::seeded(0x5310_0002);
        let (ca, cb) = pair.establish().await;

        let mut bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");

        // No acknowledgement can reach the writer from here on. `a → b` is
        // untouched, so the peer still learns of the stream and the data
        // still arrives — only the ACK cannot come back.
        pair.net.block_path(addr_b(), addr_a());

        let body = payload(4096);
        within(bi.write_all(&body), "write_all")
            .await
            .expect("write_all");
        settle().await;

        let mut peer = within(cb.accept_bi(), "accept_bi")
            .await
            .expect("accept_bi");

        {
            let mut shutdown = pin!(bi.shutdown());
            assert!(
                poll_once(shutdown.as_mut()).is_pending(),
                "ruling 57: `poll_shutdown` is `finish()` **and then** \
                 `acked()`. With `b → a` blocked nothing can have been \
                 acknowledged, so `Ready` here is `poll_shutdown` stopping at \
                 `finish()` — the weak reading that makes S28's silent tail \
                 loss reachable through `AsyncWrite`"
            );

            // Heal, and drive the *same* future to completion. Re-entering
            // `poll_shutdown` after a `Pending` must call `poll_finish` again
            // harmlessly (`CONTRACT-8.md` §3.1: "Do not add a
            // `shutdown_started: bool`").
            pair.net.heal_path(addr_b(), addr_a());
            within(shutdown, "shutdown after heal")
                .await
                .expect("shutdown completes once the data is acknowledged");
        }

        // And the data really did arrive, whole.
        let mut got = Vec::new();
        within(peer.read_to_end(&mut got), "peer read_to_end")
            .await
            .expect("clean end of stream");
        assert_same_bytes(&got, &body, "S31 shutdown payload");
    })
    .await;
}

/// **S31, acceptance 3 (the consequence) — `shutdown()` then `close()` loses
/// no tail, with loss injected.**
///
/// This is S28's guarantee reached through the `AsyncWrite` surface by an
/// application that never touches `acked()`. §15.2 permits `close()` to drop
/// stream and recovery state immediately, so a shutdown that resolves before
/// acknowledgement loses the last unacknowledged packets **at the path's loss
/// rate, silently** — the defect ruling 47 was written for, and exactly what
/// an `AsyncWrite` consumer cannot see.
///
/// # BROKEN BUILD
///
/// * **`poll_shutdown` = `poll_finish` alone.** `close()` then discards the
///   recovery state holding the dropped packets, they are never
///   retransmitted, and the reader either ends short (length assertion) or
///   never ends (caught by [`within`]).
/// * **A `close()` ordered before the shutdown's last retransmission** —
///   same signature.
///
/// The reader drains **concurrently**, which is the shape an application
/// actually has and is also the honest one: a reader that only starts after
/// the connection is dead would be asserting something about buffered
/// receive state, not about the tail.
#[tokio::test(start_paused = true)]
async fn s31_shutdown_then_close_does_not_lose_the_tail() {
    local(async {
        let pair = Pair::seeded(0x5310_0003);
        let (ca, cb) = pair.establish().await;
        lossy_both_ways(&pair);

        const LEN: usize = 48 * 1024;
        let want = payload(LEN);

        let write_side = async {
            let bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");
            let mut writer = BufWriter::new(bi);
            let mut src = &want[..];
            within(tokio::io::copy(&mut src, &mut writer), "copy")
                .await
                .expect("copy");
            within(writer.shutdown(), "shutdown")
                .await
                .expect("shutdown");

            // The line the test turns on: `close()` on the very next
            // statement, with no `acked()` anywhere in sight.
            ca.close(0, b"done").await;
        };

        let read_side = async {
            let mut bi = within(cb.accept_bi(), "accept_bi")
                .await
                .expect("accept_bi");
            let mut got = Vec::new();
            within(bi.read_to_end(&mut got), "read_to_end")
                .await
                .expect("the tail survived the close");
            // **[Integrator, ruling 241]** The handle is dropped here, on
            // purpose: read-to-EOF-then-drop is the canonical client shape,
            // and it is what found the defect.
            //
            // `SendStream::drop` on a live local end emits
            // `RESET_STREAM(NO_ERROR)`, so this reader resets the send
            // direction it never used. Ruling 165 latched **every** peer
            // reset into `peer_resets`, which only the *send* half reads, so
            // that reset reached the writer inside `shutdown()` and failed
            // it `Reset(0)` **after the payload had arrived whole**. Ruling
            // 241 scopes the latch to uni streams, where §9.8's
            // receiver-emitted reset actually means "the bytes were
            // discarded". If this test starts failing `Reset(0)` again, that
            // scoping has regressed — not this fixture.
            got
        };

        let ((), got) = tokio::join!(write_side, read_side);
        assert_same_bytes(&got, &want, "S31 tail after shutdown-then-close");
    })
    .await;
}

/// **S31, acceptance 3 (the bound) — `shutdown()` resolves *in error* if the
/// connection dies first, so it cannot hang past `DEAD_TIMEOUT`.**
///
/// Both halves are asserted, because either alone passes a wrong build:
/// *that* it eventually resolves (a `poll_acked` with no death path hangs for
/// ever) and *that* it had **not** resolved early (a `poll_shutdown` that
/// reports success without waiting satisfies "it resolved" for free).
///
/// # BROKEN BUILD
///
/// * **`poll_shutdown` = `poll_finish` alone** — resolves `Ok(())`
///   immediately; caught by [`still_pending`].
/// * **A `poll_acked` with no connection-death arm** — never resolves;
///   caught by [`within`] after the clock is advanced past `DEAD_TIMEOUT`.
/// * **A death mapped to the wrong `io::ErrorKind`.** `CONTRACT-8.md` §8.2
///   lifts `ConnectionLost::TimedOut` out to `io::ErrorKind::TimedOut`
///   precisely so a 25 s liveness death does not look like every other death
///   to a consumer whose only view is `io::Error`.
/// * **`io::Error::from(kind)` instead of `io::Error::new(kind, err)`**
///   (`CONTRACT-8.md` §2, §8.3) — `into_inner()` is `None` and the original
///   `WriteError` is unrecoverable. Asserted, because that is the difference
///   between S31's promise being satisfied "in kind but not in detail".
#[tokio::test(start_paused = true)]
async fn s31_shutdown_resolves_in_error_when_the_connection_dies_first() {
    local(async {
        let pair = Pair::seeded(0x5310_0004);
        let (ca, cb) = pair.establish().await;
        let _keep_peer = cb;

        let mut bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");

        // Silence the path in both directions *before* the write: nothing can
        // be acknowledged, and nothing can keep the connection alive.
        pair.net.block_path(addr_a(), addr_b());
        pair.net.block_path(addr_b(), addr_a());

        let body = payload(4096);
        within(bi.write_all(&body), "write_all")
            .await
            .expect("write_all");

        let mut shutdown = pin!(bi.shutdown());
        assert!(
            still_pending(shutdown.as_mut()).await,
            "ruling 57: `shutdown()` waits for acknowledgement — resolving \
             inside {NOT_BEFORE:?} on a fully blocked path is `poll_finish` \
             alone reporting success"
        );

        // The only thing that can free it now is the liveness deadline.
        tokio::time::advance(DEAD_TIMEOUT + Duration::from_secs(1)).await;

        let err = within(shutdown, "shutdown after DEAD_TIMEOUT")
            .await
            .expect_err("a dead connection resolves `shutdown()` in error");

        assert_eq!(
            err.kind(),
            std::io::ErrorKind::TimedOut,
            "CONTRACT-8.md §8.2: `ConnectionLost::TimedOut` maps to \
             `io::ErrorKind::TimedOut`, so a liveness death stays \
             distinguishable from every other death"
        );
        let inner = err
            .into_inner()
            .expect("CONTRACT-8.md §8.3: the original slither error is preserved");
        let inner = *inner
            .downcast::<WriteError>()
            .expect("the inner value is the `WriteError` itself");
        assert_eq!(
            inner,
            WriteError::ConnectionLost(ConnectionLost::TimedOut),
            "the recovered error names the death, not merely its kind"
        );
    })
    .await;
}

/// **S31, acceptance 2 — a peer's reset surfaces as `ConnectionReset`, not as
/// a silent truncation.**
///
/// > errors arrive as `io::Error`, and a peer's reset surfaces as
/// > `ConnectionReset` rather than as a silent truncation — the failure mode
/// > a byte-stream consumer cannot otherwise distinguish from a clean end.
///
/// The reset code (`7`) is recovered through `into_inner()`, because it is
/// **the one piece of information only `AsyncRead` can lose**: an
/// `io::ErrorKind` has no room for a `u64`, and the application-level reason
/// a peer reset a stream lives entirely in that code.
///
/// # BROKEN BUILD
///
/// * **`Ready(Err(e))` mapped to `Ready(Ok(()))` with nothing filled.** The
///   reader sees EOF and `read_to_end` returns `Ok` with the prefix — the
///   *silent truncation* the story names by name. Caught by `expect_err`.
/// * **`io::Error::from(io::ErrorKind::ConnectionReset)`** rather than
///   `io::Error::new(kind, err)` (`CONTRACT-8.md` §2/§8.3). `kind()` is right
///   and the code is gone; caught by the `into_inner()` half. A test that
///   asserted only `kind()` would pass this build, which is why both halves
///   are here.
/// * **A `From<ReadError>` that maps every variant to one kind** — caught by
///   this test's `ConnectionReset` together with the previous test's
///   `TimedOut`. Neither alone separates it.
///
/// The clean-EOF case is asserted in
/// [`s31_copy_through_a_bufwriter_then_shutdown_delivers_identical_bytes_and_eof`];
/// the two together are what make "distinguishable" mean anything.
#[tokio::test(start_paused = true)]
async fn s31_a_peer_reset_surfaces_as_connection_reset_not_a_silent_truncation() {
    local(async {
        let pair = Pair::seeded(0x5310_0005);
        let (ca, cb) = pair.establish().await;

        let bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");
        let (mut send, _recv) = bi.split();

        within(send.write(&payload(2048)), "write")
            .await
            .expect("write");
        settle().await;

        let mut peer = within(cb.accept_bi(), "accept_bi")
            .await
            .expect("accept_bi");

        send.reset(7);
        settle().await;

        let mut got = Vec::new();
        let err = within(peer.read_to_end(&mut got), "read_to_end after reset")
            .await
            .expect_err(
                "S31: a peer's reset must surface as an `io::Error`, not as a \
                 clean end of stream — the silent truncation a byte-stream \
                 consumer cannot distinguish from success",
            );

        assert_eq!(
            err.kind(),
            std::io::ErrorKind::ConnectionReset,
            "§16.11, ratified: `ReadError::Reset` maps to `ConnectionReset`"
        );
        let inner = err
            .into_inner()
            .expect("CONTRACT-8.md §8.3: the original `ReadError` is preserved");
        let inner = *inner
            .downcast::<ReadError>()
            .expect("the inner value is the `ReadError` itself");
        assert_eq!(
            inner,
            ReadError::Reset(7),
            "the reset **code** survives the conversion — an `io::ErrorKind` \
             has nowhere to put a `u64`, so losing it here loses it for good"
        );
    })
    .await;
}

/// **S31, "does not promise" — `flush()` is a no-op and is never delivery
/// confirmation (ruling 56).**
///
/// > `poll_flush` is a no-op returning `Ready` (ruling 56) — bytes accepted
/// > by `poll_write` are already in send state, and there is no shell buffer
/// > to push.
///
/// `CONTRACT-8.md` §3.1 makes this **unconditional**: "`Poll::Ready(Ok(()))`,
/// unconditionally. Touches nothing — not the cell, not the core, not the
/// driver." Both words are tested, because each separates a different build:
///
/// 1. **On a fully blocked path** — a `poll_flush` that waited for
///    acknowledgement (a second, weaker `acked()`, which is what ruling 56
///    forbids by name) cannot be `Ready` here.
/// 2. **On a dead connection** — a `poll_flush` that consulted the cell for
///    liveness returns `Err` here. "Touches nothing" means it does not.
///
/// # BROKEN BUILD
///
/// * **`poll_flush` delegating to `poll_acked`** — the "flush means delivery"
///   reading, which would put two verbs in competition and make `acked()` the
///   weaker of them. Caught by part 1, deterministically, because
///   [`poll_once`] advances no virtual time and no acknowledgement is even
///   possible.
/// * **`poll_flush` that returns `Err` once the connection is lost** — caught
///   by part 2.
///
/// A test that only checked that `flush().await` *resolves* would pass both
/// broken builds on a healthy connection; the blocked path and the corpse are
/// what make the assertion mean "no-op" rather than "eventually fine".
#[tokio::test(start_paused = true)]
async fn s31_flush_is_a_no_op_and_never_delivery_confirmation() {
    local(async {
        let pair = Pair::seeded(0x5310_0006);
        let (ca, cb) = pair.establish().await;
        let _keep_peer = cb;

        let mut bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");

        // ── 1. nothing can possibly have been delivered ────────────────
        pair.net.block_path(addr_a(), addr_b());
        pair.net.block_path(addr_b(), addr_a());
        within(bi.write_all(&payload(2048)), "write_all")
            .await
            .expect("write_all");

        {
            let flush = pin!(bi.flush());
            assert!(
                matches!(poll_once(flush), Poll::Ready(Ok(()))),
                "ruling 56: `poll_flush` is a no-op returning `Ready`. Pending \
                 on a blocked path is `flush` waiting for delivery — the \
                 second, weaker `acked()` ruling 56 exists to forbid"
            );
        }

        // ── 2. and it is still a no-op over a corpse ───────────────────
        tokio::time::advance(DEAD_TIMEOUT + Duration::from_secs(1)).await;
        settle().await;
        assert!(
            matches!(ca.closed().await, ConnectionLost::TimedOut),
            "fixture check: the connection is dead before part 2 runs"
        );

        {
            let flush = pin!(bi.flush());
            assert!(
                matches!(poll_once(flush), Poll::Ready(Ok(()))),
                "CONTRACT-8.md §3.1: `Ready(Ok(()))` **unconditionally** — \
                 `poll_flush` touches neither the cell nor the core, so a dead \
                 connection changes nothing about it"
            );
        }
    })
    .await;
}

/// **S31, the `Ok(Some(0))` / EOF collision — a read into an empty buffer is
/// not end-of-file (ruling 119).**
///
/// `CONTRACT-8.md` §3.2 hazard 1: the adapter short-circuits
/// `buf.remaining() == 0` **itself** and must never hand
/// `RecvStream::poll_read` an empty slice, because ruling 119 makes an empty
/// `buf` return `Ok(Some(0))` — documented to mean *park* — and once mapped
/// into a `ReadBuf` that filled nothing it is **indistinguishable from EOF**.
///
/// `Ok(Some(0))` = park and `Ok(None)` = end of stream is the distinction
/// ruling 119 exists to protect, and blind authors in this project have
/// guessed it wrong twice (round 18, and slice 4a). This is the surface where
/// the two collide.
///
/// # Why the assertion is the *next* read, not this one
///
/// A correct build and a build that forwards the empty slice **both** return
/// `Ready(Ok(()))` having filled nothing — that observation alone separates
/// nothing, and asserting only it would be a name without a pin. What
/// separates them is what happens **afterwards**: a build that latched the
/// empty read as end-of-stream answers the next real read with EOF and the
/// payload is gone for good.
///
/// # BROKEN BUILD
///
/// * **An adapter that latches `Ended::Eof` from an empty-buffer read** — the
///   next read returns `Ok(0)` and the data is unreachable. Caught.
/// * **An adapter that returns `Pending` for `remaining() == 0`** — the
///   caller parks on a waker nothing will ever fire, because no data is
///   needed to make progress. Caught by the `Ready` assertion.
/// * **An adapter that errors on an empty buffer.** Caught.
#[tokio::test(start_paused = true)]
async fn s31_a_read_into_an_empty_buffer_is_not_end_of_file() {
    local(async {
        let pair = Pair::seeded(0x5310_0007);
        let (ca, cb) = pair.establish().await;

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        let want = payload(1024);
        within(send.write(&want), "write").await.expect("write");
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        // The degenerate call, made while data is definitely available.
        let mut empty: [u8; 0] = [];
        let mut rb = ReadBuf::new(&mut empty);
        let mut cx = Context::from_waker(Waker::noop());
        // Fully qualified, and not `Pin::new(&mut recv).poll_read(..)`:
        // `RecvStream` already has an **inherent** `pub(crate) poll_read`
        // taking a `&mut [u8]`, and method resolution reaches an inherent
        // method before a trait one. Spelled the short way this call resolves
        // to the inherent method and fails as `E0624: method poll_read is
        // private` — which is what it did here before this comment existed.
        let r = <TestRecvStream as AsyncRead>::poll_read(Pin::new(&mut recv), &mut cx, &mut rb);
        assert!(
            matches!(r, Poll::Ready(Ok(()))),
            "CONTRACT-8.md §3.2: `remaining() == 0` is short-circuited to \
             `Ready(Ok(()))` without touching the handle — `Pending` parks the \
             caller on a waker nothing will fire, and `Err` is simply wrong"
        );
        assert_eq!(rb.filled().len(), 0, "an empty buffer cannot be filled");

        // ── the line the test turns on ─────────────────────────────────
        //
        // The payload must still be there. A build that read the zero-length
        // fill as end-of-stream has just lost 1 KiB silently.
        let mut got = Vec::new();
        within(
            recv.read_to_end(&mut got),
            "read_to_end after the empty read",
        )
        .await
        .expect("clean end of stream");
        assert_same_bytes(
            &got,
            &want,
            "ruling 119: a zero-length read is a park, not an end of stream — \
             a build that latched EOF here returns nothing",
        );
    })
    .await;
}

/// **S31 / `CONTRACT-8.md` §3.3 — `poll_shutdown` on a `BiStream` closes the
/// send half only.**
///
/// > `poll_shutdown` shuts down the send half only. It does not touch,
/// > abandon or reset the receive half — a half-closed `BiStream` is the
/// > shape `copy_bidirectional` and every request/response protocol relies
/// > on.
///
/// This is also the assertion `BiStream`'s own rustdoc warns is easy to fake:
/// a build "which implemented `Drop` on `BiStream` and forgot one half *would
/// pass any test that checked only the other*". Here the *receive* half is
/// checked **after** the send half is shut down, and the response is written
/// strictly after that shutdown — the ordering that catches it.
///
/// # BROKEN BUILD
///
/// * **`poll_shutdown` delegating to both halves** — the receive half is
///   reset or abandoned and the peer's response never arrives. A build that
///   had merely buffered an earlier response cannot pass, because there is no
///   earlier response.
/// * **A `Drop` added to a wrapper around `BiStream`** (`CONTRACT-8.md` §9
///   item 10) that resets the receive half.
/// * **`AsyncRead` on `BiStream` delegating to the send half** (or vice
///   versa) — a transposition a same-direction test cannot see.
#[tokio::test(start_paused = true)]
async fn s31_bistream_shutdown_closes_the_send_half_only() {
    local(async {
        let pair = Pair::seeded(0x5310_0008);
        let (ca, cb) = pair.establish().await;

        let request = payload(512);
        let response = payload(777);
        let want_request = request.clone();
        let want_response = response.clone();

        let client = async {
            let mut bi = within(ca.open_bi(), "open_bi").await.expect("open_bi");
            within(bi.write_all(&request), "write request")
                .await
                .expect("write");
            within(bi.shutdown(), "shutdown").await.expect("shutdown");

            // Half-closed: we have finished sending and must still be able to
            // receive.
            let mut got = Vec::new();
            within(bi.read_to_end(&mut got), "read response")
                .await
                .expect("the receive half survived `shutdown()`");
            got
        };

        let server = async {
            let mut bi = within(cb.accept_bi(), "accept_bi")
                .await
                .expect("accept_bi");
            let mut got = Vec::new();
            within(bi.read_to_end(&mut got), "read request")
                .await
                .expect("the client's `shutdown()` delivered a clean EOF");

            // Written strictly after the client shut its send half down.
            within(bi.write_all(&response), "write response")
                .await
                .expect("write");
            within(bi.shutdown(), "server shutdown")
                .await
                .expect("shutdown");
            got
        };

        let (got_response, got_request) = tokio::join!(client, server);
        assert_same_bytes(&got_request, &want_request, "S31 half-close request");
        assert_same_bytes(&got_response, &want_response, "S31 half-close response");
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// Ruling 226 — the `Result`-carrying adapters never end
// ══════════════════════════════════════════════════════════════════════

/// Assert one adapter parks on a **live** connection with nothing available.
///
/// This is the half that makes the never-ending assertion mean something.
/// Without it, an adapter whose `poll_next` were `Ready(Some(Err(lost)))`
/// unconditionally — one that never delivers anything at all — would satisfy
/// "it never returns `None`" for free. Working rule 9: assert from the side
/// that separates.
macro_rules! assert_pending_while_alive {
    ($adapter:expr, $name:literal) => {{
        let mut s = $adapter;
        assert!(
            poll_next_once(&mut s).is_pending(),
            "{}: on a live connection with nothing available the adapter \
             parks. `Ready` here is an adapter that answers without claiming, \
             and it would satisfy the never-ending assertion for free",
            $name
        );
    }};
}

/// Assert one adapter never ends after the connection dies (ruling 226).
macro_rules! assert_never_ends {
    ($adapter:expr, $name:literal) => {{
        let mut s = $adapter;
        for poll in 1..=3 {
            match next(&mut s).await {
                Some(Err(e)) => assert!(
                    matches!(e, ConnectionLost::PeerClosed { .. }),
                    "{}: poll {poll} reported {e:?}, not the death that \
                     actually happened — ruling 226 keeps the **reason**, \
                     which is the only thing a consumer draining this stream \
                     ever learns about why it stopped",
                    $name
                ),
                None => panic!(
                    "{}: poll {poll} returned `None`. **Ruling 226**: the \
                     `Result`-carrying adapters never end — they yield \
                     `Some(Err(ConnectionLost))` for as long as they are \
                     polled. A build that ends discards the death reason, and \
                     an adapter carrying a `finished` flag is exactly what \
                     ruling 226 declined",
                    $name
                ),
                Some(Ok(_)) => panic!(
                    "{}: poll {poll} yielded an item on a dead connection",
                    $name
                ),
            }
        }
    }};
}

/// **Ruling 226 — every `Result`-carrying `Stream` adapter yields
/// `Some(Err(ConnectionLost))` for ever and never `None`.**
///
/// > After the connection dies they yield `Some(Err(ConnectionLost))` for as
/// > long as they are polled, never `None`. A bare
/// > `while let Some(_) = s.next().await` **spins**.
///
/// This is ruling 119's class — the convention whose inversion hangs (or
/// here, spins) a consumer for ever — and `CONTRACT-8.md` §4.2 records that
/// blind authors have guessed this class wrong twice in this project. It is
/// asserted for **all five** adapters rather than one, because a "finished"
/// flag is added per type and a build that got four right and one wrong is
/// the likely shape.
///
/// Each adapter is polled **three** times after death, not once: the wrong
/// build is "`Some(Err)` once, then `None`", which a single poll cannot see.
///
/// # BROKEN BUILD
///
/// * **An adapter carrying a `finished: bool`** that yields the error once
///   and then `None` — `CONTRACT-8.md` §4.2 calls it "the better ergonomics
///   and the worse fidelity", and ruling 226 declined it. Caught on poll 2.
/// * **An adapter that ends immediately on death**, discarding the reason
///   entirely. Caught on poll 1.
/// * **An adapter that reports a death other than the one that happened** —
///   caught by the `PeerClosed` match, which separates a build that
///   synthesises a generic `ConnectionLost` from one that propagates the
///   value the connection actually died of.
/// * **An adapter that never yields anything** — would pass a never-`None`
///   assertion for free; caught by
///   [`r226_adapters_park_rather_than_answer_while_the_connection_lives`].
#[tokio::test(start_paused = true)]
async fn r226_result_carrying_adapters_never_end_after_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x2260_0001);
        let (ca, cb) = pair.establish().await;

        cb.close(9, b"bye").await;
        settle().await;
        assert!(
            matches!(
                ca.closed().await,
                ConnectionLost::PeerClosed { code: 9, .. }
            ),
            "fixture check: the connection died of a peer close before the \
             adapters are polled"
        );

        assert_never_ends!(ca.messages(), "messages()");
        assert_never_ends!(ca.datagrams(), "datagrams()");
        assert_never_ends!(ca.incoming_bi(), "incoming_bi()");
        assert_never_ends!(ca.incoming_uni(), "incoming_uni()");
        assert_never_ends!(ca.notifications(), "notifications()");
    })
    .await;
}

/// **The separator for ruling 226 — a live adapter parks rather than
/// answering.**
///
/// See [`assert_pending_while_alive`]. An adapter that returned
/// `Ready(Some(Err(..)))` unconditionally would satisfy "never `None`"
/// without ever being a working stream, and a suite that asserted only the
/// dead-connection half would ship it green.
///
/// # BROKEN BUILD
///
/// * **An adapter wired to the wrong `poll_*`** — `incoming_uni()` backed by
///   `poll_accept_bi`, say — answers `Ready` for a supply that is not its own.
/// * **An adapter that reports the connection lost while it is alive.**
/// * **A prefetching adapter** that has already claimed and queued an item:
///   it answers `Ready` here where a correct one parks. (The stronger pin on
///   ruling 58 is in `tests/story_codec.rs`.)
#[tokio::test(start_paused = true)]
async fn r226_adapters_park_rather_than_answer_while_the_connection_lives() {
    local(async {
        let pair = Pair::seeded(0x2260_0002);
        let (ca, cb) = pair.establish().await;
        let _keep_peer = cb;

        settle().await;

        assert_pending_while_alive!(ca.messages(), "messages()");
        assert_pending_while_alive!(ca.datagrams(), "datagrams()");
        assert_pending_while_alive!(ca.incoming_bi(), "incoming_bi()");
        assert_pending_while_alive!(ca.incoming_uni(), "incoming_uni()");
        assert_pending_while_alive!(ca.notifications(), "notifications()");
    })
    .await;
}

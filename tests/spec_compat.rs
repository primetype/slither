//! **Slice 8's pinned invariants, asserted by direct polling.**
//!
//! These are not capability stories. Every assertion here names, in its doc
//! comment, **the broken build it catches** — working rule 9: *a bound is
//! only a test if the degenerate case violates it, and a name is not a pin.*
//! The instrument is [`std::task::Waker::noop`] plus `Poll::Pending` /
//! `Poll::Ready` matching, not a combinator and not a simulated network,
//! because the assertions are about *how many times* and *in what state* an
//! adapter touches the verb underneath it. Where a network is unavoidable it
//! is [`slither::testutil`] on tokio's **paused clock**; there is no `sleep`.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind spec/invariant author for slice 8** from
//! `SPEC.md` §16.11 and §16.11.1, `.slices/08-composability/CONTRACT-8.md`,
//! and rulings 55–58, 61, 110, 119, 121, 225–231 **alone**, while the
//! implementer wrote `src/compat/` concurrently. Base commit `704a4ae`,
//! verified with `git log --oneline -1` as the first command. No file under
//! `src/compat/` existed in this worktree, and none was read.
//!
//! # Sources that disagree, reported rather than resolved (working rule 3)
//!
//! 1. **`CONTRACT-8.md` §8's ⚠ banner is stale.** It still opens *"This
//!    section is the one part of the contract that is not fully derivable
//!    from a ratified source"* and marks 15 of its 17 rows "recommended".
//!    `CONTRACT-8.md` §0.0 consequence 1 says that banner is obsolete and
//!    the tables are `SPEC.md` §16.11.1, ratified by ruling 227. §0.0 wins
//!    by its own preamble, and the two tables agree row-for-row — checked
//!    here, all 17 — so nothing is at stake but the banner. Reported, not
//!    edited: the contract is not this author's file.
//!
//! 2. **Ruling 58 and `SPEC.md`:6284 both cite a pin that does not exist.**
//!    Ruling 58 ends *"Pinned by Appendix B"*; §16.11 says *"Appendix B pins
//!    it"*. Appendix B is `SPEC.md`:6936–7426 and contains **no**
//!    composability obligation at all — no `adapter`, no `compos`, no
//!    `16.11`, no `prefetch`, no `poll_next`, no `AsyncRead`/`AsyncWrite`.
//!    The only hits for "claim" are §6.7's stream claims and hint routing.
//!    This is working rule 11's shape in the maintainer's own text — *a
//!    rationale must name a mechanism that exists* — and it went unnoticed
//!    because ruling 58 is quoted far more often than Appendix B is opened.
//!    [`a_stream_adapter_claims_at_most_one_message_per_poll`] is the pin
//!    those two sentences promise.
//!
//! 3. **`Incoming`'s documented `None` looks unreachable in safe code.**
//!    `CONTRACT-8.md` §4.2 and `SPEC.md` §16.11 both say `Incoming` *does*
//!    end, because `Endpoint::accept()` yields `None` when *"the endpoint is
//!    closed — the driver has stopped"*. `driver_stopped` is set only by
//!    `Driver::stop`, which runs when the driver task ends — i.e. when the
//!    last `Endpoint` handle is dropped (or the driver panics). But ruling
//!    231 makes `Incoming<'a, I>` **borrow** `&'a Endpoint<I>`, and
//!    `Endpoint` is not `Clone` (`CONTRACT-8.md` §0), so while an `Incoming`
//!    exists the endpoint cannot be dropped and the driver cannot stop.
//!    There is no `Endpoint::close()`. So the terminating condition cannot
//!    be produced while the adapter that observes it is alive. Rulings 229
//!    and 231 were taken in the same round and neither cites the other on
//!    this point. **Reported, not resolved** — it may be intended (the
//!    `None` arm is then a defensive one), but a blind author cannot tell,
//!    and no test here can reach it. What *is* pinned is the type:
//!    [`adapter_item_types`] fails to compile if `Incoming`'s `Item` becomes
//!    a `Result`, which is how a build that applied ruling 226 uniformly
//!    would spell "never ends".
//!
//! # What is asserted about the source text rather than about behaviour
//!
//! Three obligations in this slice are **not reachable through any value**:
//! ruling 227's *"the `_ =>` arm must not be `unreachable!()`"* cannot be
//! observed without a `WriteError` variant that does not exist yet, and
//! `CONTRACT-8.md` §9's *"no `VecDeque`, no spawning drainer"* is a
//! statement about the shape of a struct. Those three are asserted by
//! reading `src/compat/**/*.rs` — see [`compat_source_scan`]'s module
//! section. The scan is deliberately narrow and strips `//` comments first,
//! so the contract's own prohibitions quoted in rustdoc do not trip it.

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_util::sink::Sink;
use futures_util::stream::Stream;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, ReadBuf};

use slither::constants::INITIAL_MAX_STREAM_DATA;
use slither::testutil::{Pair, local, settle};
use slither::{
    ConnectionLost, DatagramError, Identity, Intro, Notification, ReadError, WriteError,
};

// ══════════════════════════════════════════════════════════════════════
// Instruments
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve. On the paused
/// clock a resolvable future costs no wall time at all, so this is generous
/// on purpose: it turns a hang into a named failure instead of a dead suite.
const PATIENCE: Duration = Duration::from_secs(5);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// A `Context` over [`Waker::noop`] — stable within the 1.96 MSRV, so the
/// ruling-58 pin needs no dev-dependency.
///
/// A no-op waker is the right instrument precisely *because* it drops
/// wakeups: these tests assert what an adapter did during **one** call, and
/// a real waker would let a driver turn slip in between two assertions.
fn noop_context() -> Context<'static> {
    Context::from_waker(Waker::noop())
}

/// Poll a `Stream` **exactly once**.
fn poll_stream_once<S: Stream + Unpin>(s: &mut S) -> Poll<Option<S::Item>> {
    let mut cx = noop_context();
    Pin::new(s).poll_next(&mut cx)
}

/// Poll a `Future` **exactly once**. The instrument for "*immediately*".
fn poll_future_once<F: Future>(mut f: Pin<&mut F>) -> Poll<F::Output> {
    let mut cx = noop_context();
    f.as_mut().poll(&mut cx)
}

fn poll_read_once<R: AsyncRead + Unpin>(r: &mut R, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
    let mut cx = noop_context();
    Pin::new(r).poll_read(&mut cx, buf)
}

fn poll_write_once<W: AsyncWrite + Unpin>(w: &mut W, buf: &[u8]) -> Poll<io::Result<usize>> {
    let mut cx = noop_context();
    Pin::new(w).poll_write(&mut cx, buf)
}

fn poll_flush_once<W: AsyncWrite + Unpin>(w: &mut W) -> Poll<io::Result<()>> {
    let mut cx = noop_context();
    Pin::new(w).poll_flush(&mut cx)
}

fn poll_shutdown_once<W: AsyncWrite + Unpin>(w: &mut W) -> Poll<io::Result<()>> {
    let mut cx = noop_context();
    Pin::new(w).poll_shutdown(&mut cx)
}

/// Read into a fresh 64-byte buffer and report `(ready, filled)`.
fn read_into<R: AsyncRead + Unpin>(r: &mut R, store: &mut [u8]) -> Poll<io::Result<usize>> {
    let mut rb = ReadBuf::new(store);
    match poll_read_once(r, &mut rb) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
        Poll::Ready(Ok(())) => Poll::Ready(Ok(rb.filled().len())),
    }
}

// ══════════════════════════════════════════════════════════════════════
// 1. Ruling 58 — an adapter never claims ahead of its consumer
// ══════════════════════════════════════════════════════════════════════

/// **The most important test in the slice.** Ruling 58, §16.11: a `Stream`
/// adapter claims *at most one item, and only from inside `poll_next`*.
///
/// # The broken build this catches
///
/// **A prefetching adapter** — one that loops to drain, keeps a `VecDeque`
/// or `Option<Item>` receive slot, or spawns a read-ahead task. Ruling 58
/// calls this *"the easiest way to get the composability layer wrong"*
/// because it *"look[s] like an ordinary ergonomic convenience"* while
/// rebuilding the unbounded shell queue §10.6 forbids.
///
/// # Why the obvious test does not catch it
///
/// Polling a `Stream` once and asserting *one item arrived* **passes a
/// prefetching adapter** — it also yields one item; it has merely stolen the
/// rest. So three messages are sent, `messages()` is polled **exactly
/// once**, and the assertion is made on the *verb underneath*:
/// `recv_message()` must be `Ready` with the **second** message on its very
/// first poll. A prefetching adapter has already claimed two and three, so
/// that call finds an empty queue and parks — `Poll::Pending`, and the test
/// fails on the side that separates the two builds.
///
/// The adapter is deliberately **still alive** across that assertion. If it
/// were dropped first, the test would also pass a build that prefetches and
/// then hands everything back on drop, which is not what ruling 58 says.
#[tokio::test(start_paused = true)]
async fn a_stream_adapter_claims_at_most_one_message_per_poll() {
    local(async {
        let pair = Pair::seeded(0x58);
        let (a, b) = pair.establish().await;

        for m in [&b"one"[..], &b"two"[..], &b"three"[..]] {
            within(b.send_message(m), "send_message")
                .await
                .expect("send_message");
        }
        // Reliable and ordered (§11): after the drivers have had their turns
        // all three are queued in `a`'s core, unclaimed.
        for _ in 0..4 {
            settle().await;
        }

        let mut msgs = a.messages();

        // Exactly one poll. Not `next().await`, which polls until Ready and
        // would blur "how many times" — the whole assertion.
        match poll_stream_once(&mut msgs) {
            Poll::Ready(Some(Ok(m))) => assert_eq!(
                m.as_slice(),
                b"one",
                "the adapter must yield the messages in the order the verb does"
            ),
            Poll::Ready(Some(Err(e))) => panic!("messages() failed: {e:?}"),
            Poll::Ready(None) => panic!("messages() ended on a live connection (ruling 226)"),
            Poll::Pending => {
                panic!(
                    "messages() parked with three messages queued — fixture problem, not ruling 58"
                )
            }
        }

        // `msgs` is still alive and still borrowing `a`. Both verbs take
        // `&self`, so the two shared borrows coexist.
        let recv = pin!(a.recv_message());
        match poll_future_once(recv) {
            Poll::Ready(Ok(m)) => assert_eq!(
                m.as_slice(),
                b"two",
                "ruling 58: one `poll_next` must claim exactly one item, so the verb \
                 underneath still owns messages two and three"
            ),
            Poll::Pending => panic!(
                "RULING 58 VIOLATED: one `poll_next` claimed more than one item. \
                 `recv_message()` parked, so the adapter has swallowed messages two \
                 and three into an intermediate queue — the unbounded shell queue \
                 §10.6 forbids, which is what ruling 58 exists to prevent."
            ),
            Poll::Ready(Err(e)) => panic!("recv_message() failed: {e:?}"),
        }

        drop(msgs);
    })
    .await;
}

/// Ruling 58 over the **unreliable** face, where §11.1 promises no ordering
/// at all — so the assertion is *that an item remains*, not which one.
///
/// # The broken build this catches
///
/// The same prefetching adapter as above, built only on `datagrams()`. A
/// build could plausibly get `Messages` right and `Datagrams` wrong: the
/// datagram queue is bounded and drop-oldest (§11.3), which makes "drain it
/// while we're here" look *more* defensible rather than less.
///
/// The separating side is `recv_datagram()` being **`Ready` on its first
/// poll**: a prefetcher has taken all three, so it parks.
#[tokio::test(start_paused = true)]
async fn a_stream_adapter_claims_at_most_one_datagram_per_poll() {
    local(async {
        let pair = Pair::seeded(0x59);
        let (a, b) = pair.establish().await;

        for d in [&b"alpha"[..], &b"bravo"[..], &b"charlie"[..]] {
            b.send_datagram(d).expect("send_datagram");
        }
        for _ in 0..4 {
            settle().await;
        }

        let mut dgrams = a.datagrams();

        let first = match poll_stream_once(&mut dgrams) {
            Poll::Ready(Some(Ok(d))) => d,
            Poll::Ready(Some(Err(e))) => panic!("datagrams() failed: {e:?}"),
            Poll::Ready(None) => panic!("datagrams() ended on a live connection (ruling 226)"),
            Poll::Pending => panic!("datagrams() parked with three queued — fixture problem"),
        };

        let recv = pin!(a.recv_datagram());
        match poll_future_once(recv) {
            Poll::Ready(Ok(d)) => assert_ne!(
                d, first,
                "the verb re-delivered the datagram the adapter already claimed"
            ),
            Poll::Pending => panic!(
                "RULING 58 VIOLATED on `datagrams()`: `recv_datagram()` parked, so one \
                 `poll_next` claimed more than one datagram."
            ),
            Poll::Ready(Err(e)) => panic!("recv_datagram() failed: {e:?}"),
        }

        drop(dgrams);
    })
    .await;
}

/// Ruling 58 over an **accept** face, where the item is a handle rather than
/// a buffer.
///
/// `incoming_bi` is used rather than `incoming_uni` deliberately:
/// `CONTRACT-8.md` §4.6 (ruling 51, S30) makes drawing from `messages()` and
/// `incoming_uni()` on one connection *normatively a programming error*, and
/// a test suite should not model the shape its own docs forbid.
///
/// # The broken build this catches
///
/// A prefetching `IncomingBi`. This one is the most costly version of the
/// defect: a claimed-but-unconsumed `BiStream` is a **live handle** whose
/// `Drop` resets the stream, so a prefetcher does not merely delay streams
/// two and three — dropping the adapter destroys them. `accept_bi()` parking
/// is the separating observation.
#[tokio::test(start_paused = true)]
async fn a_stream_adapter_claims_at_most_one_incoming_stream_per_poll() {
    local(async {
        let pair = Pair::seeded(0x5A);
        let (a, b) = pair.establish().await;

        // Three bidirectional streams, each announced by a byte of data so
        // the peer certainly sees all three before anything is claimed.
        let mut openers = Vec::new();
        for tag in [1u8, 2, 3] {
            let mut s = within(b.open_bi(), "open_bi").await.expect("open_bi");
            within(s.write_all(&[tag]), "write_all")
                .await
                .expect("write_all");
            openers.push(s);
        }
        for _ in 0..4 {
            settle().await;
        }

        let mut incoming = a.incoming_bi();

        match poll_stream_once(&mut incoming) {
            Poll::Ready(Some(Ok(_first))) => {}
            Poll::Ready(Some(Err(e))) => panic!("incoming_bi() failed: {e:?}"),
            Poll::Ready(None) => panic!("incoming_bi() ended on a live connection (ruling 226)"),
            Poll::Pending => panic!("incoming_bi() parked with three streams pending — fixture"),
        }

        let accept = pin!(a.accept_bi());
        match poll_future_once(accept) {
            Poll::Ready(Ok(_second)) => {}
            Poll::Pending => panic!(
                "RULING 58 VIOLATED on `incoming_bi()`: `accept_bi()` parked, so one \
                 `poll_next` claimed more than one stream. A claimed-but-unconsumed \
                 `BiStream` is a live handle whose `Drop` resets the stream."
            ),
            Poll::Ready(Err(e)) => panic!("accept_bi() failed: {e:?}"),
        }

        drop(incoming);
        drop(openers);
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 2. Ruling 227 / §16.11.1 — the `io::ErrorKind` table, row by row
// ══════════════════════════════════════════════════════════════════════

/// Every `ConnectionLost` variant, once. Seven, and the table has seven
/// rows for it — working rule 8: the list is read as exhaustive.
fn all_connection_lost() -> Vec<ConnectionLost> {
    vec![
        ConnectionLost::TimedOut,
        ConnectionLost::NonceExhausted,
        ConnectionLost::LocallyClosed,
        ConnectionLost::PeerClosed {
            code: 7,
            reason: b"bye".to_vec(),
        },
        ConnectionLost::ProtocolViolation { code: 9 },
        ConnectionLost::Replaced,
        ConnectionLost::EndpointDropped,
    ]
}

/// §16.11.1's **read** column, transcribed. Eight rows: `Finished` is not a
/// `ReadError`.
fn read_table() -> Vec<(ReadError, io::ErrorKind)> {
    use io::ErrorKind as K;
    vec![
        (ReadError::Reset(7), K::ConnectionReset),
        (ConnectionLost::TimedOut.into(), K::TimedOut),
        (ConnectionLost::NonceExhausted.into(), K::ConnectionAborted),
        (ConnectionLost::LocallyClosed.into(), K::NotConnected),
        (
            ConnectionLost::PeerClosed {
                code: 7,
                reason: b"bye".to_vec(),
            }
            .into(),
            K::ConnectionAborted,
        ),
        (
            ConnectionLost::ProtocolViolation { code: 9 }.into(),
            K::ConnectionAborted,
        ),
        (ConnectionLost::Replaced.into(), K::ConnectionAborted),
        (ConnectionLost::EndpointDropped.into(), K::NotConnected),
    ]
}

/// §16.11.1's **write** column, transcribed. Nine rows.
fn write_table() -> Vec<(WriteError, io::ErrorKind)> {
    use io::ErrorKind as K;
    vec![
        (WriteError::Reset(7), K::ConnectionReset),
        (WriteError::Finished, K::BrokenPipe),
        (ConnectionLost::TimedOut.into(), K::TimedOut),
        (ConnectionLost::NonceExhausted.into(), K::BrokenPipe),
        (ConnectionLost::LocallyClosed.into(), K::NotConnected),
        (
            ConnectionLost::PeerClosed {
                code: 7,
                reason: b"bye".to_vec(),
            }
            .into(),
            K::BrokenPipe,
        ),
        (
            ConnectionLost::ProtocolViolation { code: 9 }.into(),
            K::BrokenPipe,
        ),
        (ConnectionLost::Replaced.into(), K::BrokenPipe),
        (ConnectionLost::EndpointDropped.into(), K::NotConnected),
    ]
}

/// §16.11.1's read column, **row by row**.
///
/// # The broken build this catches
///
/// Any build that collapses the column: the pre-227 literal reading of
/// §16.11 gave `Reset → ConnectionReset` and a *slash* for everything else,
/// so the two natural collapses are "all `ConnectionLost` → `NotConnected`"
/// and "everything not a reset → `Other`". Both fail here on the first
/// non-matching row. `TimedOut` is the row that matters most: ruling 227
/// lifts it out of both columns *"because `io::ErrorKind::TimedOut` exists
/// and a `DEAD_TIMEOUT` death is what it names; collapsing it makes every
/// death look alike to a consumer whose only view is `io::Error`."*
#[test]
fn read_error_kinds_match_the_ratified_table() {
    for (err, want) in read_table() {
        let got = io::Error::from(err.clone());
        assert_eq!(
            got.kind(),
            want,
            "§16.11.1 read row for {err:?}: expected {want:?}, got {:?}",
            got.kind()
        );
    }
}

/// §16.11.1's write column, **row by row**.
///
/// # The broken build this catches
///
/// The same collapses, plus the one §16.11 was silent about before ruling
/// 227: **`WriteError::Finished`**. A build that never considered it reaches
/// the `#[non_exhaustive]` fallback and answers `Other`, and write-after-
/// finish — the archetypal broken pipe — stops being recognisable to any
/// consumer whose only view is `io::Error`.
#[test]
fn write_error_kinds_match_the_ratified_table() {
    for (err, want) in write_table() {
        let got = io::Error::from(err.clone());
        assert_eq!(
            got.kind(),
            want,
            "§16.11.1 write row for {err:?}: expected {want:?}, got {:?}",
            got.kind()
        );
    }
}

/// Ruling 227's **rule**, rather than its table: the slash in §16.11 is a
/// *variant* split, not a read/write *direction* split.
///
/// # Why this exists next to the two row-by-row tests
///
/// The row tests would catch a direction-split build, but only as eight
/// unexplained row failures. This one states the rule, and it is **two
/// sided**, which is the whole of working rule 9:
///
/// * a build that split by **direction** (every read one kind, every write
///   another) fails the `same` half — `Reset`, `TimedOut`, `LocallyClosed`
///   and `EndpointDropped` must map **identically** in both directions;
/// * a build that ignored direction entirely (one shared `impl`, or a
///   `From<ConnectionLost>` both delegate to) fails the `differ` half —
///   `NonceExhausted`, `PeerClosed`, `ProtocolViolation` and `Replaced` are
///   `ConnectionAborted` on read and `BrokenPipe` on write.
///
/// Neither half alone separates the three builds; together they do.
#[test]
fn the_kind_split_is_by_variant_not_by_direction() {
    use io::ErrorKind as K;

    let same = [
        (ConnectionLost::TimedOut, K::TimedOut),
        (ConnectionLost::LocallyClosed, K::NotConnected),
        (ConnectionLost::EndpointDropped, K::NotConnected),
    ];
    for (lost, want) in same {
        let r = io::Error::from(ReadError::from(lost.clone()));
        let w = io::Error::from(WriteError::from(lost.clone()));
        assert_eq!(r.kind(), want, "§16.11.1: read {lost:?}");
        assert_eq!(
            w.kind(),
            want,
            "§16.11.1: {lost:?} maps identically in both directions — ruling 227 reads \
             §16.11's slash as a variant split, so a direction split is wrong here"
        );
    }
    // `Reset` carries a code and is not a `ConnectionLost`; same rule.
    assert_eq!(
        io::Error::from(ReadError::Reset(1)).kind(),
        K::ConnectionReset
    );
    assert_eq!(
        io::Error::from(WriteError::Reset(1)).kind(),
        K::ConnectionReset
    );

    let differ = [
        ConnectionLost::NonceExhausted,
        ConnectionLost::PeerClosed {
            code: 3,
            reason: Vec::new(),
        },
        ConnectionLost::ProtocolViolation { code: 4 },
        ConnectionLost::Replaced,
    ];
    for lost in differ {
        let r = io::Error::from(ReadError::from(lost.clone()));
        let w = io::Error::from(WriteError::from(lost.clone()));
        assert_eq!(r.kind(), K::ConnectionAborted, "§16.11.1: read {lost:?}");
        assert_eq!(w.kind(), K::BrokenPipe, "§16.11.1: write {lost:?}");
        assert_ne!(
            r.kind(),
            w.kind(),
            "§16.11.1: {lost:?} must differ by direction — a single shared conversion, or \
             one `From<ConnectionLost>` both arms delegate to, collapses this row"
        );
    }
}

/// §16.11.1 / `CONTRACT-8.md` §8.3: *"every arm constructs
/// `io::Error::new(kind, err)`"*, so the original downcasts back out.
///
/// # The broken build this catches
///
/// `io::Error::from(kind)` — the one-liner that satisfies every `ErrorKind`
/// assertion above and returns `None` from `into_inner()`. Under it a stream
/// reset's `u64` **code** is unrecoverable, because the `AsyncRead` surface
/// is the only place it appears, and S31's *"a reset surfaces as
/// `ConnectionReset` rather than a silent truncation"* is satisfied in kind
/// and not in detail. `PeerClosed`'s `code` and `reason` go the same way.
#[test]
fn the_inner_read_error_is_preserved_with_its_payload() {
    for (err, _) in read_table() {
        let io_err = io::Error::from(err.clone());
        let inner = io_err
            .into_inner()
            .unwrap_or_else(|| panic!("§8.3: {err:?} lost its inner error — `io::Error::new` ?"));
        let recovered = *inner
            .downcast::<ReadError>()
            .unwrap_or_else(|_| panic!("§8.3: {err:?} did not downcast back to `ReadError`"));
        assert_eq!(
            recovered, err,
            "§8.3: the payload must survive the round trip"
        );
    }

    // Named explicitly because the code is the part only `AsyncRead` can
    // reach, and `u64::MAX` is the value a truncating conversion mangles.
    let io_err = io::Error::from(ReadError::Reset(u64::MAX));
    let inner = *io_err
        .into_inner()
        .expect("inner error")
        .downcast::<ReadError>()
        .expect("downcast");
    assert_eq!(inner, ReadError::Reset(u64::MAX));
}

/// The write half of §8.3, including `Finished` and a full-width code.
#[test]
fn the_inner_write_error_is_preserved_with_its_payload() {
    for (err, _) in write_table() {
        let io_err = io::Error::from(err.clone());
        let inner = io_err
            .into_inner()
            .unwrap_or_else(|| panic!("§8.3: {err:?} lost its inner error"));
        let recovered = *inner
            .downcast::<WriteError>()
            .unwrap_or_else(|_| panic!("§8.3: {err:?} did not downcast back to `WriteError`"));
        assert_eq!(
            recovered, err,
            "§8.3: the payload must survive the round trip"
        );
    }

    let io_err = io::Error::from(WriteError::Reset(u64::MAX));
    let inner = *io_err
        .into_inner()
        .expect("inner error")
        .downcast::<WriteError>()
        .expect("downcast");
    assert_eq!(inner, WriteError::Reset(u64::MAX));
}

/// The conversion is **total and non-panicking** over every constructible
/// variant, and the two enums are not confusable at the downcast.
///
/// # The broken build this catches
///
/// A conversion that panics on some arm — and, separately, one that boxes
/// the *wrong* type, so `downcast::<ReadError>()` succeeds on a value that
/// came from a `WriteError`. The second is not hypothetical: both arms are
/// written in the same `impl` block minutes apart.
///
/// # What is *not* reachable from here, and why
///
/// Ruling 227's *"the `_ =>` arm maps to `Other` and **must not** be
/// `unreachable!()`"* cannot be observed from any value: the arm exists for
/// a variant `WriteError` does not have yet (§19 reserves `Stopped`, ruling
/// 61), and no test can construct one. `src/error.rs`'s own fence records
/// the same limitation from the other side — `#[non_exhaustive]` has no
/// effect *within* the defining crate, and full effect here in `tests/`,
/// which is why the matches below are written with wildcards they cannot
/// avoid. [`compat_source_scan`] is the only available instrument.
#[test]
fn the_conversions_are_total_and_not_confusable() {
    for lost in all_connection_lost() {
        let r = io::Error::from(ReadError::from(lost.clone()));
        assert!(
            r.into_inner()
                .expect("inner")
                .downcast::<WriteError>()
                .is_err(),
            "a `ReadError` conversion boxed a `WriteError` for {lost:?}"
        );
        let w = io::Error::from(WriteError::from(lost.clone()));
        assert!(
            w.into_inner()
                .expect("inner")
                .downcast::<ReadError>()
                .is_err(),
            "a `WriteError` conversion boxed a `ReadError` for {lost:?}"
        );
    }
    assert_eq!(
        all_connection_lost().len(),
        7,
        "§16.11.1 has seven `ConnectionLost` rows"
    );
    assert_eq!(
        read_table().len(),
        8,
        "§16.11.1's read column has eight rows"
    );
    assert_eq!(
        write_table().len(),
        9,
        "§16.11.1's write column has nine rows"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 3. Ruling 56 — `poll_flush` is a no-op returning `Ready`
// ══════════════════════════════════════════════════════════════════════

/// Ruling 56, §16.11: *"`AsyncWrite::flush` is **not** delivery
/// confirmation"*. `CONTRACT-8.md` §3.1: *"`Poll::Ready(Ok(()))`,
/// unconditionally. Touches nothing — not the cell, not the core, not the
/// driver."*
///
/// # The broken build this catches
///
/// **`poll_flush` routed to `poll_acked`** — the "second, weaker `acked()`"
/// ruling 56 forbids, and the single most natural mistake in the impl,
/// because `flush` reads like it should mean something. Here it parks: bytes
/// are written and **no driver turn has run between the write and the
/// flush** (both are synchronous polls, with no `await` between them), so
/// the peer cannot possibly have acknowledged. `Pending` is the separating
/// observation, and a no-op flush cannot produce it.
#[tokio::test(start_paused = true)]
async fn poll_flush_is_ready_with_unacknowledged_bytes_outstanding() {
    local(async {
        let pair = Pair::seeded(0x56);
        let (a, _b) = pair.establish().await;

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");

        let n = match poll_write_once(&mut s, b"unacknowledged") {
            Poll::Ready(Ok(n)) => n,
            other => panic!("poll_write: {other:?}"),
        };
        assert_eq!(n, b"unacknowledged".len());

        match poll_flush_once(&mut s) {
            Poll::Ready(Ok(())) => {}
            Poll::Pending => panic!(
                "RULING 56 VIOLATED: `poll_flush` parked with bytes unacknowledged, so it \
                 is a second, weaker `acked()`. `AsyncWrite::flush` is not delivery \
                 confirmation; `SendStream::acked` is the verb that means acknowledged."
            ),
            Poll::Ready(Err(e)) => panic!("RULING 56 VIOLATED: `poll_flush` failed: {e}"),
        }
    })
    .await;
}

/// The *"touches nothing"* half of ruling 56, made observable.
///
/// # The broken build this catches
///
/// Any `poll_flush` that consults the connection's state at all. On a dead
/// connection the cell is latched, so a flush that reads it answers
/// `Ready(Err(BrokenPipe))` — while `CONTRACT-8.md` §3.1 says
/// `Ready(Ok(()))` **unconditionally**. This is the assertion that separates
/// "returns `Ready` because there is nothing to do" from "returns `Ready`
/// because it asked and the answer happened to be yes".
///
/// If an implementer read §3.1's "unconditionally" as *"unconditionally
/// `Ready`, but `Err` once dead"*, this test is where that disagreement
/// surfaces — which is what a blind split is for.
#[tokio::test(start_paused = true)]
async fn poll_flush_is_ready_ok_even_after_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x561);
        let (a, _b) = pair.establish().await;

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");
        assert!(matches!(
            poll_write_once(&mut s, b"before the end"),
            Poll::Ready(Ok(_))
        ));

        a.close(0, b"").await;
        settle().await;

        match poll_flush_once(&mut s) {
            Poll::Ready(Ok(())) => {}
            Poll::Pending => panic!("RULING 56: `poll_flush` parked on a dead connection"),
            Poll::Ready(Err(e)) => panic!(
                "RULING 56 / CONTRACT-8 §3.1: `poll_flush` is `Ready(Ok(()))` \
                 *unconditionally* and touches neither the cell nor the core, so it \
                 cannot observe the death — got {e} ({:?})",
                e.kind()
            ),
        }
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 4. Ruling 57 — `poll_shutdown` is `finish()` *and then* `acked()`
// ══════════════════════════════════════════════════════════════════════

/// Ruling 57, §16.11. The weaker reading — `finish()` alone — *"loses its
/// tail at the path's loss rate, silently: S28's bug reachable a second
/// time, through the `AsyncWrite` surface, by an application that never
/// touches `close()`."*
///
/// # The broken build this catches
///
/// **`poll_shutdown` = `poll_finish` alone.** `CONTRACT-8.md` §3.1 records
/// that `poll_finish` is *"always `Ready` on the first poll"* (§16.7 seals
/// synchronously), so that build answers `Ready(Ok(()))` immediately — here,
/// with the payload not yet transmitted, let alone acknowledged, because no
/// driver turn has run since the write. `Pending` is the only answer the
/// correct build can give, so the two are separated exactly.
///
/// The positive half matters as much: a build that parks *for ever* would
/// pass the first assertion. `shutdown()` must then resolve `Ok(())` once
/// the peer has acknowledged.
#[tokio::test(start_paused = true)]
async fn poll_shutdown_waits_for_the_acknowledgement_not_just_the_fin() {
    local(async {
        let pair = Pair::seeded(0x57);
        let (a, b) = pair.establish().await;

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");
        let payload = b"the tail of the transfer";
        assert_eq!(
            match poll_write_once(&mut s, payload) {
                Poll::Ready(Ok(n)) => n,
                other => panic!("poll_write: {other:?}"),
            },
            payload.len()
        );

        // No `await` since the write: the drivers have not run, so nothing
        // is on the wire and nothing can have been acknowledged.
        match poll_shutdown_once(&mut s) {
            Poll::Pending => {}
            Poll::Ready(Ok(())) => panic!(
                "RULING 57 VIOLATED: `poll_shutdown` resolved before the peer could \
                 acknowledge — it is `finish()` alone. `copy(..).await; shutdown().await` \
                 then loses its tail at the path's loss rate, silently (S28)."
            ),
            Poll::Ready(Err(e)) => panic!("poll_shutdown failed early: {e}"),
        }

        // The positive half: it does resolve, and the data really arrives.
        let mut r = within(b.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");
        within(s.shutdown(), "shutdown")
            .await
            .expect("shutdown resolved Ok");

        let mut got = Vec::new();
        within(r.read_to_end(&mut got), "read_to_end")
            .await
            .expect("read_to_end");
        assert_eq!(
            got.as_slice(),
            payload,
            "the tail must survive the shutdown that waited for it"
        );
    })
    .await;
}

/// Ruling 57's bound: *"It cannot hang past the connection's own death: a
/// dying connection resolves the wait in error."*
///
/// # The broken build this catches
///
/// A `poll_shutdown` that waits on the acknowledgement **without** the death
/// path — the obvious over-correction from the previous test. It hangs for
/// ever, and `within` turns that into a named failure instead of a dead
/// suite. This is exactly the class working rule 13 warns about: a fixture
/// that can only lose packets cannot express "the wait never ends", so the
/// bound has to be asserted with an explicit deadline.
///
/// The `ErrorKind` is asserted too, which joins ruling 57 to §16.11.1: the
/// peer closed, so this is the `ConnectionLost(PeerClosed)` **write** row,
/// `BrokenPipe` — *a peer that went away under a writer*.
#[tokio::test(start_paused = true)]
async fn poll_shutdown_resolves_in_error_when_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x571);
        let (a, b) = pair.establish().await;

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");
        assert!(matches!(
            poll_write_once(&mut s, b"never acknowledged"),
            Poll::Ready(Ok(_))
        ));
        assert!(
            matches!(poll_shutdown_once(&mut s), Poll::Pending),
            "precondition: the shutdown is waiting for an acknowledgement"
        );

        b.close(7, b"bye").await;
        settle().await;

        let e = within(s.shutdown(), "shutdown after the connection died")
            .await
            .expect_err("RULING 57: a dying connection must resolve the wait in error");
        assert_eq!(
            e.kind(),
            io::ErrorKind::BrokenPipe,
            "§16.11.1: `ConnectionLost(PeerClosed)` on the write side is `BrokenPipe` — \
             a peer that went away under a writer"
        );
    })
    .await;
}

/// `CONTRACT-8.md` §3.1: *"Ruling 57's shutdown needs no stored state … a
/// second `finish()` is `Ok(())` … **Do not add a `shutdown_started:
/// bool`**."*
///
/// # The broken build this catches
///
/// One that re-enters `poll_shutdown` after a `Pending` from `poll_acked`,
/// calls `poll_finish` again, and **propagates** the second call's answer as
/// an error — `WriteError::Finished`, mapped to `BrokenPipe`. Under it a
/// plain `AsyncWriteExt::shutdown()` fails on any stream whose
/// acknowledgement does not arrive on the first poll, which is every stream
/// on a real network. One poll cannot see it; this polls five times before
/// letting the shutdown complete.
#[tokio::test(start_paused = true)]
async fn poll_shutdown_is_idempotent_across_repeated_polls() {
    local(async {
        let pair = Pair::seeded(0x572);
        let (a, b) = pair.establish().await;

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");
        assert!(matches!(
            poll_write_once(&mut s, b"polled repeatedly"),
            Poll::Ready(Ok(_))
        ));

        for i in 0..5 {
            match poll_shutdown_once(&mut s) {
                Poll::Pending => {}
                Poll::Ready(Ok(())) => panic!(
                    "poll {i}: resolved before any driver turn — ruling 57's `acked()` half \
                     is missing"
                ),
                Poll::Ready(Err(e)) => panic!(
                    "poll {i}: CONTRACT-8 §3.1 — re-entering `poll_shutdown` calls \
                     `poll_finish` again *harmlessly*, because a second `finish()` is \
                     `Ok(())`. Got {e} ({:?}).",
                    e.kind()
                ),
            }
        }

        let mut r = within(b.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");
        within(s.shutdown(), "shutdown")
            .await
            .expect("shutdown resolved Ok");
        let mut got = Vec::new();
        within(r.read_to_end(&mut got), "read_to_end")
            .await
            .expect("read_to_end");
        assert_eq!(got.as_slice(), b"polled repeatedly");
    })
    .await;
}

/// `CONTRACT-8.md` §3.3: *"`poll_shutdown` shuts down the send half only. It
/// does not touch, abandon or reset the receive half — a half-closed
/// `BiStream` is the shape `copy_bidirectional` and every request/response
/// protocol relies on."*
///
/// # The broken build this catches
///
/// A `BiStream` shutdown that also finishes, drops or resets the **receive**
/// half. Every request/response exchange breaks under it, and it breaks in
/// the direction that looks like a peer problem: the request arrives, the
/// reply is written, and the originator reads zero bytes. `BiStream`'s own
/// rustdoc records the sibling defect — a build that implemented `Drop` and
/// forgot one half *"would pass any test that checked only the other"* — so
/// this test checks the other.
#[tokio::test(start_paused = true)]
async fn bistream_shutdown_closes_only_the_send_half() {
    local(async {
        let pair = Pair::seeded(0x573);
        let (a, b) = pair.establish().await;

        let request = b"GET /".as_slice();
        let response = b"200 OK, after the requester half-closed".as_slice();

        let mut ab = within(a.open_bi(), "open_bi").await.expect("open_bi");
        within(ab.write_all(request), "write request")
            .await
            .expect("write_all");

        let mut ba = within(b.accept_bi(), "accept_bi").await.expect("accept_bi");

        // The requester half-closes: FIN on its send half, receive half live.
        within(ab.shutdown(), "requester shutdown")
            .await
            .expect("shutdown");

        let mut got = Vec::new();
        within(ba.read_to_end(&mut got), "responder read_to_end")
            .await
            .expect("read_to_end");
        assert_eq!(got.as_slice(), request);

        // The reply is written *after* the requester's shutdown.
        within(ba.write_all(response), "write response")
            .await
            .expect("write_all");
        within(ba.shutdown(), "responder shutdown")
            .await
            .expect("shutdown");

        let mut back = Vec::new();
        within(ab.read_to_end(&mut back), "requester read_to_end")
            .await
            .expect(
                "CONTRACT-8 §3.3: `poll_shutdown` on a `BiStream` shuts down the send half \
                 only; the receive half must survive it",
            );
        assert_eq!(
            back.as_slice(),
            response,
            "the receive half was touched by the send half's shutdown — a half-closed \
             `BiStream` is the shape every request/response protocol relies on"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 5. Ruling 226 — the `Result`-carrying adapters never end
// ══════════════════════════════════════════════════════════════════════

/// Poll a dead face **repeatedly** and require `Some(Err(..))` every time.
///
/// The leading drain exists for `notifications()`: ruling 152 keeps a
/// notification generated before the death from being lost to it, so the
/// first item after a close may legitimately be `Ok`.
fn assert_never_ends<T, S>(s: &mut S, what: &str)
where
    S: Stream<Item = Result<T, ConnectionLost>> + Unpin,
{
    let mut lost: Option<ConnectionLost> = None;
    for _ in 0..8 {
        match poll_stream_once(s) {
            Poll::Ready(Some(Err(e))) => {
                lost = Some(e);
                break;
            }
            Poll::Ready(Some(Ok(_))) => continue,
            Poll::Ready(None) => panic!(
                "{what}: RULING 226 VIOLATED — yielded `None`. The `Result`-carrying faces \
                 never end; `None` destroys the death *reason*, which is the only thing a \
                 consumer draining this face ever learns about why it stopped."
            ),
            Poll::Pending => panic!("{what}: parked on a dead connection"),
        }
    }
    let lost = lost.unwrap_or_else(|| panic!("{what}: never reported the death at all"));

    // Repeatedly. The degenerate build — one `bool` per adapter, `Some(Err)`
    // once and then `None` — passes a single-poll assertion, and it is the
    // *better ergonomics* alternative `CONTRACT-8.md` §4.2 weighed and
    // ruling 226 declined. So the pin has to be on the second poll onward.
    for i in 0..8 {
        match poll_stream_once(s) {
            Poll::Ready(Some(Err(e))) => assert_eq!(
                e, lost,
                "{what}: poll {i} reported a different death — `ConnectionLost` is `Clone` \
                 so that the same value is re-reported indefinitely"
            ),
            Poll::Ready(None) => panic!(
                "{what}: RULING 226 VIOLATED — ended at repeat poll {i}. This is the \
                 `Some(Err(..))`-once-then-`None` build: it passes any single-poll test."
            ),
            Poll::Ready(Some(Ok(_))) => panic!("{what}: yielded an item after the death"),
            Poll::Pending => panic!("{what}: parked at repeat poll {i} on a dead connection"),
        }
    }
}

/// Ruling 226 over four of the five `Result`-carrying faces.
///
/// `incoming_uni()` is deliberately **not** here: `CONTRACT-8.md` §4.6
/// (ruling 51, S30) makes drawing from `messages()` and `incoming_uni()` on
/// one connection *normatively a programming error*, so it gets its own
/// connection in the next test rather than a shared one.
///
/// # The broken build this catches
///
/// See [`assert_never_ends`]: the one-`bool` build that ends after the first
/// error. It is the *more ergonomic* design — it gives
/// `while let Some(x) = s.next().await` a terminating shape and matches what
/// `Framed` does — which is exactly why a blind author must not be left to
/// guess, and why ruling 226 had to be taken before dispatch.
#[tokio::test(start_paused = true)]
async fn the_result_carrying_faces_never_end() {
    local(async {
        let pair = Pair::seeded(0x226);
        let (a, _b) = pair.establish().await;

        a.close(0, b"").await;
        settle().await;

        let mut messages = a.messages();
        assert_never_ends(&mut messages, "messages()");

        let mut datagrams = a.datagrams();
        assert_never_ends(&mut datagrams, "datagrams()");

        let mut incoming_bi = a.incoming_bi();
        assert_never_ends(&mut incoming_bi, "incoming_bi()");

        let mut notifications = a.notifications();
        assert_never_ends(&mut notifications, "notifications()");
    })
    .await;
}

/// Ruling 226 for `incoming_uni()`, on a connection of its own so that §4.6
/// (ruling 51, S30) is not violated by the test suite itself.
#[tokio::test(start_paused = true)]
async fn incoming_uni_never_ends() {
    local(async {
        let pair = Pair::seeded(0x2261);
        let (a, _b) = pair.establish().await;

        a.close(0, b"").await;
        settle().await;

        let mut incoming_uni = a.incoming_uni();
        assert_never_ends(&mut incoming_uni, "incoming_uni()");
    })
    .await;
}

/// The **item types**, pinned at compile time.
///
/// # The broken build this catches
///
/// A build that applied ruling 226 uniformly and made `Incoming`'s item a
/// `Result` too. `CONTRACT-8.md` §4.1 is explicit that `Incoming<'a, I>`
/// yields `Intro<I>` — *"**not** a `Result`"* — because `Endpoint::accept()`
/// returns `Option<Intro<I>>` and its `None` means the endpoint is closed.
/// Nothing at run time distinguishes the two on a live endpoint, so the pin
/// has to be on the type; this fails to compile rather than failing to pass.
///
/// It also pins the other five item types against a build that unwrapped the
/// `Result` for ergonomics, which would silently discard the death reason
/// ruling 226 exists to keep.
#[allow(dead_code)]
mod adapter_item_types {
    use super::*;
    use slither::Handshake;

    fn is_stream_of<I, S: Stream<Item = I>>(_: S) {}

    fn messages<S: Handshake>(m: slither::compat::Messages<'_, S>) {
        is_stream_of::<Result<Vec<u8>, ConnectionLost>, _>(m);
    }
    fn datagrams<S: Handshake>(d: slither::compat::Datagrams<'_, S>) {
        is_stream_of::<Result<Vec<u8>, ConnectionLost>, _>(d);
    }
    fn incoming_bi<S: Handshake>(i: slither::compat::IncomingBi<'_, S>) {
        is_stream_of::<Result<slither::BiStream<S>, ConnectionLost>, _>(i);
    }
    fn incoming_uni<S: Handshake>(i: slither::compat::IncomingUni<'_, S>) {
        is_stream_of::<Result<slither::RecvStream<S>, ConnectionLost>, _>(i);
    }
    fn notifications<S: Handshake>(n: slither::compat::Notifications<'_, S>) {
        is_stream_of::<Result<Notification, ConnectionLost>, _>(n);
    }

    /// The exception, and the reason this module exists.
    fn incoming<I: Identity>(i: slither::compat::Incoming<'_, I>) {
        is_stream_of::<Intro<I>, _>(i);
    }
}

// ══════════════════════════════════════════════════════════════════════
// 6. The empty-buffer rules — ruling 119 and ruling 110
// ══════════════════════════════════════════════════════════════════════

/// Set up one unidirectional stream, deliver `data`, and hand back the
/// receive half **drained or not** as asked.
///
/// The sender is returned alive: dropping a `SendStream` resets its stream.
async fn uni_with_data(
    conn_a: &slither::testutil::TestConnection,
    conn_b: &slither::testutil::TestConnection,
    data: &[u8],
) -> (
    slither::testutil::TestSendStream,
    slither::testutil::TestRecvStream,
) {
    let mut s = within(conn_a.open_uni(), "open_uni")
        .await
        .expect("open_uni");
    within(s.write_all(data), "write_all")
        .await
        .expect("write_all");
    let r = within(conn_b.accept_uni(), "accept_uni")
        .await
        .expect("accept_uni");
    for _ in 0..4 {
        settle().await;
    }
    (s, r)
}

/// `CONTRACT-8.md` §3.2, hazard 1: the adapter short-circuits
/// `buf.remaining() == 0` **itself** and never hands `RecvStream::poll_read`
/// an empty slice.
///
/// # The broken build this catches
///
/// One that forwards the empty slice, gets ruling 119's `Ok(Some(0))` — whose
/// documented meaning is *park* — maps it through a `ReadBuf` that filled
/// nothing, and **latches that as end-of-file**, because at the `AsyncRead`
/// surface a zero fill *is* EOF. The data still queued behind it is then
/// unreachable for ever. `Ok(Some(0))` = park and `Ok(None)` = end of stream
/// is the distinction ruling 119 exists to protect, and the contract records
/// that *"a blind test author has already guessed wrong once"* on it — twice,
/// counting slice 4a.
///
/// The separating assertion is the **second** read: the payload must still
/// be there. A build that returned `Ready` from the empty read and stopped
/// there is indistinguishable from the correct one until you ask for the
/// data.
#[tokio::test(start_paused = true)]
async fn an_empty_read_buf_neither_consumes_nor_latches_end_of_file() {
    local(async {
        let pair = Pair::seeded(0x119);
        let (a, b) = pair.establish().await;
        let payload = b"still here after the empty read".as_slice();
        let (_s, mut r) = uni_with_data(&a, &b, payload).await;

        let mut nothing: [u8; 0] = [];
        let mut empty = ReadBuf::new(&mut nothing);
        match poll_read_once(&mut r, &mut empty) {
            Poll::Ready(Ok(())) => assert_eq!(
                empty.filled().len(),
                0,
                "an empty `ReadBuf` cannot have been filled"
            ),
            Poll::Pending => panic!(
                "CONTRACT-8 §3.2: an empty `buf` must short-circuit to `Ready`, never park — \
                 no arrival of data can unblock a reader with nowhere to put it (ruling 119, \
                 mirroring ruling 110)"
            ),
            Poll::Ready(Err(e)) => panic!("empty read failed: {e}"),
        }

        let mut store = [0u8; 128];
        match read_into(&mut r, &mut store) {
            Poll::Ready(Ok(0)) => panic!(
                "RULING 119 VIOLATED: the empty read was taken for end-of-stream. \
                 `Ok(Some(0))` means *park*; `Ok(None)` means end of stream. Collapsing \
                 them loses every byte still queued, and reports it as a clean EOF."
            ),
            Poll::Ready(Ok(n)) => assert_eq!(
                &store[..n],
                payload,
                "the empty read consumed part of the payload"
            ),
            Poll::Pending => panic!("the payload was delivered; the read must not park"),
            Poll::Ready(Err(e)) => panic!("read failed: {e}"),
        }
    })
    .await;
}

/// The other side of ruling 119's collision: an empty read on a stream that
/// is **open but has nothing available** must not make the next real read
/// look like end-of-stream.
///
/// # The broken build this catches
///
/// The same forwarding build as above, caught where it is most damaging: the
/// real read must be **`Pending`**, and a build that answers `Ready` with a
/// zero fill has told `read_to_end` the transfer finished. That is a
/// truncation reported as success — ruling 121's misreport, arriving through
/// the `AsyncRead` surface instead of the handle.
///
/// This is the assertion the previous test cannot make: with data queued, a
/// zero fill and a filled read are trivially distinguishable; with the queue
/// empty, `Pending` and `Ready(0)` are the *only* two answers and they mean
/// opposite things.
#[tokio::test(start_paused = true)]
async fn a_drained_but_open_stream_parks_and_does_not_report_end_of_file() {
    local(async {
        let pair = Pair::seeded(0x1191);
        let (a, b) = pair.establish().await;
        let payload = b"drain me".as_slice();
        let (_s, mut r) = uni_with_data(&a, &b, payload).await;

        // Drain it. No `finish()` on the sender, so the stream stays open.
        let mut store = [0u8; 128];
        match read_into(&mut r, &mut store) {
            Poll::Ready(Ok(n)) => assert_eq!(&store[..n], payload),
            other => panic!("first read: {other:?}"),
        }

        let mut nothing: [u8; 0] = [];
        let mut empty = ReadBuf::new(&mut nothing);
        assert!(
            matches!(poll_read_once(&mut r, &mut empty), Poll::Ready(Ok(()))),
            "CONTRACT-8 §3.2: an empty `buf` short-circuits to `Ready`"
        );

        match read_into(&mut r, &mut store) {
            Poll::Pending => {}
            Poll::Ready(Ok(0)) => panic!(
                "RULING 119 VIOLATED: a live, drained stream reported end-of-file. \
                 `read_to_end` returns *successfully* under this build, so a truncated \
                 transfer is presented as a complete one."
            ),
            Poll::Ready(Ok(n)) => panic!("the stream was drained; got {n} more bytes"),
            Poll::Ready(Err(e)) => panic!("read failed: {e}"),
        }
    })
    .await;
}

/// `CONTRACT-8.md` §3.2 and `SPEC.md`:4805: *"`AsyncRead` requires a sticky
/// end-of-file, so a connection that dies after the FIN would otherwise
/// surface a spurious `io::Error` to `read_to_end`."* Rulings 121 and 124
/// put the latch in the handle, ahead of the connection's death latch.
///
/// # The broken build this catches
///
/// Two, and the second is the one the spec sentence is about:
///
/// * a non-sticky EOF, where a second read after the FIN errors or blocks —
///   caught by the repeat loop;
/// * an adapter that consults the **connection's** death latch before the
///   **stream's** terminal latch. That build reads a finished stream
///   correctly right up until the connection ends, and then starts answering
///   `Ready(Err(..))` to a stream that was cleanly complete. Killing the
///   connection *after* the FIN has been fully read is what separates it.
#[tokio::test(start_paused = true)]
async fn end_of_stream_is_a_sticky_zero_fill_and_survives_the_connection_death() {
    local(async {
        let pair = Pair::seeded(0x121);
        let (a, b) = pair.establish().await;
        let payload = b"complete transfer".as_slice();

        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");
        within(s.write_all(payload), "write_all")
            .await
            .expect("write_all");
        within(s.finish(), "finish").await.expect("finish");

        let mut r = within(b.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");
        let mut got = Vec::new();
        within(r.read_to_end(&mut got), "read_to_end")
            .await
            .expect("read_to_end");
        assert_eq!(got.as_slice(), payload);

        let mut store = [0u8; 64];
        for i in 0..4 {
            match read_into(&mut r, &mut store) {
                Poll::Ready(Ok(0)) => {}
                Poll::Ready(Ok(n)) => panic!("read {i}: {n} bytes past end-of-stream"),
                Poll::Pending => panic!(
                    "read {i}: parked past end-of-stream — the EOF latch is not sticky, and \
                     `read_to_end` hangs for ever under this build"
                ),
                Poll::Ready(Err(e)) => panic!("read {i}: end-of-stream reported as {e}"),
            }
        }

        // The connection dies *after* the FIN was read out in full.
        a.close(0, b"").await;
        settle().await;

        match read_into(&mut r, &mut store) {
            Poll::Ready(Ok(0)) => {}
            Poll::Ready(Err(e)) => panic!(
                "SPEC.md:4805 — the stream's end-of-file latch sits *ahead of* the \
                 connection's death latch, so a connection dying after the FIN must not \
                 surface a spurious `io::Error` to `read_to_end`. Got {e} ({:?}).",
                e.kind()
            ),
            other => panic!("read after death: {other:?}"),
        }
    })
    .await;
}

/// Ruling 110 and `CONTRACT-8.md` §3.2's second verification: a zero-length
/// write is `Ok(0)`, and **`Ok(0)` never means "blocked"** for a non-empty
/// buffer — a blocked write parks.
///
/// # The broken builds this catches
///
/// Two, from opposite directions, which is why both halves are asserted:
///
/// * a `WriteZero` guard, or a build that parks on an empty write. The
///   contract is explicit that *"no `WriteZero` guard is needed or wanted"*,
///   and §16.2's `AsyncWrite` *"is handed empty buffers by ordinary
///   `tokio::io` combinators"* — under such a build `tokio::io::copy` fails
///   or hangs on an empty chunk.
/// * a build that reports a credit-blocked write as `Ok(0)`. `write_all`
///   turns that into `ErrorKind::WriteZero`, so a transfer that should have
///   waited for a window update fails instead — and it fails only on
///   payloads past `INITIAL_MAX_STREAM_DATA`, which is why a small test
///   never sees it.
///
/// The loop asserts from the side that separates them: not merely that no
/// write returned an error, but that one **parked**, and that none returned
/// `Ok(0)` on the way there.
#[tokio::test(start_paused = true)]
async fn an_empty_write_is_ok_zero_and_a_blocked_write_parks() {
    local(async {
        let pair = Pair::seeded(0x110);
        let (a, _b) = pair.establish().await;
        let mut s = within(a.open_uni(), "open_uni").await.expect("open_uni");

        match poll_write_once(&mut s, &[]) {
            Poll::Ready(Ok(0)) => {}
            Poll::Ready(Ok(n)) => panic!("a zero-length write reported {n} bytes"),
            Poll::Pending => panic!(
                "RULING 110 VIOLATED: an empty write parked. No credit arrival can ever \
                 unblock a writer with nothing to send, so this hangs for ever."
            ),
            Poll::Ready(Err(e)) => panic!(
                "RULING 110 VIOLATED: an empty write failed with {e} ({:?}). Ordinary \
                 `tokio::io` combinators hand `AsyncWrite` empty buffers.",
                e.kind()
            ),
        }

        // No `await` anywhere in this loop, so no window update can arrive:
        // the initial credit is all there is, and it is finite.
        let chunk = vec![0xA5u8; 1024];
        let rounds = (INITIAL_MAX_STREAM_DATA as usize / chunk.len()) + 64;
        let mut accepted = 0usize;
        let mut parked = false;
        for _ in 0..rounds {
            match poll_write_once(&mut s, &chunk) {
                Poll::Ready(Ok(0)) => panic!(
                    "CONTRACT-8 §3.2 / ruling 110: `Ok(0)` for a **non-empty** buffer. A \
                     blocked write parks; `Ok(0)` here makes `write_all` fail with \
                     `ErrorKind::WriteZero` instead of waiting for credit."
                ),
                Poll::Ready(Ok(n)) => accepted += n,
                Poll::Pending => {
                    parked = true;
                    break;
                }
                Poll::Ready(Err(e)) => panic!("write failed: {e}"),
            }
        }
        assert!(
            parked,
            "the writer never parked after {accepted} B with no driver turn to grant more \
             credit — a build that grants credit for ever passes every upper-bound test"
        );
        assert!(accepted > 0, "no bytes were accepted at all");
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 7. The sinks — `CONTRACT-8.md` §4.4
// ══════════════════════════════════════════════════════════════════════

/// `CONTRACT-8.md` §4.4 and §9.12: *"`Sink::poll_close` must not close the
/// connection."*
///
/// # The broken build this catches
///
/// `poll_close` → `Connection::close(..)`. It is one plausible line, it
/// reads as tidy resource management, and it is *"a `Sink` adapter tearing
/// down a multiplexer because one of its faces was dropped"* — every stream,
/// every datagram and every other message on that connection dies with it.
///
/// The separating assertion is a full message round-trip **after** the sink
/// has been closed and dropped: under the broken build the connection is
/// gone and `send_message` returns `ConnectionLost(LocallyClosed)`.
#[tokio::test(start_paused = true)]
async fn message_sink_poll_close_does_not_close_the_connection() {
    local(async {
        let pair = Pair::seeded(0x44);
        let (a, b) = pair.establish().await;

        {
            let mut sink = a.message_sink();
            assert!(
                matches!(
                    Pin::new(&mut sink).poll_ready(&mut noop_context()),
                    Poll::Ready(Ok(()))
                ),
                "a fresh `MessageSink` has an empty slot, so `poll_ready` is `Ready`"
            );
            Pin::new(&mut sink)
                .start_send(b"through the sink".to_vec())
                .expect("start_send");
            within(
                std::future::poll_fn(|cx| Pin::new(&mut sink).poll_close(cx)),
                "poll_close",
            )
            .await
            .expect("poll_close");
        }
        settle().await;

        let first = within(b.recv_message(), "recv_message")
            .await
            .expect("recv_message");
        assert_eq!(first.as_slice(), b"through the sink");

        within(
            a.send_message(b"the connection is still up"),
            "send_message",
        )
        .await
        .expect(
            "CONTRACT-8 §4.4 / §9.12: `Sink::poll_close` must not close the connection — \
                 a sink is one face of a multiplexer",
        );
        let second = within(b.recv_message(), "recv_message")
            .await
            .expect("recv_message");
        assert_eq!(second.as_slice(), b"the connection is still up");
    })
    .await;
}

/// `CONTRACT-8.md` §4.4: `DatagramSink` *"buffers **nothing**"* and its
/// `poll_ready` is *"`Ready(Ok(()))`, **always**"* — it needs no `WakerSlot`
/// because `send_datagram` is synchronous (ruling 204 item 5).
///
/// # The broken builds this catches
///
/// * A `DatagramSink` copied from `MessageSink`, with a one-item slot: its
///   `poll_ready` parks after a `start_send` until something drives the slot
///   empty. `SinkExt::send` then deadlocks against a sink that has no
///   asynchronous work to do.
/// * A `poll_ready` that consults the connection. §4.4 says *always*, and
///   the dead connection is where "always" and "asks first" come apart —
///   the same shape as the `poll_flush` pin above.
///
/// `start_send` reporting `ConnectionLost` afterwards is the positive
/// control: it establishes that the sink *does* notice the death, so the
/// `poll_ready` assertion is about where the answer belongs rather than
/// about a sink that ignores everything.
#[tokio::test(start_paused = true)]
async fn datagram_sink_buffers_nothing_and_is_always_ready() {
    local(async {
        let pair = Pair::seeded(0x441);
        let (a, b) = pair.establish().await;

        let mut sink = a.datagram_sink();
        assert!(matches!(
            Pin::new(&mut sink).poll_ready(&mut noop_context()),
            Poll::Ready(Ok(()))
        ));
        Pin::new(&mut sink)
            .start_send(b"unbuffered".to_vec())
            .expect("start_send");

        // No flush, no driver turn: a sink that buffers nothing is ready again
        // immediately.
        assert!(
            matches!(
                Pin::new(&mut sink).poll_ready(&mut noop_context()),
                Poll::Ready(Ok(()))
            ),
            "CONTRACT-8 §4.4: `DatagramSink` buffers nothing, so `poll_ready` is `Ready` \
             immediately after a `start_send` — a one-item slot copied from `MessageSink` \
             parks here"
        );
        assert!(matches!(
            Pin::new(&mut sink).poll_flush(&mut noop_context()),
            Poll::Ready(Ok(()))
        ));

        settle().await;
        let got = within(b.recv_datagram(), "recv_datagram")
            .await
            .expect("recv_datagram");
        assert_eq!(got.as_slice(), b"unbuffered");

        a.close(0, b"").await;
        settle().await;

        assert!(
            matches!(
                Pin::new(&mut sink).poll_ready(&mut noop_context()),
                Poll::Ready(Ok(()))
            ),
            "CONTRACT-8 §4.4: `poll_ready` is `Ready(Ok(()))` **always** — it ignores its \
             `Context` entirely and never consults the connection"
        );
        let err = Pin::new(&mut sink)
            .start_send(b"after the end".to_vec())
            .expect_err("the death is reported by `start_send`, which is `send_datagram`");
        assert!(
            matches!(err, DatagramError::ConnectionLost(_)),
            "expected `ConnectionLost`, got {err:?}"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 8. Source-level obligations with no runtime instrument
// ══════════════════════════════════════════════════════════════════════

/// Every `.rs` file under `src/compat/`, with `//` comments stripped.
///
/// Stripping matters: `CONTRACT-8.md` §4.5 quotes `src/shell/shared.rs`'s
/// *"An implementer who reaches for a `VecDeque` has rebuilt the unbounded
/// intermediate queue §10.6 forbids"*, and an implementer who quotes it back
/// in rustdoc — which is the right thing to do — must not trip this scan.
///
/// Known limitation, stated rather than hidden: `/* … */` block comments and
/// string literals are not stripped. Neither appears in the patterns below
/// in any plausible correct build.
fn compat_sources() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/compat");
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
                let code: String = text
                    .lines()
                    .map(|line| match line.find("//") {
                        Some(i) => &line[..i],
                        None => line,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push((path.display().to_string(), code));
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no sources found under src/compat — this suite pins that module"
    );
    out
}

/// Three obligations that no value can express, asserted against the source.
///
/// # Why this test is not a runtime test
///
/// * **Ruling 227:** *"the `_ =>` arm maps to `Other` and **must not** be
///   `unreachable!()` — a panic on a variant a future minor version adds."*
///   The variant does not exist yet (§19 reserves `Stopped`, ruling 61), so
///   no test can construct one and reach the arm. `CONTRACT-8.md` §3.2 bans
///   `unreachable!()` a second time, for the `Ok(Some(0))` row: *"A panic
///   reachable only through a future refactor is worse than a defensive arm
///   that costs nothing."* Two independent prohibitions in one module make a
///   module-wide ban the honest reading.
/// * **`CONTRACT-8.md` §9.1** — *"No `VecDeque`, no `Vec<Item>`, no
///   `spawn`-ing drainer"* — is a statement about a struct's **shape**.
///   [`a_stream_adapter_claims_at_most_one_message_per_poll`] catches a
///   prefetcher behaviourally, which is the real pin; this catches a queue
///   that has been added but is not yet filled ahead, i.e. the same defect
///   one commit before it becomes observable.
/// * **`CONTRACT-8.md` §9.6** — *"No `tokio::spawn`; only `spawn_local`"*
///   (S21). A `tokio::spawn` of anything touching a handle does not fail to
///   compile in a test that never runs on a multi-thread runtime; it fails
///   in a consumer's.
///
/// The integrator may prefer to move this into `src/compat/tests.rs`. It is
/// here because it needs nothing from inside the crate.
#[test]
fn compat_source_scan() {
    const BANNED: &[(&str, &str)] = &[
        (
            "unreachable!",
            "ruling 227 and CONTRACT-8 §3.2 both forbid it: the `#[non_exhaustive]` \
             fallback maps to `ErrorKind::Other`, and the `Ok(Some(0))` row maps to a \
             zero fill. Neither is a panic.",
        ),
        (
            "todo!",
            "an unfinished arm in a shipped conversion is a panic in a consumer's process",
        ),
        (
            "unimplemented!",
            "same as `todo!`: CONTRACT-8 §8.2's fallback arm is `ErrorKind::Other`",
        ),
        (
            "VecDeque",
            "CONTRACT-8 §9.1 / ruling 58: no intermediate queue. The single-item \
             `MessageSink` slot is the only buffer permitted in `compat/`, and it exists \
             because `Sink`'s own protocol requires it on the *send* side.",
        ),
        (
            "tokio::spawn",
            "CONTRACT-8 §9.6 / S21: the driver is `!Send` by requirement. `spawn_local` \
             is the substitute.",
        ),
    ];

    let mut findings = Vec::new();
    for (path, code) in compat_sources() {
        for (needle, why) in BANNED {
            if code.contains(needle) {
                findings.push(format!("{path}: `{needle}` — {why}"));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "CONTRACT-8 §9's checklist is violated:\n  {}",
        findings.join("\n  ")
    );
}

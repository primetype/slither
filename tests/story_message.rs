//! **Messages and the overflow — S16 and S30.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S16** | a message up to `MESSAGE_RECV_MAX` (262 144) in **one call**; larger is `MessageError::TooLarge`. bubble-engine chunks above this and cares about the exact bound. |
//! | **S30** | mixing `recv_message()` with `accept_uni()` on one connection is a programming error the wire cannot repair, so it must fail **loudly and diagnosably**: the sender sees `WriteError::Reset(MESSAGE_OVERFLOW)`, code `0x06`, not a permanent stall and not an anonymous `0`. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 6's message half**, from
//! `STORIES.md` (S16, S30), `SPEC.md` §9.6/§9.8/§10.3/§10.6/§15.3/§16.2 and
//! Appendix B, and `.slices/06-sugar/CONTRACT-6.md` **alone**, while the
//! implementer wrote `src/` and a second blind author wrote
//! `tests/story_datagram.rs`, concurrently and invisibly. No line of slice
//! 6's implementation was read.
//!
//! # Working rule 9 — every test names the build it separates
//!
//! *A bound is only a test if the degenerate case violates it.* Each test
//! carries a `BROKEN BUILD:` block naming a concrete implementation that
//! passes a weaker version of the test while being wrong. The mirror rule
//! is enforced too — *an assertion a conforming build can fail is a flake,
//! not a pin* — and the assertions dropped for it are recorded in
//! `.slices/06-sugar/TESTS-6-message.md` §3 rather than shipped as a
//! occasional red.
//!
//! **The trap this file was written around.** The overflow rule is a
//! *negative*: a conforming message must **not** be reset. A test that
//! sends a small message and observes no reset asserts nothing, because
//! nothing came near the bound. Only [`s30_a_conforming_maximum_message_is_not_reset`]
//! — armed claim, payload of **exactly** `MESSAGE_RECV_MAX` — separates a
//! build whose FIN rides its final data frame (ruling 153) from one that
//! emits a separate empty FIN frame and resets the message that was in
//! flight. The second build's operator post-mortem reads "transfers to this
//! peer die at exactly 256 KiB" and points at the wrong cause.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the observation instrument in both directions:
//! [`within`] asserts *it resolved*, [`is_pending_for`] asserts *it had not
//! resolved by then*. [`settle`] gives both drivers a turn **without**
//! advancing virtual time, which is not enough for a 256 KiB transfer —
//! slow start needs several rounds of acknowledgement and §12's ack delay is
//! a timer — so bulk transfers here are followed by [`quiesce`], which lets
//! virtual time pass while both drivers run. There is no `sleep`.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::SocketAddr;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use slither::constants::{
    INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA, INITIAL_WINDOW, KEEPALIVE_TIMEOUT, MAX_DATAGRAM,
    MESSAGE_OVERFLOW, MESSAGE_RECV_MAX, NO_ERROR,
};
use slither::error::{ConnectionLost, MessageError, WriteError};
use slither::testutil::{Pair, Tap, TestRecvStream, TestSendStream, local, settle};
// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// `Pair` / `local` / `settle` are slice 0+3 harness and are used as
// shipped. The two message verbs are slice 6's and are called exactly as
// `CONTRACT-6.md` §2.5 spells them — if a name below does not compile, the
// contract and the implementation disagree, and **that is the finding**,
// not a rename to be made here.
// ══════════════════════════════════════════════════════════════════════

/// The message bound, in `usize`. §9.8's table: `= INITIAL_MAX_STREAM_DATA`.
const BOUND: usize = MESSAGE_RECV_MAX as usize;

/// The per-stream window an `open_uni()` stream fills before it stalls.
/// Equal to [`BOUND`] by §9.8, and that equality is *why* the overflow rule
/// exists — asserted here so a divergence is a named failure and not a
/// mysterious one three tests down.
const STREAM_WINDOW: usize = INITIAL_MAX_STREAM_DATA as usize;

/// The connection window. Four window-full uni streams is exactly this, and
/// that arithmetic is the whole of the credit-true-up test.
const CONN_WINDOW: usize = INITIAL_MAX_DATA as usize;

/// Virtual-time budget for something that **must** resolve without waiting
/// on loss recovery. On the paused clock a resolvable future costs no wall
/// time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Virtual-time budget for something that must resolve **through** loss —
/// a PTO with no RTT sample is ≈ 1 s and §13.3 doubles it per unanswered
/// probe. Deliberately under `DEAD_TIMEOUT` (25 s): a budget past the death
/// would let a test that meant to observe recovery observe a corpse and
/// report the wrong failure.
const RECOVERY_PATIENCE: Duration = Duration::from_secs(12);

/// Budget for "**promptly**, not on the next timer".
///
/// What it separates and what it does not, stated because the honest scope
/// is narrower than the name: a virtual wire has ≈ 0 RTT, so a PTO here can
/// be tens of milliseconds and this budget does **not** separate a build
/// that emits the reset from its PTO path. It does separate one that emits
/// it only from the keepalive (10 s) or the death timer (25 s), which is the
/// shape a build gets by forgetting that §16.7 seals inside the mutating
/// call — the reason ruling 151 gives `core::recv_message` its `now`.
const PROMPT: Duration = Duration::from_secs(1);

/// Budget for the **"not before"** half. Long enough for every driver turn
/// these tests create, short enough to stay far inside `DEAD_TIMEOUT`.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut` on `budget`, failing loudly instead of hanging the suite.
async fn within_for<F: Future>(fut: F, what: &str, budget: Duration) -> F::Output {
    match tokio::time::timeout(budget, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {budget:?} of virtual time"),
    }
}

/// [`within_for`] on [`PATIENCE`].
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    within_for(fut, what, PATIENCE).await
}

/// [`within_for`] on [`RECOVERY_PATIENCE`] — for what must survive loss.
async fn recovering<F: Future>(fut: F, what: &str) -> F::Output {
    within_for(fut, what, RECOVERY_PATIENCE).await
}

/// [`within_for`] on [`PROMPT`] — see that constant for the exact scope.
async fn promptly<F: Future>(fut: F, what: &str) -> F::Output {
    within_for(fut, what, PROMPT).await
}

/// `true` if `fut` had **not** resolved within `budget`. Consumes the
/// future, which is exactly the cancel-safe drop the contract promises.
async fn is_pending_for<F: Future>(fut: F, budget: Duration) -> bool {
    tokio::time::timeout(budget, fut).await.is_err()
}

/// [`is_pending_for`] on [`NOT_BEFORE`].
async fn is_pending<F: Future>(fut: F) -> bool {
    is_pending_for(fut, NOT_BEFORE).await
}

/// Poll `fut` exactly once. The instrument for "**immediately**", and the
/// only way to *make* a `recv_message()` claim without also awaiting it:
/// §9.8's overflow scan runs "at the instant such a claim is made", so a
/// single poll is the arming action several tests below depend on.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// Let virtual time pass while both drivers run.
///
/// `settle()` yields without advancing the clock, which is *not* enough for
/// a 256 KiB transfer: slow start needs several rounds of acknowledgement
/// and §12's ack delay (`MAX_ACK_DELAY` = 25 ms) is a timer. A test that
/// used `settle()` alone here would assert against a transfer that had
/// barely started, and would fail on a conforming build.
async fn quiesce(budget: Duration) {
    let _ = tokio::time::timeout(budget, std::future::pending::<()>()).await;
}

/// A payload whose every byte is a function of its offset.
///
/// 251 is prime and coprime with every packet size in play, so a shift of
/// any length — a dropped range, a re-delivered duplicate, a message
/// assembled from two streams' bytes — moves *every* subsequent byte. A
/// repeated-byte payload hides all three.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Compare without dumping 256 KiB into the panic message.
///
/// Length first, content second: truncation and duplication are the two
/// failures a single `assert_eq!` would report identically, and a build
/// that chunks a message at the wrong bound produces the first.
fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: byte count differs — a build that chunks below \
         MESSAGE_RECV_MAX is short, one that re-delivers a retransmitted \
         range is long"
    );
    if let Some(i) = got.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: first differing byte at offset {i}: got {:#04x}, want {:#04x}",
            got[i], want[i]
        );
    }
}

/// How many datagrams this wire has handed to the fabric.
///
/// The tap sits below the blackhole check, so with no partition active on
/// `who` this is exactly that wire's 0-based send index. It says nothing
/// about what *arrived*.
fn sent_from(tap: &Tap, who: SocketAddr) -> usize {
    tap.snapshot().iter().filter(|s| s.src == who).count()
}

/// Datagrams that were sent and **destroyed by the fabric**, cumulative.
///
/// `Network::sends()` counts every `send_to` before any policy decision;
/// the tap counts those that were neither send-failed nor blackholed. With
/// no injected send failure anywhere in this file, every unit of the
/// difference is a `block_path` casualty — a **counter-proved** loss, which
/// `FlakyPolicy::lossy` could never give (ruling 148: it is invisible to
/// every public counter).
fn blackholed(pair: &Pair) -> usize {
    pair.net.sends() - pair.net.tap().len()
}

/// How many packets a *first flight* can actually put on the wire.
///
/// **Not the payload's packet count** — that was this file's original bound
/// (`min_packets(len)`, now removed) and it is unreachable. Ruling 150 admits the whole payload into send state, but
/// §14.5's admission gate only lets `INITIAL_WINDOW` (12 000 B) leave before
/// an ACK returns, and on a blackholed path no ACK ever does. A 16 KiB
/// message therefore loses about ten packets on its first flight, not
/// fourteen, and the rest are still queued when the path heals.
///
/// The bound stays a **strict lower** one, which is what working rule 9
/// needs here: without it the test would pass on a wire that lost nothing.
fn min_first_flight_packets() -> usize {
    (INITIAL_WINDOW as usize) / MAX_DATAGRAM
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: §16.2 — `Ok(0)` means only that `buf` was empty; a blocked \
             write is `Pending`"
        );
        assert!(
            n <= buf.len() - done,
            "{what}: write claimed {n} bytes of a {}-byte buffer",
            buf.len() - done
        );
        done += n;
    }
}

/// Drain **exactly** `n` bytes, never one more.
async fn drain_exactly(r: &mut TestRecvStream, n: usize, what: &str) {
    let mut done = 0usize;
    while done < n {
        let want = (n - done).min(8192);
        let mut buf = vec![0u8; want];
        match recovering(r.read(&mut buf), what).await {
            Ok(Some(k)) => {
                assert!(k >= 1 && k <= want, "{what}: read returned {k} for {want}");
                done += k;
            }
            other => panic!("{what}: wanted {n} bytes, got {other:?} after {done}"),
        }
    }
}

/// Read until `Ok(None)`, failing on any error.
async fn read_to_end(r: &mut TestRecvStream, what: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match recovering(r.read(&mut buf), what).await {
            Ok(Some(n)) => {
                assert!(n >= 1 && n <= buf.len(), "{what}: read returned {n}");
                out.extend_from_slice(&buf[..n]);
            }
            Ok(None) => break,
            Err(e) => panic!("{what}: expected a clean end of stream, got {e:?}"),
        }
    }
    out
}

/// Make a `recv_message()` claim **without** awaiting it, binding the
/// parked future to `$claim` in the caller's scope.
///
/// The single poll is the arming action: §9.8:3177-3179 runs the overflow
/// scan *at the instant a claim is made* and thereafter *while it is
/// pending*, and no other public call reaches that state. The assertion
/// inside is not decoration — if this resolved, the test that follows would
/// be measuring a connection with **no** claim outstanding and would prove
/// the opposite of what it says.
///
/// The future is **bound, not returned**, and every caller keeps it alive:
/// `pin!` anchors its value in the enclosing temporary scope, so a version
/// of this that returned the `Pin` out of a block would not borrow-check.
/// Keeping it alive is also the faithful model of a receiver looping on
/// `recv_message()` — PLAN-6 §4.5 records that a *dropped* future does not
/// clear the flag either, but a test should not lean on that.
macro_rules! arm_claim {
    ($claim:ident, $conn:expr, $what:expr) => {
        let mut $claim = pin!($conn.recv_message());
        assert!(
            poll_once($claim.as_mut()).await.is_pending(),
            "{}: nothing is claimable yet, so this poll must park — it is what \
             puts the receiver demonstrably in message mode (§9.8:3177-3179)",
            $what
        );
    };
}

// §9.8's table *defines* `MESSAGE_RECV_MAX = INITIAL_MAX_STREAM_DATA`, and
// every overflow test below is built on the two being the same number: the
// bound a message may not exceed is exactly the window an `open_uni()`
// stream fills before it stalls. If they ever diverge, this file's fixtures
// stop meaning what their comments say — so it is a compile error here
// rather than six confusing reds.
const _: () = assert!(
    BOUND == STREAM_WINDOW,
    "§9.8: MESSAGE_RECV_MAX is defined as INITIAL_MAX_STREAM_DATA"
);

// The separating power of `s30_the_overflow_reset_trues_up_connection_credit`
// is this equality: four window-full uni streams are *exactly* the
// connection window, so a build that never trues up the reset streams' bytes
// has nothing left at all.
const _: () = assert!(
    4 * STREAM_WINDOW == CONN_WINDOW,
    "§10: four window-full streams are exactly INITIAL_MAX_DATA"
);

// ══════════════════════════════════════════════════════════════════════
// S16 — one call, one message
// ══════════════════════════════════════════════════════════════════════

/// **S16 — a maximum-size message crosses in one call, byte-identical.**
///
/// > A message up to `MESSAGE_RECV_MAX` (262 144) in one call.
///
/// The size is not a round number chosen for comfort: S16 records that
/// bubble-engine chunks above this bound and **cares about the exact
/// number**, so 262 144 is a consumer-visible pin, not a smoke test.
///
/// # BROKEN BUILD
///
/// * **A build that chunks below the bound** — delivers a short first
///   message. Caught by the length half of [`assert_same_bytes`], which is
///   asserted before the content half so the two failures do not report
///   identically.
/// * **A build that delivers the message as several messages.** The first
///   claim is short *and* the second claim yields instead of parking; both
///   assertions fire, and the second names the cause.
/// * **A build whose reassembly trusts the wrong copy of an overlapping
///   range** — the payload is offset-derived, so the content assertion
///   fires even where the length is right.
///
/// The size is also the module's `BOUND == STREAM_WINDOW` const assert made
/// concrete: §9.8's table *defines* `MESSAGE_RECV_MAX =
/// INITIAL_MAX_STREAM_DATA`, and every overflow test below is built on the
/// two being the same number.
#[tokio::test(start_paused = true)]
async fn s16_a_maximum_message_arrives_whole_in_one_call() {
    local(async {
        let pair = Pair::seeded(0x5160_0001);
        let (ca, cb) = pair.establish().await;

        let want = payload(BOUND);
        within(ca.send_message(&want), "send_message at the bound")
            .await
            .expect("§9.8: a payload of exactly MESSAGE_RECV_MAX is accepted");

        let got = recovering(cb.recv_message(), "recv_message at the bound")
            .await
            .expect("the connection is alive and the message is complete");
        assert_same_bytes(&got, &want, "S16 the maximum message");

        // One call in, **one** message out.
        let mut second = pin!(cb.recv_message());
        assert!(
            poll_once(second.as_mut()).await.is_pending(),
            "§9.8: one `send_message` is one uni stream is one message — a \
             second claimable payload means the sender chunked it"
        );
    })
    .await;
}

/// **S16 — one byte over the bound is rejected at the handle, and nothing
/// is sent.**
///
/// The other side of [`s16_a_maximum_message_arrives_whole_in_one_call`],
/// plus the byte below it. Slice 1's lesson was a one-sided boundary —
/// `LEN` and `LEN-1` tested, `LEN+1` not — so all three are pinned, and the
/// rejection is checked for its *silence* as well as its error.
///
/// # BROKEN BUILD
///
/// * **A truncate-and-send build** returns `Ok(())` and delivers 262 144 of
///   the 262 145 bytes. Caught by the `TooLarge` assertion.
/// * **A build that rejects *after* opening the stream or admitting the
///   payload** (§9.8:3088 — *"rejected at the handle"*; CONTRACT-6 §2.5
///   puts the check in the shell, before the core call). Caught by A's send
///   counter, which must not move. Without this half, a build that leaks a
///   stream index and a FIN-less stream on every oversize call passes.
/// * **A build that rejects everything at or near the bound** — an
///   off-by-one in the other direction. Caught by the 262 143-byte message,
///   which must still arrive whole.
#[tokio::test(start_paused = true)]
async fn s16_one_byte_over_the_bound_is_rejected_at_the_handle() {
    local(async {
        let pair = Pair::seeded(0x5160_0002);
        let (ca, cb) = pair.establish().await;

        // Quiesce first: the counter below is a *difference*, and it is only
        // meaningful once the handshake's own acknowledgements have gone.
        quiesce(NOT_BEFORE).await;

        let tap = pair.net.tap();
        let before = sent_from(&tap, pair.a.addr());

        let too_big = payload(BOUND + 1);
        let err = within(ca.send_message(&too_big), "send_message over the bound")
            .await
            .expect_err("§9.8: MESSAGE_RECV_MAX + 1 is MessageError::TooLarge");
        assert!(
            matches!(err, MessageError::TooLarge),
            "S16: the error is `TooLarge`, got {err:?}"
        );

        settle().await;
        assert_eq!(
            sent_from(&tap, pair.a.addr()),
            before,
            "§9.8:3088 — the bound is checked **at the handle**: an oversize \
             send opens no stream, admits no byte, and puts nothing on the wire"
        );

        // ...and the byte below the bound still crosses.
        let want = payload(BOUND - 1);
        within(ca.send_message(&want), "send_message one below the bound")
            .await
            .expect("MESSAGE_RECV_MAX - 1 is accepted");
        let got = recovering(cb.recv_message(), "recv_message one below the bound")
            .await
            .expect("delivered");
        assert_same_bytes(&got, &want, "S16 one byte below the bound");
    })
    .await;
}

/// **S16 — an empty message is a message.**
///
/// `CONTRACT-6.md` §2.3 states it in terms: a complete unclaimed stream
/// carrying a zero-byte payload (FIN at offset 0) is `Some(Vec::new())`,
/// **not** `None`, because `None` must mean "nothing to claim" or the shell
/// parks on a delivered message for ever.
///
/// # BROKEN BUILD
///
/// * **A build that folds "empty payload" into "nothing to claim"** — the
///   receiver parks for ever on a message that was delivered. It fails here
///   as a [`recovering`] panic rather than as a hung suite, which is the
///   only reason this is written with a budget instead of a bare `await`.
/// * **A build that treats the empty message as a no-op on the send side**
///   and never opens the stream — same failure, same line.
/// * **A build that leaves the empty message wedged at the head of the
///   unclaimed queue** — caught by the second, non-empty message, which
///   must follow it.
#[tokio::test(start_paused = true)]
async fn s16_an_empty_message_is_a_message() {
    local(async {
        let pair = Pair::seeded(0x5160_0003);
        let (ca, cb) = pair.establish().await;

        within(ca.send_message(b""), "send_message of an empty payload")
            .await
            .expect("§9.8 states no minimum");

        let got = recovering(cb.recv_message(), "the empty message")
            .await
            .expect("CONTRACT-6 §2.3: an empty message is delivered, not swallowed");
        assert!(
            got.is_empty(),
            "the empty message must arrive empty, got {} bytes",
            got.len()
        );

        // And it did not wedge the queue behind it.
        let want = payload(1_000);
        within(ca.send_message(&want), "the message after the empty one")
            .await
            .expect("accepted");
        let got = recovering(cb.recv_message(), "the message after the empty one")
            .await
            .expect("delivered");
        assert_same_bytes(&got, &want, "S16 the message after the empty one");
    })
    .await;
}

/// **S16 — messages surface in open order, and none is lost.**
///
/// §9.8 / ruling 112: `recv_message()` surfaces *the oldest fully-
/// reassembled unclaimed uni stream*, FIFO in **open** order.
///
/// # What this pins, and what it deliberately leaves to the core tests
///
/// All three messages are complete **before** the first claim, which makes
/// "oldest complete" and "oldest opened" the same stream and the expected
/// order deterministic on a conforming build. That is the strongest
/// integration-level form available: the interesting case — a complete
/// stream sitting *behind* an incomplete one, where completion order and
/// open order disagree — needs a gap injected into one stream's middle and
/// belongs to the core suite, which can drive frames directly. Recorded in
/// `TESTS-6-message.md` §3 rather than approximated here.
///
/// # BROKEN BUILD
///
/// * **A stack-ordered `unclaimed` set** (`pop_back`, or a `HashMap`
///   iteration order that is not insertion order) returns the 3 000-byte
///   message first. Caught by the length assertions, which are distinct per
///   message *by construction* — the three sizes differ so a mis-ordering
///   is a named failure rather than a content mismatch.
/// * **A build that loses a message** — the third claim parks and
///   [`recovering`] panics.
/// * **A build that surfaces one stream twice** (no §9.2 closed-stream
///   watermark) — the fourth claim yields where it must park.
#[tokio::test(start_paused = true)]
async fn s16_messages_are_claimed_in_open_order_and_none_is_lost() {
    local(async {
        let pair = Pair::seeded(0x5160_0004);
        let (ca, cb) = pair.establish().await;

        let sizes = [1_000usize, 2_000, 3_000];
        for len in sizes {
            within(ca.send_message(&payload(len)), "send_message")
                .await
                .expect("accepted");
        }
        // Everything is complete at B before the first claim, so "oldest
        // complete" and "oldest opened" are the same stream.
        quiesce(NOT_BEFORE).await;

        for len in sizes {
            let got = recovering(cb.recv_message(), "claim in open order")
                .await
                .expect("delivered");
            assert_eq!(
                got.len(),
                len,
                "ruling 112: `recv_message` surfaces the oldest **opened** \
                 unclaimed complete stream; a stack-ordered set returns the \
                 newest and lands here"
            );
            assert_same_bytes(&got, &payload(len), "S16 message contents");
        }

        let mut extra = pin!(cb.recv_message());
        assert!(
            poll_once(extra.as_mut()).await.is_pending(),
            "§9.2's closed-stream watermark: three messages in, three out — a \
             fourth claimable payload is the same stream surfaced twice"
        );
    })
    .await;
}

/// **S16 — an unclaimed complete message is retained until it is claimed.**
///
/// **Ruling 154** struck §10.3's fifth retirement trigger ("final size
/// reached with no reader"), which §9.7 never contained: **retention until
/// claimed is the rule.** §10.6 says the same from the other side — *message
/// and datagram payloads stay accounted inside the core until the handle
/// takes them*.
///
/// # BROKEN BUILD
///
/// * **A build that retires the receive half when the final size is
///   reached** — the payload is trued up and freed at the FIN, so the claim
///   that arrives afterwards finds nothing and parks for ever. It fails as
///   a [`recovering`] panic on the claim.
/// * **A build that retains the payload but forgets the stream is
///   claimable** (no `MessageReadable`) — same line; and because the claim
///   here is made *after* the data, no waker can rescue it.
///
/// Virtual time is advanced past several driver turns before the claim, so
/// the build being separated has had every opportunity to run its GC. The
/// `closed()` assertion doubles as the check that the retention is not
/// costing the connection its life.
#[tokio::test(start_paused = true)]
async fn s16_an_unclaimed_complete_message_is_retained_until_claimed() {
    local(async {
        let pair = Pair::seeded(0x5160_0005);
        let (ca, cb) = pair.establish().await;

        let want = payload(50_000);
        within(ca.send_message(&want), "send_message")
            .await
            .expect("accepted");

        // Nobody claims. Time passes — three seconds is many driver turns and
        // well inside KEEPALIVE_TIMEOUT (10 s), so the connection must live.
        assert!(
            is_pending_for(cb.closed(), Duration::from_secs(3)).await,
            "an unclaimed message must not kill the connection"
        );

        let got = recovering(cb.recv_message(), "the late claim")
            .await
            .expect("ruling 154: the stream is retained until claimed, not retired at its FIN");
        assert_same_bytes(&got, &want, "S16 the retained message");
    })
    .await;
}

/// **S16/S28 — `send_message`, `acked()`, `close()`, drop: the peer still
/// receives it in full, after the death.**
///
/// Appendix B (`SPEC.md:6257-6265`) writes this shape verbatim, and
/// **ruling 152** is what makes the last step legal: `recv_message` drains
/// after the connection's death on the same terms as `read` and `accept_*`.
/// The ordering here is the sharp one — B's `closed()` is awaited **first**,
/// so the claim provably runs on a connection that is already dead.
///
/// # BROKEN BUILD
///
/// * **A shell that checks the death latch before the core call.**
///   `CONTRACT-6.md` §2.5's precedence is *core first, latch only if the
///   core had nothing*; inverted, this returns `Err(ConnectionLost)` while a
///   complete message sits in the core. That is the natural build, because
///   §16.2's post-death paragraph named only `read` and `accept_*` until
///   ruling 152 amended it — which is precisely why this test exists.
/// * **A `Connection::acked()` whose snapshot is built from *handles***
///   omits the message stream, which has none (§16.2:4456-4458). It resolves
///   immediately, A closes before retransmitting, and B never completes the
///   message. This is the assertion that makes `acked()` load-bearing rather
///   than decorative.
/// * **A shell that parks on a dead connection** (ruling 128) — the final
///   claim, made with nothing left to drain, must be `Ready(Err(..))` under
///   a single poll. A build that parks hangs the suite; this catches it in
///   one poll instead.
///
/// # Construction
///
/// The loss is a **blackhole**, not `FlakyPolicy::lossy` (ruling 148: a
/// loss-rate drop is invisible to every public counter, so it cannot prove
/// a datagram died). `blackholed()` counts the casualties and the assertion
/// is `>= min_packets`, a strict lower bound: without it the test would pass
/// on a wire that lost nothing, which is working rule 9's own failure mode.
#[tokio::test(start_paused = true)]
async fn s16_message_then_acked_then_close_delivers_after_the_death() {
    local(async {
        let pair = Pair::seeded(0x5160_0006);
        let (ca, cb) = pair.establish().await;
        quiesce(NOT_BEFORE).await;

        let m = payload(16 * 1024);
        let killed_before = blackholed(&pair);

        // The whole first transmission dies.
        pair.net.block_path(pair.a.addr(), pair.b.addr());
        within(ca.send_message(&m), "send_message")
            .await
            .expect("accepted");
        quiesce(NOT_BEFORE).await;
        pair.net.heal_path(pair.a.addr(), pair.b.addr());

        assert!(
            blackholed(&pair) >= killed_before + min_first_flight_packets(),
            "the message's first transmission must actually have been destroyed \
             — otherwise this test proves nothing about `acked()`"
        );

        // ruling 47: wait for the peer's transport to have the bytes.
        assert_eq!(
            recovering(ca.acked(), "Connection::acked over a message").await,
            Ok(()),
            "§16.2:4456-4458 — the snapshot spans every stream, **including the \
             message streams §9.8 never surfaces a handle for**"
        );

        ca.close(NO_ERROR, b"").await;
        drop(ca);

        assert!(
            matches!(
                recovering(cb.closed(), "the peer's death").await,
                ConnectionLost::PeerClosed { .. }
            ),
            "A closed cleanly, so B's death is PeerClosed"
        );

        // The connection is **already dead** and the message still drains.
        let got = recovering(cb.recv_message(), "the post-death claim")
            .await
            .expect("ruling 152: `recv_message` drains after the death");
        assert_same_bytes(&got, &m, "Appendix B's farewell message");

        // ...and with nothing left, the claim is an error, promptly, never a park.
        let mut drained = pin!(cb.recv_message());
        match poll_once(drained.as_mut()).await {
            Poll::Ready(Err(_)) => {}
            other => panic!(
                "ruling 128: parking is never permitted on a dead connection — \
                 an empty post-death claim is `Ready(Err(ConnectionLost))`, got \
                 {}",
                match other {
                    Poll::Pending => "Pending (the build parks for ever)",
                    Poll::Ready(Ok(_)) => "Ready(Ok(..)) (a payload from nowhere)",
                    Poll::Ready(Err(_)) => unreachable!(),
                }
            ),
        }
    })
    .await;
}

/// **S16/S28 — the negative: without `acked()`, the same drop set loses the
/// message.**
///
/// Appendix B is explicit that the positive alone is not a test: *"run it
/// enough times that the pre-ruling-47 ordering fails the same assertion at
/// the injected loss rate"*. A blackhole makes "enough times" exactly once.
///
/// # BROKEN BUILD
///
/// This test separates **the test above** from a wire that loses nothing.
/// Without it, `s16_message_then_acked_then_close_delivers_after_the_death`
/// passes on a build where `acked()` returns immediately and on a fabric
/// where the blackhole was mis-configured — the exact "a name is not a pin"
/// failure of slice 2a. The `blackholed()` assertion is here too, for the
/// same reason and against the same mistake.
///
/// # Why this is not a flake
///
/// §15.2 lets `close()` drop recovery state immediately, so the pre-ruling-47
/// ordering *must* lose the tail. Two properties keep the negative
/// deterministic on a conforming build: the path is healed immediately
/// before `close()` with no intervening await, so no PTO can fire in the
/// window; and 16 KiB is ≈ 15 datagrams, far more than one PTO probe could
/// repair even if one did. Recorded in `TESTS-6-message.md` §3 as the one
/// construction in this file whose determinism is argued rather than
/// counted.
#[tokio::test(start_paused = true)]
async fn s16_without_acked_the_same_drop_set_loses_the_message() {
    local(async {
        let pair = Pair::seeded(0x5160_0007);
        let (ca, cb) = pair.establish().await;
        quiesce(NOT_BEFORE).await;

        let m = payload(16 * 1024);
        let killed_before = blackholed(&pair);

        pair.net.block_path(pair.a.addr(), pair.b.addr());
        within(ca.send_message(&m), "send_message")
            .await
            .expect("accepted");
        quiesce(NOT_BEFORE).await;

        // Heal and close with nothing in between: the pre-ruling-47 ordering.
        pair.net.heal_path(pair.a.addr(), pair.b.addr());
        ca.close(NO_ERROR, b"").await;
        drop(ca);

        assert!(
            blackholed(&pair) >= killed_before + min_first_flight_packets(),
            "the drop set must be the same one the positive test survived"
        );

        assert!(
            matches!(
                recovering(cb.closed(), "the peer's death").await,
                ConnectionLost::PeerClosed { .. }
            ),
            "the CLOSE itself was sent after the heal and does arrive"
        );

        assert!(
            recovering(cb.recv_message(), "the post-death claim")
                .await
                .is_err(),
            "Appendix B: send-close-drop with no `acked()` loses the tail at the \
             injected loss rate — if this yields the message, the blackhole did \
             not bite and the positive test proves nothing"
        );
    })
    .await;
}

/// **S16 — `send_message` admits the whole payload or nothing (ruling 150).**
///
/// `CONTRACT-6.md` §2.5: *"a dropped future has sent **nothing**: no stream
/// opened, no byte admitted"*, and the contract says outright that a blind
/// author will build the alternative because it is the natural
/// decomposition. This is that test.
///
/// # Construction — the headroom is the whole point
///
/// A's four `open_uni()` streams fill the connection window to within
/// **1 000 bytes**, and B accepts them but never reads, so §10.3's
/// read-driven credit never advances. A 4 000-byte `send_message` therefore
/// cannot be admitted whole, and *can* be admitted partially. Exhausting the
/// window completely would not separate the builds: with zero credit the
/// decomposing shell admits zero bytes too, and its half-written stream is
/// empty and invisible on the wire.
///
/// # BROKEN BUILD
///
/// * **The decomposing shell** — `open_uni` → write-loop → `finish` — admits
///   1 000 of the 4 000 bytes and parks. Dropping the future leaves a
///   **FIN-less uni stream carrying 1 000 bytes** on the wire, which B's
///   `accept_uni()` then yields: the fifth-stream assertion fires. Against a
///   *message-mode* receiver that same stream is §9.8's overflow case, so
///   the application's own `select!` timeout would manufacture the failure
///   S30 exists to diagnose.
/// * **A build that admits partially and reports `Ok(())`** — the message
///   would be delivered truncated and FIN'd; caught by the final
///   [`assert_same_bytes`].
/// * **A build that cannot resume** — after the credit is freed the same
///   payload must cross whole, with its FIN, which [`read_to_end`] requires.
///
/// B stays in `accept_uni()` mode throughout, deliberately: this test must
/// not itself commit S30's mixing error, and the leaked stream is only
/// observable through the stream verb anyway.
#[tokio::test(start_paused = true)]
async fn s16_send_message_admits_the_whole_payload_or_nothing() {
    const HEADROOM: usize = 1_000;

    local(async {
        let pair = Pair::seeded(0x5160_0008);
        let (ca, cb) = pair.establish().await;

        // Fill the connection window to within HEADROOM across four streams.
        let mut lens = Vec::new();
        let mut sends = Vec::new();
        let mut filled = 0usize;
        while filled < CONN_WINDOW - HEADROOM {
            let n = (CONN_WINDOW - HEADROOM - filled).min(STREAM_WINDOW);
            let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
            write_all(&mut s, &payload(n), "filling the connection window").await;
            filled += n;
            lens.push(n);
            sends.push(s);
        }
        assert_eq!(filled, CONN_WINDOW - HEADROOM);
        quiesce(PATIENCE).await;

        // B accepts but does **not** read: §10.3's credit is read-driven, so
        // the window stays spent and the streams stay retained.
        let mut recvs = Vec::new();
        for _ in &lens {
            recvs.push(
                recovering(cb.accept_uni(), "accept_uni")
                    .await
                    .expect("accept_uni"),
            );
        }
        quiesce(NOT_BEFORE).await;

        // A message four times the remaining credit cannot be admitted whole.
        let msg = payload(4 * HEADROOM);
        assert!(
            is_pending(ca.send_message(&msg)).await,
            "ruling 150: `send_message` admits the whole payload or nothing, so \
             with {HEADROOM} bytes of credit a {} byte message parks",
            msg.len()
        );
        quiesce(NOT_BEFORE).await;

        // The dropped future left nothing behind.
        let mut fifth = pin!(cb.accept_uni());
        assert!(
            poll_once(fifth.as_mut()).await.is_pending(),
            "CONTRACT-6 §2.5: a dropped `send_message` future has opened no \
             stream and admitted no byte — a fifth uni stream here is the \
             FIN-less half-written stream the decomposing shell leaves, and \
             against a message-mode peer it is §9.8's overflow case"
        );
        // `fifth` is left to fall out of scope: dropping a `Pin<&mut _>`
        // releases nothing (the `pin!` temporary outlives it) and trips
        // `clippy::drop_non_drop`.

        // Free the credit; the same payload must then cross whole.
        for (r, n) in recvs.iter_mut().zip(&lens) {
            drain_exactly(r, *n, "draining the filler streams").await;
        }
        quiesce(PATIENCE).await;

        within(
            ca.send_message(&msg),
            "send_message after the credit returned",
        )
        .await
        .expect("ruling 150's park is not a permanent refusal");

        let mut got_stream = recovering(cb.accept_uni(), "the message's stream")
            .await
            .expect("accept_uni");
        let got = read_to_end(&mut got_stream, "the message's stream").await;
        assert_same_bytes(&got, &msg, "the message admitted after the park");
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S30 — the mixing error, diagnosed
// ══════════════════════════════════════════════════════════════════════

/// **S30 — a conforming maximum-size message is NOT reset (ruling 153).**
///
/// *The highest-value test in this file.* Every other overflow test asserts
/// that the reset **happens**; this one asserts the boundary on the other
/// side, at the only payload size where the two builds disagree.
///
/// **Ruling 153** fixes the predicate as *the highest received offset with
/// no pinned final size*, and — the half that is easy to drop —
/// **`send_message` MUST carry the FIN on its final data frame, never a
/// separate empty FIN.** The two clauses are one mechanism: a receiver
/// watching the highest offset sees 262 144 arrive, and the *same frame*
/// must pin the final size, or for one round trip the stream is a FIN-less
/// window-full unclaimed stream and matches the overflow predicate exactly.
///
/// # Why the claim is armed first, and why the size must be exact
///
/// The scan runs only while a claim is pending or at the instant one is
/// made (§9.8:3177-3179). [`arm_claim`] puts B demonstrably in message mode
/// **before** any byte arrives, so the scan is live for every inbound frame
/// — the state a receiver looping on `recv_message()` is always in. And the
/// payload is exactly `MESSAGE_RECV_MAX`: a smaller one never reaches the
/// predicate, so a test that sent 64 KiB and observed no reset would assert
/// nothing at all.
///
/// # BROKEN BUILD
///
/// * **A `send_message` that finishes the stream with a separate empty FIN
///   frame.** The final data frame carries offset 262 143; the receiver's
///   highest offset reaches `MESSAGE_RECV_MAX` with no final size pinned;
///   the armed scan fires and resets a **conforming** message with
///   `MESSAGE_OVERFLOW`. B's claim never yields and [`recovering`] panics
///   here. In production this build ships as ruling 59's post-mortem: an
///   operator asking why transfers to this peer die at exactly 256 KiB,
///   reading a `MESSAGE_OVERFLOW` trace, and concluding the *application*
///   mixed its receive modes when it did nothing of the kind.
/// * **A predicate that ignores the pinned final size** — identical failure,
///   identical line, even when the FIN does ride the last data frame,
///   because the frame is processed as data before it is processed as a FIN.
/// * **A predicate on *buffered* bytes rather than the highest offset** —
///   passes this test and is separated by
///   [`s30_an_unclaimed_window_full_uni_stream_is_reset_with_message_overflow`];
///   the pair is what bounds the predicate from both sides.
#[tokio::test(start_paused = true)]
async fn s30_a_conforming_maximum_message_is_not_reset() {
    local(async {
        let pair = Pair::seeded(0x5300_0009);
        let (ca, cb) = pair.establish().await;

        // B is in message mode before a single byte arrives.
        arm_claim!(claim, cb, "the armed claim");
        settle().await;

        let want = payload(BOUND);
        within(ca.send_message(&want), "send_message at exactly the bound")
            .await
            .expect("a payload of exactly MESSAGE_RECV_MAX is accepted");

        let got = recovering(claim, "the armed claim over a maximum message")
            .await
            .expect(
                "ruling 153: a conforming MESSAGE_RECV_MAX message must NOT be \
                 reset — if this never resolves, `send_message` emitted a \
                 separate empty FIN frame and the overflow scan ate its own \
                 protocol's largest legal message",
            );
        assert_same_bytes(&got, &want, "S30 the conforming maximum message");

        assert!(
            is_pending(ca.closed()).await && is_pending(cb.closed()).await,
            "nothing here is a connection error"
        );
    })
    .await;
}

/// **S30 — an unclaimed window-full uni stream is reset, with the
/// distinguishable code.**
///
/// Appendix B's assertions (i), (ii) and (iv) (`SPEC.md:6047-6059`). A
/// writes an `open_uni()` stream past `MESSAGE_RECV_MAX` while B **only
/// ever** calls `recv_message()`.
///
/// # BROKEN BUILD
///
/// * **Pre-ruling-51: no overflow rule at all.** A stalls at the window for
///   ever with keepalives flowing in both directions, so liveness never
///   fires and no timeout ever saves it — §9.8:3136-3138 describes exactly
///   this. The blocked write never resolves and [`recovering`] panics; the
///   virtual-time bound is not a courtesy, it is what stops this build from
///   wedging the whole suite.
/// * **Pre-ruling-52: the reset carries `0`.** Assertion (ii) is the whole
///   of ruling 52 — a sender cannot tell `NO_ERROR` from the peer's
///   application calling `reset(0)` or from a dropped `SendStream`, and the
///   one thing it needed to learn was *which* hazard it hit.
/// * **A build that kills the connection instead** — an easy way to make
///   the write resolve in error and pass a weaker test. Assertion (iv)
///   separates it: both `closed()` futures must still be pending after more
///   than `KEEPALIVE_TIMEOUT` (10 s) of virtual time, which is also the
///   positive proof that keepalives kept both ends alive throughout.
#[tokio::test(start_paused = true)]
async fn s30_an_unclaimed_window_full_uni_stream_is_reset_with_message_overflow() {
    local(async {
        let pair = Pair::seeded(0x5300_0010);
        let (ca, cb) = pair.establish().await;

        // B is a message-mode receiver, and only that.
        arm_claim!(_claim, cb, "B's standing claim");
        settle().await;

        let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut s, &payload(STREAM_WINDOW), "filling the stream window").await;

        // The write past the window: it must fail, not stall for ever.
        let err = recovering(s.write(b"one byte past the window"), "the blocked write")
            .await
            .expect_err(
                "ruling 51: an unclaimed window-full stream fails loudly — a \
                 build with no overflow rule stalls here for ever with \
                 keepalives flowing, which is the failure S30 exists to end",
            );
        match err {
            WriteError::Reset(code) => assert_eq!(
                code, MESSAGE_OVERFLOW,
                "ruling 52 / §15.3: the code is `0x06` and **distinguishable**; \
                 `0` is the pre-ruling-52 bug, indistinguishable from the peer's \
                 own `reset(0)` and from a dropped `SendStream`"
            ),
            other => panic!(
                "§9.8:3148-3152: the sender learns through \
                 `WriteError::Reset(MESSAGE_OVERFLOW)`, got {other:?}"
            ),
        }

        // (iv) — and this is not a liveness death in disguise.
        assert!(
            is_pending_for(ca.closed(), KEEPALIVE_TIMEOUT + Duration::from_secs(2)).await,
            "Appendix B:6053-6054 — keepalives kept both sides alive throughout; \
             a build that killed the connection would also resolve the write in \
             error and pass every other assertion here"
        );
        assert!(
            is_pending(cb.closed()).await,
            "the receiver survives its own reset too"
        );
    })
    .await;
}

/// **S30 — the overflow reset trues up connection credit.**
///
/// Appendix B's assertion (iii). §9.8:3150: *"The receive half retires (its
/// bytes count as consumed at the connection level, §10.3)"*.
///
/// # Construction — why four streams, and not one
///
/// One reset stream leaves 262 144 bytes of dead weight against a 1 MiB
/// connection window, and a build with **no** true-up still has 786 432
/// bytes of room. The assertion would pass for free. Four window-full
/// streams are exactly `INITIAL_MAX_DATA`, so a build that never trues up
/// has spent the entire window on streams that no longer exist, and the
/// next message cannot be admitted at all. That is the smallest fixture
/// that separates the builds, and the arithmetic is asserted rather than
/// assumed.
///
/// # BROKEN BUILD
///
/// * **A build that retires the receive half without counting its bytes as
///   consumed.** B's advertised `MAX_DATA` never advances past
///   `INITIAL_MAX_DATA`, A has already sent exactly that much, and the final
///   `send_message` parks for ever — **with no error**, which is the silent
///   wedge §10.3's discard-credit rule exists to prevent. It fails as a
///   [`recovering`] panic on `send_message`.
/// * **A build that resets only one of the four** — the per-stream
///   assertions in the loop fire on the first stream that stalls instead.
#[tokio::test(start_paused = true)]
async fn s30_the_overflow_reset_trues_up_connection_credit() {
    local(async {
        let pair = Pair::seeded(0x5300_0011);
        let (ca, cb) = pair.establish().await;

        arm_claim!(claim, cb, "B's standing claim");
        settle().await;

        let mut streams = Vec::new();
        for _ in 0..4 {
            let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
            write_all(&mut s, &payload(STREAM_WINDOW), "filling a window").await;
            streams.push(s);
        }

        for (i, s) in streams.iter_mut().enumerate() {
            let err = recovering(s.write(b"!"), "the blocked write")
                .await
                .expect_err("every window-full unclaimed stream is reset");
            assert!(
                matches!(err, WriteError::Reset(c) if c == MESSAGE_OVERFLOW),
                "stream {i}: expected Reset(MESSAGE_OVERFLOW), got {err:?}"
            );
        }

        // 1 MiB of dead weight has been trued up — or it has not.
        let m = payload(64 * 1024);
        within(
            ca.send_message(&m),
            "send_message after four overflow resets",
        )
        .await
        .expect(
            "§9.8:3150 / §10.3: the retired bytes count as consumed at the \
             connection level. Without the true-up A has spent the whole \
             INITIAL_MAX_DATA on four streams that no longer exist and this \
             parks for ever, with no error — the silent wedge",
        );

        let got = recovering(claim, "the message after the resets")
            .await
            .expect("delivered");
        assert_same_bytes(&got, &m, "S30 the message after the resets");
    })
    .await;
}

/// **S30 — a stream behind a slower one is reset on its own account.**
///
/// Appendix B says in terms: *"Assert the not-oldest case too"*, and
/// §9.8:3181-3182 is explicit — the check applies to **every** unclaimed
/// window-full stream, *"not merely the oldest — a stream sitting behind a
/// slower one must not evade it"*.
///
/// # BROKEN BUILD
///
/// * **`if let Some(oldest) = unclaimed.front()`** — the natural reading of
///   "surfaces the oldest", carried from `recv_message`'s claim walk into
///   the scan. It inspects the older 4 KiB stream, finds it far under the
///   bound, and stops. The newer stream stalls for ever and the
///   [`recovering`] on its blocked write panics.
/// * **A build that resets every unclaimed stream indiscriminately** —
///   separated by the second half: the older stream never reached
///   `MESSAGE_RECV_MAX`, so it must still be writable. Without that half
///   this test would pass a build that fires on any unclaimed stream at all,
///   which is exactly the unguarded form ruling 51's applying agent refused.
#[tokio::test(start_paused = true)]
async fn s30_a_stream_behind_a_slower_one_is_reset_on_its_own_account() {
    local(async {
        let pair = Pair::seeded(0x5300_0012);
        let (ca, cb) = pair.establish().await;

        arm_claim!(_claim, cb, "B's standing claim");
        settle().await;

        // The older stream: small, incomplete, never near the bound.
        let mut older = within(ca.open_uni(), "open the older stream")
            .await
            .expect("open_uni");
        write_all(&mut older, &payload(4_096), "the older stream").await;
        quiesce(NOT_BEFORE).await;

        // The newer stream, behind it in open order, fills its window.
        let mut newer = within(ca.open_uni(), "open the newer stream")
            .await
            .expect("open_uni");
        write_all(&mut newer, &payload(STREAM_WINDOW), "the newer stream").await;

        let err = recovering(newer.write(b"!"), "the newer stream's blocked write")
            .await
            .expect_err(
                "§9.8:3181-3182: the scan reaches every unclaimed window-full \
                 stream. A build that inspects only `unclaimed.front()` finds \
                 the older 4 KiB stream under the bound and leaves this one \
                 stalled for ever",
            );
        assert!(
            matches!(err, WriteError::Reset(c) if c == MESSAGE_OVERFLOW),
            "the newer stream is reset on its own account, got {err:?}"
        );

        // ...and the older stream, which never reached the bound, is untouched.
        let n = within(older.write(b"still writable"), "the older stream's write")
            .await
            .expect(
                "§9.8:3145-3147: the predicate is *reaching MESSAGE_RECV_MAX*, \
                 not *being unclaimed* — a build that resets every unclaimed \
                 stream kills this one too",
            );
        assert!(n >= 1, "the older stream still accepts bytes");
    })
    .await;
}

/// **S30 — the negative: a slow `accept_uni()` receiver is NOT reset.**
///
/// Appendix B calls this *"what the guard buys"*, and §9.8:3183-3188 says
/// the unguarded form is **worse** than no rule: a receiver in stream mode
/// that is merely slow to call `accept_uni()` is exercising §16.4's
/// backpressure-by-retention, and resetting its stream the moment the sender
/// filled the initial window would break an ordinary lazy accept loop.
///
/// B never calls `recv_message()` here — not once. It is not in message
/// mode, so no evidence exists that nobody will ever accept this stream.
///
/// # BROKEN BUILD
///
/// * **The unguarded overflow check** — the blanket form ruling 51's
///   applying agent refused, and was right to. It resets the stream the
///   instant the window fills, so A's blocked write resolves
///   `Err(Reset(MESSAGE_OVERFLOW))` where it must simply wait. The
///   `is_pending_for` assertion is the one that fires. **Without this test
///   the unguarded build passes every other test in this file**, which is
///   the entire reason it is here.
/// * **A build that treats a stalled writer as a connection fault** — the
///   resumed write must succeed after the reader drains, not merely fail
///   differently.
///
/// Three seconds of stall is asserted: many driver turns, well inside
/// `KEEPALIVE_TIMEOUT` (10 s), so a conforming build cannot fail it.
#[tokio::test(start_paused = true)]
async fn s30_a_slow_accept_uni_receiver_is_not_reset() {
    local(async {
        let pair = Pair::seeded(0x5300_0013);
        let (ca, cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut s, &payload(STREAM_WINDOW), "filling the stream window").await;
        quiesce(PATIENCE).await;

        assert!(
            is_pending_for(s.write(b"more"), Duration::from_secs(3)).await,
            "§9.8:3183-3188: a receiver merely slow to call `accept_uni()` must \
             NOT be reset — this write waits under §16.4's \
             backpressure-by-retention, and an `Err(Reset)` here is the \
             unguarded build breaking an ordinary lazy accept loop"
        );

        // The lazy accept finally lands, the reader drains, credit extends.
        let mut r = recovering(cb.accept_uni(), "the late accept_uni")
            .await
            .expect("accept_uni");
        drain_exactly(&mut r, STREAM_WINDOW, "the late drain").await;
        quiesce(NOT_BEFORE).await;

        let n = within(s.write(b"more"), "the resumed write")
            .await
            .expect("the write resumes once the reader has drained");
        assert!(n >= 1, "§10.3: the read advanced the credit");
    })
    .await;
}

/// **S30 — a lost overflow reset is regenerated until acknowledged.**
///
/// Appendix B:6044-6046 and §9.6:3028-3039. The reset retires the receive
/// half at once, so its frame identity cannot live in stream state: it is
/// retained in the connection's regenerate set and re-emitted on loss
/// **until acknowledged**, and §8.7's "stream state is discarded"
/// termination does not apply to it.
///
/// # BROKEN BUILD
///
/// * **A build that put the reset in the send half's `ResetState`.** A
///   peer-opened uni stream has no send half at all, so the nearest such
///   build attaches it to the retired receive half — where
///   `is_terminal()` → `after_half_freed` → entry removal takes it with the
///   half. The reset is emitted once, lost once, and never regenerated; A's
///   PTO retransmissions are no-op'd and ACKed below §9.2's closed-stream
///   watermark, so liveness never fires and A stays wedged at the window
///   **for ever** — §9.6:3035-3039 describes this failure in the spec's own
///   words. It fails here as a [`recovering`] panic; this is the only test
///   in the file that reaches it.
///
/// # What the loss assertion does and does not prove
///
/// `blackholed()` is counter-proved but not frame-typed — the tap yields
/// sealed datagrams, so no integration test can name a RESET_STREAM. The
/// window is opened immediately before the claim and closed immediately
/// after, and the reset is emitted inside it (§16.7 seals inside the
/// mutating call), so the counter proves the window was lossy and the
/// construction places the reset in the window. Recorded as such in
/// `TESTS-6-message.md` §3.
#[tokio::test(start_paused = true)]
async fn s30_a_lost_overflow_reset_is_regenerated_until_acknowledged() {
    local(async {
        let pair = Pair::seeded(0x5300_0014);
        let (ca, cb) = pair.establish().await;

        let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut s, &payload(STREAM_WINDOW), "filling the stream window").await;
        quiesce(PATIENCE).await;

        // The receiver's next sends die — the reset among them.
        let killed_before = blackholed(&pair);
        pair.net.block_path(pair.b.addr(), pair.a.addr());

        arm_claim!(_claim, cb, "the claim that triggers the reset");
        quiesce(NOT_BEFORE).await;

        pair.net.heal_path(pair.b.addr(), pair.a.addr());
        assert!(
            blackholed(&pair) > killed_before,
            "the window in which B emitted its reset must actually have been \
             lossy — otherwise this is the ordinary reset test under a longer name"
        );

        let err = recovering(s.write(b"!"), "the blocked write after a LOST reset")
            .await
            .expect_err(
                "§9.6:3028-3039: the reset is retained in the connection's \
                 regenerate set and re-emitted until acknowledged. A build that \
                 hung it on the retired half loses it with the half, and A is \
                 wedged at the window for ever",
            );
        assert!(
            matches!(err, WriteError::Reset(c) if c == MESSAGE_OVERFLOW),
            "the regenerated reset carries the same code, got {err:?}"
        );
    })
    .await;
}

/// **S30 — the pending-claim flag is cleared by a claim that returns
/// `Some` (ruling 156).**
///
/// Three phases, and each separates a different build.
///
/// 1. A message is sent and claimed. The claim returns `Some`, which
///    **clears** the flag.
/// 2. A window-full unclaimed `open_uni()` stream, with **no claim
///    outstanding**, must *not* be reset — §9.8's guard again, this time
///    reached through the flag's lifecycle rather than through a receiver
///    that never used message mode at all.
/// 3. The next claim runs the scan **at the instant it is made**
///    (§9.8:3177-3179) and the stream *is* reset.
///
/// # BROKEN BUILD
///
/// * **A flag that is never cleared** — once any claim has been made the
///   receiver is permanently armed, so phase 2's stream is reset with no
///   claim outstanding. The `is_pending_for` assertion fires. This build is
///   attractive precisely because PLAN-6 §4.5 records the *setting* rule as
///   ratified and the *clearing* rule as unstated; ruling 156 settled it,
///   and this is the test that holds it settled.
/// * **A scan that runs only on inbound stream data** — the natural place
///   (`on_stream_frame`) and only half the rule. Phase 3 sends no further
///   data at all: the stream is already window-full and quiesced, so the
///   only thing that can trigger the reset is the claim itself. That build
///   never resets, and phase 3's [`promptly`] panics.
/// * **A flag cleared by a claim that returned `None`** — phase 2 would
///   pass for the wrong reason, but phase 3 still holds, and phase 1's
///   `Some` is what the ruling actually names.
#[tokio::test(start_paused = true)]
async fn s30_the_pending_claim_flag_clears_on_a_successful_claim() {
    local(async {
        let pair = Pair::seeded(0x5300_0015);
        let (ca, cb) = pair.establish().await;

        // ── phase 1: a claim that returns `Some` ───────────────────────────
        arm_claim!(claim1, cb, "the first claim");
        let m = payload(4_096);
        within(ca.send_message(&m), "send_message")
            .await
            .expect("accepted");
        let got = recovering(claim1, "the first claim")
            .await
            .expect("delivered");
        assert_same_bytes(&got, &m, "the message that clears the flag");

        // ── phase 2: no claim outstanding, so no reset ─────────────────────
        let mut s = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut s, &payload(STREAM_WINDOW), "filling the stream window").await;
        quiesce(PATIENCE).await;

        assert!(
            is_pending_for(s.write(b"more"), Duration::from_secs(2)).await,
            "ruling 156: a claim that returned `Some` clears the pending-claim \
             flag. With no claim outstanding §9.8's guard forbids the reset — a \
             build whose flag latches on for ever resets here"
        );

        // ── phase 3: the next claim runs the scan at the instant it is made ─
        arm_claim!(_claim2, cb, "the second claim");

        let err = promptly(s.write(b"more"), "the write after the second claim")
            .await
            .expect_err(
                "§9.8:3177-3179: the scan runs **at the instant a claim is \
                 made**, not only on inbound data — no further byte is sent \
                 here, so a build that scans only in `on_stream_frame` never \
                 fires",
            );
        assert!(
            matches!(err, WriteError::Reset(c) if c == MESSAGE_OVERFLOW),
            "the reset triggered by the claim carries MESSAGE_OVERFLOW, got {err:?}"
        );
    })
    .await;
}

//! **Ruling 259(viii) — the flow-control `Config` knob, end to end.**
//!
//! Ruling 248's item 8 states the complaint the knob answers: *"Today's
//! constants cap a single stream at `256 KiB / RTT` — about 2.5 MiB/s at
//! 100 ms — with no way for a consumer to raise them. `Config` exposes no
//! flow-control knob."* Ruling 259 disposes of it in ruling 82's shape:
//! **the production default is the ratified constant, the knob raises what
//! this endpoint advertises, and nothing on the wire changes shape.**
//!
//! # What separates the builds
//!
//! Every raised-window assertion below is written so that **the build
//! before this ruling fails it**, which is working rule 9's bar. The
//! measurement is *where the writer parks*, and it is asserted with
//! `assert_eq!` against an exact byte count, never with `<=`:
//!
//! * **The pre-knob build** — `Config` has no windows, every connection is
//!   born on `INITIAL_MAX_STREAM_DATA` / `INITIAL_MAX_DATA`. It parks at
//!   262 144 where the raised build parks at 524 288. Red.
//! * **A build that stores the knob and never advertises it** — the subtle
//!   one, and the one this ruling's implementation had to be designed
//!   around. §10.2's initial windows are *never sent on the wire*, so a
//!   peer assumes the **constants** until a credit frame says otherwise. A
//!   build that widens its own ledger and emits no MAX_STREAM_DATA parks
//!   its peer at 262 144 exactly like the pre-knob build. Red, same
//!   assertion.
//! * **A build that advertises the raise on only one birth path** —
//!   `connect()`'s pending and `accept()`'s established are separate
//!   constructors. [`a_raised_stream_window_admits_more_in_both_directions`]
//!   drives the same measurement over both, so a knob wired to one is red
//!   on the other half of one test.
//! * **A build that grants unconditionally** — never parks at all, so the
//!   `blocked` flag is red even though the byte count is not.
//!
//! # What must *not* move
//!
//! [`the_defaults_are_the_ratified_constants`] is the mirror image: a
//! `Config` nobody configured must still park at exactly the two ratified
//! constants. And [`the_message_bound_does_not_move_with_the_window`] pins
//! the design's one subtle corner — §9.8's `MESSAGE_RECV_MAX` is a
//! *cross-peer* contract checked on the send side, so it stays equal to
//! `INITIAL_MAX_STREAM_DATA` however wide this endpoint's streams are.

use std::future::Future;
use std::time::Duration;

use slither::ConfigError;
use slither::constants::{INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA, MESSAGE_RECV_MAX};
use slither::testutil::{Pair, TestSendStream, local};
use slither::{Config, MessageError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE — the same instruments `tests/story_streams.rs` uses, because
// the measurement is the same one: `timeout` on the paused clock, one
// side reporting *resolved*, the other *not yet*. There is no `sleep`.
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve.
const PATIENCE: Duration = Duration::from_secs(5);

/// Virtual-time budget for the **"not before"** half — long enough for
/// every driver turn and wire delay here, and far inside `DEAD_TIMEOUT`
/// (25 s), so a stall never passes by killing the connection.
///
/// It is also long enough for a **round trip**, which this suite needs and
/// `story_streams.rs` does not: the raise announcement is a MAX_STREAM_DATA
/// the receiver emits on the first frame the sender puts on the stream, so
/// a writer filling a raised window crosses the ratified 262 144 only once
/// that frame has come back. A budget shorter than an RTT would report the
/// raised build as parked at the default and read exactly like the bug.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// A payload whose every byte is a function of its offset.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Write until the shell reports `Pending` for a whole [`NOT_BEFORE`].
///
/// Returns the bytes accepted and whether it ended parked. Identical in
/// shape to `story_streams.rs`'s helper, and deliberately so: the two
/// suites must measure the same thing for their byte counts to be
/// comparable.
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

/// A raise that is a clean multiple of both constants, so an off-by-one in
/// the plumbing cannot be mistaken for rounding.
const RAISED_STREAM: u64 = 2 * INITIAL_MAX_STREAM_DATA; // 524 288
const RAISED_CONNECTION: u64 = 2 * INITIAL_MAX_DATA; // 2 097 152

fn raised() -> Config {
    Config::new()
        .with_flow_windows(RAISED_STREAM, RAISED_CONNECTION)
        .expect("a raise inside the varint bound")
}

// ══════════════════════════════════════════════════════════════════════
// The defaults do not move
// ══════════════════════════════════════════════════════════════════════

/// **The ratified constants stay the defaults**, observed where it counts:
/// on the wire, by a peer that configured nothing.
///
/// This is the assertion the knob is allowed to leave standing and nothing
/// else. If a future build makes some other value the default — "a nicer
/// round number", "matching quinn" — this is red, and per `CLAUDE.md` that
/// red means *this needs a ruling*, not an updated expectation.
///
/// # BROKEN BUILD
///
/// * A build whose default window is anything but 262 144 / 1 048 576.
/// * A build that emits a **spurious** initial MAX_STREAM_DATA — H15's bug.
///   The park would land above 262 144 and the byte count is `assert_eq!`.
#[tokio::test(start_paused = true)]
async fn the_defaults_are_the_ratified_constants() {
    local(async {
        let pair = Pair::seeded(0x5259_0001);
        let (ca, _cb) = pair.establish().await;

        let over = payload(INITIAL_MAX_STREAM_DATA as usize + 16 * 1024);
        let mut s = within(ca.open_uni(), "open").await.expect("open");
        let (accepted, blocked) = fill_until_blocked(&mut s, &over, "default fill").await;

        assert!(blocked, "§10.1: an unread stream parks its own writer");
        assert_eq!(
            accepted, INITIAL_MAX_STREAM_DATA as usize,
            "§10.2: a `Config::default` endpoint advertises exactly \
             `INITIAL_MAX_STREAM_DATA`, and ruling 259(viii) changes that for \
             nobody who did not ask"
        );

        // The connection level, from the same side of the same connection.
        let mut held = vec![s];
        let mut total = accepted;
        while total < INITIAL_MAX_DATA as usize {
            let mut f = within(ca.open_uni(), "open filler").await.expect("open");
            let (got, _) = fill_until_blocked(&mut f, &over, "filler").await;
            assert!(
                got >= 1,
                "{total} of {INITIAL_MAX_DATA} spent: room remains"
            );
            total += got;
            held.push(f);
            assert!(held.len() < 16, "the window is reached in a few streams");
        }
        assert_eq!(
            total, INITIAL_MAX_DATA as usize,
            "§10.2: a `Config::default` endpoint advertises exactly \
             `INITIAL_MAX_DATA` at the connection level"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// The knob raises what this endpoint advertises
// ══════════════════════════════════════════════════════════════════════

/// **The separating test.** The same transfer that parks at the default's
/// window does not park there once the window is raised — and it is driven
/// **in both directions**, because a connection is born on two paths.
///
/// `Pair` dials `a` → `b`, so `b`'s connection came out of `accept()`'s
/// established constructor and `a`'s out of `connect()`'s pending one. The
/// first phase measures what **b** advertises, the second what **a** does.
/// A knob threaded into one constructor and not the other passes one phase
/// and fails the next.
///
/// # BROKEN BUILD
///
/// * **Pre-knob**, or **knob stored but never announced**: parks at
///   262 144. §10.2's initial windows never appear on the wire, so a
///   receiver that widens its ledger in silence is indistinguishable from
///   one that did not widen it.
/// * **Grants unconditionally**: `blocked` is false.
///
/// Both phases here use uni streams, so **both receive halves are
/// peer-opened**: the opener sends (§9.1), so the receiver's half always
/// belongs to a stream its peer opened. The locally-opened receive half is
/// the bidi case, and it is
/// [`a_raised_window_reaches_a_locally_opened_bidi_half`]'s to separate.
#[tokio::test(start_paused = true)]
async fn a_raised_stream_window_admits_more_in_both_directions() {
    local(async {
        let pair = Pair::seeded_with(0x5259_0002, raised());
        let (ca, cb) = pair.establish().await;

        let over = payload(RAISED_STREAM as usize + 16 * 1024);

        // ── phase 1: a → b. `b` accepted, so this is `established()`. ──
        let mut s = within(ca.open_uni(), "a opens").await.expect("open");
        let (accepted, blocked) = fill_until_blocked(&mut s, &over, "a → b").await;
        assert!(blocked, "§10.1: an unread stream still parks its writer");
        assert_eq!(
            accepted, RAISED_STREAM as usize,
            "ruling 259(viii): the accepted side advertises the configured \
             stream window. A build that ignores the knob — or widens its \
             ledger without emitting MAX_STREAM_DATA — parks at \
             {INITIAL_MAX_STREAM_DATA} here"
        );
        assert!(
            accepted > INITIAL_MAX_STREAM_DATA as usize,
            "the knob raises: the same transfer that stalls at the default's \
             window did not stall here"
        );

        // ── phase 2: b → a. `a` dialled, so this is `connecting()`. ──
        let mut t = within(cb.open_uni(), "b opens").await.expect("open");
        let (back, blocked_back) = fill_until_blocked(&mut t, &over, "b → a").await;
        assert!(blocked_back, "§10.1: the reverse direction parks too");
        assert_eq!(
            back, RAISED_STREAM as usize,
            "ruling 259(viii): the dialled side advertises the same configured \
             window — one endpoint policy, both birth paths"
        );

        drop((s, t));
    })
    .await;
}

/// The connection level moves with its own knob, and binds a stream whose
/// own window is untouched — §10.1's two levels stay two levels.
///
/// # BROKEN BUILD
///
/// * **Pre-knob**: the sum over streams stops at 1 048 576.
/// * **One ledger, both levels collapsed**: the first stream would park at
///   the *connection* window rather than at 524 288, which phase 1 of the
///   sibling test already refutes; here the sum lands on 2 097 152 rather
///   than on a multiple of the stream window.
#[tokio::test(start_paused = true)]
async fn a_raised_connection_window_admits_more_across_streams() {
    local(async {
        let pair = Pair::seeded_with(0x5259_0003, raised());
        let (ca, _cb) = pair.establish().await;

        let over = payload(RAISED_STREAM as usize + 16 * 1024);
        // Held, never dropped: dropping a `SendStream` without `finish()`
        // resets it, and §10.3's retirement true-up would hand the credit
        // straight back — collapsing the exhaustion this is building.
        let mut held: Vec<TestSendStream> = Vec::new();
        let mut total = 0usize;
        while total < RAISED_CONNECTION as usize {
            let mut f = within(ca.open_uni(), "open filler").await.expect("open");
            let (got, _) = fill_until_blocked(&mut f, &over, "filler").await;
            assert!(
                got >= 1,
                "{total} of {RAISED_CONNECTION} connection bytes spent, so a \
                 fresh stream must accept at least one byte"
            );
            total += got;
            held.push(f);
            assert!(held.len() < 16, "the window is reached in a few streams");
        }
        assert_eq!(
            total, RAISED_CONNECTION as usize,
            "ruling 259(viii): the connection-level sum lands on the configured \
             window exactly. A build that ignores the knob stops at \
             {INITIAL_MAX_DATA}"
        );
        assert!(
            total > INITIAL_MAX_DATA as usize,
            "the knob raises the connection level too"
        );
    })
    .await;
}

/// **The locally-opened receive half — the case that decides *when* the
/// raise is announced.**
///
/// §9.1 gives a bidi stream both halves at both ends, so the side that
/// *opens* one holds a receive half for a stream the peer has not seen
/// until the first STREAM frame arrives. That ordering is the whole
/// argument for announcing the raise on the peer's first frame rather than
/// at open:
///
/// * A connection packs **credit grants before the stream fill**
///   (`pack_control`, then `Streams::fill`), so a MAX_STREAM_DATA owed at
///   open leaves in the very packet that first names the stream — *ahead*
///   of the frame that names it.
/// * §8.4 makes credit for a stream in the sender's own space that the
///   receiver has not seen **inert**: not a violation, not a wake, just
///   dropped. The raise evaporates and the peer stays on 262 144 until
///   the application reads, which on this test it never does.
///
/// # BROKEN BUILD
///
/// * **Announce at open**: the grant is inert on arrival, `b` parks at
///   262 144.
/// * **Announce only for peer-opened halves**: `a`'s half here is its own,
///   so nothing is ever owed and `b` parks at 262 144.
/// * **Pre-knob, or stored-but-never-announced**: 262 144, as everywhere
///   else in this file.
#[tokio::test(start_paused = true)]
async fn a_raised_window_reaches_a_locally_opened_bidi_half() {
    local(async {
        let pair = Pair::seeded_with(0x5259_0005, raised());
        let (ca, cb) = pair.establish().await;

        // `a` opens, so on `a`'s side this stream's receive half is
        // **locally opened** — the case a uni stream cannot produce.
        let a_bi = within(ca.open_bi(), "a.open_bi").await.expect("open_bi");
        let (mut a_send, _a_recv) = a_bi.split();
        within(a_send.write(b"hello"), "a's first write")
            .await
            .expect("write");

        let b_bi = within(cb.accept_bi(), "b.accept_bi")
            .await
            .expect("accept_bi");
        let (mut b_send, _b_recv) = b_bi.split();

        // `_a_recv` is held and never read: the park has to come from
        // credit, not from a reader that drained it.
        let over = payload(RAISED_STREAM as usize + 16 * 1024);
        let (accepted, blocked) = fill_until_blocked(&mut b_send, &over, "b → a's bi half").await;

        assert!(blocked, "§10.1: an unread half parks its writer");
        assert_eq!(
            accepted, RAISED_STREAM as usize,
            "ruling 259(viii): a receive half on a stream *we* opened carries \
             the configured window too, and the announcement reaches the peer \
             — a build that owed it at open sent it before the frame that \
             names the stream, and §8.4 dropped it"
        );

        drop((a_send, b_send));
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// The corner: §9.8's message bound does not move
// ══════════════════════════════════════════════════════════════════════

/// **`MESSAGE_RECV_MAX` is a cross-peer contract and the knob does not
/// touch it.**
///
/// §9.8's table defines `MESSAGE_RECV_MAX = INITIAL_MAX_STREAM_DATA` and
/// `constants.rs` asserts the equality at compile time. The bound is
/// checked on the **send** side, and a sender cannot know what its receiver
/// configured — the initial windows are un-negotiated. A build that let the
/// knob raise the message bound would emit a payload that any default peer
/// resets with `MESSAGE_OVERFLOW`, which is ruling 59's *"transfers die at
/// exactly 256 KiB"* post-mortem with a new cause.
///
/// So: on an endpoint whose streams are twice as wide, a message of exactly
/// `MESSAGE_RECV_MAX` still round-trips and one byte more is still
/// `TooLarge`. Both sides of the boundary, because a bound tested from one
/// side is satisfiable by a build that has no bound at all.
#[tokio::test(start_paused = true)]
async fn the_message_bound_does_not_move_with_the_window() {
    local(async {
        assert_eq!(
            MESSAGE_RECV_MAX, INITIAL_MAX_STREAM_DATA,
            "§9.8's table defines the one as the other; the knob leaves both \
             constants where ruling 103 pinned them"
        );

        let pair = Pair::seeded_with(0x5259_0004, raised());
        let (ca, cb) = pair.establish().await;

        let at_the_bound = payload(MESSAGE_RECV_MAX as usize);
        within(ca.send_message(&at_the_bound), "send at the bound")
            .await
            .expect("§9.8: a message of exactly MESSAGE_RECV_MAX is legal");
        let got = within(cb.recv_message(), "recv at the bound")
            .await
            .expect("the message arrives");
        assert_eq!(
            got.len(),
            at_the_bound.len(),
            "§9.8: the maximum legal message survives a raised window"
        );

        let over = payload(MESSAGE_RECV_MAX as usize + 1);
        let err = within(ca.send_message(&over), "send over the bound")
            .await
            .expect_err("§9.8: MESSAGE_RECV_MAX + 1 is TooLarge, raised window or not");
        assert!(
            matches!(err, MessageError::TooLarge),
            "§9.8: the message bound is the send-side check, and the knob is \
             not a licence to exceed it: got {err:?}"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// Validation — the knob raises, never lowers
// ══════════════════════════════════════════════════════════════════════

/// The refusals, from **outside** the crate: the window variants sit on
/// `ConfigError` beside the keepalive pair (folded there at integration,
/// ruling 259(viii)), and a consumer must be able to name what it caught.
///
/// The three rejections and the two boundaries they sit on. `config.rs`'s
/// unit tests assert the same predicates from inside; this asserts that the
/// type and its variants are reachable at all, which no `#[cfg(test)]`
/// module can.
#[test]
fn the_knob_refuses_to_lower_or_to_overflow() {
    // Below either ratified default — refused, not clamped. Lowering
    // re-opens every sizing proof that rests on the constants.
    assert_eq!(
        Config::new()
            .with_flow_windows(INITIAL_MAX_STREAM_DATA - 1, INITIAL_MAX_DATA)
            .unwrap_err(),
        ConfigError::WindowTooSmall
    );
    assert_eq!(
        Config::new()
            .with_flow_windows(INITIAL_MAX_STREAM_DATA, INITIAL_MAX_DATA - 1)
            .unwrap_err(),
        ConfigError::WindowTooSmall
    );
    // At the defaults: a legal no-op raise, so the refusal is `<`, not `<=`.
    assert!(
        Config::new()
            .with_flow_windows(INITIAL_MAX_STREAM_DATA, INITIAL_MAX_DATA)
            .is_ok()
    );
    // Past what a §8.1 varint carries — the only ceiling an existing
    // invariant demands (`constants.rs` pins the same bound on the
    // defaults). 2^62 - 1 is admissible; 2^62 is not.
    let varint_max = (1u64 << 62) - 1;
    assert!(
        Config::new()
            .with_flow_windows(varint_max, varint_max)
            .is_ok()
    );
    assert_eq!(
        Config::new()
            .with_flow_windows(varint_max, varint_max + 1)
            .unwrap_err(),
        ConfigError::WindowTooLarge
    );
    // `INITIAL_MAX_STREAM_DATA <= INITIAL_MAX_DATA` is a `constants.rs`
    // const-assert for the defaults; the configured pair keeps it. Both
    // values here are legal raises on their own — the pair is what is
    // refused, which is why `TooSmall` cannot fire first and mask it.
    assert_eq!(
        Config::new()
            .with_flow_windows(RAISED_CONNECTION, INITIAL_MAX_DATA)
            .unwrap_err(),
        ConfigError::StreamWindowAboveConnection
    );
}

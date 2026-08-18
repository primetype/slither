//! **The traced clauses of S25 and S30 — §18.2's operator contract, driven.**
//!
//! This file closes gap **G7**, which `tests/spec_shell.rs:53-61` records
//! against itself:
//!
//! > **G7 — the *traced* half of ruling 49 needs a dev-dependency that does
//! > not exist.** §16.3 makes it a MUST that a failing `send_to` is traced
//! > against the connection under §18.2's operator contract, and Appendix B
//! > asks the test to assert "a `slither::io` trace was emitted **per failed
//! > send** carrying the destination address". `tracing` is a dependency;
//! > `tracing-subscriber` (or any capturing layer) is not in
//! > `[dev-dependencies]`, so nothing here can observe an event.
//!
//! It is closed **without** a dev-dependency: `slither::testutil::Capture`
//! is a hand-rolled `Subscriber` over the `tracing` API slither already
//! depends on (`src/testutil/capture.rs` says why at length).
//!
//! # Why these two obligations need a test of their own
//!
//! Both are MUSTs whose *entire* observable consequence is a trace. §18.1's
//! error taxonomy is closed by ruling, so neither resolves a verb in error,
//! neither produces a `Notification`, and neither changes a byte on the
//! wire:
//!
//! * **S25 / ruling 49** — a failing `send_to` is traced against the
//!   connection and **not acted on**. Every other test in the suite can only
//!   see the *not acted on* half, which a build emitting nothing at all
//!   satisfies perfectly.
//! * **S30 / ruling 59** — the receiver's `MESSAGE_OVERFLOW` reset is traced
//!   under `slither::frames`. The **sender** learns by error code and
//!   `tests/story_message.rs` pins that; the **receiver**, the end whose verb
//!   choice caused the conflict, *"has no error and no notification"*
//!   (`STORIES.md` S30) — so the trace is the only thing that exists to test.
//!
//! In both cases a build that deleted the `tracing::warn!` would keep the
//! whole suite green before this file. That is what each mutant below was
//! run to confirm.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written from `STORIES.md` S25/S30, `SPEC.md` §16.3/§18.2/§9.8 and gap G7.
//! The emit sites were read to learn the field *names* — which are not in the
//! spec, only the payloads are — and the two expected event shapes were
//! measured by driving the protocol before a single assertion was written.

use std::future::Future;
use std::io;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use slither::WriteError;
use slither::constants::{MESSAGE_OVERFLOW, MESSAGE_RECV_MAX};
use slither::testutil::{
    Capture, CapturedEvent, ENETUNREACH, FlakyPolicy, Pair, TestSendStream, local, settle,
};
use tokio::time::Instant;

// ══════════════════════════════════════════════════════════════════════
// Literals, and the pins that tie them to the ratified constants
// ══════════════════════════════════════════════════════════════════════
//
// The traced values below are asserted as **literals** — `"6"`, `"262144"`,
// `"2"` — and never as `MESSAGE_OVERFLOW.to_string()`. Ruling 271's
// valve-pin lesson: a test written in terms of a constant follows that
// constant when it drifts, and asserts nothing about the number the wire
// froze. These two static asserts are the other half of the pin: they turn
// *this* file red if a ratified value moves, instead of letting the moved
// value quietly satisfy the assertions.

/// §15.3 / ruling 52: `MESSAGE_OVERFLOW` is `0x06`, and **distinguishable**
/// from the peer's own `reset(0)`.
const _: () = assert!(
    MESSAGE_OVERFLOW == 0x06,
    "§15.3: MESSAGE_OVERFLOW is 0x06 — the traced `error_code` below is the \
     literal 6, and a drift here needs a ruling, not an updated expectation"
);

/// §9.8: `MESSAGE_RECV_MAX` is 262 144, which is also the final size the
/// receiver reports for the stream it resets.
const _: () = assert!(
    MESSAGE_RECV_MAX == 262_144,
    "§9.8: MESSAGE_RECV_MAX is 262 144 — the traced `final_size` below is the \
     literal 262144"
);

/// The §18.2 target for `Wire::send_to` failures.
const IO: &str = "slither::io";

/// The §18.2 target for the message-mode overflow reset.
const FRAMES: &str = "slither::frames";

// ══════════════════════════════════════════════════════════════════════
// Budgets — virtual time, never a sleep
// ══════════════════════════════════════════════════════════════════════

/// Enough virtual time for a 256 KiB transfer over a perfect wire, with
/// slow start and §12's 25 ms ack delay in the loop.
const PATIENCE: Duration = Duration::from_secs(60);

/// Long enough for every driver turn these tests create, short enough to
/// stay far inside `DEAD_TIMEOUT` (25 s) — so "still pending" means "alive",
/// not "the death timer has not run yet".
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// The injected `ENETUNREACH` window. Appendix B's fixture is *"for a
/// bounded interval, then heals"*, and 2 s is comfortably inside
/// `DEAD_TIMEOUT`, so nothing here can die of the outage.
const OUTAGE: Duration = Duration::from_secs(2);

async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// `true` if `fut` had **not** resolved within [`NOT_BEFORE`].
async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

/// Poll `fut` exactly once — the instrument for "make a claim without
/// awaiting it".
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// Make a `recv_message()` claim without awaiting it, binding the parked
/// future in the caller's scope.
///
/// §9.8 runs the overflow scan *at the instant such a claim is made* and
/// thereafter while it is pending, so this is the arming action that puts
/// the receiver demonstrably in **message mode** — the mode conflict S30 is
/// about. Copied in shape from `tests/story_message.rs`, which owns the
/// original.
macro_rules! arm_claim {
    ($claim:ident, $conn:expr) => {
        let mut $claim = pin!($conn.recv_message());
        assert!(
            poll_once($claim.as_mut()).await.is_pending(),
            "nothing is claimable yet, so this poll must park — it is what puts \
             the receiver in message mode (§9.8)"
        );
    };
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8]) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), "filling the stream window")
            .await
            .unwrap_or_else(|e| panic!("write failed with {e:?}"));
        assert!(n >= 1, "§16.2: a blocked write parks, it does not return 0");
        done += n;
    }
}

/// A payload whose every byte is a function of its offset.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// The value reported for `name`, with a failure that names the whole event.
///
/// §18.2 states what a target **carries**, so an absent field is the defect
/// this helper exists to report precisely: `None` here is "the payload the
/// operator contract promises is not in the event at all".
fn field<'e>(event: &'e CapturedEvent, name: &str) -> &'e str {
    event.field(name).unwrap_or_else(|| {
        panic!(
            "§18.2: the `{}` event carries no `{name}` field. Whole event: {:?}",
            event.target, event.fields
        )
    })
}

// ══════════════════════════════════════════════════════════════════════
// S25 — a failing `send_to` is traced against the connection (ruling 49)
// ══════════════════════════════════════════════════════════════════════

/// **S25 — the traced clause, in full.**
///
/// `STORIES.md` S25: *"a failing `send_to` is **traced** against the
/// connection (§18.2's operator contract), so a `DEAD_TIMEOUT` death is
/// explicable rather than a bare timeout."* §16.3:6006-6012 states it as a
/// MUST on the driver, *"carrying the destination address and the underlying
/// error"*, and §18.2's `slither::io` row names all three payloads:
///
/// > `Wire::send_to` failures, **against the connection whose datagram it
/// > was**, with **the destination address** and **the underlying
/// > `io::Error`**.
///
/// Appendix B asks for one thing more, which is the assertion with the most
/// separating power here: *"a `slither::io` trace was emitted **per failed
/// send**"*. That is asserted from **both** sides — the count equals the
/// number of sends the fabric actually failed, and after the seam heals the
/// count stops growing.
///
/// # The fixture
///
/// `FlakyPolicy::failing_sends_until` — ruling 49's `ENETUNREACH` injection,
/// which is Appendix B's literal fixture: *"a `Wire` whose `send_to` returns
/// `ENETUNREACH` for a bounded interval, then heals"*. Three application
/// datagrams are sent inside the window, one per driver turn, so the
/// per-send obligation has three chances to be got wrong.
///
/// The failed-send count is **computed from the fabric**, not assumed:
/// `Network::sends()` counts every `send_to` before any policy decision and
/// the tap records only those that survived it, so with no partition and no
/// blocked path in play their difference is exactly the number of `send_to`
/// calls that returned `Err`.
///
/// # BROKEN BUILD
///
/// * **A build that traces nothing** — the pre-G7 state of the world, which
///   every other test in this suite passes. `traced.is_empty()` fires.
/// * **A build that traces once and latches** ("we already warned about this
///   wire") — the count assertion fires: 1 event against 3 failed sends.
///   *"Per failed send"* is the clause that separates them, and a test that
///   only asserted "something was logged" would pass on this build.
/// * **A build that traces every send rather than every *failed* send** —
///   the post-heal assertion fires. This is the other side of the bound:
///   without it, "one event per failed send" is satisfied for free by a
///   build that logs unconditionally.
/// * **A build that traces without attribution** — `conn` absent, or `None`
///   where a connection owns the datagram. That is `IMPLEMENTATION-3b.md`'s
///   finding U7 leaking past the handshake, and it is precisely what makes a
///   `DEAD_TIMEOUT` inexplicable: an operator with a warning that names no
///   connection cannot tie the outage to the session that died of it.
/// * **A build that logs the local address** rather than the destination —
///   the `to` assertion compares against B's address, and A's differs.
/// * **A build that acts on the failure** — kills the connection, or
///   resolves a verb in error. The final assertions fire. §18.1's taxonomy
///   is closed by ruling and `send_datagram` returned `Ok` throughout.
#[tokio::test(start_paused = true)]
async fn s25_a_failing_send_to_is_traced_against_the_connection() {
    local(async {
        // Installed inside the `LocalSet`: `tracing`'s scoped default is
        // thread-local, and the driver is a `!Send` actor on this thread.
        let capture = Capture::install();

        let pair = Pair::seeded(0x2500_0049);
        let (ca, cb) = pair.establish().await;
        settle().await;

        // The handshake is not what this test is about.
        capture.clear();
        let sends_before = pair.net.sends();
        let tapped_before = pair.net.tap().len();

        // Ruling 49's injection: `ENETUNREACH` for a bounded interval.
        pair.a
            .wire
            .set_policy(FlakyPolicy::perfect().failing_sends_until(Instant::now() + OUTAGE));

        for i in 0..3u8 {
            ca.send_datagram(&[i; 32]).unwrap_or_else(|e| {
                panic!(
                    "§18.1 is closed: a send failure raises no application error, \
                     and `send_datagram` never learns of one — got {e:?}"
                )
            });
            settle().await;
        }

        let failed = (pair.net.sends() - sends_before) - (pair.net.tap().len() - tapped_before);
        assert_eq!(
            failed, 3,
            "fixture check: three datagrams, one driver turn each, every one \
             met the injected `ENETUNREACH`. If this is not 3 the assertions \
             below are measuring something other than a failed send"
        );

        let traced = capture.with_target(IO);
        assert!(
            !traced.is_empty(),
            "§16.3:6006-6012 is a MUST and §18.2's `slither::io` row is \
             contract: a failing `send_to` must be traced. A build that emits \
             nothing passes every other test in this suite — that is gap G7"
        );
        assert_eq!(
            traced.len(),
            failed,
            "Appendix B: a `slither::io` trace **per failed send**. Got \
             {} event(s) for {failed} failed sends — a build that traces once \
             and latches, or one that batches, lands here",
            traced.len()
        );

        let expected_error = io::Error::from_raw_os_error(ENETUNREACH).to_string();
        for event in &traced {
            assert_eq!(
                event.level, "WARN",
                "a failed send is an operator-facing anomaly; below INFO it is \
                 filtered out of an ordinary production subscriber, which \
                 defeats the post-mortem §18.2's `slither::io` row exists for"
            );
            assert_eq!(
                field(event, "verb"),
                "send_to",
                "§18.2's `slither::io` row carries `Wire::send_to` failures — \
                 and `recv_from` failures ride the same target, so the verb is \
                 what tells an operator which half of the seam broke"
            );

            // "against the connection whose datagram it was" (§16.3).
            let conn = field(event, "conn");
            assert!(
                conn.starts_with("Some("),
                "§16.3: the trace is **against the connection whose datagram it \
                 was**. This datagram is a connection-core transmit, so `None` \
                 is finding U7 leaking past the handshake and leaves the \
                 `DEAD_TIMEOUT` post-mortem with no session to attach to. Got \
                 `{conn}`"
            );

            assert_eq!(
                field(event, "to"),
                pair.b.addr().to_string(),
                "§18.2: **the destination address**. The local address would \
                 also be a plausible-looking string and tells an operator \
                 nothing about where the datagram was going"
            );
            assert_eq!(
                field(event, "error"),
                expected_error,
                "§18.2: **the underlying `io::Error`**. Ruling 49's whole point \
                 is \"here is the errno and the address\" rather than a bare \
                 timeout, so a generic \"send failed\" string does not \
                 discharge it"
            );
        }

        // ── The seam heals: the trace stops, and traffic resumes ────────
        //
        // This is the bound from the other side. Without it, "one event per
        // failed send" is satisfied by a build that traces every send.
        tokio::time::advance(OUTAGE + Duration::from_secs(1)).await;
        settle().await;
        capture.clear();
        let sends_mid = pair.net.sends();
        let tapped_mid = pair.net.tap().len();

        ca.send_datagram(b"after the seam heals")
            .expect("§18.1: no application error, before or after");
        settle().await;

        let failed_after = (pair.net.sends() - sends_mid) - (pair.net.tap().len() - tapped_mid);
        assert_eq!(failed_after, 0, "fixture check: the seam healed");
        assert!(
            capture.with_target(IO).is_empty(),
            "a **successful** send is not traced: the obligation is per failed \
             send, and a build that logs unconditionally turns `slither::io` \
             into noise an operator filters out — which is the same as not \
             having it. Got {:?}",
            capture.with_target(IO)
        );

        let got = within(cb.recv_datagram(), "the post-heal datagram")
            .await
            .expect("the connection survived the outage and carries traffic");
        assert_eq!(
            got.as_slice(),
            b"after the seam heals",
            "the connection is the same one that was traced against"
        );

        // ── And the *deliberately not accepted* half of S25 ─────────────
        assert!(
            is_pending(ca.closed()).await && is_pending(cb.closed()).await,
            "S25: a send failure does **not** kill a connection and raises no \
             application error. Liveness is receive-driven by ruling (§7.4), \
             and `ENETUNREACH` is the signal that *precedes* a successful roam \
             (§7.3) — a build that tore down here would still emit every trace \
             asserted above"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S30 — the receiver traces the overflow reset it emitted (ruling 59)
// ══════════════════════════════════════════════════════════════════════

/// **S30 — the ruling-59 clause: the receiver's reset is traced.**
///
/// `STORIES.md` S30: *"the **receiver** — the end whose verb choice actually
/// caused the conflict — has no error and no notification, so it **MUST**
/// trace the reset under §18.2's `slither::frames`, naming the stream, its
/// final size, and the mode conflict."*
///
/// §18.2 (`SPEC.md:7191`) and §9.8 (`SPEC.md:4062-4066`) say the same, and
/// ruling 59 names the post-mortem it exists for: *"an operator asking why
/// transfers to this peer die at exactly 256 KiB, who reads the
/// **receiver's** log to find out."* All three of that sentence's nouns are
/// asserted here.
///
/// # The fixture
///
/// The mixing error itself, in the shape
/// `tests/story_message.rs:986-1054` established: B claims with
/// `recv_message()` and nothing else, A writes an `open_uni()` stream past
/// `MESSAGE_RECV_MAX`, and the receiver resets it with `MESSAGE_OVERFLOW`.
/// That file pins the **sender's** half — `WriteError::Reset(0x06)` — and
/// this one pins the half that has no API surface at all.
///
/// # BROKEN BUILD
///
/// * **A build that resets silently** — ruling 59's exact defect, and the
///   reason it is a MUST: *"the party that can fix the bug is the one with
///   no evidence of it."* `frames.len()` fires. Nothing else in the suite
///   moves, because the sender's error is unchanged.
/// * **A build that omits the final size** — the post-mortem is *"why do
///   transfers die at exactly 256 KiB"*, and the answer is the number. A
///   trace that says only "a stream was reset" does not answer it.
/// * **A build that omits or mis-numbers the stream** — with several streams
///   in flight, an operator cannot tell which transfer died. The id is
///   cross-checked against the **sender's** own `SendStream::id()`, so a
///   build tracing the internal `StreamRef` instead of the §9.1 wire id is
///   separated too.
/// * **A build carrying `0`** — the pre-ruling-52 code, indistinguishable
///   from the peer's own `reset(0)` and from a dropped `SendStream`.
/// * **A build that traces it on the wrong target** — `slither::frames` is
///   contract; renaming or dropping a target is a protocol revision (§18.2).
/// * **A build that traces per data frame** rather than per reset would
///   bury the one event under thousands; exactly one is asserted.
#[tokio::test(start_paused = true)]
async fn s30_the_receiver_traces_the_message_overflow_reset_it_emitted() {
    local(async {
        let capture = Capture::install();

        let pair = Pair::seeded(0x3000_0059);
        let (ca, cb) = pair.establish().await;

        // B is a message-mode receiver, and only that. This is the verb
        // choice that causes the conflict — and the reason ruling 59 puts
        // the trace on *this* end.
        arm_claim!(_claim, cb);
        settle().await;
        capture.clear();

        let mut s = within(ca.open_uni(), "open_uni")
            .await
            .expect("open_uni on a live connection");
        let wire_id = s
            .id()
            .expect("§9.1: an application-held stream handle always has an id in wire v1")
            .as_u64();
        assert_eq!(
            wire_id, 2,
            "§9.1: the first client-initiated unidirectional stream is id 2. \
             Pinned as a literal so the traced `stream` below is checked \
             against the number the wire froze, not against whatever the \
             implementation currently computes"
        );

        write_all(&mut s, &payload(262_144)).await;

        // The write past the window: the sender's half, which
        // `tests/story_message.rs` owns. It is here to prove the reset
        // actually happened before anything is asserted about the trace.
        let err = within(s.write(b"one byte past the window"), "the blocked write")
            .await
            .expect_err("ruling 51: an unclaimed window-full stream fails loudly");
        assert!(
            matches!(err, WriteError::Reset(code) if code == 6),
            "ruling 52 / §15.3: the sender learns `MESSAGE_OVERFLOW` = 0x06, \
             got {err:?}"
        );
        settle().await;

        let frames = capture.with_target(FRAMES);
        assert_eq!(
            frames.len(),
            1,
            "ruling 59 is a **MUST**: the receiver emits a RESET_STREAM and \
             otherwise *\"continues with no error, no notification, and \
             nothing in its API surface to say what it just did\"*, so this \
             one event is the whole of the receiver's evidence. Exactly one, \
             too: a build tracing per data frame buries it. Got {frames:?}"
        );
        let event = &frames[0];

        assert_eq!(
            event.level, "WARN",
            "the operator this exists for is reading a production log after \
             the fact; below INFO the post-mortem has nothing to read"
        );
        // The **sender's** id for this stream is asserted to be 2 above, so
        // this literal is also the join between the two ends' logs — which
        // is the whole use of putting a stream id in an operator's trace.
        // (A third assertion tying the traced value to `wire_id` would
        // follow from those two by transitivity and could never fail on its
        // own; working rule 9 disqualifies it.)
        assert_eq!(
            field(event, "stream"),
            "2",
            "§18.2 / ruling 59: **the stream**, as the §9.1 wire id the sender \
             also sees. A build tracing its internal handle instead leaves an \
             operator holding a number that appears in no other log"
        );
        assert_eq!(
            field(event, "final_size"),
            "262144",
            "§18.2 / ruling 59: **its final size**, and ruling 59's own \
             post-mortem is *\"why do transfers to this peer die at exactly \
             256 KiB\"* — the number is the answer to the question the trace \
             exists to answer. 262 144 is `MESSAGE_RECV_MAX` (§9.8), pinned as \
             a literal"
        );
        assert_eq!(
            field(event, "error_code"),
            "6",
            "§15.3 / ruling 52: `MESSAGE_OVERFLOW` = 0x06, the machine-readable \
             name of the mode conflict, and the same code the sender saw. `0` \
             is the pre-ruling-52 bug"
        );

        // §18.2's third payload — "the mode conflict that caused it" — is
        // the one carried as prose rather than as a field. Both modes must
        // be named, because the whole diagnosis is *which two* verbs were
        // mixed: an operator who reads only "stream reset" learns nothing
        // it did not already know from the code.
        assert!(
            event.message_contains("uni stream") && event.message_contains("messages"),
            "§18.2 / ruling 59: **the mode conflict**. The message must name \
             both modes — uni streams being consumed as messages — because \
             that is the sentence that tells the receiving application it \
             mixed `recv_message()` with `accept_uni()`. Got: {:?}",
            event.message
        );

        // Ruling 59 changes no behaviour: "No API change, no notification,
        // no new error variant — §18.1 stays closed — and no wire change."
        assert!(
            is_pending(ca.closed()).await && is_pending(cb.closed()).await,
            "neither end dies of the mixing error; the reset is bounded and \
             named, not fatal (S30, §9.8)"
        );
    })
    .await;
}

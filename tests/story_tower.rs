//! **Tower — S33.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S33** | a `Service` call opens **one bi stream**, writes the request, finishes, and reads the response to EOF — *the stream is the correlation*; concurrent calls do not head-of-line block each other and `serve()` drives the accepting side; `UnsyncBoxService` composes across the `!Send` boundary. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 8** (Agent B), from
//! `STORIES.md` §I, `.slices/08-composability/CONTRACT-8.md` and the frozen
//! handle surface **alone**, while the implementer wrote `src/compat/`
//! concurrently. Base commit `704a4ae`; `src/compat/` did not exist in that
//! tree, and no line of it has been read.
//!
//! # ⚠ REPORTED CONFLICT — `CONTRACT-8.md` §0.0 against its own §6.2
//!
//! Working rule 3: *report the conflict, do not silently pick one.* These
//! two statements are in the same binding document and cannot both hold.
//!
//! * **§0.0 consequence 3** (rulings 225 + 230, and the brief for this
//!   agent): `impl Service<()> for Connection<H>` with
//!   `Response = BiStream<H>`.
//! * **§6.2**: `impl<'a, S: Handshake> Service<()> for &'a Connection<S>`,
//!   arguing of §0.0's form that *"`PLAN.md` §3.4 writes
//!   `impl Service<()> for Connection`, and **that cannot be written**"* —
//!   because `open_bi(&self)` borrows, `Service::call(&mut self, …)` hands
//!   out an anonymous lifetime `type Future` cannot name, and `Service` has
//!   no GAT. §6.2 calls this *"a finding against `PLAN.md` §3.4, not a
//!   choice: the sketched form does not compile."*
//!
//! §0.0 says it wins where it contradicts the body — but §0.0's Q4/Q5 rows
//! answer the **scope** question (one bi stream per call; no `Rpc`), and the
//! receiver type was never one of PLAN-8's ten open questions. §6.2 raised it
//! independently. So this may be §0.0 restating `PLAN.md` §3.4's shape while
//! ruling on something else.
//!
//! **This suite is written against §0.0 and the brief** — the explicit, most
//! recent, ratified statement. Every call site that depends on the choice is
//! funnelled through [`open_via_service`], so if the implementer took §6.2's
//! shape, **one function changes** and the rest of the file stands.
//!
//! Two things bearing on the ruling, neither of which appears in §0.0:
//!
//! 1. **For §0.0**: `UnsyncBoxService::new` requires `S: 'static`, and
//!    `&'a Connection<S>` is not `'static`. S33's acceptance *"`UnsyncBoxService`
//!    composes"* is therefore **only satisfiable on the owned form** — see
//!    [`s33_unsync_box_service_composes`]. §6.2's shape cannot host it.
//! 2. **Against §0.0**: `ServiceExt::oneshot(self, req)` takes `self` **by
//!    value**, so on the owned form the idiomatic tower call *moves the
//!    `Connection` into the future* and dropping that future drops the last
//!    handle — performing `close(NO_ERROR, "")`. A tower user's most natural
//!    line silently closes the connection. `&'a Connection` is `Copy` and has
//!    no such hazard. **This suite therefore never uses `oneshot` on a
//!    connection**; it uses `ready()` + `call()`, which borrows.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! There is no `sleep`; `tokio::time::timeout` is the observation instrument
//! in both directions, and `spawn_local` is the only spawn (S21).

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tower::util::UnsyncBoxService;
use tower::{Service, ServiceExt};

use slither::compat::serve;
use slither::error::{ConnectError, ConnectionLost};
use slither::testutil::{
    Pair, TestBiStream, TestConnection, TestEndpoint, TestPublicKey, local, settle,
};
/// The request type `Endpoint`'s dialling `Service` takes (`CONTRACT-8.md`
/// §6.1). Named so the `poll_ready` assertion can be fully qualified — with
/// one `Service` impl per type inference would manage, but a fully qualified
/// call says which impl is being asserted about and keeps saying it if a
/// second one is ever added.
type DialRequest = (SocketAddr, TestPublicKey);

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve. Under
/// `DEAD_TIMEOUT` (25 s), so a test that hangs because the connection died
/// cannot report as a timeout on the wrong thing.
const PATIENCE: Duration = Duration::from_secs(20);

/// Virtual-time budget for the **"not before"** half.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// **The one call site that depends on the §0.0-vs-§6.2 conflict above.**
///
/// Written for §0.0's `impl Service<()> for Connection<H>`. If the
/// implementer took §6.2's `impl<'a> Service<()> for &'a Connection<S>`, the
/// whole change is:
///
/// ```ignore
/// async fn open_via_service(conn: &TestConnection) -> Result<TestBiStream, ConnectionLost> {
///     let mut svc = conn;
///     svc.ready().await?.call(()).await
/// }
/// ```
///
/// `ready()` + `call()` rather than `oneshot()`: `oneshot` takes `self` by
/// value, which on the owned form would move the `Connection` into the future
/// and close the connection when that future is dropped.
async fn open_via_service(conn: &mut TestConnection) -> Result<TestBiStream, ConnectionLost> {
    conn.ready().await?.call(()).await
}

/// Read a whole request off a bi stream, echo it back, and end the response
/// with EOF — the server half of "the stream *is* the correlation".
async fn echo(bi: &mut TestBiStream) -> std::io::Result<Vec<u8>> {
    let mut req = Vec::new();
    bi.read_to_end(&mut req).await?;
    bi.write_all(&req).await?;
    bi.shutdown().await?;
    Ok(req)
}

/// A request body whose bytes are a function of the tag and the offset, so a
/// response delivered on the **wrong stream** is caught by content rather
/// than only by length.
fn body(tag: u8, len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    v.push(tag);
    v.extend((1..len).map(|i| ((i * 37 + tag as usize) % 251) as u8));
    v
}

// ══════════════════════════════════════════════════════════════════════
// S33 — a user can drive slither from a `tower::Service`
// ══════════════════════════════════════════════════════════════════════

/// **S33, acceptance 1 — a `Service` call opens *one* bi stream, and the
/// stream is the correlation.**
///
/// > a `Service` call opens **one bi stream**, writes the request, finishes,
/// > and reads the response to EOF. The stream *is* the request/response
/// > correlation, because the wire carries no request id and a `Service` over
/// > the message verb therefore cannot work.
///
/// Ruling 225 is the reason this shape exists at all: a `Service` over §11
/// messages would need a request id the transport does not carry. So "one bi
/// stream per call" is not an implementation detail — it is the whole
/// mechanism, and it is asserted directly rather than inferred from a
/// successful round trip.
///
/// # BROKEN BUILD — what each assertion separates
///
/// * **A `call()` that opens two streams** (one per direction — the
///   `FramedRead`/`FramedWrite` shape leaking in). The second `accept_bi()`
///   resolves instead of parking. A round-trip-only test passes this build.
/// * **A `call()` built over the message verb** (the shape ruling 225
///   declined, and which the manifest comment described until it was
///   corrected). `accept_bi()` never resolves; caught by [`within`].
/// * **A `call()` that opens a uni stream too** — caught by the
///   `accept_uni()` assertion, and it would silently collide with the message
///   verb, which is S30's hazard.
/// * **A response not delimited by EOF** — the server's `shutdown()` is what
///   ends it, and `read_to_end` on the client would never return.
/// * **A request not ended by `finish()`** — the *server's* `read_to_end`
///   would never return, so this is pinned from the far side rather than
///   assumed.
/// * **A `call()` that returns a stream not connected to the request** —
///   caught by the content check, since the body is tag-derived.
#[tokio::test(start_paused = true)]
async fn s33_a_service_call_opens_exactly_one_bi_stream() {
    local(async {
        let pair = Pair::seeded(0x5330_0001);
        let (mut ca, cb) = pair.establish().await;

        let request = body(0x11, 300);
        let want = request.clone();

        let client = async {
            let mut bi = within(open_via_service(&mut ca), "Service::call")
                .await
                .expect("call");
            within(bi.write_all(&request), "write request")
                .await
                .expect("write");
            // The request is ended by `finish()` — through `AsyncWrite` that
            // is `shutdown()`, i.e. ruling 57's finish-then-ack.
            within(bi.shutdown(), "finish the request")
                .await
                .expect("shutdown");

            let mut resp = Vec::new();
            within(bi.read_to_end(&mut resp), "read the response to EOF")
                .await
                .expect("the response is delimited by EOF");
            resp
        };

        let server = async {
            let mut bi = within(cb.accept_bi(), "accept_bi")
                .await
                .expect("accept_bi");
            let req = within(echo(&mut bi), "echo").await.expect("echo");

            // ── the line ruling 225 turns on ───────────────────────────
            //
            // One call, one stream. Nothing else was opened.
            assert!(
                tokio::time::timeout(NOT_BEFORE, cb.accept_bi())
                    .await
                    .is_err(),
                "ruling 225: `call()` opens **one** bi stream and the stream \
                 is the correlation. A second one means the call opened a \
                 stream per direction, which a round-trip test cannot see"
            );
            assert!(
                tokio::time::timeout(NOT_BEFORE, cb.accept_uni())
                    .await
                    .is_err(),
                "`call()` opens no uni stream — one here would also collide \
                 with the message verb (S30)"
            );
            req
        };

        let (resp, req) = tokio::join!(client, server);
        assert_eq!(
            req, want,
            "the server saw the request the client wrote, whole and \
             terminated by the client's `finish()`"
        );
        assert_eq!(
            resp, want,
            "the response came back on the same stream — the correlation \
             slither has, because the wire carries no request id"
        );
    })
    .await;
}

/// **S33, acceptance 2 — concurrent calls do not head-of-line block each
/// other, and `serve()` drives the accepting side.**
///
/// > concurrent calls do not head-of-line block each other. `serve()` drives
/// > the accepting side.
///
/// # The shape of the failure this is aimed at
///
/// `CONTRACT-8.md` §6.3 spells `serve` as *"`accept_bi()` in a loop; per
/// accepted stream, `svc.clone()` and `tokio::task::spawn_local(async move {
/// let _ = svc.call(stream).await; })`"*. The plausible wrong build is the
/// one-character-cheaper version — `let _ = svc.call(stream).await;` **inline
/// in the loop**, with no spawn. It passes any single-call test, passes any
/// test whose calls are sequential, and head-of-line blocks every real
/// consumer.
///
/// So the service is made to **park** on the first call, and the assertion is
/// that the second call completes anyway. Under the inline build the loop
/// never reaches its second `accept_bi()` and the second response never
/// arrives.
///
/// # BROKEN BUILD
///
/// * **`serve` awaiting `svc.call(stream)` inline** instead of
///   `spawn_local`ing it. Caught by [`within`] on the fast response.
/// * **`serve` that accepts one stream and returns** — same signature.
/// * **`serve` using `tokio::spawn`** — S21 and `CONTRACT-8.md` §9 item 6.
///   That does not compile against a `!Send` handle, so it is a hard red
///   rather than a test failure, which is the right outcome.
/// * **A build that serialises *responses* rather than calls** — caught by
///   the "call 1 is still outstanding" probe, which is the not-before half:
///   without it, a build that simply answered both quickly would pass while
///   proving nothing about concurrency.
#[tokio::test(start_paused = true)]
async fn s33_concurrent_calls_do_not_head_of_line_block_each_other() {
    local(async {
        let pair = Pair::seeded(0x5330_0002);
        let (mut ca, cb) = pair.establish().await;

        const SLOW: u8 = 0xAA;
        const FAST: u8 = 0x55;

        // The gate the first call parks on. `Rc`, not `Arc`: nothing here is
        // `Send`, and nothing needs to be.
        let gate = Rc::new(Notify::new());
        let gate_for_svc = Rc::clone(&gate);

        let svc = tower::service_fn(move |mut bi: TestBiStream| {
            let gate = Rc::clone(&gate_for_svc);
            async move {
                let mut req = Vec::new();
                bi.read_to_end(&mut req).await?;
                if req.first() == Some(&SLOW) {
                    gate.notified().await;
                }
                bi.write_all(&req).await?;
                bi.shutdown().await?;
                Ok::<(), std::io::Error>(())
            }
        });

        let client = async {
            let slow_body = body(SLOW, 256);
            let fast_body = body(FAST, 256);

            let mut slow = within(open_via_service(&mut ca), "call 1")
                .await
                .expect("call 1");
            within(slow.write_all(&slow_body), "write 1")
                .await
                .expect("write 1");
            within(slow.shutdown(), "finish 1").await.expect("finish 1");

            let mut fast = within(open_via_service(&mut ca), "call 2")
                .await
                .expect("call 2");
            within(fast.write_all(&fast_body), "write 2")
                .await
                .expect("write 2");
            within(fast.shutdown(), "finish 2").await.expect("finish 2");

            // ── the line the test turns on ─────────────────────────────
            //
            // Call 1's service is parked. Call 2 must complete regardless.
            let mut got_fast = Vec::new();
            within(fast.read_to_end(&mut got_fast), "call 2 response")
                .await
                .expect(
                    "call 2 completed while call 1 was parked — a `serve` that \
                     awaits `svc.call(..)` inline never reaches its second \
                     `accept_bi()`",
                );
            assert_eq!(got_fast, fast_body, "call 2 got call 2's body back");

            // …and call 1 really was still outstanding, so the assertion
            // above is about concurrency and not about two fast calls.
            //
            // A single `read`, not `read_to_end`: this future is cancelled by
            // the timeout, and slither's `poll_read` is documented cancel-safe
            // ("a dropped future has claimed nothing") while `read_to_end` is
            // not.
            let mut probe = [0u8; 1];
            assert!(
                tokio::time::timeout(NOT_BEFORE, slow.read(&mut probe))
                    .await
                    .is_err(),
                "call 1 must still be parked in the service — if it had \
                 already answered, the fast response proved nothing about \
                 head-of-line blocking"
            );

            gate.notify_one();
            settle().await;

            let mut got_slow = Vec::new();
            within(slow.read_to_end(&mut got_slow), "call 1 response")
                .await
                .expect("call 1 completes once its service is released");
            assert_eq!(
                got_slow, slow_body,
                "call 1 got call 1's body back — the two responses did not \
                 cross streams"
            );
        };

        tokio::select! {
            lost = serve(&cb, svc) => panic!(
                "`serve` returned {lost:?} before the client finished — it \
                 runs until the connection dies and has no success exit"
            ),
            () = client => {}
        }
    })
    .await;
}

/// **S33, acceptance 3 — `UnsyncBoxService` composes.**
///
/// > the `!Send` boundary is asserted — `UnsyncBoxService` composes;
/// > `tower::buffer::Buffer`, `spawn_ready`, `BoxService`, hyper and plain
/// > `tokio::spawn` do not, because they spawn onto a work-stealing executor.
///
/// # This test's real content is that it compiles
///
/// `UnsyncBoxService::new` requires `S: Service<T> + 'static` **and**
/// `S::Future: 'static`. That is a load-bearing constraint here rather than a
/// formality: it is the reason the boxed form of the connection service can
/// exist at all, and it is the strongest evidence in this suite for §0.0's
/// owned `impl Service<()> for Connection<H>` over §6.2's borrowed form —
/// `&'a Connection<S>` is not `'static` and cannot be boxed. See the module
/// header.
///
/// The round trip is here so the test is not merely a type assertion: a boxed
/// service that compiles and then hands back a stream connected to nothing
/// would pass a compile-only check.
///
/// # BROKEN BUILD
///
/// * **A `Service::Future` that borrows the connection** — does not satisfy
///   `'static` and fails to compile at `UnsyncBoxService::new`. That is the
///   intended signal, not an accident.
/// * **A `Send` bound anywhere on the path** (S21, `CONTRACT-8.md` §9 item
///   6). `Connection` is `!Send`, so any such bound fails to compile here.
///   The negative half — that `BoxService` and `tokio::spawn` do *not*
///   compose — is not expressible as a passing test without a compile-fail
///   harness; see the report accompanying this slice.
/// * **A boxed service that returns a detached stream** — caught by the round
///   trip.
#[tokio::test(start_paused = true)]
async fn s33_unsync_box_service_composes() {
    local(async {
        let pair = Pair::seeded(0x5330_0003);
        let (ca, cb) = pair.establish().await;

        // The whole assertion: this line type-checks.
        let mut boxed: UnsyncBoxService<(), TestBiStream, ConnectionLost> =
            UnsyncBoxService::new(ca);

        let request = body(0x33, 512);
        let want = request.clone();

        let client = async {
            let mut bi = within(boxed.ready(), "boxed poll_ready")
                .await
                .expect("ready")
                .call(())
                .await
                .expect("call through the boxed service");
            within(bi.write_all(&request), "write")
                .await
                .expect("write");
            within(bi.shutdown(), "finish").await.expect("shutdown");
            let mut resp = Vec::new();
            within(bi.read_to_end(&mut resp), "read response")
                .await
                .expect("read");
            resp
        };

        let server = async {
            let mut bi = within(cb.accept_bi(), "accept_bi")
                .await
                .expect("accept_bi");
            within(echo(&mut bi), "echo").await.expect("echo")
        };

        let (resp, req) = tokio::join!(client, server);
        assert_eq!(req, want, "the boxed service opened a live stream");
        assert_eq!(resp, want, "and the response came back on it");
    })
    .await;
}

/// **S33 / `CONTRACT-8.md` §6.1 — `Endpoint`'s dialling `Service`, including
/// the synchronous error folded into the future.**
///
/// > `Connect<I>` exists only to fold `connect`'s **synchronous** `Err` (e.g.
/// > `ConnectError::AlreadyConnected`, which ruling 87/90 make arrive *before
/// > any await*) into a future, since `Service::call` cannot return a
/// > `Result`.
///
/// §0.0's Q5 row says T5 is *"the `Service` impls plus `serve`"* — plural —
/// so the dialling impl survives ruling 230's cut of `Rpc` and owes a test.
///
/// The `AlreadyConnected` half is the one that matters. `connect()` returns
/// its error **before any await**, so `Service::call` — which cannot return a
/// `Result` — has to carry it. The obvious wrong build unwraps it.
///
/// # BROKEN BUILD
///
/// * **A `call()` that `.expect()`s or `unwrap()`s the synchronous error** —
///   panics on the second dial, and takes the consumer's process with it for
///   a condition that is an ordinary, documented outcome (S3a: "call
///   `connect()` again" is the natural guess and returns `AlreadyConnected`).
/// * **A `Connect<I>` that never resolves on the error path** — caught by
///   [`within`].
/// * **A `poll_ready` that is not always `Ready(Ok(()))`** (§6.1 states it
///   unconditionally) — a tower consumer that waits for readiness before
///   calling would deadlock. Asserted with a single hand-made poll, because a
///   `timeout` cannot tell "Ready at once" from "Ready after the runtime
///   auto-advanced".
/// * **A `Connect<I>` that stores `Connecting`'s *parts* rather than owning
///   it** — §6.1: dropping a `Connecting` is ruling 50's cancellation, and a
///   dropped `Service` future must cancel exactly as `drop(connecting)` does.
///   Not asserted here; flagged in the report as a gap this suite does not
///   cover, since S29 owns cancellation.
#[tokio::test(start_paused = true)]
async fn s33_endpoint_service_dials_and_folds_the_synchronous_error() {
    local(async {
        let mut pair = Pair::seeded(0x5330_0004);
        let b_addr = pair.b.addr();
        let b_key = pair.b.public_static;

        // §6.1: "always Ready(Ok(()))", asserted exactly.
        {
            let mut cx = Context::from_waker(Waker::noop());
            let ready =
                <TestEndpoint as Service<DialRequest>>::poll_ready(&mut pair.a.endpoint, &mut cx);
            assert!(
                matches!(ready, Poll::Ready(Ok(()))),
                "CONTRACT-8.md §6.1: `Endpoint`'s `poll_ready` is always \
                 `Ready(Ok(()))` — a tower consumer that awaits readiness \
                 before calling deadlocks otherwise"
            );
        }

        let dial = async {
            let ep = &mut pair.a.endpoint;
            within(ep.ready(), "endpoint poll_ready")
                .await
                .expect("ready")
                .call((b_addr, b_key))
                .await
        };
        let accept = async {
            let intro = within(pair.b.endpoint.accept(), "accept")
                .await
                .expect("an introduction");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");
            proven.accept().await.expect("accept")
        };
        let (dialed, _accepted) = tokio::join!(dial, accept);
        let _dialed = dialed.expect("the Service dial completed");

        // ── the synchronous error, folded into the future ──────────────
        //
        // A second dial to the same static is `AlreadyConnected`, and
        // `connect()` produces it before any await (rulings 87/90).
        let second = {
            let ep = &mut pair.a.endpoint;
            within(ep.ready(), "endpoint poll_ready (2)")
                .await
                .expect("ready")
                .call((b_addr, b_key))
                .await
        };
        assert!(
            matches!(second, Err(ConnectError::AlreadyConnected)),
            "CONTRACT-8.md §6.1: `Connect<I>` folds `connect`'s synchronous \
             `Err` into the future, because `Service::call` cannot return a \
             `Result`. Got {second:?} — a build that unwraps it panics on a \
             documented, ordinary outcome (S3a)"
        );
    })
    .await;
}

/// **S33 / `CONTRACT-8.md` §6.3 — `serve` returns only when the connection
/// dies, and returns the death itself.**
///
/// > **Returns `ConnectionLost`, not `Result<(), ConnectionLost>`.** `PLAN.md`
/// > §3.4 writes the `Result`, but the loop has no success exit — it runs
/// > until `accept_bi()` fails, and the only way it can fail is the
/// > connection dying. A `Result` whose `Ok` is unreachable is a shape, not
/// > information.
///
/// §6.3 flags the return type as a **deviation from `PLAN.md` §3.4** that the
/// maintainer may reverse ("in which case it is `Err(lost)` always"). This
/// test asserts the ratified §6.3 shape; if the `Result` is chosen instead,
/// the single `assert!(matches!(lost, …))` gains an `Err(..)` wrapper and
/// nothing else moves. That is flagged in the report.
///
/// # BROKEN BUILD
///
/// * **A `serve` that returns when its service errors** — the service's
///   `Response` and `Error` are unconstrained and both discarded (§6.3), so a
///   failing service must not stop the loop. The service here always fails,
///   and `serve` must still be running afterwards.
/// * **A `serve` that returns after the first accepted stream.**
/// * **A `serve` that reports a death other than the one that happened** —
///   caught by the `PeerClosed` match.
/// * **A `serve` that never returns at all**, leaking a task past the
///   connection's death — caught by [`within`].
#[tokio::test(start_paused = true)]
async fn s33_serve_runs_until_the_connection_dies_and_survives_a_failing_service() {
    local(async {
        let pair = Pair::seeded(0x5330_0005);
        let (mut ca, cb) = pair.establish().await;

        // A service that always fails. §6.3: `Svc::Response` and `Svc::Error`
        // are unconstrained and both discarded, and `serve` does not trace
        // them — §18.2's targets are a closed list of five.
        let svc = tower::service_fn(|_bi: TestBiStream| async move {
            Err::<(), std::io::Error>(std::io::Error::other("the service declines"))
        });

        let driver = async {
            // Two calls, both of which the service will fail. `serve` must
            // outlive both.
            for i in 0..2 {
                let mut bi = within(open_via_service(&mut ca), "call")
                    .await
                    .unwrap_or_else(|e| panic!("call {i} failed: {e:?}"));
                within(bi.write_all(b"ping"), "write").await.expect("write");
                within(bi.shutdown(), "finish").await.expect("shutdown");
            }
            settle().await;

            // Now kill it from the far side.
            ca.close(4, b"enough").await;
            settle().await;
        };

        // `join!`, not `select!`: **both** must finish. `serve` returns only
        // when the connection dies, and the driver is what kills it — under
        // `select!` the driver would win the race it starts and the test
        // would assert nothing about `serve` at all.
        let (lost, ()) = tokio::join!(within(serve(&cb, svc), "serve"), driver);

        assert!(
            matches!(lost, ConnectionLost::PeerClosed { code: 4, .. }),
            "CONTRACT-8.md §6.3: `serve` runs until `accept_bi()` fails and \
             the only way it can fail is the connection dying, so it returns \
             **the death**. Got {lost:?}"
        );
    })
    .await;
}

/// **`CONTRACT-8.md` §6.2 — the connection service is ready without a round
/// trip.**
///
/// A `Service` whose `poll_ready` needed the network would make every tower
/// consumer pay a round trip before its first call, and `ServiceExt::ready`
/// would park where `call` would not. §6.2 states no `poll_ready` behaviour
/// for this impl — which working rule 8 reads as the list being exhaustive of
/// what is *specified*, so this asserts the only sane reading and flags the
/// omission in the report.
///
/// # BROKEN BUILD
///
/// * **A `poll_ready` that opens the stream** (doing `call`'s work early), so
///   a consumer that calls `ready()` and then drops the service has leaked a
///   stream the peer will accept. Caught: the peer sees a stream that no
///   `call()` produced.
/// * **A `poll_ready` gated on a driver round trip** — `Pending` here.
#[tokio::test(start_paused = true)]
async fn s33_connection_service_poll_ready_is_immediate_and_opens_nothing() {
    local(async {
        let pair = Pair::seeded(0x5330_0006);
        let (mut ca, cb) = pair.establish().await;

        {
            let mut cx = Context::from_waker(Waker::noop());
            let ready = <TestConnection as Service<()>>::poll_ready(&mut ca, &mut cx);
            assert!(
                matches!(ready, Poll::Ready(Ok(()))),
                "the connection service is ready without a round trip"
            );
        }

        settle().await;
        assert!(
            tokio::time::timeout(NOT_BEFORE, cb.accept_bi())
                .await
                .is_err(),
            "`poll_ready` opens nothing — `call()` is what opens the one bi \
             stream (ruling 225). A `poll_ready` that opened it early leaks a \
             stream whenever a consumer readies a service and then drops it"
        );
    })
    .await;
}

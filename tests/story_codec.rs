//! **Codec — S32.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S32** | `Framed<BiStream, LengthDelimitedCodec>` round-trips typed objects **in order**; `Stream`/`Sink` backpressure maps onto **flow-control credit** rather than onto an intermediate buffer; and a consumer that polls **once** finds that **exactly one** item was claimed (§16.11, §10.6, ruling 58). |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 8** (Agent B), from
//! `STORIES.md` §I, `.slices/08-composability/CONTRACT-8.md` and the frozen
//! handle surface **alone**, while the implementer wrote `src/compat/`
//! concurrently. Base commit `704a4ae`; `src/compat/` did not exist in that
//! tree, and no line of it has been read.
//!
//! # Where ruling 58 is actually pinned, and why not on `Framed`
//!
//! S32's third acceptance — *"a consumer that polls **once** finds that
//! **exactly one** item was claimed"* — cannot be pinned on `Framed`.
//! `Framed` keeps a byte buffer of its own by design: one `poll_next` decodes
//! one frame but may have pulled many frames' **bytes** out of the
//! `AsyncRead` in doing it, and that buffering is `tokio-util`'s business,
//! not slither's. Asserting "one poll, one frame" against `Framed` would
//! restate a `tokio-util` guarantee and separate no slither build at all —
//! working rule 9's "a name is not a pin".
//!
//! The invariant ruling 58 states is about **slither's** adapters, so it is
//! asserted on [`Connection::messages`] in
//! [`s32_a_consumer_that_polls_once_claims_exactly_one_item`], from the side
//! that separates: **drop the adapter after one poll and every unclaimed item
//! must still be there**. A read-ahead adapter has queued them internally and
//! loses them on drop, which is precisely the unbounded intermediate queue
//! §10.6 forbids.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! [`settle`] gives both drivers a turn **without advancing virtual time**;
//! `tokio::time::timeout` is the observation instrument in both directions.
//! There is no `sleep`.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_core::Stream;
use futures_sink::Sink;
use tokio_util::bytes::{Bytes, BytesMut};
use tokio_util::codec::LengthDelimitedCodec;

use slither::constants::{INITIAL_MAX_STREAM_DATA, MAX_PLAINTEXT};
use slither::testutil::{Pair, local, settle};

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

/// Await one item from a `Stream`.
///
/// Written over `futures_core::Stream` directly rather than through
/// `StreamExt`: the `futures-util` dev-dependency `CONTRACT-8.md` §10
/// promises is **not** in `Cargo.toml` at the base commit, and a suite that
/// cannot compile is worth less than four extra lines.
async fn next<S>(stream: &mut S) -> Option<S::Item>
where
    S: Stream + Unpin,
{
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

/// Poll a `Stream` **exactly once** with a no-op waker (`CONTRACT-8.md` §10).
///
/// The whole assertion in
/// [`s32_a_consumer_that_polls_once_claims_exactly_one_item`] is *how many
/// times the adapter was polled*, so the poll is made by hand rather than
/// through a combinator that might poll twice.
fn poll_next_once<S>(stream: &mut S) -> Poll<Option<S::Item>>
where
    S: Stream + Unpin,
{
    let mut cx = Context::from_waker(Waker::noop());
    Pin::new(stream).poll_next(&mut cx)
}

/// `poll_ready` → `start_send` → `poll_flush`, i.e. `SinkExt::send`.
async fn sink_send<S>(sink: &mut S, item: Bytes) -> Result<(), S::Error>
where
    S: Sink<Bytes> + Unpin,
{
    std::future::poll_fn(|cx| Pin::new(&mut *sink).poll_ready(cx)).await?;
    Pin::new(&mut *sink).start_send(item)?;
    std::future::poll_fn(|cx| Pin::new(&mut *sink).poll_flush(cx)).await
}

/// `SinkExt::close` — for `Framed` this flushes and then calls
/// `poll_shutdown` on the inner `BiStream`, i.e. ruling 57's `finish()` **and
/// then** `acked()`.
async fn sink_close<S>(sink: &mut S) -> Result<(), S::Error>
where
    S: Sink<Bytes> + Unpin,
{
    std::future::poll_fn(|cx| Pin::new(&mut *sink).poll_close(cx)).await
}

/// Frame `i`, `len` bytes long, with every byte a function of **both**.
///
/// The index is mixed in so that a build which delivers the right *number* of
/// right-*sized* frames in the wrong order is caught by content. 251 is prime
/// and coprime with every packet size in play.
fn frame(i: usize, len: usize) -> Bytes {
    Bytes::from(
        (0..len)
            .map(|j| ((i * 31 + j) % 251) as u8)
            .collect::<Vec<u8>>(),
    )
}

/// The frame sizes the round trip uses, and why each one is here.
///
/// Working rule 9's one-sided-boundary note: this project has shipped a test
/// that checked `LEN` and `LEN - 1` and never `LEN + 1`. `MAX_PLAINTEXT` is
/// 1170, so all three sides of the packet boundary are present, and so is the
/// **empty frame** — a zero-length payload is legal for
/// `LengthDelimitedCodec` and is exactly the value a decoder that conflates
/// "no bytes" with "no frame" gets wrong.
fn frame_sizes() -> Vec<usize> {
    vec![
        0,
        1,
        MAX_PLAINTEXT - 1,
        MAX_PLAINTEXT,
        MAX_PLAINTEXT + 1,
        4096,
    ]
}

// ══════════════════════════════════════════════════════════════════════
// S32 — a user can stream typed objects with a codec
// ══════════════════════════════════════════════════════════════════════

/// **S32, acceptance 1 — `Framed<BiStream, LengthDelimitedCodec>` round-trips
/// a sequence of objects *in order*.**
///
/// > `Framed<BiStream, LengthDelimitedCodec>` round-trips a sequence of
/// > objects **in order**.
///
/// Both constructors are exercised, because `CONTRACT-8.md` §5 defines them
/// as a pair and a suite that only used `framed_bi` would leave
/// `accept_framed_bi` — the one that has to reach `accept_bi` rather than
/// `open_bi` — untested.
///
/// # BROKEN BUILD — what each assertion separates
///
/// * **An `AsyncRead` for `BiStream` that does not latch EOF.** `Framed`
///   never yields `None` and the terminating assertion dies in [`within`].
/// * **An `AsyncRead` that reports "no data yet" as `Ready(Ok(()))` with
///   nothing filled.** `Framed` reads that as EOF mid-sequence, and either
///   ends early (the count assertion) or reports
///   `io::ErrorKind::UnexpectedEof` from a half-read length prefix.
/// * **A `framed_bi` that swapped the halves** — wired `AsyncWrite` to the
///   receive half or `AsyncRead` to the send half. Nothing round-trips.
/// * **A build that drops or reorders a frame at a packet boundary** —
///   `MAX_PLAINTEXT - 1`, `MAX_PLAINTEXT` and `MAX_PLAINTEXT + 1` are all
///   present, and the index is mixed into the content so a reorder cannot
///   pass by delivering the right sizes.
/// * **A build that swallows the empty frame** — a zero-length payload is a
///   legal frame, and a decoder path that treats "filled nothing" as
///   "nothing to decode" loses it. Caught by the count *and* by the order of
///   everything after it.
#[tokio::test(start_paused = true)]
async fn s32_framed_length_delimited_round_trips_objects_in_order() {
    local(async {
        let pair = Pair::seeded(0x5320_0001);
        let (ca, cb) = pair.establish().await;

        let sizes = frame_sizes();
        let want: Vec<Bytes> = sizes
            .iter()
            .copied()
            .enumerate()
            .map(|(i, n)| frame(i, n))
            .collect();

        let send_side = {
            let want = want.clone();
            async move {
                let mut framed = within(ca.framed_bi(LengthDelimitedCodec::new()), "framed_bi")
                    .await
                    .expect("framed_bi");
                for (i, item) in want.iter().enumerate() {
                    within(sink_send(&mut framed, item.clone()), "sink send")
                        .await
                        .unwrap_or_else(|e| panic!("frame {i} failed to send: {e:?}"));
                }
                // Ends the request half: `Framed::poll_close` flushes and then
                // calls `poll_shutdown`, which is ruling 57's finish-then-ack.
                within(sink_close(&mut framed), "sink close")
                    .await
                    .expect("close");
            }
        };

        let recv_side = async move {
            let mut framed = within(
                cb.accept_framed_bi(LengthDelimitedCodec::new()),
                "accept_framed_bi",
            )
            .await
            .expect("accept_framed_bi");

            let mut got: Vec<BytesMut> = Vec::new();
            for i in 0..sizes.len() {
                let item = within(next(&mut framed), "next frame")
                    .await
                    .unwrap_or_else(|| {
                        panic!(
                            "the stream ended after {i} frames — `Framed` ends at \
                             EOF, so this is an `AsyncRead` that reported EOF \
                             before the sender finished"
                        )
                    })
                    .unwrap_or_else(|e| panic!("frame {i} decoded as an error: {e:?}"));
                got.push(item);
            }

            // `Framed` **does** end, at EOF — it is not one of ruling 226's
            // never-ending adapters, and conflating the two is exactly the
            // mistake ruling 226 is written about from the other side.
            assert!(
                within(next(&mut framed), "end of framed stream")
                    .await
                    .is_none(),
                "`Framed` ends at the peer's clean EOF"
            );
            got
        };

        let ((), got) = tokio::join!(send_side, recv_side);

        assert_eq!(
            got.len(),
            want.len(),
            "S32: {} frames in, {} out",
            want.len(),
            got.len()
        );
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                g.len(),
                w.len(),
                "frame {i}: length differs — a coalescing build merges two \
                 frames, a splitting one halves them"
            );
            assert_eq!(
                &g[..],
                &w[..],
                "frame {i}: content differs. The index is mixed into every \
                 byte, so this is a reorder, not merely a corruption"
            );
        }
    })
    .await;
}

/// **S32, acceptance 3 — a consumer that polls once finds that exactly one
/// item was claimed (§16.11, §10.6, ruling 58).**
///
/// > a consumer that polls **once** finds that **exactly one** item was
/// > claimed … it is the single easiest way to get this layer wrong, because
/// > a read-ahead task looks like an ergonomic convenience and is in fact the
/// > unbounded shell queue §10.6 forbids.
///
/// # The assertion is the drop, not the poll
///
/// "One poll returned one item" is satisfied by a prefetching adapter for
/// free — it returns one item too, having claimed three. Working rule 9: ask
/// what the broken build does *differently*. It **holds the other two inside
/// itself**, so dropping the adapter destroys them.
///
/// So: poll once, drop the adapter, and require the connection to still hand
/// over both remaining messages through `recv_message()`. A read-ahead
/// adapter fails here by hanging (`within` panics) or by returning the wrong
/// message; a correct one loses nothing, because it claimed nothing it was
/// not asked for.
///
/// Ordering is deliberately **not** asserted — that is S16's story, not S32's.
/// What is asserted is that all three payloads are accounted for exactly
/// once, which is what "claimed at most one" actually means.
///
/// # BROKEN BUILD
///
/// * **An adapter with a `VecDeque` receive queue** — `src/shell/shared.rs`'s
///   own rustdoc names this one: *"An implementer who reaches for a
///   `VecDeque` has rebuilt the unbounded intermediate queue §10.6
///   forbids."* Caught: two messages vanish with the adapter.
/// * **An adapter that spawns a drainer task** — same signature, and it also
///   keeps claiming after the consumer stopped polling.
/// * **An adapter whose `poll_next` loops over the underlying `poll_*` until
///   `Pending`** — the "claim while we're here" shape. Caught identically.
/// * **An adapter that claims one item eagerly at construction** — caught by
///   the accounting, because the eagerly claimed item is lost on drop too.
#[tokio::test(start_paused = true)]
async fn s32_a_consumer_that_polls_once_claims_exactly_one_item() {
    local(async {
        let pair = Pair::seeded(0x5320_0002);
        let (ca, cb) = pair.establish().await;

        let sent: Vec<Vec<u8>> = (0..3usize).map(|i| frame(i, 64 + i).to_vec()).collect();
        for (i, m) in sent.iter().enumerate() {
            within(cb.send_message(m), "send_message")
                .await
                .unwrap_or_else(|e| panic!("message {i}: {e:?}"));
        }
        settle().await;

        // All three are available at the receiver before the single poll, so
        // a prefetching adapter has every opportunity to take them.
        let mut claimed: Vec<Vec<u8>> = Vec::new();
        {
            let mut msgs = ca.messages();
            match poll_next_once(&mut msgs) {
                Poll::Ready(Some(Ok(m))) => claimed.push(m),
                other => panic!(
                    "one poll of `messages()` with three messages waiting must \
                     yield exactly one item, got {other:?}"
                ),
            }
            // ── the line the test turns on ─────────────────────────────
            //
            // Everything this adapter claimed beyond the one item it
            // returned dies here.
        }

        // The other two must still be the connection's to give.
        for i in 0..2 {
            let m = within(
                ca.recv_message(),
                "recv_message after the adapter was dropped",
            )
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "message {} is gone: {e:?}. Ruling 58 — the adapter \
                         claimed ahead of its consumer and took it into an \
                         intermediate queue that died with the adapter",
                    i + 1
                )
            });
            claimed.push(m);
        }

        assert_eq!(
            claimed.len(),
            3,
            "three messages were sent and three must be recoverable"
        );
        let mut got = claimed.clone();
        let mut want = sent.clone();
        got.sort();
        want.sort();
        assert_eq!(
            got, want,
            "every message is accounted for exactly once — no duplicate \
             delivery (an adapter that claimed and also left the item) and no \
             loss (an adapter that claimed and dropped it)"
        );
    })
    .await;
}

/// **S32, acceptance 2 — `Sink` backpressure maps onto flow-control credit
/// rather than onto an intermediate buffer.**
///
/// > `Stream`/`Sink` backpressure maps onto flow-control credit rather than
/// > onto an intermediate buffer.
///
/// Both halves are asserted, and the first is the one that matters:
///
/// 1. **It stalls.** A build with a buffer of its own accepts everything
///    offered and never parks — so the assertion is `assert!(stalled)`, from
///    the side that separates, not `assert!(bytes <= bound)`, which a
///    collapsed build satisfies for free right up until it does not.
/// 2. **It stalls at roughly the credit, not somewhere else** — bounded by
///    `INITIAL_MAX_STREAM_DATA` plus generous slack for `Framed`'s own write
///    buffer and one in-flight frame. This is what makes "maps onto
///    flow-control credit" mean *that* credit.
/// 3. **It resumes when the reader drains.** A test that only proved the
///    stall would pass a build that deadlocks, which is a worse bug than the
///    one being tested for.
///
/// # BROKEN BUILD
///
/// * **Any `Vec`/`VecDeque` write buffer inside `compat/`** (`CONTRACT-8.md`
///   §9 item 1; the single-item `MessageSink` slot is the only buffer
///   permitted anywhere in the module). Caught by 1 and 2.
/// * **A `poll_write` that reports `Ok(n)` for bytes it did not place in send
///   state** — the same shape from the other end: credit is never consumed
///   and the sink never parks.
/// * **A build that parks the writer and never wakes it on a credit
///   re-grant.** Caught by 3 — and this is the failure mode a stall-only test
///   is blind to.
#[tokio::test(start_paused = true)]
async fn s32_sink_backpressure_maps_onto_flow_control_credit() {
    local(async {
        let pair = Pair::seeded(0x5320_0003);
        let (ca, cb) = pair.establish().await;

        const FRAME: usize = 8 * 1024;
        // Comfortably past `INITIAL_MAX_STREAM_DATA` (256 KiB) and under
        // `INITIAL_MAX_DATA` (1 MiB), so the *stream* window is unambiguously
        // the thing that binds.
        const OFFERED: usize = 96;

        let mut writer = within(ca.framed_bi(LengthDelimitedCodec::new()), "framed_bi")
            .await
            .expect("framed_bi");

        // One frame first: the peer learns of a stream from the first frame
        // carrying it, so `accept_framed_bi` cannot resolve before this.
        within(sink_send(&mut writer, frame(0, FRAME)), "first send")
            .await
            .expect("first send");
        settle().await;

        let mut reader = within(
            cb.accept_framed_bi(LengthDelimitedCodec::new()),
            "accept_framed_bi",
        )
        .await
        .expect("accept_framed_bi");

        // ── 1 + 2: fill until it parks ─────────────────────────────────
        //
        // The reader is deliberately not polled inside this loop.
        let mut accepted = 1usize;
        let mut stalled = false;
        for i in 1..OFFERED {
            match tokio::time::timeout(NOT_BEFORE, sink_send(&mut writer, frame(i, FRAME))).await {
                Ok(Ok(())) => accepted += 1,
                Ok(Err(e)) => panic!("send {i} failed with {e:?} rather than parking"),
                Err(_) => {
                    stalled = true;
                    break;
                }
            }
        }

        assert!(
            stalled,
            "S32: the sink accepted all {OFFERED} frames ({} bytes) without \
             ever parking. Flow control cannot admit that much unread data, \
             so this build is buffering inside `compat/` — the intermediate \
             buffer §10.6 forbids and `CONTRACT-8.md` §9 item 1 names",
            OFFERED * FRAME
        );

        let accepted_bytes = (accepted * FRAME) as u64;
        let bound = INITIAL_MAX_STREAM_DATA + 128 * 1024;
        assert!(
            accepted_bytes <= bound,
            "S32: {accepted_bytes} bytes were accepted against an \
             `INITIAL_MAX_STREAM_DATA` of {INITIAL_MAX_STREAM_DATA}. It parked \
             eventually, but not on *this* credit — something else is holding \
             the excess"
        );

        // ── 3: draining the reader releases the writer ─────────────────
        //
        // Fewer frames than the receiver can be holding, so this cannot
        // itself block: at most `INITIAL_MAX_STREAM_DATA` is buffered there,
        // which is 32 frames of this size.
        let to_drain = (accepted / 2).clamp(1, 16);
        for i in 0..to_drain {
            within(next(&mut reader), "drain")
                .await
                .unwrap_or_else(|| panic!("the framed stream ended while draining frame {i}"))
                .unwrap_or_else(|e| panic!("drain {i} failed: {e:?}"));
        }

        within(
            sink_send(&mut writer, frame(OFFERED, FRAME)),
            "send after the reader drained",
        )
        .await
        .expect(
            "the writer resumes once credit is re-granted — a build that parks \
             the writer without waking it on a re-grant deadlocks here",
        );
    })
    .await;
}

/// **S32 / `CONTRACT-8.md` §5 — `framed_bi` opens exactly one bi stream and
/// nothing else.**
///
/// > the constructors *"exist only to remove a `use`"*, and the result is
/// > *"identical to `Framed::new(conn.open_bi().await?, codec)`"*.
///
/// "Identical to" is a claim with a testable consequence: **one** stream, and
/// a *bi* one. A constructor that opened a stream per direction, or that
/// opened a uni stream alongside, would round-trip perfectly and still be
/// wrong — every such build passes
/// [`s32_framed_length_delimited_round_trips_objects_in_order`].
///
/// # BROKEN BUILD
///
/// * **A `framed_bi` that opens two streams** (one per direction, the
///   `FramedRead`/`FramedWrite` shape leaking into the `Framed` one) — the
///   second `accept_bi()` resolves instead of parking.
/// * **A `framed_bi` built over `open_uni` plus `accept_uni`** — caught by
///   the `accept_uni()` assertion, and it would also silently collide with
///   the message verb, which is S30's whole hazard.
/// * **A constructor that pre-writes a preamble** — nothing in §5 permits
///   one, and the peer's first decoded frame would not be frame 0.
#[tokio::test(start_paused = true)]
async fn s32_framed_bi_opens_exactly_one_bi_stream() {
    local(async {
        let pair = Pair::seeded(0x5320_0004);
        let (ca, cb) = pair.establish().await;

        let mut writer = within(ca.framed_bi(LengthDelimitedCodec::new()), "framed_bi")
            .await
            .expect("framed_bi");
        within(sink_send(&mut writer, frame(0, 128)), "send")
            .await
            .expect("send");
        settle().await;

        // Exactly one bi stream exists.
        let mut accepted = within(cb.accept_bi(), "accept_bi")
            .await
            .expect("accept_bi");

        assert!(
            tokio::time::timeout(NOT_BEFORE, cb.accept_bi())
                .await
                .is_err(),
            "`framed_bi` opens **one** bi stream (`CONTRACT-8.md` §5: \
             \"identical to `Framed::new(conn.open_bi().await?, codec)`\"). A \
             second one arriving means the constructor opened a stream per \
             direction"
        );
        assert!(
            tokio::time::timeout(NOT_BEFORE, cb.accept_uni())
                .await
                .is_err(),
            "`framed_bi` opens no uni stream. One appearing here would also \
             collide with the message verb, which is S30's hazard"
        );

        // And the one stream it opened really is the framed one: the first
        // frame is readable off it, prefix and all.
        let mut got = Vec::new();
        let mut buf = [0u8; 512];
        while got.len() < 4 + 128 {
            let n = within(
                tokio::io::AsyncReadExt::read(&mut accepted, &mut buf),
                "read the framed bytes",
            )
            .await
            .expect("read");
            assert!(n > 0, "the stream ended before the first frame was whole");
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(
            u32::from_be_bytes([got[0], got[1], got[2], got[3]]),
            128,
            "`LengthDelimitedCodec`'s default is a 4-byte big-endian length \
             prefix; a different value here means the constructor is not the \
             plain `Framed::new` §5 says it is"
        );
        assert_eq!(
            &got[4..4 + 128],
            &frame(0, 128)[..],
            "the payload behind the prefix is the frame that was sent"
        );
    })
    .await;
}

//! **Streams — S12, S13, S14, S17.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S12** *(maintainer's #5)* | open a stream, write, the peer reads the same bytes in the same order with no gaps or duplicates, `finish()` delivers the FIN, the reader observes end-of-stream. |
//! | **S13** | concurrent streams are independent — loss on one does not stall another — and stream ids carry the establishment parity fixed at S4. |
//! | **S14** | reset a stream; the peer surfaces `ReadError::Reset(code)`; siblings and the connection are unaffected. |
//! | **S17** | flow-control credit bounds unacknowledged data per stream *and* per connection; a slow reader stalls its own stream, not the connection. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 4b**, from `STORIES.md`
//! (236–286), `SPEC.md` §9/§10/§16.2/§16.9 and
//! `.slices/04-streams/CONTRACT-4b.md` **alone**, while the implementer
//! wrote `src/shell/` concurrently. No file under `src/shell/` and no line
//! of `src/testutil/mod.rs` was read; the harness names below were taken
//! from `.slices/00-ground/PLAN.md` §8.3 and from generated rustdoc.
//!
//! # Working rule 9 — every test names the build it separates
//!
//! *A bound is only a test if the degenerate case violates it.* Each test
//! carries a `BROKEN BUILD:` block naming a concrete implementation that
//! passes a weaker version of the test while being wrong. Where the plan
//! asked for an assertion that a **correct** build can fail, the assertion
//! is dropped and the reason is recorded in `.slices/04-streams/TESTS-4b.md`
//! §3 rather than written as a flaky red.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the observation instrument in both directions:
//! [`within`] asserts *it resolved*, [`is_pending`] asserts *it had not
//! resolved by then* — the "not before" half a one-sided test omits.
//! [`settle`] gives both drivers a turn without advancing virtual time.
//! There is no `sleep`.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use slither::constants::{INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA};
use slither::testutil::{FlakyPolicy, Pair, TestRecvStream, TestSendStream, local, settle};
use slither::{Dir, ReadError, WriteError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// `Pair` / `Peer` / `local` / `settle` are slice 0+3 harness and are used
// as shipped. The three stream handles and the four `Connection` verbs are
// 4b's and are called exactly as `CONTRACT-4b.md` §2/§3/§5 spells them —
// if a name below does not compile, the contract and the implementation
// disagree, and that is the finding, not a rename to be made here.
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve. Generous: on
/// the paused clock a resolvable future costs no wall time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Virtual-time budget for the **"not before"** half. Long enough for every
/// driver turn and wire delay these tests create, short enough that a
/// handful of them stay far inside `DEAD_TIMEOUT` (25 s) — a test that
/// spent 25 s of virtual time waiting to prove a stall would kill the
/// connection and pass for the wrong reason.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// `true` if `fut` had **not** resolved within [`NOT_BEFORE`]. Consumes the
/// future, which is exactly the cancel-safe drop the contract promises.
async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

/// Poll `fut` exactly once. The instrument for "**immediately**": a
/// `timeout` races the future, one poll observes it.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// A payload whose every byte is a function of its offset.
///
/// 251 is prime and coprime with every packet size in play, so a shift of
/// any length — a dropped overlap, a re-delivered duplicate, a
/// reassembler that trusts the second copy's offset — moves *every*
/// subsequent byte. A repeated-byte payload hides all three.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Compare without dumping a megabyte into the panic message.
///
/// Length first, content second: truncation and duplication are the two
/// failures that a single `assert_eq!` would report identically.
fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: byte count differs — a reassembler that drops a duplicated \
         frame's overlap is short, one that re-delivers it is long"
    );
    if let Some(i) = got.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: first differing byte at offset {i}: got {:#04x}, want {:#04x}",
            got[i], want[i]
        );
    }
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
///
/// Asserts the two `write` rows that a shell can get wrong silently
/// (`CONTRACT-4b.md` §3): `Ok(0)` means *only* that `buf` was empty, and a
/// write never claims more than it was handed.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: CONTRACT-4b §3 — `Ok(0)` means only that `buf` was empty; \
             a blocked write is `Pending`"
        );
        assert!(
            n <= buf.len() - done,
            "{what}: write claimed {n} bytes of a {}-byte buffer",
            buf.len() - done
        );
        done += n;
    }
}

/// Read until `Ok(None)`, failing on any error.
///
/// The `Err` arm's message is deliberately the name of the bug: a
/// `Drop for SendStream` that resets unconditionally turns the peer's
/// clean end-of-stream into `Reset(0)` and lands here.
async fn read_to_end(r: &mut TestRecvStream, what: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 4096];
    loop {
        match within(r.read(&mut buf), what).await {
            Ok(Some(n)) => {
                assert!(
                    n >= 1,
                    "{what}: CONTRACT-4b §5 — `Ok(Some(0))` means only that `buf` \
                     was empty; no data available is `Pending`"
                );
                assert!(n <= buf.len(), "{what}: read overran the caller's buffer");
                out.extend_from_slice(&buf[..n]);
            }
            Ok(None) => break,
            Err(e) => panic!(
                "{what}: expected a clean end of stream, got {e:?}. \
                 `Reset(0)` here is CONTRACT-4b §9's named bug: a `Drop` that \
                 resets a stream `finish()` already closed."
            ),
        }
    }
    out
}

/// Drain **exactly** `n` bytes, never one more.
///
/// The buffer is sized to the outstanding remainder on every call, so the
/// `read` contract's `n <= buf.len()` makes overshoot unrepresentable —
/// which is what lets the §10.3 re-grant boundary be tested at
/// `WINDOW/2 - 1` and `WINDOW/2` rather than "somewhere near half".
async fn drain_exactly(r: &mut TestRecvStream, n: usize, what: &str) {
    let mut done = 0usize;
    while done < n {
        let want = (n - done).min(8192);
        let mut buf = vec![0u8; want];
        match within(r.read(&mut buf), what).await {
            Ok(Some(k)) => {
                assert!(k >= 1 && k <= want, "{what}: read returned {k} for {want}");
                done += k;
            }
            other => panic!("{what}: wanted {n} bytes, got {other:?} after {done}"),
        }
    }
}

/// Fill `s` until its first `Pending`, reporting how many bytes were
/// accepted before the park.
///
/// Both halves matter and are returned together: *that* it parked, and
/// *where*. Asserting only the first passes a build that grants no credit;
/// asserting only the second passes a build that grants credit for ever.
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
                    "{what}: CONTRACT-4b §3 — a shell must report a blocked write as \
                     `Pending`, never as `Ok(0)`"
                );
                done += n;
            }
            Ok(Err(e)) => panic!("{what}: write failed with {e:?}"),
            Err(_) => return (done, true),
        }
    }
}

// ────────────────────────────── S12 ────────────────────────────────────

/// **S12 — a user can stream, over a path that reorders and duplicates.**
///
/// > Open a stream, write, the peer reads the same bytes in the same order
/// > with no gaps or duplicates, `finish()` delivers the FIN, the reader
/// > observes end-of-stream.
///
/// The name carries **`loss_free`** deliberately. Slice 4 has no
/// retransmission; slice 5 owns the loss half of S12. A name that does not
/// say so lets slice 5 ship with S12 looking discharged. Reordering and
/// duplication *are* in scope — they are handled by §9.5's overlap rule and
/// §7.2's replay window, both of which exist now — so this is materially
/// stronger than "lossless" suggests.
///
/// # BROKEN BUILD — what each assertion separates
///
/// * **A `Drop for SendStream` that resets unconditionally** (`CONTRACT-4b`
///   §9, the named most-likely bug of the slice). `SendHalf::reset`
///   early-returns only on an existing reset — a set `fin` does not stop
///   it — so "reset on drop, the core is idempotent" resets every
///   *finished* stream and destroys the FIN. **The reader here reads only
///   after `drop(send)`**; that ordering is the whole point, and reading
///   first is what lets this build ship. It fails in `read_to_end`'s `Err`
///   arm with `Reset(0)`.
/// * **A reassembler that mishandles a duplicated frame's overlap** — an
///   offset-shifted stream. Caught by the offset-derived payload, not by
///   the length.
/// * **A build that re-delivers duplicates** — caught by the length
///   assertion, which is made *before* the content assertion so the two
///   failures do not report identically.
/// * **A single-packet test.** `MAX_PLAINTEXT` is 1170, so 96 KiB is ~84
///   datagrams: enough that a uniform jitter draw permutes the whole burst
///   and §9.5 is actually exercised. `REPLAY_WINDOW` is 2048 packets and
///   `REASSEMBLY_CHUNKS_MAX` is 1024, so a full permutation of 84 is inside
///   both bounds by construction and cannot lose a packet on a path
///   configured for no loss.
/// * **A non-sticky end of stream.** The second `read()` after `Ok(None)`
///   must be `Ok(None)` again, not `Pending` (a reader that hangs on a
///   finished stream) and not `Err`.
///
/// The payload is 96 KiB, below `INITIAL_MAX_STREAM_DATA` (262 144), so
/// nothing here stalls on credit: S12 is about reassembly, S17 is about
/// flow control, and a test that mixes them cannot say which failed.
#[tokio::test(start_paused = true)]
async fn s12_loss_free_a_user_can_stream_over_a_reordering_duplicating_path() {
    local(async {
        let pair = Pair::seeded(0x5121_2012);
        let (ca, cb) = pair.establish().await;

        // Reorder and duplicate the **data** phase, with loss off.
        //
        // Installed after establishment on purpose: a handshake that has to
        // survive a duplicated msg1 is slice 2's story, and folding it in
        // here would make an S12 red ambiguous between two slices.
        // `jitter > 0` is where reordering lives (`.slices/00-ground/PLAN.md`
        // §8.3): two datagrams whose draws cross swap.
        let flaky = FlakyPolicy::perfect()
            .with_delay(Duration::from_millis(1), Duration::from_millis(4))
            .with_duplication(0.35);
        pair.a.wire.set_policy(flaky.clone());
        pair.b.wire.set_policy(flaky);

        const LEN: usize = 96 * 1024;
        let want = payload(LEN);

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "S12 bulk write").await;

        // Claim the stream before the FIN so the assertion below is about
        // the FIN and not about whether an unclaimed stream survives.
        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        // ── the line the slice turns on ────────────────────────────────
        //
        // Nothing has been read yet. A `Drop` that resets a finished stream
        // wipes `fresh`/`retransmit`/`unacked`/`buf` and pins `final_size`
        // at the highest byte transmitted; the reader below then sees
        // `Reset(0)` instead of the data and the FIN.
        drop(send);
        settle().await;

        let got = read_to_end(&mut recv, "S12 read to end").await;
        assert_same_bytes(&got, &want, "S12 stream contents");

        // End of stream is **sticky** (§16.2 / CONTRACT-4b §5).
        assert!(
            matches!(
                within(recv.read(&mut [0u8; 64]), "S12 second read").await,
                Ok(None)
            ),
            "§16.2: every read after the end of stream is `Ok(None)` — a \
             non-sticky build hangs a reader that loops until it sees it twice"
        );
    })
    .await;
}

// ────────────────────────────── S13 ────────────────────────────────────

/// **S13 — a stalled stream does not block a concurrent one.**
///
/// > Concurrent streams are independent; loss on one does not stall
/// > another.
///
/// Slice 4 has no loss recovery, so a datagram dropped here is never
/// retransmitted and stream A stalls **for ever**. That is the point: the
/// question is whether B is reachable across a permanent gap in A.
///
/// # Construction, and what it is coupled to
///
/// The gap is made with `Network::block_path`, not with a `drop_at` send
/// index: an index-based drop would have to account for every handshake
/// datagram already counted on that wire, and would move the moment an
/// unrelated send is added to the test.
///
/// **Coupling to flag.** Withholding *exactly* A's middle chunk works only
/// because ruling 114 makes a mutating call flush everything the ledger
/// admits — so two sequential `write()`s never share a packet, and the
/// blocked window contains chunk 2 and nothing else. **If ruling 114's
/// reading is ever revisited, this test's construction goes with it.**
///
/// # BROKEN BUILD
///
/// * **One reassembly buffer keyed by offset rather than by stream.** B's
///   bytes land in A's gap and B either never completes or completes
///   corrupted — the content assertion, not merely the arrival, is what
///   catches the corrupted case.
/// * **A fill loop that drains A before touching B.** B never delivers;
///   caught because B is read to `Ok(None)` *while* A's read future is
///   held.
/// * **A build that discards a stream on its first gap.** A's prefix must
///   still be delivered — and `settle()` runs before the drain, so chunk 3
///   has already arrived and the gap is already known when the prefix is
///   read. Without that ordering this assertion would pass such a build for
///   free.
/// * **A build that closes the connection on a gap.** Both `closed()`
///   futures are polled and must be `Pending`.
#[tokio::test(start_paused = true)]
async fn s13_a_stalled_stream_does_not_block_a_concurrent_one() {
    local(async {
        let pair = Pair::seeded(0x5133_0001);
        let (ca, cb) = pair.establish().await;

        // 3 × MAX_PLAINTEXT-ish: each chunk is several datagrams, so the
        // withheld window is a genuine multi-packet hole and not a
        // single-frame special case.
        let chunk1 = payload(3510);
        let chunk2 = payload(3510);
        let chunk3 = payload(3510);
        let chunk_b = payload(5000);

        let mut a1 = within(ca.open_uni(), "open A").await.expect("open A");
        write_all(&mut a1, &chunk1, "A chunk 1").await;
        settle().await;

        // The hole. One direction only: b → a keeps working, so nothing
        // here is a connection-wide outage.
        pair.net.block_path(pair.a.addr, pair.b.addr);
        write_all(&mut a1, &chunk2, "A chunk 2 (blackholed)").await;
        // **The driver must run before the path is healed.** Ruling 114 says
        // a mutating call *seals* everything the ledger admits — it does not
        // say the datagram has left. Sealing fills the core's output queue;
        // the I/O is the driver's, after `mark_dirty` wakes it (§16.3). With
        // no `settle()` here the blackhole window contains no driver pass at
        // all, every sealed datagram is sent *after* the heal, nothing is
        // lost, and this test reads a contiguous 10 530 bytes while claiming
        // to have made a permanent hole.
        settle().await;
        pair.net.heal_path(pair.a.addr, pair.b.addr);

        // Lands beyond the hole: buffered by the reassembler, undeliverable
        // for ever, because slice 4 never retransmits chunk 2.
        write_all(&mut a1, &chunk3, "A chunk 3 (past the gap)").await;

        let mut a2 = within(ca.open_uni(), "open B").await.expect("open B");
        write_all(&mut a2, &chunk_b, "B payload").await;
        within(a2.finish(), "B finish").await.expect("B finish");
        settle().await;

        // §16.2 / ruling 112: `accept_*` yields peer-opened streams in open
        // order. Asserting the ids rather than trusting the order makes a
        // FIFO violation a named failure instead of a confusing content
        // mismatch further down.
        let mut r1 = within(cb.accept_uni(), "accept A").await.expect("accept A");
        let mut r2 = within(cb.accept_uni(), "accept B").await.expect("accept B");
        assert_eq!(
            r1.id(),
            a1.id(),
            "ruling 112: the first `accept_uni` yields the first stream opened"
        );
        assert_eq!(
            r2.id(),
            a2.id(),
            "ruling 112: the second `accept_uni` yields the second stream opened"
        );
        assert_ne!(r1.id(), r2.id(), "two streams, two ids");

        // A delivers its prefix — the gap is already known by now.
        let mut a_got = Vec::new();
        let mut buf = vec![0u8; 1024];
        while a_got.len() < chunk1.len() {
            match within(r1.read(&mut buf), "A prefix").await {
                Ok(Some(n)) => a_got.extend_from_slice(&buf[..n]),
                other => panic!("§9.5: A's prefix must be readable across the gap, got {other:?}"),
            }
        }
        assert_same_bytes(&a_got, &chunk1, "A's prefix before the gap");

        // A is now parked at the hole. Hold the future across B's traffic.
        let mut stalled_buf = [0u8; 1024];
        let mut stalled = pin!(r1.read(&mut stalled_buf));
        assert!(
            poll_once(stalled.as_mut()).await.is_pending(),
            "§9.5: chunk 3 sits past a permanent hole and must not be delivered \
             out of order"
        );

        // ── the head-of-line question, asked with A held open ───────────
        let b_got = read_to_end(&mut r2, "B while A is stalled").await;
        assert_same_bytes(&b_got, &chunk_b, "B's contents while A is stalled");

        assert!(
            poll_once(stalled.as_mut()).await.is_pending(),
            "A must still be stalled after B completed — if A resolved here, \
             the two streams share reassembly state"
        );
        // `stalled` is left to fall out of scope: `drop`ping a
        // `Pin<&mut _>` releases nothing (the `pin!` temporary outlives it)
        // and trips `clippy::drop_non_drop`.

        // The connection itself is untouched by a permanently stalled stream.
        let mut ca_closed = pin!(ca.closed());
        let mut cb_closed = pin!(cb.closed());
        assert!(
            poll_once(ca_closed.as_mut()).await.is_pending(),
            "§9: a gap in one stream is not a connection error"
        );
        assert!(
            poll_once(cb_closed.as_mut()).await.is_pending(),
            "§9: a gap in one stream is not a connection error"
        );
    })
    .await;
}

/// **S13 — stream ids carry the establishment parity fixed at S4.**
///
/// §9.1: bit 0 is the opener (0 = connection initiator), bit 1 is the
/// direction (0 = bidirectional). `Connection` has **no `role()`
/// accessor** and §16.2 does not give it one, so the claim is phrased
/// through `StreamId::initiated_by_connection_initiator()`, which is
/// public and is a property of the id rather than of the asker.
///
/// # BROKEN BUILD
///
/// * **One shared id counter for both ends.** A test that asserted only
///   "the two sides' ids differ" passes that build. Both parities are
///   asserted, with **opposite** values, on the **same connection in the
///   same test** — which a shared counter cannot satisfy.
/// * **A build that ignores the direction bit.** Every id is checked for
///   `dir()` *and* for the raw `0x02` bit, on both sides, for both
///   directions — four combinations, so a build that hard-codes either
///   value fails.
/// * **A build that computes the parity from the local role rather than
///   from the id.** The peer's view of the *same* stream is asserted to
///   give the *same* answer. Ruling 101's doc says so in words ("not
///   whether *we* opened it"); this is the assertion that holds it.
/// * **An id that changes across `split()`.** `BiStream::id()` and both
///   halves' `id()` are asserted equal.
#[tokio::test(start_paused = true)]
async fn s13_stream_ids_carry_establishment_parity() {
    local(async {
        let pair = Pair::seeded(0x5133_0002);
        // `Pair::establish` dials a → b, so `ca` is the connection
        // initiator and `cb` the acceptor.
        let (ca, cb) = pair.establish().await;

        // ── initiator-opened, unidirectional ───────────────────────────
        let mut a_uni = within(ca.open_uni(), "a.open_uni").await.expect("open_uni");
        write_all(&mut a_uni, b"x", "a_uni probe").await;
        let a_uni_id = a_uni
            .id()
            .expect("ruling 116: a held handle's id() is always Some");
        assert!(
            a_uni_id.initiated_by_connection_initiator(),
            "§9.1: the connection initiator opened this stream"
        );
        assert_eq!(
            a_uni_id.as_u64() & 0x01,
            0,
            "§9.1: opener bit 0 = initiator"
        );
        assert_eq!(a_uni_id.dir(), Dir::Uni);
        assert_eq!(
            a_uni_id.as_u64() & 0x02,
            0x02,
            "§9.1: direction bit 1 = uni"
        );

        let r_uni = within(cb.accept_uni(), "b.accept_uni")
            .await
            .expect("accept_uni");
        assert_eq!(r_uni.id(), Some(a_uni_id), "both ends name one stream");
        assert!(
            r_uni
                .id()
                .expect("ruling 116")
                .initiated_by_connection_initiator(),
            "§9.1: the answer is a property of the id and is the same on both \
             ends — a build that answers from the local role inverts here"
        );

        // ── acceptor-opened, unidirectional ────────────────────────────
        let mut b_uni = within(cb.open_uni(), "b.open_uni").await.expect("open_uni");
        write_all(&mut b_uni, b"y", "b_uni probe").await;
        let b_uni_id = b_uni.id().expect("ruling 116");
        assert!(
            !b_uni_id.initiated_by_connection_initiator(),
            "§9.1: the acceptor opened this one — the opposite value, on the \
             same connection, is what a shared counter cannot produce"
        );
        assert_eq!(b_uni_id.as_u64() & 0x01, 1, "§9.1: opener bit 1 = acceptor");
        assert_eq!(b_uni_id.dir(), Dir::Uni);
        assert_eq!(b_uni_id.as_u64() & 0x02, 0x02);

        let ra_uni = within(ca.accept_uni(), "a.accept_uni")
            .await
            .expect("accept_uni");
        assert_eq!(ra_uni.id(), Some(b_uni_id));
        assert!(
            !ra_uni
                .id()
                .expect("ruling 116")
                .initiated_by_connection_initiator(),
            "§9.1: same id, same answer, seen from the initiator's end"
        );

        // ── initiator-opened, bidirectional ────────────────────────────
        let a_bi = within(ca.open_bi(), "a.open_bi").await.expect("open_bi");
        let a_bi_id = a_bi.id().expect("ruling 116");
        assert!(a_bi_id.initiated_by_connection_initiator());
        assert_eq!(a_bi_id.as_u64() & 0x01, 0);
        assert_eq!(a_bi_id.dir(), Dir::Bi);
        assert_eq!(a_bi_id.as_u64() & 0x02, 0, "§9.1: direction bit 0 = bidi");

        let (mut a_bi_send, a_bi_recv) = a_bi.split();
        assert_eq!(
            a_bi_send.id(),
            Some(a_bi_id),
            "ruling 120: `split()` does not renumber the stream"
        );
        assert_eq!(a_bi_recv.id(), Some(a_bi_id));
        write_all(&mut a_bi_send, b"z", "a_bi probe").await;

        let b_bi = within(cb.accept_bi(), "b.accept_bi")
            .await
            .expect("accept_bi");
        assert_eq!(b_bi.id(), Some(a_bi_id));

        // ── acceptor-opened, bidirectional ─────────────────────────────
        //
        // One bidi stream per side, far below `INITIAL_MAX_STREAMS_BIDI`
        // (32): CONTRACT-4b §2 forbids awaiting `open_bi` on an exhausted
        // bidi space, which parks for ever until slice 5 lands ACKs.
        let b_bi_own = within(cb.open_bi(), "b.open_bi").await.expect("open_bi");
        let b_bi_id = b_bi_own.id().expect("ruling 116");
        assert!(
            !b_bi_id.initiated_by_connection_initiator(),
            "§9.1: bidi parity is the acceptor's here"
        );
        assert_eq!(b_bi_id.as_u64() & 0x01, 1);
        assert_eq!(b_bi_id.dir(), Dir::Bi);
        assert_eq!(b_bi_id.as_u64() & 0x02, 0);

        let (mut b_bi_send, _b_bi_recv) = b_bi_own.split();
        write_all(&mut b_bi_send, b"w", "b_bi probe").await;
        let a_bi_peer = within(ca.accept_bi(), "a.accept_bi")
            .await
            .expect("accept_bi");
        assert_eq!(a_bi_peer.id(), Some(b_bi_id));

        // All four ids are distinct: the two parity bits and the two
        // direction bits partition the space, and a build that collapses
        // any pair of spaces collides here.
        let all = [a_uni_id, b_uni_id, a_bi_id, b_bi_id];
        for (i, x) in all.iter().enumerate() {
            for y in &all[i + 1..] {
                assert_ne!(x, y, "§9.1: the four stream spaces are disjoint");
            }
        }
    })
    .await;
}

// ────────────────────────────── S14 ────────────────────────────────────

/// **S14 — a user can abandon a stream without killing the connection.**
///
/// > Reset a stream; the peer surfaces `ReadError::Reset(code)`; other
/// > streams and the connection are unaffected.
///
/// # BROKEN BUILD
///
/// * **A shell that surfaces "some reset".** The code asserted is `0x2a`,
///   not `0`. Code `0` is §16.2's *drop* default, so a test written with
///   `0` cannot tell an explicit `reset(code)` from a dropped handle —
///   and both paths are exercised here, with different codes, so the two
///   cannot be confused.
/// * **An unlatched shell** (`CONTRACT-4b` §5, ruling 121). The core is
///   *not* sticky: after reporting the reset once it returns `Ok(None)` on
///   the retry. An application that logs the reset and reads again would
///   then see a clean end of stream for a stream whose data was
///   abandoned — §9.6's data reported as a complete transfer. The second
///   `read()` is the only thing that catches it.
/// * **A reset that tears down the connection.** A sibling stream opened
///   *after* the reset completes a full write/finish/read round trip, and
///   both `closed()` futures are still `Pending`.
/// * **A `Drop` that does nothing.** The dropped, unfinished handle must
///   produce exactly `Reset(0)` at the peer.
#[tokio::test(start_paused = true)]
async fn s14_a_reset_stream_leaves_the_connection_and_its_siblings_alive() {
    local(async {
        let pair = Pair::seeded(0x5140_0001);
        let (ca, cb) = pair.establish().await;

        const CODE: u64 = 0x2a;

        let mut victim = within(ca.open_uni(), "open victim")
            .await
            .expect("open victim");
        write_all(&mut victim, &payload(4096), "victim data").await;
        settle().await;

        let mut r_victim = within(cb.accept_uni(), "accept victim")
            .await
            .expect("accept victim");

        victim.reset(CODE);
        settle().await;

        // §9.6: the peer surfaces the reset. Any bytes still readable are
        // drained first so the assertion is "the stream ends as a reset",
        // not "the reset raced the data".
        let observed = loop {
            match within(r_victim.read(&mut [0u8; 1024]), "victim read").await {
                Ok(Some(_)) => continue,
                Ok(None) => panic!(
                    "§9.6: a reset stream must not end as a clean end-of-stream — \
                     abandoned data would be reported as a complete transfer"
                ),
                Err(e) => break e,
            }
        };
        assert_eq!(
            observed,
            ReadError::Reset(CODE),
            "§9.6: the peer surfaces the exact code the application sent"
        );

        // Ruling 121's latch. The core answers `Ok(None)` here.
        assert_eq!(
            within(r_victim.read(&mut [0u8; 1024]), "victim second read").await,
            Err(ReadError::Reset(CODE)),
            "ruling 121: the reset is sticky at the handle — an unlatched shell \
             returns `Ok(None)` on the retry and reports abandoned data as a \
             complete transfer"
        );

        // §16.2: after `reset()`, this handle's write verbs are closed.
        assert_eq!(
            within(victim.write(b"more"), "write after reset").await,
            Err(WriteError::Finished),
            "CONTRACT-4b §3: a reset handle reports `Finished`, not `Reset` — \
             `WriteError::Reset` has no producer in slice 4"
        );
        assert_eq!(
            within(victim.finish(), "finish after reset").await,
            Err(WriteError::Finished)
        );

        // ── a sibling, opened after the reset, is untouched ─────────────
        let sibling_bytes = payload(9000);
        let mut sibling = within(ca.open_uni(), "open sibling")
            .await
            .expect("open sibling");
        write_all(&mut sibling, &sibling_bytes, "sibling data").await;
        within(sibling.finish(), "sibling finish")
            .await
            .expect("finish");
        settle().await;
        let mut r_sibling = within(cb.accept_uni(), "accept sibling")
            .await
            .expect("accept sibling");
        let got = read_to_end(&mut r_sibling, "sibling read").await;
        assert_same_bytes(&got, &sibling_bytes, "the sibling's contents after a reset");

        // ── §16.2's drop default is a *different* code ──────────────────
        let mut dropped = within(ca.open_uni(), "open dropped")
            .await
            .expect("open dropped");
        write_all(&mut dropped, b"abandoned", "dropped data").await;
        settle().await;
        let mut r_dropped = within(cb.accept_uni(), "accept dropped")
            .await
            .expect("accept dropped");
        drop(dropped);
        settle().await;

        let observed = loop {
            match within(r_dropped.read(&mut [0u8; 64]), "dropped read").await {
                Ok(Some(_)) => continue,
                Ok(None) => panic!(
                    "§16.2: dropping a `SendStream` **without** `finish()` resets it — \
                     a `Drop` that does nothing ends the stream cleanly instead"
                ),
                Err(e) => break e,
            }
        };
        assert_eq!(
            observed,
            ReadError::Reset(0),
            "§16.2: the drop default is code 0, and it is distinct from the \
             {CODE:#x} an application sends explicitly"
        );

        // ── neither reset touched the connection ───────────────────────
        let mut ca_closed = pin!(ca.closed());
        let mut cb_closed = pin!(cb.closed());
        assert!(
            poll_once(ca_closed.as_mut()).await.is_pending(),
            "§18.1: a stream reset is not a connection error"
        );
        assert!(
            poll_once(cb_closed.as_mut()).await.is_pending(),
            "§18.1: a stream reset is not a connection error"
        );
    })
    .await;
}

// ────────────────────────────── S17 ────────────────────────────────────

/// **S17 — a slow reader stalls its own stream only.**
///
/// > Flow-control credit bounds unacknowledged data per stream and per
/// > connection; a reader that stops reading stalls its own stream, not
/// > the connection.
///
/// # BROKEN BUILD
///
/// Three builds are in play, and no single assertion separates all three:
///
/// * **Build A — grants nothing.** The first write parks. Caught by
///   `fill_until_blocked` reporting a byte count *below* the window.
/// * **Build B — grants unconditionally.** Nothing ever parks. Caught by
///   the `blocked` flag; a test that asserted only the byte count would
///   pass it, because build B accepts everything it is offered including
///   the first 262 144 bytes.
/// * **Build C — one ledger, both levels collapsed.** Caught **twice, from
///   opposite sides**, which is the only way to pin it: a sibling stream
///   still accepts data while stream 1 is stalled (a stream-sized shared
///   ledger fails here), *and* a fifth stream with a full, untouched
///   stream window of its own is parked by the exhausted **connection**
///   window (a connection-sized shared ledger fails here).
///
/// The byte counts are asserted with `assert_eq!`, not `<=`. An upper
/// bound is satisfied for free by build A, which is exactly working rule
/// 9's failure.
#[tokio::test(start_paused = true)]
async fn s17_a_slow_reader_stalls_its_own_stream_only() {
    local(async {
        let pair = Pair::seeded(0x5170_0001);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        // Deliberately larger than the window: the park has to be observed
        // where the *implementation* puts it, not where the test stops
        // offering bytes.
        let over = payload(window + 16 * 1024);

        let mut s1 = within(ca.open_uni(), "open s1").await.expect("open s1");
        let (accepted, blocked) = fill_until_blocked(&mut s1, &over, "s1 fill").await;

        assert!(
            blocked,
            "§10.1: with the reader never reading, the sender must park — \
             a build that grants credit unconditionally never does"
        );
        assert_eq!(
            accepted, window,
            "§10.2: the park comes at exactly `INITIAL_MAX_STREAM_DATA`. A \
             build with any other window still parks, so `blocked` alone \
             asserts nothing about *where*"
        );

        // ── a sibling stream is not stalled by stream 1 ─────────────────
        //
        // Connection window 1 048 576, of which 262 144 is spent: three
        // stream windows remain.
        let probe = payload(4096);
        let mut s2 = within(ca.open_uni(), "open s2").await.expect("open s2");
        let n = within(s2.write(&probe), "s2 first write")
            .await
            .expect("s2 write");
        assert!(
            n >= 1,
            "§10.1: stream credit is per stream — a sibling must not inherit \
             stream 1's stall"
        );

        // ── fill the connection window, then prove it is the binding level ─
        // The handles are held, not dropped: dropping a `SendStream`
        // without `finish()` resets it, and §10.3's retirement true-up
        // would hand the credit straight back — collapsing the very
        // exhaustion this test is building.
        let mut held = vec![s2];
        let mut total = accepted + n;
        while total < INITIAL_MAX_DATA as usize {
            let mut s = within(ca.open_uni(), "open filler")
                .await
                .expect("open filler");
            let (got, _) = fill_until_blocked(&mut s, &over, "filler fill").await;
            assert!(
                got >= 1,
                "§10.1: {total} of {INITIAL_MAX_DATA} connection bytes are spent, \
                 so a fresh stream must accept at least one byte"
            );
            total += got;
            held.push(s);
            assert!(
                held.len() < 16,
                "the connection window should be reached in a handful of streams"
            );
        }
        // The last filler stops at the connection window, not at its own.
        assert_eq!(
            total, INITIAL_MAX_DATA as usize,
            "§10.1: the connection window bounds the sum over streams, and the \
             sum lands on it exactly"
        );

        // A *fresh* stream: its own stream window is untouched and full, so
        // only the connection window can park its very first byte.
        let mut fresh = within(ca.open_uni(), "open fresh")
            .await
            .expect("open fresh");
        settle().await;
        assert!(
            is_pending(fresh.write(b"one byte")).await,
            "§10.1: with the connection window spent, a stream that has never \
             written a byte is parked by the connection level — the assertion \
             build C fails from the side the sibling probe cannot reach"
        );

        // ── and the connection itself is alive throughout ───────────────
        let mut ca_closed = pin!(ca.closed());
        assert!(
            poll_once(ca_closed.as_mut()).await.is_pending(),
            "§10: backpressure is not a connection error — the sender learns, \
             it does not die"
        );
        let mut cb_closed = pin!(cb.closed());
        assert!(poll_once(cb_closed.as_mut()).await.is_pending());
    })
    .await;
}

/// **S17 — the sender resumes when the reader drains.**
///
/// §10.3's re-grant rule, observed from the only place `tests/` can see
/// it. With `WINDOW = INITIAL_MAX_STREAM_DATA`, the prospective limit is
/// `bytes_read + WINDOW` and a MAX_STREAM_DATA is emitted when
/// `prospective − last_advertised ≥ WINDOW/2` — that is, at
/// `bytes_read == WINDOW/2` and not one byte earlier.
///
/// `PLAN.md` §11.4 asks for this as a **frame count** (no MAX_STREAM_DATA
/// at `WINDOW/2 − 1`, exactly one at `WINDOW/2`). `Tap::datagrams()`
/// yields AEAD ciphertext after establishment, so no integration test can
/// count frames; that assertion belongs to 4a's in-crate file (ruling
/// 123). What follows is the behavioural substitute, and it is
/// **two-sided** — which is what makes it a pin rather than a name.
///
/// # BROKEN BUILD
///
/// * **Grants on every read** (no threshold): the sender would already be
///   unblocked at `WINDOW/2 − 1`. Caught by the first `is_pending`.
/// * **Grants nothing**: the sender never resumes. Caught by the write
///   that must complete at `WINDOW/2`.
/// * **Seeds `last_advertised` at 0 rather than at `INITIAL_MAX_STREAM_DATA`**:
///   re-grants far too early, and fails the same first assertion.
/// * **Drops the ceiling on resume** (re-grants an unbounded window):
///   caught by the exact byte count accepted after the resume —
///   `bytes_read + WINDOW − bytes_sent` is `WINDOW/2`, and it is asserted
///   with `assert_eq!`.
#[tokio::test(start_paused = true)]
async fn s17_the_sender_resumes_when_the_reader_drains() {
    local(async {
        let pair = Pair::seeded(0x5170_0002);
        let (ca, cb) = pair.establish().await;

        let window = INITIAL_MAX_STREAM_DATA as usize;
        let half = window / 2;
        let over = payload(window + 16 * 1024);

        let mut s = within(ca.open_uni(), "open").await.expect("open");
        let (accepted, blocked) = fill_until_blocked(&mut s, &over, "initial fill").await;
        assert!(blocked, "§10.1: the reader has read nothing");
        assert_eq!(accepted, window, "§10.2: parked at the stream window");

        let mut r = within(cb.accept_uni(), "accept").await.expect("accept");

        // One byte short of the threshold.
        drain_exactly(&mut r, half - 1, "drain to WINDOW/2 - 1").await;
        settle().await;
        assert!(
            is_pending(s.write(&over[..4096])).await,
            "§10.3: `prospective − last_advertised` is {} at this point, one \
             short of WINDOW/2 ({half}) — no credit is owed yet",
            half - 1
        );

        // The byte that crosses it.
        drain_exactly(&mut r, 1, "drain the threshold byte").await;
        settle().await;

        let (resumed, blocked_again) =
            fill_until_blocked(&mut s, &over[..window], "post-regrant fill").await;
        assert!(
            blocked_again,
            "§10.3: the re-grant moves the ceiling, it does not remove it"
        );
        assert_eq!(
            resumed,
            half,
            "§10.3: the new limit is `bytes_read + WINDOW` = {} and {window} \
             bytes are already sent, so exactly WINDOW/2 more fit — a build \
             that drops the ceiling on resume accepts everything offered",
            half + window
        );

        // The reader still gets every byte it was sent, in order: a resume
        // that duplicated or dropped the boundary write would show here.
        //
        // The two fills each start at their own buffer's offset 0, so the
        // byte stream on the wire is `over[..window]` followed by
        // `over[..half]` — spelled out rather than recomputed, because
        // guessing it wrong makes a *passing* build look broken.
        let mut want = Vec::with_capacity(window + half);
        want.extend_from_slice(&over[..window]);
        want.extend_from_slice(&over[..half]);

        within(s.finish(), "finish").await.expect("finish");
        settle().await;
        drop(s);
        settle().await;

        // `half` bytes were already drained above, in two calls.
        let mut got = want[..half].to_vec();
        got.extend_from_slice(&read_to_end(&mut r, "drain the rest").await);
        assert_same_bytes(&got, &want, "everything the sender was allowed to send");
    })
    .await;
}

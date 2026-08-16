//! **Reliability — S12 (the loss half) and S28.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S12** *(maintainer's #5)* | open a stream, write, the peer reads the same bytes in the same order with no gaps or duplicates, `finish()` delivers the FIN, the reader observes end-of-stream. **Survives loss, reordering and duplication on the path.** |
//! | **S28** | the application can await **transport-level acknowledgement** of what it sent, then `close()` without loss. |
//!
//! Slice 4b closed only S12's loss-free half, and its test says so in its
//! own name (`s12_loss_free_…`). This file closes the other half: every
//! test here injects loss, and the loss is **proved to have happened**.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for slice 5b**, from `STORIES.md`
//! (S12 at 240–247, S28 at 438–453), `SPEC.md` §12/§13/§14/§16.2 and
//! `.slices/05-reliability/CONTRACT-5b.md` **alone**, while a second agent
//! wrote `src/shell/` and `src/core/` concurrently and blind to this file.
//! No line of the 5b implementation was read. The harness names are slice
//! 0+3's, taken from `tests/story_streams.rs` and from
//! `src/testutil/mod.rs`'s public surface; the two `acked()` verbs are
//! spelled exactly as `CONTRACT-5b.md` §2.5 spells them — **if a name
//! below does not compile, the contract and the implementation disagree,
//! and that is the finding, not a rename to be made here.**
//!
//! # Working rule 9 — the loss must be *proved*, not merely configured
//!
//! *A bound is only a test if the degenerate case violates it.* "Every
//! byte arrived" passes a build with no loss injected at all, so a loss
//! test that cannot show a datagram died proves nothing. `FlakyWire`
//! offers exactly two instruments that can show it, and every test here
//! uses one of them:
//!
//! * **[`Network::block_path`]** — a blackhole. `send_to` returns *before*
//!   the tap, so a blocked send is counted by `Network::sends()` and is
//!   **absent** from the tap: `sends() − tap.len()` is a **counter-proved**
//!   drop count. [`blackholed`] reads it.
//! * **`FlakyPolicy::drop_at`** — *"Drop exactly these 0-based send
//!   indices. No RNG is involved."* If the wire reached the index, the
//!   drop happened; [`sent_from`] shows it reached it.
//!
//! `FlakyPolicy::lossy(rate)` is used for the soak dimension but **never**
//! as a test's only source of loss: the tap is written *above* the loss
//! draw, so a probabilistic drop is invisible to every counter the public
//! API exposes. See `.slices/05-reliability/TESTS-5b.md` §4.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! [`settle`] gives both drivers a turn and advances **no** virtual time.
//! `tokio::time::timeout` is the instrument that does: [`within`] /
//! [`recovering`] assert *it resolved*, [`is_pending`] asserts *it had not
//! resolved by then*. `tokio::time::advance` is used where a **timer**
//! must fire, which is the spelling `settle`'s own rustdoc asks for.
//! There is no `sleep`.
//!
//! [`Network::block_path`]: slither::testutil::Network::block_path

use std::cell::Cell;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use slither::constants::{DEAD_TIMEOUT, MAX_PLAINTEXT, NO_ERROR};
use slither::testutil::{FlakyPolicy, Pair, Tap, TestRecvStream, TestSendStream, local, settle};
use slither::{ConnectionLost, WriteError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that must resolve **without** waiting
/// on recovery. On the paused clock a resolvable future costs no wall time.
const PATIENCE: Duration = Duration::from_secs(5);

/// Virtual-time budget for something that must resolve **through** loss
/// recovery.
///
/// A PTO with no RTT sample yet is `K_INITIAL_RTT + 4·rttvar +
/// MAX_ACK_DELAY` = 333 + 666 + 25 ≈ 1 024 ms, and §13.3 doubles it per
/// unanswered probe, so three probes cost ≈ 7 s of virtual time. Twelve
/// seconds covers that; it is deliberately **under `DEAD_TIMEOUT`** (25 s),
/// because a budget past the death would let a test that meant to observe
/// recovery observe a corpse instead, and report the wrong failure.
const RECOVERY_PATIENCE: Duration = Duration::from_secs(12);

/// Virtual-time budget for the **"not before"** half.
///
/// Long enough for every driver turn and wire delay these tests create,
/// short enough to stay far inside `DEAD_TIMEOUT`.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut` on `budget`, failing loudly instead of hanging the suite.
async fn within_for<F: Future>(fut: F, what: &str, budget: Duration) -> F::Output {
    match tokio::time::timeout(budget, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {budget:?} of virtual time"),
    }
}

/// [`within_for`] on [`PATIENCE`] — for what must not need recovery.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    within_for(fut, what, PATIENCE).await
}

/// [`within_for`] on [`RECOVERY_PATIENCE`] — for what must survive loss.
async fn recovering<F: Future>(fut: F, what: &str) -> F::Output {
    within_for(fut, what, RECOVERY_PATIENCE).await
}

/// `true` if `fut` had **not** resolved within [`NOT_BEFORE`]. Consumes the
/// future, which is exactly the cancel-safe drop the contract promises.
async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

/// Poll `fut` exactly once. The instrument for "**immediately**".
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// A payload whose every byte is a function of its offset.
///
/// 251 is prime and coprime with every packet size in play, so a shift of
/// any length — a retransmission applied at the wrong offset, a
/// re-delivered duplicate, a reassembler that trusts the second copy —
/// moves *every* subsequent byte. A repeated-byte payload hides all three,
/// and under retransmission all three are live.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Compare without dumping a megabyte into the panic message.
///
/// Length first, content second: truncation and duplication are the two
/// failures a single `assert_eq!` would report identically, and a
/// retransmitting build can produce either.
fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: byte count differs — a receiver that appends a retransmitted \
         range instead of ignoring the overlap is long; one that drops the \
         range it already had part of is short"
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
/// The tap sits **below** the send-failure and blackhole checks and
/// **above** the loss draw (`testutil/mod.rs` `send_to`, steps 3→5), so
/// with neither a partition nor an injected failure active on `who`, this
/// is exactly that wire's 0-based send index — the number `drop_at`
/// indices are counted in. It says nothing about what *arrived*.
fn sent_from(tap: &Tap, who: SocketAddr) -> usize {
    tap.snapshot().iter().filter(|s| s.src == who).count()
}

/// Datagrams that were sent and **destroyed by the fabric**, cumulative.
///
/// `Network::sends()` counts every `send_to` before any policy decision;
/// the tap counts those that were neither send-failed nor blackholed. The
/// difference is therefore the blackhole count — a **counter-proved** loss,
/// and the only drop the public API can see (a `loss`-draw drop is tapped
/// like any other send). Nothing here injects a send failure, so every
/// unit of this number is a `block_path` casualty.
fn blackholed(pair: &Pair) -> usize {
    pair.net.sends() - pair.net.tap().len()
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = recovering(s.write(&buf[done..]), what)
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

/// Read until `Ok(None)`, on the recovery budget, failing on any error.
async fn read_to_end(r: &mut TestRecvStream, what: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 4096];
    loop {
        match recovering(r.read(&mut buf), what).await {
            Ok(Some(n)) => {
                assert!(
                    n >= 1,
                    "{what}: §16.2 — `Ok(Some(0))` means only that `buf` was empty; \
                     no data available is `Pending`"
                );
                out.extend_from_slice(&buf[..n]);
            }
            Ok(None) => break,
            Err(e) => panic!(
                "{what}: expected a clean end of stream, got {e:?}. A \
                 `ConnectionLost` here is **ruling 128's post-death drain**, not \
                 this story: data that arrived before the death stays readable, \
                 and this read happens after the sender closed."
            ),
        }
    }
    out
}

/// Install `policy` on `who`'s wire with a window of **guaranteed** drops.
///
/// `drop_at` indices count every send the wire has *ever* made, and a
/// wire that has just completed a handshake is already several indices in
/// — so a window is only meaningful when it is anchored at the wire's
/// **current** index. `skip` leaves that many sends untouched before the
/// window opens; `count` is the window's width.
///
/// Returns the highest index in the window, for the post-hoc assertion
/// that the wire actually reached it (`sent_from(&tap, who) > last`).
/// Without that assertion the drops are configured but not *shown*, which
/// is the working-rule-9 failure this whole file is built to avoid.
fn arm_drops(
    tap: &Tap,
    wire: &slither::testutil::SharedWire,
    who: SocketAddr,
    mut policy: FlakyPolicy,
    skip: usize,
    count: usize,
) -> usize {
    let base = sent_from(tap, who);
    // Field assignment, not a struct literal: `FlakyPolicy` has one
    // private field (`failing`), so `FlakyPolicy { .. }` and functional
    // update are both unavailable outside the crate — see TESTS-5b.md §3.
    policy.drop_at = (base + skip..base + skip + count).collect();
    wire.set_policy(policy);
    base + skip + count - 1
}

/// The fewest datagrams a payload of `len` bytes can possibly occupy.
///
/// `MAX_PLAINTEXT` is the whole plaintext, framing included, so this is a
/// strict lower bound on the packet count and never an equality.
fn min_packets(len: usize) -> usize {
    len.div_ceil(MAX_PLAINTEXT)
}

// ══════════════════════════════════════════════════════════════════════
// S12 — loss recovery: intact, in order, exactly once
// ══════════════════════════════════════════════════════════════════════

/// **S12 — a bulk stream survives loss, reordering and duplication.**
///
/// > The peer reads the same bytes in the same order with no gaps or
/// > duplicates, `finish()` delivers the FIN, the reader observes
/// > end-of-stream. **Survives loss, reordering and duplication.**
///
/// The one test that closes the story. Slice 4b's `s12_loss_free_…` runs
/// the same shape with the loss knob at zero, deliberately; this one turns
/// it on, on **both** directions of the path, and additionally destroys a
/// window of A's datagrams by index so that the recovery under test is not
/// a matter of the seed's mood.
///
/// # BROKEN BUILD — what each assertion separates
///
/// * **A build with no retransmission at all.** The six index-dropped
///   datagrams are gone for ever; the reader never reaches its EOF and
///   [`read_to_end`] panics out of [`recovering`] rather than hanging the
///   suite. This is the assertion the whole slice exists for.
/// * **A build that retransmits by packet rather than by frame** (§13.5:
///   *"Frames, never packets"*), or that re-seals a lost range at the
///   wrong offset: the payload is offset-derived, so the **content**
///   assertion fires even where the length is right.
/// * **A build that re-delivers a retransmitted range the receiver already
///   holds.** Caught by the length assertion, which is made *before* the
///   content assertion so the two failures do not report identically.
///   Duplication is set to 5 % as well, but note the two are different
///   organs: a duplicated *datagram* dies at §7.2's replay window, a
///   retransmitted *range* arrives under a fresh counter and reaches
///   §9.5's overlap rule. Only the latter is new in slice 5.
/// * **A build with no loss injected at all** — working rule 9's own
///   failure mode. Caught by the `sent_from` assertion below: unless A's
///   wire reached the last index in the drop window, the six drops did not
///   happen and this test proved nothing. A name is not a pin.
/// * **A non-sticky end of stream.** The second `read()` after `Ok(None)`
///   must be `Ok(None)` again.
///
/// # Sizing
///
/// 192 KiB is ~168 datagrams — enough that jitter permutes whole
/// congestion windows and enough that loss detection runs many times —
/// and it is comfortably **below** `INITIAL_MAX_STREAM_DATA` (262 144), so
/// nothing here stalls on credit. S17 owns flow control; a test that mixed
/// them could not say which failed.
#[tokio::test(start_paused = true)]
async fn s12_a_bulk_stream_survives_loss_reordering_and_duplication() {
    local(async {
        let pair = Pair::seeded(0x5121_0501);
        let (ca, cb) = pair.establish().await;
        let tap = pair.net.tap();

        // Installed **after** establishment: a handshake that has to survive
        // a lost msg1 is slice 2's story, and folding it in here would make
        // an S12 red ambiguous between two slices.
        //
        // `jitter > 0` is where reordering lives: two datagrams whose draws
        // cross swap. 15 ms of jitter on a 20 ms base is enough to reorder
        // within a congestion window and — deliberately — enough to provoke
        // §13.2's *spurious* loss detection, which is a state slice 4 could
        // not reach at all.
        let flaky = FlakyPolicy::lossy(0.10)
            .with_delay(Duration::from_millis(20), Duration::from_millis(15))
            .with_duplication(0.05);
        // Six guaranteed drops on the sender's wire, four sends into the
        // transfer. B's wire gets the same probabilistic treatment, so ACKs
        // are lost too — the path is lossy in both directions, as S12 says.
        let last_dropped = arm_drops(
            &tap,
            &pair.a.wire,
            pair.a.addr(),
            flaky.clone(),
            /* skip */ 4,
            /* count */ 6,
        );
        pair.b.wire.set_policy(flaky);

        const LEN: usize = 192 * 1024;
        let want = payload(LEN);

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "S12 bulk write").await;

        // Claimed before the FIN, so the assertion below is about the FIN
        // and not about whether an unclaimed stream survives.
        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        recovering(send.finish(), "finish").await.expect("finish");
        settle().await;

        let got = read_to_end(&mut recv, "S12 read to end").await;
        assert_same_bytes(&got, &want, "S12 stream contents across a lossy path");

        // End of stream is **sticky** (§16.2, ruling 121).
        assert!(
            matches!(
                recovering(recv.read(&mut [0u8; 64]), "S12 second read").await,
                Ok(None)
            ),
            "§16.2: every read after the end of stream is `Ok(None)` — a \
             non-sticky build hangs a reader that loops until it sees it twice"
        );

        // ── working rule 9: the loss is *shown*, not assumed ────────────
        let a_sends = sent_from(&tap, pair.a.addr());
        assert!(
            a_sends > last_dropped,
            "the drop window ends at send index {last_dropped} and A's wire only \
             reached {a_sends}: the six index-drops never happened, so this test \
             proved nothing about loss recovery. `FlakyPolicy::lossy`'s own drops \
             are invisible to every counter the public API has (the tap is \
             written above the loss draw), which is why the pin is the index \
             window and not the rate."
        );
        assert!(
            a_sends > min_packets(LEN),
            "A sent {a_sends} datagrams for a payload needing at least {} — with \
             ten per cent loss in both directions a build that never retransmits \
             cannot even have tried",
            min_packets(LEN)
        );
    })
    .await;
}

/// **S12 — the first data of the connection is lost, and only the PTO can
/// rescue it.**
///
/// §13.2's loss detection is **ack-driven**: *"A tracked packet is
/// declared lost when a later packet in its space is acked and …"*. With
/// no ACK in hand it declares nothing, whatever its timers say. The state
/// this test builds is exactly that one — every datagram carrying the
/// stream is destroyed, so the peer has nothing to acknowledge and nothing
/// can come back. §13.3's `Pto` is the only machine left that can act,
/// and it must be **armed while ≥ 1 ack-eliciting packet is in the sent
/// map**, not merely as a fallback when a `Loss` timer is absent.
///
/// The blackhole is built with `block_path`, not with `drop_first(n)`.
/// `drop_first` drops indices **below `n` counting from the wire's
/// creation**, so `drop_first(4)` installed after a handshake that already
/// spent four indices drops *nothing at all* — and a test that dropped
/// nothing would pass every build. `block_path` needs no index arithmetic
/// and is counter-proved by [`blackholed`].
///
/// # BROKEN BUILD
///
/// * **A build that arms `Pto` only when a `Loss` timer is absent, or that
///   never arms it on an empty ack history.** Nothing is ever
///   acknowledged while the path is down, so §13.2's walk never runs;
///   without a probe the sender is silent for ever and the peer's read
///   times out inside [`recovering`].
/// * **A build with no loss at all.** [`blackholed`] must have grown
///   across the window — the counters show the datagrams left the wire and
///   never reached the fabric.
/// * **A build that treats an unanswered probe train as a dead
///   connection** before `DEAD_TIMEOUT`. The path heals well inside 25 s
///   and the connection must still be alive: both `closed()` futures are
///   polled.
///
/// The test deliberately does **not** pin *which* of §13.4's two arms the
/// probe takes — *"pending retransmittable frames oldest-first if any,
/// else a bare `PING`"*. Either recovers: the frames directly, or the PING
/// by drawing an ACK whose `largest` puts the outstanding packets over
/// §13.2's threshold. Pinning one reading would be pinning an ambiguity
/// (see TESTS-5b.md §3).
#[tokio::test(start_paused = true)]
async fn s12_a_stream_lost_before_anything_was_acked_is_rescued_by_the_pto() {
    local(async {
        let pair = Pair::seeded(0x5122_0502);
        let (ca, cb) = pair.establish().await;

        // A modest one-way delay, so RTT samples and the PTO have realistic
        // values in virtual time rather than collapsing to zero.
        let delay = FlakyPolicy::perfect().with_delay(Duration::from_millis(10), Duration::ZERO);
        pair.a.wire.set_policy(delay.clone());
        pair.b.wire.set_policy(delay);

        let want = payload(2000);

        // ── everything this stream sends is destroyed ───────────────────
        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "pre-blackhole write").await;
        // The driver must run **inside** the blackhole window. Ruling 114
        // says a mutating call seals what the ledger admits; it does not say
        // the datagram has left. Without this the sealed datagrams would all
        // be sent after the heal, nothing would be lost, and the test would
        // claim a blackhole it never made.
        settle().await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        let lost_now = blackholed(&pair);
        assert!(
            lost_now > lost_before,
            "the blackhole destroyed nothing: `Network::sends()` and the tap moved \
             together, so every datagram this stream sealed left the fabric \
             intact and there is no loss for §13 to recover from"
        );

        pair.net.heal_path(pair.a.addr(), pair.b.addr());

        // Nothing is retried by the test: no further write, no `settle` loop
        // that could smuggle in fresh traffic. From here only §13.3's probe
        // can move this connection.
        let mut recv = recovering(cb.accept_uni(), "accept_uni after the blackhole")
            .await
            .expect("accept_uni");
        let got = read_to_end(&mut recv, "the PTO-rescued stream").await;
        assert_same_bytes(&got, &want, "§13.3: the probe recovered the whole stream");

        assert!(
            lost_now - lost_before >= min_packets(want.len()),
            "only {} datagrams were blackholed for a {}-byte payload — the hole \
             was not the whole stream, so an ack-driven build could have \
             recovered it without ever arming a probe",
            lost_now - lost_before,
            want.len()
        );

        let mut ca_closed = pin!(ca.closed());
        let mut cb_closed = pin!(cb.closed());
        assert!(
            poll_once(ca_closed.as_mut()).await.is_pending(),
            "§13.3 / ruling 33: an unanswered probe train ends at `DEAD_TIMEOUT` \
             ({DEAD_TIMEOUT:?}), not at the first probe"
        );
        assert!(poll_once(cb_closed.as_mut()).await.is_pending());
    })
    .await;
}

/// **S12 — the ACKs are lost, not the data.**
///
/// The one shape `FlakyWire` *can* express that isolates the return path:
/// in a unidirectional bulk transfer the receiver sends nothing but
/// acknowledgements, so `block_path(b → a)` is "drop the ACKs" exactly
/// (`FlakyWire` cannot drop by **content** — see TESTS-5b.md §4.1). The
/// data arrives; the sender never learns it did.
///
/// This is the state that produces the receiver-side hazard slice 5
/// introduces and slice 4 could not reach: **a range retransmitted to a
/// peer that already has it**, arriving under a *fresh* packet counter, so
/// §7.2's replay window does not filter it and §9.5's overlap rule must.
///
/// # BROKEN BUILD
///
/// * **A sender that stops when the ACKs stop.** With `bytes_in_flight`
///   never draining, a build without §13.3's probe simply goes quiet and
///   never delivers chunk 3 after the heal.
/// * **A receiver that appends an overlapping retransmission** instead of
///   ignoring the bytes it already holds: the length assertion fires
///   (chunk 2 delivered twice).
/// * **A sender that mis-accounts the late ACK** when the path heals —
///   an ACK covering ranges it has since retransmitted — and drops or
///   duplicates chunk 3: the content assertion fires.
/// * **A build that treats a silent return path as a connection error.**
///   Both `closed()` futures are polled while the path is down.
/// * **A build with no loss at all**: [`blackholed`] must have grown while
///   the return path was down.
#[tokio::test(start_paused = true)]
async fn s12_a_stream_completes_when_the_acknowledgements_are_lost() {
    local(async {
        let pair = Pair::seeded(0x5123_0503);
        let (ca, cb) = pair.establish().await;

        let delay = FlakyPolicy::perfect().with_delay(Duration::from_millis(10), Duration::ZERO);
        pair.a.wire.set_policy(delay.clone());
        pair.b.wire.set_policy(delay);

        let chunk1 = payload(4096);
        let chunk2 = payload(4096);
        let chunk3 = payload(4096);

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &chunk1, "chunk 1").await;
        settle().await;
        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        // ── the return path dies; the forward path does not ─────────────
        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.b.addr(), pair.a.addr());

        write_all(&mut send, &chunk2, "chunk 2 (acknowledgement blackholed)").await;
        settle().await;

        // Let §13.3 fire at least once while the return path is down. This
        // is where `settle()` is not enough: it yields, it does not advance
        // the paused clock, and a timer that never comes due never fires.
        // 1.5 s clears an un-sampled PTO (≈1 024 ms) as well as a sampled
        // one, and is far inside `DEAD_TIMEOUT`.
        tokio::time::advance(Duration::from_millis(1500)).await;
        settle().await;

        let lost_now = blackholed(&pair);
        assert!(
            lost_now > lost_before,
            "the receiver sent no acknowledgement into the blackhole: with \
             nothing destroyed there is no lost-ACK state to survive, and this \
             test would pass a build that never probes"
        );

        // Scoped rather than `drop`ped: dropping a `Pin<&mut _>` releases
        // nothing — the `pin!` temporary outlives it — and trips
        // `clippy::drop_non_drop`, which `-D warnings` turns into a red.
        {
            let mut ca_closed = pin!(ca.closed());
            let mut cb_closed = pin!(cb.closed());
            assert!(
                poll_once(ca_closed.as_mut()).await.is_pending(),
                "§18.1: a silent return path is not a connection error before \
                 `DEAD_TIMEOUT`"
            );
            assert!(poll_once(cb_closed.as_mut()).await.is_pending());
        }

        // ── heal, finish, and require every byte exactly once ───────────
        pair.net.heal_path(pair.b.addr(), pair.a.addr());
        write_all(&mut send, &chunk3, "chunk 3 (after the heal)").await;
        recovering(send.finish(), "finish").await.expect("finish");
        settle().await;

        let got = read_to_end(&mut recv, "the whole stream after an ACK outage").await;
        let mut want = chunk1.clone();
        want.extend_from_slice(&chunk2);
        want.extend_from_slice(&chunk3);
        assert_same_bytes(
            &got,
            &want,
            "§9.5: a range the peer already holds may be retransmitted, and must \
             not be delivered twice",
        );
    })
    .await;
}

/// **S12 — the probe train backs off, and the transfer completes when the
/// path heals.**
///
/// §13.3: the PTO is *"doubled per consecutive unanswered probe
/// (`2^pto_count`)"*, and `pto_count` *"resets to 0 whenever **any packet
/// is newly acknowledged**"* — acknowledged, not *sent*. The distinction
/// is invisible to any assertion about completion, which is why this test
/// asserts on the **probe schedule**.
///
/// # How the schedule is observed from `tests/`
///
/// While `a → b` is blocked, every datagram A sends is counted by
/// `Network::sends()` and never reaches the tap, while every datagram B
/// sends is tapped: [`blackholed`] is therefore **exactly A's probe count**
/// during the window. Sampling it as virtual time is advanced in 5 ms
/// steps gives the probe instants to that resolution — which is ample
/// against intervals that must double.
///
/// # BROKEN BUILD
///
/// * **A build whose backoff resets on its own probes** rather than on
///   acknowledgement sends a probe train at a *fixed* interval and still
///   completes the transfer after the heal — so completion asserts
///   nothing. Caught by the interval assertion, from the side that
///   separates it: **not all intervals equal**, and the later interval at
///   least half again the earlier. This is slice 2a's jitter defect in a
///   new costume, where "no interval exceeds base + jitter" passed a core
///   with no jitter at all.
/// * **A build that never probes.** Fewer than three samples; the count
///   assertion names it.
/// * **A build that probes for ever without dying** is *not* separated
///   here, and deliberately: `DEAD_TIMEOUT` is ruling 33's, tested at S25.
#[tokio::test(start_paused = true)]
async fn s12_the_probe_train_backs_off_and_the_transfer_completes_on_heal() {
    local(async {
        let pair = Pair::seeded(0x5124_0504);
        let (ca, cb) = pair.establish().await;

        let delay = FlakyPolicy::perfect().with_delay(Duration::from_millis(10), Duration::ZERO);
        pair.a.wire.set_policy(delay.clone());
        pair.b.wire.set_policy(delay);

        let chunk1 = payload(4096);
        let chunk2 = payload(8192);

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &chunk1, "chunk 1").await;
        settle().await;
        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        // ── blackhole the forward path, mid-transfer ────────────────────
        pair.net.block_path(pair.a.addr(), pair.b.addr());
        write_all(&mut send, &chunk2, "chunk 2 (blackholed)").await;
        settle().await;

        let baseline = blackholed(&pair);
        let start = tokio::time::Instant::now();
        let step = Duration::from_millis(5);
        // 8 s of virtual time. Chunk 1 was acknowledged before the
        // blackhole, so §13.1 has a sample and the PTO is ≈85 ms — three
        // probes inside 600 ms. The window is sized for the *un*-sampled
        // regime anyway (1 024 + 2 048 + 4 096 ms), so a build that takes
        // no RTT sample from an acked data packet still reports a probe
        // schedule rather than a bare "no probes".
        //
        // Two ceilings bound it, and 8 s is under both: `DEAD_TIMEOUT`
        // (25 s), which ruling 33 arms at the **first** probe, and
        // `KEEPALIVE_TIMEOUT` (10 s) — past which §7.5's keepalive would
        // start contributing blackholed sends that are not probes and
        // silently corrupt this count. §7.5 is slice 7's, so it cannot
        // fire today; the window is chosen so that it never will.
        let samples = 1600usize;

        let mut probe_times: Vec<Duration> = Vec::new();
        let mut seen = baseline;
        for _ in 0..samples {
            tokio::time::advance(step).await;
            settle().await;
            let now = blackholed(&pair);
            while seen < now {
                probe_times.push(tokio::time::Instant::now().duration_since(start));
                seen += 1;
            }
            if probe_times.len() >= 4 {
                break;
            }
        }

        assert!(
            probe_times.len() >= 3,
            "§13.3: with nothing acknowledged and packets outstanding the PTO must \
             arm and re-arm; only {} probe(s) left the wire in {:?} of virtual \
             time. A build that disarms the probe when the path goes quiet stops \
             here.",
            probe_times.len(),
            step * u32::try_from(samples).expect("sample count fits in u32")
        );

        let first = probe_times[1] - probe_times[0];
        let later = probe_times[2] - probe_times[1];
        assert!(
            later > first,
            "§13.3: consecutive unanswered probes double — the intervals were \
             {first:?} then {later:?}. All-equal intervals are what a build whose \
             `pto_count` resets on its own *sends* produces, and that build \
             completes the transfer after the heal exactly like a correct one, so \
             completion alone separates nothing."
        );
        assert!(
            later * 2 >= first * 3,
            "§13.3: the second interval should be about twice the first \
             (`2^pto_count`); got {first:?} then {later:?}"
        );

        // ── heal, and require the whole transfer ────────────────────────
        pair.net.heal_path(pair.a.addr(), pair.b.addr());
        recovering(send.finish(), "finish").await.expect("finish");
        settle().await;

        let got = read_to_end(&mut recv, "the transfer after the blackhole healed").await;
        let mut want = chunk1.clone();
        want.extend_from_slice(&chunk2);
        assert_same_bytes(&got, &want, "§13: the blackholed range is retransmitted");
    })
    .await;
}

/// **S12/S13 — a stream that is retransmitting does not starve its
/// sibling.**
///
/// S12 says the transfer survives loss; S13 says *"loss on one does not
/// stall another"*. Slice 4b's S13 test builds a **permanent** hole and
/// asserts a sibling still completes — but under slice 4 there was no
/// retransmission to compete with. The question this test asks did not
/// exist before slice 5: when the sender has both a **retransmission
/// queue** and a large backlog on stream A, does stream B ever get a turn?
///
/// The measurement is the completion **order**, taken from the paused
/// clock inside each reader, not from the test's own sequencing — a
/// sequential test cannot tell "B finished early" from "B was read
/// early".
///
/// # BROKEN BUILD
///
/// * **A build that re-queues a lost range at the head of the rotation**
///   (`push_front` where `push_back` belongs — ruling 114 used exactly
///   this mutation): A monopolises the sender, B's 2 KiB lands only after
///   A's 96 KiB and the instant comparison inverts.
/// * **A build with no rotation at all** — strict FIFO over the send
///   queue — fails the same assertion, and fails it *before* B's stream is
///   even visible at the peer, so the second `accept_uni` is where it
///   stops.
/// * **A build with no loss injected**: the `sent_from` assertion shows
///   the drop window was reached.
/// * **A build that never recovers A**: A's reader is read to EOF too, so
///   the fair-but-lossy sibling case does not pass by abandoning A.
#[tokio::test(start_paused = true)]
async fn s12_a_retransmitting_stream_does_not_starve_its_sibling() {
    local(async {
        let pair = Pair::seeded(0x5125_0505);
        let (ca, cb) = pair.establish().await;
        let tap = pair.net.tap();

        // No jitter: reordering would add a second, unrelated source of
        // retransmission and this test is about the queue, not the draw.
        let base = FlakyPolicy::perfect().with_delay(Duration::from_millis(5), Duration::ZERO);
        pair.b.wire.set_policy(base.clone());
        // Twelve consecutive datagrams of stream A are destroyed — a
        // genuinely multi-packet hole, big enough that its retransmission
        // is a queue and not a single frame.
        let last_dropped = arm_drops(
            &tap,
            &pair.a.wire,
            pair.a.addr(),
            base,
            /* skip */ 2,
            /* count */ 12,
        );

        let big = payload(96 * 1024);
        let small = payload(2048);

        let mut a_send = within(ca.open_uni(), "open A").await.expect("open A");
        write_all(&mut a_send, &big, "A bulk").await;
        recovering(a_send.finish(), "A finish")
            .await
            .expect("finish");

        // Opened **after** A's backlog is queued: the rotation now has a
        // choice to make, and a starving build makes the wrong one.
        let mut b_send = within(ca.open_uni(), "open B").await.expect("open B");
        write_all(&mut b_send, &small, "B payload").await;
        recovering(b_send.finish(), "B finish")
            .await
            .expect("finish");
        settle().await;

        let mut r_a = recovering(cb.accept_uni(), "accept A")
            .await
            .expect("accept A");
        let mut r_b = recovering(cb.accept_uni(), "accept B")
            .await
            .expect("accept B");
        assert_eq!(
            r_a.id(),
            a_send.id(),
            "ruling 112: the first `accept_uni` yields the first stream opened"
        );
        assert_eq!(r_b.id(), b_send.id());

        // Both drained concurrently; each records **when** it saw EOF, in
        // virtual time since a common origin. Wall-clock order of the
        // `join!` arms says nothing; the paused clock does.
        let origin = tokio::time::Instant::now();
        let (a_done, b_done) = tokio::join!(
            async {
                let got = read_to_end(&mut r_a, "A read to end").await;
                (got, tokio::time::Instant::now().duration_since(origin))
            },
            async {
                let got = read_to_end(&mut r_b, "B read to end").await;
                (got, tokio::time::Instant::now().duration_since(origin))
            }
        );

        assert_same_bytes(&a_done.0, &big, "A's contents after recovery");
        assert_same_bytes(&b_done.0, &small, "B's contents");

        assert!(
            b_done.1 < a_done.1,
            "S13: stream B carries 2 KiB and stream A 96 KiB with a twelve-packet \
             hole to retransmit — B must finish first. B finished at {:?} and A at \
             {:?}, which is what a sender that services A's retransmission queue \
             to exhaustion before rotating produces.",
            b_done.1,
            a_done.1
        );

        let a_sends = sent_from(&tap, pair.a.addr());
        assert!(
            a_sends > last_dropped,
            "A's wire reached only index {a_sends}; the twelve-packet hole at \
             index {last_dropped} was never made and there was nothing to \
             retransmit"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S28 — delivery confirmation
// ══════════════════════════════════════════════════════════════════════

/// **S28 — `write; finish; acked(); close()` does not lose its tail.**
///
/// > The application can await transport-level acknowledgement of what it
/// > has sent, then `close()` without loss.
///
/// The farewell message, ruling 47's whole subject. §15.2 is deliberately
/// **unchanged** — `close()` still drops stream, recovery and congestion
/// state immediately — so the only thing standing between a lossy path and
/// a silently truncated last message is this verb.
///
/// # BROKEN BUILD
///
/// * **`acked()` resolves on `finish()`** — the tempting one-liner, and
///   the exact pre-ruling-47 behaviour. The two index-dropped datagrams
///   are still outstanding when `close()` lands, §15.2 frees the recovery
///   state, they are never retransmitted, and **the peer's read is
///   short** — it never reaches its EOF and [`read_to_end`] fails inside
///   [`recovering`]. *The assertion is on the peer's bytes, not on
///   `acked()` returning*; a build that returns `Ok(())` for the wrong
///   reason is caught only there.
/// * **`acked()` returns `WriteError::Finished`** because it was called
///   after `finish()`. §16.2:4424 forbids it in terms — *"It is legal and
///   expected after `finish()`, and never returns `WriteError::Finished`
///   for that reason"* — and the `assert_eq!` names it.
/// * **A build with no loss injected.** The drop window must have been
///   reached; otherwise the close could not have lost anything and the
///   test would pass the pre-ruling-47 ordering too.
#[tokio::test(start_paused = true)]
async fn s28_a_stream_acked_before_the_close_does_not_lose_its_tail() {
    local(async {
        let pair = Pair::seeded(0x5281_0501);
        let (ca, cb) = pair.establish().await;
        let tap = pair.net.tap();

        let flaky = FlakyPolicy::lossy(0.20).with_delay(Duration::from_millis(10), Duration::ZERO);
        // Two datagrams of the payload destroyed outright, one send into
        // the write: whatever the seed does, this tail is unacknowledged
        // when a build that resolves at `finish()` calls `close()`.
        let last_dropped = arm_drops(
            &tap,
            &pair.a.wire,
            pair.a.addr(),
            flaky.clone(),
            /* skip */ 1,
            /* count */ 2,
        );
        pair.b.wire.set_policy(flaky);

        let want = payload(4096);

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "S28 write").await;
        settle().await;

        // Claimed before the close, so only ruling 128's *read* clause is
        // in play below and not its accept latch.
        let mut recv = recovering(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        recovering(send.finish(), "finish").await.expect("finish");

        assert_eq!(
            recovering(send.acked(), "SendStream::acked").await,
            Ok(()),
            "§16.2:4424 / ruling 47: `acked()` after `finish()` is legal and \
             expected, and never reports `WriteError::Finished`"
        );

        // §15.2's licence to drop recovery state, exercised immediately.
        ca.close(NO_ERROR, b"").await;
        settle().await;

        let got = read_to_end(&mut recv, "the peer's copy after the close").await;
        assert_same_bytes(
            &got,
            &want,
            "S28: every byte written before `acked()` resolved is at the peer, \
             and the FIN with it",
        );

        let a_sends = sent_from(&tap, pair.a.addr());
        assert!(
            a_sends > last_dropped,
            "A's wire reached index {a_sends}, short of the drop window ending at \
             {last_dropped}: nothing was destroyed, so `close()` had nothing to \
             lose and this test would pass the pre-ruling-47 ordering"
        );
    })
    .await;
}

/// **S28 — `Connection::acked()` covers every stream, not the last one
/// written.**
///
/// §16.2:4428: the snapshot is *"every byte handed to the connection at
/// the instant of the call, **across every stream**"*.
///
/// # BROKEN BUILD
///
/// * **A build that snapshots only the most recently written stream**
///   passes the `SendStream::acked()` test above and fails here: the
///   drops are placed on stream **1**, so such a build resolves as soon as
///   stream 2 is acknowledged — a full round trip before stream 1's
///   retransmission can land — closes, and stream 1 arrives at the peer
///   with a hole that the freed recovery state can never fill.
/// * **A build that resolves on the first `StreamFinished`** fails the
///   same way and for the same reason.
/// * **A build with no loss injected**: the drop-window assertion.
#[tokio::test(start_paused = true)]
async fn s28_connection_acked_covers_every_stream_before_the_close() {
    local(async {
        let pair = Pair::seeded(0x5282_0502);
        let (ca, cb) = pair.establish().await;
        let tap = pair.net.tap();

        let flaky = FlakyPolicy::lossy(0.10).with_delay(Duration::from_millis(10), Duration::ZERO);
        let last_dropped = arm_drops(
            &tap,
            &pair.a.wire,
            pair.a.addr(),
            flaky.clone(),
            /* skip */ 1,
            /* count */ 2,
        );
        pair.b.wire.set_policy(flaky);

        let first = payload(4096);
        let second = payload(4096);

        let mut s1 = within(ca.open_uni(), "open s1").await.expect("open s1");
        write_all(&mut s1, &first, "s1 write").await;
        recovering(s1.finish(), "s1 finish").await.expect("finish");

        let mut s2 = within(ca.open_uni(), "open s2").await.expect("open s2");
        write_all(&mut s2, &second, "s2 write").await;
        recovering(s2.finish(), "s2 finish").await.expect("finish");
        settle().await;

        let mut r1 = recovering(cb.accept_uni(), "accept s1")
            .await
            .expect("accept");
        let mut r2 = recovering(cb.accept_uni(), "accept s2")
            .await
            .expect("accept");

        assert_eq!(
            recovering(ca.acked(), "Connection::acked").await,
            Ok(()),
            "rulings 47/54: the snapshot spans every stream, and both were \
             written before the call"
        );

        ca.close(NO_ERROR, b"").await;
        settle().await;

        assert_same_bytes(
            &read_to_end(&mut r1, "s1 after the close").await,
            &first,
            "S28: stream 1 carried the injected hole and must still be whole",
        );
        assert_same_bytes(
            &read_to_end(&mut r2, "s2 after the close").await,
            &second,
            "S28: stream 2 as well",
        );

        let a_sends = sent_from(&tap, pair.a.addr());
        assert!(
            a_sends > last_dropped,
            "A's wire reached index {a_sends}, short of the drop window ending at \
             {last_dropped}: no hole was made in stream 1, so a snapshot covering \
             only stream 2 would have passed"
        );
    })
    .await;
}

/// **S28 — an empty snapshot resolves at once, alive or dead.**
///
/// `CONTRACT-5b.md` §2.5: *"An **empty** snapshot (nothing ever written)
/// resolves `Ok(())` on the first poll, on a live *or* dead
/// connection."* The dead half is ruling 135 in its cheapest form — the
/// death latch must not outrank a settled snapshot.
///
/// # BROKEN BUILD
///
/// * **A build that parks until a `StreamFinished` that will never come**
///   hangs on a connection that has never written a byte. This is the
///   shape an application reaches by putting `acked()` in a shutdown
///   helper that also runs on idle connections, so it is not a contrived
///   input.
/// * **A build that consults the death latch first** (the plain reading of
///   ruling 124's order) returns `Err(ConnectionLost)` on the second half.
///   Asserting only the live half would pass it — which is why both halves
///   are here, on one connection, in one test.
/// * **A build that resolves eventually rather than immediately** is
///   caught by [`poll_once`]: the assertion is `Ready` on the **first**
///   poll, not "within a budget".
#[tokio::test(start_paused = true)]
async fn s28_connection_acked_on_an_empty_snapshot_resolves_at_once_live_and_dead() {
    local(async {
        let pair = Pair::seeded(0x5283_0503);
        let (ca, cb) = pair.establish().await;

        // ── alive, nothing ever written ────────────────────────────────
        //
        // Scoped rather than `drop`ped: a dropped `Pin<&mut _>` releases
        // nothing and trips `clippy::drop_non_drop` under `-D warnings`.
        {
            let mut live = pin!(ca.acked());
            assert_eq!(
                poll_once(live.as_mut()).await,
                Poll::Ready(Ok(())),
                "CONTRACT-5b §2.5: an empty snapshot is settled by definition and \
                 resolves on the first poll"
            );
        }

        // ── and now the same connection, dead ──────────────────────────
        cb.close(NO_ERROR, b"bye").await;
        settle().await;
        assert!(
            matches!(
                within(ca.closed(), "closed").await,
                ConnectionLost::PeerClosed { .. }
            ),
            "the second half is only a test if the connection really died"
        );

        let mut dead = pin!(ca.acked());
        assert_eq!(
            poll_once(dead.as_mut()).await,
            Poll::Ready(Ok(())),
            "ruling 135: both `acked()` verbs answer from their settled snapshot \
             **before** the death latch. Nothing was ever written, so there is \
             nothing the death can have prevented — reporting `ConnectionLost` \
             here is ruling 121's misreport with its sign flipped"
        );
    })
    .await;
}

/// **S28 — a fully acknowledged transfer never reports `ConnectionLost`.**
///
/// Ruling 135, and the reason it exists: *"The peer's ACK and its CLOSE
/// can arrive in one driver pass"*. §16.2:4362 describes the receiving
/// end of the same race — *"a sender that writes, finishes and drops its
/// handles closes implicitly, its peer's driver processes the data and the
/// CLOSE in one pass, and the peer's application is woken **after** the
/// latch is set"* — and ruling 135 is that defect on the **sender's**
/// side, reachable by the identical mechanism.
///
/// Here the peer reads everything, acknowledges everything, and closes.
/// Both verbs are then asked, on a connection that is already dead.
///
/// # BROKEN BUILD
///
/// * **A build that checks the death latch before the settled snapshot**
///   — the plain reading of ruling 124's precedence order, and the one an
///   implementer writes first — reports `Err(ConnectionLost)` over a
///   transfer that completed in full. The application's only recourse is
///   then to resend a message the peer already has.
/// * **A build that returns `Ok(())` on any dead connection** is the
///   opposite error and is *not* caught here — it is caught by
///   `s28_an_unacknowledged_transfer_on_a_dead_connection_reports_the_loss`,
///   which is this test with the ACK path cut. Neither test alone
///   separates both; the pair does.
#[tokio::test(start_paused = true)]
async fn s28_a_fully_acknowledged_transfer_does_not_report_connection_lost() {
    local(async {
        let pair = Pair::seeded(0x5284_0504);
        let (ca, cb) = pair.establish().await;

        let want = payload(4096);
        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "write").await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");
        let got = read_to_end(&mut recv, "the peer's copy").await;
        assert_same_bytes(&got, &want, "the transfer completed before the close");

        // The peer acknowledged everything, then closed. No `settle()` sits
        // between the two on this side: the ACK and the CLOSE are given
        // every chance to reach A's driver in one pass, which is the race
        // ruling 135 names.
        cb.close(NO_ERROR, b"done").await;
        settle().await;

        assert!(
            matches!(
                within(ca.closed(), "closed").await,
                ConnectionLost::PeerClosed { .. }
            ),
            "the assertions below are only tests if the connection is dead"
        );

        assert_eq!(
            within(send.acked(), "SendStream::acked after the death").await,
            Ok(()),
            "ruling 135 / CONTRACT-5b §2.5 outcome 2: this half reached \
             `DataRecvd` — every byte and the FIN acknowledged — and a stream \
             that completed did not un-complete when the connection died"
        );
        assert_eq!(
            within(ca.acked(), "Connection::acked after the death").await,
            Ok(()),
            "ruling 135: the connection verb answers from the same settled \
             snapshot, before the same latch"
        );
    })
    .await;
}

/// **S28 — an *un*acknowledged transfer on a dead connection reports the
/// loss.**
///
/// The other side of ruling 135, and the reason the test above is not
/// passed by `Ok(())`-always. `CONTRACT-5b.md` §2.5 outcome 3: the death
/// latch stands wherever the snapshot is **not** settled.
///
/// # BROKEN BUILD
///
/// * **A build that answers `Ok(())` whenever the connection is dead**
///   passes `s28_a_fully_acknowledged_transfer_…` and fails here. It is
///   the strictly more dangerous error of the two: it tells an application
///   its farewell message was received when the path was blackholed
///   throughout.
/// * **A build that parks on a dead connection**: rulings 124/128 —
///   *"Parking is never permitted on a dead connection"* — and [`within`]
///   fails rather than hanging.
///
/// Both `acked()` calls here are made **after** the latch is already set.
/// The other order — parked first, death second — is
/// `s28_a_parked_acked_wakes_when_the_connection_dies`, and it is a
/// different mechanism: this one is a *read* of the latch, that one is a
/// *wakeup* from it.
#[tokio::test(start_paused = true)]
async fn s28_an_unacknowledged_transfer_on_a_dead_connection_reports_the_loss() {
    local(async {
        let pair = Pair::seeded(0x5285_0505);
        let (ca, cb) = pair.establish().await;

        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &payload(4096), "write into the blackhole").await;
        settle().await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        assert!(
            blackholed(&pair) > lost_before,
            "nothing was destroyed: with the bytes safely delivered the snapshot \
             would settle and this test would assert the opposite of what it says"
        );

        // The peer never saw a byte, and closes on its own. `b → a` is open,
        // so the CLOSE lands.
        cb.close(NO_ERROR, b"bye").await;
        settle().await;
        assert!(matches!(
            within(ca.closed(), "closed").await,
            ConnectionLost::PeerClosed { .. }
        ));

        assert_eq!(
            within(send.acked(), "SendStream::acked on a dead connection").await,
            Err(WriteError::ConnectionLost(ConnectionLost::PeerClosed {
                code: NO_ERROR,
                reason: b"bye".to_vec(),
            })),
            "CONTRACT-5b §2.5 outcome 3: nothing was acknowledged, so the death \
             latch is the answer — and it must carry the peer's code and reason, \
             not a bare `TimedOut`"
        );
        assert_eq!(
            within(ca.acked(), "Connection::acked on a dead connection").await,
            Err(ConnectionLost::PeerClosed {
                code: NO_ERROR,
                reason: b"bye".to_vec(),
            }),
            "ruling 135 does not make `acked()` blind to the death — only to a \
             death that arrived after the snapshot had already settled"
        );
    })
    .await;
}

/// **S28 — an `acked()` already parked when the connection dies wakes.**
///
/// The mirror of the test above, and the one the **binding contract's own
/// waker list argues for**. `CONTRACT-5b.md` §2.5 parks
/// `SendStream::acked()` in `blocked_ackers`, *"woken by
/// `ConnEvent::StreamFinished { r }` and by `ConnEvent::StreamReset
/// { r, .. }`"* — and **the death latch is not in that list**, while the
/// sibling paragraph for `Connection::acked()` two entries below ends
/// *"and on the latch"*. Working rule 8: a list is read as exhaustive
/// whether or not it says so. Taken exhaustively, a `SendStream::acked()`
/// that is already parked when the connection dies is woken by nothing,
/// and rulings 124/128's *"Parking is never permitted on a dead
/// connection"* is violated in the one direction no reader can escape —
/// the application is asleep, not asking.
///
/// Reported in `TESTS-5b.md` §3 rather than resolved (working rule 3);
/// this test is what a red would look like.
///
/// # BROKEN BUILD
///
/// * **A build that wakes `blocked_ackers` only on `StreamFinished` /
///   `StreamReset`** hangs both halves for ever: the stream is neither
///   finished-and-acknowledged nor reset, and no further event will ever
///   arrive on a dead connection. [`within`] reports it instead of hanging
///   the suite.
/// * **A build that resolves them `Ok(())`** — the ruling-135 over-reach —
///   is caught by the exact error values: nothing was acknowledged, so
///   the settled snapshot is empty of settled bytes and the latch is the
///   answer.
#[tokio::test(start_paused = true)]
async fn s28_a_parked_acked_wakes_when_the_connection_dies() {
    local(async {
        let pair = Pair::seeded(0x528a_050a);
        let (ca, cb) = pair.establish().await;

        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &payload(4096), "write into the blackhole").await;
        settle().await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        assert!(
            blackholed(&pair) > lost_before,
            "the bytes must be unacknowledgeable, or the futures below are not \
             parked and the wakeup under test never happens"
        );

        // Both futures are parked **before** the death, and held across it.
        let mut stream_acked = pin!(send.acked());
        let mut conn_acked = pin!(ca.acked());
        assert!(
            poll_once(stream_acked.as_mut()).await.is_pending(),
            "nothing can be acknowledged across a blackholed path"
        );
        assert!(poll_once(conn_acked.as_mut()).await.is_pending());

        cb.close(NO_ERROR, b"bye").await;
        settle().await;

        assert_eq!(
            within(stream_acked, "the parked SendStream::acked at the death").await,
            Err(WriteError::ConnectionLost(ConnectionLost::PeerClosed {
                code: NO_ERROR,
                reason: b"bye".to_vec(),
            })),
            "CONTRACT-5b §2.5 lists `blocked_ackers`' wakers as `StreamFinished` \
             and `StreamReset` and omits the latch — a parked `acked()` that the \
             death does not wake is a permanent hang, and it is the one shape the \
             application cannot poll its way out of"
        );
        assert_eq!(
            within(conn_acked, "the parked Connection::acked at the death").await,
            Err(ConnectionLost::PeerClosed {
                code: NO_ERROR,
                reason: b"bye".to_vec(),
            }),
            "the contract *does* say `settled_wakers` is woken on the latch — this \
             half is here so a red on the half above is unambiguous"
        );
    })
    .await;
}

/// **S28 — `SendStream::acked()` before `finish()` parks, and resolves
/// after it.**
///
/// Ruling 139(e): *"`SendStream::acked()` **before `finish()` parks and
/// does not resolve**"*, because §16.2 requires *"every byte … **and its
/// FIN**"* and a FIN that was never queued cannot be acknowledged. It is
/// the one shape a caller writes by accident —
/// `write(..).await; acked().await;` — and it looks like a hang.
///
/// **The park is asserted with a bounded [`is_pending`], never with a bare
/// `await`**: a bare await against a build that honours 139(e) hangs the
/// suite rather than reporting anything.
///
/// # BROKEN BUILD — and why both halves are needed
///
/// * **A build that resolves once every written byte is acknowledged**,
///   ignoring the FIN, resolves the first `acked()` here: the path is
///   perfect and `settle()` has already carried every byte to the peer and
///   its ACK back. Caught by the `is_pending` half.
/// * **A build that never resolves `acked()` at all** — or one that waits
///   for a `StreamFinished` it never subscribes to — passes the
///   `is_pending` half for the wrong reason entirely. Caught by the second
///   half, on the same stream, in the same test. Working rule 9: the
///   assertion has to come from the side that separates, and here that
///   takes both sides.
#[tokio::test(start_paused = true)]
async fn s28_stream_acked_parks_before_finish_and_resolves_after_it() {
    local(async {
        let pair = Pair::seeded(0x5286_0506);
        let (ca, cb) = pair.establish().await;

        let want = payload(4096);
        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &want, "write").await;
        settle().await;

        // A perfect path: by now every written byte is at the peer and its
        // acknowledgement is home. The **only** thing outstanding is a FIN
        // that was never queued.
        let mut recv = within(cb.accept_uni(), "accept_uni")
            .await
            .expect("accept_uni");

        assert!(
            is_pending(send.acked()).await,
            "ruling 139(e): before `finish()` there is no FIN to acknowledge, so \
             `acked()` parks — even with every written byte already acknowledged. \
             A build that resolves here tells `write().await; acked().await;` that \
             a stream it has not finished is safely delivered."
        );

        within(send.finish(), "finish").await.expect("finish");
        assert_eq!(
            within(send.acked(), "acked after finish").await,
            Ok(()),
            "§16.2:4424: and once the FIN *is* queued and acknowledged it \
             resolves — the half that stops the park above being satisfied by a \
             build that never resolves at all"
        );

        // The FIN really reached the peer: the park was about the FIN, and
        // the resolution was about the FIN.
        assert_same_bytes(
            &read_to_end(&mut recv, "read to end").await,
            &want,
            "the peer saw the data and the FIN",
        );
    })
    .await;
}

/// **S28 — `acked()` reports the reset that abandoned its bytes.**
///
/// §16.2:4425: *"It returns `Reset(code)` if the stream was reset before
/// its data was acknowledged"*, and `CONTRACT-5b.md` §2.5 outcome 1 puts
/// that first in ruling 124's precedence order. §9.6's abandoned bytes are
/// never acknowledged, so the alternative to reporting the reset is
/// waiting for ever.
///
/// The forward path is blackholed for the whole test, so nothing *can* be
/// acknowledged: the `Pending` observed before the reset is caused by the
/// outstanding bytes and the resolution afterwards can only be caused by
/// the reset.
///
/// # BROKEN BUILD
///
/// * **A build that only ever resolves `Ok`** hangs here, for ever, on
///   bytes §9.6 guarantees will never be acknowledged.
/// * **A build that reports `WriteError::Finished`** — the handle *was*
///   finished — is caught by the exact `assert_eq!`; §16.2 forbids that
///   variant from this verb outright.
/// * **A build that reports `Ok(())` on a reset**, treating abandonment as
///   completion, tells the application its farewell message landed when it
///   was thrown away. Same `assert_eq!`, other side.
/// * **A build that resolves the park for an unrelated reason** is
///   excluded by the `is_pending` before the reset.
#[tokio::test(start_paused = true)]
async fn s28_stream_acked_reports_the_reset_that_abandoned_its_bytes() {
    local(async {
        let pair = Pair::seeded(0x5287_0507);
        let (ca, _cb) = pair.establish().await;

        const CODE: u64 = 0x2a;

        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        let mut send = within(ca.open_uni(), "open_uni").await.expect("open_uni");
        write_all(&mut send, &payload(4096), "write into the blackhole").await;
        settle().await;
        within(send.finish(), "finish").await.expect("finish");
        settle().await;

        assert!(
            blackholed(&pair) > lost_before,
            "the bytes must be genuinely unacknowledgeable, or the `Pending` below \
             is a race the test happened to win"
        );

        assert!(
            is_pending(send.acked()).await,
            "nothing can be acknowledged across a blackholed path, so `acked()` \
             parks — the half that makes the resolution below attributable to the \
             reset"
        );

        send.reset(CODE);
        settle().await;

        assert_eq!(
            within(send.acked(), "acked after reset").await,
            Err(WriteError::Reset(CODE)),
            "§16.2:4425 / CONTRACT-5b §2.5 outcome 1: the reset outranks \
             everything, carries the application's own code, and is never \
             `WriteError::Finished` — {CODE:#x} is deliberately not `0`, which is \
             §16.2's drop default, so the two cannot be confused"
        );
    })
    .await;
}

/// **S28 — a reset inside the snapshot lets `Connection::acked()`
/// resolve.**
///
/// §16.2:4431: the snapshot resolves when each byte is acknowledged *"**or
/// abandoned by a reset** (§9.6: an abandoned byte is never acknowledged,
/// and waiting on one would never terminate)"*. `SPEC.md`'s own test
/// obligation for ruling 47 names this case explicitly: *"a stream reset
/// inside the snapshot lets `acked()` resolve `Ok(())` rather than waiting
/// on bytes that will never be acknowledged"*.
///
/// The future is **held across the reset**, so this is one snapshot taken
/// while the bytes were live and then abandoned — not a fresh snapshot
/// taken after the reset, which a build could satisfy by simply omitting
/// reset streams. It therefore also pins the wakeup: `CONTRACT-5b.md`
/// §2.5 requires `settled_wakers` to be woken on **every** `StreamReset`.
///
/// # BROKEN BUILD
///
/// * **A build that waits for acknowledgement of abandoned bytes** never
///   resolves; §9.6 guarantees those bytes are never acknowledged, so this
///   is a permanent hang in an application's shutdown path.
/// * **A build that wakes `settled_wakers` only on `StreamFinished`**
///   leaves the future parked with its condition already true — the
///   subtler half, and the one the held future is here to catch.
/// * **A build that resolves the snapshot early** is excluded by the
///   `poll_once` assertion before the reset.
#[tokio::test(start_paused = true)]
async fn s28_connection_acked_resolves_when_a_stream_in_the_snapshot_is_reset() {
    local(async {
        let pair = Pair::seeded(0x5288_0508);
        let (ca, _cb) = pair.establish().await;

        let lost_before = blackholed(&pair);
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        let mut s1 = within(ca.open_uni(), "open s1").await.expect("open s1");
        write_all(&mut s1, &payload(4096), "s1 write").await;
        let mut s2 = within(ca.open_uni(), "open s2").await.expect("open s2");
        write_all(&mut s2, &payload(4096), "s2 write").await;
        settle().await;

        assert!(
            blackholed(&pair) > lost_before,
            "both streams' bytes must be unacknowledgeable for the snapshot to be \
             unsettled at the call"
        );

        let mut snapshot = pin!(ca.acked());
        assert!(
            poll_once(snapshot.as_mut()).await.is_pending(),
            "the snapshot holds 8 KiB that no acknowledgement can reach"
        );

        s1.reset(1);
        s2.reset(2);
        settle().await;

        assert_eq!(
            within(snapshot, "the held snapshot after both resets").await,
            Ok(()),
            "§16.2:4431: every byte in the snapshot is now abandoned by a reset, \
             which settles it — waiting on an abandoned byte never terminates"
        );
    })
    .await;
}

/// **S28 — `Connection::acked()` terminates while a bulk stream is still
/// being written.**
///
/// §16.2:4433: *"Bytes written **after** the call do not extend it, so
/// `acked()` terminates on a live connection even while a bulk stream is
/// still being written."* This is the clause that makes the verb a
/// *snapshot* rather than a quiescence check, and the one a plausible
/// implementation gets wrong.
///
/// # BROKEN BUILD
///
/// * **A build whose snapshot is "every stream's current offset, re-read
///   at each poll"** never terminates under a writer loop — the offsets
///   move faster than the acknowledgements. It is indistinguishable from a
///   correct build on every quiet connection, which is why the writer here
///   has to be genuinely running.
/// * **A build that resolves without waiting for anything** is not
///   separated by this test and is not meant to be; the tests above hold
///   that side.
///
/// # Why the counter assertion is load-bearing
///
/// If the background writer were parked — on flow control, on the
/// congestion window, on a reader that stopped draining — the "still being
/// written" premise would be false and a re-read-each-poll build would
/// terminate too, passing for free. So the writer's progress is counted
/// and the test asserts it **moved across the call**. Working rule 9: a
/// bound the degenerate case satisfies asserts nothing.
#[tokio::test(start_paused = true)]
async fn s28_connection_acked_terminates_while_a_bulk_stream_is_still_being_written() {
    local(async {
        let pair = Pair::seeded(0x5289_0509);
        let (ca, cb) = pair.establish().await;

        // **A one-way delay is load-bearing here, not decoration.** On a
        // zero-delay path a bulk writer and a bulk reader are both always
        // ready, the runtime is never idle, and tokio's paused clock — which
        // only auto-advances when every task is idle — never moves. A
        // `timeout` that can never fire turns a failing build's hang into a
        // hung *suite* instead of a red. With 5 ms each way the writer parks
        // on credit between bursts, the runtime idles, and virtual time (and
        // therefore `recovering`'s budget) advances.
        let delay = FlakyPolicy::perfect().with_delay(Duration::from_millis(5), Duration::ZERO);
        pair.a.wire.set_policy(delay.clone());
        pair.b.wire.set_policy(delay);

        // A short, finished stream: the snapshot is not empty.
        let closed_payload = payload(4096);
        let mut done = within(ca.open_uni(), "open done").await.expect("open");
        write_all(&mut done, &closed_payload, "done write").await;
        within(done.finish(), "done finish").await.expect("finish");

        // And a stream that never stops growing.
        let mut bulk = within(ca.open_uni(), "open bulk").await.expect("open");
        write_all(&mut bulk, &payload(1024), "bulk seed").await;
        settle().await;

        let mut r_done = within(cb.accept_uni(), "accept done")
            .await
            .expect("accept");
        let mut r_bulk = within(cb.accept_uni(), "accept bulk")
            .await
            .expect("accept");

        let written = Rc::new(Cell::new(0usize));
        let counter = Rc::clone(&written);
        // `yield_now` on every turn: a future that returns `Ready` does not
        // yield to the executor, so an unyielding loop of accepted writes
        // starves every other task on this current-thread runtime — the
        // same hang described above, by a second route.
        let writer = tokio::task::spawn_local(async move {
            let chunk = payload(4096);
            loop {
                match bulk.write(&chunk).await {
                    Ok(n) => counter.set(counter.get() + n),
                    Err(_) => break,
                }
                tokio::task::yield_now().await;
            }
        });
        // A reader, so the writer keeps being *granted* credit: a writer
        // parked for ever on flow control is not "still being written", and
        // a re-read-at-each-poll build would terminate too — passing for
        // free, which is working rule 9's failure.
        let reader = tokio::task::spawn_local(async move {
            let mut buf = vec![0u8; 8192];
            while let Ok(Some(_)) = r_bulk.read(&mut buf).await {
                tokio::task::yield_now().await;
            }
        });

        settle().await;
        let before = written.get();
        assert!(
            before > 0,
            "the background writer never started; the premise of this test is that \
             a bulk stream is *being written* during the call"
        );

        assert_eq!(
            recovering(ca.acked(), "Connection::acked under a live writer").await,
            Ok(()),
            "§16.2:4433: the snapshot is taken once — bytes written after the call \
             do not extend it. A build that re-reads every stream's offset at each \
             poll never resolves this."
        );

        settle().await;
        let after = written.get();
        assert!(
            after > before,
            "the writer wrote {before} bytes before the call and {after} after it: \
             it was not running during `acked()`, so a build that re-reads offsets \
             at every poll would have terminated too and this test proved nothing"
        );

        writer.abort();
        reader.abort();

        // The finished stream is intact — the snapshot's other half was real.
        assert_same_bytes(
            &read_to_end(&mut r_done, "the finished stream").await,
            &closed_payload,
            "the settled half of the snapshot arrived in full",
        );
    })
    .await;
}

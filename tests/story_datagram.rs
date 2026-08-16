//! **Datagrams — S15.**
//!
//! > `send_datagram` never waits — it drops oldest under pressure rather
//! > than blocking — and delivery is unordered and unreliable by contract.
//! > Oversize input is `DatagramError::TooLarge`, not a silent truncation.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by the **blind test author for datagrams** in slice 6, from
//! `STORIES.md` S15, `SPEC.md` §11 / §10.7 / §8.4 / §8.5 / §18.1 and
//! `.slices/06-sugar/CONTRACT-6.md` **alone**, while the implementer wrote
//! `src/core/connection/` and `src/shell/` concurrently. No line of the
//! datagram implementation was read; every name below is spelled as
//! `CONTRACT-6.md` §2.3/§2.5 spells it. **If a name here does not compile,
//! the contract and the implementation disagree, and that is the finding —
//! not a rename to be made here.**
//!
//! # Working rule 9 — every test names the build it separates
//!
//! *A bound is only a test if the degenerate case violates it.* Each test
//! carries a `BROKEN BUILD:` block naming a concrete implementation that
//! passes a weaker version of the test while being wrong. The mirror rule
//! is enforced too: **an assertion a conforming build can fail is a flake,
//! not a pin**, so where the strong form needs core-level visibility this
//! file does not have, the test says so and points at the core test that
//! owns it (`.slices/06-sugar/PLAN-6.md` §5.1, D1–D12).
//!
//! # Datagrams make working rule 9 unusually hard — two standing hazards
//!
//! 1. **Delivery is not promised** (§11.1: *"no delivery promise, no
//!    ordering promise, no retransmission, no sequence identity at all"*),
//!    so *"the datagram arrived"* is a hazardous assertion. Every arrival
//!    assertion in this file is made over
//!    [`FlakyPolicy::perfect`](slither::testutil::FlakyPolicy::perfect) on
//!    the deterministic fixture (ruling 60: seeded RNG, fixed draw order,
//!    no OS entropy), where *"what was sealed is delivered"* is a fact and
//!    not a probability. **No test in this file asserts an arrival while
//!    loss is injected on the path that arrival must cross.**
//! 2. **Ruling 148: `FlakyPolicy::lossy(rate)` is invisible to every
//!    counter the public API has** — the tap is written *above* the loss
//!    draw, so `Network::sends() − Tap::len()` sees blackholes and injected
//!    send failures only. `lossy()` appears nowhere in this file. The one
//!    test that needs a datagram to die uses `drop_at` (index-based, no RNG
//!    draw) and **proves the drop window was reached** from the tap before
//!    asserting anything about the consequence.
//!
//! # What this file cannot reach, and who owns it
//!
//! `tests/*.rs` see the public API plus `slither::testutil`. Three things
//! S15 cares about are therefore **not** pinned here:
//!
//! * **The two drop counters (ruling 156).** `datagram_drops()` is
//!   `#[cfg(test)] pub(crate)` by `CONTRACT-6.md` §2.3, and §2.7 forbids a
//!   public accessor outright. No integration test can read them; the exact
//!   eviction arithmetic is core tests D1/D6's.
//! * **Frame identity.** `Tap` yields *sealed* packets. Nothing here can
//!   say "that packet held a DATAGRAM frame". Where a packet's *length* is
//!   a sound proxy it is used and labelled as a proxy — see [`MAX_DATAGRAM`]
//!   sized sends below — and never as a frame count.
//! * **A lost wakeup.** `LocalSet::run_until` re-polls its body on any
//!   local-task wake, so a future awaited from inside the [`local`] body is
//!   re-polled whether or not the shell woke its waker. [`sd7`] is a
//!   *cancel-safety* pin and explicitly **not** a wakeup pin.
//!
//! Full log: `.slices/06-sugar/TESTS-6-datagram.md`.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the observation instrument in both directions:
//! [`within`] asserts *it resolved*, [`quiet`] asserts *nothing more
//! arrived*. [`settle`] gives both drivers a turn without advancing virtual
//! time. There is no `sleep`.
//!
//! [`sd7`]: sd7_recv_datagram_is_cancel_safe
//! [`local`]: slither::testutil::local
//! [`MAX_DATAGRAM`]: slither::constants::MAX_DATAGRAM

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::SocketAddr;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use slither::constants::{
    DATAGRAM_SEND_QUEUE, INITIAL_MAX_DATA, MAX_DATAGRAM, MAX_DATAGRAM_PAYLOAD, NO_ERROR,
};
use slither::testutil::{FlakyPolicy, Pair, Tap, TestConnection, local, settle};
use slither::{ConnectionLost, DatagramError};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// `Pair` / `Peer` / `local` / `settle` / `Tap` are slice 0+3 harness and
// are used as shipped. The two datagram verbs are slice 6's and are called
// exactly as `CONTRACT-6.md` §2.5 spells them.
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve. Generous: on
/// the paused clock a resolvable future costs no wall time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Virtual-time budget for **"and then nothing more"**.
///
/// Long enough for several ACK rounds (`MAX_ACK_DELAY` is 25 ms) so a
/// still-draining send queue is not mistaken for silence, and far under
/// `DEAD_TIMEOUT` (25 s) — a budget past the death would let a test that
/// meant to observe silence observe a corpse instead.
const SILENCE: Duration = Duration::from_secs(2);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// Poll `fut` exactly once. The instrument for "**immediately**", which is
/// how `CONTRACT-6.md` §2.5's *"parking is never permitted on a dead
/// connection"* is observed: a `Poll::Pending` here **is** the park.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// Assert that no further datagram reaches `c` within [`SILENCE`].
///
/// The cancellation is deliberate and load-bearing: `CONTRACT-6.md` §2.5
/// makes `recv_datagram` cancel-safe *"because the claim and the return are
/// the same expression"*, so a timed-out claim must have claimed nothing.
/// [`sd7`](sd7_recv_datagram_is_cancel_safe) is the test that pins it; every
/// other use of this helper depends on it.
async fn quiet(c: &TestConnection, what: &str) {
    match tokio::time::timeout(SILENCE, c.recv_datagram()).await {
        Err(_) => {}
        Ok(Ok(extra)) => panic!(
            "{what}: an unexpected {}-byte datagram arrived (tag {:?})",
            extra.len(),
            (extra.len() >= 2).then(|| tag_of(&extra))
        ),
        Ok(Err(e)) => panic!("{what}: expected silence, got {e:?}"),
    }
}

/// Claim every datagram already queued on `c`, **without advancing the
/// clock and without parking**.
///
/// Stops at the first `Poll::Pending`, which the cancel-safety contract
/// makes free of charge.
async fn claim_ready(c: &TestConnection, out: &mut Vec<Vec<u8>>, what: &str) {
    loop {
        let mut fut = pin!(c.recv_datagram());
        match poll_once(fut.as_mut()).await {
            Poll::Ready(Ok(d)) => out.push(d),
            Poll::Ready(Err(e)) => panic!("{what}: connection lost mid-drain: {e:?}"),
            Poll::Pending => break,
        }
    }
}

/// Drain `c` until [`SILENCE`] passes with nothing more arriving.
///
/// `cap` is a suite-level guard, not an assertion about the protocol: a
/// build that duplicated its send queue for ever would otherwise hang the
/// suite instead of failing it.
async fn drain(c: &TestConnection, cap: usize, what: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        match tokio::time::timeout(SILENCE, c.recv_datagram()).await {
            Ok(Ok(d)) => {
                out.push(d);
                assert!(
                    out.len() <= cap,
                    "{what}: more than {cap} datagrams arrived — the send queue is \
                     bounded at {DATAGRAM_SEND_QUEUE} (§11.3) and datagrams are never \
                     retransmitted (§8.7)"
                );
            }
            Ok(Err(e)) => panic!("{what}: connection lost mid-drain: {e:?}"),
            Err(_) => break,
        }
    }
    out
}

/// A datagram that names itself.
///
/// The first two bytes are a big-endian tag; the body is a function of the
/// tag **and** the offset. 251 is prime and coprime with every packet size
/// in play, so a payload assembled from the wrong datagram, or shifted by
/// any amount, differs at almost every byte. A repeated-byte payload hides
/// both, and both are live here: the whole point of a bounded queue is that
/// some payloads are *supposed* to vanish, and a test that could not tell
/// *which* one arrived would pass on a build that delivered the wrong one.
fn tagged(tag: u16, len: usize) -> Vec<u8> {
    assert!(len >= 2, "a self-naming datagram needs its two tag bytes");
    let mut v = vec![0u8; len];
    v[..2].copy_from_slice(&tag.to_be_bytes());
    for (i, b) in v[2..].iter_mut().enumerate() {
        *b = ((i + usize::from(tag)) % 251) as u8;
    }
    v
}

/// The tag [`tagged`] wrote.
fn tag_of(d: &[u8]) -> u16 {
    assert!(d.len() >= 2, "a datagram shorter than its own tag: {d:?}");
    u16::from_be_bytes([d[0], d[1]])
}

/// Read the tag back and check the body against it.
///
/// Note what this does **not** catch: `tagged(t, n)` is a prefix of
/// `tagged(t, m)` for `n < m`, so truncation passes here by construction.
/// Every test that cares about truncation asserts the length explicitly —
/// see [`sd4`](sd4_oversize_is_an_error_not_a_truncation).
fn assert_tagged(d: &[u8], what: &str) -> u16 {
    let tag = tag_of(d);
    let want = tagged(tag, d.len());
    if let Some(i) = d.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: datagram tagged {tag} has a foreign body — first differing byte \
             at offset {i}: got {:#04x}, want {:#04x}",
            d[i], want[i]
        );
    }
    tag
}

/// Compare without dumping a kilobyte into the panic message.
///
/// Length first, content second: truncation and substitution are the two
/// failures a single `assert_eq!` would report identically, and §11.4's
/// *"not a silent truncation"* is exactly the first of them.
fn assert_same_bytes(got: &[u8], want: &[u8], what: &str) {
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: byte count differs — a build that truncated an oversize payload \
         to MAX_DATAGRAM_PAYLOAD and sent it anyway is short by exactly the \
         overshoot"
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
/// **above** the loss draw, so with neither a partition nor an injected
/// failure active on `who` this is exactly that wire's 0-based send index —
/// the number `drop_at` indices are counted in. Carried verbatim from
/// `story_reliability.rs:155-164`.
fn sent_from(tap: &Tap, who: SocketAddr) -> usize {
    tap.snapshot().iter().filter(|s| s.src == who).count()
}

/// The lengths of `who`'s sends at 0-based indices `range`.
fn send_sizes(tap: &Tap, who: SocketAddr, range: std::ops::Range<usize>) -> Vec<usize> {
    tap.snapshot()
        .iter()
        .filter(|s| s.src == who)
        .skip(range.start)
        .take(range.len())
        .map(|s| s.bytes.len())
        .collect()
}

/// How many of `who`'s sends were exactly [`MAX_DATAGRAM`] bytes.
///
/// **A proxy, and only a proxy.** A `MAX_DATAGRAM_PAYLOAD`-byte datagram
/// occupies a packet of exactly `DATA_HEADER_LEN (14) + 1 type byte + 1169 +
/// AEAD_TAG_LEN (16)` = 1200 = `MAX_DATAGRAM` — that arithmetic *is* §11.2's
/// definition of the constant, and the `0x31` form of the same payload would
/// need 1201 and cannot exist. So in a test that opens no stream, a
/// full-`MAX_DATAGRAM` send from a peer is a maximum-size datagram packet.
/// It is **not** a frame count: `Tap` sees sealed bytes and this file never
/// claims otherwise. A build that padded every packet to the MTU would make
/// this proxy vacuous, which is why nothing rests on it alone.
fn full_sized_sends(tap: &Tap, who: SocketAddr) -> usize {
    tap.snapshot()
        .iter()
        .filter(|s| s.src == who && s.bytes.len() == MAX_DATAGRAM)
        .count()
}

// ══════════════════════════════════════════════════════════════════════
// 1. `send_datagram` never waits
// ══════════════════════════════════════════════════════════════════════

/// **S15 — `send_datagram` never waits, and pressure does not change that.**
///
/// > `send_datagram` never waits — it drops oldest under pressure rather
/// > than blocking.
///
/// Two pins, one of which is the compiler's:
///
/// * **It is a `fn`, not an `async fn`** (`CONTRACT-6.md` §2.5). The loop
///   below has no `.await` in it. A build that made the verb `async`, or
///   that returned a future, does not compile against this file at all —
///   the loudest failure available.
/// * **No driver turn happens across 200 calls.** The shell is a
///   single `!Send` actor on a current-thread runtime (§16.3): the driver
///   task can only run at an await point, so if `Network::sends()` is
///   unchanged across the burst, nothing in the burst yielded. This is the
///   assertion that separates "synchronous signature" from "synchronous
///   behaviour" — a signature alone would still admit a verb that drove the
///   wire inline.
///
/// The burst is more than three times `DATAGRAM_SEND_QUEUE` and the path is
/// a blackhole, so the send queue is the only place any of it can go and it
/// certainly overflows. **Every call still returns `Ok(())`** — §11.3's
/// *"the newly-sent datagram always enters"* and `CONTRACT-6.md` §2.1's
/// *"not representable: rejecting the newest"*.
///
/// BROKEN BUILD: a shell that returned an error, or parked, once the send
/// queue reached 64 — the natural implementation if you reach for a bounded
/// channel — fails on the 65th call. So does one that resolved the pressure
/// by blocking. A build that pumped the wire inside the verb moves
/// `Network::sends()` and fails the second assertion.
///
/// NOT PINNED HERE: *which* 64 survive. That needs the drop counters, which
/// §2.7 keeps out of the public API; core test D1 owns it.
#[tokio::test(start_paused = true)]
async fn sd1_send_datagram_never_waits_under_pressure() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0001);
        let (ca, _cb) = pair.establish().await;
        settle().await;

        // Nothing can leave A. The send queue is the only sink.
        pair.net.block_path(pair.a.addr(), pair.b.addr());

        const BURST: usize = 200;
        const {
            assert!(
                BURST > 3 * DATAGRAM_SEND_QUEUE,
                "the burst must overflow the send queue several times over"
            );
        }

        let sends_before = pair.net.sends();
        for i in 0..BURST {
            // No `.await` in this loop. That is the test.
            assert_eq!(
                ca.send_datagram(&tagged(i as u16, 64)),
                Ok(()),
                "S15/§11.3: call {i} of {BURST} — `send_datagram` drops oldest under \
                 pressure, it never refuses the newest and never blocks"
            );
        }
        assert_eq!(
            pair.net.sends(),
            sends_before,
            "§16.3: the driver is a task on this same current-thread runtime, so a \
             burst with no `.await` in it cannot have let the driver run — a change \
             here means `send_datagram` yielded or drove the wire inline"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 2. the size bound, and no silent truncation
// ══════════════════════════════════════════════════════════════════════

/// **S15 — the size bound is checked at the handle, from both sides.**
///
/// > Oversize input is `DatagramError::TooLarge`.
///
/// §11.4: *"a payload > `MAX_DATAGRAM_PAYLOAD` returns
/// `DatagramError::TooLarge` at the handle, before any queue"*. Three
/// adjacent sizes, because slice 1's lesson was a one-sided boundary —
/// `LEN` and `LEN-1` tested, `LEN+1` not:
///
/// | payload | verdict |
/// |---|---|
/// | 1168 | `Ok(())` |
/// | 1169 = `MAX_DATAGRAM_PAYLOAD` | `Ok(())` |
/// | 1170 = `MAX_PLAINTEXT` | `Err(TooLarge)` |
///
/// BROKEN BUILD: `>=` where §11.4 writes `>` rejects the ratified maximum
/// and fails row 2 — and that build is *invisible* to a test that only
/// checks 1168 and 1170, because the rejected size is exactly the one
/// ruling 155 says is hardest to send. A bound written against
/// `MAX_PLAINTEXT` (1170) instead of `MAX_DATAGRAM_PAYLOAD` (1169) accepts
/// row 3 and fails there. The far-oversize row catches a bound that
/// compares against the packet MTU instead.
#[tokio::test(start_paused = true)]
async fn sd2_the_size_bound_is_two_sided_at_the_handle() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0002);
        let (ca, _cb) = pair.establish().await;
        settle().await;

        assert_eq!(
            ca.send_datagram(&tagged(1, MAX_DATAGRAM_PAYLOAD - 1)),
            Ok(()),
            "§11.4: MAX_DATAGRAM_PAYLOAD - 1 is under the bound"
        );
        assert_eq!(
            ca.send_datagram(&tagged(2, MAX_DATAGRAM_PAYLOAD)),
            Ok(()),
            "§11.2/§11.4: MAX_DATAGRAM_PAYLOAD itself is the ratified maximum, not \
             one past it — ruling 155 exists because this is the size a `0x31`-only \
             build cannot carry"
        );
        assert_eq!(
            ca.send_datagram(&tagged(3, MAX_DATAGRAM_PAYLOAD + 1)),
            Err(DatagramError::TooLarge),
            "§11.4: one byte past the bound is TooLarge"
        );
        assert_eq!(
            ca.send_datagram(&tagged(4, MAX_DATAGRAM)),
            Err(DatagramError::TooLarge),
            "§11.4: the bound is MAX_DATAGRAM_PAYLOAD (1169), not the MTU (1200)"
        );
    })
    .await;
}

/// **S15 — a maximum-size datagram actually crosses, whole.**
///
/// This is ruling 155's integration pin. `MAX_DATAGRAM_PAYLOAD` (1169) plus
/// a type byte plus a length varint exceeds `MAX_PLAINTEXT` (1170), so the
/// `0x30` extends-to-end form is **mandatory, not an optimisation**: a
/// 1169-byte datagram in the `0x31` form would need 1172 bytes of plaintext
/// and cannot exist.
///
/// BROKEN BUILD: one that emits only `0x31`. It accepts the payload —
/// `send_datagram` returns `Ok(())`, because §11.4's bound is the only
/// admission check — and then **never sends it**: the datagram does not fit
/// any packet, sits at the head of the send queue, and is eventually
/// evicted. The failure is completely silent on the wire and completely
/// silent in the API. Here it is a loud timeout in `within`, which is the
/// entire reason this test exists rather than resting on
/// [`sd2`](sd2_the_size_bound_is_two_sided_at_the_handle)'s `Ok(())`.
///
/// The packet-size assertion is corroboration, not the pin — see
/// [`full_sized_sends`] on why a length is only a proxy for a frame.
///
/// DELIVERY DEPENDENCE: asserted over `FlakyPolicy::perfect()` with no loss
/// injected anywhere. Permitted by `PLAN-6.md` §5.0(b).
#[tokio::test(start_paused = true)]
async fn sd3_a_maximum_size_datagram_crosses_intact() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0003);
        let (ca, cb) = pair.establish().await;
        settle().await;

        let want = tagged(0x1169, MAX_DATAGRAM_PAYLOAD);
        assert_eq!(ca.send_datagram(&want), Ok(()));

        let got = within(cb.recv_datagram(), "the 1169-byte datagram")
            .await
            .expect(
                "ruling 155: a MAX_DATAGRAM_PAYLOAD datagram must be sendable — a \
                     build without the `0x30` extends-to-end form cannot send one at \
                     all, and fails here by never delivering it",
            );
        assert_same_bytes(&got, &want, "the maximum-size datagram");

        assert!(
            full_sized_sends(&pair.net.tap(), pair.a.addr()) >= 1,
            "§11.2: 14 + 1 + {MAX_DATAGRAM_PAYLOAD} + 16 = {MAX_DATAGRAM}, so the \
             packet carrying it is exactly MAX_DATAGRAM bytes; no such send was \
             observed"
        );
    })
    .await;
}

/// **S15 — oversize is an error, *not* a silent truncation.**
///
/// The story's second clause names the failure it is guarding against by
/// name, so the test has to be able to tell the two apart on the wire:
///
/// 1. a 1170-byte poison payload of `0xAA` is refused with `TooLarge`;
/// 2. a *different* 1169-byte payload is then sent and arrives **exactly**,
///    byte for byte and length for length;
/// 3. and nothing else arrives at all.
///
/// Step 1 alone is satisfied by a build that returns `Err` **and** sends a
/// truncated copy anyway; steps 2 and 3 are what separate it. Step 2 alone
/// is satisfied by a build that rejects nothing; step 1 separates that.
///
/// BROKEN BUILD: a shell that clamps with `&data[..MAX_DATAGRAM_PAYLOAD]`
/// before queueing and reports `Err` afterwards delivers 1169 bytes of
/// `0xAA` **first** (the send queue is FIFO), so step 2's
/// [`assert_same_bytes`] fails on content, not merely on count. A build
/// that queues the oversize payload and rejects it afterwards — the
/// ordering §11.4's *"before any queue"* forbids — is caught by step 3.
///
/// NOT PINNED HERE: that the rejection left the *queue* untouched, which
/// needs `datagram_drops()`. Core test D2 owns it.
#[tokio::test(start_paused = true)]
async fn sd4_oversize_is_an_error_not_a_truncation() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0004);
        let (ca, cb) = pair.establish().await;
        settle().await;

        let poison = vec![0xAAu8; MAX_DATAGRAM_PAYLOAD + 1];
        assert_eq!(
            ca.send_datagram(&poison),
            Err(DatagramError::TooLarge),
            "§11.4: oversize is refused at the handle"
        );

        let marker = tagged(0x5A5A, MAX_DATAGRAM_PAYLOAD);
        assert_eq!(ca.send_datagram(&marker), Ok(()));

        let got = within(cb.recv_datagram(), "the marker datagram")
            .await
            .expect("the connection is alive");
        assert_same_bytes(
            &got,
            &marker,
            "S15: the first datagram to arrive must be the marker — a 1169-byte run \
             of 0xAA here is the truncated poison payload, which is exactly the \
             'silent truncation' S15 forbids",
        );

        quiet(&cb, "S15: the refused payload must not appear in any form").await;
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 3. drop-oldest under pressure
// ══════════════════════════════════════════════════════════════════════

/// **S15 — under pressure the queue sheds the oldest, and the newest still
/// gets out.**
///
/// The pressure is built without a fragile fixture, by exploiting the fact
/// [`sd1`](sd1_send_datagram_never_waits_under_pressure) pins: a burst with
/// no `.await` in it cannot let the driver run, and §14.5's admission gate
/// cannot open, because an ACK can only arrive on a driver turn. So the
/// congestion window is at most `INITIAL_WINDOW` (12 000 B) for the whole
/// burst: **at most ten** of these 1200-byte packets can be admitted, and
/// the remaining 190 datagrams meet a 64-slot queue. Delivery of more than
/// ~74 of the 200 is arithmetically impossible for a conforming build,
/// which is what makes the *"fewer than all"* assertion a pin rather than a
/// hope.
///
/// Three assertions, chosen to separate both degenerate directions:
///
/// * **Fewer than 200 arrive.** An *unbounded* send queue delivers all 200.
/// * **The last one arrives.** A *reject-newest* queue — "full, try later" —
///   delivers a prefix and never the tail. This is the assertion that
///   working rule 9 asks for: the two broken builds fail *different*
///   assertions, and a one-sided "at most 64 arrived" would separate
///   neither cleanly.
/// * **No tag arrives twice, and every payload is 1169 bytes with a body
///   matching its own tag.** A build that re-queued a datagram after
///   sending it, or that mixed payloads while shuffling a `VecDeque`,
///   fails here.
///
/// NOT ASSERTED: *which* datagrams survive, and the arrival **order**.
/// §11.1 promises no ordering, so an order assertion could fail a
/// conforming build — a flake, not a pin. The exact eviction boundary
/// (first survivor, last survivor, `drops.send`) is core test D1's, because
/// it needs the counters `CONTRACT-6.md` §2.7 keeps private.
///
/// DELIVERY DEPENDENCE: `FlakyPolicy::perfect()`, no loss injected. What is
/// asserted to arrive is asserted only because nothing on this fixture can
/// destroy it once sealed.
#[tokio::test(start_paused = true)]
async fn sd5_drop_oldest_under_pressure_still_delivers_the_newest() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0005);
        let (ca, cb) = pair.establish().await;
        settle().await;

        const BURST: usize = 200;
        for i in 0..BURST {
            assert_eq!(
                ca.send_datagram(&tagged(i as u16, MAX_DATAGRAM_PAYLOAD)),
                Ok(()),
                "§11.3: every send is accepted, however full the queue is"
            );
        }

        let got = drain(&cb, BURST, "the 200-datagram burst").await;

        assert!(
            !got.is_empty(),
            "some of the burst must cross: nothing here is lossy"
        );
        assert!(
            got.len() < BURST,
            "§11.3: the send queue holds {DATAGRAM_SEND_QUEUE} and §14.5 admitted at \
             most INITIAL_WINDOW of the burst before the first driver turn, so all \
             {BURST} arriving means the queue is unbounded — got {}",
            got.len()
        );

        let mut tags: Vec<u16> = Vec::with_capacity(got.len());
        for d in &got {
            assert_eq!(
                d.len(),
                MAX_DATAGRAM_PAYLOAD,
                "every datagram in this burst was sent whole"
            );
            tags.push(assert_tagged(d, "a datagram of the burst"));
        }

        assert!(
            tags.contains(&((BURST - 1) as u16)),
            "§11.3: *'the newly-sent datagram always enters'* — the last datagram of \
             the burst is the one a reject-newest queue throws away, and it is \
             missing from the {} that arrived",
            got.len()
        );

        let mut unique = tags.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            tags.len(),
            "§8.7 puts DATAGRAM in the `never` class: no datagram is sent twice"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 4. unreliable and unordered by contract
// ══════════════════════════════════════════════════════════════════════

/// **S15 — a lost datagram is never retransmitted, and never blocks the
/// ones behind it.**
///
/// §11.1: *"no delivery promise, no ordering promise, no retransmission, no
/// sequence identity at all"*. §8.7 puts DATAGRAM in the `never` class:
/// *"loss is absorbed by the application"*.
///
/// The loss is **counter-proved**, per ruling 148. `FlakyPolicy::lossy` is
/// invisible to every public counter — the tap is written above the loss
/// draw — so a rate-based test could not show that any datagram died at
/// all. Instead: `drop_at` over a window anchored at A's current send index
/// (no RNG draw), and then three checks *before* anything is concluded:
///
/// * A actually reached the window (`sent_from` advanced);
/// * one of the dropped sends was `MAX_DATAGRAM` bytes, i.e. the maximum-size
///   datagram's own packet (see [`full_sized_sends`] on the proxy);
/// * A did **not** send past the window, so nothing escaped it.
///
/// Only then does the test conclude anything from tag 2's absence. Without
/// those three, "tag 2 never arrived" would be equally explained by "tag 2
/// was never sent", and the test would pin nothing.
///
/// Two properties fall out of one fixture:
///
/// * **No retransmission.** Eight seconds of virtual time is driven after
///   the loss — enough for the un-sampled PTO (≈1 024 ms) to fire and
///   double three times over (§13.3) — and tag 2 must still never appear.
/// * **No head-of-line blocking.** Tags 3 and 4 are sent *after* the lost
///   one and must arrive anyway. A build that gave datagrams a sequence
///   number and reassembled them in order — the single most natural way to
///   get "unordered" wrong — would hold 3 and 4 behind the gap for ever.
///
/// BROKEN BUILD: one that minted a `SentFrame::Datagram` and re-queued the
/// payload on loss (the prohibition `CONTRACT-6.md` §2.7 states and this
/// test makes real) delivers tag 2 after the first PTO. One that ordered
/// datagrams behind a reassembler delivers neither 3 nor 4.
///
/// DELIVERY DEPENDENCE: tags 0, 1, 3 and 4 are asserted to arrive over a
/// path that is `perfect()` at the moment they cross; the injected drop is
/// confined to a four-index window that is proved closed before they are
/// sent. No arrival is asserted across an active loss.
#[tokio::test(start_paused = true)]
async fn sd6_a_lost_datagram_is_never_retransmitted_and_never_blocks() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0006);
        let (ca, cb) = pair.establish().await;
        settle().await;
        let tap = pair.net.tap();

        // Two that cross cleanly, so the fixture is known good.
        for tag in [0u16, 1] {
            assert_eq!(ca.send_datagram(&tagged(tag, MAX_DATAGRAM_PAYLOAD)), Ok(()));
        }
        settle().await;
        let mut got: Vec<Vec<u8>> = Vec::new();
        claim_ready(&cb, &mut got, "the two clean datagrams").await;
        assert_eq!(got.len(), 2, "the fixture delivers over a perfect path");

        // Arm an index window on A's wire, anchored at its current index.
        const WINDOW: usize = 4;
        let base = sent_from(&tap, pair.a.addr());
        pair.a
            .wire
            .set_policy(FlakyPolicy::drop_at(base..base + WINDOW));

        assert_eq!(ca.send_datagram(&tagged(2, MAX_DATAGRAM_PAYLOAD)), Ok(()));
        settle().await;

        let after = sent_from(&tap, pair.a.addr());
        assert!(
            after > base,
            "the drop window was never reached: A made no send at index {base}, so \
             nothing was destroyed and the rest of this test would prove nothing"
        );
        assert!(
            after <= base + WINDOW,
            "A sent {} packets while the {WINDOW}-index window was armed, so a send \
             escaped it and tag 2's absence below would be unproven",
            after - base
        );
        let dropped = send_sizes(&tap, pair.a.addr(), base..after);
        assert!(
            dropped.contains(&MAX_DATAGRAM),
            "no MAX_DATAGRAM-sized send fell inside the drop window ({dropped:?}); \
             tag 2's packet was not the packet that died"
        );

        pair.a.wire.set_policy(FlakyPolicy::perfect());

        // Two more, sent after the gap. An in-order build stalls here.
        for tag in [3u16, 4] {
            assert_eq!(ca.send_datagram(&tagged(tag, MAX_DATAGRAM_PAYLOAD)), Ok(()));
        }

        // Drive well past three PTOs. A retransmitting build shows tag 2 here.
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(2)).await;
            settle().await;
            claim_ready(&cb, &mut got, "after the loss").await;
        }

        let mut tags: Vec<u16> = got.iter().map(|d| assert_tagged(d, "arrived")).collect();
        tags.sort_unstable();
        assert_eq!(
            tags,
            vec![0, 1, 3, 4],
            "§8.7/§11.1: tag 2 died on the wire and is never retransmitted; tags 3 \
             and 4 were sent after it and must arrive regardless, because datagrams \
             carry no sequence identity and nothing can be head-of-line blocked \
             behind a gap"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 5. `recv_datagram` — cancellation, and the post-death drain
// ══════════════════════════════════════════════════════════════════════

/// **`recv_datagram` is cancel-safe** (`CONTRACT-6.md` §2.5).
///
/// The dangerous window is not "drop an untouched future" — it is: park,
/// let a datagram arrive *while the future is not being polled*, then drop
/// the future. A build that moved the payload out of the queue on the wake
/// path, into the future or into a per-waker slot, loses it precisely here.
///
/// BROKEN BUILD: a `poll_recv_datagram` that claimed into the `WakerSlot`
/// on wake and dropped the claim with the slot. The datagram is gone and the
/// second call parks for ever, so `within` fails.
///
/// **STATED LIMIT — this is not a wakeup test.** `LocalSet::run_until`
/// re-polls its body on any local-task wake, so the final
/// `recv_datagram()` below would resolve even if the shell registered no
/// waker at all. Whether a *parked* reader is woken by
/// `ConnEvent::DatagramReadable` is not observable from inside the `local()`
/// body, and this file does not claim to pin it.
#[tokio::test(start_paused = true)]
async fn sd7_recv_datagram_is_cancel_safe() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0007);
        let (ca, cb) = pair.establish().await;
        settle().await;

        let want = tagged(11, 512);
        {
            let mut parked = pin!(cb.recv_datagram());
            assert!(
                poll_once(parked.as_mut()).await.is_pending(),
                "nothing has been sent yet, so the claim must park"
            );
            assert_eq!(ca.send_datagram(&want), Ok(()));
            // The datagram arrives here. `parked` is not polled again —
            // only the `local()` body is, and it resumes inside `settle`.
            settle().await;
        } // cancelled *after* the wake: the dangerous drop.

        let got = within(cb.recv_datagram(), "the datagram after a cancelled claim")
            .await
            .expect("the connection is alive");
        assert_same_bytes(
            &got,
            &want,
            "CONTRACT-6 §2.5: a dropped `recv_datagram` future has claimed nothing",
        );
    })
    .await;
}

/// **Ruling 152 — the post-death drain reaches `recv_datagram`, and
/// parking is never permitted on a dead connection.**
///
/// `CONTRACT-6.md` §2.5's precedence, in order: the **core call first**,
/// then the death latch *only if the core had nothing*, and **never** a
/// park. All three rows are asserted, and every one of them with
/// [`poll_once`] rather than an `await`, because "immediately" is the whole
/// claim — an `await` would pass on a build that parked and was later woken
/// by the LocalSet's re-poll.
///
/// BROKEN BUILD: the one `CONTRACT-6.md` §2.5 predicts by name — an author
/// reading §16.2's post-death paragraph, which lists only `read` and
/// `accept_*`, checks the death latch first and returns
/// `Err(ConnectionLost)` with three delivered datagrams still sitting in
/// the queue. It fails on the very first claim. A build that parks on a
/// dead connection (ruling 128's defect) returns `Pending` and fails on
/// whichever row it reaches first.
///
/// The datagrams are sent and settled *before* the close, so they travel in
/// their own packets: §8.5 packs control frames ahead of the DATAGRAM fill,
/// so a coalesced CLOSE would sit ahead of a datagram in the same packet
/// and this test would be measuring frame-application order instead of the
/// drain.
#[tokio::test(start_paused = true)]
async fn sd8_recv_datagram_drains_after_death_and_never_parks() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0008);
        let (ca, cb) = pair.establish().await;
        settle().await;

        let sent: Vec<Vec<u8>> = (0u16..3).map(|t| tagged(t, 96)).collect();
        for d in &sent {
            assert_eq!(ca.send_datagram(d), Ok(()));
        }
        settle().await;

        ca.close(NO_ERROR, b"bye").await;
        settle().await;

        // Prove the connection is dead *before* draining it, so the drain
        // below is provably a post-death drain and not a lucky race.
        let lost = {
            let mut c = pin!(cb.closed());
            match poll_once(c.as_mut()).await {
                Poll::Ready(l) => l,
                Poll::Pending => panic!("B has not observed the peer's CLOSE yet"),
            }
        };
        assert!(
            matches!(lost, ConnectionLost::PeerClosed { .. }),
            "expected PeerClosed, got {lost:?}"
        );

        for (i, want) in sent.iter().enumerate() {
            let mut f = pin!(cb.recv_datagram());
            match poll_once(f.as_mut()).await {
                Poll::Ready(Ok(got)) => {
                    assert_same_bytes(&got, want, &format!("post-death datagram {i}"));
                }
                Poll::Ready(Err(e)) => panic!(
                    "ruling 152: the post-death drain covers `recv_datagram` — \
                     datagram {i} of {} was delivered before the death and must \
                     still be claimable, got {e:?}",
                    sent.len()
                ),
                Poll::Pending => {
                    panic!("ruling 128/152: parking is never permitted on a dead connection")
                }
            }
        }

        let mut f = pin!(cb.recv_datagram());
        match poll_once(f.as_mut()).await {
            Poll::Ready(Err(e)) => assert!(
                matches!(e, ConnectionLost::PeerClosed { .. }),
                "the drained-dry connection reports its death, got {e:?}"
            ),
            Poll::Ready(Ok(extra)) => {
                panic!("a fourth datagram of {} bytes appeared", extra.len())
            }
            Poll::Pending => panic!(
                "ruling 128/152: with the queue drained and the connection dead, the \
                 claim must resolve with the death — parking is never permitted"
            ),
        }
    })
    .await;
}

/// **`send_datagram` on a dead connection is an error, not a panic and not
/// a silent success** (`CONTRACT-6.md` §2.5).
///
/// Both directions of death are covered: A closed locally, B observed the
/// peer's CLOSE.
///
/// BROKEN BUILD: one that queues into a core that will never pump again
/// returns `Ok(())` and fails here — a datagram accepted after the
/// connection ended is a delivery promise the protocol cannot keep, and
/// §11.1's "no delivery promise" does not extend to accepting sends on a
/// corpse.
///
/// ⚠ **CONFLICT, reported not resolved.** The last case below is the
/// *oversize payload on a dead connection*, and the contract answers it
/// twice, differently:
///
/// * `CONTRACT-6.md` §2.3's core table (line 171) says the lost check is
///   *"checked **first**, mirroring `write`"*, which makes the answer
///   `ConnectionLost`;
/// * §2.5's shell table (lines 254-259) is an unordered list of rows whose
///   guards overlap, while the *adjacent* `poll_send_message` precedence
///   (lines 278-287) puts `TooLarge` at step 1 and the death latch at step
///   2 — which makes the answer `TooLarge`.
///
/// The test therefore accepts **either** and asserts only what both
/// readings agree on. Writing it to one side would be an assertion a
/// conforming build could fail. See `TESTS-6-datagram.md` §6.
#[tokio::test(start_paused = true)]
async fn sd9_send_datagram_after_death_is_connection_lost() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0009);
        let (ca, cb) = pair.establish().await;
        settle().await;

        ca.close(NO_ERROR, b"bye").await;
        settle().await;

        match ca.send_datagram(&tagged(0, 64)) {
            Err(DatagramError::ConnectionLost(_)) => {}
            other => panic!(
                "CONTRACT-6 §2.5: a locally closed connection refuses a datagram with \
                 ConnectionLost, got {other:?}"
            ),
        }
        match cb.send_datagram(&tagged(1, 64)) {
            Err(DatagramError::ConnectionLost(ConnectionLost::PeerClosed { .. })) => {}
            other => panic!(
                "CONTRACT-6 §2.5: a connection that observed the peer's CLOSE refuses \
                 a datagram with ConnectionLost(PeerClosed), got {other:?}"
            ),
        }

        // The overlapping-guard case. Either answer is conforming today.
        let both = ca.send_datagram(&vec![0u8; MAX_DATAGRAM_PAYLOAD + 1]);
        assert!(
            matches!(
                both,
                Err(DatagramError::TooLarge) | Err(DatagramError::ConnectionLost(_))
            ),
            "an oversize payload on a dead connection must be an error of one of the \
             two kinds the contract names; CONTRACT-6 §2.3 and §2.5 disagree on \
             which, and this test deliberately does not pick — got {both:?}"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// 6. the exemptions and the degenerate payload
// ══════════════════════════════════════════════════════════════════════

/// **§10.7 — datagrams are flow-control exempt, and a stream still works
/// after a megabyte of them.**
///
/// §11.1: *"flow-control-exempt: they consume no MAX_DATA credit
/// (§10.7)"*. `INITIAL_MAX_DATA` is 1 MiB, so a build that charged datagram
/// bytes against connection credit wedges after roughly 900 maximum-size
/// datagrams — **with no error anywhere**, which is the silent wedge §10.7
/// exists to prevent.
///
/// The precondition is measured from the tap, not from the receiver, so it
/// does not depend on delivery at all: a `MAX_DATAGRAM`-sized send carries
/// `MAX_DATAGRAM_PAYLOAD` datagram bytes (see [`full_sized_sends`]), and the
/// loop runs until A has *sealed* more than `INITIAL_MAX_DATA` of them.
///
/// BROKEN BUILD, caught twice over:
///
/// * one that charges datagram bytes against **send** credit stalls before
///   sealing 1 MiB, so the loop exhausts its rounds and fails with that
///   message — this is the primary detection and it never reaches the
///   stream at all;
/// * one that charges them on the **receive** side stops granting MAX_DATA,
///   so the stream write afterwards stalls and `within` fails.
///
/// A build that is correct passes both with room to spare, because the
/// datagram bytes are simply not in either ledger.
#[tokio::test(start_paused = true)]
async fn sd10_datagrams_are_flow_control_exempt() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0010);
        let (ca, cb) = pair.establish().await;
        settle().await;
        let tap = pair.net.tap();

        const BATCH: usize = 64;
        const ROUNDS: usize = 60;
        let mut tag = 0u16;
        let mut rounds = 0usize;
        let sealed_bytes =
            |tap: &Tap| full_sized_sends(tap, pair.a.addr()) as u64 * MAX_DATAGRAM_PAYLOAD as u64;

        while sealed_bytes(&tap) <= INITIAL_MAX_DATA {
            assert!(
                rounds < ROUNDS,
                "§10.7: after {} rounds of {BATCH} maximum-size datagrams A has sealed \
                 only {} bytes of datagram payload, still short of INITIAL_MAX_DATA \
                 ({INITIAL_MAX_DATA}). A build that charges datagrams against \
                 connection credit stalls exactly here, silently — which is the wedge \
                 §10.7 exists to prevent.",
                rounds,
                sealed_bytes(&tap)
            );
            for _ in 0..BATCH {
                assert_eq!(ca.send_datagram(&tagged(tag, MAX_DATAGRAM_PAYLOAD)), Ok(()));
                tag = tag.wrapping_add(1);
            }
            settle().await;
            // Let the ACK timers fire so the congestion window reopens;
            // §14.5 is a real gate and this test is not trying to dodge it.
            tokio::time::advance(Duration::from_millis(50)).await;
            settle().await;
            // Keep B's receive queue moving. Nothing is asserted about it:
            // evictions here are permitted and uninteresting.
            let mut sink = Vec::new();
            claim_ready(&cb, &mut sink, "keeping B's queue moving").await;
            rounds += 1;
        }

        // A stream, after all that, must still be able to move bytes.
        const STREAM_BYTES: usize = 64 * 1024;
        let want = tagged(0xBEEF, STREAM_BYTES);
        let writer = async {
            let mut s = ca
                .open_uni()
                .await
                .expect("open_uni after the datagram flood");
            let mut done = 0usize;
            while done < want.len() {
                let n = within(s.write(&want[done..]), "the post-flood stream write")
                    .await
                    .expect("the stream write");
                assert!(n >= 1, "§16.2: a blocked write is Pending, not Ok(0)");
                done += n;
            }
            within(s.finish(), "the post-flood finish")
                .await
                .expect("finish");
        };
        let reader = async {
            let mut r = cb
                .accept_uni()
                .await
                .expect("accept_uni after the datagram flood");
            let mut out = Vec::with_capacity(STREAM_BYTES);
            let mut buf = vec![0u8; 4096];
            while let Some(n) = within(r.read(&mut buf), "the post-flood stream read")
                .await
                .expect("the stream read")
            {
                out.extend_from_slice(&buf[..n]);
            }
            out
        };
        let (_, got) = tokio::join!(writer, reader);
        assert_same_bytes(
            &got,
            &want,
            "§10.7: the stream's own credit was never spent on datagram bytes",
        );
    })
    .await;
}

/// **A zero-length datagram is a datagram.**
///
/// `CONTRACT-6.md` §2.3 states this row explicitly *"so nobody invents"* a
/// minimum: §8.4 admits `0x31` with `length = 0` and §11 states no lower
/// bound. It is the degenerate payload, and the two ways to get it wrong
/// are both silent.
///
/// BROKEN BUILD: one that treats an empty payload as a no-op returns
/// `Ok(())` and delivers nothing, so the first `within` fails. One that
/// collapses "empty datagram" onto the core's `None` — the same inversion
/// that hangs a reader on a finished stream — parks the receiver for ever,
/// and fails the same way. The second datagram is sent afterwards precisely
/// to separate *"the empty one was dropped"* from *"the queue was wedged by
/// it"*: a build that wedges fails on both claims, a build that silently
/// discards fails on the first only.
#[tokio::test(start_paused = true)]
async fn sd11_a_zero_length_datagram_is_a_datagram() {
    local(async {
        let pair = Pair::seeded(0x0D9A_0011);
        let (ca, cb) = pair.establish().await;
        settle().await;

        assert_eq!(
            ca.send_datagram(&[]),
            Ok(()),
            "§11 sets no minimum; §8.4 admits `0x31` with length 0"
        );
        let first = within(cb.recv_datagram(), "the empty datagram")
            .await
            .expect("the connection is alive");
        assert!(
            first.is_empty(),
            "an empty datagram must arrive empty, got {} bytes",
            first.len()
        );

        let second = tagged(1, 128);
        assert_eq!(ca.send_datagram(&second), Ok(()));
        let got = within(cb.recv_datagram(), "the datagram after the empty one")
            .await
            .expect("the connection is alive");
        assert_same_bytes(
            &got,
            &second,
            "the empty datagram must not wedge the queue behind it",
        );
    })
    .await;
}

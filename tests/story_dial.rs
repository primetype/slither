//! **S2 — a user can dial a peer that never answers, and be told.**
//! **S34 — a responder that keeps accepting survives a lost msg2.**
//!
//! S34 (`STORIES.md` §J, ruling 252) is appended below the S2 block; it has
//! its own fixture header and its own authorship note. Everything above the
//! `S34` banner is slice 3's file, untouched.
//!
//! `STORIES.md` §A, S2:
//!
//! > **Accepts:** with no response, `Connecting` resolves
//! > `Err(ConnectError::TimedOut)` at `HANDSHAKE_GIVEUP` (90 s), **not
//! > before and not never**. Retransmissions during the window follow
//! > §5.5's handshake schedule — a **fixed 5 s interval plus jitter,
//! > explicitly *not* exponential backoff**.
//! > **Anchor:** §16.1, §5.5. **Paused clock:** yes — this is a 90 s test
//! > that runs instantly.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by **TEST-B**, from `STORIES.md`, `SPEC.md` §5.5 / §16.1–§16.3 /
//! §16.10 and `.slices/03-skeleton/PLAN.md` §5.2 + T12 alone. The shell was
//! being implemented **concurrently and independently**; no file under
//! `src/shell/` and no line of `src/testutil/mod.rs` was read. Every name
//! this file spells for a shell or harness item is therefore a **proposal**
//! (see `FIXTURE` below) — the integrator renames *calls*, never
//! assertions.
//!
//! # Not a sleep, ever (§16.10)
//!
//! Every test here is `#[tokio::test(start_paused = true)]` on a
//! `LocalSet`. The 90 s give-up, the 5 s retransmit train and the 30 s
//! silence window all resolve in virtual time. `tokio::time::timeout` is
//! used as the *observation* instrument rather than as a deadline: on the
//! paused clock it auto-advances to the next armed timer, so
//! `timeout(d, &mut fut).await.is_err()` is a precise assertion that `fut`
//! was still `Pending` at `now + d` — which is the "not before" half T12
//! says a one-sided test omits.

// The fixture below reaches for names the shell had not published when this
// file was written; see the module doc.
#![allow(clippy::items_after_statements)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use slither::config::Config;
use slither::constants::{
    HANDSHAKE_GIVEUP, INIT_PACKET_LEN, PKT_HANDSHAKE_INIT, RETRANSMIT_BASE, SHELL_LATENESS_BOUND,
};
use slither::error::ConnectError;
use slither::identity::{Identity, PublicKeyOf};
use slither::testutil::{CountingIdentity, DhCounter, Network, Tap};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE — proposed names, flagged loudly (see the module doc).
//
// `.slices/03-skeleton/PLAN.md` §2.2 assigns the two-endpoint `LocalSet`
// harness to IMPL-B and this file to TEST-B, and §2.2's own note says the
// briefs must agree on the harness API. They could not: it was being
// written as this file was. So the block below is a **local** fixture
// built from the three names §16.10 ratifies as contract (`Network`,
// `FlakyWire`, `FlakyPolicy`) plus the public `Endpoint::builder()`.
//
// INTEGRATOR: redirect this block at IMPL-B's harness if one fits.
// **Nothing below the `── tests ──` line depends on these names except
// through `Node` and its methods.** Every assertion is on protocol
// behaviour and must survive the rename untouched.
//
// Names proposed here and nowhere ratified:
//   * `slither::Endpoint` / `Connecting` at the crate root — if the shell
//     re-export lands under `shell`, this is `slither::shell::Endpoint`.
//   * `Endpoint::builder().identity(..).wire(..).config(..).build()` —
//     §16.2 ratifies `builder()` and §16.3 ratifies that the application
//     supplies the `Wire` "through `Endpoint::builder()`"; the setter
//     names are this author's.
//   * `Network::new()` and `Network::wire(addr) -> FlakyWire`.
//   * `Network::tap() -> Tap` and `Tap::datagrams() -> Vec<(from, to,
//     bytes)>` — §16.10 ratifies `Network` as the routing fabric and the
//     brief states a `Tap` exists; its shape is this author's.
//   * `CountingIdentity::seeded([u8; 32])` / `.counter()` — taken from
//     `src/core/tests.rs`, which already uses exactly these.
// ══════════════════════════════════════════════════════════════════════

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::shell::Endpoint<Id>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

/// One endpoint plus the bookkeeping a story test needs about it.
struct Node {
    ep: Option<Endpoint>,
    /// §6.1's cumulative DH ladder, endpoint-wide. S1's `Cost:` line is
    /// part of the story, and `DhCounter` is the fixture that prices it.
    dhs: DhCounter,
    pk: Pk,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet`: §16.3 spawns the driver with
    /// `tokio::task::spawn_local`.
    fn spawn(net: &Network, key_seed: u8, port: u16) -> Node {
        let a = addr(port);
        let id: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = id.counter();
        let pk = *id.public_static();
        let ep = Endpoint::builder()
            .identity(id)
            .wire(net.wire(a))
            .config(Config::new())
            .build();
        Node {
            ep: Some(ep),
            dhs,
            pk,
            addr: a,
        }
    }

    fn ep(&self) -> &Endpoint {
        self.ep.as_ref().expect("the Endpoint handle was dropped")
    }
}

/// A static nobody is listening for — the "peer that never answers" whose
/// endpoint is not even bound.
fn absent_static(key_seed: u8) -> Pk {
    let id: Id = CountingIdentity::seeded([key_seed; 32]);
    *id.public_static()
}

/// Count of `HandshakeInit` datagrams the fabric carried toward `to`.
///
/// §3.1: byte 0 is the packet type, and §3.1's length gate is **exact**
/// for the handshake types — both are checked so a Data packet that
/// happens to open with `0x01` cannot be miscounted.
fn msg1_count(tap: &Tap, to: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(_from, dst, bytes)| {
            *dst == to && bytes.len() == INIT_PACKET_LEN && bytes[0] == PKT_HANDSHAKE_INIT
        })
        .count()
}

// ────────────────────────────── tests ──────────────────────────────────

/// **S2, T12 — "not before and not never", both halves.**
///
/// # The mutation this catches
///
/// T12 names two broken versions, and a one-sided test lets one of them
/// through:
///
/// * **Resolves early** — a shell that surfaces `TimedOut` at the first
///   unanswered retransmit (or at any deadline that is not
///   `HANDSHAKE_GIVEUP`). Caught **only** by the first assertion, which
///   pins the future as still `Pending` at `HANDSHAKE_GIVEUP − 1 ms`.
/// * **Never resolves** — a shell that arms no give-up timer at all, or
///   arms it against the wrong clock. Caught only by the second.
///
/// The version that asserts merely "eventually `TimedOut`" passes against
/// the first mutation, and the version that asserts merely "not resolved
/// after 10 s" passes against the second. Slice 1's one real gap was a
/// bound tested on one side; this is that lesson applied to a deadline.
///
/// 1 ms rather than 1 ns because tokio's timer wheel quantises to the
/// millisecond: at 1 ns the two observations can land in the same tick and
/// the "not before" half goes vacuous — which would be a *name is not a
/// pin* failure of exactly the kind working rule 9 warns about.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s2_no_answer_gives_timed_out_at_giveup() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let a = Node::spawn(&net, 1, 4001);
            let ghost = (addr(4002), absent_static(2));

            let mut dial = std::pin::pin!(
                a.ep()
                    .connect(ghost.0, ghost.1)
                    .expect("connect() on a NONE static must be Ok")
            );

            // ── not before ──────────────────────────────────────────
            let early =
                tokio::time::timeout(HANDSHAKE_GIVEUP - Duration::from_millis(1), dial.as_mut())
                    .await;
            assert!(
                early.is_err(),
                "§5.5 step 6: `Connecting` resolved BEFORE HANDSHAKE_GIVEUP"
            );

            // ── not never ───────────────────────────────────────────
            //
            // §16.5's `SHELL_LATENESS_BOUND` (250 ms) is the shell's
            // permitted lateness against a core deadline; anything past
            // it is a missing or misarmed timer, not scheduling jitter.
            let late = tokio::time::timeout(
                Duration::from_millis(1) + SHELL_LATENESS_BOUND,
                dial.as_mut(),
            )
            .await
            .expect("`Connecting` had not resolved by HANDSHAKE_GIVEUP + SHELL_LATENESS_BOUND");

            assert!(
                matches!(late, Err(ConnectError::TimedOut)),
                "§5.5 step 6 names TimedOut as the only handshake failure the \
                 application sees; got a different outcome"
            );

            // A dial that never completed costs no DH beyond msg1's own
            // `es`+`ss` per attempt — but the *count* of attempts is the
            // train's, so this asserts only that the give-up path did not
            // add a phantom completion (which would be +2 per attempt).
            assert_eq!(
                a.dhs.get() % 2,
                0,
                "§6.1: an initiation costs es+ss (2 DH); an odd total means a \
                 half-completion ran"
            );
        })
        .await;
}

/// **S2 — the give-up releases the static (§5.5 step 6, §16.1).**
///
/// # The mutation this catches
///
/// A shell that resolves `Connecting` with `TimedOut` but leaves the
/// static PENDING in the shared cell. The application is then permanently
/// unable to redial the peer it was just told to give up on — the exact
/// trap ruling 50 describes for the *cancel* path, reached instead through
/// the *expiry* path. `s2_no_answer_gives_timed_out_at_giveup` passes
/// against it.
///
/// §16.3 (ruling 50) states the two paths are equivalent for the guard —
/// "Cancellation writes nothing to the timestamp guard (§17.1), because it
/// authenticated nothing — **identical to a `HANDSHAKE_GIVEUP` expiry**".
/// The redial is asserted with **no clock advance** after the resolution,
/// for the same reason S29 does: it is the only formulation that separates
/// a synchronous cell write from a flag the driver notices when next
/// scheduled.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s2_giveup_releases_the_static_for_an_immediate_redial() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let a = Node::spawn(&net, 1, 4011);
            let ghost = (addr(4012), absent_static(2));

            let first = a.ep().connect(ghost.0, ghost.1).expect("first dial");
            assert!(matches!(first.await, Err(ConnectError::TimedOut)));

            // No `.await`, no clock advance, between the give-up and the
            // redial.
            let second = a.ep().connect(ghost.0, ghost.1);
            assert!(
                second.is_ok(),
                "§5.5 step 6 + §16.1: after HANDSHAKE_GIVEUP the static is NONE, \
                 so an immediate redial must be Ok, not AlreadyConnected"
            );
        })
        .await;
}

/// **S2 — the retransmit train is §5.5's fixed interval, not exponential.**
///
/// S2's accept clause names this explicitly ("a **fixed 5 s interval plus
/// jitter, explicitly *not* exponential backoff**"; §13's exponential PTO
/// "governs the post-handshake data path and does not apply here"), so it
/// is part of the story even though `.slices/03-skeleton/PLAN.md` §5.2
/// notes the schedule itself is pinned in slice 2a's core tests. What is
/// **not** pinned there is that the *shell* drives it: a driver that arms
/// the give-up but never re-arms the retransmit passes every slice-2a test
/// and fails this one.
///
/// # The mutation this catches, and how the bound separates them
///
/// Working rule 9: assert from the side that separates the broken version
/// from the correct one, not from the side both satisfy.
///
/// | Build | msg1 count over `HANDSHAKE_GIVEUP` |
/// |---|---|
/// | correct: 5 s + U[0, 333 ms] | 17–18 (19 if one lands on the mark) |
/// | **exponential backoff** (5, 10, 20, 40, 80) | **6** |
/// | **no retransmit at all** | **1** |
/// | **runaway / re-armed twice per interval** | ≳ 34 |
///
/// So `15 ≤ n ≤ 20` is a genuine two-sided separator. `n > 1` alone —
/// the obvious assertion — passes against exponential backoff, which is
/// the one shape S2 goes out of its way to forbid.
///
/// The lower bound absorbs `SHELL_LATENESS_BOUND` (250 ms) compounding on
/// every interval: at 5.333 s + 250 ms the train still emits 17.
///
/// **Jitter itself is deliberately not asserted here.** Slice 2a owns it,
/// and an upper-bound-only assertion over intervals is precisely the
/// vacuous test working rule 9 cites ("no interval exceeds base + jitter"
/// passed a core with no jitter at all). Asserting it properly needs the
/// per-datagram timestamps this fixture does not know it has.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s2_retransmit_train_is_fixed_interval_not_exponential() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let tap = net.tap();
            let a = Node::spawn(&net, 1, 4021);
            // A *bound* peer that simply never calls `accept()`: msg1 lands
            // in its stage-0 queue (§6.3) and it transmits nothing. This is
            // "never answers" without an unroutable address, so the count
            // below measures the train and not a send-failure path.
            let b = Node::spawn(&net, 2, 4022);

            let dial = a.ep().connect(b.addr, b.pk).expect("connect");
            assert!(matches!(dial.await, Err(ConnectError::TimedOut)));

            let n = msg1_count(&tap, b.addr);
            assert!(
                (15..=20).contains(&n),
                "§5.5 step 2: a fixed {:?} + jitter train emits 17–18 initiations \
                 across HANDSHAKE_GIVEUP; exponential backoff emits 6 and no train at \
                 all emits 1. Observed {n}",
                RETRANSMIT_BASE
            );

            // ── and it stops at the give-up ──────────────────────────
            //
            // Not "no msg1 in the next instant" — that is the degenerate
            // version and passes on a train merely between retransmits
            // (T11's warning, which applies to the expiry path too). Six
            // full `RETRANSMIT_BASE` intervals of silence is the
            // separating window.
            let at_giveup = msg1_count(&tap, b.addr);
            tokio::time::sleep(RETRANSMIT_BASE * 6).await;
            assert_eq!(
                msg1_count(&tap, b.addr),
                at_giveup,
                "§5.5 step 6: the retransmit train must stop at HANDSHAKE_GIVEUP"
            );
        })
        .await;
}

/// **S2 — the peer that never answers is never charged, and neither are we
/// beyond §6.1's initiator price.**
///
/// A peer that parks initiations without accepting them pays **0 DH**
/// (§6.1: "0 DH so far" at `Intro`; the arrival itself is free), no matter
/// how long the train runs. Slice 2a pins this on the core; the shell half
/// is that the driver does not eagerly run the ladder on arrival — a
/// driver that pre-reads identities to "warm" the queue would show a
/// non-zero count here while passing every other test in this file.
///
/// This is the §6.9 DoS-accounting property stated as a story assertion:
/// 18 unanswered initiations from one source must cost the responder
/// nothing.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s2_unaccepted_initiations_cost_the_responder_zero_dh() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let net = Network::new();
            let a = Node::spawn(&net, 1, 4031);
            let b = Node::spawn(&net, 2, 4032);

            let dial = a.ep().connect(b.addr, b.pk).expect("connect");
            assert!(matches!(dial.await, Err(ConnectError::TimedOut)));

            assert_eq!(
                b.dhs.get(),
                0,
                "§6.1/§6.9: an initiation that is never accepted costs the responder \
                 0 DH; the driver must not run the ladder on arrival"
            );
        })
        .await;
}

// ══════════════════════════════════════════════════════════════════════
// S34 — a responder that keeps accepting survives a lost msg2
// ══════════════════════════════════════════════════════════════════════
//
// `STORIES.md` §J, S34 (ruling 252):
//
// > The application obligation §6.5 now states, exercised end to end.
// >
// > * **Accepts:** drop exactly one msg2 with `FlakyWire`; the initiator's
// >   same `connect()` resolves within §5.5's retransmit schedule — no new
// >   dial, no application retry. The responder's accept loop admits the
// >   peer's fresh initiation; its first, never-confirmed connection
// >   surfaces `ConnectionLost::Replaced`; the replacement carries the
// >   traffic.
// > * **Accepts:** the dial story under loss — at 10 % random loss, 12 of 12
// >   establishments complete against a looping responder (the measured
// >   1-in-12 failure of a one-accept responder goes to 0).
// > * **Cost:** 4 DH per completed `accept()` ladder — the recovery pays the
// >   ladder twice at the responder, once per admitted initiation (§6.1).
// > * **Anchor:** §5.5, §6.4, §6.5, ruling 252. **Paused clock:** yes.
//
// # Authorship (working rule 6)
//
// Written by **R40-D**, from `STORIES.md` §J / S34 and ruling 252 alone, in
// a worktree cut at `9c557ce` — the commit that landed the looping
// `Pair::establish`. Nothing under `src/` outside `testutil` was read for
// behaviour; `src/testutil/mod.rs` **was** read, deliberately and by the
// brief, because ruling 252's own finding is a working-rule-13 finding
// about that file (a single-accept `establish` cannot express a lost
// msg2), and a test written blind to the harness could not know whether
// the loss it asked for actually happened.
//
// # How "exactly one msg2 was dropped" is established (working rule 9)
//
// `FlakyPolicy::drop_at([0])` on the **responder's** wire, plus a check
// that the responder's send #0 really was msg2. That check is not
// decoration: `drop_at` is an index into *this wire's* send sequence, so if
// the responder ever sent anything before its msg2 the policy would be
// dropping a different datagram and the test would be about nothing.
//
// The [`Tap`] cannot itself witness a loss — `FlakyWire::send_to` records
// in the tap at step 4 and draws for loss at step 5, so a lost datagram is
// tapped exactly like a delivered one. What the tap *can* witness is the
// consequence: a second msg1 from the initiator (the §5.5 retransmit) and a
// second msg2 from the responder (the second admitted `accept()`). Those
// two counters are the loss-path evidence throughout, and the assertion
// that turns them into a proof is
// [`s34_the_responder_never_retransmits_msg2`] below, which measures the
// broken build rather than describing it.
//
// # Measured at `9c557ce`, and quoted in each test's doc
//
// ```text
// drop_at([0]) on b's wire:
//   after the first ladder      msg1=1 msg2=1  a_dh=2  b_dh=4
//   dial resolves at            +5.094 s       (RETRANSMIT_BASE = 5 s)
//   after the second ladder     msg1=2 msg2=2  a_dh=6  b_dh=8
//   first.closed()              Replaced
// no second accept, 90 s:       msg1=18 msg2=1 → Err(ConnectError::TimedOut)
// 10 % loss, seeds 0x53400000..+12 (per run, msg1/msg2):
//   1/1 1/1 1/1 1/1 1/1 1/1 1/1 2/2 1/1 1/1 2/1 1/1   totals 14/13
// ```
// Identical under `--release`. Session ids are **not** stable across runs
// (§5.3's wall clock feeds the handshake), so nothing here pins one — only
// that the first and the replacement differ.

use slither::constants::{PKT_HANDSHAKE_RESP, RESP_PACKET_LEN, RETRANSMIT_JITTER_MAX};
use slither::error::ConnectionLost;
use slither::testutil::{
    FlakyPolicy, Pair, Spied, TestConnection, TestEndpoint, TestRecvStream, TestSendStream, settle,
};

/// The §5.5 schedule's own worst case for **one** retransmit, plus §16.5's
/// `SHELL_LATENESS_BOUND` — the shell's permitted lateness against a core
/// deadline. Anything past this is a missing or misarmed timer rather than
/// scheduling jitter.
fn one_retransmit() -> Duration {
    RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX + SHELL_LATENESS_BOUND
}

/// Await `fut`, failing loudly with `what` instead of hanging the suite.
async fn within<F: std::future::Future>(fut: F, budget: Duration, what: &str) -> F::Output {
    match tokio::time::timeout(budget, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {budget:?} of virtual time"),
    }
}

/// Poll `fut` exactly once — the instrument for "**at this instant**", with
/// no virtual time spent and so no timer given a chance to fire.
async fn poll_once<F: std::future::Future>(
    mut fut: std::pin::Pin<&mut F>,
) -> std::task::Poll<F::Output> {
    std::future::poll_fn(|cx| std::task::Poll::Ready(fut.as_mut().poll(cx))).await
}

/// Climb §6.2's staged ladder once — `accept()` → `read_identity()` →
/// `authenticate()` → `accept()`, the 4 DH S34's `Cost:` line prices.
///
/// The budget is the give-up window because the **first** await parks until
/// an `Intro` arrives, which under loss is one or more §5.5 retransmits
/// away; the three staged awaits after it are local to the driver.
async fn climb(ep: &TestEndpoint, what: &str) -> TestConnection {
    let budget = HANDSHAKE_GIVEUP;
    // `accept()` is `Option`: `None` is the endpoint having shut down, not
    // a refused introduction.
    let intro = within(ep.accept(), budget, what)
        .await
        .unwrap_or_else(|| panic!("{what}/endpoint.accept: the endpoint is gone"));
    let claimed = within(intro.read_identity(), budget, what)
        .await
        .unwrap_or_else(|e| panic!("{what}/read_identity: {e:?}"));
    let proven = within(claimed.authenticate(), budget, what)
        .await
        .unwrap_or_else(|e| panic!("{what}/authenticate: {e:?}"));
    within(proven.accept(), budget, what)
        .await
        .unwrap_or_else(|e| panic!("{what}/proven.accept: {e:?}"))
}

/// Handshake datagrams of one type that left `src`.
///
/// §3.1's length gate is **exact** for both handshake types, so the pair
/// (length, type byte) cannot be spoofed by a Data packet that happens to
/// open with the same byte.
fn handshakes(spied: &[Spied], src: SocketAddr, len: usize, ty: u8) -> usize {
    spied
        .iter()
        .filter(|s| s.src == src && s.bytes.len() == len && s.bytes.first() == Some(&ty))
        .count()
}

/// msg1s the initiator put on the wire.
fn msg1s(spied: &[Spied], from: SocketAddr) -> usize {
    handshakes(spied, from, INIT_PACKET_LEN, PKT_HANDSHAKE_INIT)
}

/// msg2s the responder put on the wire — one per admitted `accept()`, and
/// **never** a retransmission (§5.5, ruling 252).
fn msg2s(spied: &[Spied], from: SocketAddr) -> usize {
    handshakes(spied, from, RESP_PACKET_LEN, PKT_HANDSHAKE_RESP)
}

/// A payload whose every byte is a function of its offset, so a shift of
/// any length moves *every* subsequent byte.
fn payload(tag: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i % 251) as u8) ^ tag).collect()
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), HANDSHAKE_GIVEUP, what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: a blocked write is `Pending`, never `Ok(0)`"
        );
        done += n;
    }
}

/// Read exactly `want.len()` bytes and assert they are `want`, byte for
/// byte. The stall is half the assertion: streams are gapless, so a lost
/// byte never completes this read and `within` panics by name.
async fn read_expect(r: &mut TestRecvStream, want: &[u8], what: &str) {
    let mut got = Vec::with_capacity(want.len());
    while got.len() < want.len() {
        let mut buf = vec![0u8; want.len() - got.len()];
        match within(r.read(&mut buf), HANDSHAKE_GIVEUP, what).await {
            Ok(Some(n)) => {
                assert!(n >= 1 && n <= buf.len(), "{what}: read returned {n}");
                got.extend_from_slice(&buf[..n]);
            }
            other => panic!(
                "{what}: wanted {} bytes, got {other:?} after {}",
                want.len(),
                got.len()
            ),
        }
    }
    if let Some(i) = got.iter().zip(want.iter()).position(|(a, b)| a != b) {
        panic!(
            "{what}: first differing byte at offset {i}: got {:#04x}, want {:#04x}",
            got[i], want[i]
        );
    }
}

/// **S34 — the broken build, measured: msg2 is never retransmitted, so a
/// responder that accepts once hangs the dial to `TimedOut` at 90 s.**
///
/// This is the *separator* for
/// [`s34_a_lost_msg2_is_recovered_by_the_next_accept`], and the reason it
/// is a test rather than a sentence in that test's doc: ruling 252's
/// finding is that **nothing on the wire recovers a lost msg2**. §5.5 gives
/// the responder no msg2 retransmission at all — every recovery is a fresh
/// initiation from the initiator, answered only if the application calls
/// `accept()` again (§6.5). Without that second call the initiator talks
/// into a hole for the whole `HANDSHAKE_GIVEUP` window.
///
/// # What separates the correct build from the plausible wrong ones
///
/// | Build | msg2 count over the 90 s window |
/// |---|---|
/// | **correct** — no msg2 retransmission, one accept | **1** |
/// | a responder that retransmits msg2 on §5.5's train | 17–18 |
/// | a responder that answers each parked `Intro` itself | 17–18 |
///
/// `msg2 == 1` is asserted with `assert_eq!`, not `<= 2`: an upper bound
/// that the correct build satisfies with room to spare is working rule 9's
/// vacuous test. The companion `msg1` bound is the same two-sided window
/// `s2_retransmit_train_is_fixed_interval_not_exponential` uses, because
/// **the initiator's train must be unaffected by the responder's silence** —
/// a build that gave up early on an unanswered msg2 would show 6, and one
/// that recovered by re-dialling would show a fresh `Connecting`.
///
/// Measured at `9c557ce`: `msg1=18 msg2=1`, `Err(ConnectError::TimedOut)`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s34_the_responder_never_retransmits_msg2() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let pair = Pair::seeded(0x34_0001);
            let tap = pair.net.tap();
            let (a_addr, b_addr) = (pair.a.addr(), pair.b.addr());

            // Drop the responder's send #0. Which datagram that is, is
            // asserted below rather than assumed.
            pair.b.wire.set_policy(FlakyPolicy::drop_at([0]));

            let dial = pair
                .a
                .endpoint
                .connect(b_addr, pair.b.public_static)
                .expect("§16.1: connect() on a NONE static is Ok");

            // Exactly one accept, ever — the pre-ruling-252 application.
            let first = climb(&pair.b.endpoint, "the one and only accept").await;
            settle().await;

            let snap = tap.snapshot();
            let b_sends: Vec<&Spied> = snap.iter().filter(|s| s.src == b_addr).collect();
            assert_eq!(
                b_sends.len(),
                1,
                "the responder sent {} datagrams before its ladder finished; \
                 `drop_at([0])` would then be dropping something other than msg2 \
                 and this whole file would be measuring the wrong loss",
                b_sends.len()
            );
            assert_eq!(
                (b_sends[0].bytes.len(), b_sends[0].bytes[0]),
                (RESP_PACKET_LEN, PKT_HANDSHAKE_RESP),
                "§3.1: the responder's send #0 — the one `drop_at([0])` removed — \
                 must be the 107-byte msg2"
            );

            // ── the dial talks into the hole for the whole window ──────
            let verdict = within(dial, HANDSHAKE_GIVEUP + SHELL_LATENESS_BOUND, "the dial").await;
            assert!(
                matches!(verdict, Err(ConnectError::TimedOut)),
                "S34's broken build: with the responder's only msg2 lost and no \
                 second accept(), §5.5 offers no recovery and §16.1's give-up is \
                 the outcome. Got {verdict:?}"
            );

            let snap = tap.snapshot();
            assert_eq!(
                msg2s(&snap, b_addr),
                1,
                "§5.5 / ruling 252: **the responder never retransmits msg2.** \
                 A build that did would have answered the train and this dial \
                 would have completed"
            );
            assert!(
                (15..=20).contains(&msg1s(&snap, a_addr)),
                "§5.5: the initiator's fixed-interval train is unaffected by the \
                 responder's silence — 17–18 initiations across HANDSHAKE_GIVEUP. \
                 Observed {}",
                msg1s(&snap, a_addr)
            );

            // The responder is left holding a LIVE, never-confirmed
            // connection: ruling 252's exact state. It is not dropped here
            // before the assertions above, so nothing can be attributed to
            // its teardown.
            drop(first);
        })
        .await;
}

/// **S34 — one lost msg2, and the *same* `connect()` completes at the next
/// accept: `Replaced` on the unconfirmed connection, traffic on the
/// replacement.**
///
/// Every clause of S34's first accept criterion, in order:
///
/// 1. **exactly one msg2 dropped** — `drop_at([0])` on the responder's
///    wire, with the identity of send #0 asserted (see the section header)
///    **and the drop itself observed**: after the first ladder and a
///    `settle()`, one poll of the dial must still be `Pending`. Delete the
///    `drop_at` and that is the line that goes red, by name — without it
///    the failure lands on the second ladder's timeout instead, which
///    reads like a harness fault rather than a vacuous test.
/// 2. **the same `connect()`** — one `Connecting`, pinned before the first
///    ladder and still the same future when it resolves. There is no second
///    `connect()` call in this test, and a build that needed one could not
///    make this future resolve at all.
/// 3. **within §5.5's retransmit schedule** — the resolution instant is
///    measured against `RETRANSMIT_BASE`, two-sided (below).
/// 4. **the accept loop admits the fresh initiation** — the second
///    [`climb`], the whole subject of §6.5's widened SHOULD.
/// 5. **`ConnectionLost::Replaced`** on the first, never-confirmed
///    connection — §6.4's §16.1 guard with basis `Some(t)` and a strictly
///    newer candidate timestamp.
/// 6. **the replacement carries the traffic**, byte-exact, both ways.
/// 7. **4 DH per completed ladder, paid twice at the responder** — S34's
///    `Cost:` line, §6.1.
///
/// # The two-sided timing bound (working rule 9)
///
/// | Build | elapsed from `connect()` to resolution |
/// |---|---|
/// | **correct** — recovers on the first retransmit | 5 s … 5.333 s (+ lateness) |
/// | msg2 not actually dropped (the test measures nothing) | ≈ 0 |
/// | recovery deferred to a later retransmit / a re-dial | ≥ 10 s |
///
/// So `RETRANSMIT_BASE <= elapsed <= RETRANSMIT_BASE + RETRANSMIT_JITTER_MAX
/// + SHELL_LATENESS_BOUND` separates all three. The **lower** bound is the
/// half that matters most: it is what makes the drop real. `t0` is read
/// *before* `connect()`, so the first msg1 leaves at or after it and the
/// first retransmit therefore cannot land before `t0 + RETRANSMIT_BASE` —
/// the bound is derived from §5.5, not from the measurement (5.094 s at
/// `9c557ce`).
///
/// # And the loss path really was taken
///
/// `msg1 == 2` and `msg2 == 2` at the end. `msg2 == 2` is the load-bearing
/// one: the responder emits one msg2 per **admitted `accept()`** and never
/// retransmits (proved by [`s34_the_responder_never_retransmits_msg2`]), so
/// two msg2s is two ladders, and two ladders is §6.5's loop having run.
/// `msg1 == 2` says the initiator retransmitted exactly once — a build that
/// somehow delivered the first msg2 would show 1.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s34_a_lost_msg2_is_recovered_by_the_next_accept() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let pair = Pair::seeded(0x34_0002);
            let tap = pair.net.tap();
            let (a_addr, b_addr) = (pair.a.addr(), pair.b.addr());
            pair.b.wire.set_policy(FlakyPolicy::drop_at([0]));

            let t0 = tokio::time::Instant::now();
            let mut dial = std::pin::pin!(
                pair.a
                    .endpoint
                    .connect(b_addr, pair.b.public_static)
                    .expect("§16.1: connect() on a NONE static is Ok")
            );

            // ── the first ladder, whose msg2 is the dropped datagram ────
            //
            // The dial is polled alongside it: if it resolved here the drop
            // did not happen and every later assertion would be vacuous.
            let first = {
                let ladder = std::pin::pin!(climb(&pair.b.endpoint, "the first accept"));
                tokio::select! {
                    biased;
                    conn = ladder => conn,
                    _ = &mut dial => panic!(
                        "the dial resolved although the responder's only msg2 was \
                         dropped — `drop_at([0])` did not drop msg2"
                    ),
                }
            };
            settle().await;

            let snap = tap.snapshot();
            let b_sends: Vec<&Spied> = snap.iter().filter(|s| s.src == b_addr).collect();
            assert_eq!(
                b_sends.len(),
                1,
                "the responder sent {} datagrams during its first ladder; \
                 `drop_at([0])` is then not dropping msg2",
                b_sends.len()
            );
            assert_eq!(
                (b_sends[0].bytes.len(), b_sends[0].bytes[0]),
                (RESP_PACKET_LEN, PKT_HANDSHAKE_RESP),
                "§3.1: the dropped send #0 must be the 107-byte msg2"
            );

            // **The assertion that makes everything below it mean
            // something.** `settle()` has given both drivers 64 turns at
            // this instant, so a msg2 that arrived would already have
            // resolved the dial. That it has not is the loss, observed
            // directly and with no virtual time spent — remove the
            // `drop_at` above and this is the line that goes red.
            assert!(
                poll_once(dial.as_mut()).await.is_pending(),
                "the dial resolved although the responder's only msg2 was dropped: \
                 `drop_at([0])` did not drop msg2, and every assertion below here \
                 would be measuring an ordinary handshake"
            );
            assert_eq!(
                pair.b.dhs.get(),
                4,
                "§6.1 / S34's `Cost:` line: one completed accept() ladder is 4 DH"
            );
            let first_session = first.session_id();

            // ── §6.5's loop: accept again, and the same dial completes ──
            let (second, dialled) = within(
                async { tokio::join!(climb(&pair.b.endpoint, "the second accept"), &mut dial) },
                one_retransmit() + SHELL_LATENESS_BOUND,
                "the second ladder and the original dial",
            )
            .await;
            let elapsed = t0.elapsed();
            let a_conn = dialled.expect(
                "S34: the initiator's **same** connect() must complete once the \
                 responder accepts the retransmitted initiation",
            );

            assert!(
                elapsed >= RETRANSMIT_BASE,
                "§5.5: the recovery cannot precede the first retransmit — {elapsed:?} \
                 < {RETRANSMIT_BASE:?} means the msg2 was never really lost"
            );
            assert!(
                elapsed <= one_retransmit(),
                "§5.5: the same connect() must complete on the **first** retransmit; \
                 {elapsed:?} > {:?} means the recovery waited for a later one",
                one_retransmit()
            );

            // ── the never-confirmed connection dies `Replaced` ──────────
            //
            // No virtual time is spent: §4.1 fires the teardown *at* the
            // replacing accept(), which has already returned. A build that
            // surfaced it on a later timer fails here and would pass a
            // version written with a generous timeout.
            settle().await;
            let lost = within(first.closed(), Duration::ZERO, "first.closed()").await;
            assert_eq!(
                lost,
                ConnectionLost::Replaced,
                "§6.4: we hold basis `Some(t)` because we accepted, and the fresh \
                 initiation carries a strictly newer §5.3 timestamp, so the first, \
                 never-confirmed connection is `Replaced` — not TimedOut, not \
                 PeerClosed"
            );
            assert_ne!(
                first_session,
                second.session_id(),
                "§5.4: the replacement is a **fresh** connection, not the old one \
                 re-homed"
            );
            assert_eq!(
                a_conn.session_id(),
                second.session_id(),
                "the initiator's connection and the responder's replacement are \
                 the two ends of one session"
            );

            // ── the replacement carries the traffic, byte-exact ─────────
            let bi = within(a_conn.open_bi(), one_retransmit(), "open_bi")
                .await
                .expect("open_bi");
            let (mut a_send, mut a_recv) = bi.split();
            let up = payload(0xA5, 4096);
            write_all(&mut a_send, &up, "S34 initiator → responder").await;

            let peer_bi = within(second.accept_bi(), one_retransmit(), "accept_bi")
                .await
                .expect("accept_bi");
            let (mut b_send, mut b_recv) = peer_bi.split();
            read_expect(&mut b_recv, &up, "S34 initiator → responder").await;

            let down = payload(0x5A, 4096);
            write_all(&mut b_send, &down, "S34 responder → initiator").await;
            read_expect(&mut a_recv, &down, "S34 responder → initiator").await;

            // ── the loss path, and S34's cost line ──────────────────────
            let snap = tap.snapshot();
            assert_eq!(
                msg2s(&snap, b_addr),
                2,
                "S34: one msg2 per admitted accept(), never a retransmission — \
                 two msg2s is §6.5's loop having run twice, which is the whole \
                 recovery"
            );
            assert_eq!(
                msg1s(&snap, a_addr),
                2,
                "§5.5: exactly one retransmitted initiation was needed; 1 would \
                 mean the first msg2 was delivered after all"
            );
            assert_eq!(
                pair.b.dhs.get(),
                8,
                "S34's `Cost:` line: 4 DH per completed accept() ladder, and the \
                 recovery pays the ladder **twice** at the responder — once per \
                 admitted initiation (§6.1)"
            );
        })
        .await;
}

/// **S34 — the dial story under loss: 12 of 12 establishments complete at
/// 10 % random loss against a looping responder.**
///
/// Ruling 252 measured **1 in 12** dials failing at 10 % loss against a
/// one-accept responder, and the arithmetic says why: a first round loses
/// the msg2 but not the msg1 with probability `0.9 × 0.1 = 9 %`, and that
/// responder never speaks again. This test is that measurement re-run
/// against the loop, and the bar is 12/12.
///
/// # Which seeds exercise the loss path, and how that was verified
///
/// A run that never drops a handshake packet separates nothing (working
/// rule 9): 12 clean handshakes pass against a one-accept responder too.
/// The seeds are therefore chosen, not arbitrary. Per-run tap counts at
/// `9c557ce`, seeds `0x53400000 + 0..12`:
///
/// | run | 0 | 1 | 2 | 3 | 4 | 5 | 6 | **7** | 8 | 9 | **10** | 11 |
/// |---|---|---|---|---|---|---|---|---|---|---|---|---|
/// | msg1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | **2** | 1 | 1 | **2** | 1 |
/// | msg2 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | **2** | 1 | 1 | 1 | 1 |
///
/// Totals 14 msg1 and 13 msg2 for twelve establishments, and the two
/// interesting runs are distinguishable from each other:
///
/// * **run 10** — `msg1=2, msg2=1`: the *first msg1* was lost. The
///   responder saw only the retransmit and accepted once. A one-accept
///   responder survives this one.
/// * **run 7** — `msg1=2, msg2=2`: the responder accepted **twice**, so
///   both msg1s reached it, so the first msg2 is the datagram that was
///   lost. **This is the run a one-accept responder fails** — ruling 252's
///   1-in-12, landed exactly once in twelve.
///
/// The inference is forced rather than assumed: the responder emits one
/// msg2 per admitted `accept()` and never retransmits
/// ([`s34_the_responder_never_retransmits_msg2`]), and the initiator only
/// retransmits an initiation that went unconfirmed — so `msg1 == 2` with
/// `msg2 == 2` cannot be produced by any loss pattern except a lost msg2.
///
/// Both properties are asserted rather than left to the table, so a seed
/// drift turns this red instead of quietly making it vacuous. The observed
/// counts are printed in the panic message for exactly that case.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s34_twelve_of_twelve_establishments_complete_at_ten_percent_loss() {
    /// S34's bar, and ruling 252's denominator.
    const RUNS: u64 = 12;
    /// The story's rate.
    const LOSS: f64 = 0.10;
    /// Chosen so the twelve runs contain both a lost msg1 and a lost msg2;
    /// see the doc table.
    const SEED_BASE: u64 = 0x5340_0000;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut observed: Vec<(usize, usize)> = Vec::with_capacity(RUNS as usize);

            for run in 0..RUNS {
                let pair = Pair::seeded(SEED_BASE + run);
                let tap = pair.net.tap();
                let (a_addr, b_addr) = (pair.a.addr(), pair.b.addr());
                pair.a.wire.set_policy(FlakyPolicy::lossy(LOSS));
                pair.b.wire.set_policy(FlakyPolicy::lossy(LOSS));

                // `Pair::establish` is ruling 252(iv)'s looping responder.
                // It panics if either side fails, so reaching the end of
                // this loop **is** 12/12; the timeout only turns a hang
                // into a named failure.
                let (a_conn, b_conn) = within(
                    pair.establish(),
                    HANDSHAKE_GIVEUP + SHELL_LATENESS_BOUND,
                    &format!("run {run} (seed {:#x}) never established", SEED_BASE + run),
                )
                .await;

                let snap = tap.snapshot();
                observed.push((msg1s(&snap, a_addr), msg2s(&snap, b_addr)));

                // §16.3: the driver stops with the last handle, so each run
                // leaves nothing behind for the next.
                drop((a_conn, b_conn, pair));
            }

            assert_eq!(
                observed.len(),
                RUNS as usize,
                "S34: {RUNS} of {RUNS} establishments must complete at \
                 {}% loss against a looping responder",
                LOSS * 100.0
            );

            let lost_a_packet = observed.iter().filter(|(m1, _)| *m1 >= 2).count();
            let needed_a_second_accept = observed.iter().filter(|(_, m2)| *m2 >= 2).count();

            assert!(
                lost_a_packet >= 1,
                "the run separates nothing: no establishment retransmitted, so no \
                 handshake packet was lost at {}% and a one-accept responder would \
                 have passed too. Observed (msg1, msg2) per run: {observed:?}",
                LOSS * 100.0
            );
            assert!(
                needed_a_second_accept >= 1,
                "no establishment needed a second accept(): every msg2 arrived, so \
                 §6.5's loop was never the thing that completed a dial. This is the \
                 assertion ruling 252's 1-in-12 lives in. Observed (msg1, msg2) per \
                 run: {observed:?}"
            );
        })
        .await;
}

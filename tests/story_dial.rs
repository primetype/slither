//! **S2 — a user can dial a peer that never answers, and be told.**
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
type Endpoint = slither::Endpoint<Id>;

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

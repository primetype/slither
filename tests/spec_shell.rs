//! **Appendix B — "The shell surface (rulings 46, 47, 49, 50)", the part
//! slice 3 can reach.**
//!
//! Appendix B is non-normative but its obligations are written against the
//! *finished* protocol, while `PLAN.md` delivers it in ten slices. That
//! mismatch has a failure mode worth naming: a test file that **looks**
//! like it discharges Appendix B and does not. So the table below is the
//! whole obligation, marked, and the unreached rows name the slice that
//! owes them. `.slices/03-skeleton/PLAN.md` C5 is the same observation for
//! the `closed()` rows specifically.
//!
//! | Appendix B obligation | Here |
//! |---|---|
//! | `closed()` resolves on every death, with no verb in flight | `tests/story_lifecycle.rs` (S27); rows reached and owed are listed there |
//! | `closed()` is latched — concurrent, late, cancel-safe | `tests/story_lifecycle.rs` (S27, T10) |
//! | Dropping a `Connecting` frees the static; the train stops; NONE routing | `tests/story_lifecycle.rs` (S29, T11) |
//! | The peer-side half of ruling 50: a half-open session reaped at `DEAD_TIMEOUT` | **here** — `cancelled_dial_leaves_the_peer_a_silent_half_open_session`, by an equivalent construction; see G10 |
//! | Notification retention and its O(1) bound (ruling 46) | **owed: slice 7.** `Notification`, `notified()` and the slots do not exist yet (§16.2) |
//! | Delivery confirmation / `acked()` (ruling 47) | **owed: slice 5.** `acked()`, streams and messages do not exist yet |
//! | A send failure is traced, never acted on (ruling 49) | **partly here** — the *never acted on* half. See G7 for the *traced* half and G8 for "traffic resumes when the seam heals" |
//! | CLOSE linger: ≤ 1 reply/s, replies to the session address only, none to a forgery (§15.2, T13) | **not reachable from `tests/`.** See G6 |
//!
//! Also pinned here, because they are shell-surface rulings with no other
//! home in slice 3: **ruling 87** (`connect()` is synchronous), **ruling
//! 89** (`session_id()` is hiss's), **§6.2**'s drop-is-a-silent-reject at
//! all three stages, and **§16.3**'s staged-object/`Connecting` asymmetry
//! over `EndpointDropped`.
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by **TEST-B** from `SPEC.md` (§6.1–§6.2, §15.2, §16.1–§16.3,
//! §16.10, Appendix B) and `STORIES.md` alone, while IMPL-B wrote the shell
//! concurrently. No file under `src/shell/` and no line of
//! `src/testutil/mod.rs` was read. Names for shell and harness items are
//! **proposals**; the integrator renames *calls*, never assertions.
//!
//! # Gaps found while writing this file (working rule 8)
//!
//! * **G6 — §15.2's linger reply rule is unreachable from an integration
//!   test.** T13 asks for three assertions: exactly one reply to ten
//!   authenticated inbound packets in a second, the reply sent to the
//!   *session's* address rather than the triggering packet's source, and
//!   **no** reply to a packet that routes by `receiver_index` but fails the
//!   AEAD. All three require minting an authenticated (or
//!   deliberately-unauthenticated-but-index-correct) packet at a chosen
//!   moment — and after a peer receives a CLOSE it is *draining* and sends
//!   nothing (§15.2), so no second slither endpoint can generate the
//!   traffic either. The session keys live inside the crate. **This
//!   obligation is reachable only from an in-crate test module**, and this
//!   file cannot discharge it. Ruling 83 (the opening CLOSE is not a reply,
//!   so the 1 Hz clock starts at the first *reply*) is unreachable for the
//!   same reason and is **not** depended on by any test in this file.
//! * **G7 — the *traced* half of ruling 49 needs a dev-dependency that
//!   does not exist.** §16.3 makes it a MUST that a failing `send_to` is
//!   traced against the connection under §18.2's operator contract, and
//!   Appendix B asks the test to assert "a `slither::io` trace was emitted
//!   **per failed send** carrying the destination address". `tracing` is a
//!   dependency; `tracing-subscriber` (or any capturing layer) is not in
//!   `[dev-dependencies]`, so nothing here can observe an event. Reported
//!   rather than worked around: adding a dev-dependency is the
//!   orchestrator's call, not a test author's.
//! * **G8 — "traffic resumes when the seam heals" needs data frames.**
//!   Slice 4. Named in `.slices/03-skeleton/PLAN.md` §5.2 already.
//! * **G9 — §16.2's `accept() -> Option<Intro>` says "`None` = endpoint
//!   closed" and no verb in this specification closes an endpoint.** The
//!   surface has no `Endpoint::close()`, §16.3's only endpoint-lifetime
//!   rule is "dropping every handle stops the driver", and `accept(&self)`
//!   borrows the `Endpoint` — so the `Endpoint` provably outlives every
//!   `accept()` future and the `None` arm has no constructible cause. This
//!   is working rule 8's shape exactly: **a stated construction (`None`)
//!   with an unstated scope (what closes an endpoint)**. Either a verb is
//!   missing from §16.2's list, or `None` is unreachable and the return
//!   type should say so. Not invented here; the tests below `.expect()` on
//!   the `Some` arm and say why.
//!
//!   **Update (post-slice-3b seam review).** There is now one constructible
//!   cause: a **failed driver**. The driver's stop path runs on an unwind
//!   as well as on the ordinary exit, so a panicking driver drops the
//!   parked `accept()` senders while an `Endpoint` handle still lives, and
//!   `accept()` resolves `None` —
//!   `a_panicking_driver_resolves_every_waiter_instead_of_parking_it`
//!   below is the constructive proof. That answers "is `None` reachable"
//!   and **not** "is a driver fault what §16.2 meant by *endpoint
//!   closed*". `.slices/03-skeleton/FIXES-3b.md` §2a files the remaining
//!   wording question as a ruling candidate; no spec text was changed, and
//!   the `.expect()`s below stand.
//! * **G10 — Appendix B's "drop the `Connecting` after its msg2 is on the
//!   wire" is not separable on a zero-latency fabric.** Stated in full on
//!   `cancelled_dial_leaves_the_peer_a_silent_half_open_session`, which
//!   reaches the same peer-side state by §16.3's other route rather than
//!   inventing a directional-loss knob on `FlakyPolicy`.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Every test here is `#[tokio::test(start_paused = true)]` on a
//! `LocalSet`. `tokio::time::timeout` is the observation instrument, not a
//! deadline: `timeout(d, &mut fut).await.is_err()` asserts `fut` was still
//! `Pending` at `now + d`.
//!
//! # Post-slice-3b seam-review regressions (appended, different author)
//!
//! The last three tests in this file are **not** TEST-B's and are not
//! Appendix B obligations. They pin three findings from the chartered seam
//! review of `src/shell/`, and each one needed a fixture the file did not
//! have — which is the common thread worth recording:
//!
//! | Test | Pins |
//! |---|---|
//! | `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer` | §16.4's drain contract across a wire that returns `Pending` |
//! | `s3a_accept_ahead_of_connect_resolves_already_connected` | §16.1's accept-vs-connect race, and S3a's answer to it |
//! | `a_panicking_driver_resolves_every_waiter_instead_of_parking_it` | §16.3: a driver fault resolves every waiter rather than freezing the endpoint |
//!
//! `testutil::FlakyWire` models everything a *network* does — loss, delay,
//! reordering, duplication, `ENETUNREACH` — and nothing a *socket* does:
//! its `send_to` never returns `Pending` and never panics. Two of the three
//! findings lived precisely in that blind spot and were unreachable from
//! all 451 tests **by construction**, which is the shape `PLAN.md`'s target
//! 6 asks reviewers to hunt. `GatedWire` and `PanicWire` below are the two
//! wires that reach it; they are deliberately local to this file rather
//! than added to `testutil`, whose surface ruling 60 attests.
//!
//! See `.slices/03-skeleton/FIXES-3b.md` for the findings and the
//! reasoning.

#![allow(clippy::items_after_statements)]

use std::cell::Cell;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use slither::config::Config;
use slither::constants::{DEAD_TIMEOUT, RESP_PACKET_LEN, SHELL_LATENESS_BOUND};
use slither::error::{ConnectError, ConnectionLost, IntroError};
use slither::identity::{Identity, PublicKeyOf};
use slither::shell::wire::Wire;
use slither::testutil::{
    CountingIdentity, DhCounter, FlakyPolicy, FlakyWire, Network, Tap, settle,
};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE — proposed names, flagged loudly. Identical in intent to the
// blocks at the top of `tests/story_lifecycle.rs` and `tests/story_dial.rs`;
// duplicated because each file in `tests/` is its own crate and this author
// owns no shared module.
//
// INTEGRATOR: redirect at IMPL-B's `testutil` harness if one fits. Nothing
// below the `── tests ──` line touches these names except through `Node`,
// `establish` and the two tap helpers.
//
// The **single riskiest name in this file** is the send-failure injector:
//
//     let policy = FlakyPolicy::perfect();
//     let wire = net.wire_with(addr, policy.clone());
//     policy.fail_sends(true);   // every send_to returns ENETUNREACH
//
// §16.10 ratifies that `FlakyPolicy` exists, that it carries "loss,
// reordering, duplication, and **send failure**", and that "send-failure
// injection is required, not optional" — so the *capability* is contract.
// The toggle above is this author's shape for it, chosen because Appendix
// B describes "a bounded interval, then heals", which a toggle expresses
// without assuming anything about scheduling. Every assertion that uses it
// is written on the connection's behaviour, not on the injector.
// ══════════════════════════════════════════════════════════════════════

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::Endpoint<Id>;
// See the note on the same alias in `tests/story_lifecycle.rs`: §16.2 writes
// a bare `Connection`, `core::Connection<C: Handshake>` is
// suite-parameterised. One line to change; no test names the type.
type Connection = slither::Connection<Suite>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

struct Node {
    ep: Option<Endpoint>,
    dhs: DhCounter,
    pk: Pk,
    addr: SocketAddr,
}

impl Node {
    fn spawn(net: &Network, key_seed: u8, port: u16) -> Node {
        Node::spawn_with(net, key_seed, port, None)
    }

    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
    fn spawn_with(net: &Network, key_seed: u8, port: u16, policy: Option<FlakyPolicy>) -> Node {
        let a = addr(port);
        let wire = match policy {
            Some(p) => net.wire_with(a, p),
            None => net.wire(a),
        };
        Node::spawn_over(key_seed, port, wire)
    }

    /// Spawn over a caller-supplied [`Wire`] instead of a bare `FlakyWire`.
    ///
    /// `Endpoint<I>` is **not** parameterised by its wire — §16.3 gives the
    /// wire to the driver and erases it there — so a `Node` holds an
    /// endpoint over any `Wire` without a second type parameter. The two
    /// tests at the bottom of this file need wires the shared fixture
    /// cannot express: one that can be *suspended* mid-send, and one that
    /// *panics*.
    fn spawn_over<W: Wire + 'static>(key_seed: u8, port: u16, wire: W) -> Node {
        let a = addr(port);
        let id: Id = CountingIdentity::seeded([key_seed; 32]);
        let dhs = id.counter();
        let pk = *id.public_static();
        let ep = Endpoint::builder()
            .identity(id)
            .wire(wire)
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

fn absent_static(key_seed: u8) -> Pk {
    let id: Id = CountingIdentity::seeded([key_seed; 32]);
    *id.public_static()
}

async fn establish(dialler: &Node, listener: &Node) -> (Connection, Connection) {
    let dial = dialler
        .ep()
        .connect(listener.addr, listener.pk)
        .expect("connect() on a NONE static must be Ok");
    let accept = async {
        // G9: the `None` arm has no constructible cause in this
        // specification, so it is an `expect` and not a branch.
        let intro = listener
            .ep()
            .accept()
            .await
            .expect("accept() returned None");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await.expect("accept")
    };
    let (initiator, responder) = tokio::join!(dial, accept);
    (
        initiator.expect("Connecting resolved with an error"),
        responder,
    )
}

fn sent_count(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, _bytes)| *src == from)
        .count()
}

/// §3.1: byte 0 is the packet type, and the length gate is **exact** for
/// `HandshakeResp` (107 bytes).
fn msg2_count(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, bytes)| {
            *src == from
                && bytes.len() == RESP_PACKET_LEN
                && bytes[0] == slither::constants::PKT_HANDSHAKE_RESP
        })
        .count()
}

async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

// ══════════════════════════════════════════════════════════════════════
// Two wires the shared fixture cannot express.
//
// `FlakyWire` models everything a *network* does — loss, delay, reorder,
// duplication, `ENETUNREACH` — and nothing a *socket* does. Its `send_to`
// never returns `Pending` and never panics, so two whole classes of driver
// behaviour are unreachable from the 451-test suite by construction. Both
// classes turned out to hold a defect (see the module-doc note above).
// ══════════════════════════════════════════════════════════════════════

/// A latch a test opens and closes to suspend [`GatedWire`] mid-send.
struct Gate {
    open: Cell<bool>,
    notify: tokio::sync::Notify,
}

impl Gate {
    fn opened() -> Rc<Gate> {
        Rc::new(Gate {
            open: Cell::new(true),
            notify: tokio::sync::Notify::new(),
        })
    }

    fn close(&self) {
        self.open.set(false);
    }

    fn open(&self) {
        self.open.set(true);
        self.notify.notify_one();
    }
}

/// A [`Wire`] whose `send_to` can be held `Pending`, the way a real socket
/// holds one when its send buffer is full.
///
/// This is not an exotic condition: `tokio::net::UdpSocket::send_to`
/// returns `Pending` whenever the kernel's send buffer is full, which is
/// every congested endpoint. `FlakyWire::send_to` never does — it queues
/// into an in-memory inbox and returns — so **no test in this repository
/// could reach a driver-side yield between the drain and the next
/// `poll_output()`** until this type existed.
struct GatedWire {
    inner: FlakyWire,
    gate: Rc<Gate>,
}

impl Wire for GatedWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        while !self.gate.open.get() {
            self.gate.notify.notified().await;
        }
        self.inner.send_to(buf, addr).await
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.inner.recv_from(buf).await
    }
}

/// A [`Wire`] whose `send_to` panics once armed — the smallest way to make
/// the driver task unwind using only the public surface.
///
/// The panic message is deliberately self-describing: the test that arms
/// this **expects** a driver panic, and the harness prints it under
/// `--nocapture` while still reporting `ok`. That is the point of the test,
/// not a failure of it.
struct PanicWire {
    inner: FlakyWire,
    armed: Rc<Cell<bool>>,
}

impl Wire for PanicWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        assert!(
            !self.armed.get(),
            "PanicWire: deliberate driver panic, armed by \
             a_panicking_driver_resolves_every_waiter_instead_of_parking_it",
        );
        self.inner.send_to(buf, addr).await
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.inner.recv_from(buf).await
    }
}

// ────────────────────────────── tests ──────────────────────────────────

/// **Ruling 87 — `connect()` is synchronous, and `AlreadyConnected` comes
/// back before any await.**
///
/// §16.3 (4249–4264): "§16.2 declares `pub fn connect(…) -> Result<
/// Connecting, ConnectError>` — **not `async`** — so
/// `ConnectError::AlreadyConnected` is returned before any await, and a
/// oneshot reply cannot be read from it without blocking, which §16.8
/// forbids. … the NONE/PENDING/LIVE test §16.1 requires 'at the instant of
/// the call' is a synchronous read of the same shared cell §16.8 already
/// mandates for the accessors."
///
/// # The mutation this catches — and why there is no `await` in the gap
///
/// The broken version is ruling 53's *original* table, which listed
/// `connect` beside the genuinely-`async` endpoint verbs: a command sent to
/// the driver, with the PENDING bookkeeping done **on the driver task**.
/// Such a build is indistinguishable from the correct one as soon as the
/// test yields — the driver runs, the pending is minted, and the second
/// `connect()` sees PENDING.
///
/// So the second call is made with **no `.await` and no clock advance**
/// after the first. On a paused, current-thread runtime the driver is not
/// scheduled in that gap at all. A driver-side build therefore reads NONE
/// and hands back a second `Connecting` — two concurrent outbound attempts
/// to one static, which §16.1 says "every routing rule keys on" not
/// happening. The synchronous-cell build returns `AlreadyConnected`.
///
/// This is the exact mirror of `s29_cancel_then_immediate_redial`: that one
/// pins the cell being *cleared* synchronously, this one pins it being
/// *written* synchronously. Neither implies the other.
///
/// (The LIVE case — `connect()` to a static that already has a live
/// `Connection` — is **S3a**, which is not slice 3b's story. Only the
/// in-flight-outbound clause is pinned here.)
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn connect_is_synchronous_so_a_pending_static_refuses_before_any_await() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6001);
            let ghost = (addr(6002), absent_static(2));

            let first = a.ep().connect(ghost.0, ghost.1);
            assert!(first.is_ok(), "the first dial to a NONE static must be Ok");

            // ── NO await, NO clock advance, between the two ─────────
            let second = a.ep().connect(ghost.0, ghost.1);

            assert!(
                matches!(second, Err(ConnectError::AlreadyConnected)),
                "§16.1 + ruling 87: an outbound connect is in flight while its \
                 Connecting lives, and the NONE/PENDING/LIVE test is a SYNCHRONOUS \
                 read at the instant of the call. Got {:?}",
                second.err()
            );

            // The first attempt is untouched by the refusal.
            let mut first = pin!(first.expect("checked above"));
            assert!(
                poll_once(first.as_mut()).await.is_pending(),
                "the refused second call must not disturb the first attempt"
            );
        })
        .await;
}

/// **Ruling 89 — `session_id()` is hiss's, derived from the handshake hash,
/// and both peers of a session produce the same value.**
///
/// §16.2: "hiss derives it from the handshake hash, **both peers of a
/// session produce the same value**, and its own documentation states it is
/// a *public* channel-binding value meant for out-of-band comparison —
/// which is precisely what an application logs it for, and what a
/// short-authentication-string check needs."
///
/// # The mutation this catches
///
/// The obvious test is `assert_eq!(ca.session_id(), cb.session_id())`. It
/// passes against **every** degenerate accessor: one that returns
/// `Default::default()`, a zero id, a constant, or the endpoint's own index
/// if both endpoints happen to mint the same one. A constant is exactly
/// what a not-yet-wired accessor looks like.
///
/// The separating assertion is the second one: a **different** session must
/// produce a **different** id. Agreement across peers and difference across
/// sessions together are the only pair that pins a value derived from the
/// handshake hash. A short-authentication-string check is worthless without
/// both.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn session_id_agrees_across_peers_and_differs_across_sessions() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6011);
            let b = Node::spawn(&net, 2, 6012);
            let c = Node::spawn(&net, 3, 6013);
            let d = Node::spawn(&net, 4, 6014);

            let (ca, cb) = establish(&a, &b).await;
            let (cc, cd) = establish(&c, &d).await;

            assert_eq!(
                ca.session_id(),
                cb.session_id(),
                "ruling 89: both peers of a session produce the same SessionId"
            );
            assert_eq!(
                cc.session_id(),
                cd.session_id(),
                "likewise for the second pair"
            );
            assert_ne!(
                ca.session_id(),
                cc.session_id(),
                "ruling 89: the SessionId is derived from the handshake hash, so two \
                 independent sessions must differ — an accessor returning a constant \
                 or a default passes the equality assertions above and fails here"
            );
        })
        .await;
}

/// **§6.2 — dropping an `Intro` is a silent reject, and the ladder charges
/// 0 DH for it.**
///
/// §6.1/§16.1: "staged objects … drop = silent reject at every stage."
/// §6.2: "Dropping the object at any stage is the application's rejection —
/// the only rejection there is, and slither keeps no record of it (ruling
/// 48, §6.1)."
///
/// # The mutations this catch, and why "nothing transmitted" is not enough
///
/// * **Not silent** — a build that answers the initiation, or emits any
///   packet, on a rejected `Intro`. Caught by the `sent_count` assertion.
/// * **Not free** — a build whose driver eagerly runs `es` when the
///   initiation is queued, so that the "0 DH so far" of §6.1's table is a
///   fiction. Caught by the `DhCounter` assertion; §6.9's whole DoS
///   accounting rests on it.
/// * **Not recoverable** — a build that poisons the peer's static or the
///   stage-0 slot on rejection, so the *next* initiation from the same peer
///   can never be accepted. This is the one a "nothing was transmitted"
///   test cannot see, and it is the difference between a silent reject and
///   a silent blackhole. Caught by driving the very next `accept()` chain to
///   a live connection.
///
/// §5.5 step 2 supplies the next initiation for free: "every retransmit is
/// a completely fresh initiation", one per `RETRANSMIT_BASE`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dropping_an_intro_is_a_silent_reject_and_costs_nothing() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6021);
            let b = Node::spawn(&net, 2, 6022);

            let dial = a.ep().connect(b.addr, b.pk).expect("dial");
            let mut dial = pin!(dial);

            let reject_then_accept = async {
                let intro = b.ep().accept().await.expect("accept() returned None");
                assert_eq!(
                    b.dhs.get(),
                    0,
                    "§6.1: an Intro is 0 DH — the arrival itself is free"
                );
                let before = sent_count(&tap, b.addr);
                drop(intro);
                assert_eq!(
                    sent_count(&tap, b.addr),
                    before,
                    "§6.2: dropping an Intro is a SILENT reject"
                );
                assert_eq!(b.dhs.get(), 0, "§6.1: the rejection itself costs nothing");

                // The next fresh initiation (§5.5 step 2) must be
                // acceptable: a silent reject is not a blackhole.
                let intro = b.ep().accept().await.expect("accept() returned None");
                let claimed = intro.read_identity().await.expect("read_identity");
                let proven = claimed.authenticate().await.expect("authenticate");
                proven.accept().await.expect("accept")
            };

            let (connected, accepted) = tokio::join!(dial.as_mut(), reject_then_accept);

            assert!(
                connected.is_ok(),
                "the dialler's Connecting must resolve once the peer accepts a later \
                 initiation: {:?}",
                connected.err()
            );
            assert_eq!(accepted.remote_static().as_ref(), a.pk.as_ref());
        })
        .await;
}

/// **§6.1/§6.2 — the responder's ladder is 0 / 1 / 2 / 4, and no stage runs
/// ahead of its verb.**
///
/// §6.1's table prices the staged accept: `Intro` 0 DH, `read_identity()`
/// +1 (`es`), `authenticate()` +1 (`ss`), `accept()` +2 (`ee`, `se`).
/// Slice 2a pins this on the core; what is pinned **here** is that the
/// *shell*'s staged handles do not move the work — §16.3: "The endpoint
/// verbs stay round-trips because §6.2 requires the DH costs to land on the
/// driver task."
///
/// # The mutation this catches
///
/// The sharp one is at `Proven`: a driver that computes msg2 during
/// `authenticate()` — a natural optimisation, since `authenticate()` has
/// already done the expensive part and the answer is usually wanted. It
/// costs the responder 4 DH for an initiation the application then rejects,
/// which is §6.9's accounting inverted, and it **transmits msg2 on a
/// `Proven` the application drops**, which §6.2 says is a silent reject.
/// Both consequences are asserted: the count at `Proven` is `2`, exactly,
/// and no `HandshakeResp` has left the responder at that point.
///
/// `assert_eq!` and not `<=` at every step: an upper bound is satisfied by
/// a collapsed ladder that skips `ss`, and `ss` is the DH that proves
/// possession.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_staged_ladder_charges_and_transmits_only_at_its_own_verb() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6031);
            let b = Node::spawn(&net, 2, 6032);

            let dial = a.ep().connect(b.addr, b.pk).expect("dial");

            let ladder = async {
                let intro = b.ep().accept().await.expect("accept() returned None");
                assert_eq!(b.dhs.get(), 0, "§6.1: Intro is 0 DH");
                assert_eq!(msg2_count(&tap, b.addr), 0, "no msg2 before accept()");
                // Ruling 71's stage-0 accessors, which §16.4's list omitted.
                assert_eq!(
                    intro.source(),
                    a.addr,
                    "§6.2: Intro::source() is the initiation's source address"
                );
                assert_ne!(
                    intro.sender_index(),
                    0,
                    "§5.5 step 1 / §17.3: the initiator's sender_index is nonzero"
                );

                let claimed = intro.read_identity().await.expect("read_identity");
                assert_eq!(b.dhs.get(), 1, "§6.1: read_identity is exactly one DH (es)");
                assert_eq!(msg2_count(&tap, b.addr), 0, "no msg2 after read_identity");
                assert_eq!(
                    claimed.claimed_static().as_ref(),
                    a.pk.as_ref(),
                    "§6.2: claimed_static() recovers the CLAIMED static — and is \
                     never named remote_static()"
                );

                let proven = claimed.authenticate().await.expect("authenticate");
                assert_eq!(
                    b.dhs.get(),
                    2,
                    "§6.1: cumulative cost at Proven is exactly 2 (es + ss) — a 4 here \
                     means ee/se ran before accept() asked for them"
                );
                assert_eq!(
                    msg2_count(&tap, b.addr),
                    0,
                    "§6.2: nothing is transmitted until accept(); a Proven the \
                     application drops is a SILENT reject"
                );
                assert_eq!(
                    proven.peer_static().as_ref(),
                    a.pk.as_ref(),
                    "§6.2: at Proven the static is proven, not claimed"
                );

                let conn = proven.accept().await.expect("accept");
                assert_eq!(b.dhs.get(), 4, "§6.1: the accept fast path is exactly 4 DH");
                assert_eq!(
                    msg2_count(&tap, b.addr),
                    1,
                    "accept() emits exactly one msg2"
                );
                conn
            };

            let (connected, _accepted) = tokio::join!(dial, ladder);
            assert!(connected.is_ok(), "{:?}", connected.err());
        })
        .await;
}

/// **§6.2 — dropping a `Proven` transmits nothing.**
///
/// The stage-specific companion to the ladder test above: there, the
/// `Proven` is accepted; here it is dropped, which is the case §6.2 calls
/// "the application's rejection".
///
/// # The mutation this catches
///
/// A build that emits msg2 at `Proven` construction rather than at
/// `accept()`. `the_staged_ladder_charges_and_transmits_only_at_its_own_verb`
/// catches it too — but only *while* the chain is walked to `accept()`.
/// This one catches the variant that defers the transmit to the `Proven`'s
/// **drop**, which is the shape a `Drop` impl written for symmetry with
/// "close on drop" would produce, and which no assertion taken before
/// `accept()` can see.
///
/// The dialler's `Connecting` must also still be in flight afterwards: a
/// rejected chain leaves the initiator retransmitting (§5.5), not resolved.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn dropping_a_proven_transmits_nothing() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6041);
            let b = Node::spawn(&net, 2, 6042);

            // The dial is pinned and never polled to completion: §16.3 puts
            // the retransmit train and every core on the **driver task**, so
            // the handshake progresses whether or not the handle is polled.
            // Keeping the `Connecting` alive is what matters — dropping it
            // would cancel the attempt (ruling 50) and make this test a test
            // of something else.
            let dial = a.ep().connect(b.addr, b.pk).expect("dial");
            let mut dial = pin!(dial);

            let intro = b.ep().accept().await.expect("accept() returned None");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");

            let before = sent_count(&tap, b.addr);
            drop(proven);
            assert_eq!(
                sent_count(&tap, b.addr),
                before,
                "§6.2: dropping a Proven is a SILENT reject — not even on Drop"
            );
            assert_eq!(
                b.dhs.get(),
                2,
                "§6.1: the rejection does not run ee/se on the way out"
            );

            // Give the drop a full driver turn to misbehave in, then check
            // again: "nothing in the same instant" is the degenerate form.
            let before = sent_count(&tap, b.addr);
            tokio::time::sleep(SHELL_LATENESS_BOUND).await;
            assert_eq!(
                sent_count(&tap, b.addr),
                before,
                "§6.2: nothing is transmitted on a driver turn after the rejection either"
            );

            assert!(
                poll_once(dial.as_mut()).await.is_pending(),
                "§5.5: a rejected chain leaves the initiator retransmitting, not resolved"
            );
        })
        .await;
}

/// **§16.3 — a staged object's verb is a round-trip to a driver it does not
/// keep alive.**
///
/// §16.3 (4304–4311): "The consequence is that **`ConnectError` needs no
/// `EndpointDropped`** … The asymmetry with `IntroError`, `AuthError` and
/// `AcceptError` — which all carry `EndpointDropped` — is therefore correct
/// and not an omission: a staged object's verb is a **round-trip to a
/// driver it does not keep alive**, so the driver can stop underneath it;
/// an outbound attempt keeps its own driver running."
///
/// # The mutation this catches
///
/// A shell that counts staged objects as handles — the symmetric-looking
/// choice, and the one ruling 62 explicitly declines. Under it, the driver
/// survives on the strength of an `Intro` the application has forgotten
/// about, and `read_identity()` succeeds here instead of reporting
/// `EndpointDropped`. Nothing else in this file or in
/// `tests/story_lifecycle.rs` distinguishes the two, because the difference
/// is invisible while any real handle lives.
///
/// It is also the constructive proof that `IntroError::EndpointDropped` is
/// reachable at all: §18.1 lists the variant, and a variant no test can
/// reach is a variant nothing pins.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_staged_object_does_not_keep_the_driver_alive() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6051);
            let mut b = Node::spawn(&net, 2, 6052);

            // Kept alive so A's driver is not the thing that stops.
            let _dial = a.ep().connect(b.addr, b.pk).expect("dial");

            let intro = b.ep().accept().await.expect("accept() returned None");

            // Every handle B holds, gone. The `Intro` is not one.
            b.ep = None;

            let outcome = tokio::time::timeout(SHELL_LATENESS_BOUND, intro.read_identity())
                .await
                .expect(
                    "§16.3: the staged verb is a round-trip to a driver that has stopped — \
                 it must resolve, not hang",
                );

            assert!(
                matches!(outcome, Err(IntroError::EndpointDropped)),
                "§16.3/§18.1: with the driver stopped underneath it, read_identity() \
                 reports EndpointDropped. A build in which this succeeds is one that \
                 treats a staged object as a handle, which ruling 62 declines"
            );
        })
        .await;
}

/// **Ruling 49 — a send failure is traced, never acted on: the connection
/// survives and the death, when it comes, is the ordinary receive-driven
/// `TimedOut`.**
///
/// Appendix B: "Assert the connection **survives** — no teardown, no verb
/// resolving with an error, no notification … Then hold the failure past
/// `DEAD_TIMEOUT` and assert the death is still the ordinary
/// receive-driven `TimedOut` (§7.4)."
///
/// §16.3's reasoning is the thing being pinned: "A failed send is not
/// authoritative. Liveness in this protocol is **receive-driven by ruling**
/// (§7.4) — a connection dies because nothing authenticated arrived, never
/// because something failed to leave."
///
/// # The mutation this catches, and both sides of it
///
/// * **Acting on the failure** — killing the connection, or resolving a
///   verb with an error, when `send_to` returns `Err`. §16.3 says this
///   "would convert the exact scenario the migration guarantee exists for
///   into a teardown — it would **delete** the guarantee, not implement an
///   error path." Caught by the first assertion: nothing may happen in the
///   first `DEAD_TIMEOUT − 1 s`, which is far longer than any I/O reaction
///   would take.
/// * **Not dying at all** — a build that suppresses the liveness deadline
///   while sends are failing, e.g. by treating a failed send as "we are
///   still trying". Caught by the second.
///
/// The variant assertion is the third side: `TimedOut`, not some new I/O
/// death — §16.3 is explicit that "§18.1's taxonomy stays closed and gains
/// no I/O variant."
///
/// The peer is retired by ruling 88's silent drop so the silence is
/// genuine; see `s26_coincident_last_handle_drop_transmits_nothing` in
/// `tests/story_lifecycle.rs`.
///
/// **G7:** the *traced* half of the obligation — a `slither::io` event per
/// failed send, carrying the destination address — is not assertable from
/// `tests/`; see the module doc.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failing_send_does_not_kill_the_connection_and_the_death_stays_timed_out() {
    let net = Network::new();
    let policy = FlakyPolicy::perfect();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn_with(&net, 1, 6061, Some(policy.clone()));
            let mut b = Node::spawn(&net, 2, 6062);
            let (ca, cb) = establish(&a, &b).await;

            let mut watch = pin!(ca.closed());

            // The seam breaks, and the peer goes away silently (ruling 88).
            policy.fail_sends(true);
            b.ep = None;
            drop(cb);

            let early =
                tokio::time::timeout(DEAD_TIMEOUT - Duration::from_secs(1), watch.as_mut()).await;
            assert!(
                early.is_err(),
                "ruling 49: a failing send_to is traced, NOT acted on — the connection \
                 must survive it. It died with {early:?}"
            );
            assert!(
                ca.is_established(),
                "ruling 49: no teardown, and no accessor may report otherwise"
            );

            let lost = tokio::time::timeout(
                Duration::from_secs(1) + SHELL_LATENESS_BOUND,
                watch.as_mut(),
            )
            .await
            .expect("§7.4: the receive-driven liveness deadline must still fire");

            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§7.4/§18.1: the death is the ordinary receive-driven TimedOut; the \
                 taxonomy gains no I/O variant"
            );
        })
        .await;
}

/// **Ruling 49 — a `close()` whose CLOSE cannot leave still reports
/// `LocallyClosed`.**
///
/// §16.2 makes `close()` infallible (`pub async fn close(&self, code: u64,
/// reason: &[u8])` — no `Result`), and §15.2 makes the local surface
/// `LocallyClosed`. §16.3 forbids acting on a send failure. Composed: a
/// `close()` over a broken seam resolves, surfaces `LocallyClosed`, and the
/// peer learns nothing — it waits out `DEAD_TIMEOUT`, which §15.4's rows
/// already accept as the cost of a lost signal.
///
/// # The mutation this catches
///
/// A driver that treats the CLOSE as the one send worth retrying or
/// reporting — plausible, because it is the one packet whose loss has a
/// visible 25 s cost, and §15.2's linger reply rule really is "CLOSE's only
/// reliability mechanism". The temptation is to make `close()` wait for the
/// send, or to surface the I/O error somewhere. Both are forbidden:
/// `close()` "resolves once the CLOSE frame is sealed" (§16.2) — **sealed**,
/// not sent — and §18.1 gains no I/O variant.
///
/// The peer-side assertion is the half that makes it non-vacuous: the peer
/// must **not** report `PeerClosed`, which proves the packet genuinely did
/// not leave and that the local `LocallyClosed` was not merely the happy
/// path in disguise.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn close_over_a_broken_seam_still_surfaces_locally_closed() {
    let net = Network::new();
    let policy = FlakyPolicy::perfect();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn_with(&net, 1, 6071, Some(policy.clone()));
            let b = Node::spawn(&net, 2, 6072);
            let (ca, cb) = establish(&a, &b).await;

            policy.fail_sends(true);

            let mut peer = pin!(cb.closed());
            let watch = pin!(ca.closed());
            let (lost_a, ()) = tokio::join!(watch, ca.close(0x21, b"into the void"));

            assert_eq!(
                lost_a,
                ConnectionLost::LocallyClosed,
                "§15.4: our own side sees LocallyClosed whether or not the CLOSE left"
            );

            let peer_view = tokio::time::timeout(Duration::from_secs(1), peer.as_mut()).await;
            assert!(
                peer_view.is_err(),
                "the CLOSE could not leave, so the peer must learn nothing here — it \
                 observed {peer_view:?}, which means the seam was not actually broken \
                 and this test asserts nothing"
            );
        })
        .await;
}

/// **Ruling 50, the peer-side half — a cancelled dial leaves the peer a
/// silent half-open session that is reaped at `DEAD_TIMEOUT`.**
///
/// Appendix B: "let the peer answer, drop the `Connecting` after its msg2
/// is on the wire, and assert the peer's half-open session **transmits
/// nothing** and dies at `DEAD_TIMEOUT` (25 s) — ruling 39's reap case
/// (§7.4), which is the cost this ruling accepts."
///
/// §16.3 states the mechanism: "§7.4's install pin arms the death deadline
/// at install and sets `last_send` equal to `last_authenticated_recv`, so a
/// session that receives nothing after install **emits nothing at all** and
/// is reaped in silence."
///
/// # The mutations this catches
///
/// * **A session that chatters** — the natural bug is a keepalive beacon
///   armed at install, which turns every abandoned dial into 25 s of
///   traffic toward a peer that is not there. §7.4's install pin exists to
///   forbid exactly that, and only the `sent_count` assertion sees it.
/// * **A session never reaped** — the half-open state leaks for the life
///   of the process. Caught by the `TimedOut` assertion.
/// * **A session reaped on the wrong clock** — `KEEPALIVE_TIMEOUT` (10 s)
///   is the other candidate; caught by the "still alive at
///   `DEAD_TIMEOUT − 1 s`" half.
///
/// # G10 — the literal ordering Appendix B specifies is not separable here
///
/// Appendix B says "drop the `Connecting` **after** its msg2 is on the
/// wire". On the in-memory fabric there is no latency between msg2 leaving
/// the responder and the initiator's driver completing the pending, so by
/// the time a test can observe msg2 in the tap the attempt has already
/// completed — and dropping the `Connecting` then drops a **completed**
/// `Connection`, which §16.2 turns into `close(NO_ERROR, "")` and which
/// would make this test assert the opposite of what it is for. Separating
/// them needs directional loss injection ("lose the next inbound datagram
/// at A"), which is a `FlakyPolicy` capability this author will not invent.
///
/// What is written instead reaches the **identical peer-side state** by
/// §16.3's own other sentence: "A msg2 racing the drop arrives after the
/// pending index is gone: it routes by index to nothing and is inert
/// (§17.3's corollary), so no session is installed on our side and no state
/// is resurrected." The dial is cancelled first, the responder accepts
/// afterwards, and the responder is left holding exactly the half-open
/// session ruling 39 reaps. The tap assertion below pins that msg2 really
/// was emitted, so the case is not the trivially-empty one.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn cancelled_dial_leaves_the_peer_a_silent_half_open_session() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6081);
            let b = Node::spawn(&net, 2, 6082);

            let dial = a.ep().connect(b.addr, b.pk).expect("dial");

            // The initiation is parked at B (§6.3). Abandon the attempt
            // before B answers: A's pending and its index are gone.
            let cancelled = tokio::time::timeout(Duration::from_secs(1), dial).await;
            assert!(cancelled.is_err(), "the responder has not answered yet");

            // B now walks the chain on the parked initiation and installs a
            // session, answering into the void.
            let accepted = {
                let intro = b.ep().accept().await.expect("accept() returned None");
                let claimed = intro.read_identity().await.expect("read_identity");
                let proven = claimed.authenticate().await.expect("authenticate");
                proven.accept().await.expect("accept")
            };

            assert_eq!(
                msg2_count(&tap, b.addr),
                1,
                "the peer must have answered, or this test asserts nothing"
            );

            let mut watch = pin!(accepted.closed());
            let at_drop = sent_count(&tap, b.addr);

            let early =
                tokio::time::timeout(DEAD_TIMEOUT - Duration::from_secs(1), watch.as_mut()).await;
            assert!(
                early.is_err(),
                "§7.4: the half-open session is reaped at DEAD_TIMEOUT, not sooner — \
                 it died with {early:?}"
            );
            assert_eq!(
                sent_count(&tap, b.addr),
                at_drop,
                "§7.4's install pin: a session that receives nothing after install \
                 emits NOTHING AT ALL — no keepalive, no probe"
            );

            let lost = tokio::time::timeout(
                Duration::from_secs(1) + SHELL_LATENESS_BOUND,
                watch.as_mut(),
            )
            .await
            .expect("ruling 39's reap case must actually fire");

            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§15.4's liveness row: the reap surfaces TimedOut"
            );
        })
        .await;
}

// ══════════════════════════════════════════════════════════════════════
// Post-slice-3b seam-review regressions.
//
// Three findings from the chartered review of `src/shell/`. Each is
// written from the side working rule 9 asks for: the assertion that
// *separates* the fixed build from the broken one, with the broken build's
// behaviour named in the doc comment.
// ══════════════════════════════════════════════════════════════════════

/// **The drain contract holds across the wire's yield: a CLOSE sealed while
/// `send_to` is suspended still reaches the peer.**
///
/// §16.4: `poll_output()`'s terminal `Timeout` "is simultaneously the drain
/// sentinel and the next-deadline announcement", and the core's
/// `poll_output` **pops** — for `core::Connection` it is
/// `self.outputs.pop_front().unwrap_or_else(…)`. So reading the deadline is
/// only a pure read while the queue is provably empty, and the only thing
/// that establishes that is a drain with **no intervening yield**.
///
/// # The mutation this catches
///
/// The driver's loop was
///
/// ```text
/// serve()  →  transmit(outgoing).await  →  handles check  →  deadline()  →  select!
/// ```
///
/// and `transmit()` is the one yield point between the drain and
/// `deadline()`. §16.3 puts `close()` on the **handle** side of the seam
/// (ruling 53): it mutates the core on the *caller's* stack. So a `close()`
/// that lands while the driver is suspended inside `send_to` queues
/// `Transmit(CLOSE)` on a core the driver is about to call `poll_output()`
/// on outside a drain — and `deadline()` popped it and threw it away. In
/// release that is a silently lost CLOSE and 25 s of `DEAD_TIMEOUT` for the
/// peer, which is precisely the cost §15.1 says CLOSE exists to avoid; in
/// debug the `debug_assert!` beside it panicked the driver instead.
///
/// The fix is to read the deadline **before** the yield, where the drain
/// has just finished and `deadline()`'s own doc comment already claimed it
/// was — every handle-side mutation sends a command, and the command arm is
/// `biased` first, so a deadline made stale during the yield is recomputed
/// on the very next iteration rather than obeyed.
///
/// # Why the assertion separates them
///
/// The broken build loses `c_to_a`'s CLOSE: peer C learns nothing and is
/// reaped at `DEAD_TIMEOUT` with `TimedOut`, so the `PeerClosed { code: 2 }`
/// assertion fails in **release**; in **debug** the driver panics before
/// the CLOSE can leave and — since the panic now runs the stop path — C's
/// peer handle still never sees `PeerClosed`. Red in both profiles.
///
/// The B assertion is the control: B's CLOSE was already in `outgoing` when
/// the yield happened, so it survives *either* build. A test that asserted
/// only on B would pass against the broken driver. Two connections are also
/// what makes the case reachable at all — with one, there is no second core
/// to hold an undrained output.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let gate = Gate::opened();
            let a = Node::spawn_over(
                1,
                6091,
                GatedWire {
                    inner: net.wire(addr(6091)),
                    gate: Rc::clone(&gate),
                },
            );
            let b = Node::spawn(&net, 2, 6092);
            let c = Node::spawn(&net, 3, 6093);

            let (a_to_b, b_to_a) = establish(&a, &b).await;
            let (a_to_c, c_to_a) = establish(&a, &c).await;

            // The socket's send buffer fills up.
            gate.close();

            // Sealed and drained into `outgoing`; the driver then suspends
            // inside `send_to` with the CLOSE in hand.
            a_to_b.close(1, b"bee").await;
            settle().await;

            // THE INTERLEAVING: a second connection's CLOSE is sealed —
            // synchronously, on this task, per ruling 53 — while the driver
            // is parked mid-send. Its core now holds an undrained
            // `Transmit`, which is the state `deadline()` must not consume.
            a_to_c.close(2, b"cee").await;

            // The buffer drains.
            gate.open();
            settle().await;

            let peer_b = tokio::time::timeout(SHELL_LATENESS_BOUND, b_to_a.closed())
                .await
                .expect("the control: B's CLOSE was already in flight when the wire stalled");
            assert_eq!(
                peer_b,
                ConnectionLost::PeerClosed {
                    code: 1,
                    reason: b"bee".to_vec(),
                },
                "the control connection must be unaffected, or this test asserts nothing"
            );

            let peer_c = tokio::time::timeout(SHELL_LATENESS_BOUND, c_to_a.closed())
                .await
                .expect(
                    "§16.4: a CLOSE sealed while the wire was suspended must still be \
                     drained and sent. A driver that reads its deadline out of \
                     poll_output() *after* a yield pops this datagram and discards it, \
                     and the peer then waits out DEAD_TIMEOUT for nothing",
                );
            assert_eq!(
                peer_c,
                ConnectionLost::PeerClosed {
                    code: 2,
                    reason: b"cee".to_vec(),
                },
                "§15.1: the CLOSE exists to spare the peer its 25 s liveness deadline"
            );
        })
        .await;
}

/// **§16.1's accept-vs-connect race resolves to
/// `ConnectError::AlreadyConnected`, and the driver survives it.**
///
/// §16.1 declines to prevent this race and says so: "a **staged chain in
/// progress is deliberately not in `connect()`'s list**, and cannot be:
/// until `authenticate()` the chain's static is merely claimed, and §6.1
/// forbids keying anything durable on an unproven claim. The invariant is
/// held at the other end of that race instead — a proven static that is
/// PENDING at `accept()` runs §6.7's comparison … Either way … **no static
/// is ever LIVE and PENDING at once.**"
///
/// So a `connect()` issued between `Proven::accept()`'s command and the
/// driver's next turn is *expected* to be admitted by the shell's
/// synchronous map and refused by the core one command later. S3a fixes the
/// answer: `connect()` to a static that already has a live `Connection`
/// returns `Err(ConnectError::AlreadyConnected)`.
///
/// # The mutations this catches
///
/// * **The `debug_assert!(false, "the shell's static map admitted a connect
///   the core refused")` this replaces.** It asserted the two maps could not
///   disagree, which §16.1 says they can. In any debug build — every
///   `cargo test`, every consumer's dev build — one unlucky `join!`
///   detonated it, the driver task unwound, and the `LocalSet` swallowed
///   the panic. Caught twice over: the `Connecting` resolves
///   `ConnectError::Local` rather than `AlreadyConnected` once the stop
///   path runs, and the liveness phase below finds a dead driver.
/// * **A `release_static` that removed by key instead of by stamp.** The
///   losing `connect()` releases its own claim on the way out. Without the
///   stamp check it would delete the *accept's* newer LIVE entry, leaving
///   the mirror NONE while the core holds the static LIVE — a permanent,
///   silent divergence in the dangerous direction. Caught by the third
///   `connect()`, which reads the mirror and must still refuse.
///
/// # Why the liveness assertion is what it is
///
/// A stopped driver keeps serving the last values it wrote into the shared
/// cell, so `is_established()`, `remote_address()` and `session_id()` all
/// keep answering on a frozen endpoint — the naive liveness check passes
/// against exactly the build this test exists to fail. The assertion is
/// therefore **wire-observable work**: A closes the accepted connection and
/// B's independent handle must see `PeerClosed`, which cannot happen unless
/// A's driver drained the seal and put the datagram on the wire.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s3a_accept_ahead_of_connect_resolves_already_connected() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 6101);
            let b = Node::spawn(&net, 2, 6102);

            // B dials A, so A has a chain from B to walk.
            let b_dial = b.ep().connect(a.addr, a.pk).expect("B dials A");
            let intro = a.ep().accept().await.expect("accept() returned None");
            let claimed = intro.read_identity().await.expect("read_identity");
            let proven = claimed.authenticate().await.expect("authenticate");

            // THE RACE. `Proven::accept()` queues `Command::AcceptChain(K_b)`
            // on its first poll and yields; `connect()` is synchronous
            // (ruling 87) and runs before the driver's next turn, so the
            // command queue is `[AcceptChain(K_b), Connect(K_b)]` while the
            // shell's mirror still says NONE for K_b — truthfully, because
            // §16.1 keeps a staged chain out of that map on purpose.
            let (accepted, dialled) =
                tokio::join!(proven.accept(), async { a.ep().connect(b.addr, b.pk) });

            let a_to_b = accepted.expect("§6.7: the proven chain wins the race and installs");
            let dialled = dialled.expect(
                "the shell's map answers from the instant of the call, and at that \
                 instant nothing had landed for this static — so the verb is admitted",
            );

            settle().await;

            let mut dialled = pin!(dialled);
            let outcome = poll_once(dialled.as_mut()).await;
            let Poll::Ready(outcome) = outcome else {
                panic!(
                    "the refused attempt must already be resolved: the driver answers it \
                     synchronously in the same command it fails. Still Pending means the \
                     driver never got to it — which is what a driver that panicked on the \
                     divergence looks like from here"
                );
            };
            assert!(
                matches!(outcome, Err(ConnectError::AlreadyConnected)),
                "S3a: connect() to a static that already has a live Connection returns \
                 AlreadyConnected. Got {:?} — ConnectError::Local here means the driver \
                 died and the stop path answered instead of the core",
                outcome.err(),
            );

            // The stamp check, from the outside: the losing connect()
            // released its own claim on the way out, and must NOT have
            // taken the accept's newer entry with it. This read is the
            // mirror, synchronously (ruling 87).
            let third = a.ep().connect(b.addr, b.pk);
            assert!(
                matches!(third, Err(ConnectError::AlreadyConnected)),
                "release_static is stamp-checked: the accept drew the later attempt \
                 stamp, so the losing connect's release must decline. An Ok here means \
                 the mirror was cleared while the core still holds the static LIVE — \
                 the divergence that never heals. Got {:?}",
                third.err(),
            );

            // ── liveness, the only way that cannot be faked ──────────
            // B's dial resolved into a real connection when A accepted;
            // both peers now have handles to one session.
            let b_to_a = b_dial
                .await
                .expect("B's dial completed when A accepted its chain");

            let mut peer = pin!(b_to_a.closed());
            a_to_b.close(7, b"still here").await;
            settle().await;

            let seen = poll_once(peer.as_mut()).await;
            assert_eq!(
                seen,
                Poll::Ready(ConnectionLost::PeerClosed {
                    code: 7,
                    reason: b"still here".to_vec(),
                }),
                "the CLOSE only reaches B if A's driver drained the seal and put the \
                 datagram on the wire after the race. A frozen driver still answers \
                 every accessor from its stale cell, so nothing cheaper than this \
                 separates the two builds"
            );
        })
        .await;
}

/// **A driver that panics resolves every waiter instead of parking it.**
///
/// §16.3 makes the driver a single task, and `tokio::task::spawn_local`
/// stores its panic in a `JoinHandle` the shell drops — so a driver panic
/// is **invisible**: nothing propagates, and the test harness prints `ok`.
/// Before the `Drop` guard this test pins, that silence was permanent as
/// well as invisible. `Driver::stop` — which sets `driver_stopped`, latches
/// `ConnectionLost::EndpointDropped` over every connection and drops the
/// parked `accept()` senders — ran only on the loop's ordinary exit.
///
/// # What the broken build does, waiter by waiter
///
/// With no unwind guard, an unwind skips `stop()` and leaves
/// `driver_stopped == false`. Then:
///
/// * `Connection::closed()` parks in `closed_wakers` with nobody left to
///   wake it — **for ever**;
/// * a `Connecting` whose `Command::Connect` the driver *did* process parks
///   in its `PendingSlot` — the slot's `Rc` is dropped with the record and
///   never resolved;
/// * a `Connecting` whose `Command::Connect` is *still in the channel* parks
///   the same way, and is worse: no record ever named it, so even a
///   record-sweeping `stop()` misses it unless the channel is drained;
/// * `is_established()` keeps answering `true` from a cell nobody will
///   write again, and `Endpoint::connect` keeps handing out fresh
///   `Connecting`s that can never resolve.
///
/// Only the `oneshot`-backed verbs degrade on their own, because their
/// senders die with the `Driver` — which is why the failure looks like
/// "some things still work" rather than "the endpoint is dead", and why it
/// went unnoticed.
///
/// # Why the assertions separate the builds
///
/// Every one is a `poll_once` with **no clock advance and no timeout**. The
/// fixed build resolves each waiter synchronously during the unwind, so
/// `Ready` is available on the next poll; the broken build answers
/// `Pending` to all four, for ever. A `timeout()`-shaped assertion would
/// also pass, but slowly and only by exhausting virtual time — this states
/// the property directly.
///
/// Two dials are issued back-to-back on purpose. The driver handles exactly
/// one command per loop iteration, so the first becomes a `ConnRecord` and
/// the second is still sitting in the command channel when the panic lands.
/// They are the two different parking places above, and a `stop()` that
/// only swept its own records would leave the second hung.
///
/// **`cargo test -- --nocapture` prints this driver's panic and still
/// reports `ok`.** That is the behaviour under test, not a failure of it —
/// and the reason the finding survived a green suite for a whole slice.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_panicking_driver_resolves_every_waiter_instead_of_parking_it() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let armed = Rc::new(Cell::new(false));
            let a = Node::spawn_over(
                1,
                6111,
                PanicWire {
                    inner: net.wire(addr(6111)),
                    armed: Rc::clone(&armed),
                },
            );
            let b = Node::spawn(&net, 2, 6112);

            let (a_to_b, _b_to_a) = establish(&a, &b).await;

            // A dial with nobody at the other end: still in §5.5's train.
            let early = a
                .ep()
                .connect(addr(6119), absent_static(9))
                .expect("dial to a NONE static");
            settle().await;

            // Parked waiters of every kind the shell has.
            let mut watch = pin!(a_to_b.closed());
            let mut early = pin!(early);
            let mut listening = pin!(a.ep().accept());
            assert!(
                poll_once(watch.as_mut()).await.is_pending(),
                "the connection is healthy, so closed() must be parked"
            );
            assert!(
                poll_once(early.as_mut()).await.is_pending(),
                "nobody is answering 6119, so the attempt must be parked"
            );
            assert!(
                poll_once(listening.as_mut()).await.is_pending(),
                "nobody is dialling A, so accept() must be parked"
            );

            // The next send kills the driver.
            armed.set(true);

            // Two more dials. The driver handles one command per loop
            // iteration, so `first` gets a ConnRecord and its msg1 reaches
            // the wire — which panics — while `second` is still queued.
            let mut first = pin!(
                a.ep()
                    .connect(addr(6117), absent_static(7))
                    .expect("dial while the driver still lives")
            );
            let mut second = pin!(
                a.ep()
                    .connect(addr(6118), absent_static(8))
                    .expect("dial while the driver still lives")
            );
            settle().await;

            assert_eq!(
                poll_once(watch.as_mut()).await,
                Poll::Ready(ConnectionLost::EndpointDropped),
                "§15.4's endpoint-dropped row: a driver that died under a live \
                 connection must resolve closed(), not park it for ever"
            );

            for (name, mut attempt) in [
                ("the attempt in flight before the panic", early.as_mut()),
                ("the attempt whose Connect was processed", first.as_mut()),
                ("the attempt still queued in the channel", second.as_mut()),
            ] {
                let outcome = poll_once(attempt.as_mut()).await;
                let Poll::Ready(outcome) = outcome else {
                    panic!("{name} parked for ever: the stop path never reached its slot");
                };
                assert!(
                    matches!(outcome, Err(ConnectError::Local)),
                    "ruling 62: ConnectError has no EndpointDropped, and a failure with \
                     no DH spent on the caller's behalf is what Local names — the same \
                     answer connect() gives once driver_stopped is set. {name} got {:?}",
                    outcome.err(),
                );
            }

            assert!(
                matches!(poll_once(listening.as_mut()).await, Poll::Ready(None)),
                "§16.2: `None` = endpoint closed. A failed driver is the reachable \
                 cause TEST-B's G9 could not find — see FIXES-3b.md §2a, which files \
                 the wording as a ruling candidate rather than assuming it"
            );

            // And the endpoint refuses new work rather than pretending.
            assert!(
                matches!(
                    a.ep().connect(addr(6116), absent_static(6)),
                    Err(ConnectError::Local)
                ),
                "driver_stopped is what stops connect() handing out Connectings that \
                 can never resolve, and only the stop path sets it"
            );
        })
        .await;
}

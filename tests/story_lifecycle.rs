//! **Connection lifecycle — S1, S26, S27's `closed()` half, S29.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S1** *(maintainer's #1, open since slice 0)* | dial by static + address; get a live connection; close it cleanly; **both sides observe the close**. Cost: 4 DH on the initiator. |
//! | **S26** | teardown fires when **every handle** is dropped — *not* when the `Endpoint` is dropped. Drop-order sensitive, and the opposite of the obvious guess. |
//! | **S27** *(`closed()` half)* | `closed().await` resolves with the `ConnectionLost` reason **with no verb in flight**, and the signal is **latched**. |
//! | **S29** | give up on a dial and immediately redial: the cancellation is ordered ahead of the next endpoint verb, and a retry loop **replaces rather than accumulates**. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by **TEST-B** from `STORIES.md`, `SPEC.md` (§5.4–§5.6, §6.2,
//! §15.1–§15.4, §16.1–§16.3, §16.9, §16.10, Appendix B) and
//! `.slices/03-skeleton/PLAN.md` §5.2 / T10–T12 **alone**, while IMPL-B
//! wrote the shell concurrently. No file under `src/shell/` and no line of
//! `src/testutil/mod.rs` was read. Names for shell and harness items are
//! **proposals** — the integrator renames *calls*, never assertions.
//!
//! # Spec gaps found while writing this file (working rule 8)
//!
//! Recorded here rather than resolved, and **checked against every test in
//! this file** before finishing — slice 3a's author found a gap, routed one
//! test around it, and then wrote another test that depended on it.
//!
//! * **G1 — §16.3's "holding a `closed()` future while dropping every
//!   `Connection`" is not constructible.** §16.3 (4289–4292) distinguishes
//!   a `closed()` future from a handle: "a `closed()` future is not a
//!   handle, and holding one while dropping every `Connection` still stops
//!   the driver and still kills the session silently." But §16.2 declares
//!   `pub async fn closed(&self) -> ConnectionLost`, and an `async fn`'s
//!   future **captures `&self`** — so the future cannot outlive the
//!   `Connection` it was created from, and the state the sentence
//!   describes cannot be reached from safe Rust. Either the sentence is a
//!   statement about the *borrow* (in which case the compiler already
//!   enforces it and there is nothing to test), or `closed()` is meant to
//!   return an owned `'static` future and the signature is incomplete.
//!   `.slices/03-skeleton/PLAN.md` §5.2 asks for exactly this as "the
//!   negative" inside `s26_a_connecting_alone_keeps_the_driver_alive`;
//!   **this file does not write it**, and says why here instead of
//!   inventing a signature. The *positive* half of ruling 62 — a
//!   `Connecting` alone keeps the driver alive — **is** constructible and
//!   is tested, which is itself evidence the two objects differ.
//! * **G2 — ruling 62 constrains `Connecting`'s lifetime, and the spec
//!   never says so.** "The driver lives while a `Connecting` lives, and
//!   dropping the last `Connecting` — **with no `Endpoint` and no
//!   `Connection` outstanding** — stops it" (§16.3, 4294–4297) is
//!   reachable only if `Connecting` does **not** borrow the `Endpoint` it
//!   came from. §16.2 writes `pub fn connect(&self, …) -> Result<Connecting,
//!   ConnectError>` with no lifetime, which is consistent with that but
//!   does not state it. `s26_a_connecting_alone_keeps_the_driver_alive`
//!   fails to *compile* if `Connecting` borrows — a compile error there is
//!   this gap surfacing, not a bug in the test. The same derivation applies
//!   to the staged objects: §16.3's "a staged object's verb is a
//!   round-trip to a driver it does not keep alive" requires `Intro`,
//!   `Claimed` and `Proven` to be owned too.
//! * **G3 — §15.4's local-close row and ruling 88's coincident drop are
//!   two rows that one test cannot separate.** Ruling 88 settles the
//!   coincidence; what makes it a *ruling* rather than a tautology is that
//!   the non-coincident case behaves the opposite way. Both are written
//!   here, as a pair, because either one alone is satisfied by a build that
//!   never seals a CLOSE on drop at all.
//! * **G4 — the `closed()` rows slice 3 cannot reach** (this is the plan's
//!   C5, restated so the file does not *look* like it discharges Appendix
//!   B). Appendix B requires driving **each** teardown row. Reached here:
//!   `PeerClosed`, `LocallyClosed`, `TimedOut` (the silence case).
//!   **Not reached, and owed:**
//!   - `TimedOut` *via the contested verdict* (§7.5) — **slice 7**.
//!   - `Replaced` (§6.4) — **slice 7**. `src/core/endpoint/mod.rs`'s own
//!     module doc records that slice 2a returns `AcceptError::Stale` for
//!     the LIVE and PENDING branches and that "slice 7 replaces the arm",
//!     so no shell can surface `Replaced` yet.
//!   - `ProtocolViolation` (§8.2, §15.2) — needs an **authenticated**
//!     malformed frame, which cannot be minted from an integration test:
//!     the session keys are inside the crate. It is reachable only from an
//!     in-crate test module, and this file cannot discharge it. Named so
//!     the absence is coverage, not a claim.
//!   - `NonceExhausted` (§7.9) — needs 2⁶⁴ seals or a core-side epoch
//!     override; in-crate only.
//! * **G5 — `NO_ERROR` has no name in the crate.** §15.3's registry is six
//!   wire codes and `src/constants.rs` names only `APPLICATION_ERROR_BASE`
//!   (the plan's U3). This file writes `0x00` with the citation beside it;
//!   if slice 3 introduces the registry as a type, the integrator swaps the
//!   literal.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::sleep` appears only as a **clock advance** on the paused
//! clock, never as a wait for work to happen. `tokio::time::timeout` is the
//! observation instrument: `timeout(d, &mut fut).await.is_err()` asserts
//! `fut` was still `Pending` at `now + d`, which is the "not before" half
//! that one-sided tests omit.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use slither::config::Config;
use slither::constants::{
    CLOSE_REASON_MAX, DEAD_TIMEOUT, INIT_PACKET_LEN, PKT_HANDSHAKE_INIT, RETRANSMIT_BASE,
    SHELL_LATENESS_BOUND,
};
use slither::error::{AcceptError, ConnectionLost};
use slither::identity::{Identity, PublicKeyOf};
use slither::testutil::{CountingIdentity, DhCounter, Network, Tap};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE — proposed names, flagged loudly.
//
// `.slices/03-skeleton/PLAN.md` §2.2 gives the two-endpoint `LocalSet`
// harness to IMPL-B and this file to TEST-B, and notes that the two briefs
// must agree on the harness API. They could not: it was being written as
// this file was. The block below is therefore a **local** fixture over the
// three names §16.10 ratifies as contract (`Network`, `FlakyWire`,
// `FlakyPolicy`) plus the public `Endpoint::builder()`.
//
// INTEGRATOR: redirect this block at IMPL-B's harness if one fits.
// Everything below the `── tests ──` line reaches the shell only through
// `Node`, `establish` and `msg1_count`. Every assertion is on protocol
// behaviour and must survive the rename **untouched**.
//
// Proposed here and ratified nowhere:
//   * `slither::Endpoint` / `slither::Connection` at the crate root — if
//     the re-export lands under `shell`, prefix `shell::`.
//   * `Endpoint::builder().identity(..).wire(..).config(..).build()`.
//   * `Network::new()`, `Network::wire(addr) -> FlakyWire`,
//     `Network::tap() -> Tap`, `Tap::datagrams() -> Vec<(from, to, bytes)>`.
//   * `CountingIdentity::seeded([u8; 32])` / `.counter() -> DhCounter` —
//     these two are *not* guesses; `src/core/tests.rs` already uses them.
// ══════════════════════════════════════════════════════════════════════

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::Endpoint<Id>;
// §16.2 writes a bare `Connection`, but `core::Connection<C: Handshake>` is
// suite-parameterised, so the shell's almost certainly is too. **This alias
// is the single line to change** if it turns out to be `Connection<Id>`, or
// genuinely bare: no test below names the type.
type Connection = slither::Connection<Suite>;

/// §15.3's graceful-close code. See G5.
const NO_ERROR: u64 = 0x00;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

struct Node {
    /// `Option` so a test can drop the `Endpoint` handle **without**
    /// dropping the bookkeeping beside it — S26 turns on exactly that.
    ep: Option<Endpoint>,
    dhs: DhCounter,
    pk: Pk,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
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

fn absent_static(key_seed: u8) -> Pk {
    let id: Id = CountingIdentity::seeded([key_seed; 32]);
    *id.public_static()
}

/// `dialler` connects to `listener`, which walks §6.2's staged chain to
/// completion. Returns `(initiator side, responder side)`.
///
/// The two halves are `join!`ed rather than sequenced because the responder
/// cannot produce msg2 until the application drives the chain, and the
/// initiator's `Connecting` cannot resolve until msg2 arrives — sequencing
/// them deadlocks, which is itself §6.2's point.
async fn establish(dialler: &Node, listener: &Node) -> (Connection, Connection) {
    let dial = dialler
        .ep()
        .connect(listener.addr, listener.pk)
        .expect("connect() on a NONE static must be Ok");

    let accept = async {
        let intro = listener
            .ep()
            .accept()
            .await
            .expect("§16.2: accept() yields None only when the endpoint is closed");
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

/// §3.1: byte 0 is the packet type and the length gate is **exact** for the
/// handshake types — both are checked so nothing else can be miscounted.
fn msg1_count(tap: &Tap, to: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(_from, dst, bytes)| {
            *dst == to && bytes.len() == INIT_PACKET_LEN && bytes[0] == PKT_HANDSHAKE_INIT
        })
        .count()
}

/// Total datagrams the fabric carried *from* `from`, of any type. Used by
/// the "transmitted nothing" rows of §15.4, where the claim is about **all**
/// output and not only handshakes.
fn sent_count(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, _bytes)| *src == from)
        .count()
}

/// Poll `fut` exactly once and report whether it was ready on that poll.
///
/// This is the instrument for Appendix B's "**resolves immediately**" —
/// a `timeout(ZERO, …)` cannot express it, because on the paused clock a
/// zero timeout races the future rather than observing it. One poll is the
/// exact statement, and it is what separates a latch from a `Notify`.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

// ────────────────────────────── S1 ─────────────────────────────────────

/// **S1 — the maintainer's #1, end to end.**
///
/// > Dial a peer by static public key and address; get a live connection;
/// > close it cleanly; both sides observe the close.
///
/// # The mutations this catches
///
/// * **A `close()` that transmits nothing.** The peer's `PeerClosed`
///   assertion is the only thing that separates a real CLOSE frame from a
///   local teardown that merely resolves the caller's future. §15.1 exists
///   precisely because the alternative costs the peer 25 s.
/// * **A `close()` that reports the wrong side's reason.** Both variants
///   are asserted, on both sides, in the same test: `LocallyClosed` here
///   and `PeerClosed { code, reason }` there. A build that surfaces
///   `LocallyClosed` on both sides passes a one-sided test.
/// * **A DH ladder that is not §6.1's.** `assert_eq!` on 4, not `<= 4` —
///   an upper bound is satisfied by a collapsed ladder that skips `ss`,
///   which is the one DH that proves possession. S1's `Cost:` line is part
///   of the story, so this is an acceptance criterion and not a
///   performance note.
/// * **An accessor reading a stale or default cell.** `remote_address()`
///   is asserted on **both** sides and they differ (§5.6: the responder
///   anchors at the msg1 source, the initiator at the dialled address), so
///   a cell that returns one endpoint's own address, or a zero address,
///   fails.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s1_dial_then_close_both_sides_observe() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5001);
            let b = Node::spawn(&net, 2, 5002);

            let (ca, cb) = establish(&a, &b).await;

            // ── a live connection ───────────────────────────────────
            assert!(ca.is_established(), "§16.2: the initiator side is live");
            assert!(cb.is_established(), "§16.2: the responder side is live");
            assert_eq!(
                ca.remote_static().as_ref(),
                b.pk.as_ref(),
                "§16.2: remote_static() is the peer we dialled"
            );
            assert_eq!(
                cb.remote_static().as_ref(),
                a.pk.as_ref(),
                "§16.2: the responder's remote_static() is the proven initiator"
            );
            assert_eq!(
                ca.remote_address(),
                b.addr,
                "§5.5 step 4: the initiator anchors the session at the dialled address"
            );
            assert_eq!(
                cb.remote_address(),
                a.addr,
                "§5.6: the responder anchors at the initiation's msg1 source"
            );

            // ── 4 DH on the initiator side (S1's `Cost:` line) ──────
            //
            // §6.1: `es` + `ss` building msg1, then `ee` + `se` reading
            // msg2. Exact, not a ceiling.
            assert_eq!(
                a.dhs.get(),
                4,
                "§6.1: the initiator pays exactly es, ss, ee, se"
            );

            // ── close it cleanly ────────────────────────────────────
            let code = 0x2a_u64;
            let reason: &[u8] = b"story one";

            let watch_b = pin!(cb.closed());
            let watch_a = pin!(ca.closed());
            let (lost_b, lost_a, ()) = tokio::join!(watch_b, watch_a, ca.close(code, reason));

            assert_eq!(
                lost_b,
                ConnectionLost::PeerClosed {
                    code,
                    reason: reason.to_vec(),
                },
                "§15.2/§15.4: the peer surfaces PeerClosed with the same code and reason"
            );
            assert_eq!(
                lost_a,
                ConnectionLost::LocallyClosed,
                "§15.4: our own side sees LocallyClosed, never PeerClosed"
            );
        })
        .await;
}

/// **S1 — `close()` resolves once the CLOSE is sealed, not at linger
/// expiry (§16.2, rulings 81/84).**
///
/// §16.2: "`close()` resolves once the CLOSE frame is sealed and the
/// closing state is entered (§15.2)". §15.2 then holds the closing state
/// for `CLOSE_LINGER` (5 s).
///
/// # The mutation this catches
///
/// A shell that resolves the caller's `close()` future when the connection
/// state is finally dropped — i.e. `CLOSE_LINGER` later. It is a natural
/// implementation (one completion signal, fired at the end) and it is
/// wrong by a factor of twenty against `SHELL_LATENESS_BOUND`. Every other
/// test in this file passes against it, because on the paused clock a 5 s
/// wait is free.
///
/// Asserted from the separating side: **strictly inside**
/// `SHELL_LATENESS_BOUND`, which is far below `CLOSE_LINGER`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s1_close_resolves_promptly_not_at_linger_expiry() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5011);
            let b = Node::spawn(&net, 2, 5012);
            let (ca, _cb) = establish(&a, &b).await;

            let closed_in_time =
                tokio::time::timeout(SHELL_LATENESS_BOUND, ca.close(NO_ERROR, b"")).await;

            assert!(
                closed_in_time.is_ok(),
                "§16.2 + ruling 81/84: close() resolves once the CLOSE is sealed \
                 (within SHELL_LATENESS_BOUND), not after CLOSE_LINGER"
            );
        })
        .await;
}

/// **S1 — the reason is truncated at `CLOSE_REASON_MAX`, and only there.**
///
/// §16.2: `close()` "truncates `reason` at `CLOSE_REASON_MAX` (§8.4)".
///
/// # The mutations this catches
///
/// Slice 1's one real gap was a cap tested on **one** side: `LEN` and
/// `LEN − 1` were pinned and `LEN + 1` was not, so a mutation relaxing an
/// exact check to a minimum survived 187 green tests. All three sides are
/// pinned here.
///
/// * `CLOSE_REASON_MAX − 1` and `CLOSE_REASON_MAX` must arrive **whole** —
///   catches an off-by-one that truncates at the boundary.
/// * `CLOSE_REASON_MAX + 1` must arrive as exactly the first
///   `CLOSE_REASON_MAX` bytes — catches "no truncation at all" (which would
///   also blow §8.4's frame layout) *and* "truncate to something else".
///
/// The reason is a **non-constant** byte pattern rather than a repeated
/// byte, because a build that truncates by writing `CLOSE_REASON_MAX`
/// zeroes, or that reverses the buffer, passes a length-only assertion and
/// a `[0x41; N]` payload alike. The content is asserted, not just the size.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s1_close_reason_is_truncated_at_close_reason_max() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // A byte pattern with period 251 (prime, and coprime with every
            // power of two): no rotation, reversal or zero-fill of it is
            // equal to its own prefix.
            let pattern: Vec<u8> = (0..CLOSE_REASON_MAX + 8).map(|i| (i % 251) as u8).collect();

            for (i, (case, len)) in [
                ("below the cap", CLOSE_REASON_MAX - 1),
                ("exactly at the cap", CLOSE_REASON_MAX),
                ("one over the cap", CLOSE_REASON_MAX + 1),
                ("well over the cap", CLOSE_REASON_MAX + 8),
            ]
            .into_iter()
            .enumerate()
            {
                // Distinct ports per case: two cases sharing an address
                // would put two endpoints on one `Network` slot, and the
                // second establishment would silently talk to the first.
                let port = 5200 + (i as u16) * 10;
                let a = Node::spawn(&net, 1, port);
                let b = Node::spawn(&net, 2, port + 1);
                let (ca, cb) = establish(&a, &b).await;

                let sent = &pattern[..len];
                let expected = &pattern[..len.min(CLOSE_REASON_MAX)];

                let watch = pin!(cb.closed());
                let (lost, ()) = tokio::join!(watch, ca.close(9, sent));

                match lost {
                    ConnectionLost::PeerClosed { code, reason } => {
                        assert_eq!(code, 9, "{case}: the code is not truncated");
                        assert_eq!(
                            reason.len(),
                            expected.len(),
                            "{case}: §16.2 truncates the reason at CLOSE_REASON_MAX \
                             ({CLOSE_REASON_MAX}) and nowhere else"
                        );
                        assert_eq!(
                            reason, expected,
                            "{case}: the delivered reason must be the *prefix* of what \
                             was passed, byte for byte"
                        );
                    }
                    other => panic!("{case}: expected PeerClosed, got {other:?}"),
                }
            }
        })
        .await;
}

// ────────────────────────────── S26 ────────────────────────────────────

/// **S26 — dropping the `Endpoint` does not stop the driver.**
///
/// > A live `Connection` handle alone keeps the driver running after the
/// > `Endpoint` is gone.
///
/// This is S26's `⚠ CHECK`: "drop-order sensitive and the opposite of the
/// obvious guess."
///
/// # The mutation this catches, and why the obvious assertion misses it
///
/// The obvious test is `drop(endpoint); assert!(conn.is_established());`.
/// It passes against a **stopped** driver, because §16.3 makes the
/// accessors "synchronous reads of a shared cell" — a dead driver leaves a
/// stale `true` in that cell and the assertion cannot tell the difference.
/// A *name is not a pin*.
///
/// The separating assertion has to be **wire-observable work performed
/// after the `Endpoint` is gone**: `close()` is sealed, transmitted and
/// arrives, and the peer surfaces `PeerClosed`. A stopped driver seals
/// nothing, and the local side would surface `EndpointDropped` (§18.1)
/// rather than `LocallyClosed`. Both are asserted.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s26_endpoint_drop_alone_does_not_stop_the_driver() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut a = Node::spawn(&net, 1, 5021);
            let b = Node::spawn(&net, 2, 5022);
            let (ca, cb) = establish(&a, &b).await;

            // The whole point of the story.
            a.ep = None;

            assert!(
                ca.is_established(),
                "§16.3: the driver lives while any handle lives"
            );

            let watch_b = pin!(cb.closed());
            let watch_a = pin!(ca.closed());
            let (lost_b, lost_a, ()) =
                tokio::join!(watch_b, watch_a, ca.close(0x11, b"after the endpoint"));

            assert_eq!(
                lost_b,
                ConnectionLost::PeerClosed {
                    code: 0x11,
                    reason: b"after the endpoint".to_vec(),
                },
                "§16.3: a CLOSE sealed after the Endpoint was dropped must still reach \
                 the peer — the driver is alive because a Connection handle is"
            );
            assert_eq!(
                lost_a,
                ConnectionLost::LocallyClosed,
                "§18.1: EndpointDropped here would mean the driver had stopped on the \
                 Endpoint drop, which is exactly what S26 says it must not do"
            );
        })
        .await;
}

/// **S26 — the driver stops when, and only when, the last handle goes.**
///
/// # The mutation this catches
///
/// Two, and they are opposites — which is why both halves are here:
///
/// * **Teardown on `Endpoint` drop** (the obvious guess S26 warns about):
///   the `LocalSet` completes at the first assertion, which expects it not
///   to.
/// * **A driver that never stops** — a leaked `Rc` cycle between the
///   driver task and the connection state, or a command channel the driver
///   holds a sender for. The second assertion is the only thing in this
///   file that catches it, and "nothing leaks and no task outlives the
///   `LocalSet`" is S26's own wording.
///
/// `LocalSet` is used as the leak detector because it is the exact object
/// S26 names: it resolves when every task spawned on it has finished, so
/// "the driver stopped" and "the `LocalSet` completed" are the same fact.
/// Endpoint B is retired **first and completely**, so the surviving task
/// can only be A's driver — otherwise the first assertion would be
/// satisfied by B's driver and would assert nothing about A at all.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s26_last_handle_drop_stops_the_driver() {
    let net = Network::new();
    let mut local = pin!(tokio::task::LocalSet::new());

    let held: Connection = local
        .as_ref()
        .get_ref()
        .run_until(async {
            let mut a = Node::spawn(&net, 1, 5031);
            let mut b = Node::spawn(&net, 2, 5032);
            let (ca, cb) = establish(&a, &b).await;

            // Retire B entirely: its driver must stop, so it cannot
            // confound the assertions below.
            b.ep = None;
            drop(cb);

            // Retire A's Endpoint only. A's Connection is returned and
            // stays alive.
            a.ep = None;
            ca
        })
        .await;

    let still_running = tokio::time::timeout(Duration::from_secs(1), local.as_mut()).await;
    assert!(
        still_running.is_err(),
        "§16.3/S26: a live Connection alone keeps the driver running after the \
         Endpoint is gone — the LocalSet completed, so the driver stopped early"
    );

    drop(held);

    let stopped = tokio::time::timeout(SHELL_LATENESS_BOUND, local.as_mut()).await;
    assert!(
        stopped.is_ok(),
        "§16.3/S26: dropping every handle stops the driver — no task may outlive \
         the LocalSet"
    );
}

/// **S26 / ruling 62 — a `Connecting` *is* a handle.**
///
/// §16.3 (4294–4303): "The driver lives while a `Connecting` lives, and
/// dropping the last `Connecting` — with no `Endpoint` and no `Connection`
/// outstanding — stops it. … A future that changes protocol state when
/// dropped is a handle; one that does not, is not."
///
/// # The mutation this catches
///
/// A shell that counts only `Endpoint` and `Connection` as handles. The
/// dial is then silently abandoned the instant the application drops the
/// `Endpoint` — the retransmit train stops, the `Connecting` hangs for
/// ever, and nothing anywhere reports it. The first assertion is the only
/// one that sees it.
///
/// The second assertion is its opposite (a driver kept alive for ever by a
/// `Connecting` nobody holds), and both are needed: a build that treats a
/// `Connecting` as an *immortal* handle passes the first alone.
///
/// **If this test fails to compile because `Connecting` borrows the
/// `Endpoint`, that is gap G2 surfacing** — the state ruling 62 describes
/// would then be unreachable from safe Rust, which needs a ruling, not an
/// edit to this test.
#[tokio::test(flavor = "current_thread", start_paused = true)]
#[allow(clippy::async_yields_async)] // ruling 62: yielding a `Connecting`
// without awaiting it IS the state under test.
async fn s26_a_connecting_alone_keeps_the_driver_alive() {
    let net = Network::new();
    let mut local = pin!(tokio::task::LocalSet::new());

    let held = local
        .as_ref()
        .get_ref()
        .run_until(async {
            let mut a = Node::spawn(&net, 1, 5041);
            let dialling = a
                .ep()
                .connect(addr(5042), absent_static(2))
                .expect("connect");
            a.ep = None;
            dialling
        })
        .await;

    let still_running = tokio::time::timeout(Duration::from_secs(1), local.as_mut()).await;
    assert!(
        still_running.is_err(),
        "ruling 62: a Connecting is a handle — the driver must live while it does"
    );

    drop(held);

    let stopped = tokio::time::timeout(SHELL_LATENESS_BOUND, local.as_mut()).await;
    assert!(
        stopped.is_ok(),
        "ruling 62: dropping the last Connecting, with no Endpoint and no Connection \
         outstanding, stops the driver"
    );
}

/// **S26 / §15.4's local-close row — the last handle to *one* connection,
/// with the `Endpoint` still alive, seals `close(NO_ERROR, "")`.**
///
/// §16.2 (4190–4192): "Dropping the last handle to a `Connection` performs
/// `close(NO_ERROR, "")` — the graceful teardown of §15.2."
///
/// This is one half of gap **G3**. Its partner is
/// `s26_coincident_last_handle_drop_transmits_nothing`, and **neither is
/// meaningful without the other**: a build that never seals a CLOSE on drop
/// passes the partner, and a build that always does passes this one.
/// Ruling 88 is the statement that these two cases differ, so the pair is
/// the test of the ruling.
///
/// # The mutation this catches
///
/// A shell that treats every `Connection` drop as ruling 88's silent case.
/// The peer then waits `DEAD_TIMEOUT` (25 s) after every ordinary drop —
/// exactly the cost §15.1 says CLOSE exists to save.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s26_last_connection_drop_with_endpoint_alive_closes_gracefully() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5051);
            let b = Node::spawn(&net, 2, 5052);
            let (ca, cb) = establish(&a, &b).await;

            // A's Endpoint stays alive, so this is *not* the coincident
            // case ruling 88 governs.
            drop(ca);

            let lost = tokio::time::timeout(SHELL_LATENESS_BOUND, cb.closed())
                .await
                .expect(
                    "§16.2: dropping the last Connection handle performs \
                     close(NO_ERROR, \"\"), so the peer learns at once rather than \
                     at DEAD_TIMEOUT",
                );

            assert_eq!(
                lost,
                ConnectionLost::PeerClosed {
                    code: NO_ERROR,
                    reason: Vec::new(),
                },
                "§15.4's local-close row: the drop is close(NO_ERROR, \"\")"
            );
        })
        .await;
}

/// **S26 / ruling 88 — the coincident last-handle drop transmits nothing.**
///
/// §16.2 (4203–4216): "Dropping the last `Connection` **when it is also the
/// last handle in the process** is both at once, and §15.4's
/// endpoint-dropped row governs: **no CLOSE is sealed.**" Ruling 88 calls
/// this out as S26's `⚠ CHECK`.
///
/// # The mutation this catches
///
/// The reading that looks correct: `Connection::drop` always performs
/// `close(NO_ERROR, "")`. A synchronous `Drop` cannot await the driver, so
/// a build that tries anyway either blocks, panics, or — most likely —
/// posts a command the stopping driver never processes. This test says the
/// peer must observe **`TimedOut`**, not `PeerClosed`, and the two are
/// distinguishable in one assertion.
///
/// The assertion is two-sided in time as well as in variant:
///
/// * nothing arrives at the peer well before `DEAD_TIMEOUT` — a sealed
///   CLOSE would arrive within `SHELL_LATENESS_BOUND`, so the 1 s window is
///   four times over-generous and still decisive;
/// * the death does arrive, as `TimedOut`, by `DEAD_TIMEOUT` — without
///   which the test would pass on a build where the peer never learns
///   anything at all.
///
/// A byte-level cross-check accompanies it: `sent_count` must not grow
/// after the drop. That is §15.4's "Transmitted: nothing" read literally,
/// and it catches a CLOSE that is emitted but lost or misrouted, which the
/// variant assertion alone would score as a pass.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s26_coincident_last_handle_drop_transmits_nothing() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut a = Node::spawn(&net, 1, 5061);
            let b = Node::spawn(&net, 2, 5062);
            let (ca, cb) = establish(&a, &b).await;

            let mut watch = pin!(cb.closed());

            let before = sent_count(&tap, a.addr);

            // Every handle in the process that belongs to A, at once.
            a.ep = None;
            drop(ca);

            let early = tokio::time::timeout(Duration::from_secs(1), watch.as_mut()).await;
            assert!(
                early.is_err(),
                "ruling 88: the coincident drop seals no CLOSE, so the peer must learn \
                 nothing here — it observed {early:?}"
            );
            assert_eq!(
                sent_count(&tap, a.addr),
                before,
                "§15.4's endpoint-dropped row: Transmitted = nothing"
            );

            let lost = tokio::time::timeout(DEAD_TIMEOUT, watch.as_mut())
                .await
                .expect(
                    "§15.4: the peer's cost is bounded at DEAD_TIMEOUT, which that \
                         row accepts — it must actually fire",
                );
            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§15.4's endpoint-dropped row: the peer's view is liveness, ≤ 25 s"
            );
        })
        .await;
}

// ────────────────────────────── S27 ────────────────────────────────────

/// **S27 — `closed()` resolves with no verb in flight: `PeerClosed`.**
///
/// Appendix B: "Park a task on `closed()` and nothing else — **no `read`,
/// no `recv_message`, no send** — and drive each teardown row in turn …
/// **without the application ever calling a verb** — the pre-ruling-46
/// surface would have hung for ever here."
///
/// # The mutation this catches
///
/// A shell that latches the death only while some other verb is being
/// polled — i.e. one that folds `ConnEvent::Closed` handling into the
/// data-path wakeup instead of into a standalone latch. Ruling 46 exists
/// because the pre-ruling surface "could tell an application nothing except
/// as the return value of a verb it happened to be inside at the time",
/// and the mobile idle case is the one that matters. The parked task here
/// **owns the `Connection` and awaits nothing else**, which is the only
/// shape that distinguishes them.
///
/// It is a *spawned* task rather than a `join!` arm so that "parked, with
/// nothing else in flight" is literal: the task's whole body is the await.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s27_closed_resolves_with_no_verb_in_flight_peer_closed() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5071);
            let b = Node::spawn(&net, 2, 5072);
            let (ca, cb) = establish(&a, &b).await;

            let parked = tokio::task::spawn_local(async move { cb.closed().await });

            ca.close(0x77, b"row: PeerClosed").await;

            let lost = tokio::time::timeout(SHELL_LATENESS_BOUND, parked)
                .await
                .expect("the parked closed() never resolved")
                .expect("the parked task panicked");

            assert_eq!(
                lost,
                ConnectionLost::PeerClosed {
                    code: 0x77,
                    reason: b"row: PeerClosed".to_vec(),
                }
            );
        })
        .await;
}

/// **S27 — `closed()` resolves with no verb in flight: `LocallyClosed`.**
///
/// The row Appendix B lists last, and the one most easily got wrong: a
/// shell can plausibly treat `close()` as "the caller already knows" and
/// never latch anything, so a *different* task watching the same connection
/// learns nothing. §15.4 lists `LocallyClosed` as the local surface of that
/// row, and ruling 46 makes it awaitable "for whatever reason — **every**
/// row of §15.4's teardown matrix".
///
/// # The mutation this catches
///
/// `closed()` latched only on the *inbound* paths (peer CLOSE, liveness,
/// replacement). The watcher hangs for ever, and every other test in this
/// file still passes.
///
/// The watcher is a separate spawned task holding an `Rc<Connection>`, so
/// the closer and the watcher are demonstrably different call sites.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s27_closed_resolves_with_no_verb_in_flight_locally_closed() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5081);
            let b = Node::spawn(&net, 2, 5082);
            let (ca, _cb) = establish(&a, &b).await;
            let ca = Rc::new(ca);

            let parked = tokio::task::spawn_local({
                let watched = Rc::clone(&ca);
                async move { watched.closed().await }
            });

            ca.close(0x33, b"row: LocallyClosed").await;

            let lost = tokio::time::timeout(SHELL_LATENESS_BOUND, parked)
                .await
                .expect("a watcher that is not the closer never learned of the close")
                .expect("the parked task panicked");

            assert_eq!(lost, ConnectionLost::LocallyClosed);
        })
        .await;
}

/// **S27 — `closed()` resolves with no verb in flight: `TimedOut`.**
///
/// §15.4's liveness row, §7.4/§7.5: 25 s without an authenticated fresh
/// receive.
///
/// # The mutation this catches
///
/// Two, and the assertion is two-sided for that reason:
///
/// * **A liveness deadline that is not `DEAD_TIMEOUT`** — most plausibly
///   `KEEPALIVE_TIMEOUT` (10 s), which is the other timer in §7.5 and the
///   easy one to reach for. Caught only by the "still pending at
///   `DEAD_TIMEOUT − 1 s`" half.
/// * **A liveness timer the shell never arms** — the connection simply
///   never dies. Caught only by the other half.
///
/// The peer is retired by dropping **every** A handle, which by ruling 88
/// transmits nothing. That makes the silence genuine rather than a peer
/// that closed politely: if the implementation *did* seal a CLOSE here,
/// this test fails with `PeerClosed`, which is a useful cross-check on
/// `s26_coincident_last_handle_drop_transmits_nothing`.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s27_closed_resolves_with_no_verb_in_flight_timed_out() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut a = Node::spawn(&net, 1, 5091);
            let b = Node::spawn(&net, 2, 5092);
            let (ca, cb) = establish(&a, &b).await;

            let mut watch = pin!(cb.closed());

            a.ep = None;
            drop(ca);

            let early =
                tokio::time::timeout(DEAD_TIMEOUT - Duration::from_secs(1), watch.as_mut()).await;
            assert!(
                early.is_err(),
                "§7.5/§15.4: liveness fires at DEAD_TIMEOUT (25 s), not at \
                 KEEPALIVE_TIMEOUT (10 s) — it resolved early with {early:?}"
            );

            let lost = tokio::time::timeout(
                Duration::from_secs(1) + SHELL_LATENESS_BOUND,
                watch.as_mut(),
            )
            .await
            .expect("§7.5: the liveness timer must fire at DEAD_TIMEOUT");

            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§15.4's liveness row surfaces TimedOut"
            );
        })
        .await;
}

/// **S27 — `closed()` is a latch, not a queue (T10).**
///
/// §16.2 (ruling 46): "It is a **latched** signal, not a queue: cancel-safe,
/// awaitable concurrently from any number of tasks, and after death it
/// resolves immediately and for ever."
///
/// # The mutations this catches, and which assertion catches which
///
/// T10 names three broken versions. The obvious test — "`closed()` resolves
/// with `PeerClosed`" — passes against **all three**.
///
/// | Broken version | Caught by |
/// |---|---|
/// | `oneshot` (resolves once; a second `closed()` hangs) | (1) three concurrent futures, and (3) |
/// | `Notify` (a `closed()` created *after* the death never fires) | (2) |
/// | bounded broadcast (a late subscriber misses it) | (2) |
/// | a latch consumed by its first poller | (3) |
///
/// Assertion (2) is a **single poll**, not a timeout: "resolves
/// immediately" is a statement about the first poll, and a timeout would
/// pass against an implementation that resolves on the next driver tick —
/// which is precisely what a `Notify` woken by an unrelated event does.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s27_closed_is_latched_and_concurrent() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5101);
            let b = Node::spawn(&net, 2, 5102);
            let (ca, cb) = establish(&a, &b).await;

            let expected = ConnectionLost::PeerClosed {
                code: 0x55,
                reason: b"latched".to_vec(),
            };

            // (3), first half — a closed() future created before the death,
            // polled, and then dropped. It must claim nothing.
            {
                let mut abandoned = pin!(cb.closed());
                assert!(
                    poll_once(abandoned.as_mut()).await.is_pending(),
                    "closed() must not resolve on a live connection — §16.2: \
                     \"on a healthy connection it never resolves\""
                );
            }

            // (1) — three concurrent futures, all resolving with the same
            // value. A oneshot satisfies at most one of them.
            let w1 = pin!(cb.closed());
            let w2 = pin!(cb.closed());
            let w3 = pin!(cb.closed());
            let (r1, r2, r3, ()) = tokio::join!(w1, w2, w3, ca.close(0x55, b"latched"));

            assert_eq!(r1, expected, "concurrent closed() #1");
            assert_eq!(r2, expected, "concurrent closed() #2");
            assert_eq!(r3, expected, "concurrent closed() #3");

            // (2) + (3), second half — a closed() created *after* the death
            // resolves on its FIRST poll, and the abandoned future above
            // consumed nothing.
            let mut late = pin!(cb.closed());
            match poll_once(late.as_mut()).await {
                Poll::Ready(v) => assert_eq!(
                    v, expected,
                    "a closed() created after the death must resolve with the same value"
                ),
                Poll::Pending => panic!(
                    "§16.2: \"after death it resolves immediately and for ever\" — this \
                     one was Pending on its first poll, which is what a Notify or a \
                     bounded broadcast does"
                ),
            }

            // And once more, to pin "for ever" rather than "twice".
            let mut again = pin!(cb.closed());
            assert!(
                matches!(poll_once(again.as_mut()).await, Poll::Ready(_)),
                "the latch must not be consumed by a reader"
            );
        })
        .await;
}

// ────────────────────────────── S29 ────────────────────────────────────

/// **S29 — give up on a dial and immediately redial (ruling 50, T11).**
///
/// Appendix B: "dial a peer that never answers, drop the `Connecting` at
/// *t* = 5 s (**through `tokio::time::timeout`, so the test is the
/// idiom**) … an **immediate** `connect()` to the same static returns `Ok`
/// rather than `ConnectError::AlreadyConnected` — **with no advance of the
/// clock between the drop and the redial**, which is what pins the
/// cancellation ahead of the next endpoint verb."
///
/// §16.3 makes this a **MUST**: "An implementation MUST order the
/// cancellation ahead of any endpoint verb the application issues after the
/// drop returns, so an immediate redial cannot observe the corpse."
///
/// # The mutation this catches — and why the clock must not advance
///
/// The broken version is the natural one: `Connecting::drop` sets a flag,
/// or posts a command, that the driver notices **when next scheduled**. On
/// a paused clock with no `.await` between the drop and the redial, the
/// driver is never scheduled in that gap — so the flag build reads the
/// static as still PENDING and returns `AlreadyConnected`, while ruling
/// 87's synchronous shared-cell build reads what `Drop` just wrote and
/// returns `Ok`.
///
/// Insert a single `yield_now().await` between the two lines and this test
/// stops distinguishing them. That is why Appendix B specifies the gap, and
/// why there is no `await` between the `assert!` and the `connect()` below.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s29_cancel_then_immediate_redial() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5111);
            let ghost = (addr(5112), absent_static(2));

            let dial = a.ep().connect(ghost.0, ghost.1).expect("first dial");

            // The idiom every consumer writes. The `Connecting` is owned by
            // the `Timeout`, so it is dropped as this statement completes.
            let cancelled = tokio::time::timeout(Duration::from_secs(5), dial).await;
            assert!(
                cancelled.is_err(),
                "the peer never answers, so the 5 s timeout must elapse"
            );

            // ── NO await, NO clock advance, between the two ─────────
            let redial = a.ep().connect(ghost.0, ghost.1);

            assert!(
                redial.is_ok(),
                "ruling 50 (MUST): the cancellation is ordered ahead of the next \
                 endpoint verb, so an immediate redial cannot observe the corpse. \
                 Got {:?}",
                redial.err()
            );
        })
        .await;
}

/// **S29 — the cancelled retransmit train actually stops (T11, half 2).**
///
/// §16.3: "dropping it **cancels the outbound attempt immediately**: §5.5's
/// retransmit train stops, the pending and its pending-index entry are
/// dropped (§17.3) … **Nothing is transmitted**."
///
/// # The mutation this catches, and the degenerate version to avoid
///
/// T11: "'No msg1 was observed in the next instant' is the degenerate
/// version and passes on a train that is merely between retransmits." The
/// window here is **six** `RETRANSMIT_BASE` intervals, so a live train
/// would emit five or six further initiations into it.
///
/// This test deliberately does **not** redial: a redial transmits, and
/// folding the two assertions into one test would let the redial's own
/// msg1 mask a train that never stopped.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s29_cancelled_train_transmits_nothing_further() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5121);
            // A bound peer that never calls accept(): the initiations are
            // carried and parked, so the tap counts a real train.
            let b = Node::spawn(&net, 2, 5122);

            let dial = a.ep().connect(b.addr, b.pk).expect("dial");
            let cancelled = tokio::time::timeout(Duration::from_secs(5), dial).await;
            assert!(cancelled.is_err());

            let at_cancel = msg1_count(&tap, b.addr);
            assert!(
                at_cancel >= 1,
                "the train must have run before the cancel, or this test asserts \
                 nothing about stopping it"
            );

            tokio::time::sleep(RETRANSMIT_BASE * 6).await;

            assert_eq!(
                msg1_count(&tap, b.addr),
                at_cancel,
                "ruling 50: the §5.5 train is stopped, not merely unobserved"
            );
            assert_eq!(
                sent_count(&tap, a.addr),
                at_cancel,
                "ruling 50: \"Nothing is transmitted\" — not a CLOSE, not anything"
            );
        })
        .await;
}

/// **S29 — after a cancel the static routes as NONE (§5.4, §6.4).**
///
/// > an initiation arriving from that peer after a cancelled dial takes the
/// > ordinary staged-accept path (§5.4's NONE row) — there is no stale
/// > pending for the tie-break to consult.
///
/// §16.3: "A cancelled attempt leaves **no** PENDING entry anywhere the
/// tie-break can consult. All three readers of 'is this static PENDING?'
/// read the same pending tables the cancellation empties."
///
/// # The mutation this catches, and why it is deterministic in slice 3
///
/// A `Connecting::drop` that stops the retransmit train (so
/// `s29_cancelled_train_transmits_nothing_further` passes) but leaves the
/// pending entry in `§17.3`'s table — a plausible split, since the timer
/// and the table are separate structures.
///
/// In slice 3 the consequence is **deterministic**, which is what makes
/// this a pin rather than a coin flip: `src/core/endpoint/mod.rs`'s own
/// module doc records that slice 2a implements only §5.4's NONE row and
/// "returns `AcceptError::Stale` for the other two". So a leaked pending
/// sends the accept down the PENDING branch and `accept()` fails with
/// `Stale`, 100 % of the time. Under slice 7's real tie-break the same
/// leak becomes a 50 % failure; the assertion is written on the outcome
/// (`Ok`, never `Stale`) so it stays correct either way.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s29_after_cancel_the_static_routes_as_none() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5131);
            let b = Node::spawn(&net, 2, 5132);

            // A dials B; B never accepts; A gives up through the idiom.
            let dial = a.ep().connect(b.addr, b.pk).expect("dial");
            let cancelled = tokio::time::timeout(Duration::from_secs(5), dial).await;
            assert!(cancelled.is_err());

            // Now B dials A. A's static for B must be NONE.
            let dial_back = b.ep().connect(a.addr, a.pk).expect("B dials A");
            let accept_side = async {
                let intro = a.ep().accept().await.expect("accept queue closed");
                let claimed = intro.read_identity().await.expect("read_identity");
                let proven = claimed.authenticate().await.expect("authenticate");
                proven.accept().await
            };

            let (connected, accepted) = tokio::join!(dial_back, accept_side);

            match accepted {
                Ok(conn) => assert_eq!(
                    conn.remote_static().as_ref(),
                    b.pk.as_ref(),
                    "the accepted connection is the peer that dialled us"
                ),
                Err(AcceptError::Stale) => panic!(
                    "§16.3: a cancelled attempt leaves no PENDING entry — Stale here \
                     means the pending survived the drop and §5.4 took the PENDING row"
                ),
                Err(other) => panic!("unexpected accept failure: {other:?}"),
            }

            assert!(
                connected.is_ok(),
                "the peer's own Connecting must resolve: {:?}",
                connected.err()
            );
        })
        .await;
}

/// **S29 — a retry loop replaces rather than accumulates (initiator half).**
///
/// S29's amendment at approval: "dial → cancel → redial, repeated, leaves
/// the responder with exactly **one** connection per cycle, not a growing
/// set of half-open sessions."
///
/// # What this test covers, and what it does not (gap G4)
///
/// The **responder** half of that criterion — one connection per cycle, one
/// `ConnectionLost::Replaced`, 4 DH per cycle — is **not reachable in slice
/// 3**: it is S3b's replacement path, and `src/core/endpoint/mod.rs` records
/// that the LIVE branch returns `AcceptError::Stale` until **slice 7**.
/// `.slices/03-skeleton/PLAN.md` §5.2 anticipates this and asks the file to
/// say so rather than fake it. **Slice 7 owes the responder half.**
///
/// The **initiator** half is fully reachable and is what is asserted here,
/// and it is the half that carries the word *accumulates*: after N cancelled
/// dials, exactly **one** retransmit train may be running.
///
/// # The mutation this catches
///
/// A `Connecting::drop` that releases the *static* (so
/// `s29_cancel_then_immediate_redial` passes) but leaves the retransmit
/// timer armed. Each cycle then adds a train, and the endpoint quietly
/// becomes an N-fold initiation source against one peer — §6.9's exact
/// concern, arrived at from the application's side.
///
/// The bound separates the two decisively: over six `RETRANSMIT_BASE`
/// intervals one train emits 5–6 initiations, while five accumulated trains
/// emit roughly 28.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s29_retry_loop_replaces_rather_than_accumulates() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 5141);
            let b = Node::spawn(&net, 2, 5142);

            const CYCLES: usize = 4;
            for cycle in 0..CYCLES {
                let dial = a
                    .ep()
                    .connect(b.addr, b.pk)
                    .unwrap_or_else(|e| panic!("cycle {cycle}: redial refused with {e:?}"));
                let cancelled = tokio::time::timeout(Duration::from_secs(5), dial).await;
                assert!(cancelled.is_err(), "cycle {cycle}: the peer never answers");
            }

            // One final attempt, left running: exactly one train may exist.
            let survivor = a.ep().connect(b.addr, b.pk).expect("final dial");
            let mut survivor = pin!(survivor);

            let before = msg1_count(&tap, b.addr);
            tokio::time::sleep(RETRANSMIT_BASE * 6).await;
            let delta = msg1_count(&tap, b.addr) - before;

            assert!(
                (3..=9).contains(&delta),
                "S29: a retry loop replaces rather than accumulates. Over six \
                 RETRANSMIT_BASE intervals ONE train emits 5–6 initiations; \
                 {CYCLES} accumulated trains would emit roughly {}. Observed {delta}",
                (CYCLES + 1) * 6
            );

            // The survivor is still a live attempt, not a corpse.
            assert!(
                poll_once(survivor.as_mut()).await.is_pending(),
                "the surviving dial must still be in flight"
            );
        })
        .await;
}

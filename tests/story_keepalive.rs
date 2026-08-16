//! **Keepalive, liveness and the contested probe — S5, S11, S20.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S5** | a connection with no traffic at all is reaped at `DEAD_TIMEOUT`; one exchange in *either* direction self-sustains it for ever with **no opt-in**; and `set_persistent_keepalive` holds an otherwise-idle link open over an admissible band that rejects rather than clamps. |
//! | **S11** | a `None`-basis refusal marks the connection contested, an ack-eliciting PING goes out, and an ACK covering the probe floor must arrive within `KEEPALIVE_TIMEOUT`. A live peer answers and the connection survives *with the refusal standing*; a peer that cannot answer dies `TimedOut`. A second refusal while contested is a total no-op. |
//! | **S20** | where **we** dialled, a peer that restarts is let back in by the probe's verdict, inside `DEAD_TIMEOUT`. |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by **T-K** from `STORIES.md` (S5, S11, S20, S27) and
//! `.slices/07-mobility/CONTRACT-7.md` **alone**, in an isolated worktree
//! cut at `195c57a`, while the slice-7 implementer and the second test
//! author (`tests/story_mobility.rs`) worked blind to this file. No line of
//! any slice-7 implementation was read. Spec text was consulted only as
//! §7.4 (lines 2247–2320) and §7.5 (lines 2355–2465) via targeted reads —
//! `SPEC.md` was never read whole (working rule 1).
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the instrument that advances virtual time
//! *and* observes: `timeout(d, c.closed()).await.is_err()` says "still
//! alive at `now + d`", and its `Ok` says "it died by then". Tokio's
//! auto-advance stops at the **earliest** pending timer, so a 75 s
//! `timeout` still lets every 10 s keepalive in between fire — which is
//! exactly what the dance tests need. `tokio::time::advance` is used only
//! where one specific timer must fire, `settle()` where a driver needs a
//! turn and no time may pass. There is no `sleep`.
//!
//! # Contract facts these tests are built on
//!
//! * **§3.3 / ruling 39.** `S = last_send` (**marking** sends only, §7.4:
//!   first-transmission STREAM/DATAGRAM frames, and the keepalive itself);
//!   `R = last_authenticated_recv`. The passive keepalive arms at
//!   `S + KEEPALIVE_TIMEOUT` **iff `R > S`**. At install `S == R`, so a
//!   connection that never receives emits nothing and dies at
//!   install + `DEAD_TIMEOUT`.
//! * **§3.3 / rulings 40, 42, 44.** `set_persistent_keepalive` admits
//!   `[1 s, DEAD_TIMEOUT)` — floor **inclusive**, ceiling **exclusive** —
//!   returns `Result<(), ConfigError>`, and on `Err` leaves the current
//!   interval **unchanged**: no clamp, no panic.
//! * **§5.1 / rulings 36, 41.** A refusal against a `None` basis records
//!   `floor = session.next_counter()` and marks the connection. At the
//!   first instant §7.3's budget admits the PING, the PING goes out, the
//!   deadline arms at `now + KEEPALIVE_TIMEOUT`, and `Contested` is
//!   queued — **one atomic step**. Any ACK covering **any** counter
//!   `>= floor` clears it.
//! * **§5.1 / ruling 41.** A second refusal while `Armed` is a **total
//!   no-op**: floor unchanged, deadline **not** re-armed, no second PING,
//!   no second event. That is a security property, not an optimisation.
//! * **§5.1 / ruling 175.** The honest bound is *"at most one probe per
//!   mark, at most one mark per uncontested refusal, and marks cannot
//!   overlap"* — **not** one per `KEEPALIVE_TIMEOUT`. A live peer clears
//!   in ~1 RTT and the next refusal is a full second mark. No test here
//!   asserts the superseded bound; `s11_cleared_then_marked_again_hands_
//!   over_in_generation_order` asserts its negation.
//! * **§5.4 / ruling 185.** A second write to an occupied slot takes the
//!   **new** generation, so `Contested` → `ContestCleared` → `Contested`
//!   with no drain hands over as *cleared, then contested*.
//! * **§8.3.** On a **validated** address — which is every address a
//!   `connect()` supplied (§3.2) — the mark and the transmission are the
//!   same instant. The pending gap is reachable only after a roam. See
//!   the coverage note below.
//!
//! # Coverage this file cannot reach, and why (working rule 13)
//!
//! * **The `Contested::Pending` gap** — §5.1's mark-without-transmission
//!   state, and both of ruling 176's exits (a covering ACK while pending
//!   cancels the probe and emits **nothing**; a roam while pending leaves
//!   the mark intact). Reaching it needs `budget_sent + probe_len >
//!   3 * budget_recv` at the mark, and §3.2's arithmetic makes that
//!   unconstructible from an integration test: after a roam
//!   `budget_sent == 0`, and the probe is a ~30 B datagram against a
//!   budget of `3 *` the roam trigger — which is itself at least a 30 B
//!   keepalive, so 90 B admits the probe outright. Driving `budget_sent`
//!   up first requires small packets, and every small packet the peer
//!   sends raises the cap by **3×** what our reply spends, so the budget
//!   never binds in a symmetric exchange; the asymmetric case (we upload)
//!   holds our *large* packets entirely and leaves `budget_sent` at 0.
//!   The state is observable below the shell — §5.5 makes the "mark"
//!   trace *"the only observable of the pending state below the shell"* —
//!   so it belongs in a core test over `testfix`, which `pub(crate)` puts
//!   out of an integration test's reach. **Owed by the implementer's
//!   `src/core/connection/tests_roam.rs`, not by this file.**
//! * **"Nothing is transmitted at the verdict"** (§5.1) cannot be sampled
//!   atomically at the verdict instant, and the interval before it
//!   legitimately carries PTO probes for the PING (§5.1 puts the probe in
//!   the sent map). `s11_a_peer_that_cannot_answer_dies_at_the_probe_
//!   deadline` asserts the weaker post-death form — the tap does not grow
//!   after `closed()` resolves — and says so where it does.
//! * **§5.5's three `slither::policy` traces.** A `tracing` subscriber is
//!   not among this crate's dev-dependencies and adding one is not a test
//!   author's call. The mark trace is the pending state's only sub-shell
//!   observable, so **it is owed a unit test**, not an integration one.
//! * **`FlakyPolicy::lossy` is invisible to every public counter** (the
//!   tap sits above the loss draw), so no test here uses it. Loss is
//!   `block_path` or a dropped endpoint, both of which are decisive.
//!
//! # Reported, not resolved (CLAUDE.md working rule 3)
//!
//! * **K1 — `CONTRACT-7.md` §0's ruling-43 row still states the bound
//!   ruling 175 reversed, with no supersession marker, in the one table
//!   the file tells every agent to read *first* and that "override[s]
//!   anything below".** §5.1's prose does say *"ruling 43's 'one probe per
//!   `KEEPALIVE_TIMEOUT`' is **superseded** — do not write a test
//!   asserting it"*, and ruling 40's row does carry its own marker
//!   (*"Ruling 38 is reversed in part"*), so the omission is local to
//!   ruling 43's row rather than a convention. The row cannot simply be
//!   struck: its **second** sentence — the probe is counted in the sent
//!   map and in `bytes_in_flight` — is reaffirmed by §5.1's transmission
//!   step and is still live.
//!
//!   The sharper half is working rule 4's, *grep for the rationale, not
//!   only the token*: ruling 43 did not merely state a number, it
//!   **denied a characterisation** — *"not by 'the application's own
//!   accept rate', which was false because **the attacker supplies the
//!   Intros**"* — and ruling 175 **reinstates that exact phrase**
//!   (*"the refusal rate is the application's own `accept()` rate"*)
//!   without addressing the reason 43 gave for rejecting it. The two may
//!   well be reconcilable — the attacker supplies `Intro`s, but only the
//!   application supplies `accept()` calls, and §6.9 prices an unaccepted
//!   `Intro` at one queue slot — but that argument is nowhere in the
//!   contract, and it is the argument the security bound rests on. No
//!   test in this file asserts either bound.
//! * **K2 — §7.4's quiet-set list does not contain the contested PING,
//!   and ruling 182 forbids deriving what it should quote.** §3.3 places
//!   the PING in the quiet set (*"a `seal_quiet` send — a PTO probe, a
//!   credit frame, a retransmission, the contested PING"*), while §7.4's
//!   own enumeration (`SPEC.md` L2256–2258) reads *"pure ACKs, PTO
//!   probes, retransmissions, the credit frames …, RESET_STREAM, and
//!   CLOSE"* — no PING. Ruling 182 says in terms: *"Which sends are
//!   marking is §7.4's answer — quote it, do not re-derive it."*
//!
//!   §7.4's **`seal`** side is a closed positive characterisation
//!   (*"packets carrying at least one first-transmission STREAM frame or
//!   DATAGRAM frame …, and the keepalive"*), and a PING-only packet
//!   satisfies none of it, so the contract lands on the side that
//!   definition gives and **nothing here needs a ruling to proceed**.
//!   What is worth recording is the asymmetry: one side of §7.4 is a
//!   closed rule and the other is an enumeration that working rule 8
//!   would read as exhaustive, and the frame that falls between them is
//!   the one this slice introduces. No test in this file depends on the
//!   PING's class — `s11_a_none_basis_refusal…`'s survival window is
//!   `2 × KEEPALIVE_TIMEOUT`, inside `DEAD_TIMEOUT` under either reading.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::time::Duration;

use tokio::time::Instant;

use slither::config::Config;
use slither::constants::{
    DEAD_TIMEOUT, KEEPALIVE_TIMEOUT, PERSISTENT_KEEPALIVE_DEFAULT, PERSISTENT_KEEPALIVE_MIN,
    SHELL_LATENESS_BOUND,
};
use slither::error::{AcceptError, ConfigError, ConnectionLost};
use slither::identity::{Identity, PublicKeyOf};
use slither::testutil::{CountingIdentity, Network, Tap, settle};

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// The `Node` / `establish` block is `tests/story_lifecycle.rs`'s, carried
// over deliberately: it is the only harness that lets two endpoints share
// **one static key**, which is what a "restart" is from the far side and
// what every S11 and S20 test below turns on. `testutil::Pair` cannot
// express it — `Peer::spawn` is private and derives its identity seed from
// a private `derive_seed`, so a second endpoint with `b`'s static cannot be
// built through it.
//
// INTEGRATOR: one name below is a **proposal**, flagged because
// `CONTRACT-7.md` §5.3 declares the type and its three variants but not the
// module it lands in:
//
//   * `slither::Notification` — §5.3 calls it "the shell's public enum",
//     so it may well be `slither::shell::Notification` re-exported at the
//     crate root the way `Connection` and `Endpoint` are. If the re-export
//     lands elsewhere, **change this one `use` line**; no assertion below
//     names the path.
//
// Everything else this file names is written out in `CONTRACT-7.md`:
// `Connection::notified() -> Result<Notification, ConnectionLost>` (§6),
// `Connection::set_persistent_keepalive(Option<Duration>) ->
// Result<(), ConfigError>` (§6), `ConfigError::{KeepaliveTooShort,
// KeepaliveTooLong}` (§6/§10), `AcceptError::Stale` (§4.1),
// `ConnectionLost::TimedOut` (§5.1).
// ══════════════════════════════════════════════════════════════════════

use slither::Notification;

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;
type Pk = PublicKeyOf<Id>;
type Endpoint = slither::Endpoint<Id>;
type Connection = slither::Connection<Suite>;

fn addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

struct Node {
    /// `Option` so a test can drop the `Endpoint` handle without dropping
    /// the bookkeeping beside it — S20's restart turns on exactly that.
    ep: Option<Endpoint>,
    pk: Pk,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
    ///
    /// Two nodes spawned with the **same `key_seed`** hold the same static
    /// key and are, to every peer, one identity that restarted. Their
    /// ephemerals still differ: `EndpointBuilder::build` seeds the §16.6
    /// RNG from OS entropy when `rng_seed` is not supplied, so the second
    /// node's msg1 is not a replay of the first's.
    fn spawn(net: &Network, key_seed: u8, port: u16) -> Node {
        let a = addr(port);
        let id: Id = CountingIdentity::seeded([key_seed; 32]);
        let pk = *id.public_static();
        let ep = Endpoint::builder()
            .identity(id)
            .wire(net.wire(a))
            .config(Config::new())
            .build();
        Node {
            ep: Some(ep),
            pk,
            addr: a,
        }
    }

    fn ep(&self) -> &Endpoint {
        self.ep.as_ref().expect("the Endpoint handle was dropped")
    }
}

/// `dialler` connects to `listener`, which walks §6.2's staged chain.
///
/// Returns `(initiator side, responder side)`. The initiator's basis for
/// the responder's static is `None` (§17.4) — it never saw a msg1
/// timestamp — which is the precondition every S11 test needs.
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

/// One full climb of §6.2's ladder on `node`, returning whatever the final
/// `accept()` returned — `Err(AcceptError::Stale)` is the *expected*
/// outcome for every S11 test, so it is handed back rather than unwrapped.
///
/// The climb reaches `Proven`, which is what ruling 177 means by an
/// **admitted** candidate: the same static, with a verifying tail tag. A
/// refusal by anything weaker marks nothing.
async fn staged_accept(node: &Node) -> Result<Connection, AcceptError> {
    let intro = node
        .ep()
        .accept()
        .await
        .expect("§16.2: accept() yields None only when the endpoint is closed");
    let claimed = intro.read_identity().await.expect("read_identity");
    let proven = claimed.authenticate().await.expect("authenticate");
    proven.accept().await
}

/// Datagrams the fabric carried **from** `from`, of any type.
///
/// Counted off the [`Tap`], which sits *above* the loss draw but *below*
/// `block_path` — a blocked send is never tapped. Every use below is on an
/// unblocked wire for that reason.
fn sent_from(tap: &Tap, from: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, _dst, _bytes)| *src == from)
        .count()
}

/// Datagrams the fabric carried on exactly the path `from → to`.
fn sent_on(tap: &Tap, from: SocketAddr, to: SocketAddr) -> usize {
    tap.datagrams()
        .iter()
        .filter(|(src, dst, _bytes)| *src == from && *dst == to)
        .count()
}

/// Advance `d` and assert `c` was **still alive** the whole way.
///
/// The two halves are one call because they are one claim: tokio's
/// auto-advance stops at the earliest pending timer, so this drives every
/// keepalive, beacon and liveness deadline inside the window rather than
/// stepping over them.
async fn alive_through(d: Duration, c: &Connection, why: &str) {
    let early = tokio::time::timeout(d, c.closed()).await;
    assert!(
        early.is_err(),
        "the connection died inside the window with {early:?}: {why}"
    );
}

/// Advance at most `d` and return the reason `c` died, or panic.
async fn dies_within(d: Duration, c: &Connection, why: &str) -> ConnectionLost {
    tokio::time::timeout(d, c.closed())
        .await
        .unwrap_or_else(|_| panic!("the connection was still alive after {d:?}: {why}"))
}

/// Claim one notification, or panic if none arrives inside `d`.
async fn notified_within(d: Duration, c: &Connection, why: &str) -> Notification {
    tokio::time::timeout(d, c.notified())
        .await
        .unwrap_or_else(|_| panic!("no notification inside {d:?}: {why}"))
        .unwrap_or_else(|lost| panic!("the connection ended ({lost:?}) instead: {why}"))
}

/// Assert **no** notification is claimable within `d`.
async fn no_notification_within(d: Duration, c: &Connection, why: &str) {
    let seen = tokio::time::timeout(d, c.notified()).await;
    assert!(
        seen.is_err(),
        "a notification was handed over when none was owed ({seen:?}): {why}"
    );
}

/// Poll `fut` exactly once and report whether it was ready on that poll.
///
/// Appendix B's "resolves immediately": a `timeout(ZERO, …)` cannot say it,
/// because on the paused clock a zero timeout races the future rather than
/// observing it.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> std::task::Poll<F::Output> {
    std::future::poll_fn(|cx| std::task::Poll::Ready(fut.as_mut().poll(cx))).await
}

// ═══════════════════════════════════════════════════════════════════════
// S5 — the two escapes from the 25 s reap
// ═══════════════════════════════════════════════════════════════════════

/// **S5, escape (i): there is none. A silent connection is reaped.**
///
/// > a connection that carries *no* application traffic after install
/// > emits nothing and dies at install + `DEAD_TIMEOUT` (25 s) with
/// > `ConnectionLost::TimedOut`.
///
/// §7.4's install pin is what makes this a fact rather than a choice: a
/// newly installed session sets `last_send` **and**
/// `last_authenticated_recv` to the install instant, so `R > S` — the
/// passive keepalive's only trigger — is false from the start and stays
/// false while nobody sends.
///
/// # The broken implementations this catches
///
/// * **A keepalive that is not conditioned on `R > S`.** The single most
///   likely slice-7 bug, because the beacon's rule *is* unconditional and
///   the two live three lines apart in §3.3. Such a build has both sides
///   emitting at 10 s, each receive refreshing the other's `R`, and the
///   connection lives **for ever**. Caught twice over: by the tap, which
///   must not grow at all, and by the death, which must arrive.
/// * **A liveness deadline that is not `DEAD_TIMEOUT`** — most plausibly
///   `KEEPALIVE_TIMEOUT`, the other timer in §7.5. Caught only by the
///   `DEAD_TIMEOUT − 1 s` half.
/// * **A liveness timer never armed.** Caught only by the other half.
///
/// The assertion is two-sided in time and two-sided in *side*: both peers
/// are checked, because a build that reaps only the initiator passes a
/// one-sided test and leaves the responder's half-open session immortal —
/// which is §7.4's stated reason for pinning the clock *armed*.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_a_connection_with_no_traffic_emits_nothing_and_is_reaped_at_dead_timeout() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7001);
            let b = Node::spawn(&net, 2, 7002);
            let (ca, cb) = establish(&a, &b).await;

            settle().await;
            let a_at_install = sent_from(&tap, a.addr);
            let b_at_install = sent_from(&tap, b.addr);

            // Neither application ever calls a send verb.
            alive_through(
                DEAD_TIMEOUT - Duration::from_secs(1),
                &ca,
                "§7.5: liveness fires at DEAD_TIMEOUT (25 s), not at \
                 KEEPALIVE_TIMEOUT (10 s)",
            )
            .await;

            // §12.4(i): "NOTHING is transmitted." Both directions, because
            // a build that keepalives from one side only still keeps the
            // pair alive and still fails S5.
            assert_eq!(
                sent_from(&tap, a.addr),
                a_at_install,
                "ruling 39: with no authenticated receive since install, \
                 R > S is false and the initiator emits nothing — a \
                 keepalive here is the unconditional-passive-rule bug"
            );
            assert_eq!(
                sent_from(&tap, b.addr),
                b_at_install,
                "ruling 39: the responder emits nothing either"
            );

            let by = Duration::from_secs(1) + SHELL_LATENESS_BOUND;
            let lost_a = dies_within(by, &ca, "§7.5: liveness must fire at DEAD_TIMEOUT").await;
            let lost_b =
                dies_within(by, &cb, "§7.5: both sides are reaped, not just the dialler").await;

            assert_eq!(lost_a, ConnectionLost::TimedOut, "§15.4's liveness row");
            assert_eq!(lost_b, ConnectionLost::TimedOut, "§15.4's liveness row");
        })
        .await;
}

/// **S5, escape (ii): one exchange, initiator → responder, and the loop
/// feeds itself.**
///
/// > A connection that has carried **one exchange in either direction**
/// > self-sustains indefinitely via the 10 s keepalive dance, with no
/// > opt-in.
///
/// Ruling 39's explicit scope: *no* `set_persistent_keepalive` call appears
/// anywhere in this test, and that absence is the acceptance criterion.
///
/// # The broken implementations this catches, and why "still alive" alone
/// # would catch none of them (working rule 9)
///
/// "The connection is still alive at 75 s" is satisfied **for free** by a
/// build that never arms the liveness timer at all — the degenerate case
/// this bound does not violate. Three assertions separate them:
///
/// * **No passive keepalive at all.** The connection dies at 25 s. Caught
///   by `alive_through`.
/// * **A liveness timer that is never armed** (so nothing ever dies, and
///   nothing is ever sent). Caught by the *traffic* assertion: the dance
///   must actually put datagrams on the wire, at least four per side
///   inside 75 s. Its companion `s5_a_connection_with_no_traffic…` proves
///   the reaper exists, so the pair is decisive where neither is alone.
/// * **A one-directional dance** — only the side that received answers,
///   and the other's `R` never advances. Caught by requiring the traffic
///   in **both** directions, and by checking **both** connections live.
///
/// The lower bound is deliberately 4 rather than "some": one datagram
/// apiece is what a build that answers the exchange once and then stops
/// produces, and that build dies at 25 s anyway. Four is the count that
/// says a *cadence* is running, and it is loose against the ~7 a 10 s
/// dance yields in 75 s.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_one_exchange_from_the_initiator_self_sustains_with_no_opt_in() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7011);
            let b = Node::spawn(&net, 2, 7012);
            let (ca, cb) = establish(&a, &b).await;

            // §7.4: a DATAGRAM frame's first transmission is a *marking*
            // send, which is what puts the receiver into `R > S`.
            ca.send_datagram(b"one exchange").expect("send_datagram");
            let got = tokio::time::timeout(Duration::from_secs(1), cb.recv_datagram())
                .await
                .expect("the exchange must cross the fabric")
                .expect("recv_datagram");
            assert_eq!(got, b"one exchange");

            settle().await;
            let a_before = sent_from(&tap, a.addr);
            let b_before = sent_from(&tap, b.addr);

            // Three full liveness periods of application silence.
            let window = DEAD_TIMEOUT * 3;
            alive_through(
                window,
                &ca,
                "ruling 39: one exchange starts the dance and the dance is \
                 self-sustaining — no opt-in, no reap",
            )
            .await;

            let mut still = pin!(cb.closed());
            assert!(
                poll_once(still.as_mut()).await.is_pending(),
                "ruling 39: the responder is sustained by the same dance"
            );

            let a_sent = sent_from(&tap, a.addr) - a_before;
            let b_sent = sent_from(&tap, b.addr) - b_before;
            assert!(
                a_sent >= 4,
                "the dance must be running, not merely undetected: the \
                 initiator put {a_sent} datagrams on the wire in {window:?}, \
                 and a 10 s cadence owes about 7"
            );
            assert!(
                b_sent >= 4,
                "the dance is two-sided — each side's keepalive is what \
                 establishes R > S on the other: the responder put {b_sent} \
                 datagrams on the wire in {window:?}"
            );
        })
        .await;
}

/// **S5, escape (ii) again — the exchange goes responder → initiator.**
///
/// S5 says *"one exchange in **either** direction"*, and a list in the spec
/// is read as exhaustive whether or not it says so (working rule 8). The
/// two directions are not symmetric in slice 7: by §3.2 the **responder's**
/// address for its peer is *unvalidated* — it was anchored from msg1, so
/// §7.3's budget is armed on that side and not on the dialler's. A build
/// that lets the budget bind the keepalive would sustain the A → B
/// direction and reap the B → A one, and the initiator-side test above
/// would stay green.
///
/// # The broken implementation this catches
///
/// A budget check that never disarms — §7.3's *"never lifted"* reading that
/// ruling 168 reversed. Under it the responder's ~588 B budget is consumed
/// by msg2 plus a handful of keepalives and the dance suffocates, killing
/// the connection inside 25 s of the budget binding. That is the case
/// ruling 168 describes as *"an endpoint that accepts connections could
/// never serve one"*, and 75 s of dance is long enough to exhaust any
/// static cap derived from one 196 B msg1.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_one_exchange_from_the_responder_self_sustains_with_no_opt_in() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7021);
            let b = Node::spawn(&net, 2, 7022);
            let (ca, cb) = establish(&a, &b).await;

            cb.send_datagram(b"one exchange").expect("send_datagram");
            let got = tokio::time::timeout(Duration::from_secs(1), ca.recv_datagram())
                .await
                .expect("the exchange must cross the fabric")
                .expect("recv_datagram");
            assert_eq!(got, b"one exchange");

            settle().await;
            let a_before = sent_from(&tap, a.addr);
            let b_before = sent_from(&tap, b.addr);

            let window = DEAD_TIMEOUT * 3;
            alive_through(
                window,
                &cb,
                "ruling 168: the accepting side's budget disarms on a \
                 return-routability proof — a responder that can never \
                 serve a connection is the defect that ruling reversed",
            )
            .await;

            let mut still = pin!(ca.closed());
            assert!(
                poll_once(still.as_mut()).await.is_pending(),
                "the dialler is sustained by the same dance"
            );

            let a_sent = sent_from(&tap, a.addr) - a_before;
            let b_sent = sent_from(&tap, b.addr) - b_before;
            assert!(
                a_sent >= 4,
                "the initiator answered {a_sent} times in {window:?}"
            );
            assert!(
                b_sent >= 4,
                "the responder — whose address for its peer is the \
                 *unvalidated* one — kept sending: {b_sent} in {window:?}"
            );
        })
        .await;
}

/// **S5 (opt-in): the beacon holds a mutually idle link open.**
///
/// §12.4(iii), and §7.5's *"the one mechanism that sustains a **mutually
/// idle** link"*. One side opts in; neither application ever sends; both
/// live.
///
/// # The broken implementations this catches
///
/// * **A beacon that consults `R`** — i.e. one built by copying the
///   passive rule. §3.3: it *"fires **unconditionally** — it does not
///   consult `R`."* In the state this test creates, `R == S` at install
///   and never moves until the beacon itself moves it, so a beacon
///   conditioned on `R > S` never fires and both sides die at 25 s.
/// * **A beacon that never re-arms**, firing once and stopping: the pair
///   survives the first 25 s window and dies inside the second. The
///   window is three `DEAD_TIMEOUT`s for that reason.
/// * **A beacon on the wrong side of the marking set** (ruling 40's
///   declined alternative): excluded from marking, it would not drag `S`
///   forward, and `s5_a_connection_with_no_traffic…` plus this test's
///   own send counts bracket the behaviour from both sides.
///
/// The count assertion is what separates "alive" from "alive for the right
/// reason": at `PERSISTENT_KEEPALIVE_DEFAULT` the beaconing side owes
/// about `75 / 10 = 7` sends, and a build that keeps the link alive by
/// never arming a liveness timer sends none.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_a_beacon_on_one_side_holds_a_mutually_idle_link_open() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7031);
            let b = Node::spawn(&net, 2, 7032);
            let (ca, cb) = establish(&a, &b).await;

            ca.set_persistent_keepalive(Some(PERSISTENT_KEEPALIVE_DEFAULT))
                .expect("§3.3: 10 s is inside [1 s, DEAD_TIMEOUT)");

            settle().await;
            let a_before = sent_from(&tap, a.addr);
            let b_before = sent_from(&tap, b.addr);

            let window = DEAD_TIMEOUT * 3;
            alive_through(
                window,
                &ca,
                "§7.5: the beacon is unconditional and re-arms from every \
                 marking send",
            )
            .await;

            let mut still = pin!(cb.closed());
            assert!(
                poll_once(still.as_mut()).await.is_pending(),
                "§7.5: \"one side opting in is thus enough to keep the pair \
                 alive\" — the beacon establishes R > S at the peer, whose \
                 passive rule answers it"
            );

            let a_sent = sent_from(&tap, a.addr) - a_before;
            let b_sent = sent_from(&tap, b.addr) - b_before;
            assert!(
                a_sent >= 4,
                "the beacon must fire repeatedly, not once: {a_sent} sends \
                 in {window:?} at a 10 s interval"
            );
            assert!(
                b_sent >= 4,
                "§7.5: the peer's *passive* rule answers each beacon — \
                 {b_sent} answers in {window:?}"
            );
        })
        .await;
}

/// **S5 (opt-in): the admissible band, every boundary, both sides of each.**
///
/// §3.3's table and App. B L6178–6181, which names both regressions in
/// terms: *"a test that pins the old floor at `DEAD_TIMEOUT` is the
/// regression this obligation exists to catch, and a test asserting that
/// 1 s is rejected is the mirror regression."*
///
/// # The broken implementations this catches
///
/// * **Ruling 40 un-applied — the bound as a *floor***, which is what
///   ruling 38 documented before it was reversed. Such a build rejects
///   `1 s` and accepts `30 s`: the `1 s → Ok` row and the `30 s → Err`
///   row both fire.
/// * **An exclusive floor** (`> 1 s` rather than `>= 1 s`, ruling 42's
///   *"1 s inclusive"*). Caught by `PERSISTENT_KEEPALIVE_MIN → Ok`, which
///   is written against the constant rather than a literal so that a
///   drift in `constants.rs` moves the test with it.
/// * **An inclusive ceiling** (`<= DEAD_TIMEOUT`). Caught by
///   `DEAD_TIMEOUT → Err`, likewise written against the constant —
///   ruling 63 forbids a second name for it, so the test must not invent
///   one either.
/// * **A ceiling compared against `PERSISTENT_KEEPALIVE_DEFAULT`**, an
///   easy slip since both are 10 s-shaped: caught by `20 s → Ok`.
///
/// Both sides of both boundaries are asserted — `999 ms`/`1 s` and
/// `DEAD_TIMEOUT − 1 ms`/`DEAD_TIMEOUT` — which is slice 1's one-sided
/// boundary lesson applied deliberately.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_set_persistent_keepalive_admits_exactly_one_second_up_to_dead_timeout() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7041);
            let b = Node::spawn(&net, 2, 7042);
            let (ca, _cb) = establish(&a, &b).await;

            let ms = Duration::from_millis;

            // ── below the floor ────────────────────────────────────────
            for bad in [Duration::ZERO, ms(1), ms(500), ms(999)] {
                assert_eq!(
                    ca.set_persistent_keepalive(Some(bad)),
                    Err(ConfigError::KeepaliveTooShort),
                    "ruling 42: {bad:?} is below the 1 s floor"
                );
            }
            assert_eq!(
                ca.set_persistent_keepalive(Some(PERSISTENT_KEEPALIVE_MIN - ms(1))),
                Err(ConfigError::KeepaliveTooShort),
                "the floor's lower side, written against the constant"
            );

            // ── the floor itself is INCLUSIVE ──────────────────────────
            assert_eq!(
                ca.set_persistent_keepalive(Some(PERSISTENT_KEEPALIVE_MIN)),
                Ok(()),
                "ruling 42: 1 s exactly is admissible — App. B calls a test \
                 that rejects it \"the mirror regression\""
            );

            // ── the interior ───────────────────────────────────────────
            for good in [
                PERSISTENT_KEEPALIVE_MIN + ms(1),
                Duration::from_secs(2),
                PERSISTENT_KEEPALIVE_DEFAULT,
                Duration::from_secs(20),
                DEAD_TIMEOUT - ms(1),
            ] {
                assert_eq!(
                    ca.set_persistent_keepalive(Some(good)),
                    Ok(()),
                    "ruling 40: {good:?} is inside [1 s, DEAD_TIMEOUT)"
                );
            }

            // ── the ceiling itself is EXCLUSIVE ────────────────────────
            assert_eq!(
                ca.set_persistent_keepalive(Some(DEAD_TIMEOUT)),
                Err(ConfigError::KeepaliveTooLong),
                "ruling 40: the bound is a ceiling and DEAD_TIMEOUT is \
                 outside it — a build that accepts this is ruling 38's \
                 inverted bound, the regression App. B names first"
            );

            // ── above the ceiling ──────────────────────────────────────
            for bad in [
                DEAD_TIMEOUT + ms(1),
                Duration::from_secs(30),
                Duration::from_secs(60 * 60),
                Duration::MAX,
            ] {
                assert_eq!(
                    ca.set_persistent_keepalive(Some(bad)),
                    Err(ConfigError::KeepaliveTooLong),
                    "ruling 40: {bad:?} is at or above DEAD_TIMEOUT"
                );
            }

            // ── `None` disables, and is accepted at all times ──────────
            assert_eq!(
                ca.set_persistent_keepalive(None),
                Ok(()),
                "§3.3: None is accepted at all times"
            );
        })
        .await;
}

/// **S5 (opt-in): a rejected interval leaves a configured beacon
/// *unchanged*, and does not clamp.**
///
/// Ruling 44 forbids three outcomes for an out-of-range interval — a
/// panic, a `debug_assert`, and a silent clamp — and requires the current
/// interval to survive untouched. The first two are unobservable from a
/// test that passes (a panic fails every test in the file, which is the
/// point), so this one pins the third, **behaviourally**: `CONTRACT-7.md`
/// §6 gives the shell no reader for the configured interval, so the only
/// evidence available is the cadence the beacon actually keeps.
///
/// # The broken implementations this catches
///
/// * **A clamp to the ceiling.** The interval becomes ~25 s and the
///   beacon falls silent inside this window; the connection then has
///   nothing sustaining it and dies. Caught by both assertions.
/// * **A clamp to the floor** (1 s). The cadence *speeds up* rather than
///   staying put, so the count is asserted from **both** sides — `>= 6`
///   says the 2 s beacon is still running, `<= 40` says it was not
///   silently re-tuned to 1 s. An upper bound alone would pass a build
///   that clamped downward, and a lower bound alone would pass a build
///   that clamped upward; only the pair pins *unchanged*.
/// * **A rejection that disables the beacon** (`Err` handled as
///   `Some(x) → None`). Caught by the lower bound and by the survival.
///
/// The window is 20 s: at 2 s that is ~10 sends, at the 10 s the passive
/// rule alone would give it is ~2, and at 1 s it is ~20. The three are
/// separated by the bounds rather than merely bounded above.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_a_rejected_interval_leaves_the_configured_beacon_unchanged() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7051);
            let b = Node::spawn(&net, 2, 7052);
            let (ca, _cb) = establish(&a, &b).await;

            ca.set_persistent_keepalive(Some(Duration::from_secs(2)))
                .expect("2 s is inside [1 s, DEAD_TIMEOUT)");

            assert_eq!(
                ca.set_persistent_keepalive(Some(Duration::from_secs(30))),
                Err(ConfigError::KeepaliveTooLong),
                "ruling 40's ceiling"
            );
            assert_eq!(
                ca.set_persistent_keepalive(Some(Duration::from_millis(10))),
                Err(ConfigError::KeepaliveTooShort),
                "ruling 42's floor"
            );

            settle().await;
            let before = sent_from(&tap, a.addr);

            let window = Duration::from_secs(20);
            alive_through(
                window,
                &ca,
                "ruling 44: a rejected call leaves the 2 s beacon in place",
            )
            .await;

            let sent = sent_from(&tap, a.addr) - before;
            assert!(
                sent >= 6,
                "ruling 44: no clamp upward — the 2 s beacon owes ~10 sends \
                 in {window:?} and produced {sent}; a clamp to the ceiling \
                 leaves only the passive rule's ~2"
            );
            assert!(
                sent <= 40,
                "ruling 44: no clamp downward either — {sent} sends in \
                 {window:?} is a beacon faster than the 2 s that was \
                 accepted, i.e. the rejected value was applied at the floor"
            );
        })
        .await;
}

/// **S5 (opt-in): a rejected interval does not *enable* a beacon that was
/// off.**
///
/// The mirror of the test above, and the case ruling 44's *"reports
/// success while giving a beacon that does not do what was asked"* does
/// not cover on its own: here nothing was ever asked for successfully, so
/// "unchanged" means **still disabled**. §3.3's default is off.
///
/// # The broken implementation this catches
///
/// A validator that clamps into range and *then* stores — the connection
/// acquires a 1 s or 24.999 s beacon it was never given, which turns
/// S5's headline reap into an immortal idle connection. Caught by the
/// reap: the connection must still die at `DEAD_TIMEOUT`, exactly as
/// `s5_a_connection_with_no_traffic…` requires, and it must emit nothing
/// on the way there.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s5_a_rejected_interval_does_not_enable_a_beacon_that_was_off() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7061);
            let b = Node::spawn(&net, 2, 7062);
            let (ca, _cb) = establish(&a, &b).await;

            assert_eq!(
                ca.set_persistent_keepalive(Some(Duration::ZERO)),
                Err(ConfigError::KeepaliveTooShort)
            );
            assert_eq!(
                ca.set_persistent_keepalive(Some(DEAD_TIMEOUT)),
                Err(ConfigError::KeepaliveTooLong)
            );

            settle().await;
            let before = sent_from(&tap, a.addr);

            alive_through(
                DEAD_TIMEOUT - Duration::from_secs(1),
                &ca,
                "the reap is still at DEAD_TIMEOUT",
            )
            .await;

            assert_eq!(
                sent_from(&tap, a.addr),
                before,
                "ruling 44: a rejected call is not a clamped call — the \
                 beacon was off and stays off, so nothing is emitted"
            );

            let lost = dies_within(
                Duration::from_secs(1) + SHELL_LATENESS_BOUND,
                &ca,
                "ruling 44: a clamped-and-stored interval would hold this \
                 connection open past DEAD_TIMEOUT and silently break S5",
            )
            .await;
            assert_eq!(lost, ConnectionLost::TimedOut);
        })
        .await;
}

// ═══════════════════════════════════════════════════════════════════════
// S11 — the contested probe, application-visible
// ═══════════════════════════════════════════════════════════════════════

/// **S11 — a `None`-basis refusal marks the connection, the probe goes
/// out, and a live peer clears it with the refusal standing.**
///
/// The whole of §12.2's happy branch, at the shell:
///
/// > basis is None ⇒ `Err(AcceptError::Stale)` … `Contested` … an ACK
/// > covering any counter `>= floor` arrives before the deadline … the
/// > connection lives. The refusal stands.
///
/// `A` dialled `B`, so `A`'s `replacement_basis` for `B`'s static is
/// `None` (§17.4) and `A`'s address for `B` is **validated** (§3.2) — by
/// §8.3 that makes the mark and the transmission the same instant, so the
/// notification is claimable at the refusal.
///
/// The candidate is a second endpoint holding `B`'s **static key** at a
/// different address: that is what a restart looks like from `A`, and it
/// is the only way to park an initiation for a static `A` already has
/// live. Climbing the full ladder to `Proven` is what makes it an
/// *admitted* candidate under ruling 177 — a refusal by anything weaker
/// marks nothing.
///
/// # The broken implementations this catches
///
/// * **No mark at all** — slice 2a's behaviour, where §6.4's LIVE branch
///   returns `Stale` and nothing else. Caught by the `Contested`
///   assertion, and *only* by it: every other assertion here is satisfied
///   by a connection nobody probed.
/// * **A notification fired at the mark but never resolved.** Caught by
///   `ContestCleared`.
/// * **A clear predicate that is packet identity rather than a counter
///   high-water mark** (ruling 41's declined alternative). Caught by the
///   survival past the deadline: an implementation that waits for one
///   specific packet's ACK is a coin flip here, and a red is the correct
///   verdict for it.
/// * **A refusal that replaces the connection** — the S3b behaviour
///   applied to the wrong row of §4.1's table. Caught by `session_id()`,
///   which must be the *same* session afterwards, and by `cb` still being
///   alive: `ConnectionLost::Replaced` would have fired.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s11_a_none_basis_refusal_marks_contested_and_a_live_peer_clears_it() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7101);
            let b = Node::spawn(&net, 2, 7102);
            let (ca, cb) = establish(&a, &b).await;
            let session_before = ca.session_id();

            // B's static, restarted elsewhere. Same key, new endpoint.
            let b2 = Node::spawn(&net, 2, 7103);

            let refused = {
                let mut dial = pin!(
                    b2.ep()
                        .connect(a.addr, a.pk)
                        .expect("the restarted peer dials A")
                );
                tokio::select! {
                    r = dial.as_mut() => panic!(
                        "§6.4: A's static for B is LIVE with a None basis, so the \
                         accept must be refused — the dial resolved with {r:?}"
                    ),
                    r = staged_accept(&a) => r,
                }
            };

            assert_eq!(
                refused.err(),
                Some(AcceptError::Stale),
                "§4.1: admitted candidate, basis = None ⇒ Stale, old \
                 connection untouched, guard record reverted"
            );

            // §8.3, validated-address row: the notification is at the
            // transmission, and on a validated address that is the mark.
            let marked = notified_within(
                SHELL_LATENESS_BOUND * 4,
                &ca,
                "rulings 45/46: the marking is application-visible through \
                 §16.2's notification stream, at probe transmission",
            )
            .await;
            assert_eq!(marked, Notification::Contested);

            // ~1 RTT on a zero-delay fabric plus §13's ack delay. One
            // second is forty times over-generous and still decisive
            // against the 10 s verdict deadline.
            let cleared = notified_within(
                Duration::from_secs(1),
                &ca,
                "ruling 41: the PING is ack-eliciting and the peer's ACK \
                 covers the probe floor",
            )
            .await;
            assert_eq!(cleared, Notification::ContestCleared);

            // Past the deadline the probe would have had. A build that
            // never clears kills the connection here.
            alive_through(
                KEEPALIVE_TIMEOUT * 2,
                &ca,
                "§7.5: a cleared mark disarms TimerKind::Contested — the \
                 connection survives its own probe",
            )
            .await;

            assert_eq!(
                ca.session_id(),
                session_before,
                "§4.1: a None-basis refusal leaves the old connection \
                 UNTOUCHED — it is not the S3b replacement row"
            );
            let mut peer_alive = pin!(cb.closed());
            assert!(
                poll_once(peer_alive.as_mut()).await.is_pending(),
                "the refusal stands and nothing was replaced: the peer would \
                 have seen ConnectionLost::Replaced"
            );
        })
        .await;
}

/// **S11 — a peer that cannot answer dies at the *probe* deadline, and the
/// two deadlines are 15 s apart.**
///
/// §12.2 branch (b), and §5.1's verdict step: `Closed(TimedOut)` — *"the
/// same variant as liveness, no new one"*.
///
/// The peer is made unable to answer by dropping **every** handle it has:
/// ruling 88 makes that coincident drop transmit nothing, so `A` observes
/// genuine silence rather than a polite `PeerClosed`. That is also exactly
/// what a restart is.
///
/// # The broken implementations this catches (working rule 9)
///
/// The whole test turns on **which** of two deadlines fires, because both
/// produce `ConnectionLost::TimedOut` and a variant assertion alone cannot
/// tell them apart:
///
/// * **No probe at all.** The connection still dies — of ordinary
///   liveness, at `R + DEAD_TIMEOUT`, which is 25 s from establishment
///   and therefore ~15 s late. Caught **only** by the upper bound, and
///   that is why the upper bound is `KEEPALIVE_TIMEOUT + ε` rather than
///   anything looser: a bound of `DEAD_TIMEOUT` would be satisfied by
///   the degenerate build for free.
/// * **A verdict deadline that is not `KEEPALIVE_TIMEOUT`** — an
///   immediate kill at the mark, or a deadline armed from the wrong
///   instant. Caught by the lower bound.
///
/// The post-death silence assertion is a **cross-check, not the pin**: it
/// is satisfied by a build with no probe as well, and it is here to catch
/// a verdict that seals a CLOSE where §5.1 says *"Nothing is
/// transmitted."* The interval *before* the verdict is not asserted on,
/// because §5.1 puts the probe in the sent map and PTO probes for it are
/// legitimate output.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s11_a_peer_that_cannot_answer_dies_at_the_probe_deadline_not_the_liveness_one() {
    let net = Network::new();
    let tap = net.tap();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7111);
            let mut b = Node::spawn(&net, 2, 7112);
            let (ca, cb) = establish(&a, &b).await;

            // The peer restarts: every handle it had goes at once, which
            // by ruling 88 seals no CLOSE. Its static comes back elsewhere.
            // Integrator, ruling 196: the endpoint handle goes FIRST. `Drop for
            // Connection` seals a CLOSE only when it is the last handle for the
            // connection **and not the last in the process** (ruling 88) -- so
            // dropping `cb` while `b.ep` is still held is a graceful close, not
            // a crash, and A dies `PeerClosed` before the probe can run. This
            // order is what the comment above always intended.
            b.ep = None;
            drop(cb);
            let b2 = Node::spawn(&net, 2, 7113);
            settle().await;

            let refused = {
                let mut dial = pin!(
                    b2.ep()
                        .connect(a.addr, a.pk)
                        .expect("the restarted peer dials A")
                );
                tokio::select! {
                    r = dial.as_mut() => panic!(
                        "§6.4: the accept must be refused while the zombie is \
                         LIVE — the dial resolved with {r:?}"
                    ),
                    r = staged_accept(&a) => r,
                }
            };
            assert_eq!(refused.err(), Some(AcceptError::Stale));

            let marked = notified_within(
                SHELL_LATENESS_BOUND * 4,
                &ca,
                "the probe went out even though nobody can answer it",
            )
            .await;
            assert_eq!(marked, Notification::Contested);

            let armed_at = Instant::now();

            alive_through(
                KEEPALIVE_TIMEOUT - Duration::from_secs(1),
                &ca,
                "§5.1: the verdict is due at armed_at + KEEPALIVE_TIMEOUT, \
                 not sooner — an earlier death is a mis-armed deadline",
            )
            .await;

            let lost = dies_within(
                Duration::from_secs(1) + SHELL_LATENESS_BOUND,
                &ca,
                "§5.1: TimerKind::Contested fires at the deadline. A build \
                 with no probe survives here and dies at DEAD_TIMEOUT — \
                 15 s later — which is the whole point of S11",
            )
            .await;

            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§5.1: the contested verdict reuses the liveness variant — \
                 §15.4 L4105–4120 adds no new one"
            );
            let elapsed = Instant::now() - armed_at;
            assert!(
                elapsed < DEAD_TIMEOUT,
                "the death arrived {elapsed:?} after the probe, which is not \
                 distinguishable from ordinary liveness at {DEAD_TIMEOUT:?}"
            );

            // Cross-check, not the pin: §5.1's "Nothing is transmitted."
            let after_death = sent_on(&tap, a.addr, b.addr);
            settle().await;
            tokio::time::advance(SHELL_LATENESS_BOUND).await;
            settle().await;
            assert_eq!(
                sent_on(&tap, a.addr, b.addr),
                after_death,
                "§5.1: the verdict transmits nothing — not a CLOSE, not \
                 anything"
            );
        })
        .await;
}

/// **S11 — a second refusal while already contested neither re-notifies
/// nor re-arms.**
///
/// > a second refusal while already contested creates no second mark,
/// > sends no second PING, and does **not** re-arm the deadline.
///
/// §5.1's `Armed` row calls the last of those *"a **security property**,
/// not an optimisation"*, and §13 gives the reason: the attacker supplies
/// the `Intro`s, so a re-armable deadline is a way to postpone the verdict
/// for ever.
///
/// # The broken implementation this catches, and how the timing separates
/// # it (working rule 9)
///
/// The mark is taken at `T`; the verdict is due at `T + 10 s`. The second
/// refusal is driven at `T + 5 s`. A build that re-arms moves the verdict
/// to `T + 15 s`, so:
///
/// * asserting only *"it eventually dies of `TimedOut`"* would pass on the
///   re-arming build — it dies too, five seconds later;
/// * asserting *"it is dead by `T + 10 s + ε`"* fails on it, and that is
///   the assertion written.
///
/// The notification half catches the other two clauses at once: §5.4's
/// slot is rewritable (ruling 185), so a second `Contested` **would** be
/// claimable after the first was drained. That it is not is the evidence
/// that no second mark was taken and no second PING was built.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s11_a_second_refusal_while_contested_neither_re_notifies_nor_re_arms() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7121);
            let mut b = Node::spawn(&net, 2, 7122);
            let (ca, cb) = establish(&a, &b).await;

            // The peer is gone, so nothing will ever clear the mark.
            // Integrator, ruling 196: the endpoint handle goes FIRST. `Drop for
            // Connection` seals a CLOSE only when it is the last handle for the
            // connection **and not the last in the process** (ruling 88) -- so
            // dropping `cb` while `b.ep` is still held is a graceful close, not
            // a crash, and A dies `PeerClosed` before the probe can run. This
            // order is what the comment above always intended.
            b.ep = None;
            drop(cb);
            settle().await;

            // Two independent restarts of B's static: each parks its own
            // admitted candidate, so the second refusal is a genuine
            // second uncontested-looking refusal and not a re-home walk.
            let b2 = Node::spawn(&net, 2, 7123);
            let b3 = Node::spawn(&net, 2, 7124);

            let first = {
                let mut dial = pin!(b2.ep().connect(a.addr, a.pk).expect("B2 dials A"));
                tokio::select! {
                    r = dial.as_mut() => panic!("the accept must be refused, got {r:?}"),
                    r = staged_accept(&a) => r,
                }
            };
            assert_eq!(first.err(), Some(AcceptError::Stale));

            let marked = notified_within(SHELL_LATENESS_BOUND * 4, &ca, "the first mark").await;
            assert_eq!(marked, Notification::Contested);
            let armed_at = Instant::now();

            // Halfway to the verdict, a second refusal.
            tokio::time::advance(KEEPALIVE_TIMEOUT / 2).await;
            settle().await;

            let second = {
                let mut dial = pin!(b3.ep().connect(a.addr, a.pk).expect("B3 dials A"));
                tokio::select! {
                    r = dial.as_mut() => panic!("the second accept must be refused too, got {r:?}"),
                    r = staged_accept(&a) => r,
                }
            };
            assert_eq!(
                second.err(),
                Some(AcceptError::Stale),
                "§4.1: the second admitted candidate is refused on the same row"
            );

            no_notification_within(
                Duration::from_secs(1),
                &ca,
                "§5.1's Armed row is a TOTAL no-op: no second mark, so no \
                 second Contested — and §5.4's slot is rewritable, so one \
                 would have been claimable if it had been queued",
            )
            .await;

            // T + 5 s + 1 s = T + 6 s. The verdict is owed at T + 10 s; a
            // re-armed deadline would not fire until T + 15 s.
            let remaining = KEEPALIVE_TIMEOUT.saturating_sub(Instant::now() - armed_at);
            let lost = dies_within(
                remaining + SHELL_LATENESS_BOUND,
                &ca,
                "ruling 41: the deadline is NOT re-armed by a second \
                 refusal — a build that re-arms is still alive here and \
                 dies five seconds later, which hands the attacker who \
                 supplies the Intros a way to postpone the verdict for ever",
            )
            .await;
            assert_eq!(lost, ConnectionLost::TimedOut);
        })
        .await;
}

/// **S11 — cleared, then marked again: a full second mark, handed over in
/// generation order.**
///
/// Two rulings in one flow, because neither is constructible without the
/// other.
///
/// **Ruling 175** replaces ruling 43's *"one probe per
/// `KEEPALIVE_TIMEOUT`"*: the honest bound is *"at most one probe per
/// mark, at most one mark per uncontested refusal, and marks cannot
/// overlap"*, and **no cooldown is added**, because a cooldown *"would
/// leave a genuine second doubt unprobed"*. A live peer clears in ~1 RTT,
/// so the next refusal — well inside 10 s of the first — is a **full
/// second mark with a fresh floor**.
///
/// **Ruling 185** governs what the application then sees. With no drain in
/// between, the slots hold `Contested`(g1) → `ContestCleared`(g2) →
/// `Contested`(g3), and the second write to the `Contested` slot takes the
/// **new** generation. So the hand-over is *cleared, then contested* —
/// which reads as "contested now".
///
/// # The broken implementations this catches
///
/// * **A `KEEPALIVE_TIMEOUT` cooldown** — ruling 43 left in place. The
///   second refusal, at ~1 s, is inside it, so no second mark is taken
///   and nothing further is ever queued: the `Contested` assertion at the
///   end times out. This is the test that exists *because* the superseded
///   bound is tempting.
/// * **A slot that keeps the older generation on rewrite** — ruling 185's
///   named inverse. That build hands over *contested, then cleared*, i.e.
///   reports a contested connection as healthy, and the two assertions
///   fire in the wrong order. Asserting only "both arrive" would pass it,
///   so the assertions are on the **sequence**.
/// * **A queue instead of a slot** (§5.4, ruling 58). Such a build hands
///   over three notifications; the drain below claims two and then
///   asserts nothing more is owed.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s11_cleared_then_marked_again_hands_over_in_generation_order() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7131);
            let b = Node::spawn(&net, 2, 7132);
            let (ca, _cb) = establish(&a, &b).await;

            let b2 = Node::spawn(&net, 2, 7133);
            let b3 = Node::spawn(&net, 2, 7134);

            // ── mark 1 ────────────────────────────────────────────────
            let first = {
                let mut dial = pin!(b2.ep().connect(a.addr, a.pk).expect("B2 dials A"));
                tokio::select! {
                    r = dial.as_mut() => panic!("the accept must be refused, got {r:?}"),
                    r = staged_accept(&a) => r,
                }
            };
            assert_eq!(first.err(), Some(AcceptError::Stale));

            // The live peer answers the PING and clears the mark. Nothing
            // is drained: both slots are left occupied on purpose.
            tokio::time::advance(Duration::from_secs(1)).await;
            settle().await;

            // ── mark 2, a full second mark: ruling 175, no cooldown ───
            let second = {
                let mut dial = pin!(b3.ep().connect(a.addr, a.pk).expect("B3 dials A"));
                tokio::select! {
                    r = dial.as_mut() => panic!("the second accept must be refused, got {r:?}"),
                    r = staged_accept(&a) => r,
                }
            };
            assert_eq!(second.err(), Some(AcceptError::Stale));
            settle().await;

            // ── the drain, in generation order ────────────────────────
            let first_out = notified_within(
                SHELL_LATENESS_BOUND * 4,
                &ca,
                "ruling 185: with Contested(g1) → ContestCleared(g2) → \
                 Contested(g3) and no drain, the Contested slot holds g3, \
                 so the CLEARED one is handed over first",
            )
            .await;
            assert_eq!(
                first_out,
                Notification::ContestCleared,
                "ruling 185: handing Contested over first would report a \
                 contested connection as healthy — \"the exact inverse of \
                 the truth\", and the one error S11 exists to prevent"
            );

            let second_out = notified_within(
                SHELL_LATENESS_BOUND * 4,
                &ca,
                "ruling 175: a live peer clears in ~1 RTT and the NEXT \
                 refusal is a full second mark — there is no cooldown, and \
                 ruling 43's \"one probe per KEEPALIVE_TIMEOUT\" is \
                 superseded",
            )
            .await;
            assert_eq!(second_out, Notification::Contested);

            // Nothing further is owed *at this instant* — and the check has
            // to be a single poll rather than a timed wait, because the
            // second mark's own ACK is ~25 ms of virtual time away and any
            // window wide enough to observe an over-full queue is also wide
            // enough to let that legitimate `ContestCleared` land. Ruling
            // 58's cancel-safety is what makes the dropped future free:
            // "a dropped future has claimed nothing".
            let mut extra = pin!(ca.notified());
            assert!(
                poll_once(extra.as_mut()).await.is_pending(),
                "§5.4: retention is one slot per kind, never a queue — two \
                 kinds were occupied and exactly two were owed"
            );
            // Integrator: `extra` is a `Pin<&mut impl Future>`, which does
            // not implement `Drop` — `drop()` on it only extends a lifetime
            // and clippy rejects it under `-D warnings`. The future is
            // released by falling out of scope here, which is what ruling
            // 58's "a dropped future has claimed nothing" refers to; the
            // author could not run clippy (the environment blocked it) and
            // reported as much rather than claiming the gate green.

            // The second mark is a **full** mark (ruling 175), so it has its
            // own probe, its own deadline and its own resolution. A build
            // that queued a second `Contested` without arming anything
            // behind it never reaches this.
            let resolved = notified_within(
                Duration::from_secs(1),
                &ca,
                "the second probe's ACK covers the fresh floor",
            )
            .await;
            assert_eq!(resolved, Notification::ContestCleared);

            alive_through(
                KEEPALIVE_TIMEOUT * 2,
                &ca,
                "both marks cleared, so no TimerKind::Contested is armed",
            )
            .await;
        })
        .await;
}

// ═══════════════════════════════════════════════════════════════════════
// S20 — a peer that restarts gets a working connection back
// ═══════════════════════════════════════════════════════════════════════

/// **S20 — where *we* dialled, the probe's verdict lets the restarted peer
/// back in, inside `DEAD_TIMEOUT`.**
///
/// > Where we dialled, the refusal plus contested probe (S11) resolves it
/// > within `DEAD_TIMEOUT`, after which the reconnect succeeds.
///
/// §12.2 branch (b)'s last line is the mechanism: *"The static drops to
/// NONE; the parked `Intro` takes an ordinary fresh `accept()` on the next
/// attempt."* The application here is the one bubble-engine's re-dial
/// scheduler models — it loops on `accept()`, tolerates `Stale`, and takes
/// the first `Ok`.
///
/// # The broken implementation this catches (working rule 9)
///
/// **A build with no contested probe.** It reconnects too — the zombie
/// dies of ordinary liveness at `R + DEAD_TIMEOUT` and the *next* §5.5
/// retransmission after that is accepted, at roughly 25 s + one
/// `RETRANSMIT_BASE`. So "the reconnect eventually succeeds" is satisfied
/// for free and asserts nothing. The pin is the **clock**: the probe path
/// resolves at the verdict, `KEEPALIVE_TIMEOUT` after the refusal, and the
/// retransmission that follows lands well inside 20 s. The bound is
/// therefore `DEAD_TIMEOUT − 3 s`, which the probe path clears with the
/// whole retransmit grid to spare and the degenerate build cannot reach at
/// all.
///
/// The old connection's death is asserted for its variant as well, because
/// `Replaced` here would mean §4.1's replacement row fired on a `None`
/// basis — the S3b behaviour applied where S3c governs.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn s20_a_restarted_peer_we_had_dialled_reconnects_inside_dead_timeout() {
    let net = Network::new();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let a = Node::spawn(&net, 1, 7201);
            let mut b = Node::spawn(&net, 2, 7202);
            let (ca, cb) = establish(&a, &b).await;

            // The restart. Ruling 88: the coincident drop seals no CLOSE,
            // so A keeps a zombie and learns nothing.
            // Integrator, ruling 196: the endpoint handle goes FIRST. `Drop for
            // Connection` seals a CLOSE only when it is the last handle for the
            // connection **and not the last in the process** (ruling 88) -- so
            // dropping `cb` while `b.ep` is still held is a graceful close, not
            // a crash, and A dies `PeerClosed` before the probe can run. This
            // order is what the comment above always intended.
            b.ep = None;
            drop(cb);
            settle().await;

            let b2 = Node::spawn(&net, 2, 7203);
            let t0 = Instant::now();

            let dial = b2
                .ep()
                .connect(a.addr, a.pk)
                .expect("the restarted peer holds nothing, so its static for A is NONE");

            // The application's side of S20: refusals are expected, and it
            // keeps offering until one is taken.
            let serve = async {
                loop {
                    match staged_accept(&a).await {
                        Ok(conn) => break conn,
                        // §4.1: `Stale` covers every refusal, and it is
                        // the expected one until the zombie is retired.
                        Err(AcceptError::Stale) => continue,
                        Err(other) => panic!(
                            "§10: slice 7 adds no AcceptError variant, and only \
                             Stale is reachable here — got {other:?}"
                        ),
                    }
                }
            };

            let (dialled, served) =
                tokio::time::timeout(DEAD_TIMEOUT - Duration::from_secs(3), async {
                    tokio::join!(dial, serve)
                })
                .await
                .expect(
                    "S20: the contested probe's verdict must retire the zombie \
                     and let the reconnect through. A build with no probe waits \
                     for ordinary liveness at DEAD_TIMEOUT and cannot finish here",
                );

            let elapsed = Instant::now() - t0;
            let fresh = dialled.expect("the restarted peer's dial resolved with an error");
            assert!(
                elapsed < DEAD_TIMEOUT - Duration::from_secs(3),
                "the reconnect took {elapsed:?}, which is ordinary liveness \
                 rather than the probe"
            );

            assert_eq!(
                served.remote_static().as_ref(),
                b.pk.as_ref(),
                "the connection A accepted is the restarted peer's"
            );
            assert_eq!(
                fresh.remote_static().as_ref(),
                a.pk.as_ref(),
                "and the restarted peer's is A's"
            );

            // §5.4: the new connection is fresh on both sides, so it is
            // not the old one handed back under a new name.
            assert_ne!(
                served.session_id(),
                ca.session_id(),
                "§5.4 L820–823: a fresh connection with fresh transport \
                 state — no state ever crosses a handshake"
            );

            let lost = dies_within(
                SHELL_LATENESS_BOUND * 4,
                &ca,
                "the zombie was retired by the contested verdict before the \
                 reconnect could be accepted",
            )
            .await;
            assert_eq!(
                lost,
                ConnectionLost::TimedOut,
                "§5.1: the contested verdict is TimedOut. Replaced here \
                 would mean §4.1's replacement row fired against a None \
                 basis, which is S3b's row and not S3c's"
            );

            // The reconnect works, which is the story's own word for it.
            served.send_datagram(b"back").expect("send_datagram");
            let got = tokio::time::timeout(Duration::from_secs(1), fresh.recv_datagram())
                .await
                .expect("the new connection carries traffic")
                .expect("recv_datagram");
            assert_eq!(got, b"back");
        })
        .await;
}

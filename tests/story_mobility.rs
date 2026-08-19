//! **Mobility and contest — S3 (a/b/c), S4, S18, S19, and S27's
//! notification half.**
//!
//! | Story | What it is |
//! |---|---|
//! | **S3a** | our own second dial is refused `ConnectError::AlreadyConnected`; the first connection is **untouched** |
//! | **S3b** | the peer's fresh handshake replaces ours **only** where we were the responder (basis `Some(t)`) **and** the candidate timestamp is strictly newer |
//! | **S3c** | where **we** dialled (basis `None`) the accept is refused `AcceptError::Stale` and the connection is marked **contested** |
//! | **S4** | simultaneous open converges: the lexicographically smaller static is the connection initiator, which fixes stream-id parity |
//! | **S18** | the peer changes network; streams continue with no re-handshake and no data loss |
//! | **S19** | *our* address changes; the move is **entirely peer-side** |
//! | **S27** | the notification stream: `AddressMoved`, `Contested`, retention, and survival of the death |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by **T-M**, the blind test author for slice 7, from `STORIES.md`
//! and `.slices/07-mobility/CONTRACT-7.md` **alone**, in a worktree cut at
//! `195c57a` with no slice-7 implementation in it. The implementer and the
//! second test author worked concurrently and were never read. Every name
//! this file spells for a slice-7 item is the contract's spelling — if one
//! does not compile, the contract and the implementation disagree, and
//! **that is the finding**, not a rename to be made here.
//!
//! # Working rule 9 — every test names the build it separates
//!
//! *A bound is only a test if the degenerate case violates it.* Each test
//! carries a `BROKEN BUILD:` block naming a concrete implementation that
//! passes a weaker version of the same test. The brief's own warning is
//! taken literally: *"a test that only checks 'the connection still works
//! after the peer moves' passes an implementation that never roamed at all
//! if the old path still delivers"* — so every roam test **vacates the old
//! address** and pins **the source the peer now sends from** and **the
//! destination we now send to**, not merely that bytes still move.
//!
//! # Ruling 183 — the paused-clock trap, and why no test here trips it
//!
//! `Congestion::reset(now)` sets `recovery_start = now` and the fence tests
//! `sent_time <= start`, so a packet sent in the **same virtual instant** as
//! a roam **is** fenced. Nothing in this file asserts on a post-roam
//! congestion event; where a test needs a post-roam send to be *unfenced* it
//! advances the clock first and says so.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Every test is `#[tokio::test(start_paused = true)]` inside
//! [`local`]. `tokio::time::timeout` is the observation instrument in both
//! directions — [`within`] asserts *it resolved*, [`is_pending`] asserts *it
//! had not resolved by then*. On the paused clock `timeout` auto-advances to
//! the next armed timer whenever every task is idle, so a keepalive, a PTO
//! and a `DEAD_TIMEOUT` all resolve inside one `within`.

#![allow(clippy::items_after_statements)]

use std::future::Future;
use std::net::SocketAddr;
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use slither::config::{Config, WallClock};
use slither::constants::{
    DEAD_TIMEOUT, INIT_PACKET_LEN, KEEPALIVE_TIMEOUT, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP,
    RESP_PACKET_LEN, SHELL_LATENESS_BOUND,
};
use slither::error::{
    AcceptError, AuthError, ConnectError, ConnectionLost, IntroError, ReadError, WriteError,
};
use slither::identity::Identity;
use slither::shell::Notification;
use slither::testutil::{
    CountingIdentity, Network, Pair, Spied, TestConnection, TestEndpoint, TestPublicKey,
    TestRecvStream, TestSendStream, addr_c, local, settle,
};
use slither::{StreamId, Timestamp};
// ══════════════════════════════════════════════════════════════════════
// FIXTURE
//
// `Pair` / `Peer` / `local` / `settle` / `Tap` / `Network::inject` are
// slice-0..6 harness, used as shipped. `Peer::rebind`, `Peer::addr()` and
// `SharedWire::rebind` are ruling 180's landing, committed at the base
// commit; `CONTRACT-7.md` §9 forbids editing them and this file does not.
//
// Slice-7 names used here and owned by `CONTRACT-7.md`:
//   * `slither::Notification` + its three variants (§5.3)
//     [ruling 278: the crate-root spelling is gone — it is
//     `shell::Notification`, carried by the prelude]
//   * `Connection::notified()` (§6)
//   * `Connection::set_persistent_keepalive()` (§6)
//   * `Connection::remote_address()` — exists; §6 makes it non-constant
//   * `ConnectionLost::Replaced` — exists; §10 says slice 7 constructs it
//     for the first time
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for something that **must** resolve. Generous, and
/// free: on the paused clock an idle runtime jumps straight to the next
/// armed timer.
const PATIENCE: Duration = Duration::from_secs(5);

/// Budget for something that must resolve only after a **retransmission**
/// train. Comfortably inside `DEAD_TIMEOUT`, so a test that waits this long
/// is never really observing a liveness death.
const PATIENCE_PTO: Duration = Duration::from_secs(15);

/// Virtual-time budget for the **"not before"** half. Short enough that a
/// handful of them stay far inside `DEAD_TIMEOUT` — a test that spends 25 s
/// proving a negative kills the connection and passes for the wrong reason.
const NOT_BEFORE: Duration = Duration::from_millis(200);

/// Await `fut`, failing loudly instead of hanging the suite.
async fn within<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE:?} of virtual time"),
    }
}

/// [`within`] with the retransmission budget.
async fn within_pto<F: Future>(fut: F, what: &str) -> F::Output {
    match tokio::time::timeout(PATIENCE_PTO, fut).await {
        Ok(v) => v,
        Err(_) => panic!("{what}: still pending after {PATIENCE_PTO:?} of virtual time"),
    }
}

/// `true` if `fut` had **not** resolved within [`NOT_BEFORE`]. Consumes the
/// future, which is exactly the cancel-safe drop the contract promises.
async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(NOT_BEFORE, fut).await.is_err()
}

/// Poll `fut` exactly once — the instrument for "**at this instant**",
/// with no virtual time spent and so no timer given a chance to fire.
async fn poll_once<F: Future>(mut fut: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await
}

/// A payload whose every byte is a function of its offset, so a shift of
/// any length moves *every* subsequent byte. A repeated-byte payload hides
/// a dropped overlap, a re-delivered duplicate and a trusted second offset
/// alike.
fn payload(tag: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i % 251) as u8) ^ tag).collect()
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within(s.write(&buf[done..]), what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: a blocked write is `Pending`, never `Ok(0)`"
        );
        assert!(n <= buf.len() - done, "{what}: write over-claimed");
        done += n;
    }
}

/// Read exactly `want.len()` bytes and assert they are `want`.
///
/// The stall is the assertion: streams are ordered and gapless, so if a
/// byte were lost across a roam this read never completes and `within_pto`
/// panics with the stream's name.
async fn read_expect(r: &mut TestRecvStream, want: &[u8], what: &str) {
    let mut got = Vec::with_capacity(want.len());
    while got.len() < want.len() {
        let mut buf = vec![0u8; want.len() - got.len()];
        match within_pto(r.read(&mut buf), what).await {
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

/// A bidirectional stream, opened at one end and accepted at the other.
struct Bi {
    /// The opener's send half.
    o_send: TestSendStream,
    /// The opener's receive half.
    o_recv: TestRecvStream,
    /// The accepting end's send half.
    p_send: TestSendStream,
    /// The accepting end's receive half.
    p_recv: TestRecvStream,
    /// §9.1's id, which both ends read the same.
    id: StreamId,
}

/// Open a bidirectional stream at `opener` and accept it at `peer`.
///
/// §9: a stream becomes visible to the peer only when its **first STREAM
/// frame** arrives, so `open_bi()` alone leaves `accept_bi()` parked for
/// ever. The one-byte probe is what makes it visible, and it is consumed
/// here so every payload comparison in a test starts at a clean offset.
async fn bi_pair(opener: &TestConnection, peer: &TestConnection, what: &str) -> Bi {
    let bi = within(opener.open_bi(), what).await.expect("open_bi");
    let id = bi.id().expect("an opened stream has an id");
    let (mut o_send, o_recv) = bi.split();
    write_all(&mut o_send, b"\x00", what).await;

    let peer_bi = within(peer.accept_bi(), what).await.expect("accept_bi");
    assert_eq!(
        peer_bi.id(),
        Some(id),
        "{what}/§9.1: the two ends read different ids for one stream"
    );
    let (p_send, mut p_recv) = peer_bi.split();
    read_expect(&mut p_recv, b"\x00", what).await;

    Bi {
        o_send,
        o_recv,
        p_send,
        p_recv,
        id,
    }
}

/// Count the handshake datagrams in a tap window.
///
/// §3.1's length gate is **exact** for both handshake types, so a Data
/// packet that happens to open with `0x01` cannot be miscounted.
fn handshakes(spied: &[Spied]) -> usize {
    spied
        .iter()
        .filter(|s| {
            (s.bytes.len() == INIT_PACKET_LEN && s.bytes.first() == Some(&PKT_HANDSHAKE_INIT))
                || (s.bytes.len() == RESP_PACKET_LEN
                    && s.bytes.first() == Some(&PKT_HANDSHAKE_RESP))
        })
        .count()
}

/// Datagrams that left `from` addressed to `to` in a tap window.
fn sends_from_to(spied: &[Spied], from: SocketAddr, to: SocketAddr) -> usize {
    spied
        .iter()
        .filter(|s| s.src == from && s.dst == to)
        .count()
}

/// The last datagram in the window that went `from` → `to`, verbatim.
fn last_datagram(spied: &[Spied], from: SocketAddr, to: SocketAddr) -> Vec<u8> {
    spied
        .iter()
        .rev()
        .find(|s| s.src == from && s.dst == to)
        .unwrap_or_else(|| panic!("no datagram {from} → {to} in the tap window"))
        .bytes
        .clone()
}

/// A fourth address, for the two-roam retention test. `addr_c()` is the
/// third and lives in `testutil`.
fn addr_d() -> SocketAddr {
    "10.0.0.4:4004".parse().expect("literal addr")
}

// ── the identity-controlled harness ────────────────────────────────────
//
// `Pair` cannot serve S3b/S3c/S4: those need two endpoints that share one
// **static** (a restart) or a chosen static **ordering** (a tie-break), and
// `Peer` exposes a public key but never an `Identity`. This is `Pair`'s
// shape with the identity seed and the wall clock in the caller's hands.
// It deliberately has no `rebind` — `EndpointBuilder::wire` takes the wire
// by value and only `SharedWire` (whose constructor is private) survives
// the move, so every rebind test in this file runs on `Pair`.

/// A [`WallClock`] frozen at one reading, so §5.3's initiation timestamp is
/// chosen by the test rather than by the host.
struct FrozenClock(Timestamp);

impl WallClock for FrozenClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// One endpoint with a caller-chosen identity seed and wall clock.
struct Node {
    ep: TestEndpoint,
    pk: TestPublicKey,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet` (§16.3 spawns the driver with
    /// `spawn_local`).
    fn spawn(net: &Network, key_seed: u8, addr: SocketAddr, wall: Option<Timestamp>) -> Node {
        let id: slither::testutil::TestIdentity = CountingIdentity::seeded([key_seed; 32]);
        let pk = *Identity::public_static(&id);
        let mut config = Config::new();
        if let Some(t) = wall {
            config = config.with_clock(Rc::new(FrozenClock(t)));
        }
        let ep = TestEndpoint::builder()
            .identity(id)
            .wire(net.wire(addr))
            .config(config)
            .rng_seed([key_seed ^ 0x5A; 32])
            .build();
        Node { ep, pk, addr }
    }
}

/// The static a given key seed produces, without spawning anything.
///
/// Used to *choose* seeds so S4 runs under **both** orderings — the story's
/// "holds under both orderings" is a claim a single seed cannot pin.
fn static_for(key_seed: u8) -> TestPublicKey {
    let id: slither::testutil::TestIdentity = CountingIdentity::seeded([key_seed; 32]);
    *Identity::public_static(&id)
}

/// A pair of key seeds whose statics have the requested ordering.
///
/// §6.7 compares §2.4's canonical octets as unsigned lexicographic strings,
/// which is `[u8]`'s own `Ord`.
fn seeds_with(a_is_smaller: bool) -> (u8, u8) {
    let a = 1u8;
    let a_pk = static_for(a);
    for b in 2..=255u8 {
        let b_pk = static_for(b);
        if (a_pk.as_ref() < b_pk.as_ref()) == a_is_smaller {
            return (a, b);
        }
    }
    panic!("no key seed in 2..=255 gives the requested static ordering");
}

/// Where the staged ladder stopped, so a test can say *which* stage
/// refused rather than only that something did.
#[derive(Debug)]
enum LadderStop {
    Intro(IntroError),
    Auth(AuthError),
    Accept(AcceptError),
}

/// Climb §6.2's ladder to the end: `accept()` → `read_identity()` →
/// `authenticate()` → `accept()`.
async fn climb(ep: &TestEndpoint) -> Result<TestConnection, LadderStop> {
    let intro = within(ep.accept(), "endpoint.accept")
        .await
        .expect("the endpoint driver is alive and an Intro is parked");
    let claimed = within(intro.read_identity(), "read_identity")
        .await
        .map_err(LadderStop::Intro)?;
    let proven = within(claimed.authenticate(), "authenticate")
        .await
        .map_err(LadderStop::Auth)?;
    within(proven.accept(), "proven.accept")
        .await
        .map_err(LadderStop::Accept)
}

// ══════════════════════════════════════════════════════════════════════
// S3a — our own second dial is refused, and the first connection lives
// ══════════════════════════════════════════════════════════════════════

/// **S3a — `connect()` to a live static is `AlreadyConnected`, and the
/// first connection is untouched.**
///
/// > `connect()` to a static that already has a live `Connection` returns
/// > `Err(ConnectError::AlreadyConnected)`. The first connection is
/// > untouched. One connection per static is an invariant, so "the second
/// > closes the first" is *not* what happens locally.
///
/// # BROKEN BUILD this separates
///
/// * **"the second dial closes the first"** — the obvious wrong guess, and
///   the one S3's ⚠ CHECK exists to forbid. It returns `Ok` (or refuses
///   *after* tearing down), and dies on the post-refusal round trip: the
///   assertion is not "the call returned an error" but "the stream that was
///   already open still carries bytes **after** it".
/// * **a refusal that spends DH** — one that runs the ladder and only then
///   notices the conflict. Ruling 90 makes `mint_pending` synchronous and
///   0 DH; a refusal that moves the counter fails the `dhs` assertion while
///   passing every other line here.
/// * **a refusal that resolves `closed()`** — the `poll_once` pins the old
///   connection as still live *at that instant*, with no virtual time in
///   which a teardown could be blamed on something else.
#[tokio::test(start_paused = true)]
async fn s3a_a_second_dial_is_refused_and_the_first_connection_is_untouched() {
    local(async {
        let pair = Pair::seeded(0x3A_0001);
        let (ca, cb) = pair.establish().await;

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb) = (bi.o_send, bi.p_recv);

        let before = payload(0x11, 4096);
        write_all(&mut sa, &before, "S3a first write").await;
        read_expect(&mut rb, &before, "S3a first read").await;

        let dhs_before = pair.a.dhs.get();
        let session_before = ca.session_id();

        // ── the second dial ──────────────────────────────────────────
        let refused = pair.a.endpoint.connect(pair.b.addr(), pair.b.public_static);
        assert!(
            matches!(refused, Err(ConnectError::AlreadyConnected)),
            "S3a/§16.1: a second `connect()` to a LIVE static must be \
             `Err(ConnectError::AlreadyConnected)`; got a different outcome"
        );

        assert_eq!(
            pair.a.dhs.get(),
            dhs_before,
            "ruling 90: `mint_pending` is synchronous and spends 0 DH, so a \
             refused dial must not move the endpoint's DH counter"
        );

        // ── the first connection is untouched, at this instant ───────
        assert!(
            poll_once(pin!(ca.closed())).await.is_pending(),
            "S3a: the refused dial resolved the FIRST connection's `closed()`"
        );
        assert_eq!(
            ca.session_id(),
            session_before,
            "S3a: the refused dial replaced the session under the live handle"
        );
        assert_eq!(
            ca.remote_address(),
            pair.b.addr(),
            "S3a: the refused dial moved the live connection's anchor"
        );

        // ── … and still carries bytes ────────────────────────────────
        let after = payload(0x22, 4096);
        write_all(&mut sa, &after, "S3a write after the refusal").await;
        read_expect(&mut rb, &after, "S3a read after the refusal").await;

        assert!(
            is_pending(ca.notified()).await,
            "§5.3: a refused *local* dial is not one of the three \
             `Notification` kinds and must produce none"
        );
    })
    .await;
}

/// **S3a — the guard is on the static, not on liveness: a second dial while
/// the first is still PENDING is refused too.**
///
/// `CONTRACT-7.md` §4.2: `mint_pending` refuses when
/// `statics.get(&key).is_some()` — *"live or pending alike"*. S3a's prose
/// says "live", and working rule 8 says a stated construction is read as
/// exhaustive whether or not it says so; this is the half the prose omits.
///
/// # BROKEN BUILD this separates
///
/// One that keys the refusal on `StaticState::Live` only. It passes the
/// test above and produces **two concurrent dials to one static** here —
/// two pendings, two indices, and §16.1's one-session-per-static invariant
/// broken before either completes.
#[tokio::test(start_paused = true)]
async fn s3a_a_second_dial_while_the_first_is_pending_is_refused_too() {
    local(async {
        let net = Network::new();
        let a = Node::spawn(&net, 1, "10.0.1.1:5001".parse().expect("addr"), None);
        // Bound, but its application never calls `accept()`: msg1 parks in
        // its stage-0 queue and nothing comes back, so `a`'s static for `b`
        // stays PENDING for the whole `HANDSHAKE_GIVEUP` window.
        let b = Node::spawn(&net, 2, "10.0.1.2:5002".parse().expect("addr"), None);

        let first = a.ep.connect(b.addr, b.pk).expect("the first dial is Ok");
        settle().await;

        let second = a.ep.connect(b.addr, b.pk);
        assert!(
            matches!(second, Err(ConnectError::AlreadyConnected)),
            "CONTRACT-7 §4.2: `mint_pending` refuses a static that is already \
             in the map — **live or pending alike**"
        );

        // The first dial is genuinely still running, so the refusal above was
        // not an artefact of a dial that had already given up.
        assert!(
            poll_once(pin!(first)).await.is_pending(),
            "the first dial had already resolved; the PENDING case was not tested"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S3b — the peer's fresh handshake replaces ours, where it may
// ══════════════════════════════════════════════════════════════════════

/// **S3b — a strictly newer initiation from a static we ACCEPTED replaces
/// the connection, at the `accept()` call and not before.**
///
/// > A new handshake from an already-live static drops the old connection
/// > and installs the new one … But replacement requires a
/// > `replacement_basis` of `Some(t)` **and** a strictly newer candidate
/// > timestamp. We hold `Some(t)` only where we were the **responder**.
/// > **Accepts:** the old connection's handle surfaces
/// > `ConnectionLost::Replaced`; in-flight stream data on it is lost; the
/// > new connection is independent, with fresh stream state.
///
/// `CONTRACT-7.md` §12.3 is the sequence, and §4.1 the decision row
/// *(admitted candidate, `basis = Some(t)`, `t_cand > t` ⇒ `Ok`, old
/// connection `Replaced` **fired at this call**)*.
///
/// # BROKEN BUILD this separates
///
/// * **the pre-slice-7 `staged.rs:617`** — `Some((StaticState::Live, _)) =>
///   Err(AcceptError::Stale)` unconditionally. Caught by the `Ok` on
///   `climb`.
/// * **a replacement that fires at the *park*** — one that tears the zombie
///   down when the msg1 arrives rather than when the application accepts.
///   §4.1: *"the `Replaced` teardown fires exactly at the replacing
///   `accept()`, never earlier."* Caught **only** by the `poll_once` before
///   the accept, with no virtual time spent; a test that accepts first can
///   never see the difference.
/// * **a replacement that reports the wrong reason** — `TimedOut`,
///   `LocallyClosed` or `PeerClosed` instead of `Replaced`. §10: `Replaced`
///   exists and is *constructed for the first time* by this slice, so the
///   variant is the pin.
/// * **a replacement that migrates transport state** — §5.4: *"no stream,
///   flow-control, recovery, or congestion state ever crosses a
///   handshake."* Caught by the fresh connection having no inbound stream
///   and by the old stream handle failing rather than re-homing.
#[tokio::test(start_paused = true)]
async fn s3b_a_newer_initiation_from_a_static_we_accepted_replaces_at_the_accept() {
    local(async {
        let net = Network::new();
        // `a` accepts; `b` dials. That is what puts `Some(t0)` in a's basis.
        let a = Node::spawn(&net, 1, "10.0.2.1:5001".parse().expect("addr"), None);
        let b = Node::spawn(
            &net,
            2,
            "10.0.2.2:5002".parse().expect("addr"),
            Some(Timestamp::new(1_000_000, 0)),
        );

        let dial = b.ep.connect(a.addr, a.pk).expect("b dials a");
        let (dialled, accepted) = tokio::join!(within(dial, "b dial"), climb(&a.ep));
        let cb_old = dialled.expect("b's dial completed");
        let ca_old = accepted.expect("a accepted b's initiation");

        // Traffic, so the old connection is a real one and not a husk.
        let bi = bi_pair(&cb_old, &ca_old, "b opens, a accepts").await;
        let (mut sb, mut sa_old, mut ra_old) = (bi.o_send, bi.p_send, bi.p_recv);
        let msg = payload(0x33, 2048);
        write_all(&mut sb, &msg, "S3b pre-restart write").await;
        read_expect(&mut ra_old, &msg, "S3b pre-restart read").await;

        let old_session = ca_old.session_id();

        // ── b restarts: same static, nothing else, a strictly newer §5.3
        //    wall clock (§12.3: "B read a fresh wall clock") ─────────────
        let b2 = Node::spawn(
            &net,
            2,
            "10.0.2.3:5003".parse().expect("addr"),
            Some(Timestamp::new(1_000_001, 0)),
        );
        let dial2 = b2.ep.connect(a.addr, a.pk).expect("b' dials a");
        settle().await;

        // §12.3: "The zombie keeps running, untouched." The parked Intro has
        // cost the old connection nothing **at this instant**.
        assert!(
            poll_once(pin!(ca_old.closed())).await.is_pending(),
            "§4.1: the `Replaced` teardown fired at the PARK, not at the \
             replacing `accept()`"
        );

        // ── the replacing accept ─────────────────────────────────────
        let (replaced, ca_new) = tokio::join!(within(dial2, "b' dial"), climb(&a.ep));
        let cb_new = replaced.expect("b's fresh dial completed");
        let ca_new = ca_new.expect(
            "§4.1: an admitted candidate against `basis = Some(t)` with a \
             strictly newer timestamp must return `Ok`, not `Stale`",
        );

        // ── the old handle's verdict ─────────────────────────────────
        assert_eq!(
            within(ca_old.closed(), "old connection closed()").await,
            ConnectionLost::Replaced,
            "S3b: the old connection's handle must surface \
             `ConnectionLost::Replaced`, and no other reason"
        );

        // ── the new connection is independent ────────────────────────
        assert_ne!(
            ca_new.session_id(),
            old_session,
            "§5.4: the replacement installs a **fresh** connection"
        );
        assert_eq!(
            ca_new.remote_address(),
            b2.addr,
            "§5.6: the new connection anchors on the msg1 source"
        );

        // ── in-flight stream data on the old connection is lost ──────
        let orphan = payload(0x44, 1024);
        assert!(
            matches!(
                within(sa_old.write(&orphan), "write on the replaced stream").await,
                Err(WriteError::ConnectionLost(ConnectionLost::Replaced))
            ),
            "S3b: a write on the replaced connection's stream must fail with \
             `Replaced`; a build that carried the stream across the handshake \
             succeeds here"
        );
        assert!(
            matches!(
                within(ra_old.read(&mut [0u8; 64]), "read on the replaced stream").await,
                Err(ReadError::ConnectionLost(ConnectionLost::Replaced))
            ),
            "S3b: the read half dies with the connection, and with the reason \
             the connection died of"
        );
        assert!(
            is_pending(ca_new.accept_bi()).await,
            "§5.4: no stream state crosses a handshake — the fresh connection \
             must have no inbound stream at all"
        );

        // ── and the new connection works end to end ──────────────────
        let bi2 = bi_pair(&cb_new, &ca_new, "b' opens on the fresh connection").await;
        let (mut sb2, mut ra2) = (bi2.o_send, bi2.p_recv);
        let fresh = payload(0x55, 2048);
        write_all(&mut sb2, &fresh, "S3b post-restart write").await;
        read_expect(&mut ra2, &fresh, "S3b post-restart read").await;
    })
    .await;
}

/// **S3b, the other side of the bound — a candidate that is NOT strictly
/// newer never replaces.**
///
/// Working rule 9: the test above passes a build that replaces on *any*
/// admitted candidate from a live static, because the only candidate it
/// ever offers is a newer one. This is the assertion that separates them.
///
/// # Where the refusal lands, and why this test does not pin the stage
///
/// §4.1's decision table puts `t_cand <= t` at `accept()`. Through the
/// public ladder the §17.1 timestamp guard is reached **first** (it is what
/// `authenticate()` checks), so a stale candidate is refused at stage 2
/// with `AuthError::Replay` and `accept()` is never called. Both are "no
/// replacement", and the story's claim is about the outcome, so the outcome
/// is what is asserted — the stage is merely reported. **This is recorded
/// as an observation for the integrator, not resolved here** (working rule
/// 3): the contract's row is not wrong, it is unreachable from the shell.
///
/// # BROKEN BUILD this separates
///
/// One that treats "the static is LIVE and a candidate is admitted" as
/// sufficient, dropping the `t_cand > t` half of S3b's conjunction. It
/// replaces here, and the surviving connection's round trip fails.
#[tokio::test(start_paused = true)]
async fn s3b_a_candidate_that_is_not_strictly_newer_never_replaces() {
    local(async {
        let net = Network::new();
        let a = Node::spawn(&net, 1, "10.0.3.1:5001".parse().expect("addr"), None);
        let b = Node::spawn(
            &net,
            2,
            "10.0.3.2:5002".parse().expect("addr"),
            Some(Timestamp::new(2_000_000, 0)),
        );

        let dial = b.ep.connect(a.addr, a.pk).expect("b dials a");
        let (dialled, accepted) = tokio::join!(within(dial, "b dial"), climb(&a.ep));
        let cb_old = dialled.expect("b's dial completed");
        let ca_old = accepted.expect("a accepted b's initiation");

        let bi = bi_pair(&cb_old, &ca_old, "b opens, a accepts").await;
        let (mut sb, mut ra) = (bi.o_send, bi.p_recv);
        let first = payload(0x66, 2048);
        write_all(&mut sb, &first, "S3b(neg) pre write").await;
        read_expect(&mut ra, &first, "S3b(neg) pre read").await;

        let old_session = ca_old.session_id();

        // A "restart" whose clock went **backwards** — the same static, an
        // older §5.3 timestamp.
        let b_stale = Node::spawn(
            &net,
            2,
            "10.0.3.3:5003".parse().expect("addr"),
            Some(Timestamp::new(1_999_999, 0)),
        );
        let stale_dial = b_stale.ep.connect(a.addr, a.pk).expect("the stale dial");
        settle().await;

        match climb(&a.ep).await {
            // §17.1's guard is what the public ladder reaches first …
            Err(LadderStop::Auth(AuthError::Replay)) => {}
            // … and §4.1's own row is the other admissible refusal. Which of
            // the two fires is not pinned; that both of them are refusals is.
            Err(LadderStop::Accept(AcceptError::Stale)) => {}
            Err(LadderStop::Intro(e)) => panic!(
                "S3b: a stale candidate is a *replay*, refused at the guard or \
                 at the accept — a stage-1 refusal ({e:?}) means the msg1 was \
                 unreadable and the test proved nothing"
            ),
            Err(LadderStop::Auth(e)) => {
                panic!("S3b: unexpected stage-2 refusal {e:?} for a stale candidate")
            }
            Err(LadderStop::Accept(e)) => {
                panic!("S3b: unexpected accept refusal {e:?} for a stale candidate")
            }
            Ok(_) => panic!(
                "S3b: a candidate that is not strictly newer must NOT install — \
                 this build replaces on any admitted candidate from a live \
                 static, dropping the `t_cand > t` half of S3b's conjunction"
            ),
        }

        drop(stale_dial);

        // ── the original connection is untouched ─────────────────────
        assert!(
            poll_once(pin!(ca_old.closed())).await.is_pending(),
            "S3b: a stale candidate resolved the live connection's `closed()`"
        );
        assert_eq!(
            ca_old.session_id(),
            old_session,
            "S3b: a stale candidate replaced the session"
        );
        let second = payload(0x77, 2048);
        write_all(&mut sb, &second, "S3b(neg) post write").await;
        read_expect(&mut ra, &second, "S3b(neg) post read").await;
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S3c — where we dialled, the accept is refused and the connection is
//        marked contested
// ══════════════════════════════════════════════════════════════════════

/// **S3c — basis `None` ⇒ `AcceptError::Stale`, the connection untouched
/// and marked contested.**
///
/// > Where we dialled (basis `None`, because msg2 carries no payload and a
/// > dialler never learns a peer timestamp), the accept is refused
/// > `AcceptError::Stale` and the connection is marked **contested**.
///
/// `CONTRACT-7.md` §4.1 row 3, §5.1's marking table, and §8.3's
/// mark/transmission/verdict separation. **This connection was dialled, so
/// its address is validated (§3.2) and the budget never binds** — the mark
/// and the transmission fall in the same instant, and `Notification::
/// Contested` is observable immediately. The *pending gap*, where they
/// separate, is reachable only on a connection that has roamed (§8.3), and
/// is not this story.
///
/// # BROKEN BUILD this separates
///
/// * **S3b's rule applied to a `None` basis** — a build that replaces
///   whenever a newer candidate is admitted, without consulting the basis.
///   It returns `Ok` and kills the dialled connection; caught by the
///   `Stale` assertion *and* by the surviving round trip.
/// * **a refusal that does not mark** — the pre-slice-7 `Stale` return,
///   which is a correct refusal and a silent one. Caught **only** by the
///   `Contested` notification; a test that asserted the refusal alone
///   passes it, which is exactly the "a name is not a pin" shape.
/// * **a mark that tears the connection down** — caught by the round trip
///   after the refusal.
/// * **a second mark on a second refusal** — S11: *"a second refusal while
///   already contested creates no second mark, sends no second PING, and
///   does not re-arm the deadline."* §5.1 makes `Armed { .. }` a total
///   no-op. Caught by the second `notified()` staying pending.
#[tokio::test(start_paused = true)]
async fn s3c_a_refusal_against_a_none_basis_is_stale_and_marks_contested() {
    local(async {
        let net = Network::new();
        // `a` DIALS, so a's basis for b's static is `None`.
        let a = Node::spawn(&net, 1, "10.0.4.1:5001".parse().expect("addr"), None);
        let b = Node::spawn(
            &net,
            2,
            "10.0.4.2:5002".parse().expect("addr"),
            Some(Timestamp::new(3_000_000, 0)),
        );

        let dial = a.ep.connect(b.addr, b.pk).expect("a dials b");
        let (dialled, accepted) = tokio::join!(within(dial, "a dial"), climb(&b.ep));
        let ca = dialled.expect("a's dial completed");
        let cb = accepted.expect("b accepted a's initiation");

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb) = (bi.o_send, bi.p_recv);
        let first = payload(0x88, 2048);
        write_all(&mut sa, &first, "S3c pre write").await;
        read_expect(&mut rb, &first, "S3c pre read").await;

        let session_before = ca.session_id();
        assert!(
            is_pending(ca.notified()).await,
            "§5.3: a healthy dialled connection has produced no notification"
        );

        // ── a peer holding b's static reconnects ─────────────────────
        let b2 = Node::spawn(
            &net,
            2,
            "10.0.4.3:5003".parse().expect("addr"),
            Some(Timestamp::new(3_000_001, 0)),
        );
        let contender = b2.ep.connect(a.addr, a.pk).expect("b' dials a");
        settle().await;

        match climb(&a.ep).await {
            Err(LadderStop::Accept(AcceptError::Stale)) => {}
            other => panic!(
                "S3c/§4.1: an admitted candidate against a `None` basis must be \
                 refused `AcceptError::Stale`; got {other:?}"
            ),
        }

        // ── the connection is untouched … ────────────────────────────
        assert!(
            poll_once(pin!(ca.closed())).await.is_pending(),
            "S3c: the refusal tore the dialled connection down"
        );
        assert_eq!(
            ca.session_id(),
            session_before,
            "S3c: the refusal replaced the session it was supposed to defend"
        );

        // ── … and marked contested, application-visibly (S11/S27) ────
        assert_eq!(
            within(ca.notified(), "a.notified after the refusal")
                .await
                .expect("the connection is alive, so this is not a death"),
            Notification::Contested,
            "S3c + rulings 45/46: the refusal must mark the connection \
             contested and the marking must reach the application through \
             §16.2's notification stream"
        );

        // ── a second refusal is a TOTAL no-op (§5.1, S11) ────────────
        //
        // The first contender's dial is cancelled first, so the second
        // refusal is unambiguously a second *mark* attempt and not this
        // one's retransmit being re-accepted. (Either would be `Stale` and
        // neither may mark again, so the assertion is the same; the drop is
        // for the reader.)
        // **Integrator, ruling 202.** Block the *original* peer's return
        // path before the second refusal. Without this the live peer ACKs
        // the first probe within a round trip, the mark **clears**, and the
        // second refusal is a full second mark — which ruling 175 says is
        // correct, so the assertion below would be pinning the behaviour
        // ruling 43 had and ruling 175 removed. S11's no-op guarantee is
        // scoped to a refusal *while the connection is still contested*, and
        // this is what keeps it so.
        net.block_path(b.addr, a.addr);

        drop(contender);
        let b3 = Node::spawn(
            &net,
            2,
            "10.0.4.4:5004".parse().expect("addr"),
            Some(Timestamp::new(3_000_002, 0)),
        );
        let contender2 = b3.ep.connect(a.addr, a.pk).expect("b'' dials a");
        settle().await;
        match climb(&a.ep).await {
            Err(LadderStop::Accept(AcceptError::Stale)) => {}
            other => panic!("S3c: the second refusal must also be `Stale`; got {other:?}"),
        }
        assert!(
            is_pending(ca.notified()).await,
            "§5.1: a refusal while already `Armed` is a TOTAL no-op — no second \
             mark, no second PING, no second `Contested`"
        );

        // ── the refusal stands, and the connection still carries data ─
        let second = payload(0x99, 2048);
        write_all(&mut sa, &second, "S3c post write").await;
        read_expect(&mut rb, &second, "S3c post read").await;

        drop(contender2);
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S4 — simultaneous open
// ══════════════════════════════════════════════════════════════════════

/// The invariants S4 states, asserted the same way whichever route each
/// side took to them.
///
/// `smaller` is the side whose static is lexicographically smaller — S4's
/// **connection initiator for the life of the connection**.
async fn assert_s4_convergence(smaller: &TestConnection, larger: &TestConnection, what: &str) {
    assert_eq!(
        smaller.session_id(),
        larger.session_id(),
        "{what}/S4: the two sides must hold the SAME connection — a differing \
         `SessionId` is two half-connections, which is the outcome S4 exists \
         to forbid"
    );

    // §9.1's opener bit is *"a property of the id and the same on both
    // ends"*, so it is the observable that S4's "which also fixes stream-ID
    // parity" names.
    // `bi_pair` already asserts that both ends read the SAME id, which is
    // §9.1's "a property of the id and the same on both ends", and it moves
    // bytes across the stream — only agreed keys can do that, so it is also
    // the proof that the two halves really are one session.
    let s_bi = bi_pair(smaller, larger, "S4 smaller opens").await;
    assert!(
        s_bi.id.initiated_by_connection_initiator(),
        "{what}/S4: the lexicographically SMALLER static is the connection \
         initiator, so a stream it opens must carry the initiator parity"
    );

    let l_bi = bi_pair(larger, smaller, "S4 larger opens").await;
    assert!(
        !l_bi.id.initiated_by_connection_initiator(),
        "{what}/S4: the LARGER static is the acceptor, so a stream it opens \
         must carry the responder parity — a build that reads parity off \
         'I called connect()' inverts exactly here (ruling 106)"
    );
    assert_ne!(
        s_bi.id, l_bi.id,
        "{what}/§9.1: the two sides allocated the same stream id, so the \
         parity bit is not being applied at all"
    );

    let mut ss = s_bi.o_send;
    let mut lp_r = s_bi.p_recv;
    let bytes = payload(0xA5, 4096);
    write_all(&mut ss, &bytes, "S4 write").await;
    read_expect(&mut lp_r, &bytes, "S4 read").await;
}

/// **S4 — two peers dialling each other at once converge on one
/// connection, under both static orderings.**
///
/// > Both sides compare the same ordered pair of statics; the
/// > lexicographically smaller static is the connection initiator for the
/// > life of the connection, which also fixes stream-ID parity. Exactly one
/// > connection exists on each side, and it is the *same* connection.
///
/// Both `connect()` calls are made with **no `.await` between them**, so
/// both pendings are minted before either datagram can leave (ruling 90
/// makes `mint_pending` synchronous). Each side therefore meets the other's
/// msg1 with a pending of its own and takes §6.6's **internal** route.
///
/// `CONTRACT-7.md` §1.2: §6.5, §6.6 and §6.4's PENDING branch landed in
/// slice 4, so **a pass with no slice-7 implementation change is the
/// correct outcome here, not a gap.**
///
/// # Both orderings, and why one seed cannot pin it
///
/// Working rule 9: a build that hard-codes "the side that called
/// `connect()` first wins", or that compares the statics the wrong way
/// round, satisfies every assertion below under **one** ordering. The seeds
/// are chosen so the test runs both.
///
/// # BROKEN BUILD this separates
///
/// * **a comparison run the wrong way round** — fails under exactly one of
///   the two orderings, which is why both are run.
/// * **"two half-connections"** — both sides install as initiator, or both
///   as responder. Caught by the parity pair (one `true`, one `false`) and
///   by the differing stream ids.
/// * **"mutually dark"** — each drops the other's msg1 and waits for a msg2
///   that never comes. Caught by `within` on the two dials.
/// * **the crossing initiation leaking to the application** — §6.6 is
///   explicit that *"the application never sees it"*. Caught by the
///   `endpoint.accept()` assertions, which no other test here makes.
#[tokio::test(start_paused = true)]
async fn s4_simultaneous_open_converges_via_the_internal_tiebreak() {
    local(async {
        for a_is_smaller in [true, false] {
            let (ka, kb) = seeds_with(a_is_smaller);
            let net = Network::new();
            let a = Node::spawn(&net, ka, "10.0.5.1:5001".parse().expect("addr"), None);
            let b = Node::spawn(&net, kb, "10.0.5.2:5002".parse().expect("addr"), None);
            let what = if a_is_smaller { "a<b" } else { "b<a" };

            // No await between the two: both pendings exist before either
            // msg1 is on the wire.
            let da = a.ep.connect(b.addr, b.pk).expect("a dials b");
            let db = b.ep.connect(a.addr, a.pk).expect("b dials a");

            let (ra, rb) = tokio::join!(within(da, "a's dial"), within(db, "b's dial"));
            let ca = ra.expect(
                "§6.6 step 4 / §6.7: both `Connecting`s resolve `Ok` — the winner \
                 on msg2, the loser on its own cancelled pending's `Install`",
            );
            let cb = rb.expect("§6.6 step 4 / §6.7: both `Connecting`s resolve `Ok`");

            // §6.6: the crossing initiation "the application never sees".
            assert!(
                is_pending(a.ep.accept()).await,
                "{what}/§6.6: the crossing initiation must be consumed \
                 internally, never parked as an `Intro` for the application"
            );
            assert!(
                is_pending(b.ep.accept()).await,
                "{what}/§6.6: the crossing initiation must be consumed internally"
            );

            if a_is_smaller {
                assert_s4_convergence(&ca, &cb, what).await;
            } else {
                assert_s4_convergence(&cb, &ca, what).await;
            }
        }
    })
    .await;
}

/// **S4 — convergence holds when one side decides via the STAGED path and
/// the other via the internal tie-break.**
///
/// S4's accept clause names this case in terms: *"Holds under both
/// orderings, **including when one side reaches the decision via the staged
/// path and the other via the internal tie-break**."*
///
/// The mixed case is produced by letting one msg1 land **before** its
/// recipient has a pending of its own: it parks as an ordinary `Intro`
/// (`b`'s static is NONE at that moment), and only then does `b` dial. `a`
/// meets `b`'s msg1 with a pending and takes §6.6's internal route; `b`
/// meets `a`'s parked `Intro` through §6.2's ladder and reaches §6.4's
/// PENDING branch (ruling 35) — or, if `a` has already lost and answered,
/// §6.4's LIVE branch. Both refuse `b`'s accept with `Stale` on that side;
/// which of the two it is depends on scheduling, so **only the convergence
/// invariants are asserted, and the story states exactly those.**
///
/// # BROKEN BUILD this separates
///
/// * **a tie-break that only exists on the internal path** — the staged
///   `accept()` falls through to "install a second connection", and the two
///   sides end with different `SessionId`s. Ruling 35: *"the tie-break is a
///   two-sided agreement; one side cannot opt out."*
/// * **a staged loser that installs as INITIATOR** — the parity pair
///   inverts. §6.6 step 4 / ruling 106: the loser wrote msg2, so it is the
///   responder, however it got there.
#[tokio::test(start_paused = true)]
async fn s4_convergence_holds_across_the_staged_and_internal_routes() {
    local(async {
        for a_is_smaller in [true, false] {
            let (ka, kb) = seeds_with(a_is_smaller);
            let net = Network::new();
            let a = Node::spawn(&net, ka, "10.0.6.1:5001".parse().expect("addr"), None);
            let b = Node::spawn(&net, kb, "10.0.6.2:5002".parse().expect("addr"), None);
            let what = if a_is_smaller {
                "mixed a<b"
            } else {
                "mixed b<a"
            };

            // a's msg1 lands while b has NO pending: it parks as an Intro.
            let da = a.ep.connect(b.addr, b.pk).expect("a dials b");
            settle().await;

            // b climbs that Intro to `Proven` **before** it has a pending of
            // its own. This ordering is load-bearing and was found the hard
            // way: §6.5's eager path reclaims a parked initiation whose claim
            // is in the pending outbound remotes, so a `read_identity()`
            // issued *after* the local `connect()` returns
            // `IntroError::Internal` and the staged branch is never reached
            // at all. Past `authenticate()` the chain is `Proven` and the
            // decision must be taken by §6.4's staged PENDING branch.
            let intro = within(b.ep.accept(), "b.endpoint.accept")
                .await
                .expect("a's initiation is parked at b");
            let claimed = within(intro.read_identity(), "b read_identity")
                .await
                .expect("read_identity");
            let proven = within(claimed.authenticate(), "b authenticate")
                .await
                .expect("authenticate");

            // …and only now does b dial, so a meets b's msg1 with a pending
            // and takes §6.6's INTERNAL route while b takes the staged one.
            let db = b.ep.connect(a.addr, a.pk).expect("b dials a");

            let (ra, rb, staged) = tokio::join!(
                tokio::time::timeout(PATIENCE, da),
                tokio::time::timeout(PATIENCE, db),
                tokio::time::timeout(PATIENCE, proven.accept()),
            );

            // Exactly one connection on each side. `a`'s comes from its own
            // dial either way; `b`'s comes from whichever of its two routes
            // was the admitting one.
            let ca = ra
                .expect("a's dial was still pending")
                .expect("§6.6: a's `Connecting` resolves `Ok` on either branch");
            let cb = match (rb, staged) {
                (Ok(Ok(c)), Ok(Err(AcceptError::Stale))) => {
                    // b won the tie-break: its own dial completed and the
                    // crossing Intro was refused. (Whether the refusal came
                    // from §6.4's PENDING branch or its LIVE branch depends on
                    // whether a's msg2 landed first; both are `Stale`.)
                    c
                }
                (Ok(Err(ConnectError::AlreadyConnected)), Ok(Ok(c))) => {
                    // b lost the tie-break through the STAGED path: §6.4's
                    // PENDING branch cancelled b's pending — *"its
                    // `Connecting` resolves `Err(ConnectError::
                    // AlreadyConnected)`"* — and the accept installed.
                    c
                }
                (dial, ladder) => panic!(
                    "{what}/S4: exactly one of b's two routes must produce a \
                     connection and the other must refuse; got dial={dial:?}, \
                     staged={ladder:?}"
                ),
            };

            if a_is_smaller {
                assert_s4_convergence(&ca, &cb, what).await;
            } else {
                assert_s4_convergence(&cb, &ca, what).await;
            }
        }
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S18 — the peer changes network
// ══════════════════════════════════════════════════════════════════════

/// **S18 — an authenticated packet from a new source re-homes the
/// connection; streams continue with no re-handshake.**
///
/// > An authenticated, window-fresh packet from a new address re-homes the
/// > connection; streams continue with no re-handshake and no data loss.
/// > The application observes `ConnEvent::AddressMoved` and
/// > `remote_address()` reflects the new address.
///
/// The move is driven by an ordinary application write from the mover, so
/// this test fails on a **roaming** bug and not on a keepalive one; the
/// keepalive obligation gets its own test below.
///
/// # Why the old path is made unusable, and why that is the whole test
///
/// Ruling 180 abandons the vacated address's inbox, so after
/// `Peer::rebind` **nothing sent to the old address arrives**. A build that
/// never roams keeps aiming at the old anchor and the post-move round trip
/// stalls. That is the brief's requirement — *"pin the source address the
/// peer now sends to, and make the old path unusable"* — and it is why the
/// tap assertions below are stated as `0` sends to the old address rather
/// than as "some sends to the new one".
///
/// # BROKEN BUILD this separates
///
/// * **no roam at all** — `remote_address()` unchanged, the tap shows A
///   still aiming at the vacated address, and the post-move read stalls.
/// * **an event without a move** — a build that emits `AddressMoved` and
///   leaves the anchor. Caught by `remote_address()` and by
///   `sends_from_to(.., a, b_old) == 0`.
/// * **a move without an event** — caught by `notified()`.
/// * **`from`/`to` transposed** in the notification — caught by the
///   `assert_eq!` on the whole variant, which is why it is not asserted as
///   `matches!(.., AddressMoved { .. })`.
/// * **a re-handshake on the new path** — §7.3 forbids it and the story
///   says "no re-handshake". Caught by the handshake count over the
///   post-rebind tap window, which no other assertion here reaches.
/// * **an eager roam** — one that moves the anchor when *we* notice the
///   peer is unreachable rather than on authenticated receipt. Caught by
///   the assertion taken **immediately after** the rebind, before the peer
///   has sent anything.
#[tokio::test(start_paused = true)]
async fn s18_an_authenticated_packet_from_a_new_source_re_homes_the_connection() {
    local(async {
        let pair = Pair::seeded(0x18_0001);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;

        let a_addr = pair.a.addr();
        let b_old = pair.b.addr();
        assert_eq!(ca.remote_address(), b_old, "the anchor starts at b");

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);

        let p1 = payload(0x01, 4096);
        write_all(&mut sa, &p1, "S18 pre-move a→b").await;
        read_expect(&mut rb, &p1, "S18 pre-move b reads").await;
        let q1 = payload(0x02, 4096);
        write_all(&mut sb, &q1, "S18 pre-move b→a").await;
        read_expect(&mut ra, &q1, "S18 pre-move a reads").await;
        settle().await;

        assert!(
            is_pending(ca.notified()).await,
            "§5.3: a connection that has not moved has produced no notification"
        );

        // Everything from here on is the post-rebind window.
        let _ = tap.drain();

        // ── the peer's network changes ───────────────────────────────
        pair.b.rebind(addr_c());
        assert_eq!(
            pair.b.addr(),
            addr_c(),
            "ruling 180: `Peer::addr()` must follow the rebind — a field could \
             not, and would assert the pre-move address and pass"
        );
        assert_eq!(
            ca.remote_address(),
            b_old,
            "§7.3: roaming is RECEIVE-driven — the anchor may not move until \
             an authenticated, window-fresh packet arrives from the new source"
        );
        assert!(
            is_pending(ca.notified()).await,
            "§7.3: no `AddressMoved` before the peer has actually sent"
        );

        // ── the mover sends ──────────────────────────────────────────
        let q2 = payload(0x03, 4096);
        write_all(&mut sb, &q2, "S18 post-move b→a").await;
        read_expect(&mut ra, &q2, "S18 post-move a reads").await;
        settle().await;

        // Window 1: the move itself.
        let moved = tap.drain();
        assert!(
            sends_from_to(&moved, addr_c(), a_addr) > 0,
            "S18: the roam must have been triggered by a packet whose SOURCE \
             was the new address"
        );

        // ── the observables ──────────────────────────────────────────
        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "S18: `remote_address()` must reflect the new address"
        );
        assert_eq!(
            within(ca.notified(), "a.notified after the roam")
                .await
                .expect("the connection is alive"),
            Notification::AddressMoved {
                from: b_old,
                to: addr_c(),
            },
            "S18/§5.3: the application observes the move as \
             `AddressMoved {{ from: <old anchor>, to: <new source> }}`"
        );

        // ── the stream continues, in the other direction too ─────────
        let p2 = payload(0x04, 4096);
        write_all(&mut sa, &p2, "S18 post-move a→b").await;
        read_expect(&mut rb, &p2, "S18 post-move b reads").await;
        settle().await;

        // ── window 2: strictly after the roam committed ──────────────
        //
        // The window is cut here rather than at the rebind so the `== 0` is
        // an assertion about the post-roam anchor and not a race with
        // whatever we had queued for the old one.
        let after = tap.drain();
        assert_eq!(
            sends_from_to(&after, a_addr, b_old),
            0,
            "S18: after the roam, NOTHING may still be aimed at the vacated \
             address — a build that fires the event and keeps the anchor \
             lands here"
        );
        assert!(
            sends_from_to(&after, a_addr, addr_c()) > 0,
            "S18: we must be sending to the new address"
        );
        assert_eq!(
            handshakes(&moved) + handshakes(&after),
            0,
            "S18: 'streams continue with **no re-handshake**' — not one \
             `HandshakeInit` or `HandshakeResp` may cross the new path"
        );

        // The move did not restart the session.
        assert_eq!(
            ca.session_id(),
            cb.session_id(),
            "S18: the roam must not have re-keyed anything"
        );
    })
    .await;
}

/// **S18 — stream data undelivered at the moment of the move is
/// retransmitted to the new address. "No data loss" is the claim.**
///
/// The write and the rebind fall in the same instant, so the bytes are
/// aimed at the address that is about to be vacated and **cannot** arrive:
/// ruling 180 abandons that inbox precisely so this case is expressible.
/// Recovery is what closes the gap.
///
/// # BROKEN BUILD this separates
///
/// * **a roam that clears the sent map** — §13.6's first prohibition,
///   *"it does not clear the sent map"*. Those bytes are then owed by
///   nobody, the reader stalls, and `within_pto` names the stream. Every
///   other S18 assertion in this file passes such a build, because every
///   other write is settled before the move.
/// * **a roam that resets `bytes_in_flight`, `pto_count` or `loss_time`** —
///   §13.6 keeps all three; a build that resets them still recovers here,
///   so this test does *not* claim to separate those, and none of the four
///   §14.6 fences is asserted on (ruling 183: on the paused clock a
///   same-instant post-roam packet is fenced by design).
#[tokio::test(start_paused = true)]
async fn s18_data_in_flight_at_the_move_survives_it() {
    local(async {
        let pair = Pair::seeded(0x18_0002);
        let (ca, cb) = pair.establish().await;
        let b_old = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);

        // Get the dance running so both sides have R > S.
        let warm = payload(0x10, 1024);
        write_all(&mut sa, &warm, "S18(loss) warm a→b").await;
        read_expect(&mut rb, &warm, "S18(loss) warm b reads").await;
        let warm_b = payload(0x11, 1024);
        write_all(&mut sb, &warm_b, "S18(loss) warm b→a").await;
        read_expect(&mut ra, &warm_b, "S18(loss) warm a reads").await;
        settle().await;

        // ── the write and the move, in one virtual instant ───────────
        let doomed = payload(0x12, 8192);
        write_all(&mut sa, &doomed, "S18(loss) doomed write").await;
        pair.b.rebind(addr_c());
        assert_eq!(ca.remote_address(), b_old, "no roam has happened yet");

        // The mover sends, which is the only thing that can re-home us.
        let nudge = payload(0x13, 64);
        write_all(&mut sb, &nudge, "S18(loss) nudge b→a").await;
        read_expect(&mut ra, &nudge, "S18(loss) a reads the nudge").await;
        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "the nudge should have re-homed the connection"
        );

        // ── and the bytes that never made it are still owed ──────────
        //
        // `read_expect` runs on `PATIENCE_PTO`; the paused clock advances to
        // the next armed timer while the read is parked, so the loss/PTO
        // train resolves inside it with no explicit `advance`.
        read_expect(
            &mut rb,
            &doomed,
            "S18(loss) b reads the retransmitted bytes",
        )
        .await;
    })
    .await;
}

/// **S18 — a peer whose keepalive dance is running carries its own move,
/// within `KEEPALIVE_TIMEOUT`, with no application traffic at all.**
///
/// > roaming is driven by *authenticated receipt*, so **the mover must send
/// > — and the keepalive is what does it.** A peer whose dance is running
/// > (S5) carries its own move within `KEEPALIVE_TIMEOUT`.
///
/// This is the clause `s18_an_authenticated_packet_from_a_new_source_…`
/// deliberately does not test: there the move is carried by an application
/// write, so a build with no keepalive at all passes it. Here **nothing is
/// written after the rebind**, and the connection's own §7.5 machinery is
/// the only thing that can re-home it.
///
/// # Why the bound is one-sided, and why that is sound here
///
/// The peer's passive keepalive is armed at `S + KEEPALIVE_TIMEOUT` where
/// `S` is its last marking send — which is *before* the rebind — so a
/// "not before" assertion would be asserting against an interval the test
/// does not control. The separating side is the upper bound: a build with
/// no keepalive emits nothing, never re-homes inside the window, and then
/// dies at `DEAD_TIMEOUT`. Both halves of §12.4 (ii)'s self-feeding loop
/// are exercised, because A's own answering keepalive is what keeps B alive
/// while this runs.
///
/// # BROKEN BUILD this separates
///
/// * **no passive keepalive** — S5's dance missing entirely. Nothing leaves
///   B, A never re-homes, and the `remote_address()` assertion fires.
/// * **a keepalive that is sent but suppressed by `R > S` read backwards** —
///   §3.3 arms *iff* `R > S`; a build that arms iff `S > R` emits nothing on
///   exactly the connection that has been receiving, which is this one.
/// * **a keepalive that is not a marking, authenticated Data packet** —
///   fails §3.1's roam predicate at the peer and moves nothing.
#[tokio::test(start_paused = true)]
async fn s18_a_running_keepalive_dance_carries_the_move_by_itself() {
    local(async {
        let pair = Pair::seeded(0x18_0005);
        let (ca, cb) = pair.establish().await;
        let b_old = pair.b.addr();

        // One exchange in each direction, so both sides have `R > S` and
        // §12.4 (ii)'s loop is feeding itself before the move.
        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);
        let p = payload(0x70, 2048);
        write_all(&mut sa, &p, "S18(dance) a→b").await;
        read_expect(&mut rb, &p, "S18(dance) b reads").await;
        let q = payload(0x71, 2048);
        write_all(&mut sb, &q, "S18(dance) b→a").await;
        read_expect(&mut ra, &q, "S18(dance) a reads").await;
        settle().await;

        // ── the peer moves, and the application says nothing more ────
        pair.b.rebind(addr_c());
        assert_eq!(ca.remote_address(), b_old, "no roam yet");

        tokio::time::advance(KEEPALIVE_TIMEOUT + SHELL_LATENESS_BOUND + Duration::from_millis(1))
            .await;
        settle().await;

        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "S18: 'a peer whose dance is running carries its own move within \
             KEEPALIVE_TIMEOUT' — with no application traffic, §7.5's \
             keepalive is the only thing that can have done it"
        );
        assert_eq!(
            within(
                ca.notified(),
                "a.notified after the keepalive carried the move"
            )
            .await
            .expect("the connection is alive"),
            Notification::AddressMoved {
                from: b_old,
                to: addr_c(),
            },
            "S18: and the application observes it"
        );

        // The link is genuinely still usable afterwards, both ways.
        let p2 = payload(0x72, 2048);
        write_all(&mut sa, &p2, "S18(dance) post a→b").await;
        read_expect(&mut rb, &p2, "S18(dance) post b reads").await;
    })
    .await;
}

/// **S18 — the mover's positive obligation: a peer that moves and stays
/// silent dies at `DEAD_TIMEOUT`.**
///
/// > A peer that moves and stays silent is indistinguishable from one that
/// > vanished and dies at `DEAD_TIMEOUT`. This is a **positive obligation
/// > on the mover**, not a transport probe.
///
/// This is the assertion that makes ruling 180's abandoned inbox
/// load-bearing rather than a shortcut: under a carry-across rebind the
/// peer could move, stay silent, **and still receive**, so S18's central
/// claim would pass for the wrong reason (`CONTRACT-7.md` §9).
///
/// # Two-sided, deliberately
///
/// * **not before** — still alive 20 s after the move. A build that treats
///   an unreachable address as a death sentence dies here, and ruling 49 is
///   explicit that `ENETUNREACH` *"is the signal that **precedes** a
///   successful roam, not one that follows a dead connection"*.
/// * **not never** — dead by `DEAD_TIMEOUT` after the move, with
///   `TimedOut`. A build that keeps a silent, unreachable peer alive
///   for ever fails here. The one-sided version of this test passes both.
#[tokio::test(start_paused = true)]
async fn s18_a_peer_that_moves_and_stays_silent_dies_at_dead_timeout() {
    local(async {
        let pair = Pair::seeded(0x18_0003);
        let (ca, cb) = pair.establish().await;
        let a_addr = pair.a.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb) = (bi.o_send, bi.p_recv);
        let warm = payload(0x20, 2048);
        write_all(&mut sa, &warm, "S18(silent) warm").await;
        read_expect(&mut rb, &warm, "S18(silent) warm read").await;
        settle().await;

        // The peer moves, and everything it would send is blackholed — the
        // exact shape of "moved and stayed silent". A rebind clears any
        // policy attached to the old address (§9.0), so the block is applied
        // to the NEW one, after the move.
        pair.b.rebind(addr_c());
        pair.net.block_path(addr_c(), a_addr);

        // ── not before ───────────────────────────────────────────────
        tokio::time::advance(DEAD_TIMEOUT - Duration::from_secs(5)).await;
        settle().await;
        assert!(
            poll_once(pin!(ca.closed())).await.is_pending(),
            "ruling 49: a peer we cannot reach is not a dead connection — the \
             death is `DEAD_TIMEOUT` since the last authenticated RECEIVE, not \
             a verdict on a failed send"
        );

        // ── not never ────────────────────────────────────────────────
        let lost = tokio::time::timeout(
            Duration::from_secs(5) + SHELL_LATENESS_BOUND + Duration::from_secs(1),
            ca.closed(),
        )
        .await
        .expect("S18: a peer that moved and stayed silent must die at DEAD_TIMEOUT");
        assert_eq!(
            lost,
            ConnectionLost::TimedOut,
            "S18/§7.4: the silent mover is indistinguishable from one that \
             vanished, and dies of `TimedOut`"
        );
    })
    .await;
}

/// **S18's negative — a packet from a new source that is not *window-fresh*
/// does not roam, and neither does one that does not authenticate.**
///
/// `CONTRACT-7.md` §3.1's roam predicate is a conjunction of four
/// conditions, and this pins the two that a naive implementation drops.
/// `Network::inject` is the forgery fixture: it delivers **raw bytes with
/// no policy**, from a source no wire owns.
///
/// # BROKEN BUILD this separates
///
/// * **`src != anchor` ⇒ roam** — the predicate reduced to its fourth
///   condition. It re-homes onto the injected source, and every subsequent
///   packet is aimed at an address the peer does not hold: an off-path
///   attacker who can copy one datagram steers the connection into a
///   blackhole. Caught by `remote_address()` and by the surviving round
///   trip.
/// * **"authenticated" read as "it decrypted"** — ruling 169's exact
///   defect, one layer down: the replayed datagram *does* open, and only
///   §7.2's window rejects it. A build that checks the AEAD tag and not the
///   window roams here.
#[tokio::test(start_paused = true)]
async fn s18_a_replayed_or_forged_packet_from_a_new_source_does_not_roam() {
    local(async {
        let pair = Pair::seeded(0x18_0004);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let a_addr = pair.a.addr();
        let b_addr = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);
        let warm = payload(0x30, 2048);
        write_all(&mut sa, &warm, "S18(forge) warm a→b").await;
        read_expect(&mut rb, &warm, "S18(forge) warm b reads").await;
        let warm_b = payload(0x31, 2048);
        write_all(&mut sb, &warm_b, "S18(forge) warm b→a").await;
        read_expect(&mut ra, &warm_b, "S18(forge) warm a reads").await;
        settle().await;

        // A genuine, already-delivered b→a datagram: it authenticates
        // perfectly and is exactly what §7.2's replay window is for.
        let genuine = last_datagram(&tap.snapshot(), b_addr, a_addr);
        assert!(!genuine.is_empty(), "the tap saw no b→a datagram");

        pair.net.inject(addr_c(), a_addr, &genuine);
        // …and something that will not open at all, from the same source.
        let mut garbage = genuine.clone();
        if let Some(last) = garbage.last_mut() {
            *last ^= 0xFF;
        }
        pair.net.inject(addr_c(), a_addr, &garbage);
        settle().await;

        assert_eq!(
            ca.remote_address(),
            b_addr,
            "§3.1: a roam requires an AEAD-verified packet that the replay \
             window MARKED FRESH. A replay authenticates and must not roam; \
             a forgery does neither"
        );
        assert!(
            is_pending(ca.notified()).await,
            "§7.3: no `AddressMoved` may be emitted for a packet that did not \
             commit a roam"
        );

        // The connection is unharmed — so the assertion above observed a
        // live connection declining to move, not a dead one.
        let after = payload(0x32, 2048);
        write_all(&mut sa, &after, "S18(forge) post write").await;
        read_expect(&mut rb, &after, "S18(forge) post read").await;
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S19 — our address changes / NAT rebind
// ══════════════════════════════════════════════════════════════════════

/// **S19 — our own rebind is observed by the PEER and by nobody here.**
///
/// > We change interface or the NAT rebinds our mapping; the peer re-homes
/// > to our new address on our next authenticated packet.
///
/// `CONTRACT-7.md` §8.4 tabulates the asymmetry, and states the trap in
/// terms: *"A test that asserts a local `AddressMoved` after our own rebind
/// is asserting something that can never happen."* This test asserts the
/// **absence** on our side and the presence on theirs, in one run, so the
/// two cannot be confused.
///
/// It also exercises the mover being the **dialler**, whose connection
/// starts `validated` (§3.2) — the opposite side of the fixture from S18,
/// where the mover was the acceptor whose budget was armed from msg1.
///
/// # BROKEN BUILD this separates
///
/// * **an `AddressMoved` wired to the local wire's address** — a build that
///   notices *our* interface change and reports it as a move. It fires on
///   our side and fails the `is_pending` assertions, which are the only
///   ones that can catch it.
/// * **`remote_address()` reporting the local address** — caught by the
///   post-move assertion that ours is unchanged.
/// * **no peer-side roam** — caught by the post-move round trip, since our
///   old address is vacated and the peer's replies to it are dropped.
#[tokio::test(start_paused = true)]
async fn s19_our_own_rebind_moves_the_peers_anchor_and_not_ours() {
    local(async {
        let pair = Pair::seeded(0x19_0001);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let a_old = pair.a.addr();
        let b_addr = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);

        let p1 = payload(0x40, 4096);
        write_all(&mut sa, &p1, "S19 pre a→b").await;
        read_expect(&mut rb, &p1, "S19 pre b reads").await;
        let q1 = payload(0x41, 4096);
        write_all(&mut sb, &q1, "S19 pre b→a").await;
        read_expect(&mut ra, &q1, "S19 pre a reads").await;
        settle().await;
        let _ = tap.drain();

        assert_eq!(cb.remote_address(), a_old, "b's anchor starts at a");

        // ── WE move ──────────────────────────────────────────────────
        pair.a.rebind(addr_c());
        assert_eq!(
            pair.a.addr(),
            addr_c(),
            "ruling 180: `addr()` follows the rebind"
        );
        assert_eq!(
            ca.remote_address(),
            b_addr,
            "§8.4: OUR `remote_address()` is the address we send TO. Our own \
             rebind cannot change it"
        );
        assert_eq!(
            cb.remote_address(),
            a_old,
            "§7.3: the peer re-homes on RECEIPT, not on our intention"
        );

        // ── we send, which is the only thing that moves the peer ─────
        let p2 = payload(0x42, 4096);
        write_all(&mut sa, &p2, "S19 post a→b").await;
        read_expect(&mut rb, &p2, "S19 post b reads").await;
        settle().await;
        let moved = tap.drain();

        assert_eq!(
            cb.remote_address(),
            addr_c(),
            "S19: the peer re-homes to our new address on our next \
             authenticated packet"
        );
        assert_eq!(
            within(cb.notified(), "b.notified after our move")
                .await
                .expect("b's connection is alive"),
            Notification::AddressMoved {
                from: a_old,
                to: addr_c(),
            },
            "S19/§8.4: the move is observable on the PEER's side"
        );

        // ── and on our side: nothing, twice over ─────────────────────
        assert_eq!(
            ca.remote_address(),
            b_addr,
            "§8.4: our `remote_address()` is unchanged by our own move"
        );
        assert!(
            is_pending(ca.notified()).await,
            "§8.4: we fire NO local `AddressMoved` for our own rebind. This is \
             the row that can never happen, asserted from the side that can \
             observe it"
        );

        // ── the connection survived it, both ways ────────────────────
        let q2 = payload(0x43, 4096);
        write_all(&mut sb, &q2, "S19 post b→a").await;
        read_expect(&mut ra, &q2, "S19 post a reads").await;
        settle().await;

        // The window is cut after the peer's roam committed, so the `== 0` is
        // an assertion about its post-roam anchor, not a race with what it
        // had already queued for the old one.
        let after = tap.drain();
        assert_eq!(
            sends_from_to(&after, b_addr, a_old),
            0,
            "S19: after the peer re-homes, nothing may still be aimed at our \
             vacated address"
        );
        assert!(
            sends_from_to(&after, b_addr, addr_c()) > 0,
            "S19: the peer must be sending to our new address"
        );
        assert_eq!(
            handshakes(&moved) + handshakes(&after),
            0,
            "S19: a NAT rebind must not cost a handshake"
        );
    })
    .await;
}

/// **S19 — with the beacon enabled, an otherwise-silent link carries the
/// move by itself.**
///
/// > With the beacon enabled (S5) the binding is refreshed before the NAT
/// > drops it.
///
/// S18's note names the case: *"the obligation bites hardest on a quiet
/// mobile peer, because the dance only runs on a connection that has
/// already carried traffic (S5). Such a peer should enable the beacon."*
/// This is that peer. There is **no application traffic at all** — the
/// beacon is the only thing that can re-home the peer, and the peer's own
/// passive rule cannot help because `R > S` is false on both sides at
/// install (§12.4 (i)).
///
/// # BROKEN BUILD this separates
///
/// * **`set_persistent_keepalive` accepted and ignored** — nothing leaves,
///   the peer never re-homes, and (given a vacated address) the connection
///   dies. The `remote_address()` assertion is what catches it; a test that
///   merely checked the call returned `Ok(())` catches nothing at all.
/// * **a beacon that is not a real authenticated packet** — §3.4's empty
///   plaintext through `seal`. Anything that does not open at the peer
///   fails §3.1's roam predicate and the peer stays put.
/// * **a beacon that re-arms from receives** — §3.3: it *"re-arms from
///   every marking send and is **not** reset by receives"*, and it fires
///   **unconditionally**. Not separated here; that is S5's ground.
#[tokio::test(start_paused = true)]
async fn s19_the_beacon_carries_our_move_on_a_silent_link() {
    local(async {
        let pair = Pair::seeded(0x19_0002);
        let (ca, cb) = pair.establish().await;
        let a_old = pair.a.addr();

        // A short beacon, well inside §3.3's `[1 s, DEAD_TIMEOUT)` band.
        const BEACON: Duration = Duration::from_secs(2);
        ca.set_persistent_keepalive(Some(BEACON))
            .expect("§3.3: 2 s is inside the admissible band");

        settle().await;
        assert_eq!(cb.remote_address(), a_old, "b's anchor starts at a");

        // We move, and say nothing.
        pair.a.rebind(addr_c());
        assert_eq!(
            cb.remote_address(),
            a_old,
            "the peer cannot know yet — nothing has been sent"
        );

        // One beacon interval, plus the shell's permitted lateness.
        tokio::time::advance(BEACON + SHELL_LATENESS_BOUND + Duration::from_millis(1)).await;
        settle().await;

        assert_eq!(
            cb.remote_address(),
            addr_c(),
            "S19 + S5: the beacon is what refreshes the binding on an \
             otherwise-silent link, so it is what carries the move"
        );
        assert_eq!(
            within(
                cb.notified(),
                "b.notified after the beacon carried the move"
            )
            .await
            .expect("b's connection is alive"),
            Notification::AddressMoved {
                from: a_old,
                to: addr_c(),
            },
            "S19/§8.4: the move is observed by the peer"
        );
        assert!(
            is_pending(ca.notified()).await,
            "§8.4: and never by the mover"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S27 — the notification half
// ══════════════════════════════════════════════════════════════════════

/// **S27 — two unclaimed roams merge to the NET move: oldest `from`,
/// newest `to`.**
///
/// `CONTRACT-7.md` §5.4: *"An unclaimed `AddressMoved` superseded by a
/// further roam keeps the **oldest unclaimed `from`** and the **newest
/// `to`**, so the pair always describes the net move since the application
/// last looked."* Retention is **one slot per kind, never a queue**.
///
/// # BROKEN BUILD this separates — and it is two-sided
///
/// | build | `from` | `to` |
/// |---|---|---|
/// | **correct** | `addr_b` | `addr_d` |
/// | **keep the latest event** | `addr_c` ✗ | `addr_d` |
/// | **keep the earliest event** | `addr_b` | `addr_c` ✗ |
/// | **queue both** | first claim is the `b→c` pair ✗ | |
///
/// A test asserting only `to == addr_d` passes the keep-latest build; one
/// asserting only `from == addr_b` passes the keep-earliest build. The
/// whole-variant `assert_eq!` separates all three, and the trailing
/// `is_pending` separates the queue.
#[tokio::test(start_paused = true)]
async fn s27_two_unclaimed_roams_merge_to_the_net_move() {
    local(async {
        let pair = Pair::seeded(0x27_0001);
        let (ca, cb) = pair.establish().await;
        let b_first = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);
        let warm = payload(0x50, 1024);
        write_all(&mut sa, &warm, "S27 warm a→b").await;
        read_expect(&mut rb, &warm, "S27 warm b reads").await;
        settle().await;

        // ── roam one: b → addr_c. Nothing is claimed. ────────────────
        pair.b.rebind(addr_c());
        let n1 = payload(0x51, 256);
        write_all(&mut sb, &n1, "S27 nudge from addr_c").await;
        read_expect(&mut ra, &n1, "S27 a reads the first nudge").await;
        assert_eq!(ca.remote_address(), addr_c(), "the first roam committed");

        // ── roam two: addr_c → addr_d. Still nothing claimed. ────────
        pair.b.rebind(addr_d());
        let n2 = payload(0x52, 256);
        write_all(&mut sb, &n2, "S27 nudge from addr_d").await;
        read_expect(&mut ra, &n2, "S27 a reads the second nudge").await;
        assert_eq!(ca.remote_address(), addr_d(), "the second roam committed");
        settle().await;

        // ── one claim, and it describes the NET move ─────────────────
        assert_eq!(
            within(ca.notified(), "a.notified after two unclaimed roams")
                .await
                .expect("the connection is alive"),
            Notification::AddressMoved {
                from: b_first,
                to: addr_d(),
            },
            "§5.4: the merge keeps the OLDEST unclaimed `from` and the NEWEST \
             `to`, so the pair describes the net move since the application \
             last looked"
        );
        assert!(
            is_pending(ca.notified()).await,
            "§5.4: retention is ONE SLOT PER KIND, never a queue — a second \
             `AddressMoved` must not be waiting behind the first"
        );
    })
    .await;
}

/// **S27 — a notification generated before the death is still claimable
/// after it, and the death arrives once the slots are drained.**
///
/// `CONTRACT-7.md` §5.4: *"A notification generated before the death is
/// still claimable after it. `notified()` returns `Err(ConnectionLost)`
/// **only once every slot is drained**."* Ruling 152/128: a `Notification`
/// *"is a fact about the connection, complete in itself, and it survives
/// the event it describes."*
///
/// # BROKEN BUILD this separates — two-sided again
///
/// * **slots cleared at teardown** — the first `notified()` returns
///   `Err(PeerClosed)` and the move is lost. Caught by the first assertion.
/// * **a drain that never ends** — `notified()` keeps handing back the same
///   notification, or parks for ever, on a dead connection. Caught by the
///   second, and §0's ruling 152 makes parking on a dead connection
///   forbidden outright.
#[tokio::test(start_paused = true)]
async fn s27_a_notification_generated_before_the_death_survives_it() {
    local(async {
        let pair = Pair::seeded(0x27_0002);
        let (ca, cb) = pair.establish().await;
        let b_old = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut ra, mut sb, mut rb) = (bi.o_send, bi.o_recv, bi.p_send, bi.p_recv);
        let warm = payload(0x60, 1024);
        write_all(&mut sa, &warm, "S27(death) warm a→b").await;
        read_expect(&mut rb, &warm, "S27(death) warm b reads").await;
        settle().await;

        pair.b.rebind(addr_c());
        let nudge = payload(0x61, 256);
        write_all(&mut sb, &nudge, "S27(death) nudge").await;
        read_expect(&mut ra, &nudge, "S27(death) a reads the nudge").await;
        assert_eq!(ca.remote_address(), addr_c(), "the roam committed");

        // The application has NOT looked. The connection now ends.
        cb.close(7, b"gone").await;
        settle().await;
        assert!(
            !is_pending(ca.closed()).await,
            "the peer's CLOSE should have reached us"
        );

        // ── the notification survives the death ──────────────────────
        assert_eq!(
            within(ca.notified(), "a.notified after the death")
                .await
                .expect(
                    "§5.4: `notified()` returns `Err(ConnectionLost)` only once \
                     every slot is drained — the roam was generated first"
                ),
            Notification::AddressMoved {
                from: b_old,
                to: addr_c(),
            },
            "§5.4: a notification generated before the death is still claimable \
             after it"
        );

        // ── and then the death, exactly once the slots are empty ─────
        match within(ca.notified(), "a.notified once drained").await {
            Err(ConnectionLost::PeerClosed { code, .. }) => assert_eq!(
                code, 7,
                "§5.4: the drained stream ends with the reason the connection \
                 actually died of"
            ),
            other => panic!(
                "§5.4/ruling 152: once every slot is drained `notified()` must \
                 resolve `Err(ConnectionLost)` — parking on a dead connection is \
                 never permitted. Got {other:?}"
            ),
        }
    })
    .await;
}

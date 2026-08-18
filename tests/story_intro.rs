//! **The staged-accept gaps at the shell seam — S8, S10, and S24's 15 s row.**
//!
//! | Story | The clause this file closes |
//! |---|---|
//! | **S8** | *"a `Claimed` is an owned, app-held object with no lifetime, parked across turns while a human is asked … It expires at `INTRO_TTL` (15 s) if not resolved."* The **core** half is `src/core/tests.rs`'s `a_parked_decision_survives_unrelated_activity_until_its_ttl`, which drives `handle_timeout` by hand. This is the **loop** half — the one `.slices/02-handshake/PLAN.md` deferred to slice 3 and slice 3 never picked up. |
//! | **S10** | *"Under saturation, established connections keep running; the cost to hold a parked entry is bounded (measured mid-state 784 B on P-256, so 1024 parked ≈ 0.77 MiB)."* Both clauses, one behavioural and one dimensional. |
//! | **S24** | *"every timer (5 s/10 s/15 s/25 s/90 s) in virtual time"*. The **15 s** row is `INTRO_TTL`, and it is the one no flow test reached. |
//!
//! # Why the mechanism matters more than the story here
//!
//! Five facts about the code shape every assertion below, and a file written
//! from the stories alone would get three of them wrong.
//!
//! 1. **`read_identity()` performs no deadline comparison.**
//!    `core::Endpoint::read_identity_as` answers [`IntroError::Expired`] only
//!    when the `IntroId` is **absent from the queue**. So an `Expired` at this
//!    seam is not a verb noticing the time — it is evidence that the **sweep
//!    ran**.
//! 2. **The sweep runs only on `Event::Timeout`.** The driver's loop dispatches
//!    `Command` and `Received` without calling `handle_timeout`. With (1), an
//!    `Expired` observed on a **silent** endpoint is a direct observation that
//!    `core::Endpoint::deadline()` armed the intro expiry — which is what
//!    `s24_the_intro_expiry_fires_on_an_armed_timer_with_no_traffic_to_carry_it`
//!    exists to say. **Measured** (see that test's own docs): the core unit
//!    tests say it at the core seam and no pre-existing *flow* test says it at
//!    all.
//! 3. **A retransmitting dialler resets the TTL.** §6.3 rule 5 replaces an
//!    unconsumed entry's bytes and stamps `refreshed_at = now` (ruling 69's
//!    single age key), and `INTRO_TTL_MS < HANDSHAKE_GIVEUP_MS` is a `const`
//!    assertion in `src/constants.rs`. **A live dial therefore out-lives the
//!    TTL**: an S8 expiry test that leaves the dialler's path open never expires
//!    anything and passes for the wrong reason for ever. Every test here blocks
//!    the dialler's path the instant the introduction is in hand.
//! 4. **A consumed chain's age key is frozen at the initiation that fed it**,
//!    not at `read_identity()`. `s8_a_claimed_expires_fifteen_seconds_after_the_initiation_that_fed_it`
//!    is built on exactly that, and it is the half a "read it, then wait 15 s"
//!    test cannot see.
//! 5. **Dropping an `Intro` is a reject**, so it frees the slot. The flood test
//!    must *hold* all 1024 handles or the queue drains as it is measured.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::sleep_until` appears only as a **clock advance** against an
//! instant named in the test, never as a wait for work to happen; `settle()`
//! advances no virtual time at all. `tokio::time::timeout` is the observation
//! instrument, and its `Err` — *"still pending at `now + d`"* — is the "not
//! before" half that one-sided TTL tests omit.
//!
//! # Working rule 9 — every bound is written from the side that separates
//!
//! Each `INTRO_TTL` assertion is a **pair** taken on two chains parked in the
//! same virtual instant: `Ok` at `park + 15 s − 100 ms`, `Expired` at
//! `park + 15 s + 100 ms`. One side alone is satisfied by a build with no expiry
//! at all, or by one that expires everything immediately; together they exclude
//! every other ratified timer (5 s and 10 s fail the `Ok`, 25 s and 90 s fail the
//! `Expired`). The memory bound is a pair for the same reason, and there the
//! **lower** side is the one doing the work — see its own comment.

#![allow(clippy::items_after_statements)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use slither::config::Config;
use slither::constants::{INIT_PACKET_LEN, INTRO_QUEUE_CAP, INTRO_TTL, PKT_HANDSHAKE_INIT};
use slither::error::{AuthError, IntroError};
use slither::identity::Identity;
use slither::packet::Handshake;
use slither::testutil::{
    CountingIdentity, Network, Pair, TestConnecting, TestEndpoint, TestIntro, TestPublicKey, local,
    settle,
};

// ══════════════════════════════════════════════════════════════════════
// Fixture
// ══════════════════════════════════════════════════════════════════════

type Suite = slither::packet::ReferenceSuite;
type Id = CountingIdentity<Suite>;

/// Virtual-time budget for something that must resolve. Nothing here is a
/// wall-clock wait: on the paused clock an idle runtime jumps to the next
/// armed timer, so this costs nothing when the future is going to resolve
/// and costs exactly `PATIENCE` of virtual time when it is not.
const PATIENCE: Duration = Duration::from_secs(5);

/// How far either side of `INTRO_TTL` the two-sided pins are taken.
///
/// 100 ms is two orders of magnitude inside the nearest other ratified timer
/// (10 s and 25 s are the neighbours), so the pair identifies **15 s** and not
/// merely "some timer fired".
const EDGE: Duration = Duration::from_millis(100);

/// `INTRO_TTL`'s ratified value, **as a literal** — and every timing
/// assertion in this file is written against *this*, never against
/// `slither::constants::INTRO_TTL`.
///
/// Ruling 271's valve-pin lesson, and it was **measured** rather than assumed:
/// an earlier draft of this file did its arithmetic in terms of the constant,
/// and under a mutant that moved `INTRO_TTL_MS` to 10 000 the S24 test **stayed
/// green** — it advanced by the drifted constant and observed the drifted
/// timer, asserting nothing. A test written in terms of the constant drifts
/// along with it and stops being a pin, and the only way to notice is to move
/// the constant and look.
///
/// `INTRO_TTL` is asserted equal to this in
/// `s10_the_parked_mid_state_holds_the_published_memory_bound`, which is where
/// a drift is *named*; here it is where a drift is *caught*.
const TTL: Duration = Duration::from_secs(15);

fn addr(host: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, host, 1)), port)
}

/// One endpoint on a shared [`Network`], with the bookkeeping a test needs.
struct Node {
    ep: TestEndpoint,
    pk: TestPublicKey,
    addr: SocketAddr,
}

impl Node {
    /// Must be called inside a `LocalSet` (§16.3: `spawn_local`).
    fn spawn(net: &Network, seed: u8, host: u8, port: u16) -> Node {
        let at = addr(host, port);
        let id: Id = CountingIdentity::seeded([seed; 32]);
        let pk = *Identity::public_static(&id);
        let ep = TestEndpoint::builder()
            .identity(id)
            .wire(net.wire(at))
            .config(Config::new())
            .rng_seed([seed ^ 0xFF; 32])
            .build();
        Node { ep, pk, addr: at }
    }
}

/// Dial `to`, park the introduction at `to`, and **block the return path for
/// further initiations** so §6.3 rule 5 cannot refresh the entry's age key.
///
/// Returns the `Intro` the listener now holds, plus the live `Connecting` —
/// which the caller must keep, because dropping it cancels the dial and the
/// point of this fixture is a dialler that is still trying.
///
/// The block is the whole reason this is a function: leaving the path open
/// makes every expiry assertion in this file vacuous (see the module docs,
/// point 3), and a fixture is the only place to make that impossible to
/// forget.
async fn park_intro(net: &Network, from: &Node, to: &Node) -> (TestIntro, TestConnecting) {
    let connecting = from
        .ep
        .connect(to.addr, from_static(to))
        .expect("connect() on a NONE static is Ok");
    settle().await;
    let intro = tokio::time::timeout(PATIENCE, to.ep.accept())
        .await
        .expect("an initiation reached the listener")
        .expect("the accept queue is open");
    assert_eq!(
        intro.source(),
        from.addr,
        "fixture: the introduction handed over is not the one this dial parked"
    );
    net.block_path(from.addr, to.addr);
    (intro, connecting)
}

fn from_static(node: &Node) -> TestPublicKey {
    node.pk
}

/// Advance virtual time to `at` and let both drivers turn.
///
/// `sleep_until` rather than `sleep`, so the instant a test is asserting about
/// is written in the test rather than accumulated by it.
async fn advance_to(at: tokio::time::Instant) {
    tokio::time::sleep_until(at).await;
    settle().await;
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut slither::SendStream<Suite>, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = tokio::time::timeout(PATIENCE, s.write(&buf[done..]))
            .await
            .unwrap_or_else(|_| panic!("{what}: write still pending after {PATIENCE:?}"))
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: a blocked write is `Pending`, never `Ok(0)`"
        );
        done += n;
    }
}

/// Read exactly `want.len()` bytes and assert they are `want`.
async fn read_exact_eq(r: &mut slither::RecvStream<Suite>, want: &[u8], what: &str) {
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while got.len() < want.len() {
        let n = tokio::time::timeout(PATIENCE, r.read(&mut buf))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{what}: read still pending after {PATIENCE:?} with {}/{} bytes in hand",
                    got.len(),
                    want.len()
                )
            })
            .unwrap_or_else(|e| panic!("{what}: read failed with {e:?}"))
            .unwrap_or_else(|| panic!("{what}: the stream finished early"));
        assert!(
            n >= 1,
            "{what}: a blocked read is `Pending`, never `Ok(Some(0))`"
        );
        got.extend_from_slice(&buf[..n]);
    }
    assert_eq!(
        got, want,
        "{what}: the bytes did not survive the round trip"
    );
}

// ══════════════════════════════════════════════════════════════════════
// S8 — a decision parked across event-loop turns, at the shell seam
// ══════════════════════════════════════════════════════════════════════

/// **S8's loop half, and S24's 15 s row.**
///
/// Two introductions are parked in the same virtual instant and held in
/// application variables while a *third* peer establishes a connection with the
/// same listener and keeps exchanging data across fifteen seconds of virtual
/// time. That is what "parked across event-loop turns" means at the shell: the
/// listener's `accept()` loop, its driver, its timers and a live session all
/// keep running around two objects nobody has resolved.
///
/// Then the pair, on the two chains parked together:
///
/// * `park + INTRO_TTL − 100 ms` — the decision is **still takeable**;
/// * `park + INTRO_TTL + 100 ms` — [`IntroError::Expired`] reaches the holder.
///
/// # The build this separates
///
/// * **no intro expiry at all** — the second half is `Ok` and fails.
/// * **any other ratified timer** — 5 s or 10 s fails the first half, 25 s or
///   90 s the second.
/// * **a sweep that unparks on unrelated activity** — the noisy peer's fifteen
///   seconds of traffic and timers would take the first half down.
/// * **an entry refreshed by someone else's datagrams** — the second half fails,
///   which is why the noisy peer is a *different* source from the two dialled
///   ones.
#[tokio::test(start_paused = true)]
async fn s8_a_parked_intro_survives_unrelated_activity_and_expires_at_intro_ttl() {
    local(async {
        let net = Network::seeded(0x5138_0008);
        let listener = Node::spawn(&net, 0x10, 1, 4001);
        let d1 = Node::spawn(&net, 0x21, 2, 4002);
        let d2 = Node::spawn(&net, 0x32, 3, 4003);
        let noisy = Node::spawn(&net, 0x43, 4, 4004);

        // ── Two decisions parked in one instant ──────────────────────
        let (intro1, _keep1) = park_intro(&net, &d1, &listener).await;
        let (intro2, _keep2) = park_intro(&net, &d2, &listener).await;
        let parked_at = tokio::time::Instant::now();

        // ── Unrelated activity: a third peer, live, for the whole TTL ─
        let dial = noisy
            .ep
            .connect(listener.addr, noisy_target(&listener))
            .expect("connect");
        let ladder = async {
            let intro = listener
                .ep
                .accept()
                .await
                .expect("the accept queue is open");
            assert_eq!(
                intro.source(),
                noisy.addr,
                "the listener's loop handed over a parked decision instead of \
                 the new arrival — the two are not interchangeable"
            );
            intro
                .read_identity()
                .await
                .expect("read_identity")
                .authenticate()
                .await
                .expect("authenticate")
                .accept()
                .await
                .expect("accept")
        };
        let (dialled, accepted) = tokio::join!(dial, ladder);
        let dialled = dialled.expect("the noisy peer's dial completed");

        let bi = tokio::time::timeout(PATIENCE, dialled.open_bi())
            .await
            .expect("open_bi resolved")
            .expect("open_bi");
        let (mut tx, _rx) = bi.split();
        // A stream is announced by its first frame, so the opener writes before
        // the peer can accept it.
        write_all(&mut tx, b"open", "noisy open").await;
        let peer_bi = tokio::time::timeout(PATIENCE, accepted.accept_bi())
            .await
            .expect("accept_bi resolved")
            .expect("accept_bi");
        let (_ptx, mut prx) = peer_bi.split();
        read_exact_eq(&mut prx, b"open", "noisy open").await;

        // Fourteen one-second turns of genuine, unrelated work: writes, reads,
        // and every keepalive/ACK timer that falls between them.
        for round in 0u8..14 {
            write_all(&mut tx, &[round; 8], "noisy traffic").await;
            read_exact_eq(&mut prx, &[round; 8], "noisy traffic").await;
            advance_to(parked_at + Duration::from_secs(u64::from(round) + 1)).await;
        }

        // ── The pin, from the side that separates ────────────────────
        advance_to(parked_at + TTL - EDGE).await;
        let still_takeable = tokio::time::timeout(PATIENCE, intro1.read_identity())
            .await
            .expect("read_identity resolved");
        assert!(
            still_takeable.is_ok(),
            "S8: a decision parked for {:?} — 100 ms short of INTRO_TTL — while \
             an unrelated session ran the whole time must still be takeable; it \
             answered {:?}",
            TTL - EDGE,
            still_takeable.err()
        );

        advance_to(parked_at + TTL + EDGE).await;
        let expired = tokio::time::timeout(PATIENCE, intro2.read_identity())
            .await
            .expect("read_identity resolved");
        match expired {
            Err(IntroError::Expired) => {}
            Err(other) => panic!(
                "S8: the parked decision must reach its holder as \
                 IntroError::Expired at INTRO_TTL ({TTL:?}); it answered \
                 {other:?}. Ruling 261 splits Expired from Evicted, and this \
                 queue holds two entries against a cap of {INTRO_QUEUE_CAP} — \
                 an Evicted here would mean the caps fired with the queue \
                 nearly empty"
            ),
            Ok(_) => panic!(
                "S8: the parked decision outlived INTRO_TTL ({TTL:?}) and \
                 was still takeable {EDGE:?} past it. read_identity() answers \
                 Expired only when the sweep has removed the entry, so this is \
                 the sweep never running — the driver calls handle_timeout on \
                 Event::Timeout alone"
            ),
        }
    })
    .await;
}

fn noisy_target(listener: &Node) -> TestPublicKey {
    listener.pk
}

/// **S8's own words, which are about `Claimed` and not about `Intro`.**
///
/// *"a `Claimed` is an owned, app-held object with no lifetime, parked across
/// turns while a human is asked, a directory is queried, or a policy is
/// fetched. It expires at `INTRO_TTL` (15 s) if not resolved."*
///
/// The clock that matters is **the initiation's**, not the read's: §6.3 says a
/// consumed chain's mid-state *"expires 15 s after the initiation that fed it"*,
/// and `IntroEntry::deadline` gets that for free because a consumed chain is
/// never refreshed again. So this reads both chains at `t0 + 10 s` and then
/// takes the pair at 15 s **from `t0`** — with only 5 s having passed since the
/// read.
///
/// # The build this separates — and why it is invisible to the obvious test
///
/// A build that (re)stamps the age key at `read_identity()` gives this chain a
/// deadline of `t0 + 25 s`. It is therefore **still alive** at `t0 + 15.1 s`,
/// and the second half goes red. Every "hold a `Claimed` for fifteen seconds"
/// test written from the story alone waits 15 s *from the read*, lands at
/// `t0 + 25 s`, and passes against both builds. The 10 s offset is the entire
/// test.
///
/// Also separated: a sweep that skips consumed entries (the mid-state — §17.5's
/// *"live key material"*, and the thing holding the endpoint's static provider —
/// would then be immortal), and an `AuthError::Expired` wired to the wrong
/// variant. The first half excludes the mirror-image build that expires consumed
/// chains the moment they are consumed.
#[tokio::test(start_paused = true)]
async fn s8_a_claimed_expires_fifteen_seconds_after_the_initiation_that_fed_it() {
    local(async {
        let net = Network::seeded(0x5138_0018);
        let listener = Node::spawn(&net, 0x11, 1, 4001);
        let d1 = Node::spawn(&net, 0x22, 2, 4002);
        let d2 = Node::spawn(&net, 0x33, 3, 4003);

        let (intro1, _keep1) = park_intro(&net, &d1, &listener).await;
        let (intro2, _keep2) = park_intro(&net, &d2, &listener).await;
        let initiation = tokio::time::Instant::now();

        // Ten seconds of deliberation, then the human answers "tell me who it
        // claims to be" — 1 DH each, and both chains are now `Claimed`.
        advance_to(initiation + Duration::from_secs(10)).await;
        let claimed1 = tokio::time::timeout(PATIENCE, intro1.read_identity())
            .await
            .expect("read_identity resolved")
            .expect("a real msg1 is readable ten seconds into its TTL");
        let claimed2 = tokio::time::timeout(PATIENCE, intro2.read_identity())
            .await
            .expect("read_identity resolved")
            .expect("a real msg1 is readable ten seconds into its TTL");
        assert_ne!(
            claimed1.claimed_static().as_ref(),
            claimed2.claimed_static().as_ref(),
            "fixture: the two chains must be different peers, or the timestamp \
             guard — not the TTL — is what this test is measuring"
        );

        // ── The pin ──────────────────────────────────────────────────
        advance_to(initiation + TTL - EDGE).await;
        let proven = tokio::time::timeout(PATIENCE, claimed1.authenticate())
            .await
            .expect("authenticate resolved");
        assert!(
            proven.is_ok(),
            "S8: a Claimed read at t0+10 s must still be resolvable at \
             t0+{:?} — 100 ms short of the initiation's INTRO_TTL. It answered \
             {:?}",
            TTL - EDGE,
            proven.err()
        );

        advance_to(initiation + TTL + EDGE).await;
        let expired = tokio::time::timeout(PATIENCE, claimed2.authenticate())
            .await
            .expect("authenticate resolved");
        match expired {
            Err(AuthError::Expired) => {}
            Err(other) => panic!(
                "S8: a Claimed that outlives its initiation's INTRO_TTL must \
                 answer AuthError::Expired; it answered {other:?}"
            ),
            Ok(_) => panic!(
                "S8/§6.3: the mid-state expires 15 s after **the initiation \
                 that fed it**, not 15 s after read_identity(). This chain was \
                 read at t0+10 s and was still authenticable at \
                 t0+{:?} — five seconds after its initiation's deadline. A \
                 consumed chain is never refreshed, so its age key must still \
                 be t0",
                TTL + EDGE
            ),
        }
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S24 — the 15 s row, resolving in virtual time with nothing to carry it
// ══════════════════════════════════════════════════════════════════════

/// **S24's 15 s row: the timer is *armed*, not piggy-backed on traffic.**
///
/// The listener holds one parked `Intro` and **nothing else** — no connection,
/// no pending dial of its own, no timestamp-guard orphan — so
/// `core::Endpoint::deadline()`'s three-family min (§16.5: pendings, parked
/// intros, guard orphans) has exactly one term in it. The dialler's path is
/// blocked, so not one datagram crosses the fabric between the park and the
/// observation.
///
/// Two assertions, and they are only a test **together**:
///
/// * the tap shows the listener transmitted **nothing** in the interval — so
///   nothing external woke its driver, and the expiry below cannot be a side
///   effect of some other timer;
/// * the introduction expired anyway.
///
/// # The build this separates
///
/// Drop the parked intros' term from `deadline()` and the driver sleeps on
/// `Timeout(None)` for ever on a quiet endpoint: the entry becomes immortal.
/// **Measured, at this base commit: three `core::tests` unit tests catch that
/// mutant and not one of the 226 pre-existing integration tests does** — the
/// flow tests all keep a connection alive, and its keepalive timer sweeps the
/// intro queue as a side effect, so the missing arm never shows. That is ruling
/// 265's park defect — *"no timer armed at all"* — one timer family over, and
/// the core-seam tests that catch it are exactly the ones working rule 12 warns
/// about: true lemmas about a state the shell never reaches. This is the
/// construction that reaches it.
///
/// Two-sided for the reason working rule 9 gives, and the reason was **measured
/// rather than reasoned**: with only the `Expired` half, a mutant shortening
/// `INTRO_TTL` to 10 s left this test green.
#[tokio::test(start_paused = true)]
async fn s24_the_intro_expiry_fires_on_an_armed_timer_with_no_traffic_to_carry_it() {
    local(async {
        let net = Network::seeded(0x5138_0024);
        let tap = net.tap();
        let listener = Node::spawn(&net, 0x12, 1, 4001);
        let d1 = Node::spawn(&net, 0x23, 2, 4002);
        let d2 = Node::spawn(&net, 0x34, 3, 4003);

        let (early, _keep1) = park_intro(&net, &d1, &listener).await;
        let (late, _keep2) = park_intro(&net, &d2, &listener).await;
        let parked_at = tokio::time::Instant::now();
        let mark = tap.snapshot().len();

        // The **not-before** half, and it is not decoration: without it a
        // shortened `INTRO_TTL` — 10 s, say — satisfies the assertion below
        // for the wrong reason, because an entry that expired early is still
        // expired at 15.1 s. Measured, not assumed: with only the second half
        // present this test stayed green under exactly that mutant.
        advance_to(parked_at + TTL - EDGE).await;
        let still_takeable = tokio::time::timeout(PATIENCE, early.read_identity())
            .await
            .expect("read_identity resolved");
        assert!(
            still_takeable.is_ok(),
            "S24: the armed timer must be INTRO_TTL and not something shorter — \
             a chain parked {:?} ago, on an endpoint with no traffic to disturb \
             it, was already gone. It answered {:?}",
            TTL - EDGE,
            still_takeable.err()
        );

        advance_to(parked_at + TTL + EDGE).await;

        let listener_sent: Vec<_> = tap.snapshot()[mark..]
            .iter()
            .filter(|s| s.src == listener.addr)
            .map(|s| (s.dst, s.bytes.len()))
            .collect();
        assert!(
            listener_sent.is_empty(),
            "premise: the listener must be silent across the whole TTL, or its \
             driver had some other reason to wake and this test proves nothing \
             about an armed intro timer. It sent {listener_sent:?}"
        );

        let expired = tokio::time::timeout(PATIENCE, late.read_identity())
            .await
            .expect("read_identity resolved");
        assert!(
            matches!(expired, Err(IntroError::Expired)),
            "S24: INTRO_TTL is one of the five timers the story requires to \
             resolve in virtual time with no kernel. On an endpoint with no \
             connection, no pending and no traffic, the parked intro is the \
             **only** term in §16.5's deadline min — so an entry still present \
             {EDGE:?} past {TTL:?} means that term is missing and the \
             driver never armed anything. It answered {expired:?}"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S10 — a flood that does not disturb an established connection
// ══════════════════════════════════════════════════════════════════════

/// A flood source: 256 distinct IPs × 4 ports each.
///
/// The **dedup** key is the full `SocketAddr` and the **cap** key is the IP
/// (§6.3's two keys, two scopes), so four ports on one IP park as four entries
/// under one source cap — `INTRO_MAX_PER_SOURCE` exactly, with nothing evicted.
/// 256 × 4 is `INTRO_QUEUE_CAP`, written out rather than derived so the shape of
/// the flood is visible.
fn flood_source(n: usize) -> SocketAddr {
    let ip = n / 4;
    let port = 40_000 + (n % 4) as u16;
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(198, 51, (ip / 256) as u8, (ip % 256) as u8)),
        port,
    )
}

/// **S10's behavioural clause, at the shell, with two real endpoints.**
///
/// *"Under saturation, established connections keep running."* The core-level
/// version (`src/core/tests.rs`'s
/// `an_established_index_still_routes_while_the_queue_is_saturated`) asks
/// whether a `Disposition` still routes. This asks the question an application
/// asks: **does my data still move?**
///
/// The flood is one real msg1 lifted off the tap and re-injected from 1024
/// distinct sources. That is not a shortcut: mac1 keys on the **responder's**
/// static (§4.3, public data), so those bytes are genuinely mac1-valid from any
/// address on the internet, and §6.3's honesty clause says so in terms —
/// *"minting mac1-valid initiations costs an attacker only bandwidth, and no
/// spoofing capability is needed to occupy slots (distinct source ports are
/// distinct sources)"*. This is the attack as specified.
///
/// # The builds this separates
///
/// * **an eager read on the flood path** — the responder would spend 1024 DH;
///   the 0-DH assertion is §6.1's stage 0 and fails.
/// * **a cap that is not 1024** — the surfaced count fails, against a literal.
/// * **a queue that refuses at full occupancy instead of evicting** — the
///   overflow batch fails, and §6.3's evict-oldest is what keeps a genuine peer
///   reachable during a flood.
/// * **a queue with no global cap at all** — the overflow batch alone does not
///   see it (an uncapped queue admits the eight just as happily), so the eight
///   oldest holders are checked for ruling 261's `IntroError::Evicted`
///   afterwards. That is the assertion that separates *enforced* from *absent*.
/// * **a build where queue pressure stalls or kills a live session** — the data
///   assertions fail, which is S10's sentence entire.
/// * **the degenerate build that ignores the flood completely** — "the
///   connection still works" is trivially true of it, which is exactly why the
///   1024-count and the 0-DH assertions are taken *first*. Without them this
///   test asserts nothing at all.
#[tokio::test(start_paused = true)]
async fn s10_an_established_connection_keeps_moving_data_while_the_intro_queue_is_saturated() {
    local(async {
        let pair = Pair::seeded(0x5138_0010);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let b_addr = pair.b.addr();

        // A warm bidi stream, so "still moving data" is a claim about a stream
        // that was already working rather than about one opened after the fact.
        let bi_a = tokio::time::timeout(PATIENCE, ca.open_bi())
            .await
            .expect("open_bi resolved")
            .expect("open_bi");
        let (mut sa, mut ra) = bi_a.split();
        write_all(&mut sa, b"before", "warm a->b").await;
        let bi_b = tokio::time::timeout(PATIENCE, cb.accept_bi())
            .await
            .expect("accept_bi resolved")
            .expect("accept_bi");
        let (mut sb, mut rb) = bi_b.split();
        read_exact_eq(&mut rb, b"before", "warm a->b").await;
        settle().await;

        // ── The flood's ammunition: one genuine, mac1-valid msg1 ──────
        let msg1 = tap
            .snapshot()
            .iter()
            .find(|s| {
                s.dst == b_addr
                    && s.bytes.len() == INIT_PACKET_LEN
                    && s.bytes[0] == PKT_HANDSHAKE_INIT
            })
            .map(|s| s.bytes.clone())
            .expect("fixture: the tap saw no HandshakeInit to replay");

        let dhs_before = pair.b.dhs.get();

        // ── Saturate: exactly INTRO_QUEUE_CAP arrivals ───────────────
        for n in 0..1024usize {
            pair.net.inject(flood_source(n), b_addr, &msg1);
        }

        let mut held: Vec<TestIntro> = Vec::with_capacity(1024);
        for n in 0..1024usize {
            let intro = tokio::time::timeout(PATIENCE, pair.b.endpoint.accept())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "§6.3: {n} of 1024 mac1-valid initiations from distinct \
                         sources surfaced before the queue stopped admitting. \
                         INTRO_QUEUE_CAP is 1024 and the flood is 256 IPs × 4 \
                         ports = the per-source cap exactly, so nothing here \
                         should have been refused or evicted"
                    )
                })
                .expect("the accept queue is open");
            held.push(intro);
        }
        assert_eq!(held.len(), 1024, "the queue admitted the full cap");
        assert_eq!(
            INTRO_QUEUE_CAP, 1024,
            "§6.3's ratified cap moved. The count above is written as a literal \
             on purpose (ruling 271's valve-pin lesson) — a test written in \
             terms of the constant drifts with it and stops pinning anything. \
             If this is a deliberate change it needs a ruling, not an updated \
             expectation"
        );

        // Nothing beyond the cap is admitted while every slot is held.
        assert!(
            tokio::time::timeout(Duration::from_millis(1), pair.b.endpoint.accept())
                .await
                .is_err(),
            "§6.3: a 1025th introduction surfaced with all 1024 slots held and \
             no further arrivals injected"
        );

        assert_eq!(
            pair.b.dhs.get(),
            dhs_before,
            "§6.1: stage 0 is **0 DH**. A flood of 1024 initiations spent DH on \
             the responder, which is the flood turning into a CPU attack"
        );

        // ── S10's sentence: the session is untouched ─────────────────
        write_all(&mut sa, b"during-a", "a->b under saturation").await;
        read_exact_eq(&mut rb, b"during-a", "a->b under saturation").await;
        write_all(&mut sb, b"during-b", "b->a under saturation").await;
        read_exact_eq(&mut ra, b"during-b", "b->a under saturation").await;
        assert!(
            ca.is_established() && cb.is_established(),
            "S10: the established connection did not survive a saturated intro \
             queue — §17.5: established connections \"hold no queue slot\""
        );

        // ── At full occupancy, §6.3 evicts rather than refuses ───────
        for n in 0..8usize {
            let fresh = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, n as u8)), 4444);
            pair.net.inject(fresh, b_addr, &msg1);
        }
        for n in 0..8usize {
            let intro = tokio::time::timeout(PATIENCE, pair.b.endpoint.accept())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "§6.3 rule 3: at full occupancy an arrival evicts the \
                         oldest unconsumed entry — it is not refused. Arrival \
                         {n} of 8 from a fresh source never surfaced, which \
                         means a full queue locks a genuine peer out for the \
                         whole {TTL:?} TTL"
                    )
                })
                .expect("the accept queue is open");
            held.push(intro);
        }

        // The batch above must have **displaced** eight entries, not merely
        // been admitted alongside them. Without this the overflow assertion
        // does not separate "the global cap is enforced" from "there is no
        // global cap": both admit the eight. All 1024 were parked in one
        // virtual instant, so ruling 69's age key ties and `IntroId` breaks
        // them ascending — the eight oldest are the eight surfaced first.
        //
        // `Evicted` and not `Expired` is ruling 261 (2026/08/18), which split
        // the two precisely so an application can tell *"you are at your
        // intro-queue cap"* from a 15 s timeout: nothing here has aged at all,
        // and the whole flood happens in one instant of virtual time.
        for (n, displaced) in held.drain(0..8).enumerate() {
            let answer = tokio::time::timeout(PATIENCE, displaced.read_identity())
                .await
                .expect("read_identity resolved");
            match answer {
                Err(IntroError::Evicted) => {}
                Err(IntroError::Expired) => panic!(
                    "ruling 261: entry {n} was displaced under §6.3's global cap \
                     and reported Expired — a 15 s timeout for something that \
                     happened in one instant under queue pressure, which is the \
                     defect that ruling split the variant to remove"
                ),
                Err(other) => panic!("displaced entry {n} answered {other:?}"),
                Ok(_) => panic!(
                    "§6.3 rule 3: eight arrivals at full occupancy must evict \
                     eight entries. Entry {n} — one of the eight oldest, and so \
                     one of the eight victims — is still readable, so the queue \
                     admitted 1032 against a cap of 1024 and the global cap is \
                     not enforced at all"
                ),
            }
        }

        write_all(&mut sa, b"after", "a->b after the overflow").await;
        read_exact_eq(&mut rb, b"after", "a->b after the overflow").await;
        assert!(
            ca.is_established() && cb.is_established(),
            "S10: the connection died to intro-queue eviction pressure"
        );
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// S10 — the parked-entry memory bound
// ══════════════════════════════════════════════════════════════════════

/// hiss's suspended mid-state for an identity — the quantity S10 publishes.
///
/// `core::endpoint::staged::MidState<I>` is `pub(crate)`, but every piece it is
/// spelled from is public: `Identity`'s `Suite`/`Provider` and
/// `packet::Handshake`'s `Msg1Intro<P>`. So this alias is the *same type* the
/// queue parks behind its `Box`, named from outside the crate.
type MidState<I> = <<I as Identity>::Suite as Handshake>::Msg1Intro<<I as Identity>::Provider>;

/// **S10's dimensional clause.**
///
/// *"the cost to hold a parked entry is bounded (measured mid-state 784 B on
/// P-256, so 1024 parked ≈ 0.77 MiB)"*. `SPEC.md` §A.1 is where the 784 comes
/// from: *"Mid-state size, measured (reference suite, `EphemeralOnly<StdRng>`):
/// 784 B on P-256 … of which ~320 B is the provider itself."*
///
/// # Both sides, and the lower one is the one doing the work
///
/// The upper bound is the story's figure. The **lower** bound is working rule 9:
/// an upper bound on a size is satisfied for free by any refactor that turns
/// `Msg1Intro` into a handle into a side table — the assertion would stay green
/// while the memory moved somewhere the queue cap does not bound. §17.5 prices a
/// live mid-state at *"≈ 0.5–1 KB live key material each"*, so 512 B is the
/// spec's own floor and the right side to assert from.
///
/// # Why `size_of` and not a measured heap
///
/// The mid-state is what `ChainState::Claimed` boxes, so its `size_of` **is** the
/// allocation. `IntroEntry<I>` itself is `pub(crate)` and cannot be named from an
/// integration test — see this slice's report for its measured size and for the
/// §6.3/§17.5 discrepancy that measurement turned up.
#[test]
fn s10_the_parked_mid_state_holds_the_published_memory_bound() {
    /// The story's published mid-state figure, as a literal.
    const PUBLISHED: usize = 784;
    /// §17.5's own floor for a live mid-state — 0.5 KB.
    const FLOOR: usize = 512;

    let shipping = size_of::<MidState<slither::SoftwareIdentity<Suite>>>();
    let counting = size_of::<MidState<Id>>();

    for (what, measured) in [
        ("SoftwareIdentity", shipping),
        ("CountingIdentity", counting),
    ] {
        assert!(
            measured <= PUBLISHED,
            "S10: the parked mid-state for {what} measures {measured} B against \
             the {PUBLISHED} B STORIES.md publishes (SPEC.md §A.1). 1024 parked \
             is then {} B, over the ~0.77 MiB the story commits to. A figure in \
             an approved story is a claim about this build — either the growth \
             is a defect or the story needs a ruling",
            measured * 1024
        );
        assert!(
            measured >= FLOOR,
            "S10: the mid-state for {what} measures {measured} B, under §17.5's \
             own \"≈ 0.5–1 KB live key material each\". This type is supposed to \
             hold the endpoint's static provider (~320 B of the published 784) \
             and the es-derived keys — a value this small means the bound above \
             is being taken on the wrong type and is measuring nothing"
        );
    }

    assert!(
        1024 * shipping <= 1024 * PUBLISHED,
        "S10: 1024 parked mid-states is {} B, over the story's {} B",
        1024 * shipping,
        1024 * PUBLISHED
    );

    // The two ratified constants the bound is quoted against, as literals.
    assert_eq!(
        INTRO_QUEUE_CAP, 1024,
        "§6.3's queue cap is what turns a per-entry size into the story's \
         0.77 MiB. If it moved, the published aggregate moved with it"
    );
    assert_eq!(
        INTRO_TTL, TTL,
        "§6.3's 15 s TTL is S8's expiry and S24's 15 s row, and §6.3's honesty \
         clause prices the flood at 1024/15 s ≈ 68 packets/second off exactly \
         these two numbers. Every INTRO_TTL arithmetic in this file is only a \
         pin because this assertion exists"
    );
}

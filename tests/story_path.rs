//! **Path validation — S18's amplification half, under ruling 208.**
//!
//! | Story | What this file covers |
//! |---|---|
//! | **S18** | a roamed address is validated by an unforgeable challenge, and a transfer across the move completes |
//! | **S18** | §7.3's 3× ceiling still binds an address that never answers |
//!
//! # Authorship (CLAUDE.md working rule 6)
//!
//! Written by slice 7b's blind test author, from `STORIES.md` S18,
//! `.slices/07b-remediation/CONTRACT-7b.md` and `SPEC.md` §7.3, in a
//! worktree cut at `c131904` with no slice-7b implementation in it. The two
//! implementers worked concurrently and were never read.
//!
//! # What this file can and cannot see — working rule 13, stated up front
//!
//! **The tap sees ciphertext.** `Spied` carries `src`, `dst` and the
//! datagram verbatim, and every Data packet's frames are under AEAD. So no
//! test here can assert *"a `PATH_CHALLENGE` was on the wire"* — that
//! property lives in `src/core/connection/tests_path.rs`, where a raw peer
//! half can open the packet.
//!
//! What this file *can* see is **bytes, addresses and time**, which is
//! exactly what §7.3's security property is stated in: *"total bytes sent
//! MUST NOT exceed `AMPLIFICATION_FACTOR` (= 3) × total bytes received from
//! it"*. That is asserted here and nowhere else.
//!
//! **One thing neither file can reach, and it is ruling 208's own attack.**
//! The forged ACK requires a party that *holds the session key* to seal a
//! fresh ACK and have it arrive from a spoofed source. `Network::inject`
//! can spoof the source but cannot seal, and a captured datagram re-injected
//! from a new source is a **replay**, which §7.2's window refuses before
//! §7.3 ever sees it — so the injection pins the replay window, not the
//! amplification budget. The core-level twin
//! (`tests_path::an_ack_covering_everything_validates_nothing`) is where
//! that property is pinned, by handing the core an ACK directly. Working
//! rule 13: the fixture bounds the coverage, and this is the boundary.
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Every test is `#[tokio::test(start_paused = true)]` inside `local`.
//! `tokio::time::timeout` is the instrument: on the paused clock it
//! auto-advances to the next armed timer whenever every task is idle, so a
//! keepalive, a PTO and a `DEAD_TIMEOUT` all resolve inside one `within`.

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use slither::constants::AMPLIFICATION_FACTOR;
use slither::testutil::{
    Pair, Spied, TestConnection, TestRecvStream, TestSendStream, addr_c, local, settle,
};
use slither::{ConnectionLost, Notification, StreamId};

// ══════════════════════════════════════════════════════════════════════
// harness
//
// Deliberately a copy of `story_mobility.rs`'s, not a shared module: these
// two files are written by different authors in different slices and
// `tests/` has no common crate. Divergence between the two copies is
// visible in review; a shared helper that one slice edits for its own
// reasons is not.
// ══════════════════════════════════════════════════════════════════════

/// Virtual-time budget for "this resolves".
const PATIENCE: Duration = Duration::from_secs(5);

/// [`PATIENCE`] with room for retransmission and one validation round
/// trip. Kept well **under** `DEAD_TIMEOUT` (25 s) so a test that fails by
/// stalling fails as a stall and not as a connection death — the two have
/// different causes and must not be confusable.
const PATIENCE_PTO: Duration = Duration::from_secs(15);

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

/// `true` if `fut` had **not** resolved within a short window.
async fn is_pending<F: Future>(fut: F) -> bool {
    tokio::time::timeout(Duration::from_millis(200), fut)
        .await
        .is_err()
}

/// A payload whose every byte is a function of its offset.
fn payload(tag: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i % 251) as u8) ^ tag).collect()
}

async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
    let mut done = 0usize;
    while done < buf.len() {
        let n = within_pto(s.write(&buf[done..]), what)
            .await
            .unwrap_or_else(|e| panic!("{what}: write failed with {e:?}"));
        assert!(
            n >= 1,
            "{what}: a blocked write is `Pending`, never `Ok(0)`"
        );
        done += n;
    }
}

/// Read exactly `want.len()` bytes and assert they are `want`.
///
/// **The stall is the assertion.** Streams are ordered and gapless, so a
/// build in which the address never validates cannot deliver the payload
/// and this panics with the stream's name rather than hanging CI.
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
    o_send: TestSendStream,
    p_recv: TestRecvStream,
    p_send: TestSendStream,
    o_recv: TestRecvStream,
    #[allow(dead_code)]
    id: StreamId,
}

async fn bi_pair(opener: &TestConnection, peer: &TestConnection, what: &str) -> Bi {
    let bi = within(opener.open_bi(), what).await.expect("open_bi");
    let id = bi.id().expect("an opened stream has an id");
    let (mut o_send, o_recv) = bi.split();
    write_all(&mut o_send, b"\x00", what).await;

    let peer_bi = within(peer.accept_bi(), what).await.expect("accept_bi");
    let (p_send, mut p_recv) = peer_bi.split();
    read_expect(&mut p_recv, b"\x00", what).await;

    Bi {
        o_send,
        p_recv,
        p_send,
        o_recv,
        id,
    }
}

/// **Bytes**, not datagrams, that left `from` addressed to `to`.
///
/// §7.3's cap is stated in bytes and a datagram count cannot express it: a
/// build sending three full-size packets against three 30-byte keepalives
/// satisfies "three for three" and violates the rule by 40×.
fn bytes_from_to(spied: &[Spied], from: SocketAddr, to: SocketAddr) -> u64 {
    spied
        .iter()
        .filter(|s| s.src == from && s.dst == to)
        .map(|s| s.bytes.len() as u64)
        .sum()
}

// ══════════════════════════════════════════════════════════════════════
// S18 — the roam completes, and the transfer across it completes
// ══════════════════════════════════════════════════════════════════════

/// A large transfer across a roam completes **promptly**.
///
/// This is S18's *"streams continue with no re-handshake and no data
/// loss"*, measured against the clock rather than against eventual
/// delivery — and under ruling 208 the clock is the whole point.
///
/// # BROKEN BUILD this separates
///
/// **The stall.** Ruling 203 found that a sender which builds a full-size
/// packet, has it refused by the budget, and holds it whole makes §7.3's
/// escape unreachable. Ruling 208 replaces the escape's *predicate* — an
/// ACK no longer validates anything, only a matching `PATH_RESPONSE` does —
/// so a build that shrinks correctly but never puts a `PATH_CHALLENGE` in
/// the shrunken packet reproduces the stall exactly, one indirection later.
/// §7.3:2249-2256 says this design *"does not repair and does not weaken"*
/// that defect.
///
/// Such a build still makes progress: the peer's ACKs are authenticated and
/// window-fresh, so they fund the budget at ~40 bytes a round trip and 3×
/// that leaves. **It therefore passes any test that merely waits for the
/// bytes.** What separates it is the deadline: `PATIENCE_PTO` is 15 s of
/// virtual time, and 64 KiB at ~120 bytes per round trip is not close.
///
/// The payload is 64 KiB rather than a few hundred bytes for exactly that
/// reason — a small transfer fits inside the throttled build's budget and
/// the test would assert nothing.
#[tokio::test(start_paused = true)]
async fn s18_a_large_transfer_across_a_roam_completes_promptly() {
    local(async {
        let pair = Pair::seeded(0x7b_0001);
        let (ca, cb) = pair.establish().await;

        let a_addr = pair.a.addr();
        let b_old = pair.b.addr();
        assert_eq!(ca.remote_address(), b_old, "the anchor starts at b");

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb, mut sb, mut ra) = (bi.o_send, bi.p_recv, bi.p_send, bi.o_recv);

        // Traffic before the move, so the connection is in the state S18
        // describes rather than a freshly-established one.
        let warm = payload(0x01, 2048);
        write_all(&mut sa, &warm, "pre-move a→b").await;
        read_expect(&mut rb, &warm, "pre-move b reads").await;
        settle().await;

        // ── the peer's network changes ───────────────────────────────
        pair.b.rebind(addr_c());

        // The mover sends: roaming is receive-driven (§7.3).
        let nudge = payload(0x02, 512);
        write_all(&mut sb, &nudge, "post-move b→a").await;
        read_expect(&mut ra, &nudge, "post-move a reads").await;
        settle().await;

        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "S18: the anchor followed the move",
        );

        // ── the transfer that the budget must stop throttling ────────
        //
        // 64 KiB is far beyond the ~588-byte budget any single arming
        // creates, so it can only complete if the address **validated**.
        let big = payload(0x03, 64 * 1024);
        write_all(&mut sa, &big, "post-move a→b (64 KiB)").await;
        read_expect(&mut rb, &big, "post-move b reads 64 KiB").await;

        // And the reverse direction, which arms nothing and must be
        // unaffected — a build that validated by accident in one direction
        // only shows up here.
        let back = payload(0x04, 16 * 1024);
        write_all(&mut sb, &back, "post-move b→a (16 KiB)").await;
        read_expect(&mut ra, &back, "post-move a reads 16 KiB").await;

        settle().await;
        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "still anchored where the peer actually is",
        );
        let _ = a_addr;
    })
    .await;
}

/// §7.3's 3× ceiling binds an address that never answers.
///
/// *"total bytes sent to the address MUST NOT exceed `AMPLIFICATION_FACTOR`
/// (= 3) × total bytes **received from it**, authenticated, and
/// window-fresh."* This is the reflector property the whole section exists
/// for, and it is the one assertion in this slice that is stated in units
/// the tap can actually measure.
///
/// The peer roams to `addr_c` and then the return path is cut, so nothing
/// ever echoes the challenge and the address stays unvalidated for the rest
/// of the test. Everything we then aim at `addr_c` is capped.
///
/// # BROKEN BUILDS this separates
///
/// * **No budget at all**, or one whose arming was lost in the ruling-168
///   removal: 64 KiB of application data leaves for an address that has
///   sent us a few hundred bytes. Caught by the ratio.
/// * **A build that holds everything and never escapes**: sends nothing at
///   all, which satisfies any `<=` bound for free — working rule 9's
///   degenerate case exactly. Caught by the `> 0` assertion, which is why
///   the bound is two-sided.
///
/// # What it does *not* separate, stated rather than implied
///
/// It does not distinguish ruling 208's predicate from ruling 168's,
/// because with the return path cut **neither** an ACK nor a
/// `PATH_RESPONSE` can arrive. A build still validating on ACKs passes this
/// test. That build is caught in `tests_path.rs`, and the reason it cannot
/// be caught here is in this file's header.
#[tokio::test(start_paused = true)]
async fn s18_an_address_that_never_answers_stays_under_the_three_times_ceiling() {
    local(async {
        let pair = Pair::seeded(0x7b_0002);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;

        let a_addr = pair.a.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb, mut sb, mut ra) = (bi.o_send, bi.p_recv, bi.p_send, bi.o_recv);

        let warm = payload(0x11, 2048);
        write_all(&mut sa, &warm, "pre-move a→b").await;
        read_expect(&mut rb, &warm, "pre-move b reads").await;
        settle().await;

        // The measurement window opens at the rebind, so it contains the
        // roaming packet itself — which is the credit the budget arms with.
        let _ = tap.drain();

        pair.b.rebind(addr_c());
        let nudge = payload(0x12, 256);
        write_all(&mut sb, &nudge, "post-move b→a").await;
        read_expect(&mut ra, &nudge, "post-move a reads").await;
        settle().await;
        assert_eq!(ca.remote_address(), addr_c(), "the anchor moved");

        // ── the return path is cut ───────────────────────────────────
        //
        // Nothing from `addr_c` reaches us again, so no challenge can ever
        // be echoed and the address stays unvalidated. A blackholed send is
        // not recorded by the tap, so `bytes_from_to(.., addr_c, a_addr)`
        // stops growing here — which is precisely the credit the cap is
        // computed against.
        pair.net.block_path(addr_c(), a_addr);

        // Far more than any budget could admit. The write is expected to
        // block partway; that is the *held, not dropped* discipline working,
        // so it is driven with a bounded poll rather than `write_all`.
        let big = payload(0x13, 64 * 1024);
        let mut offered = 0usize;
        while offered < big.len() {
            match tokio::time::timeout(Duration::from_secs(2), sa.write(&big[offered..])).await {
                Ok(Ok(n)) => offered += n,
                Ok(Err(e)) => panic!("write failed with {e:?}"),
                // Blocked by the budget — which is the point.
                Err(_) => break,
            }
        }
        settle().await;

        let w = tap.drain();
        let sent = bytes_from_to(&w, a_addr, addr_c());
        let recv = bytes_from_to(&w, addr_c(), a_addr);

        assert!(
            sent > 0,
            "**working rule 9's other side**: a build that holds everything \
             and never escapes satisfies the ceiling for free. §7.3 requires \
             the budget to admit *something* — the roaming packet funds 3× \
             its own size and the challenge must fit inside that.",
        );
        assert!(
            sent <= AMPLIFICATION_FACTOR * recv,
            "§7.3: {sent} bytes sent to an unvalidated address against \
             {recv} received from it — the ceiling is {}× and this is {:.2}×. \
             This is the reflector the section exists to prevent.",
            AMPLIFICATION_FACTOR,
            sent as f64 / recv.max(1) as f64,
        );
    })
    .await;
}

/// A peer that moves to an address which never answers dies at
/// `DEAD_TIMEOUT`, with the **existing** `ConnectionLost::TimedOut`.
///
/// `CONTRACT-7b.md` §1.5: *"Nothing new. No new timer, no new event, no new
/// error variant... An implementer who invents a `PathValidationFailed`
/// variant, a validation timer, or a challenge retry counter has exceeded
/// this contract."* §7.3 says the same from the protocol side: *"If nothing
/// at the new address ever answers, nothing is validated and the session
/// dies by liveness inside 25 s — unconditionally, since any ack-eliciting
/// output we aim at the address arms the death clock by itself."*
///
/// # BROKEN BUILDS this separates
///
/// * One that mints a new `ConnectionLost` variant for an unanswered
///   challenge. Caught by the equality, which a `matches!` on "some death"
///   would not be.
/// * One that suppresses the death clock along with the held keepalive —
///   `CONTRACT-7b.md` §4.1 warns that this turns a spinning connection into
///   an **immortal** one, *"which is worse and is precisely the collapse
///   ruling 182's beacon proof warns about"*. Caught because the connection
///   must actually die inside the window.
#[tokio::test(start_paused = true)]
async fn s18_an_address_that_never_answers_dies_as_an_ordinary_timeout() {
    local(async {
        let pair = Pair::seeded(0x7b_0003);
        let (ca, cb) = pair.establish().await;

        let a_addr = pair.a.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb, mut sb, mut ra) = (bi.o_send, bi.p_recv, bi.p_send, bi.o_recv);

        let warm = payload(0x21, 1024);
        write_all(&mut sa, &warm, "pre-move a→b").await;
        read_expect(&mut rb, &warm, "pre-move b reads").await;
        settle().await;

        pair.b.rebind(addr_c());
        let nudge = payload(0x22, 256);
        write_all(&mut sb, &nudge, "post-move b→a").await;
        read_expect(&mut ra, &nudge, "post-move a reads").await;
        settle().await;
        assert_eq!(ca.remote_address(), addr_c(), "the anchor moved");

        // Nothing from the new address ever again: no echo, no liveness.
        pair.net.block_path(addr_c(), a_addr);

        // `DEAD_TIMEOUT` is 25 s of virtual time; the timeout auto-advances
        // to each armed timer in turn, so this resolves without a sleep.
        let lost = match tokio::time::timeout(Duration::from_secs(60), ca.closed()).await {
            Ok(v) => v,
            Err(_) => panic!(
                "the connection outlived DEAD_TIMEOUT at an address that \
                 never answered — a suppressed death clock is an immortal \
                 connection, which is worse than a spinning one",
            ),
        };

        assert_eq!(
            lost,
            ConnectionLost::TimedOut,
            "§7.5's existing verdict, and **no new variant**: ruling 208 \
             adds two frame types and nothing else",
        );
    })
    .await;
}

/// Path validation is not an application-visible event.
///
/// §7.3's observability is *"the `remote_address()` accessor plus the
/// `slither::roam` trace target"*, and the only event a move emits is
/// `AddressMoved`. `CONTRACT-7b.md` §1.5 adds no event, and working rule 8
/// reads ruling 208's list of what it introduces as closed.
///
/// # BROKEN BUILD this separates
///
/// One that surfaces validation to the application — a
/// `Notification::PathValidated`, or a second `AddressMoved` fired when the
/// challenge is answered. The second is the plausible one: the temptation
/// is to treat validation as "the move is now confirmed" and re-announce
/// it, which would break every consumer counting moves.
///
/// The `is_pending` half is what makes this a pin rather than a name: a
/// build that fires a second notification is caught only by asserting that
/// nothing further arrives, and only *after* the transfer that would have
/// triggered it has completed.
#[tokio::test(start_paused = true)]
async fn a_validated_roam_produces_exactly_one_notification() {
    local(async {
        let pair = Pair::seeded(0x7b_0004);
        let (ca, cb) = pair.establish().await;

        let b_old = pair.b.addr();

        let bi = bi_pair(&ca, &cb, "a opens, b accepts").await;
        let (mut sa, mut rb, mut sb, mut ra) = (bi.o_send, bi.p_recv, bi.p_send, bi.o_recv);

        let warm = payload(0x31, 1024);
        write_all(&mut sa, &warm, "pre-move a→b").await;
        read_expect(&mut rb, &warm, "pre-move b reads").await;
        settle().await;

        assert!(
            is_pending(ca.notified()).await,
            "precondition: nothing has been notified yet",
        );

        pair.b.rebind(addr_c());
        let nudge = payload(0x32, 256);
        write_all(&mut sb, &nudge, "post-move b→a").await;
        read_expect(&mut ra, &nudge, "post-move a reads").await;
        settle().await;

        assert_eq!(
            within(ca.notified(), "a.notified after the roam")
                .await
                .expect("the connection is alive"),
            Notification::AddressMoved {
                from: b_old,
                to: addr_c(),
            },
            "the move itself is the notification",
        );

        // Drive a transfer large enough that the address must have
        // validated for it to complete — so the instant a build would fire
        // a "validated" notification has certainly passed.
        let big = payload(0x33, 32 * 1024);
        write_all(&mut sa, &big, "post-move a→b (32 KiB)").await;
        read_expect(&mut rb, &big, "post-move b reads 32 KiB").await;
        settle().await;

        assert!(
            is_pending(ca.notified()).await,
            "validation is **not** an event: after the address validated and \
             32 KiB crossed the new path, no second notification exists. A \
             build re-announcing `AddressMoved` on validation lands here.",
        );
    })
    .await;
}

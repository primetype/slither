//! **Ruling 265, end to end — a peer that vanishes behind a starved budget.**
//!
//! The core half is `src/core/connection/tests_park.rs`: a connection that
//! owes §7.5's passive keepalive, cannot send it, and has **no timer armed
//! at all**, because §7.4 clears `armed` on the very receive that sets the
//! debt and §7.3's budget then vetoes the one send that would re-arm it.
//! This is the same construction driven through the shell, and it asks the
//! only question the shell can answer: **does the connection die?**
//!
//! §7.4 is unambiguous that it must — *"A sender writing into a black hole
//! therefore dies 25 s after its last authenticated receive no matter how
//! often, or how quietly, it writes"*. Before ruling 265 this connection
//! lived past 60 s of virtual time having sent nothing at all.
//!
//! # How each core lever transposes to `FlakyWire`
//!
//! | core | shell |
//! |---|---|
//! | burn a counter, deliver | [`FlakyPolicy::lossy`]`(0.5)` on B's wire across many small B writes — about half the counters never arrive, so A's window keeps the gaps §12.2 needs one range pair each for |
//! | freeze the window | `lossy(1.0)`. **Not** `block_path`: a blocked path returns before the tap records anything (`Wire::send_to` step 3), while loss is applied after it (step 5), and the roam below needs a *tapped but undelivered* datagram |
//! | flight ≥ cwnd | A writes bulk with every ACK from B lost. No byte-exact top-up is needed here — A's own PTO probes are **cwnd-exempt** (§14.5) and keep adding to `bytes_in_flight`, so §14.5's gate shuts on its own |
//! | `Solo::deliver_from` | [`Network::inject`] from [`addr_c`] with a **fresh** b→a datagram. §7.2 refuses to roam on a replay, so a datagram the tap saw *delivered* would pin the rejection, not the roam |
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! `tokio::time::timeout` is the instrument in both directions: its `Err` is
//! *"still pending at `now + d`"* and its `Ok` is *"it resolved by then"*.
//! On the paused clock an idle runtime jumps straight to the next armed
//! timer, so a 60 s observation costs nothing and still lets every keepalive
//! and PTO in between fire. There is no `sleep`.
//!
//! # Working rule 9 — the builds this separates
//!
//! * **before ruling 265** — `still_alive` at 60 s with nothing sent. Fails
//!   the death assertion.
//! * **a build that never roamed** — `remote_address()` is still B's, the
//!   budget never re-arms, and the connection dies for the ordinary reason.
//!   Fails the roam premise, which is asserted before anything else.
//! * **a build that reaps the connection early** — dying is not enough: the
//!   death must land no earlier than `DEAD_TIMEOUT` after the roam, which is
//!   the last authenticated receive. Fails the not-before half.

#![allow(clippy::items_after_statements)]

use std::net::SocketAddr;
use std::time::Duration;

use slither::constants::DEAD_TIMEOUT;
use slither::testutil::{
    FlakyPolicy, Pair, Spied, TestConnection, TestSendStream, addr_c, local, settle,
};

/// Virtual-time budget for something that must resolve.
const PATIENCE: Duration = Duration::from_secs(5);

/// Small B→A messages sent under 50 % loss, to fragment A's replay window.
/// §12.2's ACK needs one range pair per gap, and the room the roam funds
/// holds a few dozen.
const FRAGMENTS: u32 = 160;

/// How long A's PTO train is allowed to run, pushing `bytes_in_flight` past
/// the congestion window. Well inside `DEAD_TIMEOUT`, so nothing here is
/// observing a liveness death by accident.
const PTO_TRAIN: Duration = Duration::from_secs(8);

/// The last datagram the tap saw on a given path.
fn last_datagram(snapshot: &[Spied], from: SocketAddr, to: SocketAddr) -> Vec<u8> {
    snapshot
        .iter()
        .rev()
        .find(|s| s.src == from && s.dst == to)
        .map(|s| s.bytes.clone())
        .unwrap_or_default()
}

/// Write the whole buffer, looping over partial writes as §16.2 requires.
async fn write_all(s: &mut TestSendStream, buf: &[u8], what: &str) {
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

/// `Some(when it died)` if the connection died within `d`, else `None`.
async fn death_within(c: &TestConnection, d: Duration) -> Option<Duration> {
    let at = tokio::time::Instant::now();
    tokio::time::timeout(d, c.closed())
        .await
        .ok()
        .map(|_| at.elapsed())
}

#[tokio::test(start_paused = true)]
async fn s5_a_peer_that_vanishes_behind_a_starved_budget_is_still_reaped() {
    local(async {
        let pair = Pair::seeded(0x41_0265);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let a_addr = pair.a.addr();
        let b_addr = pair.b.addr();

        // A warm bidi stream, so both directions carry real traffic.
        let bi_a = tokio::time::timeout(PATIENCE, ca.open_bi())
            .await
            .expect("open_bi resolved")
            .expect("open_bi");
        let (mut sa, mut _ra) = bi_a.split();
        write_all(&mut sa, b"hello", "warm a->b").await;
        let bi_b = tokio::time::timeout(PATIENCE, cb.accept_bi())
            .await
            .expect("accept_bi resolved")
            .expect("accept_bi");
        let (mut sb, mut rb) = bi_b.split();
        let mut buf = [0u8; 16];
        let _ = tokio::time::timeout(PATIENCE, rb.read(&mut buf))
            .await
            .expect("read resolved")
            .expect("read");
        settle().await;

        // ── Fragment A's replay window ───────────────────────────────
        pair.b.wire.set_policy(FlakyPolicy::lossy(0.5));
        for i in 0..FRAGMENTS {
            write_all(&mut sb, &[i as u8; 4], "fragmenting b->a").await;
            settle().await;
        }
        settle().await;

        // ── Freeze it, and fill A's flight past the window ───────────
        // Nothing from B reaches A now, so A's data is never acknowledged.
        pair.b.wire.set_policy(FlakyPolicy::lossy(1.0));
        settle().await;
        let bulk = vec![0xABu8; 200 * 1024];
        let mut written = 0usize;
        while written < bulk.len() {
            // A short budget, because the point is to stop when the window
            // refuses rather than to deliver the whole buffer.
            match tokio::time::timeout(Duration::from_millis(50), sa.write(&bulk[written..])).await
            {
                Ok(Ok(n)) => written += n,
                _ => break,
            }
        }
        assert!(written > 0, "fixture: A must have put something in flight");
        let _ = tokio::time::timeout(PTO_TRAIN, std::future::pending::<()>()).await;

        // ── The roam, onto an address nothing answers from ───────────
        // A fresh b→a datagram: the wire tapped it and then dropped it, so
        // §7.2 marks it window-fresh and §7.3 re-homes on it. A replay would
        // pin the rejection instead.
        let before = tap.snapshot().len();
        write_all(&mut sb, b"roam", "the roam trigger").await;
        settle().await;
        let fresh = last_datagram(&tap.snapshot()[before..], b_addr, a_addr);
        assert!(
            !fresh.is_empty(),
            "fixture: the tap saw no fresh b->a datagram to roam with"
        );
        pair.net.inject(addr_c(), a_addr, &fresh);
        settle().await;

        assert_eq!(
            ca.remote_address(),
            addr_c(),
            "premise: §7.3 must have re-homed onto the dead address, or the \
             budget never re-arms and this test asserts nothing"
        );

        // ── The measurement ─────────────────────────────────────────
        let mark = tap.snapshot().len();
        let died = death_within(&ca, DEAD_TIMEOUT + Duration::from_secs(35)).await;
        let sent: Vec<_> = tap.snapshot()[mark..]
            .iter()
            .filter(|s| s.src == a_addr)
            .map(|s| (s.dst, s.bytes.len()))
            .collect();

        let elapsed = died.unwrap_or_else(|| {
            panic!(
                "§7.4: a connection whose peer has vanished must be reaped at \
                 DEAD_TIMEOUT ({DEAD_TIMEOUT:?}). It was still alive well past \
                 that, having sent {sent:?} in the meantime — §7.5's keepalive \
                 is owed, §7.3 vetoes it, and §7.4's clock was never armed, so \
                 the shell sleeps on Timeout(None) for ever."
            )
        });
        assert!(
            elapsed >= DEAD_TIMEOUT,
            "§7.4: the clock is anchored to the last authenticated receive — \
             the roam — so the death may not land before {DEAD_TIMEOUT:?}; it \
             landed after {elapsed:?}"
        );
    })
    .await;
}

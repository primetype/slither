//! **Split-timer loopback latency probe — the named next measurement for
//! round 41 audit item 3.1.**
//!
//! `.spec-v2-clean-slate/round41-H-audit-triage.md` §3.1 triaged an anomaly
//! from the pre-ratification audit: over real loopback UDP, a run of 1000
//! sequential unpipelined datagram round trips reported
//!
//! ```text
//! PHASE_C_OK n=1000 p50_us=131 p99_us=22105 min_us=41 max_us=86094   (release)
//! ```
//!
//! — a p50 of 131 µs with a p99 of **22 ms**, two orders of magnitude apart.
//! The paused-clock twin (`audit/C-liveness.md`'s E8) found handle→wire and
//! wire→parked-recv both **0 virtual ns** over 760 round trips, which
//! eliminates a protocol-internal cause: no core or driver timer inserts the
//! delay. It cannot speak to real syscall or scheduler cost, which the paused
//! clock does not model. The triage's classification is INVESTIGATE-LATER and
//! its decision rule is:
//!
//! > p99 of `t1−t0` ≈ 22 ms indicts the send path; p99 of `t2−t1` ≈ 22 ms with
//! > `t1−t0` in microseconds indicts wake/scheduling and closes the item as
//! > "not slither".
//!
//! The original harness was uncommitted scratch and is gone. This is its
//! replacement, and it is committed.
//!
//! # Why there are five series and not two
//!
//! [`Connection::send_datagram`](slither::Connection::send_datagram) **is not
//! `async` and performs no syscall**: it bound-checks the payload against
//! §11.4, pushes into §11.3's send queue, marks the connection dirty, and
//! returns. The `send_to` syscall happens later, on the driver's next turn.
//!
//! So `t1 − t0` is an enqueue *by construction*, and the decision rule's first
//! branch cannot fire however slow the send path is — the two-way split can
//! only ever land on the second branch, which makes it an unfalsifiable test
//! of the hypothesis it was written to settle.
//!
//! This harness therefore also wraps endpoint A's socket in [`TimedWire`], an
//! application-supplied [`Wire`] (§16.3 normative property 1 — "the
//! application supplies it") that times every `send_to`. That splits what
//! `t2 − t1` lumps together:
//!
//! | Series | Interval | What a tall p99 here would indict |
//! |---|---|---|
//! | `send_call` | `t0` → `t1` | the handle call — enqueue only |
//! | `wire_dispatch` | `t1` → first `send_to` **entry** | driver wake + drain: tokio/OS scheduling |
//! | `wire_syscall` | that `send_to` entry → exit | **the send path**, measured directly |
//! | `reply_wait` | `t1` → `t2` | everything downstream of the handle |
//! | `total` | `t0` → `t2` | continuity with the 131 µs / 22105 µs figures |
//!
//! `wire_dispatch` and `wire_syscall` are a strict prefix of `reply_wait`, so
//! the five decompose the tail rather than merely halving it. **No production
//! code is instrumented**: `Wire` is a public seam and `TimedWire` lives here.
//!
//! # Topology — deliberately D's, not a better one
//!
//! One process, **one current-thread runtime inside one `LocalSet`
//! (`slither::block_on`), both endpoints on that single OS thread**, two real
//! `tokio::net::UdpSocket`s on `127.0.0.1:0`, real unpaused clock. That is
//! what D measured and the anomaly is a property of that arrangement: when the
//! one thread is not scheduled, *both* drivers stall together, which presents
//! as low CPU (blocked, not spinning). Splitting the endpoints across threads
//! would be a different experiment and would not be comparable.
//!
//! # Running it
//!
//! ```text
//! cargo run --release --example audit_udp
//! ```
//!
//! Release, always: the triage records that the debug tail is *worse*
//! (`p99_us=63424`), which is evidence about the same phenomenon and not a
//! second one, but the numbers to compare against are the release numbers.
//!
//! It prints one machine-greppable line per series:
//!
//! ```text
//! SERIES <name> n=<count> p50_us=… p90_us=… p99_us=… min_us=… max_us=… p50_ns=… p99_ns=… max_ns=…
//! ```
//!
//! The nanosecond fields are there because two of the five series are
//! expected to be sub-microsecond, where a microsecond figure of `0` reports
//! nothing.
//!
//! # This is a measurement, not a gate
//!
//! Nothing here asserts a bound. The tail it exists to characterise is a
//! property of the machine it runs on, and a threshold would be a flake on
//! any loaded host. The verdict is written up in
//! `.spec-v2-clean-slate/round41-N-p99-split.md`.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use slither::packet::ReferenceSuite;
use slither::shell::wire::Wire;
use slither::{Connection, Endpoint, Identity, SoftwareIdentity};

// ══════════════════════════════════════════════════════════════════════
// SIZING
// ══════════════════════════════════════════════════════════════════════

/// Timed round trips. D's original count, kept exactly so the `total`
/// series is comparable percentile for percentile.
const ITERATIONS: usize = 1000;

/// Discarded round trips before timing starts.
///
/// D's phase C ran after phases A and B, so it was warm; a cold phase C
/// would differ from the number being reproduced for reasons that have
/// nothing to do with the anomaly. Fifty is enough to fault in the send and
/// receive paths and to let the connection leave its initial congestion
/// state, and small enough to cost under 10 ms.
const WARMUP: usize = 50;

/// Datagram payload. Small: this is a latency probe, one datagram is one
/// packet, and a payload near `MAX_DATAGRAM_PAYLOAD` would start measuring
/// the copy. The first eight bytes carry the iteration counter so the echo
/// can be checked against the request rather than merely counted.
const PAYLOAD: usize = 64;

/// How long to wait for an echo before declaring the run broken.
///
/// §11.1 promises no delivery, so a lost datagram is legal and would
/// otherwise hang `recv_datagram` forever — a stall that reads as a hang
/// rather than as a result. Five seconds is ~57× the worst single round
/// trip on record (86 ms) and far below any plausible run time.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);

/// The identity type. `SoftwareIdentity` rather than
/// `testutil::CountingIdentity` on purpose: `testutil` is behind the
/// `test-util` feature, and using it would force a `[[example]]`
/// `required-features` stanza and drop this target out of the
/// feature-less `cargo build --release --all-targets` gate.
type Id = SoftwareIdentity<ReferenceSuite, ChaCha20Rng>;

// ══════════════════════════════════════════════════════════════════════
// THE INSTRUMENTED WIRE
// ══════════════════════════════════════════════════════════════════════

/// One `send_to` call's timing, as observed at the [`Wire`] seam.
#[derive(Clone, Copy)]
struct SendSpan {
    /// When the driver entered `send_to`.
    entry: Instant,
    /// When it returned.
    exit: Instant,
    /// Datagram length on the wire.
    ///
    /// Recorded so the matching in [`match_sends`] can be *checked* rather
    /// than assumed: a ping-pong emits more wire sends than round trips
    /// (ACK-carrying packets), and if every matched span has one length,
    /// the match picked the same kind of packet every time.
    len: usize,
}

/// A `tokio::net::UdpSocket` that records the entry and exit instant of
/// every `send_to`.
///
/// `Rc<RefCell<_>>` and not a lock: the driver is a single `!Send` actor
/// (§16.3) and this runs on its thread, so there is no contention to
/// synchronise and a lock would add cost to the very path being measured.
/// The borrow is held across no `.await`.
struct TimedWire {
    inner: tokio::net::UdpSocket,
    sends: Rc<RefCell<Vec<SendSpan>>>,
}

impl Wire for TimedWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        let entry = Instant::now();
        let len = buf.len();
        let result = self.inner.send_to(buf, addr).await;
        let exit = Instant::now();
        self.sends.borrow_mut().push(SendSpan { entry, exit, len });
        result
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.inner.recv_from(buf).await
    }
}

// ══════════════════════════════════════════════════════════════════════
// REPORTING
// ══════════════════════════════════════════════════════════════════════

/// A named series of durations, in nanoseconds.
struct Series {
    name: &'static str,
    samples: Vec<u64>,
}

impl Series {
    fn new(name: &'static str, samples: Vec<u64>) -> Self {
        Self { name, samples }
    }

    /// Nearest-rank percentile over the sorted samples.
    ///
    /// `sorted` must be non-empty; the caller checks.
    fn percentile(sorted: &[u64], q: f64) -> u64 {
        let rank = (q * sorted.len() as f64).ceil() as usize;
        sorted[rank.clamp(1, sorted.len()) - 1]
    }

    /// One machine-greppable line.
    fn print(&self) {
        if self.samples.is_empty() {
            println!("SERIES {} n=0 (no samples)", self.name);
            return;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let us = |ns: u64| ns / 1_000;
        let p50 = Self::percentile(&sorted, 0.50);
        let p90 = Self::percentile(&sorted, 0.90);
        let p99 = Self::percentile(&sorted, 0.99);
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        println!(
            "SERIES {} n={} p50_us={} p90_us={} p99_us={} min_us={} max_us={} \
             p50_ns={} p99_ns={} max_ns={}",
            self.name,
            sorted.len(),
            us(p50),
            us(p90),
            us(p99),
            us(min),
            us(max),
            p50,
            p99,
            max,
        );
    }
}

// ══════════════════════════════════════════════════════════════════════
// FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// Bring up one connection over real loopback UDP, with endpoint A's socket
/// instrumented.
///
/// Both endpoints are returned so the caller keeps them alive: dropping the
/// last handle of an endpoint stops its driver (§16.3), which would stall
/// the probe rather than fail it.
async fn loopback_pair() -> (
    Endpoint<Id>,
    Endpoint<Id>,
    Connection<ReferenceSuite>,
    Connection<ReferenceSuite>,
    Rc<RefCell<Vec<SendSpan>>>,
) {
    let sock_a = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind the initiator socket on loopback");
    let sock_b = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind the responder socket on loopback");
    let addr_b = sock_b.local_addr().expect("the responder's bound address");

    // Seeded rather than OS-seeded so a run is replayable. This is a
    // measurement harness and the keys protect nothing; the doc note on
    // `SoftwareIdentity::generate` is about production.
    let id_a: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([0xA1; 32]))
        .expect("a seeded ChaCha20 stream yields a valid P-256 scalar");
    let id_b: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([0xB2; 32]))
        .expect("a seeded ChaCha20 stream yields a valid P-256 scalar");
    let pk_b = *Identity::public_static(&id_b);

    let sends = Rc::new(RefCell::new(Vec::with_capacity(4 * ITERATIONS)));
    let wire_a = TimedWire {
        inner: sock_a,
        sends: Rc::clone(&sends),
    };

    let ep_a: Endpoint<Id> = Endpoint::builder()
        .identity(id_a)
        .wire(wire_a)
        .rng_seed([0x11; 32])
        .build();
    let ep_b: Endpoint<Id> = Endpoint::builder()
        .identity(id_b)
        .wire(sock_b)
        .rng_seed([0x22; 32])
        .build();

    let dial = async {
        ep_a.connect(addr_b, pk_b)
            .expect("mint the pending connection")
            .await
            .expect("the dial completed")
    };
    let accept = async {
        let intro = ep_b.accept().await.expect("an introduction arrived");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await.expect("accept")
    };
    let (ca, cb) = tokio::join!(dial, accept);

    (ep_a, ep_b, ca, cb, sends)
}

// ══════════════════════════════════════════════════════════════════════
// PHASE C
// ══════════════════════════════════════════════════════════════════════

/// One round trip: send, wait for the echo, check it is the right one.
///
/// Returns `(t0, t1, t2)`. The echo check is a correctness guard on the
/// measurement, not defensive coding: a stale or duplicate datagram
/// accepted as this iteration's reply would report a *shorter* interval
/// than actually elapsed, and a dropped one would be silently replaced by
/// the next.
async fn round_trip(
    conn: &Connection<ReferenceSuite>,
    seq: u64,
    payload: &mut [u8; PAYLOAD],
) -> (Instant, Instant, Instant) {
    payload[..8].copy_from_slice(&seq.to_le_bytes());

    let t0 = Instant::now();
    conn.send_datagram(payload).expect("queue the datagram");
    let t1 = Instant::now();

    let echo = tokio::time::timeout(ECHO_TIMEOUT, conn.recv_datagram())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "no echo for datagram {seq} within {ECHO_TIMEOUT:?} — \
                 §11.1 permits the loss, but on loopback it means the run is broken"
            )
        })
        .expect("the connection outlived the probe");
    let t2 = Instant::now();

    assert_eq!(
        &echo[..8],
        &seq.to_le_bytes(),
        "the echo did not match the request — the ping-pong lost its lockstep",
    );
    (t0, t1, t2)
}

/// For each `t1`, the first `send_to` span whose entry is at or after it.
///
/// Returns `(dispatch_ns, syscall_ns, matched_lengths)`.
///
/// `spans` is in entry order (one driver task, one push per call), so this
/// is a merge rather than a search. Iterations with no matching span — the
/// last one can have its send recorded after the loop reads the log — are
/// dropped, which is why the two wire series can report a smaller `n`.
///
/// # What this is, exactly
///
/// **The next wire send after the handle call**, not "the packet carrying
/// this datagram" — those are the same thing only if the driver emits the
/// datagram's packet first on the turn it wakes for. A ping-pong emits more
/// sends than round trips, so that is worth checking rather than asserting:
/// the matched lengths are returned and printed, and a single distinct
/// length across all 1000 matches is the evidence that one kind of packet
/// was picked every time. Either way the interval is the scheduling
/// quantity the split exists to isolate.
fn match_sends(spans: &[SendSpan], t1s: &[Instant]) -> (Vec<u64>, Vec<u64>, Vec<usize>) {
    let mut dispatch = Vec::with_capacity(t1s.len());
    let mut syscall = Vec::with_capacity(t1s.len());
    let mut lengths = Vec::with_capacity(t1s.len());
    let mut cursor = 0usize;
    for &t1 in t1s {
        while cursor < spans.len() && spans[cursor].entry < t1 {
            cursor += 1;
        }
        let Some(span) = spans.get(cursor) else { break };
        dispatch.push((span.entry - t1).as_nanos() as u64);
        syscall.push((span.exit - span.entry).as_nanos() as u64);
        lengths.push(span.len);
        cursor += 1;
    }
    (dispatch, syscall, lengths)
}

/// `len=count` pairs, ascending, for a line of output.
fn length_histogram(lengths: &[usize]) -> String {
    let mut sorted = lengths.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::new();
    for len in sorted {
        match out.last_mut() {
            Some((seen, count)) if *seen == len => *count += 1,
            _ => out.push((len, 1usize)),
        }
    }
    out.iter()
        .map(|(len, count)| format!("{len}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    println!("audit_udp — round 41 item 3.1, the split-timer loopback probe");
    println!(
        "  iterations={ITERATIONS} warmup={WARMUP} payload={PAYLOAD}B \
         topology=one-thread/one-LocalSet/two-real-UDP-sockets"
    );
    println!();

    slither::block_on(async {
        let (ep_a, ep_b, ca, cb, sends) = loopback_pair().await;

        // The peer: echo every datagram straight back. It shares the one
        // OS thread with both drivers and with the loop below — that
        // sharing is the topology under test, not a shortcut.
        let echo = tokio::task::spawn_local(async move {
            while let Ok(datagram) = cb.recv_datagram().await {
                // §11.1 promises nothing, so a full send queue dropping the
                // oldest is legal; the sender's lockstep assert is what
                // would catch it mattering.
                let _ = cb.send_datagram(&datagram);
            }
        });

        let mut payload = [0u8; PAYLOAD];
        for seq in 0..WARMUP as u64 {
            round_trip(&ca, seq, &mut payload).await;
        }

        let mut send_call = Vec::with_capacity(ITERATIONS);
        let mut reply_wait = Vec::with_capacity(ITERATIONS);
        let mut total = Vec::with_capacity(ITERATIONS);
        let mut t1s = Vec::with_capacity(ITERATIONS);

        sends.borrow_mut().clear();
        let wall = Instant::now();
        for i in 0..ITERATIONS as u64 {
            let (t0, t1, t2) = round_trip(&ca, WARMUP as u64 + i, &mut payload).await;
            send_call.push((t1 - t0).as_nanos() as u64);
            reply_wait.push((t2 - t1).as_nanos() as u64);
            total.push((t2 - t0).as_nanos() as u64);
            t1s.push(t1);
        }
        let elapsed = wall.elapsed();

        let spans = sends.borrow().clone();
        let (dispatch, syscall, matched_lengths) = match_sends(&spans, &t1s);
        let all_lengths = spans.iter().map(|s| s.len).collect::<Vec<_>>();

        for series in [
            Series::new("send_call", send_call),
            Series::new("wire_dispatch", dispatch),
            Series::new("wire_syscall", syscall),
            Series::new("reply_wait", reply_wait),
            Series::new("total", total),
        ] {
            series.print();
        }
        println!(
            "WIRE_SENDS total={} per_round_trip={:.2} all_lengths={}",
            spans.len(),
            spans.len() as f64 / ITERATIONS as f64,
            length_histogram(&all_lengths),
        );
        println!(
            "WIRE_MATCHED n={} lengths={}",
            matched_lengths.len(),
            length_histogram(&matched_lengths),
        );
        println!(
            "PHASE_C_DONE iterations={ITERATIONS} wall_ms={}",
            elapsed.as_millis(),
        );

        echo.abort();
        drop(ca);
        drop(ep_a);
        drop(ep_b);
    });
}

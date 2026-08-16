//! **slither throughput benchmarks — what the protocol can carry in ideal
//! conditions.**
//!
//! Two fabrics, deliberately, because they bound the answer from opposite
//! sides and neither one alone is the number:
//!
//! | Fabric | What it includes | What it is good for |
//! |---|---|---|
//! | `loopback/*` | real `tokio::net::UdpSocket` on `127.0.0.1`, real syscalls, real timers | **the honest end-to-end number** |
//! | `inmem/*` | `testutil::Network`, no kernel at all | isolating protocol CPU from the socket |
//!
//! # Read `inmem/*` as a floor, not a ceiling
//!
//! `testutil::FlakyWire` is a *test* fixture and it is not free: `send_to`
//! pushes a `Spied { bytes: buf.to_vec() }` into the network tap on **every**
//! datagram, and queues a second `buf.to_vec()` for delivery. That is two
//! heap allocations and two copies per datagram that no real wire performs,
//! and the tap is retained until something drains it. So `inmem` is *not*
//! "the protocol with the kernel removed" — it is the protocol plus a
//! per-datagram allocation the fixture adds. Treated as a lower bound on the
//! kernel-free ceiling it is still informative; treated as the ceiling it is
//! wrong. This benchmark drains the tap between samples so the measurement
//! does not also become a memory-growth curve.
//!
//! # No `criterion`
//!
//! Deliberate. `cargo deny check` is a release gate and `Cargo.lock` is not
//! committed, so every CI run re-resolves the whole graph from the index —
//! criterion's ~40-crate dev tree would be that much more surface for a
//! semver-compatible break to land in, and it must also hold the 1.96 MSRV
//! under `--all-targets`. What criterion buys over the harness below is
//! outlier analysis and regression tracking; what is wanted here is a
//! throughput figure with its spread. That trade is worth revisiting the day
//! these become regression gates rather than measurements.
//!
//! # Running it
//!
//! ```text
//! cargo bench --features test-util --bench throughput
//! cargo bench --features test-util --bench throughput -- loopback
//! ```
//!
//! The optional argument is a substring filter over benchmark names.
//!
//! # Why the numbers move
//!
//! Every figure here is single-threaded by construction: the shell is one
//! `!Send` actor (§16.3) and **both** endpoints run on the same
//! current-thread runtime inside one `LocalSet`. A loopback figure of `N`
//! MiB/s therefore means the CPU carried `N` MiB/s of application payload
//! while also doing the *peer's* receive work. It is not comparable to a
//! two-process iperf number and should not be quoted as one.

use std::time::{Duration, Instant};

use slither::constants::MAX_DATAGRAM_PAYLOAD;
use slither::testutil::{
    CountingIdentity, FlakyPolicy, Pair, TestConnection, TestEndpoint, TestIdentity, TestPublicKey,
    TestRecvStream, TestSendStream,
};
use tokio::runtime::Runtime;
use tokio::task::LocalSet;

// ══════════════════════════════════════════════════════════════════════
// SIZING
// ══════════════════════════════════════════════════════════════════════

/// Payload moved per timed sample. Large enough that connection-level flow
/// control (`INITIAL_MAX_DATA`, 1 MiB) and stream-level credit
/// (`INITIAL_MAX_STREAM_DATA`, 256 KiB) both cycle many times inside one
/// sample — a sample that fits inside the initial window would measure the
/// window, not the protocol.
const SAMPLE_BYTES: usize = 4 << 20;

/// One `write` call's size. 64 KiB is a realistic application write and is
/// well above the 1169-byte payload MTU, so the send path does the framing
/// and packetisation work rather than the benchmark doing it by hand.
const CHUNK: usize = 64 << 10;

/// Timed samples per benchmark, after one discarded warmup.
///
/// Five is few for a statistical claim and enough for the question actually
/// being asked: *is the order of magnitude right, and is it stable?* The
/// report prints min/median/max rather than a mean precisely so a reader
/// cannot mistake it for a distribution.
const SAMPLES: usize = 5;

/// Datagrams per sample on the unreliable path.
const DATAGRAM_COUNT: usize = 4096;

/// Bytes per single-shot message. Comfortably under `MESSAGE_RECV_MAX`
/// (256 KiB) and large enough to be a transfer rather than a round trip.
const MESSAGE_BYTES: usize = 16 << 10;

/// Messages per sample.
const MESSAGE_COUNT: usize = 256;

/// Full connections per sample on the handshake benchmark.
const HANDSHAKES: usize = 16;

/// One-way fabric delay for the bandwidth-delay-product probe, so the RTT
/// is twice this. 10 ms is a plausible same-region network hop and is far
/// enough above tokio's ~1 ms timer granularity that the timer is not what
/// is being measured.
const ONE_WAY_DELAY: Duration = Duration::from_millis(10);

/// Payload per sample on the RTT probe. Deliberately smaller than
/// `SAMPLE_BYTES`: at the predicted `256 KiB / 20 ms` this still takes
/// ~160 ms per sample, and a 4 MiB sample would spend two seconds proving
/// a point one already made. Still comfortably larger than both windows,
/// so each one cycles many times.
const RTT_SAMPLE_BYTES: usize = 2 << 20;

/// Samples on the RTT probe.
const RTT_SAMPLES: usize = 3;

/// Streams opened side by side on the parallel-stream benchmark.
const PARALLEL_STREAMS: usize = 4;

/// How long the datagram receiver waits for a datagram that may have been
/// dropped before it declares the batch finished. The unreliable path is
/// allowed to lose (`DATAGRAM_SEND_QUEUE` / `DATAGRAM_RECV_QUEUE` are both
/// 64), so this benchmark reports the delivery ratio instead of asserting
/// one — an assert here would be a flake, and a silent `unwrap` would report
/// a stall as a hang.
const DATAGRAM_IDLE: Duration = Duration::from_millis(250);

// ══════════════════════════════════════════════════════════════════════
// REPORTING
// ══════════════════════════════════════════════════════════════════════

/// One benchmark's result: what it moved, and how long each sample took.
struct Report {
    name: &'static str,
    /// Payload bytes per sample, as the *application* counts them. Wire
    /// bytes are higher — headers, AEAD tags, acks — and deliberately not
    /// measured here: the question is what a consumer gets, not what the
    /// protocol spends.
    bytes_per_sample: u64,
    /// Alternative denominator for the benchmarks whose natural unit is not
    /// bytes (handshakes). `None` means report bytes only.
    ops_per_sample: Option<(u64, &'static str)>,
    samples: Vec<Duration>,
    /// A benchmark-specific remark — a delivery ratio, a caveat.
    note: Option<String>,
}

impl Report {
    fn sorted(&self) -> Vec<Duration> {
        let mut s = self.samples.clone();
        s.sort_unstable();
        s
    }

    fn mib_per_sec(&self, d: Duration) -> f64 {
        let secs = d.as_secs_f64();
        if secs <= 0.0 {
            return f64::INFINITY;
        }
        (self.bytes_per_sample as f64 / (1024.0 * 1024.0)) / secs
    }

    fn ops_per_sec(&self, d: Duration) -> Option<f64> {
        let (ops, _) = self.ops_per_sample?;
        let secs = d.as_secs_f64();
        (secs > 0.0).then(|| ops as f64 / secs)
    }

    fn print(&self) {
        let s = self.sorted();
        let (best, mid, worst) = (s[0], s[s.len() / 2], s[s.len() - 1]);

        println!("  {}", self.name);
        if let Some((_, unit)) = self.ops_per_sample {
            // Rate first for the op-denominated benchmarks: "connections
            // per second" is the figure someone is looking for, and the
            // per-op cost is what they check it against.
            let rate = |d: Duration| self.ops_per_sec(d).unwrap_or(f64::INFINITY);
            let each = |d: Duration| {
                d.as_secs_f64() * 1e3 / self.ops_per_sample.expect("checked above").0 as f64
            };
            println!(
                "      {:>9.0} {unit}/s   (median {:.0}, worst {:.0})",
                rate(best),
                rate(mid),
                rate(worst),
            );
            println!(
                "      {:>9.3} ms each  (median {:.3}, worst {:.3})",
                each(best),
                each(mid),
                each(worst),
            );
        } else {
            println!(
                "      {:>9.1} MiB/s    (median {:.1}, worst {:.1})",
                self.mib_per_sec(best),
                self.mib_per_sec(mid),
                self.mib_per_sec(worst),
            );
            println!(
                "      {:>9.1} Mbit/s   over {} samples of {:.1} MiB",
                self.mib_per_sec(best) * 8.0 * 1.048_576,
                self.samples.len(),
                self.bytes_per_sample as f64 / (1024.0 * 1024.0),
            );
        }
        if let Some(note) = &self.note {
            println!("      note: {note}");
        }
        println!();
    }
}

// ══════════════════════════════════════════════════════════════════════
// STREAM I/O HELPERS
//
// `write` and `read` are the §16.2 verbs and both are partial: `write`
// returns how many bytes it took, `read` returns `Ok(None)` at end of
// stream and `Ok(Some(n))` otherwise. A benchmark that ignored either
// would measure a shorter transfer than it reported.
// ══════════════════════════════════════════════════════════════════════

/// Write every byte of `buf`, looping over partial writes.
async fn write_all(tx: &mut TestSendStream, buf: &[u8]) {
    let mut off = 0;
    while off < buf.len() {
        let n = tx.write(&buf[off..]).await.expect("stream write");
        assert!(n > 0, "write returned 0 for a non-empty buffer");
        off += n;
    }
}

/// Read exactly `want` bytes, panicking on an early end of stream.
/// Returns how many `read` calls it took.
///
/// An early `Ok(None)` here would otherwise show up as a *fast* sample —
/// the transfer that did not happen is the quickest one — so this is a
/// correctness guard on the measurement, not defensive coding.
///
/// # Why the call count is returned
///
/// It is the diagnostic that separates two very different explanations for
/// a slow stream. `scratch` is 64 KiB and the wire payload is 1169 bytes,
/// so if `read` coalesces the contiguous bytes already sitting in the
/// reassembly buffer, the mean fill approaches 64 KiB and the await count
/// is small. If instead it hands back one frame at a time, the mean fill
/// pins near 1169 regardless of how much is buffered, and the transfer pays
/// a full poll-and-wake cycle per **kilobyte**. Those two builds differ by
/// a factor of fifty in wakeups and are indistinguishable from the MiB/s
/// figure alone.
async fn read_exactly(rx: &mut TestRecvStream, want: usize, scratch: &mut [u8]) -> usize {
    let mut got = 0;
    let mut reads = 0;
    while got < want {
        let cap = scratch.len().min(want - got);
        reads += 1;
        match rx.read(&mut scratch[..cap]).await.expect("stream read") {
            Some(n) => got += n,
            None => panic!("end of stream after {got} of {want} bytes — the sample is short"),
        }
    }
    reads
}

/// A payload whose bytes vary with offset, so a benchmark can never be
/// accidentally measuring a compressible or memset-friendly buffer.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

// ══════════════════════════════════════════════════════════════════════
// FIXTURES
// ══════════════════════════════════════════════════════════════════════

/// Establish one connection over real UDP on the loopback interface.
///
/// Uses `testutil::CountingIdentity` rather than `SoftwareIdentity` only to
/// avoid naming an RNG type in this file; the counter is a `Cell` increment
/// and does not move the measurement.
async fn loopback_pair() -> (TestEndpoint, TestEndpoint, TestConnection, TestConnection) {
    let sock_a = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind a");
    let sock_b = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind b");
    let addr_b = sock_b.local_addr().expect("local_addr b");

    let id_a: TestIdentity = CountingIdentity::seeded([0xA1; 32]);
    let id_b: TestIdentity = CountingIdentity::seeded([0xB2; 32]);
    let pk_b: TestPublicKey = *slither::Identity::public_static(&id_b);

    let ep_a: TestEndpoint = slither::Endpoint::builder()
        .identity(id_a)
        .wire(sock_a)
        .rng_seed([0x11; 32])
        .build();
    let ep_b: TestEndpoint = slither::Endpoint::builder()
        .identity(id_b)
        .wire(sock_b)
        .rng_seed([0x22; 32])
        .build();

    let dial = async {
        ep_a.connect(addr_b, pk_b)
            .expect("connect")
            .await
            .expect("the dial completed")
    };
    let accept = async {
        let intro = ep_b.accept().await.expect("an introduction");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await.expect("accept")
    };
    let (ca, cb) = tokio::join!(dial, accept);

    // The endpoints are returned so the caller keeps them alive: dropping
    // the last handle of an endpoint stops its driver (§16.3), which would
    // stall the benchmark rather than fail it.
    (ep_a, ep_b, ca, cb)
}

/// Open one bidirectional stream and hand back the writing half on `ca` and
/// the reading half on `cb`.
///
/// The stream is opened **once** and reused across every sample, so the
/// figures are data-plane throughput and not stream-open churn. A priming
/// byte is required: `open_bi` allocates the id locally, and the peer only
/// learns the stream exists when a frame carrying it arrives, so
/// `accept_bi` would otherwise never resolve.
async fn one_way_stream(
    ca: &TestConnection,
    cb: &TestConnection,
) -> (TestSendStream, TestRecvStream) {
    let bi_a = ca.open_bi().await.expect("open_bi");
    let (mut tx, _unused_rx) = bi_a.split();
    tx.write(b"\0").await.expect("priming write");

    let bi_b = cb.accept_bi().await.expect("accept_bi");
    let (_unused_tx, mut rx) = bi_b.split();
    let mut prime = [0u8; 1];
    read_exactly(&mut rx, 1, &mut prime).await;

    (tx, rx)
}

/// Move `bytes` from `tx` to `rx`, timed. Returns the elapsed time and the
/// number of `read` calls the receiver needed.
///
/// Writer and reader are joined rather than sequenced: they are two halves
/// of one connection on one thread, and a sequential shape would deadlock
/// the moment the transfer exceeded the flow-control window — which
/// `SAMPLE_BYTES` is chosen to guarantee.
async fn stream_sample(
    tx: &mut TestSendStream,
    rx: &mut TestRecvStream,
    chunk: &[u8],
    bytes: usize,
) -> (Duration, usize) {
    let chunks = bytes / CHUNK;
    let start = Instant::now();
    let write = async {
        for _ in 0..chunks {
            write_all(tx, chunk).await;
        }
    };
    let read = async {
        let mut scratch = vec![0u8; CHUNK];
        read_exactly(rx, chunks * CHUNK, &mut scratch).await
    };
    let (_, reads) = tokio::join!(write, read);
    (start.elapsed(), reads)
}

// ══════════════════════════════════════════════════════════════════════
// BENCHMARKS
// ══════════════════════════════════════════════════════════════════════

/// Bulk stream transfer over real UDP on `127.0.0.1`. **The headline.**
fn bench_loopback_stream(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let (samples, reads) = ls.block_on(rt, async {
        let (_ep_a, _ep_b, ca, cb) = loopback_pair().await;
        let (mut tx, mut rx) = one_way_stream(&ca, &cb).await;
        let chunk = payload(CHUNK);

        // Warmup, discarded: it pays the congestion window's ramp from
        // `INITIAL_WINDOW` (12 000 bytes) and the first-touch page faults
        // on both buffers. Timing it would report the ramp as the rate.
        stream_sample(&mut tx, &mut rx, &chunk, SAMPLE_BYTES).await;

        let mut out = Vec::with_capacity(SAMPLES);
        let mut reads = 0;
        for _ in 0..SAMPLES {
            let (d, r) = stream_sample(&mut tx, &mut rx, &chunk, SAMPLE_BYTES).await;
            out.push(d);
            reads += r;
        }
        (out, reads)
    });

    Report {
        name: "loopback/stream        one bi stream, real UDP syscalls",
        bytes_per_sample: SAMPLE_BYTES as u64,
        ops_per_sample: None,
        samples,
        note: Some(format!(
            "mean read fill {:.0} B over {reads} read calls; both endpoints share one thread",
            (SAMPLE_BYTES * SAMPLES) as f64 / reads as f64,
        )),
    }
}

/// The same transfer over the kernel-free fixture.
fn bench_inmem_stream(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let (samples, reads) = ls.block_on(rt, async {
        let pair = Pair::seeded(0x5117_4E00);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let (mut tx, mut rx) = one_way_stream(&ca, &cb).await;
        let chunk = payload(CHUNK);

        stream_sample(&mut tx, &mut rx, &chunk, SAMPLE_BYTES).await;

        let mut out = Vec::with_capacity(SAMPLES);
        let mut reads = 0;
        for _ in 0..SAMPLES {
            // Drain between samples, never inside one: the tap retains a
            // copy of every datagram ever sent, so leaving it to grow would
            // fold an allocator curve into the later samples and report it
            // as a slowdown in the protocol.
            tap.drain();
            let (d, r) = stream_sample(&mut tx, &mut rx, &chunk, SAMPLE_BYTES).await;
            out.push(d);
            reads += r;
        }
        (out, reads)
    });

    Report {
        name: "inmem/stream           one bi stream, no kernel",
        bytes_per_sample: SAMPLE_BYTES as u64,
        ops_per_sample: None,
        samples,
        note: Some(format!(
            "mean read fill {:.0} B; includes two Vec allocs + copies per datagram the fixture adds",
            (SAMPLE_BYTES * SAMPLES) as f64 / reads as f64,
        )),
    }
}

/// Four concurrent streams carrying the same total payload.
///
/// # The question this answers
///
/// `inmem/message` moves 16 KiB single-shot messages several times faster
/// than `inmem/stream` moves the same bytes through one stream, on the same
/// fabric and the same thread. Locally the flow-control window cannot be
/// the cause — loopback RTT makes `256 KiB / RTT` enormous — so either the
/// per-stream path costs more per byte than the message path, or a single
/// stream serialises something the connection could otherwise overlap.
///
/// Those two have opposite signatures and this separates them. If the
/// aggregate here matches `inmem/stream`, the limit is per **connection**
/// and adding streams buys nothing. If it scales toward the message figure,
/// the limit was per **stream**, and S13's independent-streams promise is
/// also a throughput lever rather than only a head-of-line-blocking one.
fn bench_inmem_streams_parallel(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let samples = ls.block_on(rt, async {
        let pair = Pair::seeded(0x0A_9A11E1);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;

        // Four explicit pairs rather than a `Vec`: each leg needs its own
        // `&mut` across one `join!`, and disjoint borrows out of a
        // collection would need either `split_at_mut` gymnastics or
        // `spawn_local` with a `'static` bound the handles do not have.
        let (mut tx0, mut rx0) = one_way_stream(&ca, &cb).await;
        let (mut tx1, mut rx1) = one_way_stream(&ca, &cb).await;
        let (mut tx2, mut rx2) = one_way_stream(&ca, &cb).await;
        let (mut tx3, mut rx3) = one_way_stream(&ca, &cb).await;

        let chunk = payload(CHUNK);
        let per = SAMPLE_BYTES / PARALLEL_STREAMS;

        let mut out = Vec::with_capacity(SAMPLES);
        for sample in 0..=SAMPLES {
            tap.drain();
            let start = Instant::now();
            let _ = tokio::join!(
                stream_sample(&mut tx0, &mut rx0, &chunk, per),
                stream_sample(&mut tx1, &mut rx1, &chunk, per),
                stream_sample(&mut tx2, &mut rx2, &chunk, per),
                stream_sample(&mut tx3, &mut rx3, &chunk, per),
            );
            if sample > 0 {
                out.push(start.elapsed());
            }
        }
        out
    });

    Report {
        name: "inmem/streams×4        four bi streams, same total bytes",
        bytes_per_sample: SAMPLE_BYTES as u64,
        ops_per_sample: None,
        samples,
        note: Some("compare against inmem/stream: equal means the connection is the limit, faster means the stream was".into()),
    }
}

/// **The bandwidth-delay-product probe.** One stream, with a known RTT
/// injected into the fabric.
///
/// # What this is testing, and why it is the important one
///
/// `flow.rs` builds both windows with `CreditWindow::new(<constant>)` and
/// regrants at half — there is **no auto-tuning anywhere**, so a receiver
/// never advertises more than `INITIAL_MAX_STREAM_DATA` (256 KiB) on a
/// stream or `INITIAL_MAX_DATA` (1 MiB) on the connection, no matter how
/// fast the path proves to be. A static window makes throughput
/// `window / RTT`, which is a *hard* ceiling that no amount of available
/// bandwidth lifts.
///
/// The local figures above cannot show this: loopback RTT is tens of
/// microseconds, so 256 KiB per RTT is far more than the CPU can seal
/// anyway, and the window never binds. Injecting a delay is what separates
/// "fast enough locally" from "fast on a real path", and the two are not
/// the same claim. If the measured rate tracks `window / RTT` as the delay
/// grows, the window is the limit and the CPU is not.
fn bench_inmem_stream_rtt(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let samples = ls.block_on(rt, async {
        let pair = Pair::seeded(0x00BD_9000);
        let tap = pair.net.tap();
        // Establish *before* slowing the wire, so the handshake's round
        // trips are not charged to the transfer.
        let (ca, cb) = pair.establish().await;
        let (mut tx, mut rx) = one_way_stream(&ca, &cb).await;

        let slow = FlakyPolicy::perfect().with_delay(ONE_WAY_DELAY, Duration::ZERO);
        pair.a.wire.set_policy(slow.clone());
        pair.b.wire.set_policy(slow);

        let chunk = payload(CHUNK);
        let mut out = Vec::with_capacity(RTT_SAMPLES);
        for _ in 0..RTT_SAMPLES {
            tap.drain();
            let (d, _) = stream_sample(&mut tx, &mut rx, &chunk, RTT_SAMPLE_BYTES).await;
            out.push(d);
        }
        out
    });

    // The prediction is printed beside the measurement so a reader can see
    // whether it held, rather than take the claim on trust.
    let rtt = ONE_WAY_DELAY.as_secs_f64() * 2.0;
    let predicted = (slither::constants::INITIAL_MAX_STREAM_DATA as f64 / (1024.0 * 1024.0)) / rtt;
    let best = samples.iter().copied().min().unwrap_or_default();
    let measured = (RTT_SAMPLE_BYTES as f64 / (1024.0 * 1024.0)) / best.as_secs_f64();

    Report {
        name: "inmem/stream@rtt       one bi stream, 20 ms injected RTT",
        bytes_per_sample: RTT_SAMPLE_BYTES as u64,
        ops_per_sample: None,
        samples,
        note: Some(format!(
            "window/RTT predicts {predicted:.1} MiB/s, measured {measured:.1} — \
             the 256 KiB stream window is static, so a real path caps at window/RTT"
        )),
    }
}

/// Unreliable datagrams — the `send_datagram` / `recv_datagram` path.
///
/// Reports the delivery ratio rather than asserting one. §18.1 makes this
/// path lossy by design and both queues are 64 deep, so a dropped datagram
/// is correct behaviour and an `unwrap` on the receiver would turn it into
/// a hang.
fn bench_inmem_datagram(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let (samples, delivered) = ls.block_on(rt, async {
        let pair = Pair::seeded(0x0A7A_6100);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let msg = payload(MAX_DATAGRAM_PAYLOAD);

        let mut out = Vec::with_capacity(SAMPLES);
        let mut got_total = 0u64;
        for sample in 0..=SAMPLES {
            tap.drain();
            let start = Instant::now();
            let send = async {
                for _ in 0..DATAGRAM_COUNT {
                    // Sync verb: it queues. Yield so the driver can drain
                    // the 64-slot send queue instead of overrunning it.
                    if ca.send_datagram(&msg).is_err() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            };
            let recv = async {
                let mut got = 0u64;
                while got < DATAGRAM_COUNT as u64 {
                    match tokio::time::timeout(DATAGRAM_IDLE, cb.recv_datagram()).await {
                        Ok(Ok(_)) => got += 1,
                        // Idle: the rest of this batch was dropped, which
                        // §18.1 permits. Stop rather than wait again.
                        Err(_) => break,
                        Ok(Err(e)) => panic!("connection lost mid-benchmark: {e}"),
                    }
                }
                got
            };
            let (_, got) = tokio::join!(send, recv);
            let elapsed = start.elapsed();
            if sample > 0 {
                // The idle timeout is wall time spent waiting, not work.
                // Leaving it in would report a drop as a slow protocol.
                out.push(if got < DATAGRAM_COUNT as u64 {
                    elapsed.saturating_sub(DATAGRAM_IDLE)
                } else {
                    elapsed
                });
                got_total += got;
            }
        }
        (out, got_total)
    });

    let attempted = (DATAGRAM_COUNT * SAMPLES) as u64;
    let ratio = delivered as f64 / attempted as f64 * 100.0;
    Report {
        name: "inmem/datagram         unreliable, 1169-byte payloads",
        // Count what actually arrived, per sample. Counting what was sent
        // would inflate the rate by exactly the loss.
        bytes_per_sample: (delivered / SAMPLES.max(1) as u64) * MAX_DATAGRAM_PAYLOAD as u64,
        ops_per_sample: None,
        samples,
        note: Some(format!(
            "{delivered}/{attempted} delivered ({ratio:.1}%) — throughput counts arrivals, not sends"
        )),
    }
}

/// Single-shot messages — the `send_message` / `recv_message` path.
fn bench_inmem_message(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let samples = ls.block_on(rt, async {
        let pair = Pair::seeded(0x0E55_A6E0);
        let tap = pair.net.tap();
        let (ca, cb) = pair.establish().await;
        let msg = payload(MESSAGE_BYTES);

        let mut out = Vec::with_capacity(SAMPLES);
        for sample in 0..=SAMPLES {
            tap.drain();
            let start = Instant::now();
            let send = async {
                for _ in 0..MESSAGE_COUNT {
                    ca.send_message(&msg).await.expect("send_message");
                }
            };
            let recv = async {
                for _ in 0..MESSAGE_COUNT {
                    let got = cb.recv_message().await.expect("recv_message");
                    assert_eq!(got.len(), MESSAGE_BYTES, "a message arrived truncated");
                }
            };
            tokio::join!(send, recv);
            if sample > 0 {
                out.push(start.elapsed());
            }
        }
        out
    });

    Report {
        name: "inmem/message          single-shot, 16 KiB each",
        bytes_per_sample: (MESSAGE_COUNT * MESSAGE_BYTES) as u64,
        ops_per_sample: None,
        samples,
        note: None,
    }
}

/// Full connection establishment: the §6.2 staged accept and its 4-DH
/// ladder, over the kernel-free fabric.
///
/// **What is inside the measurement**: two endpoint constructions (each
/// derives a P-256 static keypair, so two scalar multiplications), two
/// driver spawns, and the four-DH ladder itself. The keypair derivations
/// are real work this benchmark cannot separate out through the public API
/// — `Pair::seeded` is the only public two-endpoint constructor — so read
/// this as *connection setup*, not as the ladder alone.
fn bench_inmem_handshake(rt: &Runtime) -> Report {
    let ls = LocalSet::new();
    let samples = ls.block_on(rt, async {
        let mut out = Vec::with_capacity(SAMPLES);
        for sample in 0..=SAMPLES {
            let start = Instant::now();
            for i in 0..HANDSHAKES {
                // A distinct seed per connection: identical seeds would
                // give identical statics, and S3's supersession rule would
                // then make every connection after the first tear down its
                // predecessor — measuring supersession, not handshaking.
                let pair = Pair::seeded(0x4144_0000 + (sample * HANDSHAKES + i) as u64);
                let (ca, cb) = pair.establish().await;
                drop((ca, cb, pair));
            }
            if sample > 0 {
                out.push(start.elapsed());
            }
        }
        out
    });

    Report {
        name: "inmem/handshake        endpoint setup + full 4-DH ladder",
        bytes_per_sample: 0,
        ops_per_sample: Some((HANDSHAKES as u64, "conn")),
        samples,
        note: Some("includes two P-256 static keypair derivations per connection".into()),
    }
}

// ══════════════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════════════

fn main() {
    let filter = std::env::args()
        .skip(1)
        // `cargo bench` forwards its own flags to the harness; ignore them
        // rather than treating `--bench` as a name filter that matches
        // nothing and silently runs no benchmark.
        .find(|a| !a.starts_with('-'))
        .unwrap_or_default();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    #[allow(clippy::type_complexity)]
    let all: Vec<(&str, fn(&Runtime) -> Report)> = vec![
        ("loopback/stream", bench_loopback_stream),
        ("inmem/stream", bench_inmem_stream),
        ("inmem/streams-parallel", bench_inmem_streams_parallel),
        ("inmem/stream@rtt", bench_inmem_stream_rtt),
        ("inmem/datagram", bench_inmem_datagram),
        ("inmem/message", bench_inmem_message),
        ("inmem/handshake", bench_inmem_handshake),
    ];

    let selected: Vec<_> = all
        .into_iter()
        .filter(|(name, _)| filter.is_empty() || name.contains(&filter))
        .collect();

    if selected.is_empty() {
        eprintln!("no benchmark matches {filter:?}");
        std::process::exit(1);
    }

    println!();
    println!("slither throughput — one thread carries both endpoints");
    println!("{}", "─".repeat(72));
    println!();

    for (_, run) in selected {
        run(&rt).print();
    }
}

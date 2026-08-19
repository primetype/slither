//! **slither versus raw kernel TCP — the comparative matrix.**
//!
//! One binary, one scenario per CLI argument, one machine-greppable line per
//! matrix cell. It exists to answer a question the crate could not previously
//! answer with numbers: *where does a Noise-over-UDP userspace transport stand
//! against the kernel's own reliable stream, on the same host, with the same
//! CPU budget?* — and, since ruling 259(viii) landed
//! [`Config::with_flow_windows`](slither::config::Config::with_flow_windows), *what
//! does raising the flow-control windows actually buy on a path with delay?*
//!
//! # Read this before reading any number below
//!
//! **Raw TCP is expected to win the bulk cells on clean loopback, and that is
//! not a defect.** The two are not doing the same work:
//!
//! | | slither | raw TCP |
//! |---|---|---|
//! | Segment on the wire | ~1200 B datagram (§3.1 packet + AEAD tag) | up to 64 KiB, loopback MTU |
//! | Per-byte crypto | ChaCha20-Poly1305 seal **and** open, in this process | none |
//! | Framing, acks, recovery | userspace, this thread | kernel, and not on this thread |
//! | Both endpoints | one OS thread, by construction (§16.3's `!Send` actor) | one OS thread for the *userspace* half only |
//!
//! The last row is the one that is easiest to forget. This harness runs
//! **both** protocol endpoints on one current-thread runtime inside one
//! `LocalSet`, exactly as `benches/throughput.rs` does and for the same reason:
//! the shell is a single `!Send` actor and there is no other honest way to run
//! two of them in one process. Holding TCP to the same shape holds the
//! *userspace* CPU budget constant — but a TCP byte still gets copied,
//! segmented, acked and retransmitted by the kernel, on whatever core the
//! softirq lands on. slither has no such helper. A figure of `N` MiB/s for
//! slither means one core carried `N` MiB/s of payload while also sealing it,
//! opening it, and doing the peer's receive work; the TCP figure beside it does
//! not mean that. Neither is comparable to a two-process `iperf` number.
//!
//! # No `criterion`
//!
//! Same reasoning as `benches/throughput.rs`, unchanged: `cargo deny check` is
//! a release gate and `Cargo.lock` is not committed, so every run re-resolves
//! the graph from the index — criterion's ~40-crate dev tree is that much more
//! surface for a semver-compatible break, and it must hold the 1.96 MSRV under
//! `--all-targets` too. This file reports p50 with its spread and adds no
//! dependency at all: it uses `SoftwareIdentity` rather than
//! `testutil::CountingIdentity` precisely so it needs no `required-features`
//! stanza and stays inside the feature-less
//! `cargo build --release --all-targets` gate, the same choice
//! `examples/audit_udp.rs` documents.
//!
//! # The delay relay
//!
//! Every RTT above zero is produced by an in-file relay that adds a one-way
//! delay `D` in each direction, so `RTT = 2D`. It is **FIFO per direction**:
//! one reader task pushes `(now + D, bytes)` onto an unbounded queue and one
//! writer task pops, sleeps to the deadline and forwards. Deadlines are
//! non-decreasing within a direction, so nothing reorders, and nothing is
//! dropped — loss is deliberately **out of scope** here (the loss story is the
//! `testutil::FlakyWire` virtual-time suite, which models it properly and
//! resolves in virtual time).
//!
//! Two flavours, the same structural shape:
//!
//! - **UDP**: datagram-preserving. Two sockets; the client's address is learned
//!   from the first packet it sends. One `recv_from` + one `Vec` + one
//!   `send_to` per **datagram**.
//! - **TCP**: byte-stream. A listener in front, a fresh upstream connection
//!   behind, `read` up to [`TCP_RELAY_CHUNK`] bytes and forward that chunk when
//!   its deadline is due. One `read` + one `Vec` + one `write_all` per
//!   **chunk**.
//!
//! **This is an artefact and it is not symmetric.** The UDP relay does its work
//! per 1200-byte datagram; the TCP relay does it per 64 KiB chunk — roughly
//! fifty times fewer trips through the same machinery for the same payload. The
//! relay therefore costs slither more than it costs TCP, and any cell measured
//! *through* the relay carries that. It is documented rather than corrected
//! because correcting it would mean giving TCP a per-1200-byte relay, which is
//! not what a network does either.
//!
//! The relay runs on its **own OS thread with its own current-thread runtime**,
//! so its copies do not contend with the endpoints under test. That is one
//! extra thread of the machine's capacity that neither protocol's endpoints can
//! use, spent identically on both.
//!
//! ## The TCP relay is a proxy, and a proxy is not a link — read this
//!
//! **The UDP relay is a real link emulator. The TCP relay is not, and cannot
//! be.** UDP forwarding is stateless, so slither's two endpoints exchange
//! packets end to end and the delay is fully visible to its RTT estimator, its
//! congestion control and its flow control — the delayed slither figures are
//! measurements of slither on a delayed path.
//!
//! TCP is connection-oriented, so a userspace relay must **terminate** it: the
//! client's SYN is answered by the relay over zero-RTT loopback, and the
//! relay's own connection to the server is zero-RTT loopback too. Neither TCP
//! stack ever observes the delay. Two consequences, both of which the tables
//! below carry and neither of which can be corrected in userspace (it would
//! take IP-layer emulation — `dummynet`, `netem` — which is a root-level change
//! to the host and out of scope here):
//!
//! - **Establishment** loses exactly one RTT. A real TCP pays 1 RTT for
//!   SYN/SYN-ACK and 1 RTT for the byte round trip; through this relay it pays
//!   only the second. Every delayed `proto=tcp` establishment figure is
//!   therefore **one RTT lower than a real path would give**.
//! - **Bulk** loses the delay entirely. Neither stack's window is ever the
//!   binding constraint, because neither stack is on a delayed path; the
//!   relay's queue absorbs the delay instead. A delayed `proto=tcp` bulk figure
//!   is loopback TCP with latency added downstream of the control loop — an
//!   **unattainable upper bound**, not a comparable measurement.
//!
//! Both are marked `note=proxy-terminated` in the output so the flag travels
//! with the data rather than only with the prose.
//!
//! # Ramp, and why every bulk cell has one
//!
//! Bulk cells measure **steady state**: they run a fixed ramp ([`BULK_RAMP`])
//! during which nothing is counted, then take [`BULK_SAMPLES`] back-to-back
//! timed windows of [`BULK_WINDOW`] each on the *same* connection. The ramp
//! exists for three separate reasons and one of them is specific to this crate:
//!
//! - TCP's slow start and its receive-buffer autotuning both need several RTTs.
//! - slither's congestion window starts at `INITIAL_WINDOW` and grows.
//! - **A configured window raise is not instantaneous.** §10.2's initial values
//!   are never sent, so a peer assumes the ratified constants until a credit
//!   frame says otherwise. The connection-level raise rides the first packet
//!   the connection sends (`Connection::connecting`), but the *stream*-level
//!   raise is taken on the **first STREAM frame the peer sends on that stream**
//!   (`Streams::on_stream`, ruling 259(viii)) — not at open, because
//!   `pack_control` runs before the STREAM fill and §8.4 would make the frame
//!   inert. So the raise reaches the sender about one RTT into the transfer. A
//!   cell that started counting at byte zero would charge the raised
//!   configuration for the window it was in the act of leaving.
//!
//! # TCP socket settings
//!
//! `TCP_NODELAY` is **on** for the latency scenarios and left at the kernel
//! default (**off**, i.e. Nagle active) for the bulk scenarios, which is what a
//! bulk mover would actually get. No `SO_SNDBUF`/`SO_RCVBUF`/congestion-control
//! tuning of any kind: kernel defaults throughout, so what the tables show for
//! TCP is what the host gives an unconfigured socket. The relay sets
//! `TCP_NODELAY` on **its own** two sockets unconditionally, so the relay does
//! not contribute a Nagle artefact of its own on top of the delay it exists to
//! add.
//!
//! # Running it
//!
//! ```text
//! cargo run --release --example bench_vs_tcp -- all
//! cargo run --release --example bench_vs_tcp -- bulk-rtt
//! ```
//!
//! Scenarios: `establish`, `bulk`, `sweep`, `bulk-rtt`, `sweep-rtt`,
//! `pingpong`, `mux`, `all` (the default). Release, always — a debug build
//! measures `rustc -O0`.
//!
//! Each cell prints one line:
//!
//! ```text
//! BENCH scenario=… proto=… rtt_ms=… windows=… streams=… n=… p50=… p99=… p999=… min=… max=… unit=…
//! ```
//!
//! `p99`/`p999` are `-` where the sample count cannot support them (every bulk
//! cell: five samples have no 99th percentile, and pretending otherwise would
//! be the defect working rule 9 warns about).
//!
//! # This is a measurement, not a gate
//!
//! Nothing here asserts a bound. Every figure is a property of the host it ran
//! on, and a threshold would be a flake on any loaded machine. The measured
//! write-up lives in `.spec-v2-clean-slate/bench-vs-tcp-2026-08.md`.

use std::cell::{Cell, RefCell};
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use slither::packet::ReferenceSuite;
use slither::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

// ══════════════════════════════════════════════════════════════════════
// SIZING
// ══════════════════════════════════════════════════════════════════════

/// One application `write` call's size, both protocols. 64 KiB is a realistic
/// bulk write and is far above slither's ~1169-byte payload MTU, so the
/// protocol does the packetisation rather than the benchmark doing it by hand.
const CHUNK: usize = 64 << 10;

/// Discarded lead-in before any bulk cell starts counting. See the module
/// docs: this covers TCP slow start, slither's congestion window, and the
/// one-RTT delay before a configured stream-window raise reaches the sender.
const BULK_RAMP: Duration = Duration::from_secs(2);

/// One timed bulk window.
const BULK_WINDOW: Duration = Duration::from_secs(4);

/// Timed windows per bulk cell, taken back to back on one connection so the
/// spread reports measurement noise rather than restart variance.
const BULK_SAMPLES: usize = 5;

/// Bytes the TCP relay moves per forwarding step. Matches [`CHUNK`], so the
/// relay is one hop and not a re-segmenter.
const TCP_RELAY_CHUNK: usize = 64 << 10;

/// Chunks the TCP relay will hold in flight per direction before it stops
/// reading — so at most `TCP_RELAY_CHUNK * TCP_RELAY_DEPTH` = **64 MiB**.
///
/// # This bound is not tidiness, it is the proxy problem made visible
///
/// The UDP relay needs no bound: slither's own congestion and flow windows cap
/// what it puts on the wire, so the relay's queue is self-limiting at
/// `connection_window` in the worst case. A TCP proxy has no such protection —
/// **it terminates the connection**, so the client's TCP stack is talking to
/// the relay over zero-RTT loopback and its congestion control never sees the
/// delay this relay exists to add. Left unbounded it would buffer
/// `client_rate × D` bytes, which at gigabytes per second is a memory bomb.
///
/// The bound also puts a ceiling on the TCP-at-RTT column: `64 MiB / D`, i.e.
/// 6.4 GiB/s at 20 ms RTT, 2.6 GiB/s at 50 ms, 1.3 GiB/s at 100 ms. Read every
/// TCP figure in those cells against it.
const TCP_RELAY_DEPTH: usize = 1024;

/// Receive buffer for the UDP relay. Comfortably above slither's ~1200-byte
/// datagram; a truncating `recv_from` would silently corrupt the wire.
const UDP_RELAY_BUF: usize = 2048;

/// Timed round trips in the latency ping-pong.
const PING_ITERS: usize = 2000;

/// Discarded round trips before the ping-pong starts timing.
const PING_WARMUP: usize = 100;

/// Ping-pong payload. Small on purpose: one datagram is one packet and a
/// payload near the MTU would start measuring the copy.
const PING_PAYLOAD: usize = 64;

/// Concurrent uni-stream counts in the multiplexing scenario.
const MUX_STREAMS: [usize; 3] = [1, 4, 16];

/// Raised stream window. Chosen from bandwidth-delay-product arithmetic: the
/// worst RTT in the matrix is 100 ms, and a single stream cannot exceed
/// `window / RTT`, so 8 MiB caps that cell at 80 MiB/s — comfortably above
/// what one thread carrying both endpoints plus the AEAD can produce, so the
/// window stops being the binding constraint and the CPU becomes it. The
/// ratified default caps the same cell at `256 KiB / 100 ms` = **2.5 MiB/s**.
const RAISED_STREAM: u64 = 8 << 20;

/// Raised connection window: twice the stream window, so a single stream is
/// never gated by the connection ledger and the multiplexing cells have
/// headroom. §17.5's shape makes this a deliberate purchase — 16 MiB of
/// receive commitment per live connection.
const RAISED_CONN: u64 = 16 << 20;

/// The window ladder the `sweep` scenario walks on clean loopback.
///
/// It exists because the first run of `bulk` produced a result no arithmetic
/// predicts: raising the pair from the ratified default to 8 MiB / 16 MiB made
/// a zero-RTT single-stream transfer **slower**, not faster. A two-point
/// comparison cannot tell a cliff from a slope, and neither can name the
/// mechanism; the ladder plus the wire counters ([`WireCounters`]) can.
const SWEEP: [Windows; 5] = [
    WINDOWS_DEFAULT,
    Windows {
        stream: 512 << 10,
        connection: 2 << 20,
        label: "512Ki/2Mi",
    },
    Windows {
        stream: 1 << 20,
        connection: 4 << 20,
        label: "1Mi/4Mi",
    },
    Windows {
        stream: 2 << 20,
        connection: 8 << 20,
        label: "2Mi/8Mi",
    },
    WINDOWS_RAISED,
];

/// The identity type. `SoftwareIdentity` rather than
/// `testutil::CountingIdentity` on purpose, exactly as `examples/audit_udp.rs`
/// records: `testutil` is behind the `test-util` feature and naming it would
/// force an `[[example]]` `required-features` stanza, dropping this target out
/// of the feature-less `cargo build --release --all-targets` gate.
type Id = SoftwareIdentity<ReferenceSuite, ChaCha20Rng>;

/// Shorthand for the connection type this file works in.
type Conn = Connection<ReferenceSuite>;

/// One mebibyte, as the denominator every MiB/s figure divides by.
const MIB: f64 = 1024.0 * 1024.0;

// ══════════════════════════════════════════════════════════════════════
// REPORTING
// ══════════════════════════════════════════════════════════════════════

/// One matrix cell's result line.
struct Row {
    scenario: &'static str,
    proto: &'static str,
    rtt_ms: u64,
    windows: &'static str,
    streams: usize,
    unit: &'static str,
    /// Decimal places in the printed values.
    prec: usize,
    samples: Vec<f64>,
    note: Option<String>,
}

/// Nearest-rank percentile over an ascending slice. `sorted` must be non-empty.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

impl Row {
    fn emit(&self) {
        assert!(
            !self.samples.is_empty(),
            "{} produced no samples",
            self.scenario
        );
        let mut s = self.samples.clone();
        s.sort_by(f64::total_cmp);
        let p = |q: f64| format!("{:.*}", self.prec, percentile(&s, q));
        // Working rule 9: a 99th percentile over five samples is the maximum
        // wearing a different name. Print the honest dash instead.
        let tail = |q: f64, need: usize| {
            if s.len() >= need {
                p(q)
            } else {
                "-".to_string()
            }
        };
        let note = match &self.note {
            Some(n) => format!(" note={n}"),
            None => String::new(),
        };
        println!(
            "BENCH scenario={} proto={} rtt_ms={} windows={} streams={} n={} \
             p50={} p99={} p999={} min={} max={} unit={}{}",
            self.scenario,
            self.proto,
            self.rtt_ms,
            self.windows,
            self.streams,
            s.len(),
            p(0.50),
            tail(0.99, 100),
            tail(0.999, 1000),
            format_args!("{:.*}", self.prec, s[0]),
            format_args!("{:.*}", self.prec, s[s.len() - 1]),
            self.unit,
            note,
        );
    }
}

// ══════════════════════════════════════════════════════════════════════
// THE DELAY RELAY — its own OS thread, its own runtime
// ══════════════════════════════════════════════════════════════════════

/// What the benchmark thread asks the relay thread to build.
enum RelayCmd {
    /// A datagram-preserving UDP forwarder in front of `server`.
    Udp {
        server: SocketAddr,
        delay: Duration,
        reply: std::sync::mpsc::SyncSender<SocketAddr>,
    },
    /// A byte-stream TCP forwarder in front of `server`.
    Tcp {
        server: SocketAddr,
        delay: Duration,
        reply: std::sync::mpsc::SyncSender<SocketAddr>,
    },
}

/// A handle onto the relay thread.
///
/// Dropping it closes the command channel, which ends the relay's event loop
/// and lets its thread exit.
struct Relay {
    tx: tokio::sync::mpsc::UnboundedSender<RelayCmd>,
}

impl Relay {
    /// Start the relay thread.
    fn start() -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RelayCmd>();
        std::thread::Builder::new()
            .name("bench-relay".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("the relay's own current-thread runtime");
                rt.block_on(async move {
                    while let Some(cmd) = rx.recv().await {
                        match cmd {
                            RelayCmd::Udp {
                                server,
                                delay,
                                reply,
                            } => {
                                let front = udp_relay(server, delay).await;
                                let _ = reply.send(front);
                            }
                            RelayCmd::Tcp {
                                server,
                                delay,
                                reply,
                            } => {
                                let front = tcp_relay(server, delay).await;
                                let _ = reply.send(front);
                            }
                        }
                    }
                });
            })
            .expect("spawn the relay thread");
        Self { tx }
    }

    /// Build a relay and block until it reports its front address.
    ///
    /// This blocks the benchmark thread, which is an async context. That is
    /// deliberate and safe: the relay lives on a different thread with a
    /// different runtime, owes this call nothing but a `bind`, and this only
    /// ever runs during a cell's setup — never inside a timed section.
    fn front(
        &self,
        cmd: impl FnOnce(std::sync::mpsc::SyncSender<SocketAddr>) -> RelayCmd,
    ) -> SocketAddr {
        let (reply, wait) = std::sync::mpsc::sync_channel(1);
        self.tx.send(cmd(reply)).expect("the relay thread is alive");
        wait.recv().expect("the relay reported its front address")
    }

    fn udp(&self, server: SocketAddr, delay: Duration) -> SocketAddr {
        self.front(|reply| RelayCmd::Udp {
            server,
            delay,
            reply,
        })
    }

    fn tcp(&self, server: SocketAddr, delay: Duration) -> SocketAddr {
        self.front(|reply| RelayCmd::Tcp {
            server,
            delay,
            reply,
        })
    }
}

/// A datagram-preserving UDP forwarder. Returns the address the client dials.
///
/// Two sockets, two directions, one queue per direction: FIFO, lossless,
/// one-way delay `delay`. The client's address is learned from the first
/// datagram it sends — there is exactly one client per relay, so no
/// demultiplexing is needed and none is done.
async fn udp_relay(server: SocketAddr, delay: Duration) -> SocketAddr {
    let front = Arc::new(
        UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind the relay's client-facing socket"),
    );
    let back = Arc::new(
        UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind the relay's server-facing socket"),
    );
    let front_addr = front.local_addr().expect("the relay's front address");
    let client: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));

    // client → server
    {
        let (q_tx, mut q_rx) =
            tokio::sync::mpsc::unbounded_channel::<(tokio::time::Instant, Vec<u8>)>();
        let (front, back, client) = (Arc::clone(&front), Arc::clone(&back), Arc::clone(&client));
        tokio::spawn(async move {
            let mut buf = vec![0u8; UDP_RELAY_BUF];
            while let Ok((n, from)) = front.recv_from(&mut buf).await {
                *client.lock().expect("relay address cell") = Some(from);
                if q_tx
                    .send((tokio::time::Instant::now() + delay, buf[..n].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Some((due, data)) = q_rx.recv().await {
                tokio::time::sleep_until(due).await;
                if back.send_to(&data, server).await.is_err() {
                    break;
                }
            }
        });
    }

    // server → client
    {
        let (q_tx, mut q_rx) =
            tokio::sync::mpsc::unbounded_channel::<(tokio::time::Instant, Vec<u8>)>();
        tokio::spawn(async move {
            let mut buf = vec![0u8; UDP_RELAY_BUF];
            while let Ok((n, _from)) = back.recv_from(&mut buf).await {
                if q_tx
                    .send((tokio::time::Instant::now() + delay, buf[..n].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Some((due, data)) = q_rx.recv().await {
                tokio::time::sleep_until(due).await;
                // A reply before the client has ever spoken has nowhere to go.
                // It cannot happen here — the client is always the initiator —
                // and is dropped rather than guessed at.
                let to = *client.lock().expect("relay address cell");
                let Some(to) = to else { continue };
                if front.send_to(&data, to).await.is_err() {
                    break;
                }
            }
        });
    }

    front_addr
}

/// A byte-stream TCP forwarder. Returns the address the client dials.
///
/// One upstream connection per accepted client connection, and one pump per
/// direction. `TCP_NODELAY` is set on both of the relay's own sockets so the
/// relay adds delay and nothing else.
async fn tcp_relay(server: SocketAddr, delay: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the relay's listener");
    let front_addr = listener.local_addr().expect("the relay's front address");
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(upstream) = TcpStream::connect(server).await else {
                break;
            };
            let _ = client.set_nodelay(true);
            let _ = upstream.set_nodelay(true);
            let (cr, cw) = client.into_split();
            let (ur, uw) = upstream.into_split();
            tokio::spawn(tcp_pump(cr, uw, delay));
            tokio::spawn(tcp_pump(ur, cw, delay));
        }
    });
    front_addr
}

/// Forward one direction of a TCP connection, delayed by `delay`, in chunks of
/// at most [`TCP_RELAY_CHUNK`] bytes.
async fn tcp_pump(mut r: OwnedReadHalf, mut w: OwnedWriteHalf, delay: Duration) {
    let (q_tx, mut q_rx) =
        tokio::sync::mpsc::channel::<(tokio::time::Instant, Vec<u8>)>(TCP_RELAY_DEPTH);
    let writer = tokio::spawn(async move {
        while let Some((due, data)) = q_rx.recv().await {
            tokio::time::sleep_until(due).await;
            if w.write_all(&data).await.is_err() {
                break;
            }
        }
        // The peer's EOF has drained through; pass it on rather than leaving
        // the far side blocked on a read that will never complete.
        let _ = w.shutdown().await;
    });
    let mut buf = vec![0u8; TCP_RELAY_CHUNK];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // `send` here *awaits* on a full queue, which is the
                // backpressure that keeps the proxy from buffering without
                // limit. It is also the only place either relay applies any.
                if q_tx
                    .send((tokio::time::Instant::now() + delay, buf[..n].to_vec()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }
    drop(q_tx);
    let _ = writer.await;
}

// ══════════════════════════════════════════════════════════════════════
// THE STEADY-STATE METER
// ══════════════════════════════════════════════════════════════════════

/// Counts payload bytes into back-to-back timed windows, after a ramp.
///
/// Shared by every bulk cell and by every reader inside a multiplexed cell —
/// which is what makes the multiplexing figure an *aggregate* rather than a
/// sum of separately-timed streams.
struct BulkMeter {
    ramp_end: Instant,
    window: Duration,
    wanted: usize,
    /// `None` until the first read after the ramp arms the first window.
    armed: Option<Instant>,
    bytes: u64,
    /// Every byte counted into a closed window, so the wire counters can be
    /// divided by the payload they actually carried.
    total: u64,
    out: Vec<f64>,
}

impl BulkMeter {
    fn new(ramp: Duration, window: Duration, wanted: usize) -> Self {
        Self {
            ramp_end: Instant::now() + ramp,
            window,
            wanted,
            armed: None,
            bytes: 0,
            total: 0,
            out: Vec::with_capacity(wanted),
        }
    }

    /// Fold `n` freshly-received payload bytes in. Returns `true` once every
    /// window is closed and the transfer can stop.
    fn record(&mut self, n: usize) -> bool {
        if self.out.len() >= self.wanted {
            return true;
        }
        let now = Instant::now();
        if now < self.ramp_end {
            return false;
        }
        let Some(start) = self.armed else {
            // The ramp has just ended. Arm the first window here rather than at
            // `ramp_end`: the bytes of *this* read were in flight during the
            // ramp and charging them to a window that started later would
            // inflate it.
            self.armed = Some(now);
            self.bytes = 0;
            return false;
        };
        self.bytes += n as u64;
        let elapsed = now.duration_since(start);
        if elapsed >= self.window {
            self.out
                .push((self.bytes as f64 / MIB) / elapsed.as_secs_f64());
            self.total += self.bytes;
            self.armed = Some(now);
            self.bytes = 0;
        }
        self.out.len() >= self.wanted
    }
}

/// The shared cells every bulk cell drives: the meter, the flag its writers
/// watch, the cell's cause of death, and the optional wire seams sampled at
/// the timed section's edges.
#[derive(Clone)]
struct BulkState {
    meter: Rc<RefCell<BulkMeter>>,
    done: Rc<Cell<bool>>,
    /// Why this cell stopped early, if it did — see [`BulkState::fail`].
    ///
    /// **`done` is set only by the meter, so a cell whose connection dies
    /// never sets it.** Every task that watches `done` must watch this too or
    /// it idles forever: round 42 spent six minutes at 0 % CPU watching a
    /// `while !done.get()` loop whose connection had been killed by a
    /// `PROTOCOL_VIOLATION` ten seconds in, and the failure presented as a
    /// hang rather than as the protocol error it was.
    failure: Rc<RefCell<Option<String>>>,
    /// `(sender, receiver)` counters, absent for the TCP cells where the wire
    /// belongs to the kernel and no seam is available.
    wires: Option<(Rc<WireCounters>, Rc<WireCounters>)>,
    /// Snapshots taken exactly at the first and last counted byte, so what they
    /// bracket is the timed section and not the ramp.
    edges: Rc<Cell<Edges>>,
}

/// The four [`WireCounters::snapshot`] readings that bracket a cell's timed
/// section: sender and receiver, at the first counted byte and at the last.
#[derive(Clone, Copy, Default)]
struct Edges {
    start_a: [u64; 4],
    start_b: [u64; 4],
    end_a: [u64; 4],
    end_b: [u64; 4],
}

impl BulkState {
    fn new(samples: usize) -> Self {
        Self {
            meter: Rc::new(RefCell::new(BulkMeter::new(
                BULK_RAMP,
                BULK_WINDOW,
                samples,
            ))),
            done: Rc::new(Cell::new(false)),
            failure: Rc::new(RefCell::new(None)),
            wires: None,
            edges: Rc::new(Cell::new(Edges::default())),
        }
    }

    fn with_wires(mut self, a: &Rc<WireCounters>, b: &Rc<WireCounters>) -> Self {
        self.wires = Some((Rc::clone(a), Rc::clone(b)));
        self
    }

    fn snapshot(&self) -> ([u64; 4], [u64; 4]) {
        match &self.wires {
            Some((a, b)) => (a.snapshot(), b.snapshot()),
            None => ([0; 4], [0; 4]),
        }
    }

    fn record(&self, n: usize) {
        // The borrow is held across no `.await`.
        let mut m = self.meter.borrow_mut();
        let was_armed = m.armed.is_some();
        let finished = m.record(n);
        drop(m);
        if !was_armed && self.meter.borrow().armed.is_some() {
            let (start_a, start_b) = self.snapshot();
            let edges = self.edges.get();
            self.edges.set(Edges {
                start_a,
                start_b,
                ..edges
            });
        }
        if finished && !self.done.get() {
            let (end_a, end_b) = self.snapshot();
            let edges = self.edges.get();
            self.edges.set(Edges {
                end_a,
                end_b,
                ..edges
            });
            self.done.set(true);
        }
    }

    /// Record why this cell stopped early. The **first** cause wins: a dead
    /// connection makes every task fail, and the first one to notice holds
    /// the diagnosis closest to the cause.
    fn fail(&self, why: impl Into<String>) {
        let mut slot = self.failure.borrow_mut();
        if slot.is_none() {
            *slot = Some(why.into());
        }
    }

    /// Whether the cell has already died. Every loop that watches
    /// [`BulkState::done`] watches this as well, so a task cannot outlive
    /// the connection it is driving.
    fn failed(&self) -> bool {
        self.failure.borrow().is_some()
    }

    /// The per-window figures — **or a panic**.
    ///
    /// A cell that died mid-run used to return whatever the meter had
    /// collected, and the `BENCH` line said `n=2` with nothing to say that
    /// anything had gone wrong. These lines are read to decide whether a
    /// change worked; a short one is not a slow result, it is **no result**,
    /// and it must not be mistaken for a measurement. Fail loudly instead.
    fn finish(self) -> Vec<f64> {
        let failure = self.failure.borrow().clone();
        let out = self.meter.borrow().out.clone();
        let wanted = self.meter.borrow().wanted;
        assert!(
            failure.is_none(),
            "the cell died after {} of {wanted} windows: {}",
            out.len(),
            failure.unwrap_or_default()
        );
        assert!(
            out.len() >= wanted,
            "the cell produced {} of {wanted} windows and reported no cause — \
             the transfer ended without the meter being satisfied",
            out.len()
        );
        out
    }

    /// A comma-separated note describing what the wire carried for the payload
    /// the timed section delivered. `None` when there is no seam to read.
    ///
    /// - `amp` — sender wire bytes ÷ payload bytes. At rest this is a little
    ///   over 1 (§3.1 header, AEAD tag, frame headers); well above it means
    ///   the same payload was put on the wire more than once.
    /// - `loss` — 1 − (datagrams the receiver's socket saw ÷ datagrams the
    ///   sender's socket emitted). The relay does not drop, so a non-zero value
    ///   here is the kernel dropping on a full socket buffer.
    fn wire_note(&self) -> Option<String> {
        self.wires.as_ref()?;
        let e = self.edges.get();
        let payload = self.meter.borrow().total;
        if payload == 0 {
            return None;
        }
        let out_dgrams = e.end_a[0].saturating_sub(e.start_a[0]);
        let out_bytes = e.end_a[1].saturating_sub(e.start_a[1]);
        let in_dgrams = e.end_b[2].saturating_sub(e.start_b[2]);
        let ack_dgrams = e.end_b[0].saturating_sub(e.start_b[0]);
        let loss = if out_dgrams == 0 {
            0.0
        } else {
            1.0 - (in_dgrams as f64 / out_dgrams as f64)
        };
        Some(format!(
            "amp={:.3},loss={:.4},dg_out={out_dgrams},dg_in={in_dgrams},ack_dg={ack_dgrams},mtu={:.0}",
            out_bytes as f64 / payload as f64,
            loss,
            if out_dgrams == 0 {
                0.0
            } else {
                out_bytes as f64 / out_dgrams as f64
            },
        ))
    }
}

// ══════════════════════════════════════════════════════════════════════
// SLITHER FIXTURE
// ══════════════════════════════════════════════════════════════════════

/// Which flow-control policy an endpoint is built with.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Windows {
    stream: u64,
    connection: u64,
    label: &'static str,
}

/// The ratified constants: 256 KiB per stream, 1 MiB per connection.
///
/// Named through `constants` rather than written out, so a ratification that
/// moved either value would move this benchmark's baseline with it rather than
/// silently reporting a raise as a default.
const WINDOWS_DEFAULT: Windows = Windows {
    stream: slither::constants::INITIAL_MAX_STREAM_DATA,
    connection: slither::constants::INITIAL_MAX_DATA,
    label: "default",
};

/// [`RAISED_STREAM`] per stream, [`RAISED_CONN`] per connection.
const WINDOWS_RAISED: Windows = Windows {
    stream: RAISED_STREAM,
    connection: RAISED_CONN,
    label: "8Mi/16Mi",
};

impl Windows {
    /// `with_flow_windows` even for the default pair: the call is legal at
    /// exactly the ratified values and produces the identical `Config` (the
    /// one-shot announcement is armed only on `window > ratified`), so one code
    /// path covers the whole ladder.
    fn config(self) -> Config {
        Config::new()
            .with_flow_windows(self.stream, self.connection)
            .expect("a raise within the varint bound, stream <= connection")
    }
}

// ══════════════════════════════════════════════════════════════════════
// THE COUNTING WIRE
// ══════════════════════════════════════════════════════════════════════

/// Datagram and byte counts observed at one endpoint's [`Wire`] seam.
///
/// `Cell` and not an atomic: the driver is a single `!Send` actor (§16.3) and
/// these run on its thread, so there is nothing to synchronise and a lock would
/// add cost to the path being measured. `examples/audit_udp.rs` establishes the
/// same shape for the same reason.
#[derive(Default)]
struct WireCounters {
    out_dgrams: Cell<u64>,
    out_bytes: Cell<u64>,
    in_dgrams: Cell<u64>,
    in_bytes: Cell<u64>,
}

impl WireCounters {
    fn snapshot(&self) -> [u64; 4] {
        [
            self.out_dgrams.get(),
            self.out_bytes.get(),
            self.in_dgrams.get(),
            self.in_bytes.get(),
        ]
    }
}

/// A `tokio::net::UdpSocket` that counts what crosses it.
///
/// **No production code is instrumented**: `Wire` is a public seam (§16.3
/// normative property 1 — *"the application supplies it"*) and this lives here.
/// It exists to make one question answerable by measurement rather than
/// argument: when a configuration change moves throughput, is the wire carrying
/// *more bytes for the same payload* (retransmission) or the *same bytes more
/// slowly* (CPU)? Those two have opposite fixes and are indistinguishable from
/// a MiB/s figure alone.
struct CountingWire {
    inner: UdpSocket,
    counters: Rc<WireCounters>,
}

impl Wire for CountingWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        let result = self.inner.send_to(buf, addr).await;
        if let Ok(n) = &result {
            let c = &self.counters;
            c.out_dgrams.set(c.out_dgrams.get() + 1);
            c.out_bytes.set(c.out_bytes.get() + *n as u64);
        }
        result
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let result = self.inner.recv_from(buf).await;
        if let Ok((n, _)) = &result {
            let c = &self.counters;
            c.in_dgrams.set(c.in_dgrams.get() + 1);
            c.in_bytes.set(c.in_bytes.get() + *n as u64);
        }
        result
    }
}

/// One established slither connection over real UDP, plus the endpoints that
/// must stay alive to keep its drivers running (§16.3: dropping an endpoint's
/// last handle stops its driver, which would stall a cell rather than fail it).
struct SlitherLink {
    _ep_a: Endpoint<Id>,
    _ep_b: Endpoint<Id>,
    ca: Conn,
    cb: Conn,
    /// The sender's seam: `out` is everything A puts on the wire, `in` is the
    /// acks coming back.
    wire_a: Rc<WireCounters>,
    /// The receiver's seam. `in` here against `out` there is the delivery
    /// ratio, so datagrams lost to a full socket buffer are visible without
    /// any protocol-internal counter.
    wire_b: Rc<WireCounters>,
}

/// Bring up one connection. `relay` is `Some` when the initiator should dial a
/// delayed path instead of the responder's own socket.
///
/// Both endpoints get the same `windows`. The *receiver's* policy is the
/// load-bearing one for a one-way transfer (the knob is a receive policy and is
/// not negotiated), but a benchmark that raised only one side would be
/// measuring a configuration no consumer would deploy.
async fn slither_link(
    relay: Option<(&Relay, Duration)>,
    windows: Windows,
    seed: u8,
) -> SlitherLink {
    let sock_a = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind the initiator socket");
    let sock_b = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("bind the responder socket");
    let addr_b = sock_b.local_addr().expect("the responder's bound address");
    let dial = match relay {
        Some((r, d)) => r.udp(addr_b, d),
        None => addr_b,
    };

    // Seeded rather than OS-seeded so a run is replayable. These keys protect
    // nothing; the warning on `SoftwareIdentity::generate` is about production.
    let id_a: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([seed ^ 0xA1; 32]))
        .expect("a seeded ChaCha20 stream yields a valid P-256 scalar");
    let id_b: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([seed ^ 0xB2; 32]))
        .expect("a seeded ChaCha20 stream yields a valid P-256 scalar");
    let pk_b = *Identity::public_static(&id_b);

    let wire_a = Rc::new(WireCounters::default());
    let wire_b = Rc::new(WireCounters::default());
    let ep_a: Endpoint<Id> = Endpoint::builder()
        .identity(id_a)
        .wire(CountingWire {
            inner: sock_a,
            counters: Rc::clone(&wire_a),
        })
        .config(windows.config())
        .rng_seed([seed ^ 0x11; 32])
        .build();
    let ep_b: Endpoint<Id> = Endpoint::builder()
        .identity(id_b)
        .wire(CountingWire {
            inner: sock_b,
            counters: Rc::clone(&wire_b),
        })
        .config(windows.config())
        .rng_seed([seed ^ 0x22; 32])
        .build();

    let dial_fut = async {
        ep_a.connect(dial, pk_b)
            .expect("mint the pending connection")
            .await
            .expect("the dial completed")
    };
    let accept_fut = async {
        let intro = ep_b.accept().await.expect("an introduction arrived");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await.expect("accept")
    };
    let (ca, cb) = tokio::join!(dial_fut, accept_fut);

    SlitherLink {
        _ep_a: ep_a,
        _ep_b: ep_b,
        ca,
        cb,
        wire_a,
        wire_b,
    }
}

/// Write every byte of `buf`, looping over partial writes; `false` once the
/// stream can take no more. `write` is a partial verb (§16.4) and a benchmark
/// that ignored that would move less than it reported.
/// The error is **carried out**, not discarded: it is the diagnosis a dead
/// cell reports, and swallowing it is what made a `PROTOCOL_VIOLATION` look
/// like a hang.
async fn write_all(tx: &mut SendStream<ReferenceSuite>, buf: &[u8]) -> Result<(), String> {
    let mut off = 0;
    while off < buf.len() {
        match tx.write(&buf[off..]).await {
            Ok(0) => panic!("write returned 0 for a non-empty buffer"),
            Ok(n) => off += n,
            Err(e) => return Err(format!("write failed after {off} B of a chunk: {e}")),
        }
    }
    Ok(())
}

/// Open `count` uni streams from `ca` and accept them on `cb`.
///
/// Each stream is primed with one byte: `open_uni` allocates the id locally and
/// the peer only learns the stream exists when a frame carrying it arrives, so
/// `accept_uni` would otherwise never resolve. The priming byte is also what
/// makes the peer's configured stream-window raise reachable — §8.4 makes a
/// MAX_STREAM_DATA for a stream the receiver has not seen inert, so the raise
/// is announced on the first STREAM frame and not at open.
async fn uni_streams(
    ca: &Conn,
    cb: &Conn,
    count: usize,
) -> (
    Vec<SendStream<ReferenceSuite>>,
    Vec<RecvStream<ReferenceSuite>>,
) {
    let mut txs = Vec::with_capacity(count);
    for _ in 0..count {
        let mut tx = ca.open_uni().await.expect("open_uni");
        tx.write(b"\0").await.expect("priming write");
        txs.push(tx);
    }
    let mut rxs = Vec::with_capacity(count);
    let mut prime = [0u8; 1];
    for _ in 0..count {
        let mut rx = cb.accept_uni().await.expect("accept_uni");
        let n = rx
            .read(&mut prime)
            .await
            .expect("read the priming byte")
            .expect("the priming byte, not end of stream");
        assert_eq!(n, 1, "the priming read returned {n} bytes, not 1");
        rxs.push(rx);
    }
    (txs, rxs)
}

// ══════════════════════════════════════════════════════════════════════
// SCENARIOS 2, 3, 5 — BULK
// ══════════════════════════════════════════════════════════════════════

/// Move payload over `streams` concurrent uni streams until the meter is
/// satisfied, and return one MiB/s figure per timed window.
async fn slither_bulk(
    relay: Option<(&Relay, Duration)>,
    windows: Windows,
    streams: usize,
    seed: u8,
) -> (Vec<f64>, Option<String>) {
    let link = slither_link(relay, windows, seed).await;
    let (txs, rxs) = uni_streams(&link.ca, &link.cb, streams).await;
    let chunk: Rc<Vec<u8>> = Rc::new((0..CHUNK).map(|i| (i % 251) as u8).collect());

    // The meter's ramp starts here, not at connection birth: the handshake and
    // the stream priming are setup, not transfer.
    let state = BulkState::new(BULK_SAMPLES).with_wires(&link.wire_a, &link.wire_b);

    let mut tasks = Vec::with_capacity(streams * 2);
    for mut tx in txs {
        let (state, chunk) = (state.clone(), Rc::clone(&chunk));
        tasks.push(tokio::task::spawn_local(async move {
            // `failed()` as well as `done`: `done` is the meter's flag and a
            // dead cell never satisfies the meter.
            while !state.done.get() && !state.failed() {
                if let Err(why) = write_all(&mut tx, &chunk).await {
                    state.fail(why);
                    return;
                }
            }
            // A clean FIN rather than a timeout on the read side: it costs the
            // reader nothing and keeps every `read` on the hot path free of the
            // timer registration that a `timeout` wrapper would add.
            let _ = tx.finish().await;
        }));
    }
    for mut rx in rxs {
        let state = state.clone();
        tasks.push(tokio::task::spawn_local(async move {
            let mut scratch = vec![0u8; CHUNK];
            loop {
                match rx.read(&mut scratch).await {
                    Ok(Some(n)) => state.record(n),
                    // The FIN the writer sends once the meter is satisfied.
                    Ok(None) => return,
                    Err(e) => return state.fail(format!("read failed: {e}")),
                }
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    drop(link);
    let note = state.wire_note();
    (state.finish(), note)
}

/// The same shape over one raw TCP connection.
///
/// `nodelay` is the *endpoint* setting; the relay always sets its own.
async fn tcp_bulk(relay: Option<(&Relay, Duration)>, nodelay: bool) -> Vec<f64> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the TCP server");
    let server_addr = listener.local_addr().expect("the TCP server's address");
    let dial = match relay {
        Some((r, d)) => r.tcp(server_addr, d),
        None => server_addr,
    };

    let state = BulkState::new(BULK_SAMPLES);
    let server = {
        let state = state.clone();
        tokio::task::spawn_local(async move {
            let (mut sock, _) = listener.accept().await.expect("accept the TCP client");
            if nodelay {
                sock.set_nodelay(true).expect("TCP_NODELAY on the server");
            }
            let mut scratch = vec![0u8; CHUNK];
            loop {
                match sock.read(&mut scratch).await {
                    Ok(0) => break,
                    Ok(n) => state.record(n),
                    Err(e) => return state.fail(format!("TCP read failed: {e}")),
                }
            }
        })
    };

    let mut client = TcpStream::connect(dial)
        .await
        .expect("connect the TCP client");
    if nodelay {
        client.set_nodelay(true).expect("TCP_NODELAY on the client");
    }
    let chunk: Vec<u8> = (0..CHUNK).map(|i| (i % 251) as u8).collect();
    while !state.done.get() && !state.failed() {
        if let Err(e) = client.write_all(&chunk).await {
            state.fail(format!("TCP write failed: {e}"));
            break;
        }
    }
    let _ = client.shutdown().await;
    let _ = server.await;
    state.finish()
}

// ══════════════════════════════════════════════════════════════════════
// SCENARIO 1 — ESTABLISHMENT
// ══════════════════════════════════════════════════════════════════════

/// Dial, then round-trip one application byte. Returns the elapsed time.
///
/// `t0` is taken immediately before the dial and the clock is read immediately
/// after the echoed byte lands, so the figure is *dial to first application
/// byte back* — the quantity a consumer waits on, not a handshake
/// microbenchmark.
async fn slither_establish_once(relay: Option<(&Relay, Duration)>, seed: u8) -> Duration {
    let sock_a = UdpSocket::bind("127.0.0.1:0").await.expect("bind a");
    let sock_b = UdpSocket::bind("127.0.0.1:0").await.expect("bind b");
    let addr_b = sock_b.local_addr().expect("b's address");
    let dial = match relay {
        Some((r, d)) => r.udp(addr_b, d),
        None => addr_b,
    };
    let id_a: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([seed ^ 0xA1; 32]))
        .expect("a valid P-256 scalar");
    let id_b: Id = SoftwareIdentity::generate(ChaCha20Rng::from_seed([seed ^ 0xB2; 32]))
        .expect("a valid P-256 scalar");
    let pk_b = *Identity::public_static(&id_b);
    let ep_a: Endpoint<Id> = Endpoint::builder()
        .identity(id_a)
        .wire(sock_a)
        .rng_seed([seed ^ 0x11; 32])
        .build();
    let ep_b: Endpoint<Id> = Endpoint::builder()
        .identity(id_b)
        .wire(sock_b)
        .rng_seed([seed ^ 0x22; 32])
        .build();

    let t0 = Instant::now();
    let dial_fut = async {
        ep_a.connect(dial, pk_b)
            .expect("mint the pending connection")
            .await
            .expect("the dial completed")
    };
    let accept_fut = async {
        let intro = ep_b.accept().await.expect("an introduction");
        let claimed = intro.read_identity().await.expect("read_identity");
        let proven = claimed.authenticate().await.expect("authenticate");
        proven.accept().await.expect("accept")
    };
    let (ca, cb) = tokio::join!(dial_fut, accept_fut);

    let echo = tokio::task::spawn_local(async move {
        let bi = cb.accept_bi().await.expect("accept_bi");
        let (mut tx, mut rx) = bi.split();
        let mut byte = [0u8; 1];
        let n = rx
            .read(&mut byte)
            .await
            .expect("read the first byte")
            .expect("a byte, not end of stream");
        assert_eq!(n, 1, "the echo peer read {n} bytes, not 1");
        tx.write(&byte).await.expect("echo the byte");
        // Returned so the connection outlives the echo it is serving.
        (tx, rx, cb)
    });

    let bi = ca.open_bi().await.expect("open_bi");
    let (mut tx, mut rx) = bi.split();
    tx.write(b"\x2a").await.expect("the first application byte");
    let mut back = [0u8; 1];
    let n = rx
        .read(&mut back)
        .await
        .expect("read the echo")
        .expect("an echoed byte, not end of stream");
    let elapsed = t0.elapsed();
    assert_eq!(n, 1, "the echo was {n} bytes, not 1");
    assert_eq!(back[0], 0x2a, "the echo did not match the request");

    echo.abort();
    drop(tx);
    drop(rx);
    drop(ca);
    drop(ep_a);
    drop(ep_b);
    elapsed
}

/// The same quantity for raw TCP: `connect`, write one byte, read it back.
async fn tcp_establish_once(relay: Option<(&Relay, Duration)>) -> Duration {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let server_addr = listener.local_addr().expect("server address");
    let dial = match relay {
        Some((r, d)) => r.tcp(server_addr, d),
        None => server_addr,
    };
    let echo = tokio::task::spawn_local(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        sock.set_nodelay(true).expect("TCP_NODELAY on the server");
        let mut byte = [0u8; 1];
        sock.read_exact(&mut byte).await.expect("read one byte");
        sock.write_all(&byte).await.expect("echo one byte");
        sock
    });

    let t0 = Instant::now();
    let mut client = TcpStream::connect(dial).await.expect("connect");
    client.set_nodelay(true).expect("TCP_NODELAY on the client");
    client.write_all(b"\x2a").await.expect("the first byte");
    let mut back = [0u8; 1];
    client.read_exact(&mut back).await.expect("read the echo");
    let elapsed = t0.elapsed();
    assert_eq!(back[0], 0x2a, "the echo did not match the request");
    echo.abort();
    elapsed
}

// ══════════════════════════════════════════════════════════════════════
// SCENARIO 4 — LATENCY PING-PONG
// ══════════════════════════════════════════════════════════════════════

/// Unpipelined 64-byte round trips over slither's unreliable datagram path.
///
/// Datagrams and not a stream: this is the latency of the packet layer, and a
/// stream would add reassembly and ack-driven wakeups that the bulk scenarios
/// already measure. §11.1 promises no delivery, so the echo is checked against
/// the request rather than merely counted — a lost datagram would otherwise be
/// silently replaced by the next one and report a shorter interval.
async fn slither_pingpong(relay: Option<(&Relay, Duration)>) -> Vec<f64> {
    // Destructured rather than borrowed: `Connection` is not `Clone`, and the
    // echo peer has to *own* `cb` to outlive this frame. `_ep_a`/`_ep_b` stay
    // bound for the whole probe — dropping an endpoint's last handle stops its
    // driver (§16.3), which would stall the ping-pong rather than fail it.
    let SlitherLink {
        _ep_a,
        _ep_b,
        ca,
        cb,
        ..
    } = slither_link(relay, WINDOWS_DEFAULT, 0x5A).await;
    let echo = tokio::task::spawn_local(async move {
        while let Ok(d) = cb.recv_datagram().await {
            let _ = cb.send_datagram(&d);
        }
    });

    let mut payload = [0u8; PING_PAYLOAD];
    let mut out = Vec::with_capacity(PING_ITERS);
    for i in 0..(PING_WARMUP + PING_ITERS) as u64 {
        payload[..8].copy_from_slice(&i.to_le_bytes());
        let t0 = Instant::now();
        ca.send_datagram(&payload).expect("queue the datagram");
        let got = tokio::time::timeout(Duration::from_secs(10), ca.recv_datagram())
            .await
            .unwrap_or_else(|_| panic!("no echo for datagram {i} within 10 s — the run is broken"))
            .expect("the connection outlived the probe");
        let dt = t0.elapsed();
        assert_eq!(
            &got[..8],
            &i.to_le_bytes(),
            "the echo did not match the request — the ping-pong lost its lockstep"
        );
        if i >= PING_WARMUP as u64 {
            out.push(dt.as_secs_f64() * 1e6);
        }
    }
    echo.abort();
    drop(ca);
    drop(_ep_a);
    drop(_ep_b);
    out
}

/// The same round trip over a `TCP_NODELAY` connection.
async fn tcp_pingpong(relay: Option<(&Relay, Duration)>) -> Vec<f64> {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let server_addr = listener.local_addr().expect("server address");
    let dial = match relay {
        Some((r, d)) => r.tcp(server_addr, d),
        None => server_addr,
    };
    let echo = tokio::task::spawn_local(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        sock.set_nodelay(true).expect("TCP_NODELAY on the server");
        let mut buf = [0u8; PING_PAYLOAD];
        while sock.read_exact(&mut buf).await.is_ok() {
            if sock.write_all(&buf).await.is_err() {
                break;
            }
        }
    });

    let mut client = TcpStream::connect(dial).await.expect("connect");
    client.set_nodelay(true).expect("TCP_NODELAY on the client");
    let mut payload = [0u8; PING_PAYLOAD];
    let mut back = [0u8; PING_PAYLOAD];
    let mut out = Vec::with_capacity(PING_ITERS);
    for i in 0..(PING_WARMUP + PING_ITERS) as u64 {
        payload[..8].copy_from_slice(&i.to_le_bytes());
        let t0 = Instant::now();
        client.write_all(&payload).await.expect("write the ping");
        client.read_exact(&mut back).await.expect("read the pong");
        let dt = t0.elapsed();
        assert_eq!(
            &back[..8],
            &i.to_le_bytes(),
            "the echo did not match the request"
        );
        if i >= PING_WARMUP as u64 {
            out.push(dt.as_secs_f64() * 1e6);
        }
    }
    echo.abort();
    out
}

// ══════════════════════════════════════════════════════════════════════
// THE MATRIX
// ══════════════════════════════════════════════════════════════════════

/// The one-way delay a given RTT needs, or `None` for the direct path.
fn one_way(rtt_ms: u64) -> Option<Duration> {
    (rtt_ms > 0).then(|| Duration::from_micros(rtt_ms * 500))
}

fn scenario_establish(relay: &Relay) {
    println!("# establishment — dial to first application byte echoed back");
    for (rtt_ms, samples) in [(0u64, 20usize), (20, 10), (100, 10)] {
        let via = one_way(rtt_ms).map(|d| (relay, d));
        let mut slither_ms = Vec::with_capacity(samples);
        let mut tcp_ms = Vec::with_capacity(samples);
        block_on(async {
            for i in 0..samples {
                let d = slither_establish_once(via, i as u8).await;
                slither_ms.push(d.as_secs_f64() * 1e3);
            }
            for _ in 0..samples {
                let d = tcp_establish_once(via).await;
                tcp_ms.push(d.as_secs_f64() * 1e3);
            }
        });
        Row {
            scenario: "establish",
            proto: "slither",
            rtt_ms,
            windows: "default",
            streams: 1,
            unit: "ms",
            prec: 3,
            samples: slither_ms,
            note: None,
        }
        .emit();
        Row {
            scenario: "establish",
            proto: "tcp",
            rtt_ms,
            windows: "n/a",
            streams: 1,
            unit: "ms",
            prec: 3,
            samples: tcp_ms,
            // The relay answers the SYN locally, so this omits one RTT. See
            // the module docs; the correction is arithmetic, not measured.
            note: (rtt_ms > 0).then(|| "proxy-terminated,missing-1-rtt".to_string()),
        }
        .emit();
    }
    println!();
}

fn scenario_bulk(relay: &Relay, rtts: &[u64], tag: &'static str) {
    println!("# {tag} — one stream, steady state");
    for &rtt_ms in rtts {
        let via = one_way(rtt_ms).map(|d| (relay, d));
        for windows in [WINDOWS_DEFAULT, WINDOWS_RAISED] {
            let (samples, note) = block_on(slither_bulk(via, windows, 1, 0x30 ^ rtt_ms as u8));
            Row {
                scenario: tag,
                proto: "slither",
                rtt_ms,
                windows: windows.label,
                streams: 1,
                unit: "MiB/s",
                prec: 2,
                samples,
                note,
            }
            .emit();
        }
        let samples = block_on(tcp_bulk(via, false));
        Row {
            scenario: tag,
            proto: "tcp",
            rtt_ms,
            windows: "kernel-default",
            streams: 1,
            unit: "MiB/s",
            prec: 2,
            samples,
            // Neither TCP stack is on a delayed path — the relay terminates the
            // connection. An upper bound, not a comparable figure.
            note: (rtt_ms > 0).then(|| "proxy-terminated,not-a-delayed-path".to_string()),
        }
        .emit();
    }
    println!();
}

fn scenario_pingpong(relay: &Relay) {
    println!("# ping-pong — 64 B, unpipelined, TCP_NODELAY on");
    for rtt_ms in [0u64, 20] {
        let via = one_way(rtt_ms).map(|d| (relay, d));
        let samples = block_on(slither_pingpong(via));
        Row {
            scenario: "pingpong",
            proto: "slither",
            rtt_ms,
            windows: "default",
            streams: 1,
            unit: "us",
            prec: 1,
            samples,
            note: Some("datagram-path".into()),
        }
        .emit();
        let samples = block_on(tcp_pingpong(via));
        Row {
            scenario: "pingpong",
            proto: "tcp",
            rtt_ms,
            windows: "n/a",
            streams: 1,
            unit: "us",
            prec: 1,
            samples,
            note: Some("nodelay".into()),
        }
        .emit();
    }
    println!();
}

fn scenario_mux() {
    println!("# multiplexing — slither only, concurrent uni streams, clean loopback");
    for windows in [WINDOWS_DEFAULT, WINDOWS_RAISED] {
        for streams in MUX_STREAMS {
            let (samples, note) =
                block_on(slither_bulk(None, windows, streams, 0x70 ^ streams as u8));
            Row {
                scenario: "mux",
                proto: "slither",
                rtt_ms: 0,
                windows: windows.label,
                streams,
                unit: "MiB/s",
                prec: 2,
                samples,
                note: Some(match note {
                    Some(n) => format!("aggregate,{n}"),
                    None => "aggregate".to_string(),
                }),
            }
            .emit();
        }
    }
    println!();
}

/// The window ladder — the attribution run, and then the optimisation run.
///
/// One stream, [`SWEEP`]'s five configurations, at `rtt_ms`. Two distinct jobs:
///
/// - At **RTT 0** the point is not the throughput figures on their own but the
///   `amp` and `loss` fields beside them: they say whether a configuration that
///   moved the rate moved the *bytes on the wire* too, which is what separates
///   "the window caused retransmission" from "the window cost CPU".
/// - At **RTT 100 ms** the ladder is the only way to see that the knob has an
///   *optimum* rather than a direction. A window raise multiplies the
///   RTT-limited ceiling and divides the CPU-limited one, so two points can
///   show a gain while hiding the fact that a third would have been better.
fn scenario_sweep(relay: &Relay, rtt_ms: u64, tag: &'static str) {
    println!("# {tag} — slither only, window ladder, one stream, rtt={rtt_ms} ms");
    let via = one_way(rtt_ms).map(|d| (relay, d));
    for (i, windows) in SWEEP.into_iter().enumerate() {
        let (samples, note) = block_on(slither_bulk(via, windows, 1, 0xC0 ^ i as u8));
        Row {
            scenario: tag,
            proto: "slither",
            rtt_ms,
            windows: windows.label,
            streams: 1,
            unit: "MiB/s",
            prec: 2,
            samples,
            note,
        }
        .emit();
    }
    println!();
}

fn main() {
    let which = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .unwrap_or_else(|| "all".to_string());

    let relay = Relay::start();

    println!("bench_vs_tcp — slither versus raw kernel TCP");
    println!(
        "  chunk={CHUNK}B ramp={BULK_RAMP:?} window={BULK_WINDOW:?} samples={BULK_SAMPLES} \
         ping_iters={PING_ITERS} raised={RAISED_STREAM}/{RAISED_CONN}"
    );
    println!("  topology=one-thread/one-LocalSet/both-endpoints, relay=own-thread/own-runtime");
    println!();

    let run = |name: &str| which == "all" || which == name;
    let started = Instant::now();
    if run("establish") {
        scenario_establish(&relay);
    }
    if run("bulk") {
        scenario_bulk(&relay, &[0], "bulk");
    }
    if run("sweep") {
        scenario_sweep(&relay, 0, "sweep");
    }
    if run("bulk-rtt") {
        scenario_bulk(&relay, &[20, 50, 100], "bulk-rtt");
    }
    if run("sweep-rtt") {
        scenario_sweep(&relay, 100, "sweep-rtt");
    }
    if run("pingpong") {
        scenario_pingpong(&relay);
    }
    if run("mux") {
        scenario_mux();
    }
    println!("BENCH_DONE wall_s={:.1}", started.elapsed().as_secs_f64());
    drop(relay);
}

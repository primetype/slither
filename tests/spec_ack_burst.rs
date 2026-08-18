//! **Appendix B's two post-implementation validation obligations, run for
//! real — O53a (the ACK-loss-burst simulation) and O53b (the throughput
//! sanity check).**
//!
//! Both were written as *pre-ratification* gates: O53a says *"before
//! ratification hardens the fused choice"*, O53b *"before the §10.2
//! constants and `REASSEMBLY_CHUNKS_MAX` ratify"*. The wire ratified on
//! 2026/08/14 with neither run. **[Ruling 260]** re-scopes them from gates
//! on a decision already taken into **measured, pinned obligations**: the
//! numbers are taken here, recorded in the ruling, and left behind as
//! regression envelopes rather than as tight pins.
//!
//! # O53a — what is measured, and how
//!
//! The obligation: *"FlakyWire on the paused clock, sustained ACK-loss
//! bursts against the 2048-bit fused window under the every-2nd ACK
//! policy, quantifying spurious-retransmit and false-congestion-event
//! rates."*
//!
//! There is **no retransmit counter and no cwnd accessor** reachable from
//! an integration test — `congestion.rs`'s `cwnd`/`ssthresh` are
//! `pub(crate)` and `shell::Connection` exposes no stats verb — so both
//! rates are measured at the test level, from the fabric:
//!
//! * **Spurious-retransmit rate.** The transfer is one-way, A → B, and
//!   **A's path never loses a datagram**. So B holds every byte that
//!   leaves A the first time, and *every* forward datagram beyond the
//!   loss-free minimum carries data the receiver already held. A baseline
//!   run over an identical fabric with a loss-free return path supplies
//!   that minimum empirically, which is tighter than deriving it from the
//!   framing overhead. The rate is
//!   `(fwd_burst − fwd_baseline) / fwd_burst`, in datagrams and in wire
//!   bytes.
//!
//! * **False-congestion-event rate.** A cwnd collapse cannot be counted
//!   without the accessor, so it is **inferred from virtual time**, and is
//!   reported as an inference: the inflation
//!   `elapsed_burst / elapsed_baseline`. Read it as an **upper bound** on
//!   the false-congestion contribution, never as a count — the return path
//!   of a one-way transfer carries ACKs *and* flow-control credit
//!   (`MAX_DATA` / `MAX_STREAM_DATA`) in the same packets, so at a harsh
//!   severity the sender is legitimately credit-starved as well as
//!   feedback-starved, and a static 256 KiB window cannot open.
//!
//! The bursts are built with [`FlakyPolicy::drop_at`], which drops exactly
//! the named 0-based send indices with **no RNG involved**: [`BURSTS`]
//! consecutive return-path datagrams out of every [`PERIOD`], sustained
//! for the whole transfer. It is counter-proved — `FlakyWire::send_to`
//! writes the tap at step 4 and decides deliveries at step 5, so an index
//! the wire reached and the schedule names is an index that died. The
//! return path of a one-way transfer carries ACK-bearing packets (§12.4's
//! every-2nd ACK), so a burst on B's wire *is* an ACK-loss burst by
//! construction.
//!
//! ## What was measured, and what it says
//!
//! 2 MiB, 20 ms injected RTT, paused clock, seed `0x0053_A000`:
//!
//! | burst | ACK loss (counter-proved) | fwd datagrams | spurious (dgrams) | spurious (bytes) | virtual time | inflation |
//! |---|---|---|---|---|---|---|
//! | 0/8 (baseline) | 0.0 % (0 / 930) | 1838 | — | — | 310 ms | 1.000× |
//! | 2/8 | 25.1 % (234 / 933) | 1840 | 0.11 % | 0.04 % | 435 ms | 1.403× |
//! | 4/8 | 50.1 % (472 / 943) | 1846 | 0.43 % | 0.06 % | 924 ms | 2.981× |
//! | 6/8 | 75.1 % (750 / 999) | 1867 | **1.55 %** | 0.12 % | 10.386 s | 33.503× |
//!
//! **The numbers do not disappoint, and the mechanism is legible.** The
//! fused window is *cumulative*: `ack::derive` reads the replay window's
//! `ranges_desc()`, and on a lossless forward path that window is one
//! contiguous block, so `first_range = largest − start` covers every
//! counter ever received. Any **single** surviving ACK therefore carries
//! the receiver's complete state, and losing an ACK burst costs the sender
//! feedback *timing*, never feedback *information*. §19's range-tracker
//! ACK — the remedy the obligation held in reserve — is not needed.
//!
//! ## Working rule 9 — what the broken build scores
//!
//! *A bound is only a test if the degenerate case violates it.* The build
//! this obligation is really about is **a window that forgets ACKed
//! ranges**: an ACK that does not carry the cumulative range, so a packet
//! acknowledged only by a lost ACK is never acknowledged at all. One token
//! in `core::connection::ack::derive` produces it —
//! `first_range: largest - *first.start()` → `first_range: 0` — and it was
//! applied, measured, and reverted:
//!
//! | | fwd datagrams for 2 MiB | spurious vs the correct baseline | virtual time |
//! |---|---|---|---|
//! | correct build, **0 %** ACK loss | 1838 | — | 310 ms |
//! | forgetful ACK, **0 %** ACK loss | 3747 | **50.9 %** | 38.873 s (125×) |
//! | forgetful ACK, **25 %** ACK burst | — | — | **the connection dies**: `ConnectionLost(TimedOut)` |
//!
//! The broken build spends half its forward datagrams on data the receiver
//! already held *with no ACK loss at all*, and it cannot survive the
//! gentlest burst — §15's liveness timeout kills the connection before the
//! transfer finishes. [`SPURIOUS_CEILING`] sits two orders of magnitude
//! away from that.
//!
//! # O53b — what is pinned, and why it is machine-independent
//!
//! The obligation's original bar — *"within 20 % of quinn under its
//! shipped defaults"* — is not a bar a test can hold: it names a figure
//! from another crate on unstated hardware, and `benches/throughput.rs`
//! (ruling 247) is where slither's real throughput is measured. What can
//! be pinned, and is worth pinning, is the other half of the sentence:
//! **"with no stall"**.
//!
//! On tokio's paused clock virtual time advances *only* when the runtime
//! has nothing left to do but wait on a timer, so the same schedule
//! replays on every run and on every machine — the figure charges no
//! wall-clock work at all. Two bounds come out of that, and both are here:
//!
//! * **[`o53b_a_bulk_transfer_over_a_perfect_wire_never_waits_on_a_timer`]**
//!   — over a zero-delay fabric the measured cost of 2 MiB is **0 ns of
//!   virtual time**. Every microsecond on that clock would be a moment
//!   some part of the stack sat on a deadline instead of making progress,
//!   so the bound is *below the smallest timer in the stack*
//!   (`K_GRANULARITY`, 1 ms): not one timer may fire. As a throughput
//!   floor that is ≥ 2048 MiB/s of virtual-time throughput.
//! * **[`o53b_virtual_time_throughput_holds_its_floor`]** — over the
//!   20 ms-RTT fabric the transfer is window-limited and virtual time is
//!   meaningful: 2 MiB in **310 ms**, i.e. **6.45 MiB/s**, against the
//!   12.5 MiB/s that `INITIAL_MAX_STREAM_DATA / RTT` predicts. The floor
//!   is **half** the measured value.
//!
//! # Cost, and why O53a is a release-run test
//!
//! Measured on this commit, `--all-features`:
//!
//! | | debug | release |
//! |---|---|---|
//! | O53a (four 2 MiB transfers) | **9.1 s** | 0.3 s |
//! | both O53b tests | 0.8 s | < 0.1 s |
//!
//! The numbers are identical in both profiles — virtual time and datagram
//! counts do not depend on optimisation — so the debug run buys nothing
//! but 9.1 s on an ~18 s suite. Appendix B sanctions the alternative in as
//! many words: *"the production constant […] crossing once — **in the
//! release run if debug-slow**"* (ruling 251, the precedent
//! `tests/story_rekey.rs` set). O53a is therefore
//! `#[cfg_attr(debug_assertions, ignore)]` and runs under
//! `cargo test --release --all-features`, which every slice ends on. To
//! run it in debug anyway:
//!
//! ```text
//! cargo test --all-features --test spec_ack_burst -- --ignored
//! ```
//!
//! # Paused clock, never a sleep (§16.10)
//!
//! Every test here is `#[tokio::test(start_paused = true)]`. Virtual time
//! advances only where the fabric's injected delay and the protocol's own
//! timers ask it to.
//!
//! [`FlakyPolicy::drop_at`]: slither::testutil::FlakyPolicy::drop_at

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use slither::constants::{INITIAL_MAX_STREAM_DATA, K_GRANULARITY};
use slither::testutil::{FlakyPolicy, Pair, local, settle};
use tokio::time::Instant;

// ══════════════════════════════════════════════════════════════════════
// SIZING
// ══════════════════════════════════════════════════════════════════════

/// Payload moved per run. Large enough that the 256 KiB stream window and
/// the 1 MiB connection window both cycle many times, so the measurement
/// is of the protocol in steady state rather than of the initial window.
const PAYLOAD: usize = 2 << 20;

/// One `write` call's size — well above the 1169-byte payload MTU, so the
/// send path does the framing rather than the test doing it by hand.
const CHUNK: usize = 64 << 10;

/// One-way fabric delay, i.e. a 20 ms RTT. Without an injected RTT the PTO
/// sits at its `K_GRANULARITY` floor and the congestion controller never
/// binds, so an ACK-loss experiment on a zero-delay fabric measures
/// nothing.
const ONE_WAY: Duration = Duration::from_millis(10);

/// Return-path datagrams per burst period.
const PERIOD: usize = 8;

/// The severities run, as consecutive drops out of every [`PERIOD`]:
/// 25 %, 50 % and 75 % ACK loss, in bursts rather than as a Bernoulli
/// sprinkle.
const BURSTS: [usize; 3] = [2, 4, 6];

/// **Ruling 260's envelope, and deliberately not a tight pin.**
///
/// The harshest burst (6/8, 75 % ACK loss) measured **1.55 %** of forward
/// datagrams spent on data the receiver already held. This is that figure
/// with the ×1.5 regression headroom the ruling specifies, rounded up:
/// 1.55 % × 1.5 = 2.33 %. It is a floor under a *regression*, not a claim
/// that 2.5 % is a meaningful boundary — see the module docs for what the
/// forgetful-ACK build scores (50.9 % at **zero** ACK loss, and death at
/// 25 %).
const SPURIOUS_CEILING: f64 = 0.025;

/// How far past the setup index the drop schedule is built. Comfortably
/// beyond the return-path send count of any run here (999 at the
/// harshest); a schedule that ran out mid-transfer would silently heal the
/// wire and understate every rate below.
const SCHEDULE: usize = 40_000;

// ══════════════════════════════════════════════════════════════════════
// THE HARNESS
// ══════════════════════════════════════════════════════════════════════

/// A payload whose bytes vary with offset, so nothing here can be
/// measuring a memset-friendly buffer, and so the reader can verify what
/// arrived against its absolute offset without holding the transfer in
/// memory.
fn byte_at(offset: usize) -> u8 {
    (offset % 251) as u8
}

/// `burst` consecutive drops at the head of every [`PERIOD`] sends,
/// starting at absolute send index `start`.
fn burst_schedule(start: usize, burst: usize) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    let mut base = start;
    while base < start + SCHEDULE {
        for k in 0..burst {
            out.insert(base + k);
        }
        base += PERIOD;
    }
    out
}

/// What one run of the transfer cost.
#[derive(Debug, Clone, Copy)]
struct Run {
    /// Forward (A → B) datagrams that left the wire during the transfer.
    fwd_dgrams: usize,
    /// Their total wire bytes.
    fwd_bytes: usize,
    /// Return-path (B → A) datagrams the wire reached.
    ret_dgrams: usize,
    /// How many of those the burst schedule killed. **Counter-proved**:
    /// the tap records a send *above* the drop decision, so an index the
    /// wire reached and the schedule names is an index that died.
    ret_dropped: usize,
    /// Virtual time the transfer took.
    elapsed: Duration,
}

impl Run {
    /// The ACK loss actually achieved, as a fraction of the return path.
    fn ack_loss(&self) -> f64 {
        self.ret_dropped as f64 / self.ret_dgrams as f64
    }

    /// Forward datagrams carrying data the receiver already held, as a
    /// fraction of the forward datagrams sent — measured against `base`,
    /// the loss-free minimum for the same payload over the same fabric.
    fn spurious_dgrams(&self, base: &Run) -> f64 {
        (self.fwd_dgrams as f64 - base.fwd_dgrams as f64) / self.fwd_dgrams as f64
    }

    /// The same, in wire bytes.
    fn spurious_bytes(&self, base: &Run) -> f64 {
        (self.fwd_bytes as f64 - base.fwd_bytes as f64) / self.fwd_bytes as f64
    }

    /// Virtual-time inflation against the loss-free baseline. An **upper
    /// bound** on the false-congestion contribution, not a count of
    /// events — see the module docs.
    fn inflation(&self, base: &Run) -> f64 {
        self.elapsed.as_secs_f64() / base.elapsed.as_secs_f64()
    }

    /// Virtual-time throughput in MiB/s.
    fn mib_per_sec(&self) -> f64 {
        (PAYLOAD as f64 / (1024.0 * 1024.0)) / self.elapsed.as_secs_f64()
    }
}

/// Move [`PAYLOAD`] bytes A → B over one uni stream, with `burst`
/// return-path datagrams dropped out of every [`PERIOD`] and `one_way` of
/// fabric delay in each direction.
///
/// `burst == 0` is the baseline: an identical fabric with a loss-free
/// return path, whose forward datagram count is the loss-free minimum
/// every other run is measured against.
async fn transfer(seed: u64, burst: usize, one_way: Duration) -> Run {
    let pair = Pair::seeded(seed);
    let tap = pair.net.tap();
    let (ca, cb) = pair.establish().await;
    let a_addr: SocketAddr = pair.a.addr();
    let b_addr: SocketAddr = pair.b.addr();

    // Open and prime the stream before the wire is slowed, so neither the
    // handshake's round trips nor the stream's first frame are charged to
    // the transfer.
    let mut send = ca.open_uni().await.expect("open_uni");
    send.write(&[byte_at(0)]).await.expect("priming write");
    let mut recv = cb.accept_uni().await.expect("accept_uni");
    let mut prime = [0u8; 1];
    assert_eq!(
        recv.read(&mut prime).await.expect("priming read"),
        Some(1),
        "the priming byte did not arrive"
    );
    settle().await;

    // B's absolute send index right now. Every send so far was accepted —
    // the setup fabric is perfect — so the tap count *is* the index, and
    // the schedule below therefore starts after the handshake rather than
    // eating msg2.
    let setup = tap.drain();
    let b_index = setup.iter().filter(|s| s.src == b_addr).count();

    pair.a
        .wire
        .set_policy(FlakyPolicy::perfect().with_delay(one_way, Duration::ZERO));
    let schedule = burst_schedule(b_index, burst);
    // Built through the `drop_at` constructor rather than by functional
    // update: `FlakyPolicy`'s rustdoc says *"Every field is public, so a
    // test may build one literally"*, and `failing` is **not** — a literal
    // or `..base` construction is `E0451` from outside the crate.
    pair.b.wire.set_policy(
        FlakyPolicy::drop_at(schedule.iter().copied()).with_delay(one_way, Duration::ZERO),
    );

    // One byte of the stream is already spent on the priming write.
    let want = PAYLOAD - 1;
    let t0 = Instant::now();
    let writer = async {
        let mut off = 1usize;
        let mut chunk = vec![0u8; CHUNK];
        while off < PAYLOAD {
            let len = CHUNK.min(PAYLOAD - off);
            for (i, b) in chunk[..len].iter_mut().enumerate() {
                *b = byte_at(off + i);
            }
            let mut done = 0;
            while done < len {
                done += send.write(&chunk[done..len]).await.expect("write");
            }
            off += len;
        }
        send.finish().await.expect("finish");
    };
    let reader = async {
        let mut got = 0usize;
        let mut buf = vec![0u8; CHUNK];
        while got < want {
            let read = tokio::time::timeout(Duration::from_secs(600), recv.read(&mut buf))
                .await
                .unwrap_or_else(|_| {
                    panic!("the transfer stalled after {got} of {want} bytes, 600 s virtual")
                });
            match read.expect("read") {
                Some(n) => {
                    for (i, b) in buf[..n].iter().enumerate() {
                        assert_eq!(
                            *b,
                            byte_at(1 + got + i),
                            "the stream is not byte-exact at absolute offset {}",
                            1 + got + i
                        );
                    }
                    got += n;
                }
                None => panic!("end of stream after {got} of {want} bytes"),
            }
        }
        got
    };
    let (_, got) = tokio::join!(writer, reader);
    let elapsed = t0.elapsed();
    assert_eq!(got, want, "short transfer");

    let spied = tap.drain();
    let fwd: Vec<_> = spied.iter().filter(|s| s.src == a_addr).collect();
    let ret_dgrams = spied.iter().filter(|s| s.src == b_addr).count();

    Run {
        fwd_dgrams: fwd.len(),
        fwd_bytes: fwd.iter().map(|s| s.bytes.len()).sum(),
        ret_dgrams,
        ret_dropped: schedule.range(b_index..b_index + ret_dgrams).count(),
        elapsed,
    }
}

// ══════════════════════════════════════════════════════════════════════
// O53a
// ══════════════════════════════════════════════════════════════════════

/// **Appendix B O53a, discharged.** Sustained ACK-loss bursts at 25 %,
/// 50 % and 75 % against the 2048-bit fused window under §12.4's every-2nd
/// ACK policy, on the paused clock, quantifying both rates the obligation
/// names.
///
/// The module docs carry the measured table and the broken build's score.
/// What is asserted here:
///
/// * **The bursts really bit.** Each severity's counter-proved ACK loss
///   must land within a point of nominal. Without this the spurious-rate
///   assertion below would pass for the wrong reason on a schedule that
///   ran out or a fixture that healed — working rule 9's degenerate case.
/// * **The envelope holds** at every severity: forward datagrams spent on
///   data the receiver already held stay under [`SPURIOUS_CEILING`].
/// * **The transfer completes and is byte-exact** at every severity — the
///   `transfer` harness asserts that per read. A build that stalls under
///   ACK loss fails on the 600 s virtual-time timeout rather than on a
///   rate.
#[tokio::test(start_paused = true)]
#[cfg_attr(
    debug_assertions,
    ignore = "9.1 s in debug against 0.3 s in release, for identical numbers: \
              Appendix B's \"in the release run if debug-slow\" (ruling 251). \
              `cargo test --release --all-features` runs it; in debug, \
              `-- --ignored` does."
)]
async fn o53a_sustained_ack_loss_bursts_do_not_provoke_spurious_retransmits() {
    local(async {
        let base = transfer(0x0053_A000, 0, ONE_WAY).await;
        assert_eq!(
            base.ret_dropped, 0,
            "the baseline's return path must be loss-free: it is the minimum \
             every other run is measured against"
        );
        eprintln!("O53a baseline (0/{PERIOD}): {base:?}");

        for burst in BURSTS {
            let run = transfer(0x0053_A000, burst, ONE_WAY).await;
            let nominal = burst as f64 / PERIOD as f64;
            eprintln!(
                "O53a burst {burst}/{PERIOD}: ack_loss={:.1}% spurious_dgrams={:.2}% \
                 spurious_bytes={:.2}% inflation={:.3}x  {run:?}",
                run.ack_loss() * 100.0,
                run.spurious_dgrams(&base) * 100.0,
                run.spurious_bytes(&base) * 100.0,
                run.inflation(&base),
            );

            assert!(
                run.ret_dropped > 0 && (run.ack_loss() - nominal).abs() < 0.01,
                "the {burst}/{PERIOD} burst did not bite: {} of {} return-path \
                 datagrams dropped ({:.1} %, nominal {:.1} %). A schedule that ran \
                 out mid-transfer heals the wire silently and makes the envelope \
                 below pass for the wrong reason.",
                run.ret_dropped,
                run.ret_dgrams,
                run.ack_loss() * 100.0,
                nominal * 100.0,
            );

            // ── Ruling 260's envelope. A regression bound, not a pin: the
            // measured worst case is 1.55 % at 6/8 and the ceiling is that
            // ×1.5. The forgetful-ACK build scores 50.9 % at *zero* ACK
            // loss and dies outright at 2/8 (module docs), so this is two
            // orders of magnitude clear of the defect it guards.
            assert!(
                run.spurious_dgrams(&base) < SPURIOUS_CEILING,
                "O53a envelope: at {burst}/{PERIOD} ({:.1} % ACK loss) {:.2} % of \
                 forward datagrams carried data the receiver already held, over \
                 ruling 260's {:.1} % ceiling. {run:?} against baseline {base:?}",
                run.ack_loss() * 100.0,
                run.spurious_dgrams(&base) * 100.0,
                SPURIOUS_CEILING * 100.0,
            );
            assert!(
                run.spurious_bytes(&base) < SPURIOUS_CEILING,
                "O53a envelope, in wire bytes: {:.2} % over the {:.1} % ceiling",
                run.spurious_bytes(&base) * 100.0,
                SPURIOUS_CEILING * 100.0,
            );
        }
    })
    .await;
}

// ══════════════════════════════════════════════════════════════════════
// O53b
// ══════════════════════════════════════════════════════════════════════

/// **Appendix B O53b's "with no stall", pinned.** 2 MiB over a perfect,
/// zero-delay `FlakyWire` costs **0 ns of virtual time**.
///
/// On the paused clock virtual time advances only when the runtime has
/// nothing left to do but wait on a timer, so this is not a speed
/// measurement — it is the statement that *no timer is on the critical
/// path of a bulk transfer*. The bound is set below `K_GRANULARITY`, the
/// smallest timer granularity in the stack (1 ms), so not one timer may
/// fire: a single delayed ACK would cost `MAX_ACK_DELAY` (25 ms) and a
/// single PTO more.
///
/// Deterministic and machine-independent because the paused clock replays
/// one schedule and charges no wall-clock work. Expressed as a throughput
/// floor it is ≥ 2 MiB / 1 ms = 2048 MiB/s of virtual-time throughput.
///
/// BROKEN BUILD: any regression that makes progress wait on a deadline —
/// a credit re-grant that arrives only on the ACK timer, a send path that
/// parks until the next PTO — spends virtual time here and fails, while
/// remaining invisible to every wall-clock benchmark on a fast machine.
#[tokio::test(start_paused = true)]
async fn o53b_a_bulk_transfer_over_a_perfect_wire_never_waits_on_a_timer() {
    local(async {
        let run = transfer(0x0053_B000, 0, Duration::ZERO).await;
        eprintln!(
            "O53b no-stall: {PAYLOAD} bytes in {:?} of virtual time ({} fwd datagrams)",
            run.elapsed, run.fwd_dgrams
        );
        assert!(
            run.elapsed < K_GRANULARITY,
            "O53b: {PAYLOAD} bytes over a perfect wire spent {:?} of virtual time. \
             Measured on ruling 260's commit: 0 ns. The bound is `K_GRANULARITY` \
             ({K_GRANULARITY:?}), the smallest timer in the stack — anything at or \
             above it means a timer fired on the transfer's critical path.",
            run.elapsed,
        );
    })
    .await;
}

/// **Appendix B O53b's throughput half, re-scoped.** The obligation's
/// original bar was *"within 20 % of quinn under its shipped defaults"* —
/// a figure from another crate on unstated hardware, which no test can
/// hold and which `benches/throughput.rs` (ruling 247) already answers
/// properly. What is pinned instead is **virtual-time throughput over a
/// 20 ms-RTT fabric**, where the transfer is window-limited and the clock
/// is deterministic.
///
/// Measured on ruling 260's commit: 2 MiB in **310 ms** of virtual time =
/// **6.45 MiB/s**, against the 12.5 MiB/s that
/// `INITIAL_MAX_STREAM_DATA / RTT` predicts for a static window. The floor
/// is **half** the measured value, per ruling 260.
///
/// BROKEN BUILD: one that halves the effective window, delays credit
/// re-grant by a round trip, or adds a round trip per window — each shows
/// up here as virtual time and nowhere else, because the wall clock on
/// this fabric is dominated by allocation in the fixture rather than by
/// the protocol's round trips.
#[tokio::test(start_paused = true)]
async fn o53b_virtual_time_throughput_holds_its_floor() {
    // Half the 6.45 MiB/s measured on ruling 260's commit.
    const FLOOR_MIB_S: f64 = 3.22;

    local(async {
        let run = transfer(0x0053_B001, 0, ONE_WAY).await;
        let predicted =
            (INITIAL_MAX_STREAM_DATA as f64 / (1024.0 * 1024.0)) / (ONE_WAY.as_secs_f64() * 2.0);
        eprintln!(
            "O53b floor: {PAYLOAD} bytes in {:?} virtual = {:.2} MiB/s \
             (window/RTT predicts {predicted:.2})",
            run.elapsed,
            run.mib_per_sec(),
        );
        assert!(
            run.mib_per_sec() >= FLOOR_MIB_S,
            "O53b floor: {:.2} MiB/s of virtual-time throughput, under ruling \
             260's {FLOOR_MIB_S} MiB/s floor (half the 6.45 MiB/s measured on \
             that commit; window/RTT predicts {predicted:.2}). {run:?}",
            run.mib_per_sec(),
        );
    })
    .await;
}

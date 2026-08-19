//! `ObservedWire` — a `Wire` that knows what happened to each datagram.
//!
//! # Why this exists at all
//!
//! The obvious way to animate packets is `Network::tap()`. It does not
//! work, and the reason is in `FlakyWire::send_to`'s own documented step
//! order:
//!
//! > 4. Record in the tap.
//! > 5. Decide how many copies to deliver: 0 (lost), 1, or 2 (duplicated).
//! > 6. Draw a delay per copy and queue it.
//!
//! The tap fires *before* the fate is decided and the delay is drawn, and
//! `Tap`'s own documentation says so: *"Loss and duplication are applied
//! after the tap."* So a tapped datagram is indistinguishable between
//! delivered, lost and duplicated, and carries no arrival time. A demo
//! animating dots from the tap would have to **invent** which ones die,
//! which is the one thing this page must not do.
//!
//! Re-deriving `FlakyWire`'s draws in the demo was rejected: the draw
//! order is contract but the seeding
//! (`seed ^ ordinal.wrapping_mul(SEED_STRIDE)` into a `ChaCha20Rng`) is
//! private, so the mirror would be coupled to internals and would
//! desynchronise silently.
//!
//! # What this does instead
//!
//! §16.3's normative property 1 says the application supplies the `Wire`,
//! *"so an application needing its own socket options, a dual-stack or
//! per-interface arrangement, a tunnel, or a **simulator** installs one
//! without forking the crate."* This is that consumer.
//!
//! `ObservedWire` wraps a `FlakyWire` that is left on
//! [`FlakyPolicy::perfect`] and applies loss, duplication, delay and
//! blackholing itself. Because the demo makes each decision, it knows the
//! outcome exactly — departure instant, arrival instant, and fate, per
//! copy — and the animation reports rather than guesses.
//!
//! The `Network` keeps doing everything else: address registration,
//! routing, per-endpoint inboxes, the `sleep_until` in `recv_from` that
//! makes virtual time work, and wakeups. Nothing slither does changes: the
//! driver sees a `Wire` that sometimes loses datagrams, which is exactly
//! what it saw before.
//!
//! # The delay is scheduled, never awaited
//!
//! A `send_to` that awaited its own propagation delay would hold the
//! driver's send path for the duration and distort every timer measured
//! against it. Delayed copies are `spawn_local`'d instead, so `send_to`
//! returns in the same turn it was called in and the copy lands on the
//! peer's inbox at the drawn instant.

use std::cell::{Cell, RefCell};
use std::io;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Duration;

use slither::shell::wire::Wire;
use slither::testutil::FlakyWire;
use tokio::time::Instant;

use crate::log::Log;

/// The faults this wire applies, all owned by the demo.
#[derive(Clone, Copy, Default)]
pub struct Faults {
    /// Probability in `[0, 1]` that a datagram is destroyed outright.
    pub loss: f64,
    /// Probability in `[0, 1]` that a surviving datagram is delivered
    /// twice.
    pub duplicate: f64,
    /// Fixed propagation delay applied to every copy.
    pub base_delay: Duration,
    /// Extra uniform `[0, jitter)` per copy. Non-zero jitter is what
    /// reorders a stream.
    pub jitter: Duration,
}

struct Shared {
    inner: Rc<FlakyWire>,
    side: &'static str,
    log: Log,
    faults: Cell<Faults>,
    /// A demo-side blackhole. `Network::partition` would do the same job,
    /// but its state is not readable from outside `testutil`, so a
    /// partitioned send would be reported as delivered. Owning the flag
    /// keeps every event honest.
    blackholed: Cell<bool>,
    rng: RefCell<SplitMix64>,
}

/// A `Wire` that reports what it did. Cheap to clone; clones share the
/// fault settings, the draw stream and the ordinal counter — so a handle
/// kept by the scenario steers the one the endpoint owns.
#[derive(Clone)]
pub struct ObservedWire(Rc<Shared>);

impl ObservedWire {
    /// Wrap `inner` — which the caller must leave on
    /// [`FlakyPolicy::perfect`], since every fault is applied here.
    ///
    /// [`FlakyPolicy::perfect`]: slither::testutil::FlakyPolicy::perfect
    pub fn new(inner: FlakyWire, side: &'static str, log: Log, faults: Faults, seed: u64) -> Self {
        ObservedWire(Rc::new(Shared {
            inner: Rc::new(inner),
            side,
            log,
            faults: Cell::new(faults),
            blackholed: Cell::new(false),
            rng: RefCell::new(SplitMix64::new(seed)),
        }))
    }

    /// Swallow everything this wire sends, without an error — the demo's
    /// partition. A blackhole is not a send failure, and slither's driver
    /// distinguishes them (ruling 49).
    pub fn set_blackholed(&self, on: bool) {
        self.0.blackholed.set(on);
    }
}

impl Wire for ObservedWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        let s = &*self.0;
        let id = s.log.next_pkt();
        let kind = classify(buf);
        let len = buf.len();
        s.log.count(|c| c.sent += 1);

        let describe = |copy, fate, delay| Tx {
            id,
            copy,
            dst: addr,
            len,
            kind,
            fate,
            delay,
        };

        if s.blackholed.get() {
            s.log.count(|c| c.blackholed += 1);
            s.emit(describe(0, "blackholed", None));
            // `Ok`, deliberately: §16.3 and ruling 49 make a blackhole a
            // topology fact and a send failure an `io::Error`, and the
            // driver treats them differently.
            return Ok(len);
        }

        let faults = s.faults.get();
        // Two draws per send, in a fixed order, so a seed reproduces a run
        // exactly — the same discipline `FlakyWire` documents for its own
        // stream. The duplication draw is taken even when the loss draw
        // has already decided the outcome, so the stream position does not
        // depend on the branch.
        let lost = s.rng.borrow_mut().unit() < faults.loss;
        let duplicated = s.rng.borrow_mut().unit() < faults.duplicate;

        if lost {
            s.log.count(|c| c.lost += 1);
            s.emit(describe(0, "lost", None));
            return Ok(len);
        }

        let copies = if duplicated { 2 } else { 1 };
        if duplicated {
            s.log.count(|c| c.duplicated += 1);
        }
        let now = Instant::now();
        for copy in 0..copies {
            let delay = s.rng.borrow_mut().delay(faults.base_delay, faults.jitter);
            let at = now + delay;
            s.log.count(|c| c.delivered += 1);
            s.emit(describe(copy, "delivered", Some(delay)));

            if delay.is_zero() {
                // The inner wire is `perfect()`, so this queues the
                // datagram for `now` and returns without awaiting.
                s.inner.send_to(buf, addr).await?;
            } else {
                let inner = Rc::clone(&s.inner);
                let bytes = buf.to_vec();
                tokio::task::spawn_local(async move {
                    tokio::time::sleep_until(at).await;
                    // The peer may already be gone; a demo does not care.
                    let _ = inner.send_to(&bytes, addr).await;
                });
            }
        }
        Ok(len)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.0.inner.recv_from(buf).await
    }
}

/// One datagram, as the timeline describes it.
///
/// A struct rather than eight positional parameters — which is what this
/// was, and what clippy's `too_many_arguments` correctly objected to. The
/// two `&str`s next to each other (`kind` and `fate`) were the real hazard:
/// swapping them is not a type error.
struct Tx<'a> {
    id: u64,
    copy: u32,
    dst: SocketAddr,
    len: usize,
    kind: &'a str,
    fate: &'a str,
    /// `Some` only when the copy will actually arrive.
    delay: Option<Duration>,
}

impl Shared {
    /// Write one `tx` event.
    fn emit(&self, tx: Tx<'_>) {
        let Tx {
            id,
            copy,
            dst,
            len,
            kind,
            fate,
            delay,
        } = tx;
        let from = self.side;
        let to = if self.side == "a" { "b" } else { "a" };
        let arrive = match delay {
            Some(d) => format!(r#","arrive_us":{}"#, self.log.now_us() + d.as_micros()),
            None => String::new(),
        };
        self.log.emit(
            "tx",
            &format!(
                r#""pkt":{id},"copy":{copy},"from":"{from}","to":"{to}","dst":"{dst}","len":{len},"type":"{kind}","fate":"{fate}"{arrive}"#
            ),
        );
    }
}

/// Name the datagram from its type byte (§3.1: the packet type is byte 0).
///
/// Lengths are cross-checked against the ratified pins rather than assumed:
/// a handshake packet whose length is not `INIT_PACKET_LEN` /
/// `RESP_PACKET_LEN` is reported as `malformed` instead of being labelled
/// with a type it cannot have. The demo never parses further — it is not a
/// parser, and `testutil` deliberately is not one either.
fn classify(buf: &[u8]) -> &'static str {
    use slither::constants::{
        INIT_PACKET_LEN, PKT_DATA, PKT_HANDSHAKE_INIT, PKT_HANDSHAKE_RESP, RESP_PACKET_LEN,
    };
    match buf.first() {
        Some(&PKT_HANDSHAKE_INIT) if buf.len() == INIT_PACKET_LEN => "init",
        Some(&PKT_HANDSHAKE_RESP) if buf.len() == RESP_PACKET_LEN => "resp",
        Some(&PKT_DATA) => "data",
        Some(_) => "malformed",
        None => "empty",
    }
}

/// SplitMix64 — the demo's fault draws.
///
/// Deliberately not `rand_chacha`, which slither uses: the demo's draws
/// are the demo's, and a second RNG crate in the graph would be a
/// dependency (and wasm bytes) bought for fifteen lines of arithmetic.
/// This is Steele/Lea/Flood's mixer, the standard `SplittableRandom`
/// finaliser, and it is used here for jitter and coin flips — never for
/// anything cryptographic.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform draw in `[0, 1)` from 53 bits — the standard
    /// construction, and the same one `FlakyWire::draw_unit` uses.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `base + uniform[0, jitter)`, drawn even when `jitter` is zero so
    /// the stream advances by the same amount either way.
    fn delay(&mut self, base: Duration, jitter: Duration) -> Duration {
        let raw = self.next_u64();
        let jitter_ns = jitter.as_nanos() as u64;
        if jitter_ns == 0 {
            base
        } else {
            base + Duration::from_nanos(raw % jitter_ns)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SplitMix64, classify};
    use slither::constants::{INIT_PACKET_LEN, PKT_HANDSHAKE_INIT, RESP_PACKET_LEN};
    use std::time::Duration;

    #[test]
    fn a_handshake_byte_with_the_wrong_length_is_not_labelled_a_handshake() {
        let mut init = vec![0u8; INIT_PACKET_LEN];
        init[0] = PKT_HANDSHAKE_INIT;
        assert_eq!(classify(&init), "init");
        // One byte short of the ratified pin: the type byte alone must not
        // be enough to earn the label.
        init.pop();
        assert_eq!(classify(&init), "malformed");
        assert_ne!(INIT_PACKET_LEN, RESP_PACKET_LEN);
        assert_eq!(classify(&[]), "empty");
    }

    #[test]
    fn the_unit_draw_spans_the_range_and_stays_inside_it() {
        let mut rng = SplitMix64::new(0xC0FFEE);
        let (mut lo, mut hi) = (1.0f64, 0.0f64);
        for _ in 0..10_000 {
            let u = rng.unit();
            assert!((0.0..1.0).contains(&u), "unit() left [0,1): {u}");
            lo = lo.min(u);
            hi = hi.max(u);
        }
        // A degenerate generator returning a constant satisfies the bound
        // above for free, so assert from the side that separates them.
        assert!(lo < 0.01, "never drew low: {lo}");
        assert!(hi > 0.99, "never drew high: {hi}");
    }

    #[test]
    fn jitter_actually_varies_and_zero_jitter_does_not() {
        let mut rng = SplitMix64::new(7);
        let base = Duration::from_millis(10);
        let with: Vec<_> = (0..64)
            .map(|_| rng.delay(base, Duration::from_millis(5)))
            .collect();
        assert!(
            with.iter().any(|d| *d != with[0]),
            "every jittered delay was identical — jitter is not applied"
        );
        assert!(
            with.iter()
                .all(|d| *d >= base && *d < base + Duration::from_millis(5))
        );
        let flat: Vec<_> = (0..8).map(|_| rng.delay(base, Duration::ZERO)).collect();
        assert!(flat.iter().all(|d| *d == base));
    }
}

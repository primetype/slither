//! Two endpoints on one in-memory network, with every stage narrated.
//!
//! # Why not `testutil::Pair`
//!
//! `Pair` is the right fixture for a test and the wrong one for this demo,
//! for two reasons.
//!
//! 1. It builds its endpoints over `SharedWire(Rc<FlakyWire>)`, and the
//!    demo needs [`crate::wire::ObservedWire`] instead — see that module
//!    for why the tap cannot answer "did this packet arrive".
//! 2. `Pair::establish()` climbs §6.2's staged-accept ladder *inside*
//!    itself. The ladder is exactly what the demo exists to show, so it
//!    has to be climbed where each rung can be logged.
//!
//! [`Sim::establish`] is therefore `Pair::establish`'s structure with
//! events added, and it keeps the two subtleties that structure exists
//! for — both of which are load-bearing under loss and neither of which is
//! obvious:
//!
//! * the responder side is a **loop**, not one ladder (ruling 252): msg2
//!   is never retransmitted (§5.5), so a lost msg2 is recovered by the
//!   peer re-offering a fresh `Intro` and only the *next* `accept()`
//!   closing the gap;
//! * the in-flight ladder is `pin!`ned across `select!` polls and finished
//!   rather than dropped when the dial resolves first, because the dial
//!   can resolve one turn before the responder's `accept()` does.

use std::net::SocketAddr;
use std::time::Duration;

use slither::testutil::{
    CountingIdentity, DhCounter, Network, TestConnection, TestEndpoint, TestIdentity, TestPublicKey,
};
use slither::{Config, Identity};

use crate::log::{Log, jstr};
use crate::wire::{Faults, ObservedWire};

/// One endpoint and the handles the demo needs on it.
pub struct Side {
    pub endpoint: TestEndpoint,
    pub wire: ObservedWire,
    pub dhs: DhCounter,
    pub public_static: TestPublicKey,
    pub addr: SocketAddr,
}

/// The whole fixture.
pub struct Sim {
    /// Kept alive because every `FlakyWire` holds an `Rc` into it and the
    /// registrations are what routing reads.
    pub _net: Network,
    pub log: Log,
    pub a: Side,
    pub b: Side,
}

impl Sim {
    /// Two endpoints at `10.0.0.1:4001` (dialler) and `10.0.0.2:4002`
    /// (responder), everything derived from `seed`.
    ///
    /// # Panics
    ///
    /// Outside a `tokio::task::LocalSet` — the shell is a `!Send` actor
    /// spawned with `spawn_local` (§16.3).
    pub fn new(seed: u64, faults: Faults, log: Log) -> Sim {
        let net = Network::seeded(seed);
        let a_addr: SocketAddr = "10.0.0.1:4001".parse().expect("literal addr");
        let b_addr: SocketAddr = "10.0.0.2:4002".parse().expect("literal addr");
        let a = Side::build(&net, a_addr, "a", seed, 0xA1, faults, log.clone());
        let b = Side::build(&net, b_addr, "b", seed, 0xB2, faults, log.clone());
        log.emit(
            "topology",
            &format!(
                r#""a":{},"b":{}"#,
                jstr(&a_addr.to_string()),
                jstr(&b_addr.to_string())
            ),
        );
        Sim {
            _net: net,
            log,
            a,
            b,
        }
    }

    /// Report both DH counters — §6.1 prices the staged ladder
    /// cumulatively, so this is the number the demo's badge shows.
    pub fn dh_body(&self) -> String {
        format!(
            r#""dhs":{{"a":{},"b":{}}}"#,
            self.a.dhs.get(),
            self.b.dhs.get()
        )
    }

    fn stage(&self, side: &str, stage: &str, extra: &str) {
        let sep = if extra.is_empty() { "" } else { "," };
        self.log.emit(
            "stage",
            &format!(
                r#""side":"{side}","stage":"{stage}",{}{sep}{extra}"#,
                self.dh_body()
            ),
        );
    }

    /// Dial `a` → `b` and climb §6.2's ladder, logging every rung.
    pub async fn establish(&self) -> Result<(TestConnection, TestConnection), String> {
        self.stage("a", "dial", "");
        let dial = async {
            self.a
                .endpoint
                .connect(self.b.addr, self.b.public_static)
                .map_err(|e| format!("connect: {e}"))?
                .await
                .map_err(|e| format!("dial: {e}"))
        };
        tokio::pin!(dial);

        let mut accepted: Option<Result<TestConnection, String>> = None;
        let a_conn = 'outer: loop {
            let ladder =
                async {
                    let intro =
                        self.b.endpoint.accept().await.ok_or_else(|| {
                            "the endpoint closed before an introduction".to_owned()
                        })?;
                    self.stage(
                        "b",
                        "intro",
                        &format!(
                            r#""source":{},"sender_index":{}"#,
                            jstr(&intro.source().to_string()),
                            intro.sender_index()
                        ),
                    );
                    let claimed = intro
                        .read_identity()
                        .await
                        .map_err(|e| format!("read_identity: {e}"))?;
                    self.stage("b", "claimed", "");
                    let proven = claimed
                        .authenticate()
                        .await
                        .map_err(|e| format!("authenticate: {e}"))?;
                    self.stage(
                        "b",
                        "proven",
                        &format!(r#""timestamp":"{:?}""#, proven.timestamp()),
                    );
                    let conn = proven.accept().await.map_err(|e| format!("accept: {e}"))?;
                    self.stage("b", "connection", "");
                    Ok::<TestConnection, String>(conn)
                };
            tokio::pin!(ladder);
            tokio::select! {
                biased;
                conn = &mut ladder => {
                    let failed = conn.is_err();
                    accepted = Some(conn);
                    // A ladder that failed will not be retried: on a demo
                    // there is nothing to recover to, and looping would
                    // spin.
                    if failed {
                        break 'outer Err("the responder's ladder failed".to_owned());
                    }
                }
                conn = &mut dial => {
                    if accepted.is_none() {
                        accepted = Some(ladder.await);
                    }
                    break 'outer conn;
                }
            }
        };
        let a_conn = a_conn?;
        let b_conn = accepted.expect("set on both break paths")?;
        self.stage("a", "established", "");
        Ok((a_conn, b_conn))
    }
}

impl Side {
    fn build(
        net: &Network,
        addr: SocketAddr,
        name: &'static str,
        seed: u64,
        salt: u8,
        faults: Faults,
        log: Log,
    ) -> Side {
        let identity: TestIdentity = CountingIdentity::seeded(derive_seed(seed, salt));
        let dhs = identity.counter();
        let public_static = *Identity::public_static(&identity);
        let wire = ObservedWire::new(
            net.endpoint(addr),
            name,
            log,
            faults,
            seed ^ ((salt as u64) << 32),
        );
        let endpoint = slither::Endpoint::builder()
            .identity(identity)
            .wire(wire.clone())
            .config(Config::new())
            // §16.6: an explicit seed is what makes a run reproducible.
            // The demo has no other entropy source it would want.
            .rng_seed(derive_seed(seed, salt ^ 0xFF))
            .build();
        Side {
            endpoint,
            wire,
            dhs,
            public_static,
            addr,
        }
    }
}

/// Spread a `u64` seed and a salt over 32 bytes.
fn derive_seed(seed: u64, salt: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let word = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add((salt as u64) << 8 | i as u64);
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Read until the peer's FIN, i.e. until `read` reports `Ok(None)`.
///
/// The `Ok(Some(0))` / `Ok(None)` distinction is the whole convention
/// here: `None` is "finished", and treating a zero-length read as the end
/// instead is what hangs a reader forever on a stream that is not done.
pub async fn read_to_fin(recv: &mut slither::testutil::TestRecvStream) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match recv
            .read(&mut buf)
            .await
            .map_err(|e| format!("read: {e}"))?
        {
            Some(n) => out.extend_from_slice(&buf[..n]),
            None => return Ok(out),
        }
    }
}

/// The virtual-time ceiling every scenario runs under.
///
/// Not a safety belt against slow hardware — virtual time has no relation
/// to wall time — but against a scenario that parks with no timer to
/// advance to. `timeout` supplies a pending timer, which is exactly what
/// the auto-advancing paused clock needs to make progress and report the
/// hang instead of freezing a Worker.
pub const SCENARIO_CEILING: Duration = Duration::from_secs(300);

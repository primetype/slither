//! The UDP endpoint: a single `!Send` actor task behind a [`Wire`], plus the
//! [`Endpoint`]/[`SessionHandle`] handles and the [`Event`] stream.
//!
//! One [`Endpoint`] owns one socket and one actor. The actor drives every
//! handshake, seals and opens every Data packet, and runs the WireGuard timers
//! (retransmit, keepalive, liveness, rekey). It is a single-task actor spawned
//! with [`tokio::task::spawn_local`] (it makes no `Send` promise — a provider
//! need not be `Send`); the consumer must run it inside a
//! [`LocalSet`](tokio::task::LocalSet), exactly as `bubble-client` runs the
//! `!Send` island transport.
//!
//! # Delivery semantics (Leg 2 — ratified)
//!
//! [`SessionHandle::send`] queues a **reliable message**: it rides a DATA frame
//! inside the sealed packets, is retransmitted (RFC 9002 loss detection + PTO,
//! in the crate-internal `recovery` module) until acknowledged, and surfaces on
//! the peer exactly once ([`Event::Incoming`], deduplicated by message
//! sequence).
//! Delivery is **unordered** — each message is independent; ordered streams are
//! reserved frame space. Reliability lives within the connection: messages
//! still undelivered when the connection dies ([`Event::Dead`], or the process
//! ends) are lost, and `send` still means *queued*, not *delivered*. Lifecycle
//! transitions surface on the [`Event`] stream ([`Endpoint::next_event`]).
//! Frame-layer control packets (ACKs, probes, retransmissions) are
//! liveness-neutral: they ride the same sealed wire but do not mark the Leg 1
//! keepalive/dead clocks, so the ratified timer table behaves exactly as ruled.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::Duration;

use hiss::curve::p256::P256r1PublicKey;
use packtool::{Packed, View};
use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use tokio::sync::mpsc;
// The actor's timers run on tokio's clock so a paused test runtime advances them.
use tokio::time::{self, Instant, MissedTickBehavior};

use crate::frame::{self, Frame, MAX_MESSAGE};
use crate::handshake::{self, HsError, Identity, InitiatorPending, TimestampGuard, random_index};
use crate::recovery::Recovery;
use crate::session::Session;
use crate::wire::{
    self, DataHeader, MAX_DATAGRAM, PacketKind, RESP_PACKET_LEN, RespHeader, Timestamp,
};

// ── Handshake timers (ratified; the full table lives in slither/SPEC.md) ───────

/// The base delay before a fresh initiation retransmit.
pub const RETRANSMIT_BASE: Duration = Duration::from_secs(5);
/// The maximum uniform jitter added to [`RETRANSMIT_BASE`].
pub const RETRANSMIT_JITTER_MAX: Duration = Duration::from_millis(333);
/// A `connect` that has not established within this long gives up.
pub const HANDSHAKE_GIVEUP: Duration = Duration::from_secs(90);

/// The actor's timer-scan period. Every timer decision is evaluated on this tick
/// (virtual time under a paused test clock), so it bounds the granularity of the
/// retransmit / keepalive / liveness / rekey deadlines.
const TICK: Duration = Duration::from_millis(250);

/// Inbound (responder-side) connection ids are minted from here up, disjoint from
/// the outbound `connect` ids (which count from 1), so the two never collide in
/// the one session map.
const INBOUND_CONN_BASE: u64 = 1 << 63;

// ── Public identifiers, errors, and events ────────────────────────────────────

/// A logical connection identifier, stable across a rekey (the session is swapped
/// underneath; the id does not change).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnId(pub u64);

/// An error returned by the synchronous handle API.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SlitherError {
    /// The message exceeds [`MAX_MESSAGE`] (what fits, after framing overhead,
    /// in one sealed packet — multi-packet messages are reserved for the
    /// STREAM work). Rejected at [`SessionHandle::send`] before it ever
    /// reaches the actor.
    #[error("payload is {len} bytes, over the {max}-byte maximum")]
    PayloadTooLarge {
        /// The offered message length.
        len: usize,
        /// The maximum permitted ([`MAX_MESSAGE`]).
        max: usize,
    },
    /// The endpoint actor has stopped (its task ended); no further commands can be
    /// delivered.
    #[error("the slither endpoint has stopped")]
    EndpointClosed,
}

/// Why a `connect` failed.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ConnectError {
    /// No HandshakeResp completed the handshake within [`HANDSHAKE_GIVEUP`],
    /// across every fresh-ephemeral retransmit.
    #[error("handshake timed out after {0:?}")]
    TimedOut(Duration),
}

/// A lifecycle or delivery event surfaced by the endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A session established (a fresh connection — not a silent rekey swap).
    Established {
        /// The logical connection.
        conn: ConnId,
        /// The peer's authenticated static.
        remote_static: P256r1PublicKey,
        /// The peer's current address.
        endpoint: SocketAddr,
    },
    /// A reliable application message arrived — surfaced exactly once per
    /// message (deduplicated by its frame sequence number; a retransmitted
    /// copy is acknowledged again but not re-surfaced), in arrival order,
    /// which is **not** necessarily send order (unordered reliable delivery).
    Incoming {
        /// The connection it arrived on.
        conn: ConnId,
        /// The peer's authenticated static.
        remote_static: P256r1PublicKey,
        /// The message bytes.
        payload: Vec<u8>,
    },
    /// The session died — a liveness timeout (sent-but-nothing-received for
    /// [`DEAD_TIMEOUT`](crate::session::DEAD_TIMEOUT)), the payload-seal
    /// [`REJECT_AGE`](crate::session::REJECT_AGE) backstop, or a revoked static.
    Dead {
        /// The connection that died.
        conn: ConnId,
        /// The peer's static.
        remote_static: P256r1PublicKey,
    },
    /// An authenticated Data packet arrived from a new source, so the session
    /// roamed to it.
    EndpointMoved {
        /// The connection that roamed.
        conn: ConnId,
        /// The peer's static.
        remote_static: P256r1PublicKey,
        /// The previous address.
        from: SocketAddr,
        /// The new address.
        to: SocketAddr,
    },
    /// A `connect` gave up (an addition to the brief's four-event set — the only
    /// way to surface a handshake give-up through the fire-and-forget handle).
    Failed {
        /// The connection that failed.
        conn: ConnId,
        /// Why.
        error: ConnectError,
    },
}

// ── The Wire seam ─────────────────────────────────────────────────────────────

/// The datagram substrate the endpoint runs over — a real UDP socket in
/// production, an in-memory shim (`testutil::FlakyWire`, behind the
/// `test-util` feature) in tests. Making the socket a trait keeps the whole
/// protocol drivable without a kernel (the testability-first rule).
#[allow(async_fn_in_trait)] // the actor is single-threaded; no cross-thread Send bound is needed.
pub trait Wire {
    /// Send `buf` to `addr`, returning the bytes written.
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize>;
    /// Receive one datagram into `buf`, returning its length and source.
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
}

impl Wire for tokio::net::UdpSocket {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        tokio::net::UdpSocket::send_to(self, buf, addr).await
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        tokio::net::UdpSocket::recv_from(self, buf).await
    }
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// Endpoint configuration: the initial allow-list, the optional persistent
/// keepalive, and the CSPRNG that draws session indices and retransmit jitter.
pub struct Config {
    allow: HashSet<[u8; 33]>,
    persistent_keepalive: Option<Duration>,
    rng: ChaCha20Rng,
}

impl Config {
    /// A fresh configuration with an empty allow-list, no persistent keepalive,
    /// and an OS-entropy-seeded index/jitter CSPRNG.
    pub fn new() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("OS entropy for the slither index/jitter CSPRNG");
        let rng = ChaCha20Rng::from_seed(seed);
        hiss::zeroize::zeroize_array(&mut seed);
        Self {
            allow: HashSet::new(),
            persistent_keepalive: None,
            rng,
        }
    }

    /// Seed the index/jitter CSPRNG deterministically (tests only).
    pub fn with_rng_seed(mut self, seed: [u8; 32]) -> Self {
        self.rng = ChaCha20Rng::from_seed(seed);
        self
    }

    /// Permit inbound handshakes from `remote_static` (a family device).
    pub fn allow(mut self, remote_static: &P256r1PublicKey) -> Self {
        self.allow.insert(remote_static.to_compressed());
        self
    }

    /// Enable the optional persistent keepalive at `interval`.
    pub fn persistent_keepalive(mut self, interval: Duration) -> Self {
        self.persistent_keepalive = Some(interval);
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

// ── Handles ───────────────────────────────────────────────────────────────────

/// The endpoint handle: opens connections, adjusts the allow-list, and reads the
/// event stream. Dropping every [`Endpoint`] and [`SessionHandle`] stops the
/// actor.
pub struct Endpoint {
    cmd_tx: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<Event>,
    next_connect: u64,
}

impl Endpoint {
    /// Spawn the endpoint actor for `identity` over `wire`, with `config`.
    ///
    /// Must be called inside a [`LocalSet`](tokio::task::LocalSet) — the actor is
    /// `!Send` and is spawned with [`tokio::task::spawn_local`].
    pub fn start<I, W>(identity: I, wire: W, config: Config) -> Endpoint
    where
        I: Identity + 'static,
        W: Wire + 'static,
    {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let actor = Actor {
            identity,
            wire,
            rng: config.rng,
            allow: config.allow,
            persistent_keepalive: config.persistent_keepalive,
            ts_guard: TimestampGuard::new(),
            last_init_timestamp: None,
            sessions: HashMap::new(),
            recovery: HashMap::new(),
            index_to_conn: HashMap::new(),
            static_to_conn: HashMap::new(),
            pending: HashMap::new(),
            pending_by_index: HashMap::new(),
            next_inbound: INBOUND_CONN_BASE,
            cmd_rx,
            events: ev_tx,
        };
        tokio::task::spawn_local(actor.run());
        Endpoint {
            cmd_tx,
            events: ev_rx,
            next_connect: 1,
        }
    }

    /// Open a session to `remote_static` at `remote_addr` as the initiator.
    ///
    /// Returns immediately with a [`SessionHandle`]; establishment (or give-up)
    /// arrives later as an [`Event`].
    pub fn connect(
        &mut self,
        remote_addr: SocketAddr,
        remote_static: P256r1PublicKey,
    ) -> SessionHandle {
        let conn = ConnId(self.next_connect);
        self.next_connect += 1;
        let _ = self.cmd_tx.send(Command::Connect {
            conn,
            remote_addr,
            remote_static,
        });
        SessionHandle {
            conn,
            cmd_tx: self.cmd_tx.clone(),
        }
    }

    /// Build a handle for a connection learnt from an [`Event`] (e.g. an inbound
    /// [`Event::Established`]).
    pub fn session(&self, conn: ConnId) -> SessionHandle {
        SessionHandle {
            conn,
            cmd_tx: self.cmd_tx.clone(),
        }
    }

    /// Add `remote_static` to the inbound allow-list.
    pub fn allow(&self, remote_static: &P256r1PublicKey) {
        let _ = self.cmd_tx.send(Command::Allow {
            remote_static: *remote_static,
        });
    }

    /// Remove `remote_static` from the allow-list and tear down any live session
    /// with it.
    pub fn revoke(&self, remote_static: &P256r1PublicKey) {
        let _ = self.cmd_tx.send(Command::Revoke {
            remote_static: *remote_static,
        });
    }

    /// Await the next endpoint event, or `None` once the actor has stopped.
    pub async fn next_event(&mut self) -> Option<Event> {
        self.events.recv().await
    }
}

/// A per-connection handle: send payloads and close the connection.
#[derive(Clone)]
pub struct SessionHandle {
    conn: ConnId,
    cmd_tx: mpsc::UnboundedSender<Command>,
}

impl SessionHandle {
    /// This connection's id.
    pub fn id(&self) -> ConnId {
        self.conn
    }

    /// Queue `payload` for **reliable, unordered** delivery: it is framed as a
    /// DATA message, retransmitted until acknowledged, and surfaces on the
    /// peer exactly once — while the connection lives (an undelivered message
    /// dies with it).
    ///
    /// Rejects an oversize message synchronously
    /// ([`SlitherError::PayloadTooLarge`] past [`MAX_MESSAGE`]) and reports a
    /// stopped actor as [`SlitherError::EndpointClosed`]. Success means
    /// *queued*, not *delivered*.
    pub fn send(&self, payload: impl Into<Vec<u8>>) -> Result<(), SlitherError> {
        let payload = payload.into();
        if payload.len() > MAX_MESSAGE {
            return Err(SlitherError::PayloadTooLarge {
                len: payload.len(),
                max: MAX_MESSAGE,
            });
        }
        self.cmd_tx
            .send(Command::Send {
                conn: self.conn,
                payload,
            })
            .map_err(|_| SlitherError::EndpointClosed)
    }

    /// Close this connection (no [`Event::Dead`] is emitted — the caller knows).
    pub fn close(&self) {
        let _ = self.cmd_tx.send(Command::Close { conn: self.conn });
    }
}

// ── Actor internals ───────────────────────────────────────────────────────────

enum Command {
    Connect {
        conn: ConnId,
        remote_addr: SocketAddr,
        remote_static: P256r1PublicKey,
    },
    Send {
        conn: ConnId,
        payload: Vec<u8>,
    },
    Close {
        conn: ConnId,
    },
    Allow {
        remote_static: P256r1PublicKey,
    },
    Revoke {
        remote_static: P256r1PublicKey,
    },
}

/// A handshake in flight where we are the initiator (an initial connect or a
/// rekey).
struct PendingConnect<I: Identity> {
    remote_addr: SocketAddr,
    remote_static: P256r1PublicKey,
    /// The current attempt's typed await-msg2 state — `None` once consumed by
    /// a completion attempt (the next retransmit refreshes it).
    attempt: Option<InitiatorPending<I::Provider>>,
    current_index: u32,
    started_at: Instant,
    next_retransmit: Instant,
    is_rekey: bool,
    /// Payloads sent before the initial handshake completed, flushed on
    /// establishment.
    queued: Vec<Vec<u8>>,
}

struct Actor<I: Identity, W: Wire> {
    identity: I,
    wire: W,
    rng: ChaCha20Rng,
    allow: HashSet<[u8; 33]>,
    persistent_keepalive: Option<Duration>,
    ts_guard: TimestampGuard,
    /// The last initiation timestamp this endpoint emitted — kept so every fresh
    /// initiation is strictly greater, even when the coarse wall clock has not
    /// advanced between two rapid retransmits.
    last_init_timestamp: Option<Timestamp>,

    sessions: HashMap<ConnId, Session>,
    /// The Leg 2 recovery state, one per live connection, sibling to its
    /// [`Session`] (the ruled placement: the session stays pure Leg 1, and the
    /// reliable-message state must outlive a rekey's session swap).
    recovery: HashMap<ConnId, Recovery>,
    index_to_conn: HashMap<u32, ConnId>,
    static_to_conn: HashMap<[u8; 33], ConnId>,
    pending: HashMap<ConnId, PendingConnect<I>>,
    pending_by_index: HashMap<u32, ConnId>,
    next_inbound: u64,

    cmd_rx: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
}

impl<I: Identity, W: Wire> Actor<I, W> {
    async fn run(mut self) {
        let mut tick = time::interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut buf = vec![0u8; 2048];
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => self.handle_command(cmd).await,
                        None => break, // every handle dropped
                    }
                }
                received = self.wire.recv_from(&mut buf) => {
                    match received {
                        Ok((n, src)) => {
                            let packet = buf[..n].to_vec();
                            self.handle_inbound(&packet, src).await;
                        }
                        Err(error) => {
                            tracing::debug!(%error, "slither: recv_from failed");
                        }
                    }
                }
                _ = tick.tick() => self.on_tick().await,
            }
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    async fn flush(&self, outbox: Vec<(SocketAddr, Vec<u8>)>) {
        for (addr, bytes) in outbox {
            if let Err(error) = self.wire.send_to(&bytes, addr).await {
                tracing::debug!(%addr, %error, "slither: send_to failed");
            }
        }
    }

    fn retransmit_delay(&mut self) -> Duration {
        let span = RETRANSMIT_JITTER_MAX.as_nanos() as u64;
        let jitter = (self.rng.next_u32() as u64) % (span + 1);
        RETRANSMIT_BASE + Duration::from_nanos(jitter)
    }

    /// The next initiation timestamp — the wall clock, forced strictly past the
    /// previous one so the responder's greatest-timestamp guard always admits a
    /// genuine fresh attempt.
    fn next_timestamp(&mut self) -> Timestamp {
        let now = Timestamp::now();
        let timestamp = match self.last_init_timestamp {
            Some(prev) if now <= prev => {
                if prev.nanos + 1 >= 1_000_000_000 {
                    Timestamp {
                        secs: prev.secs + 1,
                        nanos: 0,
                    }
                } else {
                    Timestamp {
                        secs: prev.secs,
                        nanos: prev.nanos + 1,
                    }
                }
            }
            _ => now,
        };
        self.last_init_timestamp = Some(timestamp);
        timestamp
    }

    async fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::Connect {
                conn,
                remote_addr,
                remote_static,
            } => {
                self.begin_connect(conn, remote_addr, remote_static, false)
                    .await
            }
            Command::Send { conn, payload } => self.handle_send(conn, payload).await,
            Command::Close { conn } => self.teardown(conn, false),
            Command::Allow { remote_static } => {
                self.allow.insert(remote_static.to_compressed());
            }
            Command::Revoke { remote_static } => {
                let key = remote_static.to_compressed();
                self.allow.remove(&key);
                if let Some(&conn) = self.static_to_conn.get(&key) {
                    self.teardown(conn, true);
                }
            }
        }
    }

    async fn begin_connect(
        &mut self,
        conn: ConnId,
        remote_addr: SocketAddr,
        remote_static: P256r1PublicKey,
        is_rekey: bool,
    ) {
        let now = Instant::now();
        let index = random_index(&mut self.rng);
        let timestamp = self.next_timestamp();
        let provider = self.identity.provider();
        let secret = self.identity.static_secret();
        let (packet, pending) =
            match handshake::build_init(provider, secret, remote_static, index, timestamp) {
                Ok(built) => built,
                Err(error) => {
                    tracing::debug!(%error, "slither: build_init failed");
                    return;
                }
            };
        let delay = self.retransmit_delay();
        self.pending.insert(
            conn,
            PendingConnect {
                remote_addr,
                remote_static,
                attempt: Some(pending),
                current_index: index,
                started_at: now,
                next_retransmit: now + delay,
                is_rekey,
                queued: Vec::new(),
            },
        );
        self.pending_by_index.insert(index, conn);
        self.flush(vec![(remote_addr, packet)]).await;
    }

    /// Enforce the payload age gate on `conn` before application payload (a
    /// fresh send or a DATA retransmission) is sealed under its session.
    ///
    /// Age gates payload, not liveness (the maintainer, 2026/07/17): a session
    /// past [`REJECT_AGE`](crate::session::REJECT_AGE) that has failed to
    /// complete its DH rekey may not seal payload and is torn down here (the
    /// backstop); a session past [`REKEY_AGE`](crate::session::REKEY_AGE) begins
    /// a fresh handshake under the same `conn` (the silent swap — no
    /// `Established` re-emission), while the old session keeps sealing on its own
    /// counters until the swap establishes or it expires. Returns `false` when
    /// the session was torn down (the caller must not seal). Control traffic —
    /// keepalives, ACKs, and PTO probes — never calls this and stays age-exempt.
    ///
    /// The caller must have already confirmed a session exists for `conn`.
    async fn gate_payload(&mut self, conn: ConnId, now: Instant) -> bool {
        // Backstop: payload may not ride an over-age session that failed to
        // complete its DH rekey — tear it down.
        if self.sessions.get(&conn).is_some_and(|s| s.is_expired(now)) {
            self.teardown(conn, true);
            return false;
        }
        // Rekey trigger (>= REKEY_AGE): start a fresh handshake under the same
        // conn; the old session keeps sealing until the swap or expiry.
        let rekey = self.sessions.get(&conn).is_some_and(|s| s.needs_rekey(now))
            && !self.pending.contains_key(&conn);
        if rekey {
            let (addr, remote_static) = {
                let session = self.sessions.get(&conn).expect("checked");
                (session.endpoint(), *session.remote_static())
            };
            self.begin_connect(conn, addr, remote_static, true).await;
        }
        true
    }

    async fn handle_send(&mut self, conn: ConnId, payload: Vec<u8>) {
        let now = Instant::now();

        // A fresh application send is payload: enforce the age gate before it is
        // sealed (the rekey fires at REKEY_AGE; the REJECT_AGE backstop tears the
        // session down). Idle liveness and control traffic never reach here, so
        // they stay age-exempt.
        if self.sessions.contains_key(&conn) && !self.gate_payload(conn, now).await {
            return; // torn down at the backstop
        }

        if self.sessions.contains_key(&conn) {
            // Queue the message for reliable delivery and drain what is
            // pending — the first transmission usually rides out right here.
            if let Some(recovery) = self.recovery.get_mut(&conn) {
                recovery.queue_message(payload);
                self.pump(conn).await;
            } else {
                tracing::debug!("slither: session without recovery state");
            }
        } else if let Some(pending) = self.pending.get_mut(&conn) {
            // Early send before the initial handshake completed.
            pending.queued.push(payload);
        } else {
            tracing::debug!("slither: send to unknown connection");
        }
    }

    /// Drain the connection's pending frames into sealed packets and send them:
    /// each packet coalesces the owed ACK, queued and retransmitted DATA, and
    /// a PING probe where one is due. A packet carrying fresh (first-
    /// transmission) application data marks the Leg 1 liveness clock; pure
    /// control packets (ACKs, probes, retransmissions) are sealed quietly.
    async fn pump(&mut self, conn: ConnId) {
        let mut outbox = Vec::new();
        {
            let (Some(session), Some(recovery)) =
                (self.sessions.get_mut(&conn), self.recovery.get_mut(&conn))
            else {
                return;
            };
            let now = Instant::now();
            while let Some(plan) = recovery.next_packet(now, session.ack_window()) {
                let sealed = if plan.fresh {
                    session.seal(&plan.plaintext, now)
                } else {
                    session.seal_quiet(&plan.plaintext)
                };
                match sealed {
                    Ok(packet) => {
                        // The packet's counter is in its own header — read it
                        // back rather than widening the Leg 1 seal signature.
                        if let Some((header, _)) = DataHeader::parse(&packet) {
                            recovery.on_packet_sent(header.counter(), &plan, now);
                        }
                        outbox.push((session.endpoint(), packet));
                    }
                    Err(error) => {
                        tracing::debug!(%error, "slither: seal failed");
                        break;
                    }
                }
            }
        }
        self.flush(outbox).await;
    }

    async fn handle_inbound(&mut self, packet: &[u8], src: SocketAddr) {
        if packet.len() > MAX_DATAGRAM {
            return; // oversize receive: silent drop
        }
        match wire::classify(packet) {
            None => {} // unknown version/type/reserved/short: silent drop
            Some(PacketKind::HandshakeInit) => self.on_init(packet, src).await,
            Some(PacketKind::HandshakeResp) => self.on_resp(packet, src).await,
            Some(PacketKind::Data) => self.on_data(packet, src).await,
        }
    }

    async fn on_init(&mut self, packet: &[u8], src: SocketAddr) {
        let now = Instant::now();
        let provider = self.identity.provider();
        let own_public = self.identity.static_public();
        let secret = self.identity.static_secret();
        // The allow-list rides into the read's verification closure — it fires
        // before `ss`, so an unlisted initiator costs one DH (the ruling,
        // 2026/07/16); it also stays ahead of the timestamp guard by
        // construction, bounding the guard map to permitted statics.
        let accept = match handshake::accept_init(
            provider,
            &own_public,
            secret,
            packet,
            &self.allow,
        ) {
            Ok(accept) => accept,
            Err(HsError::Unlisted) => {
                tracing::debug!(target: "slither::policy", "unlisted initiator static rejected");
                return; // rejected inside the read: no `ss`, no msg2
            }
            // mac1 gate / malformed / garbage msg1 / tampered payload / forged
            // static claim: silent drop
            Err(_) => return,
        };
        let initiator_static = *accept.initiator_static();
        let key = initiator_static.to_compressed();
        if !self.ts_guard.admit(&initiator_static, accept.timestamp()) {
            tracing::debug!(target: "slither::replay", "initiation replay rejected");
            return; // drop, no msg2
        }
        let responder_index = random_index(&mut self.rng);
        let (resp_packet, established) = match accept.accept(responder_index) {
            Ok(built) => built,
            Err(error) => {
                tracing::debug!(%error, "slither: responder finish failed");
                return;
            }
        };
        let (conn, emit) = match self.static_to_conn.get(&key).copied() {
            Some(existing) => (existing, false), // replace an existing peer session, silently
            None => {
                let conn = ConnId(self.next_inbound);
                self.next_inbound += 1;
                (conn, true)
            }
        };
        let session = Session::new(established, src, now);
        self.install_session(conn, session, emit);
        self.flush(vec![(src, resp_packet)]).await;
        // A replacement (same peer, fresh msg1) reset the recovery epoch and
        // re-queued the undelivered messages — land them on the new session.
        self.pump(conn).await;
    }

    async fn on_resp(&mut self, packet: &[u8], src: SocketAddr) {
        let _ = src;
        let now = Instant::now();
        if packet.len() != RESP_PACKET_LEN {
            return;
        }
        let header = match View::<RespHeader>::try_from_slice(&packet[..RespHeader::SIZE]) {
            Ok(view) => view.unpack(),
            Err(_) => return,
        };
        let conn = match self.pending_by_index.get(&header.receiver_index()).copied() {
            Some(conn) => conn,
            None => return, // no matching in-flight attempt
        };
        let attempt = match self
            .pending
            .get_mut(&conn)
            .and_then(|pending| pending.attempt.take())
        {
            Some(attempt) => attempt,
            None => return, // already consumed this interval
        };
        let own_public = self.identity.static_public();
        // On a forged or tampered response `complete_init` fails; the attempt is
        // spent, and the next retransmit refreshes a fresh initiation.
        if let Ok(established) = handshake::complete_init(attempt, &own_public, packet) {
            let pending = self.pending.remove(&conn).expect("present");
            self.pending_by_index.remove(&pending.current_index);
            let emit = !self.sessions.contains_key(&conn);
            let session = Session::new(established, pending.remote_addr, now);
            self.install_session(conn, session, emit);

            // Queue any payloads sent before this initial handshake finished,
            // then drain: a rekey's epoch reset also lands its re-queued
            // messages on the fresh session here.
            if let Some(recovery) = self.recovery.get_mut(&conn) {
                for payload in pending.queued {
                    recovery.queue_message(payload);
                }
            }
            self.pump(conn).await;
        }
    }

    async fn on_data(&mut self, packet: &[u8], src: SocketAddr) {
        let now = Instant::now();
        let (header, _) = match DataHeader::parse(packet) {
            Some(parsed) => parsed,
            None => return,
        };
        let conn = match self.index_to_conn.get(&header.receiver_index()).copied() {
            Some(conn) => conn,
            None => return, // Data for no known session
        };
        // Opening is age-exempt (age gates payload, not liveness — the
        // maintainer, 2026/07/17): an idle session past REJECT_AGE keeps opening
        // inbound packets, so the keepalive dance sustains it indefinitely; only
        // the payload-seal path consults the REJECT_AGE backstop.
        let result = {
            let session = self.sessions.get_mut(&conn).expect("present");
            session.open(packet, src, now)
        };
        let remote_static = match self.sessions.get(&conn) {
            Some(session) => *session.remote_static(),
            None => return,
        };
        // A forged, tampered, or short packet is `Err` — a silent drop.
        if let Ok(received) = result {
            if let Some(from) = received.moved_from {
                self.emit(Event::EndpointMoved {
                    conn,
                    remote_static,
                    from,
                    to: src,
                });
            }
            // A fresh, non-empty plaintext is a frame sequence (an empty one is
            // the Leg 1 keepalive, which bypasses the frame layer; a replayed
            // duplicate never gets here).
            if let Some(plaintext) = received.payload {
                self.on_frames(conn, remote_static, header.counter(), &plaintext)
                    .await;
            }
        }
    }

    /// Process one authenticated packet's frame sequence: deliver fresh DATA
    /// (deduplicated to exactly-once by sequence), fold ACKs into recovery,
    /// and — if anything aboard was ack-eliciting — owe the peer an immediate
    /// ACK. A malformed stream fails the whole packet: it was authenticated,
    /// so a parse error is a protocol violation, not line noise.
    async fn on_frames(
        &mut self,
        conn: ConnId,
        remote_static: P256r1PublicKey,
        counter: u64,
        plaintext: &[u8],
    ) {
        let frames = match frame::decode_all(plaintext) {
            Ok(frames) => frames,
            Err(error) => {
                tracing::debug!(
                    target: "slither::frames",
                    %error,
                    "malformed frame stream in an authenticated packet; dropped"
                );
                return;
            }
        };
        let now = Instant::now();
        let highest_sent = self.sessions.get(&conn).and_then(Session::last_counter);
        let Some(recovery) = self.recovery.get_mut(&conn) else {
            return;
        };
        recovery.note_received(counter, now);
        let mut ack_eliciting = false;
        let mut incoming = Vec::new();
        for frame in frames {
            match frame {
                Frame::Padding => {}
                Frame::Ping => ack_eliciting = true,
                Frame::Ack(ack) => recovery.on_ack(&ack, highest_sent, now),
                Frame::Data(data) => {
                    ack_eliciting = true;
                    if recovery.deliver(data.seq) {
                        incoming.push(data.payload);
                    }
                }
            }
        }
        if ack_eliciting {
            recovery.mark_ack_pending();
        }
        for payload in incoming {
            self.emit(Event::Incoming {
                conn,
                remote_static,
                payload,
            });
        }
        // Send the owed ACK at once (coalesced with anything pending), and any
        // retransmissions an ACK's loss detection just queued. A re-queued
        // DATA is application payload sealed on a fresh counter, so this pump
        // is a payload-seal path whenever loss detection re-queued anything —
        // enforce the age gate first, exactly as the tick's retransmit path
        // does. A pure-ACK pump stays age-exempt.
        let retransmit_payload = self
            .recovery
            .get(&conn)
            .is_some_and(|recovery| recovery.has_retransmittable());
        if retransmit_payload && !self.gate_payload(conn, Instant::now()).await {
            return; // torn down at the backstop
        }
        self.pump(conn).await;
    }

    async fn on_tick(&mut self) {
        let now = Instant::now();
        let mut outbox = Vec::new();
        let mut dead = Vec::new();

        for (&conn, session) in self.sessions.iter_mut() {
            // Liveness is the only idle killer (age gates payload, not liveness —
            // the maintainer, 2026/07/17): a session sustained by the keepalive
            // dance lives indefinitely; only DEAD_TIMEOUT silence kills it here.
            // The REJECT_AGE backstop lives on the payload-seal path, never this
            // idle tick.
            if session.is_dead(now) {
                dead.push(conn);
                continue;
            }
            if session.should_keepalive(now, self.persistent_keepalive)
                && let Ok(bytes) = session.seal(&[], now)
            {
                outbox.push((session.endpoint(), bytes));
            }
        }

        let mut giveups = Vec::new();
        let mut retransmits = Vec::new();
        for (&conn, pending) in self.pending.iter() {
            if now.saturating_duration_since(pending.started_at) >= HANDSHAKE_GIVEUP {
                giveups.push(conn);
            } else if now >= pending.next_retransmit {
                retransmits.push(conn);
            }
        }

        self.flush(outbox).await;
        for conn in dead {
            self.teardown(conn, true);
        }
        for conn in giveups {
            self.give_up(conn);
        }
        for conn in retransmits {
            self.retransmit(conn, now).await;
        }

        // Leg 2 recovery timers, evaluated on the same tick: the RFC 9002
        // loss-detection deadline first, then the PTO (the loss timer takes
        // precedence). Both queue frames; one pump drains them.
        let conns: Vec<ConnId> = self.recovery.keys().copied().collect();
        for conn in conns {
            if !self.sessions.contains_key(&conn) {
                continue;
            }
            // RFC 9002 §6.2 fires ONE timer per event, the earlier of the two:
            // the loss-detection timer (always the sooner — a fraction of the
            // RTT) takes precedence, and the PTO is re-evaluated on the next
            // tick if still due. Firing both in one tick would escalate the PTO
            // backoff spuriously while loss detection was already handling the
            // in-flight packets.
            let (due, retransmit_payload) = {
                let Some(recovery) = self.recovery.get_mut(&conn) else {
                    continue;
                };
                let mut due = false;
                if recovery.loss_time_due(now) {
                    recovery.detect_lost(now);
                    due = true;
                } else if recovery.pto_due(now) {
                    recovery.on_pto();
                    due = true;
                }
                (due, recovery.has_retransmittable())
            };
            if !due {
                continue;
            }
            // A queued DATA retransmission is application payload sealed on a
            // fresh counter, so the retransmit path is a payload-seal path:
            // enforce the age gate here too. A retransmitting-but-not-sending
            // session must still rekey at REKEY_AGE and hit the REJECT_AGE
            // backstop if the rekey cannot complete. A bare PTO PING probe
            // carries no payload and is age-exempt.
            if retransmit_payload && !self.gate_payload(conn, now).await {
                continue; // torn down at the backstop; nothing to pump
            }
            if self.sessions.contains_key(&conn) {
                self.pump(conn).await;
            }
        }
    }

    async fn retransmit(&mut self, conn: ConnId, now: Instant) {
        let (remote_addr, remote_static, old_index) = match self.pending.get(&conn) {
            Some(pending) => (
                pending.remote_addr,
                pending.remote_static,
                pending.current_index,
            ),
            None => return,
        };
        let index = random_index(&mut self.rng);
        let timestamp = self.next_timestamp();
        let provider = self.identity.provider();
        let secret = self.identity.static_secret();
        let (packet, fresh) =
            match handshake::build_init(provider, secret, remote_static, index, timestamp) {
                Ok(built) => built,
                Err(error) => {
                    tracing::debug!(%error, "slither: retransmit build_init failed");
                    return;
                }
            };
        let delay = self.retransmit_delay();
        self.pending_by_index.remove(&old_index);
        self.pending_by_index.insert(index, conn);
        if let Some(pending) = self.pending.get_mut(&conn) {
            pending.attempt = Some(fresh);
            pending.current_index = index;
            pending.next_retransmit = now + delay;
        }
        self.flush(vec![(remote_addr, packet)]).await;
    }

    fn give_up(&mut self, conn: ConnId) {
        if let Some(pending) = self.pending.remove(&conn) {
            self.pending_by_index.remove(&pending.current_index);
            // Only an INITIAL connect surfaces a failure; a rekey give-up leaves
            // the still-usable old session and stays quiet.
            if !pending.is_rekey {
                self.emit(Event::Failed {
                    conn,
                    error: ConnectError::TimedOut(HANDSHAKE_GIVEUP),
                });
            }
        }
    }

    fn install_session(&mut self, conn: ConnId, session: Session, emit_established: bool) {
        if let Some(old) = self.sessions.remove(&conn)
            && self.index_to_conn.get(&old.our_index()) == Some(&conn)
        {
            self.index_to_conn.remove(&old.our_index());
        }
        let our_index = session.our_index();
        let remote_static = *session.remote_static();
        let endpoint = session.endpoint();
        self.index_to_conn.insert(our_index, conn);
        self.static_to_conn
            .insert(remote_static.to_compressed(), conn);
        self.sessions.insert(conn, session);
        // Fresh connection ⇒ fresh recovery; a swap (rekey, or a responder-side
        // replacement) restarts the counter space, so the recovery epoch resets
        // and every undelivered message re-queues for the new session.
        match self.recovery.get_mut(&conn) {
            Some(recovery) => recovery.epoch_reset(),
            None => {
                self.recovery.insert(conn, Recovery::new());
            }
        }
        if emit_established {
            self.emit(Event::Established {
                conn,
                remote_static,
                endpoint,
            });
        }
    }

    fn teardown(&mut self, conn: ConnId, emit_dead: bool) {
        // Undelivered reliable messages die with the connection (the ruled
        // Leg 2 lifecycle: reliability lives within the connection).
        self.recovery.remove(&conn);
        if let Some(session) = self.sessions.remove(&conn) {
            if self.index_to_conn.get(&session.our_index()) == Some(&conn) {
                self.index_to_conn.remove(&session.our_index());
            }
            let key = session.remote_static().to_compressed();
            if self.static_to_conn.get(&key) == Some(&conn) {
                self.static_to_conn.remove(&key);
            }
            if emit_dead {
                self.emit(Event::Dead {
                    conn,
                    remote_static: *session.remote_static(),
                });
            }
        }
        if let Some(pending) = self.pending.remove(&conn) {
            self.pending_by_index.remove(&pending.current_index);
        }
    }
}

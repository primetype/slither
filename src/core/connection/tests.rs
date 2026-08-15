//! Slice 3a acceptance tests for [`Connection`].
//!
//! Written by TEST-A **from `SPEC.md` and `STORIES.md` alone**, in parallel
//! with the implementation and without reading it (CLAUDE.md working rule 6).
//!
//! # How these tests are built, and why
//!
//! Almost everything here asserts on **bytes and on `ConnOutput`**, never on
//! an internal type. The connection under test is a real
//! [`Connection`] holding a real hiss session; its peer is the *other* half
//! of the same handshake, held raw by the harness so a test can seal an
//! arbitrary frame stream and open whatever slither sends back. That makes
//! every §8 assertion a wire assertion: a codec that round-trips its own
//! private `Frame` type but writes the wrong bytes fails here.
//!
//! Two exceptions are unavoidable and are marked where they occur — the
//! §8.7 ack-eliciting classifier and the §16.5 timer table are, by
//! `PLAN.md` §9's own argument (T3, T6), *unreachable* through the
//! connection's behaviour in slice 3, because slice 3 arms one timer and
//! builds only "never"-class frames. Those two are unit tests against the
//! names `PLAN.md` §2.1/§9 gives the modules. **If integration renames
//! them, rename the call — never the assertion.**
//!
//! # Organisation
//!
//! - `harness` — the two-session fixture and the hand-rolled wire
//! - `counter` — §7.1 the counter is the packet number
//! - `replay` — §7.2 the anti-replay window
//! - `liveness` — §7.4's two seal paths and the install pin
//! - `ratchet` — §7.7 the epoch ratchet (S23)
//! - `one_session` — §7.8
//! - `exhaustion` — §7.9
//! - `codec` — §8 the four frames slice 3a has
//! - `teardown` — §15 CLOSE, closing, draining, the registry
//! - `poll_contract` — §16.4
//! - `timers` — §16.5
//! - `seal_order` — §16.7 plan-seal-commit
//!
//! # What slice 3a is owed by later slices
//!
//! Recorded at the end of the file in `owed`, as doc comments on
//! `#[ignore]`-free compile-time notes, so a half-assertion never reads as
//! a pin.

#![allow(clippy::items_after_statements)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU64;
use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

use super::*;

use crate::config::Config;
use crate::constants::{
    AEAD_TAG_LEN, APPLICATION_ERROR_BASE, CLOSE_LINGER, CLOSE_REASON_MAX, CLOSE_REPLY_MIN_INTERVAL,
    DATA_HEADER_LEN, DEAD_TIMEOUT, FRAME_ACK, FRAME_CLOSE, FRAME_DATAGRAM, FRAME_DATAGRAM_LEN,
    FRAME_MAX_DATA, FRAME_MAX_STREAM_DATA, FRAME_MAX_STREAMS_BIDI, FRAME_MAX_STREAMS_UNI,
    FRAME_PADDING, FRAME_PING, FRAME_RESET_STREAM, FRAME_STOP_SENDING_RESERVED, FRAME_STREAM_BASE,
    FRAME_STREAM_MAX, MAX_ACK_RANGES, MAX_DATAGRAM, MAX_PLAINTEXT, PKT_DATA, PROLOGUE,
    REKEY_EPOCH_MSGS, REPLAY_WINDOW, VERSION,
};
use crate::core::{EstablishedSession, Install, Role, ToEndpoint, Transmit};
use crate::error::ConnectionLost;
use crate::identity::Identity;
use crate::packet::{Handshake, ReferenceSuite};
use crate::testutil::CountingIdentity;
use crate::varint::{self, VarInt};

type Suite = ReferenceSuite;
type Id = CountingIdentity<Suite>;

// ═══════════════════════════════════════════════════════════════════════
// Harness
// ═══════════════════════════════════════════════════════════════════════

/// Everything one `poll_output()` loop produced, plus the deadline it ended
/// on.
///
/// Collected into a `Vec` rather than matched one at a time because §16.4
/// makes **generation order** normative: `outs[0]` before `outs[1]` is
/// testing a rule, not an implementation detail.
#[derive(Debug, Default)]
struct Drained {
    outs: Vec<ConnOutput>,
    deadline: Option<Instant>,
}

impl Drained {
    fn transmits(&self) -> Vec<Transmit> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                ConnOutput::Transmit(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    fn events(&self) -> Vec<&ConnEvent> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                ConnOutput::Event(e) => Some(e),
                _ => None,
            })
            .collect()
    }

    fn closed(&self) -> Vec<ConnectionLost> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                ConnOutput::Event(ConnEvent::Closed(l)) => Some(l.clone()),
                _ => None,
            })
            .collect()
    }

    fn retired(&self) -> Vec<u32> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                ConnOutput::ToEndpoint(ToEndpoint::Retired { our_index }) => Some(*our_index),
                _ => None,
            })
            .collect()
    }

    /// The single transmit this drain produced. Panics on zero or two — a
    /// count assertion phrased as an extractor.
    fn one_transmit(&self) -> Transmit {
        let v = self.transmits();
        assert_eq!(v.len(), 1, "expected exactly one Transmit, got {}", v.len());
        v[0].clone()
    }

    /// Nothing but the terminal `Timeout`.
    fn is_silent(&self) -> bool {
        self.outs.is_empty()
    }

    /// Index of the first output matching `f`, for ordering assertions.
    fn position(&self, f: impl Fn(&ConnOutput) -> bool) -> Option<usize> {
        self.outs.iter().position(f)
    }
}

/// The raw half of the session the connection under test is talking to.
///
/// This is the *peer*: a bare hiss datagram pair plus the two indices, held
/// outside any `Connection` so a test can seal a frame stream slice 3a has
/// no verb for (PING, ACK, an unknown type) and open whatever comes back.
struct Peer {
    seal: <Suite as Handshake>::Seal,
    open: <Suite as Handshake>::Open,
    /// The peer's own index — what the connection under test writes into a
    /// Data header's `receiver_index`.
    our_index: u32,
    /// The connection-under-test's index — what *we* write into a Data
    /// header's `receiver_index` to reach it.
    peer_index: u32,
    /// Where the peer speaks from.
    addr: SocketAddr,
}

impl Peer {
    /// Seal `plaintext` into a complete §3.4 Data packet addressed to the
    /// connection under test.
    ///
    /// §3.4: the 14 header bytes **are** the associated data, verbatim.
    fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let counter = self.seal.next_counter();
        let mut dgram = data_header(self.peer_index, counter);
        let mut body = vec![0u8; plaintext.len() + AEAD_TAG_LEN];
        let (got, n) = self
            .seal
            .encrypt_next(&dgram, plaintext, &mut body)
            .expect("peer seal");
        assert_eq!(got, counter, "next_counter() must predict the seal");
        body.truncate(n);
        dgram.extend_from_slice(&body);
        dgram
    }

    /// The counter the peer's next seal will use.
    fn next_counter(&self) -> u64 {
        self.seal.next_counter()
    }

    /// Seal `plaintext` so that it lands on exactly `counter`, burning (and
    /// discarding) the packets in between.
    ///
    /// hiss owns the counter and only ever moves it forward (§7.1), so a
    /// test that needs a packet at a chosen counter has to seal it *when*
    /// that counter comes up and hold the bytes. Discarded packets are
    /// never delivered, so the receiver never sees them.
    fn seal_at(&mut self, counter: u64, plaintext: &[u8]) -> Vec<u8> {
        assert!(
            counter >= self.seal.next_counter(),
            "the counter only goes forward (§7.1)"
        );
        while self.seal.next_counter() < counter {
            let _ = self.seal(&padding(1));
        }
        self.seal(plaintext)
    }

    /// Open a datagram the connection under test emitted, returning its
    /// frame-stream plaintext. Panics if it does not open — which is the
    /// assertion most callers actually want.
    fn open(&mut self, dgram: &[u8]) -> Vec<u8> {
        let (header, body) = dgram.split_at(DATA_HEADER_LEN);
        assert_eq!(header[0], PKT_DATA, "§3.4 packet type");
        assert_eq!(header[1], VERSION, "§3.4 version");
        assert_eq!(
            u32::from_le_bytes(header[2..6].try_into().unwrap()),
            self.our_index,
            "§3.4 receiver_index routes to the peer's own index"
        );
        let counter = u64::from_le_bytes(header[6..14].try_into().unwrap());
        let mut out = vec![0u8; body.len()];
        let n = self
            .open
            .decrypt_at(counter, header, body, &mut out)
            .expect("the packet must open");
        out.truncate(n);
        out
    }

    /// The counter in a datagram's cleartext header (§3.4, §7.1).
    fn counter_of(dgram: &[u8]) -> u64 {
        u64::from_le_bytes(dgram[6..14].try_into().expect("data header"))
    }
}

/// §3.4's 14 cleartext bytes: `type ‖ version ‖ receiver_index ‖ counter`,
/// the multi-byte fields little-endian (§3.1, ruling 64).
///
/// Written out by hand rather than through `DataHeader` so that a change to
/// the header layout has to break a *test* that states the layout, not just
/// agree with itself.
fn data_header(receiver_index: u32, counter: u64) -> Vec<u8> {
    let mut h = Vec::with_capacity(DATA_HEADER_LEN);
    h.push(PKT_DATA);
    h.push(VERSION);
    h.extend_from_slice(&receiver_index.to_le_bytes());
    h.extend_from_slice(&counter.to_le_bytes());
    assert_eq!(h.len(), DATA_HEADER_LEN);
    h
}

/// The connection under test, with the peer that talks to it.
struct Fixture {
    conn: Connection<Suite>,
    peer: Peer,
    /// The connection's own session index — what `Retired` must carry.
    our_index: u32,
    /// The instant the session was installed. §7.4 pins both liveness
    /// clocks here.
    installed_at: Instant,
}

impl Fixture {
    /// Drain to the terminal `Timeout` (§16.4). The bound is not
    /// decoration: a core that re-emits for ever fails here as a named
    /// panic rather than as a CI hang.
    fn drain(&mut self) -> Drained {
        let mut d = Drained::default();
        for _ in 0..100_000 {
            match self.conn.poll_output() {
                ConnOutput::Timeout(t) => {
                    d.deadline = t;
                    return d;
                }
                other => d.outs.push(other),
            }
        }
        panic!("poll_output() did not reach the terminal Timeout in 100_000 outputs (§16.4)");
    }

    /// `handle_datagram` from the peer's address, then drain.
    fn feed(&mut self, now: Instant, dgram: &[u8]) -> Drained {
        let src = self.peer.addr;
        self.conn.handle_datagram(now, src, dgram);
        self.drain()
    }

    /// `handle_datagram` from an arbitrary source, then drain.
    fn feed_from(&mut self, now: Instant, src: SocketAddr, dgram: &[u8]) -> Drained {
        self.conn.handle_datagram(now, src, dgram);
        self.drain()
    }

    /// Seal `frames` as the peer and feed it, then drain.
    fn deliver(&mut self, now: Instant, frames: &[u8]) -> Drained {
        let dgram = self.peer.seal(frames);
        self.feed(now, &dgram)
    }

    fn timeout(&mut self, now: Instant) -> Drained {
        self.conn.handle_timeout(now);
        self.drain()
    }

    fn close(&mut self, now: Instant, code: u64, reason: &[u8]) -> Drained {
        self.conn.close(now, code, reason);
        self.drain()
    }

    /// The counter hiss will use for this connection's **next** seal.
    ///
    /// Read straight through §16.4's `session()` accessor and
    /// `EstablishedSession::seal`, both already public in `core::mod`. No
    /// test-only accessor is needed for it, which is what makes §16.7's
    /// "sealing is synchronous" observable at all (T4).
    fn next_counter(&self) -> u64 {
        self.conn
            .session()
            .expect("established")
            .seal
            .next_counter()
    }
}

// ── building the pair ──────────────────────────────────────────────────

const PEER_ADDR_OCTET: u8 = 2;

fn v4(a: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), port)
}

fn t0() -> Instant {
    Instant::now()
}

/// One complete IK handshake, run by hand off the [`Handshake`] trait, into
/// **both** raw datagram halves.
///
/// Done by hand rather than through two `Endpoint`s because `accept()`
/// swallows its session into a `Connection` and `Connection::session()` is
/// `&`-only, so there is no way to get a *sealable* peer out of the
/// endpoint path. This is the same crypto by the same calls.
fn handshake(epoch: NonZeroU64) -> (EstablishedSession<Suite>, Peer) {
    // Fixed indices: nothing in §7–§15 is keyed on their values, and fixed
    // values make a routing mistake read as a routing mistake.
    const A_INDEX: u32 = 0x1111_1111; // the connection under test
    const B_INDEX: u32 = 0x2222_2222; // the peer

    let a: Id = CountingIdentity::seeded([7u8; 32]);
    let b: Id = CountingIdentity::seeded([9u8; 32]);
    let a_pub = *a.public_static();
    let b_pub = *b.public_static();

    let (ap, ask) = a.open().expect("identity opens");
    let (bp, bsk) = b.open().expect("identity opens");

    let init = <Suite as Handshake>::initiator(ap, PROLOGUE, b_pub);
    let (msg1, sent) =
        <Suite as Handshake>::write_msg1(init, ask, &[0u8; crate::constants::MSG1_PAYLOAD_LEN])
            .expect("msg1");

    let resp = <Suite as Handshake>::responder(bp, PROLOGUE, bsk).expect("responder");
    let (claimed, mid) = <Suite as Handshake>::read_msg1_intro(resp, &msg1).expect("msg1 intro");
    assert_eq!(
        claimed.as_ref(),
        a_pub.as_ref(),
        "the claimed static is the initiator's"
    );
    let (_payload, read) = <Suite as Handshake>::complete(mid).expect("complete");
    let (msg2, b_transport) = <Suite as Handshake>::write_msg2(read).expect("msg2");
    let a_transport = <Suite as Handshake>::read_msg2(sent, &msg2).expect("read msg2");

    let (a_seal, a_open) = <Suite as Handshake>::into_datagram(a_transport, epoch);
    let (b_seal, b_open) = <Suite as Handshake>::into_datagram(b_transport, epoch);

    let peer_addr = v4(PEER_ADDR_OCTET, 2);
    (
        EstablishedSession {
            seal: a_seal,
            open: a_open,
            our_index: A_INDEX,
            peer_index: B_INDEX,
            anchor: peer_addr,
        },
        Peer {
            seal: b_seal,
            open: b_open,
            our_index: B_INDEX,
            peer_index: A_INDEX,
            addr: peer_addr,
        },
    )
}

/// The default epoch size, §7.7's ratified constant.
fn default_epoch() -> NonZeroU64 {
    NonZeroU64::new(REKEY_EPOCH_MSGS).expect("REKEY_EPOCH_MSGS is nonzero")
}

/// An established connection plus its peer, installed at `now`.
///
/// Built through **`connecting()` + `handle_endpoint_event(now, Install)`**
/// — §16.4's `connect()` path — rather than through
/// `Connection::established`, for one reason worth stating: `established()`
/// takes **no `Instant`**, and §7.4 requires the liveness clock to be
/// "pinned, and pinned *armed*" at install. Only this path has an instant
/// to pin it to. See `owed::ESTABLISHED_HAS_NO_INSTANT`.
fn established_at(now: Instant) -> Fixture {
    established_at_with_epoch(now, default_epoch())
}

/// As [`established_at`], at a caller-chosen epoch size (§7.7, ruling 82's
/// test-only facility).
fn established_at_with_epoch(now: Instant, epoch: NonZeroU64) -> Fixture {
    let (session, peer) = handshake(epoch);
    let our_index = session.our_index;
    let mut conn = Connection::connecting([0x5au8; 32]);
    conn.handle_endpoint_event(
        now,
        Install {
            session,
            role: Role::Initiator,
        },
    );
    let mut f = Fixture {
        conn,
        peer,
        our_index,
        installed_at: now,
    };
    let d = f.drain();
    assert!(
        d.events()
            .iter()
            .any(|e| matches!(e, ConnEvent::Established)),
        "§16.4: the install emits Established"
    );
    assert!(
        f.conn.is_established(),
        "§16.4: the install establishes the connection"
    );
    f
}

/// The `accept()`-shaped constructor: a connection that was **born**
/// established (§16.4 — "`accept()` returns a fully established connection
/// — never followed by an `Install`").
fn accepted_at(now: Instant) -> Fixture {
    let (session, peer) = handshake(default_epoch());
    let our_index = session.our_index;
    let conn = Connection::established(now, [0x5au8; 32], session, Role::Responder);
    let mut f = Fixture {
        conn,
        peer,
        our_index,
        installed_at: now,
    };
    let _ = f.drain();
    f
}

/// A `connect()`-shaped connection: created **before** its session exists,
/// as §16.4's `Install` path requires.
fn connecting() -> Connection<Suite> {
    Connection::connecting([0x5au8; 32])
}

/// Drain a bare `Connection` that has no fixture around it yet.
fn drain_bare(conn: &mut Connection<Suite>) -> Drained {
    let mut d = Drained::default();
    for _ in 0..100_000 {
        match conn.poll_output() {
            ConnOutput::Timeout(t) => {
                d.deadline = t;
                return d;
            }
            other => d.outs.push(other),
        }
    }
    panic!("poll_output() did not reach the terminal Timeout in 100_000 outputs (§16.4)");
}

/// One nanosecond — the unit every "not one instant before" boundary is
/// written in. Slice 1's lesson was a boundary tested on one side only.
const NS: Duration = Duration::from_nanos(1);

// ── frame-stream builders (§8.1, §8.4) ────────────────────────────────
//
// Every builder writes bytes, not a `Frame`. The point is that a codec that
// agrees with itself still has to agree with these.

fn vi(v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    varint::encode(VarInt::new(v).expect("fits the 62-bit space"), &mut out);
    out
}

/// A varint value forced into a **longer than minimal** encoding.
///
/// §8.1: "A sender emits the minimal encoding; a receiver accepts any
/// length (a non-minimal encoding is valid, as in QUIC)."
fn vi_padded(v: u64, len: usize) -> Vec<u8> {
    let (prefix, bytes) = match len {
        2 => (0b01u8, 2usize),
        4 => (0b10u8, 4),
        8 => (0b11u8, 8),
        _ => panic!("non-minimal widths are 2, 4 and 8"),
    };
    assert!(
        v < (1u64 << (bytes * 8 - 2)),
        "value does not fit the width"
    );
    let mut out = v.to_be_bytes()[8 - bytes..].to_vec();
    out[0] |= prefix << 6;
    out
}

fn padding(n: usize) -> Vec<u8> {
    vec![u8::try_from(FRAME_PADDING).unwrap(); n]
}

fn ping() -> Vec<u8> {
    vi(FRAME_PING)
}

/// `type ‖ largest ‖ ack_delay ‖ range_count ‖ first_range ‖ (gap, range)*`
fn ack(largest: u64, delay: u64, first_range: u64, pairs: &[(u64, u64)]) -> Vec<u8> {
    let mut f = vi(FRAME_ACK);
    f.extend(vi(largest));
    f.extend(vi(delay));
    f.extend(vi(pairs.len() as u64));
    f.extend(vi(first_range));
    for (gap, range) in pairs {
        f.extend(vi(*gap));
        f.extend(vi(*range));
    }
    f
}

/// `type ‖ error_code ‖ reason_len ‖ reason`
fn close_frame(code: u64, reason: &[u8]) -> Vec<u8> {
    let mut f = vi(FRAME_CLOSE);
    f.extend(vi(code));
    f.extend(vi(reason.len() as u64));
    f.extend_from_slice(reason);
    f
}

/// The decoded fields of a CLOSE frame that slither emitted.
#[derive(Debug, PartialEq, Eq)]
struct ParsedClose {
    code: u64,
    reason: Vec<u8>,
}

/// Parse a frame stream that slither produced, as a peer would.
///
/// Deliberately a *separate* implementation from the one under test, and
/// deliberately strict: it accepts only the four frames slice 3a has, so a
/// packet carrying anything else fails here loudly instead of being
/// skipped.
fn parse_frames(mut p: &[u8]) -> Vec<PeerFrame> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let (ty, n) = varint::decode(p).expect("a frame type");
        p = &p[n..];
        let ty = u64::from(ty);
        if ty == FRAME_PADDING {
            out.push(PeerFrame::Padding);
        } else if ty == FRAME_PING {
            out.push(PeerFrame::Ping);
        } else if ty == FRAME_ACK {
            let (largest, n) = varint::decode(p).expect("largest");
            p = &p[n..];
            let (delay, n) = varint::decode(p).expect("ack_delay");
            p = &p[n..];
            let (count, n) = varint::decode(p).expect("range_count");
            p = &p[n..];
            let (first, n) = varint::decode(p).expect("first_range");
            p = &p[n..];
            let mut pairs = Vec::new();
            for _ in 0..u64::from(count) {
                let (gap, n) = varint::decode(p).expect("gap");
                p = &p[n..];
                let (range, n) = varint::decode(p).expect("range");
                p = &p[n..];
                pairs.push((u64::from(gap), u64::from(range)));
            }
            out.push(PeerFrame::Ack {
                largest: u64::from(largest),
                delay: u64::from(delay),
                first_range: u64::from(first),
                pairs,
            });
        } else if ty == FRAME_CLOSE {
            let (code, n) = varint::decode(p).expect("error_code");
            p = &p[n..];
            let (len, n) = varint::decode(p).expect("reason_len");
            p = &p[n..];
            let len = usize::try_from(u64::from(len)).expect("reason fits");
            assert!(p.len() >= len, "reason overruns the plaintext");
            out.push(PeerFrame::Close(ParsedClose {
                code: u64::from(code),
                reason: p[..len].to_vec(),
            }));
            p = &p[len..];
        } else {
            panic!("slither emitted frame type {ty:#x}, which slice 3a has no business sending");
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
enum PeerFrame {
    Padding,
    Ping,
    Ack {
        largest: u64,
        delay: u64,
        first_range: u64,
        pairs: Vec<(u64, u64)>,
    },
    Close(ParsedClose),
}

/// The one CLOSE in a datagram slither sent, with everything else asserted
/// away.
fn one_close(peer: &mut Peer, dgram: &[u8]) -> ParsedClose {
    let pt = peer.open(dgram);
    let frames = parse_frames(&pt);
    let closes: Vec<_> = frames
        .into_iter()
        .filter_map(|f| match f {
            PeerFrame::Close(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(closes.len(), 1, "expected exactly one CLOSE frame");
    closes.into_iter().next().expect("one")
}

// ═══════════════════════════════════════════════════════════════════════
// §16.4 — the poll contract
// ═══════════════════════════════════════════════════════════════════════

mod poll_contract {
    use super::*;

    /// §16.4: *"every mutating call … is followed by draining
    /// `poll_output()` to the terminal `Timeout(Option<Instant>)`."*
    ///
    /// Scripted over **every** mutating verb the connection core has in
    /// slice 3a, because the contract is universal and a core that
    /// terminates for three of four is a core with one path that hangs the
    /// driver. `Fixture::drain` panics at 100 000 outputs, so a core that
    /// re-emits for ever fails here by name rather than as a CI hang.
    ///
    /// Catches: a `poll_output` arm that returns a non-terminal output
    /// unconditionally on any one path.
    #[test]
    fn the_drain_always_terminates_in_timeout() {
        let t = t0();

        // handle_endpoint_event
        let (session, mut peer) = handshake(default_epoch());
        let mut conn = Connection::connecting([1u8; 32]);
        conn.handle_endpoint_event(
            t,
            Install {
                session,
                role: Role::Initiator,
            },
        );
        let _ = drain_bare(&mut conn);

        // handle_datagram, with something that opens …
        let dgram = peer.seal(&padding(1));
        conn.handle_datagram(t, peer.addr, &dgram);
        let _ = drain_bare(&mut conn);

        // … and with something that does not
        conn.handle_datagram(t, peer.addr, b"not a slither packet at all");
        let _ = drain_bare(&mut conn);

        // handle_timeout
        conn.handle_timeout(t + Duration::from_secs(1));
        let _ = drain_bare(&mut conn);

        // close
        conn.close(t + Duration::from_secs(2), 0, b"bye");
        let _ = drain_bare(&mut conn);

        // and every one of them again on a connection with no session
        let mut bare = connecting();
        let _ = drain_bare(&mut bare);
        bare.handle_datagram(t, v4(9, 9), b"nothing");
        let _ = drain_bare(&mut bare);
        bare.handle_timeout(t);
        let _ = drain_bare(&mut bare);
        bare.close(t, 0, b"");
        let _ = drain_bare(&mut bare);
    }

    /// §16.4: the drain is exhaustive. A second drain with no mutating call
    /// in between must produce nothing but the sentinel — an output cannot
    /// be handed out twice.
    ///
    /// Catches: a `poll_output` that re-reads a field instead of consuming
    /// a queue (the CLOSE transmit would come out for ever).
    #[test]
    fn a_second_drain_with_no_mutating_call_yields_only_timeout() {
        let t = t0();
        let mut f = established_at(t);
        let first = f.close(t, 0, b"bye");
        assert!(!first.outs.is_empty(), "close() produces output");

        let second = f.drain();
        assert!(
            second.is_silent(),
            "a re-drain must be empty, got {:?}",
            second.outs
        );
        assert_eq!(
            second.deadline, first.deadline,
            "the announced deadline is stable across drains"
        );
    }

    /// §16.4: *"Output ordering within one drain preserves generation
    /// order — a transmit and the event it caused come out in that order.
    /// Normative."*
    ///
    /// Catches: a drain that emits events before transmits (e.g. two
    /// queues, drained events-first). A test that merely asserted "both are
    /// present" would pass that build.
    #[test]
    fn the_close_transmit_precedes_the_closed_event_it_caused() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.close(t, 0, b"bye");

        let tx = d
            .position(|o| matches!(o, ConnOutput::Transmit(_)))
            .expect("the CLOSE transmit");
        let ev = d
            .position(|o| matches!(o, ConnOutput::Event(ConnEvent::Closed(_))))
            .expect("the Closed event");
        assert!(
            tx < ev,
            "§16.4: the transmit precedes the event it caused ({tx} vs {ev})"
        );
    }

    /// §16.4: an `Install` "targets only a `connect()`-created connection
    /// awaiting completion, **exactly once**", and the accessors report the
    /// transition.
    ///
    /// Catches: an install that forgets to flip `is_established`, or that
    /// emits `Established` twice for one install.
    #[test]
    fn an_install_establishes_once_and_preserves_the_sub_seed() {
        let t = t0();
        let (session, _peer) = handshake(default_epoch());
        let index = session.our_index;
        let mut conn = Connection::connecting([0xa5u8; 32]);

        assert!(!conn.is_established(), "not established before the install");
        assert!(conn.session().is_none(), "no session before the install");
        assert_eq!(conn.sub_seed(), &[0xa5u8; 32], "§16.6's sub-seed is held");

        conn.handle_endpoint_event(
            t,
            Install {
                session,
                role: Role::Initiator,
            },
        );
        let d = drain_bare(&mut conn);

        let established = d
            .events()
            .iter()
            .filter(|e| matches!(e, ConnEvent::Established))
            .count();
        assert_eq!(established, 1, "§16.4: exactly one install, one event");
        assert!(conn.is_established());
        assert_eq!(
            conn.session().expect("a session").our_index,
            index,
            "the installed session is the one that was handed over"
        );
        assert_eq!(
            conn.sub_seed(),
            &[0xa5u8; 32],
            "§16.6: the sub-seed survives the install — it is the connection's, not the session's"
        );
    }

    /// A connection created by `connect()` has no session, so a datagram
    /// aimed at it can only be noise. §3.1's silent drop applies: no error,
    /// no trace, no output — and, above all, no panic.
    ///
    /// Catches: an `unwrap()` on `self.session` in `handle_datagram`.
    #[test]
    fn a_datagram_before_the_install_is_silent() {
        let t = t0();
        let mut conn = connecting();
        conn.handle_datagram(t, v4(9, 9), &data_header(1, 0));
        let d = drain_bare(&mut conn);
        assert!(d.is_silent(), "silent drop, got {:?}", d.outs);
        assert_eq!(d.deadline, None, "nothing is armed on a bare connection");
        assert!(!conn.is_established());
    }

    /// A datagram shorter than a Data header, and one that is not a Data
    /// packet at all, are both silent drops (§3.1) on an *established*
    /// connection too.
    ///
    /// Catches: slicing `datagram[..DATA_HEADER_LEN]` without a length
    /// check — a panic reachable from the network.
    #[test]
    fn a_runt_or_mistyped_datagram_is_a_silent_drop() {
        let t = t0();
        let mut f = established_at(t);

        for bad in [
            vec![],
            vec![PKT_DATA],
            vec![PKT_DATA, VERSION],
            data_header(f.our_index, 0)[..DATA_HEADER_LEN - 1].to_vec(),
            data_header(f.our_index, 0), // header only: no tag, cannot open
        ] {
            let d = f.feed(t, &bad);
            assert!(
                d.is_silent(),
                "a malformed datagram must drop silently, got {:?}",
                d.outs
            );
        }
        assert!(f.conn.is_established(), "and must not kill the connection");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.1 — the counter is the packet number
// ═══════════════════════════════════════════════════════════════════════

mod counter {
    use super::*;

    /// §7.1: *"the counter runs from 0 at establishment and never
    /// restarts"*. The first seal of a connection's life is counter 0, and
    /// slice 3a's first seal is the CLOSE.
    ///
    /// Catches: a core that reserves counter 0, or that starts the space at
    /// 1 to keep 0 as a sentinel.
    #[test]
    fn the_first_seal_of_a_connection_is_counter_zero() {
        let t = t0();
        let mut f = established_at(t);
        assert_eq!(f.next_counter(), 0, "§7.1: the space starts at 0");

        let d = f.close(t, 0, b"bye");
        let tx = d.one_transmit();
        assert_eq!(
            Peer::counter_of(&tx.data),
            0,
            "§7.1: the first seal burns counter 0"
        );
    }

    /// §7.1: *"Every seal — payload, control, keepalive — burns the next
    /// counter"*, monotonically and without reuse.
    ///
    /// The separating side matters: a build that re-sealed the same CLOSE
    /// bytes for each linger reply would repeat counter 0, and the peer's
    /// `decrypt_at` would refuse the replay. Asserting the counters are
    /// *strictly increasing* — not merely "a reply arrived" — is what pins
    /// it.
    #[test]
    fn every_seal_burns_the_next_counter_and_never_repeats() {
        let t = t0();
        let mut f = established_at(t);

        let mut counters = vec![];
        counters.push(Peer::counter_of(&f.close(t, 0, b"bye").one_transmit().data));

        // Two more seals, each a linger reply one rate-interval apart.
        for k in 1..=2u32 {
            let at = t + CLOSE_REPLY_MIN_INTERVAL * k + Duration::from_millis(1);
            let dgram = f.peer.seal(&padding(1));
            let d = f.feed(at, &dgram);
            counters.push(Peer::counter_of(&d.one_transmit().data));
        }

        assert_eq!(
            counters,
            vec![0, 1, 2],
            "§7.1: monotonic, contiguous, never reused"
        );
        // And every one of them opens under the peer's key at that counter,
        // which is the real claim: the header counter *is* the AEAD nonce.
        assert_eq!(f.next_counter(), 3, "three seals, three counters");
    }

    /// §7.1: the counter "rides in cleartext because the receiver decrypts
    /// with it", and §3.4 makes the 14 header bytes the associated data
    /// verbatim.
    ///
    /// Catches: a header built with our own index instead of the peer's
    /// (the failure §3.4's own module docs warn about, which produces no
    /// type error), and a counter written big-endian.
    #[test]
    fn the_close_packet_routes_and_opens_under_the_peers_key() {
        let t = t0();
        let mut f = established_at(t);
        let tx = f.close(t, 0x11, b"bye").one_transmit();

        assert_eq!(tx.to, f.peer.addr, "§15.2: to the session's address");
        assert!(
            tx.data.len() <= MAX_DATAGRAM,
            "§8.6/§3.5: never above the MTU"
        );
        assert_eq!(tx.data[0], PKT_DATA);
        assert_eq!(tx.data[1], VERSION);
        assert_eq!(
            u32::from_le_bytes(tx.data[2..6].try_into().unwrap()),
            f.peer.our_index,
            "§3.4: receiver_index is the *peer's* index, little-endian"
        );
        // The decisive part: it opens, at that counter, under that AD.
        let close = one_close(&mut f.peer, &tx.data);
        assert_eq!(close.code, 0x11);
        assert_eq!(close.reason, b"bye");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.2 — the anti-replay window
// ═══════════════════════════════════════════════════════════════════════

mod replay {
    use super::*;

    /// The oracle every window test below uses: a CLOSE frame is delivered
    /// **iff** the packet carrying it was authenticated and window-fresh.
    /// A dropped packet produces nothing at all (§7.2: "dropped after
    /// decryption *without delivery*").
    fn probe(code: u64) -> Vec<u8> {
        close_frame(code, b"probe")
    }

    fn was_delivered(d: &Drained, code: u64) -> bool {
        d.closed()
            .iter()
            .any(|l| matches!(l, ConnectionLost::PeerClosed { code: c, .. } if *c == code))
    }

    /// §7.2: a duplicate is dropped after decryption, without delivery.
    ///
    /// Catches: no replay check at all. The separating half is the *first*
    /// delivery — a build that dropped everything would pass the "the
    /// duplicate did nothing" half on its own.
    #[test]
    fn a_duplicate_counter_is_dropped_without_delivery() {
        let t = t0();
        let mut f = established_at(t);
        let dgram = f.peer.seal(&padding(1));

        let first = f.feed(t, &dgram);
        assert!(first.is_silent(), "a PADDING packet is silent");

        // Prove the packet really was accepted the first time by replaying
        // it with a *decisive* payload at the same counter — which is
        // exactly what a replay is.
        let again = f.feed(t + Duration::from_secs(1), &dgram);
        assert!(again.is_silent(), "the duplicate produced {:?}", again.outs);
        assert!(f.conn.is_established(), "and did not kill the connection");
    }

    /// §7.2: *"a counter more than 2048 behind the greatest is dropped"* —
    /// so `greatest − REPLAY_WINDOW` is **not** more than 2048 behind and
    /// must be delivered.
    ///
    /// T15's accepted side. Testing only the dropped side leaves the whole
    /// window collapsible to a much smaller one with nothing red — slice
    /// 1's one-sided-boundary lesson.
    #[test]
    fn a_counter_exactly_the_window_behind_the_greatest_is_delivered() {
        let t = t0();
        let mut f = established_at(t);
        let window = REPLAY_WINDOW as u64;

        let edge = f.peer.seal_at(1, &probe(0x21));
        let greatest = f.peer.seal_at(1 + window, &padding(1));

        assert!(f.feed(t, &greatest).is_silent(), "greatest lands silently");
        let d = f.feed(t, &edge);
        assert!(
            was_delivered(&d, 0x21),
            "greatest − {window} must be delivered (§7.2), got {:?}",
            d.outs
        );
    }

    /// §7.2's dropped side: one counter further back is *more than* 2048
    /// behind, and is dropped without delivery.
    ///
    /// Catches: an off-by-one that widens the window, and (paired with the
    /// test above) any resizing of it in either direction.
    #[test]
    fn a_counter_one_past_the_window_is_dropped() {
        let t = t0();
        let mut f = established_at(t);
        let window = REPLAY_WINDOW as u64;

        let past = f.peer.seal_at(1, &probe(0x22));
        let greatest = f.peer.seal_at(2 + window, &padding(1));

        assert!(f.feed(t, &greatest).is_silent());
        let d = f.feed(t, &past);
        assert!(
            !was_delivered(&d, 0x22),
            "greatest − {} must be dropped (§7.2)",
            window + 1
        );
        assert!(d.is_silent(), "and dropped silently, got {:?}", d.outs);
        assert!(f.conn.is_established(), "a stale packet is not a violation");
    }

    /// The far edge must be **marked**, not merely admitted.
    ///
    /// Catches: a range check with no bitmap write at the boundary word — a
    /// build that accepts `greatest − 2048` every time it arrives. The
    /// test above passes such a build; this one does not.
    #[test]
    fn the_far_edge_of_the_window_is_marked_not_merely_admitted() {
        let t = t0();
        let mut f = established_at(t);
        let window = REPLAY_WINDOW as u64;

        let edge = f.peer.seal_at(1, &padding(1));
        let edge_replay_probe = edge.clone();
        let greatest = f.peer.seal_at(1 + window, &padding(1));

        assert!(f.feed(t, &greatest).is_silent());
        assert!(f.feed(t, &edge).is_silent(), "the edge is accepted");

        // Now the same counter again: it is a duplicate, and duplicates are
        // dropped. Observable through the linger reply rule, which owes a
        // reply only to a *window-fresh* packet (§15.2).
        let d0 = f.close(t, 0, b"bye");
        assert_eq!(d0.transmits().len(), 1, "the local CLOSE");
        let at = t + CLOSE_REPLY_MIN_INTERVAL + Duration::from_millis(1);
        let d = f.feed(at, &edge_replay_probe);
        assert!(
            d.transmits().is_empty(),
            "a replayed far-edge packet is not window-fresh and owes no reply"
        );
    }

    /// §7.2: *"The replay check is strictly **post-AEAD**: check-then-mark
    /// only after `decrypt_at` authenticates."*
    ///
    /// T14, and the whole of §7.2's ordering rule in one test. A build that
    /// marks before decrypting has already burned counter *c* when the
    /// genuine packet arrives, and drops it.
    ///
    /// Catches: check-then-mark before `decrypt_at`; marking on a
    /// decryption failure.
    #[test]
    fn a_failed_decryption_never_burns_its_counter() {
        let t = t0();
        let mut f = established_at(t);

        let genuine = f.peer.seal(&probe(0x23));
        let mut corrupt = genuine.clone();
        // Flip a ciphertext byte, leaving the header — and therefore the
        // counter and the routing — untouched.
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xff;

        let d = f.feed(t, &corrupt);
        assert!(
            d.is_silent(),
            "a forgery is a silent drop, got {:?}",
            d.outs
        );
        assert!(f.conn.is_established(), "and never a protocol violation");

        let d = f.feed(t + Duration::from_millis(1), &genuine);
        assert!(
            was_delivered(&d, 0x23),
            "the genuine packet at the same counter must still be delivered (§7.2)"
        );
    }

    /// §7.2 sizing: 2048 counters of reordering memory, so an ordinary
    /// out-of-order burst is delivered whole and in any order.
    ///
    /// Catches: a window that only accepts strictly-increasing counters —
    /// which the duplicate test alone would not notice.
    #[test]
    fn reordering_inside_the_window_is_delivered() {
        let t = t0();
        let mut f = established_at(t);

        let older = f.peer.seal_at(10, &padding(1));
        let decisive = f.peer.seal_at(11, &probe(0x24));
        let newest = f.peer.seal_at(12, &padding(1));

        // Newest first, then the oldest, then the one that proves delivery.
        assert!(f.feed(t, &newest).is_silent());
        assert!(
            f.feed(t, &older).is_silent(),
            "an older counter still lands"
        );
        let d = f.feed(t, &decisive);
        assert!(
            was_delivered(&d, 0x24),
            "a counter below the greatest and inside the window is delivered (§7.2)"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.4 — the liveness half slice 3a cannot avoid
// ═══════════════════════════════════════════════════════════════════════

mod liveness {
    use super::*;

    /// §7.4: *"At install the clock is pinned, and it is pinned **armed**.
    /// A newly installed session sets both `last_authenticated_recv` and
    /// `last_send` to the install instant and starts with the death
    /// deadline **already armed**."*
    ///
    /// §16.5: *"the cores expose exact deadlines."* Slice 3a arms exactly
    /// one other timer (`CloseLinger`) and it is not armed here, so the
    /// announced deadline is the liveness deadline, exactly.
    ///
    /// Catches: a clock started unarmed — the build §7.4 says would hold a
    /// half-open session "forever", since §7.6 is deleted and liveness is
    /// the only reaper.
    #[test]
    fn a_new_session_arms_the_death_clock_at_install() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.drain();
        assert_eq!(
            d.deadline,
            Some(t + DEAD_TIMEOUT),
            "§7.4: armed at install, anchored at the install instant"
        );
    }

    /// §7.4's half-open session, reaped in silence.
    ///
    /// The two-sided boundary: alive one nanosecond before, dead after.
    /// Testing only the death leaves a build that dies immediately, or at
    /// any earlier instant, entirely green.
    ///
    /// **Spec gap, deliberately not pinned here.** §7.4 states the death
    /// condition as `now − last_authenticated_recv > DEAD_TIMEOUT` —
    /// strictly greater — while §16.5 says an armed deadline `D` fires "no
    /// earlier than `D`". The two disagree about the single instant
    /// `install + DEAD_TIMEOUT`. This test asserts only the instants both
    /// readings agree on. See `owed::LIVENESS_EXACT_INSTANT`.
    #[test]
    fn a_half_open_session_is_reaped_in_silence_and_not_one_nanosecond_early() {
        let t = t0();
        let mut f = established_at(t);

        let early = f.timeout(t + DEAD_TIMEOUT - NS);
        assert!(
            early.closed().is_empty(),
            "not dead before DEAD_TIMEOUT, got {:?}",
            early.outs
        );
        assert!(
            early.transmits().is_empty(),
            "§15.4: liveness transmits nothing"
        );

        let d = f.timeout(t + DEAD_TIMEOUT + NS);
        assert_eq!(
            d.closed(),
            vec![ConnectionLost::TimedOut],
            "§15.4: the liveness row surfaces TimedOut"
        );
        assert!(
            d.transmits().is_empty(),
            "§15.4: liveness transmits *nothing* — no CLOSE, no probe"
        );
    }

    /// **Ruling 81's no-linger half.** Liveness has no post-mortem, so
    /// `Closed` is followed by `Retired` **within the same drain**, and in
    /// that order (§16.4: state removal precedes emission is the timer
    /// rule; generation order is the drain rule).
    ///
    /// Catches: a build that retires on a `CloseLinger` it never armed
    /// (leaking the index route for the endpoint's life), and a build that
    /// emits `Retired` before `Closed`.
    #[test]
    fn a_liveness_death_retires_in_the_same_drain() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.timeout(t + DEAD_TIMEOUT + NS);

        assert_eq!(d.retired(), vec![f.our_index], "§16.4: Retired is a MUST");
        let closed = d
            .position(|o| matches!(o, ConnOutput::Event(ConnEvent::Closed(_))))
            .expect("Closed");
        let retired = d
            .position(|o| matches!(o, ConnOutput::ToEndpoint(ToEndpoint::Retired { .. })))
            .expect("Retired");
        assert!(
            closed < retired,
            "Closed precedes Retired ({closed} vs {retired})"
        );
        assert_eq!(d.deadline, None, "nothing is armed after the death");
    }

    /// §7.2: *"Liveness and roaming are driven only by packets that are
    /// both authenticated and window-marked."*
    ///
    /// A datagram that routes by `receiver_index` but fails the AEAD is
    /// neither, so it must not defer the death by one nanosecond.
    ///
    /// Catches: an implementation that anchors liveness in
    /// `handle_datagram` before `decrypt_at` — the same ordering bug §7.2's
    /// post-AEAD rule forbids, seen from the liveness side.
    #[test]
    fn an_unauthenticated_datagram_never_refreshes_liveness() {
        let t = t0();
        let mut f = established_at(t);

        let mut forged = f.peer.seal(&padding(1));
        let last = forged.len() - 1;
        forged[last] ^= 0xff;

        let mid = t + DEAD_TIMEOUT / 2;
        assert!(f.feed(mid, &forged).is_silent(), "a forgery is silent");
        let d = f.drain();
        assert_eq!(
            d.deadline,
            Some(t + DEAD_TIMEOUT),
            "the anchor must not have moved to {mid:?}"
        );

        let d = f.timeout(t + DEAD_TIMEOUT + NS);
        assert_eq!(
            d.closed(),
            vec![ConnectionLost::TimedOut],
            "the forgery bought no time at all"
        );
    }

    /// §7.4: the death clock "is reset by every authenticated, window-fresh
    /// receive", and §7.4's condition has a **second conjunct** — "at least
    /// one **arming** send has occurred since that last authenticated
    /// receive". Slice 3a has no arming send at all: its only seals are
    /// CLOSE, which is neither marking (§7.4's quiet set) nor ack-eliciting
    /// (§8.3). So a connection that receives and does not close cannot die
    /// of liveness.
    ///
    /// Catches: a liveness rule that ignores the second conjunct and simply
    /// re-arms at `receive + DEAD_TIMEOUT`. That build kills this
    /// connection; the spec's does not.
    #[test]
    fn a_fresh_receive_re_anchors_the_clock_and_no_arming_send_follows() {
        let t = t0();
        let mut f = established_at(t);

        let r = t + Duration::from_secs(10);
        let dgram = f.peer.seal(&padding(1));
        assert!(f.feed(r, &dgram).is_silent());

        // Well past the *old* anchor's deadline: the receive re-anchored.
        let d = f.timeout(t + DEAD_TIMEOUT + NS);
        assert!(
            d.closed().is_empty(),
            "the receive re-anchored the clock, got {:?}",
            d.outs
        );

        // And past the *new* anchor's deadline too, because no arming send
        // has happened since (§7.4's second conjunct).
        let d = f.timeout(r + DEAD_TIMEOUT + NS);
        assert!(
            d.closed().is_empty(),
            "§7.4: no arming send since the receive, so the deadline is not armed"
        );
        assert!(f.conn.is_established());
    }

    /// §16.5: *"`Liveness` … is **disarmed** and re-anchored by every
    /// [authenticated, window-fresh] receive."*
    ///
    /// The announced deadline is the observable form of that sentence.
    /// Kept as its own test, and its own claim, because it rests on §16.5's
    /// arming clause rather than on §7.4's death condition: a build that
    /// announces `receive + DEAD_TIMEOUT` but declines to die there is
    /// behaviourally close and still wrong, and this is the assertion that
    /// separates them.
    ///
    /// **Slice-scoped.** Slice 7 arms `Keepalive` at this moment (§7.5), so
    /// the expected value here becomes the keepalive deadline rather than
    /// `None`. That is a change of arming, not of this rule.
    #[test]
    fn a_fresh_receive_disarms_the_liveness_deadline() {
        let t = t0();
        let mut f = established_at(t);
        let r = t + Duration::from_secs(10);
        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(r, &dgram);

        assert_ne!(
            d.deadline,
            Some(r + DEAD_TIMEOUT),
            "§16.5: a receive disarms Liveness; it does not re-arm it"
        );
        assert_eq!(
            d.deadline, None,
            "slice 3a arms nothing else at this moment"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.7 — the epoch ratchet. **This is S23.**
// ═══════════════════════════════════════════════════════════════════════

mod ratchet {
    use super::*;

    /// §7.7's constant, pinned **independently of the boundary
    /// behaviour**.
    ///
    /// Ruling 82: "a configurable epoch pins the boundary *behaviour* and a
    /// separate constant test pins the *value* — independently, which is
    /// the stronger arrangement, since a single test crossing a real
    /// boundary would pass just as well against a wrong constant."
    ///
    /// Catches: a default epoch of 1024, or of `u64::MAX` (no ratchet at
    /// all), neither of which any behavioural test below can see.
    #[test]
    fn the_default_epoch_size_is_rekey_epoch_msgs() {
        assert_eq!(
            Config::new().epoch_size(),
            NonZeroU64::new(REKEY_EPOCH_MSGS).expect("nonzero"),
            "§7.7: `REKEY_EPOCH_MSGS` otherwise (ruling 82)"
        );
        assert_eq!(REKEY_EPOCH_MSGS, 65_536, "§7.7: 2¹⁶ messages per epoch");
    }

    /// **T1 — the pair is the pin.** §7.7: *"The receiver retains the
    /// current and immediately preceding epoch keys (straggler tolerance:
    /// one epoch back); anything older is refused."*
    ///
    /// The obvious test — "packets before and after the boundary both
    /// open" — passes a build with **no ratchet at all**, because a plain
    /// datagram pair opens everything. The two assertions that separate
    /// them are:
    ///
    /// - epoch *e−2* **must fail to open** (only a ratcheting receiver
    ///   refuses it), and
    /// - epoch *e−1* **must still open** (only a *correct* ratchet keeps
    ///   the straggler key; a "current epoch only" build refuses it).
    ///
    /// Neither alone is a pin. Both are asserted here, in that order, so
    /// that the second also proves the connection was alive and the oracle
    /// working when the first was asserted by absence.
    #[test]
    fn a_packet_two_epochs_back_is_refused_and_one_epoch_back_still_opens() {
        let t = t0();
        let epoch = NonZeroU64::new(8).expect("nonzero");
        let mut f = established_at_with_epoch(t, epoch);

        // Held: one packet in epoch 0, one in epoch 1. Both carry a CLOSE,
        // which is the only decisive "this was delivered" signal slice 3a
        // has.
        let e0 = f.peer.seal_at(1, &close_frame(0x40, b"e0"));
        let e1 = f.peer.seal_at(8, &close_frame(0x41, b"e1"));
        let e2 = f.peer.seal_at(16, &padding(1));

        // Commit epoch 2 — a jump of exactly `MAX_EPOCH_JUMP`.
        assert!(f.feed(t, &e2).is_silent(), "epoch 2 opens and is silent");

        // e−2: refused, and refused *silently* — §7.7's "generic decryption
        // failure at the hiss surface", not a protocol violation.
        let d = f.feed(t, &e0);
        assert!(
            d.closed().is_empty(),
            "a packet two epochs back must not open (§7.7), got {:?}",
            d.outs
        );
        assert!(d.is_silent(), "and must be a silent drop, got {:?}", d.outs);
        assert!(f.conn.is_established(), "and must not kill the connection");

        // e−1: the straggler still opens.
        let d = f.feed(t, &e1);
        assert!(
            d.closed()
                .iter()
                .any(|l| matches!(l, ConnectionLost::PeerClosed { code: 0x41, .. })),
            "§7.7: one epoch back is retained (straggler tolerance), got {:?}",
            d.outs
        );
    }

    /// **S23** — "a long-lived connection rekeys itself without the user
    /// noticing": the ratchet advances "with no handshake, no round trip
    /// and no application-visible event."
    ///
    /// Catches: an implementation that emits *anything* at the boundary —
    /// an event, a transmit, a deadline change. §7.7's implementation
    /// obligation is a negative one and this is its assertion.
    #[test]
    fn crossing_an_epoch_boundary_is_invisible() {
        let t = t0();
        let epoch = NonZeroU64::new(4).expect("nonzero");
        let mut f = established_at_with_epoch(t, epoch);

        for c in 0..=12u64 {
            let dgram = f.peer.seal(&padding(1));
            assert_eq!(Peer::counter_of(&dgram), c);
            let d = f.feed(t, &dgram);
            assert!(
                d.is_silent(),
                "counter {c} (epoch {}) produced {:?}",
                c / 4,
                d.outs
            );
        }
        assert!(
            f.conn.is_established(),
            "three epoch boundaries crossed, connection untouched"
        );
        // No handshake, no re-install: the session is still the one that
        // was installed.
        assert_eq!(f.conn.session().expect("session").our_index, f.our_index);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.8 — one session per connection
// ═══════════════════════════════════════════════════════════════════════

mod one_session {
    use super::*;

    /// §7.8: *"A connection has **exactly one session** for its whole
    /// life … the counter runs from 0 at establishment and never
    /// restarts"* (§7.1).
    ///
    /// Catches: a core that keeps per-session state alongside
    /// per-connection state, and re-zeroes the wrong one. In slice 3a the
    /// observable form is that the counter space and the session's indices
    /// are the same objects before and after a full round of traffic.
    #[test]
    fn the_session_and_its_counter_space_are_the_connections_own() {
        let t = t0();
        let mut f = established_at(t);
        let index = f.conn.session().expect("session").our_index;

        for _ in 0..4 {
            let dgram = f.peer.seal(&padding(1));
            assert!(f.feed(t, &dgram).is_silent());
        }
        assert_eq!(f.next_counter(), 0, "receives burn no send counters");

        let tx = f.close(t, 0, b"bye").one_transmit();
        assert_eq!(Peer::counter_of(&tx.data), 0, "§7.1: still starting at 0");
        assert_eq!(
            f.conn.session().expect("session").our_index,
            index,
            "§7.8: the same session throughout"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.9 — nonce exhaustion
// ═══════════════════════════════════════════════════════════════════════
//
// **Interface request, not a discovery.** ~1.8 × 10¹⁹ seals is not a test,
// and hiss's counter setter is `#[cfg(test)]` *inside hiss*, so no consumer
// can reach it. `PLAN.md` §9 T7 therefore calls for a `#[cfg(test)]`
// seal-failure injection point and does not name it. The name used below is
// TEST-A's proposal: `Connection::fail_next_seal_for_test()`, on the
// connection because that is the only object a test holds. **If the
// implementation named it differently, rename the call — the three
// assertions are the test.**

mod exhaustion {
    use super::*;

    /// §7.9: *"A seal failure is never silent and can never strand frames
    /// (plan-seal-commit, §16.7): it moves the connection to
    /// `ConnectionLost::NonceExhausted` — connection death, with no rekey
    /// escape."* §16.7: *"On seal failure nothing moved."*
    ///
    /// **Three assertions, because one is not enough.** T7's broken
    /// versions are (a) the seal error is swallowed and (b) the commit
    /// happens before the seal:
    ///
    /// 1. `Closed(NonceExhausted)` is emitted — kills (a);
    /// 2. **nothing is transmitted** — kills a build that emits the packet
    ///    it failed to seal;
    /// 3. **nothing moved**: the send counter is unchanged — kills (b), and
    ///    is the only place in slice 3a where commit-after-seal is
    ///    observable at all.
    ///
    /// Ruling 81 puts nonce exhaustion in the **no-linger** class, so
    /// `Retired` follows in the same drain.
    #[test]
    fn a_seal_failure_kills_the_connection_and_moves_nothing() {
        let t = t0();
        let mut f = established_at(t);
        let before = f.next_counter();

        f.conn.fail_next_seal();
        let d = f.close(t, 0, b"bye");

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::NonceExhausted],
            "§7.9: the seal failure is never silent"
        );
        assert!(
            d.transmits().is_empty(),
            "§16.7: nothing was sealed, so nothing may be transmitted"
        );
        // Phrased as a `!=` on purpose. §7.9's death may drop the session
        // (ruling 81 puts it in the no-linger class), so `Some(before)` is
        // not a safe expectation — but `Some(before + 1)` is exactly the
        // commit-before-seal build, and this assertion is never vacuous.
        assert_ne!(
            f.conn.session().map(|s| s.seal.next_counter()),
            Some(before + 1),
            "§16.7: on seal failure nothing moved — the counter must not have advanced"
        );
        assert_eq!(
            d.retired(),
            vec![f.our_index],
            "ruling 81: no linger, so Retired lands in the same drain"
        );
        assert_eq!(d.deadline, None, "and no linger deadline is armed");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8 — the frame layer
// ═══════════════════════════════════════════════════════════════════════

mod codec {
    use super::*;

    /// §15.3's `PROTOCOL_VIOLATION`.
    const PROTOCOL_VIOLATION: u64 = 0x01;

    /// Deliver `frames` to a fresh connection and assert §8.2's structural
    /// class fires: **the** signalled death — CLOSE with
    /// `PROTOCOL_VIOLATION`, `ConnectionLost::ProtocolViolation { code }`
    /// locally, and the closing state (§15.2).
    ///
    /// Every structural case below runs through this one function, so the
    /// full §8.2 consequence is asserted for each rather than only the
    /// convenient half.
    fn assert_structural_failure(frames: &[u8], what: &str) {
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, frames);

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::ProtocolViolation {
                code: PROTOCOL_VIOLATION
            }],
            "§8.2/§15.2: {what} is a signalled death, got {:?}",
            d.outs
        );
        let tx = d.one_transmit();
        let close = one_close(&mut f.peer, &tx.data);
        assert_eq!(
            close.code, PROTOCOL_VIOLATION,
            "§8.2: CLOSE carries PROTOCOL_VIOLATION (0x01) for {what}"
        );
        assert_eq!(
            d.deadline,
            Some(t + CLOSE_LINGER),
            "§15.2: a violation lingers as for a local close"
        );
        assert!(
            d.retired().is_empty(),
            "ruling 81: the linger has not expired yet"
        );
    }

    /// Deliver `frames` to a fresh connection and assert it survives,
    /// saying nothing **beyond the ACK §12 owes**.
    ///
    /// This asserted `transmits().is_empty()` until slice 5. That was right
    /// while §12 did not exist and became false the moment it did: an
    /// ack-eliciting arrival now owes an ACK, and §12.4 emits it at once
    /// for the second such packet or an out-of-order one. The assertion is
    /// **narrowed rather than dropped** — what these tests are for is that
    /// a PADDING/PING/unknown-shaped arrival produces nothing *else*, and
    /// that survives §12 intact. A bare `transmits().len() <= 1` would not:
    /// it would pass a build that answered a PING with a CLOSE.
    fn assert_harmless(frames: &[u8], what: &str) {
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, frames);
        assert!(
            d.closed().is_empty(),
            "§8: {what} must not kill the connection, got {:?}",
            d.outs
        );
        for tr in d.transmits() {
            let pt = f.peer.open(&tr.data);
            for frame in parse_frames(&pt) {
                assert!(
                    matches!(frame, PeerFrame::Ack { .. } | PeerFrame::Padding),
                    "§8: {what} owes nothing but §12's ACK, got {frame:?}"
                );
            }
        }
        assert!(f.conn.is_established());
    }

    // ── PADDING (§8.4) ────────────────────────────────────────────────

    /// §8.4: PADDING is *"a single `0x00` byte, no fields; **any number may
    /// appear anywhere**."*
    ///
    /// Slice 3a's frame set is small enough that a parser could plausibly
    /// treat `0x00` as "no frame" and stop, or as an unknown type and kill
    /// the connection. Both are §8.2 violations of the opposite kind, and
    /// both are caught here.
    #[test]
    fn padding_alone_is_harmless() {
        assert_harmless(&padding(1), "one PADDING byte");
        assert_harmless(&padding(64), "sixty-four PADDING bytes");
    }

    /// "Anywhere" is the word under test: before, between and after other
    /// frames.
    ///
    /// Catches: a parser that only tolerates PADDING as a trailer, which is
    /// the shape a "skip trailing zeroes" implementation has.
    #[test]
    fn padding_may_appear_anywhere_around_other_frames() {
        let mut s = padding(3);
        s.extend(ping());
        s.extend(padding(2));
        s.extend(ack(0, 0, 0, &[]));
        s.extend(padding(5));
        assert_harmless(&s, "PADDING wrapped around PING and ACK");
    }

    // ── PING and ACK (§8.4) ───────────────────────────────────────────

    /// §8.4: PING is "the type byte alone". It is ack-eliciting, but §12's
    /// ACK generation is slice 5, so slice 3a answers nothing.
    #[test]
    fn a_ping_is_accepted_and_answered_with_nothing_in_slice_3a() {
        assert_harmless(&ping(), "a PING");
    }

    /// §8.4's ACK layout, decoded and — per this slice's scope — **not
    /// acted on**.
    #[test]
    fn a_well_formed_ack_is_decoded_and_not_acted_on() {
        assert_harmless(&ack(0, 0, 0, &[]), "an ACK covering counter 0");
        assert_harmless(
            &ack(100, 1_234, 4, &[(0, 0), (3, 2)]),
            "an ACK with two extra ranges",
        );
    }

    /// §8.4: *"Semantic no-op (frame ignored whole, traced): `largest`
    /// above the highest counter this session has sealed (§12.5)."*
    ///
    /// Slice 3a has sealed nothing at all when this arrives, so *every*
    /// `largest` is above it. That must be an ignore, not a death.
    ///
    /// Catches: a build that promotes §12.5's no-op to §8.2's structural
    /// class — which would make the very first ACK any peer sends fatal.
    #[test]
    fn an_ack_largest_above_anything_sealed_is_ignored_not_fatal() {
        assert_harmless(&ack(1_000_000, 0, 0, &[]), "an ACK from the future");
    }

    /// §8.4: *"Structural errors: `range_count` > `MAX_ACK_RANGES` (64)."*
    /// Both sides of the boundary, because a one-sided test leaves the cap
    /// free to move down.
    #[test]
    fn the_max_ack_ranges_boundary_is_tested_from_both_sides() {
        let at_cap: Vec<(u64, u64)> = vec![(0, 0); MAX_ACK_RANGES];
        assert_harmless(
            &ack(10_000, 0, 0, &at_cap),
            "an ACK at exactly MAX_ACK_RANGES",
        );

        let over_cap: Vec<(u64, u64)> = vec![(0, 0); MAX_ACK_RANGES + 1];
        assert_structural_failure(
            &ack(10_000, 0, 0, &over_cap),
            "an ACK with range_count = MAX_ACK_RANGES + 1",
        );
    }

    /// §8.4: *"any range descending below counter zero"* is structural.
    ///
    /// `largest = 5`, `first_range = 10` puts the range's floor at −5.
    ///
    /// Catches: a decoder that subtracts in `u64` and wraps — the wrapped
    /// value is a colossal counter that a naive range check happily
    /// accepts.
    #[test]
    fn an_ack_range_descending_below_zero_is_a_structural_failure() {
        assert_structural_failure(&ack(5, 0, 10, &[]), "an ACK range below counter zero");
    }

    /// The structural check precedes the semantic one, which is what
    /// **parse**-then-apply means: this ACK is *both* structurally invalid
    /// (65 ranges) and semantically a no-op (`largest` above anything
    /// sealed). §8.2 says the structural class wins.
    ///
    /// Catches: an implementation that tests §12.5's `largest` first and
    /// returns early, never reaching the range-count check — a build that
    /// passes both single-fault tests above.
    #[test]
    fn a_structural_ack_error_beats_the_semantic_no_op() {
        let over_cap: Vec<(u64, u64)> = vec![(0, 0); MAX_ACK_RANGES + 1];
        assert_structural_failure(
            &ack(u64::from(u32::MAX), 0, 0, &over_cap),
            "an over-long ACK whose largest is also unsealed",
        );
    }

    // ── the structural class (§8.2) ───────────────────────────────────

    /// §8.2's first named case: an unknown frame type.
    #[test]
    fn an_unknown_frame_type_is_a_structural_failure() {
        assert_structural_failure(&vi(0x3f), "a 1-byte unknown type");
        assert_structural_failure(&vi(0x7f), "a 2-byte unknown type");
    }

    /// §8.3: *"`0x05` is *reserved*, not implemented: like any unknown
    /// type, receiving it is a structural failure."*
    ///
    /// Catches: a decoder that reserves a `Frame::StopSending` arm now and
    /// silently ignores it — the exact thing "reserved, not implemented"
    /// forbids.
    #[test]
    fn the_reserved_type_is_a_structural_failure() {
        assert_structural_failure(
            &vi(FRAME_STOP_SENDING_RESERVED),
            "the reserved STOP_SENDING type",
        );
    }

    /// §8.2: a truncated frame. The CLOSE announces five reason bytes and
    /// supplies two.
    #[test]
    fn a_truncated_frame_is_a_structural_failure() {
        let mut s = vi(FRAME_CLOSE);
        s.extend(vi(0));
        s.extend(vi(5));
        s.extend_from_slice(b"ab");
        assert_structural_failure(&s, "a CLOSE whose reason is short");
    }

    /// §8.2: a varint overrunning the plaintext. `0x80` opens a four-byte
    /// varint and only two bytes follow.
    ///
    /// Catches: a decoder that reads past the slice, or that pads the tail
    /// with zeroes instead of failing.
    #[test]
    fn a_varint_overrunning_the_plaintext_is_a_structural_failure() {
        assert_structural_failure(&[0x80, 0x00], "a 4-byte varint with 2 bytes left");
        assert_structural_failure(&[0xc0], "an 8-byte varint with 1 byte left");
    }

    /// §8.2: a **length field** overrunning the plaintext, which is a
    /// different case from a truncated varint: every varint here is
    /// complete and it is `reason_len` that lies.
    #[test]
    fn a_length_field_overrunning_the_plaintext_is_a_structural_failure() {
        let mut s = vi(FRAME_CLOSE);
        s.extend(vi(0));
        s.extend(vi(200));
        s.extend_from_slice(b"short");
        assert_structural_failure(&s, "a CLOSE whose reason_len overruns");
    }

    /// §8.4: *"Structural error: `reason_len` > 256."* Both sides.
    ///
    /// At exactly `CLOSE_REASON_MAX` the frame is valid and the connection
    /// dies of `PeerClosed` instead — which is the assertion that stops the
    /// cap being quietly lowered.
    #[test]
    fn the_close_reason_max_boundary_is_tested_from_both_sides() {
        let t = t0();
        let mut f = established_at(t);
        let reason = vec![b'r'; CLOSE_REASON_MAX];
        let d = f.deliver(t, &close_frame(9, &reason));
        assert_eq!(
            d.closed(),
            vec![ConnectionLost::PeerClosed {
                code: 9,
                reason: reason.clone()
            }],
            "§8.4: exactly CLOSE_REASON_MAX is valid"
        );

        let over = vec![b'r'; CLOSE_REASON_MAX + 1];
        assert_structural_failure(
            &close_frame(9, &over),
            "a CLOSE reason one byte over CLOSE_REASON_MAX",
        );
    }

    /// **T2 — parse-then-apply, and the only test that can separate it.**
    ///
    /// §8.2: *"**Parse the whole plaintext first, then apply.** … Nothing
    /// from the packet is applied."*
    ///
    /// One packet, two frames: a perfectly valid CLOSE followed by an
    /// unknown type. A conforming implementation parses both, hits the
    /// unknown type, applies nothing, and surfaces
    /// `ProtocolViolation { code: 0x01 }`. A **streaming** parse-and-apply
    /// loop applies the CLOSE first and surfaces
    /// `PeerClosed { code: 0x42, .. }`. Two different variants out of one
    /// packet — decisive, and constructible with only slice 3a's frames.
    ///
    /// The plain "an unknown frame type is fatal" test above passes the
    /// streaming build too; this one does not.
    #[test]
    fn parse_then_apply_a_valid_close_followed_by_garbage_is_a_violation() {
        let mut s = close_frame(0x42, b"applied too early");
        s.extend(vi(0x7f));
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, &s);

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::ProtocolViolation {
                code: PROTOCOL_VIOLATION
            }],
            "§8.2: nothing from the packet is applied — not even a valid CLOSE"
        );
        let close = one_close(&mut f.peer, &d.one_transmit().data);
        assert_eq!(
            close.code, PROTOCOL_VIOLATION,
            "and the CLOSE we send is ours, not an echo of theirs"
        );
    }

    /// The mirror of the above: the unknown type comes **first**. A
    /// two-pass implementation is symmetric; a streaming one is not, and
    /// this ordering is the one it gets right by accident.
    ///
    /// Kept because together the two pin that the *packet*, not the
    /// *prefix*, is the unit.
    #[test]
    fn garbage_before_a_valid_close_is_also_a_violation() {
        let mut s = vi(0x7f);
        s.extend(close_frame(0x42, b"never applied"));
        assert_structural_failure(&s, "an unknown type ahead of a valid CLOSE");
    }

    // ── §8.1 varints ──────────────────────────────────────────────────

    /// §8.1: *"a receiver accepts any length (a non-minimal encoding is
    /// valid, as in QUIC)."* Appendix B lists it explicitly.
    ///
    /// Every field of the CLOSE — the **type byte included**, which §8.1
    /// says is itself a varint — is written non-minimally here.
    ///
    /// Catches: a decoder that special-cases the frame type as a `u8`, and
    /// one that rejects a non-canonical encoding on principle.
    #[test]
    fn non_minimal_varints_are_accepted() {
        let mut s = vi_padded(FRAME_CLOSE, 8);
        s.extend(vi_padded(7, 4));
        s.extend(vi_padded(2, 2));
        s.extend_from_slice(b"hi");

        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, &s);
        assert_eq!(
            d.closed(),
            vec![ConnectionLost::PeerClosed {
                code: 7,
                reason: b"hi".to_vec()
            }],
            "§8.1: a non-minimal encoding decodes to the same value"
        );
    }

    /// §8.1's ceiling, on the wire: `2⁶² − 1` is representable and must
    /// arrive unchanged.
    ///
    /// Catches: a decoder that masks to 32 bits, or that loses the top two
    /// bits of the 8-byte form.
    #[test]
    fn a_close_code_at_the_varint_maximum_round_trips() {
        let max = (1u64 << 62) - 1;
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, &close_frame(max, b""));
        assert_eq!(
            d.closed(),
            vec![ConnectionLost::PeerClosed {
                code: max,
                reason: vec![]
            }],
            "§8.1: varints cap at 2⁶² − 1 and carry it exactly"
        );
    }

    /// §8.1's width boundaries, exercised through a real field rather than
    /// through the varint unit tests: 63/64, 16 383/16 384 and
    /// 2³⁰−1/2³⁰ are where the encoding changes width.
    #[test]
    fn the_varint_width_boundaries_round_trip_through_a_frame() {
        for code in [63u64, 64, 16_383, 16_384, (1 << 30) - 1, 1 << 30] {
            let t = t0();
            let mut f = established_at(t);
            let d = f.deliver(t, &close_frame(code, b""));
            assert_eq!(
                d.closed(),
                vec![ConnectionLost::PeerClosed {
                    code,
                    reason: vec![]
                }],
                "§8.1: code {code} must survive its encoding width"
            );
        }
    }

    // ── §8.2's empty plaintext, and §8.6's bound ──────────────────────

    /// §8.2: *"An empty plaintext is the keepalive and never reaches this
    /// layer (§7.5)."*
    ///
    /// Catches: a parser that requires at least one frame, which turns
    /// every keepalive slice 7 sends into a `PROTOCOL_VIOLATION` — a bug
    /// that would be invisible until slice 7 and fatal there.
    #[test]
    fn an_empty_plaintext_is_not_a_frame_error() {
        assert_harmless(&[], "an empty plaintext (the keepalive)");
    }

    /// §8.6: *"Every slither seal is at most `MAX_PLAINTEXT` + 16 = 1186 B"*
    /// — so a full-size frame stream is ordinary, not exceptional.
    #[test]
    fn a_full_size_plaintext_is_accepted() {
        assert_harmless(&padding(MAX_PLAINTEXT), "MAX_PLAINTEXT bytes of PADDING");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §15 — CLOSE, the closing and draining states, the registry
// ═══════════════════════════════════════════════════════════════════════

mod teardown {
    use super::*;

    /// §15.3.
    const NO_ERROR: u64 = 0x00;
    const PROTOCOL_VIOLATION: u64 = 0x01;

    /// §15.2's local-close row, whole: *"emit CLOSE (sealed `seal_quiet`)
    /// and enter **closing** for `CLOSE_LINGER`"*, and §15.4's local
    /// surface, `LocallyClosed`.
    ///
    /// Catches: a close that emits nothing; one that emits an
    /// unauthenticated close packet (§15.1: "the reserved cleartext close
    /// packet type `0x04` stays dead"); one that reports `PeerClosed` for
    /// our own close; and one that forgets to arm the linger.
    #[test]
    fn a_local_close_emits_one_close_and_surfaces_locally_closed() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.close(t, NO_ERROR, b"bye");

        let tx = d.one_transmit();
        assert_eq!(tx.to, f.peer.addr, "§15.2: to the session's address");
        assert_eq!(tx.data[0], PKT_DATA, "§15.1: authenticated, in-seal only");

        let close = one_close(&mut f.peer, &tx.data);
        assert_eq!(close.code, NO_ERROR);
        assert_eq!(close.reason, b"bye");
        assert_eq!(d.closed(), vec![ConnectionLost::LocallyClosed]);
        assert_eq!(
            d.deadline,
            Some(t + CLOSE_LINGER),
            "§15.2: closing for CLOSE_LINGER"
        );
    }

    /// **Ruling 81, the half that matters.** *"`Retired` … fires when the
    /// connection's state is **actually dropped** — not when its death is
    /// announced. … The linger must keep receiving to reply … while
    /// `Retired` drops the `receiver_index` route that receiving needs.
    /// Emitting it at the death would delete the mechanism."*
    ///
    /// T5's two broken versions are (a) `Retired` never emitted and
    /// (b) emitted in the same drain as `Closed`, killing the linger. A
    /// count-only assertion ("exactly one `Retired`") passes (b) and pins
    /// nothing, so this test asserts from the separating side twice: a
    /// reply is still produced a second after the death **and** `Retired`
    /// has not yet been seen; then, at the expiry, it is.
    #[test]
    fn retired_waits_for_the_linger_expiry_while_the_reply_rule_still_works() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.close(t, NO_ERROR, b"bye");
        assert!(d.retired().is_empty(), "not at the death (ruling 81)");

        let at = t + CLOSE_REPLY_MIN_INTERVAL + Duration::from_millis(1);
        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(at, &dgram);
        assert_eq!(
            d.transmits().len(),
            1,
            "§15.2: the linger still owes a reply — this is what Retired would delete"
        );
        assert!(d.retired().is_empty(), "and still no Retired");

        let d = f.timeout(t + CLOSE_LINGER);
        assert_eq!(
            d.retired(),
            vec![f.our_index],
            "ruling 81: Retired at the CloseLinger expiry, carrying our index"
        );
        assert!(
            d.closed().is_empty(),
            "§16.4/Q3: Closed is emitted exactly once, and it already was"
        );
        assert_eq!(d.deadline, None, "all state is dropped");
    }

    /// §16.5's `CloseLinger`, from both sides of its deadline.
    ///
    /// Catches: an expiry computed from the wrong anchor, and one that
    /// fires early. Testing only "it retires at 5 s" leaves a build that
    /// retires immediately entirely green.
    #[test]
    fn the_linger_expires_at_exactly_close_linger_and_not_before() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.close(t, NO_ERROR, b"");
        assert_eq!(d.deadline, Some(t + CLOSE_LINGER));

        let early = f.timeout(t + CLOSE_LINGER - NS);
        assert!(
            early.retired().is_empty(),
            "not one nanosecond early, got {:?}",
            early.outs
        );
        assert_eq!(
            early.deadline,
            Some(t + CLOSE_LINGER),
            "and the deadline is unmoved by a spurious wake"
        );

        let d = f.timeout(t + CLOSE_LINGER);
        assert_eq!(d.retired(), vec![f.our_index]);
    }

    /// §16.5: *"`handle_timeout` is idempotent: each due timer is stopped
    /// before its logic runs, so spurious or repeated calls no-op."*
    ///
    /// Called **twice with no drain in between**, which is the shape that
    /// separates a stopped timer from one merely filtered at emission.
    #[test]
    fn handle_timeout_twice_at_the_expiry_retires_once() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        f.conn.handle_timeout(t + CLOSE_LINGER);
        f.conn.handle_timeout(t + CLOSE_LINGER);
        let d = f.drain();
        assert_eq!(
            d.retired(),
            vec![f.our_index],
            "§16.5: exactly one Retired for two due calls"
        );

        let after = f.timeout(t + CLOSE_LINGER + Duration::from_secs(1));
        assert!(
            after.is_silent(),
            "and nothing afterwards, got {:?}",
            after.outs
        );
    }

    /// §15.2's receive row: *"surface `PeerClosed { code, reason }`, emit
    /// **nothing**, hold a brief drain for the same `CLOSE_LINGER`
    /// (discarding late packets, no replies), then drop all state."*
    ///
    /// Catches: a receive path that replies to a CLOSE (the 1 Hz ping-pong
    /// §15.2 exists to prevent), and one that drops state immediately,
    /// which would let a retransmitted CLOSE reach a nonexistent route.
    #[test]
    fn a_received_close_surfaces_peer_closed_transmits_nothing_and_drains() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, &close_frame(0x99, b"peer's reason"));

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::PeerClosed {
                code: 0x99,
                reason: b"peer's reason".to_vec()
            }]
        );
        assert!(d.transmits().is_empty(), "§15.2: emit **nothing**");
        assert_eq!(d.deadline, Some(t + CLOSE_LINGER), "the drain is armed");
        assert!(d.retired().is_empty(), "ruling 81: not at the death");

        // Late packets are discarded, with no reply.
        let dgram = f.peer.seal(&padding(1));
        let late = f.feed(t + Duration::from_secs(2), &dgram);
        assert!(
            late.transmits().is_empty(),
            "§15.2: draining never replies, got {:?}",
            late.transmits()
        );

        let d = f.timeout(t + CLOSE_LINGER);
        assert_eq!(d.retired(), vec![f.our_index], "then drop all state");
    }

    /// **T13.1** — §15.2: *"Replies are capped at one CLOSE per second."*
    ///
    /// Ten authenticated, window-fresh packets inside one second produce
    /// **exactly one** reply — and *at least* one, which is the half that
    /// separates the cap from "never replies".
    ///
    /// The window starts at `t + 2 s` on purpose: §15.2 does not say
    /// whether the local CLOSE itself starts the rate clock, and at two
    /// seconds both readings agree. See `owed::CLOSE_RATE_CLOCK_ORIGIN`.
    #[test]
    fn the_linger_replies_at_most_once_per_second_under_a_flood() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let base = t + Duration::from_secs(2);
        let mut replies = 0;
        for k in 0..10u32 {
            let dgram = f.peer.seal(&padding(1));
            replies += f
                .feed(base + Duration::from_millis(u64::from(k) * 10), &dgram)
                .transmits()
                .len();
        }
        assert_eq!(
            replies, 1,
            "§15.2: ten fresh packets in 90 ms owe exactly one reply"
        );
    }

    /// The separating side of the cap: it is a **rate**, not a one-shot.
    ///
    /// Catches: a build that replies to the first inbound packet and never
    /// again — which passes the flood test above with full marks.
    #[test]
    fn the_linger_replies_again_after_the_rate_interval() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let first_at = t + Duration::from_secs(2);
        let dgram = f.peer.seal(&padding(1));
        assert_eq!(f.feed(first_at, &dgram).transmits().len(), 1);

        let dgram = f.peer.seal(&padding(1));
        let too_soon = f.feed(first_at + CLOSE_REPLY_MIN_INTERVAL - NS, &dgram);
        assert!(
            too_soon.transmits().is_empty(),
            "one nanosecond inside the interval is still capped"
        );

        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(first_at + CLOSE_REPLY_MIN_INTERVAL, &dgram);
        assert_eq!(
            d.transmits().len(),
            1,
            "§15.2: ≤ 1 per second is a rate — the next second owes another"
        );
        let close = one_close(&mut f.peer, &d.one_transmit().data);
        assert_eq!(close.code, NO_ERROR, "and it repeats our code and reason");
    }

    /// **T13.2** — §15.2: the reply is sent *"to the **session's endpoint
    /// address** (the closing state does not roam; never to the triggering
    /// packet's source)."*
    ///
    /// Catches: a reply addressed to `src`. That build passes every count
    /// assertion above and fails only this one — and in production it turns
    /// the closing state into a reflector aimed at whatever address an
    /// on-path observer replays from.
    #[test]
    fn the_linger_reply_goes_to_the_session_address_not_the_packets_source() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let elsewhere = v4(9, 9999);
        assert_ne!(elsewhere, f.peer.addr);
        let dgram = f.peer.seal(&padding(1));
        let d = f.feed_from(t + Duration::from_secs(2), elsewhere, &dgram);

        let tx = d.one_transmit();
        assert_eq!(tx.to, f.peer.addr, "§15.2: the closing state does not roam");
    }

    /// **T13.3** — §15.2: a reply is owed *"only to an authenticated,
    /// window-fresh inbound packet — never to a packet that merely routed
    /// by `receiver_index`, which an off-path forger who observed the
    /// cleartext index could mint."*
    ///
    /// The forgery is asserted by absence, so it is paired with a genuine
    /// packet that *does* draw a reply — otherwise the test would pass
    /// against a build that never replies at all.
    #[test]
    fn a_packet_that_fails_the_aead_gets_no_linger_reply() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let mut forged = f.peer.seal(&padding(1));
        let last = forged.len() - 1;
        forged[last] ^= 0xff;
        let d = f.feed(t + Duration::from_secs(2), &forged);
        assert!(
            d.transmits().is_empty(),
            "§15.2: an off-path forger draws nothing, got {:?}",
            d.transmits()
        );

        let genuine = f.peer.seal(&padding(1));
        let d = f.feed(t + Duration::from_millis(2_100), &genuine);
        assert_eq!(
            d.transmits().len(),
            1,
            "and the reply rule is demonstrably still live"
        );
    }

    /// §15.2's "window-fresh" half, which is a separate requirement from
    /// "authenticated": a **replayed** packet is genuine and still owes no
    /// reply (§7.2: "No replayed packet ever moves the endpoint or
    /// refreshes liveness").
    ///
    /// Catches: a linger that replies on `decrypt_at` success alone,
    /// skipping the window — an unbounded reflector for any observer with a
    /// captured packet.
    #[test]
    fn a_replayed_packet_gets_no_linger_reply() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let dgram = f.peer.seal(&padding(1));
        let first_at = t + Duration::from_secs(2);
        assert_eq!(f.feed(first_at, &dgram).transmits().len(), 1);

        let replay_at = first_at + CLOSE_REPLY_MIN_INTERVAL + Duration::from_millis(1);
        let d = f.feed(replay_at, &dgram);
        assert!(
            d.transmits().is_empty(),
            "a replay is not window-fresh, got {:?}",
            d.transmits()
        );

        let fresh = f.peer.seal(&padding(1));
        let d = f.feed(replay_at + Duration::from_millis(1), &fresh);
        assert_eq!(
            d.transmits().len(),
            1,
            "and a genuinely fresh packet at the same moment does draw one"
        );
    }

    /// §15.2: *"A CLOSE **received** while closing moves the connection to
    /// the reply-free draining behaviour: two closing endpoints go quiet
    /// rather than ping-ponging replies at 1 Hz for the linger."*
    ///
    /// Catches: a state machine with one post-mortem state instead of two.
    /// The separating assertion is the packet **after** the peer's CLOSE:
    /// under a single-state build it still draws a reply.
    #[test]
    fn a_close_received_while_closing_stops_the_replies() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let at = t + Duration::from_secs(2);
        let d = f.deliver(at, &close_frame(3, b"you too"));
        assert!(
            d.transmits().is_empty(),
            "§15.2: no reply to the peer's CLOSE, got {:?}",
            d.transmits()
        );

        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(
            at + CLOSE_REPLY_MIN_INTERVAL + Duration::from_millis(1),
            &dgram,
        );
        assert!(
            d.transmits().is_empty(),
            "§15.2: reply-free from then on, got {:?}",
            d.transmits()
        );
    }

    /// **Q2** — the linger is *not* restarted by a CLOSE received while
    /// closing.
    ///
    /// §15.2 says the draining behaviour holds "for **the same**
    /// `CLOSE_LINGER`" and §16.5's governing principle is "state removal
    /// precedes emission". A restart would let an authenticated peer hold
    /// our post-mortem state open indefinitely by re-CLOSEing at 4.9 s.
    ///
    /// Catches exactly that restart, which no other test here would see.
    #[test]
    fn a_close_received_while_closing_does_not_restart_the_linger() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let at = t + Duration::from_secs(4);
        let d = f.deliver(at, &close_frame(3, b"you too"));
        assert_eq!(
            d.deadline,
            Some(t + CLOSE_LINGER),
            "Q2: the original deadline stands"
        );

        let d = f.timeout(t + CLOSE_LINGER);
        assert_eq!(
            d.retired(),
            vec![f.our_index],
            "and the state is dropped on the original schedule"
        );
    }

    /// **Q3** — `Closed(_)` is emitted **exactly once** per connection.
    ///
    /// This is the invariant the shell's `closed()` latch rests on, and the
    /// reason entering closing must disarm every timer but `CloseLinger`.
    ///
    /// Catches: a second `Closed` at the linger expiry, and a second at the
    /// peer's CLOSE.
    #[test]
    fn closed_is_emitted_exactly_once_across_the_whole_life() {
        let t = t0();
        let mut f = established_at(t);
        let mut seen = f.close(t, NO_ERROR, b"").closed();

        seen.extend(
            f.deliver(t + Duration::from_secs(1), &close_frame(3, b"too"))
                .closed(),
        );
        seen.extend(f.timeout(t + CLOSE_LINGER).closed());
        seen.extend(
            f.timeout(t + CLOSE_LINGER + Duration::from_secs(30))
                .closed(),
        );

        assert_eq!(
            seen,
            vec![ConnectionLost::LocallyClosed],
            "Q3: one death, one event, and it is the *first* cause"
        );
    }

    /// **Q3/U10** — entering closing disarms every timer but
    /// `CloseLinger`.
    ///
    /// Constructed so the two deadlines are genuinely in conflict: the
    /// close happens one second before the liveness deadline would fire, so
    /// a surviving `Liveness` is the *earlier* of the two and would produce
    /// a second `Closed(TimedOut)` inside the linger.
    ///
    /// Catches: a timer table that arms `CloseLinger` without disarming
    /// anything. A close at `t0` would not — the linger is earlier there,
    /// and every ordering looks identical.
    #[test]
    fn entering_closing_disarms_the_liveness_timer() {
        let t = t0();
        let mut f = established_at(t);
        let close_at = t + DEAD_TIMEOUT - Duration::from_secs(1);

        let d = f.close(close_at, NO_ERROR, b"");
        assert_eq!(
            d.deadline,
            Some(close_at + CLOSE_LINGER),
            "the only armed timer is CloseLinger"
        );

        let d = f.timeout(t + DEAD_TIMEOUT + NS);
        assert!(
            d.closed().is_empty(),
            "no second Closed from a surviving Liveness, got {:?}",
            d.outs
        );
        assert!(d.retired().is_empty(), "and no early Retired");

        let d = f.timeout(close_at + CLOSE_LINGER);
        assert_eq!(d.retired(), vec![f.our_index], "the linger still expires");
    }

    /// §15.2/§15.4: a protocol violation is a **signalled** death, and its
    /// local surface is `ProtocolViolation { code }` — *"a dedicated
    /// variant, not `LocallyClosed`"*.
    ///
    /// Catches: reporting `LocallyClosed` for a peer-caused death, which
    /// §15.2 calls out as "opposite causes with opposite operational
    /// responses".
    #[test]
    fn a_violation_surfaces_protocol_violation_not_locally_closed() {
        let t = t0();
        let mut f = established_at(t);
        let d = f.deliver(t, &vi(0x7f));

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::ProtocolViolation {
                code: PROTOCOL_VIOLATION
            }]
        );
        assert!(
            !d.closed().contains(&ConnectionLost::LocallyClosed),
            "§15.2: not LocallyClosed"
        );
    }

    /// §15.3: *"≥ `0x10` | application | application-defined codes via
    /// `close()`"*, and §8.4's byte layout carries them verbatim.
    ///
    /// Catches: a `close()` that remaps or clamps the application's code,
    /// and one that swaps `error_code` and `reason_len` in the frame.
    #[test]
    fn an_application_code_and_reason_ride_the_close_frame_verbatim() {
        for code in [
            APPLICATION_ERROR_BASE,
            APPLICATION_ERROR_BASE + 7,
            (1u64 << 62) - 1,
        ] {
            let t = t0();
            let mut f = established_at(t);
            let tx = f.close(t, code, b"application reason").one_transmit();
            let close = one_close(&mut f.peer, &tx.data);
            assert_eq!(close.code, code, "§15.3: the application's code, exactly");
            assert_eq!(close.reason, b"application reason");
        }
    }

    /// §8.4: *"`close()` truncates its `reason` to `CLOSE_REASON_MAX` … an
    /// implementation must not be able to **produce** the over-length case
    /// it must kill on receipt."*
    ///
    /// Both sides of the boundary: at exactly `CLOSE_REASON_MAX` nothing is
    /// touched; one byte over, the tail is cut and the head kept.
    ///
    /// Catches: truncation that keeps the *tail*; truncation at 255; and no
    /// truncation at all, which produces a frame the peer must kill on —
    /// a connection that murders its own peer on the way out.
    #[test]
    fn close_never_produces_an_over_length_reason() {
        let t = t0();
        let mut f = established_at(t);
        let exact = vec![b'x'; CLOSE_REASON_MAX];
        let tx = f.close(t, NO_ERROR, &exact).one_transmit();
        assert_eq!(
            one_close(&mut f.peer, &tx.data).reason,
            exact,
            "exactly CLOSE_REASON_MAX is emitted whole"
        );

        let t = t0();
        let mut f = established_at(t);
        let mut over = vec![b'h'; CLOSE_REASON_MAX];
        over.push(b'!');
        let tx = f.close(t, NO_ERROR, &over).one_transmit();
        let close = one_close(&mut f.peer, &tx.data);
        assert_eq!(close.reason.len(), CLOSE_REASON_MAX, "§8.4: truncated");
        assert_eq!(
            close.reason,
            over[..CLOSE_REASON_MAX],
            "§8.4: truncated at the tail, not the head"
        );
    }

    /// **Ruling 81's "teardown before a session exists"** and `PLAN.md`
    /// U8: a `connect()`-created connection can be closed before its
    /// install. There is no seal capability, so no CLOSE can be emitted and
    /// there is nothing to linger for.
    ///
    /// **Not asserted here: `Retired`'s payload.** `ToEndpoint::Retired`
    /// carries `our_index: u32`, and a connection with no session has no
    /// session index — §17.3 mints one only at the handshake. Ruling 81
    /// names this case, §16.4 gives the variant no other shape, and nothing
    /// says which value it carries. See `owed::RETIRED_WITHOUT_A_SESSION`.
    #[test]
    fn a_close_before_the_session_exists_transmits_nothing_and_does_not_linger() {
        let t = t0();
        let mut conn = connecting();
        conn.close(t, NO_ERROR, b"never mind");
        let d = drain_bare(&mut conn);

        assert_eq!(
            d.closed(),
            vec![ConnectionLost::LocallyClosed],
            "§15.4: the local surface is LocallyClosed"
        );
        assert!(
            d.transmits().is_empty(),
            "there is no seal capability, so there is no CLOSE"
        );
        assert_eq!(
            d.deadline, None,
            "ruling 81: no linger — there is nothing to linger for"
        );
        if let (Some(c), Some(r)) = (
            d.position(|o| matches!(o, ConnOutput::Event(ConnEvent::Closed(_)))),
            d.position(|o| matches!(o, ConnOutput::ToEndpoint(ToEndpoint::Retired { .. }))),
        ) {
            assert!(c < r, "ruling 81: Closed then Retired, in the same drain");
        }
    }

    /// The `accept()` constructor is a first-class way to be born
    /// established (§16.4), and every §15 behaviour must hold for it too.
    ///
    /// Catches: post-mortem state wired only into the `Install` path.
    #[test]
    fn an_accepted_connection_closes_the_same_way() {
        let t = t0();
        let mut f = accepted_at(t);
        let d = f.close(t, NO_ERROR, b"bye");

        let tx = d.one_transmit();
        assert_eq!(one_close(&mut f.peer, &tx.data).reason, b"bye");
        assert_eq!(d.closed(), vec![ConnectionLost::LocallyClosed]);
        assert_eq!(d.deadline, Some(t + CLOSE_LINGER));
        assert!(d.retired().is_empty());
        assert_eq!(f.timeout(t + CLOSE_LINGER).retired(), vec![f.our_index]);
    }

    /// **Plan derivation, not a spec rule — recorded rather than pinned.**
    ///
    /// `PLAN.md` U4 asks what a *closing* connection does with a
    /// structurally invalid frame stream. §15.2's retention list is
    /// exhaustive (seal capability, receive cipher states, replay window),
    /// so there is nothing left for most frames to be applied to, but the
    /// spec does not say whether a second violation re-signals.
    ///
    /// This test asserts only what **both** readings share: `Closed` is
    /// still emitted exactly once (Q3), the reported cause is the *first*
    /// death's, and the linger deadline does not move. It deliberately says
    /// nothing about whether a transmit is produced. See
    /// `owed::VIOLATION_WHILE_CLOSING`.
    #[test]
    fn a_violation_while_closing_neither_re_reports_nor_extends_the_linger() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, NO_ERROR, b"");

        let d = f.deliver(t + Duration::from_secs(2), &vi(0x7f));
        assert!(
            d.closed().is_empty(),
            "Q3: Closed was already emitted, got {:?}",
            d.outs
        );
        assert_eq!(
            d.deadline,
            Some(t + CLOSE_LINGER),
            "the linger deadline is unmoved"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §16.7 — plan, seal, commit; sealing is synchronous
// ═══════════════════════════════════════════════════════════════════════

mod seal_order {
    use super::*;

    /// **T4.1** — §16.7: *"Sealing — commit included — executes **within
    /// the mutating call that triggers it** …, never lazily inside
    /// `poll_output()`."*
    ///
    /// The obvious test ("`close()` produces a `ConnOutput::Transmit`")
    /// passes a lazy build. The separating observation is the **counter**:
    /// after `close()` and *before any `poll_output()` call at all*, hiss's
    /// send counter has already advanced, because the seal has already
    /// happened. A lazy implementation has not sealed yet and still reads
    /// 0.
    ///
    /// No test-only accessor is needed: §16.4's `session()` plus
    /// `EstablishedSession::seal` reach `next_counter()` already.
    #[test]
    fn close_seals_before_any_poll_output() {
        let t = t0();
        let mut f = established_at(t);
        assert_eq!(f.next_counter(), 0);

        f.conn.close(t, 0, b"bye");
        assert_eq!(
            f.next_counter(),
            1,
            "§16.7: the CLOSE is sealed inside close(), not inside poll_output()"
        );

        // And draining hands out the already-sealed bytes rather than
        // sealing a second time.
        let d = f.drain();
        assert_eq!(d.transmits().len(), 1);
        assert_eq!(f.next_counter(), 1, "the drain seals nothing");
    }

    /// The same rule on the receive path: a linger reply is sealed inside
    /// `handle_datagram`.
    ///
    /// Catches: a build that queues "reply owed" and seals at drain time —
    /// which would make the reply's counter, and therefore its position in
    /// the packet-number space, depend on when the driver happened to poll.
    #[test]
    fn a_linger_reply_seals_inside_handle_datagram() {
        let t = t0();
        let mut f = established_at(t);
        let _ = f.close(t, 0, b"");
        assert_eq!(f.next_counter(), 1);

        let dgram = f.peer.seal(&padding(1));
        let addr = f.peer.addr;
        f.conn
            .handle_datagram(t + Duration::from_secs(2), addr, &dgram);
        assert_eq!(
            f.next_counter(),
            2,
            "§16.7: the reply is sealed inside handle_datagram"
        );
        let d = f.drain();
        assert_eq!(d.transmits().len(), 1);
        assert_eq!(f.next_counter(), 2);
    }

    /// The negative half, which is what makes the two tests above a pin
    /// rather than a coincidence: a call that produces **no** packet must
    /// not burn a counter.
    ///
    /// Catches: a "seal eagerly, discard if unused" implementation, which
    /// would advance the counter on every rate-capped inbound packet and
    /// silently shrink §7.9's space.
    #[test]
    fn a_call_that_sends_nothing_burns_no_counter() {
        let t = t0();
        let mut f = established_at(t);

        let dgram = f.peer.seal(&padding(1));
        let _ = f.feed(t, &dgram);
        assert_eq!(f.next_counter(), 0, "a receive alone seals nothing");

        let _ = f.close(t, 0, b"");
        assert_eq!(f.next_counter(), 1);

        // Ruling 83: the opening CLOSE is **not** a reply, so the rate clock
        // is still unset here and this first reply is owed immediately. It is
        // the reply that starts the clock.
        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(t + Duration::from_millis(1), &dgram);
        assert_eq!(d.transmits().len(), 1, "the first reply is not capped");
        assert_eq!(f.next_counter(), 2, "and it burned exactly one counter");

        // Rate-capped: inside the reply interval, nothing is sent …
        let dgram = f.peer.seal(&padding(1));
        let d = f.feed(t + Duration::from_millis(2), &dgram);
        assert!(d.transmits().is_empty(), "capped by §15.2's 1 Hz rule");
        assert_eq!(f.next_counter(), 2, "… and nothing is sealed");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Unit tests against module-internal seams
// ═══════════════════════════════════════════════════════════════════════
//
// **Read this before "fixing" a name below.**
//
// The three modules in this section test properties that are, by
// `PLAN.md` §9's own argument, **unreachable through the connection's
// behaviour in slice 3a**:
//
// - §8.7's ack-eliciting classifier (T3): every frame slice 3a builds is
//   in the "never" retransmission class and PING is the only ack-eliciting
//   one among them, so the classifier is correct-by-accident for the whole
//   slice. `matches!(frame, Frame::Ping)` passes every flow test above.
// - §16.5's equal-deadline order (T6): slice 3a arms **one** timer, so
//   every ordering satisfies every flow test.
// - §12.2's descending range walk: the ACK frame is slice 5, so nothing in
//   slice 3a reads the window's shape.
//
// TEST-A cannot see the implementation (working rule 6), so the **names**
// below come from `PLAN.md` §2.1 (module paths) and §9 (T3's and T6's
// signatures) plus this slice's brief (`greatest`, `ranges_desc`). If the
// implementation named something differently, **rename the call. Never the
// assertion** — the assertions are the deliverable and each was chosen
// because a plausible wrong implementation fails it.

mod ack_eliciting_classifier {
    use super::*;
    use crate::core::connection::frame::is_ack_eliciting;

    /// **T3** — §8.7: *"A packet is ack-eliciting iff it contains at least
    /// one ack-eliciting frame (§8.3's column)."*
    ///
    /// Asserted over **all** of §8.3's rows, including the frames slice 3a
    /// cannot construct, so that slices 4–6 inherit a tested classifier
    /// instead of re-deriving one from prose.
    ///
    /// Catches: `matches!(frame, Frame::Ping)`, which returns `false` for
    /// RESET_STREAM, every STREAM, all four credit frames and both
    /// DATAGRAMs — nine wrong answers no slice-3a flow test can see.
    ///
    /// `0x05` is deliberately absent: §8.3's row is `—`, and a reserved
    /// type is a structural failure (§8.2) rather than a classified frame.
    /// Inventing an answer for it would pin something the spec does not
    /// say.
    #[test]
    fn every_row_of_the_frame_table_is_classified() {
        let expected: &[(u64, bool)] = &[
            (FRAME_PADDING, false),
            (FRAME_PING, true),
            (FRAME_ACK, false),
            (FRAME_RESET_STREAM, true),
            (FRAME_MAX_DATA, true),
            (FRAME_MAX_STREAM_DATA, true),
            (FRAME_MAX_STREAMS_BIDI, true),
            (FRAME_MAX_STREAMS_UNI, true),
            (FRAME_CLOSE, false),
            (FRAME_DATAGRAM, true),
            (FRAME_DATAGRAM_LEN, true),
        ];
        for (ty, want) in expected {
            assert_eq!(
                is_ack_eliciting(*ty),
                *want,
                "§8.3: type {ty:#x} is ack-eliciting = {want}"
            );
        }

        // §8.3's STREAM row is eight types, not one: `0x08`–`0x0f`, all
        // ack-eliciting. A classifier written as `ty == 0x08` passes the
        // table above and fails here.
        for ty in FRAME_STREAM_BASE..=FRAME_STREAM_MAX {
            assert!(
                is_ack_eliciting(ty),
                "§8.3: every STREAM type {ty:#x} is ack-eliciting"
            );
        }
    }

    /// The classifier is a function of the **type code**, and the two
    /// axes §7.4 insists are independent must not have been collapsed:
    /// the credit frames are ack-eliciting **and** liveness-neutral, and
    /// CLOSE is neither.
    ///
    /// Catches: a classifier derived from "is this in the quiet set?",
    /// which §7.4 says is a different partition of the same table.
    #[test]
    fn ack_eliciting_is_not_the_complement_of_the_quiet_set() {
        // Quiet set (§7.4) yet ack-eliciting (§8.3): the credit frames and
        // RESET_STREAM.
        assert!(is_ack_eliciting(FRAME_MAX_DATA));
        assert!(is_ack_eliciting(FRAME_RESET_STREAM));
        // Quiet set and *not* ack-eliciting: ACK and CLOSE.
        assert!(!is_ack_eliciting(FRAME_ACK));
        assert!(!is_ack_eliciting(FRAME_CLOSE));
    }
}

mod timer_table {
    use super::*;
    use crate::core::connection::timers::{TimerKind, Timers};

    /// Integration shims (see the module note). IMPL-A's table is indexed by
    /// `TimerKind` rather than holding named fields. Every helper here
    /// **delegates** — none reimplements a decision under test.
    fn clear(t: &mut Timers, k: TimerKind) {
        t.disarm(k);
    }

    /// The same table, built from an explicit arming list.
    fn armed(pairs: &[(TimerKind, Instant)]) -> Timers {
        let mut t = Timers::default();
        for (k, at) in pairs {
            t.arm(*k, *at);
        }
        t
    }

    /// TEST-A modelled `due()` as the single highest-priority due timer.
    /// IMPL-A returns the whole due set **in ruling 76's order**, so its
    /// first element is that same timer.
    fn first_due(t: &Timers, now: Instant) -> Option<TimerKind> {
        t.due(now).iter().next()
    }

    /// §16.5: *"single min-deadline out."*
    #[test]
    fn next_is_the_minimum_over_the_armed_timers_and_none_when_bare() {
        let t = t0();
        let mut timers = Timers::default();
        assert_eq!(timers.next(), None, "nothing armed, no deadline");
        assert_eq!(first_due(&timers, t), None);

        timers.set(TimerKind::CloseLinger, Some(t + Duration::from_secs(5)));
        timers.set(TimerKind::Liveness, Some(t + Duration::from_secs(25)));
        assert_eq!(
            timers.next(),
            Some(t + Duration::from_secs(5)),
            "the minimum, not the first field"
        );

        timers.set(TimerKind::Loss, Some(t + Duration::from_secs(1)));
        assert_eq!(timers.next(), Some(t + Duration::from_secs(1)));
        assert_eq!(
            first_due(&timers, t),
            None,
            "nothing is due before its deadline"
        );
        assert_eq!(
            first_due(&timers, t + Duration::from_secs(1)),
            Some(TimerKind::Loss)
        );
    }

    /// **T6** — §16.5's equal-deadline priorities, ratified exhaustive by
    /// ruling 76, asserted as a **total order** in one place.
    ///
    /// Every timer armed at the same instant, popped one at a time. The
    /// order is §16.5's, written out: teardown collection (liveness, then
    /// the `CloseLinger` expiry, then `Contested`) precedes keepalive
    /// evaluation; `AckDelay` fires after the loss evaluation at the same
    /// instant; `PersistentKeepalive` is evaluated last.
    ///
    /// `Pto` is left out of the walk on purpose — §16.5 says "exactly one
    /// of [loss detection and PTO] fires per evaluation", which is the
    /// connection's obligation, not the table's, and is tested pairwise
    /// below.
    ///
    /// Catches: a `due()` that returns the first armed field in
    /// declaration order, or in `Option`-comparison order. Slice 3a arms
    /// one timer, so nothing else in this file can see any of it.
    #[test]
    fn the_ratified_equal_deadline_order_is_a_total_order() {
        let t = t0();
        let mut timers = armed(&[
            (TimerKind::Liveness, t),
            (TimerKind::CloseLinger, t),
            (TimerKind::Contested, t),
            (TimerKind::Loss, t),
            (TimerKind::AckDelay, t),
            (TimerKind::Keepalive, t),
            (TimerKind::PersistentKeepalive, t),
        ]);

        let mut order = vec![];
        while let Some(k) = first_due(&timers, t) {
            order.push(k);
            clear(&mut timers, k);
        }
        assert_eq!(
            order,
            vec![
                TimerKind::Liveness,
                TimerKind::CloseLinger,
                TimerKind::Contested,
                TimerKind::Loss,
                TimerKind::AckDelay,
                TimerKind::Keepalive,
                TimerKind::PersistentKeepalive,
            ],
            "§16.5 (ruling 76): a terminal outcome precedes a routine one"
        );
    }

    /// §16.5: *"loss detection beats PTO."*
    #[test]
    fn loss_beats_pto_at_the_same_instant() {
        let t = t0();
        let timers = armed(&[(TimerKind::Loss, t), (TimerKind::Pto, t)]);
        assert_eq!(first_due(&timers, t), Some(TimerKind::Loss));
    }

    /// §16.5: *"`Liveness` beating `Contested` at the same instant is the
    /// harmless ordering, both being `ConnectionLost::TimedOut`."* Stated
    /// pairwise because it is the pair the spec singles out.
    #[test]
    fn liveness_beats_contested_and_close_linger() {
        let t = t0();
        let timers = armed(&[
            (TimerKind::Liveness, t),
            (TimerKind::Contested, t),
            (TimerKind::CloseLinger, t),
        ]);
        assert_eq!(first_due(&timers, t), Some(TimerKind::Liveness));
    }

    /// §16.5: *"teardown collection … precedes keepalive evaluation — a
    /// session already collected for teardown owes no keepalive."*
    #[test]
    fn teardown_collection_precedes_keepalive_evaluation() {
        let t = t0();
        let timers = armed(&[
            (TimerKind::CloseLinger, t),
            (TimerKind::Keepalive, t),
            (TimerKind::PersistentKeepalive, t),
        ]);
        assert_eq!(first_due(&timers, t), Some(TimerKind::CloseLinger));
    }

    /// §16.5: *"`AckDelay` fires after the loss/PTO evaluation at the same
    /// instant (the owed ACK then rides any probe or retransmission that
    /// evaluation produced, §8.5)."*
    #[test]
    fn ack_delay_fires_after_the_loss_evaluation() {
        let t = t0();
        let timers = armed(&[(TimerKind::AckDelay, t), (TimerKind::Loss, t)]);
        assert_eq!(first_due(&timers, t), Some(TimerKind::Loss));

        let timers = armed(&[(TimerKind::AckDelay, t), (TimerKind::Pto, t)]);
        assert_eq!(first_due(&timers, t), Some(TimerKind::Pto));
    }

    /// A later deadline never pre-empts an earlier one, whatever the
    /// priority order says. Priority breaks *ties*; it is not a sort key.
    ///
    /// Catches: a `due()` that scans in priority order and returns the
    /// first timer that is due *at all*, ignoring which is earlier — a
    /// build that passes every equal-instant test above.
    #[test]
    fn priority_breaks_ties_and_does_not_reorder_distinct_deadlines() {
        let t = t0();
        let timers = armed(&[
            (TimerKind::Liveness, t + Duration::from_secs(10)),
            (TimerKind::PersistentKeepalive, t),
        ]);
        assert_eq!(timers.next(), Some(t));
        assert_eq!(
            first_due(&timers, t),
            Some(TimerKind::PersistentKeepalive),
            "the lowest-priority timer is still the only due one"
        );
    }
}

mod replay_window_api {
    use super::*;
    use crate::core::connection::session::ReplayWindow;

    /// §7.2 fused to §12.2: the window is "the single received-packet
    /// record", and §12.2 needs its shape as **descending, newest-first**
    /// ranges.
    ///
    /// Catches: an ascending walk (the natural one over a bitmap read
    /// low-to-high), and a walk that emits one range per counter instead of
    /// merging runs. Slice 5 builds the ACK frame on top of this and cannot
    /// re-derive the order from prose without one of the two going wrong.
    #[test]
    fn ranges_are_descending_newest_first_and_merged() {
        let mut w = ReplayWindow::default();
        assert_eq!(w.greatest(), None, "a fresh window has seen nothing");

        for c in [0u64, 1, 2, 5, 9] {
            assert!(w.check_and_mark(c), "counter {c} is fresh");
        }
        assert_eq!(w.greatest(), Some(9));

        let ranges: Vec<RangeInclusive<u64>> = w.ranges_desc().collect();
        assert_eq!(
            ranges,
            vec![9..=9, 5..=5, 0..=2],
            "§12.2: newest-first, descending, runs merged"
        );
    }

    /// The window's own contract, stated as the check-and-mark it is
    /// (§7.2): fresh once, never twice, and `greatest` advances only
    /// forward.
    #[test]
    fn a_counter_is_fresh_exactly_once() {
        let mut w = ReplayWindow::default();
        assert!(w.check_and_mark(7));
        assert!(!w.check_and_mark(7), "§7.2: a duplicate is not fresh");
        assert!(w.check_and_mark(3), "an older, unseen counter is fresh");
        assert_eq!(w.greatest(), Some(7), "and does not move `greatest` back");
    }

    /// §7.2's lower edge, from both sides, at the window's own level —
    /// the same boundary the connection-level tests pin, asserted here
    /// where an off-by-one is easiest to read.
    #[test]
    fn the_lower_edge_is_the_window_width_exactly() {
        let window = REPLAY_WINDOW as u64;
        let mut w = ReplayWindow::default();
        let greatest = 1_000_000u64;
        assert!(w.check_and_mark(greatest));

        assert!(
            w.check_and_mark(greatest - window),
            "§7.2: exactly {window} behind is not *more than* {window} behind"
        );
        assert!(
            !w.check_and_mark(greatest - window - 1),
            "§7.2: one further back is dropped"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// What slice 3a cannot assert, and who owes it
// ═══════════════════════════════════════════════════════════════════════
//
// Recorded as items rather than as comments buried in test bodies, so that
// a half-assertion elsewhere in this file never reads as a whole pin. Each
// is referenced by name from the test that stops short.

#[allow(dead_code)]
mod owed {
    /// **`Connection::established()` takes no `Instant`.**
    ///
    /// §7.4: *"At install the clock is pinned, and it is pinned armed. A
    /// newly installed session sets both `last_authenticated_recv` and
    /// `last_send` to the install instant."* The `connect()` path gets that
    /// instant from `handle_endpoint_event(now, Install)`. The `accept()`
    /// path — `Connection::established(sub_seed, session)` — has no
    /// argument to pin it to.
    ///
    /// So every liveness test in this file runs through the `Install`
    /// path, and §7.4's install pin is **unstated** for the constructor
    /// §16.4 gives `accept()`. Either `established` gains a `now`, or the
    /// spec says which instant the accept path pins to.
    pub const ESTABLISHED_HAS_NO_INSTANT: () = ();

    /// **§7.4 and §16.5 disagree about one instant.**
    ///
    /// §7.4 states the death condition as
    /// `now − last_authenticated_recv > DEAD_TIMEOUT` — strictly greater.
    /// §16.5 says an armed deadline `D` "fires **no earlier than** `D`",
    /// which admits firing *at* `D`. At exactly
    /// `anchor + DEAD_TIMEOUT` the two readings differ.
    ///
    /// `liveness::a_half_open_session_is_reaped_in_silence_and_not_one_nanosecond_early`
    /// therefore asserts at `D − 1 ns` and `D + 1 ns` only.
    pub const LIVENESS_EXACT_INSTANT: () = ();

    /// **Does the local CLOSE start the ≤ 1/s reply clock?**
    ///
    /// §15.2 says "emit CLOSE and enter closing … Replies are capped at one
    /// CLOSE per second" without saying whether the emitted CLOSE is itself
    /// the first item under the cap. `teardown`'s flood test starts two
    /// seconds after the close so that both readings agree.
    pub const CLOSE_RATE_CLOCK_ORIGIN: () = ();

    /// **`Retired { our_index }` before a session exists.**
    ///
    /// Ruling 81 puts "any teardown before a session exists" in the
    /// no-linger class, so `Closed` is followed by `Retired` in the same
    /// drain. But §17.3 mints a session index at the handshake, and a
    /// `connect()`-created connection that is closed before its install has
    /// none. §16.4 gives `Retired` no other shape and nothing says what it
    /// carries here.
    pub const RETIRED_WITHOUT_A_SESSION: () = ();

    /// **A structural failure *while already closing*.**
    ///
    /// §8.2's structural class is a signalled death; §15.2's closing state
    /// retains only the seal capability, the receive cipher states and the
    /// replay window. Whether a second violation re-signals (a second CLOSE
    /// with a different code, against the 1 Hz reply rule's intent) or is
    /// ignored is `PLAN.md` U4's open question, not a spec rule.
    /// `teardown::a_violation_while_closing_neither_re_reports_nor_extends_the_linger`
    /// asserts only what both readings share.
    pub const VIOLATION_WHILE_CLOSING: () = ();

    /// **§7.4's two seal paths — half of the pin is owed to slice 4.**
    ///
    /// T16: `seal_quiet` must not touch `last_send`, `seal` must. Slice 3a
    /// seals **only** CLOSE, which is `seal_quiet`, so `last_send` is never
    /// read on any path this slice can drive: a build in which `seal_quiet`
    /// is a plain alias for `seal` passes every test in this file. The
    /// separating assertion needs a first-transmission STREAM or DATAGRAM
    /// frame, and arrives with slice 4.
    ///
    /// Do not let the absence of a red test here read as coverage.
    pub const SEAL_VERSUS_SEAL_QUIET: () = ();

    /// **§8.5's packing order is untestable with one frame.**
    ///
    /// "ACK first (if owed), then control frames …, then STREAM and
    /// DATAGRAM fill, then PING last" — slice 3a emits exactly one frame
    /// type, CLOSE, and never coalesces. The order, and the
    /// one-extends-to-end-frame rule, are owed by slices 4 and 5.
    pub const PACKING_ORDER: () = ();

    /// **An over-MTU Data packet.**
    ///
    /// §8.6 bounds what slither *seals*; §3.1's pre-AEAD gate is where an
    /// oversize inbound packet would be dropped, and §3.1 states the gate
    /// for the handshake packet types. Nothing in §7 or §8 says what an
    /// authenticated Data packet longer than `MAX_DATAGRAM` does, and no
    /// test here pins one, because either answer (silent drop, or open and
    /// parse) is defensible from the text.
    pub const OVERSIZE_DATA_PACKET: () = ();
}

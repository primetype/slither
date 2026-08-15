//! The shared in-crate fixture for the connection core's tests.
//!
//! Extracted verbatim from `tests_streams.rs` (slice 4a's blind test
//! author's file) at slice 5's dispatch, because slice 5's **two** blind
//! test authors both need it and neither may touch that file — CLAUDE.md
//! working rule 6. The extraction moved code and changed only visibility;
//! no assertion and no fixture behaviour was altered, and the lib test
//! count is unchanged across it.
//!
//! - [`Pair`] is **two real `Connection` cores** over a hand-driven wire.
//! - [`Solo`] is **one core plus a raw hiss half**, so a test can seal a
//!   frame stream the core has no verb for.
//!
//! Assertions belong in the test files, never here.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU64;
use std::time::Instant;

use super::*;

// ═══════════════════════════════════════════════════════════════════════
// The integration seam
// ═══════════════════════════════════════════════════════════════════════
//
// `CONTRACT-4a.md` pins these type *names* and their *signatures* but not
// the modules they live in (§1 names `stream_id.rs` for `StreamId`/`Dir`
// and leaves `StreamRef`'s home unstated). **If integration has to touch
// anything in this file, expect it to be these two lines.** Nothing below
// depends on where they live.
use super::stream_id::Dir;
use super::streams::StreamRef;

use crate::constants::{
    DATA_HEADER_LEN, FRAME_CLOSE, FRAME_MAX_DATA, FRAME_MAX_STREAM_DATA, FRAME_MAX_STREAMS_BIDI,
    FRAME_MAX_STREAMS_UNI, FRAME_PADDING, FRAME_PING, FRAME_RESET_STREAM, FRAME_STREAM_BASE,
    MAX_PLAINTEXT, PKT_DATA, PROLOGUE, REKEY_EPOCH_MSGS, STREAM_FIN, STREAM_LEN, STREAM_OFF,
    VERSION,
};
use crate::core::{EstablishedSession, Install, Role, Transmit};
use crate::error::ConnectionLost;
use crate::identity::Identity;
use crate::packet::{Handshake, ReferenceSuite};
use crate::testutil::CountingIdentity;
use crate::varint::{self, VarInt};

type Suite = ReferenceSuite;
type Id = CountingIdentity<Suite>;

/// **The one signature this file guesses.**
///
/// Ruling 93 and its amendment require a core-level abandonment of a
/// receive half — "dropping a `RecvStream`" is 4b's *handle*, but the
/// state change is 4a's. `CONTRACT-4a.md` §2 lists seven verbs and no
/// abandonment among them, and ruling 95 counts "eleven sites: the five
/// verbs, `accept`, `stream_id`, and the four stream-naming `ConnEvent`s"
/// — a list with the same unstated scope working rule 8 hunts.
///
/// Every abandonment test goes through this shim so the whole guess is one
/// line. `now` is present because the operation emits credit frames
/// (MAX_DATA, MAX_STREAMS) and every mutating core call takes an instant.
///
/// **Reported in `TESTS-4a.md` §5 as the highest-probability compile
/// break in this file.**
pub(crate) fn abandon_recv(conn: &mut Connection<Suite>, now: Instant, r: StreamRef) {
    conn.abandon_recv(now, r);
}

// ═══════════════════════════════════════════════════════════════════════
// The wire, decoded by hand
// ═══════════════════════════════════════════════════════════════════════

/// One frame off the wire, in this file's own vocabulary.
///
/// Deliberately not `super::frame::Frame`: a codec that agrees with itself
/// about a private type and writes the wrong bytes must fail here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wire {
    Padding,
    Ping,
    Close {
        code: u64,
        reason: Vec<u8>,
    },
    Reset {
        id: u64,
        code: u64,
        final_size: u64,
    },
    Stream {
        id: u64,
        offset: u64,
        data: Vec<u8>,
        fin: bool,
        /// Whether the frame carried an explicit LEN. `false` is §8.4's
        /// extends-to-end form, which must be the packet's final frame.
        had_len: bool,
    },
    MaxData(u64),
    MaxStreamData {
        id: u64,
        max: u64,
    },
    MaxStreamsBidi(u64),
    MaxStreamsUni(u64),
}

/// Pull one varint, advancing the cursor. Panics on truncation — a
/// truncated frame stream is a failure, not a `None`.
pub(crate) fn take_varint(buf: &[u8], at: &mut usize) -> u64 {
    let (v, n) = varint::decode(&buf[*at..]).expect("a complete varint");
    *at += n;
    u64::from(v)
}

/// Decode a whole plaintext frame stream (§8.3).
///
/// Panics, with the offending type byte named, on a frame type slice 4
/// cannot legitimately emit. That panic is itself an assertion: a core
/// emitting an ACK in slice 4 has crossed the slice boundary.
pub(crate) fn parse_frames(pt: &[u8]) -> Vec<Wire> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < pt.len() {
        let ty = take_varint(pt, &mut at);
        match ty {
            t if t == FRAME_PADDING => out.push(Wire::Padding),
            t if t == FRAME_PING => out.push(Wire::Ping),
            t if t == FRAME_CLOSE => {
                let code = take_varint(pt, &mut at);
                let len = take_varint(pt, &mut at) as usize;
                let reason = pt[at..at + len].to_vec();
                at += len;
                out.push(Wire::Close { code, reason });
            }
            t if t == FRAME_RESET_STREAM => {
                let id = take_varint(pt, &mut at);
                let code = take_varint(pt, &mut at);
                let final_size = take_varint(pt, &mut at);
                out.push(Wire::Reset {
                    id,
                    code,
                    final_size,
                });
            }
            t if (FRAME_STREAM_BASE..=crate::constants::FRAME_STREAM_MAX).contains(&t) => {
                let id = take_varint(pt, &mut at);
                let offset = if t & STREAM_OFF != 0 {
                    take_varint(pt, &mut at)
                } else {
                    0
                };
                let had_len = t & STREAM_LEN != 0;
                let len = if had_len {
                    take_varint(pt, &mut at) as usize
                } else {
                    pt.len() - at
                };
                let data = pt[at..at + len].to_vec();
                at += len;
                out.push(Wire::Stream {
                    id,
                    offset,
                    data,
                    fin: t & STREAM_FIN != 0,
                    had_len,
                });
            }
            t if t == FRAME_MAX_DATA => out.push(Wire::MaxData(take_varint(pt, &mut at))),
            t if t == FRAME_MAX_STREAM_DATA => {
                let id = take_varint(pt, &mut at);
                let max = take_varint(pt, &mut at);
                out.push(Wire::MaxStreamData { id, max });
            }
            t if t == FRAME_MAX_STREAMS_BIDI => {
                out.push(Wire::MaxStreamsBidi(take_varint(pt, &mut at)));
            }
            t if t == FRAME_MAX_STREAMS_UNI => {
                out.push(Wire::MaxStreamsUni(take_varint(pt, &mut at)));
            }
            other => panic!(
                "slice 4 emitted frame type {other:#x}, which §8.3 does not \
                 place in this slice"
            ),
        }
    }
    out
}

// ── encoders, for the frames the core has no verb to produce ───────────

pub(crate) fn put(out: &mut Vec<u8>, v: u64) {
    varint::encode(VarInt::new(v).expect("fits the 62-bit space"), out);
}

/// A STREAM frame with explicit OFF and LEN — the form that can sit
/// anywhere in a packet.
pub(crate) fn stream_frame(id: u64, offset: u64, data: &[u8], fin: bool) -> Vec<u8> {
    let mut f = Vec::new();
    let mut ty = FRAME_STREAM_BASE | STREAM_OFF | STREAM_LEN;
    if fin {
        ty |= STREAM_FIN;
    }
    put(&mut f, ty);
    put(&mut f, id);
    put(&mut f, offset);
    put(&mut f, data.len() as u64);
    f.extend_from_slice(data);
    f
}

pub(crate) fn reset_frame(id: u64, code: u64, final_size: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_RESET_STREAM);
    put(&mut f, id);
    put(&mut f, code);
    put(&mut f, final_size);
    f
}

pub(crate) fn max_stream_data_frame(id: u64, max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_STREAM_DATA);
    put(&mut f, id);
    put(&mut f, max);
    f
}

pub(crate) fn max_data_frame(max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_DATA);
    put(&mut f, max);
    f
}

pub(crate) fn max_streams_uni_frame(max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_STREAMS_UNI);
    put(&mut f, max);
    f
}

pub(crate) fn max_streams_bidi_frame(max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_STREAMS_BIDI);
    put(&mut f, max);
    f
}

// ── §9.1 ids, computed here rather than through the implementation ─────

/// §9.1's encoding, written out so a swapped bit is a test failure and not
/// a shared mistake: `index << 2 | dir_bit << 1 | opener_bit`, where
/// `0x01` is 0 for the connection **initiator** and `0x02` is 0 for
/// **bidirectional**.
pub(crate) fn raw_id(index: u64, dir: Dir, opened_by_initiator: bool) -> u64 {
    let dir_bit = match dir {
        Dir::Bi => 0,
        Dir::Uni => 0x02,
    };
    let opener_bit = u64::from(!opened_by_initiator);
    (index << 2) | dir_bit | opener_bit
}

// ═══════════════════════════════════════════════════════════════════════
// Handshake, shared by both fixtures
// ═══════════════════════════════════════════════════════════════════════

pub(crate) const A_INDEX: u32 = 0x1111_1111;
pub(crate) const B_INDEX: u32 = 0x2222_2222;

pub(crate) fn v4(a: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), port)
}

pub(crate) fn a_addr() -> SocketAddr {
    v4(1, 1)
}

pub(crate) fn b_addr() -> SocketAddr {
    v4(2, 2)
}

pub(crate) fn t0() -> Instant {
    Instant::now()
}

/// One complete IK handshake into **two** `EstablishedSession`s.
///
/// `CONTRACT-4a.md` §6 is explicit that no two-core fixture exists and
/// that whoever needs one writes it inside their own file; this is a copy
/// of `tests.rs:345`'s body with both halves kept rather than one wrapped
/// in a raw `Peer`.
pub(crate) fn handshake_pair() -> (EstablishedSession<Suite>, EstablishedSession<Suite>) {
    let epoch = NonZeroU64::new(REKEY_EPOCH_MSGS).expect("REKEY_EPOCH_MSGS is nonzero");

    let a: Id = CountingIdentity::seeded([7u8; 32]);
    let b: Id = CountingIdentity::seeded([9u8; 32]);
    let b_pub = *b.public_static();

    let (ap, ask) = a.open().expect("identity opens");
    let (bp, bsk) = b.open().expect("identity opens");

    let init = <Suite as Handshake>::initiator(ap, PROLOGUE, b_pub);
    let (msg1, sent) =
        <Suite as Handshake>::write_msg1(init, ask, &[0u8; crate::constants::MSG1_PAYLOAD_LEN])
            .expect("msg1");

    let resp = <Suite as Handshake>::responder(bp, PROLOGUE, bsk).expect("responder");
    let (_claimed, mid) = <Suite as Handshake>::read_msg1_intro(resp, &msg1).expect("msg1 intro");
    let (_payload, read) = <Suite as Handshake>::complete(mid).expect("complete");
    let (msg2, b_transport) = <Suite as Handshake>::write_msg2(read).expect("msg2");
    let a_transport = <Suite as Handshake>::read_msg2(sent, &msg2).expect("read msg2");

    let (a_seal, a_open) = <Suite as Handshake>::into_datagram(a_transport, epoch);
    let (b_seal, b_open) = <Suite as Handshake>::into_datagram(b_transport, epoch);

    (
        EstablishedSession {
            seal: a_seal,
            open: a_open,
            our_index: A_INDEX,
            peer_index: B_INDEX,
            anchor: b_addr(),
        },
        EstablishedSession {
            seal: b_seal,
            open: b_open,
            our_index: B_INDEX,
            peer_index: A_INDEX,
            anchor: a_addr(),
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════
// Drain bookkeeping
// ═══════════════════════════════════════════════════════════════════════

/// Everything one `poll_output()` loop produced, plus its deadline.
///
/// A `Vec` rather than a match-one-at-a-time loop because §16.4 makes
/// **generation order** normative.
#[derive(Debug, Default, Clone)]
pub(crate) struct Drained {
    pub(crate) outs: Vec<ConnOutput>,
    pub(crate) deadline: Option<Instant>,
}

impl Drained {
    pub(crate) fn transmits(&self) -> Vec<Transmit> {
        self.outs
            .iter()
            .filter_map(|o| match o {
                ConnOutput::Transmit(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many events satisfy `f`. Count assertions are how ruling 99 is
    /// pinned: "six events", not "an event".
    pub(crate) fn count_events(&self, f: impl Fn(&ConnEvent) -> bool) -> usize {
        self.outs
            .iter()
            .filter(|o| matches!(o, ConnOutput::Event(e) if f(e)))
            .count()
    }

    pub(crate) fn closed(&self) -> Option<ConnectionLost> {
        self.outs.iter().find_map(|o| match o {
            ConnOutput::Event(ConnEvent::Closed(l)) => Some(l.clone()),
            _ => None,
        })
    }

    pub(crate) fn position(&self, f: impl Fn(&ConnOutput) -> bool) -> Option<usize> {
        self.outs.iter().position(f)
    }
}

pub(crate) fn drain(conn: &mut Connection<Suite>) -> Drained {
    let mut d = Drained::default();
    for _ in 0..200_000 {
        match conn.poll_output() {
            ConnOutput::Timeout(t) => {
                d.deadline = t;
                return d;
            }
            other => d.outs.push(other),
        }
    }
    panic!("poll_output() did not reach the terminal Timeout (§16.4)");
}

// ═══════════════════════════════════════════════════════════════════════
// Fixture 1 — two cores
// ═══════════════════════════════════════════════════════════════════════

/// Two real `Connection` cores and the wire between them.
///
/// `a` is the connection **initiator** (§9.1's opener bit 0), `b` the
/// acceptor. Both are created by `connecting()` and installed through
/// `handle_endpoint_event`, so neither can distinguish itself by its
/// constructor — which is exactly the state ruling 106 says parity must
/// survive.
pub(crate) struct Pair {
    pub(crate) a: Connection<Suite>,
    pub(crate) b: Connection<Suite>,
    /// Datagrams A produced that have not been handed to B, and vice
    /// versa. Held so a test can withhold one stream's packets (§11.2).
    pub(crate) a_to_b: Vec<Vec<u8>>,
    pub(crate) b_to_a: Vec<Vec<u8>>,
}

impl Pair {
    /// Both cores installed at `now`.
    pub(crate) fn installed_at(now: Instant) -> Self {
        let (sa, sb) = handshake_pair();
        let mut a = Connection::connecting([0x5au8; 32]);
        let mut b = Connection::connecting([0xa5u8; 32]);
        a.handle_endpoint_event(
            now,
            Install {
                session: sa,
                role: Role::Initiator,
            },
        );
        b.handle_endpoint_event(
            now,
            Install {
                session: sb,
                role: Role::Responder,
            },
        );
        let mut p = Self {
            a,
            b,
            a_to_b: Vec::new(),
            b_to_a: Vec::new(),
        };
        // Discard the two `Established` drains so a test's first drain is
        // its own.
        let _ = p.drain_a();
        let _ = p.drain_b();
        p
    }

    /// Both cores created but **neither installed** — §16.9's early-send
    /// state.
    pub(crate) fn unestablished() -> Self {
        Self {
            a: Connection::connecting([0x5au8; 32]),
            b: Connection::connecting([0xa5u8; 32]),
            a_to_b: Vec::new(),
            b_to_a: Vec::new(),
        }
    }

    pub(crate) fn install(&mut self, now: Instant) {
        let (sa, sb) = handshake_pair();
        self.a.handle_endpoint_event(
            now,
            Install {
                session: sa,
                role: Role::Initiator,
            },
        );
        self.b.handle_endpoint_event(
            now,
            Install {
                session: sb,
                role: Role::Responder,
            },
        );
    }

    /// Drain A, queueing whatever it wants sent.
    pub(crate) fn drain_a(&mut self) -> Drained {
        let d = drain(&mut self.a);
        for t in d.transmits() {
            self.a_to_b.push(t.data);
        }
        d
    }

    pub(crate) fn drain_b(&mut self) -> Drained {
        let d = drain(&mut self.b);
        for t in d.transmits() {
            self.b_to_a.push(t.data);
        }
        d
    }

    /// Hand every queued A→B datagram to B, in order, and drain B.
    pub(crate) fn flush_a_to_b(&mut self, now: Instant) -> Drained {
        let queued = std::mem::take(&mut self.a_to_b);
        let mut all = Drained::default();
        for dgram in queued {
            self.b.handle_datagram(now, a_addr(), &dgram);
            let d = self.drain_b();
            all.outs.extend(d.outs);
            all.deadline = d.deadline;
        }
        all
    }

    pub(crate) fn flush_b_to_a(&mut self, now: Instant) -> Drained {
        let queued = std::mem::take(&mut self.b_to_a);
        let mut all = Drained::default();
        for dgram in queued {
            self.a.handle_datagram(now, b_addr(), &dgram);
            let d = self.drain_a();
            all.outs.extend(d.outs);
            all.deadline = d.deadline;
        }
        all
    }

    /// Drain both and move everything both ways until the wire is quiet,
    /// accumulating every output each side produced.
    pub(crate) fn pump(&mut self, now: Instant) -> (Drained, Drained) {
        let mut da = Drained::default();
        let mut db = Drained::default();
        for _ in 0..4096 {
            // Both sides get an instant every round: a core that owes a
            // credit frame because of a `read()` has no `now` of its own
            // to seal it with. See `tick`.
            self.a.handle_timeout(now);
            self.b.handle_timeout(now);
            let d = self.drain_a();
            da.outs.extend(d.outs);
            let d = self.drain_b();
            db.outs.extend(d.outs);
            if self.a_to_b.is_empty() && self.b_to_a.is_empty() {
                return (da, db);
            }
            let d = self.flush_a_to_b(now);
            db.outs.extend(d.outs);
            let d = self.flush_b_to_a(now);
            da.outs.extend(d.outs);
        }
        panic!("the two cores never went quiet");
    }
}

/// Write all of `data`, returning how many times `write` reported
/// **blocked** (`Ok(0)`).
///
/// The guard turns "blocked with credit available" into a named panic
/// rather than a CI hang.
pub(crate) fn write_all(
    conn: &mut Connection<Suite>,
    now: Instant,
    r: StreamRef,
    data: &[u8],
) -> usize {
    let mut sent = 0usize;
    let mut blocked = 0usize;
    while sent < data.len() {
        match conn
            .write(now, r, &data[sent..])
            .expect("write must not error")
        {
            0 => {
                blocked += 1;
                assert!(
                    blocked <= 1000,
                    "write stayed blocked after {sent} of {} bytes",
                    data.len()
                );
            }
            n => sent += n,
        }
    }
    blocked
}

/// Write until `write` first reports blocked, returning the byte count at
/// which it blocked.
///
/// **The byte count is the assertion** (§11.4): "it blocks eventually" is
/// true of a build that grants nothing and of a build that grants
/// everything on a different schedule.
pub(crate) fn write_until_blocked(conn: &mut Connection<Suite>, now: Instant, r: StreamRef) -> u64 {
    let chunk = vec![0u8; 4096];
    let mut sent = 0u64;
    for _ in 0..100_000 {
        match conn.write(now, r, &chunk).expect("write must not error") {
            0 => return sent,
            n => sent += n as u64,
        }
    }
    panic!("write never blocked after {sent} bytes");
}

/// Drain everything currently readable. Returns the bytes and whether
/// end-of-stream was observed.
pub(crate) fn read_available(
    conn: &mut Connection<Suite>,
    now: Instant,
    r: StreamRef,
) -> (Vec<u8>, bool) {
    let mut got = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    for _ in 0..100_000 {
        match conn.read(now, r, &mut buf).expect("read must not error") {
            None => return (got, true),
            Some(0) => return (got, false),
            Some(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    panic!("read never settled");
}

/// Hand the core an `Instant` without asking it to do anything else.
///
/// **Why this exists, and why it is a reported finding.** `read()` is a
/// mutating call — it drains the contiguous prefix — and §10.3 makes
/// *consumption* the thing that advances credit, so a read can cross the
/// re-grant trigger and owe a MAX_STREAM_DATA or MAX_DATA. Sealing that
/// frame needs an instant (§7.4 marks `last_send` on every seal), and
/// `CONTRACT-4a.md` gives `read()` **no `now`** — against `CLAUDE.md`'s
/// invariant that `now` is an argument on *every* mutating call. A core
/// must therefore either defer the seal to the next call carrying an
/// instant or cache the last one it saw.
///
/// Every test here that expects a credit frame **after a read** hands the
/// core an instant through this first, so it holds under either design
/// rather than passing vacuously under one of them. Recorded in
/// `TESTS-4a.md` §5.
pub(crate) fn tick(conn: &mut Connection<Suite>, now: Instant) {
    conn.handle_timeout(now);
}

/// Claim every currently-claimable stream in `dir`, sorted by wire id.
///
/// Deliberately **not** assuming a claim order: §16.4 says `accept(dir)`
/// returns one stream per call and never says in which order, and ruling
/// 99 only fixes the event *count*. Sorting asserts the claimable **set**,
/// which is the property the spec states. (Reported in `TESTS-4a.md` as an
/// unstated scope.)
pub(crate) fn accept_all(conn: &mut Connection<Suite>, dir: Dir) -> Vec<(u64, StreamRef)> {
    let mut out = Vec::new();
    while let Some(r) = conn.accept(dir) {
        let id = conn
            .stream_id(r)
            .expect("a peer-opened stream exists only after establishment")
            .as_u64();
        out.push((id, r));
        assert!(
            out.len() <= 4096,
            "accept() never stopped returning streams"
        );
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

/// Read **exactly** `want` bytes, no more, so a credit trigger can be
/// approached one byte at a time.
pub(crate) fn read_exactly(
    conn: &mut Connection<Suite>,
    now: Instant,
    r: StreamRef,
    want: usize,
) -> Vec<u8> {
    let mut got = Vec::new();
    let mut buf = vec![0u8; want];
    while got.len() < want {
        let room = want - got.len();
        match conn
            .read(now, r, &mut buf[..room])
            .expect("read must not error")
        {
            None => panic!("end of stream after {} of {want} bytes", got.len()),
            Some(0) => panic!("no data after {} of {want} bytes", got.len()),
            Some(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    got
}

impl Solo {
    /// Deliver `len` bytes of ramp payload on `id` starting at `offset`,
    /// one packet-sized STREAM frame at a time, FIN on the last if asked.
    ///
    /// Returns every output the core produced across the whole delivery.
    pub(crate) fn deliver_stream_bytes(
        &mut self,
        now: Instant,
        id: u64,
        offset: u64,
        len: usize,
        fin: bool,
    ) -> Drained {
        pub(crate) const CHUNK: usize = 1024;
        let payload = ramp(offset as usize, len);
        let mut all = Drained::default();
        let mut at = 0usize;
        while at < len {
            let n = CHUNK.min(len - at);
            let last = at + n == len;
            let f = stream_frame(id, offset + at as u64, &payload[at..at + n], fin && last);
            let d = self.deliver(now, &f);
            all.outs.extend(d.outs);
            all.deadline = d.deadline;
            at += n;
        }
        if len == 0 && fin {
            let d = self.deliver(now, &stream_frame(id, offset, &[], true));
            all.outs.extend(d.outs);
        }
        all
    }

    /// Deliver many small frames, packing as many as fit into each
    /// plaintext (§8.6). Used where the *count* of frames is the point
    /// and one packet each would be a thousand AEAD operations.
    pub(crate) fn deliver_packed(&mut self, now: Instant, frames: &[Vec<u8>]) -> Drained {
        let mut all = Drained::default();
        let mut pt: Vec<u8> = Vec::new();
        for f in frames {
            if !pt.is_empty() && pt.len() + f.len() > MAX_PLAINTEXT {
                let d = self.deliver(now, &pt);
                all.outs.extend(d.outs);
                all.deadline = d.deadline;
                pt.clear();
            }
            pt.extend_from_slice(f);
        }
        if !pt.is_empty() {
            let d = self.deliver(now, &pt);
            all.outs.extend(d.outs);
            all.deadline = d.deadline;
        }
        all
    }
}
pub(crate) struct RawPeer {
    pub(crate) seal: <Suite as Handshake>::Seal,
    pub(crate) open: <Suite as Handshake>::Open,
    /// The peer's own index — what the core under test addresses.
    pub(crate) our_index: u32,
    /// The core-under-test's index — what we address.
    pub(crate) peer_index: u32,
}

impl RawPeer {
    pub(crate) fn from_session(s: EstablishedSession<Suite>) -> Self {
        Self {
            seal: s.seal,
            open: s.open,
            our_index: s.our_index,
            peer_index: s.peer_index,
        }
    }

    pub(crate) fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let counter = self.seal.next_counter();
        let mut dgram = data_header(self.peer_index, counter);
        let mut body = vec![0u8; plaintext.len() + crate::constants::AEAD_TAG_LEN];
        let (_got, n) = self
            .seal
            .encrypt_next(&dgram, plaintext, &mut body)
            .expect("peer seal");
        body.truncate(n);
        dgram.extend_from_slice(&body);
        dgram
    }

    pub(crate) fn open_dgram(&mut self, dgram: &[u8]) -> Vec<u8> {
        let (header, body) = dgram.split_at(DATA_HEADER_LEN);
        assert_eq!(header[0], PKT_DATA, "§3.4 packet type");
        assert_eq!(header[1], VERSION, "§3.4 version");
        assert_eq!(
            u32::from_le_bytes(header[2..6].try_into().unwrap()),
            self.our_index,
            "§3.4 receiver_index routes to the peer"
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
}

pub(crate) fn data_header(receiver_index: u32, counter: u64) -> Vec<u8> {
    let mut h = Vec::with_capacity(DATA_HEADER_LEN);
    h.push(PKT_DATA);
    h.push(VERSION);
    h.extend_from_slice(&receiver_index.to_le_bytes());
    h.extend_from_slice(&counter.to_le_bytes());
    h
}

/// One core under test with a raw peer.
///
/// The core under test is the **responder**, so the raw peer is the
/// connection initiator: peer-opened streams carry opener bit 0, and the
/// core's own uni space is `index << 2 | 0x03` — the space §8.4 says the
/// peer may never send STREAM on, which is ruling 97's subject.
pub(crate) struct Solo {
    pub(crate) conn: Connection<Suite>,
    pub(crate) peer: RawPeer,
}

impl Solo {
    /// A core that has **not** yet been installed, with the two halves of
    /// the session that will install it.
    ///
    /// §16.9 makes such a core writable and nothing it writes can be
    /// sealed until the session exists, so writes to two streams both stay
    /// pending — which is the only way slice 4 can put two streams into
    /// one fill pass (ruling 114).
    pub(crate) fn connecting() -> (
        Connection<Suite>,
        EstablishedSession<Suite>,
        EstablishedSession<Suite>,
    ) {
        let (sa, sb) = handshake_pair();
        (Connection::connecting([0xa5u8; 32]), sa, sb)
    }

    /// Wrap an already-installed core and its peer half.
    pub(crate) fn around(conn: Connection<Suite>, peer: EstablishedSession<Suite>) -> Self {
        Self {
            conn,
            peer: RawPeer::from_session(peer),
        }
    }

    pub(crate) fn installed_at(now: Instant) -> Self {
        let (sa, sb) = handshake_pair();
        let mut conn = Connection::connecting([0xa5u8; 32]);
        conn.handle_endpoint_event(
            now,
            Install {
                session: sb,
                role: Role::Responder,
            },
        );
        let _ = drain(&mut conn);
        Self {
            conn,
            peer: RawPeer::from_session(sa),
        }
    }

    /// Seal `frames` as the peer, feed it, drain.
    pub(crate) fn deliver(&mut self, now: Instant, frames: &[u8]) -> Drained {
        let dgram = self.peer.seal(frames);
        self.conn.handle_datagram(now, a_addr(), &dgram);
        drain(&mut self.conn)
    }

    /// Decode every datagram a drain produced into one flat frame list.
    pub(crate) fn drain_frames(&mut self, d: &Drained) -> Vec<Wire> {
        self.packets(d).into_iter().flatten().collect()
    }

    /// The same, kept **per packet** — §8.5's packing rules are
    /// statements about one packet and are unassertable once flattened.
    pub(crate) fn packets(&mut self, d: &Drained) -> Vec<Vec<Wire>> {
        d.transmits()
            .iter()
            .map(|t| {
                let pt = self.peer.open_dgram(&t.data);
                parse_frames(&pt)
            })
            .collect()
    }

    /// A peer-opened **uni** stream's wire id (opener = initiator = 0,
    /// dir = uni = 1).
    pub(crate) fn peer_uni(index: u64) -> u64 {
        raw_id(index, Dir::Uni, true)
    }

    /// A peer-opened **bidi** stream's wire id.
    pub(crate) fn peer_bidi(index: u64) -> u64 {
        raw_id(index, Dir::Bi, true)
    }

    /// A stream **we** opened in our own uni space — the peer may never
    /// send STREAM or RESET_STREAM on it (§8.4).
    pub(crate) fn our_uni(index: u64) -> u64 {
        raw_id(index, Dir::Uni, false)
    }
}

/// A payload whose every byte is a function of its offset, so an
/// off-by-`n` shift is visible and a `0xAA`-fill is not (§11.1).
pub(crate) fn ramp(offset: usize, len: usize) -> Vec<u8> {
    (offset..offset + len).map(|i| (i % 251) as u8).collect()
}

/// Assert the connection died locally with `code`, sending the peer a
/// CLOSE carrying the same code.
///
/// Both halves, because a build that surfaces the right `ConnectionLost`
/// and puts a different code on the wire is exactly what Appendix B and a
/// peer's operator would see differently.
pub(crate) fn assert_violation(d: &Drained, frames: &[Wire], code: u64) {
    assert_eq!(
        d.closed(),
        Some(ConnectionLost::ProtocolViolation { code }),
        "§8.2: the violation surfaces as ProtocolViolation with its code"
    );
    let closes: Vec<u64> = frames
        .iter()
        .filter_map(|f| match f {
            Wire::Close { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(
        closes,
        vec![code],
        "§8.2: exactly one CLOSE, carrying the same code the peer reads"
    );
}

/// Assert the connection is still live — no `Closed`, nothing on the wire
/// that says otherwise.
pub(crate) fn assert_alive(d: &Drained) {
    assert_eq!(
        d.closed(),
        None,
        "the connection must survive this: {:?}",
        d.outs
    );
}

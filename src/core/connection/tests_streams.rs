//! Slice 4a acceptance tests for §9 (streams) and §10 (flow control).
//!
//! Written by TEST-S **from `CONTRACT-4a.md`, Round 17's rulings and the
//! spec extracts in `.slices/04-streams/PLAN.md` alone**, in parallel with
//! the implementation and without reading it (CLAUDE.md working rule 6).
//!
//! # How these tests are built, and why
//!
//! Two fixtures, and the choice between them is not cosmetic.
//!
//! - [`Pair`] is **two real `Connection` cores** over a hand-driven wire,
//!   side A installed with [`Role::Initiator`] and side B with
//!   [`Role::Responder`]. Both are built through `connecting()` +
//!   `handle_endpoint_event`, which is the shape ruling 106 exists for: if
//!   a core derived §9.1's parity from "I was created by `connect()`" both
//!   sides would claim the initiator's parity, and `Pair` is the only
//!   fixture in which that is visible.
//! - [`Solo`] is **one core plus a raw hiss half**, so a test can seal a
//!   frame stream the core has no verb for — a STREAM frame naming a
//!   stream we may not send on, 1025 disjoint one-byte ranges, a
//!   RESET_STREAM at the varint ceiling. Every §8.4 violation lives here.
//!
//! Assertions are on **bytes and on `ConnOutput`**, never on the
//! implementation's own `Frame` type: [`Wire`] is this file's private
//! decode of the frame stream, so a codec that round-trips its private
//! representation and writes the wrong bytes fails here.
//!
//! # Working rule 9 is the organising principle
//!
//! Every test carries a `Mutation caught:` line naming what a broken build
//! does and which assertion separates it. A bound that a collapsed
//! implementation satisfies for free is not a test, and this slice is
//! made almost entirely of such bounds — "credit never goes negative" is
//! true of an implementation that grants none.
//!
//! # No clock, so no runtime
//!
//! These are sans-io core tests: `now: Instant` is an argument and nothing
//! here reads a clock, so there is no virtual time to pause and
//! `#[tokio::test(start_paused = true)]` would attach a runtime nothing
//! awaits. Plain `#[test]`, no `sleep` anywhere. The paused-clock
//! requirement lands on 4b's `tests/story_streams.rs`, which has futures
//! to drive.

#![allow(clippy::items_after_statements)]
#![allow(clippy::too_many_lines)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

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
use super::stream_id::{Dir, StreamId};
use super::streams::StreamRef;

use crate::constants::{
    DATA_HEADER_LEN, FINAL_SIZE_ERROR, FLOW_CONTROL_ERROR, FRAME_CLOSE, FRAME_MAX_DATA,
    FRAME_MAX_STREAM_DATA, FRAME_MAX_STREAMS_BIDI, FRAME_MAX_STREAMS_UNI, FRAME_PADDING,
    FRAME_PING, FRAME_RESET_STREAM, FRAME_STREAM_BASE, INITIAL_MAX_DATA, INITIAL_MAX_STREAM_DATA,
    INITIAL_MAX_STREAMS_BIDI, INITIAL_MAX_STREAMS_UNI, MAX_PLAINTEXT, PKT_DATA, PROLOGUE,
    PROTOCOL_VIOLATION, REASSEMBLY_CHUNKS_MAX, REKEY_EPOCH_MSGS, STREAM_FIN, STREAM_LEN,
    STREAM_LIMIT_ERROR, STREAM_OFF, STREAM_STATE_ERROR, STREAMS_CREDIT_BATCH, VERSION,
};
use crate::core::{EstablishedSession, Install, Role, Transmit};
use crate::error::{ConnectionLost, ReadError, WriteError};
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
fn abandon_recv(conn: &mut Connection<Suite>, now: Instant, r: StreamRef) {
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
enum Wire {
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
fn take_varint(buf: &[u8], at: &mut usize) -> u64 {
    let (v, n) = varint::decode(&buf[*at..]).expect("a complete varint");
    *at += n;
    u64::from(v)
}

/// Decode a whole plaintext frame stream (§8.3).
///
/// Panics, with the offending type byte named, on a frame type slice 4
/// cannot legitimately emit. That panic is itself an assertion: a core
/// emitting an ACK in slice 4 has crossed the slice boundary.
fn parse_frames(pt: &[u8]) -> Vec<Wire> {
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

fn put(out: &mut Vec<u8>, v: u64) {
    varint::encode(VarInt::new(v).expect("fits the 62-bit space"), out);
}

/// A STREAM frame with explicit OFF and LEN — the form that can sit
/// anywhere in a packet.
fn stream_frame(id: u64, offset: u64, data: &[u8], fin: bool) -> Vec<u8> {
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

fn reset_frame(id: u64, code: u64, final_size: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_RESET_STREAM);
    put(&mut f, id);
    put(&mut f, code);
    put(&mut f, final_size);
    f
}

fn max_stream_data_frame(id: u64, max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_STREAM_DATA);
    put(&mut f, id);
    put(&mut f, max);
    f
}

fn max_data_frame(max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_DATA);
    put(&mut f, max);
    f
}

fn max_streams_uni_frame(max: u64) -> Vec<u8> {
    let mut f = Vec::new();
    put(&mut f, FRAME_MAX_STREAMS_UNI);
    put(&mut f, max);
    f
}

fn max_streams_bidi_frame(max: u64) -> Vec<u8> {
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
fn raw_id(index: u64, dir: Dir, opened_by_initiator: bool) -> u64 {
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

const A_INDEX: u32 = 0x1111_1111;
const B_INDEX: u32 = 0x2222_2222;

fn v4(a: u8, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), port)
}

fn a_addr() -> SocketAddr {
    v4(1, 1)
}

fn b_addr() -> SocketAddr {
    v4(2, 2)
}

fn t0() -> Instant {
    Instant::now()
}

/// One complete IK handshake into **two** `EstablishedSession`s.
///
/// `CONTRACT-4a.md` §6 is explicit that no two-core fixture exists and
/// that whoever needs one writes it inside their own file; this is a copy
/// of `tests.rs:345`'s body with both halves kept rather than one wrapped
/// in a raw `Peer`.
fn handshake_pair() -> (EstablishedSession<Suite>, EstablishedSession<Suite>) {
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

    /// How many events satisfy `f`. Count assertions are how ruling 99 is
    /// pinned: "six events", not "an event".
    fn count_events(&self, f: impl Fn(&ConnEvent) -> bool) -> usize {
        self.outs
            .iter()
            .filter(|o| matches!(o, ConnOutput::Event(e) if f(e)))
            .count()
    }

    fn closed(&self) -> Option<ConnectionLost> {
        self.outs.iter().find_map(|o| match o {
            ConnOutput::Event(ConnEvent::Closed(l)) => Some(l.clone()),
            _ => None,
        })
    }

    fn position(&self, f: impl Fn(&ConnOutput) -> bool) -> Option<usize> {
        self.outs.iter().position(f)
    }
}

fn drain(conn: &mut Connection<Suite>) -> Drained {
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
struct Pair {
    a: Connection<Suite>,
    b: Connection<Suite>,
    /// Datagrams A produced that have not been handed to B, and vice
    /// versa. Held so a test can withhold one stream's packets (§11.2).
    a_to_b: Vec<Vec<u8>>,
    b_to_a: Vec<Vec<u8>>,
}

impl Pair {
    /// Both cores installed at `now`.
    fn installed_at(now: Instant) -> Self {
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
    fn unestablished() -> Self {
        Self {
            a: Connection::connecting([0x5au8; 32]),
            b: Connection::connecting([0xa5u8; 32]),
            a_to_b: Vec::new(),
            b_to_a: Vec::new(),
        }
    }

    fn install(&mut self, now: Instant) {
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
    fn drain_a(&mut self) -> Drained {
        let d = drain(&mut self.a);
        for t in d.transmits() {
            self.a_to_b.push(t.data);
        }
        d
    }

    fn drain_b(&mut self) -> Drained {
        let d = drain(&mut self.b);
        for t in d.transmits() {
            self.b_to_a.push(t.data);
        }
        d
    }

    /// Hand every queued A→B datagram to B, in order, and drain B.
    fn flush_a_to_b(&mut self, now: Instant) -> Drained {
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

    fn flush_b_to_a(&mut self, now: Instant) -> Drained {
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
    fn pump(&mut self, now: Instant) -> (Drained, Drained) {
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
fn write_all(conn: &mut Connection<Suite>, now: Instant, r: StreamRef, data: &[u8]) -> usize {
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
fn write_until_blocked(conn: &mut Connection<Suite>, now: Instant, r: StreamRef) -> u64 {
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
fn read_available(conn: &mut Connection<Suite>, now: Instant, r: StreamRef) -> (Vec<u8>, bool) {
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
fn tick(conn: &mut Connection<Suite>, now: Instant) {
    conn.handle_timeout(now);
}

/// Claim every currently-claimable stream in `dir`, sorted by wire id.
///
/// Deliberately **not** assuming a claim order: §16.4 says `accept(dir)`
/// returns one stream per call and never says in which order, and ruling
/// 99 only fixes the event *count*. Sorting asserts the claimable **set**,
/// which is the property the spec states. (Reported in `TESTS-4a.md` as an
/// unstated scope.)
fn accept_all(conn: &mut Connection<Suite>, dir: Dir) -> Vec<(u64, StreamRef)> {
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
fn read_exactly(conn: &mut Connection<Suite>, now: Instant, r: StreamRef, want: usize) -> Vec<u8> {
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
    fn deliver_stream_bytes(
        &mut self,
        now: Instant,
        id: u64,
        offset: u64,
        len: usize,
        fin: bool,
    ) -> Drained {
        const CHUNK: usize = 1024;
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
    fn deliver_packed(&mut self, now: Instant, frames: &[Vec<u8>]) -> Drained {
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

// ═══════════════════════════════════════════════════════════════════════
// §7.2 — the three precursors
// ═══════════════════════════════════════════════════════════════════════
//
// Named `*_precursor_*` and never `s12_...` outright: they are core-level,
// and a precursor that reads as its story would let 4b ship without the
// handle-level test the story actually asks for (ruling 107).

mod precursors {
    use super::*;

    /// S12's core half: two cores, one stream, bytes out and bytes in.
    ///
    /// Mutation caught: a reassembler that delivers the right *number* of
    /// bytes but shifted, or that re-delivers a duplicated range. The
    /// payload is a function of its offset and the received vector is
    /// compared **by value**, so a shift fails; the length is asserted
    /// **separately**, so truncation and duplication fail differently
    /// (§11.1). A single-packet payload would test none of §9.5, so this
    /// one is 64 KiB — 57 packets at `MAX_PLAINTEXT`.
    #[test]
    fn s12_precursor_two_cores_exchange_a_finished_stream() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        let r =
            p.a.open(Dir::Uni)
                .expect("the first uni stream fits the limit");
        let payload = ramp(0, 64 * 1024);
        assert!(
            payload.len() > MAX_PLAINTEXT * 8,
            "a single-packet S12 tests nothing in §9.5"
        );

        let blocked = write_all(&mut p.a, t, r, &payload);
        assert_eq!(
            blocked, 0,
            "64 KiB is well inside both initial windows; a block here is a \
             ledger that grants nothing"
        );
        p.a.finish(t, r).expect("finish");
        let (_, db) = p.pump(t);

        assert_eq!(
            db.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            1,
            "§9.2: one stream was opened, so one event (ruling 99)"
        );

        let rb =
            p.b.accept(Dir::Uni)
                .expect("the peer-opened stream is claimable");
        assert_eq!(
            p.b.accept(Dir::Uni),
            None,
            "only one stream was opened, so only one is claimable"
        );

        let (got, eof) = read_available(&mut p.b, t, rb);
        assert_eq!(got.len(), payload.len(), "every byte, exactly once");
        assert_eq!(got, payload, "the same bytes, in the same order");
        assert!(eof, "finish() delivered the FIN, so the reader sees EOF");

        // §16.4's `Ok(None)` is a latch, not a one-shot: a reader that
        // polls again must still see EOF rather than a park or an error.
        assert_eq!(
            p.b.read(t, rb, &mut [0u8; 16]),
            Ok(None),
            "end of stream stays end of stream"
        );
    }

    /// S13's core half: two streams, one withheld.
    ///
    /// Mutation caught: **one shared reassembly buffer keyed by offset
    /// rather than by stream**, and a build that only guarantees both
    /// streams eventually complete. The separating assertion is taken
    /// **while A is still missing**: B is fully readable at that moment.
    /// Asserting only "both complete at the end" passes the head-of-line
    /// build (§11.2).
    #[test]
    fn s13_precursor_two_streams_reassemble_independently() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let a = Solo::peer_uni(0);
        let b = Solo::peer_uni(1);

        // Open both spaces' indices with a frame naming index 1: §9.2
        // opens 0 and 1 together.
        let d = s.deliver(t, &stream_frame(b, 0, &ramp(0, 512), true));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            2,
            "§9.2: naming index 1 opens 0 and 1 — two streams, two events"
        );

        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(
            claimed.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![a, b],
            "both implicitly-opened indices are claimable"
        );
        let r0 = claimed[0].1;
        let r1 = claimed[1].1;

        // Stream A's bytes are withheld entirely. Stream B is complete.
        let (got_b, eof_b) = read_available(&mut s.conn, t, r1);
        assert_eq!(got_b, ramp(0, 512), "B is readable while A is missing");
        assert!(eof_b, "B's FIN arrived, so B is at end of stream");

        let (got_a, eof_a) = read_available(&mut s.conn, t, r0);
        assert!(
            got_a.is_empty() && !eof_a,
            "A has no data and no FIN: it must park, not report EOF"
        );

        // Now deliver A, out of order, and it completes on its own.
        let _ = s.deliver(t, &stream_frame(a, 256, &ramp(256, 256), true));
        let _ = s.deliver(t, &stream_frame(a, 0, &ramp(0, 256), false));
        let (got_a, eof_a) = read_available(&mut s.conn, t, r0);
        assert_eq!(got_a, ramp(0, 512), "A reassembles independently of B");
        assert!(eof_a);
    }

    /// S17's core half: the ledger stalls the writer and the reader's
    /// drain resumes it.
    ///
    /// Mutation caught: *build A grants nothing* — the writer blocks and
    /// never resumes; *build B grants unconditionally* — the writer never
    /// blocks at all. The assertion that separates both is the **byte
    /// count at which the writer first blocks**
    /// (`INITIAL_MAX_STREAM_DATA`, exactly) together with the resume,
    /// which build A fails. A test asserting only "it blocks" passes B's
    /// opposite; one asserting only "it resumes" passes A's.
    #[test]
    fn s17_precursor_the_credit_ledger_stalls_and_resumes() {
        let t = t0();
        let mut p = Pair::installed_at(t);
        let r = p.a.open(Dir::Uni).expect("open");

        let at = write_until_blocked(&mut p.a, t, r);
        assert_eq!(
            at, INITIAL_MAX_STREAM_DATA,
            "§10.2: the writer stalls at the peer's initial stream window, \
             not before it and not past it"
        );

        let (_, _) = p.pump(t);
        let rb = p.b.accept(Dir::Uni).expect("the stream opened at the peer");

        // Reading strictly less than half the window must not resume the
        // writer: §10.3's trigger is `WINDOW/2`, and a build that grants on
        // every read passes only if this stays blocked.
        let half = (INITIAL_MAX_STREAM_DATA / 2) as usize;
        let _ = read_exactly(&mut p.b, t, rb, half - 1);
        let (_, _) = p.pump(t);
        assert_eq!(
            p.a.write(t, r, &[0u8; 1]).expect("write"),
            0,
            "§10.3: below WINDOW/2 consumed, no credit is re-granted"
        );

        // One more byte crosses the trigger.
        let _ = read_exactly(&mut p.b, t, rb, 1);
        let (_, _) = p.pump(t);
        let more = write_until_blocked(&mut p.a, t, r);
        assert_eq!(
            more,
            INITIAL_MAX_STREAM_DATA / 2,
            "§10.3's re-grant is absolute — `bytes_read + WINDOW` — so the \
             writer gains exactly the bytes the reader consumed"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9.1 — identifiers, parity, and ruling 106
// ═══════════════════════════════════════════════════════════════════════

mod identifiers {
    use super::*;

    /// Ruling 106's whole point, at core level.
    ///
    /// Mutation caught: a core deriving §9.1's parity from "I was created
    /// by `connect()`". **Both** cores here are created by `connecting()`
    /// and separated only by the `Role` on their `Install` — which is
    /// §6.6 step 4's shape, where a peer that dialled is admitted as the
    /// responder. Under the broken derivation both sides mint id 2 for
    /// their first uni stream and the failure is silent, because each end
    /// still agrees with itself about every stream it opens.
    ///
    /// Asserting only "the two sides' ids differ" would pass a build
    /// allocating from one shared counter (§11.2), so the **exact** ids
    /// are asserted.
    #[test]
    fn stream_ids_carry_the_role_from_install_not_from_who_created_the_core() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        assert_eq!(p.a.role(), Some(Role::Initiator));
        assert_eq!(p.b.role(), Some(Role::Responder));

        let a_uni = p.a.open(Dir::Uni).expect("open");
        let b_uni = p.b.open(Dir::Uni).expect("open");
        let a_bi = p.a.open(Dir::Bi).expect("open");
        let b_bi = p.b.open(Dir::Bi).expect("open");

        let id = |c: &Connection<Suite>, r| c.stream_id(r).expect("established").as_u64();

        assert_eq!(id(&p.a, a_uni), 2, "initiator uni index 0 = 0<<2 | 0x02");
        assert_eq!(id(&p.b, b_uni), 3, "acceptor  uni index 0 = 0<<2 | 0x03");
        assert_eq!(id(&p.a, a_bi), 0, "initiator bidi index 0 = 0");
        assert_eq!(id(&p.b, b_bi), 1, "acceptor  bidi index 0 = 1");

        assert_eq!(id(&p.a, a_uni) & 0x01, 0, "§9.1: the dialler's parity");
        assert_eq!(id(&p.b, b_uni) & 0x01, 1, "§9.1: the acceptor's parity");

        assert!(
            p.a.stream_id(a_uni)
                .expect("established")
                .initiated_by_connection_initiator()
        );
        assert!(
            !p.b.stream_id(b_uni)
                .expect("established")
                .initiated_by_connection_initiator()
        );
    }

    /// §9.1's two-bit tag, asserted against ids this file computes rather
    /// than against the implementation's own encoder.
    ///
    /// Mutation caught: the direction and opener bits swapped. A
    /// round-trip test (`decode(encode(x)) == x`) is symmetric under that
    /// swap and passes it; these are absolute values.
    #[test]
    fn the_two_bit_tag_places_index_direction_and_opener_where_9_1_says() {
        assert_eq!(raw_id(0, Dir::Bi, true), 0);
        assert_eq!(raw_id(0, Dir::Bi, false), 1);
        assert_eq!(raw_id(0, Dir::Uni, true), 2);
        assert_eq!(raw_id(0, Dir::Uni, false), 3);
        assert_eq!(raw_id(7, Dir::Uni, true), 30, "7 << 2 | 0x02");

        for &(index, dir, init) in &[
            (0u64, Dir::Bi, true),
            (1, Dir::Uni, false),
            (999, Dir::Bi, false),
            ((1u64 << 60) - 1, Dir::Uni, true),
        ] {
            let id = StreamId::from_u64(raw_id(index, dir, init));
            assert_eq!(id.index(), index, "§9.1: index is the id shifted by 2");
            assert_eq!(id.dir(), dir);
            assert_eq!(id.initiated_by_connection_initiator(), init);
            assert_eq!(id.as_u64(), raw_id(index, dir, init));
        }
    }

    /// Hunt H1: the id space is 62 bits and the index space is 60, and
    /// they are different numbers.
    ///
    /// Mutation caught: an `index()` that masks or saturates at the
    /// varint ceiling instead of shifting, which is invisible at every
    /// index a test would otherwise reach.
    #[test]
    fn the_index_ceiling_is_two_to_the_sixty_not_the_varint_ceiling() {
        let top = StreamId::from_u64(VarInt::MAX_VALUE);
        assert_eq!(
            top.index(),
            (1u64 << 60) - 1,
            "§9.1: 60 bits of index under a 62-bit varint"
        );
        assert_eq!(top.as_u64(), VarInt::MAX_VALUE, "total, and lossless");
    }

    /// §9.1: four **independent** spaces, each counting from 0.
    ///
    /// Mutation caught: one shared allocator across the spaces, which
    /// gives bidi index 0 then uni index 1 and still produces distinct,
    /// monotone, correctly-tagged ids.
    #[test]
    fn each_space_allocates_indices_from_zero_independently() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        let bi: Vec<u64> = (0..3)
            .map(|_| {
                let r = p.a.open(Dir::Bi).expect("open");
                p.a.stream_id(r).expect("established").index()
            })
            .collect();
        let uni: Vec<u64> = (0..3)
            .map(|_| {
                let r = p.a.open(Dir::Uni).expect("open");
                p.a.stream_id(r).expect("established").index()
            })
            .collect();

        assert_eq!(bi, vec![0, 1, 2], "the bidi space counts from 0");
        assert_eq!(uni, vec![0, 1, 2], "so does the uni space, separately");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §16.9 / ruling 95 — `StreamRef` is stable across install
// ═══════════════════════════════════════════════════════════════════════

mod early_sends {
    use super::*;

    /// Ruling 95's named test.
    ///
    /// Mutation caught: a core that hands back an internal index *typed
    /// as* a key and **remaps at install**, leaving every live handle
    /// stale. That build passes a wholly-pre-establishment test and a
    /// wholly-post-establishment test; it fails only "open early, write
    /// late", which is what this is. The write **after** install goes
    /// through the same `StreamRef` obtained **before** it, and the bytes
    /// from both sides of the install arrive contiguous and in order —
    /// so a remap that silently opened a second stream fails on the
    /// content, not merely on an error.
    #[test]
    fn an_early_opened_stream_keeps_its_handle_across_install() {
        let t = t0();
        let mut p = Pair::unestablished();

        let r =
            p.a.open(Dir::Uni)
                .expect("§16.9: open() before install is legal");
        assert_eq!(
            p.a.stream_id(r),
            None,
            "§16.9: the wire id does not exist until establishment"
        );

        let early = ramp(0, 4096);
        assert_eq!(
            write_all(&mut p.a, t, r, &early),
            0,
            "early writes are ordinary work"
        );

        let d = p.drain_a();
        assert!(
            d.transmits().is_empty(),
            "§16.9: no frame is emitted before install — nothing can send \
             without a session"
        );

        p.install(t);
        let _ = p.drain_a();
        let _ = p.drain_b();

        assert_eq!(
            p.a.stream_id(r).map(StreamId::as_u64),
            Some(2),
            "on install the internal index maps onto the parity the \
             outcome dictated"
        );

        // The same key, after the install.
        let late = ramp(4096, 4096);
        assert_eq!(write_all(&mut p.a, t, r, &late), 0);
        p.a.finish(t, r).expect("finish");
        let _ = p.pump(t);

        let rb =
            p.b.accept(Dir::Uni)
                .expect("exactly one stream reached the peer");
        assert_eq!(
            p.b.accept(Dir::Uni),
            None,
            "a remap that opened a second stream would show up here"
        );
        let (got, eof) = read_available(&mut p.b, t, rb);
        assert_eq!(
            got,
            ramp(0, 8192),
            "§16.9: delivered exactly once, in order"
        );
        assert!(eof);
    }

    /// §16.9's accessor contract, stated on its own so a build that
    /// returns `Some` early fails one test rather than confusing another.
    ///
    /// Mutation caught: `stream_id()` returning an internal index before
    /// establishment — which the shell would publish as `id()`, handing
    /// an application a wire id whose parity is not yet decided.
    #[test]
    fn stream_id_is_none_before_establishment_and_some_after() {
        let t = t0();
        let mut p = Pair::unestablished();
        let r = p.a.open(Dir::Bi).expect("open");
        assert_eq!(p.a.stream_id(r), None);
        p.install(t);
        let _ = p.drain_a();
        assert_eq!(p.a.stream_id(r).map(StreamId::as_u64), Some(0));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9.2 — implicit opening, and rulings 99 / 100
// ═══════════════════════════════════════════════════════════════════════

mod implicit_opening {
    use super::*;

    /// Ruling 99's named test.
    ///
    /// Mutation caught: **one event per frame** instead of one per
    /// stream. The shell would then have to loop `accept()` until `None`
    /// on every wake or lose five streams — a lost wakeup that surfaces
    /// only under the reordering the tests inject. `assert!(events >= 1)`
    /// passes the broken build; the count is the pin.
    #[test]
    fn an_implicit_open_of_six_streams_emits_six_stream_opened_events() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &stream_frame(Solo::peer_uni(5), 0, &ramp(0, 8), false));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            6,
            "§9.2: index 5 opens 0..=5 — six streams, so six events"
        );
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Bi })),
            0,
            "the four spaces are independent: no bidi stream was opened"
        );

        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(
            claimed.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            (0..6).map(Solo::peer_uni).collect::<Vec<_>>(),
            "every implicitly-opened index is claimable, and only those"
        );
    }

    /// Ruling 99's second test: what bounds the event burst.
    ///
    /// Mutation caught: an implementation that **opens first and
    /// validates after**. It is both the wrong error code and an
    /// unbounded event-queue amplification — one small frame, one event
    /// per index named. The separating assertion is **zero** events, not
    /// "the connection died": a build that opens 129 streams, emits 129
    /// events and then kills passes an error-code-only assertion.
    #[test]
    fn a_frame_above_the_cumulative_limit_emits_zero_events_before_the_kill() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let over = Solo::peer_uni(INITIAL_MAX_STREAMS_UNI);
        let d = s.deliver(t, &stream_frame(over, 0, &ramp(0, 8), false));
        let frames = s.drain_frames(&d);

        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "ruling 99: §10.4's limit check runs before the opens it would \
             authorise, so not one event escapes"
        );
        assert_violation(&d, &frames, STREAM_LIMIT_ERROR);
    }

    /// §11.9's two-sided row for the uni cumulative limit.
    ///
    /// Mutation caught: an off-by-one in "opening index `i` requires
    /// cumulative limit > `i`" — a `>=` there kills at index 127, which a
    /// one-sided "128 is fatal" test cannot see.
    #[test]
    fn uni_index_127_opens_and_128_is_a_stream_limit_error() {
        let t = t0();

        let mut alive = Solo::installed_at(t);
        let last = Solo::peer_uni(INITIAL_MAX_STREAMS_UNI - 1);
        let d = alive.deliver(t, &stream_frame(last, 0, &ramp(0, 4), false));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            INITIAL_MAX_STREAMS_UNI as usize,
            "index 127 opens exactly 128 streams — the whole allowance"
        );

        let mut dead = Solo::installed_at(t);
        let over = Solo::peer_uni(INITIAL_MAX_STREAMS_UNI);
        let d = dead.deliver(t, &stream_frame(over, 0, &ramp(0, 4), false));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_LIMIT_ERROR);
    }

    /// §11.9's two-sided row for the bidi cumulative limit.
    #[test]
    fn bidi_index_31_opens_and_32_is_a_stream_limit_error() {
        let t = t0();

        let mut alive = Solo::installed_at(t);
        let last = Solo::peer_bidi(INITIAL_MAX_STREAMS_BIDI - 1);
        let d = alive.deliver(t, &stream_frame(last, 0, &ramp(0, 4), false));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Bi })),
            INITIAL_MAX_STREAMS_BIDI as usize
        );

        let mut dead = Solo::installed_at(t);
        let over = Solo::peer_bidi(INITIAL_MAX_STREAMS_BIDI);
        let d = dead.deliver(t, &stream_frame(over, 0, &ramp(0, 4), false));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_LIMIT_ERROR);
    }

    /// The degenerate watermark.
    ///
    /// Mutation caught: a closed-stream watermark initialised to `0`
    /// rather than "nothing closed yet". §9.2 makes a frame "at or below
    /// the watermark and not currently open" a silent no-op, so under
    /// that initialisation the **first** frame a peer ever sends —
    /// index 0 — is dropped, while every higher index works perfectly.
    /// No other test in this file reaches it, because every other one
    /// opens a higher index first.
    #[test]
    fn the_first_stream_frame_naming_index_zero_opens_it_and_delivers() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &stream_frame(Solo::peer_uni(0), 0, &ramp(0, 16), true));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            1,
            "index 0 is a stream like any other, not a tombstone"
        );
        let r = s.conn.accept(Dir::Uni).expect("index 0 is claimable");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got, ramp(0, 16));
        assert!(eof);
    }

    /// Ruling 100's named test.
    ///
    /// Mutation caught: reading §9.5's "empty, FIN-less frame is a no-op"
    /// as suppressing §9.2's open. That would make the open set depend on
    /// a payload property §9.2 never mentions, and make a legitimate
    /// zero-length write on an open stream indistinguishable in the codec
    /// from a stream-creating frame.
    #[test]
    fn an_empty_finless_stream_frame_opens_its_stream() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &stream_frame(Solo::peer_uni(0), 0, &[], false));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            1,
            "ruling 100: §9.2's rule is about the frame, not the payload"
        );
        assert!(
            s.conn.accept(Dir::Uni).is_some(),
            "the opened stream is claimable"
        );
    }

    /// Ruling 100's other half — the half that genuinely *is* a no-op.
    ///
    /// Mutation caught: an implementation that "opens" by pinning a final
    /// size of 0, or by charging a byte of credit, or by making the
    /// stream readable. The stream must be **open and empty**: a later
    /// FIN at offset 10 has to be accepted, which it would not be if the
    /// empty frame had pinned anything.
    #[test]
    fn an_empty_finless_stream_frame_pins_nothing_and_delivers_nothing() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &[], false));
        let r = s.conn.accept(Dir::Uni).expect("open");

        let (got, eof) = read_available(&mut s.conn, t, r);
        assert!(got.is_empty(), "no bytes were delivered");
        assert!(
            !eof,
            "§9.5: no FIN, so no final size — the reader parks (`Ok(Some(0))`), \
             it does not see end of stream"
        );
        assert_eq!(
            s.conn.reassembly_capacity(),
            0,
            "ruling 94: nothing arrived, so nothing is allocated"
        );

        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 10), true));
        assert_alive(&d);
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got, ramp(0, 10), "the empty frame pinned no final size");
        assert!(eof);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 97 — the receive path's check order
// ═══════════════════════════════════════════════════════════════════════
//
// legality → watermark → limit → final size → flow control.
//
// Each test below crafts one frame that trips **two** checks at once and
// asserts the **error code**, which ruling 97 says is the only observable
// difference and is what Appendix B and a peer's operator read.

mod check_order {
    use super::*;

    /// Ruling 97's named test, in the form slice 4 can reach.
    ///
    /// Mutation caught: a receive path with **no legality check at all**,
    /// which silently opens a stream in a space the peer may never send
    /// on — inventing a receive half for a stream we are the only writer
    /// of.
    ///
    /// **Reduced from the ruling's own scenario, deliberately.** Ruling 97
    /// names a *fully-closed* local-uni index, so that §9.2's watermark
    /// no-op and §8.4's `STREAM_STATE_ERROR` both apply and legality is
    /// seen to win. A locally-opened stream can only fully close when its
    /// data or its RESET_STREAM is **acknowledged**, and slice 4 has no
    /// ACK processing — so the local-uni watermark cannot advance in 4a
    /// and that exact frame is unreachable here. Recorded in
    /// `TESTS-4a.md` as owed to slice 5.
    #[test]
    fn a_stream_frame_on_a_closed_local_uni_space_is_a_state_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Our own uni space: the peer is the connection initiator, and
        // this id carries the acceptor's parity with the uni bit, so §8.4
        // says the peer could not send it at any index, closed or not.
        let d = s.deliver(t, &stream_frame(Solo::our_uni(0), 0, &ramp(0, 4), false));
        let frames = s.drain_frames(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "an illegal frame opens nothing"
        );
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }

    /// legality **before** limit.
    ///
    /// Mutation caught: the limit check placed first. The frame names an
    /// index far above the cumulative limit *and* a space the peer may
    /// never send on; a limit-first build answers `STREAM_LIMIT_ERROR`.
    /// Ruling 97: legality is decidable from the id alone — which is only
    /// true because ruling 106 carries the role.
    #[test]
    fn legality_is_checked_before_the_cumulative_limit() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let id = Solo::our_uni(INITIAL_MAX_STREAMS_UNI + 500);
        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 4), false));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }

    /// legality **before** flow control.
    ///
    /// Mutation caught: the ledger consulted first — which means
    /// consulting it for a stream that has no receive half at all.
    #[test]
    fn legality_is_checked_before_flow_control() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let id = Solo::our_uni(0);
        let d = s.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA + 1, &ramp(0, 4), false),
        );
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }

    /// limit **before** flow control.
    ///
    /// Mutation caught: flow control first, which answers a stream-count
    /// violation with a credit code and points the peer's operator at the
    /// wrong subsystem.
    #[test]
    fn the_cumulative_limit_is_checked_before_flow_control() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let id = Solo::peer_uni(INITIAL_MAX_STREAMS_UNI);
        let d = s.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA + 1, &ramp(0, 4), false),
        );
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_LIMIT_ERROR);
    }

    /// final size **before** flow control.
    ///
    /// Mutation caught: the credit bound evaluated first, so a frame
    /// contradicting a size we have already pinned is answered with
    /// `FLOW_CONTROL_ERROR`. Ruling 97: a frame contradicting a pinned
    /// final size is a statement about a stream we already fully
    /// understand, and a credit code would mislead.
    #[test]
    fn the_final_size_is_checked_before_flow_control() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), true));
        assert_alive(&d);

        // Beyond the pinned final size (100) *and* beyond the stream
        // window (262 144).
        let d = s.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA + 1, &ramp(0, 4), false),
        );
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// watermark **before** flow control — and the **uni** half of ruling
    /// 93's amendment.
    ///
    /// Mutation caught: **per-half tombstone only.** With the receive
    /// half freed but the watermark not advanced, this frame reaches the
    /// stream-level credit check against a frozen limit and kills the
    /// connection. §9.2 says a frame at or below the watermark and not
    /// open is "processed as acknowledged" — no error, no credit
    /// consumed, no re-open — so `assert_alive` is the separating
    /// assertion, and the offset is chosen far beyond the window so that
    /// nothing but the watermark can save it.
    #[test]
    fn a_frame_below_the_watermark_beyond_credit_is_a_no_op_not_a_violation() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 16), false));
        let r = s.conn.accept(Dir::Uni).expect("open");
        abandon_recv(&mut s.conn, t, r);
        let _ = drain(&mut s.conn);

        let d = s.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA * 4, &ramp(0, 64), false),
        );
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "§9.2: never re-opened"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Ruling 93 and its amendment — abandoning a receive half
// ═══════════════════════════════════════════════════════════════════════

mod abandonment {
    use super::*;

    /// Collect every MAX_DATA value on the wire from one drain.
    fn max_datas(s: &mut Solo, d: &Drained) -> Vec<u64> {
        s.drain_frames(d)
            .into_iter()
            .filter_map(|f| match f {
                Wire::MaxData(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    /// Two peer-opened uni streams, a handful of bytes each, both
    /// claimed — the setup both ruling 93 tests share.
    fn two_thin_uni_streams(t: Instant) -> (Solo, StreamRef, StreamRef) {
        let mut s = Solo::installed_at(t);
        // Naming index 1 opens 0 and 1 (§9.2).
        let _ = s.deliver(
            t,
            &stream_frame(Solo::peer_uni(1), 0, &ramp(0, 1000), false),
        );
        let _ = s.deliver(
            t,
            &stream_frame(Solo::peer_uni(0), 0, &ramp(0, 1000), false),
        );
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), 2);
        let (r0, r1) = (claimed[0].1, claimed[1].1);
        let _ = drain(&mut s.conn);
        (s, r0, r1)
    }

    /// Ruling 93's first named test.
    ///
    /// Mutation caught: **§16.2's mechanism as written** — abandonment
    /// merely *arms* a retirement that runs when a final size is pinned.
    /// A sender stalled at the stream window sends no FIN and has no
    /// reason to reset, so under that reading no retirement ever runs and
    /// four abandoned streams wedge the connection window for the
    /// connection's life. The separating assertion is that the credit
    /// moves in the **same drain as the abandonment**, with no further
    /// frame from the peer: under §16.2's reading no MAX_DATA is ever
    /// emitted at all.
    #[test]
    fn a_dropped_recv_stream_releases_connection_credit_at_once() {
        let t = t0();
        let (mut s, r0, r1) = two_thin_uni_streams(t);

        abandon_recv(&mut s.conn, t, r0);
        let d = drain(&mut s.conn);
        let first = max_datas(&mut s, &d);

        abandon_recv(&mut s.conn, t, r1);
        let d = drain(&mut s.conn);
        let second = max_datas(&mut s, &d);

        assert!(
            first.is_empty(),
            "one abandoned stream releases 256 KiB, half of §10.3's \
             512 KiB trigger: nothing is owed yet"
        );
        assert_eq!(
            second.len(),
            1,
            "the second abandonment crosses the trigger **in its own \
             drain** — no peer frame intervened, so nothing but the \
             abandonment can have caused it"
        );
    }

    /// Ruling 93's second named test — **the one that pins the value**.
    ///
    /// Mutation caught: the planner's provisional, which trues up to the
    /// **highest received offset**. Both implementations release "at
    /// once", so the test above does not separate them; this one does.
    /// Each stream received 1 000 bytes and advertised 262 144, so the
    /// provisional releases 2 000 connection-level bytes — nowhere near
    /// §10.3's 524 288 trigger — and emits **no** MAX_DATA at all, while
    /// the ruling releases exactly 2 × 262 144 = 524 288 and emits one
    /// grant whose value is `consumed + INITIAL_MAX_DATA`.
    ///
    /// Asserting the **exact `max`** rather than "a grant arrived" is
    /// what makes this a pin: a build truing up to some third value
    /// (say, the connection window) also emits one grant.
    #[test]
    fn a_dropped_recv_stream_trues_up_to_the_stream_window_not_the_high_water_mark() {
        let t = t0();
        let (mut s, r0, r1) = two_thin_uni_streams(t);

        abandon_recv(&mut s.conn, t, r0);
        let d = drain(&mut s.conn);
        assert!(max_datas(&mut s, &d).is_empty());

        abandon_recv(&mut s.conn, t, r1);
        let d = drain(&mut s.conn);
        let grants = max_datas(&mut s, &d);

        let consumed = 2 * INITIAL_MAX_STREAM_DATA;
        assert_eq!(
            consumed,
            INITIAL_MAX_DATA / 2,
            "the arithmetic this rests on"
        );
        assert_eq!(
            grants,
            vec![consumed + INITIAL_MAX_DATA],
            "§10.3: the prospective limit is `consumed + WINDOW`, and \
             consumption for an abandoned half is the stream window it \
             advertised — not the 1 000 bytes that happened to arrive"
        );
    }

    /// Ruling 93's amendment, **uni** row.
    ///
    /// Mutation caught: **per-half tombstone only** — the receive half is
    /// freed but the space's watermark never advances, so the peer never
    /// earns its MAX_STREAMS credit back and a long-lived connection
    /// starves on stream count while every byte flows. The separating
    /// assertion is the grant itself, at the batch boundary, two-sided:
    /// seven full closures owe nothing, the eighth owes exactly one
    /// MAX_STREAMS_UNI carrying `128 + 8`.
    #[test]
    fn abandoning_peer_opened_uni_halves_fully_closes_them_and_grants_max_streams() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let batch = STREAMS_CREDIT_BATCH;
        let _ = s.deliver(
            t,
            &stream_frame(Solo::peer_uni(batch - 1), 0, &ramp(0, 8), false),
        );
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), batch as usize, "§9.2 opened the whole run");
        let _ = drain(&mut s.conn);

        let grants = |s: &mut Solo, d: &Drained| -> Vec<u64> {
            s.drain_frames(d)
                .into_iter()
                .filter_map(|f| match f {
                    Wire::MaxStreamsUni(m) => Some(m),
                    _ => None,
                })
                .collect()
        };

        for (_, r) in claimed.iter().take(batch as usize - 1) {
            abandon_recv(&mut s.conn, t, *r);
            let d = drain(&mut s.conn);
            assert!(
                grants(&mut s, &d).is_empty(),
                "§10.4 batches: fewer than STREAMS_CREDIT_BATCH grants are \
                 unadvertised, so nothing goes out"
            );
        }

        abandon_recv(&mut s.conn, t, claimed[batch as usize - 1].1);
        let d = drain(&mut s.conn);
        assert_eq!(
            grants(&mut s, &d),
            vec![INITIAL_MAX_STREAMS_UNI + batch],
            "§10.4: cumulative, and +1 per fully-closed peer-opened stream"
        );
    }

    /// Ruling 93's amendment, **bidi** row — the mirror of the test
    /// above, and the reason the amendment exists.
    ///
    /// Mutation caught: treating every abandonment as a full closure. A
    /// bidi stream's **send half is still live**, so §9.7's "fully
    /// closed" is not satisfied: no watermark advance and **no**
    /// MAX_STREAMS grant. A build that grants here hands the peer
    /// allowance against state that is not free, and does it for streams
    /// slice 4 cannot ever fully close.
    #[test]
    fn abandoning_bidi_receive_halves_grants_no_max_streams() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let batch = STREAMS_CREDIT_BATCH;
        let _ = s.deliver(
            t,
            &stream_frame(Solo::peer_bidi(batch - 1), 0, &ramp(0, 8), false),
        );
        let claimed = accept_all(&mut s.conn, Dir::Bi);
        assert_eq!(claimed.len(), batch as usize);
        let _ = drain(&mut s.conn);

        let mut all = Vec::new();
        for (_, r) in &claimed {
            abandon_recv(&mut s.conn, t, *r);
            let d = drain(&mut s.conn);
            all.extend(s.drain_frames(&d));
        }

        assert!(
            !all.iter().any(|f| matches!(f, Wire::MaxStreamsBidi(_))),
            "§9.7: our send half is still live, so the stream is not fully \
             closed and the peer has earned nothing — got {all:?}"
        );
    }

    /// Ruling 93's amendment: an abandoned **bidi** receive half is
    /// neither a watermark no-op nor an implicit open.
    ///
    /// Mutation caught: **watermark only.** With the half freed, the
    /// index gone from the open set and no watermark to catch it, the
    /// next STREAM frame naming it **resurrects** the stream — a fresh
    /// `StreamOpened`, a fresh reassembler, and the cumulative limit
    /// re-charged against state we already freed. §16.2's own rule says
    /// arrivals for an abandoned half are **discarded**, so the
    /// separating assertion is **zero** new `StreamOpened` events while
    /// the connection stays alive.
    #[test]
    fn an_abandoned_bidi_receive_half_discards_arrivals_without_reopening() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_bidi(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 64), false));
        let r = s.conn.accept(Dir::Bi).expect("open");
        abandon_recv(&mut s.conn, t, r);
        let _ = drain(&mut s.conn);

        let d = s.deliver(t, &stream_frame(id, 64, &ramp(64, 64), false));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "§16.2: arrivals for an abandoned half are discarded, never \
             re-opened — a watermark-only build resurrects it here"
        );
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamReadable { .. })),
            0,
            "nothing was delivered, so nothing became readable"
        );
    }

    /// Ruling 93's amendment: the abandoned bidi half is not an unbounded
    /// sink.
    ///
    /// Mutation caught: "discard everything for an abandoned half" taken
    /// literally, so the stream-level credit check is skipped and a peer
    /// may name any offset it likes on a half we no longer bound. The
    /// amendment is explicit that the `FLOW_CONTROL_ERROR` check **still
    /// runs** against the frozen advertised limit. Paired with the test
    /// above, which asserts the in-credit frame is *silently* discarded:
    /// one test alone cannot tell "discards everything" from "discards
    /// what it should".
    #[test]
    fn an_abandoned_bidi_receive_half_still_enforces_its_frozen_stream_limit() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_bidi(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 64), false));
        let r = s.conn.accept(Dir::Bi).expect("open");
        abandon_recv(&mut s.conn, t, r);
        let _ = drain(&mut s.conn);

        let d = s.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA, &[0u8; 1], false),
        );
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FLOW_CONTROL_ERROR);
    }

    /// §10.6's memory bound, on the path that frees rather than reads.
    ///
    /// Mutation caught: a reassembler that frees the *stream* but leaks
    /// its buffer — invisible to every behavioural assertion, and the
    /// exact shape of the leak §10.6 exists to forbid.
    #[test]
    fn abandoning_a_receive_half_releases_its_reassembly_capacity() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        // A gap, so there is genuinely buffered state to release.
        // Chunked: a single 4 KiB STREAM frame exceeds MAX_PLAINTEXT (1170)
        // and §3.1's size gate drops the datagram *silently*, so the
        // precondition below would fail on delivery, not on behaviour.
        let _ = s.deliver_stream_bytes(t, id, 4096, 4096, false);
        assert!(
            s.conn.reassembly_capacity() > 0,
            "an out-of-order range must be buffered somewhere"
        );

        let r = s.conn.accept(Dir::Uni).expect("open");
        abandon_recv(&mut s.conn, t, r);
        let _ = drain(&mut s.conn);
        assert_eq!(
            s.conn.reassembly_capacity(),
            0,
            "§9.7 frees the half; §10.6 makes that mean the memory too"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9.2 / ruling 105 — the closed-stream tombstone
// ═══════════════════════════════════════════════════════════════════════

mod tombstone {
    use super::*;

    /// Ruling 105's obligation, discharged by an **injected duplicate**.
    ///
    /// Mutation caught: no watermark at all. §9.2 spells out what the
    /// broken build does — the retransmission re-opens the stream,
    /// restarts the reassembler, re-pins the final size and fires a
    /// phantom `StreamOpened` for a stream the application already
    /// finished. The separating assertions are **zero** new
    /// `StreamOpened` events and `accept()` returning `None`: a build
    /// that re-opens is otherwise indistinguishable, because the bytes it
    /// re-delivers go to a stream nobody is reading.
    ///
    /// §12's ACK and §13's PTO do not exist yet, so the stimulus is the
    /// duplicate itself — a *stronger* one, since it arrives with no
    /// delay. The loss-driven variant is **owed to slice 7**.
    #[test]
    fn a_duplicate_stream_frame_after_the_receive_half_is_freed_is_a_no_op() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);
        let frame = stream_frame(id, 0, &ramp(0, 512), true);

        let _ = s.deliver(t, &frame);
        let r = s.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got, ramp(0, 512));
        assert!(eof, "§9.7: read to the final size frees the receive half");

        // The peer never learned we read it — only an ACK would say so —
        // so it re-sends.
        let d = s.deliver(t, &frame);
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { .. })),
            0,
            "§9.2: processed as acknowledged, never re-opened"
        );
        assert_eq!(
            s.conn.accept(Dir::Uni),
            None,
            "no phantom stream is claimable"
        );
        assert_eq!(
            s.conn.reassembly_capacity(),
            0,
            "and no reassembler was restarted"
        );
    }

    /// §8.4: "credit for a fully-closed stream is a valid no-op",
    /// approached from the side slice 4 can reach.
    ///
    /// Mutation caught: a MAX_STREAM_DATA naming a tombstoned index
    /// treated as an error or as an implicit open — §8.4 is explicit that
    /// credit frames never open streams.
    ///
    /// **Partial by construction.** The rule's own subject is a stream
    /// *we can send on*, and no such stream can fully close in slice 4
    /// (no ACKs), so what is reachable here is the legality half: the
    /// peer granting us credit on a uni stream **it** opened, which we
    /// may never write to. Recorded in `TESTS-4a.md`.
    #[test]
    fn max_stream_data_for_a_stream_we_cannot_send_on_is_a_state_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 16), false));
        let d = s.deliver(t, &max_stream_data_frame(id, INITIAL_MAX_STREAM_DATA * 2));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }

    /// §8.4's QUIC rule: a credit frame for a stream in **our** space
    /// that we have not opened is a state error, because credit frames
    /// never open streams.
    ///
    /// Mutation caught: MAX_STREAM_DATA routed through the same implicit
    /// -opening path as STREAM and RESET_STREAM, which lets a peer mint
    /// entries in our own space.
    #[test]
    fn max_stream_data_for_an_unopened_stream_of_our_own_space_is_a_state_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Our bidi space, index 0 — legal for us to send on, but we have
        // not opened it.
        let ours = raw_id(0, Dir::Bi, false);
        let d = s.deliver(t, &max_stream_data_frame(ours, INITIAL_MAX_STREAM_DATA * 2));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9.5 / §10.6 — reassembly, final size, and the two memory bounds
// ═══════════════════════════════════════════════════════════════════════

mod reassembly {
    use super::*;

    /// §9.5: ranges arrive in any order.
    ///
    /// Mutation caught: a reassembler that appends on arrival rather than
    /// placing by offset. The payload is a function of its offset and the
    /// comparison is **by value**, so the shuffled build fails on content
    /// even though the length matches (§11.1).
    #[test]
    fn ranges_arriving_out_of_order_reassemble_into_the_sent_bytes() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        for &(off, len) in &[(600u64, 200usize), (0, 200), (400, 200), (200, 200)] {
            let last = off == 600;
            let _ = s.deliver(t, &stream_frame(id, off, &ramp(off as usize, len), last));
        }

        let r = s.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got.len(), 800, "every byte, once");
        assert_eq!(got, ramp(0, 800), "and in offset order");
        assert!(eof);
    }

    /// §9.5: ranges may **overlap**, and each byte is delivered exactly
    /// once.
    ///
    /// Mutation caught: *build A* drops the overlapping region's tail —
    /// the bytes are "already received" by offset but the bookkeeping is
    /// off by the overlap; *build B* re-delivers the duplicated bytes, so
    /// the reader sees more than were sent. Length and content are
    /// asserted **separately** so the two fail differently (§11.1).
    #[test]
    fn overlapping_ranges_deliver_each_byte_exactly_once() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        // Deliberately overlapping: [0,300), [200,600), [500,800), and a
        // full re-send of [0,300).
        for &(off, len, fin) in &[
            (0u64, 300usize, false),
            (200, 400, false),
            (500, 300, false),
            (0, 300, false),
            (0, 800, true),
        ] {
            let _ = s.deliver(t, &stream_frame(id, off, &ramp(off as usize, len), fin));
        }

        let r = s.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got.len(), 800, "overlap adds no bytes");
        assert_eq!(got, ramp(0, 800));
        assert!(eof);
    }

    /// The contract's `Ok(Some(0))` / `Ok(None)` distinction, which the
    /// shell turns into "park" versus "EOF".
    ///
    /// Mutation caught: the two swapped, or a build that reports EOF as
    /// soon as the contiguous prefix is drained. Getting it backwards
    /// hangs a reader on a finished stream forever — which is why
    /// `CONTRACT-4a.md` writes it down rather than leaving it inferred.
    #[test]
    fn a_hole_parks_the_reader_and_only_the_final_byte_ends_the_stream() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);
        let mut buf = [0u8; 256];

        // A gap at [100,200): the prefix [0,100) is readable, the rest is
        // not.
        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), false));
        let _ = s.deliver(t, &stream_frame(id, 200, &ramp(200, 100), true));
        let r = s.conn.accept(Dir::Uni).expect("open");

        assert_eq!(
            s.conn.read(t, r, &mut buf),
            Ok(Some(100)),
            "the contiguous prefix, and only it"
        );
        assert_eq!(
            s.conn.read(t, r, &mut buf),
            Ok(Some(0)),
            "a hole is a park, not an end of stream — even though the FIN \
             has already arrived"
        );

        let _ = s.deliver(t, &stream_frame(id, 100, &ramp(100, 100), false));
        assert_eq!(
            s.conn.read(t, r, &mut buf),
            Ok(Some(200)),
            "the gap and the tail"
        );
        assert_eq!(s.conn.read(t, r, &mut buf), Ok(None), "now the stream ends");
        assert_eq!(s.conn.read(t, r, &mut buf), Ok(None), "and stays ended");
    }

    /// §9.5: a FIN pins the final size at the frame's end offset.
    ///
    /// Mutation caught: the final size taken from the frame's `offset`
    /// rather than `offset + len`. A FIN-only frame at the high-water
    /// offset is indistinguishable under that bug, so the FIN here
    /// **carries data**.
    #[test]
    fn a_fin_pins_the_final_size_at_the_frames_end_offset() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 500), true));
        assert_alive(&d);

        // 500 is the final size, so [500, 504) is beyond it.
        let d = s.deliver(t, &stream_frame(id, 500, &ramp(500, 4), false));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// §11.9's two-sided row for the final size.
    ///
    /// Mutation caught: an off-by-one in "a FIN pinning a size **below**
    /// already-received data". A FIN at exactly the high-water offset is
    /// the legal case and a build using `<=` rejects it, which the fatal
    /// side alone cannot see.
    #[test]
    fn a_fin_at_the_high_water_offset_is_accepted_and_one_below_it_is_not() {
        let t = t0();

        let mut alive = Solo::installed_at(t);
        let id = Solo::peer_uni(0);
        let _ = alive.deliver(t, &stream_frame(id, 0, &ramp(0, 400), false));
        let d = alive.deliver(t, &stream_frame(id, 400, &[], true));
        assert_alive(&d);
        let r = alive.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut alive.conn, t, r);
        assert_eq!(got, ramp(0, 400));
        assert!(
            eof,
            "a FIN at exactly the high-water offset ends the stream"
        );

        let mut dead = Solo::installed_at(t);
        let _ = dead.deliver(t, &stream_frame(id, 0, &ramp(0, 400), false));
        let d = dead.deliver(t, &stream_frame(id, 399, &[], true));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// §9.5: "two pins that disagree".
    ///
    /// Mutation caught: a second FIN silently overwriting the first,
    /// which lets a peer shrink a stream after the fact.
    #[test]
    fn two_fins_pinning_different_sizes_are_a_final_size_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), true));
        // The identical FIN again is a legal retransmission.
        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 100), true));
        assert_alive(&d);

        let d = s.deliver(t, &stream_frame(id, 0, &ramp(0, 90), true));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// §11.9's two-sided row for `REASSEMBLY_CHUNKS_MAX`, and ruling
    /// 104's third violation.
    ///
    /// Mutation caught: any ceiling other than 1024. A one-sided "it
    /// eventually dies" passes a build that dies at **2**, and passes a
    /// build with no coalescing whose ranges happen to hit the ceiling
    /// early. 1024 stored ranges must be **alive**; the 1025th must be
    /// `PROTOCOL_VIOLATION` — the member §10.5 omits and §10.6 defines.
    ///
    /// Odd offsets, so no range ever sits at the read position and every
    /// stored range is unambiguously discontiguous.
    #[test]
    fn exactly_1024_stored_ranges_survive_and_the_1025th_is_a_protocol_violation() {
        let t = t0();
        let id = Solo::peer_uni(0);
        let max = REASSEMBLY_CHUNKS_MAX as u64;

        let one = |k: u64| stream_frame(id, 1 + 2 * k, &[(k % 251) as u8], false);

        let mut alive = Solo::installed_at(t);
        let frames: Vec<Vec<u8>> = (0..max).map(one).collect();
        let d = alive.deliver_packed(t, &frames);
        assert_alive(&d);
        assert_eq!(
            REASSEMBLY_CHUNKS_MAX, 1024,
            "§10.6's ratified value; a change here needs a ruling"
        );

        let d = alive.deliver(t, &one(max));
        let out = alive.drain_frames(&d);
        assert_violation(&d, &out, PROTOCOL_VIOLATION);
    }

    /// The assertion that separates "coalesces on insert" from "has a low
    /// ceiling" (ruling 94, §11.7).
    ///
    /// Mutation caught: *build A* stores every received range without
    /// coalescing — it dies here, well before 4 096 frames; *the worse
    /// build* coalesces by **overwriting gaps**, keeping one range and
    /// losing data, which survives the count and fails the content. So
    /// both the survival **and** the reassembled bytes are asserted.
    ///
    /// Reverse arrival order, so every frame merges with the range in
    /// front of it and the stored count never leaves 1.
    #[test]
    fn contiguous_ranges_coalesce_so_four_thousand_frames_survive() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);
        const N: usize = 4096;
        // A compile-time guard, not a runtime one: if `REASSEMBLY_CHUNKS_MAX`
        // is ever raised past `N` this test stops proving that coalescing
        // happens at all, and that must break the build rather than pass
        // quietly. (`assert!` on two constants is `clippy::
        // assertions_on_constants`, which is what a const block is for.)
        const { assert!(N > REASSEMBLY_CHUNKS_MAX, "or this proves nothing") };

        let payload = ramp(0, N);
        let frames: Vec<Vec<u8>> = (0..N)
            .rev()
            .map(|i| stream_frame(id, i as u64, &payload[i..=i], false))
            .collect();
        let d = s.deliver_packed(t, &frames);
        assert_alive(&d);

        let d = s.deliver(t, &stream_frame(id, N as u64, &[], true));
        assert_alive(&d);

        let r = s.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got, payload, "coalescing must not lose the gaps' contents");
        assert!(eof);
    }

    /// Ruling 94's named test.
    ///
    /// Mutation caught: **eager per-stream allocation.** Ruling 94 does
    /// the arithmetic: 128 peer-opened uni streams at `option (a)`'s
    /// per-stream span allocate **32 MiB** against 1 MiB of credit — a
    /// 32× remote memory amplification produced by following the section
    /// that exists to forbid it.
    ///
    /// **The assertion is on allocated capacity, not on bytes received**,
    /// and that is the whole point: an eager allocator receives 128 bytes
    /// and passes a bytes-received assertion for free. This is working
    /// rule 9's exact trap, in the test that exists to close a memory
    /// vector.
    #[test]
    fn buffered_bytes_stay_within_the_connection_window_across_many_streams() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let n = INITIAL_MAX_STREAMS_UNI;

        // One frame opens all 128 (§9.2); then one byte to each, at a
        // non-zero offset so nothing can be delivered away.
        // The same byte value at the same offset every time: §9.5 makes a
        // byte received twice with *differing* values undefined behaviour
        // of the sender, and a test must not rely on it.
        let mut frames = vec![stream_frame(Solo::peer_uni(n - 1), 4, &[1u8], false)];
        frames.extend((0..n).map(|i| stream_frame(Solo::peer_uni(i), 4, &[1u8], false)));
        let d = s.deliver_packed(t, &frames);
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            n as usize,
            "all 128 are open, so an eager allocator has allocated all 128"
        );

        let cap = s.conn.reassembly_capacity();
        assert!(
            cap <= INITIAL_MAX_DATA,
            "§10.6: credit is the buffer commitment, and the connection \
             window is 1 MiB — allocated {cap}"
        );
        assert!(
            cap < n * INITIAL_MAX_STREAM_DATA,
            "ruling 94: lazily, so nothing like the {} B an eager \
             per-stream allocator would hold — allocated {cap}",
            n * INITIAL_MAX_STREAM_DATA
        );
    }

    /// §10.6 again, on the ordinary path.
    ///
    /// Mutation caught: capacity that grows with the stream and is never
    /// returned. Asserted as a **round trip** — zero, then non-zero,
    /// then zero — because "it is bounded" is true of a build that
    /// allocates once and leaks it.
    #[test]
    fn reassembly_capacity_returns_to_zero_when_the_half_is_read_out() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        assert_eq!(s.conn.reassembly_capacity(), 0, "nothing yet");
        // Chunked — see `abandoning_a_receive_half_releases_its_reassembly_capacity`.
        let _ = s.deliver_stream_bytes(t, id, 2048, 2048, false);
        assert!(
            s.conn.reassembly_capacity() > 0,
            "an out-of-order range has to live somewhere"
        );

        let _ = s.deliver_stream_bytes(t, id, 0, 2048, false);
        let _ = s.deliver(t, &stream_frame(id, 4096, &[], true));
        let r = s.conn.accept(Dir::Uni).expect("open");
        let (got, eof) = read_available(&mut s.conn, t, r);
        assert_eq!(got, ramp(0, 4096));
        assert!(eof);
        assert_eq!(
            s.conn.reassembly_capacity(),
            0,
            "§9.7 frees at read-to-final, and §10.6 makes that mean the \
             memory too"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §10 — flow control
// ═══════════════════════════════════════════════════════════════════════

mod flow_control {
    use super::*;

    fn credit_frames(s: &mut Solo, d: &Drained) -> Vec<Wire> {
        s.drain_frames(d)
            .into_iter()
            .filter(|f| {
                matches!(
                    f,
                    Wire::MaxData(_)
                        | Wire::MaxStreamData { .. }
                        | Wire::MaxStreamsBidi(_)
                        | Wire::MaxStreamsUni(_)
                )
            })
            .collect()
    }

    /// Hunt H15's separating assertion (§11.5).
    ///
    /// Mutation caught: `last_advertised` initialised to **0** rather
    /// than to `INITIAL_MAX_STREAM_DATA`. Then `prospective − 0` is
    /// already a whole window on the first byte read, and every stream
    /// emits a MAX_STREAM_DATA on open. A test that only checks "a grant
    /// eventually arrives" passes that build with flying colours; the
    /// assertion is **zero** grants.
    #[test]
    fn one_byte_read_emits_no_credit_at_all() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &[7u8], false));
        let r = s.conn.accept(Dir::Uni).expect("open");
        assert_eq!(s.conn.read(t, r, &mut [0u8; 8]), Ok(Some(1)));

        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        assert_eq!(
            credit_frames(&mut s, &d),
            Vec::new(),
            "§10.3: the seed is the constant, so one byte is nowhere near \
             WINDOW/2 and nothing is owed"
        );
    }

    /// §10.3's trigger and its **value**, two-sided.
    ///
    /// Mutation caught: *grants unconditionally* — fails the first half;
    /// *never grants* — fails the second; *grants additively*
    /// (`limit += WINDOW/2`) — passes both counts and fails the value.
    /// §10.3 is explicit that the limit is absolute: `bytes_read +
    /// WINDOW`.
    #[test]
    fn a_max_stream_data_arrives_at_exactly_half_a_window_read_and_not_before() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);
        let half = INITIAL_MAX_STREAM_DATA / 2;

        let _ = s.deliver_stream_bytes(t, id, 0, half as usize + 16, false);
        let r = s.conn.accept(Dir::Uni).expect("open");

        let _ = read_exactly(&mut s.conn, t, r, half as usize - 1);
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        assert_eq!(
            credit_frames(&mut s, &d),
            Vec::new(),
            "one byte short of WINDOW/2 owes nothing"
        );

        let _ = read_exactly(&mut s.conn, t, r, 1);
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        assert_eq!(
            credit_frames(&mut s, &d),
            vec![Wire::MaxStreamData {
                id,
                max: half + INITIAL_MAX_STREAM_DATA
            }],
            "§10.3: absolute — `bytes_read + WINDOW`, exactly once"
        );
    }

    /// §10.3's connection-level twin, on the same formula.
    ///
    /// Mutation caught: **one shared ledger** (§11.4's build C). A core
    /// collapsing the two levels emits one grant where two are owed, or
    /// emits the connection grant at the stream trigger. Here the reader
    /// crosses the *stream* trigger four times over before it crosses the
    /// *connection* one, so the two cannot coincide.
    #[test]
    fn max_data_arrives_at_half_the_connection_window_consumed() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let half_conn = INITIAL_MAX_DATA / 2;

        // Four peer-opened uni streams, each carrying an eighth of the
        // connection window — well inside every stream window.
        let per = (half_conn / 4) as usize;
        let ids: Vec<u64> = (0..4).map(Solo::peer_uni).collect();
        let _ = s.deliver(t, &stream_frame(ids[3], 0, &[], false));
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), 4);
        for id in &ids {
            let _ = s.deliver_stream_bytes(t, *id, 0, per, false);
        }
        let _ = drain(&mut s.conn);

        // Drain three of the four: 3/8 of the connection window.
        for (_, r) in claimed.iter().take(3) {
            let _ = read_exactly(&mut s.conn, t, *r, per);
        }
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        let seen = credit_frames(&mut s, &d);
        assert!(
            !seen.iter().any(|f| matches!(f, Wire::MaxData(_))),
            "3/8 of the connection window is below the 1/2 trigger, though \
             every one of those streams crossed its own — {seen:?}"
        );

        let _ = read_exactly(&mut s.conn, t, claimed[3].1, per);
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        let seen = credit_frames(&mut s, &d);
        assert!(
            seen.contains(&Wire::MaxData(half_conn + INITIAL_MAX_DATA)),
            "§10.3 at the connection level, absolute — {seen:?}"
        );
    }

    /// §11.9's two-sided row for stream credit.
    ///
    /// Mutation caught: `>` where `>=` belongs, or the reverse. The limit
    /// is an **absolute offset**: `offset + len == limit` is the last
    /// legal byte, `+ 1` is `FLOW_CONTROL_ERROR`, and §10.5 says there is
    /// no tolerance band.
    #[test]
    fn stream_data_ending_exactly_at_the_window_is_legal_and_one_byte_past_kills() {
        let t = t0();
        let id = Solo::peer_uni(0);

        let mut alive = Solo::installed_at(t);
        let d = alive.deliver(
            t,
            &stream_frame(id, INITIAL_MAX_STREAM_DATA - 1, &[9u8], false),
        );
        assert_alive(&d);

        let mut dead = Solo::installed_at(t);
        let d = dead.deliver(t, &stream_frame(id, INITIAL_MAX_STREAM_DATA, &[9u8], false));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, FLOW_CONTROL_ERROR);
    }

    /// §11.9's two-sided row for connection credit, and §10.1's "sum over
    /// all streams".
    ///
    /// Mutation caught: **only the stream level enforced.** Every frame
    /// here is comfortably inside its own stream window; only their sum
    /// crosses. Four streams whose high-water offsets total exactly
    /// 1 MiB are legal, and one byte on a fifth is not.
    #[test]
    fn the_sum_of_stream_offsets_is_bounded_by_the_connection_window() {
        let t = t0();
        let per = INITIAL_MAX_STREAM_DATA;
        let n = INITIAL_MAX_DATA / per; // 4

        let fill = |s: &mut Solo| {
            let _ = s.deliver(t, &stream_frame(Solo::peer_uni(n), 0, &[], false));
            for i in 0..n {
                let _ = s.deliver(t, &stream_frame(Solo::peer_uni(i), per - 1, &[3u8], false));
            }
        };

        let mut alive = Solo::installed_at(t);
        fill(&mut alive);
        let d = drain(&mut alive.conn);
        assert_alive(&d);

        let mut dead = Solo::installed_at(t);
        fill(&mut dead);
        let d = dead.deliver(t, &stream_frame(Solo::peer_uni(n), 0, &[3u8], false));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, FLOW_CONTROL_ERROR);
    }

    /// §11.4's two-level separation, from the **sending** side.
    ///
    /// Mutation caught: one collapsed ledger. Four streams take the whole
    /// connection window between them; a **fifth, fresh** stream has an
    /// untouched 256 KiB stream window of its own and must still block
    /// immediately, because the connection limit binds. A single-stream
    /// backpressure test cannot tell the two levels apart, and the story
    /// asks for both.
    #[test]
    fn a_fresh_stream_blocks_at_once_when_the_connection_window_is_spent() {
        let t = t0();
        let mut p = Pair::installed_at(t);
        let n = (INITIAL_MAX_DATA / INITIAL_MAX_STREAM_DATA) as usize;

        for _ in 0..n {
            let r = p.a.open(Dir::Uni).expect("open");
            assert_eq!(
                write_until_blocked(&mut p.a, t, r),
                INITIAL_MAX_STREAM_DATA,
                "each stream takes exactly its own window"
            );
        }

        let fresh = p.a.open(Dir::Uni).expect("open");
        assert_eq!(
            write_until_blocked(&mut p.a, t, fresh),
            0,
            "§10.1: whichever limit is tighter binds, and the connection \
             window is spent — a fresh stream window buys nothing"
        );
    }

    /// §10.4 from our own side, two-sided.
    ///
    /// Mutation caught: an off-by-one in "opening index `i` requires
    /// cumulative limit > `i`" on the **local** allocator. 32 bidi opens
    /// succeed; the 33rd is `StreamsExhausted`, which the shell converts
    /// into a park and no public verb ever returns (ruling 101).
    #[test]
    fn open_succeeds_thirty_two_times_and_the_thirty_third_is_exhausted() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        for i in 0..INITIAL_MAX_STREAMS_BIDI {
            let r =
                p.a.open(Dir::Bi)
                    .unwrap_or_else(|_| panic!("index {i} is inside the limit"));
            assert_eq!(p.a.stream_id(r).expect("established").index(), i);
        }
        assert!(
            p.a.open(Dir::Bi).is_err(),
            "§10.4: the limit counts streams ever opened"
        );
        assert!(
            p.a.open(Dir::Uni).is_ok(),
            "the uni space has its own, untouched allowance"
        );
    }

    /// §10.4's `StreamsAvailable`, and that the credit is **cumulative**.
    ///
    /// Mutation caught: MAX_STREAMS applied as "N more streams" rather
    /// than as a cumulative count. Granting 129 to a peer that has opened
    /// 128 buys exactly **one** more; an additive build buys 129 and the
    /// second `open()` here would wrongly succeed.
    #[test]
    fn max_streams_is_cumulative_and_wakes_a_blocked_opener() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        for _ in 0..INITIAL_MAX_STREAMS_UNI {
            s.conn.open(Dir::Uni).expect("inside the initial allowance");
        }
        assert!(s.conn.open(Dir::Uni).is_err(), "exhausted");

        let d = s.deliver(t, &max_streams_uni_frame(INITIAL_MAX_STREAMS_UNI + 1));
        assert_alive(&d);
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamsAvailable { dir: Dir::Uni })),
            1,
            "§10.4: receipt surfaces StreamsAvailable to wake blocked openers"
        );

        assert!(s.conn.open(Dir::Uni).is_ok(), "the one stream 129 buys");
        assert!(
            s.conn.open(Dir::Uni).is_err(),
            "and only one: the count is cumulative, not incremental"
        );
    }

    /// §8.4: monotone-max on receipt, so duplicates and reordering are
    /// idempotent.
    ///
    /// Mutation caught: last-write-wins. A reordered pair of grants then
    /// *lowers* a limit, and a sender that had already written to the
    /// higher one is retroactively in violation. The separating
    /// assertion is that the writer keeps the higher limit after the
    /// lower grant arrives.
    #[test]
    fn a_lower_credit_grant_is_a_no_op_not_a_reduction() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Uni).expect("open");
        let id = s.conn.stream_id(r).expect("established").as_u64();

        let raised = INITIAL_MAX_STREAM_DATA * 2;
        let _ = s.deliver(t, &max_stream_data_frame(id, raised));
        let _ = s.deliver(t, &max_stream_data_frame(id, INITIAL_MAX_STREAM_DATA / 2));
        let d = s.deliver(t, &max_data_frame(1));
        assert_alive(&d);

        assert_eq!(
            write_until_blocked(&mut s.conn, t, r),
            raised,
            "§8.4: monotone-max — the stale, lower grants changed nothing"
        );
    }

    /// §8.4: MAX_STREAM_DATA raises the send limit and the blocked writer
    /// is told.
    ///
    /// Mutation caught: the limit raised without a `StreamWritable`, so
    /// the shell's parked writer sleeps through its own wakeup. Both the
    /// event **and** the resumed acceptance are asserted, because a build
    /// that emits the event without moving the ledger passes the first
    /// alone.
    #[test]
    fn max_stream_data_raises_the_limit_and_wakes_the_blocked_writer() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Uni).expect("open");
        let id = s.conn.stream_id(r).expect("established").as_u64();

        assert_eq!(
            write_until_blocked(&mut s.conn, t, r),
            INITIAL_MAX_STREAM_DATA
        );
        let _ = drain(&mut s.conn);

        let d = s.deliver(
            t,
            &max_stream_data_frame(id, INITIAL_MAX_STREAM_DATA + 4096),
        );
        assert_eq!(
            d.count_events(|e| matches!(e, ConnEvent::StreamWritable { r: got } if *got == r)),
            1,
            "§16.4: credit arrived for a blocked writer"
        );
        assert_eq!(
            write_until_blocked(&mut s.conn, t, r),
            4096,
            "and the ledger actually moved, by exactly the grant"
        );
    }

    /// §11.9's two-sided row for MAX_STREAMS' structural ceiling.
    ///
    /// Mutation caught: the bound written as `>=` — which rejects 2⁶⁰,
    /// the largest representable stream index, and is invisible from the
    /// fatal side alone.
    #[test]
    fn a_max_streams_of_two_to_the_sixty_is_legal_and_one_more_is_structural() {
        let t = t0();
        let ceiling = 1u64 << 60;

        let mut alive = Solo::installed_at(t);
        let d = alive.deliver(t, &max_streams_bidi_frame(ceiling));
        assert_alive(&d);

        let mut dead = Solo::installed_at(t);
        let d = dead.deliver(t, &max_streams_bidi_frame(ceiling + 1));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, PROTOCOL_VIOLATION);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §9.6 — RESET_STREAM
// ═══════════════════════════════════════════════════════════════════════

mod reset {
    use super::*;

    /// §15.3 reserves `>= 0x10` for applications; 0 is what §16.2 makes a
    /// dropped `SendStream` send, so a test using 0 cannot tell the
    /// peer's code from the drop default (§11.3).
    const APP_CODE: u64 = 0x2a;

    /// §9.6 and §18.1, with the **exact** code.
    ///
    /// Mutation caught: a reset that tears the connection down, or one
    /// that surfaces `ConnectionLost` instead of `ReadError::Reset`, or
    /// one that reports some canonical code rather than the peer's.
    #[test]
    fn a_reset_stream_surfaces_read_error_reset_with_the_peers_code() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 64), false));
        let r = s.conn.accept(Dir::Uni).expect("open");

        let d = s.deliver(t, &reset_frame(id, APP_CODE, 64));
        assert_alive(&d);
        assert_eq!(
            d.count_events(
                |e| matches!(e, ConnEvent::StreamReset { r: got, error_code }
                                        if *got == r && *error_code == APP_CODE)
            ),
            1,
            "§16.4: the reset is signalled, with the peer's code"
        );
        assert_eq!(
            s.conn.read(t, r, &mut [0u8; 64]),
            Err(ReadError::Reset(APP_CODE)),
            "§9.6: the receive half surfaces the reset, not the buffered bytes"
        );
    }

    /// §9.6: "discards its reassembly buffer".
    ///
    /// Mutation caught: a reset that surfaces correctly and leaks the
    /// buffer — behaviourally invisible, and precisely §10.6's concern.
    #[test]
    fn a_reset_stream_discards_the_reassembly_buffer() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        // Chunked — see `abandoning_a_receive_half_releases_its_reassembly_capacity`.
        let _ = s.deliver_stream_bytes(t, id, 8192, 8192, false);
        assert!(s.conn.reassembly_capacity() > 0);

        let _ = s.deliver(t, &reset_frame(id, APP_CODE, 16384));
        assert_eq!(
            s.conn.reassembly_capacity(),
            0,
            "§9.6: the buffer goes with the stream's data"
        );
    }

    /// §8.4's ordering mandate: the credit bound is checked **before**
    /// the §9.6/§10.3 true-up, two-sided.
    ///
    /// Mutation caught: the true-up applied first, so a `final_size`
    /// past the advertised limit is folded into the ledger and only then
    /// rejected — which, with the wrong arithmetic, silently re-opens the
    /// window. The legal side (`final_size` exactly at the limit) is
    /// asserted too, because a build that rejects everything above zero
    /// passes the fatal side alone.
    #[test]
    fn a_reset_final_size_at_the_window_is_legal_and_one_past_is_a_flow_control_error() {
        let t = t0();
        let id = Solo::peer_uni(0);

        let mut alive = Solo::installed_at(t);
        let _ = alive.deliver(t, &stream_frame(id, 0, &[1u8], false));
        let d = alive.deliver(t, &reset_frame(id, 7, INITIAL_MAX_STREAM_DATA));
        assert_alive(&d);

        let mut dead = Solo::installed_at(t);
        let _ = dead.deliver(t, &stream_frame(id, 0, &[1u8], false));
        let d = dead.deliver(t, &reset_frame(id, APP_CODE, INITIAL_MAX_STREAM_DATA + 1));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, FLOW_CONTROL_ERROR);
    }

    /// §11.8's `u64` mandate.
    ///
    /// Mutation caught: unchecked arithmetic in the credit comparison. A
    /// build that computes `remaining = limit − consumed` after folding
    /// in a ceiling-sized `final_size` underflows and sees an enormous
    /// window; a build that folds first and compares later wraps.
    ///
    /// The error code alone is not enough — §11.8 warns that a build
    /// which wraps and then errors for an unrelated reason passes it — so
    /// the second assertion is that **no credit grant escapes**. A
    /// wrapped ledger looks like a huge jump in consumption and emits a
    /// MAX_DATA on the way out.
    #[test]
    fn a_reset_final_size_at_the_varint_ceiling_is_rejected_without_wrapping() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        let _ = s.deliver(t, &stream_frame(id, 0, &ramp(0, 128), false));
        let _ = drain(&mut s.conn);

        let d = s.deliver(t, &reset_frame(id, APP_CODE, VarInt::MAX_VALUE));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FLOW_CONTROL_ERROR);
        assert!(
            !frames
                .iter()
                .any(|f| matches!(f, Wire::MaxData(_) | Wire::MaxStreamData { .. })),
            "a wrapped or saturating-then-advanced ledger emits a grant \
             here — {frames:?}"
        );
    }

    /// §9.6: a RESET_STREAM for an already-FIN-complete half, two-sided.
    ///
    /// Mutation caught: any reset accepted after a FIN (which lets a peer
    /// restate a stream's length), or every reset after a FIN rejected
    /// (which breaks the legal retransmission §8.7 regenerates until
    /// acknowledged).
    #[test]
    fn a_reset_agreeing_with_a_pinned_final_size_is_a_no_op_and_disagreeing_kills() {
        let t = t0();
        let id = Solo::peer_uni(0);

        let mut alive = Solo::installed_at(t);
        let _ = alive.deliver(t, &stream_frame(id, 0, &ramp(0, 200), true));
        let d = alive.deliver(t, &reset_frame(id, APP_CODE, 200));
        assert_alive(&d);

        let mut dead = Solo::installed_at(t);
        let _ = dead.deliver(t, &stream_frame(id, 0, &ramp(0, 200), true));
        let d = dead.deliver(t, &reset_frame(id, APP_CODE, 201));
        let frames = dead.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// §8.4: a `final_size` below the highest received offset.
    ///
    /// Mutation caught: the comparison made against the *contiguous*
    /// prefix rather than the highest received offset, which a peer can
    /// exploit by leaving a hole.
    #[test]
    fn a_reset_final_size_below_the_highest_received_offset_is_a_final_size_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let id = Solo::peer_uni(0);

        // A hole at [0,400): the contiguous prefix is empty, the
        // high-water mark is 500.
        let _ = s.deliver(t, &stream_frame(id, 400, &ramp(400, 100), false));
        let d = s.deliver(t, &reset_frame(id, APP_CODE, 499));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, FINAL_SIZE_ERROR);
    }

    /// §8.4's legality clause for RESET_STREAM, mirroring ruling 97's for
    /// STREAM.
    ///
    /// Mutation caught: the legality check wired into the STREAM arm only
    /// — a plausible omission, since §9.2's implicit opening names both
    /// frames and §8.4 states the rule twice.
    #[test]
    fn a_reset_stream_on_a_space_the_peer_cannot_send_on_is_a_state_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &reset_frame(Solo::our_uni(0), APP_CODE, 0));
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, STREAM_STATE_ERROR);
    }

    /// §9.6 from the sending side, two-sided on the "or 0 if none" case.
    ///
    /// Mutation caught: `final_size` reported as the *buffered* or the
    /// *acknowledged* count. The stream is drained onto the wire before
    /// the reset, so "the end offset of the highest byte sent" is
    /// unambiguous here; the empty stream pins the other side, and a
    /// build defaulting to "unknown" or omitting the frame fails it.
    #[test]
    fn our_reset_carries_the_highest_byte_sent_as_its_final_size() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let written = s.conn.open(Dir::Uni).expect("open");
        let written_id = s.conn.stream_id(written).expect("established").as_u64();
        let payload = ramp(0, 5000);
        assert_eq!(write_all(&mut s.conn, t, written, &payload), 0);
        let _ = drain(&mut s.conn);

        s.conn.reset(t, written, APP_CODE);
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);
        assert!(
            frames.contains(&Wire::Reset {
                id: written_id,
                code: APP_CODE,
                final_size: 5000
            }),
            "§9.6: the end offset of the highest byte sent — {frames:?}"
        );

        let empty = s.conn.open(Dir::Uni).expect("open");
        let empty_id = s.conn.stream_id(empty).expect("established").as_u64();
        s.conn.reset(t, empty, APP_CODE);
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);
        assert!(
            frames.contains(&Wire::Reset {
                id: empty_id,
                code: APP_CODE,
                final_size: 0
            }),
            "§9.6: 0 if none — {frames:?}"
        );
    }

    /// §9.3: `reset()` moves the send half to `ResetSent`, from which
    /// there is no path back to `Send`.
    ///
    /// Mutation caught: `write` after `reset` accepted and buffered
    /// forever — bytes the peer will never see, and a `final_size`
    /// already asserted that they do not exist.
    #[test]
    fn write_after_reset_is_a_write_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let r = s.conn.open(Dir::Uni).expect("open");

        assert_eq!(write_all(&mut s.conn, t, r, &ramp(0, 100)), 0);
        s.conn.reset(t, r, APP_CODE);
        assert_eq!(
            s.conn.write(t, r, &[1u8; 4]),
            Err(WriteError::Finished),
            "§9.3: `ResetSent` has no incoming write edge"
        );
    }

    /// §10.3's retirement true-up, driven by an observed reset, with the
    /// **exact** grant value.
    ///
    /// Mutation caught: a true-up that is **additive** rather than
    /// absolute — `consumed += final_size` on top of bytes already
    /// counted by reads. §10.3 is explicit: a monotone bring-to-final,
    /// idempotent with what reads already counted. The 128 bytes read
    /// from each stream before the reset are the part an additive build
    /// double-counts, and they move the grant off the asserted value.
    #[test]
    fn observing_a_reset_brings_the_streams_contribution_to_its_final_size() {
        let t = t0();
        let mut s = Solo::installed_at(t);
        let per = INITIAL_MAX_DATA / 4; // 262 144 — two of them make the trigger

        let ids = [Solo::peer_uni(0), Solo::peer_uni(1)];
        let _ = s.deliver(t, &stream_frame(ids[1], 0, &[], false));
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), 2);

        for (i, id) in ids.iter().enumerate() {
            let _ = s.deliver(t, &stream_frame(*id, 0, &ramp(0, 128), false));
            let _ = read_exactly(&mut s.conn, t, claimed[i].1, 128);
        }
        let _ = drain(&mut s.conn);

        let _ = s.deliver(t, &reset_frame(ids[0], APP_CODE, per));
        assert_eq!(
            s.conn.read(t, claimed[0].1, &mut [0u8; 8]),
            Err(ReadError::Reset(APP_CODE))
        );
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        let f = s.drain_frames(&d);
        assert!(
            !f.iter().any(|w| matches!(w, Wire::MaxData(_))),
            "one retired stream is half the trigger — {f:?}"
        );

        let _ = s.deliver(t, &reset_frame(ids[1], APP_CODE, per));
        assert_eq!(
            s.conn.read(t, claimed[1].1, &mut [0u8; 8]),
            Err(ReadError::Reset(APP_CODE))
        );
        tick(&mut s.conn, t);
        let d = drain(&mut s.conn);
        let f = s.drain_frames(&d);
        assert!(
            f.contains(&Wire::MaxData(2 * per + INITIAL_MAX_DATA)),
            "§10.3: absolute — `2 × final_size`, with the 256 bytes already \
             read folded in, not added on top — {f:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7.4 / ruling 98 — which seal each frame rides
// ═══════════════════════════════════════════════════════════════════════

mod sealing {
    use super::*;

    fn last_send(s: &Solo) -> Instant {
        s.conn.liveness().expect("established").last_send()
    }

    /// Drive the core to a state whose **only** pending output is a
    /// MAX_STREAMS_UNI: eight peer-opened uni streams read to their final
    /// size, which is `STREAMS_CREDIT_BATCH` full closures at a total
    /// consumption of 128 bytes — far below every credit trigger, so no
    /// MAX_DATA or MAX_STREAM_DATA rides along.
    fn eight_uni_closures(t: Instant) -> Solo {
        let mut s = Solo::installed_at(t);
        let batch = STREAMS_CREDIT_BATCH;
        for i in 0..batch {
            let _ = s.deliver(t, &stream_frame(Solo::peer_uni(i), 0, &ramp(0, 16), true));
        }
        let claimed = accept_all(&mut s.conn, Dir::Uni);
        assert_eq!(claimed.len(), batch as usize);
        for (_, r) in &claimed {
            let (got, eof) = read_available(&mut s.conn, t, *r);
            assert_eq!(got.len(), 16);
            assert!(eof, "§9.7: read to the final size frees the half");
        }
        s
    }

    /// Ruling 98's named test, with **both halves** — ruling 33's rule.
    ///
    /// Mutation caught: MAX_STREAMS sealed with the marking `seal`, which
    /// makes a credit frame defer a keepalive for ever and, on a quiet
    /// connection that only ever grants credit, keeps the peer's liveness
    /// picture permanently stale. A build that makes credit neither
    /// marking **nor** ack-eliciting also passes "`last_send` unchanged",
    /// so the death clock's arming is asserted alongside it.
    #[test]
    fn a_max_streams_only_packet_does_not_defer_the_keepalive() {
        let t = t0();
        let t1 = t + Duration::from_secs(1);
        let mut s = eight_uni_closures(t);
        let before = last_send(&s);

        // Give the core an instant. `read()` carries none, so this is the
        // first `now` since the closures — reported in `TESTS-4a.md`.
        s.conn.handle_timeout(t1);
        let d = drain(&mut s.conn);
        let packets = s.packets(&d);

        let all: Vec<Wire> = packets.iter().flatten().cloned().collect();
        assert_eq!(
            all.iter()
                .filter(|f| !matches!(f, Wire::Padding))
                .cloned()
                .collect::<Vec<_>>(),
            vec![Wire::MaxStreamsUni(
                INITIAL_MAX_STREAMS_UNI + STREAMS_CREDIT_BATCH
            )],
            "the fixture must produce a credit-only packet or this test \
             asserts nothing — {all:?}"
        );

        assert_eq!(
            last_send(&s),
            before,
            "§7.4: the credit frames are the quiet set — `seal_quiet` \
             leaves `last_send` untouched"
        );
        assert!(
            s.conn.liveness().expect("established").is_armed(),
            "§7.4: and, being ack-eliciting, it arms the death clock"
        );
        assert!(
            d.deadline.is_some(),
            "§16.4: the drain ends on that armed deadline, not `Timeout(None)`"
        );
    }

    /// The other half of ruling 98's table, without which the test above
    /// passes a build that seals **everything** quietly.
    ///
    /// Mutation caught: a core with one seal path. `last_send` must move
    /// for a packet carrying a first-transmission STREAM frame, and the
    /// two tests are only a pin together.
    #[test]
    fn a_first_transmission_stream_frame_marks_last_send() {
        let t = t0();
        let t1 = t + Duration::from_secs(1);
        let mut s = Solo::installed_at(t);
        let before = last_send(&s);

        let r = s.conn.open(Dir::Uni).expect("open");
        assert_eq!(write_all(&mut s.conn, t1, r, &ramp(0, 4096)), 0);
        let d = drain(&mut s.conn);
        assert!(
            !d.transmits().is_empty(),
            "the write must have reached the wire"
        );

        assert_eq!(
            last_send(&s),
            t1,
            "§7.4: a fresh application send is marking — `seal`"
        );
        assert_ne!(before, t1, "the fixture must actually move the clock");
    }

    /// Ruling 98's correction of the plan's own table, which put
    /// RESET_STREAM on the marking path "by omission from §10.3".
    ///
    /// Mutation caught: exactly that. §7.4 names RESET_STREAM in the
    /// quiet set at `SPEC.md:1953–1958`; a marking RESET_STREAM would
    /// defer keepalives on a connection whose only traffic is stream
    /// cancellation.
    #[test]
    fn a_reset_stream_only_packet_does_not_mark_last_send() {
        let t = t0();
        let t1 = t + Duration::from_secs(1);
        let mut s = Solo::installed_at(t);

        let r = s.conn.open(Dir::Uni).expect("open");
        let _ = drain(&mut s.conn);
        let before = last_send(&s);

        s.conn.reset(t1, r, 0x2a);
        let d = drain(&mut s.conn);
        let frames = s.drain_frames(&d);
        assert!(
            frames.iter().any(|f| matches!(f, Wire::Reset { .. })),
            "the reset must have reached the wire — {frames:?}"
        );
        assert!(
            !frames.iter().any(|f| matches!(f, Wire::Stream { .. })),
            "nothing was written, so no STREAM frame can mark this packet"
        );
        assert_eq!(
            last_send(&s),
            before,
            "ruling 98: RESET_STREAM is in §7.4's quiet set"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8.5 — packing order and the extends-to-end rule
// ═══════════════════════════════════════════════════════════════════════

mod packing {
    use super::*;

    /// §8.5: at most one extends-to-end frame per packet, in final
    /// position.
    ///
    /// Mutation caught: a fill that emits a ¬LEN STREAM frame and then
    /// packs something after it — the receiver would read the following
    /// frames as stream data, silently. Both halves are asserted: the
    /// count **and** the position.
    #[test]
    fn at_most_one_extends_to_end_frame_per_packet_and_it_is_last() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let a = s.conn.open(Dir::Uni).expect("open");
        let b = s.conn.open(Dir::Uni).expect("open");
        assert_eq!(write_all(&mut s.conn, t, a, &ramp(0, 32 * 1024)), 0);
        assert_eq!(write_all(&mut s.conn, t, b, &ramp(0, 32 * 1024)), 0);

        let d = drain(&mut s.conn);
        let packets = s.packets(&d);
        assert!(packets.len() > 8, "64 KiB must span many packets");

        for pkt in &packets {
            let open_ended: Vec<usize> = pkt
                .iter()
                .enumerate()
                .filter(|(_, f)| matches!(f, Wire::Stream { had_len: false, .. }))
                .map(|(i, _)| i)
                .collect();
            assert!(
                open_ended.len() <= 1,
                "§8.5: at most one extends-to-end frame — {pkt:?}"
            );
            if let Some(&i) = open_ended.first() {
                assert_eq!(
                    i,
                    pkt.len() - 1,
                    "§8.5: and it is the packet's final frame — {pkt:?}"
                );
            }
        }
    }

    /// §8.5's round-robin fill, measured as **interleaving**.
    ///
    /// Mutation caught: a fill loop that drains stream A completely
    /// before touching stream B. Working rule 9's own warning applies
    /// here: "both streams made progress" is true of a strictly
    /// sequential fill when measured at the end, so the assertion is that
    /// each stream's frames appear **before the other stream's last
    /// frame** — a property a sequential fill cannot have.
    ///
    /// Deliberately **not** asserted: that some packet carries frames for
    /// both streams. §8.5 makes the quantum implementation-defined
    /// (`PLAN.md` §6.4), and a quantum of one packet is legal and would
    /// fail that — an assertion a conforming build can fail is a flake,
    /// not a pin.
    ///
    /// **The contention is built before the install, and that is the whole
    /// fixture** (ruling 114). §16.7 makes sealing synchronous inside the
    /// mutating call, and slice 4 has no congestion bound, so a `write()`
    /// on an installed core flushes everything that stream can send before
    /// the next `write()` is even called — two sequential writes can never
    /// contend, in *any* conforming build. §16.9's pre-install writes are
    /// the one place in slice 4 where two streams are pending at one fill;
    /// from slice 7 the congestion window makes it the ordinary case.
    #[test]
    fn the_stream_fill_serves_pending_streams_round_robin() {
        let t = t0();
        let (mut conn, sa, sb) = Solo::connecting();

        // Both writes land while there is no session to seal them with.
        let a = conn.open(Dir::Uni).expect("open");
        let b = conn.open(Dir::Uni).expect("open");
        assert!(
            conn.stream_id(a).is_none(),
            "§16.9: no wire id before the install — if this is Some, the              fixture is not testing what it claims to"
        );
        assert_eq!(write_all(&mut conn, t, a, &ramp(0, 32 * 1024)), 0);
        assert_eq!(write_all(&mut conn, t, b, &ramp(0, 32 * 1024)), 0);
        assert!(
            drain(&mut conn).transmits().is_empty(),
            "nothing can be on the wire before the install"
        );

        conn.handle_endpoint_event(
            t,
            Install {
                session: sb,
                role: Role::Responder,
            },
        );
        let d = drain(&mut conn);
        let mut s = Solo::around(conn, sa);
        let a_id = s.conn.stream_id(a).expect("established").as_u64();
        let b_id = s.conn.stream_id(b).expect("established").as_u64();
        let order: Vec<u64> = s
            .drain_frames(&d)
            .iter()
            .filter_map(|f| match f {
                Wire::Stream { id, .. } => Some(*id),
                _ => None,
            })
            .collect();

        let first = |want: u64| order.iter().position(|id| *id == want);
        let last = |want: u64| order.iter().rposition(|id| *id == want);
        assert!(first(a_id).is_some() && first(b_id).is_some(), "{order:?}");
        assert!(
            first(b_id) < last(a_id),
            "§8.5: B started before A finished — a sequential fill cannot \
             do this: {order:?}"
        );
        assert!(first(a_id) < last(b_id), "and symmetrically: {order:?}");
    }

    /// §8.5: control frames precede the STREAM fill within a packet.
    ///
    /// Mutation caught: a packer that appends credit frames after the
    /// fill, which under the extends-to-end rule can push a MAX_DATA past
    /// a frame that runs to the end of the plaintext.
    ///
    /// The `assert!(found)` is load-bearing: if the core never coalesces
    /// a credit frame with a fill, this test would otherwise assert
    /// nothing and read as a pin (working rule 9).
    ///
    /// **Unreachable in slice 4 — owed to slice 7 (ruling 114).** The
    /// coincidence this needs is a packet that owes credit *and* has stream
    /// data pending. §16.7 makes sealing synchronous inside the mutating
    /// call and slice 4 has no congestion bound, so a `write()` that is
    /// admitted is flushed before the call returns and a `write()` that is
    /// blocked was **refused** — the bytes stay with the caller, not in the
    /// core. There is therefore no state in which stream data is pending
    /// across calls, in any conforming slice-4 build. §14's congestion
    /// window creates exactly that state, which is why this is slice 7's
    /// and not a defect here.
    ///
    /// Kept as the author wrote it, with its own "asserted nothing" guard
    /// intact, so it goes green when the bound that makes it meaningful
    /// arrives. Deleting it would lose the obligation; leaving it running
    /// would fail a correct build.
    #[test]
    #[ignore = "needs §14's congestion bound to create pending stream data (slice 7)"]
    fn credit_frames_precede_the_stream_fill_in_a_packet() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        // Owe the peer a MAX_STREAM_DATA: half a stream window consumed.
        let id = Solo::peer_uni(0);
        let half = (INITIAL_MAX_STREAM_DATA / 2) as usize;
        let _ = s.deliver_stream_bytes(t, id, 0, half, false);
        let r = s.conn.accept(Dir::Uni).expect("open");
        let _ = read_exactly(&mut s.conn, t, r, half);

        // …and, at the same time, have plenty of our own data pending.
        let mine = s.conn.open(Dir::Uni).expect("open");
        assert_eq!(write_all(&mut s.conn, t, mine, &ramp(0, 32 * 1024)), 0);

        let d = drain(&mut s.conn);
        let packets = s.packets(&d);

        let mut found = false;
        for pkt in &packets {
            let credit = pkt
                .iter()
                .position(|f| matches!(f, Wire::MaxStreamData { .. } | Wire::MaxData(_)));
            let stream = pkt.iter().position(|f| matches!(f, Wire::Stream { .. }));
            if let (Some(c), Some(st)) = (credit, stream) {
                found = true;
                assert!(c < st, "§8.5: control frames, then the fill — {pkt:?}");
            }
        }
        assert!(
            found,
            "no packet carried both a credit frame and stream data, so this \
             test asserted nothing: {packets:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// The slice boundary, asserted rather than assumed
// ═══════════════════════════════════════════════════════════════════════

mod slice_boundary {
    use super::*;

    /// `CONTRACT-4a.md` §5: with no ACK processing a send half can never
    /// reach `DataRecvd`, so `StreamFinished` **never fires** in slice 4.
    ///
    /// Mutation caught: the collapsed implementation the contract warns
    /// about by name — **freeing the send half on send instead of on
    /// acknowledgement**. That build fires `StreamFinished` here, and it
    /// would make slice 5's loss-recovery tests pass for free while
    /// dropping every byte that needed retransmitting. This is the only
    /// place in 4a where that mutation is visible at all.
    #[test]
    fn a_send_half_never_reports_finished_because_slice_four_has_no_acks() {
        let t = t0();
        let mut p = Pair::installed_at(t);

        let r = p.a.open(Dir::Uni).expect("open");
        assert_eq!(write_all(&mut p.a, t, r, &ramp(0, 8192)), 0);
        p.a.finish(t, r).expect("finish");
        let (da, db) = p.pump(t);

        // The bytes really did arrive: this is not a test of a broken wire.
        let rb = p.b.accept(Dir::Uni).expect("the peer saw the stream");
        let (got, eof) = read_available(&mut p.b, t, rb);
        assert_eq!(got, ramp(0, 8192));
        assert!(eof);

        assert_eq!(
            da.count_events(|e| matches!(e, ConnEvent::StreamFinished { .. })),
            0,
            "§9.3: `DataRecvd` needs every byte acknowledged, and §12 is \
             slice 5 — a build firing this has freed on send"
        );
        assert_eq!(
            db.count_events(|e| matches!(e, ConnEvent::StreamFinished { .. })),
            0,
            "and the receiving side has no send half to finish at all"
        );
    }

    /// §16.4: "output ordering within one drain preserves generation
    /// order — a transmit and the event it caused come out in that
    /// order." Normative, and tests depend on it.
    ///
    /// Mutation caught: two output queues merged at drain time, or events
    /// pushed ahead of the transmits that produced them. Here the
    /// violation's CLOSE is generated before the `Closed` it causes.
    #[test]
    fn a_transmit_and_the_event_it_caused_leave_in_generation_order() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let d = s.deliver(t, &stream_frame(Solo::our_uni(0), 0, &ramp(0, 4), false));
        let close_at = d
            .position(|o| matches!(o, ConnOutput::Transmit(_)))
            .expect("§8.2 sends a CLOSE");
        let closed_at = d
            .position(|o| matches!(o, ConnOutput::Event(ConnEvent::Closed(_))))
            .expect("§8.2 surfaces the loss");
        assert!(
            close_at < closed_at,
            "§16.4: generation order — {:?}",
            d.outs
        );
    }

    /// §8.3: `0x05` is *reserved*, not implemented, and slice 4 must not
    /// quietly start accepting it.
    ///
    /// Mutation caught: STOP_SENDING handled "while we are in here
    /// anyway" — §19 defers it, and a peer that finds it working would
    /// depend on behaviour the spec has not ratified.
    #[test]
    fn the_reserved_stop_sending_type_is_still_a_structural_error() {
        let t = t0();
        let mut s = Solo::installed_at(t);

        let mut f = Vec::new();
        put(&mut f, crate::constants::FRAME_STOP_SENDING_RESERVED);
        put(&mut f, Solo::peer_uni(0));
        put(&mut f, 0);

        let d = s.deliver(t, &f);
        let frames = s.drain_frames(&d);
        assert_violation(&d, &frames, PROTOCOL_VIOLATION);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Fixture 2 — one core, one raw hiss half
// ═══════════════════════════════════════════════════════════════════════

/// The peer as a bare sealing/opening pair, so a test can put arbitrary
/// bytes on the wire.
struct RawPeer {
    seal: <Suite as Handshake>::Seal,
    open: <Suite as Handshake>::Open,
    /// The peer's own index — what the core under test addresses.
    our_index: u32,
    /// The core-under-test's index — what we address.
    peer_index: u32,
}

impl RawPeer {
    fn from_session(s: EstablishedSession<Suite>) -> Self {
        Self {
            seal: s.seal,
            open: s.open,
            our_index: s.our_index,
            peer_index: s.peer_index,
        }
    }

    fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> {
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

    fn open_dgram(&mut self, dgram: &[u8]) -> Vec<u8> {
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

fn data_header(receiver_index: u32, counter: u64) -> Vec<u8> {
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
struct Solo {
    conn: Connection<Suite>,
    peer: RawPeer,
}

impl Solo {
    /// A core that has **not** yet been installed, with the two halves of
    /// the session that will install it.
    ///
    /// §16.9 makes such a core writable and nothing it writes can be
    /// sealed until the session exists, so writes to two streams both stay
    /// pending — which is the only way slice 4 can put two streams into
    /// one fill pass (ruling 114).
    fn connecting() -> (
        Connection<Suite>,
        EstablishedSession<Suite>,
        EstablishedSession<Suite>,
    ) {
        let (sa, sb) = handshake_pair();
        (Connection::connecting([0xa5u8; 32]), sa, sb)
    }

    /// Wrap an already-installed core and its peer half.
    fn around(conn: Connection<Suite>, peer: EstablishedSession<Suite>) -> Self {
        Self {
            conn,
            peer: RawPeer::from_session(peer),
        }
    }

    fn installed_at(now: Instant) -> Self {
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
    fn deliver(&mut self, now: Instant, frames: &[u8]) -> Drained {
        let dgram = self.peer.seal(frames);
        self.conn.handle_datagram(now, a_addr(), &dgram);
        drain(&mut self.conn)
    }

    /// Decode every datagram a drain produced into one flat frame list.
    fn drain_frames(&mut self, d: &Drained) -> Vec<Wire> {
        self.packets(d).into_iter().flatten().collect()
    }

    /// The same, kept **per packet** — §8.5's packing rules are
    /// statements about one packet and are unassertable once flattened.
    fn packets(&mut self, d: &Drained) -> Vec<Vec<Wire>> {
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
    fn peer_uni(index: u64) -> u64 {
        raw_id(index, Dir::Uni, true)
    }

    /// A peer-opened **bidi** stream's wire id.
    fn peer_bidi(index: u64) -> u64 {
        raw_id(index, Dir::Bi, true)
    }

    /// A stream **we** opened in our own uni space — the peer may never
    /// send STREAM or RESET_STREAM on it (§8.4).
    fn our_uni(index: u64) -> u64 {
        raw_id(index, Dir::Uni, false)
    }
}

/// A payload whose every byte is a function of its offset, so an
/// off-by-`n` shift is visible and a `0xAA`-fill is not (§11.1).
fn ramp(offset: usize, len: usize) -> Vec<u8> {
    (offset..offset + len).map(|i| (i % 251) as u8).collect()
}

/// Assert the connection died locally with `code`, sending the peer a
/// CLOSE carrying the same code.
///
/// Both halves, because a build that surfaces the right `ConnectionLost`
/// and puts a different code on the wire is exactly what Appendix B and a
/// peer's operator would see differently.
fn assert_violation(d: &Drained, frames: &[Wire], code: u64) {
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
fn assert_alive(d: &Drained) {
    assert_eq!(
        d.closed(),
        None,
        "the connection must survive this: {:?}",
        d.outs
    );
}

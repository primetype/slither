//! The connection core — §7's session, §8's frames, §15's teardown.
//!
//! `core::Connection` is a pure state machine: **`now: Instant` is an
//! argument on every mutating call and nothing here reads a clock**, and
//! every mutating call is followed by draining
//! [`poll_output`](Connection::poll_output) to the terminal
//! [`ConnOutput::Timeout`] (§16.4).
//!
//! # What this slice builds
//!
//! §7.1's counter (hiss's, never ours), §7.2's replay window, §7.4's two
//! seal paths and death clock, §7.7's ratchet (hiss's — the obligation is
//! *not* to chase epochs), §7.8's one-session rule, §7.9's exhaustion,
//! §8's codec for PADDING/PING/ACK/CLOSE, §15's three post-mortem states,
//! §16.5's timer table and §16.7's plan-seal-commit.
//!
//! Not here: §9's streams, §10's flow control, §11's datagrams, §12's ACK
//! *semantics* (the frame is codec-only), §13's recovery, §14's congestion
//! control, §7.5's keepalives and contested probe, §6.4/§6.5's roaming.
//! **This is a boundary, not a regression**, and it is stated so a reviewer
//! does not read the absence as one. As in slice 2a, the `ConnEvent`
//! variants those sections define are **absent rather than stubbed**: an
//! uninhabited variant is a claim about the protocol.
//!
//! # The receive path, in the one order §7.2 admits
//!
//! §3.1's gate → AEAD open → **replay check** → **replay mark** → the
//! §3.4 empty-plaintext keepalive short-circuit → parse the *whole*
//! plaintext → apply. A window check before the AEAD would let an off-path
//! forger who observed a cleartext counter poison the window; a
//! parse-and-apply loop would apply a valid frame that a later frame in the
//! same packet invalidates (§8.2).
//!
//! §8.2's structural failure keeps "the already-performed replay mark" and
//! applies nothing else — so the mark happens before the parse, and stays.

pub(crate) mod ack;
pub(crate) mod close;
pub(crate) mod congestion;
pub(crate) mod datagram;
pub(crate) mod flow;
pub(crate) mod frame;
pub(crate) mod mobility;
pub(crate) mod recovery;
pub(crate) mod recv;
pub(crate) mod send;
pub(crate) mod session;
pub(crate) mod stream_id;
pub(crate) mod streams;
pub(crate) mod timers;

// Slice 3a's acceptance tests, written independently from `SPEC.md` and
// `STORIES.md` by an author who never read this directory (working rule 6).
// Declared here at integration rather than by the implementer, so that
// neither agent could reach the other's file.
#[cfg(test)]
mod tests;

// Slice 4a's §9/§10 tests, written from `SPEC.md` and `CONTRACT-4a.md` in
// an isolated worktree by an author who never saw this slice's code — and
// whose 73 tests found three defects in the contract itself (Round 18).
// Declared here at integration, for the same reason as `tests` above.
#[cfg(test)]
pub(crate) mod testfix;
#[cfg(test)]
mod tests_streams;

// Slice 5a's §12 and §13/§14 tests, written from `SPEC.md` and
// `CONTRACT-5a.md` by two authors who never saw this slice's code and never
// saw each other's file (working rule 6). Declared here rather than by
// either of them, so that no agent could reach another's path — the race
// that destroyed 68 tests in slice 2a.
#[cfg(test)]
mod tests_ack;
#[cfg(test)]
mod tests_recovery;

// Slice 7's own unit tests — the **implementer's**, and deliberately named
// so that no story file could ever collide with them (`CONTRACT-7.md` §11).
// The slice's acceptance tests are two blind authors' and live in `tests/`.
#[cfg(test)]
mod tests_roam;

// **[ruling 193(b)]** The `Contested::Pending` gap, written by a **third**
// blind author after both story-test authors proved — independently, one by
// arithmetic and one by construction — that the state is unreachable from an
// integration test. The declaration is the integrator's (working rule 15):
// it is valid only once the file and the implementation both exist, and a
// `mod` for a missing file reds every gate at once.
#[cfg(test)]
mod tests_contested;

// **[ruling 203/207]** The budget-shaped sizing of `pump_packets`, written by
// an author blind to this pass's implementation. The file is **theirs
// alone**: this implementer declared the module and created nothing at the
// path (working rule 6 — slice 2a's placeholder stub destroyed 68
// independently-written tests because both briefs named one file).
#[cfg(test)]
mod tests_sizing;

// ── slice 6's core tests: NEVER WRITTEN, block retired ───────────────────
//
// **[ruling 216]** These two declarations sat commented here since slice 6
// waiting for files that do not exist and never did — `git log --all`
// knows no `tests_datagram.rs` or `tests_message.rs`. Slice 6's authors
// wrote *integration* tests instead (`tests/story_datagram.rs`, 11 tests;
// `tests/story_message.rs`, 15), which have passed every gate since.
// Nothing was lost.
//
// Retired rather than left, because **a commented `mod` for a file that
// was never written is indistinguishable from one whose file was lost**,
// and the comment asserted the second (*"the files land with them"*). That
// is the standing hazard of ruling 211's mechanism, found on its first use
// — the integrator's uncomment step is the only thing that ever
// distinguishes the two, and nothing fails if it is skipped.

// ── slice 7b's core tests, at integration ────────────────────────────────
//
// Same discipline, now ruled rather than inferred: **[ruling 211]** amends
// working rule 6 so an implementer lands the declaration **commented out**
// and creates nothing at those paths — a `mod` naming a missing file is a
// compile error, which is `Cargo.toml`'s `[[test]]` failure one layer down.
// The integrator uncomments these when the authors' files arrive.
//
// Uncommented by the integrator, 2026/08/16, all three files present.
#[cfg(test)]
mod tests_livelock;
#[cfg(test)]
mod tests_path;
// Slice R40-A's blind-author module (rulings 249/254) — declared by the
// integrator, 2026/08/17, per working rules 6/15 and ruling 211.
#[cfg(test)]
mod tests_pto_gate;
#[cfg(test)]
mod tests_reassembly;
// Ruling 265's core regression — the vetoed keepalive and the death clock.
#[cfg(test)]
mod tests_park;
//
// **Integrator — `testfix.rs` has aged out again, and it is the reason 13
// lib tests are red on this commit.** `parse_frames`' fallback arm panics
// on frame type `0x1a`, exactly as it did for ACK at slice 5 and for
// DATAGRAM at slice 6, and its own doc comment says the arm belongs there
// rather than in the caller (working rule 15). The core now emits
// `PATH_CHALLENGE`/`PATH_RESPONSE`, so every test that decodes a packet
// built while an address is unvalidated hits it. Two `Wire` variants and
// one `parse_frames` arm — eight fixed bytes each, no length prefix — take
// the suite from 662/675 to 675/675; that was verified here against a
// temporary patch, which was reverted rather than committed because the
// file is the integrator's and T1 needs a `path_challenge_frame` builder in
// it too (`CONTRACT-7b.md` §9).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};

use crate::constants;
use crate::error::{
    ConfigError, ConnectionLost, DatagramError, MessageError, ReadError, WriteError,
};
use crate::packet::{Handshake, Inbound, classify};

use self::ack::{AckAction, AckState};
use self::close::{Closing, Lifecycle};
use self::congestion::{Controller, NewReno};
use self::datagram::Datagrams;
use self::flow::Flow;
use self::frame::{Close, Frame, Packing, Structural};
use self::mobility::{Amplification, Contested};
use self::recovery::{AckOutcome, Recovery, SentFrame, SentPacket};
use self::session::Session;
use self::timers::{TimerKind, Timers};

// §9.1's two public types (ruling 101). Fully `pub` so `core::mod.rs` can
// re-export them out of the `pub(crate)` core — a `pub(crate) use` here
// would make the outer re-export E0365.
pub use self::stream_id::{Dir, StreamId};
pub(crate) use self::streams::{StreamRef, Streams, StreamsExhausted};

use super::{EstablishedSession, Install, Role, ToEndpoint, Transmit};

/// A connection's core state machine. §16.4.
pub(crate) struct Connection<C: Handshake> {
    session: Option<Session<C>>,
    sub_seed: [u8; 32],
    /// §16.6's connection-side CSPRNG, seeded from [`sub_seed`](Self::sub_seed).
    ///
    /// **[rulings 208, 210(b)]** Its one consumer is §7.3's address-validation
    /// challenge. Ruling 208 as written said *"drawn from the endpoint
    /// RNG"*, naming a path that does not exist: both arming sites are
    /// inside this core and `Connection` cannot reach `Endpoint::rng`.
    /// §16.6 draws the sub-seed **unconditionally** — *"so connection-side
    /// randomness can never perturb the endpoint's draw order"* — which is
    /// what makes the correction free: every seeded test still sees the same
    /// endpoint-side sequence, and *"one root seed reproduces the whole
    /// system"* still holds.
    rng: ChaCha20Rng,
    outputs: VecDeque<ConnOutput>,
    installed: bool,
    lifecycle: Lifecycle,
    timers: Timers,
    /// §16.4's `Closed` is emitted **once** per connection: it is the value
    /// behind `closed()`'s latch, and a second one would be a contradictory
    /// `ConnectionLost` nobody can observe.
    closed_emitted: bool,
    /// The receive-path plaintext buffer, owned here so the session can be
    /// borrowed again while the frames are being applied.
    scratch: Vec<u8>,
    /// **[ruling 106]** `None` until the install; §9.1's stream-ID parity
    /// reads it, and the core cannot derive it (§6.6 step 4).
    role: Option<Role>,
    /// §9's four ID spaces and both halves of every open stream.
    streams: Streams,
    /// §11.3's two bounded queues and §11.5's counters. **Core state**, not
    /// shell state — §11.3 says so in terms.
    datagrams: Datagrams,
    /// §10's two credit ledgers and the cumulative stream limits.
    flow: Flow,
    /// Events generated while a packet is being applied, drained into
    /// `outputs` before the transmits they cause (§16.4's generation order).
    events: Vec<ConnEvent>,
    /// §18.1's cause, retained so the stream verbs can surface
    /// `ConnectionLost` after the one `Closed` event has gone out.
    lost: Option<ConnectionLost>,
    /// §12.4's delayed-ACK policy — four scalars **alongside** §7.2's
    /// window, never a second received-packet record (§12.2).
    ack: AckState,
    /// §13's sent-packet map, RTT estimator, and loss/PTO state.
    recovery: Recovery,
    /// §14's controller. NewReno is v1's one implementation (§14.1);
    /// CUBIC and BBR are §19's, behind the same trait.
    congestion: NewReno,
    /// §7.3's anti-amplification budget, **per session** (ruling 170).
    amplification: Amplification,
    /// §7.5's contested mark (rulings 36, 41, 45/46, 175–177, 179).
    contested: Contested,
    /// **[ruling 208]** The eight bytes a received `PATH_CHALLENGE` obliges
    /// us to echo, if one is outstanding.
    ///
    /// **At most one, overwritten rather than queued** (§8.4, §17.5, ruling
    /// 212(d)): the newest challenge is the only one whose answer can still
    /// validate anything, and a list here would turn a challenge flood into
    /// a response flood. Kept across a roam (§13.6): it answers the peer's
    /// question about *its* path, which our endpoint moving does not change.
    owed_path_response: Option<[u8; 8]>,
    /// §7.5's beacon interval. `None` disables it, which is the default:
    /// the *passive* keepalive dance needs no opt-in (ruling 39), and this
    /// knob is only for a link that is mutually idle.
    persistent_keepalive: Option<Duration>,
}

/// What a received, authenticated, window-fresh packet turned out to be.
enum Received {
    /// §3.4's empty plaintext: the keepalive. It never reaches §8's layer.
    Keepalive,
    /// A parsed frame stream, applying nothing yet (§8.2).
    Frames(Vec<Frame>),
    /// §8.2's structural failure class.
    Structural(Structural),
}

/// Which of §7.3's two path frames one packet plan carries.
///
/// **[ruling 208]** Both obligations are discharged on the **send**, not on
/// the plan, so the planner reports what it packed and the sender acts on
/// it — the same shape the datagram queue's `sent_datagram` uses, and for
/// the same reason: a refused packet must leave both of them owed.
#[derive(Default)]
struct PathPacked {
    /// The response this plan echoes, if the connection owed one.
    response: Option<[u8; 8]>,
    /// Whether the plan carries this arming's challenge.
    challenge: bool,
}

impl<C: Handshake> Connection<C> {
    /// A connection `connect()` created: no session until its `Install`
    /// arrives.
    pub(crate) fn connecting(sub_seed: [u8; 32]) -> Self {
        Self {
            session: None,
            sub_seed,
            rng: ChaCha20Rng::from_seed(sub_seed),
            outputs: VecDeque::new(),
            installed: false,
            lifecycle: Lifecycle::Live,
            timers: Timers::new(),
            closed_emitted: false,
            scratch: Vec::new(),
            role: None,
            streams: Streams::new(),
            datagrams: Datagrams::default(),
            flow: Flow::new(),
            events: Vec::new(),
            lost: None,
            ack: AckState::new(),
            recovery: Recovery::new(),
            congestion: NewReno::new(),
            // §7.3: a `connect()`-supplied address is **not** armed — a
            // dialled connection starts validated. The budget arms at
            // exactly two events and this is neither.
            amplification: Amplification::validated(),
            contested: Contested::No,
            owed_path_response: None,
            persistent_keepalive: None,
        }
    }

    /// **[rulings 208, 210(b)]** Eight fresh bytes for one arming of §7.3's
    /// budget, from §16.6's per-connection sub-seed.
    ///
    /// Drawn at the instant [`Amplification::arm`] is called and at no other
    /// instant — one challenge per arming, never reused across armings. A
    /// roam back to a previously-challenged address draws a new value:
    /// re-arming with the previous one would let a peer bank a response
    /// before the roam it will be asked to prove.
    fn draw_challenge(&mut self) -> [u8; 8] {
        let mut challenge = [0u8; 8];
        self.rng.fill_bytes(&mut challenge);
        challenge
    }

    /// A connection `accept()` returned: established on arrival, and
    /// **never** followed by an `Install` (§16.4).
    ///
    /// `now` is the install instant. §7.4 pins both liveness clocks to it
    /// and starts the death deadline **already armed** — the accept path
    /// has no later event to read that instant from, and the core may not
    /// read a clock, so the instant has to arrive here.
    pub(crate) fn established(
        now: Instant,
        sub_seed: [u8; 32],
        session: EstablishedSession<C>,
        role: Role,
    ) -> Self {
        let mut conn = Self::connecting(sub_seed);
        // §7.3's second arming event: an **accepted initiation's msg1
        // anchor**. The msg1 qualifies as authenticated — *"its handshake
        // tail tags having verified at admission"* — so it credits the
        // received counter, and it is window-fresh by construction (nothing
        // has been received on this session yet), which is what ruling 169
        // requires of anything that funds the budget.
        //
        // The credit is `INIT_PACKET_LEN` rather than a length threaded down
        // from the endpoint because §3.1's gate admits initiations at
        // **exactly** that size: there is no other length an accepted msg1
        // can have had.
        //
        // Armed **before** the install so the pump inside it cannot slip a
        // datagram past an unarmed budget. **[rulings 208, 210(b)]** The
        // challenge comes from the connection's own RNG, which exists from
        // `connecting()` — so unlike ruling 168's floor it needs nothing the
        // session install provides, and the two-phase arming that
        // `Amplification::set_floor` existed for is gone.
        let challenge = conn.draw_challenge();
        conn.amplification = Amplification::arm(challenge, constants::INIT_PACKET_LEN as u64);
        // **[A2]** The msg2 that provoked this arming has **already gone**
        // to this address: the endpoint emitted `RESP_PACKET_LEN` bytes to
        // it (`endpoint/staged.rs`, `endpoint/routing.rs`) before this
        // connection existed, and `arm` starts `sent` at 0. §7.3 caps
        // *total bytes sent* to an unvalidated address, so leaving those
        // 107 uncounted measured 3.55× against a normative MUST of 3.
        // Ruling 170 forbids an endpoint-side per-address table, so the
        // charge lands here, where the connection first exists.
        //
        // **Once, because msg2 is emitted once per arming.** `frame_resp`
        // has exactly two call sites — the staged accept and §6.6's
        // tie-break admission — each of which emits one msg2 and creates
        // one connection; there is no responder-side msg2 retransmission,
        // and a repeated msg1 against a live static goes through §6.4's
        // replacement to a *new* connection with its own arming.
        //
        // The budget still admits a challenge with room to spare:
        // 3 × 196 = 588, less 107, leaves 481 against the 39 bytes a
        // challenge datagram costs.
        conn.amplification
            .on_sent(constants::RESP_PACKET_LEN as u64);
        // `true`: this constructor **is** §6.4's accept path, so the anchor
        // is the msg1 source by construction. The budget is already armed
        // above; passing it here keeps `install`'s rule single-sourced.
        conn.install(now, session, role, true);
        conn
    }

    /// The §16.6 sub-seed. Unused until slice 4; see [`super`]'s docs.
    pub(crate) fn sub_seed(&self) -> &[u8; 32] {
        &self.sub_seed
    }

    /// Whether a session is installed.
    pub(crate) fn is_established(&self) -> bool {
        self.session.is_some()
    }

    /// The installed session, if any. `None` once the state is dropped.
    pub(crate) fn session(&self) -> Option<&EstablishedSession<C>> {
        self.session.as_ref().map(Session::established)
    }

    /// §7.4's liveness clocks.
    pub(crate) fn liveness(&self) -> Option<&session::Liveness> {
        self.session.as_ref().map(Session::liveness)
    }

    /// The counter the next successful seal will use (§7.1).
    ///
    /// Reading this after a mutating call and **before** any `poll_output`
    /// is how §16.7's synchronous sealing is observable at all: an
    /// implementation that sealed lazily inside the drain has not moved it.
    pub(crate) fn next_counter(&self) -> Option<u64> {
        self.session.as_ref().map(Session::next_counter)
    }

    /// The armed deadline for one of §16.5's named timers.
    pub(crate) fn timer(&self, kind: TimerKind) -> Option<Instant> {
        self.timers.get(kind)
    }

    /// The address this connection's datagrams go to — §5.6's anchor, moved
    /// by §7.3's roaming.
    ///
    /// `None` before a session is installed; **total after the install**, and
    /// the shell's `remote_address()` mirrors it.
    pub(crate) fn remote_address(&self) -> Option<SocketAddr> {
        self.session().map(|session| session.anchor)
    }

    /// **[ruling 137]** §14.6's path-generation stamp. `0` at construction;
    /// `+= 1` at each **committed** roam, never at a rejected one.
    #[cfg(test)]
    pub(crate) fn path_generation(&self) -> u32 {
        self.recovery.path_gen()
    }

    /// §7.3's budget, for tests. `None` when the address is **validated**
    /// (no budget armed); `Some((sent, received))` in **datagram bytes**
    /// when it is unvalidated.
    #[cfg(test)]
    pub(crate) fn amplification_budget(&self) -> Option<(u64, u64)> {
        self.amplification.counters()
    }

    /// §7.3's outstanding challenge (**[ruling 208]**), for tests. `None`
    /// once the address has validated and nothing is owed to it.
    ///
    /// Replaces `validation_floor()`, which went with ruling 168's
    /// predicate.
    #[cfg(test)]
    pub(crate) fn outstanding_challenge(&self) -> Option<[u8; 8]> {
        self.amplification.outstanding_challenge()
    }

    /// §7.5's contested mark, for tests.
    #[cfg(test)]
    pub(crate) fn contested(&self) -> Contested {
        self.contested
    }

    /// §7.5's beacon interval, or `None` if disabled.
    pub(crate) fn persistent_keepalive(&self) -> Option<Duration> {
        self.persistent_keepalive
    }

    /// §7.5's beacon. `None` disables it.
    ///
    /// Returns `Err(ConfigError::KeepaliveTooShort)` for an interval
    /// **strictly below** `PERSISTENT_KEEPALIVE_MIN` (1 s), and
    /// `Err(ConfigError::KeepaliveTooLong)` for one **at or above**
    /// `DEAD_TIMEOUT` (25 s) — the ceiling gets no constant of its own
    /// (ruling 63: *"a second place `DEAD_TIMEOUT` is written down is a
    /// place it can drift"*).
    ///
    /// On `Err` the current interval is **unchanged**: no clamp, no panic
    /// (ruling 44 — a panic is undefined behaviour across bubble-ffi to iOS,
    /// and a clamp reports success while giving a beacon that does not do
    /// what was asked).
    ///
    /// `None` is accepted at all times, including on a dead connection.
    pub(crate) fn set_persistent_keepalive(
        &mut self,
        now: Instant,
        interval: Option<Duration>,
    ) -> Result<(), ConfigError> {
        // The beacon arms from §7.4's `last_send`, not from the call, so
        // `now` names no instant this verb uses. It is an argument because
        // §16.4 puts one on every mutating call and a signature that omits
        // it is one the next slice has to widen.
        let _ = now;
        validate_persistent_keepalive(interval)?;
        self.persistent_keepalive = interval;
        self.sync_liveness_timer();
        Ok(())
    }

    /// §16.4's endpoint→connection event. `Install` only, **exactly
    /// once**: a second one is a driver bug and is ignored rather than
    /// replacing a live session (§7.8 — no transport state ever crosses a
    /// handshake, and there is no re-install path).
    pub(crate) fn handle_endpoint_event(&mut self, now: Instant, ev: Install<C>) {
        if self.installed {
            debug_assert!(false, "Install is delivered exactly once per connection");
            return;
        }
        if !self.lifecycle.is_live() {
            // Closed before its `Install` landed (§16.9's pre-establishment
            // `close()`). The shell drops such a core, but installing a
            // session into a connection that has already surfaced `Closed`
            // would resurrect it, and §16.4 emits `Closed` once.
            return;
        }
        self.install(now, ev.session, ev.role, ev.anchor_from_msg1);
    }

    /// §16.4's `handle_datagram`.
    ///
    /// `src` is the datagram's source address, and §7.3's roaming is what
    /// reads it.
    ///
    /// # The roam predicate, exhaustively
    ///
    /// A roam is committed **iff all four hold**, and **iff** the
    /// connection's [`Lifecycle`] is `Live`:
    ///
    /// 1. the packet is a **Data** packet — handshake packets never reach
    ///    this core, and §7.3 forbids them roaming a live session anyway;
    /// 2. `session.open(..)` returned `Some` — the **AEAD tag verified**;
    /// 3. the replay window **marked** it: a duplicate, or a counter more
    ///    than `REPLAY_WINDOW` behind, is **not** fresh;
    /// 4. `src` differs from the current anchor.
    ///
    /// §15.2 is explicit that a **closing or draining** connection *"does
    /// not roam; never to the triggering packet's source"*, which is the
    /// fifth conjunct and the one that is not about the packet.
    pub(crate) fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) {
        if self.lifecycle.is_dead() {
            return;
        }

        // A `connect()`-created connection has no session and no
        // `receiver_index`, so nothing can be routed to it. Silent, per
        // §3.1's tier — this is not an error and never reaches anyone.
        if self.session.is_none() {
            return;
        }

        let (counter, prev_greatest, received, roamed) = {
            let Some(Inbound::Data {
                header,
                ad,
                ciphertext,
            }) = classify::<C>(datagram)
            else {
                return;
            };
            let live = self.lifecycle.is_live();

            let session = self
                .session
                .as_mut()
                .expect("checked immediately above, and nothing between takes it");

            // §12.4's out-of-order trigger compares this counter against the
            // window's greatest **before** the mark. Read after `open`, the
            // greatest already includes this packet, the test
            // `counter != prev_greatest + 1` is false for every packet, and
            // the only surviving §12.4 trigger is the every-2nd counter —
            // which no completion test can distinguish.
            let prev_greatest = session.replay().greatest();
            let counter = header.counter;

            // AEAD, then the window check, then the mark, then liveness —
            // all inside `open`, because the order is the whole of §7.2.
            let Some(plaintext) =
                session.open(now, header.counter, ad, ciphertext, &mut self.scratch)
            else {
                return;
            };

            // §7.3's roam, taken **here** and nowhere else: `open` is the one
            // place that has authenticated the packet *and* marked it
            // window-fresh, and §7.2 gives those two together — *"no replayed
            // packet ever moves the endpoint or refreshes liveness"*. The
            // roam and the liveness refresh are the same predicate, so they
            // are taken at the same point.
            let roamed = if live && src != session.established().anchor {
                Some(session.roam_to(src))
            } else {
                None
            };

            let received = if plaintext.is_empty() {
                // §3.4: "An empty plaintext … is the keepalive — it
                // bypasses the frame layer entirely and is the only
                // non-frame plaintext." Parsing it as zero frames would be
                // indistinguishable here and wrong in slice 7.
                Received::Keepalive
            } else {
                match frame::parse(plaintext) {
                    Ok(frames) => Received::Frames(frames),
                    Err(error) => Received::Structural(error),
                }
            };
            (counter, prev_greatest, received, roamed)
        };

        // The packet is authenticated and window-fresh; the borrow of the
        // plaintext is over, so state may move now.
        match roamed {
            // §13.6's roam seam, in §16.4's generation order.
            Some(from) => self.commit_roam(now, from, src, datagram.len() as u64),
            // **[ruling 169]** Only **authenticated and window-fresh** bytes
            // fund the budget. §7.3's exclusion list said only
            // "unauthenticated or undecryptable", which on the literal text
            // let a *replayed* packet replenish a security counter — this is
            // credited at exactly the point §7.2's window marks the packet,
            // and nowhere else.
            //
            // **[A3]** …and only bytes **received from the address being
            // funded**. `roamed` is `None` under *two* conditions: the
            // source already **is** the anchor, or the connection is not
            // live. §15.2 forbids a closing or draining connection from
            // roaming, so on one every source yields `None` — and the
            // unguarded form credited an unvalidated anchor for a datagram
            // that did not come from it, where §7.3 funds the budget from
            // *"total bytes **received from it**"*.
            //
            // The predicate is tested **here**, inside the arm, and
            // `from_anchor` below is deliberately not hoisted above the
            // `match`: that binding is computed *after* the roam on purpose,
            // so the roaming packet itself counts as from-anchor, and moving
            // it would silently change the `Some` arm's meaning.
            None if self.remote_address() == Some(src) => {
                self.amplification.on_recv(datagram.len() as u64)
            }
            None => {}
        }
        self.sync_liveness_timer();
        self.fold_ack_policy(now, counter, prev_greatest, &received);

        if self.lifecycle.is_live() {
            // A roam commits only on a live connection, so `src` is the
            // anchor here whenever the packet reached one — but ruling 168's
            // proof is stated *"from that address"*, and a closing connection
            // that does not roam can still receive from elsewhere.
            let from_anchor = self.remote_address() == Some(src);
            self.apply_live(now, received, from_anchor);
        } else {
            self.apply_post_mortem(now, received);
        }
    }

    /// §16.4's `handle_timeout`. **Idempotent** — every due deadline is
    /// stopped before its logic runs (§16.5), so a repeated call at one
    /// instant finds an empty due set.
    pub(crate) fn handle_timeout(&mut self, now: Instant) {
        if self.lifecycle.is_dead() {
            return;
        }

        // §13.4's probe is **planned** here and built by the pump below.
        // Ruling 76 puts `AckDelay` *after* the loss/PTO evaluation
        // precisely so *"the owed ACK rides any probe or retransmission
        // that evaluation produced"* — and a probe sealed inside the `Pto`
        // arm would already be on the wire by the time `AckDelay` set the
        // flag. Planning it and sealing it in the same mutating call keeps
        // §16.7 satisfied (nothing is deferred to `poll_output`) while
        // making ruling 76's stated consequence actually happen.
        let mut probe = false;
        // §7.5's two keepalives are **planned** here for the same reason,
        // and ruling 174 makes the reason explicit: *loss/PTO/`AckDelay`
        // precede keepalive evaluation*, and that relation is
        // **emission-before-emission** rather than the evaluation-order one
        // §16.5's governing principle gives. A keepalive sealed inside its
        // own arm would be on the wire before the pump packed the ACK
        // `AckDelay` had just made owed — and the keepalive carries no
        // frames, so it cannot carry that ACK itself.
        let mut passive_keepalive = false;
        let mut beacon = false;

        for kind in self.timers.take_due(now).iter() {
            match kind {
                // §7.4: no authenticated fresh receive for `DEAD_TIMEOUT`
                // with an arming send outstanding. §15.4's first row —
                // nothing transmitted, and no linger to run.
                TimerKind::Liveness => self.die(ConnectionLost::TimedOut),
                // §15.2's linger expiry: drop all state. `Closed` was
                // emitted at the death (ruling 81); only `Retired` is owed.
                TimerKind::CloseLinger => self.drop_state(),
                // §13.2's walk, run with no new acknowledgement.
                TimerKind::Loss => {
                    let outcome = self.recovery.on_loss_timeout(now);
                    self.apply_ack_outcome(now, outcome);
                }
                // §13.3's firing. `pto_count` increments **here**, before
                // the probe is built (ruling 139(a)).
                TimerKind::Pto => {
                    self.recovery.on_pto_timeout();
                    probe = true;
                }
                // §12.4: the ACK becomes owed; the pump packs it.
                TimerKind::AckDelay => self.ack.on_delay_expired(),
                // §7.5's contested verdict. §15.4's contested row: the same
                // `TimedOut` variant as liveness — **no new one** — and
                // **nothing is transmitted**. No third notification either:
                // the death arrives on `closed()` (ruling 45).
                TimerKind::Contested => {
                    tracing::debug!(
                        target: "slither::policy",
                        floor = ?self.contested.floor(),
                        "contested verdict: timed out"
                    );
                    self.contested = Contested::No;
                    self.die(ConnectionLost::TimedOut);
                }
                // §7.5's two keepalives. Both send §3.4's **empty
                // plaintext** via the **marking** seal, and both are
                // therefore arming sends: a connection whose entire output
                // is beacons still dies at `R + DEAD_TIMEOUT` (ruling 40 —
                // *"arming enables death, never defers it"*).
                TimerKind::Keepalive => passive_keepalive = true,
                TimerKind::PersistentKeepalive => beacon = true,
            }

            if self.lifecycle.is_dead() {
                break;
            }
        }

        // §16.4's generation order: what the evaluation caused, then the
        // packets it made us owe.
        self.drain_events();
        self.pump_inner(now, probe);
        // …and §7.5's beacons last of all (§16.5, ruling 174).
        if passive_keepalive || beacon {
            self.transmit_keepalive_if_owed(now, passive_keepalive);
        }
    }

    /// §16.4's `close`. §15.2's local close.
    ///
    /// `reason` is truncated to `CLOSE_REASON_MAX` (§8.4) — §16.2 truncates
    /// at the handle, and this makes the over-length case unconstructible
    /// through the core as well.
    ///
    /// A second `close()`, or one on a connection already dying, is a no-op:
    /// the connection dies once and `Closed` is emitted once.
    pub(crate) fn close(&mut self, now: Instant, code: u64, reason: &[u8]) {
        if !self.lifecycle.is_live() {
            return;
        }

        // §11: a queued datagram is **discarded**, never flushed into the
        // CLOSE packet. §15.2 lets `close()` drop state immediately and
        // §11.1 promises nothing about delivery, so there is nothing owed —
        // and flushing is the thing a reader of §8.5 alone might try, since
        // the datagram fill sits in the same stage as the STREAM fill that
        // §15.2 *does* keep for the linger. Stated as code so it is not
        // rediscovered as a question (`PLAN-6.md` §6 U-9).
        self.datagrams.discard_send();

        if self.session.is_none() {
            // §16.9 makes pre-establishment work ordinary, and there is no
            // seal capability yet: no CLOSE can be emitted and there is
            // nothing to linger for. Ruling 50 takes the same position for
            // the analogous `Connecting` case — "an attempt that never
            // completed has no session to close and no wire signal to
            // send".
            //
            // No `Retired` either: it carries `our_index`, a **session**
            // index this connection has never had. See
            // `.slices/03-skeleton/IMPLEMENTATION.md` F1.
            self.emit_closed(ConnectionLost::LocallyClosed);
            self.lifecycle = Lifecycle::Dead;
            self.timers.disarm_all();
            return;
        }

        self.enter_closing(now, code, reason, ConnectionLost::LocallyClosed);
    }

    /// §16.4's drain. Terminates in [`ConnOutput::Timeout`], which is both
    /// the drain sentinel and the next-deadline announcement.
    ///
    /// Takes no `now` and seals nothing: §16.7 puts sealing inside the
    /// mutating call that triggers it, which is what makes `Loss`/`Pto`
    /// idempotency real.
    pub(crate) fn poll_output(&mut self) -> ConnOutput {
        self.outputs
            .pop_front()
            .unwrap_or_else(|| ConnOutput::Timeout(self.timers.next()))
    }

    /// The next deadline, read **without** popping (**[RATIFIED 2026/08/18
    /// — ruling 262]**).
    ///
    /// The same value [`poll_output`](Self::poll_output)'s terminal
    /// `Timeout` announces, computed the same way — `Timers::next` is
    /// already a `&self` read, so this adds no state and costs nothing.
    ///
    /// # Why the announcement is separable from the drain sentinel
    ///
    /// §16.4 makes the terminal `Timeout` *"simultaneously the drain
    /// sentinel and the next-deadline announcement"*, and a caller that
    /// needs the **sentinel** must still use `poll_output`. A caller that
    /// needs only the **value** was, until ruling 262, obliged to obtain it
    /// through a verb that *pops* — so on a non-empty queue it destroyed an
    /// output to learn a number. `Timers::next` is that number, and it does
    /// not depend on the queue being empty: §16.7 puts sealing inside the
    /// mutating call that triggers it, so every verb that queues an output
    /// re-derives the timers in the same call, and the value is correct
    /// whether or not the drain has run.
    ///
    /// The shell's driver is that caller — see `shell::driver::Driver`'s
    /// `deadline`, which is where ruling 262's construction was found.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.timers.next()
    }

    /// T7's seal-failure injection: the next seal fails as §7.9's
    /// exhausted counter would.
    #[cfg(test)]
    pub(crate) fn fail_next_seal(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.fail_next_seal();
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §9/§10 — the stream surface (§16.4)
    // ═══════════════════════════════════════════════════════════════════

    /// **[ruling 106]** The role fixed at install. `None` before it.
    pub(crate) fn role(&self) -> Option<Role> {
        self.role
    }

    /// §16.4's `open`.
    ///
    /// Legal **before establishment** (§16.9): the returned [`StreamRef`] is
    /// usable at once, `write()` on it is ordinary work, and
    /// [`stream_id`](Self::stream_id) is `None` until a session installs.
    pub(crate) fn open(&mut self, dir: Dir) -> Result<StreamRef, StreamsExhausted> {
        self.streams.open(dir, &self.flow)
    }

    /// §16.9's accessor: the wire id, once establishment has fixed parity.
    pub(crate) fn stream_id(&self, r: StreamRef) -> Option<StreamId> {
        self.streams.stream_id(r)
    }

    /// §16.4's `accept`: claim one peer-opened stream of `dir`.
    pub(crate) fn accept(&mut self, dir: Dir) -> Option<StreamRef> {
        self.streams.accept(dir)
    }

    /// §16.4's `write`. `Ok(0)` means **blocked** by stream or connection
    /// credit — the shell parks; it is not end-of-stream.
    ///
    /// **`Ok(0)` means blocked only for a non-empty input.** `write(now, r,
    /// &[])` is a no-op that also returns `Ok(0)`, so a shell must check its
    /// own buffer length before parking, or it parks forever on an empty
    /// write of its own making.
    pub(crate) fn write(
        &mut self,
        now: Instant,
        r: StreamRef,
        data: &[u8],
    ) -> Result<usize, WriteError> {
        self.lost()?;
        let n = self.streams.write(r, data, &mut self.flow)?;
        self.pump(now);
        Ok(n)
    }

    /// §16.4's `finish`: no more data, and the FIN pins the final size.
    ///
    /// Takes `now` because it emits — a FIN-bearing STREAM frame, on the
    /// **marking** `seal` (§7.4, ruling 98) — and §16.7 puts sealing *"within
    /// the mutating call that triggers it"*, never lazily inside
    /// `poll_output()`, which has no instant.
    pub(crate) fn finish(&mut self, now: Instant, r: StreamRef) -> Result<(), WriteError> {
        self.lost()?;
        self.streams.finish(r)?;
        self.pump(now);
        Ok(())
    }

    /// §16.4's `reset` — §9.6's sender-emitted RESET_STREAM.
    pub(crate) fn reset(&mut self, now: Instant, r: StreamRef, error_code: u64) {
        self.streams.reset(r, error_code);
        self.pump(now);
    }

    /// §16.4's `read`: drain the contiguous prefix.
    ///
    /// `Ok(Some(0))` is **no data available** — the shell parks. `Ok(None)`
    /// is end of stream. Getting the two backwards hangs a reader forever on
    /// a finished stream.
    ///
    /// Takes `now` because it emits, for **two** independent reasons. §10.3
    /// makes consumption drive credit, so a read that crosses the re-grant
    /// threshold owes a MAX_STREAM_DATA and possibly a MAX_DATA; and a read
    /// that reaches the final size **retires the half** (§9.7), which for a
    /// peer-opened uni stream fully closes the stream and owes a MAX_STREAMS
    /// (§10.4). The second reason stands even if §10.3's re-grant never
    /// fires.
    ///
    /// # It keeps serving after the connection has died — **ruling 128**
    ///
    /// This used to open with an unconditional `self.lost` check, which
    /// hid data that had already arrived: `drop_state` drops the session
    /// and the timers and leaves `streams` and `flow` alone, and §15.2's
    /// draining endpoint keeps them for the whole `CLOSE_LINGER`. So a
    /// sender that writes, finishes and closes — the fire-and-forget
    /// pattern §16.2 makes reachable by accident — delivered every byte to
    /// a peer that could never read one.
    ///
    /// Now the half is served normally while anything is left, and the
    /// death is reported only when nothing is: the `Ok(Some(0))` that means
    /// *"no data available"* becomes `Err(ConnectionLost)` rather than the
    /// shell's park, because **parking is never permitted on a dead
    /// connection** — nothing further can arrive, so a park would be
    /// permanent.
    ///
    /// `Ok(None)` is passed through unchanged: reaching the FIN after the
    /// death is the drain succeeding, and it is what ruling 128 asks for in
    /// terms (*"`read` serves buffered bytes then `Ok(None)`"*).
    ///
    /// **No `pump` on the dead path.** Nothing can be emitted anyway —
    /// `pump_packets` returns at once unless the lifecycle is live — and
    /// `pump` also re-derives §13's `Loss`/`Pto` deadlines, which on a dead
    /// connection would announce a deadline the driver schedules and the
    /// core then declines to act on: ruling 141's spin, from a new
    /// direction.
    pub(crate) fn read(
        &mut self,
        now: Instant,
        r: StreamRef,
        buf: &mut [u8],
    ) -> Result<Option<usize>, ReadError> {
        let out = self.streams.read(r, buf, &mut self.flow);
        if let Some(lost) = self.lost.clone() {
            return match out {
                Ok(Some(0)) => Err(ReadError::ConnectionLost(lost)),
                served => served,
            };
        }
        self.pump(now);
        out
    }

    /// Abandon a receive half — §16.2's dropped `RecvStream`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 93]** It retires **at once**: the
    /// half is freed, the connection-level contribution is trued up to the
    /// highest stream-level limit ever advertised, and the index is
    /// tombstoned by the mechanism its space requires — §9.2's watermark for
    /// a peer-opened uni stream, whose receive half is the only half this
    /// endpoint holds, and a per-half discard for a bidi stream, whose send
    /// half is still live.
    ///
    /// Takes `now` because the true-up can cross §10.3's threshold and can
    /// fully close a peer-opened uni stream, which grants MAX_STREAMS
    /// (§10.4).
    pub(crate) fn abandon_recv(&mut self, now: Instant, r: StreamRef) {
        self.streams.abandon_recv(r, &mut self.flow);
        self.pump(now);
    }

    /// **Ruling 94.** Total bytes of reassembly **capacity** currently
    /// allocated across every receive half.
    pub(crate) fn reassembly_capacity(&self) -> u64 {
        self.streams.reassembly_capacity()
    }

    /// **Ruling 253.** Total bytes ever written into reassembly storage on
    /// this connection — §10.6's work bound made test-visible, mirroring
    /// `reassembly_capacity` for its memory bound.
    ///
    /// **[AMENDED 2026/08/18 — ruling 263.]** Read *"across every live
    /// receive half"*, and so fell back whenever one retired — most
    /// sharply on a message workload, where `claim_message` retires a half
    /// per message. It is **monotone** now: `Streams` carries the retired
    /// halves' totals forward. See `Streams::retired_copy_work` for why a
    /// work meter aggregates differently from `reassembly_capacity`'s state
    /// one, and for what this is *not* evidence of — §10.6's bound is per
    /// stream, and a growing connection total is not a defect.
    // **Working rule 15 — the integrator removes this attribute.** This
    // accessor's caller is slice R40-E's blind author's work-bound test in
    // `tests_reassembly.rs`, which does not exist on the implementer's tree;
    // ruling 222 makes the `dead_code` lint live in the test build, and it
    // is right, because on this tree the item genuinely has no caller.
    // `expect` rather than `allow` on purpose: the moment the author's file
    // lands, `unfulfilled_lint_expectations` turns this into an error under
    // `-D warnings`, so it cannot go the way ruling 222's blanket allow went.
    pub(crate) fn reassembly_copy_work(&self) -> u64 {
        self.streams.reassembly_copy_work()
    }

    // ═══════════════════════════════════════════════════════════════════
    // §9.8's messages and §11's datagrams — §16.4's four sugar verbs
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `send_message`: allocate the next outbound uni stream, write
    /// the whole payload, set FIN (§9.8).
    ///
    /// # Whole payload or nothing — **rulings 150 and 163**
    ///
    /// [`SendMessage::Blocked`] means **nothing happened**: no stream was
    /// opened, no byte was buffered, no index was spent. §10.6 makes credit
    /// the buffer commitment, so admitting a payload the peer has not
    /// credited would make sender-side buffering `(uncredited message
    /// streams) × MESSAGE_RECV_MAX` — at `INITIAL_MAX_STREAMS_UNI` = 128,
    /// **32 MiB, a term §17.5's ceiling table does not contain**. That is a
    /// memory-bound change requiring its own ratification, not an
    /// implementation choice.
    ///
    /// Not in tension with ruling 134: 134 lets [`write`](Self::write)
    /// outrun the **congestion** window, which is not a buffer bound. Flow
    /// control is, and it still gates admission.
    ///
    /// # The FIN rides the last data frame, and that is normative
    ///
    /// **[ruling 153]** §9.8's overflow predicate is the highest *received*
    /// offset, so a conforming `MESSAGE_RECV_MAX`-sized message whose FIN
    /// travelled in a **separate** empty frame would satisfy that predicate
    /// for one RTT and be reset with `MESSAGE_OVERFLOW` — the *"transfers
    /// die at exactly 256 KiB"* post-mortem ruling 59 describes, pointing at
    /// the wrong cause.
    ///
    /// The requirement is met by ordering, not by a flag:
    /// [`SendHalf::next_chunk`](send::SendHalf::next_chunk) attaches the FIN
    /// to the chunk that ends at the final size, so `finish` must be applied
    /// **before** anything is packed. That is why this calls
    /// `streams.write`/`streams.finish` directly rather than
    /// [`write`](Self::write) and [`finish`](Self::finish), each of which
    /// pumps: an intervening pump would put the last data frame on the wire
    /// with no FIN on it and manufacture exactly the race above.
    pub(crate) fn send_message(
        &mut self,
        now: Instant,
        msg: &[u8],
    ) -> Result<SendMessage, MessageError> {
        // Widening, never narrowing: `MESSAGE_RECV_MAX` is a `u64` and
        // `msg.len()` a `usize`, and a cast the other way is a silent bound
        // change on a 32-bit target (`PLAN-6.md` §9 R7).
        if msg.len() as u64 > constants::MESSAGE_RECV_MAX {
            return Err(MessageError::TooLarge);
        }
        if let Some(lost) = self.lost.clone() {
            return Err(MessageError::ConnectionLost(lost));
        }

        // Connection credit **before** the open. `Streams::open` spends an
        // index against §10.4's cumulative limit and there is no un-open, so
        // checking after it would leave a spent index and a live send half
        // behind on the refusal path — which is precisely the "nothing
        // happened" ruling 150 requires.
        //
        // The stream-level window needs no check of its own: a fresh uni
        // half advertises `INITIAL_MAX_STREAM_DATA`, and
        // `constants.rs`'s `MESSAGE_RECV_MAX == INITIAL_MAX_STREAM_DATA`
        // const-assert makes the payload fit it by construction.
        if msg.len() as u64 > self.flow.send_room() {
            return Ok(SendMessage::Blocked);
        }
        let Ok(r) = self.streams.open(Dir::Uni, &self.flow) else {
            return Ok(SendMessage::Blocked);
        };

        let written = self
            .streams
            .write(r, msg, &mut self.flow)
            .expect("a freshly opened send half is neither finished nor reset");
        debug_assert_eq!(
            written,
            msg.len(),
            "ruling 150: the credit check above admits the whole payload"
        );
        self.streams
            .finish(r)
            .expect("a freshly opened send half is neither finished nor reset");
        self.pump(now);
        Ok(SendMessage::Sent)
    }

    /// §16.4's `recv_message`: claim the oldest **complete unclaimed** uni
    /// stream as one payload, then free the stream (§9.8).
    ///
    /// # It takes `now`, and that is **not** symmetric with `recv_datagram`
    ///
    /// **[ruling 151]** §16.4's own signature omits the instant, and the
    /// omission is ruling 71's shape. This verb **retires a receive half**
    /// — owing MAX_DATA under §10.3 and, through full closure, MAX_STREAMS
    /// under §10.4 — and it can **emit RESET_STREAM**, because §9.8's
    /// overflow check runs *"at the instant such a claim is made"*. §16.7
    /// puts sealing inside the mutating call, and this core has no `Instant`
    /// field to fall back on. Without `now`, S30's reset would wait for the
    /// next `now`-bearing call — on an idle connection, a timer — delaying
    /// by **seconds** the one path whose whole purpose is to fail promptly
    /// instead of stalling. [`abandon_recv`](Self::abandon_recv) already
    /// takes `now` for the weaker of those two reasons.
    ///
    /// Contrast [`recv_datagram`](Self::recv_datagram), which takes none.
    ///
    /// # It keeps serving after the connection has died — **ruling 152**
    ///
    /// Ruling 128's enumeration names `read` and `accept_*` and was written
    /// before this verb existed. Appendix B ratifies an obligation the short
    /// list cannot satisfy — *"`send_message(m)`, then `acked()`, then
    /// `close()`, then drop every handle. Assert endpoint B receives `m` in
    /// full from `recv_message()`"* — and B's driver processes the data and
    /// the CLOSE in one pass, so the claim runs after the latch. The core
    /// therefore does not consult `self.lost`; the death check is the
    /// shell's, and only once this has returned `None`.
    ///
    /// `Some(Vec::new())` is a **delivered empty message**, never "nothing
    /// to claim": a shell that conflated them would park for ever on a
    /// message that had arrived.
    pub(crate) fn recv_message(&mut self, now: Instant) -> Option<Vec<u8>> {
        let claimed = self.streams.recv_message(&mut self.flow);
        // §9.8's scan can have emitted a RESET_STREAM whether or not
        // anything was claimed, and the retirement owes credit either way —
        // so the pump is not conditional on the claim. It is skipped only
        // for a dead connection, on `read`'s reasoning: nothing can be
        // emitted, and re-deriving §13's deadlines there announces a
        // deadline the core will then decline to act on (ruling 141's spin).
        if self.lost.is_none() {
            self.pump(now);
        }
        claimed
    }

    /// §16.4's `send_datagram` (§11). **Never blocks** — §11.3's drop-oldest
    /// discipline absorbs pressure, and §11.1 promises nothing about
    /// delivery, so a full queue is still `Ok(())`.
    ///
    /// The size check is §11.4's *"at the handle, **before any queue**"*: an
    /// oversize payload queues nothing and evicts nothing.
    ///
    /// A **zero-length** datagram is queued and sent. §8.4 admits `0x31`
    /// with `length = 0` and §11 states no minimum; stated here so that no
    /// one invents one.
    pub(crate) fn send_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), DatagramError> {
        if let Some(lost) = self.lost.clone() {
            return Err(DatagramError::ConnectionLost(lost));
        }
        if data.len() > constants::MAX_DATAGRAM_PAYLOAD {
            return Err(DatagramError::TooLarge);
        }
        self.datagrams.push_send(data.to_vec());
        // §16.7: sealing happens **inside the mutating call that triggers
        // it**, and this call can put a frame on the wire.
        self.pump(now);
        Ok(())
    }

    /// §16.4's `recv_datagram`: claim the oldest queued datagram (FIFO).
    ///
    /// # It takes no `now`, and that is not an oversight
    ///
    /// **[ruling 151]** Claiming a datagram emits nothing. Datagrams are
    /// flow-control **exempt** (§10.7), so there is no credit true-up, no
    /// retirement and nothing to seal — the asymmetry with
    /// [`recv_message`](Self::recv_message) is the difference between the
    /// two ledgers, and adding an unused `now` here to make the pair look
    /// alike would be an invented emission point.
    ///
    /// Like `recv_message`, it **drains after the connection's death**
    /// (ruling 152): the core does not consult `self.lost`, and the shell
    /// reports the death only once this has returned `None`.
    pub(crate) fn recv_datagram(&mut self) -> Option<Vec<u8>> {
        self.datagrams.pop_recv()
    }

    /// Emit anything owed on the wire.
    ///
    /// **Additive**, and after the `now` correction it is a convenience
    /// rather than a necessity: every verb that can owe a frame now seals
    /// inside itself (§16.7). Kept because the two-core tests use it to make
    /// "nothing further is owed" assertable.
    pub(crate) fn flush(&mut self, now: Instant) {
        self.pump(now);
    }

    /// §12's ACK application for one stream range. **Uncalled from the
    /// wire** in slice 4; slice 5 wires SPEC §12 to it.
    ///
    /// Takes `now` because an ACK can complete a send half, fully close a
    /// stream and owe a MAX_STREAMS grant (§10.4).
    ///
    /// **[RATIFIED 2026/08/15 — ruling 113, corrected by ruling 130's
    /// round]** `fin` is carried explicitly, off §12's sent-packet map. It
    /// used to be inferred as `range.end == final_size`, which is exact for
    /// every frame *this* implementation emits — the FIN rides the frame
    /// that ends the stream and nothing else — and wrong in general, because
    /// §8.7 lets a retransmission **re-frame ranges freely**, so a range
    /// ending at the final size need not have carried the FIN. The
    /// inference would have silently set `fin_acked` on a re-framed
    /// retransmission and driven the send half to `DataRecvd` early.
    pub(crate) fn on_ack_range(
        &mut self,
        now: Instant,
        r: StreamRef,
        range: std::ops::Range<u64>,
        fin: bool,
    ) {
        self.streams
            .on_ack_range(r, range, fin, &mut self.flow, &mut self.events);
        self.drain_events();
        self.pump(now);
    }

    /// §13's loss detection for one stream range: it returns to the pending
    /// set and is re-framed on a fresh counter (§8.7 `ranges`). **Uncalled
    /// from the wire** in slice 4; slice 5 wires SPEC §13 to it.
    ///
    /// `fin` is explicit for the same reason as [`on_ack_range`](Self::on_ack_range).
    pub(crate) fn on_lost_range(
        &mut self,
        now: Instant,
        r: StreamRef,
        range: std::ops::Range<u64>,
        fin: bool,
    ) {
        self.streams.on_lost_range(r, range, fin);
        self.pump(now);
    }

    // ═══════════════════════════════════════════════════════════════════
    // §12/§13/§14 — the reliability seam
    // ═══════════════════════════════════════════════════════════════════

    /// §16.2's snapshot: every byte handed to the connection at this
    /// instant.
    ///
    /// *"Bytes written after the call do not extend it"* — which is why
    /// this is a value taken now and not a predicate re-evaluated at each
    /// poll. A snapshot re-read on every poll never terminates under a
    /// writer loop, and §16.2 states the terminating case explicitly.
    pub(crate) fn ack_snapshot(&self) -> AckSnapshot {
        AckSnapshot(self.streams.send_offsets())
    }

    /// Whether every byte in `snap` is acknowledged **or abandoned by a
    /// reset**.
    ///
    /// A stream absent from the table, or whose send half has been freed,
    /// counts as settled: a send half is freed only at `DataRecvd` or
    /// `ResetRecvd` (§9.7). An entry at offset 0 is settled vacuously.
    ///
    /// **FIN is deliberately not part of this.** §16.2 scopes the
    /// connection-level snapshot to *"every byte handed to the
    /// connection"*, and a connection-level `acked()` that also waited for
    /// a FIN would never terminate on a stream the application intends to
    /// keep open. `SendStream::acked()` is the verb that includes the FIN.
    pub(crate) fn snapshot_settled(&self, snap: &AckSnapshot) -> bool {
        snap.0
            .iter()
            .all(|(r, offset)| self.streams.send_settled(*r, *offset))
    }

    /// §14.5's sum over §13.5's map — ack-eliciting packets only.
    #[cfg(test)]
    pub(crate) fn bytes_in_flight(&self) -> u64 {
        self.recovery.bytes_in_flight()
    }

    /// §14's congestion window, in bytes.
    #[cfg(test)]
    pub(crate) fn congestion_window(&self) -> u64 {
        self.congestion.window()
    }

    /// §13.1's `smoothed_rtt` — `K_INITIAL_RTT` before any sample.
    #[cfg(test)]
    pub(crate) fn smoothed_rtt(&self) -> std::time::Duration {
        self.recovery.rtt().smoothed_rtt()
    }

    /// §13's recovery state, for the unit tests that assert on it directly.
    #[cfg(test)]
    pub(crate) fn recovery(&self) -> &Recovery {
        &self.recovery
    }

    /// §12.4's policy, folded once per authenticated, **window-fresh**
    /// packet — §7.2: *"No replayed packet ever moves the endpoint or
    /// refreshes liveness."*
    ///
    /// A duplicate that advanced the every-2nd counter would buy an
    /// attacker one extra ACK per replayed packet: free reverse-path
    /// amplification, and invisible to every test that does not count ACKs.
    ///
    /// Skipped once the connection is dying: §15.2's closing state emits
    /// only CLOSE, and arming `AckDelay` there would announce a deadline
    /// that fires with nothing to pack.
    fn fold_ack_policy(
        &mut self,
        now: Instant,
        counter: u64,
        prev_greatest: Option<u64>,
        received: &Received,
    ) {
        if !self.lifecycle.is_live() {
            return;
        }
        let (ack_eliciting, frame_seen) = match received {
            // §3.4's keepalive carries no frames, so it elicits nothing —
            // and §12.3 reports `ack_delay = 0` when the window's largest
            // is one, which is the whole reason `frame_seen` is carried.
            Received::Keepalive => (false, false),
            Received::Frames(frames) => (frame::packet_is_ack_eliciting(frames), true),
            // §8.2: nothing from a structurally-broken packet is applied,
            // and the connection is about to CLOSE. It is frame-bearing but
            // elicits nothing.
            Received::Structural(_) => (false, true),
        };

        match self
            .ack
            .on_recv(now, counter, prev_greatest, ack_eliciting, frame_seen)
        {
            AckAction::None => {}
            AckAction::Now => self.timers.disarm(TimerKind::AckDelay),
            AckAction::Arm(at) => self.timers.arm(TimerKind::AckDelay, at),
        }
    }

    /// §13.6 and §14.6's roam seam, with the anchor already moved.
    ///
    /// # What a roam does, in §16.4's generation order
    ///
    /// The anchor has moved (step 1, in `handle_datagram` where the packet
    /// is). Then: the path generation, `Recovery::on_roam`, `NewReno::reset`,
    /// the budget, the trace, the event.
    ///
    /// # What a roam does **not** do
    ///
    /// Stated because a list is read as exhaustive (working rule 8), and
    /// ruling 173 exists because §13.6's title claimed a scope its body did
    /// not cover:
    ///
    /// * it does **not** clear the sent map — *"ACKs for packets in flight
    ///   to the old address still resolve"* — and does **not** reset
    ///   `bytes_in_flight`, `pto_count`, `loss_time` or `last_ack_eliciting`:
    ///   loss detection continues undisturbed and PTO **state** *"carries
    ///   across untouched"*. **[ruling 249]** §13.6's row is precise about
    ///   which half that is: the `Pto` **announcement** is budget-gated
    ///   (§13.3), and this function zeroes the budget four rows up, so the
    ///   deadline is suppressed from the roam until the first qualifying
    ///   receive re-admits a probe. Nothing here cancels or re-arms it —
    ///   [`sync_recovery_timers`](Self::sync_recovery_timers) re-derives it
    ///   from the state this roam left alone;
    /// * it does **not** clear `smoothed_rtt` or `rttvar` — the estimator is
    ///   *"suspect-but-kept"*. Only `min_rtt` is re-seeded, and the PTO floor
    ///   **may rise** as a result;
    /// * it does **not** reset the replay window, the flow-control state, any
    ///   stream, §7.1's counter or the session keys;
    /// * it does **not** re-handshake and does **not** touch the endpoint —
    ///   §17.4: *"the endpoint tracks no per-connection address"*;
    /// * **[ruling 176]** it leaves a **pending contested mark intact, with
    ///   its floor unchanged**. The counter space is never reset (§7.7), so
    ///   the floor stays meaningful across the roam; a roam changes the
    ///   pending probe's budget prospects, not the question it asks.
    fn commit_roam(&mut self, now: Instant, from: SocketAddr, to: SocketAddr, datagram_len: u64) {
        // Saturating rather than wrapping: `u32::MAX` roams is not reachable,
        // and a wrap to 0 would alias the *initial* generation, un-fencing
        // the oldest packets in the map. Saturation degrades to "nothing is
        // fenced from here on", which is the direction that cannot report a
        // stale RTT sample as fresh.
        self.recovery.on_roam(now);
        self.congestion.reset(now);
        // **[rulings 169, 173, 208]** Both counters reset and **a fresh
        // challenge is drawn**, then the **triggering packet** credits the
        // received side: it is authenticated and window-fresh by §7.3, which
        // is exactly what ruling 169 requires of anything that funds the
        // budget. Without the reset the old address's credit would carry to
        // the new one — *"the reflector §7.3 exists to prevent,
        // reconstructed out of a missing line"*.
        //
        // §13.6's roam-seam table: the outstanding challenge is **re-drawn**
        // and any earlier one discarded, so a `PATH_RESPONSE` echoing it
        // validates nothing thereafter. A response *we* owe the peer is
        // **kept** — see `owed_path_response`, which this deliberately does
        // not clear.
        let challenge = self.draw_challenge();
        self.amplification = Amplification::arm(challenge, datagram_len);
        tracing::debug!(
            target: "slither::roam",
            %from,
            %to,
            path_generation = self.recovery.path_gen(),
            "the session re-homed to a new peer address"
        );
        self.events.push(ConnEvent::AddressMoved { from, to });
    }

    /// §12.5's processing of one received ACK.
    fn on_ack_frame(&mut self, now: Instant, ack: &frame::Ack) {
        let Some(highest_sealed) = self.next_counter().and_then(|next| next.checked_sub(1)) else {
            // Nothing has ever been sealed, so every counter this frame
            // names is above the highest sealed: §12.5 ignores it whole.
            return;
        };
        // §12.5's *"ignore whole"* is **whole**: an ACK above the highest
        // counter we have sealed is not evidence of anything, so it must not
        // clear a mark or validate an address either. `Recovery::on_ack`
        // applies the same test and traces it; this one is silent because
        // the trace would be the same event twice.
        if ack.largest <= highest_sealed {
            self.on_ack_coverage(ack.largest);
        }
        let outcome = self.recovery.on_ack(now, ack, highest_sealed);
        self.apply_ack_outcome(now, outcome);
    }

    /// §7.5's contested probe floor — the **one** high-water-mark predicate
    /// an ACK still satisfies.
    ///
    /// `largest` is the greatest counter the frame covers, so *"covers any
    /// counter at or above the floor"* is exactly `largest >= floor`.
    ///
    /// # There were two floors here, and only one of them was ruling 208's
    ///
    /// This function served §7.3's amplification floor as well, gated on
    /// `from_anchor`. **[ruling 208]** replaced that predicate with a
    /// challenge echo and its machinery is removed rather than left inert —
    /// *"leaving both in place would give an attacker the old path as a
    /// bypass"*. The half below is **not** reached by that ruling and is
    /// kept byte-for-byte: ruling 210 records deleting this function
    /// wholesale as the most likely way to break the change, because it
    /// would silently disable ruling 176's two exits from the pending state
    /// and **no wire test would notice**. The contested mark's threat model
    /// is a different one and its floor is a different floor.
    ///
    /// # Ruling 176's two exits from the pending state
    ///
    /// | state when the covering ACK lands | effect |
    /// |---|---|
    /// | `Armed` | ⇒ `No`; disarm `Contested`; emit `ContestCleared` |
    /// | `Pending` | ⇒ `No`; **cancel the probe**; emit **nothing** |
    /// | `No` | nothing |
    ///
    /// The pending exit is not exotic: the floor is *"the counter the next
    /// seal will use"*, so **any** post-mark seal — a keepalive, a
    /// retransmission, a pure ACK, application Data — lands at or above it.
    /// On the literal pre-ruling text `ContestCleared` would fire **with no
    /// preceding `Contested`**, which is the unmatched-notification mis-read
    /// ruling 46 deleted `under_probe: bool` to prevent; and the
    /// unconditional send rule would emit a **stray probe**, arming a
    /// `KEEPALIVE_TIMEOUT` verdict for a mark that no longer exists.
    ///
    /// In every clearing case the §6.4 refusal **stands**; the basis rule is
    /// untouched.
    fn on_ack_coverage(&mut self, largest: u64) {
        match self.contested {
            Contested::Armed { floor, .. } if largest >= floor => {
                self.contested = Contested::No;
                self.timers.disarm(TimerKind::Contested);
                self.events.push(ConnEvent::ContestCleared);
                tracing::debug!(
                    target: "slither::policy",
                    floor,
                    largest,
                    "contested verdict: cleared"
                );
            }
            Contested::Pending { floor } if largest >= floor => {
                self.contested = Contested::No;
                // **[ruling 176]** *"`ContestCleared` is emitted only where
                // `Contested` was."* The mark-pending gap emits nothing, and
                // so does its exit.
                tracing::debug!(
                    target: "slither::policy",
                    floor,
                    largest,
                    "contested verdict: cleared (pending) — the probe is cancelled and never sent"
                );
            }
            Contested::No | Contested::Pending { .. } | Contested::Armed { .. } => {}
        }
    }

    /// Apply one §12.5 or §13.2 evaluation: §8.7's classes, then §14's
    /// controller.
    ///
    /// Drains no events and pumps nothing — the caller does both, **once**,
    /// so §16.4's generation order survives an ACK that resolves twenty
    /// streams at one instant.
    fn apply_ack_outcome(&mut self, now: Instant, outcome: AckOutcome) {
        for frame in outcome.acked {
            match frame {
                SentFrame::Stream { r, range, fin } => {
                    // Ruling 113: `fin` is carried off the map, never
                    // re-derived — §8.7 lets a retransmission re-frame
                    // ranges freely, so a frame ending at the final size
                    // need not have carried the FIN.
                    self.streams
                        .on_ack_range(r, range, fin, &mut self.flow, &mut self.events);
                }
                SentFrame::ResetStream { r } => self.streams.on_reset_acked(r, &mut self.flow),
                // §8.7's `regenerate` class: a credit frame is
                // **superseded, not confirmed**. Its acknowledgement clears
                // nothing — the identity was cleared when it was packed,
                // and the value it carried is stale by construction.
                SentFrame::MaxData
                | SentFrame::MaxStreamData { .. }
                | SentFrame::MaxStreams { .. } => {}
            }
        }

        for frame in outcome.lost {
            match frame {
                SentFrame::Stream { r, range, fin } => self.streams.on_lost_range(r, range, fin),
                SentFrame::ResetStream { r } => self.streams.on_reset_lost(r),
                // The retransmission carries the **freshest** value, read
                // off the ledger at pack time — never the lost one.
                SentFrame::MaxData => self.streams.owe_max_data(),
                SentFrame::MaxStreamData { r } => self.streams.owe_max_stream_data(r),
                SentFrame::MaxStreams { dir } => self.streams.owe_max_streams(dir),
            }
        }

        for (sent_time, bytes, app_limited) in outcome.ack_events {
            self.congestion.on_ack(now, sent_time, bytes, app_limited);
        }

        // §14.3: **once per loss episode**, after the full lost-packet
        // scan. `AckOutcome::congestion` being an `Option` rather than a
        // `Vec` is the structural enforcement of that.
        if let Some(event) = outcome.congestion {
            self.congestion.on_congestion_event(
                now,
                event.sent_time,
                event.is_persistent,
                event.lost_bytes,
            );
        }
    }

    /// Re-derive §13's two deadlines from the sent-packet map.
    ///
    /// Both are **functions of the map**, exactly as §7.4's `Liveness`
    /// deadline is a function of the receive clock, so they are
    /// re-synchronised after every change rather than armed at each site.
    /// §13.3's *first* precondition — *"armed only while at least one
    /// ack-eliciting packet is in the sent map"* — is then true by
    /// construction: an empty map yields `None`, so an idle connection
    /// cannot self-sustain a probe train at ~20 packets/s against the 10 s
    /// keepalive cadence, which is the failure §13.3 names.
    ///
    /// # §13.3's second precondition (**[RATIFIED 2026/08/17 — ruling 249]**)
    ///
    /// *"…**and while §7.3's amplification budget admits a probe
    /// datagram**"*. The conjunct is applied **here**, at the one arming
    /// site, because [`Recovery`] has no sight of the
    /// budget — see [`probe_can_leave`](Self::probe_can_leave) for the size
    /// it asks about.
    ///
    /// Without it a saturated backoff at a closed budget re-arms itself in
    /// the past forever. `Recovery::pto_deadline` is `anchor + interval ×
    /// 2^min(pto_count, PTO_MAX_EXPONENT)`; the anchor moves only at an
    /// ack-eliciting send, the increment stops advancing once `pto_count`
    /// saturates, and a firing that can emit nothing moves neither. The
    /// single `!Send` driver **every** connection on the endpoint shares
    /// then spins at 100 % of a core until `DEAD_TIMEOUT` reaps the session
    /// — measured, silent in release, and reachable from any roam, which
    /// zeroes the budget (§13.6).
    ///
    /// **The gate is on the announcement, not the state.** Sent map,
    /// `pto_count` and anchor are untouched while the budget is closed; the
    /// connection's `Timeout` falls to the next armed timer — `Liveness` at
    /// the latest — and the session still dies at `DEAD_TIMEOUT`, which is
    /// what §7.3 intends for an address that funds nothing.
    ///
    /// **Re-arming needs no machinery.** The budget grows only on an
    /// authenticated, window-fresh receive (§7.2, §7.3, ruling 169), every
    /// live receive path ends in [`pump`](Self::pump), and every pump ends
    /// here — so the deadline is announced again on the one event that can
    /// lift the hold. That is §16.4's `Contested` principle — a deadline is
    /// never announced for output that cannot leave — and the same shape
    /// slice 7b's F1 fix already gave both keepalives
    /// ([`keepalive_can_leave`](Self::keepalive_can_leave)); ratified for
    /// `Contested`, shipped for the keepalives, and absent only here.
    fn sync_recovery_timers(&mut self) {
        if !self.lifecycle.is_live() {
            return;
        }
        let loss = self.recovery.loss_deadline();
        // **[ruling 249]** The second conjunct. `filter` rather than a
        // branch: the deadline the map permits is computed either way, and
        // only the *announcement* is withheld.
        let pto = self
            .recovery
            .pto_deadline()
            .filter(|_| self.probe_can_leave());
        self.timers.set(TimerKind::Loss, loss);
        self.timers.set(TimerKind::Pto, pto);
    }

    /// Whether §7.3's budget admits §13.4's probe datagram right now
    /// (**[RATIFIED 2026/08/17 — ruling 249]**).
    ///
    /// §13.4: *"while the budget has no room for the 39-byte challenge
    /// datagram the `Pto` deadline is not announced at all"*. The 39 bytes
    /// are §3.4's `DATA_HEADER_LEN`, `PATH_CHALLENGE`'s one-byte type, its
    /// eight opaque bytes and the AEAD tag — **composed, never
    /// transcribed**, which is the same arithmetic `constants.rs`'s
    /// amplification-floor assertion writes.
    ///
    /// That is the right size to ask about because on an **unvalidated**
    /// address the probe *is* the challenge: §8.7's standing obligation puts
    /// `PATH_CHALLENGE` on the packet the PTO built, and it is ack-eliciting,
    /// so no PING is owed beside it (ruling 221). On a **validated** address
    /// [`Amplification::admits`](mobility::Amplification::admits) is
    /// unconditionally true and this gate is vacuous — the state it exists
    /// for is a fresh roam or an msg1 anchor whose remaining room cannot fit
    /// the datagram §13.4 describes.
    ///
    /// It subsumes the other budget-shaped way a probe emits nothing:
    /// [`pump_contested_probe`](Self::pump_contested_probe) stops the pump
    /// dead when the budget refuses the contested probe's 31 bytes, and
    /// `admits` is monotone in the length, so a budget refusing 31 refuses
    /// 39 and the deadline is already withheld.
    ///
    /// **[ruling 250]** The contested probe's packet may now be 40 or 49
    /// bytes rather than 31 — but the **hold** threshold is unchanged, and
    /// this sentence still holds: the coalesced size is taken only when the
    /// room already admits it, and the bare 31-byte PING is what the pump
    /// falls back to, so 31 is still the length a hold refuses.
    fn probe_can_leave(&self) -> bool {
        let size = (constants::DATA_HEADER_LEN + 1 + 8 + constants::AEAD_TAG_LEN) as u64;
        self.amplification.admits(size)
    }

    /// §18.1's cause, once the connection has died. The verbs surface it
    /// rather than accepting work a dead connection can never do.
    fn lost(&self) -> Result<(), WriteError> {
        match self.lost.clone() {
            Some(lost) => Err(WriteError::ConnectionLost(lost)),
            None => Ok(()),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Receive
    // ═══════════════════════════════════════════════════════════════════

    /// Apply a received packet to a **live** connection (§8.2).
    ///
    /// `from_anchor` is whether the packet's source is this connection's
    /// current anchor — ruling 168's *"from that address"*.
    fn apply_live(&mut self, now: Instant, received: Received, from_anchor: bool) {
        let frames = match received {
            Received::Keepalive => {
                // §3.4's keepalive carries no frames, so there is nothing to
                // apply — but its **arrival** can have committed a roam
                // (§7.3), and S18's mover re-homes by exactly this packet.
                // Draining and pumping here is §16.4's generation order for
                // the empty frame stream: *"the events a packet caused, then
                // the packet its arrival made us owe"*. Without it a
                // keepalive-driven roam would hold its `AddressMoved` until
                // some unrelated event drained it.
                self.drain_events();
                self.pump(now);
                return;
            }
            Received::Structural(error) => {
                // §8.2: one trace, then CLOSE(`PROTOCOL_VIOLATION`) and the
                // closing state. Nothing from the packet is applied — the
                // replay mark, already performed, is the sole exception.
                tracing::warn!(
                    target: "slither::frames",
                    error = %error,
                    "structural failure in the frame stream; closing"
                );
                self.enter_closing(
                    now,
                    constants::PROTOCOL_VIOLATION,
                    b"",
                    ConnectionLost::ProtocolViolation {
                        code: constants::PROTOCOL_VIOLATION,
                    },
                );
                return;
            }
            Received::Frames(frames) => frames,
        };

        for frame in frames {
            match frame {
                // §8.4: no fields, no error cases, no effect.
                Frame::Padding => {}
                // §8.3: ack-eliciting and nothing else. The ACK it owes was
                // already folded into §12.4's policy by `fold_ack_policy`,
                // which runs once per packet rather than once per frame —
                // §12.4 counts *packets*, and a peer packing two PINGs into
                // one datagram must not buy two ACKs.
                Frame::Ping => {}
                // §12.5's processing: the sent-packet map, the RTT sample,
                // §13.2's loss evaluation and §14's controller.
                // **[ruling 208]** No longer gated on `from_anchor`: the
                // only thing an ACK's source decided was §7.3's validation,
                // and an ACK no longer decides that. §12.5's processing —
                // the sent map, the RTT sample, §13.2's loss evaluation,
                // §14's controller and §7.5's probe floor — was never
                // source-conditional.
                Frame::Ack(ack) => self.on_ack_frame(now, &ack),
                Frame::Stream(stream) => {
                    if let Err(violation) =
                        self.streams
                            .on_stream_frame(&stream, &mut self.flow, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::ResetStream(reset) => {
                    if let Err(violation) =
                        self.streams
                            .on_reset_stream(&reset, &mut self.flow, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::MaxData(max) => {
                    let raised = self.flow.on_max_data(max);
                    self.streams.on_max_data(raised, &mut self.events);
                    // **[ruling 150]** The connection-level companion to the
                    // per-half `StreamWritable`s above: a `send_message`
                    // refused for connection credit holds no stream, so
                    // none of them names it.
                    if raised {
                        self.events.push(ConnEvent::SendCreditAvailable);
                    }
                }
                Frame::MaxStreamData(grant) => {
                    if let Err(violation) =
                        self.streams
                            .on_max_stream_data(grant.id, grant.max, &mut self.events)
                    {
                        self.kill(now, violation);
                        return;
                    }
                }
                Frame::MaxStreamsBidi(max) => self.on_max_streams(Dir::Bi, max),
                Frame::MaxStreamsUni(max) => self.on_max_streams(Dir::Uni, max),
                // **[ruling 208]** §8.4: the obligation is **unconditional**
                // and the bytes are never interpreted. It is not gated on
                // the challenge being one we expected, on the source having
                // roamed, or on anything else — *"a responder that filtered
                // them would be answering a question it cannot see the point
                // of"*. It is the peer's budget, not ours, that the response
                // unlocks.
                //
                // A newer challenge **overwrites** an unanswered older one
                // rather than queueing behind it (§17.5): the newest is the
                // only one whose answer can still validate anything, and
                // this is what stops a challenge flood becoming a response
                // flood.
                Frame::PathChallenge(value) => {
                    tracing::debug!(
                        target: "slither::roam",
                        "a PATH_CHALLENGE arrived; one PATH_RESPONSE is owed"
                    );
                    self.owed_path_response = Some(value);
                }
                // **[ruling 208]** Gated on `from_anchor`, and that gate is
                // the whole predicate rather than a refinement of it: a peer
                // that echoes from its **old** address has proved nothing
                // about the new one, which is the address the budget is
                // armed against.
                //
                // A mismatch is a **semantic no-op** (§8.4, ruling 212(d)):
                // traced, never an error. `PROTOCOL_VIOLATION` here would be
                // a keyless remote kill primitive for anyone who can guess a
                // frame boundary.
                Frame::PathResponse(echo) => {
                    if from_anchor {
                        self.amplification.on_path_response(&echo);
                    }
                    tracing::debug!(
                        target: "slither::roam",
                        from_anchor,
                        validated = self.amplification.is_validated(),
                        "a PATH_RESPONSE arrived"
                    );
                }
                // §11: the unreliable path. Flow-control exempt (§10.7), so
                // nothing is charged and nothing is checked — §11.4's
                // receiver oversize rule is unrepresentable, the frame's
                // data lying inside one plaintext by construction.
                //
                // The event fires for the **admitted** datagram even when
                // the queue was full, because a new item is claimable; the
                // evicted one gets no event and no second wake.
                Frame::Datagram(datagram) => {
                    self.datagrams.push_recv(datagram.data);
                    self.events.push(ConnEvent::DatagramReadable);
                }
                Frame::Close(close) => {
                    // §15.2: surface `PeerClosed`, emit **nothing**, hold a
                    // drain for `CLOSE_LINGER`, then drop all state.
                    self.emit_closed(ConnectionLost::PeerClosed {
                        code: close.code,
                        reason: close.reason,
                    });
                    self.lifecycle = Lifecycle::Draining {
                        until: now + constants::CLOSE_LINGER,
                    };
                    self.enter_post_mortem_timers();
                    // Whatever followed the CLOSE in this packet has
                    // nothing left to be applied to.
                    break;
                }
            }
        }

        // **F1:** the two holds a keepalive can be under both lift on a
        // *received* packet, and this packet's **frames** may have lifted
        // either: an ACK covering the probe floor clears a pending mark, a
        // matching `PATH_RESPONSE` disarms the budget. `handle_datagram`
        // synced before the frames were applied, so without this the
        // keepalive would stay unarmed until some later receive — and on a
        // connection that receives nothing further, a lifted hold that never
        // re-arms is a session that goes quiet and dies at `DEAD_TIMEOUT`.
        self.sync_liveness_timer();

        // §16.4's generation order: the events a packet caused, then the
        // packet its arrival made us owe.
        self.drain_events();
        self.pump(now);
    }

    /// §10.4's MAX_STREAMS: monotone-max, and `StreamsAvailable` only when
    /// the limit actually moved — a value at or below the current one is a
    /// valid no-op (§8.4) and must wake nobody.
    fn on_max_streams(&mut self, dir: Dir, max: u64) {
        if self.flow.on_max_streams(dir, max) {
            self.events.push(ConnEvent::StreamsAvailable { dir });
        }
    }

    /// §8.2's semantic class: one CLOSE with the violation's §15.3 code.
    fn kill(&mut self, now: Instant, violation: flow::Violation) {
        let code = violation.code();
        tracing::warn!(
            target: "slither::frames",
            error = %violation,
            code,
            "semantic violation in the frame stream; closing"
        );
        // Ruling 99's *"emits **zero** events"* is delivered by the check
        // **order**, not by discarding afterwards: the limit check runs
        // before the opens it would authorise, so the offending frame never
        // generated one. Whatever earlier frames in the same packet
        // legitimately generated is kept and drained first — §8.2 discards a
        // packet's effects only on the *structural* path.
        self.drain_events();
        self.enter_closing(now, code, b"", ConnectionLost::ProtocolViolation { code });
    }

    /// Move the packet's events into the drain, in generation order.
    fn drain_events(&mut self) {
        for event in self.events.drain(..) {
            self.outputs.push_back(ConnOutput::Event(event));
        }
    }

    /// Apply a received packet to a **closing** or **draining** connection.
    ///
    /// §15.2's retention list is exhaustive — the closing state keeps the
    /// seal capability, the receive cipher states and the replay window,
    /// and every other subsystem "may drop immediately". So there is
    /// nothing for a STREAM, credit or ACK frame to be applied *to*, and
    /// the only thing the frame stream is still read for is the peer's
    /// CLOSE.
    ///
    /// A structural failure here is ignored: we are already dying, with a
    /// code, and a second CLOSE carrying a different one would fight the
    /// reply rule it would have to travel under.
    fn apply_post_mortem(&mut self, now: Instant, received: Received) {
        let peer_closed = match &received {
            Received::Frames(frames) => frames.iter().any(|f| matches!(f, Frame::Close(_))),
            Received::Keepalive | Received::Structural(_) => false,
        };

        let closing = match std::mem::replace(&mut self.lifecycle, Lifecycle::Dead) {
            Lifecycle::Closing(closing) => closing,
            // Draining is reply-free and has the deadline it will die on;
            // §15.2 gives no transition out of it.
            other => {
                self.lifecycle = other;
                return;
            }
        };

        if peer_closed {
            // §15.2: "two closing endpoints go quiet rather than
            // ping-ponging replies at 1 Hz for the linger." The existing
            // deadline stands — see this module's `close` docs.
            self.lifecycle = closing.into_draining();
            return;
        }

        // The packet was authenticated and window-fresh, which is the only
        // thing §15.2 owes a reply to.
        let mut closing = closing;
        let reply = closing.reply(now);
        self.lifecycle = Lifecycle::Closing(closing);

        if let Some(close) = reply {
            // A reply that cannot be sealed is dropped rather than turned
            // into a second death: the connection is already dying with a
            // surfaced cause, and `Closed` is emitted exactly once.
            let _ = self.transmit_close(now, &close);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Teardown
    // ═══════════════════════════════════════════════════════════════════

    /// §15.2's closing state: emit CLOSE, surface `lost`, linger.
    fn enter_closing(&mut self, now: Instant, code: u64, reason: &[u8], lost: ConnectionLost) {
        let close = Close::new(code, reason);

        if self.transmit_close(now, &close).is_err() {
            // §7.9: the only reachable seal failure is nonce exhaustion,
            // and it is terminal. Nothing was transmitted and nothing
            // moved (§16.7), so the connection dies with that cause rather
            // than the one asked for, and with no linger — a linger whose
            // replies cannot be sealed is an empty wait.
            self.die(ConnectionLost::NonceExhausted);
            return;
        }

        self.emit_closed(lost);
        self.lifecycle = Lifecycle::Closing(Closing::new(now, close));
        self.enter_post_mortem_timers();
    }

    /// Seal one CLOSE and queue it for the **session's endpoint address**.
    ///
    /// §15.2: the closing state does not roam, so this is the anchor and
    /// never a triggering packet's source. Sealed `seal_quiet` (§7.4) and
    /// not ack-eliciting (§8.3), so it neither marks the liveness clock nor
    /// arms the death deadline.
    fn transmit_close(&mut self, now: Instant, close: &Close) -> Result<(), session::SealFailed> {
        let Some(session) = self.session.as_mut() else {
            return Err(session::SealFailed);
        };

        let mut packing = Packing::new();
        let fits = packing.control(Frame::Close(close.clone()));
        debug_assert!(
            fits,
            "a CLOSE is at most 260 bytes and MAX_PLAINTEXT is 1170"
        );
        let plaintext = packing.into_plaintext();
        let size = (constants::DATA_HEADER_LEN + plaintext.len() + constants::AEAD_TAG_LEN) as u64;
        // §7.3 binds CLOSE as well, and ruling 171 puts it **last** in the
        // priority order — a scarce budget serves everything else first.
        //
        // A held CLOSE is not lost: §15.2's linger keeps receiving, and its
        // reply rule — *"CLOSE's only reliability mechanism"* — re-sends on
        // the next authenticated, window-fresh packet, which is also the
        // packet that credits the budget. So the hold is a deferral to the
        // very event that clears it, and `Ok` is the honest answer: this is
        // not a seal failure, nothing moved, and the connection must still
        // enter the closing state.
        if !self.amplification.admits(size) {
            return Ok(());
        }

        let sealed = session.seal_quiet(now, &plaintext, false)?;
        let to = session.established().anchor;

        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        self.amplification.on_sent(size);
        Ok(())
    }

    /// A death with no post-mortem: surface it and retire in one drain
    /// (ruling 81).
    ///
    /// The no-linger paths — liveness (§7.4), nonce exhaustion (§7.9),
    /// `Replaced` (§5.4) — transmit nothing (§15.4), so there is no instant
    /// to seal at and this takes no `now`.
    fn die(&mut self, lost: ConnectionLost) {
        self.emit_closed(lost);
        self.drop_state();
    }

    /// Drop all state and emit `Retired`.
    ///
    /// §16.4: *"`Retired` … fires when the connection's state is actually
    /// dropped — not when its death is announced"*, and the shell delivers
    /// it before releasing the connection's bookkeeping, or the index route
    /// and the guard-entry pin leak for the endpoint's life.
    fn drop_state(&mut self) {
        if let Some(session) = self.session.take() {
            let our_index = session.established().our_index;
            self.outputs
                .push_back(ConnOutput::ToEndpoint(ToEndpoint::Retired { our_index }));
        }
        self.lifecycle = Lifecycle::Dead;
        self.timers.disarm_all();
        self.scratch = Vec::new();

        // §15.2: *"all stream, flow-control, recovery and congestion state
        // may drop immediately"*. The recovery half is dropped here
        // unconditionally — a sent map that outlives the session holds
        // `SentPacket`s no ACK can ever arrive for, and §17.5's ceiling
        // assumes it is gone.
        //
        // `streams` and `flow` are **not** dropped here. Ruling 133 splits
        // them by path — the local closer frees them at once, the draining
        // receiver retains them for `CLOSE_LINGER` — and the retaining half
        // is what makes ruling 128's post-death drain implementable. That
        // is 5b's, and freeing them here would break it before it is
        // written. Reported in `IMPLEMENTATION-5a.md`.
        self.recovery = Recovery::new();
        self.congestion = NewReno::new();
        self.ack = AckState::new();

        // §11's send queue joins them: nothing can be sealed from here, so
        // a queued datagram is state with no future. The **receive** queue
        // is deliberately kept — ruling 152 puts `recv_datagram` in ruling
        // 128's post-death drain, on the same terms as `streams` above.
        self.datagrams.discard_send();
    }

    // ═══════════════════════════════════════════════════════════════════
    // Internals
    // ═══════════════════════════════════════════════════════════════════

    fn install(
        &mut self,
        now: Instant,
        session: EstablishedSession<C>,
        role: Role,
        anchor_from_msg1: bool,
    ) {
        self.installed = true;
        self.role = Some(role);
        self.streams.set_role(role);
        self.session = Some(Session::install(now, session));
        // **[RATIFIED 2026/08/16 — ruling 200]** §7.3 arms the budget for an
        // address *"first anchored from a msg1 source"*. That is endpoint
        // knowledge, so the endpoint states it — see `Install`.
        //
        // Keying it on the **constructor** left a hole: §6.6's internal
        // tie-break loser *dialled*, lost, and installs through this path
        // with `anchor: src`, the msg1 source (`endpoint/routing.rs:520`).
        // That peer-supplied address was treated as **validated**, so the 3×
        // cap never applied to it — a reflector on the one path where a
        // dialler adopts an address it did not choose.
        if anchor_from_msg1 && self.amplification.is_validated() {
            let challenge = self.draw_challenge();
            self.amplification = Amplification::arm(challenge, constants::INIT_PACKET_LEN as u64);
            // **[A2]** As in `established`: the endpoint's msg2 went to this
            // anchor before the `Install` arrived, and §7.3 counts total
            // bytes sent. This is §6.6's tie-break loser, which is the other
            // path that writes a msg2 (`endpoint/routing.rs`).
            self.amplification
                .on_sent(constants::RESP_PACKET_LEN as u64);
        }
        self.sync_liveness_timer();
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Established));
        // §16.9: early opens and writes are *ordinary work* that pumps when
        // a session installs. Nothing was emitted before now because there
        // was nothing to seal with.
        self.pump(now);
    }

    /// §8.5's plan-seal-commit, run until nothing more is owed (§16.7).
    ///
    /// # Ruling 98's seal table, which is §7.4's and not the plan's
    ///
    /// | Frame | Seal | Ack-eliciting |
    /// |---|---|---|
    /// | STREAM, **first transmission** | `seal` (marking) | yes |
    /// | STREAM, **retransmission** | `seal_quiet` | yes |
    /// | **RESET_STREAM** | `seal_quiet` | yes |
    /// | MAX_DATA / MAX_STREAM_DATA / MAX_STREAMS_BIDI / MAX_STREAMS_UNI | `seal_quiet` | yes |
    ///
    /// §7.4's *"`seal_quiet` does not touch `last_send` — the **quiet
    /// set**"* sentence enumerates it and RESET_STREAM is **in** it — the
    /// plan's table put it on the marking `seal` "by
    /// omission from §10.3", which is backwards and would have deferred
    /// keepalives forever. `session.rs`'s `seal_quiet` doc comment has
    /// quoted the correct set since slice 3a.
    ///
    /// Marking is a property of the **seal**, not of the frame, so a packet
    /// mixing a fresh STREAM frame with credit frames is marking. That last
    /// sentence is the only part no section states, and it follows from
    /// `seal`/`seal_quiet` being a per-seal choice.
    fn pump(&mut self, now: Instant) {
        self.pump_inner(now, false);
    }

    /// Whether anything is owed on the wire, over **both** stage-3
    /// contributors.
    ///
    /// Written once and used twice, because the two call sites disagree in
    /// opposite directions if they drift: the loop's exit condition would
    /// stop building packets with datagrams still queued, and §14.5's
    /// `app_limited` would stamp *"we stopped because there was nothing more
    /// to send"* on a packet sent while the send queue was full — growing
    /// the congestion window off a sender that is not actually idle.
    fn owes_output(&self) -> bool {
        self.streams.has_output() || self.datagrams.has_send()
    }

    /// [`pump`](Self::pump), optionally owing §13.4's probe.
    ///
    /// The two deadlines §13 owns are re-derived at the end, from the map
    /// rather than from arming sites — see
    /// [`sync_recovery_timers`](Self::sync_recovery_timers).
    fn pump_inner(&mut self, now: Instant, probe: bool) {
        self.pump_packets(now, probe);
        self.sync_recovery_timers();
    }

    /// A packet plan already sized to what §7.3's budget will admit.
    ///
    /// **[ruling 203]** *"Bound the packing target by the remaining budget —
    /// `min(MAX_DATAGRAM, room)` — so the first post-roam packet is small,
    /// ack-eliciting, and validates the address at once."*
    ///
    /// # Why building at full size was wrong
    ///
    /// [`Amplification::admits`] answers about a **finished** candidate, and
    /// a refusal is a *hold*, not a truncation. A pump that plans 1200 bytes
    /// and asks afterwards therefore holds everything until the budget grows
    /// to 1200 — while the budget only grows on received bytes, and on a
    /// connection carried by §7.5's keepalive dance the only received bytes
    /// are 30-byte empty plaintexts. 2 048 bytes of application data then
    /// wait ~20 keepalive rounds (~200 s) for a budget a ~90-byte packet
    /// would have escaped in one round trip.
    ///
    /// **[ruling 208]** *"Ruling 203's sizing fix stands and becomes more
    /// important, not less: the challenge must fit inside the armed budget,
    /// and a pump that cannot shrink cannot emit one."* The escape ruling
    /// 203 described was an ACK covering a floor; it is now a
    /// `PATH_RESPONSE`, and the packet that must be small enough to leave is
    /// the one carrying the `PATH_CHALLENGE` — 39 bytes against the 90 the
    /// smallest arming funds.
    ///
    /// # The units, which are the trap
    ///
    /// **[ruling 207(c)]** [`Amplification::room`] is in **datagram** bytes;
    /// [`Packing`]'s budget is in **plaintext** bytes. They differ by exactly
    /// §3.4's `DATA_HEADER_LEN + AEAD_TAG_LEN` = 30 (ruling 136 charges the
    /// full datagram). Capping the plaintext at the remaining *datagram*
    /// bytes overshoots by 30 and re-refuses the packet it just sized.
    ///
    /// # What this does **not** do
    ///
    /// **[ruling 207(a)]** The predicate does not move. The candidate this
    /// plan produces is still put to [`Amplification::admits`] unchanged, and
    /// no packet is exempted from it — the clamp only means the answer is
    /// now normally *yes*.
    fn packing(&self) -> Packing {
        const OVERHEAD: u64 = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;
        let Some(room) = self.amplification.room() else {
            return Packing::new();
        };
        // `min` before the cast: `room` is a `u64` and `Packing::bounded`
        // clamps to `MAX_PLAINTEXT` anyway, so this is the same value by two
        // routes and cannot truncate.
        let plaintext = room
            .saturating_sub(OVERHEAD)
            .min(constants::MAX_PLAINTEXT as u64) as usize;
        Packing::bounded(plaintext)
    }

    fn pump_packets(&mut self, now: Instant, mut probe: bool) {
        if !self.lifecycle.is_live() || self.session.is_none() {
            // §16.9: *"no frame is emitted before install (nothing sends
            // until a session exists)"*.
            return;
        }

        // §7.3's one-per-pump bound on the `PATH_CHALLENGE` offer, hoisted
        // above the probe because the probe is now the packet that may
        // carry it (**[RATIFIED 2026/08/17 — ruling 250]**).
        //
        // **The 212(c) pre-pass that stood here is gone.** It emitted the
        // path frames in a datagram of their own, *above* the contested
        // probe, under a comment citing ruling 212(c) as live law — and
        // ruling 215 **reversed 212(c) in exactly that half**: §7.3's ranks
        // are 1 CLOSE · 2 contested probe · 3 `PATH_RESPONSE` · 4
        // `PATH_CHALLENGE`, and they *"stand exactly as written"*. (212(c)'s
        // other half — that the early return below is wrong **as written** —
        // 215 kept, and ruling 250 is what finally spends it. 212(d), cited
        // elsewhere in this file, is a different sub-ruling and is live.)
        // Ruling 250 measured what the survival cost: the contested verdict
        // delayed ~10 s against a talking peer and ~15 s against an
        // abandoned one — a delay rather than a kill, which is how it passed
        // a full suite for two rounds.
        let mut owe_challenge = true;

        // **[ruling 171]** Rank 2, immediately under CLOSE. A pending probe
        // the budget cannot admit stops everything **below** it: anything
        // sent underneath would spend budget the probe is waiting for, and
        // the probe's deadline does not exist until it leaves.
        //
        // **[ruling 215]** That is the whole of the early return, and it is
        // the surviving half. An **admitted** probe does not stop the pump:
        // *"the send pump may emit the probe and continue building on the
        // same pass"*. The label on this comment read "Rank 4" until ruling
        // 250 — 212(c)'s rank for a frame this pre-pass no longer emits.
        //
        // **[ruling 250]** `owe_challenge` is threaded in because the
        // probe's own packet may carry the challenge: the pump must not
        // then offer a second one in the same pass.
        if !self.pump_contested_probe(now, &mut owe_challenge) {
            return;
        }

        // Bounded by construction: every iteration that transmits has moved
        // stream bytes out of the pending set, cleared a regenerate
        // identity, packed the owed ACK, or discharged one of §7.3's two
        // path obligations — the response by sending it, the challenge by
        // spending `owe_challenge` — and one that does none of those builds
        // an empty plan and breaks below.
        loop {
            let mut packed = streams::Packed::default();
            // **[ruling 203]** Sized to §7.3's remaining room, not to §8.6's
            // maximum — see [`packing`](Self::packing).
            let mut packing = self.packing();

            // Stage 1 — §12.4: *"An owed ACK rides the next outgoing packet
            // (packing order §8.5)"*.
            let ack_packed = self.pack_ack(now, &mut packing);
            // Stage 2, **first among the control frames** — **[ruling 208,
            // §8.5 as amended]** §7.3's two path frames, ahead of every
            // credit grant, so that in a packet shrunk to the budget's
            // admitted room the frame that **ends** the scarcity is not the
            // one trimmed out of it.
            //
            // The challenge's two gates are §7.3's boundaries, unchanged in
            // force from the PING they replace: one offer per pump
            // (`owe_challenge`), and only while something is actually owed
            // to the address.
            //
            // **[round 41 item 8 — 2026/08/18]** A `contested.is_pending()`
            // disjunct stood here, on the sentence *"a pending contested
            // probe counts as owed — it is output waiting on exactly this
            // budget"*, reported unreachable at the R40-B merge and left in
            // place. It is now **deleted**, because this loop cannot observe
            // that state at all. `pump_contested_probe` runs above it and
            // leaves `Pending` in no case: it returns early when there is no
            // mark, assigns `Armed` when it sends, and returns from the
            // **whole pump** when anything refuses the send. `Pending` has
            // one writer — `mark_contested` — and nothing under the pump
            // calls it, so the state cannot appear mid-loop either.
            //
            // Measured as well as argued, since a green suite that misses a
            // line proves little (rule 13): an assertion on this line over
            // the full suite saw `Armed` 55 times and `Pending` **0** times
            // across >37 000 iterations, while `pump_contested_probe`
            // entered on a mark 49 times and left by exactly those two
            // exits — 17 refused, 32 sent. The deletion is therefore
            // behaviour-preserving, not the behaviour change the report
            // feared. It was already unreachable before ruling 250 (the
            // 212(c) pre-pass ran first, the probe second), so neither the
            // pre-pass's deletion nor its replacement created it.
            //
            // The state that disjunct named is real, and **ruling 250 is
            // where it is served**: the probe's own packet coalesces the
            // owed path frames (`pump_contested_probe` → `pack_path_frames`,
            // 40 B with one owed, 49 B with both), and `owe_challenge` is
            // threaded through that call so this loop does not then offer a
            // second challenge on the same pass.
            //
            // **[ruling 217]** `self.ack.is_owed()` belongs in this
            // disjunction and its absence was the defect. §8.7 owes the
            // challenge *"whenever §7.3's budget admits **a packet**"* — an
            // owed ACK builds one, so the challenge rides it. Without this
            // the commonest post-roam packet in the protocol goes out
            // carrying no challenge at all, draws an ACK that under ruling
            // 208 proves nothing, and the address stays unvalidated: ruling
            // 203's stall, one indirection later.
            //
            // It stops short of manufacturing. An empty keepalive is not
            // ack-eliciting, so it owes no ACK, so no packet is built and no
            // challenge is offered — which is the blind author's
            // `an_idle_unvalidated_connection_owing_nothing_emits_no_challenge`,
            // and the reason this is a disjunct rather than an unconditional
            // `owe_challenge`.
            //
            // **[RATIFIED — ruling 221]** `probe` is a disjunct of `offer`,
            // and its absence was the defect §8.7 names by its own words:
            // ruling 221 calls it *"the fourth"*, which it was until round
            // 41 item 8 deleted the dead `contested.is_pending()` above it —
            // it is now the third of three, and the ruling's ordinal is left
            // quoted rather than silently renumbered. Without
            // it a **lost** challenge is never re-emitted. §13.4 arms the PTO
            // on the challenge packet (it is ack-eliciting), the probe finds
            // nothing else owed, `packing.ping()` below builds a bare PING,
            // and the address stays unvalidated until `DEAD_TIMEOUT` kills
            // the session — *"sent once and lost forever, which §7.3's
            // no-deadlock argument cannot survive"* (§8.7), verbatim and
            // measured.
            //
            // This is the **retransmission** half of the standing obligation,
            // and §8.7 puts it here rather than in loss recovery: neither
            // path frame is ever re-queued by loss detection, so the PTO — a
            // timer the challenge itself arms — is the only thing that can
            // ask again on a connection with nothing else to say. It is what
            // `Amplification`'s own doc already claimed was happening.
            //
            // No livelock: `probe` is set by a firing PTO and cleared after
            // the first iteration below, so this offers one challenge per
            // probe on §13.3's doubling schedule, bounded by `DEAD_TIMEOUT`.
            // It is not the manufacture ruling 217's first draft attempted —
            // the packet exists because the PTO fired, not because the
            // challenge wanted one.
            let offer = owe_challenge && (self.owes_output() || self.ack.is_owed() || probe);
            let path = self.pack_path_frames(&mut packing, offer);
            // Stages 2 and 3 — credit grants and RESET_STREAM, then the
            // STREAM and DATAGRAM fill.
            self.streams
                .pack_control(&mut self.flow, &mut packing, &mut packed);
            // **[ruling 155]** One datagram per packet, packed **before**
            // the stream fill. §8.5 names the two contributors and orders
            // neither, and the three readings differ by which side starves.
            // Appending after `streams.fill(...)` is the smallest diff and
            // the worst outcome: a saturated stream fills all 1170 bytes of
            // every packet, datagrams never go out, and §11.3's bounded
            // queue evicts continuously — **silent data loss, with no
            // counter that distinguishes it from ordinary pressure**.
            // Draining the whole queue instead starves a bulk stream for up
            // to 64 packets. One-per-packet-first is the only order whose
            // starvation is bounded in both directions and statable: a
            // stream waits at most one packet per queued datagram, a
            // datagram at most one packet per predecessor.
            //
            // **[ruling 161]** This decision **never evicts**. `peek_send`
            // does not remove, so a head the packet cannot fit stays
            // queued: §11.3's drop-oldest is `send_datagram`'s discipline,
            // on enqueue, and a packing pass is not queue pressure.
            //
            // The residual, documented rather than engineered away: a
            // **maximum-size datagram is preferentially delayed** by any
            // packet already carrying control frames, because 1169 bytes
            // plus its type byte need the whole plaintext and §8.5 packs
            // the ACK first. It is bounded — §12.4 does not put an ACK on
            // every packet — and the alternative, reordering the queue to
            // fit a smaller datagram first, trades a bounded delay for a
            // silent reordering nobody has ruled on.
            let mut sent_datagram = None;
            if let Some(data) = self.datagrams.peek_send()
                && packing.datagram(data)
            {
                sent_datagram = self.datagrams.pop_send();
            }
            // §7.4:1953-1954 — `seal` marks `last_send` for a packet
            // carrying a first-transmission STREAM frame **or DATAGRAM
            // frame**. Every datagram is a first transmission (§8.7's
            // `never` class), so there is no retransmission case to exclude;
            // a build that sealed these quiet would send keepalives it does
            // not owe.
            let marking = self.streams.fill(&mut packing, &mut packed) | sent_datagram.is_some();
            // Stage 4 — §13.4: *"A firing PTO sends one ack-eliciting
            // packet: pending retransmittable frames oldest-first if any
            // exist, else a bare PING."* The PING is owed only when the
            // first three stages produced nothing that elicits, and — **[
            // ruling 208]** — it is owed **only** for the PTO now. A PTO
            // probe is about loss detection; address validation is the
            // challenge's job, and merging the two would put a PING where a
            // challenge belongs.
            //
            // **This is where ruling 207(b)'s question was answered with "a
            // PING", and where ruling 208 changes the answer to "a
            // `PATH_CHALLENGE`".** Sizing alone was never the fix: a packet
            // shrunk to fit the budget is useless if what fits cannot end
            // the unvalidated state. Under ruling 168 that meant *cannot
            // elicit an ACK*; under ruling 208 an ACK validates **nothing**,
            // so a build that left `packing.ping()` serving both branches
            // would reproduce ruling 203's stall exactly — the pump shrinks
            // a packet, the packet elicits, the ACK arrives, and the address
            // stays unvalidated forever. §7.3's no-deadlock argument is a
            // claim about the **sender** — *"the budget always admits
            // something … and what it admits is enough to elicit the
            // `PATH_RESPONSE` that ends it"* (§7.3, *disarming*) — and only
            // the stage-2 packing above can make it true: `Packing` can
            // shrink a packet but cannot make one carry a challenge.
            //
            // **The challenge's four boundaries, stated because rule 8 reads
            // a construction's scope as exhaustive whether or not it says
            // so.** They are the PING's, unchanged in force:
            //
            // 1. **Nothing is owed *and no packet is being built*.** An idle
            //    unvalidated connection sends no challenge. Validation is
            //    not a goal in itself — it is what lets held output leave,
            //    and there is none. A challenge here would be an unprompted
            //    probe train on every pump, and §7.5's keepalive dance
            //    already carries liveness.
            //
            //    **[ruling 221]** The second half of that condition is not
            //    decoration. A connection with an **unanswered challenge**
            //    outstanding is not idle in the sense that matters: the
            //    challenge packet is ack-eliciting, so §13.4 armed a PTO on
            //    it, and when that fires a packet *is* being built and the
            //    challenge rides it. The boundary is against **manufacture**
            //    — building a packet for the challenge — which is what
            //    ruling 217's first draft did and what the blind author's
            //    `an_idle_unvalidated_connection_owing_nothing_emits_no_challenge`
            //    forbids. It was never a boundary against riding a carrier
            //    something else created.
            // 2. ~~**A bare ACK with nothing else owed.**~~ **[REVERSED by
            //    ruling 217; the text stood arguing the old position until
            //    ruling 224 read it.]** This boundary was written at
            //    `522bcdf` for ruling 203's **PING**, whose boundaries are
            //    genuinely these, and was carried onto the challenge without
            //    being re-derived against §8.7. It is now wrong: §8.7 owes
            //    the challenge *"whenever §7.3's budget admits a packet"*,
            //    an owed ACK builds one, and `ack.is_owed()` is a disjunct
            //    of `offer` above. Every accepted connection begins
            //    unvalidated, so this is the **commonest** post-roam packet
            //    in the protocol, not an edge.
            //
            //    Its objection was accurate and is simply outranked: the
            //    delayed ACK does become ack-eliciting, does enter the sent
            //    map, and does spend congestion window. §8.7 is ratified
            //    text and this comment was not. What the objection *did*
            //    predict correctly is the fixture cost — S12 counts
            //    blackholed datagrams as PTO probes and ruling 223 had to
            //    quiesce path validation before its window opens.
            // 3. **§14.5's window.** The challenge is **not** exempt from
            //    the congestion gate below. §14.5 enumerates its exemptions
            //    — PTO probes, the contested probe, non-ack-eliciting
            //    control packets — and the path frames are not among them;
            //    rule 8 reads that list as closed. A challenge the window
            //    refuses is carried by §13.4's PTO, which *is* exempt and
            //    packs stage 2 like any other packet, so the escape survives
            //    a full window.
            //
            //    **[ruling 250]** One qualification, and it is about the
            //    *carrier*, not about the frame: a challenge riding the
            //    **contested probe's** packet is exempt, because §14.5's
            //    exemption now covers *"the probe's packet as built"* —
            //    *"gating the merged packet would starve the challenge at
            //    collapsed cwnd exactly where a roam makes it owed."* The
            //    challenge has still won no exemption of its own; it has
            //    boarded one, exactly as it does on the PTO probe.
            //
            //    **[ruling 221]** This sentence was written as a statement
            //    of fact and was **not one until ruling 221**: the PTO
            //    packed stage 2 with `offer` false, so the probe carried a
            //    bare PING and the escape it describes did not exist. It is
            //    true now. Worth leaving visible — a boundary comment
            //    asserting a mechanism is a claim about the code beside it
            //    (working rule 11), and this one went unchecked through two
            //    rulings and thirty blind tests.
            // 4. **The seal.** A challenge is never marking, and needs no
            //    rule to make it so: every marking contributor (a
            //    first-transmission STREAM frame, a DATAGRAM) is a stage-3
            //    frame, so `marking` is false on a packet the challenge
            //    carries alone and the packet is sealed `seal_quiet`. That
            //    is §7.5's contested-probe treatment verbatim — *"not fresh
            //    application intent, so it does not move `last_send` and
            //    cannot suppress a keepalive"* — and it still arms the death
            //    clock, because §7.3 has *"any ack-eliciting output we aim
            //    at the address arms the death clock by itself, even where
            //    nothing marking is sent"* (§7.3, *disarming*; §7.4), and
            //    `PATH_CHALLENGE` is itself ack-eliciting. A new address
            //    that never answers therefore still kills the session at
            //    `DEAD_TIMEOUT`, with no new timer and no new variant.
            //
            // It is self-limiting rather than rate-limited: a challenge that
            // lands is answered by a `PATH_RESPONSE` that disarms the
            // budget, so the whole mechanism costs at most one round trip.
            // `owe_challenge` bounds it to one per pump so a large-but-shrunk
            // room cannot spend itself on a burst of challenges.
            let elicits = frame::packet_is_ack_eliciting(packing.frames());
            if probe && !elicits {
                packing.ping();
            }

            if packing.frames().is_empty() {
                break;
            }

            let ack_eliciting = frame::packet_is_ack_eliciting(packing.frames());
            let frames = packed.sent_frames();
            let plaintext = packing.into_plaintext();
            // **[ruling 136]** The candidate's size is the **full
            // datagram**, because §14.5 derives `INITIAL_WINDOW` at
            // `MAX_DATAGRAM` = 1200 and `MAX_DATAGRAM` is the datagram: a
            // window expressed in datagram units is spent in datagram
            // units.
            let size =
                (constants::DATA_HEADER_LEN + plaintext.len() + constants::AEAD_TAG_LEN) as u64;

            // §7.3's anti-amplification budget, **outside** the `!probe`
            // guard below and outside the `ack_eliciting` one: it *"binds
            // all output … explicitly including the §14.5 and §13.4
            // congestion-window exemptions"*, because *"those exemptions are
            // scoped to cwnd, never to this budget."*
            //
            // The datagram is **held** — not dropped, not truncated, not an
            // error. Nothing was sealed, so §16.7's *"on seal failure
            // nothing moved"* holds here too, and the next credited receive
            // re-plans it.
            if !self.amplification.admits(size) {
                self.streams.restore(&mut packed);
                if let Some(data) = sent_datagram.take() {
                    self.datagrams.unpop_send(data);
                }
                break;
            }

            // §14.5's admission gate. **Exemptions, exhaustively**: PTO
            // probes (§13.4 — a black-holed path with a full window must
            // stay probeable); the contested-connection probe (§7.5, sent
            // above) — **[ruling 250]** *"the exemption covers the probe's
            // packet **as built**"*, so the path frames it coalesces are
            // exempt too, and no packet built *here* inherits that; and
            // non-ack-eliciting control packets, which are never tracked in
            // flight and never gated.
            //
            // The exemption is from **admission only** — the probe is still
            // recorded below and still counts in `bytes_in_flight` (ruling
            // 43, §17.5), or loss recovery would hold a packet in flight it
            // could not see.
            if ack_eliciting && !probe && !self.admits(size) {
                // §14.5 gates the *send*: the frames stay pending and are
                // re-planned when the window opens. Nothing was sealed, so
                // §16.7's "on seal failure nothing moved" holds here too.
                self.streams.restore(&mut packed);
                // The datagram goes back to the **front**: §11.3's eviction
                // is drop-oldest, so returning it to the back would let a
                // repeatedly-refused datagram age to the head of the
                // eviction order and be dropped ahead of newer ones.
                if let Some(data) = sent_datagram.take() {
                    self.datagrams.unpop_send(data);
                }
                // A pure ACK is not gated. If one was owed it still goes
                // out, alone, rather than waiting on a window it does not
                // consume.
                if self.ack.is_owed() {
                    self.transmit_pure_ack(now);
                }
                break;
            }

            // §14.5's `app_limited`, recorded at send time by **us** so a
            // peer's ACK-timing games cannot un-set it.
            //
            // **[ruling 139(c)]** It is stamped on the packet that emptied
            // the queue **while headroom remained** — we stopped because
            // there was nothing more to send, not because the window
            // closed. The alternative (only packets sent after the sender
            // has already gone idle) lets a bulk sender holding the queue
            // one packet ahead grow the window while effectively idle,
            // which is the case §14.5 reasons about.
            let app_limited = !self.owes_output()
                && self.recovery.bytes_in_flight().saturating_add(size) < self.congestion.window();

            let Some(session) = self.session.as_mut() else {
                if let Some(data) = sent_datagram.take() {
                    self.datagrams.unpop_send(data);
                }
                break;
            };
            // §7.4's quiet set: retransmissions, credit frames,
            // RESET_STREAM, pure ACKs and — §13.4 — PTO probes. Marking is
            // a property of the **seal**, not of the frame, so a packet
            // mixing a fresh STREAM frame with credit frames is marking.
            let sealed = if marking && !probe {
                session.seal(now, &plaintext, ack_eliciting)
            } else {
                session.seal_quiet(now, &plaintext, ack_eliciting)
            };
            let sealed = match sealed {
                Ok(sealed) => sealed,
                Err(_) => {
                    // §7.9: the only reachable seal failure is nonce
                    // exhaustion, and it is terminal. §16.7's *"on seal
                    // failure nothing moved"* still holds for the queue,
                    // even though a dead connection will never drain it.
                    if let Some(data) = sent_datagram.take() {
                        self.datagrams.unpop_send(data);
                    }
                    self.die(ConnectionLost::NonceExhausted);
                    return;
                }
            };
            debug_assert_eq!(
                sealed.datagram.len() as u64,
                size,
                "ruling 136: the gate's candidate size is the datagram it admitted"
            );
            let to = session.established().anchor;
            self.outputs.push_back(ConnOutput::Transmit(Transmit {
                to,
                data: sealed.datagram,
            }));
            self.amplification.on_sent(size);
            self.sync_liveness_timer();

            if ack_packed {
                self.ack.on_ack_packed();
                self.timers.disarm(TimerKind::AckDelay);
            }

            // §13.5: *"Non-ack-eliciting packets are never inserted."*
            if ack_eliciting {
                self.recovery.on_sent(SentPacket {
                    counter: sealed.counter,
                    time_sent: now,
                    size,
                    app_limited,
                    // **[ruling 137]** Live since slice 7: §14.6's stamp,
                    // taken at seal time. Ruling 172's 2/2 split puts the
                    // RTT sample and the persistent-congestion walk on it.
                    path_gen: self.recovery.path_gen(),
                    frames,
                });
                self.congestion.on_sent(now, size);
            }

            // §13.4: **one** ack-eliciting packet per firing.
            probe = false;
            // **[ruling 208]** The obligations discharge **on the send**,
            // never on the plan: everything above this point restores what
            // it packed when the packet is refused, and these two are no
            // different.
            //
            // The response is discharged by one emission (§8.7). If it is
            // lost the peer's still-standing challenge asks again, and this
            // field is overwritten by whichever challenge is newest — never
            // queued.
            //
            // The challenge is **not** discharged by its emission: it stands
            // for as long as the arming does, and is re-offered by the next
            // pump. What clears here is only the one-per-pump bound, and it
            // clears on the challenge going out rather than on **any**
            // ack-eliciting packet — the pre-208 rule, which was right when
            // an ACK was the proof and is wrong now that one proves nothing.
            if path.response.is_some() {
                self.owed_path_response = None;
            }
            if path.challenge {
                owe_challenge = false;
            }

            if !self.owes_output() && !self.ack.is_owed() {
                break;
            }
        }

        // **[RATIFIED 2026/08/16 — ruling 217]** The challenge is owed by
        // the **arming**, not by having something else to send.
        //
        // Both blind agents reported this boundary as unresolved and they
        // disagreed on it, which is the split working. The implementer kept
        // the PING's boundary 1 — *nothing owed ⇒ no challenge* — carried
        // over from `CONTRACT-7b.md` §1.4. The test author wrote to §8.7's
        // *"whenever §7.3's budget admits a packet and the address is still
        // unvalidated"* and pinned the roam itself emitting one.
        //
        // §8.7 wins, on working rule 3's tiebreak: §7.3's no-deadlock proof
        // depends on it. Boundary 1 was sound under ruling 168, where any
        // ack-eliciting packet drew the ACK that validated — so waiting for
        // real output cost nothing. Under ruling 208 an ACK proves
        // **nothing**, and the only packet that can validate is one we
        // choose to send. Deferring it until output exists means the output
        // arrives into a budget still capped at 3× the roaming packet and
        // pays a round trip to escape: **ruling 203's stall, one
        // indirection later**, which is precisely what the test author
        // named.
        //
        // The implementer's own counter-argument does not survive its own
        // code: it feared *"an unprompted probe train on every pump"*, but
        // `owe_challenge` is cleared the moment the challenge is sealed, so
        // an arming emits exactly **one** — 39 bytes against the 90 B floor
        // the smallest arming funds, and against msg2's 107 on the accept
        // path. There is no train to fear.
        // **[ruling 217, amended]** A *response* we owe is manufactured;
        // a *challenge* is not.
        //
        // The first draft of 217 manufactured a packet for the challenge
        // too, and was wrong twice. It hung the suite — `owe_challenge` is a
        // local, `true` on every entry, so every pump emitted a fresh
        // dedicated packet and nothing driving a pair to quiescence ever
        // reached it. And the blind test author had already forbidden it in
        // writing: §8.7 owes the challenge *"whenever §7.3's budget admits
        // **a packet**"*, which is a rule about packets being built and
        // **does not ask the pump to manufacture one**.
        //
        // Its two tests are consistent and I misread them as a conflict:
        // the roam that must carry a challenge is ack-eliciting, so a reply
        // packet exists for it to ride; the roam that must not is an empty
        // keepalive, where no packet is built at all. What 217 correctly
        // settles is the *ride-along* — §8.7's condition, not the
        // contract's `!elicits`.
        if self.owed_path_response.is_some() {
            self.pump_path_frames(now);
        }
    }

    /// §8.5 stage 2, first among the control frames — §7.3's two path
    /// frames, **[ruling 208]**.
    ///
    /// Written once and called from both emission sites (the pump's loop and
    /// [`pump_path_frames`](Self::pump_path_frames)) so what is owed, and in
    /// what order, is stated in one place. `challenge` is the caller's
    /// answer to §7.3's boundary conditions; the response has none — it is
    /// owed unconditionally on receipt (§8.4).
    ///
    /// Nothing is discharged here. The obligations clear when the packet
    /// carrying them is **sent**, because every plan this participates in
    /// can still be refused by the budget or the window.
    fn pack_path_frames(&mut self, packing: &mut Packing, challenge: bool) -> PathPacked {
        let mut packed = PathPacked::default();
        // Answering an obligation before raising one — §7.3: *"between
        // themselves the order is free"*, and this is the conventional
        // reading.
        //
        // **[ruling 250]** The sentence that stood here — *"they never
        // contend: 14 B of header + 18 B of frames + a 16 B tag = 48 B,
        // inside the 90 B floor the smallest arming funds"* — is a universal
        // over **armed** budgets, and by the time this runs the budget has
        // been spent down by whatever left since. It is not a licence to
        // skip the room check, and none is skipped: each verb below refuses
        // its frame when nine bytes do not remain, and the caller keeps
        // owing what it could not pack. §7.3 now states the guarantee
        // against the **remaining** room at pump time.
        if let Some(value) = self.owed_path_response
            && packing.path_response(value)
        {
            packed.response = Some(value);
        }
        if challenge
            && let Some(value) = self.amplification.outstanding_challenge()
            && packing.path_challenge(value)
        {
            packed.challenge = true;
        }
        packed
    }

    /// One packet carrying nothing but §7.3's path frames, for the one state
    /// left in which the pump's loop cannot carry them: an owed
    /// `PATH_RESPONSE` the loop planned and a gate then refused
    /// (**[ruling 217]**, and the call at the end of
    /// [`pump_packets`](Self::pump_packets) is the only one). The loop's own
    /// packet may be large enough for §14.5's window to refuse it while
    /// admitting these 39 bytes; a *response* we owe is manufactured, a
    /// *challenge* is not.
    ///
    /// **[RATIFIED 2026/08/17 — ruling 250]** The **other** call site is
    /// gone. It was ruling 212(c)'s pre-pass — a dedicated datagram emitted
    /// *above* a pending contested probe — and ruling 215 reversed 212(c) in
    /// that half. The frames it carried now ride the probe's own packet
    /// ([`pump_contested_probe`](Self::pump_contested_probe)): one packet,
    /// one counter, one sent-map entry.
    ///
    /// Returns nothing. It reported *"whether a challenge went out"* so the
    /// deleted pre-pass could clear the pump's one-per-pump bound before the
    /// loop ran; the surviving call site runs **after** the loop, so there is
    /// nothing left to tell, and a return value no caller can act on is a
    /// mechanism that reads as live and is not.
    ///
    /// Sealed `seal_quiet` and **counted** in the sent map: the frames are
    /// ack-eliciting (§8.3) and §14.5's exemption list — PTO probes, the
    /// contested probe *and the packet it builds* (ruling 250),
    /// non-ack-eliciting control packets — does not name a path-frame packet
    /// of its own, so this one is gated by the congestion window as well as
    /// by the budget.
    fn pump_path_frames(&mut self, now: Instant) {
        let mut packing = self.packing();
        let packed = self.pack_path_frames(&mut packing, true);
        if packed.response.is_none() && !packed.challenge {
            return;
        }
        let plaintext = packing.into_plaintext();
        let size = (constants::DATA_HEADER_LEN + plaintext.len() + constants::AEAD_TAG_LEN) as u64;
        if !self.amplification.admits(size) || !self.admits(size) {
            // Held, not dropped: both obligations still stand and the next
            // pump re-offers them.
            return;
        }

        let Some(session) = self.session.as_mut() else {
            return;
        };
        // §7.4's quiet set: a challenge is not fresh application intent, so
        // it neither moves `last_send` nor suppresses a keepalive. It **is**
        // ack-eliciting, which arms the death clock on the answer.
        let sealed = match session.seal_quiet(now, &plaintext, true) {
            Ok(sealed) => sealed,
            Err(_) => {
                self.die(ConnectionLost::NonceExhausted);
                return;
            }
        };
        let to = session.established().anchor;
        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        self.amplification.on_sent(size);
        self.sync_liveness_timer();
        self.recovery.on_sent(SentPacket {
            counter: sealed.counter,
            time_sent: now,
            size,
            app_limited: false,
            path_gen: self.recovery.path_gen(),
            // §8.7: neither frame is ever re-queued by loss detection. The
            // challenge's reliability is the standing obligation above; the
            // response's is the peer's standing challenge.
            frames: Vec::new(),
        });
        self.congestion.on_sent(now, size);
        if packed.response.is_some() {
            self.owed_path_response = None;
        }
    }

    /// §8.5 stage 1 — the owed ACK, derived from §7.2's window (§12.2).
    ///
    /// `false` when none is owed, when nothing has been received, or when
    /// not even the first block fits the packet — in which case the ACK
    /// **stays owed** and rides the next one.
    fn pack_ack(&mut self, now: Instant, packing: &mut Packing) -> bool {
        if !self.ack.is_owed() {
            return false;
        }
        let ack_delay_us = self.ack.ack_delay_us(now);
        let Some(window) = self.session.as_ref().map(Session::replay) else {
            return false;
        };
        let Some(frame) = ack::derive(window, ack_delay_us, packing.room()) else {
            return false;
        };
        packing.ack(frame)
    }

    /// §12.4: *"if none is pending, a standalone ACK packet is generated."*
    ///
    /// Sealed `seal_quiet` (§7.4), **not** ack-eliciting — §12.4 says so in
    /// terms, "no ACK-of-ACK loops" — never tracked for loss, and it
    /// bypasses the congestion window (§14.5's third exemption).
    fn transmit_pure_ack(&mut self, now: Instant) {
        // **[ruling 203]** Sized to §7.3's room like the pump's own plan.
        // Ruling 171 ranks pure ACKs **above** retransmissions and new
        // application data under a scarce budget, so this is the one packet
        // class that must not be the one left un-shrunk: a full-size ACK the
        // room refuses stays owed while the data it outranks has already been
        // sized down and left. `ack::derive` truncates newest-first, so a
        // shrunken ACK drops the oldest ranges — *"exactly the ones prior
        // ACKs most likely already carried"* (§12.2) — and drops the frame
        // entirely, leaving it owed, when not even the first block fits.
        let mut packing = self.packing();
        if !self.pack_ack(now, &mut packing) {
            return;
        }
        let plaintext = packing.into_plaintext();
        let size = (constants::DATA_HEADER_LEN + plaintext.len() + constants::AEAD_TAG_LEN) as u64;
        // §7.3 binds pure ACKs too — §14.5's third exemption is from the
        // congestion window and from nothing else. The ACK stays **owed**,
        // so it rides the next packet the budget does admit.
        if !self.amplification.admits(size) {
            return;
        }

        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Ok(sealed) = session.seal_quiet(now, &plaintext, false) else {
            self.die(ConnectionLost::NonceExhausted);
            return;
        };
        let to = session.established().anchor;
        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        self.amplification.on_sent(size);
        self.ack.on_ack_packed();
        self.timers.disarm(TimerKind::AckDelay);
        self.sync_liveness_timer();
    }

    /// §14.5's admission gate: `bytes_in_flight + candidate_size <= cwnd`.
    ///
    /// **`<=`, not `<`.** At `cwnd = 12 000` and 1200-byte datagrams the
    /// difference is nine admitted packets against ten.
    fn admits(&self, size: u64) -> bool {
        self.recovery.bytes_in_flight().saturating_add(size) <= self.congestion.window()
    }

    /// Re-derive the `Liveness` deadline and §7.5's two keepalives from
    /// §7.4's clocks.
    ///
    /// None of the three is stored twice: each is a function of
    /// `last_authenticated_recv`, `last_send` and the arming flag, and this
    /// is the one place the timer table is told about any of them. They are
    /// re-derived **together** because they read the same two clocks, and a
    /// build that synchronised one without the others would arm a keepalive
    /// against a `last_send` that had already moved.
    ///
    /// # §7.5's passive rule
    ///
    /// With `S = last_send` (the **marking** clock) and
    /// `R = last_authenticated_recv`: arm `Keepalive` at
    /// `S + KEEPALIVE_TIMEOUT` **iff `R > S`**.
    ///
    /// **[ruling 182]** `S` counts **marking sends only**, which is §7.4's
    /// formal definition and *not* §7.5's prose *"has not sent"*. This is
    /// the first case in this project where the formal rule held the intent
    /// and the prose held the bug, and the reason is that the beacon's
    /// soundness proof rests on *"every send that can establish `S > R` is a
    /// marking send, so the death clock is armed there"* — which the prose
    /// reading collapses into an immortal half-open session. A `seal_quiet`
    /// send — a PTO probe, a credit frame, a retransmission, the contested
    /// PING — therefore neither advances `S` nor suppresses the keepalive.
    ///
    /// # §7.5's beacon
    ///
    /// Arm `PersistentKeepalive` at `S + interval`. It re-arms from every
    /// marking send, is **not** reset by receives, and fires
    /// **unconditionally** — it does not consult `R`.
    ///
    /// # The connection that emits nothing
    ///
    /// A connection with **no authenticated receive since install** has
    /// `S == R` at the install (§7.4 pins both clocks there), so `R > S` is
    /// false from the start: it transmits nothing and dies at
    /// install + `DEAD_TIMEOUT`. That is ruling 39's *"a connection with no
    /// authenticated receive since install dies in silence"*, delivered by
    /// the predicate rather than by a special case.
    ///
    /// # Neither keepalive is armed while one cannot leave (**F1**)
    ///
    /// Both keepalive deadlines are functions of `last_send`, and
    /// [`transmit_keepalive`](Self::transmit_keepalive) **returns without
    /// moving `last_send`** when §7.3's budget or a pending contested mark
    /// holds the packet. Arming from `last_send` regardless put the deadline
    /// at an instant already passed: the driver's `sleep_until` completed
    /// immediately, `handle_timeout` re-fired the same timer, and the actor
    /// spun at 100 % of one core — invisible on the wire (ruling 141) and
    /// unreachable from `FlakyWire`, which models a network and not a CPU.
    ///
    /// **Arming nothing is right and arming later is wrong.** Both holds
    /// lift only on a **received** packet — the budget grows on `on_recv`,
    /// the mark clears on an ACK covering its floor — so no future instant
    /// is predictable and there is no correct deadline to arm. Every receive
    /// path ends in this function, so the timer is restored on the one event
    /// that can lift the hold. That is *held, not dropped* expressed in the
    /// timer table, and the resulting state is a connection that announces
    /// `Timeout(None)` and parks: **quiet, not immortal**.
    ///
    /// [`TimerKind::Liveness`] is deliberately **not** suppressed. The death
    /// clock keeps whatever `Liveness::deadline()` returns; suppressing it
    /// too would turn a spinning connection into an immortal one, which is
    /// the collapse ruling 182's beacon proof warns about and is strictly
    /// worse than the spin.
    ///
    /// # The vetoed keepalive arms the death clock (**[ruling 265]**)
    ///
    /// **The paragraph above is true only where `Liveness::deadline()` is
    /// `Some`, and that scope went unstated for a slice** — working rule 8's
    /// exact shape, *a stated construction with an unstated scope*. `armed`
    /// is cleared by every authenticated, window-fresh receive, and
    /// `deadline()` returns `None` while it is clear (§7.4, `session.rs`).
    /// *Not suppressing* a `None` suppresses nothing.
    ///
    /// So the suppression above has a hole, and it is the one §7.5's own
    /// soundness proof forbids. §7.4's death predicate has **two** conjuncts
    /// — `now − last_authenticated_recv >= DEAD_TIMEOUT` **and** *"at least
    /// one **arming** send has occurred since that last authenticated
    /// receive"* — and the second is sound only because §7.5 promises an
    /// arming send within `KEEPALIVE_TIMEOUT` of every receive. Ruling 182
    /// states the promise as *"every send that can establish `S > R` is a
    /// marking send, so the death clock is armed there"*. That quantifies
    /// over **sends**, and says nothing about a keepalive that is *owed and
    /// never sent*: the very receive that sets the passive debt clears
    /// `armed`, so the keepalive is owed **precisely** in the window where
    /// the death clock is disarmed — and [`keepalive_can_leave`] can veto
    /// it. Measured on a roamed connection whose budget one pure ACK
    /// exhausts: `Timeout(None)`, **no timer armed at all**, alive and
    /// silent at 60 s — 2.4 × `DEAD_TIMEOUT`. That is the immortal half-open
    /// session, reached through a door ruling 182 did not enumerate.
    ///
    /// **The fix is an anchor, not a new timer.** A keepalive of either kind
    /// that is owed and vetoed *is* the arming event §7.4's second conjunct
    /// is waiting for, so the death clock is announced in its place, at
    /// `last_authenticated_recv + DEAD_TIMEOUT` — exactly what
    /// `Liveness::deadline()` would return were `armed` set.
    ///
    /// **Why the backstop cannot misfire.** Both disjuncts of
    /// [`keepalive_can_leave`](Self::keepalive_can_leave) lift *only* on an
    /// authenticated, window-fresh receive (the budget grows on `on_recv`,
    /// ruling 169; the mark clears on an ACK covering its floor), and such a
    /// receive **moves `last_authenticated_recv`** — so this deadline slides
    /// with it and can fire only on a connection that has genuinely stopped
    /// hearing from its peer. It is ruling 249's principle read in the other
    /// direction: 249 withheld an announcement for output that cannot leave;
    /// this supplies one for a *hold* that nothing else would ever end.
    ///
    /// **The second disjunct is provably redundant and is written anyway.**
    /// `armed` is set by any marking **or** ack-eliciting send while the
    /// passive debt is cleared **only** by a marking one, so after a receive
    /// the state is `!armed && debt`, after an ack-eliciting quiet send
    /// `armed && debt`, after a marking send `armed && !debt` — hence
    /// `!armed` **implies** `debt` everywhere except §7.4's install pin
    /// (`armed && !debt`). The `or_else` arm therefore only ever runs with
    /// the debt already set, and naming the beacon costs nothing. It is
    /// named because ruling 265 is stated over *"a keepalive, active or
    /// passive"* and a reader must not have to re-derive the implication to
    /// see that the code covers both.
    fn sync_liveness_timer(&mut self) {
        if !self.lifecycle.is_live() {
            return;
        }
        let clocks = self.session.as_ref().map(Session::liveness).copied();
        let can_leave = self.keepalive_can_leave();

        // **[RATIFIED 2026/08/18 — ruling 265]** The backstop. `or_else`, so
        // an armed clock is untouched and only the `None` this rule exists
        // for is filled.
        let owed = clocks.is_some_and(|l| l.owes_passive_keepalive())
            || self.persistent_keepalive.is_some();
        let deadline = clocks.and_then(|liveness| liveness.deadline()).or_else(|| {
            clocks
                .filter(|_| owed && !can_leave)
                .map(|l| l.last_authenticated_recv() + constants::DEAD_TIMEOUT)
        });
        self.timers.set(TimerKind::Liveness, deadline);

        // Ruling 195: the flag, not `R > S`. The comparison is false when
        // the two instants coincide — which the driver's once-per-turn
        // `now()` makes ordinary — and a receive that cannot bootstrap the
        // dance leaves a connection that neither talks nor dies.
        let passive = clocks.filter(|l| l.owes_passive_keepalive() && can_leave);
        self.timers.set(
            TimerKind::Keepalive,
            passive.map(|l| l.last_send() + constants::KEEPALIVE_TIMEOUT),
        );

        let beacon = self.persistent_keepalive.filter(|_| can_leave);
        self.timers.set(
            TimerKind::PersistentKeepalive,
            clocks
                .zip(beacon)
                .map(|(liveness, interval)| liveness.last_send() + interval),
        );
    }

    /// Whether §7.5's keepalive can leave right now (**F1**).
    ///
    /// [`transmit_keepalive`](Self::transmit_keepalive)'s guard and
    /// [`sync_liveness_timer`](Self::sync_liveness_timer)'s arming condition
    /// are the same question, and a build that states it twice is the build
    /// that drifts — the drift here is a timer armed in the past, which is a
    /// spin rather than a wrong packet.
    fn keepalive_can_leave(&self) -> bool {
        let size = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;
        !self.contested.is_pending() && self.amplification.admits(size)
    }

    /// §7.5's keepalive, re-checked against what the rest of the evaluation
    /// already sent (§16.5, ruling 174).
    ///
    /// *"`PersistentKeepalive` is evaluated last: any marking send the
    /// instant produced re-arms it, so it does not fire redundantly."* The
    /// same holds for the passive keepalive from the other side — a marking
    /// send at this instant is `S = now`, which makes `R > S` false and
    /// **is** the thing the keepalive would have been sent to do.
    ///
    /// So a keepalive is owed only if nothing marking left in this
    /// evaluation, and — for the passive one — only if §7.5's predicate
    /// still holds. A session already collected for teardown owes none
    /// either, which [`transmit_keepalive`](Self::transmit_keepalive)'s own
    /// liveness guard delivers.
    fn transmit_keepalive_if_owed(&mut self, now: Instant, passive: bool) {
        let Some(liveness) = self.liveness().copied() else {
            return;
        };
        if liveness.last_send() >= now {
            // A marking send at this instant has done the job and re-armed
            // both timers through `sync_liveness_timer`.
            return;
        }
        // The beacon fires **unconditionally** — it does not consult `R` —
        // so reaching here at all is enough for it. The passive rule
        // re-checks its own predicate.
        if !passive || liveness.owes_passive_keepalive() {
            self.transmit_keepalive(now);
        }
        // Whether or not one went out, the two deadlines are derived state
        // and `take_due` disarmed them: re-derive, or a skipped keepalive is
        // never re-armed.
        self.sync_liveness_timer();
    }

    /// §7.5's keepalive: §3.4's **empty plaintext**, sealed **marking**.
    ///
    /// A 16-byte tag-only ciphertext — a **30-byte datagram** — carrying no
    /// frames at all, so it bypasses §8's layer entirely. It never enters
    /// the sent map, is never ack-eliciting, never occupies the congestion
    /// window (§8.7, §14.5) and is **never retransmitted**. It is admitted
    /// to the peer's replay window and appears opportunistically in ACK
    /// ranges.
    ///
    /// Serves both timers: §7.5 gives them one action and distinguishes them
    /// only by when they arm.
    fn transmit_keepalive(&mut self, now: Instant) {
        if !self.lifecycle.is_live() {
            return;
        }
        let size = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;
        // §7.3 binds **all** output, and a keepalive sits at rank 7 in
        // §7.3's order — below CLOSE, both path frames, a pending contested
        // probe, pure ACKs and PTO probes.
        //
        // **F1:** this returns **without moving `last_send`**, which is
        // exactly why `sync_liveness_timer` may not arm from `last_send`
        // without asking the same question. One predicate, two callers.
        if !self.keepalive_can_leave() {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let sealed = match session.seal(now, &[], false) {
            Ok(sealed) => sealed,
            Err(_) => {
                self.die(ConnectionLost::NonceExhausted);
                return;
            }
        };
        debug_assert_eq!(
            sealed.datagram.len() as u64,
            size,
            "§3.4: the empty plaintext is a 30-byte datagram"
        );
        let to = session.established().anchor;
        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        self.amplification.on_sent(size);
        // Re-derives both keepalives from the `last_send` this seal just
        // moved: the beacon re-arms one interval on, and the passive rule
        // goes false because `R > S` no longer holds.
        self.sync_liveness_timer();
    }

    /// §7.5's contested mark, taken by §6.4's refusal against a `None`
    /// basis (ruling 36).
    ///
    /// # The four cases, exhaustively
    ///
    /// | current state | effect |
    /// |---|---|
    /// | `No`, connection `Live` | record the floor, state ⇒ `Pending`, trace. **No PING, no timer, no event.** |
    /// | `No`, **closing or draining** | **total no-op** (ruling 179) |
    /// | `Pending` | **total no-op** — floor unchanged |
    /// | `Armed` | **total no-op** — floor unchanged, **deadline NOT re-armed** |
    ///
    /// The `Armed` row is a **security property**, not an optimisation:
    /// re-arming on a second refusal would hand the attacker — who supplies
    /// the `Intro`s — a way to postpone the verdict for ever.
    ///
    /// **[ruling 175]** There is **no cooldown**, and every re-mark records a
    /// **fresh** floor. A live peer ACKs in ~1 RTT and clears the mark, so
    /// the next refusal is a full second mark; ruling 43's *"one probe per
    /// `KEEPALIVE_TIMEOUT`"* is superseded. The honest bound is *"at most one
    /// probe per mark, at most one mark per uncontested refusal, and marks
    /// cannot overlap"*.
    ///
    /// **[ruling 177]** The caller owes the *admission* precondition: only a
    /// candidate proving the same static with a verifying tail tag may reach
    /// here, or an attacker able to park mac1-valid rubbish provokes marks
    /// with no key material at all.
    /// §5.4's replacement teardown, fired by §6.4's replacing `accept()`.
    ///
    /// *"A fresh connection with fresh transport state on both sides … no
    /// stream, flow-control, recovery, or congestion state ever crosses a
    /// handshake"* — so this connection dies **whole**, and in-flight stream
    /// data on it is lost. §15.4's replaced row transmits **nothing**: there
    /// is no CLOSE and no linger, because the peer is not the party being
    /// told (it reconnected, and its new connection is already running).
    ///
    /// `Closed(Replaced)` is followed **within the same drain** by
    /// `ToEndpoint::Retired` (§16.4, ruling 81's no-linger case).
    ///
    /// A second call, or one against a connection already dying with its own
    /// cause, surfaces no second `Closed` (§16.4 emits it once) and only
    /// takes the state away.
    pub(crate) fn replaced(&mut self, now: Instant) {
        // The replaced row seals nothing (§15.4), so there is no instant to
        // seal at — but §16.4 puts a `now` on every mutating call, and a
        // signature that omits it is one a later slice has to widen.
        let _ = now;
        if self.lifecycle.is_dead() {
            return;
        }
        if self.closed_emitted {
            self.drop_state();
            return;
        }
        self.die(ConnectionLost::Replaced);
    }

    pub(crate) fn mark_contested(&mut self, now: Instant) {
        // **[ruling 179]** The carve-out, enforced on both sides: §6.4 takes
        // the mark and §7.5 describes the state, and a rule enforced only in
        // one of the two is a rule that gets missed.
        if !self.lifecycle.is_live() || !matches!(self.contested, Contested::No) {
            return;
        }
        let Some(floor) = self.next_counter() else {
            return;
        };
        self.contested = Contested::Pending { floor };
        tracing::debug!(
            target: "slither::policy",
            floor,
            "the connection is marked contested (§6.4's refusal against a None basis)"
        );
        // The transmission is a **separate** moment, and on an unvalidated
        // address §7.3's budget can hold it — which is the whole of the
        // pending gap. On a validated address the two coincide, and the
        // pump below is where they do.
        self.pump(now);
        self.drain_events();
    }

    /// §7.5's contested probe: the transmission, and the four things pinned
    /// to that one instant.
    ///
    /// §15.4: the probe's packet goes out *"at the first instant §7.3's
    /// budget admits it, **which is also when the deadline arms and when
    /// `Contested` is emitted**"*. So this sends the packet, arms
    /// `TimerKind::Contested`, queues `ConnEvent::Contested` and traces — as
    /// one step, never four.
    ///
    /// Returns `false` iff a mark is pending and the budget would not admit
    /// the probe. **[ruling 171]** A pending probe *"takes priority over all
    /// other output to an unvalidated address"* — ahead of ACKs, keepalives,
    /// PTO probes, retransmissions and new Data — so a `false` stops the
    /// pump dead rather than letting lower-priority output spend the budget
    /// the probe is waiting for. §7.5's congestion-gate argument transfers
    /// verbatim, and the budget cannot be waived, so priority is the only
    /// lever: a probe the budget could delay past its own deadline *"would
    /// silently convert congestion into a liveness verdict"*. **[ruling
    /// 215]** `true` does **not** mean "nothing was sent": an admitted probe
    /// returns `true` and the pump keeps building on the same pass.
    ///
    /// # One packet (**[RATIFIED 2026/08/17 — ruling 250]**)
    ///
    /// §7.3: *"The pump prefers **one packet**: the probe coalesces the owed
    /// path frames when the remaining room at pump time admits the coalesced
    /// size — 40 B with one 9 B path frame owed …, 49 B with both, which a
    /// roam constructs (§13.6 keeps the owed `PATH_RESPONSE` and re-draws
    /// the challenge at one instant) — and emits the bare 31 B PING
    /// otherwise, the path frames following at their rank when room next
    /// admits them; when room admits neither, nothing is emitted and no
    /// `Pto` deadline is announced (§13.3, ruling 249)."*
    ///
    /// That is **one** counter, **one** sent-map entry and one charge
    /// against the budget. Ruling 221's deletion of ruling 217's
    /// `dedicated_sent` machinery is not resurrected: this rides the
    /// ordinary seal path and the probe takes the floor counter again.
    ///
    /// `owe_challenge` is the pump's one-per-pump bound, read **and**
    /// written: a challenge this packet carries is not offered again below.
    ///
    /// ## Pump time, not arming time (working rule 8)
    ///
    /// §7.3's *"the budget holds both and always does: 40 B against a 90 B
    /// floor"* is a universal over **armed** budgets and says nothing about
    /// **remaining** ones — by the time this runs the budget has been spent
    /// down by whatever left since. The coalesced size is therefore checked
    /// against [`packing`](Self::packing)'s room (ruling 207(c)'s seam,
    /// which already sizes every packet to what the budget will admit),
    /// never asserted from the arming-instant floor.
    ///
    /// The arithmetic is **composed, never transcribed**, in the style
    /// [`probe_can_leave`](Self::probe_can_leave) uses: one PING byte plus
    /// nine per owed path frame, in *plaintext* units, because that is what
    /// `Packing` budgets in (ruling 207(c)'s trap — [`Amplification::room`]
    /// is in **datagram** bytes and the two differ by 30).
    ///
    /// ## §14.5's exemption covers the packet as built
    ///
    /// *"[AMENDED 2026/08/17 — ruling 250] The exemption covers the probe's
    /// packet **as built**: a probe that coalesces the owed
    /// `PATH_RESPONSE`/`PATH_CHALLENGE` (§7.3) remains exempt — the packet
    /// exists because the probe demanded it, the piggyback adds at most 18 B
    /// of frames, and gating the merged packet would starve the challenge at
    /// collapsed cwnd exactly where a roam makes it owed."* So this path
    /// consults [`Amplification::admits`] and **not** [`admits`](Self::admits)
    /// — which is what it did before the path frames joined, and the
    /// dedicated packet they arrive from was cwnd-gated and is gone.
    fn pump_contested_probe(&mut self, now: Instant, owe_challenge: &mut bool) -> bool {
        let Contested::Pending { floor } = self.contested else {
            return true;
        };

        // **[ruling 203/207(c)]** Sized to §7.3's *remaining* room, like
        // every other packet the pump builds. Before ruling 250 this was
        // `Packing::new()` — full size — which was harmless only because the
        // plan was a single byte.
        let mut packing = self.packing();
        // Which path frames are owed **now**. The challenge is owed on
        // exactly the terms the loop below states: the one-per-pump bound,
        // and an arming that has not been disarmed. There is no §8.7
        // *"something else is owed"* disjunct to check here, because a
        // pending contested probe **is** that something — this packet is
        // output waiting on exactly this budget.
        //
        // **[round 41 item 8 — 2026/08/18]** The loop's `offer` used to name
        // that state literally, as `contested.is_pending()`. That disjunct
        // is deleted: the loop cannot observe `Pending` at all, because this
        // function either finds no mark and returns `true`, or sends and
        // leaves `Armed`, or holds and returns `false` — which returns from
        // the pump. It was equally unreachable before ruling 250 (the 212(c)
        // pre-pass ran first, this ran second), so nothing about the
        // deletion created it. **This** is the site where §8.7's condition
        // is discharged for a connection carrying a mark, and the coalescing
        // below is how.
        let response_owed = self.owed_path_response.is_some();
        let challenge_owed = *owe_challenge && self.amplification.outstanding_challenge().is_some();
        let owed = usize::from(response_owed) + usize::from(challenge_owed);
        const PATH_FRAME_LEN: usize = 1 + frame::PATH_CHALLENGE_LEN;
        let coalesced = 1 + owed * PATH_FRAME_LEN;
        // All or nothing over the **owed set**, which is what §7.3 prices:
        // 40 B with one owed, 49 B with both. A room that admits only part
        // of the set leaves the whole of it to ranks 3 and 4, where it is
        // offered again the moment room next admits it.
        let path = if owed > 0 && packing.room() >= coalesced {
            self.pack_path_frames(&mut packing, challenge_owed)
        } else {
            PathPacked::default()
        };
        // Not a `debug_assert`: the plan is bounded by the remaining room
        // now, so a room below the PING's own datagram is reachable — and
        // an empty plaintext is §3.4's keepalive, not a probe. §7.3's
        // *"when room admits neither, nothing is emitted"*.
        if !packing.ping() {
            return false;
        }
        let plaintext = packing.into_plaintext();
        let size = (constants::DATA_HEADER_LEN + plaintext.len() + constants::AEAD_TAG_LEN) as u64;
        // **[ruling 207(a)]** The predicate does not move. Sizing to the
        // room only means the answer is now normally *yes*.
        if !self.amplification.admits(size) {
            return false;
        }

        let Some(session) = self.session.as_mut() else {
            return false;
        };
        // §7.4's quiet set: the probe is not fresh application intent, so it
        // does not move `last_send` and cannot suppress a keepalive. It
        // **is** ack-eliciting, which arms the death deadline.
        let sealed = match session.seal_quiet(now, &plaintext, true) {
            Ok(sealed) => sealed,
            Err(_) => {
                self.die(ConnectionLost::NonceExhausted);
                return false;
            }
        };
        let to = session.established().anchor;
        self.outputs.push_back(ConnOutput::Transmit(Transmit {
            to,
            data: sealed.datagram,
        }));
        self.amplification.on_sent(size);
        self.sync_liveness_timer();

        // **[ruling 43]** The probe is exempt from §14.5's admission gate and
        // is nonetheless **counted in the sent map and in
        // `bytes_in_flight`** — or loss recovery would hold a packet in
        // flight it could not see. **[ruling 250]** **One** entry, for the
        // coalesced packet as a whole.
        self.recovery.on_sent(SentPacket {
            counter: sealed.counter,
            time_sent: now,
            size,
            app_limited: false,
            path_gen: self.recovery.path_gen(),
            // §8.7: neither path frame is ever re-queued by loss detection,
            // and a PING is not retransmittable either — so a coalesced
            // packet carries no retransmittable frames and this stays empty.
            // The challenge's reliability is §8.7's standing obligation, the
            // response's is the peer's standing challenge, and the probe's
            // is `Contested`'s own deadline.
            frames: Vec::new(),
        });
        self.congestion.on_sent(now, size);

        // **[ruling 208]** Discharged on the **send**, never on the plan:
        // every early return above leaves both obligations owed.
        //
        // The response is discharged by one emission (§8.7); the challenge
        // is **not** — it stands for as long as the arming does. What clears
        // is the pump's one-per-pump bound, which is why this is the pump's
        // local and not a field.
        if path.response.is_some() {
            self.owed_path_response = None;
        }
        if path.challenge {
            *owe_challenge = false;
        }

        let deadline = now + constants::KEEPALIVE_TIMEOUT;
        self.contested = Contested::Armed {
            floor,
            armed_at: now,
            deadline,
        };
        self.timers.arm(TimerKind::Contested, deadline);
        // Pushed straight to the drain rather than through `events`: §8.1
        // pins the order *`Transmit` then `Event(Contested)`*, and this is
        // reached from inside the pump, after the callers that drain
        // `events`.
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Contested));
        tracing::debug!(
            target: "slither::policy",
            floor,
            "the contested probe was transmitted; the verdict is due one KEEPALIVE_TIMEOUT on"
        );
        true
    }

    /// Entering closing or draining disarms every timer but `CloseLinger`.
    ///
    /// §15.2 drops the state `Loss`, `Pto` and `AckDelay` act on, and says
    /// nothing about the other four. A surviving `Liveness` would fire
    /// mid-linger and produce a **second** `Closed` behind a latch that is
    /// already resolved — which is the invariant `closed()` rests on.
    fn enter_post_mortem_timers(&mut self) {
        let until = self
            .lifecycle
            .linger_until()
            .expect("called only on entering a post-mortem state");
        self.timers.disarm_all_except(TimerKind::CloseLinger);
        self.timers.arm(TimerKind::CloseLinger, until);
    }

    fn emit_closed(&mut self, lost: ConnectionLost) {
        debug_assert!(
            !self.closed_emitted,
            "§16.4: a connection dies once, and `Closed` is emitted once"
        );
        if self.closed_emitted {
            return;
        }
        self.closed_emitted = true;
        self.lost = Some(lost.clone());
        self.outputs
            .push_back(ConnOutput::Event(ConnEvent::Closed(lost)));
    }
}

/// §7.5's admissible persistent-keepalive band, as one function.
///
/// | argument | result |
/// |---|---|
/// | `None` | `Ok(())` — the beacon is disabled |
/// | `Duration::ZERO` … `999 ms` | `Err(KeepaliveTooShort)` |
/// | **`1 s` exactly** | **`Ok(())`** — the floor is **inclusive** (ruling 42) |
/// | `1 s + 1 ms` … `24.999 s` | `Ok(())` |
/// | **`25 s` exactly** | **`Err(KeepaliveTooLong)`** — the ceiling is **exclusive** (ruling 40) |
/// | `30 s`, `Duration::MAX` | `Err(KeepaliveTooLong)` |
///
/// Written once and used twice — the core's setter and the shell's — so the
/// band cannot be stated in two places and drift. The ceiling is
/// `DEAD_TIMEOUT` itself and gets **no named constant** (ruling 63: *"a
/// named ceiling would be a second place `DEAD_TIMEOUT` is written down, and
/// therefore a place it can drift"*).
pub(crate) fn validate_persistent_keepalive(interval: Option<Duration>) -> Result<(), ConfigError> {
    let Some(interval) = interval else {
        return Ok(());
    };
    if interval < constants::PERSISTENT_KEEPALIVE_MIN {
        return Err(ConfigError::KeepaliveTooShort);
    }
    if interval >= constants::DEAD_TIMEOUT {
        return Err(ConfigError::KeepaliveTooLong);
    }
    Ok(())
}

/// §16.2's acknowledgement snapshot — opaque, and taken at one instant.
///
/// *"every byte handed to the connection at this instant … Bytes written
/// after the call do not extend it."* The snapshot is therefore a **value**:
/// a `(stream, offset)` pair per live send half, frozen. Re-reading the
/// streams' current offsets at each poll instead is the implementation that
/// never terminates under a writer loop — the case §16.2 spells out.
///
/// Empty iff nothing had been written, in which case it is settled
/// immediately.
#[derive(Debug, Clone, Default)]
pub(crate) struct AckSnapshot(Vec<(StreamRef, u64)>);

/// One item of the connection core's drain. §16.4.
///
/// The variants §16.4 lists that this slice cannot yet construct — every
/// `ConnEvent` but `Established` and `Closed` — are absent rather than
/// stubbed: an uninhabited variant is a claim about the protocol, and these
/// will each arrive with the section that defines them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnOutput {
    /// Send this datagram.
    Transmit(Transmit),
    /// An application-visible event.
    Event(ConnEvent),
    /// A connection→endpoint event, folded into the one drain loop so
    /// there is no second queue to forget.
    ToEndpoint(ToEndpoint),
    /// **Terminal.**
    Timeout(Option<Instant>),
}

/// A connection event. §16.4.
///
/// The stream-naming variants are keyed by [`StreamRef`] and **not** by
/// [`StreamId`] — **ruling 95's amendment**. If they keyed by the wire id,
/// the shell could not match a wakeup to its waker before establishment,
/// which is exactly when §16.9 says work is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnEvent {
    /// The session is installed; the shell resolves `Connecting`.
    Established,
    /// A peer-opened stream is claimable through `accept(dir)`.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 99]** **One per newly-opened
    /// stream.** §9.2's implicit open of index 5 opens six streams and emits
    /// six events; `accept(dir)` returns one stream per call, so one event
    /// for six would force the shell to loop `accept()` until `None` on
    /// every wake or lose five — a lost-wakeup that manifests only under the
    /// reordering the tests inject. A frame above §10.4's cumulative limit
    /// emits **zero** and kills the connection, which is what bounds the
    /// burst.
    StreamOpened {
        /// Which direction became claimable.
        dir: Dir,
    },
    /// MAX_STREAMS credit arrived and actually raised the limit (§10.4).
    StreamsAvailable {
        /// The direction whose allowance grew.
        dir: Dir,
    },
    /// A read would now return something other than "no data".
    StreamReadable {
        /// The stream.
        r: StreamRef,
    },
    /// Stream or connection credit arrived for a blocked writer (§10.3).
    StreamWritable {
        /// The stream.
        r: StreamRef,
    },
    /// The send half is fully acknowledged — §9.7's `DataRecvd`.
    ///
    /// **Never fires in slice 4**: reaching `DataRecvd` needs §12's ACK
    /// processing, which is slice 5. That is the slice boundary, not a
    /// defect.
    StreamFinished {
        /// The stream.
        r: StreamRef,
    },
    /// The peer reset the stream (§9.6).
    StreamReset {
        /// The stream.
        r: StreamRef,
        /// The peer's application code.
        error_code: u64,
    },
    /// Connection-level send credit arrived and actually raised the limit
    /// (§10.3).
    ///
    /// **[ruling 150]** Minted for `send_message`, which is the one verb
    /// that can be refused for connection credit while holding **no
    /// stream**: `StreamWritable { r }` fires only for a half with a blocked
    /// writer, and a message refused before its `open` has no half to name.
    /// Without this event a blocked `send_message` is woken by nothing.
    ///
    /// A companion to `StreamWritable`, not a replacement: MAX_DATA emits
    /// both, one per blocked half and one for the connection.
    SendCreditAvailable,
    /// A complete message is claimable through `recv_message()`. §16.4.
    ///
    /// **One per uni stream that becomes complete while unclaimed** — the
    /// one-per-item discipline ruling 99 fixed for `StreamOpened`. Not one
    /// per STREAM frame, not one for a stream `accept_uni()` has already
    /// claimed, and never one for a locally-*sent* message.
    MessageReadable,
    /// A datagram is claimable through `recv_datagram()`. §16.4.
    ///
    /// **One per DATAGRAM frame admitted to the receive queue**, including
    /// one that evicted an older datagram: the queue went full → full, but a
    /// **new** item is claimable. Never one for the evicted datagram, and
    /// never one for a locally-*sent* datagram.
    DatagramReadable,
    /// §7.3's roam committed: `from` is the previous anchor, `to` the new
    /// one.
    ///
    /// Fires only where **the peer** moved. Our own rebind is invisible to
    /// us — we did not change where we send — so a local `AddressMoved` is
    /// something that can never happen (§7.3, S19).
    AddressMoved {
        /// The anchor the session left.
        from: SocketAddr,
        /// The anchor it moved to.
        to: SocketAddr,
    },
    /// §7.5's contested probe **went out** (rulings 45/46).
    ///
    /// **Never at the mark.** The two instants separate whenever §7.3's
    /// budget holds the PING, which is exactly when the connection has just
    /// roamed to an unvalidated address; on a validated one they coincide.
    /// A unit variant, and permanently so: ruling 46 removed
    /// `under_probe: bool` because *"the three real states (marked-pending,
    /// probing, cleared) do not map onto one bool at all."*
    Contested,
    /// An ACK covered the probe floor; the mark cleared (ruling 41).
    ///
    /// Emitted **only where `Contested` was** (ruling 176): a mark cleared
    /// while still pending emits neither.
    ContestCleared,
    /// The connection ended, with §18.1's cause. Emitted **once**.
    ///
    /// Not `Copy`, and neither is [`ConnOutput`] any more:
    /// `ConnectionLost::PeerClosed` carries the peer's reason phrase, which
    /// is `Vec<u8>` because §8.4 carries it as bytes.
    Closed(ConnectionLost),
}

/// Whether the core took the whole payload. §9.8.
///
/// **[RATIFIED 2026/08/16 — ruling 163]** *"Not now"* is **not an error**,
/// so §18.1 stays closed: `MessageError` is exhaustive, already shipped and
/// pinned by `tests/spec_errors.rs` — `TooLarge` and `ConnectionLost`,
/// nothing else — and adding a variant would be a breaking change to a
/// taxonomy that is closed by process. It is reported in the **success**
/// type instead, which is what this core already does twice over:
/// [`write`](Connection::write)'s `Ok(0)` and [`accept`](Connection::accept)'s
/// `None` both mean *the state is not ready*, and neither is an error
/// either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendMessage {
    /// Admitted in full: a uni stream was opened, every byte entered send
    /// state, and the FIN rides the final data frame (ruling 153).
    Sent,
    /// **Nothing happened — a total no-op.** No stream opened, no index
    /// spent, no byte buffered, no event queued, and the caller's payload
    /// untouched. The shell parks and retries.
    ///
    /// # The invariant, and why it is load-bearing
    ///
    /// Ruling 150 chose atomic admission precisely so a dropped
    /// `send_message` future cannot leave a **FIN-less half-written uni
    /// stream** behind, which against a message-mode receiver would
    /// *manufacture* the failure S30 exists to diagnose. A `Blocked` that
    /// had already opened a stream reintroduces that by the back door — so
    /// it is delivered by **ordering** and not by cleanup:
    /// [`send_message`](Connection::send_message) tests connection credit
    /// **before** `Streams::open`, and `Streams::open` itself returns
    /// `Err(StreamsExhausted)` above its first mutation. There is no
    /// rollback path to get wrong because there is nothing to roll back.
    ///
    /// # It carries no reason, deliberately
    ///
    /// The shell parks in one `message_senders` set fed by three wake
    /// sources — `ConnEvent::StreamsAvailable { dir: Dir::Uni }`, ruling
    /// 150's `ConnEvent::SendCreditAvailable`, and the death latch — so
    /// naming the cause would buy the caller nothing and cost a second slot
    /// to release.
    Blocked,
}

/// A two-core smoke check for the §9/§10 machinery.
///
/// **Not the slice's acceptance tests** — those are written independently by
/// the test author (working rule 6) and live in a file this implementer
/// never touches. This exists because the transmit pump, the codec and the
/// receive path have no other in-file exercise: everything else in the four
/// new modules is unit-testable against its own state, and the *seam between
/// them* is not.
///
/// It builds two real `Connection`s over one real hiss handshake and hands
/// each one the other's datagrams, so every assertion here is a wire
/// assertion.
#[cfg(test)]
mod smoke {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::num::NonZeroU64;
    use std::time::Instant;

    use super::*;
    use crate::constants::{PROLOGUE, REKEY_EPOCH_MSGS};
    use crate::identity::Identity;
    use crate::packet::ReferenceSuite;
    use crate::testutil::CountingIdentity;

    type Suite = ReferenceSuite;
    type Id = CountingIdentity<Suite>;

    fn v4(a: u8) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, a)), 4000 + a as u16)
    }

    /// Both halves of one real IK handshake, as two `EstablishedSession`s.
    fn sessions() -> (EstablishedSession<Suite>, EstablishedSession<Suite>) {
        let epoch = NonZeroU64::new(REKEY_EPOCH_MSGS).expect("nonzero");
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
        let (_claimed, mid) = <Suite as Handshake>::read_msg1_intro(resp, &msg1).expect("intro");
        let (_payload, read) = <Suite as Handshake>::complete(mid).expect("complete");
        let (msg2, b_transport) = <Suite as Handshake>::write_msg2(read).expect("msg2");
        let a_transport = <Suite as Handshake>::read_msg2(sent, &msg2).expect("read msg2");

        let (a_seal, a_open) = <Suite as Handshake>::into_datagram(a_transport, epoch);
        let (b_seal, b_open) = <Suite as Handshake>::into_datagram(b_transport, epoch);

        (
            EstablishedSession {
                seal: a_seal,
                open: a_open,
                our_index: 0x1111_1111,
                peer_index: 0x2222_2222,
                anchor: v4(2),
            },
            EstablishedSession {
                seal: b_seal,
                open: b_open,
                our_index: 0x2222_2222,
                peer_index: 0x1111_1111,
                anchor: v4(1),
            },
        )
    }

    /// Two established `Connection`s over one real IK handshake: `a` is
    /// §6.7's initiator, `b` the acceptor, so §9.1's parity is directly
    /// assertable.
    /// Two **dialled** cores, installed through §16.4's `Install`.
    ///
    /// Built this way rather than through `Connection::established` — which
    /// is what it used to be, and is one line shorter — because that
    /// constructor **is** §6.4's accept path, and since slice 7 it arms
    /// §7.3's anti-amplification budget from the accepted msg1's anchor.
    /// A budgeted core sends at most 3 × 196 bytes before the peer answers,
    /// which is correct for an accepted connection and wrong for a fixture
    /// whose subject is §9/§10's machinery: every multi-packet write below
    /// would be measuring the budget rather than the fill.
    ///
    /// A `connect()`-created connection starts validated, so this is the
    /// pair with no §7.3 state at all — and it is also what
    /// `testfix::Pair::installed_at` builds, for the same reason.
    fn pair(now: Instant) -> (Connection<Suite>, Connection<Suite>) {
        let (a_session, b_session) = sessions();
        let mut a = Connection::connecting([1u8; 32]);
        let mut b = Connection::connecting([2u8; 32]);
        a.handle_endpoint_event(
            now,
            Install {
                session: a_session,
                role: Role::Initiator,
                anchor_from_msg1: false,
            },
        );
        b.handle_endpoint_event(
            now,
            Install {
                session: b_session,
                role: Role::Responder,
                anchor_from_msg1: false,
            },
        );
        (a, b)
    }

    /// Drain one core to `Timeout`, returning what it produced.
    fn drain(conn: &mut Connection<Suite>) -> (Vec<Vec<u8>>, Vec<ConnEvent>) {
        let mut datagrams = Vec::new();
        let mut events = Vec::new();
        loop {
            match conn.poll_output() {
                ConnOutput::Transmit(t) => datagrams.push(t.data),
                ConnOutput::Event(e) => events.push(e),
                ConnOutput::ToEndpoint(_) => {}
                ConnOutput::Timeout(_) => return (datagrams, events),
            }
        }
    }

    /// Shuttle datagrams **both ways** until neither core has anything left
    /// to send, returning every event each produced.
    ///
    /// [`deliver`] cannot serve §12: it drains the receiver for its *events*
    /// and discards its datagrams, so the ACK the receiver owes never
    /// reaches the sender. The §12/§13 loop is a round trip by construction
    /// and needs a round-trip fixture.
    fn exchange(
        a: &mut Connection<Suite>,
        b: &mut Connection<Suite>,
        now: Instant,
    ) -> (Vec<ConnEvent>, Vec<ConnEvent>) {
        let mut events_a = Vec::new();
        let mut events_b = Vec::new();
        for _ in 0..64 {
            let (from_a, ea) = drain(a);
            events_a.extend(ea);
            let (from_b, eb) = drain(b);
            events_b.extend(eb);
            if from_a.is_empty() && from_b.is_empty() {
                return (events_a, events_b);
            }
            for dgram in from_a {
                b.handle_datagram(now, v4(1), &dgram);
            }
            for dgram in from_b {
                a.handle_datagram(now, v4(2), &dgram);
            }
        }
        panic!("the two cores never went quiet");
    }

    /// Hand every datagram `from` produced to `to`, and drain `to`.
    fn deliver(
        from: &mut Connection<Suite>,
        to: &mut Connection<Suite>,
        now: Instant,
        src: SocketAddr,
    ) -> Vec<ConnEvent> {
        let (datagrams, _) = drain(from);
        let mut events = Vec::new();
        for dgram in datagrams {
            to.handle_datagram(now, src, &dgram);
            events.extend(drain(to).1);
        }
        events
    }

    /// §9.1's parity, end to end: the initiator's first bidi stream is id 0
    /// and the acceptor's is id 1 — and both cores agree.
    #[test]
    fn stream_id_parity_comes_from_the_installed_role() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let ra = a.open(Dir::Bi).expect("first bidi");
        let rb = b.open(Dir::Bi).expect("first bidi");
        assert_eq!(a.stream_id(ra).map(StreamId::as_u64), Some(0));
        assert_eq!(b.stream_id(rb).map(StreamId::as_u64), Some(1));
        assert_eq!(a.role(), Some(Role::Initiator));
        assert_eq!(b.role(), Some(Role::Responder));

        let ua = a.open(Dir::Uni).expect("first uni");
        let ub = b.open(Dir::Uni).expect("first uni");
        assert_eq!(a.stream_id(ua).map(StreamId::as_u64), Some(2));
        assert_eq!(b.stream_id(ub).map(StreamId::as_u64), Some(3));
    }

    /// A write on one core arrives, in order, as a read on the other — the
    /// whole of §9.5's happy path through the real codec and the real AEAD.
    #[test]
    fn bytes_written_on_one_core_are_read_on_the_other() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        assert_eq!(a.write(now, r, b"hello world").expect("write"), 11);
        a.finish(now, r).expect("finish");
        a.flush(now);

        let events = deliver(&mut a, &mut b, now, v4(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnEvent::StreamOpened { dir: Dir::Uni })),
            "the peer's first frame opens the stream: {events:?}"
        );

        let claimed = b.accept(Dir::Uni).expect("one claimable uni stream");
        assert_eq!(
            b.stream_id(claimed).map(StreamId::as_u64),
            a.stream_id(r).map(StreamId::as_u64),
            "both ends name the stream identically"
        );

        let mut buf = [0u8; 64];
        assert_eq!(b.read(now, claimed, &mut buf), Ok(Some(11)));
        assert_eq!(&buf[..11], b"hello world");
        assert_eq!(
            b.read(now, claimed, &mut buf),
            Ok(None),
            "FIN is end of stream"
        );
    }

    /// §9.6's reset crosses the wire and surfaces as `ReadError::Reset`.
    #[test]
    fn a_reset_crosses_the_wire_and_surfaces_to_the_reader() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, b"partial").expect("write");
        let _ = deliver(&mut a, &mut b, now, v4(1));
        let claimed = b.accept(Dir::Uni).expect("claimable");

        a.reset(now, r, 42);
        let events = deliver(&mut a, &mut b, now, v4(1));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnEvent::StreamReset { error_code: 42, .. })),
            "{events:?}"
        );
        let mut buf = [0u8; 8];
        assert_eq!(b.read(now, claimed, &mut buf), Err(ReadError::Reset(42)));
    }

    /// A payload larger than one packet's plaintext is carried by more than
    /// one packet and reassembles whole.
    #[test]
    fn a_multi_packet_write_reassembles() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let payload: Vec<u8> = (0..8_000u32).map(|i| (i % 251) as u8).collect();
        let r = a.open(Dir::Uni).expect("uni");
        assert_eq!(a.write(now, r, &payload), Ok(payload.len()));
        a.finish(now, r).expect("finish");
        a.flush(now);

        let (datagrams, _) = drain(&mut a);
        assert!(
            datagrams.len() > 1,
            "8 000 bytes does not fit one MAX_PLAINTEXT packet"
        );
        for dgram in datagrams {
            b.handle_datagram(now, v4(1), &dgram);
            let _ = drain(&mut b);
        }

        let claimed = b.accept(Dir::Uni).expect("claimable");
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(Some(n)) = b.read(now, claimed, &mut buf) {
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, payload);
    }

    /// §8.5's round-robin: two streams with data pending both make progress
    /// inside one fill pass, rather than one starving the other.
    ///
    /// The contention has to be *built*: §16.7 makes `write` seal
    /// synchronously, so two sequential writes on a live connection never
    /// contend — the first has already gone out. Writing **before the
    /// install** (§16.9) queues both and makes the install's pump the first
    /// fill pass that sees two ready streams.
    #[test]
    fn the_fill_serves_streams_round_robin() {
        let now = Instant::now();
        let (a_session, b_session) = sessions();
        let mut a: Connection<Suite> = Connection::connecting([5u8; 32]);

        let r1 = a.open(Dir::Uni).expect("uni");
        let r2 = a.open(Dir::Uni).expect("uni");
        a.write(now, r1, &vec![1u8; 4_000]).expect("write");
        a.write(now, r2, &vec![2u8; 4_000]).expect("write");
        assert!(drain(&mut a).0.is_empty(), "§16.9: nothing before install");

        a.handle_endpoint_event(
            now,
            Install {
                session: a_session,
                role: Role::Initiator,
                anchor_from_msg1: false,
            },
        );

        let mut b = Connection::established(now, [6u8; 32], b_session, Role::Responder);
        let _ = drain(&mut b);

        let (datagrams, _) = drain(&mut a);
        b.handle_datagram(now, v4(1), &datagrams[0]);
        let _ = drain(&mut b);

        let s1 = b.accept(Dir::Uni).expect("first stream");
        let s2 = b
            .accept(Dir::Uni)
            .expect("the second stream shares the packet");
        let mut buf = [0u8; 4096];
        let n1 = b.read(now, s1, &mut buf).expect("read").expect("data");
        let n2 = b.read(now, s2, &mut buf).expect("read").expect("data");
        assert!(
            n1 > 0 && n2 > 0,
            "one packet carried both streams: {n1} and {n2}"
        );
        assert!(
            n1 < 4_000,
            "§8.5 serves a quantum, not a whole stream: {n1}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Slice 5's seam — §12 ↔ §13 ↔ §14, over two real cores
    // ═══════════════════════════════════════════════════════════════════
    //
    // The acceptance tests for §12, §13 and §14 are written independently
    // by two blind authors (working rule 6) in `tests_ack.rs` and
    // `tests_recovery.rs`, which this implementer never sees. These five
    // exist for the same reason the four above do: the *seam* between the
    // ACK derivation, the sent-packet map and the transmit pump has no
    // other exercise, because each of the three is unit-testable against
    // its own state and their junction is not.

    /// The whole feedback loop this slice exists to close: the receiver
    /// owes an ACK (§12.4), packs it (§12.2), the sender's map resolves it
    /// (§12.5), and §9.7's `DataRecvd` is finally reachable.
    ///
    /// `StreamFinished` **cannot** fire without every one of those working,
    /// which is what makes one assertion cover the seam.
    #[test]
    fn an_ack_drains_the_sent_map_and_completes_the_send_half() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, &vec![7u8; 8_000]).expect("write");
        a.finish(now, r).expect("finish");
        a.flush(now);
        assert!(
            a.bytes_in_flight() > 0,
            "§13.5: an ack-eliciting packet is tracked the moment it is sealed"
        );

        // A → B: the data. B owes an ACK (§12.4 — the first ack-eliciting
        // packet has no previous greatest, so it is immediate). B → A: it.
        let (mut events, _) = exchange(&mut a, &mut b, now);

        // §12.4, and this is the delayed-ACK policy being real rather than
        // nominal: A's last packet is the 1st since B's previous ACK and
        // arrived in order, so B owes nothing yet and `AckDelay` carries it.
        // A build that ACKed every packet has an empty timer here and
        // settles a step early — and passes every completion test.
        let delayed = now + constants::MAX_ACK_DELAY;
        assert_eq!(
            b.timer(TimerKind::AckDelay),
            Some(delayed),
            "§12.4: the odd packet out waits on the timer"
        );
        assert!(
            a.bytes_in_flight() > 0,
            "…and until it fires, that packet is still in flight"
        );

        b.handle_timeout(delayed);
        let (rest, _) = exchange(&mut a, &mut b, delayed);
        events.extend(rest);

        assert_eq!(
            a.bytes_in_flight(),
            0,
            "§14.5: every acknowledged packet's bytes leave the flight"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnEvent::StreamFinished { r: got } if *got == r)),
            "§9.3: `DataRecvd` is reached only by acknowledgement: {events:?}"
        );
    }

    /// §12.4's two triggers are distinguishable, which is the whole point of
    /// replacing the immediate-ACK-per-packet policy.
    ///
    /// An **in-order** first ack-eliciting packet must *not* draw an
    /// immediate ACK; it arms `AckDelay` at exactly `MAX_ACK_DELAY`. A build
    /// that kept the old policy passes every completion test and fails this.
    #[test]
    fn the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        // Counter 0 is out-of-order by §12.4's own vacuous clause (no
        // previous greatest), so it draws an immediate ACK and seeds the
        // state. From counter 1 onwards arrivals are in order.
        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, b"first").expect("write");
        let (first, _) = drain(&mut a);
        b.handle_datagram(now, v4(1), &first[0]);
        let (acks, _) = drain(&mut b);
        assert_eq!(acks.len(), 1, "§12.4: the first arrival ACKs immediately");

        a.write(now, r, b"second").expect("write");
        let (second, _) = drain(&mut a);
        b.handle_datagram(now, v4(1), &second[0]);
        let (none, _) = drain(&mut b);
        assert!(
            none.is_empty(),
            "§12.4: one in-order ack-eliciting packet owes nothing yet: {none:?}"
        );
        assert_eq!(
            b.timer(TimerKind::AckDelay),
            Some(now + constants::MAX_ACK_DELAY),
            "§12.4: …it arms `AckDelay` at MAX_ACK_DELAY instead"
        );

        a.write(now, r, b"third").expect("write");
        let (third, _) = drain(&mut a);
        b.handle_datagram(now, v4(1), &third[0]);
        let (ack, _) = drain(&mut b);
        assert_eq!(ack.len(), 1, "§12.4: an ACK is owed after every 2nd");
        assert_eq!(
            b.timer(TimerKind::AckDelay),
            None,
            "packing the ACK disarms the delay"
        );
    }

    /// §13.3's arming precondition, from **both** sides.
    ///
    /// A build that leaves `Pto` armed on an empty map self-sustains a probe
    /// train at ~20 packets/s — §13.3 names that failure itself — and no
    /// completion test can see it.
    #[test]
    fn the_pto_is_armed_only_while_something_is_in_flight() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);
        assert_eq!(a.timer(TimerKind::Pto), None, "nothing in flight yet");

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, b"payload").expect("write");
        a.flush(now);
        assert!(
            a.timer(TimerKind::Pto).is_some(),
            "§13.3: armed while an ack-eliciting packet is outstanding"
        );

        let _ = exchange(&mut a, &mut b, now);
        assert_eq!(a.bytes_in_flight(), 0);
        assert_eq!(
            a.timer(TimerKind::Pto),
            None,
            "§13.3: disarmed when the map empties"
        );
    }

    /// §13.4's probe rescues a flight that §13.2 cannot even judge.
    ///
    /// With the only data packet lost outright, nothing has ever been
    /// acknowledged, so `largest_acked` is `None` and §13.2's walk declares
    /// nothing — **only the PTO can move this connection**. The probe is a
    /// bare PING (§13.4's "else": the bytes are `unacked`, not *pending*),
    /// it is tracked despite being gate-exempt (ruling 43, §17.5), and the
    /// ACK it elicits is what finally lifts `largest_acked` above the lost
    /// counter so §13.2's packet threshold can fire.
    #[test]
    fn a_firing_pto_probes_a_flight_loss_detection_cannot_yet_judge() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, b"payload").expect("write");
        a.finish(now, r).expect("finish");
        // Two datagrams, because §16.7 seals inside the call that triggers
        // it: `write` sealed the bytes and `finish` sealed the FIN. Neither
        // ever reaches B.
        let (lost, _) = drain(&mut a);
        assert_eq!(lost.len(), 2);

        let deadline = a.timer(TimerKind::Pto).expect("§13.3 arms it");
        let before = a.bytes_in_flight();
        a.handle_timeout(deadline);
        let (probes, _) = drain(&mut a);

        assert_eq!(probes.len(), 1, "§13.4: **one** ack-eliciting packet");
        assert!(
            a.bytes_in_flight() > before,
            "ruling 43: exempt from admission, never from accounting"
        );

        // The probe reaches B, which has seen nothing else. Its ACK carries
        // a `largest` above the lost counter, and from there §13.2 does the
        // rest without any further prompting.
        b.handle_datagram(deadline, v4(1), &probes[0]);
        let _ = exchange(&mut a, &mut b, deadline);

        let claimed = b.accept(Dir::Uni).expect("the retransmission arrived");
        let mut buf = [0u8; 16];
        assert_eq!(b.read(deadline, claimed, &mut buf), Ok(Some(7)));
        assert_eq!(&buf[..7], b"payload");
        assert_eq!(
            b.read(deadline, claimed, &mut buf),
            Ok(None),
            "ruling 113: the FIN was recorded on the packet that carried it, \
             so the retransmission carries it too"
        );
    }

    /// §14.5's admission gate and **ruling 134**, which are the same seam
    /// seen from the two sides.
    ///
    /// `write()` accepts everything §10's credit admits — the window defers
    /// the *seal*, never the acceptance — and the gate then holds the
    /// surplus off the wire until an acknowledgement makes room. A build
    /// that gated `write()` instead fails the first assertion; one that
    /// gated on plaintext rather than the datagram (ruling 136) overshoots
    /// the window by 30 bytes a packet and fails the third.
    #[test]
    fn the_window_defers_the_seal_and_never_the_acceptance() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        let r = a.open(Dir::Uni).expect("uni");
        let payload = vec![9u8; 32 * 1024];
        assert_eq!(
            a.write(now, r, &payload).expect("write"),
            payload.len(),
            "ruling 134: `write()` stays a flow-control verb and the window \
             is invisible to it"
        );

        let (sealed, _) = drain(&mut a);
        let on_the_wire: u64 = sealed.iter().map(|d| d.len() as u64).sum();
        assert!(
            on_the_wire < payload.len() as u64,
            "…and yet the window held most of it back"
        );
        assert_eq!(
            on_the_wire,
            a.bytes_in_flight(),
            "ruling 136: what is in flight is what went on the wire, in \
             datagram units"
        );
        assert!(
            on_the_wire <= a.congestion_window(),
            "§14.5: bytes_in_flight + candidate_size <= cwnd"
        );
        assert!(
            on_the_wire + constants::MAX_DATAGRAM as u64 > a.congestion_window(),
            "…and one more full datagram would not have fitted, so the gate \
             really was what stopped the fill"
        );

        // The peer acknowledges, the window re-opens, and the rest follows
        // with no further application involvement — the property the gate
        // exists to provide.
        let mut clock = now;
        for _ in 0..64 {
            let _ = exchange(&mut a, &mut b, clock);
            if a.bytes_in_flight() == 0 {
                break;
            }
            // §12.4 holds the odd packet's ACK on `AckDelay`; step to it.
            clock += constants::MAX_ACK_DELAY;
            a.handle_timeout(clock);
            b.handle_timeout(clock);
        }
        assert_eq!(a.bytes_in_flight(), 0, "everything was acknowledged");

        let claimed = b.accept(Dir::Uni).expect("the stream arrived");
        let mut got = Vec::new();
        let mut buf = vec![0u8; 8192];
        while let Ok(Some(n)) = b.read(clock, claimed, &mut buf) {
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, payload, "every accepted byte was eventually sealed");
    }

    /// §16.2's snapshot settles on **acknowledgement**, not on writing.
    ///
    /// A build whose snapshot is "all streams' current offsets, re-read at
    /// each poll" never terminates under a writer loop, and one that settles
    /// at `write()` settles before the bytes have left.
    #[test]
    fn a_snapshot_settles_only_once_its_bytes_are_acknowledged() {
        let now = Instant::now();
        let (mut a, mut b) = pair(now);
        let _ = drain(&mut a);
        let _ = drain(&mut b);

        assert!(
            a.snapshot_settled(&a.ack_snapshot()),
            "§16.2: a connection with nothing written is settled at once"
        );

        let r = a.open(Dir::Uni).expect("uni");
        a.write(now, r, &vec![3u8; 4_000]).expect("write");
        a.flush(now);
        let snap = a.ack_snapshot();
        assert!(
            !a.snapshot_settled(&snap),
            "the bytes are in flight, not acknowledged"
        );

        // Bytes written *after* the call do not extend the snapshot (§16.2),
        // so this second write must not keep it from settling.
        a.write(now, r, &vec![4u8; 4_000]).expect("write");
        a.flush(now);
        let _ = exchange(&mut a, &mut b, now);
        assert!(
            a.snapshot_settled(&snap),
            "§16.2: the snapshot covers what was handed over at the call"
        );
    }

    /// §16.9: a stream opened before establishment keeps its handle across
    /// the install, writes queued early leave once a session exists, and the
    /// wire id appears only afterwards.
    ///
    /// **Ruling 95's trap**, from the other side: a core that returned an
    /// internal index *typed as* `StreamId` and remapped at install would
    /// pass "open early" and "write late" separately and fail exactly this.
    #[test]
    fn an_early_opened_stream_keeps_its_handle_across_install() {
        let now = Instant::now();
        let (a_session, b_session) = sessions();

        let mut a: Connection<Suite> = Connection::connecting([3u8; 32]);
        let r = a.open(Dir::Uni).expect("open before establishment");
        assert_eq!(a.stream_id(r), None, "§16.9: no id until established");
        assert_eq!(a.write(now, r, b"queued").expect("write"), 6);
        a.finish(now, r).expect("finish");
        let (datagrams, _) = drain(&mut a);
        assert!(datagrams.is_empty(), "§16.9: nothing sends before install");

        a.handle_endpoint_event(
            now,
            Install {
                session: a_session,
                role: Role::Initiator,
                anchor_from_msg1: false,
            },
        );
        assert_eq!(
            a.stream_id(r).map(StreamId::as_u64),
            Some(2),
            "the same handle now names initiator-uni index 0"
        );

        let mut b = Connection::established(now, [4u8; 32], b_session, Role::Responder);
        let _ = drain(&mut b);
        let _ = deliver(&mut a, &mut b, now, v4(1));

        let claimed = b.accept(Dir::Uni).expect("the early write arrived");
        let mut buf = [0u8; 16];
        assert_eq!(b.read(now, claimed, &mut buf), Ok(Some(6)));
        assert_eq!(&buf[..6], b"queued");
    }
}

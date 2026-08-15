//! §16.4's `core::Endpoint<I: Identity>` — the sans-io endpoint core.
//!
//! One socket's worth of protocol state: the stage-0 introduction queue
//! (§6.3), the timestamp guard (§17.1), the index and static tables
//! (§17.2–17.4), and every in-flight outbound initiation (§5.5). It reads
//! no clock, performs no I/O, and spawns nothing.
//!
//! # The drain contract
//!
//! Every mutating call is followed by draining [`poll_output`] to the
//! terminal `Timeout(Option<Instant>)`, which is both the drain sentinel
//! and the next-deadline announcement. **Output order within one drain is
//! generation order** and is normative (§16.4).
//!
//! # What arrives where
//!
//! * A `PKT_HANDSHAKE_INIT` that passes §3.1's exact-length gate and mac1
//!   is offered to the queue at **0 DH** and surfaces as `IntroReady`.
//! * A `PKT_HANDSHAKE_RESP` is matched **by index, never by address**
//!   (§5.5 step 4), and completes the pending it belongs to.
//! * A `PKT_DATA` routes by `receiver_index` to a live session, or is
//!   dropped. §17.3's corollary — "a datagram that routes by index but
//!   fails to open touches nothing" — is the connection core's to keep;
//!   the endpoint's part is that a miss changes nothing here.
//!
//! # The slice boundary this core knowingly carries
//!
//! §5.4's post-`ss` responder rule is three-valued: **LIVE** / **PENDING**
//! / **NONE**. Two of the three are complete: NONE from slice 2a, and
//! **PENDING** with ruling 91, which pulled §6.5's routing and §6.6's
//! internal tie-break forward into this slice — [`routing`] holds both,
//! and §6.4's PENDING branch is in [`staged`]'s `accept()`.
//!
//! **LIVE is what remains.** §6.4's re-home walk and its proven-LIVE
//! replacement admission — the basis check, `ConnectionLost::Replaced`,
//! and §7.5's contested-connection probe — still return
//! [`AcceptError::Stale`] here. That is not correct in general: a
//! replacement whose timestamp passes both the guard and the basis must
//! succeed. It is a documented, deliberate boundary, and what it preserves
//! meanwhile is §16.1's **one session per peer static**, which returning
//! `Stale` cannot violate.
//!
//! §6.5's step-4 `read_identity()` interception is also outstanding, for a
//! reason that is not scheduling: §16.4 gives that verb no `now` and §6.6
//! cannot record a guard entry without one. [`routing`]'s module docs
//! state the case, and nothing diverges meanwhile — those chains reach the
//! same comparison by §6.4's PENDING branch.
//!
//! [`poll_output`]: Endpoint::poll_output

pub(crate) mod guard;

// §6.5/§6.6's acceptance tests, written independently from `SPEC.md` by an
// author working in an isolated git worktree, blind to this directory
// (working rule 6). Declared at integration so neither agent could reach
// the other's file.
#[cfg(test)]
mod tests;
pub(crate) mod handshake;
pub(crate) mod intro_queue;
pub(crate) mod routing;
pub(crate) mod staged;
pub(crate) mod tables;

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};

use crate::config::Config;
use crate::constants;
use crate::core::{
    Connection, ConnectionId, Disposition, EndpointOutput, EstablishedSession, Install, Timestamp,
    ToEndpoint, Transmit,
};
use crate::error::ConnectError;
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::{Handshake, Inbound, Mac1Key, classify};

use self::guard::TimestampGuard;
use self::intro_queue::{Arrival, IntroQueue};
use self::staged::InitiatorSent;
use self::tables::{IndexTables, StaticEntry, StaticMap, StaticState};

pub use self::staged::IntroId;

/// An outbound initiation, from `connect()` until it completes, gives up,
/// or is cancelled. §5.5.
///
/// **Every field marked "fresh every attempt" is exactly that** — §5.5 rule
/// 2: *"Every retransmit is a completely fresh initiation — new ephemeral,
/// new random index, new strictly-greater timestamp."*
struct Pending<I: Identity> {
    conn: ConnectionId,
    remote: SocketAddr,
    remote_static: PublicKeyOf<I>,
    remote_static_bytes: Vec<u8>,
    /// Derived once from the peer's static: mac1 on an outbound msg1 is
    /// keyed on the **recipient's** key.
    peer_mac1: Mac1Key,
    /// The current attempt's index — fresh every attempt. Held apart from
    /// [`state`](Pending::state) because `read_msg2` consumes the state
    /// whether it succeeds or fails, and the route must still be
    /// retirable afterwards.
    sender_index: Option<u32>,
    /// hiss's post-msg1 initiator state — fresh every attempt. `None` when
    /// this interval's attempt could not be built, or once a completion has
    /// consumed it.
    state: Option<Box<InitiatorSent<I>>>,
    /// Armed at `RETRANSMIT_BASE + U[0, RETRANSMIT_JITTER_MAX]`. **Fixed,
    /// not exponential** — §13's exponential PTO governs the data path
    /// alone.
    next_retransmit: Instant,
    /// `HANDSHAKE_GIVEUP` after the **first** attempt, never re-based by a
    /// retransmit.
    give_up_at: Instant,
    /// §5.5 rule 3: one completion attempt per retransmit interval.
    attempt_spent: bool,
    /// Whether **any** attempt has reached the wire. Ruling 72.
    ///
    /// `false` means every interval so far failed *locally* — the identity
    /// would not open, or msg1 would not write on it — so not one msg1
    /// exists for the peer to have ignored. A give-up in that state is
    /// [`ConnectError::Local`], not [`ConnectError::TimedOut`]; see
    /// [`build_attempt`](Endpoint::build_attempt).
    attempted: bool,
    /// Whether this pending actually took a §17.1 pin.
    ///
    /// A pin **never creates an entry**, so a dial to a static nobody has
    /// recorded takes none. Releasing unconditionally would then decrement
    /// a pin somebody else took later — on §5.4's PENDING row that is a
    /// staged mid-state's pin, and stripping it lets §17.1's "never evicted
    /// while a staged mid-state exists" fail silently, aging the entry out
    /// at `TS_GUARD_ORPHAN_TTL`. Pin and release must agree, so the answer
    /// is carried rather than assumed. (The staged path already does this
    /// via `guard_pin: Option<Vec<u8>>`; this is the same discipline.)
    guard_pinned: bool,
}

/// §16.4's endpoint core.
pub(crate) struct Endpoint<I: Identity> {
    config: Config,
    identity: I,
    /// Our static's canonical §2.4 octets, cached: mac1 keying and every
    /// table read want them, and `public_static()` promises to be cheap.
    our_static_bytes: Vec<u8>,
    /// mac1 keyed on **our** static — the key an inbound packet is verified
    /// against, because we are its recipient.
    our_mac1: Mac1Key,
    /// §16.6's one seeded endpoint RNG: every index, every jitter draw and
    /// every connection sub-seed comes from it.
    rng: ChaCha20Rng,
    outputs: VecDeque<EndpointOutput<I::Suite>>,
    intros: IntroQueue<I>,
    guard: TimestampGuard,
    indices: IndexTables,
    statics: StaticMap,
    /// Keyed order, so a drain over several pendings is deterministic —
    /// §16.4 makes generation order normative and a `HashMap` would make it
    /// a coin toss.
    pendings: BTreeMap<ConnectionId, Pending<I>>,
    /// §17.2's endpoint-global monotonic forcing. Survives across
    /// connection generations, so close-and-reconnect still emits strictly
    /// greater.
    last_init_timestamp: Option<Timestamp>,
    next_connection: u64,
}

impl<I: Identity> Endpoint<I> {
    /// §16.4's constructor.
    ///
    /// `rng_seed` seeds the **one** endpoint RNG (§16.6). A build that lets
    /// a caller choose it is security-relevant — indices must be
    /// unpredictable off-path, which is what §5.5's on-path-only completion
    /// spend rests on — and that is why this core is crate-internal in this
    /// slice: publishing the constructor is a decision for the slice that
    /// has an opinion about the public surface.
    pub(crate) fn new(_now: Instant, config: Config, identity: I, rng_seed: [u8; 32]) -> Self {
        let our_static_bytes = identity.public_static().as_ref().to_vec();
        let our_mac1 = Mac1Key::derive(&our_static_bytes);
        let intros = IntroQueue::new(config.intro_queue_cap(), config.intro_max_per_source());
        Self {
            config,
            identity,
            our_static_bytes,
            our_mac1,
            rng: ChaCha20Rng::from_seed(rng_seed),
            outputs: VecDeque::new(),
            intros,
            guard: TimestampGuard::default(),
            indices: IndexTables::default(),
            statics: StaticMap::default(),
            pendings: BTreeMap::new(),
            last_init_timestamp: None,
            next_connection: 0,
        }
    }

    /// The identity this endpoint runs on.
    pub(crate) fn identity(&self) -> &I {
        &self.identity
    }

    /// Our static's canonical §2.4 octets.
    pub(crate) fn our_static(&self) -> &[u8] {
        &self.our_static_bytes
    }

    /// §17.1's recorded greatest initiation timestamp for a peer static.
    ///
    /// `None` covers both "no entry" and "an entry holding no record" — a
    /// pin-only entry left behind by mitigation (i)'s revert. The two are
    /// deliberately indistinguishable here because they are
    /// indistinguishable to the protocol: neither refuses any timestamp.
    pub(crate) fn greatest(&self, peer_static: &[u8]) -> Option<Timestamp> {
        self.guard.greatest(peer_static)
    }

    /// §17.1's live pin count for a peer static.
    ///
    /// Exists for one reason, stated plainly: the pin **count** is otherwise
    /// unobservable, and an over-release — a pending releasing a pin it
    /// never took — has no other consequence a test can reach. Its one
    /// visible effect is early aging, and ruling 70's own alias masks that:
    /// `TS_GUARD_ORPHAN_TTL == INTRO_TTL`, so the only pin holder that can
    /// coexist with a cancelled dial for the same static is a staged
    /// mid-state, whose expiry reverts its provisional record at exactly
    /// the instant the wrongly-unpinned entry would have aged out. A
    /// correct core and a broken one answer identically at every otherwise
    /// observable moment.
    ///
    /// So this is not convenience: without it that regression cannot be
    /// written at all.
    pub(crate) fn guard_pins(&self, peer_static: &[u8]) -> u32 {
        self.guard.pins(peer_static)
    }

    /// §17.4's `replacement_basis` for a peer static.
    ///
    /// `None` — this endpoint holds no connection for that static.
    /// `Some(None)` — **we dialled** it, and won or was never raced.
    /// `Some(Some(t))` — **we responded** to it, at initiation timestamp `t`
    /// — including a dial that **lost** §6.7's tie-break and installed as
    /// responder over §6.6 step 4.
    ///
    /// Not part of §16.4's API. §6.4's proven-LIVE admission — the basis's
    /// only reader — is still a later slice, so without an accessor the
    /// field is written and never checked, and a value written wrongly
    /// today would surface as a bug in that slice. `pub(crate)`, so it
    /// costs the public surface nothing.
    pub(crate) fn replacement_basis(&self, peer_static: &[u8]) -> Option<Option<Timestamp>> {
        self.statics
            .get(peer_static)
            .map(|entry| entry.replacement_basis)
    }

    /// §17.4's hint set: **the pending tables' dialled addresses alone**.
    ///
    /// A projection over the static map rather than a stored set — §17.4
    /// defines the hint set as being those addresses, so deriving it makes
    /// "established connections contribute no hints" true by construction.
    ///
    /// §6.5 step 2 consults the same projection through
    /// [`is_hinted`](Self::is_hinted); this accessor is the *observable*
    /// form of it, sorted so a test can compare a set without depending on
    /// `HashMap` iteration order.
    pub(crate) fn hints(&self) -> Vec<SocketAddr> {
        let mut hints: Vec<SocketAddr> = self.statics.hints().collect();
        hints.sort_unstable();
        hints
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.4's drain
    // ═══════════════════════════════════════════════════════════════════

    /// Drain one output. Terminates in `Timeout`, which announces the next
    /// deadline (§16.4, §16.5).
    pub(crate) fn poll_output(&mut self) -> EndpointOutput<I::Suite> {
        match self.outputs.pop_front() {
            Some(output) => output,
            None => EndpointOutput::Timeout(self.deadline()),
        }
    }

    /// §16.5, verbatim: *"The endpoint core's deadline is the min over its
    /// pendings' retransmit/give-up deadlines, the parked intros'
    /// expiries, and the timestamp-guard orphan aging (§17.1)."*
    ///
    /// That sentence is the complete list of endpoint-core timers, so this
    /// is a scan over exactly three families. A timer wheel buys nothing at
    /// these cardinalities; slice 3 may replace it when it builds §16.5's
    /// timer table.
    ///
    /// The guard's own deadline needs no clock at all: ruling 73 stamps an
    /// orphan with the instant its last **key-holder** pin was released,
    /// and ruling 80 gives every releasing verb a `now` to stamp with, so
    /// the deadline is a stored value plus a constant.
    fn deadline(&self) -> Option<Instant> {
        let pendings = self
            .pendings
            .values()
            .map(|p| p.next_retransmit.min(p.give_up_at))
            .min();
        let intros = self.intros.next_deadline();
        let orphans = self.guard.next_orphan_deadline();

        [pendings, intros, orphans].into_iter().flatten().min()
    }

    fn emit(&mut self, output: EndpointOutput<I::Suite>) {
        self.outputs.push_back(output);
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.6 — the seeded RNG
    // ═══════════════════════════════════════════════════════════════════

    /// §16.6's per-connection sub-seed, **drawn even while unused**.
    ///
    /// The parenthesis in §16.6 is written for exactly this slice: the
    /// connection core uses no randomness until slice 4, and omitting the
    /// draw now would silently shift every seeded test's index and jitter
    /// sequence when slice 4 added it.
    fn draw_sub_seed(&mut self) -> [u8; 32] {
        let mut seed = [0u8; 32];
        self.rng.fill_bytes(&mut seed);
        seed
    }

    /// §5.5 rule 2's `RETRANSMIT_BASE + U[0, RETRANSMIT_JITTER_MAX]`.
    ///
    /// A modulo draw. The bias against a 333 ms bound from a 32-bit draw is
    /// on the order of 2⁻²⁴ and this is scheduling jitter, not key
    /// material; the unpredictability requirement in §16.6 is on **indices**.
    fn draw_retransmit_delay(&mut self) -> Duration {
        let span = constants::RETRANSMIT_JITTER_MAX.as_nanos() as u64 + 1;
        let jitter = u64::from(self.rng.next_u32()) % span;
        constants::RETRANSMIT_BASE + Duration::from_nanos(jitter)
    }

    fn next_connection_id(&mut self) -> ConnectionId {
        let id = ConnectionId::from_raw(self.next_connection);
        self.next_connection += 1;
        id
    }

    // ═══════════════════════════════════════════════════════════════════
    // §17.2 — the outbound timestamp
    // ═══════════════════════════════════════════════════════════════════

    /// §5.3's initiation timestamp: the wall clock, **forced strictly
    /// greater** than the last this endpoint emitted.
    ///
    /// Endpoint-global (§17.2), so a retransmit stays admissible when a
    /// coarse clock has not advanced and close-and-reconnect still emits
    /// strictly greater. The forcing step is one nanosecond because that is
    /// the only unit the 12-byte encoding has.
    fn draw_timestamp(&mut self) -> Timestamp {
        let wall = self.config.clock().now();
        let forced = match self.last_init_timestamp {
            Some(previous) if wall <= previous => previous.succ(),
            _ => wall,
        };
        self.last_init_timestamp = Some(forced);
        forced
    }

    // ═══════════════════════════════════════════════════════════════════
    // §5.5 — outbound initiation
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `mint_pending` — the first half of ruling 90's split.
    ///
    /// Refuses a static that already has a connection — live or still
    /// dialling. `AlreadyConnected` for a live one is documentation
    /// obligation #1 ("reconnecting is `close()` then dial"); for a
    /// *pending* one it is forced by §16.1, since admitting a second dial
    /// would let both complete and leave two sessions on one static.
    ///
    /// # It costs **0 DH**, and that is the whole point (**ruling 90**)
    ///
    /// Nothing here opens the identity or touches hiss: it draws an id, a
    /// sub-seed and the peer's mac1 key, writes §5.4's PENDING row and takes
    /// §17.1's pin. §6.1's two initiator DH are
    /// [`start_attempt`](Self::start_attempt)'s, which §6.2 puts on the
    /// driver task.
    ///
    /// That split is what lets §16.2's non-`async` `connect()` answer
    /// §16.1's NONE/PENDING/LIVE test **from this map**, at the instant of
    /// the call, instead of from a shell-side mirror of it. Ruling 87
    /// described this factoring; before ruling 90 the code did not have it,
    /// and the price was a second record of a security invariant.
    ///
    /// # The pending exists before its first attempt does
    ///
    /// Between this call and the first [`start_attempt`](Self::start_attempt)
    /// the pending is real — it holds the static, the pin and the give-up
    /// deadline — but `attempted` is `false`, no index is minted and no msg1
    /// exists. `next_retransmit` is `now`, so §16.5's retransmit pass builds
    /// the first attempt if nothing else does, and a give-up reached in that
    /// state reports [`ConnectError::Local`] (ruling 72), which is the
    /// truthful verdict for a dial no peer was ever told about.
    pub(crate) fn mint_pending(
        &mut self,
        now: Instant,
        remote: SocketAddr,
        remote_static: PublicKeyOf<I>,
    ) -> Result<(ConnectionId, Connection<I::Suite>), ConnectError> {
        let key = remote_static.as_ref().to_vec();
        if self.statics.get(&key).is_some() {
            return Err(ConnectError::AlreadyConnected);
        }

        let conn = self.next_connection_id();
        let sub_seed = self.draw_sub_seed();
        let peer_mac1 = Mac1Key::derive(&key);

        let mut pending = Pending {
            conn,
            remote,
            remote_static,
            remote_static_bytes: key.clone(),
            peer_mac1,
            sender_index: None,
            state: None,
            next_retransmit: now,
            give_up_at: now + constants::HANDSHAKE_GIVEUP,
            attempt_spent: false,
            attempted: false,
            guard_pinned: false,
        };

        self.statics.insert(
            key.clone(),
            StaticEntry {
                conn,
                state: StaticState::Pending,
                dialled: Some(remote),
                // §17.4: `None` while this is merely a dial. The install
                // writes the real answer — `None` again for a msg2
                // completion, `Some(t)` for §6.6 step 4's admission.
                replacement_basis: None,
                // No tie-break has written this static's guard entry.
                guard_exempt: false,
            },
        );
        // §17.1: an in-flight outbound pending pins its static's guard
        // entry — and a pin never creates one, so for a static we have
        // only ever dialled this is a no-op, which is exactly §17.1's
        // "we hold no entry at all".
        //
        // Ruling 77 names an in-flight outbound pending among the
        // key-holder pins: it exists because *this* application asked for
        // it, and no remote party can mint one.
        pending.guard_pinned = self.guard.pin(&key, guard::PinKind::KeyHolder);

        self.pendings.insert(conn, pending);

        Ok((conn, Connection::connecting(sub_seed)))
    }

    /// §16.4's `start_attempt` — the second half of ruling 90's split, and
    /// where §6.1's **2 initiator DH** are spent.
    ///
    /// A no-op for an unknown `conn`, and that is a live case rather than
    /// defensive decoration: ruling 50's cancel retires the pending
    /// **synchronously**, in the same instant the `Connecting` is dropped,
    /// so a `connect()` cancelled before the driver ran finds nothing to
    /// attempt here — and spends nothing, and puts no msg1 on the wire.
    pub(crate) fn start_attempt(&mut self, now: Instant, conn: ConnectionId) {
        let Some(mut pending) = self.pendings.remove(&conn) else {
            return;
        };
        self.build_attempt(now, &mut pending);
        self.pendings.insert(conn, pending);
    }

    /// Build and send one attempt, and arm the next retransmit.
    ///
    /// The same function serves the first send and every retransmit, which
    /// is what makes §5.5's "every retransmit is a completely fresh
    /// initiation" true by construction rather than by discipline.
    fn build_attempt(&mut self, now: Instant, pending: &mut Pending<I>) {
        // Retire the previous attempt's route first: §5.5 requires a
        // completion to match the *current* attempt's index, so a msg2 for
        // a superseded attempt must stop routing.
        if let Some(previous) = pending.sender_index.take() {
            self.indices.remove_pending(previous);
        }
        pending.state = None;
        pending.attempt_spent = false;
        pending.next_retransmit = now + self.draw_retransmit_delay();

        let sender_index = self.indices.mint(&mut self.rng);
        let timestamp = self.draw_timestamp();

        // A local provider failure — an enclave that is locked, a key
        // handle that will not open — does **not** end the dial here.
        // Ruling 72 is explicit that a local fault is *ours* and
        // **transient**, and S21 treats a momentarily locked enclave as
        // expected; killing a dial on one bad interval would convert the
        // transient into a terminal, which is not what a taxonomy fix is
        // for. So the attempt is simply not built and the train continues —
        // the next retransmit tries again, on a fresh `open()`.
        //
        // What the ruling *does* buy is the verdict at the end of the
        // train: `attempted` stays `false` while no msg1 has reached the
        // wire, and a give-up in that state reports `ConnectError::Local`
        // rather than blaming a peer that was never sent anything. See
        // `expire_pendings`.
        //
        // Ruling 79: the detail the variant cannot carry rides §18.2's
        // `slither::io` instead — the provider's own error and the verb
        // that met it. The verb is `connect`: §5.5's retransmits all belong
        // to the one dial the application asked for.
        let (provider, our_key) = match self.identity.open() {
            Ok(opened) => opened,
            Err(error) => {
                tracing::warn!(
                    target: "slither::io",
                    verb = "connect",
                    stage = "Identity::open",
                    conn = ?pending.conn,
                    %error,
                    "the identity provider failed to open"
                );
                return;
            }
        };
        let state = <I::Suite as Handshake>::initiator(
            provider,
            constants::PROLOGUE,
            pending.remote_static.clone(),
        );
        // Also local: this is our own static's DH under hiss, not anything
        // the peer contributed — nothing has been received at this point.
        let (msg1, sent) =
            match <I::Suite as Handshake>::write_msg1(state, our_key, &timestamp.encode()) {
                Ok(written) => written,
                Err(error) => {
                    tracing::warn!(
                        target: "slither::io",
                        verb = "connect",
                        stage = "Handshake::write_msg1",
                        conn = ?pending.conn,
                        %error,
                        "msg1 would not write on our static"
                    );
                    return;
                }
            };

        let data = handshake::frame_init(sender_index, &msg1, &pending.peer_mac1);
        pending.attempted = true;
        self.indices.insert_pending(sender_index, pending.conn);
        pending.sender_index = Some(sender_index);
        pending.state = Some(Box::new(sent));
        self.emit(EndpointOutput::Transmit(Transmit {
            to: pending.remote,
            data,
        }));
    }

    /// Release every trace of a pending. Shared by give-up, cancel, and
    /// §6.4's PENDING branch on the loser side.
    ///
    /// **§17.1's `HANDSHAKE_GIVEUP` extension is armed here**, before the
    /// unpin, when the row says a tie-break wrote this static's guard
    /// entry. §17.1 dates that window from the connection's death, and for
    /// a pending this *is* the death — the give-up expiry that §17.1 names
    /// explicitly ("if that outbound never completed"), or a cancellation.
    /// Arming before releasing also keeps `unpin` from deleting the entry
    /// outright when the record is the only thing on it.
    fn drop_pending(&mut self, now: Instant, conn: ConnectionId) -> Option<Pending<I>> {
        let pending = self.pendings.remove(&conn)?;
        if let Some(index) = pending.sender_index {
            self.indices.remove_pending(index);
        }
        let exempt = self
            .statics
            .remove(&pending.remote_static_bytes)
            .is_some_and(|entry| entry.guard_exempt);
        if exempt {
            self.extend_guard_exemption(now, &pending.remote_static_bytes);
        }
        if pending.guard_pinned {
            self.guard
                .unpin(&pending.remote_static_bytes, guard::PinKind::KeyHolder, now);
        }
        Some(pending)
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.4 — inbound datagrams
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `handle_datagram`.
    pub(crate) fn handle_datagram(
        &mut self,
        now: Instant,
        src: SocketAddr,
        datagram: &[u8],
    ) -> Disposition {
        // §3.1's gate: `None` **is** the drop — no error, no trace, no
        // counter, and nothing that distinguishes it from a datagram the
        // endpoint handled itself.
        let Some(inbound) = classify::<I::Suite>(datagram) else {
            return Disposition::Done;
        };

        match inbound {
            Inbound::Init {
                header,
                msg1,
                preimage,
                mac1,
            } => {
                // §6.5 step 1 ends here: length gate, classify, mac1 —
                // one keyed hash, 0 DH, and a failure is the silent drop.
                if !self.our_mac1.verify(preimage, mac1) {
                    return Disposition::Done;
                }
                // §6.5 steps 2–3. **Still 0 DH unless `src` is a hint**,
                // which is the whole design of the hint check: an endpoint
                // with no dial in flight reaches `park_initiation` having
                // spent nothing.
                self.route_initiation(now, src, header.sender_index, msg1);
                Disposition::Done
            }

            Inbound::Resp {
                header,
                msg2,
                preimage,
                mac1,
            } => {
                self.complete_initiation(
                    header.receiver_index,
                    header.sender_index,
                    msg2,
                    preimage,
                    mac1,
                );
                Disposition::Done
            }

            Inbound::Data { header, .. } => match self.indices.session(header.receiver_index) {
                Some(conn) => Disposition::ForConnection(conn),
                None => Disposition::Done,
            },
        }
    }

    /// §6.3's arrival, and the `IntroReady` it may surface.
    fn park_initiation(&mut self, now: Instant, src: SocketAddr, sender_index: u32, msg1: &[u8]) {
        let outcome = self.intros.arrive(now, src, sender_index, msg1);
        if let Some(evicted) = outcome.evicted {
            // Only unconsumed entries are evictable, and an unconsumed
            // entry has neither a provisional guard write nor a pin — but
            // release both anyway rather than assert, because a leak here
            // is a permanent, silent denial for a real peer.
            self.release_chain_guard_state(now, evicted.guard_undo, evicted.guard_pin);
        }
        match outcome.arrival {
            Arrival::Parked(id) => self.emit(EndpointOutput::IntroReady(id, src)),
            // §6.3 rule 5: a replacement is transparent — same `IntroId`,
            // newest bytes, and **no second surfacing**.
            Arrival::Refreshed(_) | Arrival::Dropped => {}
        }
    }

    /// §5.5 rules 3–5: complete a pending from its msg2.
    ///
    /// The order is what makes §5.5's claim true — *"a guessed-index or
    /// mac1-invalid msg2 can never spend anything"*:
    ///
    /// 1. exact length (already done by §3.1's gate, ruling 65);
    /// 2. **index** match against the current attempt — and **the source
    ///    address is deliberately ignored**, because the initiator anchors
    ///    at the address it dialled and the peer roams in later (§7.3);
    /// 3. mac1, against **our** key, since we are the recipient;
    /// 4. the interval's attempt, if already spent, drops;
    /// 5. the attempt is spent **before** the crypto runs, so a failed
    ///    completion costs the interval and the next retransmit refreshes
    ///    it.
    fn complete_initiation(
        &mut self,
        receiver_index: u32,
        peer_index: u32,
        msg2: &[u8],
        preimage: &[u8],
        mac1: &[u8],
    ) {
        let Some(conn) = self.indices.pending(receiver_index) else {
            return;
        };
        if !self.our_mac1.verify(preimage, mac1) {
            return;
        }

        // Take the attempt's state, spending the interval, before any
        // crypto runs. Everything the completion needs is copied out here
        // so the pending's borrow ends before the endpoint's tables move.
        let (state, our_index, anchor, key) = {
            let Some(pending) = self.pendings.get_mut(&conn) else {
                return;
            };
            if pending.attempt_spent || pending.sender_index != Some(receiver_index) {
                return;
            }
            pending.attempt_spent = true;
            let Some(state) = pending.state.take() else {
                return;
            };
            (
                state,
                receiver_index,
                pending.remote,
                pending.remote_static_bytes.clone(),
            )
        };

        let Ok(transport) = <I::Suite as Handshake>::read_msg2(*state, msg2) else {
            // The attempt stays spent and the state is gone — `read_msg2`
            // consumes it either way. Not fatal: the next scheduled
            // retransmit builds a completely fresh initiation, and the
            // train still ends at `HANDSHAKE_GIVEUP`.
            return;
        };

        let (seal, open) = <I::Suite as Handshake>::into_datagram(transport, self.epoch_size());

        // §5.5 rule 5: our receiver index is our own `sender_index`; the
        // peer's is the response's. §5.5 rule 4: the anchor is the address
        // we **dialled**, not wherever the msg2 came from.
        let session = EstablishedSession {
            seal,
            open,
            our_index,
            peer_index,
            anchor,
        };

        self.pendings.remove(&conn);
        self.indices.remove_pending(our_index);
        self.indices.insert_session(our_index, conn);
        // The static stays claimed by this connection. §17.4: `None` when
        // we dialled — "a `connect()` completed by msg2 … neither teaches
        // us any timestamp of the peer's, because msg2 carries no payload".
        // §6.6 step 4 is the other way a dialled row installs, and it is
        // the one that writes `Some(t)`.
        self.statics.promote(&key, None);

        self.emit(EndpointOutput::ToConnection(conn, Install { session }));
    }

    /// §7.7's epoch size, for hiss's ratcheting datagram split.
    ///
    /// Config-supplied (**ruling 82**), defaulting to `REKEY_EPOCH_MSGS`.
    /// The override is a **test-only** facility and
    /// [`Config::with_epoch_size`](crate::Config::with_epoch_size) says so
    /// in the terms §16.6 uses for the RNG seed; the ratchet itself is
    /// hiss's, and nothing in this crate inspects or drives an epoch.
    fn epoch_size(&self) -> NonZeroU64 {
        self.config.epoch_size()
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.5 — timers
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `handle_timeout`. **Idempotent**: every due deadline is
    /// disarmed or advanced before its logic runs, so calling twice at one
    /// instant is a no-op the second time.
    ///
    /// # §16.5's equal-deadline order, exhaustive and normative (**ruling 76**)
    ///
    /// The four endpoint deadline families run in this order and no other:
    ///
    /// 1. **handshake give-up** — [`expire_pendings`](Self::expire_pendings)
    /// 2. **intro expiry** — §6.3 rule 4
    /// 3. **guard-orphan aging** — §17.1 mitigation (ii)
    /// 4. **retransmit** — [`retransmit_pendings`](Self::retransmit_pendings)
    ///
    /// The governing principle, from which all four follow: **a terminal
    /// outcome precedes a routine one, and state removal precedes
    /// emission.** Give-up beating a same-instant retransmit — the one pair
    /// §16.5 ordered before ruling 76 — is an instance of it, not a special
    /// case, which is why the give-up sweep is now a phase of its own
    /// rather than an arm inside the retransmit loop. §16.4 makes generation
    /// order normative, so an unordered pair here would make that claim
    /// hollow exactly where two timers collide.
    ///
    /// Ruling 73 removed the one interaction that made (1) versus (3)
    /// contentious: a give-up releases its pin, and orphan aging now runs
    /// from that release, so step (3) grants the entry a **fresh** window
    /// instead of finding an expired one.
    pub(crate) fn handle_timeout(&mut self, now: Instant) {
        // (1) Terminal, and it removes state before anything else emits.
        self.expire_pendings(now);

        // (2) §6.3 rule 4: silent eviction, emitting nothing — but a
        // consumed chain's provisional guard write must still be reverted,
        // or a real peer is left blocked by a record they never got to use.
        // Before (3), because it releases guard pins that (3) then ages.
        for expired in self.intros.expire(now) {
            self.release_chain_guard_state(now, expired.guard_undo, expired.guard_pin);
        }

        // (3) §17.1 mitigation (ii).
        self.guard.age_orphans(now);

        // (4) Routine, and the only step of the four that transmits.
        self.retransmit_pendings(now);
    }

    /// §16.5 step (1): the handshake give-up, which is terminal.
    ///
    /// Runs as a complete phase before any retransmit is built, so a
    /// give-up cannot be interleaved behind another pending's routine
    /// send (ruling 76).
    fn expire_pendings(&mut self, now: Instant) {
        let due: Vec<ConnectionId> = self
            .pendings
            .iter()
            // Normative: **give-up beats a same-instant retransmit**. The
            // comparison is `<=`, so an equality is a give-up and the
            // pending is gone before step (4) can look at it.
            .filter(|(_, p)| p.give_up_at <= now)
            .map(|(id, _)| *id)
            .collect();

        for conn in due {
            // Ruling 72: a train that never got one msg1 onto the wire
            // failed *locally* — there is no peer to have timed out. A
            // train that transmitted and was not answered is a genuine
            // `TimedOut`, whatever happened on the intervals in between.
            let attempted = self.pendings.get(&conn).is_some_and(|p| p.attempted);
            let _ = self.drop_pending(now, conn);
            let why = if attempted {
                ConnectError::TimedOut
            } else {
                ConnectError::Local
            };
            self.emit(EndpointOutput::HandshakeFailed(conn, why));
        }
    }

    /// §16.5 step (4): the routine retransmit.
    ///
    /// Every pending still here has already survived step (1), so no
    /// give-up check is needed — that is what makes the order a property of
    /// the code rather than of a comment.
    fn retransmit_pendings(&mut self, now: Instant) {
        let due: Vec<ConnectionId> = self
            .pendings
            .iter()
            .filter(|(_, p)| p.next_retransmit <= now)
            .map(|(id, _)| *id)
            .collect();

        for conn in due {
            let Some(mut pending) = self.pendings.remove(&conn) else {
                continue;
            };
            self.build_attempt(now, &mut pending);
            self.pendings.insert(conn, pending);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §16.4 — connection events
    // ═══════════════════════════════════════════════════════════════════

    /// §16.4's `handle_connection_event`.
    ///
    /// `Retired` is a MUST on teardown: it drops the index route **and**
    /// releases the guard-entry pin, and without it both leak for the
    /// endpoint's life.
    ///
    /// It is also **S29's cancellation path**. §16.4 lists no cancel verb,
    /// and `Retired`'s documented effects are exactly cancellation's, so
    /// the shell reaches for this rather than for something invented:
    /// dropping a `Connecting` retires the connection, which here stops the
    /// retransmit train, frees the index and releases the static so the
    /// very next `connect()` to it succeeds instead of returning
    /// `AlreadyConnected`. It emits **nothing** — the `Connecting` is
    /// already resolved by its own drop, and a `HandshakeFailed` would be a
    /// second resolution.
    pub(crate) fn handle_connection_event(
        &mut self,
        now: Instant,
        id: ConnectionId,
        ev: ToEndpoint,
    ) {
        match ev {
            ToEndpoint::Retired { our_index } => {
                self.indices.remove_session(our_index);
                self.indices.remove_pending(our_index);
                // Ruling 73: this is the release that starts the orphan
                // clock — and ruling 80 is why this verb has a `now` to
                // stamp it with. §16.4's own invariant already required
                // one: this is a mutating call.
                if self.drop_pending(now, id).is_none()
                    && let Some((key, entry)) = self.statics.remove_by_connection(id)
                {
                    // §17.1's extension, dated from this connection's death
                    // — the instant the bullet names. Armed before the
                    // release, for the reason `drop_pending` states.
                    if entry.guard_exempt {
                        self.extend_guard_exemption(now, &key);
                    }
                    // A live connection is a key-holder pin (ruling 77):
                    // reaching it took the peer's key, and this release is
                    // the one ruling 73's security argument is about.
                    self.guard.unpin(&key, guard::PinKind::KeyHolder, now);
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Shared teardown
    // ═══════════════════════════════════════════════════════════════════

    /// Undo §17.1's provisional write and release the mid-state pin.
    ///
    /// Every path that ends a chain **without** accepting it runs this:
    /// `reject()`, expiry, eviction, a failed `authenticate()`, and an
    /// `accept()` that returns `Stale`. Mitigation (i) is explicit that the
    /// revert applies to all of them.
    ///
    /// **The pin is released first, and the order is load-bearing.** A
    /// chain that authenticated a static nobody had recorded before *both*
    /// created the entry and pinned it, so reverting first would find the
    /// chain's own pin still held, decline to remove the entry it had just
    /// created, and leave precisely the orphan mitigation (i) exists to
    /// prevent — silently, and only for the authenticate-then-reject path
    /// that is the whole point of the mitigation.
    fn release_chain_guard_state(
        &mut self,
        now: Instant,
        undo: Option<guard::GuardUndo>,
        pin: Option<guard::ChainPin>,
    ) {
        if let Some(pin) = pin {
            // Ruling 77: the kind travels on the pin. A chain refused at
            // `authenticate()` — including a `Replay` — never reached
            // `Proven`, so its pin is still `Claimed` and its release moves
            // no timer.
            self.guard.unpin(&pin.key, pin.kind, now);
        }
        if let Some(undo) = undo {
            self.guard.revert(undo);
        }
    }
}

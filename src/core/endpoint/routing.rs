//! §6.5's routing rule and §6.6's internal tie-break completion.
//!
//! Everything an inbound `HandshakeInit` meets between mac1 and the stage-0
//! queue, plus the one admission path the application never sees.
//!
//! # §6.5, as four steps and two of them free
//!
//! | Step | Cost | Outcome |
//! |---|---|---|
//! | 1 — length gate, classify, mac1 | one keyed hash | a failure is a silent drop |
//! | 2 — the **hint check** | **0 DH** | `src` ∉ hints ⇒ park at stage 0, surface an `Intro` |
//! | 3 — the **eager path** | 1 DH (`es`) | claim ∈ pending outbound remotes ⇒ §6.6; otherwise **demote**, carrying the paid mid-state |
//! | 4 — the `read_identity()` interception | — | **not in this slice; see below** |
//!
//! Step 1 is [`Endpoint::handle_datagram`]'s; steps 2 and 3 are
//! [`route_initiation`](Endpoint::route_initiation).
//!
//! # The hint set is a projection, and it answers "pending outbound remote" too
//!
//! §17.4 defines the hint set as "the pending tables' dialled addresses",
//! and §6.5 defines the probed set as "pending outbound remotes **only**".
//! Both are read off the **one** static map ([`StaticMap`]): a row in
//! [`StaticState::Pending`] contributes its `dialled` address to the first
//! and its key to the second. That is why "established connections
//! contribute no hints" needs no filter — an `accept()`-installed row is
//! `Live` with `dialled: None` — and why §6.5's routing and §6.4's PENDING
//! branch cannot disagree about which statics are PENDING: there is one
//! map, and ruling 90 made it the only one.
//!
//! A pending is a pending **from `mint_pending`**, before its first msg1
//! exists. §6.5 says "in-flight outbound initiations" and §6.7 says "an
//! in-flight outbound pending"; neither excludes the window between
//! ruling 90's two halves, and excluding it would make §6.5 route a
//! crossing msg1 to the staged path while §6.4's PENDING branch — reading
//! the same row — still called that static PENDING. One map, one answer.
//!
//! # §6.6's two routes to one comparison
//!
//! §6.6 is explicit that PENDING is not exclusive to the internal path: a
//! chain staged while its static was NONE and accepted after a `connect()`
//! made that static PENDING reaches **§6.4's PENDING branch** instead, and
//! that branch "is a **different route to the same comparison**, not an
//! exemption from it". So the comparison itself
//! ([`wins_tiebreak`](Endpoint::wins_tiebreak)), the winner-side record
//! ([`record_tiebreak_timestamp`](Endpoint::record_tiebreak_timestamp)) and
//! the loser-side pending cancellation are written **once, here**, and
//! `staged.rs`'s `accept()` calls the same three. Two copies of a
//! two-sided agreement is how the two sides come to disagree.
//!
//! # Step 4 is **not implemented in this slice**, and why
//!
//! §6.5 step 4 — the `read_identity()` interception — requires
//! [`internal_tiebreak`](Endpoint::internal_tiebreak) to run inside
//! `read_identity()`. §6.6 steps 3 and 4 both **record** a §17.1 guard
//! entry, and [`TimestampGuard::record`](super::guard::TimestampGuard::record)
//! takes the instant: it stamps mitigation (iii)'s LRU recency, stamps
//! `orphaned_at` on an entry it creates, and drives the LRU sweep. **§16.4
//! gives `read_identity` no `now`** — it is the one staged verb without
//! one — and ruling 80 forbids inventing one ("no watermark, no
//! approximation and no floor to correct one"). This is ruling 80's own
//! defect a third time: §16.4 states "a `now` on every mutating call" and
//! then omits it from a mutating call.
//!
//! **Nothing diverges meanwhile.** A chain step 4 would have swallowed is
//! parked, surfaces as an `Intro`, and reaches §6.6's other route — §6.4's
//! PENDING branch — at `accept()`, which "can never disagree" with this
//! one. What is lost is that the application *sees* the introduction and
//! makes a decision §6.5 wanted the endpoint to make for it, so §6.5's
//! false-negative case (a peer whose source port a NAT rewrote) now needs
//! the application to do what §6.5 already tells it to do: "**Applications
//! that dial SHOULD also drain `accept()`**".
//!
//! [`StaticMap`]: super::tables::StaticMap
//! [`StaticState::Pending`]: super::tables::StaticState::Pending

use std::net::SocketAddr;
use std::time::Instant;

use crate::constants;
use crate::core::endpoint::handshake as framing;
use crate::core::{ConnectionId, EndpointOutput, EstablishedSession, Install, Timestamp, Transmit};
use crate::error::ConnectError;
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::{Handshake, Mac1Key};

use super::Endpoint;
use super::guard::{ChainPin, PinKind};
use super::intro_queue::Arrival;
use super::staged::{ChainState, MidState};
use super::tables::StaticState;

/// What §6.5 step 3's split intro read produced.
///
/// Three outcomes, not two, and the split is ruling 72's: a **local**
/// failure is ours and transient at 0 DH, a **hiss** failure is the peer's
/// bytes and definitive at 1 DH. §6.5 states neither, and collapsing them
/// would make our locked enclave cost the peer its introduction.
enum EagerRead<I: Identity> {
    /// The claim was read. 1 DH (`es`) is spent and the mid-state holds
    /// the unpaid `ss`.
    Read {
        /// The **claimed** static — attacker-choosable until `ss` (§6.1).
        claimed: PublicKeyOf<I>,
        /// hiss's suspended read.
        mid: Box<MidState<I>>,
    },
    /// **Our** provider or responder machine would not build. 0 DH.
    Local,
    /// hiss refused the bytes. 1 DH, and the verdict is definitive.
    Malformed,
}

impl<I: Identity> Endpoint<I> {
    // ═══════════════════════════════════════════════════════════════════
    // §6.5 — the routing rule
    // ═══════════════════════════════════════════════════════════════════

    /// §6.5 steps 2 and 3, for a datagram that has already passed step 1.
    ///
    /// The **only** caller is [`handle_datagram`](Endpoint::handle_datagram)'s
    /// `Init` arm, which has done the length gate, the classification and
    /// mac1. Every path below either parks (surfacing an `Intro`), routes
    /// to §6.6, or drops silently; none returns a value, because §16.4's
    /// `handle_datagram` answers `Disposition::Done` for every initiation.
    pub(super) fn route_initiation(
        &mut self,
        now: Instant,
        src: SocketAddr,
        sender_index: u32,
        msg1: &[u8],
    ) {
        // §6.5 step 2 — the hint check, at **0 DH**. The hint's one job is
        // to catch the crossing msg1 of a simultaneous open cheaply, so
        // the common case (nobody is dialling) leaves this function having
        // done one address comparison per in-flight dial and nothing else.
        if !self.is_hinted(src) {
            self.park_initiation(now, src, sender_index, msg1);
            return;
        }

        // §6.5 step 3 — the eager path. 1 DH (`es`), spent before we know
        // whose static this claims, which is what buys the answer.
        match self.eager_read(msg1) {
            // Ruling 72: the fault is *ours* and may well be transient, so
            // the peer's initiation is not destroyed by it. Parking is the
            // conservative fallback — the packet is intact, we merely
            // could not pay for it, and the staged path can pay later.
            EagerRead::Local => self.park_initiation(now, src, sender_index, msg1),
            // Ruling 72's other half: the peer's bytes are at fault and the
            // verdict is definitive. §3.1's silent drop — parking it would
            // hand the application a chain whose only possible outcome is
            // `IntroError::Malformed` at a second DH.
            EagerRead::Malformed => {}
            EagerRead::Read { claimed, mid } => {
                match self.pending_outbound_remote(claimed.as_ref()) {
                    // "The packet never touches the accept queue and the
                    // application never sees it."
                    // Unboxed on the way in: §6.6 consumes the mid-state at
                    // step 1 and never stores it, so the box would buy one
                    // allocation and no indirection anyone reads. The
                    // demotion below is the opposite — the box *is* what
                    // the chain holds.
                    Some(conn) => {
                        self.internal_tiebreak(now, src, sender_index, conn, claimed, *mid);
                    }
                    // "A peer sharing a source with a dialled address still
                    // surfaces as an `Intro`."
                    None => self.demote(now, src, sender_index, msg1, claimed, mid),
                }
            }
        }
    }

    /// §6.5 step 2's membership test: is `src` in §17.4's hint set?
    ///
    /// A linear scan over the static map's `dialled` addresses, which is
    /// the hint set's definition rather than a cache of it. The set is
    /// bounded by the number of concurrent dials an application has open,
    /// so a scan is the whole structure; a second map keyed on address
    /// would also be a second thing keyed on an *unauthenticated*
    /// quantity, which §6.1 permits exactly once (§6.3's queue) and
    /// `intro_queue`'s module docs make a review criterion.
    fn is_hinted(&self, src: SocketAddr) -> bool {
        self.statics.hints().any(|hint| hint == src)
    }

    /// §6.5's probed set: the **pending outbound remotes only**.
    ///
    /// Answers with the dial's `ConnectionId`, because every caller needs
    /// it — §6.6 step 4 completes *that* connection, and §6.4's loser
    /// branch cancels it.
    ///
    /// `Live` rows deliberately answer `None`: §6.5 is explicit that "a
    /// LIVE static's initiation takes the ordinary staged path", where
    /// §6.4's proven-LIVE `accept()` is the replacement (§5.4).
    pub(super) fn pending_outbound_remote(&self, peer_static: &[u8]) -> Option<ConnectionId> {
        self.statics
            .get(peer_static)
            .filter(|entry| entry.state == StaticState::Pending)
            .map(|entry| entry.conn)
    }

    /// §6.5 step 3's split intro read — §6.1's `es`, off the staged path.
    ///
    /// The same three-way outcome `read_identity()` has, for the same
    /// reasons (ruling 72), minus the queue: there is no chain here yet,
    /// so nothing is discarded and nothing is left parked — the caller
    /// decides what to do with each verdict.
    fn eager_read(&mut self, msg1: &[u8]) -> EagerRead<I> {
        let (provider, our_key) = match self.identity.open() {
            Ok(opened) => opened,
            Err(error) => {
                tracing::warn!(
                    target: "slither::io",
                    verb = "handle_datagram",
                    stage = "Identity::open",
                    %error,
                    "the identity provider failed to open"
                );
                return EagerRead::Local;
            }
        };
        let responder =
            match <I::Suite as Handshake>::responder(provider, constants::PROLOGUE, our_key) {
                Ok(responder) => responder,
                Err(error) => {
                    tracing::warn!(
                        target: "slither::io",
                        verb = "handle_datagram",
                        stage = "Handshake::responder",
                        %error,
                        "the responder machine would not build on our static"
                    );
                    return EagerRead::Local;
                }
            };
        match <I::Suite as Handshake>::read_msg1_intro(responder, msg1) {
            Ok((claimed, mid)) => EagerRead::Read {
                claimed,
                mid: Box::new(mid),
            },
            Err(_) => EagerRead::Malformed,
        }
    }

    /// §6.5 step 3's **demotion**: park under §6.3's rules, carrying the
    /// paid mid-state, tagged identity-already-read.
    ///
    /// The entry lands `Claimed` rather than `Parked`, which is
    /// `intro_queue`'s "freeze-on-carry park" and makes two §6.5 sentences
    /// true at once: it "still surfaces as an `Intro`", and
    /// "`read_identity()` on it returns the cached claim at **0
    /// incremental DH**" — ruling 74's early return answers it from the
    /// chain without opening a provider.
    ///
    /// **Consumed, and it must be.** A chain holding a mid-state can never
    /// be byte-replaced (§6.3 rule 5) — the mid-state was read from
    /// *these* bytes — and `by_addr` holds unconsumed entries only, which
    /// `arrive`'s own `debug_assert` polices. Marking it consumed is the
    /// same step `read_identity()` takes for the same reason.
    fn demote(
        &mut self,
        now: Instant,
        src: SocketAddr,
        sender_index: u32,
        msg1: &[u8],
        claimed: PublicKeyOf<I>,
        mid: Box<MidState<I>>,
    ) {
        let outcome = self.intros.arrive(now, src, sender_index, msg1);
        if let Some(evicted) = outcome.evicted {
            self.release_chain_guard_state(now, evicted.guard_undo, evicted.guard_pin);
        }
        let id = match outcome.arrival {
            Arrival::Parked(id) => {
                self.emit(EndpointOutput::IntroReady(id, src));
                id
            }
            // §6.3 rule 5: transparent, and **no second surfacing**. The
            // refreshed entry was unconsumed, so it was `Parked` and held
            // no guard state of its own; the mid-state below is read from
            // the bytes `arrive` has just written onto it.
            Arrival::Refreshed(id) => id,
            // The source's whole allowance is consumed chains, or the queue
            // is. One `es` is spent for nothing, which is the price §6.5
            // sets by putting the read before the queue; §3.1 makes the
            // drop invisible either way.
            Arrival::Dropped => return,
        };

        // §17.1, exactly as `read_identity()`: a staged mid-state pins its
        // static's guard entry, the pin **never creates** one, and ruling
        // 77 makes it a `Claimed` pin because 1 DH proves nothing.
        let key = claimed.as_ref().to_vec();
        let pinned = self.guard.pin(&key, PinKind::Claimed);

        self.intros.consume(id);
        let entry = self
            .intros
            .get_mut(id)
            .expect("the entry was parked or refreshed immediately above");
        entry.state = ChainState::Claimed { mid, claimed };
        if pinned {
            entry.guard_pin = Some(ChainPin {
                key,
                kind: PinKind::Claimed,
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.6 — the internal tie-break completion
    // ═══════════════════════════════════════════════════════════════════

    /// §6.6, entire: the endpoint's one internal admission path.
    ///
    /// Runs on the already-paid mid-state, in §6.6's order — **tag, guard,
    /// tie-break, admit** — and every early exit leaves the in-flight
    /// outbound pending untouched. That ordering is the security property:
    /// step 1 is what makes "**a forgery cannot cancel a pending**" true,
    /// and step 2 is what bounds the replay §6.7 documents rather than
    /// closes.
    ///
    /// # Costs, cumulative from the datagram
    ///
    /// | Outcome | DH |
    /// |---|---|
    /// | tag death (step 1) | 2 — `es`, `ss` |
    /// | guard rejection (step 2) | 2 |
    /// | winner-side drop (step 3) | 2 |
    /// | admission (step 4) | **4** — `+ee`, `+se` |
    ///
    /// Four is §6.1's `accept()` price, and it is the same four: this path
    /// is a responder handshake that the application did not drive.
    fn internal_tiebreak(
        &mut self,
        now: Instant,
        src: SocketAddr,
        peer_index: u32,
        conn: ConnectionId,
        claimed: PublicKeyOf<I>,
        mid: MidState<I>,
    ) {
        // ── Step 1: Tag (`ss`, +1 DH) ────────────────────────────────
        // "A forged claim of a pending static dies here at the msg1 tail's
        // AEAD tag. A failure leaves the in-flight outbound pending
        // untouched — nothing unauthenticated can reach the tie-break."
        let Ok((payload, read)) = <I::Suite as Handshake>::complete(mid) else {
            tracing::debug!(
                target: "slither::policy",
                event = "tiebreak_tag_death",
                %src,
                "an initiation claiming a pending static failed msg1's tail tag"
            );
            return;
        };
        let timestamp = Timestamp::decode(&payload);
        // The `ss` has proven possession: the claim is now the peer static.
        let peer_static = claimed.as_ref().to_vec();

        // ── Step 2: Guard (§17.1) ────────────────────────────────────
        // Mitigation (iii): a **failed check** refreshes no recency and
        // writes nothing.
        if !self.guard.admits(&peer_static, timestamp) {
            tracing::debug!(
                target: "slither::policy",
                event = "tiebreak_replay",
                %src,
                "an authenticated initiation for a pending static failed the timestamp guard"
            );
            return;
        }

        // ── Step 3: Tie-break (§6.7) ─────────────────────────────────
        if self.wins_tiebreak(&peer_static) {
            // "Our static smaller ⇒ we are the winner: the authenticated
            // inbound is silently dropped, its timestamp **recorded**
            // (§6.7), and our own outbound completes normally."
            //
            // `read` is dropped here, which is the drop: no msg2 is
            // written and the mid-state goes with it.
            self.record_tiebreak_timestamp(now, conn, &peer_static, timestamp);
            tracing::debug!(
                target: "slither::policy",
                event = "tiebreak_won",
                %src,
                "our static is the smaller: the crossing initiation is dropped and recorded"
            );
            return;
        }

        // ── Step 4: Admit (the loser side) ───────────────────────────
        //
        // **msg2 is written before anything is committed**, and the order
        // is deliberate. §6.6 lists step 4's effects — record, cancel,
        // mint, write — but says nothing about a `write_msg2` that fails,
        // and a failure after the cancellation would destroy a live dial
        // and put nothing in its place. Writing first makes that failure
        // fall under the rule §6.6 *does* state for its other steps: "a
        // silent drop with a trace, the pending untouched, nothing
        // recorded". No observable ordering changes on the success path —
        // the record and the cancellation are not observable relative to
        // each other, and the transmit is emitted last either way.
        let Ok((msg2, transport)) = <I::Suite as Handshake>::write_msg2(read) else {
            tracing::debug!(
                target: "slither::policy",
                event = "tiebreak_msg2_failed",
                %src,
                "msg2 would not write on a lost tie-break; the pending is untouched"
            );
            return;
        };

        // The record is a **full admission** (§17.1's third write site) and
        // is permanent from this instant: there is no chain to hand a
        // `GuardUndo` to and no `accept()` to make it permanent later.
        self.record_tiebreak_timestamp(now, conn, &peer_static, timestamp);

        // §6.7: "we cancel our pending now — post-`ss`, on the
        // authenticated inbound (the pending and its index dropped; no
        // give-up, no error)".
        //
        // Not `drop_pending`: that releases the static row and the §17.1
        // pin, and **both belong to the connection this admission is about
        // to complete** — the same `ConnectionId`, promoted in place.
        // Removing and re-inserting the row would also meet
        // `StaticMap::insert`'s `§16.1: one session per peer static`
        // assertion coming the other way.
        let Some(mut pending) = self.pendings.remove(&conn) else {
            debug_assert!(false, "the pending was read out of the static map above");
            return;
        };
        if let Some(index) = pending.sender_index.take() {
            self.indices.remove_pending(index);
        }
        // hiss's initiator state goes with it: our own msg1 will never be
        // answered now, and a msg2 for it must not route.
        pending.state = None;
        drop(pending);

        // §17.3's responder index, and §5.6's anchor — the **msg1 source**,
        // because on this branch we are the responder.
        let our_index = self.indices.mint(&mut self.rng);
        let (seal, open) = <I::Suite as Handshake>::into_datagram(transport, self.epoch_size());
        // mac1 on the response is keyed on the **recipient's** static — the
        // initiator's, which the `ss` at step 1 proved they hold.
        let peer_mac1 = Mac1Key::derive(&peer_static);
        let data = framing::frame_resp(our_index, peer_index, &msg2, &peer_mac1);

        self.indices.insert_session(our_index, conn);
        // §5.4 PENDING → LIVE **in place**, and §17.4's basis becomes
        // `Some(t)`: "the tie-break loser's admit step (§6.6 step 4)" is
        // named there among the three responder cases.
        self.statics.promote(&peer_static, Some(timestamp));

        tracing::debug!(
            target: "slither::policy",
            event = "tiebreak_admitted",
            %src,
            "the peer's static is the smaller: our pending is cancelled and we install as responder"
        );

        self.emit(EndpointOutput::Transmit(Transmit { to: src, data }));
        // §16.4: `Install` "resolv[es] its `Connecting` — whether from msg2
        // completion or a lost tie-break's admission (§6.7)", **exactly
        // once**. This is that second case, on the connection `connect()`
        // created.
        self.emit(EndpointOutput::ToConnection(
            conn,
            Install {
                session: EstablishedSession {
                    seal,
                    open,
                    our_index,
                    peer_index,
                    anchor: src,
                },
            },
        ));
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.7 — the pieces both routes share
    // ═══════════════════════════════════════════════════════════════════

    /// §6.7's comparison: **the peer with the lexicographically smaller
    /// static public key is the winning initiator.**
    ///
    /// Over §2.4's canonical encoding as unsigned octet strings — "always
    /// equal-length within a suite, and identical to the mac1-keying and
    /// identity-map octets, so the comparison is over the `as_ref()` bytes
    /// directly and needs no `Ord` bound" (Appendix A.3). `[u8]`'s `Ord`
    /// **is** the unsigned lexicographic order, so this is the spec's
    /// sentence and not a rendering of it.
    ///
    /// Equality cannot occur: `connect()` to our own static is out of
    /// scope under §16.1, so a `false` here always means the peer's static
    /// is strictly smaller.
    ///
    /// Called from **both** routes to the comparison — §6.6 step 3 and
    /// §6.4's PENDING branch — which is what §6.6 means by "they can never
    /// disagree".
    pub(super) fn wins_tiebreak(&self, peer_static: &[u8]) -> bool {
        self.our_static() < peer_static
    }

    /// §17.1's two tie-break writes: §6.6 step 4's full admission and
    /// §6.7's winner-side record.
    ///
    /// Both are **permanent on the spot**. The winner's is §17.1's one
    /// write that records without admitting — "the tie-break **winner**'s
    /// side, which authenticates the loser's initiation post-`ss`, drops
    /// it, and records its timestamp anyway" — and §6.4's ordering clause
    /// names it as the single deliberate exception to mitigation (i)'s
    /// revert-on-`Stale`. So the [`GuardUndo`] is dropped rather than
    /// carried: there is nothing left to undo it with.
    ///
    /// # Two things it must also do, neither of which is a write
    ///
    /// **The `HANDSHAKE_GIVEUP` extension is armed here and applied at the
    /// pin's release.** §17.1: an entry written by either of these two
    /// sites "stays exempt from orphan aging **and** LRU eviction for
    /// `HANDSHAKE_GIVEUP` (90 s) **after the connection it belongs to
    /// dies** … If that outbound never completed, the 90 s runs from its
    /// own `HANDSHAKE_GIVEUP` expiry instead." Both of those instants are
    /// the instant this static's §17.1 pin is released, so the flag rides
    /// the static row and
    /// [`drop_pending`](Endpoint::drop_pending)/`Retired` turn it into
    /// `exempt_until`. Stamping an absolute instant *now* would instead
    /// expire the exemption while the connection was still alive, which is
    /// the one thing §6.7 says the bound may not do.
    ///
    /// **The pending's pin may not exist yet.** §17.1's pin "never
    /// *creates* an entry", so a dial to a static nobody had ever recorded
    /// took none — and this write is exactly what creates it. Leaving it
    /// unpinned would falsify §17.1's own bullet ("an entry is pinned …
    /// while an in-flight outbound pending exists for its static") for
    /// precisely the entries a tie-break just wrote, and age out at
    /// `TS_GUARD_ORPHAN_TTL` the record §6.7's single-use bound rests on.
    /// `authenticate()` runs the same fix-up for the same reason.
    ///
    /// [`GuardUndo`]: super::guard::GuardUndo
    pub(super) fn record_tiebreak_timestamp(
        &mut self,
        now: Instant,
        conn: ConnectionId,
        peer_static: &[u8],
        timestamp: Timestamp,
    ) {
        // Permanent: the undo is deliberately not carried.
        let _ = self.guard.record(peer_static, timestamp, now);
        self.statics.arm_guard_exemption(peer_static);
        if let Some(pending) = self.pendings.get_mut(&conn)
            && !pending.guard_pinned
        {
            pending.guard_pinned = self.guard.pin(peer_static, PinKind::KeyHolder);
        }
    }

    /// §17.1's `HANDSHAKE_GIVEUP` extension, at the instant the pin goes.
    ///
    /// Called from the two places a static row is released — a pending's
    /// death ([`drop_pending`](Endpoint::drop_pending)) and a connection's
    /// (`Retired`) — with the row's own flag, so an entry no tie-break
    /// wrote is untouched.
    ///
    /// **Order matters against the release.** `unpin` deletes an entry that
    /// has no record, no pins and no exemption; arming the exemption first
    /// is what keeps a tie-break entry alive to be aged rather than
    /// deleted outright, and `GuardEntry::pinned` reads `exempt_until` for
    /// the LRU, which §17.1 requires the extension to cover as well as
    /// orphan aging.
    ///
    /// **A cancel is treated as a death**, which §17.1 does not name. It
    /// names two instants — the connection's death, and, "if that outbound
    /// never completed", its `HANDSHAKE_GIVEUP` expiry — and ruling 50's
    /// cancel is neither. `now + HANDSHAKE_GIVEUP` is the same expression
    /// both named instants produce, and it is the conservative direction:
    /// being wrong here retains one 45-byte timestamp too long, where
    /// being wrong the other way re-arms the replay §6.7 says the bound
    /// "may not be dropped".
    pub(super) fn extend_guard_exemption(&mut self, now: Instant, peer_static: &[u8]) {
        self.guard
            .extend_exemption(peer_static, now + constants::HANDSHAKE_GIVEUP);
    }

    /// §6.4's PENDING branch, loser side: cancel the dial that lost.
    ///
    /// "*The `accept()` **cancels** the pending — the pending and its index
    /// are dropped and its `Connecting` resolves
    /// `Err(ConnectError::AlreadyConnected)` — and the accept proceeds as
    /// an ordinary fresh install with this endpoint as responder.*"
    ///
    /// Note what this route does **not** share with §6.6 step 4: there the
    /// dial's own connection is the one that survives, completed by an
    /// `Install`; here it is destroyed and `accept()` returns a fresh one.
    /// The difference is not a choice — on this route the application is
    /// holding the chain and `accept()` owes it a `Connection`, while on
    /// that one the packet never surfaced. Both leave §16.1 with exactly
    /// one connection for the static.
    ///
    /// `drop_pending` releases the static row, which is also what lets
    /// `accept()`'s install meet `StaticMap::insert`'s `§16.1: one session
    /// per peer static` assertion rather than trip it.
    pub(super) fn cancel_pending_losing_tiebreak(&mut self, now: Instant, dial: ConnectionId) {
        self.drop_pending(now, dial);
        // §16.4's `HandshakeFailed` is shell-only and resolves the
        // `Connecting`; the dial's connection core is simply dropped. It
        // is emitted **before** the accept's msg2, which is generation
        // order and normative (§16.4).
        self.emit(EndpointOutput::HandshakeFailed(
            dial,
            ConnectError::AlreadyConnected,
        ));
    }
}

#[cfg(test)]
mod tests {
    //! §6.5 and §6.6 at the core, driven by arithmetic on an `Instant`.
    //!
    //! These are the **implementer's** tests, written alongside the code
    //! and therefore not the acceptance criteria — CLAUDE.md working rule
    //! 6 puts those in an independent author's file. What they are for is
    //! the two things the brief asks to be *measured* rather than argued:
    //! §6.1's DH ladder under the new paths, and the liveness regression
    //! ruling 91 exists to close.

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use super::*;
    use crate::config::Config;
    use crate::core::endpoint::staged::IntroId;
    use crate::core::{Connection, Disposition};
    use crate::error::{AcceptError, AuthError};
    use crate::identity::Identity;
    use crate::packet::ReferenceSuite;
    use crate::testutil::{CountingIdentity, DhCounter};

    type Suite = ReferenceSuite;
    type Id = CountingIdentity<Suite>;
    type Pk = PublicKeyOf<Id>;

    fn addr(last: u8, port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn t0() -> Instant {
        Instant::now()
    }

    /// The `sender_index` of a framed initiation, read back through
    /// §3.1's own classifier so a test never re-implements the layout.
    fn init_index(datagram: &[u8]) -> u32 {
        match crate::packet::classify::<Suite>(datagram) {
            Some(crate::packet::Inbound::Init { header, .. }) => header.sender_index,
            _ => panic!("not a framed initiation"),
        }
    }

    /// The Noise msg1 of a framed initiation, likewise.
    fn init_msg1(datagram: &[u8]) -> Vec<u8> {
        match crate::packet::classify::<Suite>(datagram) {
            Some(crate::packet::Inbound::Init { msg1, .. }) => msg1.to_vec(),
            _ => panic!("not a framed initiation"),
        }
    }

    /// What one `poll_output()` drain produced.
    #[derive(Default)]
    struct Drained {
        transmits: Vec<Transmit>,
        intros: Vec<(IntroId, SocketAddr)>,
        installs: Vec<ConnectionId>,
        failed: Vec<(ConnectionId, ConnectError)>,
        deadline: Option<Instant>,
    }

    /// One endpoint core, its address, and its DH counter.
    struct Node {
        ep: Endpoint<Id>,
        dhs: DhCounter,
        pk: Pk,
        addr: SocketAddr,
    }

    impl Node {
        fn new(now: Instant, key_seed: u8, rng_seed: u8, at: SocketAddr) -> Self {
            let identity: Id = CountingIdentity::seeded([key_seed; 32]);
            let dhs = identity.counter();
            let pk = *identity.public_static();
            let ep = Endpoint::new(now, Config::default(), identity, [rng_seed; 32]);
            Node {
                ep,
                dhs,
                pk,
                addr: at,
            }
        }

        fn canonical(&self) -> &[u8] {
            self.pk.as_ref()
        }

        /// §16.4's contract: drain to the terminal `Timeout`.
        fn drain(&mut self) -> Drained {
            let mut d = Drained::default();
            for _ in 0..10_000 {
                match self.ep.poll_output() {
                    EndpointOutput::Timeout(deadline) => {
                        d.deadline = deadline;
                        return d;
                    }
                    EndpointOutput::Transmit(t) => d.transmits.push(t),
                    EndpointOutput::IntroReady(id, src) => d.intros.push((id, src)),
                    EndpointOutput::ToConnection(id, _) => d.installs.push(id),
                    EndpointOutput::HandshakeFailed(id, why) => d.failed.push((id, why)),
                }
            }
            panic!("poll_output() never reached Timeout (§16.4)");
        }

        fn feed(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Drained {
            let disposition = self.ep.handle_datagram(now, src, datagram);
            assert!(
                matches!(disposition, Disposition::Done),
                "an initiation is always the endpoint's own"
            );
            self.drain()
        }

        fn timeout(&mut self, now: Instant) -> Drained {
            self.ep.handle_timeout(now);
            self.drain()
        }

        /// Ruling 90's two halves, which together are one dial.
        fn dial(&mut self, now: Instant, to: SocketAddr, peer: &Pk) -> (ConnectionId, Drained) {
            let (id, _connection) = self
                .ep
                .mint_pending(now, to, *peer)
                .expect("the static is NONE");
            self.ep.start_attempt(now, id);
            (id, self.drain())
        }
    }

    /// A real msg1 from `from` to `to`, taken off `from`'s own wire, with
    /// the dial cancelled again so the sender keeps no state. Used where a
    /// test needs an initiation without a live pending behind it.
    fn lone_msg1(from: &mut Node, now: Instant, to: &Node) -> Vec<u8> {
        let (conn, drained) = from.dial(now, to.addr, &to.pk);
        let datagram = drained.transmits[0].data.clone();
        let our_index = init_index(&datagram);
        from.ep
            .handle_connection_event(now, conn, crate::core::ToEndpoint::Retired { our_index });
        let _ = from.drain();
        datagram
    }

    /// Two nodes whose statics are in a **known** order: `.0` is the
    /// smaller, so `.0` is §6.7's winning initiator.
    ///
    /// Chosen by measurement rather than assumed — CLAUDE.md's own note on
    /// `accept_vs_connect_race_reaches_6_4s_pending_branch` is that the
    /// branch under test depends on the key order, so a test that assumed
    /// it would silently swap sides if a seed ever changed.
    fn ordered_pair(now: Instant) -> (Node, Node) {
        let x = Node::new(now, 7, 0x11, addr(1, 4001));
        let y = Node::new(now, 9, 0x22, addr(2, 4002));
        if x.canonical() < y.canonical() {
            (x, y)
        } else {
            (y, x)
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.5 step 2 — the hint check
    // ═══════════════════════════════════════════════════════════════════

    /// §6.5: "the *hint set* is the dialled addresses of all in-flight
    /// outbound initiations — **nothing else** … If `src` ∉ hint set →
    /// park at stage 0 (§6.3) and surface an `Intro`."
    ///
    /// The **cost** is the assertion that separates a conforming core from
    /// one that reads eagerly and asks afterwards: an unhinted arrival is
    /// still **0 DH**, because §6.1 prices a parked `Intro` at zero and
    /// §6.5 puts the read strictly behind the membership test.
    #[test]
    fn an_unhinted_initiation_parks_at_zero_dh() {
        let t = t0();
        let (mut a, mut b) = ordered_pair(t);
        let msg1 = lone_msg1(&mut b, t, &a);

        a.dhs.reset();
        let drained = a.feed(t, b.addr, &msg1);

        assert_eq!(
            drained.intros.len(),
            1,
            "§6.5 step 2: it parks and surfaces"
        );
        assert_eq!(a.dhs.get(), 0, "§6.1: a parked introduction costs 0 DH");
    }

    /// §17.4: "Established connections contribute no hints: their
    /// initiations take the ordinary staged path (§5.4)."
    ///
    /// The degenerate core this separates from is one whose hint set is
    /// "every address we have ever dialled": there the second initiation
    /// would be read eagerly at 1 DH and — the static being LIVE, not
    /// PENDING — demoted, which §6.5 forbids in terms ("a LIVE static's
    /// initiation takes the ordinary staged path").
    #[test]
    fn an_established_connection_contributes_no_hint() {
        let t = t0();
        let (mut a, mut b) = ordered_pair(t);

        // A dials B and the dial completes, so B's address is now a LIVE
        // row's and no longer a hint.
        let (_dial, out) = a.dial(t, b.addr, &b.pk);
        let msg1 = out.transmits[0].data.clone();
        let resp = {
            let drained = b.feed(t, a.addr, &msg1);
            let (id, _src) = drained.intros[0];
            b.ep.authenticate(t, id).expect("a real msg1 authenticates");
            let (_conn, _c) = b.ep.accept(t, id).expect("B has no row for A");
            b.drain().transmits[0].data.clone()
        };
        let _ = a.feed(t, b.addr, &resp);
        assert!(a.ep.hints().is_empty(), "§17.4: a LIVE row hints nothing");

        // A second initiation **from that same address** must take the
        // staged path at 0 DH. It is a third peer's, because §16.1 forbids
        // B a second dial to A — which is itself the point: the address is
        // all a hint could match on, and it no longer matches anything.
        let mut c = Node::new(t, 13, 0x33, addr(3, 4003));
        let msg1_again = lone_msg1(&mut c, t, &a);
        a.dhs.reset();
        let drained = a.feed(t, b.addr, &msg1_again);
        assert_eq!(
            drained.intros.len(),
            1,
            "§6.5: LIVE goes to the staged path"
        );
        assert_eq!(
            a.dhs.get(),
            0,
            "no eager read fired for an established peer"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.5 step 3 — the eager path and its demotion
    // ═══════════════════════════════════════════════════════════════════

    /// §6.5 step 3, second bullet: "claimed ∉ pending outbound remotes →
    /// the raw packet is **demoted** to the stage-0 queue … **carrying its
    /// paid mid-state**, tagged identity-already-read: a peer sharing a
    /// source with a dialled address still surfaces as an `Intro`, and
    /// `read_identity()` on it returns the cached claim at 0 incremental
    /// DH."
    ///
    /// Three claims, three assertions, and the middle one is what makes
    /// this a test rather than a restatement: **1 then 0**, not 1 then 1.
    /// A core that demoted the raw bytes and threw the mid-state away
    /// would surface the same `Intro` and reach the same `Proven`, and
    /// would cost 2 DH by `read_identity()` — one more than §6.1 prices
    /// the whole ladder's second row at.
    #[test]
    fn a_demoted_initiation_carries_its_paid_mid_state() {
        let t = t0();
        let (mut a, b) = ordered_pair(t);
        let mut c = Node::new(t, 13, 0x33, addr(3, 4003));

        // A dials B, so B's address is a hint. C's initiation arrives from
        // that same address — §6.5's "a peer sharing a source with a
        // dialled address".
        let _ = a.dial(t, b.addr, &b.pk);
        let from_c = lone_msg1(&mut c, t, &a);

        a.dhs.reset();
        let drained = a.feed(t, b.addr, &from_c);
        assert_eq!(drained.intros.len(), 1, "it still surfaces as an `Intro`");
        assert_eq!(a.dhs.get(), 1, "§6.5 step 3: the eager read is one `es`");

        let (id, _src) = drained.intros[0];
        let claimed = a.ep.read_identity(id).expect("the claim is cached");
        assert_eq!(
            claimed.as_ref(),
            c.canonical(),
            "the demoted entry is tagged with the claim the eager read produced"
        );
        assert_eq!(
            a.dhs.get(),
            1,
            "§6.5: `read_identity()` on a demoted entry is **0 incremental DH**"
        );

        a.ep.authenticate(t, id).expect("a real msg1 authenticates");
        assert_eq!(a.dhs.get(), 2, "§6.1: `authenticate()` is 2 DH cumulative");
        a.ep.accept(t, id).expect("C's static is NONE");
        assert_eq!(a.dhs.get(), 4, "§6.1: `accept()` is 4 DH cumulative");
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.6 — the internal tie-break
    // ═══════════════════════════════════════════════════════════════════

    /// §6.6 step 1: "a forged claim of a pending static dies here at the
    /// msg1 tail's AEAD tag. A failure leaves the in-flight outbound
    /// pending **untouched** — nothing unauthenticated can reach the
    /// tie-break." §6.7 states the consequence: "**a forgery cannot cancel
    /// a pending**."
    ///
    /// The forgery is mac1-valid — mac1 keys on public data (§4.3), so an
    /// attacker mints it freely — and carries a corrupted tail. The
    /// assertions are chosen against the *broken* core: one that applied
    /// §6.7's comparison at `es` would cancel the loser's pending here, so
    /// the pending must be shown still **alive** (it retransmits) rather
    /// than merely still listed, and the guard must hold nothing.
    #[test]
    fn a_forgery_cannot_cancel_a_pending() {
        let t = t0();
        // The **larger** static is the one with something to lose: it is
        // §6.7's tie-break loser, so a core that applied the comparison at
        // `es` — before the `ss` that proves anything — would cancel this
        // pending on the forgery below.
        let (mut small, mut large) = ordered_pair(t);

        let (dial, out) = large.dial(t, small.addr, &small.pk);
        assert_eq!(out.transmits.len(), 1, "the dial put one msg1 on the wire");

        // A genuine msg1 from the peer, with its tail tag broken. mac1 is
        // then recomputed over the corrupted bytes: mac1 keys on public
        // data (§4.3), so an attacker mints a valid one for free, and a
        // forgery that died at mac1 would prove nothing about §6.6 step 1.
        let mut forged = lone_msg1(&mut small, t, &large);
        let index = init_index(&forged);
        let tail = forged.len() - crate::constants::MAC1_LEN - 1;
        forged[tail] ^= 0xff;
        let framed = framing::frame_init(
            index,
            &init_msg1(&forged),
            &Mac1Key::derive(large.canonical()),
        );

        large.dhs.reset();
        let drained = large.feed(t, small.addr, &framed);
        assert_eq!(
            large.dhs.get(),
            2,
            "§6.6: the forgery reached step 1 and died there — `es` then `ss`, no msg2"
        );
        assert!(
            drained.transmits.is_empty() && drained.installs.is_empty(),
            "§6.6 step 1: a tag death writes no msg2 and installs nothing"
        );
        assert!(
            drained.intros.is_empty(),
            "§6.5: the packet never touches the accept queue"
        );
        assert_eq!(
            large.ep.greatest(small.canonical()),
            None,
            "§6.6: a step-1 failure records nothing"
        );

        // Alive, not merely listed: the retransmit train is still running.
        let later = t
            + crate::constants::RETRANSMIT_BASE
            + crate::constants::RETRANSMIT_JITTER_MAX
            + Duration::from_millis(1);
        let after = large.timeout(later);
        assert_eq!(
            after.transmits.len(),
            1,
            "§6.7: a forgery cannot cancel a pending — the dial is still retransmitting"
        );
        assert!(
            after.failed.is_empty(),
            "and it was not failed either: {:?}",
            after.failed
        );
        let _ = dial;
    }

    /// §6.6 step 3, winner side, reached by §6.5's eager path: "the
    /// authenticated inbound is silently dropped, its timestamp
    /// **recorded** (§6.7), and our own outbound completes normally."
    /// §17.4: "The winning connection's replacement basis stays `None`."
    ///
    /// The record is the assertion that separates this from a core that
    /// merely drops the packet — which is otherwise indistinguishable,
    /// because a winner-side drop emits nothing at all.
    #[test]
    fn the_tiebreak_winner_drops_the_inbound_and_records_it() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        let (dial, _) = small.dial(t, large.addr, &large.pk);
        let crossing = lone_msg1(&mut large, t, &small);

        small.dhs.reset();
        let drained = small.feed(t, large.addr, &crossing);

        assert!(
            drained.intros.is_empty() && drained.transmits.is_empty(),
            "§6.5: the application never sees it, and §6.6 step 3 writes no msg2"
        );
        assert!(
            drained.installs.is_empty() && drained.failed.is_empty(),
            "the winner's own outbound is untouched"
        );
        assert_eq!(
            small.dhs.get(),
            2,
            "§6.6: `es` then `ss`, and no `ee`/`se` on the winner side"
        );
        assert!(
            small.ep.greatest(large.canonical()).is_some(),
            "§6.7: the winner **records** the loser's timestamp"
        );
        assert_eq!(
            small.ep.replacement_basis(large.canonical()),
            Some(None),
            "§17.4: the winner is the initiator, so its basis stays `None`"
        );
        assert!(
            small.ep.guard_pins(large.canonical()) > 0,
            "§17.1: the in-flight outbound pending pins the entry the record just created"
        );
        let _ = dial;
    }

    /// §6.6 step 2: "the per-static greatest-timestamp guard (§17.1):
    /// strictly greater, or the initiation dies here."
    ///
    /// Driven by replaying the *same* initiation: the first pass records
    /// it (winner side), so the second is no longer strictly greater. The
    /// second pass must cost the same 2 DH and change nothing — a core
    /// that checked the guard *after* the tie-break would record twice,
    /// and one that skipped the guard entirely would be indistinguishable
    /// on the winner side, which is why the loser side is checked too.
    #[test]
    fn a_replayed_initiation_dies_at_the_guard() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        // Loser side: the *large* static is the one that admits.
        let (_dial, _) = large.dial(t, small.addr, &small.pk);
        let crossing = lone_msg1(&mut small, t, &large);

        let first = large.feed(t, small.addr, &crossing);
        assert_eq!(first.installs.len(), 1, "§6.6 step 4 admitted the first");
        let recorded = large
            .ep
            .greatest(small.canonical())
            .expect("the admission recorded");

        // Replay it. The connection is LIVE now, so §6.5 routes it to the
        // staged path — the guard is the thing under test either way.
        let replay = large.feed(t, small.addr, &crossing);
        if let Some((id, _)) = replay.intros.first() {
            assert!(
                matches!(large.ep.authenticate(t, *id), Err(AuthError::Replay)),
                "§17.1: the same initiation is no longer strictly greater"
            );
        }
        assert_eq!(
            large.ep.greatest(small.canonical()),
            Some(recorded),
            "a failed check writes nothing (§17.1 mitigation (iii))"
        );
    }

    /// §6.6 step 4, the loser side, and §6.7's promise that the dial's own
    /// `Connecting` is what completes: "The admission completes the
    /// connection as an `Install` (§16.4), resolving its `Connecting`
    /// exactly as a msg2 completion would."
    ///
    /// Every clause of step 4 is separately observable and separately
    /// asserted: msg2 on the wire, the `Install` on **the dial's own**
    /// `ConnectionId` (not a fresh one), the basis `Some(t)`, and the
    /// §16.1 row promoted rather than doubled.
    #[test]
    fn the_tiebreak_loser_cancels_its_pending_and_installs_as_responder() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        let (dial, _) = large.dial(t, small.addr, &small.pk);
        let crossing = lone_msg1(&mut small, t, &large);

        large.dhs.reset();
        let drained = large.feed(t, small.addr, &crossing);

        assert_eq!(large.dhs.get(), 4, "§6.6: `es`, `ss`, then `ee` and `se`");
        assert_eq!(drained.transmits.len(), 1, "§6.6 step 4 writes msg2");
        assert_eq!(
            drained.transmits[0].to, small.addr,
            "§5.6: the responder anchors at the msg1 source"
        );
        assert_eq!(
            drained.installs,
            vec![dial],
            "§6.7: the **dial's own** connection is completed by the tie-break's Install"
        );
        assert!(
            drained.failed.is_empty(),
            "no give-up, no error (§6.7): {:?}",
            drained.failed
        );
        assert!(
            drained.intros.is_empty(),
            "§6.5: the application never sees the packet"
        );
        assert!(
            matches!(large.ep.replacement_basis(small.canonical()), Some(Some(_))),
            "§17.4: the tie-break loser's admit step sets the basis to `Some(t)`"
        );
        assert!(
            large.ep.hints().is_empty(),
            "§17.4: the row is LIVE now, and a LIVE row hints nothing"
        );
        assert!(
            matches!(
                large.ep.mint_pending(t, small.addr, small.pk),
                Err(ConnectError::AlreadyConnected)
            ),
            "§16.1: one session per static, and the row was promoted rather than doubled"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // §6.4's PENDING branch
    // ═══════════════════════════════════════════════════════════════════

    /// §6.4's PENDING branch, **winner** side, and §17.1's one exception:
    /// "the tie-break-**winner** case of the PENDING branch below returns
    /// `Stale` and **keeps** its record … **No other `Stale` leaves a
    /// record behind.**"
    ///
    /// Both halves are asserted, because only the pair is a test: the
    /// winner's `Stale` keeps the record, and a LIVE-row `Stale` in the
    /// same file still reverts it. A core that stopped reverting
    /// altogether passes the first and fails the second.
    #[test]
    fn the_pending_branch_winner_refuses_and_keeps_its_record() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        // The §6.4 ordering: the chain is staged while the static is NONE,
        // and `connect()` makes it PENDING before `accept()` runs. The
        // initiation arrives from an address nobody has dialled, so §6.5
        // parks it — which is the ordering §6.4:1436 names.
        let from_large = lone_msg1(&mut large, t, &small);
        let elsewhere = addr(9, 4009);
        let drained = small.feed(t, elsewhere, &from_large);
        let (id, _src) = drained.intros[0];

        let claimed = small.ep.read_identity(id).expect("readable");
        assert_eq!(claimed.as_ref(), large.canonical());
        let (_peer, timestamp) = small.ep.authenticate(t, id).expect("authenticates");

        let (dial, _) = small.dial(t, large.addr, &large.pk);
        let refused = small.ep.accept(t, id);
        let after = small.drain();

        assert!(
            matches!(refused, Err(AcceptError::Stale)),
            "§6.4: our static is smaller, so we are the tie-break winner: {:?}",
            refused.map(|_| "Ok")
        );
        assert!(
            after.failed.is_empty(),
            "§6.4: the pending is **left in place** — {:?}",
            after.failed
        );
        assert_eq!(
            small.ep.greatest(large.canonical()),
            Some(timestamp),
            "§17.1's one exception: this `Stale` KEEPS the candidate's timestamp"
        );
        assert!(
            small.ep.guard_pins(large.canonical()) > 0,
            "§17.1: the chain's pin went with the chain, but the dial still pins the entry"
        );
        let _ = dial;
    }

    /// The other half of the pair above: §17.1 mitigation (i) still holds
    /// for every `Stale` that is not the tie-break winner's.
    ///
    /// The LIVE row is the reachable one — §6.4's re-home admission is a
    /// later slice, so a proven-LIVE `accept()` still refuses — and its
    /// refusal must leave the guard exactly as it was.
    #[test]
    fn a_live_row_stale_still_reverts_its_record() {
        let t = t0();
        let (small, mut large) = ordered_pair(t);
        let mut third = Node::new(t, 21, 0x44, addr(4, 4004));

        // `large` accepts `third`, so `third`'s static is LIVE on it.
        let first = lone_msg1(&mut third, t, &large);
        let drained = large.feed(t, addr(8, 4008), &first);
        let (id, _) = drained.intros[0];
        large.ep.authenticate(t, id).expect("authenticates");
        large.ep.accept(t, id).expect("NONE");
        let _ = large.drain();
        let established = large
            .ep
            .greatest(third.canonical())
            .expect("the accept made the record permanent");

        // A second, strictly newer initiation from the same static.
        let second = lone_msg1(&mut third, t, &large);
        let drained = large.feed(t, addr(8, 4010), &second);
        let (id2, _) = drained.intros[0];
        let (_p, newer) = large.ep.authenticate(t, id2).expect("authenticates");
        assert!(newer > established, "§5.5: every retransmit is newer");
        assert!(
            matches!(large.ep.accept(t, id2), Err(AcceptError::Stale)),
            "§6.4's proven-LIVE admission is a later slice"
        );
        assert_eq!(
            large.ep.greatest(third.canonical()),
            Some(established),
            "§17.1 mitigation (i): a non-winner `Stale` REVERTS its provisional record"
        );
        let _ = small;
    }

    /// §6.4's PENDING branch, **loser** side: "the `accept()` **cancels**
    /// the pending — the pending and its index are dropped and its
    /// `Connecting` resolves `Err(ConnectError::AlreadyConnected)` — and
    /// the accept proceeds as an ordinary fresh install with this endpoint
    /// as responder."
    ///
    /// The assertion that separates this from the interim
    /// unconditional-`Stale` is that `accept()` returns **`Ok`**; the one
    /// that separates it from a core that merely forgot the dial is the
    /// `HandshakeFailed(AlreadyConnected)`, without which the application's
    /// `Connecting` never resolves at all.
    #[test]
    fn the_pending_branch_loser_cancels_its_dial_and_installs() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        let from_small = lone_msg1(&mut small, t, &large);
        let drained = large.feed(t, addr(9, 4009), &from_small);
        let (id, _src) = drained.intros[0];
        large.ep.authenticate(t, id).expect("authenticates");

        let (dial, _) = large.dial(t, small.addr, &small.pk);
        let accepted = large.ep.accept(t, id);
        let after = large.drain();

        let (conn, _connection): (ConnectionId, Connection<Suite>) = accepted
            .expect("§6.4: the peer's static is smaller, so we lose and install as responder");
        assert_ne!(
            conn, dial,
            "§6.4: this route installs a **fresh** connection; the dial is cancelled"
        );
        assert_eq!(
            after.failed,
            vec![(dial, ConnectError::AlreadyConnected)],
            "§6.4: the cancelled pending's `Connecting` resolves `AlreadyConnected`"
        );
        assert_eq!(after.transmits.len(), 1, "the accept wrote msg2");
        assert!(
            matches!(large.ep.replacement_basis(small.canonical()), Some(Some(_))),
            "§17.4: we responded, so the basis is `Some(t)`"
        );
        assert!(
            large.ep.hints().is_empty(),
            "the dial is gone: §16.1 leaves exactly one connection for this static"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Ruling 91's regression, measured
    // ═══════════════════════════════════════════════════════════════════

    /// §6.4:1436's "ordinary API ordering" — `read_identity()` →
    /// `connect()` → `accept()` against one static — **completes**, in
    /// **both key orders**, with **no clock advance at all**.
    ///
    /// This is the number ruling 91 exists for. Before §6.5 and §6.6 the
    /// same scenario left both dials unresolved and both peers reported
    /// `ConnectError::TimedOut` at `HANDSHAKE_GIVEUP`, measured at both
    /// ends. Three separate things are asserted, and each fails a
    /// different broken core:
    ///
    /// 1. **Both sides resolve**, which the interim
    ///    unconditional-`Stale` core fails outright;
    /// 2. **at `t`**, with the pump advancing no clock — which a core that
    ///    relied on the peer's ~5 s retransmit to be caught by the eager
    ///    path would fail, and which is what §6.4's PENDING branch is
    ///    *for*: §6.5's interception "cannot fire on a chain the
    ///    application already holds";
    /// 3. and the run is then driven **past `HANDSHAKE_GIVEUP`**, where
    ///    neither peer may report `TimedOut` — the regression's own
    ///    signature.
    ///
    /// Both orders are run because the two peers take **different** code
    /// paths and which peer takes which is decided by §6.7's comparison:
    /// the endpoint holding the chain reaches §6.4's PENDING branch, its
    /// peer reaches §6.6's internal one, and one run exercises one pair.
    #[test]
    fn the_ordinary_api_ordering_completes_in_both_key_orders() {
        for chain_holder_is_smaller in [true, false] {
            let t = t0();
            let (small, large) = ordered_pair(t);
            // `a` holds the chain and runs `read_identity()` →
            // `connect()` → `accept()`; `b` is the peer dialling into it.
            let (mut a, mut b) = if chain_holder_is_smaller {
                (small, large)
            } else {
                (large, small)
            };
            let order = if chain_holder_is_smaller {
                "chain holder is the tie-break WINNER"
            } else {
                "chain holder is the tie-break LOSER"
            };

            // B dials A, so A has a chain to walk.
            let (b_dial, out) = b.dial(t, a.addr, &a.pk);
            let msg1 = out.transmits[0].data.clone();

            // §6.4:1436's ordering, exactly. The chain is staged while A's
            // row for B is NONE, and `connect()` makes it PENDING before
            // `accept()` runs.
            let drained = a.feed(t, b.addr, &msg1);
            let (id, _src) = drained.intros[0];
            a.ep.read_identity(id).expect("readable");
            a.ep.authenticate(t, id).expect("authenticates");
            let (a_dial, dial_out) = a.dial(t, b.addr, &b.pk);
            let accepted = a.ep.accept(t, id);
            let after_accept = a.drain();

            // A's dial is resolved either by the accept returning an
            // established connection (§6.4's loser side, which cancels the
            // dial with `AlreadyConnected`) or, on the winner side, by the
            // `Install` its own outbound earns once B loses §6.6's
            // comparison.
            let mut a_resolved = accepted.is_ok();
            let mut b_resolved = false;
            let mut timed_out: Vec<(&str, ConnectionId, ConnectError)> = Vec::new();

            let mut to_b: Vec<Vec<u8>> = dial_out
                .transmits
                .iter()
                .chain(after_accept.transmits.iter())
                .map(|t| t.data.clone())
                .collect();
            for (conn, why) in after_accept.failed {
                match why {
                    ConnectError::AlreadyConnected => {
                        assert_eq!(conn, a_dial, "{order}: only the dial is cancelled");
                        assert!(accepted.is_ok(), "{order}: §6.4's loser side installs");
                    }
                    other => timed_out.push(("A", conn, other)),
                }
            }

            // **No clock advance.** The pump is purely event-driven at `t`:
            // no `handle_timeout`, so no retransmit exists to rescue it,
            // and no application-side `accept()` on B either — if this
            // converges, §6.5 and §6.6 converged it.
            let mut to_a: Vec<Vec<u8>> = Vec::new();
            for _ in 0..4 {
                for datagram in std::mem::take(&mut to_b) {
                    let d = b.feed(t, a.addr, &datagram);
                    b_resolved |= !d.installs.is_empty();
                    for (conn, why) in d.failed {
                        timed_out.push(("B", conn, why));
                    }
                    to_a.extend(d.transmits.iter().map(|t| t.data.clone()));
                }
                for datagram in std::mem::take(&mut to_a) {
                    let d = a.feed(t, b.addr, &datagram);
                    a_resolved |= !d.installs.is_empty();
                    for (conn, why) in d.failed {
                        timed_out.push(("A", conn, why));
                    }
                    to_b.extend(d.transmits.iter().map(|t| t.data.clone()));
                }
            }

            assert!(
                a_resolved,
                "{order}: A's `read_identity() → connect() → accept()` never resolved, \
                 with no clock advanced — this is ruling 91's regression"
            );
            assert!(b_resolved, "{order}: B's dial never resolved");

            // Past the give-up, which is where the regression reported
            // itself. Nothing may surface here.
            let past = t + crate::constants::HANDSHAKE_GIVEUP + Duration::from_secs(1);
            for (side, node) in [("A", &mut a), ("B", &mut b)] {
                for (conn, why) in node.timeout(past).failed {
                    timed_out.push((side, conn, why));
                }
            }
            assert!(
                timed_out.is_empty(),
                "{order}: a dial reached HANDSHAKE_GIVEUP — {timed_out:?}"
            );
            let _ = (a_dial, b_dial);
        }
    }

    /// **Working rule 8, decided and pinned:** a pending is a pending from
    /// `mint_pending`, before `start_attempt` has put an msg1 on the wire.
    ///
    /// Ruling 90 split the dial in two, and §6.5 predates the split: it
    /// says "the dialled addresses of all **in-flight outbound
    /// initiations**", §6.7 says "an **in-flight outbound pending**", and
    /// §6.4's PENDING branch says "if an **in-flight outbound initiation**
    /// exists". Those three named one set before ruling 90 and now
    /// straddle a window the shell can observe — §16.2's `connect()` is
    /// synchronous while `start_attempt` lands on the driver a command
    /// later.
    ///
    /// **The reading taken is §17.4's**: the hint set *is* "the pending
    /// tables' dialled addresses", and a `mint_pending` row has one. The
    /// argument is not the wording but the consequence — the same row
    /// answers §16.1's admission test (ruling 90 made it the only map),
    /// §6.5's probed set, and §6.4's PENDING branch, and §6.6 requires
    /// those last two to "never disagree". Reading it the other way makes
    /// a static PENDING for admission and NONE for routing at the same
    /// instant.
    ///
    /// **It is observable**, which is why this is a test and not a
    /// comment: under the other reading the crossing msg1 below parks as
    /// an `Intro` and converges one round-trip later instead.
    #[test]
    fn a_minted_pending_is_already_a_pending_outbound_remote() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        // `mint_pending` **only** — ruling 90's first half, no msg1.
        let (dial, _connection) = large
            .ep
            .mint_pending(t, small.addr, small.pk)
            .expect("the static is NONE");
        let quiet = large.drain();
        assert!(
            quiet.transmits.is_empty(),
            "ruling 90: `mint_pending` puts nothing on the wire"
        );
        assert_eq!(
            large.ep.hints(),
            vec![small.addr],
            "§17.4: the hint set is the pending tables' dialled addresses"
        );

        // The crossing msg1 arrives inside the window.
        let crossing = lone_msg1(&mut small, t, &large);
        let drained = large.feed(t, small.addr, &crossing);

        assert!(
            drained.intros.is_empty(),
            "§6.5: a PENDING static's initiation **must** enter the internal path"
        );
        assert_eq!(
            drained.installs,
            vec![dial],
            "§6.6 step 4 completes the dial that had not yet transmitted"
        );
    }

    /// §16.1, at the end of the same scenario: **exactly one connection
    /// per static**, on both peers and in both key orders.
    ///
    /// The bound the degenerate version violates: a core that installed on
    /// both sides regardless (§6.4's "mutually dark" divergence) leaves
    /// each peer holding a row it can no longer dial over, which is
    /// indistinguishable here — so the assertion is on the **basis**,
    /// which records which side responded, and the two must disagree.
    #[test]
    fn exactly_one_side_responds() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        let (_b_dial, out) = large.dial(t, small.addr, &small.pk);
        let msg1 = out.transmits[0].data.clone();

        let drained = small.feed(t, large.addr, &msg1);
        let (id, _src) = drained.intros[0];
        small.ep.read_identity(id).expect("readable");
        small.ep.authenticate(t, id).expect("authenticates");
        let (_a_dial, dial_out) = small.dial(t, large.addr, &large.pk);
        let refused = small.ep.accept(t, id);
        assert!(
            matches!(refused, Err(AcceptError::Stale)),
            "the smaller static wins §6.7's comparison"
        );
        let mut to_large = dial_out.transmits;
        to_large.extend(small.drain().transmits);

        for transmit in to_large {
            let d = large.feed(t, small.addr, &transmit.data);
            for reply in d.transmits {
                let _ = small.feed(t, large.addr, &reply.data);
            }
        }

        assert_eq!(
            small.ep.replacement_basis(large.canonical()),
            Some(None),
            "§17.4/§6.7: the winner dialled, so its basis is `None`"
        );
        assert!(
            matches!(large.ep.replacement_basis(small.canonical()), Some(Some(_))),
            "§17.4/§6.7: the loser responded, so its basis is `Some(t)`"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // §17.1's HANDSHAKE_GIVEUP extension
    // ═══════════════════════════════════════════════════════════════════

    /// §17.1: an entry written by a **winner-side record** "stays exempt
    /// from orphan aging **and** LRU eviction for `HANDSHAKE_GIVEUP` (90 s)
    /// after the connection it belongs to dies … If that outbound never
    /// completed, the 90 s runs from its own `HANDSHAKE_GIVEUP` expiry
    /// instead."
    ///
    /// The degenerate core this separates from is one that writes the
    /// record and nothing else: there the entry is an ordinary orphan and
    /// `TS_GUARD_ORPHAN_TTL` (15 s, ruling 70) deletes it — re-arming
    /// exactly the replay §6.7 says the bound "may not be dropped". So the
    /// probe is at **15 s past the death**, where the two answers differ,
    /// and again past the 90 s, where they agree again.
    #[test]
    fn a_winner_side_record_outlives_its_dial_by_the_giveup() {
        let t = t0();
        let (mut small, mut large) = ordered_pair(t);

        let (dial, _) = small.dial(t, large.addr, &large.pk);
        let crossing = lone_msg1(&mut large, t, &small);
        let _ = small.feed(t, large.addr, &crossing);
        let recorded = small
            .ep
            .greatest(large.canonical())
            .expect("§6.7: the winner records");

        // The dial never completes: it gives up at HANDSHAKE_GIVEUP, which
        // is the instant §17.1 names for this case.
        let death = t + crate::constants::HANDSHAKE_GIVEUP;
        let d = small.timeout(death);
        assert_eq!(
            d.failed,
            vec![(dial, ConnectError::TimedOut)],
            "the outbound never completed"
        );

        let ordinary_orphan_would_be_gone =
            death + crate::constants::TS_GUARD_ORPHAN_TTL + Duration::from_secs(1);
        let _ = small.timeout(ordinary_orphan_would_be_gone);
        assert_eq!(
            small.ep.greatest(large.canonical()),
            Some(recorded),
            "§17.1: the winner-side record is exempt from orphan aging for HANDSHAKE_GIVEUP"
        );

        let past_the_extension =
            death + crate::constants::HANDSHAKE_GIVEUP + Duration::from_secs(1);
        let _ = small.timeout(past_the_extension);
        assert_eq!(
            small.ep.greatest(large.canonical()),
            None,
            "§17.1: and only for HANDSHAKE_GIVEUP — the extension lapses rather than pinning forever"
        );
    }
}

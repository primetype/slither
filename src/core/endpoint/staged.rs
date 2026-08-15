//! §6.1's typestate chain, as the endpoint core holds it.
//!
//! The application-facing typestate (`Intro` → `Claimed` → `Proven`) is
//! §6.2's, lives on the shell handles, and is slice 3. What lives here is
//! the **state a parked chain is in**, keyed by [`IntroId`], which is what
//! §16.4's synchronous staged verbs address.
//!
//! | Stage | Cumulative DH | Adds | What is held |
//! |---|---|---|---|
//! | parked | **0** | — | the raw msg1 bytes and nothing else |
//! | [`ChainState::Claimed`] | **1** | `es` | hiss's suspended mid-state, plus the *claimed* static |
//! | [`ChainState::Proven`] | **2** | `ss` | the read-through responder state, the proven static, the timestamp |
//! | accepted | **4** | `ee`, `se` | — the chain is gone; a session exists |
//!
//! # The responder machine is built lazily, at `read_identity()`
//!
//! Not at park. Building it at park would call [`Identity::open`] — an
//! enclave round-trip — for **every mac1-valid packet in a flood**: a stage
//! ratified at 0 DH but plainly not at 0 cost. §17.5 corroborates from the
//! other side: each mid-state "holds the endpoint's static provider: for a
//! hardware/enclave static this is up to 1024 concurrent provider handles,
//! an operationally scarce resource". So an unconsumed entry holds **zero**
//! provider handles, and a consumed chain holds exactly one.
//!
//! # Why the states are boxed
//!
//! Appendix A.1 measures the mid-state at ≈ 784 B on P-256. Unboxed, the
//! enum is a `clippy::large_enum_variant` and every queue move memcpys the
//! larger part of a kilobyte.
//!
//! [`Identity::open`]: crate::identity::Identity::open

use std::net::SocketAddr;
use std::time::Instant;

use crate::constants;
use crate::core::endpoint::handshake as framing;
use crate::core::{
    Connection, ConnectionId, EndpointOutput, EstablishedSession, Timestamp, Transmit,
};
use crate::error::{AcceptError, AuthError, IntroError};
use crate::identity::{Identity, PublicKeyOf};
use crate::packet::{Handshake, Mac1Key};

use super::Endpoint;
use super::guard::{ChainPin, PinKind};
use super::tables::{StaticEntry, StaticState};

/// A parked introduction's identity inside one endpoint.
///
/// A **monotone counter**, deliberately: §6.1 forbids anything durable
/// keyed on the source address or on `sender_index`, and a counter is keyed
/// on neither. Never reused within an endpoint's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntroId(u64);

impl IntroId {
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// hiss's suspended mid-state for this identity's suite and provider.
pub(crate) type MidState<I> =
    <<I as Identity>::Suite as Handshake>::Msg1Intro<<I as Identity>::Provider>;

/// hiss's fully-read responder state, ready to write msg2.
pub(crate) type ResponderRead<I> =
    <<I as Identity>::Suite as Handshake>::ResponderRead<<I as Identity>::Provider>;

/// hiss's post-msg1 initiator state, awaiting msg2.
pub(crate) type InitiatorSent<I> =
    <<I as Identity>::Suite as Handshake>::InitiatorSent<<I as Identity>::Provider>;

/// How far up §6.1's ladder a parked chain has been driven.
pub(crate) enum ChainState<I: Identity> {
    /// **0 DH.** Length-gated, classified, mac1-verified, parked. The msg1
    /// bytes live on the entry, not here, because an unconsumed entry's
    /// bytes are replaceable and its accessors must read the newest at call
    /// time (§6.3).
    Parked,
    /// **1 DH** (`es`). The static is *claimed*, not proven — see
    /// documentation obligation #2: authorise on it, never punish on it.
    Claimed {
        /// hiss's suspended read, holding the unpaid `ss` and msg1's tail.
        mid: Box<MidState<I>>,
        /// The claimed static.
        claimed: PublicKeyOf<I>,
    },
    /// **2 DH** (`+ss`). Possession is proven and the initiation timestamp
    /// is decrypted.
    Proven {
        /// hiss's read-through responder state; `write_message_2` consumes
        /// it and pays the remaining `ee`, `se`.
        state: Box<ResponderRead<I>>,
        /// The **proven** peer static.
        peer: PublicKeyOf<I>,
        /// §5.2's initiation timestamp, as decrypted by the `ss`.
        timestamp: Timestamp,
    },
    /// Never observable. It exists for the instant between the
    /// `mem::replace` that takes an owned, non-`Clone` hiss value out from
    /// behind `&mut self` and the write of its successor. A verb that finds
    /// it is a bug.
    Poisoned,
}

// ═══════════════════════════════════════════════════════════════════════
// §16.4's staged verbs, keyed by `IntroId`
// ═══════════════════════════════════════════════════════════════════════

impl<I: Identity> Endpoint<I> {
    /// §16.4's `intro_source` (**ruling 71**).
    ///
    /// Read **live** from the parked entry, never from a value cached when
    /// the introduction surfaced. §6.3 requires that a refreshed entry's
    /// "accessors reflect the newest bytes at call time"; for the source
    /// address this happens to be invariant under replacement — the address
    /// *is* the dedup key — but reading it live is what makes that a
    /// property of the code rather than of a comment.
    pub(crate) fn intro_source(&self, id: IntroId) -> Option<SocketAddr> {
        self.intros.get(id).map(|entry| entry.src)
    }

    /// §16.4's `intro_sender_index` (**ruling 71**).
    ///
    /// Read **live**, and here it is load-bearing: §5.5 mints a **new
    /// random index on every retransmit**, so a value cached when the
    /// introduction surfaced is no longer the one on the wire the moment a
    /// same-source refresh lands.
    pub(crate) fn intro_sender_index(&self, id: IntroId) -> Option<u32> {
        self.intros.get(id).map(|entry| entry.sender_index)
    }

    /// §16.4's `read_identity` — §6.1's stage 1, **1 DH** (`es`).
    ///
    /// Returns the *claimed* static. Documentation obligation #2: this is
    /// an unauthenticated assertion made before any DH proves possession —
    /// **authorise on it, never punish on it**.
    ///
    /// # Cumulative, not incremental
    ///
    /// A chain that already carries a mid-state — §6.5 step 3's
    /// eager-demoted entry — returns its cached result at **0 incremental
    /// DH**, and §6.1 is explicit that the cumulative table is unchanged
    /// either way. Any assertion about cost has to be cumulative to be true
    /// of both. The early returns below are what make §6.5's "and
    /// `read_identity()` on it returns the cached claim at 0 incremental
    /// DH" true.
    ///
    /// # §6.5 step 4's interception is **not** here
    ///
    /// §6.5 makes this verb the backstop for a crossing msg1 whose source
    /// the hint check missed: the endpoint would run §6.6's internal
    /// tie-break and answer [`IntroError::Internal`]. §6.6 records a §17.1
    /// guard entry, [`TimestampGuard::record`] needs the instant, and
    /// §16.4 gives this verb no `now` — see [`routing`](super::routing)'s
    /// module docs for the case that needs a ruling. Nothing diverges
    /// meanwhile; those chains reach the same comparison through §6.4's
    /// PENDING branch at `accept()`.
    ///
    /// [`TimestampGuard::record`]: super::guard::TimestampGuard::record
    ///
    /// # Idempotent (**ruling 74**)
    ///
    /// This verb is keyed by [`IntroId`] and takes `&mut self`, so — unlike
    /// §6.2's `self`-consuming handles — it **can** be called twice. §6.1
    /// resolves that in the direction that cannot perturb the ladder: a
    /// second call on an already `Claimed` or `Proven` chain returns the
    /// revealed static and pays **0 DH**, *opening no provider*. The early
    /// returns below are that rule, not an optimisation — the ladder holds
    /// under any number of calls because [`Identity::open`] is never
    /// reached from a chain that has already left stage 0.
    ///
    /// # Two failure modes, opposite in every way that matters (**ruling 72**)
    ///
    /// A **local** failure — the identity will not open, or the responder
    /// machine will not build on it — has spent **0 DH**, is *our* fault,
    /// and may well be transient (a momentarily locked enclave, which S21
    /// treats as expected). So it reports [`IntroError::Local`] and **the
    /// chain is left parked**: a retry can still succeed.
    ///
    /// A **hiss** failure means msg1 is structurally unreadable: the
    /// *peer's* bytes are at fault, 1 DH is spent, the verdict is
    /// definitive, and the chain is **discarded** with its stage-0 slot
    /// freed. It reports [`IntroError::Malformed`].
    ///
    /// The implementation already drew this distinction — parked versus
    /// discarded — before §18.1 could express it; ruling 72 is what gave
    /// the two outcomes two names. Collapsing them again tells the
    /// application the remote peer sent garbage when our own key hardware
    /// was locked, which is §18.2's recurring shape: the party who can fix
    /// the problem handed evidence pointing elsewhere.
    ///
    /// [`Identity::open`]: crate::identity::Identity::open
    pub(crate) fn read_identity(&mut self, id: IntroId) -> Result<PublicKeyOf<I>, IntroError> {
        self.read_identity_as(id, "read_identity")
    }

    /// [`read_identity`](Self::read_identity), told which verb the caller
    /// is so §18.2's trace can name it. **Ruling 79.**
    ///
    /// `authenticate()` may drive this same read (ruling 75) and meet the
    /// same local fault, and §18.2's `slither::io` row asks for "the
    /// provider's own error **and the verb that met it**". A single event
    /// naming the application's verb is more use to an operator than two
    /// events naming an internal call chain, so the name is passed down
    /// rather than emitted twice.
    fn read_identity_as(
        &mut self,
        id: IntroId,
        verb: &'static str,
    ) -> Result<PublicKeyOf<I>, IntroError> {
        let msg1 = {
            let entry = self.intros.get(id).ok_or(IntroError::Expired)?;
            match &entry.state {
                // Ruling 74: a repeat call is answered from the chain, at 0
                // incremental DH and with no provider opened.
                ChainState::Claimed { claimed, .. } => return Ok(claimed.clone()),
                ChainState::Proven { peer, .. } => return Ok(peer.clone()),
                ChainState::Poisoned => {
                    debug_assert!(false, "Poisoned is never observable");
                    return Err(IntroError::Expired);
                }
                ChainState::Parked => entry.msg1.clone(),
            }
        };

        // The responder machine is built HERE, not at park: see the module
        // docs, and §17.5's ceiling on concurrent provider handles.
        //
        // Ruling 72: both of these are *our* provider failing, at 0 DH.
        // `Local`, and the chain stays parked — note the bare `return`,
        // with no `discard_chain`, which is the whole difference from the
        // hiss arm below.
        //
        // Ruling 79: the variant cannot carry the provider's error — the
        // §18.1 types are `Clone + PartialEq + Eq + Send + Sync` and
        // `Identity::Error` is bounded on none of them, deliberately, since
        // an enclave provider is `!Send`. So the audiences split: the
        // application gets the variant it can act on, and the operator gets
        // the detail on §18.2's `slither::io`.
        let (provider, our_key) = match self.identity.open() {
            Ok(opened) => opened,
            Err(error) => {
                tracing::warn!(
                    target: "slither::io",
                    verb,
                    stage = "Identity::open",
                    %error,
                    "the identity provider failed to open"
                );
                return Err(IntroError::Local);
            }
        };
        let responder =
            match <I::Suite as Handshake>::responder(provider, constants::PROLOGUE, our_key) {
                Ok(responder) => responder,
                Err(error) => {
                    // Also `Local`, and also ours: this is hiss refusing to
                    // build a responder on *our* static, with nothing of the
                    // peer's involved yet. `stage` says which of the two it
                    // was, so §18.2's row — which names `Identity::open()` —
                    // stays legible beside it.
                    tracing::warn!(
                        target: "slither::io",
                        verb,
                        stage = "Handshake::responder",
                        %error,
                        "the responder machine would not build on our static"
                    );
                    return Err(IntroError::Local);
                }
            };

        match <I::Suite as Handshake>::read_msg1_intro(responder, &msg1) {
            Ok((claimed, mid)) => {
                // §17.1: a staged mid-state pins its static's guard entry,
                // and the pin **never creates** one — for a static no
                // key-holder has ever written, this is a no-op.
                //
                // Ruling 77: `Claimed`, and that is the whole point. The
                // static here is *claimed*, reached for 1 DH and proving
                // nothing (§6.1: attacker-choosable), so this pin bars
                // eviction but must not touch the orphan clock.
                let key = claimed.as_ref().to_vec();
                let pinned = self.guard.pin(&key, PinKind::Claimed);

                self.intros.consume(id);
                let entry = self
                    .intros
                    .get_mut(id)
                    .expect("the chain was present at the top of this verb");
                entry.state = ChainState::Claimed {
                    mid: Box::new(mid),
                    claimed: claimed.clone(),
                };
                if pinned {
                    entry.guard_pin = Some(ChainPin {
                        key,
                        kind: PinKind::Claimed,
                    });
                }
                Ok(claimed)
            }
            Err(_) => {
                // Ruling 72, the other half: the peer's bytes are at fault,
                // 1 DH is spent and the verdict is definitive — so this arm
                // alone destroys the entry and frees its stage-0 slot.
                //
                // Not `discard_chain`, and that is the point: ruling 80
                // left this verb without a `now` on the grounds that it
                // never has guard state to release, and this arm is the
                // only place it removes an entry. The entry reaching here
                // was `Parked` (checked at the top), and a `Parked` entry
                // holds neither a provisional write nor a pin — both are
                // written only at this verb's success arm and at
                // `authenticate()`, and §6.3 never byte-replaces a consumed
                // entry back into `Parked`. So there is nothing to release
                // and no instant to release it at.
                let discarded = self.intros.remove(id);
                debug_assert!(
                    discarded
                        .as_ref()
                        .is_none_or(|e| e.guard_pin.is_none() && e.guard_undo.is_none()),
                    "a parked chain holds no guard state"
                );
                Err(IntroError::Malformed)
            }
        }
    }

    /// §16.4's `authenticate` — §6.1's stage 2, **2 DH cumulative**
    /// (`+ss`). The guard admits here, post-`ss`, which is what keeps
    /// §17.1's write path key-holder-only.
    ///
    /// # It advances a still-parked chain (**ruling 75**)
    ///
    /// §6.2's typestate makes `read_identity()` unskippable on the handle
    /// path; the core is keyed by `IntroId` and has no such fence. A chain
    /// still at stage 0 is therefore driven through the skipped `es` here
    /// rather than refused — §6.1 prices this verb at **2 DH cumulative**,
    /// so doing the missing work lands on *exactly* the ratified amount,
    /// and no error variant has to be invented for a case §18.1 does not
    /// name. Like ruling 74 above, this is resolved in the direction that
    /// cannot perturb §6.1's ladder: the permissive answer is the one that
    /// costs what the table already says.
    ///
    /// # The write is provisional
    ///
    /// A successful record hands the chain a `GuardUndo` and is undone by
    /// every path that ends the chain without accepting it — §17.1
    /// mitigation (i), which is explicit that this covers a dropped chain
    /// and an `accept()` returning `Stale` alike.
    pub(crate) fn authenticate(
        &mut self,
        now: Instant,
        id: IntroId,
    ) -> Result<(PublicKeyOf<I>, Timestamp), AuthError> {
        if matches!(
            self.intros.get(id).map(|entry| &entry.state),
            Some(ChainState::Parked)
        ) {
            // Ruling 75's `es`, driven here — so this verb can meet the
            // same local fault `read_identity()` can, which is why §18.1
            // gained `AuthError::Local` (ruling 78). Matched exhaustively:
            // a sixth `IntroError` variant must stop here rather than be
            // swept into a security signal by a `_` arm.
            self.read_identity_as(id, "authenticate")
                .map_err(|e| match e {
                    IntroError::Expired => AuthError::Expired,
                    // Ruling 78. Routing *our* locked enclave to
                    // `HandshakeFailed` did not merely misattribute the fault;
                    // it reported the peer as an attacker through the one
                    // variant §18.1 designates a security signal.
                    IntroError::Local => AuthError::Local,
                    // Ruling 72's other half: the peer's bytes really are at
                    // fault, and this is what `HandshakeFailed` is for.
                    IntroError::Malformed => AuthError::HandshakeFailed,
                    // Neither is reachable **from here**, and the reasons
                    // differ. `EndpointDropped` is the driver stopping, which
                    // the core cannot report. `Internal` is §6.5 step 4's
                    // interception, which belongs to the `read_identity()`
                    // verb and not to ruling 75's drive of the same `es`:
                    // §18.1 has an `IntroError::Internal` and **no
                    // `AuthError::Internal`**, so a chain authenticated
                    // straight from `Parked` is not intercepted — it reaches
                    // the identical comparison at `accept()`, by §6.4's
                    // PENDING branch. Either way, answer with the lifecycle
                    // variant rather than the security one.
                    IntroError::Internal | IntroError::EndpointDropped => AuthError::Expired,
                })?;
        }

        let (mid, claimed) = {
            let entry = self.intros.get_mut(id).ok_or(AuthError::Expired)?;
            match &entry.state {
                ChainState::Proven {
                    peer, timestamp, ..
                } => {
                    return Ok((peer.clone(), *timestamp));
                }
                ChainState::Claimed { .. } => {}
                _ => return Err(AuthError::Expired),
            }
            match ::core::mem::replace(&mut entry.state, ChainState::Poisoned) {
                ChainState::Claimed { mid, claimed } => (mid, claimed),
                other => {
                    entry.state = other;
                    return Err(AuthError::Expired);
                }
            }
        };

        let Ok((payload, read)) = <I::Suite as Handshake>::complete(*mid) else {
            // §6.1: a tail-tag failure is `HandshakeFailed`. It is a
            // security signal and carries no detail, deliberately.
            self.discard_chain(now, id);
            return Err(AuthError::HandshakeFailed);
        };
        let timestamp = Timestamp::decode(&payload);
        // The `ss` has proven possession, so the claimed static is now the
        // peer static — and this is the first moment §17.1 permits a write.
        let key = claimed.as_ref().to_vec();

        if !self.guard.admits(&key, timestamp) {
            // Mitigation (iii): recency refreshes on a successful record,
            // **never** on a failed check. Nothing is written here.
            self.discard_chain(now, id);
            return Err(AuthError::Replay);
        }
        let undo = self.guard.record(&key, timestamp, now);

        // The record may have created the entry the pin could not, so take
        // the mid-state's pin now if `read_identity` found nothing to pin.
        //
        // Ruling 77: either way the pin ends up a **key-holder's** — the
        // `ss` above is what proves it, and §17.1 names a `Proven` chain
        // beside a live connection. A pin `read_identity` already took was
        // a merely-`Claimed` one and is promoted rather than doubled.
        let pinned = match self.intros.get(id).map(|entry| entry.guard_pin.is_some()) {
            Some(false) => self.guard.pin(&key, PinKind::KeyHolder),
            Some(true) => {
                self.guard.promote_pin(&key);
                false
            }
            // The chain vanished, which the check immediately below turns
            // into a revert. Touch no pin from here.
            None => false,
        };

        if self.intros.get(id).is_none() {
            debug_assert!(false, "the chain cannot vanish mid-verb");
            self.guard.revert(undo);
            return Err(AuthError::Expired);
        }
        let entry = self
            .intros
            .get_mut(id)
            .expect("presence checked immediately above");
        entry.state = ChainState::Proven {
            state: Box::new(read),
            peer: claimed.clone(),
            timestamp,
        };
        entry.guard_undo = Some(undo);
        match &mut entry.guard_pin {
            // Promoted above: the chain is `Proven`, so its pin is a
            // key-holder's and must be released as one (ruling 77).
            Some(pin) => pin.kind = PinKind::KeyHolder,
            None if pinned => {
                entry.guard_pin = Some(ChainPin {
                    key,
                    kind: PinKind::KeyHolder,
                });
            }
            None => {}
        }

        Ok((claimed, timestamp))
    }

    /// §16.4's `accept` — §6.1's stage 3, **4 DH cumulative** (`+ee`,
    /// `+se`), and msg2 sent.
    ///
    /// The session anchors at the **msg1 source address** (§5.6), which is
    /// what arms §7.3's amplification budget; the budget itself is slice 7.
    ///
    /// # §5.4's three-valued rule, and where each value goes
    ///
    /// The proven static is looked up in the static map.
    ///
    /// * **NONE** — the ordinary fresh install below.
    /// * **PENDING** — §6.4's PENDING branch, which applies **§6.7's
    ///   comparison over the same ordered pair of statics**. Winner:
    ///   [`AcceptError::Stale`], the pending left in place, and the guard
    ///   record **kept**. Loser: the pending is cancelled, its `Connecting`
    ///   resolves `Err(ConnectError::AlreadyConnected)`, and this call
    ///   proceeds as an ordinary fresh install with this endpoint as
    ///   responder. §6.6 calls this "a **different route to the same
    ///   comparison**", so both halves call the same functions §6.6's
    ///   internal route does — see [`routing`](super::routing).
    /// * **LIVE** — still [`AcceptError::Stale`], and that one is a knowing
    ///   boundary: §6.4's re-home walk and its proven-LIVE replacement
    ///   admission are a later slice. What `Stale` preserves meanwhile is
    ///   §16.1's one-session-per-peer invariant, which it cannot violate.
    ///
    /// # The guard record, and its one exception
    ///
    /// A `Stale` normally **reverts** the chain's provisional record —
    /// §17.1 mitigation (i) names "an `accept()` that returns
    /// `AcceptError::Stale`" alongside a dropped chain. §6.4 carves out
    /// exactly one case: *"the tie-break-**winner** case of the PENDING
    /// branch below returns `Stale` and **keeps** its record — that record
    /// *is* the point of the branch (§6.7's winner-side record), the write
    /// that denies a later replay of that same initiation the vacuous
    /// guard pass it would otherwise enjoy. **No other `Stale` leaves a
    /// record behind.**"* The LIVE arm and the not-proven arms therefore
    /// still revert, and only the winner arm does not.
    pub(crate) fn accept(
        &mut self,
        now: Instant,
        id: IntroId,
    ) -> Result<(ConnectionId, Connection<I::Suite>), AcceptError> {
        let (peer_key, timestamp, anchor, peer_index) = {
            let entry = self.intros.get(id).ok_or(AcceptError::Stale)?;
            match &entry.state {
                ChainState::Proven {
                    peer, timestamp, ..
                } => (
                    peer.as_ref().to_vec(),
                    *timestamp,
                    entry.src,
                    entry.sender_index,
                ),
                // Not proven: §6.2's typestate reaches `accept()` only
                // through `authenticate()`, and `Stale` is the only
                // non-`EndpointDropped` variant §18.1 offers.
                _ => return Err(AcceptError::Stale),
            }
        };

        // §5.4's three-valued rule. `lost_tiebreak` rides through to the
        // install because §17.1's `HANDSHAKE_GIVEUP` extension attaches to
        // the row this call is about to write, not to the one it removed.
        let mut lost_tiebreak = false;
        match self
            .statics
            .get(&peer_key)
            .map(|entry| (entry.state, entry.conn))
        {
            None => {}
            Some((StaticState::Live, _)) => {
                self.discard_chain(now, id);
                return Err(AcceptError::Stale);
            }
            Some((StaticState::Pending, dial)) => {
                if self.wins_tiebreak(&peer_key) {
                    self.keep_winner_side_record(now, id, dial, &peer_key, timestamp);
                    return Err(AcceptError::Stale);
                }
                self.cancel_pending_losing_tiebreak(now, dial);
                lost_tiebreak = true;
            }
        }

        // §16.6: the sub-seed is drawn at connection creation, before the
        // index, so the endpoint's draw order is the same shape as
        // `connect()`'s and later connection-side randomness cannot
        // perturb it.
        let sub_seed = self.draw_sub_seed();
        let our_index = self.indices.mint(&mut self.rng);

        let read = {
            let entry = self
                .intros
                .get_mut(id)
                .expect("the chain was present at the top of this verb");
            match ::core::mem::replace(&mut entry.state, ChainState::Poisoned) {
                ChainState::Proven { state, .. } => state,
                other => {
                    entry.state = other;
                    return Err(AcceptError::Stale);
                }
            }
        };

        let Ok((msg2, transport)) = <I::Suite as Handshake>::write_msg2(*read) else {
            self.discard_chain(now, id);
            return Err(AcceptError::Stale);
        };
        let (seal, open) = <I::Suite as Handshake>::into_datagram(transport, self.epoch_size());

        // mac1 on the response is keyed on the **recipient's** static —
        // the initiator's, which the `ss` has just proven they hold.
        let peer_mac1 = Mac1Key::derive(&peer_key);
        let data = framing::frame_resp(our_index, peer_index, &msg2, &peer_mac1);

        let conn = self.next_connection_id();
        self.indices.insert_session(our_index, conn);
        self.statics.insert(
            peer_key,
            StaticEntry {
                conn,
                state: StaticState::Live,
                // §17.4: established connections contribute no hints.
                dialled: None,
                // §17.4: `Some(t)` when **we responded**. Written once and
                // never updated; §6.4's admission is its only reader.
                replacement_basis: Some(timestamp),
                // §17.1's `HANDSHAKE_GIVEUP` extension — see
                // `keep_winner_side_record` for the reading this rests on.
                // An ordinary NONE-path accept is an ordinary staged
                // admission and gets none.
                guard_exempt: lost_tiebreak,
            },
        );

        // The provisional write becomes permanent and the mid-state's pin
        // becomes the connection's: drop both handles WITHOUT reverting or
        // unpinning. `Retired` releases the pin at teardown (§16.4).
        if let Some(mut entry) = self.intros.remove(id) {
            entry.guard_undo = None;
            entry.guard_pin = None;
        }

        self.emit(EndpointOutput::Transmit(Transmit { to: anchor, data }));

        let session = EstablishedSession {
            seal,
            open,
            our_index,
            peer_index,
            anchor,
        };
        // `now` is the install instant: §7.4 pins both liveness clocks to
        // it and starts the death deadline already armed, which is what
        // makes "a half-open session is reaped by liveness in 25 s" a fact.
        Ok((conn, Connection::established(now, sub_seed, session)))
    }

    /// §16.4's `reject`. Infallible, and it **emits nothing**: §6.1 makes
    /// dropping the object at any stage a *silent* reject — no msg2, no
    /// packet, no record kept (ruling 48).
    ///
    /// An unknown or expired `id` is a no-op, which is why §16.4 gives this
    /// verb no `Result`.
    /// `now` is **[RATIFIED 2026/08/15 — ruling 80]**: this verb releases
    /// a §17.1 pin, and ruling 73 measures the orphan TTL from that
    /// release. §16.4's own invariant already required it — a `now` on
    /// every mutating call — so the argument closes a gap rather than
    /// widening the surface.
    pub(crate) fn reject(&mut self, now: Instant, id: IntroId) {
        self.discard_chain(now, id);
    }

    /// Remove a chain and undo everything it provisionally held.
    fn discard_chain(&mut self, now: Instant, id: IntroId) {
        if let Some(entry) = self.intros.remove(id) {
            self.release_chain_guard_state(now, entry.guard_undo, entry.guard_pin);
        }
    }

    /// §6.4's PENDING branch, **winner side**: refuse, keep the pending,
    /// and keep the record.
    ///
    /// *"`accept()` returns `AcceptError::Stale`, the pending is **left in
    /// place**, and the candidate's timestamp is **recorded** in the guard
    /// exactly as §6.7's winner-side record does (§17.1) — the candidate
    /// authenticated post-`ss`, so the write is a key-holder write like
    /// every other."*
    ///
    /// # Why this is `discard_chain`'s opposite, and why that is right
    ///
    /// [`discard_chain`](Self::discard_chain) reverts §17.1's provisional
    /// write, which is mitigation (i) and correct for every other refusal.
    /// Here it would be **wrong**, and §17.1 says so from the other side:
    /// the winner's record is one of its four write sites, made precisely
    /// so "a later replay of that same initiation" cannot enjoy "the
    /// vacuous guard pass it would otherwise enjoy against an endpoint
    /// that has admitted nothing from this peer". `authenticate()` already
    /// wrote the candidate's timestamp; **keeping** it — dropping the
    /// `GuardUndo` unused — *is* the winner-side record. Writing it again
    /// would be the same value at the same instant.
    ///
    /// The chain's **pin** still goes: §17.1 pins an entry while "a staged
    /// mid-state exists for its static", and this call is where that
    /// mid-state stops existing. The record is what stays, not the pin.
    /// The entry survives the release regardless, because the pending
    /// holds a pin of its own — and if it does not yet, this takes it
    /// (see [`record_tiebreak_timestamp`]).
    ///
    /// Ordering against `unpin` is deliberate: the exemption is armed and
    /// the pending's pin secured **before** the chain's pin is released,
    /// so the entry is never momentarily an unpinned, unexempt orphan.
    ///
    /// [`record_tiebreak_timestamp`]: Endpoint::record_tiebreak_timestamp
    fn keep_winner_side_record(
        &mut self,
        now: Instant,
        id: IntroId,
        dial: ConnectionId,
        peer_key: &[u8],
        timestamp: Timestamp,
    ) {
        // The **candidate's** timestamp, which `authenticate()` has already
        // written at this same key — so this is idempotent against that
        // write, and what it adds is §17.1's `HANDSHAKE_GIVEUP` extension
        // and the pin `mint_pending` could not take.
        debug_assert_eq!(
            self.guard.greatest(peer_key),
            Some(timestamp),
            "§17.1: `authenticate()` recorded this candidate before `accept()` ran"
        );
        self.record_tiebreak_timestamp(now, dial, peer_key, timestamp);

        if let Some(entry) = self.intros.remove(id) {
            if let Some(pin) = entry.guard_pin {
                self.guard.unpin(&pin.key, pin.kind, now);
            }
            // **Not** reverted. §6.4: "No other `Stale` leaves a record
            // behind" — this is the other one.
            drop(entry.guard_undo);
        }
    }
}

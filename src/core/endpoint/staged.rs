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
    /// A chain that already carries a mid-state — §6.5's pre-read entry, or
    /// slice 7's eager-demoted one — returns its cached result at **0
    /// incremental DH**, and §6.1 is explicit that the cumulative table is
    /// unchanged either way. Any assertion about cost has to be cumulative
    /// to be true of both.
    ///
    /// # Two failure modes, treated differently on purpose
    ///
    /// A **local** failure (the identity will not open, our own static will
    /// not install) has spent **0 DH** and may well be transient — an
    /// enclave that is momentarily locked — so the chain is left parked and
    /// a retry can still succeed. A **hiss** failure means msg1 is
    /// structurally unreadable: 1 DH is spent, the verdict is definitive,
    /// and the chain is discarded. §18.1's `IntroError` is closed and has
    /// no variant for a local provider fault, so both report `Malformed`;
    /// see the slice notes.
    pub(crate) fn read_identity(&mut self, id: IntroId) -> Result<PublicKeyOf<I>, IntroError> {
        let msg1 = {
            let entry = self.intros.get(id).ok_or(IntroError::Expired)?;
            match &entry.state {
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
        let Ok((provider, our_key)) = self.identity.open() else {
            return Err(IntroError::Malformed);
        };
        let Ok(responder) =
            <I::Suite as Handshake>::responder(provider, constants::PROLOGUE, our_key)
        else {
            return Err(IntroError::Malformed);
        };

        match <I::Suite as Handshake>::read_msg1_intro(responder, &msg1) {
            Ok((claimed, mid)) => {
                // §17.1: a staged mid-state pins its static's guard entry,
                // and the pin **never creates** one — for a static no
                // key-holder has ever written, this is a no-op.
                let key = claimed.as_ref().to_vec();
                let pinned = self.guard.pin(&key);

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
                    entry.guard_pin = Some(key);
                }
                Ok(claimed)
            }
            Err(_) => {
                self.discard_chain(id);
                Err(IntroError::Malformed)
            }
        }
    }

    /// §16.4's `authenticate` — §6.1's stage 2, **2 DH cumulative**
    /// (`+ss`). The guard admits here, post-`ss`, which is what keeps
    /// §17.1's write path key-holder-only.
    ///
    /// # It advances a still-parked chain
    ///
    /// §6.2's typestate makes `read_identity()` unskippable on the handle
    /// path; the core is keyed by `IntroId` and has no such fence. A chain
    /// still at stage 0 is therefore driven through the `es` here rather
    /// than refused — §6.1 prices this verb at **2 DH cumulative**, so
    /// doing the missing work costs exactly what the table says, and no
    /// error variant has to be invented for a case §18.1 does not name.
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
            self.read_identity(id).map_err(|e| match e {
                IntroError::Expired => AuthError::Expired,
                _ => AuthError::HandshakeFailed,
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
            self.discard_chain(id);
            return Err(AuthError::HandshakeFailed);
        };
        let timestamp = Timestamp::decode(&payload);
        // The `ss` has proven possession, so the claimed static is now the
        // peer static — and this is the first moment §17.1 permits a write.
        let key = claimed.as_ref().to_vec();

        if !self.guard.admits(&key, timestamp) {
            // Mitigation (iii): recency refreshes on a successful record,
            // **never** on a failed check. Nothing is written here.
            self.discard_chain(id);
            return Err(AuthError::Replay);
        }
        let undo = self.guard.record(&key, timestamp, now);

        // The record may have created the entry the pin could not, so take
        // the mid-state's pin now if `read_identity` found nothing to pin.
        let needs_pin = self
            .intros
            .get(id)
            .is_some_and(|entry| entry.guard_pin.is_none());
        let pinned = needs_pin && self.guard.pin(&key);

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
        if pinned {
            entry.guard_pin = Some(key);
        }

        Ok((claimed, timestamp))
    }

    /// §16.4's `accept` — §6.1's stage 3, **4 DH cumulative** (`+ee`,
    /// `+se`), and msg2 sent.
    ///
    /// The session anchors at the **msg1 source address** (§5.6), which is
    /// what arms §7.3's amplification budget; the budget itself is slice 7.
    ///
    /// # §5.4's three-valued rule, and this slice's boundary
    ///
    /// The proven static is looked up in the static map: **LIVE** and
    /// **PENDING** both return [`AcceptError::Stale`] here. `Stale` is
    /// already the correct answer for PENDING (§6.4's tie-break-winner
    /// branch); for LIVE it is a knowing, documented boundary — §6.4's
    /// replacement admission is slice 7 — and what it preserves meanwhile
    /// is §16.1's one-session-per-peer invariant, which returning `Stale`
    /// cannot violate.
    ///
    /// Either way the chain's provisional guard record is **reverted**:
    /// §17.1 mitigation (i) names an `accept()` that returns `Stale`
    /// alongside a dropped chain.
    pub(crate) fn accept(
        &mut self,
        _now: Instant,
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

        if self.statics.get(&peer_key).is_some() {
            self.discard_chain(id);
            return Err(AcceptError::Stale);
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
            self.discard_chain(id);
            return Err(AcceptError::Stale);
        };
        let (seal, open) = <I::Suite as Handshake>::into_datagram(transport, Self::epoch_size());

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
                // never updated; §6.4 in slice 7 is its only reader.
                replacement_basis: Some(timestamp),
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
        Ok((conn, Connection::established(sub_seed, session)))
    }

    /// §16.4's `reject`. Infallible, and it **emits nothing**: §6.1 makes
    /// dropping the object at any stage a *silent* reject — no msg2, no
    /// packet, no record kept (ruling 48).
    ///
    /// An unknown or expired `id` is a no-op, which is why §16.4 gives this
    /// verb no `Result`.
    pub(crate) fn reject(&mut self, id: IntroId) {
        self.discard_chain(id);
    }

    /// Remove a chain and undo everything it provisionally held.
    fn discard_chain(&mut self, id: IntroId) {
        if let Some(entry) = self.intros.remove(id) {
            self.release_chain_guard_state(entry.guard_undo, entry.guard_pin);
        }
    }
}

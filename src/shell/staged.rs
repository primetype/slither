//! §6.2's staged accept — `Intro` → `Claimed` → `Proven` → `Connection`.
//!
//! The ladder an application climbs one DH at a time, so it can look at a
//! peer's *claimed* identity before spending a second DH proving it. Each
//! stage's `async` verb is a driver round-trip, because §6.2 requires the DH
//! cost to land on the driver task (§16.3, ruling 53).
//!
//! ```text
//! Intro     0 DH   source(), sender_index()
//!   │ read_identity()   +1 DH  (es)
//! Claimed   1 DH   claimed_static()          ← CLAIMED, not proven
//!   │ authenticate()    +1 DH  (ss)          ← §17.1's guard admits here
//! Proven    2 DH   peer_static(), timestamp()
//!   │ accept()          +2 DH  (ee, se)
//! Connection
//! ```
//!
//! # Dropping is the rejection
//!
//! Dropping a staged object at **any** stage is the application's
//! rejection, and the only rejection there is: slither keeps no record of it
//! (§6.1, ruling 48) and the peer is told nothing. There is no `reject()`
//! verb because there is nothing for one to do that `drop` does not.
//!
//! # A staged object is not a handle
//!
//! §16.3 is explicit: "a staged object's verb is a **round-trip to a driver
//! it does not keep alive**". Holding an `Intro` while dropping every
//! `Endpoint`, `Connecting` and `Connection` stops the driver, and the next
//! verb resolves `EndpointDropped` — which is exactly why `IntroError`,
//! `AuthError` and `AcceptError` each carry that variant while
//! `ConnectError` does not (ruling 62).

use std::net::SocketAddr;

use tokio::sync::oneshot;

use crate::core::{IntroId, Timestamp};
use crate::error::{AcceptError, AuthError, IntroError};
use crate::identity::{Identity, PublicKeyOf};

use super::connection::Connection;
use super::shared::{Command, Shell};

/// A parked introduction — §6.1's **stage 0**, at **0 DH**.
///
/// Nothing has been spent on it: it is msg1's bytes plus the address they
/// came from, and it has passed §3.1's length gate and mac1 and nothing
/// else. Drop is a silent reject.
pub struct Intro<I: Identity> {
    shell: Shell<I>,
    id: IntroId,
    /// The value at hand-over, used only if the parked entry is gone.
    source: SocketAddr,
    sender_index: u32,
    consumed: bool,
}

impl<I: Identity> Intro<I> {
    pub(crate) fn new(shell: Shell<I>, id: IntroId, source: SocketAddr, sender_index: u32) -> Self {
        Self {
            shell,
            id,
            source,
            sender_index,
            consumed: false,
        }
    }

    /// Where this initiation came from.
    ///
    /// **Attacker-chosen, at 0 DH, with no return-routability proof — do
    /// not key durable state on it.** §6.1 forbids that in terms, and it
    /// names **three** quantities, not one: *"Nothing durable may be keyed
    /// on the claimed static, the source address, or `sender_index` — no
    /// map insertion, no rate-limit bucket, no unbounded logging."* This is
    /// documentation obligation #2 (see the [crate docs](crate)) at the
    /// rung *below* the claimed static, and the address is the **cheaper**
    /// of the two to abuse, not the safer: a claimed static costs an
    /// attacker one DH and requires knowing some real public key, while a
    /// source address costs nothing and can name any host on the internet.
    /// Nothing has answered from this address at stage 0 — the ladder's
    /// first reply is msg2, which `accept()` sends — so a spoofed source is
    /// free, and a denylist or a rate-limit bucket keyed here bans whoever
    /// the attacker wrote in the packet. Under the flood §6.3 describes,
    /// that is precisely the reflex to expect.
    ///
    /// Use it to **route, log at bounded volume, and decide** — an address
    /// is legitimate input to an authorisation policy. Never use it to
    /// punish, and never let it grow a map.
    ///
    /// Read **live** from the parked entry (ruling 71): §6.3 requires a
    /// refreshed entry's accessors to reflect the newest bytes at call
    /// time. For the source address that happens to be invariant under
    /// replacement — the address *is* §6.3's dedup key — but reading it
    /// live is what makes that a property of the code rather than of a
    /// comment. If the entry has expired since hand-over, the value it had
    /// then is returned; an expired introduction still came from somewhere.
    pub fn source(&self) -> SocketAddr {
        self.shell
            .state
            .borrow()
            .endpoint
            .intro_source(self.id)
            .unwrap_or(self.source)
    }

    /// The `sender_index` on the initiation currently parked here.
    ///
    /// **The third of §6.1's three attacker-chosen quantities — do not key
    /// durable state on it either**, and see [`source`](Self::source) for
    /// the rule and the reason. This one carries its own trap: §5.5 mints a
    /// fresh random index on **every retransmit**, so it is not stable even
    /// for a genuine peer, and a map keyed on it grows one entry per
    /// retransmit of one honest initiation.
    ///
    /// Read **live**, and here it is load-bearing (ruling 71): §5.5 mints a
    /// **new random index on every retransmit**, and §6.3's dedup replaces
    /// a same-source entry *without re-surfacing it*, so a value cached
    /// when this handle was created is no longer the one on the wire the
    /// moment the peer retransmits.
    pub fn sender_index(&self) -> u32 {
        self.shell
            .state
            .borrow()
            .endpoint
            .intro_sender_index(self.id)
            .unwrap_or(self.sender_index)
    }

    /// §6.1's stage 1 — **1 DH** (`es`), 0 on a pre-read entry.
    ///
    /// Reveals the peer's **claimed** static: an unauthenticated assertion,
    /// made before any DH proves possession. This is documentation
    /// obligation #2 — **authorise on it, never punish on it**. Denylisting
    /// on a claimed static lets an attacker claim any public key in order
    /// to get its owner banned.
    ///
    /// On any error the chain is discarded. §6.2's typestate consumes the
    /// handle, so there is no retry to preserve it for — and leaving it
    /// parked would be worse than discarding it: §6.3's dedup replaces a
    /// same-source entry **without re-surfacing it**, so the peer's next
    /// retransmit would find the slot occupied by a chain no application
    /// can reach, and would go unanswered until `INTRO_TTL`.
    /// # The key this returns is **claimed**, not authenticated — S7
    ///
    /// **Do not denylist on it.** This is stage 0: the peer has asserted a
    /// static public key and *nothing has verified that it holds the
    /// matching private key*. Only [`Claimed::authenticate`] does that.
    ///
    /// An attacker can therefore claim **any** public key it likes. If a
    /// host bans the key this returns, an attacker bans a third party by
    /// naming them: one unauthenticated packet per victim, no key material,
    /// no session. A denylist keyed here is a denial-of-service primitive
    /// pointed at your own users.
    ///
    /// Rate-limiting, logging and routing on a claimed key are fine — they
    /// cost the claimant something or cost the victim nothing. Anything
    /// that **persists a penalty against the named identity** belongs after
    /// [`authenticate`](Claimed::authenticate), which is the first point at
    /// which the key is the peer's.
    ///
    /// # Cancel-safety
    ///
    /// The `Intro` stays alive across the round-trip and is only marked
    /// consumed once the reply is in hand, so **dropping this future
    /// rejects the chain** — §6.2's silent reject, freeing the stage-0 slot
    /// at once. Marking it consumed up front would instead orphan the
    /// chain until `INTRO_TTL`, holding one of §6.3's four per-source slots
    /// against a peer for 15 s.
    pub async fn read_identity(mut self) -> Result<Claimed<I>, IntroError> {
        let shell = self.shell.clone();
        let id = self.id;
        let result = round_trip(&shell, |reply| Command::ReadIdentity(id, reply)).await;
        // Reached only if the future was not dropped during the await.
        self.consumed = true;
        match result {
            Some(Ok(claimed)) => Ok(Claimed {
                shell,
                id,
                claimed_static: claimed,
                consumed: false,
            }),
            Some(Err(error)) => {
                shell.send(Command::Reject(id));
                Err(error)
            }
            None => Err(IntroError::EndpointDropped),
        }
    }
}

impl<I: Identity> Drop for Intro<I> {
    fn drop(&mut self) {
        if !self.consumed {
            self.shell.send(Command::Reject(self.id));
        }
    }
}

impl<I: Identity> std::fmt::Debug for Intro<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Both fields printed here are §6.1 attacker-chosen quantities, and
        // `Claimed`'s `Debug` below suppresses its one for that reason —
        // so whether this should suppress too is a live question and is
        // **deliberately not answered here**. It stays as it is: an
        // `Intro`'s `Debug` is an operator's only view of a stage-0
        // arrival, and the two values are diagnostics rather than a
        // secret-ish assertion of identity. What §6.1 forbids is *unbounded
        // logging* and durable keying, which is a property of the caller's
        // loop and not of this impl — see `source()` for the warning.
        f.debug_struct("Intro")
            .field("source", &self.source())
            .field("sender_index", &self.sender_index())
            .finish_non_exhaustive()
    }
}

/// A chain whose peer has **claimed** a static — §6.1's stage 1, at 1 DH
/// cumulative.
///
/// The claim is not proof. Drop is a silent reject.
pub struct Claimed<I: Identity> {
    shell: Shell<I>,
    id: IntroId,
    claimed_static: PublicKeyOf<I>,
    consumed: bool,
}

impl<I: Identity> Claimed<I> {
    /// The static the peer **claims**.
    ///
    /// Named `claimed_static`, never `remote_static`, and the name is the
    /// documentation: at this stage possession has not been proven. §6.1's
    /// `ss` in [`authenticate`](Self::authenticate) is what proves it.
    pub fn claimed_static(&self) -> &PublicKeyOf<I> {
        &self.claimed_static
    }

    /// §6.1's stage 2 — **+1 DH** (`ss`), 2 cumulative.
    ///
    /// Possession is proven here, and §17.1's timestamp guard admits the
    /// initiation here. On any error the chain is discarded, for the reason
    /// given on [`Intro::read_identity`].
    ///
    /// Cancel-safe on the same terms as
    /// [`Intro::read_identity`](Intro::read_identity): a dropped future
    /// rejects the chain rather than orphaning it.
    pub async fn authenticate(mut self) -> Result<Proven<I>, AuthError> {
        let shell = self.shell.clone();
        let id = self.id;
        let result = round_trip(&shell, |reply| Command::Authenticate(id, reply)).await;
        self.consumed = true;
        match result {
            Some(Ok((peer_static, timestamp))) => Ok(Proven {
                shell,
                id,
                peer_static,
                timestamp,
                consumed: false,
            }),
            Some(Err(error)) => {
                shell.send(Command::Reject(id));
                Err(error)
            }
            None => Err(AuthError::EndpointDropped),
        }
    }
}

impl<I: Identity> Drop for Claimed<I> {
    fn drop(&mut self) {
        if !self.consumed {
            self.shell.send(Command::Reject(self.id));
        }
    }
}

impl<I: Identity> std::fmt::Debug for Claimed<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The claimed static is deliberately not printed: it is an
        // unauthenticated assertion, and a `Debug` that showed it invites
        // exactly the logging that documentation obligation #2 warns about.
        f.debug_struct("Claimed").finish_non_exhaustive()
    }
}

/// A chain whose peer has **proven** possession — §6.1's stage 2, at 2 DH
/// cumulative.
///
/// Drop is a silent reject.
pub struct Proven<I: Identity> {
    shell: Shell<I>,
    id: IntroId,
    peer_static: PublicKeyOf<I>,
    timestamp: Timestamp,
    consumed: bool,
}

impl<I: Identity> Proven<I> {
    /// The peer's static — **proven**, and safe to authorise or deny on.
    pub fn peer_static(&self) -> &PublicKeyOf<I> {
        &self.peer_static
    }

    /// §5.2's initiation timestamp, as the peer sent it.
    ///
    /// The one wall-clock reading in the protocol (§5.3), carried verbatim:
    /// it is **not** range-checked, because §5.3 specifies no validation and
    /// a slither that rejected an out-of-range value would have added a wire
    /// behaviour the spec does not have.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// §6.1's stage 3 — **+2 DH** (`ee`, `se`), 4 cumulative.
    ///
    /// Writes msg2 and installs the session, returning the established
    /// [`Connection`].
    ///
    /// # Cancel-safety
    ///
    /// Dropping the future before the reply arrives rejects the chain (a
    /// no-op if the driver has already consumed it). Dropping it *after*
    /// the driver has built the connection drops that `Connection` — the
    /// last handle to it — which performs §16.2's `close(NO_ERROR, "")`.
    /// That is the right answer for a session nobody claimed: the peer is
    /// told at once rather than waiting out `DEAD_TIMEOUT`.
    pub async fn accept(mut self) -> Result<Connection<I::Suite>, AcceptError> {
        let shell = self.shell.clone();
        let id = self.id;
        let peer_static = self.peer_static.clone();
        let result = round_trip(&shell, move |reply| {
            Command::AcceptChain(id, peer_static, reply)
        })
        .await;
        self.consumed = true;
        match result {
            Some(result) => result,
            None => Err(AcceptError::EndpointDropped),
        }
    }
}

impl<I: Identity> Drop for Proven<I> {
    fn drop(&mut self) {
        if !self.consumed {
            self.shell.send(Command::Reject(self.id));
        }
    }
}

impl<I: Identity> std::fmt::Debug for Proven<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proven")
            .field("timestamp", &self.timestamp)
            .finish_non_exhaustive()
    }
}

/// One driver round-trip: send the command, await the reply.
///
/// `None` means the driver stopped under the verb — the state each staged
/// error type's `EndpointDropped` describes (ruling 62).
///
/// **Cancel-safety** is the *caller's*: dropping the returned future drops
/// the `oneshot` receiver, and each verb above keeps its staged object alive
/// across this await so the drop rejects the chain. The DH the driver may
/// already have spent is not refunded, which is unavoidable and is why §6.1
/// prices the ladder cumulatively.
async fn round_trip<I: Identity, T>(
    shell: &Shell<I>,
    command: impl FnOnce(oneshot::Sender<T>) -> Command<I>,
) -> Option<T> {
    if shell.driver_stopped() {
        return None;
    }
    let (tx, rx) = oneshot::channel();
    shell.send(command(tx));
    rx.await.ok()
}

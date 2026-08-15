//! §16.2's `Connection` handle — the subset slice 3 builds.
//!
//! `close()`, ruling 46's `closed()`, and the four accessors. Every other
//! §16.2 verb — `open_*`, `accept_*`, `send_message`, `recv_*`, `acked`,
//! `notified`, `set_persistent_keepalive` — belongs to slices 4–7 and is
//! **absent rather than stubbed**: in this module tree an unimplemented verb
//! is a claim about the protocol, and an `unimplemented!()` on a public
//! surface is a worse claim than an absence.
//!
//! # Where the work happens
//!
//! §16.3 (ruling 53) puts the connection data path on the **shared-cell**
//! side of the seam: the handle borrows the connection's
//! `Rc<RefCell<ConnCell>>`, calls the sans-io core directly — which seals
//! synchronously (§16.7) — marks the cell dirty and wakes the driver, which
//! drains `poll_output()` to `Timeout` and performs the I/O. Each verb is
//! written **once**, as `poll_*(&self, cx, …) -> Poll<_>`; the `async fn`
//! §16.2 declares is `poll_fn` over it.

use std::cell::RefCell;
use std::future::poll_fn;
use std::net::SocketAddr;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::constants;
use crate::core::ConnectionId;
use crate::error::ConnectionLost;
use crate::packet::{Channel, Handshake};

use super::shared::{ConnCell, ShellLink, WakerSlot, now};

/// The static public key type of a suite — §2.4's canonical octets.
type PublicKeyFor<S> = <<S as Channel>::Curve as hiss::curve::Curve>::PublicKey;

/// A live slither connection — one Noise session and its frame layer
/// (§16.1).
///
/// # Lifetime, and the two drop rules that are not the same rule
///
/// Dropping the **last handle to this connection** performs
/// `close(NO_ERROR, "")`: the graceful teardown of §15.2, with a CLOSE on
/// the wire.
///
/// Dropping the last handle **in the process** stops the driver and every
/// connection dies silently — **nothing is transmitted** (§15.4's
/// endpoint-dropped row).
///
/// Where the two coincide — this is the last `Connection` *and* the last
/// handle of any kind — **[RATIFIED 2026/08/15 — ruling 88]** the
/// endpoint-dropped row governs and **no CLOSE is sealed**. A synchronous
/// `Drop` cannot await the driver, and the driver is already stopping; the
/// peer's cost is bounded at `DEAD_TIMEOUT` (25 s), which that row already
/// accepts.
///
/// This is documentation obligation #4 and it is drop-order sensitive: it
/// is the opposite of the obvious guess, and which of the two rules fires
/// depends on the order your values fall out of scope. Keeping an
/// [`Endpoint`](super::Endpoint) alive across the drop is what makes the
/// CLOSE happen.
pub struct Connection<S: Handshake> {
    shell: Rc<dyn ShellLink>,
    cell: Rc<RefCell<ConnCell<S>>>,
    id: ConnectionId,
    /// Fixed for the connection's life — §6.1 proved it before the
    /// connection existed, and nothing changes it — so it is held here
    /// rather than in the cell and keeps answering after §15.2's linger has
    /// dropped the session.
    remote_static: PublicKeyFor<S>,
    /// Fixed for the same reason, by §7.8: one session per connection, and
    /// no transport state ever crosses a handshake.
    session_id: hiss::noise::SessionId,
}

impl<S: Handshake> Connection<S> {
    pub(crate) fn new(
        shell: Rc<dyn ShellLink>,
        cell: Rc<RefCell<ConnCell<S>>>,
        id: ConnectionId,
        remote_static: PublicKeyFor<S>,
        session_id: hiss::noise::SessionId,
    ) -> Self {
        shell.acquire();
        cell.borrow_mut().handles += 1;
        Self {
            shell,
            cell,
            id,
            remote_static,
            session_id,
        }
    }

    // No `id()` accessor. §16.2's `Connection` surface is a list, and in
    // this project a list is read as exhaustive whether or not it says so
    // (CLAUDE.md working rule 8) — `ConnectionId` appears in §16.4's *core*
    // API and nowhere on the handle. It would be useful for correlating
    // traces and it may well be worth a ruling, but adding a public
    // accessor is additive later and removing one is breaking, so the
    // absence is the reversible choice.

    /// Close the connection (§15.2).
    ///
    /// Resolves once the CLOSE frame is **sealed** and the closing state is
    /// entered — **not** once it has left the wire, and not once the peer
    /// has it. §15.2 then lingers for `CLOSE_LINGER` (5 s), replying at
    /// most once a second to authenticated, window-fresh inbound.
    ///
    /// `reason` is truncated at [`CLOSE_REASON_MAX`] (§8.4).
    ///
    /// A second `close()`, or one on a connection that has already died, is
    /// a no-op: a connection dies once, and §16.4 emits its `Closed` once.
    ///
    /// [`CLOSE_REASON_MAX`]: crate::constants::CLOSE_REASON_MAX
    pub async fn close(&self, code: u64, reason: &[u8]) {
        poll_fn(|cx| self.poll_close(cx, code, reason)).await
    }

    /// The one implementation of [`close`](Self::close) (§16.3, ruling 53).
    ///
    /// Always `Ready` on the first poll, and that is the specification
    /// rather than a shortcut: §16.7 makes sealing synchronous **inside the
    /// mutating call that triggers it**, and §16.2 resolves `close()` at
    /// the seal. The `cx` is unused for exactly that reason — the verb is
    /// written in the poll form because §16.3 requires every data-path verb
    /// to have one, and because slice 4's `AsyncWrite` shutdown path is the
    /// same function with its error mapped.
    fn poll_close(&self, _cx: &mut Context<'_>, code: u64, reason: &[u8]) -> Poll<()> {
        self.close_now(code, reason);
        Poll::Ready(())
    }

    /// Seal the CLOSE and mark the cell dirty. Shared by
    /// [`poll_close`](Self::poll_close) and the last-handle drop.
    fn close_now(&self, code: u64, reason: &[u8]) {
        let mutated = {
            let mut cell = self.cell.borrow_mut();
            match cell.core.as_mut() {
                Some(core) => {
                    core.close(now(), code, reason);
                    cell.dirty = true;
                    true
                }
                // The driver has already released the core — §15.2's
                // linger expired, or the connection was never installed.
                // There is nothing left to seal with.
                None => false,
            }
        };
        if mutated {
            self.shell.mark_dirty(self.id);
        }
    }

    /// Resolve when this connection ends, with the reason it ended
    /// (**ruling 46**).
    ///
    /// Every row of §15.4's teardown matrix resolves it. It is a **latched**
    /// signal, not a queue:
    ///
    /// * **cancel-safe** — a dropped future has consumed nothing and leaves
    ///   the waker map exactly as it found it;
    /// * **concurrent** — any number of tasks may await it and all of them
    ///   resolve;
    /// * **permanent** — after the death it resolves immediately, with the
    ///   same value, for ever, including for a future first awaited *after*
    ///   the death.
    ///
    /// On a healthy connection it never resolves, which is what makes it
    /// the `select!` arm of a long-running loop.
    ///
    /// A `closed()` future is **not a handle** (§16.3, ruling 62): holding
    /// one while dropping every [`Connection`] still stops the driver and
    /// still kills the session silently.
    pub async fn closed(&self) -> ConnectionLost {
        // The key is minted once, before the first poll, and released by
        // the guard's `Drop` — including on cancellation. This is the whole
        // of the verb's cancel-safety.
        let cell = Rc::clone(&self.cell);
        let key = cell.borrow_mut().closed_wakers.key();
        let slot = WakerSlot::new(key, {
            let cell = Rc::clone(&self.cell);
            move |key| cell.borrow_mut().closed_wakers.unpark(key)
        });
        poll_fn(|cx| self.poll_closed(cx, slot.key())).await
    }

    /// The one implementation of [`closed`](Self::closed) (§16.3, ruling
    /// 53).
    fn poll_closed(&self, cx: &mut Context<'_>, key: u64) -> Poll<ConnectionLost> {
        let mut cell = self.cell.borrow_mut();
        match cell.closed.clone() {
            Some(lost) => Poll::Ready(lost),
            None => {
                cell.closed_wakers.park(key, cx);
                Poll::Pending
            }
        }
    }

    /// The peer's static public key — **proven**, not claimed (§6.1).
    ///
    /// A synchronous read of the shared cell (§16.8), never a driver
    /// round-trip. It keeps answering after the connection has died, so a
    /// post-mortem handle can still say who it was talking to.
    pub fn remote_static(&self) -> PublicKeyFor<S> {
        self.remote_static.clone()
    }

    /// The address this connection's datagrams go to — §5.6's anchor.
    ///
    /// Slice 7's roaming (§7.3) is what makes this change; in this slice it
    /// is fixed for the connection's life.
    pub fn remote_address(&self) -> SocketAddr {
        self.cell.borrow().remote_address
    }

    /// hiss's channel binding for this session (**ruling 89**).
    ///
    /// This is [`hiss::noise::SessionId`], re-exported — not a slither
    /// type. hiss derives it from the handshake hash, both peers of a
    /// session produce the same value, and it is a *public*
    /// channel-binding value meant for out-of-band comparison: logging it,
    /// or a short-authentication-string check.
    ///
    /// # Its `Eq` is not constant-time
    ///
    /// By hiss's deliberate choice. It carries no secret, and it **must not
    /// be used to compare one** — reaching for it as a token or a session
    /// key comparison is the mistake this paragraph exists to prevent.
    pub fn session_id(&self) -> hiss::noise::SessionId {
        self.session_id.clone()
    }

    /// Whether a session is installed.
    ///
    /// `true` for the whole of a live connection's life, and `false` again
    /// once §15.2's linger has expired and the state is dropped — which is
    /// a real transition an application can observe on a handle it still
    /// holds, not a placeholder.
    pub fn is_established(&self) -> bool {
        self.cell.borrow().is_established()
    }
}

impl<S: Handshake> std::fmt::Debug for Connection<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written: the curve's `PublicKey` is not required to be
        // `Debug`, and a peer key is not something to print by default.
        f.debug_struct("Connection")
            .field("id", &self.id)
            .field("remote_address", &self.remote_address())
            .field("established", &self.is_established())
            .finish_non_exhaustive()
    }
}

impl<S: Handshake> Drop for Connection<S> {
    fn drop(&mut self) {
        let last_for_connection = {
            let mut cell = self.cell.borrow_mut();
            cell.handles -= 1;
            cell.handles == 0
        };

        // `release` decrements the process-wide handle count and, at zero,
        // tells the driver to stop. It must run **before** the decision
        // below, because that decision is exactly "was this also the last
        // handle in the process?" (ruling 88).
        let last_in_process = self.shell.release();

        if last_for_connection && !last_in_process {
            self.close_now(constants::NO_ERROR, b"");
        }
    }
}

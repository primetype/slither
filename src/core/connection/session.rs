//! §7 — the transport session: counters, the replay window, the liveness
//! clocks, and the two seal paths.
//!
//! Everything here is *per session*, and §7.8 makes that the same scope as
//! *per connection*: **a connection has exactly one session for its whole
//! life**. There is deliberately no re-install path, no session swap and no
//! survival matrix — the cheapest thing in this slice to get wrong by
//! helpfulness.
//!
//! # What is hiss's and what is slither's
//!
//! The counter (§7.1) and the epoch ratchet (§7.7) are **hiss's**, entirely.
//! `into_datagram_with_epoch` gives a pair whose keys ratchet on a
//! counter-derived schedule with no wire signalling, retaining the current
//! and immediately-preceding epoch keys and committing a future epoch only
//! after the tag verifies. Slither's obligation here is a **negative** one:
//! do not drive it, and **do not chase epochs** (§7.7). Nothing in this file
//! computes an epoch, and nothing recovers from a stale one — a straggler
//! more than one epoch back is an ordinary decryption failure and an
//! ordinary silent drop.
//!
//! The anti-replay window (§7.2) is **slither's**, entirely: hiss's
//! `decrypt_at` imposes neither monotonicity nor uniqueness and will open
//! one counter repeatedly, by design.
//!
//! # The ordering rule that is the whole of §7.2
//!
//! **AEAD first, then the window check, then the window mark, then
//! deliver.** A window check before `decrypt_at` would let an off-path
//! forger who observed a cleartext counter poison the window by minting a
//! packet that never authenticates.

use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

use packtool::Packet;

use crate::constants;
use crate::core::EstablishedSession;
use crate::packet::{DataHeader, Handshake};

/// Words in the §7.2 bitmap: `REPLAY_WINDOW` bits, 256 B per connection.
const REPLAY_WORDS: usize = constants::REPLAY_WINDOW / 64;

const _: () = assert!(constants::REPLAY_WINDOW.is_multiple_of(64));
const _: () = assert!(REPLAY_WORDS == 32);

/// §7.2's anti-replay window: a greatest authenticated counter **plus** a
/// `REPLAY_WINDOW`-bit sliding bitmap.
///
/// # The indexing, and why it is offset by one
///
/// §7.2 describes "a greatest authenticated counter **plus** the 2048-bit
/// bitmap", and drops "a duplicate, or a counter **more than** 2048 behind
/// the greatest". Those two sentences fix the layout between them: the
/// greatest is held in its own field, so the bitmap's 2048 bits are free to
/// cover offsets **1..=2048**. Bit `i` therefore records
/// `greatest − (i + 1)`.
///
/// The consequence is the boundary: `greatest − 2048` is accepted (it is
/// exactly 2048 behind, not *more than*), and `greatest − 2049` is dropped.
/// An implementation that puts the greatest at bit 0 holds one counter
/// fewer and rejects `greatest − 2048`.
///
/// # This is also the ACK record
///
/// §7.2, ratified: *"The ACK record stays **fused** to this window (§12.2):
/// the window is the single received-packet record — reuse, don't
/// duplicate."* [`greatest`](ReplayWindow::greatest) and
/// [`ranges_desc`](ReplayWindow::ranges_desc) are what slice 5 derives an
/// ACK from; they exist now, with tests, so that the fusion rule cannot be
/// broken later by an accessor that was never provided.
#[derive(Debug, Clone)]
pub(crate) struct ReplayWindow {
    greatest: Option<u64>,
    bits: [u64; REPLAY_WORDS],
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayWindow {
    /// An empty window: nothing received, no greatest.
    pub(crate) const fn new() -> Self {
        Self {
            greatest: None,
            bits: [0; REPLAY_WORDS],
        }
    }

    /// The greatest authenticated counter, or `None` if nothing has been
    /// received. §12.2's ACK derivation starts here.
    pub(crate) fn greatest(&self) -> Option<u64> {
        self.greatest
    }

    /// Whether `counter` would be accepted — **without** marking it.
    ///
    /// Not on the receive path: §7.2's rule is check-**and**-mark in one
    /// step after the AEAD. This exists so a test can separate the two.
    pub(crate) fn would_accept(&self, counter: u64) -> bool {
        match self.greatest {
            None => true,
            Some(greatest) => match greatest.checked_sub(counter) {
                // Ahead of the greatest: always fresh.
                None => true,
                Some(0) => false,
                Some(offset) if offset > constants::REPLAY_WINDOW as u64 => false,
                Some(offset) => !self.bit(offset - 1),
            },
        }
    }

    /// §7.2's post-AEAD check-then-mark. `true` means **fresh**: deliver it,
    /// and let it drive liveness and roaming.
    ///
    /// Must be called only after `decrypt_at` has authenticated the packet.
    pub(crate) fn check_and_mark(&mut self, counter: u64) -> bool {
        match self.greatest {
            None => {
                self.greatest = Some(counter);
                true
            }
            Some(greatest) if counter > greatest => {
                let delta = counter - greatest;
                self.shift(delta);
                // The old greatest moves to offset `delta`, i.e. bit
                // `delta - 1`. Beyond the window it simply falls off.
                self.set_bit(delta - 1);
                self.greatest = Some(counter);
                true
            }
            Some(greatest) => {
                let offset = greatest - counter;
                if offset == 0 || offset > constants::REPLAY_WINDOW as u64 {
                    return false;
                }
                let index = offset - 1;
                if self.bit(index) {
                    false
                } else {
                    self.set_bit(index);
                    true
                }
            }
        }
    }

    /// The received counters as ranges, **newest-first and descending** —
    /// §12.2's construction order.
    ///
    /// The `MAX_ACK_RANGES` and packet-capacity truncation sit on top of
    /// this, in slice 5; this is the raw derivation and nothing consumes it
    /// yet.
    pub(crate) fn ranges_desc(&self) -> impl Iterator<Item = RangeInclusive<u64>> + '_ {
        RangesDesc {
            window: self,
            offset: 0,
        }
    }

    /// Whether the counter at `offset` below the greatest was received.
    /// Offset 0 is the greatest itself, which is received by definition.
    fn received_at(&self, offset: u64) -> bool {
        offset == 0 || self.bit(offset - 1)
    }

    fn bit(&self, index: u64) -> bool {
        if index >= constants::REPLAY_WINDOW as u64 {
            return false;
        }
        let index = index as usize;
        self.bits[index / 64] & (1u64 << (index % 64)) != 0
    }

    fn set_bit(&mut self, index: u64) {
        if index >= constants::REPLAY_WINDOW as u64 {
            return;
        }
        let index = index as usize;
        self.bits[index / 64] |= 1u64 << (index % 64);
    }

    /// Slide the bitmap `delta` positions older, dropping what falls out.
    fn shift(&mut self, delta: u64) {
        if delta >= constants::REPLAY_WINDOW as u64 {
            self.bits = [0; REPLAY_WORDS];
            return;
        }
        let delta = delta as usize;
        let words = delta / 64;
        let bits = delta % 64;

        let mut out = [0u64; REPLAY_WORDS];
        for i in (words..REPLAY_WORDS).rev() {
            let src = i - words;
            let mut value = self.bits[src] << bits;
            if bits > 0 && src > 0 {
                value |= self.bits[src - 1] >> (64 - bits);
            }
            out[i] = value;
        }
        self.bits = out;
    }
}

struct RangesDesc<'a> {
    window: &'a ReplayWindow,
    offset: u64,
}

impl Iterator for RangesDesc<'_> {
    type Item = RangeInclusive<u64>;

    fn next(&mut self) -> Option<RangeInclusive<u64>> {
        let greatest = self.window.greatest?;
        // The walk stops at the window's edge, and earlier if the
        // connection has not yet seen `REPLAY_WINDOW` counters at all.
        let last = (constants::REPLAY_WINDOW as u64).min(greatest);

        while self.offset <= last && !self.window.received_at(self.offset) {
            self.offset += 1;
        }
        if self.offset > last {
            return None;
        }
        let start = self.offset;
        while self.offset <= last && self.window.received_at(self.offset) {
            self.offset += 1;
        }
        let end = self.offset - 1;
        Some((greatest - end)..=(greatest - start))
    }
}

/// §7.4's two clocks.
///
/// *"The connection is dead when `now − last_authenticated_recv >= D_eff`
/// **and** at least one **arming** send has occurred since that last
/// authenticated receive."*
///
/// The death deadline is **derived** from `last_authenticated_recv` rather
/// than stored, which is what makes §7.4's "is **not** re-armed by
/// subsequent sends" true by construction rather than by discipline: no
/// send moves the deadline, it only flips the arming flag, so "the send
/// clock never defers death, it only enables it" cannot be implemented
/// wrongly here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Liveness {
    last_authenticated_recv: Instant,
    last_send: Instant,
    armed: bool,
    /// **[RATIFIED 2026/08/16 — ruling 195]** §7.5's passive predicate, as
    /// **state rather than arithmetic**.
    ///
    /// §7.5 states the rule two ways in one sentence: the prose is *"a side
    /// that **has received since it last sent**"* — a flag — and the formal
    /// rule is `R > S` — a comparison. They agree everywhere except when
    /// the two instants are **equal**, and equal is reachable: the shell
    /// driver caches `now()` **once per turn**, so a receive and a send
    /// handled in one turn share an `Instant`. Under `start_paused` nothing
    /// advances between install and a first exchange, so it happens every
    /// time — leaving `R == S`, no keepalive (the comparison is false) and
    /// no death (`armed` is false): SECV5-2's **immortal half-open
    /// session**, which ruling 39's install pin exists to prevent.
    ///
    /// `R >= S` is **not** the repair: at install `R == S` deliberately, so
    /// it would make an idle-from-install connection keepalive at once and
    /// destroy the 25 s reap. The comparison cannot separate *"nothing
    /// received since install"* from *"received at the same instant as
    /// install"* — it is the wrong instrument, and the prose named the
    /// right one.
    received_since_marking_send: bool,
}

impl Liveness {
    /// §7.4's install pin: both clocks at the install instant, and the
    /// death deadline **already armed**.
    ///
    /// *"the handshake is the arming event, so the rule's second conjunct
    /// holds from install onward and no subsequent send is needed to enable
    /// it."* This is what makes "a half-open session is reaped by liveness"
    /// a fact rather than an implementation choice — without it a session
    /// that receives nothing would be held **forever**, since §7.6 is
    /// deleted and liveness is the only reaper.
    pub(crate) fn pinned_at_install(now: Instant) -> Self {
        Self {
            last_authenticated_recv: now,
            last_send: now,
            armed: true,
            // Ruling 39: the dance must **not** bootstrap from the install
            // alone — a connection that carries nothing emits nothing and
            // is reaped at its effective dead timeout.
            received_since_marking_send: false,
        }
    }

    /// The death deadline, if armed.
    ///
    /// `last_authenticated_recv + dead_timeout`, fired at the deadline
    /// rather than strictly after it. See §7.4 and
    /// `.slices/03-skeleton/IMPLEMENTATION.md` F2 for the boundary wording
    /// this reads against.
    pub(crate) fn deadline(&self, dead_timeout: Duration) -> Option<Instant> {
        self.armed
            .then(|| self.last_authenticated_recv.checked_add(dead_timeout))
            .flatten()
    }

    /// Record a send. `marking` is §7.4's `seal`; `ack_eliciting` is
    /// §8.7's per-packet property.
    ///
    /// *"A send **arms** the death deadline if **either** it is a marking
    /// send … **or** it carries any ack-eliciting frame, whether or not it
    /// marks; the two triggers are independent and either alone
    /// suffices."*
    fn on_send(&mut self, now: Instant, marking: bool, ack_eliciting: bool) {
        if marking {
            self.last_send = now;
            // Ruling 195: a marking send is what the passive keepalive
            // would have been sent to do, so it clears the debt — whether
            // or not the clock has advanced since the receive that set it.
            self.received_since_marking_send = false;
        }
        if marking || ack_eliciting {
            self.armed = true;
        }
    }

    /// §7.2's authenticated, window-fresh receive: the anchor moves and the
    /// deadline is disarmed until the next arming send.
    fn on_authenticated_fresh_recv(&mut self, now: Instant) {
        self.last_authenticated_recv = now;
        self.armed = false;
        // Ruling 195: §7.5's *"has received since it last sent"*, recorded
        // as the fact it is. This is the whole of the passive predicate.
        self.received_since_marking_send = true;
    }

    /// §7.5's `last_send` — the marking clock. Read by slice 7's keepalive;
    /// exposed now because the two seal paths are otherwise
    /// indistinguishable, which is exactly the degenerate implementation
    /// (`seal_quiet` as an alias for `seal`) a slice-3 test must separate.
    pub(crate) fn last_send(&self) -> Instant {
        self.last_send
    }

    /// The receive anchor the death deadline hangs from.
    pub(crate) fn last_authenticated_recv(&self) -> Instant {
        self.last_authenticated_recv
    }

    /// **[RATIFIED 2026/08/16 — ruling 195]** §7.5's passive predicate:
    /// *"a side that has received since it last sent"*.
    ///
    /// **Use this, never `last_authenticated_recv() > last_send()`.** The
    /// comparison is the same predicate everywhere the two instants differ
    /// and is **wrong when they are equal**, which the driver's once-per-turn
    /// `now()` makes reachable — see the field's own note. A keepalive owed
    /// here fires at `last_send() + K_eff`.
    pub(crate) fn owes_passive_keepalive(&self) -> bool {
        self.received_since_marking_send
    }

    /// Whether the death deadline is armed.
    pub(crate) fn is_armed(&self) -> bool {
        self.armed
    }
}

/// A sealed Data packet: §16.7's `(counter, bytes)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sealed {
    /// The counter the packet was sealed under — the packet number (§7.1).
    pub(crate) counter: u64,
    /// `header ‖ ciphertext ‖ tag`, ready for the wire.
    pub(crate) datagram: Vec<u8>,
}

/// A seal that did not happen. §7.9 — **connection death, no rekey escape**.
///
/// One variant on purpose: hiss distinguishes `NonceOverflow` from
/// `MessageTooLong` and `OutputBufferTooSmall`, but the latter two are *our*
/// bugs (an oversized frame plan, an undersized output buffer) and §18.1's
/// closed taxonomy has no variant for them. They are `debug_assert`ed at the
/// call site and otherwise take the same terminal path, because a transport
/// that panics on a send is worse than one that dies loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SealFailed;

/// The installed transport session: hiss's seal/open pair, §7.2's window
/// and §7.4's clocks.
pub(crate) struct Session<C: Handshake> {
    established: EstablishedSession<C>,
    replay: ReplayWindow,
    liveness: Liveness,
    /// T7's injection seam: nonce exhaustion is ~1.8 × 10¹⁹ seals away and
    /// hiss's `set_counter_for_test` is internal to hiss, so §7.9's path is
    /// unreachable without one.
    #[cfg(test)]
    fail_next_seal: bool,
}

impl<C: Handshake> Session<C> {
    /// Install a completed session, pinning §7.4's clocks (§5.4, §7.8).
    pub(crate) fn install(now: Instant, established: EstablishedSession<C>) -> Self {
        Self {
            established,
            replay: ReplayWindow::new(),
            liveness: Liveness::pinned_at_install(now),
            #[cfg(test)]
            fail_next_seal: false,
        }
    }

    /// The completed session §16.4's `Install` carried.
    pub(crate) fn established(&self) -> &EstablishedSession<C> {
        &self.established
    }

    /// §7.3's roam: move §5.6's anchor, returning the address it left.
    ///
    /// Deliberately **narrow** rather than a blanket `established_mut()`.
    /// The anchor is the only field of an established session that ever
    /// moves — the cipher states, both indices and §7.7's epoch are fixed
    /// for the session's life (§7.8) — and a `&mut` to the whole struct
    /// would be a second way to reach them.
    ///
    /// The caller owns the predicate: this performs no check at all, because
    /// §7.3's four conjuncts and §15.2's lifecycle carve-out are stated
    /// where the packet is, not where the field is.
    pub(crate) fn roam_to(&mut self, addr: SocketAddr) -> SocketAddr {
        std::mem::replace(&mut self.established.anchor, addr)
    }

    /// §7.2's window — also §12.2's received-packet record.
    pub(crate) fn replay(&self) -> &ReplayWindow {
        &self.replay
    }

    /// §7.4's clocks.
    pub(crate) fn liveness(&self) -> &Liveness {
        &self.liveness
    }

    /// The counter the next successful seal will use (§7.1).
    ///
    /// Reading it after a mutating call is how a test tells a synchronous
    /// seal (§16.7) from one deferred into `poll_output()`.
    pub(crate) fn next_counter(&self) -> u64 {
        C::next_counter(&self.established.seal)
    }

    /// §7.4's **marking** seal: fresh application intent, or the keepalive.
    ///
    /// Unused in slice 3a — every seal this slice performs is a CLOSE,
    /// which §7.4 puts in the quiet set. It exists because the two paths
    /// are one function apart and a slice that shipped only the quiet one
    /// would leave slice 4 to invent the marking rule from prose.
    pub(crate) fn seal(
        &mut self,
        now: Instant,
        plaintext: &[u8],
        ack_eliciting: bool,
    ) -> Result<Sealed, SealFailed> {
        self.seal_inner(now, plaintext, true, ack_eliciting)
    }

    /// §7.4's **quiet** seal: pure ACKs, PTO probes, retransmissions, the
    /// credit frames, RESET_STREAM, and CLOSE.
    ///
    /// Identical on the wire — same sealed Data packet, same counter
    /// increment — and does not touch `last_send`.
    pub(crate) fn seal_quiet(
        &mut self,
        now: Instant,
        plaintext: &[u8],
        ack_eliciting: bool,
    ) -> Result<Sealed, SealFailed> {
        self.seal_inner(now, plaintext, false, ack_eliciting)
    }

    fn seal_inner(
        &mut self,
        now: Instant,
        plaintext: &[u8],
        marking: bool,
        ack_eliciting: bool,
    ) -> Result<Sealed, SealFailed> {
        debug_assert!(
            plaintext.len() <= constants::MAX_PLAINTEXT,
            "§8.6: a slither seal is at most MAX_PLAINTEXT + AEAD_TAG_LEN"
        );

        // ── plan ────────────────────────────────────────────────────────
        // §3.4: the header is the AEAD associated data, so it must be
        // built before the seal — which is exactly what `next_counter()`
        // exists for (Appendix A.2). Nothing here mutates hiss state.
        let counter = self.next_counter();
        if counter == u64::MAX {
            // §7.9, proactively: `u64::MAX` is reserved for `Rekey()` and
            // will never be used, so the session is already exhausted.
            return Err(SealFailed);
        }
        #[cfg(test)]
        if self.fail_next_seal {
            self.fail_next_seal = false;
            return Err(SealFailed);
        }

        let header = Packet::pack(&DataHeader::new(self.established.peer_index, counter));
        let header: &[u8] = header.as_ref();

        let mut datagram = vec![0u8; header.len() + plaintext.len() + constants::AEAD_TAG_LEN];
        datagram[..header.len()].copy_from_slice(header);

        // ── seal ────────────────────────────────────────────────────────
        let (sealed_at, written) = match C::seal(
            &mut self.established.seal,
            header,
            plaintext,
            &mut datagram[header.len()..],
        ) {
            Ok(out) => out,
            Err(_error) => {
                // hiss guarantees the counter did not advance and nothing
                // was written, which is §16.7's "on seal failure nothing
                // moved" for free.
                debug_assert_eq!(
                    self.next_counter(),
                    counter,
                    "a failed seal must leave the counter unchanged"
                );
                return Err(SealFailed);
            }
        };
        debug_assert_eq!(
            sealed_at, counter,
            "§3.4: the counter in the AD must be the one the seal used"
        );
        datagram.truncate(header.len() + written);

        // ── commit ──────────────────────────────────────────────────────
        // Only reached on seal success. In this slice the whole commit is
        // §7.4's clock update; slices 4–6 add the dequeue, the pending-ACK
        // clear, `on_sent` and the recovery timers **here**.
        self.liveness.on_send(now, marking, ack_eliciting);

        Ok(Sealed {
            counter: sealed_at,
            datagram,
        })
    }

    /// Open a received Data packet, §7.2's ordering exactly.
    ///
    /// `Some(plaintext)` means **authenticated and window-fresh**: deliver
    /// it, and let it drive liveness. `None` is a silent drop — a forgery, a
    /// straggler more than one epoch back, a duplicate, or a counter beyond
    /// the window's tail. §7.2 gives all of those one behaviour, and this
    /// function's caller cannot tell them apart, deliberately.
    ///
    /// The plaintext lands in `scratch`, which the caller owns: the frame
    /// parser borrows it while the session is free to be borrowed again.
    pub(crate) fn open<'a>(
        &mut self,
        now: Instant,
        counter: u64,
        ad: &[u8],
        ciphertext: &[u8],
        scratch: &'a mut Vec<u8>,
    ) -> Option<&'a [u8]> {
        let plaintext_len = ciphertext.len().checked_sub(constants::AEAD_TAG_LEN)?;
        if plaintext_len > constants::MAX_PLAINTEXT {
            // §3.5's cap. §3.1's gate already bounds the datagram, so this
            // is unreachable through the endpoint; it is here because
            // `handle_datagram` takes bytes, not a gate result.
            return None;
        }

        scratch.clear();
        scratch.resize(plaintext_len, 0);

        // AEAD first. On failure `scratch` holds unauthenticated bytes,
        // which is why nothing below reads it on that path.
        let written = C::open(
            &mut self.established.open,
            counter,
            ad,
            ciphertext,
            scratch.as_mut_slice(),
        )
        .ok()?;

        // Only now: the window check, and the mark.
        if !self.replay.check_and_mark(counter) {
            // §18.2's `slither::replay` carries exactly this: "replay-window
            // rejections". It is the one post-AEAD drop with an operator
            // target — §3.1's pre-AEAD gate is the crate's silent tier and
            // an AEAD failure is named by no target at all, because a
            // forgery and a straggler more than one epoch back are
            // indistinguishable at the hiss surface (§7.7).
            //
            // `debug`, not `warn`: ordinary reordering past the window's
            // tail reaches here on a healthy path, and §18.2 fixes the
            // target rather than the level.
            tracing::debug!(
                target: "slither::replay",
                counter,
                greatest = ?self.replay.greatest(),
                "a received packet was rejected by the replay window"
            );
            return None;
        }

        // And only now does liveness advance — §7.2: "No replayed packet
        // ever moves the endpoint or refreshes liveness."
        self.liveness.on_authenticated_fresh_recv(now);

        scratch.truncate(written);
        Some(&scratch[..written])
    }

    /// T7's seal-failure injection: the next seal fails as if the counter
    /// were exhausted (§7.9).
    #[cfg(test)]
    pub(crate) fn fail_next_seal(&mut self) {
        self.fail_next_seal = true;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    // ── §7.2, the window ────────────────────────────────────────────────

    #[test]
    fn a_fresh_window_accepts_anything_once() {
        let mut w = ReplayWindow::new();
        assert_eq!(w.greatest(), None);
        assert!(w.check_and_mark(7));
        assert_eq!(w.greatest(), Some(7));
        assert!(!w.check_and_mark(7), "a duplicate is not fresh");
    }

    #[test]
    fn out_of_order_within_the_window_is_fresh_once_each() {
        let mut w = ReplayWindow::new();
        assert!(w.check_and_mark(9));
        assert!(w.check_and_mark(7), "opening 7 after 9 is fine");
        assert!(!w.check_and_mark(7));
        assert!(w.check_and_mark(8));
        assert_eq!(
            w.greatest(),
            Some(9),
            "an older counter is not the greatest"
        );
    }

    /// §7.2's lower edge, from **both** sides. Testing only the rejected
    /// side leaves the window collapsible to a much smaller one with
    /// nothing red.
    #[test]
    fn the_window_edge_is_two_sided() {
        let window = constants::REPLAY_WINDOW as u64;
        let mut w = ReplayWindow::new();
        assert!(w.check_and_mark(10_000));

        assert!(
            w.would_accept(10_000 - window),
            "exactly REPLAY_WINDOW behind is not *more than* REPLAY_WINDOW behind"
        );
        assert!(w.check_and_mark(10_000 - window));
        assert!(
            !w.would_accept(10_000 - window - 1),
            "one further back is dropped"
        );
        assert!(!w.check_and_mark(10_000 - window - 1));
    }

    #[test]
    fn advancing_the_greatest_slides_the_window() {
        let window = constants::REPLAY_WINDOW as u64;
        let mut w = ReplayWindow::new();
        assert!(w.check_and_mark(1));
        // Jump forward so counter 1 falls exactly on the tail…
        assert!(w.check_and_mark(1 + window));
        assert!(!w.check_and_mark(1), "still remembered at the tail");
        // …and one further, so it falls off entirely. It is then refused
        // for being too old, not for being remembered.
        assert!(w.check_and_mark(2 + window));
        assert!(!w.check_and_mark(1));
    }

    #[test]
    fn a_jump_past_the_whole_window_clears_it() {
        let mut w = ReplayWindow::new();
        for c in 0..100 {
            assert!(w.check_and_mark(c));
        }
        assert!(w.check_and_mark(1_000_000));
        // Everything old is now beyond the tail.
        assert!(!w.would_accept(99));
        // And a counter just behind the new greatest is fresh again.
        assert!(w.check_and_mark(999_999));
    }

    /// The old greatest is remembered after an advance — the one bit the
    /// offset-by-one indexing is most likely to lose.
    #[test]
    fn the_previous_greatest_is_remembered_after_an_advance() {
        for delta in [1u64, 2, 63, 64, 65, 127, 128, 2047, 2048] {
            let mut w = ReplayWindow::new();
            assert!(w.check_and_mark(5000));
            assert!(w.check_and_mark(5000 + delta));
            assert!(
                !w.check_and_mark(5000),
                "counter 5000 must still be marked after a {delta}-step advance"
            );
        }
    }

    // ── §12.2's derivation input ────────────────────────────────────────

    #[test]
    fn ranges_desc_is_empty_before_anything_is_received() {
        let w = ReplayWindow::new();
        assert_eq!(w.ranges_desc().collect::<Vec<_>>(), Vec::new());
    }

    #[test]
    fn ranges_desc_is_newest_first_and_descending() {
        let mut w = ReplayWindow::new();
        for c in [10u64, 11, 12, 15, 16, 20] {
            assert!(w.check_and_mark(c));
        }
        assert_eq!(
            w.ranges_desc().collect::<Vec<_>>(),
            vec![20..=20, 15..=16, 10..=12]
        );
    }

    #[test]
    fn ranges_desc_stops_at_counter_zero() {
        let mut w = ReplayWindow::new();
        for c in [0u64, 1, 2] {
            assert!(w.check_and_mark(c));
        }
        assert_eq!(w.ranges_desc().collect::<Vec<_>>(), vec![0..=2]);
    }

    /// The alternating worst case §12.2 names: it is what forces the
    /// newest-first truncation, so the derivation input must produce it.
    #[test]
    fn ranges_desc_handles_the_alternating_worst_case() {
        let mut w = ReplayWindow::new();
        for c in (0u64..64).step_by(2) {
            assert!(w.check_and_mark(c));
        }
        let ranges: Vec<_> = w.ranges_desc().collect();
        assert_eq!(ranges.len(), 32);
        assert_eq!(ranges[0], 62..=62);
        assert_eq!(ranges[31], 0..=0);
    }

    // ── §7.4, the clocks ────────────────────────────────────────────────

    #[test]
    fn install_pins_both_clocks_and_arms_the_deadline() {
        let now = Instant::now();
        let liveness = Liveness::pinned_at_install(now);

        assert_eq!(liveness.last_send(), now);
        assert_eq!(liveness.last_authenticated_recv(), now);
        assert!(liveness.is_armed(), "§7.4: pinned *armed*");
        assert_eq!(
            liveness.deadline(constants::DEAD_TIMEOUT),
            Some(now + constants::DEAD_TIMEOUT),
            "a session that receives nothing dies at install + DEAD_TIMEOUT"
        );
    }

    #[test]
    fn a_quiet_send_does_not_move_last_send() {
        let now = Instant::now();
        let mut liveness = Liveness::pinned_at_install(now);
        let later = now + Duration::from_secs(3);

        liveness.on_send(later, false, false);
        assert_eq!(
            liveness.last_send(),
            now,
            "seal_quiet must not touch last_send"
        );

        liveness.on_send(later, true, false);
        assert_eq!(liveness.last_send(), later, "seal marks it");
    }

    /// The two arming triggers are independent: a quiet **ack-eliciting**
    /// send arms without marking, and a marking send arms without being
    /// ack-eliciting.
    #[test]
    fn arming_and_marking_are_independent_axes() {
        let now = Instant::now();

        let mut a = Liveness::pinned_at_install(now);
        a.on_authenticated_fresh_recv(now);
        assert!(!a.is_armed());
        a.on_send(now, false, true);
        assert!(a.is_armed(), "an ack-eliciting quiet send arms");
        assert_eq!(a.last_send(), now, "…and still does not mark");

        let mut b = Liveness::pinned_at_install(now);
        b.on_authenticated_fresh_recv(now);
        b.on_send(now, true, false);
        assert!(b.is_armed(), "a marking send arms");

        let mut c = Liveness::pinned_at_install(now);
        c.on_authenticated_fresh_recv(now);
        c.on_send(now, false, false);
        assert!(!c.is_armed(), "a quiet, non-eliciting send does neither");
    }

    /// "The send clock never defers death, it only enables it": later sends
    /// do not move the deadline.
    #[test]
    fn later_sends_do_not_re_arm_or_defer_the_deadline() {
        let now = Instant::now();
        let mut liveness = Liveness::pinned_at_install(now);
        let deadline = liveness.deadline(constants::DEAD_TIMEOUT);

        for step in 1..10 {
            liveness.on_send(now + Duration::from_secs(step), true, true);
            assert_eq!(
                liveness.deadline(constants::DEAD_TIMEOUT),
                deadline,
                "the deadline hangs from the receive clock, not the send clock"
            );
        }
    }

    #[test]
    fn an_authenticated_fresh_receive_disarms_and_re_anchors() {
        let now = Instant::now();
        let mut liveness = Liveness::pinned_at_install(now);
        let recv = now + Duration::from_secs(4);

        liveness.on_authenticated_fresh_recv(recv);
        assert!(!liveness.is_armed());
        assert_eq!(liveness.deadline(constants::DEAD_TIMEOUT), None);

        liveness.on_send(recv, false, true);
        assert_eq!(
            liveness.deadline(constants::DEAD_TIMEOUT),
            Some(recv + constants::DEAD_TIMEOUT)
        );
    }
}

#[cfg(test)]
mod transport_tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use super::*;
    use crate::identity::Identity;
    use crate::packet::{Inbound, ReferenceSuite, classify};
    use crate::testutil::CountingIdentity;

    type Suite = ReferenceSuite;
    type Id = CountingIdentity<Suite>;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    /// A real IK handshake, run inline, split into two established
    /// sessions. Nothing here is a story test — it exists so the seal/open
    /// plumbing below has genuine hiss cipher states to run against.
    fn established_pair() -> (Session<Suite>, Session<Suite>) {
        let initiator = Id::seeded([7u8; 32]);
        let responder = Id::seeded([9u8; 32]);
        let now = Instant::now();

        let (ip, ik) = initiator.open().expect("software identity opens");
        let state =
            <Suite as Handshake>::initiator(ip, constants::PROLOGUE, *responder.public_static());
        let (msg1, state) =
            <Suite as Handshake>::write_msg1(state, ik, &(), &[0u8; constants::MSG1_PAYLOAD_LEN])
                .expect("msg1");

        let (rp, rk) = responder.open().expect("software identity opens");
        let r = <Suite as Handshake>::responder(rp, constants::PROLOGUE, rk).expect("responder");
        let (_claimed, mid) = <Suite as Handshake>::read_msg1_intro(r, &msg1).expect("intro");
        let (_payload, read) = <Suite as Handshake>::complete(mid, &()).expect("complete");
        let (msg2, r_transport) = <Suite as Handshake>::write_msg2(read).expect("msg2");
        let i_transport = <Suite as Handshake>::read_msg2(state, &msg2).expect("read msg2");

        let epoch = crate::config::Config::DEFAULT_EPOCH_SIZE;
        let (i_seal, i_open) = <Suite as Handshake>::into_datagram(i_transport, epoch);
        let (r_seal, r_open) = <Suite as Handshake>::into_datagram(r_transport, epoch);

        let i = EstablishedSession::<Suite> {
            seal: i_seal,
            open: i_open,
            our_index: 11,
            peer_index: 22,
            anchor: addr(2222),
        };
        let r = EstablishedSession::<Suite> {
            seal: r_seal,
            open: r_open,
            our_index: 22,
            peer_index: 11,
            anchor: addr(1111),
        };
        (Session::install(now, i), Session::install(now, r))
    }

    /// Hand a sealed datagram to the peer, through §3.1's gate — the same
    /// path `handle_datagram` takes, so the header the seal built is the
    /// header the AEAD is fed.
    fn deliver<'a>(
        to: &mut Session<Suite>,
        now: Instant,
        datagram: &[u8],
        scratch: &'a mut Vec<u8>,
    ) -> Option<&'a [u8]> {
        let Some(Inbound::Data {
            header,
            ad,
            ciphertext,
        }) = classify::<Suite>(datagram)
        else {
            return None;
        };
        to.open(now, header.counter, ad, ciphertext, scratch)
    }

    /// The seal builds the header **before** the seal and passes it as the
    /// AD verbatim (§3.4), the counter it carries is the one hiss used
    /// (§7.1), and the peer's gate can route and open it.
    #[test]
    fn a_sealed_packet_opens_at_the_peer() {
        let (mut i, mut r) = established_pair();
        let now = Instant::now();

        assert_eq!(i.next_counter(), 0, "§7.1: the counter runs from 0");
        let sealed = i.seal_quiet(now, b"hello frames", false).expect("seal");
        assert_eq!(sealed.counter, 0);
        assert_eq!(i.next_counter(), 1, "§16.7: the seal is synchronous");

        // The datagram is header ‖ ciphertext ‖ tag, and routes by the
        // **peer's** index.
        assert_eq!(
            sealed.datagram.len(),
            constants::DATA_HEADER_LEN + 12 + constants::AEAD_TAG_LEN
        );
        assert_eq!(sealed.datagram[0], constants::PKT_DATA);
        assert_eq!(&sealed.datagram[2..6], &22u32.to_le_bytes());
        assert_eq!(&sealed.datagram[6..14], &0u64.to_le_bytes());

        let mut scratch = Vec::new();
        assert_eq!(
            deliver(&mut r, now, &sealed.datagram, &mut scratch),
            Some(&b"hello frames"[..])
        );
    }

    /// §7.2's post-AEAD rule, from the side that separates it: a **forged**
    /// packet at counter `c` must not burn `c`, so the genuine packet at
    /// `c` still arrives.
    #[test]
    fn a_forgery_at_a_counter_does_not_burn_it() {
        let (mut i, mut r) = established_pair();
        let now = Instant::now();

        let sealed = i.seal_quiet(now, b"genuine", false).expect("seal");
        let mut forged = sealed.datagram.clone();
        let last = forged.len() - 1;
        forged[last] ^= 0xff;

        let mut scratch = Vec::new();
        assert_eq!(
            deliver(&mut r, now, &forged, &mut scratch),
            None,
            "a bad tag is dropped"
        );
        assert_eq!(r.replay().greatest(), None, "and marks nothing");
        assert_eq!(
            deliver(&mut r, now, &sealed.datagram, &mut scratch),
            Some(&b"genuine"[..]),
            "the genuine packet at the same counter still arrives"
        );
    }

    /// A replay of a genuine packet is dropped **after** decryption,
    /// without delivery — and does not refresh liveness (§7.2).
    #[test]
    fn a_replay_is_dropped_and_does_not_refresh_liveness() {
        let (mut i, mut r) = established_pair();
        let now = Instant::now();

        let sealed = i.seal_quiet(now, b"once", false).expect("seal");
        let mut scratch = Vec::new();

        let first = now + std::time::Duration::from_secs(1);
        assert!(deliver(&mut r, first, &sealed.datagram, &mut scratch).is_some());
        assert_eq!(r.liveness().last_authenticated_recv(), first);

        let later = now + std::time::Duration::from_secs(9);
        assert_eq!(deliver(&mut r, later, &sealed.datagram, &mut scratch), None);
        assert_eq!(
            r.liveness().last_authenticated_recv(),
            first,
            "§7.2: no replayed packet ever refreshes liveness"
        );
    }

    /// §3.4's keepalive shape: an empty plaintext seals to a 30-byte
    /// datagram and opens to nothing.
    #[test]
    fn an_empty_plaintext_is_a_thirty_byte_datagram() {
        let (mut i, mut r) = established_pair();
        let now = Instant::now();

        let sealed = i.seal(now, b"", false).expect("seal");
        assert_eq!(sealed.datagram.len(), 30);

        let mut scratch = Vec::new();
        assert_eq!(
            deliver(&mut r, now, &sealed.datagram, &mut scratch),
            Some(&[][..])
        );
    }

    /// Out-of-order delivery opens: hiss imposes no monotonicity and §7.2's
    /// window admits anything inside it, once.
    #[test]
    fn packets_open_out_of_order_once_each() {
        let (mut i, mut r) = established_pair();
        let now = Instant::now();

        let a = i.seal_quiet(now, b"a", false).expect("seal");
        let b = i.seal_quiet(now, b"b", false).expect("seal");
        let c = i.seal_quiet(now, b"c", false).expect("seal");
        assert_eq!((a.counter, b.counter, c.counter), (0, 1, 2));

        let mut scratch = Vec::new();
        assert_eq!(
            deliver(&mut r, now, &c.datagram, &mut scratch),
            Some(&b"c"[..])
        );
        assert_eq!(
            deliver(&mut r, now, &a.datagram, &mut scratch),
            Some(&b"a"[..])
        );
        assert_eq!(
            deliver(&mut r, now, &b.datagram, &mut scratch),
            Some(&b"b"[..])
        );
        assert_eq!(r.replay().greatest(), Some(2));
        assert_eq!(r.replay().ranges_desc().collect::<Vec<_>>(), vec![0..=2]);
    }

    /// §7.9's injection seam: the seal fails, **nothing is written, and
    /// nothing moved** — the counter is where it was.
    #[test]
    fn an_injected_seal_failure_moves_nothing() {
        let (mut i, _r) = established_pair();
        let now = Instant::now();

        let before = i.next_counter();
        let last_send = i.liveness().last_send();
        i.fail_next_seal();

        assert_eq!(
            i.seal(now + std::time::Duration::from_secs(1), b"x", true),
            Err(SealFailed)
        );
        assert_eq!(
            i.next_counter(),
            before,
            "§16.7: on seal failure nothing moved"
        );
        assert_eq!(
            i.liveness().last_send(),
            last_send,
            "and the liveness commit did not run either"
        );
    }

    /// The two seal paths are identical on the wire and differ only in
    /// `last_send` — the assertion that separates `seal_quiet` from an
    /// alias for `seal`. Slice 4 owes the other half: a marking seal of a
    /// first-transmission STREAM frame **advances** it.
    #[test]
    fn seal_quiet_does_not_mark_and_seal_does() {
        let (mut i, _r) = established_pair();
        let install = i.liveness().last_send();
        let later = install + std::time::Duration::from_secs(3);

        let quiet = i.seal_quiet(later, b"quiet", false).expect("seal");
        assert_eq!(i.liveness().last_send(), install);

        let marking = i.seal(later, b"quiet", false).expect("seal");
        assert_eq!(i.liveness().last_send(), later);

        // Identical on the wire: same shape, consecutive counters.
        assert_eq!(quiet.datagram.len(), marking.datagram.len());
        assert_eq!(marking.counter, quiet.counter + 1);
    }
}

//! §6.1's IK ladder, abstracted over the suite — the seam [`Channel`]
//! deliberately does not carry.
//!
//! [`Channel`]'s own rustdoc says why it holds no handshake surface:
//! *"That seam is designed where it is used; freezing a guess for it in a
//! public trait here would be the expensive kind of wrong."* The endpoint
//! core is where it is used, so it is designed here — as a **supertrait
//! extension**, so that every item [`Channel`] froze in slice 1 stays
//! exactly where it was and slice 1's tests keep passing unchanged.
//!
//! # What this abstracts, and why it must be abstracted at all
//!
//! `hiss::noise!` generates its transitions as *inherent* methods on a
//! family of per-state types (`IKInitiatorMsg1<P>`, `IKResponderMsg1Intro<P>`,
//! …). Inherent methods on generated types cannot be named generically, so
//! a `core::Endpoint<I>` that is generic over the suite cannot call them
//! without a trait to route through. [`crate::channel!`] stamps the one
//! `impl` per suite; nothing here is implemented by hand.
//!
//! # The staged pair is the point
//!
//! [`read_msg1_intro`](Handshake::read_msg1_intro) /
//! [`complete`](Handshake::complete) are hiss's suspending read
//! (`read_message_1_intro` → `IKResponderMsg1Intro::complete`). They are
//! what makes §6.1's **1 DH to inspect, 2 to authenticate** ladder real
//! rather than aspirational: the `es` is paid by the intro, the proving
//! `ss` waits in an owned, parkable mid-state, and dropping that mid-state
//! is the rejection.
//!
//! # The generic-associated-type shape
//!
//! Every state type is a GAT over the provider, so the core's bound is one
//! clause (`I::Suite: Handshake`) rather than a provider parameter carried
//! on every `impl` block and every function. [`Handshake::Transport`],
//! [`Handshake::Seal`] and [`Handshake::Open`] need no GAT: hiss's
//! `Transport<Proto>` is generic over the *pattern*, not the provider — the
//! provider is consumed by the handshake and does not survive into the
//! session. That is what lets an established session be provider-free.

use std::num::NonZeroU64;

use hiss::curve::Curve;
use hiss::noise::HandshakeError;
use hiss::provider::{CryptoKeyProvider, DhProvider};

use crate::constants;
use crate::packet::Channel;

/// A suite's static public key type — §2.4's canonical encoding is
/// `AsRef<[u8]>` on it.
///
/// Not sugar: written out, the projection is a `clippy::type_complexity` on
/// sight, and it appears in three signatures below.
pub type PublicKeyFor<C> = <<C as Channel>::Curve as Curve>::PublicKey;

/// The suite's IK handshake ladder. Implemented by [`crate::channel!`],
/// never by hand.
///
/// The `Vec<u8>` returns carry the **Noise message only** — the caller
/// frames it with a header and mac1. hiss returns owned fixed arrays
/// (`[u8; IK::MSG1_SIZE]`), but a generic trait cannot name `[u8;
/// Self::MSG1_LEN]` without `generic_const_exprs`, which is not stable.
/// The one allocation per handshake message is on no hot path: a flood
/// never reaches here — it dies at the length gate or mac1, at zero DH and
/// zero allocations.
pub trait Handshake: Channel {
    /// The initiator before msg1 is written. `IKInitiatorMsg1<P>`.
    type Initiator<P: DhProvider<Self::Curve>>;
    /// The initiator after msg1, awaiting msg2. `IKInitiatorMsg2<P>`.
    type InitiatorSent<P: DhProvider<Self::Curve>>;
    /// The responder before msg1 is read. `IKResponderMsg1<P>`.
    type Responder<P: DhProvider<Self::Curve>>;
    /// The **suspended** responder: `es` paid, `ss` unpaid, the claimed
    /// static revealed. `IKResponderMsg1Intro<P>`. §6.1's stage 1.
    type Msg1Intro<P: DhProvider<Self::Curve>>;
    /// The responder after msg1 is fully read, before msg2 is written.
    /// `IKResponderMsg2<P>`. §6.1's stage 2.
    type ResponderRead<P: DhProvider<Self::Curve>>;

    /// The completed handshake, before the datagram split. Provider-free.
    type Transport;
    /// The sealing half of the datagram pair (§16.4's "seal").
    type Seal;
    /// The opening half of the datagram pair (§16.4's "open").
    type Open;

    /// Begin an outbound initiation against a known peer static (§5.5).
    fn initiator<P: DhProvider<Self::Curve>>(
        provider: P,
        prologue: &[u8],
        remote_static: PublicKeyFor<Self>,
    ) -> Self::Initiator<P>;

    /// Write msg1 — **2 DH** (`es`, `ss`) — over a fresh ephemeral hiss
    /// mints internally, carrying §5.2's 12-byte timestamp payload.
    fn write_msg1<P: DhProvider<Self::Curve>>(
        state: Self::Initiator<P>,
        static_key: <P as CryptoKeyProvider<Self::Curve>>::PrivateKey,
        payload: &[u8; constants::MSG1_PAYLOAD_LEN],
    ) -> Result<(Vec<u8>, Self::InitiatorSent<P>), HandshakeError>;

    /// Complete the initiation from msg2 — **2 DH** (`ee`, `se`).
    ///
    /// `msg2` must be exactly `Self::MSG2_LEN` bytes; §3.1's gate has
    /// already guaranteed that for anything that reaches here.
    fn read_msg2<P: DhProvider<Self::Curve>>(
        state: Self::InitiatorSent<P>,
        msg2: &[u8],
    ) -> Result<Self::Transport, HandshakeError>;

    /// Begin a responder handshake. **0 DH** — this only installs our own
    /// static, so it is safe to build lazily at `read_identity()`.
    fn responder<P: DhProvider<Self::Curve>>(
        provider: P,
        prologue: &[u8],
        static_key: <P as CryptoKeyProvider<Self::Curve>>::PrivateKey,
    ) -> Result<Self::Responder<P>, HandshakeError>;

    /// Read msg1 as far as the peer's **claimed** static — **1 DH**
    /// (`es`). §6.1's stage 1.
    ///
    /// The returned key is *claimed, not proven*: possession is only
    /// established when [`complete`](Handshake::complete) succeeds.
    fn read_msg1_intro<P: DhProvider<Self::Curve>>(
        state: Self::Responder<P>,
        msg1: &[u8],
    ) -> Result<(PublicKeyFor<Self>, Self::Msg1Intro<P>), HandshakeError>;

    /// Pay the proving `ss` — **1 DH** — and decrypt msg1's payload.
    /// §6.1's stage 2. Dropping the mid-state instead is the rejection.
    fn complete<P: DhProvider<Self::Curve>>(
        mid: Self::Msg1Intro<P>,
    ) -> Result<([u8; constants::MSG1_PAYLOAD_LEN], Self::ResponderRead<P>), HandshakeError>;

    /// Write msg2 — **2 DH** (`ee`, `se`) — and finish the handshake.
    /// §6.1's stage 3.
    fn write_msg2<P: DhProvider<Self::Curve>>(
        state: Self::ResponderRead<P>,
    ) -> Result<(Vec<u8>, Self::Transport), HandshakeError>;

    /// Split a completed handshake into §16.4's seal/open pair, ratcheting
    /// on a counter-derived epoch schedule (§7.7).
    fn into_datagram(
        transport: Self::Transport,
        epoch_size: NonZeroU64,
    ) -> (Self::Seal, Self::Open);

    /// The counter the next **successful** [`seal`](Handshake::seal) will
    /// use — Appendix A.2's `next_counter()`, routed through the suite.
    ///
    /// §3.4 makes the 14-byte Data header the AEAD associated data, so the
    /// header — which carries the counter — must be built *before* the
    /// seal. This is the accessor that makes that possible without
    /// mirroring hiss-owned state. A failed seal leaves it unchanged, and
    /// at `u64::MAX` it returns the counter that will never be used (§7.9).
    fn next_counter(seal: &Self::Seal) -> u64;

    /// hiss's channel binding for the session, off the sealing half.
    ///
    /// **[RATIFIED 2026/08/15 — ruling 89]** §16.2's `session_id()` is
    /// `hiss::noise::SessionId`, re-exported, and the ruling says it "is
    /// reachable from the seal half slither already holds
    /// (`DatagramSend::session_id`), so nothing is captured at install."
    /// That is true of hiss's concrete type and **not** of
    /// [`Seal`](Handshake::Seal), which is an associated type with no
    /// bounds — so the shell cannot reach it without this accessor. Routed
    /// through the suite for exactly the reason
    /// [`next_counter`](Handshake::next_counter) is.
    fn session_id(seal: &Self::Seal) -> &::hiss::noise::SessionId;

    /// Seal one Data packet's plaintext (§7.1).
    ///
    /// `ad` is the 14 header bytes verbatim (§3.4); `out` must have room
    /// for `plaintext.len() + AEAD_TAG_LEN`. Returns the counter the packet
    /// was sealed under — simultaneously the AEAD nonce, the packet number
    /// and the epoch selector — and the bytes written. **On any error the
    /// counter does not advance and nothing is written**, which is what
    /// §16.7's "on seal failure nothing moved" rests on.
    fn seal(
        seal: &mut Self::Seal,
        ad: &[u8],
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<(u64, usize), HandshakeError>;

    /// Open one Data packet at the counter its header carried (§7.2).
    ///
    /// Takes `&mut` because the §7.7 epoch ratchet commits on success.
    /// **No replay rejection happens here** — hiss imposes neither
    /// monotonicity nor uniqueness, by design, so §7.2's window is the
    /// caller's duty and is strictly post-AEAD. On an error `out` holds
    /// unauthenticated bytes that must not be read.
    fn open(
        open: &mut Self::Open,
        counter: u64,
        ad: &[u8],
        ciphertext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, HandshakeError>;
}

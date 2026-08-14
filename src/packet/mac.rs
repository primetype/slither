//! §4 — mac1, the day-one DoS gate.
//!
//! ```text
//! key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
//! mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
//! ```
//!
//! **Both BLAKE2b invocations are plain** — no salt, no personalisation
//! (ruling 66), matching WireGuard's plain keyed BLAKE2s. Domain
//! separation is by **concatenation**: [`MAC1_LABEL`] is a prefix on the
//! key preimage, never the primitive's personalisation parameter. The key
//! preimage is `MAC1_LABEL.len() + STATIC_PUBLIC_LEN` = 12 + 65 = 77 bytes
//! on the reference suite.
//!
//! `recipient_static_canonical` is the **recipient's** static in §2.4's
//! canonical encoding: on a HandshakeInit the responder's, on a
//! HandshakeResp the initiator's.
//!
//! mac1 is verified **before any curve or DH work** (§4.2) — a garbage
//! flood, a wrong-key packet or a mismatched-suite packet dies at one
//! keyed hash and never reaches the DH provider. Data packets carry no
//! mac1 at all (§3.4).
//!
//! # The one raw primitive
//!
//! This module takes BLAKE2b from `cryptoxide` **directly**, which is the
//! single exception to "every Noise/curve operation flows through hiss":
//! mac1 is a keyed hash over public data, not session cryptography. It
//! must not become two exceptions. In particular **no `hiss::noise` hash
//! type appears here**, because that would silently make mac1 follow the
//! suite's `Hash` and undo §4.4 — mac1 is fixed keyed-BLAKE2b for every
//! suite.

use cryptoxide::hashing::blake2b::Blake2b;

use crate::constants;

/// A mac1 key: `BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)`.
/// §4.1.
///
/// **Not secret** (§4.3). Anyone holding the recipient's public static can
/// compute it and mint mac1-valid packets; mac1 is an anti-amplification
/// and cheap-reject gate, and the real authentication is the Noise
/// handshake underneath. There is deliberately no `Zeroize` and no `Drop`
/// impl: either would imply a security property mac1 explicitly does not
/// have.
///
/// # Why a key type rather than a free function
///
/// The derivation happens **once per static**, not once per packet, and
/// the type is what makes that the easy thing to write. On the receive
/// path the recipient is always us, so the key is a constant of the
/// endpoint; on the send path it is a constant of the peer. A garbage
/// flood must cost one keyed hash, not a hash plus a key derivation
/// (§4.2's cost ladder) — and a free function keyed by a public key hands
/// every caller the chance to re-derive per packet, which nothing would
/// ever notice.
#[derive(Clone)]
pub(crate) struct Mac1Key([u8; 32]);

impl Mac1Key {
    /// Derive from §2.4's canonical static encoding — for a
    /// `C: Channel`, the `as_ref()` octets of the recipient's
    /// `Curve::PublicKey`.
    ///
    /// Plain BLAKE2b-256 over `MAC1_LABEL ‖ static_canonical`; the two
    /// `update` calls are one hash over the concatenation.
    pub(crate) fn derive(static_canonical: &[u8]) -> Self {
        Self(
            Blake2b::<256>::new()
                .update(constants::MAC1_LABEL)
                .update(static_canonical)
                .finalize(),
        )
    }

    /// The 16-byte tag over `preimage` — "all packet bytes preceding the
    /// tag" (§4.1), which §3.1's exact length gate resolves to `[0, 180)`
    /// for a HandshakeInit and `[0, 91)` for a HandshakeResp.
    ///
    /// Plain keyed BLAKE2b-128. `finalize_at`, not `finalize`: cryptoxide
    /// emits the array-returning `finalize` for 224/256/384/512 only, so a
    /// 128-bit context writes into a caller-owned buffer.
    pub(crate) fn tag(&self, preimage: &[u8]) -> [u8; constants::MAC1_LEN] {
        let mut tag = [0u8; constants::MAC1_LEN];
        Blake2b::<128>::new_keyed(&self.0)
            .update(preimage)
            .finalize_at(&mut tag);
        tag
    }

    /// `true` iff `candidate` is the mac1 of `preimage` under this key.
    ///
    /// The comparison folds the whole difference before testing it.
    /// Constant time is **not required** here — §4.3 says the key is
    /// derived from public data, so a timing leak leaks nothing an
    /// attacker cannot already compute — and it is written this way
    /// anyway, because "this comparison is variable-time on purpose" is a
    /// comment nobody believes three years later.
    pub(crate) fn verify(&self, preimage: &[u8], candidate: &[u8]) -> bool {
        if candidate.len() != constants::MAC1_LEN {
            return false;
        }

        let expected = self.tag(preimage);
        let mut diff = 0u8;
        for (a, b) in expected.iter().zip(candidate) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

// `Blake2b::<128>` above takes its output width as a const-generic
// *literal*, which is the one place `MAC1_LEN` could drift from the
// primitive that produces it: `finalize_at` checks the buffer length at
// run time, and a run-time panic in the DoS gate is not a failure mode
// worth having. This makes the disagreement a build failure instead.
// (Stage one needs no such tie: `Blake2b::<256>::finalize` returns
// `[u8; 32]`, and the field's type is that array.)
const _: () = assert!(constants::MAC1_LEN * 8 == 128);

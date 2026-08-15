//! The identity seam — the `I` in §16.4's `core::Endpoint<I: Identity>`.
//!
//! §16.4 parameterises the endpoint core over an identity because "the
//! mid-state map is typed over `I::Provider`": a parked staged chain owns a
//! suspended hiss handshake, and that type carries the provider. So the
//! seam is not "a key" — it is the pair *(provider, static private handle)*
//! a single handshake consumes.
//!
//! # Why [`Identity::open`] is a factory
//!
//! Three facts about hiss 0.3.2 force it:
//!
//! 1. Every handshake instance **consumes a provider by value** —
//!    `IK::initiator(provider, …)` and `IK::responder(provider, …)`.
//! 2. Each also needs our static private key **by value** — the responder
//!    at construction, the initiator at `write_message_1`.
//! 3. `CryptoKeyProvider::PrivateKey` is deliberately **not** `Clone`
//!    ("secret keys should not be silently duplicated").
//!
//! An endpoint runs many handshakes at once — up to `INTRO_QUEUE_CAP`
//! parked mid-states, each of which §17.5 says "holds the endpoint's static
//! provider". One provider and one key cannot serve them, so the seam mints
//! a fresh pair per handshake. `&self`, so it can be called while the
//! introduction queue is borrowed; fallible, because a hardware call can
//! fail and the alternative is a panic on the accept path.
//!
//! Nothing here asks for key *bytes*. That is the whole point: an iOS
//! Secure Enclave identity returns a retained `SecKey` handle — a refcount
//! bump, not a copy — so S21's "key material is non-exportable" survives
//! the seam intact.
//!
//! # No `Send`, deliberately
//!
//! Neither [`Identity`] nor its associated types carry a `Send` bound, and
//! none may be added: an enclave-backed key is not `Send`, and a bound here
//! would exclude the case the seam exists for (S21). `hiss::provider::DhProvider`
//! itself carries no `Send` bound either, so the requirement is
//! satisfiable end to end.
//!
//! # `DhProviderAsync` is excluded on purpose
//!
//! [`Identity::Provider`] is bounded on `DhProvider` — the **synchronous**
//! surface — and `hiss::provider::DhProviderAsync` is deliberately not
//! accommodated. Three independent reasons, so no one of them lapsing
//! reopens the question:
//!
//! 1. **hiss cannot drive it.** The state machines `noise!` generates are
//!    bounded `CP: DhProvider<Curve>` at every entry point; an async-only
//!    backend cannot run an IK handshake through hiss at all.
//! 2. **§16.4 forbids the shape.** The core's staged verbs are ratified
//!    *synchronous*. An async DH would make `read_identity` /
//!    `authenticate` / `accept` return futures inside the core, which is a
//!    spec change rather than an implementation choice.
//! 3. **It would reintroduce the very bound this seam exists to avoid.**
//!    `DhProviderAsync::dh_async` returns `impl Future<Output = …> + Send`
//!    — a `Send` requirement written into the trait. Accommodating it would
//!    import `Send` into the one seam built to be free of it.
//!
//! Reason 3 is the counter-intuitive one, and it is why "support both
//! surfaces" is the wrong instinct here.

use std::cell::RefCell;
use std::fmt;

use hiss::curve::Curve;
use hiss::curve::p256::{Error as P256Error, P256r1PrivateKey, P256r1PublicKey};
use hiss::noise::P256;
use hiss::provider::{CryptoKeyProvider, DhProvider, EphemeralOnly};
use rand_chacha::ChaCha20Rng;
use rand_core::{CryptoRng, Rng, SeedableRng};

use crate::packet::{Channel, Handshake};

/// The DH curve an identity's suite uses.
pub type CurveOf<I> = <<I as Identity>::Suite as Channel>::Curve;

/// The static public key type an identity's suite uses — §2.4's canonical
/// encoding is `AsRef<[u8]>` on it.
pub type PublicKeyOf<I> = <CurveOf<I> as Curve>::PublicKey;

/// The private-key handle an identity's provider mints.
pub type PrivateKeyOf<I> = <<I as Identity>::Provider as CryptoKeyProvider<CurveOf<I>>>::PrivateKey;

/// An endpoint's long-term static identity, as a **factory** for the
/// per-handshake `(provider, static private key)` pair hiss consumes.
///
/// See the [module docs](self) for why this is a factory rather than a key
/// holder, and for why the asynchronous DH surface is deliberately absent.
pub trait Identity {
    /// The crypto suite this identity's key belongs to.
    ///
    /// Bounded on [`Handshake`] rather than [`Channel`] so the core needs
    /// no second `where` clause: [`Handshake`] is a supertrait extension of
    /// [`Channel`], and `slither::channel!` stamps both.
    type Suite: Handshake;

    /// The DH provider one handshake runs on.
    ///
    /// **No `Send` bound, ever** — see the module docs.
    type Provider: DhProvider<CurveOf<Self>>;

    /// Why [`open`](Identity::open) failed.
    type Error: ::core::error::Error + 'static;

    /// The canonical static public key (§2.4).
    ///
    /// Cheap, cached and infallible: mac1 keying and every identity table
    /// read it, so it must not become a hardware round-trip.
    fn public_static(&self) -> &PublicKeyOf<Self>;

    /// Mint the provider and static-key handle for **one** handshake.
    ///
    /// Called lazily — at `read_identity()` on the responder path, and once
    /// per attempt on the initiator path — never at park (§17.5: an
    /// enclave static would otherwise hold up to 1024 concurrent provider
    /// handles for packets nobody has looked at yet).
    fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>;
}

// ═══════════════════════════════════════════════════════════════════════
// The software identity
// ═══════════════════════════════════════════════════════════════════════

/// Why a [`SoftwareIdentity`] could not be built or opened.
#[derive(Debug, thiserror::Error)]
pub enum SoftwareIdentityError {
    /// The 32 bytes are not a canonical secp256r1 scalar in `[1, n-1]`.
    #[error("the scalar is not a valid secp256r1 private key")]
    InvalidScalar(#[source] P256Error),
}

/// The crates.io-only default identity: a P-256 static held in memory.
///
/// **P-256 specifically**, not any suite's curve. hiss exposes no generic
/// "import a private key from bytes" seam — `P256r1PrivateKey::from_bytes`
/// is the concrete one — and [`Identity::open`] must hand out a fresh
/// owned key per handshake, which a non-`Clone` handle can only satisfy by
/// re-import. The suite's *cipher* and *hash* stay generic; only the curve
/// is fixed. A backend for another curve implements [`Identity`] directly.
///
/// # The RNG is a seed source, not the handshake RNG
///
/// `open()` draws a **fresh 32-byte sub-seed** from `R` and builds the
/// handshake's `EphemeralOnly` around a `ChaCha20Rng` seeded with it.
/// Cloning `R` into each handshake would be a catastrophe rather than a
/// convenience: two clones of a seeded RNG produce the *same* ephemeral,
/// and §5.5 requires a completely fresh ephemeral on every retransmit.
/// Drawing a sub-seed advances the parent, so every handshake gets a
/// distinct stream and a seeded parent still makes the whole sequence
/// reproducible.
pub struct SoftwareIdentity<S, R = ChaCha20Rng> {
    scalar: [u8; 32],
    public: P256r1PublicKey,
    rng: RefCell<R>,
    suite: ::core::marker::PhantomData<fn() -> S>,
}

impl<S, R> fmt::Debug for SoftwareIdentity<S, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The scalar is never printed.
        f.debug_struct("SoftwareIdentity").finish_non_exhaustive()
    }
}

impl<S, R> SoftwareIdentity<S, R>
where
    R: CryptoRng,
{
    /// Build an identity from the canonical big-endian encoding of a
    /// secp256r1 private scalar, plus the CSPRNG sub-seeds are drawn from.
    pub fn from_scalar(scalar: [u8; 32], rng: R) -> Result<Self, SoftwareIdentityError> {
        let key =
            P256r1PrivateKey::from_bytes(scalar).map_err(SoftwareIdentityError::InvalidScalar)?;
        let public = key.public();
        Ok(Self {
            scalar,
            public,
            rng: RefCell::new(rng),
            suite: ::core::marker::PhantomData,
        })
    }

    /// Generate a fresh static key from `rng`, and keep `rng` as the
    /// sub-seed source.
    pub fn generate(mut rng: R) -> Result<Self, SoftwareIdentityError> {
        let mut scalar = [0u8; 32];
        loop {
            rng.fill_bytes(&mut scalar);
            if P256r1PrivateKey::from_bytes(scalar).is_ok() {
                break;
            }
        }
        Self::from_scalar(scalar, rng)
    }
}

impl<S, R> Identity for SoftwareIdentity<S, R>
where
    S: Handshake<Curve = P256>,
    R: CryptoRng,
{
    type Suite = S;
    type Provider = EphemeralOnly<ChaCha20Rng>;
    type Error = SoftwareIdentityError;

    fn public_static(&self) -> &P256r1PublicKey {
        &self.public
    }

    fn open(&self) -> Result<(Self::Provider, P256r1PrivateKey), Self::Error> {
        let mut seed = [0u8; 32];
        self.rng.borrow_mut().fill_bytes(&mut seed);
        let key = P256r1PrivateKey::from_bytes(self.scalar)
            .map_err(SoftwareIdentityError::InvalidScalar)?;
        Ok((EphemeralOnly::new(ChaCha20Rng::from_seed(seed)), key))
    }
}

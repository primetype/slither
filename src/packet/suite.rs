//! §2 — crypto suites: the [`Channel`] trait, the [`crate::channel!`] macro, and
//! slither's own reference suite.
//!
//! A suite is declared **once**, by [`crate::channel!`], which stamps the
//! `hiss::noise!` IK invocation with a caller-chosen `<Curve, Cipher,
//! Hash>` triple and implements [`Channel`] over it. The IK token block
//! and msg1's 12-byte payload declaration are hardcoded inside the macro:
//! **IK is the only pattern** (§2.2), and §5.2's payload is wire, not
//! policy.
//!
//! **There is no suite identifier on the wire.** Endpoints are
//! monomorphic per suite; a mismatched-suite packet dies silently — at
//! the length gate or at mac1 when the curves differ, at the first AEAD
//! open for a same-curve sibling suite (§2.2, amended by ruling 279). The
//! version byte does not encode the suite, and nothing here ever asks
//! "which suite is this".
//!
//! **Offered suites (ruling 279).** The reference suite is
//! `P256 / ChaChaPoly / Blake2b`; `P256 / AesGcm / Blake2b` is offered
//! alongside it, declared the same way. Choose knowing the hardware:
//! cryptoxide's AES fast path exists on `aarch64` with
//! `target_feature="aes"` — on by default on Apple targets, opt-in via
//! `-C target-feature=+aes` on `aarch64-unknown-linux-gnu` — and measured
//! 1.63× the reference suite's end-to-end stream throughput there.
//! Everywhere else (x86-64, where cryptoxide 0.6.x has no hardware path;
//! wasm, which never will) AES-GCM runs a constant-time software fallback
//! and ChaChaPoly is the right choice — which is why it remains the
//! reference. The two suites share every wire length (`PK` = 65,
//! `TAG` = 16), and mismatched deployments fail closed (§2.2).
//!
//! Everything the suite varies is listed by §2.3 and is confined to the
//! four derived sizes below. The three packet headers, `MAX_DATAGRAM`,
//! `MAX_PLAINTEXT`, mac1's label and length, and the whole frame layer are
//! suite-independent — mac1 in particular is fixed keyed-BLAKE2b for every
//! suite (§4.4) and does **not** follow [`Channel::Hash`].

use hiss::noise::{Blake2b, ChaChaPoly, P256};

use crate::constants;

/// A crypto suite: one `<Curve, Cipher, Hash>` triple, plus the four wire
/// sizes §2.3 derives from it. Declared by [`crate::channel!`]. §2.2.
///
/// # Not implemented by hand
///
/// Every item below is *derived*, and [`crate::channel!`] derives all of them.
/// Implementing this trait manually is possible and pointless: a
/// hand-written `PROTOCOL_NAME` that disagreed with the one hiss seeds the
/// handshake hash with would be a silent interop break with a green test
/// suite, and a hand-written size would move a wire byte.
///
/// # What this trait deliberately does not carry
///
/// No handshake surface: no key, no state machine, no DH. `hiss::noise!`
/// generates its transitions as *inherent* methods across a family of
/// per-state types, and the seam that abstracts them is the staged-accept
/// ladder of §6.1–6.2 — the identity hook on `read_message_1_with`, the
/// one-DH/two-DH split, the fresh-ephemeral retransmit. That seam is
/// designed where it is used; freezing a guess for it in a public trait
/// here would be the expensive kind of wrong.
pub trait Channel {
    /// The DH curve.
    ///
    /// `PublicKey: AsRef<[u8]>` is §2.4's canonical-encoding requirement,
    /// expressed slither-side because `hiss::curve::Curve` bounds
    /// `PublicKey` by `Clone` alone. Those octets *are* the canonical
    /// encoding — mac1 is keyed over them (§4.4), §6.7's tie-break
    /// compares them, and identity maps key on them. `Ord` is deliberately
    /// **not** required (Appendix A.3): the tie-break compares
    /// equal-length octet strings, which needs no ordering on the key
    /// type.
    type Curve: hiss::curve::DhCurve<PublicKey: AsRef<[u8]>>;

    /// The AEAD.
    type Cipher: hiss::noise::Cipher;

    /// The hash the *Noise handshake* uses. **mac1 does not follow it**
    /// (§4.4).
    type Hash: hiss::noise::Hash;

    /// `Noise_IK_<curve>_<cipher>_<hash>` — the string that seeds the
    /// initial handshake hash. §2.2 pins the reference suite's value
    /// (`Noise_IK_P256_ChaChaPoly_BLAKE2b`) by test.
    ///
    /// Built at compile time from hiss's own `NAME` constants, never
    /// spelled out: see [`protocol_name`].
    const PROTOCOL_NAME: &'static str;

    /// `Curve::PUBLIC_KEY_SIZE` — the length of §2.4's canonical static
    /// encoding.
    const STATIC_PUBLIC_LEN: usize;

    /// `Cipher::TAG_SIZE`.
    const AEAD_TAG_LEN: usize;

    /// Bytes in the IK handshake's first Noise message. §2.3:
    /// `PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)`.
    const MSG1_LEN: usize;

    /// Bytes in the IK handshake's second Noise message. §2.3:
    /// `PK + TAG`.
    const MSG2_LEN: usize;

    /// Bytes on the wire for a complete handshake-initiation packet. §2.3:
    /// `INIT_HEADER_LEN + MSG1_LEN + MAC1_LEN`.
    ///
    /// §3.1's gate accepts a `PKT_HANDSHAKE_INIT` of **exactly** this
    /// length and nothing else (ruling 65).
    const INIT_PACKET_LEN: usize;

    /// Bytes on the wire for a complete handshake-response packet. §2.3:
    /// `RESP_HEADER_LEN + MSG2_LEN + MAC1_LEN`.
    ///
    /// §3.1's gate accepts a `PKT_HANDSHAKE_RESP` of **exactly** this
    /// length and nothing else (ruling 65).
    const RESP_PACKET_LEN: usize;
}

// ═══════════════════════════════════════════════════════════════════════
// The protocol-name builder (`channel!`'s, not a public surface)
// ═══════════════════════════════════════════════════════════════════════

/// The largest protocol name [`crate::channel!`] will build, in bytes.
///
/// Not public API. It is `pub` because [`crate::channel!`] expands in the
/// **caller's** crate, so every path the expansion names must be publicly
/// reachable from there — forced, not chosen.
#[doc(hidden)]
pub const PROTOCOL_NAME_CAP: usize = 96;

/// Build `Noise_<pattern>_<curve>_<cipher>_<hash>` in a `const` context,
/// returning a padded buffer and the used length.
///
/// Not public API; see [`PROTOCOL_NAME_CAP`] for why it is `pub`.
///
/// The name is **derived** from hiss's own `NAME` constants — including
/// the pattern's, read off the type `hiss::noise!` generated — rather than
/// written out as a literal. That string seeds the initial handshake hash,
/// so a spelling that drifted from hiss's would break interoperability
/// without failing anything locally.
///
/// A `&'static str` cannot be returned directly: there is no `const`
/// string concatenation on stable Rust, and a `const fn` cannot return a
/// reference into its own frame. The caller turns the pair into a
/// `&'static str` in its own `const` block.
#[doc(hidden)]
pub const fn protocol_name(
    pattern: &str,
    curve: &str,
    cipher: &str,
    hash: &str,
) -> ([u8; PROTOCOL_NAME_CAP], usize) {
    let mut out = [0u8; PROTOCOL_NAME_CAP];
    let mut len = 0;

    len = append(&mut out, len, b"Noise_");
    len = append(&mut out, len, pattern.as_bytes());
    len = append(&mut out, len, b"_");
    len = append(&mut out, len, curve.as_bytes());
    len = append(&mut out, len, b"_");
    len = append(&mut out, len, cipher.as_bytes());
    len = append(&mut out, len, b"_");
    len = append(&mut out, len, hash.as_bytes());

    (out, len)
}

/// Append `src` at `len`, returning the new length. Overflow is a build
/// failure, not a truncation.
const fn append(out: &mut [u8; PROTOCOL_NAME_CAP], mut len: usize, src: &[u8]) -> usize {
    assert!(
        len + src.len() <= PROTOCOL_NAME_CAP,
        "the suite's Noise protocol name exceeds PROTOCOL_NAME_CAP"
    );

    let mut i = 0;
    while i < src.len() {
        out[len] = src[i];
        i += 1;
        len += 1;
    }
    len
}

// ═══════════════════════════════════════════════════════════════════════
// `channel!`
// ═══════════════════════════════════════════════════════════════════════

/// Declare a crypto suite. §2.2.
///
/// ```
/// use slither::prelude::*;
///
/// slither::channel! {
///     /// My application's suite.
///     pub MySuite<P256, ChaChaPoly, Blake2b>;
/// }
/// ```
///
/// The prelude folds hiss's three suite types in, so that one line covers
/// the declaration (ruling 278). **It does not remove the Cargo.toml
/// requirement below** — the expansion names `::hiss` absolutely.
///
/// The invocation stamps three items into the invoking module: the suite
/// type you named, an `impl` of [`Channel`] for it, and a type called `IK`
/// — the `hiss::noise!` state machine for the handshake. The IK token
/// block and msg1's 12-byte payload are hardcoded here: **IK is the only
/// pattern**, and there is no pattern parameter to pass.
///
/// # Your crate must depend on `hiss`
///
/// `hiss::noise!` emits absolute `::hiss::…` paths, and a `macro_rules`
/// wrapper cannot rewrite them, so the expansion resolves `hiss` in
/// **your** crate:
///
/// ```toml
/// [dependencies]
/// slither = "0.2"
/// hiss = { version = "0.3", default-features = false }   # required, see below
/// ```
///
/// `default-features = false` matches what slither itself asks for:
/// hiss's default `x25519-cryptoxide` backs a curve slither never
/// touches, and enabling it here pulls that backend into your build for
/// nothing.
///
/// slither re-exports the version it was built against as
/// [`slither::hiss`](crate::hiss) so you can check the two agree.
/// **The re-export does not remove the requirement**: a `use
/// slither::hiss` brings no `::hiss` crate root into scope, and the
/// expansion names `::hiss`.
///
/// # One invocation per module
///
/// The generated state machine must be named `IK`, because
/// `hiss::noise!` uses the declared identifier as the pattern's `NAME`
/// and that string seeds the initial handshake hash. Two invocations in
/// one module therefore collide on `IK`; put each suite in its own
/// module. §2.2 forecloses two suites on one socket anyway — endpoints
/// are monomorphic per suite and there is no suite byte to switch on.
#[macro_export]
macro_rules! channel {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident < $curve:ty, $cipher:ty, $hash:ty > ;
    ) => {
        ::hiss::noise! {
            /// The IK handshake for this suite (§2.2 — IK is the only
            /// pattern), carrying §5.2's 12-byte timestamp payload on
            /// msg1 and no payload on msg2.
            $vis IK<$curve, $cipher, $hash> {
                <- s
                ...
                -> e, es, s, ss [12]
                <- e, ee, se
            }
        }

        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        $vis struct $name;

        impl $crate::packet::Channel for $name {
            type Curve = $curve;
            type Cipher = $cipher;
            type Hash = $hash;

            const PROTOCOL_NAME: &'static str = {
                const RAW: ([u8; $crate::packet::suite::PROTOCOL_NAME_CAP], usize) =
                    $crate::packet::suite::protocol_name(
                        <IK as ::hiss::noise::Pattern>::NAME,
                        <$curve as ::hiss::curve::Curve>::NAME,
                        <$cipher as ::hiss::noise::Cipher>::NAME,
                        <$hash as ::hiss::noise::Hash>::NAME,
                    );
                const BYTES: &[u8] = &RAW.0;

                match ::core::str::from_utf8(BYTES.split_at(RAW.1).0) {
                    ::core::result::Result::Ok(name) => name,
                    ::core::result::Result::Err(_) => {
                        ::core::panic!("the suite's Noise protocol name is not UTF-8")
                    }
                }
            };

            const STATIC_PUBLIC_LEN: usize =
                <$curve as ::hiss::curve::Curve>::PUBLIC_KEY_SIZE;
            const AEAD_TAG_LEN: usize =
                <$cipher as ::hiss::noise::Cipher>::TAG_SIZE;

            // §2.3, executed rather than tabulated. Writing the four sizes
            // AS the derivation — instead of writing literals and
            // asserting them against it — means a suite cannot be declared
            // whose sizes disagree with the formula.
            const MSG1_LEN: usize = Self::STATIC_PUBLIC_LEN
                + (Self::STATIC_PUBLIC_LEN + Self::AEAD_TAG_LEN)
                + ($crate::constants::MSG1_PAYLOAD_LEN + Self::AEAD_TAG_LEN);
            const MSG2_LEN: usize = Self::STATIC_PUBLIC_LEN + Self::AEAD_TAG_LEN;
            const INIT_PACKET_LEN: usize = $crate::constants::INIT_HEADER_LEN
                + Self::MSG1_LEN
                + $crate::constants::MAC1_LEN;
            const RESP_PACKET_LEN: usize = $crate::constants::RESP_HEADER_LEN
                + Self::MSG2_LEN
                + $crate::constants::MAC1_LEN;
        }

        // §6.1's ladder, routed through the suite so a generic
        // `core::Endpoint<I>` can call it. `hiss::noise!` generates its
        // transitions as INHERENT methods on per-state types, which no
        // generic caller can name; this block is the only place those
        // names are written down, and it is mechanical — every identifier
        // below is `IK` plus hiss's fixed `{Role}Msg{n}[Intro]` suffix.
        //
        // Purely additive: no item of the `Channel` impl above moves, so
        // slice 1's frozen wire tests are untouched.
        impl $crate::packet::Handshake for $name {
            type Initiator<P: ::hiss::provider::DhProvider<$curve>> = IKInitiatorMsg1<P>;
            type InitiatorSent<P: ::hiss::provider::DhProvider<$curve>> = IKInitiatorMsg2<P>;
            type Responder<P: ::hiss::provider::DhProvider<$curve>> = IKResponderMsg1<P>;
            type Msg1Intro<P: ::hiss::provider::DhProvider<$curve>> = IKResponderMsg1Intro<P>;
            type ResponderRead<P: ::hiss::provider::DhProvider<$curve>> = IKResponderMsg2<P>;

            type Transport = ::hiss::noise::Transport<IK>;
            type Seal = ::hiss::noise::DatagramSend<IK>;
            type Open = ::hiss::noise::DatagramRecv<IK>;

            fn initiator<P: ::hiss::provider::DhProvider<$curve>>(
                provider: P,
                prologue: &[u8],
                remote_static: <$curve as ::hiss::curve::Curve>::PublicKey,
            ) -> IKInitiatorMsg1<P> {
                IK::initiator(provider, prologue, remote_static)
            }

            fn write_msg1<P: ::hiss::provider::DhProvider<$curve>>(
                state: IKInitiatorMsg1<P>,
                static_key: <P as ::hiss::provider::CryptoKeyProvider<$curve>>::PrivateKey,
                payload: &[u8; $crate::constants::MSG1_PAYLOAD_LEN],
            ) -> ::core::result::Result<
                (::std::vec::Vec<u8>, IKInitiatorMsg2<P>),
                ::hiss::noise::HandshakeError,
            > {
                let (bytes, next) = state.write_message_1(static_key, payload)?;
                ::core::result::Result::Ok((bytes.to_vec(), next))
            }

            fn read_msg2<P: ::hiss::provider::DhProvider<$curve>>(
                state: IKInitiatorMsg2<P>,
                msg2: &[u8],
            ) -> ::core::result::Result<
                ::hiss::noise::Transport<IK>,
                ::hiss::noise::HandshakeError,
            > {
                // §3.1's gate admits only an exactly-`RESP_PACKET_LEN`
                // response, so this conversion cannot fail for anything
                // the core routes here. It is written as an error rather
                // than an `unwrap` because a panic reachable from a
                // received packet is the wrong failure for a transport.
                let exact: &[u8; IK::MSG2_SIZE] = match ::core::convert::TryFrom::try_from(msg2) {
                    ::core::result::Result::Ok(m) => m,
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::hiss::noise::HandshakeError::MessageTooShort,
                        );
                    }
                };
                state.read_message_2(exact)
            }

            fn responder<P: ::hiss::provider::DhProvider<$curve>>(
                provider: P,
                prologue: &[u8],
                static_key: <P as ::hiss::provider::CryptoKeyProvider<$curve>>::PrivateKey,
            ) -> ::core::result::Result<IKResponderMsg1<P>, ::hiss::noise::HandshakeError> {
                IK::responder(provider, prologue, static_key)
            }

            fn read_msg1_intro<P: ::hiss::provider::DhProvider<$curve>>(
                state: IKResponderMsg1<P>,
                msg1: &[u8],
            ) -> ::core::result::Result<
                (
                    <$curve as ::hiss::curve::Curve>::PublicKey,
                    IKResponderMsg1Intro<P>,
                ),
                ::hiss::noise::HandshakeError,
            > {
                let exact: &[u8; IK::MSG1_SIZE] = match ::core::convert::TryFrom::try_from(msg1) {
                    ::core::result::Result::Ok(m) => m,
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::hiss::noise::HandshakeError::MessageTooShort,
                        );
                    }
                };
                state.read_message_1_intro(exact)
            }

            fn complete<P: ::hiss::provider::DhProvider<$curve>>(
                mid: IKResponderMsg1Intro<P>,
            ) -> ::core::result::Result<
                (
                    [u8; $crate::constants::MSG1_PAYLOAD_LEN],
                    IKResponderMsg2<P>,
                ),
                ::hiss::noise::HandshakeError,
            > {
                mid.complete()
            }

            fn write_msg2<P: ::hiss::provider::DhProvider<$curve>>(
                state: IKResponderMsg2<P>,
            ) -> ::core::result::Result<
                (::std::vec::Vec<u8>, ::hiss::noise::Transport<IK>),
                ::hiss::noise::HandshakeError,
            > {
                let (bytes, transport) = state.write_message_2()?;
                ::core::result::Result::Ok((bytes.to_vec(), transport))
            }

            fn into_datagram(
                transport: ::hiss::noise::Transport<IK>,
                epoch_size: ::core::num::NonZeroU64,
            ) -> (
                ::hiss::noise::DatagramSend<IK>,
                ::hiss::noise::DatagramRecv<IK>,
            ) {
                transport.into_datagram_with_epoch(epoch_size)
            }

            fn next_counter(seal: &::hiss::noise::DatagramSend<IK>) -> u64 {
                seal.next_counter()
            }

            fn session_id(
                seal: &::hiss::noise::DatagramSend<IK>,
            ) -> &::hiss::noise::SessionId {
                seal.session_id()
            }

            fn seal(
                seal: &mut ::hiss::noise::DatagramSend<IK>,
                ad: &[u8],
                plaintext: &[u8],
                out: &mut [u8],
            ) -> ::core::result::Result<(u64, usize), ::hiss::noise::HandshakeError> {
                seal.encrypt_next(ad, plaintext, out)
            }

            fn open(
                open: &mut ::hiss::noise::DatagramRecv<IK>,
                counter: u64,
                ad: &[u8],
                ciphertext: &[u8],
                out: &mut [u8],
            ) -> ::core::result::Result<usize, ::hiss::noise::HandshakeError> {
                open.decrypt_at(counter, ad, ciphertext, out)
            }
        }

        // slither's §2.3 arithmetic against hiss's own computed sizes, for
        // EVERY suite the macro stamps. Two things ride on this pair:
        //
        //  * a hiss change to the point encoding or the tag size turns the
        //    BUILD red in every suite, not one golden test in one suite;
        //  * `MSG1_PAYLOAD_LEN` is spelled `[12]` in the token block above
        //    because `hiss::noise!` takes a literal there. This is what
        //    ties that literal to `constants::MSG1_PAYLOAD_LEN`: disagree,
        //    and `MSG1_LEN` no longer equals `IK::MSG1_SIZE`.
        const _: () = assert!(
            <$name as $crate::packet::Channel>::MSG1_LEN == IK::MSG1_SIZE,
            "slither's §2.3 MSG1_LEN disagrees with hiss's computed IK::MSG1_SIZE"
        );
        const _: () = assert!(
            <$name as $crate::packet::Channel>::MSG2_LEN == IK::MSG2_SIZE,
            "slither's §2.3 MSG2_LEN disagrees with hiss's computed IK::MSG2_SIZE"
        );

        // §3.5: neither handshake packet may fragment.
        const _: () = assert!(
            <$name as $crate::packet::Channel>::INIT_PACKET_LEN
                <= $crate::constants::MAX_DATAGRAM
        );
        const _: () = assert!(
            <$name as $crate::packet::Channel>::RESP_PACKET_LEN
                <= $crate::constants::MAX_DATAGRAM
        );
    };
}

// ═══════════════════════════════════════════════════════════════════════
// The reference suite
// ═══════════════════════════════════════════════════════════════════════

crate::channel! {
    /// slither's reference suite (§2.2): `P256 / ChaChaPoly / Blake2b`,
    /// whose Noise protocol name is `Noise_IK_P256_ChaChaPoly_BLAKE2b`.
    ///
    /// **Unattested name** (§2.2 names the suite, not a type). slither
    /// declares it with [`crate::channel!`] like any consumer would — the only
    /// way "declared once via the macro" is true of slither itself.
    pub ReferenceSuite<P256, ChaChaPoly, Blake2b>;
}

// ── The reference suite against §3.5's table ───────────────────────────
//
// The bridge between §2.3's derivation and the literals `constants.rs`
// carries. Slice 0 pinned `STATIC_PUBLIC_LEN` against
// `P256::PUBLIC_KEY_SIZE` directly and noted that this slice should
// re-pin it against the `noise!`-declared channel once that type exists,
// which is the tighter bound. This is that re-pin; slice 0's assertion
// stays, because it costs nothing and keeps `constants.rs`
// self-contained.
const _: () =
    assert!(<ReferenceSuite as Channel>::STATIC_PUBLIC_LEN == constants::STATIC_PUBLIC_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::AEAD_TAG_LEN == constants::AEAD_TAG_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::MSG1_LEN == constants::IK_MSG1_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::MSG2_LEN == constants::IK_MSG2_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::INIT_PACKET_LEN == constants::INIT_PACKET_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::RESP_PACKET_LEN == constants::RESP_PACKET_LEN);

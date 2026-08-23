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
// `channel!` and `channel_psk!`
// ═══════════════════════════════════════════════════════════════════════

/// Declare a crypto suite over the **`IK`** pattern. §2.2.
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
/// type you named, `impl`s of [`Channel`] and [`Handshake`] for it, and a
/// type called `IK` — the `hiss::noise!` state machine for the handshake.
/// The token block and msg1's 12-byte payload are hardcoded here; there is
/// no pattern parameter to pass.
///
/// # The other pattern
///
/// [`channel_psk!`](crate::channel_psk) stamps the same suite over
/// **`IKpsk1`** — msg1 gains a trailing `psk` token — for admitting a peer
/// under a secret carried out of band rather than by a `known` set. §2.2
/// covers when each applies; a psk suite's endpoint is a *separate*
/// endpoint, never a second pattern on one socket.
///
/// [`Handshake`]: crate::packet::Handshake
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
/// hiss = { version = "0.4", default-features = false }   # required, see below
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
/// one module therefore collide on `IK` — and a `channel!` beside a
/// [`channel_psk!`](crate::channel_psk) does **not** collide, since the
/// latter stamps `IKpsk1`. Put each suite in its own module. §2.2
/// forecloses two suites on one socket anyway — endpoints are monomorphic
/// per suite and there is no suite byte to switch on.
#[macro_export]
macro_rules! channel {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident < $curve:ty, $cipher:ty, $hash:ty > ;
    ) => {
        ::hiss::noise! {
            /// The IK handshake for this suite (§2.2), carrying §5.2's
            /// 12-byte timestamp payload on msg1 and no payload on msg2.
            $vis IK<$curve, $cipher, $hash> {
                <- s
                ...
                -> e, es, s, ss [12]
                <- e, ee, se
            }
        }

        $crate::__channel_impl! {
            $(#[$meta])*
            $vis $name < $curve, $cipher, $hash >;
            kind          = no_psk;
            psk_ty        = ();
            machine       = IK;
            initiator     = IKInitiatorMsg1;
            initiator_sent= IKInitiatorMsg2;
            responder     = IKResponderMsg1;
            intro         = IKResponderMsg1Intro;
            responder_read= IKResponderMsg2;
        }
    };
}

/// Declare a crypto suite over the **`IKpsk1`** pattern — msg1's token
/// block gains a trailing `psk`. §2.2.
///
/// ```
/// use slither::prelude::*;
///
/// slither::channel_psk! {
///     /// The suite my in-person pairing window speaks.
///     pub PairingSuite<P256, ChaChaPoly, Blake2b>;
/// }
/// ```
///
/// Identical to [`channel!`](crate::channel) in every respect below —
/// same triple, same hardcoded `[12]` payload, same Cargo.toml
/// requirement, same one-invocation-per-module rule — except that the
/// stamped state machine is named `IKpsk1` and
/// [`Handshake::Psk`](crate::packet::Handshake::Psk) is
/// [`hiss::psk::Psk`] instead of `()`.
///
/// # What the pattern buys
///
/// A pre-shared key established out of band (a QR shown across a table is
/// the motivating case) admits a peer the `known` set cannot: a
/// **stranger**. The `psk` token sits **after** msg1's `s`, so the staged
/// ladder still reveals the claimed static at 1 DH and the key is selected
/// with the peer already named — an unenrolled dialler is rejected having
/// cost the responder **one `es`**, and never the proving `ss`.
///
/// # What changes at the call sites
///
/// A suite stamped here has `Psk = hiss::psk::Psk`, so §16.2's
/// `connect()` and §6.2's `authenticate()` — which are defined only for
/// `Psk = ()` — are **not available** on its endpoint. Use
/// `connect_with(addr, static, psk)` and `authenticate_with(psk)`. That is
/// deliberate: there is no PSK-shaped default to fall into (ruling 280).
///
/// # The wire separates itself
///
/// The pattern name seeds the Noise protocol name, so this suite speaks
/// `Noise_IKpsk1_<curve>_<cipher>_<hash>` — a different string, and
/// therefore a different initial handshake hash, from the `IK` suite over
/// the same triple. The `psk` token puts **no bytes on the wire**: all
/// four of §2.3's sizes are identical to that suite's, and a packet
/// crossing between them dies at msg1's first AEAD open having spent 1 DH
/// (§2.2, §6.9).
#[macro_export]
macro_rules! channel_psk {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident < $curve:ty, $cipher:ty, $hash:ty > ;
    ) => {
        ::hiss::noise! {
            /// The IKpsk1 handshake for this suite (§2.2), carrying §5.2's
            /// 12-byte timestamp payload on msg1 and no payload on msg2.
            /// The trailing `psk` puts no bytes on the wire.
            $vis IKpsk1<$curve, $cipher, $hash> {
                <- s
                ...
                -> e, es, s, ss, psk [12]
                <- e, ee, se
            }
        }

        $crate::__channel_impl! {
            $(#[$meta])*
            $vis $name < $curve, $cipher, $hash >;
            kind          = psk;
            psk_ty        = ::hiss::psk::Psk;
            machine       = IKpsk1;
            initiator     = IKpsk1InitiatorMsg1;
            initiator_sent= IKpsk1InitiatorMsg2;
            responder     = IKpsk1ResponderMsg1;
            intro         = IKpsk1ResponderMsg1Intro;
            responder_read= IKpsk1ResponderMsg2;
        }
    };
}

/// The shared body of [`channel!`](crate::channel) and
/// [`channel_psk!`](crate::channel_psk). Not public API.
///
/// # Why the state type names are passed in
///
/// `hiss::noise!` names its per-state types `{Pattern}{Role}Msg{n}[Intro]`
/// (Appendix A), so this body needs `IKInitiatorMsg1` for one caller and
/// `IKpsk1InitiatorMsg1` for the other. `macro_rules` **cannot concatenate
/// identifiers** — there is no stable `concat_idents!` — so the five names
/// are spelled out at each call site rather than built from `machine`.
/// They are mechanical, and the `const _` assertions at the bottom of this
/// expansion fail the build if a caller ever spells one wrong.
#[doc(hidden)]
#[macro_export]
macro_rules! __channel_impl {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident < $curve:ty, $cipher:ty, $hash:ty >;
        kind          = $kind:ident;
        psk_ty        = $psk_ty:ty;
        machine       = $mach:ident;
        initiator     = $init:ident;
        initiator_sent= $init_sent:ident;
        responder     = $resp:ident;
        intro         = $intro:ident;
        responder_read= $resp_read:ident;
    ) => {
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
                        <$mach as ::hiss::noise::Pattern>::NAME,
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
            //
            // The formula is pattern-independent, and that is a fact about
            // the wire rather than an omission: a `psk` token mixes a key
            // and emits nothing, so IKpsk1's four sizes equal IK's over
            // the same triple. The `const _` pins below check that against
            // hiss's own computed sizes for whichever machine was stamped.
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
        // names are written down.
        //
        // Purely additive: no item of the `Channel` impl above moves, so
        // slice 1's frozen wire tests are untouched.
        impl $crate::packet::Handshake for $name {
            type Initiator<P: ::hiss::provider::DhProvider<$curve>> = $init<P>;
            type InitiatorSent<P: ::hiss::provider::DhProvider<$curve>> = $init_sent<P>;
            type Responder<P: ::hiss::provider::DhProvider<$curve>> = $resp<P>;
            type Msg1Intro<P: ::hiss::provider::DhProvider<$curve>> = $intro<P>;
            type ResponderRead<P: ::hiss::provider::DhProvider<$curve>> = $resp_read<P>;

            type Psk = $psk_ty;

            type Transport = ::hiss::noise::Transport<$mach>;
            type Seal = ::hiss::noise::DatagramSend<$mach>;
            type Open = ::hiss::noise::DatagramRecv<$mach>;

            fn initiator<P: ::hiss::provider::DhProvider<$curve>>(
                provider: P,
                prologue: &[u8],
                remote_static: <$curve as ::hiss::curve::Curve>::PublicKey,
            ) -> $init<P> {
                $mach::initiator(provider, prologue, remote_static)
            }

            fn write_msg1<P: ::hiss::provider::DhProvider<$curve>>(
                state: $init<P>,
                static_key: <P as ::hiss::provider::CryptoKeyProvider<$curve>>::PrivateKey,
                psk: &Self::Psk,
                payload: &[u8; $crate::constants::MSG1_PAYLOAD_LEN],
            ) -> ::core::result::Result<
                (::std::vec::Vec<u8>, $init_sent<P>),
                ::hiss::noise::HandshakeError,
            > {
                let (bytes, next) =
                    $crate::__channel_write_msg1!($kind, state, static_key, psk, payload)?;
                ::core::result::Result::Ok((bytes.to_vec(), next))
            }

            fn read_msg2<P: ::hiss::provider::DhProvider<$curve>>(
                state: $init_sent<P>,
                msg2: &[u8],
            ) -> ::core::result::Result<
                ::hiss::noise::Transport<$mach>,
                ::hiss::noise::HandshakeError,
            > {
                // hiss takes a fixed-size array; §3.1's length gate has
                // already guaranteed the size for anything that reaches
                // here, so this conversion cannot fail in practice — but
                // it is a `TryFrom`, and the honest error is the one hiss
                // would give for a truncated message.
                let exact: &[u8; $mach::MSG2_SIZE] = match ::core::convert::TryFrom::try_from(msg2) {
                    ::core::result::Result::Ok(exact) => exact,
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
            ) -> ::core::result::Result<$resp<P>, ::hiss::noise::HandshakeError> {
                $mach::responder(provider, prologue, static_key)
            }

            fn read_msg1_intro<P: ::hiss::provider::DhProvider<$curve>>(
                state: $resp<P>,
                msg1: &[u8],
            ) -> ::core::result::Result<
                (
                    <$curve as ::hiss::curve::Curve>::PublicKey,
                    $intro<P>,
                ),
                ::hiss::noise::HandshakeError,
            > {
                let exact: &[u8; $mach::MSG1_SIZE] = match ::core::convert::TryFrom::try_from(msg1) {
                    ::core::result::Result::Ok(exact) => exact,
                    ::core::result::Result::Err(_) => {
                        return ::core::result::Result::Err(
                            ::hiss::noise::HandshakeError::MessageTooShort,
                        );
                    }
                };
                state.read_message_1_intro(exact)
            }

            fn complete<P: ::hiss::provider::DhProvider<$curve>>(
                mid: $intro<P>,
                psk: &Self::Psk,
            ) -> ::core::result::Result<
                (
                    [u8; $crate::constants::MSG1_PAYLOAD_LEN],
                    $resp_read<P>,
                ),
                ::hiss::noise::HandshakeError,
            > {
                $crate::__channel_complete!($kind, mid, psk)
            }

            fn write_msg2<P: ::hiss::provider::DhProvider<$curve>>(
                state: $resp_read<P>,
            ) -> ::core::result::Result<
                (::std::vec::Vec<u8>, ::hiss::noise::Transport<$mach>),
                ::hiss::noise::HandshakeError,
            > {
                let (bytes, transport) = state.write_message_2()?;
                ::core::result::Result::Ok((bytes.to_vec(), transport))
            }

            fn into_datagram(
                transport: ::hiss::noise::Transport<$mach>,
                epoch_size: ::core::num::NonZeroU64,
            ) -> (
                ::hiss::noise::DatagramSend<$mach>,
                ::hiss::noise::DatagramRecv<$mach>,
            ) {
                transport.into_datagram_with_epoch(epoch_size)
            }

            fn next_counter(seal: &::hiss::noise::DatagramSend<$mach>) -> u64 {
                seal.next_counter()
            }

            fn session_id(
                seal: &::hiss::noise::DatagramSend<$mach>,
            ) -> &::hiss::noise::SessionId {
                seal.session_id()
            }

            fn seal(
                seal: &mut ::hiss::noise::DatagramSend<$mach>,
                ad: &[u8],
                plaintext: &[u8],
                out: &mut [u8],
            ) -> ::core::result::Result<(u64, usize), ::hiss::noise::HandshakeError> {
                seal.encrypt_next(ad, plaintext, out)
            }

            fn open(
                open: &mut ::hiss::noise::DatagramRecv<$mach>,
                counter: u64,
                ad: &[u8],
                ciphertext: &[u8],
                out: &mut [u8],
            ) -> ::core::result::Result<usize, ::hiss::noise::HandshakeError> {
                open.decrypt_at(counter, ad, ciphertext, out)
            }
        }

        // slither's §2.3 arithmetic against hiss's own computed sizes, for
        // EVERY suite either macro stamps. Three things ride on this pair:
        //
        //  * a hiss change to the point encoding or the tag size turns the
        //    BUILD red in every suite, not one golden test in one suite;
        //  * `MSG1_PAYLOAD_LEN` is spelled `[12]` in the token block above
        //    because `hiss::noise!` takes a literal there. This is what
        //    ties that literal to `constants::MSG1_PAYLOAD_LEN`: disagree,
        //    and `MSG1_LEN` no longer equals the machine's `MSG1_SIZE`;
        //  * on a `channel_psk!` suite it is also what pins "the `psk`
        //    token puts no bytes on the wire" — the derivation above has
        //    no psk term at all, so if hiss ever gave one a wire cost,
        //    this equality would fail.
        const _: () = assert!(
            <$name as $crate::packet::Channel>::MSG1_LEN == $mach::MSG1_SIZE,
            "slither's §2.3 MSG1_LEN disagrees with hiss's computed MSG1_SIZE"
        );
        const _: () = assert!(
            <$name as $crate::packet::Channel>::MSG2_LEN == $mach::MSG2_SIZE,
            "slither's §2.3 MSG2_LEN disagrees with hiss's computed MSG2_SIZE"
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

/// `write_message_1`'s per-pattern argument list. Not public API.
///
/// The `IK` arm drops `$psk` on the floor — it is `&()`. The `psk` arm
/// passes it **before** the payload, which is hiss's own order: the
/// generated parameter list is built per token, and a declared payload's
/// tail is appended last.
#[doc(hidden)]
#[macro_export]
macro_rules! __channel_write_msg1 {
    (no_psk, $state:expr, $key:expr, $psk:expr, $payload:expr) => {{
        // Not merely a discard: the annotation makes `kind = no_psk`
        // *mean* `Psk = ()` at compile time, so a caller that paired
        // `no_psk` with a real key type fails the build here rather than
        // silently ignoring a PSK it was handed.
        let _: &() = $psk;
        $state.write_message_1($key, $payload)
    }};
    (psk, $state:expr, $key:expr, $psk:expr, $payload:expr) => {
        $state.write_message_1($key, $psk, $payload)
    };
}

/// The mid-state's `complete()` per pattern. Not public API.
///
/// On `IKpsk1` this is where the pre-shared key is mixed — after the
/// claimed static is already in hand, which is the ordering the whole
/// pattern exists for.
#[doc(hidden)]
#[macro_export]
macro_rules! __channel_complete {
    (no_psk, $mid:expr, $psk:expr) => {{
        let _: &() = $psk;
        $mid.complete()
    }};
    (psk, $mid:expr, $psk:expr) => {
        $mid.complete($psk)
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

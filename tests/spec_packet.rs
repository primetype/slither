//! Public-surface conformance fence for `slither::packet`, plus slice 1's
//! share of story S22.
//!
//! An integration test is a separate crate — this is the only place that
//! can catch a `$crate` path bug inside `channel!`'s expansion, a missing
//! `pub`, or a macro-hygiene failure that a same-crate unit test cannot
//! see, because the reference suite already compiles from *inside* the
//! crate that defines the macro, which is a strictly weaker test than a
//! downstream consumer's use of it.
//!
//! **Authorship (CLAUDE.md working rule 6).** Written by an author who has
//! not read `src/packet/`'s implementation, from `SPEC.md` §2.2/§2.3/§3.1
//! and `.slices/01-packets/PLAN.md` §§2-3 and §9.2/§9.4 alone — the
//! declared public API surface (names and signatures), no bytes, no
//! offsets.
//!
//! **S22 (STORIES.md 347-353) — a user can pick a crypto suite, and
//! mismatches fail closed.** Slice 1 closes three of its four clauses
//! (PLAN.md §9.2); this file carries only the one clause an integration
//! test can actually reach:
//!
//! - "the suite is declared once via the macro" — `two_suites_coexist_in_one_crate`, below.
//!
//! The other two slice-1 clauses — "a peer on a different suite … fails
//! [at the length gate]" and "an unknown version byte is dropped
//! silently" — are pinned in `src/packet/tests.rs` instead
//! (`a_mismatched_suite_dies_at_the_length_gate`,
//! `unknown_version_is_dropped_silently`), because both need `classify`
//! and `Inbound`, which are `pub(crate)` (PLAN.md §2.2) and unreachable
//! from an integration test. The fourth clause — "a wrong static … fails
//! the handshake and installs nothing" — needs a driven handshake and is
//! slice 2's (PLAN.md §9.2).

use slither::packet::{Channel, suite::ReferenceSuite};

/// SPEC.md §2.2: "The reference suite is `P256 / ChaChaPoly / Blake2b`,
/// and its Noise protocol name — `Noise_IK_P256_ChaChaPoly_BLAKE2b` — is
/// pinned by test."
#[test]
fn reference_suite_protocol_name() {
    assert_eq!(
        <ReferenceSuite as Channel>::PROTOCOL_NAME,
        "Noise_IK_P256_ChaChaPoly_BLAKE2b"
    );
}

/// SPEC.md §2.3's reference-suite values (174 / 81 / 196 / 107 —
/// `PLAN.md` §4 names them by number), pinned both as literals and against
/// the `Channel` trait's own associated consts: a `channel!` expansion
/// whose arithmetic disagreed with `constants.rs` in a way that still
/// looked self-consistent from inside `src/packet/` would be caught here.
#[test]
fn reference_suite_sizes_are_the_spec_values() {
    assert_eq!(<ReferenceSuite as Channel>::MSG1_LEN, 174);
    assert_eq!(<ReferenceSuite as Channel>::MSG2_LEN, 81);
    assert_eq!(<ReferenceSuite as Channel>::INIT_PACKET_LEN, 196);
    assert_eq!(<ReferenceSuite as Channel>::RESP_PACKET_LEN, 107);

    assert_eq!(
        <ReferenceSuite as Channel>::MSG1_LEN,
        slither::constants::IK_MSG1_LEN
    );
    assert_eq!(
        <ReferenceSuite as Channel>::MSG2_LEN,
        slither::constants::IK_MSG2_LEN
    );
    assert_eq!(
        <ReferenceSuite as Channel>::INIT_PACKET_LEN,
        slither::constants::INIT_PACKET_LEN
    );
    assert_eq!(
        <ReferenceSuite as Channel>::RESP_PACKET_LEN,
        slither::constants::RESP_PACKET_LEN
    );
}

/// A second, genuinely different `slither::channel!` invocation — `Hash =
/// Sha256` rather than the reference suite's `Blake2b` — declared in a
/// module of its own (PLAN.md §3.5b: one `channel!` per *module*, not per
/// crate). Exercises the macro from outside slither's own crate, which a
/// same-crate unit test cannot: this is exactly the case §3.5a's
/// `::hiss::…` absolute-path requirement could break silently for a real
/// consumer while still working from inside slither itself.
mod second_suite {
    slither::channel! {
        pub Second<hiss::curve::p256::P256, hiss::noise::cipher::ChaChaPoly, hiss::noise::hash::Sha256>;
    }
}

/// S22: "the suite is declared once via the macro". Both suites compile,
/// coexist, and have distinct protocol names; the reference suite's name
/// is §2.2's stated pin.
#[test]
fn two_suites_coexist_in_one_crate() {
    use second_suite::Second;

    assert_ne!(
        <Second as Channel>::PROTOCOL_NAME,
        <ReferenceSuite as Channel>::PROTOCOL_NAME
    );
    assert_eq!(
        <ReferenceSuite as Channel>::PROTOCOL_NAME,
        "Noise_IK_P256_ChaChaPoly_BLAKE2b"
    );
}

/// A second suite over X25519 (`PK = 32`, not P-256's 65) — Q-O2, adopted
/// in `Cargo.toml`'s `[dev-dependencies]` (`hiss = { version = "0.3.2",
/// features = ["x25519-cryptoxide"] }`). `second_suite` above varies
/// `Hash`, which §2.3's formula does not touch; this one varies `Curve`,
/// which is what actually moves the four derived sizes — the only test in
/// the slice that runs §2.3's arithmetic against a second data point,
/// rather than trusting that it merely *compiles* for other suites.
mod third_suite {
    slither::channel! {
        pub Third<hiss::curve::x25519::X25519, hiss::noise::cipher::ChaChaPoly, hiss::noise::hash::Blake2b>;
    }
}

#[test]
fn a_second_curve_derives_different_sizes() {
    use third_suite::Third;

    // X25519's well-known 32-byte public key, confirmed against the
    // suite's own reported constant rather than assumed.
    assert_eq!(<Third as Channel>::STATIC_PUBLIC_LEN, 32);
    assert_ne!(
        <Third as Channel>::STATIC_PUBLIC_LEN,
        <ReferenceSuite as Channel>::STATIC_PUBLIC_LEN
    );

    // Every derived size differs from the reference suite's...
    assert_ne!(
        <Third as Channel>::MSG1_LEN,
        <ReferenceSuite as Channel>::MSG1_LEN
    );
    assert_ne!(
        <Third as Channel>::MSG2_LEN,
        <ReferenceSuite as Channel>::MSG2_LEN
    );
    assert_ne!(
        <Third as Channel>::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_ne!(
        <Third as Channel>::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );

    // ...and follows §2.3's formula, computed from THIS suite's own PK/TAG
    // (not the reference suite's literals):
    //   MSG1_LEN = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)
    //   MSG2_LEN = PK + TAG
    //   INIT_PACKET_LEN = INIT_HEADER_LEN + MSG1_LEN + MAC1_LEN
    //   RESP_PACKET_LEN = RESP_HEADER_LEN + MSG2_LEN + MAC1_LEN
    let pk = <Third as Channel>::STATIC_PUBLIC_LEN;
    let tag = <Third as Channel>::AEAD_TAG_LEN;
    assert_eq!(
        tag,
        slither::constants::AEAD_TAG_LEN,
        "TAG is suite-independent per §2.3"
    );

    let expected_msg1 = pk + (pk + tag) + (slither::constants::MSG1_PAYLOAD_LEN + tag);
    let expected_msg2 = pk + tag;
    let expected_init =
        slither::constants::INIT_HEADER_LEN + expected_msg1 + slither::constants::MAC1_LEN;
    let expected_resp =
        slither::constants::RESP_HEADER_LEN + expected_msg2 + slither::constants::MAC1_LEN;

    assert_eq!(<Third as Channel>::MSG1_LEN, expected_msg1);
    assert_eq!(<Third as Channel>::MSG2_LEN, expected_msg2);
    assert_eq!(<Third as Channel>::INIT_PACKET_LEN, expected_init);
    assert_eq!(<Third as Channel>::RESP_PACKET_LEN, expected_resp);

    // Concretely, for X25519 / ChaCha20-Poly1305: 108 / 48 / 130 / 74 — the
    // same 130 that `src/packet/tests.rs`'s
    // `a_mismatched_suite_dies_at_the_length_gate` computes by hand.
    assert_eq!(<Third as Channel>::MSG1_LEN, 108);
    assert_eq!(<Third as Channel>::MSG2_LEN, 48);
    assert_eq!(<Third as Channel>::INIT_PACKET_LEN, 130);
    assert_eq!(<Third as Channel>::RESP_PACKET_LEN, 74);
}

/// The offered same-curve sibling suite — `P256 / AesGcm / Blake2b`,
/// ruling 279 — declared exactly as a consumer would declare it, in a
/// module of its own like the other non-reference suites above.
mod aes_suite {
    slither::channel! {
        pub OfferedAes<hiss::curve::p256::P256, hiss::noise::cipher::AesGcm, hiss::noise::hash::Blake2b>;
    }
}

/// Ruling 279's pins for the offered suite: the protocol name, and the
/// fact that every §2.3 size **equals** the reference suite's — `PK` = 65
/// is shared and `TAG` = 16 on every suite (ruling 68), so nothing is
/// left to differ. The equality is the factual basis of §2.2 as amended:
/// a same-curve sibling's packet *cannot* die at the length gate, because
/// there is no length to distinguish it by — it dies at the first AEAD
/// open instead. The broken build this catches: a `channel!` expansion
/// (or a hiss `AesGcm`) whose tag or point arithmetic drifted from the
/// reference suite's would silently reopen the length gate and falsify
/// the amended claim.
#[test]
fn the_offered_aes_suite_is_pinned_and_length_identical() {
    use aes_suite::OfferedAes;

    assert_eq!(
        <OfferedAes as Channel>::PROTOCOL_NAME,
        "Noise_IK_P256_AESGCM_BLAKE2b"
    );
    assert_ne!(
        <OfferedAes as Channel>::PROTOCOL_NAME,
        <ReferenceSuite as Channel>::PROTOCOL_NAME
    );

    // The same-curve sibling property, both as literals (independent of
    // the reference pins above) and as equalities (the §2.2 basis).
    assert_eq!(<OfferedAes as Channel>::STATIC_PUBLIC_LEN, 65);
    assert_eq!(<OfferedAes as Channel>::AEAD_TAG_LEN, 16);
    assert_eq!(<OfferedAes as Channel>::MSG1_LEN, 174);
    assert_eq!(<OfferedAes as Channel>::MSG2_LEN, 81);
    assert_eq!(<OfferedAes as Channel>::INIT_PACKET_LEN, 196);
    assert_eq!(<OfferedAes as Channel>::RESP_PACKET_LEN, 107);

    assert_eq!(
        <OfferedAes as Channel>::MSG1_LEN,
        <ReferenceSuite as Channel>::MSG1_LEN
    );
    assert_eq!(
        <OfferedAes as Channel>::MSG2_LEN,
        <ReferenceSuite as Channel>::MSG2_LEN
    );
    assert_eq!(
        <OfferedAes as Channel>::INIT_PACKET_LEN,
        <ReferenceSuite as Channel>::INIT_PACKET_LEN
    );
    assert_eq!(
        <OfferedAes as Channel>::RESP_PACKET_LEN,
        <ReferenceSuite as Channel>::RESP_PACKET_LEN
    );
}

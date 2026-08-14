# Slice 1 — implementation notes (author A, the implementer)

Written incrementally per CLAUDE.md working rule 2. This file is the
implementer's record; it is not a report of gates until the gates are run.

## Standing constraints accepted from the brief

- Q-O1..Q-O7 all **accepted** as the plan recommends.
- Rulings 64 (LE header), 65 (exact handshake length gate), 66 (plain
  BLAKE2b, both invocations), 67 (the gate emits nothing) are binding and
  supersede the plan where they differ (§6.4's Q-M1 is now ruled; §6.5's
  Q-M2 is now ruled).
- **I ship no committed test.** `src/packet/tests.rs` and
  `src/packet/golden_vectors.rs` are stubs; `tests/spec_packet.rs` is not
  mine to write. `tests/spec_constants.rs` / `tests/spec_errors.rs` are
  untouched.

## Spec ranges read (never a bare Read of SPEC.md)

| Range | Section |
|---|---|
| 445–530 | §2.1–2.4 |
| 530–684 | §3.1–3.5 (incl. rulings 64 and 65 text) |
| 684–756 | §4.1–4.4 (incl. ruling 66 text) |
| 757–806 | §5.1–5.3 |
| 4866–4900 | §17.3 |

## Verified spec facts that drive the code

- §3.1 gate table (ruling 65): INIT exactly 196, RESP exactly 107, DATA
  `30 ..= 1200`. Everything else: silent drop.
- §4.1 (ruling 66): key = plain BLAKE2b-256(MAC1_LABEL ‖ canonical
  static), 77-byte preimage on the reference suite; tag = plain keyed
  BLAKE2b-128 over `[0, 180)` (Init) / `[0, 91)` (Resp). Data: no mac1.
- §3.2/3.3/3.4 layouts: Init `type‖version‖sender_index`; Resp
  `type‖version‖sender_index‖receiver_index`; Data
  `type‖version‖receiver_index‖counter`.
- §5.2 payload: `ts_secs(8, BE) ‖ ts_nanos(4, BE)` — big-endian, and
  §3.1's ruling-64 text now names it as exclusion 3 of three (so the
  plan's Q-M3 is already discharged in the spec text I read).
- §17.3: index minting is not slice 1's; headers enforce nothing.

## Progress log

- [x] Read plan in full, CLAUDE.md, last four rulings, spec ranges.
- [x] Verified the three "trust them" facts against the vendored sources
      myself rather than taking them on trust:
      `packtool-0.6.0/src/primitives.rs` lines 57/63 → `to_le_bytes` /
      `from_le_bytes` (ruling 64's premise);
      `packtool-macro-0.6.0/src/expand.rs` → `#[packed(value = …)]` takes
      a `syn::Lit`; `cryptoxide-0.6.2/src/hashing/blake2b.rs` lines
      367–370 → `context_finalize!` instantiated for 224/256/384/512
      only, so `Context<128>` has `finalize_at` alone. Also confirmed
      cryptoxide's BLAKE2b has **no** salt/personalisation surface at
      all, so ruling 66 holds by construction.
- [x] Wrote `suite.rs`, `header.rs`, `mac.rs`, `payload.rs`, `mod.rs`;
      stubbed `tests.rs` and `golden_vectors.rs`; wired `lib.rs` and
      added the `hiss` dev-dependency (Q-O2).
- [x] Threw away verification code (`src/packet/scratch_verify.rs` plus a
      temporary `mod` line) after running it. Deleted; `grep -rn scratch
      src/` is empty. What it checked, and against what:
      * **mac1** against **Python `hashlib.blake2b`** — an implementation
        with no shared code with cryptoxide. `key = blake2b(b"slither
        mac1" ‖ static, digest_size=32)` over a 65-byte 0x04-led static
        (77-byte preimage) and `blake2b(preimage, digest_size=16,
        key=key)` over 180-byte and 91-byte preimages. Both digests
        matched byte for byte, which is ruling 66 (plain, no salt, no
        personalisation) confirmed out of band.
      * **header bytes**: `InitHeader::new(0x11223344)` →
        `010144332211`; `RespHeader::new(0x11223344, 0x55667788)` →
        `02014433221188776655`; `DataHeader::new(0xaabbccdd,
        0x0102030405060708)` → `0301ddccbbaa0807060504030201`. Each
        multi-byte field is byte-reversed relative to its value (ruling
        64), the Resp field order is sender-then-receiver, and the Data
        counter uses all eight bytes.
      * **payload**: `(0x0102030405060708, 0x090a0b0c)` →
        `0102030405060708090a0b0c` — big-endian, and `Ord` orders
        `(1,0) < (1,1) < (2,0)`.
      * **gate**: 196/107/30 accepted; 195, 197, 106, 108, 29 dropped
        (ruling 65's exactness); `MAX_DATAGRAM` accepted and
        `MAX_DATAGRAM + 1` dropped; all of `0x00`, `0x04`, `0x05`,
        `0x06`, `0xff` dropped; all 255 non-`VERSION` bytes dropped;
        empty and 1-byte inputs dropped; `ad`, `preimage`, `msg1`,
        `msg2`, `ciphertext` each checked by **pointer**, not only by
        equality, to be subslices of the input at the right offset.
      * **suite genericity**: a second suite over X25519/Sha256 in its
        own module derived `Noise_IK_25519_ChaChaPoly_SHA256` and
        108 / 48 / 130 / 74 from the same formula — different from the
        reference suite's 174 / 81 / 196 / 107, and self-consistent. It
        also confirmed mac1 does not follow the suite hash.
      * **`PROTOCOL_NAME`**: `Noise_IK_P256_ChaChaPoly_BLAKE2b`.

## Deviations from the plan, and why

1. **`Channel::PROTOCOL_NAME` stays a `const &'static str`** (the plan's
   declared API, which the independent test author is writing against),
   but it could not be a literal: hiss builds the name at run time with
   `format!` and the component `NAME`s are associated consts, not
   literals. `channel!` therefore builds it in a `const` block from
   `<IK as Pattern>::NAME` and the three suite `NAME`s, through a
   `#[doc(hidden)] pub const fn` in `packet::suite`. Derived, not
   spelled: the string seeds the initial handshake hash, so a literal
   that drifted from hiss's would be a silent interop break.
2. **`Curve::PublicKey: AsRef<[u8]>` is an associated-type bound
   (`type Curve: DhCurve<PublicKey: AsRef<[u8]>>`), not the plan's
   trait-level `where` clause.** Identical requirement, §2.4's; the
   `where` form would have to be repeated at every downstream
   `C: Channel` signature.
3. **The four §2.3 sizes are *defined* by the derivation rather than
   written as literals and asserted against it.** Strictly stronger: no
   suite can be declared whose sizes disagree with the formula, and the
   two independent checks that remain are `MSG1_LEN == IK::MSG1_SIZE`
   (hiss's own arithmetic, every suite) and the reference suite against
   `constants.rs`'s 174 / 81 / 196 / 107.
4. **`Msg1Payload` gained `new()` and `pub(crate)` fields.** The plan's
   sketch declared only `encode`/`decode`, which leaves the type
   unconstructible from `packet::tests` (a sibling module cannot reach
   another module's private fields) and from slice 2. Both spellings now
   work, so neither the test author nor slice 2 can guess wrong.
5. **`#![allow(dead_code)]` on `packet`**, with the reason at the site:
   slice 1 has no consumer, and `-D warnings` is a release gate.

## Slice-0 hand-forwards closed here

- **R10 — is `hiss::noise::datagram::MAX_EPOCH_JUMP` const-reachable
  under `default-features = false`?** **Yes.** `constants.rs`'s
  `const _: () = assert!(MAX_EPOCH_JUMP == hiss::noise::datagram::MAX_EPOCH_JUMP)`
  compiles under `cargo build --no-default-features`, which is gate 1b
  and is green. The redundant runtime mirror in `constants::tests` can
  stay as the belt it was written to be.
- **`STATIC_PUBLIC_LEN` re-pinned against the `noise!`-declared
  channel**, in `suite.rs`, as slice 0 asked. Slice 0's direct
  `P256::PUBLIC_KEY_SIZE` assertion stays.

## Hand-forward to slice 2 (new, beyond the plan's list)

- **`hiss-macros` resolves to 0.3.1, not the 0.3.0 the plan read**, and
  0.3.1 adds exactly the thing §6.2's staged accept needs: for a first
  message ending `…, s, ss` (IK's shape) the generated state machine
  gains a **staged read** — `read_message_1_intro` stops after the
  revealed static, having paid only the DH up to that point, and
  suspends into an owned mid-state whose `complete()` pays the rest.
  That is §6.1–6.2's one-DH/two-DH split, already generated. Slice 2
  should design `Channel`'s handshake extension around it rather than
  around the synchronous `read_message_1_with`. Size and protocol-name
  generation are unchanged between 0.3.0 and 0.3.1 (verified).
- The `channel!` expansion is where a suite's `IK` lives; it is `pub` at
  `slither::packet::suite::IK` for the reference suite, and slice 2 will
  need to reach it through the `Channel` impl rather than by name if the
  endpoint is to stay generic. Nothing in slice 1 forces a shape here.

## Gate results (all run individually; see the handover report)

| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | clean, exit 0 |
| `cargo build --no-default-features` | clean, exit 0 |
| `cargo fmt --all --check` | no diff, exit 0 |
| `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings, exit 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | exit 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | exit 0 |
| `cargo test` | 35 + 103 + 11 + 4 doctests, all pass |
| `cargo test --all-features` | same, all pass |
| `cargo +1.96 check --all-features --all-targets` | exit 0 |
| `cargo deny check` | advisories/bans/licenses/sources ok |

**Gate 6 (wire pins) has no golden vectors yet, by design.** The 103
size/constant assertions in `tests/spec_constants.rs` pass and the
compile-time assertions in `constants.rs`, `header.rs`, `payload.rs`,
`mac.rs` and the `channel!` expansion all hold — but
`src/packet/golden_vectors.rs` is a stub and `src/packet/tests.rs` is a
stub, so **no golden vector has been checked against this code.** That
reconciliation is §8.5's, after the independent derivation and the
independent harness land.
</content>

# Slice 1 — "Packets & the gate" — implementation plan

> **Status: DRAFT 2026/08/14 — awaiting orchestrator answers and two
> maintainer rulings.** Written against `SPEC.md` (ratified 2026/08/14,
> **65 entries — ruling 64 landed today**), `PLAN.md` (approved
> 2026/08/14), `STORIES.md` (30 approved stories), `CLAUDE.md`, and the
> committed slice-0 tree.
>
> **The spec is the authority.** Where this plan and `SPEC.md` disagree,
> the spec wins and this plan is wrong.
>
> **Ruling 64 is the shape of this slice.** The packet header is
> **little-endian**. That reaches exactly three fields —
> `sender_index`, `receiver_index`, `counter` — and it is the reason the
> headers can be `#[derive(Packed)]` structs with plain `u32`/`u64`
> fields instead of `[u8; N]` arrays with hand conversions. §8.1's
> varints and §2.4's canonical static comparison are untouched. **So is
> §5.2's msg1 timestamp payload, which is big-endian and is not a header
> field — see §7, which exists because that is the trap this slice can
> most plausibly fall into.**

## Sources consulted (and deliberately not consulted)

Read narrowly, per `CLAUDE.md` working rule 1. Every range below was read
with `Read` + `offset`/`limit`; `SPEC.md` was never read whole.

| Range | What |
|---|---|
| `SPEC.md` 445–527 | §2 — the hiss contract, `channel!`, per-suite derived sizes, the canonical static encoding |
| `SPEC.md` 528–645 | §3 — packet types, **ruling 64's little-endian rule**, the three headers, sizes and caps |
| `SPEC.md` 646–700 | §4 — mac1 construction, verification order, what it is not, suite genericity |
| `SPEC.md` 701–750 | §5.1–5.3 — prologue, handshake payloads, the initiation timestamp |
| `SPEC.md` 4810–4824 | §17.3 — the random nonzero `u32` session index |
| `SPEC.md` 412–432 | §1.3 — ratification discipline, and *what* the first golden freeze covers |
| `SPEC.md` 4998–5025 | §18.2 — the five trace targets (checked for a gate target; there is none — §6.5) |
| `SPEC.md` 5195–5215 | Appendix B — the wire-pin obligations |
| `SPEC.md` 970–980, 1455–1470 | §6.1's cost ladder row and §6.6's silent-drop wording — the two places outside §3/§4 that constrain the gate |
| `.spec-v2-clean-slate/rulings.md` | **ruling 64 only** (`tail -60`), per the brief |
| `PLAN.md` §1 (module map), §4 (slice table), §5 (review gates) | scope fence |
| `STORIES.md` 347–353 | S22 |
| `.slices/00-ground/PLAN.md` | structure, conventions, and its hand-forward notes to this slice |
| `src/{lib,constants}.rs`, `Cargo.toml`, `tests/spec_constants.rs` | the committed foundation |

**Third-party sources read to verify mechanism rather than trust memory**
(each named because a wrong assumption here is a wire bug):

| Source | What it settled |
|---|---|
| `packtool-0.6.0/src/primitives.rs` | integers pack via `to_le_bytes` / `from_le_bytes` — **ruling 64's premise verified, not assumed** |
| `packtool-0.6.0/src/lib.rs`, `view.rs`, `packet.rs` | the `Packed` trait (`SIZE`, `check`, `unchecked_*`), `View::try_from_slice`, `Packet::pack` |
| `packtool-macro-0.6.0/src/expand.rs` | `#[packed(value = …)]` takes a **`syn::Lit` — a literal only, never a const path** (§4.2) |
| `hiss-0.3.2/src/noise/mod.rs`, `curve/mod.rs`, `noise/cipher.rs` | `Protocol`, `Curve::{NAME, PUBLIC_KEY_SIZE, PublicKey}`, `Cipher::TAG_SIZE`; **`Curve::PublicKey` is bounded by `Clone` only**, which is why §2.4's slither-side `AsRef<[u8]>` clause is load-bearing |
| `hiss-0.3.2/src/noise/pattern.rs`, `hiss-macros-0.3.0/src/{parse,codegen}.rs` | the IK token block, the `[N]` payload suffix syntax, and that the generated sizes are **associated consts `IK::MSG1_SIZE` / `IK::MSG2_SIZE`** |
| `hiss-macros-0.3.0/src/codegen.rs` | the generated code emits **absolute `::hiss::…` paths** — the constraint in §3.5 |
| `hiss-0.3.2/Cargo.toml` | the only feature is `x25519-cryptoxide` (default); slither's `default-features = false` therefore leaves **P-256 as the only curve** — §3.6 |
| `cryptoxide-0.6.2/src/hashing/blake2b.rs` | `Blake2b::<BITS>::new()` / `new_keyed(key)` → `Context<BITS>`, `update`; and that **`finalize()` exists for 224/256/384/512 only**, so mac1's 128-bit output must use `finalize_at` (§5.2) |

**Not consulted:** the v0.1 `src/` bodies, `SPEC-v0.1-wire-historical.md`,
and git `5324ce5`. Slice 1 is the wire, and the v0.1 wire is superseded —
this is the one slice where reading the old implementation is actively
dangerous, because the v0.1 header was big-endian, its mac1 keyed a
33-byte compressed P-256 point (§4.4), and both look plausible.

**Not consulted, and deliberately so: the parallel golden-vector
derivation.** See §8.

---

## 1. Scope, and what slice 1 is not

Slice 1 is where **bytes acquire meaning**. It delivers the suite
declaration (§2), the three packet headers (§3.2–3.4), the pre-AEAD
classify/drop gate (§3.1), mac1 (§4), the msg1 payload codec (§5.2), and
the first golden freeze of all of it.

**Nothing in slice 1 drives a handshake, holds a key, opens a session, or
reads a clock.** No `hiss` state machine is stepped, no DH is performed,
no index is minted, no timestamp guard is consulted, no frame is parsed.
If a file in this slice needs to know what a *connection* is, the scope
fence has been breached.

The three boundaries most likely to be crossed by accident, each stated
as a rule:

1. **§5.2's payload *encoding* is slice 1's; §5.3's monotonic forcing and
   §17.1's guard are slice 2's.** The reason is Appendix B: the first
   golden freeze covers "the prologue, the 12-byte msg1 payload, mac1's
   canonical keying", so slice 1 must own an encoder for the payload or
   the vector is unenforceable. It owns the codec and nothing else — no
   wall-clock read, no `>` comparison, no orphan cap.
2. **§17.3's index is a *rule*, not a value, and minting it is slice
   2's.** Slice 1's headers carry `sender_index` / `receiver_index` as
   plain `u32` fields and enforce **nothing** about them — not
   nonzero-ness, not uniqueness. A header struct that rejects index 0
   has taken §17.3's job and put a table invariant in the packet layer.
3. **`Channel` in slice 1 is the *suite*, not the handshake seam.** See
   §3.3.

### Deliverables

| # | File | New? | Author |
|---|---|---|---|
| 1 | `src/packet/mod.rs` | new | implementer |
| 2 | `src/packet/suite.rs` | new — **declared module-map deviation, §2.2** | implementer |
| 3 | `src/packet/header.rs` | new | implementer |
| 4 | `src/packet/mac.rs` | new | implementer |
| 5 | `src/packet/payload.rs` | new — **declared module-map deviation, §2.2** | implementer |
| 6 | `src/packet/golden_vectors.rs` | new, `#[cfg(test)]`, **data only** | **the independent derivation agent** (§8) |
| 7 | `src/packet/tests.rs` | new, `#[cfg(test)]` | **an author who has not read the implementation** (§9.1) |
| 8 | `tests/spec_packet.rs` | new | **an author who has not read the implementation** (§9.1) |
| 9 | `src/lib.rs` | edited | implementer — two lines (§2.1) |
| 10 | `Cargo.toml` | edited | implementer — one dev-dependency line, **only if Q-O2 is answered yes** |

`tests/spec_constants.rs` and `tests/spec_errors.rs` are **not touched**.
Slice 1 adds constants to nothing and errors to nothing; if an
implementer finds themselves editing either, that is a finding to report,
not an edit to make.

### Definition of done

All eight `CLAUDE.md` gates green on the slice-1 commit, **each run and
its output pasted** (working rule 7; the commands are §14), plus:

- the golden-wire harness green against the **independently derived**
  vectors, with the reconciliation protocol of §8.4 followed if it is
  not;
- S22's slice-1 half passing as a test (§9.2);
- gate 6 ("wire pins") reported with its **slice-1 meaning**, which is
  the first slice in which it means what it says: the golden vectors
  exist from here on, and a red one is a ruling request.

---

## 2. Module layout

### 2.1 The tree

```
src/
  lib.rs                   + pub mod packet;
                           + pub use packet::{Channel, channel};   ← see below
  packet/
    mod.rs        §3.1 the classify/drop gate, §3.5 the caps, the module doc
    suite.rs      §2   the `Channel` trait, the `channel!` macro, the reference suite
    header.rs     §3.2–3.4 InitHeader / RespHeader / DataHeader (packtool)
    mac.rs        §4   Mac1Key — derivation, tag, verify
    payload.rs    §5.2 the 12-byte msg1 payload codec
    golden_vectors.rs   #[cfg(test)] — data only, independently derived (§8)
    tests.rs            #[cfg(test)] — every internal-surface test (§9)
tests/
  spec_packet.rs          the public-surface conformance fence + S22 (§9.2)
```

`lib.rs` gains exactly:

```rust
pub mod packet;

pub use packet::Channel;
```

`channel!` is `#[macro_export]`, so it lands at the crate root
(`slither::channel!`) regardless of which file defines it; no `pub use`
is needed or wanted for it. `Channel` is re-exported because it appears
in §16.2's public signature `Endpoint<C: Channel>` — the same reason the
ten error types are re-exported and `constants` is not.

### 2.2 Visibility, item by item

| Item | Visibility | Why |
|---|---|---|
| `mod packet` | `pub` | `channel!` expands in the **caller's** crate, so every path its expansion names must be publicly reachable. This is forced, not chosen |
| `packet::Channel` | `pub` | §2.2/§16.2: `Endpoint<C: Channel>` |
| `channel!` | `#[macro_export]` | S22's surface |
| `packet::ReferenceSuite` (name: **Q-O1**) | `pub` | §2.2's reference suite, stamped by `channel!` itself |
| `packet::suite` (module) | `pub` | it holds the reference suite's generated `IK`; see §3.5 on naming |
| `packet::header::{InitHeader, RespHeader, DataHeader}` | `pub(crate)` | no §16 surface exposes a header. Start closed; widening later is not a breaking change, narrowing is |
| `packet::mac::Mac1Key` | `pub(crate)` | ditto |
| `packet::payload::Msg1Payload` | `pub(crate)` | ditto |
| `packet::{classify, Inbound}` | `pub(crate)` | the gate is core-internal |

**Two declared module-map deviations.** `PLAN.md` §1 maps §2/§3/§4 onto
`packet/{mod,header,mac}.rs` — three files for four spec sections plus a
gate. Adding `suite.rs` and `payload.rs` keeps the map's actual
principle ("deliberately one-to-one, because a reviewer must be able to
find the code for a section without searching") rather than its literal
file list. Alternatives, so the orchestrator can overrule cheaply
(**Q-O1**):

- `suite.rs` → `mod.rs`. Costs: `mod.rs` then carries §2 + §3.1 + §3.5,
  and — the substantive reason — the reference suite's `channel!`
  invocation generates a type named `IK` into whichever module invokes
  it (§3.5), so `mod.rs` would acquire `slither::packet::IK`.
- `payload.rs` → `mod.rs` or → `suite.rs`. Cheaper, but §5.2 is the one
  place in the crate where a big-endian integer sits next to a
  little-endian one, and giving it its own file with its own header
  comment is the cheapest available guard (§7).

---

## 3. §2 — the suite: `Channel` and `channel!` (the S22 surface)

### 3.1 What the spec fixes

§2.2, quoted, because every design choice below is downstream of it:

> `channel!` (a `macro_rules` macro — no proc-macro) stamps the
> `hiss::noise!` IK invocation with a caller-chosen `<Curve, Cipher,
> Hash>` triple plus the `Channel`/`Protocol` implementation. The IK
> token block and the handshake payload declaration (`[12]` on msg1;
> msg2 declares no payload — §5.2) are hardcoded in the macro; **IK is
> the only pattern**. […] **There is no suite identifier on the wire.**
> Endpoints are monomorphic per suite […] A mismatched-suite packet dies
> silently at the length gate or at mac1 — the same fate as garbage. The
> version byte does not encode the suite.

Three consequences that are easy to lose:

1. **No negotiation, no suite byte, no runtime suite dispatch.** The
   gate never asks "which suite is this"; it asks "is this the length my
   suite says it is". A `Suite` enum, a suite registry, or a
   `dyn Channel` would each be a wire-visible invention.
2. **`IK` is the only pattern**, hardcoded. `channel!` takes no pattern
   parameter. The generated Noise name must be `Noise_IK_…`, which
   (verified in `hiss-macros`) means the generated *type* must literally
   be named `IK`, because `hiss::noise!` uses the declared identifier as
   `Pattern::NAME` and that string seeds the initial handshake hash.
   **A misnamed type is a silent interop break with a green test suite.**
3. **A mismatched suite dies as garbage** — so nothing in slice 1 may
   produce a distinguishable error for it. It is a length-gate drop or a
   mac1 drop, both silent.

### 3.2 What varies, and what the wire sees

Per §2.3, with `PK = Curve::PUBLIC_KEY_SIZE` and `TAG = Cipher::TAG_SIZE`:

```
MSG1_LEN        = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)
MSG2_LEN        = PK + TAG
INIT_PACKET_LEN = INIT_HEADER_LEN + MSG1_LEN + MAC1_LEN
RESP_PACKET_LEN = RESP_HEADER_LEN + MSG2_LEN + MAC1_LEN
```

Everything else — the three headers, `MAX_DATAGRAM`, `MAX_PLAINTEXT`,
mac1's label and length, the whole frame layer — is **suite-independent**.
mac1 in particular is fixed keyed-BLAKE2b for every suite (§4.4,
ratified), so it does **not** follow `Channel::Hash`.

### 3.3 The `Channel` trait — suite facts only, and why not more

```rust
/// A crypto suite, declared once by [`channel!`]. §2.2.
///
/// The associated `where` clause is §2.4's canonical-encoding
/// requirement, expressed slither-side because `hiss::curve::Curve`
/// bounds `PublicKey` by `Clone` alone. `Ord` is deliberately **not**
/// required (Appendix A.3).
pub trait Channel
where
    <Self::Curve as hiss::curve::Curve>::PublicKey: AsRef<[u8]>,
{
    type Curve:  hiss::curve::DhCurve;
    type Cipher: hiss::noise::Cipher;
    type Hash:   hiss::noise::Hash;

    /// `Noise_IK_<curve>_<cipher>_<hash>`. §2.2 pins the reference
    /// suite's value by test.
    const PROTOCOL_NAME: &'static str;

    /// `Curve::PUBLIC_KEY_SIZE` — the canonical static encoding's
    /// length (§2.4).
    const STATIC_PUBLIC_LEN: usize;
    /// `Cipher::TAG_SIZE`.
    const AEAD_TAG_LEN: usize;

    /// The four §2.3 derived sizes.
    const MSG1_LEN: usize;
    const MSG2_LEN: usize;
    const INIT_PACKET_LEN: usize;
    const RESP_PACKET_LEN: usize;
}
```

**`Channel` carries no handshake surface in slice 1, and that is a
decision, not an omission.** `hiss::noise!` generates a state machine
whose transitions are *inherent* methods across a family of per-state
types (`IKInitiatorMsg2`, …), and abstracting that behind a trait is the
design work of §5 + §6.1–6.2's staged ladder — `read_message_1_with`'s
identity hook, the one-DH/two-DH split, the fresh-ephemeral retransmit.
Designing that seam blind, one slice early, is how a wrong abstraction
gets frozen into a public trait. Slice 2 extends `Channel` (or adds a
companion trait `Handshake`) with that surface; nothing in v0.2 is
released before slice 9, so the extension costs nothing.

**Recorded so slice 2 does not rediscover it:** the extension must not
require `Send` anywhere (`CLAUDE.md`; S21), and the DH provider is a
parameter of the *endpoint*, not of the suite.

### 3.4 What `channel!` expands to

Sketch, not final syntax:

```rust
#[macro_export]
macro_rules! channel {
    ($(#[$meta:meta])* $vis:vis $name:ident<$curve:ty, $cipher:ty, $hash:ty>;) => {
        ::hiss::noise! {
            /// The IK handshake for this suite. §2.2 — IK is the only pattern.
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

        impl $crate::packet::Channel for $name { /* the eight items, all derived */ }

        // §2.3, executed — emitted for EVERY suite the macro stamps:
        const _: () = assert!(<$name as $crate::packet::Channel>::MSG1_LEN == IK::MSG1_SIZE);
        const _: () = assert!(<$name as $crate::packet::Channel>::MSG2_LEN == IK::MSG2_SIZE);
        // … plus the four derivation asserts and the two MAX_DATAGRAM fits (§3.7)
    };
}
```

Four things this shape buys, each stated because each is a place a
simpler shape loses something:

- **The `[12]` payload is inside the macro**, so no caller can declare an
  IK without it. §5.2's payload is wire, not policy.
- **`IK::MSG1_SIZE` is hiss's own computed size.** Asserting slither's
  §2.3 arithmetic against it means a hiss change to the point encoding
  or the tag size turns the **build** red in every suite, not one golden
  test in one suite.
- **The asserts ride in the expansion**, so a *consumer's* suite is
  checked by the same arithmetic as the reference suite. §2.3 stops
  being a table and becomes an executed derivation.
- **`$vis` is threaded to the generated `IK`**, so a consumer who wants a
  private suite gets one.

### 3.5 The two constraints on `channel!` that must be documented, not discovered

**(a) The expansion names `::hiss::…`, so the calling crate must depend
on `hiss`.** Verified in `hiss-macros-0.3.0/src/codegen.rs`: the
generated code uses absolute `::hiss::provider::…` / `::hiss::curve::…`
paths. A `macro_rules` wrapper cannot rewrite them. This is not a defect
— a consumer already names `P256`, `ChaChaPoly` and `Blake2b`, which are
hiss types — but it means:

> **`slither::channel!` requires `hiss` in the consumer's
> `Cargo.toml`, on the same minor line slither uses.**

That belongs in the `channel!` rustdoc with a copy-pasteable
`Cargo.toml` snippet, and it is a candidate sixth documentation
obligation (**Q-M2**). Recommend additionally `pub use hiss;` from
slither so the version that must match is *findable*, while stating
plainly that re-exporting does not remove the direct-dependency
requirement.

**(b) One `channel!` per module.** The generated type is named `IK` by
necessity (§3.1 point 2), so two invocations in one module collide. A
`macro_rules` macro cannot synthesise an identifier without a
`paste`-like dependency, which slither will not add. Recommend:
**document the restriction, and let the collision be the enforcement** —
the error names `IK` and is comprehensible. The alternative is an
explicit module parameter in the invocation syntax
(`channel! { pub Foo in mod foo { … } }`), which is uglier at every call
site to serve a case (two suites in one crate) that §2.2 says is
foreclosed on one socket anyway. **Q-O1** records the choice; §9.2's
S22 test proves that two suites in *separate* modules do coexist.

### 3.6 The reference suite, and the second-suite test

slither stamps its own reference suite with `channel!` — dogfooding, and
the only way §2.2's "declared once via the macro" is true of slither
itself:

```rust
// src/packet/suite.rs
crate::channel! {
    /// The reference suite (§2.2): `Noise_IK_P256_ChaChaPoly_BLAKE2b`.
    pub ReferenceSuite<P256, ChaChaPoly, Blake2b>;
}
```

(`crate::channel!` path invocation, because `#[macro_export]` macros are
only textually in scope after their definition point.)

**A problem worth naming: under `default-features = false`, hiss offers
exactly one curve.** Its only feature is `x25519-cryptoxide`, which is a
default slither disables. So a second suite declared in-crate can vary
only the `Hash` (`Sha256`, `Blake2s`, `Sha512`) — and the hash does not
change any derived size. §2.3's *size* genericity would then be a
compile-shape claim that no test ever executes, on the one slice whose
whole job is to freeze sizes.

Recommendation (**Q-O2**): add to `[dev-dependencies]`

```toml
hiss = { version = "0.3.2", features = ["x25519-cryptoxide"] }
```

so the test profile can declare a second suite over X25519, whose `PK`
differs from P-256's, and assert the four §2.3 derivations produce a
**different** set of sizes from the same formula. That turns 174 / 81 /
196 / 107 from four literals that happen to agree with a formula into an
executed derivation with two data points. It adds no runtime dependency,
changes no `cryptoxide` version (only a feature), and touches no wire
byte. If the orchestrator declines it, §2.3's genericity is pinned by
the hash-varying suite alone and the plan says so rather than implying
more.

### 3.7 The compile-time assertions this slice adds

In the `channel!` expansion, for every suite:

```rust
const _: () = assert!(Self::MSG1_LEN == PK + (PK + TAG) + MSG1_PAYLOAD_LEN + TAG);
const _: () = assert!(Self::MSG2_LEN == PK + TAG);
const _: () = assert!(Self::INIT_PACKET_LEN == INIT_HEADER_LEN + Self::MSG1_LEN + MAC1_LEN);
const _: () = assert!(Self::RESP_PACKET_LEN == RESP_HEADER_LEN + Self::MSG2_LEN + MAC1_LEN);
const _: () = assert!(Self::MSG1_LEN == IK::MSG1_SIZE);   // slither's arithmetic vs hiss's
const _: () = assert!(Self::MSG2_LEN == IK::MSG2_SIZE);
const _: () = assert!(Self::INIT_PACKET_LEN <= MAX_DATAGRAM);
const _: () = assert!(Self::RESP_PACKET_LEN <= MAX_DATAGRAM);
```

In `packet/suite.rs`, for the **reference suite only** — the bridge
between §2.3's derivation and slice 0's literals:

```rust
const _: () = assert!(<ReferenceSuite as Channel>::STATIC_PUBLIC_LEN == STATIC_PUBLIC_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::AEAD_TAG_LEN      == AEAD_TAG_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::MSG1_LEN          == IK_MSG1_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::MSG2_LEN          == IK_MSG2_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::INIT_PACKET_LEN   == INIT_PACKET_LEN);
const _: () = assert!(<ReferenceSuite as Channel>::RESP_PACKET_LEN   == RESP_PACKET_LEN);
```

This **closes slice 0's hand-forward note** verbatim: *"`STATIC_PUBLIC_LEN`
is pinned against `P256::PUBLIC_KEY_SIZE`; slice 1 should re-pin it
against the `noise!`-declared channel's `PUBLIC_KEY_SIZE` once that type
exists, which is the tighter bound."* Slice 0's direct
`P256::PUBLIC_KEY_SIZE` assertion **stays** — it costs nothing and it
keeps `constants.rs` self-contained.

**Also owed here (slice 0's hand-forward, R10):** verify at
implementation time whether `hiss::noise::datagram::MAX_EPOCH_JUMP` is
const-reachable under `default-features = false`. It is a slice-0
assertion, but slice 1 is the first slice that compiles against hiss's
`noise` module in anger, so it is the first slice that can answer it.

---

## 4. §3.2–3.4 — the three headers

### 4.1 The layouts, quoted

Everything in this section that looks like a layout fact is a
**quotation of `SPEC.md`**, marked as such. This plan derives no offset
and states no byte value; those come from the independent derivation
(§8). If that derivation disagrees with a quoted line below, it is a
spec-vs-derivation conflict for the maintainer — not something either
side reconciles quietly.

> §3.1: Every packet opens with `type: u8, version: u8`. **All
> multi-byte header fields are little-endian** … exactly three fields
> across the whole grammar — `sender_index`, `receiver_index` and
> `counter`.
>
> §3.2: `type(1) ‖ version(1) ‖ sender_index(4)` ← InitHeader, 6 B
>
> §3.3: `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)`
> ← RespHeader, 10 B
>
> §3.4: `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`
> ← DataHeader, 14 B

Note the asymmetry, because it is the single likeliest transcription
error in the slice and no compile check catches it: **RespHeader is
`sender` then `receiver`; DataHeader carries `receiver` only.** A
DataHeader whose `u32` is filled from *our* index instead of the peer's
routes every packet to the wrong session and produces no type error.
§9.3's test names it.

### 4.2 The structs

```rust
use packtool::Packed;

/// §3.2. 6 bytes.
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InitHeader {
    packet_type: u8,                     // private — set by `new`
    version: u8,                         // private — set by `new`
    pub(crate) sender_index: u32,        // little-endian on the wire (ruling 64)
}

/// §3.3. 10 bytes.
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RespHeader {
    packet_type: u8,
    version: u8,
    pub(crate) sender_index: u32,
    pub(crate) receiver_index: u32,
}

/// §3.4. 14 bytes. These bytes are the AEAD associated data, verbatim.
#[derive(Packed, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DataHeader {
    packet_type: u8,
    version: u8,
    pub(crate) receiver_index: u32,
    pub(crate) counter: u64,
}
```

with constructors that are the only way to build one:

```rust
impl InitHeader { pub(crate) const fn new(sender_index: u32) -> Self; }
impl RespHeader { pub(crate) const fn new(sender_index: u32, receiver_index: u32) -> Self; }
impl DataHeader { pub(crate) const fn new(receiver_index: u32, counter: u64) -> Self; }
```

Each `new` fills `packet_type` from `constants::PKT_HANDSHAKE_INIT` /
`PKT_HANDSHAKE_RESP` / `PKT_DATA` and `version` from
`constants::VERSION`. **No literal is re-declared anywhere in this
file.**

Six decisions, each with the reason it is not the obvious alternative:

1. **Plain `u32` / `u64`, never `[u8; N]`.** Ruling 64, and verified
   rather than assumed: `packtool-0.6.0/src/primitives.rs` packs
   integers with `to_le_bytes` and reads them with `from_le_bytes`. The
   derive **is** the encoder. `to_be_bytes` / `from_be_bytes` /
   `to_le_bytes` / `from_le_bytes` must not appear anywhere in
   `src/packet/` — §9.4 makes that a grep test, because it is the one
   regression that would silently survive every other check by
   re-introducing a hand conversion that happens to agree today.
2. **`packet_type` and `version` are private fields.** A `pub(crate)`
   `version` field is a struct literal away from a header that claims a
   version slither does not speak. Private + `new()` makes the wrong
   packet unconstructible rather than merely unlikely.
3. **No `#[packed(value = …)]` unit-struct markers for the type and
   version bytes**, tempting though they are. Two independent reasons:
   (a) `packtool-macro` accepts a `syn::Lit` for `value` — **a literal
   only, never a const path** (verified in `expand.rs`), so it would
   force `0x01u8` to be written out a second time, against `CLAUDE.md`'s
   "reference these constants, never re-declare a literal"; and (b)
   §3.1 puts the type/version check **before any further work**, i.e.
   before header parsing, so a `check()`-time rejection would put the
   gate in a second place and invert the spec's stated order. The gate
   owns that check, once (§6).
4. **No inherent getters.** packtool's derive already emits associated
   accessors named after each field (`InitHeader::sender_index(view)`);
   an inherent `fn sender_index(&self)` collides with it in the same
   associated-item namespace. `pub(crate)` fields, read directly.
5. **`Copy`, and no `Default`.** These are 6/10/14-byte values. A
   `Default` header is a header with packet type `0x00`, which is not a
   packet type.
6. **No `SIZE` constant of our own** — `<InitHeader as Packed>::SIZE` is
   the one source, asserted below.

### 4.3 `const SIZE` asserted against `src/constants.rs`

In `header.rs`, adjacent to the structs:

```rust
const _: () = assert!(<InitHeader as Packed>::SIZE == constants::INIT_HEADER_LEN);
const _: () = assert!(<RespHeader as Packed>::SIZE == constants::RESP_HEADER_LEN);
const _: () = assert!(<DataHeader as Packed>::SIZE == constants::DATA_HEADER_LEN);
```

These are the assertions that make §3.5's table executable. A field added,
removed, widened or narrowed fails the **build**. Slice 0's 43 asserts
already tie `INIT_HEADER_LEN` into `INIT_PACKET_LEN` and thence into
`MAX_DATAGRAM` and `MAX_PLAINTEXT`, so all three lengths are now pinned
end-to-end from the packtool layout up to the MTU.

One more, because §3.4's AD rule depends on it and a reader should not
have to re-derive it:

```rust
// §3.4: the 14 header bytes are the AEAD associated data, verbatim.
const _: () = assert!(<DataHeader as Packed>::SIZE + constants::AEAD_TAG_LEN
                      + constants::MAX_PLAINTEXT == constants::MAX_DATAGRAM);
```

### 4.4 Encode and decode

Use packtool's own surface; write no byte-shuffling by hand.

- **Encode:** `packtool::Packet::pack(&header)` → an owned
  `Packet<InitHeader>` whose `as_ref()` is the wire bytes; or
  `Packed::unchecked_write_to_slice` into an existing packet buffer,
  which is what the send path wants (the handshake packet is built in
  one buffer: header, then hiss's message, then mac1).
- **Decode:** `packtool::View::<'_, DataHeader>::try_from_slice(&dgram[..SIZE])`,
  then `.unpack()`. The `Err` is a length error only (see §4.2 decision
  3) and the gate has already excluded it, so the decode is
  infallible-in-practice; it is still written as a `Result` handled by
  a drop rather than an `unwrap`.

**The AD is the raw slice, never a re-encode.** §3.4 says the AD is "the
14 header bytes … verbatim". The gate therefore hands the AEAD the
**received bytes**, not `Packet::pack(&decoded_header)`. Re-encoding
would be correct today and is a place to be wrong forever; §9.3 pins it
by asserting the two are equal *and* by taking the slice.

---

## 5. §4 — mac1, the DoS gate

### 5.1 The construction, quoted

> §4.1:
> ```
> key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
> mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
> ```
> §4.2: mac1 is verified **before any curve or DH work**.
> §4.4: mac1 is **fixed keyed-BLAKE2b for every suite** — it does not
> follow the suite's Hash. […] The keying encoding is the canonical
> encoding (§2.4).

`recipient_static_canonical` is the **recipient's** static: on a
HandshakeInit the responder's; on a HandshakeResp the initiator's.

### 5.2 The one raw primitive

`cryptoxide` directly — this is the single exception `CLAUDE.md` grants,
and it must not become two. `cryptoxide-0.6.2` gives exactly what §4.1
asks for:

```rust
use cryptoxide::hashing::blake2b::Blake2b;

// Stage one — unkeyed BLAKE2b-256 over MAC1_LABEL ‖ canonical static.
let key: [u8; 32] = Blake2b::<256>::new()
    .update(constants::MAC1_LABEL)
    .update(static_canonical)
    .finalize();

// Stage two — keyed BLAKE2b-128. NOTE `finalize_at`, not `finalize`.
let mut tag = [0u8; constants::MAC1_LEN];
Blake2b::<128>::new_keyed(&key)
    .update(preimage)
    .finalize_at(&mut tag);
```

**`Context<128>` has no `finalize()`, and this will cost an hour if it is
discovered at the keyboard.** Verified in
`cryptoxide-0.6.2/src/hashing/blake2b.rs`: the array-returning
`finalize()` is emitted by a `context_finalize!` macro instantiated for
**224, 256, 384 and 512 only**, with the source comment *"Due to
limitation of const generic, we can't define finalize in the generic
context"*. `Context<128>` therefore offers `finalize_at(&mut [u8])`
alone, which asserts `out.len() == 16` — so the `[u8; MAC1_LEN]` buffer
is declared first and written into. Stage one is unaffected: 256 **is**
instantiated, so `finalize()` returns `[u8; 32]` directly.

`update` consumes and returns `self` (`update_mut` is the in-place form),
so the chain above type-checks as written.

Nothing else from `cryptoxide` is used, and `Cargo.toml`'s
`features = ["blake2"]` already bounds it to that. **No `hiss::noise`
hash type appears in `mac.rs`** — that would silently make mac1 follow
the suite Hash and quietly undo ruling 4.4.

### 5.3 `Mac1Key` — the type, and the reason it is a type

```rust
/// A mac1 key: BLAKE2b-256 over `MAC1_LABEL ‖ recipient_static`. §4.1.
///
/// Derived once per static, not once per packet. Not secret (§4.3) —
/// anyone holding the recipient's public static can compute it.
#[derive(Clone)]
pub(crate) struct Mac1Key([u8; 32]);

impl Mac1Key {
    /// `static_canonical` is §2.4's canonical encoding — for a
    /// `C: Channel`, `pubkey.as_ref()`.
    pub(crate) fn derive(static_canonical: &[u8]) -> Self;

    /// The 16-byte tag over `preimage` = all packet bytes preceding the tag.
    pub(crate) fn tag(&self, preimage: &[u8]) -> [u8; constants::MAC1_LEN];

    /// `true` iff `tag(preimage) == candidate`.
    pub(crate) fn verify(&self, preimage: &[u8], candidate: &[u8]) -> bool;
}
```

**Why a key type rather than a free `fn mac1(static, packet)`:** on the
*receive* path the recipient is always **us**, so the key is a constant
of the endpoint and is derived **once at endpoint construction**, never
per packet — which is the whole point of a DoS gate (a garbage flood must
cost one keyed hash, not a hash plus a key derivation). On the *send*
path the key is a constant of the peer, derived once per pending/
connection. A free function keyed by a public key hands every caller the
opportunity to re-derive per packet, and nothing would ever notice.

The `[u8; 32]` is not secret (§4.3), so **no `Zeroize`, no `Drop`
impl**. Adding one would imply a security property mac1 explicitly does
not have, and §4.3 is emphatic about that.

**Constant-time comparison is not required and is recommended anyway.**
§4.3 states mac1's key is derived from public data, so a timing leak
leaks nothing an attacker cannot compute. A plain `==` on `[u8; 16]` is
therefore conformant. Recommend the folded-XOR compare regardless: it
costs nothing measurable, and "this comparison is variable-time on
purpose" is a comment nobody will believe in three years. Flagged as a
judgement call so it is not read as a spec requirement.

### 5.4 The preimage extent, and where mac1 sits in the ladder

> §4.1: `mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)`

For both handshake packet types the preimage is therefore the whole
datagram **less its trailing `MAC1_LEN` bytes** — header **and** the
hiss message, tag excluded. Expressed once, in one place:

```rust
let (preimage, tag) = dgram.split_at(dgram.len() - constants::MAC1_LEN);
```

Because the gate has already fixed the datagram's length to the suite's
`INIT_PACKET_LEN` / `RESP_PACKET_LEN` (§6), that split is exact and
unambiguous. **This is the load-bearing reason to prefer the strict
length reading in §6.3** — under a lenient reading, "the tag" would have
two candidate positions and the spec names neither.

Data packets carry **no mac1** (§3.4's layout has none). A mac1 check on
the data path would be a wire-visible invention.

The position in §6.1's cost ladder, quoted:

> `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed
> hash, **0 DH** | … | short/oversize, unknown type/version, bad mac1 —
> all silent, before the queue

So the order is **fixed**: length → classify → mac1 → (slice 2's queue).
mac1 runs **after** the gate and **before** anything in slice 2. Slice 1
delivers the first three and stops.

---

## 6. §3.1 — the classify/drop gate

### 6.1 The rule, quoted

> §3.1: A datagram shorter than its type's fixed minimum, longer than
> `MAX_DATAGRAM`, or bearing an unknown type or version is silently
> dropped before any further work. This pre-AEAD gate is the **only**
> silent-drop tier for malformed traffic: a packet that fails here may
> genuinely be corruption, so nothing is signalled; a packet that passes
> the AEAD and then fails structurally is a peer bug or an attack, and
> gets a signalled death (§8.2).
>
> §3.5: Oversize receive (> `MAX_DATAGRAM`) is a silent drop.

Reserved types `0x04` (unused), `0x05` (cookie/mac2, §19) and `0x06..`
are "never emitted, silently dropped" — they take the *same* path as an
unknown type. Slice 1 mints no behaviour for them beyond the drop, and
in particular **no `PKT_RESERVED_*` branch that logs, counts or
answers**; `constants::PKT_RESERVED_UNUSED` / `PKT_RESERVED_COOKIE`
exist to be *documented*, not dispatched.

### 6.2 The shape

```rust
/// A datagram that survived §3.1's gate. Borrowed — nothing is copied.
pub(crate) enum Inbound<'a> {
    Init { header: InitHeader, msg1: &'a [u8], preimage: &'a [u8], mac1: &'a [u8] },
    Resp { header: RespHeader, msg2: &'a [u8], preimage: &'a [u8], mac1: &'a [u8] },
    Data { header: DataHeader, ad: &'a [u8], ciphertext: &'a [u8] },
}

/// §3.1's pre-AEAD gate — the crate's ONLY silent-drop tier.
/// `None` **is** the drop: it is not an error, it is not traced, it is
/// not counted, and it never reaches the application.
pub(crate) fn classify<C: Channel>(dgram: &'_ [u8]) -> Option<Inbound<'_>>;
```

- **Generic over `C: Channel`**, because `INIT_PACKET_LEN` and
  `RESP_PACKET_LEN` are per-suite (§2.3) while the headers and caps are
  not.
- **`Option`, not `Result`.** §18.1's taxonomy is closed, and slice 0's
  §11 already walked slice 1 and concluded *"silent drop — §3.1/§3.5/§4
  make all three drops, not errors. No variant needed, by design."* A
  `DropReason` enum returned to a caller invites a caller to act on it;
  the spec's whole point is that nothing acts on it.
- **`ad` and `preimage` are slices into `dgram`**, so the AEAD and mac1
  both see received bytes verbatim (§4.4, §5.4).
- **Borrowed, not owned.** The gate allocates nothing. It is the
  cheapest thing in the crate and must stay that way.

### 6.3 The order of checks

Derived from §3.1's own sentence, and stated because the order is
observable in cost even though it is not observable on the wire:

1. `dgram.len() > MAX_DATAGRAM` ⇒ drop. Type-independent, so it is first.
2. `dgram.len() < 2` ⇒ drop. Cannot read what §3.1 puts at the front of
   every packet.
3. Read the type byte. Not `PKT_HANDSHAKE_INIT` / `PKT_HANDSHAKE_RESP` /
   `PKT_DATA` ⇒ drop (this is where every reserved and future type
   dies).
4. Read the version byte. `!= constants::VERSION` ⇒ drop.
5. Length against the type's rule:
   - `PKT_HANDSHAKE_INIT` ⇒ `C::INIT_PACKET_LEN`
   - `PKT_HANDSHAKE_RESP` ⇒ `C::RESP_PACKET_LEN`
   - `PKT_DATA` ⇒ at least `DATA_HEADER_LEN + AEAD_TAG_LEN`, the
     empty-plaintext keepalive of §3.4 being exactly that
6. Parse the header (§4.4) and take the slices.

**Steps 3 and 4 must both run before step 5**, because §3.1 says
"shorter than **its type's** fixed minimum" — the minimum is not knowable
until the type is.

### 6.4 An ambiguity in step 5 — reported, not resolved (**Q-M1**)

§3.1 says a datagram *"shorter than its type's fixed minimum"* is
dropped, and separately that one *longer than `MAX_DATAGRAM`* is
dropped. Read literally, a **197-byte HandshakeInit** is neither: it is
not shorter than 196 and not longer than 1200, its type and version are
known, and it passes the gate.

§3.2 and §3.3, however, declare those packets as **fixed sizes** — "196
bytes (reference suite)", "107 bytes" — not minima. And §4.1's preimage
is "all packet bytes preceding the tag", which under the lenient reading
has no defined answer: is the tag the last 16 bytes of the datagram, or
the 16 bytes at the suite's fixed offset? The spec names neither, which
is itself the strongest evidence that the lenient reading was never
intended.

Per `CLAUDE.md` working rule 3 I am not picking one silently. The prose
("fixed minimum") and the section headings ("196 bytes") point in
different directions, and this is the freeze slice.

**Recommendation: exact length for `PKT_HANDSHAKE_INIT` and
`PKT_HANDSHAKE_RESP`; `>=` minimum for `PKT_DATA`.** It is the only
reading under which §4.1's preimage is well-defined; it matches hiss's
own API, which takes `&[u8; MSG1_SIZE]` and cannot be handed a longer
message; and it is the strictly safer default for a DoS gate. But it is
**wire-visible behaviour on the slice that freezes the wire**, so it
wants a ruling rather than an implementer's judgement.

### 6.5 The gate is silent — including in the trace (**Q-M2**)

§18.2's table has **five** targets: `slither::policy`, `slither::replay`,
`slither::frames`, `slither::roam`, `slither::io`. None of them covers
the pre-AEAD gate or a mac1 failure, and the section closes: *"The
targets are operator-visible contract: renaming or dropping one is a
protocol revision."* Adding a sixth is therefore also a protocol
revision.

**So slice 1 emits no `tracing` event from `classify` or from mac1
verification.** A drop counter is exactly the kind of thing an
implementer adds helpfully and a reviewer never questions.

**A conflict to report, found by grepping for the rationale rather than
the token (working rule 4).** slither's committed `Cargo.toml` says, of
the `tracing` dependency:

> Structured diagnostics (SPEC.md §18.2's trace targets): **the DoS-gate
> drop counters**, the roaming trace, the unlisted-static reject and the
> handshake give-up all surface as `tracing` events …

Of those four, §18.2's table carries only the roaming trace. The
comment asserts a §18.2 obligation that §18.2 does not contain. Either
the spec owes a sixth target or the comment is wrong; my reading is that
the comment overreaches — §3.1 and §6.1's ladder both say "silent" — but
it is a committed file making a spec claim, so the maintainer should say
which. Note that "silent" plausibly means *nothing is signalled to the
peer or the application* rather than *no local trace exists*; that
ambiguity is precisely why it needs an answer and not a guess.

### 6.6 What the gate must NOT do

- **Not decrypt, not route, not touch a table.** Routing by
  `receiver_index` is §17.3 and slice 2's.
- **Not validate an index.** §17.3's nonzero rule is a *minting* rule
  (§1, boundary 3).
- **Not verify mac1.** mac1 is a separate step in the ladder, on a
  separate key, and `classify` must stay usable by a caller that has no
  static key at all (the golden harness is exactly such a caller).
- **Not allocate, not copy, not `Box`.**
- **Not special-case the keepalive.** §3.4's empty-plaintext keepalive
  is a `PKT_DATA` with a 16-byte ciphertext; it satisfies the minimum
  and passes. Distinguishing it is §7.5's job, post-AEAD, in slice 3.

---

## 7. §5.2 — the msg1 payload, and the endianness trap

**This section exists because it is the most plausible way for ruling 64
to be applied wrongly, on the day it landed.**

> §5.2:
> ```
> msg1 payload (12 B, encrypted in msg1's tail):
>     ts_secs(8, BE) ‖ ts_nanos(4, BE)
> ```

The msg1 payload is **big-endian**. Ruling 64 does not reach it: it is
not a header field, it rides inside msg1's AEAD-sealed tail, and §3.1's
rule is explicit that it covers "exactly three fields" — `sender_index`,
`receiver_index`, `counter`.

Ruling 64's own scope paragraph is headed *"Scope, stated so it is not
over-applied"* and lists **two** exclusions: §8.1's varints and §2.4's
static comparison. §5.2's big-endian timestamp is a **third** and is not
listed. There is no contradiction — §5.2 still says BE, and a payload is
not a header field — but the paragraph reads as exhaustive and is not,
which is the shape of thing that gets over-applied by the next reader.
Recorded as **Q-M3**: a one-line scope amendment, no wire change.

The codec:

```rust
/// msg1's 12-byte encrypted payload (§5.2). Big-endian — NOT the
/// little-endian of §3.1's header fields (ruling 64 reaches exactly
/// three header fields and this is not one of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Msg1Payload { secs: u64, nanos: u32 }

impl Msg1Payload {
    pub(crate) const fn encode(&self) -> [u8; constants::MSG1_PAYLOAD_LEN];
    pub(crate) const fn decode(bytes: &[u8; constants::MSG1_PAYLOAD_LEN]) -> Self;
}
```

- **Not a packtool struct.** packtool packs little-endian; using it here
  would be exactly wrong. This is the one place in `src/packet/` where
  `to_be_bytes` / `from_be_bytes` appear, and §9.4's grep test carves
  out this file by name so the carve-out is visible rather than
  implicit.
- **`Ord` is derived deliberately**: §5.3's strictly-greater rule and
  §17.1's guard both compare timestamps, and deriving `Ord` on
  `(secs, nanos)` in that field order gives the correct chronological
  ordering for free. Slice 2 consumes it; slice 1 does not use it.
- **No clock read, no monotonic forcing, no `SystemTime`.** §5.3 is
  slice 2's. A `Msg1Payload::now()` in slice 1 would put the protocol's
  one wall-clock read (§16.5) in the packet layer.
- `MSG1_PAYLOAD_LEN` is already asserted `== TIMESTAMP_LEN` in
  `constants.rs`; `encode`'s return type ties the codec to it.

---

## 8. The golden-wire vectors — the frame, and only the frame

### 8.0 What this section deliberately does not contain

**No byte value, no field offset and no layout of this plan's own
devising appears anywhere in this section, and none appears anywhere in
this document.** A separate agent is deriving the wire layout from spec
text alone, in parallel, with no sight of this plan and no sight of
`src/`. That independence is the entire point: Appendix B freezes these
vectors "for the first time at wire version 1, then held
byte-identical", and a vector set that certifies itself against the
implementation that produced it freezes a mistake permanently.

The instinct to specify everything is the wrong instinct here. This
section specifies **where the vectors live, what shape they take, which
properties they must pin, and what happens when they disagree with the
code**. The numbers are not mine to write.

Everything quoted from `SPEC.md` in §4.1, §5.1 and §7 above is quoted and
marked as such. If the independent derivation disagrees with a quoted
line, that is a spec-vs-derivation conflict for the maintainer (§8.4),
not a licence for either side to adjust.

### 8.1 Where they live, and the two-file split

```
src/packet/golden_vectors.rs   #[cfg(test)] — DATA ONLY, no test logic,
                               no `use` of any slither item, written by the
                               independent derivation agent
src/packet/tests.rs            #[cfg(test)] — the harness that feeds them
                               through the implementation, written by an
                               author who has not read the implementation
```

**The split is the mechanism, not a filing convention.** The derivation
agent writes *bytes and the field values those bytes encode*, as plain
`const` items, and needs to know nothing about slither's API — not a type
name, not a function signature, not a module path. A vector file that
compiles against nothing cannot have been shaped by the implementation.
The harness author gets my §2–§7 API surface plus the spec, and writes
mechanical assertions. **Three people, three inputs, no shared way to be
wrong.**

Why `src/packet/` rather than `tests/`: the headers, mac1, the payload
codec and the gate are all `pub(crate)` (§2.2), and an integration test
in `tests/` cannot reach them. The alternatives are to widen the public
API to suit a test — which would put wire internals under semver — or to
add a `#[doc(hidden)]` shim, which is the same thing wearing a hat.
Slice 0's convention (`tests/spec_constants.rs`) applies to `pub`
surfaces and is honoured for the one part of slice 1 that has one:
`tests/spec_packet.rs` (§9.2). **Q-O3** records the choice.

Consequence to accept knowingly: `src/` ships in the published crate, so
the vectors ship with it. That is arguably right — the wire freeze
travelling with the code is a feature — and they are small.

### 8.2 The shape of `golden_vectors.rs`

Data only. Each vector is a group of `const` items: the **encoded
bytes**, plus the **field values those bytes encode**, separately. That
shape is what makes a wrong harness obvious — the harness's only job
becomes `encode(fields) == bytes` and `decode(bytes) == fields`, and
there is nowhere for it to hide an assumption.

Schema (names are indicative; the derivation agent may rename, and the
harness follows):

```rust
//! Golden wire vectors — wire version 1. Derived from SPEC.md §§2–5
//! independently of `src/packet/`. FROZEN: a red test here is a ruling
//! request, never an expectation to update (SPEC.md §1.3, Appendix B).

pub(crate) mod init_header {
    pub(crate) const SENDER_INDEX: u32 = /* … */;
    pub(crate) const BYTES: [u8; 6]    = /* … */;
}
// … resp_header, data_header, msg1_payload, mac1_init, mac1_resp,
//    prologue, mac1_label, sizes
```

**Two obligations on the values the derivation agent chooses** — these
constrain the *choice*, not the *encoding*, and without them a vector can
be byte-identical under both byte orders and pin nothing:

1. **Every multi-byte integer in a vector must have a different encoding
   little-endian than big-endian.** No palindromes, no values below 256,
   no zero. This is the whole reason the vectors exist after ruling 64.
2. **Where a header carries two index fields, they must differ from each
   other**, so a swapped-field bug (§4.1's asymmetry) cannot pass.

A third, for the counter specifically: it must exercise bytes above the
low 32 bits, so a `u32`-sized counter bug fails.

### 8.3 What the vectors must pin

Twelve properties. Each is a *property*; none is a number.

| # | Property | Why it must be pinned here |
|---|---|---|
| 1 | `PROLOGUE`'s exact octets, its length, and that its final byte equals `VERSION` | Appendix B names the prologue explicitly; §5.1 binds the version into the Noise transcript, so a wrong prologue is a cryptographic failure, not a parse failure |
| 2 | `MAC1_LABEL`'s exact octets and length | §4.1's key derivation input |
| 3 | `InitHeader` — the encoded bytes for a stated `sender_index`, and each field's offset and width | §3.2; the LE pin |
| 4 | `RespHeader` — bytes for stated, **distinct** `sender_index` and `receiver_index`, with offsets and widths | §3.3; the LE pin **and** the field-order pin |
| 5 | `DataHeader` — bytes for a stated `receiver_index` and `counter`, with offsets and widths | §3.4; the LE pin, and the counter's full 8 bytes |
| 6 | **The AD extent**: for a Data packet, the associated data is exactly the leading `DATA_HEADER_LEN` bytes of the datagram, verbatim, and nothing more | §3.4's "the 14 header bytes are the AEAD associated data, verbatim" — an off-by-one here is undetectable except by a peer |
| 7 | **The mac1 preimage extent**: for both handshake types, the preimage is the datagram less its trailing `MAC1_LEN` bytes — covering the header *and* the hiss message, excluding the tag | §4.1's "all packet bytes preceding the tag" |
| 8 | The derived mac1 **key** (32 bytes) for a stated canonical static | §4.1 stage one, separately from stage two, so a failure localises |
| 9 | The mac1 **tag** for that key over an Init-shaped preimage and over a Resp-shaped preimage | §4.1 stage two; two shapes because the preimage lengths differ |
| 10 | The **canonical static** used for 8 and 9, as 65 raw octets, with a note that it is §2.4's uncompressed SEC1 form | §4.4's ruling moved P-256 keying from the 33-byte compressed form; this vector is what makes that ruling permanent |
| 11 | The **msg1 payload**: the 12 bytes for a stated `(ts_secs, ts_nanos)`, **big-endian** | Appendix B names "the 12-byte msg1 payload"; §7's trap |
| 12 | The **sizes**: `INIT_HEADER_LEN`, `RESP_HEADER_LEN`, `DATA_HEADER_LEN`, `IK_MSG1_LEN`, `IK_MSG2_LEN`, `INIT_PACKET_LEN`, `RESP_PACKET_LEN`, `MAC1_LEN`, `AEAD_TAG_LEN`, `MAX_DATAGRAM`, `MAX_PLAINTEXT`, and the minimum Data datagram | the 174 / 81 / 196 / 107 pins `PLAN.md` §4 names by number |

Two further pins that are not vectors but belong to the same freeze and
live in `tests/spec_packet.rs` (§9.2): the **protocol name string**
`Noise_IK_P256_ChaChaPoly_BLAKE2b` (§2.2 says "pinned by test"), and the
**packet type bytes** `0x01` / `0x02` / `0x03` appearing as the first
byte of each header (already covered by `tests/spec_constants.rs` as
*constants*; here they are pinned as *wire position*).

### 8.4 What the vectors must NOT pin, and why

**No hiss ciphertext.** No vector may contain the bytes of a completed
msg1 or msg2, or of a sealed Data ciphertext.

This is not squeamishness, it is a supply-chain fact. `Cargo.lock` is
not committed (`CLAUDE.md`), so every gate run re-resolves `hiss` within
`^0.3.2`. A hiss patch that changed how many bytes it draws from the
caller's RNG — or the order it draws them — would change every ephemeral
and therefore every handshake byte, turning the golden test red for a
reason that has nothing to do with slither's wire. A frozen vector that
can go red without a slither change destroys the discipline the freeze
exists to create: the next red would be argued about instead of ruled on.

This is also exactly what `SPEC.md` asks for. §1.3 and Appendix B name
the freeze's contents precisely — **"the prologue, the 12-byte msg1
payload, mac1's canonical keying"** — and every one of those is
slither's own, computable without stepping a single hiss state machine.
The twelve properties in §8.3 are all hiss-independent by construction:
they need a fixed 65-byte octet string (not a live P-256 key), a fixed
preimage, and BLAKE2b.

The live-handshake round trip is slice 2's, and it asserts **lengths and
structure**, not ciphertext bytes.

### 8.5 The reconciliation protocol — the part that is easy to get wrong under time pressure

When the vectors and the implementation first meet:

1. **Nobody edits `golden_vectors.rs` to make a test pass.** Not the
   implementer, not the harness author, not the orchestrator.
2. A disagreement is triaged into exactly one of three buckets, and the
   triage is written down before anything is changed:
   - **the implementation is wrong** ⇒ fix the implementation;
   - **the derivation is wrong** ⇒ the derivation agent corrects it,
     citing the spec line, and states what it misread;
   - **the spec is ambiguous** ⇒ **stop, and ask for a ruling.**
     §6.4 is already a live candidate for this bucket.
3. `CLAUDE.md` states the standing rule and it applies from this slice
   onward: *"any change that moves a wire byte turns a test red. That is
   by design — treat such a red as 'this needs a ruling', not 'update
   the expectation.'"*
4. The triage outcome is recorded in the slice's handoff notes, whatever
   it is. A vector that was corrected during slice 1 is exactly the
   history a later reviewer needs.

`golden_vectors.rs` carries that rule in its module doc, in the file, so
a future contributor meets it before the diff.

---

## 9. Test plan

### 9.1 Authorship — stricter than slice 0, and for a reason

`CLAUDE.md` working rule 6: *"The test author is not the implementer for
story-level acceptance tests. One author writing both can make both wrong
in a mutually consistent way, and CI stays green."*

Slice 0 applied the swap to two files (`constants.rs`, `error.rs`) and
let implementers test their own behavioural code. **Slice 1 applies it to
everything, and the implementer ships no committed test at all.**

The reason is that slice 1 is *entirely* transcription. There is no
behaviour here that fails visibly — a wrong byte order, a swapped field,
a mac1 preimage that is one byte short, a big-endian payload written
little-endian: every one of them round-trips perfectly against itself and
passes any test its own author writes. This is the exact failure mode
rule 6 exists for, at its highest concentration in the project.

| Author | Writes | Must not see |
|---|---|---|
| **A — implementer** | `suite.rs`, `header.rs`, `mac.rs`, `payload.rs`, `mod.rs`, the `lib.rs` wiring | the vectors, until reconciliation |
| **B — derivation agent** (already running) | `golden_vectors.rs` — data only | this plan, `src/` |
| **C — test author** | `src/packet/tests.rs`, `tests/spec_packet.rs` | A's implementation |

A may write throwaway `#[cfg(test)]` checks while working; **none is
committed.** File ownership is the enforcement: A owns no test file.

C works from `SPEC.md` §§2–5 plus §§2–7 of this plan (the API surface —
names and signatures, no bytes, no offsets). C's tests must compile
against the signatures alone.

### 9.2 Story-level — S22 (partial)

> **S22 — a user can pick a crypto suite, and mismatches fail closed.**
> *Accepts:* the suite is declared once via the macro; a peer on a
> different suite, or a wrong static, fails the handshake and installs
> nothing. An unknown version byte is dropped silently — there is no
> negotiation, ever. *Anchor:* §1.1, §2, §3.1. *Paused clock:* yes.

Slice 1 closes three of the four clauses:

| Clause | Slice | Test |
|---|---|---|
| "declared once via the macro" | **1** | `two_suites_coexist_in_one_crate` (`tests/spec_packet.rs`) — two `slither::channel!` invocations in two modules; both compile, `PROTOCOL_NAME`s differ, and the reference suite's name is `Noise_IK_P256_ChaChaPoly_BLAKE2b` (§2.2's stated pin) |
| "a peer on a different suite … fails" | **1** (the packet half) | `a_mismatched_suite_dies_at_the_length_gate` — a datagram of another suite's `INIT_PACKET_LEN`, classified as the reference suite, is `None`. §2.2: *"A mismatched-suite packet dies silently at the length gate or at mac1 — the same fate as garbage."* |
| "an unknown version byte is dropped silently" | **1** | `unknown_version_is_dropped_silently` |
| "a wrong static … fails the handshake and installs nothing" | **2** | needs a driven handshake |

**On S22's "paused clock: yes" — an honest note rather than a gesture.**
Nothing in slice 1's half of S22 depends on time; `classify` is a pure
function of a byte slice, and the macro is a compile-time construct.
Wrapping either in `#[tokio::test(start_paused = true)]` would look like
a timer test and prove nothing. The paused-clock obligation belongs to
S22's slice-2 clause, where a handshake is actually driven, and it is
carried forward in §14's handoff notes rather than discharged
cosmetically here.

### 9.3 Unit tests — `src/packet/tests.rs` (author C)

**The gate (§3.1):**

1. `unknown_version_is_dropped_silently` — every byte `0x00..=0xff`
   except `VERSION`, over all three well-formed packet shapes ⇒ `None`.
2. `unknown_and_reserved_types_are_dropped` — `0x00`, `0x04`, `0x05`,
   and `0x06..=0xff` ⇒ `None`, at each type's would-be length and at
   several others. The reserved types take the same path as the unknown
   ones; there is no third behaviour.
3. `oversize_is_dropped` — `MAX_DATAGRAM + 1` ⇒ `None`; a valid Data
   packet at exactly `MAX_DATAGRAM` ⇒ `Some`. The boundary is inclusive.
4. `short_is_dropped` — 0 bytes, 1 byte, and each type one byte short of
   its minimum ⇒ `None`.
5. `a_mismatched_suite_dies_at_the_length_gate` — §9.2.
6. `keepalive_is_the_minimum_data_packet` — `DATA_HEADER_LEN +
   AEAD_TAG_LEN` ⇒ `Some`; one byte fewer ⇒ `None`. §3.4's
   empty-plaintext keepalive must survive the gate, and it is the one
   Data shape a "minimum sensible payload" bug would eat.
7. `the_gate_never_panics` — a seeded `ChaCha20Rng` sweep of a few
   thousand random datagrams of random length `0..=1300`; assert only
   that nothing panics and every result is `Some`-or-`None`. Cheap,
   deterministic, and no `proptest` dependency (slice 0's scope fence).
8. `data_ad_is_the_leading_header_bytes_verbatim` — `Inbound::Data`'s
   `ad` is a subslice **of the input**, checked by pointer range, not
   only by equality. A re-encoded AD passes an equality check today and
   is a permanent hazard; §3.4 says "verbatim".
9. `handshake_preimage_and_tag_partition_the_datagram` —
   `preimage.len() + mac1.len() == dgram.len()`, `mac1.len() ==
   MAC1_LEN`, both subslices of the input, no overlap. §4.1's extent.

**The headers (§3.2–3.4):**

10. `header_sizes_match_constants` — a runtime mirror of §4.3's const
    asserts, so the pin survives if a const assert is ever dropped.
11. `headers_round_trip` — pack → unpack over a sweep including `0`,
    `1`, `u32::MAX`, `u64::MAX - 1`, `u64::MAX`. **Not a byte test** —
    the bytes are §8's.
12. `resp_header_fields_do_not_swap` — distinct sender and receiver
    values read back to the right fields. Stated dependency: a
    *consistently* swapped encode+decode passes this test and is caught
    only by golden vector 4. Both are needed; neither alone suffices,
    and the comment in the test says so.

**mac1 (§4):**

13. `mac1_key_matches_the_golden_vector` — vector 8, stage one alone, so
    a failure localises to the derivation rather than the tag.
14. `mac1_tag_matches_the_golden_vectors` — vectors 9, Init-shaped and
    Resp-shaped.
15. `mac1_rejects_every_single_bit_flip` — flip each bit of a bounded
    sample of preimage positions, and each bit of the tag; all reject.
16. **`mac1_does_not_follow_the_suite_hash`** — declare a second suite
    with `Hash = Sha256` and assert its mac1 over identical input equals
    the reference suite's. **This is ruling 4.4's only mechanical
    guard**, and the "improvement" it prevents — making mac1 generic over
    `Channel::Hash` — is one a reviewer would praise.
17. `mac1_keys_on_the_canonical_static` — the keying input's length is
    `STATIC_PUBLIC_LEN` (65, not 33). §4.4 moved P-256 keying from the
    compressed to the uncompressed form; if vector 10 supplies both
    encodings, additionally assert the two derived keys differ.

**The msg1 payload (§5.2):**

18. `msg1_payload_matches_the_golden_vector` — vector 11.
19. **`msg1_payload_is_big_endian_not_little`** — encode a chosen
    `(secs, nanos)`, and assert the result is **not** the little-endian
    encoding of the same two fields (computed inside the test). This
    pins byte order **without naming a byte**, which is why it works
    alongside the independent vectors rather than duplicating them. Name
    it exactly this, so the next reader of ruling 64 finds it.
20. `msg1_payload_round_trips_and_orders_chronologically` — `decode ∘
    encode == id`, and `Ord` agrees with `(secs, nanos)` chronological
    order (slice 2's guard depends on it).

**The golden harness (§8):**

21. `golden_<group>` — one mechanical test per vector group: encode the
    stated fields, compare to the stated bytes; decode the stated bytes,
    compare to the stated fields. No logic beyond that.
22. `sizes_match_the_golden_vectors` — vector 12 against
    `constants::*` and `<ReferenceSuite as Channel>::*`.

**The ruling-64 fence:**

23. `no_hand_rolled_byte_order_outside_the_payload_codec` — via
    `include_str!` on each file in `src/packet/`, assert that
    `to_be_bytes`, `from_be_bytes`, `to_le_bytes` and `from_le_bytes`
    appear in **`payload.rs` only**. Ruling 64's third argument was that
    big-endian "forced every header field to `[u8; N]` with
    `to_be_bytes`/`from_be_bytes` at each site … adding a place to be
    wrong per field". This test is that argument, made permanent. It is
    also the only test in the slice that would survive a future
    contributor who "just needed a quick conversion here".

### 9.4 Public-surface tests — `tests/spec_packet.rs` (author C)

The file exists for a reason no unit test can serve: an integration test
is a **separate crate**, so it is the only thing that catches a `$crate`
path bug, a missing `pub`, or a `channel!` hygiene failure. State that in
its module doc.

24. `reference_suite_protocol_name` — `Noise_IK_P256_ChaChaPoly_BLAKE2b`
    (§2.2, "pinned by test").
25. `reference_suite_sizes_are_the_spec_values` — 174 / 81 / 196 / 107
    against `slither::constants::*`.
26. `two_suites_coexist_in_one_crate` — §9.2.
27. `a_second_curve_derives_different_sizes` — **only if Q-O2 is
    answered yes**: an X25519 suite, asserting the four §2.3 derivations
    yield a different, self-consistent set from the same formula.
    Without it, §2.3's size genericity is a compile-shape claim no test
    executes (§3.6).

**Caveat to record, because the test looks stronger than it is:** an
integration test links against slither's `[dependencies]` as well as its
dev-dependencies, so `hiss` is in scope for free. It therefore proves
the macro's paths resolve — but it does **not** prove that a real
downstream consumer's build works, because that consumer must add `hiss`
to their own `Cargo.toml` (§3.5a). Only documentation closes that gap.

### 9.5 Not planned, and why

- **No `trybuild` compile-fail tests.** They would be the natural way to
  pin "two `channel!` in one module is an error" and "a suite without a
  `[12]` payload cannot be declared", and they need a new dev-dependency
  and a UI-snapshot corpus that drifts with every compiler release.
  Slice 0's dependency fence stands. Recorded as an option for slice 9.
- **No `proptest`.** Same fence; test 7 is a seeded sweep instead.
- **No live-handshake round trip.** Slice 2's, and §8.4 says why no
  vector may depend on one.
- **No benchmark of the gate.** The DoS argument is about *work per
  packet* (§4.2's "one keyed hash"), which §5.3's key-derived-once design
  and code review settle better than a number that varies by machine.

---

## 10. Build order

```
        ┌─ (serial) API-surface freeze: packet/mod.rs skeleton + lib.rs wiring ─┐
        └─ signatures from §§2–7 handed to author C; nothing else shared         ┘
                          │
         ┌────────────────┼────────────────────────────┐
   A: suite/header/     B: golden_vectors.rs      C: tests.rs +           ← parallel
      mac/payload/gate     (already running)         tests/spec_packet.rs
         └────────────────┼────────────────────────────┘
                          │
                  reconciliation (§8.5)                                   ← serial, triaged
                          │
                  the eight gates                                         ← serial
```

**The serial prologue is mandatory and is one small commit:** `lib.rs`
gains `pub mod packet;` + `pub use packet::Channel;`, and
`src/packet/mod.rs` declares the submodules and the `classify` /
`Inbound` signatures with `todo!()` bodies. Doing it first means no agent
edits `lib.rs`, and author C has something to compile against before A
has written a line of logic.

**A and C must not converge.** If C needs a signature that does not
exist, the answer is an amendment to this plan, circulated to both — not
a look at A's file.

---

## 11. The gate table

**No gate below has been run.** This slice has produced a plan and no
code; per `CLAUDE.md` working rule 7 I will not report a gate I have not
executed. The table is the obligation, plus what in *this* slice
threatens each one.

| # | Gate | Command | What slice 1 puts at risk |
|---|---|---|---|
| 1 | Compiles | `cargo build --all-features --all-targets` | `channel!` hygiene: `$crate` paths, `#[macro_export]` ordering (invoke via `crate::channel!`), `$vis` threading, and `::hiss::` resolution inside the expansion |
| 1b | Feature floor | `cargo build --no-default-features` | slice 0 added this; `packet` must not depend on `test-util` |
| 2 | Format | `cargo fmt --all --check` | **rustfmt does not format `macro_rules!` bodies.** The `channel!` body must be hand-formatted to the surrounding style, and a reviewer must know rustfmt is not checking it |
| 3 | Lints | `cargo clippy --all-features --all-targets -- -D warnings` | the highest-risk gate. Lints are suppressed inside *external* macro expansions, but `channel!` is **slither's own**, so everything it emits is linted at each expansion site — including `#![warn(missing_docs)]` against the generated suite struct and against whatever `hiss::noise!` emits through it. If a blanket `#[allow]` proves necessary, it must be scoped to the generated items and **never** wrap the whole expansion, because it would then silence lints in consumers' crates too |
| 4 | Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `--all-features` | intra-doc links from `channel!`'s rustdoc to `Channel`; doc examples inside a `#[macro_export]` macro are compiled as doctests **in the crate's own context**, so a `channel!` example must be written to work there |
| 5 | Tests | `cargo test` **and** `cargo test --all-features` | — |
| 6 | **Wire pins** | the golden and size tests, under `cargo test` | **the first slice where this gate means what it says.** Report it explicitly (§14) |
| 7 | MSRV | `cargo +1.96 check --all-features --all-targets` | `Blake2b::<256>` const-generic turbofish and `const _: () = assert!(…)` are both long-stable; the real risk is a `const fn` in `payload.rs` using something newer. Keep `encode`/`decode` free of anything past 1.96, or drop `const` |
| 8 | Supply chain | `cargo deny check` | no new crate. Q-O2 adds a **feature** to an existing dev-dependency (`cryptoxide/x25519` via `hiss`), which changes no licence and no source |

---

## 12. Risks

**R1 — the golden vectors and the implementation are written to the same
misreading anyway.** The whole §8/§9.1 apparatus fails if authors B and C
read §3.1 the same wrong way. Mitigation: B derives from the spec with no
API, C tests from the spec plus signatures, and A implements from the
spec plus this plan — three routes. Residual risk is a genuinely
ambiguous spec line, which is exactly what §6.4 is, which is why it is a
ruling request rather than an implementer's call.

**R2 — `channel!` under `-D warnings` (gate 3).** The likeliest way this
slice runs long. `hiss::noise!` generates a large amount of code; passed
through slither's own macro, that code is linted at slither's expansion
site and at every consumer's. Mitigation: build the reference suite
first, in the serial prologue, and run gate 3 before writing anything
else. If it is bad, the fallback is a scoped `#[allow]` on the generated
items only — and if *that* is not enough, the fallback is that
`channel!` emits a private module wrapper carrying the allow, which
costs a naming decision and nothing else. **Do not solve it with a
crate-level `#![allow]`.**

**R3 — `Channel` is the wrong shape for slice 2.** §3.3 deliberately
ships a suite-facts-only trait and defers the handshake seam. The risk is
that slice 2 finds `Channel` cannot be extended without moving the type
parameter. Mitigation: nothing is released before slice 9, and slice 2's
plan should open by re-reading §3.3 and saying whether the extension or a
companion trait is right. Flagged in §14's handoff.

**R4 — the hiss-in-the-consumer's-Cargo.toml requirement (§3.5a) is
discovered by a consumer, not by us.** No test in this crate can catch it
(§9.4's caveat). Mitigation: documentation, and **Q-M2** asks whether it
is worth a sixth crate-level documentation obligation. It is the same
*shape* as `PLAN.md` §9's five: a hazard with no code fix that a consumer
meets by getting it wrong.

**R5 — an implementer "helpfully" adds a drop counter.** §6.5. It is the
single most natural addition to a DoS gate and §18.2 does not authorise
it. Mitigation: §6.5 says so in the plan, the code comment says so at the
site, and **Q-M2** gets it ruled either way rather than left to taste.

**R6 — the P-256-only dependency configuration hides a suite-genericity
bug** until the first consumer declares another curve, at which point the
wire is frozen. §3.6, **Q-O2**. This is the cheapest risk on the list to
retire and the most expensive to carry.

**R7 — reconciliation under time pressure (§8.5).** The failure mode is
social, not technical: a red golden test at the end of a long slice, with
the fix that makes it green being one line in the vectors file.
Mitigation: the protocol is written down before the vectors exist, the
rule lives in the vector file's own module doc, and the triage is
recorded in the handoff whatever its outcome.

**R8 — MSRV 1.96 and `const fn`.** Minor, and cheap to check early
(gate 7). Named because it is the one gate that is easy to defer and
annoying to fix late.

---

## 13. Open questions

Split as the brief asks: **(a)** decisions the orchestrator can make,
**(b)** decisions that need a maintainer ruling because they touch the
wire, the spec text, or an operator-visible contract.

### 13.a For the orchestrator — seven, one blocking

| # | Question | Recommendation | Blocking? |
|---|---|---|---|
| **Q-O1** | `PLAN.md` §1 maps §2/§3/§4 onto `packet/{mod,header,mac}.rs`. This plan adds **`suite.rs`** (§2) and **`payload.rs`** (§5.2). Accept the two declared deviations? | **Accept.** `suite.rs` because the reference suite's `channel!` invocation puts a type named `IK` into whichever module invokes it (§3.5b), and `packet::IK` reads badly; `payload.rs` because it is the crate's only big-endian integer and the file boundary is the cheapest guard (§7) | **Yes** — decides the file list |
| **Q-O2** | Add `hiss = { version = "0.3.2", features = ["x25519-cryptoxide"] }` to `[dev-dependencies]`, so a second *curve* suite can be declared in tests? | **Yes.** Under `default-features = false` hiss offers exactly one curve, so §2.3's per-suite size derivation is otherwise never executed on the slice that freezes those sizes (§3.6, R6). No runtime dependency, no version change, no wire byte | **Yes** — decides `Cargo.toml` and test 27 |
| **Q-O3** | Golden vectors as `src/packet/golden_vectors.rs` + `src/packet/tests.rs` (data / harness split, §8.1), or `tests/golden_wire.rs` with the packet types widened to `pub`? | **The `src/packet/` split.** The alternative puts wire internals under semver to suit a test. Slice 0's `tests/` convention is honoured for the one part of slice 1 that has a public surface (`tests/spec_packet.rs`) | **Yes** — the derivation agent needs to know where to write |
| **Q-O4** | The public name of the stamped reference suite. Spec says "the reference suite" and names no type | **`ReferenceSuite`**, so `Endpoint<ReferenceSuite>` reads. Alternatives: `Reference`, `P256ChaChaPolyBlake2b`, `Ik256`. Unattested either way — §15 | No |
| **Q-O5** | `channel!` syntax: "one invocation per module", enforced by the `IK` name collision and documented; or an explicit module parameter in the invocation? | **One per module, documented.** The alternative is uglier at every call site to serve a case §2.2 forecloses on one socket anyway | No |
| **Q-O6** | Confirm §9.1's three-author split, and that the **implementer ships no committed test**? | **Confirm.** Slice 1 is pure transcription; a wrong byte order round-trips perfectly against itself. This is rule 6's highest-value application in the project | No, but it shapes the agent brief |
| **Q-O7** | Add `pub use hiss;` to slither's root, so the version a consumer must match is findable? | **Yes**, with rustdoc stating plainly that the re-export does **not** remove the direct-dependency requirement of §3.5a | No |

### 13.b For the maintainer — three rulings

| # | Question | Why it cannot be an implementer's call | Recommendation |
|---|---|---|---|
| **Q-M1** | **§6.4 — is a handshake packet's length *exact* or a *minimum*?** §3.1's prose says "shorter than its type's fixed minimum"; §3.2/§3.3 declare fixed sizes ("196 bytes", "107 bytes"). Under the lenient reading a 197-byte HandshakeInit passes the gate, and §4.1's "all packet bytes preceding the tag" then has **no defined answer** — the tag could be the datagram's last 16 bytes or the 16 at the suite's fixed offset, and the spec names neither | **Wire-visible behaviour, on the slice that freezes the wire.** It changes which datagrams the DoS gate admits and it changes mac1's preimage. Working rule 3 also applies: prose and section heading disagree, and I am not defaulting to either | **Exact** for `PKT_HANDSHAKE_INIT` and `PKT_HANDSHAKE_RESP`; `>=` minimum for `PKT_DATA`. Only under the strict reading is §4.1 well-defined; it matches hiss's `&[u8; MSG1_SIZE]` API; and it is the safer default for a gate whose purpose is cheap rejection |
| **Q-M2** | **§6.5 — does the pre-AEAD gate trace?** §18.2 lists five targets, none covering the gate or mac1, and declares the list operator-visible contract ("renaming or dropping one is a protocol revision" — so *adding* one is too). But slither's committed `Cargo.toml` says "**the DoS-gate drop counters**, the roaming trace, the unlisted-static reject and the handshake give-up all surface as `tracing` events" and attributes all four to §18.2, which carries only the roaming trace | Adding a sixth target is a protocol revision; and a committed file currently asserts a §18.2 obligation §18.2 does not contain. Note the live ambiguity: "silent" (§3.1) plausibly means *nothing is signalled to the peer or the application*, which would not forbid a local trace | **Slice 1 emits nothing**, and either §18.2 gains a target by ruling or the `Cargo.toml` comment is corrected. My reading is the comment overreaches — §3.1 and §6.1's ladder both say "silent" — but a committed file making a spec claim should not be quietly reworded by an implementer |
| **Q-M3** | **§7 — ruling 64's scope paragraph is not exhaustive.** It is headed *"Scope, stated so it is not over-applied"* and names two exclusions: §8.1's varints and §2.4's static comparison. **§5.2's big-endian msg1 timestamp is a third** and is unlisted | No contradiction exists — §5.2 still says BE, and a payload is not a header field — so this is not a wire question. But the paragraph *reads* as exhaustive, on a ruling one day old, and the omission is the shape of thing the next reader over-applies. Working rule 4: the rationale, not only the token | **A one-line amendment** adding §5.2 to ruling 64's exclusion list, and a matching clause in §3.1's "Two things this rule does not reach". No value, constant, layout or behaviour moves |

**Only Q-M1 blocks implementation.** Q-M2 and Q-M3 can land after the
code; neither moves a byte. Q-M1 must be answered before `classify` is
written, because the golden vectors for the mac1 preimage extent
(§8.3 property 7) depend on the answer.

---

## 14. Handoff checklist

Per `CLAUDE.md` working rule 7 — **do not report a gate as green without
running it; paste the command and its output.**

### Pre-flight

```bash
grep -rn 'to_be_bytes\|from_be_bytes' src/packet/          # payload.rs ONLY
grep -rn 'to_le_bytes\|from_le_bytes' src/packet/          # must be EMPTY (ruling 64)
grep -rn 'RustCrypto\|sha2::\|blake2::' src/               # must be empty
grep -c 'cryptoxide' src/packet/mac.rs                     # the one raw primitive, one file
git diff --stat tests/spec_constants.rs tests/spec_errors.rs   # must be empty
```

### The eight gates

```bash
cargo build --all-features --all-targets
cargo build --no-default-features
cargo fmt --all --check
cargo clippy --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo test
cargo test --all-features
cargo test --all-features golden          # gate 6, named explicitly
cargo +1.96 check --all-features --all-targets
cargo deny check
```

### Report format

Every command with its output (or its tail plus the summary line).

**Gate 6 needs a sentence, and it is a different sentence from slice
0's:** *"the golden-wire vectors exist from this slice onward and are
byte-identical; N vectors across M groups, independently derived, plus
the compile-time size asserts. Any future red here is a ruling request."*
If any vector was corrected during reconciliation, **§8.5's triage
outcome is reported here**, with which of the three buckets it fell into.

### Hand-forward notes for slice 2

1. **S22's fourth clause** — "a wrong static … fails the handshake and
   installs nothing" — and S22's **paused-clock obligation** are slice
   2's. Slice 1 discharged neither and did not pretend to (§9.2).
2. **`Channel` has no handshake surface** (§3.3). Slice 2's plan should
   open by deciding whether to extend `Channel` or add a companion
   `Handshake` trait, and must not add a `Send` bound anywhere (S21).
3. **The prologue is slice 2's *mechanism*, slice 1's *bytes*.** See
   §15 finding F1 — a mismatched prologue is not a silent drop; it is a
   cryptographic handshake failure.
4. **`Mac1Key` is derived once per static, not per packet** (§5.3). The
   endpoint's own key is a construction-time constant; a per-peer key
   belongs to the pending/connection. If slice 2 derives one per inbound
   datagram, §4.2's cost ladder is broken and no test will notice.
5. **The gate hands out `ad` and `preimage` as subslices of the received
   datagram** (§6.2). Slice 2 and 3 must pass those through, never
   re-encode a header to rebuild the AD.
6. **`Msg1Payload` derives `Ord`** in `(secs, nanos)` order, ready for
   §5.3's strictly-greater rule and §17.1's guard.
7. **Slice 0's R10 is still open**: whether
   `hiss::noise::datagram::MAX_EPOCH_JUMP` is const-reachable under
   `default-features = false`. Slice 1 answers it in passing (§3.7); if
   not, record the outcome here.
8. **No packet type, version byte, or reserved code is dispatched** in
   slice 1 beyond the drop (§6.1). Slice 2 must not add a branch for
   `0x04`/`0x05`.

---

## 15. Findings, and what is owed to the spec

### Findings raised by writing this plan

**F1 — `PLAN.md` §4's slice-table shorthand conflates two mechanisms.**
Slice 1's row reads *"version/prologue silent drop"*. The **version**
byte is a silent drop at §3.1's gate. The **prologue** is not, and cannot
be: §5.1 binds it into the Noise transcript precisely so that *"a peer
that mis-classifies the version does not merely parse garbage, it fails
the handshake cryptographically"*. A wrong prologue therefore produces a
handshake failure in slice 2, never a gate drop in slice 1. Slice 1 owns
the prologue **constant and its golden vector**; it owns none of its
effect. Not a spec defect — `PLAN.md` §4 is a one-line summary — but an
implementer reading only that row would look for a prologue check in the
gate and either add one (wrong) or conclude the plan is incomplete.

**F2 — ruling 64's exclusion list is not exhaustive.** §7, **Q-M3**.

**F3 — `Cargo.toml`'s tracing comment asserts a §18.2 obligation that
§18.2 does not contain.** §6.5, **Q-M2**.

**F4 — under slither's own dependency configuration, hiss offers exactly
one curve**, so §2.2/§2.3's suite genericity is untestable in-crate
without a dev-dependency feature. §3.6, **Q-O2**, R6.

**F5 — `slither::channel!` requires `hiss` in the calling crate's
`Cargo.toml`.** `hiss::noise!` emits absolute `::hiss::…` paths and a
`macro_rules` wrapper cannot rewrite them. Consumer-visible, unavoidable,
and undocumented anywhere today. §3.5a, R4.

### Unattested names slice 1 introduces

Same discipline as slice 0's §15: ship them, doc-comment each as
unattested with the section that fixes the *thing*, and write the
amendment from real names once the code exists.

| Implementation name | Spec home | What the spec says instead |
|---|---|---|
| `ReferenceSuite` | §2.2 | "The reference suite is **`P256 / ChaChaPoly / Blake2b`**" — names the suite, not a type |
| `Channel::PROTOCOL_NAME` | §2.2 | "its Noise protocol name … is pinned by test" |
| `Mac1Key` | §4.1 | "`key = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)`" — names the value, not a type |
| `Msg1Payload` | §5.2 | "msg1 payload (12 B, encrypted in msg1's tail)" |
| `classify` / `Inbound` | §3.1 | describes the gate's behaviour; names no function and no result type |

**Attested and unchanged, for contrast:** `Channel`, `channel!`,
`InitHeader`, `RespHeader`, `DataHeader` — §2.2 names the first two, and
§3.2/§3.3/§3.4 name the three headers directly in their layout diagrams.

Nothing in this section changes a value, a byte, or a behaviour.


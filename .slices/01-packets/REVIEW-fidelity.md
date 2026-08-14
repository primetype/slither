# Slice 1 — fidelity review (does the code say what SPEC.md says?)

Commit `eea52d6`. Read-only review. Normative surface consulted: **§2, §3,
§4, §5.1–5.3, §17.3** (targeted `grep -n` + offset `Read`, never a whole-file
read) and rulings **64–67** from `.spec-v2-clean-slate/rulings.md`.
`.slices/01-packets/DERIVATION.md` was **not** opened — every byte below was
re-derived from spec text independently.

## VERDICT: CONFORMANT. Nothing above MINOR. No wire byte disagreed.

---

## 1. Golden wire vectors — independently derived, byte-for-byte

Method: derived every value in `src/packet/golden_vectors.rs` from spec text
alone. mac1 tags computed with Python `hashlib.blake2b` from my own reading of
§4.1 + ruling 66 (plain, no salt, no personalisation; key preimage
`MAC1_LABEL ‖ static` = 12 + 65 = 77 B; extents `[0,180)` / `[0,91)` from
ruling 65).

| Vector | Spec source | My independent derivation | Verdict |
|---|---|---|---|
| `prologue::BYTES` | §5.1 `b"slither\x01"` | `73 6C 69 74 68 65 72 01` | ✅ match |
| `mac1_label::BYTES` | §4.1 `b"slither mac1"` | `73 6C 69 74 68 65 72 20 6D 61 63 31` (12 B) | ✅ match |
| `canonical_static::BYTES` | §2.4 SEC1 uncompressed, 65 B | P-256 `G` typed from SEC 2 / FIPS 186-4; **verified on-curve** (`y²≡x³−3x+b mod p`) | ✅ match |
| `init_header::BYTES` | §3.2 + ruling 64 (LE) | `01 01 0D 0C 0B 0A` | ✅ match |
| `resp_header::BYTES` | §3.3 + ruling 64 (LE) | `02 01 0D 0C 0B 0A 44 33 22 11` | ✅ match |
| `data_header::BYTES` | §3.4 + ruling 64 (LE) | `03 01 DD CC BB AA 08 07 06 05 04 03 02 01` | ✅ match |
| `msg1_payload::BYTES` | §5.2 `ts_secs(8,BE) ‖ ts_nanos(4,BE)` | `00 00 00 00 68 00 00 00 0A 0B 0C 0D` | ✅ match |
| `mac1_init::TAG` | §4.1, preimage `[0,180)` | `C36A7CA198E62765A018C7519723C95F` | ✅ match |
| `mac1_resp::TAG` | §4.1, preimage `[0,91)` | `0D8F5F508D3DB9FE81D40E8A711BF160` | ✅ match |
| `sizes::*` (12 literals) | §2.3, §3.5 | 6·10·14·174·81·196·107·16·16·1200·1170·30 | ✅ match |

Not carried by the file but confirmed derivable: mac1 key
`C6980476C4CE2EE26D75978D7D291C1C376A393252B00C7EF783CA633620005E`.
`msg1_payload::NANOS = 0x0A0B0C0D` = 168 496 141 (< 10⁹, a legal nanosecond
remainder — the doc comment's decimal is right).

Three independent BLAKE2b readings now agree on the two tags: my Python, the
vector's Python, and `cryptoxide` (via the passing
`mac1_tag_matches_the_golden_vectors` test). Ruling 66's "plain, no salt, no
personalisation" is confirmed, not asserted — I read
`cryptoxide::hashing::blake2b::Context::new_keyed` and it is RFC 7693 plain
keyed mode (key zero-padded to one 128-byte block, parameter block carrying
only `digest_length` and `key_length`), which is precisely what
`hashlib.blake2b(…, key=…)` does.

## 2. `classify` — order, coverage, panic-freedom

Order in `src/packet/mod.rs:145`: **oversize → too-short-to-read → type →
version → the type's length rule.** §3.1's requirement (type before length,
because the accepted length is a property of the type) is met. The
`MAX_DATAGRAM` cap comes first correctly: it is §3.5's global cap, not a
property of the type.

| §3.1 drop cause | Handled | Where |
|---|---|---|
| `> MAX_DATAGRAM` (1200) | ✅ | step 1, `>` so 1200 is accepted |
| under 2 bytes (no readable type/version) | ✅ | step 2 |
| unknown type — `0x00`, `0x04` reserved-unused, `0x05` cookie, `0x06..` | ✅ | step 3, one path, no third behaviour |
| unknown version | ✅ | step 4, all three types |
| Init length ≠ `C::INIT_PACKET_LEN` (196) | ✅ exact, ruling 65 | step 5 |
| Resp length ≠ `C::RESP_PACKET_LEN` (107) | ✅ exact, ruling 65 | step 5 |
| Data length outside `30 ..= 1200` | ✅ range, ruling 65 | step 5 + step 1 |

**Ruling 67 (silence) is honoured.** `classify` returns `Option`, not a
`Result` with a drop reason; no `tracing` call, no counter, no `#[cfg]`-gated
metric anywhere in `src/packet/`. §18.2 stays at five targets. `grep`
confirms `tracing` is not imported in the module.

**Panic-freedom.** Every index and `split_at` is bounded for any suite
`channel!` can stamp: `INIT_PACKET_LEN = 6 + MSG1_LEN + 16 ≥ 22` and
`RESP_PACKET_LEN = 10 + MSG2_LEN + 16 ≥ 26`, so `len − MAC1_LEN` cannot
underflow and `preimage[..INIT_HEADER_LEN]` cannot over-index; Data's
`split_at(14)` runs only after `len ≥ 30`. `unpack` returns `Option` rather
than unwrapping. No arithmetic can overflow on the receive path. (The one
theoretical hole is a hand-written `Channel` — see NIT-1.)

## 3. mac1 preimage and AEAD associated data are the *received* bytes

Confirmed. `Inbound::Init.preimage` is `&dgram[..len−16]`,
`Inbound::Resp.preimage` is the same, and `Inbound::Data.ad` is
`&dgram[..14]` — all **subslices of the caller's datagram**, produced by
`split_at`. The decoded `header` is carried *alongside*, never re-encoded
into the AD or the preimage. `grep` over `src/packet/{mod,header,mac,suite}.rs`
finds **zero** `to_le_bytes` / `to_be_bytes` / `pack` calls; the only
hand-written byte-order code in the whole module is `payload.rs`'s
big-endian timestamp, which is §5.2's deliberate exception (ruling 64's
third exclusion). §3.4's "verbatim" is satisfied literally.

## 4. Crypto rules (CLAUDE.md)

| Rule | Status |
|---|---|
| Every Noise/curve op through `hiss` | ✅ `src/packet/` performs no curve or AEAD work at all; `channel!` declares the IK via `::hiss::noise!` |
| The ONE raw primitive is mac1's keyed BLAKE2b from `cryptoxide` | ✅ `mac.rs` is the only `cryptoxide` import in the crate; no `hiss::noise::Hash` appears there, so §4.4's "mac1 does not follow the suite Hash" holds structurally |
| No RustCrypto | ✅ `cargo tree --all-features -e normal` matches none of `sha2`/`digest`/`aes`/`elliptic-curve`/`generic-array`/`crypto-common`/`poly1305`/`p256`/`curve25519-dalek` |
| No `Send` bounds on the actor path | ✅ `grep -n "Send\|Sync"` over `src/packet/*.rs` (excl. tests): zero hits |
| `cryptoxide` pinned to exactly hiss's range | ✅ slither `">=0.6.0, <0.7"`; hiss 0.3.2 `">=0.6.0, <0.7"` — verified against hiss's own `Cargo.toml` |
| `rand_core` on hiss's line | ✅ both `0.10` |
| MSRV in lockstep with hiss | ✅ both `rust-version = "1.96"` |

`Cargo.toml`'s only slice-1 change is a **dev**-dependency
`hiss = { version = "0.3.2", features = ["x25519-cryptoxide"] }`. Its comment
claims "no crate to the graph": verified — hiss's `default = ["x25519-cryptoxide"]`
and that feature is `cryptoxide/x25519`, and `cryptoxide` is already a direct
dependency. The claim is exact.

The §18.2 `Cargo.toml` comment ruling 67 was about now reads correctly and
**names its own former error** — and it was corrected in `720e8b0` (the ruling
commit), not quietly by the implementer in `eea52d6`. Working rule 5 held.

## 5. `channel!` against §2.2 / §2.3

- **`macro_rules`, not a proc-macro** ✅ (§2.2 names the mechanism).
- **IK token block hardcoded** ✅ — `<- s / ... / -> e, es, s, ss [12] / <- e, ee, se`,
  which is Noise IK exactly, with §5.2's 12-byte payload on msg1 and **no**
  payload declaration on msg2.
- **No suite byte on the wire** ✅ — no header carries one, and `classify`
  never asks which suite a datagram is; §2.2's "dies at the length gate or at
  mac1, the same fate as garbage" is what the code does (an X25519 Init is
  130 B and fails `!= 196` outright).
- **Sizes genuinely derived, not hardcoded** ✅ — the macro writes §2.3's
  formula *as* the definition, then pins it two ways in **every** expansion:
  `MSG1_LEN == IK::MSG1_SIZE` and `MSG2_LEN == IK::MSG2_SIZE` against hiss's
  own computed sizes. That second assertion is also what ties the `[12]`
  literal in the token block (which `hiss::noise!` requires to be a literal)
  back to `constants::MSG1_PAYLOAD_LEN`. `tests/spec_packet.rs` exercises a
  real second data point over X25519 (`PK = 32` → 108 / 48 / 130 / 74).
- **`PROTOCOL_NAME`** ✅ built from hiss's own `NAME` constants in the same
  order hiss uses internally — I read `hiss-0.3.2/src/noise/handshake.rs:83`,
  `format!("Noise_{}_{}_{}_{}", Pattern::NAME, Curve::NAME, Cipher::NAME, Hash::NAME)`.
  `Noise_IK_P256_ChaChaPoly_BLAKE2b` is pinned by test, as §2.2 requires.
- **§2.4's bound** ✅ `type Curve: DhCurve<PublicKey: AsRef<[u8]>>` — slither-side,
  `Ord` deliberately absent. Verified that `hiss`'s `P256r1PublicKey: AsRef<[u8]>`
  yields the 65 uncompressed SEC1 octets, i.e. §2.4's canonical encoding.

## 6. Release gates — run on this tree, output pasted

```
cargo build --all-features --all-targets   → Finished dev profile              (exit 0)
cargo fmt --all --check                    → no diff                            (exit 0)
cargo clippy --all-features --all-targets -- -D warnings → Finished, no warnings (exit 0)
RUSTDOCFLAGS=-D warnings cargo doc --no-deps                → Generated .../index.html (exit 0)
RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features → Generated .../index.html (exit 0)
cargo test                    → 65 + 103 + 11 + 4 + 4 = 187 passed, 0 failed
cargo test --all-features     → 65 + 103 + 11 + 4 + 4 = 187 passed, 0 failed
cargo +1.96 check --all-features --all-targets → Finished dev profile           (exit 0)
cargo deny check              → advisories ok, bans ok, licenses ok, sources ok (exit 0)
```

---

## Findings

### BLOCKER — none.
### MAJOR — none.

### MINOR-1 — `classify`'s Data lower bound uses the *reference suite's* tag size, not `C::AEAD_TAG_LEN`; §2.3 and §3.5 disagree about whether that is right. **Reporting, not resolving.**

`src/packet/mod.rs:213`

```rust
if dgram.len() < constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN {
```

`constants::AEAD_TAG_LEN` is documented in `src/constants.rs:118` as *"Bytes
of ChaCha20-Poly1305 authentication tag"* — the reference suite's value.
`classify` is generic over `C: Channel` **precisely so** the handshake lengths
are per-suite (its own doc says so), and `C::AEAD_TAG_LEN = Cipher::TAG_SIZE`
is in scope and unused.

The two spec statements in tension:

* **§2.3** defines `TAG` as *"the suite AEAD's tag size"* and lists
  `AEAD_TAG_LEN` = 16 under **"Reference-suite values"**; §2.2 names *"in
  principle, the AEAD tag size"* as a **wire-visible consequence of the
  suite**. On this reading §3.1's Data row `DATA_HEADER_LEN + AEAD_TAG_LEN`
  is per-suite and the code is reference-suite-specific.
* **§3.5** states `MAX_PLAINTEXT` = 1170 (= `MAX_DATAGRAM` − 14 − 16) as a
  flat protocol constant with no suite qualification, and §3.1 annotates the
  Data row "(30)". On this reading 30 and 1170 are protocol constants and the
  code is right.

Per working rule 1 I am not picking. **Unreachable today** — hiss 0.3.2 ships
exactly one `Cipher` (`ChaChaPoly`, `TAG_SIZE = 16`), and `suite.rs:339` pins
`ReferenceSuite::AEAD_TAG_LEN == constants::AEAD_TAG_LEN`. It becomes a live
wire divergence the day hiss gains a second AEAD, and the fix is not a
one-token patch: if the Data floor is per-suite then `MAX_PLAINTEXT` is too,
and §3.5 says it is not. That makes it a **ruling**, not a code change.

Note this is the same *shape* as rulings 65 and 66 — an unstated parameter of
a stated construction — one layer out: §2.3 says which sizes vary by suite,
§3.5 tabulates two of them as fixed, and nothing reconciles the two.

### NIT-1 — the gate's "no panic" property depends on `channel!`, and `Channel` is publicly hand-implementable.

`mod.rs` states the principle explicitly twice (*"a panic in the DoS gate is
a worse failure than a dropped packet"*, `mod.rs:229` and `:238`). For any
macro-stamped suite the property holds structurally. But `pub trait Channel`
is unsealed, and `suite.rs:33` concedes *"Implementing this trait manually is
possible and pointless"* — a hand impl with, say, `INIT_PACKET_LEN = 10` makes
`dgram.len() − constants::MAC1_LEN` underflow on a 10-byte `0x01 0x01 …`
datagram, i.e. a panic in the DoS gate reached from the network. Sealing
`Channel`, or a `saturating_sub`, closes it. No spec text is violated (§2.2
does not forbid hand impls) and no reachable configuration is affected today.

### NIT-2 — `slither::packet::suite::IK` is public, unattested API.

`channel!` forwards `$vis` to `hiss::noise!`, so slither's own
`pub ReferenceSuite` invocation exports a `pub struct IK` at
`slither::packet::suite::IK`. Not wrong (§2.2 names no type, and slice 2 will
want it), and deliberate enough that the macro doc lists it as one of the
three stamped items — but it is public surface that no spec section attests,
now shipped alongside a frozen wire.

---

## Things I checked and found clean, listed so a later reader knows they were checked

* No stale pre-ruling-64 prose survives: `grep -in "big-endian"` over
  `SPEC.md` returns only §3.1/§3.4's *deliberate* exclusions (the msg1
  timestamp, §8.1's varints, the AES-GCM aside) and §8.1 itself. No header
  field is still described as big-endian anywhere.
* No stale pre-ruling-65 prose survives: `grep -in "fixed minimum|shorter
  than|length-correct|short/oversize"` returns only §5.5's *"length-correct"*
  (line 876), which the exact gate satisfies, and an unrelated §7.5 sentence.
  §3.1's "fixed minimum" wording and §6.2's "short/oversize" are gone.
* §6.9 (DoS accounting) is a per-packet **cost** table and imposes no counter
  or observability obligation — no conflict with ruling 67's silence.
* §17.3's nonzero-index rule is a **minting** rule; §3.1's drop list does not
  include a zero index, and `InitHeader`/`RespHeader`/`DataHeader` correctly
  enforce nothing about it. `header.rs:30` says so explicitly.
* §3.4's empty-plaintext keepalive is the 30-byte minimum and is correctly
  *not* special-cased in the gate — telling it apart is post-AEAD work.
* `Msg1Payload`'s derived `Ord` is over `(secs, nanos)` in field order, which
  is chronological — the comparison §5.3 and §17.1 need.
* `PROTOCOL_NAME_CAP = 96` cannot silently truncate: `append`'s `assert!` runs
  in a `const` context, so overflow is a build failure. Longest realistic name
  is ~36 bytes.
* `Blake2b::<128>` / `MAC1_LEN` drift is a build failure
  (`mac.rs:121`), not the runtime `finalize_at` panic it would otherwise be.
* Every golden vector is consumed by at least one test; none is decorative.

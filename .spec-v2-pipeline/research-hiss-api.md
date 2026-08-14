# hiss generated surface, the split-read proposal, the fallback, and the Connection object

Research for slither v0.2 (TODO.md §3/§4). Read-only survey of
`/Users/nicolasdiprima/work/primetype/slither` and
`/Users/nicolasdiprima/work/primetype/hiss` (hiss 0.3.1 / hiss-macros 0.3.0).
All line references are to those working trees as of 2026-08-13.

---

## 1. Generated surface for slither's IK

### 1.1 The invocation

`/Users/nicolasdiprima/work/primetype/slither/src/handshake.rs:64-81`

```rust
hiss::noise! {
    pub IK<P256, ChaChaPoly, Blake2b> {
        <- s
        ...
        -> e, es, s, ss [12]
        <- e, ee, se
    }
}
```

Parsed (`hiss-macros/src/parse.rs:186-340`) into:

- `pre_messages = [ {dir: ToInitiator, tokens: [S], payload: None} ]`
- `messages = [ {ToResponder, [E, Es, S, Ss], payload: Some(12)},
                {ToInitiator, [E, Ee, Se], payload: None} ]`

Suite mode is selected because a `<Curve, Cipher, Hash>` list is present
(`codegen.rs:196-201`), so the full state-machine expansion runs
(`codegen.rs:231-267`).

### 1.2 Everything the macro emits, in order

`expand_suite` (`codegen.rs:250-266`) emits seven blocks:

1. **Marker struct** — `pub struct IK;` with `#[derive(Debug, Clone, Copy, Default)]`
   (`codegen.rs:309-314`). Slither re-exports it as `pub type SlitherChannel = handshake::IK;`
   (`slither/src/lib.rs:83`).
2. **`Pattern` impl** (`codegen.rs:323-329`): `NAME = "IK"`, `NUM_MESSAGES = 2`,
   plus the type-level `PreMessages`/`Messages` cons-lists. `NAME` is what makes
   the transcript read `Noise_IK_P256_ChaChaPoly_BLAKE2b`
   (built in `hiss/src/noise/handshake.rs:82-88`; pinned by slither's test at
   `slither/src/handshake.rs:604-619`).
3. **`WellFormed` assertion** — a `const _: fn() = || { … }` that fails to compile
   on a Noise §7.3-invalid pattern (`codegen.rs:331-337`).
4. **`Protocol` impl** (`codegen.rs:343-350`): `type Pattern = IK; type Curve = P256;
   type Cipher = ChaChaPoly; type Hash = Blake2b;`. **This is the single type
   parameter every post-handshake type is generic over** — see §4.
5. **Wire-size consts** in `impl IK` (`codegen.rs:357-425`):
   `pub const MSG1_SIZE: usize` (= 174) and `pub const MSG2_SIZE: usize` (= 81),
   plus hidden `__KEYED_BEFORE_MSG{1,2,3}: bool`. The macro does **no** size
   arithmetic itself — sizes are const expressions over `hiss::noise::WireSize`
   (`codegen.rs:369`, `400-418`), with the declared payload length added as a bare
   `+ 12` term (`codegen.rs:376-393`). Slither const-asserts these against its
   ratified wire at `slither/src/handshake.rs:97-98` and `slither/src/wire.rs:295-298`.
6. **Entry points** in `impl IK` (`codegen.rs:449-540`).
7. **Two role state machines** (`codegen.rs:555-596`).

### 1.3 Exact signatures

Write `Pk = <P256 as hiss::curve::Curve>::PublicKey` (= `P256r1PublicKey`) and
`Sk = <CP as hiss::provider::CryptoKeyProvider<P256>>::PrivateKey`.

**Constructors** (`codegen.rs:449-540`). Fallibility is decided by whether the
role owns a *local* pre-message static (`codegen.rs:457`, `premessage_params` at
`codegen.rs:433-447`): the `<- s` line is in the responder's send direction, so
the responder supplies a private key and its constructor is fallible; the
initiator supplies the peer's public key and its constructor is infallible.

```rust
impl IK {
    pub fn initiator<CP>(provider: CP, prologue: &[u8], remote_static: Pk)
        -> IKInitiatorMsg1<CP>
    where CP: DhProvider<P256>;

    pub fn responder<CP>(provider: CP, prologue: &[u8], static_key: Sk)
        -> Result<IKResponderMsg1<CP>, HandshakeError>
    where CP: DhProvider<P256>;
}
```

**State types** (`codegen.rs:120-123` names them, `677-707` defines them):

| Type | Awaiting | Notes |
|---|---|---|
| `IKInitiatorMsg1<CP>` | write msg1 | |
| `IKInitiatorMsg2<CP>` | read msg2 | slither holds it in `InitiatorPending` (`handshake.rs:251-260`) |
| `IKResponderMsg1<CP>` | read msg1 | |
| `IKResponderMsg2<CP>` | write msg2 | slither holds it in `RespAccept` (`handshake.rs:367-373`) |

Each is `pub struct X<CP> where CP: CryptoKeyProvider<P256> { inner: HandshakeInner<P256, ChaChaPoly, Blake2b, CP> }`
— one private field, no `Clone`, no `Debug`, visibility inherited from the
invocation's `pub`.

**Mid-handshake accessors** (`codegen.rs:602-675`), emitted only on states where
the key is *guaranteed* present (hence `&Pk`, not `Option`):

- `IKInitiatorMsg1`: `remote_static()` (from the pre-message).
- `IKInitiatorMsg2`: `local_ephemeral()`, `remote_static()`.
- `IKResponderMsg1`: **none** — for the responder the `<- s` pre-message is its
  *own* static, so `remote_s` is still `None` at this state (`codegen.rs:565-571`).
- `IKResponderMsg2`: `remote_ephemeral()`, `remote_static()`. Slither uses the
  latter at `slither/src/handshake.rs:492`.

**Methods** (`codegen.rs:856-928` for writes, `987-1200` for reads). Every one
takes `mut self` **by value**:

```rust
impl<CP: DhProvider<P256>> IKInitiatorMsg1<CP> {
    pub fn write_message_1(mut self, static_key: Sk, payload: &[u8; 12])
        -> Result<([u8; IK::MSG1_SIZE], IKInitiatorMsg2<CP>), HandshakeError>;
}

impl<CP: DhProvider<P256>> IKInitiatorMsg2<CP> {
    pub fn read_message_2(mut self, message: &[u8; IK::MSG2_SIZE])
        -> Result<Transport<IK>, HandshakeError>;      // final message → Transport
}

impl<CP: DhProvider<P256>> IKResponderMsg1<CP> {
    pub fn read_message_1(mut self, message: &[u8; IK::MSG1_SIZE])
        -> Result<([u8; 12], IKResponderMsg2<CP>), HandshakeError>;

    pub fn read_message_1_with(
        mut self,
        message: &[u8; IK::MSG1_SIZE],
        verify: impl FnOnce(&Pk) -> Result<(), HandshakeError>,
    ) -> Result<([u8; 12], IKResponderMsg2<CP>), HandshakeError>;
}

impl<CP: DhProvider<P256>> IKResponderMsg2<CP> {
    pub fn write_message_2(mut self)
        -> Result<([u8; IK::MSG2_SIZE], Transport<IK>), HandshakeError>;
}
```

No `read_message_2_with` is emitted: msg2 carries no `s` token
(`verify_on_read`, `codegen.rs:954-956`; dispatch at `codegen.rs:970-975`).
Note the payload parameter/return: writer takes `&[u8; 12]` **last**
(`codegen.rs:899-906`), reader returns `[u8; 12]` **by value** alongside the next
state (`codegen.rs:1166-1177`).

### 1.4 `read_message_1_with`'s exact body and the closure's firing point

The token loop (`codegen.rs:1094-1160`) emits straight-line code in pattern order.
For `-> e, es, s, ss [12]` under `ReadStyle::Verify` the expansion is exactly:

```rust
let cursor = 0usize;
let (_remote_ephemeral, n) = support::recv_e(&mut self.inner, &message[cursor..])?;   // 1096-1099
let cursor = cursor + n;                                     //   65 bytes, mix_hash only
support::es_responder(&mut self.inner)?;                                              // 1153-1158
let (remote_static, n) = support::recv_s(&mut self.inner, &message[cursor..])?;       // 1117-1120
let cursor = cursor + n;                                     //   81 bytes = 65 + 16 tag
verify(&remote_static)?;                                     // ◀── THE CLOSURE, 1121
support::ss(&mut self.inner)?;                                                        // 1153-1158
let mut payload = [0u8; 12];
support::recv_tail(&mut self.inner, &message[cursor..], &mut payload)?;               // 1173
Ok((payload, IKResponderMsg2 { inner: self.inner }))                                  // 1197
```

So the closure fires **after `recv_e` + `es` + `recv_s`, strictly before `ss`**
and therefore strictly before the payload tail is touched. That is exactly what
slither documents (`slither/src/handshake.rs:24-43`, `429-446`) and pins
(`slither/src/handshake.rs:832-899`); hiss pins the same property twice
(`hiss/tests/noise_macro_shapes.rs:588-621` "reject costs exactly the one `es` DH",
and `623-675` "`es` has run; `ss` has not").

`verify` is `impl FnOnce(&Pk) -> Result<(), HandshakeError>` (`codegen.rs:1112-1116`);
returning `Err` aborts the whole read and the half-advanced state is dropped
(it was moved into the method). The generated doc text spells the contract out:
*"the identity is **claimed, not yet proven**… rejecting is always safe, but side
effects in the closure must not treat the key as authenticated"* (`codegen.rs:1061-1077`),
plus, when a payload is declared, *"the payload is only ever returned from an
accepted read"* (`codegen.rs:1084-1090`).

**Transcript mixing order on msg1** (responder side), from `support`/`process`:

| Step | Support fn | Symmetric-state effect | Cost |
|---|---|---|---|
| construct | `new_handshake` (`support.rs:52-61` → `handshake.rs:78-105`) | `initialize("Noise_IK_P256_ChaChaPoly_BLAKE2b")` then `mix_hash(prologue)` | name is 32 B ≤ 64 B `HASH_LEN`, so **no hash** — memcpy+pad (`symmetric_state.rs:80-95`); 1 BLAKE2b for the prologue |
| pre-message `s` | `set_s` (`support.rs:81-100`) | `provider.public_key(static_key)`, `mix_hash(s_pub)` | **1 fixed-base scalar mult** (`curve/p256/software.rs:104-112`, `Point::mul_base`) + 1 BLAKE2b |
| `e` | `recv_e` (`support.rs:207-222` → `process.rs:180-210`) | parse 65 B, canonical re-encode check, `mix_hash(re)` | point decode, 1 BLAKE2b, **no DH** |
| `es` | `es_responder` (`support.rs:278-299`) | `mix_key(DH(s, re))` → cipher now keyed, `n = 0` | **1 DH** + HKDF-2 (3 HMAC) |
| `s` | `recv_s` (`support.rs:227-242` → `process.rs:240-278`) | `decrypt_and_hash(81 B)` → AEAD open at `n = 0`, then `mix_hash(ciphertext)`; canonical check | 1 ChaChaPoly open, `n → 1` |
| *(closure)* | — | — | — |
| `ss` | `ss` (`support.rs:382-401`) | `mix_key(DH(s, rs))` → **fresh** `CipherState`, `n` back to 0 | **1 DH** + HKDF-2 |
| tail | `recv_tail` (`support.rs:449-462` → `process.rs:97-112`) | `decrypt_and_hash(28 B)` → 12 B payload + tag verify, `mix_hash(ciphertext)` | 1 ChaChaPoly open |

msg1 byte layout: `[0..65)` plaintext `e`, `[65..146)` encrypted `s` (65 + 16),
`[146..174)` encrypted payload (12 + 16) = 174 = `IK_MSG1_LEN`
(`slither/src/wire.rs:81-83`, `295`; slither's tamper test hard-codes the same
offsets at `slither/src/handshake.rs:797-800`).

### 1.5 Consumption semantics, construction cost, what is cheap to rebuild

- **Everything consumes.** Constructors take `provider` and `static_key` by
  value; every `write_message_N` / `read_message_N[_with]` takes `mut self`.
  This is deliberate: "an error is terminal … enforced only by ownership"
  (`support.rs:24-31`, `process.rs:17-28`). A failed read therefore *cannot* be
  retried on the same state — the state is gone.
- **Constructing `IKResponderMsg1` costs no DH**, but it is not free: it is
  **one fixed-base P-256 scalar multiplication** (deriving our static's public
  half for the pre-message `mix_hash`) plus 2 BLAKE2b compressions and one
  `SymmetricState::initialize` memcpy. The mult goes through
  `CryptoKeyProvider::public_key`, *not* `DhProvider::dh`, so slither's
  `CountingProvider` (which only counts `dh`, `slither/src/testutil.rs:74-83`)
  does **not** see it — the "1 DH" ruling counts DH only.
  For a hardware-backed static this call could be a keychain/enclave round-trip,
  which matters for the fallback (§3).
- **Cheap to rebuild**: the symmetric state (hash init + 2 mix_hash) and the
  scalar re-parse (`SoftwareIdentity::static_secret`, `slither/src/handshake.rs:213-216`
  → `P256r1PrivateKey::from_bytes`, a range check only). **Not** free to rebuild:
  the `d·G` mult, and (in slither's plumbing) a fresh `Identity::provider()` draw,
  which advances the master CSPRNG — see the golden-wire caveat in §3.5.
- The responder generates **no ephemeral** while reading msg1; its `e` is created
  only inside `write_message_2` (`send_e`, `support.rs:154-182`). So a msg1 read
  consumes no randomness at all.
- `HandshakeInner` (`hiss/src/noise/handshake.rs:36-62`) holds: `symmetric`,
  `e`/`e_pub`/`s`/`s_pub`/`re`/`rs`, `has_psk`, `provider`.

### 1.6 Transport / datagram conversion

`write_message_2` / `read_message_2` return `Transport<IK>` via
`into_transport` (`support.rs:466-475` → `process.rs:122-140`), which snapshots
the handshake hash as the `SessionId`, then `split()`s the symmetric state into
two `CipherState`s and assigns them by role (`process.rs:135-139`).

Slither never uses the stream `Transport::send/receive`; both sides immediately
call `transport.into_datagram_with_epoch(REKEY_EPOCH_MSGS)`
(`slither/src/handshake.rs:348` and `:414`) — see §4.

---

## 2. The split-read proposal, concretely

### 2.1 Where the boundary sits, and whether it is a safe checkpoint

The `es`/`ss` boundary is the line `verify(&remote_static)?;` in §1.4 — i.e.
after `recv_s` returns and before `support::ss` is called. **It is a clean Noise
token boundary, and the symmetric state there is fully consistent.**

At that instant:

- **Mixed into `h`**: protocol name, prologue, our static public key, the peer's
  ephemeral `re`, and the 81-byte `s` ciphertext. (5 items, in that order.)
- **Mixed into `ck`**: exactly one `mix_key`, the `es` shared secret.
- **`CipherState`**: keyed from the `es`-derived `temp_k`, nonce `n == 1`
  (advanced by the `s` decrypt at `cipher_state.rs:197-231`).
- **Known keys**: `s`, `s_pub`, `re`, `rs` (claimed). No `e` yet.
- **Still to run**: `ss` (which replaces the `CipherState` wholesale via
  `mix_key` → `CipherState::from_key`, `symmetric_state.rs:109-118`, so the
  `n == 1` above is discarded), then the tail decrypt.

Nothing is half-applied: the "an error leaves the state half-advanced" warning
(`process.rs:17-28`) is about a *failed* step, not about pausing at a completed
one. Pausing here is exactly what `read_message_1_with` already does — it runs
arbitrary caller code at this point today. So the split is a **pure refactor of
control flow**: `intro` + `complete` would execute the identical `support::*`
calls in the identical order, so the transcript, the golden wire bytes and the
session id are unchanged **by construction**.

### 2.2 Sketch of what the macro would emit

Predicate: emit the split exactly where the `_with` verify variant is emitted
today — `verify_on_read(line)` (`codegen.rs:954-956`), i.e. the message reveals
the peer's `s` and is not the psk-after-`s` (lookup) shape. Dispatch would go
next to `codegen.rs:970-975`.

```rust
// ── new state type, named off the existing scheme (codegen.rs:120-123) ──
#[doc = "…paused mid-read of message 1: `e, es, s` are done, `ss` and the
         payload tail are not. The static is CLAIMED, not proven."]
pub struct IKResponderMsg1Mid<CP>
where CP: hiss::provider::CryptoKeyProvider<P256>
{
    inner: hiss::noise::support::HandshakeInner<P256, ChaChaPoly, Blake2b, CP>,
    tail:  [u8; IK::__MSG1_TAIL_SIZE],     // 28 for this pattern
}

impl<CP: DhProvider<P256>> IKResponderMsg1<CP> {
    pub fn read_message_1_intro(
        mut self,
        message: &[u8; IK::MSG1_SIZE],
    ) -> Result<(Pk, IKResponderMsg1Mid<CP>), HandshakeError> {
        let cursor = 0usize;
        let (_re, n) = support::recv_e(&mut self.inner, &message[cursor..])?;
        let cursor = cursor + n;
        support::es_responder(&mut self.inner)?;
        let (remote_static, n) = support::recv_s(&mut self.inner, &message[cursor..])?;
        let cursor = cursor + n;
        let mut tail = [0u8; IK::__MSG1_TAIL_SIZE];
        tail.copy_from_slice(&message[cursor..]);
        Ok((remote_static, IKResponderMsg1Mid { inner: self.inner, tail }))
    }
}

impl<CP: CryptoKeyProvider<P256>> IKResponderMsg1Mid<CP> {
    /// The CLAIMED static. Not authenticated until `complete()` succeeds.
    pub fn claimed_static(&self) -> &Pk { … }   // support::remote_static
}

impl<CP: DhProvider<P256>> IKResponderMsg1Mid<CP> {
    pub fn complete(mut self)
        -> Result<([u8; 12], IKResponderMsg2<CP>), HandshakeError>
    {
        support::ss(&mut self.inner)?;
        let mut payload = [0u8; 12];
        support::recv_tail(&mut self.inner, &self.tail, &mut payload)?;
        Ok((payload, IKResponderMsg2 { inner: self.inner }))
    }
}
```

Three carrier options for the un-read tail, since `MidRead` cannot borrow the
message without dragging a lifetime into the parked state:

- **(a) owned fixed array** (shown above). Cleanest for slither — no lifetime,
  no re-supply. Needs one new hidden const, `__MSGn_TAIL_SIZE`, and the macro
  currently refuses to do size arithmetic itself (`codegen.rs:16-19`), so it must
  come from `WireSize` applied to the *prefix* token list `Cons<E, Cons<Es, Cons<S, Nil>>>`,
  subtracted from `MSGn_SIZE`, respecting the same `__KEYED_BEFORE_MSGn` flag
  (mirroring `gen_sizes`, `codegen.rs:357-425`). ~25 extra lines, mechanical.
- **(b) `MidRead<'a, CP>` borrowing `&'a [u8; MSGn_SIZE]`** — zero copy, but a
  parked `Intro`/`Claimed` object then borrows the packet buffer. Wrong shape for
  slither's accept queue.
- **(c) `complete(self, message: &[u8; MSGn_SIZE])`** — no new const, no copy,
  but the caller must re-supply *the same* message (a mismatch is caught by the
  tail tag, so it degrades to `DecryptionFailed`, but it is a footgun). Lowest
  effort if you want the change small.

Recommendation: **(a)**, with (c) as the escape hatch if the `WireSize` prefix
const proves fiddly.

### 2.3 Hazards

1. **Zeroisation of a dropped MidRead — fine, with one wart.** Dropping the mid
   state drops `HandshakeInner`, whose fields self-scrub: `SymmetricState::drop`
   zeroes `ck` (`symmetric_state.rs:202-208`), `CipherState::drop` zeroes `k`
   (`cipher_state.rs:315-322`), `P256r1PrivateKey::drop` zeroes the scalar
   (`curve/p256/software.rs:180-184`). **Wart**: `SymmetricState::drop`'s comment
   says "h … zero it anyway for defence in depth" but the code does not zero `h`.
   Pre-existing (affects every dropped handshake today, not just MidRead), and `h`
   is not secret — worth a one-line fix in hiss while you are in there.
2. **A parked MidRead is live key material.** It holds our static private key, the
   provider, the `es`-derived chaining key and cipher key. Rough footprint:
   two 64-byte `Vec`s (BLAKE2b `ck`/`h`) + 32-byte key + `ChaCha20Rng` (~136 B) +
   two 65-byte P-256 public keys + the static scalar ≈ **0.5–1 KB**, versus 196 B
   for a raw parked packet. TODO.md's decision to park **stage-0 objects only**
   (raw packet + addr) is the right one and should stay; the `Claimed` stage should
   be short-lived and carry the same deadline as `Intro`.
3. **The claimed identity is attacker-chosen.** The `s` ciphertext is sealed under
   the key derived from `es = DH(responder_static, e)`, and *anyone* who knows the
   responder's public static (it is public, and it also keys mac1,
   `slither/src/handshake.rs:461-467`) can pick their own `e` and encrypt an
   arbitrary claimed static. So reaching `Claimed` requires no secret at all.
   Consequences for slither's staged accept: nothing durable may be keyed on the
   claimed static — no map insertion, no rate-limit bucket, no logging that grows
   unbounded. (Today slither is correct here: `TimestampGuard` and
   `static_to_conn` are only touched post-`ss`, `slither/src/endpoint.rs:719-736`.)
   The closure form at least carries the "claimed, not yet proven" doc inline
   (`codegen.rs:1061-1077`); a bare `-> (Pk, MidRead)` return loses that signal.
   **Recommend a newtype** (`hiss::noise::Claimed<K>` with `.into_claimed()` /
   `.as_ref()`), or at minimum name the accessor `claimed_static()` rather than
   `remote_static()`.
4. **Payload placement is preserved.** The tail decrypt stays inside `complete()`,
   after `ss` — so the 12-byte timestamp still only ever emerges from a completed,
   authenticated read. The existing generated guarantee (`codegen.rs:1084-1090`)
   carries over verbatim.
5. **Keep `MidRead` non-`Clone`** (consistent with every other generated state) —
   two completions from one msg1 would be confusing rather than unsafe, but there
   is no reason to allow it.
6. **API-surface growth is unconditional.** There is no DSL syntax to opt in, so
   the split would be emitted for *every* `s`-revealing read in every pattern
   (`X`, `IX`, `Xpsk0`, `IK`, …), each gaining a public type. That is additive
   and semver-safe, but it doubles the reader surface in the generated docs.

### 2.4 Size of the hiss change

**Zero runtime change.** `support` already exposes every primitive the split
needs — `recv_e`, `es_responder`, `recv_s`, `ss`, `recv_tail`, `into_transport`
(`support.rs:207`, `278`, `227`, `382`, `449`, `466`). Nothing in
`hiss/src/noise/**` moves, so no new crypto surface and no risk to the engine.

Confined to `hiss-macros`:

| Work | Est. |
|---|---|
| Factor `gen_read_method`'s token loop (`codegen.rs:1092-1160`) into a helper emitting a token *subrange* | ~40 lines moved |
| `gen_split_read`: mid-state struct, `_intro`, `complete`, `claimed_static` | ~120 lines |
| Generated doc text for the three new items (matching the existing prose density) | ~60 lines |
| `__MSGn_TAIL_SIZE` const in `gen_sizes` (option (a) only) | ~25 lines |
| Tests in `tests/noise_macro_shapes.rs`: DH counts 1→2 across the split, session-id equality with the plain read, drop-at-mid leaves nothing | ~100 lines |
| Optional `Claimed<K>` newtype in `hiss/src/noise` + re-export | ~40 lines |

**≈ 300–450 lines, one crate, purely additive → a hiss 0.3.x minor.** The
strongest argument for landing it: because it re-uses the same helpers in the
same order, byte-compatibility is provable by inspection rather than by test.

---

## 3. The fallback, mechanically

### 3.1 Stage 1 — record the claim, then reject

```rust
let mut claimed = None;
let outcome = IK::responder(provider_1, PROLOGUE, identity.static_secret())?
    .read_message_1_with(msg1, |c| {
        claimed = Some(*c);
        Err(HandshakeError::PeerRejected { reason: "staged".into() })
    });
debug_assert!(matches!(outcome, Err(HandshakeError::PeerRejected { .. })));
```

Work performed, per §1.4/§1.5: state build (1 fixed-base mult, 2 BLAKE2b),
`recv_e` (point decode + 1 BLAKE2b), `es` (**1 DH** + HKDF), `recv_s`
(1 ChaChaPoly open of 65 B + point decode), closure → `Err`, abort.
`ss` never runs; the payload tail is never touched.

**A rejected read leaves no residue — verified from code:**

- The read consumed `self` by value (`codegen.rs:1189-1191`), so the whole
  `IKResponderMsg1<CP>` — `HandshakeInner`, provider, static key, symmetric state
  — is dropped when the method frame unwinds on `?`. Scrubbing is as in §2.3(1).
- Nothing outside the state is touched. The `es` DH reads `inner.s`/`inner.re`
  and writes only `inner.symmetric` (`support.rs:278-299`); `recv_s` writes only
  `inner.rs` and `inner.symmetric` (`process.rs:240-278`). The provider is
  per-handshake and `&self` for `dh` (`provider/mod.rs:309-317`).
- In slither, the allow-list is a `&HashSet` and the `TimestampGuard` is only
  consulted after an accepted read (`slither/src/endpoint.rs:719-722`), so a
  rejected read cannot pollute either. Slither's own test already proves the
  no-poisoning property end to end: a tampered msg1 fails, then a clean msg1
  from the same peer succeeds (`slither/src/handshake.rs:780-830`).
- The read is therefore **idempotent and side-effect free**: re-running it in
  stage 2 on the same bytes reproduces the identical transcript.

**Is rebuilding the reader state cheap?** Hashing-wise yes (no protocol-name
hash, 2 BLAKE2b). Curve-wise **no, not free**: `IK::responder` → `set_s` →
`provider.public_key(static)` → `d·G` (`support.rs:81-100`,
`curve/p256/software.rs:104-112`). Software P-256 `mul_base` is a fixed-base comb
(~4× faster than a variable-base mult per its own comment), so it is roughly
¼ of a DH — real but small. For a hardware static it could be a syscall.

### 3.2 Stage 2 — re-run the whole read, this time accepting

```rust
let (payload, msg2_state) = IK::responder(provider_2, PROLOGUE, identity.static_secret())?
    .read_message_1_with(msg1, |c| if admitted(c) { Ok(()) } else { Err(…) })?;
```

Redone: state build (1 mult), `recv_e`, `es` (**1 DH**), `recv_s`. New: `ss`
(**1 DH**), tail decrypt → the timestamp. Then `RespAccept::accept` →
`write_message_2` → `send_e` (ephemeral keygen + 1 mult), `ee` (**1 DH**),
`se` (**1 DH**), `send_tail`.

### 3.3 Exact counts

Counting `DhProvider::dh` calls (what slither's `CountingProvider` counts,
`slither/src/testutil.rs:74-83`) and, separately, base-point mults:

| Path | Fallback DH | Fallback `d·G` | Native split DH | Native `d·G` |
|---|---|---|---|---|
| Reject at stage 1 (unlisted claim) | **1** (`es`) | 1 | **1** | 1 |
| Reject at stage 2 (e.g. timestamp replay) | **3** (`es`, `es`, `ss`) | 2 | **2** (`es`, `ss`) | 1 |
| Full accept | **5** (`es`, `es`, `ss`, `ee`, `se`) | 3 | **4** | 2 |
| Drop at stage 0 (bad mac1 / banned addr) | 0 | 0 | 0 | 0 |

**Correction to TODO.md §4**: "Rejects cost 1 DH either way; only the accept path
pays +1 DH" is right for a *stage-1* reject but understates a *stage-2* reject —
the fallback pays **+1 DH on every path that reaches stage 2**, reject or accept.
That is defensible: reaching stage 2 already requires passing the allow-list, so
the anonymous-attacker cost (the DoS-relevant number) is unchanged at 1 DH, and
the extra DH is only ever spent on a peer you have already chosen to talk to.

Concretely: ~one extra P-256 variable-base mult (~30–60 µs software) plus a
fixed-base mult and a 65-byte AEAD open per accepted handshake, on top of 4 DH
already. Negligible unless the static lives in hardware.

### 3.4 Claimed-vs-proven semantics — confirmed

The recording closure sees the static **before authentication**, and the split
would too. Proof chain:

- Emission order: `recv_s` → `verify` → `ss` (`codegen.rs:1117-1122`, `1153-1158`).
- hiss's own pin: the closure asserts `dhs.get() == 1` — "`es` has run; `ss` has
  not" (`tests/noise_macro_shapes.rs:650-657`), and a rejecting closure leaves
  the count at 1 (`tests/noise_macro_shapes.rs:616-620`).
- Slither's pin: rejected read = 1 DH, accepted read = 2, msg2 write = 4
  (`slither/src/handshake.rs:855-894`).
- The documented contract: "claimed, not yet proven … rejecting is always safe,
  but side effects in the closure must not treat the key as authenticated"
  (`codegen.rs:1061-1077`), echoed in slither's module doc
  (`slither/src/handshake.rs:24-43`).

Proof of possession arrives only with `ss` **and** the tail's AEAD tag: a forger
who claims a listed static passes `recv_s` (they control the `es` key schedule)
but derives the wrong `ss`, so the tail tag fails → `DecryptionFailed`. Hence
`RespAccept`'s claim that the static is authenticated
(`slither/src/handshake.rs:376-380`) is sound, and the `TimestampGuard`'s
"cannot pollute the map under a key it does not hold"
(`slither/src/handshake.rs:499-509`) holds. **The staged `Claimed → Proven`
boundary is exactly this boundary** — no new cryptographic reasoning is needed
for either the split or the fallback.

### 3.5 Correctness caveats of the fallback

1. **The golden-wire test is sensitive to the number of `Identity::provider()`
   draws.** `SoftwareIdentity::provider()` pulls a fresh 32-byte seed off the
   master `ChaCha20Rng` (`slither/src/handshake.rs:207-211`). The stage-1 read
   consumes **no** randomness (the responder generates no ephemeral until msg2),
   but the extra `provider()` *call* advances the master stream, so the
   responder's msg2 ephemeral changes → `GOLDEN_RESP` and `GOLDEN_SID` in
   `slither/src/handshake.rs:929-1007` would move if the staged path drives that
   test. Three ways out:
   - **Clone the provider for the throwaway stage-1 read.** `EphemeralOnly<R>` is
     `#[derive(Clone)]` (`hiss/src/provider/mod.rs:275-278`) and `ChaCha20Rng` is
     `Clone`, so this works today; it needs `Identity::Provider: Clone` added to
     the associated-type bound at `slither/src/handshake.rs:160`
     (`CountingProvider` would need a trivial manual `Clone`).
   - **Use a randomness-free provider for stage 1** — stage 1 only needs
     `public_key` and `dh`, never `generate_ephemeral_key`. Clean for
     `SoftwareIdentity`; does *not* generalise to a hardware static, whose private
     key type is opaque.
   - **Keep `accept_init` as a one-shot** alongside the staged path and let the
     golden test keep calling it. Cheapest, and arguably right: the golden test
     is pinning the *wire*, not the accept plumbing.
2. **The Intro must retain the raw 196-byte msg1 until stage 2**, because stage 2
   re-reads the same bytes. TODO.md already plans this ("raw ~196 B + addr").
3. **Both stages must use the same prologue and the same responder static.** Both
   are fixed constants in slither (`slither/src/wire.rs:54`,
   `Identity::static_secret`), so this is free — but it becomes a real constraint
   if a key rotation could land between stages.
4. **Hardware statics pay double.** Each rebuild calls
   `CryptoKeyProvider::public_key` on the long-term static. For
   `EphemeralOnly`/P-256 that is a local `mul_base`; for a Secure Enclave provider
   it may be an out-of-process call. This is the strongest argument for
   preferring the native split once hiss ships it.
5. **No security caveat.** Because the rejected read is side-effect-free and
   deterministic, the fallback is observationally equivalent to the split on every
   path — same transcript, same wire, same decisions, same rejection points.

---

## 4. Connection-object needs

### 4.1 The datagram surface the Connection must own

Produced once, at handshake completion, by
`Transport::into_datagram_with_epoch(epoch_size: NonZeroU64)`
(`hiss/src/noise/datagram.rs:133-157`), which **consumes** the `Transport`.
Slither calls it on both sides with the same constant
(`slither/src/handshake.rs:348`, `:414`;
`REKEY_EPOCH_MSGS = 65_536`, `slither/src/session.rs:50`).

```rust
impl<Proto: Protocol> DatagramSend<Proto> {
    pub fn encrypt_next(&mut self, ad: &[u8], plaintext: &[u8], output: &mut [u8])
        -> Result<(u64 /*counter*/, usize /*written*/), HandshakeError>;   // :205-227
    pub fn session_id(&self) -> &SessionId;                                 // :231-233
}

impl<Proto: Protocol> DatagramRecv<Proto> {
    pub fn decrypt_at(&mut self, counter: u64, ad: &[u8], ciphertext: &[u8], output: &mut [u8])
        -> Result<usize, HandshakeError>;                                   // :446-457
    pub fn session_id(&self) -> &SessionId;                                 // :461-463
}
```

That is the **entire** public surface — no `Clone`, no `Debug`, no counter
accessor, no rekey trigger.

**Counters.** The send counter lives inside hiss and is strictly monotonic; the
caller learns it only *after* the seal. But slither needs it *before*, because
the 14-byte `DataHeader` (which carries the counter) **is** the AEAD associated
data. Hence the mirrored `next_counter` field plus a `debug_assert_eq!` that the
mirror matched (`slither/src/session.rs:141-143`, `228-254`). The new Connection
object inherits this mirror unchanged. *hiss wish-list item*: a public
`DatagramSend::next_counter() -> u64` (the value exists as
`CipherState::nonce()`, but it is `pub(crate)`, `cipher_state.rs:165-167`) would
delete the mirror and the assert.

**Replay is entirely slither's.** `decrypt_at` will open the same counter any
number of times (`datagram.rs:402-412`). Slither's RFC-6479-style 128-counter
window (`slither/src/session.rs:57-119`) runs *after* AEAD success
(`slither/src/session.rs:288-297`), and doubles as the Leg 2 ACK source via
`snapshot()` (`slither/src/session.rs:110-118`, `258-260`).

### 4.2 Epoch / rekey mechanics — what the swap replaces

- **Schedule**: epoch of a message = `counter / epoch_size`; epoch `e` uses the
  key reached by applying Noise §11.3 `Rekey()` `e` times to the handshake key
  (`datagram.rs:110-132`). A pure function of the counter → **no wire signalling
  at all**, which is why the ratchet is byte-invisible (slither pins the
  `REKEY(0^32)` vector at `slither/src/handshake.rs:734-747`).
- **Send side**: before each seal, if `nonce() / epoch_size > key_epoch`, call
  `CipherState::rekey()` until they match (`datagram.rs:211-225`). The swap
  replaces **only the 32-byte key** in place; **the nonce is not reset**
  (`cipher_state.rs:146-157`) — that is what keeps the counter→epoch map total.
  State: `SendEpoch { epoch_size, key_epoch }` (`datagram.rs:160-166`).
- **Receive side**: `RecvRatchet { epoch_size, current_epoch, current_key,
  prev_key }` (`datagram.rs:276-286`). Current epoch → open, no state change.
  `current_epoch − 1` → open under the retained `prev_key`. Older → refused.
  Future → **candidate keys derived from a copy; committed state advances only
  after the tag verifies** (`datagram.rs:353-390`), and more than
  `MAX_EPOCH_JUMP = 2` epochs ahead is refused **without deriving any key**
  (`datagram.rs:74`, `353-356`).
- **What the swap does *not* touch**: `SessionId`, session indices, peer address,
  replay window, timers. So the Connection object never sees the ratchet — it is
  invisible below `encrypt_next`/`decrypt_at`.
- **Two operational consequences for the Connection object.** (1) A peer more
  than 2 epochs (131 072 counters) ahead is permanently unopenable — that must be
  handled as a dead session / re-handshake, not as a decrypt retry. (2) Both
  peers must pass the identical `epoch_size`; slither pins it as protocol, not a
  knob (`slither/src/session.rs:44-50`).

### 4.3 The full state the Connection must own

Exactly today's `Session` (`slither/src/session.rs:137-158`), which is already
transport-agnostic (bytes in / bytes out):

`DatagramSend` + `DatagramRecv` + `SessionId` + mirrored `next_counter` +
`ReplayWindow` + `our_index`/`peer_index` + authenticated `remote_static` +
`endpoint: SocketAddr` (roams on authenticated receive,
`slither/src/session.rs:300-307`) + `established_at`/`last_recv`/`last_send`
driving `should_keepalive` / `is_dead` / `needs_rekey` / `is_expired`
(`slither/src/session.rs:323-365`). Note the deliberate `seal` vs `seal_quiet`
split (`slither/src/session.rs:207-226`): Leg 2 control traffic is
liveness-neutral. Any `split()` into futures `Sink`/`Stream` halves (TODO phase 3)
must keep the *roaming* update on the receive half while the send half reads the
peer address — that shared `endpoint` field is the only real coupling.

### 4.4 Genericity — what makes a `Channel` trait work

**The datagram halves are already generic in exactly the right way.**
`DatagramSend<Proto>` / `DatagramRecv<Proto>` take a single type parameter
`Proto: Protocol` (`datagram.rs:176`, `255`), and `Protocol`
(`codegen.rs:343-350`) carries `Curve`/`Cipher`/`Hash` as associated types.
Their methods speak **plain slices** — `ad: &[u8]`, `plaintext: &[u8]`,
`output: &mut [u8]` — with no const-generic arrays anywhere, and the only
associated const in play is `TAG_SIZE` via `Proto::Cipher`.

So `Connection<C: Channel>` can hold `DatagramSend<C>`/`DatagramRecv<C>`
directly, provided `Channel: hiss::noise::Protocol`. **No `generic_const_exprs`
is needed on the post-handshake path at all.**

The const-array leakage is confined to the **handshake** surface, and it is all
*inherent*, not trait items: `IK::MSG1_SIZE`/`MSG2_SIZE` are inherent consts on
the marker struct (`codegen.rs:420-424`), and the read/write methods are inherent
`impl`s over `[u8; IK::MSGn_SIZE]`. That is precisely why TODO.md §1's plan works:
the `channel!` macro stamps the concrete `noise!` invocation, and the `Channel`
trait's handshake methods hand back `Vec<u8>`/slices, hiding the arrays. The
state types (`IKResponderMsg1<CP>` etc.) are also per-invocation concrete types,
so they must sit behind associated types on `Channel` (e.g.
`type ResponderMsg1<CP>: …`) or behind slither's own wrapper structs — the latter
is what `InitiatorPending`/`RespAccept` already do
(`slither/src/handshake.rs:251-260`, `367-373`).

Also relevant: `Established` (`slither/src/handshake.rs:229-244`) is already the
single seam between handshake and session, and it mentions
`DatagramSend<SlitherChannel>`/`DatagramRecv<SlitherChannel>` only —
parameterising it to `DatagramSend<C>` is a one-token change.

Two smaller genericity notes:
- `SessionId` is `Clone` and protocol-independent (`hiss/src/noise/session_id.rs`),
  so it crosses the trait boundary freely.
- `DatagramSend`/`DatagramRecv` are neither `Clone` nor `Debug`, and their `Drop`
  scrubs keys — so the Connection object must own them, not copy them, and cannot
  derive `Debug`.

---

## Appendix — quick reference of the load-bearing citations

| Fact | Where |
|---|---|
| The `noise!` invocation | `slither/src/handshake.rs:64-81` |
| Closure fires after `recv_s`, before `ss` | `hiss-macros/src/codegen.rs:1110-1123`, `1153-1158` |
| "claimed, not yet proven" contract | `hiss-macros/src/codegen.rs:1061-1077` |
| "payload only from an accepted read" | `hiss-macros/src/codegen.rs:1084-1090` |
| `_with` variant predicate | `hiss-macros/src/codegen.rs:954-956`, `970-975` |
| Responder constructor is fallible; costs `d·G` | `codegen.rs:457`, `496-516`; `support.rs:81-100`; `curve/p256/software.rs:104-112` |
| Every method consumes `self`; error is terminal | `codegen.rs:1189`; `support.rs:24-31`; `process.rs:17-28` |
| Key scrubbing on drop (and the un-zeroed `h`) | `symmetric_state.rs:202-208`; `cipher_state.rs:315-322`; `curve/p256/software.rs:180-184` |
| DH-count pins (hiss) | `hiss/tests/noise_macro_shapes.rs:588-621`, `623-675` |
| DH-count pin (slither) | `slither/src/handshake.rs:832-899` |
| Epoch ratchet construction / send / recv | `hiss/src/noise/datagram.rs:133-157`, `205-227`, `305-390` |
| `MAX_EPOCH_JUMP = 2` | `hiss/src/noise/datagram.rs:74` |
| Counter mirror + AD coupling | `slither/src/session.rs:141-143`, `228-254` |
| `EphemeralOnly` is `Clone` | `hiss/src/provider/mod.rs:275-278` |
| Golden-wire bytes | `slither/src/handshake.rs:929-1007` |

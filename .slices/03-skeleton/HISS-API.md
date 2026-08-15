# hiss transport API digest (post-handshake surface)

**Status:** COMPLETE.
**Sources:** `hiss-0.3.2`, `hiss-macros-0.3.1` (crates.io registry),
slither's existing hiss-using code, `SPEC.md` Appendix A (lines 5246–5364)
plus targeted reads of §7.1/§7.2 (1844–1893), §7.7 (2465–2513) and §7.9
(2528–2542) — ~130 lines total, taken with `Read` offset/limit per working
rule 1, in order to check the source against the spec's own claims.

## TL;DR — the three answers

1. **Counter:** hiss **owns** the send counter; there is no `encrypt_at`.
   `next_counter()` (read-only) tells you the counter the next seal will
   use, so the header can be built before sealing. **Opening at an
   arbitrary counter WORKS** — `decrypt_at(counter, …)` imposes no
   monotonicity and no replay rejection, by explicit design. Opening
   packet 7 after packet 9 is fine.
2. **Ratchet:** hiss provides it **natively** —
   `Transport::into_datagram_with_epoch(epoch_size)`, counter-derived,
   **per-direction independent**, no wire signalling, `MAX_EPOCH_JUMP = 2`,
   commit-only-after-verify. slither already wires it. Nothing to build.
3. **Exhaustion:** clean `Err(HandshakeError::NonceOverflow)`. Never
   panics, never wraps. Usable range `0 ..= u64::MAX - 1`; `u64::MAX` is
   reserved for `Rekey()` and refused on both send and receive.

Registry roots (abbreviated below as `hiss-0.3.2/…` / `hiss-macros-0.3.1/…`):
- `/Users/nicolasdiprima/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hiss-0.3.2/`
- `/Users/nicolasdiprima/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hiss-macros-0.3.1/`

---

## 0. Inventory (what exists, where)

Transport-relevant modules in `hiss-0.3.2/src/`:

| File | Lines | What it holds |
|---|---|---|
| `src/noise/transport.rs` | 352 | `Transport<Proto>`, `TransportSend`, `TransportRecv` (stream mode) |
| `src/noise/datagram.rs` | 483 | **`DatagramSend`, `DatagramRecv`, `MAX_EPOCH_JUMP`** — the slither seam |
| `src/noise/cipher_state.rs` | 396 | `CipherState<Ci>`, `MAX_MESSAGE_LEN`, `rekey_key` |
| `src/noise/cipher.rs` | 149 | `Cipher` trait: `TAG_SIZE`, `encrypt`, `decrypt` |
| `src/noise/error.rs` | 92 | `HandshakeError` — the single error type |
| `src/noise/session_id.rs` | 89 | `SessionId` |
| `src/noise/buffers.rs` | 227 | fixed-size message buffers |
| `src/lib.rs` | 533 | crate root / re-exports |

No `transport`-side code is macro-generated: `DatagramSend`/`DatagramRecv`
and every method below are hand-written in `datagram.rs`. (The `noise!`
macro generates only the *handshake* state types and their
`write_message_N` / `read_message_N` / `read_message_N_intro` methods.)

---

## 1. The counter / nonce seam (§7.1, §7.2)

### 1.1 Getting a datagram pair

`hiss-0.3.2/src/noise/transport.rs:200` (stream split — **not** what slither
wants) versus the two datagram constructors on `Transport<Proto>`:

```rust
// hiss-0.3.2/src/noise/datagram.rs:96
pub fn into_datagram(self) -> (DatagramSend<Proto>, DatagramRecv<Proto>)

// hiss-0.3.2/src/noise/datagram.rs:133-136
pub fn into_datagram_with_epoch(
    self,
    epoch_size: NonZeroU64,
) -> (DatagramSend<Proto>, DatagramRecv<Proto>)
```

Both `impl<Proto: Protocol> Transport<Proto>` — `hiss-0.3.2/src/noise/datagram.rs:76`.
Both **consume** the `Transport`.

### 1.2 Sealing — hiss owns the counter (caller CANNOT supply it)

```rust
// hiss-0.3.2/src/noise/datagram.rs:205-210
pub fn encrypt_next(
    &mut self,
    ad: &[u8],
    plaintext: &[u8],
    output: &mut [u8],
) -> Result<(u64, usize), HandshakeError>
```

- **The counter is hiss-owned and strictly monotonic (`0, 1, 2, …`).**
  There is **no** `encrypt_at(counter, …)`. The doc comment is explicit:
  "The counter is owned by `hiss` and is strictly monotonic (`0, 1, 2,
  …`); the caller can never choose it, so it can never cause nonce
  reuse." (`datagram.rs:190-192`)
- Returns `(counter, bytes_written)`. `counter` is the explicit nonce the
  message was sealed under — **this is the value slither puts in the
  §7.1 Data header, little-endian**.
- **Separate buffers**, not in place: `plaintext: &[u8]` in, `output:
  &mut [u8]` out. `output` must be ≥ `plaintext.len() + OVERHEAD`
  (`datagram.rs:199`).
- The tag is written by hiss into `output`, appended after the
  ciphertext; `bytes_written == plaintext.len() + TAG_SIZE`.
- **On any error the counter does not advance and nothing is written**
  (`datagram.rs:203-204`).

#### The chicken-and-egg solver: `next_counter()`

```rust
// hiss-0.3.2/src/noise/datagram.rs:244
pub fn next_counter(&self) -> u64
```

This is the method that makes §7.1 workable. slither's Data header carries
the counter **and** the header is the AEAD associated data — so the header
must be built *before* the seal, which needs the counter *before* the seal.
`next_counter()` returns exactly the counter the next **successful**
`encrypt_next` will use, and a failed seal leaves it unchanged
(`datagram.rs:229-246`). Sequence for slither:

1. `let c = send.next_counter();`
2. build the Data header with `c` little-endian;
3. `send.encrypt_next(&header, plaintext, &mut out)` → `(c2, n)`; `c2 == c`.

At `u64::MAX` the accessor still returns `u64::MAX` — the counter that will
never be used; the next seal fails with `NonceOverflow` (`datagram.rs:239-243`).

### 1.3 Opening — caller supplies an arbitrary counter (the §7.2 answer)

```rust
// hiss-0.3.2/src/noise/datagram.rs:465-471
pub fn decrypt_at(
    &mut self,
    counter: u64,
    ad: &[u8],
    ciphertext: &[u8],
    output: &mut [u8],
) -> Result<usize, HandshakeError>
```

**Out-of-order opening WORKS and hiss enforces NO monotonicity.** Verbatim
from the doc comment at `datagram.rs:424-430`:

> **Out of order, and no replay rejection.** The same `counter` can be
> decrypted more than once, and counters may arrive in any order or not at
> all — nothing here rejects a replay. **Replay protection is explicitly
> the caller's duty**: a datagram protocol that must reject duplicates has
> to track the counters it has already accepted (typically a sliding
> window).

Opening packet 7 after packet 9 is therefore fine — **with one caveat that
only applies to the epoch-ratcheting half**: a straggler more than one
*epoch* behind the committed epoch fails with `DecryptionFailed`
(`datagram.rs:353-360`). Within an epoch, and across one epoch boundary,
arbitrary order is unrestricted. For the plain (`into_datagram`) half there
is no restriction at all — `CipherState::decrypt_at` is stateless with
respect to the counter (`datagram.rs:473`).

- `output` must be ≥ `ciphertext.len() - OVERHEAD` (`datagram.rs:432`).
- Separate buffers again; `ciphertext` includes the tag.
- On `DecryptionFailed`, `output` holds **unauthenticated** bytes that
  must not be read, and committed keys are unchanged (`datagram.rs:460-464`).
- `counter == u64::MAX` is refused with `NonceOverflow` before any crypto
  (`datagram.rs:339-341`, ratchet path only — see §3 below for the plain path).

### 1.4 Associated data

`ad: &[u8]` on both `encrypt_next` and `decrypt_at`. It is an ordinary
byte slice with **no hiss-imposed length limit** — see §6 for what the
underlying `Cipher` trait does with it. slither passes its Data header
here.

---

## 2. The epoch ratchet (§7.7)

**hiss provides this natively.** `Transport::into_datagram_with_epoch`
(`hiss-0.3.2/src/noise/datagram.rs:133`) returns a pair whose keys ratchet
forward on a **counter-derived** schedule with no handshake and no round
trip — exactly §7.7's shape.

### 2.1 The schedule

From `datagram.rs:36-45` (module doc) and `datagram.rs:119-125`:

- The caller fixes `epoch_size: NonZeroU64`. **hiss fixes no default** —
  slither passes `NonZeroU64::new(65_536)`.
- A message sealed at counter `c` belongs to epoch `c / epoch_size`.
- Epoch `e` uses the key reached by applying the Noise §11.3 `Rekey()`
  transform `e` times to the handshake key.
- The schedule is a **pure function of the counter**, so both peers agree
  on which key opens which packet with **no extra wire signalling**.
- **Both peers must pass the same `epoch_size`** or they disagree on keys.

### 2.2 Per-direction independence — YES

`datagram.rs:44-45`: "Each direction ratchets **independently** on its own
counter, exactly as the two Noise `CipherState`s rekey independently."

Send side: `SendEpoch { epoch_size, key_epoch }` lives inside
`DatagramSend` (`datagram.rs:161-166`); `encrypt_next` advances it eagerly
as the counter crosses a boundary (`datagram.rs:211-225`).

Receive side: `RecvRatchet { epoch_size, current_epoch, current_key,
prev_key }` lives inside `DatagramRecv` (`datagram.rs:295-305`).

### 2.3 Receive-side ratchet discipline (matters for §7.2 + §7.7)

`RecvRatchet::decrypt_at` at `hiss-0.3.2/src/noise/datagram.rs:324-409`:

- **Current epoch** → open under committed key, no state change (`:346-348`).
- **Previous epoch (exactly one back)** → opens under the retained
  `prev_key` (`:353-359`). So a datagram reordered across **one** epoch
  boundary still opens.
- **Older than one epoch back** → `HandshakeError::DecryptionFailed`; that
  key was ratcheted away and is gone (`:359`).
- **Future epoch, ≤ `MAX_EPOCH_JUMP` (= 2) ahead** → derive *candidate*
  keys from a copy; **commit only after the AEAD tag verifies** (`:372-408`).
  A forged packet cannot desynchronise the receiver.
- **Future epoch, > `MAX_EPOCH_JUMP` ahead** → `DecryptionFailed`
  **without deriving any key** (`:372-375`) — CPU-DoS bound.

```rust
// hiss-0.3.2/src/noise/datagram.rs:74
pub const MAX_EPOCH_JUMP: u64 = 2;
```

### 2.4 The non-ratcheting alternative, and the stream-mode `rekey`

- `into_datagram` (plain) **never ratchets**: both halves keep handshake
  keys for the session's life (`datagram.rs:30-34`). Explicitly rejected
  by hiss's own docs as a place to bolt on a rekey: "With explicit
  counters an *unsynchronised* rekey while packets are in flight would be
  a correctness trap, so the plain pair offers none."
- `Transport::rekey` / `TransportSend::rekey` / `TransportRecv::rekey`
  (`transport.rs:156`, `:267`, `:328`) exist but are **stream-mode only**
  and are consumed by `into_datagram*`; there is **no public `rekey()` on
  `DatagramSend` or `DatagramRecv`.** The ratchet is driven solely by the
  counter schedule. Confirmed by grep — see §9.

## 3. Nonce exhaustion (§7.9)

**hiss errors — it never panics and never wraps.** The exact error is
`HandshakeError::NonceOverflow` (`hiss-0.3.2/src/noise/error.rs:40-42`):

```rust
/// The nonce counter has overflowed (2^64 − 1 messages encrypted).
#[error("nonce overflow — session must be rekeyed")]
NonceOverflow,
```

Three distinct guards, all returning `NonceOverflow`:

| Site | Condition | Citation |
|---|---|---|
| Send, ratcheting half, **before** any key advance | `self.cipher.nonce() == u64::MAX` | `datagram.rs:216-218` |
| Send, inner cipher state | `self.n == u64::MAX` (before AEAD) | `cipher_state.rs:98-100` |
| Receive | `counter == u64::MAX` supplied by caller | `cipher_state.rs:273-275` (plain) and `datagram.rs:339-341` (ratchet) |

Semantics that matter to §7.9:

- The **usable counter range is `0 ..= u64::MAX - 1`**. `u64::MAX` is
  *reserved for the `Rekey()` transform* (`rekey_key` calls
  `Ci::encrypt(key, u64::MAX, …)` — `cipher_state.rs:305`), so no
  legitimately sealed message ever carries it, and the receiver refuses it
  outright.
- The last successful seal is at counter `u64::MAX - 1`; the counter then
  reads `u64::MAX` and **every subsequent seal fails**
  (`cipher_state.rs:331-345` is hiss's own test of exactly this).
- **On error nothing advances and nothing is written** — the failure is
  clean and repeatable, so slither can poll `next_counter()`, see
  `u64::MAX`, and tear the session down proactively rather than waiting
  for the error.
- For the ratcheting half the guard is *duplicated at `datagram.rs:216`
  specifically so that a refused seal cannot ratchet the key first* —
  "on any error nothing changes" holds for key state too.

**Note the mismatch with §7.9's likely intent:** hiss's exhaustion point is
`u64::MAX`, i.e. ~2^64 messages. slither's own §7.9 limit (whatever it
sets) will be far lower and must be enforced by slither, not by hiss —
hiss will happily seal until 2^64−1.

---

## 4. Error types (§18 mapping)

There is exactly **one** error type on the whole hiss surface:
`hiss::noise::HandshakeError` — `hiss-0.3.2/src/noise/error.rs:8`. It is
`#[derive(Debug, thiserror::Error)]` and **`#[non_exhaustive]`**
(`error.rs:6-7`), so slither's `match` must carry a `_ =>` arm.

### 4.1 Variants reachable from `encrypt_next` / `decrypt_at`

| Variant | Meaning | Reachable from | slither §18 class |
|---|---|---|---|
| `DecryptionFailed` | AEAD tag mismatch: tampered ciphertext, wrong `ad`, wrong `counter`, **or a refused epoch** (too-old / too-far-future) | `decrypt_at` | **peer sent garbage** — drop the packet silently |
| `NonceOverflow` | counter hit `u64::MAX` on send, or caller/wire supplied `u64::MAX` on receive | both | send side = **our state is exhausted** (fatal, session must end); receive side = **peer sent garbage** |
| `MessageTooLong { len }` | on-wire message (ciphertext + tag) > 65535 | both | send side = **our bug** (we built an oversized frame); receive side = **peer sent garbage** |
| `OutputBufferTooSmall { needed, actual }` | our `output` slice is undersized | send (`encrypt_with_ad` unkeyed path) and via `Cipher::encrypt`/`decrypt` | **our own state/bug** — never peer-caused |
| `RekeyWithoutKey` | `rekey()` on an unkeyed `CipherState` | not reachable through the datagram halves (a completed transport is always keyed; `into_datagram_with_epoch` degrades to the no-ratchet path rather than panicking — `datagram.rs:147-154`) | **our own bug** if seen |
| `Crypto(Box<dyn Error + Send + Sync>)` | underlying curve/provider failure | `rekey_key` → `Ci::encrypt` in principle; primarily handshake-side | **our own state broken** |

Note `DecryptionFailed` is deliberately **indistinguishable** between "bad
tag" and "epoch too old / too far ahead" (`datagram.rs:359`, `:374`). If
slither's §18 wants to tell a stale-epoch straggler from a forgery it
**cannot** get that from hiss — it must compute the epoch itself from the
counter and its own `epoch_size` before calling `decrypt_at`.

### 4.2 Handshake-only variants (listed for completeness, not on the data path)

`MessageTooShort`, `MissingStaticKey`, `MissingEphemeralKey`,
`MissingRemoteStatic`, `MissingRemoteEphemeral`, `InvalidPublicKey`,
`NonCanonicalPublicKey`, `PeerRejected { reason: String }`, `Io`
(`error.rs:16-91`).

### 4.3 The "peer garbage vs our bug" line — the load-bearing summary

- **Peer-caused, drop the packet, keep the session**: `DecryptionFailed`;
  `MessageTooLong` and `NonceOverflow` *when they arise from a
  wire-supplied counter or length*.
- **Locally-caused, a bug or terminal exhaustion**:
  `OutputBufferTooSmall`, `RekeyWithoutKey`, `Crypto`, and
  `NonceOverflow` *on the send side*.
- The **same variant appears on both sides of the line**, so slither
  cannot classify by variant alone — it must classify by **call site**
  (seal vs. open). This is a real constraint on the §18 mapping.

## 5. State ownership, `Send`, `Clone`, split halves

### 5.1 It is a split pair, and the split is forced

`into_datagram` / `into_datagram_with_epoch` **consume** the `Transport`
and return `(DatagramSend<Proto>, DatagramRecv<Proto>)`
(`hiss-0.3.2/src/noise/datagram.rs:96`, `:133`). There is no combined
datagram object and no way to rejoin the halves. The two halves are
**fully independent values** — hold them in separate fields, pass them to
separate code paths, drop them separately.

Field layout (all private, no accessors beyond those listed in 5.2):

```rust
// hiss-0.3.2/src/noise/datagram.rs:176-181
pub struct DatagramSend<Proto: Protocol> {
    cipher: CipherState<Proto::Cipher>,
    session_id: SessionId,
    epoch: Option<SendEpoch>,
}

// hiss-0.3.2/src/noise/datagram.rs:274-277
pub struct DatagramRecv<Proto: Protocol> {
    keys: RecvKeys<Proto::Cipher>,
    session_id: SessionId,
}
```

### 5.2 Full public method inventory

`DatagramSend<Proto>`:
- `encrypt_next(&mut self, ad, plaintext, output) -> Result<(u64, usize), HandshakeError>` — `datagram.rs:205`
- `next_counter(&self) -> u64` — `datagram.rs:244`
- `session_id(&self) -> &SessionId` — `datagram.rs:250`

`DatagramRecv<Proto>`:
- `decrypt_at(&mut self, counter, ad, ciphertext, output) -> Result<usize, HandshakeError>` — `datagram.rs:465`
- `session_id(&self) -> &SessionId` — `datagram.rs:480`

**That is the entire surface.** No `rekey`, no `OVERHEAD` const (it is on
`Transport`/`TransportSend`/`TransportRecv` only), no key accessor, no
epoch accessor, no counter setter outside `#[cfg(test)]`
(`set_counter_for_test` at `datagram.rs:260` is `pub(crate)` **and**
`#[cfg(test)]` — unreachable from slither).

Note `decrypt_at` takes **`&mut self`** even though the plain (non-epoch)
path underneath is stateless and takes `&self` (`cipher_state.rs:252`).
The `&mut` is required by the ratcheting path. slither's receive path must
therefore hold `DatagramRecv` mutably.

### 5.3 `Send` — auto-derived, and yes in practice

Grep found **no** `impl Send`, `impl Sync`, or `unsafe impl` anywhere in
`hiss-0.3.2/src/` (see §9). `Send`/`Sync` are therefore purely
auto-derived from the fields.

Both halves contain only: `Option<[u8; 32]>`, `u64`, `NonZeroU64`,
`PhantomData<Ci>`, and `SessionId(Box<[u8]>)`
(`hiss-0.3.2/src/noise/session_id.rs:30`). All are `Send + Sync`.
`PhantomData<Proto::Cipher>` makes the auto-impl conditional on
`Proto::Cipher: Send`; `ChaChaPoly` is a `#[derive(Debug, Clone, Copy,
Default)] pub struct ChaChaPoly;` unit type (`cipher.rs:60-61`), so it is.

**Load-bearing consequence for slither's `!Send` actor rule:** the
datagram halves carry **no provider and no `Identity`** — the DH provider
is consumed during the handshake and never reaches `Transport`, let alone
`DatagramSend`/`DatagramRecv`. So the *transport* state is not what forces
`!Send`; the *handshake* state (which holds the provider) is. slither must
still not add `Send` bounds to the actor path, but the reason lives
upstream of slice 3, not in it. **Do not write the auto-impl into a
bound** — `Proto` is slither's own macro-generated type, and a future
provider-carrying field would silently break such a bound.

### 5.4 `Clone` — NO

Neither `DatagramSend` nor `DatagramRecv` nor `Transport` nor
`CipherState` derives or implements `Clone`. The only `Clone` in the
transport neighbourhood is `SessionId` (`session_id.rs:28`). Grep evidence
in §9. **Key material is move-only by construction**, and both halves have
`Drop` impls that zeroise (`cipher_state.rs:315-322`,
`datagram.rs:412-419`).

### 5.5 What is *not* reachable from outside hiss

`CipherState<Ci>` is publicly exported (`noise/mod.rs:178`) but its useful
transport methods are **crate-private**:

| Item | Visibility | Citation |
|---|---|---|
| `CipherState::from_key` | `pub(crate)` | `cipher_state.rs:44` |
| `CipherState::encrypt_next_with_ad` | `pub(crate)` | `cipher_state.rs:127` |
| `CipherState::nonce` | `pub(crate)` | `cipher_state.rs:165` |
| `CipherState::key` | `pub(crate)` | `cipher_state.rs:176` |
| `CipherState::decrypt_at` | `pub(crate)` | `cipher_state.rs:252` |
| `rekey_key::<Ci>` | `pub(crate)` | `cipher_state.rs:294` |
| `MAX_MESSAGE_LEN` | `pub(crate)` | `cipher_state.rs:19` |
| `Transport::into_cipher_states` | `pub(crate)` | `transport.rs:228` |

Public on `CipherState`: `empty()`, `has_key()`, `encrypt_with_ad()`,
`decrypt_with_ad()`, `rekey()` — all stream-mode, implicit-nonce.

**Therefore slither cannot extract, replace, or inspect a chaining key or
cipher key.** There is no escape hatch. Anything the datagram halves do
not offer, slither cannot build on hiss's key material without reaching
for a raw primitive — which the project rule forbids. Fortunately (see §8)
nothing slice 3 needs falls in that gap.

`MAX_EPOCH_JUMP` **is** reachable — `pub mod datagram` (`noise/mod.rs:160`)
plus `pub const` (`datagram.rs:74`) → `hiss::noise::datagram::MAX_EPOCH_JUMP`.
It is *not* in the `pub use` list at `noise/mod.rs:180`, which re-exports
only `DatagramRecv, DatagramSend`, so the fully-qualified path is required.

---

## 6. Sizes and limits

### 6.1 Tag / overhead

```rust
// hiss-0.3.2/src/noise/cipher.rs:11
const TAG_SIZE: usize;
// hiss-0.3.2/src/noise/cipher.rs:73  (ChaChaPoly)
const TAG_SIZE: usize = 16;
```

Exposed as `Transport::OVERHEAD` (`transport.rs:110`),
`TransportSend::OVERHEAD` (`transport.rs:252`), `TransportRecv::OVERHEAD`
(`transport.rs:306`) — each `= <Proto::Cipher as Cipher>::TAG_SIZE`.

**`DatagramSend` / `DatagramRecv` have NO `OVERHEAD` const**, even though
their doc comments refer to "`plaintext.len() + OVERHEAD`"
(`datagram.rs:199`, `:432`). slither must name it itself, e.g.
`<<Proto as Protocol>::Cipher as Cipher>::TAG_SIZE`, or read
`Transport::<Proto>::OVERHEAD` *before* the `into_datagram*` call consumes
the transport. Worth flagging to the implementer: the doc references a
constant that does not exist on the type it documents.

### 6.2 Message length cap

```rust
// hiss-0.3.2/src/noise/cipher_state.rs:19
pub(crate) const MAX_MESSAGE_LEN: usize = 65535;
```

- **Not public.** slither cannot import it.
- Enforced on **send** as `plaintext.len() + TAG_SIZE > MAX_MESSAGE_LEN`
  (`cipher_state.rs:103-106`), i.e. max plaintext = 65519 bytes.
- Enforced on **receive** as `ciphertext.len() > MAX_MESSAGE_LEN`, checked
  **before any work** (`cipher_state.rs:205-209`, `:261-265`,
  `datagram.rs:334-338`).

**No conflict with slither.** `MAX_DATAGRAM` 1200 and `MAX_PLAINTEXT` 1170
are both far under 65535/65519. hiss's cap will never fire on slither's
data path — slither's own limits bind first, and slither must enforce them
itself because hiss will not.

### 6.3 Associated data — no length limit

`ad: &[u8]` is passed straight through to
`cryptoxide::chacha20poly1305::ChaCha20Poly1305::new(key, &nonce, ad)`
(`cipher.rs:93`, `:118`). No length check anywhere in the hiss path.
slither's Data header (a handful of bytes) is trivially fine.

### 6.4 Buffer preconditions and the one panic hazard

- `encrypt_next`: needs `output.len() >= plaintext.len() + 16`, else
  `OutputBufferTooSmall` (`cipher.rs:84-89`). **Checked, returns an
  error.**
- `decrypt_at`: `ciphertext.len() >= 16` else `DecryptionFailed`;
  `output.len() >= ciphertext.len() - 16` else `OutputBufferTooSmall`
  (`cipher.rs:105-114`). **Checked, returns an error.**
- **Panic hazard, plaintext-mode only:** `CipherState::decrypt_at`'s
  unkeyed branch does `output[..len].copy_from_slice(ciphertext)` with
  **no length check** (`cipher_state.rs:267-271`) — unlike
  `decrypt_with_ad`, which does check (`cipher_state.rs:213-218`).
  Unreachable for a completed handshake (always keyed), but it is a real
  asymmetry in hiss and the only place in the datagram path that can panic
  rather than error.

### 6.5 Nonce construction (for the record)

The 64-bit counter becomes the ChaCha20-Poly1305 nonce as **4 zero bytes
followed by the counter little-endian** (`cipher.rs:63-69`). Standard
Noise, and it matches §7.1's little-endian counter on the wire: the value
slither writes into the Data header and the value hiss feeds the AEAD are
the same u64 in the same byte order.

### 6.6 AEAD failure hygiene

On `DecryptionFailed`, `ChaChaPoly::decrypt` zeroes `output[..pt_len]`
before returning (`cipher.rs:119-125`), and the trait *requires* this of
any implementation (`cipher.rs:32-38`). Note this contradicts
`DatagramRecv::decrypt_at`'s own doc, which says `output` holds
"**unauthenticated** bytes that must not be read" (`datagram.rs:460-464`).
Both are safe advice; the doc is the conservative statement and the
implementation is stricter. **slither should follow the doc** (treat
`output` as unreadable on error) rather than depend on the zeroing.

## 7. Appendix A vs. source — conflicts, and one live doc bug

### 7.1 Appendix A itself: **no contradictions found**

`SPEC.md:5246-5364` makes exactly three claims, and all three check out:

| Claim | Verdict | Citation |
|---|---|---|
| **A.1** `read_message_1_intro` + `IKResponderMsg1Intro<CP>` at `hiss-macros/src/codegen.rs:1382` | consistent (handshake — out of scope here, not re-verified) | `SPEC.md:5258-5325` |
| **A.2** `DatagramSend::next_counter()` shipped at `src/noise/datagram.rs:244` | **exact match**, line number included | `SPEC.md:5327-5328` vs `datagram.rs:244` |
| **A.3** canonical encoding needs only `AsRef<[u8]>` | consistent; slither expresses it at `src/packet/suite.rs:58` | `SPEC.md:5337-5349` |

A.2's line citation is byte-accurate against hiss 0.3.2. Appendix A is
**silent on the epoch ratchet** — it neither claims nor denies
`into_datagram_with_epoch`. That is a gap in Appendix A's coverage, not a
contradiction: §7.7 (`SPEC.md:2473`) and §2.1's table (`SPEC.md:461-462`)
carry the ratchet claims instead, and **both are correct** against the
source:

- `SPEC.md:2474` "a message sealed at `counter` belongs to epoch
  `counter / REKEY_EPOCH_MSGS`" = `datagram.rs:343`.
- `SPEC.md:2475` "each direction ratchets independently" = `datagram.rs:44-45`.
- `SPEC.md:2476` "`2⁶⁴ − 1` is reserved for the `Rekey()` transform" =
  `cipher_state.rs:305`, guarded at `datagram.rs:339` and `cipher_state.rs:273`.
- `SPEC.md:2478` `Rekey(k) = ENCRYPT(k, 2⁶⁴−1, empty, zeros[32])[0..32]` =
  `cipher_state.rs:302-308` **verbatim**, including the discard of the
  trailing 16-byte tag.
- `SPEC.md:2481-2483` "retains the current and immediately preceding epoch
  keys … anything older is refused" = `datagram.rs:295-305`, `:353-360`.
- `SPEC.md:2470` `MAX_EPOCH_JUMP` = 2, hiss-fixed = `datagram.rs:74`
  (already pinned by `src/constants.rs:537`).
- `SPEC.md:2491-2492` "refused without key derivation — a generic
  decryption failure at the hiss surface" = `datagram.rs:372-375`, which
  returns `DecryptionFailed` with no derivation. **Correct, and the
  "generic" is load-bearing**: hiss gives no way to distinguish it.
- `SPEC.md:2530` "Sealing at counter `2⁶⁴ − 1` is refused by hiss" =
  `cipher_state.rs:98-100` + `datagram.rs:216-218`.
- `SPEC.md:1846-1847` "monotonic, hiss-owned, never caller-chosen" =
  `datagram.rs:190-192`; there is no `encrypt_at` (§9.2).
- `SPEC.md:1866-1867` "The replay check is strictly **post-AEAD**:
  check-then-mark only after `decrypt_at` authenticates" — correct and
  **necessary**, because `decrypt_at` performs no replay check of its own
  (`datagram.rs:424-430`).

### 7.2 One live inaccuracy in **slither's own code**, not in hiss

`src/core/mod.rs:166-167` documents the `EstablishedSession::open` field:

```rust
/// The opening half. Stateless in hiss, so replay rejection is
/// slither's duty (§7.2) — not this slice's.
pub open: C::Open,
```

**"Stateless in hiss" is true only of the plain `into_datagram` half.**
slither builds its pair with `into_datagram_with_epoch`
(`src/packet/suite.rs:405`), whose `DatagramRecv` holds a `RecvRatchet`
with mutable `current_epoch` / `current_key` / `prev_key`
(`hiss-0.3.2/src/noise/datagram.rs:295-305`) and whose `decrypt_at` takes
**`&mut self`** and commits state on success (`datagram.rs:390-402`).

The *conclusion* ("replay rejection is slither's duty") is right; the
*reason given* is wrong for the half slither actually uses. The
implementer must not assume `open` can be shared immutably or called from
`&self` — it needs `&mut`. **Reporting, not resolving** (project working
rule 3): this may be a stale comment from before the epoch split landed,
or it may be deliberate shorthand for "stateless with respect to the
counter". It should get a ruling or a one-line fix, not a silent edit by
the slice-3 implementer.

### 7.3 A doc/API gap inside hiss (minor, worth knowing)

`DatagramSend::encrypt_next` (`datagram.rs:199`) and
`DatagramRecv::decrypt_at` (`datagram.rs:432`) both document buffer sizing
in terms of `OVERHEAD` — a constant that exists on `Transport`,
`TransportSend` and `TransportRecv` but **not on either datagram half**
(§6.1). Use `Suite::AEAD_TAG_LEN` (`src/packet/suite.rs:81`), which
slither already defines as `Cipher::TAG_SIZE`.

### 7.4 hiss internal doc tension (informational)

`DatagramRecv::decrypt_at` says that on `DecryptionFailed` `output` holds
"**unauthenticated** bytes that must not be read" (`datagram.rs:460-464`),
while `Cipher::decrypt`'s contract *requires* implementations to zero
`output[..pt_len]` first (`cipher.rs:32-38`), and `ChaChaPoly` does
(`cipher.rs:119-125`). Not a correctness problem — follow the stricter
advice and treat `output` as unreadable — but slither must not write a
test that asserts either the zeroing *or* residual bytes, since the two
docs disagree about which is guaranteed.

---

## 8. What slither must build itself

Slice 3's five concerns, against what hiss delivers:

| §  | Concern | hiss provides | slither must build | Raw primitive needed? |
|---|---|---|---|---|
| 7.1 | Packet counter = Noise nonce | **Yes, fully.** `next_counter()` reads it, `encrypt_next()` seals and returns it | Only the *wire encoding*: write/parse the u64 little-endian in the Data header, and pass that header as `ad` | **No** |
| 7.2 | Anti-replay window | **Nothing.** `decrypt_at` explicitly performs no replay check and will open the same counter repeatedly (`datagram.rs:424-430`) | The **entire** RFC 6479 2048-bit sliding bitmap (`[u64; 32]`), greatest-counter tracking, and the strictly post-AEAD check-then-mark | **No** — pure integer/bitmap arithmetic |
| 7.7 | Epoch ratchet | **Yes, fully and natively.** `into_datagram_with_epoch(NonZeroU64)`, per-direction, counter-derived, no round trip, `MAX_EPOCH_JUMP = 2`, commit-only-after-verify | Nothing. Already wired at `src/packet/suite.rs:405` and `src/core/endpoint/mod.rs:652,680`. Slice 3 must only **not** try to drive it, and must **not** chase epochs (`SPEC.md:2512`) | **No** |
| 7.9 | Nonce exhaustion | **Detection at `u64::MAX` only**, as `HandshakeError::NonceOverflow`, on both seal and open | The mapping to `ConnectionLost::NonceExhausted` and the plan-seal-commit discipline (§16.7) so a seal failure strands no frames. Optionally a proactive check on `next_counter()` | **No** |
| — | Data seal/open | **Yes.** `encrypt_next` / `decrypt_at` with separate buffers | Buffer management (`MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170), the `ad` = header wiring, and the error→§18 mapping | **No** |

**Nothing slice 3 needs requires touching a raw primitive.** The one
theoretical gap — extracting or replacing key material — is closed off by
hiss (`pub(crate)` on `CipherState::key`, `from_key`, `rekey_key`, see
§5.5), and slither does not need it because the ratchet is native.

### 8.1 The three things the implementer must get right

1. **Build the header before the seal.** `next_counter()` first, then the
   header, then `encrypt_next(&header, …)`. Assert the returned counter
   equals the one read — §3.4's mirror-and-assert survives as a debug
   check even though it is no longer the operative mechanism
   (`SPEC.md:5333-5335`).
2. **Replay check is post-AEAD, always.** `decrypt_at` succeeding says
   only "this was sealed by the peer under this counter" — it says nothing
   about freshness. Mark the window only after `Ok(_)`.
3. **`open` needs `&mut`.** `DatagramRecv::decrypt_at` takes `&mut self`
   (`datagram.rs:465`) because the epoch ratchet commits on success. See
   §7.2 above — the field's own doc comment currently implies otherwise.

### 8.2 Reference call sequence

```rust
// Seal (slither owns nothing but the header bytes)
let counter = session.seal.next_counter();          // datagram.rs:244
if counter == u64::MAX { /* §7.9: ConnectionLost::NonceExhausted */ }
let header = DataHeader { receiver_index: peer_index, counter }.encode();
let (c, n) = session.seal.encrypt_next(&header, plaintext, &mut out)?; // :205
debug_assert_eq!(c, counter);
// datagram on the wire = header ‖ out[..n];  n == plaintext.len() + AEAD_TAG_LEN

// Open
let (header, body) = split_data_packet(datagram)?;
let counter = header.counter;                        // LE u64 off the wire
// NO pre-AEAD replay check that consumes state; §7.2 is post-AEAD.
let n = session.open.decrypt_at(counter, header_bytes, body, &mut pt)?;  // :465
// ONLY NOW:
if !replay_window.check_and_mark(counter) { return; /* drop, no delivery */ }
// and only now does liveness / roaming advance (§7.2 bullet 2)
```

## 9. Searches run (evidence for negative results)

All run against
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/hiss-0.3.2/src/`
(`$H`) and `…/hiss-macros-0.3.1/src/` (`$M`).

### 9.1 "Is there a rekey/ratchet the caller can drive?"

```
grep -rn "fn rekey\|fn ratchet\|ratchet(" $H/src/
```

Hits — **all of them**:

```
src/noise/transport.rs:156    pub fn rekey(&mut self)   // Transport      (stream)
src/noise/transport.rs:267    pub fn rekey(&mut self)   // TransportSend  (stream)
src/noise/transport.rs:328    pub fn rekey(&mut self)   // TransportRecv  (stream)
src/noise/cipher_state.rs:146 pub fn rekey(&mut self)   // CipherState    (stream)
src/noise/cipher_state.rs:294 pub(crate) fn rekey_key<Ci>(...)  // NOT public
src/noise/mod.rs:2010, :2289  // tests
```

**Conclusion: there is no public `rekey` on `DatagramSend` or
`DatagramRecv`.** The only ratchet reachable from a datagram half is the
automatic counter-derived one installed by `into_datagram_with_epoch`.

```
grep -rn "rekey\|ratchet\|Ratchet\|Rekey" $M/src/
```

**Zero hits.** No rekey/ratchet code is macro-generated; the `noise!`
macro emits handshake states only.

### 9.2 "Can the caller choose the send counter?"

```
grep -rn "encrypt_at\|seal_at\|decrypt_next\|set_counter\|set_nonce" $H/src/ $M/src/
```

Hits — only `set_counter_for_test` / `set_nonce_for_test`, both
`pub(crate)` **and** `#[cfg(test)]` (`datagram.rs:260`,
`cipher_state.rs:63`), plus their call sites inside hiss's own tests
(`mod.rs:1882`, `:1926`, `:2258`, `cipher_state.rs:335`, `:350`).

**Conclusion: `encrypt_at` does not exist. There is no public way to
supply, set, rewind, or skip the send counter.** `next_counter()` is
read-only.

### 9.3 "Is any transport type macro-generated?"

```
grep -rn "Datagram\|into_datagram\|Transport" $M/src/
```

Every hit is a doc-comment string or the `::hiss::noise::Transport<#name>`
return type at `codegen.rs:814`. The macro **names** `Transport` as a
handshake terminal; it defines no transport type and no transport method.

**Conclusion: the entire post-handshake surface is hand-written in
`hiss-0.3.2/src/noise/{transport,datagram,cipher_state,cipher}.rs`.**
Nothing about the data path is hidden behind `noise!` expansion. (Contrast
`read_message_1_intro`, which genuinely exists only at
`hiss-macros-0.3.1/src/codegen.rs` — a handshake concern, out of scope
here.)

### 9.4 "Are `Send`/`Sync`/`Clone` hand-implemented?"

```
grep -rn "unsafe impl\|impl.*Send for\|impl.*Sync for\|derive(Clone\|impl.*Clone for" $H/src/
```

Hits: `psk.rs:15`, `provider/apple.rs:104` `:504`, `provider/mod.rs:275`,
`curve/{x448,x25519,ed25519,p256}` public-key types, and
`noise/session_id.rs:28`. **No `unsafe impl` anywhere. No `Clone` on any
of `Transport`, `TransportSend`, `TransportRecv`, `DatagramSend`,
`DatagramRecv`, `CipherState`.** All `Send`/`Sync` are auto-derived.

### 9.5 Public re-export surface

```
grep -n "^pub use\|^pub mod" $H/src/noise/mod.rs
```

`pub use self::datagram::{DatagramRecv, DatagramSend};` (`mod.rs:180`) —
**`MAX_EPOCH_JUMP` is not re-exported**, but `pub mod datagram`
(`mod.rs:160`) makes `hiss::noise::datagram::MAX_EPOCH_JUMP` reachable.
`pub use self::transport::{Transport, TransportRecv, TransportSend};`
(`mod.rs:194`). `pub use self::cipher::{ChaChaPoly, Cipher};`
(`mod.rs:177`) — so `Cipher::TAG_SIZE` is nameable by slither.

### 9.6 Spec cross-reference

```
grep -n "into_datagram\|DatagramSend\|DatagramRecv\|next_counter\|encrypt_next\
\|decrypt_at\|epoch_size\|MAX_EPOCH_JUMP\|NonceOverflow\|HandshakeError\|Transport<" SPEC.md
```

Hits at SPEC.md lines 292, 451, 461, 462, 661, 1846, 1867, 1870, 2310,
2470, 2473, 5327, 5802. Notably **§7.7 at line 2473 names
`into_datagram_with_epoch`** and line 462 pins `MAX_EPOCH_JUMP = 2` as
"hiss-fixed" — the spec already knows about the native ratchet even
though Appendix A does not list it. See §7.

```
grep -rn "hiss::" src/
```

slither already binds the seam: `src/packet/suite.rs:301-302` declares
`type Seal = ::hiss::noise::DatagramSend<IK>` and
`type Open = ::hiss::noise::DatagramRecv<IK>`;
`src/constants.rs:537` pins `MAX_EPOCH_JUMP == hiss::noise::datagram::MAX_EPOCH_JUMP`
in a `const` assert.

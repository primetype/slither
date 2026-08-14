# Independent derivation of slither wire-v1 byte layout

**Status:** independent second reading, drawn from `SPEC.md` text alone.
**Date:** 2026/08/14.
**Sources read:** `SPEC.md` §2 (445–527), §3 (528–645), §4 (646–700),
§5.1–5.3 (701–749), §17.3 (4810–4824), §1.3 (405–434), Appendix A.2/A.3
(5145–5194), Appendix B wire pins (5195–5210), plus targeted greps for
endianness and golden-vector prose. `CLAUDE.md`. Ruling 64 via
`tail -60 .spec-v2-clean-slate/rulings.md`.

**Sources deliberately NOT read:** `src/`, `tests/`, `PLAN.md`,
`STORIES.md`, `.slices/*`, git history, any other ruling. Nothing here was
checked against an implementation. That is the point of this artifact:
where it agrees with the implementation, the wire is real; where it
disagrees, one of the two readings is wrong and the maintainer decides.

## Notation and confidence marking

Every claim below is tagged:

- **[CITE §x L:n]** — the spec states this, at that line. Confidence: the
  spec's own.
- **[DERIVE]** — follows from cited statements by arithmetic or by a stated
  external standard (Noise, SEC1, RFC 7693). Confidence stated per item.
- **[GAP]** — the spec does not settle this; my reading is a proposal and
  needs a ruling or an explicit confirmation.

Byte ranges are half-open: `[a, b)` means offsets `a` through `b-1`
inclusive, length `b - a`. Offset 0 is the first byte of the UDP payload.

---

## 0. The three constants this whole document stands on

| Constant | Value | Cite |
|---|---|---|
| `VERSION` | `0x01` | §3.1 L:534 |
| `PKT_HANDSHAKE_INIT` | `0x01` | §3.1 L:535 |
| `PKT_HANDSHAKE_RESP` | `0x02` | §3.1 L:536 |
| `PKT_DATA` | `0x03` | §3.1 L:537 |
| `PROLOGUE` | `b"slither\x01"` | §5.1 L:706 |
| `MAC1_LABEL` | `b"slither mac1"` | §4.1 L:657 |
| `MAC1_LEN` | 16 | §4.1 L:658 |
| `AEAD_TAG_LEN` | 16 | §2.3 L:510 |
| `TIMESTAMP_LEN` / `MSG1_PAYLOAD_LEN` | 12 | §5.2 L:723–724 |
| `INIT_HEADER_LEN` | 6 | §3.5 L:632 |
| `RESP_HEADER_LEN` | 10 | §3.5 L:633 |
| `DATA_HEADER_LEN` | 14 | §3.5 L:634 |
| `MAX_DATAGRAM` | 1200 | §3.5 L:635 |
| `MAX_PLAINTEXT` | 1170 | §3.5 L:636 |
| `IK_MSG1_LEN` | 174 | §2.3 L:506 |
| `IK_MSG2_LEN` | 81 | §2.3 L:507 |
| `INIT_PACKET_LEN` | 196 | §2.3 L:508 |
| `RESP_PACKET_LEN` | 107 | §2.3 L:509 |

Reference suite: **`P256 / ChaChaPoly / Blake2b`**, Noise protocol name
`Noise_IK_P256_ChaChaPoly_BLAKE2b` **[CITE §2.2 L:474–476]**. `PK` = 65,
`TAG` = 16 **[CITE §2.3 L:501–502]**.

---

## 1. Byte-offset tables for the three headers

### 1.1 The endianness rule, stated once

**[CITE §3.1 L:542–551, RATIFIED 2026/08/14 ruling 64]** *All multi-byte
header fields are little-endian.* The rule reaches **exactly three
fields** in the entire grammar: `sender_index`, `receiver_index`,
`counter`. Everything else in every header is a single byte
(`type`, `version`) or an opaque octet string (msg1, msg2, mac1,
ciphertext), and octet strings have no byte order.

**[CITE §3.1 L:553–559]** Two things the rule explicitly does *not* reach:
§8.1's varints (big-endian, RFC 9000 §16) and §2.4/§6.7's canonical static
comparison (a lexicographic octet-string compare, not an integer
encoding). See Finding F2 for a third thing it arguably should have named
and did not.

**Concrete little-endian encoding — the thing that must not be ambiguous:**

| Value | Type | Wire bytes, in transmission order |
|---|---|---|
| `sender_index = 0x0A0B0C0D` | `u32` LE | `0D 0C 0B 0A` |
| `receiver_index = 0x0A0B0C0D` | `u32` LE | `0D 0C 0B 0A` |
| `sender_index = 0x00000001` | `u32` LE | `01 00 00 00` |
| `counter = 0x0102030405060708` | `u64` LE | `08 07 06 05 04 03 02 01` |
| `counter = 0` | `u64` LE | `00 00 00 00 00 00 00 00` |
| `counter = 1` | `u64` LE | `01 00 00 00 00 00 00 00` |
| `counter = 2` | `u64` LE | `02 00 00 00 00 00 00 00` |

That is: **least significant byte first, at the lowest offset**. In Rust
terms, `u32::to_le_bytes` / `u64::to_le_bytes` written at the field
offset, and `u32::from_le_bytes` / `u64::from_le_bytes` read back.
**[DERIVE — confidence: certain]** This is the definition of
little-endian; no interpretive latitude.

### 1.2 `InitHeader` — 6 bytes

**[CITE §3.2 L:572]** `type(1) ‖ version(1) ‖ sender_index(4)`.

| Offset | Len | Field | Type | Byte order | Value |
|---|---|---|---|---|---|
| 0 | 1 | `type` | `u8` | n/a | `0x01` (`PKT_HANDSHAKE_INIT`) |
| 1 | 1 | `version` | `u8` | n/a | `0x01` (`VERSION`) |
| 2 | 4 | `sender_index` | `u32` | **little-endian** | initiator's random nonzero `u32` |

Total **6 B**, matching `INIT_HEADER_LEN` **[CITE §3.5 L:632]**. No
padding, no reserved bytes, no `receiver_index` — the initiator does not
yet know the responder's index, and the field is absent, not zeroed
**[DERIVE from §3.2 L:572 — confidence: certain; the grammar line has
three components and the total is 6]**.

`sender_index` is drawn per §17.3: a **random nonzero `u32`**, re-drawn
while present in *either* the session table or the pending table, from the
endpoint RNG, off-path-unpredictable by requirement **[CITE §17.3
L:4813–4815]**.

**Hexdump note, load-bearing for golden vectors:** the first two bytes of
a HandshakeInit are `01 01` — `type` and `version` are *the same byte
value*. A transposition of the two fields is invisible in an Init vector.
See Finding F4.

### 1.3 `RespHeader` — 10 bytes

**[CITE §3.3 L:585]** `type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)`.

| Offset | Len | Field | Type | Byte order | Value |
|---|---|---|---|---|---|
| 0 | 1 | `type` | `u8` | n/a | `0x02` (`PKT_HANDSHAKE_RESP`) |
| 1 | 1 | `version` | `u8` | n/a | `0x01` (`VERSION`) |
| 2 | 4 | `sender_index` | `u32` | **little-endian** | responder's random nonzero `u32` |
| 6 | 4 | `receiver_index` | `u32` | **little-endian** | the initiator's index this response answers |

Total **10 B**, matching `RESP_HEADER_LEN` **[CITE §3.5 L:633]**.

**Field order is `sender` then `receiver`** — the sender's own index comes
first **[CITE §3.3 L:585, the grammar line reads left to right]**.
Confidence: certain, but note this is pinned by *one* line of grammar and
the two fields have identical type and width, so a swap is a silent
misread. A golden vector must use two *distinct* index values to pin it
(Finding F4).

### 1.4 `DataHeader` — 14 bytes

**[CITE §3.4 L:598]** `type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)`.

| Offset | Len | Field | Type | Byte order | Value |
|---|---|---|---|---|---|
| 0 | 1 | `type` | `u8` | n/a | `0x03` (`PKT_DATA`) |
| 1 | 1 | `version` | `u8` | n/a | `0x01` (`VERSION`) |
| 2 | 4 | `receiver_index` | `u32` | **little-endian** | the *recipient's* session index |
| 6 | 8 | `counter` | `u64` | **little-endian** | the counter the seal returned |

Total **14 B**, matching `DATA_HEADER_LEN` **[CITE §3.5 L:634]**.

**There is no `sender_index` on a Data packet** **[DERIVE from §3.4 L:598
— confidence: certain]**. Routing is by `receiver_index` alone, which the
recipient minted and therefore recognises **[CITE §3.4 L:608–609]**. This
asymmetry with `RespHeader` (which carries both) is deliberate and is why
`DATA_HEADER_LEN` is 14 and not 18.

**There is no cleartext length field** **[CITE §3.4 L:622–623]**: the AEAD
yields the exact plaintext length and the frame parser runs to the end of
it.

---

## 2. Full packet layouts

### 2.1 `HandshakeInit` (type `0x01`) — 196 bytes

**[CITE §3.2 L:572–573]** `InitHeader(6) ‖ hiss IK msg1(174) ‖ mac1(16)`.

msg1's internal structure **[CITE §2.3 L:495]**:
`MSG1_LEN = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)`, annotated in the
spec as `e ‖ enc_s ‖ enc_payload`.

| Offset | Len | Component | Notes |
|---|---|---|---|
| 0 | 1 | `type` = `0x01` | header |
| 1 | 1 | `version` = `0x01` | header |
| 2 | 4 | `sender_index` (u32 LE) | header |
| **6** | **174** | **hiss IK msg1** | opaque to slither |
| 6 | 65 | msg1 `e` — initiator ephemeral public | cleartext, SEC1 uncompressed `0x04 ‖ X ‖ Y` |
| 71 | 81 | msg1 `enc_s` — encrypted initiator static | 65 B ciphertext ‖ 16 B tag |
| 71 | 65 | ↳ ciphertext of the static | |
| 136 | 16 | ↳ AEAD tag | |
| 152 | 28 | msg1 `enc_payload` — encrypted 12-byte timestamp | 12 B ciphertext ‖ 16 B tag |
| 152 | 12 | ↳ ciphertext of `ts_secs ‖ ts_nanos` | |
| 164 | 16 | ↳ AEAD tag | |
| **180** | **16** | **`mac1`** | keyed on the **responder's** static |
| — | **196** | **total** | |

**Arithmetic, shown explicitly:**

```
msg1  = 65 + (65 + 16) + (12 + 16)
      = 65 +     81    +    28
      = 174                        ✓ matches IK_MSG1_LEN = 174  (§2.3 L:506)

init  = INIT_HEADER_LEN + MSG1_LEN + MAC1_LEN
      = 6              + 174      + 16
      = 196                        ✓ matches INIT_PACKET_LEN = 196 (§2.3 L:508)

offsets: 0 + 6 = 6 ; 6 + 174 = 180 ; 180 + 16 = 196   ✓ contiguous, no gaps
```

**Both totals sum.** No spec bug here.

**Confidence on the msg1 *internal* split (offsets 6/71/136/152/164):**
**[DERIVE — high, not certain]**. §2.3 L:495 gives the three terms and the
annotation `e ‖ enc_s ‖ enc_payload`, which fixes the order. Standard
Noise IK msg1 is `e, es, s, ss, payload` — the ephemeral in clear, then
the static encrypted under the `es` key, then the payload encrypted under
the `ss` key — and §5.2 L:726–727 confirms "the payload sits inside msg1's
encrypted tail, **after** `e, es, s, ss`". The AEAD-tag-after-ciphertext
ordering within each encrypted block is the Noise/RFC 8439 convention, not
a slither statement. **This internal split is a property of hiss's msg1,
not of slither's wire.** slither treats msg1 as 174 opaque bytes; if hiss
laid it out differently the offsets 71/136/152/164 would move while 6/180
would not. A golden vector should pin the *whole 174 bytes*, and may
annotate the split, but slither's own conformance rests on offsets 6 and
180 only.

### 2.2 `HandshakeResp` (type `0x02`) — 107 bytes

**[CITE §3.3 L:585–586]** `RespHeader(10) ‖ hiss IK msg2(81) ‖ mac1(16)`.

msg2's internal structure **[CITE §2.3 L:496]**: `MSG2_LEN = PK + TAG`,
annotated `e ‖ the empty payload's tag`.

| Offset | Len | Component | Notes |
|---|---|---|---|
| 0 | 1 | `type` = `0x02` | header |
| 1 | 1 | `version` = `0x01` | header |
| 2 | 4 | `sender_index` (u32 LE) | header — responder's index |
| 6 | 4 | `receiver_index` (u32 LE) | header — echoes initiator's index |
| **10** | **81** | **hiss IK msg2** | opaque to slither |
| 10 | 65 | msg2 `e` — responder ephemeral public | cleartext, SEC1 uncompressed |
| 75 | 16 | msg2 tag — the **empty** payload's AEAD tag | zero-length ciphertext ‖ 16 B tag |
| **91** | **16** | **`mac1`** | keyed on the **initiator's** static |
| — | **107** | **total** | |

**Arithmetic, shown explicitly:**

```
msg2  = 65 + 16
      = 81                         ✓ matches IK_MSG2_LEN = 81   (§2.3 L:507)

resp  = RESP_HEADER_LEN + MSG2_LEN + MAC1_LEN
      = 10             + 81       + 16
      = 107                        ✓ matches RESP_PACKET_LEN = 107 (§2.3 L:509)

offsets: 0 + 10 = 10 ; 10 + 81 = 91 ; 91 + 16 = 107   ✓ contiguous, no gaps
```

**Both totals sum.** No spec bug here.

**msg2 carries no payload** **[CITE §3.3 L:591–592, §5.2 L:718, L:729–731]**:
its encrypted tail is the empty payload's AEAD tag alone. The tag still
authenticates the full transcript because the cipher is fully keyed after
`e, ee, se`, so a tampered msg2 fails completion. **[DERIVE]** The
zero-length ciphertext contributes zero bytes, so the "ciphertext ‖ tag"
block is 16 bytes and not 16 + something.

### 2.3 `Data` (type `0x03`) — 30 .. 1200 bytes

**[CITE §3.4 L:598–599]** `DataHeader(14) ‖ ciphertext(plaintext + 16)`.

| Offset | Len | Component |
|---|---|---|
| 0 | 1 | `type` = `0x03` |
| 1 | 1 | `version` = `0x01` |
| 2 | 4 | `receiver_index` (u32 LE) |
| 6 | 8 | `counter` (u64 LE) |
| **14** | `P` | ciphertext of the `P`-byte plaintext |
| `14 + P` | 16 | AEAD tag |
| — | `30 + P` | total |

```
minimum datagram = 14 + 0 + 16 = 30   ✓ §3.4 L:624 "a 30-byte datagram"
maximum datagram = 14 + 1170 + 16 = 1200 ✓ = MAX_DATAGRAM (§3.5 L:635-636)
per-packet overhead = 14 + 16 = 30    ✓ §3.5 L:639
max sealed message  = 1170 + 16 = 1186 ✓ §2.1 L:464
```

All four sum. No spec bug.

**The 30-byte datagram (empty plaintext, tag-only ciphertext) is the
keepalive** **[CITE §3.4 L:624–626]**; it bypasses the frame layer and is
the only non-frame plaintext.

**There is no `mac1` on a Data packet** **[DERIVE from §3.4 L:598–599 —
confidence: certain]**. The grammar has two components; §4 speaks only of
HandshakeInit and HandshakeResp recipients (L:662–663); the DoS gate
exists to precede *DH work* (§4.2 L:667–670) and a Data packet does no DH.

---

## 3. The mac1 preimage, per packet type

### 3.1 The construction

**[CITE §4.1 L:650–653]**

```
key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
```

**Step 1 — the key.** An **unkeyed** BLAKE2b with a **32-byte** digest,
over the concatenation of the 12-byte label and the recipient's canonical
static.

- `MAC1_LABEL` = `b"slither mac1"` **[CITE §4.1 L:657]**, 12 bytes:
  `73 6C 69 74 68 65 72 20 6D 61 63 31`
  (`s l i t h e r ␠ m a c 1`). **[DERIVE — certain]** ASCII, no NUL
  terminator, no length prefix; the spec writes a Rust byte-string
  literal, whose bytes are exactly its characters.
- `recipient_static_canonical` = 65 bytes for the reference suite
  **[CITE §4.1 L:660–662]**.
- Key-derivation preimage length = **12 + 65 = 77 bytes** for the
  reference suite. **[DERIVE — certain]**
- Output = **32 bytes**. **[DERIVE from "BLAKE2b-256" — high]**;
  "BLAKE2b-*n*" names the digest length in bits by universal convention
  (RFC 7693). Note this is **BLAKE2b** (64-bit words, up to 64-byte
  digest), *not* BLAKE2s, even though a 32-byte digest is BLAKE2s's
  natural size — the spec says `b` twice and §4.4 L:683 says "fixed keyed-
  BLAKE2b for every suite". Confidence: high.

**Step 2 — the tag.** A **keyed** BLAKE2b with a **16-byte** digest and
the 32-byte key from step 1, over the packet prefix.

- BLAKE2b parameter block, fully determined **[DERIVE — high]**:
  `digest_length = 16`, `key_length = 32`, `fanout = 1`, `depth = 1`,
  all other parameters zero (leaf_length, node_offset, node_depth,
  inner_length, salt, personal). The keyed mode is RFC 7693 §2.9: the key
  is zero-padded to one 128-byte block and prepended as the first data
  block. `cryptoxide`'s `Blake2b::new_keyed(16, &key)` expresses exactly
  this. **This is the one raw primitive slither is permitted**
  (`CLAUDE.md`, §4.4 L:683–687).
- **No personalisation string and no salt.** **[GAP — low risk]** The
  spec does not mention either; the derivation is that they are absent
  (all-zero), because a construction that used them would have had to say
  so and because the label is already carried in the *key derivation*
  rather than in a personalisation field. Confidence: high, but this is
  an inference from silence — worth an explicit confirmation, since a
  personalised BLAKE2b produces entirely different bytes and the vector
  would freeze the wrong construction.
- Output = **16 bytes** = `MAC1_LEN` **[CITE §4.1 L:658]**.

### 3.2 The preimage extent, resolved to concrete offsets

"All packet bytes preceding the tag" **[CITE §3.2 L:580, §4.1 L:652]**
resolves as:

| Packet | mac1 field at | Preimage range | Preimage length |
|---|---|---|---|
| `HandshakeInit` (196 B) | `[180, 196)` | **`[0, 180)`** | **180 bytes** |
| `HandshakeResp` (107 B) | `[91, 107)` | **`[0, 91)`** | **91 bytes** |
| `Data` | — no mac1 — | n/a | n/a |

**[DERIVE — certain, given exact packet lengths]** 196 − 16 = 180;
107 − 16 = 91. The preimage therefore covers **the header and the whole
Noise message**, including the `type` and `version` bytes and the
little-endian index field(s). Ruling 64 states this consequence
explicitly: "mac1's preimage is 'all packet bytes preceding the tag'
(§4.1), so the header bytes feed the DoS gate" — which is why endianness
had to be settled before the freeze.

**Caveat, and it is Finding F1:** "all packet bytes preceding the tag" is
well-defined only if the packet length is *exactly* 196 / 107. §3.1
L:561–563 gates on "shorter than its type's **fixed minimum**", which does
not reject an over-long Init or Resp. Under that reading a 200-byte Init
has two candidate tag positions (offset 180 by the fixed layout, offset
184 by "the last 16 bytes") and the preimage extent is ambiguous. See
Findings.

### 3.3 Which static keys which direction — the two differ

**[CITE §4.1 L:662–663, §3.2 L:579–580, §3.3 L:593]**

| Packet | Direction | mac1 key derived from | Rationale |
|---|---|---|---|
| `HandshakeInit` | initiator → responder | the **responder's** static (the recipient) | the sender must already know whom it is dialling |
| `HandshakeResp` | responder → initiator | the **initiator's** static (the recipient) | the responder learned it from msg1's `enc_s` |

The rule is uniform — **mac1 is always keyed on the recipient's static** —
but because the two packets travel in opposite directions, the two
packets' mac1 keys are derived from *different* public keys. **A golden
vector must therefore pin two distinct key derivations, and a symmetric
implementation bug (keying both on the same static) is only caught by a
vector that uses two different static key pairs.**

**[DERIVE — certain]** §4.3 L:673–676: the key is derived from *public*
data, so anyone holding the recipient's static can mint mac1-valid
packets. mac1 is an anti-amplification / cheap-reject gate, not an
authenticator; the real authentication is the Noise handshake. Verified
**before any curve or DH work** **[CITE §4.2 L:667]**.

---

## 4. The canonical static encoding (§2.4) and mac1 keying

**[CITE §2.4 L:514–517]** The canonical encoding of a static public key is
**the `AsRef<[u8]>` octets of `Curve::PublicKey`**:

| Curve | Canonical encoding | Length |
|---|---|---|
| **P-256 (reference)** | uncompressed SEC1 storage form, `0x04 ‖ X(32) ‖ Y(32)` | **65 bytes** |
| X25519 | raw | 32 bytes |

**Reference-suite length: 65 bytes.** First byte is always `0x04`
**[CITE Appendix A.3 L:5176–5177]**, which also records that a
*compressed* input normalises to the same 65 canonical bytes — hiss
enforces normalisation regardless of the parse form **[CITE §2.4 L:516]**.

**One encoding, three uses** **[CITE §2.4 L:517–519]**:

1. **mac1 keying** (§4) — it is the second half of the key-derivation
   preimage: `BLAKE2b-256(b"slither mac1" ‖ static[0..65])`.
2. The simultaneous-open tie-break comparison (§6.7).
3. Identity maps and accept policies key on it.

**How it feeds mac1, concretely** **[DERIVE — certain]**: the key
derivation hashes 77 bytes for P-256 — 12 label bytes at offsets `[0,12)`
of the preimage, then the 65 canonical static bytes at `[12,77)`, the
first of which is `0x04`. No length prefix, no separator, no padding
between them; §4.1 writes `‖`, plain concatenation. Because all canonical
encodings within a suite are equal-length **[CITE §2.4 L:519–521]**, this
concatenation is unambiguous without a length field.

**[CITE §2.4 L:523–526, §4.4 L:691–699]** The ratified change: P-256 mac1
keying moved from the pre-release drafts' **33-byte compressed** form to
the **65-byte uncompressed** form, fixed at the first golden freeze. No
shipped bytes had to move. **An implementation that keys mac1 on 33
compressed bytes is wrong**, and that is exactly the kind of error the
golden vector exists to freeze correctly rather than to freeze wrongly.

**[CITE §2.4 L:521–522, A.3 L:5169–5172]** The only bound required is
`Curve::PublicKey: AsRef<[u8]>`, a slither-side `where` clause; `Ord` is
**not** required.

---

## 5. The prologue

**[CITE §5.1 L:706]** `PROLOGUE = b"slither\x01"` (fixed; identical for
every handshake).

**Derived byte sequence [DERIVE — certain]:**

```
73 6C 69 74 68 65 72 01
 s  l  i  t  h  e  r  \x01
```

**Length: 8 bytes** — seven ASCII characters of `slither` plus one
version byte. Derived character by character:

| # | Char | Hex |
|---|---|---|
| 0 | `s` | `0x73` |
| 1 | `l` | `0x6C` |
| 2 | `i` | `0x69` |
| 3 | `t` | `0x74` |
| 4 | `h` | `0x68` |
| 5 | `e` | `0x65` |
| 6 | `r` | `0x72` |
| 7 | — | `0x01` |

No NUL terminator, no length prefix, no trailing newline. The `\x01`
escape is a single byte, not the two characters `\` and `x01`.

**[DERIVE — high]** The trailing `0x01` **equals `VERSION`** (§3.1 L:534)
and is not a coincidence: §5.1 L:709–711 says the prologue "carries the
wire version, binding it into the Noise transcript: a peer that
mis-classifies the version does not merely parse garbage, it fails the
handshake cryptographically." So the version appears on the wire twice per
handshake packet — once in the clear at header offset 1, and once
*implicitly*, inside the Noise `h` chain, via the prologue. The two must
agree; a wire-version bump moves both.

**[DERIVE]** The prologue is hashed into `h` by Noise's
`MixHash(prologue)` immediately after `InitializeSymmetric(protocol_name)`.
It does not appear as bytes in any packet. **A golden vector cannot
observe the prologue directly** — it observes it only through its effect
on every ciphertext and tag in the handshake. Appendix B L:5202–5203 lists
"the prologue" among the frozen golden vectors; that must mean *pinning
the constant itself* (a byte-equality assert on the 8 bytes), because it
is not extractable from a datagram.

---

## 6. The msg1 payload — the 12-byte timestamp

**[CITE §5.2 L:715–717]**

```
msg1 payload (12 B, encrypted in msg1's tail):
    ts_secs(8, BE) ‖ ts_nanos(4, BE)
```

| Offset within payload | Len | Field | Type | Byte order |
|---|---|---|---|---|
| 0 | 8 | `ts_secs` — seconds since the Unix epoch | `u64` | **big-endian** |
| 8 | 4 | `ts_nanos` — nanoseconds within the second | `u32` | **big-endian** |
| — | 12 | total = `TIMESTAMP_LEN` = `MSG1_PAYLOAD_LEN` | | |

**Big-endian, as written.** `ts_secs = 0x0000000068000000` encodes as
`00 00 00 00 68 00 00 00`; `ts_nanos = 0x0A0B0C0D` encodes as
`0A 0B 0C 0D`. **This is the reverse convention from the three header
fields.** See Finding F2 — I am reporting the tension, not resolving it.
The spec text at L:717 says BE and ruling 64's rule is scoped to *header*
fields, so **BE is what the spec says and BE is what the implementation
must emit** absent a new ruling.

**Semantics [CITE §5.3 L:735–740]:** the wall clock
(`secs_since_unix_epoch ‖ nanos`), the one wall-clock read in the
protocol, forced **strictly greater** than the previous timestamp this
endpoint emitted — endpoint-global across all connections and connection
generations. Consequence for a golden vector: the timestamp is an
*input* to be pinned, never sampled.

**Where it sits [CITE §5.2 L:726–727]:** inside msg1's encrypted tail,
*after* `e, es, s, ss`, authenticated against the initiator's proven
static. By the layout of §2.1 above, its ciphertext occupies packet
offsets **`[152, 164)`** with its AEAD tag at **`[164, 180)`**
**[DERIVE — high; inherits the msg1-internal-split caveat of §2.1]**.

### 6.1 Visibility — what a golden vector can and cannot pin

**[CITE §3.2 L:577–578]** *"msg1's encrypted tail carries the 12-byte
payload `timestamp(12)` (§5.2); **nothing of it appears in the header**."*

**The msg1 payload is NOT visible in the header.** Stated explicitly, with
its consequences:

- The `InitHeader`'s 6 bytes are `type`, `version`, `sender_index`. There
  is no timestamp field, no fragment of one, and no length field that
  varies with it.
- **Confidentiality [CITE §5.3 L:742–746]:** the payload has Noise
  confidentiality level 2 — encrypted to the responder's static, opaque
  to any passive observer (**no clock-skew fingerprinting**), and
  authenticated. It is **not** forward-secret against a later compromise
  of the responder's static; acceptable, because the plaintext is a
  wall-clock reading, not a secret.
- **What a golden vector can pin:** (a) the 12 plaintext bytes, as an
  input, by asserting the BE encoding of a chosen `(secs, nanos)` pair;
  (b) the resulting 28 ciphertext-plus-tag bytes at `[152, 180)`, as an
  *output* — but only as an opaque snapshot, since they are the product
  of a ChaCha20-Poly1305 seal under a key that came out of two P-256 DHs.
- **What a golden vector cannot do:** read the timestamp back out of a
  captured datagram without the responder's static private key. A test
  that "checks the timestamp on the wire" is not possible at the byte
  level; it must either decrypt (i.e. run the responder) or assert the
  ciphertext bytes verbatim.
- **Sensitivity, which is the useful property:** because the payload is
  the last thing sealed and its tag closes msg1, changing a single
  timestamp bit changes 28 packet bytes at `[152,180)` **and** all 16
  mac1 bytes at `[180,196)` — but changes *nothing* at `[0,152)`. That
  makes a two-vector differential (same keys, two timestamps) a precise
  test of the payload's position and encoding. **[DERIVE — high]**

---

## 7. The Data packet's AEAD associated data, and counter/nonce identity

### 7.1 The AD extent

**[CITE §3.4 L:602–607]** *"**The 14 header bytes are the AEAD associated
data, verbatim.**"*

| | |
|---|---|
| **AD extent** | packet bytes **`[0, 14)`** — the entire `DataHeader`, nothing more, nothing less |
| **AD length** | 14 bytes, always |
| **AD contents** | `type(0x03) ‖ version(0x01) ‖ receiver_index(u32 LE) ‖ counter(u64 LE)` |
| **Transformation** | none — "verbatim". Not re-serialised, not canonicalised, not truncated |

**[CITE §3.4 L:603–607]** The counter inside the AD is *redundant* (it is
already the nonce) but harmless, and "AD = the header, verbatim" is the
simplest possible rule. Authenticating `receiver_index`, `type` and
`version` is **stronger than WireGuard**, whose data AEAD uses empty AAD,
at zero cost: header tampering becomes tag-detectable.

**[DERIVE — certain]** The AD must be constructed **before** sealing,
which is why the counter has to be knowable in advance; Appendix A.2
L:5157–5160 says exactly this.

**Neither handshake packet has an AD in this sense.** msg1/msg2's internal
AEADs use Noise's `h` as their associated data — a hiss-internal matter,
not a slither wire field.

### 7.2 Counter ↔ nonce: does the spec support the byte-identity claim?

**The claim [CITE §3.4 L:612–614]:** *"Little-endian per §3.1, so these
eight bytes **are** the low eight bytes of the ChaChaPoly nonce, not a
byte-reversal of them."* Ruling 64 makes the same claim: "it makes the
`counter` on the wire byte-identical to the ChaCha20-Poly1305 nonce Noise
derives from it, which big-endian would leave as its byte-reverse."

**My assessment: the claim is TRUE for the reference suite, but the spec
does not contain the premise that makes it true, and the claim is
suite-specific in a section that is otherwise suite-independent.**

*Where it is right.* The Noise specification's ChaChaPoly cipher function
encodes the 96-bit nonce as **32 zero bits followed by the little-endian
encoding of `n`**. So for `counter = n`:

```
ChaChaPoly nonce (12 B) = 00 00 00 00 ‖ LE64(n)
                          └ [0,4) ─┘   └ [4,12) ┘
DataHeader bytes [6,14) = LE64(n)
```

The eight header bytes at `[6,14)` are **byte-for-byte identical** to
nonce bytes `[4,12)`, in the same order. Under the old big-endian rule
they would have been the exact byte-reversal. The ruling's substantive
point stands. **[DERIVE — high]**

*Three qualifications, in ascending severity.*

1. **"low eight bytes" is loose wording.** They are the *trailing* eight
   bytes of the 12-byte nonce block. Read as a little-endian 96-bit
   integer, the nonce's *low* bytes are at offsets `[0,8)`, and the whole
   block's numeric value would be `n · 2³²`, not `n`. "The last eight
   bytes" or "nonce bytes `[4,12)`" would be exact. Editorial. **(F6)**
2. **The premise is not in SPEC.md.** Nowhere in the sections I am
   permitted to read does the spec state how the 12-byte ChaChaPoly nonce
   is constructed from the 64-bit counter. §2.1 L:459 says only "The
   counter is the AEAD nonce". So §3.4's byte-identity claim is not
   *self-contained*: it is verifiable only against the Noise
   specification. That is fine as rationale but means **the claim cannot
   be checked from SPEC.md alone**, and it is not a wire requirement
   either way — the wire requirement is "counter, u64, little-endian",
   full stop. **(F5)**
3. **It is suite-specific, in a suite-independent section.** Noise
   encodes the AESGCM nonce as 32 zero bits followed by the **big-endian**
   encoding of `n`. Under a hypothetical `P256/AESGCM/Blake2b` suite the
   byte-identity claim inverts and becomes *false* — the header counter
   would be the byte-reversal of the nonce's counter bytes. §2.2 L:479–481
   asserts "the data-packet header … [is] suite-independent", and it is:
   the *rule* (LE) does not move. But **the second of ruling 64's two
   stated reasons evaporates outside ChaChaPoly**, and §3.4's sentence
   reads as a general property when it is not. This does not change a
   byte of wire v1. It is a rationale that should not be relied on when a
   second suite ships. **(F5)**

**Bottom line for the implementation:** encode `counter` as
`u64::to_le_bytes` at offset 6. That is required by §3.1 regardless of
whether the nonce rationale holds. Do not *derive* the header bytes from a
nonce object; derive them from the `u64`.

### 7.3 Which counter value

**[CITE §3.4 L:610–611]** The header carries *"exactly the value the seal
returned"* — the hiss-owned monotonic send counter, which is
simultaneously the AEAD nonce, the packet number (§7.1) and the epoch
selector (§7.7). It is **strictly monotonic from 0** and never
caller-chosen **[CITE §2.1 L:458]**. The first data packet in a direction
therefore carries `counter = 0`, encoded `00 00 00 00 00 00 00 00`.

**[CITE §2.1 L:465]** `2⁶⁴ − 1` is reserved for hiss's `Rekey()`; the
usable space is `0 ..= 2⁶⁴ − 2`.

**[CITE Appendix A.2 L:5157–5165]** `DatagramSend::next_counter()` has
**shipped**, and the §3.4 mirror-and-assert interim (L:617–621) is
**superseded** — retained in §3.4 only as the fallback shape, not the
operative mechanism. §3.4's own prose still reads *"Until Appendix A.2's
counter accessor ships…"*, which is now counterfactual. **(F7 — stale
prose, already acknowledged by A.2.)** The implementation should call the
accessor, not mirror.

---

## 8. The derivable / not-derivable split

This is the section the golden-vector author must act on. **Hand-derivable**
bytes I assert here and the implementation must match; a mismatch is a bug
in one of us. **Not-derivable** bytes can only be snapshot from a running
implementation, and a snapshot is worthless unless every input that
determines it is pinned.

### 8.1 `HandshakeInit` (196 B)

| Range | Len | Class | What pins it |
|---|---|---|---|
| `[0,1)` | 1 | **Derivable** | `0x01` — §3.1 L:535 |
| `[1,2)` | 1 | **Derivable** | `0x01` — §3.1 L:534 |
| `[2,6)` | 4 | **Derivable given the input** | `u32::to_le_bytes(sender_index)`. The *value* is RNG-drawn (§17.3); the *encoding* is asserted. Pin the value as a test input. |
| `[6,71)` | 65 | **Not derivable** | initiator ephemeral public = `G · e_i`. P-256 scalar mult. |
| `[71,136)` | 65 | **Not derivable** | ChaChaPoly ciphertext of the initiator's 65-byte static, key from `es`. |
| `[136,152)` | 16 | **Not derivable** | that block's Poly1305 tag. |
| `[152,164)` | 12 | **Not derivable** | ChaChaPoly ciphertext of the 12-byte BE timestamp, key from `ss`. |
| `[164,180)` | 16 | **Not derivable** | that block's Poly1305 tag. |
| `[180,196)` | 16 | **Not derivable** | keyed-BLAKE2b-128 over `[0,180)`. |

Derivable: **6 of 196 bytes** (3 %). Everything else is downstream of a
P-256 scalar multiplication or a BLAKE2b compression.

But the **structure** is fully derivable and is what the assertion should
test: offsets 0/1/2/6/71/136/152/164/180, the lengths 6/174/16, the LE
encoding at `[2,6)`, the `0x04` leading byte at offset 6, and the mac1
preimage extent `[0,180)`.

### 8.2 `HandshakeResp` (107 B)

| Range | Len | Class | What pins it |
|---|---|---|---|
| `[0,1)` | 1 | **Derivable** | `0x02` — §3.1 L:536 |
| `[1,2)` | 1 | **Derivable** | `0x01` |
| `[2,6)` | 4 | **Derivable given the input** | `u32::to_le_bytes(responder sender_index)` |
| `[6,10)` | 4 | **Derivable given the input** | `u32::to_le_bytes(receiver_index)`; **must byte-equal the Init's `[2,6)`** — a cross-packet invariant a vector can assert with no crypto at all. Corroborated by §5.5 L:834–835: on completion "our receiver index = our `sender_index`, the peer's = the response's `sender_index`", and by §5.5 L:820–821, where completion requires an **index match** |
| `[10,75)` | 65 | **Not derivable** | responder ephemeral public = `G · e_r` |
| `[75,91)` | 16 | **Not derivable** | empty payload's Poly1305 tag |
| `[91,107)` | 16 | **Not derivable** | keyed-BLAKE2b-128 over `[0,91)` |

Derivable: **10 of 107 bytes** (9 %) — the whole header. Plus the
structural assertions: offsets 0/1/2/6/10/75/91, lengths 10/81/16, the
`0x04` at offset 10, the mac1 preimage extent `[0,91)`, and the
index-echo invariant.

### 8.3 `Data` (30 + P bytes)

| Range | Len | Class | What pins it |
|---|---|---|---|
| `[0,1)` | 1 | **Derivable** | `0x03` |
| `[1,2)` | 1 | **Derivable** | `0x01` |
| `[2,6)` | 4 | **Derivable given the input** | `u32::to_le_bytes(receiver_index)` |
| `[6,14)` | 8 | **Derivable given the input** | `u64::to_le_bytes(counter)`; first packet in a direction is all-zero |
| `[14, 14+P)` | P | **Not derivable** | ChaCha20 keystream XOR plaintext, key from the handshake Split |
| `[14+P, 30+P)` | 16 | **Not derivable** | Poly1305 tag over AD `[0,14)` ‖ ciphertext |

Derivable: **14 of `30+P` bytes** — the whole header, which is also the
whole AD. This is the packet with the highest derivable fraction and the
one where a hand-written expectation is most valuable: **the AD is exactly
the 14 bytes I can write down by hand**, so `AD == packet[0..14]` is a
pure structural assertion needing no crypto.

### 8.4 Not-derivable items and their required pinned inputs

A golden vector that is not reproducible is not a golden vector. For each
not-derivable artefact, everything that must be fixed:

**msg1's three encrypted blocks and its ephemeral (`[6,180)`):**

1. Suite = `P256 / ChaChaPoly / Blake2b`, and thus the protocol-name
   string `Noise_IK_P256_ChaChaPoly_BLAKE2b` (it is hashed into `h`).
2. `PROLOGUE` = the 8 bytes above.
3. Initiator **static private scalar** (32 bytes, fixed literal).
4. Responder **static private scalar** (32 bytes) — needed because IK's
   `es`/`ss` both use the responder's static, and the initiator must
   embed the corresponding *public* key as its dial target.
5. Initiator **ephemeral private scalar** (32 bytes) — **pin the scalar,
   not an RNG seed.** See the warning below.
6. `ts_secs` and `ts_nanos` as explicit `u64`/`u32` literals.

**msg2's ephemeral and tag (`[10,91)`):**

7. Everything above (msg2's transcript incorporates msg1).
8. Responder **ephemeral private scalar** (32 bytes).

**mac1 on Init (`[180,196)`):**

9. The responder's static **public** key in canonical 65-byte form.
10. `sender_index` (it is inside the preimage).
11. Every byte of `[6,180)`, i.e. items 1–6 transitively.

**mac1 on Resp (`[91,107)`):**

12. The initiator's static public key (65 canonical bytes) — **note this
    is the other one**; a vector that reuses the same static for both
    directions cannot catch a keying-direction bug.
13. Both index values.
14. Every byte of `[10,91)`.

**Data ciphertext and tag:**

15. A completed handshake with all of the above, since the transport keys
    come from Noise's `Split()` (the initiator's sending key is the first
    output).
16. Direction (initiator→responder vs. responder→initiator; the two use
    different keys).
17. `counter` as an explicit literal.
18. `receiver_index` as an explicit literal (it is in the AD, so it
    changes the tag).
19. The exact plaintext bytes. If the plaintext is a frame stream, the
    *frame encoding* must itself be pinned, which drags §8's varints into
    the vector — consider a raw-plaintext vector (or the empty-plaintext
    keepalive) so the packet layer can be frozen independently of the
    frame layer.

**The RNG-seed warning — read this before writing the vectors.** Do
**not** pin an RNG seed and let key generation run. P-256 scalar
generation is rejection-sampled: the number of draws, and hence the
resulting key, depends on the RNG's internal byte-stream layout. A
semver-compatible bump of `rand_core` or of the DH provider can silently
change the derived key and therefore every byte of the vector — and
`CLAUDE.md` records that `Cargo.lock` is deliberately **not** committed,
so the graph re-resolves on every run. **Pin the scalars as literal byte
arrays and inject them.** The same applies to `sender_index` /
`receiver_index`, which §17.3 draws from the endpoint RNG: pin the values,
not the source.

**Cross-check the vector is honest.** A snapshot vector proves nothing
about correctness — it proves only that behaviour has not *changed*. Its
value comes from being taken once, at a moment when an independent reading
(this document) agrees with the structure. Where the implementation and
this derivation agree, freeze. Where they differ, resolve before freezing.

---

## 9. Findings, ranked by severity

### F1 — HIGH. The pre-AEAD length gate says "minimum" where the packet length is *fixed*, leaving the mac1 preimage extent ambiguous for over-long handshake packets.

**[CITE §3.1 L:561–563]** *"A datagram shorter than its type's fixed
minimum, longer than `MAX_DATAGRAM`, or bearing an unknown type or version
is silently dropped."*

For `PKT_DATA` this is correct — 30 is a genuine minimum and the length
varies. For `PKT_HANDSHAKE_INIT` and `PKT_HANDSHAKE_RESP` the length is
**fixed**: §2.3 names the constants `INIT_PACKET_LEN` / `RESP_PACKET_LEN`
(not `_MIN_`), and §3.2/§3.3's headings read "— 196 bytes" and "— 107
bytes", not "at least". A gate that only rejects *short* packets admits a
200-byte HandshakeInit, and then:

- "all packet bytes preceding the tag" (§4.1 L:652) has two readings —
  the tag is at offset 180 by the fixed layout, or at offset 184 as "the
  last 16 bytes". The mac1 preimage extent, the one thing this document
  had to resolve to concrete offsets, becomes implementation-defined.
- Under the "fixed layout" reading, trailing bytes are unauthenticated by
  mac1 *and* unauthenticated by Noise: the packet becomes **malleable**.
  An on-path attacker can append arbitrary bytes and the packet still
  passes. That is a real (if low-impact) property to hand an attacker at
  the DoS gate, and it also makes byte-identical replay detection by
  full-datagram comparison unreliable.
- Two conforming implementations could disagree on whether a peer's
  packet is valid.

**Three phrasings of the same gate exist, and they do not agree:**

| Cite | Wording | Reading |
|---|---|---|
| §3.1 L:561 | "shorter than its type's **fixed minimum**" | length ≥ min |
| §6.2 L:975 | "**short/oversize**, unknown type/version, bad mac1 — all silent" | length ≥ min, ≤ MAX_DATAGRAM |
| §5.5 L:820 | "the first **length-correct**, index-matching, mac1-valid msg2" | **exact length** |

§5.5's "length-correct" is the decisive one: it is a normative behavioural
clause about msg2 acceptance, and "correct" is not "at least". Under the
§3.1 reading a 111-byte msg2 would be "length-correct"; under §5.5's it
would not.

**This is a prose-versus-prose conflict, and per the working rules I am
not picking a side.** But I record which side I believe: **§5.5 L:820 and
the `_PACKET_LEN` constants hold the intent (exact length); §3.1 L:561
holds the bug**, because "fixed minimum" is a phrase written for the Data
case — where it is exactly right — and generalised across all three types
without re-examination. That is precisely ruling 64's "unargued default"
pattern: a line nobody ever contested, invisible to the process that
ratified it.

**Requested ruling:** state explicitly that `HandshakeInit` and
`HandshakeResp` are dropped unless the datagram length is **exactly**
`INIT_PACKET_LEN` / `RESP_PACKET_LEN`. If the answer is instead "trailing
bytes tolerated", then §4.1 must say *which* offset the tag sits at, and
the malleability should be acknowledged.

### F2 — MEDIUM. Ruling 64's "two things this rule does not reach" list omits the msg1 timestamp, which is the third multi-byte integer on the wire and stays big-endian.

**[CITE §5.2 L:717]** `ts_secs(8, BE) ‖ ts_nanos(4, BE)`.
**[CITE §3.1 L:542–544]** the LE rule reaches "exactly three fields …
`sender_index`, `receiver_index` and `counter`".
**[CITE §3.1 L:553–559]** *"**Two** things this rule does not reach"* —
§8.1's varints and §2.4's static comparison.

The enumeration reads as exhaustive and it is not. The msg1 timestamp is a
multi-byte integer encoding that appears on the wire (as ciphertext) and
is **big-endian**. It is not reached by the rule — it is a *payload*
field, not a *header* field — so the spec is not self-contradictory. But:

- §5 is dated `DRAFT 2026/08/13`, one day before ruling 64. A reader
  cannot tell from the document whether the BE at L:717 survived
  consideration or simply was not looked at. Ruling 64's own closing
  paragraph is about exactly this failure mode.
- The wire now carries **two byte orders in one datagram's semantic
  content** — LE `sender_index` at `[2,6)` and BE `ts_secs` at payload
  offset 0 — and unlike the varint case, the "they are never in the same
  cleartext" defence does **not** apply: the responder decrypts msg1
  while it still holds the header, so both orders *are* observable
  together by a legitimate implementation. Ruling 64's own consistency
  argument therefore does not cover this case, which suggests it was not
  considered.
- Implementation hazard: an implementer who reads §3.1 first will reach
  for `to_le_bytes` throughout.

**What I am asserting for the implementation:** the spec says BE at L:717,
and per `CLAUDE.md` the code matches the spec. **Emit BE.** But this needs
an explicit confirmation before the freeze — it is a wire byte, it is
about to become permanent, and the record does not show it was decided.

**A note in BE's favour, so the confirmation is informed:** a BE
`ts_secs` sorts correctly as an octet string, and §5.3's whole point is a
strictly-greater comparison against a stored previous timestamp. If any
code path ever compares stored timestamps as bytes, BE is the right
choice and LE would be a bug. That is a substantive reason to keep BE, not
merely inertia.

### F3 — MEDIUM. `mac1`'s BLAKE2b parameterisation is under-specified: personalisation and salt are never mentioned.

**[CITE §4.1 L:650–653]** gives `BLAKE2b-256(label ‖ static)` and
`keyed-BLAKE2b-128(key, data)`. RFC 7693's parameter block also carries a
16-byte salt and a 16-byte personalisation, both of which change every
output byte. The spec's silence is most naturally read as "both
all-zero", and the label is already carried in the key derivation, so
that reading is almost certainly right — but "almost certainly" is not
what one wants at a permanent freeze, and WireGuard (the acknowledged
precedent, §4.4 L:685) does *not* use personalisation either, which
supports the zero reading.

Also unstated, though I judge each unambiguous: that `BLAKE2b-256` is
**unkeyed** (it takes no key argument in the formula, so yes); that its
output is 32 bytes and becomes the *full* key of the second hash (so
`key_length = 32`); and that `BLAKE2b-256` means BLAKE2b-with-32-byte-digest
rather than BLAKE2s (§4.4 L:683 settles this: "fixed keyed-BLAKE2b").

**Requested:** one sentence in §4.1 fixing salt and personalisation as
absent/zero.

### F4 — MEDIUM (test-design). `type` and `version` are both `0x01` on a HandshakeInit, so an Init-only vector cannot detect a field transposition.

**[CITE §3.1 L:534–535]** `VERSION = 0x01` and `PKT_HANDSHAKE_INIT = 0x01`.
The first two bytes of every HandshakeInit are `01 01`. An implementation
that emitted `version ‖ type` instead of `type ‖ version` would produce a
byte-identical Init header, a byte-identical mac1, and a green test.

Not a spec bug — the collision is harmless on the wire, since both fields
are constants. It is a **golden-vector design requirement**:

- The vector set **must** include a `HandshakeResp` (`02 01`) and a
  `Data` (`03 01`) packet, not just an Init.
- The two index fields in `RespHeader` are the same type and width and sit
  adjacent, so the vector **must** use two *distinct*, *asymmetric* index
  values (e.g. `0x11223344` and `0xAABBCCDD`, never `0x01020304` and
  `0x04030201`) or a sender/receiver swap goes undetected.
- Likewise the LE assertion needs a **byte-asymmetric** value: an index
  of `0x00000001` (`01 00 00 00`) does distinguish LE from BE, but an
  index like `0x0A0B0C0D` makes the failure diagnosable at a glance.
  Never use a palindromic value.
- Pick a `counter` with eight distinct bytes for the same reason, in
  addition to a `counter = 0` case.

### F5 — LOW/MEDIUM. §3.4's counter/nonce byte-identity claim is true but suite-specific, and its premise is not stated anywhere in SPEC.md.

Detailed in §7.2 above. Summary: the claim holds under Noise's ChaChaPoly
nonce rule (32 zero bits ‖ LE64(n)) and *inverts* under Noise's AESGCM
rule (32 zero bits ‖ BE64(n)). §2.2 L:479–481 declares the data header
suite-independent; the *rule* is, but the *justification* at §3.4
L:612–614 and the second half of ruling 64's rationale are not. Nothing
about wire v1 changes. Flagged so that a future AESGCM suite does not
find a spec sentence claiming a property that is then false, and so that
nobody implements the header field by copying bytes out of a nonce object.

Additionally, SPEC.md never states the nonce construction, so the claim
cannot be verified from the spec alone. A one-clause parenthetical
("Noise encodes the ChaChaPoly nonce as 32 zero bits followed by LE64(n)")
would make §3.4 self-contained.

### F6 — LOW (editorial). "the low eight bytes of the ChaChaPoly nonce" should read "the last eight bytes" / "nonce bytes `[4,12)`".

**[CITE §3.4 L:612–613]**. They are the *trailing* eight bytes of the
12-byte nonce block. Read as a little-endian 96-bit integer the block's
low bytes are at `[0,4)` (the zeros) and its value is `n · 2³²`. The
substantive claim is unaffected; the wording invites a reader to place the
counter at nonce offset 0.

### F7 — LOW (stale prose, already acknowledged). §3.4 still describes the mirror-and-assert interim as current.

**[CITE §3.4 L:617–621]** *"Until Appendix A.2's counter accessor
ships…"* versus **[CITE Appendix A.2 L:5157–5165]** *"`next_counter()` —
**SHIPPED** … The §3.4 mirror-and-assert interim is superseded."* A.2
states it retains the §3.4 text "only as the fallback shape, not as the
operative mechanism", so the conflict is *acknowledged* rather than
undetected — but §3.4 reads standalone as current instruction, and an
implementer working from §3.4 alone would build the mirror. Suggest
tagging the paragraph "*(superseded — see Appendix A.2; retained as the
fallback shape)*".

### F8 — LOW. The receive-side treatment of index `0` is not stated.

**[CITE §17.3 L:4813]** minting draws a random **nonzero** `u32`, so no
session or pending ever holds index 0. The receiver-side consequence
(a packet bearing `receiver_index = 0` matches nothing and is dropped)
follows automatically from table lookup, so this is not a hole in
behaviour. But the *reason* index 0 is excluded from minting is never
given, and "0 is not a valid index" is not written down as a rule a
validator could cite. Harmless today; worth one clause, if only so a
future reader does not "optimise away" the nonzero draw.

### F9 — INFO. msg1's and msg2's internal offsets are hiss's layout, not slither's wire.

§2.1 and §2.2 of this document give offsets 71/136/152/164 within msg1 and
75 within msg2. These are derived from §2.3's size formula plus the Noise
IK message pattern, **not** from a slither statement about field
positions. slither's own conformance surface is: header at `[0,6)` /
`[0,10)`, an opaque 174 / 81 bytes, mac1 at `[180,196)` / `[91,107)`.
A golden vector should assert the whole opaque block; annotating the
internal split is useful documentation but is pinning *hiss's* wire, and
a hiss change there would turn the vector red for a reason outside
slither's control. Worth stating in the vector file so a future red is
diagnosed correctly.

### F10 — INFO. Appendix B lists "the prologue" among the golden vectors, but the prologue never appears in a datagram.

**[CITE Appendix B L:5202–5203]**. The prologue is `MixHash`-ed into `h`
and is observable only through its effect on downstream tags. "Freezing
the prologue" must mean a byte-equality assert on the 8-byte constant
itself, plus the transitive effect captured in the handshake vectors. Same
observation for "the 12-byte msg1 payload", which is only ever ciphertext
on the wire, and for "mac1's canonical keying", where what is freezable is
the 65-byte canonical static (an input) and the 16 mac1 bytes (an output),
not the intermediate 32-byte key. Naming which of the three each vector
pins would prevent a vector that asserts nothing.

### F11 — INFO. `MAX_PLAINTEXT` is called suite-independent but its formula contains the suite's tag size.

**[CITE §2.2 L:478–481]** *"Wire-visible consequences of the suite are the
handshake message sizes … and, **in principle, the AEAD tag size**;
everything above the seal — **the data-packet header**, the frame layer,
and every constant in §§8–15 — is suite-independent."*
**[CITE §3.5 L:636]** `MAX_PLAINTEXT` = 1170 (= `MAX_DATAGRAM` − 14 − 16).

The `14` is genuinely suite-independent (`DATA_HEADER_LEN`); the `16` is
`AEAD_TAG_LEN`, which §2.2 has just called suite-dependent in principle
and which §2.3 lists in the **per-suite derived sizes** table. So
`MAX_PLAINTEXT` sits in §3.5's flat constants table but is, strictly, a
per-suite derived size. Also true of the "30 B per-packet overhead"
(L:639) and the 30-byte keepalive datagram (L:624). Nothing moves for wire
v1 — every hiss AEAD in play has a 16-byte tag — and the header itself
really is suite-independent, so §2.2's sentence is defensible as written.
Recorded only so that a future non-16-byte-tag suite finds the derived
constants already identified rather than hard-coded.

---

## 10. Summary of what I assert

Every item below is asserted against the implementation. A disagreement is
a bug in one of the two readings.

1. `InitHeader` = `type:u8@0 ‖ version:u8@1 ‖ sender_index:u32-LE@2`, 6 B.
2. `RespHeader` = `type:u8@0 ‖ version:u8@1 ‖ sender_index:u32-LE@2 ‖ receiver_index:u32-LE@6`, 10 B.
3. `DataHeader` = `type:u8@0 ‖ version:u8@1 ‖ receiver_index:u32-LE@2 ‖ counter:u64-LE@6`, 14 B.
4. LE means least-significant byte at the lowest offset:
   `0x0A0B0C0D → 0D 0C 0B 0A`; `counter=1 → 01 00 00 00 00 00 00 00`.
5. `HandshakeInit` = `[0,6)` header ‖ `[6,180)` msg1 ‖ `[180,196)` mac1 = 196 B. Sums.
6. `HandshakeResp` = `[0,10)` header ‖ `[10,91)` msg2 ‖ `[91,107)` mac1 = 107 B. Sums.
7. `Data` = `[0,14)` header ‖ ciphertext ‖ 16-byte tag; 30 B min, 1200 B max. Sums.
8. mac1 preimage = `[0,180)` on Init (180 B), `[0,91)` on Resp (91 B). No mac1 on Data.
9. mac1 key = unkeyed `BLAKE2b-256` over 77 bytes = `73 6C 69 74 68 65 72 20 6D 61 63 31` ‖ 65 canonical static bytes. mac1 = keyed `BLAKE2b` with 32-byte key, 16-byte digest.
10. Init's mac1 keys on the **responder's** static; Resp's on the **initiator's**. Different keys.
11. Canonical static (P-256) = 65 bytes, `0x04 ‖ X(32) ‖ Y(32)`.
12. `PROLOGUE` = `73 6C 69 74 68 65 72 01`, 8 bytes. Never appears in a datagram.
13. msg1 payload = `ts_secs:u64-BE@0 ‖ ts_nanos:u32-BE@8`, 12 B, **big-endian**, encrypted, invisible in the header.
14. Data AD = packet bytes `[0,14)`, verbatim, 14 bytes, always.
15. Header `counter` bytes are byte-identical to ChaChaPoly nonce bytes `[4,12)` — true for this suite, and irrelevant to the wire rule, which is simply u64-LE.

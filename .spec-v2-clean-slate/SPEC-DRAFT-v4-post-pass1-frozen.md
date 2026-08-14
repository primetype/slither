# slither — protocol specification, wire version 1 (first release)

> **DRAFT v4, 2026/08/14 (unratified; walkthrough revisions applied).**
> This document is the complete specification of the slither protocol at
> **wire version 1 — the first released wire**. It supersedes all prior
> slither wire and specification text **wholesale**: `SPEC.md` (2026/07/16
> and 2026/07/17) and `SPEC-v2.md` (the phase-1 draft) are **pre-release
> drafts** — no released software ever spoke them — with **zero
> authority**, not incorporated by reference, and not compatible with this
> wire. There is no on-wire transition mechanism, because there is nothing
> released to transition from (§1.1). On ratification the DRAFT markers
> flip to RATIFIED and the code must match this file; a later change to
> either is a protocol revision, not an edit. Exactly one clause still
> awaits maintainer sign-off — Appendix A.1's no-fallback gate, pending
> joint work with hiss (§1.3); every other maintainer flag was ruled in
> the 2026/08/14 walkthrough. The two [OPEN] markers draft v1 carried are
> resolved (§6.4's re-home rule, §15.2's violation surface); **no [OPEN]
> markers remain**. British English, Oxford comma, dates `YYYY/MM/DD`.
>
> **Round-1 revisions applied (draft v1 → v2, 2026/08/13).** All 58 round-1
> review findings, resolved in eight clusters (clusters A and F are
> **partially superseded** by the draft-v4 ratchet-only ruling below):
> **A** — the CONTINUATION/CONTINUED restart-vs-rekey machinery redesigned
> over a three-valued LIVE/PENDING/NONE local state: flag routing gated on
> LIVE, CONTINUED computed (never hardcoded), the `Replaced` teardown
> deferred to `accept()`, mid-state-carrying entries frozen, §6.4's
> candidate-flag discard deleted (§5.4, §6.3–6.8).
> **B** — flow control made a real memory bound: retired receive halves
> credit the connection level, the re-grant formula corrected, a second
> reassembly-fragment bound added, consumption defined (§9–§10, §16.8).
> **C** — an anti-amplification budget (`AMPLIFICATION_FACTOR` = 3) binding
> all output to unvalidated addresses, and pre-roam packets fenced from the
> fresh path's congestion controller (§7.3, §13, §14).
> **D** — exactly-once stream delivery pinned by a per-space closed-stream
> watermark (§9.2, §9.7, §8.4).
> **E** — recovery aligned with RFC 9002: PTO armed only with ack-eliciting
> packets in flight, persistent congestion at the un-backed-off PTO, no
> window growth during recovery (§13, §14).
> **F** — the liveness anchor corrected to receive-keyed death, a
> persistent-keepalive floor, and idle sessions rekeying via the keepalive
> (§7.4–7.6).
> **G** — the core/shell seam completed: closing-state retention, stream IDs
> assigned at establishment, a uniform pull model with claim verbs, the
> wake-event set closed (§15.2, §16).
> **H** — post-AEAD structural frame failure is a signalled death (CLOSE
> with `PROTOCOL_VIOLATION`), `ConnectionLost::ProtocolViolation { code }`
> added, mutual close drains reply-free (§3.1, §8.2, §15, §18.1).
>
> **Final editorial pass (draft v2 → v3, 2026/08/14):** RAC-1, RAC-2,
> RBH-1, RBH-2 — four wire-free re-review fixes (§6.3/§6.9, §7.3,
> §9.6/§9.8/§8.7, §10.3). No wire byte, constant value, frame type, or
> error code moved.
>
> **Walkthrough revisions (draft v3 → v4, 2026/08/14).** Every maintainer
> flag walked 1:1 and ruled (the rulings log:
> `.spec-v2-clean-slate/rulings.md`); all rulings applied in one batch
> revision. The headline rulings: **ratchet-only rekey** — the periodic DH
> re-handshake is deleted; the §7.7 epoch ratchet is the only rekey; a
> connection has exactly one session, and a new handshake from an
> already-live static replaces the connection via `accept()` (§5.4, §6.4,
> §7.6 deleted, §7.8 collapsed); **no version bump** — nothing ever
> shipped, so this wire is the first: `VERSION = 0x01`,
> `PROLOGUE = b"slither\x01"` (§1.1); and, consequent on ratchet-only,
> the **CONTINUATION/CONTINUED flags are deleted** — the msg1 payload is
> the 12-byte timestamp alone, msg2 carries no payload, and the reference
> handshake packets are 196 B and 107 B (§2.3, §5.2). Clusters A and F of
> the round-1 revisions are partially superseded by the ratchet-only
> ruling; their surviving halves are re-ratified inline. Ratification of
> the whole document follows the Appendix A.1 resolution and a focused
> adversarial re-review of the restructured §§5–7.

slither is a Noise-over-UDP transport: mutually authenticated, encrypted
**streams, messages, and datagrams** between two peers, WireGuard-shaped
below (a cheap mac1 DoS gate, fresh-ephemeral handshake retransmission, an
anti-replay sliding window, endpoint roaming, and the keepalive/liveness
timers) and QUIC-shaped above (one unified frame layer inside every
sealed packet: STREAM fragments, ACK ranges, flow-control credit, an
unreliable DATAGRAM frame, and a CLOSE frame; RFC 9002 loss detection and
NewReno congestion control). All session cryptography flows through `hiss`
(Noise **IK** via the `noise!` macro and its datagram transport); the one
raw primitive is mac1's keyed BLAKE2b from `cryptoxide`.

## 1. Status, scope, and the first wire *(DRAFT 2026/08/13)*

### 1.1 The first wire

Wire version 1 is the **first released slither wire**. **[RATIFIED
2026/08/14]** No released software ever spoke the superseded pre-release
drafts (`SPEC.md`, `SPEC-v2.md`), so there is no on-wire migration and no
version cut — this wire simply claims the first version number. The
version byte is `VERSION = 0x01` in every packet header, and the Noise
prologue is `PROLOGUE = b"slither\x01"` (§5.1): an unknown version or
packet type is a silent drop (§3.1), and a version-confused peer that
somehow passed classification still fails cryptographically, because the
version is bound into the Noise transcript via the prologue. **No version
negotiation exists or is reserved for.** Version 1 means exactly this
specification; if there is ever a version 2, it is a deliberate break
with its own prologue. Negotiation is permanently out of scope for a
mutually-authenticated pair protocol — both ends are configured, not
discovered. (The alternative — a reserved negotiation surface — buys
nothing for configured peers and is a standing parsing liability;
declined.)

**Accepted note — old dev binaries.** The pre-release drafts happened to
use the same version byte and prologue, so a development binary speaking
the old wire is distinguished **structurally, not cryptographically**:
its mac1 is keyed over a different static encoding (the 33-byte
compressed SEC1 form; this wire keys over the 65-byte canonical
uncompressed form — §2.4, §4.4), so every cross-wire handshake packet
dies at the mac1 gate as a silent drop before any DH, and no session can
ever form between the two wires. Accepted, because nothing was ever
released: the exposure is confined to unreleased development builds.

### 1.2 Scope

This specification covers the whole protocol: the crypto suite seam and
`channel!` (§2), the packet grammar (§3), mac1 (§4), the handshake (§5),
the staged accept and initiation routing (§6), the session layer —
counter, replay, roaming, liveness, the epoch ratchet (§7), the unified
frame layer (§8), streams (§9), flow control (§10), datagrams (§11), the ACK
frame and policy (§12), loss recovery (§13), congestion control (§14), CLOSE
and the connection lifecycle (§15), the object model and the sans-io cores
(§16), endpoint-global state (§17), errors and observability (§18), and the
deferral list (§19). Appendix A records the gating hiss dependencies;
Appendix B the test obligations; the final table consolidates every named
constant.

The old Leg 1/Leg 2 split is dead. There is one frame layer inside the seal:
every sealed packet's plaintext is a frame stream, DATAGRAM is just another
frame type, and one packet can coalesce an ACK, a STREAM fragment, a credit
grant, and an unreliable datagram. The old whole-message DATA frame, its
sequence space, its dedup floor, and the 1159-byte message cap are gone —
fragmentation and ordering are the STREAM frame's offset field (§9), and
"reliable message" is API sugar over short unidirectional streams (§9.8).

### 1.3 Ratification discipline and carried rulings

The code matches the spec, never the other way round. All golden wire
vectors freeze **for the first time** at this wire (the prologue, the
msg1 payload, and the canonical-encoding ruling of §4.4 fix the handshake
and mac1 bytes), and then hold under the standing test discipline: any
change that moves a wire byte turns a pinned test red, and such a red
means "this needs a ruling", not "update the expectation" (Appendix B).

The maintainer-flagged rulings carried from the superseded pre-release
drafts were **re-ratified individually** in the 2026/08/14 walkthrough —
one-connection-per-static (§16.1), the staged type names (§6.2), the
intro-queue DoS posture (§6.3), own-bytes-on-consume and the `Superseded`
removal (§6.3), the guard-eviction mitigations (§17.1), the
membership-oracle restatement (§6.5, in reduced form), the `accept()`
re-home model (§6.4), and the two-core sans-io shape (§16.4) — each
carrying its **[RATIFIED 2026/08/14]** marker at its home section. Two
did not: the instant swap-cut is **moot** (its home, §7.6, is deleted by
the ratchet-only ruling), and the no-fallback split-read gate
(Appendix A.1) is **pending** — being worked jointly with hiss — and
keeps its **[MAINTAINER]** flag; the document ratifies as a whole only
after that resolution and the follow-up re-review.

## 2. Crypto suites and `channel!` *(DRAFT 2026/08/13)*

### 2.1 The hiss contract

slither rides hiss 0.3.x: the IK handshake is declared through the `noise!`
macro, and the data path rides the datagram transport
(`DatagramSend`/`DatagramRecv`). slither never touches curve or AEAD
primitives itself (the one exception is §4's mac1). The following hiss facts
are load-bearing for the wire; each row states what hiss fixes and how the
wire respects it.

| hiss fixes | the wire respects it by |
|---|---|
| The send counter is hiss-owned, strictly monotonic from 0, and never caller-chosen; it is returned by each seal | the counter is **transmitted verbatim** in the Data header (§3.4); slither never mints a second packet number (§7.1) |
| The counter is the AEAD nonce; the receiver must know it before it can decrypt | the counter rides in **cleartext** in the header — non-negotiable (§3.4) |
| The receive half is stateless with respect to ordering and replay: it opens any counter, any number of times | slither owns 100 % of replay protection — the RFC 6479 window, consulted strictly **after** the AEAD authenticates (§7.2) |
| The epoch ratchet: a message at `counter` belongs to epoch `counter / epoch_size`; both ends must pass the identical epoch size | `REKEY_EPOCH_MSGS = 65 536` is a protocol constant, not a knob (§7.7) |
| `MAX_EPOCH_JUMP = 2` (hiss-fixed): a counter more than two epochs ahead is refused without deriving any key; committed keys advance only after the AEAD tag verifies (commit-and-cap) | forged far-future counters are bounded upstream of the replay window and can never desynchronise the receiver; epoch death is subsumed by liveness (§7.7) |
| Straggler tolerance is exactly one epoch back | the reordering budget (replay window, 2048) sits far inside one epoch (65 536) (§7.2) |
| `MAX_MESSAGE_LEN = 65 535` bounds any one sealed message (ciphertext incl. tag) | every slither seal is ≤ `MAX_PLAINTEXT` + 16 = 1186 B, far under the cap (§8.6) |
| The counter value `2⁶⁴ − 1` is reserved for the `Rekey()` transform; sealing at it is refused | the usable counter space is `0 ..= 2⁶⁴ − 2`; exhaustion is the terminal `ConnectionLost::NonceExhausted` (§7.9) |
| The two directions have independent counters; a fresh handshake builds a fresh transport counting from 0 | packet-number spaces are **per direction**, and a connection has exactly one session for its life (§7.1, §7.8) |

### 2.2 `channel!` — what varies, what the wire sees

`channel!` (a `macro_rules` macro — no proc-macro) stamps the `hiss::noise!`
IK invocation with a caller-chosen `<Curve, Cipher, Hash>` triple plus the
`Channel`/`Protocol` implementation. The IK token block and the handshake
payload declaration (`[12]` on msg1; msg2 declares no payload — §5.2) are
hardcoded in the macro; **IK is the only pattern**. The reference suite is
**`P256 / ChaChaPoly / Blake2b`**, and its Noise protocol name —
`Noise_IK_P256_ChaChaPoly_BLAKE2b` — is pinned by test.

Wire-visible consequences of the suite are the handshake message sizes
(point encodings) and, in principle, the AEAD tag size; everything above
the seal — the data-packet header, the frame layer, and every constant in
§§8–15 — is suite-independent. **There is no suite identifier on the
wire.** Endpoints are monomorphic per suite: the shell type is
`Endpoint<C: Channel>`, and §16.2's `Endpoint` and §16.4's
`core::Endpoint<I: Identity>` are the same parameterisation viewed from the
shell and the core (`I`'s provider is the suite's DH provider; those
sections elide the parameters). A
mismatched-suite packet dies silently at the length gate or at mac1 — the
same fate as garbage. The version byte does not encode the suite.

### 2.3 Per-suite derived sizes

With `PK` = `Curve::PUBLIC_KEY_SIZE` and `TAG` = the suite AEAD's tag size:

```
MSG1_LEN        = PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)   (e ‖ enc_s ‖ enc_payload)
MSG2_LEN        = PK + TAG                                     (e ‖ the empty payload's tag)
INIT_PACKET_LEN = 6  + MSG1_LEN + 16                           (InitHeader ‖ msg1 ‖ mac1)
RESP_PACKET_LEN = 10 + MSG2_LEN + 16                           (RespHeader ‖ msg2 ‖ mac1)
```

Reference-suite values (P-256: `PK` = 65; ChaCha20-Poly1305: `TAG` = 16 =
`AEAD_TAG_LEN`; `MSG1_PAYLOAD_LEN` = 12 — §5.2, test-pinned):

| Constant | Value |
|---|---|
| `IK_MSG1_LEN` | **174** (= 65 + 81 + 28) |
| `IK_MSG2_LEN` | **81** (= 65 + 16) |
| `INIT_PACKET_LEN` | **196** |
| `RESP_PACKET_LEN` | **107** |
| `AEAD_TAG_LEN` | 16 |

### 2.4 The canonical static encoding

The **canonical encoding of a static public key is the `AsRef<[u8]>` octets
of `Curve::PublicKey`** — for P-256, the 65-byte uncompressed SEC1 storage
form (`0x04 ‖ X ‖ Y`, normalisation enforced by hiss regardless of the
encoding a key was parsed from); for X25519, the raw 32 bytes. One encoding,
three uses: mac1 is keyed over it (§4), the simultaneous-open tie-break
compares it (§6.7), and identity maps and accept policies key on it. Within
a suite all canonical encodings are equal-length, so the tie-break compares
the `as_ref()` octets directly as unsigned octet strings — the only bound
required is `Curve::PublicKey: AsRef<[u8]>`, expressed as a slither-side
`where` clause; `Ord` is **not** required (Appendix A.3). The stated
consequence: P-256 mac1 keying moves from the pre-release drafts' 33-byte
compressed form to the 65-byte uncompressed form — fixed at the first
golden freeze (§4.4), with no shipped bytes to move.

## 3. Packet grammar *(DRAFT 2026/08/13)*

### 3.1 Packet types and version

| Constant | Value | Meaning |
|---|---|---|
| `VERSION` | `0x01` | protocol version byte; unknown ⇒ silent drop |
| `PKT_HANDSHAKE_INIT` | `0x01` | initiator's IK msg1 |
| `PKT_HANDSHAKE_RESP` | `0x02` | responder's IK msg2 |
| `PKT_DATA` | `0x03` | sealed transport datagram |
| `0x04` | reserved | **unused** — the cleartext close packet concept is dead (CLOSE is a frame, §15); never emitted, silently dropped |
| `0x05` | reserved | cookie reply / mac2 (§19) — never emitted, silently dropped |
| `0x06..` | reserved | future — never emitted, silently dropped |

Every packet opens with `type: u8, version: u8`. **All multi-byte header
fields are big-endian.** A datagram shorter than its type's fixed minimum,
longer than `MAX_DATAGRAM`, or bearing an unknown type or version is
silently dropped before any further work. This pre-AEAD gate is the
**only** silent-drop tier for malformed traffic: a packet that fails here
may genuinely be corruption, so nothing is signalled; a packet that passes
the AEAD and then fails structurally is a peer bug or an attack, and gets a
signalled death (§8.2).

### 3.2 HandshakeInit (`0x01`) — 196 bytes (reference suite)

```
type(1) ‖ version(1) ‖ sender_index(4)                       ← InitHeader, 6 B
        ‖ hiss IK msg1(174) ‖ mac1(16)
```

- `sender_index` — the initiator's random nonzero `u32` index (§17.3).
- msg1's encrypted tail carries the 12-byte payload `timestamp(12)`
  (§5.2); nothing of it appears in the header.
- `mac1` — keyed on the **responder's** static (the recipient, §4); its
  preimage is all packet bytes preceding the tag.

### 3.3 HandshakeResp (`0x02`) — 107 bytes (reference suite)

```
type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)   ← RespHeader, 10 B
        ‖ hiss IK msg2(81) ‖ mac1(16)
```

- `sender_index` — the responder's random nonzero `u32` index.
- `receiver_index` — the initiator's index this response answers.
- msg2 carries no payload (§5.2): its encrypted tail is the empty
  payload's AEAD tag alone.
- `mac1` — keyed on the **initiator's** static (the recipient, §4).

### 3.4 Data (`0x03`) — 14-byte header ‖ ciphertext

```
type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)        ← DataHeader, 14 B
        ‖ ciphertext(plaintext + 16)
```

- **The 14 header bytes are the AEAD associated data, verbatim.** The
  counter in the AD is redundant (it is already the nonce), but harmless,
  and "AD = the header, verbatim" is the simplest possible rule.
  Authenticating `receiver_index`, `type`, and `version` is stronger than
  WireGuard (whose data AEAD uses empty AAD) at zero cost: header tampering
  is tag-detectable.
- `receiver_index` — the recipient's session index; routes the packet to a
  session (and thus a key) before decryption (§17.3).
- `counter` — exactly the value the seal returned: the hiss-owned monotonic
  send counter, which is simultaneously the AEAD nonce, the packet number
  (§7.1), and the epoch selector (§7.7). Full 8 bytes, in clear, no
  truncation and no header protection in this version — the WireGuard
  posture; truncated packet numbers and header protection are deferred
  metadata levers (§19). Until Appendix A.2's counter accessor ships, a
  mirror-and-assert interim — the implementation mirrors the expected next
  counter and `debug_assert_eq!`s it against each seal's returned value —
  is conformant; the mirrored value feeds **only** the AD construction and
  is never fed back to hiss.
- **There is no cleartext length field**: the AEAD gives the exact
  plaintext length, and the frame parser runs to the end of it (§8.2).
- An **empty plaintext** (16-byte tag-only ciphertext; a 30-byte datagram)
  is the **keepalive** — it bypasses the frame layer entirely and is the
  only non-frame plaintext (§7.5).

### 3.5 Sizes and caps

| Constant | Value |
|---|---|
| `INIT_HEADER_LEN` | 6 |
| `RESP_HEADER_LEN` | 10 |
| `DATA_HEADER_LEN` | 14 |
| `MAX_DATAGRAM` | 1200 |
| `MAX_PLAINTEXT` | 1170 (= `MAX_DATAGRAM` − 14 − 16) |
| session index | random nonzero `u32`, re-drawn per §17.3 |

Per-packet overhead on the data path is 14 + 16 = 30 B. Oversize receive
(> `MAX_DATAGRAM`) is a silent drop. Oversize *send* has no single rule:
stream data fragments across packets by construction (§9.5), and each
sending surface enforces its own cap at the handle — `DatagramError::TooLarge`
above `MAX_DATAGRAM_PAYLOAD` (§11.4), `MessageError::TooLarge` above
`MESSAGE_RECV_MAX` (§9.8).

## 4. mac1 — the DoS gate *(DRAFT 2026/08/13)*

### 4.1 Construction

```
key  = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_canonical)
mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
```

| Constant | Value |
|---|---|
| `MAC1_LABEL` | `b"slither mac1"` |
| `MAC1_LEN` | 16 |

`recipient_static_canonical` is the recipient's static public key in the
canonical encoding of §2.4 — for the reference suite, the 65-byte
uncompressed SEC1 form. On a HandshakeInit the recipient is the responder;
on a HandshakeResp the recipient is the initiator.

### 4.2 Verification order

mac1 is verified **before any curve or DH work**. A garbage flood, a
wrong-key packet, or a mismatched-suite packet dies at one keyed hash and
never reaches the DH provider. This is the floor of the staged-accept cost
ladder (§6.2).

### 4.3 What mac1 is not

mac1 is **not** a secret authenticator: its key is derived from public
data, so anyone who knows the recipient's static can mint mac1-valid
packets. It is an anti-amplification and cheap-reject gate; the real
authentication is the Noise handshake underneath. The cookie/mac2 second
tier (packet type `0x05`) against floods of *valid-looking* packets remains
reserved (§19).

### 4.4 Suite genericity

**[RATIFIED 2026/08/14]** mac1 is **fixed keyed-BLAKE2b for every suite** —
it does not follow the suite's Hash. mac1 is a keyed hash over public data,
not session cryptography, so it falls under the raw-primitive rule (taken
from `cryptoxide` directly), and WireGuard's fixed-BLAKE2s sets the
precedent; making it follow the suite Hash would demand a keyed-hash mode
from every hiss Hash and buy nothing. Together with the no-suite-byte rule
(§2.2) this forecloses any future multi-suite endpoint on one socket —
mismatched suites die as garbage, which is acceptable for
mutually-configured peers. The keying encoding is the canonical encoding
(§2.4): a generic formulation inheriting `AsRef` silently would have been a
trap under a frozen wire; here it is a deliberate ruling, and the resulting
P-256 change (33-byte compressed → 65-byte uncompressed keying, relative to
the pre-release drafts) is costless — no shipped bytes existed, and the
first golden freeze pins the canonical form (§1.1, §1.3). The
alternative — pinning the compressed form per curve — would preserve the
pre-release drafts' mac1 bytes at the price of a second, per-curve encoding
rule alongside the canonical one.

## 5. Handshake *(DRAFT 2026/08/13)*

### 5.1 Prologue

```
PROLOGUE = b"slither\x01"        (fixed; identical for every handshake)
```

The prologue carries the wire version, binding it into the Noise
transcript: a peer that mis-classifies the version does not merely parse
garbage, it fails the handshake cryptographically (§1.1).

### 5.2 Handshake payloads

```
msg1 payload (12 B, encrypted in msg1's tail):
    ts_secs(8, BE) ‖ ts_nanos(4, BE)
msg2: no payload (msg2's encrypted tail is the empty payload's tag alone)
```

| Constant | Value |
|---|---|
| `TIMESTAMP_LEN` | 12 |
| `MSG1_PAYLOAD_LEN` | 12 |

The payload sits inside msg1's encrypted tail, after `e, es, s, ss`
(authenticated against the initiator's proven static): encrypted,
authenticated, and forgeable only by key-holders — who can only hurt
themselves (§5.4, §17.1). msg2 declares no payload; its tail's tag still
authenticates the full transcript (the cipher is fully keyed after
`e, ee, se`), so a tampered msg2 fails completion exactly as before.

### 5.3 The initiation timestamp

The timestamp is the wall clock (`secs_since_unix_epoch ‖ nanos`), the one
wall-clock read in the protocol (§16.5), forced **strictly greater** than
the previous timestamp this endpoint emitted — endpoint-global, across all
connections and connection generations (§17.2) — so a retransmit is always
admissible even when the coarse clock has not advanced, and a close-and-
reconnect still emits strictly greater.

**The exact confidentiality guarantee (carried).** The msg1 payload has
Noise confidentiality level 2: it is encrypted to the responder's static
key, so it is opaque to any passive observer — no clock-skew
fingerprinting — and authenticated (a tampered ciphertext fails the tail's
AEAD tag, failing the handshake). It is **not** forward-secret against a
later compromise of the responder's static key; acceptable, because the
plaintext is a wall-clock reading, not a secret.

### 5.4 One session per connection — restart is structural

**[RATIFIED 2026/08/14 — the ratchet-only ruling; amended
2026/08/14.]** There is no periodic
DH re-handshake in this protocol (§7.6 is deleted); the §7.7 epoch
ratchet is the **only** rekey. Every completed handshake therefore
establishes a **fresh connection with fresh transport state on both
sides** — sessions and connections are 1:1, and no stream, flow-control,
recovery, or congestion state ever crosses a handshake (§7.8). The old
wire's restart bug — rekey and restart cryptographically
indistinguishable at msg1, producing silent, *confirmed*, undetectable
data loss when a restarted peer's from-zero identifiers were swallowed as
duplicates and acknowledged — is dead **by construction**, not by flag:
there is no state-carrying handshake for a restart to masquerade as, so
no flag bits, no CONTINUATION/CONTINUED machinery, and no
restart-detection surface exist. The handshake payload is the timestamp
alone (§5.2).

**Responder rule.** The local state for the proven static — always
consulted **post-`ss`**, at the point each path proves the static: the
tie-break's routing (§6.6) or the staged chain's admission (§6.4) — is
exactly one of **three** values:
**LIVE** — an established connection exists; **PENDING** — an in-flight
outbound initiation exists and no established connection (the
simultaneous-open state; §16.1 forbids both at once); or **NONE** —
neither.

- **LIVE** — the initiation is a **candidate replacement**: it parks as
  an ordinary
  `Intro`, frozen with its paid mid-state where one was paid (§6.3), and
  surfaces through the staged accept. The live connection keeps running
  untouched; `accept()` of that `Intro` performs the
  `ConnectionLost::Replaced` teardown as the act that installs the
  replacement (§6.4). A withheld or replayed initiation left unaccepted
  costs one parked `Intro` and nothing else. Two tests gate the
  admission: the per-static greatest-timestamp guard (§17.1) rejects
  anything older than or equal to what we have already admitted for that
  static, and the connection's replacement basis (§17.4) rejects anything
  not strictly newer than the initiation that established it — and
  rejects everything where we dialled, because then we hold no
  initiation of that peer's to measure against. Neither test can tell a
  withheld genuine retransmit from a fresh one (§6.4).
- **PENDING** — the simultaneous open: the deterministic tie-break
  decides (§6.6–§6.7); no teardown either way. A static that is PENDING
  only because a `connect()` raced a chain already staged against it
  never reaches the tie-break — §6.4's PENDING branch cancels the
  pending and installs as responder.
- **NONE** — the ordinary fresh accept via the staged path (§6.1).

A restarted peer needs no protocol help: its fresh initiation is simply a
replacement (LIVE row) or a fresh accept (NONE row), and its zombie
counterpart-connection dies at the replacing `accept()` or at liveness
(§7.5). **No routing outcome can merge two transport-state generations,
because no handshake carries state across.**

### 5.5 Initiator behaviour

1. Draw a random nonzero `sender_index` (re-draw rule §17.3) and a fresh
   strictly-greater timestamp; build msg1 over a fresh ephemeral with the
   12-byte payload (§5.2); append mac1; send to the dialled address.
2. Arm a retransmit at `RETRANSMIT_BASE` + uniform jitter ≤
   `RETRANSMIT_JITTER_MAX` (5 s + U[0, 333 ms]). **Every retransmit is a
   completely fresh initiation** — new ephemeral, new random index, new
   strictly-greater timestamp. The interval is fixed, not exponential —
   WireGuard's shipped shape, kept for simplicity.
3. **One completion attempt per retransmit interval.** The pending's
   attempt is *taken* on the first **length-correct, index-matching,
   mac1-valid** msg2; a second msg2 in the same interval is dropped. A
   failed completion (bad crypto)
   spends the attempt — the next scheduled retransmit refreshes it — so a
   guessed-index or mac1-invalid msg2 can never spend anything, and no
   forged-msg2 volume can induce initiations faster than the retransmit
   schedule. (The residual exposure — an on-path, index-observing forger
   beating the genuine msg2 each interval — is documented in §6.9.)
4. **The msg2 source address is deliberately ignored.** Completion
   requires an index match, not an address match; the initiator anchors
   the session at the dialled address, and the peer roams in on its first
   authenticated data packet (§7.3). (The responder has no dialled
   address: it anchors at the msg1 source — a peer-supplied address,
   send-gated by §7.3's anti-amplification budget; §5.6.)
5. On completion: session live — our receiver index = our
   `sender_index`, the peer's = the response's `sender_index`.
6. Give up at `HANDSHAKE_GIVEUP` (90 s): `Connecting` resolves
   `Err(ConnectError::TimedOut)` — the only handshake failure the
   application ever sees. (Every initiation is a `connect()`; there are
   no internal rekey initiations — §5.4.)

### 5.6 Responder shape

The responder's processing is the staged DH-cost ladder — mac1 (0 DH) →
`es` (1 DH, recovers the *claimed* static) → `ss` (1 DH, proves possession
and decrypts the timestamp) → `ee`, `se` (+2 DH, msg2) — driven by the
application through the staged accept, with one internal exception: the
simultaneous-open tie-break (§6.6–§6.7). §6 is its normative home. The
per-static greatest-timestamp guard (strictly greater, else drop — §17.1)
admits at `authenticate()` on the staged path and at the tie-break's
admit step, in both cases post-`ss`, so only key-holders can write guard
entries.

The responder anchors an accepted session at the initiation's msg1 source
address — the one place a peer-supplied address becomes a send target
before any authenticated data has arrived from it. That anchor arms
§7.3's anti-amplification budget: until the address validates by traffic,
output to it is capped at `AMPLIFICATION_FACTOR` × the authenticated
bytes received from it (§7.3).

### 5.7 Timer values and their derivations

| Timer | Value | Derivation |
|---|---|---|
| `RETRANSMIT_BASE` + jitter | 5 s + U[0, 333 ms] | WireGuard Rekey-Timeout + jitter, cross-checked in the paper, the kernel, and wireguard-go; fixed-interval is WireGuard's shipped shape |
| `HANDSHAKE_GIVEUP` | 90 s | WireGuard Rekey-Attempt-Time |
| `KEEPALIVE_TIMEOUT` | 10 s | WireGuard Keepalive-Timeout (§7.5) |
| `DEAD_TIMEOUT` | 15 s | keepalive + one retransmit interval of grace; the only idle killer (§7.5) |
| `PERSISTENT_KEEPALIVE` | 25 s default | WireGuard's convention; a recommended default, per-connection `Option<Duration>` (§7.5) |

There is no DH-rekey trigger at all — neither by message count
(WireGuard's 2⁶⁰) nor by time (WireGuard's Rekey-After-Time): the epoch
ratchet is the only rekey, refreshing keys by count with no handshake
(§5.4, §7.7).

## 6. Staged accept and initiation routing *(DRAFT 2026/08/13)*

The responder's staged DH costs become an application-driven typestate,
and inbound initiations route between that typestate and the endpoint's
one internal path, the simultaneous-open tie-break (§6.6–§6.7).
Requirements: every application-visible admission — fresh accept and
replacement alike — is decided by the application at `accept()` (there
are no rekeys, so nothing bypasses it except the tie-break); the 0-DH
drop of an unwanted `Intro` survives; the per-attacker-packet cost is
bounded and stated (§6.9); the 1/2/4 cumulative DH costs are preserved on
the application-visible path; and stage-0-only parking survives, with the
one bounded carried-mid-state exception (§6.5 step 3). Routing keys on
the one-connection-per-static invariant (§16.1).

### 6.1 The typestate and its DH costs

**[RATIFIED 2026/08/14]** The type names `Intro`, `Claimed`, and `Proven`
are final — each is honest about the security state it represents.

| Stage | Cumulative responder cost | Visible to the application | Automatic (non-policy) rejections |
|---|---|---|---|
| `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed hash, **0 DH** | source address, `sender_index` | short/oversize, unknown type/version, bad mac1 — all silent, before the queue |
| `read_identity()` → `Claimed` | **1 DH** (`es`)† | the **claimed** static | structurally unreadable msg1 (`Malformed`) |
| `authenticate()` → `Proven` | **2 DH** (+ `ss`) | possession proven; the initiation timestamp | tail-tag failure (`HandshakeFailed`); timestamp replay (`Replay` — the guard is not policy) |
| `accept()` → `Connection` | **4 DH** (+ `ee`, `se`; msg2 sent) | an established connection | — |

† A hinted-source entry may arrive with its identity **pre-read** (§6.5
step 3): the 1 DH was charged once, at the eager read, and
`read_identity()` on such an entry returns the cached claimed static at 0
incremental DH. The cumulative table is unchanged either way. The same
holds for every **frozen** mid-state-carrying entry (eager-demoted —
§6.3): its staged accessors return cached results at 0 incremental DH,
and a later same-source initiation parks as a *new* entry that pays its
own ladder.

The table prices `accept()`'s fast path; a **re-homed** `accept()` (§6.4)
adds the admitted candidate's `es` + `ss` on top of the 4 — an
application-driven spend.

Dropping the object at any stage is a **silent reject**: no msg2, nothing
transmitted, the slot freed. The claimed static at `Claimed` is
attacker-choosable (reaching it requires no secret), and the same
discipline applies one stage earlier: `source()` and `sender_index()` are
exposed at 0 DH and are equally attacker-chosen. **Nothing durable may be
keyed on the claimed static, the source address, or `sender_index`** — no
map insertion, no rate-limit bucket, no unbounded logging. Proof of
possession arrives only at `authenticate()`, where a forged claim of a
real static dies at the msg1 tail's AEAD tag.

### 6.2 The staged verbs

```rust
impl Intro {                                                  // 0 DH so far
    pub fn source(&self) -> SocketAddr;
    pub fn sender_index(&self) -> u32;
    pub async fn read_identity(self) -> Result<Claimed, IntroError>;  // 1 DH (0 on pre-read)
}   // drop = silent reject
impl Claimed {                                                // 1 DH; identity CLAIMED
    pub fn claimed_static(&self) -> &PublicKey;               // never `remote_static()`
    pub async fn authenticate(self) -> Result<Proven, AuthError>;     // +1 DH; guard admits here
}   // drop = silent reject
impl Proven {                                                 // 2 DH; possession proven
    pub fn peer_static(&self) -> &PublicKey;
    pub fn timestamp(&self) -> Timestamp;
    pub async fn accept(self) -> Result<Connection, AcceptError>;     // +2 DH (re-home adds 2)
}   // drop = silent reject
```

Each stage's `async` is a driver round-trip; the DH costs land on the
driver task (§16.3). The staged error types are §18.1's; the verbs they
serve are summarised there.

### 6.3 The stage-0 queue

| Constant | Value | Notes |
|---|---|---|
| `INTRO_QUEUE_CAP` | **1024** slots, endpoint-wide | configurable in `Config` |
| `INTRO_MAX_PER_SOURCE` | **4** chains per source IP (per /64 for IPv6) — the sum of unconsumed stage-0 entries and consumed chains | configurable |
| `INTRO_TTL` | **15 s** after the entry's last refresh | ≈ 3 retransmit intervals; the flood hold-cost bound |

The accept queue parks **stage-0 state only** — the raw 196-byte msg1 plus
the source address (≈ 220 B per entry; worst case ≈ 225 KB at the default
cap) — with one bounded exception: an eager-demoted entry carries its
already-paid mid-state (§6.5 step 3). The bound is post-mac1 — a higher bar
than WireGuard's pre-mac1 4096-slot ring.

**[RATIFIED 2026/08/14]** The flood posture below — evict-oldest overflow,
the per-source cap, and the 15 s TTL — is a ruled posture: each is cheap,
none is load-bearing for correctness, and together they change the flood
exposure from a holdable reservation to a per-packet race (honesty clause
below).

- **Dedup key: the full source `SocketAddr` alone; replace-with-newest**,
  and replacement refreshes the deadline. **[RATIFIED 2026/08/14]** (This
  supersedes any `(addr, sender_index)` key: every retransmit is a
  completely fresh initiation with a new random index — §5.5 — so an
  index-bearing key would never match across retransmits, defeating dedup.
  The key is stage-0-only; a peer whose address changes mid-attempt
  recovers by convergence, and post-establishment mobility is roaming's
  job — §7.3.)
  Distinct initiators behind one NAT present distinct ports, hence distinct
  keys; a same-4-tuple collision is a rebind of the same flow, for which
  newest-wins is correct. Replacement initiations from established peers
  park here like any other (§5.4): dedup's replace-with-newest and the
  evict-oldest guarantee give a retransmitting genuine peer the same
  per-packet race as any fresh initiator.
- **Per-source cap**: at most `INTRO_MAX_PER_SOURCE` chains per source IP,
  counting the **sum of unconsumed stage-0 entries and consumed chains**.
  An arrival that would exceed the cap replaces that IP's oldest
  **unconsumed** entry — eviction operates on the unconsumed tier only;
  consumed chains are DH-paid, and non-evictable for it: **app-held**
  after `read_identity()`, or **endpoint-frozen** for a
  mid-state-carrying entry (freeze-on-carry, below), which the
  application may never have held — and if the source's whole
  allowance is held by consumed chains, the arrival is dropped. Frozen
  carried entries remain counted under this cap and expire at
  `INTRO_TTL` like any entry (honesty clause below).
  `read_identity()` is net-zero for its source's count (−1 unconsumed,
  +1 consumed).
- **Overflow: evict-oldest.** A full queue evicts the oldest unconsumed
  entry (by park time) in favour of the arrival: a genuine initiation
  always obtains a slot — except where its own source's allowance is
  wholly held by consumed chains (the per-source drop above; honesty
  clause below) — and an attacker must win a per-packet race against
  the genuine peer's ~5 s retransmit rather than hold a reservation.
  Consumed chains are never evicted by overflow.
- **Expiry** is silent eviction at `INTRO_TTL`; staged verbs on an expired
  attempt return `IntroError::Expired` (`AuthError::Expired` at that
  stage). A consumed chain's mid-state expires 15 s after the initiation
  that fed it. `accept()` alone is exempt — the re-home rule (§6.4) means a
  chain's age never fails an `accept()`, only the absence of any parked
  initiation does.

**Consumption and supersession — own-bytes-on-consume.** An entry is
**unconsumed** until `read_identity()` — and only if it carries no
mid-state (below). While unconsumed, a newer
initiation from the same source transparently replaces its bytes and
refreshes its TTL: same `IntroId`, newest bytes, accessors reflect the
newest bytes at call time, no second surfacing. The moment
`read_identity()` runs, the chain owns its bytes and its `IntroId`: the
source's stage-0 slot is freed, a subsequent initiation parks as a new
entry, and a consumed `Claimed`/`Proven` chain can **never** be superseded
by any later packet — an unauthenticated mac1-valid packet cannot clobber
DH-paid work.

**Mid-state-carrying entries are consumed on arrival — freeze-on-carry.**
Any stage-0 entry that carries a paid mid-state — eager-demoted (§6.5
step 3) — is **consumed from the moment it
parks**: its bytes and its `IntroId` are frozen, a later same-source
initiation parks as a *new* entry subject to the per-source cap, and the
staged accessors never straddle two initiations. An unauthenticated
mac1-valid packet can therefore never byte-replace an entry whose cached
mid-state claims an identity, so `read_identity()` and
`authenticate()` can never report an identity for bytes that identity
never sent. (The minimal-state alternative — replacement replaces such an
entry wholesale, clearing the identity-already-read tag so
`read_identity()` re-pays its 1 DH — is declined: freeze-on-carry closes
an identity-straddling injection surface, and Appendix B's
DH-cost pins assume it.)

**[RATIFIED 2026/08/14]** **`Superseded` appears in no error enum**:
under own-bytes-on-consume no verb can observe supersession, so the variant
is unreachable and deleted rather than retained.

**Mid-states are live key material.** A mid-state — post-`read_identity`,
or carried by an eager-demoted entry — lives inside the endpoint core keyed
by `IntroId`. It holds the endpoint's static provider and the `es`-derived
keys (≈ 0.5–1 KB); bounded by the queue cap and the TTL (§17.5).

**Honesty clause — the queue-occupancy exposure.** mac1's key is public
data (§4.3), so minting mac1-valid initiations costs an attacker only
bandwidth, and no spoofing capability is needed to occupy slots (distinct
source ports are distinct sources). The caps buy: one source is bounded to
4 chains total (consumed and unconsumed together); filling the queue needs
≥ 256 distinct sources; evict-oldest makes full occupancy a per-packet
race, not a reservation; and the TTL prices sustained full occupancy at
≈ 68 packets/second (1024 / 15 s). The denial, while sustained, is
endpoint-wide **for inbound accepts — replacements of established
connections included**, since under the ratchet-only design replacement
initiations park in this queue like any other (§5.4); established
connections themselves keep running regardless — they hold no queue
slot. One occupancy surface is
sharper than the rest and is stated precisely: freeze-on-carry parks
**endpoint-frozen, non-evictable** consumed slots that no application
ever held, so an attacker that spoofs a hint-set source (the dialled
address of an in-flight outbound connect — §6.5) and claims unknown
statics mints up to
`INTRO_MAX_PER_SOURCE` (4) frozen stage-0 slots for that source IP
(/64 for IPv6), at 1 DH each, each held for up to `INTRO_TTL` — and
while the /64's whole allowance is frozen, a genuine **new** peer
sharing it is denied a stage-0 slot until expiry (the evict-oldest
guarantee does not reach the consumed tier). The exposure is bounded
by the per-source-both-tiers cap and the TTL, is open only while an
outbound connect is in flight to that address, touches no established
connection, and requires spoofing that specific dialled address;
genuine peers on any other
source are unaffected. This is not
WireGuard-equivalent exposure (WireGuard's ring is a transient work queue
backed by the under-load cookie gate); until the deferred cookies/mac2
round (§19), the per-source cap is the only occupant-shaped defence.

### 6.4 `accept()` re-homes to the freshest parked initiation

**[RATIFIED 2026/08/14, amended 2026/08/14]** The staged chain proves
*identity*; `accept()`
commits to the *peer*, never to the specific initiation inspected.
Fresh-ephemeral
retransmission re-mints the initiator's index every ~5 s and a msg2
answering a superseded initiation is ignored (§5.5), so a `Proven` chain's
own initiation goes stale in about one retransmit interval — far inside a
human-in-the-loop accept decision. The rule:

- **Fast path.** If the chain's own initiation is still the freshest
  parked for its source, `accept()` proceeds on it at the table's price.
- **Re-home.** Otherwise the endpoint replays the parked candidates for
  the chain's source in order of **park time, newest first** (park time,
  not msg1 timestamp — the timestamp is encrypted and unknown until
  `es` + `ss`). For each candidate it runs `es` + `ss`; a candidate is
  admitted only if **all three** hold: the read yields the **same** proven
  static, the tail tag verifies, and the timestamp guard admits its
  strictly-greater timestamp. msg2 is written for the
  admitted candidate. A failing candidate is discarded and the next-newest
  tried, until admission or exhaustion. The per-source cap bounds the walk
  at four `es` + `ss` pairs.
- **The §16.1 guard — a proven-LIVE admission replaces only against a
  newer basis.** At admission, fast path or re-home: if a LIVE connection
  exists for the proven static, the `accept()` **is** the replacement
  (§5.4) — but **only if** that connection's replacement basis (§17.4) is
  `Some(t)` and the candidate's timestamp is **strictly greater than
  `t`**. Then the install fires `ConnectionLost::Replaced` on the old
  connection and the replacement takes its place: the teardown executes
  exactly at the act that commits it, never earlier, so a withheld or
  replayed replacement initiation left unaccepted costs one parked
  `Intro` and nothing else (§15.4). Otherwise — basis `None` (we dialled
  this connection, so we hold no initiation timestamp of the peer's to
  measure a replacement against) or a candidate not strictly newer than
  `t` — `accept()` returns `AcceptError::Stale` and the live connection
  is untouched. There is no `AcceptError::AlreadyConnected`: a
  basis-passing proven-LIVE accept is a replacement, so the variant is
  unreachable and deleted (§18.1; `ConnectError::AlreadyConnected`
  remains for `connect()`, §16.1).
- **What basis and guard bar, stated honestly.** The timestamp guard
  (§17.1) rejects any candidate whose timestamp is ≤ the greatest this
  endpoint has admitted for that static; the basis rule above
  additionally rejects any candidate not strictly newer than the
  initiation that established the live connection. Neither
  distinguishes a **withheld genuine retransmit** from a fresh one: every
  retransmit is a completely fresh initiation carrying a strictly greater
  timestamp (§5.5), so an initiation captured off the wire and injected
  later is indistinguishable from the peer reconnecting. Such a candidate
  destroys nothing until the application accepts it; applications that
  auto-accept replacements accept that residual. A candidate presented
  against a `None` basis is refused outright — which is what keeps a
  passively captured msg1 from destroying a connection we dialled, the
  case in which we hold no timestamp of that peer's at all.
- **PENDING at admission.** If an in-flight outbound initiation exists
  for the proven static, the `accept()` **cancels** it — the pending and
  its index are dropped and its `Connecting` resolves
  `Err(ConnectError::AlreadyConnected)` — and the accept proceeds as an
  ordinary fresh install with this endpoint as responder. The tie-break
  does **not** run here: the tie-break's job is to make two crossing
  *initiations* converge on one session (§6.7), and this path has already
  committed to ours as responder. This is the branch that closes the
  `read_identity()` → `connect()` → `accept()` ordering — the static was
  NONE when the chain was staged and became PENDING before it was
  accepted, so §6.5's interception, which fires when a *parked* `Intro`'s
  claim turns out to be a pending outbound remote, cannot fire on a chain
  the application already holds. Without this branch §16.1's
  one-connection-per-static invariant would be broken by two ordinary
  API calls in the wrong order.
- **Stale.** If no initiation for that static is currently parked, or the
  admitted candidate fails the basis rule above, `accept()` returns
  `AcceptError::Stale`; the application SHOULD
  re-accept when the peer's next initiation surfaces as a new `Intro`.

### 6.5 The routing rule

Inbound `HandshakeInit` processing in the endpoint core:

1. **Stage 0 (always):** length gate, classify, mac1 verify. Failure is a
   silent drop. Cost: one keyed hash.
2. **Hint check (no DH):** the *hint set* is the dialled addresses of all
   in-flight outbound initiations — **nothing else** (established
   connections' addresses are not hints; a LIVE static's initiation takes
   the ordinary staged path below). If `src` ∉ hint set → park at stage 0
   (§6.3) and surface an `Intro`.
3. **Eager path (`src` ∈ hint set):** the endpoint immediately runs the
   split intro read (1 DH, `es` — Appendix A.1) and inspects the claimed
   static:
   - claimed ∈ *pending outbound remotes* → the **internal tie-break**
     (§6.6). The packet never touches the accept queue and the
     application never sees it.
   - claimed ∉ pending outbound remotes → the raw packet is **demoted**
     to the stage-0 queue under the §6.3 rules, **carrying its paid
     mid-state**, tagged identity-already-read: a peer sharing a source
     with a dialled address still surfaces as an `Intro`, and
     `read_identity()` on it returns the cached claim at 0 incremental DH.
4. **`read_identity()` interception (the backstop):** when a parked
   `Intro`'s claimed static turns out to be a pending outbound remote,
   the endpoint performs the same internal tie-break and
   `read_identity()` returns `Err(IntroError::Internal)` — the
   application learns no identity and makes no decision.

The probed set is pending outbound remotes **only**: a PENDING static's
initiation **must** enter the internal path — that is where the
tie-break runs (§6.6, §6.7) — while LIVE- and NONE-state claims are
parked or demoted to the staged path, where a proven-LIVE `accept()` is
the replacement (§5.4, §6.4). The hint's one job is to catch the
crossing msg1 of a simultaneous open cheaply; its false negative (the
peer dialling out from an address other than the one we dialled) parks
the msg1 as an ordinary `Intro` and self-heals at the `read_identity()`
interception.

The false negative does **not** self-heal through retransmission — a peer
dialling from a rewritten source port sends every retransmit from that
same port (§5.5) — so the `read_identity()` interception is the only
backstop, and it is application-driven: two peers that dial
simultaneously, are both NAT-port-rewritten, and never probe inbound
intros will both fail at `HANDSHAKE_GIVEUP` and must retry. The failure
is self-limiting in practice, because the two retries are scheduled by
the applications and so de-synchronise. **Applications that dial SHOULD
also drain `accept()`.**

**The membership-timing oracle, restated.** **[RATIFIED 2026/08/14, in
reduced form.]** The probed set is the **pending-outbound-remote set
only** — an in-flight-dial oracle, not a configuration oracle, and no
established connection is probed — exposed both as timing (the
tie-break's extra `ss`) and as an explicit API discriminator
(`IntroError::Internal` versus a `Claimed` at one DH less). Statics are
public data, and the window is the lifetime of an in-flight `connect()`;
the exposure is accepted.

### 6.6 The internal tie-break completion

The endpoint's one internal admission path — reached only when the
claimed static is a pending outbound remote (§6.5 step 3, or the step-4
interception). It runs on the already-paid mid-state, in this order:

1. **Tag** — `complete()` (`ss`, +1 DH): a forged claim of a pending
   static dies here at the msg1 tail's AEAD tag. A failure leaves the
   in-flight outbound pending untouched — nothing unauthenticated can
   reach the tie-break.
2. **Guard** — the per-static greatest-timestamp guard (§17.1): strictly
   greater, or the initiation dies here (the shape of a replayed genuine
   msg1, once the guard holds an entry for that static — §6.7's honesty
   clause states what happens before it does). LIVE and NONE statics are
   unreachable in this path: §6.5
   routes them to the staged accept, where a proven-LIVE `accept()` is
   the replacement (§5.4, §6.4). The converse does **not** hold —
   PENDING is not exclusive to this path: a chain staged while its static
   was NONE and accepted after a `connect()` made that static PENDING
   reaches §6.4's PENDING branch instead, which cancels the pending and
   installs as responder without any tie-break.
3. **Tie-break** — §6.7 decides the race. Our static smaller ⇒ we are the
   winner: the authenticated inbound is silently dropped, its timestamp
   **recorded** (§6.7), and our own outbound completes normally. The
   peer's static smaller ⇒ proceed as responder.
4. **Admit** (the loser side) — only on passing all of the above is the
   strictly-greater timestamp **recorded** as a full admission, our own
   pending cancelled (§6.7), a
   responder index minted (§17.3), and msg2 written (`ee`, `se`, +2 DH).
   The admission completes the connection as an `Install`
   (§16.4), resolving its `Connecting` exactly as a msg2 completion
   would (§6.7), and sets that connection's replacement basis to
   `Some(t)` for the admitted timestamp `t` (§17.4) — on this branch we
   are the responder.

A failure at steps 1–2 is a silent drop with a trace (`slither::policy`),
the pending untouched, nothing recorded. A step-3 winner-side drop is
likewise silent and traced, but it is **not** record-free: the winner
records the loser's timestamp (§6.7). It writes no basis — the winner is
the connection initiator, so its basis stays `None` (§17.4).

### 6.7 Simultaneous open — the deterministic tie-break

When an inbound initiation's claimed static matches a peer to whom we hold
an in-flight outbound pending, the race is resolved by a tie-break
evaluated identically on both sides, never by completion order (each
side's inbound handshake always completes first locally; "whichever
completes first" would install different key sets on the two sides and go
mutually dark for 15 s): **the peer with the lexicographically smaller
static public key is the winning initiator.** The comparison is over the
**canonical static encoding** (§2.4) as unsigned octet strings — always
equal-length within a suite, and identical to the mac1-keying and
identity-map octets, so the comparison is over the `as_ref()` bytes
directly and needs no `Ord` bound (Appendix A.3).

**The tie-break runs only on an authenticated inbound — after `ss`
succeeds.** A match detected at `es` selects the tie-break path but
decides nothing: an `ss` failure is a silent drop with the pending
untouched — **a forgery cannot cancel a pending**. Only when `ss` and
the guard pass (§6.6's order) is it applied:

- **Our static is smaller** ⇒ we are the winning initiator: the
  authenticated inbound is silently dropped (mid-state discarded, no
  msg2) and our own outbound completes normally — but its timestamp
  **is** recorded in the guard (§17.1). The packet authenticated
  post-`ss`, so the write is a key-holder write like every other
  (§17.1's invariant is preserved); recording it is what denies a later
  replay of that same initiation the vacuous guard pass it would
  otherwise enjoy against an endpoint that has admitted nothing from this
  peer. The winning connection's replacement basis stays `None`: we are
  its initiator, and the recorded timestamp belongs to the guard, not to
  the connection (§17.4).
- **The peer's static is smaller** ⇒ we cancel our pending now — post-`ss`,
  on the authenticated inbound (the pending and its index dropped; no
  give-up, no error) — and the tie-break admits and writes msg2 as
  responder (§6.6 step 4).

**What a replay can still do here, stated honestly.** A forgery cannot
cancel a pending. A **replay** of a genuine msg1 can, until the guard
holds an entry for that static: replay and first transmission are
indistinguishable when we have never admitted an initiation from that
peer, so a captured msg1 that arrives while we hold a pending to its
sender passes `ss`, passes the guard vacuously, and — if our static is
the larger — cancels our pending and installs a session the replayer
cannot complete. The cost is bounded: the admission records the
timestamp, so each captured initiation is single-use, the winner-side
record above closes the same hole from the other direction, and the
resulting session carries nothing and dies at `DEAD_TIMEOUT` (§7.5). It
is strictly weaker than a capture-capable attacker's baseline ability to
drop our handshake outright.

Both sides compare the same ordered pair and reach complementary
conclusions, so exactly one session — the winner's msg1, the loser's
msg2 — is constructed, and both sides hold it. A connecting
(never-established) connection that loses the tie-break is completed by
the tie-break's `Install` (§16.4), which resolves its
`Connecting` exactly as a msg2 completion would — connect resolution is
**edge-triggered exactly once** per connection lifecycle. Equal statics
cannot occur (`connect()` to our own static is out of scope under §16.1).
Queued sends are unaffected by either outcome: they live in the connection
core's stream state and pump on whichever session installs (§16.9).

**Stream-parity consequence.** The tie-break winner is the **connection
initiator** for the life of the connection: stream-ID parity (§9.1) is
fixed by this outcome at establishment and never changes thereafter.

### 6.8 Restart handling, summarised

Restart needs no machinery of its own (§5.4): a restarted *peer*
reconnects, its initiation parks as an ordinary `Intro` at our LIVE
static, the live (now-zombie) connection keeps running untouched, and the
`Replaced` teardown fires only at the replacing `accept()` (§6.4); a
restarted *self* holds no state at all — the peer's zombie connection to
us receives nothing it can open, dies at liveness (§7.5), and
the application reconnects, surfacing at our end as an ordinary fresh
`Intro` (or being replaced at its end if we reconnect first).

**Which of the two shapes a restart takes depends on the basis** (§17.4).
Where the zombie is a connection we **accepted**, its basis is `Some(t)`
and the restart replaces at the first `accept()`: the restarted peer
reads a fresh wall clock, so its initiation is strictly newer than the
one that established the connection (§5.3), barring a backwards clock
jump. Where the zombie is a connection we **dialled**, its basis is
`None` and no initiation can replace it — `accept()` returns
`AcceptError::Stale` (§6.4) and the `Intro` is refused, because we hold
no initiation of that peer's against which a captured msg1 could be
distinguished from a genuine reconnect. The restart still resolves with
no machinery, delayed by at most `DEAD_TIMEOUT`: the zombie receives
nothing it can open, so it dies at liveness; the static drops to NONE;
and the peer's next initiation — its retransmit train re-mints one every
~5 s (§5.5), and its application retries beyond that — takes the
ordinary fresh accept. That bounded delay on connections we dialled is
the price of refusing an off-path replay the same power, and it is
ruled acceptable.

### 6.9 DoS accounting

Per attacker packet; the mac1 verification (one keyed hash) is already
paid by us in every row. Stimulus 196 B (HandshakeInit); msg2 107 B.

| Packet class | Our cost beyond the mac1 hash |
|---|---|
| mac1-invalid garbage / wrong key / wrong suite | 0 |
| mac1-valid, src ∉ hint set, `Intro` left or dropped unprobed | **0 DH**, one bounded queue slot (≈ 220 B) |
| mac1-valid, src ∉ hint set, application probes identity then drops | 1 DH — an application-chosen spend |
| mac1-valid, src spoofed into the hint set (a dialled address of an in-flight connect), claimed static unknown | **1 DH** — the `es` paid once at the eager read and carried through the demotion (§6.5 step 3) |
| forged claim of a pending-outbound static | 2 DH (`es` + `ss`), dies at the tail tag with the pending untouched (§6.7); a forged claim of any other static is an application-chosen spend (the staged rows above) |
| replayed genuine msg1 reaching the tie-break path | **2 DH** (`es` + `ss`), dying at the timestamp guard once an entry exists for that static — before then it survives the guard and can cancel our pending (§6.7's honesty clause), at the same 2 DH; on the staged path the same replay costs only what the application chooses to probe — the sustained 2-DH primitive is confined to the in-flight-dial window |
| genuine replacement initiation from a key-holding peer | staged like any other: DH only as the application probes; + 2 DH (`ee` + `se`) only per `accept()` — replacement admission is basis-gated and application-gated (§6.4, §17.4) |
| authenticated peer sends a violating frame stream (credit breach, limit breach, stream-state or final-size violation, or a post-AEAD structural failure — §8.2) | no DH: one parse + one CLOSE seal + the 5 s linger (§15.2) |
| authenticated peer floods packets during our linger | ≤ 1 CLOSE reply per second, to authenticated window-fresh inbound only (§15.2) |
| mac1-valid, index-matching forged msg2 (initiator side) | **2 DH** (`ee` + `se`), dies at msg2's tag; spends that interval's completion attempt (§5.5) |

**The ceiling, explicitly:** the maximum cost of any single attacker
packet is **2 DH** — on the responder side and, via the forged-msg2 row,
on the initiator side too — and only 1 of those is reachable without
holding a hint-set address, observing our 32-bit index (on-path in
practice), or the application choosing to spend.

**Three rate-honesty notes** complete the table. The eager `es` of §6.5 is
**rate-ungated** per spoofed hint-set source: the §6.3 caps bound *state*,
never *work* — mac1-valid initiations from a spoofed hint-set address (a
dialled address of an in-flight connect; the exposure window is that
dial's lifetime) cost 1 DH each, every time, until the deferred cookie
tier (§19) prices
them. Each such unknown-static demotion also parks **frozen** (consumed
on arrival, §6.3), occupying a non-evictable consumed slot, so the
per-source cap — not evict-oldest — is what bounds this occupant
surface until the cookie tier: ≤ `INTRO_MAX_PER_SOURCE` (4) frozen
stage-0 slots per /64 at 1 DH each, `INTRO_TTL`-expiring, denying a
genuine new peer of that /64 a slot while held (§6.3's honesty
clause). A re-home walk's candidates are **attacker-fillable**:
mac1-valid rubbish parked for a chain's source can cost a legitimate
`accept()` up to four wasted `es` + `ss` pairs before it returns `Stale`
— application-driven and cap-bounded, but attacker-provoked. And
**the protocol paces replacement not at all**: a key-holding peer
can present a fresh, strictly-greater initiation as fast as it can send,
and an auto-accepting application replaces its own connection on each,
paying a full teardown plus 4 DH per accept and §17.5's state churn with
it. Applications that auto-accept replacements SHOULD rate-limit
replacing accepts per static; the protocol's only guarantee is the one
§16.1 gives — every replacement keys on the *proven* static, so the
damage is confined to the connection to that static and a key-holder
harms nobody but itself.

One initiator-side exposure completes the accounting: a mac1-valid,
index-matching but cryptographically invalid msg2 spends the initiator's
completion attempt for that interval (§5.5). This is an on-path
(index-observing) capability — off-path requires guessing a 32-bit
index — and its full mitigation is foreclosed by hiss's consuming state
machines; the attempt refreshes at the next scheduled retransmit, so a
sustained on-path forger denies the handshake for at most the 90 s
give-up and can never induce initiations faster than the schedule.

A re-homed `accept()` pays the inspected chain's `es` + `ss` plus up to
four candidates' `es` + `ss` plus the admitted one's `ee` + `se` —
app-driven, post-authentication, bounded by the
per-source cap; not attacker amplification.

**No amplification.** The accept path replies 107 B (msg2) to a 196 B
stimulus — ratio < 1 — and no path in this specification emits bytes **to
an address that has not authenticated**: no cookie replies, no error
packets, the `0x04` packet type is never emitted, CLOSE exists only
inside the seal and its linger replies only to authenticated,
window-fresh inbound (§15.2), every staged rejection is local and silent,
and a peer-supplied anchor or roam target is send-capped by the
anti-amplification budget until it validates by traffic (§7.3).

## 7. Session layer *(DRAFT 2026/08/13)*

### 7.1 The counter is the packet number

The hiss `DatagramSend` counter **is** the packet number: monotonic,
hiss-owned, never caller-chosen, simultaneously the AEAD nonce and the
epoch selector. ACKs reference it directly (§12); there is no second
identifier, so a retransmission is never ambiguous (frames are
retransmitted, never packets — §13.5) and there is no Karn's problem. It
rides in cleartext because the receiver decrypts with it (§3.4).

Packet-number spaces are **per direction, one per connection for its
whole life**: a connection has exactly one session (§5.4), so the counter
runs from 0 at establishment and never restarts — there is no
re-handshake, no second counter space, and no make-before-break demux.
Every seal — payload, control, keepalive — burns the next counter.

### 7.2 The anti-replay window

| Constant | Value |
|---|---|
| `REPLAY_WINDOW` | **2048** bits (an RFC 6479 sliding bitmap, `[u64; 32]`, 256 B per connection) |

- The window tracks a greatest authenticated counter plus the 2048-bit
  bitmap. The replay check is strictly **post-AEAD**: check-then-mark only
  after `decrypt_at` authenticates. A duplicate, or a counter more than
  2048 behind the greatest, is dropped after decryption **without
  delivery**. The window's greatest advances only on authenticated
  counters; hiss's `MAX_EPOCH_JUMP` and commit-and-cap bound forged-counter
  cost upstream of it (§2.1).
- **Liveness and roaming are driven only by packets that are both
  authenticated and window-marked** (fresh). No replayed packet ever moves
  the endpoint or refreshes liveness.
- Sizing: 2048 sits above boringtun's 1024 and below the kernel's 8192;
  it is ~20 ms of reordering memory at 1 Gbps line rate and ~200 ms at
  100 Mbps, comfortably inside one ratchet epoch (65 536), with margin for
  any future truncated-packet-number scheme.

**[RATIFIED 2026/08/14]** The ACK record stays **fused** to this window (§12.2):
the window is the single received-packet record — reuse, don't duplicate.
The failure mode under congestion control is bounded and benign: ACK
fidelity is capped at 2048 counters, so only an ACK-loss burst longer than
the window's time-width causes delivered-but-unreported packets, which
surface as spurious retransmissions (streams dedup by offset, §9.5) and at
worst one spurious congestion event absorbed by the recovery-period rule
(§14.3). A reviewer could reasonably demand the decoupled QUIC-style range
tracker now; it is instead the flagged upgrade when sustained >100 Mbps
per connection matters (nothing on the wire changes for it — the ACK
encoding is already range-based), and the Appendix B ACK-loss-burst
simulation quantifies the exposure before ratification hardens (§19).

### 7.3 Roaming

An **authenticated, fresh, window-marked** Data packet whose source
differs from the session's current endpoint moves the endpoint to the new
source. Nothing unauthenticated, and no replayed packet, ever moves it.
**Handshake packets never roam a live session** — an accepted initiation
*anchors* a new or replacement session at its msg1 source, which is not
roaming. Observability is the `remote_address()` accessor plus the
`slither::roam` trace target (§18.2). Roaming additionally resets the congestion
controller with the pre-roam flight fenced off (§14.6) — new wiring
relative to all prior slither: a new path
carries no continuity evidence for the old window.

**The anti-amplification budget.** **[RATIFIED 2026/08/14]** Roaming moves the
endpoint on one authenticated packet, and an accepted initiation anchors
at its msg1 source (§5.6) — in both cases a peer-supplied address becomes
a send target with no return-routability proof, while §13.4 and §14.5
exempt whole output classes (PTO probes, pure ACKs, CLOSE, keepalives)
from the congestion window. Unchecked, that is a reflector. The rule:
whenever a session's endpoint address changes (a roam) or is first
anchored from a msg1 source, the address is **unvalidated** and a
send-side budget arms — total bytes sent to the address MUST NOT exceed
`AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it and
authenticated** on this session, both counters resetting at each such
address change. Authenticated means the packet's AEAD tag verified
(§7.2's authenticated class; the anchoring initiation qualifies, its
handshake tail tags having verified at admission) — merely-received
bytes, unauthenticated or undecryptable datagrams claiming the
address, MUST NOT replenish the budget. The budget
binds **all** output to the address, **explicitly including the §14.5 and
§13.4 congestion-window exemptions** — those exemptions are scoped to
cwnd, never to this budget. The 3× ratio is **never lifted**: it is the
amplification factor QUIC accepts (RFC 9000 §8.2/§9.3), it forces an
attacker to pay a third of any flood it reflects — removing the
reflection incentive at zero protocol machinery — and a genuine peer
clears it within about one round trip, because its own authenticated
traffic funds the budget continuously, while a path that sends nothing
dies by liveness inside 15 s (no deadlock). The judgment calls needing
the ruling: the never-lifted ratio versus an
N-authenticated-packets-over-1-RTT validation unlock (more state, the
same reflection property), and this wire-free budget versus explicit
PATH_CHALLENGE/PATH_RESPONSE validation — two new frame types and a
second reset seam, the one reviewed alternative that would move the wire;
recorded in §19 as the QUIC-faithful-migration lever.

| Constant | Value |
|---|---|
| `AMPLIFICATION_FACTOR` | 3 (× authenticated bytes received, per unvalidated address) |

### 7.4 The liveness model — `seal` versus `seal_quiet`

The liveness clock is driven by **application intent only**. Two seal
paths, identical on the wire (same sealed-Data packet, same counter
increment):

- **`seal`** marks `last_send`: packets carrying at least one
  first-transmission STREAM frame or DATAGRAM frame (fresh application
  sends), and the keepalive (§7.5).
- **`seal_quiet`** does not touch `last_send` — the **quiet set**: pure
  ACKs, PTO probes, retransmissions, the credit frames (MAX_DATA,
  MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI), RESET_STREAM, and CLOSE.

Ack-eliciting and liveness-marking are independent axes: credit frames are
ack-eliciting (they need loss recovery — §8.7) yet liveness-neutral.

**The liveness anchor is the receive clock, armed by intent.** The
connection is dead when `now − last_authenticated_recv > DEAD_TIMEOUT`
**and** at least one marking send has occurred since that last
authenticated receive. Equivalently: the death deadline arms on the
*first* marking send after a receive, is **not** re-armed by subsequent
marking sends, and is reset by every authenticated, window-fresh receive
(§7.2). A sender writing into a black hole therefore dies 15 s after its
last authenticated receive no matter how often it writes — the send clock
never defers death, it only enables it. An unending PTO train must not
defer death (probes are non-marking), and an immediate ACK must not
suppress the keepalive dance. A packet coalescing fresh application frames
with control frames marks the clock (it carries fresh intent).

### 7.5 Keepalive and liveness timers

| Constant | Value |
|---|---|
| `KEEPALIVE_TIMEOUT` | 10 s |
| `DEAD_TIMEOUT` | 15 s |
| `PERSISTENT_KEEPALIVE` | 25 s (recommended default; per-connection `Option<Duration>`, off by default) |

- **The keepalive is the empty plaintext** (§3.4) — the cheapest possible
  liveness beacon, bypassing the frame layer, sealed via `seal`. Its
  classification, explicit: the keepalive is a **marking** send. Passive
  rule: a side that has received since it last sent, and has not sent for
  `KEEPALIVE_TIMEOUT`, sends a keepalive.
- **Persistent keepalive** is a per-connection opt-in interval for NAT
  holding. Its trigger is WireGuard's: it fires when no marking send has
  occurred for the configured interval, and re-arms from every marking
  send. `set_persistent_keepalive` **rejects an interval shorter than
  `DEAD_TIMEOUT`** at the handle. **[RATIFIED 2026/08/14]** The floor
  stands as ruled: it constrains a config knob so a marking beacon can
  never outpace the death clock (the alternative — admitting short
  intervals but excluding persistent keepalives from the marking set — is
  declined for splitting the keepalive into two classes). The flag's
  other half — the idle-rekey rule, under which the keepalive consulted
  `REKEY_AGE` — is moot: there is no DH rekey to consult (§5.4, the
  ratchet-only ruling). The anchor correction itself
  (§7.4) is a forced fix, not a flagged call.
- **Liveness** (`DEAD_TIMEOUT`) keys on the receive clock (§7.4): a
  connection that has marked a send since its last authenticated receive
  and then receives nothing authenticated for 15 s is dead
  (`ConnectionLost::TimedOut`). It is **the only idle killer** — an idle
  session sustained by the keepalive dance keeps receiving, so it lives
  indefinitely.
- Keepalives are admitted to the replay window (they appear
  opportunistically in ACK ranges; `ack_delay = 0` when the window's
  largest was not frame-seen — §12.3) but never reach recovery, never
  count as ack-eliciting, and never enter the congestion window (§14.5).
- **PING** is not a keepalive: it is the ack-eliciting PTO probe (§13.4),
  sealed via `seal_quiet` — liveness-neutral. Two signals, two masters.

### 7.6 [deleted 2026/08/14 — the ratchet-only ruling]

The periodic DH re-handshake this section specified — `REKEY_AGE`
(120 s), `REJECT_AGE` (180 s), the `NeedsRekey` signal, the silent swap,
the instant swap-cut, and make-before-break demux — is **deleted**: the
§7.7 epoch ratchet is the only rekey, and a new handshake from an
already-live static is a connection replacement via `accept()` (§5.4,
§6.4). The swap-cut's maintainer flag is moot with the section. The
section number is retained to keep §7's numbering stable.

### 7.7 The epoch ratchet

| Constant | Value |
|---|---|
| `REKEY_EPOCH_MSGS` | 65 536 (2¹⁶) messages per epoch |
| `MAX_EPOCH_JUMP` | 2 (hiss-fixed) |

Transport keys ratchet forward on a counter-derived schedule
(`into_datagram_with_epoch`), retiring ageing key material by rotation. A
message sealed at `counter` belongs to epoch `counter / REKEY_EPOCH_MSGS`;
each direction ratchets independently; the counter is **never reset** by
the ratchet; `2⁶⁴ − 1` is reserved for the `Rekey()` transform. Epoch `e`'s
key is Noise §11.3 `Rekey()` applied `e` times:
`Rekey(k) = ENCRYPT(k, 2⁶⁴ − 1, empty, zeros[32])[0..32]`. The
ChaCha20-Poly1305 vector, pinned by test:
`REKEY(0³²) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`.
The receiver retains the current and immediately preceding epoch keys
(straggler tolerance: one epoch back); anything older is refused, its key
ratcheted away. The ratchet is **forward rotation only, not healing**:
post-compromise healing within a connection **does not exist** — an
exfiltrated session key decrypts its direction until the application
reconnects, and healing is application reconnect policy (the TLS 1.3
KeyUpdate model; WireGuard's contrary choice — a periodic DH
re-handshake — is noted and overridden by the ratchet-only ruling, §5.4).

**Epoch death is subsumed by liveness.** A peer more than two epochs ahead
is permanently unopenable (refused without key derivation — a generic
decryption failure at the hiss surface). No dedicated detection or
recovery path exists or may be added: a legitimate peer can only get there
across ≥ 65 536 messages of silence, which `DEAD_TIMEOUT` excludes by
orders of magnitude; if the condition somehow arises, no inbound packet
opens, `last_recv` stops advancing, and liveness tears the session down.
**Implementations must not chase epochs.**

### 7.8 One session per connection — nothing survives a handshake

A connection has **exactly one session** for its whole life (§5.4). No
transport state ever crosses a handshake: a completed handshake installs
a **new connection** — fresh cipher states, replay window, counters,
streams, flow-control ledgers, recovery and congestion state, liveness
clocks, and indices — and the connection it replaces (if any) dies whole
at the replacing `accept()` (`ConnectionLost::Replaced`, §6.4) or at
liveness. There is no survival matrix, no re-queue rule, and no rekey
seam: session-scoped and connection-scoped are the same scope. Data
un-ACKed at a replacement is lost with the old connection and is the
application's to re-send on the new one — exactly the reconnect
semantics of every fresh `connect()`.

### 7.9 Nonce exhaustion

Sealing at counter `2⁶⁴ − 1` is refused by hiss (§2.1). A seal failure is
never silent and can never strand frames (plan-seal-commit, §16.7): it
moves the connection to `ConnectionLost::NonceExhausted` — **connection
death, with no rekey escape** (there is no DH re-handshake to mint a
fresh counter space — §5.4). The bound is unreachable in practice: the
usable space is `0 ..= 2⁶⁴ − 2` ≈ 1.8 × 10¹⁹ seals per direction — at a
sustained 10⁷ packets/s that is ≈ 1.8 × 10¹² seconds, over 58 000
years — and the epoch ratchet bounds per-key AEAD volume long before
counts matter (65 536 messages × ≤ 1 186 B ≈ 78 MB per epoch key, §7.7).
The wire's varints additionally cap ACK-referenced counters at 2⁶² − 1
(§8.1) — ≈ 4.6 × 10¹⁸ packets, still over 14 000 years at 10⁷ packets/s;
both bounds are stated for completeness, neither is reachable at any
physical send rate.

## 8. The frame layer *(DRAFT 2026/08/13)*

### 8.1 Varint encoding

All frame-body integer fields — the frame type byte included — are QUIC
variable-length integers, byte-identical to RFC 9000 §16. The top two bits
of the first byte select the total length; the remaining bits, big-endian
across the encoding, are the value:

| Prefix | Length | Usable bits | Maximum value |
|---|---|---|---|
| `00` | 1 byte | 6 | 63 |
| `01` | 2 bytes | 14 | 16 383 |
| `10` | 4 bytes | 30 | 1 073 741 823 |
| `11` | 8 bytes | 62 | 2⁶² − 1 (4 611 686 018 427 387 903) |

A sender emits the minimal encoding; a receiver accepts any length (a
non-minimal encoding is valid, as in QUIC). The cleartext packet header is
**not** varint — fixed widths there (u32 index, u64 counter) keep
classification and AD construction trivial. Stated consequence: varints
cap at 2⁶² − 1, so ACK `largest` (§12.1) and stream offsets and final
sizes (§9.5) cap there too. A connection would need > 4.6 × 10¹⁸ packets
to reach the ACK bound — over 14 000 years at 10⁷ packets/s (§7.9) —
unreachable, but the bound is explicit.

### 8.2 The frame stream: parse-then-apply

A sealed Data packet's plaintext is a concatenation of frames, parsed to
the end of the plaintext (the AEAD gives the exact length; there is no
packet-level length prefix). An empty plaintext is the keepalive and never
reaches this layer (§7.5).

**Parse the whole plaintext first, then apply.** Two failure classes,
strictly distinguished:

- **Structural failure** — an unknown frame type, a truncated frame, a
  varint overrunning the plaintext, a length field overrunning the
  plaintext, a non-final extends-to-end frame (§8.4), or any per-frame
  structural error case below — is a **signalled death**. **[RATIFIED 2026/08/14]**
  Nothing from the packet is applied (no ACK scheduling, no state change
  beyond the already-performed replay mark), one trace fires on
  `slither::frames`, and the connection emits CLOSE with
  `PROTOCOL_VIOLATION` (the existing `0x01`, §15.3) and enters the
  closing state (§15.2), surfacing
  `ConnectionLost::ProtocolViolation { code }` (§18.1). The reasoning is
  population, not tidiness: after the AEAD tag verifies, corruption is
  excluded (2⁻¹²⁸) and version skew is excluded by design (one version,
  no negotiation — §1.1), so a structurally invalid frame stream is a
  peer bug or a deliberate violation — the same population the semantic
  class below already closes on. The silent-drop alternative leaves a
  buggy peer retransmitting its malformed frame for ever, both liveness
  clocks fresh (the packets *are* received and window-marked), with no
  error and no operator signal — an unbounded livelock. The silent drop
  survives **only** pre-AEAD, at §3.1's length/type/version gate, where
  corruption is genuinely possible. This is a behaviour change with no
  wire change (the code already exists in the registry); the flagged call
  is confirming the population.
- **Semantic violation** — a structurally valid frame whose application
  would break protocol state (a flow-control breach §10.5, a stream-limit
  breach §10.5, a stream-state error, a final-size violation §9.5) — is a
  protocol violation by an authenticated peer: the connection emits CLOSE
  with the matching error code and enters the closing state (§15.2).

### 8.3 The frame table

QUIC's type numbers are reused verbatim where the concept is shared; the
gaps are harmless (the type is a varint).

| Type | Frame | Fields (all varints) | Ack-eliciting | Retransmission | Home |
|---|---|---|---|---|---|
| `0x00` | PADDING | — | no | never | §8.4 |
| `0x01` | PING | — | yes | never | §13.4 |
| `0x02` | ACK | largest, ack_delay, range_count, first_range, (gap, range)* | no | never | §12 |
| `0x04` | RESET_STREAM | stream_id, error_code, final_size | yes | regenerate | §9.6 |
| `0x05` | (reserved: STOP_SENDING) | — | — | — | §19 |
| `0x08`–`0x0f` | STREAM | stream_id, [offset], [length], data; OFF = 0x04, LEN = 0x02, FIN = 0x01 | yes | ranges | §9.5 |
| `0x10` | MAX_DATA | max | yes | regenerate | §10.3 |
| `0x11` | MAX_STREAM_DATA | stream_id, max | yes | regenerate | §10.3 |
| `0x12` | MAX_STREAMS_BIDI | max (cumulative) | yes | regenerate | §10.4 |
| `0x13` | MAX_STREAMS_UNI | max (cumulative) | yes | regenerate | §10.4 |
| `0x1c` | CLOSE | error_code, reason_len, reason | no | linger rule (§15.2) | §15 |
| `0x30`/`0x31` | DATAGRAM | [length (0x31 only)], data | yes | never | §11 |

`0x05` is *reserved*, not implemented: like any unknown type, receiving it
is a structural failure — CLOSE with `PROTOCOL_VIOLATION` (§8.2). The
retransmission classes are §8.7's.

### 8.4 Frame layouts and error cases

**PADDING (`0x00`)** — a single `0x00` byte, no fields; any number may
appear anywhere. Not ack-eliciting, never retransmitted, no error cases.

**PING (`0x01`)** — the type byte alone. Ack-eliciting; never
retransmitted (a lost PING is superseded by the next probe). No error
cases.

**ACK (`0x02`)**

```
type(0x02) ‖ largest(varint) ‖ ack_delay(varint, µs)
           ‖ range_count(varint) ‖ first_range(varint)
           ‖ range_count × [ gap(varint) ‖ range(varint) ]
```

Semantics and policy in §12. Structural errors (§8.2's structural class):
`range_count` > `MAX_ACK_RANGES` (64); any range descending below counter
zero. Semantic no-op (frame ignored whole, traced): `largest` above the
highest counter this session has sealed (§12.5).

**RESET_STREAM (`0x04`)**

```
type(0x04) ‖ stream_id(varint) ‖ error_code(varint) ‖ final_size(varint)
```

Abrupt termination of the sender's stream (§9.6). Ack-eliciting; on loss,
regenerated. Semantic violations: a `stream_id` naming a stream the
sender of the frame could not send on (their receive-only half) ⇒
`STREAM_STATE_ERROR` — with exactly one exception, the message-mode
overflow reset of §9.8, in which the *receiver* of a uni stream emits
RESET_STREAM as its abandonment signal (§9.6); a `final_size` below the
receiver's
highest-received offset, or conflicting with an already-pinned final size
⇒ `FINAL_SIZE_ERROR`; a `final_size` that would push stream- or
connection-level consumption above the advertised limit ⇒
`FLOW_CONTROL_ERROR`, checked **before** the §9.6/§10.3 credit true-up,
with checked or saturating `u64` arithmetic mandated (an unchecked sum
wraps for large `final_size` values and silently re-opens the window). A
RESET_STREAM naming an index at or below the space's closed-stream
watermark and not currently open is a no-op — ACKed, never re-opened
(§9.2).

**STREAM (`0x08`–`0x0f`)**

```
type(0x08 | OFF(0x04) | LEN(0x02) | FIN(0x01))
     ‖ stream_id(varint)
     ‖ [ offset(varint)   if OFF ]
     ‖ [ length(varint)   if LEN ]
     ‖ data(length bytes, or to the end of the plaintext if ¬LEN)
```

| Constant | Value |
|---|---|
| `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` | 0x04 / 0x02 / 0x01 (bits of the type byte) |

OFF absent ⇒ offset 0. LEN absent ⇒ the data extends to the end of the
plaintext, and the frame must be the packet's final frame. FIN marks the
data's end offset as the stream's final size (an empty FIN-only frame is
valid). Semantics in §9.5. Structural errors: `length` overrunning the
plaintext; a ¬LEN frame that is not final; `offset + length` exceeding
2⁶² − 1. Semantic violations: data beyond stream or connection credit ⇒
`FLOW_CONTROL_ERROR`; a `stream_id` the peer could not send on ⇒
`STREAM_STATE_ERROR`; opening a stream beyond the cumulative limit ⇒
`STREAM_LIMIT_ERROR`; data beyond, or a FIN conflicting with, a pinned
final size ⇒ `FINAL_SIZE_ERROR`. A frame naming an index at or below the
space's closed-stream watermark and not currently open is a no-op —
ACKed, never re-opened (§9.2). All offset arithmetic (`offset + length`,
final-size and credit comparisons) is checked or saturating.

**MAX_DATA (`0x10`)** / **MAX_STREAM_DATA (`0x11`)**

```
type(0x10) ‖ max(varint)
type(0x11) ‖ stream_id(varint) ‖ max(varint)
```

Absolute-offset credit grants (§10). Monotone-max on receipt: a value not
above the current limit is a valid no-op (duplicates and reordering are
idempotent). Ack-eliciting; on loss, regenerated with the freshest value,
never byte-retransmitted. Semantic violations: MAX_STREAM_DATA for a
stream the *receiver of the frame* cannot send on ⇒ `STREAM_STATE_ERROR`;
likewise — QUIC's rule — MAX_STREAM_DATA for a stream in a space the
frame's receiver opens that the receiver has not yet opened (credit
frames never open streams; §9.2's implicit opening is for STREAM and
RESET_STREAM only). Credit
for a fully-closed stream (at or below the watermark, §9.2) is a valid
no-op.

**MAX_STREAMS_BIDI (`0x12`)** / **MAX_STREAMS_UNI (`0x13`)**

```
type(0x12|0x13) ‖ max(varint, cumulative stream count)
```

Cumulative-count credit for the corresponding space (§10.4). Monotone-max
on receipt. Ack-eliciting; regenerated. Structural error: `max` > 2⁶⁰
(unrepresentable as a stream index) — §8.2's structural class.

**CLOSE (`0x1c`)**

```
type(0x1c) ‖ error_code(varint) ‖ reason_len(varint) ‖ reason(reason_len B)
```

| Constant | Value |
|---|---|
| `CLOSE_REASON_MAX` | 256 B |

One frame type — no transport/application split and no offending-frame-type
field. `reason` SHOULD be UTF-8 but is carried as bytes. Not
ack-eliciting; never loss-retransmitted — the linger's reply rule is its
reliability (§15.2). Structural error: `reason_len` > 256 (§8.2).
`close()` truncates its `reason` to `CLOSE_REASON_MAX` at the handle
(§16.2) — an implementation must not be able to *produce* the over-length
case it must kill on receipt.

**DATAGRAM (`0x30`/`0x31`)**

```
type(0x30) ‖ data(to the end of the plaintext)          — must be final
type(0x31) ‖ length(varint) ‖ data(length B)
```

Unreliable payload (§11). Ack-eliciting; **never** retransmitted.
Structural errors: `length` overrunning the plaintext; a `0x30` frame
that is not the packet's final frame. A receiver-side oversize case is
unrepresentable (§11.4).

### 8.5 Coalescing and packing order

Frames-to-packets is many-to-many: one packet carries many frames, and
one stream's bytes span many packets. Within a packet the sender packs in
this order: the ACK first (if owed), then control frames (credit grants,
RESET_STREAM, CLOSE), then STREAM and DATAGRAM fill, then PING last if a
probe still owes ack-eliciting content. At most one extends-to-end frame
(¬LEN STREAM, or `0x30` DATAGRAM) per packet, in final position. Within
the STREAM fill, streams with pending data are served **round-robin** —
one quantum per stream per fill pass, the quantum size
implementation-defined — which is what makes the no-head-of-line-blocking
contract real under contention (§9.8).

### 8.6 The per-seal bound

hiss caps any one sealed message at `MAX_MESSAGE_LEN` = 65 535 B of
ciphertext. Every slither seal is at most `MAX_PLAINTEXT` + 16 = 1186 B —
far inside the cap; the frame layer never approaches it, and no future
batching may exceed it.

### 8.7 Retransmission classes and ack-eliciting

A packet is **ack-eliciting** iff it contains at least one ack-eliciting
frame (§8.3's column). Only ack-eliciting packets enter the sent-packet
map (§13.5); pure-ACK packets, CLOSE packets, and keepalives are never
tracked and never occupy the congestion window (§14.5).

Loss recovery retransmits **frames, never packets** (§13.5). Three
classes:

- **ranges** (STREAM): the lost packet's stream ranges return to the
  pending set and are re-framed on fresh counters — split, merged, or
  coalesced with new data freely; only still-un-ACKed sub-ranges are
  resent.
- **regenerate** (MAX_DATA, MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI,
  RESET_STREAM): the lost frame's *identity* re-queues, and the
  retransmission carries the **freshest current value** — never the stale
  bytes. (For RESET_STREAM the values are fixed at reset time; it re-emits
  until acknowledged or the stream state is discarded — with one
  carve-out: the receiver-emitted overflow reset of §9.6/§9.8 retires
  its receive half at the moment of emission, so its identity is
  retained at the connection level and re-emitted until acknowledged;
  the discard termination never applies to it.)
- **never** (PADDING, PING, ACK, DATAGRAM, CLOSE): loss is absorbed by the
  next ACK, the next probe, the unreliability contract, or the linger
  reply rule respectively.

## 9. Streams *(DRAFT 2026/08/13)*

### 9.1 Stream identifiers

A stream ID is a varint. Its two low bits tag the stream; the remaining
60 bits are `index`, a per-space monotonically allocated counter from 0:

| Bit | Meaning |
|---|---|
| `0x01` | opener: 0 = the connection initiator, 1 = the acceptor |
| `0x02` | direction: 0 = bidirectional, 1 = unidirectional |

This yields four independent ID spaces (initiator/acceptor ×
bidi/uni), QUIC's encoding verbatim. The opener bit is mandatory anyway —
both sides open streams unprompted, and parity is the only handshake-free
collision avoidance — and the direction bit is what makes the message
primitive cheap (§9.8): a uni stream lets the receiver allocate no
send-half state and expect no reverse FIN. **[RATIFIED 2026/08/14]** The four-space
choice trades slither-minimalism for QUIC congruence (a one- or two-space
collapse would save nothing real — the two bits are already paid for in
the varint — but a reviewer may prefer the smaller conceptual surface);
see also §10.2's constants flag (the two halves of one ruling).

**Role stability.** The opener bit refers to the roles of the
connection's **establishment**: the connection initiator is the dialler,
or under simultaneous open the tie-break winner (§6.7). Parity is fixed
at establishment and never changes for the connection's life.

### 9.2 Implicit opening

There is no OPEN frame. A frame referencing stream `N` of a space opens
`N` and every lower-numbered not-yet-open stream of that space, subject to
the cumulative limit (§10.4) — opening past it is `STREAM_LIMIT_ERROR`.
The first STREAM or RESET_STREAM frame is the open.

**The closed-stream watermark.** Each of the four spaces keeps, alongside
its open set, the **highest fully-closed stream index** (§9.7). A STREAM
or RESET_STREAM frame naming an index **at or below the watermark and not
currently open** is a **no-op — processed as acknowledged, never
re-opened**; implicit opening applies only to indices *above* the
watermark. This tombstone is what makes exactly-once delivery real: a
receive half frees at read-to-final (§9.7) and a sugar stream frees the
instant its message surfaces (§9.8), both *before* the sender can know
(only our ACK tells it), so a single lost ACK makes the peer's routine
PTO retransmission re-name the freed stream — and without the watermark
that retransmission would re-open it, restart the reassembler, re-pin the
final size, and surface the same message twice (or fire a phantom
`StreamOpened` for a finished stream). The watermark is monotone, lives
for the connection's life, and costs one index
per space. (An index at or below the watermark that is not open is
necessarily a *closed* stream: implicit opening opened everything at or
below the watermark when the watermark stream was first named.)

### 9.3 The send half

Conceptual states (RFC 9000 §3.1's shape):

```
Ready ──write──▶ Send ──STREAM+FIN sent──▶ DataSent ──all ACKed──▶ DataRecvd (terminal)
   │                │                          │
   └────────────────┴──────reset()────────────▶ ResetSent ──RESET ACKed──▶ ResetRecvd (terminal)
```

At the terminals the send half's state is freed (§9.7). The six-state
diagram is exposition, not an implementation mandate: an implementation
collapses it (quinn-proto's shape: `Ready` / `DataSent { finish_acked }` /
`ResetSent`, with the terminals represented by removal).

### 9.4 The receive half

```
Recv ──STREAM+FIN──▶ SizeKnown ──all bytes──▶ DataRecvd ──app read all──▶ DataRead (terminal)
   │                     │
   └──RESET_STREAM───────┴──▶ ResetRecvd ──app read reset──▶ ResetRead (terminal)
```

The receive half buffers arriving ranges and delivers the **contiguous
prefix** to the application as it becomes available; a FIN pins the final
size; the terminals free the state. The same collapse note applies
(`Recv { size: Option<u64> }` / `ResetRecvd { size, error_code }`).

### 9.5 STREAM frame semantics

Each STREAM frame is a labelled byte range `(stream_id, offset, data)` of
a per-stream logical byte sequence — not a self-contained message. The
offset field alone reconstructs order, decoupled from packet arrival
order and packet numbers; fragmentation is not a special case (a large
write is consecutive ranges across as many packets as needed), and there
is no message-size ceiling beyond flow control. Rules:

- Ranges arrive in any order and may **overlap** (retransmission
  re-framing, §8.7): a receiver delivers each byte exactly once; a byte
  received twice with differing values is undefined behaviour of the
  sender (an honest sender never produces it) and the receiver may keep
  either.
- **FIN pins the final size** as the frame's end offset
  (`offset + data length`). Receiving data beyond a pinned final size,
  a FIN pinning a size below already-received data, or two pins that
  disagree ⇒ `FINAL_SIZE_ERROR` (CLOSE, §8.2).
- Retransmitted stream bytes consume no new flow-control credit (§10.7);
  data beyond advertised credit ⇒ `FLOW_CONTROL_ERROR`.
- An empty STREAM frame with FIN is a valid end-of-stream marker; an
  empty frame without FIN and without data is valid and a no-op
  (tolerated, never emitted).
- Reassembly memory is bounded twice over: by advertised credit (the span
  a receiver must cover, §10.6) and by the reassembly-fragment mandate of
  §10.6 — per-stream reassembly state MUST be O(advertised credit) and
  MUST NOT scale with the number of received frames.

### 9.6 RESET_STREAM semantics

`reset(error_code)` abandons a send half abruptly: pending and in-flight
data for the stream stop being retransmitted, and RESET_STREAM
`{ stream_id, error_code, final_size }` is emitted (regenerated until
acknowledged), where `final_size` is the number of bytes the stream would
have carried (the end offset of the highest byte sent, or 0 if none),
**truing up the receiver's connection-level flow-control accounting**:
the receiver counts the full `final_size` against `MAX_DATA` consumption
exactly as if the bytes had arrived (§10.1), so both ends agree on
consumed credit even though the tail never arrives. The receive half
surfaces `ReadError::Reset(error_code)` (§18.1), discards its reassembly
buffer, and closes when the application observes the reset. A RESET_STREAM
for an already-FIN-complete receive half is a valid no-op if the final
sizes agree, `FINAL_SIZE_ERROR` otherwise.

The true-up runs only **after** the §8.4 `FLOW_CONTROL_ERROR` check that
`final_size` does not exceed the advertised limits, so it releases
exactly the credit the asserted bytes had already consumed and can never
manufacture more; and when the receive half is retired, the same
`final_size` counts as **consumed** for connection-level credit-advance
(§10.3).

**The receiver-emitted reset (the §8.4 exception).** In exactly one case
the *receiver* of a uni stream emits RESET_STREAM — the message-mode
overflow of §9.8. Its `final_size` field carries the receiver's highest
received offset and is informational: the stream's sender, on receiving
it, stops (re)transmitting the stream, frees its send half (un-ACKed
ranges dropped; the freed half counts toward full closure, §9.7), and
surfaces `WriteError::Reset(error_code)` to a blocked or subsequent
writer. No flow-control true-up runs in this direction — the frame
releases the *sender's* obligation, not the receiver's credit.
**Its delivery is reliable independent of the retired half**: emitting
it retires the receive half at once (§9.8), but the reset's frame
identity `{ stream_id, error_code = 0, final_size }` is retained in
the connection's regenerate set and re-emitted on loss **until
acknowledged** — §8.7's "stream state is discarded" termination does
not apply to this frame (the retained identity is a few words of
connection state, not stream state). Without this retention a single
lost reset re-strands the sender for good: its PTO retransmissions
are no-op'd and ACKed below the closed-stream watermark (§9.2), so
liveness never fires, while the send half stays wedged at the stream
window — the exact stall the reset exists to cure.

### 9.7 Lifecycle and garbage collection

Stream state is freed eagerly:

- a **send half** frees when every byte up to the final size, FIN
  included, is acknowledged (`DataRecvd`), or when its RESET_STREAM is
  acknowledged (`ResetRecvd`);
- a **receive half** frees when the application has read to the final
  size (`DataRead`), or has observed the reset (`ResetRead`), or — for an
  abandoned handle — when the final size is reached with no reader
  (§16.2);
- a stream is **fully closed** when its halves (one for uni, two for
  bidi) are freed; full closure is what earns the peer a MAX_STREAMS
  credit (§10.4).

The allocator never reuses a stream ID; churn is bounded by the free-list
pattern (state lives only for open streams). Freeing is what advances the
closed-stream watermark (§9.2): a fully-closed stream leaves no per-stream
state behind, only the per-space tombstone that keeps late
retransmissions naming it inert. Retiring a receive half also trues up
connection-level credit for its unread bytes (§10.3).

### 9.8 Messages — sugar over auto-managed uni streams

**[RATIFIED 2026/08/14]** The reliable-unordered message primitive is **API sugar
over unidirectional streams — there is no DATA frame type and no second
reliability engine.** Two reliability paths (whole-message retransmit
alongside range retransmit) were the single largest avoidable surface in
the old design; one-stream-per-message preserves the
no-head-of-line-blocking contract (messages on distinct streams never
stall each other), and exactly-once surfacing rests on offset
reassembly **plus the closed-stream watermark** (§9.2 — a freed message
stream cannot be re-opened by a late retransmission) — the old seq-dedup
machinery is gone. The costs accepted:
per-message overhead is a STREAM header (≈ 3–6 B versus the old 11-byte
DATA header — a wash or better), stream-state churn per message (bounded
by §9.7's GC), and messages bounded by the initial stream window. The
alternative — a distinct reliable-message frame — preserves the tiny-RPC
micro-optimum but doubles the retransmission surface forever.

- `Connection::send_message(bytes)`: allocates the next outbound uni
  stream, writes the whole payload, sets FIN, and garbage-collects the
  stream when the FIN'd range is fully acknowledged. No stream handle
  surfaces. Payloads above `MESSAGE_RECV_MAX` are rejected at the handle
  (`MessageError::TooLarge`) — a larger sugar send could stall forever
  against a sugar-consuming receiver, which never extends credit.
- `Connection::recv_message()`: treats each incoming uni stream as one
  message, surfacing the payload only when reassembly is complete (FIN
  and all bytes), then frees the stream.
- `accept_uni()`/`open_uni()` remain available for incremental streams.
  The two receive verbs draw from the same incoming-uni supply: a stream
  claimed by `accept_uni()` leaves message consideration;
  `recv_message()` surfaces the oldest fully-reassembled unclaimed uni
  stream. Which mode consumes a given stream is the receiving
  application's choice, **invisible on the wire**.

| Constant | Value |
|---|---|
| `MESSAGE_RECV_MAX` | = `INITIAL_MAX_STREAM_DATA` (262 144 B) |

The bound is structural: the receiver never extends per-stream credit for
a sugar-consumed stream, so a message is bounded by the initial stream
window. Larger transfers use real streams.

**The overflow policy.** **[RATIFIED 2026/08/14]** A uni stream that consumes its
full initial window without pinning a final size can never complete as a
message (`MESSAGE_RECV_MAX` = the initial window, and message-consumed
streams never earn more credit); left alone it stalls the sender at the
window for ever while its buffered bytes hold connection credit on both
ends (§10.3). The rule: while a `recv_message()` claim is pending and the
oldest unclaimed uni stream reaches that state, the receiver **resets**
it — the receive half retires (its bytes count as consumed at the
connection level, §10.3) and the receiver emits RESET_STREAM with error
code 0, the one receiver-emitted reset (§9.6, §8.4), retained and
regenerated until acknowledged despite the retired half (§9.6, §8.7) —
so the sender's stream frees instead of wedging, even when the reset
itself is lost. Streams claimed by `accept_uni()` are
untouched: real streams extend credit normally. The alternative — keep
extending stream credit past the message bound and surface the stream as
a stream — avoids the receiver-reset carve-out at the price of handing
the application a mode it never asked for; the reset is the recommended
default, and the choice needs the ruling.

### 9.9 STOP_SENDING

Reserved (`0x05`), deferred (§19). Until it ships there is no wire signal
for "stop transmitting this stream to me"; an uninterested receiver drops
its handle and discards arrivals (§16.2), and the sender runs to FIN or
resets.

## 10. Flow control *(DRAFT 2026/08/13)*

### 10.1 The model

Two levels, both receiver-driven, both expressed as **absolute byte
offsets** (a limit says "you may send up to offset X", never "X more
bytes"):

- **Stream level**: each stream's data is bounded by the peer's
  advertised per-stream limit (initially `INITIAL_MAX_STREAM_DATA`,
  raised by MAX_STREAM_DATA).
- **Connection level**: the **sum over all streams** of the highest
  received offset (the final size, once pinned) — where a reset stream
  contributes its trued-up `final_size`
  (§9.6) — is bounded by the peer's connection limit (initially
  `INITIAL_MAX_DATA`, raised by MAX_DATA). A sender respects both limits;
  whichever is tighter binds.

Limits advance monotonically: a received credit frame applies as
monotone-max, so duplicates and reordering are naturally idempotent
(§8.4).

### 10.2 Initial values — protocol constants, no negotiation

**[RATIFIED 2026/08/14]** slither has no negotiation surface at all, so initial
windows are protocol constants, identical in both directions and all
stream spaces; later credit is receiver policy. The values ship
**ratified-but-revisitable**, gated on the Appendix B window-constants
throughput validation (with §9.1's four-space choice, the two halves of
one ruling):

| Constant | Value |
|---|---|
| `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) |
| `INITIAL_MAX_STREAMS_BIDI` | 32 (cumulative) |
| `INITIAL_MAX_STREAMS_UNI` | 128 (cumulative — higher for message traffic, §9.8) |
| `STREAMS_CREDIT_BATCH` | 8 |

### 10.3 Advancing credit

The re-grant rule, concrete (RFC 9000 §4.2's shape): with `WINDOW` the
level's window (`INITIAL_MAX_STREAM_DATA` for a stream,
`INITIAL_MAX_DATA` for the connection), the **prospective limit** is
`bytes_read + WINDOW`, and the receiver emits MAX_STREAM_DATA or MAX_DATA
when `prospective_limit − last_advertised ≥ WINDOW/2` — that is, when the
read offset has advanced at least half a window since the last
advertisement. Consumption, not arrival, drives credit: an unread
buffer earns nothing. Credit frames are ack-eliciting (loss recovery
regenerates them with the freshest value, §8.7) yet liveness-neutral
(§7.4). Sugar-consumed streams never earn stream-level credit (§9.8);
their reads still earn connection-level credit.

**Retirement advances connection credit.** When a receive half is retired
for any reason — read to its final size, reset observed, handle abandoned
(§16.2), surfaced as a message (§9.8), or final size reached with no
reader (§9.7) — **all of its bytes up to its final size count as consumed
for connection-level credit-advance**, exactly as if the application had
read them; stream-level credit is simply never re-granted for a retired
stream (there is no stream to grant to). Without this rule, MAX_DATA is
an absolute limit advanced only by reads, and cumulative discarded or
reset bytes march the connection into a permanent send stall after
`INITIAL_MAX_DATA` with no error and no timer — reachable in honest
operation by any application that cancels streams. The true-up applies
only **after** the §8.4 `FLOW_CONTROL_ERROR` bound check, with checked or
saturating `u64` arithmetic (§9.6): it releases exactly the credit the
bytes had consumed and can never be driven above the advertised limit —
a peer cannot manufacture credit, only waste its own window. The
true-up is **absolute, not additive**: it advances the stream's
contribution to the connection-level consumed count **to** its
`final_size` — a monotone bring-to-final, idempotent with bytes
already counted by reads (§10.1's per-stream absolute sum) — and
never adds `final_size` on top of them; a read-then-retire sequence
therefore counts each byte exactly once, and a receiver can never
over-advance its own MAX_DATA past its buffer commitment.

### 10.4 Stream limits — cumulative credit

**[RATIFIED 2026/08/14]** MAX_STREAMS frames exist, diverging from a fixed-cap
design, because messages-as-streams (§9.8) makes stream churn the common
case and the limit is **cumulative-count** (QUIC's model): a fixed
cumulative cap would kill the connection after N messages, and a
"concurrent" cap requires both ends to agree on close timing — exactly
the ambiguity the credit model exists to avoid. The cost is two frame
types and a replenishment rule.

- The limit counts **streams ever opened** in a space; opening stream
  index `i` requires cumulative limit > `i`.
- The receiver grants +1 as it **fully closes a peer-opened stream** of
  the space (§9.7) — closing streams we opened must not inflate the
  peer's allowance (RFC 9000 §4.6's scope) — batching
  advertisements: emit MAX_STREAMS when ≥ `STREAMS_CREDIT_BATCH` (8)
  grants are unadvertised, **or** when the peer's remaining allowance
  drops to ≤ 8. Receipt of MAX_STREAMS surfaces
  `ConnEvent::StreamsAvailable { dir }` to wake blocked openers (§16.4).
- Opening beyond the limit ⇒ `STREAM_LIMIT_ERROR` ⇒ CLOSE (§8.2).
- `STREAMS_BLOCKED` stays deferred with the rest of the BLOCKED family
  (§19).

### 10.5 Violations

A peer exceeding advertised credit — stream or connection level — is a
protocol violation: CLOSE with `FLOW_CONTROL_ERROR`. A peer opening
beyond a stream limit: CLOSE with `STREAM_LIMIT_ERROR`. There is no
tolerance band; the limits are exact (§8.2's semantic class).

### 10.6 Credit is the buffer commitment

**[RATIFIED 2026/08/14]** The advertised credit **is** the receiver's buffer
commitment: a receiver only advertises what it will buffer until read.
This replaces the superseded draft's `shed_mask`/`RECV_BUFFER` receive
backpressure wholesale — the problem that machinery solved (bounded
receive memory without acknowledging undelivered data) is solved
principledly: an in-credit packet always has buffer room **by
construction**, so no delivered-but-shed state can exist, and a
beyond-credit packet is a violation (§10.5), not a shed. DATAGRAM frames
need no shed logic — dropping an unreliable datagram at a full queue is
legitimate (§11.5), and acknowledging its packet is honest (an ACK
confirms packet arrival; datagram delivery was never promised). The
removal is sound only because flow control is now load-bearing for memory
safety; the supporting check: **no non-stream, non-datagram frame can
force unbounded buffering** — ACK processing is bounded intersecting
(§12.5), credit frames apply as O(1) monotone-max, PING/PADDING are
O(1), RESET_STREAM *frees* state, and CLOSE enters the linger. What
survives from the removed design is its invariant: liveness and roaming
are driven only by authenticated, window-marked packets (§7.2).

**The second bound — reassembly fragments.** **[RATIFIED 2026/08/14]** Byte credit
bounds the *span* a receiver must cover, not the *number of stored
discontiguous ranges* inside it: one-byte STREAM frames at offsets
0, 2, 4, … would store ~512 000 ranges within 1 MiB of credit, inflating
real memory 25–50× over the advertised commitment — the exact
amplification argument §11.3 uses against byte-bounded datagram queues,
applied to the reliable path. The mandate: **per-stream reassembly state
MUST be O(advertised credit) and MUST NOT scale with the number of
received frames.** Two implementations are admissible: (a) a
span-allocated buffer plus a received-bitmap (a 1 MiB span costs
1 MiB + 128 KiB, frame-count-independent; the ceiling below is then
unreachable), or (b) the default — received ranges are coalesced on
insert, and a stream whose stored discontiguous ranges would exceed
`REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol
violation: CLOSE with `PROTOCOL_VIOLATION` (§15.3; quinn's
defragment-plus-hard-fail shape). The ceiling value ships
ratified-but-revisitable, gated on the Appendix B
defragmentation/throughput check.

| Constant | Value |
|---|---|
| `REASSEMBLY_CHUNKS_MAX` | 1024 stored discontiguous ranges per stream |

**Consumption, defined.** Consumption is **the application taking bytes
out of the connection core** — a `read()` draining the contiguous prefix,
a message or datagram claimed by its verb (§16.4), or a retirement
true-up (§10.3). No unbounded intermediate queue may exist between core
and handle: message and datagram payloads stay accounted inside the core
(or its flow-control ledger) until the handle takes them, and reliable
stream or message data MUST NOT be droppable under the shell's
non-blocking delivery policy (§16.8) — §16.4's pull model is what makes
both properties implementable.

### 10.7 Exemptions

- **DATAGRAM frames are flow-control-exempt** (they are
  congestion-controlled instead, §11.3).
- **Retransmissions of the same stream bytes consume no new credit** —
  credit accounts the stream's offset high-water mark, not bytes on the
  wire.

## 11. Datagrams *(DRAFT 2026/08/13)*

### 11.1 Contract

The DATAGRAM frame (§8.4) is the unreliable path: no delivery promise, no
ordering promise, no retransmission, **no sequence identity at all**
(applications needing one embed their own). Datagrams are

- **ack-eliciting**: the carrying packet is tracked and acknowledged — an
  ACK confirms the packet arrived, not that the datagram was delivered to
  the application (§11.5);
- **congestion-controlled**: they count in flight and the admission gate
  applies (§14.5), but loss never retransmits them;
- **flow-control-exempt**: they consume no MAX_DATA credit (§10.7).

### 11.2 Size bound

| Constant | Value |
|---|---|
| `MAX_DATAGRAM_PAYLOAD` | 1169 B (= `MAX_PLAINTEXT` − 1: a type-`0x30` frame's one type byte, data to the end of the plaintext) |

A datagram never spans packets.

### 11.3 Queues

| Constant | Value |
|---|---|
| `DATAGRAM_SEND_QUEUE` | 64 datagrams |
| `DATAGRAM_RECV_QUEUE` | 64 datagrams |

Both queues are bounded by **count**, discipline **drop-oldest with the
newest always accepted** (the arriving or newly-sent datagram always
enters; the oldest queued is evicted to make room). Count-not-bytes is a
deliberate divergence from quinn's byte bounds (1.25 MB receive / 1 MiB
send), argued: the worst case is crisp — 64 × 1169 B ≈ 73 KiB per queue
per connection — the entry count is bounded (a byte bound admits
allocation-churn amplification from a tiny-datagram flood: 1.25 MB of
one-byte datagrams is 1.25 million queue entries), and datagrams only
arrive from the authenticated peer. The constants are
ratified-but-revisitable. Both queues, their eviction discipline, and the
drop counters live **in the connection core**, not the shell (§16.4) —
the bound is protocol state, not a delivery detail.

### 11.4 Oversize rules

- **Send**: a payload > `MAX_DATAGRAM_PAYLOAD` returns
  `DatagramError::TooLarge` at the handle, before any queue.
- **Receive**: an oversized DATAGRAM frame is **impossible by
  construction** — a frame's data lies inside one sealed packet's
  plaintext ≤ `MAX_PLAINTEXT`, and a length field overrunning the
  plaintext is a structural failure (§8.2) — so no receiver oversize rule
  exists; the connection-fatal case in quinn's model is unrepresentable
  here.

### 11.5 Drops are counted

Every queue-overflow drop — send-side eviction and receive-side
eviction — increments a counter surfaced on the `slither::frames` trace
target (§18.2); the counters are core state (§11.3, §16.4), so the trace
is core behaviour, not a shell detail. A silent drop is a known
operability weakness of the
precedent and is deliberately not copied.

## 12. ACK *(DRAFT 2026/08/13)*

### 12.1 Range semantics

The ACK frame (layout §8.4) acknowledges received packet counters as
descending ranges, RFC 9000 §19.3.1 semantics in varints:

- the first block covers `largest − first_range ..= largest`;
- for each subsequent `(gap, range)` pair, with `prev_smallest` the
  smallest counter of the preceding block: the block's largest is
  `prev_smallest − gap − 2`, and the block covers
  `block_largest − range ..= block_largest`;
- a block descending below counter zero is structural failure (§8.4).

`ack_delay` is raw microseconds as a varint — no exponent scaling (there
is no negotiation to carry an exponent, and immediate or 25 ms delays fit
1–4 bytes). No ECN counts exist (§19).

### 12.2 Derivation — fused to the replay window

An ACK is derived from the replay window's snapshot (greatest + bitmap,
§7.2) — the single received-packet record; there is no second tracker.
Because the 2048-bit worst case (alternating) no longer fits one packet,
construction emits ranges **newest-first, descending**, truncating at
`MAX_ACK_RANGES` pairs or at packet capacity, whichever binds — the
dropped oldest ranges are exactly the ones prior ACKs most likely already
carried.

| Constant | Value |
|---|---|
| `MAX_ACK_RANGES` | 64 — the cap on `range_count` (the `(gap, range)` pairs; at most 65 blocks including the first) |

A received ACK with `range_count` > 64 is malformed — a structural
failure of §8.2's class (nothing from the packet applied; CLOSE with
`PROTOCOL_VIOLATION`).

### 12.3 `ack_delay`

Measured from the arrival of the packet bearing `largest` to the emission
of the ACK, in microseconds. When the window's largest counter was not
frame-seen (a keepalive's counter, §7.5), `ack_delay = 0`. The RTT
estimator subtracts the peer's `ack_delay` capped at `MAX_ACK_DELAY`
(§13.1).

### 12.4 Delayed-ACK policy

**[RATIFIED 2026/08/14]** The old immediate-ACK-per-packet policy is replaced by
QUIC's default; congestion control now consumes ACK timing, and streams
make 1:1 ACK traffic a real reverse-path cost, while 25 ms is already the
PTO formula's assumption — the change is self-consistent. The cost is one
more named timer and slightly laggier RTT samples; immediate-ACK remains
the conservative fallback if the Appendix B timing obligations disappoint.

| Constant | Value |
|---|---|
| `MAX_ACK_DELAY` | 25 ms |

- An ACK is owed after every **2nd** ack-eliciting packet, or when the
  `AckDelay` timer (armed at `MAX_ACK_DELAY` on receipt of the first
  unacknowledged ack-eliciting packet) fires — whichever first.
- An ACK is owed **immediately** on out-of-order arrival: an ack-eliciting
  packet whose counter is not exactly one greater than the window's
  previous greatest (it opens, fills, or sits inside a gap). The first
  ack-eliciting packet of a session has no previous greatest, so the rule
  applies vacuously and yields an immediate ACK — harmless, and it seeds
  the peer's RTT estimate early.
- An owed ACK rides the next outgoing packet (packing order §8.5); if none
  is pending, a standalone ACK packet is generated. Pure-ACK packets are
  sealed `seal_quiet` (§7.4), are not ack-eliciting (no ACK-of-ACK loops),
  are never tracked for loss, and bypass the congestion window (§14.5).

### 12.5 Processing a received ACK

- **Bounded intersecting processing**: a received ACK is intersected with
  the sender's in-flight set, never materialised into the counters its
  ranges imply — a wire-legal ACK whose 64 ranges span millions of
  counters costs O(in-flight × range_count), never a multi-megabyte
  expansion.
- An ACK whose `largest` exceeds the highest counter this session has
  sealed is **ignored whole** — the frame applies as a no-op with a trace;
  the packet's other frames still apply (§8.2's parse/apply split). This
  is a **deliberate divergence** from RFC 9000 §13.1's
  SHOULD-treat-as-`PROTOCOL_VIOLATION`: under bounded intersecting
  processing the forged-future ACK is already harmless, and the no-op
  keeps the failure local.
- Newly acknowledged packets clear their frames from the in-flight set and
  feed recovery (§13.2) and the congestion controller (§14.2). Duplicate
  acknowledgment of a counter is a no-op.

## 13. Loss recovery *(DRAFT 2026/08/13)*

Per connection (a connection has exactly one session — §7.8), over the
ack-eliciting sent-packet map. RFC 9002's shape throughout.

### 13.1 RTT estimation (RFC 9002 §5)

The estimator keeps `latest_rtt`, `smoothed_rtt`, `rttvar`, and `min_rtt`.
An ACK yields an RTT sample when its `largest` is newly acknowledged and
at least one newly acknowledged packet is ack-eliciting. First sample:
`smoothed_rtt = latest_rtt`, `rttvar = latest_rtt / 2`,
`min_rtt = latest_rtt`. Later samples: `min_rtt = min(min_rtt, latest)`;
the peer's `ack_delay`, capped at `MAX_ACK_DELAY`, is subtracted only when
doing so does not push the sample below `min_rtt`; then
`rttvar = ¾ · rttvar + ¼ · |smoothed_rtt − adjusted|` and
`smoothed_rtt = ⅞ · smoothed_rtt + ⅛ · adjusted`. Before any sample the
estimator seeds from `K_INITIAL_RTT` with `rttvar = K_INITIAL_RTT / 2`.

| Constant | Value |
|---|---|
| `K_INITIAL_RTT` | 333 ms |
| `K_GRANULARITY` | 1 ms |

The RTT estimator is **connection-scoped**: it survives roaming
(a path property), but as a **prior**, not a fact — it seeds the new
path's first PTO and is corrected by the next sample (§14.6).
One exception to `min_rtt`'s monotonicity: on a roam (§7.3), `min_rtt` is
**re-seeded from the first post-roam sample** — it MUST be allowed to
rise, or an old short path pins the PTO floor under a new long one and
manufactures spurious (cwnd-exempt, budget-bound) probes for the
connection's remaining life.

### 13.2 Ack-based loss detection (RFC 9002 §6.1)

A tracked packet is declared lost when a later packet in its space has
been acknowledged **and** either:

- **packet threshold**: it is `K_PACKET_THRESHOLD` = 3 or more counters
  below the largest acknowledged; or
- **time threshold**: it was sent more than
  `loss_delay = max(9/8 · max(smoothed_rtt, latest_rtt), K_GRANULARITY)`
  before the acknowledgment arrived.

Survivors inside the threshold arm the `Loss` timer at
`time_sent + loss_delay` (minimum across in-flight packets). Lost packets'
frames re-queue by retransmission class (§8.7); the lost packet's bytes
leave `bytes_in_flight`, and the loss feeds the congestion controller once
per episode (§14.3).

### 13.3 Probe timeout (RFC 9002 §6.2)

```
PTO = smoothed_rtt + max(4 · rttvar, K_GRANULARITY) + MAX_ACK_DELAY
```

anchored at the last ack-eliciting send, doubled per consecutive
unanswered probe (`2^pto_count`), capped at `PTO_BACKOFF_CAP` = 2⁶.
`pto_count` resets to 0 whenever any packet is newly acknowledged. The
probe train is ended by liveness (`DEAD_TIMEOUT` — under symmetric loss
and, since the anchor is the receive clock, under asymmetric loss too,
§7.4) — the cap is an overflow
guard, not a death sentence.

**The `Pto` timer is armed only while at least one ack-eliciting packet
is in the sent map** (RFC 9002 §6.2.1); when the map empties it is
disarmed, and when the `Loss` timer is armed it takes precedence (§16.5).
Without the precondition an idle connection self-sustains a probe train —
PTO fires, the bare PING is ack-eliciting, the peer ACKs, `pto_count`
resets, the timer re-arms — at ~20 packets/s against the 10 s keepalive
cadence, defeating §16.5's timer economy.

| Constant | Value |
|---|---|
| `K_PACKET_THRESHOLD` | 3 |
| time threshold | 9⁄8 |
| `PTO_BACKOFF_CAP` | 2⁶ |

### 13.4 Probe content

A firing PTO sends one ack-eliciting packet: pending retransmittable
frames oldest-first if any exist, else a bare PING. The one-packet,
pending-oldest-first-else-PING content rule is a **deliberate
simplification** of RFC 9002 §6.2.4, which sends new data before old and
up to two datagrams. Probes are sealed
`seal_quiet` (liveness-neutral, §7.4) **and** exempt from the congestion
admission gate (§14.5) — two independent properties of the same send, for
different reasons (a partitioned session must still die; a black-holed
path must stay probeable). Probes are **not** exempt from §7.3's
anti-amplification budget on an unvalidated address.

### 13.5 Frames, never packets

A lost packet is never retransmitted as a packet: its still-needed frames
are re-framed into new packets under fresh counters (§8.7), STREAM ranges
split or merged freely. The sent-packet map holds, per ack-eliciting
counter: send time, the frame identities aboard (stream ranges, credit
frame identities, RESET_STREAM, PING, DATAGRAM markers), and the packet's
**size in bytes** — the `size` field feeding `bytes_in_flight` (§14.5).
Non-ack-eliciting packets are never inserted.

### 13.6 What resets when — the roam seam

Roaming is the **only** recovery seam: there is no rekey, and a
connection's one session lives as long as the connection (§7.8).

**Roaming** (§7.3): the sent map is **kept** — ACKs for packets in flight
to the old address still resolve, and `bytes_in_flight` remains consistent
with the retained map; loss detection and PTO continue undisturbed. The
congestion controller resets with the **pre-roam flight fenced off**
(§14.6): packets sent before the roam still resolve for loss and
retransmission, but feed no congestion event, no persistent-congestion
walk, no RTT sample, and no `app_limited` window growth — the
roam-triggering path break must not be read as fresh-path congestion (the
break *is* §14.4's predicate: two far-apart losses with nothing acked
between; unfenced, every roam would start at `MINIMUM_WINDOW` instead of
`INITIAL_WINDOW`). The RTT estimator is treated as suspect-but-kept, with
`min_rtt` re-seeded from the first post-roam sample (§13.1).
(Consequence: immediately after a roam,
`bytes_in_flight` may exceed the fresh initial window; the admission gate
then blocks new sends until old-path packets are acknowledged or declared
lost — a bounded stall of at most one loss-detection/PTO cycle, kept
probeable by the PTO exemption within §7.3's budget.)

## 14. Congestion control *(DRAFT 2026/08/13)*

### 14.1 The controller seam

Congestion control sits behind a small trait (quinn-proto's shape), wired
at the three existing recovery mutation points — packet sent, packets
newly acknowledged, loss episode:

```rust
trait Controller {
    fn on_sent(&mut self, now: Instant, bytes: u64);
    fn on_ack(&mut self, now: Instant, sent_time: Instant, bytes: u64, app_limited: bool);
    fn on_congestion_event(&mut self, now: Instant, sent_time: Instant,
                           is_persistent: bool, lost_bytes: u64);   // once per loss episode
    fn window(&self) -> u64;
}
```

NewReno is the one v1 implementation; CUBIC and BBR are pure additions
behind the trait later (§19). Nothing congestion-related appears on the
wire — the wire is deliberately CC-agnostic.

### 14.2 NewReno

| Constant | Value |
|---|---|
| `INITIAL_WINDOW` | 12 000 B (= min(10 × 1200, max(2 × 1200, 14 720)) — RFC 9002 §7.2 at `MAX_DATAGRAM` = 1200) |
| `MINIMUM_WINDOW` | 2 400 B (= 2 × 1200) |
| `LOSS_REDUCTION_FACTOR` | 0.5 |

`ssthresh` starts at `u64::MAX`. **Slow start** (cwnd < ssthresh): cwnd
grows by the bytes newly acknowledged. **Congestion avoidance**: integer
appropriate-byte-counting — accumulate acknowledged bytes and add one
`MAX_DATAGRAM` to cwnd each time the accumulator exceeds cwnd (one MTU per
RTT, no floating point).

### 14.3 The recovery period — one cut per episode

On a congestion event (any loss of an ack-eliciting packet):
`cwnd = max(cwnd × 0.5, MINIMUM_WINDOW)`, `ssthresh = cwnd`, and the
recovery period starts at the event. Subsequent congestion events for
packets **sent before** the recovery period started are ignored — one
loss burst produces exactly one window cut. Symmetrically,
**acknowledgments of packets sent before the recovery period started do
not grow the window** (RFC 9002 §7.3.2) — the same
`sent_time ≤ recovery_start` test gates both the event and the growth;
without it, the pre-cut flight's ACKs keep inflating cwnd through the
recovery they triggered. `on_congestion_event` fires
once per loss episode (after the full lost-packet scan), never once per
lost packet.

### 14.4 Persistent congestion

Computed inside the loss-detection walk (§13.2), not bolted on: if two
ack-eliciting packets sent more than
`persistent_period = PTO × PERSISTENT_CONGESTION_THRESHOLD` apart are both
lost with **no packet acknowledged between them**, and **a prior RTT
sample exists** (the pre-sample `K_INITIAL_RTT` phase never triggers it),
the controller collapses: `cwnd = MINIMUM_WINDOW`, slow start effectively
restarts. `persistent_period` evaluates the §13.3 PTO formula **with
`pto_count = 0`** (RFC 9002 §7.6.1): the backoff is deliberately excluded
so the period is a property of the path, not of the probe count — with
the backoff included, the threshold would run up to 2⁶× too long and
persistent congestion would never trigger under exactly the sustained
loss it exists to detect. Packets sent before a roam are excluded from
the walk (§13.6, §14.6).

| Constant | Value |
|---|---|
| `PERSISTENT_CONGESTION_THRESHOLD` | 3 |

### 14.5 `bytes_in_flight` and the admission gate

`bytes_in_flight` is the sum of the `size` fields over the sent-packet map
(§13.5) — ack-eliciting packets only. The send-side admission gate:

```
send permitted  iff  bytes_in_flight + candidate_size ≤ cwnd
```

**Exemptions**, exhaustively:

- **PTO probes** (§13.4) — a black-holed path with a full window must
  stay probeable; the exemption is orthogonal to, and coexists with, the
  probe's liveness-neutral `seal_quiet`.
- **Non-ack-eliciting control packets** — pure ACKs, CLOSE, keepalives —
  are never tracked in flight and never gated.

DATAGRAM frames are congestion-controlled: they count in flight and the
gate applies (a queued datagram waits for window room), but loss never
retransmits them (§11.1).

**The exemptions are cwnd-scoped only.** On an unvalidated address — a
fresh roam target or msg1-source anchor — §7.3's anti-amplification
budget binds **all** output, the exempt classes above included: probes,
pure ACKs, CLOSE, and keepalives are free of the congestion window, never
of the budget.

**`app_limited`** (quinn's mechanism, pinned): the send path maintains an
application-limited flag — set when the sender runs out of queued data
with cwnd headroom remaining — and **records it onto each sent packet**;
`on_ack` reads the acknowledged packet's recorded flag, and when it is
set the controller does not grow the window on that acknowledgment — idle
connections earn no phantom window. The hostile-peer reasoning, stated: a
peer controls ACK arrival timing and can therefore choose *when* the
predicate is consulted, but the flag is recorded at send time by *us*, so
ACK-timing games cannot un-set it; the dangerous direction is a window
that grows while the application is idle and later discharges as a burst
— the recorded-per-packet form bounds it, and §7.3's budget caps what any
accrued window can emit at an unvalidated address.

### 14.6 The roam reset seam

**[RATIFIED 2026/08/14, in reduced form — the rekey seam is deleted with
§7.6; the roam seam stands exactly as ratified.]** The controller resets
to initial state (cwnd = `INITIAL_WINDOW`, ssthresh = `u64::MAX`) on the
one seam; the RTT estimator survives it as a prior (§13.1):

- **roaming** — on the authenticated address move (§7.3): a new path, no
  continuity evidence; the sent map is *kept* (§13.6) but the window is
  not. **The recovery-period marker is set to the roam instant — not
  cleared** — and packets sent before the roam are fenced from the fresh
  controller (§13.6): they resolve for loss and retransmission but feed
  no congestion event, no persistent-congestion walk, no RTT sample, and
  no `app_limited` growth (RFC 9000 §9.4's per-path separation; quinn's
  path-generation stamping). This is conservative per RFC 9002/quinn
  precedent (a fresh
  controller per path); a reviewer could argue for keeping cwnd across a
  same-NAT port rebind — declined here for want of evidence the path is
  the same. One stated divergence: RFC 9000 §9.4 also resets the RTT
  estimator on a path change; keeping it as a prior is a deliberate
  slither choice (roaming here is the same peer moving, not adversarial
  migration), with the consequence that a roam onto a slower path briefly
  runs on stale `smoothed_rtt` — corrected by the first post-roam sample,
  with `min_rtt` re-seeded so the PTO floor may rise (§13.1).

### 14.7 Explicitly out

No pacing (no sub-RTT wakeups in the v1 shell; a 12 KB initial window
bounds bursts adequately), no ECN (wire and socket work), no CUBIC/BBR
(trait-additive later). All deferred with pointers in §19.

## 15. CLOSE and the connection lifecycle *(DRAFT 2026/08/13)*

### 15.1 The CLOSE frame

**[RATIFIED 2026/08/14]** Deliberate teardown gets a wire signal — without one, a
clean disconnect costs the peer 15 s of liveness wait. The semantics are
minimal by design: no close-ACK, no handshake, no QUIC
transport/application split, a 5 s linger instead of QUIC's 3×PTO
closing/draining bookkeeping. The QUIC-faithful alternative is more state
for little gain at slither's scale. Only the **authenticated, in-seal**
CLOSE exists — nothing unauthenticated can kill a connection; the
reserved cleartext close packet type (`0x04`) stays dead.

| Constant | Value |
|---|---|
| `CLOSE_LINGER` | 5 s |
| close-reply rate | ≤ 1 CLOSE per second |
| `CLOSE_REASON_MAX` | 256 B |

### 15.2 Semantics

- **Local close** — `close(error_code, reason)` (the handle truncates
  `reason` to `CLOSE_REASON_MAX`, §8.4): emit CLOSE (sealed
  `seal_quiet`, §7.4) and enter **closing** for `CLOSE_LINGER`. The
  closing state retains **the seal capability, the receive cipher
  states, and the replay window** (all stream, flow-control, recovery,
  and congestion state may drop immediately): a reply is owed only to an
  **authenticated, window-fresh** inbound packet — never to a packet
  that merely routed by `receiver_index`, which an off-path forger who
  observed the cleartext index could mint — and is sent **to the
  session's endpoint address** (the closing state does not roam; never
  to the triggering packet's source). Replies are capped at one CLOSE
  per second; at linger expiry (`CloseLinger` timer, §16.5), drop all
  state. The linger's reply rule
  is CLOSE's only reliability mechanism — CLOSE is not ack-eliciting and
  is never retransmitted by loss detection. A CLOSE **received** while
  closing moves the connection to the reply-free draining behaviour
  below: two closing endpoints go quiet rather than ping-ponging replies
  at 1 Hz for the linger.
- **Receiving an authenticated CLOSE**: surface
  `ConnectionLost::PeerClosed { code, reason }`, emit **nothing**, hold a
  brief drain for the same `CLOSE_LINGER` (discarding late packets, no
  replies), then drop all state.
- **Protocol violations by the authenticated peer** (§8.2's semantic
  class — `FLOW_CONTROL_ERROR`, `STREAM_LIMIT_ERROR`,
  `STREAM_STATE_ERROR`, `FINAL_SIZE_ERROR` — and its post-AEAD
  structural class, `PROTOCOL_VIOLATION`) get a signalled death instead
  of a silent one: emit CLOSE with the matching code, then linger as for
  a local close. **[RATIFIED 2026/08/14 — one ruling, two homes with
  §8.2.]** The locally surfaced error is
  `ConnectionLost::ProtocolViolation { code }` (§18.1) — a dedicated
  variant, not `LocallyClosed`: "I closed" and "the peer's misbehaviour
  forced a close" are opposite causes with opposite operational
  responses (peer reputation, allow-list demotion, alerting), and
  §18.1's own precedent is that a security signal earns its own variant.
  The addition is enum-level only; the CLOSE frame and the on-wire code
  registry are unchanged.

### 15.3 Error-code registry

| Code | Name | Meaning |
|---|---|---|
| `0x00` | `NO_ERROR` | graceful close |
| `0x01` | `PROTOCOL_VIOLATION` | a semantic violation with no more specific code |
| `0x02` | `FLOW_CONTROL_ERROR` | advertised credit exceeded (§10.5) |
| `0x03` | `STREAM_LIMIT_ERROR` | cumulative stream limit exceeded (§10.4) |
| `0x04` | `STREAM_STATE_ERROR` | a frame for a stream its sender could not touch (§8.4) |
| `0x05` | `FINAL_SIZE_ERROR` | final-size disagreement (§9.5, §9.6) |
| `0x06`–`0x0f` | reserved | transport-reserved; never sent |
| ≥ `0x10` | application | application-defined codes via `close()` |

### 15.4 The teardown matrix

Every way a connection dies, what is transmitted (mostly: nothing), and
what each side observes:

| Cause | Transmitted | Local surface | Peer's view |
|---|---|---|---|
| liveness — 15 s without an authenticated fresh receive (§7.5) | nothing | `ConnectionLost::TimedOut` | its own liveness fires ≈ symmetrically |
| nonce exhaustion (§7.9) | nothing | `ConnectionLost::NonceExhausted` | liveness |
| local `close(code, reason)` / last-handle drop (§16.2) | CLOSE, then ≤ 1 reply/s for 5 s | `ConnectionLost::LocallyClosed` | `PeerClosed { code, reason }` |
| peer's CLOSE received | nothing (drain only) | `ConnectionLost::PeerClosed { code, reason }` | (it closed) |
| protocol violation by the peer — semantic or post-AEAD structural (§8.2, §15.2) | CLOSE(code), linger | `ConnectionLost::ProtocolViolation { code }` | `PeerClosed { code, reason }` |
| replaced — an `Intro` proving this connection's static was **accepted**; the teardown fires at the replacing `accept()` (§5.4, §6.4) | nothing on the old connection | a fresh `Intro` first, then `ConnectionLost::Replaced` at its `accept()` | (it reconnected; its new connection proceeds) |
| endpoint dropped — every handle gone (§16.3) | nothing | — (the driver stops) | liveness, ≤ 15 s |

`ConnectionLost::EndpointDropped` is the answer a surviving verb call
receives when the driver has stopped mid-flight (§18.1) — it is a
handle-side observation, not a teardown cause of its own.

## 16. Object model and the sans-io core *(DRAFT 2026/08/13)*

### 16.1 The object model

```text
Endpoint                                   // socket + demux; owns the accept queue
├── connect(addr, static) → Connecting     // Future → Connection
├── accept().await → Intro → Claimed → Proven → Connection    (§6)
└── Connection                             // one Noise session + the frame layer
      ├── SendStream / RecvStream          // per-stream handles (bidi pairs)
      ├── send_message / recv_message      // §9.8 sugar
      └── send_datagram / recv_datagram    // §11
```

| Object | Owns |
|---|---|
| `Endpoint` | socket + demux (`receiver_index → Connection`, pending-index table), the stage-0 intro queue + staged-accept verbs, the hint set + the internal tie-break (§6.6–§6.7), all initiator pendings, the timestamp guard, the per-connection replacement basis (§17.4), index minting, the root RNG |
| staged objects (`Intro → Claimed → Proven`) | one inbound initiation's graduated state; each verb is a driver round-trip; drop = silent reject at every stage |
| `Connection` | one Noise session (seal/open, replay window, roaming, liveness, the epoch ratchet), the unified frame layer, recovery + congestion controller, connection-level flow control, the streams table, the datagram queues, the CLOSE lifecycle |
| stream handles | per-stream state: send buffer + un-ACKed ranges + FIN, or reassembler + credit ledger; borrow the connection core through the shell |

**One connection per remote static, endpoint-wide.** **[RATIFIED
2026/08/14, clause flipped per the ratchet-only ruling; amended
2026/08/14.]**
`connect()` to a static with a live `Connection` or an in-flight outbound
connect returns `ConnectError::AlreadyConnected`; an authenticated
inbound initiation whose proven static matches a live connection is, by
definition, a **replacement of that connection, admitted via
`accept()`** (§5.4, §6.4) — never a second concurrent connection. Every
routing rule keys on this invariant; without it, "which connection does
this initiation replace" has no answer.
A **staged chain in progress is deliberately not in `connect()`'s list**,
and cannot be: until `authenticate()` the chain's static is merely
claimed, and §6.1 forbids keying anything durable on an unproven claim.
The invariant is held at the other end of that race instead — a proven
static that is PENDING at `accept()` cancels the pending (§6.4's PENDING
branch), so `read_identity()` → `connect()` → `accept()` on one static
still yields exactly one connection, and no static is ever LIVE and
PENDING at once.
WireGuard's model is identical (one `wg_peer` per static). `connect()` to
our own static is out of scope under this rule, which is what makes the
tie-break's equal-statics case unrepresentable (§6.7). This forecloses
multi-connection-per-peer for this wire line.

### 16.2 Shell surface

```rust
impl Endpoint {
    pub fn builder() -> EndpointBuilder;                 // identity, socket/Wire, Config
    pub async fn accept(&self) -> Option<Intro>;         // None = endpoint closed
    pub fn connect(&self, remote: SocketAddr, remote_static: PublicKey)
        -> Result<Connecting, ConnectError>;             // Connecting: Future<Output = Result<Connection, ConnectError>>
}
// staged verbs: §6.2

impl Connection {
    pub async fn open_bi(&self)  -> Result<(SendStream, RecvStream), ConnectionLost>;
    pub async fn open_uni(&self) -> Result<SendStream, ConnectionLost>;
    pub async fn accept_bi(&self)  -> Result<(SendStream, RecvStream), ConnectionLost>;
    pub async fn accept_uni(&self) -> Result<RecvStream, ConnectionLost>;
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>;
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub async fn close(&self, code: u64, reason: &[u8]);
    pub fn set_persistent_keepalive(&self, interval: Option<Duration>);
    // accessors (synchronous shared-cell reads, §16.8):
    pub fn remote_static(&self) -> PublicKey;
    pub fn remote_address(&self) -> SocketAddr;
    pub fn session_id(&self) -> SessionId;
    pub fn is_established(&self) -> bool;
}

impl SendStream {
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, WriteError>;
    pub async fn finish(&mut self) -> Result<(), WriteError>;
    pub fn reset(&mut self, error_code: u64);
    pub fn id(&self) -> Option<StreamId>;   // None before establishment (§16.9)
}
impl RecvStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
        // Ok(None) = FIN reached, all data delivered
    pub fn id(&self) -> Option<StreamId>;   // None before establishment (§16.9)
}
```

`open_bi`/`open_uni` wait for MAX_STREAMS allowance when the cumulative
limit is exhausted (§10.4), woken by `StreamsAvailable` (§16.4); `write`
waits for stream and connection credit
(§10.1); `send_message` waits for stream allowance, then behaves per §9.8.
`send_datagram` never waits (drop-oldest queue, §11.3). `finish()`
resolves when the FIN is accepted into the stream's send state (errors
surface as `WriteError`); `close()` resolves once the CLOSE frame is
sealed and the closing state is entered (§15.2), and truncates `reason`
at `CLOSE_REASON_MAX` (§8.4). `set_persistent_keepalive` rejects
intervals below `DEAD_TIMEOUT` (§7.5). The types are
normative in shape; an implementation may rename.

**Drop semantics.** Dropping a staged object is a silent reject (§6.2).
Dropping the last handle to a `Connection` performs
`close(NO_ERROR, "")` — the graceful teardown of §15.2 (the superseded
"drop = silent local teardown" rule carried onto the wire signal that now
exists). Dropping a `SendStream` without `finish()` resets it with error
code 0. Dropping a `RecvStream` abandons the receive half: arrivals for
it are discarded and stream-level credit is never again advanced (a
sender that keeps pushing stalls at the stream window), the half closes
when the pinned final size or a reset arrives (§9.7), and on that
retirement its bytes up to the final size count as consumed at the
connection level (§10.3) — an abandoned stream never wedges the
connection window. Dropping every handle stops the driver and every
connection dies silently — nothing transmitted (§15.4).

### 16.3 Driver and handle lifetimes

The shell is **one `!Send` driver task**, spawned with
`tokio::task::spawn_local` (a `LocalSet` is required), owning the socket
for both receive and send and owning both sans-io cores. **Do not add
`Send` bounds to the actor path** — a DH provider is not required to be
`Send` (hardware statics). `Endpoint`, staged objects, `Connection`, and
stream handles are thin channel-backed clients; connections do not send on
socket clones. The `Wire` trait seam (real socket or `testutil::FlakyWire`)
is the driver's I/O boundary. The driver lives while any handle lives;
dropping every handle stops it, and every session dies silently with it.

### 16.4 The two cores and the poll contract

**[RATIFIED 2026/08/14, amended 2026/08/14]** The protocol logic lives in
two pure state machines —
`core::Endpoint<I: Identity>` and `core::Connection` — with the str0m
single-`poll_output` contract: **every mutating call** (`handle_datagram`,
`handle_timeout`, verb calls, stream/datagram/message operations,
`connect`) **is followed by draining `poll_output()` to the terminal
`Timeout(Option<Instant>)`**, which is simultaneously the drain sentinel
and the next-deadline announcement — a driver cannot forget to drain.
Connection→endpoint events fold into `ConnOutput::ToEndpoint` (one drain
loop, no second queue to forget). The generic `I: Identity` must reach the
endpoint core's type (the mid-state map is typed over `I::Provider`).

```rust
impl<I: Identity> core::Endpoint<I> {
    fn new(now: Instant, config: Config, identity: I, rng_seed: [u8; 32]) -> Self;
    fn connect(&mut self, now: Instant, remote: SocketAddr, remote_static: PublicKey)
        -> Result<(ConnectionId, core::Connection), ConnectError>;
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Disposition;
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_connection_event(&mut self, id: ConnectionId, ev: ToEndpoint);
    fn poll_output(&mut self) -> EndpointOutput;                       // drain to Timeout
    // staged verbs (§6.2), by IntroId:
    fn read_identity(&mut self, id: IntroId) -> Result<PublicKey, IntroError>;
    fn authenticate(&mut self, now: Instant, id: IntroId)
        -> Result<(PublicKey, Timestamp), AuthError>;
    fn accept(&mut self, now: Instant, id: IntroId)
        -> Result<(ConnectionId, core::Connection), AcceptError>;
    fn reject(&mut self, id: IntroId);
}

enum Disposition { ForConnection(ConnectionId), Done }

enum EndpointOutput {
    Transmit(Transmit),                          // msg1/msg2, retransmits, tie-break msg2
    IntroReady(IntroId, SocketAddr),
    ToConnection(ConnectionId, Install),
    HandshakeFailed(ConnectionId, ConnectError), // shell-only (below)
    Timeout(Option<Instant>),                    // terminal
}
struct Install { session: EstablishedSession }
struct EstablishedSession { /* the hiss transport pair (seal + open), our session
                               index, the peer's index, and the anchor address (§5.6) */ }
struct Transmit { to: SocketAddr, data: Vec<u8> }
```

```rust
impl core::Connection {
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_endpoint_event(&mut self, now: Instant, ev: Install);    // Install only
    // application surface (mirrors §16.2, core-shaped):
    fn open(&mut self, dir: Dir) -> Result<StreamId, StreamsExhausted>;
    fn write(&mut self, now: Instant, id: StreamId, data: &[u8]) -> Result<usize, WriteError>;
    fn finish(&mut self, id: StreamId) -> Result<(), WriteError>;
    fn reset(&mut self, now: Instant, id: StreamId, error_code: u64);
    fn read(&mut self, id: StreamId, buf: &mut [u8]) -> Result<Option<usize>, ReadError>;
    fn send_message(&mut self, now: Instant, msg: &[u8]) -> Result<(), MessageError>;
    fn send_datagram(&mut self, now: Instant, data: &[u8]) -> Result<(), DatagramError>;
    fn close(&mut self, now: Instant, code: u64, reason: &[u8]);
    // claim verbs — the pull model (§10.6, §11.3, §9.8):
    fn accept(&mut self, dir: Dir) -> Option<StreamId>;   // claim a peer-opened stream
    fn recv_message(&mut self) -> Option<Vec<u8>>;        // claim the oldest complete unclaimed message
    fn recv_datagram(&mut self) -> Option<Vec<u8>>;       // claim the oldest queued datagram
    fn poll_output(&mut self) -> ConnOutput;
}

enum ConnOutput {
    Transmit(Transmit),
    Event(ConnEvent),
    ToEndpoint(ToEndpoint),
    Timeout(Option<Instant>),                    // terminal
}
enum ConnEvent {
    Established,                                 // the install; the shell resolves Connecting
    StreamOpened { dir: Dir },                   // signal: claim via accept(dir)
    StreamsAvailable { dir: Dir },               // MAX_STREAMS credit arrived (§10.4)
    StreamReadable { id: StreamId },
    StreamWritable { id: StreamId },             // stream/connection credit arrived for a blocked writer
    StreamFinished { id: StreamId },             // send half fully acknowledged
    StreamReset { id: StreamId, error_code: u64 },
    MessageReadable,                             // signal: claim via recv_message() (§9.8)
    DatagramReadable,                            // signal: claim via recv_datagram() (§11)
    AddressMoved { from: SocketAddr, to: SocketAddr },
    Closed(ConnectionLost),
}
enum ToEndpoint {
    Retired { our_index: u32 },                  // teardown: drop the index route (MUST)
}
```

- **`accept()` returns a fully established connection — never followed by
  an `Install`.** `Install` targets only a `connect()`-created connection
  awaiting completion, **exactly once**, resolving its
  `Connecting` — whether from msg2 completion or a lost tie-break's
  admission (§6.7). It carries the session and nothing else: with the
  rekey swap deleted (§7.6) there is exactly one install per connection,
  so no discriminator distinguishes them and none is carried. Emitting a
  symmetry `Install`
  after `accept()` (double-install) and waiting for one that never comes
  are both excluded.
- **The pull model is uniform.** The core **retains** what it has not
  handed over: reassembled-but-unclaimed incoming uni streams, queued
  received datagrams, and peer-opened streams awaiting `accept(dir)`.
  The receive-side `ConnEvent`s are **signals**, not payload carriers —
  the shell wakes the matching blocked verb, and the verb claims through
  the core (`accept`, `recv_message`, `recv_datagram`, `read`). A
  reassembled-never-claimed uni stream holds its stream state and its
  MAX_STREAMS credit until claimed (backpressure by retention, §9.8);
  the 64-datagram receive queue and its drop counter are core state
  (§11.3). This is what §10.6's no-unbounded-intermediate-queue rule and
  §16.8's no-drop-for-reliable-data rule rest on: nothing reliable ever
  sits in a droppable shell channel.
- **`HandshakeFailed` never reaches `core::Connection`**: the shell
  resolves `Connecting` with `Err(ConnectError::TimedOut)` and drops the
  never-established pending core. `handle_endpoint_event` carries
  `Install` only.
- **`Retired` is a MUST**: every terminal `ConnOutput` — a
  `Closed(ConnectionLost)` event, or the completion of the close linger —
  is followed **within the same drain** by `ToEndpoint::Retired`, and the
  shell delivers it to `handle_connection_event` **before** releasing the
  connection's shell-side bookkeeping (else the index route and the
  guard-entry pin leak for the endpoint's life). The
  all-handles-dropped case is exempt (the driver simply stops).
- **`Timeout(None)`** = drained and no deadline armed; `Timeout(Some(d))`
  = drained, next deadline `d`. Identical semantics for both cores.
- **Output ordering within one drain preserves generation order** — a
  transmit and the event it caused come out in that order. Normative;
  tests and logs depend on it.

### 16.5 Time and timers

- **`now: Instant` is an explicit argument on every mutating call**; the
  cores never read a clock. The initiation timestamp (§5.3) is the one
  wall-clock read, behind a clock service injected in the endpoint
  config. `poll_output` takes no `now`.
- **Named timers, single min-deadline out.** The connection core's timer
  table: `Keepalive`, `PersistentKeepalive`, `Liveness`, `Loss`, `Pto`,
  `AckDelay`, `CloseLinger`. `Pto` is armed only while an ack-eliciting
  packet is in the sent map (§13.3). The endpoint core's deadline is the
  min over
  its pendings' retransmit/give-up deadlines, the parked intros'
  expiries, and the timestamp-guard orphan aging (§17.1).
- **`handle_timeout` is idempotent**: each due timer is stopped before its
  logic runs, so spurious or repeated calls no-op. For `Loss`/`Pto` the
  idempotency additionally rests on synchronous sealing (§16.7).
- **Equal-deadline priorities** (normative): give-up beats a same-instant
  retransmit; per connection, loss detection beats PTO and exactly one of
  the two fires per evaluation; teardown collection (liveness,
  `CloseLinger` expiry) precedes keepalive evaluation — a session already
  collected for teardown owes no keepalive; `AckDelay` fires after the
  loss/PTO evaluation at the same instant (the owed ACK then rides any
  probe or retransmission that evaluation produced, §8.5); and
  `PersistentKeepalive` is evaluated last — any marking send the instant
  produced re-arms it (§7.5).

**The lateness bound.**

| Parameter | Value |
|---|---|
| `L` (shell lateness bound) | 250 ms |

> Every armed deadline `D` fires no earlier than `D` and no later than
> `D + L`. `L` is a **conformance parameter of the shell, not of the
> protocol**: the cores expose exact deadlines, and a shell may batch or
> tick provided it honours `L`.

### 16.6 RNG

The endpoint core owns one seeded RNG (constructor `[u8; 32]`;
config-supplied for tests, OS entropy otherwise). Every index, jitter
draw, and — via the forced increment — timestamp draw comes from it. At
connection creation the endpoint draws a 32-byte **sub-seed** for the
connection core (drawn even while unused, so later connection-side
randomness cannot perturb the endpoint's draw order). One root seed
reproduces the whole system.

Session and pending indices MUST be unpredictable to an off-path
observer — index unpredictability is load-bearing for §5.5's
on-path-only completion spend and §15.2's authenticated-only linger
reply — so the config-supplied seed is a **test-only facility**: a
production endpoint seeds from OS entropy, and a build that accepts a
caller-chosen seed is security-relevant and must be feature-gated or
documented as such.

### 16.7 Plan-seal-commit; sealing is synchronous

Packetisation is **plan, seal, commit**: build the packet plan, seal it,
and **only on seal success** commit the recovery transition (dequeue, mark
transmitted, clear the pending ACK, `on_sent`, arm timers). On seal
failure nothing moved — a seal error can never strand frames outside both
the pending set and the loss tracker; the only reachable seal failure is
nonce exhaustion, which is terminal (§7.9). Seal returns
`(counter, bytes)`. Sealing — commit included — executes **within the
mutating call that triggers it** (`handle_timeout`, `handle_datagram`,
the application surface), never lazily inside `poll_output()`; this is
what makes `Loss`/`Pto` idempotency real (a repeated `handle_timeout`
before a drain observes the deadline already advanced by the committed
probe).

### 16.8 The no-blocking invariant

Every driver→handle delivery is a bounded channel with a non-blocking
policy or a oneshot reply that cannot block the driver. The accessors are
**synchronous reads of a shared cell the driver updates** — never driver
round-trips. Per-stream wakers key the shell's blocked-readers/
blocked-writers maps (the quinn pattern): a stream verb that would wait
parks its waker under its `StreamId` and is woken by the matching
`ConnEvent`. The driver never performs a blocking send toward a handle.
The shell is deadlock-free by construction. One class is exempt from the
non-blocking drop policy by prohibition: **reliable data is never
droppable** — stream bytes, messages, and claim-pending receive state
stay in the core until the application takes them (§16.4's pull model,
§10.6); the bounded channels carry signals and wakes, not reliable
payloads.

### 16.9 Early sends

Queued work before establishment is **ordinary work**: the connection core
exists from `connect()`, and early stream opens, writes, messages, and
datagrams land in ordinary stream/queue state, pumping when a session
installs — delivered exactly once after establishment, lost if the connect
fails (the failure surfaces through `Connecting`). There is no special
pre-establishment mechanism.

**Stream identity before establishment.** Wire stream IDs encode opener
parity, which is fixed only at establishment (a tie-break loss makes the
dialler the acceptor — §6.7, §9.1), so **stream IDs are assigned at
establishment**: pre-establishment handles hold core-internal indices, no
frame is emitted before install (nothing sends until a session exists),
and `id()` returns `None` until the connection is established (§16.2). On
install the core maps its internal indices onto the parity the outcome
dictates, in open order — the on-wire IDs are identical whichever
resolution the race takes.

### 16.10 Kernel-free drivability

The whole protocol is drivable without a kernel: two endpoints over
`testutil::FlakyWire` on tokio's **paused clock**, with every timer — the
5 s/10 s/15 s/25 ms/90 s family included — resolving in
virtual time. New behaviour gets a paused-clock flow test, not a sleep
(Appendix B).

## 17. Endpoint-global state *(DRAFT 2026/08/13)*

Four pieces of state are endpoint-core-global; none may be pushed into a
connection.

### 17.1 The timestamp guard

Per-remote-static greatest initiation timestamp (§5.3). Admission (check
**and** record) happens at `authenticate()` on the staged path, at a
re-homed `accept()`'s candidate admission (§6.4), and at the internal
tie-break's admit step (§6.6 step 4); one further write records **without
admitting** — the tie-break **winner**'s side, which authenticates the
loser's initiation post-`ss`, drops it, and records its timestamp anyway
(§6.7). All four writes are post-`ss`, so **only key-holders write guard
entries**; the record is made on a full admission or on that
authenticated winner-side drop, and nowhere else.

The guard alone does **not** make the proven-LIVE replacement of §6.4
safe, and never did. It rejects any candidate whose timestamp is ≤ the
greatest this endpoint has admitted for that static — which kills an
older-or-equal replay — but a genuine retransmit withheld off the wire
carries a strictly greater timestamp by construction (§5.5) and passes,
and where we have admitted nothing for that static the check passes
vacuously. What makes the replacement safe is the **pair**: the guard
plus the per-connection replacement basis (§17.4), which refuses any
candidate not strictly newer than the initiation that established the
live connection, and refuses every candidate outright on a connection we
dialled, where no basis exists. A withheld-newer replay still reaches
the application — as an `Intro` that destroys nothing until it is
accepted (§6.4).

- An entry is **pinned** — never evicted — while a live `Connection`, an
  in-flight outbound pending, or a staged mid-state exists for its static.
  For a staged mid-state (whose static is merely claimed until
  `authenticate()`) the pin never *creates* an entry — a bounded
  exception to §6.1's nothing-durable rule, flipping a bit on an entry a
  key-holder already wrote, reverting on drop.
- All other entries (orphans — dead connections) live in a bounded LRU
  with timer aging:

| Constant | Value |
|---|---|
| `TS_GUARD_ORPHAN_CAP` | 1024 orphan entries (≈ 45 B each) |

**Honesty clause and mitigations.** **[RATIFIED 2026/08/14, amended
2026/08/14]** Evicting an orphan
re-admits a replay of that static's last initiation: the replayed msg1 is
genuine, authenticates, and surfaces as a fresh `Intro` — or, if
accepted, a half-open session reaped by liveness in 15 s (WireGuard
accepts the same on responder restart). Pinning guarantees eviction never
touches an established connection's replacement protection. The clause is
completed rather than left incidental: eviction is attacker-triggerable on
demand (orphans require keys, but self-generated statics are free —
~1024 authenticate-then-drop chains flush the tier at ~2048 DH of our
cost); LRU order is adversarially optimal (longest-idle legitimate peers
evict first); and the observable consequence of a re-admitted replay is a
spurious **unaccepted** `Intro` attributed to a real peer at an
attacker-chosen address — never the destruction of a live connection
(pinning protects established statics, so a live static's guard entry is
never evicted and an older-or-equal replay of it still dies at the
guard; a strictly-newer withheld replay survives the guard, as it always
has, and is then measured against that connection's replacement basis —
§17.4, §6.4 — which refuses it outright where we dialled; and a
replacement in any case executes only at `accept()`); if the application
accepts it, the resulting half-open session dies at 15 s liveness and
leaks nothing, but
"peer is online" side-effects fire on a forgery. The ruled mitigations:
**(i) no-orphan-on-reject** — a static authenticated and then rejected
without ever being accepted writes no orphan (its record drops with the
chain; a pre-existing entry reverts), so the authenticate-then-drop flood
cannot mint orphans; the cost, stated honestly, is that a replay of such
a never-accepted initiation can be re-authenticated later, surfacing only
as a fresh `Intro`. **(ii) Timer aging** — orphans age out on an
`INTRO_TTL`-scale timer as well as the LRU cap. **(iii) LRU "use" is
admission only** — recency refreshes on a successful post-`ss` record,
never on a failed check, keeping the write path key-holder-only.

### 17.2 `last_init_timestamp`

The endpoint-global outbound monotonic forcing (§5.3). It survives across
connection generations to the same peer — close-and-reconnect still emits
strictly greater — so endpoint scope is the correct superset.

### 17.3 The index tables

`index → connection` (sessions) and `pending-index → connection`
(in-flight initiations). **Index minting draws a random nonzero `u32` and
re-draws while the value is present in *either* table** — drawn from the
endpoint RNG and off-path-unpredictable by requirement (§16.6) — required
because
a pending's msg1 index graduates into the session index on completion;
this closes the route-stealing collision. The corollary, stated: **a
datagram that routes by index but fails to open touches nothing** — not
liveness, not roaming, not the replay window (§7.2's decrypt-first
ordering guarantees it). It matters because a freed-then-re-drawn index
makes stale traffic land on the wrong connection routinely; such traffic
is inert.

### 17.4 The static map and the hint set

The `static → connection` map (§16.1's invariant made concrete) plus the
pending tables' dialled addresses (§17.3), which are §6.5's hint set —
the probed set for the internal tie-break is the pending outbound
remotes alone (§6.5). Established connections contribute no hints:
their initiations take the ordinary staged path (§5.4), so the endpoint
tracks no per-connection address (the connection core owns its own
endpoint address, §7.3).

**The replacement basis.** Each entry of the `static → connection` map
carries one further field, `replacement_basis: Option<Timestamp>`,
alongside the connection it names — connection-scoped state, endpoint-held
because the endpoint is where §6.4's admission decides. It is
`Some(t)` where `t` is the timestamp of the msg1 that **established** the
connection while we were the **responder** — a staged `accept()`, a
re-homed `accept()`'s admitted candidate, or the tie-break loser's admit
step (§6.6 step 4) — and `None` when we **dialled** (a `connect()`
completed by msg2, or a tie-break we won: neither teaches us any
timestamp of the peer's, because msg2 carries no payload, §5.2). It is
written once at install, never updated, and dies with the connection; a
replacement installs its own. §6.4's admission is the only reader.
The basis is not the timestamp guard (§17.1): the guard is
endpoint-global, evictable, and orphan-tiered, while the basis lives and
dies with one connection and cannot be flushed by an attacker's
authenticate-then-drop churn. One `Timestamp` per connection
(`TIMESTAMP_LEN`, §5.2), inside §17.5's per-connection term.

### 17.5 State ceilings

The composite bound, in one place, so an application can size its accept
policy:

| State | Ceiling | Worst case |
|---|---|---|
| stage-0 entries + consumed chains | one budget of `INTRO_QUEUE_CAP` (1024) slots | ≈ 220 B raw bytes each, ≈ 225 KB |
| staged mid-states (consumed chains + carried pre-read entries) | ≤ `INTRO_QUEUE_CAP` | ≈ 0.5–1 KB live key material each, ≈ 1 MB — and each holds the endpoint's static provider: for a hardware/enclave static this is up to 1024 concurrent provider handles, an operationally scarce resource the TTL bounds in time |
| timestamp-guard map | `TS_GUARD_ORPHAN_CAP` (1024) orphans + pinned (≤ connections + pendings + mid-states) | ≈ 45 B each |
| established connections | **application-governed — unbounded by the protocol** | per connection, the receive commitment is the advertised credit — ≤ `INITIAL_MAX_DATA` (1 MiB) plus per-stream book-keeping and reassembly metadata bounded by `REASSEMBLY_CHUNKS_MAX` (§10.6 — the second bound is what makes the credit term the dominant term rather than a 25–50× underestimate) — plus the datagram queues (≈ 146 KiB, §11.3), the replay window (256 B), and a sent map bounded by cwnd; the credit term dominates |

## 18. Errors and observability *(DRAFT 2026/08/13)*

### 18.1 The error taxonomy

Closed and normative: every variant this specification names appears here
exactly once; `Superseded` appears nowhere (§6.3), and
`AcceptError::AlreadyConnected` appears nowhere — a proven-LIVE
`accept()` is a replacement (§6.4), so the variant is unreachable and
deleted.

- **`ConnectError::{AlreadyConnected, TimedOut}`** —
  `AlreadyConnected`: §16.1 — returned by `connect()` itself, and also
  the resolution of an in-flight `Connecting` cancelled by a racing
  `accept()` on the same proven static (§6.4's PENDING branch);
  `TimedOut`: initial-connect give-up at
  `HANDSHAKE_GIVEUP` (§5.5).
- **`IntroError::{Expired, Internal, Malformed, EndpointDropped}`** —
  `Expired`: the parked entry outlived `INTRO_TTL` (§6.3); `Internal`:
  the §6.5 interception — the initiation belonged to a pending outbound
  remote (a simultaneous open) and
  was consumed by the endpoint; the application learns no identity;
  `Malformed`: the msg1 read fails structurally; `EndpointDropped`: the
  driver stopped mid round-trip.
- **`AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}`** —
  `Replay`: the automatic guard failure (§17.1); `HandshakeFailed`: the
  tail-tag death of a forged claim — **the only variant in the staged
  taxonomy that is a security signal**; the rest are liveness and
  lifecycle; `Expired`/`EndpointDropped` as above, at this stage.
- **`AcceptError::{Stale, EndpointDropped}`** —
  `Stale`: no initiation for the proven static is parked, **or** the
  admitted candidate fails the replacement-basis rule — the live
  connection's basis is `None`, or the candidate is not strictly newer
  than it (§6.4, §17.4; the
  application SHOULD re-accept on the peer's next `Intro`).
  There is no `Expired` here: a chain's age never fails an `accept()`.
  There is no `AlreadyConnected`: a basis-passing proven-LIVE `accept()`
  is a replacement, and a PENDING static's `accept()` cancels the pending
  rather than failing (§6.4).
- **`ConnectionLost::{TimedOut, NonceExhausted,
  LocallyClosed, PeerClosed { code, reason },
  ProtocolViolation { code }, Replaced,
  EndpointDropped}`** — the teardown matrix (§15.4) maps each to its
  cause; `PeerClosed` carries the peer's CLOSE payload;
  `ProtocolViolation { code }` is the violation-triggered teardown
  (§15.2) — with `AuthError::HandshakeFailed`, the second security
  signal in the taxonomy, kept distinct from `LocallyClosed` because
  peer misbehaviour and application intent demand opposite operational
  responses; `Replaced` is the replacement teardown, fired at the
  replacing `accept()` (§5.4, §6.4);
  `EndpointDropped` is the surviving-handle observation of a stopped
  driver.
- **`WriteError::{Reset(u64), ConnectionLost(ConnectionLost),
  Finished}`** — `Reset`: the stream was reset — by the local
  application, or by the peer's message-mode overflow reset (§9.8, the
  one receiver-emitted RESET_STREAM);
  `Finished`: write-after-finish. (No `Stopped`: STOP_SENDING is
  deferred, §9.9; the variant is reserved for that round.)
- **`ReadError::{Reset(u64), ConnectionLost(ConnectionLost)}`** —
  `Reset(code)`: the peer's RESET_STREAM (§9.6).
- **`MessageError::{TooLarge, ConnectionLost(ConnectionLost)}`** —
  `TooLarge`: payload > `MESSAGE_RECV_MAX` at the handle (§9.8).
- **`DatagramError::{TooLarge, ConnectionLost(ConnectionLost)}`** —
  `TooLarge`: payload > `MAX_DATAGRAM_PAYLOAD` at the handle (§11.4).

The wire error codes (`0x00`–`0x05`, ≥ `0x10` application) are §15.3's
registry.

### 18.2 Trace targets — the operator contract

| Target | Carries |
|---|---|
| `slither::policy` | guard rejections, internal tie-break outcomes (admissions, tag deaths, winner-side drops) |
| `slither::replay` | replay-window rejections |
| `slither::frames` | the frame layer's violation CLOSEs (post-AEAD structural failures and semantic violations, §8.2) and the **datagram queue-overflow drop counters** (§11.5) |
| `slither::roam` | endpoint moves (§7.3) |

The targets are operator-visible contract: renaming or dropping one is a
protocol revision.

## 19. Out of scope and deferred *(DRAFT 2026/08/13)*

**[RATIFIED 2026/08/14]** The deferrals below are ratified as a block; each names
its future home.

| Deferred | Pointer |
|---|---|
| STOP_SENDING | frame `0x05` (§8.3, §9.9); `WriteError::Stopped` reserved with it |
| the BLOCKED family (`DATA_BLOCKED`, `STREAM_DATA_BLOCKED`, `STREAMS_BLOCKED`) | pure diagnostics; QUIC types `0x14`–`0x17` if adopted (§10) |
| pacing | needs sub-RTT shell wakeups; revisit with a finer event loop (§14.7) |
| ECN | wire (ACK ECN counts) + socket plumbing (§14.7) |
| CUBIC / BBR | pure additions behind the `Controller` trait (§14.1) |
| header protection | second per-packet crypto pass outside hiss; metadata hardening axis (§3.4) |
| packet-number truncation | the per-packet-overhead lever; safe against hiss's commit-and-cap, couples to the replay-window width (§3.4, §7.2) |
| PMTUD | `MAX_DATAGRAM` is fixed at 1200 (§3.5) |
| cookies / mac2 | packet type `0x05`; WireGuard's under-load model is the template; answers §6.3's occupancy exposure, §6.5's hint-set spoof, and §6.9's ungated eager-`es` rate (§4.3) |
| PATH_CHALLENGE / PATH_RESPONSE | the QUIC-faithful upgrade of §7.3's anti-amplification budget: explicit address validation before roam commit; two frame types from the reserved space plus a second reset seam alongside the roam seam (§7.3, §14.6) |
| the range-tracker ACK | decouples ACK fidelity from the replay window; wire-compatible (§7.2, §12.2) |
| per-peer `ss` precomputation | needs a hiss seam or a bounded memoising provider; re-opens the DH-cost table (§6.1) |
| persistence | nothing in this specification survives a process restart by design (§5.4: restart is a replacement or a fresh accept, never a merge) |
| reflector / mDNS / probe ping-pong | discovery and hole-punching; packet types would come from the reserved space (§3.1) |
| PSK patterns | IK is the only pattern (§2.2) |
| bubble integration | slither stays independent: zero `bubble-*` dependencies |

## Appendix A — hiss dependencies *(DRAFT 2026/08/13; non-normative)*

This specification gates on **one hiss 0.3.x minor** carrying its
additions — A.1 is the one hard gate; A.2 has a conformant interim, and
A.3 reduces to documentation — verified against released hiss 0.3.1 (the
local tree is functionally identical to the `v0.3.1` tag —
research-gaps.md 2026/08/13, clean tree, two metadata-only commits
ahead). This appendix **reconciles against the shipped hiss API** when
the in-flight hiss work (being pursued jointly, §1.3) lands.

**A.1 The split msg1 read** — `read_message_1_intro` → a `Mid` state →
`complete()`, with the un-read tail carried as an owned array inside the
mid state (option (a); no lifetime, no re-supply). Verified absent from
0.3.1: a repo-wide search finds zero matches for
`MidRead`/`read_message_1_intro`/`Mid::complete` in any source or doc
file, hiss's own TODO schedules nothing of the kind, and the codegen's
read surface is exactly three styles (`Plain`, `Lookup`, `Verify` —
`hiss-macros/src/codegen.rs:930-943`) with at most two generated methods
per message (`codegen.rs:958-984`). Required because **the `Claimed` stage
must *suspend*, not merely decide**: `Claimed` is an app-held object
(human-in-the-loop rejection is a motivating use case), parked across
event-loop turns. **[MAINTAINER]** The shipped `read_message_1_with`
Verify closure — which does hand the claimed static to a closure between
`es` and `ss` (`codegen.rs:939-956`) — is **explicitly rejected as a
fallback**: the closure decides synchronously inside the read, so a
`Claimed` built on it would re-pay `es` at `authenticate()` — a 3-DH
accepted read that distorts the 1/2/4 DH-cost ladder the staged accept is
built on (§6.1) — and there is **no fallback path in this specification
or in the code**: fallback complexity would be spent on a path intended
for deletion.

The `Mid` surface, shaped: `Mid` exposes the **claimed static** (read at
`es` — serving both `read_identity()` and §6.5's eager inspection), and
`complete()` performs `ss` and returns the decrypted 12-byte msg1
payload alongside the responder state — the payload is where §5.2's
timestamp comes from.

**A.2 `DatagramSend::next_counter()`** — a runtime accessor exposing the
counter the next seal will use. Required because the Data header is the
AEAD associated data and must be built **before** sealing (§3.4), and
0.3.1 exposes no counter accessor (verified: zero matches for
`next_counter`/`fn counter` anywhere, including `src/noise/datagram.rs`).
The normative statement is behavioural only — the header carries exactly
the counter the seal used — but the accessor is what makes an
implementation not mirror hiss-owned state. Until it ships, the §3.4
mirror-and-assert interim is **conformant**: mirror the expected counter,
`debug_assert_eq!` it against each seal's returned value, use the
mirrored value only for the AD, and never feed it back to hiss.

**A.3 The canonical encoding — doc-only.** 0.3.1 bounds the associated
type `Clone` only (`src/curve/mod.rs:69`; there is no encoder anywhere
in the trait — `public_key_from_bytes` is decode-only). The
canonical-encoding ruling (§2.4) needs exactly one bound,
`AsRef<[u8]>`, and slither expresses it as its own
`where C::PublicKey: AsRef<[u8]>` clause, which compiles against 0.3.1
unchanged — **the `where` clause is the permanent mechanism, not an
interim**, so no hiss trait change is required. `Ord` is **not**
required: the §6.7 tie-break compares the equal-length `as_ref()` octets
directly as unsigned octet strings. What is requested from hiss is
**doc-only**: a stability promise that each curve's stored `as_ref()`
form is canonical and will not change (P-256 already stores the
normalised 65-byte uncompressed form — `src/curve/p256/mod.rs:145-150`),
since mac1 keying, the tie-break, and the golden vectors all pin it.

**Non-item, no longer needed — the msg2 payload.** The superseded
draft's msg2 `[1]` payload would have needed no hiss change (released
0.3.1 supports a `[N]` payload on the final message —
`hiss-macros/src/parse.rs:262-282`, `codegen.rs:1166-1198`; hiss's own
Cacophony suite exercises IK with `[16] … [15]` payloads). The
ratchet-only ruling deleted the msg2 payload entirely (§5.2), so nothing
is needed at all; the item is retained only so the walkthrough's
verification work is not re-litigated.

**Wart, carried for the hiss round:** `SymmetricState::drop`'s comment
promises to zero `h` "for defence in depth" but the code does not
(`src/noise/symmetric_state.rs:202-208`; `ck` and the cipher keys do
self-scrub). Pre-existing, affects every dropped handshake, and `h` is
not secret — worth a one-line hiss fix alongside A.1–A.3.

## Appendix B — test obligations *(DRAFT 2026/08/13; non-normative)*

The obligations this specification's normative clauses demand. All flow
tests run two endpoints over `testutil::FlakyWire` on tokio's paused
clock (§16.10); no test sleeps.

**Wire pins.**
- Golden vectors frozen **for the first time** at wire version 1 (the
  prologue, the 12-byte msg1 payload, mac1's canonical keying — §1.3),
  then held byte-identical.
- Compile-time size/constant asserts for every §3/§2.3 length and every
  frame-layout constant.
- DH-cost pins: 1 DH per rejected probe, 2 per authenticate, 4 total per
  accept (§6.1).

**Handshake and routing.**
- The staged-accept queue obligations (§6.3, carried): cap; addr-only
  dedup with replace-with-newest and one surfacing per source;
  evict-oldest overflow; the per-source cap counting consumed +
  unconsumed (`read_identity()` net-zero); TTL expiry;
  own-bytes-on-consume (a consumed chain is unsupersedable; a
  post-consumption initiation parks fresh); **freeze-on-carry** (a
  mid-state-carrying eager-demoted entry is
  consumed on arrival: a later mac1-valid same-source packet parks
  separately and can never straddle the frozen entry's accessors).
- Hint routing: eager tie-break entry; unknown-claim demotion with the
  carried mid-state (1 DH total for the class, `read_identity()` at 0
  incremental); interception → `IntroError::Internal`.
- **The local-state routing** (§5.4), over all three states: LIVE ⇒ the
  initiation parks as an ordinary `Intro`, the live connection untouched
  until the replacing `accept()` fires `Replaced` (a withheld or
  replayed initiation left unaccepted destroys nothing); PENDING ⇒ the
  internal tie-break, no
  teardown either way (§6.6–§6.7); NONE ⇒ the ordinary staged accept.
  Restart end-to-end: a restarted peer's reconnect replaces via
  `accept()`, and its zombie counterpart dies at the replacement or at
  liveness — no state merge is representable (§5.4, §6.8).
- **Replay against a live connection — both cases, separately pinned**
  (§6.4, §17.1, §17.4): an **older-or-equal** replay dies at the
  timestamp guard and never surfaces to the application; a
  **withheld-newer** replay — a genuine retransmit captured off the wire
  and injected later — passes the guard, surfaces as an `Intro`,
  authenticates, and **destroys nothing until it is accepted**. The
  second case is the one the guard has never barred; the test must
  exercise it explicitly and not stand on the first.
- **The replacement basis** (§17.4): a connection we **dialled** carries
  basis `None`, and a proven-LIVE `accept()` against it returns
  `AcceptError::Stale` with the live connection untouched — the
  captured-msg1 injection test, run against a connection established by
  `connect()`; a connection we **accepted** carries `Some(t)`, a
  candidate with timestamp ≤ `t` returns `Stale`, and a strictly greater
  one replaces. The tie-break winner records the loser's timestamp in the
  guard (§6.7) and keeps basis `None`; the loser installs with `Some(t)`.
- **The PENDING branch at `accept()`** (§6.4): `read_identity()` →
  `connect()` → `accept()` on one static yields **exactly one**
  connection — the pending and its index are dropped, its `Connecting`
  resolves `Err(ConnectError::AlreadyConnected)`, the accept installs as
  responder, and no tie-break runs.
- Tie-break: both static orderings from both ends converging on one
  shared session; forgery-cannot-cancel (a forged claim of the dialled
  static dies at the tag with the pending untouched); **stream-ID parity
  fixed by the tie-break at establishment** (§6.7, §9.1).
- `accept()` re-home: fast path; newest-first re-home admission;
  `Stale`; the proven-LIVE replacement (`Replaced` fires exactly at the
  replacing `accept()`, never earlier); one completion attempt per
  interval (a forged msg2 spends it; the next scheduled retransmit
  refreshes it); `ConnectError::AlreadyConnected` at `connect()` (no
  `AcceptError` counterpart exists — §18.1).
- Guard pinning,
  no-orphan-on-reject, orphan timer aging, admission-only LRU refresh;
  index re-draw across both tables.

**Frame layer.**
- Varint round-trips, boundary values, non-minimal-encoding acceptance
  (§8.1).
- Frame-table round-trips for every frame; unknown-type and every
  post-AEAD structural-failure case ⇒ nothing from the packet applied,
  CLOSE with `PROTOCOL_VIOLATION`, and
  `ConnectionLost::ProtocolViolation { code }` surfaced (§8.2); pre-AEAD
  gate failures stay silent (§3.1).
- Packing order and the one-extends-to-end-frame rule (§8.5).

**Streams, flow control, messages, datagrams.**
- Stream reassembly under reordering, overlap, and duplication; FIN
  final-size pinning; every `FINAL_SIZE_ERROR` case (§9.5, §9.6).
- **The closed-stream tombstone** (§9.2): free a stream (read-to-final
  and sugar-surfaced), drop the ACK, and let the peer's PTO
  retransmission re-name it — no re-open, no phantom `StreamOpened`, no
  second surfacing of the same message; the watermark holds for the
  connection's life (§9.2).
- Flow-control stall-and-resume at both levels; the §10.3 re-grant
  formula (MAX_STREAM_DATA/MAX_DATA emitted exactly when the read offset
  advances ≥ WINDOW/2 past the last advertisement); violation ⇒ CLOSE
  with `FLOW_CONTROL_ERROR` (§10).
- **Discard-credit** (§10.3): abandoned handles, observed resets, and
  sugar-surfaced streams true up connection credit — a stream-cancelling
  application never wedges MAX_DATA; the §8.4 bound check rejects a
  `final_size` beyond the advertised limit *before* any true-up (no
  credit inflation, no `u64` wrap).
- **The reassembly-fragment bound** (§10.6): a one-byte-frames-at-
  even-offsets flood stays O(credit) or dies at `REASSEMBLY_CHUNKS_MAX`
  with `PROTOCOL_VIOLATION`; the defragmentation cost is measured by the
  throughput gate below.
- MAX_STREAMS replenishment (batching at 8, low-allowance emission,
  peer-opened streams only) and
  `STREAM_LIMIT_ERROR` (§10.4).
- Message sugar: exactly-once surfacing, the 256 KiB bound at the handle,
  GC after full ACK (§9.8); the overflow reset (a FIN-less
  window-filling uni stream under a pending `recv_message()` is reset,
  the sender surfaces `WriteError::Reset`, connection credit trues up);
  the lost-reset case (the receiver-emitted RESET_STREAM is regenerated
  until acknowledged — dropping its first transmission still frees the
  sender, §9.6, §8.7).
- DATAGRAM: no retransmission on loss; queue-overflow drop-oldest with
  newest accepted; the drop counter emitted on `slither::frames` (§11).

**ACK, recovery, congestion.**
- Delayed-ACK policy timing: every-2nd, the 25 ms timer, immediate on
  gap (§12.4).
- Window-2048 admit/duplicate/edge cases; ACK truncation newest-first at
  the cap and at packet capacity; over-cap received ACK ⇒ structural
  failure (CLOSE with `PROTOCOL_VIOLATION`, §8.2);
  above-highest-sealed ⇒ frame ignored whole (§7.2, §12).
- Loss/PTO staircase on the paused clock (packet threshold, time
  threshold, PTO doubling and cap, reset-on-ack) (§13).
- **PTO-disarm** (§13.3): an idle connection with an empty sent map arms
  no `Pto` — no self-sustaining PING train; the timer re-arms with the
  next ack-eliciting send.
- NewReno: slow start, congestion avoidance (ABC), recovery-period
  one-cut, **no cwnd growth from ACKs of pre-recovery-period packets**
  (§14.3), persistent congestion (3× the un-backed-off PTO —
  `pto_count = 0` — with the RTT-sample precondition)
  (§14.2–14.4).
- The cwnd admission gate incl. the PTO-probe exemption and
  non-ack-eliciting exemption (§14.5).
- **One session per connection** (§7.8): a completed replacement
  handshake installs a fresh connection carrying nothing — fresh
  streams, credit, recovery, counters — and the old connection dies
  whole with `Replaced` at the replacing `accept()`.
- Roaming: CC reset with the pre-roam flight fenced (old-path losses
  fire no congestion event and no persistent-congestion collapse; the
  flight still resolves for retransmission), sent map kept, RTT kept as
  a prior with `min_rtt` re-seeded (§13.1, §13.6, §14.6).

**Liveness and amplification.**
- **The liveness anchor** (§7.4): a sender writing into a black hole
  dies at `DEAD_TIMEOUT` after its last authenticated receive — marking
  sends do not re-arm the deadline; a healthy receiving session never
  dies; the `PERSISTENT_KEEPALIVE < DEAD_TIMEOUT` rejection at the
  handle (§7.5); an idle keepalive-sustained connection lives
  indefinitely with no handshake ever re-run (§5.4, §7.5).
- **The anti-amplification budget** (§7.3): a roam or msg1-source anchor
  caps all output — PTO probes, pure ACKs, CLOSE, and keepalives
  included — at 3× authenticated bytes received until traffic validates
  the address; a
  genuine roam clears the budget within ~1 RTT; a silent address dies by
  liveness having received at most 3× what it sent.

**CLOSE.**
- Linger semantics: one CLOSE emitted, ≤ 1 reply/s under inbound flood —
  replies only to authenticated, window-fresh inbound, sent to the
  session address — state dropped at 5 s; receive side surfaces
  `PeerClosed` and never replies (§15.2).
- Mutual close: a CLOSE received while closing drains reply-free — no
  1 Hz ping-pong (§15.2).
- Violation ⇒ CLOSE with the matching registry code and
  `ConnectionLost::ProtocolViolation { code }` locally (§15.2, §15.3).

**Post-implementation validation obligations (gates on the flagged
rulings).**
- **The ACK-loss-burst simulation** (the §7.2/D-5 gate): FlakyWire on
  the paused clock, sustained ACK-loss bursts against the 2048-bit fused
  window under the every-2nd ACK policy, quantifying spurious-retransmit
  and false-congestion-event rates. If the numbers disappoint, the
  range-tracker ACK (§19) is the ready remedy — before ratification
  hardens the fused choice.
- **The window-constants throughput sanity check** (the §10.2 and §10.6
  gate): bulk-transfer throughput over the same FlakyWire topology MUST
  be **within 20 % of quinn under its shipped defaults**, with no stall,
  measured with the §10.6 reassembly bound active (the
  defragmentation/coalescing cost is part of the number) — before the
  §10.2 constants and `REASSEMBLY_CHUNKS_MAX` ratify.

## Named constants *(consolidated; reference suite where suite-dependent)*

| Constant | Value | Home |
|---|---|---|
| `VERSION` | 0x01 | §3.1 |
| `PROLOGUE` | `b"slither\x01"` | §5.1 |
| `PKT_HANDSHAKE_INIT` / `PKT_HANDSHAKE_RESP` / `PKT_DATA` | 0x01 / 0x02 / 0x03 | §3.1 |
| reserved packet types | 0x04 (unused), 0x05 (cookie/mac2) | §3.1 |
| `INIT_HEADER_LEN` / `RESP_HEADER_LEN` / `DATA_HEADER_LEN` | 6 / 10 / 14 B | §3.2–3.4 |
| `MAC1_LABEL` / `MAC1_LEN` | `b"slither mac1"` / 16 B | §4.1 |
| `TIMESTAMP_LEN` / `MSG1_PAYLOAD_LEN` | 12 / 12 B (msg2 has no payload) | §5.2 |
| `IK_MSG1_LEN` / `IK_MSG2_LEN` | 174 / 81 B | §2.3 |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` | 196 / 107 B | §2.3 |
| `AEAD_TAG_LEN` | 16 B | §2.3 |
| `MAX_DATAGRAM` / `MAX_PLAINTEXT` | 1200 / 1170 B | §3.5 |
| `REKEY_EPOCH_MSGS` / `MAX_EPOCH_JUMP` | 65 536 / 2 (hiss-fixed) | §7.7 |
| `REPLAY_WINDOW` | 2048 bits | §7.2 |
| frame types | 0x00, 0x01, 0x02, 0x04, 0x08–0x0f, 0x10–0x13, 0x1c, 0x30/0x31; 0x05 reserved | §8.3 |
| `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` | 0x04 / 0x02 / 0x01 | §8.4 |
| `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) | §10.2 |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) | §10.2 |
| `INITIAL_MAX_STREAMS_BIDI` / `_UNI` | 32 / 128 (cumulative) | §10.2 |
| `STREAMS_CREDIT_BATCH` | 8 | §10.4 |
| credit re-grant threshold | ½ window consumed | §10.3 |
| `MESSAGE_RECV_MAX` | = `INITIAL_MAX_STREAM_DATA` | §9.8 |
| `MAX_DATAGRAM_PAYLOAD` | 1169 B (= `MAX_PLAINTEXT` − 1) | §11.2 |
| `DATAGRAM_SEND_QUEUE` / `DATAGRAM_RECV_QUEUE` | 64 / 64 (count; drop-oldest, newest always accepted; ≈ 73 KiB worst case each) | §11.3 |
| `REASSEMBLY_CHUNKS_MAX` | 1024 stored discontiguous ranges per stream | §10.6 |
| `CLOSE_REASON_MAX` | 256 B | §8.4 |
| `CLOSE_LINGER` / close-reply rate | 5 s / ≤ 1 per s | §15.1 |
| `MAX_ACK_RANGES` | 64 | §12.2 |
| ACK policy | every 2nd ack-eliciting, `MAX_ACK_DELAY` cap, immediate on gap | §12.4 |
| `MAX_ACK_DELAY` | 25 ms | §12.4 / §13.3 |
| `K_PACKET_THRESHOLD` / time threshold / `K_GRANULARITY` | 3 / 9⁄8 / 1 ms | §13.2 |
| `K_INITIAL_RTT` / `PTO_BACKOFF_CAP` | 333 ms / 2⁶ | §13.1 / §13.3 |
| `INITIAL_WINDOW` / `MINIMUM_WINDOW` | 12 000 / 2 400 B | §14.2 |
| `LOSS_REDUCTION_FACTOR` / `PERSISTENT_CONGESTION_THRESHOLD` | 0.5 / 3 | §14.2 / §14.4 |
| `RETRANSMIT_BASE` / `RETRANSMIT_JITTER_MAX` | 5 s / 333 ms | §5.5 |
| `HANDSHAKE_GIVEUP` | 90 s | §5.5 |
| `KEEPALIVE_TIMEOUT` / `DEAD_TIMEOUT` | 10 s / 15 s | §7.5 |
| `PERSISTENT_KEEPALIVE` (default, per-connection) | 25 s; handle-rejected below `DEAD_TIMEOUT` (the liveness floor) | §7.5 |
| `AMPLIFICATION_FACTOR` | 3 (× authenticated bytes received, per unvalidated address) | §7.3 |
| `INTRO_QUEUE_CAP` / `INTRO_MAX_PER_SOURCE` / `INTRO_TTL` | 1024 / 4 / 15 s | §6.3 |
| `TS_GUARD_ORPHAN_CAP` | 1024 | §17.1 |
| `L` (shell lateness bound) | 250 ms | §16.5 |
| wire error codes | 0x00–0x05 + ≥ 0x10 application | §15.3 |
| session index | nonzero u32, random, re-drawn across both tables | §17.3 |

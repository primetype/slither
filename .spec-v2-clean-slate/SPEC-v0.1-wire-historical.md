# slither — protocol specification

> **RATIFIED 2026/07/16 (the maintainer).** Every constant, label, timer, and
> byte layout below is frozen as the slither v1 wire; the code must match this
> file, and a change to either is a protocol revision, not an edit. British
> English, Oxford comma, dates `YYYY/MM/DD`.

slither is a WireGuard-shaped Noise-over-UDP packet layer: an authenticated,
encrypted, **unreliable** datagram session between two peers. It borrows
WireGuard's homework — a cheap mac1 DoS gate, fresh-ephemeral handshake
retransmission, an anti-replay sliding window, endpoint roaming, and the
keepalive/liveness/rekey timers — over Bubble's cryptography (the `hiss` Noise
**IK** handshake and its datagram transport; keyed BLAKE2b from `cryptoxide`).

Leg 1 is the **packet layer only**. Frames, ACKs, and reliability ride **on top**
of this in Leg 2 — built, ratified, and specified in §9; §§1–8 remain the
ratified Leg 1 wire, untouched.

## 1. Crypto suite

| Element | Value |
|---|---|
| Noise protocol | `Noise_IK_P256_ChaChaPoly_BLAKE2b` (`hiss`, plain IK — no PSK) |
| Curve | P-256 (secp256r1) |
| AEAD | ChaCha20-Poly1305 (16-byte tag) |
| Hash | BLAKE2b |
| Handshake ephemeral / static on the wire | 65-byte uncompressed SEC1 point |
| Static public key (addressing, mac1 keying) | 33-byte compressed SEC1 (`hiss` `Packed` encoding) |
| Handshake payload | msg1 carries the 12-byte initiation timestamp, encrypted (see §5); msg2 carries the empty payload |
| Datagram transport | `hiss` `DatagramSend`/`DatagramRecv` (explicit per-packet counter) |

## 2. Packet types and version

| Constant | Value | Meaning |
|---|---|---|
| `VERSION` | `0x01` | protocol version byte; unknown ⇒ silent drop |
| `TYPE_HANDSHAKE_INIT` | `0x01` | initiator's IK msg1 |
| `TYPE_HANDSHAKE_RESP` | `0x02` | responder's IK msg2 |
| `TYPE_DATA` | `0x03` | sealed transport datagram |
| `0x04` | reserved | close — never emitted; silently dropped |
| `0x05` | reserved | cookie reply / mac2 — never emitted; silently dropped |
| `0x06` | reserved | probe ping (future hole-punch) — never emitted; dropped |
| `0x07` | reserved | probe pong (future hole-punch) — never emitted; dropped |
| `0x08..` | reserved | future — silently dropped |

Every packet opens with `type: u8, version: u8`. **All multi-byte integers are
big-endian.** (packtool packs raw integers little-endian, so slither's header
fields are `[u8; N]` byte arrays filled with `to_be_bytes`.)

## 3. Wire layouts

Sizes are for the P-256 suite. `IK msg1` = `e(65) ‖ enc_s(65+16) ‖
enc_ts(12+16)` = **174** (the tail is the encrypted 12-byte timestamp payload
plus its AEAD tag); `IK msg2` = `e(65) ‖ tag(16)` = **81**.

### HandshakeInit (`0x01`) — 196 bytes

```
type(1) ‖ version(1) ‖ sender_index(4)                              ← InitHeader, 6 B
        ‖ hiss IK msg1(174) ‖ mac1(16)
```

- `sender_index` — the initiator's random nonzero `u32` session index.
- The 12-byte initiation timestamp rides **encrypted inside msg1's tail** (§5);
  it does not appear in the header.
- `mac1` — keyed on the **responder's** static (the recipient); §4. Its
  preimage is all packet bytes preceding the tag, which now include msg1's
  payload ciphertext.

### HandshakeResp (`0x02`) — 107 bytes

```
type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)          ← RespHeader, 10 B
        ‖ hiss IK msg2(81) ‖ mac1(16)
```

- `sender_index` — the responder's random nonzero `u32` index.
- `receiver_index` — the initiator's index this response answers.
- `mac1` — keyed on the **initiator's** static (the recipient); §4.

### Data (`0x03`) — 14-byte header ‖ ciphertext

```
type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)              ← DataHeader, 14 B
        ‖ ciphertext(plaintext + 16)
```

- The **14 header bytes are the AEAD associated data.**
- `receiver_index` — the recipient's session index (routes the packet to a
  session).
- `counter` — exactly the value `DatagramSend::encrypt_next` returned (the
  hiss-owned monotonic send counter).
- An **empty plaintext** (16-byte tag-only ciphertext, 30-byte datagram) is the
  **keepalive**.

### Sizes and caps

| Constant | Value |
|---|---|
| `InitHeader::SIZE` | 6 |
| `RespHeader::SIZE` | 10 |
| `DataHeader::SIZE` | 14 |
| `TIMESTAMP_LEN` | 12 |
| `IK_MSG1_LEN` | 174 |
| `IK_MSG2_LEN` | 81 |
| `INIT_PACKET_LEN` | 196 |
| `RESP_PACKET_LEN` | 107 |
| `AEAD_TAG_LEN` | 16 |
| `MAC1_LEN` | 16 |
| `MAX_DATAGRAM` | 1200 |
| `MAX_PLAINTEXT` | 1170 (`MAX_DATAGRAM − 14 − 16`) |
| session index | random nonzero `u32` (regenerated on `0`) |

`INIT_PACKET_LEN` is unchanged at 196 across the Leg 1b move: the header shed
the 12 timestamp bytes and msg1's tail gained exactly 12 bytes of ciphertext.

Oversize send is a typed error ([`PayloadTooLarge`]); oversize receive
(`> MAX_DATAGRAM`) is a silent drop.

## 4. mac1 — the DoS gate

```
key = BLAKE2b-256(MAC1_LABEL ‖ recipient_static_pub_compressed[33])
mac1 = keyed-BLAKE2b-128(key, all packet bytes preceding the tag)
```

| Constant | Value |
|---|---|
| `MAC1_LABEL` | `b"slither mac1"` |

- The **recipient's** static public key keys the tag (its 33-byte compressed
  SEC1 = `hiss` `Packed` encoding). On a HandshakeInit the recipient is the
  responder; on a HandshakeResp the recipient is the initiator.
- mac1 is verified **before any curve or DH work** — a garbage flood or a
  wrong-key packet dies here, never reaching the DH provider.
- mac1 is **not** a secret authenticator (its key is public); it is an
  anti-amplification / cheap-reject gate. The real authentication is the Noise
  handshake underneath. Cookie/mac2 (`0x05`) against a flood of *valid-looking*
  packets is reserved for a future leg.

## 5. Handshake behaviour

### Prologue and the initiation timestamp

```
PROLOGUE = b"slither\x01"                      (fixed; identical for every handshake)
msg1 payload = ts_secs(8, BE) ‖ ts_nanos(4, BE)  (12 bytes, encrypted in msg1's tail)
```

WireGuard puts a monotonic timestamp inside the encrypted handshake-init payload
to defeat initiation replay, and slither does the same (ruled 2026/07/16, fork
B): the 12-byte timestamp is msg1's Noise **application payload**, encrypted
into the message tail after the `e, es, s, ss` tokens, alongside the tail's AEAD
tag. The prologue is the fixed `PROLOGUE` constant.

**The exact confidentiality guarantee.** The msg1 payload has Noise
**confidentiality level 2**: it is encrypted to the responder's static key, so
it is opaque to any passive observer — this kills the passive clock-skew
fingerprint the old cleartext-header timestamp exposed — and it is
authenticated (a tampered ciphertext fails the tail's AEAD tag, failing the
handshake) and replay-ordered (§ responder, step 4). It is **not**
forward-secret against a later compromise of the responder's static key —
acceptable, because the plaintext is a wall-clock reading, not a secret.

The timestamp is the wall clock (`secs_since_unix_epoch ‖ nanos`), forced
strictly greater than the previous one this endpoint emitted (so a retransmit is
always admitted even when the coarse clock has not advanced).

### Initiator

1. Draw a random nonzero `sender_index` and a fresh timestamp; supply the
   timestamp as msg1's declared 12-byte application payload (the pattern's
   `[12]` suffix), build msg1 over a fresh ephemeral provider (the
   `hiss::noise!`-generated sans-io machine — msg1 is a fixed-size byte
   array, no I/O), append mac1, send.
2. Arm a retransmit at **`RETRANSMIT_BASE` + uniform jitter ≤ `RETRANSMIT_JITTER_MAX`**.
   **Each retransmit is a completely fresh initiation** — new ephemerals, new
   index, new (strictly greater) timestamp.
3. On a length-correct, index-matching, mac1-valid HandshakeResp: complete
   the Noise read (`<- e, ee, se`), `Transport::into_datagram()`, session
   live. Our receiver index = our `sender_index`; the peer's index = the
   response's `sender_index`.
4. Give up after **`HANDSHAKE_GIVEUP`** (a typed `Failed` event).

### Responder

The policy gate sits between `s` and `ss` (ruled 2026/07/16): the responder
reads msg1 through the generated **verification read**
(`read_message_1_with`), whose closure receives the claimed initiator static
the moment it decrypts — so a mac1-valid but unlisted initiator costs the
responder exactly **one** ECDH (`es`) and is dropped silently; only admitted
peers pay the `ss` DH and the timestamp decryption.

1. mac1 gate (keyed on our own static) — **before any DH**.
2. **The verification read**: drive `<- e, es, s` — `es` is the responder's
   first (and, on a rejection, only) DH; `s` **recovers the claimed**
   initiator static, which the read hands to the policy closure **before**
   `ss` is computed. The claim is not yet proof of possession (hiss's
   documented contract for the `_with` reads): `es` needs only the
   responder's *public* static, so anyone who passes mac1 can present any
   key here.
3. **Allow-list (the policy closure)**: if the claimed static is not
   permitted, the closure rejects (`PeerRejected`) and the read aborts —
   no `ss`, no timestamp decryption, no msg2 (trace-counted).
4. **The admitted tail**: the accepted read continues through `ss` — the
   second DH — closing msg1, **authenticating the claim** (only the holder
   of the claimed static's private key can compute `ss`; the tail's AEAD tag
   verifies it), and decrypting the timestamp payload (a tampered payload
   ciphertext fails that tag here — a failed handshake, dropped silently
   like any bad msg1) — then **pause** before msg2. A forged claim of a
   *listed* static therefore costs the responder both DHs before it dies
   here — WireGuard's exact shape, and, like WireGuard, an accepted
   ~one-ECDH timing difference that a prober could use to test a public
   key's allow-list membership (statics are public data; accepted — ratified
   2026/07/16 with the rest of this file).
5. **Greatest-timestamp guard**: per initiator static, admit only a strictly
   greater timestamp; a replay (equal or lesser) is dropped — no msg2.
6. Draw a random nonzero `sender_index`, write msg2 (`-> e, ee, se`), append mac1
   (keyed on the initiator's static), session live immediately.
7. A retransmitted, newer msg1 from the same static **replaces** the peer's
   session in place (same logical connection).

### Allow-list

The endpoint is otherwise policy-free: the constructor takes the set of permitted
remote statics, with `allow`/`revoke` methods. This is the family-devices set;
the caller owns policy. `connect` (as initiator) is not gated by the allow-list —
the caller chose the target.

## 6. Session behaviour

### Anti-replay window (RFC 6479 shape)

- A greatest-counter (`u64`) plus a **128-bit** sliding bitmap (`REPLAY_WINDOW =
  128`). `bit 0` tracks the greatest counter accepted; `bit k` tracks
  `greatest − k`.
- Checked-and-marked **only after `decrypt_at` authenticates** the packet. A
  fresh counter is admitted and recorded; a duplicate or a counter more than 128
  behind the greatest is dropped **after** decryption **without delivery**.

### Roaming

An **authenticated, fresh** (non-replayed) Data packet whose source differs from
the session's current endpoint moves the endpoint to the new source (an
`EndpointMoved` event). Nothing unauthenticated, and no replayed packet, ever
moves it.

### Timers

| Constant | Value | Meaning |
|---|---|---|
| `RETRANSMIT_BASE` | 5 s | base retransmit delay (fresh initiation) |
| `RETRANSMIT_JITTER_MAX` | 333 ms | uniform jitter added to the base |
| `HANDSHAKE_GIVEUP` | 90 s | connect gives up (typed `Failed`) |
| `KEEPALIVE_TIMEOUT` | 10 s | received-but-not-sent this long ⇒ send empty-Data keepalive |
| `DEAD_TIMEOUT` | 15 s | sent-but-nothing-received this long (since the send) ⇒ session dead |
| `PERSISTENT_KEEPALIVE` | 25 s | optional persistent keepalive (off by default) |
| `REKEY_AGE` | 120 s | on the next send past this age, initiate a fresh handshake |
| `REJECT_AGE` | 180 s | seal/open refused outright; the session is torn down |

- **Keepalive** is passive: a side that has received a data packet and not sent
  since sends an empty-Data keepalive after `KEEPALIVE_TIMEOUT`. This starts the
  10-s keepalive dance that keeps an active session alive.
- **Liveness** (`DEAD_TIMEOUT`) is measured from the last **send**, so the
  keepalive dance (reply up to 10 s later) always lands inside the 15-s window.
- **Rekey** is send-triggered: a session older than `REKEY_AGE` starts a fresh
  handshake on the next send (a silent swap under the same logical connection),
  while the old session keeps working until the new one establishes.
- At `REJECT_AGE` a session may no longer seal **payload**: the payload-path
  backstop tears it down (`Dead`). *(Amended 2026/07/17 under the ratified
  amendment below — the pre-amendment text also refused idle opens and killed
  idle sessions on the tick.)*

> **RATIFIED amendment (the maintainer, 2026/07/17).** The ruling *age gates
> payload, not liveness* narrows the two age timers to the **payload-seal
> path**; the ratified 2026/07/16 timer *values* above are unchanged.
>
> - **The idle age-death is removed.** An idle session no longer dies at
>   `REJECT_AGE`. Liveness (`DEAD_TIMEOUT`) is the sole idle killer: a session
>   sustained by the keepalive dance lives indefinitely. Key-age hygiene is now
>   provided by rotation — the epoch ratchet (see *Key ratchet* below) — not by
>   a forced teardown.
> - **`REKEY_AGE` and `REJECT_AGE` are consulted only where application payload
>   is sealed** — a fresh `send`, and a DATA retransmission (a reliable frame on
>   a fresh counter is payload too). Quiet control — keepalives, ACKs, and PTO
>   probes (the `seal_quiet` family) — is age-exempt, and inbound *opening* is
>   age-exempt, so an over-age idle session keeps opening keepalives.
> - **The `REKEY_AGE` trigger** is unchanged in spirit: the first payload past
>   this age starts a fresh handshake (the silent swap, no `Established`
>   re-emission), the old session sealing until the swap or expiry.
> - **The `REJECT_AGE` backstop** now means: application payload may not be
>   sealed under a session older than 180 s that has failed to complete its DH
>   rekey — the endpoint tears it down at that point (the old behaviour, scoped
>   to the payload path). Liveness (15 s) preempts it in any symmetric
>   partition; the backstop bites only under asymmetric loss — payload path
>   down, liveness path up — where a stuck retransmitting session must still
>   rekey and, failing that, be torn down.

### Key ratchet (Noise §11.3) — ratified 2026/07/17

> **RATIFIED (the maintainer, 2026/07/17).** §§1–8's ratified 2026/07/16 wire is unchanged — the ratchet is
> key-schedule-internal, so no packet byte moves (the golden handshake/packet
> vectors and the DH-cost pins stay green by construction).

slither's transport keys ratchet forward on a counter-derived epoch schedule
(hiss's `Transport::into_datagram_with_epoch`), retiring ageing key material by
rotation rather than by a forced re-handshake — the hygiene the removed idle
age-death used to supply.

| Constant | Value |
|---|---|
| `REKEY_EPOCH_MSGS` | 65 536 (2¹⁶) messages per epoch |

- **The REKEY transform.** Epoch `e`'s key is the Noise §11.3 `Rekey()` applied
  `e` times to the handshake key, where
  `Rekey(k) = ENCRYPT(k, 2⁶⁴ − 1, empty, zeros[32])[0..32]` — the AEAD's own key
  derivation, keyed on `k`, at the reserved nonce `2⁶⁴ − 1`, over 32 zero bytes,
  keeping the first 32 output bytes. The ChaCha20-Poly1305 vector, independently
  recomputed twice in review and pinned by
  `handshake::tests::rekey_transform_kat`:
  `REKEY(0³²) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`.
- **The epoch rule.** A message sealed at `counter` belongs to epoch
  `counter / REKEY_EPOCH_MSGS`; **each direction ratchets independently** on its
  own send counter. The counter is **never reset** (a rekey-by-re-handshake
  begins a fresh session with its own counter space), and `2⁶⁴ − 1` is
  **reserved** for `Rekey()` — no message ever seals under it (sealing refuses
  it, nonce exhaustion).
- **Both ends, same epoch size.** `REKEY_EPOCH_MSGS` is part of the protocol,
  not a tuning knob: both peers pass the identical value or they disagree on
  which key opens which packet, so the initiator and responder transport splits
  both call `into_datagram_with_epoch(REKEY_EPOCH_MSGS)`.
- **Straggler tolerance.** The receiver retains the current **and the
  immediately preceding** epoch key, so a datagram reordered across one boundary
  still opens; a datagram from any older epoch is refused (its key is gone).
- **`MAX_EPOCH_JUMP = 2` (hiss-fixed).** A receiver refuses a counter more than
  two epochs beyond its committed epoch **without deriving any key**, bounding
  the CPU a single forged far-future counter can demand. The liveness
  interaction: a *legitimate* peer more than two epochs ahead would mean a whole
  epoch (65 536 messages) elapsed in silence, which slither's liveness timer
  (`DEAD_TIMEOUT`, 15 s) rules out long before — the receive path never needs to
  chase that far.
- **No healing.** The ratchet is *forward* rotation only, not post-compromise
  security: a `Rekey()` chain is derivable by anyone holding the current epoch
  key, so an attacker who learns a key follows the ratchet forward. Healing —
  recovering secrecy after a key compromise — remains the job of the **DH
  re-handshake** on the payload path (`REKEY_AGE`), which mixes fresh ephemeral
  DH into a brand-new session. The composition: rotation retires key age on idle
  sessions; the DH re-handshake heals under real payload traffic.

## 7. Deviations from the brief (ratified 2026/07/16 with the rest of this file)

- **Timestamp in the encrypted msg1 payload (Leg 1b — the fork-B ruling,
  2026/07/16).** Leg 1 originally folded the timestamp into the prologue and
  carried it in cleartext `InitHeader` fields, because hiss then had no
  handshake-payload API. hiss has since grown one (today the `noise!` macro's
  `[N]` payload suffix, which slither's pattern declares on msg1 — Leg M1;
  the interim sync-driver staging API is gone), and the maintainer ruled that
  the timestamp moves into the encrypted msg1 payload, restoring WireGuard's
  shape (§5). The HandshakeInit layout is now
  `… sender_index ‖ IK-msg1-with-embedded-payload ‖ mac1`; the prologue is the
  fixed constant. The former deviation is resolved.
- **Either side may rekey.** WireGuard rekeys initiator-only; slither v1 lets
  whichever side sends past `REKEY_AGE` initiate — the brief's stated v1
  simplification.
- **A fifth event, `Failed`.** The brief's event set is
  `Established, Incoming, Dead, EndpointMoved`; a handshake give-up needs a
  typed signal through the fire-and-forget handle, so a `Failed { conn, error }`
  event is added.

## 8. Out of scope (Leg 1)

Frames / ACK / reliability (Leg 2); cookies / mac2; probe ping/pong and hole
punching; the island reflector; mDNS discovery; PSK patterns; congestion
control; any `bubble-*` integration; any persistence.

## 9. Leg 2 — the reliable frame layer (RATIFIED 2026/07/17)

> **RATIFIED 2026/07/17 (the maintainer).** Everything in this section (every
> frame type, layout, constant, and behaviour) is frozen as the slither v1
> frame layer; the code must match it. Nothing here alters §§1–8: the frame
> layer occupies the **sealed Data-packet plaintext**, which is invisible to
> the ratified Leg 1 wire — the packet headers, handshake bytes, sizes, mac1,
> replay-window mechanics, timers, and roaming are untouched.

Leg 2 copies QUIC's homework one layer down: frames within the sealed packets,
ACK ranges with an ack_delay, and RFC 9002 loss detection (packet and time
thresholds, PTO with exponential backoff) over the Leg 1 datagram `counter`,
which **is** the packet number — unique, monotonic, never reused, so a
retransmission is never ambiguous (no Karn's problem). **Frames are
retransmitted, never packets.** Deliberately not copied: TLS/QPACK, 0-RTT,
version negotiation, flow control, and congestion control (no cwnd, no
pacing — loss detection drives retransmission only).

### 9.1 Frame types

A sealed packet's plaintext is one or more frames, concatenated; parsing runs
to the end of the plaintext (the AEAD gives the exact length, so there is no
packet-level length prefix). An **empty** plaintext is the Leg 1 keepalive and
bypasses the frame layer entirely.

| Frame | Value | Meaning |
|---|---|---|
| `FRAME_PADDING` | `0x00` | one zero byte, no fields; coalescible, ignorable |
| `FRAME_PING` | `0x01` | no fields; elicits an ACK, nothing else |
| `FRAME_ACK` | `0x02` | ACK ranges + ack_delay (§9.2) |
| `FRAME_DATA` | `0x03` | one reliable application message (§9.3) |
| `0x04..=0x0F` | reserved | the STREAM / file-transfer / close space — never emitted; on receipt the receiver **stops parsing and silently ignores the rest of the packet** (the layout is unknowable), keeping the frames already parsed |
| `0x10..` | unknown | a protocol violation — the whole packet is dropped |

The plaintext was authenticated, so a **malformed frame stream fails the whole
packet** (a truncated frame, an over-cap or underflowing ACK, an unknown
type): it is a protocol violation, dropped with a trace, never a panic.

### 9.2 ACK (`0x02`)

```
type(1) ‖ largest(8, BE) ‖ ack_delay_µs(4, BE) ‖ first_range(2, BE)
        ‖ range_count(1)                                   ← AckHeader, 16 B
        ‖ range_count × [ gap(2, BE) ‖ length(2, BE) ]     ← 4 B each
```

- `largest` — the largest received packet counter; the first block covers
  `largest − first_range ..= largest`.
- Each `(gap, length)` pair descends, QUIC RFC 9000 §19.3.1 semantics with
  fixed widths: with `prev_smallest` the smallest counter of the preceding
  block, the block's largest is `prev_smallest − gap − 2` and it covers
  `largest − length ..= largest` of itself. Ranges that would descend below
  counter zero are a protocol violation.
- `ack_delay_µs` — microseconds between receiving `largest` and sending the
  ACK, measured on tokio's clock (zero when that arrival was not seen at the
  frame layer, e.g. a keepalive's counter).
- **The ACK source is the Leg 1 replay window** (§6): the ACK ranges are built
  directly from its greatest-counter + 128-bit bitmap — one received-packet
  record, not two. An ACK therefore reports the freshest ≤ 128 counters;
  older counters simply stop being acknowledged (the DATA dedup absorbs any
  resulting spurious retransmission).
- **ACK policy: immediate.** An ACK is owed the moment a packet carrying
  ack-eliciting frames (DATA, PING) arrives, and rides the next outgoing
  packet at once, coalesced with whatever else is pending. ACK frames are not
  themselves ack-eliciting (no ACK-of-ACK loops), and pure-ACK packets are
  not tracked for loss — the next ACK re-reports the whole window.
- **Bounded processing.** A received ACK is intersected with the receiver's
  own in-flight set (bounded by the window) rather than expanded into the
  counters its ranges imply — so a wire-legal ACK whose 63 ranges span
  millions of counters costs `O(in-flight × range_count)`, never a
  multi-megabyte materialisation. (A sender ordinarily emits contiguous
  ranges well within the window; the bound defends against an ill-formed or
  hostile — but authenticated — peer.)

### 9.3 DATA (`0x03`)

```
type(1) ‖ seq(8, BE) ‖ length(2, BE)                       ← DataFrameHeader, 11 B
        ‖ message bytes(length)
```

- `seq` — the message's own sequence number, a per-connection space distinct
  from the packet counter: this is the **retransmittable identity** (a
  retransmitted DATA keeps its `seq`, rides a fresh counter) and the
  **exactly-once dedup key** (a duplicate delivery is ACKed again but not
  re-surfaced).
- Delivery is **unordered reliable** (the ruled v1 lean): each DATA frame is
  an independent reliable message — at-least-once on the wire, exactly-once
  surfaced — with no head-of-line blocking and no reassembly buffer. Ordered
  delivery is a STREAM-frame concern, deferred with the reserved space.
- A message larger than [`MAX_MESSAGE`] is rejected at the handle with the
  typed `PayloadTooLarge`; multi-packet messages (fragmentation) are OUT,
  reserved for the STREAM work. A zero-length message is valid.
- Reliability lives **within the connection**: messages not yet delivered
  when the connection dies (`Dead`, `close`, process exit) are lost. A rekey
  does *not* lose messages — the undelivered set re-queues onto the fresh
  session (§9.5).

### 9.4 Loss detection and PTO (RFC 9002, congestion-control-free)

Per connection, ack-eliciting sent packets are tracked (counter, send time,
the DATA seqs aboard). On an ACK: newly-ACKed packets clear their messages;
if the frame's `largest` is newly ACKed, it yields an RTT sample
(`latest_rtt`, `smoothed_rtt`, `rttvar`, `min_rtt` per RFC 9002 §5, ack_delay
subtracted per §5.3); an ACK claiming a counter the session never sealed is a
protocol violation, ignored whole.

A tracked packet is **lost** when a later packet has been ACKed AND it is
either `K_PACKET_THRESHOLD` counters older than the largest ACKed, or older
than the time threshold `9/8 · max(smoothed_rtt, latest_rtt)`; survivors arm
the loss-detection timer. Lost frames re-queue for retransmission in a fresh
packet on a fresh counter.

When ACKs stop, the **PTO** (`smoothed_rtt + max(4·rttvar, kGranularity) +
MAX_ACK_DELAY`, doubling per unanswered probe) retransmits the oldest
undelivered message — or a PING when nothing retransmittable is outstanding —
to elicit an ACK. Both timers are evaluated on the actor's 250 ms TICK.

### 9.5 Interplay with the ratified Leg 1 timers

The §6 timer table stands unchanged. Frame-layer **control packets — pure
ACKs, PTO probes, and retransmissions — are liveness-neutral**: they ride the
identical sealed-Data wire but do not mark the last-send liveness clock
(`seal_quiet`). Otherwise the probe train would indefinitely defer
`DEAD_TIMEOUT` and an immediate ACK would suppress the ruled keepalive dance;
with the exclusion, a partitioned connection still dies exactly 15 s after
its last fresh send, probes notwithstanding. Only fresh application sends and
the Leg 1 keepalive mark the clock. (One accepted redundancy: a pure receiver
keeps keepaliving on schedule even though its ACKs already flow.)

On a **rekey** (or a responder-side session replacement) the counter space
restarts, so the per-epoch recovery state (sent-packet map, ACK schedule)
resets and every undelivered message re-queues on the new session; the
per-connection state (message seqs, receiver dedup, the RTT estimate — a path
property) survives the swap.

### 9.6 Constants (all ratified 2026/07/17)

| Constant | Value | Source |
|---|---|---|
| `FRAME_PADDING` | `0x00` | — |
| `FRAME_PING` | `0x01` | — |
| `FRAME_ACK` | `0x02` | — |
| `FRAME_DATA` | `0x03` | — |
| reserved frame space | `0x04..=0x0F` | STREAM / file transfer / close |
| `AckHeader::SIZE` | 16 | — |
| `AckRangePair::SIZE` | 4 | — |
| `DataFrameHeader::SIZE` (`DATA_OVERHEAD`) | 11 | — |
| `MAX_MESSAGE` | 1159 (`MAX_PLAINTEXT − 11`) | the v1 message cap |
| `MAX_ACK_RANGES` | 63 | the most a 128-counter window can produce; a maximal ACK is 268 B |
| `MAX_ACK_DELAY` | 25 ms | the QUIC default; a budget — the v1 ACK policy is immediate |
| `K_PACKET_THRESHOLD` | 3 | RFC 9002 default |
| time threshold | 9/8 | RFC 9002 default |
| `K_GRANULARITY` | 1 ms | RFC 9002 default (the 250 ms TICK bounds it in practice) |
| `K_INITIAL_RTT` | 333 ms | RFC 9002 default (first PTO ≈ 1 s) |
| `PTO_BACKOFF_CAP` | 2⁶ | local overflow guard; the train ends by `DEAD_TIMEOUT` (symmetric loss) or the `REJECT_AGE` payload backstop (asymmetric loss — amended 2026/07/17) |

### 9.7 Behavioural deltas against Leg 1 (ratified with this section)

- **`send` is now reliable** (unordered, exactly-once, within the
  connection's life) rather than fire-and-forget; `Incoming` surfaces each
  message exactly once, in arrival order.
- **The message cap moved** from `MAX_PLAINTEXT` (1170) to `MAX_MESSAGE`
  (1159): the 11-byte DATA overhead now rides inside the plaintext. A
  max-size message still fills exactly one 1200-byte datagram.
- **An empty `send` is now a real (empty) message** and surfaces as an empty
  `Incoming` payload; under Leg 1 it was indistinguishable from a keepalive
  and silently vanished.

## 10. Out of scope (Leg 2)

Ordered streams and fragmentation (the reserved STREAM space); congestion
control and pacing; cookies / mac2; probe ping/pong and hole punching; the
island reflector; mDNS discovery; PSK patterns; any `bubble-*` integration;
any persistence.

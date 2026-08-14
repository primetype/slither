# Wire reconciliation — a QUIC-shaped wire over hiss's sealed datagrams

Research track for the clean-slate slither redesign. Goal: design the packet
wire so that a QUIC-style ordered-reliable-stream transport rides natively on
`hiss`'s `DatagramSend`/`DatagramRecv` (Noise IK over P-256/ChaChaPoly/BLAKE2b),
with Noise as the crypto instead of TLS. The crypto is fixed; the wire is a
blank sheet. Everything below cites `file:line`. Paths are absolute in the
repos:
`hiss` = `/Users/nicolasdiprima/work/primetype/hiss`,
`slither` = `/Users/nicolasdiprima/work/primetype/slither`.

The single most important finding up front: **hiss's send counter already *is*
a QUIC packet number** in every load-bearing respect — monotonic, never reused,
owned by the crypto layer, and simultaneously the AEAD nonce. The redesign
should lean into this and make the hiss counter the on-wire packet number that
ACKs reference, exactly as the current slither Leg 2 already does (SPEC.md:390).
The value of a clean slate is not to change that decision but to *re-derive* it
deliberately, size the surrounding state (replay window, ACK record, header
bytes) for a high-throughput streaming transport rather than for a keepalive
VPN, and pin every place the wire must bend to a hiss constraint.

---

## 1. hiss's sealed-datagram contract, precisely

The out-of-order transport is `hiss/src/noise/datagram.rs`. A completed Noise
`Transport` is consumed into a `(DatagramSend, DatagramRecv)` pair by either
`into_datagram` (no key ratchet, `datagram.rs:96`) or
`into_datagram_with_epoch` (counter-derived key ratchet, `datagram.rs:133`).
slither uses the epoch variant on both sides (`handshake.rs:348`, `:414`).

**What the send half gives.** `DatagramSend::encrypt_next(ad, plaintext,
output) -> (counter, bytes)` (`datagram.rs:205`). The `counter` it returns is
the explicit nonce the message was sealed under. It is:

- **hiss-owned and strictly monotonic** — `0, 1, 2, …`, taken from the inner
  `CipherState.n` (`cipher_state.rs:133-135`, `:107-108`). The caller is *told*
  the counter but can never *choose* it (`datagram.rs:190-197`), so it can
  never drive two messages onto the same nonce — the nonce-reuse safety of
  Noise is preserved by construction.
- **advanced only on success** — on any error nothing is written and the
  counter does not move (`cipher_state.rs:98-109`, `:133-136`;
  `datagram.rs:203-204`).
- **exhaustion-guarded** — sealing at `u64::MAX` is refused with
  `NonceOverflow` (`cipher_state.rs:98-100`), and `u64::MAX` is *reserved* for
  the `Rekey()` transform (`datagram.rs:216-218`, `cipher_state.rs:305`), so
  the usable counter space is `0 ..= 2^64 − 2`. Not a practical limit
  (`transport.rs:150-154`).

**What the receive half requires.** `DatagramRecv::decrypt_at(counter, ad,
ciphertext, output) -> bytes` (`datagram.rs:446`). It is **stateless with
respect to ordering and replay**:

- It opens whatever `counter` the caller presents, **in any order, any number
  of times** (`datagram.rs:407-411`, `cipher_state.rs:233-247`). It performs
  **no replay rejection** — "Replay protection is explicitly the caller's duty"
  (`datagram.rs:22-26`, `:407-411`). A datagram protocol that needs dedup must
  track seen counters itself (a sliding window in the WireGuard/IPsec style).
- The counter is passed straight through as the AEAD nonce
  (`cipher_state.rs:276` → `Ci::decrypt(key, counter, ad, …)`;
  `datagram.rs:328`), so **the receiver must already know the counter before it
  can decrypt**. This is the fact that forces the counter into the cleartext
  packet header (see §2–3).
- `ad` is caller-chosen and opaque to hiss; the seal only requires that
  encrypt and decrypt use the *same* `ad` bytes. hiss neither reads nor
  constrains the AD contents.

**The epoch ratchet (rekey without a re-handshake).** With
`into_datagram_with_epoch(epoch_size)` the transport keys rotate on a
counter-derived schedule so an idle-but-long-lived session does not die of key
age (`datagram.rs:29-49`, `:110-157`):

- A message at `counter` belongs to epoch `counter / epoch_size`
  (`datagram.rs:119-125`, `:220`, `:324`). Epoch `e`'s key is the Noise §11.3
  `Rekey()` transform chained `e` times from the handshake key
  (`cipher_state.rs:294-313`). **Both peers must pass the identical
  `epoch_size`** or they disagree on which key opens which packet
  (`datagram.rs:123-125`). slither fixes it protocol-wide at
  `REKEY_EPOCH_MSGS = 65_536` (`session.rs:50`, SPEC.md:312).
- Each direction ratchets **independently** on its own counter
  (`datagram.rs:44-45`, `:123-125`).
- The receiver keeps the **current and immediately-preceding** epoch keys, so a
  datagram reordered across one boundary still opens (`datagram.rs:270-286`,
  `:331-341`); anything older is refused, its key ratcheted away.
- **`MAX_EPOCH_JUMP = 2` (hiss-fixed constant, `datagram.rs:74`).** A counter
  more than two epochs beyond the committed epoch is refused **without deriving
  any key** (`datagram.rs:353-356`), bounding the CPU a single forged
  far-future counter can demand. A *legitimate* peer that far ahead means a
  whole 65 536-message epoch elapsed in silence — a dead session the liveness
  timer kills long before (`datagram.rs:61-73`, SPEC.md:335-341).
- **Commit-and-cap discipline.** A future-epoch datagram is opened under a
  *candidate* key derived from a copy; the receiver's committed keys advance
  **only after the AEAD tag verifies** (`datagram.rs:343-390`, esp. `:370-382`).
  A forged packet bearing a huge counter is therefore rejected without moving
  the receiver forward — it **cannot desynchronise the session**. This is the
  property that makes any counter-reconstruction scheme (§3) safe: a
  mis-reconstructed counter is just a failed decrypt, never a wedged receiver.

**Other fixed limits.**

- **`MAX_MESSAGE_LEN = 65_535`** (`cipher_state.rs:19`) — the ciphertext
  (including tag) of any one sealed message; enforced on both encrypt
  (`cipher_state.rs:103-106`) and decrypt (`cipher_state.rs:205-209`,
  `datagram.rs:315-319`). slither's `MAX_DATAGRAM = 1200` (`wire.rs:63`) is far
  under this, but the frame parser and any future stream-batching must respect
  the cap — a single seal can never exceed 64 KiB.
- **Tag size = 16 bytes** (ChaChaPoly `TAG_SIZE`, `cipher.rs:73`; surfaced as
  `Transport::OVERHEAD`, `transport.rs:110`). Every sealed packet costs a flat
  16-byte AEAD tag on top of the header.
- **`session_id`** — both halves and the peer derive the same `SessionId` from
  the handshake hash (`datagram.rs:229-233`, `:459-463`). Available for demux
  keying but *not* currently on the wire; slither routes by a separate 32-bit
  index instead (see §3).

**Fixed by hiss vs. free for slither.**

| hiss fixes | slither is free to choose |
|---|---|
| Counter is monotonic, hiss-owned, never chosen by caller (`datagram.rs:190-197`) | Whether/how to put the counter on the wire (full 8 B, truncated, protected) |
| Counter *is* the AEAD nonce; recv needs it to decrypt (`cipher_state.rs:276`) | The header layout, field widths, and which bytes are the AD |
| Recv does no replay/dedup (`datagram.rs:407-411`) | Replay-window size and shape; ACK record; loss detection |
| Epoch = `counter/epoch_size`, both ends same size (`datagram.rs:119-125`) | The `epoch_size` value (fixed at 65 536), and everything above the seal |
| `MAX_EPOCH_JUMP = 2`, commit-and-cap (`datagram.rs:74`, `:343-390`) | Frame taxonomy inside the seal; keepalive/liveness policy |
| `MAX_MESSAGE_LEN = 65_535`, tag = 16 B, `u64::MAX` reserved | Handshake packet framing, demux index, mac1 gate |

---

## 2. The packet-number question

QUIC's packet number is (a) the AEAD nonce input, (b) the ACK-referenced
identifier, (c) monotonically increasing, and (d) never reused within a number
space. hiss's `DatagramSend` counter is **exactly (a)+(c)+(d) already**:
it is the nonce (`cipher_state.rs:276`), monotonic (`cipher_state.rs:107-108`),
and never reused (the guard at `cipher_state.rs:98-100` plus monotonicity).
The only thing it is *not*, out of the box, is (b) — an ACK-referenced
identifier — because nothing references it yet.

**Recommendation: use the hiss counter directly as the packet number.** An ACK
references hiss counters; there is **no separate packet-number field** on the
wire. This is what the current Leg 2 design already ratified — "the Leg 1
datagram `counter`, which **is** the packet number — unique, monotonic, never
reused, so a retransmission is never ambiguous (no Karn's problem)"
(SPEC.md:390-391) — and a clean slate should keep it, because inventing a
second monotonic identifier next to the counter would be pure redundancy that
could only ever drift out of sync with the real nonce.

Trace of the consequences:

- **The counter rides in the packet header, in cleartext, because it is needed
  to decrypt** (`cipher_state.rs:276`; header today `wire.rs:227-247`). This is
  acceptable and precedented: QUIC's packet number is likewise recoverable by
  the receiver (QUIC hides it behind *header protection* but the receiver still
  reconstructs it before decrypting), and WireGuard ships its 64-bit counter in
  cleartext with no protection at all. slither's TODO already accepts this
  explicitly: "the cleartext packet counter already exists as the nonce, same
  as WireGuard" (TODO.md:81-82). The metadata leaked is the send-rate / counter
  progression, not payload — the same leak WireGuard tolerates.
- **ACK ranges over the counter space work directly.** Because the counter is
  dense and monotonic from 0 (`cipher_state.rs` starts `n = 0`; first packet is
  counter 0), ACK ranges are ordinary descending runs over `u64` — exactly
  QUIC RFC 9000 §19.3.1 semantics, which the current `AckFrame::from_window`
  already implements over the counter (`frame.rs:298-343`, SPEC.md:414-448).
  "Never below counter zero" is the only edge (`frame.rs:301`).
- **Epoch-rekey keeps this clean.** The counter is **never reset by the
  ratchet** — `Rekey()` derives a new key but does not touch the counter
  (`transport.rs:141-160`, `cipher_state.rs:146-157`); the epoch is a pure
  function of the still-monotonic counter (`datagram.rs:220`, `:324`). So an
  ACK range may span an epoch boundary with no special handling: the counter it
  names uniquely selects both the packet *and* (by division) the key that
  opened it. Rekey is invisible to the ACK layer.
- **Where the counter space *does* restart: a DH re-handshake.** A re-handshake
  builds a brand-new `Transport` → new `DatagramSend` counting from 0 again.
  So slither has **per-session packet-number spaces**, analogous to QUIC's
  separate spaces (Initial/Handshake/1-RTT) rather than one space across key
  updates. This is clean here because each session also carries a distinct
  `receiver_index` (§3), so the demux separates the two spaces during the
  make-before-break rekey overlap, and the recovery/ACK state resets per
  session while per-connection state (message seqs, dedup, RTT) survives the
  swap (SPEC.md:506-510). The synthesis should note this is a *deliberate
  divergence from QUIC*, which keeps one PN space across key updates — slither
  can't, because a re-handshake is a fresh Noise session with a fresh counter,
  and that is the right trade (the alternative, carrying a counter across
  sessions, would violate the "caller never chooses the counter" invariant at
  `datagram.rs:190-197`).

**One thing the counter is NOT: an ACK is not a nonce.** Do not let the ACK
layer ever feed a counter back into a *send* nonce. The send counter is
hiss-owned; ACKs reference *received* counters for reliability only. The two
directions have independent counters (`transport.rs:150-154`,
`datagram.rs:44-45`), so "packet number space" is per-direction as well as
per-session.

---

## 3. Header design

The sealed data path splits into **cleartext header** (what the receiver needs
before it holds the key, or before it can pick the key) and **sealed body**
(everything else).

**What MUST be cleartext:**

1. **The counter** — it is the AEAD nonce and the epoch selector; the receiver
   cannot decrypt or even choose a key without it (`cipher_state.rs:276`,
   `datagram.rs:324`). Non-negotiable.
2. **A receiver/connection index for demux** — before decryption the receiver
   must route the packet to a session to know *which key* to try. slither uses
   a 32-bit `receiver_index` (`wire.rs:239-247`) demuxed through
   `index_to_conn: HashMap<u32, ConnId>` (`endpoint.rs:434`, `:798`), exactly
   WireGuard's receiver index. (hiss's `SessionId` could serve instead but is
   larger and not currently wired to the packet.)
3. **Type + version bits** — to classify the datagram (handshake vs data vs
   reserved) before any parse, cheaply and panic-free (`wire.rs:319-332`).

**What lives inside the seal:** *everything else* — all frames (STREAM, ACK,
MAX_DATA, DATAGRAM, …). There is no cleartext length field: the AEAD gives the
exact plaintext length and the parser runs to the end (SPEC.md:398-399).

**The AD choice.** slither authenticates the *entire* cleartext header as the
AEAD associated data (`session.rs:236-237`, `:284-285`; the 14 header bytes are
the AD, SPEC.md:88). hiss is agnostic — it only requires the same `ad` on both
ends (`datagram.rs:205`/`:446`). Note two subtleties for the redesign:

- The counter in the AD is *redundant* — it is already the nonce, so tampering
  with it changes the nonce and fails the tag regardless. Keeping it in the AD
  is harmless and keeps the AD = "the header verbatim", which is the simplest
  rule.
- Authenticating `receiver_index` and the type/flags byte is a slither
  *choice*, stronger than WireGuard (whose data AEAD uses empty AAD and leaves
  the receiver index unauthenticated routing). Recommend keeping it:
  header-tampering becomes tag-detectable at zero extra cost.

**Proposed compact header — two options.**

*Option A (recommended for v1): the WireGuard-simple header, essentially
today's.*

```
type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)      = 14 B, all = AD
```

Full 8-byte counter, no reconstruction, no header protection. This is the
current `DataHeader` (`wire.rs:227-247`). Proven, zero risk, the counter is the
literal nonce with nothing to reconstruct. Overhead per packet: 14 B header +
16 B tag = 30 B.

*Option B (efficiency lever): QUIC-compact header with a truncated packet
number.*

```
flags(1: form|version|pn_len) ‖ receiver_index(4) ‖ counter_trunc(1..4)   = 6..9 B
```

Fold type+version into a flags byte; carry only the low 1–4 bytes of the
counter and **reconstruct** the full 64-bit value at the receiver from the
largest previously accepted counter (QUIC's `pn` reconstruction). Saves 5–8 B
per packet. Costs, and the hiss interactions the synthesis must respect:

- The reconstructed value must be the **exact** 64-bit counter, because it is
  both the nonce and the epoch selector (`datagram.rs:324`). The on-wire
  truncated bytes go in the AD; the reconstructed full counter is the nonce —
  same split QUIC uses.
- A mis-reconstruction (or a forged truncated PN) is **safe**: it yields a
  wrong nonce/epoch and the AEAD simply fails, and the epoch machinery's
  commit-and-cap (`datagram.rs:370-390`) guarantees no committed-key movement.
  So truncation cannot wedge the receiver — worst case is a dropped packet.
- Reconstruction interacts with the replay window: you reconstruct against the
  window's `greatest` (`session.rs:66-104`), so the window must be wide enough
  that in-flight reordering never ambiguates the truncated bits — another
  reason to size the window generously (§5).

**Recommendation:** ship Option A for v1 (matches WireGuard, no reconstruction
risk, the counter-in-clear is already accepted policy), and keep Option B on
the table as the per-packet-overhead lever once streaming throughput makes 8
header bytes matter. **Header protection** (QUIC-style PN encryption) is a
*separate* axis: it would hide the counter/epoch progression from a passive
observer but adds a second per-packet crypto op and complexity; WireGuard omits
it and slither's TODO accepts the cleartext counter (TODO.md:81-82), so
recommend **no** header protection in v1, flagged as a future metadata-hardening
option.

**Handshake packet types and the mac1 DoS gate — keep them.** Strong
recommendation to retain WireGuard's separate handshake packet types and the
mac1 gate:

- **mac1** (`mac.rs`, SPEC.md §4) is a cheap keyed-BLAKE2b tag over the packet,
  keyed on the *recipient's* public static, verified **before any DH**
  (`mac.rs:73-79`, SPEC.md:135). It is the anti-amplification / cheap-reject
  gate: a garbage flood or wrong-key packet dies at one keyed hash, never
  reaching the P-256 provider. Removing it would expose the DH provider to a
  trivial CPU flood.
- The **staged accept** (typestate `Intro → Claimed → Proven → Connection`,
  TODO.md:41-60) is built *on* the IK handshake shape: mac1 (0 DH) → `es`
  (1 DH, recovers claimed static) → `ss` (1 DH, proves possession + decrypts
  the timestamp) → `ee,se` + msg2. Both the DoS gate and the graduated-cost
  accept depend on keeping the IK handshake and its distinct packet types.
- Keep the reserved `0x05` cookie/mac2 type (`wire.rs:42`) for a future
  second-tier gate against *valid-looking* floods (SPEC.md:139).

So: the data path is a blank sheet, but the **handshake framing and mac1 stay
as-is** — they are load-bearing security structure, not legacy.

---

## 4. Frames inside the seal

**The layering, confirmed.** One sealed packet is:

```
cleartext header  ‖  AEAD_seal( nonce = counter, ad = header, plaintext = frame ‖ frame ‖ … )
```

The counter comes from `encrypt_next` (`datagram.rs:205`, returned alongside
the byte count); the header carrying it is the AD; the plaintext is a
concatenation of QUIC-style frames parsed to the end of the decrypted buffer
(SPEC.md:398-399, `frame.rs` `encode_all`/`decode_all`). The current frame
taxonomy is PADDING/PING/ACK/DATA with a reserved `0x04..=0x0F` STREAM space
(SPEC.md:401-408) — the streams track owns extending this; the wire layering is
what matters here and it is simply "header ‖ AEAD(frames)".

**Coexistence with the keepalive and the WireGuard timers.** Today a keepalive
is an **empty plaintext** — a 16-byte tag-only ciphertext, distinct from any
frame (SPEC.md:93-94, 398-399). A DATA frame carrying a zero-length message is
*not* empty plaintext (it is an 11-byte DATA header + 0 bytes), so the two are
distinguishable (SPEC.md:452-468, 541-543). For the clean slate, decide
deliberately between two keepalive signals:

- **Empty-plaintext keepalive** (current): cheapest possible liveness beacon,
  zero frame overhead, and unambiguous now that an empty *message* is a real
  DATA frame. Marks the liveness clock (sealed via `seal`, `session.rs:207`).
- **Explicit PING/keepalive frame** (QUIC-shaped): PING elicits an ACK and is
  ack-eliciting (SPEC.md:405). More uniform, but costs a frame byte and blurs
  "liveness beacon" with "ack-eliciting probe".

Recommend keeping the empty-plaintext keepalive as the *liveness* beacon
(cheap, marks the send clock) and using PING purely as the *ack-eliciting*
probe — they serve different masters (liveness vs. loss detection).

**How a pure-ACK packet stays liveness-neutral — the `seal_quiet` concept.**
This is the crux of frame/timer coexistence. The WireGuard timers measure the
keepalive and dead deadlines from the **last send** (`session.rs:323-342`):
`should_keepalive` fires when received-since-sent and 10 s elapsed;
`is_dead` fires when sent-since-received and 15 s elapsed. If the frame layer's
automatic control traffic (ACKs, PTO probes, retransmissions) refreshed that
clock, two failures follow (SPEC.md:495-504):

1. an unending PTO probe train would defer `DEAD_TIMEOUT` forever — a
   partitioned session would never die;
2. an immediate ACK would reset the last-send clock and suppress the ruled 10 s
   keepalive dance.

The fix is `Session::seal_quiet` (`session.rs:224-226`): it seals a control
packet through the identical `seal_inner` — same sealed-Data wire, same counter
increment, same replay-relevant counter — **but does not touch `last_send`**.
Only fresh application sends and the Leg 1 keepalive mark the clock
(`session.rs:207-211`). Result: a partitioned connection still dies exactly
15 s after its last *fresh* send, probes notwithstanding (SPEC.md:498-504).
The clean-slate design must preserve this split: **the liveness clock is driven
by application intent, not by the automatic reliability machinery**, even
though both ride the same seal. One accepted redundancy: a pure receiver keeps
keepaliving on schedule even though its ACKs already flow (SPEC.md:503-504).

---

## 5. Replay window vs. ACK vs. packet-number space

Today these three are **fused into one 128-bit structure** and the redesign
should consciously decide whether to keep them fused or split them — they are
different duties:

- **Replay window** — a *security* duty: reject a duplicate or too-old counter
  so a captured packet can't be replayed. Sized for *reordering tolerance*.
  Currently a `greatest: u64` + `bitmap: u128` RFC 6479 window,
  `REPLAY_WINDOW = 128` (`session.rs:53`, `:66-104`).
- **ACK** — a *reliability* signal: tell the sender which packet numbers
  arrived, so it can retransmit lost *frames* and sample RTT. Sized for *ACK
  fidelity*.
- **Packet-number space** — the counter axis both of the above live on; per
  §2 it is the hiss counter, per-direction and per-session.

The current design *reuses* the replay window as the ACK source: `from_window`
builds ACK ranges straight off the same `(greatest, bitmap)` snapshot
(`frame.rs:298-343`, `session.rs:112-118`, `:258-260`, SPEC.md:432-436). One
record, not two — the "reuse don't duplicate" rule. The cost of fusing is that
**ACK fidelity is capped at the replay-window width**: an ACK can only report
the freshest ≤ 128 counters; older received counters silently stop being
acknowledged, and the sender's DATA dedup absorbs the resulting spurious
retransmissions (SPEC.md:435-436).

**Reconciliation with hiss's stateless recv.** hiss does *zero* replay/dedup
(`datagram.rs:407-411`); slither owns 100 % of it. Two invariants the redesign
must keep:

1. **Replay-check only *after* the AEAD authenticates.** `open` calls
   `decrypt_at` first and consults the window only on success
   (`session.rs:288-296`). Never trust an off-the-wire counter before the tag
   verifies — otherwise a forged counter could poison `greatest`.
2. **The window's `greatest` advances only on authenticated counters**, and the
   epoch cap (`MAX_EPOCH_JUMP = 2`, `datagram.rs:74`, `:353-356`) plus
   commit-and-cap (`datagram.rs:370-390`) already bound how far a *forged*
   far-future counter can reach — it fails decrypt before it ever touches the
   window. So the two hiss safety rails and the slither replay window compose
   cleanly: hiss bounds forged counters' key-derivation cost; slither's window
   bounds authenticated counters' replay.

**How big should the window be now that it's free?** 128 is small for a
streaming transport. Reference points:

- **Linux kernel WireGuard**: RFC 6479 with an ~8192-bit total / ~8000-packet
  effective window (`COUNTER_BITS_TOTAL 8192`, minus a redundant word). Two
  orders of magnitude more than slither's 128.
- **IPsec / RFC 6479** deployments: commonly 64–1024.
- **QUIC (RFC 9000)** mandates *no* fixed replay window — a packet number is
  never processed twice because the receiver tracks received PNs (as ranges)
  for ACK generation and simply ignores duplicates; the "window" is effectively
  the received-PN range set, pruned below largest-acked.

At streaming rates the 128 window is a real ceiling: 1 Gbps of 1200-byte
packets is ~100 k pkt/s, so 128 packets ≈ 1.3 ms of reordering/ACK memory —
below a single RTT. Recommend **decoupling and enlarging**:

- **Replay window**: grow to a WireGuard-scale sliding bitmap (≈ 2 k–8 k bits;
  a `[u64; N]` word array, still O(1) admit). This is cheap RAM and restores
  real reordering tolerance for a fast flow. It also widens the safety margin
  for any truncated-PN reconstruction (§3, Option B).
- **ACK record**: either (a) keep it fused but at the new larger width, so one
  structure still serves both duties (simplest, bounded memory, preserves
  "one received-packet record"); or (b) split off a QUIC-style **range-based**
  received-PN tracker for ACKs, giving unbounded fidelity independent of the
  security window. (b) matters most once congestion control arrives, where ACK
  precision drives cwnd and pacing; the current design explicitly excludes CC
  (SPEC.md:391-392) so (a) suffices for now.

**Recommendation:** enlarge the fused structure to a WireGuard-scale window for
v1 (keeps the single-record simplicity, fixes the throughput ceiling), and flag
the range-based ACK tracker as the upgrade the congestion-control milestone will
want. Either way, keep the **immediate-ACK policy** (SPEC.md:437-441) and the
**bounded ACK processing** (intersect a received ACK with the in-flight set
rather than materialising its ranges — `frame.rs` `AckFrame::contains`/`acked`,
SPEC.md:442-448) so a hostile-but-authenticated peer can't force a
multi-megabyte expansion.

---

## 6. A concrete strawman wire

Pulling §§2–5 together. **Handshake packets are kept from the ratified wire**
(WireGuard-proven, and the mac1 gate + staged accept depend on them); the
**data packet is the redesigned surface**.

### Handshake packets (unchanged in shape)

```
HandshakeInit  0x01 :  type(1) ‖ version(1) ‖ sender_index(4) ‖ IK_msg1(174) ‖ mac1(16)   = 196 B
HandshakeResp  0x02 :  type(1) ‖ version(1) ‖ sender_index(4) ‖ receiver_index(4)
                       ‖ IK_msg2(81) ‖ mac1(16)                                            = 107 B
```

- IK msg1 carries the 12-byte initiation timestamp as its *encrypted* Noise
  payload (WireGuard's replay defence; SPEC.md:145-167, `wire.rs:79-83`).
- mac1 keyed on the recipient's static, verified before any DH (`mac.rs`,
  SPEC.md §4).
- `0x05` reserved for cookie/mac2 (future second-tier flood gate).

### Data packet (the redesign)

*Cleartext header (also the AEAD associated data):*

```
type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)      = 14 B      [Option A, recommended v1]
```

- `type/version` → classify before parse (`wire.rs:319-332`).
- `receiver_index` → demux to a session/key before decrypt
  (`endpoint.rs:434`, `:798`); WireGuard's receiver index; 4 B is ample since
  roaming is by *authenticated source address* (`session.rs:300-306`), not by a
  migration-privacy connection ID.
- `counter` → the hiss send counter = the packet number = the AEAD nonce = the
  epoch selector. **This is the load-bearing reconciliation with hiss**
  (`datagram.rs:205`, `cipher_state.rs:276`, `datagram.rs:324`).
- Option B (`flags(1) ‖ receiver_index(4) ‖ counter_trunc(1..4)`, 6–9 B) is the
  drop-in overhead-reduction once throughput justifies PN reconstruction (§3).

*Sealed body:*

```
AEAD_seal( nonce = counter, ad = header(14 B), plaintext = frame ‖ frame ‖ … )
```

- Frames run to the end of the decrypted buffer; no cleartext length field
  (the AEAD gives the length).
- 16-byte tag (`cipher.rs:73`). Per-packet overhead: 14 + 16 = 30 B; max
  application plaintext 1200 − 30 = 1170 B (`wire.rs:75`), minus per-frame
  headers.
- Empty plaintext = liveness keepalive (marks the send clock via `seal`);
  control frames (ACK/PTO/retransmit) sealed via `seal_quiet` are
  liveness-neutral (`session.rs:207-226`, §4).

### Rationale in one paragraph

The counter is the pivot: because hiss already owns a monotonic, never-reused
nonce and hands it back on every seal (`datagram.rs:205`), slither gets a QUIC
packet number for free and needs no second identifier. Putting that counter in
the cleartext header is forced (the receiver decrypts with it,
`cipher_state.rs:276`) and precedented (WireGuard). ACKs reference it directly;
the epoch ratchet rides invisibly underneath because the epoch is a pure
function of the same counter and the counter never resets within a session
(`transport.rs:141-160`, `datagram.rs:324`). Replay and ACK are slither's
alone (hiss does neither, `datagram.rs:407-411`) and should be sized for a
streaming transport, not a keepalive VPN. The handshake and mac1 stay because
they are the DoS gate and the staged-accept spine, not legacy weight.

### Open questions for the architecture synthesis

1. **Full 8-byte counter (Option A) vs. truncated PN (Option B).** Per-packet
   overhead vs. reconstruction complexity. Reconstruction is *safe* against the
   epoch machinery (`datagram.rs:370-390`) but adds a moving part and couples to
   the replay-window width.
2. **Header protection (PN encryption): in or out?** Out for v1 (WireGuard
   precedent, TODO.md:81-82); revisit for metadata hardening.
3. **Replay window size and ACK coupling.** Grow to WireGuard-scale (≈ 2 k–8 k
   bits) for throughput; keep ACK fused to it, or split a range-based received-PN
   tracker once congestion control needs precise ACKs (§5).
4. **Keepalive signal.** Empty-plaintext beacon (cheap, liveness-marking) vs.
   explicit PING (ack-eliciting) — recommend both, different roles (§4).
5. **Per-session packet-number spaces at rekey.** slither restarts the counter
   on a DH re-handshake (a fresh hiss `Transport`), diverging from QUIC's
   single-space-across-key-updates. The `receiver_index` demux separates the
   overlapping spaces; confirm the reset-recovery / survive-connection-state
   split (SPEC.md:506-510) is the intended model.
6. **Demux key: 32-bit index vs. hiss `SessionId`.** The index is smaller and
   WireGuard-shaped; `SessionId` is handshake-derived and already computed
   (`datagram.rs:459-463`) but larger on the wire. Index recommended; note the
   choice.

### hiss constraints the wire MUST respect (with citations)

- Counter is hiss-owned, monotonic, caller can never choose it —
  `datagram.rs:190-197`, `:205-227`; `cipher_state.rs:127-136`. → slither must
  *transmit* the counter, never mint its own packet number.
- Counter is the AEAD nonce; recv needs it before decrypt —
  `cipher_state.rs:276`; `datagram.rs:328`, `:189-192`. → counter must be
  cleartext or exactly reconstructible.
- Recv is stateless, no replay — `datagram.rs:22-26`, `:407-411`;
  `cipher_state.rs:233-247`. → slither owns the replay window; check *after*
  the tag verifies (`session.rs:288-296`).
- Epoch = `counter / epoch_size`, both ends identical size —
  `datagram.rs:119-125`, `:220`, `:324`. → `REKEY_EPOCH_MSGS` is protocol-fixed
  (`session.rs:50`), not a knob.
- `MAX_EPOCH_JUMP = 2`; forged far-future counters refused without key
  derivation — `datagram.rs:74`, `:353-356`. → bounds forged-counter CPU;
  legit > 2-epoch jumps are ruled out by liveness.
- Commit-and-cap: committed keys advance only on AEAD success —
  `datagram.rs:370-390`. → any counter reconstruction / forged counter is a
  safe drop, never a desync.
- Straggler tolerance is exactly one epoch back — `datagram.rs:270-286`,
  `:331-341`. → reordering across *two* epoch boundaries loses the key; window
  and reordering budgets must stay inside one epoch (65 536 msgs).
- `MAX_MESSAGE_LEN = 65_535` — `cipher_state.rs:19`, enforced
  `datagram.rs:315-319`. → one seal ≤ 64 KiB; frame parser and any future
  stream batching must cap accordingly.
- Tag = 16 B (`cipher.rs:73`; `transport.rs:110`). → flat 16-byte per-packet
  AEAD cost.
- `u64::MAX` reserved for `Rekey()`; sealing/opening refuse it —
  `datagram.rs:216-218`, `:320-322`; `cipher_state.rs:98-100`, `:273-275`. →
  usable counter space `0 ..= 2^64 − 2`; exhaustion is terminal `NonceOverflow`.
- Two independent per-direction counters — `transport.rs:150-154`,
  `datagram.rs:44-45`. → packet-number space is per-direction (and per-session).
```

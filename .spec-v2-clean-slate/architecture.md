# slither clean-slate architecture — decision document

> Synthesis for the whole-protocol redesign (streams in, wire broken once).
> Consumed by the spec-writer agent; this is not the SPEC. Inputs: the three
> clean-slate research reports (quic-streams, congestion, wire-reconciliation),
> the four prior research reports (hiss-api, wireguard, sansio-quinn,
> actor-inventory), SPEC-v2.md (superseded draft; security engineering
> carried), SPEC.md v1 (reference only, zero authority), TODO.md.
> Fixed anchors respected throughout: hiss 0.3.x IK via `noise!`/`channel!`,
> quinn-shaped object model with staged accept, sans-io core + thin `!Send`
> tokio shell on a `LocalSet`, zero bubble deps, MSRV 1.96.

---

## A. Layer picture

```
UDP datagram (≤ MAX_DATAGRAM = 1200 B)
│
├─ 0x01 HandshakeInit ─ type(1)‖ver(1)‖sender_index(4) ‖ IK msg1 ‖ mac1(16)
├─ 0x02 HandshakeResp ─ type(1)‖ver(1)‖sender_index(4)‖receiver_index(4)
│                        ‖ IK msg2 ‖ mac1(16)
│        (mac1 gate before any DH; staged accept rides the IK token order)
│
└─ 0x03 Data
   ┌──────────────────────────────────────────────────────────────────┐
   │ cleartext header (14 B, = the AEAD associated data)              │
   │   type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8 BE)       │
   ├──────────────────────────────────────────────────────────────────┤
   │ AEAD_seal( nonce = counter, ad = header,                         │
   │            plaintext = frame ‖ frame ‖ … )      + 16 B tag       │
   │                                                                  │
   │   ONE unified frame stream (all fields RFC 9000 §16 varints):    │
   │   PADDING · PING · ACK · RESET_STREAM · STREAM(OFF/LEN/FIN)      │
   │   MAX_DATA · MAX_STREAM_DATA · MAX_STREAMS_{BIDI,UNI} · CLOSE    │
   │   DATAGRAM                                                       │
   │                                                                  │
   │   empty plaintext (tag-only) = liveness keepalive, bypasses      │
   │   the frame layer entirely                                       │
   └──────────────────────────────────────────────────────────────────┘
```

**The Leg 1/Leg 2 split is dead.** There is one frame layer inside the seal
(RFC 9221 style): the old "whole packet is either raw datagram or reliable
frames" tiering folds into "every sealed packet's plaintext is a frame
stream; DATAGRAM is just another frame type". One packet can coalesce an ACK,
a STREAM fragment, a credit grant, and an unreliable datagram. The old
whole-message DATA frame, its seq space, its dedup floor, and the 1159-byte
message cap are all gone — fragmentation and ordering are the STREAM frame's
offset field, and "reliable message" is API sugar over short unidirectional
streams (B.8).

**On the counter.** The hiss `DatagramSend` counter *is* the packet number:
monotonic, hiss-owned, never caller-chosen, simultaneously the AEAD nonce and
the epoch-ratchet selector. ACKs reference it directly; there is no second
identifier. It must ride in cleartext because the receiver decrypts with it
(hiss `decrypt_at(counter, …)`). Per-direction, per-session (a DH
re-handshake restarts it at 0 — B.15).

**Object model mapping.**

| Object | Owns |
|---|---|
| `Endpoint` | socket + demux (`receiver_index → Connection`, pending-index table), the stage-0 intro queue + staged-accept verbs, the hint map + internal rekey continuation, all initiator pendings, the timestamp guard, index minting, the root RNG. |
| staged objects (`Intro → Claimed → Proven`) | one inbound initiation's graduated state; each verb is a driver round-trip into the endpoint core; drop = silent reject at every stage. |
| `Connection` | one Noise session (seal/open, replay window, roaming, liveness/rekey timers), the unified frame layer, recovery (RTT/loss/PTO) + congestion controller, connection-level flow control, the streams table, the datagram queue, CLOSE lifecycle. |
| stream handles (`SendStream`/`RecvStream`, bidi pairs) | per-stream state: send buffer + un-ACKed ranges + FIN, or reassembler + received-credit ledger; borrow the connection core through the shell. |

**Sans-io shape (carried from SPEC-v2 §6, re-derived).** Two pure cores —
`core::Endpoint<I: Identity>` and `core::Connection` — with the str0m
single-`poll_output` contract: every mutating call (`handle_datagram`,
`handle_timeout`, verb calls, `send`/`open_*`) is followed by draining
`poll_output()` to the terminal `Timeout(Option<Instant>)`, which is both the
drain sentinel and the next-deadline announcement. Connection→endpoint events
fold into `ConnOutput::ToEndpoint` (no second queue to forget). Named-timer
table per core, min-deadline out, idempotent `handle_timeout` (stop-then-run).
`now: Instant` is an explicit argument everywhere; the initiation timestamp is
the one wall-clock read, behind an injected clock service. One root RNG seed
in the endpoint core; per-connection sub-seeds drawn at creation.
Plan-seal-commit sealing (recovery mutates only on seal success), sealing
synchronous inside the mutating call, seal returns `(counter, bytes)`. The
shell is one `!Send` driver task (`spawn_local`, `LocalSet` required) owning
the socket and both cores behind the `Wire` trait seam; the whole protocol is
drivable kernel-free over `testutil::FlakyWire` on tokio's paused clock.
Lateness bound `L = 250 ms` is a shell conformance parameter, not protocol.

---

## B. Decisions

### B.1 Unified frame layer — ADOPTED (research recommendation)

One frame stream inside every seal; DATAGRAM is a frame type; the Leg 1/Leg 2
tier split dies. Strictly fewer wire concepts, one retransmission engine, and
coalescing (ACK + STREAM + DATAGRAM in one packet) for free. The empty
plaintext keeps its special role as the liveness keepalive (B.14) — it is not
a frame and is the only non-frame plaintext.

### B.2 Varints, RFC 9000 §16 wholesale — ADOPTED

All frame-body integer fields (type byte included) are QUIC varints,
byte-identical to RFC 9000 §16 / quinn-proto's `varint.rs`. ~50 lines, every
frame benefits, no compatibility cost. The cleartext header is NOT varint —
fixed widths there (u32 index, u64 counter) keep classification and AD
construction trivial. One stated consequence: varints cap at 2^62−1, so ACK
`largest` and stream offsets cap there too; a session would need > 4.6×10^18
packets to reach it while `REKEY_AGE` forces a fresh counter space every
120 s — unreachable, but the spec states the bound explicitly.

### B.3 Full 8-byte cleartext counter — ADOPTED (Option A)

`counter(8)` in clear, no truncated-PN reconstruction in v1. Reconstruction is
provably safe against hiss (commit-and-cap means a mis-reconstruction is a
failed decrypt, never a desync) but it is a moving part coupled to the
replay-window width, for 5–7 saved bytes per packet. WireGuard ships the full
counter in clear; slither accepted the metadata leak already. Truncation is
flagged as a future overhead lever, nothing in this design precludes it.

### B.4 Header protection — NONE in v1 — ADOPTED

No PN encryption. It would hide counter/epoch progression from a passive
observer at the price of a second per-packet crypto pass outside hiss (which
would strain the "all crypto through hiss" rule). Flagged as future metadata
hardening, same axis as B.3 but independent of it.

### B.5 Header layout; whole header = AD — ADOPTED

`type(1) ‖ version(1) ‖ receiver_index(4) ‖ counter(8)` = 14 B, and those 14
bytes are the AEAD associated data verbatim. The counter in the AD is
redundant (it is the nonce) but harmless, and "AD = the header, verbatim" is
the simplest possible rule. Authenticating `receiver_index` and type/version
is stronger than WireGuard (empty AAD) at zero cost — kept. No cleartext
length field: the AEAD gives the plaintext length; frames parse to the end.
Per-packet overhead 14 + 16 = 30 B; `MAX_PLAINTEXT` = 1170.

### B.6 Demux key: 32-bit receiver index — ADOPTED

WireGuard-shaped random nonzero u32, demuxed through the endpoint's
`index → connection` table. hiss's `SessionId` (64 B handshake hash) is
available but oversized for the wire and adds nothing: roaming is by
authenticated source address, not by unlinkable-CID rotation. Index minting
re-draws while the value exists in *either* the session-index or
pending-index table (SPEC-v2 §7 ruling carried — closes the route-stealing
collision).

### B.7 Stream-ID spaces: QUIC's 2 low bits, all four spaces — DECIDED

Adopt QUIC's encoding verbatim: stream ID is a varint; bit 0 = opener
(0 = connection initiator, 1 = acceptor), bit 1 = direction (0 = bidi,
1 = uni); the remaining 60 bits are a per-space monotonically allocated index
from 0. Rationale against the research's "collapse to 1–2 spaces" lean: the
opener bit is *mandatory* anyway (both sides open streams unprompted; parity
is the only handshake-free collision avoidance), and the direction bit is
what makes the message primitive (B.8) cheap — a message is a uni stream, and
uni streams let the receiver allocate no send-half state and expect no
reverse FIN. Two bits are already paid for in the varint; collapsing saves
nothing real and forfeits quinn-proto's directly reusable `StreamId` shape.
Implicit opening by first use (a frame for stream N opens lower-numbered
streams of that space) is adopted; there is no OPEN frame.

**Role stability across rekey (new precision):** the opener bit refers to the
roles of the connection's *original* establishment (tie-break winner = the
connection initiator, B.15/C). Rekeys swap Noise handshake roles freely but
never change stream-ID parity.

### B.8 Reliable-unordered messages: sugar over auto-managed uni streams — DECIDED

The genuinely open fork. Ruling: **the middle path** — unified wire (no DATA
frame type, no second reliability engine), plus a first-class convenience API:

- `Connection::send_message(bytes)`: internally allocates the next outbound
  uni stream, writes the whole payload, sets FIN, and garbage-collects the
  stream state when the FIN'd range is fully ACKed. No stream handle
  surfaces.
- `Connection::recv_message()`: treats each incoming uni stream as one
  message; surfaces the payload only when reassembly is complete (FIN + all
  bytes), then frees the stream. `accept_uni()`/`open_uni()` remain available
  for callers who want incremental streams; which mode consumes incoming uni
  streams is the receiving application's choice, invisible on the wire.

Why not a separate DATA-frame primitive: two reliability paths (whole-message
retransmit + range retransmit) is the single largest avoidable surface in the
old design, and one-stream-per-message preserves the no-head-of-line-blocking
contract that made Leg 2 valuable — messages on distinct streams never stall
each other. Why not streams-only with no sugar: the dominant existing use is
small RPC-shaped messages; forcing every caller through open/write/FIN/GC
boilerplate regresses the product. Costs accepted: per-message overhead is a
STREAM frame header (~3–6 B: type + stream-ID varint + implicit offset 0)
versus the old 11-byte DATA header — a wash or better; stream-state churn is
bounded by the free-list pattern quinn-proto proves out.

Message size bound: sugar-received messages are bounded by the stream's
initial flow-control window — the receiver never extends credit for a
sugar-consumed stream, so `MESSAGE_RECV_MAX = INITIAL_MAX_STREAM_DATA`
(256 KiB). Larger transfers use real streams. Exactly-once surfacing is
inherent (offset reassembly); the old seq-dedup machinery dies.

### B.9 Flow control — ADOPTED in full (research recommendation)

Both levels (stream and connection), absolute byte offsets
(`MAX_STREAM_DATA` / `MAX_DATA` carry limits, monotone-max on receipt, so
duplicates and reordering are naturally idempotent), fixed initial-window
protocol constants, **no transport-parameter negotiation** (slither has no
negotiation surface at all; initial values are protocol constants, later
credit is receiver policy). The BLOCKED family (`DATA_BLOCKED`,
`STREAM_DATA_BLOCKED`, `STREAMS_BLOCKED`) is deferred — pure diagnostics.
Credit-advance rule (concrete, for the spec): re-advertise a level's limit
when application consumption has advanced it ≥ half a window beyond the last
advertised value. DATAGRAM frames are flow-control-exempt (RFC 9221 §5.3).
The credit ceiling *is* the buffer commitment: a receiver only advertises
what it will buffer — this is what replaces `shed_mask` (see C).
Retransmissions of the same stream bytes do not consume new credit.
A peer exceeding advertised credit is a protocol violation ⇒ CLOSE with
`FLOW_CONTROL_ERROR`.

### B.10 MAX_STREAMS: cumulative credit frames, fixed initial values — DECIDED (diverges from the research recommendation)

The research recommended a fixed protocol cap with no frames. That
recommendation implicitly priced streams as rare; B.8 makes them the common
case (one per message), and QUIC's stream limit is *cumulative-count*, not
concurrent — a fixed cumulative cap kills the connection after N messages,
and a "concurrent" cap requires both ends to agree on close timing, which is
exactly the ambiguity QUIC's credit model exists to avoid. So:
`MAX_STREAMS_BIDI` / `MAX_STREAMS_UNI` frames exist, carrying an absolute
cumulative count; initial values are protocol constants
(`INITIAL_MAX_STREAMS_BIDI = 32`, `INITIAL_MAX_STREAMS_UNI = 128` — uni
higher for message traffic); the receiver grants +1 as it fully closes a
stream, batching advertisements (emit when ≥ `STREAMS_CREDIT_BATCH = 8`
unadvertised, or when the peer's remaining allowance drops ≤ 8). Opening
beyond the limit ⇒ `STREAM_LIMIT_ERROR` ⇒ CLOSE. `STREAMS_BLOCKED` stays
deferred with the rest of the BLOCKED family.

### B.11 CONNECTION_CLOSE → the CLOSE frame — ADOPTED (simplified)

Today teardown is timer-only; a deliberate disconnect costs the peer 15 s of
liveness wait. New: `CLOSE { error_code: varint, reason_len: varint, reason ≤
256 B }`, one frame type (no QUIC transport/application split, no
offending-frame-type field). Semantics, minimal and sound:

- `close(code, reason)`: emit CLOSE (sealed, `seal_quiet`), enter **closing**
  for `CLOSE_LINGER = 5 s`: retain only seal capability; answer at most one
  CLOSE per second to inbound packets from the peer; then drop all state.
- Receiving an authenticated CLOSE: surface
  `ConnectionLost::PeerClosed{code, reason}`, emit nothing, drop state after
  a brief drain (same linger, no replies). No close-ACK, no handshake.
- CLOSE is not ack-eliciting and never retransmitted by loss detection (the
  linger's reply rule is its reliability).
- Protocol violations by an authenticated peer (flow-control breach, stream
  limit breach, malformed frame stream) now have a signalled death instead of
  a silent one: emit CLOSE with the matching error code, then linger.
- Only authenticated (in-seal) CLOSE exists — nothing unauthenticated can
  kill a connection; the reserved cleartext close packet type stays dead.

### B.12 Congestion control — ADOPTED in full (research recommendation)

NewReno behind a small `Controller` trait (quinn-proto's shape: `on_sent`,
`on_ack(bytes, sent_time, app_limited)`, `on_congestion_event(sent_time,
is_persistent, lost_bytes)` — once per loss episode, `window()`), wired at
the three existing recovery mutation points. `INITIAL_WINDOW = min(10×1200,
max(2×1200, 14720)) = 12 000 B`, `MINIMUM_WINDOW = 2 400 B`,
`LOSS_REDUCTION_FACTOR = 0.5`, `ssthresh` starts at u64::MAX, integer
appropriate-byte-counting in congestion avoidance. Persistent congestion
(threshold 3 × PTO, requires a prior RTT sample) grafted into the existing
loss-detection walk, collapsing cwnd to `MINIMUM_WINDOW`. `bytes_in_flight`
via a `size` field on the sent-packet map. Send-side admission gate:
`bytes_in_flight + candidate ≤ cwnd`, **exempting PTO probes** (a black-holed
path must stay probeable — the exemption is orthogonal to, and coexists with,
the probe's liveness-neutral `seal_quiet`) and non-ack-eliciting control
packets (pure ACK, CLOSE, keepalive — never tracked in flight). DATAGRAM
frames are congestion-controlled (count in flight, gate applies) but never
retransmitted. `app_limited` derived from "cwnd headroom existed but nothing
was queued" suppresses idle window growth. No pacing (no sub-RTT wakeups in
v1's shell; cwnd bounds bursts adequately at 12 KB), no ECN (wire + socket
work), no CUBIC/BBR (pure additions behind the trait later).

**Reset seams:** cwnd/ssthresh/bytes_in_flight reset to initial on **rekey**
(`epoch_reset` — the old epoch's in-flight accounting can never resolve) and
on **roaming** (`EndpointMoved` — new path, no continuity evidence; this is
new wiring, today roaming has no CC hook). RTT survives both, as today, but
is treated as a prior, not a fact. On roaming the sent map is *kept* (ACKs
for in-flight packets to the old address still resolve; only the controller
resets); on rekey the sent map dies with the counter space (B.15).

### B.13 Replay window 2048 bits; ACK stays fused, truncated newest-first — DECIDED

The freeze that pinned 128 bits is void. New: RFC 6479 sliding bitmap,
`REPLAY_WINDOW = 2048` bits (`[u64; 32]`, 256 B/connection) — above
boringtun's 1024, below the kernel's 8192; ~20 ms of memory at 1 Gbps,
~200 ms at 100 Mbps, comfortably inside one ratchet epoch (65 536), and wide
enough that any future truncated-PN scheme has margin. Replay check stays
strictly post-AEAD; the window's `greatest` advances only on authenticated
counters (hiss's `MAX_EPOCH_JUMP` + commit-and-cap bound forged-counter cost
upstream of it).

**The re-examination asked for:** does CC-in-v1 force the QUIC-style
range-tracker ACK now? No — fused survives v1, deliberately. The fused
window is the single received-packet record (reuse, don't duplicate), and its
failure mode under CC is bounded and benign: ACK fidelity is capped at 2048
counters, so only an ACK-loss burst longer than the window's time-width
(~20 ms at line rate, ~200 ms at 100 Mbps) causes delivered-but-unreported
packets, which surface as spurious retransmissions (streams dedup by offset)
and at worst one spurious congestion event absorbed by the recovery-period
rule (one cut per episode). The decoupled range tracker is flagged as the
known upgrade when sustained >100 Mbps per connection matters; nothing on the
wire changes for it (ACK encoding is already range-based).

Because the 2048-bit worst case (alternating) no longer fits one packet, the
old "window width ⇒ MAX_ACK_RANGES" derivation dies: ACK construction takes
the masked window snapshot, emits ranges **newest-first, descending**, and
truncates at `MAX_ACK_RANGES = 64` or at packet capacity, whichever binds —
dropped oldest ranges are exactly the ones prior ACKs most likely already
carried. A received ACK with more than 64 ranges is malformed (drop the
packet). Bounded ACK processing carries over: intersect ranges with the
in-flight set, never materialise; an ACK whose `largest` exceeds the highest
sealed counter is ignored whole.

### B.14 Keepalive: empty-plaintext beacon + PING probe; the seal/seal_quiet split — ADOPTED

Two signals, two masters, both kept:

- **Empty plaintext** (tag-only ciphertext) is the liveness beacon: cheapest
  possible, bypasses the frame layer, sealed via `seal` (marks `last_send`).
  Passive keepalive at 10 s (received-since-sent), persistent keepalive
  per-connection opt-in (default value 25 s).
- **PING** is the ack-eliciting probe (PTO's last resort), sealed via
  `seal_quiet` — liveness-neutral.

The liveness clock is driven by application intent only: fresh application
sends (STREAM writes, DATAGRAM sends) and the keepalive mark `last_send`;
ACKs, PTO probes, retransmissions, credit frames (MAX_DATA /
MAX_STREAM_DATA / MAX_STREAMS), RESET_STREAM, and CLOSE all ride
`seal_quiet`. Ack-eliciting and liveness-marking are independent axes:
credit frames are ack-eliciting (they need loss recovery — regenerated with
freshest values, not byte-retransmitted) yet liveness-neutral. This preserves
the ruled property: a partitioned connection dies exactly `DEAD_TIMEOUT`
after its last fresh send, probe trains notwithstanding. Keepalives are
admitted to the replay window (they appear opportunistically in ACK ranges;
`ack_delay = 0` when the window's largest was not frame-seen) but never reach
recovery.

### B.15 Packet-number spaces across rekey; what survives — DECIDED (the new seam, stated precisely)

A DH re-handshake builds a fresh hiss `Transport`: the counter restarts at 0,
so slither has **per-session, per-direction packet-number spaces** — a
deliberate divergence from QUIC's PN-continuity across key updates, forced by
hiss's "caller never chooses the counter" invariant and made safe by the
`receiver_index` demux separating the overlapping spaces during
make-before-break (the old session keeps sealing until the swap; the swap
cuts it instantly — C). The hiss key-ratchet epoch (`REKEY_EPOCH_MSGS`) is a
different "epoch": counter-derived, wire-invisible, never resets the counter,
zero recovery/CC interaction.

**Resets on rekey (session-scoped):** hiss cipher states; the replay window;
the sent-packet map, `largest_acked`, `loss_time`, `pto_count`,
ack-pending/ack-delay state (`bytes_in_flight` → 0 with the map);
cwnd/ssthresh (B.12); the liveness clocks (`last_send`/`last_recv` = now);
session indices.

**Survives rekey (connection-scoped) — the streams answer, new relative to
all prior work:**

- **Every stream's state, in full**: send buffers, sent-but-un-ACKed ranges,
  FIN state, reassemblers, final sizes, stream-ID allocators, and the
  cumulative MAX_STREAMS ledgers. Stream offsets are absolute bytes on a
  connection-scoped sequence — nothing about them references a counter
  space.
- **Flow-control state, verbatim**: both directions, both levels. Limits and
  consumption counters are absolute offsets; they neither reset nor
  re-negotiate. (There is nothing to "re-grant": the numbers simply persist.)
- **RTT estimator** (path property), the peer address, the timestamp guard
  (endpoint-scoped anyway).

**Re-queue rule (generalises "undelivered messages re-queue"):** at swap,
walk the dying sent map and return every un-ACKed retransmittable frame's
identity to the pending set — stream ranges as ranges, credit frames as
"re-emit freshest value". Additionally, the connection re-emits its current
MAX_DATA / MAX_STREAM_DATA / MAX_STREAMS values once on the new session
(cheap, removes any credit-in-flight-died stall). DATAGRAM frames queued but
unsent survive; in-flight ones die unmourned (unreliable).

**Restart vs rekey — fixed on the wire (new; kills SPEC-v2's known
limitation).** Rekey and restart were cryptographically indistinguishable at
msg1, which under replace-in-place produced silent, *confirmed*,
undetectable data loss (the restarted peer's from-zero seqs were swallowed as
duplicates and ACKed). With streams the equivalent bug is worse (offset
collisions). Since this is the one wire break, it is fixed now:

- msg1's encrypted payload grows one byte: `timestamp(12) ‖ flags(1)`;
  `flags & 0x01 = CONTINUATION` — set iff this initiation rekeys an
  established connection whose transport state the initiator retains.
- msg2 gains a 1-byte encrypted payload: `flags(1)`;
  `flags & 0x01 = CONTINUED` — set iff the responder matched the initiation
  to an existing connection and retains its transport state.
- Responder rule: CONTINUATION=1 from a known static with a live connection
  ⇒ the internal continuation (silent swap, state survives), msg2
  CONTINUED=1. CONTINUATION=0 from a known static ⇒ the peer restarted: tear
  down the old connection (`ConnectionLost::Replaced`) and surface the
  initiation through the staged accept as a fresh Intro. CONTINUATION=1 from
  an unknown static ⇒ we restarted: process as a fresh accept; msg2
  CONTINUED=0.
- Initiator rule: sent CONTINUATION=1, received CONTINUED=0 ⇒ the peer lost
  state: the connection dies honestly (`ConnectionLost::PeerRestarted`); the
  application reconnects fresh. No flag mismatch can silently merge two
  transport-state generations.

The flags are inside the Noise payloads: authenticated post-`ss`, encrypted,
forgeable only by key-holders (who can only hurt themselves). Reference-suite
sizes move: msg1 175 B / packet 197 B, msg2 82 B / packet 108 B.

**Feasibility verified (research-gaps.md Fact 1):** released hiss 0.3.1
supports a `[N]` payload on the *final* message as-is — the parser accepts it
(`hiss-macros/src/parse.rs:262-282`), the final read returns
`([u8; N], Transport)` with the tail tag checked before `into_transport`
(`codegen.rs:1166-1198`, `:718-730`), and hiss's own Cacophony suite already
exercises IK with `[16] … [15]` payloads (`tests/noise_cacophony.rs:343`,
`:515-521`, reachable from the `v0.3.1` tag). Declaring `[13]`/`[1]` needs no
hiss change or release.

### B.16 Timers — re-derived; values and their justifications

| Timer | Value | Derivation |
|---|---|---|
| `RETRANSMIT_BASE` + jitter | 5 s + U[0,333 ms] | = WireGuard Rekey-Timeout + jitter, cross-checked in paper/kernel/go. Fixed-interval (not exponential) is WG's shipped shape; kept for simplicity. Every retransmit is a completely fresh initiation (new ephemeral, index, strictly-greater timestamp). |
| `HANDSHAKE_GIVEUP` | 90 s | = WG Rekey-Attempt-Time. Initial connect ⇒ `ConnectError::TimedOut`; rekey give-up silent. |
| `KEEPALIVE_TIMEOUT` | 10 s | = WG Keepalive-Timeout; passive (received-since-sent) rule kept. |
| `DEAD_TIMEOUT` | 15 s | keepalive + 5 s grace (one retransmit interval); v1's ruling re-affirmed — the only idle killer. |
| `PERSISTENT_KEEPALIVE` | 25 s default, per-connection `Option<Duration>` | WG's convention; reclassified (SPEC-v2 §8) from constant to recommended default — carried. |
| `REKEY_AGE` | 120 s | = WG Rekey-After-Time. Send-triggered, payload-path-only consult (2026/07/17 amendment carried); not a timer. |
| `REJECT_AGE` | 180 s | = WG Reject-After-Time; payload backstop, consult not timer. |
| `MAX_ACK_DELAY` | 25 ms | now armed as a real ack-delay timer (B.17) and kept in the PTO formula. |
| `CLOSE_LINGER` | 5 s (new) | closing/draining hold: covers the peer's PTO backoff at any plausible RTT without holding state for QUIC's full 3×PTO bookkeeping. |
| `INTRO_TTL` | 15 s | carried from SPEC-v2 (≈3 retransmit intervals; flood hold-cost bound). |
| Loss / PTO | RFC 9002 formulas | unchanged: 9/8 time threshold, packet threshold 3, PTO backoff ×2 capped 2^6, `K_INITIAL_RTT` 333 ms, granularity 1 ms. |
| shell lateness `L` | 250 ms | conformance bound on shell tardiness; cores expose exact deadlines. |

No message-count DH-rekey trigger (WG's 2^60): the epoch ratchet refreshes
keys by count and `REKEY_AGE` re-handshakes by time long before counts
matter.

### B.17 ACK frame: varint encoding; delayed-ACK policy — DECIDED (policy change from v1)

Encoding: QUIC §19.3 shape, all varints — `largest`, `ack_delay`
(microseconds, raw varint, no exponent scaling — there is no negotiation to
carry an exponent, and immediate/25 ms delays fit 1–4 bytes), `range_count`,
`first_range`, then descending `(gap, range)` pairs with RFC 9000 §19.3.1
semantics. No ECN counts. Cap and truncation per B.13. Derived from the
window snapshot (fused), `ack_delay` measured from the arrival of `largest`.

Policy: v1's "immediate ACK for every ack-eliciting packet" is replaced by
**QUIC's default**: ACK after every 2nd ack-eliciting packet or at
`MAX_ACK_DELAY = 25 ms`, whichever first; immediately on out-of-order arrival
(gap observed). Rationale: CC now consumes ACK timing, and streams make
1:1 ACK traffic a real reverse-path cost at throughput; 25 ms is already the
PTO formula's assumption so the change is self-consistent. Costs one named
timer (`AckDelay`) in the connection core's table. ACKs remain
non-ack-eliciting and untracked; pure-ACK packets bypass cwnd.

### B.18 Handshake wire framing — kept in shape; bytes move deliberately

The WireGuard-shaped `HandshakeInit`/`HandshakeResp` packets, mac1 gate, and
reserved cookie type all stay — they are the DoS gate and the staged-accept
spine (0 DH → `es` → `ss` → `ee,se` cost ladder is the product feature).
What moves relative to the old bytes, each deliberate:

1. `VERSION = 0x02` and `PROLOGUE = b"slither\x02"` (B.20) — old and new
   implementations mutually silent-drop, and the version is bound into the
   Noise transcript via the prologue.
2. msg1 payload `[12] → [13]`, msg2 payload `[0] → [1]` (B.15 continuation
   flags). Reference-suite packet lengths become 197 / 108.
3. Everything else is unchanged by value: type bytes 0x01/0x02/0x03, 0x05
   reserved for cookie/mac2 (0x04 stays reserved-unused — the close packet
   concept is dead, CLOSE is a frame), nonzero u32 indices, mac1 keyed
   BLAKE2b-128 with key = BLAKE2b-256(`"slither mac1"` ‖ recipient static
   canonical encoding — the `AsRef` octets per B.19, so 65-B uncompressed
   for P-256, a deliberate change from v1's 33-B compressed), verified
   before any curve work, timestamp strictly-greater forcing, per-static
   greatest-timestamp guard admitted at `authenticate()` / continuation
   post-`ss`.

All golden vectors regenerate exactly once for wire v2, then re-freeze under
the same test discipline.

### B.19 `channel!` — what varies, what the wire sees — DECIDED

`channel!` (macro_rules, no proc-macro) stamps the `hiss::noise!` IK
invocation with a caller-chosen `<Curve, Cipher, Hash>` triple plus the
`Channel`/`Protocol` impl; the IK token block and the `[13]`/`[1]` payloads
are hardcoded. IK is the only pattern. Reference suite:
`P256 / ChaChaPoly / Blake2b`.

- **Wire-visible consequences of the suite**: handshake message sizes (point
  encodings) and, in principle, tag size — so `MSG1_LEN`/`MSG2_LEN`/packet
  lengths are *per-suite derived constants* (the spec's table gives the
  reference suite's values and the derivation formulas). The data-packet
  header, frame layer, and every constant above it are suite-independent.
- **mac1 stays fixed keyed-BLAKE2b for every suite** (the anchor's
  raw-primitive rule, and WireGuard's fixed-BLAKE2s precedent). Making it
  follow the suite Hash would demand a keyed-hash mode from every hiss Hash
  and buy nothing — mac1 is a public-data gate, not session crypto.
- **The canonical static encoding is defined as the `AsRef<[u8]>` octets of
  `Curve::PublicKey`** (P-256: the 65-byte uncompressed SEC1 storage form,
  normalisation enforced by hiss — `p256/mod.rs:145-150`; X25519: the raw
  32 bytes). mac1 is keyed over it, the simultaneous-open tie-break compares
  it, and identity maps/allow-lists key on it — one encoding, three uses,
  aligned with hiss's derived `Ord`. This requires a hiss bound change —
  `Curve::PublicKey: AsRef<[u8]> + Ord` (0.3.1's bound is `Clone` only,
  `curve/mod.rs:69`) — recorded as the third Appendix A dependency; every
  shipped curve already satisfies both. Stated consequence: P-256 mac1
  keying moves from v1's 33-byte compressed form to the 65-byte uncompressed
  form. That is a mac1-byte change absorbed by wire v2's one-time golden
  regeneration; a generic formulation inheriting `AsRef` silently would have
  been a trap under the old frozen wire, and is a deliberate ruling here.
- **No suite identifier on the wire.** Endpoints are monomorphic per suite
  (`Endpoint<C: Channel>`); a mismatched-suite packet dies silently at the
  length gate or mac1 — the same fate as garbage. The version byte does not
  encode the suite.

### B.20 Version byte — DECIDED

`VERSION = 0x02` in every packet header; unknown version or type ⇒ silent
drop; **no version negotiation** exists or is reserved-for. Version 2 means
exactly this spec. The prologue carries the same version so a
version-confused peer also fails cryptographically, not just at
classification. If there is ever a version 3, it is another deliberate break
with its own prologue — negotiation is permanently out of scope for a
mutually-authenticated pair protocol.

### Frame taxonomy (consolidated)

QUIC's numbers are reused verbatim where the concept is shared (zero
cognitive overhead against RFC 9000/quinn-proto; gaps are harmless — the
type is a varint, one byte through 0x3f):

| Type | Frame | Fields (varints) | Ack-eliciting | Retransmit | Notes |
|---|---|---|---|---|---|
| 0x00 | PADDING | — | no | — | |
| 0x01 | PING | — | yes | no | PTO probe |
| 0x02 | ACK | largest, ack_delay(µs), range_count, first_range, (gap,range)* | no | no | B.13/B.17 |
| 0x04 | RESET_STREAM | stream_id, error_code, final_size | yes | regenerate | |
| 0x05 | (reserved: STOP_SENDING) | — | — | — | deferred |
| 0x08–0x0f | STREAM | stream_id, [offset], [length], data; OFF=0x04, LEN=0x02, FIN=0x01 | yes | ranges | the whole fragmentation + ordering mechanism |
| 0x10 | MAX_DATA | max | yes | regenerate | |
| 0x11 | MAX_STREAM_DATA | stream_id, max | yes | regenerate | |
| 0x12 | MAX_STREAMS_BIDI | max (cumulative) | yes | regenerate | |
| 0x13 | MAX_STREAMS_UNI | max (cumulative) | yes | regenerate | |
| 0x1c | CLOSE | error_code, reason_len, reason | no | linger rule | B.11 |
| 0x30/0x31 | DATAGRAM | [length (0x31)], data | yes | never | no flow control; cwnd-gated |

Parse rule: **parse the whole plaintext first, then apply** — any unknown or
malformed frame type fails the whole packet (drop, trace, nothing applied).
This is stricter and simpler than v1's split reserved/unknown rule: with no
extension negotiation, unknown means corruption or skew. Packing order within
a packet: ACK first, then control (credit, RESET_STREAM, CLOSE), then
STREAM/DATAGRAM fill, then PING if a probe still owes ack-eliciting content.

---

## C. Carry-over audit of SPEC-v2

| SPEC-v2 ruling | Verdict | Notes |
|---|---|---|
| Object model: `Endpoint` / staged accept / `Connection`; one connection per remote static; `AlreadyConnected` | **survives unchanged** | stream handles are added below `Connection`; one-conn-per-static remains the routing keystone. |
| Staged accept typestate `Intro→Claimed→Proven→Connection`, DH costs 0/1/2/4, drop = silent reject, claimed-static discipline (nothing durable keyed on a claim) | **survives unchanged** | costs identical (payload byte changes don't touch the DH ladder). Still gates on the hiss split-read minor (E.2): `Claimed` must *suspend*, which 0.3.1's Verify closure cannot express — see Appendix A's rejected-fallback note. |
| Intro queue: cap 1024, per-source 4 counting consumed+unconsumed, TTL 15 s, addr-only dedup replace-with-newest, evict-oldest overflow, own-bytes-on-consume, no `Superseded` anywhere | **survives unchanged** | the per-source-both-tiers counting and own-bytes model were review fixes — carried verbatim. |
| Eager path + hint set + demotion-with-carried-mid-state + `IntroError::Internal` interception | **survives unchanged** | membership-timing-oracle acceptance restated as before. |
| Simultaneous-open tie-break: lexicographically smaller static wins, **post-`ss` only**, forgery cannot cancel a pending (BLOCKER fix) | **survives re-derived** | comparison is over the canonical static encoding — the `AsRef` octets per B.19 (65 B uncompressed for P-256, 32 B for X25519; always equal-length within a suite, and identical to the mac1-keying and identity-map octets, so hiss's derived `Ord` implements it directly). SPEC-v2 named the 33-B compressed form; the encoding moves with the mac1 ruling. New consequence: the tie-break winner fixes stream-ID parity for the connection's life (B.7). |
| No immediate re-arm on failed completion; one completion attempt per retransmit interval; msg2 preconditions length+index+mac1 (MAJOR fix) | **survives unchanged** | |
| `accept()` re-home to freshest parked initiation by park time, guard-rejected candidates discarded, `AcceptError::Stale` (MAJOR fix) | **survives unchanged** | re-home candidates additionally carry the B.15 flags; a CONTINUATION=1 candidate for a not-yet-established static is nonsensical and is discarded like a guard failure. |
| Internal continuation order: tag → guard → pacing → tie-break → admit; record on full admission only; pacing 50/s (20 ms) per known static | **survives re-derived** | gains one step: the CONTINUATION flag routes restart-vs-rekey (B.15) between guard and pacing — flag=0 from a known static diverts to teardown + staged accept instead of silent swap. |
| Timestamp guard: endpoint-global, per-static, pinned/orphan split, `TS_GUARD_ORPHAN_CAP` 1024, no-orphan-on-reject, admission-only LRU refresh, timer aging | **survives unchanged** | |
| Index re-draw across both tables; routes-by-index-but-fails-to-open touches nothing | **survives unchanged** | |
| **`shed_mask` + `RECV_BUFFER` receive backpressure** | **dies — superseded by flow control** | The problem it solved (bounded receive memory without ACKing undelivered data) is solved principledly: credit *is* the buffer commitment, so an in-credit packet always has room by construction; a beyond-credit packet is a protocol violation ⇒ CLOSE (`FLOW_CONTROL_ERROR`), not a shed. DATAGRAM frames need no shed logic — dropping an unreliable datagram at a full app queue is legitimate, and ACKing its packet is honest (the ACK confirms packet arrival; datagram delivery was never promised). The window-marked/never-delivered/never-ACKed trichotomy, the masked-ACK derivation, and the 128-bit second bitmap all go. What survives from that work: the invariant that **liveness and roaming are driven only by authenticated, window-marked packets** — restated without the shed vocabulary. |
| Swap cuts the old session instantly (no grace window) | **survives re-derived** | still ruled; now coherent with CC: in-flight packets dying at the swap cannot fire a false congestion event because cwnd resets at the swap anyway. Un-ACKed stream ranges re-queue (B.15). |
| Replay window stays 128 / `MAX_ACK_RANGES = 63` derivation chain | **dies** | the freeze that forced it is void: window 2048, cap 64, newest-first truncation (B.13). |
| ACK-from-the-replay-window (fused), bounded intersecting ACK processing, ACK-above-highest-sealed ignored | **survives** | minus the shed mask; plus truncation (B.13). |
| Epoch death subsumed by liveness (`MAX_EPOCH_JUMP` refusal = generic decrypt failure; never chase epochs) | **survives unchanged** | |
| Roaming: authenticated-fresh-marked only; handshake packets never roam; accessor + `slither::roam` trace + `ToEndpoint::AddressMoved` | **survives re-derived** | roaming now also resets the congestion controller (B.12) — new wiring. |
| Liveness-neutral seal set (`seal_quiet`) | **survives extended** | credit frames, RESET_STREAM, CLOSE join ACKs/probes/retransmits in the quiet set (B.14). |
| Per-connection persistent keepalive (default 25 s) | **survives unchanged** | |
| Sans-io: two cores, str0m poll contract with `ToEndpoint` folded in, `Timeout(Option<Instant>)` terminal, named timers, idempotent `handle_timeout`, equal-deadline priorities, lateness `L`, RNG root+sub-seeds, plan-seal-commit, synchronous sealing, `Install{initial}` exactly-once, `HandshakeFailed` shell-only, `Retired` MUST, no-blocking invariant, accessors as shared-cell reads | **survives re-derived** | `ConnOutput::Event` grows stream/datagram variants (readable/writable/finished/reset, datagram received, message received); the timer table gains `AckDelay` and `CloseLinger`; per-stream waker keys in the shell (quinn's `blocked_readers/writers` pattern). Everything else verbatim. |
| Queued-sends-before-establish = ordinary sends (core exists from `connect()`) | **survives generalised** | early stream opens/writes/messages queue in stream state and pump on install; lost if connect fails, surfaced via `Connecting`. |
| Error taxonomy: `IntroError`, `AuthError` (`HandshakeFailed` the only security signal), `AcceptError{Stale,AlreadyConnected,EndpointDropped}`, `ConnectError{AlreadyConnected,TimedOut}`, `ConnectionLost`, `SendError`; trace targets `slither::{policy,replay,frames,roam}` | **survives extended** | `ConnectionLost` gains `PeerClosed{code,reason}` (B.11), `PeerRestarted` and `Replaced` (B.15). `SendError` becomes per-surface: stream write/finish errors, `DatagramTooLarge`, `MessageTooLarge`. Taxonomy details for the writer in F. |
| `send_unreliable` shares the DATA seq space; allocate-without-tracking | **dies** | DATA frames are gone; the unreliable path is the DATAGRAM frame, which has no sequence identity at all (app-level if needed). |
| `MAX_MESSAGE` 1159 handle check | **dies** | replaced by stream flow control; message sugar bounded by `INITIAL_MAX_STREAM_DATA` (B.8). |
| Peer-restart seq collision: silent confirmed loss, documented-not-fixed | **dies — fixed** | the CONTINUATION/CONTINUED flags make restart explicit and honest (B.15). |
| hiss Appendix A dependencies: split msg1 read, `next_counter()` | **survives extended** | plus one new item: a declared payload on msg2 (`[1]`) — see E.1. |
| DoS accounting table + 2-DH single-packet ceiling + no-amplification | **survives re-derived** | costs unchanged (payload byte doesn't change DH counts); msg2 grows to 108 B against a 197 B stimulus — still < 1 amplification. The writer re-tabulates with new sizes and adds the frame-layer rows (authenticated-peer violations ⇒ bounded CLOSE work). |

---

## D. [MAINTAINER] decisions

1. **Messages as sugar over uni streams; no DATA frame (B.8).** Buys one
   reliability engine and kills the message cap, at the cost of stream-state
   churn per message and messages bounded by the stream window (256 KiB). The
   alternative — a distinct reliable-message frame — preserves the tiny-RPC
   micro-optimum but doubles the retransmission surface forever.
2. **MAX_STREAMS credit frames rather than a fixed cap (B.10).** Diverges
   from the research recommendation because messages-as-streams makes stream
   churn the common case and QUIC's limit is cumulative. Costs two frame
   types and a replenishment rule; the alternative risks either connection
   death at a cumulative cap or close-timing ambiguity at a concurrent cap.
3. **Handshake payload change: msg1 `timestamp‖flags` (13 B), msg2 `flags`
   (1 B), CONTINUATION/CONTINUED semantics (B.15).** Fixes the known
   silent-confirmed-loss restart bug on the wire, at the cost of a
   handshake-byte change, one new rule in the continuation, and two new
   `ConnectionLost` variants. Declining it re-freezes a known data-loss bug
   for another wire generation.
4. **ACK policy moves from immediate to every-2nd + 25 ms + immediate-on-gap
   (B.17).** Halves reverse-path ACK traffic and matches the PTO formula's
   assumption, at the cost of one more timer and slightly laggier RTT
   samples. Immediate-ACK remains the conservative fallback.
5. **Replay window 2048 with the ACK still fused to it (B.13).** The
   spurious-retransmit/false-congestion exposure under long ACK-loss bursts
   at >100 Mbps is accepted for v1; the range-tracker split is deferred. A
   reviewer could reasonably demand the tracker now. Gate for revisiting:
   the Appendix B ACK-loss-burst simulation (E.4's validation obligation)
   quantifies the exposure before ratification hardens.
6. **cwnd reset on roaming as well as rekey; RTT survives both (B.12).**
   Conservative per RFC 9002/quinn precedent; a reviewer could argue for
   keeping cwnd across a same-NAT port rebind.
7. **CLOSE frame semantics (B.11):** 5 s linger, ≤1 reply/s, no close-ACK,
   violations get CLOSE-with-code. Minimal by design; the alternative
   (QUIC-faithful closing/draining with 3×PTO) is more state for little gain
   at slither's scale.
8. **Version 0x02 + prologue `slither\x02`, no negotiation ever (B.20).**
9. **mac1 fixed keyed-BLAKE2b across all suites; no suite byte on the wire;
   canonical static encoding = the `AsRef` octets (B.19).** Mismatched
   suites die as garbage — acceptable for mutually-configured peers, but it
   forecloses any future multi-suite endpoint on one socket. The encoding
   ruling moves P-256 mac1 keying from 33-B compressed to 65-B uncompressed
   (absorbed by the one-time golden regeneration) and requires the hiss
   `Curve::PublicKey: AsRef<[u8]> + Ord` bound — technically breaking for
   downstream `Curve` implementors, so it is a hiss semver ruling, not a
   patch assumption (Appendix A dependency 3).
10. **Stream-ID four-space QUIC encoding (B.7) and the flow-control initial
    constants** (`MAX_DATA` 1 MiB, per-stream 256 KiB, streams 32/128,
    batch 8): the values ship **ratified-but-revisitable**, gated on the
    Appendix B throughput validation (E.3's obligation), and the four-space
    choice trades slither-minimalism for QUIC congruence.
11. **`shed_mask`/`RECV_BUFFER` removal in favour of credit-as-backpressure
    (C).** Sound only because flow control is now load-bearing for memory
    safety; a reviewer should confirm no non-stream, non-datagram frame can
    force unbounded buffering (ACK/credit frames are O(1) to apply — believed
    yes).
12. **Deferrals ratified as a block:** STOP_SENDING, the BLOCKED family,
    pacing, ECN, CUBIC/BBR, header protection, PN truncation, PMTUD,
    cookies/mac2 (0x05 reserved, WireGuard under-load model remains the
    template), range-tracker ACK, per-peer `ss` precomputation.
13. **Carried SPEC-v2 [MAINTAINER] rulings re-affirmed wholesale** (they were
    individually flagged there and are adopted as decided): one-conn-per-static,
    staged names, no-fallback split-read gate, intro-queue DoS posture,
    own-bytes/`Superseded` removal, guard-eviction mitigations,
    membership-oracle restatement, swap-cut, re-home model, two-core sans-io
    shape.

---

## E. Research gaps — verification status

All six items were fact-checked (see `research-gaps.md`, 2026/08/13, against
the local hiss checkout — verified functionally identical to released 0.3.1:
clean tree, 2 metadata-only commits ahead of tag `v0.3.1`). None remains an
open fact question; two recast as post-implementation validation obligations.

1. **hiss msg2 payload support — CLOSED, supported as-is.** Released 0.3.1
   accepts `[N]` on the final message (`hiss-macros/src/parse.rs:262-282`;
   final-message position is not among the parser's three rejections), the
   generated final read returns `([u8; N], Transport)` with the tail-tag
   check firing before `into_transport` (`codegen.rs:1166-1198`, `:718-730`),
   and `tests/noise_cacophony.rs:343`/`:515-521` already exercise IK with
   `[16] … [15]` against frozen third-party vectors, reachable from the
   `v0.3.1` tag. B.15 and D.3 stand unchanged; no hiss change or release is
   needed for the flags. (Consequence is arithmetic only: `IK_MSG1_LEN`
   174 → 175, `IK_MSG2_LEN` 81 → 82 on the reference suite.)
2. **hiss split-read + `next_counter()` — CLOSED: not present; the gate
   stands.** Repo-wide search of 0.3.1 (= local tree) finds zero matches for
   `MidRead`/`read_message_1_intro`/`next_counter`, and hiss's own TODO
   schedules neither. The verifier's nuance — `ReadStyle::Verify` already
   hands the claimed static to a closure between `es` and `ss`
   (`codegen.rs:939-956`) — is settled here explicitly: **the `Claimed`
   stage must *suspend*, not merely decide.** `Claimed` is an app-held
   object (human-in-the-loop rejection is a motivating use case), parked
   across event-loop turns; the Verify closure only decides synchronously
   inside the read, so building `Claimed` on it would force re-paying `es`
   at `authenticate()` — a 3-DH accepted read that distorts the 1/2/4
   DH-cost ladder the staged accept is built on. The Verify-closure fallback
   is therefore **rejected** (the SPEC-v2 no-fallback ruling, carried in
   D.13), and Appendix A keeps the split read and `next_counter()` as gating
   hiss-0.3.x-minor dependencies.
3. **Flow-control and stream-limit initial values — RECAST as a
   post-implementation validation obligation.** 1 MiB / 256 KiB / 32 / 128 /
   batch-8 ship as **ratified-but-revisitable** constants; Appendix B
   carries a throughput sanity check of the window constants over FlakyWire
   on the paused clock, compared against quinn's shipped defaults. D.10
   references this gate.
4. **Fused-ACK spurious-loss rate — RECAST as a post-implementation
   validation obligation.** Appendix B carries the FlakyWire ACK-loss-burst
   simulation quantifying spurious-retransmit and false-congestion-event
   rates under the B.17 policy and the 2048-bit fused window. D.5 references
   this gate; the range-tracker split is the ready remedy if the numbers
   disappoint.
5. **Curve-generic mac1 encoding — SETTLED with a ruling (B.19 amended).**
   Verified blocking caveat: 0.3.1's `Curve::PublicKey` bound is `Clone`
   only (`curve/mod.rs:69`) — no generic byte access exists, and P-256
   carries two live encodings (65-B uncompressed storage, normalised per
   `p256/mod.rs:145-150`; 33-B compressed via `to_compressed()`/`Packed`),
   which v1 used for the wire and for mac1 respectively. Ruling adopted:
   add `AsRef<[u8]> + Ord` to the bound (Appendix A dependency 3; every
   shipped curve already conforms) and define the canonical encoding as the
   `AsRef` octets — mac1 keying, tie-break comparison, and identity maps all
   use it. P-256 mac1 keying deliberately moves to the 65-B uncompressed
   form; wire v2's one-time golden regeneration absorbs it.
6. **DATAGRAM queue policy — SETTLED with a ruling (§11 outline amended).**
   Precedent verified: quinn bounds both datagram queues by **bytes**
   (recv 1.25 MB head-drop with the newest always accepted,
   `connection/datagrams.rs:132-139`; send 1 MiB with caller-chosen
   drop-oldest or backpressure), never by count; an oversized datagram is
   connection-fatal there; RFC 9221 prescribes nothing. slither keeps
   **count bounds (64/64)**, argued in §11: crisp worst-case memory
   (64 × 1169 ≈ 75 KiB per queue), bounded entry count (a byte bound admits
   per-entry overhead blowup under a tiny-datagram flood — quinn's 1.25 MB
   of 1-byte datagrams is 1.25 M queue entries), and datagrams only arrive
   from the authenticated peer. Oversize cases ruled; drops counted and
   traced (quinn's silent drop is a known operability weakness — not
   copied).

---

## F. Spec outline

The writer follows this outline exactly. British English, Oxford comma,
dates `YYYY/MM/DD`, RATIFIED/DRAFT markers per house style. The document is
self-contained (no incorporation of the old SPEC by reference — it has zero
authority).

### §1 Status, scope, and the one break
Wire version 2 is a clean break from every prior slither wire; no
compatibility, no negotiation, unknown version/type ⇒ silent drop. Scope:
whole protocol — handshake, sealed packets, unified frames, streams, flow
control, congestion control, object model, sans-io core. Ratification
discipline: code matches spec; golden vectors regenerate once and re-freeze.

### §2 Crypto suites and `channel!`
hiss contracts (counter ownership, cleartext-counter necessity, stateless
recv, epoch ratchet + `MAX_EPOCH_JUMP` + commit-and-cap, `MAX_MESSAGE_LEN`,
reserved u64::MAX, per-direction counters — with the "wire must respect"
table). `channel!` declaration; IK only; reference suite
P256/ChaChaPoly/Blake2b; per-suite derived handshake sizes (formulas +
reference values); mac1 fixed keyed-BLAKE2b via cryptoxide (raw-primitive
rule); no suite identifier on the wire; `Noise_IK_..._...` name pinning.

### §3 Packet grammar
Type bytes, version, header layouts (Init 6 B, Resp 10 B, Data 14 B),
big-endian rule for header fields, whole-header-as-AD, sizes/caps
(`MAX_DATAGRAM`, `MAX_PLAINTEXT`, oversize rules), empty-plaintext keepalive,
reserved 0x04/0x05.

### §4 mac1
Key derivation (label ‖ recipient canonical static — the `AsRef` octets,
65-B uncompressed for P-256), 16-byte tag over all preceding bytes, verify
before any curve work, not a secret authenticator, per-suite genericity note
(fixed keyed-BLAKE2b for every suite).

### §5 Handshake
Prologue; msg1 payload `timestamp(12)‖flags(1)` and msg2 payload `flags(1)`
with CONTINUATION/CONTINUED semantics (B.15 rules verbatim); timestamp
confidentiality analysis carried; strictly-greater forcing
(endpoint-global); initiator rules (fresh initiation per retransmit, one
completion attempt per interval with length+index+mac1 preconditions, msg2
source ignored, give-up 90 s, rekey give-up silent); responder staged shape
(the DH ladder); the per-static greatest-timestamp guard.

### §6 Staged accept and initiation routing
SPEC-v2 §§4–5 re-derived on the new wire, structure preserved: the typestate
and DH-cost table (with the eager pre-read footnote); stage-0 queue constants
and rules (dedup, per-source both-tiers cap, evict-oldest, TTL,
own-bytes-on-consume, honesty clause); `accept()` re-home + `Stale`; the
routing rule (stage 0 → hint check → eager path/demotion → interception);
the internal continuation numbered order **with the new CONTINUATION step**
(tag → guard → continuation-flag routing → pacing → tie-break → admit);
restart handling (`Replaced`, fresh Intro); the post-`ss` lexicographic
tie-break (+ stream-parity consequence); pacing (50/s per static);
membership-oracle restatement; DoS accounting table (updated sizes, 2-DH
ceiling, no-amplification).

### §7 Session layer
Seal/open; counter = packet number (the reconciliation, per-direction
per-session spaces); replay window 2048 (RFC 6479, post-AEAD check-then-mark,
authenticated-marked-only drives liveness and roaming); roaming rule +
observability + CC reset hook; liveness model (`seal` vs `seal_quiet` sets —
the extended quiet set), keepalives (passive 10 s, persistent per-connection),
dead 15 s; rekey (send-triggered 120 s consult, 180 s backstop, silent swap,
swap-cut, make-before-break demux); the continuation-confirmed state
survival rules (B.15's reset/survive lists and the re-queue + credit
re-emission rules); epoch ratchet (65 536, straggler one epoch,
epoch-death-subsumed-by-liveness); nonce exhaustion ⇒
`ConnectionLost::NonceExhausted`.

### §8 Frame layer
Varint encoding (§16 verbatim); the frame table (types, fields,
ack-eliciting, retransmission classes: ranges / regenerate / never);
parse-then-apply, unknown/malformed ⇒ whole-packet drop; coalescing and
packing order; frames-to-packets is many-to-many; hiss
`MAX_MESSAGE_LEN`-derived per-seal bound note.

### §9 Streams
ID encoding (2 bits + 60-bit index, four spaces, parity fixed at
establishment); implicit open; send/recv half state machines (conceptual
states, quinn-collapsed implementation note); STREAM frame semantics (offset
ranges, FIN pins final size, `FINAL_SIZE_ERROR`); reassembly and
contiguous-prefix delivery; RESET_STREAM semantics (final_size true-up);
stream lifecycle and GC (send: all-ACKed or reset-ACKed; recv: read-to-final
or reset-read); message sugar (uni-stream mapping, no credit extension,
`MESSAGE_RECV_MAX`); STOP_SENDING reserved.

### §10 Flow control
Two levels, absolute offsets, monotone-max; initial constants; the
half-window re-grant rule; MAX_STREAMS cumulative model + batch rule;
violations (`FLOW_CONTROL_ERROR`, `STREAM_LIMIT_ERROR`) ⇒ CLOSE; credit is
the buffer commitment (the backpressure story — replaces shed/RECV_BUFFER);
DATAGRAM exemption; retransmissions consume no new credit; rekey
re-emission rule.

### §11 Datagrams
DATAGRAM frame; single-packet bound (`MAX_DATAGRAM_PAYLOAD` derived);
ack-eliciting but never retransmitted; cwnd-gated, flow-control-exempt; no
delivery/order promise. Queue discipline (ruled, E.6): both queues bounded
by **count** (`DATAGRAM_SEND_QUEUE`/`DATAGRAM_RECV_QUEUE` = 64 each),
drop-oldest with the newest always accepted (quinn's receive discipline).
Count-not-bytes is a deliberate divergence from quinn's byte bounds
(1.25 MB/1 MiB), argued: worst-case memory is crisply
count × `MAX_DATAGRAM_PAYLOAD` ≈ 75 KiB per queue per connection, entry
count is bounded (a byte bound admits allocation-churn amplification from a
tiny-datagram flood), and datagrams originate only from the authenticated
peer; the constant is ratified-but-revisitable. Oversize rules: send side —
payload > `MAX_DATAGRAM_PAYLOAD` returns `DatagramError::TooLarge` at the
handle, before any queue; receive side — an oversized DATAGRAM frame is
**impossible by construction** (a frame's data lies inside one sealed
packet's plaintext ≤ `MAX_PLAINTEXT`, and a length field overrunning the
plaintext is a malformed frame ⇒ whole-packet drop per §8), so no receiver
oversize rule exists — quinn's connection-fatal case is unrepresentable.
Every queue-overflow drop increments a counter surfaced on the
`slither::frames` trace target (deliberately better than quinn's silent
drop).

### §12 ACK
Frame encoding (all-varint, µs ack_delay, no exponent); descending-range
semantics; fused window derivation + newest-first truncation at
`MAX_ACK_RANGES`/capacity; delayed-ACK policy (every 2nd, 25 ms,
immediate-on-gap); ACKs non-ack-eliciting, untracked, cwnd-exempt; bounded
intersecting processing; above-highest-sealed ⇒ ignore whole; over-cap ⇒
malformed packet.

### §13 Loss recovery
RFC 9002 §5 RTT (ack_delay subtraction capped at `MAX_ACK_DELAY`, min_rtt
rule); §6.1 packet + time thresholds; §6.2 PTO (formula, doubling, cap,
reset-on-ack); probe content (pending retransmittable oldest-first, else
PING); retransmit frames never packets, ranges re-framed on fresh counters;
sent-map `size` field; what resets on rekey vs roaming (B.15/B.12 split).

### §14 Congestion control
Controller trait seam (methods, wiring points); NewReno (initial/min window,
0.5 reduction, ABC, recovery period one-cut rule); persistent congestion in
the loss walk (3×PTO, RTT-sample precondition); bytes_in_flight accounting;
admission gate + PTO-probe and non-ack-eliciting exemptions; app_limited;
resets (rekey, roaming; RTT survives as prior); explicitly out: pacing, ECN,
CUBIC/BBR (trait-additive later).

### §15 CLOSE and connection lifecycle
CLOSE frame + linger/drain rules (B.11); error-code registry; the full
teardown matrix (liveness, backstop, nonce exhaustion, local close, peer
CLOSE, peer restart/Replaced, endpoint drop) with events/errors and what is
transmitted (mostly: nothing).

### §16 Object model and the sans-io core
Shell surface (`Endpoint::{builder, connect, accept}`, staged verbs,
`Connection::{open_bi, open_uni, accept_bi, accept_uni, send_message,
recv_message, send_datagram, recv_datagram, close, accessors}`, stream
handles read/write/finish/reset); drop semantics; driver/handle lifetimes;
the two cores and their concrete surfaces (`handle_datagram`,
`handle_timeout`, `poll_output` → `Transmit`/`Event`/`ToEndpoint`/`Timeout`
terminal, `Disposition`, `Install{initial}` exactly-once, `HandshakeFailed`
shell-only, `Retired` MUST); named-timer tables (connection: Keepalive,
PersistentKeepalive, Liveness, Loss, Pto, AckDelay, CloseLinger; endpoint:
retransmit/give-up/intro-expiry min); equal-deadline priorities; idempotent
timeouts; lateness bound `L`; RNG root + sub-seeds; injected clock service;
plan-seal-commit + synchronous sealing; no-blocking invariant; output
ordering normative; paused-clock testability requirement (two endpoints over
FlakyWire, no kernel).

### §17 Endpoint-global state
Timestamp guard (pinned/orphan, caps, no-orphan-on-reject, admission-only
LRU, aging, honesty clause); `last_init_timestamp`; index tables +
re-draw rule + inert-stale-traffic corollary; static→connection + hint map;
state-ceilings table (updated: intro tiers, mid-states/provider handles,
guard, per-connection memory now = flow-control windows, ~1 MiB + stream
state, application-governed connection count).

### §18 Errors and observability
Full taxonomy: `ConnectError{AlreadyConnected, TimedOut}`;
`IntroError{Expired, Internal, Malformed, EndpointDropped}`;
`AuthError{Replay, HandshakeFailed, Expired, EndpointDropped}` (security
signal note); `AcceptError{Stale, AlreadyConnected, EndpointDropped}`;
`ConnectionLost{TimedOut, RekeyFailed, NonceExhausted, LocallyClosed,
PeerClosed{code, reason}, PeerRestarted, Replaced, EndpointDropped}`; stream
errors (`WriteError{Stopped?, Reset, ConnectionLost, Finished}`,
`ReadError{Reset(code), ConnectionLost}`), `MessageError{TooLarge, …}`,
`DatagramError{TooLarge, …}`; wire error codes (0x00 NO_ERROR, 0x01
PROTOCOL_VIOLATION, 0x02 FLOW_CONTROL_ERROR, 0x03 STREAM_LIMIT_ERROR, 0x04
STREAM_STATE_ERROR, 0x05 FINAL_SIZE_ERROR, ≥0x10 application); trace targets
`slither::{policy, replay, frames, roam}` as operator contract —
`slither::frames` carries the datagram queue-overflow drop counters (§11).

### §19 Out of scope / deferred
The D.12 block, each with one-line pointer to its future home; plus
persistence, reflector/mDNS/probes, PSK, bubble integration.

### Appendix A — hiss dependencies
Three gating items for one hiss 0.3.x minor, plus one confirmed non-item:

1. **The split msg1 read** (`read_message_1_intro` → `Mid` →
   `complete()`, option-(a) owned-tail carrier) — required because the
   `Claimed` stage must **suspend** (app-held object, human-in-the-loop
   decisions, parked across event-loop turns). The shipped
   `read_message_1_with` Verify closure is **explicitly rejected as a
   fallback**: it decides synchronously inside the read, so `Claimed` built
   on it would re-pay `es` at `authenticate()` (a 3-DH accepted read),
   distorting the 1/2/4 DH-cost ladder — the SPEC-v2 no-fallback ruling,
   carried (E.2).
2. **`DatagramSend::next_counter()`** — the AD header must be built before
   sealing; 0.3.1 exposes no counter accessor (verified absent).
3. **`Curve::PublicKey: AsRef<[u8]> + Ord`** bound (0.3.1 bounds it `Clone`
   only) — underpins the canonical-encoding ruling (B.19/E.5); every shipped
   curve already conforms, but the bound tightening is technically breaking
   for downstream `Curve` implementors, so it is a hiss semver ruling.

Non-item: the msg2 `[1]` payload is **supported by released 0.3.1 as-is**
(E.1 citations) — no dependency. Carry the `SymmetricState` `h`-zeroing wart
note.

### Appendix B — test obligations
Golden vectors regenerated once for wire v2 (new prologue/payloads) then
frozen; size/constant compile-time asserts; DH-cost pins (1 reject / 2
accept / 4 total); staged-accept queue obligations (carried list from
SPEC-v2 App B); continuation-flag matrix (rekey, restart-initiator,
restart-responder, mismatch ⇒ `PeerRestarted`); tie-break both orderings +
forgery-cannot-cancel + stream-parity stability across rekey; varint
round-trips; frame-table round-trips + unknown-type whole-packet drop;
stream reassembly/reorder/FIN/final-size violations; flow-control credit
stall-and-resume, violation ⇒ CLOSE; MAX_STREAMS replenishment; message
sugar exactly-once + 256 KiB bound; DATAGRAM drop/no-retransmit; delayed-ACK
policy timing; window-2048 admit/duplicate/edge; ACK truncation newest-first;
loss/PTO staircase on paused clock; NewReno slow-start/avoidance/recovery/
persistent-congestion; cwnd gate + PTO exemption; rekey state-survival
matrix (streams/credit survive, recovery/CC reset, re-queue + credit
re-emission); roaming CC reset; CLOSE linger + reply rate; datagram
queue-overflow drop-oldest + counter emission; **the ACK-loss-burst
simulation** (FlakyWire, paused clock: sustained ACK-loss bursts against the
2048-bit fused window under the every-2nd ACK policy, quantifying
spurious-retransmit and false-congestion-event rates — the D.5/E.4 gate);
**the window-constants throughput sanity check** (flow-control and
stream-limit initials sustain the target rates without stall — the D.10/E.3
gate); all flow tests on FlakyWire + paused clock.

### Named-constants table (complete; reference suite where suite-dependent)

| Constant | Value | Home |
|---|---|---|
| `VERSION` | 0x02 | §3 |
| `PROLOGUE` | `b"slither\x02"` | §5 |
| `PKT_HANDSHAKE_INIT` / `PKT_HANDSHAKE_RESP` / `PKT_DATA` | 0x01 / 0x02 / 0x03 | §3 |
| reserved packet types | 0x04 (unused), 0x05 (cookie/mac2) | §3 |
| `INIT_HEADER_LEN` / `RESP_HEADER_LEN` / `DATA_HEADER_LEN` | 6 / 10 / 14 B | §3 |
| `MAC1_LABEL` / `MAC1_LEN` | `b"slither mac1"` / 16 B | §4 |
| `TIMESTAMP_LEN` / `HS_FLAGS_LEN` | 12 / 1 B | §5 |
| `MSG1_PAYLOAD_LEN` / `MSG2_PAYLOAD_LEN` | 13 / 1 B | §5 |
| `FLAG_CONTINUATION` / `FLAG_CONTINUED` | bit 0x01 (msg1 / msg2 flags) | §5 |
| `IK_MSG1_LEN` / `IK_MSG2_LEN` (reference suite) | 175 / 82 B | §2 |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` (reference suite) | 197 / 108 B | §2 |
| `AEAD_TAG_LEN` | 16 B | §2 |
| `MAX_DATAGRAM` / `MAX_PLAINTEXT` | 1200 / 1170 B | §3 |
| `REKEY_EPOCH_MSGS` / `MAX_EPOCH_JUMP` | 65 536 / 2 (hiss-fixed) | §7 |
| `REPLAY_WINDOW` | 2048 bits | §7 |
| frame types | table in §8 (0x00, 0x01, 0x02, 0x04, 0x08–0x0f, 0x10–0x13, 0x1c, 0x30/0x31; 0x05 reserved) | §8 |
| `STREAM_OFF` / `STREAM_LEN` / `STREAM_FIN` bits | 0x04 / 0x02 / 0x01 | §8 |
| `INITIAL_MAX_DATA` | 1 048 576 B (1 MiB) | §10 |
| `INITIAL_MAX_STREAM_DATA` | 262 144 B (256 KiB) | §10 |
| `INITIAL_MAX_STREAMS_BIDI` / `_UNI` | 32 / 128 (cumulative) | §10 |
| `STREAMS_CREDIT_BATCH` | 8 | §10 |
| credit re-grant threshold | ½ window consumed | §10 |
| `MESSAGE_RECV_MAX` | = `INITIAL_MAX_STREAM_DATA` | §9 |
| `MAX_DATAGRAM_PAYLOAD` | 1169 B (derived: `MAX_PLAINTEXT` − 1) | §11 |
| `DATAGRAM_SEND_QUEUE` / `DATAGRAM_RECV_QUEUE` | 64 / 64 (count bound; drop-oldest, newest always accepted; ≈ 75 KiB worst case each) | §11 |
| `CLOSE_REASON_MAX` | 256 B | §8 |
| `CLOSE_LINGER` / close-reply rate | 5 s / ≤ 1 per s | §15 |
| `MAX_ACK_RANGES` | 64 | §12 |
| ACK policy | every 2nd ack-eliciting, `MAX_ACK_DELAY` cap, immediate on gap | §12 |
| `MAX_ACK_DELAY` | 25 ms | §12/§13 |
| `K_PACKET_THRESHOLD` / time threshold / `K_GRANULARITY` | 3 / 9⁄8 / 1 ms | §13 |
| `K_INITIAL_RTT` / `PTO_BACKOFF_CAP` | 333 ms / 2⁶ | §13 |
| `INITIAL_WINDOW` / `MINIMUM_WINDOW` | 12 000 / 2 400 B | §14 |
| `LOSS_REDUCTION_FACTOR` / `PERSISTENT_CONGESTION_THRESHOLD` | 0.5 / 3 | §14 |
| `RETRANSMIT_BASE` / `RETRANSMIT_JITTER_MAX` | 5 s / 333 ms | §5 |
| `HANDSHAKE_GIVEUP` | 90 s | §5 |
| `KEEPALIVE_TIMEOUT` / `DEAD_TIMEOUT` | 10 s / 15 s | §7 |
| `PERSISTENT_KEEPALIVE` (default, per-connection) | 25 s | §7 |
| `REKEY_AGE` / `REJECT_AGE` | 120 s / 180 s | §7 |
| `INITIATIONS_PER_SECOND` (pacing, per static) | 50 (20 ms spacing) | §6 |
| `INTRO_QUEUE_CAP` / `INTRO_MAX_PER_SOURCE` / `INTRO_TTL` | 1024 / 4 / 15 s | §6 |
| `TS_GUARD_ORPHAN_CAP` | 1024 | §17 |
| `L` (shell lateness bound) | 250 ms | §16 |
| wire error codes | 0x00–0x05 + ≥0x10 app (§18 registry) | §18 |
| session index | nonzero u32, random, re-draw across both tables | §17 |

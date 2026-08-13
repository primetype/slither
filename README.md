# slither

A WireGuard-shaped Noise-over-UDP packet layer — an authenticated, encrypted,
**unreliable** datagram session between two peers — with a QUIC-shaped
**reliable frame layer** (Leg 2, ratified) riding inside the sealed packets.

slither borrows WireGuard's homework — a cheap mac1 DoS gate, fresh-ephemeral
handshake retransmission, an anti-replay sliding window, endpoint roaming, and the
keepalive/liveness/rekey timers — but the cryptography is Bubble's: the
[`hiss`](https://crates.io/crates/hiss) Noise **IK** handshake over
**P-256 / ChaCha20-Poly1305 / BLAKE2b** and its out-of-order datagram transport,
with keyed **BLAKE2b** for mac1 taken from `cryptoxide` directly. It is the
QUIC-style "framing over a Noise channel instead of TLS" direction, at the packet
layer.

## Lineage

- **WireGuard-lite** — the handshake shape (IK, an initiation timestamp, a mac1
  cookie gate), the anti-replay window (RFC 6479), endpoint roaming, and the
  timers are WireGuard's, adapted.
- **QUIC-style frames** — the reliable frame layer (frames, ACK ranges, RFC
  9002 loss detection + PTO — retransmit frames, never packets) is **Leg 2**,
  riding ON TOP of this unreliable, authenticated packet layer. Built and
  **ratified** in `SPEC.md` §9; congestion control, streams, and fragmentation
  stay reserved.

## The protocol in one paragraph

The dialling peer runs the Noise IK handshake as the initiator (the responder's
static is pre-known), retransmitting a **completely fresh initiation** — new
ephemerals, index, and a strictly-greater timestamp, carried **encrypted** as
msg1's Noise payload — every ~5 s until it establishes or gives up at 90 s. The responder verifies a keyed-BLAKE2b **mac1**
before any curve work (the DoS gate), authenticates the initiator's static,
checks it against an **allow-list** (the family-devices set) and a **per-static
greatest-timestamp** replay guard, then replies. Both sides convert the completed
handshake into `hiss`'s datagram transport: every Data packet carries the
hiss-owned monotonic counter in its 14-byte header (which is also the AEAD
associated data), and the receiver runs a 128-bit sliding **replay window**. An
authenticated packet from a new source **roams** the session to it; nothing
unauthenticated ever does. Idle sessions exchange 10 s keepalives; a session that
sends into 15 s of silence is declared dead; a session past 120 s rekeys on its
next send and is refused outright at 180 s.

On top of that packet layer, the Leg 2 **frame layer** (ratified) makes
`send` a **reliable, unordered, exactly-once message**: each message is a
sequence-numbered DATA frame, ACKed via ranges built from the replay window,
retransmitted (RFC 9002 packet/time thresholds + PTO) on fresh counters until
acknowledged, and deduplicated on the receiver. Reliability lives within the
connection — what a dead connection had not delivered is lost.

## Usage sketch

```rust,ignore
use slither::endpoint::{Config, Endpoint};
use slither::handshake::SoftwareIdentity;

// Inside a tokio current-thread runtime + LocalSet (the actor is `!Send`):
let identity = SoftwareIdentity::from_scalar(my_static_scalar, my_rng)?;
let socket = tokio::net::UdpSocket::bind("0.0.0.0:51820").await?;
let config = Config::new().allow(&family_device_static);
let mut endpoint = Endpoint::start(identity, socket, config);

let session = endpoint.connect(peer_addr, peer_static);
session.send(b"hello".to_vec())?;             // reliable, unordered, exactly-once

while let Some(event) = endpoint.next_event().await {
    // Established, Incoming { payload, .. }, Dead, EndpointMoved, Failed
}
```

## Testability

The socket sits behind a small `Wire` trait, so the whole protocol is drivable
without a kernel. The tests run two endpoints over an in-memory
`FlakyWire` (loss, reorder, duplication, delay, partitioning) on tokio's **paused
clock**, so the 5 s / 15 s / 90 s / 120 s / 180 s timers resolve in virtual time;
one test uses a real UDP loopback socket.

## Status

**Leg 1 — the sealed packet layer:** every wire constant is **ratified**
(2026/07/16) and frozen in [`SPEC.md`](SPEC.md) §§1–8. **Leg 2 — the reliable
frame layer:** built and **ratified** (2026/07/17), every frame layout and
constant frozen in [`SPEC.md`](SPEC.md) §9. slither has **zero
`bubble-*` dependencies** and is extractable to its own repo by deleting one
line from the workspace manifest.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

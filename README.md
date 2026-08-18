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
  9002 loss detection + PTO — retransmit frames, never packets — and NewReno
  congestion control) is **Leg 2**, riding ON TOP of this unreliable,
  authenticated packet layer. Built and **ratified** in `SPEC.md` §§9, 14:
  streams, reliable messages, unreliable datagrams and congestion control
  all ship in v1. Pacing, ECN and alternate controllers (CUBIC/BBR) stay
  reserved (§14.7).

## The protocol in one paragraph

The dialling peer runs the Noise IK handshake as the initiator (the responder's
static is pre-known), retransmitting a **completely fresh initiation** — new
ephemerals, index, and a strictly-greater timestamp, carried **encrypted** as
msg1's Noise payload — every ~5 s until it establishes or gives up at 90 s. The responder verifies a keyed-BLAKE2b **mac1**
before any curve work (the DoS gate), authenticates the initiator's static
and checks a **per-static greatest-timestamp** replay guard, then replies —
whether to *admit* that static at all is the application's call, made via
the staged accept ladder (below), not a slither-held allow-list. Both sides convert the completed
handshake into `hiss`'s datagram transport: every Data packet carries the
hiss-owned monotonic counter in its 14-byte header (which is also the AEAD
associated data), and the receiver runs a 128-bit sliding **replay window**. An
authenticated packet from a new source **roams** the session to it; nothing
unauthenticated ever does. Idle sessions exchange 10 s keepalives; a session
that sends into 25 s of silence is declared dead; a session rekeys after
65 536 (2¹⁶) messages in the current epoch (§7.7), not on a wall-clock timer.

On top of that packet layer, the Leg 2 **frame layer** (ratified) makes
`send` a **reliable, unordered, exactly-once message**: each message is a
sequence-numbered DATA frame, ACKed via ranges built from the replay window,
retransmitted (RFC 9002 packet/time thresholds + PTO) on fresh counters until
acknowledged, and deduplicated on the receiver. Reliability lives within the
connection — what a dead connection had not delivered is lost.

## Usage sketch

```rust,ignore
use slither::{Config, Endpoint};
use slither::identity::SoftwareIdentity;

// Inside a tokio current-thread runtime + LocalSet (the actor is `!Send`):
let identity = SoftwareIdentity::from_scalar(my_static_scalar, my_rng)?;
let socket = tokio::net::UdpSocket::bind("0.0.0.0:51820").await?;
let endpoint: Endpoint<_> = Endpoint::builder()
    .identity(identity)
    .wire(socket)
    .config(Config::new())
    .build();

// Dial: connect() is sync (0 DH so far); the returned `Connecting` future
// is what spends the 2 initiator DH and resolves once the handshake lands.
let connection = endpoint.connect(peer_addr, peer_static)?.await?;
connection.send_message(b"hello").await?;   // reliable, unordered, exactly-once

// Accept: a staged ladder, so the app can inspect a claimed identity
// before spending a DH on it. `accept()` is driven in a loop.
while let Some(intro) = endpoint.accept().await {
    let claimed = intro.read_identity().await?;      // 1 DH
    if !my_allow_list.contains(claimed.claimed_static()) {
        continue; // dropping `claimed` is the silent reject
    }
    let proven = claimed.authenticate().await?;       // 2 DH
    let connection = proven.accept().await?;
    // connection.recv_message().await, connection.notified().await, ...
}
```

There is no `examples/` directory in the tree today; if one is added, this
section should link it rather than repeat the sketch inline.

## Testability

The socket sits behind a small `Wire` trait, so the whole protocol is drivable
without a kernel. The tests run two endpoints over an in-memory
`FlakyWire` (loss, reorder, duplication, delay, partitioning) on tokio's
**paused clock**, so the 5 s / 10 s / 25 s / 90 s timers resolve in virtual
time; one test uses a real UDP loopback socket (`tests/spec_shell.rs`).

## Status

**Leg 1 — the sealed packet layer:** every wire constant is **ratified**
(2026/07/16) and frozen in [`SPEC.md`](SPEC.md) §§1–8. **Leg 2 — the reliable
frame layer:** built and **ratified** (2026/07/17), every frame layout and
constant frozen in [`SPEC.md`](SPEC.md) §9. slither is an **independent
crate** with zero `bubble-*` dependencies — everything it needs resolves from
crates.io.

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

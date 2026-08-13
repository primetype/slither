# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Initial release, extracted from the bubble-reboot workspace (2026/08/13)
into its own repository.

### Added

- **Leg 1 — the sealed packet layer** (ratified 2026/07/16, frozen in
  `SPEC.md` §§1–8): a WireGuard-shaped Noise-over-UDP datagram session.
  The Noise **IK** handshake over **P-256 / ChaCha20-Poly1305 / BLAKE2b**
  (via [`hiss`](https://crates.io/crates/hiss) 0.3.1, `noise!`-declared,
  with the 12-byte timestamp riding the encrypted msg1 payload), the keyed
  BLAKE2b **mac1** DoS gate, fresh-ephemeral handshake retransmission, the
  RFC 6479-shaped anti-replay window, endpoint roaming, and the WireGuard
  timer set (retransmit 5 s, give-up 90 s, keepalive 10 s, dead 15 s,
  rekey 120 s), plus the Noise §11.3 key ratchet.
- **Leg 2 — the reliable frame layer** (ratified 2026/07/17, `SPEC.md` §9):
  reliable, unordered, exactly-once messages inside the sealed packets —
  DATA and ACK-range frames, RFC 9002 loss detection and PTO, no congestion
  control (out of scope for v1, with streams and fragmentation).
- **The endpoint actor**: one UDP socket behind a `Wire` trait, driven by a
  single `!Send` tokio task (`LocalSet`); sessions surface as an `Event`
  stream, inbound handshakes are gated by an allow-list of remote statics.
- **`testutil`** (feature `test-util`): the `FlakyWire` in-memory network
  (loss, reorder, duplication, delay, partition) and a deterministic
  counting identity, so a consumer can drive the whole protocol without a
  kernel — the suite slither's own paused-clock tests run on.

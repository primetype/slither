# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-08-20

### Fixed

- **Dark-mode diagrams on the crates.io page.** crates.io rewrites a
  README's relative `<img src>` to the repository's raw URL but leaves
  `<picture><source srcset>` untouched, so 0.2.0's two dark-theme SVG
  variants resolved against the crate page's own URL and 404'd for
  dark-mode readers. The two `srcset` URLs are now absolute, in the same
  `raw/HEAD` form crates.io generates for the light halves. No code
  change.

## [0.2.0] - 2026-08-20

A clean rewrite against `SPEC.md`, now **ratified** as slither's v1
wire (80 rulings across ten rounds, `cd12ed7`). Replaces the
pre-rewrite design described below in its entirety — different object
model, different handle API, different timers. See `SPEC.md` for the
specification; the ruling-by-ruling design record (280+ decisions) is
kept by the maintainers outside the repository.

### Added

- **Two sans-io cores plus one shell.** `core::Endpoint<I: Identity>`
  and `core::Connection` are pure state machines — `now: Instant` is an
  argument on every mutating call, neither reads a clock — driven by a
  single `!Send` shell actor (`tokio::task::spawn_local`, one `Wire`
  trait for the socket seam), so the whole protocol is drivable without
  a kernel on tokio's paused clock.
- **A. Connection lifecycle** — dial and close (`Endpoint::connect`,
  `Connection::close`/`closed`), a dial that never answers times out and
  reports why, a second `connect()` to a live peer is refused rather
  than silently superseding it, simultaneous dial-dial resolves to one
  connection, and an idle connection with no traffic is reaped rather
  than held open for free.
- **B. Inbound admission — the staged accept.** A four-rung ladder
  (`Intro` → `Claimed` → `Proven` → `Connection`, 0/1/2 DH) so an
  application can reject an inbound identity, or park the decision
  across event-loop turns, before spending a DH on it; a flood of
  inbound initiations does not disturb established connections.
- **C. Data transfer** — reliable unordered exactly-once messages
  (`send_message`/`recv_message`), multiple concurrent streams with no
  head-of-line blocking (`open_bi`/`open_uni`/`accept_bi`/`accept_uni`),
  stream abandonment without killing the connection, unreliable
  datagrams (`send_datagram`/`recv_datagram`), and backpressure via
  flow-control credit rather than unbounded buffering.
- **D. Mobility** — a connection survives the peer changing network or
  our own address changing (NAT rebind), and a peer that restarts gets
  a working connection back, all via authenticated-only roaming.
- **E. Identity and crypto** — the `Identity`/`DhProvider` seam admits a
  hardware-backed static key (no `Send` bound anywhere on the driver
  path), a pluggable crypto suite with fail-closed mismatches, and
  silent long-lived rekeying (message-count epochs, §7.7).
- **A second offered cipher suite** (ruling 279):
  `P256 / AesGcm / Blake2b` — Noise §12.4 AES-256-GCM via hiss 0.4.0,
  `AesGcm` re-exported in the prelude, declared with `channel!` like any
  suite. Same wire lengths as the reference suite in every packet;
  measured **1.63×** the reference's end-to-end stream throughput where
  the hardware carries AES (aarch64 with `target_feature="aes"` — Apple
  Silicon by default). The reference suite is unchanged: ChaCha20-Poly1305
  stays the portable default, fast and constant-time on every target
  (x86-64 and wasm have no cryptoxide AES hardware path today).
  `benches/throughput.rs` runs both suites; mismatched same-curve
  deployments fail closed at the identity probe (§2.2 as amended).
- **F. Operational** — the whole protocol drivable without a kernel
  (`testutil::FlakyWire` on tokio's paused clock), a caller-supplied
  `Wire` with explicable send failures, and clean teardown on dropping
  every handle.
- **G/H — death, contest and redial.** `Connection::closed()`/
  `notified()` for death/roam/contest events, `acked()` to wait for
  send-and-close, immediate redial after giving up on a dial, and loud
  (not silent) failure when an application mixes `send_message` with
  uni streams.
- **I. Composability** (`compat`, ratified ruling 209) —
  `AsyncRead`/`AsyncWrite` over a stream, a `Sink`/`Stream`-backed codec
  surface, and a `tower::Service` adapter.
- **J. Liveness of the accept loop** (ruling 252) — a responder that
  keeps calling `accept()` survives a lost msg2.
- **RFC 9002 loss recovery and NewReno congestion control** (`SPEC.md`
  §§13–14), fully implemented — not deferred, contrary to the previous
  `[Unreleased]` entry below.

### Changed

- Endpoint construction moved from a one-shot `start()` (below) to
  `Endpoint::builder().identity(..).wire(..).config(..).build()`.
- Session events moved from a single `Event` stream to per-connection
  `notified()`/`closed()` plus the staged-accept ladder.
- Peer admission moved from a `Config`-level allow-list to an
  application-driven decision mid-ladder (`Claimed::claimed_static()`).
- `hiss` pinned to `0.4.0` (was `0.3.1`), `cryptoxide` floor moved to
  `0.6.3` in lockstep. Your crate's **own** `hiss` dependency (the
  `channel!` expansion needs it) must sit on the same `0.4` line — two
  hiss majors in one graph will not type-check.
- **Documentation overhaul** (round 44, ruling 276): the README rewritten
  why-first (status block, install, requirements, quickstart, limits, two
  SVG diagrams); the crate docs how-first with a compile-tested quickstart
  and five new API examples; a runnable `examples/echo.rs`; the shell's
  module prose folded into the rendered `shell` page.
- **`slither::prelude` replaces the flat crate root** (ruling 278). One
  glob — `use slither::prelude::*;` — carries the golden path: `block_on`,
  `Config`, `Identity`/`SoftwareIdentity`, `Wire`, the handles, the staged
  ladder, the stream types, `Notification`, and hiss's `P256`/`ChaChaPoly`/
  `AesGcm`/`Blake2b` so the same line covers a `channel!` declaration. The root now
  keeps only the eight modules, `prelude`, `channel!`, `pub use hiss` and
  the four signature-reachability types (`SessionId`, `Dir`, `StreamId`,
  `Timestamp`); every other name has **one blessed spelling, at its
  module** — `slither::error::ReadError`, `slither::config::WallClock`,
  `slither::packet::Channel`. The ten error types stay module-only and are
  deliberately not in the prelude. The prelude itself changes only by
  ruling.
- **Building an endpoint outside a `LocalSet` now panics with slither's
  own message**, naming `slither::prelude::block_on` and the fix —
  previously tokio's bare `spawn_local` message, which named no slither
  symbol.
- **The lib compiles for `wasm32-wasip1`** (ruling 277), including
  `test-util`'s in-memory `FlakyWire`: `tokio/net` is now a
  target-conditional dependency (`cfg(not(target_family = "wasm"))`) and
  the `impl Wire for tokio::net::UdpSocket` carries the same `cfg`, so it
  exists everywhere except wasm. Native builds resolve identically —
  cargo unions base and target features. There is no UDP in wasm; the
  `Wire` seam is how a wasm host supplies its own transport.

### Removed

- `examples/udp_loopback.rs` — the v0.1 example was deleted with the
  v0.1 wire (slice 0); `examples/echo.rs` (round 44) is its replacement.

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
- **`examples/udp_loopback.rs`**: two endpoints over real UDP loopback
  sockets — handshake, one reliable message each way — runnable with
  `cargo run --example udp_loopback`.
- **`testutil`** (feature `test-util`): the `FlakyWire` in-memory network
  (loss, reorder, duplication, delay, partition) and a deterministic
  counting identity, so a consumer can drive the whole protocol without a
  kernel — the suite slither's own paused-clock tests run on.

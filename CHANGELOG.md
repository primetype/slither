# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-18

A clean rewrite against `SPEC.md`, now **ratified** as slither's v1
wire (80 rulings across ten rounds, `cd12ed7`). Replaces the
pre-rewrite design described below in its entirety — different object
model, different handle API, different timers. See `SPEC.md` and
`.spec-v2-clean-slate/rulings.md` for the specification and the record
of why.

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
- `hiss` pinned to `0.3.2` (was `0.3.1`).
- **Documentation overhaul** (round 44, ruling 276): the README rewritten
  why-first (status block, install, requirements, quickstart, limits, two
  SVG diagrams); the crate docs how-first with a compile-tested quickstart
  and five new API examples; a runnable `examples/echo.rs`; the shell's
  module prose folded into the rendered `shell` page.
- **Building an endpoint outside a `LocalSet` now panics with slither's
  own message**, naming `slither::block_on` and the fix — previously
  tokio's bare `spawn_local` message, which named no slither symbol.

### Removed

- `examples/udp_loopback.rs` — not present in the current tree; either
  restore it against the new API or drop the README's reference to it.

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

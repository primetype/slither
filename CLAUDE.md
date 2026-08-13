# slither — project instructions

## The spec is frozen (hard rule)

`SPEC.md` is **ratified**: Leg 1 (the sealed packet layer, §§1–8, 2026/07/16)
and Leg 2 (the reliable frame layer, §9, 2026/07/17). Every wire constant,
layout, timer value and behaviour in it is frozen, and **the code must match
the spec** — never the other way round. Do not change a ratified constant, a
header layout, a frame type, or a timer without an explicit ratification
decision from the maintainer recorded in `SPEC.md`.

The bytes are pinned by
`handshake::tests::golden_wire_is_byte_identical_to_the_pre_migration_driver`
and the size/constant tests across the crate: any change that moves a wire
byte turns a test red. That is by design — treat such a red as "this needs a
ruling", not "update the expectation".

## Crypto rules

- **Every Noise/curve operation flows through `hiss`.** slither declares its
  IK handshake via the `noise!` macro and rides the datagram transport
  (`DatagramSend`/`DatagramRecv`); it never touches curve or AEAD primitives
  itself.
- **The one raw primitive is mac1's keyed BLAKE2b**, taken from `cryptoxide`
  directly (the raw-primitive rule: it is a keyed hash over public data, not
  session cryptography). slither's `cryptoxide` requirement is pinned to
  **exactly the range hiss uses** (`>=0.6.0, <0.7` as of hiss 0.3.1) —
  verify against hiss's Cargo.toml when bumping either.
- **`rand_core` must match the line hiss's public bounds name** (0.10 as of
  hiss 0.3.1; `hiss::rand_core` re-exports it). Two rand_core majors in one
  graph produce an unsatisfiable `CryptoRng` bound, not a version error.
- **No RustCrypto crates** for any slither cryptography.

## Architecture invariants

- The endpoint is a **single `!Send` actor** behind the `Wire` trait, spawned
  with `tokio::task::spawn_local`; consumers run it on a current-thread
  runtime inside a `LocalSet`. Do not add `Send` bounds to the actor path —
  a DH provider is not required to be `Send`.
- The whole protocol is **drivable without a kernel**: tests run two
  endpoints over `testutil::FlakyWire` on tokio's **paused clock** (the
  5 s/15 s/90 s/120 s timers resolve in virtual time). New behaviour gets a
  paused-clock flow test, not a sleep.
- `slither` is independent: **zero `bubble-*` dependencies**, everything
  resolves from crates.io.

## Release gates (hard rules)

A release may be cut **only when every gate below is green on the exact
commit being released**. If a gate fails, the release is blocked until it is
fixed, not deferred to "the next patch."

| Gate | Command | Bar |
|------|---------|-----|
| Compiles | `cargo build --all-features --all-targets` | clean build |
| Format | `cargo fmt --all --check` | no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| Docs | `cargo doc --no-deps` and `--all-features`, `RUSTDOCFLAGS=-D warnings` | no broken intra-doc links |
| Tests | `cargo test` **and** `cargo test --all-features` | all pass |
| Wire pins | the golden-wire and size/constant tests (run under `cargo test`) | byte-identical |
| MSRV | `cargo +<MSRV> check --all-features --all-targets` | passes on the declared MSRV |
| Supply chain | `cargo deny check` | clean |

These mirror the CI pipeline (`Check` → `Test`, plus the daily `Audit`
cron). CI green on the release commit satisfies every gate.

### MSRV policy

The MSRV is declared in `Cargo.toml` (`rust-version`) and pinned by the
`msrv` CI job — keep both in lockstep, and in lockstep with **hiss's MSRV**
(same policy: a recent stable floored at `stable − 3`). Currently **1.96**.

### Lockfile

`Cargo.lock` is **not** committed (hiss's convention): every CI run and
local gate run re-resolves from the index, so a breaking change shipped
inside a semver-compatible range surfaces at the next run instead of hiding
behind a months-old pin.

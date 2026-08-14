# Slice 0 — "Ground" — implementation plan

> **Status: REVISED 2026/08/14 — all ten questions answered; ready to
> implement.** Written against `SPEC.md`, `PLAN.md` (approved
> 2026/08/14), `STORIES.md` (30 approved stories) and `CLAUDE.md`.
>
> **Revision 2 folds in rulings 60, 61 and 62 and the orchestrator's five
> answers.** The draft raised five disputed points; all five resolved in
> the plan's favour on substance, one (`#[non_exhaustive]`) against it on
> the remedy:
>
> | Raised | Outcome |
> |---|---|
> | `MESSAGE_OVERFLOW` 0x06 conflict (R1/Q-M5) | **Upheld, and wider than found.** §18.1's closing sentence *and* the Named-constants table both still read `0x00`–`0x05`; §9.8 prose still argued against minting the code. All three now fixed in `SPEC.md`. Codes are **`0x00`–`0x06`, `0x07`–`0x0f` reserved, ≥ `0x10` application** |
> | `error.rs` ≠ "§18.1 verbatim" (R2) | **Upheld.** `ConfigError` ships. Ten types, 41 variants |
> | send-failure injection missing (R3) | **Upheld and made normative** — §16.10, **ruling 60**, which also makes seeded determinism a MUST and attests all three fixture names. Closes S24's attestation gap |
> | `Connecting` is a handle (Q-M4) | **Ruling 62.** Argument carried; generalised to *a future that changes protocol state when dropped is a handle; one that does not, is not.* `ConnectError` keeps exactly two variants |
> | `#[non_exhaustive]` everywhere (Q-M1) | **Declined — ruling 61.** `WriteError` **only**. See §5.3 |
>
> **The spec is the authority.** Where this plan and `SPEC.md` disagree,
> the spec wins and this plan is wrong.

## Sources consulted (and deliberately not consulted)

Read narrowly, per `CLAUDE.md` working rule 1 (an agent already died
ingesting `SPEC.md` whole). Line numbers are **post-amendment**
(`SPEC.md` grew from 5 577 to 5 643 lines with rulings 60–62; the ranges
the draft was written against have shifted):

| Range | What |
|---|---|
| `SPEC.md` 5597–5643 | "Named constants" consolidated table — the authority for §4 |
| `SPEC.md` 4897–4988 | §18.1 the error taxonomy + **ruling 61** |
| `SPEC.md` 2429–2451 | §8.1 varint encoding |
| `SPEC.md` 4150–4217 | §16.3 the `Wire` trait + the failing-send trace obligation |
| `SPEC.md` 4536–4573 | §16.10 kernel-free drivability + **ruling 60** (the attested fixture surface) |
| `SPEC.md` 4989–5031 | §18.2 trace targets |
| `SPEC.md` 476–520, 608–625 | §2.3 / §2.4 / §3.5 — needed to *derive* the sizes rather than transcribe magic numbers |
| `SPEC.md` 2490–2513, 3688–3701 | §8.3 frame table, §15.3 error-code registry — the consolidated table names these rows without naming their members |
| `SPEC.md` 3792–3830, 3855–3892, 4039–4060, 4096–4112 | §16.2 shell surface + ruling 44's `ConfigError` + §16.3 handle lifetimes + **ruling 62** |
| `SPEC.md` 5466–5524 (pre-amendment numbering) | Appendix B — the fixture obligations that constrain `testutil` |
| `PLAN.md` §1, §3, §4, §9 | module map, composability/features, slice table, doc obligations |
| `STORIES.md` S24, S25 | the attestation gap (now closed by ruling 60) and the `Wire` story |
| `hiss-0.3.2` `Cargo.toml`, `curve/p256/mod.rs`, `noise/datagram.rs` | to pin `STATIC_PUBLIC_LEN` and `MAX_EPOCH_JUMP` against hiss rather than against a literal |

**Not consulted for guidance:** the v0.1 `src/` bodies. One exception,
declared: I took a *symbol-name-only* overview of `src/testutil.rs` and
read the 12-line `Wire`/`Identity` trait declarations in
`src/endpoint.rs` / `src/handshake.rs`. Rationale in §12, risk R8 — S24
says a downstream crate already depends on `Network`/`FlakyWire`/
`FlakyPolicy` by name, so the *names* are an input to this slice even
though the wire they carried is superseded. No v0.1 wire logic was read.

---

## 1. Scope, and what slice 0 is not

Slice 0 is the crate's foundation: the constants, the errors, the varint
codec, the I/O seam, and the in-memory network. **Nothing in slice 0
parses or emits a slither packet.** No `hiss` handshake is driven, no
mac1 is computed, no frame is encoded. If a file in this slice needs to
know what a packet *looks like*, the scope fence has been breached.

The honest limit the brief already names: the *two-endpoint* paused-clock
fixture cannot exist, because there are no endpoints until slice 2. Slice
0 delivers the substrate beneath it and a single-hop proof that the
substrate works.

### Deliverables

| # | File | New? |
|---|---|---|
| 0 | *delete* `src/**` | — |
| 1 | `Cargo.toml` | rewritten |
| 2 | `src/lib.rs` | new |
| 3 | `src/constants.rs` | new |
| 4 | `src/error.rs` | new |
| 5 | `src/varint.rs` | new |
| 6 | `src/shell/mod.rs` | new — structurally required (`wire.rs` needs a parent) |
| 7 | `src/shell/wire.rs` | new |
| 8 | `src/testutil/mod.rs` | new |

### Definition of done

All eight `CLAUDE.md` gates green on the slice-0 commit, **each run and
its output pasted** (working rule 7 — the exact commands are §14), plus
`testutil::tests::a_byte_crosses_two_flaky_wires_under_injected_loss`
(§9) passing on `#[tokio::test(start_paused = true)]`.

Gate 6 ("wire pins") needs its slice-0 reading stated rather than
assumed: **no golden-wire vectors exist until slice 1**, so in this slice
the pins are the 39 compile-time assertions in `constants.rs` and the
independently-authored transcription test. §14 says how to report it.

---

## 2. Precondition — deleting `src/`

`git rm -r src/` (not `rm`), one commit, message naming `5324ce5` as the
preserved v0.1 tree. Deleting first rather than incrementally replacing
is deliberate: a half-deleted `src/` compiles against v0.1's `Wire`
signature and v0.1's 33-byte compressed static (`[u8; 33]` in
`endpoint.rs`), and the v2 wire uses the 65-byte uncompressed SEC1 form
(§2.4). A tree that compiles against the wrong encoding is the exact
failure mode the clean rewrite exists to avoid.

`examples/` also references the v0.1 API and must go in the same commit
(`cargo build --all-targets` fails otherwise, so gate 1 catches it
either way).

---

## 3. `Cargo.toml`

### Changes from the current file

```toml
version = "0.2.0"
edition = "2024"          # unchanged
rust-version = "1.96"     # unchanged; lockstep with hiss 0.3.2 (verified)
```

`exclude` already carries `"/.slices"` — the maintainer applied it when
answering Q-O4, so slice 0 must **not** re-add it. Verify it is still
there rather than assuming; it is the only thing standing between the
slice plans and the published crate.

`description` needs rewriting: the current one describes the v0.1 wire
("no congestion control, streams, or fragmentation — reserved"), all
three of which v2 has. Proposed: *"A WireGuard-shaped Noise-over-UDP
packet layer carrying a QUIC-shaped reliable frame layer: streams,
messages and datagrams over an IK handshake (P-256 / ChaCha20-Poly1305 /
BLAKE2b) with a staged accept."*

### Features

Exactly `PLAN.md` §3's plan, no more:

```toml
[features]
default = []
test-util = []                                        # exposes `testutil`
sink  = ["dep:futures-core", "dep:futures-sink"]      # §3.2
codec = ["sink", "dep:tokio-util"]                    # §3.3 — implies sink
tower = ["dep:tower-service"]                         # §3.4
```

`codec = ["sink", …]` is the literal encoding of PLAN.md §3.3's "implies
`sink`". `test-util` stays dependency-free — `FlakyWire` needs only
`tokio/time` and `tokio/sync`, both already hard deps.

### Dependencies

Unchanged, except `hiss` moves to the published `0.3.2` (per `CLAUDE.md`;
the gating dep is closed):

```toml
hiss       = { version = "0.3.2", default-features = false }
cryptoxide = { version = ">=0.6.0, <0.7", default-features = false, features = ["blake2"] }
packtool   = "0.6"
tokio      = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
thiserror  = "2"
rand_chacha = "0.10"
rand_core   = "0.10"
getrandom   = "0.4"
tracing     = "0.1"
```

**Verified against `hiss-0.3.2/Cargo.toml` directly** (not from memory):
`cryptoxide = ">=0.6.0, <0.7"`, `rand_core = "0.10"`, `edition = 2024`,
`rust-version = "1.96"`. All four match. The `cryptoxide` range and the
`rand_core` line are `CLAUDE.md` hard rules; both hold.

New optional deps:

```toml
futures-core = { version = "0.3", optional = true }
futures-sink = { version = "0.3", optional = true }
tokio-util   = { version = "0.7", default-features = false, features = ["codec"], optional = true }
tower-service = { version = "0.3", optional = true }
```

`tokio-util` takes `default-features = false` deliberately: the default
pulls `tokio/io-util` and more than `codec` needs, and slice 8 wants the
smallest surface. Verify at slice 8 that `Framed` compiles under it; if
not, that is a slice-8 amendment, not a slice-0 one.

Each new dep will need a `deny.toml` licence check — all four are
MIT/Apache-2.0 and their licences are already in the `allow` list. Gate 8
(`cargo deny check`) confirms rather than assumes.

**Scope fence.** No `tracing-subscriber`, no `tracing-test`, no
`proptest`. The trace-capture dev-dep is a slice-7 need (§18.2's
assertions); adding it now would ship an untested dependency.

### Tests

Nothing in `Cargo.toml` is testable except by the gates. Gate 1
(`--all-features --all-targets`) and gate 8 (`cargo deny check`) *are*
this file's tests.

**One CI change lands in slice 0** (Q-O3, answered — split):
`cargo build --no-default-features` joins the `Check` job **now**,
because it is meaningful today: `test-util` gates the whole `testutil`
module, so a default build that accidentally depends on it breaks
immediately. The three per-feature builds (`--features sink` / `codec` /
`tower`) are **deferred to slice 8**, when those features actually gate
code — a build that compiles nothing proves nothing. `--all-features`
already covers them until then.

---

## 4. `src/constants.rs` — **the file to be most careful with**

> A wrong value here is a wire bug, not a test failure. Every constant is
> transcribed from `SPEC.md` 5531–5577 (the consolidated "Named
> constants" table, which the spec itself declares the reference), with
> §-home sections consulted where the table names a *row* rather than its
> members (frame types → §8.3; error codes → §15.3; sizes → §2.3/§3.5).

### 4.0 Conventions

- **Public module, no crate-root re-exports.** `pub mod constants;` and
  nothing else. One home, so a reviewer finding §7.5 in the spec finds
  `constants::KEEPALIVE_TIMEOUT` and not three aliases.
- **Every constant carries a doc comment naming its spec section.**
  With `#![warn(missing_docs)]` and clippy `-D warnings`, an
  undocumented constant fails gate 3. That is intentional: the doc line
  *is* the attestation.
- **Types follow the wire.** Lengths that index a buffer are `usize`.
  Frame types and error codes are `u64` (they are varints, §8.1). Packet
  type bytes and `VERSION` are `u8` (fixed-width header fields, §3.1).
  Flow-control values are `u64` (varints). In-memory caps are `usize`.
- **Every timer is a `Duration` built from a private `_MS: u64`.**
  ```rust
  const DEAD_TIMEOUT_MS: u64 = 25_000;
  pub const DEAD_TIMEOUT: Duration = Duration::from_millis(DEAD_TIMEOUT_MS);
  ```
  This is not decoration. `Duration` has no `const` comparison operators,
  so **no compile-time assertion on timer ordering is possible without
  the integer companion.** The `_MS` consts stay private; only the
  `Duration`s are public surface.

### 4.1 The constants, in table order

Column 4 is the derivation that pins it; a blank means the value is
primitive (asserted by nothing but the spec, and by the golden-wire
vectors that land in slice 1).

| # | Name | Value | Type | Derived from |
|---|---|---|---|---|
| 1 | `VERSION` | `0x01` | `u8` | — (§3.1) |
| 2 | `PROLOGUE` | `b"slither\x01"` | `&'static [u8; 8]` | last byte **is** `VERSION` |
| 3 | `PKT_HANDSHAKE_INIT` | `0x01` | `u8` | — |
| 4 | `PKT_HANDSHAKE_RESP` | `0x02` | `u8` | — |
| 5 | `PKT_DATA` | `0x03` | `u8` | — |
| 6 | `PKT_RESERVED_UNUSED` | `0x04` | `u8` | — ⚠ name unattested |
| 7 | `PKT_RESERVED_COOKIE` | `0x05` | `u8` | — ⚠ name unattested (spec: "cookie/mac2") |
| 8 | `INIT_HEADER_LEN` | `6` | `usize` | — (§3.2) |
| 9 | `RESP_HEADER_LEN` | `10` | `usize` | — (§3.3) |
| 10 | `DATA_HEADER_LEN` | `14` | `usize` | — (§3.4) |
| 11 | `MAC1_LABEL` | `b"slither mac1"` | `&'static [u8; 12]` | — (§4.1) |
| 12 | `MAC1_LEN` | `16` | `usize` | — (§4.1) |
| 13 | `TIMESTAMP_LEN` | `12` | `usize` | — (§5.2) |
| 14 | `MSG1_PAYLOAD_LEN` | `12` | `usize` | **= `TIMESTAMP_LEN`** |
| 15 | `STATIC_PUBLIC_LEN` | `65` | `usize` | **= `<P256 as Curve>::PUBLIC_KEY_SIZE`** (§2.4) ⚠ name unattested |
| 16 | `AEAD_TAG_LEN` | `16` | `usize` | — (§2.3) |
| 17 | `IK_MSG1_LEN` | `174` | `usize` | **= `PK + (PK + TAG) + (MSG1_PAYLOAD_LEN + TAG)`** |
| 18 | `IK_MSG2_LEN` | `81` | `usize` | **= `PK + TAG`** |
| 19 | `INIT_PACKET_LEN` | `196` | `usize` | **= `INIT_HEADER_LEN + IK_MSG1_LEN + MAC1_LEN`** |
| 20 | `RESP_PACKET_LEN` | `107` | `usize` | **= `RESP_HEADER_LEN + IK_MSG2_LEN + MAC1_LEN`** |
| 21 | `MAX_DATAGRAM` | `1200` | `usize` | — (§3.5) |
| 22 | `MAX_PLAINTEXT` | `1170` | `usize` | **= `MAX_DATAGRAM − DATA_HEADER_LEN − AEAD_TAG_LEN`** |
| 23 | `REKEY_EPOCH_MSGS` | `65_536` | `u64` | — (§7.7; slither's choice, passed to hiss's `into_datagram_with_epoch`) |
| 24 | `MAX_EPOCH_JUMP` | `2` | `u64` | **= `hiss::…::MAX_EPOCH_JUMP`** (hiss-fixed) |
| 25 | `REPLAY_WINDOW` | `2048` | `usize` | — (§7.2) — **bits**, doc says so |
| 26 | `FRAME_PADDING` | `0x00` | `u64` | — (§8.3) |
| 27 | `FRAME_PING` | `0x01` | `u64` | — |
| 28 | `FRAME_ACK` | `0x02` | `u64` | — |
| 29 | `FRAME_RESET_STREAM` | `0x04` | `u64` | — |
| 30 | `FRAME_STOP_SENDING_RESERVED` | `0x05` | `u64` | — reserved, never sent (§19) ⚠ name unattested |
| 31 | `FRAME_STREAM_BASE` | `0x08` | `u64` | — |
| 32 | `FRAME_STREAM_MAX` | `0x0f` | `u64` | **= `FRAME_STREAM_BASE \| STREAM_FLAG_MASK`** |
| 33 | `FRAME_MAX_DATA` | `0x10` | `u64` | — |
| 34 | `FRAME_MAX_STREAM_DATA` | `0x11` | `u64` | — |
| 35 | `FRAME_MAX_STREAMS_BIDI` | `0x12` | `u64` | — |
| 36 | `FRAME_MAX_STREAMS_UNI` | `0x13` | `u64` | — |
| 37 | `FRAME_CLOSE` | `0x1c` | `u64` | — |
| 38 | `FRAME_DATAGRAM` | `0x30` | `u64` | — |
| 39 | `FRAME_DATAGRAM_LEN` | `0x31` | `u64` | **= `FRAME_DATAGRAM \| 0x01`** |
| 40 | `STREAM_OFF` | `0x04` | `u64` | — (§8.4) |
| 41 | `STREAM_LEN` | `0x02` | `u64` | — |
| 42 | `STREAM_FIN` | `0x01` | `u64` | — |
| 43 | `STREAM_FLAG_MASK` | `0x07` | `u64` | **= `STREAM_OFF \| STREAM_LEN \| STREAM_FIN`** ⚠ name unattested |
| 44 | `INITIAL_MAX_DATA` | `1_048_576` | `u64` | — (§10.2) |
| 45 | `INITIAL_MAX_STREAM_DATA` | `262_144` | `u64` | — (§10.2) |
| 46 | `INITIAL_MAX_STREAMS_BIDI` | `32` | `u64` | — (§10.2, cumulative) |
| 47 | `INITIAL_MAX_STREAMS_UNI` | `128` | `u64` | — (§10.2, cumulative) |
| 48 | `STREAMS_CREDIT_BATCH` | `8` | `u64` | — (§10.4) |
| 49 | `CREDIT_REGRANT_DIVISOR` | `2` | `u64` | — (§10.3, "½ window consumed") ⚠ name unattested |
| 50 | `MESSAGE_RECV_MAX` | `262_144` | `u64` | **= `INITIAL_MAX_STREAM_DATA`** (§9.8) |
| 51 | `MAX_DATAGRAM_PAYLOAD` | `1169` | `usize` | **= `MAX_PLAINTEXT − 1`** (§11.2) |
| 52 | `DATAGRAM_SEND_QUEUE` | `64` | `usize` | — (§11.3) |
| 53 | `DATAGRAM_RECV_QUEUE` | `64` | `usize` | — (§11.3) |
| 54 | `REASSEMBLY_CHUNKS_MAX` | `1024` | `usize` | — (§10.6) |
| 55 | `CLOSE_REASON_MAX` | `256` | `usize` | — (§8.4) |
| 56 | `CLOSE_LINGER` | `5 s` | `Duration` | — (§15.1) |
| 57 | `CLOSE_REPLY_MIN_INTERVAL` | `1 s` | `Duration` | — (§15.1, "≤ 1 per s") ⚠ name unattested |
| 58 | `MAX_ACK_RANGES` | `64` | `usize` | — (§12.2) |
| 59 | `ACK_ELICITING_PER_ACK` | `2` | `u64` | — (§12.4, "every 2nd") ⚠ name unattested |
| 60 | `MAX_ACK_DELAY` | `25 ms` | `Duration` | — (§12.4/§13.3) |
| 61 | `K_PACKET_THRESHOLD` | `3` | `u64` | — (§13.2) |
| 62 | `K_TIME_THRESHOLD_NUM` | `9` | `u32` | — (§13.2, "9⁄8") ⚠ name unattested |
| 63 | `K_TIME_THRESHOLD_DEN` | `8` | `u32` | — ⚠ name unattested |
| 64 | `K_GRANULARITY` | `1 ms` | `Duration` | — (§13.2) |
| 65 | `K_INITIAL_RTT` | `333 ms` | `Duration` | — (§13.1) |
| 66 | `PTO_BACKOFF_CAP` | `64` | `u32` | **= `1 << 6`** (§13.3 caps `2^pto_count`; §13.5 "2⁶× too long" — the **multiplier**, settled) |
| 67 | `INITIAL_WINDOW` | `12_000` | `u64` | **= `10 × MAX_DATAGRAM`** (RFC 9002 `kInitialWindow` at this MTU) |
| 68 | `MINIMUM_WINDOW` | `2_400` | `u64` | **= `2 × MAX_DATAGRAM`** (RFC 9002 `kMinimumWindow`) |
| 69 | `LOSS_REDUCTION_FACTOR` | `0.5` | `f64` | — (§14.2) — see §4.4 |
| 70 | `PERSISTENT_CONGESTION_THRESHOLD` | `3` | `u32` | — (§14.4) |
| 71 | `RETRANSMIT_BASE` | `5 s` | `Duration` | — (§5.5) |
| 72 | `RETRANSMIT_JITTER_MAX` | `333 ms` | `Duration` | — (§5.5) |
| 73 | `HANDSHAKE_GIVEUP` | `90 s` | `Duration` | — (§5.5) |
| 74 | `KEEPALIVE_TIMEOUT` | `10 s` | `Duration` | — (§7.5) |
| 75 | `DEAD_TIMEOUT` | `25 s` | `Duration` | — (§7.5) |
| 76 | `PERSISTENT_KEEPALIVE_DEFAULT` | `10 s` | `Duration` | — (§7.5) ⚠ spec name is bare `PERSISTENT_KEEPALIVE` |
| 77 | `PERSISTENT_KEEPALIVE_MIN` | `1 s` | `Duration` | — (§7.5, ruling 42 floor, **inclusive**) ⚠ name unattested |
| 78 | `AMPLIFICATION_FACTOR` | `3` | `u64` | — (§7.3) |
| 79 | `INTRO_QUEUE_CAP` | `1024` | `usize` | — (§6.3) |
| 80 | `INTRO_MAX_PER_SOURCE` | `4` | `usize` | — (§6.3) |
| 81 | `INTRO_TTL` | `15 s` | `Duration` | — (§6.3) |
| 82 | `TS_GUARD_ORPHAN_CAP` | `1024` | `usize` | — (§17.1) |
| 83 | `SHELL_LATENESS_BOUND` | `250 ms` | `Duration` | — (§16.5, spec calls it `L`) ⚠ name unattested |
| 84 | `NO_ERROR` | `0x00` | `u64` | — (§15.3) |
| 85 | `PROTOCOL_VIOLATION` | `0x01` | `u64` | — |
| 86 | `FLOW_CONTROL_ERROR` | `0x02` | `u64` | — |
| 87 | `STREAM_LIMIT_ERROR` | `0x03` | `u64` | — |
| 88 | `STREAM_STATE_ERROR` | `0x04` | `u64` | — |
| 89 | `FINAL_SIZE_ERROR` | `0x05` | `u64` | — |
| 90 | `MESSAGE_OVERFLOW` | `0x06` | `u64` | — (§15.3, **ruling 52**) — **conflict resolved 2026/08/14, see R1** |
| 91 | `APPLICATION_ERROR_BASE` | `0x10` | `u64` | — (§15.3, "≥ 0x10 application") ⚠ name unattested |

The error-code range is now consistent across all four places that state
it — §15.3's registry, §18.1's closing sentence, the Named-constants
table, and §9.8's prose: **`0x00`–`0x06`, `0x07`–`0x0f` reserved
(transport-reserved, never sent), `≥ 0x10` application.** Implement
exactly that. `0x07`–`0x0f` gets **no constants**; it is a gap, and a
`RESERVED_*` name for each would invite one to be sent.

The ceiling of `PERSISTENT_KEEPALIVE`'s admissible range is
`DEAD_TIMEOUT` **exclusive** (ruling 40) — no separate constant; the
validator compares against `DEAD_TIMEOUT` directly. Deliberate: an
alias would let the two drift.

**Session index** (`nonzero u32, random, re-drawn`, §17.3) is a *rule*,
not a value. No constant. The zero-index rejection belongs to `tables.rs`
in slice 2. Recommend **not** minting `SESSION_INDEX_RESERVED = 0`.

### 4.2 The compile-time assertions

Every derived row above gets a `const _: () = assert!(…)`. Full list, in
the order they should appear in the file:

```rust
// ── Identity and versioning ────────────────────────────────────────────
const _: () = assert!(PROLOGUE.len() == 8);
const _: () = assert!(PROLOGUE[7] == VERSION);              // §5.1 pins the tail
const _: () = assert!(MAC1_LABEL.len() == 12);              // §4.1

// ── Suite-derived sizes (§2.3) ────────────────────────────────────────
// The one assertion that reaches outside slither: it pins slither's
// handshake arithmetic to hiss's curve, so a hiss change that moved the
// point encoding turns the BUILD red, not a test.
const _: () = assert!(
    STATIC_PUBLIC_LEN == <hiss::curve::p256::P256 as hiss::curve::Curve>::PUBLIC_KEY_SIZE
);
const _: () = assert!(MSG1_PAYLOAD_LEN == TIMESTAMP_LEN);
const _: () = assert!(
    IK_MSG1_LEN == STATIC_PUBLIC_LEN
                 + (STATIC_PUBLIC_LEN + AEAD_TAG_LEN)
                 + (MSG1_PAYLOAD_LEN + AEAD_TAG_LEN)
);
const _: () = assert!(IK_MSG2_LEN == STATIC_PUBLIC_LEN + AEAD_TAG_LEN);
const _: () = assert!(INIT_PACKET_LEN == INIT_HEADER_LEN + IK_MSG1_LEN + MAC1_LEN);
const _: () = assert!(RESP_PACKET_LEN == RESP_HEADER_LEN + IK_MSG2_LEN + MAC1_LEN);
const _: () = assert!(INIT_PACKET_LEN <= MAX_DATAGRAM);     // must not fragment
const _: () = assert!(RESP_PACKET_LEN <= MAX_DATAGRAM);

// ── Data-path sizes (§3.5, §11.2) ─────────────────────────────────────
const _: () = assert!(MAX_PLAINTEXT == MAX_DATAGRAM - DATA_HEADER_LEN - AEAD_TAG_LEN);
const _: () = assert!(MAX_DATAGRAM_PAYLOAD == MAX_PLAINTEXT - 1);
// A CLOSE with a maximum reason must fit one packet (§8.4, §15.1):
// 1 type byte + worst-case 8-byte varint code + 8-byte varint len + reason.
const _: () = assert!(1 + 8 + 8 + CLOSE_REASON_MAX <= MAX_PLAINTEXT);
// §11.3's own parenthetical: "≈ 73 KiB worst case each".
const _: () = assert!(DATAGRAM_SEND_QUEUE * MAX_DATAGRAM_PAYLOAD < 80 * 1024);
const _: () = assert!(DATAGRAM_RECV_QUEUE * MAX_DATAGRAM_PAYLOAD < 80 * 1024);

// ── Frame grammar (§8.3, §8.4) ────────────────────────────────────────
const _: () = assert!(STREAM_FLAG_MASK == STREAM_OFF | STREAM_LEN | STREAM_FIN);
const _: () = assert!(FRAME_STREAM_MAX == FRAME_STREAM_BASE | STREAM_FLAG_MASK);
const _: () = assert!(FRAME_STREAM_BASE & STREAM_FLAG_MASK == 0);   // flags don't collide
const _: () = assert!(FRAME_DATAGRAM_LEN == FRAME_DATAGRAM | 0x01);

// ── Flow control (§10.2, §9.8) ────────────────────────────────────────
const _: () = assert!(MESSAGE_RECV_MAX == INITIAL_MAX_STREAM_DATA);
const _: () = assert!(INITIAL_MAX_STREAM_DATA <= INITIAL_MAX_DATA);
const _: () = assert!(INITIAL_MAX_DATA        <= crate::varint::VarInt::MAX_VALUE);
const _: () = assert!(INITIAL_MAX_STREAM_DATA <= crate::varint::VarInt::MAX_VALUE);
const _: () = assert!(MESSAGE_RECV_MAX        <= crate::varint::VarInt::MAX_VALUE);

// ── Session (§7.2, §7.7) ──────────────────────────────────────────────
const _: () = assert!(REPLAY_WINDOW % 64 == 0);             // whole u64 words
const _: () = assert!(MAX_EPOCH_JUMP == hiss::noise::datagram::MAX_EPOCH_JUMP);

// ── Recovery and congestion (§13, §14) ────────────────────────────────
const _: () = assert!(PTO_BACKOFF_CAP == 1 << 6);           // the spec writes 2⁶
const _: () = assert!(INITIAL_WINDOW == 10 * MAX_DATAGRAM as u64);  // RFC 9002
const _: () = assert!(MINIMUM_WINDOW ==  2 * MAX_DATAGRAM as u64);  // RFC 9002
const _: () = assert!(MINIMUM_WINDOW < INITIAL_WINDOW);
const _: () = assert!(K_TIME_THRESHOLD_NUM > K_TIME_THRESHOLD_DEN); // > 1, or it's not a threshold

// ── Timer ordering (§5.5, §6.3, §7.5, §12.4, §13.2) ───────────────────
// These are the reason the private `_MS` companions exist.
const _: () = assert!(KEEPALIVE_TIMEOUT_MS < DEAD_TIMEOUT_MS);
const _: () = assert!(MAX_ACK_DELAY_MS < KEEPALIVE_TIMEOUT_MS);
const _: () = assert!(K_GRANULARITY_MS <= MAX_ACK_DELAY_MS);
const _: () = assert!(RETRANSMIT_BASE_MS + RETRANSMIT_JITTER_MAX_MS < HANDSHAKE_GIVEUP_MS);
const _: () = assert!(INTRO_TTL_MS < HANDSHAKE_GIVEUP_MS);
const _: () = assert!(PERSISTENT_KEEPALIVE_MIN_MS <= PERSISTENT_KEEPALIVE_DEFAULT_MS);
const _: () = assert!(PERSISTENT_KEEPALIVE_DEFAULT_MS < DEAD_TIMEOUT_MS);   // ruling 40's ceiling
```

Two of these deserve their rationale stated because they are the ones
that will look like over-engineering to a reviewer:

- **`STATIC_PUBLIC_LEN == P256::PUBLIC_KEY_SIZE`** turns "hiss changed
  its point encoding" from a golden-wire test failure (slice 1, and only
  if the vectors were regenerated correctly) into a compile error in
  slice 0. `SPEC.md` §2.3 states the sizes *as derivations* over `PK` and
  `TAG`; this is that derivation, executed.
- **`INITIAL_WINDOW == 10 × MAX_DATAGRAM`** is not a coincidence — it is
  RFC 9002's `kInitialWindow` evaluated at slither's MTU. Pinning it
  means a future MTU ruling cannot silently leave the congestion
  constants behind.

**Verified arithmetic** (each computed, not recalled):
`65 + 81 + 28 = 174` ✓ · `65 + 16 = 81` ✓ · `6 + 174 + 16 = 196` ✓ ·
`10 + 81 + 16 = 107` ✓ · `1200 − 14 − 16 = 1170` ✓ · `1170 − 1 = 1169` ✓ ·
`10 × 1200 = 12 000` ✓ · `2 × 1200 = 2 400` ✓ · `64 × 1169 = 74 816 B =
73.06 KiB` ✓ · `1 + 8 + 8 + 256 = 273 ≤ 1170` ✓.

### 4.3 What `constants.rs` must NOT contain

- **No functions.** Not `fn is_stream_frame(t: u64) -> bool`, not
  `fn error_code_name(c: u64)`. Predicates over the frame table belong to
  `core::connection::frame` (slice 3). A constants file that grows
  behaviour becomes a second home for §8.3.
- **No enums.** `enum FrameType` is slice 3's; `enum ErrorCode` is
  slice 3's `close.rs`. Slice 0 ships the *bytes*, so slice 1's
  golden-wire vectors can be written against them before any parser
  exists to be wrong.
- **No derived implementation constants.** `REPLAY_WINDOW_WORDS = 32`,
  `STREAM_ID_TYPE_MASK`, packet-header field offsets — each is the
  private business of the module that owns it. The consolidated table is
  the fence: if the spec did not name it, it does not live here.
- **No `Config` defaults struct.** `INITIAL_MAX_DATA` etc. are the
  *values*; the `Config` that carries them is slice 2's.
- **No `VARINT_MAX`.** §8.1 is `varint.rs`'s home (see §6).

### 4.4 The one judgement call inside the file

`LOSS_REDUCTION_FACTOR: f64 = 0.5`. A float in a protocol-constants file
is a smell, and 0.5 is exactly representable so nothing is lost either
way. Two options: keep the `f64` and match the spec's notation, or ship
`LOSS_REDUCTION_NUM/DEN = 1/2` and do integer arithmetic in
`congestion.rs`. **Recommend the `f64`**, because it is what §14.2 says,
and add a doc line telling slice 5 that the implementation halves with an
integer shift rather than multiplying by the float (`cwnd / 2`, not
`(cwnd as f64 * 0.5) as u64`) — the constant is the spec's statement, the
shift is the arithmetic. Flagged so slice 5 does not discover it.

### 4.5 Tests

Compile-time assertions are the primary test — they cannot be skipped,
deferred, or `#[ignore]`d. Runtime tests add only what `const` cannot do:

1. `named_constants_match_the_consolidated_table` — a single test
   asserting each of the 91 values against a literal, written **from the
   spec table and not from the source file**, by a different author than
   the one who writes `constants.rs` (working rule 6). Yes, this
   duplicates the file. That is the point: it is the transcription check,
   and it is the cheapest possible defence against a typo in a wire
   constant.
2. `timers_are_expressible_in_virtual_time` — every `Duration` is a whole
   number of milliseconds (`d.subsec_nanos() % 1_000_000 == 0`), so the
   paused clock can land exactly on each deadline. A sub-millisecond
   timer would make §16.10's tests flaky-by-construction.
3. `frame_types_are_distinct` — collect all 14 frame-type constants,
   assert no duplicates and that none falls inside
   `FRAME_STREAM_BASE..=FRAME_STREAM_MAX` except the STREAM range itself.
4. `error_codes_are_distinct_and_below_the_application_base` — the
   **seven** transport codes (`NO_ERROR` … `MESSAGE_OVERFLOW`) are
   unique, contiguous `0x00..=0x06`, and `< APPLICATION_ERROR_BASE`;
   `0x07..0x10` is the reserved gap and no constant occupies it. This
   test is the standing guard for R1's failure mode — a restatement of
   the registry drifting from the registry.
5. `epoch_jump_matches_hiss` — belt-and-braces runtime mirror of the
   const assert, in case hiss's const turns out not to be const-context
   reachable under `default-features = false` (**verify at
   implementation time**; if it is not, the const assert is dropped and
   this test is the only pin).

---

## 5. `src/error.rs`

### This file is §18.1 **plus one type §18.1 excludes**

"§18.1's closed taxonomy, verbatim" alone would ship a crate whose §16.2
shell surface does not compile. `set_persistent_keepalive` returns
`Result<(), ConfigError>` (§16.2), and **ruling 44** defines
`ConfigError::{KeepaliveTooShort, KeepaliveTooLong}` while stating that it
*"deliberately sits outside §18.1's protocol-error taxonomy, which stays
closed: no peer, no packet, and no connection state is involved."*
Confirmed by the orchestrator; ten types is right.

So: `error.rs` carries §18.1's **nine** types verbatim, **plus**
`ConfigError` from §16.2/ruling 44, with a module doc that states exactly
why the tenth type is there and that its presence does not open §18.1.
S24's sibling story pins it: `STORIES.md` line 129 requires
`Result<(), ConfigError>` with `Err` at both bounds.

### 5.1 The types, exactly

**`#[non_exhaustive]` appears on `WriteError` and nowhere else**
(ruling 61). Every other type below is exhaustive on purpose; §5.3 has
the reasoning, and a reviewer who "fixes" the inconsistency by adding the
attribute to the other nine has undone a ruling.

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    #[error("a connection to this static already exists")]
    AlreadyConnected,
    #[error("the initial connect gave up after HANDSHAKE_GIVEUP")]
    TimedOut,
    // NO `EndpointDropped` (ruling 62): a `Connecting` is a handle, so
    // the driver cannot stop beneath one and the variant would describe
    // no reachable state. §18.1 stays closed.
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntroError {
    #[error("the parked introduction outlived INTRO_TTL")]
    Expired,
    #[error("the initiation belonged to a pending outbound dial and was consumed")]
    Internal,
    #[error("the introduction's msg1 is structurally unreadable")]
    Malformed,
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("the timestamp guard rejected this initiation as a replay")]
    Replay,
    #[error("the handshake failed to authenticate")]      // ← security signal
    HandshakeFailed,
    #[error("the parked introduction outlived INTRO_TTL")]
    Expired,
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcceptError {
    #[error("no initiation is parked for this static, or it fails the replacement basis")]
    Stale,
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionLost {                    // ← MUST derive Clone: see §5.2
    #[error("no authenticated packet arrived for DEAD_TIMEOUT")]
    TimedOut,
    #[error("the send counter is exhausted")]
    NonceExhausted,
    #[error("closed by this application")]
    LocallyClosed,
    #[error("closed by the peer: code {code}")]
    PeerClosed { code: u64, reason: Vec<u8> },
    #[error("torn down after a protocol violation: code {code}")]
    ProtocolViolation { code: u64 },
    #[error("replaced by a newer connection from the same static")]
    Replaced,
    #[error("the endpoint driver stopped")]
    EndpointDropped,
}

/// The **one** non-exhaustive error type (ruling 61): §19 explicitly
/// reserves `Stopped` for the STOP_SENDING round, so this type
/// demonstrably will gain a variant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    #[error("the stream was reset: code {0}")]
    Reset(u64),
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
    #[error("write after finish")]
    Finished,
    // NO `Stopped` yet: STOP_SENDING is deferred (§9.9). The variant is
    // reserved for that round and MUST NOT be added here — the
    // `#[non_exhaustive]` above is the reservation, not a licence.
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("the peer reset the stream: code {0}")]
    Reset(u64),
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageError {
    #[error("the message exceeds MESSAGE_RECV_MAX")]
    TooLarge,
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatagramError {
    #[error("the datagram exceeds MAX_DATAGRAM_PAYLOAD")]
    TooLarge,
    #[error(transparent)]
    ConnectionLost(#[from] ConnectionLost),
}

/// Outside §18.1 by ruling 44 — a configuration error, not a protocol
/// one: no peer, no packet, no connection state, nothing observable on
/// the wire. Exhaustive, like the taxonomy proper (ruling 61).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("the persistent-keepalive interval is below the 1 s floor")]
    KeepaliveTooShort,
    #[error("the persistent-keepalive interval is at or above DEAD_TIMEOUT")]
    KeepaliveTooLong,
}
```

That is **41 variants across 10 types**. Nothing else. Not one variant is
added, renamed, merged, or given a payload the spec does not name.

### 5.2 `ConnectionLost: Clone` — required, and why

`ConnectionLost` is the payload of a *fan-out*: `closed()` resolves for
every holder, `notified()` returns `Result<_, ConnectionLost>` on every
handle, and every one of `WriteError`/`ReadError`/`MessageError`/
`DatagramError` embeds it. A connection dies once and the same value is
handed to N awaiting futures and to every subsequent verb call — that is
impossible without `Clone` unless the shell wraps it in an `Rc`, which
would put `Rc` in the public error type. **`Clone` is a hard requirement,
not a convenience**, and `PeerClosed { reason: Vec<u8> }` is the only
variant that makes it non-trivial (a heap clone bounded at
`CLOSE_REASON_MAX` = 256 B — negligible).

A test pins it, because a later `#[non_exhaustive]` payload could silently
break it:

```rust
fn _assert_clone<T: Clone>() {}
#[test] fn connection_lost_is_clone() { _assert_clone::<ConnectionLost>(); }
```

`Clone` propagates to all four embedding types, so all four derive it too.

### 5.3 `#[non_exhaustive]` — `WriteError` only (ruling 61)

**Settled 2026/08/14. The draft recommended applying it to all ten types;
that was declined, and the reasoning is recorded in §18.1 so it is not
re-proposed.** The rule is now:

> `#[non_exhaustive]` goes **only where a variant is actually reserved**.

- **`WriteError` carries it** — §19 explicitly reserves `Stopped` for the
  STOP_SENDING round, so that type demonstrably will gain a variant.
- **The other nine are exhaustive.** The taxonomy is closed by process,
  and the type system should say the same thing wherever that is true.

The argument that carried it, which the draft missed: a consumer matching
an exhaustive enum with **no `_` arm** gets a **compile error** the day a
variant is added — and for a transport that is the loud failure worth
having. A wildcard arm silently swallows a new error into a branch
written for the old ones, which is strictly worse than a build break. The
draft weighed only the ergonomic cost of the `_` arm and never weighed
what the `_` arm *does* when a variant lands.

The price is that adding a variant to any of the nine is a major version
bump. Ruling 61 calls that "the correct price and a useful brake", and it
is consistent with the taxonomy being closed by process anyway: a variant
addition already requires a ratification decision, so it is never a quiet
patch release regardless.

Two neighbours stay as they are, for reasons that do not generalise:
§16.2's `Notification` remains `#[non_exhaustive]` — it is a *signal set*
a later wire line may extend, not an error taxonomy — and `ConfigError`
sits outside §18.1 entirely (ruling 44) and is exhaustive.

**Implementation note.** `#[non_exhaustive]` on nine fewer types means
slice 0's own tests, and every test in slices 1–8, match without `_`
arms. That is a feature: §5.6 test 1's exhaustive-match fence becomes the
*normal* way the crate matches its own errors, not a special construct.

### 5.4 Other derives

`Debug` (required by `Error`), `Clone`, `PartialEq`/`Eq` (tests assert
`assert_eq!(err, ConnectionLost::TimedOut)` throughout slices 3–7).
**No `Copy`** — `PeerClosed` allocates. **No `Hash`, no `PartialOrd`** —
no story needs them and each is surface. **No `serde`** — not a
dependency and not asked for.

`#[from] ConnectionLost` on the four embedding types gives `?` for free;
that is the only conversion in slice 0. The `io::Error` conversions
(`From<ReadError>`/`From<WriteError>`, PLAN.md §3.1) are **slice 8's**,
not slice 0's — they belong to `compat/io.rs`. Scope fence.

### 5.5 What `error.rs` must NOT contain

- **No `SlitherError` umbrella enum.** v0.1 had one (the current
  `Cargo.toml` comment names it). §18.1 has no such type; adding a
  top-level union re-opens the taxonomy through the back door and gives
  every consumer a second way to match.
- **No `io::Error` conversions** (slice 8).
- **No wire-code ↔ variant mapping.** `code: u64` → `PROTOCOL_VIOLATION`
  is §15.3/§15.4's teardown matrix, and it lives in `close.rs` (slice 3).
- **No `Result<T>` alias.** Ten error types; one alias would have to pick
  a favourite.
- **No `Notification`** — it is §16.2's, not §18.1's, and it belongs to
  `shell/connection.rs` (slice 3/7).

### 5.6 Tests

1. `taxonomy_is_closed` — a compile-fence: one exhaustive `match` per
   type over every variant, with **no `_` arm**, inside a `#[cfg(test)]`
   function. Adding a variant makes it fail to compile; that is the
   mechanical enforcement of "closed", and it is stronger than a comment.
   (Written **before** the enum, per §10's author swap.) This works for
   all ten types, `WriteError` included: **`#[non_exhaustive]` has no
   effect inside the crate that declares it**, so an in-crate match must
   still cover every variant and the fence catches an addition there too.
   The attribute only forces a `_` arm on *downstream* consumers — which
   is exactly the asymmetry ruling 61 wants.
2. `connection_lost_is_clone` — §5.2.
3. `display_strings_are_non_empty_and_lowercase_initial` — every
   `to_string()` is non-empty and does not end in `.` (thiserror style
   consistency; `-D warnings` will not catch it).
4. `embedded_connection_lost_converts` — `ConnectionLost::TimedOut` into
   each of `WriteError`/`ReadError`/`MessageError`/`DatagramError` via
   `?`, and `source()` chains to it (`#[error(transparent)]`).
5. `error_types_are_send_and_sync` — errors *may* be `Send`: they are
   values, not handles, and `!Send` on the actor path does not extend to
   error payloads. A consumer will want to send one across a channel.
   `static_assertions`-free: `fn _s<T: Send + Sync + 'static>() {}`.
   **This does not violate `CLAUDE.md`'s no-`Send` rule** — that rule is
   about the *actor path* (the `Wire`, the DH provider, the handles), not
   about plain data.

---

## 6. `src/varint.rs` — §8.1

### 6.1 Visibility — `pub(crate)`, and the reason it matters

Recommend **`pub(crate) mod varint;`**, not `pub`. Three reasons:

1. No §16 surface exposes a varint. A consumer never encodes one.
2. `encode`'s "value exceeds 2⁶² − 1" case needs a way to say no. If the
   module is public, that is a **new public error type** — and §18.1 is
   closed. Keeping the module crate-private keeps the overflow condition
   entirely internal, which is the cheapest way to honour the closure.
3. Promoting `pub(crate)` → `pub` later is not a breaking change. The
   reverse is. Start closed.

### 6.2 Public (crate) surface

A newtype, not free functions over `u64`. This is quinn's shape and it
buys a real invariant: §8.1's *stated consequence* is that ACK `largest`,
stream offsets and final sizes all cap at 2⁶² − 1. Encoding that cap in
the type means slices 3–5 cannot construct an unencodable value at all,
rather than checking at every call site and forgetting once.

```rust
/// A QUIC variable-length integer (RFC 9000 §16), byte-identical. §8.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct VarInt(u64);

impl VarInt {
    /// The largest representable value, 2⁶² − 1.
    pub(crate) const MAX: VarInt = VarInt(Self::MAX_VALUE);
    /// `MAX` as a bare `u64`, for use in `const` assertions.
    pub(crate) const MAX_VALUE: u64 = (1u64 << 62) - 1;

    /// `None` if `v > MAX_VALUE`.
    pub(crate) const fn new(v: u64) -> Option<VarInt>;
    /// Infallible: every `u32` fits the 62-bit space.
    pub(crate) const fn from_u32(v: u32) -> VarInt;
    /// Infallible const constructor for literals; **panics at compile
    /// time** if `v > MAX_VALUE`, so it is only usable in `const` position.
    pub(crate) const fn from_const(v: u64) -> VarInt;
    pub(crate) const fn into_inner(self) -> u64;
    /// The *minimal* encoded length: 1, 2, 4 or 8. §8.1's sender rule.
    pub(crate) const fn encoded_len(self) -> usize;
}

impl From<VarInt> for u64 { … }
impl From<u32>    for VarInt { … }

/// Append the minimal encoding of `v` to `out`. §8.1: a sender emits the
/// minimal encoding.
pub(crate) fn encode(v: VarInt, out: &mut Vec<u8>);

/// Write the minimal encoding into `out`, returning the bytes written, or
/// `None` if `out` is too short. The frame packer (§8.6) writes into a
/// fixed 1170-byte plaintext buffer, so this is the form it uses.
pub(crate) fn encode_to(v: VarInt, out: &mut [u8]) -> Option<usize>;

/// Decode one varint from the front of `buf`: the value and the bytes
/// consumed. `None` on a truncated input. §8.1: a receiver accepts **any**
/// length, so a non-minimal encoding decodes successfully.
pub(crate) fn decode(buf: &[u8]) -> Option<(VarInt, usize)>;
```

`decode` returning `Option` rather than a typed error is deliberate: the
frame layer's response to a truncated varint is fixed by §8.2 (a
structural failure → CLOSE with `PROTOCOL_VIOLATION`), so there is
exactly one caller and exactly one reaction. A richer error would be
information the caller discards.

### 6.3 What `varint.rs` must NOT contain

- **No frame types, no field names, no frame parsing.** §8.2–§8.7 is
  slice 3.
- **No `Buf`/`BufMut`, no `bytes` crate.** Not a dependency; `&[u8]` and
  `Vec<u8>` are enough and keep the dependency graph as declared.
- **No `VarInt` arithmetic** (`Add`, `Sub`, `checked_add`). Slices 4–5
  will want offsets to add; let them ask, so the overflow semantics are
  designed once with a caller in view rather than guessed now.
- **No public re-export** from `lib.rs`.

### 6.4 Tests

The strongest available pins are external, so use them:

1. **RFC 9000 Appendix A.1's four vectors**, byte-for-byte:
   | Encoding | Value |
   |---|---|
   | `c2 19 7c 5e ff 14 e8 8c` | 151 288 809 941 952 652 |
   | `9d 7f 3e 7d` | 494 878 333 |
   | `7b bd` | 15 293 |
   | `25` | 37 |
   | `40 25` | 37 — **non-minimal, must decode** |
   Each decodes to the stated value; each of the first four is what
   `encode` produces for that value; `40 25` is *not* what `encode`
   produces for 37 (it produces `25`), which is the minimal-encoding rule
   tested from both sides.
2. `boundaries_round_trip` — 0, 63, 64, 16 383, 16 384, 1 073 741 823,
   1 073 741 824, 2⁶² − 1. For each: `encoded_len` is 1/1/2/2/4/4/8/8,
   `decode(encode(v))` is `(v, encoded_len)`, and the first byte's top two
   bits are the expected prefix.
3. `above_max_is_rejected` — `VarInt::new(1 << 62)` is `None`;
   `VarInt::new(u64::MAX)` is `None`; `VarInt::new(MAX_VALUE)` is `Some`.
4. `truncated_input_is_none` — empty; a `0b01…` first byte with 1 byte
   available; `0b10…` with 3; `0b11…` with 7. All `None`.
5. `decode_accepts_every_non_minimal_encoding_of_a_small_value` — 37
   encoded in 1, 2, 4 and 8 bytes all decode to 37, and each reports its
   own length consumed (1/2/4/8, not 1). This is the rule most likely to
   be implemented wrongly, because "reject non-minimal" is the safer-
   *looking* choice and §8.1 explicitly forbids it.
6. `exhaustive_round_trip_near_boundaries` — every value in
   `0..=1024`, and ±4 around each boundary, round-trips. Cheap, total,
   no proptest dependency.
7. `encode_to_respects_a_short_buffer` — `None`, and **`out` is not
   partially written** (the packer relies on it: §8.6 packs frames until
   one does not fit, and a partial write would corrupt the packet). This
   is the subtle one; it must be tested, not assumed.

---

## 7. `src/shell/wire.rs` — §16.3

`src/shell/mod.rs` in this slice is three lines: a module doc naming
§16.3 and `pub mod wire;`. Everything else in the `shell/` map
(`driver.rs`, `endpoint.rs`, `staged.rs`, `connection.rs`, `stream.rs`)
arrives in slices 2–3.

### 7.1 The trait, verbatim

```rust
/// The datagram substrate an endpoint runs over. §16.3, ratified
/// 2026/08/14 (ruling 49).
///
/// # Normative properties
///
/// 1. **The application supplies it**, through `Endpoint::builder()`
///    (§16.2) — so an application needing its own socket options, a
///    dual-stack or per-interface arrangement, a tunnel, or a simulator
///    installs one without forking the crate.
/// 2. **It is not required to be `Send`**, and no `Send` bound may be
///    added to it or to the futures its methods return. The driver is a
///    single `!Send` actor, and the reasoning that keeps a DH provider
///    free of `Send` (a hardware static) keeps a `Wire` free of it.
/// 3. **Both methods take `&self`**, because the one driver task owns
///    the seam and drives both directions from it. A `Wire` needs no
///    interior handle duplication, and connections never send on socket
///    clones.
/// 4. **[`testutil::FlakyWire`] is a `Wire`** — the in-memory
///    implementation the paused-clock flow tests ride (§16.10), which is
///    why every timer in the spec is testable without a kernel, a port,
///    or a sleep.
///
/// # A failing `send_to` is traced, not acted on
///
/// When `send_to` returns `Err`, the driver **must** trace it under
/// `slither::io` (§18.2) against the connection whose datagram it was,
/// carrying the destination address and the underlying error. It does
/// **not** kill the connection, resolve any verb with an error, or
/// produce a `Notification`: liveness is receive-driven (§7.4), and
/// `ENETUNREACH` is the signal that *precedes* a successful roam (§7.3),
/// not one that follows a dead connection.
#[allow(async_fn_in_trait)] // the actor is single-threaded; no Send bound is wanted here.
pub trait Wire {
    /// Send `buf` to `addr`, returning the bytes written.
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize>;
    /// Receive one datagram into `buf`, returning its length and source.
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
}

impl Wire for tokio::net::UdpSocket {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        tokio::net::UdpSocket::send_to(self, buf, addr).await
    }
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        tokio::net::UdpSocket::recv_from(self, buf).await
    }
}
```

The trait body is a byte-for-byte transcription of §16.3's code block.
The rustdoc is the four normative properties plus the trace obligation,
because §16.3 declares the seam "normative, not an implementation note".

### 7.2 The `async fn` mechanism, at MSRV 1.96 with no `Send`

**Use native AFIT — `async fn` in trait, stabilised 1.75, well under
MSRV 1.96 — exactly as the spec's code block writes it.** The spec's
block is normative; transcribing it is not a preference.

The three candidates and their trade-offs:

| Mechanism | Verdict |
|---|---|
| **AFIT** (`async fn` in trait) | **Chosen.** Desugars to RPITIT: no allocation, no dependency, and the returned future is `Send` **iff** the impl's future is — i.e. `!Send` is admitted by construction, never required. Costs: (a) the trait is **not dyn-compatible**, so `Box<dyn Wire>` is impossible; (b) the `async_fn_in_trait` lint fires, warning that callers cannot add a `+ Send` bound — which is precisely what we want, so `#[allow]` with a comment saying so. |
| **Explicit RPITIT** (`fn send_to(&self, …) -> impl Future<Output = …> + '_`) | Equivalent in every way that matters, and it silences the lint by being explicit. Rejected only because it is **not what §16.3 writes**, and §16.3 is normative. Worth a ruling request if the lint proves noisy under `-D warnings`. |
| **`async-trait`** | **Rejected.** It boxes every future — an allocation per datagram on the hottest path in the crate — adds a proc-macro dependency to the audit surface, and defaults to adding `Send` bounds (`#[async_trait(?Send)]` is needed to avoid it). A mechanism whose *default* violates a `CLAUDE.md` invariant is the wrong mechanism. |

**The consequence that reaches slice 2 — decided, not left to
accident (Q-O2, answered):** because AFIT is not dyn-compatible,
`Endpoint` cannot hold a `Box<dyn Wire>`. **Slice 2 uses
`Endpoint<W: Wire>`**, generic; that parameter leaks into `Connecting`
and possibly into the staged handles. §2.2 already makes the shell type
generic over the suite (`Endpoint<C: Channel>`), so this is a second
parameter, not the first, and AFIT makes it near-forced anyway.

The alternative stays available and is not foreclosed: if slice 2 or
later wants type erasure, a **private** `DynWire` shim in `shell/` —

```rust
trait DynWire {
    fn send_to<'a>(&'a self, buf: &'a [u8], addr: SocketAddr)
        -> Pin<Box<dyn Future<Output = io::Result<usize>> + 'a>>;   // no Send
    fn recv_from<'a>(&'a mut …) -> …;
}
impl<W: Wire> DynWire for W { … }
```

— erases it without touching the public trait, without a spec change, and
without a `Send` bound. Slice 0's obligation is to write the trait as
specified; the choice above is recorded here so slice 2 implements a
decision rather than discovering a constraint.

### 7.3 What `shell/wire.rs` must NOT contain

- **No driver.** No `select!`, no receive loop, no buffer pool. The
  `!Send` actor is slice 3.
- **No trace call.** The `slither::io` obligation is the *driver's*
  (§18.2 says "the driver MUST trace it"), not the trait's. A `Wire` impl
  that traced its own failures would double-count and would put the
  obligation on the application's implementation, which is precisely
  backwards.
- **No `Endpoint`, no `EndpointBuilder`.** §16.2 is slice 2.
- **No blanket impl for `Rc<W>`/`&W`.** Tempting and harmless-looking;
  it is public surface with no story, and it interacts with the
  generic-vs-dyn question above. Add it when something needs it.
- **No `local_addr()` on the trait.** The spec's trait has two methods.
  An endpoint that needs its bound address gets it from the builder.

### 7.4 Tests

1. `udp_socket_round_trip` — two real `tokio::net::UdpSocket`s bound to
   `127.0.0.1:0`, one datagram each way through the **trait** methods
   (`Wire::send_to`, not the inherent ones), asserting bytes and source.
   The one test in this slice that touches the kernel, and it is here to
   prove the blanket impl is not a fiction. `#[tokio::test]`, no paused
   clock (a real socket needs a real reactor).
2. `wire_is_object_unsafe_by_design` — not a test, a **doc comment**.
   There is no way to assert dyn-incompatibility in a test; record it in
   rustdoc so slice 2 meets it as documentation rather than as a compiler
   error at 2 a.m.
3. `a_wire_need_not_be_send` — a compile-fence: define a local
   `struct NotSend(std::rc::Rc<()>)`, `impl Wire for NotSend`, and drive
   it through a generic `async fn drive<W: Wire>(w: &W)`. If anyone ever
   adds a `Send` bound to the trait or to a generic helper, **this test
   stops compiling.** This is the mechanical guard for `CLAUDE.md`'s
   architecture invariant, and it is cheap. It should exist from slice 0
   and never be deleted.

---

## 8. `src/testutil/mod.rs`

> **The attestation gap is closed. §16.10 gained ruling 60 on
> 2026/08/14**, and this file is now implementing a normative surface,
> not a test convention. Four things follow, and each is a MUST:
>
> 1. **All three names are contract** — `testutil::Network`,
>    `testutil::FlakyWire`, `testutil::FlakyPolicy` — *"on the same terms
>    as §18.2's trace targets: renaming or dropping one is a protocol
>    revision, because a consumer's test suite is built on them."* So
>    slice 0 may not rename them for taste, and neither may any later
>    slice.
> 2. **`Network` is "the in-memory routing fabric… it owns the
>    address→endpoint map and moves datagrams between `FlakyWire`s"** —
>    §8.2 below is that sentence, implemented.
> 3. **`FlakyPolicy` carries loss, reordering, duplication *and send
>    failure*, and MUST be deterministic under a caller-supplied seed.**
>    Both halves are normative — §8.3 and §8.4.
> 4. **Send-failure injection "belongs in `FlakyPolicy` from the start:
>    retrofitting it later would rewrite the tests of every slice that
>    had already ridden the fixture."** It is slice 0's, explicitly.
>
> This closes S24's attestation gap, which the story listed as owed.

### 8.1 Module gate

```rust
#[cfg(any(test, feature = "test-util"))]
pub mod testutil;
```

so slither's own tests get it implicitly and a downstream crate opts in
with `features = ["test-util"]`. `docs.rs` already builds
`all-features = true`, so it is documented.

### 8.2 `Network` — how datagrams are routed

```rust
/// An in-memory datagram fabric: a set of addresses, each with an inbox,
/// and a policy per sending endpoint. Single-threaded by construction
/// (`Rc`, `RefCell`) — same shape as the `!Send` driver it feeds.
pub struct Network(Rc<RefCell<Inner>>);

impl Network {
    /// A network with a fixed RNG seed. Every delivery decision is a
    /// function of this seed and the per-wire send order — **the same
    /// test yields the same drops, delays and duplicates on every run.**
    pub fn seeded(seed: u64) -> Network;
    /// `seeded(0)`. There is no OS-entropy constructor: a testutil whose
    /// failures are irreproducible is worse than no testutil.
    pub fn new() -> Network;

    /// Register `addr` and return the `Wire` bound to it.
    pub fn endpoint(&self, addr: SocketAddr) -> FlakyWire;

    // Topology controls
    pub fn partition(&self, addr: SocketAddr);              // addr sends and receives nothing
    pub fn heal(&self, addr: SocketAddr);
    pub fn block_path(&self, from: SocketAddr, to: SocketAddr);   // one direction
    pub fn heal_path(&self, from: SocketAddr, to: SocketAddr);

    // Observation
    pub fn tap(&self) -> Tap;                       // records every accepted send
    pub fn sends(&self) -> usize;                   // total send_to calls, drops included

    // Forgery — a datagram from an address no `FlakyWire` owns.
    pub fn inject(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8]);
}

/// One observed datagram. Slice 1's golden-wire assertions and Appendix
/// B's "no further msg1 leaves the endpoint after the drop" both read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spied { pub src: SocketAddr, pub dst: SocketAddr, pub bytes: Vec<u8> }

pub struct Tap(/* Rc<RefCell<Vec<Spied>>> */);
impl Tap {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn drain(&self) -> Vec<Spied>;
    pub fn snapshot(&self) -> Vec<Spied>;
}
```

**Routing, step by step**, for `FlakyWire { addr: src }.send_to(buf, dst)`:

1. Bump `sends`. (Counted **before** any policy decision, so a test can
   distinguish "we tried to send" from "it arrived" — which is what
   ruling 50's "no further msg1 leaves the endpoint" assertion needs.)
2. If the policy has an active **send failure** for `now`, return
   `Err(io::Error::from_raw_os_error(ENETUNREACH))` — nothing is queued,
   nothing is tapped. (§8.4.)
3. If `src` is partitioned, or the path `src → dst` is blocked, return
   `Ok(buf.len())` and drop silently. A blackhole is *not* a send error:
   this is the topology control, and conflating the two would make the
   ruling-49 fixture untestable.
4. Record in the tap.
5. Draw `deliveries` from the policy (0 = lost, 1 = normal, 2 =
   duplicated) — §8.3.
6. For each delivery, draw a delay and push `(src, bytes.to_vec(),
   deliver_at = Instant::now() + delay)` into `dst`'s inbox, then
   `Notify::notify_waiters()`.
   - If `dst` is not registered, the datagram is dropped (a real socket
     would get an ICMP port-unreachable at best; slither ignores those).
   - If `dst` is partitioned, dropped.
7. Return `Ok(buf.len())`.

**Reordering falls out of the delay, not out of a shuffle.** Two
datagrams whose independently-drawn jitters cross arrive out of order.
That is the honest mechanism (it is how a real network reorders), it
composes with the paused clock for free, and it means "reorder" needs no
separate knob. The inbox is therefore a `BinaryHeap` keyed on
`(deliver_at, sequence)` — the sequence number breaking ties keeps
equal-deadline datagrams FIFO and **deterministic**.

`recv_from(&self, buf)`:

1. Peek the earliest `deliver_at`. If the heap is empty,
   `Notify::notified().await` and retry.
2. `tokio::time::sleep_until(deliver_at).await`. **On the paused clock
   this auto-advances virtual time when every task is idle — this line is
   the whole of §16.10's kernel-free drivability.**
3. **Then** pop, copy `min(len, buf.len())` bytes, and return
   `(copied, src)`.

**Popping after the sleep, never before, is what makes `recv_from`
cancel-safe** — and cancel-safety is not optional here: the driver
`select!`s over `recv_from` against the command channel and the timer, so
a dropped `recv_from` future must lose nothing. It gets its own test
(§8.6 test 5). Getting this backwards is the single most likely
silent-data-loss bug in the whole slice, and it would present in slice 3
as an unreproducible flake.

**Truncation.** `recv_from` copies `min(len, buf.len())` and returns the
copied count — matching `tokio::net::UdpSocket::recv_from` (POSIX
`recvfrom` without `MSG_TRUNC`). The driver's buffer is `MAX_DATAGRAM`,
so an oversize datagram arrives truncated and dies at §3.5's length gate.
Tested, because a `FlakyWire` that silently delivered 2 KiB into a
1200-byte buffer would make the length gate look correct while it never
fires.

### 8.3 `FlakyPolicy` — deterministic loss, reorder and duplication

```rust
#[derive(Clone, Debug)]
pub struct FlakyPolicy {
    /// P(drop) per datagram, 0.0–1.0.
    pub loss: f64,
    /// P(deliver a second copy), 0.0–1.0.
    pub duplicate: f64,
    /// Minimum one-way delay.
    pub base_delay: Duration,
    /// Uniform additional delay in `[0, jitter)`. **Reordering lives
    /// here**: two datagrams whose draws cross swap.
    pub jitter: Duration,
    /// Drop the next N datagrams unconditionally, then behave normally.
    pub drop_first: usize,
    /// Drop exactly these 0-based send indices — no RNG involved.
    pub drop_at: BTreeSet<usize>,
    /// Fail `send_to` with this error while `now` is inside the window.
    pub send_failure: Option<SendFailure>,
}

#[derive(Clone, Debug)]
pub struct SendFailure { pub kind: io::ErrorKind, pub raw_os: i32, pub until: Instant }

impl FlakyPolicy {
    pub fn perfect() -> Self;                                   // no loss, no delay
    pub fn drop_first(n: usize) -> Self;
    pub fn drop_at(indices: impl IntoIterator<Item = usize>) -> Self;
    pub fn lossy(rate: f64) -> Self;
    pub fn with_delay(self, base: Duration, jitter: Duration) -> Self;
    pub fn with_duplication(self, rate: f64) -> Self;
    /// `send_to` returns ENETUNREACH until `until`, then heals. The
    /// ruling-49 / S25 fixture (Appendix B).
    pub fn failing_sends_until(self, until: Instant) -> Self;
}
```

**The determinism contract. Normative under ruling 60** — *"It MUST be
deterministic under a caller-supplied seed. A flow test that cannot be
replayed byte-for-byte from its seed is not a regression test, and the
loss-dependent behaviour in §13 and §7.5 is exactly where a
once-in-a-thousand-runs failure would otherwise be unactionable."*
The rules below are how that MUST is met; test 3 in §8.7 is how it is
pinned.

Note the phrase **caller-supplied**: `Network::seeded(seed)` is the
normative constructor and `Network::new()` is sugar for `seeded(0)`.
There is no OS-entropy path, and adding one later would violate the
ruling.

- **One `ChaCha20Rng` per `FlakyWire`, not one per `Network`.** Seeded
  `ChaCha20Rng::seed_from_u64(net_seed ^ (ordinal as u64).wrapping_mul(0x9E3779B97F4A7C15))`
  where `ordinal` is the wire's registration index. A per-wire stream
  means **adding a third endpoint to a test does not reshuffle the first
  two's draws** — a shared RNG would make every existing assertion in the
  file move when someone adds an endpoint, and that property is worth
  more than the marginal simplicity.
- **A fixed draw order per `send_to`**, documented in the source:
  1. `drop_at` / `drop_first` — index-based, **no draw**;
  2. one `f64` draw for `loss`;
  3. one `f64` draw for `duplicate`;
  4. one `u64` draw per delivery for `jitter`.
  Changing this order changes every seeded test's outcome, so it is
  part of the contract, not an implementation detail.
- **No wall-clock, no `SystemTime`, no thread-id, no `HashMap` iteration
  order** anywhere in a decision path. Registration order is an explicit
  counter, not a hash order.
- **`drop_at` / `drop_first` are the sharp tools.** Probabilistic loss is
  reproducible under a seed but brittle: it moves when an unrelated send
  is added. Appendix B's obligations ("the first two msg1s die", "a
  sustained ACK-loss burst") want exact control, so ship both and
  recommend index-based policies for anything asserting a specific
  outcome.

### 8.4 Send-failure injection — normative, slice 0's, ruling 60

The draft raised this as a gap in the brief. It is now settled in the
spec's own text. §16.10:

> **"Send-failure injection is required, not optional."** Ruling 49 makes
> a failing `send_to` a trace obligation, and Appendix B's obligation for
> it is unreachable without a fixture that can fail a send. **It belongs
> in `FlakyPolicy` from the start: retrofitting it later would rewrite
> the tests of every slice that had already ridden the fixture.**

and Appendix B, unchanged:

> *"Give the endpoint a `Wire` whose `send_to` returns `ENETUNREACH` for
> a bounded interval, then heals. […] A `FlakyWire` that can fail sends
> is the fixture; the obligation is unreachable without one."*

So `SendFailure` and the `failing_sends_until` constructor are slice 0
deliverables, not slice 7's. Cost: ~20 lines and one branch in `send_to`.
Test 10 in §8.7 proves it works two slices before anything depends on it.

### 8.5 `FlakyWire`, and the counting identity

```rust
/// The in-memory `Wire` (§16.3, §16.10). Named in the spec; `Network`
/// and `FlakyPolicy` are not (S24's attestation gap).
pub struct FlakyWire { /* addr, Rc<RefCell<Inner>>, RefCell<ChaCha20Rng>, RefCell<FlakyPolicy> */ }

impl FlakyWire {
    pub fn local_addr(&self) -> SocketAddr;
    pub fn set_policy(&self, policy: FlakyPolicy);
    pub fn policy(&self) -> FlakyPolicy;                 // a clone, not a borrow
}

impl Wire for FlakyWire {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize>;
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
}
```

`&self` on both, per §16.3 property 3, so all mutable state sits behind
`RefCell`/`Cell`. **`FlakyWire` is deliberately `!Send`** (`Rc` inside) —
it is the fixture for an actor that must not require `Send`, so a `Send`
`FlakyWire` would let a `Send` bound creep into the driver unnoticed. The
`a_wire_need_not_be_send` fence in §7.4 and this are the same guard from
two directions.

#### The counting DH provider

```rust
/// Wraps a `DhProvider<P256>` and counts every `dh` call. Later slices
/// assert the §6.1 ladder: `dhs == 0` at park, `1` after
/// `read_identity()`, `2` after `authenticate()`, and `4` per
/// cancel-and-redial cycle (S29).
pub struct CountingProvider<P> { inner: P, dhs: Rc<Cell<u32>> }

impl<P: DhProvider<P256>> DhProvider<P256> for CountingProvider<P> { /* dh() increments, then delegates */ }
impl<P: CryptoKeyProvider<P256>> CryptoKeyProvider<P256> for CountingProvider<P> { /* pure delegation */ }

/// A shared, cheap handle on the count. `Rc<Cell<_>>`: `!Send`, like
/// everything on the actor path.
#[derive(Clone, Debug)]
pub struct DhCounter(Rc<Cell<u32>>);
impl DhCounter {
    pub fn new() -> Self;
    pub fn get(&self) -> u32;
    pub fn reset(&self);
    pub fn provider<P>(&self, inner: P) -> CountingProvider<P>;
}
```

**Counting rule: exactly one increment per `DhProvider::dh` call, and
nothing else counts.** Key generation is not a DH. This has to be exact,
because §6.1's whole design argument is "one DH to inspect, two to
authenticate" and the ladder assertions are how it is enforced.

**The `Identity` impl lands in slice 2, not slice 0 (Q-O1, confirmed).**
`CountingIdentity` must `impl Identity`, and `Identity` is the trait
§16.4 names in `core::Endpoint<I: Identity>` — a **slice 2** file that
does not exist, and which the v0.2 module map gives no home to at all
(v0.1 kept it in `handshake.rs`, which v2 deletes). So:

- **Slice 0 ships `CountingProvider` + `DhCounter`** — the DH counter is
  the deliverable the later slices assert on, and it depends only on
  hiss, which exists today.
- **Slice 2 adds `CountingIdentity: Identity`** in the same file, when
  the trait exists. **No module-map deviation** — nothing new is minted
  in slice 0 and `src/identity.rs` is not created.

This defers nothing that slices 0–1 need: neither drives a handshake.

### 8.6 What `testutil` must NOT contain

- **No packet construction.** No msg1 builder, no `Spied::is_handshake()`,
  no header parsing in `Spied`. `Spied` carries `Vec<u8>`; slice 1 is
  where bytes acquire meaning.
- **No `software_identity()` helper** (v0.1 had one) until `Identity`
  exists — same reason as above.
- **No two-endpoint fixture.** There are no endpoints. Slice 2 adds it,
  and it belongs in `testutil` when it does.
- **No `tracing-subscriber` capture helper.** Slice 7's need; adding it
  drags in a dependency slice 0 cannot justify.
- **No real-clock sleeps anywhere.** `tokio::time::sleep_until` only.

### 8.7 Tests

1. **`a_byte_crosses_two_flaky_wires_under_injected_loss`** — the
   definition-of-done test. See §9.
2. `perfect_policy_delivers_everything_in_order` — 100 datagrams, no
   loss, no jitter, arrive in order with identical bytes.
3. `seeded_runs_are_identical` — build the same `Network::seeded(7)`
   scenario twice with `loss: 0.3, duplicate: 0.1, jitter: 20ms`, record
   the full `(order, bytes, arrival_instant)` trace of each, and assert
   the two traces are equal. **This is the reproducibility contract, and
   it is a test rather than a comment.**
4. `different_seeds_diverge` — the same scenario under two seeds produces
   different traces. Guards against a policy that silently ignores the
   RNG (a `loss` field never read would pass test 3 and fail this).
5. **`recv_from_is_cancel_safe`** — queue one datagram; create a
   `recv_from` future, poll it once, drop it before its `deliver_at`;
   then `recv_from` again and assert the datagram still arrives, intact
   and once. §8.2's pop-after-sleep rule, mechanically enforced.
6. `jitter_reorders_and_the_heap_is_stable` — with `jitter` large
   relative to send spacing, delivery order differs from send order at
   least once across a seeded run; and with `jitter = 0`, equal deadlines
   deliver FIFO (the sequence tie-break).
7. `duplication_delivers_two_identical_copies`.
8. `drop_at_is_exact` — `drop_at([1, 3])` over five sends delivers
   exactly indices 0, 2, 4, with **no RNG draw** consumed (assert by
   running the same scenario with two different seeds and getting the
   same result).
9. `partition_blackholes_without_a_send_error` — `send_to` returns
   `Ok(len)` while nothing arrives. The distinction §8.2 step 3 draws.
10. **`send_failure_is_an_err_and_then_heals`** — with
    `failing_sends_until(t0 + 3s)`: `send_to` returns
    `Err(ENETUNREACH)`, nothing is queued and **nothing is tapped**;
    after advancing the paused clock past `t0 + 3s`, the next `send_to`
    succeeds and arrives. The ruling-49 fixture, proved usable before
    slice 7 depends on it.
11. `oversize_datagram_is_truncated_like_a_socket` — send 2000 bytes,
    receive into `[0u8; 1200]`, get `Ok((1200, src))`.
12. `inject_forges_a_source` — `inject(unregistered_addr, dst, b"x")`
    arrives at `dst` reporting `unregistered_addr`. The fixture slices
    1–2 need for mac1 garbage and off-path spoofing.
13. `unregistered_destination_is_dropped_not_an_error`.
14. `dh_counter_counts_only_dh` — a `CountingProvider` over a real hiss
    `EphemeralOnly` provider: generating a keypair leaves the count at 0;
    one `dh()` makes it 1; `DhCounter` clones observe the same value.
15. `the_whole_module_is_not_send` — a compile-fence mirroring §7.4's:
    `FlakyWire` and `Network` are used from a `!Send` context and the
    file never names `Send`.

Tests 1, 5, 6 and 10 are the ones that matter; the rest are cheap and
guard regressions. Tests 3 and 5 in particular are the ones I would write
**first** — an in-memory network that is either nondeterministic or
cancel-unsafe poisons every slice above it, and both failures present as
"a flaky test in slice 3", days after the cause.

---

## 9. The definition-of-done test

```rust
#[tokio::test(start_paused = true)]
async fn a_byte_crosses_two_flaky_wires_under_injected_loss() {
    let net = Network::seeded(0xA11CE);
    let a = net.endpoint("10.0.0.1:4001".parse().unwrap());
    let b = net.endpoint("10.0.0.2:4002".parse().unwrap());

    // The first two datagrams die; the third gets through. Index-based,
    // so the outcome does not depend on an RNG draw.
    a.set_policy(
        FlakyPolicy::drop_at([0, 1])
            .with_delay(Duration::from_millis(50), Duration::from_millis(10)),
    );

    let t0 = tokio::time::Instant::now();
    for _ in 0..3 {
        a.send_to(b"!", b.local_addr()).await.unwrap();
    }

    let mut buf = [0u8; 1200];
    let (n, src) = b.recv_from(&mut buf).await.unwrap();

    assert_eq!(&buf[..n], b"!");
    assert_eq!(src, a.local_addr());
    assert_eq!(net.sends(), 3, "all three left the wire");
    assert_eq!(net.tap().len(), 3, "all three were tapped; two were dropped after");

    // Virtual time advanced by the injected one-way delay …
    let elapsed = tokio::time::Instant::now() - t0;
    assert!(
        (Duration::from_millis(50)..Duration::from_millis(60)).contains(&elapsed),
        "delivery waited base_delay + jitter in virtual time, got {elapsed:?}",
    );

    // … and nothing else is coming.
    assert!(
        tokio::time::timeout(Duration::from_secs(1), b.recv_from(&mut buf))
            .await
            .is_err(),
        "the two dropped datagrams never arrive",
    );
}
```

Four things it proves, which is why it is the gate:

1. A byte crosses A → B through the `Wire` trait, over `FlakyWire`.
2. An injected loss policy really drops (2 of 3), deterministically.
3. **Virtual time advances** — the assertion on `elapsed` fails if
   `recv_from` busy-waits or if the `sleep_until` is missing, and the
   whole test runs in ~0 ms of wall clock. This is §16.10 in miniature.
4. `timeout` composes with `recv_from` — i.e. `recv_from` is a
   well-behaved future under cancellation, which is the property the
   driver's `select!` will rely on in slice 3.

Add a wall-clock guard so a regression to real sleeps is loud rather than
slow: assert `std::time::Instant::now() - wall_t0 < Duration::from_secs(1)`.

---

## 10. Build order and parallelism

```
        ┌─ (serial) git rm -r src/ examples/ ─┐
        └─ (serial) Cargo.toml + lib.rs stub ─┘
                          │
         ┌────────────┬───┴────────┬─────────────┐
      constants.rs  varint.rs   error.rs   shell/{mod,wire}.rs      ← wave 1, parallel
         └────────────┴────────────┴─────────────┘
                          │
                  testutil/mod.rs                                   ← wave 2 (needs Wire)
                          │
                  lib.rs crate docs                                 ← wave 3 (serial, last)
                          │
                  the eight gates
```

**The serial prologue is mandatory.** One agent deletes `src/`, writes
`Cargo.toml`, and writes a **stub `lib.rs`** containing only the crate
attributes and the full module tree:

```rust
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod constants;
pub mod error;
pub(crate) mod varint;
pub mod shell;

#[cfg(any(test, feature = "test-util"))]
pub mod testutil;

pub use error::{
    AcceptError, AuthError, ConfigError, ConnectError, ConnectionLost,
    DatagramError, IntroError, MessageError, ReadError, WriteError,
};
```

Doing this first means **no wave-1 agent ever edits `lib.rs`**, which is
the only file all four would otherwise contend for.

**Wave 1 — four agents, no file overlap:**

| Agent | Files | Blocked by | Notes |
|---|---|---|---|
| A | `src/constants.rs` | needs `varint::VarInt::MAX_VALUE` to *exist* by name | agree the name in the prologue; the const-assert is the only coupling |
| B | `src/varint.rs` | — | fully independent |
| C | `src/error.rs` | — | fully independent |
| D | `src/shell/mod.rs`, `src/shell/wire.rs` | — | fully independent |

A and B share exactly one symbol (`VarInt::MAX_VALUE`), fixed by this
plan. If the orchestrator prefers zero coupling, drop the three
varint-fit assertions from `constants.rs` and move them into
`varint.rs`'s tests — a small loss (they become runtime, not compile
time) for full independence.

**The author swap applies to exactly two files — `constants.rs` and
`error.rs` — and nowhere else (Q-O5, answered).**

`constants.rs`'s transcription test (§4.5 test 1) and `error.rs`'s
exhaustive-match fence (§5.6 test 1) are written by a *different* agent
than the one writing the file, from `SPEC.md` first. Concretely: **agent
A writes `constants.rs`, agent C writes its transcription test; agent C
writes `error.rs`, agent A writes its fence.**

The reason is *not* `CLAUDE.md`'s working rule 6, which names
story-level acceptance tests and slice 0 closes no story. It is narrower
and sharper: **an implementer who mis-transcribes a constant will
mis-transcribe it into its own test identically, and both will look
right.** That is the self-consistency failure the rule exists for, and
transcription is where it bites hardest — a wrong `IK_MSG1_LEN` with a
matching test is a green build and a broken wire.

Everywhere else in slice 0 — `varint.rs`, `shell/wire.rs`,
`testutil/mod.rs` — **the implementer writes their own tests.** Those
files encode behaviour that fails visibly; they are not transcription.

**Wave 2** is one agent on `testutil/mod.rs` — the largest single file in
the slice (~600 lines with tests) and the one with the real design
content. It needs `shell::wire::Wire` to compile, so it cannot start
until D lands.

**Wave 3** is the crate docs in `lib.rs`, written last because they cite
what exists.

**Not parallelisable:** the gate run. All eight, in order, on one tree.

---

## 11. Consistency check

### Does slice 0 foreclose a later slice?

**No, with three caveats already recorded.**

- **The `Wire` trait's dyn-incompatibility** (§7.2) constrains slice 2's
  `Endpoint` to a generic parameter or a private erasure shim. It does
  not foreclose either. Q-O2 settles it: **generic**, with the shim
  available later.
- **`varint` as `pub(crate)`** (§6.1) can be promoted without a break.
- **`CountingIdentity` deferred to slice 2** (§8.5) — nothing in slices
  0–1 drives a handshake, so nothing is blocked.

### Does `error.rs` cover every error the later slices need?

I walked slices 1–8 against the ten types. Coverage:

| Slice | Errors needed | Covered? |
|---|---|---|
| 1 packets | version/prologue mismatch, mac1 failure, oversize | ✅ **silent drop** — §3.1/§3.5/§4 make all three drops, not errors. No variant needed, by design |
| 2 handshake/staged | `ConnectError`, `IntroError`, `AuthError`, `AcceptError` | ✅ all four, complete |
| 3 skeleton/CLOSE | `ConnectionLost` ×7, `ConfigError` ×2 | ✅ — **provided `ConfigError` ships**, which is why §5 adds it |
| 4 streams/flow | `WriteError`, `ReadError` | ✅ — and `open_uni()` under an exhausted stream credit **waits** (§16.2: "`send_message` waits for stream allowance"), so no `StreamLimit` variant is needed |
| 5 reliability | — | ✅ recovery surfaces nothing new; loss is invisible above the seam |
| 6 sugar | `MessageError::TooLarge`, `DatagramError::TooLarge`, `MESSAGE_OVERFLOW` on the receive side | ✅ — the overflow reaches the sender as `WriteError::Reset(0x06)` (ruling 52) and the receiver as a **trace only** (ruling 59), which is why no variant exists for it |
| 7 mobility | roaming, keepalive, contested | ✅ — **the contested probe deliberately shares `ConnectionLost::TimedOut`** (§15.4: "the same variant, no new one"). A reviewer will want to add `Contested`; ruling 45 already declined it — it surfaces as a `Notification`, not an error |
| 8 composability | `io::Error` conversions | ✅ — conversions, not variants |

**The one residual gap the draft found is now closed by ruling 62.**
`ConnectError` has **no `EndpointDropped`**, while `IntroError`,
`AuthError`, `AcceptError` and `ConnectionLost` all do. The draft argued
this was safe because a `Connecting` must be a handle — §16.3 says *"the
driver lives while any handle lives"*, and ruling 50 makes dropping a
`Connecting` a state-changing event, which only makes sense if it is one.

That is now ratified, and generalised past the special case:

> **A future that changes protocol state when dropped is a handle; one
> that does not, is not.** (§16.3, ruling 62)

A `Connecting` **owns an in-flight protocol attempt** — a pending, its
index, and §5.5's retransmit train — so it is a handle and the driver
cannot stop beneath it. A `closed()` future owns nothing and merely
observes, so it is not. The consequence is stated in the spec:
`ConnectError` needs no `EndpointDropped`, §18.1 stays closed as written,
and **the asymmetry against the staged errors is correct rather than an
omission** — a staged verb is a round-trip to a driver it does not keep
alive; an outbound attempt keeps its own driver running.

Slice 0 therefore ships `ConnectError` with exactly two variants, and
`src/error.rs` carries that comment so nobody re-derives the question.

### Feature layout vs `PLAN.md` §3

| PLAN.md §3 | This plan |
|---|---|
| `sink` = futures-core + futures-sink | `sink = ["dep:futures-core", "dep:futures-sink"]` ✅ |
| `codec` = tokio-util, **implies `sink`** | `codec = ["sink", "dep:tokio-util"]` ✅ |
| `tower` = tower-service only | `tower = ["dep:tower-service"]` ✅ |
| §3.1 `AsyncRead`/`AsyncWrite` — **no feature**, tokio already a dep | no feature added ✅ |
| `test-util` exposes `testutil` | unchanged from v0.1 ✅ |

Match. Four features, no more, no defaults.

### `CLAUDE.md`: no `Send` on the actor path

**Nothing proposed adds one, and two mechanical guards are added:**

- `Wire` is AFIT — the returned futures are `Send` only if an impl's are.
  `async-trait` was rejected specifically because its default adds them.
- `FlakyWire`, `Network`, `DhCounter`, `CountingProvider` are all
  `Rc`-based and therefore `!Send` **by construction**, so a `Send` bound
  added anywhere on the actor path fails to compile against the fixture.
- `shell/wire.rs`'s `a_wire_need_not_be_send` (§7.4 test 3) is a
  permanent compile-fence.
- The **one** place `Send` is asserted is on the error types (§5.6 test
  5), which are values, not handles. I read that as outside the rule; if
  the orchestrator disagrees, drop the test — nothing depends on it.

### `CLAUDE.md`: zero `bubble-*` dependencies

Confirmed. The full slice-0 dependency set is `hiss`, `cryptoxide`,
`packtool`, `tokio`, `thiserror`, `rand_chacha`, `rand_core`,
`getrandom`, `tracing`, plus the four optional adapter crates
(`futures-core`, `futures-sink`, `tokio-util`, `tower-service`). All
thirteen resolve from crates.io. No path deps, no git sources —
`deny.toml` already denies both.

### Other

- **`Cargo.lock` stays uncommitted.** `.gitignore` already lists it.
  A `Cargo.lock` currently exists on disk, untracked — correct.
- **`.slices` in `Cargo.toml`'s `exclude` — done** (Q-O4, applied by the
  maintainer). Slice 0 verifies rather than re-adds.
- **`PLAN.md` §1's "four documentation obligations" — fixed** (Q-O4);
  §1 and §9 now both say five. Slice 0's `lib.rs` writes five.

---

## 12. Risks — resolved and remaining

### Resolved since the draft

**R1 — `MESSAGE_OVERFLOW` (0x06). ✅ RESOLVED, and the defect was wider
than the draft found.** The draft found the Named-constants table still
reading `0x00`–`0x05` against §15.3's `0x06 MESSAGE_OVERFLOW` (ruling
52). Ruling 52's application had in fact updated the two **normative**
sections (§15.3, §9.8) and missed **three** restatements, found by three
different routes: the Named-constants table (this plan), **§18.1's own
closing sentence** (the maintainer), and a §9.8 prose paragraph still
arguing the code was not worth minting (round 9). All three are now
fixed. §15.3 was authoritative exactly as recommended.

**The transferable lesson, which is `CLAUDE.md` working rule 4 in
concrete form:** a ruling that changes a value must be verified by
grepping for the **rationale**, not only the token. Two of the three
misses were prose that still argued the reversed position while quoting
no number at all. Slice 0's §4.5 test 4 is the standing mechanical guard
for the one restatement that lives in code.

**R2 — `ConfigError` missing from the brief. ✅ RESOLVED — the brief was
wrong.** `error.rs` ships all ten types.

**R3 — send-failure injection missing from the brief. ✅ RESOLVED, and
now normative** — §16.10, ruling 60, which additionally makes seeded
determinism a MUST and attests `Network` / `FlakyWire` / `FlakyPolicy` as
contract. **S24's attestation gap is closed.**

**R4 — `CountingIdentity` in slice 0. ✅ RESOLVED** (Q-O1). Provider and
counter now; `Identity` impl in slice 2. No module-map deviation.

**R5 — the `Wire` trait's dyn-incompatibility. ✅ RESOLVED** (Q-O2):
`Endpoint<W: Wire>`, generic. A `DynWire` shim stays available later
without a spec change.

**R-new — `#[non_exhaustive]`: the draft's recommendation was wrong, and
the correction is worth keeping.** The draft argued for it on all ten
types, weighing only the ergonomic cost of the consumer's `_` arm. It
never weighed what that arm *does* when a variant lands: it silently
routes a new error into a branch written for the old ones. Ruling 61
takes the compile break instead, on `WriteError` alone (§19 reserves
`Stopped`). Recorded here so the uniform-application argument is not
re-proposed by a later slice that notices the inconsistency.

### Still open



**R6 — twelve constant names do not exist in the spec.** `STATIC_PUBLIC_LEN`,
`STREAM_FLAG_MASK`, `CREDIT_REGRANT_DIVISOR`, `CLOSE_REPLY_MIN_INTERVAL`,
`ACK_ELICITING_PER_ACK`, `K_TIME_THRESHOLD_NUM`/`_DEN`,
`SHELL_LATENESS_BOUND`, `APPLICATION_ERROR_BASE`, the two
`PKT_RESERVED_*`, `FRAME_STOP_SENDING_RESERVED`, and the
`PERSISTENT_KEEPALIVE_DEFAULT`/`_MIN` split (the spec names one bare
`PERSISTENT_KEEPALIVE` and states its range in prose). Each is a **value**
the spec fixes and a **name** it does not. Every one gets a doc comment
saying "not a spec name; §X fixes the value". A reader grepping the spec
for `SHELL_LATENESS_BOUND` finds nothing and must be told why.
**Q-M3, half answered:** ship them with these names, doc-commented as
unattested; the amendment is written **after** `constants.rs` exists,
from real names rather than guesses. Full list in §15, "Owed to the
spec".

**R7 — `PTO_BACKOFF_CAP`. ✅ SETTLED by reading, no ruling needed.** It
is the **multiplier, 64**: §13.3 caps `2^pto_count` and §13.5 says "2⁶×
too long". Keep `assert!(PTO_BACKOFF_CAP == 1 << 6)`, which leaves both
readings legible in the source. Slice 5 clamps `pto_count ≤ 6` — the same
statement from the other side.

**R8 — I read v0.1 symbol names.** Declared at the top. The brief said
not to read `src/` for guidance. I read: a symbol-name-only overview of
`testutil.rs`, the 12-line `Identity` trait, and the 12-line `Wire`
trait + its `UdpSocket` impl. Justification: (a) S24 states a downstream
crate depends on `Network`/`FlakyWire`/`FlakyPolicy` **by name**, so the
names are an input to slice 0's API design, not guidance about a
superseded wire; (b) v0.1's `Wire` is *identical* to the now-ratified
§16.3 trait, so reading it confirmed the AFIT + `#[allow]` mechanism
already passes this repo's `-D warnings` gate — worth knowing before
committing to it. No wire logic, no packet code, no handshake code was
read. If the orchestrator judges this over the line, the plan stands
without it: every design decision above is derivable from `SPEC.md` and
`PLAN.md` alone, and the only thing lost is the name-stability argument.

**R9 — `recv_from` cancel-safety is the slice's likeliest silent bug.**
§8.2. It has a dedicated test (§8.7 test 5) and it should be written
before the implementation. If it is got wrong, it will not surface until
slice 3's driver `select!`s over it, and it will present as an
intermittent, unreproducible failure in a different file — the worst
possible debugging shape.

**R10 — `hiss::noise::datagram::MAX_EPOCH_JUMP` may not be reachable in
a `const` context under `default-features = false`.** I verified the
const exists and is `pub` in hiss 0.3.2; I did **not** compile against
it. If the path is not reachable, drop the const assert and keep §4.5
test 5 as the pin. Two minutes to check at implementation time; noted so
it is not discovered as a build break.

**R11 — feature-isolation builds. ✅ SPLIT** (Q-O3).
`--no-default-features` joins the `Check` job in slice 0 — meaningful
today, because `test-util` gates the whole `testutil` module. The three
per-feature builds move to **slice 8**, when `sink`/`codec`/`tower`
actually gate code; until then they compile nothing and prove nothing.
Residual risk carried to slice 8: a `codec`-only build that fails because
`sink`'s items sit behind a `cfg`. Noted in this plan so slice 8 inherits
it rather than rediscovers it.

---
## 13. Questions — all answered

No question in this plan blocks implementation. Recorded here as
decisions, with the reasoning, so a later slice does not re-litigate one.

### Maintainer rulings

| # | Question | Ruling |
|---|---|---|
| **Q-M1** → **ruling 61** | `#[non_exhaustive]` on every error type, or some? | **`WriteError` only; the other nine are exhaustive.** The draft's uniform recommendation was **declined**. A consumer matching without a `_` arm gets a compile error the day a variant is added — the loud failure worth having; a wildcard arm silently swallows a new error into a branch written for the old ones. §19 reserves `Stopped`, so `WriteError` alone demonstrably will gain one. §5.3 |
| **Q-M2** | Is `PTO_BACKOFF_CAP` the multiplier (64) or the exponent (6)? | **Settled by reading, no ruling.** The **multiplier, 64** — §13.3 caps `2^pto_count`, §13.5 says "2⁶× too long". Keep `assert!(PTO_BACKOFF_CAP == 1 << 6)`. §4.1 row 66 |
| **Q-M3** | Attest `Network`/`FlakyPolicy` and the twelve unnamed constants? | **Half applied, half deferred.** The fixture attestation is **ruling 60** (§16.10) — all three names are now contract. The constants: ship them with these names, doc-commented as unattested, and list them in §15 so the amendment can be written from real names once `constants.rs` exists. **Do not guess the spec text now.** |
| **Q-M4** → **ruling 62** | Is a `Connecting` a handle? | **Yes** — the draft's argument carried, and the spec now states the *principle* rather than the special case: **a future that changes protocol state when dropped is a handle; one that does not, is not.** `ConnectError` ships with exactly two variants; §18.1 stays closed; the asymmetry against the staged errors is correct, not an omission. §11 |
| **Q-M5** | The `MESSAGE_OVERFLOW` 0x06 conflict. | **Upheld, and wider than found.** §15.3 was authoritative; **three** restatements had gone stale (the Named-constants table, §18.1's closing sentence, a §9.8 prose paragraph). All fixed. Implement **`0x00`–`0x06`**, `0x07`–`0x0f` reserved, ≥ `0x10` application. §4.1, R1 |

### Orchestrator answers

| # | Question | Answer |
|---|---|---|
| **Q-O1** | `CountingIdentity` in slice 0 or slice 2? | **Slice 2.** `CountingProvider` + `DhCounter` in slice 0. No module-map deviation; `src/identity.rs` is not minted. §8.5 |
| **Q-O2** | `Endpoint<W: Wire>` or an erasure shim? | **Generic, `Endpoint<W: Wire>`.** AFIT's dyn-incompatibility makes it near-forced; a `DynWire` shim stays available later without a spec change. §7.2 |
| **Q-O3** | Feature-isolation builds in CI now? | **Split.** `--no-default-features` joins the `Check` job **now** (`test-util` gates `testutil`, so it is meaningful today). The three per-feature builds go to **slice 8** — a build that compiles nothing proves nothing. §3, R11 |
| **Q-O4** | `PLAN.md` §1's stale "four" doc obligations. | **Fixed**, along with `.slices` in `Cargo.toml`'s `exclude`. Slice 0 verifies both rather than re-applying. §11 |
| **Q-O5** | Does working rule 6's author swap apply? | **Keep it, narrowly — `constants.rs` and `error.rs` only**, and for a reason that is not the rule's: an implementer who mis-transcribes a constant mis-transcribes it into its own test identically, and both look right. Everywhere else in slice 0 the implementer writes their own tests. §10 |

---

## 14. Handoff checklist

Per `CLAUDE.md` working rule 7 — **do not report a gate as green without
running it; paste the command and its output.** Run in this order; a red
gate stops the slice.

### Pre-flight (not gates, but they fail fastest)

```bash
git -C . rm -r src examples          # §2 — one commit, naming 5324ce5
grep -n '"/\.slices"' Cargo.toml     # Q-O4 must still be applied
grep -n 'Cargo.lock' .gitignore      # must remain ignored (hiss convention)
```

### The eight gates

```bash
# 1 — Compiles
cargo build --all-features --all-targets

# 1b — feature floor (Q-O3; new to the Check job in this slice)
cargo build --no-default-features

# 2 — Format
cargo fmt --all --check

# 3 — Lints
cargo clippy --all-features --all-targets -- -D warnings

# 4 — Docs (both invocations, both with -D warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# 5 — Tests (both invocations)
cargo test
cargo test --all-features

# 6 — Wire pins: no golden vectors exist until slice 1. In slice 0 this
#     gate is the compile-time assertions in constants.rs (they cannot be
#     skipped) plus the transcription test. Name it explicitly:
cargo test --all-features constants

# 7 — MSRV
cargo +1.96 check --all-features --all-targets

# 8 — Supply chain
cargo deny check
```

### The slice-0 acceptance test, named

```bash
cargo test --all-features a_byte_crosses_two_flaky_wires_under_injected_loss -- --nocapture
```

### Report format

For each of the above: the command, and its output (or its tail plus the
summary line for the verbose ones). **Gate 6 needs a sentence**, because
it is the one that does not mean in slice 0 what it means in slice 1:
*"no golden-wire vectors exist yet; the wire pins in this slice are the
39 compile-time assertions in `constants.rs` and the transcription test,
both of which passed."* State it rather than letting a reader assume the
golden vectors ran.

### Hand-forward notes for slice 1

- The golden-wire vectors land in slice 1 and pin 174 / 81 / 196 / 107.
  `constants.rs` already asserts all four **as derivations**, so a
  disagreement between the vectors and the constants is a ruling request,
  never an expectation to update.
- `STATIC_PUBLIC_LEN` is pinned against `P256::PUBLIC_KEY_SIZE`; slice 1
  should **re-pin it against the `noise!`-declared channel's
  `PUBLIC_KEY_SIZE`** once that type exists, which is the tighter bound.
- Verify R10 at implementation time: whether
  `hiss::noise::datagram::MAX_EPOCH_JUMP` is reachable in a `const`
  context under `default-features = false`. If not, drop that one const
  assert and keep §4.5 test 5.

---

## 15. Owed to the spec

Ship these names now, doc-commented as unattested; write the amendment
**after** `constants.rs` exists, from the real names (Q-M3). Each is a
value `SPEC.md` fixes and a name it never gives, so a reader grepping the
spec for the identifier finds nothing.

| Implementation name | Value | Spec home | What the spec says instead |
|---|---|---|---|
| `STATIC_PUBLIC_LEN` | 65 | §2.4 | "the 65-byte uncompressed SEC1 storage form"; §2.3 writes it as `PK` |
| `PKT_RESERVED_UNUSED` | 0x04 | §3.1 | "reserved packet types … 0x04 (unused)" |
| `PKT_RESERVED_COOKIE` | 0x05 | §3.1 | "0x05 (cookie/mac2)" |
| `FRAME_STOP_SENDING_RESERVED` | 0x05 | §8.3 | "(reserved: STOP_SENDING)" |
| `STREAM_FLAG_MASK` | 0x07 | §8.4 | the three flags are named; their union is not |
| `CREDIT_REGRANT_DIVISOR` | 2 | §10.3 | "credit re-grant threshold … ½ window consumed" |
| `ACK_ELICITING_PER_ACK` | 2 | §12.4 | "every 2nd ack-eliciting" |
| `K_TIME_THRESHOLD_NUM` / `_DEN` | 9 / 8 | §13.2 | "time threshold … 9⁄8" |
| `PTO_BACKOFF_CAP` | 64 | §13.3 | written as "2⁶" |
| `CLOSE_REPLY_MIN_INTERVAL` | 1 s | §15.1 | "close-reply rate … ≤ 1 per s" |
| `SHELL_LATENESS_BOUND` | 250 ms | §16.5 | the spec calls it `L` |
| `PERSISTENT_KEEPALIVE_DEFAULT` / `_MIN` | 10 s / 1 s | §7.5 | one bare `PERSISTENT_KEEPALIVE`, with its range in prose |
| `APPLICATION_ERROR_BASE` | 0x10 | §15.3 | "≥ 0x10 application" |

Two shapes recur and are worth naming in the amendment rather than
listing thirteen exceptions:

1. **A ratio or rate stated in prose** (`½ window`, `every 2nd`, `9⁄8`,
   `≤ 1 per s`, `2⁶`) needs an identifier and a stated *unit* —
   "multiplier or exponent?" was a real ambiguity here (Q-M2).
2. **A range stated in prose against one named constant**
   (`PERSISTENT_KEEPALIVE`, admissible `[1 s, DEAD_TIMEOUT)`) becomes two
   or three identifiers in code. Naming the default and the floor —
   leaving the ceiling as a comparison against `DEAD_TIMEOUT` — is the
   shape that cannot drift.

Nothing here changes a value, a byte, or a behaviour. It is an
attestation debt, and slice 0 is where it becomes concrete enough to pay.

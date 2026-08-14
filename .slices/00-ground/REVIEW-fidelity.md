# Slice 0 — Lens A (fidelity): does the code say what the spec says?

Reviewed at commit `73757c3` ("Slice 0 (Ground)"). Scope: `src/constants.rs`,
`src/error.rs`, `src/shell/wire.rs`, `src/varint.rs`, `src/testutil/mod.rs`
against `SPEC.md`'s Named-constants table (+ ruling 63's subsection), §18.1,
§16.3, §8.1 and §16.10/ruling 60.

Test *adequacy* is Lens B's; this lens asks only whether the code states what
the spec states.

**Working-tree note (not a finding against the commit).** During this review
`src/testutil/mod.rs:473` was modified in the working tree by the parallel
Lens B agent, from `endpoints.get_mut(&dst)` to `get_mut(&src)` — evidently a
deliberate mutation-test injection. It routes every datagram into the
*sender's* inbox. I verified against `git show 73757c3:src/testutil/mod.rs`
that the committed line is `&dst` and correct. Flagged only so the mutation is
not left in the tree if that agent dies mid-run.

---

## 1. `src/constants.rs` vs the Named-constants table

**Verdict: every constant MATCHES. Zero DIFFERS, zero ABSENT.** Two EXTRA
identifiers (below, NIT-1). The judgement calls the brief singled out are all
resolved the way the spec rules.

### The prose-stated values (the ones a wrong reading would hide)

| Spec (prose) | Code | Verdict |
|---|---|---|
| `PTO_BACKOFF_CAP` "2⁶" (§13.3 = multiplier; ruling 63 fixes **64**) | `PTO_BACKOFF_CAP: u32 = 64`, doc says "cap on the PTO backoff **multiplier**" | MATCHES — the ruled reading, and the doc says *multiplier* so a later reader cannot re-guess |
| time threshold `9⁄8` (§13.2) | `K_TIME_THRESHOLD_NUM: u32 = 9` / `_DEN: u32 = 8` | MATCHES. `u32` is the *right* type, not an incidental one: `Duration: Mul<u32> + Div<u32>`, so `rtt * 9 / 8` is exact integer arithmetic with no float |
| "½ window consumed" (§10.3) | `CREDIT_REGRANT_DIVISOR: u64 = 2`, doc "the divisor, so `2` means half the window" | MATCHES — the unit is stated, which is exactly what ruling 63 asks for |
| "every 2nd ack-eliciting" (§12.4) | `ACK_ELICITING_PER_ACK: u64 = 2` | MATCHES |
| close-reply "≤ 1 per s" (§15.1) | `CLOSE_REPLY_MIN_INTERVAL = 1 s` | MATCHES (rate → minimum interval, the only sane reading) |
| `PERSISTENT_KEEPALIVE` range **[1 s, `DEAD_TIMEOUT`)** (§7.5) | `PERSISTENT_KEEPALIVE_DEFAULT = 10 s`, `PERSISTENT_KEEPALIVE_MIN = 1 s`, **no ceiling constant** | MATCHES, and follows ruling 63's own instruction verbatim: "Name the default and the floor, and leave the ceiling as a comparison against `DEAD_TIMEOUT`." The doc comment on `PERSISTENT_KEEPALIVE_MIN` says the ceiling is `DEAD_TIMEOUT`, exclusive, compared directly. `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}` is the matching pair |
| `L` = 250 ms (§16.5) | `SHELL_LATENESS_BOUND = 250 ms` | MATCHES |
| "≥ 0x10 application" (§15.3) | `APPLICATION_ERROR_BASE: u64 = 0x10` | MATCHES |
| `STATIC_PUBLIC_LEN` "the 65-byte uncompressed SEC1 form" (§2.4) | `65`, *and* asserted against `P256::PUBLIC_KEY_SIZE` | MATCHES, best-pinned constant in the file |

### The literal-valued rest

Checked one by one against SPEC.md:5601–5643 and §3.1/§3.2–3.5/§8.3/§15.3.
All match: `VERSION` 0x01 · `PROLOGUE` `b"slither\x01"` · packet types
0x01/0x02/0x03 + reserved 0x04/0x05 · header lens 6/10/14 · `MAC1_LABEL` /
`MAC1_LEN` 16 · `TIMESTAMP_LEN` / `MSG1_PAYLOAD_LEN` 12/12 · `AEAD_TAG_LEN` 16 ·
`IK_MSG1_LEN` / `IK_MSG2_LEN` 174/81 · `INIT_PACKET_LEN` / `RESP_PACKET_LEN`
196/107 · `MAX_DATAGRAM` / `MAX_PLAINTEXT` 1200/1170 · `REKEY_EPOCH_MSGS` 65 536 ·
`MAX_EPOCH_JUMP` 2 · `REPLAY_WINDOW` 2048 bits · all 14 frame types incl.
`0x05` reserved and the `0x08–0x0f` STREAM range · `STREAM_OFF/LEN/FIN`
0x04/0x02/0x01 · `INITIAL_MAX_DATA` 1 048 576 · `INITIAL_MAX_STREAM_DATA`
262 144 · streams 32/128 · `STREAMS_CREDIT_BATCH` 8 · `MESSAGE_RECV_MAX` =
`INITIAL_MAX_STREAM_DATA` · `MAX_DATAGRAM_PAYLOAD` 1169 · datagram queues 64/64 ·
`REASSEMBLY_CHUNKS_MAX` 1024 · `CLOSE_REASON_MAX` 256 · `CLOSE_LINGER` 5 s ·
`MAX_ACK_RANGES` 64 · `MAX_ACK_DELAY` 25 ms · `K_PACKET_THRESHOLD` 3 ·
`K_GRANULARITY` 1 ms · `K_INITIAL_RTT` 333 ms · `INITIAL_WINDOW` / `MINIMUM_WINDOW`
12 000/2 400 · `LOSS_REDUCTION_FACTOR` 0.5 · `PERSISTENT_CONGESTION_THRESHOLD` 3 ·
`RETRANSMIT_BASE` / `RETRANSMIT_JITTER_MAX` 5 s/333 ms · `HANDSHAKE_GIVEUP` 90 s ·
`KEEPALIVE_TIMEOUT` / `DEAD_TIMEOUT` 10 s/25 s · `AMPLIFICATION_FACTOR` 3 ·
`INTRO_QUEUE_CAP` / `INTRO_MAX_PER_SOURCE` / `INTRO_TTL` 1024/4/15 s ·
`TS_GUARD_ORPHAN_CAP` 1024 · the seven §15.3 codes `NO_ERROR`…`MESSAGE_OVERFLOW`
at 0x00–0x06 under exactly §15.3's names.

Types are sane throughout and follow the module's own stated rule (lengths
`usize`, varint-encoded fields `u64`, fixed-width header fields `u8`,
`Duration` for timers). `LOSS_REDUCTION_FACTOR: f64` is the one type that
could invite a bug, and it carries an explicit "halve with `cwnd / 2`, never
multiply by this" warning — the right call.

### The one table row with no constant

- **`| session index | nonzero u32, random, re-drawn across both tables |`
  (§17.3)** — ABSENT from `constants.rs`, correctly: it is a *property*, not
  a value. Nothing to name. No action.

### NIT-1 — two identifiers the spec does not name
- **Severity**: NIT
- **Location**: `src/constants.rs:184`, `src/constants.rs:187`
- **Spec**: §8.3's frame table writes the STREAM range as `0x08`–`0x0f`;
  ruling 63's subsection names `STREAM_FLAG_MASK` but not the range endpoints.
- **Code**: `FRAME_STREAM_BASE = 0x08`, `FRAME_STREAM_MAX = 0x0f`.
- **Why it matters**: the values are spec-fixed and the identifiers are
  obviously right, so this is not a wire risk. But the module's own fence says
  "if the spec did not name it, it does not live here", and these two are
  precisely ruling 63's shape #2 (a range stated in prose). Either add them to
  the ruling-63 table on the next amendment, or soften the module fence. A
  maintainer decision, not a code change.

### NIT-2 — a wrong section cross-reference
- **Severity**: NIT
- **Location**: `src/constants.rs:316`
- **Spec**: `PTO_BACKOFF_CAP` lives in §13.3 (SPEC.md:3403, 3427). §13.5 is
  "Frames, never packets" and never mentions the backoff cap.
- **Code**: `/// The cap on the PTO backoff **multiplier**, 2⁶. §13.3, §13.5.`
- **Why it matters**: rustdoc is the map from code to spec section; a wrong
  pointer costs the next reader a grep. Drop `§13.5`.

### NIT-3 — `CLAUDE.md` and `Cargo.toml` say "59 rulings"; SPEC.md has 63
- **Severity**: NIT
- **Location**: `CLAUDE.md:6`, `Cargo.toml:23`
- **Spec**: SPEC.md carries rulings **60** (§16.10), **61** (§18.1), **62**
  (§16.3 area), **63** (Named constants), all `[RATIFIED 2026/08/14]`.
- **Code/docs**: both say "59 rulings"/"59 rulings, their reviews".
- **Why it matters**: an agent that trusts `CLAUDE.md`'s count will not go
  looking for rulings 60–63, which are exactly the ones slice 0 implements.

---

## 2. The 39 compile-time assertions

**Count: 39, confirmed.** Of those:

- **2 are external pins** — the strongest kind, because they turn a
  *dependency* change into a build failure:
  `STATIC_PUBLIC_LEN == P256::PUBLIC_KEY_SIZE` (`constants.rs:484`) and
  `MAX_EPOCH_JUMP == hiss::noise::datagram::MAX_EPOCH_JUMP` (`:524`).
- **34 are genuine derivations or cross-constant invariants** — they compute
  one constant from others, or assert an ordering that could be violated
  independently. Spot-checked arithmetic, all correct: `IK_MSG1_LEN` =
  65 + (65+16) + (12+16) = 174; `IK_MSG2_LEN` = 65+16 = 81; `INIT_PACKET_LEN` =
  6+174+16 = 196; `RESP_PACKET_LEN` = 10+81+16 = 107; `MAX_PLAINTEXT` =
  1200−14−16 = 1170; CLOSE worst case 1+8+8+256 = 273 ≤ 1170; datagram queue
  64×1169 = 74 816 < 81 920 ("≈ 73 KiB", §11.3's own parenthetical);
  `INITIAL_WINDOW` = 10×1200 = 12 000 and `MINIMUM_WINDOW` = 2×1200 = 2 400,
  which is RFC 9002's `kInitialWindow`/`kMinimumWindow` evaluated at this MTU;
  the seven timer orderings; the four frame-grammar bit identities; the three
  `VarInt::MAX_VALUE` bounds.
- **1 is a notation restatement that still earns its place**:
  `PTO_BACKOFF_CAP == 1 << 6` (`:527`). It restates the literal, but it encodes
  the *ruled reading* — someone "correcting" 64 to 6 turns the build red. Keep.
- **2 are tautologies** (see NIT-4).

So **36 of 39 are load-bearing**, 1 is defensible, 2 prove nothing.

### NIT-4 — two assertions restate a type
- **Severity**: NIT
- **Location**: `src/constants.rs:476`, `src/constants.rs:478`
- **Code**: `assert!(PROLOGUE.len() == 8)` where `PROLOGUE: &[u8; 8]`, and
  `assert!(MAC1_LABEL.len() == 12)` where `MAC1_LABEL: &[u8; 12]`.
- **Why it matters**: the declared array type already *is* the pin — changing
  the literal changes the type and fails at the declaration, before the
  assertion runs. These two prove nothing the type system did not already
  prove. Harmless; listed only because the brief asked for the count.
  (`PROLOGUE[7] == VERSION`, by contrast, is genuinely load-bearing: it is the
  one that stops a version bump from leaving the prologue behind.)

### MINOR-1 — four derivations the spec states that are not asserted
- **Severity**: MINOR (all four are *additions*; none indicates a wrong value)
- **Location**: `src/constants.rs:466–541` (the assertion block)
- **What the spec says / what is missing**:
  1. §7.7 writes `REKEY_EPOCH_MSGS` as "**65 536 (2¹⁶)**" — the exact
     prose-notation shape that made `PTO_BACKOFF_CAP` a hazard, and the file
     asserts `PTO_BACKOFF_CAP == 1 << 6` for that reason. The symmetric
     `assert!(REKEY_EPOCH_MSGS == 1 << 16)` is absent.
  2. §15.1 fixes `CLOSE_LINGER` = 5 s and the close-reply rate at ≤ 1/s. That
     the linger must admit at least one reply
     (`CLOSE_REPLY_MIN_INTERVAL_MS < CLOSE_LINGER_MS`) is a real ordering in
     the same family as the seven that *are* asserted, and it is absent.
  3. §6.3's `INTRO_MAX_PER_SOURCE` (4) ≤ `INTRO_QUEUE_CAP` (1024) — a
     one-source cap above the global cap would be meaningless. Not asserted.
  4. *(implied, not stated — weakest of the four)* §7.3's budget must admit
     the handshake response:
     `AMPLIFICATION_FACTOR × INIT_PACKET_LEN ≥ RESP_PACKET_LEN`
     (3×196 = 588 ≥ 107). §7.3 states the 3× rule and asserts "no deadlock"
     but never writes this arithmetic down; it is the invariant that makes
     the two compatible, and it spans three constants none of which is
     asserted against the others.
- **Why it matters**: each is cheap, each is the kind of thing a future
  constant change would silently break, and the file's stated policy is that
  every spec-stated derivation is re-derived here. Not blocking.

Not recommended: assertions tying `INIT_HEADER_LEN`/`RESP_HEADER_LEN`/
`DATA_HEADER_LEN` to their §3.2–3.4 field layouts (1+1+4, 1+1+4+4, 1+1+4+8).
The spec names no field-width constants, so such an assertion would have to
invent them, and the packet-length assertions already pin the sums.

---

## 3. `src/error.rs` vs §18.1

**Verdict: exact. Nine types, every variant, every payload shape — no
addition, no omission, no rename.** Checked variant-by-variant against
SPEC.md:4897–4987.

| §18.1 | Code | Verdict |
|---|---|---|
| `ConnectError::{AlreadyConnected, TimedOut}` | same, exactly two | MATCHES |
| `IntroError::{Expired, Internal, Malformed, EndpointDropped}` | same | MATCHES |
| `AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}` | same | MATCHES |
| `AcceptError::{Stale, EndpointDropped}` | same; no `Expired`, no `AlreadyConnected` | MATCHES (both exclusions are explicit in §18.1) |
| `ConnectionLost::{TimedOut, NonceExhausted, LocallyClosed, PeerClosed{code,reason}, ProtocolViolation{code}, Replaced, EndpointDropped}` | same, struct variants with the same field names | MATCHES |
| `WriteError::{Reset(u64), ConnectionLost(ConnectionLost), Finished}` | same; no `Stopped` | MATCHES |
| `ReadError::{Reset(u64), ConnectionLost(_)}` | same | MATCHES |
| `MessageError::{TooLarge, ConnectionLost(_)}` | same | MATCHES |
| `DatagramError::{TooLarge, ConnectionLost(_)}` | same | MATCHES |

Nothing named `Superseded` and nothing named `AcceptError::AlreadyConnected`
appears anywhere in the crate — §18.1's two explicit deletions hold.
`ConnectError` has no `EndpointDropped`, which §18.1 does not list and ruling
62 (SPEC.md:4096, "a `Connecting` *is* a handle") explains; the doc comment
cites it.

**Ruling 61 (`#[non_exhaustive]`) — verified mechanically.** `grep -c` over
`src/` finds the attribute exactly once, on `WriteError` (`error.rs:159`).
The other nine types are exhaustive.

**`ConfigError` (outside §18.1, ruling 44)** — checked at its real home,
§16.2 (SPEC.md:3870–3888). The spec fixes
`Result<(), ConfigError>` with `ConfigError::{KeepaliveTooShort,
KeepaliveTooLong}`, "each naming the bound it violated". Code has exactly those
two, exhaustive, with `Display` strings naming the 1 s floor and the
`DEAD_TIMEOUT` ceiling. MATCHES.

Payload-shape notes, all fine: `PeerClosed.reason` is `Vec<u8>` (the spec
bounds it at `CLOSE_REASON_MAX` bytes and never calls it UTF-8, so bytes is
right); error codes are `u64` to match §15.3's varint registry; `Clone` on
`ConnectionLost` is required by the §16.2 fan-out and the doc says why.

**No findings at any severity in this file.**

---

## 4. `src/shell/wire.rs` vs §16.3

**Verdict: the trait is byte-for-byte the spec's code block, and the no-`Send`
property is real, not merely intended.**

Signature (SPEC.md:4183–4188 vs `wire.rs:61–66`): identical, down to the doc
comments — `pub trait Wire`, `async fn send_to(&self, buf: &[u8], addr:
SocketAddr) -> std::io::Result<usize>`, `async fn recv_from(&self, buf: &mut
[u8]) -> std::io::Result<(usize, SocketAddr)>`. The impl for
`tokio::net::UdpSocket` the spec requires is present (`wire.rs:68`) and is not
feature-gated — `tokio/net` is a hard dependency, so "the default case costs
the application nothing" holds.

The four normative properties:

1. **Application supplies it via `Endpoint::builder()`** — the builder does not
   exist in slice 0. Correctly documented as intent rather than faked. Deferred,
   not violated.
2. **No `Send` bound.** Verified rather than assumed:
   - the trait has no supertrait and no `where Self: Send`;
   - both methods are `async fn` in trait, which desugars to RPITIT with *no*
     bounds — a `Send` bound cannot be attached by a caller either, which is
     exactly what `#[allow(async_fn_in_trait)]` at `wire.rs:60` is suppressing.
     The comment above it says so;
   - the impl for `UdpSocket` is a concrete impl, so `UdpSocket: Send + Sync`
     leaks no bound onto the trait;
   - there is no blanket `impl<T: ...> Wire for T`, so no bound can arrive that
     way;
   - the fence at `wire.rs:122` builds a genuinely `!Send` `Wire` (it holds an
     `Rc`, so the *futures* capturing `&self` are `!Send` too), drives it
     through a `Send`-free generic `drive<W: Wire>`, and awaits it on a
     current-thread runtime.

   I tried to construct a case where a `!Send` `Wire` fails to compile against
   this definition and could not: the only failure mode is `Box<dyn Wire>`,
   which fails for *every* `Wire` (dyn-incompatibility, not `Send`), and which
   the module documents at `wire.rs:41–56` together with the private-`DynWire`
   escape hatch. Not a finding.
3. **`&self` on both methods** — yes, both.
4. **`FlakyWire` is a `Wire`** — yes, `impl Wire for FlakyWire`
   (`testutil/mod.rs:549`), and the round-trip test at `wire.rs:88` proves the
   `UdpSocket` impl is not a fiction.

Ruling 49's "a failing `send_to` is traced, not acted on" is documented on the
trait as the **driver's** obligation, not the implementation's — matching
SPEC.md:4207–4213 and §18.2's `slither::io` row. There is no driver yet to
carry it out, so nothing to check beyond the doc.

**No findings at any severity in this file.**

---

## 5. `src/varint.rs` vs §8.1

**Verdict: conformant, including the asymmetry that is the section's whole
point.**

- **Length prefixes** (§8.1's table): `encoded_len` returns 1/2/4/8 at the
  exact boundaries 2⁶/2¹⁴/2³⁰; `decode` reads `n = 1 << (first >> 6)`. Both
  match RFC 9000 §16.
- **Value space**: `MAX_VALUE = (1 << 62) - 1` = 4 611 686 018 427 387 903,
  the spec's 2⁶² − 1. `VarInt::new` rejects above it; `decode` cannot produce
  above it (6 bits + 7×8 = 62). The newtype makes §8.1's cap unrepresentable
  rather than checkable, which is what makes the ACK-`largest`/offset/final-size
  caps of §12.1 and §9.5 automatic.
- **Sender emits minimal** — `encode`/`encode_to` have no other mode; the
  minimal length is `encoded_len` and the value is or-ed with the prefix
  (`0x4000`, `0x8000_0000`, `0xc000_…`), big-endian throughout. Correct.
- **Receiver accepts any length** — `decode(&[0x40, 0x25])` returns
  `(37, 2)`. §8.1: "a receiver accepts any length (a non-minimal encoding is
  valid, as in QUIC)". The code does *not* reject non-minimal forms, and the
  module doc calls out that rejecting them "would be a spec violation, not a
  hardening". This is the single most likely place to be wrong-in-the-safe-
  looking-direction, and it is right.
- **Boundaries**: truncation returns `None` at every prefix class;
  `encode_to` on a short buffer returns `None` **without writing** — needed by
  §8.6's pack-until-it-does-not-fit loop.

`pub(crate)` visibility is deliberate and correct: a public `encode` would need
a public "exceeds 2⁶² − 1" error type, which §18.1's closed taxonomy forbids.

**No findings at any severity in this file.**

---

## 6. `src/testutil/mod.rs` vs §16.10 / ruling 60

**Verdict: the three contract names are present and correct, determinism is
real, send-failure injection exists.**

**The three contract names** (SPEC.md:4551–4572) — `testutil::Network`,
`testutil::FlakyWire`, `testutil::FlakyPolicy` — all `pub`, all at exactly
those paths, and the module is exported as `pub mod testutil` under
`cfg(any(test, feature = "test-util"))` with `docs.rs` building all features.
Their roles match the ruling's descriptions: `Network` owns the
address→endpoint map and moves datagrams; `FlakyWire` is a `Wire` attached at
one address; `FlakyPolicy` is the per-datagram impairment. `lib.rs:78–83`
records that renaming one is a protocol revision.

**Determinism under a caller-supplied seed — MUST, and it holds.** I looked
specifically for the four ways a replay diverges:

- *Wall clock / OS entropy*: none. No `SystemTime` anywhere; `Instant` is
  `tokio::time::Instant` (`mod.rs:64`), i.e. the paused virtual clock. The only
  RNG is `ChaCha20Rng::seed_from_u64` (`mod.rs:405`). `Network::new()` is
  `seeded(0)` — there is deliberately **no** OS-entropy constructor, and the
  doc says adding one would violate the ruling.
- *Hash iteration order*: none reachable. Every collection is ordered —
  `BTreeMap<SocketAddr, EndpointState>`, `BTreeSet` for `partitioned`,
  `blocked` and `drop_at` (`mod.rs:319–321`, `:188`). The `Inner` field comment
  states the reason explicitly.
- *Address-dependent ordering*: none. Per-wire RNG seeds derive from
  `seed ^ ordinal.wrapping_mul(SEED_STRIDE)` where `ordinal` is an **explicit
  registration counter** (`mod.rs:329, 393–400`), not a map position — so
  adding a third endpoint cannot reshuffle the first two's draws, and no
  decision depends on a `SocketAddr`'s ordering.
- *Heap tie-breaking*: `Queued` orders by `(deliver_at, seq)` with a monotonic
  `seq` (`mod.rs:295–301`), so equal-deadline datagrams are FIFO rather than
  `BinaryHeap`-arbitrary. This is the subtle one and it is handled.

The draw order is fixed and documented as contract (`mod.rs:567–577`), and two
details make it robust rather than merely stated: the duplicate draw is taken
even when the loss draw already decided the outcome, and the delay draw is
taken even when `jitter` is zero (`mod.rs:536–546`) — so a policy change does
not shift the stream position of later sends. Index-based `drop_at`/`drop_first`
consume no draws at all, which is why the docs steer specific-outcome
assertions to them.

**Send-failure injection** (required, not optional): `FlakyPolicy.send_failure`
+ `SendFailure { kind, raw_os, until }` + `failing_sends_until()`, defaulting to
a real `ENETUNREACH` errno chosen per-target (101 Linux/Android, 51 BSD/Apple)
so a test can assert `raw_os_error()`. `send_to` returns the error *before*
tapping or queueing, and a blackholed path returns `Ok` instead — the
distinction ruling 49's fixture needs (`mod.rs:552–598`).

**Reordering** is provided through jitter (two datagrams whose delay draws
cross swap) rather than a dedicated knob, and the field doc says so. §16.10
requires reordering to exist, not to have its own knob; it exists and is
seed-reproducible. Conformant.

`Network` and `FlakyWire` are `Rc`-based and therefore `!Send`, which is the
right shape for a fixture feeding a `!Send` actor — a `Send` fixture would let
a `Send` bound reach the driver unnoticed.

**No findings at any severity in this file** (the `&src`/`&dst` line in the
working tree is Lens B's injected mutation, not committed code — see the note
at the top).

---

## Summary

| Severity | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 0 |
| MINOR | 1 (MINOR-1: four un-asserted derivations) |
| NIT | 4 |

**Verdict: CONFORMANT.** Every wire value, every error variant, every trait
signature and every normative property in scope says what `SPEC.md` says. The
single MINOR is a request for four *additional* assertions, not a wrong value;
the NITs are two redundant assertions, one wrong `§` pointer, one stale ruling
count in `CLAUDE.md`/`Cargo.toml`.

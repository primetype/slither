# Slice 2a — sans-io endpoint core: implementation notes

Incremental notes (CLAUDE.md working rule 2). Written as I go, never batched.

## 0. Status log

- [x] Notes file created (first action).
- [ ] Rulings 69-71 read.
- [ ] PLAN.md read in full.
- [ ] Existing source surveyed.
- [ ] Implementation.
- [ ] Gates.

## 1. Rulings 69-71 (binding, landed after PLAN.md was written)

- **69** — intro queue evict-oldest and per-source eviction order by **last
  refresh**, never original park time. One `age_key()` helper returning the
  last-refresh instant serves both expiry and eviction. A same-source
  retransmit that replaces an entry's bytes makes it young again.
- **70** — `TS_GUARD_ORPHAN_TTL` exists in `src/constants.rs`, an alias of
  `INTRO_TTL`. Use it for §17.1 orphan aging. Do not write `INTRO_TTL` there.
- **71** — §16.4 gains `intro_source(IntroId) -> Option<SocketAddr>` and
  `intro_sender_index(IntroId) -> Option<u32>`. Must read **live** from the
  parked entry (§5.5 mints a fresh random index per retransmit).

Also standing: 64 (header LE), 65 (msg1 timestamp BE / length gate), 66
(mac1 BLAKE2b plain), 67, 68 (`TAG` fixed 16, `PK` only per-suite quantity).

## 2. PLAN.md digest (read in full, 1627 lines)

Module layout the plan mandates:

```
src/identity.rs        NEW pub    Identity seam + SoftwareIdentity + 3 aliases
src/config.rs          NEW pub    Config + WallClock (Rc<dyn>) + SystemClock
src/packet/handshake.rs NEW pub   Handshake: Channel supertrait, GATs over P
src/packet/suite.rs    EXT        channel! also stamps impl Handshake
src/core/mod.rs        NEW pub(crate)  Transmit/Disposition/EndpointOutput/
                                  ToEndpoint/Install/EstablishedSession/
                                  ConnectionId/Timestamp
src/core/endpoint/mod.rs        Endpoint<I>, drain, handle_datagram
src/core/endpoint/handshake.rs  initiator driving + responder msg2 write
src/core/endpoint/staged.rs     IntroId, ChainState, four verbs
src/core/endpoint/intro_queue.rs §6.3 entire
src/core/endpoint/guard.rs      §17.1
src/core/endpoint/tables.rs     §17.2-17.4
src/core/connection/mod.rs      MINIMAL Connection
src/core/tests.rs               STUB (test author writes it)
src/testutil/mod.rs    EXT      CountingIdentity
```

Key shapes:

- `Identity` is a **factory**: `open() -> Result<(Provider, PrivateKey), Error>`
  per handshake, because hiss consumes both by value and `PrivateKey: !Clone`.
- `Handshake: Channel` supertrait with GATs `type Initiator<P: DhProvider<..>>`
  etc.; `Transport` has no GAT (generic over pattern, not provider).
- `ChainState` boxed: `Parked{bytes}` 0DH / `Claimed{mid,claimed}` 1DH /
  `Proven{st,peer,ts}` 2DH / `Poisoned` (transient, mem::replace only).
- Lazy responder build at `read_identity()`, NOT at park (§17.5 provider
  handles).
- Arrival order (derived, F-3): dedup -> per-source cap -> global cap -> park.
- `accept()` implements §5.4's NONE row only; LIVE/PENDING -> `AcceptError::Stale`
  (documented slice boundary, §1.4).
- Guard write at `authenticate()` is **provisional**: `GuardUndo{key, previous}`
  on the chain; reject/drop/expiry/eviction reverts.
- `pins: u32` count not bool; pin never creates an entry.
- Initiator path never touches the guard.
- `last_init_timestamp` endpoint-global; `succ()` = +1 nanosecond with carry.
- Index mint: nonzero u32, re-draw while present in *either* table.
- `replacement_basis`: `Some(t)` on accept, `None` on connect. Written, never read.
- Hint set = projection over pending table (dialled addresses), not a structure.
- Sub-seed drawn at BOTH connect() and accept(), even though unused.
- Endpoint deadline = min(pending retransmit/giveup, intro expiry, guard orphan
  aging). Give-up beats a same-instant retransmit (normative).
- Cancellation reuses `ToEndpoint::Retired { our_index }` (F-5).
- msg2 source address deliberately ignored; index match only.
- Completion order: exact len -> pending index -> mac1(ours) -> attempt_spent? ->
  set spent BEFORE crypto -> read_message_2.
- `mod core` hazard: write `::core::` absolutely, `crate::core::` for the module.

## 3. Findings / conflicts

(appended as found)

## 4. Decisions

(appended as made)

## 5. hiss 0.3.2 / hiss-macros 0.3.1 API, verified from source

- `IK::initiator<CP: DhProvider<Curve>>(provider: CP, prologue: &[u8], remote_static: PubKey)
   -> IKInitiatorMsg1<CP>` (infallible — no local `s` in the pre-message for the
   initiator; `set_rs` cannot fail). codegen.rs:527-606.
- `IKInitiatorMsg1<CP>::write_message_1(self, static_key: PrivKey, payload: &[u8;12])
   -> Result<([u8; IK::MSG1_SIZE], IKInitiatorMsg2<CP>), HandshakeError>` (codegen.rs:947).
- `IKInitiatorMsg2<CP>::read_message_2(self, message: &[u8; IK::MSG2_SIZE])
   -> Result<Transport<IK>, HandshakeError>`.
- `IK::responder<CP>(provider, prologue, static_key: PrivKey)
   -> Result<IKResponderMsg1<CP>, HandshakeError>` (fallible: `set_s`).
- `IKResponderMsg1<CP>::read_message_1_intro(self, message: &[u8; IK::MSG1_SIZE])
   -> Result<(PubKey, IKResponderMsg1Intro<CP>), HandshakeError>`  [1 DH: es]
- `IKResponderMsg1Intro<CP>::complete(self)
   -> Result<([u8;12], IKResponderMsg2<CP>), HandshakeError>`      [1 DH: ss]
- `IKResponderMsg2<CP>::write_message_2(self)
   -> Result<([u8; IK::MSG2_SIZE], Transport<IK>), HandshakeError>` [2 DH: ee, se]
- `Transport<IK>::into_datagram_with_epoch(self, epoch_size: NonZeroU64)
   -> (DatagramSend<IK>, DatagramRecv<IK>)` (datagram.rs:133).
- `CryptoKeyProvider::PrivateKey: Send` (hiss requires it); `Error: std::error::Error
   + Send + Sync + 'static`. `DhProvider` itself carries NO `Send`.
- **No generic private-key import seam exists in hiss.** `P256r1PrivateKey::from_bytes`
   is P-256 specific. Consequence: `SoftwareIdentity` is bound to `Curve = P256`
   (suite/cipher/hash still generic). Recorded as a deviation from the plan's
   unconstrained `SoftwareIdentity<S, R>`.

## 6. Files written

- `src/identity.rs` — `Identity` trait (factory), `CurveOf`/`PublicKeyOf`/
  `PrivateKeyOf` aliases, `SoftwareIdentity<S, R>`, `SoftwareIdentityError`.
- `src/config.rs` — `Config`, `WallClock`, `SystemClock`.
- `src/packet/handshake.rs` — `Handshake: Channel` (GATs), `PublicKeyFor<C>`.
- `src/packet/suite.rs` — EXT: `channel!` stamps `impl Handshake`.
- `src/packet/mod.rs` — EXT: `pub mod handshake`, re-export, module doc.
- `src/core/mod.rs` — `ConnectionId`, `Timestamp`, `Transmit`,
  `EstablishedSession<C>`, `Install<C>`, `ToEndpoint`, `Disposition`,
  `EndpointOutput<C>`.
- `src/core/connection/mod.rs` — minimal `Connection<C>`, `ConnOutput`,
  `ConnEvent`.
- `src/core/endpoint/mod.rs` — `Endpoint<I>`, drain, deadline, connect,
  handle_datagram, handle_timeout, handle_connection_event.
- `src/core/endpoint/staged.rs` — `IntroId`, `ChainState`, the four verbs +
  ruling 71's two accessors.
- `src/core/endpoint/intro_queue.rs` — §6.3 entire (`age_key` = last refresh).
- `src/core/endpoint/guard.rs` — §17.1 (provisional write + undo, pins, LRU,
  `TS_GUARD_ORPHAN_TTL` aging).
- `src/core/endpoint/tables.rs` — §17.2-17.4.
- `src/core/endpoint/handshake.rs` — framing helpers (mac1 keyed on the
  RECIPIENT's static both ways).
- `src/core/tests.rs` — STUB, doc comment only.
- `src/testutil/mod.rs` — EXT: `CountingIdentity` (!Send, shared `DhCounter`).
- `src/lib.rs` — EXT: new modules, re-exports.

## 7. Throwaway verification (deleted before the slice ended)

`src/core/verify_tmp.rs`, 7 `#[test]`s, all green, then removed:
full handshake + 0/1/2/4 DH ladder; reject at 0 DH emits nothing; expiry is
silent and idempotent and announces `t0 + INTRO_TTL`; retransmit at
5 s + jitter costing 2 more DH, then give-up at 90 s emitting
`HandshakeFailed(TimedOut)` and no Transmit; cancel via `Retired` frees the
static so a redial succeeds; dedup keeps the `IntroId`, emits no second
`IntroReady`, and ruling 69's refresh moves the deadline while ruling 71's
`intro_sender_index` reads the NEW index; three initiations emit strictly
increasing timestamps.

## 8. Bug the throwaway verification caught

**§17.1 mitigation (i) was silently broken by the release ORDER.** A chain
that authenticated a static nobody had recorded before does two things in
`authenticate()`: `guard.record()` **creates** the entry, and the mid-state
pin is then taken on it (the pin at `read_identity()` had found nothing to
pin, since a pin never creates). On `reject()`, `release_chain_guard_state`
reverted first: `revert` saw `previous == None`, checked `pins > 0`, found
the chain's **own** pin still held, declined to remove the entry it had just
created — and left exactly the orphan mitigation (i) exists to prevent.

Silent, permanent, and it only bites the authenticate-then-reject path,
which is the path the mitigation is written for. Fix: unpin before
reverting, documented at the call site as load-bearing order.

## 9. Findings, conflicts and deviations (for the maintainer)

### D-1 (DECLINED — plan §3.1's literal wording would repeat ephemerals)

The plan says `SoftwareIdentity` "clones a seeded RNG into a fresh
`EphemeralOnly`". **Two clones of a seeded RNG produce the same stream**, so
every handshake would reuse one ephemeral — directly contrary to §5.5's
"every retransmit is a completely fresh initiation — new ephemeral". `open()`
instead draws a fresh 32-byte **sub-seed** from a `RefCell`-held parent RNG
and seeds a new `ChaCha20Rng` with it: distinct streams per handshake, and a
seeded parent still makes the whole endpoint replayable.

### C-1 (PLAN vs SPEC — the §17.1 pin moment; spec followed, reported)

Plan §7.2 takes the mid-state pin at `authenticate()`. §17.1 says an entry is
pinned "while a live `Connection`, an in-flight outbound pending, **or a
staged mid-state** exists for its static", and addresses the merely-claimed
case head on: "For a staged mid-state (**whose static is merely claimed until
`authenticate()`**) the pin never *creates* an entry." That places the pin at
`read_identity()`. Implemented at `read_identity()`; `authenticate()` takes
it then only if the entry did not exist to pin earlier (its own `record()`
may have created it).

### C-2 (SPEC GAP — §18.1 has no variant for a LOCAL provider failure)

`Identity::open()` is fallible by design (an enclave can be locked). Neither
`ConnectError` (`AlreadyConnected` | `TimedOut`) nor `IntroError` (`Expired`
| `Internal` | `Malformed` | `EndpointDropped`) can express it, and the
taxonomy is closed by ratification. Resolved without inventing a variant:

- **connect / retransmit** — the attempt is simply not built. The train
  continues, the next interval retries, and a provider that never recovers
  surfaces as the outcome the spec DOES define: `TimedOut` at
  `HANDSHAKE_GIVEUP`. Nothing is invented and nothing is lost.
- **read_identity** — returns `IntroError::Malformed` and leaves the chain
  **parked** (0 DH spent, retryable). `EndpointDropped` was rejected as
  strictly worse: it means "the driver stopped", and an application that
  believes that tears down its whole accept loop over a transient hardware
  hiccup. `Malformed` misattributes to the peer, which is harmless here —
  ruling 48 forbids any denylist, so nothing can act on the misattribution.
- A **hiss** failure at `read_msg1_intro` also reports `Malformed`, but
  DISCARDS the chain: 1 DH is spent and the verdict is definitive.

**This is a reportable gap, not a resolved one.** If the maintainer wants a
local fault distinguishable from a malformed msg1, that needs a ruling.

### C-3 (SPEC GAP — a core verb called out of stage)

§16.4's core verbs are keyed by `IntroId` and carry no typestate fence;
§6.2's fence exists only on the shell handles. §18.1 names no "wrong stage"
error. Resolved:

- `authenticate()` on a still-**parked** chain **advances it** through the
  missing `es` first. §6.1 prices `authenticate()` at **2 DH cumulative**, so
  driving the missing work costs exactly what the ratified table says and no
  error has to be invented. Calling it twice returns the cached
  `(peer, timestamp)` at 0 incremental DH.
- `accept()` on a non-**proven** chain returns `AcceptError::Stale` — the
  only non-`EndpointDropped` variant §18.1 offers.

### C-4 (SPEC's §16.4 Rust block is schematic, not literal)

`EndpointOutput`, `Install`, `EstablishedSession` and `core::Connection` are
written non-generic in §16.4 (as is `PublicKey`), but the seal/open halves
are `DatagramSend<IK>` / `DatagramRecv<IK>` — per-suite by construction. All
four are therefore generic over `C: Handshake` here. No behaviour changes;
recorded so a reviewer does not read it as drift.

### C-5 (hiss has no generic private-key import seam)

`SoftwareIdentity` is bound to `Curve = P256` (cipher and hash stay generic).
`P256r1PrivateKey::from_bytes` is the only import path, `PrivateKey` is
deliberately not `Clone`, and `open()` must hand out an owned key per
handshake — so re-import is forced and re-import is curve-specific. The
plan's `SoftwareIdentity<S, R>` was unconstrained. A non-P-256 backend
implements `Identity` directly, which is what the seam is for.

### D-2 (minor deviation) — `Identity::Suite: Handshake`, not `: Channel`

`Handshake` is a supertrait extension of `Channel`, so this is strictly
stronger and removes a `where I::Suite: Handshake` clause from every impl
block in the core. Plan §4.2's "the core writes `I::Suite: Handshake` and
nothing else" is satisfied either way.

### Confirmed as planned

- **F-5 / plan §8.4** — cancellation reuses `ToEndpoint::Retired`; no verb
  invented. Verified: a redial on the next line succeeds.
- **Plan §1.4** — `accept()` implements §5.4's NONE row; LIVE and PENDING
  return `Stale`. Documented at the call site as a slice boundary.
- **Plan §6.2** — arrival order dedup -> per-source cap -> global cap -> park,
  with each ordering's reason at the call site.
- **Plan §11(a).4** — `Timestamp::succ()` = +1 nanosecond with carry.
- **Rulings 69, 70, 71** — all three implemented; see §1 above.

## 10. Gates (all green on the handed-over tree)

| Gate | Command | Result |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | exit 0, clean |
| Format | `cargo fmt --all --check` | exit 0, no diff |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | exit 0, zero warnings |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` (+ `--all-features`) | exit 0 both |
| Tests | `cargo test` / `cargo test --all-features` | 66 + 103 + 11 + 4 + 4 doc, 0 failed, exit 0 both |
| Wire pins | golden-wire + size/constant tests under `cargo test` | all `packet::tests::golden_*` and `sizes_match_the_golden_vectors` pass, byte-identical |
| MSRV | `cargo +1.96 check --all-features --all-targets` | exit 0 |
| Supply chain | `cargo deny check` | advisories/bans/licenses/sources ok, exit 0 |

**Coverage caveat, stated plainly:** `src/core/tests.rs` is a doc-comment
stub by design (CLAUDE.md working rule 6 — the test author is writing it in
parallel). **None of slice 2a's core behaviour is covered by a committed
test.** The test counts above are slices 0-1's; the endpoint core adds zero.
The only evidence this slice's behaviour is correct is the throwaway
verification in §7-§8, which no longer exists in the tree.

## 11. Responses to the test author's OPEN-QUESTIONS.md

Read after implementing; three of four A-items were already resolved the
way the author's provisional recommends, and both B-items are now closed.

- **A1** (second `read_identity()` on a `Claimed` chain) — reading 1:
  **cached, 0 incremental DH**. §6.1's dagger note says the cumulative table
  is unchanged for a cached result, which reading 2 would break; reading 3
  would refuse a call §16.4 makes legal.
- **A2** (does a `Malformed` `read_identity()` destroy the entry?) — **it
  depends on which failure**, and the distinction is deliberate. A *hiss*
  failure (msg1 structurally unreadable, 1 DH spent, definitive) destroys
  the entry — the author's provisional, same reasoning. A *local* failure
  (`Identity::open()` or `Handshake::responder()`, 0 DH spent, possibly a
  transient enclave lock) leaves the chain **parked and unconsumed**, so a
  retry can still work and the shell's drop→`reject()` reclaims it.
- **A3** (does orphan aging run from last admission or from pin release?) —
  **DIVERGENCE, and it needs a ruling.** Implemented as
  `last_admitted + TS_GUARD_ORPHAN_TTL`; the author's provisional is from
  pin release. Their reading has the better textual case (§17.1 defines
  orphans as *"All other entries (orphans — dead connections)"*, so an entry
  cannot age *as an orphan* before it is one), but implementing it needs an
  `orphaned_at` field set when `pins` reaches zero, which means threading
  `now` into `unpin()` — a change adjacent to mitigation (iii) that I will
  not make without a ruling. The two collapse whenever an entry is never
  pinned after admission, which is every flow slice 2a can reach.
- **A4** (§16.5's equal-deadline list) — left unordered, as recommended.
  `handle_timeout` happens to run expiry, then orphan aging, then pendings,
  but nothing depends on that and no test should.
- **B1/B2** — **closed.** `Endpoint::replacement_basis(&[u8]) ->
  Option<Option<Timestamp>>` and `Endpoint::hints() -> Vec<SocketAddr>`
  (sorted, deterministic) are now `pub(crate)` accessors. Both are outside
  §16.4's list and documented as existing only so the two write-never-read
  fields are assertable before slice 7.

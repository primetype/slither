# Slice 2a — the sans-io endpoint core

*Status: COMPLETE — plan only, no implementation code. Sections 1–11 are
the plan; Appendix S is the spec/source evidence each section rests on,
recorded as it was read so the reasoning can be audited without
re-reading `SPEC.md`.*

*Read for this plan, in order and with `offset`/`limit`: §16.4
(4322–4499), §6.1 (1033–1112), §6.3 (1138–1277), §5.5–5.7 (874–1017),
§17.1–17.4 (4746–4945), §5.3–5.4 (798–872), plus §2.2–2.4 (470–536),
§6.2 (1113–1136 — 24 lines, to confirm the slice boundary), §16.5–16.6
(4500–4570), §17.5 (4947–4979), Appendix A (5141–5259), and the last 120
lines of `rulings.md`. `SPEC.md` was never read whole.*

**Two questions in §11(b) are conflicts between two ratified statements
(R-1, R-3). Working rule 3 applies: they are reported, not resolved.**

## 1. Scope, and what moved to slice 3

### 1.1 The narrowing is right, and here is the spec line

`PLAN.md` line 281 bundles the sans-io core and the shell handles into
one slice. That is a dependency inversion, and two ratified lines settle
it:

- **§6.2 (1113–1136)** — every staged verb on the shell handles is
  `async`, consumes `self`, and returns the next typestate object:
  `pub async fn read_identity(self) -> Result<Claimed, IntroError>`.
  The section says why in one sentence: *"Each stage's `async` is a
  driver round-trip; the DH costs land on the driver task (§16.3)."*
  There is no driver task before slice 3.
- **§16.4 (4345–4351)** — the *core's* staged verbs are synchronous,
  take `&mut self`, and are keyed by `IntroId`. They mention no future,
  no channel, and no task.

So the core is buildable and testable with **no tokio, no `LocalSet`, no
paused clock, and no `async`** — and the shell handles are not buildable
without the driver. **I agree with the narrowing.** This slice is the
core.

### 1.2 In scope (slice 2a)

| Spec | What lands |
|---|---|
| §16.4 | `core::Endpoint<I: Identity>`, the `EndpointOutput` drain, `Disposition`, `Transmit`, `Install`, `EstablishedSession`, and the four synchronous staged verbs |
| §16.5 (endpoint half only) | the endpoint's min-deadline: pendings' retransmit/give-up, parked-intro expiry, guard orphan aging (4523–4526, quoted verbatim in §8) |
| §16.6 | the one seeded endpoint RNG, and the per-connection **sub-seed drawn at connection creation even while unused** |
| §5.3, §5.5, §5.7 | initiator driving: fresh ephemeral / index / timestamp per retransmit, 5 s + U[0,333 ms], give-up 90 s, one completion attempt per interval, msg2 source ignored |
| §5.6 | responder shape — the staged ladder, the msg1-source anchor recorded on the session |
| §6.1 | the `Intro`/`Claimed`/`Proven` chain **inside the core**, and the 1/2/4 DH ladder |
| §6.3 | the stage-0 queue, entire |
| §17.1 | the timestamp guard, including the provisional-write/revert rule |
| §17.2–17.4 | `last_init_timestamp`, both index tables with the re-draw rule, the static map with `replacement_basis` (written, not read), and the hint set (populated, not consulted) |
| §2, §3, §4 | *consumed*, not changed: `classify`, `Mac1Key`, the three headers, `Msg1Payload` |

### 1.3 Explicitly **not** in scope, with where each goes

| Not here | Where | Why |
|---|---|---|
| §6.2's `async` staged handles, `Endpoint`, `EndpointBuilder`, `Connecting` | **slice 3** | each verb is a driver round-trip (§6.2) |
| the driver actor, `Wire` pumping, `LocalSet` | slice 3 | — |
| `core::Connection`'s real surface (§7–§15) | slices 3–6 | this slice defines only the minimal `Connection` §16.4's `accept()`/`connect()` signatures force it to return |
| §6.4 re-home, §6.5 routing + hint consultation + eager read, §6.6–6.8 tie-break and restart | **slice 7** | `PLAN.md` line 286 puts them there, and that reading is right — with **one exception**, F-1 below |
| §7.3's amplification budget on the responder anchor | slice 7 | §5.6 arms it; the anchor address is recorded here, the budget is enforced there |

### 1.4 The one place the boundary leaks — and what I propose

**F-1 restated.** §6.3 rule 4 exempts `accept()` from expiry *by
appealing to §6.4*: "a chain's age never fails an `accept()`, only the
absence of any parked initiation does." And §5.4 (833–866) makes
`accept()` consult a **three-valued** local state for the proven static,
"always … post-`ss`": **LIVE** / **PENDING** / **NONE**. Only the NONE
row is self-contained in this slice's sections.

Proposal, and it is deliberately the *conservative* half:

- **NONE** — implemented fully: mint index, `write_message_2`, install,
  write `replacement_basis = Some(t)`, emit the `Transmit`.
- **LIVE** and **PENDING** — `accept()` returns
  `AcceptError::Stale`, which is the **only** variant §18.1 offers
  (`error.rs:98–105`: `Stale` and `EndpointDropped`, nothing else), and
  which is *already the correct answer* for §6.4's PENDING/tie-break-winner
  branch. For LIVE it is **not** correct in general — a replacement whose
  timestamp passes both tests must succeed — so it is a **documented,
  test-pinned slice boundary**, not a silent approximation.
- **Expiry and `accept()`** — with §6.4 absent there is no "freshest
  parked initiation" to re-home onto, so an expired chain's `accept()`
  has nothing to succeed with. Slice 2a returns `AcceptError::Stale`
  there too ("no initiation is parked for this static" is literally that
  variant's documented meaning), and slice 7 replaces the arm.

The invariant that must not be broken meanwhile is §16.1's
**one session per peer static**: slice 2a must never install a second
connection for a static that already has one. Returning `Stale` on LIVE
preserves it. This is stated as a test obligation in §9, not left to
care.

## 2. Module layout — exact files, exact items, `pub` vs `pub(crate)`

`PLAN.md`'s module map (lines 44–96) already names most of these files.
This slice follows it exactly, with **three deviations, flagged**:
`src/identity.rs` and `src/config.rs` are new (the map has no home for
either), and `core/endpoint/routing.rs` is **not** created here (it is
§6.5–6.8, slice 7).

```
src/
  identity.rs              NEW · pub    the Identity seam (§3 below)
  config.rs                NEW · pub    Config + the wall-clock service (§16.5)
  packet/
    handshake.rs           NEW · pub    the `Handshake` extension trait (§4 below)
    suite.rs               EXTENDED     `channel!` also stamps `impl Handshake`
    mod.rs                 EXTENDED     `pub mod handshake; pub use handshake::Handshake;`
  core/
    mod.rs                 NEW · pub(crate)
    endpoint/
      mod.rs               NEW   core::Endpoint<I>, the drain, handle_datagram
      handshake.rs         NEW   initiator driving + responder msg2 write (§5)
      staged.rs            NEW   IntroId, the chain, the four verbs (§6.1)
      intro_queue.rs       NEW   §6.3 entire
      guard.rs             NEW   §17.1
      tables.rs            NEW   §17.2–17.4
    connection/
      mod.rs               NEW   the MINIMAL core::Connection (see 2.4)
    tests.rs               NEW   the in-crate acceptance tests (§9)
  testutil/mod.rs          EXTENDED     CountingIdentity (Cargo.toml already promises it)
```

### 2.1 Visibility, and why `core` is `pub(crate)` this slice

`core` is **`pub(crate)`** in slice 2a. Reasons, in order of weight:

1. Nothing outside the crate can drive it — there is no driver until
   slice 3 — so publishing it now publishes an unusable surface that
   semver then freezes.
2. §16.6 (4565–4570) says a build accepting a **caller-chosen RNG seed**
   is "security-relevant and must be feature-gated or documented as
   such", and §16.4's `core::Endpoint::new` takes `rng_seed: [u8; 32]`
   unconditionally. Keeping the constructor crate-internal defers that
   decision to the slice that has an opinion about the public surface
   (§16.11, slice 8) instead of guessing now.
3. Promotion later is additive; demotion is breaking.

`pub(crate)` also decides where the tests live: **`src/core/tests.rs`**,
following the exact precedent slice 1 set with `src/packet/tests.rs`
(851 lines, `pub(crate)` items) alongside the public-surface tests in
`tests/spec_packet.rs`. This slice adds **no** file under `tests/`.

`Identity`, `Config`, `Timestamp`, `Handshake` and the ID newtypes are
**`pub`** — `Identity` because a consumer implements it (bubble-engine's
`EnclaveSlitherIdentity` is the named case, S21), `Config` because a
consumer builds it, `Handshake` because `channel!` expands in the
*caller's* crate and every path the expansion names must be publicly
reachable from there (the same forcing that already made
`suite::PROTOCOL_NAME_CAP` `pub`).

### 2.2 Exact items

**`src/identity.rs`** (§3 gives the rationale)

```rust
pub trait Identity { type Suite; type Provider; type Error;
                     fn public_static(&self) -> &PublicKeyOf<Self>;
                     fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>; }
pub type CurveOf<I>      = <<I as Identity>::Suite as Channel>::Curve;
pub type PublicKeyOf<I>  = <CurveOf<I> as hiss::curve::Curve>::PublicKey;
pub type PrivateKeyOf<I> = <<I as Identity>::Provider as CryptoKeyProvider<CurveOf<I>>>::PrivateKey;
pub struct SoftwareIdentity<S, R> { … }   // the crates.io-only default
```

The three aliases are not sugar: without them every signature in the
core spells
`<<I as Identity>::Provider as CryptoKeyProvider<<<I as Identity>::Suite as Channel>::Curve>>::PrivateKey`,
which is a `clippy::type_complexity` on sight.

**`src/config.rs`**

```rust
pub struct Config { intro_queue_cap: usize, intro_max_per_source: usize,
                    clock: Rc<dyn WallClock> }          // §6.3: cap and per-source are
pub trait WallClock { fn now(&self) -> Timestamp; }     // "configurable in Config";
pub struct SystemClock;                                 // INTRO_TTL is NOT
```

§16.5 (4502–4505) puts the wall clock here in as many words: *"The
initiation timestamp (§5.3) is the one wall-clock read, behind a **clock
service injected in the endpoint config**."* `Rc<dyn …>`, not
`Arc`/`Box<dyn Send>` — see §3.

**`src/core/mod.rs`** — `Transmit`, `Disposition`, `EndpointOutput`,
`ToEndpoint`, `Install`, `EstablishedSession`, `ConnectionId`,
`Timestamp`. Verbatim from §16.4's block.

**`src/core/endpoint/staged.rs`** — `IntroId(u64)`, the chain state
machine, the four verbs.

**`src/core/endpoint/intro_queue.rs`** — `IntroQueue`, `IntroEntry`,
`SourceKey`.

**`src/core/endpoint/guard.rs`** — `TimestampGuard`, `GuardEntry`,
`ProvisionalWrite`.

**`src/core/endpoint/tables.rs`** — `IndexTables` (both maps + the
re-draw mint), `StaticMap` (with `replacement_basis`), `HintSet`.

### 2.3 `Timestamp` — a new public type, and one free property

`§16.4`'s `authenticate() -> Result<(PublicKey, Timestamp), AuthError>`
names a type that does not exist yet. `packet::Msg1Payload` is its
`pub(crate)` wire encoding (`secs: u64 ‖ nanos: u32`, big-endian by
ruling 64). Define:

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Timestamp { secs: u64, nanos: u32 }
```

The derived `Ord` on `(secs, nanos)` is **byte-for-byte the lexicographic
order of the 12 big-endian wire octets**, for every input including
`nanos ≥ 1_000_000_000`. So the guard's "strictly greater" comparison
needs no normalisation and no validation gate — and **no validation gate
may be invented**, since §5.3 specifies none and a rejection slither
invents is a wire behaviour the spec does not have.

### 2.4 The minimal `core::Connection`

§16.4 forces `connect()` and `accept()` to return
`(ConnectionId, core::Connection)`, so the type must exist now. Slice 2a
gives it exactly: the `EstablishedSession` (or `None`, for a
`connect()`-created connection awaiting its `Install`),
`handle_endpoint_event(now, Install)` which installs **exactly once**,
`poll_output()` returning `Timeout(None)` after an `Established` event,
and nothing else. Every §7–§15 verb arrives in slices 3–6. This is
stated as a boundary so a reviewer does not read the stub as a
regression.

### 2.5 One build hazard worth naming

A crate-level `mod core` makes a **bare** `use core::mem::…` in any
submodule an `E0659` ambiguity against the `core` crate in the extern
prelude. The existing code already writes `::core::` absolutely
(`suite.rs:255–258`, `payload.rs:83`), so the convention is in place;
this slice keeps it and adds `crate::core::…` for the module. Cheap to
state now, expensive to debug later.

## 3. `Identity` and the `DhProvider` seam — how S21's no-`Send` is enforced

### 3.1 Why `Identity` is a *factory*, not a key holder

Three facts from hiss 0.3.2, read from source:

1. Every handshake instance **consumes a provider by value**:
   `IK::initiator(provider, prologue, remote_static)` and
   `IK::responder(provider, prologue, static_key)`
   (`hiss-macros-0.3.1/src/codegen.rs:582–606`, bounded
   `CP: ::hiss::provider::DhProvider<Curve>`).
2. Each also needs **our static private key by value** — the responder
   at construction, the initiator at `write_message_1(static_key, &payload)`.
3. `CryptoKeyProvider::PrivateKey` is **deliberately not `Clone`**
   (`hiss-0.3.2/src/provider/mod.rs:118–125`: *"secret keys should not be
   silently duplicated"*).

An endpoint runs many handshakes at once — up to `INTRO_QUEUE_CAP` parked
mid-states, each of which §17.5 says "holds the endpoint's static
provider". So the seam cannot be "the endpoint owns one provider and one
key"; it must be able to **mint a (provider, static-private-handle) pair
per handshake**:

```rust
pub trait Identity {
    type Suite: Channel;
    type Provider: DhProvider<CurveOf<Self>>;
    type Error: core::error::Error + 'static;

    /// The canonical static public key (§2.4). Cheap, cached, infallible:
    /// mac1 keying and every table key read it on the hot path.
    fn public_static(&self) -> &PublicKeyOf<Self>;

    /// Mint the provider and static-key handle for ONE handshake.
    fn open(&self) -> Result<(Self::Provider, PrivateKeyOf<Self>), Self::Error>;
}
```

`open()` is where each backend does the thing only it can do:
`SoftwareIdentity` re-imports the 32 scalar bytes
(`P256r1PrivateKey::from_bytes`, `hiss-0.3.2/src/curve/p256/software.rs:44`)
and clones a seeded RNG into a fresh `EphemeralOnly`; an enclave identity
retains its `SecKey` handle (a refcount bump, not a key copy) — which is
exactly why S21's *"key material is **non-exportable**"* survives: the
trait never asks for bytes.

`&self`, not `&mut self`, so `open()` can be called while the queue is
borrowed. Fallible, because an enclave call can fail and because the
alternative is a panic on the accept path.

### 3.2 How the no-`Send` requirement is **enforced**

Three mechanisms, in increasing strength. Only the third is enforcement.

1. **No bound is written.** `Identity`, `Identity::Provider`,
   `Identity::Error`, `Config`, `WallClock` and every `core` type carry
   no `Send`, no `Sync`, no `'static`-beyond-storage. `Config` holds
   `Rc<dyn WallClock>`, not `Arc<dyn WallClock + Send + Sync>` — a
   deliberate choice, since the `Arc` shape would be the natural default
   and would silently make `Config: Send` the thing consumers depend on.
2. **A `!Send` identity is used by the tests that already exist.**
   `testutil::CountingProvider` holds `Rc<Cell<u32>>`
   (`src/testutil/mod.rs:734–737`) and is therefore **already `!Send`**.
   The `CountingIdentity` built on it is `!Send`. Every DH-ladder test in
   §5 below is therefore also a compile-time proof that no `Send` bound
   sits anywhere on the path: **add one, and the DH-cost tests stop
   compiling**, which reddens the `Compiles` and `Tests` gates together.
   This is the cheapest possible enforcement and it is free — the type
   already exists, built for this slice.
3. **A negative assertion, so (2) cannot go vacuous.** (2) only proves
   something if `CountingIdentity` really is `!Send`. Pin it with the
   standard autotrait-ambiguity trick, hand-written so no dependency is
   added:

   ```rust
   // in src/core/tests.rs
   trait AmbiguousIfSend<A> { fn assertion() {} }
   impl<T: ?Sized> AmbiguousIfSend<()> for T {}
   impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
   // Compiles ONLY while the type is NOT Send; two applicable impls
   // otherwise, and inference fails.
   const _: fn() = || { <core::Endpoint<CountingIdentity> as AmbiguousIfSend<_>>::assertion() };
   ```

   Note what is asserted and what is not: **not** that every endpoint is
   `!Send` (a software endpoint may well be `Send`, and forbidding that
   would be over-tight), but that *this* endpoint, over a `!Send`
   identity, exists and works. That is precisely S21's claim.

### 3.3 `DhProviderAsync` — deliberately **not** accommodated

Decision: `Identity::Provider: DhProvider<_>` only. `DhProviderAsync`
(`hiss-0.3.2/src/provider/mod.rs:213`) is **excluded on purpose**, and
the reasons are independent, so no one of them going away reopens it:

1. **hiss cannot drive it.** The generated state machines are bounded
   `CP: DhProvider<Curve>` — the synchronous trait — at every entry point
   (`codegen.rs:586,602`), and hiss's own doc says so: *"It is what the
   state machines `noise!` generates are generic over"*
   (`provider/mod.rs:174–176`). An async-only backend cannot run an IK
   handshake through hiss at all.
2. **§16.4 forbids the shape.** The core's staged verbs are ratified
   synchronous. An async DH would make `read_identity`/`authenticate`/
   `accept` return futures *inside the core*, which is a spec change, not
   an implementation choice.
3. **It would reintroduce the very bound S21 exists to avoid.**
   `DhProviderAsync::dh_async` returns
   `impl Future<Output = …> + Send` — a `Send` requirement, written into
   the trait. Accommodating the async seam would import `Send` into the
   one seam built to be free of it.

Reason 3 is the one to record in the rustdoc: it is counter-intuitive,
and an agent optimising for "support both surfaces" would otherwise
reach for it. The rustdoc on `Identity` must say **why** the async
provider is absent, or a later slice re-proposes it.

## 4. The `Channel` handshake extension over hiss 0.3.1's staged read

### 4.1 The problem, stated exactly

`channel!` stamps **two** things into the caller's module: the suite type
with its `impl Channel`, and a type called `IK` — the `hiss::noise!`
state machine (`src/packet/suite.rs:224–234`). `Channel` deliberately
carries **no** handshake surface, and its own rustdoc says why
(`suite.rs:38–46`): *"That seam is designed where it is used; freezing a
guess for it in a public trait here would be the expensive kind of
wrong."*

Slice 2a **is** where it is used, so the seam gets designed now. The core
needs, generically over the suite, these six hiss types and seven calls:

| hiss item (reference case) | Where slither needs it |
|---|---|
| `IKInitiatorMsg1<CP>` — `IK::initiator(p, prologue, remote_static)` | `connect()`, and every retransmit |
| `.write_message_1(our_priv, &[u8;12]) -> ([u8; MSG1_SIZE], IKInitiatorMsg2<CP>)` | msg1 bytes; 2 DH (`es`, `ss`) |
| `IKInitiatorMsg2<CP>::read_message_2(&msg2) -> Transport<IK>` | completion; +2 DH (`ee`, `se`) |
| `IKResponderMsg1<CP>` — `IK::responder(p, prologue, our_priv)` | built lazily, at `read_identity()` |
| `.read_message_1_intro(&msg1) -> (PublicKey, IKResponderMsg1Intro<CP>)` | `read_identity()`; 1 DH (`es`) |
| `IKResponderMsg1Intro<CP>::complete() -> ([u8;12], IKResponderMsg2<CP>)` | `authenticate()`; +1 DH (`ss`) |
| `IKResponderMsg2<CP>::write_message_2() -> ([u8; MSG2_SIZE], Transport<IK>)` | `accept()`; +2 DH (`ee`, `se`) |

State-type naming is mechanical and stable:
`{Pattern}{Role}Msg{n}` for states (`codegen.rs:129`) and
`{Pattern}{Role}Msg{n}Intro` for the mid-state (`codegen.rs:1382`), so
`channel!` can name all six literally from its own `IK` identifier.

### 4.2 The recommendation: a new `Handshake` trait, `Channel` untouched

```rust
// src/packet/handshake.rs
pub trait Handshake: Channel {
    type Initiator<P: DhProvider<Self::Curve>>;
    type InitiatorSent<P: DhProvider<Self::Curve>>;
    type Responder<P: DhProvider<Self::Curve>>;
    type Msg1Intro<P: DhProvider<Self::Curve>>;
    type ResponderRead<P: DhProvider<Self::Curve>>;
    type Transport;

    fn initiator<P: …>(provider: P, prologue: &[u8], remote: &PublicKey) -> Self::Initiator<P>;
    fn write_msg1<P: …>(st: Self::Initiator<P>, ours: P::PrivateKey, payload: &[u8; MSG1_PAYLOAD_LEN])
        -> Result<(Vec<u8>, Self::InitiatorSent<P>), HandshakeError>;
    fn read_msg2<P: …>(st: Self::InitiatorSent<P>, msg2: &[u8]) -> Result<Self::Transport, …>;
    fn responder<P: …>(provider: P, prologue: &[u8], ours: P::PrivateKey) -> Result<Self::Responder<P>, …>;
    fn read_msg1_intro<P: …>(st: Self::Responder<P>, msg1: &[u8])
        -> Result<(PublicKey, Self::Msg1Intro<P>), …>;
    fn complete<P: …>(mid: Self::Msg1Intro<P>)
        -> Result<([u8; MSG1_PAYLOAD_LEN], Self::ResponderRead<P>), …>;
    fn write_msg2<P: …>(st: Self::ResponderRead<P>) -> Result<(Vec<u8>, Self::Transport), …>;
}
```

Four properties this shape buys, each of which is why it beats the
alternatives:

- **`Channel` is not modified.** Slice 1 froze `Channel` and pinned it in
  `tests/spec_packet.rs` and `src/packet/tests.rs`, both of which this
  slice may not touch. A supertrait extension adds items without moving
  one existing item, so those tests keep passing unchanged.
- **A supertrait, not a cycle.** `trait Handshake: Channel` is
  one-directional. (`trait Channel: Handshake` would need `Handshake`'s
  associated types to name `Self::Curve`, which lives on `Channel` — a
  supertrait cycle, and not allowed.)
- **GATs keep the core's bounds to one clause.** The core writes
  `I::Suite: Handshake` and nothing else. The alternative —
  `trait Handshake<P>` with the provider as a *trait* parameter — forces
  `I::Suite: Handshake<I::Provider>` on every `impl` block and every
  function in the core. GATs have been stable since 1.65, far below the
  1.96 MSRV.
- **One `impl` block in the macro.** `channel!` gains a single
  `impl $crate::packet::Handshake for $name { … }` naming
  `IKInitiatorMsg1<P>` and friends. Consumers' existing `channel!`
  invocations gain the impl for free; nothing they wrote changes.

**Fallback if GAT inference bites** (it should not, since every use site
fixes `P = I::Provider`): drop to `trait Handshake<P>` and carry the
extra bound. Recorded so the implementer does not treat a GAT error as a
blocker.

### 4.3 Two decisions inside the seam

**Return `Vec<u8>` or the fixed array?** hiss returns owned fixed arrays
(`[u8; MSG1_SIZE]`), and §16.4's `Transmit { to, data: Vec<u8> }` needs a
`Vec` anyway because the packet is `header ‖ msg ‖ mac1`. The trait
returns `Vec<u8>` of the **Noise message only**, and the caller frames
it. Rationale: a generic trait cannot name `[u8; C::MSG1_LEN]` without
`generic_const_exprs`, which is not stable. The one allocation per
handshake message is not on any hot path — a flood never reaches here
(0 DH, 0 allocations, dropped at mac1).

**`Transport` has no `P`.** `hiss::noise::Transport<IK>` is generic over
the *pattern*, not the provider (`codegen.rs:814`) — the provider is
consumed by the handshake and does not survive into the session. That is
why `type Transport;` needs no GAT, and it is also what lets
`EstablishedSession` be provider-free, which matters when slice 4 stores
it inside `core::Connection`.

### 4.4 What `EstablishedSession` holds

§16.4 (4364–4365) specifies it: *"the hiss transport pair (seal + open),
our session index, the peer's index, and the anchor address (§5.6)"*. The
pair is hiss's `DatagramSend`/`DatagramRecv`
(`Transport::into_datagram_with_epoch`, `hiss-0.3.2/src/noise/datagram.rs:133`).
Slice 2a performs the split **at install** and stores the halves; the
epoch/ratchet arguments are §7.7 and land in slice 3, so the split call's
parameters are the one thing here that slice 3 may revise. Flagged rather
than hidden.

## 5. The typestate ladder and the 1/2/4 DH cost pinned by test

### 5.1 The chain, and the one Rust hazard in it

`IntroId(u64)` — a monotone endpoint counter, never reused. It is **not**
derived from the source address or `sender_index`: §6.1 (1062–1064)
forbids anything durable keyed on either, and a monotone counter is
keyed on nothing.

The chain must move owned, non-`Clone` hiss values out from behind
`&mut self` (`complete(self)` and `write_message_2(self)` both consume).
That forces a take-and-replace shape, worth writing down because getting
it wrong ends in a spurious `Clone` bound the hiss types cannot satisfy:

```rust
enum ChainState<C: Handshake, P> {
    Parked  { bytes: Box<[u8; …]> },                                   // 0 DH
    Claimed { mid: Box<C::Msg1Intro<P>>, claimed: PublicKey },         // 1 DH
    Proven  { st: Box<C::ResponderRead<P>>, peer: PublicKey, ts: Timestamp }, // 2 DH
    Poisoned,                                       // transient, mem::replace only
}
```

`Poisoned` is never observable: it exists for the instant between
`mem::replace` and writing the successor, and a verb that finds it is a
bug (`debug_assert!`, then behave as `Expired`). Boxing is not
decoration — Appendix A.1 measures the mid-state at **784 B on P-256**,
and an unboxed variant is a `clippy::large_enum_variant` plus a 784-byte
memcpy on every queue move.

### 5.2 Where each DH is paid

| Verb | hiss call | DH paid | Cumulative |
|---|---|---|---|
| park | *(none)* | — | **0** |
| `read_identity(id)` | `IK::responder(…)` then `read_message_1_intro` | `es` | **1** |
| `authenticate(now, id)` | `mid.complete()` | `ss` | **2** |
| `accept(now, id)` | `write_message_2()` | `ee`, `se` | **4** |

The responder state machine is built **lazily, at `read_identity()`**,
not at park. Building it at park would call `Identity::open()` — an
enclave round-trip — for every mac1-valid packet in a flood: a stage
ratified at 0 DH but plainly not at 0 cost. §17.5 corroborates: a
mid-state "holds the endpoint's static provider: for a hardware/enclave
static this is up to 1024 concurrent provider handles, an operationally
scarce resource". Slice 2a therefore holds **zero** provider handles for
unconsumed entries and at most one per consumed chain.

Initiator side, because it is the half a DH-count test trips over:
`write_message_1` pays **2** (`es`, `ss`) and `read_message_2` pays **2**
(`ee`, `se`). Since §5.5 makes every retransmit a completely fresh
initiation, **each retransmit costs 2 more DH** — an unanswered 90 s dial
spends 2 × (1 + ⌊90/5⌋) ≈ 38. That is the price of the fresh-ephemeral
rule, it is intended, and a test asserting "a dial costs 4 DH" without
controlling `now` will fail mysteriously once a retransmit fires.
`testutil`'s own doc already anticipates the shape (`4` per
cancel-and-redial cycle = 2 + 2).

### 5.3 The pins, as tests

`CountingProvider` counts **exactly one increment per `DhProvider::dh`
call and nothing else** (`src/testutil/mod.rs:726–733`) — key generation
is not a DH. `CountingIdentity` hands every `open()` a `CountingProvider`
sharing one `DhCounter`, so the count is **endpoint-wide and cumulative
across handshakes**, which is exactly what §6.1's table prices.

| Test | Asserts | Story |
|---|---|---|
| `park_costs_no_dh` | 1024 mac1-valid initiations parked ⇒ `dhs == 0` | S6 |
| `reject_at_intro_costs_no_dh` | park, `reject(id)` ⇒ `dhs == 0` and the drain yields **no `Transmit`** | S6 |
| `read_identity_costs_one_dh` | ⇒ `dhs == 1`, key equals the initiator's static | S7 |
| `reject_at_claimed_costs_one_dh` | then `reject` ⇒ still `dhs == 1`; `ss` never ran | S7 |
| `authenticate_costs_two_dh_cumulative` | ⇒ `dhs == 2` | S9 |
| `reject_at_proven_costs_two_dh` | ⇒ `dhs == 2`, nothing transmitted, nothing installed | S9 |
| `accept_fast_path_costs_four_dh` | ⇒ `dhs == 4`, exactly one `Transmit` of `RESP_PACKET_LEN` | S9 |
| `expiry_costs_no_further_dh` | park, advance past `INTRO_TTL`, `handle_timeout` ⇒ `dhs == 0` | S6/S10 |

Every one is an **ordinary `#[test]`** — no `#[tokio::test]`, no paused
clock, no `LocalSet`. `now` is a plain `Instant` the test advances by
arithmetic. That falls straight out of §16.4's "`now: Instant` is an
argument … the cores never read a clock", and it is worth saying out loud
because this project's habit is to reach for the paused clock, which is
right for the shell and unnecessary here.

**The 4-DH pin is named for the fast path** (`accept_fast_path_…`)
because §6.1 (1054–1056) adds `es` + `ss` on top of the 4 for a re-homed
`accept()`. Slice 7 then adds a second test instead of editing this one.

**Assertions are cumulative, never per-call.** §6.1's dagger note
(1045–1052) says a pre-read or frozen entry returns cached results at
**0 incremental DH** while "the cumulative table is unchanged". A test
written as "`read_identity` costs 1" is false for those entries; written
as "cumulative after `read_identity` is 1" it is true for all of them.

## 6. The stage-0 queue (§6.3)

### 6.1 The structure

```rust
struct IntroQueue<C, P> {
    entries:    HashMap<IntroId, IntroEntry<C, P>>,   // ≤ cap, total both tiers
    by_addr:    HashMap<SocketAddr, IntroId>,         // UNCONSUMED entries only
    per_source: HashMap<SourceKey, u16>,              // unconsumed + consumed
    next_id:    u64,
}
struct IntroEntry<C, P> {
    id: IntroId, src: SocketAddr, sender_index: u32,
    parked_at: Instant, deadline: Instant,
    consumed: bool,                  // read_identity() OR freeze-on-carry
    state: ChainState<C, P>,
    guard_undo: Option<GuardUndo>,   // §7.2 — the provisional guard write
}
enum SourceKey { V4(Ipv4Addr), V6Prefix64([u8; 8]) }   // §6.3: "per source IP (per /64 for IPv6)"
```

Two keys, two scopes, and they are **different on purpose**: the dedup
key is the full `SocketAddr` (1158), the cap key is the IP / v6 /64
(1143). Distinct initiators behind one NAT present distinct ports, so
they dedup separately while sharing one cap — which is the stated intent.

Eviction is a **linear scan** over ≤ 1024 entries, not a heap. Two
reasons: the honesty clause prices sustained full occupancy at ≈ 68
packets/second (1024/15 s), so the scan is ~70k comparisons/second in the
worst case the spec itself contemplates; and a heap keyed on park time is
*wrong* the moment a refresh changes the key, which is precisely the
ambiguity F-2 records. A scan cannot be silently wrong about an ordering
it recomputes each time.

`by_addr` is a map keyed on the source address, which is the exact shape
§6.1 warns about. It is admissible because §6.3 mandates the queue and
because it is **bounded and TTL'd** — 1024 entries, 15 s — not durable.
The distinction must be preserved by construction, so: the queue is the
**only** structure in this slice keyed on an unauthenticated quantity,
and a code-review criterion for the slice is that no other map, counter
or trace is keyed on `src`, `sender_index`, or a *claimed* static.

### 6.2 The arrival algorithm — derived, and stated so it can be objected to

§6.3 gives three rules and never states their **order**, and the orders
are not equivalent (F-3). Derived order, with the reason each step sits
where it does:

```
on a datagram that classify() returns as Init, and whose mac1 verifies
against Mac1Key::derive(our_static):                                 // 0 DH

1. dedup:  if let Some(id) = by_addr.get(&src)      // unconsumed by construction
           → replace bytes, refresh deadline (and parked_at — F-2),
             same IntroId, NO IntroReady, per_source unchanged.  DONE.
2. cap:    let k = SourceKey::of(src);
           if per_source[k] >= INTRO_MAX_PER_SOURCE
               → evict that k's oldest UNCONSUMED entry;
                 if it has none (allowance wholly consumed) → DROP silently. DONE.
3. global: if entries.len() >= INTRO_QUEUE_CAP
               → evict the globally oldest UNCONSUMED entry;
                 if none exists → DROP silently (see below).
4. park:   insert; per_source[k] += 1; emit IntroReady(id, src).
```

- **1 before 2** because dedup is net-zero for the cap: a same-`SocketAddr`
  arrival is a *replacement*, and running the cap first could evict a
  stranger to make room for an entry that was never going to be added.
- **2 before 3** because the per-source rule replaces within the source,
  leaving the total unchanged — so the global cap cannot trip afterwards.
  Global-first would evict a different source's entry *and then still*
  have to enforce the per-source cap: one wasted eviction of an innocent.
- **Step 3's "if none"** is unstated in §6.3. The per-source
  all-consumed drop is stated (1179–1180); the global all-consumed case
  is not. Dropping is the only option that does not breach §17.5's
  ceiling of "one budget of `INTRO_QUEUE_CAP` slots", so it is derived,
  not chosen. Recorded in §11(a).

### 6.3 Consumption, supersession, freezing

- `consumed` flips at `read_identity()` **and only there** in this slice
  (freeze-on-carry needs §6.5's eager read — slice 7). At that moment:
  remove from `by_addr` (freeing the source's stage-0 slot),
  `per_source` **unchanged** (§6.3: net-zero, −1 unconsumed +1 consumed —
  which is exactly what "unchanged" is, and modelling the tiers as one
  counter is what makes it unchanged *by construction* rather than by a
  matched pair of ±1s that can drift).
- A consumed chain is **never** byte-replaced and **never** evicted by
  either cap. Only expiry and the verbs remove it.
- Accessors reflect the **newest bytes at call time** (1203). See F-4:
  this has a consequence §16.4's API list does not carry.
- `Superseded` is in no error enum, and none is added.
- The entry type already carries `state: ChainState`, so slice 7's
  freeze-on-carry adds a `Parked → Claimed`-at-park construction and sets
  `consumed = true`; **no reshaping**. Designed for that now, since
  §6.3 says "Appendix B's DH-cost pins assume it".

### 6.4 Expiry

- TTL is `INTRO_TTL` (15 s) from **last refresh** for an unconsumed
  entry; for a consumed chain, §6.3 is explicit that the mid-state
  "expires 15 s after the **initiation that fed it**" — so a consumed
  entry's deadline is frozen at consume time, not extended by anything.
- Expiry is **silent eviction**, emitting nothing.
- Verbs on an expired/absent id: `read_identity` →
  `IntroError::Expired`; `authenticate` → `AuthError::Expired`;
  `accept` → `AcceptError::Stale` (§1.4); `reject` → no-op, infallible
  (§16.4 gives it no `Result`).
- Expiry of a **consumed** chain must run its `guard_undo` (§7.2) — this
  is the failure mode that would otherwise leave a guard entry a peer
  never got to use, silently blocking that peer's next genuine
  initiation. It is the single most consequential line in the queue.

### 6.5 Tests (all synchronous)

`queue_caps_at_1024` · `per_source_caps_at_four` ·
`per_source_counts_consumed_and_unconsumed_together` ·
`ipv6_shares_a_cap_across_a_64` · `dedup_replaces_and_keeps_the_intro_id` ·
`dedup_emits_no_second_intro_ready` · `overflow_evicts_oldest_unconsumed` ·
`overflow_never_evicts_a_consumed_chain` ·
`all_consumed_source_drops_the_arrival` ·
`expiry_is_silent_and_frees_the_source_count` ·
`consumed_chain_expires_15s_after_its_initiation` ·
`read_identity_frees_the_stage0_slot_for_a_new_entry` ·
`consumed_chain_is_never_byte_replaced` ·
`per_source_counters_return_to_zero_when_the_queue_drains` (the leak
canary — a stuck counter is a permanent per-source denial and is
invisible to every other test).

## 7. The timestamp guard (§17.1) and the tables (§17.2–17.4)

### 7.1 The guard's shape

```rust
struct TimestampGuard {
    entries: HashMap<StaticKeyBytes, GuardEntry>,       // canonical §2.4 octets
    orphan_lru: /* intrusive order over the unpinned tier */,
}
struct GuardEntry {
    greatest: Timestamp,
    pins: u32,                     // live connections + pendings + mid-states
    exempt_until: Option<Instant>, // §6.6/§6.7's HANDSHAKE_GIVEUP extension (slice 7)
    last_admitted: Instant,        // LRU recency — admission only
}
```

`pins` is a **count**, not a bool: §17.1 pins on "a live `Connection`, an
in-flight outbound pending, **or** a staged mid-state", and a static can
have a mid-state and a pending at once (that is §5.4's PENDING row). A
bool loses the second pin on the first release, un-pinning a live entry.

Slice 2a's only *admission* write site is `authenticate()`; the other
three (§6.4 re-home, §6.6 tie-break admit, §6.7 winner-side record) are
slice 7 and are left as unimplemented arms with `exempt_until` already in
the struct so slice 7 adds a call, not a migration.

### 7.2 The provisional write — the part that is easy to miss

§17.1's mitigation **(i) no-orphan-on-reject**: "a static authenticated
and then rejected without ever being accepted writes no orphan (its
record drops with the chain; **a pre-existing entry reverts**)". So the
write at `authenticate()` is *provisional until `accept()`*, and the
chain must carry its undo:

```rust
struct GuardUndo { key: StaticKeyBytes, previous: Option<Timestamp> }
```

- `authenticate()` — check `t > entry.greatest` (strictly; absent ⇒
  passes vacuously). On pass: record `previous`, write `greatest = t`,
  refresh LRU recency, take a pin for the mid-state. On fail:
  `AuthError::Replay`, **no** write, **no** LRU refresh (mitigation
  (iii): "recency refreshes on a successful post-`ss` record, never on a
  failed check").
- `accept()` — the write becomes permanent: drop `guard_undo`, convert
  the mid-state pin into a connection pin.
- `reject()`, drop, expiry, eviction — **run `guard_undo`**: restore
  `previous`, or remove the entry entirely if there was none. Release the
  pin.

Test it directly, because the failure is silent and permanent:
`authenticate_then_reject_leaves_the_guard_empty` and
`authenticate_then_reject_restores_a_prior_value`. Without the second,
an implementation that removes the entry outright passes the first and
still destroys a real peer's replay protection.

### 7.3 Two things slice 2a must **not** do to the guard

- **The initiator path never touches it.** §17.1 (4835–4841): for a
  static we only ever dialled "we hold **no entry at all** — every §17.1
  write site is a post-`ss` read of an *inbound* msg1, and a `connect()`
  completed by msg2 writes nothing". A `connect()` that writes a guard
  entry is a defect, and `dial_writes_no_guard_entry` is the test.
- **No entry is ever created by a pin.** §17.1: for a staged mid-state
  "the pin never *creates* an entry … flipping a bit on an entry a
  key-holder already wrote". The same holds for the pending and
  connection pins by the previous bullet. So `pin(static)` on an absent
  key is a **no-op**, not an insert.

### 7.4 Orphan aging and the LRU

- `TS_GUARD_ORPHAN_CAP` = 1024 (already in `constants.rs:398`).
- Eviction: LRU over **unpinned** entries only, recency = last successful
  admission.
- Timer aging: mitigation (ii) says orphans age out on an
  "`INTRO_TTL`-scale timer" (§17.1 and again at line 1593). **There is no
  named constant** — see §11(b). The implementation cannot avoid picking
  a number, and picking one silently is how an unratified constant enters
  the wire's behaviour. Proposal, pending a ruling: a `Config` field
  `ts_guard_orphan_ttl` defaulting to `INTRO_TTL`, so the choice is
  visible, overridable and greppable rather than buried.

### 7.5 The tables (§17.2–17.4)

**`last_init_timestamp` (§17.2)** — one endpoint-global `Timestamp`.
Every outbound initiation draws `t = max(wall_clock.now(),
last_init_timestamp.succ())` and stores it. `succ()` = +1 **nanosecond**
with carry into seconds. §5.3 says "forced strictly greater" and never
names the increment unit; nanoseconds is the only unit the 12-byte
encoding has, so it is derived rather than chosen — recorded in §11(a)
so a reviewer can object rather than discover.
Test: `two_dials_in_the_same_coarse_tick_emit_strictly_increasing` and
`close_and_reconnect_still_emits_strictly_greater` (§17.2's stated
reason for endpoint scope).

**The index tables (§17.3)** — `sessions: HashMap<u32, ConnectionId>` and
`pendings: HashMap<u32, ConnectionId>`. Minting: draw a random **nonzero
`u32` from the endpoint RNG** and **re-draw while present in *either*
table**. §17.3 states why: "a pending's msg1 index graduates into the
session index on completion; this closes the route-stealing collision".
Tests: `minted_index_is_never_zero`,
`minted_index_avoids_both_tables` (seed the RNG so the first draw
collides — this is exactly what a config-supplied seed is for), and
`data_on_an_unknown_index_touches_nothing` ⇒ `Disposition::Done`, no
state change anywhere (§17.3's stated corollary).

**The static map and hint set (§17.4)** —
`statics: HashMap<StaticKeyBytes, (ConnectionId, Option<Timestamp>)>`,
the second field being `replacement_basis`: `Some(t)` when we
**responded** (slice 2a's staged `accept()`), `None` when we **dialled**.
Written **once at install, never updated**, dies with the connection.
§6.4 is its only reader — and §6.4 is slice 7 — so slice 2a **writes it
and never reads it**. That is not dead code: writing it wrong now is
undetectable until slice 7 and then looks like a slice-7 bug, so it gets
its own tests: `accept_records_a_some_basis_equal_to_the_msg1_timestamp`
and `connect_records_a_none_basis`.

The **hint set** is not a separate structure: §17.4 says it *is* "the
pending tables' dialled addresses". So it is a projection over the
pending table, and "established connections contribute no hints" is a
property to preserve by construction — slice 2a stores **no
per-connection address** in the endpoint (the connection core owns its
own endpoint address, §7.3). Test:
`an_established_connection_contributes_no_hint`. Consultation is §6.5,
slice 7.

## 8. §5.5–5.7 driving — retransmit, jitter, give-up

### 8.1 The pending initiation

```rust
struct Pending<C, P> {
    conn: ConnectionId, remote: SocketAddr, remote_static: PublicKey,
    peer_mac1: Mac1Key,                // derived once from remote_static
    sender_index: u32,                 // fresh EVERY attempt
    st: Box<C::InitiatorSent<P>>,      // fresh EVERY attempt
    timestamp: Timestamp,              // fresh, strictly greater, EVERY attempt
    next_retransmit: Instant,          // now + 5 s + U[0, 333 ms]
    give_up_at: Instant,               // start + 90 s, set ONCE
    attempt_spent: bool,               // §5.5 step 3
}
```

`connect(now, remote, remote_static)`:
`ConnectError::AlreadyConnected` if the static map holds a live
connection (documentation obligation #1, `lib.rs:47–50`); else mint a
`ConnectionId`, **draw the 32-byte connection sub-seed (§16.6, drawn even
though `core::Connection` does not use it yet — see 8.5)**, build the
first attempt, emit `Transmit`, return `(id, Connection)` with no session
installed. `Install` follows later, exactly once.

**Building one attempt** (used identically for the first send and every
retransmit — one function, so they cannot drift):
mint index (re-draw against both tables) → draw timestamp (forced
strictly greater) → `Identity::open()` → `IK::initiator(provider,
PROLOGUE, remote_static)` → `write_message_1(our_priv, &ts.encode())`
[2 DH, fresh ephemeral inside hiss] → frame
`InitHeader::new(index) ‖ msg1 ‖ peer_mac1.tag(preimage)` →
`Transmit { to: remote, data }`. The previous attempt's index is removed
from the pending table and the new one inserted, so an in-flight msg2 for
a superseded attempt no longer routes — which is correct: §5.5 requires
an **index match** against the *current* attempt.

### 8.2 Retransmit, jitter, give-up

- **5 s + U[0, 333 ms]**, from `RETRANSMIT_BASE` and
  `RETRANSMIT_JITTER_MAX` (`constants.rs:351,354`), drawn from the
  endpoint RNG (§16.6: "every index, jitter draw … comes from it").
  **Fixed interval, not exponential** — S2 says so explicitly and calls
  out that §13's exponential PTO governs the data path only.
- **Give-up at 90 s**, `HANDSHAKE_GIVEUP` — computed from the *first*
  attempt, never re-based by a retransmit.
- **Equal-deadline priority is normative** (§16.5, 4536–4537): "give-up
  beats a same-instant retransmit". Test it directly with a seed that
  lands a retransmit exactly on the 90 s mark:
  `give_up_beats_a_same_instant_retransmit` — the drain must yield
  `HandshakeFailed`, and **no** `Transmit`.
- On give-up: emit
  `EndpointOutput::HandshakeFailed(conn, ConnectError::TimedOut)`,
  remove the pending, free its index, release its guard pin, remove its
  static-map entry. The shell resolves `Connecting` with
  `Err(ConnectError::TimedOut)` and drops the never-established core
  (§16.4, 4483–4486) — that half is slice 3.

### 8.3 Completion

On a `PKT_HANDSHAKE_RESP` datagram, in this order:

1. `classify::<C>()` — **exact** `RESP_PACKET_LEN` (ruling 65), else drop
   silently.
2. `receiver_index` present in the **pending** table? Else drop.
3. mac1 verify against `Mac1Key::derive(our_static)` — we are the
   recipient, so the key is *ours*, not the peer's. Else drop.
4. `attempt_spent`? Then drop — §5.5 step 3, "a second msg2 in the same
   interval is dropped".
5. Set `attempt_spent = true` **before** the crypto (a failed completion
   *spends* the attempt), then `read_message_2` [2 DH]. On failure, the
   attempt stays spent and the next scheduled retransmit refreshes it.
6. On success: session live, our receiver index = our `sender_index`,
   peer's = the response's `sender_index`; anchor = **the dialled
   address**; `replacement_basis = None`; emit
   `ToConnection(conn, Install { session })`.

**The source address of msg2 is deliberately ignored** (§5.5 step 4) —
step 2 keys on the index and nothing else. Test:
`msg2_from_a_different_address_still_completes`, which is the kind of
rule an implementer "fixes" into a bug.

Ordering of 4/5 relative to 3 is what makes §5.5's claim true — "a
guessed-index or mac1-invalid msg2 can never spend anything". Tests:
`a_mac1_invalid_msg2_spends_nothing`,
`a_wrong_index_msg2_spends_nothing`,
`a_second_valid_msg2_in_one_interval_is_dropped`,
`a_failed_completion_spends_the_attempt_and_the_next_retransmit_refreshes_it`.

### 8.4 Cancellation (S29) — and a gap in §16.4

S29 requires that dropping the `Connecting` **cancels**: the retransmit
train stops, nothing is transmitted, and a `connect()` to the same static
on the very next line succeeds instead of returning `AlreadyConnected`.
The pending lives in the *endpoint* core, so the shell cannot do this
alone — but **§16.4's API list has no cancel verb**.

Proposal, rather than inventing one: the shell calls
`handle_connection_event(id, ToEndpoint::Retired { our_index })`. Its
documented effects are exactly cancellation's — "drop the index route"
and release "the guard-entry pin" (4487–4493) — and slice 2a extends it
to also drop the pending and its static-map entry, emitting **nothing**
(the `Connecting` is already resolved by the drop, so a `HandshakeFailed`
would be a second resolution). Recorded as F-5 in §11(a): I believe this
is the intended reading, but it is a reading, and inventing a verb where
the spec has one is the failure mode worth avoiding in both directions.

### 8.5 The sub-seed, and why it is in this slice

§16.6: "At connection creation the endpoint draws a 32-byte **sub-seed**
for the connection core (**drawn even while unused**, so later
connection-side randomness cannot perturb the endpoint's draw order)."
`core::Connection` uses no randomness until slice 4 — and that is exactly
the case the parenthesis is written for. Omitting the draw now and adding
it in slice 4 silently changes every seeded test's index and jitter
sequence. Draw it in slice 2a, at both `connect()` and `accept()`, store
it on the minimal `Connection`. Test:
`connection_creation_advances_the_endpoint_rng_by_one_subseed`.

### 8.6 The endpoint's deadline

§16.5 (4523–4526), verbatim: *"The endpoint core's deadline is the min
over its pendings' retransmit/give-up deadlines, the parked intros'
expiries, and the timestamp-guard orphan aging (§17.1)."* Slice 2a
computes it by scanning those three families at the end of each drain
and returns it as the terminal `Timeout(Option<Instant>)`. A timer wheel
is not needed at these cardinalities and §16.5's timer table is slice 3's
row in `PLAN.md`; noted so slice 3 knows it may replace the scan.
`handle_timeout(now)` is **idempotent**: each due deadline is
disarmed/advanced before its logic runs (§16.5, 4529–4531). Test:
`handle_timeout_twice_at_the_same_instant_is_a_no_op`.

## 9. Test plan

### 9.1 The rule that shapes everything else

Working rule 6: **the test author is not the implementer.** The tests are
written from the story and its spec section *first*, against an API that
does not exist yet, and the implementation is written against them. For
that to be possible, this plan must declare the API precisely enough to
compile against. §9.3 is that declaration.

### 9.2 Where the tests live, and what they need

`src/core/tests.rs`, a `#[cfg(test)] mod tests` in-crate — because §2.1
makes `core` `pub(crate)`. This is the precedent slice 1 set
(`src/packet/tests.rs`, 851 lines). **No file under `tests/` is added or
modified by this slice**, and in particular `tests/spec_constants.rs`,
`tests/spec_errors.rs`, `tests/spec_packet.rs` and `src/packet/tests.rs`
are untouched. If an implementer finds they must change one, that is a
finding to report, not an edit to make.

**Every test in this slice is a plain `#[test]`.** No `#[tokio::test]`,
no `tokio::time::pause()`, no `LocalSet`, no `FlakyWire`. `now` is an
argument (§16.4), so time is `let t0 = Instant::now();` and
`t0 + Duration::from_secs(5)`. Two endpoints are driven against each
other by hand:

```rust
let out = a.poll_output();               // EndpointOutput::Transmit { to, data }
b.handle_datagram(now, A_ADDR, &data);   // …and drain b to Timeout
```

A ~40-line `Harness` in the test module does the drain-to-`Timeout`
bookkeeping and collects outputs into a `Vec<EndpointOutput>` so tests
can assert on **order** — §16.4 makes generation order normative
(4496–4498), so `assert_matches!(outs[0], Transmit{..})` before
`outs[1]` is testing a rule, not an implementation detail.

### 9.3 What the test author needs declared

1. Every item in §2.2, by exact name and signature.
2. `testutil::CountingIdentity` — its constructor, its `DhCounter`
   accessor, and the fact that it is `!Send`.
3. A msg1/msg2 **framing helper** so a test can forge a mac1-valid
   initiation from an arbitrary source without standing up a second
   endpoint (needed for the flood and per-source tests, where 1024
   distinct endpoints would be absurd). It composes existing slice-1
   pieces — `InitHeader::new`, `Mac1Key::derive(responder_static).tag()`
   — so it introduces no new crypto.
4. The seeded-RNG constructor, so index-collision and jitter tests are
   deterministic (§16.6's config seed is *for this*).
5. The exact `EndpointOutput` / `Disposition` variants, since almost
   every assertion is a match on one.

### 9.4 Story coverage, honestly scoped

| Story | This slice closes | Waits for |
|---|---|---|
| **S6** — reject at 0 DH | **all of it, at core level**: park costs 0 DH, reject costs 0 DH, nothing transmitted | the `Intro` handle's ergonomics (slice 3) |
| **S7** — inspect for 1 DH | all of it at core level (`dhs == 1`, reject still 1) | ditto |
| **S8** — park a decision across turns | the **durability half**: the chain survives arbitrarily many `handle_timeout`/`handle_datagram` calls and expires at exactly `INTRO_TTL` | the "across event-loop turns" half is meaningless without a loop — slice 3 |
| **S9** — prove, then still decline | all of it at core level (2 DH, 4 DH, decline installs nothing) | slice 3 |
| **S10** — a flood does not disturb established connections | the **queue half**: bounds hold under 1024+ arrivals, 0 DH, established index routes still resolve to `Disposition::ForConnection` while saturated | "established connections keep running" needs `core::Connection` — slices 3–4 |
| **S21** — Secure Enclave key | **the enforceable half, and it is the important half**: a `!Send` identity drives the whole ladder, pinned by §3.2's negative assertion | the actor path's `!Send`-ness — slice 3 |
| **S29** — give up and immediately redial | the **core half**: cancel drops the pending, stops the train, frees the static so the next `connect()` does not return `AlreadyConnected` | drop-of-`Connecting` and ruling 50's *ordering* guarantee — slice 3 |
| **S2** — dial a peer that never answers | the **timer half**: `HandshakeFailed(TimedOut)` at exactly 90 s, retransmits at 5 s + jitter, not exponential | `Connecting` resolving `Err(ConnectError::TimedOut)` — slice 3. **S2 is not closed by this slice.** |

### 9.5 The tests in one place

Sections 5.3, 6.5, 7.2–7.5 and 8.2–8.6 each list theirs. Two more that
belong to no single section:

- `the_drain_always_terminates_in_timeout` — after every mutating call,
  in a loop over a scripted sequence. §16.4's contract in one assertion.
- `accept_on_a_live_static_returns_stale_and_installs_nothing` — §1.4's
  boundary, pinned so the LIVE-row gap is visible rather than latent, and
  so slice 7 has a test to *change* rather than a behaviour to discover.

## 10. The gate table and what in this slice risks one

| Gate | Command | This slice's risk |
|---|---|---|
| Compiles | `cargo build --all-features --all-targets` | **medium** — GATs on `Handshake`; the `mod core` vs `::core` ambiguity (§2.5); the `channel!` expansion must compile in a *consumer* crate too, and the only thing that proves it is the doctest in `channel!`'s rustdoc, which already exists and must keep passing |
| Format | `cargo fmt --all --check` | low |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` | **high** — `type_complexity` on the provider/private-key paths (mitigated by §2.2's three aliases), `large_enum_variant` on `ChainState` (mitigated by boxing), `too_many_arguments` on the attempt builder, `missing_docs` on every new `pub` item (`lib.rs:98` sets `#![warn(missing_docs)]`, and `-D warnings` promotes it) |
| Docs | `cargo doc --no-deps` / `--all-features`, `RUSTDOCFLAGS=-D warnings` | medium — new intra-doc links into `hiss` and across `core`/`packet`; a `pub(crate)` `core` cannot be linked from a `pub` item's docs, which is a broken-link error, not a warning |
| Tests | `cargo test` **and** `cargo test --all-features` | the slice's own bar |
| Wire pins | golden-wire + size/constant tests | **the one to watch** — see below |
| MSRV | `cargo +1.96 check --all-features --all-targets` | low; GATs stable since 1.65, edition 2024 already in use |
| Supply chain | `cargo deny check` | **none** — this slice adds **no dependency**. `rand_chacha`, `rand_core`, `getrandom`, `hiss`, `cryptoxide` are all already in `Cargo.toml` |

### 10.1 The wire-pin risk, stated precisely

This slice **modifies `src/packet/suite.rs`** — `channel!` gains an
`impl Handshake for $name`. That file is covered by
`tests/spec_packet.rs` and `src/packet/tests.rs`, neither of which this
slice may modify. The change is **purely additive** (a new `impl` block;
no existing item touched, no constant moved), so no wire byte moves and
no golden vector changes.

**If any golden-wire or size/constant test goes red, that is a ruling
request, not an expectation to update.** Two specific ways it could
legitimately go red, both of which mean *stop and report*:

1. The first real msg1/msg2 bytes this crate has ever produced disagree
   with the frozen golden vectors — that is a genuine finding about the
   vectors or about the framing, and it must be reported, not
   accommodated.
2. `IK::MSG1_SIZE` vs `Channel::MSG1_LEN` — already a `const _: () =
   assert!(…)` inside `channel!` (`suite.rs:293–300`), so a hiss change
   breaks the **build**, in every suite, not one test. That guard stays.

### 10.2 One process note

Working rule 7: no gate is reported green without the command and its
output pasted. This plan runs none of them — it produces no code. The
implementing slice ends on the **full table**, not on `cargo test`.

## 11. Open questions

### (a) For the briefing agent — decisions I made and would like checked

Each of these I resolved by derivation and stated the derivation so it
can be objected to. None of them needs a ruling in my judgement; all of
them would be cheaper to correct now than after the code exists.

1. **`core` is `pub(crate)` this slice** (§2.1), so the tests live in
   `src/core/tests.rs` following slice 1's precedent. §16.4 presents
   `core::Endpoint` as though public, and §16.6 says a build accepting a
   caller-chosen RNG seed must be feature-gated or documented as such —
   deferring publication defers that. Is deferring right, or does slice
   3 need `core` public from day one?
2. **The arrival order dedup → per-source cap → global cap** (§6.2),
   with the derivations for each ordering. §6.3 states three rules and
   never their order.
3. **Global all-consumed ⇒ drop** (§6.2 step 3). §6.3 states the
   per-source all-consumed drop and not the global one; dropping is the
   only option that respects §17.5's ceiling.
4. **`Timestamp::succ()` = +1 nanosecond** (§7.5). §5.3 says "forced
   strictly greater" and never names the unit.
5. **F-5 — cancellation reuses `ToEndpoint::Retired`** (§8.4) rather
   than adding a verb §16.4 does not list. S29 needs the core to drop a
   pending; `Retired`'s documented effects are exactly the right ones.
   Confirm the reading, or tell me to propose a verb.
6. **The `Handshake` supertrait, not an extension of `Channel`** (§4.2).
   It keeps slice 1's frozen `Channel` and its tests untouched.
7. **`Identity::open()` mints a (provider, static-key) pair per
   handshake** (§3.1) — forced by hiss consuming both by value and by
   `PrivateKey` not being `Clone`. And `DhProviderAsync` is
   **deliberately excluded**, for the three reasons in §3.3.
8. **Two module-map deviations**: `src/identity.rs` and `src/config.rs`
   are new files `PLAN.md`'s map has no home for; `routing.rs` is not
   created (§6.5–6.8 is slice 7).
9. **Slice 2a's `accept()` implements §5.4's NONE row only**, returning
   `AcceptError::Stale` for LIVE and PENDING (§1.4). This preserves
   §16.1's one-session-per-static invariant but is knowingly incomplete
   for LIVE. Test-pinned so it is visible.
10. **S2 is not closed by this slice** and S8/S10/S21/S29 are closed only
    in half (§9.4). If the slice is expected to close S2, the scope is
    wrong, not the plan.

### (b) Needing a maintainer ruling

**R-1 (F-2) — "oldest by park time" versus "15 s after last refresh".**
§6.3 uses two age notions for one entry and never relates them:
`INTRO_TTL` runs from the entry's **last refresh** (1144), replacement
"refreshes the deadline" (1159), while overflow evicts "the oldest
unconsumed entry (**by park time**)" (1185–1186) and the per-source cap
evicts "that IP's oldest **unconsumed** entry" (1174–1175) with no age
notion named at all.

If *park time* means the original park, then an entry refreshed nine
times is the eviction victim while holding the freshest bytes in the
queue — and the entry that keeps being refreshed is precisely the genuine
retransmitting peer, which contradicts the stated purpose of
evict-oldest: "a genuine initiation always obtains a slot … an attacker
must win a per-packet race against the genuine peer's ~5 s retransmit".
If it means last refresh, the two notions coincide and the section reads
consistently.

**I am not picking one.** Working rule 3, and the prose here is the half
that argues a purpose while the formal wording is the half that names a
field. Note the shape: *a stated construction with an unstated scope* —
"oldest" is used without saying which clock measures it, exactly as
ruling 68's `TAG` was used without saying what varies it. Until ruled,
the implementation should carry **one** helper (`fn age_key(&self) ->
Instant`) so the choice is a single line and its two readings are one
edit apart.

**R-2 — the timestamp-guard orphan aging has no constant.** §17.1
mitigation (ii) and line 1593 both say orphans age out on an
"`INTRO_TTL`-scale timer". The Named-constants appendix (5725) lists only
`TS_GUARD_ORPHAN_CAP`; `constants.rs` likewise. An implementer cannot
avoid choosing a number, and a number chosen silently becomes behaviour
nobody ratified. Proposal: name it `TS_GUARD_ORPHAN_TTL` and ratify it at
`INTRO_TTL` (15 s), or state explicitly that it is a `Config` knob with
no ratified default.

**R-3 (F-4) — §16.4's core API cannot serve §6.2's `sender_index()`.**
§6.2 gives `Intro` two 0-DH accessors, `source()` and `sender_index()`.
§6.3 (1203) requires that while an entry is unconsumed, "accessors
reflect the **newest bytes at call time**" — and every retransmit carries
**a new random index** (§5.5). So `sender_index()` cannot be a value the
shell cached when the `Intro` surfaced; it must read through to the core
at call time. §16.4's endpoint API lists no such accessor:
`IntroReady(IntroId, SocketAddr)` carries the address (so `source()` is
fine, and unchanged by replacement since the address *is* the dedup key),
but nothing carries the index.

Either §16.4's list is elided — in which case the core simply gains
`fn sender_index(&self, id: IntroId) -> Result<u32, IntroError>` and this
is a one-line clarification — or `sender_index()` is meant to be a
snapshot, in which case §6.3's "newest bytes at call time" needs its
scope narrowed to say which accessors it covers. The two readings differ
observably, so I am reporting rather than choosing. Same shape as R-1.

**R-4 — the LIVE row and slice ordering.** Not a spec defect: a check on
whether §1.4's boundary is acceptable, i.e. that an `accept()` of a
genuine replacement for a LIVE static returns `AcceptError::Stale`
between this slice and slice 7. The alternative is pulling §6.4's
admission (guard + `replacement_basis` comparison, both of whose state
this slice already builds) forward into slice 2a — perhaps 60 lines, but
it drags §6.4's re-home, its ordering clause, and the `Replaced` teardown
with it, and the teardown needs `core::Connection`. My reading is that
the boundary is right and the gap should be test-pinned; confirming it is
a maintainer's call because it is the one place where slice 2a is
knowingly not spec-complete.

---

## Appendix S — spec notes, recorded as read

*(Written incrementally so partial progress survives. Line numbers are
`SPEC.md` line numbers at the time of reading.)*

### S-1. §16.4 (4322–4499) — the poll contract

Endpoint core signature, verbatim (4337–4352):

```rust
impl<I: Identity> core::Endpoint<I> {
    fn new(now: Instant, config: Config, identity: I, rng_seed: [u8; 32]) -> Self;
    fn connect(&mut self, now: Instant, remote: SocketAddr, remote_static: PublicKey)
        -> Result<(ConnectionId, core::Connection), ConnectError>;
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Disposition;
    fn handle_timeout(&mut self, now: Instant);                        // idempotent
    fn handle_connection_event(&mut self, id: ConnectionId, ev: ToEndpoint);
    fn poll_output(&mut self) -> EndpointOutput;                       // drain to Timeout
    // staged verbs (§6.2), by IntroId:
    fn read_identity(&mut self, id: IntroId) -> Result<PublicKey, IntroError>;
    fn authenticate(&mut self, now: Instant, id: IntroId)
        -> Result<(PublicKey, Timestamp), AuthError>;
    fn accept(&mut self, now: Instant, id: IntroId)
        -> Result<(ConnectionId, core::Connection), AcceptError>;
    fn reject(&mut self, id: IntroId);
}

enum Disposition { ForConnection(ConnectionId), Done }

enum EndpointOutput {
    Transmit(Transmit),                          // msg1/msg2, retransmits, tie-break msg2
    IntroReady(IntroId, SocketAddr),
    ToConnection(ConnectionId, Install),
    HandshakeFailed(ConnectionId, ConnectError), // shell-only
    Timeout(Option<Instant>),                    // terminal
}
struct Install { session: EstablishedSession }
struct EstablishedSession { /* hiss transport pair (seal + open), our session
                              index, peer's index, anchor address (§5.6) */ }
struct Transmit { to: SocketAddr, data: Vec<u8> }
```

Load-bearing facts for this slice:

- The staged verbs **are synchronous and take `IntroId`** — confirms the
  brief's narrowing. Nothing in the endpoint core is `async`.
- `Endpoint` is generic over `I: Identity`, and the spec states *why*:
  "the mid-state map is typed over `I::Provider`" (4333–4334). That is
  the §6.1 suspended msg1 mid-state (`hiss`'s `…Msg1Intro`), which is
  parameterised by the provider type. So `Identity` must expose an
  associated `Provider` type, and the stage-0 queue's entries are typed
  over it. **This is the sentence that forces `Identity` into slice 2a**
  rather than slice 3.
- `new` takes `rng_seed: [u8; 32]` — the core owns a deterministic,
  seeded RNG, never a thread RNG. That is what makes the core testable
  synchronously and reproducibly (§16.6 to be confirmed).
- `HandshakeFailed(ConnectionId, ConnectError)` is emitted **by the
  endpoint core** but "never reaches `core::Connection`" (4483–4486) —
  it is a drain output the shell consumes. So the endpoint core owns
  give-up (§5.7) for `connect()`-created connections, and the pending
  connection's core is simply dropped by the shell.
- `ToConnection(ConnectionId, Install)` is emitted **exactly once** per
  `connect()`-created connection (4416–4425); `accept()` returns an
  already-established connection and gets **no** `Install`.
- `Timeout(Option<Instant>)` is terminal and is the drain sentinel; every
  mutating call must be followed by a drain. Output ordering within a
  drain **preserves generation order** and is normative (4496–4498).
- `Disposition { ForConnection(ConnectionId), Done }` is the
  `handle_datagram` return: the endpoint tells the driver which
  connection core to feed next. In slice 2a the `ForConnection` arm is
  reachable (an established connection's data packet) but the connection
  core is slice 4+; see §1 for how this slice terminates that path.
- `ToEndpoint::Retired { our_index: u32 }` is the only
  connection→endpoint event, and delivering it is a MUST before the
  shell releases connection bookkeeping (4487–4493) — it drops the index
  route **and** the guard-entry pin. The guard-entry pin is §17.1 and is
  in this slice; the emitter is not.

### S-2. §6.1 (1033–1112) — the typestate and its DH costs

Ladder (cumulative responder cost), from the table at 1038–1043:

| Stage | Cumulative | Adds | Visible | Automatic rejects |
|---|---|---|---|---|
| `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed hash, **0 DH** | — | source address, `sender_index` | wrong length (§3.1, exact for handshakes), unknown type/version, bad mac1 — **all silent, before the queue** |
| `read_identity()` → `Claimed` | **1 DH** | `es` | the *claimed* static | structurally unreadable msg1 → `Malformed` |
| `authenticate()` → `Proven` | **2 DH** | `ss` | possession proven; the initiation timestamp | tail-tag failure → `HandshakeFailed`; timestamp replay → `Replay` (guard is **not** policy) |
| `accept()` → `Connection` | **4 DH** | `ee`, `se`; msg2 sent | an established connection | — |

- The dagger note (1045–1052) is the one that shapes the data structure:
  a **hinted-source** entry may arrive **pre-read** (§6.5 step 3) — the
  1 DH was charged once, eagerly — and `read_identity()` then returns
  the **cached** claimed static at **0 incremental DH**. Same for every
  **frozen** mid-state-carrying entry (eager-demoted, §6.3). *The
  cumulative table is unchanged either way.* → the queue entry must be
  able to hold a **cached stage result**, and the DH-cost test must
  assert *cumulative* cost, not per-call cost.
- A **re-homed** `accept()` (§6.4) adds the admitted candidate's `es` +
  `ss` **on top of** the 4 (1054–1056). §6.4 is slice 7 — so the
  4-DH pin in this slice is for the **fast path** only, and the test
  must be named accordingly so slice 7 does not have to fight it.
- **Nothing durable may be keyed on the claimed static, the source
  address, or `sender_index`** (1062–1064) — no map insertion, no
  rate-limit bucket, no unbounded logging. This is a *code review
  criterion for this slice*, because the stage-0 queue is exactly the
  structure that would be tempted to key on source address. §6.3's
  per-source cap is a bounded counter over a bounded queue, not a
  durable map — that distinction must be preserved by construction.
- slither keeps **no** ban list / deny list / reputation store
  (1069–1078, ruling 48). Nothing in this slice may add one.
- Dropping the object at any stage is a **silent reject**: no msg2,
  nothing transmitted, slot freed. `reject(id)` therefore emits **no**
  `Transmit` — testable directly (drain to `Timeout` yields nothing
  else).

### S-3. §6.3 (1138–1277) — the stage-0 queue *(densest sub-section)*

Constants (1140–1144):

| Constant | Value | Notes |
|---|---|---|
| `INTRO_QUEUE_CAP` | **1024** slots, endpoint-wide | configurable in `Config` |
| `INTRO_MAX_PER_SOURCE` | **4** chains per source **IP** (per **/64** for IPv6) — the **sum of unconsumed stage-0 entries and consumed chains** | configurable |
| `INTRO_TTL` | **15 s** after the entry's **last refresh** | ≈ 3 retransmit intervals |

Entry contents: raw **196-byte msg1** + source address (≈ 220 B/entry;
≈ 225 KB worst case at cap) — *plus* the one bounded exception, an
eager-demoted entry carrying its already-paid mid-state (§6.5 step 3).
Mid-state ≈ 0.5–1 KB, holds the endpoint's static provider and the
`es`-derived keys, keyed by `IntroId` inside the endpoint core, bounded
by cap × TTL (§17.5).

Rules, each with the shape the implementation must take:

1. **Dedup key = the full source `SocketAddr` alone; replace-with-newest;
   replacement refreshes the deadline** (1158–1171). Supersedes any
   `(addr, sender_index)` key — every retransmit is a fresh initiation
   with a new random index (§5.5), so an index-bearing key never matches
   across retransmits. Distinct initiators behind one NAT have distinct
   ports → distinct keys; a same-4-tuple collision is a rebind of the
   same flow, newest-wins correct.
2. **Per-source cap** (1172–1184), keyed on source **IP** (/64 v6) —
   *note this is a **different key** from the dedup key.* Counts
   **unconsumed + consumed** for that IP. An arrival that would exceed
   the cap replaces **that IP's oldest unconsumed** entry; eviction
   operates on the **unconsumed tier only**. Consumed chains are
   non-evictable (DH-paid): **app-held** after `read_identity()`, or
   **endpoint-frozen** for a mid-state-carrying entry. If a source's
   whole allowance is consumed chains, **the arrival is dropped**.
   Frozen carried entries are counted under the cap and expire at
   `INTRO_TTL` like any entry. `read_identity()` is **net-zero** for the
   count (−1 unconsumed, +1 consumed).
3. **Global overflow: evict-oldest unconsumed by park time** (1185–1191).
   A genuine initiation always obtains a slot, except the per-source
   all-consumed drop. **Consumed chains are never evicted by overflow.**
4. **Expiry** (1192–1197): silent eviction at `INTRO_TTL`. Staged verbs
   on an expired attempt return `IntroError::Expired`
   (`AuthError::Expired` at that stage). A **consumed chain's mid-state
   expires 15 s after the initiation that fed it** — i.e. measured from
   the initiation, not from consumption. **`accept()` alone is exempt**:
   under §6.4's re-home, a chain's age never fails `accept()`; only the
   absence of any parked initiation does. *(→ see finding F-1: this is
   the one place the core is not self-contained without §6.4.)*
5. **Own-bytes-on-consume** (1199–1209). An entry is **unconsumed until
   `read_identity()`** — and only if it carries no mid-state. While
   unconsumed, a newer initiation from the same source **transparently
   replaces its bytes and refreshes its TTL**: *same `IntroId`*, newest
   bytes, **accessors reflect the newest bytes at call time**, and **no
   second surfacing** (no second `IntroReady`). At `read_identity()` the
   chain **owns** its bytes and its `IntroId`: the source's stage-0 slot
   is freed, a subsequent initiation parks as a **new** entry, and a
   consumed `Claimed`/`Proven` chain can **never** be superseded.
6. **Freeze-on-carry** (1211–1224). Any entry carrying a paid mid-state
   (eager-demoted, §6.5 step 3) is **consumed from the moment it parks**:
   bytes and `IntroId` frozen, later same-source initiation parks as a
   *new* entry subject to the per-source cap, staged accessors never
   straddle two initiations. Rationale: an unauthenticated mac1-valid
   packet must never byte-replace an entry whose cached mid-state claims
   an identity. The minimal-state alternative is **explicitly declined**
   and "Appendix B's DH-cost pins assume it".
7. **`Superseded` appears in no error enum** (1226–1228, ratified) —
   under own-bytes-on-consume no verb can observe supersession.

Honesty clause (1235–1276) is rationale, not behaviour, but it pins two
numbers a test can assert indirectly: filling the queue needs ≥ 256
distinct sources (1024 / 4), and sustained full occupancy costs
≈ 68 packets/second (1024 / 15 s).

**F-1 (finding — the §6.4 dependency).** Rule 4's `accept()` exemption is
stated *in terms of* §6.4's re-home. The core cannot implement `accept()`
coherently from §6.3 alone: it must know whether an expired chain's
`accept()` re-homes onto a fresher parked initiation or fails. Slice 7
owns §6.4. See §1 for how this slice bounds that.

**F-2 (candidate defect — a stated construction with unstated scope).**
Two different age notions are used for the same entry and their relation
is never stated:
- `INTRO_TTL` is "15 s after the entry's **last refresh**" (1144), and
  replacement "refreshes the deadline" (1159).
- Overflow evicts "the oldest unconsumed entry (**by park time**)"
  (1185–1186), and the per-source cap evicts "that IP's oldest
  **unconsumed** entry" (1174–1175) with no tiebreak notion named at all.

If *park time* means original park, an entry refreshed nine times is
still the eviction victim under overflow while being the freshest bytes
in the queue — which contradicts the stated purpose of evict-oldest
("a genuine initiation always obtains a slot … an attacker must win a
per-packet race against the genuine peer's ~5 s retransmit"): the genuine
retransmitter is precisely the entry that keeps being refreshed. If park
time means last refresh, the two notions coincide and the rule reads
consistently. **The prose intent and the formal wording disagree; I am
not picking one.** See §11(b).

**F-3 (smaller, same shape).** The per-source cap's eviction victim is
"that IP's oldest **unconsumed** entry" — but the dedup key is the full
`SocketAddr` while the cap key is the IP (/64). The **order of
operations** on arrival (dedup-replace vs cap-check vs global
evict-oldest) is never stated, and the orders are not equivalent: an
arrival whose `SocketAddr` matches an existing unconsumed entry must
dedup-replace (net-zero for the cap), never trip the cap. See §11(a) —
I believe this one is derivable rather than needing a ruling, and §2/§6
state the derived order explicitly so a reviewer can object to it.

### S-4. §5.5–5.7 (874–1017) — initiator, responder, timers

**§5.5 initiator, six numbered rules** — all six are endpoint-core state:

1. Draw a **random nonzero `sender_index`** (re-draw rule §17.3) and a
   **fresh strictly-greater timestamp**; build msg1 over a **fresh
   ephemeral** with the 12-byte payload (§5.2); append mac1; send to the
   dialled address.
2. Arm a retransmit at `RETRANSMIT_BASE` + uniform jitter ≤
   `RETRANSMIT_JITTER_MAX` = **5 s + U[0, 333 ms]**. **Every retransmit
   is a completely fresh initiation** — new ephemeral, new random index,
   new strictly-greater timestamp. Interval is **fixed, not
   exponential**.
3. **One completion attempt per retransmit interval.** The attempt is
   *taken* on the first **length-correct, index-matching, mac1-valid**
   msg2; a second msg2 in the same interval is **dropped**. A failed
   completion (bad crypto) **spends** the attempt — the next scheduled
   retransmit refreshes it. So a guessed-index or mac1-invalid msg2 can
   never spend anything.
4. **The msg2 source address is deliberately ignored** — completion
   requires an **index** match, not an address match. The initiator
   anchors the session at the **dialled** address; the peer roams in on
   its first authenticated data packet (§7.3).
5. On completion: our receiver index = our `sender_index`; the peer's =
   the response's `sender_index`.
6. **Give up at `HANDSHAKE_GIVEUP` (90 s)** → `Connecting` resolves
   `Err(ConnectError::TimedOut)` — *the only handshake failure the
   application ever sees*. Every initiation is a `connect()`; there are
   no internal rekey initiations (§5.4). The train also ends **early** if
   the application drops the `Connecting` (§16.3, ruling 50) — that half
   is shell-side (slice 3); the core needs a cancel path for it.

**§5.6 responder** (908–925): the staged ladder mac1 (0) → `es` (1,
recovers the *claimed* static) → `ss` (1, proves possession **and
decrypts the timestamp**) → `ee`, `se` (+2, msg2), driven by the
application through the staged accept, with **one internal exception**:
the simultaneous-open tie-break (§6.6–§6.7, slice 7). The per-static
greatest-timestamp guard (strictly greater, else drop — §17.1) admits at
`authenticate()` on the staged path and at the tie-break's admit step,
**in both cases post-`ss`, so only key-holders can write guard entries**.
The responder anchors an accepted session at the **msg1 source address**,
which arms §7.3's amplification budget (§7.3 is not this slice).

**§5.7 timers** — the three this slice needs:
`RETRANSMIT_BASE` + jitter = **5 s + U[0, 333 ms]**;
`HANDSHAKE_GIVEUP` = **90 s**. (`KEEPALIVE_TIMEOUT` 10 s, `DEAD_TIMEOUT`
25 s, `PERSISTENT_KEEPALIVE` 10 s default / **[1 s, `DEAD_TIMEOUT`)** are
connection-core timers — not this slice.)
Also normative here and worth carrying: **there is no DH-rekey trigger at
all** (1014–1017) — the epoch ratchet is the only rekey. So the endpoint
core has exactly **one** initiation source: `connect()`.

### S-5. §17.1–17.4 (4746–4945) — guard and tables

**§17.1 the timestamp guard.** Per-remote-static greatest initiation
timestamp (§5.3). Four write sites, **all post-`ss`, so only key-holders
write guard entries**:

| Write site | In slice 2a? |
|---|---|
| `authenticate()` on the staged path — check **and** record | **yes** |
| a re-homed `accept()`'s candidate admission (§6.4) | no — slice 7 |
| the internal tie-break's admit step (§6.6 step 4) | no — slice 7 |
| the tie-break **winner**'s side: record **without admitting** (§6.7) | no — slice 7 |

- **Revert is part of the write.** "A record made for a candidate whose
  `accept()` then returns `AcceptError::Stale` is **reverted**", and
  mitigation **(i) no-orphan-on-reject**: a static authenticated and then
  rejected without ever being accepted **writes no orphan** — *its record
  drops with the chain; a pre-existing entry reverts*. → the
  `authenticate()` write is **provisional**: the chain must remember the
  previous value (or its absence) so `reject()`/drop/expiry can restore
  it. **This is core state in slice 2a and is easy to miss.**
- **Pinning**: an entry is pinned (never evicted) while a live
  `Connection`, an **in-flight outbound pending**, or a **staged
  mid-state** exists for its static. For a staged mid-state (static
  merely *claimed*) the pin **never creates an entry** — a bounded
  exception to §6.1's nothing-durable rule: it flips a bit on an entry a
  key-holder already wrote, and **reverts on drop**.
- **The pin outlives its connection by `HANDSHAKE_GIVEUP` (90 s)** — but
  only for entries written by the tie-break admit step or a winner-side
  record (§6.6/§6.7). Not reachable in slice 2a; the mechanism still has
  to exist in the data structure, so the entry carries an
  `exempt_until: Option<Instant>` from the start or slice 7 rewrites it.
- Orphans (dead connections) live in a bounded LRU with timer aging:
  `TS_GUARD_ORPHAN_CAP` = **1024** entries (≈ 45 B each).
- Mitigation **(iii) LRU "use" is admission only** — recency refreshes on
  a **successful post-`ss` record**, never on a failed check.
- Mitigation **(ii) timer aging** — orphans age out on an
  "`INTRO_TTL`-scale timer" (also worded that way at 1593). **No named
  constant exists**: the Named-constants appendix (5725) lists only
  `TS_GUARD_ORPHAN_CAP`. See §11(b) — the implementation cannot avoid
  choosing a number.
- The guard alone does not make §6.4's replacement safe; the **pair** is
  guard + `replacement_basis` (§17.4). For a static we **only ever
  dialled we hold no entry at all** — every write site is a post-`ss`
  read of an *inbound* msg1, and a `connect()` completed by msg2 writes
  nothing. Directly relevant to slice 2a: **the initiator path never
  touches the guard.**

**§17.2 `last_init_timestamp`** (4869–4873): the **endpoint-global**
outbound monotonic forcing (§5.3). Survives across connection generations
to the same peer — close-and-reconnect still emits strictly greater.

**§17.3 the index tables** (4875–4888): `index → connection` (sessions)
and `pending-index → connection` (in-flight initiations). **Index minting
draws a random nonzero `u32` and re-draws while the value is present in
*either* table**, from the endpoint RNG, off-path-unpredictable by
requirement (§16.6) — required because a pending's msg1 index graduates
into the session index on completion; this closes route-stealing.
Corollary: **a datagram that routes by index but fails to open touches
nothing** — not liveness, not roaming, not the replay window.

**§17.4 the static map and the hint set** (4890–4945): `static →
connection` (§16.1's one-session-per-peer invariant made concrete), plus
the pending tables' **dialled addresses**, which *are* §6.5's hint set —
the probed set is **the pending outbound remotes alone**. Established
connections contribute **no** hints (their initiations take the ordinary
staged path, §5.4; the endpoint tracks no per-connection address). Each
`static → connection` entry carries `replacement_basis: Option<Timestamp>`
— `Some(t)` when **we responded** (staged `accept()`, re-homed candidate,
tie-break loser's admit), `None` when **we dialled**. Written **once at
install, never updated**, dies with the connection. **§6.4's admission is
the only reader** — so slice 2a *writes* the basis at `accept()`/install
and never reads it.

### S-6. §5.3–5.4 (798–872) — timestamp and structural restart

- **§5.3**: the timestamp is the wall clock (`secs ‖ nanos`), **the one
  wall-clock read in the protocol** (§16.5), forced **strictly greater**
  than the previous timestamp *this endpoint* emitted — endpoint-global,
  across all connections and connection generations (§17.2). Stated
  reason: a retransmit stays admissible when a coarse clock has not
  advanced, and close-and-reconnect still emits strictly greater.
  Confidentiality: Noise level 2, opaque to a passive observer (no
  clock-skew fingerprinting), **not** forward-secret against a later
  compromise of the responder's static — acceptable, it is a clock
  reading.
- **§5.4** (ratchet-only): there is **no periodic DH re-handshake**; the
  §7.7 epoch ratchet is the only rekey. Every completed handshake
  establishes a **fresh connection with fresh transport state on both
  sides** — sessions and connections are 1:1, and **no** stream,
  flow-control, recovery or congestion state crosses a handshake. So the
  endpoint core has exactly **one** initiation source: `connect()`.
- **The responder rule**, consulted **post-`ss`**, is a three-valued
  local state for the proven static: **LIVE** (established connection
  exists) / **PENDING** (in-flight outbound initiation and no established
  connection) / **NONE**. §16.1 forbids both at once. Slice 2a implements
  NONE; LIVE (candidate replacement, §6.4) and PENDING (tie-break,
  §6.6–6.7) are slice 7 — see §1.4.

### S-7. §16.5, §16.6, §17.5 — the endpoint's timers, RNG and ceilings

- §16.5 (4523–4526), **verbatim and load-bearing**: "The endpoint core's
  deadline is the min over its pendings' retransmit/give-up deadlines,
  the parked intros' expiries, and the timestamp-guard orphan aging
  (§17.1)." That sentence is the complete list of endpoint-core timers.
- §16.5 (4502–4505): the initiation timestamp is "the one wall-clock
  read, behind a **clock service injected in the endpoint config**" —
  this is what puts a clock seam in `Config`.
- §16.5 (4529–4537): `handle_timeout` is **idempotent** (each due timer
  is stopped before its logic runs); equal-deadline priority is
  **normative** and the endpoint-relevant clause is **"give-up beats a
  same-instant retransmit"**.
- §16.5's lateness bound `L` = 250 ms is "a conformance parameter of the
  **shell**, not of the protocol; the cores expose exact deadlines" — so
  it is slice 3's, not this slice's.
- §16.6: one seeded RNG in the endpoint core (`[u8; 32]`), source of
  **every index, jitter draw, and — via the forced increment — timestamp
  draw**. At connection creation the endpoint draws a 32-byte
  **sub-seed** for the connection core, **drawn even while unused**, so
  later connection-side randomness cannot perturb the endpoint's draw
  order. Indices MUST be unpredictable off-path (load-bearing for §5.5's
  on-path-only completion spend and §15.2's linger reply), so the
  config-supplied seed is a **test-only facility** and a build accepting
  one is "security-relevant and must be feature-gated or documented as
  such".
- §17.5 ceilings relevant here: stage-0 entries + consumed chains share
  **one** budget of 1024 slots (≈ 220 B each); staged mid-states ≤ 1024
  at ≈ 0.5–1 KB each — "and **each holds the endpoint's static
  provider**: for a hardware/enclave static this is up to 1024 concurrent
  provider handles, an operationally scarce resource the TTL bounds in
  time". That clause is why §5.2 builds the responder state machine
  lazily at `read_identity()` rather than at park.

### S-8. hiss 0.3.2 / hiss-macros 0.3.1 — the API as built

Read from source, not from the delivery report:

| Item | Location | Shape |
|---|---|---|
| entry constructors | `hiss-macros-0.3.1/src/codegen.rs:582–606` | `IK::initiator<CP>(provider: CP, prologue: &[u8], remote_static) -> IKInitiatorMsg1<CP>`; `IK::responder<CP>(provider, prologue, static_key: CP::PrivateKey) -> Result<IKResponderMsg1<CP>, HandshakeError>` — both bounded `CP: DhProvider<Curve>` |
| state naming | `codegen.rs:129` | `{Pattern}{Role}Msg{n}` |
| mid-state naming | `codegen.rs:1382` | `{Pattern}{Role}Msg{n}Intro` → `IKResponderMsg1Intro<CP>` |
| the staged pair | `hiss-0.3.2/tests/noise_macro_shapes.rs:714–727` | `let (claimed, mid) = hs.read_message_1_intro(&msg1)?;` `mid.claimed_static()`; `mid.complete()?` → `(payload, next)` |
| `Transport` | `codegen.rs:814` | `hiss::noise::Transport<IK>` — generic over the **pattern**, not the provider |
| datagram split | `hiss-0.3.2/src/noise/datagram.rs:96,133` | `into_datagram()` / `into_datagram_with_epoch()` → `(DatagramSend, DatagramRecv)` |
| `DhProvider` | `src/provider/mod.rs:187` | **no `Send` bound**; hiss's own doc: "It is what the state machines `noise!` generates are generic over" |
| `DhProviderAsync` | `src/provider/mod.rs:213` | `dh_async` returns `impl Future + **Send**` |
| `PrivateKey` | `src/provider/mod.rs:118–125` | `type PrivateKey: Send;` — **deliberately not `Clone`** |
| software key import | `src/curve/p256/software.rs:44,54` | `P256r1PrivateKey::from_bytes([u8;32])` / `to_bytes()` |
| `EphemeralOnly` | `src/provider/mod.rs:492–496` | `EphemeralOnly::new(rng)`; `#[derive(Clone)]`, so `Clone`/`Send`/`Sync` are inherited from `R` |

hiss's own tests already pin the ladder slither depends on
(`noise_macro_shapes.rs`): `dhs == 1` after `intro`, `dhs == 2` after
`complete()`, and `staged_reject_by_drop_costs_exactly_one_dh`. Appendix
A.1 says so too ("the DH ladder is pinned by test, not merely
documented"). slither's own pins are therefore about *slither's*
accounting — that it calls each stage exactly once, and never eagerly.

### S-9. Findings raised elsewhere in this document

- **F-1** — `accept()`'s expiry exemption is stated in terms of §6.4
  (slice 7). Resolution in §1.4.
- **F-2** — "oldest by park time" vs "15 s after last refresh". → R-1.
- **F-3** — the arrival algorithm's order of operations is unstated. →
  §6.2, and §11(a) item 2.
- **F-4** — §6.2's `sender_index()` accessor has no source in §16.4's
  core API, and §6.3 requires it to be live rather than cached. → R-3.
- **F-5** — S29's cancellation needs a core-side effect §16.4 lists no
  verb for. → §8.4, and §11(a) item 5.

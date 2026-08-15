# Slice 3b — implementation log (IMPL-B)

Status: **COMPLETE** — all eight gates green on the working tree (not committed).

## 0. Reading log (append-only)

- `CLAUDE.md` — full.
- `.slices/03-skeleton/CORRECTIONS.md` — full. Slice 3b's `tests/` files are
  reachable because `pub mod shell`.
- `.slices/03-skeleton/HISS-API.md` — full. Key facts for 3b: `DatagramSend`
  has `session_id(&self) -> &SessionId`; `SessionId: Clone`; the datagram
  halves are `Send` by auto-derive but the *handshake* state is not, which is
  why the actor stays `!Send`.
- `.slices/03-skeleton/PLAN.md` §0.4 (build order), §2.2 (file ownership),
  §4.1–4.6 (spec digest of §16.1/16.2/16.3/16.8/16.9/16.10), §5.2 (story
  map), §6 U6–U10.

### Carried facts from the plan digest

- §16.2 `close()` "resolves once the CLOSE frame is sealed and the closing
  state is entered (§15.2), and truncates `reason` at `CLOSE_REASON_MAX`".
- §16.2 `closed()` (ruling 46) has **four** testable properties: latched
  (not one-shot), concurrent from any number of tasks, cancel-safe, and
  resolves immediately and for ever after death.
- §16.3 ruling 62: a `Connecting` **is** a handle (keeps the driver alive);
  a `closed()` future is **not**.
- §16.3 ruling 50: dropping a `Connecting` cancels; the MUST is that the
  cancellation is ordered ahead of any endpoint verb issued after the drop
  returns.
- §16.8: driver→handle deliveries are bounded/non-blocking; accessors are
  synchronous reads of a shared cell; the driver never blocks toward a
  handle.
- §16.10: the three contract names are `testutil::Network`,
  `testutil::FlakyWire`, `testutil::FlakyPolicy` — the new harness must not
  shadow them, and may otherwise be named freely.
- U10 provisional (slice 3a's to implement): entering closing/draining
  disarms every timer but `CloseLinger`; `Closed(_)` fires exactly once.

### Core surface I must drive (read from the code, not the plan)

`core::Endpoint<I: Identity>` — all `pub(crate)`:

```
Endpoint::new(now, config, identity, rng_seed: [u8;32]) -> Self
poll_output(&mut self) -> EndpointOutput<I::Suite>          // terminal Timeout(Option<Instant>)
connect(&mut self, now, remote, remote_static) -> Result<(ConnectionId, Connection<Suite>), ConnectError>
handle_datagram(&mut self, now, src, datagram) -> Disposition
handle_timeout(&mut self, now)
handle_connection_event(&mut self, now, id: ConnectionId, ToEndpoint)
intro_source(&self, IntroId) -> Option<SocketAddr>
intro_sender_index(&self, IntroId) -> Option<u32>
read_identity(&mut self, IntroId) -> Result<PublicKeyOf<I>, IntroError>
authenticate(&mut self, now, IntroId) -> Result<(PublicKeyOf<I>, Timestamp), AuthError>
accept(&mut self, now, IntroId) -> Result<(ConnectionId, Connection<Suite>), AcceptError>
reject(&mut self, now, IntroId)
```

`core::Connection<C: Handshake>` — all `pub(crate)`:

```
Connection::connecting(sub_seed) / ::established(now, sub_seed, session)
poll_output(&mut self) -> ConnOutput                        // terminal Timeout(Option<Instant>)
handle_endpoint_event(&mut self, now, Install<C>)
handle_datagram(&mut self, now, src, datagram)
handle_timeout(&mut self, now)
close(&mut self, now, code, reason)
is_established / session / replay / liveness / next_counter / timer / sub_seed
```

`EndpointOutput`: `Transmit | IntroReady(IntroId, SocketAddr) | ToConnection(ConnectionId, Install) | HandshakeFailed(ConnectionId, ConnectError) | Timeout(Option<Instant>)`.
`ConnOutput`: `Transmit | Event(ConnEvent) | ToEndpoint(ToEndpoint) | Timeout(Option<Instant>)`.
`ConnEvent`: `Established | Closed(ConnectionLost)`.
`ToEndpoint`: `Retired { our_index: u32 }`.

**S29's cancel is `handle_connection_event(now, id, Retired { our_index })`** —
the endpoint core's docs say so explicitly ("It is also S29's cancellation
path"), and `drop_pending` releases index + static + guard pin. For a
`Connecting` that never established, `our_index` is a value the connection
never had; `handle_connection_event` calls `indices.remove_session` /
`remove_pending` on it and then `drop_pending(now, id)`, which is keyed on
`ConnectionId` and does the real work. So the shell must pass **some**
`u32`; the connection core would have emitted `Retired` only if it had a
session. See decision D4 below.

## 1. Testutil harness API (TEST-B depends on this)

Everything below is in `slither::testutil`, behind `feature = "test-util"`
(implicit under `cfg(test)`). The three ruling-60 contract names —
`Network`, `FlakyWire`, `FlakyPolicy` — are untouched; everything here is
additive.

### The two-endpoint harness

```rust
pub async fn local<F: Future>(body: F) -> F::Output;   // runs `body` in a LocalSet
pub async fn settle();                                 // 64 yields, advances no virtual time

pub struct Pair { pub net: Network, pub a: Peer, pub b: Peer }   // no Drop; fields move out
impl Pair {
    pub fn seeded(seed: u64) -> Pair;                       // 10.0.0.1:4001 / 10.0.0.2:4002
    pub fn seeded_with(seed: u64, config: Config) -> Pair;
    pub async fn establish(&self) -> (TestConnection, TestConnection);  // a dials b, 4 DH
}

pub struct Peer {                                      // no Drop; fields move out
    pub endpoint: TestEndpoint,
    pub addr: SocketAddr,
    pub wire: SharedWire,
    pub dhs: DhCounter,
    pub public_static: TestPublicKey,
}

pub fn addr_a() -> SocketAddr;   // 10.0.0.1:4001
pub fn addr_b() -> SocketAddr;   // 10.0.0.2:4002
pub fn addr_c() -> SocketAddr;   // 10.0.0.3:4003

pub type TestIdentity   = CountingIdentity<ReferenceSuite>;
pub type TestEndpoint   = slither::Endpoint<TestIdentity>;
pub type TestConnection = slither::Connection<ReferenceSuite>;
pub type TestIntro      = slither::Intro<TestIdentity>;
pub type TestConnecting = slither::Connecting<TestIdentity>;
pub type TestPublicKey  = PublicKeyOf<TestIdentity>;
```

`Pair::seeded` **panics outside a `LocalSet`** — the driver is spawned with
`spawn_local` (§16.3). `Pair` and `Peer` deliberately have no `Drop` and
public fields, so a test can move `peer.endpoint` out and drop it on its own,
which is what S26's drop-order assertions need.

`settle()` advances **no** virtual time on purpose: a test that needs a
timer should say so with `tokio::time::advance`, so the deadline it depends
on is visible in the test rather than hidden in the fixture. It exists
because §16.2 resolves `close()` at the **seal**, not at the send, so a test
that closes and then asserts on the peer or on `Network::sends()` must give
both drivers a turn first.

### Additions to the existing fixture

```rust
impl Network {
    pub fn wire(&self, addr: SocketAddr) -> FlakyWire;                       // = endpoint()
    pub fn wire_with(&self, addr: SocketAddr, p: FlakyPolicy) -> FlakyWire;  // policy pre-installed
}
impl Tap {
    pub fn datagrams(&self) -> Vec<(SocketAddr, SocketAddr, Vec<u8>)>;       // (from, to, bytes)
}
impl FlakyPolicy {
    pub fn fail_sends(&self, failing: bool);   // shared across clones — the live seam toggle
    pub fn is_failing(&self) -> bool;
}
pub struct SharedWire(Rc<FlakyWire>);          // Wire + Clone + set_policy/policy/local_addr
```

`FlakyPolicy::fail_sends` is a **shared** toggle (`Rc<Cell<bool>>` inside),
so `let p = FlakyPolicy::perfect(); net.wire_with(a, p.clone()); p.fail_sends(true);`
works — Appendix B's "fails for a bounded interval, then heals" without
having to know the interval's end in advance. It composes with the existing
`failing_sends_until(instant)`: either being active fails the send.

`SharedWire` exists because `EndpointBuilder::wire` takes the wire **by
value**, so without it a test could never reach the wire again.

### What TEST-B actually used, and the integration deltas

TEST-B could not see this file while writing, wrote its own local `Node`
fixture over the three contract names, and flagged its guesses. **All of its
guesses were met by changing this side, not `tests/`:**

| TEST-B proposed | Action taken |
|---|---|
| `slither::Endpoint<Id>`, `slither::Connection<Suite>` at the crate root | re-exported at the root; `Connection` made **suite**-parameterised (see D2) |
| `EndpointBuilder…​.build()` | the terminal method is named `build()` |
| `Network::wire(addr)`, `Network::wire_with(addr, policy)` | added |
| `Tap::datagrams() -> Vec<(from, to, bytes)>` | added |
| `FlakyPolicy::fail_sends(bool)` on a shared policy | added |
| `CountingIdentity::seeded` / `.counter()` | already existed |

The only remaining integration item is a lint, not an API: see §5, finding
**I1**.

## 2. Module layout

```
src/shell/mod.rs         module docs, the LocalSet contract, re-exports
src/shell/wire.rs        (slice 0, untouched)
src/shell/driver.rs      the !Send actor: select! loop, both cores, the drain
                         discipline, ruling 49's slither::io trace
src/shell/endpoint.rs    Endpoint, EndpointBuilder, connect, accept, Connecting
src/shell/staged.rs      Intro -> Claimed -> Proven, drop = silent reject
src/shell/connection.rs  Connection, close(), closed(), the four accessors
src/shell/shared.rs      the Rc<RefCell<_>> cell, the command enum, the waker map
```

`shared.rs` is a seventh file the plan's §2.2 table does not list. It holds
only what `driver.rs` and the three handle files **both** need (the shared
cell, `Command`, `Wakers`); putting it in `driver.rs` would make every
handle file `use super::driver::…` for types the driver does not own. No
behaviour lives there. Flagged because §2.2's file list reads as exhaustive
(working rule 8) and this is an addition to it.

### The seam, concretely

One `Rc<RefCell<ShellState<I>>>` shared by the driver and every handle,
plus one **unbounded** `mpsc` command channel handle → driver.

* **`Endpoint::connect`** (sync, ruling 87): borrows the cell, tests
  `statics` for NONE, writes PENDING, pushes `Command::Connect`, returns
  `Connecting`. No DH on the caller's task — the core's `connect()` (which
  pays 2 DH building msg1) runs on the driver.
* **`Connecting::drop`**: borrows the cell, writes the static back to NONE,
  pushes `Command::Cancel`. FIFO on one channel is what orders the
  cancellation ahead of the redial's `Command::Connect` (ruling 50's MUST),
  and the synchronous NONE write is what lets the redial's *synchronous*
  test pass with no clock advance (ruling 87).
* **`accept` + the three staged verbs**: `Command::*` + `oneshot` reply
  (ruling 53) — the DH lands on the driver.
* **`Connection::close`, `closed`, the accessors**: the connection's
  `core::Connection` lives in an `Rc<RefCell<ConnCell<S>>>` the driver also
  holds. A handle mutates it, marks it dirty, and pushes
  `Command::Dirty(id)`; the **driver** drains `poll_output()` to `Timeout`
  and performs the I/O (§16.3 verbatim: "it is merely not always the driver
  that made the call").

**Why the command channel is unbounded.** §16.8 bounds *driver → handle*
deliveries. Handle → driver runs the other way and carries **verbs, not
payload**: one item per verb call or per handle drop, so the application's
own call rate bounds the depth. A bounded channel could only fail two ways,
both forbidden: block a synchronous `connect()`/`Drop` (§16.8), or drop a
`Command::Cancel` (§10.6's "reliable is never droppable", and ruling 50's
MUST). Nothing reliable ever sits in it.

**The one borrow rule that makes this safe:** *no `RefCell` borrow is ever
held across an `await`.* Every borrow in `driver.rs` is inside a
non-`async` helper or a `{ }` block that ends before the next `.await`.
This is the invariant the seam review should check first.

## 3. Decisions the plan did not settle

**D1 — the handle→driver command channel is unbounded.** §16.8 bounds
*driver → handle* deliveries. Handle → driver runs the other way and carries
**verbs, not payload**: one item per verb call or per handle drop, so the
application's own call rate bounds the depth. A bounded channel could only
fail two ways, both forbidden: block a synchronous `connect()`/`Drop`
(§16.8), or drop a `Command::Cancel` (§10.6's "reliable is never
droppable", and ruling 50's MUST). Nothing reliable ever sits in it.

**D2 — `Connection` is parameterised by the *suite*, not the identity.**
`Connection<S: Handshake>`, matching §16.4's `core::Connection<C: Handshake>`
and §16.2's bare `Connection`. An `Identity` is a statement about where
*our own* private key lives; it has nothing to do with the session, and two
endpoints holding the same static behind different providers must produce
the same connection type. The two things the handle still needs from the
identity-parameterised `Shell` — decrementing the handle count and marking
the cell dirty — travel through a small `Rc<dyn ShellLink>` instead of
through a type parameter. TEST-B, writing independently, guessed the same
shape.

**D3 — the shell keeps its own NONE/PENDING/LIVE map, stamped.** Forced;
see conflict **C-B1** in §5. Every entry carries a monotone `attempt` stamp
and every driver-side removal is stamp-checked, because on a paused clock
`drop(connecting); ep.connect(same_static)` runs to completion **before the
driver is scheduled at all** — so by the time the driver processes the first
attempt's cancellation or give-up, the map already holds the second
attempt's entry. Removing by key alone would delete the live redial.

**D4 — cancelling a `Connecting` sends `Retired { our_index: 0 }`.** §16.4
lists no cancel verb; the endpoint core's own docs name
`handle_connection_event(.., Retired)` as "S29's cancellation path". A
`connect()`-created connection that never installed a session has no session
index to name. `0` is provably safe: `IndexTables::mint` draws a random
**nonzero** `u32` absent from both tables, so no live route is ever keyed on
it. Any other placeholder would have a 2⁻³² chance per live session of
evicting somebody else's route. Documented at the call site.

**D5 — `accept()` uses `Command` + `oneshot<Intro>`, per ruling 53's
table.** The oneshot carries a **fully built `Intro`**, not an `IntroId`, so
a cancelled `accept()` drops an `Intro` — which is §6.2's silent reject, the
documented meaning of dropping a staged object, rather than an id leaked
until `INTRO_TTL`. The driver also checks `Sender::is_closed()` immediately
before sending; on a single-threaded runtime the check and the send are
atomic with respect to the application, because no other task on the thread
can run while the driver is running.

**D6 — a staged verb that fails discards the chain.** §6.2's typestate
consumes the handle, so there is no retry to preserve it for — and leaving
it parked is *worse*: §6.3's dedup replaces a same-source entry **without
re-surfacing it**, so the peer's next retransmit would find the slot held by
a chain no application can reach, and go unanswered until `INTRO_TTL`.
Ruling 72's "the chain is left parked" is a statement about the **core**
verb, which is idempotent and retryable; the shell handle is not.

**D7 — `remote_static` and `session_id` are cached in the handle;
`remote_address` is read from the cell.** §7.8 pins one session per
connection with no rekey swap, so the first two cannot go stale — and
caching them is what lets a post-mortem handle keep answering after §15.2's
linger has dropped the session. `remote_address` moves on a §7.3 roam
(slice 7), so it must be a live cell read.

**D8 — the driver checks "every handle gone" *after* it has flushed I/O.**
So a CLOSE sealed by a legitimate last-handle-to-*one*-connection drop still
reaches the wire even if the endpoint is dropped immediately afterwards.
Ruling 88's coincident case seals nothing in the first place, so nothing
here can resurrect it. Both halves are pinned by tests.

**D9 — no `Connection::id()`.** §16.2's `Connection` surface is a list, and
in this project a list is read as exhaustive (working rule 8);
`ConnectionId` appears in §16.4's *core* API and nowhere on the handle. It
would be useful for correlating traces and may be worth a ruling; the
absence is the reversible choice.

**D11 — a cancelled staged verb rejects its chain.** Each of
`read_identity`/`authenticate`/`accept` keeps its staged object **alive
across the driver round-trip** and marks it consumed only once the reply is
in hand, so dropping the future runs the object's `Drop` — §6.2's silent
reject — and frees the stage-0 slot at once. Marking it consumed up front
(the obvious spelling) would orphan the chain until `INTRO_TTL`, holding one
of §6.3's four per-source slots against a peer for 15 s. For `accept()` the
post-reply case is different and also right: the driver has already built
the `Connection`, dropping the oneshot drops it, and that is the last handle
to it — so §16.2's `close(NO_ERROR, "")` fires and the peer is told at once
rather than waiting out `DEAD_TIMEOUT`.

**D12 — the driver prunes its ready-introduction queue.** `Driver::ready`
holds `IntroId`s for entries the core has parked under §6.3's
`INTRO_QUEUE_CAP`, which bounds it *at any instant* but not over time: an
application that never calls `accept()` would accumulate one dead id per
expiry for the endpoint's life, which is the unbounded intermediate queue
§10.6 forbids. `prune_ready` drops ids the core no longer holds, using
ruling 71's `intro_source` as the presence oracle. It is also what stops a
stale id being handed to a later `accept()` ahead of a live one — pinned by
`an_expired_introduction_is_not_handed_to_a_later_accept`.

**D10 — the endpoint core lives in the shared cell.** Not privately in the
driver, so that §6.2's `Intro::source()` and `Intro::sender_index()` are
**live reads** of the parked entry. Ruling 71 makes that load-bearing: §5.5
mints a new random index on every retransmit and §6.3's dedup replaces a
same-source entry without re-surfacing it, so a value cached at hand-over is
stale the moment the peer retransmits. Only `&self` accessors are reached
from a handle; every mutating endpoint verb is still a driver round-trip,
because §6.2 requires the DH to land there.

## 4. Ruling compliance notes (81/84, 87, 88, 89, 58)

**81 / 84 — `Retired` timing.** The connection core already decides *which*
deaths carry a `Retired` (slice 3a, frozen). The shell's obligation is
ordering, and `driver.rs` discharges it in two named places:

* `Driver::serve_connection` handles `ConnOutput::ToEndpoint(Retired)` by
  calling `core::Endpoint::handle_connection_event` **inside the same drain
  pass that produced it** — before anything else touches the connection.
* `Driver::release_dead` — the shell-side release — runs only after every
  drain in the pass has finished, and keys on *the core's* state
  (`closed.is_some() && !is_established()`), never on "did I see a
  `Retired`". That is what makes rulings 81/84's fourth case correct: a
  connection that never installed a session emits **no** `Retired` at all,
  and would leak a shell record for ever if the release waited for one.

There is therefore no ordering in which the index route or the §17.1 guard
pin outlives the shell bookkeeping that frees it. Pinned by
`a_closed_connection_frees_its_static_when_the_linger_expires` (the linger
path: still `AlreadyConnected` during the linger, `Ok` after it) and by
TEST-B's `s29_retry_loop_replaces_rather_than_accumulates`.

**87 — `connect()` is synchronous.** `Endpoint::connect` reads and writes
the shared static map on the caller's stack and returns; the core's
`connect()` (2 DH) runs on the driver. `Connecting::drop` writes the static
back to NONE **in that same cell**, so an immediate redial with no clock
advance reads what the drop just wrote. No cancellation flag is polled
anywhere. Pinned by
`a_cancelled_dial_frees_the_static_with_no_clock_advance` and by TEST-B's
`s29_cancel_then_immediate_redial`.

**88 — the coincident last-handle drop transmits nothing.**
`Connection::drop` decrements the per-connection count, then calls
`Shell::release()`, then seals `close(NO_ERROR, "")` **only if** it was the
last handle to this connection **and not** the last handle in the process.
The rustdoc on `Connection` states the rule, names it as documentation
obligation #4, and says it is the opposite of the obvious guess. Pinned as a
**pair** (the coincident case transmits nothing; the non-coincident case
does), because either alone is satisfied by a build that never seals on
drop.

**89 — `SessionId`.** `hiss::noise::SessionId`, re-exported at the crate
root with rustdoc saying its `Eq` is **not** constant-time and must not be
used to compare a secret; the same warning is on `Connection::session_id`.
See finding **F1** in §5: the ruling's claim that it is "reachable from the
seal half slither already holds" is true of hiss's concrete type and false
of slither's `Handshake::Seal`, which has no bounds.

**58 (§16.11) — no prefetch.** Nothing in the shell claims ahead of a
consumer. The driver's `ready` queue holds `IntroId`s the **core** has
parked under §6.3's `INTRO_QUEUE_CAP`, and hands one over only when an
`accept()` is outstanding; a cancelled `accept()` rejects the `Intro` rather
than buffering it. `closed()` is a latch, not a queue. No adapter machinery
presumes a prefetch.

## 5. Conflicts / suspected spec defects (working rules 3 and 8)

Reported, not resolved. Each says what slice 3b did in the meantime.

### C-B1 — ruling 87 says `connect()` "only mints the pending"; slice 3a's frozen core mints **and** spends 2 DH

§16.3 (4255–4257), verbatim: *"Nor does it need one: **`connect()` performs
no DH.** §6.1's initiator costs are paid when msg1 is built, on the driver;
the verb itself only mints the pending."* That construction has minting and
msg1-building as **separable** steps, and the natural implementation is then
for the synchronous verb to write the core's own static map (no DH) and let
the driver build msg1 (2 DH). Then there is exactly one static map and
U6's warning — *"slice 3's shell must not maintain a second copy"* — is
satisfied for free.

`core::Endpoint::connect()` (slice 2a, frozen) does both in one call: it
inserts the `StaticEntry` **and** calls `start_attempt`, which opens the
identity and writes msg1 — the 2 DH `a_dial_costs_two_dh_and_each_retransmit_two_more`
pins. There is no core accessor that writes the static map without paying,
and slice 3b may not add one.

**What slice 3b did:** kept a shell-side stamped mirror
(`ShellState::statics`), documented every transition in `driver.rs`'s module
docs, and made the driver `debug_assert!` when the core refuses a connect the
mirror admitted — which is the one failure mode the duplication can have.

**Which text is right is not slice 3b's call.** Note the shape: the prose
("only mints the pending") describes a core factoring that does not exist,
while the code-like rule (§16.2's signature) is satisfiable either way —
working rule 3's pattern, so it is reported rather than picked. The cheap
resolution is a core split (`mint_pending` / `start_attempt`) in a later
slice, which would delete the mirror entirely.

### F1 — ruling 89's reachability claim does not hold through slither's own `Handshake` seam (fixed, minimally, and flagged)

Ruling 89: *"It is reachable from the seal half slither already holds
(`DatagramSend::session_id`), so nothing is captured at install."* True of
hiss's concrete type. **False of `Handshake::Seal`**, which is an associated
type with **no bounds at all** (`src/packet/handshake.rs:83`), so the shell —
which is generic over `I: Identity` — cannot call anything on it.

This is working rule 8's shape exactly: a stated construction
(`session_id()` is reachable) with an unstated scope (through *which* type).

**What slice 3b did:** added `Handshake::session_id(seal: &Self::Seal) -> &SessionId`
beside the existing `Handshake::next_counter`, and its one-line impl in
`channel!`. Additive to a `pub trait` that only `channel!` implements; moves
no wire byte, changes no constant, frame or timer. `src/packet/` is in
neither the "you own" nor the "do not touch" list of slice 3b's brief and
has no concurrent owner, so this created no race — but it is a slice-1 file
and the maintainer should confirm.

### F2 — §16.2's four accessors are declared total, but §16.9 admits pre-establishment handles

§16.2 declares `session_id() -> SessionId` and `remote_static() -> PublicKey`
with no `Option`, beside `is_established() -> bool` — which is only
interesting if a `Connection` can exist while *not* established. §16.9 says
so outright: *"the connection core exists from `connect()`, and early stream
opens, writes, messages, and datagrams land in ordinary stream/queue
state"*, and `SendStream::id()` returns `None` "before establishment
(§16.9)" — so §16.9 clearly contemplates handles on a pre-establishment
connection.

**There is no shell path to one.** `Connecting` resolves only on
`ConnEvent::Established`, and `Proven::accept()` returns an already-installed
connection. So in slice 3b the question is unreachable and the accessors are
honestly total. It becomes live the moment slice 4 wants §16.9's early
sends, and at that point either `Connection` gains a pre-establishment
constructor (and `session_id()` needs an answer) or §16.9's early-send
paragraph needs a note saying the handle does not exist yet. Named now so it
is not discovered as a signature change in slice 4.

### F3 — §16.4's `EndpointOutput::Transmit` cannot carry ruling 49's attribution (the plan's U7, confirmed)

§18.2 requires a `slither::io` trace *"against the connection whose datagram
it was"*. `ConnOutput::Transmit` is already scoped to a connection and the
driver traces it with the `ConnectionId`. `EndpointOutput::Transmit` carries
`{ to, data }` and nothing else, so a failed msg1, retransmit or msg2 is
traced with the destination address and the error but `conn = None`. The
plan's provisional (do not change `EndpointOutput`'s shape this slice) was
followed; the driver's `Outgoing` type carries `conn: Option<ConnectionId>`
so the day the core can attribute, one field changes.

### I1 — an integration item, not a spec question: `clippy::async_yields_async`

`tests/story_lifecycle.rs`'s `s26_a_connecting_alone_keeps_the_driver_alive`
returns a `Connecting` out of a `run_until` block — which is exactly ruling
62's state ("a `Connecting` alone, with no `Endpoint` and no `Connection`")
and cannot be written any other way. `clippy::async_yields_async` is
deny-by-default and reads it as a forgotten `.await`.

Slice 3b's implementer may not touch `tests/`, so the allow went into
`Cargo.toml`'s `[lints.clippy]` with the reasoning written out.
**The integrator should prefer the narrow form** — a
`#[allow(clippy::async_yields_async)]` on that one test function — and
delete the `Cargo.toml` entry. The comment in `Cargo.toml` says so.

### Not a defect, recorded because it was checked: `our_index: 0` is safe

`IndexTables::mint` draws a random **nonzero** `u32` absent from both
tables, so `Retired { our_index: 0 }` — the shell's cancellation of a
pending that never had a session index — cannot evict anyone else's route.
Had indices been drawn from the full `u32`, the cancel path would have had a
2⁻³² chance per live session of removing the wrong one. The bound is stated
in `mint`'s own doc comment; it is load-bearing here and nothing said so.

## 6. Gate output

Run by IMPL-B on the finished tree, in this order.

```
$ cargo build --all-features --all-targets
   Compiling slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.22s

$ cargo fmt --all --check
(no output — no diff)

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.68s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.46s
   Generated .../target/doc/slither/index.html

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.39s
   Generated .../target/doc/slither/index.html

$ cargo test
running 297 tests   test result: ok. 297 passed; 0 failed   (lib)
running 103 tests   test result: ok. 103 passed; 0 failed   (tests/spec_constants.rs)
running  11 tests   test result: ok.  11 passed; 0 failed   (tests/spec_errors.rs)
running   4 tests   test result: ok.   4 passed; 0 failed   (tests/spec_packet.rs)
running   0 tests   (tests/spec_shell.rs — feature-gated off)
running   0 tests   (tests/story_dial.rs — feature-gated off)
running   0 tests   (tests/story_lifecycle.rs — feature-gated off)
running   5 tests   test result: ok.   5 passed; 0 failed   (doctests)
                                                              → 420 total

$ cargo test --all-features
running 297 tests   test result: ok. 297 passed; 0 failed   (lib)
running 103 tests   test result: ok. 103 passed; 0 failed   (tests/spec_constants.rs)
running  11 tests   test result: ok.  11 passed; 0 failed   (tests/spec_errors.rs)
running   4 tests   test result: ok.   4 passed; 0 failed   (tests/spec_packet.rs)
running   9 tests   test result: ok.   9 passed; 0 failed   (tests/spec_shell.rs)
running   4 tests   test result: ok.   4 passed; 0 failed   (tests/story_dial.rs)
running  16 tests   test result: ok.  16 passed; 0 failed   (tests/story_lifecycle.rs)
running   7 tests   test result: ok.   7 passed; 0 failed   (doctests)
                                                              → 451 total

$ cargo +1.96 check --all-features --all-targets
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.88s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

**Baseline was 407** (285 + 103 + 11 + 4 + 4). Nothing existing went red; the
wire pins and size/constant tests in `tests/spec_constants.rs` and
`tests/spec_packet.rs` are byte-identical.

New: **+12 lib tests** (the shell's implementation smoke tests — `src/shell/mod.rs`),
**+3 doctests**, and **+29 of TEST-B's story tests**, which passed on the
first integration attempt after the API reconciliation in §1.

### TEST-B's story tests (all green)

```
tests/story_lifecycle.rs   s1_dial_then_close_both_sides_observe
                           s1_close_resolves_promptly_not_at_linger_expiry
                           s1_close_reason_is_truncated_at_close_reason_max
                           s26_endpoint_drop_alone_does_not_stop_the_driver
                           s26_last_connection_drop_with_endpoint_alive_closes_gracefully
                           s26_coincident_last_handle_drop_transmits_nothing
                           s26_last_handle_drop_stops_the_driver
                           s26_a_connecting_alone_keeps_the_driver_alive
                           s27_closed_resolves_with_no_verb_in_flight_{locally_closed,peer_closed,timed_out}
                           s27_closed_is_latched_and_concurrent
                           s29_cancel_then_immediate_redial
                           s29_cancelled_train_transmits_nothing_further
                           s29_after_cancel_the_static_routes_as_none
                           s29_retry_loop_replaces_rather_than_accumulates
tests/story_dial.rs        s2_no_answer_gives_timed_out_at_giveup
                           s2_giveup_releases_the_static_for_an_immediate_redial
                           s2_retransmit_train_is_fixed_interval_not_exponential
                           s2_unaccepted_initiations_cost_the_responder_zero_dh
tests/spec_shell.rs        connect_is_synchronous_so_a_pending_static_refuses_before_any_await
                           a_staged_object_does_not_keep_the_driver_alive
                           the_staged_ladder_charges_and_transmits_only_at_its_own_verb
                           dropping_an_intro_is_a_silent_reject_and_costs_nothing
                           dropping_a_proven_transmits_nothing
                           cancelled_dial_leaves_the_peer_a_silent_half_open_session
                           session_id_agrees_across_peers_and_differs_across_sessions
                           a_failing_send_does_not_kill_the_connection_and_the_death_stays_timed_out
                           close_over_a_broken_seam_still_surfaces_locally_closed
```

### The implementation smoke tests (`src/shell/mod.rs`, mechanism not story)

Deliberately about the *seam*, not the stories: `a_dial_and_a_staged_accept_meet`,
`a_close_reaches_the_peer_as_peer_closed`,
`closed_is_latched_concurrent_and_cancel_safe`,
`a_cancelled_dial_frees_the_static_with_no_clock_advance`,
`the_endpoint_is_not_the_last_handle`,
`the_coincident_last_handle_drop_transmits_nothing` **paired with**
`a_non_coincident_last_connection_drop_does_transmit`,
`dropping_an_intro_is_silent`,
`a_dial_nobody_answers_gives_up_at_the_giveup_and_not_before` (both sides of
the bound), `an_idle_connection_dies_at_dead_timeout`,
`a_closed_connection_frees_its_static_when_the_linger_expires`,
`an_expired_introduction_is_not_handed_to_a_later_accept`.

# slither public API review — reviewer "opus"

## Base commit

Verified as my first command:

```
$ git -C /Users/nicolasdiprima/work/primetype/slither log --oneline -1
cd12ed7 Ruling 242: SPEC.md is stamped RATIFIED, with its amendment table
```

**Matches the brief's required base `cd12ed7`.** `git status --short` was
empty at that moment. Per working rule 16 the manifest was read only via
`git show cd12ed7:Cargo.toml`; `benches/` was never read; nothing was
written into the repository; no `cargo` command was run at all (see §5).

Sections read directly, in full, at this commit: **SPEC.md §16.2**
(5035–5459), **§16.4** (5697–5949), **§16.11 + §16.11.1** (6198–6360),
**§6.2** (1230–1254); **STORIES.md** in full; **rulings.md** ruling 150
(3806–3840). Everything below marked *verified* was read; everything
marked *judgement* is mine.

---

## 1. The enumerated surface

`#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`. Nothing on by default.

### Crate root (`src/lib.rs:199–293`)

| Kind | Items |
|---|---|
| modules | `compat`, `config`, `constants`, `error`, `identity`, `packet`, `shell`; `testutil` (`cfg(any(test, feature="test-util"))`) |
| `pub(crate)` | `core`, `varint` |
| re-exports | `block_on`; `Config, SystemClock, WallClock`; the ten `error` types; `CurveOf, Identity, PrivateKeyOf, PublicKeyOf, SoftwareIdentity`; `Channel, Handshake`; `BiStream, Claimed, Connecting, Connection, Endpoint, EndpointBuilder, Intro, Notification, Proven, RecvStream, SendStream`; `hiss::noise::SessionId`; `ConnectionId, Dir, IntroId, StreamId, Timestamp`; `hiss` |
| macro | `channel!` — `#[macro_export]` at `src/packet/suite.rs:218`; the crate's **only** exported macro |

### `shell` (default)

- `Endpoint<I>`: `builder<W: Wire>()`, `async accept() -> Option<Intro<I>>`,
  `connect(SocketAddr, PublicKeyOf<I>) -> Result<Connecting<I>, ConnectError>`.
  **Three verbs, exactly §16.2's three.**
- `EndpointBuilder<I, W>`: `identity`, `wire`, `config`, `rng_seed`, `build`
  (all `#[must_use]`), plus `Default` and a redacting `Debug`.
- `Connecting<I>`: `Future`, `Debug`.
- `Connection<S>`: the **18** verbs §16.2 lists, one for one — `open_bi`,
  `open_uni`, `accept_bi`, `accept_uni`, `send_message`, `recv_message`,
  `send_datagram`, `recv_datagram`, `acked`, `close`,
  `set_persistent_keepalive`, `persistent_keepalive`, `closed`, `notified`,
  `remote_static`, `remote_address`, `session_id`, `is_established`.
- `SendStream<S>`: `write`, `finish`, `acked`, `reset`, `id` (5, = §16.2).
- `RecvStream<S>`: `read -> Result<Option<usize>, ReadError>`, `id` (2).
- `BiStream<S>`: `split`, `join`, `id` (3).
- `Intro<I>`: `source`, `sender_index`, `read_identity`;
  `Claimed<I>`: `claimed_static`, `authenticate`;
  `Proven<I>`: `peer_static`, `timestamp`, `accept`. **Exactly §6.2's list**
  (SPEC.md:1232–1247), verified line by line.
- `Notification` — `#[non_exhaustive]`, `Debug + Clone + PartialEq + Eq`,
  variants `AddressMoved { from, to }`, `Contested`, `ContestCleared`.
  Matches §16.2's block at SPEC.md:5091–5096.
- `shell::wire::Wire` — `async fn send_to`/`recv_from`, `impl Wire for
  tokio::net::UdpSocket`.

### `compat`

| Gate | Items |
|---|---|
| none | `compat::io` — `From<ReadError>`/`From<WriteError> for io::Error`; `AsyncRead`/`AsyncWrite` on `SendStream`, `RecvStream`, `BiStream` (4 impls). `compat::rt::block_on` (re-exported at the root). |
| `sink` | `Connection::{messages, datagrams, incoming_bi, incoming_uni, notifications, message_sink, datagram_sink}`, `Endpoint::incoming`; types `Messages, Datagrams, IncomingBi, IncomingUni, Notifications, Incoming, MessageSink, DatagramSink`. **All eight faces §16.11 names (SPEC.md:6317–6328 incl. ruling 229's `incoming`) are present.** |
| `codec` | `Connection::{framed_bi, accept_framed_bi}` |
| `tower` | `impl Service<(SocketAddr, PublicKeyOf<I>)> for Endpoint<I>`; `impl Service<()> for &Connection<S>`; `impl Service<()> for Connection<S>`; futures `Connect`, `OpenBi`, `OpenBiOwned`; `serve()` |

### `config`, `error`, `identity`, `constants`, `packet`

`WallClock`, `SystemClock`, `Config` (+ 4 `#[must_use]` `with_*`, 4 getters,
`DEFAULT_EPOCH_SIZE`, `Default`, redacting `Debug`) · ten error enums ·
`Identity`, `CurveOf`, `PublicKeyOf`, `PrivateKeyOf`, `SoftwareIdentity`,
`SoftwareIdentityError` · ~90 consts · `Channel`, `Handshake`,
`ReferenceSuite`, `PublicKeyFor<C>`, `PROTOCOL_NAME_CAP`, `protocol_name()`,
`channel!`.

### `testutil` (feature `test-util`)

Far larger than the crate docs describe: `ENETUNREACH`, `Spied`, `Tap`,
`SendFailure`, `FlakyPolicy`, `Network`, `FlakyWire`, `DhCounter`,
`CountingProvider`, `CountingIdentity`, `SharedWire`, `Peer`, `Pair`,
`addr_a/b/c`, and the aliases `TestIdentity`, `TestEndpoint`,
`TestConnection`, `TestSendStream`, `TestRecvStream`, `TestBiStream`,
`TestIntro`, `TestConnecting`, `TestPublicKey`.

---

## 2. Findings

Ordered by severity. "Verified" = I opened the artefact and read the lines
cited. "Judgement" = my reading, not a quotation.

---

### F1 — **blocker (spec conformance)** · `write_kind` implements the superseded ruling 227, not ruling 238

**`src/compat/io.rs:61–93`** (and its doc block at 48–73).

*Verified.* §16.11.1, SPEC.md:6264–6279, **[CORRECTED 2026/08/16 — ruling
238]**, reads:

> **The `WriteError` match is exhaustive, and deliberately so.** … A `_ =>`
> arm therefore does not future-proof the conversion — it **hides** the
> future. An exhaustive match turns the day `Stopped` lands into a
> **compile error at the exact site that must be updated**, which is
> strictly stronger than silently mapping a new variant to `Other` …

The code still carries `#[allow(unreachable_patterns)]` and
`_ => io::ErrorKind::Other`, with a comment citing ruling 227:

```rust
#[allow(unreachable_patterns)]
fn write_kind(err: &WriteError) -> io::ErrorKind {
    match err { …
        // **[ruling 227]** `WriteError` is `#[non_exhaustive]` … so this
        // arm is required — and it maps to `Other` …
        _ => io::ErrorKind::Other,
    }
}
```

**Consumer impact today: none** — the arm is unreachable in-crate, and every
other row of the table is exact. **What it costs is the future**: the day
`Stopped` lands (§19, ruling 61), the arm silently maps it to
`ErrorKind::Other` instead of failing the build at the one site that must be
updated. That is precisely the outcome ruling 238 was ratified to prevent.

Severity is **blocker** under CLAUDE.md's hard rule *"the code must match the
spec"*; on consumer-visible impact alone it would be should-fix. I flag it
high because it is a §16.11.1 clause that is **directly testable** and was
ratified today.

**Suggested shape:** delete the `#[allow]` and the `_` arm; rewrite the doc
comment to cite 238 rather than 227. Note the code's *own* comment
(io.rs:61–73) already reaches ruling 238's premise — *"`#[non_exhaustive]` is
inert inside the defining crate … In-crate, the match is already
exhaustive"* — and then keeps the arm anyway.

---

### F2 — **conflict, reported not resolved** · two ratified signatures for `BiStream::join`

*Verified, both lines read.*

- **SPEC.md:5110–5113 (§16.2)**, header comment *"join's shape is ruling 120"*:
  ```rust
  pub fn join(send: SendStream, recv: RecvStream)
      -> Result<Self, (SendStream, RecvStream)>;   // Err = not the same stream
  ```
- **SPEC.md:6219 (§16.11)**:
  ```rust
  pub fn join(send: SendStream, recv: RecvStream) -> Self;
  ```

The code (`src/shell/stream.rs:975–984`) implements §16.2's `Result` form,
checking `Rc::ptr_eq(&send.cell, &recv.cell) && send.conn == recv.conn &&
send.r == recv.r`, and its rustdoc cites ruling 120 and ruling 44's
*rejection-is-a-`Result`* precedent.

I am not resolving this. Per the tiebreak the maintainer reaches for —
*follow the statement some other proof depends on* — §16.2's form is the one
with an argument attached (ruling 120 + ruling 44's precedent + the
`clippy::result_large_err` `#[expect]` that exists only because of it),
while §16.11's block is a bare signature in a section whose stated purpose is
*"no verb and no state"*. But that is an observation, not a ruling.

---

### F3 — **should-fix** · §16.4's `ConnEvent` list omits `SendCreditAvailable`, which ruling 150 authorised

*Verified.* `grep -n 'SendCreditAvailable' SPEC.md` → **zero hits**.

- §16.4's ratified `enum ConnEvent` (SPEC.md:5778–5793) lists 13 variants.
- §16.2's *"Why this is not `core::ConnEvent`"* argument (SPEC.md:5301–5304)
  re-enumerates *"the core enum's remaining variants"* as ten, and rests on
  *"each is **already served**: each is the wakeup behind a blocking verb"*.
- `src/core/connection/mod.rs:3163–3230` has **14** variants — the extra one
  is `SendCreditAvailable` (mod.rs:3222), pushed at mod.rs:1600, consumed at
  `src/shell/driver.rs:644` and `src/shell/connection.rs:864`.
- **rulings.md:3833–3839 (ruling 150)** authorises it explicitly: *"Slice 6
  mints one `pub(crate)` `ConnEvent` variant for it. That enum is internal,
  the addition is additive."*

So the code is authorised and the **spec text was never amended**. Not a
public-API defect (`ConnEvent` is `pub(crate)`), but it is a §16.4
conformance gap, and it leaves §16.2's *"each is already served"* argument
ranging over an incomplete list — the shape ruling 152 called out three
paragraphs later in that same ruling block (*"Three of my lists have now been
read as exhaustive and found short"*). Working rule 8.

**Suggested shape:** add the variant to §16.4's block and to §16.2's
enumeration, each with the one-line reason (`send_message` refused for
**connection** credit has no other wakeup, because `StreamWritable { r }`
needs a half and a pending message has no stream yet).

---

### F4 — **should-fix** · `src/shell/wire.rs:47–48` states an API shape the crate does not have

*Verified.* The `Wire` rustdoc says:

> The price is that there is no `Box<dyn Wire>`: **an endpoint is generic
> over its wire (`Endpoint<W: Wire>`), not erased.**

The type is `pub struct Endpoint<I: Identity>` (`src/shell/endpoint.rs:45`).
`W` appears only on `EndpointBuilder<I, W>`; `build()` returns `Endpoint<I>`
and moves the wire into the driver task (`src/shell/endpoint.rs:473–498`).

The *substance* survives (the driver is monomorphised over `W`, nothing is
boxed), but the spelling handed to the reader is wrong, and this is the one
trait a consumer must implement for S25. A reader looking for
`Endpoint<W: Wire>` will not find it and will not know whether the doc or the
code moved. Working rule 11's shape, in rustdoc.

**Suggested shape:** *"the **builder** is generic over its wire
(`EndpointBuilder<I, W: Wire>`); `build()` monomorphises the driver over it
and the resulting `Endpoint<I>` names it no further."*

---

### F5 — **should-fix** · `#[must_use]` missing on the three staged types, and on `Connecting`

*Verified.* `grep -rn 'must_use' src/shell/ src/compat/` → five hits in
`endpoint.rs` (all `EndpointBuilder`), four `pub(crate)` hits in
`shared.rs`, **none** in `staged.rs` and **none** in `compat/`.

- **S8's acceptance criterion is literal** (STORIES.md:189): *"It is
  `#[must_use]` and not `Clone`, so a parked chain cannot be forked."*
  `Intro`/`Claimed`/`Proven` (`src/shell/staged.rs:50, 223, 292`) derive
  nothing, so *not `Clone`* holds; `#[must_use]` does not.
- The reason it matters is not style: §16.2's drop semantics (SPEC.md:5394)
  make dropping a staged object a **silent reject**. `#[must_use]` is the
  language's one guard against a value whose only observable effect is its
  drop, and the ladder is *the* place a consumer binds one and forgets it.
- Same class: `Connecting<I>` (`endpoint.rs:257`) has no `#[must_use]`, and
  dropping it is ruling 50's **cancellation**. `Connect`, `OpenBi`,
  `OpenBiOwned` (`compat/tower.rs:106, 195, 265`) are futures with no
  `#[must_use]` either; the standard annotation is `#[must_use = "futures do
  nothing unless polled"]`.

Partial mitigation, and why this is should-fix rather than blocker:
`accept()` returns `Option<Intro>` and `read_identity()`/`authenticate()`/
`connect()` return `Result`, both already `#[must_use]`, so the bare
statement form does warn. What is unguarded is the value that has been bound
and then abandoned.

---

### F6 — **should-fix** · eleven public types have no `Debug` (C-DEBUG)

*Verified* by enumerating `impl … Debug for` across `src/` (12 hits, all in
`shell/`, `config.rs`, `identity.rs`) against the `pub struct`/`pub enum`
list.

Missing: `compat::stream::{Messages, Datagrams, IncomingBi, IncomingUni,
Notifications, Incoming, MessageSink, DatagramSink}`
(`src/compat/stream.rs:360, 384, 404, 427, 447, 481, 510, 578`) and
`compat::tower::{Connect, OpenBi, OpenBiOwned}`
(`src/compat/tower.rs:106, 195, 265`).

Also missing across `testutil` on `Network` (`src/testutil/mod.rs:391`),
`FlakyWire` (577), `CountingProvider` (905), `CountingIdentity` (967),
`SharedWire` (1064), `Peer` (1109), `Pair` (1183) — which matters more than
usual because `Cargo.toml`'s `[package.metadata.docs.rs] all-features` and
lib.rs:168–172 both state that `testutil` is *"a consumer-facing surface, not
test-only plumbing"* and **attested** (ruling 60).

The rest of the crate does this carefully — `Config`, `SoftwareIdentity`,
`Intro`, `Claimed`, `EndpointBuilder` all have hand-written redacting
`Debug`s that deliberately omit secrets — which is why the gap reads as an
oversight rather than a decision. A struct holding `&'a Connection<S>` and a
waker slot can print
`.debug_struct("Messages").finish_non_exhaustive()` and lose nothing.

---

### F7 — **should-fix** · `compat`'s module doc says *"every adapter borrows"*; `OpenBiOwned` does not

*Verified.* `src/compat/mod.rs:63–71`:

> **# Every adapter borrows — ruling 231** … Every adapter therefore carries
> a lifetime, and the consequence a consumer meets is that a **borrowed
> adapter cannot be moved into `spawn_local`** …

`OpenBiOwned<S>` (`src/compat/tower.rs:265`) carries no lifetime — it owns a
`Box<Connection<S>>` minted by `clone_handle()`. It exists *because* the
borrowed form cannot satisfy S33 (`tower.rs:216–229`, ruling 239, which
reverses ruling 236 on exactly this point).

Working rule 4(a)'s shape at module level: ruling 239 introduced the
counterexample and the paragraph asserting the universal was not revisited.
It matters because that paragraph is the crate's *answer* to the
`spawn_local` question, so a consumer who reads it concludes the owned route
does not exist — when the crate ships one, for one adapter, and could ship
more.

---

### F8 — **should-fix** · no public route to a second `Connection` handle, and the documented workaround does not cover two tasks

*Judgement, from verified facts.*

- `Connection<S>` is not `Clone`; `clone_handle()` is `pub(crate)`
  (`src/shell/connection.rs:113`) and is used by the crate's own owned
  `Service` impl (`src/compat/tower.rs:252`).
- Every `sink`-feature adapter borrows, and the fix the docs give is *"move
  the handle into the task and build the adapter inside it"*
  (compat/mod.rs:69–71, repeated verbatim on all eight constructors).
- That works for **one** task. The ordinary shape — a reader task on
  `conn.messages()` and a writer task on `conn.message_sink()`, or a
  `closed()` watcher beside a data loop — needs two owners.
- The answer is `Rc<Connection>`, which is sound here (`!Send` throughout,
  every verb takes `&self`, and the last `Rc` drop is still the last-handle
  `close(NO_ERROR, "")`). The crate's own tests use it —
  `tests/story_lifecycle.rs:850`, *"The watcher is a separate spawned task
  holding an `Rc<Connection>`"* — and **no rustdoc anywhere mentions it**
  (`grep -rn 'Rc<Connection' src/` → zero hits outside a `tower.rs` prose
  reference to `clone_handle`).

A first-hour gap: a consumer meets it the moment they want a `closed()`
watcher, finds `Connection` is not `Clone`, and has to invent `Rc`
themselves — or reaches for `conn.oneshot(())`, which
`src/compat/tower.rs:231–239` documents as **closing the connection**.

**Suggested shape:** one paragraph in `compat/mod.rs`'s "Every adapter
borrows" section, and ideally on `Connection` itself: *"`Connection` is
deliberately not `Clone` — the last handle's drop is `close(NO_ERROR, "")`,
and a `Clone` would move when that happens. To share one connection between
tasks, wrap it in `Rc` and build the adapters inside each task."* If an owned
second handle is wanted publicly, `clone_handle` already exists and already
carries the accounting.

---

### F9 — **should-fix** · `SoftwareIdentityError` is the one public error type not re-exported at the root

*Verified.* `src/lib.rs:237–240` re-exports all ten `error` types;
`src/lib.rs:241` re-exports `SoftwareIdentity` **without**
`SoftwareIdentityError` (`src/identity.rs:126`). So:

```rust
let id = slither::SoftwareIdentity::<S>::generate(rng)?;   // fine
match e { slither::identity::SoftwareIdentityError::InvalidScalar(_) => … }
//        ^^^^^^^^^^^^^^^^^^ the only type on the happy path needing a module path
```

lib.rs:151–152's module bullet — *"[`error`] — the closed error taxonomy of
§18.1, plus `ConfigError`. Its ten types are re-exported at the crate root"*
— is accurate about `error` and, read as the crate's error policy, misses
that an eleventh public error type lives elsewhere. `ConfigError` sets the
precedent: it is *outside* §18.1's taxonomy (ruling 44) and is re-exported
anyway.

---

### F10 — **should-fix** · `lib.rs:268` says "three identifiers" and re-exports five; two of them appear in no public signature

*Verified.* `src/lib.rs:268–273`:

> The **three** identifiers §16.4's surface names that a consumer must be
> able to spell. They live inside the `pub(crate)` core, so they are
> re-exported here to be publicly *reachable* — a public signature naming an
> unreachable type is a rustdoc break, not merely a lint.

followed by `pub use crate::core::{ConnectionId, Dir, IntroId, StreamId,
Timestamp};` — **five** names. Working rule 8's shape, in the crate root's
own prose.

The stated justification holds for three of them, and I checked which:

| Type | Reachable from a public signature? |
|---|---|
| `StreamId` | yes — `SendStream::id`, `RecvStream::id`, `BiStream::id` |
| `Timestamp` | yes — `Proven::timestamp` (`src/shell/staged.rs:312`) |
| `Dir` | yes — `StreamId::dir()` (`src/core/connection/stream_id.rs:65`) |
| `ConnectionId` | **no** |
| `IntroId` | **no** |

*Verified* by grepping every `pub fn`/`pub async fn` in `shell/`, `compat/`,
`config.rs`, `identity.rs`, `packet/` for each name. `ConnectionId`
(`src/core/mod.rs:93`) and `IntroId` (`src/core/endpoint/staged.rs:55`) are
opaque `u64` newtypes with no public constructor, no public accessor and no
signature that produces or consumes one. A consumer can name them and can do
nothing with them.

That is public surface frozen under semver at the 0.2 release for no consumer
benefit. Removing them later is breaking; not shipping them is free.
**Suggested shape:** drop `ConnectionId` and `IntroId` from the re-export and
fix "three" to name the three that remain.

---

### F11 — **polish** · lib.rs's module list is written for an earlier slice

*Verified.* Three stale statements in the crate root's docs, at the commit
that stamps SPEC.md RATIFIED:

- `src/lib.rs:161–163`: *"[`shell`] — the I/O shell. **Slice by slice it
  grows** the driver and the handles; **today it carries**
  [`shell::wire::Wire`], the datagram seam an application supplies."* The
  shell today carries `Endpoint`, `Connection`, three stream handles, three
  staged types and `Notification`.
- `src/lib.rs:159–160`: *"`core` — §16.4's two sans-io state machines.
  **Crate-internal until the driver that can drive them exists**."* The
  driver exists (`src/shell/driver.rs`, 1429 lines).
- `src/lib.rs:208–213`: *"`pub(crate)` **in this slice**, deliberately:
  nothing outside the crate can drive them **until the driver lands** …"*

The `pub(crate)` **decision** may well still be right — but its stated reason
has expired, and the crate is about to freeze it. Whether `core` ships public
is a maintainer question I am not answering; what I report is that the
recorded justification no longer describes the tree. (Consequence if it
stays: §16.10's kernel-free drivability is reachable downstream only through
`testutil` + the shell, never against the bare cores.)

---

### F12 — **polish** · `channel!` is the crate's only macro and is nearly undiscoverable

*Verified.* `#[macro_export]` at `src/packet/suite.rs:218` puts it at
`slither::channel!`. S22's acceptance is *"the suite is declared once via the
macro"* (STORIES.md:349), so it is the **first** thing a consumer needs after
`SoftwareIdentity`. In lib.rs it appears exactly once — inside the
`pub use hiss` doc paragraph (`src/lib.rs:278`), as a link explaining why
`hiss` must also be a direct dependency. It is absent from the `# Modules`
list, from the `packet` bullet, and from the crate-level shape diagram.

---

### F13 — **polish** · `packet::ReferenceSuite` is public, load-bearing for `testutil`, and undocumented at the root

*Verified.* `src/packet/mod.rs:63` exports it; `src/testutil/mod.rs:967,
1029–1053` uses it as the default type parameter of `CountingIdentity` and as
the concrete suite behind all nine `Test*` aliases. It is not re-exported at
the crate root and lib.rs's `packet` bullet (`src/lib.rs:153–154`) does not
mention it, listing only *"the suite declaration, §6.1's handshake ladder as
a trait, the three headers, mac1 and §3.1's gate"*.

---

### F14 — **polish** · `Endpoint` exposes neither its local address nor its own static

*Judgement.* §16.2's `impl Endpoint` block names three functions and
**working rule 8 reads that list as exhaustive**, so the code is right to
stop there — I am not asking for a spec change. What I flag is the consumer
path: `EndpointBuilder::wire(w)` takes the socket **by value**, so after
`build()` there is no way to ask an endpoint bound to `:0` what port it got,
and no way to read back its own public static to send to a peer. Both are
recoverable — call `UdpSocket::local_addr()` and `Identity::public_static()`
*before* handing them to the builder — and neither `wire()`'s nor
`identity()`'s rustdoc (`src/shell/endpoint.rs:409–425`) says so. Two
sentences there would close it.

---

### F15 — **polish / note** · `EndpointBuilder::build()` panics, and `EndpointBuilder: Default` is public

*Verified.* `src/shell/endpoint.rs:472–498` — `build()` `expect()`s on a
missing identity or wire, panics outside a `LocalSet`, and panics if
`getrandom::fill` fails. All three are documented under `# Panics` (458–467)
and all three are defensible as programmer errors. The note is only that
`impl Default for EndpointBuilder<I, W>` (501–505) is public, so
`EndpointBuilder::<I, W>::default().build()` is a reachable, documented panic
with no compile-time guard. A typestate builder would remove it; that is a
bigger change than this stage of the project wants, and I would not make it
now.

---

### Things I checked and found correct — recorded because a clean result is information

*All verified by reading both sides.*

- **§16.11.1's table, all 15 rows**, against `src/compat/io.rs:33–94`. Exact,
  including `TimedOut` lifted into both columns and the `NotConnected` pair.
  Both `From` impls use `io::Error::new(kind, err)`, so
  `into_inner().downcast::<ReadError>()` recovers the reset code — pinned by
  a doctest at io.rs:104–111.
- **Ruling 58 (never claim ahead)** — every `poll_next` in
  `src/compat/stream.rs` is one `poll_*` call mapped to `Some`, with no
  buffer. The single exception is `MessageSink`'s one slot, which is on the
  **send** side and is documented as such (stream.rs:252–256).
- **Ruling 226 (Result faces never end)** — stated on the module
  (compat/mod.rs:53–61), on each of the seven constructors, **and** on each
  adapter type, each time with the `take_while` recipe and an explicit *"a
  bare `while let Some(_)` spins"*. `Incoming` correctly documents itself as
  the exception (stream.rs:309–314).
- **Ruling 231 (adapters borrow)** — a `# It borrows` section with a worked
  `spawn_local` snippet on all eight constructors. Modulo F7, this is the
  best-documented hazard in the crate.
- **Rulings 56/57** — `poll_flush` no-op and `poll_shutdown` =
  `finish()`-then-`acked()` both implemented and both carrying the rustdoc
  §16.11 demands (`src/compat/io.rs:137–178`).
- **S30's three-verb doc obligation** — present on `send_message`
  (connection.rs:728), `open_uni` (556) and `accept_uni` (621), *and* on
  `messages()`/`incoming_uni()` in compat (stream.rs:76–89, 183–194).
- **S7's claimed-vs-proven obligation** — `src/shell/staged.rs:145–158`, the
  address/index rung at 75–85, and `AuthError::Replay`'s note at
  `src/error.rs:142–178`. `Claimed`'s `Debug` deliberately omits the claimed
  static (staged.rs:281–282).
- **`#[non_exhaustive]` audit** — exactly two carry it: `WriteError`
  (error.rs:274, ruling 61) and `Notification` (shared.rs:609, §16.2). `Dir`
  deliberately does not (ruling 101, stream_id.rs:133–137). §18.1's taxonomy
  is closed everywhere else, which is what the spec says. The attribute's
  in-crate inertness is understood and pinned by
  `write_error_is_exhaustive_in_crate` (error.rs:343–355).
- **`std::error::Error` and `source()`** — all eleven error types derive
  `thiserror::Error`; the four that wrap `ConnectionLost` use
  `#[error(transparent)] #[from]`, so `source()` chains. The `Local` variants
  deliberately carry no inner error (ruling 79 routes it to §18.2's trace)
  and say so.
- **S21 / no `Send`** — `grep` for `: Send`, `+ Send`, `where … Send` across
  `shell/`, `compat/`, `identity.rs`, `config.rs`, `packet/` returns only
  `oneshot::Sender` and `SendStream` matches. **Zero `Send` bounds.**
  `serve()`'s bounds are `'static` only, and it uses `spawn_local`.
- **§6.2's staged block** — code matches SPEC.md:1232–1247 exactly, including
  `claimed_static` *"never `remote_static()`"* (the name is checked at
  staged.rs:233–236).

---

## 3. Story table, S1–S33

Legend: ✅ satisfied through the public API alone · ⚠ satisfied with a
caveat · ❔ uncertain.

| Story | Verdict | API involved |
|---|---|---|
| S1 open + close | ✅ | `Endpoint::connect` → `Connecting` → `Connection`; `close(code, reason)`; peer's `closed()` → `ConnectionLost::PeerClosed { code, reason }`; ours `LocallyClosed` |
| S2 dial nobody answers | ✅ | `Connecting` resolves `Err(ConnectError::TimedOut)` |
| S3a own redial refused | ✅ | `ConnectError::AlreadyConnected`; the `close`-then-dial recipe is doc obligation #1 at `lib.rs:47–82` |
| S3b peer replaces | ✅ | `ConnectionLost::Replaced` via `closed()` |
| S3c our dial refuses | ✅ | `AcceptError::Stale` from `Proven::accept`; `Notification::Contested` |
| S4 simultaneous open | ✅ | no verb needed; parity readable via `StreamId::initiated_by_connection_initiator` |
| S5 reaped / held open | ✅ | `set_persistent_keepalive(Option<Duration>) -> Result<(), ConfigError>`; `persistent_keepalive()` (ruling 189) makes "a rejected call leaves it unchanged" observable |
| S6 reject at 0 DH | ✅ | `Endpoint::accept() -> Option<Intro>`; `Intro::{source, sender_index}`; **drop** = reject |
| S7 inspect for 1 DH | ✅ | `Intro::read_identity() -> Claimed`; `Claimed::claimed_static()`; hazard rustdoc present |
| S8 park across turns | ⚠ **F5** | `Claimed` is owned, lifetime-free and not `Clone` ✔; the acceptance criterion's `#[must_use]` is **absent** |
| S9 prove then decline | ✅ | `Claimed::authenticate() -> Proven`; `Proven::{peer_static, timestamp, accept}` |
| S10 flood | ✅ | `Config::{with_intro_queue_cap, with_intro_max_per_source}` + getters; `constants::{INTRO_QUEUE_CAP, INTRO_MAX_PER_SOURCE, INTRO_TTL}` |
| S11 contested | ✅ | `Notification::{Contested, ContestCleared}` via `notified()` / `notifications()` |
| S12 stream | ✅ | `open_bi`/`open_uni`, `SendStream::{write, finish}`, `RecvStream::read` |
| S13 several streams | ✅ | as S12; `StreamId::{index, dir}` |
| S14 abandon a stream | ✅ | `SendStream::reset(code)`; peer gets `ReadError::Reset(code)`, sticky (ruling 121) |
| S15 datagram | ✅ | `send_datagram` (sync, never waits), `DatagramError::TooLarge`, `recv_datagram` |
| S16 single-shot message | ✅ | `send_message`, `MessageError::TooLarge`, `constants::MESSAGE_RECV_MAX` |
| S17 backpressure | ✅ | `write` parks on credit; no verb needed |
| S18 peer moves | ⚠ *story text* | `Notification::AddressMoved { from, to }`, `remote_address()`. STORIES.md:298 says *"The application observes `ConnEvent::AddressMoved`"* — `ConnEvent` is `pub(crate)` and unreachable. §16.2 / ruling 46 re-routed this to `Notification`, so **the code is right and the story text is stale**; flagged, not resolved |
| S19 we move / NAT rebind | ✅ | as S18, plus `set_persistent_keepalive` |
| S20 peer restarts | ✅ | S3b + S11's path; `AcceptError::Stale`, `ConnectionLost::TimedOut` |
| S21 Secure Enclave | ✅ | `Identity` with `type Provider: DhProvider<CurveOf<Self>>`, no `Send` anywhere (verified by grep); `block_on` for the `LocalSet` |
| S22 pick a suite | ⚠ **F12** | `channel!` + `Channel`/`Handshake`; the macro is barely discoverable from the crate root |
| S23 invisible rekey | ✅ | `Config::with_epoch_size`, `constants::REKEY_EPOCH_MSGS`; correctly emits **no** `Notification` |
| S24 kernel-free | ⚠ | `testutil::{Network, FlakyWire, FlakyPolicy}` behind `test-util`, attested at lib.rs:168–172. Satisfiable **only** via `testutil` — which is what the story asks for, so not a defect; noted because the brief asked me to flag `testutil`-only stories |
| S25 supply the wire | ✅ | `shell::wire::Wire` (`send_to`/`recv_from` over `io::Result`, no `Send`); `impl Wire for UdpSocket`. Its rustdoc carries **F4** |
| S26 drop every handle | ✅ | drop semantics on `Connection`/`SendStream`/`RecvStream`; doc obligation #4 at `lib.rs:137–140`; ruling 88's both-at-once case at stream.rs:1024–1026 |
| S27 await death / events | ✅ | `closed() -> ConnectionLost` (latched, cancel-safe), `notified() -> Result<Notification, ConnectionLost>`, `notifications()` |
| S28 acked then close | ✅ | `Connection::acked()` (snapshot) and `SendStream::acked()` |
| S29 cancel and redial | ✅ | drop `Connecting`; `Connect` owns the `Connecting` so the tower path cancels identically (tower.rs:100–105) |
| S30 messages ✗ uni streams | ✅ | rustdoc on all three verbs **and** both compat faces; `WriteError::Reset(constants::MESSAGE_OVERFLOW)` |
| S31 AsyncRead/AsyncWrite | ✅ | four ungated impls; `From<ReadError>`/`From<WriteError> for io::Error`; `poll_shutdown` = finish+acked |
| S32 codec | ✅ | `Connection::{framed_bi, accept_framed_bi}` under `codec`; ruling 58's one-item invariant holds in every `poll_next` |
| S33 tower | ✅ | three `Service` impls, `serve()`, and **`OpenBiOwned`** — which exists precisely so `UnsyncBoxService::new` (`S: 'static`) can accept it (ruling 239). The `!Send` caveat is rustdoc at tower.rs:21–44, as S33 demands |

**No story is satisfiable only through `pub(crate)`.** S24 is satisfiable
only through `testutil`, by design.

Observation, offered as data rather than a finding: story labels `S6`–`S10`
and `S23` appear **only** in the crate-internal suites (`src/core/tests.rs`,
`src/core/connection/tests.rs`), not in any file under `tests/`. The public
verbs they name *are* exercised from `tests/` (`read_identity` /
`authenticate` / `accept` in `story_lifecycle.rs`, `spec_shell.rs`,
`story_keepalive.rs`, `story_mobility.rs`, `story_tower.rs`; DH counting in
`story_dial.rs`, `story_lifecycle.rs`, `spec_shell.rs`). Whether the
story-level acceptance should sit at the shell rather than the core is a
maintainer call, not an API defect. `S24` appears nowhere as a label at all.

---

## 4. Conflicts found — reported, not resolved

1. **§16.2 vs §16.11 — `BiStream::join`.** SPEC.md:5112–5113 says
   `Result<Self, (SendStream, RecvStream)>`; SPEC.md:6219 says `Self`. Both
   ratified, both read by me. Code implements the former. → **F2**
2. **§16.11.1 (ruling 238) vs `src/compat/io.rs`.** The spec says the
   `WriteError` match is exhaustive and that a `_` arm hides the future; the
   code has `#[allow(unreachable_patterns)]` and `_ => Other`, citing the
   superseded ruling 227. → **F1**
3. **§16.4 / §16.2 vs `core::ConnEvent`.** The spec's enum lists 13 variants
   and §16.2's argument re-lists ten; the code has 14. The extra one,
   `SendCreditAvailable`, was authorised by ruling 150 (rulings.md:3833–3839)
   and never written into SPEC.md. → **F3**
4. **STORIES.md S18 vs §16.2.** STORIES.md:298 has the application observing
   `ConnEvent::AddressMoved`; `ConnEvent` is `pub(crate)` and §16.2 / ruling
   46 route it to `Notification::AddressMoved`. The code follows §16.2. Story
   text, not spec text — lowest stakes of the four.
5. **`src/shell/wire.rs:47–48` vs `src/shell/endpoint.rs:45`.** Doc says
   `Endpoint<W: Wire>`; the type is `Endpoint<I: Identity>`. → **F4**
6. **`src/compat/mod.rs:63–71` vs `src/compat/tower.rs:265`.** *"Every
   adapter borrows"* vs `OpenBiOwned<S>`, which does not. → **F7**

---

## 5. What I could not check, and why

- **I ran no `cargo` command at all.** The brief permits
  `build`/`check`/`test` and forbids anything that could touch
  `Cargo.toml`/`Cargo.lock`. `Cargo.lock` is not committed, is regenerated on
  every resolve, and the integrator is concurrently editing `Cargo.toml` in
  the same tree — so any cargo invocation could re-resolve under them. I
  judged the marginal value low against that risk and worked entirely from
  source. **Consequences:** I have **not** verified that the crate compiles
  at `cd12ed7`, that clippy is clean, or that
  `cargo doc --no-deps --all-features` under `RUSTDOCFLAGS=-D warnings`
  passes. Every intra-doc link I cite was checked by grepping for the
  target's definition, not by rustdoc. In particular I confirmed `channel!`
  exists (so lib.rs:278's link resolves) but did not machine-check the other
  intra-doc links.
- **Feature-combination compilation.** I read the `cfg` gates and confirmed
  the shapes (`codec` implies `sink`; `tower` pulls only `tower-service`;
  `sink` gates `compat::stream`), but I did not *build* `--features codec`
  alone, `--features tower` alone, or `--no-default-features`. A break that
  only appears in one combination would be invisible to me.
- **`benches/` and the working-copy `Cargo.toml`.** Off-limits under working
  rule 16. Anything the integrator is adding there — a new public item
  exercised by a bench, a new feature — is outside this review.
- **The `core` cores' own API.** §16.4 is one of my assigned sections, but
  `core` is `pub(crate)`, so most of §16.4 is not *public* surface. I checked
  §16.4's lists against the code where the shell reflects them (F3, and the
  stage-0 accessors, which are present as `Intro::source`/`sender_index`) and
  did not audit the core's internal signatures verb by verb.
- **Runtime semantics.** This is a surface review. I did **not** verify that
  `notified()` actually drains before reporting death, that `acked()`'s
  snapshot terminates, that ruling 58's one-item invariant holds *at runtime*
  (I verified it by reading each `poll_next`, which is a claim about the
  code's shape, not its behaviour), or any timer. The maintainer's own note —
  *drive the state and read the output* — applies: several of my ✅ verdicts
  say **the API can express the story**, not that the story passes.
- **Whether `core` should be public.** F11 reports that the recorded
  justification for `pub(crate)` has expired. Deciding it is a ruling, and the
  brief tells me to report rather than resolve.
- **Semver history.** slither 0.2.0 appears unpublished from the tree, so I
  treated every finding as "cheap now, breaking later" rather than checking
  against a released 0.1 surface.
- **`rulings.md` beyond ruling 150.** It is ~400 KB. I opened exactly the one
  ruling I make a claim about (150), per working rule 11 — *a citation is a
  claim about the cited text*. Where I cite rulings 44, 46, 50, 55–58, 61, 88,
  101, 116, 119–121, 124, 152, 189, 225–231, 238, 239 I am quoting **SPEC.md's
  or the code's** statement of them, and I have said so; I did **not** open
  those rulings, so if any is characterised wrongly in the spec or in a doc
  comment, I inherited the error.

---
---

## Appendix — raw notes log (appended during the read, kept for provenance)

### FOLLOW-UPs raised and closed during the read

- *Does `channel!` exist?* — **yes**, `src/packet/suite.rs:218`, the only
  `#[macro_export]` in the crate. lib.rs:278's link resolves. Became F12.
- *Is `Notification` `#[non_exhaustive]`?* — **yes**, `shared.rs:609`.
- *Are `Intro`/`Claimed`/`Proven` `Clone`?* — **no derives at all**. Half of
  S8 holds; became F5.
- *Is `Dir` dead surface?* — **no**, `StreamId::dir()` reaches it. Only
  `ConnectionId` and `IntroId` are. Became F10.

### `missing_docs` is `warn`, not `deny`

`src/lib.rs:197`. The docs gate is `RUSTDOCFLAGS=-D warnings`, which promotes
*rustdoc* lints — it does not promote rustc's `missing_docs`. So an
undocumented public item warns and does not block a gate. Not filed as a
finding: I found no undocumented public item. Recorded because the safety net
is thinner than the gate table suggests.

### §16.11's face list vs code — 8/8

`messages`, `datagrams`, `incoming_bi`, `incoming_uni`, `notifications`,
`incoming` (ruling 229), `message_sink`, `datagram_sink`. compat/mod.rs:30's
table says *"six `Stream`s and two `Sink`s"* — count checks out.

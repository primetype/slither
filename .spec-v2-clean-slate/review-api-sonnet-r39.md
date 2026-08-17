# slither Public API Review (sonnet)

## Verified base commit

Ran `git -C /Users/nicolasdiprima/work/primetype/slither log --oneline -1`.
Output: `cd12ed7 Ruling 242: SPEC.md is stamped RATIFIED, with its amendment table`

This matches the required base `cd12ed7`. Proceeding.

Concurrency notice acknowledged: will NOT read `Cargo.toml` or `benches/` from
the working copy; will use `git show cd12ed7:Cargo.toml` if needed. Will write
nothing into the repository. No `cargo add`/`update`.

---

## Working log / notes (raw, in order)

### STORIES.md read in full (668 lines)

33 stories, S1-S33, grouped A-I. Key API surface each story implies:

- S1: `connect(addr, static)` -> `Connecting` -> `Connection`; `close(code, reason)`;
  `ConnectionLost::PeerClosed{code,reason}`, `ConnectionLost::LocallyClosed`.
- S2: `Connecting` resolves `Err(ConnectError::TimedOut)` at 90s (HANDSHAKE_GIVEUP).
- S3: S3a `connect()` -> `Err(ConnectError::AlreadyConnected)`. S3b/S3c replacement
  semantics (internal, `replacement_basis`). `ConnectionLost::Replaced`.
- S4: simultaneous open converges to one connection (internal tie-break, not really
  API-visible beyond one Connection existing).
- S5: idle connection dies at DEAD_TIMEOUT(25s) -> `ConnectionLost::TimedOut`.
  `set_persistent_keepalive(Some(i)) -> Result<(), ConfigError>`, Err below 1s and
  at/above DEAD_TIMEOUT, rejected call leaves interval unchanged.
- S6: inbound -> `Intro` (0 DH). Reject costs 0 DH.
- S7: `Intro::read_identity()` -> `Claimed` + claimed static, 1 DH (es). Reject Claimed
  = 1 DH total.
- S8: `Claimed` is owned, `#[must_use]`, NOT Clone, parked across turns, expires at
  INTRO_TTL (15s).
- S9: `Claimed::authenticate()` -> `Proven` at 2 DH cumulative (ss). `accept()` on
  Proven -> live connection at 4 DH.
- S10: stage-0 queue capped 1024, 4/source, 15s TTL (internal enforcement, not nec. API).
- S11: contested notification via §16.2 notification stream (S27's stream) -
  application-visible via per-connection event, NOT core's internal ConnEvent.
- S12: streams - open, write, read, finish() delivers FIN.
- S13: concurrent streams independent.
- S14: reset a stream -> peer surfaces `ReadError::Reset(code)`.
- S15: `send_datagram` never blocks, drops oldest under pressure; oversize ->
  `DatagramError::TooLarge`.
- S16: single-shot message API, bound MESSAGE_RECV_MAX=262144 -> `MessageError::TooLarge`.
  Likely `send_message()`/`recv_message()`.
- S17: flow-control backpressure (mostly internal/observable via stalled poll).
- S18: `ConnEvent::AddressMoved`, `remote_address()` accessor. Mover must send via
  keepalive (S5 opt-in).
- S19: same as S18 but for our own address changing / NAT rebind.
- S20: peer restart -> reconnect within DEAD_TIMEOUT via S3b/S11 paths.
- S21: `!Send` DH provider requirement - architectural, not enumerable API item per se,
  but the Identity/DH provider trait must not require Send.
- S22: suite declared via `noise!` macro; mismatch fails closed, unknown version
  dropped silently.
- S23: epoch ratchet - fully internal, "no application-visible event" - so nothing
  should be in the public API for this (good - check nothing leaks it).
- S24: `testutil::FlakyWire`, `testutil::{Network, FlakyPolicy}` - test-util feature.
- S25: `Wire` trait: `send_to`/`recv_from` over `io::Result`, NOT required to be Send.
  Failing send traced (§18.2 operator contract / tracing), not surfaced as app error.
- S26: teardown on every handle dropped, not just Endpoint.
- S27: `Connection::closed().await` -> `ConnectionLost`. Per-connection notification
  stream distinct from blocking verbs - delivers `AddressMoved`, contested marking.
  Does NOT mirror core's ConnEvent (StreamReadable, MessageReadable excluded, already
  served by blocking verbs).
- S28: await acknowledgement of sent data before close - "transport-level ack" API,
  probably `acked()` on stream / message future, ties to ruling 47.
- S29: dropping `Connecting` cancels attempt (Drop impl), enabling immediate redial.
- S30: mixing `recv_message()` with `accept_uni()` is a programming error;
  `WriteError::Reset(MESSAGE_OVERFLOW)` code 0x06 on sender side; receiver traces only
  (§18.2 slither::frames target).
- S31: `BiStream` implements `AsyncRead`/`AsyncWrite` (compat feature). `poll_shutdown`
  = finish() then acked(). `poll_flush` no-op Ready (ruling 56). Errors as io::Error,
  peer reset -> `ConnectionReset` ErrorKind.
- S32: `Framed<BiStream, LengthDelimitedCodec>` codec support (codec feature),
  Stream/Sink impls, backpressure maps to flow control, one-item-per-poll invariant
  (ruling 58).
- S33: `tower::Service` impl (tower feature) - one bi stream per call; `serve()`
  drives accepting side; `UnsyncBoxService` works, `BoxService`/`Buffer`/`spawn_ready`
  do not (must be documented, !Send boundary).

Documentation obligations explicitly called out (must appear in rustdoc, not just spec):
1. S3a: close()-then-dial, not connect() again.
2. S7: denylist-on-claimed-static hazard, at read_identity() rustdoc.
3. S5: connect-ahead-of-need loses connection at 25s - must be prominent in crate docs.
4. S26: teardown triggers on every handle drop, not endpoint drop - drop-order note.
5. S30 (⚠ CHECK): docs on send_message, open_uni AND accept_uni about the
   message/uni-stream mixing hazard.

Now reading src/lib.rs and module structure.

### src/lib.rs read in full (293 lines)

Crate root re-exports at top level:
```
pub use compat::block_on;
pub use config::{Config, SystemClock, WallClock};
pub use error::{AcceptError, AuthError, ConfigError, ConnectError, ConnectionLost,
  DatagramError, IntroError, MessageError, ReadError, WriteError};
pub use identity::{CurveOf, Identity, PrivateKeyOf, PublicKeyOf, SoftwareIdentity};
pub use packet::{Channel, Handshake};
pub use shell::{BiStream, Claimed, Connecting, Connection, Endpoint, EndpointBuilder,
  Intro, Notification, Proven, RecvStream, SendStream};
pub use hiss::noise::SessionId;
pub use crate::core::{ConnectionId, Dir, IntroId, StreamId, Timestamp};
pub use hiss;
```
Public modules: compat, config, constants, error, identity, packet, shell. `core` is
pub(crate) (deliberate per doc comment - not stable until driver is public + RNG-seed
build is security-relevant). `testutil` gated `#[cfg(any(test, feature = "test-util"))]`.

`#![forbid(unsafe_code)]`, `#![warn(missing_docs)]` (NOTE: `warn` not `deny` - so
missing docs would not fail a normal build; only caught by manual inspection or if CI
runs with `-D warnings` via RUSTFLAGS. The release gate table says
`RUSTDOCFLAGS=-D warnings` for `cargo doc`, which is a different flag
(RUSTDOCFLAGS vs RUSTFLAGS) - RUSTDOCFLAGS -D warnings affects doc-link warnings during
`cargo doc`, not the `missing_docs` lint on a normal build, though `missing_docs` does
also fire during `cargo doc` generation... need to verify whether missing_docs is
still `warn` and thus doesn't block gate. This is worth flagging - checking further.)

The 5 documentation obligations from STORIES.md are embedded verbatim as a numbered doc
section in lib.rs (matches S3a/S7/S5/S26/S30 documentation obligations) - confirmed
present and thorough, with runnable `no_run` doctest example for obligation 1.

`channel!` macro confirmed real (src/packet/suite.rs) - not a broken link, resolves the
earlier concern.

`core` types re-exported for reachability: `ConnectionId, Dir, IntroId, StreamId,
Timestamp` - despite `core` mod being pub(crate), these specific types are re-exported
directly at crate root via `pub use crate::core::{...}`.

### src/shell/mod.rs (partial, header + re-exports) and src/shell/wire.rs (full, 152 lines)

shell/mod.rs pub surface:
```
pub mod wire;
pub use self::connection::Connection;
pub use self::endpoint::{Connecting, Endpoint, EndpointBuilder};
pub use self::shared::Notification;
pub use self::staged::{Claimed, Intro, Proven};
pub use self::stream::{BiStream, RecvStream, SendStream};
```

`Wire` trait (src/shell/wire.rs) - matches S25 exactly: `async fn send_to(&self, buf:
&[u8], addr: SocketAddr) -> io::Result<usize>`, `async fn recv_from(&self, buf: &mut
[u8]) -> io::Result<(usize, SocketAddr)>`. Not Send-bound (uses `async fn` in trait,
`#[allow(async_fn_in_trait)]`), deliberately not dyn-compatible (documented). Blanket
impl for `tokio::net::UdpSocket`. Well documented with normative properties numbered.
Confirms S25's Wire trait requirement satisfied AS WRITTEN. Good example of doc
quality - this file is exemplary.

Full pub API of shell/{endpoint,connection,stream,staged}.rs (grep results, need line
content still for signatures):

endpoint.rs:
- `struct Endpoint<I: Identity>`
- `Endpoint::builder<W: Wire>() -> EndpointBuilder<I, W>`
- `Endpoint::accept(&self) -> Option<Intro<I>>` (async) — NOTE: returns Option, not
  Result. Need to check what None means (endpoint torn down?) vs error.
- `Endpoint::connect(...)` (NOT async per grep - matches doc claim it's sync returning
  Connecting immediately, "not async because mint_pending costs 0 DH")
- `struct Connecting<I: Identity>`
- `struct EndpointBuilder<I: Identity, W: Wire>`
- `EndpointBuilder::identity/wire/config/rng_seed/build`

connection.rs (struct Connection<S: Handshake>):
- close(&self, code: u64, reason: &[u8]) (async, no return value shown - "resolves
  once CLOSE is sealed" per S1, matches doc example `stale.close(...).await;` with no
  `?`)
- closed(&self) -> ConnectionLost (async)
- notified(&self) -> Result<Notification, ConnectionLost> (async) -- matches S27
- set_persistent_keepalive(&self, interval: Option<Duration>) -> Result<(), ConfigError>
  -- matches S5 exactly
- persistent_keepalive(&self) -> Option<Duration>
- acked(&self) -> Result<(), ConnectionLost> (async) -- matches S28 (connection-level ack)
- open_bi/open_uni/accept_bi/accept_uni -> Result<_, ConnectionLost> (async)
- send_message(&self, msg: &[u8]) -> Result<(), MessageError> (async)
- recv_message(&self) -> Result<Vec<u8>, ConnectionLost> (async) -- NOTE: error type is
  ConnectionLost not MessageError - asymmetric with send_message. Need to check why
  (maybe recv can't hit MessageError user-side - TooLarge is a receiver enforcement
  concern producing a reset seen elsewhere, not surfaced as MessageError to the
  receiver). Worth checking against S16/S30.
- send_datagram(&self, data: &[u8]) -> Result<(), DatagramError> (NOT async, matches S15
  "never waits")
- recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost> (async)
- remote_static(&self) -> PublicKeyFor<S>
- remote_address(&self) -> SocketAddr
- session_id(&self) -> hiss::noise::SessionId
- is_established(&self) -> bool

stream.rs:
- struct SendStream<S: Handshake>: id() -> Option<StreamId>, write(&mut self, buf) ->
  Result<usize, WriteError> (async), finish(&mut self) -> Result<(), WriteError>
  (async), acked(&mut self) -> Result<(), WriteError> (async) -- matches S28 stream-level,
  reset(&mut self, error_code: u64) (NOT async, sync fire-and-forget, matches S14)
- struct RecvStream<S: Handshake>: id(), read(&mut self, buf: &mut [u8]) ->
  Result<Option<usize>, ReadError> (async) -- Option<usize> read convention (None = EOF)
- struct BiStream<S: Handshake>: id(), split(self) -> (SendStream<S>, RecvStream<S>),
  join(...)

staged.rs (the S6-S9 ladder):
- struct Intro<I: Identity>: source() -> SocketAddr, sender_index() -> u32,
  read_identity(mut self) -> Result<Claimed<I>, IntroError> (async, consumes self by
  value - good, prevents reuse)
- struct Claimed<I: Identity>: claimed_static() -> &PublicKeyOf<I>,
  authenticate(mut self) -> Result<Proven<I>, AuthError> (async, consumes self)
- struct Proven<I: Identity>: peer_static() -> &PublicKeyOf<I>, timestamp() ->
  Timestamp, accept(mut self) -> Result<Connection<I::Suite>, AcceptError> (async,
  consumes self)

Note: Intro/Claimed/Proven have no visible explicit "reject" method in the grep -
rejection appears to be implicit via Drop (matches S6 "an application decides... 0 DH"
- presumably dropping the Intro/Claimed/Proven IS the rejection). Need to verify: is
there a Drop impl, and does dropping actually signal rejection to the core (vs. just
leaking state until INTRO_TTL expiry)? This matters for S6's "costs 0 DH" claim: if
rejection is silent drop with no wire signal, that's consistent with spec description.
Need to check Drop impls.

### VERIFIED: src/shell/staged.rs read in full (382 lines)

Confirms: `Intro`, `Claimed`, `Proven` each have `impl Drop` that calls
`Command::Reject(id)` if not `consumed`. Doc comment: "Dropping is the rejection...
There is no `reject()` verb because there is nothing for one to do that `drop` does
not." Matches S6/S7/S8/S9's staged-ladder story.

Cancel-safety carefully documented on each async verb (dropping the future during
await also rejects via the `consumed` flag pattern - the flag is only set to `true`
*after* the round-trip resolves, so a cancelled future's Drop still fires the object's
own outer Drop with `consumed == false`... wait, need to check: `round_trip` uses
`shell.send(...)`; then `self.consumed = true` happens AFTER `.await` in the async fn
body. If the *future itself* is dropped mid-await (e.g. via `select!` or timeout), the
async fn body's stack frame drops, including `self` (moved into the fn as `mut self`),
so `self`'s Drop impl runs with `consumed == false` → rejects. Confirmed correct and
matches documented cancel-safety claims.

**FINDING CANDIDATE (should-fix):** STORIES.md S8 states explicitly, as an "Accepts"
criterion: *"It is `#[must_use]` and not `Clone`, so a parked chain cannot be forked."*
(referring to `Claimed`). Verified by reading src/shell/staged.rs in full: none of
`Intro` (line 50), `Claimed` (line 223), `Proven` (line 292) carry `#[must_use]`.  All
three lack `Clone` (correct, matches the "not Clone" half). The "not Clone" half of
the criterion holds; the "#[must_use]" half does not, as literally read. Practical
impact is narrowed because `read_identity()`/`authenticate()`/`accept()` all return
`Result<_,_>` and `Result` itself is `#[must_use]` in std, so the most common
call-and-immediately-discard mistake (`intro.read_identity().await;` as a bare
statement) is *already* caught by `Result`'s own must_use. But there remain
uncaptured cases the story's literal text closes off: e.g. a helper that unwraps and
returns a bare `Claimed<I>` and is then called as a statement,
`get_next_claimed(&mut queue);`, would silently drop (= reject) without a warning,
where `#[must_use]` on the struct itself would catch it. Given STORIES.md is the
acceptance criteria and states this attribute explicitly and testably, and it is
absent, flagging as **should-fix** (not blocker, since the primary hazard is caught
by Result's must_use already; not polish, since it is an explicit named acceptance
criterion currently false in the code).

Also noted: `Debug` impls are hand-written (not derived) for `Intro`/`Claimed`/
`Proven`, deliberately suppressing the raw claimed_static on `Claimed` (documented
rationale: avoid encouraging logging of an unauthenticated claim). `Intro`'s Debug
does print source()/sender_index() "live" values - has a long comment acknowledging
this is a live judgement call, not obviously wrong, and explicitly flagged in-code
as "deliberately not answered" — consistent with working rule 3 (report conflicts,
don't resolve) applied by the implementer themselves. Good practice.

Cargo.toml (read via `git show cd12ed7:Cargo.toml`, NOT working copy, per
concurrency notice) confirms feature set: default = [], test-util, sink
(futures-core/futures-sink), codec (implies sink, + tokio-util), tower
(tower-service only). Matches src/lib.rs's feature table exactly. Also confirms
edition 2024, rust-version 1.96, and the full [[test]] stanza list (11 story/spec
integration test files under test-util etc.)

tests/ dir listing (18 files): spec_compat.rs, spec_constants.rs, spec_errors.rs,
spec_packet.rs, spec_shell.rs, spec_streams.rs, story_codec.rs, story_compat.rs,
story_datagram.rs, story_dial.rs, story_keepalive.rs, story_lifecycle.rs,
story_message.rs, story_mobility.rs, story_path.rs, story_reliability.rs,
story_streams.rs, story_tower.rs.

### VERIFIED: src/config.rs and src/identity.rs read in full (240 + 285 lines)

config.rs: `WallClock` trait (`now() -> Timestamp`), `SystemClock` (Debug, Clone,
Copy, Default), `Config` (Clone, hand-written Debug via finish_non_exhaustive - Rc<dyn
WallClock> can't derive Debug). Builder methods (`with_intro_queue_cap`,
`with_intro_max_per_source`, `with_epoch_size`, `with_clock`) all correctly
`#[must_use]` (consuming `self -> Self` builders - correct use of the attribute,
contrasts with the missing #[must_use] on Intro/Claimed/Proven noted earlier).
Accessors (`intro_queue_cap`, `intro_max_per_source`, `epoch_size`, `clock`) plain
getters, well named, no `get_` prefix (correct per Rust API guidelines). `Rc` not
`Arc` deliberately documented (keeps Config !Send-compatible, consistent with S21).
Config uses `Rc<dyn WallClock>` for the clock — no Send/Sync issue since actor is
!Send throughout. Well done module.

identity.rs: `Identity` trait, `CurveOf<I>`/`PublicKeyOf<I>`/`PrivateKeyOf<I>` type
aliases (good naming, matches lib.rs re-export list exactly:
`CurveOf, Identity, PrivateKeyOf, PublicKeyOf, SoftwareIdentity`). `Identity::open`
returns `Result<(Provider, PrivateKey), Self::Error>` - a factory, well justified
(hiss consumes provider+key by value per-handshake). No Send bound anywhere -
confirmed matches S21 exactly, and rationale for excluding DhProviderAsync given (3
independent reasons) - excellent depth.

`SoftwareIdentity<S, R = ChaCha20Rng>`: Debug hand-written (`finish_non_exhaustive`,
scalar never printed - good, avoids leaking secret in Debug output). No `Clone` (data
holds a raw scalar/secret + RefCell<R> - correctly not Clone since that would
duplicate a private key; consistent with "secret keys should not be silently
duplicated" from hiss). `from_scalar`/`generate` constructors with careful docs about
RNG must be OS-seeded in production - the note is thorough about consequences of a
predictable RNG in each constructor (static-key compromise vs. ephemeral/forward-
secrecy compromise) - genuinely excellent documentation depth for a crypto-adjacent
API surface.

`SoftwareIdentityError`: derives only `Debug, thiserror::Error` (no Clone/PartialEq/Eq)
- asymmetric with the ten §18.1 taxonomy error types, which all derive
Debug+Clone+PartialEq+Eq. Minor inconsistency (this type is outside the closed
taxonomy so the rule doesn't strictly bind it) - noting as a polish-level candidate,
need to check impact: `Identity::Error` is bounded `::core::error::Error + 'static`
only, so a consumer's own Identity impl's Error type isn't required to be Clone
either — this specific type not being Clone doesn't break any generic bound. Likely
not worth a finding; skipping unless it clearly matters after cross-checking usages
of ConnectError::Local/IntroError::Local etc (which do NOT carry the identity's
error type at all, per ruling 79's documented design - "detail re-addressed to trace,
not variant"). So SoftwareIdentityError's traits are essentially private impl choice.
Not flagging as a finding.

### VERIFIED: src/error.rs read in full (539 lines)

Ten error types total: `ConnectError` (3 variants), `IntroError` (5), `AuthError`
(5), `AcceptError` (2), `ConnectionLost` (7), `WriteError` (3, `#[non_exhaustive]`),
`ReadError` (2), `MessageError` (2), `DatagramError` (2), `ConfigError` (2, outside
§18.1 by ruling 44).

All derive `Debug, Clone, PartialEq, Eq, thiserror::Error`. All Send+Sync (asserted
by test `error_types_are_send_and_sync`). `ConnectionLost` clone is a documented hard
requirement (fan-out to N holders). Only `WriteError` carries `#[non_exhaustive]`
(ruling 61, reserved for future `Stopped` variant per §19 STOP_SENDING). Doc
explicitly warns future maintainers not to spread `#[non_exhaustive]` to the other
nine, and correctly notes `#[non_exhaustive]` has no effect within the defining
crate (matches brief's warning about this exact mistake) - has an in-crate
exhaustiveness fence test `write_error_is_exhaustive_in_crate` specifically because
of that, with commentary explaining why an out-of-crate fence over
`#[non_exhaustive]` can't work. This is careful, correct engineering.

No `SlitherError` umbrella, no `Result<T>` alias - both deliberate per module doc,
reasoned (avoids re-opening taxonomy / picking a favorite). Every Display string
tested for house style (lowercase start, no trailing period) via
`display_strings_are_non_empty_and_lowercase_initial`. `#[error(transparent)]` on
ConnectionLost-embedding variants forwards `source()` correctly (tested).

`recv_message()` returning `Result<Vec<u8>, ConnectionLost>` (not `MessageError`) now
makes sense: `MessageError::TooLarge` only applies to the *sender's* own
oversize check on `send_message`: a receiver can never observe an inbound message
larger than MESSAGE_RECV_MAX because the wire-level guard (S30's MESSAGE_OVERFLOW
reset) prevents one from ever completing — so `MessageError` would have an
unreachable variant if used as recv_message's error type. This resolves my earlier
open question; NOT a defect.

This claims **§18.1 names nine error types exactly** (ConnectError, IntroError,
AuthError, AcceptError, ConnectionLost, WriteError, ReadError, MessageError,
DatagramError = 9, plus ConfigError as the tenth/excluded one) - I have NOT yet
independently verified this count against SPEC.md §18.1 itself (a claim about the
spec requires opening the spec per working rule 11/CLAUDE.md rule 4b). Queued to
check via grep+Read on SPEC.md.

### VERIFIED: src/compat/{mod,io,stream,codec,rt,tower}.rs read in full (~1524 lines)

All excellent, thoroughly documented, matches S31/S32/S33 acceptance criteria
essentially line-for-line:
- io.rs: `From<ReadError>`/`From<WriteError>` for `io::Error` preserve inner error
  (verified via doctest in the file itself: `e.into_inner().downcast::<ReadError>()`).
  §16.11.1 ErrorKind mapping present with a detailed ruling-227 note about
  `#[non_exhaustive]` being inert in-crate requiring a defensive (not
  `unreachable!()`) `_` arm mapping to `Other`. `AsyncRead`/`AsyncWrite` on
  SendStream/RecvStream/BiStream, `poll_shutdown` = finish() then acked() (matches
  S31 exactly, ruling 57). `poll_flush` no-op Ready (matches S31/ruling 56 exactly,
  with explicit "not delivery confirmation" callout).
- stream.rs (feature "sink"): `Connection::{messages, datagrams, incoming_bi,
  incoming_uni, notifications, message_sink, datagram_sink}`,
  `Endpoint::incoming()`. Every `Stream::poll_next` impl calls its underlying
  poll_* exactly once (ruling 58/S32 "exactly one item per poll" - verified by
  reading every impl body, all one-line delegations, no internal buffering except
  MessageSink's documented single slot). Every adapter struct carries a lifetime
  'a (ruling 231/"every adapter borrows" - verified: Messages<'a,S>,
  Datagrams<'a,S>, IncomingBi<'a,S>, IncomingUni<'a,S>, Notifications<'a,S>,
  Incoming<'a,I>, MessageSink<'a,S>, DatagramSink<'a,S> all hold `&'a
  Connection<S>` or `&'a Endpoint<I>`). "Never ends" (ruling 226) documented and
  implemented correctly on every Result-item stream; Endpoint::incoming() is the
  one exception (Item = Intro, not Result, ends on driver stop) and this is
  called out explicitly in its own doc section.
- codec.rs (feature "codec"): `Connection::{framed_bi, accept_framed_bi}` -
  trivial wrappers around open_bi/accept_bi + Framed::new. Matches S32.
- rt.rs: `block_on` - current-thread runtime + LocalSet, documented panics
  (runtime-build failure, nested-runtime call), full worked doctest example
  covering S31's shape (open_bi, write, finish, acked).
- tower.rs (feature "tower"): `Service<(SocketAddr, PublicKeyOf<I>)> for
  Endpoint<I>` (dialer, `Connect<I>` future); `Service<()> for &'a Connection<S>`
  (OpenBi<'a,S>, borrowed) AND `Service<()> for Connection<S>` (OpenBiOwned<S>,
  OWNED, added later per ruling 239 reversing ruling 236, specifically to satisfy
  S33's "UnsyncBoxService composes" criterion since UnsyncBoxService requires
  `'static`); `serve()` free fn, spawn_local only, returns `ConnectionLost`
  directly (deliberately not `Result<(), ConnectionLost>` since Ok is
  unreachable - documented rationale). Extensive doc on the !Send/Send-façade
  caveat, naming exactly what does/doesn't compose
  (UnsyncBoxService yes; tower::buffer::Buffer, spawn_ready, BoxService, hyper,
  tokio::spawn no) - matches S33's rustdoc-not-folklore criterion verbatim.

Verified `clone_handle` (used by the owned tower Service impl) is `pub(crate)`
(src/shell/connection.rs:113) - so `Connection` remains correctly non-`Clone`
publicly, consistent with stream.rs/mod.rs's repeated claim "neither Connection
nor Endpoint is Clone".

No Debug impls on any compat adapter type (Messages, Datagrams, IncomingBi, etc.)
- likely acceptable/expected (these hold boxed closures / borrows that don't
carry useful Debug info, similar to tokio_stream/futures adapter conventions);
noting as a very minor polish candidate, not filing as a separate finding given
low real-world impact and precedent in the ecosystem.

Remaining to read/check: shell/{endpoint,connection,stream}.rs full bodies (doc
comments + edge cases beyond signatures already grepped), shared.rs's Notification
enum body, constants.rs (skim), and cross-check against SPEC.md §16.2/16.4/16.11
+ §18.1 (error-type count claim) via grep+Read with offset/limit.

---

## Findings

(severity: blocker / should-fix / polish)

**Index:** Finding 1 (blocker, `src/compat/io.rs:74-93`) below; Finding 2
(should-fix, `src/shell/staged.rs` — missing `#[must_use]`) and Finding 3
(should-fix, `src/shell/wire.rs` doc-accuracy) appear later in this section,
in the order they were found rather than resorted, to keep the working-log
trail intact.

### FINDING 1 (blocker) — `src/compat/io.rs:74-93`'s `write_kind` still carries the `_ =>` arm that ruling 238 explicitly ratifies deleting

**Verified by reading both artefacts** (working rule 11): SPEC.md §16.11.1
(read at offset 6228-6284) and `.spec-v2-clean-slate/rulings.md`'s ruling 238
entry (read at offset 6814-6843), cross-checked against the actual code at
`src/compat/io.rs:74-93`.

SPEC.md §16.11.1, under "Two binding details, both directly testable", states
verbatim (this is the RATIFIED, current spec text, corrected 2026/08/16 by
ruling 238, and ruling 238 is listed in SPEC.md's own header amendment table
per ruling 242, so this correction is confirmed to be part of the ratified
document at commit cd12ed7):

> **The `WriteError` match is exhaustive, and deliberately so.**
> [...] `#[non_exhaustive]` is **inert inside the defining crate** [...]
> A `_ =>` arm therefore does not future-proof the conversion — it **hides**
> the future. An exhaustive match turns the day `Stopped` lands into a
> **compile error at the exact site that must be updated** [...]

Ruling 238's own text is unambiguous about the code change required: *"the
arm is deleted and the match is exhaustive."*

But `src/compat/io.rs` (read in full) still reads, at lines 74-93:

```rust
#[allow(unreachable_patterns)]
fn write_kind(err: &WriteError) -> io::ErrorKind {
    match err {
        WriteError::Reset(_) => io::ErrorKind::ConnectionReset,
        WriteError::Finished => io::ErrorKind::BrokenPipe,
        WriteError::ConnectionLost(lost) => match lost { ... },
        // **[ruling 227]** `WriteError` is `#[non_exhaustive]` ...
        _ => io::ErrorKind::Other,
    }
}
```

The code's own comment above the `_` arm still argues **for** keeping it,
citing ruling 227 and asserting "the conclusion survives; only the reason
does" — which is exactly the position ruling 238 overturns ("Reviewing the
agent's finding changes the conclusion as well as the reasoning"). The code
was not updated when the spec was corrected.

**Why it matters to a consumer:** functionally near-invisible today (no
`WriteError::Stopped` variant exists yet), so no wrong `io::ErrorKind` is
observed today. But per CLAUDE.md's hard rule ("the code must match the
spec — never the other way round") this is a confirmed, unambiguous
drift: the exact defect ruling 238 exists to prevent — a future `Stopped`
variant silently compiling to `Other` instead of forcing a compile error
at this site — is still live in the shipped code. `-D warnings` does not
catch it today only because `#[allow(unreachable_patterns)]` suppresses
the very lint that would otherwise flag the dead arm ruling 238 says to
delete.

**Suggested fix (mechanical, per the ruling's own text):** delete the
`#[allow(unreachable_patterns)]` line and the `_ => io::ErrorKind::Other,`
arm; the match over `Reset`/`Finished`/`ConnectionLost(..)` is already
exhaustive without it.

**This is not a "conflicting statements, pick one" case** — ruling 238 is
dated after ruling 227, explicitly overturns it, and is reflected in
SPEC.md's own text and amendment-table. There is no live prose/formal
tension here; the spec is unambiguous and the code simply has not caught
up. Flagged as **blocker** because it is a concrete, mechanical
spec-vs-code mismatch on a point the maintainer has already ruled on in
terms, not a judgement call.

I could not determine from the repository whether this drift is already a
known/tracked item pending a follow-up commit; I report it as found at
commit cd12ed7 regardless.

---

### VERIFIED: SPEC.md §18.1 (offset 6655-6780) read in full, cross-checked against src/error.rs

Every claim in error.rs's module doc and every variant/count matches §18.1
exactly: ConnectError{AlreadyConnected,TimedOut,Local}=3,
IntroError{Expired,Internal,Malformed,Local,EndpointDropped}=5,
AuthError{Replay,HandshakeFailed,Expired,Local,EndpointDropped}=5,
AcceptError{Stale,EndpointDropped}=2,
ConnectionLost{TimedOut,NonceExhausted,LocallyClosed,PeerClosed{code,reason},
ProtocolViolation{code},Replaced,EndpointDropped}=7,
WriteError{Reset(u64),ConnectionLost(ConnectionLost),Finished}=3
(#[non_exhaustive], confirmed "WriteError alone" per ruling 61's text),
ReadError{Reset(u64),ConnectionLost}=2, MessageError{TooLarge,ConnectionLost}=2,
DatagramError{TooLarge,ConnectionLost}=2. `ConfigError` confirmed "outside
§18.1 entirely, by ruling 44" in the spec's own words, matching error.rs's
claim verbatim. `AcceptError::AlreadyConnected` confirmed absent from both
spec and code ("appears nowhere... the variant is unreachable and deleted").
**No discrepancy found — error.rs is a faithful, fully verified
implementation of §18.1.**

### VERIFIED: SPEC.md §16.2 (5035-5459), §16.4 (5697-5949), §16.11+16.11.1
(6198-6360) read in full, cross-checked exhaustively against shell/*.rs and
compat/*.rs

§16.2's `Connection` impl code block lists exactly 18 methods in this order:
open_bi, open_uni, accept_bi, accept_uni, send_message, recv_message,
send_datagram, recv_datagram, acked, close, set_persistent_keepalive,
persistent_keepalive, closed, notified, remote_static, remote_address,
session_id, is_established. **src/shell/connection.rs's pub fn list (verified
by grep earlier, cross-checked again here) contains exactly these 18 and no
others.** Exhaustive match, no extra verb, nothing missing — this is a clean
pass on working rule 8's "list is exhaustive" defect class.

`Endpoint`: builder/accept/connect — all 3 present, `connect()` confirmed
NOT async (verified by reading src/shell/endpoint.rs:190-223 directly),
matching §16.2's `pub fn connect(...)  -> Result<Connecting, ConnectError>`
signature (non-async) exactly, and matching the code's own documented
rationale (0-DH first half on the shared cell, ruling 87/90).

`SendStream` (write, finish, acked, reset, id = 5), `RecvStream` (read, id =
2), `BiStream` (split, join, id = 3) — all exact matches to §16.2's code
block, no extra, nothing missing.

`Notification` enum: verified in src/shell/shared.rs:609-611 and body
(609-639ish) — `#[non_exhaustive]`, `#[derive(Debug, Clone, PartialEq, Eq)]`,
exactly 3 variants `AddressMoved{from,to}`, `Contested`, `ContestCleared` —
matches §16.2's code block and §16.4's ConnEvent-to-Notification translation
description exactly.

§16.4 (the pub(crate) core API) mentions "stage-0 accessors ([RATIFIED
2026/08/15, ruling 71]): fn intro_source(&self, id) -> Option<SocketAddr>;
fn intro_sender_index(&self, id) -> Option<u32>" — this is the exact
historical omission the brief warned about (§16.4 once omitted these). VERIFIED
PRESENT both at the core level (used internally, per staged.rs's
`self.shell.state.borrow().endpoint.intro_source(self.id)`) and correctly
surfaced at the public level as `Intro::source()` / `Intro::sender_index()`
(src/shell/staged.rs:99, 122). No gap.

§16.11.1's full io::ErrorKind mapping table (9 rows) verified cell-by-cell
against src/compat/io.rs's `read_kind`/`write_kind` — exact match on every
cell (see the one exception noted as Finding 1: the `_=>` arm ruling 238
says to delete is still present, though the *mapping value* itself, `Other`,
is not a table cell mismatch — the table has no row for a hypothetical
future `Stopped`, so this is purely the "arm should not exist" defect,
not a wrong-value defect).

§16.11's composability list: "named seven [faces]... `incoming` is the
eighth" (ruling 229). Verified: Connection has exactly 7 constructor methods
in compat/stream.rs (messages, datagrams, incoming_bi, incoming_uni,
notifications, message_sink, datagram_sink) + Endpoint::incoming() = 8 total.
Exact match.

**No further discrepancies found between SPEC.md §16.2/16.4/16.11/16.11.1/18.1
and the code, aside from Finding 1 above.** This is a very clean, carefully
maintained codebase — the one drift found is narrow and mechanical.

### VERIFIED: `cargo doc --no-deps --all-features` with `RUSTDOCFLAGS=-D warnings`, and `cargo clippy --all-features --all-targets -- -D warnings`

Both commands run (explicitly permitted per the brief; Cargo.toml/benches/ not
touched, no `cargo add`/`update` run). Output:

```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
 Documenting slither v0.2.0 (...)
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.72s
   Generated .../target/doc/slither/index.html
```
```
$ cargo clippy --all-features --all-targets -- -D warnings
   Checking slither v0.2.0 (...)
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.21s
```

Both clean, zero warnings. Confirms full public doc coverage (missing_docs
would fire under rustdoc's `-D warnings` and did not) and confirms clippy is
clean including on the `#[allow(unreachable_patterns)]` arm Finding 1 flags
(the `allow` is precisely why clippy does not catch that one on its own —
consistent with Finding 1 being a spec-conformance gap rather than a
build/lint failure).

### VERIFIED: story test coverage, cross-checked by grepping `tests/*.rs`

`CountingProvider`/`DhCounter` confirmed present and `pub` in
`src/testutil/mod.rs` (behind `test-util`), satisfying S6/S7's explicit "Test
bar: a counting DH provider asserts `dhs == 0`" / "`dhs == 1`" requirement.
Story-numbered test functions (`sN_...`) found for S1, S2, S4, S5, S11-S14,
S16-S20, S26-S33 (105 total `sN_*` functions across `tests/*.rs`).
S3/S6-S10/S21-S25 do not use the `sN_` naming convention but are covered by
descriptively-named tests in `tests/spec_shell.rs` (e.g.
`connect_is_synchronous_so_a_pending_static_refuses_before_any_await` for
S3a, `dropping_an_intro_is_a_silent_reject_and_costs_nothing` for S6/S8,
`the_staged_ladder_charges_and_transmits_only_at_its_own_verb` for S6-S9's DH
pricing, `dropping_a_proven_transmits_nothing` for S9,
`a_staged_object_does_not_keep_the_driver_alive` for S8,
`a_failing_send_does_not_kill_the_connection_and_the_death_stays_timed_out`
for S25, `session_id_agrees_across_peers_and_differs_across_sessions` for
S22-adjacent). S15 uses `sdN_` naming in `tests/story_datagram.rs` (11
tests). I did not read every one of these test bodies end-to-end (time
budget); I confirmed their existence and names align with the stories, which
is a lighter-weight check than reading each assertion.

### FINDING 3 (should-fix) — `src/shell/wire.rs`'s doc claims `Endpoint<W: Wire>`; the actual type is `Endpoint<I: Identity>` with no `W` parameter at all

**Verified by reading both**: the doc comment (src/shell/wire.rs, read in
full) and the struct definition (src/shell/endpoint.rs:45, read directly).

`src/shell/wire.rs`, in the "This trait is deliberately not dyn-compatible"
section, states:

> The price is that there is no `Box<dyn Wire>`: **an endpoint is generic
> over its wire (`Endpoint<W: Wire>`), not erased.**

But `src/shell/endpoint.rs:45` reads:

```rust
pub struct Endpoint<I: Identity> {
    shell: Shell<I>,
    ...
}
```

`Endpoint` has exactly one type parameter, `I: Identity`, and no `W`
parameter anywhere in its definition. `W: Wire` only appears on
`EndpointBuilder<I: Identity, W: Wire>` (src/shell/endpoint.rs:392); once
`EndpointBuilder::build()` runs, the concrete `Wire` is moved into the
spawned `Driver<I, W>` task and the returned `Endpoint<I>` handle carries no
`W` at all — communication with the driver crosses a command channel, not a
generic parameter.

**Why it matters:** this is a claim about a public type's signature, stated
in the rustdoc of the very trait (`Wire`) a consumer implements to supply
their own transport. A reader who takes the doc literally and tries to name
`Endpoint<MyWire>` or write a function generic over `Endpoint<W: Wire>` will
find neither compiles — `Endpoint<I>` takes an `Identity`, not a `Wire`, as
its parameter. The underlying point the doc is making (there is no `Box<dyn
Wire>`, no dynamic dispatch) is true and well-argued; only the specific
signature `Endpoint<W: Wire>` used to illustrate it is inaccurate — the type
that is actually generic over `W` is `EndpointBuilder`, not `Endpoint`.

**Suggested fix:** change the illustrative signature to name
`EndpointBuilder<I, W: Wire>`, or drop the specific type-signature notation
and just say "the endpoint is monomorphized over its wire type at
`build()`-time, with no dynamic dispatch."

This is a documentation-accuracy issue, not a functional defect and not a
spec/code conflict (SPEC.md's own §16.2/§16.3 pseudocode does not show
generics on `Endpoint` either) — filed as **should-fix** because it is a
concrete, verified inaccuracy in a doc comment a consumer implementing
`Wire` is likely to read closely.

### FINDING 2 (should-fix) — `Intro`, `Claimed`, `Proven` are not `#[must_use]`, though STORIES.md's S8 states this as an explicit acceptance criterion

See the working-log entry above ("FINDING CANDIDATE") for the full
derivation. Summary: STORIES.md S8 (§B, "a user can park a decision across
event-loop turns") states as an "Accepts" bullet: *"It is `#[must_use]` and
not `Clone`, so a parked chain cannot be forked."* — referring to `Claimed`,
and by the same logic applicable to `Intro`/`Proven` as the other rungs of
the same ladder.

**Verified by reading `src/shell/staged.rs` in full**: `Intro` (line 50),
`Claimed` (line 223) and `Proven` (line 292) are plain `pub struct`
definitions with no `#[must_use]` attribute. None derive or implement
`Clone` — that half of the criterion holds. `#[must_use]` is absent on all
three.

**Why it matters:** every producing verb (`read_identity`, `authenticate`,
`accept`) already returns a `Result<_, _>`, and `Result` itself carries
`#[must_use]` in `std`, so the most common mistake — calling a staged verb
and discarding the whole expression as a statement — is already caught.
What `#[must_use]` on the struct itself would additionally catch: any helper
a consumer writes that unwraps and returns a bare `Claimed<I>` (or
`Intro`/`Proven`) being called as a statement and silently dropped (=
rejected, per the type's own "Drop is a silent reject" contract) with no
compiler warning. Given dropping is a **meaningful, load-bearing action**
here (unlike an ordinary "forgot to use this" case), the case for
`#[must_use]` is if anything stronger than usual — compare
`std::sync::MutexGuard`, which is `#[must_use]` despite `Drop` also doing
meaningful work (unlocking).

**Suggested fix:** add `#[must_use]` to all three struct definitions in
`src/shell/staged.rs` (lines 50, 223, 292).

**Severity judgement:** should-fix rather than blocker because the
practical exposure is narrow (covered by `Result`'s own must_use in the
overwhelmingly common case) and no test currently asserts the attribute
either way, so nothing is silently broken today — but it is a literal,
named, testable acceptance-criterion bullet from an approved story that
does not currently hold.

---

## Story-by-story table (S1-S33)

Legend: satisfied = API exists and matches the story's "Accepts" text as read;
gap = something missing or contradicting; uncertain = not independently
confirmed against a runtime test (verified only by reading the API surface
and/or doc comments).

| Story | Verdict | API calls involved |
|---|---|---|
| S1 | satisfied | `Endpoint::connect`, `Connecting` (awaits to `Connection`), `Connection::close`, `ConnectionLost::{PeerClosed,LocallyClosed}` |
| S2 | satisfied | `Connecting` resolves `Err(ConnectError::TimedOut)`; retransmit-schedule tested in `tests/story_dial.rs` (`s2_retransmit_train_is_fixed_interval_not_exponential`) |
| S3 (a/b/c) | satisfied | `Endpoint::connect` -> `Err(ConnectError::AlreadyConnected)` (S3a, verified signature); S3b/S3c internal, surfaced via `ConnectionLost::Replaced` / `AcceptError::Stale`; tested in `tests/spec_shell.rs` |
| S4 | satisfied (internal correctness; nothing new on the public surface to check beyond "exactly one Connection results") | `tests/story_dial.rs`/`spec_shell.rs` tie-break tests |
| S5 | satisfied | `Connection::set_persistent_keepalive(Option<Duration>) -> Result<(), ConfigError>`, `Connection::persistent_keepalive() -> Option<Duration>`, `ConnectionLost::TimedOut`; verified `ConfigError` variants match ruling 42/40 bounds exactly |
| S6 | satisfied | `Endpoint::accept() -> Option<Intro<I>>` (0 DH); `Intro`'s `Drop` = silent reject; `testutil::CountingProvider`/`DhCounter` present for the test bar |
| S7 | satisfied | `Intro::read_identity() -> Result<Claimed<I>, IntroError>` (1 DH); `Claimed::claimed_static()`; drop = reject at 1 DH total; denylist documentation obligation present in rustdoc at `read_identity` and in crate docs |
| S8 | gap (minor) | `Claimed` is an owned, non-`Clone`, `INTRO_TTL`-expiring (`IntroError::Expired`) object — matches, **except** it is not `#[must_use]` as the story's own "Accepts" text states (**Finding 2**) |
| S9 | satisfied | `Claimed::authenticate() -> Result<Proven<I>, AuthError>` (2 DH cumulative); `Proven::accept() -> Result<Connection<I::Suite>, AcceptError>` (4 DH) |
| S10 | satisfied (config, not runtime-verified by me) | `Config::{intro_queue_cap, intro_max_per_source, with_intro_queue_cap, with_intro_max_per_source}` defaulting to 1024/4; TTL is fixed (not configurable), matching the story |
| S11 | satisfied | `Notification::{Contested, ContestCleared}` via `Connection::notified()`/`notifications()`; confirmed application-visible per ruling 45/46, not `core::ConnEvent`; tested `s11_*` in `tests/story_lifecycle.rs` |
| S12 | satisfied | `Connection::open_bi/open_uni/accept_bi/accept_uni`, `SendStream::write/finish`, `RecvStream::read`; tested `s12_*` |
| S13 | satisfied | independent stream handles, `StreamId` parity; tested `s13_*` |
| S14 | satisfied | `SendStream::reset(error_code)`, `ReadError::Reset(code)` on the peer; tested `s14_*` |
| S15 | satisfied | `Connection::send_datagram` (sync, never blocks), `DatagramError::TooLarge`; tested `sd1`-`sd11` in `tests/story_datagram.rs` |
| S16 | satisfied | `Connection::send_message/recv_message`, `MessageError::TooLarge`, bound `MESSAGE_RECV_MAX` = 262144 confirmed in `src/constants.rs`; tested `s16_*` |
| S17 | satisfied | flow-control backpressure via `SendStream::write`/`Connection::open_*` parking on credit; tested `s17_*` |
| S18 | satisfied | `Notification::AddressMoved{from,to}`, `Connection::remote_address()`; mover-keepalive obligation documented in crate docs (obligation #3) and `Connection::set_persistent_keepalive`; tested `s18_*` (8 tests) |
| S19 | satisfied | same surface as S18, our-side rebind; tested `s19_*` |
| S20 | satisfied | peer-restart recovery via S3b/S11 machinery; tested `s20_*` |
| S21 | satisfied | `Identity` trait carries no `Send` bound anywhere (verified by reading `src/identity.rs` in full); compile-fence test `a_wire_need_not_be_send` in `src/shell/wire.rs` proves the `!Send` path for `Wire`; no equivalent fence found specifically for `Identity`/DH-provider `!Send` (see "could not check") |
| S22 | satisfied (by inspection of `channel!`/`Handshake`; not independently traced through a mismatch test) | `slither::channel!` macro (src/packet/suite.rs), `Handshake` trait; version/prologue constants (`VERSION=0x01`, `PROLOGUE=b"slither\x01"`) confirmed in `src/constants.rs` matching CLAUDE.md's pinned values |
| S23 | satisfied (by design: no application-visible surface for rekey, which is exactly what the story requires) | `Config::epoch_size`/`with_epoch_size` (test-only, documented as such per ruling 82); no public rekey event exists, consistent with "no application-visible event" |
| S24 | satisfied | `testutil::{FlakyWire, Network, FlakyPolicy}` all `pub` behind `test-util`; the entire `tests/` suite is built on this |
| S25 | satisfied | `shell::wire::Wire` trait (`send_to`/`recv_from` over `io::Result`, no `Send` bound, not dyn-compatible by design); trace-not-act documented; tested in `tests/spec_shell.rs` (`a_failing_send_does_not_kill_the_connection_and_the_death_stays_timed_out`) |
| S26 | satisfied | handle-drop teardown semantics documented as crate-doc obligation #4; tested `s26_*` (5 tests) |
| S27 | satisfied | `Connection::closed() -> ConnectionLost`, `Connection::notified() -> Result<Notification, ConnectionLost>`; tested `s27_*` (6 tests) |
| S28 | satisfied | `Connection::acked() -> Result<(), ConnectionLost>`, `SendStream::acked() -> Result<(), WriteError>`; tested `s28_*` (10 tests, the most thoroughly tested story in the suite) |
| S29 | satisfied | `Connecting`'s `Drop` cancels the attempt (verified doc + code in `src/shell/endpoint.rs`); tested `s29_*` |
| S30 | satisfied | `WriteError::Reset(MESSAGE_OVERFLOW=0x06)` on the sender; receiver-side trace-only documented; `Connection::messages()`/`incoming_uni()` doc comments explicitly warn about the shared-supply hazard (verified in `src/compat/stream.rs`); tested `s30_*` (7 tests) |
| S31 | satisfied | `AsyncRead`/`AsyncWrite` on `SendStream`/`RecvStream`/`BiStream` (verified in full in `src/compat/io.rs`); `poll_shutdown` = finish+acked (ruling 57), `poll_flush` no-op (ruling 56); `io::ErrorKind` mapping verified cell-by-cell against §16.11.1 — **except Finding 1**, the dead `_=>` arm ruling 238 says to delete is still present in `write_kind` |
| S32 | satisfied | `Connection::{framed_bi, accept_framed_bi}` (src/compat/codec.rs); `Stream`/`Sink` adapters in `src/compat/stream.rs` verified to claim exactly one item per `poll_next` (ruling 58) by reading every impl body |
| S33 | satisfied | `Service<(SocketAddr, PublicKeyOf<I>)> for Endpoint<I>`, `Service<()> for &Connection<S>` and `for Connection<S>` (both borrowed and owned, per ruling 239), `serve()`; `UnsyncBoxService` composability specifically addressed via the owned impl added in ruling 239 |

**Summary: 32 of 33 stories fully satisfied as read against the public API
and rustdoc; S8 has one narrow, named gap (Finding 2, the missing
`#[must_use]`).** Story-level runtime correctness (does the test actually
pass, does the behaviour actually hold at runtime) was **not** independently
re-verified by executing `cargo test` for most stories — see below.

---

### VERIFIED: `cargo test --all-features` run in full — all green

Ran the full test suite (explicitly permitted per the brief; no Cargo.toml or
benches/ touched, no `cargo add`/`update`). Every one of 20 test binaries
reports `test result: ok`, zero failures, summing to roughly 1000+ tests
(727 unit/integration in the largest binary + 24+112+11+4+12+20+4+10+11+4+12
+16+15+16+4+15+6+6 across the story/spec suites + 12 doctests, all `ok`).
This substantially strengthens the story-by-story "satisfied" verdicts above
— it is not merely that the API shape matches the stories' text, the
paused-clock behavioural tests actually pass at this commit. I did not
individually re-derive each test's assertions against its story's prose
(that would mean re-reading ~100 test bodies, out of budget for this pass),
but the story-numbered naming convention and the counts above give me good
confidence the mapping in the table holds.

---

## What I could not check, and why

- **I did not read every line of `src/shell/connection.rs` (1406 lines),
  `src/shell/stream.rs` (1044 lines) or `src/shell/driver.rs` (1429 lines).**
  I read every `pub` signature (via targeted `grep`) and read substantial
  doc-comment and cross-reference context around each one (the staged ladder,
  the compat layer's callers of `poll_open_bi`/`poll_recv_message`/etc, the
  `Drop` impls, the `#[must_use]` audit), but did not read these three files
  top-to-bottom. Given they are the largest files in the crate and mostly
  internal machinery behind already-verified public signatures, I judged the
  public-API-surface risk here to be low relative to the time cost — but a
  reviewer with a different mandate (implementation correctness rather than
  API surface) should read them fully.
- **I did not independently verify most story tests' assertions against
  their story's prose**, only that `cargo test --all-features` is fully
  green and that test names/counts align with the stories (see above). A
  test named `s18_a_running_keepalive_dance_carries_the_move_by_itself` could
  in principle assert something weaker than S18 requires without my having
  caught it — I did not re-derive each assertion by hand.
- **I did not verify a compile-fence specifically for `Identity`/DH-provider
  `!Send`-ness** (S21's core claim), the way `src/shell/wire.rs` has
  `a_wire_need_not_be_send` for `Wire`. I confirmed by reading
  `src/identity.rs` in full that no `Send` bound appears anywhere in the
  `Identity` trait or its associated types, and that this is extensively
  argued in the module doc (including the specific "why `DhProviderAsync` is
  excluded" reasoning), but I did not find and did not exhaustively search
  for an equivalent runtime/compile-time fence test asserting a `!Send`
  identity provider actually drives an endpoint end-to-end (as
  `a_wire_need_not_be_send` does for `Wire`). This may exist under a name my
  greps did not match; I did not do an exhaustive search of `src/core/tests.rs`
  or `src/shell/endpoint.rs`'s test module for it.
- **I did not read `src/core/` (the sans-io cores) in any depth.** It is
  `pub(crate)`, so it is out of scope for a *public* API review by
  construction, and I treated it that way — my only uses of it were (a)
  cross-checking §16.4's pseudocode against the two accessor methods
  (`intro_source`/`intro_sender_index`) that are re-surfaced publicly via
  `Intro`, and (b) confirming `ConnectionId`/`Dir`/`IntroId`/`StreamId`/
  `Timestamp` are re-exported at the crate root for reachability. I did not
  check the `core` module's own internal API design.
- **I did not read `benches/` or the working-copy `Cargo.toml`**, per the
  concurrency notice — I used `git show cd12ed7:Cargo.toml` throughout, which
  should be identical to what a consumer building from that commit sees.
- **`SPEC.md` was read only in the sections the brief named plus a handful I
  followed up on from findings** (§16.2, §16.4, §16.11, §16.11.1, §18.1, plus
  a short excerpt of §17.1 encountered while reading §16.4 — not read in
  depth). I did not read §6 (the staged-accept normative text) or §7 (session
  lifecycle/roaming/keepalive) directly — my verification of S6-S11 and
  S18-S20 rests on the shell-layer rustdoc's citations of those sections
  plus the passing tests, not on reading the sections themselves. Given the
  hard "never read SPEC.md whole" rule and the brief's explicit list of
  which sections matter to an API review, I judged this an acceptable
  boundary, but it means a claim like "`Intro::source()`'s behaviour matches
  §6.1 exactly" is verified only transitively (code doc cites the section
  correctly and tests pass), not by me independently reading §6.1.
- **I did not run `cargo +<MSRV> check`, `cargo fmt --all --check`, or
  `cargo deny check`** — outside an API-surface review's scope and not
  requested by the brief; noted only for completeness against the release
  gate table in CLAUDE.md, which is not what I was asked to audit here.
- **I have no visibility into what the other, parallel reviewer found or
  will find** — by design (blind review), and I made no attempt to guess or
  reconcile with it.

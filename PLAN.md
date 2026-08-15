# slither v0.2 — implementation plan

> **Status: APPROVED 2026/08/14.** All ten decisions in §6 are ruled
> (rulings 53–59); the spec text is amended to match.
> Inputs: `SPEC.md` (ratified 2026/08/14, 80 rulings) and `STORIES.md`
> (30 capability stories, all approved).
> The spec is the authority. Where this plan and the spec disagree, the
> spec wins and this plan is wrong.

This is a **clean rewrite**. The existing `src/` (6 757 lines) implements
the v0.1 wire, which v2 supersedes; none of its module boundaries survive
the staged accept, the unified frame layer, or the two-core split. What
*does* survive is reusable in place — the packtool header idiom, the mac1
construction, `testutil`'s shape, and the recovery module's RFC 9002
arithmetic — and each is named at the slice that reuses it.

---

## 1. The shape of the thing

Three layers, and the boundary between the second and third is where
every defect found in rounds 6 and 7 lived.

```
core::Endpoint / core::Connection      pure state machines, no I/O, no clock
        ↑ poll_output() to Timeout, every mutating call            (§16.4)
shell: one !Send driver task           owns the cores, owns the Wire  (§16.3)
        ↑ handles
compat: AsyncRead/Write · Stream/Sink · Codec · tower       ← this plan's addition
```

The cores are `no_std`-shaped in spirit: `now: Instant` is an argument,
never a read; randomness comes from one seeded RNG; nothing allocates a
task. That is what makes §16.10's kernel-free drivability real and what
makes Appendix B's paused-clock obligations writable at all.

`compat` is new. It is now §16.11 of the spec, ratified as rulings 55–58.
Everything asked for — `BufWriter`, `tokio_util::codec`, `Stream`/`Sink`,
`tower::Service` — is a **shell-layer adapter over those verbs**, adds no
core state, and touches no wire byte. It is nonetheless a first-class
deliverable, not a nicety, because it decides the shape of the shell: see
§2.

### Module map — spec section to file

Deliberately one-to-one, because the spec is the authority and a reviewer
must be able to find the code for a section without searching.

```
src/
  lib.rs                    re-exports; the crate-level doc carries the five
                            documentation obligations (§9 below)
  constants.rs              every named constant, one place       ("Named constants")
  error.rs                  the closed taxonomy, verbatim                  (§18.1)
  varint.rs                                                                (§8.1)
  packet/{mod,header,mac}.rs   grammar, packtool headers, mac1     (§2, §3, §4)
  core/
    mod.rs                  poll contract types, Transmit, Disposition    (§16.4)
    endpoint/
      mod.rs                core::Endpoint<I: Identity>
      handshake.rs          initiator + responder driving                   (§5)
      staged.rs             typestate, IntroId, the four verbs        (§6.1–6.2)
      intro_queue.rs        stage-0 queue: cap, per-source, TTL           (§6.3)
      routing.rs            routing rule, tie-break, restart          (§6.5–6.8)
      guard.rs              the timestamp guard                          (§17.1)
      tables.rs             index tables, static map, hint set      (§17.2–17.4)
    connection/
      mod.rs                core::Connection
      session.rs            counter, replay window, roaming, liveness, ratchet (§7)
      frame.rs              codec, parse-then-apply, packing            (§8.2–8.7)
      stream.rs             send/recv halves, RESET_STREAM, GC         (§9.1–9.7)
      message.rs            the sugar, MESSAGE_OVERFLOW                    (§9.8)
      flow.rs               credit, both levels                            (§10)
      datagram.rs           queues, drop-oldest, counters                  (§11)
      ack.rs                fused to the replay window                     (§12)
      recovery.rs           RFC 9002 — reuse v0.1's arithmetic             (§13)
      congestion.rs         NewReno behind the controller seam             (§14)
      close.rs              CLOSE, linger, teardown matrix                 (§15)
      timers.rs             the named timer table, min-deadline out      (§16.5)
  shell/
    mod.rs
    driver.rs               the !Send actor: select! over wire/commands/timer
    endpoint.rs             Endpoint, EndpointBuilder, Connecting
    staged.rs               Intro → Claimed → Proven handles
    connection.rs           Connection, the closed() latch, notification slots
    stream.rs               SendStream, RecvStream, BiStream
    wire.rs                 the Wire trait + UdpSocket blanket impl      (§16.3)
  compat/
    io.rs                   AsyncRead / AsyncWrite                     (default)
    stream.rs               futures Stream / Sink              (feature = "sink")
    codec.rs                Framed constructors               (feature = "codec")
    tower.rs                Service impls                     (feature = "tower")
  testutil/mod.rs           Network, FlakyWire, FlakyPolicy, counting identity
```

---

## 2. The one architectural decision that gates everything

**How does a handle reach the core?** The answer decides whether
`AsyncWrite` is natural or an ordeal. Ruled before slice 3, as required.

§16.3 calls handles "thin channel-backed clients". §16.8 calls for "the
quinn pattern" — "per-stream wakers key the shell's blocked-readers /
blocked-writers maps" — and requires accessors to be "synchronous reads
of a shared cell the driver updates". Those two sentences describe
different mechanisms, and the second is the one quinn actually is.

**Ruled — split the seam by what it costs (ruling 53, §16.3).**

| Surface | Mechanism | Why |
|---|---|---|
| Endpoint verbs: `connect`, `accept`, the three staged verbs | command channel + oneshot reply | §6.2 requires the DH to land on the driver task; these are rare and already `async` in the spec |
| Connection data path: `write`, `read`, `open_*`, `accept_*`, `send_message`, `recv_*`, `close`, `acked`, `notified` | `Rc<RefCell<ConnShell>>` shared with the driver, waker maps keyed by `StreamId` | poll-native, no round-trip, no allocation per write |
| Accessors: `remote_static`, `remote_address`, `session_id`, `is_established` | read through the same `RefCell` | §16.8's "shared cell" |

The driver is `!Send` and single-threaded, so `Rc<RefCell<_>>` costs a
refcount and a borrow flag — there is no lock, and no contention to have.

**Everything else follows from this.** Each data-path verb is written
once as `poll_*(&mut self, cx) -> Poll<...>`; the spec's `async fn` is
then `poll_fn(|cx| self.poll_write(cx, buf)).await`, three lines. And
`AsyncWrite::poll_write` is the *same function*, with the error mapped to
`io::Error`. Take the other branch and every adapter in `compat` has to
box and store an in-flight future, `Unpin` gets delicate, and
`tokio::io::copy` allocates per call.

Every mutating borrow ends by marking the connection dirty and waking the
driver, which drains `poll_output()` to `Timeout` and does the I/O. The
core contract of §16.4 is untouched: the shell still drains after every
mutating call — it is just not always the driver that made the call.

This amended §16.3's wording as **ruling 53** (D1, §6). Shell-only: no
wire byte, no core type, no timer.

---

## 3. The composability layer

The maintainer's target, typed honestly. A `Connection` is a multiplexer, so
the byte-oriented object is a **stream**, not the connection — everything
below follows from that one substitution.

### 3.1 Byte streams — `AsyncRead` / `AsyncWrite` (no feature, tokio is already a dep)

```rust
impl tokio::io::AsyncWrite for SendStream {}   // poll_write / poll_flush / poll_shutdown
impl tokio::io::AsyncRead  for RecvStream {}   // FIN → EOF (0 bytes filled)

pub struct BiStream { /* SendStream + RecvStream */ }
impl BiStream {
    pub fn split(self) -> (SendStream, RecvStream);
    pub fn join(send: SendStream, recv: RecvStream) -> Self;
}
impl AsyncRead for BiStream {}
impl AsyncWrite for BiStream {}

impl From<ReadError>  for std::io::Error {}    // Reset → ConnectionReset
impl From<WriteError> for std::io::Error {}    // ConnectionLost → NotConnected / BrokenPipe
```

`open_bi()` / `accept_bi()` return `BiStream` rather than the spec's
tuple; `.split()` recovers the tuple, so nothing is lost and the common
case gets the duplex object the ecosystem wants. The user's example, with
real types:

```rust
let conn = endpoint.connect(addr, peer_static)?.await?;
let mut w = tokio::io::BufWriter::new(conn.open_bi().await?);
tokio::io::copy(&mut File::open(path).await?, &mut w).await?;
w.shutdown().await?;                    // finish + await acknowledgement
```

Two semantics are pinned, because both are guessable wrongly (rulings 56, 57):

- **`poll_flush` is a no-op that returns `Ready`.** Bytes accepted by
  `poll_write` are already in send state; `poll_write` only ever accepts
  what flow-control credit admits, so there is no shell buffer to push.
  `AsyncWrite::flush` is emphatically **not** delivery confirmation.
- **`poll_shutdown` is `finish()` *and then* `acked()`.** This is the
  stronger of the two candidates and it is chosen deliberately: S28
  exists because message-then-close silently loses its tail, and
  `copy(...).await; shutdown().await` is exactly the shape a consumer
  will write for a file transfer. Shutdown resolves in error if the
  connection dies first, so it cannot hang past `DEAD_TIMEOUT`.

### 3.2 Objects — `Stream` and `Sink` (`feature = "sink"`, futures-core + futures-sink)

```rust
impl Endpoint {
    pub fn incoming(&self) -> Incoming;          // Stream<Item = Intro>
}
impl Connection {
    pub fn messages(&self)     -> Messages;      // Stream<Item = Result<Vec<u8>, ConnectionLost>>
    pub fn datagrams(&self)    -> Datagrams;     // Stream<Item = Result<Vec<u8>, ConnectionLost>>
    pub fn incoming_bi(&self)  -> IncomingBi;    // Stream<Item = Result<BiStream,  ConnectionLost>>
    pub fn incoming_uni(&self) -> IncomingUni;   // Stream<Item = Result<RecvStream, ConnectionLost>>
    pub fn notifications(&self)-> Notifications; // Stream<Item = Result<Notification, ConnectionLost>>

    pub fn message_sink(&self)  -> MessageSink;  // Sink<Bytes, Error = MessageError>
    pub fn datagram_sink(&self) -> DatagramSink; // Sink<Bytes, Error = DatagramError>
}
```

**One invariant governs all of them, and it is normative for the
implementation: an adapter claims at most one item, and only from inside
`poll_next`.** No prefetch, no read-ahead task, no intermediate queue.
§16.4's pull model is the reason reliable data is never droppable
(§16.8), and an adapter that claimed ahead of its consumer would
reintroduce exactly the unbounded shell queue §10.6 forbids. This is the
single easiest way to get the composability layer wrong, so it is stated
here and pinned by a test.

`incoming_uni()` and `messages()` draw from the same supply and must not
both be used — that is S30 / ruling 51, and the rustdoc on both says so.

### 3.3 Framing — `tokio_util::codec` (`feature = "codec"`, implies `sink`)

Falls out of §3.1 for free; the constructors exist only to remove a
`use`:

```rust
let mut framed = conn.framed_bi(LengthDelimitedCodec::new()).await?;
framed.send(bytes).await?;
let reply = framed.next().await;
// identical to Framed::new(conn.open_bi().await?, codec)
```

### 3.4 Services — `tower` (`feature = "tower"`, tower-service only)

slither has **no request/response correlation on the wire**, so a
`Service` over messages would need a request id the transport does not
carry. The honest fit is *one bi stream per call* — the stream **is** the
correlation — plus the two connector shapes:

```rust
impl Service<(SocketAddr, PublicKey)> for Endpoint   { type Response = Connection; }
impl Service<()>                      for Connection { type Response = BiStream;   }

pub struct Rpc<C: Codec> { /* conn + codec */ }      // Service<C::Item, Response = C::Item>
                                                     // call = open_bi, send, finish, read, EOF
pub async fn serve<S>(conn: &Connection, svc: S) -> Result<(), ConnectionLost>;
                                                     // accept_bi loop → spawn_local per stream
```

**The `!Send` caveat, stated plainly rather than discovered later.**
`tower::Service` itself carries no `Send` bound and neither do
`tokio_util::codec`, `futures`' combinators, or `tokio::io::copy` — so
the surface above composes. What does *not* compose is anything that
spawns onto a work-stealing executor: `tower::buffer::Buffer`,
`spawn_ready`, `BoxService` (use `UnsyncBoxService`), hyper, and plain
`tokio::spawn` on any slither handle. The mitigation is D6's optional
`bridge` — a `Send` façade running the driver on its own thread — and it
is deliberately **out of v0.2 scope**: it re-crosses the core→shell seam
with channels, which is precisely where round 7's five defects lived. The
handle traits are designed so it can be added later without a breaking
change; it should not be added while that seam is still new.

### 3.5 Ergonomics — the `LocalSet` tax

Every consumer must build a current-thread runtime inside a `LocalSet`,
and that is the first thing they will hit. Ship a helper and a
copy-pasteable example rather than a paragraph of prose:

```rust
pub fn block_on<F: Future>(f: F) -> F::Output;   // current_thread rt + LocalSet
```

---

## 4. Delivery slices

Vertical, each ending green on the full gate table. Story coverage is
the definition of done for a slice — a slice is not finished when its
code compiles, but when its stories are paused-clock tests that pass.

| # | Slice | Delivers | Stories closed |
|---|---|---|---|
| 0 | **Ground** | branch + skeleton, `constants.rs`, `error.rs`, `varint.rs`, `Wire` + `UdpSocket` impl, `testutil` (`Network`, `FlakyWire`, `FlakyPolicy`, counting identity), the two-endpoint paused-clock fixture | — (closes S24's attestation gap) |
| 1 | **Packets & the gate** | §2 suite decl, §3 headers via packtool, §3.5 sizes, §4 mac1, the length/type/version silent-drop gate (§3.1, exact for handshakes per ruling 65). `PROLOGUE` is *declared* here but only *binds* in slice 2: §5.1 folds it into the Noise transcript, so a mismatch fails the handshake **cryptographically** and is never a silent drop. **Golden-wire vectors land here** — 174/81/196/107 pinned as byte tests before any code can move them | S22 (partial) |
| 2 | **Handshake & the ladder** | §5 driving (5 s + jitter, give-up 90 s), §6.1–6.3 typestate + intro queue, §17.1 guard, §17.2–17.4 tables, shell `Endpoint`/`Connecting`/staged handles | S2, S6, S7, S8, S9, S10, S21, S22, S29 |
| 3a | **The connection core** *(cut 2026/08/15, round 12)* | §7.1–7.2 counter + replay window, **§7.4's liveness half** — the two seal paths, the arming rule, the install pin, the `Liveness` timer (moved from slice 7; §15.2 seals CLOSE with `seal_quiet`), §7.7 ratchet, §7.8–7.9, §8 frame codec with **PADDING/PING/ACK/CLOSE** (four, not three: PADDING is `0x00` in §8.3's ratified table and an unknown type is a `PROTOCOL_VIOLATION` kill), §15 CLOSE + the three post-mortem states + the code registry, §16.4 poll contract on `core::Connection`, §16.5 connection timers + equal-deadline order, §16.7 plan-seal-commit | S23, plus a named **S1 precursor** at core level |
| 3b | **The shell** | the driver, **slice 2b's deferred `Endpoint`/`Connecting`/staged handles**, the `Connection` handle + `closed()` + the four accessors, ruling 53's two mechanisms, §16.2/§16.3/§16.8/§16.9/§16.10, ruling 49's `slither::io` trace | S1, S2, S26, S27 (`closed()` half), S29 |
| 4 | **Streams** | §9.1–9.7 ids, implicit open, both halves, RESET_STREAM, GC; §10 flow control; §16.9 early sends + id-at-establishment | S12 (lossless), S13, S14, S17 |
| 5 | **Reliability** | §12 ACK fused to the replay window + delayed ACK, §13 RFC 9002 (reuse v0.1 arithmetic), §14 NewReno, ruling 47's `acked()` / `flush()` | S12 (full, over `FlakyWire`), S28 |
| 6 | **Sugar** | §9.8 messages + `MESSAGE_OVERFLOW` + the guarded overflow check, §11 datagrams + drop-oldest + counters | S15, S16, S30 |
| 7 | **Mobility & contest** | §7.3 roaming + amplification budget, §7.5 keepalive + persistent keepalive + the contested probe *(§7.4's liveness half moved to 3a — round 12)*, §5.4 / §6.4 / §6.6–6.8 replacement + tie-break + restart, `notified()` + `Notification` | S3, S4, S5, S11, S18, S19, S20, S27 (full) |
| 8 | **Composability** | all of §3 above: `compat/{io,stream,codec,tower}.rs`, `BiStream`, the `io::Error` conversions, the no-prefetch pin | S25, and S31–S33 (drafted, §7) |
| 9 | **Ship** | the five documentation obligations, Appendix B complete, all eight gates, MSRV 1.96, `cargo deny`, rustdoc `-D warnings`, bubble-engine cutover | — |

**Slices 0–3 are the spine and 3b is the risk.** It is the slice where
both cores, the driver, and the handle seam first exist together, and it
is the layer three protocol-focused reviews never examined. Slices 4–7
all build on it. Hence §5. *(Round 12 cut slice 3 in two and the risk
did not divide evenly with it: 3a is a sans-io state machine of the kind
this project has already built twice, while every one of the seam
review's five chartered targets is 3b's.)*

Rough sizing, implementation plus tests, for planning only: slices 0–1
≈ 2k lines, slice 2 ≈ 3k, slice 3 ≈ 3k, slice 4 ≈ 3k, slice 5 ≈ 3k,
slice 6 ≈ 1.5k, slice 7 ≈ 3.5k, slice 8 ≈ 1.5k. Call it 20k lines
against v0.1's 6.7k — the growth is streams, flow control, recovery,
congestion control and the staged accept, none of which v0.1 has.

---

## 5. Review gates between slices

The pattern from rounds 6 and 7 is not decoration: **the protocol core
has survived every attack, and every recent defect has lived at the
boundary where an application meets the transport.** The plan spends its
review budget accordingly.

- **After slice 3b — a seam review, before slices 4–7 build on it.**
  Targets: the poll contract's drain discipline, `Retired` ordering,
  waker registration under `RefCell`, cancel-safety of every `async fn`,
  drop order across handles. Not a protocol review; those have been done.
  *(Moved from "after slice 3" by round 12's cut: all five targets are
  3b's, and reviewing them after a combined ~4 k-line slice would make
  the frame codec, the replay window and the ratchet noise to carry.)*
- **After slice 7 — an adversarial protocol review** of roaming,
  liveness and the contested probe against `FlakyWire` policies. This is
  where round 5's blocker lived and it is the densest state in the spec.
- **After slice 8 — an API review** against the four documentation
  obligations, with a consumer writing code from the rustdoc alone.
- **Continuously:** every slice ends on the full gate table, not just
  `cargo test`. A red golden-wire test is a ruling request, never an
  expectation to update.

---

## 6. Decisions needing a ruling

Each is shell-or-plan-layer. **None touches a wire byte**, and the golden
vectors stay green through all of them.

**All ruled 2026/08/14** except D10. Recorded in
`.spec-v2-clean-slate/rulings.md` round 9; the spec text is amended to
match (§16.3, §16.2, and the new §16.11).

| # | Decision | Ruling |
|---|---|---|
| **D1** → **ruling 53** | §16.3 said handles are "channel-backed"; §16.8 says "the quinn pattern" with waker maps and a shared cell. Which? | **Split by cost (§2).** Channels for the endpoint and staged verbs, where §6.2 requires the DH on the driver task; `Rc<RefCell<_>>` + waker maps for the connection data path and accessors. §16.3 amended. Without it, every `compat` adapter boxes a stored future. |
| **D2** → **ruling 54** | `Connection::flush()` collides with `AsyncWrite::poll_flush`, which means something weaker on the same object graph. | **Renamed `Connection::acked()`**, symmetric with `SendStream::acked()`. Semantics unchanged in every respect. Applied across §9.8, §14.5, §16.1, §16.2, Appendix B. |
| **D8** → **ruling 55** | `open_bi()` / `accept_bi()` return the tuple per §16.2, or `BiStream`? | **`BiStream`, `.split()` → the tuple.** Shape change only; it is what makes `Framed` and `copy_bidirectional` work with no adapter. |
| **D4** → **ruling 56** | `AsyncWrite::poll_flush`: no-op, or force a packet out? | **No-op returning `Ready`.** No shell buffer exists to push; promising more would put `flush` in competition with `acked()`. |
| **D3** → **ruling 57** | `poll_shutdown`: `finish()` only, or `finish()` + `acked()`? | **`finish()` + `acked()`.** The weak reading makes S28's silent tail loss reachable a second time, through `AsyncWrite`, by an application that never touches `close()`. |
| **D5** → **ruling 58** | May a `Stream` adapter claim ahead of its consumer? | **No — normative.** One item, only inside `poll_next`. Anything else rebuilds the unbounded shell queue §10.6 forbids, while looking like an ergonomic convenience. Pinned by Appendix B. |
| **D6** | A `Send` façade for multi-thread runtimes, buffered service layers, hyper. | **Deferred past v0.2 by design.** It re-crosses the seam that produced round 7's defects. §16.11's handle shapes stay compatible with adding it later without a breaking change. |
| **D7** | Adapters in-crate behind features, or companion crates? | **In-crate**, additive features `sink` / `codec` / `tower`. Gates already run `--all-features`; three tiny stable deps plus `tokio-util` under `codec`. |
| **D9** | v0.2 in place, or alongside v0.1? | **Rewrite on main.** Maintainer's ruling. `bubble-engine`'s `path = "../../slither"` does resolve here, so its build stops until the cutover — accepted; bubble is not a constraint on the rewrite and will be updated once this lands. |
| **D10** → **ruling 59** | S29 and S30, drafted from rulings 50–52. | **Approved 2026/08/14, both amended.** S29 pins that a cancel-and-redial loop **replaces rather than accumulates** (4 DH + one `Replaced` per cycle, bounded by §16.1). S30 gains **ruling 59**: the receiver MUST trace the overflow reset under `slither::frames`, because it is the end that caused the conflict and the only end that otherwise learns nothing. `STORIES.md` is complete at **30 approved stories**. |

---

## 7. Stories owed by this plan

The composability layer is new capability, so it owes stories on the same
terms as everything else. Drafted here, to move into `STORIES.md` on
approval.

- **S31 — a user can treat a stream as an `AsyncRead`/`AsyncWrite`.**
  `tokio::io::copy` a file into a `BiStream` behind a `BufWriter`,
  `shutdown()`, and the peer reads identical bytes and observes EOF.
  Over `FlakyWire` with loss, on the paused clock. Errors arrive as
  `io::Error`, and a reset surfaces as `ConnectionReset` rather than a
  silent truncation.
- **S32 — a user can stream typed objects with a codec.**
  `Framed<BiStream, LengthDelimitedCodec>` round-trips a sequence of
  objects in order; `Stream`/`Sink` backpressure maps onto flow-control
  credit rather than an intermediate buffer, and the no-prefetch
  invariant (D5) is asserted by a consumer that polls once and checks
  that exactly one item was claimed.
- **S33 — a user can drive slither from a `tower::Service`.** A `Service`
  call opens one bi stream, writes the request, finishes, reads the
  response to EOF, and concurrent calls do not head-of-line block each
  other. `serve()` drives the accepting side. The `!Send` boundary is
  asserted: `UnsyncBoxService` composes, and the caveats in §3.4 are
  rustdoc, not folklore.

---

## 8. Risks

1. **The core→shell seam (highest).** Round 7 found five defects there
   and three prior protocol reviews found none of them. Mitigated by
   D1 settling the mechanism before slice 3 and by the post-slice-3
   review gate.
2. **Scope.** ~20k lines. Mitigated by vertical slices that each end
   green — the work is interruptible at eight points, not one.
3. **Recovery and congestion control without a real network.** RFC 9002
   is easy to implement plausibly and wrongly. Mitigated by porting
   v0.1's recovery arithmetic (already tested) and by driving `FlakyWire`
   policies that reproduce the RFC's own scenarios.
4. **`!Send` versus the ecosystem.** Real friction, not a bug. Mitigated
   by §3.4's honest caveat list up front and D6's deferred bridge.
5. **bubble-engine.** A path dep to a branch under rewrite. Mitigated by
   D9 — pin, then cut over at slice 9, when S4 may also let bubble delete
   its own "the lower key dials" convention.

---

## 9. Documentation obligations (slice 9, but written as each lands)

Five hazards with no code fix, each of which a consumer meets by getting
it wrong. Every one belongs in rustdoc at the call site, not only here.

1. **S3a** — `connect()` to a live static returns `AlreadyConnected`;
   reconnect is `close()` **then** dial. "Call connect again" is the
   natural guess and it is wrong.
2. **S7** — denylisting on a *claimed* static bans on an unauthenticated
   assertion; an attacker can claim any public key to get a third party
   banned. Goes on `read_identity()`.
3. **S5** — connect-ahead-of-need dies at 25 s. Ruled, deliberate, and
   the single most surprising behaviour for a new consumer.
4. **S26** — teardown triggers on dropping **every handle**, not the
   endpoint. Drop-order sensitive and the opposite of the obvious guess.
5. **S30** — `send_message` + `open_uni` on one connection is a
   programming error with a defined loud failure. Goes on all three of
   `send_message`, `open_uni` and `accept_uni`, because the safe and
   unsafe shapes look alike at the call site.

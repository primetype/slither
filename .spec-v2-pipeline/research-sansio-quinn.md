# Research: sans-io core + thin shell shapes (quinn-proto, quinn, str0m) for slither v0.2

Sources are cited inline. Local paths point into
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` (quinn-proto 0.11.16, quinn 0.11.11) —
these were read directly, not summarized from memory. str0m and the essays were fetched from the
web (no local checkout of str0m was present).

Context read: `/Users/nicolasdiprima/work/primetype/slither/TODO.md` — the v0.2 plan already
proposes a quinn-shaped object API (`Endpoint`/staged `Intro→Connection`/`Reliable` wrapper) and a
sans-io core, with the open item "confirm sans-io (5b) over internal-driver (5a)". Everything below
is aimed at closing that item and giving the SPEC.md a concrete method-surface to rule on.

---

## 1. quinn-proto's core interface

quinn-proto is explicitly documented as sans-io. From the doc comment on `Endpoint` itself
(`quinn-proto-0.11.16/src/endpoint.rs:42-44`):

> "The main entry point to the library. This object performs no I/O whatsoever. Instead, it
> consumes incoming packets and connection-generated events via `handle` and `handle_event`."

There are **two** state machines, `Endpoint` and `Connection`, communicating via two small opaque
event types (`EndpointEvent`, `ConnectionEvent`) — the wiring between them is entirely the shell's
job. This bipartite core/event-passing split is the single most important structural fact for
slither, whose plan has the same shape (`Endpoint` handing off to per-connection objects).

### Endpoint surface (`quinn-proto-0.11.16/src/endpoint.rs`)

```rust
pub struct Endpoint { rng: StdRng, index: ConnectionIndex, connections: Slab<ConnectionMeta>, ... }

impl Endpoint {
    pub fn new(config: Arc<EndpointConfig>, server_config: Option<Arc<ServerConfig>>,
               allow_mtud: bool, rng_seed: Option<[u8; 32]>) -> Self;

    // bytes in -> routed to a Connection, a new Incoming, or a direct Response
    pub fn handle(&mut self, now: Instant, remote: SocketAddr, local_ip: Option<IpAddr>,
                  ecn: Option<EcnCodepoint>, data: BytesMut, buf: &mut Vec<u8>)
                  -> Option<DatagramEvent>;

    // Connection -> Endpoint events flow back through here
    pub fn handle_event(&mut self, ch: ConnectionHandle, event: EndpointEvent)
                         -> Option<ConnectionEvent>;

    pub fn connect(&mut self, now: Instant, config: ClientConfig, remote: SocketAddr,
                    server_name: &str) -> Result<(ConnectionHandle, Connection), ConnectError>;

    // staged accept, see below
    pub fn accept(&mut self, incoming: Incoming, now: Instant, buf: &mut Vec<u8>,
                  server_config: Option<Arc<ServerConfig>>)
                  -> Result<(ConnectionHandle, Connection), AcceptError>;
    pub fn refuse(&mut self, incoming: Incoming, buf: &mut Vec<u8>) -> Transmit;
    pub fn retry(&mut self, incoming: Incoming, buf: &mut Vec<u8>) -> Result<Transmit, RetryError>;
    pub fn ignore(&mut self, incoming: Incoming);
}

pub enum DatagramEvent {
    ConnectionEvent(ConnectionHandle, ConnectionEvent),  // route to existing Connection
    NewConnection(Incoming),                              // stage-0 accept object
    Response(Transmit),                                   // endpoint-generated reply (version-negotiation, stateless reset)
}
```

(`endpoint.rs:104-330,536-720,1153-1160`.)

**Routing / connection IDs.** `Endpoint::handle` demuxes purely by `(remote addr, local_ip)` four-tuple
plus connection ID, using an internal `ConnectionIndex` (`FourTuple` + CID → `ConnectionHandle`
lookup, `endpoint.rs:230-260` — the routing table itself). No sockets are touched; `handle` takes
the already-received datagram bytes and returns *what to do with them* as data, never performs I/O.
It also short-circuits into a stateless reset or version-negotiation `Transmit` directly when the
CID is unrecognized (`endpoint.rs:249-291`), and buffers datagrams that arrive for a not-yet-accepted
`Incoming` up to bounded per-connection and total byte caps (`incoming_buffer_size`,
`incoming_buffer_size_total`; `endpoint.rs:222-236`) — overflow silently drops. This maps directly
onto slither's plan-point 3 ("Accept queue parks stage-0 objects only... bounded, overflow drops
silently").

**Accept staging (`Incoming`).** `Incoming` is a plain, `Drop`-instrumented struct (an
`IncomingImproperDropWarner` logs a warning — not a panic — if it's dropped without being passed to
`accept`/`refuse`/`retry`/`ignore`, `endpoint.rs:1219-1230`). It exposes read-only accessors
(`remote_address()`, `remote_address_validated()`, `may_retry()`, `orig_dst_cid()`) before any DH
work is spent. This is architecturally the same shape as slither's `Intro`/`Claimed`/`Proven`
staged typestate — quinn's version only has one user-visible stage (retry-eligibility gating,
which is QUIC's address-validation concern, analogous to but weaker than slither's DH-cost
staging), but the *mechanism* (opaque struct, explicit accept/refuse/retry verbs, warn-on-improper-drop) is directly reusable.

### Connection surface (`quinn-proto-0.11.16/src/connection/mod.rs`)

```rust
impl Connection {
    // -------- timers: ONE deadline out --------
    pub fn poll_timeout(&mut self) -> Option<Instant>;                       // :378
    pub fn handle_timeout(&mut self, now: Instant);                          // :1164

    // -------- app-facing events --------
    pub fn poll(&mut self) -> Option<Event>;                                 // :388
    pub fn poll_endpoint_events(&mut self) -> Option<EndpointEvent>;         // :406

    // -------- bytes in --------
    pub fn handle_event(&mut self, event: ConnectionEvent);                  // :1092

    // -------- bytes out --------
    pub fn poll_transmit(&mut self, now: Instant, max_datagrams: usize,
                          buf: &mut Vec<u8>) -> Option<Transmit>;             // :452

    pub fn close(&mut self, now: Instant, error_code: VarInt, reason: Bytes); // :1240
    pub fn is_handshaking(&self) -> bool;                                    // :1312
    pub fn is_closed(&self) -> bool;                                         // :1323
    pub fn is_drained(&self) -> bool;                                        // :1331
    // ... streams(), datagrams(), stats(), rtt(), side(), etc.
}
```

**Timers — one deadline, many named sources, internal coalescing.** This is the crux fact for
slither's "5 timers" question. `Connection` keeps a fixed table of 9 named timer kinds
(`connection/timer.rs`):

```rust
pub(crate) enum Timer {
    LossDetection = 0, Idle = 1, Close = 2, KeyDiscard = 3, PathValidation = 4,
    KeepAlive = 5, Pacing = 6, PushNewCid = 7, MaxAckDelay = 8,
}
struct TimerTable { data: [Option<Instant>; 10] }
impl TimerTable {
    fn set(&mut self, timer: Timer, time: Instant);
    fn next_timeout(&self) -> Option<Instant> { self.data.iter().filter_map(|&x| x).min() }
    fn is_expired(&self, timer: Timer, after: Instant) -> bool { ... }
}
```

`poll_timeout()` is just `self.timers.next_timeout()` — the **minimum** of all live named
deadlines, exposed as a single `Option<Instant>`. `handle_timeout(now)` is the fan-out: it loops
over **all 9** `Timer::VALUES`, and for each one whose deadline is `<= now` it stops that timer and
runs that timer's specific logic (idle → kill connection, keep-alive → send ping, loss-detection →
retransmit, etc.) — `connection/mod.rs:1164-1230`. The doc comment on `handle_timeout` states the
safety property explicitly:

> "It is most efficient to call this immediately after the system clock reaches the latest
> `Instant` that was output by `poll_timeout`; however spurious extra calls will simply no-op and
> therefore are safe."

So the public contract is **one deadline, idempotent re-entry** — internally many named timers, but
the shell only ever needs to track a single `Option<Instant>` per connection and call
`handle_timeout(now)` whenever it fires (or spuriously; it's a no-op if nothing is due).

**poll_transmit details.** Writes encoded bytes into a **caller-supplied** `buf: &mut Vec<u8>`
(no internal buffer ownership — the shell controls allocation/pooling) and returns a `Transmit`
struct describing where and how to send it:

```rust
pub struct Transmit {
    pub destination: SocketAddr,
    pub ecn: Option<EcnCodepoint>,
    pub size: usize,
    pub segment_size: Option<usize>,   // GSO batching, None = single datagram
    pub src_ip: Option<IpAddr>,
}
```
(`quinn-proto-0.11.16/src/lib.rs:304-318`.) `poll_transmit` must be re-called in a loop until it
returns `None` (single-call-returns-one-datagram, standard "drain the poll" pattern) — see the
driver code in §2.

**Accept at the proto layer** is exactly the `Endpoint::accept`/`refuse`/`retry`/`ignore` verbs
above, consuming the `Incoming` by value.

---

## 2. quinn's tokio shell

Source: `quinn-0.11.11/src/endpoint.rs`, `connection.rs`, `mutex.rs`, `runtime.rs`, `incoming.rs`.

### Driver tasks: one per Endpoint, one per Connection

- **Endpoint driver** (`EndpointDriver`, a `Future`) is spawned once, inside
  `Endpoint::new_with_abstract_socket` (`endpoint.rs:133-163`), immediately after constructing the
  `proto::Endpoint`. Its `poll()` (`endpoint.rs:366-397`) does, every wake:
  1. lock `state: Mutex<State>`,
  2. `drive_recv` — poll the UDP socket for datagrams, decode with `udp::RecvMeta`, feed each into
     `proto::Endpoint::handle`, and dispatch the resulting `DatagramEvent` (new `Incoming` →
     buffered for `accept()`; `ConnectionEvent` → forwarded over that connection's mpsc channel;
     `Response` → sent directly on the socket),
  3. `handle_events` — drain an `mpsc::UnboundedReceiver<(ConnectionHandle, EndpointEvent)>` fed by
     every connection driver, call `proto::Endpoint::handle_event`, forward any resulting
     `ConnectionEvent` back to that connection's own channel,
  4. if there's more work (`keep_going`), explicitly `cx.waker().wake_by_ref()` to reschedule
     immediately rather than relying on the runtime to notice — an explicit anti-starvation /
     anti-missed-wakeup technique, not a spurious accident.

- **Connection driver** (`ConnectionDriver`, `connection.rs:240-273`) is spawned once **per
  connection**, both for outbound (`Connecting::new`, `connection.rs:44-77`) and inbound
  (`EndpointInner::accept`, `endpoint.rs:432-449`) connections. Its `poll()` loop:
  1. lock this connection's own `Mutex<State>`,
  2. `process_conn_events` — drain this connection's `mpsc::UnboundedReceiver<ConnectionEvent>`
     (fed by the endpoint driver) and call `proto::Connection::handle_event`,
  3. `drive_transmit` — loop calling `proto::Connection::poll_transmit`, writing each `Transmit` to
     the **shared UDP socket directly** (an `Arc<dyn AsyncUdpSocket>` clone — connections send
     independently of the endpoint driver; only *receiving* is centralized for demuxing),
  4. `drive_timer` — call `proto::Connection::poll_timeout()`, reset/create one reusable
     `Pin<Box<dyn AsyncTimer>>` to that deadline (**only if the deadline actually changed** — "Avoid
     resetting the timer when the deadline is unchanged", `connection.rs:1177-1180` — this is the
     concrete anti-timer-churn technique), and poll it; on expiry call
     `proto::Connection::handle_timeout(now)`,
  5. `forward_endpoint_events` — drain `proto::Connection::poll_endpoint_events()` and send each to
     the endpoint over the shared mpsc sender,
  6. `forward_app_events` — drain `proto::Connection::poll()` and translate each `Event` into
     wakeups: `Waker::wake()` on `blocked_readers`/`blocked_writers` maps keyed by `StreamId`,
     `Notify::notify_waiters()` for endpoint-wide conditions (new incoming stream, datagram
     received, etc.).

### Sharing state: Arc<Mutex<>> for state, mpsc channels for cross-object events, Notify for fan-out wakeups

- `Endpoint` (the user handle) = `Arc<EndpointInner>` = `{ state: Mutex<State>, shared: Shared }`.
  `Connection`/`Connecting` (user handles) = `Arc<ConnectionInner>` = `{ state: Mutex<State>,
  shared: Shared }`. Every synchronous handle method (`Connection::send_datagram`,
  `Endpoint::connect`, etc.) just locks the mutex and calls straight into the proto object, then
  calls `state.wake()` to prod the driver task's stored `Waker` if one is registered.
- The **only** channel-based plumbing is the two mpsc streams that move `EndpointEvent`/
  `ConnectionEvent` between the endpoint driver and each connection driver — deliberately kept
  separate from the `Mutex<State>` so that a connection driver never needs to lock the endpoint's
  mutex (and vice versa) just to relay a routing event. Both are **unbounded** — see pitfalls (§5)
  for the backpressure implication.
- Wakeup fan-out uses `tokio::sync::Notify` for "any number of waiters, no lost-wakeup" conditions
  (new `Incoming` available, endpoint idle, new inbound stream, datagram received) and per-key
  `Waker` maps (`FxHashMap<StreamId, Waker>`) for "wake exactly the one future blocked on this
  specific stream" conditions — a real distinction the shell makes deliberately, not one generic
  wake-everyone mechanism.
- A custom `Mutex<T>` wrapper (`quinn-0.11.11/src/mutex.rs`) exists specifically to *diagnose* lock
  contention: behind a `lock_tracking` feature it times every lock acquisition and hold, and
  `tracing::warn!`s if either exceeds 1ms, recording the last 20 lock "purposes". This is strong
  evidence quinn's authors hit real contention/starvation bugs in production and instrumented for
  it rather than architecting it away — worth taking as a warning, see §5.

### `Endpoint::accept()` / `Incoming`/`Connecting` over the proto core

`Endpoint::accept()` returns an `Accept<'_>` future (`endpoint.rs:640-677`) that awaits a
`tokio::sync::Notify` (`shared.incoming`) and pops from a `VecDeque<proto::Incoming>` buffered by
the driver's `drive_recv`. The shell's `Incoming` (`quinn-0.11.11/src/incoming.rs:18-24`) is a thin
wrapper `{ inner: proto::Incoming, endpoint: EndpointRef }` whose `accept()`/`refuse()`/`retry()`/
`ignore()` just lock the endpoint mutex and call the matching `proto::Endpoint` verb — and its
`Drop` impl calls `refuse()` automatically if the caller never explicitly resolved it
(`incoming.rs:104-110`, "Implicit reject, similar to Connection's implicit close"). It also
implements `IntoFuture` (`accept().await` sugar) by internally calling `accept()` then polling the
resulting `Connecting`. `Connecting` itself (`connection.rs:37-77`) spawns the per-connection driver
task the moment it's constructed (inside `ConnectionSet::insert` / `Connecting::new`) — i.e. the
driver task exists and is already pumping the handshake before the caller ever gets a `Connecting`
value to await.

### Backpressure

Two independent backpressure points, both explicit and both bounded except the event channels:
1. **Socket writes**: `drive_transmit` calls `AsyncUdpSocket::try_send`; on `WouldBlock` it stashes
   the pending `proto::Transmit` in `buffered_transmit: Option<proto::Transmit>` and returns
   "not ready", to be retried once `UdpPoller::poll_writable` wakes the task — `poll_transmit` is
   simply not called again until the previous transmit has actually gone out
   (`connection.rs:998-1060`).
2. **Accept queue**: bounded by `proto::Endpoint`'s `incoming_buffer_size` /
   `incoming_buffer_size_total` (see §1) — overflow silently drops at the proto layer, before the
   shell even sees it.
3. **Event channels** (endpoint↔connection): **unbounded** `mpsc`. No backpressure at all — see §5.

---

## 3. str0m and the sans-io pattern essays: single-`poll_output` contrast

str0m (sans-io WebRTC, no local checkout available — read via docs.rs/GitHub) collapses quinn's
four poll methods (`poll_transmit`, `poll_timeout`, `poll`, `poll_endpoint_events`) into **one**:

```rust
impl Rtc {
    pub fn accepts(&self, input: &Input) -> bool;
    pub fn handle_input(&mut self, input: Input) -> Result<(), RtcError>;
    pub fn poll_output(&mut self) -> Result<Output, RtcError>;
}

pub enum Input<'a> {
    Timeout(Instant),
    Receive(Instant, net::Receive<'a>),
}

pub enum Output {
    Timeout(Instant),      // "no more output right now; here's the next deadline"
    Transmit(net::Transmit),
    Event(Event),           // Connected, MediaData, ChannelData, IceConnectionStateChange, ...
}
```
(github.com/algesten/str0m, `src/lib.rs`, fetched via raw.githubusercontent.com.)

The contract (documented on `Rtc`) is: "Every mutation of an `Rtc` instance must be followed by a
complete drain of `poll_output` until it returns `Output::Timeout`." Note the elegant unification:
**the timeout deadline is itself the terminal variant of the same enum** that carries transmits and
events — there's no separate "am I done polling" query and no separate "what's the deadline" query;
draining the loop *is* discovering the deadline. The driver loop degenerates to:

```
loop {
    match rtc.poll_output()? {
        Output::Transmit(t) => socket.send(t),
        Output::Event(e)    => app_events.push(e),
        Output::Timeout(deadline) => break deadline,   // wait for this OR next input, whichever first
    }
}
```

### Trade-offs vs quinn's separate-poll shape

| | quinn-proto (4 methods) | str0m (1 method) |
|---|---|---|
| Driver loop shape | must remember to call `poll_transmit`, `poll`, `poll_endpoint_events` each in their own drain loop, plus `poll_timeout` once | one loop, one `match`, naturally exhaustive |
| Forgetting to drain something | silently loses events/transmits (a real bug class — nothing forces you to drain all four) | impossible by construction — the `Output::Timeout` sentinel *is* the drain-complete signal, one loop drains everything |
| Cross-object events (Connection ↔ Endpoint) | needs a *fifth* channel (`poll_endpoint_events`/`handle_event`) because str0m has no equivalent second object in the same core | not applicable — str0m's `Rtc` is a single object per peer connection, no endpoint/connection split to bridge |
| Allocation / ordering | four independently-typed return values, no ordering guarantee between e.g. a transmit and the event that caused it | one `Output` stream preserves relative ordering of transmits vs events as generated, which can matter for tests/logs |
| Extensibility | adding a new "kind of thing that comes out" (new poll method) is a breaking API addition but doesn't touch existing call sites | adding a new `Output` variant is non-breaking for callers using `match ... => {}` catch-alls but breaking for exhaustive matches — same trade-off either way in Rust, just concentrated in one enum instead of spread across four return types |
| Method count / API surface | larger, more self-documenting method names (`poll_transmit` reads better at the call site than `matches!(out, Output::Transmit(_))`) | smaller surface, but callers must know the enum to know what can come out |

The essays converge on the same observation. From swatinem.de's "Finding a usable sans-io pattern"
(https://swatinem.de/blog/sans-io-pattern/): the author explicitly compares (a) quinn/str0m-style
poll methods, (b) a Stateright-inspired **out-parameter** pattern (`on_msg(&mut self, msg, out: &mut
Out)` accumulating into a caller-supplied `Vec`-like sink — avoids allocation but "quite infectious"
through generic/trait call chains), and (c) their own preferred **single `tick()` method** taking
input plus elapsed time and returning output messages + next-timeout-duration directly (no
poll-loop at all, everything in one return value) — chosen for testability and because it "works
with concrete types" rather than needing trait objects to thread an out-param through layered
protocols. The essay also flags that `Duration` (elapsed-since-last-tick) is easier to construct in
tests than raw `Instant`, an ergonomics point distinct from quinn's absolute-`Instant`-everywhere
choice.

From firezone's engineering blog (https://www.firezone.dev/blog/sans-io) — Firezone's own
`snownet` library composes str0m (ICE) with a WireGuard-shaped sans-io core, so this is the closest
precedent to slither's actual problem (WireGuard-shaped protocol, sans-io, object composition):
they describe the canonical method set as `handle_input`/`poll_transmit`/`handle_timeout`/
`poll_timeout`, `Instant` parameters everywhere instead of internal clock calls (enables
property-based testing that fast-forwards time), and name a concrete pitfall explicitly: "a bug in
the state machine where the value returned from `poll_timeout` is not advanced can lead to
busy-looping behaviour" — i.e. if `poll_timeout` returns a deadline that is `<= now` forever (state
that should have cleared the timer didn't), the driver spins.

---

## 4. Distilled recommendation for slither's core

slither's shape: one datagram flow per `Connection` (no multiplexed streams yet), a handful of
named timers (handshake retransmit, rekey, keepalive/idle, roaming settle, — roughly 5 per the
prompt), plus an `Endpoint` that demuxes by `(addr, sender_index)` into either an `Intro` (pre-DH,
stage 0) or a `Connection`. This is structurally much closer to str0m/snownet's single-object-per-
peer shape than to QUIC's stream-multiplexing complexity, but it *does* have the Endpoint/Connection
two-object split that str0m's `Rtc` doesn't — so the cross-object-event question (quinn's
`EndpointEvent`/`ConnectionEvent`) is real for slither and str0m's design doesn't answer it; quinn's
does.

### Candidate (a): quinn-style separate polls

```rust
impl Connection {
    fn handle_datagram(&mut self, now: Instant, from: SocketAddr, data: &[u8]);
    fn poll_transmit(&mut self, now: Instant, buf: &mut Vec<u8>) -> Option<Transmit>;
    fn poll_timeout(&mut self) -> Option<Instant>;
    fn handle_timeout(&mut self, now: Instant);
    fn poll_event(&mut self) -> Option<Event>;               // app-facing
    fn poll_endpoint_event(&mut self) -> Option<EndpointEvent>; // -> Endpoint, e.g. "retire sender_index"
}
```
- **Multiple timers**: internal named `TimerTable`-style array (5 named timers, `[Option<Instant>;
  5]`), `poll_timeout` = min of the array, `handle_timeout` fans out to whichever are due — proven
  pattern, directly copyable from quinn-proto's `connection/timer.rs`.
- **Retransmit scheduling**: a timer entry (`Timer::HandshakeRetransmit`) set/cleared exactly like
  quinn's `LossDetection` timer; on fire, `handle_timeout` re-queues the last handshake message for
  `poll_transmit` to emit.
- **RNG for handshake retransmit**: see the RNG note below — applies identically to all three
  candidates.
- **Testability**: excellent — every method takes `Instant` explicitly, `handle_datagram`/
  `poll_transmit`/`handle_timeout` are pure enough to fuzz/property-test with a virtual clock (feed
  the return of `poll_timeout` straight back into `handle_timeout`).
- **Con**: five methods to remember to drain (worse than quinn's four, because slither also needs
  the `Endpoint`-facing one); a caller that forgets to drain `poll_event` after `handle_timeout`
  silently loses events. Needs explicit "you must poll until None after any handle_* call" doc
  discipline, same as quinn's.

### Candidate (b): single `poll_output` event enum (str0m-style)

```rust
enum Input<'a> { Datagram { now: Instant, from: SocketAddr, data: &'a [u8] }, Timeout(Instant) }
enum Output {
    Transmit { to: SocketAddr, data: Vec<u8> },
    Event(Event),
    EndpointEvent(EndpointEvent),   // slither-specific: str0m has no equivalent, quinn splits this out
    Timeout(Instant),               // terminal: drain-complete sentinel AND next deadline
}
impl Connection {
    fn handle_input(&mut self, input: Input<'_>);
    fn poll_output(&mut self) -> Output;
}
```
- **Multiple timers**: same internal `TimerTable` as (a); only the *public* shape changes —
  `poll_output` computes `next_timeout()` itself when nothing else is pending, and that becomes the
  terminal `Output::Timeout`.
- **Retransmit scheduling**: identical internal mechanism to (a); the only difference is how it
  surfaces (as one branch of `Output` instead of a separate `poll_transmit`).
- **RNG**: identical to (a) and (c) — orthogonal to the poll shape.
- **Testability**: equally good — same `Instant`-explicit contract; arguably *better* for driver-
  loop bugs, because a single `while let Output::X = poll_output() {}`-style loop can't accidentally
  skip draining one of the four separate queues the way (a) can. This directly forecloses the
  "forgot to drain poll_event" bug class quinn is exposed to.
- **Con**: `EndpointEvent` has to live inside the same `Output` enum as transmits/app-events, so
  every caller's `match` needs a catch-all or handles all three; for slither's Endpoint↔Connection
  wiring this is mildly awkward (an app-level caller polling a `Connection` for `Event`s also has to
  filter out `EndpointEvent`s meant for the `Endpoint`, or the shell has to intercept them before
  the app ever sees the enum). quinn's split avoids this by giving `EndpointEvent` its own channel;
  str0m avoids the problem entirely by not having a second object to talk to. Recommend: if slither
  goes single-poll, keep `EndpointEvent` **out** of the app-facing `Output` enum and give it its own
  narrow `poll_endpoint_event()` method (a "mostly-(b), one exception" shape) — since the shell,
  not the app, ever consumes it, this doesn't reopen the multi-drain footgun for application code.

### Candidate (c): input → output pure function, explicit `now: Instant` everywhere, no internal poll loop

```rust
struct Outcome { transmit: Option<Transmit>, events: SmallVec<[Event; 2]>, next_timeout: Option<Instant> }
impl Connection {
    fn on_datagram(&mut self, now: Instant, from: SocketAddr, data: &[u8]) -> Outcome;
    fn on_timeout(&mut self, now: Instant) -> Outcome;
    // no free-standing poll; every state transition returns its own effects directly
}
```
- **Multiple timers**: still needs the internal named-timer table (unavoidable — 5 timers don't
  collapse into fewer just because the polling shape changed), but `next_timeout` in `Outcome` is
  computed and returned inline rather than queried separately.
- **Retransmit scheduling**: same internal mechanism again.
- **Con — multiple transmits per call**: this is where (c) genuinely struggles relative to (a)/(b).
  quinn's `poll_transmit` is called in a **loop** because a single `handle_timeout` can legitimately
  need to emit several datagrams (e.g. GSO batches, or — for slither — a retransmit *and* a
  keepalive firing at the same instant). `Outcome.transmit: Option<Transmit>` forces a choice:
  either make it `Vec<Transmit>` (fine, but now the "pure function" has to pre-allocate a vec on
  every call even when nothing fires) or restrict the model to "at most one transmit per input"
  (which the current design likely can't guarantee once rekey + retransmit + keepalive timers can
  coincide). This is a real, not cosmetic, drawback for a 5-timer protocol.
- **Testability**: best of the three for unit tests — a test is just `let out = conn.on_timeout(t);
  assert_eq!(out.transmit, ...)`, no loop-until-None boilerplate, easy to snapshot-assert an entire
  `Outcome` per input. Weaker for property/fuzz testing that wants to drive many (Connection,
  Endpoint) pairs through a discrete-event simulation, because the caller has to reconstruct the
  "poll until quiescent" loop itself around `Outcome.next_timeout` rather than getting it for free.
- **RNG**: identical to (a)/(b).

### RNG injection into a pure core (applies to all three shapes)

quinn-proto's answer, directly transferable: **the pure core owns its own PRNG instance, seeded
once at construction, not re-seeded from OS entropy on every retransmit.**

- `Endpoint::new(..., rng_seed: Option<[u8; 32]>)` seeds one `StdRng`, falling back to OS entropy
  (`SysRng`) only if no seed is given (`endpoint.rs:79-84`).
- Every time a new `Connection` is created, the `Endpoint` draws 32 fresh bytes **from its own
  `rng`** (`self.rng.fill_bytes(&mut rng_seed)`, `endpoint.rs:809-810`) and passes them as a plain
  `[u8; 32]` into `Connection::new`, which seeds its *own* independent `StdRng`
  (`connection/mod.rs:256,271-273`). The connection then draws from that stream whenever it needs
  fresh randomness — e.g. `self.rng.random()` for a path-validation challenge nonce
  (`connection/mod.rs:3068,3075`).
- Net effect: **no connection ever calls `OsRng`/`SystemTime`/`getrandom` directly.** Randomness is
  a resource injected once at the root (`Endpoint`) and deterministically fanned out — which means
  the entire two-object system (`Endpoint` + all its `Connection`s) is **exactly reproducible**
  given one root seed, while still being unpredictable to an outside attacker if that root seed
  came from real OS entropy. This is the correct answer to "handshake-retransmit needing fresh
  randomness inside a pure core": the core is still pure (no I/O, no ambient `Instant::now()`/
  `OsRng` calls), it just also owns a small piece of *state* (the RNG) exactly like it owns timers —
  seeded via a constructor parameter, advanced via calls, fully mockable in tests by passing a fixed
  seed.
- For slither specifically: since a handshake retransmit of the *same* message (e.g. resending msg1
  because msg2 didn't arrive) must **not** rotate the ephemeral key (that would restart the Noise
  handshake, not retransmit it), "fresh randomness on retransmit" only applies to path-validation-
  style nonces or to a *new* handshake attempt after full timeout — not to retransmission proper.
  Worth confirming in the SPEC which of slither's 5 timers actually need a fresh random draw on
  fire (candidates: a fresh cookie/challenge nonce, jittering a keepalive interval) versus which
  just resend a buffered, already-encrypted message unchanged (ordinary retransmit — no RNG call at
  all). Either way, the *injection* mechanism (seed at construction, `Endpoint` roots it, per-
  `Connection` sub-seed derived once) is the reusable piece regardless of which candidate shape you
  pick.

### Recommendation

For slither's scale (2 objects, ~5 timers, no streams), **candidate (b) with the `EndpointEvent`
carve-out** (single `poll_output` for transmit/app-event/timeout, a separate narrow
`poll_endpoint_event()` for the Connection→Endpoint channel only) gets str0m's biggest practical win
— a driver loop that cannot forget to drain something — without inheriting str0m's blind spot (it
has no second object to bridge to). Candidate (a) is the safe, well-precedented fallback if the team
would rather have quinn's exact, battle-tested shape and accept the extra discipline burden.
Candidate (c) is worth adopting *only* for the leaf pure functions inside the core (e.g. a
`recovery.rs`/`handshake.rs` function that computes "what to retransmit," already true of slither's
existing pure modules per TODO.md) — not as the top-level `Connection` API, because of the
multiple-simultaneous-transmit problem above.

---

## 5. Pitfalls (evidenced from quinn's source, not just general folklore)

- **Timer coalescing / churn.** quinn's connection driver explicitly avoids resetting its
  `AsyncTimer` when the deadline hasn't changed (`connection.rs:1177-1180`, comment: "Avoid
  resetting the timer when the deadline is unchanged") — resetting an OS/runtime timer on every
  poll even when the deadline is unchanged is wasted work and, at scale, a measurable cost. Also
  note the *design* choice to represent N named timers as one public deadline (§1) is itself a
  coalescing strategy — don't expose N timer objects to the shell; expose 1 and coalesce inside the
  core.
- **Busy-loop / stuck-deadline bugs.** Firezone's post names this directly: if `poll_timeout`
  returns a deadline `<= now` and the corresponding internal timer isn't actually cleared by
  `handle_timeout`, the driver spins forever re-firing the same instant. quinn-proto guards this by
  making `handle_timeout` **idempotent by construction** (`timers.stop(timer)` before running that
  timer's logic — connection/mod.rs:1164-1166) rather than relying on callers to be careful. Same
  discipline applies to all three candidate shapes in §4.
- **Waker storms / missed wakeups, the same coin.** quinn's driver `poll()` explicitly calls
  `cx.waker().wake_by_ref()` when there's more work to do (`endpoint.rs:389-391`,
  `connection.rs:264-266`) rather than trusting a downstream `Poll::Pending` to naturally reschedule
  — this is a deliberate "keep_going" flag threaded through every drive_* helper. Get this wrong in
  either direction and you get either a stall (forgot to rewake → task never polled again despite
  pending work) or a storm (waking unconditionally every poll even with nothing to do, burning CPU).
  The `IO_LOOP_BOUND`/`RECV_TIME_BOUND`/`WorkLimiter` machinery in quinn
  (`quinn-0.11.11/src/work_limiter.rs`, `lib.rs:128,136`) exists specifically to bound how much work
  one poll of one task does per wakeup, so one busy connection can't starve the executor for
  everyone else sharing it — worth budgeting for even at slither's smaller scale if many
  `Connection`s can share a runtime.
- **Lock contention between driver and handles.** quinn ships a *feature-gated instrumented mutex*
  (`quinn-0.11.11/src/mutex.rs`) purely to detect and warn (`tracing::warn!`) when a lock is held
  >1ms, recording the last 20 "purposes" that touched it. This only exists because it was a real
  operational problem — every synchronous app-facing method (`Connection::send_datagram`, stream
  writes, etc.) contends with the driver task's per-poll lock, and a driver task doing expensive
  work (e.g. `poll_transmit`'s packet-building loop) while holding the lock can stall app-thread
  calls. Consider: keep the driver's per-poll critical section as short as possible, and don't do
  anything on the "cheap synchronous handle method" path that could block waiting for the driver's
  lock for unbounded time.
- **Unbounded channels = no backpressure.** Both of quinn's cross-task event channels
  (`EndpointEvent`/`ConnectionEvent`, `endpoint.rs:472`, `connection.rs:983-984`) are
  `mpsc::unbounded_channel()`. That's a deliberate simplicity trade-off (never blocks a sender) but
  it means a pathological peer that causes a flood of endpoint-facing events (e.g. connection-ID
  churn) has no backpressure until the process runs out of memory; quinn's *proto* layer bounds the
  things that matter (accept-queue bytes) but the *shell*'s plumbing does not self-limit. If slither
  reuses this channel-per-object pattern, decide explicitly whether unbounded is acceptable at
  slither's expected connection counts or whether a bounded channel + backpressure signal is needed.
- **`Instant` vs wall-clock.** quinn-proto uses `std::time::Instant` (monotonic) throughout
  (`quinn-proto-0.11.16/src/lib.rs:108`, `pub(crate) use std::time::{Duration, Instant, SystemTime,
  UNIX_EPOCH}` — note `SystemTime` is imported separately and used only where wall-clock semantics
  are genuinely needed, e.g. token/NAT timestamp validation, never for internal timers). The shell's
  `Runtime` trait explicitly documents `now()` as existing to allow "simulating the flow of time for
  testing" (`quinn-0.11.11/src/runtime.rs:26-28`) — but note the *proto core itself needs no such
  trait*: it just takes `now: Instant` as a plain argument everywhere, which is trivially mockable
  in a unit test with zero abstraction cost. The mockable-clock *trait* is purely a shell concern
  (for constructing real vs. fake `AsyncTimer`s); don't let it leak into the core's API. Getting this
  backwards — using wall-clock `SystemTime::now()` for retransmit/idle timers — is a classic bug
  class: NTP adjustments or manual clock changes can jump backwards, causing timers to appear to
  fire far in the future or immediately.
- **"Who owns the socket."** quinn splits this cleanly: the `Endpoint` driver task is the sole
  *receiver* on the UDP socket (necessary — only one task can meaningfully drain+demux a socket by
  connection ID), but every `Connection` driver task holds its own `Arc<dyn AsyncUdpSocket>` clone
  and **sends directly**, concurrently, without funneling writes through the endpoint driver
  (`connection.rs` `drive_transmit` calls `self.socket.try_send` directly). This requires the socket
  abstraction to support concurrent sends from multiple tasks (quinn's `AsyncUdpSocket::
  create_io_poller` explicitly documents supporting "any number of interested tasks" each with their
  own waker, `runtime.rs:40-48`). If slither's shell instead centralizes all sends through the
  `Endpoint` (e.g. one socket, one task doing all I/O, `Connection`s only queue), that's a valid and
  simpler alternative — but it changes the threading model (every `Connection`'s transmit now
  contends on the endpoint's queue/lock) and should be a deliberate choice, not a default.
- **`!Send` constraints.** Not directly evidenced as a quinn pain point (quinn's `Runtime`/
  `AsyncUdpSocket`/`AsyncTimer` traits all require `Send` — `runtime.rs:16,33,40` — quinn assumes a
  multi-threaded executor), but the constraint is real for sans-io libraries that also target
  single-threaded/wasm environments: str0m explicitly supports non-Send targets (browser/wasm), and
  quinn's own source has `#[cfg(not(wasm_browser))]` guards around socket-construction code
  (`quinn-0.11.11/src/endpoint.rs:18-19,141-142`) precisely because the wasm target can't assume
  `Send + 'static` executors or real OS sockets. For slither: decide up front whether the sans-io
  core itself needs to be `Send` (probably yes, if the tokio shell moves it between tasks or wraps
  it in `Arc<Mutex<>>` the way quinn does) — a pure core with no I/O handles inside it should be
  `Send`/`Sync`-free-by-default as long as it holds no `Rc`/raw pointers/thread-local state; keep it
  that way deliberately rather than accidentally, since it's what lets the shell choose Arc<Mutex<>>
  (quinn's choice) vs. a single-threaded actor without the core caring either way.

---

## Sources

**Local (read directly):**
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/endpoint.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/connection/mod.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/connection/timer.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/shared.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-proto-0.11.16/src/lib.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/endpoint.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/connection.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/incoming.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/mutex.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/runtime.rs`
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/quinn-0.11.11/src/work_limiter.rs`
- `/Users/nicolasdiprima/work/primetype/slither/TODO.md` (task context, read-only)

**Web:**
- str0m: https://docs.rs/str0m/latest/str0m/ and https://raw.githubusercontent.com/algesten/str0m/main/src/lib.rs
- Firezone, "sans-IO Pattern in Rust Networking Code": https://www.firezone.dev/blog/sans-io
- Swatinem, "Finding a usable sans-io pattern": https://swatinem.de/blog/sans-io-pattern/
- (background, not directly quoted) HN discussion "Sans-IO: The secret to effective Rust for network services": https://news.ycombinator.com/item?id=40872020

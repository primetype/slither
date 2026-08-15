# Slice 3b seam review — reviewer: Opus

Commit under review: `4a7d28e` ("Slice 3b: the driver and the handles. S1 closes.")
Charter: `PLAN.md` §5 / §8 — "the core→shell seam (highest [risk])".
Scope: `src/shell/{mod,driver,endpoint,staged,connection,shared}.rs`. `src/core/**` in
scope only where the shell drives it wrongly. **Findings, not fixes** — no file in the
repository was modified.

Status legend: **[D]** demonstrated — an exact interleaving, run as a probe outside the
repo, with its output pasted; **[C]** demonstrated by construction — the code path is
unambiguous and the argument is given; **[S]** suspected, unproven.

Probes live in
`/private/tmp/claude-501/-Users-nicolasdiprima-work-primetype-slither/721dbfa4-ee18-4ecb-8459-b449d16c1367/scratchpad/probe`
— a separate crate with a **path dependency** on slither and `features = ["test-util"]`.
Nothing in the repository was touched to run them.

---

## 0. Findings index

| # | Sev | Status | Finding | Where |
|---|---|---|---|---|
| **F1** | **Critical** | **[D]** | A driver panic runs no `shutdown()`, is swallowed by the `LocalSet`, and leaves every `Connecting` and every `closed()` parked for ever. Tests stay green. | `driver.rs:170-234` |
| **F2** | **High** | **[D]** | C-B1: the shell's static mirror and the core's `StaticMap` **do** diverge, on a race §16.1 explicitly declines to hold. In debug it detonates F1; in release it is correct. | `driver.rs:731`, `driver.rs:832-844` |
| **F3** | **High** | **[D]** | `Driver::deadline()` consumes core output outside a drain and **discards** it. A CLOSE sealed while `Wire::send_to` is `Pending` is silently lost. | `driver.rs:611-634` |
| **F4** | **High** | **[D]** | `Driver::waiting` is pruned only when `ready` is non-empty. Every cancelled `accept()` leaks 133 bytes for the endpoint's life. §10.6's unbounded queue. | `driver.rs:512-541` |
| **F5** | **Medium** | **[C]** | The only test of rulings 81/84's `Retired` ordering asserts on shell-side state that is released **independently** of whether `Retired` was delivered. Name is not a pin. | `mod.rs:398-433` |
| **F6** | **Medium** | **[D]** | Unstated scope: ruling 50's "**Nothing is transmitted**" on a `Connecting` drop is false once the attempt has completed into the slot — a CLOSE goes out. Report, do not resolve. | `endpoint.rs:230-256` |
| **F7** | **Medium** | **[C]** | §16.4's *normative* output ordering ("a transmit and the event it caused come out in that order") holds at `poll_output()` but is inverted in effect: all transmits are batched behind all events, across all cores. | `driver.rs:176-177, 241-271` |
| **F8** | **Low** | **[C]** | `command_cancel` releases the shell record **before** delivering `Retired` — literally the inverse of §16.4's MUST. Harmless today because `handle_connection_event` emits nothing; a slice-7 hazard. | `driver.rs:766-783` |
| **F9** | **Low** | **[C]** | `serve()`'s outer drain loop exhausts `DRAIN_BOUND` **silently**, unlike the two inner loops which `panic!`. | `driver.rs:244-265` |
| **F10** | **Low** | **[S]** | `wake_all()` and `PendingSlot::resolve` invoke wakers while a `RefCell` borrow is live. Sound with tokio's LocalSet waker; unsound with any waker that polls inline. | `driver.rs:398-402`, `shared.rs:278-283` |
| **F11** | **Info** | **[C]** | `shared.rs` is a seventh file the plan's §2.2 table does not list (IMPL-B flagged this itself; confirmed, and it is the right call). | — |

**Chartered targets with no finding**, argued in full below: `Retired` ordering on every
path but F8's (target 2), waker-map boundedness and borrow-across-`await` (target 3),
cancel-safety of the four staged/connection verbs (target 4), and the ruling-88 drop
matrix (target 5).

Verified green on `4a7d28e` before reviewing:
`cargo test --all-features` → 297 + 103 + 11 + 4 + 9 + 4 + 16 + 7 = **451 passed, 0 failed**;
`cargo clippy --all-features --all-targets -- -D warnings` → clean;
`cargo fmt --all --check` → no diff.

---

## 1. Target 1 — the poll contract's drain discipline

### 1.1 Clean: every mutating call is drained

Every mutating core call in the shell is followed by a return to `run()`'s loop head,
whose **first statement** is `self.serve()` (`driver.rs:176`). Enumerated, so the list is
checkable rather than asserted:

| Mutating call | Site | Drained by |
|---|---|---|
| `endpoint.connect` | `driver.rs:697-702` | loop head |
| `endpoint.handle_datagram` | `driver.rs:574-579` | loop head |
| `endpoint.handle_timeout` | `driver.rs:594` | loop head |
| `endpoint.read_identity` | `driver.rs:655` | loop head |
| `endpoint.authenticate` | `driver.rs:659-664` | loop head |
| `endpoint.accept` | `driver.rs:792` | loop head |
| `endpoint.reject` | `driver.rs:670` | loop head |
| `endpoint.handle_connection_event` (cancel) | `driver.rs:775-779` | loop head |
| `endpoint.handle_connection_event` (`Retired`) | `driver.rs:361-365` | `serve()`'s **outer** loop re-enters `serve_endpoint` after every connection pass |
| `conn.handle_endpoint_event` | `driver.rs:316` | `dirty = true`, then the same pass |
| `conn.handle_datagram` | `driver.rs:586` | `dirty = true`, loop head |
| `conn.handle_timeout` | `driver.rs:598` | `dirty = true`, loop head |
| `conn.close` (handle side) | `connection.rs:140` | `dirty = true` + `Command::Dirty` |

The `Retired` row is the one that could have gone wrong and does not: `serve()`'s outer
loop is `serve_endpoint(); collect dirty; break if empty; drain each`, so an endpoint
mutation caused by a connection drain is always followed by another `serve_endpoint`.

`Connection::close_now` (`connection.rs:135-153`) is the handle-side mutation, and it sets
`cell.dirty` **and** pushes `Command::Dirty` — belt and braces, and correct: the flag makes
the very next `serve()` pick it up even if the command is still queued.

### 1.2 F3 [D] — `Driver::deadline()` consumes and discards core output

`driver.rs:611-634`:

```rust
fn deadline(&self) -> Option<std::time::Instant> {
    let endpoint = match self.shell.state.borrow_mut().endpoint.poll_output() {
        EndpointOutput::Timeout(deadline) => deadline,
        _ => { debug_assert!(false, "the endpoint core queued an output outside a drain"); None }
    };
    self.conns.values().filter_map(|record| {
        let mut cell = record.cell.borrow_mut();
        match cell.core.as_mut()?.poll_output() {
            ConnOutput::Timeout(deadline) => deadline,
            _ => { debug_assert!(false, "a connection core queued an output outside a drain"); None }
        }
    }).chain(endpoint).min()
}
```

`core::Connection::poll_output` is `self.outputs.pop_front().unwrap_or_else(…)`
(`src/core/connection/mod.rs:323-327`) — it **pops**. The `_` arm therefore does not merely
mis-report the deadline; it **destroys a datagram**.

The window is real. `run()` is

```
serve()          →  transmit(outgoing).await   →  handles check  →  deadline()  →  select!
```

and `transmit().await` (`driver.rs:552-566`) is the only yield point between the drain and
`deadline()`. `Wire::send_to` is an `async fn` on a user-supplied trait
(`src/shell/wire.rs:63`); `tokio::net::UdpSocket::send_to` returns `Pending` whenever the
socket send buffer is full. When it does, a handle task runs, calls `close()` (or drops the
last `Connection`), the core queues `Transmit(CLOSE)` — and `deadline()` eats it.

**`testutil::FlakyWire` never returns `Pending`, so the entire 451-test suite cannot reach
this.** That is target 6's shape exactly.

Probe (`probe/tests/deadline.rs`): a `YieldWire` whose `send_to` yields once before
delegating — i.e. an ordinary socket under backpressure — plus two connections A↔B and
A↔C. Close B, yield once so the driver suspends inside `send_to`, then close C.

```
$ cargo test --test deadline -- --nocapture          # debug
thread '…' panicked at src/shell/driver.rs:627:25:
a connection core queued an output outside a drain
peer B saw: Ok(PeerClosed { code: 1, reason: [98, 101, 101] })
peer C saw: Err(Elapsed(()))
test a_close_sealed_while_the_wire_is_blocked_is_dropped_by_deadline ... ok

$ cargo test --release --test deadline -- --nocapture
peer B saw: Ok(PeerClosed { code: 1, reason: [98, 101, 101] })
peer C saw: Err(Elapsed(()))
test a_close_sealed_while_the_wire_is_blocked_is_dropped_by_deadline ... ok
```

Cost: in **release**, a CLOSE is silently lost and the peer pays `DEAD_TIMEOUT` (25 s) —
precisely the cost §15.1 says CLOSE exists to avoid, and with no trace, because ruling 49's
tracing only covers a `send_to` that *returns* `Err`. In **debug**, the driver panics, which
is F1. Note the test *passes* in both.

The shape of the defect is that `deadline()` is written as if `poll_output()` at the
terminal were a pure read. It is, when the queue is empty — but the function has no way to
establish that the queue is empty, because the only thing that establishes it is a drain
with no intervening yield, and there **is** an intervening yield.

### 1.3 F7 [C] — the normative ordering holds at `poll_output()` and not in effect

§16.4 (SPEC.md:4634-4636) is explicit that this is normative:

> - **Output ordering within one drain preserves generation order** — a
>   transmit and the event it caused come out in that order. Normative;
>   tests and logs depend on it.

`serve()` consumes outputs in order, so the letter is met. But `ConnOutput::Transmit` is
**pushed onto a `Vec<Outgoing>`** (`driver.rs:351-354`) that is not flushed until
`transmit()` runs after the *whole* pass (`driver.rs:176-177`), while `ConnOutput::Event`
and `ConnOutput::ToEndpoint` are acted on inline (`driver.rs:355-367`). So:

* every `closed()` waiter for a connection is woken (`latch`, `driver.rs:397-403`) **before**
  that connection's CLOSE bytes are handed to the wire;
* transmits from the endpoint core and from *every* connection are interleaved into one
  batch across all outer-loop iterations, so a connection's transmit can be flushed after an
  unrelated connection's event.

I do not think this is currently observable as a protocol defect — §16.2 already resolves
`close()` at the seal, not the send — and batching is a legitimate design. But §16.4 says
"tests and logs depend on it", and the `slither::io` trace ordering (§18.2) is a log that
now does not match generation order. **Reported, not resolved**: the spec sentence is about
`poll_output()`'s output sequence, and whether it also constrains the shell's *actions* is
not stated. That is a scope question for the maintainer, not for a reviewer to pick.

### 1.4 F9 [C] — the outer drain loop exhausts silently

`serve()` (`driver.rs:244-265`) is `for _ in 0..DRAIN_BOUND { … }` with no trailing
`panic!`, while `serve_endpoint` (`driver.rs:324-326`) and `serve_connection`
(`driver.rs:370-372`) both end in a named panic. If the outer loop ever exhausts, the shell
proceeds to `release_dead`/`prune_ready`/`deadline()` with connections still dirty and the
endpoint core undrained — i.e. straight into F3's discard path — instead of failing loudly.
The asymmetry is almost certainly an oversight; the two inner guards show the intent.

### 1.5 Clean: no announced deadline is dropped

`deadline()` takes the `min` over the endpoint core's `Timeout` and every live connection's
`Timeout`, and `sleep_until` is armed only when it is `Some` (`driver.rs:197-205`). A
connection released by `release_dead` has already been removed from `self.conns`, so it
contributes nothing and needs to contribute nothing. The `Event::Timeout` arm calls
`handle_timeout` on **all** cores, which is correct under §16.5's idempotency guarantee
(`src/core/connection/mod.rs:249-252`, `src/core/endpoint/mod.rs:~690`).

---

## 2. Target 2 — `Retired` ordering

§16.4 (SPEC.md:4626-4631):

> In both cases the shell delivers `Retired` to `handle_connection_event`
> **before** releasing the connection's shell-side bookkeeping (else the
> index route and the guard-entry pin leak for the endpoint's life). The
> all-handles-dropped case is exempt (the driver simply stops).

### 2.1 Clean, and here is the argument

**The mechanism.** `serve_connection` handles `ConnOutput::ToEndpoint` by calling
`endpoint.handle_connection_event` **inside the drain loop that produced it**
(`driver.rs:355-366`). `release_dead` — the only place shell bookkeeping is released — runs
after the whole outer loop (`driver.rs:267`). No ordering exists in which the release
precedes the delivery.

**The predicate is the right one.** `release_dead` keys on
`cell.closed.is_some() && !cell.is_established()` (`driver.rs:485-488`), not on "did I see a
`Retired`". I checked this against the core rather than trusting the comment.
`Connection::drop_state` (`src/core/connection/mod.rs:517-526`) is the *only* place the
session is taken, and it pushes `Retired` into `outputs` in the same statement:

```rust
fn drop_state(&mut self) {
    if let Some(session) = self.session.take() {
        let our_index = session.established().our_index;
        self.outputs.push_back(ConnOutput::ToEndpoint(ToEndpoint::Retired { our_index }));
    }
    …
}
```

So `is_established()` goes false **only** in the same call that queues `Retired`, and
`serve_connection` drains to `Timeout` before returning — the `Retired` is therefore always
consumed before `release_dead` can observe the false. The ruling-84 case (no session ever
installed) emits `Closed` alone (`src/core/connection/mod.rs:292-315`) and is released by
the same predicate without waiting for a `Retired` that will never come. Correct.

**Can a connection reach `closed && !established` without being drained?** No. The core
mutates only inside `handle_datagram` / `handle_timeout` / `handle_endpoint_event` /
`close`, and every one of those call sites in the shell sets `cell.dirty = true` in the same
block (`driver.rs:316-318`, `584-588`, `596-600`, `connection.rs:140-142`). `handle_timeout`
marks **every** connection dirty, so a linger expiry can never be missed.

**Panics and early returns.**
* `serve_connection`'s `None => return` on a released core is unreachable while the record
  is in `self.conns` — `release_dead` removes the record and nulls the core in the same
  iteration (`driver.rs:493-503`).
* `fail_pending` (`driver.rs:461-472`) removes a record without a `Retired`. Correct: it is
  driven by `EndpointOutput::HandshakeFailed`, which is the core telling us it has already
  run `drop_pending` — which itself removes the pending index, the static and the guard pin
  (`src/core/endpoint/mod.rs:506-516`).
* `shutdown()` (`driver.rs:873-883`) nulls every core without delivering `Retired`. The spec
  exempts this case verbatim ("The all-handles-dropped case is exempt"), and it is harmless
  anyway: the endpoint core lives in `ShellState`, which is only reachable from handles and
  from staged objects, all of which die with it.
* A **driver panic** skips `shutdown()` entirely — see F1. It does not leak a route (the
  whole endpoint core is orphaned) but it does hang the handles.

**Guard-entry pin.** `handle_connection_event` releases it on both branches
(`src/core/endpoint/mod.rs:808-838`): via `drop_pending` for a pending, or via
`remove_by_connection` + `guard.unpin` for a live connection. Both are reached from
`serve_connection` and from `command_cancel`. `our_index: 0` is safe as IMPL-B says —
`IndexTables::mint` (`src/core/endpoint/tables.rs:45-56`) redraws while zero — and I
confirmed that `drop_pending` keys on `ConnectionId` and removes the static **by the
pending's own stored key**, so the placeholder index never reaches anything that could
mis-key.

### 2.2 F8 [C] — `command_cancel` inverts the order the MUST states

`driver.rs:766-783`:

```rust
let Some(id) = slot.borrow().id else { return; };
let Some(_record) = self.conns.remove(&id) else { return; };      // ← bookkeeping released
self.shell.state.borrow_mut().endpoint
    .handle_connection_event(now(), id, ToEndpoint::Retired { our_index: 0 });   // ← MUST, after
```

This is the literal inverse of the §16.4 sentence. It is harmless **today** for one reason
only: `handle_connection_event` is documented and implemented to emit nothing
(`src/core/endpoint/mod.rs:801-803`), so no endpoint output generated by it can be looked up
against the record that has just been removed.

It becomes a defect the moment that stops being true. §16.1 says the PENDING-branch
tie-break at `accept()` "cancels the pending and installs in its place" — slice 7 — and any
output the core emits for `id` during a cancel would find `self.conns.get(&id)` returning
`None` and be **silently dropped** (`driver.rs:313`, `driver.rs:462`). Swapping the two
statements costs nothing and removes the hazard.

### 2.3 Clean: no leak on the redial races

The stamped mirror does hold up against every cancel/give-up interleaving I could
construct, and I want to say so explicitly because it is the part IMPL-B designed for:

* `[Connect(1), Cancel(1), Connect(2)]` — the FIFO plus `biased` on the command arm
  (`driver.rs:201-202`) keeps the order; `Cancel(1)` runs `drop_pending`, which frees the
  core's static, before `Connect(2)` tests it.
* Give-up racing a drop — `fail_pending` releases `(K, 1)`; a redial has already claimed
  `(K, 2)`; `release_static`'s stamp check (`shared.rs:357-365`) declines. Correct.
* Drop after `HandshakeFailed` — `Connecting::drop`'s `in_flight` guard
  (`endpoint.rs:237-238`) sees `Failed`, sends no `Cancel`, releases no static. Correct.
* Drop after establishment — same guard sees `Ready`. Correct, and this is the case that
  would otherwise have retired a **live** session with `our_index: 0`. It is the sharpest
  thing in `endpoint.rs` and it is right.

The hole is not here. It is in a race the stamping scheme does not model at all: §7.

---

## 3. Target 3 — waker registration under `RefCell`

### 3.1 Clean, with the argument

**No borrow is held across an `await`.** I checked every `borrow()`/`borrow_mut()` in
`src/shell/` for the longest path it can span:

* `driver.rs` — every core call is inside a non-`async` helper. The two places where a
  borrow could have escaped into a block the function does not control are handled
  deliberately: `run()` binds `let handles = self.shell.state.borrow().handles;` to a `let`
  rather than leaving it a condition temporary (`driver.rs:190`, with the comment saying
  why), and `serve_connection` computes `output` inside a `{ }` that closes before the
  `match` (`driver.rs:340-347`). `serve()`'s dirty scan borrows per-item inside a closure.
* `connection.rs` — `poll_close`/`close_now` borrow inside a block, then call
  `mark_dirty` with the borrow released (`connection.rs:136-153`). `poll_closed` holds one
  borrow for check-and-park, which is the correct shape.
* `endpoint.rs` — `connect`'s claim is inside a `{ }` whose `?` unwinds the guard
  (`endpoint.rs:112-122`); `Connecting::poll` holds one borrow and does no reentrant call.
* `staged.rs` — accessors take `borrow()` for the duration of one `intro_source` call.

**The waker map is bounded.** `Wakers` (`shared.rs:52-99`) mints monotone `u64` keys, and
`WakerSlot`'s `Drop` (`shared.rs:122-126`) unparks unconditionally — including on
cancellation and including for a future that never parked. `wake_all` takes the map rather
than iterating it, so no stale waker survives a death. `Connection::closed()`
(`connection.rs:175-186`) mints its key **before** the first poll and holds the guard across
the whole `poll_fn`, so there is exactly one entry per live future and zero per dead one.
Confirmed by `mod.rs`'s `closed_is_latched_concurrent_and_cancel_safe`, which polls a future
to `Pending` and then drops it.

**No lost wake.** `poll_closed` (`connection.rs:190-199`) reads `cell.closed` and parks
under **one** `borrow_mut()`, so there is no check-then-park gap. On a single-threaded
runtime there is no gap to have anyway, but the code does not depend on that.

**No re-entrant `borrow_mut()` panic.** The only calls that can run foreign code under a
live borrow are `Waker::wake()` and the implicit `Drop` of a replaced value; see F10.

### 3.2 F10 [S] — wakers are invoked under a live borrow

`Driver::latch` (`driver.rs:397-403`) holds `cell.borrow_mut()` across
`closed_wakers.wake_all()`, and `PendingSlot::resolve` (`shared.rs:278-283`) holds `&mut
self` across `waker.wake()`. With tokio's `LocalSet` waker, `wake()` only pushes to the
local run queue, so this is sound and I could not construct a failure. It is unsound with
any waker that polls inline, and a `closed()` future is exactly the sort of thing a consumer
wraps in a hand-rolled combinator. Hoisting the wakers out of the borrow (collect, drop the
guard, then wake) costs nothing. Unproven; flagged because the charter names it.

A related instance I checked and cleared: `PendingSlot::resolve` assigns `self.outcome =
outcome`, which **drops the previous outcome under the borrow**. If that were a
`Ready(Connection)`, `Connection::drop` would take `cell.borrow_mut()` and
`state.borrow_mut()` re-entrantly. It cannot happen: every driver-side path takes
`record.slot` out (`driver.rs:436`, `469`, `504`) so a slot is resolved at most once.

---

## 4. Target 4 — cancel-safety of every `async fn`

Verb by verb, at each await point.

| Verb | Cancelled at | What happens | Verdict |
|---|---|---|---|
| `Endpoint::accept` | before the send | nothing sent | clean |
| `Endpoint::accept` | after the send, before the reply | `Command::Accept(tx)` is already queued; the driver pushes a **dead sender** into `waiting` | **F4** |
| `Endpoint::accept` | after the driver sent the `Intro` | `oneshot::Receiver::drop` drops the stored `Intro` → `Intro::drop` → `Command::Reject`. §6.2's silent reject | clean, and documented |
| `Intro::read_identity` | mid-round-trip | `self` is kept alive across the await and `consumed` set only after (`staged.rs:126-131`), so `Intro::drop` sends `Reject`. 1 DH already spent is not refunded | clean |
| `Claimed::authenticate` | mid-round-trip | same shape (`staged.rs:195-199`) | clean |
| `Proven::accept` | before the driver ran | `Proven::drop` → `Reject(id)`; the core's `discard_chain` reverts the §17.1 provisional write | clean |
| `Proven::accept` | after the driver built the connection | `drop(reply.send(Ok(handle)))` (`driver.rs:868`) drops the `Connection` — last handle — which seals `close(NO_ERROR, "")`. A later `Reject(id)` on a consumed id is a no-op (`src/core/endpoint/staged.rs:594-598` is `if let Some(entry) = …`) | clean |
| `Connection::close` | any | `poll_close` is `Ready` on the first poll (`connection.rs:128-131`); dropping before the first poll seals nothing, which matches §16.2 resolving at the seal | clean |
| `Connection::closed` | any | `WakerSlot::drop` unparks; the latch is not consumed | clean |
| `Connecting` (a `Future`, not an `async fn`) | any | dropping **is** the cancel — the specified behaviour. But see F6 for the completed-and-unpolled case | see F6 |

Two things I want to record because they took work to clear:

* **`round_trip` checks `driver_stopped` then sends then awaits** (`staged.rs:317-327`). If
  the driver stops in the gap, the `oneshot` sender dies with the `Driver` and `rx.await`
  errors → `None` → `EndpointDropped`. There is no hang. This is why F1's damage is confined
  to `Connecting` and `closed()`.
* **`dispatch_intros`'s `is_closed()`-then-`send()`** (`driver.rs:518-539`) is genuinely
  atomic with respect to the application, because no other task on the thread can run while
  the driver is between the two statements. The comment claims this and it is true.

### 4.1 F4 [D] — `Driver::waiting` grows without bound

`driver.rs:512-541`:

```rust
fn dispatch_intros(&mut self) {
    while !self.ready.is_empty() {                                  // ← the only gate
        while self.waiting.front().is_some_and(oneshot::Sender::is_closed) {
            self.waiting.pop_front();
        }
        …
    }
}
```

Dead senders are pruned **only while an introduction is available**. On an endpoint that
receives none — a pure client, or a server between connections — nothing is ever pruned, and
`Command::Accept(reply)` unconditionally pushes (`driver.rs:650-653`). The triggering
pattern is the canonical cancellation idiom:

```rust
loop {
    tokio::select! {
        intro = endpoint.accept() => { … }
        _ = &mut shutdown => break,
    }
}
```

or `timeout(d, ep.accept())`. Every iteration the accept arm loses leaves one
`oneshot::Sender<Intro<I>>` parked for the endpoint's life.

This is the mirror image of the hazard `prune_ready` exists for. IMPL-B documented that one
at length (`driver.rs:273-289`, decision D12) — "an application that never calls `accept()`
would otherwise accumulate one dead id per expiry for the endpoint's life, which is exactly
the unbounded intermediate queue §10.6 forbids" — and did not apply the same reasoning to the
queue on the other side of the same function.

Probe (`probe/tests/waiting_leak.rs`), counting global allocator, 20 000 cancelled accepts,
with a control that runs the identical loop shape without `accept()`:

```
live bytes before 20000 cancelled accept()s: 202523
live bytes after : 2862619
growth: 2660096 bytes  (133 bytes per cancelled accept)
CONTROL growth over 20000 identical cancellations: 0 bytes
```

The control at exactly zero rules out the runtime, the timer wheel and the fixture. Growth
is linear and monotone; nothing frees it short of the endpoint dying.

Note also that the rustdoc on `Endpoint::accept` (`endpoint.rs:68-74`) says "Dropping the
future before it resolves **takes nothing**". True of the introduction; false of the
driver's memory. Working rule 8's shape — a stated construction with an unstated scope.

---

## 5. Target 5 — drop order across handles

### 5.1 Clean: the ruling-88 matrix

`Connection::drop` (`connection.rs:258-276`) is:

```rust
let last_for_connection = { …; cell.handles -= 1; cell.handles == 0 };
let last_in_process = self.shell.release();
if last_for_connection && !last_in_process { self.close_now(NO_ERROR, b""); }
```

The ordering — `release()` before the decision — is load-bearing and correct, and the
comment says so. I traced all four orderings:

| Order | Result | Ruling |
|---|---|---|
| `drop(connection)` with `Endpoint` alive | `last_in_process` false → CLOSE sealed, `Command::Dirty` queued | §16.2 local-close row ✔ |
| `drop(endpoint); drop(connection)` | `release()` hits 0 → `last_in_process` true → **no CLOSE** | ruling 88 ✔ |
| `drop(connection); drop(endpoint)` | CLOSE sealed first; the driver's `serve()`+`transmit()` run **before** the `handles == 0` check (`driver.rs:176-193`) so the CLOSE reaches the wire | D8 ✔ |
| `drop(connecting)` as the last handle | `Cancel` queued, then `HandlesGone`; the biased command arm processes `Cancel` first | ruling 50 ✔ |

`Connecting::drop` also cannot trip `Shell::release`'s "released twice" `debug_assert`
(`shared.rs:435`): a slot holding a `Ready(Connection)` has already `acquire`d, so the count
is ≥ 2 when the `Drop` body runs and the field drop that follows brings it to ≥ 1.

**Drop of a partially built handle.** `EndpointBuilder::build` acquires before
`spawn_local` (`endpoint.rs:391-392`). If `spawn_local` panics (outside a `LocalSet`), the
`Driver` future — which owns the command receiver — is dropped, the `Endpoint` is dropped
during unwind, `release()` fires, `send(HandlesGone)` fails silently, and nothing leaks. The
three `expect`s before it all fire before any state is acquired.

**Drop during unwind.** `Connection::drop` calls into the core during unwind. If the core
panicked there it would abort; nothing in `core::Connection::close` allocates or asserts on a
path a drop can reach (`src/core/connection/mod.rs:292-315`), so I do not think this is a
live risk. Unproven.

### 5.2 F6 [D] — ruling 50's "Nothing is transmitted" has an unstated scope

Ruling 50 (rulings.md:734-749) and §16.3 both say, without qualification, that dropping a
`Connecting` transmits nothing:

> **Nothing is transmitted.** An attempt that never completed has no session to close and
> no wire signal to send, as in §15.4's endpoint-dropped row.

The rationale sentence scopes it to "an attempt that never completed". The **rule** does
not. And the case the rule does not cover is reachable: the driver resolves the slot to
`Ready(Connection)` (`driver.rs:436-450`), the application never polls, and drops the
`Connecting`. `in_flight` is then false (`endpoint.rs:237-238`), the slot field drops, the
parked `Connection` is the last handle to its connection, and `close(NO_ERROR, "")` fires.

Probe (`probe/tests/connecting_drop.rs`):

```
A transmitted 1 datagram(s) on the drop
peer B saw: Ok(PeerClosed { code: 0, reason: [] })
immediate redial ok = false
```

I believe the implementation's behaviour is the **better** one — §16.2's last-handle rule is
the more specific rule, and telling the peer at once beats 25 s of `DEAD_TIMEOUT`. But
working rule 3 says report, do not resolve, and this is the third instance in this project of
prose carrying a scope the formal rule omits. TEST-B found the same thing independently and
routed around it (`tests/spec_shell.rs:828-838`, gap G10: "dropping the `Connecting` then
drops a **completed** `Connection`, which §16.2 turns into `close(NO_ERROR, \"\")`"). Two
independent agents hitting one ambiguity is the signal the spec needs a sentence.

Note the second line of probe output: `immediate redial ok = false`. That is correct — the
static is genuinely LIVE — but it means the S29 idiom `drop(connecting); connect(same)` gives
a *different* answer depending on whether the attempt happened to complete first. That
asymmetry is worth a sentence in §16.3 wherever ruling 50's MUST is stated.

---

## 6. Target 6 — code tuned to a test rather than to the spec

Working rule 9 applied to all 29 story tests plus the 12 in-crate smoke tests. The tests are
of unusually high quality — nearly every one names the mutation it separates and asserts from
the separating side, and several (`s2_no_answer_gives_timed_out_at_giveup`,
`s27_closed_is_latched_and_concurrent`, `s1_close_reason_is_truncated_at_close_reason_max`,
the ruling-88 pair) are model examples. The bound in
`s29_retry_loop_replaces_rather_than_accumulates` (`3..=9` against one train's 5-6 and two
trains' 10-12) is a genuine two-sided separator. I found **one** test that does not pin what
it is named for, plus three "works because the harness never does X" gaps.

### 6.1 F5 [C] — the `Retired`-ordering test cannot see `Retired`

`src/shell/mod.rs:388-433`,
`a_closed_connection_frees_its_static_when_the_linger_expires`. Its own rustdoc states the
mutation it claims to catch:

> The broken version — releasing the shell record first, so `Retired` never reaches the
> endpoint core — leaves the static LIVE for the endpoint's life, and the redial below
> returns `AlreadyConnected`.

Both of its assertions read **shell-side** state:

```rust
assert!(!a.is_established(), "the linger never expired");        // reads ConnCell::core
let redial = pair.a.endpoint.connect(…).expect("`Retired` never reached the endpoint core");
```

`is_established()` reads `ConnCell::core`, which `release_dead` nulls (`driver.rs:503`)
regardless of whether `Retired` was delivered. `connect()` reads `ShellState::statics` — the
**mirror** — which `release_dead` frees on the line above (`driver.rs:496-499`), also
regardless. Neither assertion can observe `core::Endpoint::statics` at all.

So the minimal mutation — delete the `handle_connection_event` call at `driver.rs:361-365`
— leaves both assertions passing. The core would then hold the static for ever, the mirror
would say NONE, `connect()` would return `Ok`, and the `Connecting` it hands back would be
dead on arrival: `Command::Connect` reaches the driver, the core answers
`AlreadyConnected`, and `debug_assert!(false)` at `driver.rs:731` panics the driver task —
**which the `LocalSet` swallows** (F1). The test's `drop(redial); drop(b);` never notices.
Green.

I did not apply that mutation (the charter forbids it), but the cb1 probe below demonstrates
every link in the chain independently: it shows exactly the state "mirror free, core
occupied", it shows `connect()` returning `Ok`, it shows the resulting `Connecting` parked
for ever, and it shows the harness reporting `test … ok` through a driver panic.

**Why it matters more than an ordinary weak test:** this is the *only* test anywhere that
addresses rulings 81/84's linger path. `s29_after_cancel_the_static_routes_as_none` covers
the **cancel** path properly (it drives an inbound accept, which fails `Stale` if the core's
pending survived — a genuine core-side observation), and `s29_cancelled_train_transmits_nothing_further`
covers the train. Nothing does the equivalent for the linger. The fix shape is the same one
S29 already uses: after the linger, drive a real inbound initiation from that peer to
completion, or assert the redial's msg1 actually leaves — something that reads the **core**.

### 6.2 Three "works because the harness never does X"

* **F3** — the entire suite runs on `testutil::FlakyWire`, whose `send_to` never returns
  `Pending`. Every real socket does. The one code path that depends on "no yield between
  `serve()` and `deadline()`" is therefore untested by construction.
* **F4** — no test cancels an `accept()`. `Endpoint::accept` is only ever awaited to
  completion (all 29 story tests) or checked for `EndpointDropped`
  (`a_staged_object_does_not_keep_the_driver_alive`).
* **F2** — no test dials and accepts the *same* static. §16.1 says that race is deliberately
  left to `accept()` to resolve, so it is not an exotic case; it is the case the spec spends a
  paragraph on.

### 6.3 One further note on the harness, not a finding

A panicking `spawn_local` task is silent under `LocalSet`: every probe above printed a
driver panic **and** `test … ok`. That is a property of tokio, not of this code, but it means
the project's chosen leak/liveness detector (`LocalSet` completion, used well in
`s26_last_handle_drop_stops_the_driver`) does not double as a panic detector. A
`tests/`-level guard — e.g. joining the driver's `JoinHandle`, or a
`std::panic::set_hook` that fails the test — would have caught F2 and F3 the first time
either was reached.

---

## 7. Target 7 — IMPL-B's five findings, verified

| IMPL-B finding | Verdict |
|---|---|
| **C-B1** — ruling 87 says `connect()` "only mints the pending"; the frozen core mints **and** spends 2 DH | **Confirmed, and worse than reported.** §7.1 |
| **F1** (theirs) — ruling 89's reachability claim does not hold through `Handshake::Seal` | **Confirmed.** `Handshake::session_id` now exists at `src/packet/handshake.rs:169` with its impl at `src/packet/suite.rs:412-416`. Additive to a `pub trait` only `channel!` implements; no wire byte, constant, frame or timer moves — I checked the golden-wire and size tests still pass byte-identical (`tests/spec_constants.rs`, `tests/spec_packet.rs`, 107 tests green). The maintainer should still ratify the trait addition, since `src/packet/` is a slice-1 file. |
| **F2** (theirs) — §16.2's four accessors are total, §16.9 admits pre-establishment handles | **Confirmed and currently unreachable**, exactly as stated. `Connecting` resolves only on `ConnEvent::Established` (`driver.rs:383`) and `Proven::accept` returns an installed connection (`driver.rs:801-811`). Worth adding: `core::Connection::close` already has a pre-establishment arm (`src/core/connection/mod.rs:296-314`) that emits `Closed(LocallyClosed)` with no `Retired` — so the *core* half of §16.9 is built and only the handle is missing. Slice 4 will hit this on its first line. |
| **F3** (theirs) — `EndpointOutput::Transmit` cannot carry ruling 49's attribution | **Confirmed.** `Outgoing` carries `conn: Option<ConnectionId>` and the endpoint branch passes `None` (`driver.rs:305-308`). The `Wire` rustdoc makes the trace a MUST "against the connection whose datagram it was" (`src/shell/wire.rs:29-32`), so a failed msg1/msg2/retransmit currently violates the letter of it. The one-field fix IMPL-B describes is right. |
| **I1** — `clippy::async_yields_async` | **Already closed by the narrow form.** `Cargo.toml` has no `[lints]` table; the `#[allow]` sits on the one function at `tests/story_lifecycle.rs:620`. Clippy is clean at `-D warnings`. |
| "Not a defect, recorded because it was checked: `our_index: 0` is safe" | **Confirmed** against `IndexTables::mint` (`src/core/endpoint/tables.rs:45-56`) and `drop_pending` (`src/core/endpoint/mod.rs:506-516`). The reasoning is sound and the note was worth writing. |

### 7.1 C-B1 — the two questions the charter asks

#### (a) Can the mirror and the core's map disagree, and what happens if they do?

**Yes. Demonstrated.** And the divergence is not an exotic interleaving — it is the race
§16.1 spends a paragraph *declining to prevent*:

> A **staged chain in progress is deliberately not in `connect()`'s list**, and cannot be:
> until `authenticate()` the chain's static is merely claimed, and §6.1 forbids keying
> anything durable on an unproven claim. The invariant is held at the other end of that race
> instead — a proven static that is PENDING at `accept()` runs §6.7's comparison …
> Either way … **no static is ever LIVE and PENDING at once.**
> — SPEC.md:3915-3940

The interleaving. `Proven::accept()` queues `Command::AcceptChain(K)` on its first poll and
yields. The application then calls `Endpoint::connect(K)` before the driver is scheduled —
the synchronous verb ruling 87 requires. `claim_static` (`shared.rs:337-351`) reads the
mirror, sees NONE (because nothing has landed yet), and writes PENDING. The queue is now
`[AcceptChain(K), Connect(K)]`, and:

1. `command_accept_chain` runs. `core::Endpoint::accept` finds its own `statics` map empty
   for K and succeeds. The shell then does a **raw, unconditional, unstamped**
   `state.statics.insert(static_key, StaticSlot { attempt, state: Live })`
   (`driver.rs:832-844`), **overwriting** the PENDING entry the synchronous `connect()` just
   wrote.
2. `command_connect` runs. `core::Endpoint::connect` now finds K present
   (`src/core/endpoint/mod.rs:369-371`) and returns `AlreadyConnected`.
3. `debug_assert!(false, "the shell's static map admitted a connect the core refused: …")`
   (`driver.rs:731`) fires.

Probe (`probe/tests/cb1.rs`) — the two arms of a `tokio::join!`, which is about as ordinary
as application code gets:

```rust
let (accepted, dialled) = tokio::join!(proven.accept(), async { ep_a.connect(addr_b, pk_b) });
```

```
$ cargo test --test cb1 -- --nocapture               # debug
thread '…' panicked at src/shell/driver.rs:731:17:
the shell's static map admitted a connect the core refused: AlreadyConnected
accepted = Ok("Ok")
connect() returned = Ok("Ok")
OUTCOME: Connecting still pending
test accept_chain_ahead_of_connect_for_the_same_static ... ok

$ cargo test --release --test cb1 -- --nocapture
OUTCOME: Connecting resolved Err(AlreadyConnected)
test accept_chain_ahead_of_connect_for_the_same_static ... ok
```

**What happens if they disagree** is therefore split cleanly:

* **Release** — the outcome is *correct*. `release_static`'s stamp check declines to remove
  the accept's entry, and the caller gets `Err(ConnectError::AlreadyConnected)`, which is
  exactly what §16.1 prescribes. The mirror's duplication does not corrupt anything.
* **Debug — including every `cargo test` run, and any consumer's dev build — the endpoint
  dies.** The driver task unwinds, `shutdown()` never runs, and F1 takes over. Blast radius
  probe (`probe/tests/cb1b.rs`) on an endpoint with an *unrelated* healthy connection:

```
A<->C is_established() = true                                  ← stale cell, still says live
A<->C closed() within 300 virtual seconds = Err(Elapsed(()))   ← never resolves. ever.
C<->A closed() within 300 virtual seconds = Ok(TimedOut)       ← the peer reaps at DEAD_TIMEOUT
A: a later connect() to a fresh static = false
LocalSet::run_until returned
test blast_radius ... ok
```

One unlucky `join!` and every connection on that endpoint is frozen, every `closed()` future
is parked for ever, `is_established()` lies, and the test harness says `ok`.

So the `debug_assert` is not a safety net. It **encodes an invariant §16.1 explicitly says
is not held on this side of the race**, and in the build where it is active it converts a
legal, spec-anticipated outcome into a fatal one. The minimum change is to delete the
assertion and let the `Err` branch stand — it is already correct. The right change is
IMPL-B's own suggestion: split the core into `mint_pending` / `start_attempt` and delete the
mirror.

#### (b) Is the stamping scheme sufficient, or does it only shrink the window?

**Neither, exactly: it is sufficient for the race it was designed for and blind to the one
that exists.**

The scheme protects **removals**. All four removal sites are stamp-checked —
`Connecting::drop` (`endpoint.rs:244-247`), `fail_pending` (`driver.rs:465-468`),
`release_dead` (`driver.rs:496-499`), `command_connect`'s error branch
(`driver.rs:735-738`) — and I could not construct a cancel/give-up/redial interleaving that
defeats them (§2.3). On that axis IMPL-B's analysis in decision D3 is correct and the design
is sound.

But **insertions are not stamped, and one of them is unconditional**:

| Writer | Guard |
|---|---|
| `claim_static` (`shared.rs:337-351`) | `contains_key` — refuses to overwrite ✔ |
| `establish` (`driver.rs:425-434`) | `get_mut` + `slot.attempt == record.attempt` ✔ |
| `command_accept_chain` (`driver.rs:832-844`) | **none — a bare `insert`** ✘ |

That third row is the whole finding. It is also the only writer that runs on the *inbound*
side, which is why the comment beside it ("`claim_static` is not used: the shell's map is the
*outbound* admission test, and an inbound replacement is §5.4's business, decided in the
core, which has already decided it") is where the reasoning goes wrong: the core has decided
about **its** map, which was empty; it has said nothing about the mirror, which was not.

So the answer to (b) is that the window is not merely smaller — for the accept-vs-connect
race there is no stamping at all, and the window is exactly "between the synchronous
`connect()` and the driver's next turn", which ruling 87 *guarantees* is non-empty. Making
the insert stamp-aware would not fix it either: the mirror genuinely cannot know, at
`claim_static` time, that a proven chain for the same static is one command ahead of it in
the queue. Only the single-map design removes the class.

**A note on what the mirror does get right.** IMPL-B's claim that the duplication has
"one failure mode" is accurate — I looked for a leak (an entry stuck LIVE or PENDING with no
connection behind it) on every path in §2.3 and §7.1 and did not find one. The mirror is not
wrong about state; it is wrong about *timing*, and only in the direction "admits a connect
the core will refuse", which is the benign direction. That is worth saying plainly, because
it means the fix is small and the release-build behaviour is already correct.

---

## 8. F1 [D] — a driver panic is silent, unrecoverable, and hangs every handle

Pulled out last because it is the amplifier for everything above.

`Driver::run` (`driver.rs:170-234`) calls `self.shutdown()` only on the normal `break` path
(`driver.rs:233`). `shutdown()` is what sets `ShellState::driver_stopped = true`
(`driver.rs:874`), latches `EndpointDropped` over every connection, and drops the parked
`accept()` senders. On an unwind — from either `panic!` (`driver.rs:324`, `370`), any of the
six `debug_assert!` that can fire in this file (`driver.rs:356`, `417`, `615`, `627`, `731`,
`808`) or the one in `shared.rs:435`, a `RefCell` conflict, or any future bug — none of that
happens:

* `driver_stopped` stays `false`, so `Endpoint::connect` keeps handing out `Connecting`s
  (`endpoint.rs:114-120`) that will never resolve, and `round_trip`'s early-out never fires;
* `Connecting::poll` parks its waker (`endpoint.rs:208-210`) with nobody left to wake it;
* `Connection::poll_closed` parks (`connection.rs:194-196`) with nobody left to wake it;
* `is_established()` and `remote_address()` keep returning the last values the dead driver
  wrote, so nothing an application can poll reveals the state.

Staged verbs and `Endpoint::accept()` *do* resolve, because their `oneshot` senders are
dropped with the `Driver` — which is why the failure looks like "some things work" rather
than "the endpoint is dead".

And it is invisible: `tokio::task::spawn_local` stores the panic in a `JoinHandle` the shell
drops, so nothing propagates. Every probe in this review printed a driver panic **and**
`test … ok`.

Cost of being wrong here is the highest in the review not because the trigger is likely —
F2 is the only trigger I demonstrated — but because it converts *every* driver-side
assertion, present and future, into a silent permanent hang, and slices 4-7 will add many
more. The shape of the fix is a `Drop` on the `Driver` (or a `catch_unwind`/guard around
`run`'s body) that runs `shutdown()`'s three lines, so an unwind degrades to
`ConnectionLost::EndpointDropped` and `ConnectError::Local` rather than to silence.

---

## Appendix A — probes

All four are in
`/private/tmp/claude-501/-Users-nicolasdiprima-work-primetype-slither/721dbfa4-ee18-4ecb-8459-b449d16c1367/scratchpad/probe`,
a standalone crate with `slither = { path = …, features = ["test-util"] }`. No repository
file was modified.

| File | Demonstrates |
|---|---|
| `tests/cb1.rs` | F2 — `AcceptChain` ahead of `Connect` for one static; debug panic vs. correct release outcome |
| `tests/cb1b.rs` | F1 — blast radius: an unrelated healthy connection frozen, `closed()` never resolving |
| `tests/deadline.rs` | F3 — a `YieldWire` (one `Pending` per send) losing a CLOSE in both profiles |
| `tests/waiting_leak.rs` | F4 — counting allocator, 20 000 cancelled accepts, plus a zero-growth control |

## Appendix B — mutations described but not applied

Per the charter, these are described rather than applied:

1. **F5's mutation.** Delete the `self.shell.state.borrow_mut().endpoint.handle_connection_event(…)`
   call in `serve_connection`'s `ConnOutput::ToEndpoint` arm (`driver.rs:361-365`). Expected:
   `a_closed_connection_frees_its_static_when_the_linger_expires` still passes, and
   `cargo test --all-features` stays green apart from any S29 test that observes the core.
2. **F8's check.** Swap the two statements in `command_cancel` (`driver.rs:772-779`) so the
   `Retired` is delivered before `self.conns.remove(&id)`. Expected: no test changes — which
   is the point; nothing pins the current order either way.
3. **F3's minimal repair, to confirm the diagnosis.** Have `deadline()` push a non-`Timeout`
   output back (or read the deadline without consuming). Expected: `probe/tests/deadline.rs`
   reports `peer C saw: Ok(PeerClosed { code: 2, … })`.

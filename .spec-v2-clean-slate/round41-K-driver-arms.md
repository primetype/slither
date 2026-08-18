# K — driver `debug_assert!(false)` arms: reachability (ruling 262 commission)

Base commit: `ae718236b4da215ba85d4366b482c09715d0e811` (verified clean; see §0)
Agent: K (measurement, EVIDENCE-ONLY — nothing here lands without a maintainer decision)
Worktree: `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a38ce6e031ab1da1c`

## 0. Base verification

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a38ce6e031ab1da1c
$ git rev-parse HEAD
ae718236b4da215ba85d4366b482c09715d0e811
$ git status --short
(empty)
$ git branch --show-current
worktree-agent-a38ce6e031ab1da1c
```

Base matches the brief's expected `ae71823`. No reset needed. This is an
isolated worktree, not the main repo (rule 14's first act).

## 1. The arms, verbatim

`grep -n debug_assert src/shell/driver.rs`:

```
530:                    debug_assert!(
767:                    debug_assert!(false, "ConnEvent::Established without an installed session");
977:    /// # It is an `assert!`, not a `debug_assert!` (**[RATIFIED 2026/08/17
980:    /// It shipped as a `debug_assert!`, which left the class **silent in
1040:                debug_assert!(false, "the endpoint core queued an output outside a drain");
1052:                        debug_assert!(false, "a connection core queued an output outside a drain");
1250:            debug_assert!(false, "§16.4: `accept()` returns an established connection");
1301:    /// `debug_assert!`s it, so the `is_none()` guard here is defence for the
```

The two arms commissioned by ruling 262 are at **1040** (endpoint core) and
**1052** (connection core), both inside `fn deadline(&self)`:

```rust
    fn deadline(&self) -> Option<std::time::Instant> {
        let endpoint = match self.shell.state.borrow_mut().endpoint.poll_output() {
            EndpointOutput::Timeout(deadline) => deadline,
            _ => {
                debug_assert!(false, "the endpoint core queued an output outside a drain");
                None
            }
        };

        self.conns
            .values()
            .filter_map(|record| {
                let mut cell = record.cell.borrow_mut();
                match cell.core.as_mut()?.poll_output() {
                    ConnOutput::Timeout(deadline) => deadline,
                    _ => {
                        debug_assert!(false, "a connection core queued an output outside a drain");
                        None
                    }
                }
            })
            .chain(endpoint)
            .min()
            .inspect(|announced| { /* ruling 255 streak assert, sibling */ })
    }
```

Note the shape difference from the sibling: the sibling (`assert!(streak < 3,
{PAST_DEADLINE})`, line 1082) is on the **value** path and is a release
`assert!` by ruling 249(ii)+255. These two are on the **shape** path and are
`debug_assert!(false)` + `None` — i.e. **in release they swallow the popped
output and continue**.

## 2. MECHANISM — the poll loop and its caller

### 2.1 The deadline-collection position
### 2.2 The doc comment's claim about who establishes emptiness

`sed -n '1025,1040p' src/shell/driver.rs` — the final doc paragraph of
`deadline`, quoted verbatim (this is the clause the brief asks for):

```
    /// # It must be called with no yield since the drain
    ///
    /// `poll_output()` **pops**, so the `_` arms below do not merely
    /// mis-report a deadline — they *destroy* whatever the core queued,
    /// which for a connection core is a datagram. This function has no way
    /// to establish that the queues are empty; only its **caller's
    /// position** does, and [`run`](Self::run) step 2 is that position and
    /// says why. Moving this call after `transmit().await` is what the
    /// regression test
    /// `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer`
    /// (`tests/spec_shell.rs`) exists to catch.
```

**So the invariant has a named owner already: the caller's position.** The
function is explicitly documented as unable to establish its own
precondition. That is the pivot for §5's verdict — the question is not
"is the assertion true" but "is the *caller* the only thing that can make it
true, and does the caller do so unconditionally".

### 2.1 The deadline-collection position — `run`'s loop

`sed -n '213,300p' src/shell/driver.rs` (abridged to the structure; the step-2
comment quoted in full because it is the argument):

```rust
    pub(crate) async fn run(mut self) {
        let mut buf = [0u8; constants::MAX_DATAGRAM];
        loop {
            // 1. Everything the cores want done. This is where §16.4's
            //    drain contract is discharged.
            let outgoing = self.serve();

            // 2. The next deadline, read **here** — after the drain and
            //    before the first yield of the iteration.
            //    […] `poll_output` **pops**. So reading a deadline is a pure
            //    read only while the queue is provably empty, and the only
            //    thing that establishes that is a drain with no yield after
            //    it. `transmit()` below is a yield […] and §16.3 (ruling 53)
            //    puts `close()` on the *handle* side of the seam, so a
            //    `close()` landing during that yield queues a `Transmit` on a
            //    core the driver has already drained. Reading the deadline
            //    after the yield popped that datagram and discarded it: a
            //    silently lost CLOSE and 25 s of `DEAD_TIMEOUT` for the peer.
            //
            //    Reading it here cannot go stale in a way that matters:
            //    **every** handle-side core mutation ends by sending a
            //    command (`Connection::close_now` → `Command::Dirty`,
            //    `Connecting::drop` → `Cancel`, `Endpoint::connect` →
            //    `Connect`), and the command arm is `biased` first […]
            let deadline = self.deadline();

            self.transmit(outgoing).await;          // <- first yield of the iteration

            let handles = self.shell.state.borrow().handles;   // 3. stop check
            if handles == 0 { break; }

            let event = { tokio::select! { biased;
                    command = commands.recv()          => Event::Command(command),
                    received = wire.recv_from(&mut buf)=> Event::Received(received),
                    () = sleep_until(deadline), if deadline.is_some() => Event::Timeout,
            } };                                     // 4. park

            if !matches!(event, Event::Timeout) { self.last_timeout = None; self.overdue_streak.set(0); }

            match event {
                Event::Command(Some(command))  => self.handle_command(command),
                Event::Command(None)           => break,
                Event::Received(Ok((len, src))) => { … self.handle_datagram(src, &buf[..len]); }
                Event::Received(Err(error))    => { tracing::warn!(…); }
                Event::Timeout                 => self.handle_timeout(),
            }
        }
    }
```

**Structural facts that fall straight out of this listing:**

1. `deadline()` is called from **exactly one place**, and that place is the
   statement immediately following `self.serve()`. There is no `.await`, no
   `borrow()` release hazard, and no user code between them — they are two
   consecutive synchronous statements in the same function body.
2. Every event handler (`handle_command`, `handle_datagram`, `handle_timeout`)
   runs at the **bottom** of the loop, i.e. the mutation happens and then the
   loop wraps to step 1 (`serve()`) before step 2 (`deadline()`) can see it.
   No handler is positioned between the drain and the collection.
3. Therefore the *only* candidate producers of a non-empty queue at the
   collection point are: **(a)** `serve()` itself failing to drain something,
   and **(b)** a mutation that happens inside `deadline()`'s own execution
   (reentrancy). Everything else is separated by a full `serve()`.
### 2.3 Every mutating call between last drain and this poll

#### 2.3.1 `serve()` — where the drain contract is discharged (`:342`)

```rust
    fn serve(&mut self) -> Vec<Outgoing> {
        let mut out = Vec::new();
        let mut settled = false;

        for _ in 0..DRAIN_BOUND {
            self.serve_endpoint(&mut out);
            let dirty: Vec<ConnectionId> = self.conns.iter()
                .filter(|(_, record)| record.cell.borrow().dirty)
                .map(|(id, _)| *id).collect();
            if dirty.is_empty() { settled = true; break; }
            for id in dirty { self.serve_connection(id, &mut out); }
        }
        assert!(settled, "the shell's drain did not settle in {DRAIN_BOUND} passes (§16.4)");

        self.release_dead();
        self.prune_ready();
        self.prune_waiting();
        self.dispatch_intros();
        out
    }
```

**The termination state is exactly the arms' precondition.** On the pass that
breaks, `serve_endpoint` ran at the top (returning only on
`EndpointOutput::Timeout`, i.e. endpoint queue empty) and the *only* thing
between that and the `break` is a read-only `dirty` scan. So at the `break`:

* endpoint queue is empty — established by `serve_endpoint`'s `Timeout` return;
* every connection has `dirty == false`.

`serve_endpoint` (`:441`) and `serve_connection` (`:507`) both end their
`DRAIN_BOUND` loop in a **`panic!`**, not a fall-through, so neither can
return with a non-empty queue:

```
537:        panic!("core::Endpoint::poll_output did not reach Timeout in {DRAIN_BOUND} outputs (§16.4)");
805:        panic!("core::Connection::poll_output did not reach Timeout in {DRAIN_BOUND} outputs (§16.4)");
```

and `serve`'s own bound is an `assert!` (release-live), which the `serve` doc
records as a deliberate fix — *"this one used to fall through silently, which
is the one asymmetry of the three […] `deadline` **pops**, so an undrained
`Transmit` sitting there is destroyed rather than sent."* **That is the arms'
failure mode having already been reached once by this route, and closed.**

So the whole of the reachability question reduces to two claims:

* **(E)** nothing mutates the endpoint core between `serve_endpoint`'s last
  `Timeout` and `deadline()`; and
* **(C)** `dirty == false` implies "this connection core's queue is empty" —
  i.e. **`dirty` is a sound over-approximation of "may have queued output"**.

#### 2.3.2 The post-drain tail of `serve()` — audited against (E)

The four calls that run *after* the `settled` break and *before* `deadline()`:

| call | site | touches a core? |
|---|---|---|
| `release_dead()` | `:821` | **drops** cores (`record.cell.borrow_mut().core = None`); calls no core verb. Also `resolve_slot(.., Failed(Local))`. |
| `prune_ready()` | `:398` | read-only: `state.endpoint.intro_source(id)` under a shared `borrow()`. |
| `prune_waiting()` | `:434` | `self.waiting.retain(|r| !r.is_closed())` — driver-local. |
| `dispatch_intros()` | `:851` | `state.endpoint.intro_sender_index(id)` under a shared `borrow()`; `reply.send(intro)`. |

Three of the four cannot mutate a core at all — `prune_ready` and
`dispatch_intros` take `state.borrow()` (shared), so a mutating verb would not
compile. `release_dead` sets `core = None`, which *removes* a queue rather
than filling one, and is benign for the arms twice over: `deadline`'s
connection arm is guarded by `cell.core.as_mut()?`, so a released core is
skipped by the `filter_map` before any `poll_output` happens.

The one residue is `dispatch_intros`'s `drop(reply.send(intro))`: on `Err` the
returned `Intro`'s `Drop` is §6.2's silent reject, which is an endpoint
mutation. The code calls that arm unreachable and gives the reason — the inner
`while` pops closed senders immediately before, *"on a single-threaded runtime
this check and the `send` below are atomic with respect to the application"*.
`§2.4` below re-derives that atomicity claim independently, and `§4` probes it.
(Whether `Intro::Drop` calls the core directly or posts a `Command` is settled
in §2.5 — if it posts a command it is harmless here regardless.)

#### 2.3.3 The event handlers are on the far side of a full `serve()`

`handle_command`, `handle_datagram` and `handle_timeout` all run at the
**bottom** of `run`'s loop body. Between any of them and the next `deadline()`
lies a complete `serve()`. They cannot be the producers; they can only be the
producers *of the mutation `serve()` then fails to drain*, which is claim (C).
#### 2.5.2 Every connection-core call site in the shell, with its dirty

```
$ grep -rnE "core\.(write|read|finish|reset|abandon_recv|send_message|send_datagram|set_persistent_keepalive|close|flush|on_ack_range|on_lost_range)\(" src/shell/
stream.rs:308     core.write(now(), self.r, buf)              -> dirty at 314 / 318, **not** on Err
stream.rs:380     core.finish(now(), self.r)                  -> dirty at 382
stream.rs:512     core.reset(now(), self.r, error_code)       -> dirty at 513
stream.rs:574     core.reset(now(), constants::NO_ERROR)      -> dirty at 575
stream.rs:792     core.read(now(), self.r, buf)               -> dirty at 811/816/821/826, **not** on two arms
stream.rs:868     core.abandon_recv(now(), self.r)            -> dirty at 869
connection.rs:370 core.set_persistent_keepalive(now(), iv)    -> dirty at 372
connection.rs:803 core.send_datagram(now(), data)             -> dirty at 805
connection.rs:870 core.send_message(now(), msg)               -> dirty at 880
shared.rs:538     core.close(now(), code, reason)             -> dirty at 539  (close_now)
```
plus the driver's own three (`handle_endpoint_event` :457, `handle_datagram`
:927, `handle_timeout` :951), each paired with `cell.dirty = true`, and
`deliver_to_core` (:500) covering `replaced`/`mark_contested`.

`Intro::Drop` is **not** a direct core call — it posts a command:

```rust
// src/shell/staged.rs:201
impl<I: Identity> Drop for Intro<I> {
    fn drop(&mut self) {
        if !self.consumed { self.shell.send(Command::Reject(self.id)); }
    }
}
```

so §2.3.2's residue in `dispatch_intros` is discharged: even on the
"unreachable" `Err`, the drop enqueues a **command**, which the driver handles
at the bottom of a later loop iteration, with a full `serve()` before the next
`deadline()`.

#### 2.5.3 **The two conditional-dirty arms — the sharpest lead**

Two call sites decide `dirty` per match arm, and each has arms that set it
**false after the core call already happened**:

```rust
// src/shell/stream.rs:308  (poll_write)
            match core.write(now(), self.r, buf) {
                Ok(0) => { … park …; cell.dirty = true; (Poll::Pending, true) }
                Ok(n) => { cell.dirty = true; (Poll::Ready(Ok(n)), true) }
                Err(e) => (Poll::Ready(Err(e)), false)                    // <- no dirty
            }
// src/shell/stream.rs:792  (poll_read)
            match core.read(now(), self.r, buf) {
                Ok(Some(0)) => match cell.closed.clone() {
                    Some(lost) => (Poll::Ready(Err(ReadError::ConnectionLost(lost))), false),  // <- no dirty
                    None => { … park …; cell.dirty = true; (Poll::Pending, true) }
                },
                Ok(Some(n)) => { cell.dirty = true; … }
                Ok(None)    => { cell.dirty = true; … }
                Err(ReadError::Reset(code)) => { cell.dirty = true; … }
                Err(e) => (Poll::Ready(Err(e)), false)                    // <- no dirty
            }
```

Now the cores. **`write` and `read` are not symmetric about `pump`, and that
asymmetry is the whole question:**

```rust
// src/core/connection/mod.rs:837
    pub(crate) fn write(&mut self, now, r, data) -> Result<usize, WriteError> {
        self.lost()?;                                        // early return — no pump
        let n = self.streams.write(r, data, &mut self.flow)?; // early return — no pump
        self.pump(now);                                      // the ONLY emitter
        Ok(n)
    }
// src/core/connection/mod.rs:909
    pub(crate) fn read(&mut self, now, r, buf) -> Result<Option<usize>, ReadError> {
        let out = self.streams.read(r, buf, &mut self.flow);
        if let Some(lost) = self.lost.clone() {
            return match out {                                // dead path: **no pump** (documented)
                Ok(Some(0)) => Err(ReadError::ConnectionLost(lost)),
                served => served,
            };
        }
        self.pump(now);                                      // <-- RUNS EVEN WHEN `out` IS `Err`
        out
    }
```

**`poll_write`'s `Err(e) => false` is sound by construction**: every `Err` from
`core.write` returns *before* `pump`, and `streams.write` has no access to
`self.outputs` (it is handed `&mut self.flow` only). No emission is possible on
that arm. ✓

**`poll_read`'s two `false` arms are not sound by the same argument**, because
`core.read` runs `pump(now)` on the **live-core** path regardless of whether
`out` is `Ok` or `Err`. Two candidate reaching states follow, and §2.6/§4 chase
both:

* **(R1)** live core (`core.lost == None`) + shell latch set (`cell.closed ==
  Some`) + `streams.read` → `Ok(Some(0))`. `pump(now)` runs; the shell returns
  `Err(ConnectionLost)` with `dirty = false`. The code *names this state as
  real*: *"The core's own `lost` is not the same fact as this cell's `closed`:
  `Driver::stop` latches every cell […] over cores that are still perfectly
  live and still answer `Ok(Some(0))`."*
* **(R2)** live core + `streams.read` → `Err(e)` for some `e` other than
  `Reset`. `pump(now)` runs; the shell takes the fallthrough `Err(e) => false`.

#### 2.5.4 R2 is closed **by the type**

`streams.read` (`src/core/connection/streams.rs:547`) returns exactly three
shapes, and only one of them is an `Err`:

```rust
        match outcome {
            ReadOutcome::Data(n)    => Ok(Some(n)),
            ReadOutcome::End        => { self.retire_recv(r, flow); Ok(None) }
            ReadOutcome::Reset(code)=> { self.retire_recv(r, flow); Err(ReadError::Reset(code)) }
        }
```

and `ReadError` has two variants (`src/error.rs:289`): `Reset(u64)` and
`ConnectionLost`. `streams.read` **never constructs `ConnectionLost`** — the
only producer of that variant inside `Connection::read` is the dead-path
`Ok(Some(0)) => Err(ReadError::ConnectionLost(lost))`, which is on the branch
that **does not pump**.

So the `Err(e) => (…, false)` fallthrough in `poll_read` receives only
`ConnectionLost`, and only from the no-pump branch. `Err(Reset)` — the one
error a live, pumping `read` can return — is matched *above* it and **does**
set `dirty`. **R2 is unreachable, and the arm ordering in `poll_read` is what
makes it so.** ✓

#### 2.5.5 R1 is closed **by `emit_closed`**

R1 needed `cell.closed == Some` while the core is present *and* `core.lost ==
None`. The core sets both facts in one statement:

```rust
// src/core/connection/mod.rs:3364
    fn emit_closed(&mut self, lost: ConnectionLost) {
        debug_assert!(!self.closed_emitted, "§16.4: a connection dies once, …");
        if self.closed_emitted { return; }
        self.closed_emitted = true;
        self.lost = Some(lost.clone());                                   // <-- here
        self.outputs.push_back(ConnOutput::Event(ConnEvent::Closed(lost))); // <-- and here
    }
```

`self.lost = Some(..)` is the **only** assignment to `lost` in the whole core:

```
$ grep -n "self\.lost\s*=" src/core/connection/*.rs | grep -v tests
src/core/connection/mod.rs:3373:        self.lost = Some(lost.clone());
```

The shell's `cell.closed` is written in exactly one place —
`Driver::latch` (`driver.rs:1324`) — with two callers:

* `publish(ConnEvent::Closed(lost))`, which can only run because the core
  pushed that event from `emit_closed`, i.e. with `core.lost` already `Some`;
* `Driver::stop`, which latches `EndpointDropped` over possibly-live cores —
  **but nulls the core on the very next line**:

```rust
        for record in self.conns.values_mut() {
            Self::latch(&record.cell, ConnectionLost::EndpointDropped);
            record.cell.borrow_mut().core = None;
            …
        }
```

and `stop` runs from `Drop for Driver`, i.e. **after `run` has returned**, so
no `deadline()` follows it in any case.

**Therefore `cell.closed == Some` ∧ `cell.core == Some` ⟹ `core.lost ==
Some` ⟹ `Connection::read` takes the no-pump branch.** R1 is unreachable. ✓

*Side finding (not a defect, a stale justification — rule 11's "the artefact a
claim is about is the artefact that must be opened"):* `poll_read`'s comment
justifies its latch re-check with *"`Driver::stop` latches every cell … over
cores that are still perfectly live and still answer `Ok(Some(0))`"*. `stop`
releases the core on the next line, so by the time any handle can observe that
latch there is **no core to answer `Ok(Some(0))`** — `poll_read` returns from
its `let Some(core) = … else` arm instead. The re-check is still *correct* and
still worth keeping (it is one line and it is the invariant's local statement),
but the mechanism its comment names does not produce the state it describes.
Worth a line in ruling 262 or a follow-up; it does **not** change any verdict
here.

### 2.4 Reentrancy: can a user future run between step 1 and step 2?

`serve()` and `deadline()` are consecutive **synchronous** statements — no
`.await`, no `Pending` return, nothing between them in the source. So a user
future can only run if something *inside* `serve()` calls into consumer code
synchronously. Three vectors exist, and the codebase already names the first:

**(V1) A consumer-supplied `Waker` that polls inline.** `Driver::latch`'s own
doc:

```
    /// `Waker::wake` runs the **consumer's** executor, and an executor that
    /// polls a ready task inline rather than queueing it re-enters
    /// `Connection::poll_closed`, which takes this same `borrow_mut`. That
    /// is `already mutably borrowed` inside the driver task — from a
    /// consumer doing nothing wrong, on a `Waker` this crate does not
    /// supply. So the borrow ends first […]
```

Note the direction of that defence: **releasing the borrow before waking is
what makes the reentrant call *succeed* rather than panic.** It converts an
`already mutably borrowed` abort into a silently-permitted reentrant core
mutation. That is the right trade, and it is also precisely what makes V1 a
live vector for these arms rather than a panic.

**(V2) A consumer `Drop` impl.** `release_dead` drops the whole `ConnRecord`,
which owns `remote_static: PublicKeyOf<I>` — a **consumer-supplied** associated
type whose `Drop` is consumer code, running inside `serve()`.

**(V3) `oneshot::Sender::send` / `resolve_slot`,** which wake consumer tasks
from `dispatch_intros` and `release_dead`.

**Where in `serve()` the vector fires is what decides reachability**, and this
is the crux:

* Fired from inside the **drain loop** (`serve_endpoint` / `serve_connection`
  → `publish` → waker maps): harmless. `serve_connection` re-reads
  `borrow.dirty` at the top of every iteration and the outer `serve()` loop
  re-scans `dirty` after every pass, so a reentrant `poll_write` that queues a
  `Transmit` and sets `dirty` is picked up by the very loop it interrupted.
* Fired from the **post-drain tail** (`release_dead` → `resolve_slot`;
  `dispatch_intros` → `reply.send`; a `PublicKeyOf<I>` `Drop`): **the `dirty`
  scan has already finished.** A reentrant consumer call that mutates *any
  other* live connection's core queues an output with **no further drain
  before `deadline()`**. `mark_dirty` posts a `Command::Dirty`, which wakes the
  driver for the *next* iteration — but step 2 of the *current* iteration has
  already popped and discarded one output.

That is the shape of the only construction I can build, and §4 tries to build
it.
### 2.5 §16.4 contract audit — every mutating verb call site in the shell

#### 2.5.0 What "non-empty" means, precisely

Both cores' `poll_output` are two-liners, so the arms fire **iff `outputs` is
non-empty**:

```rust
// src/core/connection/mod.rs:787
    pub(crate) fn poll_output(&mut self) -> ConnOutput {
        self.outputs.pop_front()
            .unwrap_or_else(|| ConnOutput::Timeout(self.timers.next()))
    }
// src/core/endpoint/mod.rs:275
    pub(crate) fn poll_output(&mut self) -> EndpointOutput<I::Suite> {
        match self.outputs.pop_front() {
            Some(output) => output,
            None => EndpointOutput::Timeout(self.deadline()),
        }
    }
```

There is no lazy generation and no side condition. `Timeout` is *computed*
from `timers.next()` / `deadline()` and is never enqueued, so a queue is
"empty" exactly when nothing was pushed since the last drain. The only
producers:

```
$ grep -rn "outputs\.push_back" src/core/ | grep -v tests
src/core/endpoint/mod.rs:308:        self.outputs.push_back(output);          // fn emit()
src/core/connection/mod.rs:1834:            self.outputs.push_back(ConnOutput::Event(event));
src/core/connection/mod.rs:1947:        self.outputs.push_back(ConnOutput::Transmit(Transmit {
src/core/connection/mod.rs:2572:        …Transmit…   2777:  …Transmit…   2855: …Transmit…
src/core/connection/mod.rs:3057:        …Transmit…   3286:  …Transmit…
```

#### 2.5.-1 The verb inventory

`pub`/`pub(crate)` `&mut self` verbs, extracted mechanically (script in
`scratchpad/round41/verbs.py`):

```
$ python3 …/verbs.py src/core/connection/mod.rs src/core/endpoint/mod.rs
==================== src/core/connection/mod.rs
479 set_persistent_keepalive · 499 handle_endpoint_event · 534 handle_datagram
655 handle_timeout · 747 close · 787 poll_output · 796 fail_next_seal(cfg test)
816 open · 826 accept · 837 write · 855 finish · 863 reset · 909 read
939 abandon_recv · 1005 send_message · 1086 recv_message · 1110 send_datagram
1138 recv_datagram · 1148 flush · 1167 on_ack_range · 1185 on_lost_range
3110 replaced · 3125 mark_contested
==================== src/core/endpoint/mod.rs
275 poll_output · 400 mint_pending · 467 start_attempt · 595 handle_datagram
805 handle_timeout · 897 handle_connection_event
$ python3 …/verbs.py src/core/endpoint/staged.rs
223 read_identity · 407 authenticate · 585 accept · 783 reject
```

#### 2.5.0b Every endpoint-core call site in the shell

```
$ grep -rn "endpoint\.\w*(" src/shell/          (doc-comment and test hits elided)
driver.rs:404 :  state.endpoint.intro_source(ready.id)                 [&self — prune_ready]
driver.rs:947 :  …endpoint.handle_timeout(now)                          [handle_timeout, bottom of loop]
driver.rs:1037:  …endpoint.poll_output()                                [THE ARM]
driver.rs:1104:  …endpoint.read_identity(now(), id)                     [Command, bottom of loop]
driver.rs:1118:  …endpoint.authenticate(now(), id)                      [Command, bottom of loop]
driver.rs:1128:  …endpoint.reject(now(), id)                            [Command, bottom of loop]
driver.rs:1234:  …endpoint.accept(now(), id)                            [Command::AcceptChain, bottom]
driver.rs:~869:  …endpoint.intro_sender_index(ready.id)                 [&self — dispatch_intros]
endpoint.rs:373: state.endpoint.handle_connection_event(now(), id, Retired{our_index:0})
                                                                        [**HANDLE SIDE** — Connecting::drop]
shared.rs:873 :  self.endpoint.poll_output()                            [ShellState::drain_endpoint]
```

Six of the mutating sites are inside `handle_command`/`handle_timeout`, i.e.
at the **bottom** of `run`'s loop, and a whole `serve()` separates each from
the next `deadline()`. That leaves exactly the handle-side pair.

#### 2.5.0c **The third sibling: `ShellState::drain_endpoint` (`shared.rs:872`)**

Ruling 262 commissions two arms; the same shape exists at a **third** site,
and it is already documented as a pin rather than as a guard:

```rust
    pub(crate) fn drain_endpoint(&mut self) {
        for _ in 0..HANDLE_DRAIN_BOUND {
            match self.endpoint.poll_output() {
                crate::core::EndpointOutput::Timeout(_) => return,
                other => {
                    debug_assert!(false,
                        "a handle-side endpoint verb queued an output (§16.4, ruling 90): {}", …);
                }
            }
        }
        panic!("core::Endpoint::poll_output did not reach Timeout in {HANDLE_DRAIN_BOUND} outputs (§16.4)");
    }
```

Its doc names the invariant, its owner, and the identical destruction risk:

```
    /// "Every mutating call is followed by draining `poll_output()` to the
    /// terminal `Timeout`" (§16.4), and ruling 90 puts two mutating endpoint
    /// calls on a handle:
    ///   * `Endpoint::connect` → `core::Endpoint::mint_pending`, and
    ///   * `Connecting::drop` → `core::Endpoint::handle_connection_event(.., Retired)`
    /// **Both are 0 DH and both emit nothing**, so this loop terminates on
    /// its first pop […] were one of them ever to emit, popping here would
    /// *destroy* the output — the silent-CLOSE-loss shape of finding 4 in
    /// `.slices/03-skeleton/FIXES-3b.md`. The assert turns that into a named
    /// failure in every debug build, which is every `cargo test`.
```

**This closes the handle-side endpoint hole for `deadline`'s arm, and it does
so structurally**: whatever a handle-side endpoint verb queues is popped by
`drain_endpoint` *before the handle returns*, so by the time the driver next
runs `deadline()` the endpoint queue is empty either way — in debug because
the assert aborts, in release because the loop keeps popping to `Timeout`.

**Note the difference in shape, which matters for §6's recommendation:**
`drain_endpoint` pops **in a loop**, so in release it leaves the queue *empty*
and merely loses the outputs. `deadline`'s arms pop **once** and return `None`,
so in release they lose one output per connection per driver turn and leave the
rest queued. `deadline`'s arms are the strictly worse release behaviour of the
two, and they are the ones that are only `debug_assert!`.

#### 2.5.1 The three handle-side `&mut` core calls that set **no** dirty flag

This is where I expected the hole, and it is the sharpest part of the audit.
Grepping `core.as_mut()` outside the driver gives 3 sites that call a `&mut
self` core verb and then do **not** set `cell.dirty` and do **not**
`mark_dirty`:

| site | verb | dirties? |
|---|---|---|
| `src/shell/connection.rs:965` | `CoreConnection::recv_datagram` | **no** — ruling 151, deliberate |
| `src/shell/connection.rs:~1064` (`poll_open_with`) | `core.open(dir)` | **no** |
| `src/shell/connection.rs:1137` (`poll_accept_with`) | `core.accept(dir)` | **no** |

Each was checked in the core, and **all three are pure bookkeeping — none
touches `outputs`**:

```rust
// src/core/connection/mod.rs:1138
    pub(crate) fn recv_datagram(&mut self) -> Option<Vec<u8>> { self.datagrams.pop_recv() }
// src/core/connection/mod.rs:816 -> streams.rs:384
    pub(crate) fn open(&mut self, dir: Dir, flow: &Flow) -> Result<StreamRef, StreamsExhausted> {
        let index = self.local[dir.slot()].ever_opened;
        if index >= flow.remote_max_streams(dir) { return Err(StreamsExhausted); }
        let r = self.alloc();
        self.local[dir.slot()].ever_opened += 1;
        self.local[dir.slot()].open.insert(index, r);
        self.entries.insert(r, Stream::new(dir, index, true));
        Ok(r)
    }
// src/core/connection/mod.rs:826 -> streams.rs:412
    pub(crate) fn accept(&mut self, dir: Dir) -> Option<StreamRef> {
        let r = self.unclaimed[dir.slot()].pop_front()?;
        self.overflow_candidates.remove(&r);
        if let Some(stream) = self.entries.get_mut(&r) { stream.unclaimed = false; }
        Some(r)
    }
```

No `outputs.push_back`, no `pump`, no `seal` on any of the three paths. **The
absence of `dirty` on these three is sound, not an omission** — and the
contrast is instructive: `poll_recv_message` right next door calls
`core.recv_message(now())`, which *can* emit (§9.8's overflow scan and a
credit true-up), and it sets `dirty` **on the strength of the call rather than
the answer**, then `mark_dirty`s outside the borrow. Every verb that can push
is paired with a dirty; the three that cannot push are not.

## 3. MEASURE — instrumented run

### 3.1 Instrumentation patch

Both arms' `debug_assert!(false, …)` replaced by a **file-backed** counter, so
a hit survives libtest's stdout/stderr capture *and* survives being in a
passing test, in both profiles (a `debug_assert` would have been compiled out
of the release run, which is exactly the profile the question is about):

```rust
fn kprobe(which: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::var("SLITHER_KPROBE") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "KPROBE-HIT {which}");
        }
    }
}
```
```rust
        let endpoint = match self.shell.state.borrow_mut().endpoint.poll_output() {
            EndpointOutput::Timeout(deadline) => deadline,
            _ => { kprobe("endpoint"); None }
        };
        …
                    _ => { kprobe("connection"); None }
```

### 3.1b **Control — the probe is shown to be able to fire** (working rule 9)

A counter that reads zero proves nothing until the plumbing is demonstrated.
A temporary control probe was added to the **`Timeout`** arm (the one that
always runs) and one test binary was run:

```
$ SLITHER_KPROBE=…/kprobe-control.log cargo test --all-features --test spec_shell
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
$ wc -l < …/kprobe-control.log
     132
```

**132 hits from 12 tests in one binary.** The env var reaches the test
processes, the file is created and appended, and `deadline()` is on the hot
path exactly as expected. The control was then removed and both arms left
instrumented alone.

### 3.2 `cargo test --all-features` (debug)

```
$ rm -f …/kprobe-debug.log
$ SLITHER_KPROBE=…/kprobe-debug.log cargo test --all-features
… (all binaries green; the largest is the unit suite)
test result: ok. 744 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.37s
test result: ok. 112 passed; … 24 passed; … 20 passed; … 17 passed; … 16 passed; …
test result: ok. 12 passed …  (× several binaries)   [zero `failed` anywhere]
Doc-tests slither: 12 passed; 0 failed
$ wc -l < …/kprobe-debug.log
0 (file never created)
```

### 3.3 `cargo test --release --all-features`

```
$ rm -f …/kprobe-release.log
$ SLITHER_KPROBE=…/kprobe-release.log cargo test --release --all-features
test result: ok. 744 passed; 0 failed; … 112 passed; … 24 passed; … 20 passed;
test result: ok. 17 passed; … 16 passed (×2); … 15 passed; … 12 passed (×4);
test result: ok. 11, 10, 7, 6 (×2), 5, 4 (×3), 1 passed — 0 failed anywhere
$ wc -l < …/kprobe-release.log
0 (file never created)
```

### 3.4 Hit counts

| profile | endpoint arm | connection arm | control (Timeout arm) |
|---|---|---|---|
| `--all-features` (debug) | **0** | **0** | 132 in one binary |
| `--release --all-features` | **0** | **0** | — |

**Zero, in both profiles, across the entire suite.** Combined with the control,
this is a real negative rather than a broken instrument. It is *not* by itself
a proof of unreachability — working rule 13 (*the fixture bounds the
coverage*) applies with full force here, and §4 says exactly which fault class
`FlakyWire` cannot express.

## 4. ADVERSARIAL — **a reaching state was constructed**

### 4.1 Headline

**The connection arm at `driver.rs:1052` is REACHABLE, and the reproducer
below reaches it.** The mechanism is exactly §2.4's post-drain-tail
reentrancy: a consumer-supplied `Waker` that polls inline, woken by
`dispatch_intros`'s `reply.send(intro)` — which runs **after** `serve()`'s
last `dirty` scan and **before** `deadline()`.

```
$ SLITHER_KPROBE=…/kprobe-repro.log cargo test --all-features \
      --test kprobe_reentrancy -- --nocapture
running 1 test
KREENTRANT poll_write -> Ready(Ok(15))
KREENTRANT fired=1
test reentrant_write_during_dispatch_intros ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

$ sort …/kprobe-repro.log | uniq -c
   1 KPROBE-HIT connection
```

One hit, on the connection arm, from a suite that produced **zero** hits over
1 000+ tests. The 15 bytes accepted by `poll_write` are a `Transmit` the very
next statement pops and discards.

### 4.2 The reproducer

`tests/kprobe_reentrancy.rs` (scratch — reverted before hand-off; kept here in
full because it is the evidence):

```rust
thread_local! {
    static REENTRANT: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
    static FIRED: Cell<usize> = const { Cell::new(0) };
}

struct Inline;
impl Wake for Inline {
    fn wake(self: Arc<Self>) { self.wake_by_ref(); }
    fn wake_by_ref(self: &Arc<Self>) {
        let taken = REENTRANT.with(|slot| slot.borrow_mut().take());
        if let Some(mut f) = taken { FIRED.with(|c| c.set(c.get() + 1)); f(); }
    }
}

#[tokio::test(start_paused = true)]
async fn reentrant_write_during_dispatch_intros() {
    local(async {
        let pair = Pair::seeded(0x4B00_0001);
        let (_ca, cb) = pair.establish().await;
        settle().await;

        // B has a live connection with a writable stream.
        let (sb, _rb) = cb.open_bi().await.expect("open_bi").split();
        settle().await;

        // A third endpoint C: B's §16.1 LIVE test refuses a second dial from
        // A's static, so the fresh introduction must come from a new static.
        let c_identity: TestIdentity = CountingIdentity::seeded([0x5C; 32]);
        let c_endpoint = slither::Endpoint::builder()
            .identity(c_identity).wire(pair.net.endpoint(addr_c()))
            .config(slither::Config::new()).rng_seed([0x5D; 32]).build();

        // Arm the reentrancy: the first `wake()` writes on B's live stream.
        let mut sb: TestSendStream = sb;
        REENTRANT.with(|slot| { *slot.borrow_mut() = Some(Box::new(move || {
            let waker = Waker::noop().clone();
            let mut cx = Context::from_waker(&waker);
            let r = Pin::new(&mut sb).poll_write(&mut cx, b"reentrant-write");
            eprintln!("KREENTRANT poll_write -> {r:?}");
        })); });

        // Park an `accept()` on B under the inline-polling waker.
        let mut acc = Box::pin(pair.b.endpoint.accept());
        let w = Waker::from(Arc::new(Inline));
        let mut cx = Context::from_waker(&w);
        assert!(acc.as_mut().poll(&mut cx).is_pending());
        settle().await;

        // C dials B → IntroReady → drain settles → dispatch_intros sends the
        // Intro in the post-drain tail → our waker fires inline → the write
        // queues a Transmit on cb's core → `deadline()` pops and drops it.
        let _dial = c_endpoint.connect(pair.b.addr(), pair.b.public_static).expect("connect");
        settle().await;
        settle().await;

        assert_eq!(FIRED.with(Cell::get), 1);
    }).await;
}
```

### 4.3 Why the ordinary suite never sees it (working rule 13, again)

Every existing test drives slither through `tokio`'s current-thread runtime
and `LocalSet`, whose `wake()` **queues** the task rather than polling it
inline. So the whole reentrancy class is absent from the fixture by
construction — the same shape as the seam review's two unreachable findings,
one layer up: `FlakyWire` cannot express "this driver panics" and the *tokio
runtime* cannot express "this consumer's waker polls inline". The 0/0 in §3 is
therefore a statement about the harness, not about the protocol.

**This is not an exotic consumer.** The crate's own code already treats
inline-polling wakers as a supported case and defends against them **three
times**, always by releasing the borrow before waking:

* `Driver::latch` — *"an executor that polls a ready task inline rather than
  queueing it re-enters `Connection::poll_closed`, which takes this same
  `borrow_mut`. […] So the borrow ends first"*;
* `resolve_slot` — *"`Waker::wake` is the consumer's executor. An executor
  that polls a ready task inline re-enters `Connecting::poll` […]"*;
* `poll_recv_message` — *"Outside the borrow (finding F10): driving re-enters
  the cell."*

And note the sting: **those defences are what make this reachable.** Waking
under the borrow would have aborted the reentrant call with `already mutably
borrowed`; releasing it first — the correct fix for that bug — lets the
reentrant core mutation succeed, and lands it in the one window `serve()` no
longer covers.

### 4.4 Which tail call is the trigger, and the others in the same position

Confirmed trigger: `dispatch_intros` → `drop(reply.send(intro))`. Two more
calls sit in the same post-`dirty`-scan window and wake consumer code:

* `release_dead` → `resolve_slot(&slot, PendingOutcome::Failed(ConnectError::Local))`
  — wakes a `Connecting`;
* `release_dead` → dropping the `ConnRecord`, which owns
  `remote_static: PublicKeyOf<I>`, a **consumer-supplied** type whose `Drop`
  is consumer code (this one needs no waker at all — a custom `Identity` is
  enough, which is S21's iOS-Secure-Enclave story's own shape).

`fail_pending`'s `resolve_slot` is **not** in this window — it is called from
inside `serve_endpoint`, i.e. within the drain loop, so the `dirty` scan of
that same pass still covers it. That asymmetry is worth stating explicitly in
whatever ruling 262 lands: *inside the drain loop is safe; the tail is not.*

### 4.5 What was destroyed, and the proof it is the shipped code

Variant logging on the arm, same reproducer:

```
$ cat …/kprobe-repro2.log
KPROBE-HIT connection Transmit(57 bytes to 10.0.0.1:4001)
```

**A 57-byte datagram — the packet carrying the 15 bytes `poll_write` had just
returned `Ok(15)` for — was popped and dropped on the floor.**

Then the instrumentation was reverted so `driver.rs` is byte-identical to the
base commit, and the reproducer re-run:

```
$ git diff --stat src/shell/driver.rs
(empty)
$ cargo test --all-features --test kprobe_reentrancy -- --nocapture
running 1 test
KREENTRANT poll_write -> Ready(Ok(15))

thread 'reentrant_write_during_dispatch_intros' (4944217) panicked at src/shell/driver.rs:1052:25:
a connection core queued an output outside a drain
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
KREENTRANT fired=1
test reentrant_write_during_dispatch_intros ... ok
```

**The shipped, unmodified assertion fires.** `git diff --stat` empty is the
load-bearing line: this is not an artefact of my probe.

Two things to notice in that output.

1. **The test still reports `ok`.** The panic is inside the driver task
   spawned by `spawn_local`, whose `JoinHandle` the shell drops — the exact
   "nothing surfaces; the harness prints `ok` over a frozen endpoint" failure
   the `Drop for Driver` doc describes. So even in **debug**, this arm does
   not fail a test; it kills the driver silently and degrades every connection
   on the endpoint to `EndpointDropped`. That is materially worse than the
   doc's framing of *"a named failure in every debug build"*.
2. In **release** the `debug_assert` vanishes and the arm is the silent
   `None`: the datagram is destroyed and the driver carries on. For stream
   data that costs a PTO (the core recorded the packet as sent, so loss
   recovery retransmits it). For an unreliable DATAGRAM (§10.7, no
   retransmission) or a CLOSE it is **permanent** — which is precisely the
   *"silently lost CLOSE and 25 s of `DEAD_TIMEOUT` for the peer"* that the
   `deadline` doc and `FIXES-3b.md` finding 4 both name.

## 4ter. The separating assertion, and the three-way measurement

The reproducer was strengthened with the assertion that actually separates a
fix from a paint-over (working rule 9 — *ask what the broken version would do*):

```rust
        // ── THE SEPARATING ASSERTION ───────────────────────────────────
        // The bytes must reach A **without the clock advancing**. `settle()`
        // only yields (`SETTLE_YIELDS` × `yield_now`); it never advances
        // virtual time, so no `Loss`/`Pto` timer can fire and no
        // retransmission can cover for a destroyed datagram.
        let ra = tokio::time::timeout(std::time::Duration::from_millis(1), ca.accept_bi())
            .await.expect("A must see the stream with no clock advance").expect("accept_bi");
        let mut ra = ra.split().1;
        let mut got = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut ra, &mut got).await.expect("read");
        assert_eq!(&got[..n], b"reentrant-write",
            "the reentrant write's datagram must not be destroyed");
```

**Why the no-clock-advance framing is the load-bearing half.** Under the base
in *release* the core has already recorded the packet as sent, so loss
recovery would retransmit it and a test that merely waited would pass over a
real data loss. Pinning delivery to virtual-time-zero is what makes the
assertion fail on the broken version. (One earlier draft of this test was
*wrong in the other direction*: it captured the `SendStream` in a one-shot
closure that was dropped after firing, and a dropped `SendStream` sends
RESET_STREAM (§9.6) — so it failed with `Reset(0)` for a reason having nothing
to do with the arm. The stream is parked in a `thread_local` for that reason.)

| build | driver assert | delivery at t=0 | result |
|---|---|---|---|
| **base `ae71823`, debug** | **panics** `a connection core queued an output outside a drain` (driver.rs:1052) | ✗ `Elapsed(())` | FAILED |
| **base `ae71823`, release** | *(compiled out — silent)* | ✗ `Elapsed(())` | FAILED |
| **candidate A, debug** | n/a (arm removed) | ✓ `b"reentrant-write"` | ok |

```
=== BASE, DEBUG ===
KREENTRANT poll_write -> Ready(Ok(15))
thread '…' panicked at src/shell/driver.rs:1052:25:
a connection core queued an output outside a drain
KREENTRANT fired=1
thread '…' panicked at tests/kprobe_reentrancy.rs:113:14:
A must see the stream with no clock advance: Elapsed(())
test reentrant_write_during_dispatch_intros ... FAILED

=== BASE, RELEASE ===
KREENTRANT poll_write -> Ready(Ok(15))
KREENTRANT fired=1
thread '…' panicked at tests/kprobe_reentrancy.rs:113:14:
A must see the stream with no clock advance: Elapsed(())
test reentrant_write_during_dispatch_intros ... FAILED
```

**The release row is the finding in one line: no panic, no log, no red — and
the application's `Ok(15)` bytes are gone.**

## 5. VERDICT

**REACHABLE.** Not "unprovable" and not "reachable in principle" — reached, by
a test, against the byte-identical base commit, in both profiles.

**Which half proved it: the adversarial half (§4), not the measurement.** The
instrumented suite returned a clean **0/0** in debug and release across
1 000+ tests (§3) with a validated probe, and that zero is *entirely* an
artefact of the fixture: every existing test drives the crate on tokio's
current-thread runtime, whose `wake()` queues rather than polls inline. No
amount of additional testing against that harness would have found this —
working rule 13 exactly, and the third time this project has hit it.

**The mechanism, stated as the invariant and its owner.** `deadline()`'s doc
is right that only the caller's position can establish the precondition, and
`run` step 2 is *almost* that position. What it misses is that `serve()`'s
**post-drain tail** — `release_dead`, `prune_ready`, `prune_waiting`,
`dispatch_intros` — runs after the final `dirty` scan and **calls into
consumer code**:

* `dispatch_intros` → `drop(reply.send(intro))` wakes a parked `accept()`;
* `release_dead` → `resolve_slot(.., Failed(Local))` wakes a `Connecting`;
* `release_dead` → drops `ConnRecord`, hence `remote_static: PublicKeyOf<I>`,
  a **consumer** type whose `Drop` is consumer code (no waker needed at all).

A consumer whose executor polls inline — which this crate's own docs name in
three separate places as a supported case, and defend against by releasing
the borrow first — re-enters the data path there and queues core output that
nothing drains before `deadline()` pops it.

So the precondition is not "no yield since the drain". It is **"no consumer
code since the last `dirty` scan"**, and `serve()` violates it itself, in its
own tail, with no yield involved. §16.4's drain contract is discharged for
every *mutating verb*; it is not discharged for the *tail*.

**Working rule 12 applies to the existing argument.** The step-2 comment
verifies something true — that no `.await` separates `serve()` from
`deadline()` — and the state it assumes is "consumer code only runs at await
points". On a single-threaded reactor with consumer-supplied wakers that
assumption is false, and it is false inside `serve()` itself. This is the
seam-review pattern once more: a sound argument about the wrong state.

**One more thing the verdict must say.** The `deadline` doc treats these arms
as a detector, the way `drain_endpoint`'s sibling doc does — *"the assert
turns that into a named failure in every debug build, which is every
`cargo test`"*. §4.5 measured that this is **not what happens**: the panic is
inside the `spawn_local` task whose `JoinHandle` the shell drops, so the test
printed `ok` while the driver died under it. The arms are neither a guard
(the datagram is destroyed either way) nor a working detector (debug does not
go red). They are currently a silent data-loss site with a comment on it.

## 6. Candidate mechanisms, measured

The brief asks whether the arm can **re-queue** or must **process in place**.
Measured answer: **neither is available at that signature**, and that is
itself informative.

* **Re-queue is impossible without a new core verb.** `outputs` is a private
  `VecDeque` on each core and the only accessor is `poll_output`, which pops.
  There is no `push_front`, and adding one would be a §16.4 API change whose
  entire purpose is to undo a mistake the same function just made.
* **Processing in place is impossible at that signature.** `deadline(&self)`
  is a shared borrow returning `Option<Instant>`. Handling a `Transmit` needs
  `&mut self` (to push into `outgoing`, which is already built and about to
  be awaited); handling a `ConnOutput::Event` needs `publish`, also
  `&mut self`. The honest version of "process in place" is *"run the drain
  again"* — which is candidate B, and belongs in the caller, not the arm.

### 6.A Candidate A — make the read non-destructive (implemented, measured)

Root cause in one sentence: **`deadline()` uses a popping verb to read a
value.** Both cores already compute that value with a `&self` method
(`Timers::next`, `Endpoint::deadline`), so the fix is additive and tiny.

Patch shape (full text in `scratchpad/round41/candidate-A.patch`, 79 lines):

* `core::Connection::next_deadline(&self) -> Option<Instant>` = `self.timers.next()`
* `core::Endpoint::next_deadline(&self) -> Option<Instant>` = `self.deadline()`
* `Driver::deadline` reads both accessors under a **shared** `borrow()`; both
  `_` arms and both `debug_assert!(false)` lines are deleted; the
  `.chain(endpoint).min().inspect(..)` tail — ruling 255's streak `assert!` —
  is byte-for-byte unchanged.

**Properties.**

* The arms are **removed, not strengthened**. There is no popped output, so
  there is nothing to assert about and nothing to destroy. Ruling 250's
  "dead disjunct" shape, except this disjunct was alive.
* A core with an undrained output now announces its **correct** next deadline
  and keeps the output. The `Command::Dirty` its mutator posted wins the
  `biased` command arm on the very next turn, so the datagram goes out one
  turn later — measured as delivery at **virtual time zero** in §4ter.
* `borrow_mut()` becomes `borrow()` on both cells, which also removes an
  exclusive borrow held across consumer-reachable code.
* Ruling 255's streak `assert!` is untouched: it is on the value path.
* **`run` step 2's positional constraint dissolves.** The step-2 comment's
  *"Moving this call after `transmit().await` is what the regression test
  `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer` exists
  to catch"* stops being true, because moving it would no longer lose
  anything. That test still passes (`spec_shell`, green below), but ruling
  262 must decide whether to keep the positional discipline as
  belt-and-braces or record that it is now free. **Recommendation: keep the
  call where it is and rewrite the comment** — reading the deadline before
  the send is still the freshest value, and the discipline costs nothing.

**§16.4 impact.** §16.4 says the terminal `Timeout` is *"simultaneously the
drain sentinel and the next-deadline announcement"*. Candidate A does not
falsify that sentence; it adds a second, non-destructive way to obtain the
announcement half. It is still a **core-API addition and therefore needs
ruling 262 to approve it.** `SPEC.md` was not touched and must not be until
that decision exists.

**Gates on candidate A** (candidate A applied on `ae71823`, agent K worktree):

```
$ cargo fmt --all --check
fmt: no diff

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.01s      (zero warnings)

$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.56s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
 Documenting slither v0.2.0 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.24s

$ cargo test
744 passed · 112 passed · 11 passed · 10 passed · 4 passed — 0 failed anywhere

$ cargo test --all-features
744 · 112 · 24 · 20 · 17 · 16 · 16 · 15 · 12 ×4 · 11 · 10 · 7 · 6 ×2 · 5 · 4 ×3 · 1
— 0 failed anywhere (1 ignored, as at base)

$ cargo test --release --all-features
744 · 112 · 24 · 20 · 17 · 16 ×2 · 15 · 12 ×4 · 11 · 10 · 7 · 6 ×2 · 5 · 4 ×3 · 1 ×2
— 0 failed anywhere
```

The golden-wire and size/constant tests run inside those and stayed green, so
no wire byte moved — as expected for a change that adds two `&self` readers.

**Not run, and not claimed** (working rule 7): `cargo +1.96 check` (MSRV) and
`cargo deny check`. Neither is plausibly affected by two `&self` accessors,
but "plausibly" is not a gate result.

### 6.B Candidate B — drain after the tail (analysed, not implemented)

Keep `deadline()` destructive and instead make `serve()`'s postcondition
true by re-entering the drain after the tail:

```rust
        for _ in 0..DRAIN_BOUND {
            // … existing inner drain, to `settled` …
            self.release_dead(); self.prune_ready();
            self.prune_waiting(); self.dispatch_intros();
            if !self.any_dirty() { settled = true; break; }
        }
```

**Not recommended, for four reasons.** *(a)* It pays an extra tail pass on
every `serve()` for a condition that is almost always false. *(b)* It is
sound only if `dirty` is a **complete** oracle for "this core has output" —
the very property §2.5 needed a ten-call-site audit to establish, and which a
future verb can silently break; candidate A depends on no such invariant.
*(c)* It converts an adversarial consumer's reentrancy into a `DRAIN_BOUND`
panic rather than into ordinary progress. *(d)* It leaves the destructive read
in place, so the next tail call anyone adds re-opens exactly this hole.

### 6.C Candidate C — `unreachable!()` or a release `assert!` (rejected)

The brief's option (a), and it is now off the table. The state is reachable
from a consumer doing nothing wrong, so promoting to a release `assert!`
would turn silent data loss into a **driver panic that kills every connection
on the endpoint** — the ordering error ruling 249(ii) explicitly avoided by
landing after 249(i) had removed the protocol-reachable trip. Here there is no
249(i) to land first *unless it is candidate A*, at which point the assert has
nothing left to guard. `unreachable!()` is strictly worse: the claim is false.

## 7. Rule 9 — the separating assertion a regression should pin

**The pin, for the REACHABLE verdict (candidate A or B):**

> After a consumer-reentrant core mutation performed from inside `serve()`'s
> post-drain tail, the queued datagram reaches the peer **with no advance of
> virtual time**.

`tests/kprobe_reentrancy.rs` is that test. What makes it a pin rather than a
name:

* **It fails on the broken version, in both profiles.** Debug: the driver's
  own `debug_assert` fires *and* the delivery assertion times out. Release:
  no panic at all, and the delivery assertion still times out. Measured, §4ter.
* **The no-clock-advance clause is the load-bearing half.** The core records
  the destroyed packet as *sent*, so loss recovery retransmits it. A test that
  merely `await`ed for the bytes would pass over real data loss on the
  unfixed code. Pinning delivery to virtual time zero is what separates
  "never destroyed" from "destroyed and recovered a PTO later".
* **It would fail against a fix that only silences the assert.** Deleting the
  `debug_assert!` and keeping the pop leaves the release row's behaviour
  exactly as measured — still red.
* **The degenerate implementation violates it.** A driver that drops every
  reentrant mutation delivers nothing; a driver that keeps it delivers on the
  next turn. Those two differ *at t = 0*, which is where the assertion sits.

**What a fixed-verdict pin would have had to look like, had it come out
UNREACHABLE** — recorded because it is the thing that was *not* available:
an assertion that both cores are at `Timeout` when `deadline()` runs cannot be
written without popping, which is the bug. That is exactly working rule 12's
note about this function (*"`poll_output()` is idempotent at `Timeout` — the
question was whether the cores **are** at `Timeout` when `deadline()` runs"*),
and it is why the unreachability claim could never have been pinned by a test
in the first place. It had to be pinned by construction, and candidate A is
the construction.

**A second, cheaper pin worth adding either way:** an in-crate test that
`Driver::stop`'s latch leaves no live core behind, i.e. that
`cell.closed.is_some()` implies `cell.core.is_none() || core.lost.is_some()`.
That is the invariant §2.5.5 leaned on to close R1, it is currently
maintained by adjacency (two consecutive statements in `stop`) and by
`emit_closed` assigning `lost` next to its push, and nothing tests it. If
either drifts, `poll_read`'s `Ok(Some(0))`-with-latch arm starts pumping on a
live core with `dirty = false` and R1 becomes a **second** route into the same
destruction — one that needs no exotic waker at all.

## 8. Appendix: raw command log

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a38ce6e031ab1da1c
$ git rev-parse HEAD
ae718236b4da215ba85d4366b482c09715d0e811
$ git status --short
(clean)
```

Artefacts left in `scratchpad/round41/`:

| file | what |
|---|---|
| `K-driver-arms.md` | this report |
| `candidate-A.patch` | candidate A, `git apply`-able on `ae71823` (79 lines) |
| `kprobe-control.log` | 132 control hits — the probe is shown able to fire |
| `kprobe-repro.log` | `KPROBE-HIT connection` — the construction, one hit |
| `kprobe-repro2.log` | `KPROBE-HIT connection Transmit(57 bytes to 10.0.0.1:4001)` |
| `kprobe_reentrancy.rs.txt` | **the reproducer, final form** — the file that lived at `tests/kprobe_reentrancy.rs` |
| `verbs.py` | the `&mut self` verb extractor used for §2.5's inventory |

`kprobe-debug.log` and `kprobe-release.log` are **absent by design**: the
probe only creates the file on a hit, and the full suite produced none.

**Worktree state at hand-off.** Reverted to `ae71823` exactly — `src/`,
`Cargo.toml` and `tests/` all restored, the scratch test file and its
`[[test]]` stanza removed. Nothing was committed. Candidate A survives only
as the patch file above, per the brief's evidence-only instruction.

**The reproducer itself is not in the tree.** If ruling 262 wants it as a
regression test it needs a home and a `[[test]]` stanza, and working rule 15
makes that stanza the integrator's to add — the same reason it is not left
behind here. Its full text is preserved verbatim as `scratchpad/round41/kprobe_reentrancy.rs.txt`
(§4.2 shows an earlier draft of it; the `.rs.txt` file is the measured one).

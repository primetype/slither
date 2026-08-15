# Seam review — post-slice-3b (sonnet)

Reviewer: Claude (Sonnet 5), chartered post-slice-3b seam review.
Commit under review: `4a7d28e` — "Slice 3b: the driver and the handles. S1 closes."
Scope: `src/shell/{mod,driver,endpoint,staged,connection,shared}.rs` (~3070 lines, all new).
`src/core/**` frozen and separately reviewed — only in scope where the shell drives it wrongly.

Write findings, not fixes. No repository file modified by this review.

---

## Method notes (filled in as I go)

- Read `IMPLEMENTATION-3b.md` in full. Implementer's own summary of the
  seam design: one `Rc<RefCell<ShellState<I>>>` + one unbounded mpsc
  command channel handle→driver; per-connection `Rc<RefCell<ConnCell<S>>>`.
  Nine decisions (D1-D12, D... some renumbered) and four self-reported
  conflicts (C-B1, F1, F2, F3) plus one integration note (I1). IMPL-B
  claims the load-bearing invariant is "no RefCell borrow is ever held
  across an await" — need to verify this claim directly against
  `driver.rs`, not take it on faith.
- IMPL-B explicitly names D3 (shell-side stamped static mirror) as the
  answer to C-B1: shell keeps its own NONE/PENDING/LIVE map because the
  frozen core's `connect()` does 2 DH inline (mint + start_attempt
  fused), contradicting ruling 87's "connect() performs no DH" prose.
  This is exactly target 7. Need to find the actual disagreement window
  and what breaks.
- D4: cancelling a Connecting sends `Retired { our_index: 0 }` — need to
  check this against `IndexTables::mint`'s nonzero guarantee directly in
  the core, not just trust the doc comment claim.
- Now reading PLAN.md §4, §6, §7, then the six shell source files in
  full before forming per-target findings.
- Read PLAN.md §4 (shell spec digest), §6 (unstated-scope hunt U1-U10),
  §7 (conflicts C1-C5). Confirmed ratified rulings 81, 84, 87, 88, 89 in
  full from `.spec-v2-clean-slate/rulings.md` (verbatim, not summarized).
  Key: ruling 81/84 final text: `Closed(_)` at death; `Retired` follows
  `CloseLinger` expiry on closing/draining paths, or same drain on
  no-linger paths; no `Retired` at all when no session ever installed.
  Ruling 87's premise ("connect() performs no DH... the verb only mints
  the pending") is asserted in the ruling text itself, dated 2026/08/15
  (round 14) -- i.e. written AFTER slice 3a's core was already frozen
  with `connect()` fused (mint + start_attempt, 2 DH). So the ruling's
  own factual premise about the core is questionable -- worth checking
  ruling 87's author had actually re-examined the frozen core or was
  reasoning from the spec text alone. This bears directly on target 7.
- Read src/shell/driver.rs, connection.rs, endpoint.rs, staged.rs, shared.rs,
  mod.rs (the #[cfg(test)] smoke tests) in full.
- Traced the drain loop (`Driver::run` / `serve` / `serve_endpoint` /
  `serve_connection`) by hand against target 1. Structural finding: every
  mutating entry point (`handle_command`, `handle_datagram`,
  `handle_timeout`) is invoked exactly once per `select!`-driven loop
  iteration, and the loop unconditionally calls `self.serve()` again at
  the top of the *next* iteration -- so every mutating call is eventually
  drained, with no other core-mutating call able to interleave first
  (single-threaded, no yield between "handle the one event" and "loop
  back to top"). Handle-side mutations (`Connection::close_now`) apply
  directly to the core on the *handle's* task/stack, then signal
  `Command::Dirty` -- deferred draining, matching the documented "not
  always the driver that made the call".
- Traced `Connecting::drop` (endpoint.rs:230-256) and `Connection::drop`
  (connection.rs:258-276) by hand against target 5 (ruling 50, 88).
  `Connecting::drop` writes NONE into the *same* `Rc<RefCell<ShellState>>`
  `Endpoint::connect` reads, before sending `Command::Cancel` -- so the
  MUST is structurally satisfied with no clock advance, matching D6/Q6.
  Traced the "unpolled Ready" edge case (Connecting resolved by the
  driver into the slot, but dropped before ever being polled): the
  contained `Connection` drops along with the `PendingSlot`'s only
  remaining `Rc`, correctly triggering `Connection::drop`'s own
  accounting a second time. No double-borrow/panic found on this path.
- Traced ruling 88's coincident-drop case in `Connection::drop`: handle
  count decremented, then `self.shell.release()` (process-wide), *then*
  `close_now()` only if `last_for_connection && !last_in_process` --
  order is deliberate and matches the documented rule.
- Traced `closed()` / `Wakers` / `WakerSlot` (target 3) by hand:
  check-then-park happens inside one unbroken `cell.borrow_mut()`, so no
  interleaving is possible between the check and the registration
  (single-threaded, no yield inside `poll_closed`). `WakerSlot::drop`
  unparks unconditionally and idempotently. No lost-wake or re-entrant-
  borrow scenario found in this path.
- Traced staged verbs (target 4) by hand: `self` (Intro/Claimed/Proven)
  is moved into the `async fn`'s generated state and stays alive across
  the `round_trip(...).await`; `self.consumed` is only set to `true`
  *after* the await resolves, so a mid-await cancellation always leaves
  `consumed == false` and the object's own `Drop` fires
  `Command::Reject`. Confirmed the "DH already spent, not refunded" cost
  IMPL-B documents is real: the driver may have already mutated the core
  and buffered a reply in the oneshot before the receiver drops; the
  buffered value (in `accept()`'s case, a fully-built `Connection`) is
  dropped synchronously *on the driver's own call stack* inside
  `command_accept_chain`, which re-enters `Connection::drop` while
  `handle_command` is still on the stack. Traced the borrow scopes by
  hand: no borrow is active at that point (`self.shell.state` and
  `cell`'s borrows from earlier in `command_accept_chain` are already
  released by then), so no re-entrant-borrow panic -- but flagging this
  as the least obviously-safe path found so far; worth a second pass.
- Open thread before writing target 1's final verdict: `Driver::deadline()`
  calls `poll_output()` on both cores *again*, after `serve()` already
  drained each to `Timeout` (whose payload was discarded with `_` inside
  `serve_endpoint`/`serve_connection`). This assumes calling
  `poll_output()` while already parked at `Timeout` is idempotent (no
  side effects, same deadline returned) -- checking the frozen core's
  `poll_output` implementations next to confirm this is actually true
  before ruling target 1 clean.
- Confirmed both `poll_output` implementations are idempotent at the
  `Timeout` state: `core::Connection::poll_output`
  (src/core/connection/mod.rs:322-326) recomputes `self.timers.next()`
  fresh each call, and `Timers::next(&self)` (src/core/connection/timers.rs:170-172)
  is a pure `.iter().flatten().copied().min()` over an array, `&self` not
  `&mut self`. `core::Endpoint::poll_output` (src/core/endpoint/mod.rs:260-265)
  likewise recomputes `self.deadline()`, also `&self`
  (src/core/endpoint/mod.rs:278-287), a pure min over pendings/intros/
  guard. So `Driver::deadline()`'s second round of `poll_output()` calls
  (driver.rs:604-634) is safe and reads, not mutates. Target 1 clean.
- Verified `IndexTables::mint` (src/core/endpoint/tables.rs:47-57) really
  does loop until `candidate != 0` and absent from both tables — D4's
  `our_index: 0` safety claim (driver.rs:754-765) is correct, not merely
  asserted.
- Verified target 7 (C-B1) by reading `core::Endpoint::accept()`
  (src/core/endpoint/staged.rs:480-575): it unconditionally returns
  `AcceptError::Stale` for *any* existing entry in `self.statics` (LIVE
  or PENDING alike — the guard at staged.rs:503-506), and the endpoint
  module's own docs (src/core/endpoint/mod.rs:22-35) state that §6.4's
  replace/re-home is explicitly a slice-7 boundary, not yet implemented.
  So the one scenario that could make the shell's stamped mirror
  (`ShellState::statics`) diverge from the core's own map *without*
  going through the stamp-checked `release_static` path — an inbound
  accept() replacing an already-LIVE static — is **unreachable** in
  slice 3b's actual code paths: the core refuses it before the
  shell-side mirror write in `command_accept_chain` (driver.rs:786-869)
  is ever reached. Traced all six of the mirror's writers (the table in
  driver.rs's module docs, driver.rs:61-68) against the core's real
  transitions one at a time; found no divergence window in
  currently-reachable code. Full verdict and reasoning in §7 below.
- Verified target 2 (Retired ordering) against the **frozen core**, not
  just the shell's own claims, because `Driver::release_dead`
  (driver.rs:481-509) gates release on `cell.closed.is_some() &&
  !cell.is_established()` rather than an explicit "did I see Retired"
  flag. Read `core::Connection::drop_state` (src/core/connection/mod.rs:511-525):
  it is the *only* place `self.session` is set to `None` (making
  `is_established()` false) for every no-linger and linger-expiry death,
  and it pushes `ToEndpoint::Retired` in the *same* function call before
  returning. Read `die()` (mod.rs:506-509) and `handle_timeout`'s
  `TimerKind::CloseLinger => self.drop_state()` arm (mod.rs:263-264): both
  converge on `drop_state()`. Read `close()`'s no-session branch
  (mod.rs:284-309, ruling 84's case): it sets `Lifecycle::Dead` directly
  without calling `drop_state()`, and `is_established()` was *already*
  `false` (no session ever existed) before this call — so
  `release_dead()` correctly frees it in the very next drain with no
  `Retired` ever queued, exactly matching ruling 84. Since
  `is_established()` going false and `Retired` being queued always
  happen inside the same core function call, and `release_dead()` only
  runs after `serve_connection` has drained a connection's *entire*
  output queue to `Timeout` (driver.rs:339-373) for every dirty
  connection in the pass, there is no interleaving in which shell-side
  release can precede endpoint delivery. Full argument in §2 below.
- **New finding, not self-reported by IMPL-B**: `Driver::dispatch_intros`
  (driver.rs:512-541) only prunes dead `self.waiting` entries (accept()
  callers whose future was cancelled) *inside* the `while
  !self.ready.is_empty()` loop (driver.rs:513; the inner prune itself is
  at 518-520). `Driver::prune_ready` (driver.rs:290-297), by contrast,
  runs unconditionally on every drain pass via `serve()` (driver.rs:268).
  A cancelled `accept()` on an endpoint that never has a ready
  introduction to hand out therefore leaves its dead
  `oneshot::Sender<Intro<I>>` in `self.waiting` forever — the same
  unbounded-intermediate-queue shape D12 fixed for `self.ready`, applied
  to its dual. D12's module docs never mention `self.waiting` needing
  the same treatment. Confirmed by full read of `dispatch_intros`: no
  other call site prunes `self.waiting`. Full argument in §6 below.
- Independently reran the release gates rather than trusting
  IMPLEMENTATION-3b.md's §6 transcript: `cargo build --all-features
  --all-targets` (clean), `cargo test --all-features` (297+103+11+4+9+4+16+7
  = matches the reported 451, all green), `cargo clippy --all-features
  --all-targets -- -D warnings` (clean, cached). Corroborates the gate
  table before relying on any of IMPLEMENTATION-3b.md's other claims.

---

## 1. Drain discipline (§16.4)

**Verdict: clean.** Argument below; no finding.

### Structural argument

`Driver::run` (driver.rs:170-234) is a `loop`. Every mutating entry
point — `handle_command` (640-680), `handle_datagram` (572-590),
`handle_timeout` (592-602) — is invoked from exactly one `match event`
arm per iteration (one `select!` branch wins per iteration; `commands`
is biased first for ruling 50's ordering). None of these arms contains
an early `break`; the only `break` is `Event::Command(None)`, which is
unreachable while any handle lives (the loop's own handles==0 check at
line 191 already broke first in that case). So after handling exactly
one event, control unconditionally falls back to the top of the `loop`,
which calls `self.serve()` (line 176) — the drain — before waiting for
anything else. No other core-mutating call can interleave between "one
event handled" and "drain to Timeout," because the whole thing is
single-threaded with no `.await` between them.

Handle-side mutations are the other source (§16.3's "not always the
driver that made the call"): `Connection::close_now`
(connection.rs:135-153) calls `core.close(now(), ..)` directly on the
handle's own task/stack, then sends `Command::Dirty(id)`
(shared.rs:228-230) over the unbounded channel. This *defers* the drain
to whenever the driver task is next scheduled, but nothing is lost: the
mutation is already applied to the `RefCell`-protected core; the
`Command::Dirty` just wakes the driver to go collect the resulting
outputs. No other borrow of the same cell can occur in between (there is
only one thread).

`serve()` (241-271) itself is a fixed-point loop: drain the endpoint
core, collect every connection whose cell is marked dirty, drain each of
those, and repeat until nothing is dirty (bounded by `DRAIN_BOUND =
100_000`, a panic guard against a runaway core rather than a normal
codepath). This correctly converges even when draining one connection's
`ToEndpoint(Retired)` event mutates the endpoint core, which might
enqueue outputs (e.g. install another connection) — those are only
visible on the *next* call to `serve_endpoint`, and the outer `for`
loop's re-scan of `dirty` after every pass picks them up.

### Ordering within a drain

`serve_connection` (333-373) and `serve_endpoint` (300-327) each drain
one core's `poll_output()` in a tight loop, handling exactly one output
per iteration and appending to `out: Vec<Outgoing>` (or acting
immediately, for `ToEndpoint`/`ToConnection`/events) before moving to
the next. Since the core's own `outputs: VecDeque` is FIFO
(`pop_front()`, confirmed at src/core/connection/mod.rs:322-326 and
src/core/endpoint/mod.rs:260-265) and the shell never reorders what it
reads, "a transmit and the event it caused" preserve the core's own
emission order by construction — the shell adds no reordering hazard
here. Any bug in the core's *own* enqueue order would be a core defect,
out of scope per the charter.

### The re-poll after drain is idempotent (checked, not assumed)

`Driver::deadline()` (604-634) calls `poll_output()` a *second* time on
every core after `serve()` already drained each to `Timeout` (whose
payload was discarded with `_` inside the per-core drain loops). This
pattern only works if `poll_output()` at the empty-queue state is a
side-effect-free read. Verified directly: `Timers::next(&self)`
(src/core/connection/timers.rs:170-172) and `Endpoint::deadline(&self)`
(src/core/endpoint/mod.rs:278-287) are both `&self`, pure `.min()`
scans with no mutation, no jitter draw, nothing state-changing. So the
repeated `poll_output()` calls are safe rereads, not a second
consumption. This is exactly the kind of "the shell drives the frozen
core" check the charter calls in-scope, and it holds.

### What I checked and found no counter-example for

- A mutating call never drained: none found — every mutating entry
  point funnels through the loop's unconditional return-to-top.
- A drain that stops early: `serve()`'s fixed-point loop re-scans dirty
  connections after every pass; `serve_endpoint`/`serve_connection` each
  run to their own `Timeout` before returning.
- A dropped `Transmit` or event: `out: Vec<Outgoing>` accumulates every
  `Transmit` seen; `ToEndpoint`/`Event` outputs are acted on immediately
  inside the same loop iteration that popped them, never discarded.
- An announced deadline never re-armed: `deadline()` runs once per
  iteration of `run()`'s loop, after every drain, and its idempotent
  reread (above) means the value is always fresh relative to the state
  `serve()` just settled.

---

## 2. `Retired` ordering (rulings 81, 84)

**Verdict: clean.** This is the chartered highest-cost target and the
one I verified most deeply, against the frozen core's real code rather
than the shell's documentation of it. Full argument below; no finding.

### The mechanism

`Driver::release_dead` (driver.rs:481-509) is the shell-side release. It
does **not** track "have I seen a `Retired` for this connection" as an
explicit flag — instead it gates on:

```rust
cell.closed.is_some() && !cell.is_established()
```

`ConnCell::is_established()` (shared.rs:194-199) delegates to
`CoreConnection::is_established()`, which is `self.session.is_some()`
(src/core/connection/mod.rs:129-131). The question is whether this
proxy is *exactly* synchronized with `Retired` being queued — if
`is_established()` could go false *before* `Retired` is queued (or
`cell.core` released before the queued `Retired` is drained), the MUST
would break.

Traced every place `self.session` is set to `None`:

1. **`drop_state()`** (src/core/connection/mod.rs:511-525) is the
   *only* place that clears `self.session` for both the no-linger and
   linger-expiry death paths. It does two things in one function call,
   in this order: `let our_index = ...; self.outputs.push_back(Retired
   { our_index })` (only if a session existed), *then*
   `self.lifecycle = Lifecycle::Dead`. So the moment `session` becomes
   `None` is textually inside the same synchronous call that queues
   `Retired`.
2. **`die()`** (mod.rs:506-509, the no-linger paths — liveness, nonce
   exhaustion, and eventually `Replaced`) calls `emit_closed(lost)` then
   `drop_state()` — both inside one call, so `Closed` and `Retired` land
   in the core's `outputs` queue together, before the shell ever gets
   control back.
3. **`handle_timeout`'s `TimerKind::CloseLinger => self.drop_state()`**
   arm (mod.rs:263-264) is the linger-expiry path: the *same*
   `drop_state()` runs much later (after `CLOSE_LINGER`), driven by a
   fresh `handle_timeout()` call.
4. **`close()`'s no-session branch** (mod.rs:284-309 — ruling 84's "no
   `Retired` without a session" case): sets `Lifecycle::Dead` directly,
   *without* calling `drop_state()`, and never queues `Retired`. But
   `is_established()` was already `false` here (no session ever
   existed, this is the pre-establishment close case), so
   `release_dead()`'s gate is satisfied without ever having required a
   `Retired` — exactly ruling 84's point, and correctly implemented.

Because `is_established()` transitioning to `false` and `Retired` being
enqueued always happen inside the *same* core function call
(`drop_state()`, for every path that ever emits `Retired`), and because
`Driver::serve_connection` (333-373) drains one connection's *entire*
output queue to `Timeout` before returning — delivering
`ToEndpoint::Retired` to `handle_connection_event` synchronously inside
that same drain (driver.rs:355-366) — by the time `release_dead()` runs
at the end of `serve()` (line 267, after every dirty connection in the
pass has been fully drained), any connection whose `is_established()`
just went false has *already* had its `Retired` delivered in this exact
same drain pass. There is no ordering in which `release_dead()` can see
`!is_established()` without `Retired` having already reached
`handle_connection_event` first (except the no-session case, where none
was ever owed).

### Checked against every path named in the charter

- **Closing → linger expiry**: `close()`'s session-exists branch calls
  `enter_closing` (mod.rs:449-464), which does *not* touch `session` or
  emit `Retired` — only `emit_closed(LocallyClosed)`. So during the
  5s linger, `is_established()` stays `true` (`release_dead()`'s gate is
  false, correctly skipped), and only the *later* `CloseLinger` timeout
  → `drop_state()` clears the session and queues `Retired`, in the same
  drain pass as its own delivery.
- **Peer CLOSE while live → draining**: `apply_live`'s `Frame::Close`
  arm (mod.rs:381-393) sets `Lifecycle::Draining` and calls
  `emit_closed(PeerClosed)`, again without touching `session`. Same
  reasoning as above — `is_established()` stays true until the linger's
  `drop_state()`.
- **No-linger deaths (liveness, nonce exhaustion)**: `Closed` and
  `Retired` are queued together via `die()` → `drop_state()`, in one
  drain.
- **Never-established teardown (ruling 84)**: `close()`'s no-session
  branch — no `Retired` ever queued, `release_dead()`'s gate was already
  satisfied before this call. Confirmed no leak: nothing here ever
  needed a `Retired` to run this cleanup path.
- **Early returns**: `release_dead()` has one early-return-shaped
  construct — `let Some(record) = self.conns.remove(&id) else {
  continue; };` (492-495) inside its cleanup loop — but this only
  `continue`s past an id that's already gone from `self.conns` (can't
  happen, since `done` was just collected from `self.conns.iter()` in
  the same synchronous call, no yield in between), so it's dead code,
  not a bypass.

### One thing worth a second look (not a finding, a note)

The synchronization is not via an explicit flag ("Retired delivered:
yes/no") but via the coincidence that `is_established()` and `Retired`
share a single source (`drop_state()`). This is elegant and, as traced,
currently airtight — but it is a *coincidence of the current core's
factoring* rather than an invariant enforced by a type or a comment
co-located with `release_dead()`'s gate. If a future slice ever adds a
path that clears `self.session` *without* going through `drop_state()`
(e.g., some other teardown shortcut), the shell's proxy would silently
break without any local signal at the call site. Not a defect today —
flagged because working rule 8 says a construction like this deserves a
comment stating the coupling explicitly, and `release_dead()`'s own
comment (driver.rs:474-480) does not mention that its correctness
depends on this specific coincidence in the core.

---

## 3. Waker registration under `RefCell`

**Verdict: clean.** Argument below; no finding.

### `closed()` / `Wakers` / `WakerSlot`

`Connection::closed()` (connection.rs:175-186): mints a key
(`cell.borrow_mut().closed_wakers.key()`, a scoped, one-statement
borrow, line 180), constructs a `WakerSlot` guard (not yet registered —
`WakerSlot::new` just stores the key and a release closure, shared.rs:112-115),
then polls `poll_closed` via `poll_fn`.

`poll_closed` (connection.rs:190-199): a single `cell.borrow_mut()`
spans the *entire* check-then-park sequence — check `cell.closed`, and
only if `None`, call `cell.closed_wakers.park(key, cx)` before returning
`Pending`. Because this whole sequence runs under one unbroken borrow,
with no `.await` and no other code able to run on this thread in
between, there is no window in which the state could change between the
check and the registration — which is exactly the race the classic
"register waker, then check state" pattern exists to close in a
multi-threaded setting. Here, single-threadedness plus one borrow
spanning both steps gets the same guarantee more cheaply.

`Wakers::park` (shared.rs:68-81) and `wake_all` (94-98): `wake_all`
drains the map (`mem::take`) before waking, so a woken task that
re-polls and re-parks does not observe its own stale entry, and a woken
task that resolves immediately needs nothing removed. `latch()`
(driver.rs:397-403) sets `cell.closed` and calls `wake_all()` inside one
borrow, matching `poll_closed`'s own single-borrow read — no
interleaving possible between "closed is set" and "wake_all runs" from
outside.

Cancel-safety: `WakerSlot::drop` (shared.rs:122-126) unconditionally
calls `unpark(key)`, which is `self.parked.remove(&key)` — idempotent
whether or not the key was ever actually parked (e.g. dropped before
first poll) or already removed by a prior `wake_all()`. The `slot` local
in `closed()` outlives the `poll_fn` future across the whole `.await`
(borrowed by the polling closure, not moved out), so a mid-`.await` drop
of the outer future correctly runs `WakerSlot::drop` and leaves the map
exactly as found — matching the documented cancel-safety claim
(connection.rs:161-163).

### Re-entrant borrow check

Searched for any place a callback captured by a waker guard or closure
could run while a borrow of the *same* `RefCell` is already active
higher up the call stack. `latch()`'s `cell.borrow_mut()` and
`poll_closed`'s `cell.borrow_mut()` are always taken and released within
one synchronous function, never nested — confirmed by reading every
`.borrow()`/`.borrow_mut()` call site in connection.rs, driver.rs, and
shared.rs and checking each one's enclosing scope ends before the next
one begins (this is also asserted as the file's own rule, shared.rs:8-15
and driver.rs:27-34, and I did not find a violation of it).

### Unbounded waker map

`closed_wakers`'s key space grows only while `closed()` futures are
outstanding and not yet dropped/resolved — bounded by application
concurrency, not by time or connection count, the same shape as every
other "verb-rate-bounded, not payload-bounded" channel in this design
(D1's own reasoning). Not a finding.

Note: `self.waiting` (the `accept()` caller queue, `Vec<oneshot::Sender<Intro<I>>>`-equivalent
`VecDeque`) is architecturally the same *kind* of structure as a waker
map but is pruned very differently from `closed_wakers` — see §6's new
finding on `dispatch_intros`, which is a genuine unbounded-growth gap in
that queue specifically, distinct from anything wrong with the
`Wakers`/`WakerSlot` machinery reviewed here.

---

## 4. Cancel-safety of every `async fn`

**Verdict: clean**, with one path flagged as the least-obviously-safe
(traced and found safe, but worth a second, independent look before
slice 4 builds more on top of it).

### Inventory of every `async fn` in the shell

`Connection::close`/`closed` (connection.rs:115,175), `Endpoint::accept`
(endpoint.rs:75), `Intro::read_identity`, `Claimed::authenticate`,
`Proven::accept` (staged.rs:126,195,275), and the internal
`round_trip` helper (staged.rs:317).

### `Connection::close()` / `closed()`

`close()` (connection.rs:115-117) is `poll_fn` over `poll_close`, which
is **always `Ready` on the first poll** (connection.rs:121-131,
documented as deliberate — §16.7 makes sealing synchronous). There is no
await point inside it at all, so there is nothing to cancel mid-flight;
the mutation and the resolution are the same synchronous step.

`closed()` — covered in §3 above: a dropped future's `WakerSlot::drop`
cleans up exactly its own entry; nothing is lost or stranded because the
future itself never held anything but a key.

### The staged ladder: `self` by value across a driver round-trip

`read_identity(mut self)`, `authenticate(mut self)`, `accept(mut self)`
(staged.rs:126,195,275) each move `self` into the generated `async fn`
state, which stays alive across `round_trip(...).await`
(staged.rs:317-327 — a plain `send` then `rx.await`). `self.consumed` is
only set to `true` **after** the round trip resolves
(staged.rs:131,199,283). So a mid-`.await` cancellation of the *outer*
future always finds `consumed == false`, and the object's own `Drop`
(`Intro`/`Claimed`/`Proven`, e.g. staged.rs:148-154) fires
`Command::Reject(id)` — the documented "cancel rejects the chain"
behaviour (D11). Traced where the object "goes": it lives inside the
`async fn`'s anonymous future struct, and is dropped along with that
future when the caller drops it (e.g. a `select!` losing arm, or a
`timeout()` firing).

### The deepest interleaving: cancelling `accept()` after the driver has already built the `Connection`

This is the path I'd flag as least-obviously-safe. If the driver has
already run `command_accept_chain` (driver.rs:786-869) to completion —
built the `Connection` handle and called `reply.send(Ok(handle))`
(driver.rs:868) — **before** the caller's `Proven::accept()` future is
dropped, the drop races the buffered oneshot value. If the caller drops
first, `reply.send(...)`'s caller already returned (this already
happened, synchronously, on the driver's own stack, since oneshot
`send` is not async) — so the actual race is the *other* direction:
caller drops the receiver, and `command_accept_chain` was already
finished by the time that happens. The documented case
(staged.rs:267-274) is: caller drops the future *after* the driver
replied but before reading it. Since `oneshot::Receiver`'s `Drop`
doesn't retroactively un-send, what actually happens is the buffered
`Ok(handle)` is dropped along with the closed receiver — **on the
caller's own task**, not the driver's, since the receiver and its
buffered value live in the caller's future. Traced this precisely: this
drops the already-built `Connection` on the *caller's* stack, which is
`Connection::drop` (connection.rs:258-276) — a fresh, unrelated call
stack from the driver's — so no re-entrancy risk there at all. I had
initially mis-traced this as happening on the driver's stack (inside
`command_accept_chain`'s `drop(reply.send(Ok(handle)))`, driver.rs:868)
via the *other* interleaving — caller's receiver already dropped
**before** the driver calls `reply.send(...)` (e.g. the future was
cancelled while `command_accept_chain` was mid-flight, which cannot
happen mid-*call* since there's no yield inside it, but easily happens
between commands: `Command::AcceptChain` is still queued, the caller's
future is dropped, *then* the driver processes the (now-orphaned)
command). In *that* interleaving, `reply.send(Ok(handle))` at
driver.rs:868 returns `Err(Ok(handle))` (receiver already gone), and
`drop(...)` on that value **does** run `Connection::drop` synchronously
on the driver's own stack, inside `handle_command` → `command_accept_chain`.

Traced the borrow scopes for *this* interleaving by hand: at
driver.rs:868, the borrows taken earlier in the same function
(`self.shell.state.borrow_mut()` at 792 and 833, `cell.borrow_mut()`
inside `Connection::new` at 846) have all already ended (each is scoped
to its own statement/block). So when `Connection::drop`
(connection.rs:258-276) runs here, its own `self.cell.borrow_mut()`
(261) and `self.shell.release()` → `self.state.borrow_mut()`
(shared.rs:427-428) find no outstanding borrow and do not panic. If
`last_for_connection && !last_in_process`, `close_now()` also runs here
(connection.rs:135-153), sending `Command::Dirty(id)` — just another
unbounded-channel push, processed on a *later* loop iteration, not a
recursive call back into `handle_command`. No stack growth, no
re-entrant-borrow panic found on this path. This is the deepest and
least-obvious interleaving I found in the shell; I'd want a dedicated
paused-clock test exercising exactly this (cancel `Proven::accept()`
after `settle()` has let the driver build the connection, before ever
awaiting the result) if one doesn't already exist — I did not find one
by name in the story/spec test files (see §6).

### What's lost, honestly, on every cancelled staged verb

The DH the driver may already have spent before the cancellation lands
is not refunded (staged.rs:315-316, documented). This is an accepted,
stated cost, not a bug — flagging only that it's real and I confirmed
the reasoning holds (§6.1 prices the ladder cumulatively, so a
half-completed round trip cannot un-spend a DH already performed).

---

## 5. Drop order across handles (rulings 50, 62, 88; story S26)

**Verdict: clean for the ratified paths (50, 88)**, with one new
adjacent finding below (driver-panic robustness — low severity, not a
ratified-path defect).

### Ruling 50's MUST: cancel ordered ahead of any later endpoint verb

`Connecting::drop` (endpoint.rs:230-256): computes `in_flight` from
`self.resolved` and the slot's current outcome (237-238), and if in
flight, calls `release_static` (243-247) — a synchronous write into the
**same** `Rc<RefCell<ShellState>>` that `Endpoint::connect`'s
`claim_static` reads (endpoint.rs:113-121) — *before* sending
`Command::Cancel` (249-251). Since `Endpoint::connect` and `Connecting`
share the identical `Rc` (cloned via `Shell::clone`, shared.rs:378-385),
the NONE write is visible to the very next synchronous call with **no
yield, no clock advance, and no driver round-trip** in between. Verified
by hand against the exact scenario in the module smoke test
`a_cancelled_dial_frees_the_static_with_no_clock_advance`
(mod.rs:149-178): `drop(dialling); let redial = ...connect(...).expect(...)`
on consecutive lines with nothing between them — the code makes this
true structurally, not by luck of scheduling.

The `Command::Cancel` and any later `Command::Connect` (from a redial)
travel on the same FIFO `mpsc::UnboundedSender` (shared.rs:413-415,
`Command::send`), and `commands` is the `biased` first arm in the
driver's `select!` (driver.rs:200-206) — so even the driver-side
protocol teardown (index, pending-index, hint set — `command_cancel`,
driver.rs:766-783) is correctly ordered ahead of any later `Connect` for
the same static, independent of the *synchronous* NONE-write argument
above, which is the one that actually makes the MUST true with no clock
advance (Q6's rejected-alternative note, confirmed: a driver-polled flag
would not have this property on a paused clock, and this implementation
does not use one).

Traced the "unpolled `Ready`" edge case by hand: if the driver has
already resolved the slot to `PendingOutcome::Ready(connection)`
(endpoint installed) but the `Connecting` is dropped without ever being
polled, `in_flight` is `false` (outcome is not `Waiting`) — no
double-cancel, no static release. `Connecting::drop`'s only remaining
field to drop is `self.slot: Rc<RefCell<PendingSlot<I>>>`; since
`Driver::establish` (driver.rs:407-451) `.take()`s its own copy of that
`Rc` and lets it go out of scope at the end of the function
(driver.rs:436-450), the `Connecting`'s own `Rc` is the *last* strong
reference by the time this drop runs — so dropping it also drops the
`PendingSlot`, which drops the `PendingOutcome::Ready(Connection)`
inside it, correctly re-entering `Connection::drop`'s own accounting
(handle count, `shell.release()`, and — if this was also the last
handle to the connection but not the last in the process —
`close_now()`, sealing a graceful CLOSE for a connection the application
never got to see). No double-borrow found on this path either: `Shell<I>`
has no custom `Drop`, so `Connecting.shell`'s field-drop is inert, and
`Connection`'s own independent `Rc<dyn ShellLink>` (cloned separately at
construction, driver.rs:443-449) is unaffected by `Connecting`'s field
order.

### Ruling 88's coincident-drop case

`Connection::drop` (connection.rs:258-276): computes
`last_for_connection` first (per-cell handle count), then
`last_in_process = self.shell.release()` **second**, then only calls
`close_now()` if `last_for_connection && !last_in_process` (272-274).
This order is what makes the rule correct — `release()` must be called
before the branch decides, because the branch's own condition is "was
this also the last handle in the process." Traced both directions via
the paired smoke tests in mod.rs (`the_coincident_last_handle_drop_transmits_nothing`,
222-258, and `a_non_coincident_last_connection_drop_does_transmit`,
264-289 — a real pair per working rule 9, not a single test that a
degenerate "always transmit" or "never transmit" implementation would
also pass). Independently reran both under `cargo test --all-features`:
green.

Also traced `Driver::run`'s own ordering (D8): the `handles == 0` check
(driver.rs:190-193) runs *after* `self.transmit(outgoing).await`
(177) — so a CLOSE sealed by a legitimate (non-coincident) last-handle
drop still reaches `transmit()` in the same iteration before the driver
considers stopping. For the coincident case, `close_now()` is never
called at all (traced above), so there's nothing for this ordering to
accidentally resurrect — matches the code comment's own reasoning
(driver.rs:178-184).

### New finding (low severity, not a ratified-path defect): driver panic leaves cell-based waiters permanently hung

`Driver::shutdown` (driver.rs:871-883) is the only place that latches
`ConnectionLost::EndpointDropped` over every remaining connection and
sets `driver_stopped = true`. It is called from exactly one place: the
end of `run()`, on the *normal* (non-panicking) `handles == 0` exit.
If the driver task instead **panics** — the only realistic trigger is
the `DRAIN_BOUND` guard (driver.rs:324-326, 370-372), itself a defense
against a core bug, so this requires a pre-existing core defect to
reach — `shutdown()` never runs. Traced the consequence for each handle
kind:

- Verbs backed by a `oneshot` (`accept()`, the three staged verbs):
  degrade gracefully. The `mpsc` command channel's receiver is dropped
  along with the panicking `Driver`, so any `Shell::send` afterward
  silently no-ops (shared.rs:413-415) and drops the bundled
  `oneshot::Sender` — which resolves the caller's `rx.await` to `Err`
  (`RecvError`), correctly mapped to `None` → `EndpointDropped` (or the
  local error) by `round_trip` (staged.rs:320-326) and `Endpoint::accept`
  (endpoint.rs:75-82).
- `closed()` and any still-`Waiting` `Connecting`: **do not** degrade
  gracefully. Nothing ever calls `wake_all()` on `closed_wakers` or
  resolves the `PendingSlot`'s `Waking`/`Waiting` outcome, because that
  only happens via `Driver::latch`/`establish`/`fail_pending`
  (driver.rs:397-403, 407-451, 461-472) — all driver-owned code that
  never runs post-panic. A task awaiting `closed()` on a healthy
  connection whose driver just panicked, or awaiting a `Connecting` that
  was still in flight, hangs **forever**.

This is an asymmetry between the two seam mechanisms (§16.3's split by
cost) that only manifests if the driver task itself panics — which
should never happen in correct operation, and is explicitly a core
concern rather than a shell one under this charter's scope ("core is
frozen and in scope only where the shell drives it wrongly"). I'm
recording it because it *is* a shell-side gap (no `Drop` impl or
`catch_unwind` boundary on the driver closes it) and because "drop
during unwind" is explicitly named in the charter. **Suspected, traced
by hand, not executed** — I did not attempt to actually force a driver
panic and observe the hang, since doing so would require deliberately
corrupting the frozen core (out of scope to modify) or finding a real
core bug (none found). Severity: low, because it requires a pre-existing
core defect to trigger at all, and a defensive fix (a `Drop` impl on
`Driver` that runs `shutdown()`'s cleanup, or wrapping `run()`'s body in
`catch_unwind`) is a small, local, reversible addition whenever it's
judged worth the cost.

---

## 6. Code tuned to a test rather than the spec (TEST-B contamination risk)

### Finding A (medium severity, demonstrated): `self.waiting` has no unconditional pruning, unlike its dual `self.ready`

`Driver::prune_ready` (driver.rs:290-297) runs **unconditionally** on
every drain pass, from `serve()` (driver.rs:268), to keep `self.ready`
bounded per §10.6 (D12's own stated rationale). Its dual —
`self.waiting: VecDeque<oneshot::Sender<Intro<I>>>`, the queue of
outstanding `accept()` callers — has no equivalent unconditional prune.
The only place a dead entry (a cancelled `accept()`'s closed receiver)
is removed is inside `dispatch_intros` (driver.rs:512-541):

```rust
fn dispatch_intros(&mut self) {
    while !self.ready.is_empty() {                       // 513
        while self.waiting.front().is_some_and(oneshot::Sender::is_closed) {  // 518
            self.waiting.pop_front();                     // 519
        }
        let Some(reply) = self.waiting.pop_front() else { return; };  // 521
        ...
    }
}
```

The pruning loop at 518-520 is nested *inside* the outer `while
!self.ready.is_empty()`. If `self.ready` is empty — no peer has ever
dialled this endpoint, or none since the last introduction was consumed
— the whole function body is skipped and **no pruning happens at all**,
regardless of how many dead entries have accumulated in `self.waiting`.

**Triggering interleaving:** an application that calls `accept()` with
a timeout in a loop (a common pattern — `tokio::time::timeout(d,
endpoint.accept()).await`) on an endpoint that is rarely or never
dialled. Every timed-out call drops the `accept()` future, closing its
`oneshot::Receiver`, leaving the matching `oneshot::Sender` — one per
call — parked in `self.waiting` forever, because `dispatch_intros` never
reaches the prune step while `self.ready` stays empty. This is the exact
unbounded-intermediate-queue shape §10.6 forbids and D12 explicitly
fixed for `self.ready`; the fix was not applied to `self.waiting`.

**Cost:** unbounded memory growth (`self.waiting`'s length, each entry a
`oneshot::Sender<Intro<I>>`) over the endpoint's life, for this specific
but plausible usage pattern. **Self-healing**: the moment any peer does
dial and `self.ready` becomes non-empty, the front-pruning loop
correctly skips every dead entry before delivering to the first live
one (verified by reading 518-527 — the delivery logic is otherwise
correct, this is purely a *when-does-pruning-run* gap, not a
misdelivery risk). So the practical impact is bounded to "unnecessary
memory retention on an idle or lightly-dialled listener that polls
`accept()` with timeouts," not a correctness or misdelivery bug.
**Demonstrated by code reading** (the gating condition is unambiguous);
not executed, since observing it would require instrumenting private
driver state or a memory-growth measurement rather than a behavioral
assertion — a probe wasn't attempted for that reason. This is a genuine
gap in the implementation's own stated design principle (D12), not
something either TEST-B or IMPL-B flagged.

### Finding B (informational/methodological, demonstrated): no test anywhere exercises 2+ simultaneous connections on one endpoint

Grepped every `.connect(` call site across `tests/story_lifecycle.rs`,
`tests/story_dial.rs`, `tests/spec_shell.rs`, and
`src/shell/mod.rs`'s smoke tests (2577 + ~330 lines respectively): every
single one either (a) is the sole connection attempt for that endpoint,
or (b) is a *second* attempt to the *same* target from the same
endpoint (testing `AlreadyConnected` / cancel-then-redial / give-up-
then-redial). `testutil`'s own harness declares `addr_c()`
(IMPLEMENTATION-3b.md's §1 API listing, "10.0.0.3:4003") specifically
for a third party, but it is never referenced anywhere in the test
tree — confirmed via `grep -rn "addr_c" tests/ src/`, zero hits outside
its own definition.

This means `self.conns: BTreeMap<ConnectionId, ConnRecord<I>>`
(driver.rs:136) never holds more than one entry across the entire
slice-3b test suite, and neither does `self.shell.state.statics` beyond
a single pending/live key at a time. Every multi-connection aggregation
path in `driver.rs` — the `dirty` collection in `serve()`
(driver.rs:253-258, iterating `self.conns`), `deadline()`'s `.chain(..).min()`
across `self.conns.values()` (driver.rs:620-633), `release_dead()`'s
batch `done` collection (driver.rs:482-490), and
`handle_timeout()`'s `for record in self.conns.values()` loop
(driver.rs:595-601) — is exercised, in every test that runs, with a
collection of size 0 or 1. I read each of these and they are written
generically (proper `.iter()`/`.values()` scans, no `.next()`-only or
`.first()`-only shortcuts) — **I found no actual bug**, but working rule
9 applies directly: a regression that collapsed any of these to "handle
only the first/last dirty connection" or "only the most recently touched
connection's deadline" would pass all 2577+ lines of story/spec tests
and all of `mod.rs`'s smoke tests without a single failure. This is
exactly the shape working rule 9 warns about — a green suite that a
degenerate implementation would also pass — applied to the seam's
multi-connection bookkeeping rather than to a single property.

Not attributable to TEST-B specifically (the implementer's own smoke
tests in `mod.rs` have the identical gap), and not something IMPL-B's
self-report (§5 of IMPLEMENTATION-3b.md) flags. I would treat this as a
coverage recommendation for whoever picks up slice 4, more than a
finding about *this* slice's correctness — the code I read is correct
as far as I can tell, just unproven by test for N>1.

### Everything else checked for test-tuning

Read every story test's name against its assertions (not just skimmed)
looking for "works because exactly one connection" or hard-coded
first-iteration behaviour distinct from the multi-connection gap above.
Nothing else stood out: `s29_retry_loop_replaces_rather_than_accumulates`
(story_lifecycle.rs:1244) specifically pins the *stamp* mechanism
against a degenerate "replace by key alone" implementation (matching
`StaticSlot`'s own doc comment, shared.rs:140-147); the ruling-88 pair
(§5 above) is a genuine two-sided pin, not a single-sided one; the
`an_expired_introduction_is_not_handed_to_a_later_accept` test
(mod.rs:443-481) is explicitly a working-rule-9-shaped test — it asserts
on the *live* introduction succeeding rather than on `self.ready`'s
length, which is exactly the right side to assert from (a
degenerate/no-op `prune_ready` would fail this test, since the stale id
would be handed out first and `read_identity()` would answer
`IntroError::Expired`).

---

## 7. IMPL-B finding C-B1 — shell-side static mirror vs core's map

**Verdict: verified — not currently divergent or unsafe.** The
duplication IMPL-B reports is real and the maintainer should still rule
on it (a core split, `mint_pending`/`start_attempt`, would delete the
mirror entirely, as IMPL-B itself recommends), but I traced every
reachable code path in slice 3b and found no scenario where
`ShellState::statics` (the shell's mirror, shared.rs:301-315) and the
core's own static map can disagree in a way that breaks "one connection
per static" or "no static is ever LIVE and PENDING at once."

### The six writers, checked one at a time against the frozen core

Per the table in driver.rs's module docs (driver.rs:61-68):

1. **NONE → PENDING, handle, `Endpoint::connect`** (endpoint.rs:105-139):
   `claim_static` (shared.rs:337-351) writes the mirror *before* the
   driver ever sees `Command::Connect`. If the mirror is wrong here (says
   NONE when the core would refuse), `command_connect`'s error branch
   (driver.rs:727-741) catches it with a `debug_assert!` and *recovers*
   safely regardless (releases the mirror entry, resolves the
   `Connecting` as `Failed`) — so even a hypothetical divergence in this
   specific direction degrades to a spurious failure, not silent
   corruption. I could not construct a scenario where it actually fires,
   given points 2-6 below.
2. **PENDING → NONE, handle, `Connecting::drop`** (endpoint.rs:230-256):
   synchronous, same `Rc`, see §5's ruling-50 argument above — sound.
3. **PENDING → NONE, driver, `HandshakeFailed`** (`fail_pending`,
   driver.rs:461-472): stamp-checked via `release_static`
   (shared.rs:357-365) — a stale writer (an old attempt whose static was
   already reclaimed by a newer one) finds a mismatched stamp and
   silently declines, protecting a newer PENDING/LIVE entry. This is the
   mechanism that makes cancel-then-redial safe (StaticSlot's own doc
   comment, shared.rs:140-147) — traced and confirmed correct.
4. **PENDING → LIVE, driver, `Established`** (`establish`,
   driver.rs:407-451): also stamp-checked (424-434) before writing
   `Live`.
5. **NONE → LIVE, driver, staged `accept()`** (`command_accept_chain`,
   driver.rs:786-869): the risky one. This does an **unconditional**
   `state.statics.insert(...)` (836-842) — no stamp check at all,
   because it's meant to only ever be reached from NONE. Verified this
   assumption directly against the core: `core::Endpoint::accept()`
   (src/core/endpoint/staged.rs:480-575) unconditionally returns
   `AcceptError::Stale` for *any* pre-existing entry in its own
   `self.statics` map, LIVE or PENDING alike (the guard at
   staged.rs:503-506: `if self.statics.get(&peer_key).is_some() { ...
   return Err(AcceptError::Stale); }`). And the endpoint core's own
   module docs (src/core/endpoint/mod.rs:22-35) state outright that
   §6.4's replace/re-home — the only scenario that would make an
   accept() succeed against an existing LIVE entry — is an explicit,
   documented **slice-7** boundary not yet implemented; slice 2a/3a
   "returns `AcceptError::Stale` for the other two [PENDING and LIVE]."
   So `command_accept_chain`'s core-side `endpoint.accept(...)` call
   (driver.rs:792) can only ever succeed when the core's own map had no
   entry for that key — meaning the shell's unconditional overwrite at
   836-842 can only ever write into a slot the mirror should also have
   had as NONE (assuming points 1-4, 6 keep the mirror synchronized,
   which they do). The "LIVE → LIVE replace, bypassing the stamp check"
   scenario I initially worried about (an inbound accept() racing an
   outbound connect's own cleanup, both touching the same key) is
   **unreachable in slice 3b** — the core refuses the accept() before
   the shell-side write is ever reached.
6. **LIVE → NONE, driver, `release_dead`** (driver.rs:481-509):
   stamp-checked via `release_static` (496-499), same protection as
   point 3.

### What I checked and could not break

- Whether the shell's mirror could get "stuck" claiming PENDING/LIVE
  forever while the core says NONE (the dangerous direction — silent,
  permanent `AlreadyConnected` for a static the core would actually
  accept): every core-side transition to NONE I could find (give-up,
  linger expiry, no-linger death, the never-established `close()`
  special case) funnels through `release_static`'s stamp check, which
  only *declines* to remove an entry that's been superseded by a
  *newer* attempt — never leaves a genuinely-dead entry stuck. I did not
  find a core-side "static freed" transition without a matching shell
  writer.
- Whether accept()'s NONE→LIVE overwrite (point 5) could race an
  outbound connect's own PENDING claim for the same key: since
  `command_accept_chain` has no `.await` anywhere in its body (confirmed
  by reading the whole function), and it's the sole path from
  `Command::AcceptChain` handling to the mirror write, nothing else can
  run on this single thread between the core's `accept()` call (792) and
  the mirror `insert()` (836-842) — no interleaving window exists.
- Re-ran `cargo test --all-features`: 451 tests green, including
  `s29_retry_loop_replaces_rather_than_accumulates` and
  `s29_after_cancel_the_static_routes_as_none` — the two tests that
  would most directly surface a mirror/core divergence in the outbound
  path. Empirical corroboration, not a substitute for the trace above,
  but consistent with it.

### Caveat, stated honestly

This verdict is scoped to **slice 3b's actual reachable code**. The
moment slice 7 implements §6.4's replace and `core::Endpoint::accept()`
starts succeeding against an existing LIVE entry, point 5's
unconditional overwrite becomes reachable from a genuinely-occupied
slot, and the "complete set of writers" table (driver.rs:61-68) will
need a new row — my trace above shows the *existing* stamp machinery in
`release_static` would already handle the old connection's late cleanup
correctly even then (its stamp would no longer match, so it declines to
clobber the new LIVE entry), but that's a claim about *slice 7's* code,
which does not exist yet, so I am not certifying it — only noting that
the mechanism already in place looks like it generalizes, which the
maintainer may want on record before slice 7 starts.

---

## Findings summary (by severity)

### Medium

**M1 — `self.waiting` (the `accept()` caller queue) has no unconditional
pruning; dead entries accumulate forever on an endpoint that is never or
rarely dialled while the application polls `accept()` with a timeout.**
`src/shell/driver.rs:512-541` (`dispatch_intros`), specifically the
pruning loop at 518-520 being nested inside the `while
!self.ready.is_empty()` guard at 513. Contrast with the unconditional
`prune_ready` at driver.rs:290-297, called every drain pass
(driver.rs:268) — the same fix, applied to `self.ready` but not its
dual. Triggering input: repeated `tokio::time::timeout(d,
endpoint.accept()).await` on an endpoint that is never dialled.
**Demonstrated by code reading**; not executed (would require
instrumenting private state or measuring memory growth). Cost:
unbounded memory growth over the endpoint's life for a plausible usage
pattern, self-healing the moment any peer ever dials. §10.6 violation in
spirit, matching the exact defect class D12 fixed for the sibling queue.

### Low / informational

**L1 — driver panic (reachable only via a pre-existing core defect
hitting the `DRAIN_BOUND` guard) leaves `closed()` waiters and
in-flight `Connecting`s hung forever**, asymmetric with oneshot-backed
verbs which degrade gracefully via channel-closed semantics.
`src/shell/driver.rs` has no `Drop` impl and no `catch_unwind` boundary;
`shutdown()` (871-883) — the only cleanup that latches
`EndpointDropped` and wakes every waiter — runs only on the loop's
normal exit path. **Suspected, traced by hand, not executed** (would
require a real core bug to trigger; none found). Cost: low, since it
requires a pre-existing core defect as a precondition; the fix (a
defensive `Drop` on `Driver`, or wrapping `run()` in `catch_unwind`)
is small and reversible whenever judged worth it.

**L2 — no test anywhere in the slice-3b suite (2577+ lines of
story/spec tests, plus the implementer's own smoke tests) exercises a
single `Endpoint`/driver managing 2+ simultaneous connections or pending
attempts.** `testutil::addr_c()` exists and is never used. Every
multi-connection aggregation path in `driver.rs` (`serve()`'s dirty
collection at 253-258, `deadline()`'s multi-core `.min()` at 604-634,
`release_dead()`'s batch cleanup at 481-509, `handle_timeout()`'s loop
at 592-602) reads as correctly generic on inspection — **no bug found**
— but is unproven by test for N>1. Working-rule-9 shape: a regression
collapsing any of these to "first/last connection only" would pass the
entire existing suite. Recommend as a coverage item for slice 4 rather
than a slice-3b defect.

**L3 (documentation-only, forward-looking) — the "complete set of
writers" table for `ShellState::statics`** (driver.rs:61-68) **omits a
LIVE→LIVE (replace) row**, currently harmless because
`core::Endpoint::accept()` unconditionally refuses (`AcceptError::Stale`)
any static already occupied — §6.4's replace is an explicit,
documented slice-7 boundary (src/core/endpoint/mod.rs:22-35) — so the
row's absence has no live consequence yet. Traced that the existing
stamp-check machinery in `release_static` would already handle the
old-connection's late cleanup correctly once replace exists, but that's
a claim about code that doesn't exist yet, not a certification.

**L4 (documentation-only) — `Driver::release_dead`'s correctness for
target 2 depends on a coincidence in the frozen core** (that
`is_established()` going false and `Retired` being queued always
happen inside the single function `drop_state()`,
src/core/connection/mod.rs:511-525) **that is not stated at the gate's
own call site** (driver.rs:474-480's comment). Not a defect today
(verified exhaustively in §2); a future core change that clears
`session` outside `drop_state()` would silently break the shell's proxy
with no local signal. Worth a comment cross-referencing the coupling.

### Clean, verified with argument (not merely trusted)

- **Drain discipline** (§1): every mutating call is followed by a drain,
  with no interleaving possible; output ordering is preserved by
  construction; the deadline-reread pattern is confirmed idempotent
  against the frozen core's actual `poll_output` implementations.
- **`Retired` ordering** (§2, the chartered highest-cost target):
  verified against the frozen core's real code (not just the shell's
  documentation of it) that `is_established()` and `Retired` share one
  synchronized source, and that `release_dead()` cannot observe one
  without the other having already run in the same drain. No leak found
  on any path, including ruling 84's no-session case.
- **Waker registration under `RefCell`** (§3): check-then-park happens
  under one unbroken borrow with no yield inside it; no re-entrant
  borrow found; cancel-safety verified via `WakerSlot`'s unconditional,
  idempotent release.
- **Cancel-safety of every `async fn`** (§4): every staged verb's `self`
  survives across its round-trip and rejects the chain on drop; the
  deepest interleaving (cancelling `accept()` after the driver already
  built the `Connection`) traced through to a synchronous,
  non-re-entrant `Connection::drop` on the driver's own stack — safe,
  but flagged as worth a dedicated test.
- **Drop order across handles** (§5): ruling 50's MUST is structurally
  (not just conventionally) true via the shared synchronous cell write;
  ruling 88's coincident case is correctly order-dependent on
  `Shell::release()` running before the branch that decides whether to
  seal a CLOSE.
- **C-B1** (§7): the shell-side static-map duplication cannot currently
  diverge from the core, because the one scenario that could expose it
  (inbound accept() replacing a LIVE static without going through the
  stamp-checked release path) is unreachable — the core refuses it
  outright, verified directly in `core::Endpoint::accept()`.

---

## Verdict on C-B1

**Verified: not currently divergent or unsafe.** IMPL-B's own
self-report (IMPLEMENTATION-3b.md, C-B1 and D3) is accurate about *why*
the duplication exists (ruling 87's "connect() performs no DH" prose
does not match the frozen `core::Endpoint::connect()`, which mints and
spends 2 DH in one call) and honest that it is forced rather than
chosen. I independently traced all six writers of the shell's mirror
(`ShellState::statics`) against the frozen core's actual behavior,
rather than trusting the shell's own module-doc table, and specifically
verified the one path that looked riskiest on paper — an inbound
`accept()` writing `NONE → LIVE` unconditionally, with no stamp check —
by reading `core::Endpoint::accept()` directly
(`src/core/endpoint/staged.rs:503-506`): it refuses (`AcceptError::Stale`)
against *any* existing entry, LIVE or PENDING, because §6.4's replace is
an explicit, documented slice-7 boundary
(`src/core/endpoint/mod.rs:22-35`) that does not exist in this code yet.
So the scenario the charter asks about — "can the mirror and the core's
map disagree, and what breaks if they do" — has no live instance in
slice 3b: every mirror write I could trace corresponds 1:1 with a real
core transition, and every removal is stamp-checked against the
cancel-then-redial race the stamp exists for. The invariants "one
connection per static" and "no static is ever LIVE and PENDING at once"
hold, as far as I could exercise them by reading and by rerunning the
test suite (451 green, including the two tests most likely to surface a
divergence).

This is not a certification that the duplication is a good idea —
IMPL-B and the maintainer are right that a core split
(`mint_pending`/`start_attempt`) would delete the mirror entirely and is
the cheaper long-term fix — only that, as built, it does not currently
produce the failure mode the charter asked me to hunt for. The one
caveat is forward-looking (L3 above): the safety argument for the
NONE→LIVE write depends on §6.4's replace being unimplemented, and that
assumption needs re-checking the day slice 7 lands.

---

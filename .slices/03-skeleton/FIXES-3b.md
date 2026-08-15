# FIXES-3b — post-slice-3b seam review remediation

Status: **complete — every gate green, nothing committed.**

Files changed: `src/shell/driver.rs`, `tests/spec_shell.rs`.
Files deleted: `tests/probe_cb1.rs`.

Brief: fix confirmed findings from the two blind seam reviews
(`REVIEW-seam-opus.md`, `REVIEW-seam-sonnet.md`), resolved against the
committed probe `tests/probe_cb1.rs`.

Constraints honoured: `src/core/**` frozen; no edits to `SPEC.md`,
`PLAN.md`, `STORIES.md`, `rulings.md`; no ratified constant / header /
frame / timer touched; no `Send` bounds added to the actor path; all
new behaviour tested on tokio's paused clock.

---

## 0. Probe output (before any change)

`tests/probe_cb1.rs`, commit `5fb22cd`, unmodified tree.

```
$ cargo test --all-features --test probe_cb1 -- --nocapture
running 1 test
thread 'cb1_accept_ahead_of_connect_on_one_static' (7243768) panicked at src/shell/driver.rs:731:17:
the shell's static map admitted a connect the core refused: AlreadyConnected
accept  -> Ok("Connection")
connect -> Ok("Connecting")
Connecting resolved -> Err(Elapsed(()))
PROBE SURVIVED — driver did not panic
test cb1_accept_ahead_of_connect_on_one_static ... ok

test result: ok. 1 passed; 0 failed; ...
```

```
$ cargo test --release --all-features --test probe_cb1 -- --nocapture
running 1 test
accept  -> Ok("Connection")
connect -> Ok("Connecting")
Connecting resolved -> Ok(false)
PROBE SURVIVED — driver did not panic
test cb1_accept_ahead_of_connect_on_one_static ... ok
```

Reading of the two runs:

* **Debug** — `debug_assert!(false, …)` at `driver.rs:731` fires, the driver
  task unwinds, `shutdown()` never runs, and the `Connecting` future the
  application is holding **never resolves**: the probe's 120 *virtual*
  second `timeout` elapses (`Err(Elapsed)`). The `LocalSet` swallows the
  panic and the harness prints `ok`. Both F2 and F1, in one run.
* **Release** — `debug_assert!` compiles out, `command_connect`'s `Err`
  branch runs, and the `Connecting` resolves. `Ok(false)` is the probe's
  own `r.map(|x| x.is_ok())` — i.e. `timeout` returned `Ok(_)` (the future
  *resolved*, no hang) carrying `Err(ConnectError::AlreadyConnected)`. That
  is S3a's ratified answer. **The release path is already correct**, which
  is the brief's finding and the probe confirms it.

So Opus's F2 is right and Sonnet's §7 "not currently divergent" verdict is
wrong — but wrong for an instructive reason, recorded in §7 below.

## 1. The false assertion — `src/shell/driver.rs:727-735` (Opus F2)

**Analysis (before the edit).** The assertion says the shell's mirror and
the core's `statics` map cannot disagree. They can, legitimately:

* `Endpoint::connect` (`endpoint.rs:112-122`) tests the **mirror**
  synchronously and writes PENDING. Ruling 87 requires the verb to answer
  without a driver round-trip, so the core has not been consulted.
* `Proven::accept()` queues `Command::AcceptChain` on its first poll and
  yields. If the application then calls `connect()` on the same static
  before the driver's next turn, the queue is
  `[AcceptChain(K), Connect(K)]` and the mirror says NONE for K, because
  the accept has not landed.
* `command_accept_chain` runs first. `core::Endpoint::accept` finds *its*
  map empty for K and succeeds, then the shell writes `Live` for K.
* `command_connect` runs. `core::Endpoint::connect` now finds K present
  and answers `AlreadyConnected` — correctly.

That is the race §16.1 spends a paragraph declining to prevent on the
connect side ("a staged chain in progress is deliberately not in
`connect()`'s list, and cannot be"). The invariant is held at the *other*
end — §6.7's comparison at `accept()` — not here. So the `debug_assert`
encodes an invariant the spec says is not held on this side.

**Change.** Deleted the `debug_assert!(false, …)`; replaced it with a
comment naming the interleaving, citing §16.1 and S3a, and stating that
the `Err` branch beneath is the ratified answer.

**The stamp check, verified rather than assumed.** The worry is that the
losing `connect()`'s `release_static(&static_key, attempt)` in the `Err`
branch would delete the *accept's* newer LIVE entry, leaving the mirror
NONE while the core holds K LIVE — a permanent, silent divergence in the
dangerous direction.

It does not, and here is the arithmetic. `ShellState::next_attempt` is a
single monotone counter (`shared.rs:330`) and both writers draw from it:

| Step | Writer | `next_attempt` before → after | Entry for K |
|---|---|---|---|
| `connect()` | `claim_static` (`shared.rs:337-351`) | *n* → *n*+1 | `{attempt: n, Pending}` |
| `AcceptChain` | `command_accept_chain` (`driver.rs:832-844`) | *n*+1 → *n*+2 | `{attempt: n+1, Live}` — overwrites |
| `Connect` fails | `release_static(&K, n)` (`shared.rs:357-365`) | — | `slot.attempt` is *n*+1 ≠ *n* → **declines** |

`release_static` removes only on `slot.attempt == attempt`. The accept's
entry carries the *later* stamp because it was minted later, so the
connect's stale release always loses. The mirror keeps `{n+1, Live}`,
which is what the core also holds. Verified empirically below by the
regression test `s3a_accept_ahead_of_connect_resolves_already_connected`,
whose second phase does wire-observable work on the accepted connection
after the race and then checks that a *third* `connect()` to the same
static still answers `AlreadyConnected` — which reads the mirror. A
mirror wrongly released to NONE would answer `Ok`.

## 2. Panicking driver must not freeze the endpoint (Opus F1 / Sonnet L1)

**Change.** `Driver::run` no longer calls `shutdown()` on the normal exit.
`shutdown` is renamed `stop`, moved (with `latch`) into a second impl block
carrying only the struct's own bounds — `impl<I: Identity, W: Wire>` — and
called from a new `impl Drop for Driver`. `Drop` runs during unwinding by
construction, so there is now **exactly one** stop path and it covers the
ordinary exit, a panic anywhere in the driver, and a `Driver` future
dropped without ever being polled (a `LocalSet` pulled out from under it).

`Drop` rather than `catch_unwind` for the reason the brief gives:
`catch_unwind` would need `AssertUnwindSafe` over a `!Send`, `!UnwindSafe`
actor holding an `Rc<RefCell<_>>`. **The panic is not swallowed** — it
propagates out of the task exactly as before.

The bounds split is why `latch` moved: a `Drop` impl may not carry bounds
the struct does not, and the existing impl block requires `I: 'static` for
the erased `Rc<dyn ShellLink>`. Nothing in the stop path needs it.

**`stop` had to grow, not just move.** The old `shutdown` closed only two
of the four ways a waiter can be left parked:

| Waiter | Old `shutdown` | Now |
|---|---|---|
| `accept()`, the three staged verbs | resolved — their `oneshot` senders die with the `Driver` | unchanged |
| `Connection::closed()` | `latch(EndpointDropped)` + `wake_all` | unchanged |
| `Connecting` whose `Command::Connect` **was** processed (slot lives in a `ConnRecord`) | **parked for ever** — `conns.clear()` dropped the record's `Rc` without resolving the slot | `Failed(ConnectError::Local)` |
| `Connecting` whose `Command::Connect` is **still in the channel** | **parked for ever** — no record names it; dropping the receiver frees the driver's `Rc` and leaves the handle's copy parked | channel is `close()`d and drained; every `Connect` slot resolved |

`ConnectError::Local` is the right value by ruling 62 — `ConnectError`
deliberately has no `EndpointDropped`, and `Endpoint::connect` already
answers `Local` when it finds `driver_stopped` set (`endpoint.rs:119`).

`stop` is idempotent: `latch` is guarded on `is_none()`, `driver_stopped`
is set-once, and both collections are cleared.

**Verified by** `a_panicking_driver_resolves_every_waiter_instead_of_parking_it`
(`tests/spec_shell.rs`), which arms a `PanicWire` and asserts all four
waiter kinds resolve — with **no clock advance**, using `poll_once`, so the
broken build's answer is `Pending` rather than a slow timeout. It covers
both `Connecting` cases above by issuing two dials back-to-back: the driver
handles one command per loop iteration, so the first gets a record and the
second is still in the channel when the panic lands.

### 2a. Ruling candidate: a failed driver is the reachable cause of `accept() -> None`

**Recorded here, `SPEC.md` untouched.**

§16.2 documents `accept() -> Option<Intro>` with "`None` = endpoint
closed". TEST-B's gap **G9** (`tests/spec_shell.rs` module docs) found that
nothing in the specification closes an endpoint: there is no
`Endpoint::close()`, §16.3's only endpoint-lifetime rule is "dropping every
handle stops the driver", and `accept(&self)` borrows the `Endpoint` — so
the `Endpoint` provably outlives every `accept()` future and the `None` arm
had no constructible cause. Every test in the file `.expect()`s the `Some`
arm and says why.

That is no longer true. **A failed driver is a real, reachable cause**, and
it was already the intended one: `stop()` clears `self.waiting`, and the
comment on that line has always read *"Dropping the senders is what makes a
parked `accept()` resolve `None` — §16.2's 'None = endpoint closed'"*. The
mechanism was built; what was missing was any way to reach it, because the
only path to `stop()` was an exit in which no handle exists to call
`accept()` on. With the `Drop` guard, a panicking driver reaches `stop()`
with handles still alive, and a parked `accept()` resolves `None`.

So the ruling question is not "is `None` unreachable" but **which of the
two readings §16.2 intends**:

1. *"Endpoint closed"* names the driver having stopped for any reason,
   including failure — in which case §16.2's wording is fine and G9's
   objection is answered by this fix, but the phrase deserves a sentence
   saying a driver fault is one of the reasons.
2. `None` was only ever meant for an orderly close that §16.2 forgot to
   give a verb — in which case a failed driver arguably owes the caller
   something more distinguishable than the same `None` an orderly close
   would produce.

Working rule 3 applies: **reported, not resolved.** No spec text changed.
Note also that this is the *third* member of working rule 8's productive
defect class in this area — a stated construction (`None`) with an unstated
scope (what closes an endpoint) — and it is now half-answered by code
rather than by the spec, which is exactly the state a ruling should not be
left in.

## 3. Unbounded growth in `Driver::waiting` (Opus F4 / Sonnet M1)

**Confirmed by inspection and by both reviewers independently** (Opus
measured 133 bytes per cancelled `accept()` against a zero-growth control;
Sonnet reached the same conclusion by reading the gate).

`dispatch_intros`'s prune sits *inside* `while !self.ready.is_empty()`, so
an endpoint that is never dialled never prunes at all, while its sibling
`prune_ready` runs unconditionally from `serve()` every drain.

**Change.** Added `Driver::prune_waiting` — `self.waiting.retain(|reply|
!reply.is_closed())` — and called it from `serve()` on the line after
`prune_ready()`, so the two duals now run on identical terms. The
front-prune inside `dispatch_intros` is **kept**: it is doing a different
job there (never hand an introduction to a dead receiver, which would
`Reject` it out from under a live caller queued behind), and it is the half
that has to be atomic with the `send` beside it.

`is_closed()` is monotone — a dropped receiver stays dropped — so the
`retain` can never discard a caller that is still waiting.

## 4. `deadline()` consuming and discarding core output (Opus F3) — **REAL**

**Verdict: Opus is right and Sonnet is wrong**, and the disagreement is
instructive rather than careless. Sonnet checked the right property of the
wrong thing: it verified that `poll_output()` *at the `Timeout` state* is
idempotent (`Timers::next(&self)` and `Endpoint::deadline(&self)` are pure
`min` scans over `&self` — true, and worth having verified). What it did
not check is whether the cores are *at* `Timeout` when `deadline()` runs.
Opus checked exactly that and found they need not be.

**The window.** `run()`'s loop was `serve() → transmit().await → handles
check → deadline() → select!`. `transmit()` is the only yield between the
drain and `deadline()`, and `Wire::send_to` is an application-supplied
`async fn` — `tokio::net::UdpSocket::send_to` returns `Pending` whenever the
kernel send buffer is full. §16.3 (ruling 53) puts `close()` on the *handle*
side of the seam, mutating the core on the caller's stack. So a `close()`
landing during that yield queues `Transmit(CLOSE)` on a core the driver has
already drained, and `deadline()` — whose `poll_output()` **pops** —
consumed and discarded it.

**Why no existing test could reach it.** `testutil::FlakyWire::send_to`
queues into an in-memory inbox and returns; it never yields. All 451 tests
ride it. This is target 6's shape — code untested by construction rather
than by omission.

**Written first, then fixed** (brief's instruction). The new test
`a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer` needed a
new fixture, `GatedWire`, whose `send_to` can be held `Pending`. Against the
unfixed driver:

```
$ cargo test --all-features --test spec_shell -- a_close_sealed
running 1 test
test a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer ... FAILED

thread '…' panicked at src/shell/driver.rs:647:25:
a connection core queued an output outside a drain
thread '…' panicked at tests/spec_shell.rs:1126:18:
§16.4: a CLOSE sealed while the wire was suspended must still be drained and
sent. … : Elapsed(())

test result: FAILED. 0 passed; 1 failed; …
```

Both halves of the finding in one run: the `debug_assert` fires (debug), and
the peer's `closed()` never resolves because the CLOSE was destroyed
(the cost in release, where the assert compiles out).

**Change.** `deadline()` moved to immediately after `serve()`, before
`transmit().await` — the position its own doc comment already *claimed* it
occupied ("Both queues are empty here — `serve` ran to `Timeout` and every
mutating call since sent a command rather than queuing an output"). The
comment was true of the position, false of the code. No core change; the
frozen `poll_output` is untouched.

A deadline read there cannot go stale in a way that matters: **every**
handle-side core mutation ends by sending a command (`close_now` →
`Dirty`, `Connecting::drop` → `Cancel`, `Endpoint::connect` → `Connect`),
and the command arm is `biased` first, so a mutation during the yield wins
the `select!` immediately and the loop recomputes rather than sleeping on
the stale value. `sleep_until` takes an absolute instant, so a slow send
does not shift it either.

The two `debug_assert!`s in `deadline()` are **kept** — with the call moved,
they now assert something structurally true, and after finding 2 a driver
panic is no longer silent. `deadline()`'s doc comment is rewritten to say
that its correctness is a property of its **caller's position**, not of
itself.

## 5. Regression tests

`tests/probe_cb1.rs` **deleted**. Three tests added to `tests/spec_shell.rs`
in that file's idiom, plus two local `Wire` fixtures and one `Node`
constructor (`Node::spawn_over`, which takes a caller-supplied `Wire` —
`Endpoint<I>` is not parameterised by its wire, so no second type parameter
is needed).

| Test | Finding | Broken build's answer |
|---|---|---|
| `s3a_accept_ahead_of_connect_resolves_already_connected` | 1 | driver panics; `Connecting` answers `Local` (or parks), and the liveness phase finds a dead driver |
| `a_panicking_driver_resolves_every_waiter_instead_of_parking_it` | 2 | all four waiters `Pending`, for ever |
| `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer` | 4 | peer C never sees `PeerClosed`; in debug the driver panics first |

### Working rule 9, applied and *executed*

Each mutation was applied to a working copy, run, and reverted from a
scratchpad backup (never `git checkout` — working rule 10).

**M1 — restore the deleted `debug_assert!` in `command_connect`:**

```
test s3a_accept_ahead_of_connect_resolves_already_connected ... FAILED
thread '…' panicked at src/shell/driver.rs:829:17:
the shell's static map admitted a connect the core refused: AlreadyConnected
thread '…' panicked at tests/spec_shell.rs:1222:17:
the refused attempt must already be resolved: … Still Pending means the driver
never got to it — which is what a driver that panicked on the divergence looks
like from here
```

**M2a — remove the `Drop` guard, restore the normal-exit-only `stop()`:**

```
test a_panicking_driver_resolves_every_waiter_instead_of_parking_it ... FAILED
assertion `left == right` failed: §15.4's endpoint-dropped row: a driver that
died under a live connection must resolve closed(), not park it for ever
  left: Pending
 right: Ready(EndpointDropped)
```

**M2b — keep the guard, drop the command-channel drain:**

```
thread '…' panicked at tests/spec_shell.rs:1402:21:
the attempt still queued in the channel parked for ever: the stop path never
reached its slot
```

**M2c — keep the guard and the drain, do not resolve the records' slots**
(i.e. exactly the old `shutdown`, which cleared `conns` instead):

```
thread '…' panicked at tests/spec_shell.rs:1402:21:
the attempt in flight before the panic parked for ever: the stop path never
reached its slot
```

**M4 — deadline read after the yield:** the pre-fix run in §4 above.

Each mutation is caught by a *different* assertion, so the three
`Connecting` cases in the panic test are genuinely three cases and not one
assertion firing three times.

### The double-panic hazard, checked rather than argued

A `Drop` that panics *during* an unwind aborts the process, so the guard is
only safe if no `RefCell` guard is still live when it runs. The argument is
that unwinding drops inner frames' locals before the outermost — but this
project's rule is that a bound is only a test if the degenerate case
violates it, so it was executed.

The construction: revert fix 4 so the driver panics inside `deadline()`'s
`filter_map` closure, where `let mut cell = record.cell.borrow_mut();` is
**live on the stack**, and `stop()`'s `latch` then takes `borrow_mut()` on
*that same cell*. A temporary assertion checked
`connect(…) == Err(ConnectError::Local)` afterwards — `driver_stopped` is
set by nothing but `stop()`. Result: the assertion **passed**, the run
failed only on its intended peer-C assertion, and cargo reported an ordinary
test failure rather than `SIGABRT`. So the guard runs to completion when the
unwind begins under a live cell borrow. Both temporary edits reverted.

### One fix is *not* pinned by a committed test — finding 3

`prune_waiting` has **no behavioural consequence**: `dispatch_intros`
front-prunes before every delivery, so a build without the fix still
delivers introductions correctly and in order. The only observable is
memory, and observing it needs a `#[global_allocator]`, which would mean a
new test target and a `[[test]]` entry in `Cargo.toml` — beyond this
brief's scope, and this file's convention (G6, G7) is to report such a gap
rather than invent infrastructure for it.

It was measured instead, with a throwaway target since deleted, 20 000
cancelled `accept()`s and a control loop of identical shape without
`accept()`:

```
                          without prune_waiting     with prune_waiting
live bytes before                     433 449                  177 217
live bytes after                    3 079 209                  177 217
growth                              2 645 760                        0
  per cancelled accept()                132.3                        0
CONTROL growth                              0                        0
```

132.3 bytes/cancel reproduces Opus's 133 independently, and the control at
exactly zero rules out the runtime, the timer wheel and the fixture.
**Recommendation for the maintainer:** either accept the gap on the record,
or add a `tests/shell_bounds.rs` target (`required-features =
["test-util"]`) carrying a counting allocator, which would also give §10.6's
other bounds a home.

## 6. Gates

Run on the working tree, in this order, after the last edit. Nothing is
committed.

| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | clean |
| `cargo fmt --all --check` | no diff |
| `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` / `--all-features` | clean, both |
| `cargo test` | 297 + 103 + 11 + 4 + 5 doc = **420**, 0 failed |
| `cargo test --all-features` | 297 + 103 + 11 + 4 + **12** + 4 + 16 + 7 doc = **454**, 0 failed |
| Wire pins (`spec_constants` 103, `spec_packet` 4) | byte-identical, green |
| `cargo +1.96 check --all-features --all-targets` | clean |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |

Baseline was **451** under `--all-features` plus the probe. The probe is
deleted (−1) and three regression tests are added (+3): 451 → 454. **Nothing
existing went red**, and no wire byte moved.

`cargo test --release --all-features` was also run (not a gate, but two of
the three findings had different debug and release behaviour): 454, 0 failed.

## 7. Findings judged not real, or out of scope

* **Sonnet §7 / "Verdict on C-B1: not currently divergent or unsafe" —
  wrong, and instructively so.** It traced the mirror's six writers against
  the core and concluded that the risky one (`command_accept_chain`'s
  unstamped `insert`) can only be reached from NONE, because
  `core::Endpoint::accept()` refuses any static already in **its** map. That
  is true and it checks the wrong direction. The divergence does not come
  from the accept finding the core's map occupied; it comes from the accept
  finding the core's map **empty** — which it legitimately is, because the
  racing `connect()` never touched the core — and then overwriting a mirror
  entry that is *not* empty. Sonnet's own §7 sentence "nothing else can run
  on this single thread between the core's `accept()` call and the mirror
  `insert()` — no interleaving window exists" is correct and irrelevant: the
  window is not inside `command_accept_chain`, it is between the synchronous
  `connect()` and the driver's next turn, which ruling 87 *guarantees* is
  non-empty. Opus's F2 and the probe are right.
* **Sonnet §1 "drain discipline: clean" — wrong**, for the reason given in
  §4 above: it verified `poll_output()` is idempotent *at* `Timeout` without
  checking whether the cores are at `Timeout` when `deadline()` runs.
* **Sonnet L1 severity "low, because it requires a pre-existing core defect
  to trigger" — the premise was wrong.** L1 assumed the only reachable panic
  was the `DRAIN_BOUND` guard. `debug_assert!(false, …)` at
  `driver.rs:731` was reachable from a two-line `tokio::join!`, and so was
  the one in `deadline()`. Sonnet traced the consequence correctly and
  priced it from a trigger inventory it had not completed.
* **Sonnet L3 (the writers table needs a LIVE→LIVE row for slice 7)** —
  forward-looking, correctly scoped to code that does not exist. Not
  actioned; the module docs now carry the *lag* paragraph, which is the
  slice-3b half of the same subject.
* **Sonnet L4 (`release_dead`'s gate depends on a coincidence in the frozen
  core that is not stated at the call site)** — a fair documentation point,
  but the comment it asks for would be in `src/shell/driver.rs` describing
  `src/core/**` behaviour, and neither reviewer found a defect. Left alone:
  not in this brief, and nothing about it changed.
* **Opus F5 (the `Retired`-ordering test asserts only shell-side state)** —
  **real, confirmed, and deliberately not fixed here.** The brief scopes
  this pass to findings 1–4 plus two regression tests. F5 needs a rewritten
  test in `src/shell/mod.rs` that observes the *core* — the shape S29
  already uses — and rewriting a slice-3b acceptance test is a test-authorship
  decision, not a fix. **Flagged as the highest-value remaining item:** it is
  the only test anywhere covering rulings 81/84's linger path, and Opus's
  Appendix B mutation (delete the `handle_connection_event` call at
  `serve_connection`'s `ToEndpoint` arm) leaves it green.
* **Opus F6 (ruling 50's "Nothing is transmitted" has an unstated scope)** —
  real, and a **spec** question. Two independent agents (Opus, and TEST-B as
  gap G10) hit it. Working rule 3: reported, not resolved. `SPEC.md` and
  `rulings.md` untouched.
* **Opus F7 (transmits batched behind events, §16.4's normative ordering)** —
  real as described, and Opus is right that whether §16.4's sentence
  constrains `poll_output()`'s sequence or the shell's *actions* is not
  stated. A ruling question; not a fix. Note it interacts with finding 4's
  change only in that the deadline is now read before the batch is flushed,
  which moves nothing.
* **Opus F8 (`command_cancel` releases the record before delivering
  `Retired`, inverting §16.4's MUST)** — real, harmless today because
  `handle_connection_event` emits nothing, and a two-statement swap. **Not
  done**: it is not in this brief, and swapping it changes no test either
  way (Opus says so, and that is exactly why it should land with a test
  rather than as a drive-by). Recommended for the next pass, together with
  F5.
* **Opus F9 (`serve()`'s outer drain loop exhausts silently, unlike the two
  inner loops)** — real asymmetry. Not in this brief. Worth noting that
  finding 2 changes its cost: adding the `panic!` there would now degrade to
  `EndpointDropped` rather than to silence, so the argument for making it
  loud is stronger than it was.
* **Opus F10 (wakers invoked under a live borrow)** — unproven, and it stays
  unproven: sound with tokio's `LocalSet` waker, unsound with a waker that
  polls inline. Not in this brief. It is cheap to fix (collect, drop the
  guard, wake) and someone should.
* **Opus F11 / IMPL-B's C-B1 core split (`mint_pending` / `start_attempt`)** —
  **explicitly not mine.** `src/core/**` is frozen; the split would delete
  the shell-side mirror entirely and needs a maintainer ruling. Finding 1's
  fix is the minimum change and leaves that decision open — it removes a
  false assertion without entrenching the mirror.
* **Sonnet L2 (no test drives 2+ simultaneous connections on one
  endpoint)** — real coverage gap, and **now partly closed as a side
  effect**: `a_close_sealed_while_the_wire_is_suspended_still_reaches_its_peer`
  is the first test anywhere to put two live connections on one endpoint, so
  `serve()`'s dirty scan, `deadline()`'s `.chain().min()` and
  `release_dead()`'s batch now run at N = 2. `testutil::addr_c()` remains
  unused; the new test uses its own ports.

## 8. What was deliberately *not* touched

`src/core/**`, `SPEC.md`, `PLAN.md`, `STORIES.md`, `rulings.md`. No ratified
constant, header layout, frame type or timer. No `Send` bound anywhere on
the actor path — the two new test wires are `!Send` by construction (they
hold `Rc`s), which incidentally re-fences the invariant from `tests/`. No
crypto touched: neither new wire parses a packet.

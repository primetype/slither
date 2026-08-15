# FIXES-3b round 2 — post-slice-3b seam review, second pass

Baseline: HEAD `9a26c15` on `main`, tree clean, 454 tests green
(`--all-features`), 420 bare.

Scope: the four findings confirmed real and deliberately left at
`9a26c15`. `src/core/**` is frozen; `SPEC.md`, `PLAN.md`, `STORIES.md`,
`rulings.md` are not to be touched.

---

## F5 — the linger test pins nothing

### Two corrections to the brief, before anything else

1. **The test is not under `tests/`.** It is
   `a_closed_connection_frees_its_static_when_the_linger_expires` at
   `src/shell/mod.rs:388-433`, inside `mod tests` — Opus's location is
   right, the brief's is not.
2. **It is not an independent author's work.** That module's own header
   (`src/shell/mod.rs:56-63`) says so in as many words: *"Implementation
   smoke tests — **not** slice 3b's story tests. `tests/story_lifecycle.rs`,
   `tests/story_dial.rs` and `tests/spec_shell.rs` are the acceptance
   criteria and were written independently (working rule 6)."* So this is
   IMPL-B's own smoke test. Rewriting it is not a working-rule-6
   crossing at all, which removes the brief's stated hesitation. It also
   means it is the *implementer* who wrote a test that could not see the
   thing it was named for — the exact failure mode rule 6 exists to
   prevent, showing up in the file rule 6 does not cover.

`grep -rn "linger"` over `tests/` confirms there is no other shell-level
linger test: `tests/story_lifecycle.rs:370`
(`s1_close_resolves_promptly_not_at_linger_expiry`) asserts the
*opposite* end — that `close()` resolves at the seal and **not** after
`CLOSE_LINGER` — and `tests/spec_constants.rs:586` only pins the
constant's value. Opus is right that this is the only test anywhere
covering rulings 81/84's linger path.

### The test as found

```rust
a.close(0, b"").await;
settle().await;

assert!(a.is_established(), "the linger dropped state early");
assert_eq!(… connect(…).err(), Some(ConnectError::AlreadyConnected),
           "the static was freed before the linger expired");

tokio::time::advance(CLOSE_LINGER + 1ms).await;
settle().await;

assert!(!a.is_established(), "the linger never expired");
let redial = … connect(…).expect("`Retired` never reached the endpoint core: …");
drop(redial);          // ← the outcome is never observed
drop(b);
```

The **during-linger** half is not the weak part, and it is worth saying
why, because the fix must not damage it:

* `a.is_established()` reads `ConnCell::core` → `CoreConnection::is_established()`
  → `self.session.is_some()`. That is genuine *connection-core* state.
* Together with the mirror `connect()`, the pair separates a real shell
  mutation: weakening `release_dead`'s gate
  (`driver.rs:569`) from `cell.closed.is_some() && !cell.is_established()`
  to `cell.closed.is_some()` releases the bookkeeping at the death.
  Then `cell.core = None` during the linger — first assertion red — and
  the mirror static is freed — second assertion red. That mutation is
  also the one that drops the connection core *without it ever having
  emitted `Retired`*, so the during-linger half does hold one real edge
  of the MUST.

The **after-linger** half is the weak part, exactly as Opus says. Both
of its assertions are released by `release_dead` (`driver.rs:563-591`)
independently of whether `Retired` was delivered:

* `release_dead` nulls `record.cell.borrow_mut().core` (line 585) → the
  `!a.is_established()` assertion passes either way;
* `release_dead` calls `release_static` on the **mirror** (line 578-581)
  → the handle-side `connect()`'s `claim_static` sees NONE and returns
  `Ok` either way.

Neither reads `core::Endpoint`'s own static map, which is what
`handle_connection_event(Retired)` frees. And `drop(redial)` throws the
one value that would have carried the core's answer.


### The mutation (executed, before)

`src/shell/driver.rs`, `serve_connection`'s `ConnOutput::ToEndpoint` arm
(lines 459-464 at `9a26c15`) — the MUST itself, deleted:

```rust
                ConnOutput::ToEndpoint(event) => {
                    debug_assert!(matches!(event, ToEndpoint::Retired { .. }), …);
-                   // The MUST, discharged here and not later.
-                   self.shell.state.borrow_mut().endpoint
-                       .handle_connection_event(now(), id, event);
+                   // MUTATION (F5 probe): the MUST is not discharged.
+                   let _ = &event;
                }
```

`cargo test --all-features` on that build:

```
test shell::tests::a_closed_connection_frees_its_static_when_the_linger_expires ... ok
test result: ok. 297 passed; 0 failed; …
test result: ok. 103 passed; 0 failed; …
test result: ok. 11 passed; 0 failed; …
test result: ok. 4 passed; 0 failed; …
test result: ok. 12 passed; 0 failed; …
test result: ok. 4 passed; 0 failed; …
test result: ok. 16 passed; 0 failed; …
test result: ok. 7 passed; 0 failed; …
```

**454/454 green.** Worse than Opus reported: it is not that this one
test survives its own stated mutation — **no test in the crate sees
`Retired` being dropped on the floor at all.** The §16.4 MUST was
entirely unpinned.

(Also verified: with the `debug_assert!` at `command_connect`'s `Err`
branch now gone as of finding 1 in `FIXES-3b.md`, the mutated build does
not even panic the driver — Opus's 4a7d28e chain via F1 no longer
applies. The mutation is silent in debug and release alike.)

### What observably differs

Under the mutation the endpoint core keeps three things for the
endpoint's life: the `receiver_index` route, the §17.1 guard-entry pin,
and the **static**. Of those, only the static is reachable from the
shell surface in slice 3 — and only through the *core's* answer to a
later `connect()`, never through the mirror.

I checked whether the linger's *receive* half could be observed
directly, since the brief suggests it: it cannot, in slice 3.
`apply_post_mortem` (`src/core/connection/mod.rs:409-445`) replies only
to an authenticated, window-fresh, **non-CLOSE** packet — an inbound
CLOSE takes the closing side to draining precisely so two closers "go
quiet rather than ping-ponging". The only authenticated packet a slither
peer can emit in slice 3 *is* a CLOSE: there are no stream, message,
datagram, ACK-eliciting or keepalive verbs yet (§16.2, "absent rather
than stubbed"). So nothing can trigger a linger reply, which is exactly
what `tests/spec_shell.rs`'s gap **G6** already records — and it holds
for an in-crate module too, not only for `tests/`. Reported rather than
worked around.

That leaves two degenerate shell builds to separate, and the rewrite
separates both:

| Build | What it does | Which half sees it |
|---|---|---|
| 1 — release at the death | `release_dead`'s gate weakened to `cell.closed.is_some()`: the record is torn down when `close()` seals, dropping the connection core **before** it ever emits `Retired` | first half — `is_established()` must still be `true` during the linger, because §15.2's retention is what lets the linger receive |
| 2 — `Retired` dropped | the mutation above | second half — the core still holds the static, refuses the redial, and **no msg1 is sealed** |

A *core* that emitted `Retired` at the death instead of at the expiry is
a mutation of frozen `src/core/**`; at this seam it is
indistinguishable from build 1, and it is pinned in
`src/core/connection/tests.rs`. Stated in the test's rustdoc rather
than silently ignored.

### The rewrite

`src/shell/mod.rs:388-433` → `388-497`. **What changed, assertion by
assertion:**

* **Unchanged** — `assert!(a.is_established(), "the linger dropped state
  early")` and the during-linger `connect() == Err(AlreadyConnected)`.
  These already separate build 1 (verified by executing it, below), and
  the brief's "change only what is needed" applies.
* **Unchanged** — `advance(CLOSE_LINGER + 1ms)` and
  `assert!(!a.is_established(), "the linger never expired")`. The
  1 ms margin is a tight upper bound on the linger and I did not widen
  it.
* **`drop(redial)` → the redial is driven.** This is the whole fix. Two
  assertions replace the discarded value:
  1. **new** — a tap reading across the redial: at least one datagram
     leaves A. Under mutation 2 the core refuses before anything is
     sealed, so the delta is exactly 0. This one separates without B's
     cooperation, and it is the assertion that fired.
  2. **new** — `redial.await.expect(…)` joined with B's full staged
     accept, then `is_established()` on both. The resolved `Connecting`
     is the *only* carrier of `core::Endpoint::connect`'s answer, which
     is what the old `.expect()` on the **synchronous** `connect()` was
     mistaken for.
* **Moved** — `drop(b)` (and a new `drop(a)`) now happen before the
  redial rather than at the end, so the redial is not racing two dead
  handles; both cores are already released by then.

Deliberately *not* done: no new harness API, no `addr_c`, no second
network, no change to `Pair`. The tap and `settle()` were already there.

### The mutation (executed, after)

Same mutation re-applied to the rewritten test:

```
thread '…a_closed_connection_frees_its_static_when_the_linger_expires' panicked at src/shell/mod.rs:475:13:
no msg1 left the wire: `core::Endpoint::connect` refused the redial, so `Retired` never reached the endpoint core and the static leaked
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 296 filtered out
```

And build 1, executed too — `release_dead`'s gate weakened to
`cell.closed.is_some()`:

```
thread '…a_closed_connection_frees_its_static_when_the_linger_expires' panicked at src/shell/mod.rs:438:13:
the linger dropped state early
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 296 filtered out
```

Both mutations reverted; `git diff --stat src/shell/driver.rs` empty,
and the test is green again on the restored build.

### Status

**Fixed.** `src/shell/mod.rs` only. `src/shell/driver.rs` unchanged by
this finding.

---

## F8 — `command_cancel` releases bookkeeping before delivering `Retired`

### Verification

**Real, and it is the literal inverse.** §16.4, quoted from `SPEC.md`
line 4628-4630:

> In both cases the shell delivers `Retired` to
> `handle_connection_event` **before** releasing the connection's
> shell-side bookkeeping (else the index route and the guard-entry pin
> leak for the endpoint's life).

`command_cancel` at `9a26c15` ran `self.conns.remove(&id)` first and
`handle_connection_event` second. (Note `_record` binds rather than
discards, so the `ConnRecord` *value* survived to the end of the
function — but the removal from `self.conns` is the release that
matters, because every driver-side lookup of an endpoint output goes
through `self.conns.get(&id)`: `serve_endpoint`'s `ToConnection` arm and
`handle_datagram`'s. Both would find `None`.)

Opus's severity call is right too: harmless **today**, for one reason
only — `core::Endpoint::handle_connection_event`
(`src/core/endpoint/mod.rs:808-832`) has a single arm and emits nothing.
Read it and it is a pure mutation of `indices`, `statics` and `guard`.
So nothing can be looked up against the record that has just gone.

One thing neither reviewer said, and it is working rule 8's shape:
§16.4's ordering sentence begins *"In both cases"*, referring to the two
`Retired`-carrying paths it has just listed. `command_cancel`'s
`Retired` is a **third** thing — one the *shell* synthesises for ruling
50's cancel, which §16.4 does not list at all (§16.4 has no cancel verb;
that is why the shell reaches for `Retired`). So the sentence's scope
does not literally reach this call site. The rationale does — the
route and the pin are the same route and pin — so the ordering is held
here rather than argued away, and the code now says so. **No spec
change requested**; noted as an observation.

### The fix

`src/shell/driver.rs`, `command_cancel`. The removal became a
membership test, so the early return is unchanged and the record is
present for the whole of the call:

```rust
-       let Some(_record) = self.conns.remove(&id) else {
-           return;
-       };
+       if !self.conns.contains_key(&id) {
+           return;
+       }
        self.shell.state.borrow_mut().endpoint
            .handle_connection_event(now(), id, ToEndpoint::Retired { our_index: 0 });
+       self.conns.remove(&id);
```

plus a rustdoc section recording the MUST, why the old order was
harmless, and the slice-7 case (§16.1's PENDING-branch tie-break at
`accept()`) that would have made it a defect.

**Not pinned by a test, and cannot be in this slice.** The fix is
observable only through output that `handle_connection_event` does not
yet produce; any test asserting the new order would have to assert on
the *absence* of a difference, which is rule 9's own anti-pattern.
Stated here rather than left implied — the same disclosure
`FIXES-3b.md` §5 made for its finding 3.

### Status

**Fixed.** No behaviour change today; no test moved.

---

## F9 — `serve()`'s outer drain loop exhausts silently

### The behaviour chosen: **panic**, like both inner loops

`serve_endpoint` and `serve_connection` each end their `DRAIN_BOUND`
loop in a named `panic!`; `serve`'s outer loop fell out of the `for`
and carried straight on. Made consistent by panicking, for two reasons
beyond symmetry:

1. **Falling through is not benign.** The next three statements are
   `release_dead`, `prune_ready`, `prune_waiting`, `dispatch_intros`,
   and then — back in `run` — `self.deadline()`. `deadline()` calls
   `poll_output()`, which **pops**: on a core still holding queued
   output it does not mis-report a deadline, it *destroys* a
   `Transmit`. That is exactly the lost-CLOSE failure `deadline`'s own
   rustdoc and `FIXES-3b.md`'s finding 4 are about. A silent exhaustion
   routes into it.
2. **Panicking is cheaper than it was** when the loop was written.
   `FIXES-3b.md`'s finding 2 gave `Driver` a `Drop` that runs `stop()`
   on an unwind, so a driver panic now degrades to
   `ConnectionLost::EndpointDropped` and `ConnectError::Local` rather
   than to an endpoint that is frozen while still reporting itself
   healthy. `FIXES-3b.md` §7 anticipated exactly this ("the argument for
   making it loud is stronger than it was").

The alternative — demote all three to a `tracing::error!` and carry on —
was rejected: the two inner ones are the ratified shape, and §16.4's
drain contract is a state-machine invariant, not an I/O condition.

### The fix

```rust
        let mut settled = false;
        for _ in 0..DRAIN_BOUND {
            …
            if dirty.is_empty() {
                settled = true;
                break;
            }
            …
        }
        assert!(
            settled,
            "the shell's drain did not settle in {DRAIN_BOUND} passes (§16.4)"
        );
```

`assert!` rather than `panic!` only because the natural exit here is a
`break` rather than a `return`; it is an unconditional panic in release
as in debug, exactly like the other two. Plus a rustdoc section stating
the reasoning.

**Not pinned by a test**, and the same is already true of the two inner
loops: reaching any of the three requires a core that violates §16.4's
drain contract, i.e. a mutation of frozen `src/core/**`. Consistency
with the existing pair is the whole of the claim.

### Status

**Fixed.**

---

## F10 — wakers invoked under a live `RefCell` borrow

### Reachability: **Opus's "unproven" is too kind, and Sonnet's "clean" is wrong**

Opus rated it unexploitable today and flagged it only because the
charter named the target. Sonnet returned a **clean** verdict with an
argument that reads, in full, as: every `borrow()`/`borrow_mut()` in
`connection.rs`, `driver.rs` and `shared.rs` is "taken and released
within one synchronous function, never nested". That is a true property
of *slither's own* call graph and it does not bear on the question —
the third time in this review pair that Sonnet has verified something
true and adjacent. The re-entrant frame is not in this crate. It is
inside `Waker::wake()`, which is **the consumer's executor**.

I did not leave it unproven. Both sites are now reachable from a
committed test, and both were **executed as mutations**:

| Site | Re-entry point | Panic under the old code |
|---|---|---|
| `Driver::latch` → `Wakers::wake_all` | `Connection::closed()` / `poll_closed`, `cell.borrow_mut()` | `RefCell already borrowed` at `src/shell/connection.rs:180` |
| `PendingSlot::resolve` | `Connecting::poll`, `this.slot.borrow_mut()` | `RefCell already borrowed` at `src/shell/endpoint.rs:206` |

The executor used is not exotic: an executor that polls a *ready* task
inline instead of pushing it onto a run queue is an ordinary design, and
nothing in slither's public surface asks a consumer not to write one.
`std::task::Wake` requires `Send + Sync` while every cell being
re-entered is `!Send`, so the test's action reaches the connection
through a thread-local — a detail of how the test is written, not of
what the driver sees: the re-entry is a synchronous call out of
`wake()` either way.

**The consequence is worse than a panic in the caller.** Both panics
land *inside the driver task*, which `spawn_local` swallows (Opus §6.3).
`Driver::drop` then runs `stop()`, so what the application observes is
not a crash but an endpoint that has silently become `EndpointDropped` /
`ConnectError::Local` for ever. Both tests assert on the **positive**
side — that the inline poll completed with the right value — rather than
on "no panic", because "no panic" is not observable here.

One related instance Opus checked and cleared, now closed by
construction as well: `resolve` assigned `self.outcome = outcome`,
**dropping the previous outcome under the borrow**. Opus is right that
every driver-side path takes `record.slot` out first, so the previous
outcome is always `Waiting` — but a `PendingOutcome::Ready` owns a
`Connection`, whose `Drop` takes `ConnCell`'s borrow and can stop the
driver (ruling 88). That was a property of the six call sites, not of
the function.

### The fix

Both sites now hand the dangerous values back and dispose of them with
the borrow released, and both are `#[must_use]` so the safe order is the
only one that compiles quietly:

* `src/shell/shared.rs` — `Wakers::wake_all(&mut self)` → **`Wakers::take_all(&mut self) -> Vec<Waker>`**. One caller.
* `src/shell/shared.rs` — `PendingSlot::resolve` now returns `(PendingOutcome<I>, Option<Waker>)` and is private; the new free function **`resolve_slot(&Rc<RefCell<PendingSlot<I>>>, PendingOutcome<I>)`** is the only entry point and does borrow → drop previous → wake, in that order.
* `src/shell/driver.rs` — `latch` scopes its borrow and wakes after; the six `slot.borrow_mut().resolve(…)` call sites became `resolve_slot(&slot, …)`.

### The tests

Two, both in `src/shell/mod.rs`'s in-crate module, sharing one
`InlineWaker` harness:

* `a_waker_that_polls_inline_does_not_re_enter_a_live_borrow` — parks a
  `closed()` future under the inline waker, then the peer closes.
  Mutation (borrow restored around `wake_all`) →
  `RefCell already borrowed` at `connection.rs:180`, outcome `None`,
  **red**.
* `an_inline_waker_may_re_poll_a_connecting_from_wake` — parks a
  `Connecting` under the same waker and drives §5.5's 90 s give-up, the
  path that reaches `fail_pending` → `resolve_slot`. Mutation (wake
  moved back inside the borrow) → `RefCell already borrowed` at
  `endpoint.rs:206`, outcome `None`, **red**.

Both are `#[tokio::test(start_paused = true)]` on the paused clock; no
sleep anywhere. The give-up test needs one `settle()` **before** the
clock jump so the driver starts the attempt at `T` rather than at
`T + 91 s` — without it the give-up is measured from the far side of
the advance and never fires, which cost me a debugging round and is
worth knowing for any later timer test that polls a future by hand.

### Status

**Fixed, and now pinned.**

---

## Gates

Every one run on the working tree at the end of this pass (not
committed, per the brief).

### `cargo build --all-features --all-targets`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
```

### `cargo fmt --all --check`

Clean — after one `cargo fmt --all` for an over-long `.expect(…)` chain
in the rewritten F5 test.

```
FMT CLEAN
```

### `cargo clippy --all-features --all-targets -- -D warnings`

```
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.73s
```

### `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` and `--all-features`

```
 Documenting slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.41s
   Generated .../target/doc/slither/index.html
--- all-features ---
 Documenting slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.39s
   Generated .../target/doc/slither/index.html
```

### `cargo test` (bare) — **422**, was 420

```
     Running unittests src/lib.rs
test result: ok. 299 passed; 0 failed; …
     Running tests/spec_constants.rs
test result: ok. 103 passed; 0 failed; …
     Running tests/spec_errors.rs
test result: ok. 11 passed; 0 failed; …
     Running tests/spec_packet.rs
test result: ok. 4 passed; 0 failed; …
   Doc-tests slither
test result: ok. 5 passed; 0 failed; …
```

### `cargo test --all-features` — **456**, was 454

```
     Running unittests src/lib.rs
test result: ok. 299 passed; 0 failed; …
     Running tests/spec_constants.rs
test result: ok. 103 passed; 0 failed; …
     Running tests/spec_errors.rs
test result: ok. 11 passed; 0 failed; …
     Running tests/spec_packet.rs
test result: ok. 4 passed; 0 failed; …
     Running tests/spec_shell.rs
test result: ok. 12 passed; 0 failed; …
     Running tests/story_dial.rs
test result: ok. 4 passed; 0 failed; …
     Running tests/story_lifecycle.rs
test result: ok. 16 passed; 0 failed; …
   Doc-tests slither
test result: ok. 7 passed; 0 failed; …
```

+2 is exactly the two new F10 tests. Nothing existing moved, and no
existing count changed.

### `cargo test --release --all-features` — **456**

Same eight lines, same counts (299 / 103 / 11 / 4 / 12 / 4 / 16 / 7).
Run because two earlier findings differed by profile; these do not.

### Wire pins

`cargo test --all-features golden` — 12 passed, byte-identical:
`golden_prologue`, `golden_init_header`, `golden_resp_header`,
`golden_data_header`, `golden_mac1_label`, `golden_mac1`,
`golden_mac1_key`/`mac1_key_matches_the_golden_vector`,
`golden_msg1_payload`, `golden_canonical_static`,
`sizes_match_the_golden_vectors`. `tests/spec_constants.rs` 103/103.
Nothing in this pass touches `src/packet/**`.

### `cargo +1.96 check --all-features --all-targets`

```
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s
```

(`const { RefCell::new(None) }` in the new `thread_local!` and
`Waker::noop()` both compile on 1.96.)

### `cargo deny check`

```
advisories ok, bans ok, licenses ok, sources ok
```

exit code `0`.

---

## Notes, conflicts, and what was deliberately not done

### Two errors in the brief

1. **"The test lives under `tests/`"** — it does not; it is in
   `src/shell/mod.rs`.
2. **"is an independent author's work"** — it is not; that module's
   header says in terms that it is the *implementer's* smoke-test file
   and that the independently-written acceptance tests are the three
   under `tests/`. Both stated up front in the F5 section rather than
   worked around.

### One suggestion in the brief I could not carry out, and why

The brief suggests pinning F5 through §15.2's receive half — *"a route
dropped early is observable from the peer"*. It is not, in slice 3.
`apply_post_mortem` owes a reply only to an authenticated, window-fresh
**non-CLOSE** packet, and the only authenticated packet a slither peer
can emit in this slice is a CLOSE (no stream, message, datagram, ACK or
keepalive verbs exist yet). `tests/spec_shell.rs`'s gap **G6** already
records this for `tests/`; it holds for an in-crate module too. Said
rather than silently substituted.

### `src/core/**` untouched

No fix needed one. The known core split
(`connect()` → `mint_pending`/`start_attempt`) was not approached.

### Not touched

`SPEC.md`, `PLAN.md`, `STORIES.md`, `rulings.md`, `src/core/**`,
`src/packet/**`, `Cargo.toml`. No ratified constant, header layout,
frame type or timer. No `Send` bound anywhere — both new tests are
`!Send` by construction (`Rc`, thread-local), and `InlineWaker` is a
unit struct precisely because `std::task::Wake`'s `Send + Sync` bound
must not reach the cell. No crypto touched. No sleeps: both new tests
are `start_paused = true`.

### Two fixes are not pinned by a test, and cannot be here

**F8** (ordering in `command_cancel`) and **F9** (the drain-bound
panic). Both are unobservable without either a core that emits from
`handle_connection_event` (slice 7) or a core that violates §16.4's
drain contract (a mutation of frozen code). Recorded rather than
papered over, and the two inner drain loops are in the same position
today.

### Files changed

| File | What |
|---|---|
| `src/shell/mod.rs` | F5's rewrite; two new F10 tests plus the shared `InlineWaker` harness |
| `src/shell/driver.rs` | F8's ordering; F9's `settled` assert; F10's `latch` and the six `resolve_slot` call sites |
| `src/shell/shared.rs` | F10's `Wakers::take_all` and `resolve_slot` |

Not committed, per the brief.

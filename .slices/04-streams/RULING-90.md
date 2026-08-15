# Ruling 90 — split `connect()` into `mint_pending` + `start_attempt`, delete the shell mirror

Working log. Appended as I go (working rule 2). Skeleton written before the
first `Read` of any source file.

## 0. Status

- [ ] Read ruling 90 (round 15) in `.spec-v2-clean-slate/rulings.md`
- [ ] Read `.slices/03-skeleton/FIXES-3b.md` finding 1 and `REVIEW-seam-opus.md` C-B1
- [ ] Read `.slices/03-skeleton/IMPLEMENTATION-3b.md` §5 (C-B1)
- [ ] Read `src/core/endpoint/mod.rs` connect verb
- [ ] Read `src/shell/{driver,endpoint,shared}.rs`
- [ ] Split the core verb
- [ ] Delete the mirror
- [ ] Mechanical test call-site updates only
- [ ] DhCounter proof at both verbs
- [ ] Re-run seam review mutation
- [ ] Gates

## 1. The ruling, quoted

## 2. What the mirror is (C-B1)

## 3. Current core `connect()` — what it does, in order

## 4. Current shell call path

## 5. The split as implemented

## 6. Mirror deletion — residual grep

## 7. DH ladder proof

## 8. Cancellation ordering (ruling 50) after the split

## 9. The AcceptChain/Connect race — impossible or handled?

## 10. Findings / conflicts

## 11. Gates

---

# Working notes (appended live)

## 1. The ruling, quoted (rulings.md:1840-1862)

> **Ruling 90 — `core::Endpoint::connect()` splits into `mint_pending` and
> `start_attempt`, in slice 4.** Ruling 87 settled that the shell's
> `connect()` is synchronous, and justified it with "`connect()` performs
> no DH — §6.1's initiator costs are paid when msg1 is built". **That
> sentence describes a core factoring that does not exist**: slice 3a's
> `core::Endpoint::connect()` mints the pending *and* builds msg1, two DH,
> in one call. […] the price is a **shell-side mirror of the static map**,
> a second record of "one connection per static", which is a security
> invariant.
>
> Splitting the core verb deletes the mirror. `mint_pending` costs no DH,
> so the shell may call it synchronously and read the core's own map;
> `start_attempt` builds msg1 on the driver, where §6.2 requires it.

## 2. What the mirror is (C-B1)

From `IMPLEMENTATION-3b.md` §5 C-B1 and `FIXES-3b.md` §1:

- §16.3 (4255–4257) verbatim: *"Nor does it need one: **`connect()`
  performs no DH.** §6.1's initiator costs are paid when msg1 is built, on
  the driver; the verb itself only mints the pending."*
- Slice 2a's frozen `core::Endpoint::connect()` inserts the `StaticEntry`
  **and** calls the private `start_attempt`, which opens the identity and
  writes msg1 (2 DH, pinned by
  `a_dial_costs_two_dh_and_each_retransmit_two_more`).
- Slice 3b therefore kept `ShellState::statics`: a stamped NONE/PENDING/
  LIVE mirror. Design note D3: *"Every entry carries a monotone `attempt`
  stamp and every driver-side removal is stamp-checked, because on a
  paused clock `drop(connecting); ep.connect(same_static)` runs to
  completion **before the driver is scheduled at all**"*.
- The mirror's divergence (opus F2 / C-B1): queue is
  `[AcceptChain(K), Connect(K)]`; mirror says NONE for K at the instant of
  `connect()` because the accept has not landed; `command_accept_chain`
  runs first and the core's map gets K LIVE; `command_connect` then gets
  `AlreadyConnected` from the core. The `debug_assert!` that said this
  could not happen was deleted at `9a26c15`.
- The stamp mechanism exists **only** to keep that mirror honest: the
  losing connect's `release_static(&K, n)` must not delete the accept's
  newer `{n+1, Live}` entry.

## 3. Current core `connect()` — what it does, in order

`src/core/endpoint/mod.rs:362-417`:

1. `key = remote_static.as_ref().to_vec()`
2. **`if self.statics.get(&key).is_some() { return Err(AlreadyConnected) }`** — §16.1's test, on the core's own map
3. `next_connection_id()`, `draw_sub_seed()`, `Mac1Key::derive(&key)` — 0 DH
4. build the `Pending` value (all local)
5. `self.statics.insert(key, StaticEntry { conn, state: Pending, dialled: Some(remote), replacement_basis: None })`
6. `pending.guard_pinned = self.guard.pin(&key, PinKind::KeyHolder)` — §17.1
7. **`self.start_attempt(now, &mut pending)`** — the 2 DH: `Identity::open()`,
   `Handshake::initiator`, `write_msg1`, `frame_init`, `emit(Transmit)`
8. `self.pendings.insert(conn, pending)`
9. `Ok((conn, Connection::connecting(sub_seed)))`

The cut is therefore **between 6 and 7**, and 8 must move above the cut so
that the public `start_attempt(now, conn)` can find the pending by id.
`start_attempt` is also called from `retransmit_pendings`
(`mod.rs:772-787`), which already does `remove` → `start_attempt` →
`insert`; that shape survives unchanged if the private helper keeps its
`&mut Pending<I>` signature and the new public verb is a thin wrapper.

## 4. Current shell call path

- `shell/endpoint.rs:105-139` `Endpoint::connect` — sync; `claim_static`
  (mirror), builds `PendingSlot`, sends `Command::Connect{..., attempt, slot}`.
- `shell/driver.rs:801-894` `command_connect` — calls `core.connect()` (2 DH
  here), builds `ConnCell` + `ConnRecord`, or on `Err` releases the mirror
  stamp and resolves the slot `Failed`.
- `shell/driver.rs:945-963` `command_cancel` — `slot.id` may be `None`.
- `shell/driver.rs:515-559` `establish` — PENDING → LIVE, stamp-checked.
- `shell/driver.rs:569-580` `fail_pending`, `589-616` `release_dead` —
  stamp-checked `release_static`.
- `shell/driver.rs:1009-1024` `command_accept_chain` — NONE → LIVE, mints its
  own stamp off `next_attempt`.
- `shell/endpoint.rs:230-256` `Connecting::drop` — `release_static` then
  `Command::Cancel`.

## 10. Findings / conflicts (running)

### FINDING 1 (BLOCKING) — the split *flips* the accept-vs-connect race

`core::Endpoint::accept()` (`src/core/endpoint/staged.rs:503-506`) guards on
the **same** map `mint_pending` writes:

```rust
if self.statics.get(&peer_key).is_some() {
    self.discard_chain(now, id);
    return Err(AcceptError::Stale);
}
```

Today the mirror keeps the core's map empty until the driver runs, so in the
`[AcceptChain(K), Connect(K)]` interleaving the **accept wins**: the core's
map is empty when `AcceptChain` lands, the accept installs LIVE, and the
`Connect` behind it is refused `AlreadyConnected`.

After the split the synchronous `mint_pending` writes `K = Pending` into the
core's map **at the instant of the `connect()` call**, i.e. *before* the
driver processes the already-queued `AcceptChain`. So `accept()` now hits its
own guard, discards the chain and returns `Stale` — **the connect wins.**

`tests/spec_shell.rs:1250` asserts the opposite:

```rust
let a_to_b = accepted.expect("§6.7: the proven chain wins the race and installs");
```

So the divergence does become *impossible* (one map), but the race's
**winner changes**, and an existing assertion would have to change. Per the
brief that is a stop-and-report. Investigating which outcome the spec
prescribes before touching anything.

### FINDING 1, continued — which outcome does the spec prescribe?

§6.6 step 2 (SPEC.md:1531-1543) names this interleaving **explicitly**:

> The converse does **not** hold — PENDING is not exclusive to this path: a
> chain staged while its static was NONE and accepted after a `connect()`
> made that static PENDING reaches **§6.4's PENDING branch** instead. […]
> it applies §6.7's ordering over the identical pair of statics […] — loser
> cancels its pending and installs as responder, winner keeps its pending,
> refuses the `accept()` with `AcceptError::Stale`, and records the
> candidate's timestamp exactly as step 3's winner side does.

§6.4's PENDING branch (SPEC.md:1407-1448):

> - **The peer's static is smaller** — we would be the tie-break *loser*:
>   the `accept()` **cancels** the pending […] and the accept proceeds as an
>   ordinary fresh install with this endpoint as responder.
> - **Our static is smaller** — we would be the tie-break *winner*:
>   `accept()` returns `AcceptError::Stale`, the pending is **left in
>   place**, and the candidate's timestamp is **recorded** in the guard […]
>
> This is the branch that closes the `read_identity()` → `connect()` →
> `accept()` ordering — the static was NONE when the chain was staged and
> became PENDING before it was accepted […]. Without this branch §16.1's
> one-connection-per-static invariant would be broken by two ordinary API
> calls in the wrong order.

And §6.4's `Stale` rule (SPEC.md:1401-1406): the tie-break-**winner** Stale
is *the one exception* that **keeps** its guard record. `discard_chain`
reverts it.

`core::Endpoint::accept()` (`staged.rs:467-475`) documents that it does
**neither** side of that comparison — LIVE and PENDING both get an
unconditional `Stale` + `discard_chain`, "a knowing, documented boundary —
§6.4's replacement admission is slice 7".

### The key order for the race test's identities — measured, not assumed

```
A(seed 1) = 04 65 90 5a …      B(seed 2) = 04 ac a2 10 …
A < B ? true      tie-break winner is A (the accepting side)
```

So under §6.7, **A is the winner**: `accept()` must return `Stale` and A's
own outbound must complete. `tests/spec_shell.rs:1250` asserts the
opposite — `.expect("§6.7: the proven chain wins the race and installs")` —
and cites §6.7 to justify an outcome §6.7 forbids for this key pair. It
passes today only because the mirror keeps the core's map empty at the
instant `AcceptChain` lands, so `accept()` takes the **NONE** path where no
tie-break applies at all.

**That is the mirror's real cost, and it is worse than "a second record":**
the mirror makes the endpoint answer §6.4's PENDING branch *as if the static
were NONE*, skipping §6.7's tie-break — the unilateral opt-out §6.4:1423-1434
says produces "two distinct sessions, two key sets […] mutually dark until
`DEAD_TIMEOUT`".

So ruling 90's split is not tidiness: it moves the race onto the branch that
owns it. But slice 3a's core answers that branch unconditionally, so the
split as briefed lands on a `Stale` that is *coincidentally* right for these
two keys and wrong in general (and wrong about the guard record either way).

## 5. The split as implemented

**Core** (`src/core/endpoint/mod.rs`):

| Was | Is |
|---|---|
| `pub(crate) fn connect(now, remote, remote_static) -> Result<(ConnectionId, Connection), ConnectError>` | `pub(crate) fn mint_pending(..)` — same signature, **stops before the DH** and inserts the pending |
| `fn start_attempt(&mut self, now, &mut Pending)` (private) | `fn build_attempt(&mut self, now, &mut Pending)` — unchanged body, renamed |
| — | `pub(crate) fn start_attempt(&mut self, now, conn: ConnectionId)` — looks the pending up and calls `build_attempt`; a **no-op for an unknown id** |

`retransmit_pendings` calls `build_attempt`, unchanged. The only new
observable state is the window between the two verbs, in which the pending
exists with `attempted == false`, no index and `next_retransmit == now`.

**Shell:**

- `shell/endpoint.rs::connect` → `mint_pending` + `drain_endpoint()`, then
  `Command::Connect { id, core, remote, remote_static, slot }`.
- `Connecting` carries `id: ConnectionId` (was `static_key` + `attempt`).
- `Connecting::drop` → `handle_connection_event(now, id, Retired{0})` +
  `drain_endpoint()`, then `Command::Cancel(id)`.
- `driver::command_connect` → insert `ConnRecord`, then
  `endpoint.start_attempt(now, id)`. **No `Err` arm** — there is no second
  admission test left to fail.
- `driver::command_cancel(id)` → `self.conns.remove(&id)`. Nothing else.
- `establish`, `fail_pending`, `release_dead`, `command_accept_chain` lose
  their static-map writes entirely.
- `ShellState`: `statics`, `next_attempt`, `claim_static`, `release_static`
  deleted; `StaticSlot` and `StaticState` deleted; `PendingSlot::id`
  deleted (ruling 90 mints the id synchronously, so `Connecting` owns it).
- `ShellState::drain_endpoint()` added: §16.4's drain contract on the handle
  side, for the two 0-DH verbs a handle may now call.

## 6. Mirror deletion — residual grep

```
$ grep -rn "claim_static\|release_static\|next_attempt\|StaticSlot\|ShellState::statics" src/ tests/
(no matches)
```

Confirmed **after** the code compiled without them, not before. The library,
all targets and all tests build clean.

## 7. DH ladder proof — measured, at both verbs

Probed from the **shell**, which is where the split has to hold: the verb is
synchronous, so everything it costs is spent before the first await, and
`start_attempt` cannot have run until this task yields.

```
$ cargo test --all-features --test <scratch> -- --nocapture
after connect() returns, before any await: 0 DH
after the driver ran start_attempt: 2 DH
redial immediately after drop(connecting): Ok, DH now 2
cancel-before-driver: 0 DH, 0 datagrams on the wire
```

- `mint_pending` — **0 DH**. §6.2's "the DH lands on the driver" holds with
  the verb still non-`async`, and now it holds *because of the factoring*
  rather than because of a round-trip.
- `start_attempt` — **2 DH** (`es` + `ss`), on the driver.
- The redial after a cancel is admitted with **no clock advance**, at 0
  further DH, reading the core's own map.

**A behaviour improvement, noted rather than assumed:** a `Connecting`
dropped before the driver ever ran now spends **0 DH and puts 0 datagrams on
the wire**. Before ruling 90, `command_connect` called
`core::Endpoint::connect`, which built and sent msg1 unconditionally (2 DH),
and the `Cancel` behind it undid an attempt the peer had already seen. No
test pinned the old cost; `s29_*` and `cancelled_dial_leaves_the_peer_a_
silent_half_open_session` all stay green.

## 8. Cancellation ordering (ruling 50) after the split

Structural, and through the core's own map. `Connecting::drop` calls
`core::Endpoint::handle_connection_event(now, id, Retired { our_index: 0 })`
**synchronously**, in the same instant the handle dies — 0 DH, so §6.2 does
not require the driver for it — and that call runs `drop_pending`, which
removes the pending, its index, its §17.4 hint and its §17.1 pin, and
**returns the static to NONE in the very map `mint_pending` reads**. So

```text
drop(connecting);                    // releases the static, here
let retry = endpoint.connect(a, k)?; // mint_pending, same map, same instant
```

holds with no clock advance and no driver turn between them — it is one
task's straight-line code against one map, not two commands whose order has
to be argued.

It *had* to move to the drop. A `Retired` left on the driver would be one
command later than the redial's admission test, and the redial would be
answered `AlreadyConnected` by a map still holding the corpse. The mirror
used to absorb exactly that; there is no mirror. `command_cancel` is now
`self.conns.remove(&id)` and nothing else.

§16.4's MUST — `Retired` delivered *before* the shell-side bookkeeping is
released — is preserved and strengthened: the drop delivers `Retired` and
*then* sends the command that releases the record, so the order is the
channel's rather than two adjacent statements'.

Green, unweakened: `s29_cancel_then_immediate_redial`,
`s29_cancelled_train_transmits_nothing_further`,
`s29_after_cancel_the_static_routes_as_none`,
`s29_retry_loop_replaces_rather_than_accumulates` (`tests/story_lifecycle.rs`),
`connect_is_synchronous_so_a_pending_static_refuses_before_any_await` and
`cancelled_dial_leaves_the_peer_a_silent_half_open_session`
(`tests/spec_shell.rs`), `s2_giveup_releases_the_static_for_an_immediate_redial`
(`tests/story_dial.rs`).

## Seam-review mutation, re-run

Deleted the `handle_connection_event` call in `serve_connection`'s
`ConnOutput::ToEndpoint` arm:

```
---- shell::tests::a_closed_connection_frees_its_static_when_the_linger_expires stdout ----
panicked at src/shell/mod.rs:479:18:
`mint_pending` still found the static occupied after the linger:
`Retired` never reached the endpoint core (ruling 90): AlreadyConnected
test result: FAILED. 298 passed; 1 failed
```

Reverted from a file copy (working rule 10 — the tree holds uncommitted
work, so no `git checkout`); 299/299 green again.

Note **where** it now fires. Before ruling 90 the mutation was invisible to
`connect()` — the mirror was freed by `release_dead` whether or not the
`Retired` had landed — and only the resolved `Connecting` reported the
core's answer. Now `connect()` itself is the detector, one line into the
second half of the test.

### The ruling-50 claim, verified by its own mutation

The claim "`Connecting::drop` must retire the pending in the core
**synchronously**" is load-bearing, not stylistic. Mutation: move the
`handle_connection_event(.., Retired)` out of `Connecting::drop` and into
`Driver::command_cancel` — the obvious implementation, and the one the
ruling's text does not warn against.

```
shell::tests::a_cancelled_dial_frees_the_static_with_no_clock_advance
  panicked at src/shell/mod.rs:177: the cancellation is ordered ahead of
  this verb: AlreadyConnected
s29_cancel_then_immediate_redial
  panicked at tests/story_lifecycle.rs:1075: ruling 50 (MUST): ... Got
  Some(AlreadyConnected)
s29_retry_loop_replaces_rather_than_accumulates
  panicked at tests/story_lifecycle.rs:1258: cycle 1: redial refused with
  AlreadyConnected
```

Three tests, in two files, by two authors. Reverted from a file copy;
baseline restored.

## 9. The AcceptChain/Connect race — **impossible**, not merely handled

The two-map divergence is now impossible **by construction**: there is one
map, and both the admission test (`mint_pending`) and every transition read
and write it. `Driver::command_connect` no longer has an `Err` arm, because
there is no second test left to disagree with the first — the compiler
enforces that, not a comment. `grep` shows nothing left of `claim_static`,
`release_static`, `next_attempt` or `StaticSlot`.

What survives is a different question, and it is FINDING 1: **who wins**.
The race is now arbitrated at the instant of the synchronous `connect()`
rather than by the driver's command order, which lands it on §6.4's PENDING
branch. Two unconditional answers, neither of them §6.4's:

| | what `accept()` does on a PENDING static | which §6.4 branch that is |
|---|---|---|
| before ruling 90 (mirror) | takes the **NONE** path and installs; the `Connect` behind it is refused `AlreadyConnected` | the **loser** branch's outcome, unconditionally |
| after ruling 90 | `Stale` + `discard_chain`; the dial proceeds | the **winner** branch's outcome, unconditionally, and without keeping the winner's guard record |
| §6.4 as ratified | compare the two statics (§6.7) and take the branch it selects | — |

Both converge on exactly one connection; neither evaluates the comparison
§6.4 says is "**not** optional on this path […] never over local state".
Ruling 90 does not create that defect — `staged.rs:467-475` already flags
the unconditional `Stale` as a slice-7 boundary — but it **widens** the set
of interleavings that reach it, and it flips this one's observable outcome.

## 10. Findings / conflicts (final)

### FINDING 2 — ruling 90's own rationale has an unstated consequence (working rule 11, again)

"`mint_pending` costs no DH, so the shell may call it synchronously and read
the core's own map" is true and sufficient for `connect()`. It says nothing
about **`Connecting::drop`**, which must *also* become a synchronous core
call or ruling 50's MUST breaks — proved above by mutation, three tests in
two files. The ruling that exists because a rationale named a mechanism that
was not there has a rationale that omits a mechanism that had to be.

Not a defect in the *rule* — the split is right — but worth recording beside
rulings 87 and 89 as the same shape: **a stated construction with an
unstated scope** (working rule 8).

### FINDING 3 — two scope questions ruling 90 does not answer

1. **Is `connect` deleted or kept as a wrapper?** "splits into" reads as
   replaced, and that is what is implemented; §16.4's API list still names
   `connect`, and a list in the spec is read as exhaustive (working rule 8).
   §16.4 needs the two names.
2. **What does `start_attempt` do for an unknown `ConnectionId`?** It has to
   be a no-op, because ruling 50's cancel can retire the pending before the
   `Connect` command is processed. That is now a *reachable* case rather
   than defensive coding, and it is the reason a cancelled-before-the-driver
   dial spends 0 DH.

### Note — `src/shell/mod.rs` was not in the brief's file list

It had to be touched anyway: five doc comments there described the mirror,
and one `expect` message named it. Leaving them would be exactly working
rule 4's failure (the token changed, the rationale still argued the old
position). **Prose only — no assertion, no control flow, no test logic
changed** in that file.

## 11. Gates

| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | clean |
| `cargo fmt --all --check` | no diff |
| `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` / `--all-features` | clean |
| `cargo test` (bare) | **422 / 422** |
| `cargo test --all-features` | **455 / 456** — one red, FINDING 1 |
| `cargo test --release --all-features` | **455 / 456** — the same one |
| `cargo +1.96 check --all-features --all-targets` | passes |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |

The one red is `s3a_accept_ahead_of_connect_resolves_already_connected`,
`tests/spec_shell.rs:1250`:

```
§6.7: the proven chain wins the race and installs: Stale
```

**Not weakened, not touched.** It is a ruling request, per FINDING 1.

---

# Round 2 — the test corrected, and the exposure checked

## 12. FINDING 1 resolved: what the test is now *for*

`s3a_accept_ahead_of_connect_resolves_already_connected` →
**`accept_vs_connect_race_reaches_6_4s_pending_branch`**
(`tests/spec_shell.rs`; the module-doc table updated with it).

**Two of its three original assertions are no longer pinnable by anything,
and the doc comment says so:**

- the two maps disagreeing — there is no second map;
- `release_static`'s stamp check — there are no stamps.

**What it pins now.** The same interleaving, with the arbitration moved:
`mint_pending` writes PENDING at the instant of the synchronous call, while
`AcceptChain` is still queued, so the accept no longer finds NONE — it finds
PENDING and routes to §6.4's PENDING branch, which **the mirror made
unreachable through the shell**. Concretely:

1. the key order is **checked, not assumed** (`a.pk < b.pk`), with an
   explicit instruction to invert the assertions rather than delete the
   check if the seeds ever change — the branch of §6.4 under test depends on
   it;
2. `accept()` → `Err(AcceptError::Stale)` — §6.4's winner side, correct for
   these keys;
3. the winner **keeps its pending** (§6.4) — the dial is not resolved by the
   refusal;
4. an explicitly labelled **INTERIM BOUNDARY** block: neither dial completes,
   because §6.5's hint check and §6.6's internal tie-break are slice 7. The
   doc says this block **must go red when slice 7 lands**, and what to
   replace it with;
5. the **recovery** an application actually has: drop the `Connecting` (which
   frees the static in the core's own map, ruling 50), accept B's next
   retransmission — the session installs and B's dial completes;
6. **S3a at the shell seam**, preserved from the old test: a dial to the now-
   LIVE static returns `AlreadyConnected` before any await;
7. the wire-observable liveness phase, kept: A closes, B's independent handle
   sees `PeerClosed`.

## 13. The mutation, executed — and it says more than expected

The claim "putting the mirror back turns this red" was run, not asserted, as
`core::Endpoint::accept` blind to PENDING (the mirror's effect expressed
inside the core):

```
thread 'accept_vs_connect_race_reaches_6_4s_pending_branch' panicked at
  src/core/endpoint/tables.rs:134:  §16.1: one session per peer static
thread 'accept_vs_connect_race_reaches_6_4s_pending_branch' panicked at
  tests/spec_shell.rs:1319:  §6.4's PENDING branch, winner side … Got Err(EndpointDropped)
```

It does not merely fail the assertion — it detonates `StaticMap::insert`'s
own `debug_assert!`, because the accept installs a LIVE row over the PENDING
row the dial holds. **The state the mirror produced is a state the core will
not represent.** It escaped that assertion only because the mirror kept the
row out of the core's map in the first place. That is independent evidence
for ruling 90's direction, from the core's own invariant rather than from
argument. (Reverted from a file copy; `git diff --stat` clean on
`src/core/endpoint/staged.rs`.)

## 14. Two claims I wrote and then had to withdraw (working rule 9)

I drafted a doc comment claiming three mutations. **Two did not hold**, and I
found out by running them rather than by reading them back:

- **"`Connecting::drop` deferring the retirement to the driver."** Run: this
  test stays **green**. The recovery block needs a fresh `Intro`, which needs
  B to retransmit, which needs a clock advance and a driver turn — so a
  deferred `Retired` lands in time. The claim is deleted and the doc now
  names where ruling 50's ordering *is* pinned (three tests, all verified
  red under that mutation).
- **"A frozen driver."** Cutting transmits wholesale **hangs** the test at
  the *first* `accept()` rather than failing the last phase, so the mutation
  does not isolate what the claim said it did. Working rule 13's fixture
  bound: `FlakyWire` cannot express "this driver freezes after
  establishment". Withdrawn and attributed.

This is the exact failure the coordinator warned about — a test renamed onto
new ground while pinning less than its name claims — caught only because the
claims were executed.

## 15. The exposure change, stated plainly and measured

Probe of the same scenario after the split (paused clock, deleted after
measurement):

```
accept()  -> Some(Stale)
connect() -> Ok
after settle:   A dial Pending          B dial Pending
at give-up:     A dial Err(TimedOut)    B dial Err(TimedOut)
a fresh Intro after B's retransmits?    YES
recovery (drop the dial, then accept):  INSTALLED
B dial after A's recovery:              Ok
```

| | before ruling 90 | after |
|---|---|---|
| the accept | takes the **NONE** path and installs | takes the PENDING path, `Stale` |
| which §6.4 branch | the **loser**'s outcome, unconditionally | the **winner**'s outcome, unconditionally |
| initiator role decided by | **command order** | not decided locally at all |
| outcome | one session, immediately | **both dials `TimedOut` at 90 s** unless the application recovers |
| recovery | none needed | drop the `Connecting`, re-accept — measured to install |

### Checking the coordinator's reasoning rather than adopting it

> "both peers keeping their pendings converges on §6.7's deterministic
> tie-break at the handshake level, whereas both installing as responder is
> ruling 35's 'mutually dark' failure."

**Both premises are false in this slice. The verdict survives on other
grounds.**

1. **There is no handshake-level tie-break to converge on.** §6.5's hint
   consultation and §6.6's internal completion are *also* slice 7 —
   `src/core/endpoint/mod.rs:26-40` ("PENDING needs §6.6–6.7's tie-break …
   both are slice 7") and `:247` ("Consultation is §6.5, slice 7"). With no
   internal path, our msg1 is parked by the peer as an ordinary `Intro`, and
   its `accept()` refuses on the same PENDING rule. Measured: **both dials
   `TimedOut`**.
2. **The old behaviour was not "both install as responder".** The mirror's
   install was always paired with a **refusal of the connect**, so no msg1 of
   ours was ever in flight — and §6.4:1426's divergence needs exactly that
   in-flight msg1 ("if we installed as responder here regardless, our own
   in-flight msg1 would still reach the peer"). The old pair converged on one
   session and interoperated with a conformant peer. It is not ruling 35's
   mutually-dark shape.

**Where I land, and why.** I agree the new half is the one to carry, but the
trade is **liveness now for conformance later**, not safe-versus-dark:

- the old behaviour bought its convergence with a rule §6.4 forbids in terms
  — "a two-sided agreement evaluated over the pair of statics, **never over
  local state**" — and was *silently* non-conformant;
- the new behaviour assigns nothing locally, so it cannot disagree with a
  conformant peer; it fails **loudly** (`TimedOut`, both ends) and has a
  measured application-level recovery. Loud beats silent is this project's
  own standing preference;
- and §13's mutation shows the old outcome is not even representable in the
  core's own map.

**But the maintainer should decide with this in front of them:** ruling 90
turns `read_identity() → connect() → accept()` — which §6.4:1436 calls an
ordinary API ordering — from "completes immediately" into "both sides time
out after 90 s unless the application drops its `Connecting` and re-accepts".
That is a real, measured liveness regression the ruling text does not
mention. **Pulling §6.5's hint check and §6.6's internal completion forward
into slice 4 would remove the trade entirely**, and would also let the
INTERIM block in the rewritten test be replaced by the completion it
describes. I do not think unconditional-`Stale` is the *worse* interim, so I
am not asking for ruling 90's sequencing to be reopened — but the sequencing
question is really "when does §6.6 land", not "when does the split land".

## 16. Amendment needed to ruling 90's text (FINDING 2) — for the maintainer

**I have not edited `rulings.md`.** The amendment I believe ruling 90 needs,
stated so it can be pasted or rewritten:

> **Amendment (round 16).** Ruling 90's rationale — "`mint_pending` costs no
> DH, so the shell may call it synchronously and read the core's own map" —
> is true and sufficient for `connect()`, and **silent about
> `Connecting::drop`, which must become a synchronous core call too**. With
> the mirror deleted, a redial's admission test is `mint_pending` reading the
> core's map; a `Retired` still delivered on the driver is one command later
> than that test, and ruling 50's MUST — the cancellation ordered ahead of
> any endpoint verb issued after the drop returns, *with no clock advance* —
> fails. Verified by mutation: deferring it reds
> `a_cancelled_dial_frees_the_static_with_no_clock_advance`,
> `s29_cancel_then_immediate_redial` and
> `s29_retry_loop_replaces_rather_than_accumulates` — three tests, two files,
> two authors. The verb is 0 DH, so §6.2 permits the handle to call it; that
> permission is the mechanism the rationale should have named.
>
> This is rulings 87 and 89's shape — **a stated construction with an
> unstated scope** (working rule 8) — occurring inside the ruling written to
> fix that shape. Ruling 64 recorded that a rationale is not reviewed by the
> act of ratifying its rule; this is the third instance in the maintainer's
> own text, and the first in a ruling whose subject *is* the defect.

Two further scope questions ruling 90 leaves open, also for the maintainer
(implemented as stated, flagged rather than assumed):

1. **`connect` is deleted, not kept as a wrapper.** "Splits into" reads as
   replaced. §16.4's API list still names `connect`; it needs the two names.
2. **`start_attempt` is a no-op for an unknown `ConnectionId`.** It must be:
   ruling 50's cancel can retire the pending before the `Connect` command is
   processed. That is now a *reachable* case, and it is why a dial cancelled
   before the driver runs spends 0 DH and puts nothing on the wire.

## 17. Housekeeping

- `tests/zz_scratch_keyorder.rs` and `tests/zz_probe_race.rs` were mine.
  Both deleted; `ls tests/` shows only the six tracked files.
- `src/core/tests.rs` holds **exactly** the three mechanical call-site
  changes reported, plus a three-line comment on the harness helper saying
  why a dial is now two calls. `git diff src/core/tests.rs` is 18 lines and
  changes **no assertion, no expectation and no test logic**.
- `src/core/endpoint/staged.rs` was mutated and reverted; `git diff --stat`
  on it is empty. `src/core/**` otherwise carries only ruling 90's split.

## 18. Gates — round 2, all green

| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | clean |
| `cargo fmt --all --check` | no diff |
| `cargo clippy --all-features --all-targets -- -D warnings` | zero warnings |
| `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` | clean |
| `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features` | clean |
| `cargo test` | **422 passed, 0 failed** |
| `cargo test --all-features` | **456 passed, 0 failed** |
| `cargo test --release --all-features` | **456 passed, 0 failed** |
| `cargo +1.96 check --all-features --all-targets` | passes |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |

Baseline restored: 456 / 422, the same counts as `da114c5`. No test added,
removed, skipped or weakened; one rewritten, and what it can no longer pin is
named in its own doc comment.

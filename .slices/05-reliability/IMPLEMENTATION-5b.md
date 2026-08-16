# Slice 5b — implementation log

Implementer agent. Base commit `6f5e499`, branch `main`, main tree.

Scope: `SendStream::acked()`, `Connection::acked()` (contract §2.5), ruling
128's post-death drain (contract §2.6).

Owned files: `src/shell/stream.rs`, `src/shell/connection.rs`,
`src/shell/driver.rs`, `src/shell/shared.rs`, `src/core/connection/mod.rs`
(read guard only), commented-out `[[test]]` stanza in `Cargo.toml`.

---

## 1. Reading notes

### What slice 5a already landed (verified by reading, not assumed)

- `core::Connection::ack_snapshot() -> AckSnapshot` (`mod.rs:626`) and
  `snapshot_settled(&AckSnapshot) -> bool` (`mod.rs:642`). `AckSnapshot` is
  `pub(crate) struct AckSnapshot(Vec<(StreamRef, u64)>)` at `mod.rs:1400` —
  **the tuple field is private to `core::connection`**, so the shell can hold
  the value and pass it back but cannot inspect it. Not re-exported from
  `core::mod.rs`; the shell names it `crate::core::connection::AckSnapshot`.
- `Streams::send_offsets()` lists **only halves that are still `Some`**: a
  half freed at `DataRecvd`/`ResetRecvd` disappears from the snapshot.
- `SendHalf::settled_to(offset)` = `reset.is_some() || offset == 0 ||
  acked.covers(0..offset)` — **bytes only, deliberately FIN-free**
  (`send.rs:469`, and `mod.rs:637` says so in terms).
- `ConnEvent::StreamFinished { r }` fires from `Streams::on_ack_range`
  (`streams.rs:481`) exactly when `SendHalf::is_terminal()` — `reset.acked`
  **or** (`fin_acked && acked.covers(0..final_size)`) — and the half is set
  to `None` in the same step.
- `ConnEvent::StreamReset { r, error_code }` fires from
  `Streams::on_reset_stream` (`streams.rs:628`) and touches the **receive**
  half only. See §4-C1.
- Driver: `publish()` ignored `StreamFinished`; `latch()` (`driver.rs:1021`)
  sets `closed` and sweeps `closed_wakers` + `take_all_stream_wakers()`.

### The gap the contract leaves open

Contract §2.5 outcome 2 is *"this half already reached `DataRecvd`"*. **No
core accessor answers that**, and 5b's file ownership forbids adding one
(`streams.rs`/`send.rs` are not mine; `mod.rs` is mine for the `read` guard
only). `snapshot_settled` cannot serve: it is FIN-free by design, and the
snapshot's field is invisible to the shell. So the fact is latched
shell-side from the event the driver already receives — which is what
SPEC.md:4397 calls for anyway (*"the shell translates"*), and what
SPEC.md:4417 describes (*"the core already knows the answer:
`ConnEvent::StreamFinished` … was simply never surfaced"*).

---

## 2. What I built

### 2.1 `SendStream::acked()` — `src/shell/stream.rs`

`pub async fn acked(&mut self) -> Result<(), WriteError>` over
`pub(crate) fn poll_acked(&mut self, cx) -> Poll<…>`, in ruling 124's order
with ruling 135's exception:

1. `LocalEnd::Reset(code)` → `Err(WriteError::Reset(code))`;
2. `cell.finished_senders.contains(&r)` → `Ok(())` — **above** the death
   latch (ruling 135);
3. death latch → `Err(WriteError::ConnectionLost(l))`;
4. otherwise park in `blocked_ackers[r]`.

Never returns `WriteError::Finished`; `LocalEnd::Finished` is deliberately
**not** terminal for this verb (§16.2:4424).

Supporting changes, all in my files:

- `LocalEnd::Reset` now carries the code (`Reset(u64)`). The core cannot
  supply it after the fact: `SendHalf::take_reset` hands the code to the
  frame layer once and the half is freed outright at `ResetRecvd`.
- `SendStream` gains `ack_key`, its slot in `blocked_ackers[r]`, minted in
  `install` beside the writers' slot and released in `Drop`. A second key
  rather than a shared one, so a cancelled `write()` cannot evict a live
  `acked()`'s waker.
- `ConnCell::blocked_ackers: BTreeMap<StreamRef, Wakers>` — the contract's
  type and name, unchanged.
- `ConnCell::finished_senders: BTreeSet<StreamRef>` — the `DataRecvd` latch.
  **Bounded by live `SendStream` handles**: `ConnCell::note_send_finished`
  writes it only while `blocked_ackers` holds that stream's slot (i.e. only
  while a handle that could ask exists), and `SendStream::drop` removes
  both. Without that gate it would grow once per finished stream for the
  connection's life — once per message under §9.8.

### 2.2 `Connection::acked()` — `src/shell/connection.rs`

`pub async fn acked(&self) -> Result<(), ConnectionLost>`. The snapshot is
taken **in the body** (`core.ack_snapshot()`), then `poll_fn` over
`poll_acked(cx, snapshot.as_ref(), key)`, exactly as the contract requires:

1. `snapshot_settled(&snap)` → `Ok(())`, above the latch (ruling 135);
2. latch → `Err(lost)`;
3. core gone with no latch → `debug_assert` + `EndpointDropped`;
4. otherwise park in `settled_wakers`, `closed_wakers`' sibling, with a
   per-future `WakerSlot` (several `acked()` futures can coexist on `&self`).

An empty snapshot is settled vacuously and resolves on the first poll.

### 2.3 The wakeups — `src/shell/driver.rs`, `src/shell/shared.rs`

- `publish`'s `StreamFinished { r }` arm: latch + wake `blocked_ackers[r]` +
  wake `settled_wakers`. Wakers are taken inside the borrow and woken after
  it (finding F10).
- `publish`'s `StreamReset { r, .. }` arm: additionally wakes
  `blocked_ackers[r]` and `settled_wakers`, per the contract. It is a
  **wake, not a verdict** — see §4-C1.
- `ConnCell::take_all_stream_wakers` now sweeps `blocked_ackers` and
  `settled_wakers`, so `Driver::latch` resolves both verbs at the death.
  Ruling 128's *"parking is never permitted on a dead connection"* reaches
  them too.
- **`shared::wake_settled`, called from `Driver::handle_datagram` and from
  `SendStream::reset`/`Drop`** — an addition to the contract's wake list,
  §3-D1.

### 2.4 Ruling 128's post-death drain

`core::Connection::read` (`src/core/connection/mod.rs`): the unconditional
`self.lost` check is replaced by a post-hoc one. `streams.read` runs first;
when `lost` is set, only the `Ok(Some(0))` ("no data available") answer is
converted to `Err(ConnectionLost)` and everything else — data, `Ok(None)`,
`Err(Reset)` — passes through. **No `pump` on the dead path**: nothing can
be emitted anyway (`pump_packets` returns unless the lifecycle is live), and
`pump` also re-derives §13's `Loss`/`Pto` deadlines, which post-death would
announce a deadline the driver schedules and the core then declines to act
on — ruling 141's spin from a new direction.

`RecvStream::poll_read`: own terminal state → empty-buffer short-circuit →
core → latch, never `Pending` with the latch set.

`Connection::poll_accept_with`: `accept(dir)` → latch if `None` → never
`Pending` with the latch set. `core::Connection::accept` needed no change,
as the contract says.

Rustdoc: the drain and its window are documented on `RecvStream::read` and
`Connection::accept_bi`, including ruling 133's no-linger consequence.

### 2.5 `Cargo.toml`

The `[[test]] story_reliability` stanza, **commented out** under an
integration header (ruling 126).

---

## 3. Deviations from the contract

**D1 — an extra wake source for `Connection::acked()`, without which a
ratified §16.2 sentence is false.**

Contract §2.5 says `settled_wakers` is *"woken on **every**
`StreamFinished` and `StreamReset`, and on the latch"*. Those three do not
cover the case §16.2 states in terms: *"Bytes written after the call do not
extend it, so `acked()` terminates on a live connection even while a bulk
stream is still being written."* A stream with no FIN never reaches
`DataRecvd`, so **no `StreamFinished` ever fires for it**, however much of
it is acknowledged — and slice 5a emits no other event on ACK application.
A `write(3000); acked()` with no `finish()` therefore parks for ever under
the contract's list.

I added `shared::wake_settled`, called from `Driver::handle_datagram` (where
§12's ACK is applied) and from `SendStream::reset`/`Drop` (a local reset
settles a snapshot entry, §16.2's *"or abandoned by a reset"*). The
enumeration of what can settle a snapshot entry is written into that
function's rustdoc so a later change re-checks it rather than trusting a
claim about one call site.

This is a **superset** of the contract's wake list: nothing was removed, and
a spurious wake is always safe (the future re-polls and re-parks). It is
still a deviation and it is the one I would most like reviewed.

**D2 — `Connection::acked()` on a cell whose core has already been
released reports `ConnectionLost`, including for an empty snapshot.**

The contract says *"an **empty** snapshot (nothing ever written) resolves
`Ok(())` on the first poll, on a live *or* dead connection."* Once
`Driver::release_dead` has cleared the core there is no snapshot to take and
no way to know whether one would have been empty: `AckSnapshot`'s field is
private to `core::connection` and the type has no `is_empty`, `len` or
`PartialEq`. Rather than guess `Ok(())` — which would report success on a
corpse for the non-empty case, ruling 121's misreport — the verb falls
through to the latch. The divergence is confined to *nothing was ever
written **and** the core is already gone*, where `Err(ConnectionLost)` is
also a defensible answer. Documented at the call site.

**D3 — the `Ok(None)` half of §2.6's core condition is not implemented as
written, because the core cannot express it.**

§2.6: return `ConnectionLost` *"only when `self.lost.is_some()` **and** the
half is gone or has nothing left"*. `Streams::read` returns `Ok(None)` for
**both** "the half is gone" and "the FIN was reached and every byte
delivered" (`streams.rs:407–412` versus the `ReadOutcome::End` arm), and
distinguishing them needs a `Streams` accessor I am not allowed to add. I
pass `Ok(None)` through, because ruling 128 asks for exactly that answer in
terms (*"`read` serves buffered bytes then `Ok(None)`"*) and because the
absent-half case is unreachable from a live `RecvStream`: the handle's
ruling-121 latch (`Ended::Eof`/`Ended::Reset`) answers before the core is
consulted, and the only ways to lose a receive half are a terminal read or
that handle's own `Drop`.

**D4 — the shell re-checks the latch on `Ok(Some(0))`, rather than trusting
the core's guard.**

§2.6 words the conversion as the core's. It cannot be only the core's:
`Driver::stop`/`latch` sets `cell.closed` on **every** cell — on an unwind
as well as an ordinary exit — over cores that are still live and whose
`self.lost` is `None`, and those cores go on answering `Ok(Some(0))`. The
first build I ran carried a `debug_assert!(cell.closed.is_none())` on that
path and the existing test
`shell::tests::a_parked_stream_waiter_resolves_when_the_driver_stops` fired
it immediately. Both guards are now present; the shell's is the load-bearing
one, and its comment says why.

---

## 4. Conflicts — reported, not resolved

**C1 — the contract's `SendStream::acked()` outcome 1 names a peer reset
that this build cannot represent.**

§2.5:1 is *"`local_end == LocalEnd::Reset`, **or the stream was reset by the
peer's §9.8 overflow reset** → `Err(WriteError::Reset(code))`"*, and §2.5:4
wakes on `ConnEvent::StreamReset`. Two facts about the code:

- `Streams::on_reset_stream` applies §9.6's RESET_STREAM to the **receive**
  half only (`streams.rs:611–631`). On a bidi stream it says nothing about
  our send half, so *deciding* `Reset` from that event would report a reset
  the local sender never suffered.
- The one reset a peer can inflict on our send half — §9.8's
  receiver-emitted overflow reset on a uni stream **we** opened — is
  rejected before it is applied: `check_peer_may_send` (`streams.rs:1031`)
  returns `Violation::StreamState` for a RESET_STREAM naming a locally-opened
  uni stream, which kills the connection. §9.8's message machinery is slice
  6's, and the carve-out arrives with it.

So `acked()`'s `Reset(code)` has exactly one producer today: this handle's
own `reset()`. I wake on `StreamReset` as instructed and decide nothing from
it, with the reason recorded at the call site. When slice 6 lands §9.8, the
outcome needs a second producer — most naturally a `ConnEvent` for *our*
send half being reset by the peer, which does not exist.

**C2 — ruling 133's local-close half is unimplemented, unassigned, and one
existing test now fails because of it.** *(the finding of this slice)*

`tests/spec_streams.rs:1146` —
`every_stream_verb_answers_connection_lost_after_the_connection_dies` — fails
at the `accept_uni` row: `ca` closes locally while holding an unclaimed
peer-opened stream, and my ruling-128 `accept_*` hands it over.

Under ruling 128 alone my build is correct: *"while the core still holds a
stream's received state, `accept_*` hands over"*, and it does hold it.
Under ruling 128 **plus** ruling 133 the test is correct: the **closing**
endpoint frees `streams`/`flow` at once, so there would be nothing to hand
over and the latch would answer. The gap is that ruling 133's local-close
half exists in no code:

- 5a declined it deliberately (`IMPLEMENTATION-5a.md` §4-C4 and the comment
  still standing in `drop_state`, `mod.rs:1082–1089`) on the grounds that it
  is entangled with 5b's;
- `CONTRACT-5b.md` §2.6 assigns me the `read` guard and the two shell
  precedence changes and nothing else, and my brief says
  `src/core/connection/mod.rs` — **the `read` guard only**.

I therefore did not implement it. **This needs a ruling, not a patch**, for
two reasons beyond ownership:

1. 5a's C4 stands unanswered — `enter_closing` serves both `close()` and
   `kill()` (a protocol violation), and ruling 133's rationale (*"at the
   closer the application signalled that it is done"*) distinguishes them
   while the code path does not.
2. **Freeing `streams` at local close would silently corrupt
   `Connection::acked()`.** `Streams::send_settled` counts an absent half as
   settled, so with the streams freed, `acked()` after a `close()` answers
   `Ok(())` over bytes that were never acknowledged — ruling 121's misreport
   in the direction ruling 47 exists to prevent. S28's order (`acked()` then
   `close()`) hides it; the reverse order does not. Whoever implements
   ruling 133 must decide what `acked()` says afterwards.

The second failing assertion in that same test — `r.read(…)` reporting
`ConnectionLost` on the **draining** side, `spec_streams.rs:1174` — is a
plain expired premise (ruling 144's category): ruling 128 overturns it, and
the test's own doc comment gives ruling 118's reason for it. It is not
reached today because the `accept_uni` assertion panics first.

**C3 — `src/shell/mod.rs`'s module docs now describe a verb that exists.**
Lines 42–48 say `acked()` is *"absent rather than stubbed … slice 5's
(ruling 122b)"*. That file is not mine. One paragraph to update at
integration.

---

## 5. Mechanisms named that do not exist

- **`CONTRACT-5b.md` §2.5:2, "this half already reached `DataRecvd`"** — no
  core accessor answers it and 5b may not add one; see §1. Latched in the
  shell from `ConnEvent::StreamFinished` instead.
- **`CONTRACT-5b.md` §2.5:1, the peer's §9.8 overflow reset** — §4-C1.
- **`CONTRACT-5b.md` §2.6's line numbers** — `mod.rs:467–469` is `write`'s
  doc comment at this base; `read`'s guard was at `mod.rs:522–524`.
  `accept` is at `mod.rs:461`, not `405–408`. Both moved when 5a landed.
- **`PLAN-5.md` §6.1's blocker list is complete and correct**, checked
  against the code: `connection.rs`'s accept latch, `stream.rs`'s read
  precedence, and `core::Connection::read`'s `lost` guard. Ruling 132's
  correction of ruling 128 holds — nothing in `Retired`, `drop_state` or
  ruling 81 had to move.

---

## 6. Fixture capabilities I needed and could not add

**F-A (the important one) — `LocalSet::run_until` masks every lost wakeup
reachable from a test body.** The body future is re-polled whenever any
local task wakes, so a verb that parks and is *never* woken still resolves
when awaited directly inside `local(async { … })`. I found this by removing
my own `wake_settled` call and watching the test that depends on it pass
anyway. Awaiting the same verb from a `tokio::task::spawn_local` task
separates the two builds immediately — it hangs, then passes with the wake
restored.

This is working rule 13's shape a second time: `FlakyWire` cannot express a
socket fault, and `local()` cannot express a lost wakeup. **Every
wakeup-related test in this repository is currently blind unless it spawns**,
which is worth a fixture helper — `settle_spawned(fut)`, or a `local_task`
wrapper — that 5b was not able to add (`src/testutil/mod.rs` is not mine).

**F-B — 5a's F-2 bites `acked()` exactly as 5a predicted.** A single
ack-eliciting packet is acknowledged only after `MAX_ACK_DELAY`, and
`settle()` advances no virtual time, so `write(b"hello"); finish();
acked()` from a spawned task hangs under `settle()` alone and passes after
`tokio::time::sleep(…)`. It cost me one false bug report against my own
code. A story test that writes a small payload, calls `acked()` and settles
**will hang, and it will look like a defect in `acked()`.** Two packets, or
an explicit clock advance, avoids it.

---

## 7. Verification I ran (deleted before the commit)

Six throwaway tests in `tests/zz_scratch_5b_smoke.rs`, removed before
committing because the acceptance tests are the blind author's (working rule
6). What they established:

| Check | Result |
|---|---|
| `write; finish; acked()` resolves, and the peer really has the bytes | pass |
| `Connection::acked()` with an **empty** snapshot | `Ok(())` on the first poll |
| `Connection::acked()` with **no FIN** anywhere | resolves — and **hangs** with D1's wake removed |
| `SendStream::acked()` before `finish()` | `Pending`, as ruling 139(e) requires |
| both verbs after the peer's CLOSE, fully acked | `Ok(())` — ruling 135 |
| `reset(42)` then `acked()` | `Err(WriteError::Reset(42))` |
| unacknowledged stream on a dead connection | `Err(ConnectionLost)`, never `Pending` |

Both `#[ignore]`d obligations, re-run explicitly:

- `tests/spec_streams.rs::a_receiver_can_drain_a_stream_the_sender_closed_behind`
  — **passes** under this build (`cargo test --test spec_streams -- --ignored`).
  Ruling 128's obligation is discharged; the integrator can delete the
  `#[ignore]`.
- `src/core/connection/tests_streams.rs::credit_frames_precede_the_stream_fill_in_a_packet`
  — **still fails**: *"no packet carried both a credit frame and stream
  data"*, the observed packets being `[MaxStreamData]` and `[Stream …]`
  separately. That test is ruling 130/134's and turns on §8.5's packing,
  which is 5a's work and none of mine; nothing in 5b could move it.

---

## 8. Gate output

Every gate below was run on the committed tree.

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.65s

$ cargo fmt --all --check
(no diff)

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.10s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated target/doc/slither/index.html

$ cargo test
lib 602 ok / spec_constants 103 ok / spec_errors 11 ok / spec_packet 4 ok
doc-tests 5 ok        — all pass (the failing target is behind `test-util`)

$ cargo test --all-features --no-fail-fast
unittests src/lib.rs .......... ok.  602 passed; 0 failed; 1 ignored
tests/spec_constants.rs ....... ok.  103 passed
tests/spec_errors.rs .......... ok.   11 passed
tests/spec_packet.rs .......... ok.    4 passed
tests/spec_shell.rs ........... ok.   12 passed
tests/spec_streams.rs ......... FAILED. 18 passed; 1 failed; 1 ignored
tests/story_dial.rs ........... ok.    4 passed
tests/story_lifecycle.rs ...... ok.   16 passed
tests/story_streams.rs ........ ok.    6 passed
Doc-tests slither ............. ok.    7 passed

  failures:
    every_stream_verb_answers_connection_lost_after_the_connection_dies
    (tests/spec_streams.rs:1152 — the `accept_uni` row)
```

**783 passing, 1 failing, 2 ignored**, against the base's 784 / 0 / 2:
exactly one test flipped, and it is §4-C2. It is reported rather than
"fixed": the file is not mine, and the assertion is not simply expired —
half of it is a live consequence of ruling 133 being unimplemented.

```
$ cargo test --release --all-features --no-fail-fast
identical to the debug run, same single failure — no `debug_assert`-only
divergence between the two profiles

$ cargo +1.96 check --all-features --all-targets   # the declared MSRV
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.08s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

Nine gates green, one test red, and the red is §4-C2 rather than a defect
in this slice.

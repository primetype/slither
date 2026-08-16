# TESTS-5b — story tests for slice 5b (S12, S28)

Blind story-test author, worktree cut from `6f5e499`. Sole owned path:
`tests/story_reliability.rs` — **15 tests**, all
`#[tokio::test(start_paused = true)]`, no `sleep`. Written from
`STORIES.md`, `SPEC.md` §12/§13/§14/§16.2 and `CONTRACT-5b.md`; no line of
the 5b implementation was read (working rule 6).

Parse gate, in place of a compile the worktree cannot do (`acked()` does
not exist here, and stubbing it in `src/` would collide with the
implementer's file — working rule 6's slice-2a accident):

```
$ rustfmt --edition 2024 --check tests/story_reliability.rs && echo "PARSE + FORMAT CLEAN"
PARSE + FORMAT CLEAN
```

**The integrator still owes the `[[test]]` stanza** (working rule 15,
`CONTRACT-5b.md`'s appendix):

```toml
[[test]]
name = "story_reliability"
required-features = ["test-util"]
```

## 1. Inventory

| # | test | story | loss instrument |
|---|---|---|---|
| 1 | `s12_a_bulk_stream_survives_loss_reordering_and_duplication` | S12 | `drop_at` ×6 + `lossy(0.10)` + jitter + duplication, both wires |
| 2 | `s12_a_stream_lost_before_anything_was_acked_is_rescued_by_the_pto` | S12 | `block_path(a→b)`, counter-proved |
| 3 | `s12_a_stream_completes_when_the_acknowledgements_are_lost` | S12 | `block_path(b→a)`, counter-proved |
| 4 | `s12_the_probe_train_backs_off_and_the_transfer_completes_on_heal` | S12 | `block_path(a→b)`, counter-proved, sampled |
| 5 | `s12_a_retransmitting_stream_does_not_starve_its_sibling` | S12/S13 | `drop_at` ×12 |
| 6 | `s28_a_stream_acked_before_the_close_does_not_lose_its_tail` | S28 | `drop_at` ×2 + `lossy(0.20)` |
| 7 | `s28_connection_acked_covers_every_stream_before_the_close` | S28 | `drop_at` ×2 + `lossy(0.10)` |
| 8 | `s28_connection_acked_on_an_empty_snapshot_resolves_at_once_live_and_dead` | S28 | — (ruling 135, cheapest form) |
| 9 | `s28_a_fully_acknowledged_transfer_does_not_report_connection_lost` | S28 | — (ruling 135) |
| 10 | `s28_an_unacknowledged_transfer_on_a_dead_connection_reports_the_loss` | S28 | `block_path(a→b)` |
| 11 | `s28_a_parked_acked_wakes_when_the_connection_dies` | S28 | `block_path(a→b)` |
| 12 | `s28_stream_acked_parks_before_finish_and_resolves_after_it` | S28 | — (ruling 139(e)) |
| 13 | `s28_stream_acked_reports_the_reset_that_abandoned_its_bytes` | S28 | `block_path(a→b)` |
| 14 | `s28_connection_acked_resolves_when_a_stream_in_the_snapshot_is_reset` | S28 | `block_path(a→b)` |
| 15 | `s28_connection_acked_terminates_while_a_bulk_stream_is_still_being_written` | S28 | — (snapshot semantics) |

Mapping to `PLAN-5.md` §7: S12-1→1, S12-2→2, S12-4→4, S12-5→5, S28-1→6,
S28-2→7, S28-3→8, S28-4→15, S28-5→13. **S12-3 is not written** (§4.2);
test 3 takes its place with a deterministic construction. Tests 9, 10, 11,
12 and 14 are additions: 9/10 are ruling 135 from both sides, 11 is the
contract's own waker-list omission (§3, K6), 12 is ruling 139(e), and 14
is `SPEC.md`'s own ruling-47 test obligation
(`SPEC.md:6240–6246`) which §7's table does not carry.

## 2. What each test pins / what the broken build does

Every test carries the same block in its rustdoc; this is the index.

| # | pins | the **broken** build |
|---|---|---|
| 1 | bytes intact, in order, exactly once, EOF sticky, across loss + reorder + duplication | no retransmission → the reader never reaches EOF and `recovering` panics; retransmission at the wrong offset → the offset-derived payload's *content* assertion; a re-delivered range → the *length* assertion, made first so the two never report identically |
| 2 | §13.3 arms the `Pto` on an **empty ack history** | a build that arms `Pto` only as a fallback when a `Loss` timer is absent never probes: nothing was ever acknowledged, so §13.2's ack-driven walk declares nothing and the sender is silent for ever |
| 3 | the sender survives a dead **return** path; the receiver survives a range it already holds | a sender that goes quiet when ACKs stop; a receiver that appends an overlapping retransmission (length); a sender that mis-accounts the late ACK (content); a build that treats a silent return path as a connection error (`closed()` polled) |
| 4 | §13.3's **backoff schedule**, sampled from `Network`'s counters | a build whose `pto_count` resets on its own *sends* rather than on acknowledgement probes at a fixed interval **and still completes the transfer after the heal** — so completion asserts nothing and the interval assertion is the whole test |
| 5 | a retransmitting stream does not starve a sibling | `push_front` where `push_back` belongs (ruling 114's own mutation): A monopolises the sender and the completion instants invert |
| 6 | `write; finish; acked(); close()` keeps its tail | `acked()` resolving at `finish()` — the pre-ruling-47 ordering — closes with two datagrams outstanding, §15.2 frees the recovery state, **the peer's read is short**. The assertion is on the peer's bytes, not on `acked()` returning |
| 7 | the snapshot spans **every** stream | a build snapshotting only the last-written stream resolves a round trip before stream 1's retransmission lands, and stream 1 arrives holed |
| 8 | an empty snapshot resolves on the **first poll**, live *and* dead | a build that parks for a `StreamFinished` that never comes hangs; a build that consults the death latch first fails the dead half. Asserting only the live half passes both |
| 9 | ruling 135 — a settled snapshot outranks the death latch | a build that checks the latch first reports `ConnectionLost` over a fully delivered, fully acknowledged transfer, and the application resends what the peer already has |
| 10 | the latch still stands where the snapshot is **not** settled | a build that answers `Ok(())` on any dead connection passes 9 and fails here. Neither test alone separates both; the pair does |
| 11 | a **parked** `acked()` is woken by the death | a build that wakes `blocked_ackers` only on `StreamFinished`/`StreamReset` — which is exactly what `CONTRACT-5b.md` §2.5 lists — hangs for ever, asleep, with no verb left to poll |
| 12 | ruling 139(e): parks before `finish()`, resolves after | a build that resolves on "all written bytes acknowledged" resolves the first half (the path is perfect and every byte is already acked); a build that never resolves passes that half for the wrong reason and fails the second |
| 13 | §16.2:4425 — a reset before acknowledgement is `Err(Reset(code))` | a build that only ever resolves `Ok` hangs on bytes §9.6 guarantees are never acknowledged; a build reporting `Finished` or `Ok(())` fails the exact `assert_eq!`. Code `0x2a`, never `0`, so an explicit reset cannot be confused with §16.2's drop default |
| 14 | §16.2:4431 — abandonment settles the snapshot, and `StreamReset` wakes it | a build that waits for acknowledgement of abandoned bytes never terminates; a build that wakes `settled_wakers` only on `StreamFinished` leaves the future parked with its condition already true — which is why the future is **held across** the reset rather than re-created after it |
| 15 | §16.2:4433 — snapshot, not quiescence | a build whose snapshot is "every stream's offset, re-read at each poll" never terminates under a writer loop |

### The working-rule-9 discipline, concretely

Three assertions in this file exist **only** to stop a test passing
vacuously, and each one is named in its own message:

* every `drop_at` test asserts `sent_from(&tap, a) > last_dropped` — if
  the wire never reached the window, the drops never happened and the test
  proved nothing about recovery;
* every `block_path` test asserts `blackholed(&pair)` **grew** across the
  window;
* test 15 asserts the background writer's byte counter moved **across**
  the `acked()` call — a parked writer makes "still being written" false
  and a re-read-each-poll build would terminate too.

## 3. Conflicts found — reported, not resolved (working rule 3)

**K1 — `CONTRACT-5b.md` §2.5:71–73 contradicts itself about when
`Connection::acked()`'s snapshot is taken.**

> "Because the snapshot is taken **at the call** and not at the first
> poll, this is written as an `async fn` that takes the snapshot in its
> body and then `poll_fn`s over `poll_acked(cx, &snapshot, key)`."

An `async fn`'s body does not run until the future is **first polled**.
The clause states a requirement and then prescribes the one construction
that cannot meet it. `SPEC.md:4428` sides with the requirement — *"every
byte handed to the connection at the instant of the **call**"* — so
call-time capture looks like the intent, and it needs a non-`async fn`
returning a future built eagerly (snapshot outside the `async` block).
The difference is observable exactly when a caller does
`let f = conn.acked(); write(more).await; f.await` — the shape a
"prepare the shutdown future, then finish up" helper writes naturally.
**Reporting.** Every test here awaits `acked()` in the same expression
that creates it, or holds a future across a mutation that is
**not a write** (test 14 holds one across a *reset*, where both readings
agree), so no assertion depends on the answer.

**K2 — `CONTRACT-5b.md` §2.5's `blocked_ackers` waker list omits the
death latch.** The `SendStream::acked()` park is *"woken by
`ConnEvent::StreamFinished { r }` and by `ConnEvent::StreamReset
{ r, .. }`"*; the `Connection::acked()` park two entries below is *"woken
on **every** `StreamFinished` and `StreamReset`, **and on the latch**"*.
Outcome 3 of the same section is the death latch, so the stream verb
plainly *has* to observe it — but working rule 8 is explicit that a list
is read as exhaustive whether or not it says so, and this one is
contradicted by its own sibling one paragraph away. Taken as written, a
`SendStream::acked()` already parked when the connection dies is woken by
nothing: a permanent hang, in violation of rulings 124/128's *"Parking is
never permitted on a dead connection"*, and in the one direction the
application cannot poll its way out of. **Reporting**, and test 11 is what
the red looks like. This is the same defect *shape* as ruling 71 and
ruling 64 — a stated construction with an unstated scope — in a document
whose §0 says both agents must implement it as written.

**K3 — `PLAN-5.md` §7's S12-2 construction drops nothing.** The row says
*"the same transfer with `drop_first(N)` set **after** `establish()` so
the first N *data* packets are lost outright"*. `drop_first` is absolute:
`send_to` drops when `index < policy.drop_first`, `index` counts from the
**wire's creation**, and `SharedWire::set_policy`'s own doc says *"the
send index is not reset"*. A wire that has just completed a handshake is
already several indices in, so `drop_first(4)` installed afterwards can
drop **nothing at all** — and a test that dropped nothing would pass every
build, including one with no retransmission, while claiming to have made a
hole. Working rule 9's failure with the fixture as the accomplice.
**Reported, and worked around**: test 2 uses `block_path`, which needs no
index arithmetic and is counter-proved; where an index window *is* wanted
(tests 1, 5, 6, 7) `arm_drops` anchors it at the wire's current index,
read from the tap, and asserts afterwards that the wire reached it.

**K4 — `SPEC.md` §13.4's "pending retransmittable frames" has two
readings, and they differ in what a receiver can ever see.** *"Firing PTO
sends one ack-eliciting packet: pending retransmittable frames
oldest-first if any, else a bare PING."* Narrow reading: only frames
already **declared lost** by §13.2 are "pending", so a PTO after a total
ACK outage — where §13.2 has never run, because it is ack-driven — sends a
bare `PING`. Wide reading (RFC 9002 §6.2.4's): unacknowledged data
qualifies, so the PTO retransmits it. Both recover, one RTT apart. What
they decide is whether **"a range the peer already holds arrives again"**
is reachable at all — the receiver-side hazard slice 5 introduces, since
such a range carries a *fresh* packet counter and so passes §7.2's replay
window untouched and lands on §9.5's overlap rule. **Reporting**; test 3
is deliberately written to be agnostic and asserts only what both readings
guarantee.

**K5 — `src/testutil/mod.rs:182–183` says every `FlakyPolicy` field is
public; `:204–210` says one is private.** The struct doc: *"Every field is
public, so a test may build one literally"*. The `failing` field: *"The
shared toggle behind [`FlakyPolicy::fail_sends`]. **Private** and `Rc`, so
clones share it"*. A struct literal — or a functional update
`FlakyPolicy { drop_at, ..base }` — from `tests/` is `E0451`. Harmless
once known, and it cost a rewrite here; `arm_drops` assigns the field
instead. Documentation only, no behaviour at stake. **Reporting** because
the struct doc is what an author reads first.

**K6 — a scope the contract's precedence list does not state: a stream
reset *after* it reached `DataRecvd`.** §2.5 orders outcome 1
(`local_end == Reset` → `Err(Reset)`) above outcome 2 (`DataRecvd` →
`Ok(())`). An application that calls `acked()`, gets `Ok(())`, and then
calls `reset()` — legal, and ruling 129 discusses reset-after-finish as a
real state — makes a *second* `acked()` on the same handle answer
`Err(Reset)` where the first answered `Ok(())`. That is a completed
transfer reported as a failure, which is precisely the shape outcome 2
exists to prevent, and the list does not say which way it should go.
**Reporting, not tested** — a test would pin an unratified reading.

## 4. What could not be tested, and why

### 4.0 The loss instrument — what the fixture can and cannot prove

Working rule 9 demands that a loss-recovery test **prove the loss
happened**; "every byte arrived" passes a build with no loss injected.
Reading `src/testutil/mod.rs` settles what instrument exists.

| fact | line | consequence |
|---|---|---|
| `Network::sends()` counts every `send_to` **before any policy decision** | 517–525 | counts attempts, including drops |
| `Tap` records a send **after** the send-failure and blackhole checks but **before** the loss/duplication draws (`send_to` steps 4→5) | 102–111, 683–701 | **the tap contains datagrams that were then dropped by `loss`** |
| loss/duplication draws happen at step 5, and nothing records their outcome | 690–701 | there is **no drop counter** on `Network` |

**Therefore `sends() − tap.len()` measures blackholes and injected send
failures only — it does not see probabilistic loss.** A test that injects
`FlakyPolicy::lossy(0.10)` and asserts "the bytes arrived" has **no
instrument at all** with which to prove a single datagram was lost. That
is the working-rule-9 trap for this slice, and it is a property of the
fixture, not of the tests (working rule 13).

Two instruments **are** exact, and every lossy test here is built on one
of them:

1. **`FlakyPolicy::drop_at(indices)` / `drop_first(n)`** — index-based,
   *"No RNG is involved"* (200–201, 691). If the wire reaches send index
   `i`, index `i` was dropped, by construction. The residual question is
   only *"did the wire reach that index?"*, and that **is** observable:
   a wire's send index equals the number of tapped datagrams with that
   `src`, provided no partition/blackhole/send-failure is active on it
   (the tap sits above the drop, below the failure checks). So
   `tapped_from(a) > max(indices)` proves every listed drop occurred.
   `arm_drops` and `sent_from` are the two halves.
2. **`Network::block_path(from, to)`** — a blackhole. Sends during the
   block are counted by `sends()` and **absent from the tap** (`send_to`
   step 3 returns before step 4), so the delta pair
   `(Δsends, Δtapped) = (n, 0)` is a *counter-proved* loss of `n`
   datagrams. This is the "`Network`'s counters" instrument, and it is the
   only one where the counters themselves show a drop. `blackholed` reads
   it, and test 4 turns it into a **probe clock** as well.

`FlakyPolicy::lossy(rate)` is still used, for the soak dimension — its
own doc calls it *"reproducible under the network seed, but brittle"*
(281–283) — but **never as the sole source of loss in a test whose
claim is that loss was survived.** Every such test also carries
`drop_at`, so the pin does not rest on a draw.

### 4.1 What the fixture cannot express at all

- **"Drop exactly the ACK."** `FlakyWire` drops by **index** or by
  **draw**, never by **content**: `Spied` is *"deliberately just bytes:
  `testutil` never parses a packet"* (86–91), and the drop decision at
  `send_to` step 5 never looks at `buf`. The nearest expressible thing is
  the one test 3 uses: in a **unidirectional** transfer the receiver sends
  nothing *but* acknowledgements, so `block_path(b → a)` is "drop the
  ACKs" exactly — for as long as the block lasts, and for all of them. A
  test needing *one* ACK dropped and the rest delivered cannot be written
  from `tests/`.
- **"Retransmit a range the peer already holds", on demand.** It follows
  from a lost ACK only under K4's wide reading of §13.4, and from
  reordering only probabilistically (spurious loss detection). It cannot
  be forced. Note that `with_duplication(1.0)` is **not** a substitute: a
  duplicated *datagram* repeats a packet counter and dies at §7.2's replay
  window, while a retransmitted *range* arrives under a fresh counter and
  reaches §9.5. The two exercise different organs, and only the second is
  new in slice 5.
- **Per-wire send counters.** `FlakyWire::sent` is private with no
  getter; `Network::sends()` is global. The tap is the only per-wire
  count, and it is blind to what the drop step did. Test 4's probe clock
  works only because the blocked direction is the *untapped* one and the
  open direction is the *tapped* one, so the two counters can be
  subtracted — a construction that would break the moment both directions
  were blocked.
- **Congestion state.** `cwnd`, `bytes_in_flight`, `Solo`, the
  sent-packet map and the frame stream are in-crate; `tests/*.rs` reaches
  only the public API plus `testutil`, and after establishment
  `Tap::datagrams()` yields AEAD ciphertext. **No frame can be counted
  from here.** No test in this file asserts a `cwnd` trajectory, an ACK
  frame count or a retransmission count; those belong to 5a's in-crate
  `tests_ack.rs` / `tests_recovery.rs`, where the frames exist.

### 4.2 Written, then declined: `PLAN-5.md` §7's S12-3

> *S12-3: reordering only (`with_delay(1 ms, 60 ms)`, no loss); assert the
> transfer completes **and** that the receiver sent at least one immediate
> ACK … The pin is the ACK count against a lower bound derived from the
> gap count.*

**Not written.** The plan's own rule for this section is
`rulings.md:2681–2686` — *not writing a test is the harder call and the
right one* — and three separate confounds make every version of the
assertion either unable to fail a wrong build or able to fail a right
one:

1. **The tap cannot see ACKs.** From `tests/` the only observable is *B's
   datagram count*, and in a unidirectional transfer that is a
   **proxy** for the ACK count, not the ACK count. It also counts every
   other packet B might emit.
2. **The `AckDelay` timer inflates the wrong build.** §12.4 owes an ACK
   after every 2nd ack-eliciting packet **or** when the 25 ms timer fires.
   A build implementing only those two triggers — the build the test is
   meant to fail — emits extra timer ACKs precisely when arrivals are
   spread out, which is what reordering does. The absolute bound
   `acks > N/2` does not separate it.
3. **Heavy jitter changes `N` itself.** 60 ms of jitter on a 1 ms base
   provokes §13.2's *spurious* loss detection, so the reordered run sends
   more data packets than the ordered one — and the every-2nd-only build
   therefore emits more ACKs in the reordered run too. The
   normalised form (`acks_r · sends_o > acks_o · sends_r`) survives that
   one but not confound 2.

§12.4's three triggers are pinned where they are countable: 5a's
in-crate `tests_ack.rs` (A-1 in `PLAN-5.md` §7 asserts both halves —
"after 2, an ACK" *and* "after 1, none until +25 ms"). Adding a
weaker, flakier restatement in `tests/` would not strengthen them, and
*"an assertion a conforming build can fail is a flake, not a pin"*.

Test 3 (`…when_the_acknowledgements_are_lost`) takes S12-3's slot with a
deterministic construction and a counter-proved outage, and pins what the
reordering test was really after — that the sender survives an ACK
outage and the receiver survives what comes back over it.

### 4.3 Other things deliberately not asserted here

- **A `cwnd` trajectory, or any congestion-control number.** Congestion
  control admits more than one conforming output; §14 is 5a's, in-crate.
- **`DEAD_TIMEOUT` ending the probe train** (ruling 33). Test 4 stops
  well short of it and says so; S25 owns it.
- **Ruling 128 for its own sake.** Tests 6 and 7 read a stream *after* the
  sender closed, so they depend on the post-death drain — but
  `tests/spec_streams.rs` has the dedicated test, and `read_to_end`'s
  error arm here names ruling 128 explicitly so a red is not misread as an
  S28 failure.
- **A stream reset *after* `DataRecvd`** — K6; the contract does not say.
- **`send_message(m).await; acked().await; close()`** — S28's literal
  shape is §9.8's, which is slice 6's. `PLAN-5.md` §7 records the
  substitution (a stream stands in for the message); slice 6 re-verifies
  the message form. `SPEC.md:6229–6236` states the obligation in
  `send_message` terms, so **it is not discharged by this file** and
  should not be marked so.

## 5. Notes on the contract

Implemented as written throughout (§0's rule), including where §3 above
says it is wrong. Two of those matter to the implementer today:

- **K2** is the one I would fix before merging: if `blocked_ackers` is
  wired to the two listed events and nothing else, test 11 hangs — and the
  hang is in the application, not the suite, on any real deployment.
- **K1** changes the shape of `Connection::acked`, not just a line: a
  snapshot taken at the call cannot be taken inside an `async fn` body.
  No test here depends on the outcome.

Nothing in `CONTRACT-5b.md` §2.6 (ruling 128's post-death drain) is tested
here beyond what tests 6 and 7 need incidentally; `tests/spec_streams.rs`
and the integrator's `#[ignore]` removals own it.

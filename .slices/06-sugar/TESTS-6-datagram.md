# TESTS-6-datagram — blind test author's log (story S15, §11 datagrams)

Author: blind test author for datagrams, slice 6. Worktree cut from `f19cb99`.
Companion file: `tests/story_datagram.rs` (the only source path this author owns).

Written from `STORIES.md` S15, `.slices/06-sugar/CONTRACT-6.md`, `SPEC.md` §11 /
§10.7 / §8.4 / §18.1, and the rulings named in the brief (148, 152, 155, 156).
**No implementation was read.**

## 1. Inputs read (with what each pinned)

### `SPEC.md` §11 (lines 3499–3560)

- §11.1 — *"no delivery promise, no ordering promise, no retransmission, **no
  sequence identity at all** (applications needing one embed their own)"*.
  Ack-eliciting; congestion-controlled but **loss never retransmits**;
  flow-control-exempt.
- §11.2 — `MAX_DATAGRAM_PAYLOAD` = 1169 = `MAX_PLAINTEXT` − 1 ("a type-`0x30`
  frame's one type byte, data to the end of the plaintext"). *"A datagram never
  spans packets."*
- §11.3 — both queues 64, **by count**, discipline **drop-oldest with the newest
  always accepted**: *"the arriving or newly-sent datagram always enters; the
  oldest queued is evicted to make room"*. Queues, eviction and counters live in
  **the connection core**, not the shell.
- §11.4 — send: `> MAX_DATAGRAM_PAYLOAD` ⇒ `DatagramError::TooLarge` *"at the
  handle, **before any queue**"*. Receive: oversize is unrepresentable; **no
  receiver oversize rule**.
- §11.5 — every overflow drop (send eviction **and** recv eviction) increments a
  counter surfaced on the `slither::frames` trace.

### `.slices/06-sugar/CONTRACT-6.md` (BINDING)

- §2.3 — core `send_datagram(now, &[u8]) -> Result<(), DatagramError>`;
  `recv_datagram() -> Option<Vec<u8>>` **takes no `now`** (ruling 151, §10.7).
  Every-outcome table: lost-first; `TooLarge` queues **nothing** and evicts
  nothing; **empty datagram is `Ok(())` and is queued and sent**; full queue is
  still `Ok(())` with `drops.send += 1`.
- §2.3 — `datagram_drops()` is `#[cfg(test)]` `pub(crate)`, **not public API**
  (§2.7: *"No public accessor for the drop counters"*). ⇒ **`tests/*.rs` cannot
  read a counter.** See §5 below; this is the single biggest constraint on this
  file.
- §2.5 — shell `pub fn send_datagram(&self, data: &[u8]) -> Result<(),
  DatagramError>` — **`&self`, not `async`**, cancel-safety vacuous;
  `pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>`.
  Oversize *"does not mark dirty"*.
- §2.5 precedence for `poll_recv_datagram`: (1) core call → `Ready(Ok)` if
  `Some`; (2) death latch → `Ready(Err(lost))` **only if the core had nothing**;
  (3) `core.is_none()`; (4) park. *"Parking is never permitted on a dead
  connection"* (ruling 128/152).
- §2.4 — `ConnEvent::DatagramReadable` fires **once per DATAGRAM frame admitted
  to the recv queue**, *including* one that evicted an older datagram; never for
  the evicted one; never for a locally-sent one.
- §2.7 — no datagram identity / no `SentFrame::Datagram`; no `datagrams` Stream
  face (slice 8).
- §0 ruling 155 — one datagram per packet, packed **before** the stream fill;
  the `0x30` extends-to-end form is **mandatory** (1169 + type + length >
  `MAX_PLAINTEXT` = 1170).

### Harness (`src/testutil/mod.rs`, slice 0+3, used as shipped)

`Pair::seeded` / `Pair::establish` / `Peer { endpoint, addr, wire, dhs,
public_static }` / `local` / `settle` (64 yields, **no clock advance**) /
`Network::{tap, sends, block_path, heal_path}` / `Tap::snapshot -> Vec<Spied
{ src, dst, bytes }>` / `SharedWire::set_policy` / `FlakyPolicy::{perfect,
drop_at, drop_first, lossy}`. Two facts the tests lean on:

- **the tap sits below the blackhole check and above the loss draw**, so a
  `drop_at` send *is* tapped — which is what makes an index window
  post-hoc-provable (`story_reliability.rs:155-176` says the same);
- `PLAN-6.md` §5.0(b) — delivery is assertable only over `perfect()`;
  never `lossy()` (ruling 148).

## 2. Test inventory

Eleven tests, all `#[tokio::test(start_paused = true)]`, all inside
`local()`, **no `sleep` anywhere**. `rustfmt --edition 2024 --check` is
clean (the parse gate; the file cannot be compiled in this worktree because
the datagram verbs do not exist yet, and **no stub was written** — a stub in
`src/` would collide with the implementer's file, working rule 6).

| # | name | S15 clause / spec |
|---|---|---|
| SD1 | `sd1_send_datagram_never_waits_under_pressure` | *never waits*; §11.3 newest always enters |
| SD2 | `sd2_the_size_bound_is_two_sided_at_the_handle` | *oversize is `TooLarge`*; §11.4 |
| SD3 | `sd3_a_maximum_size_datagram_crosses_intact` | ruling 155, the `0x30` form |
| SD4 | `sd4_oversize_is_an_error_not_a_truncation` | *not a silent truncation*; §11.4 |
| SD5 | `sd5_drop_oldest_under_pressure_still_delivers_the_newest` | *drops oldest under pressure*; §11.3 |
| SD6 | `sd6_a_lost_datagram_is_never_retransmitted_and_never_blocks` | *unreliable and unordered*; §11.1, §8.7 |
| SD7 | `sd7_recv_datagram_is_cancel_safe` | CONTRACT-6 §2.5 |
| SD8 | `sd8_recv_datagram_drains_after_death_and_never_parks` | **ruling 152**, ruling 128 |
| SD9 | `sd9_send_datagram_after_death_is_connection_lost` | CONTRACT-6 §2.5 death row |
| SD10 | `sd10_datagrams_are_flow_control_exempt` | §10.7, §11.1 |
| SD11 | `sd11_a_zero_length_datagram_is_a_datagram` | CONTRACT-6 §2.3 empty row |

Mapping to `PLAN-6.md` §5.1's integration list: SD1←SD1, SD2+SD3+SD4←SD2,
SD5←SD3, SD6←SD4, SD7←SD5, SD8←SD6 (**not** `#[ignore]`d — see §6 conflict
B). SD9/SD10/SD11 are additions; SD10 and SD11 are integration mirrors of
core D9 and D12, and SD9 pins a `CONTRACT-6.md` §2.5 row the plan's list
omits.

## 3. What each test pins, and what the broken build does

**SD1 — never waits.** Blackhole A→B, then 200 `send_datagram` calls with
**no `.await` in the loop**. Two assertions: every call is `Ok(())`, and
`Network::sends()` is *unchanged* across the burst.
*BROKEN BUILD:* an `async fn` or a future-returning verb **does not
compile** against this file — the loudest failure available, and the reason
the loop's shape is itself the test. A shell backed by a bounded channel
that returns an error or parks at 64 fails on call 65. A verb that drove the
wire inline moves `Network::sends()`, because §16.3's driver is a task on
the same current-thread runtime and cannot run without an await — that
second assertion is what separates *synchronous signature* from
*synchronous behaviour*.

**SD2 — the bound, three-sided.** 1168 `Ok`, **1169 `Ok`**, 1170
`Err(TooLarge)`, 1200 `Err(TooLarge)`.
*BROKEN BUILD:* `>=` where §11.4 writes `>` rejects the ratified maximum,
and that build is **invisible** to a test that checks only 1168 and 1170 —
the rejected size is exactly the one ruling 155 says is hardest to send.
A bound written against `MAX_PLAINTEXT` accepts 1170. Slice 1's lesson
(`LEN`/`LEN-1` tested, `LEN+1` not) applied in both directions.

**SD3 — the `0x30` pin.** A 1169-byte datagram is sent over a perfect wire
and must arrive byte-identical.
*BROKEN BUILD:* **a build that emits only `0x31`.** It returns `Ok(())` —
§11.4's size bound is the only admission check — and then never sends the
datagram at all: `1 + varint(1169) + 1169 = 1172 > 1170`, so it never fits
any packet, sits at the head of the queue and is eventually evicted. **The
failure is silent on the wire and silent in the API.** Here it is a loud
`within` timeout. This is the test PLAN §4.2 asks for, and the reason SD2's
`Ok(())` is not enough on its own.
Corroboration (labelled as such in the file): the tap must show at least one
`MAX_DATAGRAM`-sized send from A. Weak — a build that padded every packet to
the MTU passes it for free — so nothing rests on it alone.

**SD4 — error, not truncation.** 1170 bytes of `0xAA` → `Err(TooLarge)`;
then a *different* 1169-byte marker → `Ok`; B's **first** datagram must be
the marker, byte for byte; then `SILENCE` with nothing more.
*BROKEN BUILD:* a shell that clamps with `&data[..MAX_DATAGRAM_PAYLOAD]`
and reports `Err` afterwards delivers 1169 bytes of `0xAA` **first** (the
queue is FIFO), so the assertion fails on *content*, not merely on count —
which is why the two payloads are deliberately distinguishable. A build that
queues the oversize payload and rejects afterwards (the ordering §11.4's
*"before any queue"* forbids) is caught by the silence assertion.

**SD5 — drop-oldest, and the newest still gets out.** 200 × 1169 B sent
synchronously, so no driver turn and therefore **no ACK** can occur during
the burst: §14.5's window is at most `INITIAL_WINDOW` (12 000 B) throughout,
at most ~10 of these 1200-byte packets are admitted, and the other 190
datagrams meet a 64-slot queue. Delivery of more than ~74 is arithmetically
impossible for a conforming build, which is what makes *"fewer than all"* a
pin rather than a hope.
*BROKEN BUILD, two directions separated by two different assertions:* an
**unbounded** send queue delivers all 200 and fails `got.len() < BURST`; a
**reject-newest** queue delivers a prefix and fails `tags.contains(&199)`.
A one-sided "at most 64 arrived" would separate neither cleanly — that is
working rule 9's slice-2a lesson applied. Plus: no tag twice (a build that
re-queued after sending), every payload exactly 1169 B with a body matching
its own tag (a build that shuffled payloads while evicting).
*NOT asserted:* arrival **order** (§11.1 promises none — an order assertion
could fail a conforming build) and *which* datagrams survive (needs the
counters; core D1 owns it).

**SD6 — never retransmitted, never head-of-line blocked.** The one test that
needs a datagram to die. `drop_at(base..base+4)` anchored at A's current
send index, then **three proofs before any conclusion**: A reached the
window (`sent_from` advanced); one of the dropped sends was `MAX_DATAGRAM`
bytes (the max-size datagram's own packet); A did **not** send past the
window, so nothing escaped. Then heal, send tags 3 and 4, and drive 8 s of
virtual time — the un-sampled PTO is ≈1 024 ms and §13.3 doubles it, so
three-plus probes fire.
*BROKEN BUILD:* one that minted a `SentFrame::Datagram` and re-queued on
loss (`CONTRACT-6.md` §2.7's prohibition, made real) delivers tag 2 after
the first PTO. One that gave datagrams a sequence number and reassembled
in order — **the most natural way to get "unordered" wrong** — holds tags 3
and 4 behind the gap for ever and fails by `within`/`drain` starvation.
Without the three proofs, "tag 2 never arrived" would be equally explained
by "tag 2 was never sent": the test would pin nothing. That is ruling 148's
lesson generalised beyond `lossy()`.

**SD7 — cancel safety.** Park a claim, let a datagram arrive *while the
future is not polled*, then drop the future, then claim again.
*BROKEN BUILD:* a `poll_recv_datagram` that claimed into the `WakerSlot` on
the wake path and dropped the claim with the slot loses the payload; the
second claim parks for ever.
*STATED LIMIT (in the test's own rustdoc):* this is **not** a wakeup test.
`LocalSet::run_until` re-polls its body on any local-task wake, so the final
claim would resolve even if the shell registered no waker at all. Whether a
parked reader is woken by `ConnEvent::DatagramReadable` is not observable
from inside the `local()` body. Left as a stated limit rather than dressed
up as a pin.

**SD8 — ruling 152's post-death drain, and never a park.** Three datagrams
delivered and left unclaimed; A closes; **B's death is proved first** with a
single poll of `closed()`; then the three claims and the terminal error, all
four by `poll_once`, never by `await`.
*BROKEN BUILD:* the one `CONTRACT-6.md` §2.5 predicts by name — an author
reading §16.2's post-death paragraph, which lists only `read` and
`accept_*`, checks the death latch first and returns `Err(ConnectionLost)`
with three delivered datagrams still queued. Fails on the very first claim.
A build that parks on a dead connection (ruling 128's defect) returns
`Pending` and fails on whichever row it reaches first. **`poll_once` rather
than `await` is load-bearing here**: an `await` would pass on a build that
parked and was then woken by the LocalSet's re-poll, which is precisely the
defect the ruling forbids.
The datagrams are settled *before* the close on purpose: §8.5 packs control
frames ahead of the DATAGRAM fill, so a coalesced CLOSE would sit ahead of a
datagram in the same packet and the test would be measuring frame-application
order instead of the drain.

**SD9 — a dead connection refuses a send.** Both directions of death
(locally closed; peer-closed). *BROKEN BUILD:* one that queues into a core
that will never pump again returns `Ok(())` — a delivery promise the
protocol cannot keep. The third case (oversize **on** a dead connection) is
deliberately written to accept **either** error: see §6 conflict A.

**SD10 — §10.7 flow-control exemption.** A seals more than
`INITIAL_MAX_DATA` (1 MiB) of datagram payload — **measured from the tap,
not from the receiver**, so the precondition does not depend on delivery at
all — then a uni stream must still move 64 KiB.
*BROKEN BUILD, caught twice:* one charging datagram bytes against **send**
credit stalls before sealing 1 MiB, exhausts the round budget and fails with
that message (the primary detection — it never reaches the stream); one
charging on the **receive** side stops granting MAX_DATA and the stream
write stalls. §10.7's whole purpose is that this wedge is otherwise
**silent** — no error, no event, just a connection that stops.

**SD11 — the degenerate payload.** `send_datagram(&[])` → `Ok`, and B must
receive `Some(vec![])`; then a second, non-empty datagram must also arrive.
*BROKEN BUILD:* one that treats empty as a no-op delivers nothing; one that
collapses "empty datagram" onto the core's `None` — the same inversion that
hangs a reader on a finished stream (working rule 14's slice-4a incident) —
parks the receiver for ever. The **second** datagram separates *"the empty
one was discarded"* (first claim fails only) from *"the queue is wedged"*
(both fail).

## 4. Delivery-dependence audit (working-rule-9 trap 1)

*"Datagrams are unreliable by contract, so 'the datagram arrived' is a
hazardous assertion."* Stated per test:

| test | asserts an arrival? | on what fixture |
|---|---|---|
| SD1 | **no** — nothing is asserted to arrive; the path is a blackhole | n/a |
| SD2 | **no** — handle-side only, nothing awaited | n/a |
| SD3 | yes | `perfect()`, no loss injected anywhere |
| SD4 | yes, plus an assertion that something *does not* arrive | `perfect()`; the non-arrival is of a payload the API **refused**, so it is a claim about the sender, not about the wire |
| SD5 | yes (the last datagram of the burst) | `perfect()`; loss is never injected, the shedding is the *queue's*, not the fabric's |
| SD6 | yes (tags 0, 1, 3, 4) | `perfect()` **at the moment each crosses**; the injected drop is confined to a four-index window proved closed before 3 and 4 are sent |
| SD7 | yes | `perfect()` |
| SD8 | yes (three, pre-death) | `perfect()` |
| SD9 | **no** | n/a |
| SD10 | yes (the *stream's* bytes — reliable by contract) | `perfect()`; the datagram half asserts only what A **sealed**, read from the tap |
| SD11 | yes | `perfect()` |

**No test asserts an arrival across an active loss injection.** The
justification for the arrivals that are asserted is `PLAN-6.md` §5.0(b):
`testutil` is deterministic by MUST (ruling 60), so over `perfect()` *"what
was sealed is delivered"* is a fact and not a probability.

## 5. Counter-visibility audit (ruling 148 / trap 2)

- **`FlakyPolicy::lossy` appears nowhere in this file.** Ruling 148: the tap
  is written above the loss draw, so `sends() − tap.len()` sees blackholes
  and injected send failures only, and a rate-based test cannot prove a
  single datagram died.
- The only test that needs a death (SD6) uses **`drop_at`** — index-based,
  no RNG draw — and proves the window was **reached**, that a
  `MAX_DATAGRAM`-sized send fell **inside** it, and that A did **not** send
  past it. Three assertions, because the first alone would leave "tag 2 was
  never sent" as an equally good explanation of its absence.
- SD1 uses `block_path`, whose casualties are counter-proved by
  `sends() − tap.len()` — but SD1 asserts nothing about loss, only that the
  send queue is the sole sink.

## 6. Conflicts found (working rule 3 — reported, NOT resolved)

### A. `send_datagram`'s precedence: oversize vs. the death latch

Two rows of the same binding document give different answers for one input
(an oversize payload on a dead connection):

- `CONTRACT-6.md:171` — core table: connection already lost →
  `Err(ConnectionLost)`, *"checked **first**, mirroring `write`
  (`mod.rs:472`, `self.lost()?`)"*.
- `CONTRACT-6.md:254-259` — shell table: four rows, **unordered**, with
  overlapping guards (`payload > 1169` and `death latch set` are both true).
- `CONTRACT-6.md:278-287` — the *adjacent* `poll_send_message` precedence is
  explicitly ordered and puts `TooLarge` at **step 1**, the death latch at
  step 2, citing §9.8:3088 *"rejected at the handle"*. §11.4 uses the same
  phrase — *"at the handle, before any queue"* — for datagrams.

So the core says lost-first and the shell's nearest analogue says
size-first, and `send_datagram`'s own shell row set is silent on order.
**SD9 asserts only that it is an `Err` of one of the two kinds** and says so
in its rustdoc. Writing it to either side would be an assertion a conforming
build could fail — a flake, not a pin. **This needs a ruling**, and the
answer should also say whether the *core* and the *shell* are required to
agree (the shell can return `TooLarge` without ever calling the core).

This is exactly working rule 8's shape: a stated construction (§11.4's
"at the handle, before any queue") with unstated scope (before *which* other
checks).

### B. Q5 is answered by ruling 152, but three places still say it is open

- `CONTRACT-6.md:18` (§0, ruling 152) — *"The post-death drain covers
  `recv_message` and `recv_datagram`… Same precedence table; **parking is
  still never permitted on a dead connection**."* §0's own banner says these
  rulings **override anything below**.
- `CONTRACT-6.md:186` — the `recv_datagram` outcome table still carries
  *"⚠ see §8 Q5"* on the dead-but-non-empty row.
- `CONTRACT-6.md:195` — the `recv_message` row still carries *"⚠ §8 Q5 / §7
  C-2: … requires this and **no ruling says so**"*, which ruling 152 makes
  false.
- `PLAN-6.md:1195` — integration SD6 is *"⚠ gated on Q5. Ship `#[ignore]`d
  with the question named; the integrator un-ignores once ruled."*

I followed §0 (it says it overrides) and shipped **SD8 live, not
`#[ignore]`d**. Reporting rather than silently resolving, per working rule 3:
if the intent really is to ship it ignored, that is a one-line change for the
integrator, but the three stale ⚠ markers should be swept either way — this
is working rule 4's defect (the value changed, the prose arguing the old
position did not).

### C. §11.5's counter is singular; §11.3 and §18.2 say counters

`SPEC.md:3555-3557` — *"increments **a counter** surfaced on the
`slither::frames` trace target (§18.2); **the counters** are core state"* —
singular and plural in one sentence. Already recorded as `PLAN-6.md` §7 C-4
and resolved by ruling 156 (**two**). Noted here only because a test author
reading §11.5 alone would build one counter; no new finding.

## 7. Could not test, and why

1. **Both drop counters (ruling 156).** `datagram_drops()` is `#[cfg(test)]
   pub(crate)` (`CONTRACT-6.md:163-164`) and §2.7 forbids a public accessor.
   `tests/*.rs` cannot see them. Core tests D1/D2/D6/D11 own the exact
   eviction arithmetic; SD5 gets only the weak two-sided form.
2. **Frame identity — no integration test can count frames.** `Tap` yields
   sealed packets. Where a length is a sound proxy (a 1169-byte datagram
   occupies exactly `MAX_DATAGRAM` = 1200 bytes, and its `0x31` form would
   need 1201 and cannot exist) it is used and **labelled a proxy**; it is
   vacuous against a build that padded every packet to the MTU, so nothing
   rests on it alone. The `0x30`-vs-`0x31` distinction proper is core D4's.
3. **A lost wakeup.** `LocalSet::run_until` re-polls its body on any
   local-task wake, so a future awaited from inside `local()` cannot
   distinguish "the shell woke my waker" from "the body was re-polled".
   SD7 is stated as a cancel-safety pin only.
4. **`ConnEvent::DatagramReadable`'s once-per-frame emission scope**
   (`CONTRACT-6.md:220-223`), including the *"fires for a datagram that
   evicted an older one"* clause. It is a `pub(crate)` event; from the public
   API a missing event is indistinguishable from a datagram that has not
   arrived yet. Core/shell-side test.
5. **§11.5's trace obligation.** See §8 item 1 — nothing in this file, and
   as far as `PLAN-6.md` §5.1 goes nothing in the slice, captures a
   `tracing` line. Flagged, not tested.
6. **A send failure or a driver panic**, working rule 13's class: `FlakyWire`
   models what a *network* does, not what a *socket* does. Out of reach by
   construction, as it was for the seam review.
7. **§11.3's byte bound rationale** (64 × 1169 ≈ 73 KiB per queue) — a
   memory-footprint claim, not an observable behaviour.

### Handover — the integrator's one-line job (working rule 15)

`Cargo.toml` has **no `autotests = false`**, but every existing shell test
carries an explicit stanza because `slither::testutil` lives behind the
`test-util` feature:

```toml
[[test]]
name = "story_datagram"
required-features = ["test-util"]
```

Without it `cargo test` compiles `tests/story_datagram.rs` **without**
`test-util` and the target fails to build — `slither::testutil` does not
exist (`src/lib.rs:129-130`). **I did not add it**: `Cargo.toml` is the file
neither blind agent can validly write alone (working rule 15 names this
exact case — slice 4b's manifest, where a stanza pointing at a not-yet-
existing test file makes cargo refuse to parse the manifest and *no gate can
run at all*). It is the integrator's, and it should land in the same commit
as this file and `tests/story_message.rs`.

## 8. Contract items I believe are wrong

### 1. §11.5's trace is the drop counters' *only* public-facing surface, and nothing pins it

`SPEC.md:3553-3560` makes the counters an **operability** obligation: *"A
silent drop is a known operability weakness of the precedent and is
deliberately not copied."* The whole point is that an operator can see
evictions. But `CONTRACT-6.md` §2.7 forbids a public accessor and §2.3 makes
`datagram_drops()` `#[cfg(test)]`, so **the trace line is the only thing a
user of the library ever sees** — and `PLAN-6.md` §5.1's twelve core tests
assert `datagram_drops()`, never a trace line. Result: the counters are
tested, and §11.5's actual deliverable is tested by nothing. A build that
incremented both counters correctly and emitted no trace at all passes every
test in the slice.
Not a contradiction — a **gap**, and of working rule 8's exact shape (a
stated construction, §11.5's trace obligation, with no stated owner). It
needs either a trace-capture test or an explicit "not pinned in v1" note.

### 2. Ruling 155's "one datagram per packet, packed before the stream fill" can starve the send queue behind a maximum-size datagram, and nothing says so

`PLAN-6.md` §4.2's packing algorithm looks only at *"the datagram at the
front of the send queue"* and, if it does not fit, *"leave it queued for the
next packet"* — it never skips to the next entry. §8.5 packs control frames
**first**. So whenever an ACK is pending, `room < MAX_PLAINTEXT`, and a
1169-byte datagram at the head of the queue **cannot fit** — for that packet
or for any packet built while an ACK is pending. Under sustained
bidirectional traffic that is a large fraction of packets, and every
datagram behind the stuck one is evicted by arrivals while it waits.

That is **head-of-line blocking in the explicitly unordered path**, produced
by a rule ("one per packet, from the front") whose scope was chosen for a
different reason (ruling 155 was about starvation *by the stream fill*).
Ruling 155's rationale rejects datagrams-after-the-fill because *"a
saturated stream starves them entirely and the bounded queue evicts
continuously — silent loss with no distinguishing counter"* — and the
front-only rule reproduces a smaller version of exactly that, with exactly
the same silence, whenever the head datagram is near maximum size.

I am **not** proposing the fix (skipping to the next fitting entry would
reorder, which §11.1 permits but nobody ruled on). Reporting it because
ruling 155's rationale names an outcome it does not fully avoid, and working
rule 11 asks that a rationale be checked against what else it has to be true
of. It also bounds my own SD3: that test sends its 1169-byte datagram on an
otherwise idle connection precisely so this cannot make it flake.

### 3. `CONTRACT-6.md` §2.5's `send_datagram` row list is not stated as ordered, and its guards overlap

Covered as conflict A above. The style rule the contract sets for itself
(`CONTRACT-5a.md:35-40`, *"every return value is stated for every state"*)
is met row by row and broken by the combination: two rows are simultaneously
true for one input and nothing says which wins. The neighbouring
`poll_send_message` block gets this right by numbering its steps; the
`send_datagram` block should do the same.

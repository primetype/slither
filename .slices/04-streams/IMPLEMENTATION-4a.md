# Slice 4a — implementation report (IMPL-S)

Status: **COMPLETE.** All gates green (§7). Tree left dirty for integration;
nothing committed.

---

## 0. The two things to read first

1. **§3.1 — the round-robin/packing disagreement with the test author.**
   Two of their 73 tests fail against my build, and both trace to one
   unstated question: **does a mutating verb flush everything it can, or
   does the core assemble packets later?** §16.7 says the first; their tests
   assume the second. This needs a ruling, not a patch.
2. **§4.2 — three of their tests are fixture defects, not implementation
   defects**: they deliver STREAM frames whose *plaintext* exceeds
   `MAX_PLAINTEXT`, which §3.1's size gate drops silently before the frame
   layer sees them. Verified by probe; with an in-size frame the assertions
   pass exactly as written.

**68 of the test author's 73 tests pass** against code neither of us had
seen, with only arity errors at the pre-ruling-108 call sites. Every type
name, method name, event variant and error value matched.

---

## 1. What I built, per module

| File | Contents |
|---|---|
| `stream_id.rs` (new) | §9.1's `StreamId`/`Dir` (public, ruling 101), `Opener`/`Space` (`pub(crate)`), the two-bit tag, `INDEX_MAX` = 2⁶⁰−1 and `MAX_STREAMS_CEILING` = 2⁶⁰. `Opener::of_role` is the **only** parity conversion (ruling 106). |
| `flow.rs` (new) | §10 entire at connection level: `Violation` (five members — §10.5's two, §10.6's third per ruling 104, plus §8.4's two per-frame semantic errors), `CreditWindow` (§10.3's re-grant, written once for both levels, `last_advertised` seeded to the constant per **H15**), `Flow` (recv charge/consume, send room, the four cumulative limits, ruling 102's two triggers against one constant). |
| `recv.rs` (new) | §9.4's receive half collapsed; the coalesce-on-insert reassembler (ruling 94, **lazy** — there is no `with_capacity` anywhere in it); `REASSEMBLY_CHUNKS_MAX` checked *after* coalescing; §9.5's three `FINAL_SIZE_ERROR` cases; contiguous-prefix `read`; `RecvTombstone` — ruling 93's **second** tombstone mechanism. |
| `send.rs` (new) | §9.3's send half collapsed; **three** range sets (`fresh` / `retransmit` / `unacked`) plus `acked`; `RangeSet`; `write`/`finish`/`reset`; `next_chunk` with the §8.5 quantum; `on_ack_range`/`on_lost_range`/`on_reset_acked`/`on_reset_lost` (§12.4/§12.5's seam, uncalled from the wire). |
| `streams.rs` (new) | The four-space table with **all four** watermarks; §9.2's implicit opening; ruling 97's check order; ruling 99's per-stream events; §9.7 GC and full-closure; the FIFO claim queue; the §8.5 rotation; `Regenerate` — §8.7's identity set. |
| `frame.rs` (edit) | Parse/encode for RESET_STREAM, STREAM (all eight flag combinations), MAX_DATA, MAX_STREAM_DATA, MAX_STREAMS_BIDI/UNI; two new `Structural` variants; `Packing::fill` + the one-extends-to-end guard **in `Packing`** (§12.3's seam); `stream_payload_room`; `STREAM_FILL_QUANTUM` (private to the module tree, **not** in `constants.rs`). |
| `mod.rs` (edit) | The nine contract verbs plus five additive ones; the six new `ConnEvent`s; `pump()` — §8.5's stages and ruling 98's seal table; `kill()` for §8.2's semantic class; a two-core `smoke` module. |
| `constants.rs`, `tests/spec_constants.rs` (edit) | Ruling 103's three kinds, marked in place. **No asserted value changed** — the wire pins are byte-identical (§7). |
| `src/core/mod.rs`, `src/lib.rs` (edit) | Re-exports only. |

### Test coverage I added inside my own files

75 tests: `stream_id.rs` 4, `flow.rs` 7, `recv.rs` 15, `send.rs` 17,
`streams.rs` 11, `mod.rs::smoke` 6 (two real `Connection`s over one real IK
handshake, so every assertion is a wire assertion), plus frame-codec cases.
These are **not** the slice's acceptance tests; they exist because the seam
*between* the modules — pump, codec, receive path — has no other in-file
exercise.

---

## 2. Contract and ruling points that were hard to satisfy

**Ruling 98 (the seal table).** Implemented from §7.4, not the plan. The
hard part is that the marking/quiet choice is a property of the *packet*,
and "first transmission" is a property of a *range*. A single pending set
would make the distinction unrepresentable and the rule correct only by
accident in a slice where every transmission happens to be a first one. So
`send.rs` carries `fresh` and `retransmit` as **separate** sets, `Chunk`
reports which it came from, and `pump` seals marking iff at least one chunk
was fresh. `send.rs::a_retransmitted_chunk_is_not_a_first_transmission`
pins it; the wire-level row is owed to slice 7 (ruling 113 already records
this).

**Ruling 97's order, and what makes step 1 safe.** `check_peer_may_send` is
a total function of the id and the role and touches no table — which is what
lets it run before the watermark. Confirmed against the code, not assumed:
it reads `id.dir()`, `id.opener()` and `self.role`, nothing else.

**Ruling 93's amendment, and why both mechanisms are structurally
separate.** Full closure (`send.is_none() && recv.is_none()`) drives the
watermark; a freed receive half on a still-open stream leaves a
`RecvTombstone` holding `{limit, high_water, final_size}`. The tombstone runs
the stream-level `FLOW_CONTROL_ERROR` check against the frozen limit and
charges nothing. **I also kept the `FINAL_SIZE_ERROR` check on the
tombstone** — see §6.

**Ruling 94's lazy allocation.** `Reassembly::new()` has an explicit comment
where a `with_capacity` would go, because that one line is the 32× vector.
Chunks are allocated at exactly the arriving length (`to_vec`, and
`vec![0u8; span]` on a merge), so `capacity()` is an honest number that
never exceeds bytes received-and-undrained.

**H15.** `CreditWindow::new(window)` sets `last_advertised = window`.
`flow.rs::a_fresh_window_owes_no_grant` reds if it is seeded to zero.

---

## 3. Conflicts found and **not** resolved

### 3.1 §16.7's synchronous sealing versus §8.5's observable round-robin — **needs a ruling**

Two of the test author's tests fail, and they fail for one reason:

- `packing::the_stream_fill_serves_pending_streams_round_robin` — asserts
  that stream B's frames start before stream A's last frame, over a
  sequence of `write(a); write(b); drain()`.
- `packing::credit_frames_precede_the_stream_fill_in_a_packet` — asserts
  that some packet carries **both** a credit frame and stream data, and
  panics with *"no packet carried both … so this test asserted nothing"*.

My build makes both **unsatisfiable**, and I believe any build that obeys
§16.7 literally does. §16.7 (`SPEC.md:4942–4947`, quoted in ruling 108) says
sealing *"executes within the mutating call that triggers it … never lazily
inside `poll_output()`"*. If `write(a)` seals, it seals **everything A can
send** — there is no other bound in slice 4 (§14's congestion window is
slice 7, and §12.6's seam explicitly says the "how many bytes may I send
right now" question has one call site that slice 7 inserts a second bound
into). By the time `write(b)` runs, A is already on the wire. Output order
is 57 A-frames then 57 B-frames.

Same mechanism for the second test: `read()` pumps a MAX_STREAM_DATA
immediately, in its own packet; the STREAM frames were pumped by an earlier
`write`. Nothing is ever owed at two levels simultaneously *unless* a write
was blocked and the incoming packet both delivered data and raised the
window — reachable, but not what the test does.

**Both readings are defensible and they are observably different**, so I
have implemented §16.7's and reported rather than picked (working rule 3).
The options as I see them:

1. **Keep §16.7 literal** (what I built). §8.5's round-robin then governs
   only the case where two streams have pending data *at the same fill*,
   which in slice 4 is reachable via §16.9 (write before install) and, from
   slice 7, whenever the congestion window binds. My
   `smoke::the_fill_serves_streams_round_robin` builds exactly that
   contention and shows the round-robin working. The author's two tests
   become slice-7 tests.
2. **Bound the pump.** Give `pump` a per-call packet budget so a verb emits
   some and defers the rest — but then the deferred remainder needs a
   trigger, and slice 4 has none (no ACK clock, no PTO). This reintroduces
   the liveness hole ruling 108 just closed.
3. **Assemble at drain time.** Contradicts §16.7 in terms.

I think (1) is right and (3) is what the author's harness assumes. The
question the spec does not answer is: **is "seal synchronously" a rule about
*when* a seal may happen, or a rule that every mutating call must flush the
whole send buffer?** §16.7 as quoted says the former; my implementation
takes the latter because nothing else would ever send. That gap is the
ruling.

### 3.2 §8.4's MAX_STREAM_DATA rules do not reach one reachable case

§8.4 gives two `STREAM_STATE_ERROR` rules for MAX_STREAM_DATA: a stream the
frame's receiver cannot send on, and *"a stream in a space the frame's
**receiver opens** that the receiver has not yet opened"*. Neither reaches
**a stream in a space the *peer* opens that the peer has not yet opened** —
e.g. the peer sends MAX_STREAM_DATA for peer-bidi index 7 having opened
none. It is legal traffic in principle (we do hold a send half on a
peer-opened bidi stream), it must not allocate (§8.4: *"credit frames never
open streams"*), and §8.4 names no error for it.

I made it an **inert no-op** and allocate nothing. Working rule 8's shape:
a stated construction (the two rules) with a scope that does not cover its
own domain. `streams.rs::max_stream_data_never_opens_a_stream` pins all
three cases including this one.

### 3.3 §8.4's *"a ¬LEN frame that is not final"* is unconstructible

§8.4 lists it as a structural error for STREAM. A parser that honours
*"the data extends to the end of the plaintext"* consumes the rest, so no
input can express a non-final ¬LEN frame — the error is unreachable **by
construction of the parser**, and the real rule is a *sender* rule that a
receiver cannot enforce. A receiver of `[¬LEN STREAM][PING]` reads the
PING's byte as stream data and cannot know. Recorded rather than dropped
(H14's precedent): if a later slice adds a length-bearing envelope this
becomes reachable and this note is where to look.

### 3.4 Ruling 113's *"two watermarks are scheduled debt"*

I maintain **all four** watermarks uniformly (`SpaceTable` is one type used
four times), so the two locally-opened ones are implemented and simply
cannot be *exercised* in 4a. That is strictly more than ruling 113 asks for
and costs nothing; the debt is a test debt, not an implementation debt, and
the three tests ruling 113 owes to slice 5 will pass against this build
without further work.

---

## 4. Things I believe are wrong

### 4.1 The `ConnEvent` breakage in `src/shell/driver.rs` — reported, not fixed

`src/shell/driver.rs:499` matches `ConnEvent` exhaustively. The six new
variants break it:

```
error[E0004]: non-exhaustive patterns: `ConnEvent::StreamOpened { .. }`,
  `ConnEvent::StreamsAvailable { .. }`, `ConnEvent::StreamReadable { .. }`
  and 3 more not covered
   --> src/shell/driver.rs:499:15
```

`src/shell/**` is not my path, so **I did not touch it** — the brief is
explicit and working rule 6 is absolute. The integration edit is one arm:

```rust
ConnEvent::StreamOpened { .. }
| ConnEvent::StreamsAvailable { .. }
| ConnEvent::StreamReadable { .. }
| ConnEvent::StreamWritable { .. }
| ConnEvent::StreamFinished { .. }
| ConnEvent::StreamReset { .. } => {}
```

**How I ran the gates anyway**: in a detached `git worktree` under the
scratchpad, seeded from HEAD, with my six files copied in and that stub
applied *there*. The delivered tree has never had a byte written to
`src/shell/`. `git status` shows six modified files, all mine.

### 4.2 Three of the test author's tests are fixture defects, not build defects

- `abandonment::abandoning_a_receive_half_releases_its_reassembly_capacity`
- `reassembly::reassembly_capacity_returns_to_zero_when_the_half_is_read_out`
- `reset::a_reset_stream_discards_the_reassembly_buffer`

All three fail on a **precondition** — `assert!(reassembly_capacity() > 0)`
*before* the behaviour under test. All three build a single STREAM frame of
4096 / 2048 / 8192 payload bytes and hand it to `Solo::deliver`, which seals
it as one plaintext. `MAX_PLAINTEXT` is **1170**, so the datagram exceeds
`MAX_DATAGRAM` and §3.1's size gate drops it silently — no error, no trace,
no counter, which is exactly why the failure looks like a leak.

Verified by probe in the worktree: with `ramp(2048, 1000)` instead of
`ramp(2048, 2048)` the same sequence gives `capacity = 1000`, and `0` after
the RESET_STREAM. The fix is theirs — chunk through `deliver_stream_bytes`,
which their own harness already provides and which chunks at 1024.

**This is working rule 13 at the frame layer**: the harness models a frame
stream and not a *packet*, so an over-`MAX_PLAINTEXT` frame is
indistinguishable from a lost one. Three tests aimed at §10.6 — the section
that exists to close a memory vector — asserted nothing.

### 4.3 The contract still omitted `abandon_recv` when I received it

Ruling 109 has since covered this. Recording the shape because it is
ruling 95's list-count defect *for the third time*: ruling 93 specifies
behaviour for dropping a `RecvStream`, the ledger it trues up lives in the
core, and the contract's verb list had no entry point. I built it as an
additive `pub(crate)` verb before the correction arrived, with the same name
the test author chose independently.

### 4.4 `on_ack_range`/`on_lost_range` lost their FIN flag

The corrected signature is `(now, r, range)`. The send half needs to know
whether the acknowledged frame carried the **FIN**, or `DataRecvd` is
unreachable and §9.7's send-side GC ships untested. I infer it as
`range.end == final_size`, which is exact for every frame this
implementation emits (the FIN rides the frame that ends the stream, and
`return_chunk`/`on_lost_range` un-set `fin_sent` so it is re-emitted). But
**§8.7 lets a retransmission re-frame ranges freely**, so a range ending at
the final size need not have carried the FIN, and slice 5's sent-packet map
is where the answer actually lives. `SendHalf::on_ack_range(range, fin)`
keeps the flag; slice 5 should call the half directly, or the `Connection`
signature should regain it.

---

## 5. Deliberately not built — the seams I left

- **§9.8 messages** — read only far enough to know the boundary. The claim
  queue is a **queue of unclaimed peer-opened halves** (`unclaimed`), FIFO,
  not "the newest one", so §9.8's second claim verb draws from the same
  supply. `RecvHalf::earns_stream_credit` is a **field**, always `true` in
  slice 4, so slice 6 does not thread a mode flag through the ledger. §9.6's
  receiver-emitted reset is *not* built and is deliberately **not** merged
  with the sender-emitted one: the latter lives in the stream's own state
  (`SendHalf::reset`), and slice 6 needs a connection-level structure that
  outlives the stream.
- **STOP_SENDING (`0x05`)** — not built in any form; still
  `Structural::UnknownType`, still tested. No private "tell the sender to
  stop" mechanism was invented; ruling 93 is the answer to that pressure.
- **§11 datagrams** — not built. The one-extends-to-end rule lives in
  `Packing`, not in the fill loop, so slice 6 adds a second contributor to
  `Stage::Fill` without duplicating it. `flow.rs` is unreachable from any
  non-stream path by construction: every entry point is keyed by a stream
  (§10.7).
- **§12 ACKs** — not built. `Regenerate` (the identity set) and
  `SendHalf::unacked` (the retention set) are populated and driven by
  nothing. **The retention set never drains**, by design; freeing on send
  would make slice 5's tests pass for free, and
  `send.rs::sending_does_not_free_the_retention_set` reds if it does.
- **§13 loss recovery / §14 congestion control** — not built. The fill
  loop's "how many bytes may I send right now" question has **one** call
  site (`SendHalf::write`'s `conn_room` argument), which is where slice 7's
  second bound goes.
- **`ConnEvent::StreamFinished` never fires from the wire** — correct, and
  it is reachable from `Connection::on_ack_range` so the GC path is
  testable now.

---

## 6. Decisions the contract did not cover, that I was forced to make

Ordered by how much I would like a ruling on them.

1. **§16.7's flush semantics.** §3.1 above. The highest-value item here.
2. **The tombstone keeps the `FINAL_SIZE_ERROR` check, not only the
   flow-control one.** The contract's §4a mandates the stream-level
   `FLOW_CONTROL_ERROR` check on an abandoned half and is silent on final
   size. `RecvTombstone` carries `final_size` (24 bytes total) and runs both.
   Dropping it would make §9.5's check depend on local read timing in a
   *second*, unstated way — C6 already makes it depend on it once.
3. **`Connection::on_reset_acked(now, r)` exists and is not in §16.4's
   listing at all.** Without it a locally-reset send half can never reach
   `ResetRecvd`, so §9.7's second terminal and the full-closure path through
   it are unreachable and untestable. It takes `now` for ruling 108's reason.
   *Asked of the whole §16.4 stream listing, "can this emit a frame?" now
   answers: `open` no, `accept` no, `stream_id`/`role`/`reassembly_capacity`
   no, everything else yes — and `on_reset_acked` is the one verb the
   listing does not have.*
4. **`read()` needs `now` for two independent reasons**, not one. Ruling 108
   names §10.3's re-grant. The second is that a read reaching the final size
   **retires the half** (§9.7), which for a peer-opened uni stream fully
   closes the stream and owes a MAX_STREAMS grant (§10.4). The second reason
   stands even if the re-grant threshold never trips.
5. **`CREDIT_REGRANT_DIVISOR` is a fourth constant ruling 103 does not
   classify.** It sits in §10.3's named-constants table beside the ones the
   ruling does classify, and it is *receiver policy, invisible* by exactly
   the `STREAMS_CREDIT_BATCH` argument — two peers re-granting at different
   fractions interoperate perfectly. Marked as such in `constants.rs` and
   `spec_constants.rs`, with a pointer here. **This is the recursive shape
   round 17 named of itself**: ruling 103 extended H16's hunt one section
   further and stopped one constant short.
6. **`finish()` twice is `Ok(())`; `finish()` after `reset()` is
   `Err(Finished)`.** The contract's table defines `Finished` only for
   `write`. A repeat `finish` is a statement of intent that is already true;
   after a reset the half is gone.
7. **`read()` on a half this endpoint does not hold returns `Ok(None)`**
   (EOF), not an error: §18.1's taxonomy is closed (ruling 61) and there is
   no "wrong direction" variant to invent.
8. **`write`/`finish`/`reset` on a stale or unknown `StreamRef` return
   `Err(WriteError::Finished)`.**
9. **`StreamRef` derives `Ord` and `PartialOrd`** beyond ruling 95's list, so
   the stream table is a `BTreeMap` and every event burst has a deterministic
   order — §16.4 makes output ordering normative and a `HashMap` would make
   ruling 99's six-event burst nondeterministically ordered.
10. **The sender never emits the ¬LEN (extends-to-end) STREAM form.** §8.5
    bounds it (*"at most one"*) rather than requiring it; the 1–2 bytes it
    saves are not worth a stage-ordering hazard that misfires only once slice
    6 adds a second fill contributor. The **parser** implements it fully, and
    `Packing` enforces the rule so slice 6 inherits it. Consequence: a test
    asserting "at most one extends-to-end frame" passes vacuously against
    this build — flagged because that is working rule 9's shape.
11. **`STREAM_FILL_QUANTUM = 1024`**, `pub(super)` in `frame.rs`, chosen so a
    single stream still fills a packet in one pass while two streams
    alternate within one. Not in `constants.rs`; §8.5 makes it
    implementation-defined and the pinned table would make it a wire pin.
12. **MAX_STREAMS is emitted only when the limit actually rises**, and
    `StreamsAvailable` likewise. Monotone-max makes a non-increasing value a
    valid no-op (§8.4), and a no-op must wake nobody.
13. **`RESET_STREAM`'s `final_size` under-charges our own connection window.**
    Ruling 111 makes it the highest offset *sent*; we charged `write_offset`
    at `write` time. The difference is buffered-but-never-transmitted bytes,
    which stay charged against our send window for the connection's life. It
    is conservative (we send less than we may) and self-limiting (bounded by
    one stream window per reset stream), but it is a real asymmetry between
    the two ends' accounting and slice 5 may want to reclaim it.
14. **A semantic violation mid-packet keeps the events earlier frames in the
    same packet legitimately generated**, and drains them before `Closed`.
    Ruling 99's *"emits zero events"* is delivered by the **check order**,
    not by discarding afterwards; §8.2 discards a packet's effects only on
    the *structural* path.

---

## 7. Gate output

Run in a detached `git worktree` at HEAD (`103f1ca`) with my six modified
files copied in and the one-arm `src/shell/driver.rs` integration stub
applied **there only** — see §4.1. The delivered tree is byte-identical
except for that arm.

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.75s

$ cargo fmt --all --check
(no output — no diff)

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.65s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated .../target/doc/slither/index.html
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated .../target/doc/slither/index.html

$ cargo test
   (5 suites, all "test result: ok", 0 failed)

$ cargo test --all-features
   565 passed, 0 failed

$ cargo test --release --all-features          # CLAUDE.md's new §11.8 gate
   565 passed

$ cargo test --test spec_constants             # the wire pins
test result: ok. 103 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo +1.96 check --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.88s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

**Wire pins are green and no asserted value was changed** — ruling 103's
edits to `tests/spec_constants.rs` are comments only (7 `Kind:` markers).
454 → 565 tests; slice 3a's suite is untouched and passing.

### The test author's suite, run against this build

Wired in the same throwaway worktree (never in the delivered tree), with the
15 pre-ruling-108 call sites given their `now` argument mechanically:

```
73 tests: 68 passed, 5 failed
  3 failures — fixture defects (§4.2), frames over MAX_PLAINTEXT
  2 failures — the §16.7 / §8.5 conflict (§3.1), needs a ruling
```

Zero name, type, signature or value mismatches beyond the `now` arity.

---

## Reading log (append-only, as I went)

- Read `CONTRACT-4a.md` (232 lines) in full. Binding API surface noted:
  `StreamId`/`Dir` public, `Opener`/`Space`/`StreamRef`/`StreamsExhausted`
  `pub(crate)`; nine `Connection` verbs; six new `ConnEvent` variants keyed
  by `StreamRef`.
- Read `rulings.md` Round 17 (lines 2044–2468), rulings 93–107 in full.
  Key deltas from the plan: ruling 98 (seal table — §7.4 governs; RESET_STREAM
  is quiet, STREAM retransmission is quiet), ruling 93 (true-up = highest
  stream-level limit ever advertised).
- Read `PLAN.md` §2–§5 (237–725), §6 (726–796), §8 (861–1290), §9 (1291–1527),
  §12 (1783–1935).
- Read the existing tree: `src/core/connection/{mod,frame}.rs` in full,
  `session.rs` seal surface, `src/core/mod.rs` (Role/Install/Transmit),
  `src/constants.rs` §8.3/§8.4/§10 blocks, `src/error.rs`
  `WriteError`/`ReadError`, `src/lib.rs` re-export block.
- **Contract amendment received mid-work** (§4a, ruling 93's two tombstone
  mechanisms). Re-read `CONTRACT-4a.md` lines 170–254. Design updated before
  any code was written: a per-half `RecvTombstone` **plus** watermark advance
  only where the abandonment fully closes the stream.
- **Second correction received mid-work** (rulings 108–113): `now` on
  `read`/`finish`/`abandon_recv`, `on_ack_range`/`on_lost_range` on
  `Connection`, the empty-write rule, `final_size` = highest byte sent, FIFO
  `accept`. All applied; read rulings 112–113 in full afterwards.
- Read `rulings.md` 2509–2640 (round 18) and re-ran the whole §16.4 stream
  listing against "can this verb emit a frame?" — result in §6.3.
- Ran the test author's `collected/tests_streams.rs` against this build in a
  throwaway worktree, after both files were finished. Result in §7.

**`SPEC.md` was never read whole.** Everything used came from the plan's
quotations, the rulings, and one targeted `grep -n` for §7.4's quiet set,
which `session.rs:412` already carried.

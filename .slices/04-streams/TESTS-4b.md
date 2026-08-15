# TESTS-4b — the blind test author's record

Author: **4b-test**, blind to the implementation. Worktree cut from
`79b2925`. Owned paths: `tests/story_streams.rs`, `tests/spec_streams.rs`,
this file. Nothing else was created or edited.

Written from `STORIES.md` (236–286), `SPEC.md` §9/§10/§16.2/§16.8/§16.9,
`.slices/04-streams/CONTRACT-4b.md` (whole) and `PLAN-4b.md` §2.2/§4/§5/§6.
No file under `src/shell/` was read; `src/testutil/mod.rs` was not opened
(see §5 on how its signatures were recovered instead).

**25 tests: 6 in `story_streams.rs`, 19 in `spec_streams.rs`.**

---

## 0. API I am writing against (CONTRACT-4b.md, transcribed)

- `Connection<S>::{open_bi -> BiStream, open_uni -> SendStream, accept_bi ->
  BiStream, accept_uni -> RecvStream}`, all `async fn(&self) -> Result<_,
  ConnectionLost>`.
- `SendStream<S>::{write(&mut self, &[u8]) -> Result<usize, WriteError>,
  finish(&mut self) -> Result<(), WriteError>, reset(&mut self, u64),
  id() -> Option<StreamId>}`.
- `RecvStream<S>::{read(&mut self, &mut [u8]) -> Result<Option<usize>,
  ReadError>, id() -> Option<StreamId>}`.
- `BiStream<S>::{split(self) -> (Send, Recv), join(Send, Recv) ->
  Result<Self, (Send, Recv)>, id()}`. No `Drop` of its own; fields drop in
  order `send`, then `recv`.
- `write`: `Ok(0)` **only** for empty buf; `Pending` is the only "blocked";
  `Err(Finished)` after `finish()`/`reset()`; `Err(Reset)` unreachable in
  slice 4.
- `read`: `Ok(Some(0))` **only** for empty buf; `Pending` is the only
  "no data"; `Ok(None)` sticky EOF; `Err(Reset(code))` sticky (ruling 121).
- `finish()` resolves on the first poll and never parks; idempotent.
- `reset()` sync, infallible, idempotent, first code wins, legal after
  `finish()`.
- `id()` caches; always `Some` for a handle the app holds (ruling 116);
  keeps answering after close and after the connection dies.
- Drop of `SendStream` resets **only when `closed_locally == false`** —
  §9's named most-likely-bug.
- Aliases: `TestSendStream`, `TestRecvStream`, `TestBiStream`.

---

## 1. Inventory, with what each test pins and what the broken build does

Per working rule 9, every row names an implementation that **passes a
weaker version of the test while being wrong**. The same text is in each
test's `BROKEN BUILD` doc comment, so it travels with the code.

### `tests/story_streams.rs` — 6 tests

| # | Test | Pins | The broken build it separates |
|---|---|---|---|
| 1 | `s12_loss_free_a_user_can_stream_over_a_reordering_duplicating_path` | 96 KiB over a path with jitter-reordering and 35 % duplication, loss off; content equality by offset-derived payload; `finish()` → **`drop(send)`** → read to EOF; EOF sticky | **The slice's named bug**: `Drop` resets unconditionally, destroying a *finished* stream's FIN and unsent buffer. Nothing is read until after `drop(send)` — reading first is exactly what lets that build ship. Also: a reassembler that mishandles a duplicated overlap (content), one that re-delivers duplicates (length, asserted first and separately), a non-sticky EOF (second `read`) |
| 2 | `s13_a_stalled_stream_does_not_block_a_concurrent_one` | A permanent multi-packet hole in stream A via `block_path`; B read to EOF **while an A read future is held `Pending`**; A's prefix delivered; both `closed()` still `Pending` | One reassembly buffer keyed by offset not by stream; a fill loop that drains A before touching B; a build that discards a stream on its first gap (`settle()` runs before the drain, so the gap is already known — without that ordering the assertion passes such a build for free); a build that treats a gap as a connection error |
| 3 | `s13_stream_ids_carry_establishment_parity` | Four ids — {initiator, acceptor} × {uni, bidi} — **on one connection in one test**, each checked for `initiated_by_connection_initiator()`, the raw `0x01`/`0x02` bits and `dir()`; the peer's view of the same id gives the same answer; `split()` preserves the id; all four ids distinct | A single shared id counter ("the two sides' ids differ" passes it); a build ignoring the direction bit; **a build that answers parity from the local role rather than from the id** — the peer-side assertion is the only thing that catches it |
| 4 | `s14_a_reset_stream_leaves_the_connection_and_its_siblings_alive` | `reset(0x2a)` → peer `Err(Reset(0x2a))`; **second read still `Err(Reset(0x2a))`**; write/finish after reset → `Finished`; a sibling opened *after* the reset does a full round trip; a dropped unfinished handle gives exactly `Reset(0)`; both `closed()` `Pending` | A shell surfacing "some reset" (code asserted exactly, and `0` is the *drop* default so a test written with `0` cannot tell the two apart — both are exercised here with different codes); **an unlatched shell** (ruling 121: the core answers `Ok(None)` on the retry, reporting abandoned data as a complete transfer); a reset that tears down the connection |
| 5 | `s17_a_slow_reader_stalls_its_own_stream_only` | Park at **exactly** `INITIAL_MAX_STREAM_DATA` (offering more than the window, so the park is observed where the *implementation* puts it); a sibling still writes; then the connection window filled to **exactly** `INITIAL_MAX_DATA` and a *fresh* stream with an untouched stream window parked by the connection level | Build A (grants nothing) — fails the byte count; build B (grants unconditionally) — fails the `blocked` flag; **build C (one collapsed ledger) — caught twice, from opposite sides**, which is the only way to pin it: a stream-sized shared ledger fails the sibling probe, a connection-sized one fails the byte count |
| 6 | `s17_the_sender_resumes_when_the_reader_drains` | §10.3's threshold **two-sided**: still `Pending` after draining `WINDOW/2 − 1`, resumes at `WINDOW/2`, and then accepts **exactly** `WINDOW/2` more; content equality across the resume boundary | Grants on every read (unblocked one byte early); grants nothing; seeds `last_advertised` at 0; **drops the ceiling on resume** — caught only by the exact post-resume byte count |

### `tests/spec_streams.rs` — 19 tests

**Drop semantics (8)**

| # | Test | The broken build |
|---|---|---|
| 1 | `dropping_an_unfinished_send_stream_resets_it_with_code_zero` | A `Drop` that does nothing (explicit `Ok(_)` panic arm); a `Drop` with a different code (`assert_eq!` on `0`, not `matches!(Reset(_))`) |
| 2 | **`dropping_a_finished_send_stream_still_delivers_the_end_of_stream`** | **The mandated test.** The unconditional-reset `Drop`. 40 KiB so "every byte not yet on the wire" is a real set; nothing read until after the drop |
| 3 | `an_explicit_reset_after_finish_supersedes_the_fin` | The **positive control** for #2, and the reason #2 cannot be satisfied by making `Drop` a no-op: a `reset` that early-returns on `fin` passes #2 and fails here. Neither test is a pin alone |
| 4 | `dropping_a_send_stream_that_parked_on_credit_still_resets_it` | A `Drop` that skips the core call when a waker is registered; F10's borrow-then-wake re-entry (which would panic on this path) |
| 5 | `dropping_a_recv_stream_stalls_only_that_stream` | A build that keeps advancing stream credit for an abandoned half; the stall asserted at **exactly** the stream window, so a build with any other window fails while still stalling |
| 6 | `dropping_recv_streams_releases_the_connection_window` | **Ruling 93's true-up, and the only place `tests/` can see it.** Four bidi streams fill the connection window exactly; a fifth is parked; the peer drops all four `BiStream`s **whole**; the fifth resumes. Separates tombstone-without-true-up (a permanent send stall with no error and no timer) **and** a `Drop` written on `BiStream` that forgets the receive half. Two-sided: parked before, resolving after |
| 7 | `dropping_the_recv_half_of_a_bi_stream_leaves_the_send_half_alive` | A `Drop` on `BiStream` rather than on the halves — one that ties the two lifetimes together |
| 8 | `a_live_send_stream_stops_the_last_connection_drop_from_closing_underneath_it` | **Ruling 115 / §7.** The rejected alternative: a `SendStream` that is not a handle, so dropping the last `Connection` fires `close(NO_ERROR, "")` underneath it and every later `write` is `ConnectionLost`. Asserted twice — the peer's `closed()` still `Pending`, and the stream still delivering |

**Two-sided boundaries (6)**

| # | Test | The broken build |
|---|---|---|
| 9 | `an_empty_buffer_is_the_only_meaning_of_ok_zero_on_write` | A shell reporting a blocked write as `Ok(0)`. Both rows in one test: `write(&[])` is `Ok(0)` *and* a blocked non-empty write is `Pending`. Either row alone passes the inverted build. `0` and `1` both asserted (slice 1's one-sided-boundary trap) |
| 10 | `an_empty_buffer_is_the_only_meaning_of_ok_some_zero_on_read` | `Ok(Some(0))` used for "no data" (spins a caller); **`Ok(None)` used for "no data"** — working rule 14's near-miss, which reports a partial transfer as complete. Caught by writing more *after* a `Pending` observation and reading it |
| 11 | `finish_resolves_immediately_is_idempotent_and_closes_the_write_verbs` | A `finish()` that waits for the peer (`poll_once`, not `timeout` — a timeout races the future rather than observing it); a non-idempotent `finish`; a `write` after `finish` that silently discards |
| 12 | `reset_is_idempotent_and_the_first_code_wins` | A last-wins `reset` (two distinct non-zero codes); a non-idempotent one |
| 13 | `id_keeps_answering_after_the_stream_closes_and_after_the_connection_dies` | **The uncached build.** `Streams::after_half_freed` removes the entry, so an uncached `id()` answers `None` again once the stream fully closes — and passes every test that reads `id()` on a live stream. Also asserted after both connections close |
| 14 | `join_rejects_mismatched_halves_and_hands_them_back_unchanged` | A `debug_assert` on mismatch (ruling 44: *rejection is a `Result`, never a panic*) — which is why the `--release` gate matters; a `join` that accepts the mismatch; a `join` that consumes the halves on rejection (ids asserted, pair then rejoined and used) |
| 15 | `every_stream_verb_answers_connection_lost_after_the_connection_dies` | **Ruling 118.** A stream the peer opened is deliberately left unclaimed when the connection dies: a drain-then-report `accept_*` hands it back, and every `read` on it then fails. `poll_once` is what states *immediately*. Also: a `reset()` that panics on a dead cell, an `id()` that stops answering |

**Cancel-safety (4)**

| # | Test | The broken build |
|---|---|---|
| 16 | `a_cancelled_read_loses_no_byte_and_duplicates_none` | A shell-side scratch buffer (§10.6, ruling 56). The construction is the one that reaches it: polled to `Pending`, data arrives and fires the waker, future dropped **without ever being polled again** |
| 17 | `a_cancelled_write_claims_nothing` | A `poll_write` storing a partially consumed buffer (ruling 53's named hazard). The cancelled payload is `0xFF`, a byte `payload()` never produces, so one leaked byte is a mismatch at a nameable offset rather than a length that happens to work out |
| 18 | `open_uni_parks_at_the_cumulative_limit_and_resumes_when_the_peer_frees_streams` | §10.4 off by one **in either direction** — all 128 opens asserted *and* the 129th parking. Plus §4.4's leak, observed precisely: after the cancelled park and the replenishment, the resumed stream's `StreamId::index()` must be **128, not 129** |
| 19 | `a_cancelled_accept_claims_nothing` | A cancelled `accept_*` that swallowed a stream (§4.5's pop is *"strictly worse"* than §4.4's); a shell assuming one `StreamOpened` event equals one stream (ruling 99/112) — two streams opened back to back, claimed by two separate calls, is the smallest case that distinguishes it |

---

## 2. Design decisions worth reviewing

- **`NOT_BEFORE` is 200 ms of virtual time, not seconds.** Every
  "it is still parked" assertion spends its budget on the paused clock.
  `DEAD_TIMEOUT` is 25 s, so a test that spent seconds proving a stall
  would kill the connection and **pass for the wrong reason**. Five such
  assertions in one test is ~1 s.
- **No `sleep` anywhere.** `timeout` in both directions; `settle()` for
  quiescence (it advances no virtual time, by design).
- **No test awaits `open_bi` on an exhausted bidi space.** Every bidi test
  stays under 5 streams against `INITIAL_MAX_STREAMS_BIDI` = 32.
- **`StreamRef` is never named.** Every assertion is in `StreamId`s, byte
  counts or handle behaviour.
- **S13's gap uses `Network::block_path`, not `FlakyPolicy::drop_at`.** An
  index-based drop would have to account for every handshake datagram
  already counted on that wire and would move the moment an unrelated send
  is added. **Coupling flagged in the test's doc comment**: withholding
  exactly A's middle chunk works only because ruling 114 makes two
  sequential `write()`s never share a packet. If ruling 114's reading is
  revisited, that construction goes with it.
- **S12 installs the flaky policy *after* establishment.** A handshake
  surviving a duplicated msg1 is slice 2's story; folding it in would make
  an S12 red ambiguous between two slices. 96 KiB ≈ 84 datagrams, and a
  full jitter permutation of 84 is inside `REPLAY_WINDOW` (2048 packets)
  and `REASSEMBLY_CHUNKS_MAX` (1024) **by construction**, so the reordering
  cannot manufacture loss on a path configured for none.

---

## 3. Conflicts and defects found — reported, NOT resolved (working rule 3)

### C1 — `BiStream::join`'s error type cannot be `unwrap`ped or `expect`ed

`CONTRACT-4b.md` §6 gives `join(...) -> Result<Self, (SendStream,
RecvStream)>`; §1 says none of the three handles is `Clone`, and nothing
gives them `Debug`. `Result::expect`/`unwrap` require `E: Debug`, so

```rust
let bi = TestBiStream::join(s, r).expect("...");   // E0277, twice
```

**does not compile.** Confirmed against a contract-shaped stub. The
`Result` shape itself is right (ruling 120, and ruling 44's *rejection is
a `Result`, never a panic*), so this is not a rule to change — but every
caller must `match`, and that is not stated anywhere. Either derive
`Debug` on the handles or say so in `join`'s rustdoc. My test uses a
`match` and carries a comment pointing here.

### C2 — three rules each claim to run "before anything else", and their order is unstated

Working rule 8's shape exactly: *a stated construction with an unstated
scope.*

- `CONTRACT-4b.md` §8: *"`ConnCell::closed` … **Every 4b verb answers from
  it before anything else.**"*
- §5, ruling 119: *"the shell short-circuits an empty `buf` **before
  touching the core**"* (and §3, ruling 110, for `write`).
- §5, ruling 121: the reset is *"**Sticky**, latched by the shell"*.

So these have two defensible answers each, and the contract does not pick:

| Call | Candidate answers |
|---|---|
| `read(&mut [])` on a stream that was reset | `Ok(Some(0))` or `Err(Reset(code))` |
| `read(&mut [])` after the connection died | `Ok(Some(0))` or `Err(ConnectionLost)` |
| `write(&[])` after `finish()` | `Ok(0)` or `Err(Finished)` |
| `write(&[])` after the connection died | `Ok(0)` or `Err(ConnectionLost)` |

§3's `Finished` row adds a wrinkle: it is a **handle** field
(`closed_locally`), not core state, so "before calling the core" does not
decide it either.

**I tested only the unambiguous combinations** — an empty buffer on a
live, open stream — and said so in the tests. Four boundary rows are
currently untested because the contract does not say what they are. This
wants a ruling; it is cheap to add tests once it exists.

### C3 — `PLAN-4b.md` §6.1's short-write assertion can fail a *correct* build

The plan asks S12 to assert that `write` returns `Ok(n)` with
`n < buf.len()` at least once over 64 KiB, separating *"a `write` that
silently claims the whole buffer"*. But claiming the whole buffer is
**correct** when the whole buffer fits inside the flow-control window:
`CONTRACT-4b.md` §3 defines `Ok(n)` as *n bytes accepted into send state
and already sealed*, ruling 114 makes a mutating call flush everything the
ledger admits, and nothing bounds `n` by `MAX_PLAINTEXT`. With a 96 KiB
payload inside a 262 144-byte window, a single `Ok(98304)` is a legal
answer, and the assertion would go red against a correct implementation.

**I did not write it.** What I wrote instead is the version flow control
*forces*: S17 asserts `write` accepts exactly `INITIAL_MAX_STREAM_DATA`
and then parks — a short write no correct build can avoid — and
`write_all` asserts the weaker invariant that a write never claims more
than it was handed. If the maintainer wants the plan's row, it needs a
payload larger than the stream window, which makes S12 a flow-control test
too.

### C4 — §16.8 keys the waker maps by `StreamId`; ruling 117 and §10 key them by `StreamRef`

Already raised as candidate conflict C-A in `PLAN-4b.md` §3.1. Recorded
here only so it is not lost: it is not reachable from `tests/` and no test
of mine depends on either reading.

### C5 — `PLAN-4b.md` §6.5's fifth row was written before OQ-1 was ruled

The row is annotated *"whether a `SendStream` can outlive its
`Connection` at all depends on OQ-1. Written before OQ-1 is ruled, that
test encodes a guess."* `CONTRACT-4b.md` §7 (ruling 115) has since ruled
it, so I wrote it — test 8 — against the ruling, not the guess. Noted
because the plan's caveat now reads as live and is not.

---

## 4. What I could not test, and why

1. **Frame counts of any kind.** `Tap::datagrams()` yields sealed
   datagrams — AEAD ciphertext after establishment. `PLAN.md` §11.2/§11.4's
   two assertions (no MAX_STREAM_DATA at `WINDOW/2 − 1`, exactly one at
   `WINDOW/2`) are unreachable; ruling 123 moves them to 4a's in-crate
   file. **Test 6 is the behavioural substitute and is two-sided.**
2. **§4.4/§4.5's actual leak window** — between `core.open()`/`core.accept()`
   succeeding and the handle being constructed. It is inside one
   synchronous `poll_*` body and is unreachable from any test. The
   *consequence* is reachable and is asserted: `StreamId::index()` after a
   cancelled park (test 18).
3. **Ruling 93's tombstone-only half** — the watermark alone resurrecting
   an abandoned bidi half. Core-level; test 6 covers the other half (the
   true-up), which is the one reachable from outside.
4. **A `Drop` outside a tokio runtime** (`PLAN-4b.md` §5.3's R4:
   `shared::now()` is `tokio::time::Instant::now()`, which both `Drop`s
   call). `local()` is the only way to build a `Pair`, so every handle in
   `tests/` is created and dropped inside a runtime. **Working rule 13's
   shape — the fixture models a network, not a process.** If a guard is
   needed it wants an in-crate test.
5. **Whether a `SendStream` *alone* keeps the driver alive.** `Pair` holds
   both `Endpoint`s for the test's life, and an `Endpoint` keeps the driver
   up regardless — so that half of ruling 115 is not separable through
   `Pair`. Test 8 asserts the half that is: dropping the last `Connection`
   must not close underneath a live stream. Reaching the other half needs a
   fixture that can drop the `Endpoint` and keep a stream (`Peer`'s fields
   are `pub`, so it is constructible — but not through
   `Pair::establish()`).
6. **A `SendStream` dropped while a `write` future is genuinely parked.**
   `write(&mut self, ..)` borrows the handle for the future's life, so the
   state is not constructible from safe Rust. Test 4 is the reachable
   variant: a handle whose waker-map slot was populated by a cancelled
   park, dropped immediately after.
7. **`WriteError::Reset`** — no producer in slice 4. Asserting `Finished`
   on both the write-after-reset and finish-after-reset paths *is* the test
   that no build fabricated one.
8. **`id()` returning `None`** — ruling 116 removed the cause. Explicitly
   not asserted, and said so in the module doc so the absence reads as a
   decision.
9. **`open_bi` on an exhausted bidi space** — parks for ever until slice 5.
   Not awaited anywhere.
10. **The four boundary rows in C2** — blocked on a precedence ruling.

---

## 5. How the harness signatures were obtained without opening `src/testutil/mod.rs`

My brief forbids opening that file. `FlakyPolicy`'s builder signatures are
not quoted in `PLAN-4b.md`, so I took them from
`.slices/00-ground/PLAN.md` §8.3 and **verified every one against generated
rustdoc** (`cargo doc --all-features --no-deps`, then reading
`target/doc/slither/testutil/*.html`). That is generated output, not the
source file, and it also surfaced the `Pair` / `Peer` / `local` harness —
which is why these tests use `Pair::seeded(seed)` and `pair.establish()`
rather than re-deriving slice 3's local `Node` fixture a third time.

**Recommendation for the next brief**: name the harness in the brief, or
say that rustdoc is the sanctioned route. Slice 3's two test authors each
hand-rolled a `Node` fixture and flagged it loudly *"proposed and ratified
nowhere"*; the harness they were guessing at already existed.

---

## 6. Verification actually run (working rule 7)

The API does not exist in this worktree, so `cargo test` cannot compile
these files here — that is expected and stated in my brief. To avoid
handing over 25 tests with unknown type and borrow errors, I compiled both
files against a **throwaway `CONTRACT-4b`-shaped stub crate in the
scratchpad** (never in `src/`, never in this repo):

```
$ cargo build --tests --manifest-path <scratchpad>/stubcheck/Cargo.toml
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ cargo clippy --tests --manifest-path <scratchpad>/stubcheck/Cargo.toml
  14 warning: call to `std::mem::drop` with a value that does not implement `Drop`
  (all `clippy::drop_non_drop`, all on `SendStream`/`RecvStream`, which have
   no `Drop` in the stub and do have one in the real slice 4b — the lint
   cannot fire there. Every warning on a `Pin<&mut _>` was removed, because
   that one *would* fire.)

$ rustfmt --check --edition 2024 tests/story_streams.rs tests/spec_streams.rs
FMT CLEAN
```

This caught three real defects before hand-off: C1 above, a borrow error
where a `pin!`ed `finish()` future outlived its `drop`, and two
offset-accounting mistakes of mine in the S17 resume test.

**The stub proves shape, not behaviour.** It says nothing about whether the
implementation is correct — only that these tests will compile against the
contract as written, so the first integration red is a behavioural finding
rather than a typo.

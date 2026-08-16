# Slice 8 — Composability layer (`compat/`)

**Base commit:** `5ed5bb4` ("Rulings 205 withdrawn, 206 corrected: two claims
about unopened documents") — verified with `git log --oneline -1` as the first
act of this planning run (working rule 14).

**Status:** complete, pending the maintainer's answers to §Open questions.
Skeleton written before any input was read (working rule 2); every section
appended as its inputs were read.

**Companion:** `.slices/08-composability/CONTRACT-8.md` — the binding API
contract the three blind agents build against. **Commit both before
dispatching** (working rule 14).

**This planner touched `src/` not at all, and `.slices/07-mobility/` not at
all.** It owns exactly `PLAN-8.md` and `CONTRACT-8.md`.

---

## Open questions

*(to the maintainer; recommendations attached, nothing resolved unilaterally.
Working rule 3: where two statements conflict I report the conflict and do
not pick one.)*

---

### Q1 — S31–S33 do not exist in `STORIES.md`. Are they ratified, and where do they live?

**The brief says "`STORIES.md` — S25 and S31–S33".** `STORIES.md` has no S31,
S32 or S33; its headings run S1…S30 and its own D10 entry records
*"`STORIES.md` is complete at **30 approved stories**."* The three drafts are
in **`PLAN.md` §7, lines 356–378**, under *"Stories owed by this plan …
**Drafted here, to move into `STORIES.md` on approval**"*, and `PLAN.md`'s
slice table marks them *"(drafted, §7)"*.

This matters because `CLAUDE.md` sets the bar as *"a slice is done when its
stories are paused-clock tests that pass"*, and because Agent B's brief must
quote something. **Recommendation:** ratify S31–S33 and move them into
`STORIES.md` (making it 33) **before** dispatch, so Agent B is briefed from
the authoritative file rather than from a plan's draft. If you prefer to
leave them in `PLAN.md` §7, say so explicitly and I will point Agent B's
brief there — but then `CLAUDE.md`'s "30 approved stories" sentence and
`STORIES.md`'s "complete at 30" both need a line saying slice 8's acceptance
lives elsewhere, or the next agent hits the same wall.
**Depends on:** nothing. Blocks: Agent B's brief.

---

### Q2 — Ruling 122(a) promoted the `poll_*` verbs for `compat/` but not the `WakerSlot` accessors they require. Two things are needed; which shape do you want?

Ruling 122(a): *"The `poll_*` forms are **`pub(crate)`** … slice 8's
`compat/` is in-crate and reaches them, and promotion later is additive while
publishing a `key: u64` parameter now is not."* **The mechanism named is real
but incomplete** — this is working rule 11's shape (*a rationale must name a
mechanism that exists; check it against the code*) and working rule 8's
(*a stated construction with an unstated scope*). Opening
`src/shell/connection.rs` shows both halves of the gap:

**(a) The accessors are private, not `pub(crate)`.** Every `Connection`
`poll_*` takes `key: u64`, and the key comes from a `WakerSlot` minted at
`connection.rs:279, 477, 919, 928, 937, 952, 961` — all declared plain `fn`,
private to `shell::connection`. `crate::compat` is a different module and
cannot call one. Without a slot an adapter has no key that is stable for its
life **and released on drop**, which is the whole of the verbs'
cancel-safety.

**(b) `WakerSlot<impl FnMut(u64)>` cannot be a struct field.** The return type
uses `impl Trait`, so it is unnameable — and a `Stream` adapter is a struct
that must **hold** its slot across `poll_next` calls. A `Messages<'_, S>` with
an unnameable field type does not compile, and it cannot be made generic
without leaking an unnameable parameter into a public type.

**Recommendation:** add seven `pub(crate)` type-erased accessors alongside the
existing private ones, returning `WakerSlot<Box<dyn FnMut(u64)>>` (which
satisfies `F: FnMut(u64)` and is nameable). Purely in-crate and additive; no
public signature moves, so ruling 204 is untouched; ~30 lines in
`src/shell/connection.rs`, owned by Agent A as task T0. The alternative —
moving `compat/` under `src/shell/` — contradicts `PLAN.md` §3's stated layout
(`PLAN.md:88`) and buries a consumer-facing surface inside the shell module.
**Depends on:** your call on whether an in-crate visibility promotion needs a
ruling at all. **Blocks:** Agent A and `CONTRACT-8.md` §4's exact adapter
struct shapes.

---

### Q3 — `Endpoint::incoming()` is the one adapter with no `poll_*` behind it, and §16.11's own list omits it

`Endpoint::accept()` is on **ruling 53's channel side**, not the shared-cell
side — §6.2 requires the DH to land on the driver task, so the verb is
`oneshot`-backed:

```rust
pub async fn accept(&self) -> Option<Intro<I>> {
    if self.shell.driver_stopped() { return None; }
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.shell.send(Command::Accept(tx));
    rx.await.ok()
}
```

There is no `poll_accept`. So an `Incoming` stream must **box and store an
in-flight future** — which is precisely the cost SPEC.md:5177 names as what
ruling 53 exists to avoid: *"Under the channel form neither is available:
every adapter in §16.11 must box and store an in-flight future, `Unpin`
becomes delicate … and every write costs a round-trip and an allocation."*

**Two further observations, reported not resolved:**

- **§16.11's own list omits `incoming()`.** It reads: *"The `Stream`/`Sink`
  faces (`messages`, `datagrams`, `incoming_bi`, `incoming_uni`,
  `notifications`, and the two sinks), the codec constructors, and any
  `tower::Service` shapes…"* — seven faces, none of them the endpoint's.
  `PLAN.md` §3.2 adds `Endpoint::incoming() -> Incoming` as an eighth.
  **Working rule 8 says a list in the spec is read as exhaustive whether or
  not it says so**, and this project has seven rulings in that exact shape.
  Either §16.11's list is under-scoped or `PLAN.md` §3.2 is over-scoped. I am
  not resolving it.
- **`Incoming` holds a claim where `accept()` releases one.** In a `select!`,
  a bare `accept()` future is dropped on every unselected branch and the claim
  is released; an `Incoming` is *not* dropped (only the `Next` future is), so
  its `oneshot` stays outstanding and the driver may hand an `Intro` into it
  between polls. That is still *at most one item, requested from inside
  `poll_next`*, so I read it as compliant with ruling 58 — but it is a
  behavioural difference from the verb it wraps, it moves one `Intro` out of
  §6.3's queue and therefore out of `INTRO_TTL`'s reach, and dropping
  `Incoming` mid-claim is §6.2's silent reject (which `accept()`'s rustdoc
  already documents as *"not a loss"*).

**Recommendation:** add `pub(crate) fn poll_accept(&self, cx: &mut Context<'_>,
pending: &mut Option<oneshot::Receiver<Intro<I>>>) -> Poll<Option<Intro<I>>>`
to `Endpoint`, and write `accept()` as `poll_fn` over it — ruling 53's
"written once" rule applied to the one verb that currently escapes it. The
adapter then stores a `oneshot::Receiver`, which is `Unpin`, instead of a
boxed future. ~25 lines, no public signature change. **Blocks:** `CONTRACT-8.md`
§4's `Incoming` definition. **If you decline**, `Incoming` stores
`Option<Pin<Box<dyn Future<Output = Option<Intro<I>>> + 'a>>>` and I will
write that into the contract instead — it works, it is just the shape the
spec's own rationale argues against.

---

### Q4 — `Cargo.toml` and `src/lib.rs` say the `tower` feature is "over the message verb"; `PLAN.md` §3.4 says that shape cannot work

Three statements, and two of them disagree:

| Where | What it says |
|---|---|
| `Cargo.toml`, `tower` feature comment | *"A `tower_service::Service` shape over **the message verb**. Slice 8."* |
| `src/lib.rs:158`, the crate feature table | *"a `tower::Service` shape over **the message verb**"* |
| `PLAN.md` §3.4 | *"slither has **no request/response correlation on the wire**, so a `Service` over messages would need a request id the transport does not carry. The honest fit is **one bi stream per call** — the stream **is** the correlation."* |

`PLAN.md` §3.4 is the later and reasoned position, and §16.11 is neutral
(*"any `tower::Service` shapes are ordinary adapters over §16.2's verbs under
ruling 58 and are otherwise unconstrained"*). **I am reporting this rather
than picking**, per working rule 3 — but I note this is a case where the
*rationale* is on the prose side and the two "formal" statements are manifest
comments carrying no argument at all, so the tiebreak working rule 3 names
(*follow the statement some other proof depends on*) points at `PLAN.md` §3.4.
**Recommendation:** confirm §3.4, and have Agent A correct both comments in
the same slice (`Cargo.toml` is the integrator's file, so that correction is
the integrator's line, not Agent A's).

**A second question rides on the same feature.** `PLAN.md` §3.4's `Rpc<C:
Codec>` is defined in terms of `C::Item`, i.e. `tokio_util::codec` — so
`Rpc` cannot compile under `tower` alone. Either `tower = ["codec",
"dep:tower-service"]`, or `Rpc` is gated `#[cfg(all(feature = "tower",
feature = "codec"))]`. **Recommendation:** the `cfg(all(..))` gate, so that
`tower` stays the cheap dependency `PLAN.md` §3.4 describes ("tower-service
only") and only `Rpc` costs `tokio-util`. Whichever you choose changes
`tests/story_tower.rs`'s `required-features`, so it must be settled **before**
Agent B is briefed.

---

### Q5 — how much `tower` is in scope? `serve()` needs a signature and a `'static` story

`PLAN.md` §3.4 sketches `pub async fn serve<S>(conn: &Connection, svc: S) ->
Result<(), ConnectionLost>` with *"accept_bi loop → spawn_local per stream"*.
Three problems with taking it literally:

1. **`S` collides.** `Connection` is `Connection<S: Handshake>` in the real
   code. The contract renames the handshake parameter to `H` throughout.
2. **`spawn_local` requires `'static` futures.** `conn` is borrowed, but the
   `BiStream<H>` handed to the spawned task owns its `Rc`s and is `'static`,
   so the shape works — provided the bound is written
   `Svc: Service<BiStream<H>, Response = ()> + Clone + 'static, Svc::Future:
   'static`. **No `Send` anywhere** (S21).
3. **Sizing.** `PLAN.md:301` budgets slice 8 at ≈1.5k lines *including tests*.
   Two `Service` impls plus `Rpc` plus `serve` plus their rustdoc is most of
   that budget on the least-specified module in the slice.

**Recommendation:** ship T5 as the two connector `Service` impls plus `serve`,
and treat `Rpc` as the part to cut if the slice runs long — S33's acceptance
text ("*a `Service` call opens one bi stream, writes the request, finishes,
reads the response to EOF*") is satisfiable by the `Service<()> for
Connection` impl plus `serve`, without `Rpc`. Confirm or overrule; Agent B's
S33 suite depends on which.

---

### Q6 — slice 7 is not closed. Do not cut slice 8's worktrees from a red tree

Ruling 204, `rulings.md:5673`: *"It is a review of signatures, taken against a
tree with one failing test. Ruling 203's defect is sender behaviour … **slice
7 still does not close until it is fixed**."* The same ruling says: *"Slice
8's compatibility layer is **not** ratified here — it is unbuilt."*

Working rule 7 forbids reporting a gate green without running it, and three
blind agents cut from a tree with a known-failing test will each report a red
gate they did not cause and cannot fix. **Recommendation:** close ruling
203's defect and land slice 7 first; cut slice 8's three worktrees from that
green commit, and name it in all three briefs. **This blocks dispatch, not
planning.**

---

### Q8 — §16.11 gives two `io::ErrorKind` mappings for nine variants, and the one it gives is a slash

§16.11 and ruling 55's block state the conversions as:

```rust
impl From<ReadError>  for std::io::Error {}   // Reset → ConnectionReset
impl From<WriteError> for std::io::Error {}   // ConnectionLost → NotConnected / BrokenPipe
```

`Reset → ConnectionReset` is unambiguous. The other is **a slash between two
kinds with no rule for choosing**, over a `ConnectionLost` with **seven**
variants — and it says nothing about `WriteError::Finished` at all. Working
rule 8: *"when the spec introduces a symbol, a parameter or a list, ask what
bounds it, and whether the text says."* Here it does not.

`CONTRACT-8.md` §8 carries a full nine-row recommendation with the rule behind
it written out so it can be judged rather than memorised: **on the write side,
a peer or transport that went away under a writer is `BrokenPipe`; a
connection this side never had or gave up is `NotConnected`; `TimedOut` is
lifted out of both because `io::ErrorKind::TimedOut` exists and a 25 s
`DEAD_TIMEOUT` is precisely what it names.** That reading treats §16.11's
slash as a **variant** split rather than a read/write direction split, which
is how it is written — the comment sits on the `WriteError` line alone.

**This needs ratification before dispatch**, because Agent C's suite asserts
the table row by row and a table settled at integration is a table two agents
disagreed about in public. **Depends on:** nothing. **Blocks:** Agent C.

---

### Q9 — do the `Result`-carrying streams end, or report the death for ever?

`PLAN.md` §3.2 gives the item types (`Stream<Item = Result<Vec<u8>,
ConnectionLost>>`) and says nothing about termination. Both answers are
defensible and they are **not** distinguishable by a test author reading the
item type:

- **Never end** — `Some(Err(lost))` for ever. Faithful to the verb: the
  underlying `poll_*` re-report the latched death indefinitely, and
  `ConnectionLost` is `Clone` *for that reason* (`src/error.rs:193`: *"a
  connection dies once and the same value is handed to N awaiting futures and
  to every later verb call"*). No adapter state.
- **End after one error** — `Some(Err(lost))`, then `None`. One `bool` per
  adapter (not a queue; ruling 58 untouched), a terminating
  `while let Some(item) = s.next().await`, and the shape `Framed` uses.

**Recommendation: never end**, on the grounds that an adapter that terminates
is *deciding* something the verb it wraps does not, and a consumer that wants
the terminating shape writes `.take_while(…)` in one line — whereas a
consumer that wants the reason back after a `None` cannot recover it. I hold
this weakly; the ergonomic argument is real.

What I hold **strongly** is that it must be answered before Agent B and Agent
C are briefed. It is the same class as ruling 119's `Ok(Some(0))` versus
`Ok(None)` — *"the convention whose inversion hangs a reader forever on a
finished stream"*, which a blind author has already guessed wrong twice in
this project.

---

### Q10 — the adapters must borrow, because neither handle is `Clone`. Confirm that is acceptable

`Connection<S>` and `Endpoint<I>` have **no `Clone` impl** (only the internal
`Shell<I>` does), and `Connection`'s drop rule is load-bearing — *"dropping
the last handle to this connection performs `close(NO_ERROR, "")`"*. So every
adapter borrows: `Messages<'a, S>`, `Incoming<'a, I>`, and so on.
`PLAN.md` §3.2 writes them without lifetimes (`pub fn messages(&self) ->
Messages;`), which reads as owning.

The consequence a consumer meets: **a borrowed adapter cannot be moved into
`spawn_local`**, so the "spawn a task per connection that drains
`messages()`" shape needs the `Connection` moved in and the adapter built
inside the task. That is workable and is what the rustdoc should show.
**Recommendation:** borrow, with the lifetime written into every adapter type
and an example on each showing the spawn-local shape. The alternative —
making `Connection: Clone` — changes a ratified handle's semantics (the last-
handle drop rule) and is emphatically **not** slice 8's to do.

---

## 1. Scope

`src/compat/{mod,io,stream,codec,tower,rt}.rs` — `AsyncRead`/`AsyncWrite` on
the three stream handles, the `io::Error` conversions, `Stream`/`Sink`
adapters behind `feature = "sink"`, `tokio_util::codec` constructors behind
`feature = "codec"`, a `tower::Service` shape behind `feature = "tower"`, and
the `block_on` `LocalSet` helper.

**The three cargo features already exist in `Cargo.toml`** with their optional
dependencies wired, written when the manifest was first laid down:

```toml
sink  = ["dep:futures-core", "dep:futures-sink"]
codec = ["sink", "dep:tokio-util"]
tower = ["dep:tower-service"]
```

So the implementer adds **no dependency and no feature**. See Open question 4
for the one thing that manifest does say and `PLAN.md` §3.4 contradicts.

### Out of scope, recorded so it is not re-proposed

- **A `Send` façade / `bridge`** — declined at `rulings.md:~918`, *"deferred
  past v0.2 **by design, not omission**"*. `PLAN.md` §3.4 repeats it.
- **Companion crates for the adapters** — declined in the same block, in
  favour of in-crate additive features.
- **Any change to a public handle signature** — ruling 204. Slice 8 is
  purely additive.
- **Promotion of the `poll_*` verbs to `pub`** — ruling 122(a) put them at
  `pub(crate)` *because* slice 8's `compat/` is in-crate and reaches them,
  and because *"publishing a `key: u64` parameter now"* is not additive.

### A scheduling fact the plan must state

`rulings.md:5675` (ruling 204): *"Ruling 203's defect is sender behaviour …
slice 7 still does not close until it is fixed."* Ruling 204 also records the
ratification was *"taken against a tree with one failing test"*, and says
explicitly: **"Slice 8's compatibility layer is not ratified here — it is
unbuilt."** Slice 8 must not be dispatched onto a red tree; see Open
question 6.

## 2. Inputs consulted

| Input | Location | Read as |
|---|---|---|
| Working rules | `CLAUDE.md` | binding on this plan and on every slice-8 agent |
| Composability design | `PLAN.md` §3, lines 138–268 | starting point, not a blank page |
| Stories | `STORIES.md` S25, S31–S33 | acceptance criteria |
| Spec | `SPEC.md` §16.11, lines **5822–5908** (section ends at 5908; §17 opens at 5909) | authority |
| Ruling 55 | `rulings.md:874` | `open_bi`/`accept_bi` yield `BiStream`; `.split()` yields the pair |
| Ruling 56 | `rulings.md:879` | `poll_flush` no-op returning `Ready` |
| Ruling 57 | `rulings.md:885` | `poll_shutdown` = `finish()` then `acked()` |
| Ruling 58 | `rulings.md:894` | **normative**: an adapter never claims ahead of its consumer |
| Ruling 96 | `rulings.md:2194` | `BiStream` *the type* landed in slice 4; its `AsyncRead`/`AsyncWrite` impls and `compat/` are slice 8 |
| Ruling 204 | `rulings.md:~5660–5720` | ratifies the public handle API as it stands; slice 8 changes no handle signature |

**Adjacent rulings found while reading, load-bearing for this slice and not
in the brief** — they are the semantics a blind test author will otherwise
guess:

- **Ruling 110** (`rulings.md:2575`) — a zero-length `write` is a no-op
  returning `Ok(0)`, and `Ok(0)` means *blocked* only for a non-empty input.
  `AsyncWrite` combinators hand empty buffers to `poll_write` routinely.
- **Ruling 119** (`rulings.md:2941`) — an empty `buf` short-circuits `read`
  to `Ok(Some(0))` without touching the core. `Ok(Some(0))` = park,
  `Ok(None)` = end of stream. *A blind test author has already guessed this
  wrong once* (round 18, and again in slice 4a per working rule 14).
- **Ruling 120** (`rulings.md:2953`) — `BiStream::join(send, recv) ->
  Result<BiStream, (SendStream, RecvStream)>`; checks both halves name the
  same `StreamRef` on the same `ConnectionId`, hands the pair back on
  mismatch. **Rejection is a `Result`, never a panic** (ruling 44).
- **Ruling 121** (`rulings.md:2966`) — `ReadError::Reset` is **sticky at the
  handle**: the handle latches its terminal outcome and re-reports it, so a
  reset stream never reads as a clean `Ok(None)` EOF. This is directly
  load-bearing for the `AsyncRead` impl and for the `Stream` adapters.
- **Ruling 122(a)** (`rulings.md:2983`) — the `poll_*` forms are
  **`pub(crate)`**, and the ruling names slice 8's `compat/` explicitly as
  the in-crate consumer that reaches them. Promotion to `pub` later is
  additive; publishing a `key: u64` parameter now is not.
- **Ruling 54** (`rulings.md:862`) — `Connection::flush()` was renamed
  `acked()` *because of* the `AsyncWrite` surface. Do not reintroduce a
  `flush` that means acknowledgement.
- **Declined and recorded, do not re-propose** (`rulings.md:~918`): a `Send`
  façade (deferred past v0.2 **by design, not omission**), and companion
  crates for the adapters (rejected in favour of in-crate additive features
  `sink`, `codec`, `tower`).

## 3. Stories in scope

**Finding, reported not resolved (working rule 3 / rule 5): S31–S33 are not
in `STORIES.md`.** The brief names them as "`STORIES.md` … S31–S33". They do
not exist there. `STORIES.md`'s headings run `S1`…`S30` and its own D10 entry
records *"`STORIES.md` is complete at **30 approved stories**"*.

S31–S33 live in **`PLAN.md` §7, lines 356–378**, under the heading *"Stories
owed by this plan"*, and that section says of them: *"**Drafted here, to move
into `STORIES.md` on approval.**"* `PLAN.md`'s own slice table (line 288)
agrees — slice 8's stories column reads *"S25, and S31–S33 **(drafted, §7)**"*.

`CLAUDE.md` states the bar as *"`STORIES.md`'s 30 approved capability stories
are the acceptance criteria: a slice is done when its stories are
paused-clock tests that pass"*. On the literal text slice 8 has exactly one
approved story (S25) and three drafts. **See Open question 1.**

### S25 — a user can supply the wire, and a send failure is explicable *(approved, RESCOPED by ruling 49)*

`STORIES.md:378`. Slice 8 does **not** re-litigate S25; it is listed against
slice 8 because the `io::Error` conversions are where an application meets a
`Wire` failure in `io` terms. What S25 pins that slice 8 must not violate:

- A failing `send_to` does **not** kill a connection and raises **no
  application error**; it is *traced*, not acted on. Any adapter that turns a
  wire-level send failure into an `io::Error` on `poll_write` contradicts
  this. **The `io::Error`s in slice 8 come from `WriteError`/`ReadError`, never
  from the `Wire`.**
- §18.1's taxonomy stays **closed**. Slice 8 adds no error type. (Confirmed
  against ruling 204 item 2: nine of ten enums are exhaustively matchable;
  `WriteError` alone is `#[non_exhaustive]`, reserving `Stopped`.)

### S31 — a user can treat a stream as an `AsyncRead`/`AsyncWrite` *(draft, `PLAN.md:362`)*

Verbatim acceptance shape: `tokio::io::copy` a file into a `BiStream` behind
a `BufWriter`, `shutdown()`, and the peer reads identical bytes and observes
EOF. Over `FlakyWire` with loss, on the paused clock. Errors arrive as
`io::Error`, and **a reset surfaces as `ConnectionReset` rather than a silent
truncation**.

*Note the last clause is exactly ruling 121's sticky-`Reset` behaviour seen
through the `AsyncRead` surface. If the latch is not honoured, `copy` returns
`Ok(n)` on a truncated transfer — the story's stated failure.*

### S32 — a user can stream typed objects with a codec *(draft, `PLAN.md:368`)*

`Framed<BiStream, LengthDelimitedCodec>` round-trips a sequence of objects in
order; `Stream`/`Sink` backpressure maps onto flow-control credit rather than
an intermediate buffer, and **the no-prefetch invariant (D5 / ruling 58) is
asserted by a consumer that polls once and checks that exactly one item was
claimed**.

### S33 — a user can drive slither from a `tower::Service` *(draft, `PLAN.md:374`)*

A `Service` call opens one bi stream, writes the request, finishes, reads the
response to EOF, and **concurrent calls do not head-of-line block each
other**. `serve()` drives the accepting side. The `!Send` boundary is
asserted: `UnsyncBoxService` composes, and §3.4's caveats are **rustdoc, not
folklore**.

### Stories slice 8 must not break

- **S30 / rulings 51, 52, 59** — `incoming_uni()` and `messages()` draw from
  the same supply and must not both be used. Both adapters' rustdoc says so.
- **S28 / ruling 57** — `poll_shutdown` must not lose the tail.
- **S21** — the Secure Enclave story is why there are **no `Send` bounds**
  anywhere on the actor path.

## 4. Invariants that bound the slice

Each one is quoted, not paraphrased, because each is a way this slice gets
built wrong.

1. **§16.11 / ruling 58 — an adapter never claims ahead of its consumer.**
   *"A `Stream` adapter over `recv_message`, `recv_datagram`,
   `accept_bi`/`accept_uni` or `notified()` claims **at most one item, and
   only from inside `poll_next`**. No prefetch, no read-ahead task, no
   intermediate queue."* **Normative**, and *"Appendix B pins it"*.
   The same rule is already written from the other end in
   `src/shell/shared.rs:~655`: *"An implementer who reaches for a `VecDeque`
   has rebuilt the unbounded intermediate queue §10.6 forbids — and ruling
   58's 'an adapter never claims ahead of its consumer' is the same rule
   from the other end: `take_oldest` hands over **one** item, and only from
   inside a `poll`."*
2. **Ruling 53 / §16.3 — each data-path verb is written once.** SPEC.md:5174:
   *"Each data-path verb is written **once**, as `poll_*(&mut self, cx) ->
   Poll<_>`; §16.2's `async fn` is then `poll_fn(|cx| self.poll_*(cx,
   ..)).await`, and §16.11's `AsyncRead`/`AsyncWrite` is the *same function*
   with its error mapped."* The adapters **reuse** the `pub(crate) poll_*`;
   they do not re-implement a verb.
   The same passage names the failure mode this avoids: *"every adapter in
   §16.11 must box and store an in-flight future, `Unpin` becomes delicate
   (a stored future has already copied a buffer that the next `poll_write`
   may not pass again), and every write costs a round-trip and an
   allocation."*
3. **Ruling 56 — `poll_flush` is a no-op returning `Ready`**, and
   *"Rustdoc must say so at the impl."* `AsyncWrite::flush` is **not**
   delivery confirmation. Ruling 54 renamed `Connection::flush()` to
   `acked()` precisely so the two cannot be confused; do not undo that.
4. **Ruling 57 — `poll_shutdown` is `finish()` *and then* `acked()`.**
   Bounded by connection death, so it cannot hang past `DEAD_TIMEOUT`.
5. **No `Send` bounds anywhere on the actor path.** S21 (Secure Enclave) is
   the story that forces it. `compat` must not add `Send` to any bound, and
   must not use `tokio::spawn` — `spawn_local` is the substitute.
6. **S30 / ruling 51 — `incoming_uni()` and `messages()` draw from the same
   supply and must not both be used.** The rustdoc on **both** says so.
7. **No shell-side scratch buffer** (§10.6). `RecvStream::read`'s existing
   rustdoc: *"Bytes are read straight into the caller's `buf` — there is no
   shell-side scratch buffer … so there is nowhere for a dropped future to
   strand data."* An `AsyncRead` impl that reads into a `Vec` and copies out
   violates this.
8. **The `io::Error`s come from `WriteError`/`ReadError`, never from the
   `Wire`** (S25 / ruling 49): a failing `send_to` does not kill a
   connection and raises no application error.
9. **§18.1's taxonomy stays closed.** Slice 8 adds **no error type**.
   (Ruling 204 item 2; ruling 61 for `WriteError`'s `#[non_exhaustive]`.)
10. **Ruling 121 / 124 — terminal state is sticky at the handle and answers
    ahead of the connection's death latch.** SPEC.md:4805: *"§16.11 makes
    this load-bearing rather than tidy — `AsyncRead` requires a sticky
    end-of-file, so a connection that dies after the FIN would otherwise
    surface a spurious `io::Error` to `read_to_end`."* The latch is already
    in `RecvStream`; the adapter must not defeat it.
11. **Rulings 110 / 119 — the empty-buffer rules.** A zero-length `write` is
    `Ok(0)` and is **not** "blocked"; an empty `buf` short-circuits `read` to
    `Ok(Some(0))` without touching the core. Both are reachable from
    ordinary `tokio::io` combinators, which is why ruling 110 exists.
12. **`BiStream` has no `Drop` of its own** and its halves drop in field
    order. A `compat` wrapper must not add one.

## 5. Task breakdown

Sizing target from `PLAN.md:301` is **≈1.5k lines**, implementation plus
tests — the smallest slice after slice 6. Anything much larger means the
`tower` module has grown a framework; see Open question 5.

### T0 — prerequisite: reach the `poll_*` verbs from `compat/` *(implementer)*

Ruling 122(a) made the `poll_*` verbs `pub(crate)` naming slice 8 as their
consumer. **It did not promote the thing they need to be called with.** Every
`Connection` `poll_*` takes a `key: u64`, and the key comes from a
`WakerSlot` minted by an accessor that is **private to
`shell::connection`**, not `pub(crate)`:

```
src/shell/connection.rs:279  fn notification_slot(&self)   -> WakerSlot<impl FnMut(u64)>
                       :477  fn settled_slot(&self)        -> …
                       :919  fn message_reader_slot(&self) -> …
                       :928  fn datagram_reader_slot(&self)-> …
                       :937  fn message_sender_slot(&self) -> …
                       :952  fn opener_slot(&self, dir)    -> …
                       :961  fn acceptor_slot(&self, dir)  -> …
```

`crate::compat` is a different module, so it cannot call them. **This is
working rule 8's defect class exactly** — a stated construction (ruling
122(a)'s "slice 8's `compat/` is in-crate and reaches them") with an
unstated scope. See **Open question 2**, which also covers the second half of
the problem: `WakerSlot<impl FnMut(u64)>` is **unnameable as a struct
field**, and every `Stream` adapter has to store its slot for the adapter's
whole life.

### T1 — `compat/io.rs` (no feature gate)

- `impl From<ReadError> for io::Error`, `impl From<WriteError> for io::Error`
  — §8's table below is exact and binding.
- `impl<S: Handshake> AsyncWrite for SendStream<S>` — `poll_write` over
  `SendStream::poll_write`; `poll_flush` = `Ready(Ok(()))` **with the rustdoc
  ruling 56 demands**; `poll_shutdown` = `poll_finish` then `poll_acked`.
- `impl<S: Handshake> AsyncRead for RecvStream<S>` — over
  `RecvStream::poll_read`, into `ReadBuf`'s uninitialised tail, no scratch
  buffer.
- `impl<S: Handshake> AsyncRead for BiStream<S>` and `AsyncWrite for
  BiStream<S>` — delegating to the halves.
- **Not** a new `Drop`, **not** a new error type.

### T2 — `compat/rt.rs` (no feature gate)

`pub fn block_on<F: Future>(f: F) -> F::Output` — current-thread runtime
inside a `LocalSet`. Plus the copy-pasteable example `PLAN.md` §3.5 asks for,
because *"every consumer must build a current-thread runtime inside a
`LocalSet`, and that is the first thing they will hit."*

### T3 — `compat/stream.rs` (`feature = "sink"`)

Seven `Stream`s and two `Sink`s, each **one item, only inside `poll_next` /
`poll_ready`+`start_send`**. Each is a thin struct over the borrowed handle
plus its `WakerSlot`. The rustdoc on `messages()` **and** `incoming_uni()`
carries S30's warning (ruling 51).

`Endpoint::incoming()` is the one that is **not** a `poll_*` — see Open
question 3.

### T4 — `compat/codec.rs` (`feature = "codec"`)

Two constructors, and nothing else: they *"exist only to remove a `use`"*
(`PLAN.md` §3.3). `framed_bi(codec)` and `framed_uni(...)` — see the contract
for the exact shapes.

### T5 — `compat/tower.rs` (`feature = "tower"`)

The two connector `Service` impls, `Rpc`, and `serve`. Smallest module that
satisfies S33; see Open questions 4 and 5.

### T6 — wiring

`src/lib.rs` gains `pub mod compat;` and the re-exports. The feature table in
the crate-level rustdoc (`src/lib.rs:155–158`) already lists all four
features — **one of its rows is wrong**; see Open question 4.

### T7 — tests

Three story suites and one spec suite, in `tests/`. See §6 for ownership and
§7 for the manifest stanzas that make them runnable.

## 6. Three-way blind dispatch — file ownership

**Working rule 6: parallel agents own disjoint file paths, no exceptions.**
In slice 2a an implementer's placeholder stub destroyed 68 independently
written tests because two briefs named `src/core/tests.rs`; in slice 1 the
same overlap existed and the finish order happened to favour the tests, which
is why nobody noticed. The table below is the whole of the contract on this
point — **if a path is not in exactly one row, the dispatch has a race in
it.**

### Agent A — implementer

Blind to B and C. Briefed with `CONTRACT-8.md`, `PLAN-8.md`, `PLAN.md` §3,
§16.11 quoted in full, and rulings 55–58, 96, 110, 119, 120, 121, 122, 204.

| Owns (exclusively) | New? |
|---|---|
| `src/compat/mod.rs` | new |
| `src/compat/io.rs` | new |
| `src/compat/rt.rs` | new |
| `src/compat/stream.rs` | new |
| `src/compat/codec.rs` | new |
| `src/compat/tower.rs` | new |
| `src/lib.rs` | existing — adds `pub mod compat;`, the re-exports, and the feature-table fix |
| `src/shell/connection.rs` | existing — **T0 visibility promotion only** |
| `src/shell/endpoint.rs` | existing — **T0 / Open question 3 only** |
| `src/shell/stream.rs` | existing — **only if T0 needs it**; no behaviour change |

**Agent A creates no file under `tests/` and does not touch `Cargo.toml`.**
It declares the in-crate test module **commented out**, under an integration
header, exactly as slice 4b's implementer did with its `[[test]]` stanzas:

```rust
// Slice 8. The integrator's to uncomment (working rules 6 and 15): the file
// is the spec-test author's alone, and `mod tests;` naming a missing file is
// a compile error on which no gate can run.
// #[cfg(test)]
// mod tests;
```

### Agent B — story test author (S31, S32, S33)

Blind to A and C. Briefed with `CONTRACT-8.md`, `PLAN.md` §7's three story
drafts **verbatim**, `STORIES.md` S25/S28/S30 for the stories slice 8 must
not break, and the paused-clock fixture conventions from an existing suite it
is told to read (`tests/story_streams.rs`).

| Owns (exclusively) |
|---|
| `tests/story_compat.rs` — S31 |
| `tests/story_codec.rs` — S32 |
| `tests/story_tower.rs` — S33 |

### Agent C — spec / invariant test author (§16.11 and its rulings)

Blind to A and B. Briefed with `CONTRACT-8.md`, §16.11 quoted in full, and
rulings 56, 57, 58, 110, 119, 121.

| Owns (exclusively) |
|---|
| `tests/spec_compat.rs` — the ruling-58 no-prefetch pin, ruling 56's flush, ruling 57's shutdown, the §8 error-mapping table, the empty-buffer rules |
| `src/compat/tests.rs` — **only** if Open question 2 lands on an in-crate accounting assertion; otherwise this file does not exist |

### Why two test authors and not one

S31–S33 are *capability* stories over `FlakyWire` on the paused clock; §16.11's
rulings are *pinned invariants* asserted by direct polling. They fail
differently and a single author tends to write the second as a weaker version
of the first. **Working rule 9 is the specific risk here:** the ruling-58 pin
is the easiest bound in this slice to write so that the broken implementation
satisfies it for free.

**Ruling-58 pin, stated as a requirement on Agent C rather than left to
taste.** *A test that polls a `Stream` once and asserts one item arrived
passes a prefetching adapter.* The assertion must separate the two from the
side that distinguishes them: send **three** messages, poll `messages()`
**exactly once** (`Waker::noop()`, stable in the 1.96 MSRV — no dev-dependency
needed), then assert that `conn.recv_message()` **immediately** yields the
**second** message. A prefetching adapter has swallowed messages two and
three, so that call parks and the test fails. Working rule 9: ask what the
broken version does, and assert from the side that separates them.

### Blindness mechanics (working rule 14)

- Each agent gets a **git worktree cut from a commit that already contains
  `PLAN-8.md` and `CONTRACT-8.md`**. Slice 4a's author guessed an API for ten
  minutes because its contract was uncommitted at the moment its worktree was
  cut; slice 7's third author was briefed at one commit and cut from another
  that contained the implementation it existed to be blind to.
- The brief **names the commit**, and each agent's **first command** is
  `git log --oneline -1`, reported before it reads any source. *"An isolated
  agent's first act is to check its base and report it."*
- **Commit `PLAN-8.md` and `CONTRACT-8.md` before dispatching, not after.**

## 7. Integrator-owned files (rule 15 residue)

**Working rule 15: a file whose contents are only valid once *both* blind
agents' work exists belongs to the integrator, and the briefs must say so.**
Rule 6 partitions the paths; rule 15 names the residue.

### `Cargo.toml` — integrator only. No other agent touches it.

Three separate reasons, all load-bearing:

1. **The `[[test]]` stanzas.** Cargo does not warn about a `[[test]]` whose
   file is missing — it **refuses to parse the manifest**, so an implementer
   landing live stanzas for a partner's not-yet-existing test files commits a
   tree on which *no gate can run at all*, while working rule 7 forbids
   reporting a gate green without running it. And per **ruling 194**, already
   recorded in this manifest's own comment: there is no `autotests = false`,
   so cargo auto-discovers the targets **without** their `required-features`
   and the feature-less `cargo test` gate fails on `unresolved import
   slither::testutil` until the stanzas exist. They are not cosmetic.

   ```toml
   [[test]]
   name = "story_compat"
   required-features = ["test-util"]

   [[test]]
   name = "story_codec"
   required-features = ["test-util", "codec"]

   [[test]]
   name = "story_tower"
   required-features = ["test-util", "tower", "codec"]   # see Open question 4

   [[test]]
   name = "spec_compat"
   required-features = ["test-util", "sink"]
   ```

2. **Three dev-dependencies the current manifest does not have, and without
   which the story tests cannot compile.** A blind test author that discovers
   this has to guess, which is the slice-4a failure. Verified against the
   manifest at `5ed5bb4`:
   - **`tokio`'s `io-util` feature.** The dev-dependency reads
     `features = ["test-util", "rt", "macros", "time", "net", "sync"]`.
     S31's acceptance *is* `tokio::io::copy` into a `BufWriter` followed by
     `shutdown()` — all three live behind `io-util`. (The `AsyncRead` /
     `AsyncWrite` **traits** need no feature, which is why `compat/io.rs`
     itself compiles without it and the gap is invisible from the
     implementer's side. Whether `tokio-util/codec` already pulls `io-util`
     transitively should be **verified, not assumed**.)
   - **`futures-util`** (or `futures`), dev-only. `futures-core`/`futures-sink`
     supply the *traits*; `StreamExt::next()` and `SinkExt::send()` — which is
     literally what S32's acceptance text writes — are in `futures-util`.
   - **`tower`** with `features = ["util"]`, dev-only. S33's acceptance asserts
     *"`UnsyncBoxService` composes"*, and `UnsyncBoxService` is in `tower`,
     not in the `tower-service` the library depends on. `PLAN.md` §3.4's
     "tower-service only" is right about the **library** dependency and does
     not reach the test profile.

3. **Uncommenting Agent A's `#[cfg(test)] mod tests;`**, if Open question 2
   lands on an in-crate test file.

### The integrator's own checks

- Re-read `CONTRACT-8.md` against what both sides actually built and report
  every divergence as a **finding**, not a fix. Slice 4a's integration read as
  a design disagreement when it was really a missing file.
- **Working rule 16**: while another agent holds the tree, read from the
  commit — `git show <base>:<path>` — never from the working copy, and never
  `git add -A`. Ruling 166 produced two rulings from one contaminated read,
  one of which publicly faulted an agent that had been correct.
- **Working rule 10**: commit before mutating.

## 8. Gates

**Every slice ends on the full table, not just `cargo test`** (`CLAUDE.md`).
Working rule 7: do not report a gate green without running it — paste the
command and its output.

| Gate | Command |
|---|---|
| Compiles | `cargo build --all-features --all-targets` |
| Format | `cargo fmt --all --check` |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` |
| Docs | `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` and `--all-features` |
| Tests | `cargo test` **and** `cargo test --all-features` |
| Release tests | `cargo test --release --all-features` |
| Wire pins | golden-wire + size/constant tests (under `cargo test`) |
| MSRV | `cargo +1.96 check --all-features --all-targets` |
| Supply chain | `cargo deny check` |

### Slice-8-specific gate hazards

- **The feature lattice is not covered by `--all-features` alone.** `codec`
  implies `sink`; `tower` does not. Add to the slice's own checks:
  `cargo check --features sink`, `--features codec`, `--features tower`,
  `--features "tower,codec"`. The manifest comment on `codec` states the
  requirement in terms — *"enabling `codec` alone must not be a compile
  error"* — and only a single-feature build tests it.
- **`cargo deny check`** sees four new crates in the graph for the first time
  (`futures-core`, `futures-sink`, `tokio-util`, `tower-service`, plus the
  dev-only `futures-util` and `tower`). They are declared but have never been
  resolved, because nothing enabled the features. Run it early, not at the
  end.
- **`RUSTDOCFLAGS=-D warnings`** with `#![warn(missing_docs)]` already on the
  crate: every new public item needs a doc comment, and rulings 56 and 57 both
  require **specific** rustdoc at the impl, not merely some.
- **The wire pins must stay byte-identical.** Slice 8 touches no wire byte; if
  a golden-wire test moves, that is *"this needs a ruling"*, not *"update the
  expectation."*

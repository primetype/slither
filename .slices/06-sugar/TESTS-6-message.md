# TESTS-6-message — blind test author's report (§9.8, S16 + S30)

Worktree cut from `f19cb99`. Owned path: `tests/story_message.rs`.
Written from `STORIES.md`, `SPEC.md` §9.8 and `.slices/06-sugar/CONTRACT-6.md`,
never from an implementation.

## 1. Inventory — 15 tests

PLAN-6 §5.2 names SM1–SM3 and §5.3 names SM4–SM9 for this file. SM8 is not
written (see §3). Six tests below are not in the plan's table: they come from
Round 26's rulings 150, 152, 153, 154 and 156, which post-date §5.

| # | fn | Pins | The **broken build** it separates |
|---|---|---|---|
| 1 | `s16_a_maximum_message_arrives_whole_in_one_call` | S16: 262 144 in one call, byte-identical, and *one* message | a build chunking below the bound delivers a short first message (content assert) or leaves a remainder (the second claim must stay `Pending`) |
| 2 | `s16_one_byte_over_the_bound_is_rejected_at_the_handle` | S16 two-sided: 262 145 → `TooLarge`, **and no packet leaves A**; then 262 143 → delivered | truncate-and-send returns `Ok`; a build rejecting *after* opening the stream moves A's send counter; a build rejecting everything fails the 262 143 half |
| 3 | `s16_an_empty_message_is_a_message` | CONTRACT §2.3: FIN at offset 0 ⇒ `Some(vec![])`, never `None` | a build folding empty into "nothing to claim" parks the receiver for ever — `within` panics instead of hanging the suite |
| 4 | `s16_messages_are_claimed_in_open_order_and_none_is_lost` | ruling 112 FIFO among simultaneously-complete messages | a stack-ordered `unclaimed` returns the newest first; a build that loses one leaves the third claim pending |
| 5 | `s16_an_unclaimed_complete_message_is_retained_until_claimed` | **ruling 154** — retention until claimed; §10.3's struck fifth trigger | a build retiring the receive half at the FIN has discarded the payload; the later claim never yields |
| 6 | `s16_message_then_acked_then_close_delivers_after_the_death` | Appendix B:6257-6265 + **ruling 152**: B's `closed()` resolves **first**, then `recv_message()` still yields `m` in full; a further claim is a **prompt** `Err`, never a park | a shell checking the death latch before the core call returns `Err` — CONTRACT §2.5's precedence, inverted; a shell that parks on a dead connection hangs (ruling 128) |
| 7 | `s16_without_acked_the_same_drop_set_loses_the_message` | the negative Appendix B demands: the pre-ruling-47 ordering **fails** at the same drop set | without it, test 6 passes for free on a build that never loses anything — a wire this test proves is lossy |
| 8 | `s16_send_message_admits_the_whole_payload_or_nothing` | **ruling 150** / CONTRACT §2.5's cancel-safety sentence | the natural decomposition (open → write-loop → finish) admits the 1 000 bytes of credit left and parks; the dropped future leaves a FIN-less stream that B's `accept_uni()` then yields |
| 9 | `s30_a_conforming_maximum_message_is_not_reset` | **ruling 153** — the highest-value test in the file | a build emitting a separate empty FIN frame, or a predicate that ignores the pinned final size, resets a *conforming* 262 144-byte message with `MESSAGE_OVERFLOW` — ruling 59's "transfers die at 256 KiB" post-mortem, pointing at the wrong cause |
| 10 | `s30_an_unclaimed_window_full_uni_stream_is_reset_with_message_overflow` | S30/Appendix B (i)(ii)(iv): `Err(Reset(0x06))`, **not `0`**, and no liveness death | pre-ruling-51 stalls for ever with keepalives flowing; pre-ruling-52 carries `0`; a build that killed the connection instead would also resolve the write in error — (iv) is what stops that passing |
| 11 | `s30_the_overflow_reset_trues_up_connection_credit` | S30/Appendix B (iii) | with no true-up, four resets leave 1 MiB of dead weight = exactly `INITIAL_MAX_DATA`, and the next `send_message` parks for ever with no error |
| 12 | `s30_a_stream_behind_a_slower_one_is_reset_on_its_own_account` | §9.8:3181-3182 — **every** unclaimed window-full stream | `if let Some(oldest) = unclaimed.front()` inspects the small incomplete older stream, finds it under the bound, and leaves the newer one stalled |
| 13 | `s30_a_slow_accept_uni_receiver_is_not_reset` | §9.8:3183-3188 — the guard, and what it buys | the unguarded form (the blanket instruction ruling 51's agent refused) resets an ordinary lazy accept loop: A gets `Err(Reset)` where it must get progress |
| 14 | `s30_a_lost_overflow_reset_is_regenerated_until_acknowledged` | §9.6:3028-3039, §8.7 — retained until **acknowledged**, despite the retired half | a build that put the reset in the send half's `ResetState` loses it with the half; A hangs for ever |
| 15 | `s30_the_pending_claim_flag_clears_on_a_successful_claim` | **ruling 156**, in three phases | phase 2 separates a flag never cleared (it resets a stream with no claim outstanding, breaking the guard); phase 3 separates a build that scans only on inbound data and not *at* the claim |

## 2. Conflicts found — reported, NOT resolved (working rule 3)

### F-1 — §9.8:3146's parenthetical asserts as a *fact* the thing ruling 153 had to add as a *conjunct*

`SPEC.md:3145-3147` states the rule as:

> an **unclaimed** uni stream — neither claimed by `accept_uni()`, nor
> surfaceable by `recv_message()`, **which it cannot be, having no FIN** —
> that reaches `MESSAGE_RECV_MAX` is **reset** by the receiver

The em-dashed clause is not a restriction on the rule; it is an *argument*
that FIN-less-ness follows from window-fullness. It is false at exactly one
payload size — `MESSAGE_RECV_MAX` — where a conforming message has a pinned
final size **and** a highest received offset of exactly the bound. Ruling
153 fixes the predicate by adding "with no pinned final size" as an
independent conjunct, but **§9.8's own prose was not swept**: read alone it
still says the reset applies to anything that "reaches `MESSAGE_RECV_MAX`",
with a parenthetical explaining why the excluded case cannot arise.

This is working rule 8's shape (a stated construction whose scope the text
contradicts) *and* working rule 4's (a verification that greps for the
changed value misses the prose still arguing the reversed position). It is
also the exact defect my test 9 separates, which is why I am confident it is
real rather than a reading.

**Not resolved here.** Recommend §9.8:3145-3147 carry ruling 153's conjunct
in the rule sentence rather than as an aside.

### F-2 — §15.3's registry entry states the predicate **without** ruling 153's conjunct; Appendix B states it **with**

Two normative statements of the same predicate, in the same document,
disagreeing on exactly the clause F-1 is about:

- `SPEC.md:6041-6042` (Appendix B): *"a **FIN-less** window-filling uni
  stream under a pending `recv_message()` is reset"* — has the conjunct.
- `SPEC.md:4067` (§15.3 registry): *"an unclaimed uni stream reached
  `MESSAGE_RECV_MAX` while a `recv_message()` claim was pending"* — does
  **not**.

An implementer working from the registry entry — which is the natural place
to look up what `0x06` means — builds the version that resets a conforming
262 144-byte message. **Not resolved here.**

### F-3 — `CONTRACT-6.md` §2.3's core `send_message` signature contradicts ratified §16.4, and names an undefined type

- `SPEC.md:4864` (ratified §16.4 core API):
  `fn send_message(&mut self, now: Instant, msg: &[u8]) -> Result<(), MessageError>;`
- `CONTRACT-6.md:150-151`:
  `pub(crate) fn send_message(&mut self, now: Instant, msg: &[u8]) -> Result<SendMessage, MessageError>;`

`SendMessage` is defined nowhere in `CONTRACT-6.md`, nowhere in `SPEC.md`,
and is not listed in §2.7's prohibitions either. CLAUDE.md's first hard rule
is that the spec is the authority and the code matches it, so a blind
implementer has a contract that instructs it to diverge from a ratified
signature in favour of a type that does not exist.

**And the tension is real, not merely editorial.** Ruling 150 requires the
core to be able to say *"not now"* (no stream allowance, or insufficient
connection credit — the case for which it mints a `pub(crate)` `ConnEvent`).
`Result<(), MessageError>` cannot express that: §2.6 records that
`MessageError` is closed, already shipped, and already pinned by
`tests/spec_errors.rs`, so it may not gain a variant — §2.6 says so itself
("Q1's 'not now' must not be a `MessageError`"). So §16.4's ratified return
type and ruling 150's requirement are, as written, unsatisfiable together.

**This needs a ruling and is not mine to make.** It does not reach my file —
`tests/` sees only the shell's `Result<(), MessageError>`, which is
unambiguous in §16.2:4177 — but it lands squarely on the implementer.

### F-4 — `CONTRACT-6.md` §2.3 still carries ⚠ RULING REQUIRED markers for questions its own §0 answers, and one of them asserts something now false

§0's header says its table *"override[s] anything below"*. Yet:

| line | marker | answered by |
|---|---|---|
| `:149` | *"⚠ RULING REQUIRED on the return type — §8 Q1"* | §0 ruling **150**, twelve lines above |
| `:156` | *"⚠ RULING REQUIRED: `now` is added to §16.4's signature — §8 Q2"* | §0 ruling **151**; `SPEC.md:4869` already carries `// ruling 151` |
| `:186`, `:195` | *"⚠ see §8 Q5"* / *"⚠ §8 Q5 / §7 C-2: Appendix B's message-then-close obligation requires this **and no ruling says so**"* | §0 ruling **152** — so the quoted clause is **false**, and false about a ruling printed in the same file |

`PLAN-6.md:1195` goes further and instructs the datagram author to ship the
post-death test `#[ignore]`d pending Q5.

I reached this independently before the integrator's mid-flight correction
confirmed it, and I have written Appendix B's obligation as a **live** test
(#6), not an ignored one. Recording it here because working rule 11 is
explicit that a rationale must name a mechanism that exists: *"no ruling says
so"* names the absence of a ruling that is twelve lines away, and that is the
same defect class as rulings 87/89/90 in the maintainer's own text.

### F-5 — `Cargo.toml` has no `story_message` stanza and no `autotests = false`, so a feature-less `cargo test` breaks on this file

Not a spec conflict — a working-rule-15 residue, and it is load-bearing
because rule 7 forbids reporting a gate green without running it.

`Cargo.toml` declares six `[[test]]` targets, each with
`required-features = ["test-util"]`, and sets no `autotests` key. Auto-
discovery is therefore on, `tests/story_message.rs` is inferred as a target
**without** the feature gate, and a plain `cargo test` tries to compile it.
Confirmed, not assumed:

```
$ cargo build --tests
error[E0432]: unresolved import `slither::testutil`
error: could not compile `slither` (test "story_message") due to 1 previous error
```

`cargo test` is a release gate in its own right, separate from
`cargo test --all-features`, so this blocks the gate table until the stanza
lands. **It is the integrator's file, not mine** (working rule 15), and
`tests/story_datagram.rs` needs the identical stanza:

```toml
[[test]]
name = "story_message"
required-features = ["test-util"]
```

## 3. Not tested, and why

### U-1 — ruling 59's tracing MUST is **uncoverable from `tests/`**, and nothing else in slice 6 covers it either

S30's fourth `Accepts` bullet and `SPEC.md:3156-3159` are a **MUST**:

> The receiver **MUST** trace what it emitted, under §18.2's
> `slither::frames` (ruling 59) […] naming the stream, its final size, and
> the mode conflict.

PLAN-6 §5.3 assigns this to me as **SM8**. I have not written it, and I do
not believe a weak version should be written in its place.

**Why it cannot be reached.** `tests/*.rs` sees the public API plus
`slither::testutil`. There is no tracing-capture helper in `testutil` (I
enumerated its public surface: `Tap`, `Spied`, `FlakyPolicy`, `Network`,
`FlakyWire`, `SharedWire`, `Pair`, `Peer`, the counting identities, `local`,
`settle` — nothing touches `tracing`). And `tracing-subscriber` is **not** a
dev-dependency: `Cargo.toml`'s `[dev-dependencies]` is `tokio` and `hiss`
only, and both entries carry a comment justifying that they add no crate to
the graph. So there is no subscriber to install and no sink to read.

`Tap` cannot substitute: it yields **sealed** datagrams, so no integration
test can see a RESET_STREAM at all, let alone assert on a log line.

**This is the whole obligation, not a corner of it.** Ruling 59's entire
content is that the *receiver* — the end whose verb choice caused the
conflict, and the end that gets no error — is left evidence. The sender half
is pinned five times over in this file. The receiver half is pinned nowhere,
in any file, by any slice: it needs either a capture helper in `testutil` or
a `tracing-subscriber` dev-dependency, and both are decisions above a test
author's pay grade. **An untested MUST is not a thing to discover at the
release gate.** The same shape was reported independently for §11.5's
datagram drop-counter trace, which suggests the gap is structural to slice 6
rather than particular to §9.8.

### U-2 — the not-oldest **completion-order** case (PLAN-6 M5's integration form)

Test 4 pins FIFO among messages that are *all complete* before the first
claim, which separates a stack-ordered `unclaimed` set. It does **not**
separate a *completion-order* queue, because with everything complete the
two orders coincide. The separating fixture — a complete stream sitting
behind an incomplete one — needs a gap injected into one message's middle
while the other completes, which from `tests/` means index-arithmetic on a
wire whose packet boundaries are implementation-defined (§8.5's fill quantum;
PLAN-6 §5.0(c) forbids asserting packet counts). PLAN-6 assigns the strong
form to the core suite as **M5**, which can drive frames directly. Left
there deliberately rather than approximated here.

### U-3 — "promptly, not on the next timer" (ruling 151) is only partly separable

`PROMPT` is 1 s of virtual time. On a zero-delay virtual wire a PTO can be
tens of milliseconds, so this budget does **not** separate a build that
emits the overflow reset from its PTO path rather than inside the mutating
call. It does separate one that emits it only at the keepalive (10 s) or the
death timer (25 s). The constant's own rustdoc states this scope, so nobody
later reads the name as a stronger claim than the number supports. A sharper
pin needs the core suite, where `now` is an argument and no timer exists.

### U-4 — a lost *wakeup* is invisible to every test in this file

`LocalSet::run_until` re-polls its body on any local-task wake, so a verb
that parks without registering its waker is still re-polled and still
resolves. Nothing here can detect a missing `message_readers` registration
or a `take_all_stream_wakers` sweep that omits the new slots — which is
`CONTRACT-6.md` §2.5's explicitly-exhaustive waker table, and ruling 147's
defect verbatim. This is working rule 13's shape: the fixture bounds the
coverage, and no amount of test-writing against it closes this.

### U-5 — the reset frame's `final_size` (§9.6:3021) is unobservable

§9.6 says the receiver-emitted reset carries the receiver's highest received
offset, informational. `Tap` yields sealed datagrams, so no integration test
can read it. Ruling 59's trace is the only surface that would expose it —
see U-1.

### U-6 — the one construction whose determinism is argued rather than counted

Test 7 (`s16_without_acked_the_same_drop_set_loses_the_message`) needs the
pre-ruling-47 ordering to lose the message *deterministically*. Two
properties make it so on a conforming build: the path is healed immediately
before `close()` with no intervening await, so no PTO can fire in that
window; and 16 KiB is ≈ 15 datagrams, far more than a single PTO probe could
repair even if one did. Unlike the loss itself — which `blackholed()` counts
— this is reasoning, not a counter. If it ever flakes, the fix is a larger
payload, not a longer budget.

## 3b. Gates actually run (working rule 7 — command and output, not a claim)

The file **cannot** compile in this worktree: the message verbs do not exist
at `f19cb99`. Per the brief, no stub was written — a stub in `src/` collides
with the implementer's file and destroys its work (working rule 6, the
slice-2a accident).

**Parse and format gate:**

```
$ rustfmt --edition 2024 --check tests/story_message.rs && echo "FMT CLEAN"
FMT CLEAN
```

**Type gate, run deliberately to find *my* mistakes rather than the absent
verbs.** Every error must be one of the two verbs; anything else is a bug in
this file:

```
$ cargo build --all-features --test story_message 2>&1 | grep -E "^(error|warning)" | sort | uniq -c | sort -rn
  18 error[E0599]: no method named `recv_message` found for struct `Connection<S>` in the current scope
  14 error[E0599]: no method named `send_message` found for struct `Connection<S>` in the current scope
   1 error: could not compile `slither` (test "story_message") due to 32 previous errors
```

32 errors, 32 absent verbs, **zero warnings and no other diagnostic**. So
everything else type-checks against the shipped API as of `f19cb99`:
`Pair`/`Tap`/`Network`, `open_uni`/`accept_uni`/`write`/`read`,
`acked`/`closed`/`close`, `ConnectionLost::PeerClosed`, `WriteError::Reset`,
`MessageError::TooLarge`, both compile-time const asserts, the `arm_claim!`
statement macro and every `pin!` lifetime.

Caveat, stated because working rule 12 says to ask what a clean argument
assumed: inference downstream of a missing method is suppressed, so this
gate proves the file is *free of independent* errors, not that every
assertion around a `recv_message` result type-checks. The integrator's build
is the real gate.

## 4. Reading log

- created skeleton before first Read (working rule 2)

### The public surface my tests may touch (CONTRACT-6 §2.5)

```rust
impl<S: Handshake> Connection<S> {
    pub async fn send_message(&self, msg: &[u8]) -> Result<(), MessageError>;
    pub async fn recv_message(&self) -> Result<Vec<u8>, ConnectionLost>;
    pub fn send_datagram(&self, data: &[u8]) -> Result<(), DatagramError>; // not async
    pub async fn recv_datagram(&self) -> Result<Vec<u8>, ConnectionLost>;
}
```

`MessageError { TooLarge, ConnectionLost(..) }` — exhaustive, already landed at
`src/error.rs:264`. `MESSAGE_RECV_MAX = 262_144` (`constants.rs:297`),
`MESSAGE_OVERFLOW = 0x06` (`constants.rs:497`).

Core-side (NOT reachable from `tests/`): `core::Connection::send_message(now,
msg) -> Result<SendMessage, MessageError>`, `recv_message(now) -> Option<Vec<u8>>`,
`ConnEvent::MessageReadable`.

### Contract facts my assertions lean on

- §2.3 `recv_message`: oldest **complete unclaimed** peer-opened **uni** stream in
  open order; retires the receive half; a zero-byte payload is `Some(Vec::new())`,
  never `None`.
- §2.3: a **bidi** stream is never surfaced by `recv_message` (`unclaimed[Dir::Uni]`
  only) — §9.8:3113-3116 makes bidi the documented safe alternative.
- §2.3: dead connection + complete unclaimed stream ⇒ `Some(payload)` (Appendix B).
- §2.5 precedence for `poll_recv_message`: **core call first**, death latch only if
  the core had nothing. Parking never permitted on a dead connection.
- §2.5 `poll_send_message`: `TooLarge` checked **in the shell before the core call**.
- §0/150: `send_message` admits the whole payload or nothing; a dropped future has
  sent nothing — no stream opened, no byte admitted.
- §0/153: overflow predicate is the **highest received offset**; `send_message`
  MUST carry the FIN on its final data frame.

### Spec lines my assertions cite

- `SPEC.md:3145-3149` — the reset rule: *an **unclaimed** uni stream — neither
  claimed by `accept_uni()`, nor surfaceable by `recv_message()`, **which it
  cannot be, having no FIN** — that reaches `MESSAGE_RECV_MAX` is reset*.
- `SPEC.md:3177-3188` — the guard: the check runs *while a `recv_message()` claim
  is pending, and at the instant such a claim is made*; it applies to **every**
  unclaimed window-full stream, not merely the oldest; `accept_uni()`-claimed
  streams are untouched; a stream-mode receiver merely slow to `accept_uni()` is
  **not** reset.
- `SPEC.md:3019-3039` — §9.6's receiver-emitted reset: `final_size` carries the
  receiver's highest received offset and is informational; the sender frees its
  send half and surfaces `WriteError::Reset(error_code)`; **retained and
  regenerated until acknowledged** despite the retired half; *"no flow-control
  true-up runs in this direction"*.
- `SPEC.md:4067` — §15.3 registry: `0x06 MESSAGE_OVERFLOW`, *carried in
  RESET_STREAM's `error_code`, never in CLOSE*.
- `SPEC.md:4368-4387` — ruling 128 as amended by **152**: `recv_message` and
  `recv_datagram` drain after the death on the same terms; parking is never
  permitted on a dead connection.
- `SPEC.md:4456-4464` — `Connection::acked()` snapshots *"every byte handed to the
  connection … including the message streams §9.8 never surfaces a handle for"*;
  bytes written after the call do not extend it.
- `SPEC.md:6040-6046`, `6047-6059` — Appendix B's message obligations, five named
  assertions for the mixed-mode shape.
- `SPEC.md:6257-6265` — the message-then-close obligation, verbatim shape.
- `SPEC.md:3481-3489` — §10.6 consumption: *"message and datagram payloads stay
  accounted inside the core … until the handle takes them"*.
- `SPEC.md:4293` — *"`send_message` waits for stream allowance, then behaves per
  §9.8"* — the one spec sentence that says `send_message` can wait at all.

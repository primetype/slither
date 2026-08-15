# Slice 3a — connection core: implementation record

IMPL-A. Written incrementally as work proceeds (working rule 2).

## 1. Scope as built

Per brief: §7.1–7.2, §7.4's liveness half, §7.7–7.9, §8's codec (four
frames), §15, §16.4's poll contract on `core::Connection`, §16.5's
connection timers, §16.7 plan-seal-commit. No shell, no driver, no
handles.

Files owned: `src/core/connection/{mod,session,frame,close,timers}.rs`,
`src/core/mod.rs` (re-exports), `src/constants.rs`/`config.rs`/`error.rs`
only on a genuine gap.

## 2. Settled decisions I am building to (from the brief)

- **Ruling 81** — `Retired` fires when connection state is *dropped*.
  No linger paths (liveness, nonce exhaustion, `Replaced`, pre-session
  teardown): `Closed(_)` then `Retired` in the same drain. Closing /
  draining: `Closed(_)` at the death, `Retired` at `CloseLinger` expiry.
- **Ruling 82** — `epoch_size` is config-supplied, defaulting to
  `REKEY_EPOCH_MSGS`; documented test-only.
- **Four frames**: PADDING `0x00`, PING `0x01`, ACK `0x02`, CLOSE `0x1c`.
  ACK is codec-only in 3a. Empty-plaintext keepalive short-circuits
  before the frame parser (§3.4).
- **§7.4's liveness half in scope**: two seal paths, arming rule, install
  pin, `Liveness` timer, `ConnectionLost::TimedOut`. §7.5 stays in 7.
- Q2 — a CLOSE received while closing does not restart the linger.
- Q3 — `Closed(_)` exactly once (`debug_assert!`); closing/draining
  disarms every timer but `CloseLinger`.
- Q4 — window exposes `greatest()` and `ranges_desc()`.
- U4 — closing/draining parses the frame stream but applies nothing but
  CLOSE detection.
- U8 — `close()` before establishment: silent local teardown, no linger.

## 3. Spec lines relied on

Read targeted (working rule 1), never whole:

| Lines | Section | What I took from it |
|---|---|---|
| 640–674 | §3.4 | the 14 header bytes are the AD verbatim; **an empty plaintext is the keepalive and bypasses the frame layer** |
| 1947–1999 | §7.4 | the two seal paths; the death rule `now − last_authenticated_recv > DEAD_TIMEOUT` **and** an arming send since; arming = marking **or** ack-eliciting; not re-armed; reset by every authenticated window-fresh receive; **install pins both clocks and arms the deadline** |
| 2560–2780 | §8.1–8.5 | the frame type is itself a varint; parse-then-apply; the two failure classes; the twelve-row table; the four layouts; the packing order |
| 2789–2822 | §8.6–8.7 | per-seal bound; ack-eliciting is a per-packet property; the three retransmission classes |
| 3756–3857 | §15.1–15.4 | closing/draining; retention list; reply rule and its rate; the registry; the eight-row teardown matrix |
| 4519–4562 | §16.4 | ruling 81's amended `Retired` bullet, verbatim; generation-order normativity |
| 4564–4609 | §16.5 | the eight named timers; idempotency by stopping a timer before its logic; ruling 76's exhaustive equal-deadline order |

## 4. Decisions the plan did not settle

1. **The replay window's indexing.** §7.2 says the window is "a greatest
   authenticated counter **plus** the 2048-bit bitmap", and that "a
   counter **more than** 2048 behind the greatest" is dropped. Those two
   are only simultaneously satisfiable if the bitmap's 2048 bits cover
   offsets **1..=2048** and the greatest is tracked by the separate
   field. Bit `i` therefore stands for `greatest − (i + 1)`.
   `greatest − 2048` is accepted, `greatest − 2049` is dropped —
   which is exactly the pair the plan's T15 asks for. An implementation
   that indexes bit 0 at the greatest holds only 2047 older counters and
   fails T15's accepted side.
2. **The close-reply rate anchor.** §15.2 caps *replies* at one CLOSE per
   second and does not say whether the CLOSE that opened the closing
   state starts the clock. It does **not**: `last_reply` starts `None`,
   so the first authenticated window-fresh inbound packet after
   `close()` is always answered. The other reading makes the plan's T13
   ("ten packets within one second produce exactly one reply, and at
   least one") unsatisfiable when the ten fall inside the first second.
3. **A CLOSE arriving inside a *draining* connection** is a no-op — §15.2
   only gives the closing→draining transition. Draining emits nothing and
   already has the deadline it will die on.
4. **A seal failure during `close()`** surfaces `NonceExhausted`, not
   `LocallyClosed`, and takes the no-linger path: §7.9 makes any seal
   failure that death, and a linger whose replies cannot be sealed is
   an empty wait.
5. **`is_ack_eliciting` / `retransmission` are table-driven over all
   twelve §8.3 rows**, including the eight types slice 3a's parser cannot
   yet build (plan T3). The parser and the classifier therefore
   deliberately disagree about what is "known": the classifier answers
   for the ratified table, the parser implements four frames and treats
   the rest as §8.2's unknown type. Slices 4–6 close the gap by adding
   parse arms, not by editing the classifier.

## 5. Conflicts / defects found (spec, plan, brief)

### F1 — ruling 81's "any teardown before a session exists" names a `Retired` that cannot be constructed

The brief and §16.4 (amended) put "any teardown before a session exists"
in the **no-linger** list: `Closed(ConnectionLost)` followed by
`ToEndpoint::Retired` within the same drain. But `ToEndpoint::Retired`
carries `{ our_index: u32 }` — a **session** index — and a connection
that has never been installed has none. `Connection::connecting(sub_seed)`
is handed no index (§16.4 fixes that signature and the brief pins it
"unchanged"), and the pending's `sender_index` is endpoint-owned
bookkeeping that `connect()`/give-up/cancel already tear down.

**What I built:** a pre-session `close()` emits `Closed(LocallyClosed)`
and **no** `Retired` — there is no index route to drop. Reported rather
than resolved (working rule 3): the alternative is to widen either
`connecting()` or `ToEndpoint`, and both are ratified shapes.

This is working rule 8's shape exactly — a stated construction (the
no-linger list) with a scope the carrier cannot express.

### F2 — §7.4's death rule is strict (`>`) and its own prose says the death lands *at* the deadline

§7.4: "The connection is dead when `now − last_authenticated_recv >
DEAD_TIMEOUT`", and four lines later: "A session that receives nothing
after install **dies at install + `DEAD_TIMEOUT`**", and "a half-open
session is reaped by liveness in 25 s". Under the strict inequality the
death is at the first instant *after* `DEAD_TIMEOUT`, which is not a
representable deadline; under §16.5 an armed deadline `D` "fires no
earlier than `D`".

**What I built:** deadline `= last_authenticated_recv + DEAD_TIMEOUT`,
firing at `now >= deadline` — i.e. the prose's "dies at install +
DEAD_TIMEOUT". Working rule 3: the prose carries the intent, the formal
rule carries the off-by-one-instant. Reported, not silently chosen.

### F3 — `Connection::established` cannot pin §7.4's install clock without `now`

The brief lists `established(sub_seed, session)` as "already exists,
unchanged", and simultaneously puts §7.4's **install pin** ("both clocks
at install, deadline already armed") in slice 3a. Those cannot both hold:
the accept-path connection is born established and never receives an
`Install`, so the pin has no other instant to read, and the core may not
read a clock.

**What I built:** `established(now, sub_seed, session)`. `now` is already
in scope at the sole call site (`src/core/endpoint/staged.rs`'s `accept`,
which takes `now: Instant` per ruling 80), so the change is one word at
one site, and it makes the signature match `handle_endpoint_event(now,
ev)`. This is the one place I have knowingly departed from the brief's
"unchanged" list, per its own instruction to say so rather than build
something wrong.

### F4 — §8.1's "stated consequence" list omits CLOSE's error code

§8.1: *"Stated consequence: varints cap at 2⁶² − 1, so ACK `largest`
(§12.1) and stream offsets and final sizes (§9.5) cap there too."* That
list is not exhaustive: §16.2's `close(code: u64, reason)` takes a bare
`u64`, §15.3's registry says only "≥ `0x10` application", and a code
above 2⁶² − 1 is unencodable as a varint. `close()` returns `()`, so
there is no error path to report it through.

**What I built:** the encoder saturates such a code at `VarInt::MAX`
rather than panicking or refusing. Working rule 8's shape again — a
stated construction (the varint cap and its consequence list) with an
understated scope. A ruling could instead cap `close()`'s code at the
handle, which is the §8.4 `reason` precedent and is probably the better
answer; I did not invent that cap because it is wire-visible behaviour.

### F5 — §18.2's `slither::replay` target has no other home

Not a defect — a gap in the plan. `.slices/03-skeleton/PLAN.md` reads
§18.2 for `slither::frames` and `slither::io` and does not mention
`slither::replay` ("replay-window rejections"), which slice 3a is the
first and only slice that can emit. It is implemented in
`Session::open`, at `debug` level (§18.2 fixes the *target*, which is
operator contract; the level is not stated, and ordinary reordering past
the window's tail reaches this path on a healthy connection).

Note the three-tier drop hierarchy this completes, which is worth
stating because two of the three look alike from a caller:

| Tier | Where | Observability |
|---|---|---|
| §3.1's gate | pre-AEAD length/type/version | **silent** — no error, no trace, no counter (ruling 67) |
| AEAD failure | `decrypt_at` returns `Err` | **no target named**: a forgery and a straggler more than one epoch back are indistinguishable at the hiss surface (§7.7), so nothing can be said |
| replay window | post-AEAD `check_and_mark` | `slither::replay` |

## 6. Module-by-module notes

### `src/core/connection/timers.rs` (new, ~390 lines with tests)

§16.5's eight named timers. **`TimerKind`'s declaration order is ruling
76's equal-deadline priority** and the derived `Ord` is that priority, so
there is no second place to write the order down differently.

`take_due(now) -> Due` returns the whole due set with every member
already disarmed — which makes §16.5's *"each due timer is stopped
before its logic runs"* structural rather than a discipline each arm has
to remember, and makes a repeated `handle_timeout(now)` find an empty
set. `Loss` suppresses `Pto` **before** the disarm, so a suppressed PTO
stays armed for the next evaluation rather than being silently dropped;
that is ruling 76's "exactly one of the two fires per evaluation" as a
property of the data structure, testable with no §13 machinery at all.

Slice 3a arms two of the eight (`Liveness`, `CloseLinger`), so every
ordering assertion here is a unit test on `Timers` — the plan's T6.

### `src/core/connection/session.rs` (new, ~830 lines with tests)

§7.1/§7.2/§7.4/§7.7/§7.9. The window's offset-by-one indexing is §4.1
above. `Liveness`'s death deadline is **derived** from
`last_authenticated_recv` rather than stored, which makes §7.4's "is not
re-armed by subsequent sends" true by construction — no send can move
it, only flip the arming flag.

`seal`/`seal_quiet` are one function apart and `seal` is **unused in
slice 3a** (every seal this slice performs is a CLOSE, which §7.4 puts
in the quiet set). It is built anyway: a slice that shipped only the
quiet path would leave slice 4 to invent the marking rule from prose,
and the degenerate implementation — `seal_quiet` as an alias for `seal`
— is invisible to every possible slice-3 flow test.

`seal_inner` is §16.7's three phases in one function with the phases
marked: **plan** (`next_counter`, then the header, mutating nothing),
**seal**, **commit** (§7.4's clock update — the whole commit list this
slice has; slices 4–6 add the dequeue, the pending-ACK clear, `on_sent`
and the recovery timers at that marked point).

The `transport_tests` module runs a **real IK handshake inline** and
splits it into two `EstablishedSession`s, so the seal/open plumbing is
tested against genuine hiss cipher states rather than a mock: the AD
wiring, the counter in the header, out-of-order opening, the forgery
case, and §3.4's 30-byte keepalive shape.

### `src/core/connection/frame.rs` (new, ~880 lines with tests)

Four frames; twelve-row classifiers (§4.5 above). The parse cursor is
the **one** place an overrun check is written, so no per-frame parser
rewrites one and gets it wrong once.

`Packing` expresses §8.5's order as stages that can only run forwards
(`debug_assert`ed), with the gap where slice 4's STREAM fill and slice
6's DATAGRAM fill insert a stage rather than rewriting the order.

Two §8.2 structural cases are **not** reachable in slice 3a and so have
no variant: "a non-final extends-to-end frame" needs ¬LEN STREAM or
`0x30` DATAGRAM (slices 4 and 6), and MAX_STREAMS' `max > 2⁶⁰` needs
that frame (slice 5). Named here so the omission is a boundary, not a
miss.

One thing worth knowing for TEST-A and slices 4+: `0x7f` is **not** a
one-byte unknown type — it is the first byte of a two-byte varint
(prefix `01`), so alone it is `VarintOverrun`, not `UnknownType`. The
one-byte unassigned codes are `0x03`, `0x06`, `0x07`, `0x14`–`0x1b` and
`0x1d`–`0x3f`.

### `src/core/connection/close.rs` (new, ~230 lines with tests)

§15's three post-mortem states and the reply rule. `Closing` owns the
CLOSE it sent, because every reply is that same frame verbatim.

### `src/core/connection/mod.rs` (138 → ~560 lines)

The receive path is the one order §7.2 admits, and the post-mortem path
is separated from the live one so that §15.2's "apply nothing but CLOSE
detection" cannot drift into "apply everything whose state still
happens to exist".

`ConnEvent` **lost its `Copy` derive**: `ConnectionLost::PeerClosed`
carries the peer's reason phrase as `Vec<u8>` (§8.4 carries it as
bytes). `ConnOutput` was never `Copy`. Nothing in the crate relied on
either.

### Files outside the brief's ownership list that I had to touch

| File | Change | Why |
|---|---|---|
| `src/packet/handshake.rs` | +3 trait methods: `next_counter`, `seal`, `open` | `Handshake::Seal`/`Open` were **opaque associated types with no bounds and no operations**, so the connection core could not call `encrypt_next`/`decrypt_at` on them at all. hiss exposes no trait to bound them with. The plan and `HISS-API.md` both assume `session.seal.next_counter()`, which does not compile through a generic `C::Seal`. |
| `src/packet/suite.rs` | the three `impl`s in the `channel!` expansion | same; mechanical, three one-line forwards |
| `src/config.rs` | `epoch_size` field, `DEFAULT_EPOCH_SIZE`, `with_epoch_size`, accessor, 2 tests | ruling 82, as instructed |
| `src/core/endpoint/mod.rs` | `fn epoch_size()` → `fn epoch_size(&self)` reading `Config`; one call site | ruling 82 |
| `src/core/endpoint/staged.rs` | one call site; `Connection::established(now, …)` | ruling 82; F3 |
| `src/varint.rs` | removed the module-wide `#![allow(dead_code)]`, narrowed to `from_const` | the file's own comment says "Remove this when the frame codec lands." It has landed; `from_const` waits for the first varint *literal* on the wire (slices 4–5) and now says so on its own line |
| `src/core/mod.rs` | re-export `TimerKind` | so the driver names ruling 76's order from one place |

**Not touched:** `src/core/tests.rs`, `src/core/connection/tests.rs`
(never created), anything under `tests/`, `SPEC.md`, `PLAN.md`,
`STORIES.md`, `rulings.md`. No `#[cfg(test)] mod tests;` was added to
`src/core/connection/mod.rs`.

### U3 needed no invention

The plan's U3 proposed introducing §15.3's registry "as a Rust type
(`CloseCode`, or `pub const ERROR_*: u64`)". The second form **already
exists** in `src/constants.rs` (`NO_ERROR` … `MESSAGE_OVERFLOW`,
`APPLICATION_ERROR_BASE`), with `tests/spec_errors.rs` pinning them, so
nothing new was added. The code path uses `constants::PROTOCOL_VIOLATION`.

## 7. Gate output

Run on the working tree at hand-off. Baseline at `750ad4b` was 263
tests (141 + 103 + 11 + 4 + 4), confirmed by `git stash` + `cargo test`;
the tree is now **325** (203 + 103 + 11 + 4 + 4) — **+62, no existing
test removed or changed**.

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.71s

$ cargo fmt --all --check
(no diff)

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated .../target/doc/slither/index.html

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated .../target/doc/slither/index.html

$ cargo test
test result: ok. 203 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 103 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.  11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.   4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.   4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test --all-features
test result: ok. 203 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 103 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.  11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.   4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok.   4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo +1.96 check --all-features --all-targets      # MSRV, not required of me
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.94s

$ cargo deny check                                     # supply chain, unchanged
advisories ok, bans ok, licenses ok, sources ok
```

## 8. End-to-end verification, and what was deliberately not kept

Working rule 6 gives the story- and spec-level acceptance tests to
TEST-A, so none are in this tree. To avoid handing over code whose
wiring had never been exercised, I wrote three temporary two-`Connection`
checks, ran them, and **deleted them**: a local `close()` round trip
(Transmit → `Closed(LocallyClosed)`; the peer surfaces
`PeerClosed { 0x2a, "bye" }`; `Retired { our_index }` at the linger
expiry, then `Timeout(None)`), the linger's reply rule and the liveness
death at exactly `install + DEAD_TIMEOUT`, and a `[valid CLOSE][unknown
type]` packet surfacing `ProtocolViolation` rather than `PeerClosed`.
All three passed. They are TEST-A's to write independently, and their
absence here is deliberate.

**One process hazard I hit, worth a working rule.** To confirm the 263
baseline I ran `git stash push -u` / `git stash pop`. `-u` stashes
**untracked** files — which, in a parallel-agent slice, includes the
other agent's brand-new file. TEST-A's `src/core/connection/tests.rs`
(3 099 lines) was in the tree by then and was moved out and back over a
~2 s window. It came back intact (123 868 bytes, same content), and
`pop` refuses to clobber a file recreated in the meantime, so the
failure mode is a loud conflict rather than a silent loss. But this is
CLAUDE.md rule 10's hazard wearing a different hat: rule 10 warns about
`git checkout <file>` discarding uncommitted work, and `git stash -u`
reaches *further* — into files the running agent does not own and did
not write. Measure a baseline in a separate clone or a `git worktree`,
not by stashing the shared tree.

The permanent tests in this tree are unit tests of my own plumbing: the
bitmap and its edges, the varint/frame codec and its boundaries, the
classifiers over all twelve §8.3 rows, the timer order, the reply rate,
and the seal/open round trip against real hiss cipher states.

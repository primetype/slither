# Round 44 — Writer W3 report

**Base commit:** `721167a`, verified as the first command (rule 14):

```
$ git -C /Users/nicolasdiprima/work/primetype/slither log -1 --oneline
721167a Round 43: the SECV5 pins land — the one-shot keepalive, the spent-packet separator, the 272 pin
```

**Exclusive paths (rule 6), all seven written:** `src/identity.rs`,
`src/shell/mod.rs`, `src/shell/connection.rs`, `src/shell/endpoint.rs`,
`src/shell/staged.rs`, `src/shell/stream.rs`, `src/packet/suite.rs`, plus
this report.

**Never touched:** `README.md`, `docs/`, `examples/echo.rs`, `src/lib.rs`,
`Cargo.toml`. No `git add`, no commit. See deviation D4 for the one command
that could have crossed the line and the check that shows it did not.

---

## Task 1 — §5's doc examples

All four new fences compile **featureless**; two of them **run**. No fence
touches `testutil`. Every one uses only `slither`, `hiss`, `rand_chacha`,
`getrandom` and `tokio` — all normal `[dependencies]`.

| §5 row | Landed | Fence | Where |
|---|---|---|---|
| `Connection::send_message` | ✅ | `no_run` | `src/shell/connection.rs`, `# Example` after the `send_message → acked → close` idiom sentence |
| `Connection::recv_message` | ✅ | none | one sentence + `[send_message](Self::send_message)` link |
| `SoftwareIdentity::generate` | ✅ | **plain (runs)** | `src/identity.rs`, `# Example` on `generate` |
| `Identity::public_static` | ✅ | **plain (runs)** | `src/identity.rs`, `# Example` on the trait method |
| `Endpoint::connect` | ✅ | `no_run` | `src/shell/endpoint.rs`, `# Example` immediately after the summary line; **all** existing prose and the `text` cancel-then-redial diagram untouched |
| `Connecting` | ✅ | none | one sentence + `[Endpoint::connect]` link |
| `src/identity.rs` module `//!` | ✅ | — | §5's exact rewrite; `§16.4` citation demoted to sentence three |
| `src/shell/mod.rs` module `//!` | ✅ | — | §5's exact rewrite as the opening, composed with the §8(i) fold |
| `src/packet/suite.rs` `toml` fence | ✅ | — | now `hiss = { version = "0.3", default-features = false }` |

Notes on how the examples were derived:

- **The `send_message` pair is `examples/echo.rs` compressed.** Identity,
  socket and address setup are `#`-hidden; what shows is the two `build()`
  calls, the answerer's four-rung ladder with its per-rung DH annotations,
  and the dialler's `connect → send_message → acked → close`.
- **`acked()` appears only on the sending side**, per the echo writer's
  finding. I went one step further and made the answerer hold the session
  open with `let _ = conn.closed().await;` instead of letting the task end.
  Dropping the answerer's `Connection` there sends a CLOSE that can beat the
  acknowledgement the dialler is waiting on, so the shown code would have
  taught a race even though `no_run` never executes it. The comment on that
  line says exactly that.
- **`public_static` uses `*`, not `.clone()`** — matching `examples/echo.rs`'s
  `let answerer_key = *answerer.public_static();`. The P-256 public key is
  `Copy`. The example's comment says the value crosses an out-of-band channel
  to the peer's `connect()`, and that slither never learns a static from the
  wire.
- **`generate`'s fence finishes the recipe** the type-level note at
  `identity.rs:189` leaves as `// …then SoftwareIdentity::<MySuite>::generate(rng)`,
  including the `channel!` invocation that supplies `MySuite`. It runs, so it
  exercises real keygen.
- **The `toml` fence was restructured rather than extended.** I first wrote
  the `default-features = false` rationale as `#`-prefixed lines *inside* the
  fence, then backed it out: rustdoc's `#`-line hiding is a Rust-code-block
  behaviour, and betting a rendered page on how it treats a ```toml``` block
  is not worth it. The rationale is now prose immediately below the fence and
  the fence carries a one-line trailing comment. Nothing about the fix
  changed.

## Task 2 — §8(i), the fold (RATIFIED)

`src/shell/mod.rs`'s public `//!` now carries, in this order:

1. §5's new first sentence, with `SPEC.md §16` demoted to sentence two.
2. The unchanged sans-io/actor paragraph and the `Endpoint`/`accept` shape
   diagram.
3. **`# The four things in this module`** — `endpoint.rs`'s orientation
   paragraph *verbatim*, the staged trio, `connection.rs`'s verb inventory
   (expanded: the four stream verbs and the four sugar verbs are now named
   individually), and the three stream handles.
4. **`# A LocalSet is required`** — unchanged, plus one clause for the new
   guard.
5. **`# The staged accept ladder — §6.2`** — `staged.rs`'s prose and its
   DH-annotated `text` diagram, moved *verbatim*, with
   `## Dropping is the rejection` and `## A staged object is not a handle`
   demoted from `#` to `##`.
6. **`# The seam, in one paragraph`** — the existing endpoint half unchanged;
   the second paragraph now folds `connection.rs`'s and `stream.rs`'s
   "Where the work happens" mechanism into one statement covering both.
7. **`# What is not here`** — the composability layer, pointing at
   `crate::compat`.

The six `mod` declarations stay **private**. `pub mod wire;` is unchanged.
No `pub` keyword was added anywhere.

Each source file keeps a short internal-facing `//!` with a
`[the shell module docs](super)` pointer, plus whatever is genuinely
implementation-facing:

| File | What stays | Lines |
|---|---|---|
| `staged.rs` | pointer + the `Command`/`oneshot` round-trip mechanics and stage consumption | 10 |
| `endpoint.rs` | pointer + "thin client of §16.3: command-channel verbs, `connect()`'s synchronous 0-DH half, the builder that spawns the driver" | 7 |
| `connection.rs` | pointer + ruling 264's marker + the exact `poll_*(&self, cx, …) -> Poll<_>` signature | 10 |
| `stream.rs` | pointer + the exact `poll_*(&mut self, cx, …)` signature + **the waker-key-is-a-field rationale** + `SendStream::acked`'s `ConnEvent::StreamFinished` / §9.7 `DataRecvd` chain + the `AsyncRead`/`AsyncWrite` link definitions | 20 |

### Fact-preservation audit — "nothing may be lost"

I dumped all four `//!` blocks from `721167a` with `git show` and checked
every clause against the new text. Every fact survives except one class,
which I dropped **deliberately and am flagging** (deviation D1).

| Fact (source) | Survives at |
|---|---|
| ladder `Intro → Claimed → Proven → Connection` (staged) | mod.rs, prose + diagram |
| "one DH at a time … claimed identity before spending a second DH" (staged) | mod.rs, verbatim |
| "each stage's `async` verb is a driver round-trip (§6.2, §16.3, ruling 53)" (staged) | mod.rs, verbatim |
| the DH-annotated `text` diagram incl. §17.1's guard annotation (staged) | mod.rs, verbatim |
| Dropping is the rejection: §6.1, ruling 48, no `reject()` verb (staged) | mod.rs, verbatim |
| Not a handle: §16.3 quote, `EndpointDropped`, why `ConnectError` lacks it, ruling 62 (staged) | mod.rs, verbatim |
| "one socket's worth … thin client over the driver (§16.3) … no second copy of the cores' state" (endpoint) | mod.rs, verbatim |
| the verb inventory + "§16.2's surface is complete" (connection) | mod.rs, **expanded** |
| composability layer wraps rather than adds; `[corrected 2026/08/18 — ruling 264]` (connection) | mod.rs (`# What is not here`) + the marker in connection.rs |
| shared-cell mechanism: borrow, sans-io call, seals synchronously §16.7/ruling 114, mark dirty, wake, drain to `Timeout`, do I/O (connection + stream) | mod.rs, one merged paragraph |
| "nothing here is a driver round-trip **and nothing here awaits**" (stream) | mod.rs — *this clause was lost in my first draft and restored*; see below |
| written once as `poll_*`; `async fn` is `poll_fn`; `AsyncWrite` is the same fn with its error mapped (connection + stream) | mod.rs + both files' exact signatures |
| waker key is a **field**, because `poll_write` has no argument to carry a slot (stream) | stream.rs — genuinely internal |
| "absent rather than stubbed: an unimplemented verb is a claim about the protocol" (stream + mod) | mod.rs, `# What is not here` |
| ruling 96 (`AsyncRead`/`AsyncWrite`) | mod.rs |
| ruling 122b, `ConnEvent::StreamFinished`, §9.7 `DataRecvd`, §12 ACK processing | stream.rs |
| the two docs.rs link definitions | mod.rs and stream.rs |

**The one clause I lost and caught.** My first fold rendered stream.rs's
*"Nothing here is a driver round-trip and nothing here awaits"* as
*"Nothing on that side is a driver round-trip"* — dropping the second half of
a sentence while editing the first. That is working rule 4(a) exactly, in the
one artefact this round is about. The audit above is what found it; the
sentence now reads *"Nothing on that side is a driver round-trip, and the
stream verbs do not await at all."*

## Task 3 — §8(ii), the LocalSet panic guard (RATIFIED)

**The guard**, at what was `src/shell/endpoint.rs:524`:

```rust
let driver = Driver::new(wire, shell, commands).run();
let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
    tokio::task::spawn_local(driver);
}));
if spawned.is_err() {
    panic!("slither: Endpoint::builder()…build() was called outside a …");
}
```

`#![forbid(unsafe_code)]` untouched — `catch_unwind` and `AssertUnwindSafe`
are both safe. The comment above it records why `catch_unwind` is the only
available mechanism (§8(ii)'s caveat (a): a `LocalSet` runs on a multi-thread
runtime via `run_until`, so `Handle::current().runtime_flavor()` would
falsely panic on a valid configuration).

**The message, as actually emitted** (captured with `--nocapture`, so this is
the rendered text, not the literal):

```
slither: Endpoint::builder()…build() was called outside a `tokio::task::LocalSet`.
The driver is a single `!Send` task (a DH provider and a Wire are not required to
be `Send`), so it is spawned with `tokio::task::spawn_local`, which needs a
current-thread runtime with a `LocalSet` — `#[tokio::main]` alone is not one.
Wrap your code in `slither::block_on(async { … })`, or build the endpoint inside
`tokio::task::LocalSet::new().run_until(…)`.
```

It names `slither::block_on`, names `tokio::task::LocalSet`, states the cause,
and gives both fixes.

**Docs updated.** `build()`'s `# Panics` section (was `:486`) gains a
paragraph saying the `LocalSet` case now panics with slither's own message,
naming both fixes, plus the one required sentence:

> Under `panic = "abort"` the guard cannot intercept — `catch_unwind` does not
> catch an aborting panic — so that profile still shows tokio's raw
> *"`spawn_local` called from outside of a `task::LocalSet`"*.

`EndpointBuilder`'s type doc (was `:394`) had the same claim in its
`# A LocalSet is required` section; it now says the panic is slither's and
points at `build()`'s panics section. `shell/mod.rs`'s own
`# A LocalSet is required` gained one clause to match. Leaving any of the
three saying only "panics" would have been rule 4's shape.

**The pin (rule 9).**
`shell::endpoint::tests::build_outside_a_localset_panics_with_slithers_own_message`
— a plain `#[test]` in a new `#[cfg(test)] mod tests` at the foot of
`src/shell/endpoint.rs` (a file I own; `shell/mod.rs`'s existing block is
`testutil`-based and paused-clock, and this test wants neither). It builds a
bare current-thread runtime with **no** `LocalSet` — which is what
`#[tokio::main]` hands a consumer — binds a real `UdpSocket`, calls
`Endpoint::builder().identity(…).wire(…).build()` inside `rt.block_on`,
catches the unwind, downcasts the payload, and asserts on three substrings.
Self-contained: `SoftwareIdentity` + `crate::packet::ReferenceSuite`, no
`testutil`, so it runs featureless.

**Rule 9's separating statement — what would turn it red** (stated, *not*
mutated; nothing was edited to produce this):

Tokio's own panic text is
*"`spawn_local` called from outside of a `task::LocalSet` or
`runtime::LocalRuntime`"*. It contains **none** of `slither::block_on`,
`tokio::task::LocalSet`, or `Endpoint::builder`. Therefore:

1. **Deleting the guard** (restoring the bare
   `tokio::task::spawn_local(Driver::new(…).run());`) still panics, so a test
   asserting only "it panics" would stay green — but all three `contains`
   assertions fail, because the payload becomes tokio's `&'static str`.
2. **Keeping the guard and weakening the message** — dropping the
   `slither::block_on(async { … })` clause, or rewording
   `tokio::task::LocalSet` to a bare "LocalSet", or renaming
   `Endpoint::builder()…build()` to "build()" — fails the corresponding
   assertion individually. Each of the three is independently separating.
3. **Catching but swallowing** (`let _ = catch_unwind(…)` with no re-panic)
   fails at `.expect_err("build() outside a LocalSet must panic")`.

The degenerate implementation the bound must exclude is *"it panics with
something"*, and each assertion is chosen so that version fails.

**One thing the guard does not do, recorded rather than fixed.** The default
panic hook prints tokio's message *before* slither's, so a consumer sees both
lines. Suppressing it needs `std::panic::set_hook`, which is process-global
and would race any concurrent panic in another thread — an unacceptable
side-effect for a library constructor. Two lines where one would do, with the
useful one last and carrying the backtrace, is the better trade. The `-- --nocapture`
output above shows exactly what a consumer sees.

---

## Verification (rule 7 — commands and output)

### 1. Doctests, featureless and all-features

```
$ cargo test --doc
running 16 tests
test src/shell/endpoint.rs - shell::endpoint::EndpointBuilder (line 454) - compile ... ok
test src/compat/rt.rs - compat::rt::block_on (line 42) - compile ... ok
test src/lib.rs - (line 175) - compile ... ok
test src/identity.rs - identity::SoftwareIdentity (line 223) - compile ... ok
test src/shell/connection.rs - shell::connection::Connection<S>::notified (line 256) - compile ... ok
test src/lib.rs - (line 25) - compile ... ok
test src/shell/connection.rs - shell::connection::Connection<S>::send_message (line 707) - compile ... ok
test src/shell/endpoint.rs - shell::endpoint::Endpoint<I>::connect (line 177) - compile ... ok
test src/packet/suite.rs - packet::suite::IK (line 280) ... ok
test src/packet/suite.rs - packet::suite::IK (line 316) ... ok
test src/packet/suite.rs - packet::suite::IK (line 248) ... ok
test src/config.rs - config::Config::with_flow_windows (line 270) ... ok
test src/compat/io.rs - compat::io::io::Error (line 105) ... ok
test src/packet/suite.rs - packet::suite::channel (line 177) ... ok
test src/identity.rs - identity::SoftwareIdentity<S,R>::generate (line 294) ... ok
test src/identity.rs - identity::Identity::public_static (line 118) ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.64s
```

My four are all present: `send_message` and `connect` as `- compile` (the
`no_run` pair), `generate` and `public_static` with no marker (**they ran**).

```
$ cargo test --doc --all-features
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.71s
```

**On §10.2's arithmetic.** §10.2 predicted 19–20 from a 14 baseline. The
featureless run is at **16**, and that is not a shortfall: §10.2's 14 was
measured on the clean `721167a` tree, whereas this tree also holds W2's
in-flight `src/lib.rs`, which shows **two** fences (`line 25`, `line 175`)
where the baseline note assumed one plus a quickstart. My own contribution is
exactly **+4**, and the four are named in the listing above. The integrator
should re-derive the total on the merged tree rather than against 14.

### 2. The guard pin, featureless

```
$ cargo test build_outside_a_localset
running 1 test
test shell::endpoint::tests::build_outside_a_localset_panics_with_slithers_own_message ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 779 filtered out; finished in 0.01s
```

The separating statement is in Task 3 above. No mutation was performed
(project rule 10: mutating means `git checkout <file>`, and this tree holds
three other writers' uncommitted work).

### 3. Docs gate

```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.07s
   Generated .../target/doc/slither/index.html

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.13s
   Generated .../target/doc/slither/index.html
```

Zero warnings, so every new intra-doc link resolves — including the four
`[the shell module docs](super)` pointers and `[crate::compat]`,
`[crate::compat::io]`, `[crate::block_on]`.

### 4. Clippy

```
$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.55s
```

### 5. Format

```
$ cargo fmt --check
(no output — no diff)
```

### 6. The `hiss` staleness grep

```
$ grep -n 'hiss = "0.3"$' src/packet/suite.rs
(no output; exit 1 — empty)
```

### 7. Full suite

```
$ cargo test --all-features 2>&1 | tail -5
test src/identity.rs - identity::Identity::public_static (line 118) ... ok
test src/testutil/mod.rs - testutil::FlakyPolicy::fail_sends (line 267) ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.73s

all doctests ran in 5.40s; merged doctests compilation took 0.66s
```

Because `tail -5` shows only the last target, here is the whole run reduced:

```
$ cargo test --all-features 2>&1 | grep -c 'test result: FAILED'
0
$ cargo test 2>&1 | grep -c 'test result: FAILED'     # featureless
0
```

The lib unit-test target reports **780 passed** (779 at base + my one new
pin). Both the fold and the guard touched live code paths and nothing moved.

Also run, since the fold and the guard are code changes:

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.76s
```

---

## Deviations and findings

### D1 — I declined to carry forward a stale claim, and I am flagging it rather than treating it as settled

`shell/mod.rs`'s old `# What is here, and what is not` and `stream.rs`'s
`# What is not here` both say the composability layer is **not built**:

> *"the `AsyncRead`/`AsyncWrite` impls are slice 8's (ruling 96)"* — `mod.rs`
> *"[`AsyncRead`]/[`AsyncWrite`] are slice 8 (ruling 96) — **absent rather than stubbed**"* — `stream.rs`

**Checked against the code, not inferred** (rule 11): `src/compat/io.rs` is
declared `pub mod io;` in `src/compat/mod.rs` with **no feature gate**, and
carries `impl AsyncWrite for SendStream<S>` (`:245`),
`impl AsyncRead for RecvStream<S>` (`:279`), `impl AsyncRead for BiStream<S>`
(`:296`) and `impl AsyncWrite for BiStream<S>` (`:313`). I also verified the
rest of that inventory exists: `notified` (`connection.rs:286`),
`set_persistent_keepalive` (`:378`), `persistent_keepalive` (`:423`),
`send_datagram` (`:870`), `recv_datagram` (`:914`), `Connection::acked`
(`:473`), `SendStream::acked` (`stream.rs:441`).

So both sentences are false today, and `connection.rs`'s own `//!` — corrected
on **2026/08/18 by ruling 264** — already says the opposite in the same
module tree. That is rule 4's shape: ruling 264 fixed one file's clause and
did not sweep the two neighbours making the same claim.

§8(i) told me to move the prose and lose nothing. Publishing a known-false
sentence into the crate's most-read public module doc is worse than the
alternative, and rule 5 says to say so rather than do it. **What I did:** the
*design principle* survives verbatim ("an unimplemented verb is a claim about
the protocol, so a verb slither does not implement is absent rather than
present and returning an error"), ruling 96 and ruling 122b are both still
cited, and the layer is now described as living in `crate::compat` and
wrapping these verbs — which is what `connection.rs` has said since ruling
264. **What I dropped:** the slice-schedule framing itself — "Slice 3 builds
…, Slice 4 adds …", "is slice 5's", "is slice 8's". All ten slices are done;
that text is build history, not orientation.

**This is a judgement call inside a ratified instruction and the integrator
should confirm it**, not inherit it. If the maintainer wants the slice
provenance preserved, it belongs in the ruling record rather than in a public
`//!`, and I can restore it verbatim in one edit.

### D2 — a second stale-shape survivor I did **not** touch, because it is not mine

`src/compat/rt.rs:19-20` says `block_on` exists because
`spawn_local` *"panics outside a `LocalSet`"* — true, but as of today the
panic is slither's own message, which that file does not mention.
`src/compat/` is outside my seven paths (rule 6), so I left it. **One
sentence for the integrator**, in the file that most needs it: `rt.rs`'s
`# Panics` section is where a consumer who has already found `block_on` will
look.

### D3 — the type-level `no_run` recipe in `identity.rs` now overlaps `generate`'s new fence

`identity.rs`'s `# r must be seeded from OS entropy in production` note keeps
its `no_run` fence, whose last line is
`// …then SoftwareIdentity::<MySuite>::generate(rng)`. `generate`'s new fence
is that recipe finished. §5 asked me to finish the recipe and did **not** ask
me to remove the stub, and §10.2's expected doctest count assumes it stays, so
I left it and adjusted the cross-reference wording only ("for the two-line
recipe **and the argument behind it**"). It is a mild instance of §3.2's
one-home rule; flagged, not acted on.

### D4 — `cargo fmt --all` ran once on a shared tree; verified it touched nothing of anyone else's

My new test tripped `cargo fmt --check`, and I fixed it with
`cargo fmt --all` — which formats **every** file in the package, including
`src/lib.rs` while W2 holds it. That was careless. It caused no damage and I
verified rather than assumed:

```
$ stat -f '%Sm %N' -t '%H:%M:%S' src/lib.rs README.md src/shell/endpoint.rs
07:31:06 src/lib.rs
07:21:37 README.md
07:39:11 src/shell/endpoint.rs
```

`rustfmt` rewrites only files it changes. `src/shell/endpoint.rs` carries the
07:39 timestamp of the `fmt` run; `src/lib.rs` (07:31) and `README.md` (07:21)
both predate it, so neither was rewritten. There is no `rustfmt.toml`, so
defaults apply and doc comments are not reformatted at all — which is why a
doc-only edit in `lib.rs` had nothing for `fmt` to do. Every later check used
the read-only `cargo fmt --check`.

### D5 — placement choice on `Endpoint::connect`

§5 says "the fence is added, nothing is removed", and nothing was. I put
`# Example` **immediately after the one-line summary**, ahead of the "Not
`async`" rationale and the S5 warning, rather than at the end. `connect`'s doc
is ~50 lines of rationale before the new reader reaches anything they can
copy; the example first is the whole point of C5. All existing prose,
including the `text` cancel-then-redial diagram, is intact and in its original
order below it.

---

## `git diff --stat` over my seven files

```
$ git diff --stat -- src/identity.rs src/shell/mod.rs src/shell/connection.rs \
      src/shell/endpoint.rs src/shell/staged.rs src/shell/stream.rs src/packet/suite.rs
 src/identity.rs         |  69 ++++++++++++++++++-
 src/packet/suite.rs     |   7 +-
 src/shell/connection.rs |  93 +++++++++++++++++++++-----
 src/shell/endpoint.rs   | 173 +++++++++++++++++++++++++++++++++++++++++++++---
 src/shell/mod.rs        | 109 ++++++++++++++++++++++++------
 src/shell/staged.rs     |  38 +++--------
 src/shell/stream.rs     |  35 ++++------
 7 files changed, 422 insertions(+), 102 deletions(-)
```

The three net-negative files are the fold's donors (`staged.rs`,
`stream.rs`, and `connection.rs`'s header); their text is in `shell/mod.rs`.
